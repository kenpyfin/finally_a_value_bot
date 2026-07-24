//! Loopback MCP server exposing the bot ToolRegistry to the Cursor SDK agent.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

use crate::agent_history::ToolCallRecord;
use crate::channels::telegram::{AgentEvent, AgentRequestContext, AppState};
use crate::tool_hook_dispatch::{
    dispatch_tool_with_hooks, run_post_tool_batch_hooks, ToolHookDispatchContext,
    ToolHookDispatchOutcome,
};
use crate::tools::ToolAuthContext;

pub const MCP_SERVER_NAME: &str = "finally-a-value-bot";
const AUTH_CONTEXT_KEY: &str = "__finally_a_value_bot_auth";
const RUN_TTL_SECS: u64 = 3600;
/// Official MCP protocol versions we speak (newest first).
/// Note: `2025-11-05` is **not** a real MCP version (common mix-up of `2024-11-05` + `2025-11-25`).
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";

fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    let requested = requested.map(str::trim).filter(|s| !s.is_empty());
    if let Some(req) = requested {
        if let Some(matched) = SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .find(|v| *v == req)
        {
            return matched;
        }
    }
    DEFAULT_PROTOCOL_VERSION
}

pub const DEFAULT_DENIED_TOOLS: &[&str] = &[
    "cursor_agent",
    "cursor_agent_send",
    "list_cursor_agent_runs",
];

type CursorMcpRunSummary = (
    Vec<String>,
    Vec<ToolCallRecord>,
    Vec<String>,
    Option<String>,
);
type CursorMcpFinishSummary = (
    Option<String>,
    Vec<String>,
    Vec<ToolCallRecord>,
    Vec<String>,
);

#[derive(Debug, Clone)]
pub struct CursorMcpRegisterParams {
    pub run_key: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub caller_channel: String,
    pub is_scheduled_task: bool,
    pub tool_auth: ToolAuthContext,
    pub expose_send_message: bool,
}

struct CursorMcpRunState {
    params: CursorMcpRegisterParams,
    expires_at: Instant,
    schedule_skill_activated: bool,
    modify_skill_activated: bool,
    discovery_streak_count: usize,
    legacy_edit_without_block_count: usize,
    force_stall_response: Option<String>,
    executed_tool_names: Vec<String>,
    executed_tool_inputs: Vec<(String, serde_json::Value)>,
    tool_call_records: Vec<ToolCallRecord>,
    history_hook_events: Vec<String>,
    event_tx: Option<UnboundedSender<AgentEvent>>,
}

struct McpDispatchScratch {
    schedule_skill_activated: bool,
    modify_skill_activated: bool,
    discovery_streak_count: usize,
    legacy_edit_without_block_count: usize,
    force_stall_response: Option<String>,
    executed_tool_names: Vec<String>,
    executed_tool_inputs: Vec<(String, serde_json::Value)>,
    history_hook_events: Vec<String>,
}

#[derive(Clone, Default)]
pub struct CursorMcpRegistry {
    runs: Arc<RwLock<HashMap<String, Arc<Mutex<CursorMcpRunState>>>>>,
}

impl CursorMcpRegistry {
    pub fn new() -> Self {
        Self {
            runs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_run(
        &self,
        params: CursorMcpRegisterParams,
        event_tx: Option<UnboundedSender<AgentEvent>>,
    ) -> String {
        self.purge_expired();
        let token = uuid::Uuid::new_v4().to_string();
        let expires_at = Instant::now() + Duration::from_secs(RUN_TTL_SECS);
        let state = CursorMcpRunState {
            params,
            expires_at,
            schedule_skill_activated: false,
            modify_skill_activated: false,
            discovery_streak_count: 0,
            legacy_edit_without_block_count: 0,
            force_stall_response: None,
            executed_tool_names: Vec::new(),
            executed_tool_inputs: Vec::new(),
            tool_call_records: Vec::new(),
            history_hook_events: Vec::new(),
            event_tx,
        };
        if let Ok(mut guard) = self.runs.write() {
            guard.insert(token.clone(), Arc::new(Mutex::new(state)));
        }
        token
    }

    pub fn revoke_run(&self, token: &str) {
        if let Ok(mut guard) = self.runs.write() {
            guard.remove(token);
        }
    }

    pub fn active_run_count(&self) -> usize {
        self.purge_expired();
        self.runs.read().map(|g| g.len()).unwrap_or(0)
    }

    pub fn take_run_summary(&self, token: &str) -> Option<CursorMcpRunSummary> {
        let arc = self.runs.read().ok()?.get(token)?.clone();
        let guard = arc.lock().ok()?;
        Some((
            guard.executed_tool_names.clone(),
            guard.tool_call_records.clone(),
            guard.history_hook_events.clone(),
            guard.force_stall_response.clone(),
        ))
    }

    pub async fn finish_run(
        &self,
        state: &AppState,
        context: &AgentRequestContext<'_>,
        run_key: &str,
        token: &str,
        event_tx: Option<&UnboundedSender<AgentEvent>>,
    ) -> Option<CursorMcpFinishSummary> {
        let arc = self.runs.read().ok()?.get(token).cloned()?;
        let (
            discovery_streak_count,
            legacy_edit_without_block_count,
            force_stall_present,
            mut history_hook_events,
            run_tool_names,
            tool_call_records,
        ) = {
            let guard = arc.lock().ok()?;
            (
                guard.discovery_streak_count,
                guard.legacy_edit_without_block_count,
                guard.force_stall_response.is_some(),
                guard.history_hook_events.clone(),
                guard.executed_tool_names.clone(),
                guard.tool_call_records.clone(),
            )
        };

        let batch = run_post_tool_batch_hooks(
            state,
            context,
            run_key,
            event_tx,
            discovery_streak_count,
            legacy_edit_without_block_count,
            force_stall_present,
            &mut history_hook_events,
        )
        .await;

        let stall = {
            let mut guard = arc.lock().ok()?;
            guard.history_hook_events = history_hook_events.clone();
            if let Some(stall) = batch.stall_response.clone() {
                guard.force_stall_response = Some(stall.clone());
                Some(stall)
            } else {
                guard.force_stall_response.clone()
            }
        };

        self.revoke_run(token);
        Some((
            stall,
            run_tool_names,
            tool_call_records,
            history_hook_events,
        ))
    }

    fn purge_expired(&self) {
        let Ok(mut guard) = self.runs.write() else {
            return;
        };
        let now = Instant::now();
        guard.retain(|_, run| run.lock().map(|g| g.expires_at > now).unwrap_or(false));
    }

    fn get_run(&self, token: &str) -> Option<Arc<Mutex<CursorMcpRunState>>> {
        self.purge_expired();
        let guard = self.runs.read().ok()?;
        let arc = guard.get(token)?.clone();
        let expired = arc
            .lock()
            .map(|g| g.expires_at <= Instant::now())
            .unwrap_or(true);
        if expired {
            return None;
        }
        Some(arc)
    }

    fn tool_allowed(&self, run: &CursorMcpRunState, name: &str) -> bool {
        if DEFAULT_DENIED_TOOLS.contains(&name) {
            return false;
        }
        if name == "send_message" && !run.params.expose_send_message {
            return false;
        }
        true
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

fn is_loopback(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn json_rpc_error(
    id: Option<serde_json::Value>,
    code: i64,
    message: impl Into<String>,
) -> Response {
    (
        StatusCode::OK,
        Json(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }),
    )
        .into_response()
}

fn json_rpc_result(id: Option<serde_json::Value>, result: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        Json(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }),
    )
        .into_response()
}

fn strip_auth_from_schema(schema: &serde_json::Value) -> serde_json::Value {
    let mut out = schema.clone();
    if let Some(props) = out.get_mut("properties").and_then(|v| v.as_object_mut()) {
        props.remove(AUTH_CONTEXT_KEY);
    }
    if let Some(required) = out.get_mut("required").and_then(|v| v.as_array_mut()) {
        required.retain(|v| v.as_str() != Some(AUTH_CONTEXT_KEY));
    }
    out
}

pub fn mcp_endpoint_url(web_port: u16) -> String {
    format!("http://127.0.0.1:{web_port}/internal/cursor-mcp")
}

pub fn build_mcp_servers_config(url: &str, token: &str) -> serde_json::Value {
    json!({
        MCP_SERVER_NAME: {
            "type": "http",
            "url": url,
            "headers": {
                "Authorization": format!("Bearer {token}")
            }
        }
    })
}

/// Streamable HTTP GET: we do not open a long-lived SSE listen stream.
/// Spec allows `405 Method Not Allowed` for this case.
pub async fn handle_cursor_mcp_get(peer: SocketAddr, headers: HeaderMap) -> Response {
    if !is_loopback(&peer) {
        warn!(peer = %peer, "Rejected Cursor MCP GET from non-loopback peer");
        return (StatusCode::FORBIDDEN, "loopback only").into_response();
    }
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if !origin_is_allowed(origin) {
            warn!(%origin, "Rejected Cursor MCP GET with invalid Origin");
            return (StatusCode::FORBIDDEN, "invalid origin").into_response();
        }
    }
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

fn origin_is_allowed(origin: &str) -> bool {
    let o = origin.trim().to_ascii_lowercase();
    o.is_empty()
        || o == "null"
        || o.starts_with("http://127.0.0.1")
        || o.starts_with("http://localhost")
        || o.starts_with("https://127.0.0.1")
        || o.starts_with("https://localhost")
}

pub async fn handle_cursor_mcp(
    peer: SocketAddr,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_loopback(&peer) {
        warn!(peer = %peer, "Rejected Cursor MCP request from non-loopback peer");
        return (StatusCode::FORBIDDEN, "loopback only").into_response();
    }

    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if !origin_is_allowed(origin) {
            warn!(%origin, "Rejected Cursor MCP request with invalid Origin");
            return (StatusCode::FORBIDDEN, "invalid origin").into_response();
        }
    }

    let token = match extract_bearer(&headers) {
        Some(t) => t,
        None => {
            return (StatusCode::UNAUTHORIZED, "Bearer token required").into_response();
        }
    };

    let request: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return json_rpc_error(None, -32700, format!("Parse error: {e}"));
        }
    };

    let method = request.method.clone();
    let run_arc = match state.cursor_mcp.get_run(&token) {
        Some(r) => r,
        None => {
            warn!(
                method = %method,
                "Cursor MCP rejected: invalid or expired run token"
            );
            return json_rpc_error(request.id, -32001, "Invalid or expired run token");
        }
    };

    match method.as_str() {
        "initialize" => {
            let requested = request
                .params
                .as_ref()
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str());
            let negotiated = negotiate_protocol_version(requested);
            info!(
                requested = requested.unwrap_or(""),
                negotiated, "Cursor MCP initialize"
            );
            json_rpc_result(
                request.id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "finally_a_value_bot",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
        }
        "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
        "tools/list" => {
            let resp = handle_tools_list(&state, &run_arc, request.id);
            info!("Cursor MCP tools/list completed");
            resp
        }
        "tools/call" => handle_tools_call(&state, &run_arc, request.id, request.params).await,
        "ping" => json_rpc_result(request.id, json!({})),
        other => json_rpc_error(request.id, -32601, format!("Method not found: {other}")),
    }
}

fn handle_tools_list(
    state: &AppState,
    run_arc: &Arc<Mutex<CursorMcpRunState>>,
    id: Option<serde_json::Value>,
) -> Response {
    let run = match run_arc.lock() {
        Ok(g) => g,
        Err(_) => return json_rpc_error(id, -32000, "Run lock poisoned"),
    };
    let tools: Vec<serde_json::Value> = state
        .tools
        .definitions()
        .into_iter()
        .filter(|def| run.params.expose_send_message || def.name != "send_message")
        .filter(|def| state.cursor_mcp.tool_allowed(&run, &def.name))
        .map(|def| {
            json!({
                "name": def.name,
                "description": def.description,
                "inputSchema": strip_auth_from_schema(&def.input_schema),
            })
        })
        .collect();
    json_rpc_result(id, json!({ "tools": tools }))
}

async fn handle_tools_call(
    state: &AppState,
    run_arc: &Arc<Mutex<CursorMcpRunState>>,
    id: Option<serde_json::Value>,
    params: Option<serde_json::Value>,
) -> Response {
    let params = params.unwrap_or(json!({}));
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if tool_name.is_empty() {
        return json_rpc_error(id, -32602, "tools/call requires name");
    }

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let (run_key, caller_channel, chat_id, persona_id, is_scheduled_task, tool_auth, event_tx) = {
        let run = match run_arc.lock() {
            Ok(g) => g,
            Err(_) => return json_rpc_error(id, -32000, "Run lock poisoned"),
        };
        if !state.cursor_mcp.tool_allowed(&run, &tool_name) {
            return json_rpc_error(id, -32002, format!("Tool not allowed: {tool_name}"));
        }
        (
            run.params.run_key.clone(),
            run.params.caller_channel.clone(),
            run.params.chat_id,
            run.params.persona_id,
            run.params.is_scheduled_task,
            run.params.tool_auth.clone(),
            run.event_tx.clone(),
        )
    };

    let context = AgentRequestContext {
        caller_channel: &caller_channel,
        chat_id,
        chat_type: "private",
        persona_id,
        is_scheduled_task,
        is_background_job: false,
        run_key: Some(run_key.clone()),
        reply_bot_instance_id: None,
        session_id: None,
    };

    let mut dispatch_state = {
        let run = run_arc.lock().expect("run lock");
        McpDispatchScratch {
            schedule_skill_activated: run.schedule_skill_activated,
            modify_skill_activated: run.modify_skill_activated,
            discovery_streak_count: run.discovery_streak_count,
            legacy_edit_without_block_count: run.legacy_edit_without_block_count,
            force_stall_response: run.force_stall_response.clone(),
            executed_tool_names: run.executed_tool_names.clone(),
            executed_tool_inputs: run.executed_tool_inputs.clone(),
            history_hook_events: run.history_hook_events.clone(),
        }
    };

    let mut dispatch_ctx = ToolHookDispatchContext {
        state,
        context: &context,
        run_key: &run_key,
        event_tx: event_tx.as_ref(),
        tool_auth: &tool_auth,
        schedule_skill_activated: &mut dispatch_state.schedule_skill_activated,
        modify_skill_activated: &mut dispatch_state.modify_skill_activated,
        discovery_streak_count: &mut dispatch_state.discovery_streak_count,
        legacy_edit_without_block_count: &mut dispatch_state.legacy_edit_without_block_count,
        executed_tool_names: &mut dispatch_state.executed_tool_names,
        executed_tool_inputs: &mut dispatch_state.executed_tool_inputs,
        force_stall_response: &mut dispatch_state.force_stall_response,
        history_hook_events: &mut dispatch_state.history_hook_events,
    };

    if let Some(tx) = event_tx.as_ref() {
        let _ = tx.send(AgentEvent::ToolStart {
            tool_use_id: uuid::Uuid::new_v4().to_string(),
            name: tool_name.clone(),
            input: arguments.clone(),
        });
    }

    let outcome = dispatch_tool_with_hooks(&mut dispatch_ctx, &tool_name, arguments).await;

    if let Some(tx) = event_tx.as_ref() {
        let _ = tx.send(AgentEvent::ToolResult {
            tool_use_id: uuid::Uuid::new_v4().to_string(),
            name: tool_name.clone(),
            is_error: outcome.result.is_error,
            output: outcome.result.content.clone(),
            duration_ms: outcome.record.duration_ms,
            status_code: outcome.result.status_code,
            bytes: outcome.result.bytes,
            error_type: outcome.result.error_type.clone(),
        });
    }

    let ToolHookDispatchOutcome {
        result,
        blocked,
        record,
    } = outcome;

    {
        let mut run = run_arc.lock().expect("run lock");
        run.schedule_skill_activated = dispatch_state.schedule_skill_activated;
        run.modify_skill_activated = dispatch_state.modify_skill_activated;
        run.discovery_streak_count = dispatch_state.discovery_streak_count;
        run.legacy_edit_without_block_count = dispatch_state.legacy_edit_without_block_count;
        run.force_stall_response = dispatch_state.force_stall_response;
        run.executed_tool_names = dispatch_state.executed_tool_names;
        run.executed_tool_inputs = dispatch_state.executed_tool_inputs;
        run.history_hook_events = dispatch_state.history_hook_events;
        run.tool_call_records.push(record);
    }

    info!(
        tool = %tool_name,
        is_error = result.is_error,
        blocked = blocked,
        "Cursor MCP tool call"
    );

    json_rpc_result(
        id,
        json!({
            "content": [{
                "type": "text",
                "text": result.content
            }],
            "isError": result.is_error
        }),
    )
}

pub async fn probe_mcp_health(web_port: u16) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let url = mcp_endpoint_url(web_port);
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": DEFAULT_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "doctor", "version": "1" }
        }
    });
    let resp = client
        .post(&url)
        .header("Authorization", "Bearer probe-invalid")
        .json(&body)
        .send()
        .await;
    match resp {
        Ok(r) => {
            r.status().is_success()
                || r.status() == StatusCode::UNAUTHORIZED
                || r.status() == StatusCode::FORBIDDEN
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_auth_from_schema_removes_internal_key() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                AUTH_CONTEXT_KEY: { "type": "object" }
            },
            "required": ["path", AUTH_CONTEXT_KEY]
        });
        let out = strip_auth_from_schema(&schema);
        assert!(!out["properties"]
            .as_object()
            .unwrap()
            .contains_key(AUTH_CONTEXT_KEY));
        let required = out["required"].as_array().unwrap();
        assert!(!required
            .iter()
            .any(|v| v.as_str() == Some(AUTH_CONTEXT_KEY)));
    }

    #[test]
    fn build_mcp_servers_config_has_http_transport() {
        let cfg = build_mcp_servers_config("http://127.0.0.1:1/internal/cursor-mcp", "tok");
        let entry = &cfg[MCP_SERVER_NAME];
        assert_eq!(entry["type"], "http");
        assert!(entry["headers"]["Authorization"]
            .as_str()
            .unwrap()
            .contains("tok"));
    }

    #[test]
    fn negotiate_protocol_version_echoes_cursor_request() {
        assert_eq!(negotiate_protocol_version(Some("2025-11-25")), "2025-11-25");
        assert_eq!(negotiate_protocol_version(Some("2025-06-18")), "2025-06-18");
        // Bogus / typo versions must not be echoed (Cursor disconnects).
        assert_eq!(
            negotiate_protocol_version(Some("2025-11-05")),
            DEFAULT_PROTOCOL_VERSION
        );
        assert_eq!(negotiate_protocol_version(None), DEFAULT_PROTOCOL_VERSION);
    }
}
