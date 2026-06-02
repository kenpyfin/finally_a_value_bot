//! Post-Delivery Quality Evaluator (PDQE): async QC after the user receives a reply.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tracing::{info, warn};

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

/// Context captured at end of an agent run for post-delivery evaluation.
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
    pub is_quality_nudge: bool,
    pub is_conversational: bool,
    pub intent_signature: Option<String>,
    pub tool_names: Vec<String>,
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
    if ctx.is_quality_nudge {
        return Some("quality_nudge_run");
    }
    if !quality_eval_channel_allowed(config, &ctx.caller_channel) {
        return Some("channel_not_allowed");
    }
    if ctx.stop_reason == "ask_clarification" {
        return Some("ask_clarification");
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

fn quality_nudge_count_for_run(
    db: &crate::db::Database,
    run_key: &str,
) -> Result<usize, FinallyAValueBotError> {
    let events = db.get_run_timeline_events(run_key, 64)?;
    Ok(events
        .iter()
        .filter(|e| e.event_type == "quality_nudge_enqueued")
        .count())
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
        r#"You are a post-delivery quality evaluator. Judge whether the delivered assistant reply satisfies the session goal (`current_request`) for this turn.

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
        "Session goal (current_request):\n{}\n\nDelivered reply:\n{}\n\nTool trace:\n{}\nStop reason: {}\n",
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
        _ => QualityVerdictKind::Skip,
    };
    Ok(QualityVerdict {
        kind,
        issues: raw.issues,
        feedback_for_agent: raw.feedback_for_agent,
        confidence: raw.confidence.unwrap_or(0.0).clamp(0.0, 1.0),
    })
}

fn fast_path_verdict(config: &Config, ctx: &PostDeliveryEvalContext) -> Option<QualityVerdict> {
    if ctx.is_conversational {
        return Some(QualityVerdict {
            kind: QualityVerdictKind::Pass,
            issues: vec![],
            feedback_for_agent: String::new(),
            confidence: 1.0,
        });
    }
    let goal = ctx.session_goal.current_request.to_ascii_lowercase();
    let reply = ctx.delivered_text.to_ascii_lowercase();
    if reply.trim() == "done." && goal.len() > 20 && !goal.contains('?') {
        return Some(QualityVerdict {
            kind: QualityVerdictKind::Fail,
            issues: vec!["empty_done".into()],
            feedback_for_agent:
                "The reply was only \"Done.\" but the session goal required substantive work."
                    .into(),
            confidence: 0.9,
        });
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

#[derive(Debug, Clone)]
pub enum PdqFollowUp {
    None,
    CorrectiveRun {
        feedback: String,
        chat_id: i64,
        persona_id: i64,
        caller_channel: String,
        chat_type: String,
        parent_run_key: String,
    },
}

async fn append_timeline(
    db: std::sync::Arc<Database>,
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

pub async fn run_post_delivery_eval_task(
    config: &Config,
    db: Arc<Database>,
    env_redactor: &EnvSecretRedactor,
    ctx: PostDeliveryEvalContext,
) -> Result<PdqFollowUp, FinallyAValueBotError> {
    if should_skip_pdqe(config, &ctx).is_some() {
        return Ok(PdqFollowUp::None);
    }

    append_timeline(
        db.clone(),
        &ctx.run_key,
        ctx.chat_id,
        ctx.persona_id,
        "quality_eval_started",
        "{}",
    )
    .await;

    let verdict = match evaluate_delivery_quality(config, env_redactor, &ctx).await {
        Ok(v) => v,
        Err(e) => {
            warn!("PDQE evaluation failed (fail-open): {e}");
            return Ok(PdqFollowUp::None);
        }
    };

    let payload = serde_json::json!({
        "verdict": format!("{:?}", verdict.kind),
        "issues": verdict.issues,
        "feedback_preview": verdict.feedback_for_agent.chars().take(200).collect::<String>(),
        "confidence": verdict.confidence,
    })
    .to_string();

    match verdict.kind {
        QualityVerdictKind::Pass => {
            info!(
                run_key = %ctx.run_key,
                "PDQE pass (confidence={})",
                verdict.confidence
            );
            append_timeline(
                db.clone(),
                &ctx.run_key,
                ctx.chat_id,
                ctx.persona_id,
                "quality_eval_pass",
                &payload,
            )
            .await;
            Ok(PdqFollowUp::None)
        }
        QualityVerdictKind::Skip => {
            info!(run_key = %ctx.run_key, "PDQE skip");
            Ok(PdqFollowUp::None)
        }
        QualityVerdictKind::Fail => {
            info!(
                run_key = %ctx.run_key,
                "PDQE fail (confidence={}): {}",
                verdict.confidence,
                verdict.feedback_for_agent
            );
            append_timeline(
                db.clone(),
                &ctx.run_key,
                ctx.chat_id,
                ctx.persona_id,
                "quality_eval_fail",
                &payload,
            )
            .await;

            if config.workflow_auto_learn {
                if let Some(ref sig) = ctx.intent_signature {
                    let steps_json =
                        serde_json::to_string(&ctx.tool_names).unwrap_or_else(|_| "[]".into());
                    let reason = format!("quality_eval:{}", verdict.issues.join(","));
                    let chat_id = ctx.chat_id;
                    let sig = sig.clone();
                    let _ = call_blocking(db.clone(), move |d| {
                        d.upsert_workflow_learning(
                            chat_id,
                            &sig,
                            &steps_json,
                            "[]",
                            "",
                            "failure",
                            Some(reason.as_str()),
                            "{}",
                            false,
                            0.0,
                        )
                    })
                    .await;
                }
            }

            let nudge_cap = config.quality_eval_max_nudges_per_run;
            let existing = call_blocking(db.clone(), {
                let run_key = ctx.run_key.clone();
                move |d| quality_nudge_count_for_run(d, &run_key)
            })
            .await
            .unwrap_or(0);

            if existing >= nudge_cap {
                info!(run_key = %ctx.run_key, "PDQE corrective nudge skipped: budget exhausted");
                return Ok(PdqFollowUp::None);
            }

            if verdict.confidence < config.quality_eval_min_confidence {
                info!(
                    run_key = %ctx.run_key,
                    "PDQE corrective nudge skipped: confidence {} < {}",
                    verdict.confidence,
                    config.quality_eval_min_confidence
                );
                return Ok(PdqFollowUp::None);
            }

            let feedback = build_quality_feedback_message(&ctx.session_goal, &verdict);
            append_timeline(
                db,
                &ctx.run_key,
                ctx.chat_id,
                ctx.persona_id,
                "quality_nudge_enqueued",
                &payload,
            )
            .await;

            Ok(PdqFollowUp::CorrectiveRun {
                feedback,
                chat_id: ctx.chat_id,
                persona_id: ctx.persona_id,
                caller_channel: ctx.caller_channel.clone(),
                chat_type: ctx.chat_type.clone(),
                parent_run_key: ctx.run_key.clone(),
            })
        }
    }
}

/// Build tool trace summary for PDQE from messages.
pub fn build_pdqe_tool_trace(env_redactor: &EnvSecretRedactor, messages: &[Message]) -> String {
    build_tool_results_summary(env_redactor, messages, 8)
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
