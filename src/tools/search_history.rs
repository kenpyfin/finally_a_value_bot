use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use super::{auth_context_from_input, schema_object, Tool, ToolResult};
use crate::claude::ToolDefinition;
use crate::db::Database;

pub struct SearchHistoryTool {
    db: Arc<Database>,
}

impl SearchHistoryTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Tool for SearchHistoryTool {
    fn name(&self) -> &str {
        "search_chat_history"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_chat_history".into(),
            description: "Search past messages in this chat using full-text search. Use this to recall past conversations, facts, or context the user mentioned previously. Always use this before saying \"I don't remember\" or asking the user to repeat something.".into(),
            input_schema: schema_object(
                json!({
                    "query": {
                        "type": "string",
                        "description": "Keyword or phrase to search for in past messages"
                    },
                    "chat_id": {
                        "type": "integer",
                        "description": "The chat ID to search in (use the current chat_id from the system prompt)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 20, max: 100)"
                    },
                    "from_date": {
                        "type": "string",
                        "description": "Optional start date filter in YYYY-MM-DD format"
                    },
                    "to_date": {
                        "type": "string",
                        "description": "Optional end date filter in YYYY-MM-DD format"
                    }
                }),
                &["query", "chat_id"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => q.to_string(),
            _ => return ToolResult::error("Missing or empty 'query' parameter".into()),
        };

        let chat_id = match input.get("chat_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return ToolResult::error("Missing 'chat_id' parameter".into()),
        };

        // Permission check via auth context
        if let Some(auth) = auth_context_from_input(&input) {
            if !auth.can_access_chat(chat_id) {
                return ToolResult::error(format!(
                    "Permission denied: cannot search history for chat {chat_id}"
                ));
            }
        }

        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20)
            .min(100) as usize;

        let from_date = input
            .get("from_date")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let to_date = input
            .get("to_date")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let persona_id = auth_context_from_input(&input)
            .map(|a| a.caller_persona_id)
            .unwrap_or(0);

        let db = self.db.clone();
        let query_owned = query.clone();
        let from_ref = from_date.clone();
        let to_ref = to_date.clone();

        let result = tokio::task::spawn_blocking(move || {
            db.search_messages(
                chat_id,
                persona_id,
                &query_owned,
                limit,
                from_ref.as_deref(),
                to_ref.as_deref(),
            )
        })
        .await;

        match result {
            Ok(Ok(messages)) => {
                if messages.is_empty() {
                    return ToolResult::success(format!("No messages found matching '{query}'"));
                }
                let results: Vec<serde_json::Value> = messages
                    .iter()
                    .map(|m| {
                        let excerpt: String = m.content.chars().take(200).collect();
                        let excerpt = if m.content.chars().count() > 200 {
                            format!("{excerpt}...")
                        } else {
                            excerpt
                        };
                        json!({
                            "timestamp": m.timestamp,
                            "sender": m.sender_name,
                            "is_bot": m.is_from_bot,
                            "excerpt": excerpt
                        })
                    })
                    .collect();
                ToolResult::success(serde_json::to_string_pretty(&results).unwrap_or_default())
            }
            Ok(Err(e)) => ToolResult::error(format!(
                "Search failed: {e}. Try simpler keywords or check the query syntax."
            )),
            Err(e) => ToolResult::error(format!("Search task error: {e}")),
        }
    }
}
