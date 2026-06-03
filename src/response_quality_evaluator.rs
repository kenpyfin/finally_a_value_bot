//! Pre-delivery Quality Evaluator (PDQE): synchronous QC before the user receives a reply.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tracing::warn;

use crate::agent_history::append_pdqe_step_to_agent_history;
use crate::agent_turn_context::SessionGoalContext;
use crate::channels::telegram::BACKGROUND_JOB_HANDOFF_PREFIX;
use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::config::Config;
use crate::db::call_blocking;
use crate::db::Database;
use crate::error::FinallyAValueBotError;
use crate::llm;
use crate::post_tool_evaluator::build_tool_results_summary;
use crate::safety_redaction::EnvSecretRedactor;

const EVAL_TIMEOUT_SECS: u64 = 60;
const PDQE_TOOL_TRACE_MAX_MESSAGES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityVerdictKind {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Clone)]
pub struct QualityVerdict {
    pub kind: QualityVerdictKind,
    pub issues: Vec<String>,
    pub feedback_for_agent: String,
    pub confidence: f64,
}

/// Context for pre-delivery quality evaluation (in-loop gate).
#[derive(Debug, Clone)]
pub struct PostDeliveryEvalContext {
    pub run_key: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub caller_channel: String,
    pub chat_type: String,
    pub stop_reason: String,
    pub delivered_text: String,
    pub session_goal: SessionGoalContext,
    pub tool_trace_summary: String,
    pub principles_excerpt: String,
    pub is_scheduled_task: bool,
    pub is_conversational: bool,
    pub intent_signature: Option<String>,
    pub tool_names: Vec<String>,
    /// Basename of the run's `agent_history/*.md` file (set when the main loop saved history).
    pub agent_history_basename: Option<String>,
    pub runtime_data_dir: String,
}

pub fn quality_eval_channel_allowed(config: &Config, channel: &str) -> bool {
    let ch = channel.trim().to_lowercase();
    config
        .quality_eval_channels
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .any(|allowed| allowed == ch)
}

pub fn should_skip_pdqe(config: &Config, ctx: &PostDeliveryEvalContext) -> Option<&'static str> {
    if !config.response_quality_evaluator_enabled {
        return Some("disabled");
    }
    if !quality_eval_channel_allowed(config, &ctx.caller_channel) {
        return Some("channel_not_allowed");
    }
    if ctx.stop_reason == "ask_clarification" || ctx.stop_reason == "cancelled" {
        return Some("stop_reason");
    }
    if ctx.delivered_text.trim().is_empty() {
        return Some("empty_delivery");
    }
    if ctx
        .delivered_text
        .starts_with(BACKGROUND_JOB_HANDOFF_PREFIX)
    {
        return Some("background_handoff");
    }
    if ctx.session_goal.current_request.trim().is_empty() {
        return Some("no_session_goal");
    }
    None
}

pub fn build_quality_feedback_message(
    session_goal: &SessionGoalContext,
    verdict: &QualityVerdict,
) -> String {
    format!(
        "[quality_eval_feedback]\nSession goal (current_request): {}\n\nQuality review found: {}\nIssues: {}\n\nRevise or complete the task for the user relative to the session goal above. Do not repeat the same mistakes. If you cannot fix it, say what is blocked.",
        session_goal.current_request,
        verdict.feedback_for_agent,
        if verdict.issues.is_empty() {
            "(none listed)".to_string()
        } else {
            verdict.issues.join("; ")
        }
    )
}

fn build_pdqe_system_prompt(principles_excerpt: &str) -> String {
    let mut prompt = String::from(
        r#"You are a pre-delivery quality evaluator. Judge whether the candidate assistant reply satisfies the session goal (`current_request`) for this turn.

Output JSON only:
{"verdict":"pass"|"fail"|"skip","issues":["..."],"feedback_for_agent":"one paragraph","confidence":0.0}

Rules:
- The session goal is ONLY `current_request`. Prior turns (`prior_turn`) and persona memory/bulletin are background — do not penalize ignoring unrelated older tasks.
- Pass: direct answer to current_request, justified clarification question, or honest blocked state relative to current_request.
- Fail: off-topic vs current_request, clearly incomplete for that ask, contradicts tool evidence, or expands into memory/bulletin topics the user did not raise.
- When disambiguation context is provided for a short reply, judge whether the reply resolves the referent correctly.
- Do not use web search. Judge only the supplied fields.
- confidence is 0.0–1.0 (how sure you are of verdict).
"#,
    );
    if !principles_excerpt.trim().is_empty() {
        prompt.push_str("\n# Principles (constraints only)\n\n");
        prompt.push_str(principles_excerpt);
        prompt.push('\n');
    }
    prompt
}

fn build_pdqe_user_prompt(ctx: &PostDeliveryEvalContext) -> String {
    let mut prompt = format!(
        "Session goal (current_request):\n{}\n\nCandidate reply:\n{}\n\nTool trace:\n{}\nStop reason: {}\n",
        ctx.session_goal.current_request,
        ctx.delivered_text,
        ctx.tool_trace_summary,
        ctx.stop_reason
    );
    if ctx.session_goal.is_short_reply {
        prompt.push_str("\nShort-reply turn: yes\n");
        if let Some(ref d) = ctx.session_goal.disambiguation_assistant {
            prompt.push_str("Disambiguation (prior assistant excerpt):\n");
            prompt.push_str(d);
            prompt.push('\n');
        }
    }
    prompt
}

pub fn parse_evaluator_json_response(text: &str) -> Result<QualityVerdict, FinallyAValueBotError> {
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
        verdict: String,
        #[serde(default)]
        issues: Vec<String>,
        #[serde(default)]
        feedback_for_agent: String,
        #[serde(default)]
        confidence: Option<f64>,
    }
    let raw: Raw = serde_json::from_str(json_str).map_err(|e| {
        FinallyAValueBotError::Config(format!(
            "PDQE failed to parse JSON: {e}. Raw: {}",
            json_str.chars().take(300).collect::<String>()
        ))
    })?;
    let kind = match raw.verdict.to_lowercase().as_str() {
        "pass" => QualityVerdictKind::Pass,
        "fail" => QualityVerdictKind::Fail,
        "skip" => QualityVerdictKind::Skip,
        other => {
            return Err(FinallyAValueBotError::Config(format!(
                "PDQE unknown verdict: {other}"
            )));
        }
    };
    Ok(QualityVerdict {
        kind,
        issues: raw.issues,
        feedback_for_agent: raw.feedback_for_agent,
        confidence: raw.confidence.unwrap_or(0.0).clamp(0.0, 1.0),
    })
}

fn fast_path_verdict(config: &Config, ctx: &PostDeliveryEvalContext) -> Option<QualityVerdict> {
    let reply = ctx.delivered_text.trim();
    if reply.is_empty() {
        return None;
    }
    if ctx.stop_reason == "ask_clarification" && reply.contains('?') {
        return Some(QualityVerdict {
            kind: QualityVerdictKind::Pass,
            issues: vec![],
            feedback_for_agent: String::new(),
            confidence: 1.0,
        });
    }
    let _ = config;
    None
}

pub async fn evaluate_delivery_quality(
    config: &Config,
    env_redactor: &EnvSecretRedactor,
    ctx: &PostDeliveryEvalContext,
) -> Result<QualityVerdict, FinallyAValueBotError> {
    if let Some(_reason) = should_skip_pdqe(config, ctx) {
        return Ok(QualityVerdict {
            kind: QualityVerdictKind::Skip,
            issues: vec![],
            feedback_for_agent: String::new(),
            confidence: 0.0,
        });
    }

    if let Some(v) = fast_path_verdict(config, ctx) {
        return Ok(v);
    }

    let provider = llm::create_evaluator_provider(config)?;
    let system = build_pdqe_system_prompt(&ctx.principles_excerpt);
    let user = build_pdqe_user_prompt(ctx);
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user),
    }];

    let response = tokio::time::timeout(
        Duration::from_secs(EVAL_TIMEOUT_SECS),
        provider.send_message(&system, messages, None),
    )
    .await
    .map_err(|_| FinallyAValueBotError::Config("PDQE Perplexity call timed out".into()))??;

    let text: String = response
        .content
        .iter()
        .filter_map(|block| match block {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    let _ = env_redactor;
    parse_evaluator_json_response(&text)
}

pub fn log_pdqe_to_agent_history(ctx: &PostDeliveryEvalContext, step: &str, detail: &str) {
    let Some(ref basename) = ctx.agent_history_basename else {
        return;
    };
    if let Err(e) = append_pdqe_step_to_agent_history(
        &ctx.runtime_data_dir,
        ctx.chat_id,
        ctx.persona_id,
        basename,
        step,
        detail,
    ) {
        warn!(
            run_key = %ctx.run_key,
            basename = %basename,
            "Failed to append PDQE step to agent history: {e}"
        );
    }
}

pub async fn append_quality_timeline(
    db: Arc<Database>,
    run_key: &str,
    chat_id: i64,
    persona_id: i64,
    event_type: &str,
    payload: &str,
) {
    let _ = call_blocking(db, {
        let run_key = run_key.to_string();
        let event_type = event_type.to_string();
        let payload = payload.to_string();
        move |db| {
            db.append_run_timeline_event(&run_key, chat_id, persona_id, &event_type, Some(&payload))
        }
    })
    .await;
}

/// Build tool trace summary for PDQE from messages in the current run.
pub fn build_pdqe_tool_trace(
    env_redactor: &EnvSecretRedactor,
    messages: &[Message],
    run_start_index: usize,
) -> String {
    let start = run_start_index.min(messages.len());
    let slice = &messages[start..];
    let window = slice.len().min(PDQE_TOOL_TRACE_MAX_MESSAGES);
    build_tool_results_summary(env_redactor, slice, window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pass_verdict() {
        let j = r#"{"verdict":"pass","issues":[],"feedback_for_agent":"","confidence":0.95}"#;
        let v = parse_evaluator_json_response(j).unwrap();
        assert_eq!(v.kind, QualityVerdictKind::Pass);
        assert!((v.confidence - 0.95).abs() < 0.01);
    }

    #[test]
    fn parse_fail_verdict() {
        let j = r#"{"verdict":"fail","issues":["incomplete"],"feedback_for_agent":"missing steps","confidence":0.8}"#;
        let v = parse_evaluator_json_response(j).unwrap();
        assert_eq!(v.kind, QualityVerdictKind::Fail);
        assert_eq!(v.issues, vec!["incomplete"]);
    }

    #[test]
    fn channel_allowlist() {
        let mut config = Config::test_config();
        config.quality_eval_channels = "telegram,web".into();
        assert!(quality_eval_channel_allowed(&config, "telegram"));
        assert!(quality_eval_channel_allowed(&config, "Web"));
        assert!(!quality_eval_channel_allowed(&config, "scheduler"));
    }
}
