//! Agent run history: records and persists detailed per-run traces so the agent
//! can later read them for self-improvement and workflow optimization.

use chrono::{DateTime, Utc};
use std::path::PathBuf;
use tracing::info;

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
