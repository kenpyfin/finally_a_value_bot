//! Post-Tool Evaluator (PTE): evaluates whether a task is complete after tool execution.
//! Called after each tool iteration to decide whether to continue the agent loop or synthesize a final response.

use crate::claude::{ContentBlock, Message, MessageContent, ResponseContentBlock};
use crate::config::Config;
use crate::error::MicroClawError;
use crate::llm;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PteAction {
    Continue,
    Complete,
}

#[derive(Debug, Clone)]
pub struct PteResult {
    pub action: PteAction,
    pub reason: String,
}

/// Build the PTE system prompt with principles and memory context baked in.
fn build_pte_system_prompt(principles_content: &str, memory_context: &str) -> String {
    let mut prompt = String::from(
        r#"You are a task-completion evaluator. Given the agent's principles, memory context, the user's original request, and the tool results so far, determine whether the task is complete.

Output JSON only: {"action": "continue" | "complete", "reason": "brief rationale"}

Rules:
- "complete" = all aspects of the user's request have been fulfilled by the tool results
- "continue" = the task needs more steps, or the results are partial/inconclusive
- Consider the agent's principles when evaluating: if principles require confirmation, verification, or follow-up steps, the task is not complete until those are done
- Consider memory context: if the user has ongoing projects or preferences that affect what "done" means, factor those in
- If in doubt, say "continue" — it is safer to let the LLM decide than to prematurely stop
- Keep reason concise (one sentence)
"#,
    );

    if !principles_content.trim().is_empty() {
        prompt.push_str("\n# Principles\n\n");
        prompt.push_str(principles_content);
        prompt.push_str("\n");
    }

    if !memory_context.trim().is_empty() {
        prompt.push_str("\n# Memory Context\n\n");
        prompt.push_str(memory_context);
        prompt.push_str("\n");
    }

    prompt
}

/// Extract the original user request from the conversation (first user message text).
fn extract_original_request(messages: &[Message]) -> String {
    for msg in messages {
        if msg.role == "user" {
            match &msg.content {
                MessageContent::Text(t) => {
                    let truncated = if t.chars().count() > 500 {
                        format!("{}...", t.chars().take(500).collect::<String>())
                    } else {
                        t.clone()
                    };
                    return truncated;
                }
                MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        if let ContentBlock::Text { text } = block {
                            let truncated = if text.chars().count() > 500 {
                                format!("{}...", text.chars().take(500).collect::<String>())
                            } else {
                                text.clone()
                            };
                            return truncated;
                        }
                    }
                }
            }
        }
    }
    "(no user request found)".to_string()
}

/// Build a summary of the most recent tool calls and results.
fn build_tool_results_summary(messages: &[Message], max_messages: usize) -> String {
    let mut out = String::new();
    let start = messages.len().saturating_sub(max_messages);

    for msg in messages.iter().skip(start) {
        match &msg.content {
            MessageContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::ToolUse { name, input, .. } => {
                            let input_str =
                                serde_json::to_string(input).unwrap_or_else(|_| "{}".into());
                            let input_preview = if input_str.len() > 200 {
                                format!("{}...", &input_str[..200])
                            } else {
                                input_str
                            };
                            out.push_str(&format!("Tool called: {} with {}\n", name, input_preview));
                        }
                        ContentBlock::ToolResult { content, is_error, .. } => {
                            let status = if is_error.unwrap_or(false) {
                                "ERROR"
                            } else {
                                "OK"
                            };
                            let preview = if content.chars().count() > 300 {
                                format!("{}...", content.chars().take(300).collect::<String>())
                            } else {
                                content.clone()
                            };
                            out.push_str(&format!("Result ({}): {}\n", status, preview));
                        }
                        _ => {}
                    }
                }
            }
            MessageContent::Text(t) => {
                if msg.role == "assistant" && !t.trim().is_empty() {
                    let preview = if t.chars().count() > 200 {
                        format!("{}...", t.chars().take(200).collect::<String>())
                    } else {
                        t.clone()
                    };
                    out.push_str(&format!("Assistant: {}\n", preview));
                }
            }
        }
    }

    out
}

/// Build the user message for PTE evaluation.
fn build_pte_user_prompt(messages: &[Message], iteration: usize, max_iterations: usize) -> String {
    let original_request = extract_original_request(messages);
    let tool_summary = build_tool_results_summary(messages, 6);

    format!(
        "Original user request: {}\n\nTools called and their results:\n{}\nCurrent iteration: {} of {}",
        original_request, tool_summary, iteration + 1, max_iterations
    )
}

/// Evaluate whether the task is complete after tool execution.
/// Returns Continue immediately if PTE is disabled.
pub async fn evaluate_completion(
    config: &Config,
    principles_content: &str,
    memory_context: &str,
    messages: &[Message],
    iteration: usize,
) -> Result<PteResult, MicroClawError> {
    if !config.post_tool_evaluator_enabled {
        return Ok(PteResult {
            action: PteAction::Continue,
            reason: String::new(),
        });
    }

    let mut llm_config = config.clone();
    let model = config.post_tool_evaluator_model.trim();
    if !model.is_empty() {
        llm_config.model = model.to_string();
    } else if !config.orchestrator_model.trim().is_empty() {
        llm_config.model = config.orchestrator_model.trim().to_string();
    }

    let system_prompt = build_pte_system_prompt(principles_content, memory_context);
    let user_prompt = build_pte_user_prompt(messages, iteration, config.max_tool_iterations);

    let pte_messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_prompt),
    }];

    let provider = llm::create_provider(&llm_config);
    let response = provider
        .send_message(&system_prompt, pte_messages, None)
        .await?;

    let text: String = response
        .content
        .iter()
        .filter_map(|block| match block {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    let parsed = parse_pte_response(&text)?;
    info!(
        "PTE decision: {:?} at iteration {} - {}",
        parsed.action,
        iteration + 1,
        parsed.reason
    );
    Ok(parsed)
}

fn parse_pte_response(text: &str) -> Result<PteResult, MicroClawError> {
    let trimmed = text.trim();
    let json_str = if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    #[derive(Deserialize)]
    struct Raw {
        action: String,
        reason: Option<String>,
    }
    let raw: Raw = serde_json::from_str(json_str).map_err(|e| {
        MicroClawError::Config(format!(
            "PTE failed to parse JSON: {e}. Raw: {}",
            json_str.chars().take(300).collect::<String>()
        ))
    })?;
    let action = match raw.action.to_lowercase().as_str() {
        "complete" => PteAction::Complete,
        _ => PteAction::Continue,
    };
    Ok(PteResult {
        action,
        reason: raw.reason.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pte_continue() {
        let j = r#"{"action": "continue", "reason": "task not done"}"#;
        let r = parse_pte_response(j).unwrap();
        assert_eq!(r.action, PteAction::Continue);
        assert_eq!(r.reason, "task not done");
    }

    #[test]
    fn test_parse_pte_complete() {
        let j = r#"{"action": "complete", "reason": "all steps done"}"#;
        let r = parse_pte_response(j).unwrap();
        assert_eq!(r.action, PteAction::Complete);
        assert_eq!(r.reason, "all steps done");
    }

    #[test]
    fn test_parse_pte_unknown_defaults_to_continue() {
        let j = r#"{"action": "unknown", "reason": "weird"}"#;
        let r = parse_pte_response(j).unwrap();
        assert_eq!(r.action, PteAction::Continue);
    }

    #[test]
    fn test_build_pte_system_prompt_empty() {
        let prompt = build_pte_system_prompt("", "");
        assert!(prompt.contains("task-completion evaluator"));
        assert!(!prompt.contains("# Principles"));
        assert!(!prompt.contains("# Memory Context"));
    }

    #[test]
    fn test_build_pte_system_prompt_with_content() {
        let prompt = build_pte_system_prompt("Be helpful", "User likes Rust");
        assert!(prompt.contains("# Principles"));
        assert!(prompt.contains("Be helpful"));
        assert!(prompt.contains("# Memory Context"));
        assert!(prompt.contains("User likes Rust"));
    }

    #[test]
    fn test_extract_original_request() {
        let messages = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("Hello, help me".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("Sure!".into()),
            },
        ];
        let req = extract_original_request(&messages);
        assert_eq!(req, "Hello, help me");
    }

    #[test]
    fn test_extract_original_request_empty() {
        let messages: Vec<Message> = vec![];
        let req = extract_original_request(&messages);
        assert_eq!(req, "(no user request found)");
    }
}
