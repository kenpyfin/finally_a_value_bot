//! Deterministic problem-solving pipeline (web-selectable agent engine).

mod consolidate;
mod execute;
mod intent;
mod plan;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

use crate::agent_history::{
    EvaluatorStepRecord, IterationRecord, PipelineFinishExtras, PipelineStageRecord,
};
use crate::telegram::{
    pipeline_finish_turn, AgentEvent, AgentProcessResult, AgentRequestContext, AgentRunPrep,
    AppState,
};

pub use intent::{IntentCategory, IntentDecision};

pub async fn run_deterministic_pipeline(
    state: &AppState,
    context: AgentRequestContext<'_>,
    prep: AgentRunPrep,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<Arc<AtomicBool>>,
) -> anyhow::Result<AgentProcessResult> {
    let run_start = Instant::now();
    let use_local = state.llm.multimodel_config().ready_for_routing();
    let mut cloud_calls: u32 = 0;
    let mut pipeline_stages: Vec<PipelineStageRecord> = Vec::new();
    let mut history_iterations: Vec<IterationRecord> = Vec::new();
    let mut pdqe_steps: Vec<EvaluatorStepRecord> = Vec::new();
    let mut pdqe_retries: usize = 0;
    let mut agent_history_basename: Option<String> = None;
    let mut run_tool_names: Vec<String> = Vec::new();
    let mut messages = prep.messages.clone();

    macro_rules! record_stage {
        ($stage:expr, $detail:expr, $started:expr) => {
            pipeline_stages.push(PipelineStageRecord {
                stage: $stage.to_string(),
                detail: $detail.to_string(),
                duration_ms: $started.elapsed().as_millis(),
            });
        };
    }

    let t0 = Instant::now();
    let intent = intent::classify_intent(state, &prep.latest_user_text, prep.has_image_input)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    cloud_calls += 1;
    record_stage!(
        "intent",
        format!(
            "category={:?} clarify={} goal={}",
            intent.category, intent.needs_clarification, intent.restated_goal
        ),
        t0
    );

    if matches!(
        intent.category,
        IntentCategory::Conversational | IntentCategory::Question
    ) {
        cloud_calls += 1;
        let answer = consolidate::direct_answer(
            state,
            &prep.system_prompt,
            &messages,
            intent.category == IntentCategory::Question,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        return finish_pipeline(
            state,
            &context,
            event_tx,
            &prep,
            &mut messages,
            &mut pdqe_retries,
            &mut pdqe_steps,
            &mut history_iterations,
            &mut agent_history_basename,
            &run_tool_names,
            run_start,
            answer,
            "pipeline_direct",
            pipeline_stages,
            cloud_calls,
        )
        .await;
    }

    let proceed_despite_clarify =
        context.is_scheduled_task || context.is_background_job || context.caller_channel == "web";
    if intent.needs_clarification && !proceed_despite_clarify {
        let msg = intent::format_clarification_message(&intent);
        record_stage!("clarify", "asked_user".to_string(), Instant::now());
        return finish_pipeline(
            state,
            &context,
            event_tx,
            &prep,
            &mut messages,
            &mut pdqe_retries,
            &mut pdqe_steps,
            &mut history_iterations,
            &mut agent_history_basename,
            &run_tool_names,
            run_start,
            msg,
            "ask_clarification",
            pipeline_stages,
            cloud_calls,
        )
        .await;
    }
    if intent.needs_clarification && proceed_despite_clarify {
        record_stage!(
            "clarify",
            format!(
                "proceeded_on_assumptions: {}",
                intent.assumptions_if_proceeding.join("; ")
            ),
            Instant::now()
        );
    }

    let t1 = Instant::now();
    let plan = plan::resolve_plan(
        state,
        &intent,
        prep.persona_memory_state.as_ref(),
        &prep.latest_user_text,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    cloud_calls += 1;
    record_stage!("plan", plan::plan_summary(&plan), t1);

    let exec_ctx = execute::ExecuteContext {
        state,
        tool_auth: &prep.tool_auth,
        base_system: &prep.system_prompt,
        session_messages: &messages,
        event_tx,
        cancel: cancel.clone(),
    };
    let t2 = Instant::now();
    let step_results = execute::execute_plan(&exec_ctx, &plan, use_local)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for r in &step_results {
        run_tool_names.extend(r.tool_names.clone());
        history_iterations.extend(r.iterations.clone());
    }
    let failed = step_results.iter().filter(|r| !r.success).count();
    record_stage!(
        "execute",
        format!(
            "steps={} failed={} local={}",
            step_results.len(),
            failed,
            use_local
        ),
        t2
    );

    let t3 = Instant::now();
    let final_text = consolidate::synthesize_final(state, &intent, &plan, &step_results)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    cloud_calls += 1;
    record_stage!("consolidate", format!("chars={}", final_text.len()), t3);

    finish_pipeline(
        state,
        &context,
        event_tx,
        &prep,
        &mut messages,
        &mut pdqe_retries,
        &mut pdqe_steps,
        &mut history_iterations,
        &mut agent_history_basename,
        &run_tool_names,
        run_start,
        final_text,
        "pipeline_complete",
        pipeline_stages,
        cloud_calls,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn finish_pipeline(
    state: &AppState,
    context: &AgentRequestContext<'_>,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    prep: &AgentRunPrep,
    messages: &mut Vec<crate::claude::Message>,
    pdqe_retries: &mut usize,
    pdqe_steps: &mut Vec<EvaluatorStepRecord>,
    history_iterations: &mut Vec<IterationRecord>,
    agent_history_basename: &mut Option<String>,
    run_tool_names: &[String],
    run_start: Instant,
    mut response: String,
    stop_reason: &str,
    pipeline_stages: Vec<PipelineStageRecord>,
    cloud_calls: u32,
) -> anyhow::Result<AgentProcessResult> {
    let is_conversational = stop_reason == "ask_clarification" || stop_reason == "pipeline_direct";
    let extras = PipelineFinishExtras {
        pipeline_stages,
        cloud_calls,
    };

    loop {
        let mut iterations = history_iterations.clone();
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
            pdqe_retries,
            pdqe_steps,
            &mut iterations,
            &prep.principles_content,
            is_conversational,
            run_tool_names,
            agent_history_basename,
            !run_tool_names.is_empty(),
            &prep.user_msg_preview,
            run_start,
            &prep.initial_llm_snapshot_json,
            &prep.multimodel_run_summary,
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
                if *pdqe_retries > 1 {
                    return Ok(AgentProcessResult { response });
                }
                response = format!(
                    "{response}\n\n(Note: quality check requested revision; delivering best effort.)"
                );
            }
        }
    }
}
