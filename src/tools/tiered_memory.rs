//! Per-persona tiered memory (MEMORY.md) with Tier 1 (long-term), Tier 2 (mid-term), Tier 3 (short-term).

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::claude::ToolDefinition;

use super::{auth_context_from_input, authorize_chat_persona_access, schema_object, Tool, ToolResult};

const TIER_HEADERS: [(u8, &str); 3] = [
    (1, "## Tier 1 — Long term"),
    (2, "## Tier 2 — Mid term"),
    (3, "## Tier 3 — Short term"),
];

fn memory_path(groups_dir: &Path, chat_id: i64, persona_id: i64) -> PathBuf {
    groups_dir
        .join(chat_id.to_string())
        .join(persona_id.to_string())
        .join("MEMORY.md")
}

/// Parse MEMORY.md and extract one tier's content (between its header and the next ## or EOF).
fn parse_tier_content(full: &str, tier: u8) -> String {
    let header = TIER_HEADERS
        .iter()
        .find(|(n, _)| *n == tier)
        .map(|(_, h)| *h)
        .unwrap_or("");
    if header.is_empty() {
        return String::new();
    }
    let mut in_tier = false;
    let mut lines = Vec::new();
    for line in full.lines() {
        if line.starts_with("## ") {
            if line.trim() == header {
                in_tier = true;
                continue;
            }
            if in_tier {
                break;
            }
            continue;
        }
        if in_tier {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}

/// Replace content for one tier in the full markdown; preserve others. Creates template if needed.
fn replace_tier_content(full: &str, tier: u8, new_content: &str) -> String {
    let mut out = String::new();
    let header = TIER_HEADERS
        .iter()
        .find(|(n, _)| *n == tier)
        .map(|(_, h)| *h)
        .unwrap_or("");
    if header.is_empty() {
        return full.to_string();
    }

    let mut found_any = false;
    let mut current_tier: Option<u8> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    fn flush_tier(
        out: &mut String,
        tier_num: u8,
        lines: &[&str],
        replace_with: Option<&str>,
    ) {
        let (_, header) = TIER_HEADERS.iter().find(|(n, _)| *n == tier_num).unwrap();
        out.push_str(header);
        out.push('\n');
        out.push('\n');
        if let Some(s) = replace_with {
            out.push_str(s.trim());
            if !s.ends_with('\n') {
                out.push('\n');
            }
        } else {
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push('\n');
    }

    for line in full.lines() {
        if line.starts_with("## ") {
            if let Some(prev) = current_tier {
                let replace = if prev == tier {
                    Some(new_content)
                } else {
                    None
                };
                flush_tier(&mut out, prev, &current_lines, replace);
                current_lines.clear();
            }
            current_tier = TIER_HEADERS
                .iter()
                .find(|(_, h)| line.trim() == *h)
                .map(|(n, _)| *n);
            found_any = true;
            continue;
        }
        if let Some(_t) = current_tier {
            current_lines.push(line);
        } else if out.is_empty() {
            // Preamble before any tier
            out.push_str(line);
            out.push('\n');
        }
    }
    if let Some(prev) = current_tier {
        let replace = if prev == tier {
            Some(new_content)
        } else {
            None
        };
        flush_tier(&mut out, prev, &current_lines, replace);
    }

    if !found_any {
        // No tier headers found; produce full template with only this tier filled
        out.clear();
        out.push_str("# Memory\n\n");
        for (n, h) in &TIER_HEADERS {
            out.push_str(h);
            out.push_str("\n\n");
            if *n == tier {
                out.push_str(new_content.trim());
                if !new_content.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push('\n');
        }
    }
    out
}

pub struct ReadTieredMemoryTool {
    groups_dir: PathBuf,
}

impl ReadTieredMemoryTool {
    pub fn new(data_dir: &str) -> Self {
        ReadTieredMemoryTool {
            groups_dir: PathBuf::from(data_dir).join("groups"),
        }
    }
}

#[async_trait]
impl Tool for ReadTieredMemoryTool {
    fn name(&self) -> &str {
        "read_tiered_memory"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_tiered_memory".into(),
            description: "Read this persona's tiered memory (MEMORY.md). Optional tier (1, 2, or 3) returns only that section. Tier 1 = long-term principles-like; Tier 2 = active projects; Tier 3 = recent focus/mood.".into(),
            input_schema: schema_object(
                json!({
                    "chat_id": {
                        "type": "integer",
                        "description": "Chat ID (default: current chat from context)"
                    },
                    "persona_id": {
                        "type": "integer",
                        "description": "Persona ID (default: current persona from context)"
                    },
                    "tier": {
                        "type": "integer",
                        "description": "Optional: 1, 2, or 3 to return only that tier's content"
                    }
                }),
                &[],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => return ToolResult::error("Missing auth context".into()),
        };
        let chat_id = input
            .get("chat_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(auth.caller_chat_id);
        let persona_id = input
            .get("persona_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(auth.caller_persona_id);
        if let Err(e) = authorize_chat_persona_access(&input, chat_id, persona_id) {
            return ToolResult::error(e);
        }

        let path = memory_path(&self.groups_dir, chat_id, persona_id);
        info!("Reading tiered memory: {}", path.display());

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return ToolResult::success("No memory file found (not yet created).".into()),
        };

        let tier_opt = input.get("tier").and_then(|v| v.as_i64()).map(|n| n as u8);
        let result = if let Some(t) = tier_opt {
            if !(1..=3).contains(&t) {
                return ToolResult::error("tier must be 1, 2, or 3".into());
            }
            let section = parse_tier_content(&content, t);
            if section.is_empty() {
                format!("(Tier {} is empty.)", t)
            } else {
                section
            }
        } else {
            if content.trim().is_empty() {
                "Memory file is empty.".to_string()
            } else {
                content
            }
        };

        ToolResult::success(result)
    }
}

pub struct WriteTieredMemoryTool {
    groups_dir: PathBuf,
}

impl WriteTieredMemoryTool {
    pub fn new(data_dir: &str) -> Self {
        WriteTieredMemoryTool {
            groups_dir: PathBuf::from(data_dir).join("groups"),
        }
    }
}

#[async_trait]
impl Tool for WriteTieredMemoryTool {
    fn name(&self) -> &str {
        "write_tiered_memory"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_tiered_memory".into(),
            description: "Write one tier of this persona's MEMORY.md. Tier 1 = long-term (only on explicit user ask); Tier 2 = active projects; Tier 3 = recent focus/mood (update often; use past-tense status language, never 'awaiting/finalizing/TODO' — memory is context, not a task queue). Replaces that tier's section; other tiers are preserved.".into(),
            input_schema: schema_object(
                json!({
                    "chat_id": {
                        "type": "integer",
                        "description": "Chat ID"
                    },
                    "persona_id": {
                        "type": "integer",
                        "description": "Persona ID"
                    },
                    "tier": {
                        "type": "integer",
                        "description": "Tier to write: 1 (long-term), 2 (mid-term), or 3 (short-term)",
                        "enum": [1, 2, 3]
                    },
                    "content": {
                        "type": "string",
                        "description": "Markdown content for this tier (replaces existing)"
                    }
                }),
                &["tier", "content"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let auth = match auth_context_from_input(&input) {
            Some(a) => a,
            None => return ToolResult::error("Missing auth context".into()),
        };
        let chat_id = input
            .get("chat_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(auth.caller_chat_id);
        let persona_id = input
            .get("persona_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(auth.caller_persona_id);
        let tier = match input
            .get("tier")
            .and_then(|v| v.as_i64())
            .filter(|&n| (1..=3).contains(&n))
        {
            Some(n) => n as u8,
            None => return ToolResult::error("Missing or invalid 'tier' (must be 1, 2, or 3)".into()),
        };
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if let Err(e) = authorize_chat_persona_access(&input, chat_id, persona_id) {
            return ToolResult::error(e);
        }

        let path = memory_path(&self.groups_dir, chat_id, persona_id);
        info!("Writing tiered memory tier {}: {}", tier, path.display());

        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let new_content = if existing.trim().is_empty() {
            let mut out = String::from("# Memory\n\n");
            for (n, h) in &TIER_HEADERS {
                out.push_str(h);
                out.push_str("\n\n");
                if *n == tier {
                    out.push_str(content.trim());
                    if !content.ends_with('\n') {
                        out.push('\n');
                    }
                }
                out.push('\n');
            }
            out
        } else {
            replace_tier_content(&existing, tier, content)
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ToolResult::error(format!("Failed to create directory: {e}"));
            }
        }

        match std::fs::write(&path, new_content) {
            Ok(()) => ToolResult::success(format!("Tier {} updated.", tier)),
            Err(e) => ToolResult::error(format!("Failed to write memory: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tier_content() {
        let md = r#"# Memory

## Tier 1 — Long term
One line.

## Tier 2 — Mid term
Two
lines.

## Tier 3 — Short term
Three."#;
        assert_eq!(parse_tier_content(md, 1), "One line.");
        assert_eq!(parse_tier_content(md, 2), "Two\nlines.");
        assert_eq!(parse_tier_content(md, 3), "Three.");
    }

    #[test]
    fn test_replace_tier_preserves_others() {
        let md = r#"# Memory

## Tier 1 — Long term
Old T1

## Tier 2 — Mid term
Old T2

## Tier 3 — Short term
Old T3"#;
        let new = replace_tier_content(md, 2, "New T2 content");
        assert!(new.contains("Old T1"));
        assert!(new.contains("New T2 content"));
        assert!(new.contains("Old T3"));
    }
}
