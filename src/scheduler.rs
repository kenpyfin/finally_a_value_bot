use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::Semaphore;
use tracing::{error, info};

use crate::channel::deliver_to_contact;
use crate::db::{call_blocking, ScheduledTask};
use crate::error::FinallyAValueBotError;
use crate::job_heartbeat::{
    signal_from_agent_event, spawn_shared_heartbeat, HeartbeatSignal, JobType,
};
use crate::telegram::{process_with_agent_with_events, AgentRequestContext, AppState};

fn channel_from_chat_type(chat_type: &str) -> &'static str {
    match chat_type {
        "discord" => "discord",
        "whatsapp" => "whatsapp",
        "web" => "web",
        _ => "telegram",
    }
}

/// Snapshot after DB claim and persona resolution; agent work runs in a spawned task.
struct PreparedScheduledRun {
    task_id: i64,
    chat_id: i64,
    persona_id: i64,
    channel: &'static str,
    prompt: String,
    next_run: Option<String>,
    started_at_str: String,
}

pub fn spawn_scheduler(state: Arc<AppState>) {
    let permits = state.config.scheduler_max_concurrent_tasks.max(1);
    let semaphore = Arc::new(Semaphore::new(permits));
    let task_timeout_secs = state.config.scheduler_task_timeout_secs;
    let stale_reclaim_secs = state.config.scheduler_stale_running_reclaim_secs as i64;
    let poll_interval_secs = state.config.scheduler_poll_interval_secs.max(1);

    tokio::spawn(async move {
        info!(
            "Scheduler started (poll_interval_secs={poll_interval_secs}, max_concurrent={permits}, task_timeout_secs={task_timeout_secs}, stale_reclaim_secs={stale_reclaim_secs})"
        );
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(poll_interval_secs)).await;
            run_due_tasks(
                &state,
                semaphore.clone(),
                task_timeout_secs,
                stale_reclaim_secs,
            )
            .await;
        }
    });
}

async fn run_due_tasks(
    state: &Arc<AppState>,
    semaphore: Arc<Semaphore>,
    task_timeout_secs: u64,
    stale_reclaim_secs: i64,
) {
    let now = Utc::now().to_rfc3339();

    match call_blocking(state.db.clone(), {
        let now = now.clone();
        move |db| db.reclaim_stale_running_tasks(&now, stale_reclaim_secs)
    })
    .await
    {
        Ok(ids) if !ids.is_empty() => {
            info!(
                "Scheduler: reclaimed {} stale running task(s): {:?}",
                ids.len(),
                ids
            );
        }
        Err(e) => error!("Scheduler: failed to reclaim stale running tasks: {e}"),
        _ => {}
    }

    match call_blocking(state.db.clone(), {
        let now = now.clone();
        move |db| db.reconcile_stale_active_job_heartbeats(&now, stale_reclaim_secs)
    })
    .await
    {
        Ok(keys) if !keys.is_empty() => {
            info!(
                "Scheduler: reconciled {} stale active job heartbeat(s): {:?}",
                keys.len(),
                keys
            );
        }
        Err(e) => error!("Scheduler: failed to reconcile stale job heartbeats: {e}"),
        _ => {}
    }

    match call_blocking(state.db.clone(), {
        let now = now.clone();
        move |db| db.reconcile_orphan_stale_background_jobs(&now, stale_reclaim_secs)
    })
    .await
    {
        Ok(ids) if !ids.is_empty() => {
            info!(
                "Scheduler: reconciled {} orphan stale background job(s): {:?}",
                ids.len(),
                ids
            );
        }
        Err(e) => error!("Scheduler: failed to reconcile orphan background jobs: {e}"),
        _ => {}
    }

    let tasks = match call_blocking(state.db.clone(), {
        let now_for_due = now.clone();
        move |db| db.get_due_tasks(&now_for_due)
    })
    .await
    {
        Ok(t) => t,
        Err(e) => {
            error!("Scheduler: failed to query due tasks: {e}");
            return;
        }
    };

    let due_count = tasks.len();
    let mut claimed = 0usize;
    let mut spawned = 0usize;

    for task in tasks {
        // Claim synchronously before spawning so the next tick cannot re-list the same task
        // while workers are still waiting on the semaphore.
        let (prepared, did_claim) = match claim_and_prepare_scheduled_task(state, task, &now).await
        {
            Ok(pair) => pair,
            Err(e) => {
                error!("Scheduler: failed to prepare task: {e}");
                continue;
            }
        };
        if did_claim {
            claimed += 1;
        }
        let Some(prepared) = prepared else {
            continue;
        };
        spawned += 1;

        let state = state.clone();
        let sem = semaphore.clone();
        tokio::spawn(async move {
            let _permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let chat_id_for_queue = prepared.chat_id;
            let chat_queue = state.chat_queue.clone();
            let queue_position = chat_queue
                .enqueue(chat_id_for_queue, async move {
                    run_scheduled_agent_and_finalize(state, prepared, task_timeout_secs).await;
                })
                .await;
            info!(
                target: "queue",
                chat_id = chat_id_for_queue,
                queue_position = queue_position,
                "Enqueued scheduled agent run"
            );
        });
    }

    if due_count > 0 {
        info!("Scheduler tick: due_count={due_count}, claimed={claimed}, spawned={spawned}");
    }
}

/// Claim in DB, resolve channel/persona.
/// Second tuple element is true iff the atomic DB claim (`try_mark_task_running`) succeeded.
async fn claim_and_prepare_scheduled_task(
    state: &Arc<AppState>,
    task: ScheduledTask,
    now: &str,
) -> Result<(Option<PreparedScheduledRun>, bool), FinallyAValueBotError> {
    let task_id = task.id;
    let chat_id = task.chat_id;
    let scheduled_persona_id = task.persona_id;
    let prompt = task.prompt.clone();

    info!(
        "Scheduler: executing task #{} for chat {}",
        task_id, chat_id
    );

    let started_at = Utc::now();
    let started_at_str = started_at.to_rfc3339();

    let tz: chrono_tz::Tz = state.config.timezone.parse().unwrap_or(chrono_tz::Tz::UTC);
    let next_run = if task.schedule_type == "cron" {
        match cron::Schedule::from_str(&task.schedule_value) {
            Ok(schedule) => schedule
                .after(&started_at.with_timezone(&tz))
                .next()
                .map(|t| t.with_timezone(&Utc).to_rfc3339()),
            Err(e) => {
                error!("Scheduler: invalid cron for task #{}: {e}", task_id);
                None
            }
        }
    } else {
        None
    };

    if task.schedule_type == "cron" && next_run.is_none() {
        error!(
            "Scheduler: skipping task #{} — could not compute next cron occurrence",
            task_id
        );
        return Ok((None, false));
    }

    let started_for_claim = started_at_str.clone();
    let next_run_claim = next_run.clone();
    let now_bound = now.to_string();
    let claimed = call_blocking(state.db.clone(), move |db| {
        db.try_mark_task_running(
            task_id,
            &started_for_claim,
            next_run_claim.as_deref(),
            &now_bound,
        )
    })
    .await?;
    if !claimed {
        info!(
            "Scheduler: task #{} already claimed or not due, skipping",
            task_id
        );
        return Ok((None, false));
    }

    let channel = match call_blocking(state.db.clone(), move |db| db.get_chat_type(chat_id)).await {
        Ok(Some(chat_type)) => channel_from_chat_type(&chat_type),
        _ => "telegram",
    };

    let persona_id = call_blocking(state.db.clone(), move |db| {
        if scheduled_persona_id > 0 && db.persona_exists(chat_id, scheduled_persona_id)? {
            return Ok(scheduled_persona_id);
        }
        let fallback = db.get_current_persona_id(chat_id)?;
        if scheduled_persona_id > 0 && fallback != scheduled_persona_id {
            info!(
                "Scheduler: task #{} persona {} missing for chat {}, falling back to persona {}",
                task_id, scheduled_persona_id, chat_id, fallback
            );
        }
        Ok(fallback)
    })
    .await
    .unwrap_or(0);

    if persona_id == 0 {
        error!("Scheduler: could not resolve persona for chat {}", chat_id);
        let next_run_finalize = next_run.clone();
        let _ = call_blocking(state.db.clone(), move |db| {
            db.finalize_task_run(task_id, next_run_finalize.as_deref())?;
            Ok(())
        })
        .await;
        let finished_at_str = Utc::now().to_rfc3339();
        let _ = call_blocking(state.db.clone(), move |db| {
            db.log_task_run(
                task_id,
                chat_id,
                &started_at_str,
                &finished_at_str,
                0,
                false,
                Some("Could not resolve persona for chat"),
            )?;
            Ok(())
        })
        .await;
        return Ok((None, true));
    }

    Ok((
        Some(PreparedScheduledRun {
            task_id,
            chat_id,
            persona_id,
            channel,
            prompt,
            next_run,
            started_at_str,
        }),
        true,
    ))
}

/// Agent run, deliver, finalize, log. Dropping the outer `timeout` future does not necessarily
/// kill child OS processes started by tools (e.g. bash); it only bounds async work.
async fn run_scheduled_agent_and_finalize(
    state: Arc<AppState>,
    p: PreparedScheduledRun,
    task_timeout_secs: u64,
) {
    let task_id = p.task_id;
    let chat_id = p.chat_id;
    let persona_id = p.persona_id;
    let channel = p.channel;
    let prompt = p.prompt;
    let next_run = p.next_run;
    let started_at_str = p.started_at_str;
    let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let hb_key = format!("scheduled:{}:{}", task_id, started_at_str);
    let hb_tx = spawn_shared_heartbeat(
        state.clone(),
        hb_key,
        chat_id,
        persona_id,
        JobType::Scheduled,
    );
    let _ = hb_tx.send(HeartbeatSignal::Started(format!(
        "scheduled task #{} started",
        task_id
    )));
    let (evt_tx, mut evt_rx) = unbounded_channel();
    let hb_forward = {
        let hb_tx = hb_tx.clone();
        tokio::spawn(async move {
            while let Some(evt) = evt_rx.recv().await {
                if let Some(sig) = signal_from_agent_event(&evt) {
                    let _ = hb_tx.send(sig);
                }
            }
        })
    };

    let agent_fut = process_with_agent_with_events(
        &state,
        AgentRequestContext {
            caller_channel: channel,
            chat_id,
            chat_type: "private",
            persona_id,
            is_scheduled_task: true,
            is_background_job: false,
            run_key: Some(format!("scheduled:{}:{}", task_id, started_at_str)),
        },
        Some(&prompt),
        None,
        Some(&evt_tx),
    );

    let (success, result_summary) = match tokio::time::timeout(
        std::time::Duration::from_secs(task_timeout_secs),
        agent_fut,
    )
    .await
    {
        Err(_elapsed) => {
            drop(evt_tx);
            let _ = hb_forward.await;
            error!(
                "Scheduler: task #{} timed out after {}s",
                task_id, task_timeout_secs
            );
            let _ = hb_tx.send(HeartbeatSignal::Failed(format!(
                "scheduled task #{} timed out after {}s",
                task_id, task_timeout_secs
            )));
            let err_text = format!(
                "Scheduled task #{} timed out after {} seconds.",
                task_id, task_timeout_secs
            );
            let delivery_ok = deliver_to_contact(
                state.db.clone(),
                Some(&state.bot),
                state.discord_http.as_deref(),
                &state.config.bot_username,
                chat_id,
                persona_id,
                &err_text,
                Some(state.config.workspace_root_absolute()),
            )
            .await
            .is_ok();
            let summary = if delivery_ok {
                Some(format!("Timed out after {task_timeout_secs}s"))
            } else {
                Some(format!(
                    "Timed out after {task_timeout_secs}s (and failed to deliver timeout message)"
                ))
            };
            (false, summary)
        }
        Ok(Ok(response)) => {
            drop(evt_tx);
            let _ = hb_forward.await;
            let response_text = if response.trim().is_empty() {
                format!("Scheduled task #{} completed.", task_id)
            } else {
                response
            };
            const DEDUPE_WINDOW_SECS: i64 = 120;
            let dedupe_text = crate::channel::with_persona_indicator(
                state.db.clone(),
                persona_id,
                &response_text,
            )
            .await;
            let skip_dup = match call_blocking(state.db.clone(), {
                let text = dedupe_text;
                move |db| {
                    db.should_skip_duplicate_final_delivery(chat_id, &text, DEDUPE_WINDOW_SECS)
                }
            })
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        target: "scheduler",
                        task_id = task_id,
                        error = %e,
                        "duplicate-final check failed; delivering anyway"
                    );
                    false
                }
            };
            if skip_dup {
                info!(
                    target: "scheduler",
                    task_id = task_id,
                    chat_id = chat_id,
                    "Skipping duplicate scheduled delivery: latest stored message already matches"
                );
                let _ = hb_tx.send(HeartbeatSignal::Finished(format!(
                    "scheduled task #{} completed (duplicate delivery skipped)",
                    task_id
                )));
                (true, Some("Skipped duplicate final delivery".to_string()))
            } else {
                match deliver_to_contact(
                    state.db.clone(),
                    Some(&state.bot),
                    state.discord_http.as_deref(),
                    &state.config.bot_username,
                    chat_id,
                    persona_id,
                    &response_text,
                    Some(state.config.workspace_root_absolute()),
                )
                .await
                {
                    Ok(()) => {
                        let _ = hb_tx.send(HeartbeatSignal::Finished(format!(
                            "scheduled task #{} completed",
                            task_id
                        )));
                        let summary = if response_text.len() > 200 {
                            format!(
                                "{}...",
                                &response_text[..response_text.floor_char_boundary(200)]
                            )
                        } else {
                            response_text
                        };
                        (true, Some(summary))
                    }
                    Err(e) => {
                        let _ = hb_tx.send(HeartbeatSignal::Failed(format!(
                            "scheduled task #{} delivery failed: {}",
                            task_id, e
                        )));
                        error!(
                            "Scheduler: task #{} produced a response but delivery failed: {}",
                            task_id, e
                        );
                        (
                            false,
                            Some(format!("Delivery error after successful execution: {e}")),
                        )
                    }
                }
            }
        }
        Ok(Err(e)) => {
            drop(evt_tx);
            let _ = hb_forward.await;
            error!("Scheduler: task #{} failed: {e}", task_id);
            let _ = hb_tx.send(HeartbeatSignal::Failed(format!(
                "scheduled task #{} failed: {}",
                task_id, e
            )));
            let err_text = format!("Scheduled task #{} failed: {e}", task_id);
            let delivery_ok = deliver_to_contact(
                state.db.clone(),
                Some(&state.bot),
                state.discord_http.as_deref(),
                &state.config.bot_username,
                chat_id,
                persona_id,
                &err_text,
                Some(state.config.workspace_root_absolute()),
            )
            .await
            .is_ok();
            if delivery_ok {
                (false, Some(format!("Error: {e}")))
            } else {
                (
                    false,
                    Some(format!("Error: {e} (and failed to deliver error message)")),
                )
            }
        }
    };

    let finished_at = Utc::now();
    let finished_at_str = finished_at.to_rfc3339();
    let duration_ms = (finished_at - started_at).num_milliseconds();

    let next_run_cleanup = next_run.clone();
    let started_for_cleanup = started_at_str.clone();
    let finished_for_cleanup = finished_at_str.clone();
    let summary_for_cleanup = result_summary.clone();
    if let Err(e) = call_blocking(state.db.clone(), move |db| {
        db.finalize_task_run(task_id, next_run_cleanup.as_deref())?;
        db.log_task_run(
            task_id,
            chat_id,
            &started_for_cleanup,
            &finished_for_cleanup,
            duration_ms,
            success,
            summary_for_cleanup.as_deref(),
        )?;
        Ok(())
    })
    .await
    {
        error!(
            "Scheduler: failed to finalize or log task #{}: {e}",
            task_id
        );
    }
}
