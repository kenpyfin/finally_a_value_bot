use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, UnboundedSender};
use tracing::warn;

use crate::channel::deliver_to_contact;
use crate::db::call_blocking;
use crate::telegram::{AgentEvent, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobType {
    ManualBackground,
    Scheduled,
}

impl JobType {
    fn as_str(self) -> &'static str {
        match self {
            JobType::ManualBackground => "manual_background",
            JobType::Scheduled => "scheduled",
        }
    }

    fn notify_user_periodically(self) -> bool {
        matches!(self, JobType::ManualBackground)
    }
}

#[derive(Debug, Clone)]
pub enum HeartbeatSignal {
    Started(String),
    Progress(String),
    ToolStart(String),
    ToolResult { tool: String, is_error: bool },
    Finished(String),
    Failed(String),
}

pub fn signal_from_agent_event(evt: &AgentEvent) -> Option<HeartbeatSignal> {
    match evt {
        AgentEvent::Iteration { iteration } => Some(HeartbeatSignal::Progress(format!(
            "iteration {}",
            iteration
        ))),
        AgentEvent::WorkflowSelected {
            workflow_id,
            confidence,
        } => Some(HeartbeatSignal::Progress(format!(
            "selected workflow {} (confidence {:.2})",
            workflow_id, confidence
        ))),
        AgentEvent::ToolStart { name, .. } => Some(HeartbeatSignal::ToolStart(name.clone())),
        AgentEvent::ToolResult { name, is_error, .. } => Some(HeartbeatSignal::ToolResult {
            tool: name.clone(),
            is_error: *is_error,
        }),
        AgentEvent::TextDelta { .. } => None,
        AgentEvent::FinalResponse { .. } => None,
    }
}

pub fn spawn_shared_heartbeat(
    state: Arc<AppState>,
    run_key: String,
    chat_id: i64,
    persona_id: i64,
    job_type: JobType,
) -> UnboundedSender<HeartbeatSignal> {
    let (tx, mut rx) = mpsc::unbounded_channel::<HeartbeatSignal>();
    tokio::spawn(async move {
        let heartbeat_period = Duration::from_secs(30);
        let mut ticker = tokio::time::interval(heartbeat_period);
        let mut stage = "queued".to_string();
        let mut message = "queued".to_string();
        let mut active = true;
        let mut last_user_notify = Instant::now()
            .checked_sub(heartbeat_period)
            .unwrap_or_else(Instant::now);

        let _ = call_blocking(state.db.clone(), {
            let run_key = run_key.clone();
            let stage = stage.clone();
            let message = message.clone();
            let job_type = job_type.as_str().to_string();
            move |db| {
                db.upsert_job_heartbeat(
                    &run_key, chat_id, persona_id, &job_type, &stage, &message, true,
                )
            }
        })
        .await;
        let _ = call_blocking(state.db.clone(), {
            let run_key = run_key.clone();
            let payload = format!(
                r#"{{"stage":"{}","message":"{}"}}"#,
                stage,
                message.replace('"', "'")
            );
            move |db| {
                db.append_run_timeline_event(
                    &run_key,
                    chat_id,
                    persona_id,
                    "heartbeat",
                    Some(&payload),
                )
            }
        })
        .await;

        loop {
            tokio::select! {
                maybe_sig = rx.recv() => {
                    let Some(sig) = maybe_sig else {
                        if active {
                            warn!(
                                run_key = %run_key,
                                chat_id,
                                "heartbeat channel closed without Finished/Failed; marking inactive"
                            );
                            stage = "aborted".to_string();
                            message = "heartbeat channel closed (worker ended unexpectedly)".to_string();
                            let _ = call_blocking(state.db.clone(), {
                                let run_key = run_key.clone();
                                let stage = stage.clone();
                                let message = message.clone();
                                let job_type = job_type.as_str().to_string();
                                move |db| {
                                    db.upsert_job_heartbeat(
                                        &run_key,
                                        chat_id,
                                        persona_id,
                                        &job_type,
                                        &stage,
                                        &message,
                                        false,
                                    )
                                }
                            })
                            .await;
                            let _ = call_blocking(state.db.clone(), {
                                let run_key = run_key.clone();
                                let stage = stage.clone();
                                let message = message.clone();
                                let payload = format!(
                                    r#"{{"stage":"{}","message":"{}"}}"#,
                                    stage,
                                    message.replace('"', "'")
                                );
                                move |db| {
                                    db.append_run_timeline_event(
                                        &run_key,
                                        chat_id,
                                        persona_id,
                                        "heartbeat",
                                        Some(&payload),
                                    )
                                }
                            })
                            .await;
                        }
                        break;
                    };
                    match sig {
                        HeartbeatSignal::Started(m) => {
                            stage = "running".to_string();
                            message = m;
                        }
                        HeartbeatSignal::Progress(m) => {
                            stage = "running".to_string();
                            message = m;
                        }
                        HeartbeatSignal::ToolStart(tool) => {
                            stage = "running".to_string();
                            message = format!("running tool: {}", tool);
                        }
                        HeartbeatSignal::ToolResult { tool, is_error } => {
                            stage = if is_error { "running_with_errors".to_string() } else { "running".to_string() };
                            message = format!(
                                "tool {} {}",
                                tool,
                                if is_error { "reported an error" } else { "completed" }
                            );
                        }
                        HeartbeatSignal::Finished(m) => {
                            stage = "completed".to_string();
                            message = m;
                            active = false;
                        }
                        HeartbeatSignal::Failed(m) => {
                            stage = "failed".to_string();
                            message = m;
                            active = false;
                        }
                    }

                    let _ = call_blocking(state.db.clone(), {
                        let run_key = run_key.clone();
                        let stage = stage.clone();
                        let message = message.clone();
                        let job_type = job_type.as_str().to_string();
                        move |db| db.upsert_job_heartbeat(&run_key, chat_id, persona_id, &job_type, &stage, &message, active)
                    })
                    .await;
                    let _ = call_blocking(state.db.clone(), {
                        let run_key = run_key.clone();
                        let stage = stage.clone();
                        let message = message.clone();
                        let payload = format!(r#"{{"stage":"{}","message":"{}"}}"#, stage, message.replace('"', "'"));
                        move |db| db.append_run_timeline_event(&run_key, chat_id, persona_id, "heartbeat", Some(&payload))
                    })
                    .await;

                    if job_type.notify_user_periodically() && stage != "completed" && stage != "failed" {
                        let _ = deliver_to_contact(
                            state.db.clone(),
                            Some(&state.bot),
                            state.discord_http.as_deref(),
                            &state.config.bot_username,
                            chat_id,
                            persona_id,
                            &format!("Background update: {}", message),
                            Some(state.config.workspace_root_absolute()),
                        )
                        .await;
                        last_user_notify = Instant::now();
                    }

                    if !active {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let _ = call_blocking(state.db.clone(), {
                        let run_key = run_key.clone();
                        let stage = stage.clone();
                        let message = message.clone();
                        let job_type = job_type.as_str().to_string();
                        move |db| db.upsert_job_heartbeat(&run_key, chat_id, persona_id, &job_type, &stage, &message, active)
                    }).await;
                    let _ = call_blocking(state.db.clone(), {
                        let run_key = run_key.clone();
                        let stage = stage.clone();
                        let message = message.clone();
                        let payload = format!(r#"{{"stage":"{}","message":"{}"}}"#, stage, message.replace('"', "'"));
                        move |db| db.append_run_timeline_event(&run_key, chat_id, persona_id, "heartbeat", Some(&payload))
                    })
                    .await;

                    if active
                        && job_type.notify_user_periodically()
                        && last_user_notify.elapsed() >= heartbeat_period
                    {
                        let _ = deliver_to_contact(
                            state.db.clone(),
                            Some(&state.bot),
                            state.discord_http.as_deref(),
                            &state.config.bot_username,
                            chat_id,
                            persona_id,
                            &format!("Background update: {}", message),
                            Some(state.config.workspace_root_absolute()),
                        )
                        .await;
                        last_user_notify = Instant::now();
                    }
                }
            }
        }
    });
    tx
}
