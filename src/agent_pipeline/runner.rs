//! Config-driven phase interpreter for the deterministic pipeline.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

use crate::agent_history::{
    EvaluatorStepRecord, IterationRecord, PipelineFinishExtras, PipelineStageRecord,
};
use crate::error::FinallyAValueBotError;
use crate::skills::SkillsCatalogMode;
use crate::telegram::{
    pipeline_finish_turn, AgentEvent, AgentProcessResult, AgentRequestContext, AgentRunPrep,
    AppState,
};

use super::cloud_context::{self, PipelineCloudContext};
use super::consolidate;
use super::execute::{self, ExecuteContext, StepResult};
use super::intent::{self, IntentCategory, IntentDecision};
use super::plan::{self, Plan};
use super::profile::{
    self, PhaseKind, PipelineProfile, ResolvedPhase, TransitionEvalContext, TransitionTarget,
};

struct PipelineRunState {
    intent: Option<IntentDecision>,
    plan: Option<Plan>,
    step_results: Vec<StepResult>,
    final_response: Option<String>,
    last_phase_extra: String,
    cloud_calls: u32,
    heuristic_intent: bool,
    merged_intent_plan: bool,
    skipped_consolidate: bool,
    history_iterations: Vec<IterationRecord>,
    run_tool_names: Vec<String>,
    pipeline_stages: Vec<PipelineStageRecord>,
}

impl PipelineRunState {
    fn stage_detail_suffix(&self) -> String {
        let category = self
            .intent
            .as_ref()
            .map(|i| format!("{:?}", i.category))
            .unwrap_or_else(|| "none".into());
        format!(
            "category={category} heuristic={} merged_intent_plan={} skipped_consolidate={} cloud_calls={}",
            self.heuristic_intent, self.merged_intent_plan, self.skipped_consolidate, self.cloud_calls
        )
    }
}

pub async fn run_profiled_pipeline(
    state: &AppState,
    context: AgentRequestContext<'_>,
    prep: AgentRunPrep,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<Arc<AtomicBool>>,
    profile: &PipelineProfile,
) -> anyhow::Result<AgentProcessResult> {
    let run_start = Instant::now();
    let mut run = PipelineRunState {
        intent: None,
        plan: None,
        step_results: Vec::new(),
        final_response: None,
        last_phase_extra: String::new(),
        cloud_calls: 0,
        heuristic_intent: false,
        merged_intent_plan: false,
        skipped_consolidate: false,
        history_iterations: Vec::new(),
        run_tool_names: Vec::new(),
        pipeline_stages: Vec::new(),
    };
    let mut messages = prep.messages.clone();
    let session_max = cloud_context::DEFAULT_CLOUD_SESSION_EXCERPT_MAX_CHARS;
    let cloud_ctx = cloud_context::build_pipeline_cloud_context(
        state,
        &prep,
        session_max,
        SkillsCatalogMode::Full,
    )
    .await;

    let mut current_phase_id = profile.entry_phase_id.clone();
    let mut guard = 0usize;

    loop {
        guard += 1;
        if guard > 32 {
            return Err(anyhow::anyhow!("pipeline exceeded phase visit budget"));
        }

        let Some(phase) = profile.phase_by_id(&current_phase_id) else {
            return Err(anyhow::anyhow!(
                "unknown pipeline phase '{current_phase_id}'"
            ));
        };
        if !phase.enabled {
            return Err(anyhow::anyhow!("pipeline phase '{}' is disabled", phase.id));
        }

        let resolved = profile.resolve_phase(phase);
        let t0 = Instant::now();
        run.last_phase_extra.clear();
        run_phase(
            state,
            &prep,
            &context,
            &cloud_ctx,
            event_tx,
            cancel.as_ref(),
            &resolved,
            &mut run,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        run.pipeline_stages.push(PipelineStageRecord {
            stage: phase.label.clone(),
            detail: format!(
                "phase_id={} kind={:?} {} {}",
                phase.id,
                phase.kind,
                run.stage_detail_suffix(),
                run.last_phase_extra
            ),
            duration_ms: t0.elapsed().as_millis(),
        });

        let transition_ctx = TransitionEvalContext {
            intent: run.intent.as_ref(),
            plan_empty: run.plan.as_ref().is_none_or(|p| p.steps.is_empty()),
            execute_any_failed: run.step_results.iter().any(|r| !r.success),
            execute_all_succeeded: !run.step_results.is_empty()
                && run.step_results.iter().all(|r| r.success),
            caller_channel: context.caller_channel,
            is_scheduled_task: context.is_scheduled_task,
            is_background_job: context.is_background_job,
        };

        if run.intent.as_ref().is_some_and(|i| i.needs_clarification)
            && profile::should_proceed_despite_clarify(&transition_ctx, &profile.policies)
        {
            run.pipeline_stages.push(PipelineStageRecord {
                stage: "clarify".into(),
                detail: format!(
                    "proceeded_on_assumptions: {}",
                    run.intent
                        .as_ref()
                        .map(|i| i.assumptions_if_proceeding.join("; "))
                        .unwrap_or_default()
                ),
                duration_ms: 0,
            });
        }

        let target = profile::resolve_transition(phase, &transition_ctx, &profile.policies)
            .ok_or_else(|| anyhow::anyhow!("no matching transition for phase '{}'", phase.id))?;

        match target {
            TransitionTarget::DirectAnswer => {
                let intent = run
                    .intent
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("direct_answer terminal without intent"))?;
                run.cloud_calls += 1;
                let includes = &phase.context_includes;
                let system = profile::compose_system_prompt(
                    phase,
                    phase.kind,
                    Some(&prep.system_prompt),
                    includes,
                );
                let answer = consolidate::direct_answer(
                    state,
                    &system,
                    &messages,
                    intent.category == IntentCategory::Question,
                    includes,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
                return finish_pipeline(
                    state,
                    &context,
                    event_tx,
                    &prep,
                    &mut messages,
                    &mut run,
                    run_start,
                    answer,
                    "pipeline_direct",
                )
                .await;
            }
            TransitionTarget::Clarify => {
                let intent = run
                    .intent
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("clarify terminal without intent"))?;
                let msg = intent::format_clarification_message(&intent);
                return finish_pipeline(
                    state,
                    &context,
                    event_tx,
                    &prep,
                    &mut messages,
                    &mut run,
                    run_start,
                    msg,
                    "ask_clarification",
                )
                .await;
            }
            TransitionTarget::Finish => {
                let final_text = run
                    .final_response
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("finish terminal without response"))?;
                return finish_pipeline(
                    state,
                    &context,
                    event_tx,
                    &prep,
                    &mut messages,
                    &mut run,
                    run_start,
                    final_text,
                    "pipeline_complete",
                )
                .await;
            }
            TransitionTarget::Phase(next) => {
                current_phase_id = next;
            }
        }
    }
}

async fn run_phase(
    state: &AppState,
    prep: &AgentRunPrep,
    _context: &AgentRequestContext<'_>,
    cloud_ctx: &PipelineCloudContext,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<&Arc<AtomicBool>>,
    resolved: &ResolvedPhase<'_>,
    run: &mut PipelineRunState,
) -> Result<(), FinallyAValueBotError> {
    match resolved.phase.kind {
        PhaseKind::IntentClassify => {
            run_intent_phase(state, prep, cloud_ctx, resolved, run).await?;
        }
        PhaseKind::PlanGenerate => {
            run_plan_phase(state, prep, cloud_ctx, resolved, run).await?;
        }
        PhaseKind::ExecutePlan => {
            let plan = run.plan.clone().ok_or_else(|| {
                FinallyAValueBotError::Config("execute phase without plan".into())
            })?;
            let exec_ctx = ExecuteContext {
                state,
                tool_auth: &prep.tool_auth,
                current_request: &prep.latest_user_text,
                event_tx,
                cancel: cancel.cloned(),
                operational: Some(&resolved.operational),
                phase_preamble: resolved.phase.preamble.as_deref(),
                context_includes: Some(&resolved.phase.context_includes),
                agent_system_prompt: Some(&prep.system_prompt),
            };
            let use_local = state.llm.local_delegate_config().ready_for_routing();
            let (_step_results, _exec_stats) = execute::execute_plan_with_config(
                &exec_ctx,
                &plan,
                Some(resolved),
                &resolved.operational,
                &resolved.policies,
                use_local,
            )
            .await?;
            for r in &_step_results {
                run.run_tool_names.extend(r.tool_names.clone());
                run.history_iterations.extend(r.iterations.clone());
            }
            run.step_results = _step_results;
        }
        PhaseKind::SynthesizeDelivery => {
            let intent = run.intent.clone().ok_or_else(|| {
                FinallyAValueBotError::Config("consolidate phase without intent".into())
            })?;
            let plan = run.plan.clone().unwrap_or(Plan {
                source: "empty".into(),
                vault_path: None,
                steps: vec![],
            });
            let skip = !consolidate::should_synthesize_final_with_config(
                &plan,
                &run.step_results,
                &resolved.operational,
                resolved.policies.skip_consolidate_when_good,
            );
            let final_text = if skip {
                run.skipped_consolidate = true;
                run.cloud_calls += 1;
                let draft = consolidate::fallback_summary(&run.step_results);
                consolidate::polish_delivery_with_config(
                    state,
                    &intent,
                    &draft,
                    &prep.latest_user_text,
                    Some(resolved),
                    &resolved.operational,
                    Some(&prep.system_prompt),
                )
                .await?
            } else {
                run.cloud_calls += 1;
                consolidate::synthesize_final_with_config(
                    state,
                    &intent,
                    &plan,
                    &run.step_results,
                    &prep.latest_user_text,
                    Some(resolved),
                    Some(&prep.system_prompt),
                )
                .await?
            };
            run.final_response = Some(final_text);
        }
    }
    Ok(())
}

async fn run_intent_phase(
    state: &AppState,
    prep: &AgentRunPrep,
    cloud_ctx: &PipelineCloudContext,
    resolved: &ResolvedPhase<'_>,
    run: &mut PipelineRunState,
) -> Result<(), FinallyAValueBotError> {
    if prep.has_image_input && resolved.policies.image_input_force_task {
        run.heuristic_intent = true;
        run.intent = Some(IntentDecision {
            category: IntentCategory::Task,
            restated_goal: prep.latest_user_text.chars().take(500).collect(),
            success_criteria: vec!["Process the attached image per the user request.".into()],
            needs_clarification: false,
            clarifying_questions: vec![],
            candidate_sop_hint: None,
            assumptions_if_proceeding: vec![],
        });
        return Ok(());
    }

    if resolved.policies.merged_classify_and_plan_enabled {
        if let Some((outcome, plan)) = plan::try_classify_and_plan_with_config(
            state,
            &prep.latest_user_text,
            prep.persona_memory_state.as_ref(),
            None,
            Some(cloud_ctx),
            Some(resolved),
            Some(&resolved.operational),
            Some(&resolved.policies),
            Some(&prep.system_prompt),
        )
        .await?
        {
            run.merged_intent_plan = true;
            run.cloud_calls += outcome.cloud_calls;
            run.intent = Some(outcome.decision);
            run.plan = Some(plan);
            return Ok(());
        }
    }

    let outcome = intent::classify_intent_with_config(
        state,
        &prep.latest_user_text,
        prep.has_image_input,
        Some(cloud_ctx),
        Some(resolved),
        Some(&resolved.operational),
        Some(&resolved.policies),
        Some(&prep.system_prompt),
    )
    .await?;
    run.heuristic_intent = outcome.heuristic;
    run.cloud_calls += outcome.cloud_calls;
    run.intent = Some(outcome.decision);
    Ok(())
}

async fn run_plan_phase(
    state: &AppState,
    prep: &AgentRunPrep,
    cloud_ctx: &PipelineCloudContext,
    resolved: &ResolvedPhase<'_>,
    run: &mut PipelineRunState,
) -> Result<(), FinallyAValueBotError> {
    if run.plan.is_some() {
        return Ok(());
    }
    let intent = run
        .intent
        .clone()
        .ok_or_else(|| FinallyAValueBotError::Config("plan phase without intent".into()))?;
    run.cloud_calls += 1;
    let plan = plan::resolve_plan_with_config(
        state,
        &intent,
        prep.persona_memory_state.as_ref(),
        &prep.latest_user_text,
        Some(cloud_ctx),
        Some(resolved),
        Some(&resolved.operational),
        Some(&resolved.policies),
        Some(&prep.system_prompt),
    )
    .await?;
    run.plan = Some(plan);
    run.last_phase_extra = plan::plan_summary(run.plan.as_ref().expect("plan set"));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn finish_pipeline(
    state: &AppState,
    context: &AgentRequestContext<'_>,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    prep: &AgentRunPrep,
    messages: &mut Vec<crate::claude::Message>,
    run: &mut PipelineRunState,
    run_start: Instant,
    response: String,
    stop_reason: &str,
) -> anyhow::Result<AgentProcessResult> {
    let is_conversational = stop_reason == "ask_clarification" || stop_reason == "pipeline_direct";
    let extras = PipelineFinishExtras {
        pipeline_stages: run.pipeline_stages.clone(),
        cloud_calls: run.cloud_calls,
        agent_engine: "deterministic".into(),
    };
    let mut pdqe_retries = 0usize;
    let mut pdqe_steps: Vec<EvaluatorStepRecord> = Vec::new();
    let mut agent_history_basename: Option<String> = None;

    loop {
        let mut iterations = run.history_iterations.clone();
        match pipeline_finish_turn(
            state,
            context,
            event_tx,
            &prep.run_key,
            context.chat_id,
            context.persona_id,
            stop_reason,
            response.clone(),
            &prep.system_prompt,
            messages,
            prep.protected_message_count,
            &mut pdqe_retries,
            &mut pdqe_steps,
            &mut iterations,
            &prep.principles_content,
            is_conversational,
            &run.run_tool_names,
            &mut agent_history_basename,
            !run.run_tool_names.is_empty(),
            &prep.user_msg_preview,
            run_start,
            &prep.initial_llm_snapshot_json,
            &prep.local_delegate_run_summary,
            Some(&extras),
        )
        .await?
        {
            Some(result) => {
                info!(
                    chat_id = context.chat_id,
                    persona_id = context.persona_id,
                    stop_reason,
                    cloud_calls = extras.cloud_calls,
                    "Deterministic pipeline finished"
                );
                return Ok(result);
            }
            None => {
                // PDQE requested revision; re-enter the finish loop until the gate delivers.
            }
        }
    }
}
