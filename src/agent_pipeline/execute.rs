//! Per-step local execution and verification for the deterministic pipeline.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;

use crate::agent_history::{IterationRecord, ToolCallRecord};
use crate::claude::{ContentBlock, Message, MessageContent, ResponseContentBlock, ToolDefinition};
use crate::error::FinallyAValueBotError;
use crate::multimodel::ModelTier;
use crate::telegram::{AgentEvent, AppState};
use crate::tools::ToolAuthContext;

use super::plan::{Plan, PlanStep};

const STEP_MAX_ITERATIONS: usize = 3;
const LLM_ROUND_TIMEOUT_SECS: u64 = 180;
const TOOL_EXECUTION_TIMEOUT_SECS: u64 = 3600;

const STEP_EXECUTE_PREAMBLE: &str = "[STEP EXECUTION]\n\
Execute ONLY this plan step. Call tools directly — do not re-plan the full task. \
If blocked, summarize what failed in one paragraph.\n\n";

#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub success: bool,
    pub summary: String,
    pub tool_names: Vec<String>,
    pub had_tool_errors: bool,
    pub iterations: Vec<IterationRecord>,
}

pub struct ExecuteContext<'a> {
    pub state: &'a AppState,
    pub tool_auth: &'a ToolAuthContext,
    pub base_system: &'a str,
    pub session_messages: &'a [Message],
    pub event_tx: Option<&'a UnboundedSender<AgentEvent>>,
    pub cancel: Option<Arc<AtomicBool>>,
}

pub async fn execute_plan(
    ctx: &ExecuteContext<'_>,
    plan: &Plan,
    use_local: bool,
) -> Result<Vec<StepResult>, FinallyAValueBotError> {
    let mut results = Vec::new();
    for step in &plan.steps {
        let mut attempt = run_step_once(ctx, step, use_local, false).await?;
        if !attempt.success {
            attempt = run_step_once(ctx, step, use_local, true).await?;
        }
        if !attempt.success && use_local {
            if let Some(fixed) = escalate_step(ctx, step, &attempt).await? {
                attempt = fixed;
            }
        }
        results.push(attempt);
    }
    Ok(results)
}

async fn run_step_once(
    ctx: &ExecuteContext<'_>,
    step: &PlanStep,
    use_local: bool,
    is_retry: bool,
) -> Result<StepResult, FinallyAValueBotError> {
    let tier = if use_local && ctx.state.llm.multimodel_config().local_routable() {
        ModelTier::Local
    } else {
        ModelTier::Strategy
    };
    let tool_defs = filter_tool_defs(ctx.state, &step.allowed_tools);
    let step_system = format!(
        "{STEP_EXECUTE_PREAMBLE}## Step {id}: {goal}\n\
         Expected output: {expected}\n\
         Verification: {verification}\n\
         Inputs/context: {inputs}\n\n\
         {base}",
        id = step.id,
        goal = step.goal,
        expected = step.expected_output,
        inputs = if step.inputs.is_empty() {
            "(use session context)".to_string()
        } else {
            step.inputs.clone()
        },
        verification = step.verification,
        base = ctx.base_system,
    );
    let mut messages: Vec<Message> = ctx.session_messages.to_vec();
    let retry_note = if is_retry {
        "\n\n[step_retry] Previous attempt did not meet verification. Try a different approach."
    } else {
        ""
    };
    messages.push(Message {
        role: "user".into(),
        content: MessageContent::Text(format!(
            "[step_contract id=\"{}\"]{}{}[/step_contract]",
            step.id, step.goal, retry_note
        )),
    });

    let mut history_iterations = Vec::new();
    let mut tool_names = Vec::new();
    let mut had_tool_errors = false;
    let mut last_assistant = String::new();

    for iteration in 0..STEP_MAX_ITERATIONS {
        if ctx
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            break;
        }

        let llm_fut = ctx.state.llm.send_message_for_tier(
            tier,
            &step_system,
            messages.clone(),
            Some(tool_defs.clone()),
        );
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(LLM_ROUND_TIMEOUT_SECS),
            llm_fut,
        )
        .await
        .map_err(|_| FinallyAValueBotError::LlmApi("step LLM timeout".into()))??;

        let stop_reason = response.stop_reason.as_deref().unwrap_or("end_turn");
        let assistant_text: String = response
            .content
            .iter()
            .filter_map(|b| match b {
                ResponseContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        last_assistant = assistant_text.clone();

        let tool_uses: Vec<_> = response
            .content
            .iter()
            .filter_map(|b| match b {
                ResponseContentBlock::ToolUse {
                    id, name, input, ..
                } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect();

        let effective_stop = if tool_uses.is_empty() {
            stop_reason
        } else {
            "tool_use"
        };

        let snap = ctx.state.llm.tier_endpoint_snapshot(tier);
        let (model_tier, provider, model, endpoint) =
            crate::agent_history::IterationRecord::tier_fields_from_snapshot(&snap);

        if effective_stop != "tool_use" {
            history_iterations.push(IterationRecord {
                iteration: iteration + 1,
                stop_reason: effective_stop.to_string(),
                assistant_text_preview: truncate_preview(&assistant_text, 200),
                tool_calls: vec![],
                hook_events: vec![],
                pte: None,
                model_tier,
                provider,
                model,
                endpoint,
            });
            break;
        }

        let assistant_blocks: Vec<ContentBlock> = response
            .content
            .iter()
            .map(|b| match b {
                ResponseContentBlock::Text { text } => ContentBlock::Text { text: text.clone() },
                ResponseContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    thought_signature,
                } => ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    thought_signature: thought_signature.clone(),
                },
            })
            .collect();
        messages.push(Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(assistant_blocks),
        });

        let mut tool_call_records = Vec::new();
        let mut tool_results = Vec::new();
        for (tool_id, name, input) in tool_uses {
            tool_names.push(name.clone());
            if let Some(tx) = ctx.event_tx {
                let _ = tx.send(AgentEvent::ToolStart {
                    tool_use_id: tool_id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
            }
            let started = Instant::now();
            let exec_fut = ctx
                .state
                .tools
                .execute_with_auth(&name, input, ctx.tool_auth);
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(TOOL_EXECUTION_TIMEOUT_SECS),
                exec_fut,
            )
            .await
            .map_err(|_| FinallyAValueBotError::ToolExecution(format!("{name} timed out")))?;
            if result.is_error {
                had_tool_errors = true;
            }
            let duration_ms = started.elapsed().as_millis();
            tool_call_records.push(ToolCallRecord {
                name: name.clone(),
                input_preview: truncate_preview(&result.content, 120),
                result_preview: truncate_preview(&result.content, 200),
                duration_ms,
                is_error: result.is_error,
            });
            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: result.content,
                is_error: if result.is_error { Some(true) } else { None },
            });
        }
        messages.push(Message {
            role: "user".into(),
            content: MessageContent::Blocks(tool_results),
        });
        history_iterations.push(IterationRecord {
            iteration: iteration + 1,
            stop_reason: "tool_use".into(),
            assistant_text_preview: truncate_preview(&assistant_text, 200),
            tool_calls: tool_call_records,
            hook_events: vec![],
            pte: None,
            model_tier,
            provider,
            model,
            endpoint,
        });
    }

    let summary = if last_assistant.trim().is_empty() {
        history_iterations
            .last()
            .and_then(|i| i.tool_calls.last())
            .map(|t| t.result_preview.clone())
            .unwrap_or_else(|| "(no output)".into())
    } else {
        last_assistant.clone()
    };

    let success = verify_step(step, &summary, had_tool_errors);

    Ok(StepResult {
        step_id: step.id.clone(),
        success,
        summary,
        tool_names,
        had_tool_errors,
        iterations: history_iterations,
    })
}

fn verify_step(step: &PlanStep, output: &str, had_tool_errors: bool) -> bool {
    if had_tool_errors {
        return false;
    }
    let lower_out = output.to_lowercase();
    let verification = step.verification.to_lowercase();
    if verification.contains("any") && verification.contains("success") {
        return !output.trim().is_empty();
    }
    if verification.contains("tool evidence") || verification.contains("evidence") {
        return output.trim().len() >= 20;
    }
    let keywords: Vec<&str> = step
        .goal
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .take(4)
        .collect();
    if keywords.is_empty() {
        return !output.trim().is_empty();
    }
    keywords
        .iter()
        .any(|k| lower_out.contains(&k.to_lowercase()))
}

async fn escalate_step(
    ctx: &ExecuteContext<'_>,
    step: &PlanStep,
    failed: &StepResult,
) -> Result<Option<StepResult>, FinallyAValueBotError> {
    let system =
        "You are a problem-solving assistant. Given a failed plan step, produce a revised \
        single-step execution brief in plain text (max 8 lines): what to try differently.";
    let user = format!(
        "Step goal: {}\nVerification: {}\nFailure summary: {}\nHad tool errors: {}",
        step.goal, step.verification, failed.summary, failed.had_tool_errors
    );
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user),
    }];
    let response = ctx
        .state
        .llm
        .send_message_for_tier(ModelTier::Strategy, system, messages, None)
        .await?;
    let guidance: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    if guidance.trim().is_empty() {
        return Ok(None);
    }
    let mut revised = step.clone();
    revised.inputs = format!("{}\n\nEscalation guidance:\n{}", step.inputs, guidance);
    let mut result = run_step_once(ctx, &revised, false, false).await?;
    result.step_id = step.id.clone();
    Ok(Some(result))
}

fn filter_tool_defs(state: &AppState, allowed: &[String]) -> Vec<ToolDefinition> {
    let allow: std::collections::HashSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
    state
        .tools
        .definitions()
        .into_iter()
        .filter(|d| allow.contains(d.name.as_str()))
        .collect()
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
