//! Per-step local execution and verification for the deterministic pipeline.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures_util::future::join_all;
use tokio::sync::mpsc::UnboundedSender;

use crate::agent_history::{IterationRecord, ToolCallRecord};
use crate::agent_pipeline::profile::{
    OperationalConfig, PhaseContextIncludes, PolicyConfig, PriorStepFeedMode, ResolvedPhase,
    DEFAULT_PRIOR_STEP_SUMMARY_PROMPT,
};
use crate::claude::{ContentBlock, Message, MessageContent, ResponseContentBlock, ToolDefinition};
use crate::error::FinallyAValueBotError;
use crate::multimodel::ModelTier;
use crate::telegram::{AgentEvent, AppState};
use crate::tools::run_skill_script::{
    looks_like_skill_script_filename, runnable_script_hint_for_skill,
};
use crate::tools::{ToolAuthContext, ToolResult};

use super::plan::{Plan, PlanStep};
use super::skill_script_contract::{
    augment_skill_script_result as augment_skill_result, contract_required_flags_block,
    enrich_run_skill_script_input, PriorStepSnapshot,
};

const TOOL_EXECUTION_TIMEOUT_SECS: u64 = 3600;

const STEP_EXECUTE_PREAMBLE: &str = "[LOCAL STEP EXECUTION]\n\
You are the local executor. Follow the step contract below EXACTLY — do not re-plan, re-classify intent, or invent skills.\n\
Use only the skill_name/skill_script/skill_args_hint and inputs from the contract. \
If blocked, summarize what failed in one paragraph.\n\n";

pub fn builtin_step_execute_preamble() -> &'static str {
    STEP_EXECUTE_PREAMBLE
}

const SKILL_SCRIPT_PREAMBLE: &str = "Skill execution rules:\n\
- After activate_skill, run_skill_script must use a filename from SKILL.md ## Scripts or the step contract.\n\
- Never pass shell command names (ls, bash, sh, python3) as the script parameter.\n\
- Prefer the skill_script / inputs contract below when present.\n\
- Include every required CLI flag from the contract on each run_skill_script call (see script --help / SKILL.md).\n\n";

#[derive(Debug, Clone, Default)]
pub struct ExecuteStats {
    pub total_llm_rounds: u32,
    pub context_chars: usize,
    pub run_skill_script_calls: u32,
    pub invalid_script_calls: u32,
    pub skill_steps: u32,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub success: bool,
    pub summary: String,
    /// Full assistant + tool I/O log for handoff to later steps.
    pub full_output: String,
    pub tool_names: Vec<String>,
    pub had_tool_errors: bool,
    pub iterations: Vec<IterationRecord>,
}

#[derive(Debug, Clone)]
pub struct PriorStepFeedItem {
    pub step_id: String,
    pub success: bool,
    pub content: String,
}

pub struct ExecuteContext<'a> {
    pub state: &'a AppState,
    pub tool_auth: &'a ToolAuthContext,
    pub current_request: &'a str,
    pub event_tx: Option<&'a UnboundedSender<AgentEvent>>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub operational: Option<&'a OperationalConfig>,
    pub phase_preamble: Option<&'a str>,
    pub context_includes: Option<&'a PhaseContextIncludes>,
    pub agent_system_prompt: Option<&'a str>,
}

pub async fn execute_plan_with_config(
    ctx: &ExecuteContext<'_>,
    plan: &Plan,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: &OperationalConfig,
    policies: &PolicyConfig,
    use_local_override: bool,
) -> Result<(Vec<StepResult>, ExecuteStats), FinallyAValueBotError> {
    let use_local = if let Some(r) = resolved {
        matches!(
            crate::agent_pipeline::profile::resolve_model_tier(
                r.phase.model_route,
                ctx.state,
                &r.policies
            ),
            tier if tier.is_local()
        )
    } else {
        use_local_override
    };
    let mut results = Vec::new();
    let mut stats = ExecuteStats::default();
    let includes = ctx.context_includes.cloned().unwrap_or_else(|| {
        PhaseContextIncludes::defaults_for_kind(
            crate::agent_pipeline::profile::PhaseKind::ExecutePlan,
        )
    });
    for step in &plan.steps {
        if step.allowed_tools.iter().any(|t| t == "run_skill_script") {
            stats.skill_steps += 1;
        }
        let prior_feed = if includes.include_prior_step_summaries {
            prepare_prior_step_feed(ctx, &results, &includes, resolved).await?
        } else {
            Vec::new()
        };
        let mut attempt = run_step_once(
            ctx,
            step,
            use_local,
            false,
            &results,
            &prior_feed,
            &mut stats,
            resolved,
            operational,
        )
        .await?;
        if !attempt.success && policies.retry_failed_steps && should_retry_step(&attempt) {
            attempt = run_step_once(
                ctx,
                step,
                use_local,
                true,
                &results,
                &prior_feed,
                &mut stats,
                resolved,
                operational,
            )
            .await?;
        }
        if !attempt.success
            && use_local
            && policies.escalate_to_strategy_on_skill_failure
            && should_escalate_step(&attempt)
        {
            if let Some(fixed) = escalate_step(
                ctx,
                step,
                &attempt,
                &results,
                &prior_feed,
                &mut stats,
                operational,
                resolved,
            )
            .await?
            {
                attempt = fixed;
            }
        }
        results.push(attempt);
    }
    Ok((results, stats))
}

fn should_retry_step(failed: &StepResult) -> bool {
    if failed.had_tool_errors {
        return true;
    }
    !failed.summary.trim().is_empty() && !failed.tool_names.is_empty()
}

fn should_escalate_step(failed: &StepResult) -> bool {
    failed
        .tool_names
        .iter()
        .any(|n| n == "run_skill_script" || n == "activate_skill")
        || !failed.tool_names.is_empty()
}

async fn run_step_once(
    ctx: &ExecuteContext<'_>,
    step: &PlanStep,
    use_local: bool,
    is_retry: bool,
    prior_results: &[StepResult],
    prior_feed: &[PriorStepFeedItem],
    stats: &mut ExecuteStats,
    resolved: Option<&ResolvedPhase<'_>>,
    operational: &OperationalConfig,
) -> Result<StepResult, FinallyAValueBotError> {
    let tier = if let Some(r) = resolved {
        crate::agent_pipeline::profile::resolve_model_tier(
            r.phase.model_route,
            ctx.state,
            &r.policies,
        )
    } else if use_local && ctx.state.llm.multimodel_config().local_routable() {
        ModelTier::Local
    } else {
        ModelTier::Strategy
    };
    let max_iterations = if tier.is_local() {
        operational.max_iterations_local
    } else {
        operational.max_iterations
    };
    let includes = ctx.context_includes.cloned().unwrap_or_else(|| {
        PhaseContextIncludes::defaults_for_kind(
            crate::agent_pipeline::profile::PhaseKind::ExecutePlan,
        )
    });
    let tool_defs = filter_tool_defs(ctx.state, &step.allowed_tools);
    let step_system = build_step_system(
        step,
        ctx.state,
        ctx.phase_preamble,
        &includes,
        ctx.agent_system_prompt,
    );
    let mut messages =
        build_local_step_messages(ctx.current_request, step, prior_feed, is_retry, &includes);
    stats.context_chars = stats
        .context_chars
        .saturating_add(step_system.len() + message_chars(&messages));

    let mut history_iterations = Vec::new();
    let mut tool_names = Vec::new();
    let mut had_tool_errors = false;
    let mut last_assistant = String::new();
    let mut last_activated_skill: Option<String> = None;
    let mut consecutive_short_no_tool = 0usize;
    let mut full_output_log: Vec<String> = Vec::new();
    let full_output_max = includes.prior_step_full_output_max_chars;

    for iteration in 0..max_iterations {
        if ctx
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            break;
        }

        stats.total_llm_rounds += 1;
        let llm_fut = ctx.state.llm.send_message_for_tier(
            tier,
            &step_system,
            messages.clone(),
            Some(tool_defs.clone()),
        );
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(operational.llm_round_timeout_secs),
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
        if !assistant_text.trim().is_empty() {
            append_assistant_output(&mut full_output_log, iteration + 1, &assistant_text);
        }

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
            if assistant_text.trim().len() < operational.iteration_breaker_min_chars {
                consecutive_short_no_tool += 1;
            } else {
                consecutive_short_no_tool = 0;
            }
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
            if consecutive_short_no_tool >= 2 {
                break;
            }
            break;
        }
        consecutive_short_no_tool = 0;

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

        let (read_only, mutating): (Vec<_>, Vec<_>) = tool_uses
            .into_iter()
            .partition(|(_, name, _)| is_read_only_tool(name));

        let mut tool_call_records = Vec::new();
        let mut tool_results = Vec::new();

        if !read_only.is_empty() {
            let prior_snapshots = prior_step_snapshots(prior_results);
            let parallel = join_all(read_only.into_iter().map(|(tool_id, name, input)| {
                execute_tool_call(ctx, tool_id, name, input, step, None, &prior_snapshots)
            }))
            .await;
            for (tool_id, name, input, record, result) in parallel {
                append_tool_output(&mut full_output_log, &name, &input, &result);
                apply_tool_stats(name.as_str(), &record, stats);
                tool_names.push(name);
                if record.is_error {
                    had_tool_errors = true;
                }
                tool_call_records.push(record);
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: result.content,
                    is_error: if result.is_error { Some(true) } else { None },
                });
            }
        }

        for (tool_id, name, input) in mutating {
            let prior_snapshots = prior_step_snapshots(prior_results);
            let (tid, n, input, record, result) = execute_tool_call(
                ctx,
                tool_id,
                name,
                input,
                step,
                Some(&mut last_activated_skill),
                &prior_snapshots,
            )
            .await;
            append_tool_output(&mut full_output_log, &n, &input, &result);
            apply_tool_stats(n.as_str(), &record, stats);
            tool_names.push(n);
            if record.is_error {
                had_tool_errors = true;
            }
            tool_call_records.push(record);
            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: tid,
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

    let summary = build_step_summary(&last_assistant, &history_iterations, had_tool_errors);
    let full_output = truncate_preview(&full_output_log.join("\n"), full_output_max);

    let success = verify_step(step, &summary, had_tool_errors);

    Ok(StepResult {
        step_id: step.id.clone(),
        success,
        summary,
        full_output,
        tool_names,
        had_tool_errors,
        iterations: history_iterations,
    })
}

async fn execute_tool_call(
    ctx: &ExecuteContext<'_>,
    tool_id: String,
    name: String,
    input: serde_json::Value,
    step: &PlanStep,
    last_activated_skill: Option<&mut Option<String>>,
    prior_results: &[PriorStepSnapshot],
) -> (
    String,
    String,
    serde_json::Value,
    ToolCallRecord,
    ToolResult,
) {
    if let Some(tx) = ctx.event_tx {
        let _ = tx.send(AgentEvent::ToolStart {
            tool_use_id: tool_id.clone(),
            name: name.clone(),
            input: input.clone(),
        });
    }
    let started = Instant::now();
    let mut input = input;
    let input_for_log = input.clone();
    let result = if name == "activate_skill" {
        if let Some(slot) = last_activated_skill {
            if let Some(skill) = input.get("skill_name").and_then(|v| v.as_str()) {
                *slot = Some(skill.to_string());
            }
        }
        run_tool_with_timeout(ctx, &name, &input, tool_timeout_secs(ctx)).await
    } else if name == "run_skill_script" {
        maybe_correct_skill_script_input(
            ctx,
            step,
            &mut input,
            last_activated_skill.map(|s| &*s),
            prior_results,
        )
        .await
    } else {
        run_tool_with_timeout(ctx, &name, &input, tool_timeout_secs(ctx)).await
    };
    let duration_ms = started.elapsed().as_millis();
    let record = ToolCallRecord {
        name: name.clone(),
        input_preview: truncate_preview(&input.to_string(), 120),
        result_preview: truncate_preview(&result.content, 200),
        duration_ms,
        is_error: result.is_error,
    };
    (tool_id, name, input_for_log, record, result)
}

fn apply_tool_stats(name: &str, record: &ToolCallRecord, stats: &mut ExecuteStats) {
    if name == "run_skill_script" {
        stats.run_skill_script_calls += 1;
        if record.is_error
            && (record.result_preview.contains("Invalid run_skill_script")
                || record.result_preview.contains("must be a filename")
                || record.result_preview.contains("Use a skill filename"))
        {
            stats.invalid_script_calls += 1;
        }
    }
}

async fn maybe_correct_skill_script_input(
    ctx: &ExecuteContext<'_>,
    step: &PlanStep,
    input: &mut serde_json::Value,
    last_activated_skill: Option<&Option<String>>,
    prior_results: &[PriorStepSnapshot],
) -> ToolResult {
    enrich_run_skill_script_input(ctx.state, input, step, prior_results);
    let script = input
        .get("script")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let skill_name = input
        .get("skill_name")
        .and_then(|v| v.as_str())
        .or(step.skill_name.as_deref())
        .or(last_activated_skill.and_then(|s| s.as_deref()))
        .map(str::to_string);
    if !script.is_empty() && looks_like_skill_script_filename(&script) {
        let result =
            run_tool_with_timeout(ctx, "run_skill_script", input, tool_timeout_secs(ctx)).await;
        return augment_skill_result(ctx.state, skill_name.as_deref(), &script, result);
    }
    let contract_script = step.skill_script.as_deref();
    if let (Some(skill), Some(script)) = (skill_name.as_deref(), contract_script) {
        let skill_owned = skill.to_string();
        let script_owned = script.to_string();
        input["skill_name"] = serde_json::Value::String(skill_owned.clone());
        input["script"] = serde_json::Value::String(script_owned.clone());
        enrich_run_skill_script_input(ctx.state, input, step, prior_results);
        let result =
            run_tool_with_timeout(ctx, "run_skill_script", input, tool_timeout_secs(ctx)).await;
        return augment_skill_result(ctx.state, Some(&skill_owned), &script_owned, result);
    }
    if let Some(skill) = skill_name.as_deref() {
        if let Some(hint) = runnable_script_hint_for_skill(ctx.state, skill) {
            return ToolResult::error(format!(
                "Invalid run_skill_script script '{script}'. Use a skill filename (e.g. hotify_cli.py), not a shell command. {hint}"
            ));
        }
    }
    ToolResult::error(format!(
        "Invalid run_skill_script script '{script}': must be a filename under the skill directory (e.g. hotify_cli.py). \
         Never use find, ls, bash, or '.' as script. Activate the skill first and use ## Scripts from SKILL.md."
    ))
}

fn prior_step_snapshots(prior_results: &[StepResult]) -> Vec<PriorStepSnapshot> {
    prior_results
        .iter()
        .map(|result| PriorStepSnapshot {
            summary: result.summary.clone(),
            full_output: result.full_output.clone(),
            tool_input_previews: result
                .iterations
                .iter()
                .flat_map(|i| i.tool_calls.iter().map(|t| t.input_preview.clone()))
                .collect(),
            tool_result_previews: result
                .iterations
                .iter()
                .flat_map(|i| i.tool_calls.iter().map(|t| t.result_preview.clone()))
                .collect(),
        })
        .collect()
}

fn append_assistant_output(log: &mut Vec<String>, round: usize, text: &str) {
    log.push(format!("## Assistant (round {round})\n{}\n", text.trim()));
}

fn append_tool_output(
    log: &mut Vec<String>,
    name: &str,
    input: &serde_json::Value,
    result: &ToolResult,
) {
    log.push(format!(
        "### Tool: {name}\nInput:\n{}\nOutput:\n{}\n",
        input, result.content
    ));
}

async fn prepare_prior_step_feed(
    ctx: &ExecuteContext<'_>,
    prior_results: &[StepResult],
    includes: &PhaseContextIncludes,
    resolved: Option<&ResolvedPhase<'_>>,
) -> Result<Vec<PriorStepFeedItem>, FinallyAValueBotError> {
    let mut feed = Vec::with_capacity(prior_results.len());
    for result in prior_results {
        let raw = if result.full_output.trim().is_empty() {
            result.summary.clone()
        } else {
            result.full_output.clone()
        };
        let content = match includes.prior_step_feed_mode {
            PriorStepFeedMode::Full => raw,
            PriorStepFeedMode::Summary => {
                summarize_prior_step_output(ctx, includes, resolved, &raw).await?
            }
        };
        feed.push(PriorStepFeedItem {
            step_id: result.step_id.clone(),
            success: result.success,
            content,
        });
    }
    Ok(feed)
}

async fn summarize_prior_step_output(
    ctx: &ExecuteContext<'_>,
    includes: &PhaseContextIncludes,
    resolved: Option<&ResolvedPhase<'_>>,
    full_output: &str,
) -> Result<String, FinallyAValueBotError> {
    if full_output.trim().is_empty() {
        return Ok(String::new());
    }
    let system = if includes.prior_step_summary_prompt.trim().is_empty() {
        DEFAULT_PRIOR_STEP_SUMMARY_PROMPT.to_string()
    } else {
        includes.prior_step_summary_prompt.clone()
    };
    let tier = if let Some(r) = resolved {
        crate::agent_pipeline::profile::resolve_model_tier(
            r.phase.model_route,
            ctx.state,
            &r.policies,
        )
    } else {
        ModelTier::Strategy
    };
    let timeout_secs = ctx.operational.map(|o| o.timeout_secs).unwrap_or(45);
    let user_body = format!(
        "Prior step output to summarize:\n\n{}",
        truncate_preview(full_output, includes.prior_step_full_output_max_chars)
    );
    let messages = vec![Message {
        role: "user".into(),
        content: MessageContent::Text(user_body),
    }];
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        ctx.state
            .llm
            .send_message_for_tier(tier, &system, messages, None),
    )
    .await
    .map_err(|_| FinallyAValueBotError::LlmApi("prior step summary timeout".into()))??;
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
        Ok(truncate_preview(full_output, 4000))
    } else {
        Ok(text)
    }
}

async fn run_tool_with_timeout(
    ctx: &ExecuteContext<'_>,
    name: &str,
    input: &serde_json::Value,
    timeout_secs: u64,
) -> ToolResult {
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        ctx.state
            .tools
            .execute_with_auth(name, input.clone(), ctx.tool_auth),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => ToolResult::error(format!("{name} timed out")),
    }
}

fn build_step_summary(
    last_assistant: &str,
    history_iterations: &[IterationRecord],
    had_tool_errors: bool,
) -> String {
    if !last_assistant.trim().is_empty() {
        return last_assistant.to_string();
    }
    if had_tool_errors {
        if let Some(stderr) = history_iterations
            .iter()
            .flat_map(|i| i.tool_calls.iter())
            .filter(|t| t.is_error)
            .map(|t| t.result_preview.as_str())
            .next_back()
        {
            return format!("Tool error: {stderr}");
        }
    }
    history_iterations
        .last()
        .and_then(|i| i.tool_calls.last())
        .map(|t| t.result_preview.clone())
        .unwrap_or_else(|| "(no output)".into())
}

fn tool_timeout_secs(ctx: &ExecuteContext<'_>) -> u64 {
    ctx.operational
        .map(|o| o.tool_execution_timeout_secs)
        .unwrap_or(TOOL_EXECUTION_TIMEOUT_SECS)
}

fn build_step_system(
    step: &PlanStep,
    state: &AppState,
    phase_preamble: Option<&str>,
    includes: &PhaseContextIncludes,
    agent_system_prompt: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if includes.include_agent_system_prompt {
        if let Some(p) = agent_system_prompt.filter(|s| !s.trim().is_empty()) {
            parts.push(p.to_string());
        }
    }
    if includes.include_system_prompt {
        let preamble = phase_preamble.unwrap_or(STEP_EXECUTE_PREAMBLE);
        let skill_block = if step.allowed_tools.iter().any(|t| t == "run_skill_script") {
            SKILL_SCRIPT_PREAMBLE
        } else {
            ""
        };
        parts.push(format!("{preamble}{skill_block}"));
    }
    if includes.include_step_contract {
        let contract = step_skill_contract_block(step, state);
        parts.push(format!(
            "## Step {id}: {goal}\n\
             Expected output: {expected}\n\
             Verification: {verification}\n\
             Inputs/context: {inputs}\n\
             {contract}",
            id = step.id,
            goal = step.goal,
            expected = step.expected_output,
            inputs = if step.inputs.is_empty() {
                "(follow contract fields above)".to_string()
            } else {
                step.inputs.clone()
            },
            verification = step.verification,
            contract = contract,
        ));
    }
    if parts.is_empty() {
        crate::agent_pipeline::profile::MINIMAL_SYSTEM_STUB.to_string()
    } else {
        parts.join("\n\n")
    }
}

fn build_local_step_messages(
    current_request: &str,
    step: &PlanStep,
    prior_feed: &[PriorStepFeedItem],
    is_retry: bool,
    includes: &PhaseContextIncludes,
) -> Vec<Message> {
    let mut messages = Vec::new();
    if includes.include_current_request {
        messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text(format!(
                "[current_request]\n{}\n[/current_request]",
                current_request.chars().take(2000).collect::<String>()
            )),
        });
    }
    if includes.include_prior_step_summaries && !prior_feed.is_empty() {
        let blocks: Vec<String> = prior_feed
            .iter()
            .map(|item| {
                format!(
                    "[prior_step_output id=\"{}\" status=\"{}\"]\n{}\n[/prior_step_output]",
                    item.step_id,
                    if item.success { "OK" } else { "FAILED" },
                    item.content
                )
            })
            .collect();
        messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text(blocks.join("\n\n")),
        });
    }
    let retry_note = if is_retry {
        "\n\n[step_retry] Previous attempt did not meet verification. Follow the contract exactly; try a different tool invocation."
    } else {
        ""
    };
    if includes.include_step_contract {
        let contract_detail = format!(
            "goal={}\ninputs={}\nexpected_output={}\nverification={}{}{}{}",
            step.goal,
            step.inputs,
            step.expected_output,
            step.verification,
            step.skill_name
                .as_ref()
                .map(|s| format!("\nskill_name={s}"))
                .unwrap_or_default(),
            step.skill_script
                .as_ref()
                .map(|s| format!("\nskill_script={s}"))
                .unwrap_or_default(),
            step.skill_args_hint
                .as_ref()
                .map(|s| format!("\nskill_args_hint={s}"))
                .unwrap_or_default(),
        );
        messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text(format!(
                "[step_contract id=\"{}\"]\n{}{}\n[/step_contract]",
                step.id, contract_detail, retry_note
            )),
        });
    } else if messages.is_empty() {
        messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text("Execute the assigned step.".into()),
        });
    }
    messages
}

fn step_skill_contract_block(step: &PlanStep, state: &AppState) -> String {
    let mut lines = Vec::new();
    if let Some(skill) = &step.skill_name {
        lines.push(format!("Contract skill_name: {skill}"));
    }
    if let Some(script) = &step.skill_script {
        lines.push(format!("Contract skill_script: {script}"));
    }
    if let Some(args) = &step.skill_args_hint {
        lines.push(format!("Contract args hint: {args}"));
    }
    if let Some(required) = contract_required_flags_block(state, step) {
        lines.push(required);
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn message_chars(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.len(),
            MessageContent::Blocks(b) => b
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => text.len(),
                    _ => 0,
                })
                .sum(),
        })
        .sum()
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
    prior_results: &[StepResult],
    prior_feed: &[PriorStepFeedItem],
    stats: &mut ExecuteStats,
    operational: &OperationalConfig,
    resolved: Option<&ResolvedPhase<'_>>,
) -> Result<Option<StepResult>, FinallyAValueBotError> {
    let system =
        "You are a problem-solving assistant. Given a failed plan step, produce a revised \
        single-step execution brief in plain text (max 8 lines): what to try differently.";
    let user = format!(
        "Step goal: {}\nVerification: {}\nFailure summary: {}\nHad tool errors: {}\nTool trace: {}",
        step.goal,
        step.verification,
        failed.summary,
        failed.had_tool_errors,
        failed.tool_names.join(", ")
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
    let mut result = run_step_once(
        ctx,
        &revised,
        false,
        false,
        prior_results,
        prior_feed,
        stats,
        resolved,
        operational,
    )
    .await?;
    result.step_id = step.id.clone();
    Ok(Some(result))
}

fn is_read_only_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "grep" | "glob" | "search_vault" | "read_repo_map" | "agent_history"
    )
}

fn filter_tool_defs(state: &AppState, allowed: &[String]) -> Vec<ToolDefinition> {
    let allow: HashSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
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
