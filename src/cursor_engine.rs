//! Cursor SDK agent engine: delegates a full turn to a local Python sidecar wrapping `cursor_sdk`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

use crate::agent_history::{
    EvaluatorStepRecord, IterationRecord, PipelineFinishExtras, PipelineStageRecord,
};
use crate::channels::telegram::{
    pipeline_finish_turn, process_classic_agent_with_events, AgentEvent, AgentProcessResult,
    AgentRequestContext, AgentRunPrep, AppState,
};
use crate::claude::{ContentBlock, Message, MessageContent};
use crate::db::call_blocking;
use crate::tools;

const MAX_PROMPT_LEN: usize = 120_000;

#[derive(Debug, Serialize)]
struct SidecarRunRequest<'a> {
    prompt: &'a str,
    cwd: &'a str,
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_params: Option<&'a [crate::cursor_engine_config::CursorModelParam]>,
}

#[derive(Debug, Deserialize)]
struct SidecarStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    result: Option<String>,
}

struct SidecarRunOutcome {
    final_text: String,
    returned_agent_id: Option<String>,
    run_status: String,
    sidecar_error: Option<String>,
}

fn is_stale_cursor_agent_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("not found") && lower.contains("agent")
}

fn cursor_session_scope(context: &AgentRequestContext<'_>) -> String {
    if let Some(sid) = context.session_id.as_deref() {
        return sid.to_string();
    }
    if context.is_scheduled_task || context.is_background_job {
        return context.run_key.clone().unwrap_or_default();
    }
    String::new()
}

async fn cursor_engine_classic_fallback(
    state: &AppState,
    context: AgentRequestContext<'_>,
    prep: &AgentRunPrep,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<Arc<AtomicBool>>,
    reason: &str,
) -> anyhow::Result<AgentProcessResult> {
    if context.is_scheduled_task {
        return Err(anyhow::anyhow!(
            "Scheduled task requires the Cursor engine but it is unavailable ({reason}). \
             Check Settings → Cursor (CURSOR_SDK_RUNNER_URL) and that the sidecar is running."
        ));
    }
    if context.is_background_job {
        let prompt = prep.latest_user_text.trim();
        let override_prompt = if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        };
        warn!("Cursor engine fallback to classic for background job: {reason}");
        return process_classic_agent_with_events(
            state,
            context,
            override_prompt,
            None,
            event_tx,
            cancel,
        )
        .await;
    }
    warn!("Cursor engine fallback to classic: {reason}");
    process_classic_agent_with_events(state, context, None, None, event_tx, cancel).await
}

async fn consume_sidecar_stream(
    response: reqwest::Response,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<SidecarRunOutcome, anyhow::Error> {
    let mut final_text = String::new();
    let mut returned_agent_id: Option<String> = None;
    let mut run_status = String::from("unknown");
    let mut sidecar_error: Option<String> = None;
    let mut buffer = String::new();

    let mut byte_stream = response.bytes_stream();
    while let Some(chunk) = byte_stream.next().await {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(anyhow::anyhow!("Run cancelled"));
        }
        let chunk = chunk.map_err(|e| anyhow::anyhow!("Cursor sidecar stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            let event: SidecarStreamEvent = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Cursor sidecar NDJSON parse error: {e} line={line}");
                    continue;
                }
            };
            match event.event_type.as_str() {
                "text" | "text_delta" => {
                    let delta = if event.text.is_empty() {
                        event.message
                    } else {
                        event.text
                    };
                    if !delta.is_empty() {
                        final_text.push_str(&delta);
                        if let Some(tx) = event_tx {
                            let _ = tx.send(AgentEvent::TextDelta { delta });
                        }
                    }
                }
                "done" => {
                    run_status = if event.status.is_empty() {
                        "finished".into()
                    } else {
                        event.status
                    };
                    returned_agent_id = event.agent_id;
                    if let Some(result) = event.result.filter(|s| !s.is_empty()) {
                        if final_text.is_empty() {
                            final_text = result;
                        }
                    }
                }
                "error" => {
                    sidecar_error = Some(if event.message.is_empty() {
                        "Cursor sidecar run failed".into()
                    } else {
                        event.message
                    });
                }
                other => {
                    warn!("Cursor sidecar unknown event type: {other}");
                }
            }
        }
    }

    Ok(SidecarRunOutcome {
        final_text,
        returned_agent_id,
        run_status,
        sidecar_error,
    })
}

pub async fn run_cursor_engine(
    state: &AppState,
    context: AgentRequestContext<'_>,
    prep: AgentRunPrep,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<Arc<AtomicBool>>,
) -> anyhow::Result<AgentProcessResult> {
    let run_start = Instant::now();
    let settings = state
        .cursor_settings
        .read()
        .map_err(|e| anyhow::anyhow!("cursor settings lock poisoned: {e}"))?
        .clone();
    let base_url = settings.sdk_runner_url.trim();

    if base_url.is_empty() {
        return cursor_engine_classic_fallback(
            state,
            context,
            &prep,
            event_tx,
            cancel,
            "CURSOR_SDK_RUNNER_URL unset",
        )
        .await;
    }

    let chat_id = context.chat_id;
    let persona_id = context.persona_id;
    let session_scope = cursor_session_scope(&context);

    let workspace_root = PathBuf::from(state.config.working_dir());
    let working_dir = tools::persona_shared_dir(&workspace_root, chat_id, persona_id);
    if let Err(e) = tokio::fs::create_dir_all(&working_dir).await {
        return Err(anyhow::anyhow!(
            "Failed to create persona workspace {}: {e}",
            working_dir.display()
        ));
    }
    let cwd = working_dir.to_string_lossy().to_string();
    if let Err(msg) = tools::path_guard::check_path(&cwd) {
        return Err(anyhow::anyhow!(msg));
    }

    let mut prompt = flatten_turn_prompt(&prep.system_prompt, &prep.messages);
    if context.is_scheduled_task {
        prompt = format!(
            "[scheduled_task]\nAutomated scheduled job — not interactive chat. \
             Complete the task below and return a concise result.\n\n{prompt}"
        );
    }
    if prep.has_image_input {
        prompt.push_str(
            "\n\n(Note: image input was attached but the Cursor engine currently supports text only. \
             Describe any needed visual context in follow-up if required.)\n",
        );
    }
    if prompt.len() > MAX_PROMPT_LEN {
        prompt.truncate(prompt.floor_char_boundary(MAX_PROMPT_LEN));
        prompt.push_str("\n\n(prompt truncated for Cursor engine limit)");
    }

    let model = settings.sdk_model.trim();
    let model = if model.is_empty() {
        "composer-2.5"
    } else {
        model
    };

    let resume_agent_id = call_blocking(state.db.clone(), {
        let session_scope = session_scope.clone();
        move |db| db.get_cursor_engine_agent_id(chat_id, persona_id, &session_scope)
    })
    .await
    .ok()
    .flatten();

    let sidecar_url = format!("{}/run", base_url.trim_end_matches('/'));
    let timeout_secs = settings.timeout_secs.max(60);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs + 30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return cursor_engine_classic_fallback(
                state,
                context,
                &prep,
                event_tx,
                cancel,
                &format!("HTTP client build failed: {e}"),
            )
            .await;
        }
    };

    let started_at = chrono::Utc::now().to_rfc3339();
    let sidecar_started = Instant::now();
    let mut resume_id = resume_agent_id.clone();
    let mut outcome = SidecarRunOutcome {
        final_text: String::new(),
        returned_agent_id: None,
        run_status: String::from("unknown"),
        sidecar_error: Some("Cursor sidecar run did not start".into()),
    };

    let model_params = if settings.sdk_model_params.is_empty() {
        None
    } else {
        Some(settings.sdk_model_params.as_slice())
    };

    for attempt in 0..2 {
        let body = SidecarRunRequest {
            prompt: &prompt,
            cwd: &cwd,
            model,
            agent_id: resume_id.as_deref(),
            model_params,
        };

        let response = match client.post(&sidecar_url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                return cursor_engine_classic_fallback(
                    state,
                    context,
                    &prep,
                    event_tx,
                    cancel,
                    &format!("sidecar request failed: {e}"),
                )
                .await;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            warn!(
                "Cursor sidecar returned {}: {}",
                status.as_u16(),
                text.chars().take(200).collect::<String>()
            );
            return cursor_engine_classic_fallback(
                state,
                context,
                &prep,
                event_tx,
                cancel,
                &format!("sidecar HTTP {}", status.as_u16()),
            )
            .await;
        }

        outcome = consume_sidecar_stream(response, event_tx, cancel.as_ref()).await?;

        if let Some(ref err) = outcome.sidecar_error {
            if attempt == 0 && resume_id.is_some() && is_stale_cursor_agent_error(err) {
                warn!(
                    chat_id,
                    persona_id,
                    stale_agent_id = resume_id.as_deref().unwrap_or(""),
                    "Stale Cursor agent id; clearing and retrying with a fresh session"
                );
                let db = state.db.clone();
                let session_scope_for_db = session_scope.clone();
                let _ = call_blocking(db, move |database| {
                    database.clear_cursor_engine_agent_id(
                        chat_id,
                        persona_id,
                        &session_scope_for_db,
                    )
                })
                .await;
                resume_id = None;
                continue;
            }
            return Err(anyhow::anyhow!(err.clone()));
        }

        break;
    }

    let SidecarRunOutcome {
        mut final_text,
        returned_agent_id,
        run_status,
        sidecar_error,
    } = outcome;

    if sidecar_error.is_some() {
        return Err(anyhow::anyhow!(
            sidecar_error.unwrap_or_else(|| "Cursor sidecar run failed".into())
        ));
    }

    if final_text.trim().is_empty() {
        final_text = "(Cursor agent completed with no text output.)".into();
    }

    let finished_at = chrono::Utc::now().to_rfc3339();
    let success = run_status == "finished" || run_status == "success";
    let prompt_preview: String = if prompt.len() <= 200 {
        prompt.clone()
    } else {
        format!("{}...", &prompt[..prompt.floor_char_boundary(200)])
    };
    let output_preview: String = if final_text.len() <= 500 {
        final_text.clone()
    } else {
        format!("{}...", &final_text[..final_text.floor_char_boundary(500)])
    };

    if let Some(ref agent_id) = returned_agent_id {
        let db = state.db.clone();
        let session_scope_for_db = session_scope.clone();
        let agent_id_owned = agent_id.clone();
        let _ = call_blocking(db.clone(), move |database| {
            database.set_cursor_engine_agent_id(
                chat_id,
                persona_id,
                &session_scope_for_db,
                &agent_id_owned,
            )
        })
        .await;
    }

    let channel = context.caller_channel.to_string();
    let workdir_owned = cwd.clone();
    let _ = call_blocking(state.db.clone(), move |database| {
        database.insert_cursor_agent_run(
            chat_id,
            &channel,
            &prompt_preview,
            Some(workdir_owned.as_str()),
            &started_at,
            &finished_at,
            success,
            None,
            Some(&output_preview),
            None::<&str>,
            None::<&str>,
        )
    })
    .await;

    let pipeline_stages = vec![PipelineStageRecord {
        stage: "cursor_sdk".into(),
        detail: format!(
            "model={} resume={} status={}",
            model,
            resume_agent_id.is_some(),
            run_status
        ),
        duration_ms: sidecar_started.elapsed().as_millis(),
    }];
    let extras = PipelineFinishExtras {
        pipeline_stages,
        cloud_calls: 0,
        agent_engine: "cursor".into(),
    };

    let mut messages = prep.messages.clone();
    let mut pdqe_retries = 0usize;
    let mut pdqe_steps: Vec<EvaluatorStepRecord> = Vec::new();
    let history_iterations: Vec<IterationRecord> = Vec::new();
    let mut agent_history_basename: Option<String> = None;
    let run_tool_names: Vec<String> = Vec::new();

    info!(
        chat_id,
        persona_id,
        model,
        resume = resume_agent_id.is_some(),
        status = %run_status,
        "Cursor SDK engine finished sidecar run"
    );

    loop {
        let mut iterations = history_iterations.clone();
        match pipeline_finish_turn(
            state,
            &context,
            event_tx,
            &prep.run_key,
            chat_id,
            persona_id,
            if success {
                "cursor_engine_complete"
            } else {
                "cursor_engine_error"
            },
            final_text.clone(),
            &prep.system_prompt,
            &mut messages,
            prep.protected_message_count,
            &mut pdqe_retries,
            &mut pdqe_steps,
            &mut iterations,
            &prep.principles_content,
            true,
            &run_tool_names,
            &mut agent_history_basename,
            false,
            &prep.user_msg_preview,
            run_start,
            &prep.initial_llm_snapshot_json,
            &prep.multimodel_run_summary,
            Some(&extras),
        )
        .await?
        {
            Some(result) => return Ok(result),
            None => {
                // PDQE requested revision; re-enter the finish loop until the gate delivers.
            }
        }
    }
}

fn flatten_turn_prompt(system_prompt: &str, messages: &[Message]) -> String {
    let mut out = String::new();
    out.push_str("# System\n\n");
    out.push_str(system_prompt);
    out.push_str("\n\n# Conversation\n\n");
    for msg in messages {
        let text = message_text(msg);
        if text.trim().is_empty() {
            continue;
        }
        out.push_str("## ");
        out.push_str(msg.role.as_str());
        out.push('\n');
        out.push_str(&text);
        out.push_str("\n\n");
    }
    out
}

fn message_text(msg: &Message) -> String {
    match &msg.content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_cursor_agent_error_detects_missing_agent() {
        assert!(is_stale_cursor_agent_error(
            "Cursor SDK startup failed: Agent agent-cea12fe8-fdd5-4fa4-880b-d8f7f6225a54 not found"
        ));
        assert!(!is_stale_cursor_agent_error("prompt required"));
    }

    #[test]
    fn cursor_session_scope_uses_run_key_for_scheduled() {
        let ctx = AgentRequestContext {
            caller_channel: "telegram",
            chat_id: 1,
            chat_type: "private",
            persona_id: 2,
            is_scheduled_task: true,
            is_background_job: false,
            run_key: Some("scheduled:42:2026-01-01T00:00:00Z".into()),
            reply_bot_instance_id: None,
            session_id: None,
        };
        assert_eq!(
            cursor_session_scope(&ctx),
            "scheduled:42:2026-01-01T00:00:00Z"
        );
    }

    #[test]
    fn flatten_turn_prompt_includes_roles() {
        let messages = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("hello".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("hi".into()),
            },
        ];
        let out = flatten_turn_prompt("sys", &messages);
        assert!(out.contains("# System"));
        assert!(out.contains("## user"));
        assert!(out.contains("hello"));
        assert!(out.contains("## assistant"));
    }
}
