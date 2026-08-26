use std::collections::{HashMap, VecDeque};
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::WebSocket;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use base64::Engine;
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info, warn};

use crate::background_jobs::{
    await_handoff_startup_ack, handoff_trigger_for_db, is_background_handoff_response,
    try_enqueue_background_handoff, HandoffEnqueueOutcome,
};
use crate::channel::{deliver_agent_final_to_contact, deliver_to_contact, DeliveryScope};
use crate::chat_queue::{QueueEnqueueMeta, QueueRemoveOutcome, QueueSource};
use crate::claude::{Message, MessageContent};
use crate::config::Config;
use crate::db::{
    call_blocking, ChannelBotInstance, ChannelPersonaMode, JobHeartbeat, Persona, StoredMessage,
    BOT_INSTANCE_WEB,
};
use crate::hook_executor::validate_command_payload;
use crate::slash_commands::{parse as parse_slash_command, SlashCommand};
use crate::social_oauth;
use crate::telegram::{
    archive_conversation, process_with_agent, process_with_agent_with_events, AgentEvent,
    AgentRequestContext, AppState,
};
use crate::web_terminal::{self, TerminalHub};
use std::time::SystemTime;

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

#[derive(Clone)]
struct WebState {
    app_state: Arc<AppState>,
    auth_token: Option<String>,
    run_hub: RunHub,
    request_hub: RequestHub,
    terminal_hub: TerminalHub,
    limits: WebLimits,
    /// Cache for `ensure_web_binding_for_universal` to avoid repeated DB writes on every
    /// `/api/history` poll while this server process is alive.
    web_binding_universal_done: Arc<Mutex<Option<i64>>>,
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
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendRequest {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    sender_name: Option<String>,
    message: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    attachments: Vec<SendAttachmentRequest>,
}

#[derive(Debug, Deserialize)]
struct SendAttachmentRequest {
    filename: Option<String>,
    media_type: Option<String>,
    /// Inline base64 payload (legacy). Prefer `tool_path` + `url` from `POST /api/uploads`.
    #[serde(default)]
    data_base64: Option<String>,
    /// Relative path under `shared/upload/` (e.g. `upload/web/{chat_id}/{persona_id}/{file}`).
    #[serde(default)]
    tool_path: Option<String>,
    /// Public URL path (e.g. `/api/uploads/web/{chat_id}/{persona_id}/{file}`).
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UploadQueryParams {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct UploadResponse {
    filename: String,
    media_type: String,
    bytes: u64,
    tool_path: String,
    url: String,
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
struct TodosQuery {
    chat_id: Option<i64>,
    /// open (default) | done | all
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TodoPatchRequest {
    /// open | done
    status: String,
}

#[derive(Debug, Deserialize)]
struct TodoPathParams {
    id: i64,
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
    prompt: Option<String>,
    /// When changing timing, send both `schedule_type` and `schedule_value` (same semantics as create).
    schedule_type: Option<String>,
    schedule_value: Option<String>,
    timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunStatusQuery {
    run_id: String,
    /// When true, include `timeline_events` count in the response.
    /// Defaults to `false` to keep polling cheap.
    timeline_events: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ArtifactQuery {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    kind: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize, Clone)]
struct ArtifactItem {
    id: String,
    name: String,
    kind: String,
    size_bytes: Option<u64>,
    created_at: Option<String>,
    source: String,
    url: String,
    preview_url: String,
}

#[derive(Debug, Deserialize)]
struct UploadQuery {
    preview: Option<bool>,
    download: Option<bool>,
}

fn artifact_kind_from_filename(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg")
    {
        "image".to_string()
    } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
        "markdown".to_string()
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        "html".to_string()
    } else if lower.ends_with(".txt")
        || lower.ends_with(".json")
        || lower.ends_with(".log")
        || lower.ends_with(".csv")
        || lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".xml")
    {
        "text".to_string()
    } else {
        "other".to_string()
    }
}

fn extract_upload_urls_from_text(text: &str) -> Vec<String> {
    let Some(re) = regex::Regex::new(r#"/api/uploads/[^\s\)\]\(<>"']+"#).ok() else {
        return Vec::new();
    };
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

fn system_time_to_rfc3339(value: std::time::SystemTime) -> Option<String> {
    let datetime: chrono::DateTime<chrono::Utc> = value.into();
    Some(datetime.to_rfc3339())
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
fn resolve_chat_id_for_web(
    _chat_id: Option<i64>,
    config: &Config,
) -> Result<i64, (StatusCode, String)> {
    Ok(config.operator_inbox_chat_id())
}

/// Ensure web/default always points to the configured universal chat.
/// This allows UNIVERSAL_CHAT_ID changes to take effect on restart.
async fn ensure_web_binding_for_universal(
    state: &WebState,
    chat_id: i64,
) -> Result<(), (StatusCode, String)> {
    // Web always resolves to the same universal chat id; cache to avoid repeated writes.
    let cid = chat_id;
    {
        let guard = state.web_binding_universal_done.lock().await;
        if guard.is_some_and(|id| id == cid) {
            return Ok(());
        }
    }
    call_blocking(state.app_state.db.clone(), move |db| {
        db.upsert_chat(cid, None, "web")?;
        db.link_channel(cid, BOT_INSTANCE_WEB, "web", "default")?;
        Ok(())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    {
        let mut guard = state.web_binding_universal_done.lock().await;
        *guard = Some(cid);
    }
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

async fn resolve_history_persona_id(
    state: &WebState,
    chat_id: i64,
    persona_id: Option<i64>,
) -> Result<i64, (StatusCode, String)> {
    let pid = match persona_id {
        Some(id) if id > 0 => id,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "persona_id query parameter is required".into(),
            ))
        }
    };
    let exists = call_blocking(state.app_state.db.clone(), move |db| {
        db.persona_exists(chat_id, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !exists {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Persona {pid} does not exist for this chat"),
        ));
    }
    Ok(pid)
}

async fn api_history(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let start = Instant::now();
    let requested_day = query.day.clone();
    let requested_limit = query.limit;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let persona_id = resolve_history_persona_id(&state, chat_id, query.persona_id).await?;
    let cid2 = chat_id;
    let pid = persona_id;

    let messages = if let Some(ref sid) = query.session_id {
        let session_id = sid.clone();
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_all_messages_for_session(&session_id)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else if let Some(ref day) = requested_day {
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
        match requested_limit {
            Some(limit) => call_blocking(state.app_state.db.clone(), move |db| {
                db.get_recent_messages(cid2, pid, limit, false)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
            None => call_blocking(state.app_state.db.clone(), move |db| {
                db.get_all_messages(cid2, pid)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        }
    };

    let items: Vec<HistoryItem> = messages
        .into_iter()
        .map(|m| HistoryItem {
            id: m.id,
            sender_name: m.sender_name,
            content: crate::agent_turn_context::strip_stored_dialogue_markup(&m.content),
            is_from_bot: m.is_from_bot,
            timestamp: m.timestamp,
        })
        .collect();

    info!(
        target: "web",
        endpoint = "/api/history",
        chat_id = chat_id,
        persona_id = persona_id,
        day = ?requested_day,
        limit = ?requested_limit,
        returned_messages = items.len(),
        duration_ms = start.elapsed().as_millis(),
        "History fetched"
    );

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

    let persona_id = resolve_history_persona_id(&state, chat_id, query.persona_id).await?;
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

async fn list_chat_artifacts(
    state: &WebState,
    chat_id: i64,
    persona_id: i64,
    kind_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ArtifactItem>, (StatusCode, String)> {
    let shared_dir = state
        .app_state
        .config
        .workspace_root_absolute()
        .join("shared")
        .join("upload")
        .join("web")
        .join(chat_id.to_string())
        .join(persona_id.to_string());
    let legacy_dir = FsPath::new(state.app_state.config.working_dir())
        .join("uploads")
        .join("web")
        .join(chat_id.to_string());

    let mut by_url: HashMap<String, ArtifactItem> = HashMap::new();

    let mut scan_dir = |dir: &std::path::Path, source: &str| {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
            else {
                continue;
            };
            let kind = artifact_kind_from_filename(&name);
            if let Some(filter) = kind_filter {
                if filter != "all" && filter != kind {
                    continue;
                }
            }
            let url = format!("/api/uploads/web/{chat_id}/{persona_id}/{name}");
            let preview_url = format!("{url}?preview=1");
            let meta = std::fs::metadata(&path).ok();
            let created_at = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(system_time_to_rfc3339);
            let item = ArtifactItem {
                id: format!("{}:{name}", source),
                name: name.clone(),
                kind,
                size_bytes: meta.as_ref().map(|m| m.len()),
                created_at,
                source: source.to_string(),
                url: url.clone(),
                preview_url,
            };
            by_url.entry(url).or_insert(item);
        }
    };

    scan_dir(&shared_dir, "web_upload");
    scan_dir(&legacy_dir, "legacy_upload");

    let cid = chat_id;
    let pid = persona_id;
    let history = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_all_messages(cid, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for msg in &history {
        for url in extract_upload_urls_from_text(&msg.content) {
            if by_url.contains_key(&url) {
                continue;
            }
            let name = url.rsplit('/').next().unwrap_or("artifact.bin").to_string();
            let kind = artifact_kind_from_filename(&name);
            if let Some(filter) = kind_filter {
                if filter != "all" && filter != kind {
                    continue;
                }
            }
            by_url.insert(
                url.clone(),
                ArtifactItem {
                    id: format!("message:{}:{}", msg.id, url),
                    name,
                    kind,
                    size_bytes: None,
                    created_at: Some(msg.timestamp.clone()),
                    source: "message_link".to_string(),
                    preview_url: format!("{url}?preview=1"),
                    url,
                },
            );
        }
    }

    let mut items: Vec<ArtifactItem> = by_url.into_values().collect();
    items.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.name.cmp(&b.name))
    });
    if items.len() > limit {
        items.truncate(limit);
    }
    Ok(items)
}

async fn api_artifacts(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<ArtifactQuery>,
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
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let kind_filter = query.kind.as_deref().map(|k| k.trim().to_ascii_lowercase());
    let artifacts =
        list_chat_artifacts(&state, chat_id, persona_id, kind_filter.as_deref(), limit).await?;

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": persona_id,
        "artifacts": artifacts,
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
    let cid = chat_id;
    let persona_id_queue = if let Some(pid) = body.persona_id {
        pid
    } else {
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let queue_label = body.message.chars().take(120).collect::<String>();
    let abort_hook = {
        let db = state.app_state.db.clone();
        let bot_username = state.app_state.config.bot_username.clone();
        let run_hub = state.run_hub.clone();
        let run_id_abort = run_id.clone();
        let limits = state.limits.clone();
        let base = crate::queue_abort::make_store_only_hard_abort_hook(
            db,
            bot_username,
            chat_id,
            persona_id_queue,
            run_id.clone(),
        );
        std::sync::Arc::new(move |reason: String| {
            let run_hub = run_hub.clone();
            let run_id_abort = run_id_abort.clone();
            let limits = limits.clone();
            let base = base.clone();
            let reason_for_hub = reason.clone();
            Box::pin(async move {
                base(reason).await;
                run_hub
                    .publish(
                        &run_id_abort,
                        "error",
                        serde_json::json!({ "error": reason_for_hub }).to_string(),
                        limits.run_history_limit,
                    )
                    .await;
                run_hub.remove_later(run_id_abort, 300).await;
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        })
    };
    let queue_meta = QueueEnqueueMeta {
        run_id: run_id.clone(),
        persona_id: persona_id_queue,
        source: QueueSource::Web,
        label: queue_label,
        project_id: None,
        workflow_id: None,
        on_hard_abort: Some(abort_hook),
    };
    let state_for_task = state.clone();
    let run_id_for_task = run_id.clone();
    let limits = state.limits.clone();
    let (queue_position, _) = state
        .app_state
        .chat_queue
        .enqueue_with_meta(chat_id, queue_meta, |cancel| async move {
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
                Some(cancel),
            )
            .await
            {
                Ok(resp) => {
                    let v = resp.0;
                    if let Some(jid) = v.get("background_job_id").and_then(|x| x.as_str()) {
                        if !jid.is_empty() {
                            state_for_task
                                .run_hub
                                .publish(
                                    &run_id_for_task,
                                    "background_job",
                                    json!({
                                        "job_id": jid,
                                        "message": "Task queued in background."
                                    })
                                    .to_string(),
                                    limits.run_history_limit,
                                )
                                .await;
                        }
                    }
                    let response_text = v
                        .get("response")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let mut done_payload = json!({ "response": response_text });
                    if let Some(jid) = v.get("background_job_id").and_then(|x| x.as_str()) {
                        if !jid.is_empty() {
                            done_payload["background_job_id"] = json!(jid);
                        }
                    }
                    state_for_task
                        .run_hub
                        .publish(
                            &run_id_for_task,
                            "done",
                            done_payload.to_string(),
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
    let cid = chat_id;
    let persona_id_queue = if let Some(pid) = body.persona_id {
        pid
    } else {
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let queue_label = text.chars().take(120).collect::<String>();
    let abort_hook = {
        let db = state.app_state.db.clone();
        let bot_username = state.app_state.config.bot_username.clone();
        let run_hub = state.run_hub.clone();
        let run_id_abort = run_id.clone();
        let limits = state.limits.clone();
        let base = crate::queue_abort::make_store_only_hard_abort_hook(
            db,
            bot_username,
            chat_id,
            persona_id_queue,
            run_id.clone(),
        );
        std::sync::Arc::new(move |reason: String| {
            let run_hub = run_hub.clone();
            let run_id_abort = run_id_abort.clone();
            let limits = limits.clone();
            let base = base.clone();
            let reason_for_hub = reason.clone();
            Box::pin(async move {
                base(reason).await;
                run_hub
                    .publish(
                        &run_id_abort,
                        "error",
                        serde_json::json!({ "error": reason_for_hub }).to_string(),
                        limits.run_history_limit,
                    )
                    .await;
                run_hub.remove_later(run_id_abort, 300).await;
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        })
    };
    let queue_meta = QueueEnqueueMeta {
        run_id: run_id.clone(),
        persona_id: persona_id_queue,
        source: QueueSource::Web,
        label: queue_label,
        project_id: None,
        workflow_id: None,
        on_hard_abort: Some(abort_hook),
    };
    let state_for_task = state.clone();
    let run_id_for_task = run_id.clone();
    let limits = state.limits.clone();
    let (queue_position, _) = state
        .app_state
        .chat_queue
        .enqueue_with_meta(chat_id, queue_meta, |cancel| async move {
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
                                    json!({"message": format!("iteration {iteration}")})
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
                        AgentEvent::Hook {
                            event_name,
                            tool_name,
                            matched_hook_ids,
                            blocked_reason,
                            additional_context_count,
                        } => {
                            run_hub
                                .publish(
                                    &run_id_for_events,
                                    "hook",
                                    json!({
                                        "event_name": event_name,
                                        "tool_name": tool_name,
                                        "matched_hook_ids": matched_hook_ids,
                                        "blocked_reason": blocked_reason,
                                        "additional_context_count": additional_context_count,
                                    })
                                    .to_string(),
                                    run_history_limit,
                                )
                                .await;
                        }
                        AgentEvent::FinalResponse { text } => {
                            if !text.is_empty() {
                                run_hub
                                    .publish(
                                        &run_id_for_events,
                                        "delta",
                                        json!({"delta": text}).to_string(),
                                        run_history_limit,
                                    )
                                    .await;
                            }
                        }
                    }
                }
            });

            match send_and_store_response_with_events(
                state_for_task.clone(),
                body,
                Some(&evt_tx),
                Some(&run_id_for_task),
                Some(cancel),
            )
            .await
            {
                Ok(resp) => {
                    let v = resp.0;
                    if let Some(jid) = v.get("background_job_id").and_then(|x| x.as_str()) {
                        if !jid.is_empty() {
                            state_for_task
                                .run_hub
                                .publish(
                                    &run_id_for_task,
                                    "background_job",
                                    json!({
                                        "job_id": jid,
                                        "message": "Task queued in background."
                                    })
                                    .to_string(),
                                    limits.run_history_limit,
                                )
                                .await;
                        }
                    }
                    let response_text = v
                        .get("response")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let mut done_payload = json!({ "response": response_text });
                    if let Some(jid) = v.get("background_job_id").and_then(|x| x.as_str()) {
                        if !jid.is_empty() {
                            done_payload["background_job_id"] = json!(jid);
                        }
                    }
                    state_for_task
                        .run_hub
                        .publish(
                            &run_id_for_task,
                            "done",
                            done_payload.to_string(),
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

/// Preview for ops poll / background job JSON. Byte-capped on a UTF-8 boundary.
fn background_job_result_preview(text: &str) -> &str {
    if text.len() > 200 {
        &text[..text.floor_char_boundary(200)]
    } else {
        text
    }
}

fn json_background_job(j: &crate::db::BackgroundJob) -> serde_json::Value {
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
        "result_preview": j.result_text.as_deref().map(background_job_result_preview),
        "error_text": j.error_text,
        "lease_owner": j.lease_owner,
        "lease_expires_at": j.lease_expires_at,
        "last_progress_at": j.last_progress_at,
        "last_stage": j.last_stage,
        "job_kind": j.job_kind,
        "shell_command": j.shell_command,
        "workdir": j.workdir,
        "tmux_session": j.tmux_session,
        "output_path": j.output_path,
        "exit_code": j.exit_code,
        "label": j.label,
    })
}

fn json_job_heartbeat(h: &JobHeartbeat) -> serde_json::Value {
    json!({
        "run_key": h.run_key,
        "chat_id": h.chat_id,
        "persona_id": h.persona_id,
        "job_type": h.job_type,
        "stage": h.stage,
        "message": h.message,
        "active": h.active,
        "updated_at": h.updated_at,
    })
}

async fn api_run_status(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<RunStatusQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let run_key = query.run_id.clone();
    let include_timeline_events = query.timeline_events.unwrap_or(false);
    let timeline_count = if include_timeline_events {
        call_blocking(state.app_state.db.clone(), {
            let run_key = run_key.clone();
            move |db| Ok(db.get_run_timeline_events(&run_key, 500)?.len() as i64)
        })
        .await
        .unwrap_or(0)
    } else {
        0
    };

    let hb_opt = call_blocking(state.app_state.db.clone(), {
        let run_key = run_key.clone();
        move |db| db.get_job_heartbeat(&run_key)
    })
    .await
    .ok()
    .flatten();

    let bg_json = if hb_opt.as_ref().is_some_and(|h| {
        matches!(
            h.job_type.as_str(),
            "manual_background" | "shell_background"
        )
    }) {
        call_blocking(state.app_state.db.clone(), {
            let run_key = run_key.clone();
            move |db| db.get_background_job(&run_key)
        })
        .await
        .ok()
        .flatten()
        .map(|j| json_background_job(&j))
    } else {
        None
    };

    if let Some((done, last_event_id)) = state.run_hub.status(&query.run_id).await {
        return Ok(Json(json!({
            "ok": true,
            "run_id": query.run_id,
            "done": done,
            "last_event_id": last_event_id,
            "timeline_events": timeline_count,
            "heartbeat": hb_opt.as_ref().map(json_job_heartbeat),
            "background_job": bg_json,
            "source": "run_hub",
        })));
    }

    if let Some(ref hb) = hb_opt {
        let done = !hb.active;
        return Ok(Json(json!({
            "ok": true,
            "run_id": query.run_id,
            "done": done,
            "last_event_id": 0u64,
            "timeline_events": timeline_count,
            "heartbeat": json_job_heartbeat(hb),
            "background_job": bg_json,
            "source": "database",
        })));
    }

    Err((StatusCode::NOT_FOUND, "run not found".into()))
}

async fn api_queue_diagnostics(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let lanes = state.app_state.chat_queue.diagnostics().await;
    let db = state.app_state.db.clone();
    let now = chrono::Utc::now().to_rfc3339();
    let pending_timeout_secs = state
        .app_state
        .config
        .background_job_pending_start_timeout_secs as i64;
    let mut background_by_chat: HashMap<i64, Vec<serde_json::Value>> = HashMap::new();
    let mut rows = Vec::new();
    for lane in &lanes {
        let chat_id = lane.chat_id;
        let mut items = Vec::new();
        for it in &lane.items {
            let cid = chat_id;
            let pid = it.persona_id;
            let persona_name = call_blocking(db.clone(), move |db| {
                Ok(db
                    .get_persona(pid)?
                    .filter(|p| p.chat_id == cid)
                    .map(|p| p.name)
                    .unwrap_or_else(|| format!("persona #{pid}")))
            })
            .await
            .unwrap_or_else(|_| format!("persona #{pid}"));
            items.push(json!({
                "run_id": it.run_id,
                "persona_id": it.persona_id,
                "persona_name": persona_name,
                "source": it.source,
                "label": it.label,
                "state": it.state,
                "project_id": it.project_id,
                "workflow_id": it.workflow_id,
                "position": it.position,
            }));
        }
        rows.push(json!({
            "chat_id": lane.chat_id,
            "persona_id": lane.persona_id,
            "pending": lane.pending,
            "active_for_ms": lane.active_for_ms,
            "oldest_wait_ms": lane.oldest_wait_ms,
            "last_error": lane.last_error,
            "project_id": lane.project_id,
            "workflow_id": lane.workflow_id,
            "items": items,
        }));

        let cid = lane.chat_id;
        let now_bg = now.clone();
        let timeout = pending_timeout_secs;
        if let Ok(active) = call_blocking(db.clone(), move |database| {
            database.list_active_background_jobs_for_chat(cid, &now_bg, timeout)
        })
        .await
        {
            let entries: Vec<serde_json::Value> = active.iter().map(json_background_job).collect();
            if !entries.is_empty() {
                background_by_chat.insert(lane.chat_id, entries);
            }
        }
    }
    let bg_map: serde_json::Map<String, serde_json::Value> = background_by_chat
        .into_iter()
        .map(|(k, v)| (k.to_string(), serde_json::Value::Array(v)))
        .collect();
    Ok(Json(json!({
        "ok": true,
        "lanes": rows,
        "background_by_chat": bg_map,
    })))
}

#[derive(Debug, Deserialize)]
struct OpsPollQuery {
    chat_id: Option<i64>,
    /// When false/0, skip personas list (sidebar refresh is slower than queue/background).
    include_personas: Option<String>,
    limit: Option<usize>,
}

fn ops_poll_include_personas(raw: Option<&str>) -> bool {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(s) => !matches!(
            s.to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
    }
}

/// Combined queue + background (+ optional personas) snapshot for the web ops poller.
/// Replaces three parallel GETs per tick to cut connection churn under frequent poll.
async fn api_ops_poll(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<OpsPollQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    let include_personas = ops_poll_include_personas(query.include_personas.as_deref());
    if include_personas {
        ensure_web_binding_for_universal(&state, chat_id).await?;
    }

    let all_lanes = state.app_state.chat_queue.diagnostics().await;
    let db = state.app_state.db.clone();
    let mut lane_rows = Vec::new();
    for lane in all_lanes.into_iter().filter(|l| l.chat_id == chat_id) {
        let mut items = Vec::new();
        for it in &lane.items {
            let cid = chat_id;
            let pid = it.persona_id;
            let persona_name = call_blocking(db.clone(), move |database| {
                Ok(database
                    .get_persona(pid)?
                    .filter(|p| p.chat_id == cid)
                    .map(|p| p.name)
                    .unwrap_or_else(|| format!("persona #{pid}")))
            })
            .await
            .unwrap_or_else(|_| format!("persona #{pid}"));
            items.push(json!({
                "run_id": it.run_id,
                "persona_id": it.persona_id,
                "persona_name": persona_name,
                "source": it.source,
                "label": it.label,
                "state": it.state,
                "project_id": it.project_id,
                "workflow_id": it.workflow_id,
                "position": it.position,
            }));
        }
        lane_rows.push(json!({
            "chat_id": lane.chat_id,
            "persona_id": lane.persona_id,
            "pending": lane.pending,
            "active_for_ms": lane.active_for_ms,
            "oldest_wait_ms": lane.oldest_wait_ms,
            "last_error": lane.last_error,
            "project_id": lane.project_id,
            "workflow_id": lane.workflow_id,
            "items": items,
        }));
    }

    let limit = query.limit.unwrap_or(20).min(100);
    let jobs = call_blocking(state.app_state.db.clone(), move |database| {
        database.list_background_jobs_for_chat(chat_id, limit)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let heartbeats = call_blocking(state.app_state.db.clone(), move |database| {
        database.list_job_heartbeats_for_chat(chat_id, 200)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let hb_by_key: HashMap<String, JobHeartbeat> = heartbeats
        .into_iter()
        .map(|h| (h.run_key.clone(), h))
        .collect();
    let now = chrono::Utc::now().to_rfc3339();
    let pending_timeout_secs = state
        .app_state
        .config
        .background_job_pending_start_timeout_secs as i64;
    let active_count = call_blocking(state.app_state.db.clone(), move |database| {
        database.count_active_background_jobs_for_chat(chat_id, &now, pending_timeout_secs)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let job_items: Vec<serde_json::Value> = jobs
        .into_iter()
        .map(|j| {
            let hb = hb_by_key.get(&j.id);
            let mut row = json_background_job(&j);
            if let Some(obj) = row.as_object_mut() {
                obj.insert(
                    "heartbeat".into(),
                    hb.map(json_job_heartbeat)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            row
        })
        .collect();

    let personas = if include_personas {
        let cid = chat_id;
        let persona_rows: Vec<Persona> =
            call_blocking(state.app_state.db.clone(), move |database| {
                database.list_personas(cid)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let cid2 = chat_id;
        let active_id = call_blocking(state.app_state.db.clone(), move |database| {
            database.get_active_persona_id(cid2)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let cid3 = chat_id;
        let last_bot_rows = call_blocking(state.app_state.db.clone(), move |database| {
            database.list_persona_last_bot_message_at(cid3)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let last_bot_by_persona: HashMap<i64, crate::db::PersonaLastBotInfo> = last_bot_rows
            .into_iter()
            .map(|row| (row.persona_id, row))
            .collect();
        Some(
            persona_rows
                .iter()
                .map(|p| {
                    let last = last_bot_by_persona.get(&p.id);
                    json!({
                        "id": p.id,
                        "name": p.name,
                        "is_active": active_id == Some(p.id),
                        "last_bot_message_at": last.map(|r| r.last_bot_message_at.clone()),
                        "last_bot_message_session_id": last.and_then(|r| r.session_id.clone()),
                        "last_bot_message_session_title": last.and_then(|r| r.session_title.clone()),
                    })
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "lanes": lane_rows,
        "jobs": job_items,
        "active_count": active_count,
        "personas_included": include_personas,
        "personas": personas.unwrap_or_default(),
    })))
}

#[derive(Deserialize)]
struct QueueCancelRequest {
    run_id: String,
    chat_id: Option<i64>,
}

async fn api_queue_cancel(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<QueueCancelRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let run_id = body.run_id.trim();
    if run_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "run_id is required".into()));
    }
    info!(chat_id, run_id, "web queue cancel requested");
    let ok = state
        .app_state
        .chat_queue
        .request_cancel(run_id, chat_id)
        .await;
    if !ok {
        warn!(chat_id, run_id, "web queue cancel target not found");
        return Err((StatusCode::NOT_FOUND, "run not found for this chat".into()));
    }
    info!(chat_id, run_id, "web queue cancel accepted");
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct QueueRemoveRequest {
    run_id: String,
    chat_id: Option<i64>,
}

async fn api_queue_remove(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<QueueRemoveRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let run_id = body.run_id.trim();
    if run_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "run_id is required".into()));
    }
    info!(chat_id, run_id, "web queue remove requested");
    let outcome = state
        .app_state
        .chat_queue
        .request_remove_queued(run_id, chat_id)
        .await;
    match outcome {
        QueueRemoveOutcome::Removed => {
            info!(chat_id, run_id, "web queue remove accepted");
            Ok(Json(json!({ "ok": true })))
        }
        QueueRemoveOutcome::Running => {
            warn!(
                chat_id,
                run_id, "web queue remove rejected because run is running"
            );
            Err((
                StatusCode::CONFLICT,
                "run is currently running; use Stop instead".into(),
            ))
        }
        QueueRemoveOutcome::NotFound => {
            warn!(chat_id, run_id, "web queue remove target not found");
            Err((StatusCode::NOT_FOUND, "run not found for this chat".into()))
        }
    }
}

fn is_background_job_active_status(status: &str) -> bool {
    matches!(
        status,
        "pending" | "running" | "completed_raw" | "main_agent_processing"
    )
}

#[derive(Deserialize)]
struct BackgroundJobCancelRequest {
    job_id: String,
    chat_id: Option<i64>,
}

async fn api_background_job_cancel(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<BackgroundJobCancelRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let job_id = body.job_id.trim().to_string();
    if job_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "job_id is required".into()));
    }

    let job = call_blocking(state.app_state.db.clone(), {
        let job_id = job_id.clone();
        move |db| db.get_background_job(&job_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some(job) = job else {
        return Err((StatusCode::NOT_FOUND, "background job not found".into()));
    };
    if job.chat_id != chat_id {
        return Err((StatusCode::NOT_FOUND, "background job not found".into()));
    }
    if !is_background_job_active_status(job.status.as_str()) {
        return Err((
            StatusCode::CONFLICT,
            format!("background job is not active (status: {})", job.status),
        ));
    }

    info!(chat_id, job_id, "web background job cancel requested");

    if job.job_kind == "shell" {
        if let Err(e) = crate::background_shell::cancel_background_shell_job(
            &state.app_state,
            &job,
            "Cancelled by user",
        )
        .await
        {
            return Err((StatusCode::CONFLICT, e));
        }
        return Ok(Json(json!({ "ok": true })));
    }

    let cancelled = state
        .app_state
        .background_job_control
        .request_cancel(&job_id, chat_id)
        .await;
    if !cancelled {
        warn!(
            chat_id,
            job_id, "web background job cancel target not found in runtime registry"
        );
        return Err((
            StatusCode::CONFLICT,
            "background job is no longer cancellable".into(),
        ));
    }

    Ok(Json(json!({ "ok": true })))
}

async fn send_and_store_response_with_events(
    state: WebState,
    body: SendRequest,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<AgentEvent>>,
    run_key: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let raw_text = body.message.trim().to_string();
    let mut text = raw_text.clone();
    let mut image_data: Option<(String, String)> = None;

    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    let persona_id = if let Some(pid) = body.persona_id {
        pid
    } else {
        let cid = chat_id;
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let attachment_notes = process_web_attachments(
        &state,
        chat_id,
        persona_id,
        &body.attachments,
        &mut image_data,
    )
    .await?;
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
                let cid_sched = chat_id;
                let pid_sched = persona_id;
                let tasks = if pid_sched > 0 {
                    call_blocking(state.app_state.db.clone(), move |db| {
                        db.get_scheduled_tasks_for_chat_and_persona(cid_sched, pid_sched)
                    })
                    .await
                } else {
                    call_blocking(state.app_state.db.clone(), move |db| {
                        db.get_scheduled_tasks_for_chat_for_display(cid_sched)
                    })
                    .await
                };
                match &tasks {
                    Ok(t) => crate::tools::schedule::format_tasks_list(t),
                    Err(e) => format!("Error listing tasks: {e}"),
                }
            }
            SlashCommand::Archive => {
                let cid2 = chat_id;
                let pid = persona_id;
                let history = call_blocking(state.app_state.db.clone(), move |db| {
                    db.get_recent_messages(cid2, pid, 500, false)
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
            state.app_state.telegram_bots.as_ref(),
            state.app_state.discord_http.as_ref(),
            state.app_state.wecom.as_deref(),
            &state.app_state.config.bot_username,
            chat_id,
            persona_id,
            &resp,
            Some(state.app_state.config.workspace_root_absolute()),
            DeliveryScope::StoreOnly,
            None,
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

    let full_prompt_for_handoff = text.clone();
    let user_msg = StoredMessage {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id,
        persona_id,
        session_id: body.session_id.clone(),
        sender_name: sender_name.clone(),
        content: text,
        is_from_bot: false,
        timestamp: chrono::Utc::now().to_rfc3339(),
        origin: crate::db::message_origin_interactive(),
    };
    call_blocking(state.app_state.db.clone(), move |db| {
        db.store_message(&user_msg)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let agent_out = process_with_agent_with_events(
        &state.app_state,
        AgentRequestContext {
            caller_channel: "web",
            chat_id,
            chat_type: "private",
            persona_id,
            is_scheduled_task: false,
            is_background_job: false,
            run_key: run_key.map(|s| s.to_string()),
            reply_bot_instance_id: None,
            session_id: body.session_id.clone(),
        },
        None,
        image_data,
        event_tx,
        cancel,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut response = agent_out.response;

    let mut background_job_id: Option<String> = None;
    let mut background_job_queued = false;

    if is_background_handoff_response(&response) {
        let trig = handoff_trigger_for_db(&response).unwrap_or("timeout");
        match try_enqueue_background_handoff(
            state.app_state.clone(),
            chat_id,
            persona_id,
            full_prompt_for_handoff,
            trig,
            "web",
        )
        .await
        {
            HandoffEnqueueOutcome::Queued { job_id, start_ack } => {
                background_job_id = Some(job_id.clone());
                background_job_queued = true;
                response = await_handoff_startup_ack(start_ack).await;
            }
            HandoffEnqueueOutcome::BlockedAlreadyRunning => {
                response = "A background task is already running for this chat. Please wait for it to finish before starting another long-running background task.".into();
            }
            HandoffEnqueueOutcome::ActiveLookupFailed(msg) => {
                response = format!("Failed to check active background jobs: {msg}");
            }
            HandoffEnqueueOutcome::DbCreateFailed(e) => {
                response = format!("Failed to queue background task: {e}");
            }
        }
    } else {
        let delivery = deliver_agent_final_to_contact(
            state.app_state.db.clone(),
            state.app_state.telegram_bots.as_ref(),
            state.app_state.discord_http.as_ref(),
            state.app_state.wecom.as_deref(),
            &state.app_state.config.bot_username,
            chat_id,
            persona_id,
            &response,
            Some(state.app_state.config.workspace_root_absolute()),
            DeliveryScope::StoreOnly,
            body.session_id.clone(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        response = delivery.response_for_client;
    }

    let mut out = json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": persona_id,
        "prompt": raw_text,
        "response": response,
        "background_job_queued": background_job_queued,
    });
    if let Some(id) = background_job_id {
        out["background_job_id"] = json!(id);
    } else {
        out["background_job_id"] = serde_json::Value::Null;
    }
    Ok(Json(out))
}

#[cfg(test)]
fn upload_rel_url_exists(state: &WebState, rel_url: &str) -> bool {
    let Some(rel) = rel_url.strip_prefix("/api/uploads/") else {
        return true;
    };
    let shared_path = state
        .app_state
        .config
        .workspace_root_absolute()
        .join("shared")
        .join("upload")
        .join(rel);
    if shared_path.is_file() {
        return true;
    }
    let legacy_path = FsPath::new(state.app_state.config.working_dir())
        .join("uploads")
        .join(rel);
    legacy_path.is_file()
}

#[cfg(test)]
async fn materialize_response_file_links(
    state: &WebState,
    chat_id: i64,
    persona_id: i64,
    response: &str,
) -> Result<String, (StatusCode, String)> {
    crate::final_delivery_media::materialize_web_delivery_file_links(
        &state.app_state.config.workspace_root_absolute(),
        Some(state.app_state.config.working_dir()),
        chat_id,
        persona_id,
        response,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn web_max_document_bytes(config: &Config) -> u64 {
    config
        .max_document_size_mb
        .saturating_mul(1024)
        .saturating_mul(1024)
}

/// Axum default is 2 MB; allow inline JSON base64 payloads up to ~1.4× file limit + headroom.
fn web_json_body_limit_bytes(config: &Config) -> usize {
    let doc = web_max_document_bytes(config) as f64;
    (doc * 1.45).ceil() as usize + 65_536
}

fn web_multipart_body_limit_bytes(config: &Config) -> usize {
    web_max_document_bytes(config) as usize + 2 * 1024 * 1024
}

fn web_upload_dir(state: &WebState, chat_id: i64) -> PathBuf {
    state
        .app_state
        .config
        .workspace_root_absolute()
        .join("shared")
        .join("upload")
        .join("web")
        .join(chat_id.to_string())
}

fn web_upload_dir_for_persona(state: &WebState, chat_id: i64, persona_id: i64) -> PathBuf {
    web_upload_dir(state, chat_id).join(persona_id.to_string())
}

fn resolve_upload_tool_path_on_disk(state: &WebState, tool_path: &str) -> Option<PathBuf> {
    let clean = tool_path.trim().trim_start_matches('/');
    if clean.is_empty() || clean.contains("..") {
        return None;
    }
    let full = state
        .app_state
        .config
        .workspace_root_absolute()
        .join("shared")
        .join(clean);
    if full.is_file() {
        Some(full)
    } else {
        None
    }
}

fn fast_upload_filename(safe_name: &str) -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    format!("{ts}-{safe_name}")
}

fn attachment_notes_for_saved(
    filename: &str,
    bytes_len: u64,
    mime: &str,
    tool_path: &str,
    saved_path: &FsPath,
    rel_url: &str,
) -> Vec<String> {
    let mut notes = vec![format!(
        "[document] filename={} bytes={} mime={} tool_path={} saved_path={} url={}",
        filename,
        bytes_len,
        mime,
        tool_path,
        saved_path.display(),
        rel_url
    )];
    if mime.starts_with("image/") {
        let alt = filename.replace(']', "_");
        notes.push(format!("![{alt}]({rel_url})"));
    } else {
        notes.push(format!("[{filename}]({rel_url})"));
    }
    notes
}

async fn set_image_data_from_bytes(
    image_data: &mut Option<(String, String)>,
    bytes: &[u8],
    mime: &str,
) {
    if image_data.is_none() && mime.starts_with("image/") {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        *image_data = Some((b64, mime.to_string()));
    }
}

async fn set_image_data_from_path(
    image_data: &mut Option<(String, String)>,
    path: &FsPath,
    mime: &str,
) -> Result<(), (StatusCode, String)> {
    if image_data.is_some() || !mime.starts_with("image/") {
        return Ok(());
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    set_image_data_from_bytes(image_data, &bytes, mime).await;
    Ok(())
}

async fn process_web_attachments(
    state: &WebState,
    chat_id: i64,
    persona_id: i64,
    attachments: &[SendAttachmentRequest],
    image_data: &mut Option<(String, String)>,
) -> Result<Vec<String>, (StatusCode, String)> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }

    let max_bytes = web_max_document_bytes(&state.app_state.config);
    let dir = web_upload_dir_for_persona(state, chat_id, persona_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut notes = Vec::new();
    for (idx, att) in attachments.iter().enumerate() {
        let mime = att
            .media_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let filename = att
            .filename
            .clone()
            .unwrap_or_else(|| format!("web-attachment-{}.bin", idx + 1));

        if let Some(tool_path) = att.tool_path.as_deref().filter(|s| !s.trim().is_empty()) {
            let expected_prefix = format!("upload/web/{chat_id}/{persona_id}/");
            if !tool_path.starts_with(&expected_prefix) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("attachment path must start with {expected_prefix}"),
                ));
            }
            let disk_path =
                resolve_upload_tool_path_on_disk(state, tool_path).ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("attachment not found: {tool_path}"),
                    )
                })?;
            let meta = tokio::fs::metadata(&disk_path)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let bytes_len = meta.len();
            if bytes_len > max_bytes {
                notes.push(format!(
                    "[document] filename={} bytes={} mime={} skipped=too_large",
                    filename, bytes_len, mime
                ));
                continue;
            }
            let rel_url = att.url.clone().unwrap_or_else(|| {
                let saved_file = disk_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file");
                format!("/api/uploads/web/{chat_id}/{persona_id}/{saved_file}")
            });
            set_image_data_from_path(image_data, &disk_path, &mime).await?;
            notes.extend(attachment_notes_for_saved(
                &filename, bytes_len, &mime, tool_path, &disk_path, &rel_url,
            ));
            continue;
        }

        let b64 = att.data_base64.as_deref().filter(|s| !s.trim().is_empty());
        let Some(b64) = b64 else {
            return Err((
                StatusCode::BAD_REQUEST,
                "attachment must include tool_path or data_base64".into(),
            ));
        };

        let bytes = decode_base64_payload(b64).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("invalid attachment base64: {e}"),
            )
        })?;

        if (bytes.len() as u64) > max_bytes {
            notes.push(format!(
                "[document] filename={} bytes={} mime={} skipped=too_large",
                filename,
                bytes.len(),
                mime
            ));
            continue;
        }

        let safe_name = sanitize_upload_filename(&filename);
        let stable_name = stable_upload_filename(&safe_name, &bytes, idx + 1);
        let path = dir.join(stable_name);
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let saved_file = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tool_path = format!("upload/web/{}/{}/{}", chat_id, persona_id, saved_file);
        let rel_url = format!("/api/uploads/web/{chat_id}/{persona_id}/{saved_file}");

        set_image_data_from_bytes(image_data, &bytes, &mime).await;

        notes.extend(attachment_notes_for_saved(
            &filename,
            bytes.len() as u64,
            &mime,
            &tool_path,
            &path,
            &rel_url,
        ));
    }

    Ok(notes)
}

async fn api_upload(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<UploadQueryParams>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    let persona_id = if let Some(pid) = query.persona_id {
        pid
    } else {
        let cid = chat_id;
        call_blocking(state.app_state.db.clone(), move |db| {
            db.get_current_persona_id(cid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let max_bytes = web_max_document_bytes(&state.app_state.config);
    let dir = web_upload_dir_for_persona(&state, chat_id, persona_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut uploaded: Option<UploadResponse> = None;

    while let Some(mut field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid multipart upload: {e}"),
        )
    })? {
        if field.name() != Some("file") {
            continue;
        }
        let original_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "web-upload.bin".to_string());
        let mime = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let safe_name = sanitize_upload_filename(&original_name);
        let disk_name = fast_upload_filename(&safe_name);
        let path = dir.join(&disk_name);
        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let mut total: u64 = 0;
        while let Some(chunk) = field.chunk().await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("failed to read upload chunk: {e}"),
            )
        })? {
            total = total.saturating_add(chunk.len() as u64);
            if total > max_bytes {
                let _ = tokio::fs::remove_file(&path).await;
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!(
                        "file exceeds maximum size of {} MB (MAX_DOCUMENT_SIZE_MB)",
                        state.app_state.config.max_document_size_mb
                    ),
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        file.flush()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let saved_file = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tool_path = format!("upload/web/{}/{}/{}", chat_id, persona_id, saved_file);
        let rel_url = format!("/api/uploads/web/{chat_id}/{persona_id}/{saved_file}");
        uploaded = Some(UploadResponse {
            filename: original_name,
            media_type: mime,
            bytes: total,
            tool_path,
            url: rel_url,
        });
        break;
    }

    uploaded.map(Json).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "missing file field in upload".into(),
        )
    })
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

fn stable_upload_filename(safe_name: &str, bytes: &[u8], fallback_index: usize) -> String {
    let final_name = if safe_name.is_empty() {
        format!("web-attachment-{}.bin", fallback_index)
    } else {
        safe_name.to_string()
    };
    let digest = Sha256::digest(bytes);
    let hash = digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("{hash}-{final_name}")
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
    let last_bot_by_persona: HashMap<i64, crate::db::PersonaLastBotInfo> = last_bot_rows
        .into_iter()
        .map(|row| (row.persona_id, row))
        .collect();

    let global_engine = state.app_state.runtime_toggles.agent_engine();
    let items: Vec<serde_json::Value> = personas
        .iter()
        .map(|p| {
            let last = last_bot_by_persona.get(&p.id);
            let effective = crate::runtime_toggles::resolve_run_agent_engine(
                p.agent_engine_override.as_deref(),
                global_engine,
            );
            json!({
                "id": p.id,
                "name": p.name,
                "model_override": p.model_override,
                "is_active": active_id == Some(p.id),
                "last_bot_message_at": last.map(|r| r.last_bot_message_at.clone()),
                "last_bot_message_session_id": last.and_then(|r| r.session_id.clone()),
                "last_bot_message_session_title": last.and_then(|r| r.session_title.clone()),
                "agent_engine_override": p.agent_engine_override,
                "agent_engine_effective": effective.as_str(),
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

#[derive(Debug, Deserialize, Default)]
struct AgentHistoryOptimizeRequest {
    #[serde(default)]
    operator_notes: Option<String>,
}

#[derive(Deserialize)]
struct PersonaBookmarkPathParams {
    persona_id: i64,
}

#[derive(Deserialize)]
struct PersonaBookmarkDeletePathParams {
    persona_id: i64,
    message_id: String,
}

#[derive(Deserialize)]
struct PersonaMessagePathParams {
    persona_id: i64,
    message_id: String,
}

#[derive(Deserialize)]
struct PersonaPolicyPathParams {
    persona_id: i64,
}

#[derive(Deserialize)]
struct HookDefinitionUpsertBody {
    id: Option<i64>,
    name: String,
    event_name: String,
    matcher: Option<String>,
    action_type: String,
    action_payload_json: Option<String>,
    enabled: Option<bool>,
    #[serde(default)]
    scoped_persona_ids: Option<Vec<i64>>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    creating_persona_id: Option<i64>,
}

#[derive(Deserialize)]
struct HookDefinitionDeletePathParams {
    id: i64,
}

#[derive(Deserialize)]
struct SkillsQuery {
    persona_id: Option<i64>,
}

#[derive(Deserialize)]
struct HooksQuery {
    persona_id: Option<i64>,
}

/// PATCH tri-state: absent = unchanged, JSON `null` = clear, value = set.
/// Plain `Option<Option<T>>` treats JSON `null` as absent; this wrapper maps it to `Some(None)`.
fn deserialize_patch_field<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

#[derive(Debug, Deserialize, Default)]
struct PersonaPolicyPatchBody {
    /// `null` => default allow-all. `[]` => explicit allow-none.
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    allowed_hook_ids: Option<Option<Vec<i64>>>,
    /// `null` => default allow-all. `[]` => explicit allow-none.
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    allowed_skill_names: Option<Option<Vec<String>>>,
}

#[derive(Deserialize)]
struct PersonaBookmarkUpsertBody {
    message_id: String,
    note: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PersonaBulletinPatchBody {
    /// `null` clears override (server defaults). Omitted = leave unchanged.
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    recent_history_min_user: Option<Option<i64>>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    recent_history_min_assistant: Option<Option<i64>>,
    /// `null` clears memo. Omitted = leave unchanged.
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    operator_memo: Option<Option<String>>,
    #[serde(default)]
    dense_delivery_enabled: Option<bool>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    dense_delivery_messaging_max_chars: Option<Option<i64>>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    dense_delivery_web_max_chars: Option<Option<i64>>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    dense_delivery_summary_chars: Option<Option<i64>>,
    /// `null`/empty clears override (inherit global). Omitted = leave unchanged.
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_patch_field")]
    agent_engine_override: Option<Option<String>>,
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() > max_chars {
        format!("{}...", input.chars().take(max_chars).collect::<String>())
    } else {
        input.to_string()
    }
}

fn normalize_persona_scope_ids(ids: &[i64]) -> Vec<i64> {
    let mut out: Vec<i64> = ids.iter().copied().filter(|id| *id > 0).collect();
    out.sort_unstable();
    out.dedup();
    out
}

// --- Chat Sessions ---

#[derive(Debug, Deserialize)]
struct ChatSessionsListQuery {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    /// Deprecated: session archive is gone; the list always includes every session.
    #[serde(default)]
    include_archived: Option<bool>,
}

fn chat_session_json(session: &crate::db::ChatSession) -> serde_json::Value {
    serde_json::json!({
        "id": session.id,
        "chat_id": session.chat_id,
        "persona_id": session.persona_id,
        "title": session.title,
        "intent": session.intent,
        "status": session.status,
        "created_at": session.created_at,
        "last_active_at": session.last_active_at,
        "archived_at": session.archived_at,
        "ttl_hours": session.ttl_hours,
        "mirror_main_chat": session.mirror_main_chat,
    })
}

async fn api_chat_sessions_list(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<ChatSessionsListQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
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
    let _ = query.include_archived;
    let sessions = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_chat_sessions(chat_id, persona_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = sessions.iter().map(chat_session_json).collect();
    Ok(Json(serde_json::json!({ "sessions": items })))
}

#[derive(Debug, Deserialize)]
struct CreateChatSessionRequest {
    chat_id: Option<i64>,
    persona_id: Option<i64>,
    intent: String,
    #[serde(default)]
    ttl_hours: Option<i64>,
    #[serde(default)]
    mirror_main_chat: Option<bool>,
}

async fn bootstrap_chat_session_context(
    app_state: Arc<AppState>,
    session_id: String,
    chat_id: i64,
    persona_id: i64,
    intent: String,
) {
    let bootstrap_prompt = format!(
        concat!(
            "You are bootstrapping a new focused session. The user's intent is:\n\n",
            "\"{}\"\n\n",
            "Search the vault (using search_vault or mempalace search) for notes relevant to this intent. ",
            "Also list which skills from the workspace are relevant.\n\n",
            "Return ONLY a compact JSON object (no markdown fences) with this structure:\n",
            "{{\"relevant_notes\": [\"note title or path\", ...], ",
            "\"selected_skills\": [\"skill_name\", ...], ",
            "\"key_context\": \"2-3 sentence summary of relevant background knowledge\"}}\n\n",
            "If nothing relevant is found, return the JSON with empty arrays and a brief note in key_context. ",
            "Do NOT ask the user anything. Do NOT include explanation outside the JSON."
        ),
        intent
    );

    let bootstrap_result = process_with_agent(
        &app_state,
        AgentRequestContext {
            caller_channel: "web",
            chat_id,
            chat_type: "private",
            persona_id,
            is_scheduled_task: false,
            is_background_job: false,
            run_key: None,
            reply_bot_instance_id: None,
            session_id: Some(session_id.clone()),
        },
        Some(&bootstrap_prompt),
        None,
    )
    .await;

    let Ok(response) = bootstrap_result else {
        warn!(
            session_id = %session_id,
            "chat session bootstrap agent run failed"
        );
        return;
    };

    let trimmed = response.trim().to_string();
    let ctx_sid = session_id.clone();
    let ctx_json = trimmed.clone();
    if let Err(e) = call_blocking(app_state.db.clone(), move |db| {
        db.update_chat_session_bootstrap_context(&ctx_sid, &ctx_json)
    })
    .await
    {
        warn!(
            session_id = %session_id,
            error = %e,
            "failed to persist chat session bootstrap context"
        );
    }

    let bot_msg = StoredMessage {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id,
        persona_id,
        session_id: Some(session_id),
        sender_name: app_state.config.bot_username.clone(),
        content: format!("Session ready. Context loaded for: {}", intent),
        is_from_bot: true,
        timestamp: chrono::Utc::now().to_rfc3339(),
        origin: crate::db::message_origin_interactive(),
    };
    if let Err(e) = call_blocking(app_state.db.clone(), move |db| db.store_message(&bot_msg)).await
    {
        warn!(error = %e, "failed to store chat session bootstrap message");
    }
}

async fn api_chat_sessions_create(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<CreateChatSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
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

    let intent = body.intent.trim().to_string();
    if intent.is_empty() || intent.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            "intent must be 1-500 characters".into(),
        ));
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let title = intent.chars().take(60).collect::<String>();
    // ttl_hours is accepted for compatibility; session TTL auto-archive is deprecated.
    let ttl_hours = body.ttl_hours.unwrap_or(0).max(0);
    let mirror_main_chat = body.mirror_main_chat.unwrap_or(false);

    let sid = session_id.clone();
    let t = title.clone();
    let i = intent.clone();
    call_blocking(state.app_state.db.clone(), move |db| {
        db.create_chat_session(
            &sid,
            chat_id,
            persona_id,
            &t,
            &i,
            ttl_hours,
            mirror_main_chat,
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let created = call_blocking(state.app_state.db.clone(), {
        let session_id = session_id.clone();
        move |db| db.get_chat_session(&session_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "session not found".into(),
    ))?;

    let app_state = state.app_state.clone();
    let bootstrap_sid = session_id.clone();
    let bootstrap_intent = intent.clone();
    tokio::spawn(async move {
        bootstrap_chat_session_context(
            app_state,
            bootstrap_sid,
            chat_id,
            persona_id,
            bootstrap_intent,
        )
        .await;
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "session": chat_session_json(&created),
        "session_id": session_id,
        "bootstrap_pending": true,
    })))
}

async fn api_chat_sessions_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let session = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_chat_session(&session_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match session {
        Some(s) => {
            let mut value = chat_session_json(&s);
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "bootstrap_context_json".into(),
                    serde_json::json!(s.bootstrap_context_json),
                );
            }
            Ok(Json(value))
        }
        None => Err((StatusCode::NOT_FOUND, "session not found".into())),
    }
}

#[derive(Debug, Deserialize)]
struct PatchChatSessionRequest {
    title: Option<String>,
    status: Option<String>,
    ttl_hours: Option<i64>,
    mirror_main_chat: Option<bool>,
}

async fn api_chat_sessions_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(session_id): Path<String>,
    Json(body): Json<PatchChatSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    if let Some(ref status) = body.status {
        match status.as_str() {
            "archived" => {
                return Err((
                    StatusCode::GONE,
                    "session archive is deprecated; delete the session instead".into(),
                ));
            }
            "active" => {
                // no-op: archive/TTL restore is deprecated; sessions stay active
            }
            _ => {
                return Err((StatusCode::BAD_REQUEST, "status must be 'active'".into()));
            }
        }
    }

    if let Some(ref title) = body.title {
        let sid = session_id.clone();
        let t = title.clone();
        call_blocking(state.app_state.db.clone(), move |db| {
            db.update_chat_session_title(&sid, &t)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    if let Some(ttl) = body.ttl_hours {
        let sid = session_id.clone();
        let ttl_val = ttl.max(0);
        call_blocking(state.app_state.db.clone(), move |db| {
            db.update_chat_session_ttl(&sid, ttl_val)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    if let Some(mirror_main_chat) = body.mirror_main_chat {
        let sid = session_id.clone();
        call_blocking(state.app_state.db.clone(), move |db| {
            db.update_chat_session_mirror_main_chat(&sid, mirror_main_chat)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn api_chat_sessions_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_chat_session(&session_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted {
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err((StatusCode::NOT_FOUND, "session not found".into()))
    }
}

// --- end Chat Sessions ---

async fn api_skills_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<SkillsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let persona_skill_policy = if let Some(pid) = query.persona_id {
        let exists = call_blocking(state.app_state.db.clone(), move |db| {
            db.persona_exists(chat_id, pid)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !exists {
            return Err((StatusCode::NOT_FOUND, "persona not found".into()));
        }
        Some(
            call_blocking(state.app_state.db.clone(), move |db| {
                db.get_persona_hook_skill_policy(chat_id, pid)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        )
    } else {
        None
    };

    let skill_manager = &state.app_state.skills;
    let all_skills = skill_manager.discover_all_skills();
    let mut remote_count = 0usize;
    let rows: Vec<serde_json::Value> = all_skills
        .into_iter()
        .map(|s| {
            let remote = skill_manager.skill_is_remote(&s);
            if remote {
                remote_count += 1;
            }
            let allowed_for_persona = persona_skill_policy.as_ref().map(|policy| {
                let Some(row) = policy else {
                    return true;
                };
                let Some(allowed) = row.allowed_skill_names.as_ref() else {
                    return true;
                };
                allowed.iter().any(|a| a.eq_ignore_ascii_case(&s.name))
            });
            json!({
                "name": s.name,
                "description": s.description,
                "when_to_use": s.when_to_use,
                "platforms": s.platforms,
                "deps": s.deps,
                "source": s.source,
                "version": s.version,
                "updated_at": s.updated_at,
                "remote": remote,
                "allowed_for_persona": allowed_for_persona,
            })
        })
        .collect();
    Ok(Json(json!({
        "ok": true,
        "skills": rows,
        "total": rows.len(),
        "remote_count": remote_count,
    })))
}

fn hook_definition_to_json(
    h: crate::db::HookDefinitionRecord,
    persona_status: Option<(bool, bool, bool)>,
) -> serde_json::Value {
    let payload = serde_json::from_str::<serde_json::Value>(&h.action_payload_json)
        .unwrap_or_else(|_| json!({}));
    let mut row = json!({
        "id": h.id,
        "name": h.name,
        "event_name": h.event_name,
        "matcher": h.matcher,
        "action_type": h.action_type,
        "action_payload_json": h.action_payload_json,
        "action_payload": payload,
        "scoped_persona_ids": h.scoped_persona_ids,
        "is_global": h.scoped_persona_ids.is_none(),
        "enabled": h.enabled,
        "updated_at": h.updated_at,
    });
    if let Some((scoped_for_persona, allowed_for_persona, active_for_persona)) = persona_status {
        if let Some(obj) = row.as_object_mut() {
            obj.insert("scoped_for_persona".into(), json!(scoped_for_persona));
            obj.insert("allowed_for_persona".into(), json!(allowed_for_persona));
            obj.insert("active_for_persona".into(), json!(active_for_persona));
        }
    }
    row
}

async fn api_hooks_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<HooksQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let persona_id = query.persona_id;
    let chat_id = if persona_id.is_some() {
        let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
        ensure_web_binding_for_universal(&state, chat_id).await?;
        if let Some(pid) = persona_id {
            let exists = call_blocking(state.app_state.db.clone(), move |db| {
                db.persona_exists(chat_id, pid)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if !exists {
                return Err((StatusCode::NOT_FOUND, "persona not found".into()));
            }
        }
        Some(chat_id)
    } else {
        None
    };

    let db = state.app_state.db.clone();
    let hooks_json = call_blocking(db, move |db| {
        let hooks = db.list_hook_definitions()?;
        let rows = if let (Some(chat_id), Some(pid)) = (chat_id, persona_id) {
            hooks
                .into_iter()
                .map(|h| {
                    let status = h.persona_status(db, chat_id, pid)?;
                    Ok(hook_definition_to_json(h, Some(status)))
                })
                .collect::<Result<Vec<_>, crate::error::FinallyAValueBotError>>()?
        } else {
            hooks
                .into_iter()
                .map(|h| hook_definition_to_json(h, None))
                .collect()
        };
        Ok::<Vec<serde_json::Value>, crate::error::FinallyAValueBotError>(rows)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(json!({ "ok": true, "hooks": hooks_json })))
}

async fn api_hooks_post(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<HookDefinitionUpsertBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let payload_json = body
        .action_payload_json
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("{}")
        .to_string();
    if body.action_type.trim().eq_ignore_ascii_case("command") {
        validate_command_payload(&state.app_state.config, &payload_json)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    }
    let enabled = body.enabled.unwrap_or(true);
    let scope_raw = body.scope.as_deref().map(str::trim).unwrap_or_default();
    let scope_normalized = if scope_raw.is_empty() {
        None
    } else if scope_raw.eq_ignore_ascii_case("global") {
        Some("global")
    } else if scope_raw.eq_ignore_ascii_case("persona") {
        Some("persona")
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "scope must be 'global' or 'persona' when provided".to_string(),
        ));
    };
    let scoped_from_body = body
        .scoped_persona_ids
        .as_ref()
        .map(|ids| normalize_persona_scope_ids(ids));
    let scoped_persona_ids: Option<Vec<i64>> = if let Some(scope) = scope_normalized {
        match scope {
            "global" => None,
            "persona" => match scoped_from_body {
                Some(ids) => Some(ids),
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "scope=persona requires scoped_persona_ids".to_string(),
                    ))
                }
            },
            _ => None,
        }
    } else if body.id.is_some() {
        if let Some(ids) = scoped_from_body {
            Some(ids)
        } else {
            let existing_id = body.id.unwrap_or_default();
            let existing = call_blocking(state.app_state.db.clone(), move |db| {
                db.get_hook_definition(existing_id)
            })
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            let Some(existing) = existing else {
                return Err((StatusCode::NOT_FOUND, "hook not found".to_string()));
            };
            existing.scoped_persona_ids
        }
    } else if let Some(ids) = scoped_from_body {
        Some(ids)
    } else if let Some(creating_persona_id) = body.creating_persona_id {
        if creating_persona_id <= 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                "creating_persona_id must be positive".to_string(),
            ));
        }
        let exists = call_blocking(state.app_state.db.clone(), move |db| {
            db.persona_exists(chat_id, creating_persona_id)
        })
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        if !exists {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("creating_persona_id {} not found", creating_persona_id),
            ));
        }
        Some(vec![creating_persona_id])
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "persona scope required: provide scoped_persona_ids or creating_persona_id, or set scope=global"
                .to_string(),
        ));
    };
    let id = call_blocking(state.app_state.db.clone(), move |db| {
        db.upsert_hook_definition(
            body.id,
            &body.name,
            &body.event_name,
            body.matcher.as_deref(),
            &body.action_type,
            &payload_json,
            scoped_persona_ids.as_deref(),
            enabled,
        )
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn api_hooks_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<HookDefinitionDeletePathParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_hook_definition(path.id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "deleted": deleted })))
}

async fn api_persona_policy_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaPolicyPathParams>,
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
    let policy = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_persona_hook_skill_policy(chat_id, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": path.persona_id,
        "allowed_hook_ids": policy.as_ref().and_then(|p| p.allowed_hook_ids.clone()),
        "allowed_skill_names": policy.as_ref().and_then(|p| p.allowed_skill_names.clone()),
        "uses_default_hooks": policy.as_ref().and_then(|p| p.allowed_hook_ids.as_ref()).is_none(),
        "uses_default_skills": policy.as_ref().and_then(|p| p.allowed_skill_names.as_ref()).is_none(),
        "updated_at": policy.as_ref().map(|p| p.updated_at.clone()),
    })))
}

async fn api_persona_policy_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaPolicyPathParams>,
    Json(body): Json<PersonaPolicyPatchBody>,
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
    let current = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_persona_hook_skill_policy(chat_id, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut next_hooks = current.as_ref().and_then(|p| p.allowed_hook_ids.clone());
    let mut next_skills = current.as_ref().and_then(|p| p.allowed_skill_names.clone());
    if let Some(v) = body.allowed_hook_ids {
        next_hooks = v;
    }
    if let Some(v) = body.allowed_skill_names {
        next_skills = v;
    }
    if let Some(list) = next_hooks.as_mut() {
        list.retain(|id| *id > 0);
        list.sort_unstable();
        list.dedup();
    }
    if let Some(list) = next_skills.as_mut() {
        let mut norm = Vec::new();
        for item in list.iter() {
            let trimmed = item.trim();
            if !trimmed.is_empty() {
                norm.push(trimmed.to_string());
            }
        }
        norm.sort_by_key(|s| s.to_ascii_lowercase());
        norm.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        *list = norm;
    }

    let next_hooks_for_db = next_hooks.clone();
    let next_skills_for_db = next_skills.clone();
    call_blocking(state.app_state.db.clone(), move |db| {
        db.set_persona_hook_skill_policy(
            chat_id,
            pid,
            next_hooks_for_db.as_deref(),
            next_skills_for_db.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "persona_id": path.persona_id,
        "allowed_hook_ids": next_hooks,
        "allowed_skill_names": next_skills,
    })))
}

async fn api_persona_bulletin_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaBookmarkPathParams>,
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

    let pid2 = path.persona_id;
    let focus = call_blocking(state.app_state.db.clone(), move |db| {
        db.get_persona_bulletin_focus(chat_id, pid2)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let pid3 = path.persona_id;
    let bookmarks = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_persona_message_bookmarks(chat_id, pid3, 20)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let focus_json = focus.map(|f| {
        json!({
            "title": f.title,
            "content": f.content,
            "updated_at": f.updated_at,
        })
    });
    let bookmarks_json: Vec<serde_json::Value> = bookmarks
        .into_iter()
        .map(|b| {
            json!({
                "message_id": b.message_id,
                "role": b.role,
                "content_preview": b.content_preview,
                "note": b.note,
                "created_at": b.created_at,
                "updated_at": b.updated_at,
            })
        })
        .collect();

    let pid4 = path.persona_id;
    let persona = call_blocking(state.app_state.db.clone(), move |db| {
        let p = db.get_persona(pid4)?;
        Ok(p.filter(|x| x.chat_id == chat_id))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let cfg = &state.app_state.config;
    let def_u = cfg.recent_history_min_user_messages.clamp(1, 25) as i64;
    let def_a = cfg.recent_history_min_assistant_messages.clamp(1, 25) as i64;
    let global_engine = state.app_state.runtime_toggles.agent_engine();
    let (hs_json, delivery_json, engine_json) = if let Some(ref p) = persona {
        let o_u = p.recent_history_min_user;
        let o_a = p.recent_history_min_assistant;
        let e_u = o_u.unwrap_or(def_u).clamp(1, 25);
        let e_a = o_a.unwrap_or(def_a).clamp(1, 25);
        let hs = json!({
            "min_user": {
                "effective": e_u,
                "persona_override": o_u,
                "uses_default": o_u.is_none(),
            },
            "min_assistant": {
                "effective": e_a,
                "persona_override": o_a,
                "uses_default": o_a.is_none(),
            },
            "defaults": { "min_user": def_u, "min_assistant": def_a },
        });
        let msg_max = crate::dense_delivery_guard::effective_max_chars(p, "wecom");
        let web_max = crate::dense_delivery_guard::effective_max_chars(p, "web");
        let delivery = json!({
            "enabled": p.dense_delivery_enabled,
            "messaging_max_chars": msg_max,
            "web_max_chars": web_max,
            "summary_chars": crate::dense_delivery_guard::effective_summary_chars(p),
            "defaults": {
                "messaging_max_chars": crate::dense_delivery_guard::DEFAULT_MESSAGING_MAX_CHARS,
                "web_max_chars": crate::dense_delivery_guard::DEFAULT_WEB_MAX_CHARS,
                "summary_chars": crate::dense_delivery_guard::DEFAULT_SUMMARY_CHARS,
            },
        });
        let effective = crate::runtime_toggles::resolve_run_agent_engine(
            p.agent_engine_override.as_deref(),
            global_engine,
        );
        let engine = json!({
            "override": p.agent_engine_override,
            "global": global_engine.as_str(),
            "effective": effective.as_str(),
            "uses_default": p.agent_engine_override.as_deref().map(str::trim).unwrap_or("").is_empty(),
        });
        (hs, delivery, engine)
    } else {
        (json!({}), json!({}), json!({}))
    };

    Ok(Json(json!({
        "ok": true,
        "persona_id": path.persona_id,
        "focus": focus_json,
        "bookmarks": bookmarks_json,
        "history_suffix": hs_json,
        "operator_memo": persona.as_ref().and_then(|p| p.operator_memo.clone()),
        "dense_delivery_enabled": persona.as_ref().map(|p| p.dense_delivery_enabled).unwrap_or(false),
        "dense_delivery": delivery_json,
        "agent_engine_override": persona.as_ref().and_then(|p| p.agent_engine_override.clone()),
        "agent_engine_global": global_engine.as_str(),
        "agent_engine_effective": crate::runtime_toggles::resolve_run_agent_engine(
            persona.as_ref().and_then(|p| p.agent_engine_override.as_deref()),
            global_engine,
        ).as_str(),
        "agent_engine": engine_json,
    })))
}

async fn api_persona_bulletin_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaBookmarkPathParams>,
    Json(body): Json<PersonaBulletinPatchBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let pid = path.persona_id;
    if body.recent_history_min_user.is_none()
        && body.recent_history_min_assistant.is_none()
        && body.operator_memo.is_none()
        && body.dense_delivery_enabled.is_none()
        && body.dense_delivery_messaging_max_chars.is_none()
        && body.dense_delivery_web_max_chars.is_none()
        && body.dense_delivery_summary_chars.is_none()
        && body.agent_engine_override.is_none()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "no fields to update (send recent_history_min_user, recent_history_min_assistant, operator_memo, dense_delivery_enabled, and/or agent_engine_override)"
                .into(),
        ));
    }

    let exists = call_blocking(state.app_state.db.clone(), move |db| {
        db.persona_exists(chat_id, pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !exists {
        return Err((StatusCode::NOT_FOUND, "persona not found".into()));
    }

    let pid2 = path.persona_id;
    let current = call_blocking(state.app_state.db.clone(), move |db| db.get_persona(pid2))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "persona not found".into()))?;
    if current.chat_id != chat_id {
        return Err((StatusCode::NOT_FOUND, "persona not found".into()));
    }

    let patch_prompt = body.recent_history_min_user.is_some()
        || body.recent_history_min_assistant.is_some()
        || body.operator_memo.is_some();
    let mut nu = current.recent_history_min_user;
    let mut na = current.recent_history_min_assistant;
    let mut memo = current.operator_memo.clone();

    if let Some(v) = body.recent_history_min_user {
        if let Some(n) = v {
            if !(1..=25).contains(&n) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "recent_history_min_user must be between 1 and 25".into(),
                ));
            }
            nu = Some(n);
        } else {
            nu = None;
        }
    }
    if let Some(v) = body.recent_history_min_assistant {
        if let Some(n) = v {
            if !(1..=25).contains(&n) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "recent_history_min_assistant must be between 1 and 25".into(),
                ));
            }
            na = Some(n);
        } else {
            na = None;
        }
    }
    if let Some(v) = body.operator_memo {
        memo = match v {
            None => None,
            Some(s) => {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else if t.chars().count() > crate::db::OPERATOR_MEMO_MAX_CHARS {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!(
                            "operator_memo exceeds {} characters",
                            crate::db::OPERATOR_MEMO_MAX_CHARS
                        ),
                    ));
                } else {
                    Some(s)
                }
            }
        };
    }

    let mut dense_enabled = current.dense_delivery_enabled;
    let mut msg_max = current.dense_delivery_messaging_max_chars;
    let mut web_max = current.dense_delivery_web_max_chars;
    let mut summary_chars = current.dense_delivery_summary_chars;
    let mut engine_ov = current.agent_engine_override.clone();
    let mut patch_delivery = false;
    let mut patch_engine = false;

    if let Some(v) = body.dense_delivery_enabled {
        dense_enabled = v;
        patch_delivery = true;
    }
    if let Some(v) = body.dense_delivery_messaging_max_chars {
        msg_max = v.filter(|n| *n > 0);
        patch_delivery = true;
    }
    if let Some(v) = body.dense_delivery_web_max_chars {
        web_max = v.filter(|n| *n > 0);
        patch_delivery = true;
    }
    if let Some(v) = body.dense_delivery_summary_chars {
        summary_chars = v.filter(|n| *n > 0);
        patch_delivery = true;
    }
    if let Some(v) = body.agent_engine_override {
        patch_engine = true;
        engine_ov = match v {
            None => None,
            Some(s) => {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else if crate::runtime_toggles::AgentEngine::parse_override(t).is_none() {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "agent_engine_override must be classic, classic_cost_routing, cursor, deterministic, or null".into(),
                    ));
                } else {
                    Some(t.to_string())
                }
            }
        };
    }

    let ok = call_blocking(state.app_state.db.clone(), move |db| {
        if patch_prompt {
            db.set_persona_prompt_overrides(chat_id, pid, nu, na, memo.as_deref())?;
        }
        if patch_delivery {
            db.set_persona_delivery_policy(
                chat_id,
                pid,
                dense_enabled,
                msg_max,
                web_max,
                summary_chars,
            )?;
        }
        if patch_engine {
            db.set_persona_engine_override(chat_id, pid, engine_ov.as_deref())?;
        }
        Ok(true)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !ok {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to update persona".into(),
        ));
    }

    Ok(Json(json!({ "ok": true, "persona_id": path.persona_id })))
}

async fn api_persona_bookmarks_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaBookmarkPathParams>,
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

    let pid2 = path.persona_id;
    let bookmarks = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_persona_message_bookmarks(chat_id, pid2, 50)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let bookmarks_json: Vec<serde_json::Value> = bookmarks
        .into_iter()
        .map(|b| {
            json!({
                "message_id": b.message_id,
                "role": b.role,
                "content_preview": b.content_preview,
                "note": b.note,
                "created_at": b.created_at,
                "updated_at": b.updated_at,
            })
        })
        .collect();
    Ok(Json(json!({
        "ok": true,
        "persona_id": path.persona_id,
        "bookmarks": bookmarks_json,
    })))
}

async fn api_persona_bookmarks_post(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaBookmarkPathParams>,
    Json(body): Json<PersonaBookmarkUpsertBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    if body.message_id.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message_id is required".into()));
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
    let message_id = body.message_id.trim().to_string();
    let pid_lookup = path.persona_id;
    let message = call_blocking(state.app_state.db.clone(), {
        let message_id = message_id.clone();
        move |db| db.get_message_for_persona(chat_id, pid_lookup, &message_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some(message) = message else {
        return Err((
            StatusCode::NOT_FOUND,
            "message not found for persona".into(),
        ));
    };
    let role = if message.is_from_bot {
        "assistant"
    } else {
        "user"
    };
    let preview = truncate_chars(message.content.trim(), 280);
    let note_clean = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, 180));
    let pid_upsert = path.persona_id;
    call_blocking(state.app_state.db.clone(), {
        let note_clean = note_clean.clone();
        let message_id = message_id.clone();
        let preview = preview.clone();
        move |db| {
            db.upsert_persona_message_bookmark(
                chat_id,
                pid_upsert,
                &message_id,
                role,
                &preview,
                note_clean.as_deref(),
            )
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "persona_id": path.persona_id,
        "bookmark": {
            "message_id": message_id,
            "role": role,
            "content_preview": preview,
            "note": note_clean,
        }
    })))
}

async fn api_persona_bookmarks_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaBookmarkDeletePathParams>,
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
    let message_id = path.message_id.trim().to_string();
    if message_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message_id is required".into()));
    }
    let pid_delete = path.persona_id;
    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_persona_message_bookmark(chat_id, pid_delete, &message_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "persona_id": path.persona_id,
        "message_id": path.message_id,
        "deleted": deleted,
    })))
}

/// Full text of one message in the persona thread (used by web bookmark reader; bookmarks only store a short preview).
async fn api_persona_message_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaMessagePathParams>,
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

    let message_id = path.message_id.trim().to_string();
    if message_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message_id is required".into()));
    }

    let pid_lookup = path.persona_id;
    let cid = chat_id;
    let message = call_blocking(state.app_state.db.clone(), {
        let message_id = message_id.clone();
        move |db| db.get_message_for_persona(cid, pid_lookup, &message_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let Some(m) = message else {
        return Err((StatusCode::NOT_FOUND, "message not found".into()));
    };

    Ok(Json(json!({
        "ok": true,
        "persona_id": path.persona_id,
        "message": {
            "id": m.id,
            "sender_name": m.sender_name,
            "content": m.content,
            "is_from_bot": m.is_from_bot,
            "timestamp": m.timestamp,
        }
    })))
}

async fn api_persona_message_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaMessagePathParams>,
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

    let message_id = path.message_id.trim().to_string();
    if message_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message_id is required".into()));
    }

    let pid_delete = path.persona_id;
    let deleted = call_blocking(state.app_state.db.clone(), {
        let message_id = message_id.clone();
        move |db| db.delete_message(chat_id, pid_delete, &message_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !deleted {
        return Err((StatusCode::NOT_FOUND, "message not found".into()));
    }

    Ok(Json(json!({
        "ok": true,
        "persona_id": path.persona_id,
        "message_id": path.message_id,
        "deleted": true,
    })))
}

fn ensure_persona_memory_file_exists_for_web(state: &AppState, chat_id: i64, persona_id: i64) {
    let path = state.memory.persona_memory_state_path(chat_id, persona_id);
    if path.exists() {
        return;
    }
    let display_name = if !state.config.agent_display_name.trim().is_empty() {
        Some(state.config.agent_display_name.as_str())
    } else {
        None
    };
    let _ = state
        .memory
        .ensure_persona_memory_state_exists(chat_id, persona_id, display_name);
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
    let mem_path = state
        .app_state
        .memory
        .persona_memory_state_path(chat_id, pid);
    let memory_state = state
        .app_state
        .memory
        .read_or_migrate_persona_memory_state(chat_id, pid)
        .unwrap_or_default();
    let content = serde_json::to_string_pretty(&memory_state).unwrap_or_else(|_| "{}".to_string());
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
    let mem_path = state
        .app_state
        .memory
        .persona_memory_state_path(chat_id, pid);
    let current_mtime = file_mtime_ms(&mem_path).unwrap_or(0);
    if let Some(expected) = body.if_match_mtime_ms {
        if expected != current_mtime {
            return Err((
                StatusCode::CONFLICT,
                "memory was modified; reload and retry".into(),
            ));
        }
    }

    let mut state_payload: crate::memory::PersonaMemoryState = serde_json::from_str(&body.content)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("invalid memory_state JSON: {e}"),
            )
        })?;
    state_payload.normalize();
    state
        .app_state
        .memory
        .validate_memory_state(&state_payload)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    state
        .app_state
        .memory
        .write_persona_memory_state(chat_id, pid, state_payload)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = state.app_state.memory.append_persona_memory_event(
        chat_id,
        pid,
        "web_memory_state_put",
        "user_manual",
        json!({"path": mem_path.to_string_lossy().to_string()}),
    );

    let new_mtime = file_mtime_ms(&mem_path).unwrap_or(0);
    Ok(Json(json!({
        "ok": true,
        "persona_id": pid,
        "mtime_ms": new_mtime,
    })))
}

async fn api_workspace_agents_md_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let path = state.app_state.memory.groups_root_memory_path();
    let content = state
        .app_state
        .memory
        .read_groups_root_memory()
        .unwrap_or_default();
    let mtime_ms = file_mtime_ms(&path).unwrap_or(0);
    Ok(Json(json!({
        "ok": true,
        "content": content,
        "mtime_ms": mtime_ms,
        "path": path.to_string_lossy(),
    })))
}

#[derive(Deserialize)]
struct WorkspaceAgentsMdPutBody {
    content: String,
    if_match_mtime_ms: Option<i64>,
}

async fn api_workspace_agents_md_put(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<WorkspaceAgentsMdPutBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    if body.content.len() > 256 * 1024 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "principles content too large".into(),
        ));
    }

    let path = state.app_state.memory.groups_root_memory_path();
    let current_mtime = file_mtime_ms(&path).unwrap_or(0);
    if let Some(expected) = body.if_match_mtime_ms {
        if expected != current_mtime {
            return Err((
                StatusCode::CONFLICT,
                "principles file was modified; reload and retry".into(),
            ));
        }
    }

    state
        .app_state
        .memory
        .write_groups_root_memory(&body.content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let new_mtime = file_mtime_ms(&path).unwrap_or(0);
    Ok(Json(json!({
        "ok": true,
        "mtime_ms": new_mtime,
    })))
}

async fn api_persona_agent_history_latest(
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

    let data_dir = state.app_state.config.runtime_data_dir();
    match crate::agent_history::read_latest_agent_history(&data_dir, chat_id, pid) {
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            "no agent history for this persona".into(),
        )),
        Ok(Some(r)) => Ok(Json(json!({
            "ok": true,
            "persona_id": pid,
            "filename": r.filename,
            "content": r.content,
            "mtime_ms": r.mtime_ms,
            "path": r.path.to_string_lossy(),
        }))),
        Err(crate::agent_history::ReadLatestAgentHistoryError::FileTooLarge(_)) => Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "agent history file too large".into(),
        )),
        Err(crate::agent_history::ReadLatestAgentHistoryError::Io(e)) => {
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

async fn api_persona_agent_history_optimize(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<PersonaMemoryPathParams>,
    body: Option<Json<AgentHistoryOptimizeRequest>>,
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

    let mm = state.app_state.llm.local_delegate_config();
    if mm.tier2_base_url.trim().is_empty() || mm.tier2_model.trim().is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Tier 2 (Knowledge) base URL and model must be configured in Settings → Multi-model."
                .into(),
        ));
    }

    let data_dir = state.app_state.config.runtime_data_dir();
    let history = match crate::agent_history::read_latest_agent_history(&data_dir, chat_id, pid) {
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                "no agent history for this persona".into(),
            ))
        }
        Ok(Some(r)) => r,
        Err(crate::agent_history::ReadLatestAgentHistoryError::FileTooLarge(_)) => {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "agent history file too large".into(),
            ))
        }
        Err(crate::agent_history::ReadLatestAgentHistoryError::Io(e)) => {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    };

    let operator_notes = body
        .and_then(|Json(req)| req.operator_notes)
        .and_then(|notes| {
            let trimmed = notes.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

    let app_state = state.app_state.clone();
    match crate::run_optimizer::try_enqueue_run_optimize(
        app_state,
        chat_id,
        pid,
        history.filename.clone(),
        history.content,
        operator_notes,
    )
    .await
    {
        crate::run_optimizer::RunOptimizeEnqueueOutcome::Queued { job_id } => Ok(Json(json!({
            "ok": true,
            "job_id": job_id,
            "filename": history.filename,
            "message": "Queued. Track progress in Background jobs.",
        }))),
        crate::run_optimizer::RunOptimizeEnqueueOutcome::BlockedAlreadyRunning => Err((
            StatusCode::CONFLICT,
            "another background job is already active for this chat".into(),
        )),
        crate::run_optimizer::RunOptimizeEnqueueOutcome::ActiveLookupFailed(e) => {
            Err((StatusCode::INTERNAL_SERVER_ERROR, e))
        }
        crate::run_optimizer::RunOptimizeEnqueueOutcome::DbCreateFailed(e) => {
            Err((StatusCode::INTERNAL_SERVER_ERROR, e))
        }
    }
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
    crate::tools::ensure_persona_shared_dir(
        std::path::Path::new(state.app_state.config.working_dir()),
        chat_id,
        persona_id,
    )
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
        db.link_channel(contact_chat_id, BOT_INSTANCE_WEB, "web", "default")
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
        db.unlink_channel(BOT_INSTANCE_WEB, "web", "default")
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

fn persona_todo_to_json(t: &crate::db::PersonaTodo) -> serde_json::Value {
    json!({
        "id": t.id,
        "chat_id": t.chat_id,
        "persona_id": t.persona_id,
        "title": t.title,
        "status": t.status,
        "source_hint": t.source_hint,
        "created_at": t.created_at,
        "updated_at": t.updated_at,
        "completed_at": t.completed_at,
    })
}

async fn api_todos_list(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<TodosQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let status_raw = query
        .status
        .as_deref()
        .unwrap_or("open")
        .trim()
        .to_ascii_lowercase();
    let status_filter: Option<String> = match status_raw.as_str() {
        "all" => None,
        "open" | "done" => Some(status_raw.clone()),
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Invalid status '{other}'; use open, done, or all"),
            ))
        }
    };

    let filter = status_filter.clone();
    let todos = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_todos_for_chat(chat_id, filter.as_deref(), 200)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = todos.iter().map(persona_todo_to_json).collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "status": status_raw,
        "todos": items,
    })))
}

async fn api_todos_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(path): Path<TodoPathParams>,
    Json(body): Json<TodoPatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let status = body.status.trim().to_ascii_lowercase();
    if status != "open" && status != "done" {
        return Err((
            StatusCode::BAD_REQUEST,
            "status must be 'open' or 'done'".into(),
        ));
    }

    let chat_id = resolve_chat_id_for_web(None, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;

    let todo_id = path.id;
    let updated = call_blocking(state.app_state.db.clone(), move |db| {
        let Some(existing) = db.get_persona_todo(todo_id)? else {
            return Ok(None);
        };
        if existing.chat_id != chat_id {
            return Ok(None);
        }
        db.set_persona_todo_status(todo_id, &status)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let Some(todo) = updated else {
        return Err((StatusCode::NOT_FOUND, "todo not found".into()));
    };

    Ok(Json(json!({
        "ok": true,
        "todo": persona_todo_to_json(&todo),
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
    let prompt_update = body.prompt.as_ref().map(|p| p.trim().to_string());
    let schedule_type_in = body.schedule_type.as_ref().map(|s| s.trim().to_string());
    let schedule_value_in = body.schedule_value.as_ref().map(|s| s.trim().to_string());
    let has_schedule_pair = schedule_type_in.is_some() && schedule_value_in.is_some();
    let schedule_partial = schedule_type_in.is_some() ^ schedule_value_in.is_some();
    if schedule_partial {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide both schedule_type and schedule_value to change the schedule".into(),
        ));
    }
    if status.is_none() && persona_id.is_none() && prompt_update.is_none() && !has_schedule_pair {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide at least one field to update: status, persona_id, prompt, or schedule_type+schedule_value".into(),
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

    if let Some(ref p) = prompt_update {
        if p.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "prompt must not be empty".into()));
        }
    }

    let schedule_preflight = if has_schedule_pair {
        let st = schedule_type_in.as_ref().unwrap().as_str();
        let sv = schedule_value_in.as_ref().unwrap().as_str();
        let effective_tz = body.timezone.as_deref().or_else(|| {
            let default = state.app_state.config.timezone.trim();
            if default.is_empty() {
                None
            } else {
                Some(default)
            }
        });
        Some(
            crate::tools::schedule::preflight_schedule_request(st, sv, effective_tz)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?,
        )
    } else {
        None
    };

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
    if let Some(p) = prompt_update {
        let ok = call_blocking(state.app_state.db.clone(), move |db| {
            db.update_task_prompt(task_id, &p)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !ok {
            return Err((StatusCode::NOT_FOUND, "Task not found".into()));
        }
    }

    if let Some(pref) = &schedule_preflight {
        let schedule_type_owned = schedule_type_in.as_ref().unwrap().clone();
        let schedule_value_owned = pref.schedule_value.clone();
        let next_run_owned = pref.next_run.clone();
        let ok = call_blocking(state.app_state.db.clone(), move |db| {
            db.update_task_schedule(
                task_id,
                &schedule_type_owned,
                &schedule_value_owned,
                &next_run_owned,
            )
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !ok {
            return Err((StatusCode::NOT_FOUND, "Task not found".into()));
        }
    }

    let mut response = json!({
        "ok": true,
        "message": "Task updated",
    });
    if let Some(pref) = schedule_preflight {
        response["next_run"] = json!(pref.next_run);
        response["timezone"] = json!(pref.timezone_used);
        response["timezone_assumption"] = json!(if pref.timezone_defaulted_to_utc {
            "Timezone not provided. UTC was assumed."
        } else {
            "Timezone provided by request."
        });
    }

    Ok(Json(response))
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

    let heartbeats = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_job_heartbeats_for_chat(chat_id, 200)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let hb_by_key: HashMap<String, JobHeartbeat> = heartbeats
        .into_iter()
        .map(|h| (h.run_key.clone(), h))
        .collect();

    let active_heartbeats = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_active_job_heartbeats_for_chat(chat_id, 20)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let pending_timeout_secs = state
        .app_state
        .config
        .background_job_pending_start_timeout_secs as i64;
    let active_count = call_blocking(state.app_state.db.clone(), move |db| {
        db.count_active_background_jobs_for_chat(chat_id, &now, pending_timeout_secs)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = jobs
        .into_iter()
        .map(|j| {
            let hb = hb_by_key.get(&j.id);
            let mut row = json_background_job(&j);
            if let Some(obj) = row.as_object_mut() {
                obj.insert(
                    "heartbeat".into(),
                    hb.map(json_job_heartbeat)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            row
        })
        .collect();

    let active_hb_json: Vec<serde_json::Value> =
        active_heartbeats.iter().map(json_job_heartbeat).collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "jobs": items,
        "active_count": active_count,
        "active_heartbeats": active_hb_json,
    })))
}

async fn api_background_job_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(job_id): Path<String>,
    Query(query): Query<BackgroundJobsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    let job = call_blocking(state.app_state.db.clone(), {
        let job_id = job_id.clone();
        move |db| db.get_background_job(&job_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some(job) = job else {
        return Err((StatusCode::NOT_FOUND, "background job not found".into()));
    };
    if job.chat_id != chat_id {
        return Err((StatusCode::NOT_FOUND, "background job not found".into()));
    }

    let hb = call_blocking(state.app_state.db.clone(), {
        let job_id = job_id.clone();
        move |db| db.get_job_heartbeat(&job_id)
    })
    .await
    .ok()
    .flatten();

    let timeline = call_blocking(state.app_state.db.clone(), {
        let job_id = job_id.clone();
        move |db| db.get_run_timeline_events(&job_id, 50)
    })
    .await
    .unwrap_or_default();

    let timeline_json: Vec<serde_json::Value> = timeline
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id,
                "event_type": e.event_type,
                "payload_json": e.payload_json,
                "created_at": e.created_at,
            })
        })
        .collect();

    let mut job_json = json_background_job(&job);
    if let Some(obj) = job_json.as_object_mut() {
        if let Some(rt) = job.result_text {
            obj.insert("result_text".into(), json!(rt));
        }
    }

    Ok(Json(json!({
        "ok": true,
        "job": job_json,
        "heartbeat": hb.as_ref().map(json_job_heartbeat),
        "timeline_events_recent": timeline_json,
    })))
}

#[derive(Debug, Deserialize)]
struct ContactsBindingsQuery {
    chat_id: Option<i64>,
}

/// Accepted for API compatibility; [`api_settings_patch`] always returns 501.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SettingsPatchRequest {
    upsert: Option<HashMap<String, String>>,
    remove: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ChannelPersonaPolicyUpsertRequest {
    chat_id: Option<i64>,
    bot_instance_id: i64,
    mode: String, // "all" | "single"
    persona_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ChannelPersonaPolicyDeleteRequest {
    chat_id: Option<i64>,
    bot_instance_id: i64,
}

fn setting_is_secret(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("TOKEN")
        || upper.contains("SECRET")
        || upper.contains("API_KEY")
        || upper.ends_with("_KEY")
        || upper.contains("PASSWORD")
}

fn mask_setting_value(value: &str) -> String {
    if value.len() <= 6 {
        return "***".to_string();
    }
    let prefix = &value[..3];
    let suffix = &value[value.len().saturating_sub(2)..];
    format!("{prefix}***{suffix}")
}

fn is_llm_ready(cfg: &Config) -> bool {
    crate::llm_catalog::is_api_key_configured_for_provider(&cfg.llm_provider)
        || crate::llm_catalog::any_provider_api_key_configured()
}

fn is_channel_ready(db: &crate::db::Database) -> bool {
    crate::channel_integration_config::has_any_messaging_bot_token(db).unwrap_or(false)
}

async fn api_contacts_bindings(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<ContactsBindingsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;

    let chat_id = resolve_chat_id_for_web(query.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let rows = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_contact_channel_integration_rows(chat_id)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            let (persona_mode, persona_id) = match r.persona_mode {
                ChannelPersonaMode::All => ("all", None),
                ChannelPersonaMode::Single => ("single", r.persona_id),
            };
            json!({
                "bot_instance_id": r.bot_instance_id,
                "platform": r.platform,
                "label": r.label,
                "channel_type": r.platform,
                "channel_handle": r.channel_handle,
                "linked": r.linked,
                "persona_mode": persona_mode,
                "persona_id": persona_id
            })
        })
        .collect();

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "bindings": items,
    })))
}

#[derive(Debug, Deserialize)]
struct LlmModelPatchRequest {
    model: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    custom: bool,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    thinking_enabled: Option<bool>,
    #[serde(default)]
    show_thinking: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct LlmModelsQuery {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimePatchRequest {
    #[serde(default)]
    tool_output_debug: Option<bool>,
    #[serde(default)]
    post_tool_evaluator_enabled: Option<bool>,
    #[serde(default)]
    response_quality_evaluator_enabled: Option<bool>,
    #[serde(default)]
    agent_engine: Option<String>,
}

async fn api_runtime_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let sources = call_blocking(state.app_state.db.clone(), |db| {
        Ok::<_, crate::error::FinallyAValueBotError>((
            crate::runtime_toggles::RuntimeToggles::tool_output_debug_from_app_settings(db)?,
            crate::runtime_toggles::RuntimeToggles::post_tool_evaluator_from_app_settings(db)?,
            crate::runtime_toggles::RuntimeToggles::response_quality_evaluator_from_app_settings(
                db,
            )?,
            crate::runtime_toggles::RuntimeToggles::agent_engine_from_app_settings(db)?,
        ))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let toggles = &state.app_state.runtime_toggles;
    let mm_cfg = state.app_state.llm.local_delegate_config();
    let engine = toggles.agent_engine();
    let local_ready = mm_cfg.local_routable();
    Ok(Json(json!({
        "ok": true,
        "tool_output_debug": toggles.tool_output_debug(),
        "post_tool_evaluator_enabled": toggles.post_tool_evaluator_enabled(),
        "response_quality_evaluator_enabled": toggles.response_quality_evaluator_enabled(),
        "agent_engine": engine.as_str(),
        "local_delegate_configured": mm_cfg.local_configured(),
        "local_delegate_tools_ok": mm_cfg.local_tools_ok,
        "local_delegate_ready": local_ready,
        "cost_routing_effective": crate::local_delegate::cost_routing_effective(engine, &mm_cfg),
        "terminal": web_terminal::capabilities_json(&state.app_state.config),
        "sources": {
            "tool_output_debug": if sources.0 { "app_settings" } else { "env" },
            "post_tool_evaluator_enabled": if sources.1 { "app_settings" } else { "env" },
            "response_quality_evaluator_enabled": if sources.2 { "app_settings" } else { "env" },
            "agent_engine": if sources.3.is_some() { "app_settings" } else { "env" },
        },
        "description": "When enabled, verbose shell output is shown in chat (including background-job completion). When off, full logs are agent-only.",
    })))
}

async fn api_runtime_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<RuntimePatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let toggles = &state.app_state.runtime_toggles;
    let mut messages: Vec<&str> = Vec::new();

    if let Some(enabled) = body.tool_output_debug {
        toggles.set_tool_output_debug(enabled);
        call_blocking(state.app_state.db.clone(), move |db| {
            crate::runtime_toggles::RuntimeToggles::persist_tool_output_debug(db, enabled)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        messages.push(if enabled {
            "Verbose pipeline logging enabled."
        } else {
            "Verbose pipeline logging disabled."
        });
    }
    if let Some(enabled) = body.post_tool_evaluator_enabled {
        toggles.set_post_tool_evaluator_enabled(enabled);
        call_blocking(state.app_state.db.clone(), move |db| {
            crate::runtime_toggles::RuntimeToggles::persist_post_tool_evaluator_enabled(db, enabled)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        messages.push(if enabled {
            "Post-tool evaluator (PTE) enabled."
        } else {
            "Post-tool evaluator (PTE) disabled."
        });
    }
    if let Some(enabled) = body.response_quality_evaluator_enabled {
        toggles.set_response_quality_evaluator_enabled(enabled);
        call_blocking(state.app_state.db.clone(), move |db| {
            crate::runtime_toggles::RuntimeToggles::persist_response_quality_evaluator_enabled(
                db, enabled,
            )
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        messages.push(if enabled {
            "Pre-delivery quality evaluator (PDQE) enabled."
        } else {
            "Pre-delivery quality evaluator (PDQE) disabled."
        });
    }
    if let Some(ref engine_raw) = body.agent_engine {
        let engine = crate::runtime_toggles::AgentEngine::parse(engine_raw);
        toggles.set_agent_engine(engine);
        call_blocking(state.app_state.db.clone(), move |db| {
            crate::runtime_toggles::RuntimeToggles::persist_agent_engine(db, engine)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let routing_for_engine = engine == crate::runtime_toggles::AgentEngine::ClassicCostRouting;
        let mut mm_cfg = state.app_state.llm.local_delegate_config();
        if mm_cfg.routing_enabled != routing_for_engine {
            mm_cfg.routing_enabled = routing_for_engine;
            mm_cfg = mm_cfg.normalize();
            call_blocking(state.app_state.db.clone(), {
                let cfg_db = mm_cfg.clone();
                move |db| crate::local_delegate::persist_to_db(db, &cfg_db)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            state
                .app_state
                .llm
                .apply_local_delegate_config(mm_cfg.clone())
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        }

        messages.push(match engine {
            crate::runtime_toggles::AgentEngine::Deterministic => {
                "Agent engine set to deterministic pipeline."
            }
            crate::runtime_toggles::AgentEngine::Cursor => {
                "Agent engine set to Cursor SDK (local sidecar)."
            }
            crate::runtime_toggles::AgentEngine::ClassicCostRouting => {
                "Agent engine set to Classic · Cost routing."
            }
            crate::runtime_toggles::AgentEngine::Classic => {
                "Agent engine set to Classic · Single turn."
            }
        });
    }

    let mm_cfg = state.app_state.llm.local_delegate_config();
    let engine = toggles.agent_engine();
    let local_ready = mm_cfg.local_routable();
    let cost_routing_effective = crate::local_delegate::cost_routing_effective(engine, &mm_cfg);
    let mut warnings: Vec<String> = Vec::new();
    if engine == crate::runtime_toggles::AgentEngine::ClassicCostRouting && !local_ready {
        if !mm_cfg.local_configured() {
            warnings.push(
                "Cost routing selected but local URL/model are not configured. Runs use cloud model only until configured and verified.".into(),
            );
        } else if !mm_cfg.local_tools_ok {
            warnings.push(
                "Cost routing selected but local tool calling is not verified. Runs use cloud model only until you run Test in Local delegate.".into(),
            );
        }
    }

    if messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "No runtime toggle fields provided".to_string(),
        ));
    }

    Ok(Json(json!({
        "ok": true,
        "tool_output_debug": toggles.tool_output_debug(),
        "post_tool_evaluator_enabled": toggles.post_tool_evaluator_enabled(),
        "response_quality_evaluator_enabled": toggles.response_quality_evaluator_enabled(),
        "agent_engine": engine.as_str(),
        "local_delegate_configured": mm_cfg.local_configured(),
        "local_delegate_tools_ok": mm_cfg.local_tools_ok,
        "local_delegate_ready": local_ready,
        "cost_routing_effective": cost_routing_effective,
        "warnings": warnings,
        "source": "app_settings",
        "message": messages.join(" "),
    })))
}

async fn api_deterministic_pipeline_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let profile = state
        .app_state
        .pipeline_profile
        .read()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "pipeline profile lock poisoned".into(),
            )
        })?
        .clone();
    let defaults = crate::agent_pipeline::profile::PipelineProfile::default_profile();
    let builtin_prompts: serde_json::Map<String, serde_json::Value> =
        crate::agent_pipeline::profile::PhaseKind::all()
            .iter()
            .map(|kind| {
                (
                    kind.label().to_string(),
                    serde_json::Value::String(
                        crate::agent_pipeline::profile::builtin_prompt_for_kind(*kind).to_string(),
                    ),
                )
            })
            .collect();
    Ok(Json(json!({
        "ok": true,
        "schema_version": crate::agent_pipeline::profile::SCHEMA_VERSION,
        "profile": profile,
        "defaults": defaults,
        "builtin_prompts": builtin_prompts,
        "agent_engine": state.app_state.runtime_toggles.agent_engine().as_str(),
    })))
}

#[derive(Debug, Deserialize)]
struct DeterministicPipelinePatchRequest {
    #[serde(default)]
    reset_defaults: bool,
    #[serde(default)]
    profile: Option<crate::agent_pipeline::profile::PipelineProfile>,
}

async fn api_deterministic_pipeline_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<DeterministicPipelinePatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let new_profile = if body.reset_defaults {
        crate::agent_pipeline::profile::PipelineProfile::default_profile()
    } else if let Some(p) = body.profile {
        p
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide profile or reset_defaults=true".into(),
        ));
    };
    let new_profile = new_profile.migrate();
    if let Err(errs) = new_profile.validate() {
        return Err((
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&json!({ "ok": false, "errors": errs }))
                .unwrap_or_else(|_| "validation failed".into()),
        ));
    }
    call_blocking(state.app_state.db.clone(), {
        let profile_db = new_profile.clone();
        move |db| crate::agent_pipeline::profile::persist_to_db(db, &profile_db)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    {
        let mut guard = state.app_state.pipeline_profile.write().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "pipeline profile lock poisoned".into(),
            )
        })?;
        *guard = new_profile.clone();
    }
    Ok(Json(json!({
        "ok": true,
        "profile": new_profile,
        "message": if body.reset_defaults {
            "Deterministic pipeline profile reset to defaults."
        } else {
            "Deterministic pipeline profile saved."
        },
    })))
}

async fn api_llm_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let provider_id =
        crate::llm_catalog::resolve_catalog_provider_id(&state.app_state.llm.current_provider());
    let preset = crate::llm_catalog::find_provider(&provider_id);
    let current_model = state.app_state.llm.current_model();
    let (model_source, provider_source, base_url_source, thinking_source, show_thinking_source) =
        call_blocking(state.app_state.db.clone(), |db| {
            let settings = db.list_app_settings()?;
            let model_source = settings.iter().any(|s| {
                s.key
                    .eq_ignore_ascii_case(crate::llm_catalog::APP_SETTING_LLM_MODEL)
                    && !s.value.trim().is_empty()
            });
            let provider_source = settings.iter().any(|s| {
                s.key
                    .eq_ignore_ascii_case(crate::llm_catalog::APP_SETTING_LLM_PROVIDER)
                    && !s.value.trim().is_empty()
            });
            let base_url_source = settings.iter().any(|s| {
                s.key
                    .eq_ignore_ascii_case(crate::llm_catalog::APP_SETTING_LLM_BASE_URL)
                    && !s.value.trim().is_empty()
            });
            let thinking_source = settings.iter().any(|s| {
                s.key
                    .eq_ignore_ascii_case(crate::llm_catalog::APP_SETTING_LLM_THINKING_ENABLED)
                    && !s.value.trim().is_empty()
            });
            let show_thinking_source = settings.iter().any(|s| {
                s.key
                    .eq_ignore_ascii_case(crate::llm_catalog::APP_SETTING_SHOW_THINKING)
                    && !s.value.trim().is_empty()
            });
            Ok((
                model_source,
                provider_source,
                base_url_source,
                thinking_source,
                show_thinking_source,
            ))
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let is_local = crate::llm_catalog::is_local_provider(&provider_id);
    let default_base_url = if is_local {
        crate::llm_catalog::default_base_url_for_provider(&provider_id).map(|s| s.to_string())
    } else {
        None
    };
    let base_url = state.app_state.llm.current_base_url();

    let catalog_models = crate::llm_catalog::catalog_models_json(&provider_id, &current_model);
    let catalog: Vec<serde_json::Value> = catalog_models
        .iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect();

    let in_catalog = catalog_models
        .iter()
        .any(|m| m.id == current_model && !m.from_active_config);

    let providers: Vec<serde_json::Value> =
        crate::llm_catalog::providers_catalog_json(&provider_id, &current_model)
            .into_iter()
            .filter_map(|p| serde_json::to_value(p).ok())
            .collect();

    Ok(Json(json!({
        "ok": true,
        "provider": {
            "id": provider_id,
            "label": preset.map(|p| p.label).unwrap_or("Unknown provider"),
        },
        "provider_source": if provider_source { "app_settings" } else { "default" },
        "api_key_configured": crate::llm_catalog::is_api_key_configured_for_provider(&provider_id),
        "model": current_model,
        "model_in_catalog": in_catalog,
        "model_source": if model_source { "app_settings" } else { "default" },
        "is_local_provider": is_local,
        "base_url": base_url,
        "default_base_url": default_base_url,
        "base_url_source": if is_local {
            if base_url_source { "app_settings" } else { "default" }
        } else {
            "n/a"
        },
        "catalog": catalog,
        "providers": providers,
        "catalog_source": "static_curated",
        "cost_reference_note": "Approximate USD per 1M tokens for curated ids — verify on your provider billing page. Model lists are loaded live from the provider (GET /api/llm/models); this payload is the curated fallback. Put API keys in repo-root .env only.",
        "custom_model_allowed": true,
        "thinking_enabled": state.app_state.llm.thinking_enabled(),
        "thinking_source": if thinking_source {
            "app_settings"
        } else {
            "default"
        },
        "show_thinking": state.app_state.llm.show_thinking(),
        "show_thinking_source": if show_thinking_source {
            "app_settings"
        } else if std::env::var("SHOW_THINKING").is_ok() {
            "env"
        } else {
            "default"
        },
        "thinking_supported": provider_id == "google" || provider_id == "gemini",
    })))
}

async fn api_llm_models_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<LlmModelsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let provider_id = query
        .provider
        .as_deref()
        .map(crate::llm_catalog::resolve_catalog_provider_id)
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| state.app_state.llm.current_provider());
    if crate::llm_catalog::find_provider(&provider_id).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unknown provider {provider_id:?}"),
        ));
    }
    let current_provider = state.app_state.llm.current_provider();
    let current_model = if provider_id.eq_ignore_ascii_case(&current_provider) {
        state.app_state.llm.current_model()
    } else {
        String::new()
    };
    let is_local = crate::llm_catalog::is_local_provider(&provider_id);
    let base_url = if is_local {
        let raw = query
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_string)
            .or_else(|| {
                if provider_id.eq_ignore_ascii_case(&current_provider) {
                    state.app_state.llm.current_base_url()
                } else {
                    None
                }
            });
        raw.map(|u| crate::llm_catalog::normalize_local_base_url(&u, &provider_id))
    } else {
        None
    };
    if is_local && base_url.as_deref().unwrap_or("").is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "base_url is required for Ollama and llama.cpp providers".into(),
        ));
    }

    let api_key = crate::llm_catalog::resolve_api_key_for_provider_with_config(
        &provider_id,
        Some(&state.app_state.config),
    );
    let static_models = crate::llm_catalog::catalog_models_json(&provider_id, &current_model);

    let live_result =
        crate::llm::fetch_live_provider_models(&provider_id, &api_key, base_url.as_deref()).await;

    let (source, models, truncated, live_count, message) = match live_result {
        Ok(ids) => {
            let merged =
                crate::llm_catalog::merge_live_model_ids(&provider_id, &current_model, &ids);
            (
                "live",
                merged.models,
                merged.truncated,
                Some(merged.live_count),
                None,
            )
        }
        Err(err) => ("static_fallback", static_models, false, None, Some(err)),
    };

    let models_json: Vec<serde_json::Value> = models
        .iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect();

    Ok(Json(json!({
        "ok": true,
        "provider": provider_id,
        "source": source,
        "truncated": truncated,
        "live_count": live_count,
        "models": models_json,
        "base_url": base_url,
        "message": message,
    })))
}

async fn api_llm_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<LlmModelPatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let model = body.model.trim().to_string();
    if model.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "model is required".into()));
    }
    if model.len() > 256 {
        return Err((StatusCode::BAD_REQUEST, "model id is too long".into()));
    }
    let _ = body.custom;
    let provider_id = body
        .provider
        .as_deref()
        .map(crate::llm_catalog::resolve_catalog_provider_id)
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| state.app_state.llm.current_provider());
    // Live catalog ids are not in the static curated list; custom=true is not required.

    let (provider_saved, model_saved) = state
        .app_state
        .llm
        .apply_selection(
            provider_id.clone(),
            model.clone(),
            if crate::llm_catalog::is_local_provider(&provider_id) {
                Some(
                    body.base_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|u| !u.is_empty())
                        .ok_or((
                            StatusCode::BAD_REQUEST,
                            "base_url is required for Ollama and llama.cpp providers".into(),
                        ))?
                        .to_string(),
                )
            } else {
                None
            },
        )
        .map_err(|e| {
            if e.contains("No API key") || e.contains("base_url") {
                (StatusCode::BAD_REQUEST, e)
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e)
            }
        })?;

    let provider_db = provider_saved.clone();
    let model_db = model_saved.clone();
    let base_url_db = if crate::llm_catalog::is_local_provider(&provider_saved) {
        state
            .app_state
            .llm
            .current_base_url()
            .map(|url| crate::llm_catalog::normalize_local_base_url(&url, &provider_saved))
    } else {
        None
    };
    let base_url_response = base_url_db.clone();
    let thinking_enabled = body
        .thinking_enabled
        .unwrap_or_else(|| state.app_state.llm.thinking_enabled());
    let show_thinking = body
        .show_thinking
        .unwrap_or_else(|| state.app_state.llm.show_thinking());
    call_blocking(state.app_state.db.clone(), move |db| {
        db.set_app_setting(crate::llm_catalog::APP_SETTING_LLM_PROVIDER, &provider_db)?;
        db.set_app_setting(crate::llm_catalog::APP_SETTING_LLM_MODEL, &model_db)?;
        if let Some(ref url) = base_url_db {
            db.set_app_setting(crate::llm_catalog::APP_SETTING_LLM_BASE_URL, url)?;
        } else {
            db.set_app_setting(crate::llm_catalog::APP_SETTING_LLM_BASE_URL, "")?;
        }
        db.set_app_setting(
            crate::llm_catalog::APP_SETTING_LLM_THINKING_ENABLED,
            if thinking_enabled { "true" } else { "false" },
        )?;
        db.set_app_setting(
            crate::llm_catalog::APP_SETTING_SHOW_THINKING,
            if show_thinking { "true" } else { "false" },
        )?;
        Ok(())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .app_state
        .llm
        .apply_thinking_settings(thinking_enabled, show_thinking)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(json!({
        "ok": true,
        "provider": {
            "id": provider_saved,
        },
        "model": model_saved,
        "base_url": base_url_response,
        "thinking_enabled": thinking_enabled,
        "show_thinking": show_thinking,
        "provider_source": "app_settings",
        "model_source": "app_settings",
        "base_url_source": if base_url_response.is_some() {
            "app_settings"
        } else {
            "n/a"
        },
        "message": "Provider and model updated. New agent runs use this selection immediately.",
    })))
}

#[derive(Debug, Deserialize)]
struct MultimodelTestRequest {
    tier: String,
    base_url: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct MultimodelModelsQuery {
    base_url: String,
}

async fn api_multimodel_models_get(
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(query): Query<MultimodelModelsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let base_url_raw = query.base_url.trim();
    if base_url_raw.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "base_url query parameter is required".into(),
        ));
    }
    let base_url = crate::local_delegate::normalize_base_url_for_provider(base_url_raw, "");
    if base_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "base_url query parameter is required".into(),
        ));
    }
    match crate::llm::fetch_openai_compatible_models(&base_url).await {
        Ok(models) => Ok(Json(json!({
            "ok": true,
            "models": models,
            "base_url": base_url,
        }))),
        Err(e) => Err((StatusCode::BAD_GATEWAY, e)),
    }
}

async fn api_multimodel_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let cfg = state.app_state.llm.local_delegate_config();
    let strategy_provider =
        crate::llm_catalog::resolve_catalog_provider_id(&state.app_state.llm.current_provider());
    let strategy_model = state.app_state.llm.current_model();
    Ok(Json(json!({
        "ok": true,
        "routing_enabled": cfg.routing_enabled,
        "enabled": cfg.routing_enabled,
        "local_base_url": cfg.local_base_url,
        "local_model": cfg.local_model,
        "local_tools_ok": cfg.local_tools_ok,
        "tier1_base_url": cfg.tier1_base_url,
        "tier1_model": cfg.tier1_model,
        "tier2_base_url": cfg.tier2_base_url,
        "tier2_model": cfg.tier2_model,
        "tier1_tools_ok": cfg.tier1_tools_ok,
        "tier2_tools_ok": cfg.tier2_tools_ok,
        "strategy_provider": strategy_provider,
        "strategy_model": strategy_model,
        "description": "Local OpenAI-compatible endpoint for cost routing, PTE/PDQE, and deterministic local phases.",
    })))
}

async fn api_multimodel_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<crate::local_delegate::LocalDelegatePatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let old = state.app_state.llm.local_delegate_config();
    let mut cfg = old.clone();
    if let Some(routing) = body.routing_enabled.or(body.enabled) {
        cfg.routing_enabled = routing;
    }
    // New unified local fields
    if let Some(ref url) = body.local_base_url {
        cfg.local_base_url = url.trim().to_string();
    }
    if let Some(ref model) = body.local_model {
        cfg.local_model = model.trim().to_string();
    }
    // Legacy fields (accepted for backward compat)
    if let Some(ref url) = body.tier1_base_url {
        cfg.tier1_base_url = url.trim().to_string();
    }
    if let Some(ref model) = body.tier1_model {
        cfg.tier1_model = model.trim().to_string();
    }
    if let Some(ref url) = body.tier2_base_url {
        cfg.tier2_base_url = url.trim().to_string();
    }
    if let Some(ref model) = body.tier2_model {
        cfg.tier2_model = model.trim().to_string();
    }
    // Invalidate tools_ok if local config changed
    let local_changed = body.local_base_url.is_some() || body.local_model.is_some();
    if local_changed
        && (old.local_base_url != cfg.local_base_url || old.local_model != cfg.local_model)
    {
        cfg.local_tools_ok = false;
    }
    cfg = cfg.normalize();
    if cfg.routing_enabled && !cfg.local_configured() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Local model base URL and model are required to enable cost routing.".into(),
        ));
    }
    if cfg.routing_enabled && !cfg.local_tools_ok {
        return Err((
            StatusCode::BAD_REQUEST,
            "Run the tool-calling test for the local model before enabling cost routing.".into(),
        ));
    }
    call_blocking(state.app_state.db.clone(), {
        let cfg_db = cfg.clone();
        move |db| crate::local_delegate::persist_to_db(db, &cfg_db)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .app_state
        .llm
        .apply_local_delegate_config(cfg.clone())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(json!({
        "ok": true,
        "routing_enabled": cfg.routing_enabled,
        "enabled": cfg.routing_enabled,
        "local_base_url": cfg.local_base_url,
        "local_model": cfg.local_model,
        "local_tools_ok": cfg.local_tools_ok,
        "message": if cfg.routing_enabled {
            "Local delegate routing enabled."
        } else {
            "Local delegate routing disabled."
        },
    })))
}

async fn api_multimodel_test_post(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<MultimodelTestRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let tier = match body.tier.trim().to_ascii_lowercase().as_str() {
        "local" | "technical" | "tier1" | "1" | "knowledge" | "tier2" | "2" => {
            crate::local_delegate::ModelTier::LocalReadOnly
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unknown tier {:?}. Use 'local'.", other),
            ))
        }
    };
    let model = body.model.trim();
    let base_url_raw = body.base_url.trim();
    if model.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "model is required".into()));
    }
    if base_url_raw.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "base_url is required".into()));
    }
    let label = "local";
    let fallback_base = "";
    let base_url =
        crate::local_delegate::normalize_base_url_for_provider(base_url_raw, fallback_base);
    let base = state.app_state.config.clone();
    let mut test_cfg = base;
    test_cfg.llm_provider = "llama".into();
    test_cfg.api_key = String::new();
    test_cfg.model = model.to_string();
    test_cfg.llm_base_url = Some(base_url.clone());
    match crate::llm::test_model(&test_cfg, model).await {
        Ok(()) => {}
        Err(e) => return Err((StatusCode::BAD_GATEWAY, e)),
    }
    match crate::llm::test_local_delegate_tools(&test_cfg, model, tier).await {
        Ok(()) => {
            call_blocking(state.app_state.db.clone(), move |db| {
                crate::local_delegate::persist_tier_tools_ok(db, tier, true)
            })
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if let Ok(cfg) = call_blocking(state.app_state.db.clone(), |db| {
                crate::local_delegate::load_from_db(db)
            })
            .await
            {
                let _ = state.app_state.llm.apply_local_delegate_config(cfg);
            }
            Ok(Json(json!({
                "ok": true,
                "tier": label,
                "model": model,
                "base_url": base_url,
                "tools_ok": true,
                "message": format!("{label} tier reachable and tool-calling verified at {base_url}."),
            })))
        }
        Err(e) => Err((
            StatusCode::BAD_GATEWAY,
            format!("Server reachable but tool-calling probe failed: {e}"),
        )),
    }
}

fn cursor_engine_json(
    cfg: &crate::cursor_engine_config::CursorEngineSettings,
    health: &crate::cursor_engine_config::SidecarHealth,
    agent_engine: &str,
    sidecar_managed: bool,
    web_port: u16,
    web_enabled: bool,
) -> serde_json::Value {
    json!({
        "ok": true,
        "sdk_runner_url": cfg.sdk_runner_url,
        "sdk_model": cfg.sdk_model,
        "sdk_model_params": cfg.sdk_model_params,
        "sdk_runner_ok": cfg.sdk_runner_ok,
        "sidecar_reachable": health.reachable,
        "api_key_configured": health.api_key_configured,
        "engine_ready": cfg.engine_ready(health),
        "sidecar_managed": sidecar_managed,
        "agent_engine": agent_engine,
        "cli_path": cfg.cli_path,
        "cli_model": cfg.cli_model,
        "cli_runner_url": cfg.cli_runner_url,
        "cli_on_path": crate::cursor_engine_config::cli_on_path(&cfg.cli_path),
        "timeout_secs": cfg.timeout_secs,
        "tmux_enabled": cfg.tmux_enabled,
        "mcp_tools_enabled": cfg.mcp_tools_enabled,
        "mcp_expose_send_message": cfg.mcp_expose_send_message,
        "delegation_slim_prompt": cfg.delegation_slim_prompt,
        "delegation_resume_delta": cfg.delegation_resume_delta,
        "mcp_endpoint_url": crate::cursor_mcp_bridge::mcp_endpoint_url(web_port),
        "mcp_bridge_ready": web_enabled && cfg.mcp_tools_enabled,
        "install_steps": [
            "Set CURSOR_API_KEY in repo-root .env (Cursor Dashboard → Integrations)",
            "Restart the bot — it auto-creates a runtime venv and installs cursor-sdk + aiohttp",
            "Optional: CURSOR_SDK_AUTO_INSTALL=false to manage Python deps yourself",
        ],
        "sidecar_error": health.error,
    })
}

async fn api_cursor_engine_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let cfg = state
        .app_state
        .cursor_settings
        .read()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .clone();
    let health = crate::cursor_engine_config::probe_sidecar_health(&cfg.sdk_runner_url).await;
    let agent_engine = state.app_state.runtime_toggles.agent_engine().as_str();
    Ok(Json(cursor_engine_json(
        &cfg,
        &health,
        agent_engine,
        state.app_state.cursor_sidecar.managed_locally,
        state.app_state.config.web_port,
        state.app_state.config.web_enabled,
    )))
}

async fn api_cursor_engine_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<crate::cursor_engine_config::CursorEnginePatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let mut cfg = state
        .app_state
        .cursor_settings
        .read()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .clone();

    if let Some(ref url) = body.sdk_runner_url {
        let trimmed = url.trim().to_string();
        if !trimmed.is_empty() {
            crate::cursor_engine_config::validate_runner_url(&trimmed)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        }
        if trimmed != cfg.sdk_runner_url {
            cfg.sdk_runner_ok = false;
        }
        cfg.sdk_runner_url = trimmed;
    }
    if let Some(ref model) = body.sdk_model {
        cfg.sdk_model = model.trim().to_string();
    }
    if let Some(params) = body.sdk_model_params {
        cfg.sdk_model_params = params
            .into_iter()
            .filter(|p| !p.id.trim().is_empty() && !p.value.trim().is_empty())
            .collect();
    }
    if let Some(ref path) = body.cli_path {
        cfg.cli_path = path.trim().to_string();
    }
    if let Some(ref model) = body.cli_model {
        cfg.cli_model = model.trim().to_string();
    }
    if let Some(ref url) = body.cli_runner_url {
        let trimmed = url.trim().to_string();
        if !trimmed.is_empty() {
            crate::cursor_engine_config::validate_runner_url(&trimmed)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        }
        cfg.cli_runner_url = trimmed;
    }
    if let Some(secs) = body.timeout_secs {
        cfg.timeout_secs = secs.clamp(60, 86_400);
    }
    if let Some(enabled) = body.tmux_enabled {
        cfg.tmux_enabled = enabled;
    }
    if let Some(enabled) = body.mcp_tools_enabled {
        cfg.mcp_tools_enabled = enabled;
    }
    if let Some(enabled) = body.mcp_expose_send_message {
        cfg.mcp_expose_send_message = enabled;
    }
    if let Some(enabled) = body.delegation_slim_prompt {
        cfg.delegation_slim_prompt = enabled;
    }
    if let Some(enabled) = body.delegation_resume_delta {
        cfg.delegation_resume_delta = enabled;
    }

    if cfg.sdk_model.trim().is_empty() {
        cfg.sdk_model = crate::config::default_cursor_sdk_model();
    }
    if cfg.cli_path.trim().is_empty() {
        cfg.cli_path = crate::config::default_cursor_agent_cli_path();
    }

    call_blocking(state.app_state.db.clone(), {
        let cfg_db = cfg.clone();
        move |db| crate::cursor_engine_config::persist_to_db(db, &cfg_db)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let mut guard = state
            .app_state
            .cursor_settings
            .write()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        *guard = cfg.clone();
    }

    let health = crate::cursor_engine_config::probe_sidecar_health(&cfg.sdk_runner_url).await;
    let agent_engine = state.app_state.runtime_toggles.agent_engine().as_str();
    let mut out = cursor_engine_json(
        &cfg,
        &health,
        agent_engine,
        state.app_state.cursor_sidecar.managed_locally,
        state.app_state.config.web_port,
        state.app_state.config.web_enabled,
    );
    if let serde_json::Value::Object(ref mut map) = out {
        map.insert(
            "message".into(),
            serde_json::Value::String("Cursor settings saved.".into()),
        );
    }
    Ok(Json(out))
}

async fn api_cursor_engine_health_post(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let mut cfg = state
        .app_state
        .cursor_settings
        .read()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .clone();
    let health = crate::cursor_engine_config::probe_sidecar_health(&cfg.sdk_runner_url).await;

    let ok = health.reachable && health.api_key_configured;
    if ok {
        cfg.sdk_runner_ok = true;
        call_blocking(state.app_state.db.clone(), {
            let cfg_db = cfg.clone();
            move |db| crate::cursor_engine_config::persist_to_db(db, &cfg_db)
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if let Ok(mut guard) = state.app_state.cursor_settings.write() {
            *guard = cfg.clone();
        }
    }

    let message = if ok {
        "Sidecar reachable and CURSOR_API_KEY is configured on the host.".to_string()
    } else if !health.reachable {
        health
            .error
            .clone()
            .unwrap_or_else(|| "Sidecar is not reachable.".into())
    } else {
        "Sidecar is up but CURSOR_API_KEY is not set on the host.".into()
    };

    let agent_engine = state.app_state.runtime_toggles.agent_engine().as_str();
    let mut out = cursor_engine_json(
        &cfg,
        &health,
        agent_engine,
        state.app_state.cursor_sidecar.managed_locally,
        state.app_state.config.web_port,
        state.app_state.config.web_enabled,
    );
    if let serde_json::Value::Object(ref mut map) = out {
        map.insert("message".into(), serde_json::Value::String(message));
        map.insert("health_ok".into(), serde_json::Value::Bool(ok));
    }
    Ok(Json(out))
}

async fn api_cursor_mcp_post(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    State(state): State<WebState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    crate::cursor_mcp_bridge::handle_cursor_mcp(addr, State(state.app_state.clone()), headers, body)
        .await
}

async fn api_cursor_mcp_get(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
) -> axum::response::Response {
    crate::cursor_mcp_bridge::handle_cursor_mcp_get(addr, headers).await
}

async fn api_cursor_engine_models_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let cfg = state
        .app_state
        .cursor_settings
        .read()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .clone();
    match crate::cursor_engine_config::fetch_sidecar_model_catalog(&cfg.sdk_runner_url).await {
        Ok(models) => Ok(Json(json!({
            "ok": true,
            "models": models,
        }))),
        Err(e) => Err((StatusCode::BAD_GATEWAY, e)),
    }
}

async fn api_settings_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let settings = call_blocking(state.app_state.db.clone(), move |db| db.list_app_settings())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut items: Vec<serde_json::Value> = settings
        .into_iter()
        .filter(|s| !crate::config::is_llm_related_runtime_setting_key(&s.key))
        .filter(|s| {
            !s.key
                .eq_ignore_ascii_case(crate::runtime_toggles::APP_SETTING_TOOL_OUTPUT_DEBUG)
        })
        .filter(|s| {
            !s.key.eq_ignore_ascii_case(
                crate::runtime_toggles::APP_SETTING_POST_TOOL_EVALUATOR_ENABLED,
            )
        })
        .filter(|s| {
            !s.key.eq_ignore_ascii_case(
                crate::runtime_toggles::APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED,
            )
        })
        .filter(|s| {
            !matches!(
                s.key.trim().to_ascii_uppercase().as_str(),
                crate::channel_integration_config::APP_SETTING_BOT_USERNAME
                    | crate::channel_integration_config::APP_SETTING_ALLOWED_GROUPS
                    | crate::channel_integration_config::APP_SETTING_CONTROL_CHAT_IDS
                    | crate::channel_integration_config::APP_SETTING_DISCORD_ALLOWED_CHANNELS
                    | crate::channel_integration_config::APP_SETTING_WHATSAPP_PHONE_NUMBER_ID
                    | crate::channel_integration_config::APP_SETTING_WHATSAPP_VERIFY_TOKEN
                    | crate::channel_integration_config::APP_SETTING_WHATSAPP_WEBHOOK_PORT
                    | crate::channel_integration_config::APP_SETTING_CHANNEL_INTEGRATION_SEEDED
            )
        })
        .map(|s| {
            let secret = setting_is_secret(&s.key);
            json!({
                "key": s.key,
                "value": if secret { mask_setting_value(&s.value) } else { s.value.clone() },
                "raw_value": s.value,
                "is_secret": secret,
                "updated_at": s.updated_at,
                "source": "runtime_db",
            })
        })
        .collect();
    items.sort_by(|a, b| {
        let ak = a
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let bk = b
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        ak.cmp(&bk)
    });

    let channel_ready = call_blocking(state.app_state.db.clone(), |db| {
        Ok::<bool, crate::error::FinallyAValueBotError>(is_channel_ready(db))
    })
    .await
    .unwrap_or(false);
    let cfg = &state.app_state.config;
    let cursor_cfg = state
        .app_state
        .cursor_settings
        .read()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .clone();
    let cursor_engine_ready = cursor_cfg.sdk_configured() && cursor_cfg.sdk_runner_ok;
    let mm_cfg = state.app_state.llm.local_delegate_config();
    let agent_engine = state.app_state.runtime_toggles.agent_engine();
    let local_delegate_ready = mm_cfg.local_routable();
    Ok(Json(json!({
        "ok": true,
        "settings": items,
        "bootstrap": {
            "workspace_dir": cfg.workspace_dir,
            "web_enabled": cfg.web_enabled,
            "web_host": cfg.web_host,
            "web_port": cfg.web_port,
            "web_auth_token_set": cfg.web_auth_token.as_ref().is_some_and(|v| !v.trim().is_empty()),
        },
        "installation_status": {
            "llm_ready": is_llm_ready(cfg),
            "channel_ready": channel_ready,
            "cursor_engine_ready": cursor_engine_ready,
            "local_delegate_ready": local_delegate_ready,
            "agent_engine": agent_engine.as_str(),
            "cost_routing_effective": crate::local_delegate::cost_routing_effective(agent_engine, &mm_cfg),
            "web_enabled": cfg.web_enabled,
            // PATCH /api/settings is disabled; no in-app "pending restart" until we track real diffs.
            "requires_restart_for_env_changes": false,
            "runtime_env_merge_from_app_settings": true,
            "llm_model_from_app_settings": true,
            "tool_output_debug": state.app_state.runtime_toggles.tool_output_debug(),
            "post_tool_evaluator_enabled": state.app_state.runtime_toggles.post_tool_evaluator_enabled(),
            "response_quality_evaluator_enabled": state.app_state.runtime_toggles.response_quality_evaluator_enabled(),
            "terminal": web_terminal::capabilities_json(cfg),
        }
    })))
}

async fn api_settings_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(_): Json<SettingsPatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "Persisting generic key/value settings to SQLite is disabled. Configure the process via repo-root .env (or process environment). The app_settings table is legacy and not merged at startup.".to_string(),
    ))
}

/// Restarts the user-level gateway service installed via `finally_a_value_bot gateway install` (systemd user unit or launchd).
async fn api_restart_post(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err((
            StatusCode::NOT_IMPLEMENTED,
            "Gateway restart is only supported on Linux and macOS.".to_string(),
        ))
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if !crate::gateway::user_gateway_service_installed() {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "message": "Gateway service is not installed for this user. Run: finally_a_value_bot gateway install"
                })),
            ));
        }
        crate::gateway::schedule_user_gateway_restart();
        Ok((
            StatusCode::ACCEPTED,
            Json(json!({
                "ok": true,
                "message": "Gateway restart scheduled. The page may disconnect briefly."
            })),
        ))
    }
}

fn json_channel_bot_instance_redacted(inst: &ChannelBotInstance) -> serde_json::Value {
    let masked = mask_setting_value(&inst.token);
    json!({
        "id": inst.id,
        "platform": inst.platform,
        "label": inst.label,
        "token_set": !inst.token.trim().is_empty(),
        "token_redacted": masked,
        "bot_username": inst.bot_username,
        "allowed_groups": inst.allowed_groups.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        "discord_allowed_channels": inst.discord_allowed_channels.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        "whatsapp_phone_number_id": inst.whatsapp_phone_number_id,
        "whatsapp_verify_token_set": !inst.whatsapp_verify_token.trim().is_empty(),
        "whatsapp_verify_token_redacted": if inst.whatsapp_verify_token.trim().is_empty() {
            String::new()
        } else {
            mask_setting_value(&inst.whatsapp_verify_token)
        },
        "whatsapp_webhook_port": inst.whatsapp_webhook_port,
        "wecom_corp_id": inst.wecom_corp_id,
        "wecom_agent_id": inst.wecom_agent_id,
        "wecom_callback_token_set": !inst.wecom_callback_token.trim().is_empty(),
        "wecom_callback_token_redacted": if inst.wecom_callback_token.trim().is_empty() {
            String::new()
        } else {
            mask_setting_value(&inst.wecom_callback_token)
        },
        "wecom_encoding_aes_key_set": !inst.wecom_encoding_aes_key.trim().is_empty(),
        "wecom_encoding_aes_key_redacted": if inst.wecom_encoding_aes_key.trim().is_empty() {
            String::new()
        } else {
            mask_setting_value(&inst.wecom_encoding_aes_key)
        },
        "wecom_webhook_port": inst.wecom_webhook_port,
        "wecom_allowed_chats": inst.wecom_allowed_chats,
        "wecom_aibot_id": inst.wecom_aibot_id,
        "wecom_mode": inst.wecom_mode,
        "created_at": inst.created_at,
        "env_primary": false,
        "is_primary": matches!(
            inst.id,
            crate::db::BOT_INSTANCE_TELEGRAM_PRIMARY
                | crate::db::BOT_INSTANCE_DISCORD_PRIMARY
                | crate::db::BOT_INSTANCE_WHATSAPP_PRIMARY
                | crate::db::BOT_INSTANCE_WECOM_PRIMARY
        ),
    })
}

#[derive(Debug, Deserialize)]
struct ChannelBotInstanceCreateRequest {
    platform: String,
    label: String,
    token: String,
    #[serde(default)]
    bot_username: Option<String>,
    #[serde(default)]
    allowed_groups: Option<String>,
    #[serde(default)]
    discord_allowed_channels: Option<String>,
    #[serde(default)]
    whatsapp_phone_number_id: Option<String>,
    #[serde(default)]
    whatsapp_verify_token: Option<String>,
    #[serde(default)]
    whatsapp_webhook_port: Option<u16>,
    #[serde(default)]
    wecom_corp_id: Option<String>,
    #[serde(default)]
    wecom_agent_id: Option<i64>,
    #[serde(default)]
    wecom_callback_token: Option<String>,
    #[serde(default)]
    wecom_encoding_aes_key: Option<String>,
    #[serde(default)]
    wecom_webhook_port: Option<u16>,
    #[serde(default)]
    wecom_allowed_chats: Option<String>,
    #[serde(default)]
    wecom_aibot_id: Option<String>,
    #[serde(default)]
    wecom_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelBotInstanceUpdateRequest {
    label: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    bot_username: Option<String>,
    #[serde(default)]
    allowed_groups: Option<String>,
    #[serde(default)]
    discord_allowed_channels: Option<String>,
    #[serde(default)]
    whatsapp_phone_number_id: Option<String>,
    #[serde(default)]
    whatsapp_verify_token: Option<String>,
    #[serde(default)]
    whatsapp_webhook_port: Option<u16>,
    #[serde(default)]
    wecom_corp_id: Option<String>,
    #[serde(default)]
    wecom_agent_id: Option<i64>,
    #[serde(default)]
    wecom_callback_token: Option<String>,
    #[serde(default)]
    wecom_encoding_aes_key: Option<String>,
    #[serde(default)]
    wecom_webhook_port: Option<u16>,
    #[serde(default)]
    wecom_allowed_chats: Option<String>,
    #[serde(default)]
    wecom_aibot_id: Option<String>,
    #[serde(default)]
    wecom_mode: Option<String>,
}

fn parse_id_list_i64(raw: &str) -> Result<Vec<i64>, String> {
    let mut ids = Vec::new();
    let mut invalid = Vec::new();
    for part in raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        let token = token
            .strip_prefix("chat:")
            .or_else(|| token.strip_prefix("user:"))
            .unwrap_or(token)
            .trim_start_matches('+');
        match token.parse::<i64>() {
            Ok(id) => ids.push(id),
            Err(_) => invalid.push(token.to_string()),
        }
    }
    if !invalid.is_empty() {
        return Err(format!(
            "Invalid chat IDs (numeric Telegram IDs, comma-separated): {}",
            invalid.join(", ")
        ));
    }
    Ok(ids)
}

fn parse_id_list_u64(raw: &str) -> Result<Vec<u64>, String> {
    let mut ids = Vec::new();
    let mut invalid = Vec::new();
    for part in raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        match token.parse::<u64>() {
            Ok(id) => ids.push(id),
            Err(_) => invalid.push(token.to_string()),
        }
    }
    if !invalid.is_empty() {
        return Err(format!(
            "Invalid channel IDs (numeric Discord IDs, comma-separated): {}",
            invalid.join(", ")
        ));
    }
    Ok(ids)
}

fn looks_like_masked_secret(value: &str) -> bool {
    let v = value.trim();
    v.contains("***") || v == "***"
}

async fn api_channel_bot_instances_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let list = call_blocking(state.app_state.db.clone(), move |db| {
        db.list_all_channel_bot_instances()
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let items: Vec<serde_json::Value> = list
        .iter()
        .map(json_channel_bot_instance_redacted)
        .collect();
    Ok(Json(json!({ "ok": true, "instances": items })))
}

async fn api_channel_bot_instances_post(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<ChannelBotInstanceCreateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let platform = body.platform;
    let platform_for_provision = platform.clone();
    let label = body.label;
    let token = body.token;
    let bot_username = body.bot_username.map(|v| v.trim().to_string());
    let allowed_groups = body
        .allowed_groups
        .as_deref()
        .map(parse_id_list_i64)
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?
        .unwrap_or_default();
    let discord_allowed_channels = body
        .discord_allowed_channels
        .as_deref()
        .map(parse_id_list_u64)
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?
        .unwrap_or_default();
    let whatsapp_phone_number_id = body.whatsapp_phone_number_id.map(|v| v.trim().to_string());
    let whatsapp_verify_token = body.whatsapp_verify_token.map(|v| v.trim().to_string());
    let whatsapp_webhook_port = body.whatsapp_webhook_port;
    let wecom_corp_id = body.wecom_corp_id.map(|v| v.trim().to_string());
    let wecom_agent_id = body.wecom_agent_id;
    let wecom_callback_token = body.wecom_callback_token.map(|v| v.trim().to_string());
    let wecom_encoding_aes_key = body.wecom_encoding_aes_key.map(|v| v.trim().to_string());
    let wecom_webhook_port = body.wecom_webhook_port;
    let wecom_allowed_chats = body.wecom_allowed_chats.map(|v| v.trim().to_string());
    let wecom_aibot_id = body.wecom_aibot_id.map(|v| v.trim().to_string());
    let wecom_mode = body.wecom_mode.map(|v| v.trim().to_string());
    let db = state.app_state.db.clone();
    let id = call_blocking(db.clone(), move |db| {
        let id = db.create_channel_bot_instance(&platform, &label, &token)?;
        db.update_channel_bot_instance_options(
            id,
            bot_username.as_deref(),
            Some(&allowed_groups),
            Some(&discord_allowed_channels),
            whatsapp_phone_number_id.as_deref(),
            whatsapp_verify_token.as_deref(),
            whatsapp_webhook_port,
        )?;
        db.update_channel_bot_instance_wecom_options(
            id,
            wecom_corp_id.as_deref(),
            wecom_agent_id,
            wecom_callback_token.as_deref(),
            wecom_encoding_aes_key.as_deref(),
            wecom_webhook_port,
            wecom_allowed_chats.as_deref(),
            wecom_aibot_id.as_deref(),
            wecom_mode.as_deref(),
        )?;
        Ok(id)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let linked = call_blocking(db, move |db| {
        db.provision_bindings_for_instance(&platform_for_provision, id)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "id": id,
        "bindings_provisioned": linked,
        "message": "Bot instance created. Restart the process to run dispatchers for new instances. Existing contacts were auto-linked where possible."
    })))
}

async fn api_channel_bot_instances_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(id): Path<i64>,
    Json(body): Json<ChannelBotInstanceUpdateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let label = body.label;
    let token = body.token;
    let bot_username = body.bot_username.map(|v| v.trim().to_string());
    let allowed_groups = body
        .allowed_groups
        .as_deref()
        .map(parse_id_list_i64)
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let discord_allowed_channels = body
        .discord_allowed_channels
        .as_deref()
        .map(parse_id_list_u64)
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let whatsapp_phone_number_id = body.whatsapp_phone_number_id.map(|v| v.trim().to_string());
    let whatsapp_verify_token = body.whatsapp_verify_token.and_then(|v| {
        if looks_like_masked_secret(&v) {
            None
        } else {
            Some(v.trim().to_string())
        }
    });
    let whatsapp_webhook_port = body.whatsapp_webhook_port;
    let wecom_corp_id = body.wecom_corp_id.map(|v| v.trim().to_string());
    let wecom_agent_id = body.wecom_agent_id;
    let wecom_callback_token = body.wecom_callback_token.and_then(|v| {
        if looks_like_masked_secret(&v) {
            None
        } else {
            Some(v.trim().to_string())
        }
    });
    let wecom_encoding_aes_key = body.wecom_encoding_aes_key.and_then(|v| {
        if looks_like_masked_secret(&v) {
            None
        } else {
            Some(v.trim().to_string())
        }
    });
    let wecom_webhook_port = body.wecom_webhook_port;
    let wecom_allowed_chats = body.wecom_allowed_chats.map(|v| v.trim().to_string());
    let wecom_aibot_id = body.wecom_aibot_id.map(|v| v.trim().to_string());
    let wecom_mode = body.wecom_mode.map(|v| v.trim().to_string());
    let updated = call_blocking(state.app_state.db.clone(), move |db| {
        let Some(current) = db.get_channel_bot_instance(id)? else {
            return Ok(false);
        };
        let next_token = token
            .as_deref()
            .filter(|t| !looks_like_masked_secret(t))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(current.token.as_str());
        db.update_channel_bot_instance(id, &label, next_token)?;
        db.update_channel_bot_instance_options(
            id,
            bot_username.as_deref(),
            allowed_groups.as_deref(),
            discord_allowed_channels.as_deref(),
            whatsapp_phone_number_id.as_deref(),
            whatsapp_verify_token.as_deref(),
            whatsapp_webhook_port,
        )?;
        db.update_channel_bot_instance_wecom_options(
            id,
            wecom_corp_id.as_deref(),
            wecom_agent_id,
            wecom_callback_token.as_deref(),
            wecom_encoding_aes_key.as_deref(),
            wecom_webhook_port,
            wecom_allowed_chats.as_deref(),
            wecom_aibot_id.as_deref(),
            wecom_mode.as_deref(),
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, "Unknown bot instance id".into()));
    }
    Ok(Json(
        json!({ "ok": true, "message": "Updated. Restart the process to apply token changes to dispatchers." }),
    ))
}

async fn api_channel_bot_instances_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let deleted = call_blocking(state.app_state.db.clone(), move |db| {
        db.delete_channel_bot_instance(id)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "Not found or cannot delete".into()));
    }
    Ok(Json(json!({ "ok": true, "removed": true })))
}

fn json_channel_integration_response(
    settings: &crate::channel_integration_config::ChannelIntegrationSettings,
    instances: &[ChannelBotInstance],
) -> serde_json::Value {
    let tg = instances
        .iter()
        .find(|i| i.id == crate::db::BOT_INSTANCE_TELEGRAM_PRIMARY);
    let dc = instances
        .iter()
        .find(|i| i.id == crate::db::BOT_INSTANCE_DISCORD_PRIMARY);
    let wa = instances
        .iter()
        .find(|i| i.id == crate::db::BOT_INSTANCE_WHATSAPP_PRIMARY);
    json!({
        "ok": true,
        "bot_username": settings.bot_username,
        "allowed_groups": settings.allowed_groups.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        "control_chat_ids": settings.control_chat_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        "discord_allowed_channels": settings.discord_allowed_channels.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        "whatsapp_phone_number_id": settings.whatsapp_phone_number_id,
        "whatsapp_verify_token_set": !settings.whatsapp_verify_token.trim().is_empty(),
        "whatsapp_verify_token_redacted": if settings.whatsapp_verify_token.trim().is_empty() {
            String::new()
        } else {
            mask_setting_value(&settings.whatsapp_verify_token)
        },
        "whatsapp_webhook_port": settings.whatsapp_webhook_port,
        "telegram_token_set": tg.map(|i| !i.token.trim().is_empty()).unwrap_or(false),
        "telegram_token_redacted": tg.map(|i| mask_setting_value(&i.token)).unwrap_or_default(),
        "telegram_label": tg.map(|i| i.label.clone()).unwrap_or_else(|| "Primary Telegram".into()),
        "discord_token_set": dc.map(|i| !i.token.trim().is_empty()).unwrap_or(false),
        "discord_token_redacted": dc.map(|i| mask_setting_value(&i.token)).unwrap_or_default(),
        "discord_label": dc.map(|i| i.label.clone()).unwrap_or_else(|| "Primary Discord".into()),
        "whatsapp_access_token_set": wa.map(|i| !i.token.trim().is_empty()).unwrap_or(false),
        "whatsapp_access_token_redacted": wa.map(|i| mask_setting_value(&i.token)).unwrap_or_default(),
        "whatsapp_label": wa.map(|i| i.label.clone()).unwrap_or_else(|| "Primary WhatsApp".into()),
        "instances": instances.iter().map(json_channel_bot_instance_redacted).collect::<Vec<_>>(),
        "requires_restart": true,
    })
}

async fn api_channels_integration_get(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let config = state.app_state.config.clone();
    let (settings, instances) = call_blocking(state.app_state.db.clone(), move |db| {
        let settings = crate::channel_integration_config::load_from_db(db, &config)?;
        let instances = db.list_all_channel_bot_instances()?;
        Ok::<_, crate::error::FinallyAValueBotError>((settings, instances))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json_channel_integration_response(
        &settings, &instances,
    )))
}

async fn api_channels_integration_patch(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<crate::channel_integration_config::ChannelIntegrationPatch>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let config = state.app_state.config.clone();
    let (settings, instances) = call_blocking(state.app_state.db.clone(), move |db| {
        let settings = body.apply_and_save(db, &config)?;
        let instances = db.list_all_channel_bot_instances()?;
        Ok::<_, crate::error::FinallyAValueBotError>((settings, instances))
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut resp = json_channel_integration_response(&settings, &instances);
    if let Some(obj) = resp.as_object_mut() {
        obj.insert(
            "message".into(),
            json!("Saved. Restart the gateway to apply token and dispatcher changes."),
        );
    }
    Ok(Json(resp))
}

async fn api_channel_persona_policy_upsert(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<ChannelPersonaPolicyUpsertRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let mode = match body.mode.as_str() {
        "all" => ChannelPersonaMode::All,
        "single" => ChannelPersonaMode::Single,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "mode must be 'all' or 'single'".to_string(),
            ))
        }
    };
    let persona_id = body.persona_id.filter(|id| *id > 0);
    let bot_instance_id = body.bot_instance_id;
    call_blocking(state.app_state.db.clone(), move |db| {
        db.set_channel_persona_policy(chat_id, bot_instance_id, mode, persona_id)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "chat_id": chat_id,
        "bot_instance_id": bot_instance_id,
        "mode": body.mode,
        "persona_id": persona_id,
    })))
}

async fn api_channel_persona_policy_delete(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<ChannelPersonaPolicyDeleteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_auth(&headers, state.auth_token.as_deref())?;
    let chat_id = resolve_chat_id_for_web(body.chat_id, &state.app_state.config)?;
    ensure_web_binding_for_universal(&state, chat_id).await?;
    let removed = call_blocking(state.app_state.db.clone(), move |db| {
        db.clear_channel_persona_policy(chat_id, body.bot_instance_id)
    })
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "removed": removed,
    })))
}

async fn api_terminal_sessions_post(
    headers: HeaderMap,
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    web_terminal::require_terminal_api_auth(&headers, &state.app_state.config)?;
    let body = state
        .terminal_hub
        .create_session(&state.app_state.config)
        .await?;
    Ok(Json(body))
}

async fn api_terminal_ws(ws: WebSocketUpgrade, State(state): State<WebState>) -> impl IntoResponse {
    let hub = state.terminal_hub.clone();
    let config = Arc::new(state.app_state.config.clone());
    ws.on_upgrade(move |socket: WebSocket| async move {
        web_terminal::handle_websocket(socket, hub, config).await;
    })
}

pub async fn start_web_server(state: Arc<AppState>) {
    let limits = WebLimits::from_config(&state.config);
    let web_state = WebState {
        auth_token: state.config.web_auth_token.clone(),
        app_state: state.clone(),
        run_hub: RunHub::default(),
        request_hub: RequestHub::default(),
        terminal_hub: TerminalHub::default(),
        limits,
        web_binding_universal_done: Arc::new(Mutex::new(None)),
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
    if let Err(e) = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    {
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

async fn upload_file(
    State(state): State<WebState>,
    Path(path): Path<String>,
    Query(query): Query<UploadQuery>,
) -> impl IntoResponse {
    let clean = path.replace("..", "");
    if clean.is_empty() {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }
    let legacy_path = FsPath::new(state.app_state.config.working_dir())
        .join("uploads")
        .join(clean);
    let shared_path = state
        .app_state
        .config
        .workspace_root_absolute()
        .join("shared")
        .join("upload")
        .join(path.replace("..", ""));

    let full_path = if shared_path.is_file() {
        shared_path
    } else if legacy_path.is_file() {
        legacy_path
    } else {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    };

    match tokio::fs::read(&full_path).await {
        Ok(bytes) => {
            let content_type = guess_upload_content_type(&full_path);
            let mut headers = HeaderMap::new();
            let _ = headers.insert("content-type", HeaderValue::from_static(content_type));
            let _ = headers.insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );

            let force_download = query.download.unwrap_or(false);
            let is_html = content_type == "text/html; charset=utf-8";
            let allow_inline = content_type.starts_with("image/")
                || content_type.starts_with("text/")
                || content_type == "application/json"
                || content_type == "application/pdf";

            if is_html {
                if query.preview.unwrap_or(false) {
                    let _ = headers.insert(
                        "content-security-policy",
                        HeaderValue::from_static(
                            "default-src 'none'; img-src data: blob: https: http:; style-src 'unsafe-inline'; sandbox",
                        ),
                    );
                } else {
                    let _ = headers.insert(
                        "content-disposition",
                        HeaderValue::from_static("attachment"),
                    );
                }
            } else if force_download || !allow_inline {
                let _ = headers.insert(
                    "content-disposition",
                    HeaderValue::from_static("attachment"),
                );
            }

            (headers, bytes).into_response()
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
        Some("md") | Some("markdown") => "text/markdown; charset=utf-8",
        Some("html") | Some("htm") => "text/html; charset=utf-8",
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
        .is_none_or(|s| !s.is_platform_enabled(&platform))
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
                html_escape::encode_text(&e.to_string())
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
    let json_limit = web_json_body_limit_bytes(&web_state.app_state.config);
    let multipart_limit = web_multipart_body_limit_bytes(&web_state.app_state.config);
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
            "/api/settings",
            get(api_settings_get).patch(api_settings_patch),
        )
        .route("/api/llm", get(api_llm_get).patch(api_llm_patch))
        .route("/api/llm/models", get(api_llm_models_get))
        .route(
            "/api/multimodel",
            get(api_multimodel_get).patch(api_multimodel_patch),
        )
        .route("/api/multimodel/test", post(api_multimodel_test_post))
        .route("/api/multimodel/models", get(api_multimodel_models_get))
        .route(
            "/api/cursor-engine",
            get(api_cursor_engine_get).patch(api_cursor_engine_patch),
        )
        .route(
            "/api/cursor-engine/health",
            post(api_cursor_engine_health_post),
        )
        .route(
            "/api/cursor-engine/models",
            get(api_cursor_engine_models_get),
        )
        .route(
            "/internal/cursor-mcp",
            get(api_cursor_mcp_get).post(api_cursor_mcp_post),
        )
        .route(
            "/api/runtime",
            get(api_runtime_get).patch(api_runtime_patch),
        )
        .route("/api/terminal/sessions", post(api_terminal_sessions_post))
        .route("/api/terminal/ws", get(api_terminal_ws))
        .route(
            "/api/deterministic-pipeline",
            get(api_deterministic_pipeline_get).patch(api_deterministic_pipeline_patch),
        )
        .route("/api/restart", post(api_restart_post))
        .route(
            "/api/channels/integration",
            get(api_channels_integration_get).patch(api_channels_integration_patch),
        )
        .route(
            "/api/channel_bot_instances",
            get(api_channel_bot_instances_get).post(api_channel_bot_instances_post),
        )
        .route(
            "/api/channel_bot_instances/:id",
            patch(api_channel_bot_instances_patch).delete(api_channel_bot_instances_delete),
        )
        .route(
            "/api/channel_persona_policy",
            post(api_channel_persona_policy_upsert).delete(api_channel_persona_policy_delete),
        )
        .route(
            "/api/schedules",
            get(api_schedules_list).post(api_schedules_create),
        )
        .route("/api/schedules/:id", patch(api_schedules_update))
        .route("/api/todos", get(api_todos_list))
        .route("/api/todos/:id", patch(api_todos_patch))
        .route("/api/background_jobs", get(api_background_jobs_list))
        .route("/api/background_jobs/:job_id", get(api_background_job_get))
        .route(
            "/api/background_jobs/cancel",
            post(api_background_job_cancel),
        )
        .route("/api/history", get(api_history))
        .route("/api/history/days", get(api_history_days))
        .route("/api/artifacts", get(api_artifacts))
        .route(
            "/api/uploads",
            post(api_upload).layer(DefaultBodyLimit::max(multipart_limit)),
        )
        .route("/api/send", post(api_send))
        .route(
            "/api/send_stream",
            post(api_send_stream).layer(DefaultBodyLimit::max(json_limit)),
        )
        .route("/api/stream", get(api_stream))
        .route("/api/run_status", get(api_run_status))
        .route("/api/queue_diagnostics", get(api_queue_diagnostics))
        .route("/api/ops_poll", get(api_ops_poll))
        .route("/api/queue/cancel", post(api_queue_cancel))
        .route("/api/queue/remove", post(api_queue_remove))
        .route("/api/reset", post(api_reset))
        .route("/api/delete_session", post(api_delete_session))
        .route("/api/personas", get(api_personas))
        .route("/api/personas/switch", post(api_personas_switch))
        .route("/api/personas/create", post(api_personas_create))
        .route("/api/personas/delete", post(api_personas_delete))
        .route(
            "/api/chat_sessions",
            get(api_chat_sessions_list).post(api_chat_sessions_create),
        )
        .route(
            "/api/chat_sessions/:session_id",
            get(api_chat_sessions_get)
                .patch(api_chat_sessions_patch)
                .delete(api_chat_sessions_delete),
        )
        .route("/api/skills", get(api_skills_get))
        .route("/api/hooks", get(api_hooks_get).post(api_hooks_post))
        .route("/api/hooks/:id", delete(api_hooks_delete))
        .route(
            "/api/personas/:persona_id/bulletin",
            get(api_persona_bulletin_get).patch(api_persona_bulletin_patch),
        )
        .route(
            "/api/personas/:persona_id/policy",
            get(api_persona_policy_get).patch(api_persona_policy_patch),
        )
        .route(
            "/api/personas/:persona_id/bookmarks",
            get(api_persona_bookmarks_get).post(api_persona_bookmarks_post),
        )
        .route(
            "/api/personas/:persona_id/bookmarks/:message_id",
            delete(api_persona_bookmarks_delete),
        )
        .route(
            "/api/personas/:persona_id/messages/:message_id",
            get(api_persona_message_get).delete(api_persona_message_delete),
        )
        .route(
            "/api/personas/:persona_id/memory",
            get(api_persona_memory_get).put(api_persona_memory_put),
        )
        .route(
            "/api/personas/:persona_id/agent_history/latest",
            get(api_persona_agent_history_latest),
        )
        .route(
            "/api/personas/:persona_id/agent_history/latest/optimize",
            post(api_persona_agent_history_optimize),
        )
        .route(
            "/api/workspace/agents_md",
            get(api_workspace_agents_md_get).put(api_workspace_agents_md_put),
        )
        .route("/api/oauth/authorize/:platform", get(api_oauth_authorize))
        .route("/api/oauth/callback/:platform", get(api_oauth_callback))
        .layer(DefaultBodyLimit::max(json_limit))
        .with_state(web_state)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn test_ops_poll_include_personas_query() {
        assert!(ops_poll_include_personas(None));
        assert!(ops_poll_include_personas(Some("1")));
        assert!(ops_poll_include_personas(Some("true")));
        assert!(!ops_poll_include_personas(Some("0")));
        assert!(!ops_poll_include_personas(Some("false")));
        assert!(!ops_poll_include_personas(Some("OFF")));
    }

    #[test]
    fn test_background_job_result_preview_utf8_boundary() {
        // Em dash is 3 bytes; a raw `&t[..200]` panic sits inside it when byte 199 is mid-char.
        let prefix = "a".repeat(199);
        let text = format!("{prefix}—more");
        assert!(text.is_char_boundary(0));
        assert!(!text.is_char_boundary(200));
        let preview = background_job_result_preview(&text);
        assert!(preview.len() < 200);
        assert!(preview.ends_with('a'));
        assert!(!preview.contains('—'));
    }

    #[test]
    fn test_artifact_kind_from_filename() {
        assert_eq!(artifact_kind_from_filename("report.md"), "markdown");
        assert_eq!(artifact_kind_from_filename("image.png"), "image");
        assert_eq!(artifact_kind_from_filename("index.html"), "html");
        assert_eq!(artifact_kind_from_filename("notes.txt"), "text");
        assert_eq!(artifact_kind_from_filename("archive.zip"), "other");
    }

    #[test]
    fn test_extract_upload_urls_from_text() {
        let text = "See [/api/uploads/web/1/file.md](/api/uploads/web/1/file.md) and ![](/api/uploads/web/1/img.png)";
        let urls = extract_upload_urls_from_text(text);
        assert!(urls.iter().any(|u| u == "/api/uploads/web/1/file.md"));
        assert!(urls.iter().any(|u| u == "/api/uploads/web/1/img.png"));
    }

    #[tokio::test]
    async fn test_materialize_response_file_links_rewrites_file_url_target() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = 997894126_i64;
        let persona_id = 3_i64;
        let workspace_root = web_state.app_state.config.workspace_root_absolute();
        let spec = workspace_root
            .join("shared")
            .join("personas")
            .join(chat_id.to_string())
            .join(persona_id.to_string())
            .join("ORIGIN/Projects/GN-dComm-Integration-Spec.md");
        std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
        std::fs::write(&spec, b"# spec").unwrap();

        let file_url = format!("file://{}", spec.display());
        let input = format!("[GN Spec]({file_url})");
        let output = materialize_response_file_links(&web_state, chat_id, persona_id, &input)
            .await
            .unwrap();
        assert!(output.contains("/api/uploads/web/997894126/3/"));
        assert!(!output.contains("file://"));
        let urls = extract_upload_urls_from_text(&output);
        assert_eq!(urls.len(), 1);
        assert!(upload_rel_url_exists(&web_state, &urls[0]));
    }

    #[tokio::test]
    async fn test_materialize_response_file_links_rewrites_local_markdown_target() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = 997894126_i64;
        let workspace_root = web_state.app_state.config.workspace_root_absolute();
        let shared_dir = workspace_root.join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();
        let local = shared_dir.join("report.pdf");
        std::fs::write(&local, b"pdf").unwrap();

        let input = "[Download](shared/report.pdf)";
        let output = materialize_response_file_links(&web_state, chat_id, 1, input)
            .await
            .unwrap();
        assert!(output.contains("/api/uploads/web/997894126/"));
        assert!(!output.contains("(shared/report.pdf)"));

        let urls = extract_upload_urls_from_text(&output);
        assert_eq!(urls.len(), 1);
        assert!(upload_rel_url_exists(&web_state, &urls[0]));
    }

    #[tokio::test]
    async fn test_materialize_response_file_links_repairs_missing_upload_url() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = 997894126_i64;
        let workspace_root = web_state.app_state.config.workspace_root_absolute();
        let shared_dir = workspace_root.join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::write(shared_dir.join("report.pdf"), b"pdf").unwrap();

        let input = "[Download](/api/uploads/web/997894126/report.pdf)";
        let output = materialize_response_file_links(&web_state, chat_id, 1, input)
            .await
            .unwrap();
        assert!(output.contains("/api/uploads/web/997894126/"));
        assert_ne!(input, output);

        let urls = extract_upload_urls_from_text(&output);
        assert_eq!(urls.len(), 1);
        assert!(upload_rel_url_exists(&web_state, &urls[0]));
    }

    #[tokio::test]
    async fn test_materialize_repairs_fabricated_bot_copy_upload_url() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = 997894126_i64;
        let persona_id = 24_i64;
        let workspace_root = web_state.app_state.config.workspace_root_absolute();
        let persona_dir = workspace_root
            .join("shared")
            .join("personas")
            .join(chat_id.to_string())
            .join(persona_id.to_string());
        std::fs::create_dir_all(&persona_dir).unwrap();
        std::fs::write(
            persona_dir.join("PZ-20260608-PARK-HOTIFY-MEDIUM.png"),
            [137u8, 80, 78, 71, 13, 10, 26, 10],
        )
        .unwrap();

        let input = "![preview](/api/uploads/web/997894126/24/20260604-045735-bot-PZ-20260608-PARK-HOTIFY-MEDIUM.png)";
        let output = materialize_response_file_links(&web_state, chat_id, persona_id, input)
            .await
            .unwrap();
        assert_ne!(input, output);
        let urls = extract_upload_urls_from_text(&output);
        assert_eq!(urls.len(), 1);
        assert!(upload_rel_url_exists(&web_state, &urls[0]));
    }

    #[tokio::test]
    async fn test_materialize_after_bare_normalize_persona_scoped_image() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = 997894126_i64;
        let persona_id = 24_i64;
        let workspace_root = web_state.app_state.config.workspace_root_absolute();
        let persona_dir = workspace_root
            .join("shared")
            .join("personas")
            .join(chat_id.to_string())
            .join(persona_id.to_string());
        std::fs::create_dir_all(&persona_dir).unwrap();
        let img = persona_dir.join("PZ-foo.png");
        std::fs::write(&img, [137u8, 80, 78, 71, 13, 10, 26, 10]).unwrap();

        let raw = "Preview:\n\nPZ-foo.png\n\nOK?";
        let normalized = crate::final_delivery_media::normalize_assistant_artifact_references(
            &raw,
            &workspace_root,
            chat_id,
            persona_id,
        );
        let output = materialize_response_file_links(&web_state, chat_id, persona_id, &normalized)
            .await
            .unwrap();
        assert!(output.contains("/api/uploads/web/997894126/24/"));
        assert!(output.contains("![PZ-foo.png]"));
        let urls = extract_upload_urls_from_text(&output);
        assert_eq!(urls.len(), 1);
        assert!(upload_rel_url_exists(&web_state, &urls[0]));
    }

    #[tokio::test]
    async fn test_materialize_response_file_links_rewrites_parenthesized_local_path() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = 997894126_i64;
        let workspace_root = web_state.app_state.config.workspace_root_absolute();
        let shared_dir = workspace_root.join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::write(shared_dir.join("report.pdf"), b"pdf").unwrap();

        let input = "(shared/report.pdf)";
        let output = materialize_response_file_links(&web_state, chat_id, 1, input)
            .await
            .unwrap();
        assert!(output.contains("/api/uploads/web/997894126/"));
        assert!(!output.contains("(shared/report.pdf)"));

        let urls = extract_upload_urls_from_text(&output);
        assert_eq!(urls.len(), 1);
        assert!(upload_rel_url_exists(&web_state, &urls[0]));
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

    #[allow(dead_code)]
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

    fn test_llm_from_provider(provider: Arc<dyn LlmProvider>) -> Arc<crate::llm::LlmHandle> {
        let cfg = crate::config::test_config();
        crate::llm::LlmHandle::from_provider(&cfg, provider)
    }

    fn test_state(llm: Arc<crate::llm::LlmHandle>) -> Arc<AppState> {
        let mut cfg = crate::config::test_config();
        cfg.web_port = 3900;
        cfg.universal_chat_id = Some(997894126);
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
        let mut telegram_bots = std::collections::HashMap::new();
        telegram_bots.insert(crate::db::BOT_INSTANCE_TELEGRAM_PRIMARY, bot.clone());
        let runtime_toggles = crate::runtime_toggles::RuntimeToggles::new(cfg.tool_output_debug);
        let env_redactor =
            std::sync::Arc::new(crate::safety_redaction::EnvSecretRedactor::discover(&cfg));
        let cursor_settings = Arc::new(std::sync::RwLock::new(
            crate::cursor_engine_config::CursorEngineSettings::from_env(&cfg),
        ));
        let pipeline_profile = Arc::new(std::sync::RwLock::new(
            crate::agent_pipeline::profile::PipelineProfile::default_profile(),
        ));
        let cursor_sidecar = crate::cursor_sdk_sidecar::SidecarHandle::inactive();
        let steel_browser = crate::steel_browser_sidecar::SteelBrowserHandle::inactive();
        let state = AppState {
            config: cfg.clone(),
            env_redactor: env_redactor.clone(),
            runtime_toggles: runtime_toggles.clone(),
            cursor_settings,
            pipeline_profile,
            cursor_sidecar,
            steel_browser,
            telegram_bots: Arc::new(telegram_bots),
            db: db.clone(),
            memory: MemoryManager::new(&runtime_dir, cfg.working_dir()),
            skills: SkillManager::from_skills_dirs(cfg.skill_discovery_dirs()),
            llm,
            tools: ToolRegistry::new(&cfg, bot, db, runtime_toggles, env_redactor),
            cursor_mcp: Arc::new(crate::cursor_mcp_bridge::CursorMcpRegistry::new()),
            discord_http: Arc::new(std::collections::HashMap::new()),
            wecom: None,
            chat_queue: crate::chat_queue::ChatRunQueue::default(),
            background_job_control: crate::background_jobs::BackgroundJobControl::default(),
        };
        Arc::new(state)
    }

    fn test_web_state(
        llm: Arc<crate::llm::LlmHandle>,
        auth_token: Option<String>,
        limits: WebLimits,
    ) -> WebState {
        let state = test_state(llm);
        WebState {
            app_state: state,
            auth_token,
            run_hub: RunHub::default(),
            request_hub: RequestHub::default(),
            terminal_hub: TerminalHub::default(),
            limits,
            web_binding_universal_done: Arc::new(Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn test_api_hooks_get_persona_fields() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = web_state
            .app_state
            .config
            .universal_chat_id
            .unwrap_or(997894126_i64);
        let db = web_state.app_state.db.clone();

        let owner_persona_id = db
            .create_persona(chat_id, "owner", None)
            .expect("create owner persona");
        let other_persona_id = db
            .create_persona(chat_id, "other", None)
            .expect("create other persona");

        let global_hook_id = db
            .upsert_hook_definition(
                None,
                "global-hook",
                "BeforeTurn",
                None,
                "add_context",
                r#"{"additional_context":"global"}"#,
                None,
                true,
            )
            .expect("upsert global hook");
        let scoped_hook_id = db
            .upsert_hook_definition(
                None,
                "scoped-hook",
                "BeforeTurn",
                None,
                "add_context",
                r#"{"additional_context":"scoped"}"#,
                Some(&[owner_persona_id]),
                true,
            )
            .expect("upsert scoped hook");
        let disabled_hook_id = db
            .upsert_hook_definition(
                None,
                "disabled-hook",
                "BeforeTurn",
                None,
                "add_context",
                r#"{"additional_context":"disabled"}"#,
                None,
                false,
            )
            .expect("upsert disabled hook");

        let res_owner = api_hooks_get(
            HeaderMap::new(),
            State(web_state.clone()),
            Query(HooksQuery {
                persona_id: Some(owner_persona_id),
            }),
        )
        .await;
        let body_owner = res_owner.expect("owner ok").0;
        let hooks_owner = body_owner
            .get("hooks")
            .and_then(|v| v.as_array())
            .expect("hooks array");

        let find_hook = |id: i64| {
            hooks_owner
                .iter()
                .find(|h| h.get("id").and_then(|v| v.as_i64()) == Some(id))
                .expect("hook exists")
        };

        let global_hook = find_hook(global_hook_id);
        assert_eq!(
            global_hook
                .get("scoped_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            global_hook
                .get("allowed_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            global_hook
                .get("active_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        let scoped_hook = find_hook(scoped_hook_id);
        assert_eq!(
            scoped_hook
                .get("scoped_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            scoped_hook
                .get("allowed_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            scoped_hook
                .get("active_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        let disabled_hook = find_hook(disabled_hook_id);
        assert_eq!(
            disabled_hook
                .get("active_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );

        let res_other = api_hooks_get(
            HeaderMap::new(),
            State(web_state.clone()),
            Query(HooksQuery {
                persona_id: Some(other_persona_id),
            }),
        )
        .await;
        let body_other = res_other.expect("other ok").0;
        let hooks_other = body_other
            .get("hooks")
            .and_then(|v| v.as_array())
            .expect("hooks array");
        let find_hook_other = |id: i64| {
            hooks_other
                .iter()
                .find(|h| h.get("id").and_then(|v| v.as_i64()) == Some(id))
                .expect("hook exists")
        };

        let scoped_hook_other = find_hook_other(scoped_hook_id);
        assert_eq!(
            scoped_hook_other
                .get("scoped_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            scoped_hook_other
                .get("allowed_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            scoped_hook_other
                .get("active_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );

        let res_missing = api_hooks_get(
            HeaderMap::new(),
            State(web_state.clone()),
            Query(HooksQuery {
                persona_id: Some(99999),
            }),
        )
        .await;
        assert!(
            matches!(res_missing, Err((StatusCode::NOT_FOUND, _))),
            "expected 404 for missing persona"
        );

        db.set_persona_hook_skill_policy(chat_id, owner_persona_id, Some(&[]), None)
            .expect("block all hooks for owner");
        let res_owner_blocked = api_hooks_get(
            HeaderMap::new(),
            State(web_state.clone()),
            Query(HooksQuery {
                persona_id: Some(owner_persona_id),
            }),
        )
        .await;
        let body_owner_blocked = res_owner_blocked.expect("owner ok").0;
        let hooks_owner_blocked = body_owner_blocked
            .get("hooks")
            .and_then(|v| v.as_array())
            .expect("hooks array");
        let find_hook_blocked = |id: i64| {
            hooks_owner_blocked
                .iter()
                .find(|h| h.get("id").and_then(|v| v.as_i64()) == Some(id))
                .expect("hook exists")
        };

        let global_hook_blocked = find_hook_blocked(global_hook_id);
        assert_eq!(
            global_hook_blocked
                .get("scoped_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            global_hook_blocked
                .get("allowed_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            global_hook_blocked
                .get("active_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );

        let scoped_hook_blocked = find_hook_blocked(scoped_hook_id);
        assert_eq!(
            scoped_hook_blocked
                .get("scoped_for_persona")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            scoped_hook_blocked
                .get("allowed_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            scoped_hook_blocked
                .get("active_for_persona")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[tokio::test]
    async fn test_multipart_upload_then_send_with_ref() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
        let chat_id = web_state
            .app_state
            .config
            .universal_chat_id
            .unwrap_or(997894126);
        let app = build_router(web_state);

        let boundary = "----testboundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.png\"\r\nContent-Type: image/png\r\n\r\nPNGDATA\r\n--{boundary}--\r\n"
        );
        let upload_req = Request::builder()
            .method("POST")
            .uri(format!("/api/uploads?chat_id={chat_id}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let upload_resp = app.clone().oneshot(upload_req).await.unwrap();
        assert_eq!(upload_resp.status(), StatusCode::OK);
        let upload_bytes = axum::body::to_bytes(upload_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let upload_json: serde_json::Value = serde_json::from_slice(&upload_bytes).unwrap();
        let tool_path = upload_json
            .get("tool_path")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        let url = upload_json
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();

        let send_body = json!({
            "chat_id": chat_id,
            "sender_name": "u",
            "message": "see image",
            "attachments": [{
                "filename": "photo.png",
                "media_type": "image/png",
                "tool_path": tool_path,
                "url": url,
            }]
        });
        let send_req = Request::builder()
            .method("POST")
            .uri("/api/send_stream")
            .header("content-type", "application/json")
            .body(Body::from(send_body.to_string()))
            .unwrap();
        let send_resp = app.oneshot(send_req).await.unwrap();
        assert_eq!(send_resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_web_body_limit_helpers() {
        let mut cfg = crate::config::test_config();
        cfg.max_document_size_mb = 10;
        assert!(web_json_body_limit_bytes(&cfg) > 10 * 1024 * 1024);
        assert!(web_multipart_body_limit_bytes(&cfg) >= 10 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_send_stream_then_stream_done() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
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
        assert!(
            text.contains("event: done"),
            "stream should end with done; body was: {text}"
        );
        assert!(
            text.contains("event: delta") || text.contains("event: replay_meta"),
            "stream should include deltas or replay metadata; body was: {text}"
        );
    }

    #[tokio::test]
    async fn test_slash_command_via_send_stream_returns_done_with_response() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
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
            test_llm_from_provider(Arc::new(DummyLlm)),
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
        // `/api/send` releases the per-session slot as soon as the work is queued, so inflight
        // does not stay held for the LLM. Rate limiting uses `max_requests_per_window` instead.
        let limits = WebLimits {
            max_inflight_per_session: 2,
            max_requests_per_window: 1,
            rate_window: Duration::from_secs(10),
            run_history_limit: 128,
            session_idle_ttl: Duration::from_secs(60),
        };
        let web_state = test_web_state(test_llm_from_provider(Arc::new(DummyLlm)), None, limits);
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

        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let resp2 = app.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_stream_includes_tool_events_and_replay() {
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(ToolFlowLlm {
                calls: AtomicUsize::new(0),
            })),
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
        let web_state = test_web_state(
            test_llm_from_provider(Arc::new(DummyLlm)),
            None,
            WebLimits::default(),
        );
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
        let web_state = test_web_state(test_llm_from_provider(Arc::new(DummyLlm)), None, limits);
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
        let state = test_state(test_llm_from_provider(Arc::new(DummyLlm)));
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
