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
    EvaluatorStepRecord, IterationRecord, PipelineFinishExtras, PipelineStageRecord, ToolCallRecord,
};
use crate::channels::telegram::hook_turn_bridge::{
    append_hook_context_messages, pre_stop_follow_up, run_before_turn_hooks, run_pre_stop_hooks,
    PreStopFollowUp, DEFERRED_COMMITMENT_MAX_NUDGES,
};
use crate::channels::telegram::{
    pipeline_finish_turn, process_classic_agent_with_events, AgentEvent, AgentProcessResult,
    AgentRequestContext, AgentRunPrep, AppState,
};
use crate::claude::{Message, MessageContent};
use crate::cursor_delegation_prompt::{
    build_cursor_delegation_prompt, select_delegation_prompt_mode, slim_delegation_system_prompt,
    DelegationPromptMode, DelegationRuntimeHeader,
};
use crate::cursor_mcp_bridge::{
    build_mcp_servers_config, mcp_endpoint_url, CursorMcpRegisterParams, CursorMcpRegistry,
};
use crate::db::call_blocking;
use crate::tools;

#[derive(Debug, Serialize)]
struct SidecarRunRequest<'a> {
    prompt: &'a str,
    cwd: &'a str,
    model: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    session_scope: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_params: Option<&'a [crate::cursor_engine_config::CursorModelParam]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mcp_servers: Option<serde_json::Value>,
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
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    is_error: Option<bool>,
    #[serde(default)]
    thinking: Option<String>,
}

struct SidecarRunOutcome {
    final_text: String,
    returned_agent_id: Option<String>,
    run_status: String,
    sidecar_error: Option<String>,
    cleared_stale_resume: bool,
}

const CURSOR_EMPTY_OUTPUT_PLACEHOLDER: &str = "(Cursor agent completed with no text output.)";

fn is_cursor_empty_user_facing_text(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty() || trimmed == CURSOR_EMPTY_OUTPUT_PLACEHOLDER
}

fn cursor_planning_opener(text: &str) -> bool {
    let first = text
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    first.starts_with("i'll ")
        || first.starts_with("i will ")
        || first.starts_with("let me ")
        || first.starts_with("first i")
        || first.starts_with("next i")
        || first.starts_with("i'm ")
        || first.starts_with("i am ")
        || first.starts_with("i already")
}

/// Cursor emits one assistant `text` event per tool-round. Those are complete utterances,
/// not token deltas — concatenating them without a break produces `process.The ratio`.
fn cursor_utterance_looks_like_final_answer(text: &str) -> bool {
    let t = text.trim();
    let chars = t.chars().count();
    if chars < 220 {
        return false;
    }
    let lines = t.lines().filter(|l| !l.trim().is_empty()).count();
    if cursor_planning_opener(t) && lines <= 2 && chars < 480 {
        return false;
    }
    lines >= 3 || t.contains("### ") || t.contains("\n- ") || chars >= 400
}

fn push_cursor_utterance(parts: &mut Vec<String>, delta: &str) {
    let t = delta.trim();
    if t.is_empty() {
        return;
    }
    if let Some(last) = parts.last_mut() {
        if last == t || last.ends_with(t) {
            return;
        }
        if t.starts_with(last.as_str()) && t.len() > last.len() {
            *last = t.to_string();
            return;
        }
    }
    parts.push(t.to_string());
}

/// Cursor sometimes emits many tiny `text` events (token-sized) instead of `text_delta`.
fn cursor_text_event_is_stream_fragment(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains("\n\n") || t.starts_with("### ") {
        return false;
    }
    if cursor_planning_opener(t) {
        return false;
    }
    let chars = t.chars().count();
    if chars > 72 {
        return false;
    }
    if chars >= 20
        && t.split_whitespace().count() >= 4
        && (t.ends_with('.') || t.ends_with('!') || t.ends_with('?'))
    {
        return false;
    }
    true
}

fn cursor_fragment_join_needs_space(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if right.starts_with(char::is_whitespace) {
        return false;
    }
    if right.starts_with(|c: char| {
        matches!(
            c,
            ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'' | '-' | '/'
        )
    }) {
        return false;
    }
    if left.ends_with(['(', '[', '{', '"', '\'', '-', '/']) {
        return false;
    }
    if left.ends_with(char::is_whitespace) {
        return false;
    }
    // Token continuation such as "hot" + "ify" → "hotify".
    if left.ends_with(|c: char| c.is_ascii_alphanumeric())
        && right.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && right.chars().count() <= 24
        && !right.contains('\n')
    {
        return false;
    }
    true
}

fn join_cursor_stream_fragments(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        let chunk = part.trim();
        if chunk.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(chunk);
            continue;
        }
        if cursor_fragment_join_needs_space(&out, chunk) {
            out.push(' ');
        }
        out.push_str(chunk);
    }
    out
}

fn cursor_parts_look_like_fragment_stream(parts: &[String]) -> bool {
    let trimmed: Vec<&str> = parts
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if trimmed.len() < 12 {
        return false;
    }
    let short = trimmed
        .iter()
        .filter(|s| s.chars().count() <= 72 && !s.contains("\n\n"))
        .count();
    if short * 100 / trimmed.len().max(1) < 85 {
        return false;
    }
    trimmed.iter().filter(|s| cursor_planning_opener(s)).count() <= 2
}

fn cursor_text_looks_like_per_word_newlines(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() < 12 {
        return false;
    }
    let short = lines.iter().filter(|l| l.chars().count() <= 72).count();
    short * 100 / lines.len().max(1) >= 85
}

fn append_cursor_stream_fragment(parts: &mut Vec<String>, fragment: &str) {
    if fragment.is_empty() {
        return;
    }
    if fragment.starts_with(char::is_whitespace) {
        if let Some(last) = parts.last_mut() {
            last.push_str(fragment);
            return;
        }
    }
    let chunk = fragment.trim_start();
    if chunk.is_empty() {
        return;
    }
    if let Some(last) = parts.last_mut() {
        if cursor_fragment_join_needs_space(last, chunk) {
            last.push(' ');
        }
        last.push_str(chunk);
    } else {
        parts.push(chunk.to_string());
    }
}

fn append_cursor_token_delta(parts: &mut Vec<String>, delta: &str) {
    if delta.is_empty() {
        return;
    }
    if let Some(last) = parts.last_mut() {
        last.push_str(delta);
    } else if !delta.trim().is_empty() {
        parts.push(delta.to_string());
    }
}

/// Prefer the last real write-up over a glued stream of "I'll look / Next I'll / …".
pub fn coalesce_cursor_delivery_text(parts: &[String], done_result: Option<&str>) -> String {
    if let Some(raw) = done_result.map(str::trim).filter(|s| !s.is_empty()) {
        let result = normalize_cursor_done_result(raw);
        if cursor_utterance_looks_like_final_answer(&result) {
            return result;
        }
        let streamed = coalesce_cursor_streamed_parts(parts);
        if !streamed.is_empty()
            && result.len() > streamed.len() + 80
            && (result.contains(streamed.as_str())
                || cursor_text_looks_like_duplicate_delivery(&streamed, &result))
        {
            return result;
        }
    }

    let mut parts: Vec<String> = parts
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(raw) = done_result.map(str::trim).filter(|s| !s.is_empty()) {
        let result = normalize_cursor_done_result(raw);
        let already = parts.iter().any(|p| p == &result);
        if !already {
            if parts
                .last()
                .is_some_and(|p| result.contains(p.as_str()) && result.len() > p.len() + 20)
            {
                parts.pop();
            }
            parts.push(result);
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    if let Some(last) = parts.last() {
        if cursor_utterance_looks_like_final_answer(last) {
            return dedupe_cursor_delivery_text(last);
        }
    }
    for part in parts.iter().rev() {
        if cursor_utterance_looks_like_final_answer(part) {
            return dedupe_cursor_delivery_text(part);
        }
    }
    if cursor_parts_look_like_fragment_stream(&parts) {
        return join_cursor_stream_fragments(&parts);
    }
    dedupe_cursor_delivery_text(&parts.join("\n\n"))
}

fn normalize_cursor_done_result(raw: &str) -> String {
    let repaired = if cursor_text_looks_like_per_word_newlines(raw) {
        join_cursor_stream_fragments(
            &raw.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>(),
        )
    } else {
        raw.to_string()
    };
    dedupe_cursor_delivery_text(&repaired)
}

fn coalesce_cursor_streamed_parts(parts: &[String]) -> String {
    let trimmed: Vec<String> = parts
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if trimmed.is_empty() {
        return String::new();
    }
    if cursor_parts_look_like_fragment_stream(&trimmed) {
        join_cursor_stream_fragments(&trimmed)
    } else {
        trimmed.join("\n\n")
    }
}

fn cursor_text_looks_like_broken_token_spacing(text: &str) -> bool {
    text.contains(" ## ")
        || text.contains("R 4 ")
        || text.contains("CLIP Seg ")
        || text.contains("Aug 26 – ")
        || text.contains("PZ _ ")
}

fn cursor_text_looks_like_duplicate_delivery(streamed: &str, done: &str) -> bool {
    let a = streamed.trim();
    let b = done.trim();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    for marker in [
        "## 1.",
        "The last experiment block is",
        "### What was fixed",
    ] {
        if b.matches(marker).count() >= 2 {
            return true;
        }
        if a.contains(marker) && b.contains(marker) && b.len() > a.len() + 200 {
            return true;
        }
    }
    false
}

fn dedupe_cursor_delivery_text(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    for marker in [
        "The last experiment block is",
        "## 1. Current track",
        "## 1.",
        "### What was fixed",
    ] {
        if let Some(first) = t.find(marker) {
            if let Some(second_rel) = t[first + marker.len()..].find(marker) {
                let split_at = first + marker.len() + second_rel;
                let head = t[..split_at].trim();
                let tail = t[split_at..].trim();
                if cursor_text_looks_like_broken_token_spacing(head)
                    || tail.len() >= head.len().saturating_sub(200)
                {
                    return tail.to_string();
                }
            }
        }
    }
    t.to_string()
}

fn empty_output_nudge_prompt() -> String {
    concat!(
        "[system_runtime_context]\n",
        "Your previous turn produced no user-visible text. ",
        "Reply now with a concise final answer the user can read in chat. ",
        "Do not end with only tool calls; include a short message."
    )
    .to_string()
}

/// Prefer a short non-JSON tool result when Cursor returns no assistant text.
fn recover_user_text_from_tool_records(records: &[ToolCallRecord]) -> Option<String> {
    for record in records.iter().rev() {
        if record.is_error {
            continue;
        }
        let preview = record.result_preview.trim();
        if preview.is_empty() || preview.len() > 4_000 {
            continue;
        }
        let first = preview.chars().next().unwrap_or('\0');
        if first == '{' || first == '[' {
            continue;
        }
        return Some(preview.to_string());
    }
    None
}

fn is_stale_cursor_agent_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("not found") && lower.contains("agent")
}

fn is_busy_cursor_agent_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("already has active run")
        || lower.contains("agent is busy")
        || lower.contains("agent_busy")
}

fn is_unusable_cursor_agent_error(message: &str) -> bool {
    is_stale_cursor_agent_error(message) || is_busy_cursor_agent_error(message)
}

fn is_cursor_sidecar_recoverable_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    if lower.contains("sidecar request failed") || lower.starts_with("sidecar http") {
        return true;
    }
    if lower.contains("bridge request failed") || lower.contains("bridge request timed out") {
        return true;
    }
    if lower.contains("timed out waiting for bridge discovery")
        || lower.contains("bridge discovery")
    {
        return true;
    }
    if lower.contains("cursor sdk startup failed")
        && (lower.contains("bridge") || lower.contains("discovery") || lower.contains("timed out"))
    {
        return true;
    }
    if lower.contains("at capacity") || lower.contains("cursor_run_concurrency") {
        return true;
    }
    lower.contains("connection refused") || lower.contains("errno 111")
}

/// Revokes the MCP run token on drop unless `take_for_finish` was used.
struct McpTokenGuard {
    registry: Arc<CursorMcpRegistry>,
    token: Option<String>,
}

impl McpTokenGuard {
    fn new(registry: Arc<CursorMcpRegistry>, token: Option<String>) -> Self {
        Self { registry, token }
    }

    fn take_for_finish(&mut self) -> Option<String> {
        self.token.take()
    }
}

impl Drop for McpTokenGuard {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            self.registry.revoke_run(&token);
        }
    }
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
    if context.is_scheduled_task || context.is_background_job {
        let prompt = prep.latest_user_text.trim();
        let override_prompt = if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        };
        let kind = if context.is_scheduled_task {
            "scheduled task"
        } else {
            "background job"
        };
        warn!("Cursor engine fallback to classic for {kind}: {reason}");
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
    tool_call_records: &mut Vec<ToolCallRecord>,
    mcp_tool_count: &mut usize,
) -> Result<SidecarRunOutcome, anyhow::Error> {
    let mut final_text = String::new();
    let mut text_parts: Vec<String> = Vec::new();
    let mut returned_agent_id: Option<String> = None;
    let mut run_status = String::from("unknown");
    let mut sidecar_error: Option<String> = None;
    let mut buffer = String::new();

    let mut byte_stream = response.bytes_stream();
    let mut cancel_tick = tokio::time::interval(std::time::Duration::from_millis(250));
    cancel_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = cancel_tick.tick(), if cancel.is_some() => {
                if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                    return Err(anyhow::anyhow!("Run cancelled"));
                }
            }
            chunk = byte_stream.next() => {
                let Some(chunk) = chunk else {
                    break;
                };
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
                        "text" => {
                            let delta = if event.text.is_empty() {
                                event.message
                            } else {
                                event.text
                            };
                            if !delta.trim().is_empty() {
                                if cursor_text_event_is_stream_fragment(&delta) {
                                    append_cursor_stream_fragment(&mut text_parts, &delta);
                                    if let Some(tx) = event_tx {
                                        let _ = tx.send(AgentEvent::TextDelta { delta });
                                    }
                                } else {
                                    let emit = if text_parts.is_empty() {
                                        delta.trim().to_string()
                                    } else {
                                        format!("\n\n{}", delta.trim())
                                    };
                                    push_cursor_utterance(&mut text_parts, &delta);
                                    if let Some(tx) = event_tx {
                                        let _ = tx.send(AgentEvent::TextDelta { delta: emit });
                                    }
                                }
                            }
                        }
                        "text_delta" => {
                            let delta = if event.text.is_empty() {
                                event.message
                            } else {
                                event.text
                            };
                            if !delta.is_empty() {
                                append_cursor_token_delta(&mut text_parts, &delta);
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
                            final_text = coalesce_cursor_delivery_text(
                                &text_parts,
                                event.result.as_deref(),
                            );
                        }
                        "error" => {
                            sidecar_error = Some(if event.message.is_empty() {
                                "Cursor sidecar run failed".into()
                            } else {
                                event.message
                            });
                        }
                        "tool_use" => {
                            let name = event.name.unwrap_or_default();
                            let tool_use_id = uuid::Uuid::new_v4().to_string();
                            let input = event.input.unwrap_or(serde_json::json!({}));
                            if let Some(tx) = event_tx {
                                let _ = tx.send(AgentEvent::ToolStart {
                                    tool_use_id: tool_use_id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                });
                            }
                            tool_call_records.push(ToolCallRecord {
                                name,
                                input_preview: serde_json::to_string(&input).unwrap_or_default(),
                                result_preview: String::new(),
                                duration_ms: 0,
                                is_error: false,
                            });
                        }
                        "tool_result" => {
                            let name = event.name.unwrap_or_default();
                            let is_error = event.is_error.unwrap_or(false);
                            let output = event.output.unwrap_or_default();
                            if let Some(tx) = event_tx {
                                let _ = tx.send(AgentEvent::ToolResult {
                                    tool_use_id: uuid::Uuid::new_v4().to_string(),
                                    name: name.clone(),
                                    is_error,
                                    output: output.clone(),
                                    duration_ms: 0,
                                    status_code: Some(if is_error { 1 } else { 0 }),
                                    bytes: output.len(),
                                    error_type: None,
                                });
                            }
                            if let Some(record) = tool_call_records
                                .iter_mut()
                                .rev()
                                .find(|r| r.name == name && r.result_preview.is_empty())
                            {
                                record.result_preview = output;
                                record.is_error = is_error;
                            }
                            *mcp_tool_count = mcp_tool_count.saturating_add(1);
                        }
                        "thinking" => {
                            if state_show_thinking(event_tx) {
                                if let Some(text) = event.thinking.filter(|s| !s.is_empty()) {
                                    if let Some(tx) = event_tx {
                                        let _ = tx.send(AgentEvent::TextDelta { delta: text });
                                    }
                                }
                            }
                        }
                        other => {
                            warn!("Cursor sidecar unknown event type: {other}");
                        }
                    }
                }
            }
        }
    }

    if final_text.is_empty() {
        final_text = coalesce_cursor_delivery_text(&text_parts, None);
    }

    Ok(SidecarRunOutcome {
        final_text,
        returned_agent_id,
        run_status,
        sidecar_error,
        cleared_stale_resume: false,
    })
}

fn state_show_thinking(event_tx: Option<&UnboundedSender<AgentEvent>>) -> bool {
    event_tx.is_some()
}

#[allow(clippy::too_many_arguments)]
async fn invoke_sidecar_turn(
    state: &AppState,
    client: &reqwest::Client,
    sidecar_url: &str,
    prompt: &str,
    cwd: &str,
    model: &str,
    model_params: Option<&[crate::cursor_engine_config::CursorModelParam]>,
    resume_id: Option<String>,
    chat_id: i64,
    persona_id: i64,
    session_scope: &str,
    event_tx: Option<&UnboundedSender<AgentEvent>>,
    cancel: Option<&Arc<AtomicBool>>,
    mcp_servers: Option<serde_json::Value>,
    tool_call_records: &mut Vec<ToolCallRecord>,
    mcp_tool_count: &mut usize,
) -> Result<SidecarRunOutcome, anyhow::Error> {
    let body = SidecarRunRequest {
        prompt,
        cwd,
        model,
        session_scope,
        agent_id: resume_id.as_deref(),
        model_params,
        mcp_servers: mcp_servers.clone(),
    };

    let response = client
        .post(sidecar_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("sidecar request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "sidecar HTTP {}: {}",
            status.as_u16(),
            text.chars().take(200).collect::<String>()
        ));
    }

    let outcome = consume_sidecar_stream(
        response,
        event_tx,
        cancel,
        tool_call_records,
        mcp_tool_count,
    )
    .await?;

    if let Some(ref err) = outcome.sidecar_error {
        if resume_id.is_some() && is_unusable_cursor_agent_error(err) {
            warn!(
                chat_id,
                persona_id,
                stale_agent_id = resume_id.as_deref().unwrap_or(""),
                busy = is_busy_cursor_agent_error(err),
                "Unusable Cursor agent id; clearing for fresh delegation prompt"
            );
            let db = state.db.clone();
            let session_scope_for_db = session_scope.to_string();
            let _ = call_blocking(db, move |database| {
                database.clear_cursor_engine_agent_id(chat_id, persona_id, &session_scope_for_db)
            })
            .await;
            return Ok(SidecarRunOutcome {
                final_text: String::new(),
                returned_agent_id: None,
                run_status: "stale_agent_id".into(),
                sidecar_error: None,
                cleared_stale_resume: true,
            });
        }
        return Err(anyhow::anyhow!(err.clone()));
    }

    Ok(outcome)
}

fn rebuild_delegation_prompt(
    mode: DelegationPromptMode,
    delegation_system: &str,
    hook_messages: &[Message],
    runtime_header: &DelegationRuntimeHeader,
    is_scheduled_task: bool,
    has_image_input: bool,
) -> String {
    build_cursor_delegation_prompt(
        mode,
        delegation_system,
        hook_messages,
        runtime_header,
        is_scheduled_task,
        has_image_input,
    )
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

    let before_turn = run_before_turn_hooks(state, &context, &prep.run_key, event_tx).await?;
    let mut hook_summaries: Vec<String> = before_turn.summary.into_iter().collect();
    if let Some(reason) = before_turn.result.blocked_reason {
        return Ok(AgentProcessResult {
            response: format!("This run was blocked before execution: {reason}"),
        });
    }

    let mut hook_messages = prep.messages.clone();
    append_hook_context_messages(&mut hook_messages, &before_turn.result.additional_contexts);

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
    if let Err(msg) = crate::self_repo::check_agent_cwd_allowed(&workspace_root, &working_dir) {
        return Err(anyhow::anyhow!(msg));
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

    let model_params = if settings.sdk_model_params.is_empty() {
        None
    } else {
        Some(settings.sdk_model_params.as_slice())
    };

    let mcp_enabled = settings.mcp_tools_enabled && state.config.web_enabled;
    let (mcp_servers, mut mcp_guard) = if mcp_enabled {
        let url = mcp_endpoint_url(state.config.web_port);
        let token = state.cursor_mcp.register_run(
            CursorMcpRegisterParams {
                run_key: prep.run_key.clone(),
                chat_id,
                persona_id,
                caller_channel: context.caller_channel.to_string(),
                is_scheduled_task: context.is_scheduled_task,
                tool_auth: prep.tool_auth.clone(),
                expose_send_message: settings.mcp_expose_send_message,
            },
            event_tx.cloned(),
        );
        let servers = build_mcp_servers_config(&url, &token);
        (
            Some(servers),
            McpTokenGuard::new(state.cursor_mcp.clone(), Some(token)),
        )
    } else {
        (None, McpTokenGuard::new(state.cursor_mcp.clone(), None))
    };

    let started_at = chrono::Utc::now().to_rfc3339();
    let sidecar_started = Instant::now();
    let mut resume_id = resume_agent_id.clone();

    let slim_enabled = settings.delegation_slim_prompt && mcp_enabled;
    let delegation_system = slim_delegation_system_prompt(&prep.system_prompt, slim_enabled);
    let runtime_header = DelegationRuntimeHeader {
        chat_id,
        persona_id,
        mcp_enabled,
    };
    let mut delegation_mode = select_delegation_prompt_mode(
        resume_agent_id.as_deref(),
        context.is_scheduled_task,
        settings.delegation_resume_delta,
    );
    let mut delegation_mode_label = delegation_mode.as_str().to_string();
    let initial_prompt = rebuild_delegation_prompt(
        delegation_mode,
        &delegation_system,
        &hook_messages,
        &runtime_header,
        context.is_scheduled_task,
        prep.has_image_input,
    );
    let initial_prompt_len = initial_prompt.len();
    let mut prompt = initial_prompt.clone();
    let mut nudge_count = 0usize;
    let mut final_text;
    let mut run_status;
    let mut returned_agent_id: Option<String> = None;
    let mut stream_tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut mcp_tool_count_total = 0usize;

    loop {
        let mut mcp_tool_count_turn = 0usize;
        let outcome = match invoke_sidecar_turn(
            state,
            &client,
            &sidecar_url,
            &prompt,
            &cwd,
            model,
            model_params,
            resume_id.clone(),
            chat_id,
            persona_id,
            &session_scope,
            event_tx,
            cancel.as_ref(),
            mcp_servers.clone(),
            &mut stream_tool_records,
            &mut mcp_tool_count_turn,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                // McpTokenGuard Drop revokes on abort/cancel/error.
                let msg = e.to_string();
                if is_cursor_sidecar_recoverable_error(&msg) {
                    return cursor_engine_classic_fallback(
                        state, context, &prep, event_tx, cancel, &msg,
                    )
                    .await;
                }
                return Err(e);
            }
        };

        mcp_tool_count_total = mcp_tool_count_total.saturating_add(mcp_tool_count_turn);

        if outcome.cleared_stale_resume {
            resume_id = None;
            delegation_mode = DelegationPromptMode::FullSlim;
            delegation_mode_label = "full_slim_stale_retry".into();
            prompt = rebuild_delegation_prompt(
                delegation_mode,
                &delegation_system,
                &hook_messages,
                &runtime_header,
                context.is_scheduled_task,
                prep.has_image_input,
            );
            continue;
        }

        final_text = outcome.final_text;
        run_status = outcome.run_status;
        if let Some(agent_id) = outcome.returned_agent_id {
            returned_agent_id = Some(agent_id);
            resume_id = returned_agent_id.clone();
        }

        if final_text.trim().is_empty() {
            let can_nudge_empty =
                nudge_count < DEFERRED_COMMITMENT_MAX_NUDGES && returned_agent_id.is_some();
            if can_nudge_empty {
                warn!(
                    chat_id,
                    persona_id,
                    nudge_count,
                    "Cursor agent returned empty text; nudging for user-facing reply"
                );
                nudge_count += 1;
                prompt = empty_output_nudge_prompt();
                continue;
            }
            if let Some(recovered) = recover_user_text_from_tool_records(&stream_tool_records) {
                info!(
                    chat_id,
                    persona_id,
                    recovered_len = recovered.len(),
                    "Recovered user-facing text from Cursor tool results after empty assistant output"
                );
                final_text = recovered;
            } else {
                final_text = CURSOR_EMPTY_OUTPUT_PLACEHOLDER.into();
            }
        }

        let can_continue = nudge_count < DEFERRED_COMMITMENT_MAX_NUDGES;
        let pre_stop = run_pre_stop_hooks(
            state,
            &context,
            &prep.run_key,
            event_tx,
            "end_turn",
            &final_text,
            &hook_messages,
            nudge_count,
            can_continue,
        )
        .await?;
        if let Some(summary) = pre_stop.summary {
            hook_summaries.push(summary);
        }

        match pre_stop_follow_up(&pre_stop.result, nudge_count) {
            PreStopFollowUp::Proceed => break,
            PreStopFollowUp::BlockFinal { message } => {
                final_text = message;
                break;
            }
            PreStopFollowUp::Nudge {
                prompt: nudge_prompt,
            } => {
                nudge_count += 1;
                prompt = nudge_prompt;
                if returned_agent_id.is_none() {
                    break;
                }
                continue;
            }
        }
    }

    let finished_at = chrono::Utc::now().to_rfc3339();
    let success = run_status == "finished" || run_status == "success";

    let mut run_tool_names: Vec<String> = Vec::new();
    let mut history_tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut history_hook_events: Vec<String> = Vec::new();

    if let Some(token) = mcp_guard.take_for_finish() {
        if let Some((stall, names, records, hook_events)) = state
            .cursor_mcp
            .finish_run(state, &context, &prep.run_key, &token, event_tx)
            .await
        {
            if let Some(stall_text) = stall {
                final_text = stall_text;
                run_status = "loop_guard_stalled".into();
            }
            run_tool_names = names;
            history_tool_records = records;
            history_hook_events = hook_events;
        }
    }

    if is_cursor_empty_user_facing_text(&final_text) {
        if let Some(recovered) = recover_user_text_from_tool_records(&history_tool_records)
            .or_else(|| recover_user_text_from_tool_records(&stream_tool_records))
        {
            info!(
                chat_id,
                persona_id,
                recovered_len = recovered.len(),
                "Replaced empty Cursor placeholder with tool-result summary"
            );
            final_text = recovered;
        }
    }

    if run_tool_names.is_empty() && !stream_tool_records.is_empty() {
        run_tool_names = stream_tool_records.iter().map(|r| r.name.clone()).collect();
        history_tool_records = stream_tool_records.clone();
    }

    let had_tool_calls = mcp_tool_count_total > 0 || !run_tool_names.is_empty();
    let initial_prompt_preview: String = if initial_prompt.len() <= 200 {
        initial_prompt.clone()
    } else {
        format!(
            "{}...",
            &initial_prompt[..initial_prompt.floor_char_boundary(200)]
        )
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
            &initial_prompt_preview,
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

    let hooks_detail = if hook_summaries.is_empty() && history_hook_events.is_empty() {
        String::new()
    } else {
        let mut parts = hook_summaries.clone();
        parts.extend(history_hook_events.clone());
        format!(" hooks={}", parts.join(";"))
    };
    let tool_detail = if run_tool_names.is_empty() {
        String::new()
    } else {
        format!(" tools={}", run_tool_names.join(","))
    };
    let pipeline_stages = vec![PipelineStageRecord {
        stage: "cursor_sdk".into(),
        detail: format!(
            "model={} resume={} status={} nudges={} mcp={} delegation={} prompt_chars={}{}{}",
            model,
            resume_agent_id.is_some(),
            run_status,
            nudge_count,
            mcp_enabled,
            delegation_mode_label,
            initial_prompt_len,
            hooks_detail,
            tool_detail
        ),
        duration_ms: sidecar_started.elapsed().as_millis(),
    }];
    let extras = PipelineFinishExtras {
        pipeline_stages,
        cloud_calls: 0,
        agent_engine: "cursor".into(),
    };

    let mut messages = prep.messages.clone();
    messages.push(Message {
        role: "assistant".into(),
        content: MessageContent::Text(final_text.clone()),
    });
    let mut pdqe_retries = 0usize;
    let mut pdqe_steps: Vec<EvaluatorStepRecord> = Vec::new();
    let history_iterations = if history_tool_records.is_empty() {
        Vec::new()
    } else {
        vec![IterationRecord {
            iteration: 1,
            stop_reason: "tool_use".into(),
            assistant_text_preview: if final_text.len() <= 200 {
                final_text.clone()
            } else {
                format!("{}...", &final_text[..final_text.floor_char_boundary(200)])
            },
            tool_calls: history_tool_records,
            hook_events: history_hook_events,
            pte: None,
            model_tier: "cursor".into(),
            provider: "cursor_sdk".into(),
            model: model.to_string(),
            endpoint: settings.sdk_runner_url.clone(),
        }]
    };
    let mut agent_history_basename: Option<String> = None;

    info!(
        chat_id,
        persona_id,
        model,
        resume = resume_agent_id.is_some(),
        status = %run_status,
        nudges = nudge_count,
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
            had_tool_calls,
            &prep.user_msg_preview,
            run_start,
            &prep.initial_llm_snapshot_json,
            &prep.local_delegate_run_summary,
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
    fn is_busy_cursor_agent_error_detects_active_run() {
        assert!(is_busy_cursor_agent_error(
            "Agent agent-1c9d98ad-57cf-46ae-87ac-f6c5910b081c already has active run"
        ));
        assert!(is_unusable_cursor_agent_error(
            "Cursor SDK startup failed: Agent agent-1c9d98ad-57cf-46ae-87ac-f6c5910b081c already has active run"
        ));
        assert!(!is_busy_cursor_agent_error("prompt required"));
    }

    #[test]
    fn coalesce_cursor_progress_gets_paragraph_breaks() {
        let parts = vec![
            "I'll diagnose the flat loaf from your ratio photo.".into(),
            "The ratio screenshot is in. Next I’ll check your vault notes.".into(),
            "The ratio math is clear: 90% whole wheat. I'll archive that diagnosis.".into(),
        ];
        let out = coalesce_cursor_delivery_text(&parts, None);
        assert!(out.contains("\n\n"));
        assert!(!out.contains("photo.The ratio"));
        assert!(out.contains("90% whole wheat"));
    }

    #[test]
    fn coalesce_cursor_prefers_last_real_writeup() {
        let parts = vec![
            "I'll inspect the landing page and how steps 2–5 are rendered.".into(),
            "Implementing per-step mobile illustrations.".into(),
            "I've fixed the missing illustrations and the ads on both landing pages.\n\n### What was fixed\n\n1. Hardcoded illustrations into each of the five steps so they show on load.\n2. Mobile opacity is 100% so the mocks stay visible.\n3. Ads scripts are on both landing pages.\n\nPlease check the deploy.".into(),
        ];
        let out = coalesce_cursor_delivery_text(&parts, None);
        assert!(out.starts_with("I've fixed the missing illustrations"));
        assert!(!out.contains("I'll inspect the landing page"));
        assert!(out.contains("### What was fixed"));
    }

    #[test]
    fn coalesce_cursor_joins_word_fragment_streams() {
        let parts = vec![
            "Use".into(),
            "production".into(),
            "pz".into(),
            "-".into(),
            "hot".into(),
            "ify".into(),
            "on".into(),
            "R4".into(),
            "base".into(),
            "instead".into(),
            "of".into(),
            "lingerie".into(),
            "-".into(),
            "transfer".into(),
        ];
        let out = coalesce_cursor_delivery_text(&parts, None);
        assert_eq!(
            out,
            "Use production pz-hotify on R4 base instead of lingerie-transfer"
        );
        assert!(!out.contains("\n\n"));
    }

    #[test]
    fn coalesce_cursor_repairs_done_result_with_per_word_newlines() {
        let broken = "Use\n\nproduction\n\npz\n\n-\n\nhot\n\nify\n\non\n\nR4\n\nbase";
        let out = coalesce_cursor_delivery_text(&[], Some(broken));
        assert_eq!(out, "Use production pz-hotify on R4 base");
    }

    #[test]
    fn coalesce_cursor_prefers_sdk_done_result_over_streamed_prefix() {
        let streamed = "I'll pull the experiment notes and SOP. The last experiment block is ** Aug 26 – 27 **.";
        let sdk = "The last experiment block is **Aug 26–27**, not the July CLIPSeg runs.\n\n## 1. Current track\n\nGoal: prove Stage 1→2.";
        let parts = vec![streamed.to_string()];
        let out = coalesce_cursor_delivery_text(&parts, Some(sdk));
        assert_eq!(out, sdk);
        assert!(!out.contains("I'll pull the experiment notes"));
    }

    #[test]
    fn dedupe_cursor_delivery_text_drops_repeated_writeup() {
        let broken = "I'll search notes. The last experiment block is ** Aug 26 – 27 **.\n\n---\n\n## 1. Track A";
        let clean = "The last experiment block is **Aug 26–27**.\n\n---\n\n## 1. Track B";
        let combined = format!("{broken}\n\n{clean}");
        let out = coalesce_cursor_delivery_text(&[], Some(&combined));
        assert!(out.starts_with("The last experiment block is **Aug 26–27**"));
        assert!(!out.contains("I'll search notes"));
        assert!(out.contains("Track B"));
    }

    #[test]
    fn cursor_text_event_is_stream_fragment_skips_planning_utterances() {
        assert!(!cursor_text_event_is_stream_fragment(
            "I'll diagnose the flat loaf from your ratio photo."
        ));
        assert!(cursor_text_event_is_stream_fragment("hot"));
        assert!(cursor_text_event_is_stream_fragment("ify"));
    }

    #[test]
    fn empty_output_helpers_detect_placeholder_and_recover_tool_text() {
        assert!(is_cursor_empty_user_facing_text(""));
        assert!(is_cursor_empty_user_facing_text(
            CURSOR_EMPTY_OUTPUT_PLACEHOLDER
        ));
        assert!(!is_cursor_empty_user_facing_text("hello"));

        let nudge = empty_output_nudge_prompt();
        assert!(nudge.contains("no user-visible text"));

        let records = vec![
            ToolCallRecord {
                name: "bash".into(),
                input_preview: "{}".into(),
                result_preview: r#"{"ok":true}"#.into(),
                duration_ms: 1,
                is_error: false,
            },
            ToolCallRecord {
                name: "read_file".into(),
                input_preview: "{}".into(),
                result_preview: "Album links should sit above the group notice.".into(),
                duration_ms: 1,
                is_error: false,
            },
        ];
        assert_eq!(
            recover_user_text_from_tool_records(&records).as_deref(),
            Some("Album links should sit above the group notice.")
        );
        assert!(recover_user_text_from_tool_records(&[]).is_none());
    }

    #[test]
    fn is_cursor_sidecar_recoverable_error_detects_bridge_failures() {
        assert!(is_cursor_sidecar_recoverable_error(
            "Bridge request failed: ConnectError: [Errno 111] Connection refused"
        ));
        assert!(is_cursor_sidecar_recoverable_error(
            "sidecar request failed: connection error"
        ));
        assert!(is_cursor_sidecar_recoverable_error(
            "sidecar HTTP 503: unavailable"
        ));
        assert!(is_cursor_sidecar_recoverable_error(
            "Cursor SDK startup failed: Timed out waiting for bridge discovery"
        ));
        assert!(is_cursor_sidecar_recoverable_error(
            "Cursor sidecar at capacity (CURSOR_RUN_CONCURRENCY=4)"
        ));
        assert!(!is_cursor_sidecar_recoverable_error("prompt required"));
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
    fn full_slim_prompt_includes_roles_and_hook_context() {
        use crate::channels::telegram::hook_turn_bridge::append_hook_context_messages;

        let mut messages = vec![
            Message {
                role: "user".into(),
                content: MessageContent::Text("hello".into()),
            },
            Message {
                role: "assistant".into(),
                content: MessageContent::Text("hi".into()),
            },
        ];
        append_hook_context_messages(&mut messages, &["scheduled policy".into()]);
        let header = DelegationRuntimeHeader {
            chat_id: 1,
            persona_id: 2,
            mcp_enabled: false,
        };
        let prompt = build_cursor_delegation_prompt(
            DelegationPromptMode::FullSlim,
            "sys",
            &messages,
            &header,
            false,
            false,
        );
        assert!(prompt.contains("# System"));
        assert!(prompt.contains("## user"));
        assert!(prompt.contains("hello"));
        assert!(prompt.contains("## assistant"));
        assert!(prompt.contains("[hook_context]"));
        assert!(prompt.contains("scheduled policy"));
    }
}
