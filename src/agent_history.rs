//! Agent run history: records and persists detailed per-run traces so the agent
//! can later read them for self-improvement and workflow optimization.

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tracing::info;

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
}

pub struct AgentRunRecord {
    pub timestamp: DateTime<Utc>,
    pub channel: String,
    pub user_message_preview: String,
    pub iterations: Vec<IterationRecord>,
    pub total_iterations: usize,
    pub stop_reason: String,
    pub total_duration_ms: u128,
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

            if !iter.assistant_text_preview.is_empty() {
                md.push_str(&format!("Assistant: \"{}\"\n", iter.assistant_text_preview));
            }
        }

        md
    }
}

/// Persist a run record to disk. Rotates old files if count exceeds MAX_HISTORY_FILES.
pub fn write_agent_history_run(
    data_dir: &str,
    chat_id: i64,
    persona_id: i64,
    record: &AgentRunRecord,
) {
    let dir = history_dir(data_dir, chat_id, persona_id);

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("Failed to create agent_history dir {}: {e}", dir.display());
        return;
    }

    let filename = format!("{}.md", record.timestamp.format("%Y%m%d-%H%M%S"));
    let path = dir.join(&filename);
    let content = record.to_markdown();

    if let Err(e) = std::fs::write(&path, &content) {
        tracing::warn!("Failed to write agent history to {}: {e}", path.display());
        return;
    }

    info!(
        "Agent history saved: {} ({} bytes, {} iterations)",
        path.display(),
        content.len(),
        record.total_iterations
    );

    rotate_old_files(&dir);
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
