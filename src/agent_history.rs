//! Agent run history: records and persists detailed per-run traces so the agent
//! can later read them for self-improvement and workflow optimization.

use chrono::{DateTime, Utc};
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::claude::{ContentBlock, Message, MessageContent};

/// Max UTF-8 bytes for the JSON blob appended under [`SNAPSHOT_SECTION_START`] in run markdown.
pub const MAX_INITIAL_LLM_SNAPSHOT_BYTES: usize = 800 * 1024;

/// Markdown delimiter between the iteration trace and the optional first-turn LLM snapshot (web + debugging).
pub const SNAPSHOT_SECTION_START: &str = "\n## Initial LLM prompt (debug snapshot)\n";

/// Appended after the main run body when post-delivery quality evaluation runs.
pub const QUALITY_EVAL_SECTION_START: &str = "\n## Post-delivery quality evaluation\n";

/// Max bytes read for a single agent history file (web UI / API).
pub const MAX_AGENT_HISTORY_READ_BYTES: u64 = 4 * 1024 * 1024;

/// Basename must be `YYYYMMDD-HHMMSS.md` (same as `write_agent_history_run`).
pub fn is_valid_agent_history_filename(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() != 18 {
        return false;
    }
    for i in 0..8 {
        if !b[i].is_ascii_digit() {
            return false;
        }
    }
    if b[8] != b'-' {
        return false;
    }
    for i in 9..15 {
        if !b[i].is_ascii_digit() {
            return false;
        }
    }
    b[15..] == *b".md"
}

/// Lists `YYYYMMDD-HHMMSS.md` basenames under `dir`, sorted ascending (oldest first).
pub fn list_agent_history_md_basenames_sorted(dir: &Path) -> std::io::Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.extension().map_or(false, |e| e == "md") {
            continue;
        }
        let os_name = entry.file_name();
        let Some(name) = os_name.to_str() else {
            continue;
        };
        if is_valid_agent_history_filename(name) {
            out.push(name.to_string());
        }
    }
    out.sort();
    Ok(out)
}

pub struct LatestAgentHistoryRead {
    pub filename: String,
    pub path: PathBuf,
    pub content: String,
    pub mtime_ms: i64,
}

#[derive(Debug)]
pub enum ReadLatestAgentHistoryError {
    Io(std::io::Error),
    FileTooLarge(u64),
}

impl std::fmt::Display for ReadLatestAgentHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadLatestAgentHistoryError::Io(e) => write!(f, "{e}"),
            ReadLatestAgentHistoryError::FileTooLarge(n) => {
                write!(f, "agent history file too large ({n} bytes)")
            }
        }
    }
}

impl std::error::Error for ReadLatestAgentHistoryError {}

/// Reads the newest valid `.md` run file for this persona, if any.
pub fn read_latest_agent_history(
    data_dir: &str,
    chat_id: i64,
    persona_id: i64,
) -> Result<Option<LatestAgentHistoryRead>, ReadLatestAgentHistoryError> {
    let dir = history_dir(data_dir, chat_id, persona_id);
    let basenames =
        list_agent_history_md_basenames_sorted(&dir).map_err(ReadLatestAgentHistoryError::Io)?;
    let Some(newest) = basenames.last() else {
        return Ok(None);
    };
    let full_path = dir.join(newest);
    let meta = std::fs::metadata(&full_path).map_err(ReadLatestAgentHistoryError::Io)?;
    let len = meta.len();
    if len > MAX_AGENT_HISTORY_READ_BYTES {
        return Err(ReadLatestAgentHistoryError::FileTooLarge(len));
    }
    let content = std::fs::read_to_string(&full_path).map_err(ReadLatestAgentHistoryError::Io)?;
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(Some(LatestAgentHistoryRead {
        filename: newest.clone(),
        path: full_path,
        content,
        mtime_ms,
    }))
}

const MAX_HISTORY_FILES: usize = 50;

pub struct ToolCallRecord {
    pub name: String,
    pub input_preview: String,
    pub result_preview: String,
    pub duration_ms: u128,
    pub is_error: bool,
}

pub struct IterationRecord {
    pub iteration: usize,
    pub stop_reason: String,
    pub assistant_text_preview: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub hook_events: Vec<String>,
}

pub struct AgentRunRecord {
    pub timestamp: DateTime<Utc>,
    pub channel: String,
    pub user_message_preview: String,
    pub iterations: Vec<IterationRecord>,
    pub total_iterations: usize,
    pub stop_reason: String,
    pub total_duration_ms: u128,
    /// JSON (pretty) of `system_prompt`, `tool_names_first_turn`, and `messages` as sent on the first LLM call.
    pub initial_llm_snapshot: Option<String>,
}

fn history_dir(data_dir: &str, chat_id: i64, persona_id: i64) -> PathBuf {
    PathBuf::from(data_dir)
        .join("groups")
        .join(chat_id.to_string())
        .join(persona_id.to_string())
        .join("agent_history")
}

/// Return the history directory path (for the read tool).
pub fn history_dir_path(data_dir: &str, chat_id: i64, persona_id: i64) -> PathBuf {
    history_dir(data_dir, chat_id, persona_id)
}

impl AgentRunRecord {
    pub fn to_markdown(&self) -> String {
        let mut md = String::with_capacity(32768);

        md.push_str(&format!(
            "# Run {}\nChannel: {} | User: \"{}\"\nTotal: {} iteration(s) | Stop: {} | Duration: {} ms\n",
            self.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            self.channel,
            self.user_message_preview,
            self.total_iterations,
            self.stop_reason,
            self.total_duration_ms,
        ));

        for iter in &self.iterations {
            md.push_str(&format!(
                "\n## Iteration {}\nStop: {}\n",
                iter.iteration, iter.stop_reason
            ));

            if !iter.tool_calls.is_empty() {
                for tc in &iter.tool_calls {
                    let status = if tc.is_error { "ERR" } else { "OK" };
                    md.push_str(&format!(
                        "- Tool: {} ({}ms) {} — input: {} → result: {}\n",
                        tc.name, tc.duration_ms, status, tc.input_preview, tc.result_preview
                    ));
                }
            }
            if !iter.hook_events.is_empty() {
                for hook in &iter.hook_events {
                    md.push_str(&format!("- Hook: {}\n", hook));
                }
            }

            if !iter.assistant_text_preview.is_empty() {
                md.push_str(&format!("Assistant: \"{}\"\n", iter.assistant_text_preview));
            }
        }

        if let Some(ref snap) = self.initial_llm_snapshot {
            if !snap.trim().is_empty() {
                md.push_str(SNAPSHOT_SECTION_START);
                md.push_str(snap);
            }
        }

        md
    }
}

fn snap_block(b: &ContentBlock) -> serde_json::Value {
    match b {
        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
        ContentBlock::Image { source } => json!({
            "type": "image_omitted",
            "media_type": &source.media_type,
            "approx_base64_chars": source.data.len(),
        }),
        ContentBlock::ToolUse {
            id,
            name,
            input,
            thought_signature,
        } => json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
            "thought_signature": thought_signature,
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content,
            "is_error": is_error,
        }),
    }
}

fn snap_message_content(c: &MessageContent) -> serde_json::Value {
    match c {
        MessageContent::Text(t) => json!(t),
        MessageContent::Blocks(blocks) => json!(blocks.iter().map(snap_block).collect::<Vec<_>>()),
    }
}

fn snap_message(m: &Message) -> serde_json::Value {
    json!({
        "role": &m.role,
        "content": snap_message_content(&m.content),
    })
}

/// Pretty JSON: system prompt, tool names for the first turn, and messages (image payloads summarized).
pub fn format_initial_llm_snapshot_json(
    system_prompt: &str,
    messages: &[Message],
    tool_names: &[String],
) -> String {
    let messages_json: Vec<serde_json::Value> = messages.iter().map(snap_message).collect();
    let root = json!({
        "schema": "initial_llm_request_v1",
        "system_prompt": system_prompt,
        "tool_names_first_turn": tool_names,
        "messages": messages_json,
    });
    let mut s = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string());
    if s.len() > MAX_INITIAL_LLM_SNAPSHOT_BYTES {
        let mut end = MAX_INITIAL_LLM_SNAPSHOT_BYTES.saturating_sub(120);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n…\n(snapshot truncated for storage)\n");
    }
    s
}

/// Append one PDQE (or similar) step to an existing run markdown file.
pub fn append_pdqe_step_to_agent_history(
    data_dir: &str,
    chat_id: i64,
    persona_id: i64,
    basename: &str,
    step: &str,
    detail: &str,
) -> std::io::Result<()> {
    if !is_valid_agent_history_filename(basename) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid agent history basename",
        ));
    }
    let path = history_dir(data_dir, chat_id, persona_id).join(basename);
    let mut content = std::fs::read_to_string(&path)?;
    if !content.contains(QUALITY_EVAL_SECTION_START) {
        content.push_str(QUALITY_EVAL_SECTION_START);
    }
    let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    content.push_str(&format!(
        "- **{at}** `{step}`{}\n",
        if detail.trim().is_empty() {
            String::new()
        } else {
            format!(" — {}", detail.trim())
        }
    ));
    std::fs::write(&path, content)
}

/// Persist a run record to disk. Rotates old files if count exceeds MAX_HISTORY_FILES.
/// Returns the written basename (`YYYYMMDD-HHMMSS.md`) on success.
pub fn write_agent_history_run(
    data_dir: &str,
    chat_id: i64,
    persona_id: i64,
    record: &AgentRunRecord,
) -> Option<String> {
    let dir = history_dir(data_dir, chat_id, persona_id);

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("Failed to create agent_history dir {}: {e}", dir.display());
        return None;
    }

    let filename = format!("{}.md", record.timestamp.format("%Y%m%d-%H%M%S"));
    let path = dir.join(&filename);
    let content = record.to_markdown();

    if let Err(e) = std::fs::write(&path, &content) {
        tracing::warn!("Failed to write agent history to {}: {e}", path.display());
        return None;
    }

    info!(
        "Agent history saved: {} ({} bytes, {} iterations)",
        path.display(),
        content.len(),
        record.total_iterations
    );

    rotate_old_files(&dir);
    Some(filename)
}

fn rotate_old_files(dir: &PathBuf) {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
            .collect(),
        Err(_) => return,
    };

    if entries.len() <= MAX_HISTORY_FILES {
        return;
    }

    entries.sort_by_key(|e| e.file_name());
    let to_remove = entries.len() - MAX_HISTORY_FILES;
    for entry in entries.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(entry.path());
    }
}

/// Truncate a string to `max_chars`, appending "..." if truncated.
/// Avoids splitting mid-character.
pub fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let boundary = s.floor_char_boundary(max_chars);
    format!("{}...", &s[..boundary])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn append_pdqe_step_adds_section_and_entries() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_str().unwrap();
        let chat_id = 1_i64;
        let persona_id = 2_i64;
        let record = AgentRunRecord {
            timestamp: Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap(),
            channel: "web".into(),
            user_message_preview: "hello".into(),
            iterations: vec![],
            total_iterations: 0,
            stop_reason: "end_turn".into(),
            total_duration_ms: 1,
            initial_llm_snapshot: None,
        };
        let basename = write_agent_history_run(data_dir, chat_id, persona_id, &record).unwrap();
        append_pdqe_step_to_agent_history(
            data_dir,
            chat_id,
            persona_id,
            &basename,
            "quality_eval_started",
            "run-1",
        )
        .unwrap();
        append_pdqe_step_to_agent_history(
            data_dir,
            chat_id,
            persona_id,
            &basename,
            "quality_eval_pass",
            "confidence=0.95",
        )
        .unwrap();
        let content = std::fs::read_to_string(
            history_dir_path(data_dir, chat_id, persona_id).join(&basename),
        )
        .unwrap();
        assert!(content.contains(QUALITY_EVAL_SECTION_START.trim()));
        assert!(content.contains("`quality_eval_started`"));
        assert!(content.contains("`quality_eval_pass`"));
        assert!(content.contains("confidence=0.95"));
    }
}
