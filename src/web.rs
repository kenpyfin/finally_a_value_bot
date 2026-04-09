use std::collections::{HashMap, VecDeque};
use std::path::Path as FsPath;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use base64::Engine;
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info};

use crate::channel::deliver_to_contact;
use crate::claude::{Message, MessageContent};
use crate::config::Config;
use crate::db::{call_blocking, Persona, StoredMessage};
use crate::slash_commands::{parse as parse_slash_command, SlashCommand};
use crate::social_oauth;
use crate::telegram::{
    archive_conversation, process_with_agent, process_with_agent_with_events, AgentEvent,
    AgentRequestContext, AppState, BACKGROUND_JOB_HANDOFF_PREFIX,
};
use std::time::SystemTime;

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

#[derive(Clone)]
struct WebState {
    app_state: Arc<AppState>,
    auth_token: Option<String>,
    run_hub: RunHub,
    request_hub: RequestHub,
    limits: WebLimits,
}

#[derive(Clone, Debug)]
struct RunEvent {
    id: u64,
    event: String,
    data: String,
}

#[derive(Clone, Default)]
struct RunHub {
    channels: Arc<Mutex<HashMap<String, RunChannel>>>,
}

#[derive(Clone, Debug)]
struct WebLimits {
    max_inflight_per_session: usize,
    max_requests_per_window: usize,
    rate_window: Duration,
    run_history_limit: usize,
    session_idle_ttl: Duration,
}

impl Default for WebLimits {
    fn default() -> Self {
        Self {
            max_inflight_per_session: 2,
            max_requests_per_window: 8,
            rate_window: Duration::from_secs(10),
            run_history_limit: 512,
            session_idle_ttl: Duration::from_secs(300),
        }
    }
}

impl WebLimits {
    fn from_config(cfg: &Config) -> Self {
        Self {
            max_inflight_per_session: cfg.web_max_inflight_per_session,
            max_requests_per_window: cfg.web_max_requests_per_window,
            rate_window: Duration::from_secs(cfg.web_rate_window_seconds),
            run_history_limit: cfg.web_run_history_limit,
            session_idle_ttl: Duration::from_secs(cfg.web_session_idle_ttl_seconds),
        }
    }
}

#[derive(Clone, Default)]
struct RequestHub {
    sessions: Arc<Mutex<HashMap<String, SessionQuota>>>,
}

struct SessionQuota {
    inflight: usize,
    recent: VecDeque<Instant>,
    last_touch: Instant,
}

impl Default for SessionQuota {
    fn default() -> Self {
        Self {
            inflight: 0,
            recent: VecDeque::new(),
            last_touch: Instant::now(),
        }
    }
}

#[derive(Clone)]
struct RunChannel {
    sender: broadcast::Sender<RunEvent>,
    history: VecDeque<RunEvent>,
    next_id: u64,
    done: bool,
}

impl RunHub {
    async fn create(&self, run_id: &str) {
        let (tx, _) = broadcast::channel(512);
        let mut guard = self.channels.lock().await;
        guard.insert(
            run_id.to_string(),
            RunChannel {
                sender: tx,
                history: VecDeque::new(),
                next_id: 1,
                done: false,
            },
        );
    }

    async fn publish(&self, run_id: &str, event: &str, data: String, history_limit: usize) {
        let mut guard = self.channels.lock().await;
        let Some(channel) = guard.get_mut(run_id) else {
            return;
        };

        let evt = RunEvent {
            id: channel.next_id,
            event: event.to_string(),
            data,
        };
        channel.next_id = channel.next_id.saturating_add(1);
        if channel.history.len() >= history_limit {
            let _ = channel.history.pop_front();
        }
        channel.history.push_back(evt.clone());
        if evt.event == "done" || evt.event == "error" {
            channel.done = true;
        }
        let _ = channel.sender.send(evt);
    }

    async fn subscribe_with_replay(
        &self,
        run_id: &str,
        last_event_id: Option<u64>,
    ) -> Option<(
        broadcast::Receiver<RunEvent>,
        Vec<RunEvent>,
        bool,
        bool,
        Option<u64>,
    )> {
        let guard = self.channels.lock().await;
        let channel = guard.get(run_id)?;
        let oldest_event_id = channel.history.front().map(|e| e.id);
        let replay_truncated = matches!(
            (last_event_id, oldest_event_id),
            (Some(last), Some(oldest)) if last.saturating_add(1) < oldest
        );
        let replay = channel
            .history
            .iter()
            .filter(|e| last_event_id.is_none_or(|id| e.id > id))
            .cloned()
            .collect::<Vec<_>>();
        Some((
            channel.sender.subscribe(),
            replay,
            channel.done,
            replay_truncated,
            oldest_event_id,
        ))
    }

    async fn status(&self, run_id: &str) -> Option<(bool, u64)> {
        let guard = self.channels.lock().await;
        let channel = guard.get(run_id)?;
        Some((channel.done, channel.next_id.saturating_sub(1)))
    }

    async fn remove_later(&self, run_id: String, after_seconds: u64) {
        let channels = self.channels.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(after_seconds)).await;
            let mut guard = channels.lock().await;
            guard.remove(&run_id);
        });
    }
}

impl RequestHub {
    async fn begin(
        &self,
        session_key: &str,
        limits: &WebLimits,
    ) -> Result<(), (StatusCode, String)> {
        let now = Instant::now();
        let mut guard = self.sessions.lock().await;
        let quota = guard.entry(session_key.to_string()).or_default();
        quota.last_touch = now;

        while let Some(ts) = quota.recent.front() {
            if now.duration_since(*ts) > limits.rate_window {
                let _ = quota.recent.pop_front();
            } else {
                break;
            }
        }

        if quota.inflight >= limits.max_inflight_per_session {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "too many concurrent requests for session".into(),
            ));
        }
        if quota.recent.len() >= limits.max_requests_per_window {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "rate limit exceeded for session".into(),
            ));
        }

        quota.inflight += 1;
        quota.recent.push_back(now);
        Ok(())
    }

    async fn end_with_limits(&self, session_key: &str, limits: &WebLimits) {
        let now = Instant::now();
        let mut guard = self.sessions.lock().await;
        if let Some(quota) = guard.get_mut(session_key) {
            while let Some(ts) = quota.recent.front() {
                if now.duration_since(*ts) > limits.rate_window {
                    let _ = quota.recent.pop_front();
                } else {
                    break;
                }
            }
            quota.inflight = quota.inflight.saturating_sub(1);
            quota.last_touch = now;
            if quota.inflight == 0 && quota.recent.is_empty() {
                guard.remove(session_key);
            }
        }
        guard.retain(|_, quota| {
            !(quota.inflight == 0 && now.duration_since(quota.last_touch) > limits.session_idle_ttl)
        });
    }
}

fn auth_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn require_auth(
    headers: &HeaderMap,
    expected_token: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    let Some(expected) = expected_token else {
        return Ok(());
    };

    let provided = auth_token_from_headers(headers).unwrap_or_default();

    if provided == expected {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "unauthorized".into()))
    }
}

#[derive(Debug, Serialize)]
struct HistoryItem {
    id: String,
    sender_name: String,
    content: String,
    is_from_bot: bool,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    limit: Option<usize>,
    day: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendRequest {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    sender_name: Option<String>,
    message: String,
    #[serde(default)]
    attachments: Vec<SendAttachmentRequest>,
}

#[derive(Debug, Deserialize)]
struct SendAttachmentRequest {
    filename: Option<String>,
    media_type: Option<String>,
    data_base64: String,
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    run_id: String,
    last_event_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ResetRequest {
    chat_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PersonasQuery {
    chat_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PersonasSwitchRequest {
    chat_id: Option<i64>,
    persona_name: String,
}

#[derive(Debug, Deserialize)]
struct PersonaCreateRequest {
    chat_id: Option<i64>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct PersonaDeleteRequest {
    chat_id: Option<i64>,
    persona_id: i64,
}

#[derive(Debug, Deserialize)]
struct ContactsBindRequest {
    #[allow(dead_code)]
    chat_id: Option<i64>,
    /// Canonical chat_id of the contact to bind web to (e.g. from Telegram).
    contact_chat_id: i64,
}

#[derive(Debug, Deserialize)]
struct ContactsUnlinkRequest {
    #[allow(dead_code)]
    chat_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SchedulesQuery {
    chat_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ScheduleCreateRequest {
    chat_id: Option<i64>,
    prompt: String,
    schedule_type: String, // "cron" | "once"
    schedule_value: String,
    timezone: Option<String>,
    persona_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DeleteSessionRequest {
    chat_id: Option<i64>,
    #[allow(dead_code)]
    persona_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ScheduleUpdateRequest {
    status: Option<String>, // "paused" | "active" | "cancelled"
    persona_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RunStatusQuery {
    run_id: String,
}

async fn index() -> impl IntoResponse {
    match WEB_ASSETS.get_file("index.html") {
        Some(file) => Html(String::from_utf8_lossy(file.contents()).to_string()).into_response(),
        None => (StatusCode::NOT_FOUND, "index.html missing").into_response(),
    }
}

async fn api_health(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    Ok(Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "web_enabled": state.app_state.config.web_enabled,
    })))
}

/// Single universal chat; no multi-chat concept. Web always uses this chat.
const DEFAULT_UNIVERSAL_CHAT_ID: i64 = 997894126;

/// Resolve chat_id for web requests. Always returns the single universal chat.
/// chat_id in request is ignored; there is only one conversation across all channels.
fn resolve_chat_id_for_web(
    _chat_id: Option<i64>,
    config: &Config,
) -> Result<i64, (StatusCode, String)> {
    Ok(config
        .universal_chat_id
        .unwrap_or(DEFAULT_UNIVERSAL_CHAT_ID))
}

/// Ensure web/default always points to the configured universal chat.
/// This allows UNIVERSAL_CHAT_ID changes to take effect on restart.
async fn ensure_web_binding_for_universal(
    state: &WebState,
    chat_id: i64,
) -> Result<(), (StatusCode, String)> {
    let cid = chat_id;
    call_blocking(state.app_state.db.clone(), move |db| {
        db.upsert_chat(cid, None, "web")?;
        db.link_channel(cid, "web", "default")?;
        Ok(())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

async fn api_chat(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let cid = chat_id;
    let persona_id = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_current_persona_id(cid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": persona_id,
    })))
}

async fn api_history(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let cid = chat_id;
    let persona_id = if let Some(pid) = query.persona_id {
        pid
    } else {
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let cid2 = chat_id;
    let pid = persona_id;

    let messages = if let Some(ref day) = query.day {
        let (from_date, to_date) = day_range(day);
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_messages_for_date_range(
                cid2,
                pid,
                Some(from_date.as_str()),
                Some(to_date.as_str()),
                2000,
            )
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        let mut msgs = call_blocking(state.app_state.db.clone(), move |db| {
            db.get_all_messages(cid2, pid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if let Some(limit) = query.limit {
            if msgs.len() > limit {
                msgs = msgs[msgs.len() - limit..].to_vec();
            }
        }
        msgs
    };

    let items: Vec<HistoryItem> = messages
        .into_iter()
        .map(|m| HistoryItem {
            id: m.id,
            sender_name: m.sender_name,
            content: m.content,
            is_from_bot: m.is_from_bot,
            timestamp: m.timestamp,
        })
        .collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": persona_id,
        "messages": items,
    })))
}

/// Return (from_date, to_date) as ISO strings for a given day (YYYY-MM-DD).
fn day_range(day: &str) -> (String, String) {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") {
        let start: chrono::DateTime<chrono::Utc> = chrono::DateTime::from_naive_utc_and_offset(
            d.and_hms_opt(0, 0, 0).unwrap(),
            chrono::Utc,
        );
        let end: chrono::DateTime<chrono::Utc> = chrono::DateTime::from_naive_utc_and_offset(
            d.and_hms_opt(23, 59, 59).unwrap(),
            chrono::Utc,
        );
        return (start.to_rfc3339(), end.to_rfc3339());
    }
    ("".into(), "".into())
}

async fn api_history_days(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let cid = chat_id;
    let persona_id = if let Some(pid) = query.persona_id {
        pid
    } else {
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let cid2 = chat_id;
    let pid = persona_id;
    let days = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_message_days(cid2, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": persona_id,
        "days": days,
    })))
}

async fn api_send(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<SendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let start = Instant::now();
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let key = format!("chat:{}", chat_id);
    if let Err((status, msg)) = state.request_hub.begin(&key, &state.limits).await {
        info!(
            target: "web",
            endpoint = "/api/send",
            chat_id = chat_id,
            status = status.as_u16(),
            reason = %msg,
            "Request rejected by limiter"
        );
        return Err((status, msg));
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    state.run_hub.create(&run_id).await;
    let state_for_task = state.clone();
    let run_id_for_task = run_id.clone();
    let limits = state.limits.clone();
    let queue_position = state
        .app_state
        .chat_queue
        .enqueue(chat_id, async move {
            state_for_task
                .run_hub
                .publish(
                    &run_id_for_task,
                    "status",
                    json!({"message": "running"}).to_string(),
                    limits.run_history_limit,
                )
                .await;
            match send_and_store_response_with_events(
                state_for_task.clone(),
                body,
                None,
                Some(&run_id_for_task),
            )
            .await
            {
                Ok(resp) => {
                    let response_text = resp
                        .0
                        .get("response")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    state_for_task
                        .run_hub
                        .publish(
                            &run_id_for_task,
                            "done",
                            json!({"response": response_text}).to_string(),
                            limits.run_history_limit,
                        )
                        .await;
                }
                Err((_, err_msg)) => {
                    state_for_task
                        .run_hub
                        .publish(
                            &run_id_for_task,
                            "error",
                            json!({"error": err_msg}).to_string(),
                            limits.run_history_limit,
                        )
                        .await;
                }
            }
            state_for_task
                .run_hub
                .remove_later(run_id_for_task, 300)
                .await;
        })
        .await;
    state
        .run_hub
        .publish(
            &run_id,
            "status",
            json!({
                "message": if queue_position > 1 {
                    format!("queued ({} ahead)", queue_position.saturating_sub(1))
                } else {
                    "queued".to_string()
                }
            })
            .to_string(),
            state.limits.run_history_limit,
        )
        .await;
    state.request_hub.end_with_limits(&key, &state.limits).await;
    info!(
        target: "web",
        endpoint = "/api/send",
        chat_id = chat_id,
        run_id = %run_id,
        queue_position = queue_position,
        latency_ms = start.elapsed().as_millis(),
        "Accepted queued request"
    );
    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "run_id": run_id,
        "state": "queued",
        "queue_position": queue_position,
    })))
}

async fn api_send_stream(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<SendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let start = Instant::now();

    let text = body.message.trim().to_string();
    if text.is_empty() && body.attachments.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is required".into()));
    }

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let key = format!("chat:{}", chat_id);
    if let Err((status, msg)) = state.request_hub.begin(&key, &state.limits).await {
        info!(
            target: "web",
            endpoint = "/api/send_stream",
            chat_id = chat_id,
            status = status.as_u16(),
            reason = %msg,
            "Request rejected by limiter"
        );
        return Err((status, msg));
    }

    let run_id = uuid::Uuid::new_v4().to_string();
    state.run_hub.create(&run_id).await;
    let state_for_task = state.clone();
    let run_id_for_task = run_id.clone();
    let limits = state.limits.clone();
    let queue_position = state
        .app_state
        .chat_queue
        .enqueue(chat_id, async move {
        let run_start = Instant::now();
        state_for_task
            .run_hub
            .publish(
                &run_id_for_task,
                "status",
                json!({"message": "running"}).to_string(),
                limits.run_history_limit,
            )
            .await;

        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let run_hub = state_for_task.run_hub.clone();
        let run_id_for_events = run_id_for_task.clone();
        let run_history_limit = limits.run_history_limit;
        let forward = tokio::spawn(async move {
            while let Some(evt) = evt_rx.recv().await {
                match evt {
                    AgentEvent::Iteration { iteration } => {
                        run_hub
                            .publish(
                                &run_id_for_events,
                                "status",
                                json!({"message": format!("iteration {iteration}")}).to_string(),
                                run_history_limit,
                            )
                            .await;
                    }
                    AgentEvent::WorkflowSelected {
                        workflow_id,
                        confidence,
                    } => {
                        run_hub
                            .publish(
                                &run_id_for_events,
                                "workflow_selected",
                                json!({
                                    "workflow_id": workflow_id,
                                    "confidence": confidence
                                })
                                .to_string(),
                                run_history_limit,
                            )
                            .await;
                    }
                    AgentEvent::ToolStart {
                        tool_use_id,
                        name,
                        input,
                    } => {
                        run_hub
                            .publish(
                                &run_id_for_events,
                                "tool_start",
                                json!({
                                    "tool_use_id": tool_use_id,
                                    "name": name,
                                    "input": input
                                })
                                .to_string(),
                                run_history_limit,
                            )
                            .await;
                    }
                    AgentEvent::ToolResult {
                        tool_use_id,
                        name,
                        is_error,
                        output,
                        duration_ms,
                        status_code,
                        bytes,
                        error_type,
                    } => {
                        run_hub
                            .publish(
                                &run_id_for_events,
                                "tool_result",
                                json!({
                                    "tool_use_id": tool_use_id,
                                    "name": name,
                                    "is_error": is_error,
                                    "output": output,
                                    "duration_ms": duration_ms,
                                    "status_code": status_code,
                                    "bytes": bytes,
                                    "error_type": error_type
                                })
                                .to_string(),
                                run_history_limit,
                            )
                            .await;
                    }
                    AgentEvent::TextDelta { delta } => {
                        run_hub
                            .publish(
                                &run_id_for_events,
                                "delta",
                                json!({"delta": delta}).to_string(),
                                run_history_limit,
                            )
                            .await;
                    }
                    AgentEvent::FinalResponse { .. } => {}
                }
            }
        });

        match send_and_store_response_with_events(
            state_for_task.clone(),
            body,
            Some(&evt_tx),
            Some(&run_id_for_task),
        )
        .await
        {
            Ok(resp) => {
                let response_text = resp
                    .0
                    .get("response")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                if response_text.starts_with(BACKGROUND_JOB_HANDOFF_PREFIX) {
                    let job_id = uuid::Uuid::new_v4().to_string();
                    let prompt_text = resp
                        .0
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let persona_id = resp
                        .0
                        .get("persona_id")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

                    // Allow only one active background subagent per chat to avoid
                    // result interleaving and ambiguous follow-up replies.
                    let active_jobs = call_blocking(state_for_task.app_state.db.clone(), move |db| {
                        db.count_active_background_jobs_for_chat(chat_id)
                    })
                    .await
                    .unwrap_or(0);
                    if active_jobs > 0 {
                        state_for_task
                            .run_hub
                            .publish(
                                &run_id_for_task,
                                "done",
                                json!({
                                    "response": "A background task is already running for this chat. Please wait for it to finish before starting another long-running background task."
                                })
                                .to_string(),
                                limits.run_history_limit,
                            )
                            .await;
                    } else {
                        let jid = job_id.clone();
                        let prompt_for_db = prompt_text.clone();
                        let _ = call_blocking(state_for_task.app_state.db.clone(), move |db| {
                            db.create_background_job(&jid, chat_id, persona_id, &prompt_for_db, "timeout")
                        })
                        .await;

                        crate::background_jobs::spawn_background_job(
                            state_for_task.app_state.clone(),
                            job_id.clone(),
                            chat_id,
                            persona_id,
                            prompt_text,
                        );

                        state_for_task
                            .run_hub
                            .publish(
                                &run_id_for_task,
                                "background_job",
                                json!({
                                    "job_id": job_id,
                                    "message": "Task moved to background due to timeout. You can keep chatting while it runs."
                                })
                                .to_string(),
                                limits.run_history_limit,
                            )
                            .await;

                        state_for_task
                            .run_hub
                            .publish(
                                &run_id_for_task,
                                "done",
                                json!({
                                    "response": "This task is now running as a background subagent. You can continue chatting; a separate reply will arrive when it finishes.",
                                    "background_job_id": job_id
                                })
                                .to_string(),
                                limits.run_history_limit,
                            )
                            .await;
                    }
                } else {
                    state_for_task
                        .run_hub
                        .publish(
                            &run_id_for_task,
                            "done",
                            json!({"response": response_text}).to_string(),
                            limits.run_history_limit,
                        )
                        .await;
                }
            }
            Err((_, err_msg)) => {
                state_for_task
                    .run_hub
                    .publish(
                        &run_id_for_task,
                        "error",
                        json!({"error": err_msg}).to_string(),
                        limits.run_history_limit,
                    )
                    .await;
            }
        }
        drop(evt_tx);
        let _ = forward.await;
        info!(
            target: "web",
            endpoint = "/api/send_stream",
            chat_id = chat_id,
            run_id = %run_id_for_task,
            latency_ms = run_start.elapsed().as_millis(),
            "Stream run finished"
        );

        state_for_task
            .run_hub
            .remove_later(run_id_for_task, 300)
            .await;
    })
        .await;

    state
        .run_hub
        .publish(
            &run_id,
            "status",
            json!({
                "message": if queue_position > 1 {
                    format!("queued ({} ahead)", queue_position.saturating_sub(1))
                } else {
                    "queued".to_string()
                }
            })
            .to_string(),
            limits.run_history_limit,
        )
        .await;
    state.request_hub.end_with_limits(&key, &state.limits).await;
    info!(
        target: "web",
        endpoint = "/api/send_stream",
        chat_id = chat_id,
        run_id = %run_id,
        queue_position = queue_position,
        latency_ms = start.elapsed().as_millis(),
        "Accepted stream run"
    );

    Ok(Json(json!({
        "ok": true,
        "run_id": run_id,
        "state": "queued",
        "queue_position": queue_position,
    })))
}

async fn api_stream(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<StreamQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let start = Instant::now();

    let Some((mut rx, replay, done, replay_truncated, oldest_event_id)) = state
        .run_hub
        .subscribe_with_replay(&query.run_id, query.last_event_id)
        .await
    else {
        return Err((StatusCode::NOT_FOUND, "run not found".into()));
    };
    info!(
        target: "web",
        endpoint = "/api/stream",
        run_id = %query.run_id,
        last_event_id = ?query.last_event_id,
        replay_count = replay.len(),
        replay_truncated = replay_truncated,
        oldest_event_id = ?oldest_event_id,
        latency_ms = start.elapsed().as_millis(),
        "Stream subscription established"
    );

    let stream = async_stream::stream! {
        let meta = Event::default().event("replay_meta").data(
            json!({
                "replay_truncated": replay_truncated,
                "oldest_event_id": oldest_event_id,
                "requested_last_event_id": query.last_event_id,
            })
            .to_string()
        );
        yield Ok::<Event, std::convert::Infallible>(meta);

        let mut finished = false;
        for evt in replay {
            let is_done = evt.event == "done" || evt.event == "error";
            let event = Event::default()
                .id(evt.id.to_string())
                .event(evt.event)
                .data(evt.data);
            yield Ok::<Event, std::convert::Infallible>(event);
            if is_done {
                finished = true;
                break;
            }
        }

        if finished || done {
            return;
        }

        loop {
            match rx.recv().await {
                Ok(evt) => {
                    let done = evt.event == "done" || evt.event == "error";
                    let event = Event::default()
                        .id(evt.id.to_string())
                        .event(evt.event)
                        .data(evt.data);
                    yield Ok::<Event, std::convert::Infallible>(event);
                    if done {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    ))
}

async fn api_run_status(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<RunStatusQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let Some((done, last_event_id)) = state.run_hub.status(&query.run_id).await else {
        return Err((StatusCode::NOT_FOUND, "run not found".into()));
    };
    let timeline_count = call_blocking(state.app_state.db.clone(), {
        let run_key = query.run_id.clone();
        move |db| Ok(db.get_run_timeline_events(&run_key, 500)?.len() as i64)
    })
    .await
    .unwrap_or(0);
    Ok(Json(json!({
        "ok": true,
        "run_id": query.run_id,
        "done": done,
        "last_event_id": last_event_id,
        "timeline_events": timeline_count,
    })))
}

async fn api_queue_diagnostics(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let lanes = state.app_state.chat_queue.diagnostics().await;
    let rows = lanes
        .into_iter()
        .map(|lane| {
            json!({
                "chat_id": lane.chat_id,
                "pending": lane.pending,
                "active_for_ms": lane.active_for_ms,
                "oldest_wait_ms": lane.oldest_wait_ms,
                "last_error": lane.last_error,
                "project_id": lane.project_id,
                "workflow_id": lane.workflow_id,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "ok": true,
        "lanes": rows,
    })))
}

async fn send_and_store_response_with_events(
    state: WebState,
    body: SendRequest,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    run_key: Option<&str>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let raw_text = body.message.trim().to_string();
    let mut text = raw_text.clone();
    let mut image_data: Option<(String, String)> = None;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let attachment_notes =
        process_web_attachments(&state, chat_id, &body.attachments, &mut image_data).await?;
    if !attachment_notes.is_empty() {
        let note_text = attachment_notes.join("\n");
        if text.trim().is_empty() {
            text = note_text;
        } else {
            text = format!("{}\n\n{}", text.trim(), note_text);
        }
    }

    if text.trim().is_empty() && image_data.is_none() {
        return Err((StatusCode::BAD_REQUEST, "message is required".into()));
    }

    // Single entry point: parse slash command first. If command, run backend handler and return — never send to LLM.
    if let Some(cmd) = parse_slash_command(&raw_text) {
        ensure_web_binding_for_universal(&state, chat_id).await?;
        call_blocking(state.app_state.db.clone(), move |db| {
            db.upsert_chat(chat_id, Some("default"), "web")
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let cid = chat_id;
        let persona_id = if let Some(pid) = body.persona_id {
            pid
        } else {
            call_blocking(state.app_state.db.clone(), move |db| {
                db.get_current_persona_id(cid)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        };

        let resp = match cmd {
            SlashCommand::Reset => {
                let cid2 = chat_id;
                let _ = call_blocking(state.app_state.db.clone(), move |db| {
                    db.delete_session(cid2, persona_id)
                })
                .await;
                "Conversation cleared. Principles and per-persona memory are unchanged.".into()
            }
            SlashCommand::Skills => state.app_state.skills.list_skills_formatted(),
            SlashCommand::Persona => {
                crate::persona::handle_persona_command(
                    state.app_state.db.clone(),
                    chat_id,
                    text.trim(),
                    Some(&state.app_state.config),
                )
                .await
            }
            SlashCommand::Schedule => {
                let tasks = call_blocking(state.app_state.db.clone(), |db| {
                    db.get_all_scheduled_tasks_for_display()
                })
                .await;
                match &tasks {
                    Ok(t) => crate::tools::schedule::format_tasks_list_all(t),
                    Err(e) => format!("Error listing tasks: {e}"),
                }
            }
            SlashCommand::Archive => {
                let cid2 = chat_id;
                let pid = persona_id;
                let history = call_blocking(state.app_state.db.clone(), move |db| {
                    db.get_recent_messages(cid2, pid, 500)
                })
                .await
                .unwrap_or_default();
                let messages: Vec<Message> = history
                    .into_iter()
                    .map(|m| Message {
                        role: if m.is_from_bot { "assistant" } else { "user" }.into(),
                        content: MessageContent::Text(m.content),
                    })
                    .collect();
                if messages.is_empty() {
                    "No conversation to archive.".into()
                } else {
                    archive_conversation(
                        &state.app_state.config.runtime_data_dir(),
                        chat_id,
                        &messages,
                    );
                    format!("Archived {} messages.", messages.len())
                }
            }
        };

        deliver_to_contact(
            state.app_state.db.clone(),
            Some(&state.app_state.bot),
            state.app_state.discord_http.as_deref(),
            &state.app_state.config.bot_username,
            chat_id,
            persona_id,
            &resp,
            Some(state.app_state.config.workspace_root_absolute()),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(json!({
            "ok": true,
            "chat_id": chat_id,
            "response": resp,
        })));
    }

    // Not a slash command: normal flow — resolve, store message, run agent
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let sender_name = body
        .sender_name
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("web-user")
        .to_string();

    call_blocking(state.app_state.db.clone(), move |db| {
        db.upsert_chat(chat_id, Some("default"), "web")
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let cid = chat_id;
    let persona_id = if let Some(pid) = body.persona_id {
        pid
    } else {
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    let user_msg = StoredMessage {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id,
        persona_id,
        sender_name: sender_name.clone(),
        content: text,
        is_from_bot: false,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    call_blocking(state.app_state.db.clone(), move |db| {
        db.store_message(&user_msg)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let response = if let Some(tx) = event_tx {
        process_with_agent_with_events(
            &state.app_state,
            AgentRequestContext {
                caller_channel: "web",
                chat_id,
                chat_type: "private",
                persona_id,
                is_scheduled_task: false,
                is_background_job: false,
                run_key: run_key.map(|s| s.to_string()),
            },
            None,
            image_data,
            Some(tx),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        process_with_agent(
            &state.app_state,
            AgentRequestContext {
                caller_channel: "web",
                chat_id,
                chat_type: "private",
                persona_id,
                is_scheduled_task: false,
                is_background_job: false,
                run_key: run_key.map(|s| s.to_string()),
            },
            None,
            image_data,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    if !response.starts_with(BACKGROUND_JOB_HANDOFF_PREFIX) {
        deliver_to_contact(
            state.app_state.db.clone(),
            Some(&state.app_state.bot),
            state.app_state.discord_http.as_deref(),
            &state.app_state.config.bot_username,
            chat_id,
            persona_id,
            &response,
            Some(state.app_state.config.workspace_root_absolute()),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": persona_id,
        "prompt": raw_text,
        "response": response,
    })))
}

async fn process_web_attachments(
    state: &WebState,
    chat_id: i64,
    attachments: &[SendAttachmentRequest],
    image_data: &mut Option<(String, String)>,
) -> Result<Vec<String>, (StatusCode, String)> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }

    let max_bytes = state
        .app_state
        .config
        .max_document_size_mb
        .saturating_mul(1024)
        .saturating_mul(1024);
    let dir = state
        .app_state
        .config
        .workspace_root_absolute()
        .join("shared")
        .join("upload")
        .join("web")
        .join(chat_id.to_string());
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut notes = Vec::new();
    for (idx, att) in attachments.iter().enumerate() {
        let bytes = decode_base64_payload(&att.data_base64).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("invalid attachment base64: {e}"),
            )
        })?;
        let mime = att
            .media_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let filename = att
            .filename
            .clone()
            .unwrap_or_else(|| format!("web-attachment-{}.bin", idx + 1));

        if (bytes.len() as u64) > max_bytes {
            notes.push(format!(
                "[document] filename={} bytes={} mime={} skipped=too_large",
                filename,
                bytes.len(),
                mime
            ));
            continue;
        }

        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let safe_name = sanitize_upload_filename(&filename);
        let path = dir.join(format!("{}-{}-{}", ts, idx + 1, safe_name));
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let saved_file = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tool_path = format!("upload/web/{}/{}", chat_id, saved_file);

        if image_data.is_none() && mime.starts_with("image/") {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_slice());
            *image_data = Some((b64, mime.clone()));
        }

        notes.push(format!(
            "[document] filename={} bytes={} mime={} tool_path={} saved_path={}",
            filename,
            bytes.len(),
            mime,
            tool_path,
            path.display()
        ));
    }

    Ok(notes)
}

fn decode_base64_payload(payload: &str) -> anyhow::Result<Vec<u8>> {
    let raw = payload
        .split_once(',')
        .map(|(_, b64)| b64)
        .unwrap_or(payload)
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|e| anyhow::anyhow!(e))
}

fn sanitize_upload_filename(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "web-upload.bin".to_string()
    } else {
        sanitized
    }
}

/// Clear context: delete only the current persona's session for this contact (per-persona reset).
async fn api_reset(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<ResetRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let cid = chat_id;
    let pid = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_current_persona_id(cid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let cid2 = chat_id;
    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_session(cid2, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "deleted": deleted,
        "message": "Conversation cleared. Principles and per-persona memory are unchanged."
    })))
}

async fn api_delete_session(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<DeleteSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_chat_data(chat_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "deleted": deleted,
        "message": "Conversation cleared. Principles and per-persona memory are unchanged."
    })))
}

async fn api_personas(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<PersonasQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let cid = chat_id;

    let personas: Vec<Persona> =
        call_blocking(state.app_state.db.clone(), move |db| db.list_personas(cid))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let cid2 = chat_id;
    let active_id = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_active_persona_id(cid2)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let cid3 = chat_id;
    let last_bot_rows = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_persona_last_bot_message_at(cid3)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let last_bot_by_persona: HashMap<i64, String> = last_bot_rows.into_iter().collect();

    let items: Vec<serde_json::Value> = personas
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "model_override": p.model_override,
                "is_active": active_id == Some(p.id),
                "last_bot_message_at": last_bot_by_persona.get(&p.id).cloned(),
            })
        })
        .collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "personas": items,
    })))
}

async fn api_personas_switch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<PersonasSwitchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let persona_name = body.persona_name.clone();
    let persona_name_for_msg = persona_name.clone();

    let persona = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_persona_by_name(chat_id, &persona_name)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let Some(persona) = persona else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Persona '{}' not found", persona_name_for_msg),
        ));
    };

    let ok = call_blocking(state.app_state.db.clone(), move |db| {
        db.set_active_persona(chat_id, persona.id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !ok {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to switch persona".into(),
        ));
    }

    Ok(Json(json!({
        "ok": true,
        "message": format!("Switched to {}", persona_name_for_msg),
    })))
}

#[derive(Deserialize)]
struct PersonaMemoryPathParams {
    persona_id: i64,
}

fn ensure_persona_memory_file_exists_for_web(state: &AppState, chat_id: i64, persona_id: i64) {
    let path = state.memory.persona_memory_path(chat_id, persona_id);
    if path.exists() {
        return;
    }
    let template =
        "# Memory\n\n## Tier 1 — Long term\n\n\n## Tier 2 — Mid term\n\n\n## Tier 3 — Short term\n";
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, template);
}

fn file_mtime_ms(path: &std::path::Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

async fn api_persona_memory_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaMemoryPathParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let pid = path.persona_id;
    let exists = call_blocking(state.app_state.db.clone(), move |db| {
        db.persona_exists(chat_id, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !exists {
        return Err((StatusCode::NOT_FOUND, "persona not found".into()));
    }

    ensure_persona_memory_file_exists_for_web(&state.app_state, chat_id, pid);
    let mem_path = state.app_state.memory.persona_memory_path(chat_id, pid);
    let content = std::fs::read_to_string(&mem_path).unwrap_or_default();
    let mtime_ms = file_mtime_ms(&mem_path).unwrap_or(0);
    Ok(Json(json!({
        "ok": true,
        "persona_id": pid,
        "content": content,
        "mtime_ms": mtime_ms,
        "path": mem_path.to_string_lossy(),
    })))
}

#[derive(Deserialize)]
struct PersonaMemoryPutBody {
    content: String,
    if_match_mtime_ms: Option<i64>,
}

async fn api_persona_memory_put(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaMemoryPathParams>,
    Json(body): Json<PersonaMemoryPutBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    if body.content.len() > 256 * 1024 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "memory content too large".into(),
        ));
    }

    let pid = path.persona_id;
    let exists = call_blocking(state.app_state.db.clone(), move |db| {
        db.persona_exists(chat_id, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !exists {
        return Err((StatusCode::NOT_FOUND, "persona not found".into()));
    }

    ensure_persona_memory_file_exists_for_web(&state.app_state, chat_id, pid);
    let mem_path = state.app_state.memory.persona_memory_path(chat_id, pid);
    let current_mtime = file_mtime_ms(&mem_path).unwrap_or(0);
    if let Some(expected) = body.if_match_mtime_ms {
        if expected != current_mtime {
            return Err((
                StatusCode::CONFLICT,
                "memory was modified; reload and retry".into(),
            ));
        }
    }

    if let Some(parent) = mem_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    std::fs::write(&mem_path, body.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let new_mtime = file_mtime_ms(&mem_path).unwrap_or(0);
    Ok(Json(json!({
        "ok": true,
        "persona_id": pid,
        "mtime_ms": new_mtime,
    })))
}

async fn api_personas_create(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<PersonaCreateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Persona name cannot be empty".into(),
        ));
    }
    let name_owned = name.to_string();
    let persona_id = call_blocking(state.app_state.db.clone(), move |db| {
        db.create_persona(chat_id, &name_owned, None)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "persona_id": persona_id,
        "message": format!("Persona '{}' created", name),
    })))
}

async fn api_personas_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<PersonaDeleteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let persona_id = body.persona_id;
    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_persona(chat_id, persona_id)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "deleted": deleted,
        "message": if deleted { "Persona deleted" } else { "Persona not found or cannot delete default" },
    })))
}

async fn api_contacts_bind(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<ContactsBindRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let contact_chat_id = body.contact_chat_id;
    call_blocking(state.app_state.db.clone(), move |db| {
        db.link_channel(contact_chat_id, "web", "default")
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "message": "Web bound to contact",
        "contact_chat_id": contact_chat_id,
    })))
}

async fn api_contacts_unlink(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(_body): Json<ContactsUnlinkRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let removed = call_blocking(state.app_state.db.clone(), move |db| {
        db.unlink_channel("web", "default")
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "removed": removed,
        "message": if removed { "Web unlinked from contact" } else { "No binding found" },
    })))
}

async fn api_schedules_list(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<SchedulesQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let tasks = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_tasks_for_chat(chat_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = tasks
        .into_iter()
        .map(|t| {
            json!({
                "id": t.id,
                "chat_id": t.chat_id,
                "persona_id": t.persona_id,
                "prompt": t.prompt,
                "schedule_type": t.schedule_type,
                "schedule_value": t.schedule_value,
                "next_run": t.next_run,
                "last_run": t.last_run,
                "status": t.status,
                "created_at": t.created_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "tasks": items,
    })))
}

async fn api_schedules_create(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<ScheduleCreateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let effective_tz = body.timezone.as_deref().or_else(|| {
        let default = state.app_state.config.timezone.trim();
        if default.is_empty() {
            None
        } else {
            Some(default)
        }
    });
    let preflight = crate::tools::schedule::preflight_schedule_request(
        &body.schedule_type,
        &body.schedule_value,
        effective_tz,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let prompt = body.prompt;
    let schedule_type = body.schedule_type;
    let schedule_value = preflight.schedule_value.clone();
    let next_run_for_db = preflight.next_run.clone();
    let requested_persona_id = body.persona_id.filter(|id| *id > 0);
    let id = call_blocking(state.app_state.db.clone(), move |db| {
        let persona_id = if let Some(pid) = requested_persona_id {
            if !db.persona_exists(chat_id, pid)? {
                return Err(crate::error::FinallyAValueBotError::ToolExecution(format!(
                    "Persona {} does not exist for this chat",
                    pid
                )));
            }
            pid
        } else {
            db.get_current_persona_id(chat_id)?
        };
        db.create_scheduled_task_for_persona(
            chat_id,
            persona_id,
            &prompt,
            &schedule_type,
            &schedule_value,
            &next_run_for_db,
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "id": id,
        "message": "Schedule created",
        "next_run": preflight.next_run,
        "timezone": preflight.timezone_used,
        "timezone_assumption": if preflight.timezone_defaulted_to_utc {
            "Timezone not provided. UTC was assumed."
        } else {
            "Timezone provided by request."
        },
    })))
}

async fn api_schedules_update(
    headers: HeaderMap,
    State(state): State<WebState>,
    axum::extract::Path(task_id): axum::extract::Path<i64>,
    Json(body): Json<ScheduleUpdateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let status = match body.status.as_deref() {
        Some("paused") => Some("paused"),
        Some("active") | Some("resumed") => Some("active"),
        Some("cancelled") => Some("cancelled"),
        Some(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "status must be paused, active, or cancelled".into(),
            ))
        }
        None => None,
    };
    let persona_id = body.persona_id;
    if status.is_none() && persona_id.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide at least one field to update: status or persona_id".into(),
        ));
    }
    if let Some(pid) = persona_id {
        if pid <= 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                "persona_id must be a positive integer".into(),
            ));
        }
    }

    let task = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_task_by_id(task_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some(task) = task else {
        return Err((StatusCode::NOT_FOUND, "Task not found".into()));
    };
    if task.chat_id != chat_id {
        return Err((StatusCode::NOT_FOUND, "Task not found".into()));
    }

    if let Some(pid) = persona_id {
        let exists = call_blocking(state.app_state.db.clone(), move |db| {
            db.persona_exists(chat_id, pid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !exists {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Persona {} does not exist for this chat", pid),
            ));
        }
    }

    if let Some(next_status) = status {
        let ok = call_blocking(state.app_state.db.clone(), move |db| {
            db.update_task_status(task_id, next_status)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !ok {
            return Err((StatusCode::NOT_FOUND, "Task not found".into()));
        }
    }
    if let Some(pid) = persona_id {
        let ok = call_blocking(state.app_state.db.clone(), move |db| {
            db.update_task_persona(task_id, pid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !ok {
            return Err((StatusCode::NOT_FOUND, "Task not found".into()));
        }
    }

    Ok(Json(json!({
        "ok": true,
        "message": "Task updated",
    })))
}

// --- Background jobs API ---

#[derive(Debug, Deserialize)]
struct BackgroundJobsQuery {
    chat_id: Option<i64>,
    limit: Option<usize>,
}

async fn api_background_jobs_list(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<BackgroundJobsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    let limit = query.limit.unwrap_or(20).min(100);
    let jobs = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_background_jobs_for_chat(chat_id, limit)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = jobs
        .into_iter()
        .map(|j| {
            json!({
                "id": j.id,
                "chat_id": j.chat_id,
                "persona_id": j.persona_id,
                "prompt": j.prompt,
                "status": j.status,
                "trigger_reason": j.trigger_reason,
                "created_at": j.created_at,
                "started_at": j.started_at,
                "finished_at": j.finished_at,
                "result_preview": j.result_text.as_deref().map(|t| if t.len() > 200 { &t[..200] } else { t }),
                "error_text": j.error_text,
            })
        })
        .collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "jobs": items,
    })))
}

#[derive(Debug, Deserialize)]
struct ContactsBindingsQuery {
    chat_id: Option<i64>,
}

async fn api_contacts_bindings(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<ContactsBindingsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let bindings = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_bindings_for_contact(chat_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = bindings
        .into_iter()
        .map(|b| json!({ "channel_type": b.channel_type, "channel_handle": b.channel_handle }))
        .collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "bindings": items,
    })))
}

pub async fn start_web_server(state: Arc<AppState>) {
    let limits = WebLimits::from_config(&state.config);
    let web_state = WebState {
        auth_token: state.config.web_auth_token.clone(),
        app_state: state.clone(),
        run_hub: RunHub::default(),
        request_hub: RequestHub::default(),
        limits,
    };

    let router = build_router(web_state);

    let addr = format!("{}:{}", state.config.web_host, state.config.web_port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind web server at {}: {}", addr, e);
            return;
        }
    };

    info!("Web UI available at http://{addr}");
    if let Err(e) = axum::serve(listener, router).await {
        error!("Web server error: {e}");
    }
}

async fn asset_file(Path(file): Path<String>) -> impl IntoResponse {
    let clean = file.replace("..", "");
    match WEB_ASSETS.get_file(format!("assets/{clean}")) {
        Some(file) => {
            let content_type = if clean.ends_with(".css") {
                "text/css; charset=utf-8"
            } else if clean.ends_with(".js") {
                "application/javascript; charset=utf-8"
            } else {
                "application/octet-stream"
            };
            ([("content-type", content_type)], file.contents().to_vec()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

async fn upload_file(State(state): State<WebState>, Path(path): Path<String>) -> impl IntoResponse {
    let clean = path.replace("..", "");
    if clean.is_empty() {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }
    let full_path = FsPath::new(state.app_state.config.working_dir())
        .join("uploads")
        .join(clean);
    if !full_path.is_file() {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    match tokio::fs::read(&full_path).await {
        Ok(bytes) => {
            let content_type = guess_upload_content_type(&full_path);
            ([("content-type", content_type)], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

fn guess_upload_content_type(path: &FsPath) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("txt") => "text/plain; charset=utf-8",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

async fn icon_file() -> impl IntoResponse {
    match WEB_ASSETS.get_file("icon.png") {
        Some(file) => ([("content-type", "image/png")], file.contents().to_vec()).into_response(),
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

async fn favicon_file() -> impl IntoResponse {
    if let Some(file) = WEB_ASSETS.get_file("favicon.ico") {
        return ([("content-type", "image/x-icon")], file.contents().to_vec()).into_response();
    }
    if let Some(file) = WEB_ASSETS.get_file("icon.png") {
        return ([("content-type", "image/png")], file.contents().to_vec()).into_response();
    }
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

#[derive(Debug, Deserialize)]
struct OAuthAuthorizeQuery {
    chat_id: Option<i64>,
}

async fn api_oauth_authorize(
    State(state): State<WebState>,
    Path(platform): Path<String>,
    Query(query): Query<OAuthAuthorizeQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let platform = platform.to_lowercase();
    if !["tiktok", "instagram", "linkedin"].contains(&platform.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "Unknown platform".into()));
    }
    if state
        .app_state
        .config
        .social
        .as_ref()
        .map_or(true, |s| !s.is_platform_enabled(&platform))
    {
        return Err((StatusCode::BAD_REQUEST, "Platform not configured".into()));
    }

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;

    let state_token = uuid::Uuid::new_v4().simple().to_string();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();

    let platform_clone = platform.clone();
    let state_token_clone = state_token.clone();
    call_blocking(state.app_state.db.clone(), move |db| {
        db.create_oauth_pending_state(&state_token_clone, &platform_clone, chat_id, &expires_at)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let auth_url = social_oauth::authorize_url(&state.app_state.config, &platform, &state_token)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build authorize URL".into(),
            )
        })?;

    Ok(axum::response::Redirect::temporary(&auth_url))
}

#[derive(Debug, Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn api_oauth_callback(
    State(state): State<WebState>,
    Path(platform): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
) -> impl IntoResponse {
    let platform = platform.to_lowercase();

    if let (Some(err), desc) = (query.error, query.error_description.as_deref()) {
        let msg = desc.unwrap_or(&err);
        return (
            StatusCode::BAD_REQUEST,
            Html(format!(
                r#"<!DOCTYPE html><html><head><title>OAuth Error</title></head><body>
                <h1>Authorization failed</h1><p>{}</p></body></html>"#,
                msg.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            )),
        )
            .into_response();
    }

    let (code, state_token) = match (query.code, query.state) {
        (Some(c), Some(s)) => (c, s),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    r#"<!DOCTYPE html><html><head><title>OAuth Error</title></head><body>
                    <h1>Missing code or state</h1></body></html>"#,
                ),
            )
                .into_response();
        }
    };

    let Some((stored_platform, chat_id)) = call_blocking(state.app_state.db.clone(), move |db| {
        db.consume_oauth_pending_state(&state_token)
    })
    .await
    .ok()
    .flatten() else {
        return (
            StatusCode::BAD_REQUEST,
            Html(
                r#"<!DOCTYPE html><html><head><title>OAuth Error</title></head><body>
                <h1>Invalid or expired state</h1><p>Please try the authorization flow again.</p></body></html>"#,
            ),
        )
            .into_response();
    };

    if stored_platform != platform {
        return (
            StatusCode::BAD_REQUEST,
            Html(
                r#"<!DOCTYPE html><html><head><title>OAuth Error</title></head><body>
                <h1>Platform mismatch</h1></body></html>"#,
            ),
        )
            .into_response();
    }

    let base = social_oauth::oauth_base_url(&state.app_state.config).unwrap_or_default();
    let redirect_uri = format!(
        "{}/api/oauth/callback/{}",
        base.trim_end_matches('/'),
        platform
    );

    let token_result =
        match social_oauth::exchange_code(&state.app_state.config, &platform, &code, &redirect_uri)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(format!(
                        r#"<!DOCTYPE html><html><head><title>OAuth Error</title></head><body>
                    <h1>Token exchange failed</h1><p>{}</p></body></html>"#,
                        e.to_string()
                            .replace('&', "&amp;")
                            .replace('<', "&lt;")
                            .replace('>', "&gt;")
                    )),
                )
                    .into_response();
            }
        };

    let platform_for_db = platform.clone();
    if let Err(e) = call_blocking(state.app_state.db.clone(), move |db| {
        db.upsert_social_token(
            &platform_for_db,
            chat_id,
            &token_result.access_token,
            token_result.refresh_token.as_deref(),
            token_result.expires_at.as_deref(),
        )
    })
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!(
                r#"<!DOCTYPE html><html><head><title>OAuth Error</title></head><body>
                <h1>Failed to store token</h1><p>{}</p></body></html>"#,
                html_escape::encode_text(&e.to_string()).to_string()
            )),
        )
            .into_response();
    }

    let platform_name = match platform.as_str() {
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        "linkedin" => "LinkedIn",
        _ => &platform,
    };

    (
        StatusCode::OK,
        Html(format!(
            r#"<!DOCTYPE html><html><head><title>Authorization successful</title></head><body>
            <h1>Authorization successful</h1>
            <p>{} has been connected. You can now ask the bot to fetch your feed.</p>
            <p><a href="/">Back to chat</a></p></body></html>"#,
            platform_name
        )),
    )
        .into_response()
}

fn build_router(web_state: WebState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/assets/*file", get(asset_file))
        .route("/api/uploads/*path", get(upload_file))
        .route("/icon.png", get(icon_file))
        .route("/favicon.ico", get(favicon_file))
        .route("/api/health", get(api_health))
        .route("/api/chat", get(api_chat))
        .route("/api/contacts/bind", post(api_contacts_bind))
        .route("/api/contacts/unlink", post(api_contacts_unlink))
        .route("/api/contacts/bindings", get(api_contacts_bindings))
        .route(
            "/api/schedules",
            get(api_schedules_list).post(api_schedules_create),
        )
        .route("/api/schedules/:id", patch(api_schedules_update))
        .route("/api/background_jobs", get(api_background_jobs_list))
        .route("/api/history", get(api_history))
        .route("/api/history/days", get(api_history_days))
        .route("/api/send", post(api_send))
        .route("/api/send_stream", post(api_send_stream))
        .route("/api/stream", get(api_stream))
        .route("/api/run_status", get(api_run_status))
        .route("/api/queue_diagnostics", get(api_queue_diagnostics))
        .route("/api/reset", post(api_reset))
        .route("/api/delete_session", post(api_delete_session))
        .route("/api/personas", get(api_personas))
        .route("/api/personas/switch", post(api_personas_switch))
        .route("/api/personas/create", post(api_personas_create))
        .route("/api/personas/delete", post(api_personas_delete))
        .route(
            "/api/personas/:persona_id/memory",
            get(api_persona_memory_get).put(api_persona_memory_put),
        )
        .route("/api/oauth/authorize/:platform", get(api_oauth_authorize))
        .route("/api/oauth/callback/:platform", get(api_oauth_callback))
        .with_state(web_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::call_blocking;
    use crate::llm::LlmProvider;
    use crate::{claude::ResponseContentBlock, error::FinallyAValueBotError};
    use crate::{db::Database, memory::MemoryManager, skills::SkillManager, tools::ToolRegistry};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use teloxide::Bot;
    use tower::ServiceExt;

    #[test]
    fn test_web_assets_embedded() {
        assert!(
            WEB_ASSETS.get_file("index.html").is_some(),
            "embedded web asset missing: index.html"
        );
        assert!(
            WEB_ASSETS.get_file("icon.png").is_some(),
            "embedded web asset missing: icon.png"
        );
        let assets_dir = WEB_ASSETS.get_dir("assets");
        assert!(
            assets_dir.is_some(),
            "embedded web asset dir missing: assets"
        );
        assert!(
            assets_dir.unwrap().files().next().is_some(),
            "embedded web asset dir is empty: assets"
        );
    }

    struct DummyLlm;

    #[async_trait::async_trait]
    impl LlmProvider for DummyLlm {
        async fn send_message(
            &self,
            _system: &str,
            _messages: Vec<crate::claude::Message>,
            _tools: Option<Vec<crate::claude::ToolDefinition>>,
        ) -> Result<crate::claude::MessagesResponse, crate::error::FinallyAValueBotError> {
            Ok(crate::claude::MessagesResponse {
                content: vec![crate::claude::ResponseContentBlock::Text {
                    text: "hello from llm".into(),
                }],
                stop_reason: Some("end_turn".into()),
                usage: None,
            })
        }

        async fn send_message_stream(
            &self,
            _system: &str,
            _messages: Vec<crate::claude::Message>,
            _tools: Option<Vec<crate::claude::ToolDefinition>>,
            text_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
        ) -> Result<crate::claude::MessagesResponse, crate::error::FinallyAValueBotError> {
            if let Some(tx) = text_tx {
                let _ = tx.send("hello ".into());
                let _ = tx.send("from llm".into());
            }
            self.send_message("", vec![], None).await
        }
    }

    struct SlowLlm {
        sleep_ms: u64,
    }

    #[async_trait::async_trait]
    impl LlmProvider for SlowLlm {
        async fn send_message(
            &self,
            _system: &str,
            _messages: Vec<crate::claude::Message>,
            _tools: Option<Vec<crate::claude::ToolDefinition>>,
        ) -> Result<crate::claude::MessagesResponse, FinallyAValueBotError> {
            tokio::time::sleep(Duration::from_millis(self.sleep_ms)).await;
            Ok(crate::claude::MessagesResponse {
                content: vec![ResponseContentBlock::Text {
                    text: "slow".into(),
                }],
                stop_reason: Some("end_turn".into()),
                usage: None,
            })
        }
    }

    struct ToolFlowLlm {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl LlmProvider for ToolFlowLlm {
        async fn send_message(
            &self,
            _system: &str,
            _messages: Vec<crate::claude::Message>,
            _tools: Option<Vec<crate::claude::ToolDefinition>>,
        ) -> Result<crate::claude::MessagesResponse, FinallyAValueBotError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Ok(crate::claude::MessagesResponse {
                    content: vec![ResponseContentBlock::ToolUse {
                        id: "tool_1".into(),
                        name: "glob".into(),
                        input: json!({"pattern": "*.rs", "path": "."}),
                        thought_signature: None,
                    }],
                    stop_reason: Some("tool_use".into()),
                    usage: None,
                });
            }
            Ok(crate::claude::MessagesResponse {
                content: vec![ResponseContentBlock::Text {
                    text: "after tool".into(),
                }],
                stop_reason: Some("end_turn".into()),
                usage: None,
            })
        }
    }

    fn test_state(llm: Arc<dyn LlmProvider>) -> Arc<AppState> {
        let mut cfg = Config {
            telegram_bot_token: "tok".into(),
            bot_username: "bot".into(),
            llm_provider: "anthropic".into(),
            api_key: "key".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            llm_base_url: None,
            max_tokens: 8192,
            max_tool_iterations: 100,
            max_history_messages: 50,
            max_document_size_mb: 100,
            workspace_dir: "./workspace".into(),
            openai_api_key: None,
            timezone: "UTC".into(),
            allowed_groups: vec![],
            control_chat_ids: vec![],
            max_session_messages: 40,
            compact_keep_recent: 20,
            whatsapp_access_token: None,
            whatsapp_phone_number_id: None,
            whatsapp_verify_token: None,
            whatsapp_webhook_port: 8080,
            discord_bot_token: None,
            discord_allowed_channels: vec![],
            show_thinking: false,
            web_enabled: true,
            web_host: "127.0.0.1".into(),
            web_port: 3900,
            web_auth_token: None,
            web_max_inflight_per_session: 2,
            web_max_requests_per_window: 8,
            web_rate_window_seconds: 10,
            web_run_history_limit: 512,
            web_session_idle_ttl_seconds: 300,
            universal_chat_id: Some(997894126),
            browser_managed: false,
            browser_executable_path: None,
            browser_cdp_port_base: 9222,
            browser_idle_timeout_secs: None,
            browser_headless: false,
            safety_output_guard_mode: "moderate".into(),
            safety_max_emojis_per_response: 12,
            safety_tail_repeat_limit: 8,
            safety_execution_mode: "warn_confirm".into(),
            safety_risky_categories: vec![
                "destructive".into(),
                "system".into(),
                "network".into(),
                "package".into(),
            ],
            agent_browser_path: None,
            web_search_searxng_url: None,
            cursor_agent_cli_path: "cursor-agent".into(),
            cursor_agent_model: String::new(),
            cursor_agent_timeout_secs: 1500,
            social: None,
            vault: None,
            orchestrator_enabled: true,
            orchestrator_model: String::new(),
            tool_skill_agent_enabled: true,
            tool_skill_agent_model: String::new(),
            post_tool_evaluator_enabled: false,
            post_tool_evaluator_model: String::new(),
            delegate_tool_enabled: true,
            delegate_max_iterations: 10,
            delegate_model: String::new(),
            cursor_agent_tmux_session_prefix: "finally_a_value_bot-cursor".into(),
            cursor_agent_tmux_enabled: true,
            cursor_agent_runner_url: None,
            scheduler_task_timeout_secs: 3600,
            scheduler_stale_running_reclaim_secs: 7200,
            scheduler_max_concurrent_tasks: 2,
            scheduler_poll_interval_secs: 60,
        };
        let dir = std::env::temp_dir().join(format!(
            "finally_a_value_bot_webtest_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        cfg.workspace_dir = dir.to_string_lossy().to_string();
        let runtime_dir = cfg.runtime_data_dir();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let db = Arc::new(Database::new(&runtime_dir).unwrap());
        let bot = Bot::new("123456:TEST_TOKEN");
        let state = AppState {
            config: cfg.clone(),
            bot: bot.clone(),
            db: db.clone(),
            memory: MemoryManager::new(&runtime_dir, cfg.working_dir()),
            skills: {
                let root = cfg.workspace_root_absolute();
                SkillManager::from_skills_dirs([
                    root.join("skills"),
                    root.join("shared").join("skills"),
                ])
            },
            llm,
            tools: ToolRegistry::new(&cfg, bot, db),
            discord_http: None,
            chat_queue: crate::chat_queue::ChatRunQueue::default(),
        };
        Arc::new(state)
    }

    fn test_web_state(
        llm: Arc<dyn LlmProvider>,
        auth_token: Option<String>,
        limits: WebLimits,
    ) -> WebState {
        let state = test_state(llm);
        WebState {
            app_state: state,
            auth_token,
            run_hub: RunHub::default(),
            request_hub: RequestHub::default(),
            limits,
        }
    }

    #[tokio::test]
    async fn test_send_stream_then_stream_done() {
        let web_state = test_web_state(Arc::new(DummyLlm), None, WebLimits::default());
        let app = build_router(web_state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/send_stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sender_name":"u","message":"hi"}"#))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = v.get("run_id").and_then(|x| x.as_str()).unwrap();

        let req2 = Request::builder()
            .method("GET")
            .uri(format!("/api/stream?run_id={run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp2 = app.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp2.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("event: delta"));
        assert!(text.contains("event: done"));
    }

    #[tokio::test]
    async fn test_slash_command_via_send_stream_returns_done_with_response() {
        let web_state = test_web_state(Arc::new(DummyLlm), None, WebLimits::default());
        let app = build_router(web_state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/send_stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sender_name":"u","message":"/reset"}"#))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = v.get("run_id").and_then(|x| x.as_str()).unwrap();

        let req_stream = Request::builder()
            .method("GET")
            .uri(format!("/api/stream?run_id={run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp_stream = app.oneshot(req_stream).await.unwrap();
        assert_eq!(resp_stream.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp_stream.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);

        assert!(
            text.contains("event: done"),
            "stream should contain event: done"
        );
        assert!(
            text.contains("Conversation cleared"),
            "done event should contain slash command response"
        );
        assert!(
            !text.contains("event: delta"),
            "slash command should return only in done, no deltas"
        );
    }

    #[tokio::test]
    async fn test_auth_failure_requires_header() {
        let web_state = test_web_state(
            Arc::new(DummyLlm),
            Some("secret-token".into()),
            WebLimits::default(),
        );
        let app = build_router(web_state);

        let req = Request::builder()
            .method("GET")
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_same_session_concurrency_limited() {
        let limits = WebLimits {
            max_inflight_per_session: 1,
            max_requests_per_window: 10,
            rate_window: Duration::from_secs(10),
            run_history_limit: 128,
            session_idle_ttl: Duration::from_secs(60),
        };
        let web_state = test_web_state(Arc::new(SlowLlm { sleep_ms: 300 }), None, limits);
        let app = build_router(web_state);

        let req1 = Request::builder()
            .method("POST")
            .uri("/api/send")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sender_name":"u","message":"one"}"#))
            .unwrap();
        let req2 = Request::builder()
            .method("POST")
            .uri("/api/send")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sender_name":"u","message":"two"}"#))
            .unwrap();

        let app_a = app.clone();
        let first = tokio::spawn(async move { app_a.oneshot(req1).await.unwrap() });
        tokio::time::sleep(Duration::from_millis(40)).await;
        let resp2 = app.clone().oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);

        let resp1 = first.await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_stream_includes_tool_events_and_replay() {
        let web_state = test_web_state(
            Arc::new(ToolFlowLlm {
                calls: AtomicUsize::new(0),
            }),
            None,
            WebLimits::default(),
        );
        let app = build_router(web_state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/send_stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sender_name":"u","message":"do tool"}"#))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = v.get("run_id").and_then(|x| x.as_str()).unwrap();

        let req_stream = Request::builder()
            .method("GET")
            .uri(format!("/api/stream?run_id={run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp_stream = app.clone().oneshot(req_stream).await.unwrap();
        assert_eq!(resp_stream.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp_stream.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("event: tool_start"));
        assert!(text.contains("event: tool_result"));
        assert!(text.contains("event: done"));

        let req_status = Request::builder()
            .method("GET")
            .uri(format!("/api/run_status?run_id={run_id}"))
            .body(Body::empty())
            .unwrap();
        let status_resp = app.clone().oneshot(req_status).await.unwrap();
        assert_eq!(status_resp.status(), StatusCode::OK);
        let status_body = axum::body::to_bytes(status_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let status_json: serde_json::Value = serde_json::from_slice(&status_body).unwrap();
        let last_event_id = status_json
            .get("last_event_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert!(last_event_id > 0);

        let req_replay = Request::builder()
            .method("GET")
            .uri(format!(
                "/api/stream?run_id={run_id}&last_event_id={last_event_id}"
            ))
            .body(Body::empty())
            .unwrap();
        let replay_resp = app.oneshot(req_replay).await.unwrap();
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let replay_bytes = axum::body::to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let replay_text = String::from_utf8_lossy(&replay_bytes);
        // Nothing newer than last_event_id; only replay metadata should be present.
        assert!(replay_text.contains("event: replay_meta"));
        assert!(!replay_text.contains("event: delta"));
        assert!(!replay_text.contains("event: done"));
    }

    #[tokio::test]
    async fn test_reconnect_from_last_event_id_gets_non_empty_replay() {
        let web_state = test_web_state(Arc::new(DummyLlm), None, WebLimits::default());
        let app = build_router(web_state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/send_stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sender_name":"u","message":"reconnect"}"#))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let run_id = v.get("run_id").and_then(|x| x.as_str()).unwrap();

        let req_stream = Request::builder()
            .method("GET")
            .uri(format!("/api/stream?run_id={run_id}"))
            .body(Body::empty())
            .unwrap();
        let resp_stream = app.clone().oneshot(req_stream).await.unwrap();
        assert_eq!(resp_stream.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp_stream.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);

        let mut ids = Vec::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("id: ") {
                if let Ok(id) = rest.trim().parse::<u64>() {
                    ids.push(id);
                }
            }
        }
        assert!(ids.len() >= 2);
        let reconnect_from = ids[0];

        let req_replay = Request::builder()
            .method("GET")
            .uri(format!(
                "/api/stream?run_id={run_id}&last_event_id={reconnect_from}"
            ))
            .body(Body::empty())
            .unwrap();
        let replay_resp = app.oneshot(req_replay).await.unwrap();
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let replay_bytes = axum::body::to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let replay_text = String::from_utf8_lossy(&replay_bytes);
        assert!(replay_text.contains("event: delta") || replay_text.contains("event: done"));
    }

    #[tokio::test]
    async fn test_rate_limit_window_recovers() {
        let limits = WebLimits {
            max_inflight_per_session: 2,
            max_requests_per_window: 1,
            rate_window: Duration::from_millis(200),
            run_history_limit: 128,
            session_idle_ttl: Duration::from_secs(60),
        };
        let web_state = test_web_state(Arc::new(DummyLlm), None, limits);
        let app = build_router(web_state);

        let mk_req = |msg: &str| {
            Request::builder()
                .method("POST")
                .uri("/api/send")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"sender_name":"u","message":"{}"}}"#,
                    msg
                )))
                .unwrap()
        };

        let resp1 = app.clone().oneshot(mk_req("r1")).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let resp2 = app.clone().oneshot(mk_req("r2")).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);

        tokio::time::sleep(Duration::from_millis(260)).await;
        let resp3 = app.oneshot(mk_req("r3")).await.unwrap();
        assert_eq!(resp3.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_db_paths_use_call_blocking_in_web_flow() {
        let state = test_state(Arc::new(DummyLlm));
        let chat_id = 12345_i64;
        let cid = chat_id;
        let pid = call_blocking(state.db.clone(), move |db| db.get_current_persona_id(cid))
            .await
            .unwrap_or(0);
        let cid2 = chat_id;
        let message_count =
            call_blocking(state.db.clone(), move |db| db.get_all_messages(cid2, pid))
                .await
                .unwrap()
                .len();
        assert_eq!(message_count, 0);
    }
}
