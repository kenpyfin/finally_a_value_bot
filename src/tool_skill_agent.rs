//! Tool and Skill Agent (TSA): gatekeeper that runs before every tool execution.
//! Returns Allow or Deny (with reason/suggestion) so the main agent loop can execute or inject a synthetic tool result.

use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::config::Config;
use crate::error::MicroClawError;
use crate::llm;
use crate::tools::ToolAuthContext;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TsaDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone)]
pub struct TsaResult {
    pub decision: TsaDecision,
    pub reason: String,
    pub suggestion: Option<String>,
}

const TSA_SYSTEM: &str = r#"You are a tool and skill gatekeeper. Given a conversation snippet and a requested tool call (name + input), output JSON only:

{"decision": "allow" | "deny", "reason": "brief rationale", "suggestion": "optional alternative or instruction"}

Rules:
- Allow if the tool is clearly relevant to the user's request and not redundant or unsafe.
- Allow write_file and edit_file anywhere in the repo (including the skills directory) when used for self-improvement, creating or updating skills, or at the user's request.
- Allow cursor_agent when the task benefits from an agent (editing, creating skills, or multi-step work in the repo).
- Deny only if: the tool is irrelevant, the same call was just made redundantly, or the action is unsafe for this context.
- For sync_skills: allow when the user asked to add or update skills from an external source, or when it supports self-improvement.
- For activate_skill: allow when the skill is relevant to the current task; deny if irrelevant or spammy.
- Keep reason and suggestion concise (one sentence each)."#;

/// Build a short context string from the last few messages (for TSA prompt).
fn build_context_snippet(messages: &[Message], max_messages: usize, max_chars_per_msg: usize) -> String {
    let start = messages.len().saturating_sub(max_messages);
    let mut out = String::new();
    for msg in messages.iter().skip(start) {
        let role = &msg.role;
        let content = match &msg.content {
            MessageContent::Text(t) => t.as_str(),
            MessageContent::Blocks(_) => "[blocks]",
        };
        let truncated = if content.chars().count() > max_chars_per_msg {
            format!("{}...", content.chars().take(max_chars_per_msg).collect::<String>())
        } else {
            content.to_string()
        };
        out.push_str(&format!("{}: {}\n", role, truncated));
    }
    out
}

/// Evaluate whether to allow or deny this tool use. Call before execute_with_auth.
pub async fn evaluate_tool_use(
    config: &Config,
    tool_name: &str,
    tool_input: &serde_json::Value,
    messages: &[Message],
    _auth: Option<&ToolAuthContext>,
) -> Result<TsaResult, MicroClawError> {
    if !config.tool_skill_agent_enabled {
        return Ok(TsaResult {
            decision: TsaDecision::Allow,
            reason: String::new(),
            suggestion: None,
        });
    }

    let mut llm_config = config.clone();
    let model = config
        .tool_skill_agent_model
        .trim();
    if !model.is_empty() {
        llm_config.model = model.to_string();
    } else if !config.orchestrator_model.trim().is_empty() {
        llm_config.model = config.orchestrator_model.trim().to_string();
    }

    let context = build_context_snippet(messages, 4, 300);
    let input_preview = serde_json::to_string(tool_input).unwrap_or_else(|_| "{}".into());
    let input_preview = if input_preview.len() > 500 {
        format!("{}...", &input_preview[..500])
    } else {
        input_preview
    };
    let user_content = format!(
        "Conversation:\n{}\n\nRequested tool: {}\nTool input (JSON): {}",
        context, tool_name, input_preview
    );

    let messages_tsa = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_content),
    }];

    let provider = llm::create_provider(&llm_config);
    let response = provider
        .send_message(TSA_SYSTEM, messages_tsa, None)
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

    let parsed = parse_tsa_response(&text)?;
    info!(
        "TSA decision: {:?} for tool {} - {}",
        parsed.decision, tool_name, parsed.reason
    );
    Ok(parsed)
}

fn parse_tsa_response(text: &str) -> Result<TsaResult, MicroClawError> {
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
        decision: String,
        reason: Option<String>,
        suggestion: Option<String>,
    }
    let raw: Raw = serde_json::from_str(json_str).map_err(|e| {
        MicroClawError::Config(format!(
            "TSA failed to parse JSON: {e}. Raw: {}",
            json_str.chars().take(300).collect::<String>()
        ))
    })?;
    let decision = match raw.decision.to_lowercase().as_str() {
        "allow" => TsaDecision::Allow,
        _ => TsaDecision::Deny,
    };
    Ok(TsaResult {
        decision,
        reason: raw.reason.unwrap_or_default(),
        suggestion: raw.suggestion.filter(|s| !s.trim().is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tsa_allow() {
        let j = r#"{"decision": "allow", "reason": "ok"}"#;
        let r = parse_tsa_response(j).unwrap();
        assert_eq!(r.decision, TsaDecision::Allow);
        assert_eq!(r.reason, "ok");
    }

    #[test]
    fn test_parse_tsa_deny() {
        let j = r#"{"decision": "deny", "reason": "irrelevant", "suggestion": "Use X instead"}"#;
        let r = parse_tsa_response(j).unwrap();
        assert_eq!(r.decision, TsaDecision::Deny);
        assert_eq!(r.reason, "irrelevant");
        assert_eq!(r.suggestion.as_deref(), Some("Use X instead"));
    }
}
