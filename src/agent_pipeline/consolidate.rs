//! Consolidation and direct-answer paths for the deterministic pipeline.

use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::error::FinallyAValueBotError;
use crate::multimodel::ModelTier;
use crate::telegram::AppState;

use super::execute::StepResult;
use super::intent::IntentDecision;
use super::plan::Plan;

const CONSOLIDATE_SYSTEM: &str = "\
You are synthesizing a final operator-facing reply from structured step results. \
Ground every claim in the step summaries provided. \
If a step failed, say so honestly and state what was accomplished. \
Do not mention internal pipeline stages. Be concise and actionable.";

pub async fn synthesize_final(
    state: &AppState,
    intent: &IntentDecision,
    plan: &Plan,
    step_results: &[StepResult],
) -> Result<String, FinallyAValueBotError> {
    let mut body = format!(
        "Original goal: {}\nPlan source: {}\n\n",
        intent.restated_goal, plan.source
    );
    for r in step_results {
        body.push_str(&format!(
            "## Step {} — {}\n{}\nTools: {}\n\n",
            r.step_id,
            if r.success { "OK" } else { "FAILED" },
            r.summary,
            if r.tool_names.is_empty() {
                "(none)".into()
            } else {
                r.tool_names.join(", ")
            }
        ));
    }
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(body),
    }];
    let response = state
        .llm
        .send_message_for_tier(ModelTier::Strategy, CONSOLIDATE_SYSTEM, messages, None)
        .await?;
    let text: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    if text.trim().is_empty() {
        return Ok(fallback_summary(step_results));
    }
    Ok(text)
}

pub async fn direct_answer(
    state: &AppState,
    system_prompt: &str,
    messages: &[Message],
    with_tools: bool,
) -> Result<String, FinallyAValueBotError> {
    let tools = if with_tools {
        Some(state.tools.definitions_filtered(true))
    } else {
        None
    };
    let response = state
        .llm
        .send_message_for_tier(ModelTier::Strategy, system_prompt, messages.to_vec(), tools)
        .await?;
    let stop = response.stop_reason.as_deref().unwrap_or("end_turn");
    if stop == "tool_use" {
        let text: String = response
            .content
            .iter()
            .filter_map(|b| match b {
                ResponseContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        if !text.trim().is_empty() {
            return Ok(text);
        }
        return Ok(
            "I need to use tools for this — please switch to the classic engine or rephrase as a task."
                .into(),
        );
    }
    let text: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    Ok(if text.trim().is_empty() {
        "I don't have a response.".into()
    } else {
        text
    })
}

fn fallback_summary(step_results: &[StepResult]) -> String {
    let mut lines = vec!["Here's what I found:".to_string()];
    for r in step_results {
        lines.push(format!(
            "- Step {}: {}",
            r.step_id,
            truncate(&r.summary, 400)
        ));
    }
    lines.join("\n")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
