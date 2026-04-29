//! Per-persona tiered memory backed by canonical memory_state.json.

use async_trait::async_trait;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tracing::info;

use crate::claude::ToolDefinition;
use crate::memory::{ActiveProjectMemory, MemoryManager, PersonaMemoryState};

use super::{
    auth_context_from_input, authorize_chat_persona_access, schema_object, Tool, ToolResult,
};

fn parse_tier_content(state: &PersonaMemoryState, tier: u8) -> String {
    match tier {
        1 => {
            let mut lines = Vec::new();
            if !state.identity.display_name.trim().is_empty() {
                lines.push(format!(
                    "- Identity|display_name={}",
                    state.identity.display_name.trim()
                ));
            }
            if !state.identity.self_model.trim().is_empty() {
                lines.push(format!(
                    "- Identity|self_model={}",
                    state.identity.self_model.trim()
                ));
            }
            if !state.identity.voice_style.trim().is_empty() {
                lines.push(format!(
                    "- Identity|voice_style={}",
                    state.identity.voice_style.trim()
                ));
            }
            lines.extend(
                state
                    .identity
                    .non_negotiables
                    .iter()
                    .map(|v| format!("- IdentityConstraint|{v}")),
            );
            lines.extend(state.tier1.stable_facts.clone());
            lines.extend(
                state
                    .tier1
                    .workflow_principles
                    .iter()
                    .map(|v| format!("- WorkflowPrinciple|{v}")),
            );
            lines.join("\n").trim().to_string()
        }
        2 => state
            .tier2
            .active_projects
            .iter()
            .map(|p| {
                format!(
                    "- ProjectState|id={}|status={}|updated={}|summary={}",
                    p.id, p.status, p.updated_at, p.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        3 => state.tier3.recent_focus.join("\n").trim().to_string(),
        _ => String::new(),
    }
}

fn normalize_tier2_task_states(content: &str) -> String {
    let mut out = Vec::new();
    let mut seen_exact = HashSet::new();
    let mut last_next_goal: Option<String> = None;
    let mut task_state_latest: HashMap<String, String> = HashMap::new();
    let mut task_state_order: Vec<String> = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("- next goal:") {
            last_next_goal = Some(trimmed.to_string());
            continue;
        }
        // Canonical state line format:
        // - TaskState|key=<task_key>|status=<queued|running|stalled|completed|cancelled>|updated=<iso>|evidence=<summary>
        if let Some(rest) = trimmed.strip_prefix("- TaskState|key=") {
            let key = rest.split('|').next().unwrap_or("").trim().to_string();
            if !key.is_empty() {
                if !task_state_latest.contains_key(&key) {
                    task_state_order.push(key.clone());
                }
                task_state_latest.insert(key, trimmed.to_string());
                continue;
            }
        }
        if seen_exact.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }

    if !task_state_latest.is_empty() {
        for key in task_state_order {
            if let Some(line) = task_state_latest.get(&key) {
                out.push(line.clone());
            }
        }
    }
    if let Some(goal) = last_next_goal {
        out.push(goal);
    }

    out.join("\n")
}

fn normalize_tier3_recent_focus(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out.into_iter().take(15).collect()
}

fn parse_project_state_line(line: &str) -> Option<ActiveProjectMemory> {
    let rest = line.strip_prefix("- ProjectState|")?;
    let mut id = String::new();
    let mut status = String::new();
    let mut updated_at = String::new();
    let mut summary = String::new();
    for part in rest.split('|') {
        let mut kv = part.splitn(2, '=');
        let key = kv.next()?.trim();
        let value = kv.next().unwrap_or("").trim();
        match key {
            "id" => id = value.to_string(),
            "status" => status = value.to_string(),
            "updated" => updated_at = value.to_string(),
            "summary" => summary = value.to_string(),
            _ => {}
        }
    }
    if summary.is_empty() {
        return None;
    }
    Some(ActiveProjectMemory {
        id,
        status,
        summary,
        updated_at,
    })
}

fn apply_tier_write(state: &mut PersonaMemoryState, tier: u8, content: &str) {
    match tier {
        1 => {
            let mut stable_facts = Vec::new();
            let mut workflow_principles = Vec::new();
            let mut identity_constraints = Vec::new();
            for raw_line in content.lines() {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(v) = line.strip_prefix("- Identity|display_name=") {
                    state.identity.display_name = v.trim().to_string();
                    continue;
                }
                if let Some(v) = line.strip_prefix("- Identity|self_model=") {
                    state.identity.self_model = v.trim().to_string();
                    continue;
                }
                if let Some(v) = line.strip_prefix("- Identity|voice_style=") {
                    state.identity.voice_style = v.trim().to_string();
                    continue;
                }
                if let Some(v) = line.strip_prefix("- IdentityConstraint|") {
                    identity_constraints.push(v.trim().to_string());
                    continue;
                }
                if let Some(v) = line.strip_prefix("- WorkflowPrinciple|") {
                    workflow_principles.push(v.trim().to_string());
                    continue;
                }
                stable_facts.push(line.to_string());
            }
            state.identity.non_negotiables = identity_constraints;
            state.tier1.stable_facts = stable_facts;
            state.tier1.workflow_principles = workflow_principles;
        }
        2 => {
            let normalized = normalize_tier2_task_states(content);
            let mut projects = Vec::new();
            for raw_line in normalized.lines() {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(project) = parse_project_state_line(line) {
                    projects.push(project);
                } else {
                    projects.push(ActiveProjectMemory {
                        id: String::new(),
                        status: "active".to_string(),
                        summary: line.to_string(),
                        updated_at: String::new(),
                    });
                }
            }
            state.tier2.active_projects = projects;
        }
        3 => {
            state.tier3.recent_focus = normalize_tier3_recent_focus(content);
        }
        _ => {}
    }
}

pub struct ReadTieredMemoryTool {
    memory: MemoryManager,
}

impl ReadTieredMemoryTool {
    pub fn new(data_dir: &str) -> Self {
        ReadTieredMemoryTool {
            memory: MemoryManager::new(data_dir, data_dir),
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
            description: "Read this persona's tiered memory from canonical memory_state.json (legacy MEMORY.md auto-migrates). Optional tier (1, 2, or 3) returns only that section.".into(),
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

        let state = match self
            .memory
            .read_or_migrate_persona_memory_state(chat_id, persona_id)
        {
            Some(s) => s,
            None => {
                return ToolResult::success(
                    "No canonical memory state found (not yet created).".into(),
                )
            }
        };
        let state_path = self.memory.persona_memory_state_path(chat_id, persona_id);
        info!("Reading tiered memory: {}", state_path.display());

        let tier_opt = input.get("tier").and_then(|v| v.as_i64()).map(|n| n as u8);
        let result = if let Some(t) = tier_opt {
            if !(1..=3).contains(&t) {
                return ToolResult::error("tier must be 1, 2, or 3".into());
            }
            let section = parse_tier_content(&state, t);
            if section.is_empty() {
                format!("(Tier {} is empty.)", t)
            } else {
                section
            }
        } else {
            crate::memory::render_memory_markdown(&state)
        };

        ToolResult::success(result)
    }
}

pub struct WriteTieredMemoryTool {
    memory: MemoryManager,
}

impl WriteTieredMemoryTool {
    pub fn new(data_dir: &str) -> Self {
        WriteTieredMemoryTool {
            memory: MemoryManager::new(data_dir, data_dir),
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
            description: "Write one tier of canonical memory_state.json. Tier 1 = long-term (only on explicit user ask); Tier 2 = active projects; Tier 3 = recent focus/mood. Replaces only that tier's section.".into(),
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
                        "description": "Text content for this tier (replaces existing content in that tier)"
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
            None => {
                return ToolResult::error("Missing or invalid 'tier' (must be 1, 2, or 3)".into())
            }
        };
        let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");

        if let Err(e) = authorize_chat_persona_access(&input, chat_id, persona_id) {
            return ToolResult::error(e);
        }

        let mut state = self
            .memory
            .read_or_migrate_persona_memory_state(chat_id, persona_id)
            .unwrap_or_default();
        apply_tier_write(&mut state, tier, content);
        let state_path = self.memory.persona_memory_state_path(chat_id, persona_id);
        info!(
            "Writing tiered memory tier {} to canonical JSON: {}",
            tier,
            state_path.display()
        );
        match self
            .memory
            .write_persona_memory_state(chat_id, persona_id, state)
        {
            Ok(()) => {
                let _ = self.memory.append_persona_memory_event(
                    chat_id,
                    persona_id,
                    "tier_write",
                    "agent_auto",
                    json!({
                        "tier": tier,
                        "state_path": state_path.to_string_lossy().to_string(),
                    }),
                );
                ToolResult::success(format!("Tier {} updated in canonical memory state.", tier))
            }
            Err(e) => ToolResult::error(format!("Failed to write canonical memory state: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tier_content_from_state() {
        let mut state = PersonaMemoryState::default();
        state.identity.display_name = "Nova".into();
        state.tier1.stable_facts = vec!["- stable fact".into()];
        state.tier2.active_projects = vec![ActiveProjectMemory {
            id: "project-1".into(),
            status: "active".into(),
            summary: "Build memory model".into(),
            updated_at: "2026-04-27T00:00:00Z".into(),
        }];
        state.tier3.recent_focus = vec!["- recent".into()];
        assert!(parse_tier_content(&state, 1).contains("Identity|display_name=Nova"));
        assert!(parse_tier_content(&state, 2).contains("Build memory model"));
        assert_eq!(parse_tier_content(&state, 3), "- recent");
    }

    #[test]
    fn test_apply_tier_write_identity_and_principles() {
        let mut state = PersonaMemoryState::default();
        let content = "\
- Identity|display_name=KenAssistant
- Identity|voice_style=concise
- IdentityConstraint|Do not hallucinate
- WorkflowPrinciple|Check run history before claiming no memory
- Stable fact line";
        apply_tier_write(&mut state, 1, content);
        assert_eq!(state.identity.display_name, "KenAssistant");
        assert_eq!(state.identity.voice_style, "concise");
        assert_eq!(state.identity.non_negotiables.len(), 1);
        assert_eq!(state.tier1.workflow_principles.len(), 1);
        assert_eq!(state.tier1.stable_facts.len(), 1);
    }

    #[test]
    fn test_normalize_tier2_task_states_dedupes_next_goal_and_taskstate() {
        let input = r#"
- Keep this.
- Next Goal: old one
- TaskState|key=swap:pz-20260330|status=running|updated=2026-04-01T01:00:00Z|evidence=queued
- TaskState|key=swap:pz-20260330|status=stalled|updated=2026-04-01T02:00:00Z|evidence=timeout
- Next Goal: latest one
- Keep this.
"#;
        let out = normalize_tier2_task_states(input);
        assert_eq!(out.matches("TaskState|key=swap:pz-20260330").count(), 1);
        assert!(out.contains("status=stalled"));
        assert_eq!(out.matches("Next Goal:").count(), 1);
        assert!(out.contains("latest one"));
    }

    #[test]
    fn test_normalize_tier3_recent_focus_dedupes_lines() {
        let input = r#"
- monitoring queue
- monitoring queue
- checking output
"#;
        let out = normalize_tier3_recent_focus(input);
        assert_eq!(
            out.iter()
                .filter(|l| l.contains("monitoring queue"))
                .count(),
            1
        );
        assert_eq!(
            out.iter().filter(|l| l.contains("checking output")).count(),
            1
        );
    }

    #[test]
    fn test_parse_project_state_line() {
        let line = "- ProjectState|id=proj-a|status=running|updated=2026-04-27T00:00:00Z|summary=Ship migration";
        let project = parse_project_state_line(line).unwrap();
        assert_eq!(project.id, "proj-a");
        assert_eq!(project.status, "running");
        assert_eq!(project.summary, "Ship migration");
    }
}
