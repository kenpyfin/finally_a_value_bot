//! Deterministic problem-solving pipeline (web-selectable agent engine).

mod cloud_context;
mod consolidate;
mod execute;
mod intent;
mod plan;
pub mod profile;
mod runner;
mod skill_script_contract;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;

use crate::telegram::{
    AgentEvent, AgentProcessResult, AgentRequestContext, AgentRunPrep, AppState,
};

pub use intent::{IntentCategory, IntentDecision};
pub use profile::PipelineProfile;

pub async fn run_deterministic_pipeline(
    state: &AppState,
    context: AgentRequestContext<'_>,
    prep: AgentRunPrep,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<Arc<AtomicBool>>,
) -> anyhow::Result<AgentProcessResult> {
    let profile = state
        .pipeline_profile
        .read()
        .map_err(|_| anyhow::anyhow!("pipeline profile lock poisoned"))?
        .clone();
    runner::run_profiled_pipeline(state, context, prep, event_tx, cancel, &profile).await
}
