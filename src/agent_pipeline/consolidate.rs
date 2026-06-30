//! Consolidation and direct-answer paths for the deterministic pipeline.

use crate::agent_pipeline::profile::{OperationalConfig, PhaseContextIncludes, ResolvedPhase};
use crate::claude::{Message, MessageContent, ResponseContentBlock};
use crate::error::FinallyAValueBotError;
use crate::multimodel::ModelTier;
use crate::telegram::AppState;

use super::execute::StepResult;
use super::intent::IntentDecision;
use super::plan::Plan;

pub(crate) const DELIVERY_SYSTEM: &str = "\
You write the final user-facing reply after a multi-step agent run.\n\
Rules:\n\
- Address the user's original message directly and conversationally (match their tone: casual if they were casual).\n\
- Lead with outcomes: images created, files saved, what the user can look at now — use markdown image links with absolute paths when step output mentions .png/.jpg paths.\n\
- If hotify or ComfyUI failed (exit code 1, HOTIFY_START, queue errors), say so plainly but briefly — do NOT claim the skill script was missing if hotify_cli.py actually ran.\n\
- Do not mention pipeline stages, step numbers, 'execution phase', or internal tooling unless the user asked for debugging.\n\
- Do not produce bullet-only status reports or 'Action Required' blocks unless the user explicitly asked for an ops summary.\n\
- If work partially succeeded, show what worked first, then one short paragraph on what failed and the sensible next try.\n\
- Be concise but natural — like a skilled assistant texting back, not a ticket system.";

pub fn builtin_delivery_system_prompt() -> &'static str {
    DELIVERY_SYSTEM
}

/// When true, a single successful step summary can skip the full delivery LLM (still uses polish on skip path).
pub fn should_synthesize_final_with_config(
    plan: &Plan,
    step_results: &[StepResult],
    operational: &OperationalConfig,
    skip_when_good: bool,
) -> bool {
    if !skip_when_good {
        return true;
    }
    if step_results.is_empty() {
        return true;
    }
    if step_results.iter().any(|r| !r.success) {
        return true;
    }
    if plan.steps.len() > 1 && step_results.len() > 1 {
        let combined: usize = step_results.iter().map(|r| r.summary.len()).sum();
        if combined > operational.max_polish_only_combined_chars {
            return true;
        }
    }
    if step_results.len() == 1 {
        let only = &step_results[0];
        if only.had_tool_errors {
            return true;
        }
        if only.summary.trim().len() < operational.min_polish_only_summary_chars {
            return true;
        }
        return false;
    }
    step_results.iter().any(|r| r.had_tool_errors)
}

pub async fn synthesize_final_with_config(
    state: &AppState,
    intent: &IntentDecision,
    plan: &Plan,
    step_results: &[StepResult],
    user_request: &str,
    resolved: Option<&ResolvedPhase<'_>>,
    agent_system_prompt: Option<&str>,
) -> Result<String, FinallyAValueBotError> {
    let includes = resolved
        .map(|r| &r.phase.context_includes)
        .cloned()
        .unwrap_or_default();
    let system = if let Some(r) = resolved {
        crate::agent_pipeline::profile::compose_system_prompt(
            r.phase,
            r.phase.kind,
            agent_system_prompt,
            &includes,
        )
    } else {
        DELIVERY_SYSTEM.to_string()
    };
    let tier = resolved
        .map(|r| {
            crate::agent_pipeline::profile::resolve_model_tier(
                r.phase.model_route,
                state,
                &r.policies,
            )
        })
        .unwrap_or(ModelTier::Strategy);
    let body = format_delivery_body(intent, plan, step_results, user_request, &includes);
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(body),
    }];
    let response = state
        .llm
        .send_message_for_tier(tier, &system, messages, None)
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

/// Light LLM pass when execute already produced a good single-step answer.
pub async fn polish_delivery_with_config(
    state: &AppState,
    intent: &IntentDecision,
    draft: &str,
    user_request: &str,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: &OperationalConfig,
    agent_system_prompt: Option<&str>,
) -> Result<String, FinallyAValueBotError> {
    if draft.trim().len() < operational.min_polish_only_summary_chars {
        return synthesize_final_with_config(
            state,
            intent,
            &Plan {
                source: "polish".into(),
                vault_path: None,
                steps: vec![],
            },
            &[StepResult {
                step_id: "1".into(),
                success: true,
                summary: draft.to_string(),
                full_output: draft.to_string(),
                tool_names: vec![],
                had_tool_errors: false,
                iterations: vec![],
            }],
            user_request,
            resolved,
            agent_system_prompt,
        )
        .await;
    }
    let includes = resolved
        .map(|r| &r.phase.context_includes)
        .cloned()
        .unwrap_or_default();
    let system = if let Some(r) = resolved {
        crate::agent_pipeline::profile::compose_system_prompt(
            r.phase,
            r.phase.kind,
            agent_system_prompt,
            &includes,
        )
    } else {
        DELIVERY_SYSTEM.to_string()
    };
    let tier = resolved
        .map(|r| {
            crate::agent_pipeline::profile::resolve_model_tier(
                r.phase.model_route,
                state,
                &r.policies,
            )
        })
        .unwrap_or(ModelTier::Strategy);
    let request_line = if includes.include_current_request {
        format!("User message:\n{user_request}\n\n")
    } else {
        String::new()
    };
    let goal_line = if includes.include_execution_summary {
        format!("Goal: {}\n\n", intent.restated_goal)
    } else {
        String::new()
    };
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(format!(
            "{request_line}{goal_line}Draft reply to polish (keep facts, improve tone):\n{draft}",
        )),
    }];
    let response = state
        .llm
        .send_message_for_tier(tier, &system, messages, None)
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
    Ok(if text.trim().is_empty() {
        draft.to_string()
    } else {
        text
    })
}

fn format_delivery_body(
    intent: &IntentDecision,
    plan: &Plan,
    step_results: &[StepResult],
    user_request: &str,
    includes: &PhaseContextIncludes,
) -> String {
    let request_line = if includes.include_current_request {
        format!("User message:\n{user_request}\n\n")
    } else {
        String::new()
    };
    if !includes.include_execution_summary {
        return format!("{request_line}Write the final user-facing reply.",);
    }
    let mut body = format!(
        "{request_line}Restated goal: {}\nPlan source: {}\n\nStep results:\n",
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
    body
}

pub async fn direct_answer(
    state: &AppState,
    system_prompt: &str,
    messages: &[Message],
    with_tools: bool,
    includes: &PhaseContextIncludes,
) -> Result<String, FinallyAValueBotError> {
    let llm_messages = if includes.include_session_excerpt {
        messages.to_vec()
    } else {
        messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .cloned()
            .map(|m| vec![m])
            .unwrap_or_else(|| messages.to_vec())
    };
    let tools = if with_tools {
        Some(state.tools.definitions_filtered(true))
    } else {
        None
    };
    let response = state
        .llm
        .send_message_for_tier(ModelTier::Strategy, system_prompt, llm_messages, tools)
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

pub fn fallback_summary(step_results: &[StepResult]) -> String {
    if step_results.len() == 1 {
        let only = &step_results[0];
        if !only.summary.trim().is_empty() {
            return only.summary.clone();
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn step(success: bool, summary: &str, tool_errors: bool) -> StepResult {
        StepResult {
            step_id: "1".into(),
            success,
            summary: summary.into(),
            full_output: summary.into(),
            tool_names: vec![],
            had_tool_errors: tool_errors,
            iterations: vec![],
        }
    }

    #[test]
    fn skip_consolidate_single_good_step() {
        let plan = Plan {
            source: "ephemeral".into(),
            vault_path: None,
            steps: vec![],
        };
        let results = vec![step(
            true,
            "Completed successfully with enough detail to deliver directly to the operator without synthesis.",
            false,
        )];
        assert!(!should_synthesize_final_with_config(
            &plan,
            &results,
            &OperationalConfig::default(),
            true,
        ));
    }

    #[test]
    fn synthesize_when_step_failed() {
        let plan = Plan {
            source: "ephemeral".into(),
            vault_path: None,
            steps: vec![],
        };
        let results = vec![step(false, "failed", true)];
        assert!(should_synthesize_final_with_config(
            &plan,
            &results,
            &OperationalConfig::default(),
            true,
        ));
    }
}
