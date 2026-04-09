use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

use super::command_runner::{build_command, shell_command};
use super::{resolve_tool_working_dir, schema_object, Tool, ToolResult};
use crate::claude::ToolDefinition;

/// Search mode: native (embedding + ChromaDB HTTP) or command (run vault_search_command).
#[derive(Clone)]
pub enum SearchVaultMode {
    Native {
        embedding_url: String,
        vector_db_url: String,
        collection: String,
        http_client: reqwest::Client,
    },
    Command {
        vault_search_command: String,
        working_dir: PathBuf,
    },
}

pub struct SearchVaultTool {
    mode: SearchVaultMode,
}

impl SearchVaultTool {
    /// Native mode: call embedding server + ChromaDB HTTP API. Requires both to be running.
    pub fn new_native(embedding_url: &str, vector_db_url: &str, collection: &str) -> Self {
        Self {
            mode: SearchVaultMode::Native {
                embedding_url: embedding_url.trim_end_matches('/').to_string(),
                vector_db_url: vector_db_url.trim_end_matches('/').to_string(),
                collection: collection.to_string(),
                http_client: reqwest::Client::new(),
            },
        }
    }

    /// Command mode: run vault_search_command with {query} substituted. No ChromaDB server needed.
    pub fn new_command(vault_search_command: &str, working_dir: &str) -> Self {
        Self {
            mode: SearchVaultMode::Command {
                vault_search_command: vault_search_command.to_string(),
                working_dir: PathBuf::from(working_dir),
            },
        }
    }

    /// Legacy constructor for native mode (backwards compatible).
    pub fn new(embedding_url: &str, vector_db_url: &str, collection: &str) -> Self {
        Self::new_native(embedding_url, vector_db_url, collection)
    }

    /// Command mode: run vault_search_command with {query} substituted.
    async fn execute_command_mode(
        &self,
        vault_search_command: &str,
        working_dir: &PathBuf,
        query: &str,
    ) -> ToolResult {
        // Substitute {query} in the command (support both {query} and {query:shell} if needed)
        let command = vault_search_command.replace("{query}", query);

        let working_dir_resolved = resolve_tool_working_dir(working_dir);
        if let Err(e) = tokio::fs::create_dir_all(&working_dir_resolved).await {
            return ToolResult::error(format!(
                "Failed to create working directory {}: {e}",
                working_dir_resolved.display()
            ));
        }

        let spec = shell_command(&command);
        let timeout_secs = 60u64;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            build_command(&spec, Some(&working_dir_resolved)).output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let mut result_text = String::new();
                if !stdout.is_empty() {
                    result_text.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !result_text.is_empty() {
                        result_text.push('\n');
                    }
                    result_text.push_str("STDERR:\n");
                    result_text.push_str(&stderr);
                }
                if result_text.is_empty() {
                    result_text = format!("Command completed with exit code {exit_code}");
                }

                if result_text.len() > 30000 {
                    result_text.truncate(30000);
                    result_text.push_str("\n... (output truncated)");
                }

                if exit_code == 0 {
                    ToolResult::success(result_text).with_status_code(exit_code)
                } else {
                    ToolResult::error(format!("Exit code {exit_code}\n{result_text}"))
                        .with_status_code(exit_code)
                        .with_error_type("process_exit")
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Failed to execute vault search command: {e}"))
                .with_error_type("spawn_error"),
            Err(_) => ToolResult::error(format!(
                "Vault search command timed out after {timeout_secs} seconds"
            ))
            .with_error_type("timeout"),
        }
    }
}

#[async_trait]
impl Tool for SearchVaultTool {
    fn name(&self) -> &str {
        "search_vault"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_vault".into(),
            description: "Semantically search the ORIGIN vault (Obsidian notes, documents) using vector similarity. Use this to find relevant knowledge base entries. This searches the vault knowledge base, NOT conversation history — use search_chat_history for that.".into(),
            input_schema: schema_object(
                json!({
                    "query": {
                        "type": "string",
                        "description": "Natural language query to search for in the vault"
                    },
                    "n_results": {
                        "type": "integer",
                        "description": "Number of results to return (default: 5, max: 20)"
                    }
                }),
                &["query"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => q.to_string(),
            _ => return ToolResult::error("Missing or empty 'query' parameter".into()),
        };

        match &self.mode {
            SearchVaultMode::Command {
                vault_search_command,
                working_dir,
            } => {
                return self
                    .execute_command_mode(vault_search_command, working_dir, &query)
                    .await;
            }
            SearchVaultMode::Native { .. } => {
                // Fall through to native implementation below
            }
        }

        let n_results = input
            .get("n_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20) as usize;

        let SearchVaultMode::Native {
            embedding_url,
            vector_db_url,
            collection,
            http_client,
        } = &self.mode
        else {
            unreachable!()
        };

        // Step 1: Get embedding from embedding server
        let embed_resp = match http_client
            .post(format!("{}/embedding", embedding_url))
            .json(&json!({"content": query}))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Embedding server unreachable: {e}")),
        };

        if !embed_resp.status().is_success() {
            let status = embed_resp.status();
            let body = embed_resp.text().await.unwrap_or_default();
            return ToolResult::error(format!("Embedding server returned {status}: {body}"));
        }

        let embed_json: serde_json::Value = match embed_resp.json().await {
            Ok(j) => j,
            Err(e) => return ToolResult::error(format!("Failed to parse embedding response: {e}")),
        };

        // Handle both llama.cpp formats:
        //   {"embedding": [[0.1, 0.2, ...]]}  — list of embedding vectors
        //   {"embedding": [0.1, 0.2, ...]}     — single flat vector
        let embedding: Vec<serde_json::Value> =
            if let Some(outer) = embed_json.get("embedding").and_then(|v| v.as_array()) {
                if outer.first().and_then(|v| v.as_array()).is_some() {
                    // Nested: [[...]] — take first vector
                    outer
                        .first()
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default()
                } else {
                    // Flat: [...] — use directly
                    outer.clone()
                }
            } else {
                return ToolResult::error(
                    "Unexpected embedding response format (missing 'embedding' field)".into(),
                );
            };

        if embedding.is_empty() {
            return ToolResult::error("Embedding server returned empty embedding vector".into());
        }

        // Step 2: Get ChromaDB collection ID
        let col_resp = match http_client
            .get(format!(
                "{}/api/v1/collections/{}",
                vector_db_url, collection
            ))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ToolResult::error(format!(
                    "ChromaDB unreachable when fetching collection: {e}"
                ))
            }
        };

        if !col_resp.status().is_success() {
            let status = col_resp.status();
            let body = col_resp.text().await.unwrap_or_default();
            return ToolResult::error(format!(
                "ChromaDB collection '{}' not found ({status}): {body}",
                collection
            ));
        }

        let col_json: serde_json::Value = match col_resp.json().await {
            Ok(j) => j,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to parse ChromaDB collection response: {e}"
                ))
            }
        };

        let collection_id = match col_json.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return ToolResult::error(
                    "Could not find collection ID in ChromaDB response".into(),
                )
            }
        };

        // Step 3: Query ChromaDB with the embedding
        let query_resp = match http_client
            .post(format!(
                "{}/api/v1/collections/{}/query",
                vector_db_url, collection_id
            ))
            .json(&json!({
                "query_embeddings": [embedding],
                "n_results": n_results,
                "include": ["documents", "metadatas", "distances"]
            }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("ChromaDB query error: {e}")),
        };

        if !query_resp.status().is_success() {
            let status = query_resp.status();
            let body = query_resp.text().await.unwrap_or_default();
            return ToolResult::error(format!("ChromaDB query failed ({status}): {body}"));
        }

        let results_json: serde_json::Value = match query_resp.json().await {
            Ok(j) => j,
            Err(e) => {
                return ToolResult::error(format!("Failed to parse ChromaDB query response: {e}"))
            }
        };

        // ChromaDB returns nested arrays (one per query vector)
        let documents = results_json
            .get("documents")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let metadatas = results_json
            .get("metadatas")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let distances = results_json
            .get("distances")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if documents.is_empty() {
            return ToolResult::success(format!("No vault results found for: {query}"));
        }

        let formatted: Vec<serde_json::Value> = documents
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let text = doc.as_str().unwrap_or("[empty]");
                let text_truncated: String = text.chars().take(500).collect();
                let content = if text.chars().count() > 500 {
                    format!("{text_truncated}...")
                } else {
                    text_truncated
                };

                let dist = distances.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);

                let source = metadatas
                    .get(i)
                    .and_then(|m| m.as_object())
                    .and_then(|o| {
                        o.get("source")
                            .or_else(|| o.get("file"))
                            .or_else(|| o.get("filename"))
                    })
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                json!({
                    "rank": i + 1,
                    "source": source,
                    "distance": dist,
                    "content": content
                })
            })
            .collect();

        ToolResult::success(serde_json::to_string_pretty(&formatted).unwrap_or_default())
    }
}
