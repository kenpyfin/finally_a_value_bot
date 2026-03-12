//! Delegate tool: allows the main agent to spawn a focused sub-agent for a specific task.
//! The sub-agent runs an ephemeral tool loop and returns a single result string.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use teloxide::prelude::*;
use tracing::info;

use crate::agent_loop::run_ephemeral_loop;
use crate::claude::{Message, MessageContent, ToolDefinition};
use crate::config::Config;
use crate::db::Database;
use crate::llm::LlmProvider;

use super::{auth_context_from_input, schema_object, Tool, ToolResult, ToolRegistry};

const MAX_TASK_LEN: usize = 50_000;
const MAX_CONTEXT_LEN: usize = 20_000;
const MAX_RESULT_LEN: usize = 50_000;

const SUB_AGENT_SYSTEM: &str = "You are a focused task assistant. Complete the following task using the tools available to you. Be thorough but concise. When the task is done, reply with a clear final answer. Do not ask follow-up questions.";

const LLM_TIMEOUT_SECS: u64 = 180;
const TOOL_TIMEOUT_SECS: u64 = 120;

pub struct DelegateTool {
    config: Config,
    bot: Bot,
    db: Arc<Database>,
    llm: Arc<dyn LlmProvider>,
}

impl DelegateTool {
    pub fn new(config: Config, bot: Bot, db: Arc<Database>, llm: Arc<dyn LlmProvider>) -> Self {
        Self {
            config,
            bot,
            db,
            llm,
        }
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "delegate".into(),
            description: "Delegate a focused task to a sub-agent that has its own tool loop. \
                Use this for independent sub-tasks (research, file operations, multi-step work) \
                that can run without your conversation context. The sub-agent returns a single \
                result string. Do not delegate simple tasks you can do directly."
                .into(),
            input_schema: schema_object(
                json!({
                    "task": {
                        "type": "string",
                        "description": "Detailed instruction for the sub-agent. Be specific about what to do and what to return."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional extra context to help the sub-agent (e.g. relevant background, file paths, user preferences)."
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Maximum tool iterations for the sub-agent (default from config, typically 10)."
                    }
                }),
                &["task"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let task = match input.get("task").and_then(|v| v.as_str()) {
            Some(t) if !t.trim().is_empty() => t.trim(),
            _ => return ToolResult::error("Missing or empty 'task' parameter".into()),
        };

        if task.len() > MAX_TASK_LEN {
            return ToolResult::error(format!(
                "Task exceeds maximum length of {} characters",
                MAX_TASK_LEN
            ));
        }

        let context = input
            .get("context")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                if s.len() > MAX_CONTEXT_LEN {
                    &s[..s.floor_char_boundary(MAX_CONTEXT_LEN)]
                } else {
                    s
                }
            });

        let max_iterations = input
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(self.config.delegate_max_iterations);

        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => {
                return ToolResult::error(
                    "Delegate requires auth context (called outside of agent loop?)".into(),
                );
            }
        };

        let user_message = match context {
            Some(ctx) => format!("{}\n\nTask: {}", ctx, task),
            None => task.to_string(),
        };

        let messages = vec![Message {
            role: "user".into(),
            content: MessageContent::Text(user_message),
        }];

        let sub_registry = ToolRegistry::new(&self.config, self.bot.clone(), self.db.clone());

        let llm_to_use: &dyn LlmProvider = if !self.config.delegate_model.trim().is_empty() {
            // When a separate delegate model is configured, create a temporary provider.
            // For now, use the main LLM — delegate_model override deferred to avoid
            // holding a second provider. The config field exists for future use.
            self.llm.as_ref()
        } else {
            self.llm.as_ref()
        };

        info!(
            "Delegate: starting sub-agent ({} max iterations) for task: {}",
            max_iterations,
            if task.len() > 100 {
                format!("{}...", &task[..task.floor_char_boundary(100)])
            } else {
                task.to_string()
            }
        );

        match run_ephemeral_loop(
            llm_to_use,
            &sub_registry,
            SUB_AGENT_SYSTEM,
            messages,
            &auth,
            max_iterations,
            LLM_TIMEOUT_SECS,
            TOOL_TIMEOUT_SECS,
        )
        .await
        {
            Ok(result) => {
                let trimmed = if result.len() > MAX_RESULT_LEN {
                    format!(
                        "{}...\n[truncated at {} chars]",
                        &result[..result.floor_char_boundary(MAX_RESULT_LEN)],
                        MAX_RESULT_LEN
                    )
                } else {
                    result
                };
                info!("Delegate: sub-agent completed ({} chars)", trimmed.len());
                ToolResult::success(trimmed)
            }
            Err(e) => {
                info!("Delegate: sub-agent failed: {}", e);
                ToolResult::error(format!("Sub-agent failed: {}", e))
            }
        }
    }
}
