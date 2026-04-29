use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

const MEMORY_SCHEMA_VERSION: u32 = 1;
const MEMORY_STATE_FILE: &str = "memory_state.json";
const MEMORY_EVENTS_FILE: &str = "memory_events.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryStateMeta {
    #[serde(default = "default_memory_schema_version")]
    pub version: u32,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub updated_at: String,
}

fn default_memory_schema_version() -> u32 {
    MEMORY_SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdentityMemory {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub self_model: String,
    #[serde(default)]
    pub voice_style: String,
    #[serde(default)]
    pub non_negotiables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tier1Memory {
    #[serde(default)]
    pub stable_facts: Vec<String>,
    #[serde(default)]
    pub workflow_principles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActiveProjectMemory {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tier2Memory {
    #[serde(default)]
    pub active_projects: Vec<ActiveProjectMemory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tier3Memory {
    #[serde(default)]
    pub recent_focus: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowMemoryEntry {
    #[serde(default)]
    pub intent_signature: String,
    #[serde(default)]
    pub approach_summary: String,
    #[serde(default)]
    pub step_trace: Vec<String>,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub support_count: u64,
    #[serde(default)]
    pub last_seen_at: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowMemory {
    #[serde(default)]
    pub intents: Vec<WorkflowMemoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryLinks {
    #[serde(default)]
    pub mem_palace_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonaMemoryState {
    #[serde(default)]
    pub meta: MemoryStateMeta,
    #[serde(default)]
    pub identity: IdentityMemory,
    #[serde(default)]
    pub tier1: Tier1Memory,
    #[serde(default)]
    pub tier2: Tier2Memory,
    #[serde(default)]
    pub tier3: Tier3Memory,
    #[serde(default)]
    pub workflow_memory: WorkflowMemory,
    #[serde(default)]
    pub links: MemoryLinks,
}

impl PersonaMemoryState {
    pub fn normalize(&mut self) {
        self.meta.version = MEMORY_SCHEMA_VERSION;
        self.identity.non_negotiables = dedupe_trimmed_lines(&self.identity.non_negotiables);
        self.tier1.stable_facts = dedupe_trimmed_lines(&self.tier1.stable_facts);
        self.tier1.workflow_principles = dedupe_trimmed_lines(&self.tier1.workflow_principles);
        self.tier3.recent_focus = dedupe_trimmed_lines(&self.tier3.recent_focus)
            .into_iter()
            .take(15)
            .collect();
        self.links.mem_palace_refs = dedupe_trimmed_lines(&self.links.mem_palace_refs);

        let now = Utc::now().to_rfc3339();
        if self.meta.updated_at.trim().is_empty() {
            self.meta.updated_at = now.clone();
        }

        let mut seen_projects = HashSet::new();
        self.tier2.active_projects = self
            .tier2
            .active_projects
            .drain(..)
            .filter_map(|mut p| {
                p.summary = p.summary.trim().to_string();
                if p.summary.is_empty() {
                    return None;
                }
                if p.id.trim().is_empty() {
                    p.id = deterministic_key_from_text(&p.summary);
                }
                if p.status.trim().is_empty() {
                    p.status = "active".to_string();
                }
                if p.updated_at.trim().is_empty() {
                    p.updated_at = now.clone();
                }
                if seen_projects.insert(p.id.clone()) {
                    Some(p)
                } else {
                    None
                }
            })
            .collect();

        let mut by_intent: HashMap<String, WorkflowMemoryEntry> = HashMap::new();
        for mut item in self.workflow_memory.intents.drain(..) {
            item.intent_signature = item.intent_signature.trim().to_ascii_lowercase();
            if item.intent_signature.is_empty() {
                continue;
            }
            item.approach_summary = item.approach_summary.trim().to_string();
            item.step_trace = dedupe_trimmed_lines(&item.step_trace);
            item.evidence_refs = dedupe_trimmed_lines(&item.evidence_refs);
            item.confidence = item.confidence.clamp(0.0, 1.0);
            if item.outcome.trim().is_empty() {
                item.outcome = "unknown".to_string();
            }
            if item.last_seen_at.trim().is_empty() {
                item.last_seen_at = now.clone();
            }
            match by_intent.get(&item.intent_signature) {
                Some(existing) if existing.support_count >= item.support_count => {}
                _ => {
                    by_intent.insert(item.intent_signature.clone(), item);
                }
            }
        }
        self.workflow_memory.intents = by_intent.into_values().collect();
        self.workflow_memory
            .intents
            .sort_by(|a, b| b.support_count.cmp(&a.support_count));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub ts: String,
    pub event_type: String,
    pub actor: String,
    pub chat_id: i64,
    pub persona_id: i64,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct MemoryManager {
    /// Directory containing groups/ (for per-chat memory and daily logs).
    data_dir: PathBuf,
    /// Global AGENTS.md is read/written from workspace root shared/AGENTS.md (single source of truth).
    working_dir: PathBuf,
    /// Optional override for principles path (relative to working_dir). Default: AGENTS.md at workspace root.
    principles_path_override: Option<String>,
}

impl MemoryManager {
    pub fn new(data_dir: &str, working_dir: &str) -> Self {
        MemoryManager::with_principles_path(data_dir, working_dir, None)
    }

    /// Create MemoryManager with optional principles path override (e.g. "shared/ORIGIN/AGENTS.md").
    pub fn with_principles_path(
        data_dir: &str,
        working_dir: &str,
        principles_path_override: Option<String>,
    ) -> Self {
        MemoryManager {
            data_dir: PathBuf::from(data_dir).join("groups"),
            working_dir: PathBuf::from(working_dir),
            principles_path_override: principles_path_override.filter(|p| !p.trim().is_empty()),
        }
    }

    /// Path for global principles/memory: workspace root shared/AGENTS.md (single source of truth).
    fn global_memory_path(&self) -> PathBuf {
        self.working_dir.join("shared").join("AGENTS.md")
    }

    fn chat_memory_path(&self, chat_id: i64) -> PathBuf {
        self.data_dir.join(chat_id.to_string()).join("AGENTS.md")
    }

    /// Path for shared principles for all chats/personas: workspace_dir/AGENTS.md (at workspace root),
    /// or vault.principles_path if configured (e.g. shared/ORIGIN/AGENTS.md).
    pub fn groups_root_memory_path(&self) -> PathBuf {
        if let Some(ref p) = self.principles_path_override {
            self.working_dir.join(p.trim().trim_start_matches('/'))
        } else {
            self.working_dir.join("AGENTS.md")
        }
    }

    /// Path string for AGENTS.md (principles, for display in system prompt).
    pub fn groups_root_memory_path_display(&self) -> String {
        self.groups_root_memory_path().to_string_lossy().to_string()
    }

    /// Path for per-persona tiered memory: groups/{chat_id}/{persona_id}/MEMORY.md.
    pub fn persona_memory_path(&self, chat_id: i64, persona_id: i64) -> PathBuf {
        self.data_dir
            .join(chat_id.to_string())
            .join(persona_id.to_string())
            .join("MEMORY.md")
    }

    /// Path for canonical per-persona memory state.
    pub fn persona_memory_state_path(&self, chat_id: i64, persona_id: i64) -> PathBuf {
        self.data_dir
            .join(chat_id.to_string())
            .join(persona_id.to_string())
            .join(MEMORY_STATE_FILE)
    }

    /// Path for append-only canonical memory events.
    pub fn persona_memory_events_path(&self, chat_id: i64, persona_id: i64) -> PathBuf {
        self.data_dir
            .join(chat_id.to_string())
            .join(persona_id.to_string())
            .join(MEMORY_EVENTS_FILE)
    }

    /// Path for per-persona daily log: `groups/{chat_id}/{persona_id}/memory/YYYY-MM-DD.md`
    fn daily_log_path(&self, chat_id: i64, persona_id: i64, date: &str) -> PathBuf {
        self.data_dir
            .join(chat_id.to_string())
            .join(persona_id.to_string())
            .join("memory")
            .join(format!("{date}.md"))
    }

    pub fn read_global_memory(&self) -> Option<String> {
        let path = self.global_memory_path();
        std::fs::read_to_string(path).ok()
    }

    /// Read shared AGENTS.md at workspace root. Used as principles for all personas.
    pub fn read_groups_root_memory(&self) -> Option<String> {
        let path = self.groups_root_memory_path();
        std::fs::read_to_string(path).ok()
    }

    /// Write principles to the same path as [`Self::read_groups_root_memory`] (workspace AGENTS.md or `vault.principles_path`).
    pub fn write_groups_root_memory(&self, content: &str) -> std::io::Result<()> {
        let path = self.groups_root_memory_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    pub fn read_chat_memory(&self, chat_id: i64) -> Option<String> {
        let path = self.chat_memory_path(chat_id);
        std::fs::read_to_string(path).ok()
    }

    /// Read per-persona tiered memory from canonical JSON state.
    /// Falls back to legacy MEMORY.md and migrates when possible.
    pub fn read_persona_memory(&self, chat_id: i64, persona_id: i64) -> Option<String> {
        self.read_or_migrate_persona_memory_state(chat_id, persona_id)
            .map(|s| render_memory_markdown(&s))
            .or_else(|| std::fs::read_to_string(self.persona_memory_path(chat_id, persona_id)).ok())
    }

    /// Read a single daily log file if it exists. `date` must be "YYYY-MM-DD".
    pub fn read_daily_log(&self, chat_id: i64, persona_id: i64, date: &str) -> Option<String> {
        let path = self.daily_log_path(chat_id, persona_id, date);
        std::fs::read_to_string(path).ok()
    }

    /// Read today's and yesterday's daily logs and return combined content for injection.
    /// Returns empty string if neither file exists.
    pub fn read_daily_logs_today_yesterday(&self, chat_id: i64, persona_id: i64) -> String {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let yesterday = (Utc::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let mut out = String::new();
        if let Some(content) = self.read_daily_log(chat_id, persona_id, &yesterday) {
            if !content.trim().is_empty() {
                out.push_str(&format!("## {yesterday}\n{content}\n\n"));
            }
        }
        if let Some(content) = self.read_daily_log(chat_id, persona_id, &today) {
            if !content.trim().is_empty() {
                out.push_str(&format!("## {today}\n{content}\n\n"));
            }
        }
        out.trim().to_string()
    }

    /// Append content to the daily log for the given date. Creates file and parent dir if needed.
    /// `date` must be "YYYY-MM-DD".
    pub fn append_daily_log(
        &self,
        chat_id: i64,
        persona_id: i64,
        date: &str,
        content: &str,
    ) -> std::io::Result<()> {
        let path = self.daily_log_path(chat_id, persona_id, date);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        if !content.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        f.write_all(content.as_bytes())?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn write_global_memory(&self, content: &str) -> std::io::Result<()> {
        let path = self.global_memory_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    #[allow(dead_code)]
    pub fn write_chat_memory(&self, chat_id: i64, content: &str) -> std::io::Result<()> {
        let path = self.chat_memory_path(chat_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    /// Build memory context for the system prompt from canonical per-persona memory_state.json.
    /// Principles (workspace_dir/AGENTS.md) are loaded separately and injected as the "Principles" section.
    /// Daily logs are intentionally excluded to reduce context pollution.
    pub fn build_memory_context(&self, chat_id: i64, persona_id: i64) -> String {
        let mut context = String::new();

        if let Some(state) = self.read_or_migrate_persona_memory_state(chat_id, persona_id) {
            if let Ok(state_json) = serde_json::to_string_pretty(&state) {
                context.push_str("<memory_this_persona>\n");
                context.push_str(&render_memory_markdown(&state));
                context.push_str("\n</memory_this_persona>\n");
                context.push_str("<memory_state_json>\n");
                context.push_str(&state_json);
                context.push_str("\n</memory_state_json>\n");
            }
        } else if let Some(persona_mem) =
            std::fs::read_to_string(self.persona_memory_path(chat_id, persona_id)).ok()
        {
            if !persona_mem.trim().is_empty() {
                context.push_str("<memory_this_persona>\n");
                context.push_str(&persona_mem);
                context.push_str("\n</memory_this_persona>\n");
            }
        }

        context
    }

    pub fn read_persona_memory_state(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Option<PersonaMemoryState> {
        let path = self.persona_memory_state_path(chat_id, persona_id);
        let content = std::fs::read_to_string(path).ok()?;
        let mut state: PersonaMemoryState = serde_json::from_str(&content).ok()?;
        state.normalize();
        Some(state)
    }

    pub fn validate_memory_state(&self, state: &PersonaMemoryState) -> Result<(), String> {
        if state.meta.version == 0 {
            return Err("meta.version must be >= 1".to_string());
        }
        if state.tier3.recent_focus.len() > 15 {
            return Err("tier3.recent_focus must not exceed 15 entries".to_string());
        }
        for entry in &state.workflow_memory.intents {
            if entry.intent_signature.trim().is_empty() {
                return Err("workflow_memory.intents intent_signature cannot be empty".to_string());
            }
            if !(0.0..=1.0).contains(&entry.confidence) {
                return Err(format!(
                    "workflow_memory confidence out of range for intent '{}'",
                    entry.intent_signature
                ));
            }
        }
        Ok(())
    }

    pub fn write_persona_memory_state(
        &self,
        chat_id: i64,
        persona_id: i64,
        mut state: PersonaMemoryState,
    ) -> std::io::Result<()> {
        state.normalize();
        if let Err(err) = self.validate_memory_state(&state) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, err));
        }
        state.meta.revision = state.meta.revision.saturating_add(1);
        state.meta.updated_at = Utc::now().to_rfc3339();

        let path = self.persona_memory_state_path(chat_id, persona_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_extension("json.tmp");
        let backup_path = path.with_extension("json.bak");
        let bytes = serde_json::to_vec_pretty(&state).map_err(std::io::Error::other)?;
        std::fs::write(&tmp_path, bytes)?;
        if path.exists() {
            let _ = std::fs::copy(&path, &backup_path);
        }
        std::fs::rename(&tmp_path, &path)?;
        let _ = self.write_origin_snapshot(chat_id, persona_id, &state);
        Ok(())
    }

    pub fn append_persona_memory_event(
        &self,
        chat_id: i64,
        persona_id: i64,
        event_type: &str,
        actor: &str,
        payload: serde_json::Value,
    ) -> std::io::Result<()> {
        let path = self.persona_memory_events_path(chat_id, persona_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let event = MemoryEvent {
            ts: Utc::now().to_rfc3339(),
            event_type: event_type.to_string(),
            actor: actor.to_string(),
            chat_id,
            persona_id,
            payload,
        };
        let line = serde_json::to_string(&event).map_err(std::io::Error::other)?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }

    pub fn read_or_migrate_persona_memory_state(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Option<PersonaMemoryState> {
        if let Some(state) = self.read_persona_memory_state(chat_id, persona_id) {
            return Some(state);
        }
        let legacy_path = self.persona_memory_path(chat_id, persona_id);
        let legacy = std::fs::read_to_string(&legacy_path).ok()?;
        let mut state = legacy_markdown_to_state(&legacy);
        state.normalize();
        let _ = self.write_persona_memory_state(chat_id, persona_id, state.clone());
        let _ = self.append_persona_memory_event(
            chat_id,
            persona_id,
            "memory_migrated_from_markdown",
            "migration",
            json!({
                "legacy_path": legacy_path.to_string_lossy().to_string(),
                "schema_version": MEMORY_SCHEMA_VERSION
            }),
        );
        Some(state)
    }

    pub fn ensure_persona_memory_state_exists(
        &self,
        chat_id: i64,
        persona_id: i64,
        display_name: Option<&str>,
    ) -> std::io::Result<()> {
        if self.persona_memory_state_path(chat_id, persona_id).exists() {
            return Ok(());
        }
        let mut state = PersonaMemoryState::default();
        state.meta.version = MEMORY_SCHEMA_VERSION;
        state.meta.updated_at = Utc::now().to_rfc3339();
        if let Some(name) = display_name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                state.identity.display_name = trimmed.to_string();
            }
        }
        self.write_persona_memory_state(chat_id, persona_id, state.clone())?;
        self.append_persona_memory_event(
            chat_id,
            persona_id,
            "memory_state_initialized",
            "system",
            json!({ "schema_version": MEMORY_SCHEMA_VERSION }),
        )?;
        Ok(())
    }

    fn write_origin_snapshot(
        &self,
        chat_id: i64,
        persona_id: i64,
        state: &PersonaMemoryState,
    ) -> std::io::Result<()> {
        let snapshot_path = self
            .working_dir
            .join("shared")
            .join("ORIGIN")
            .join("MemorySnapshots")
            .join(format!("chat-{}-persona-{}.md", chat_id, persona_id));
        if let Some(parent) = snapshot_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut content = String::new();
        content.push_str(&format!(
            "# Memory Snapshot chat={} persona={}\n\n",
            chat_id, persona_id
        ));
        content.push_str(&format!(
            "Updated: {}\nSchema: {}\nRevision: {}\n\n",
            state.meta.updated_at, state.meta.version, state.meta.revision
        ));
        content.push_str(&render_memory_markdown(state));
        std::fs::write(snapshot_path, content)
    }

    #[allow(dead_code)]
    pub fn groups_dir(&self) -> &Path {
        &self.data_dir
    }
}

fn dedupe_trimmed_lines(lines: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn deterministic_key_from_text(input: &str) -> String {
    let key: String = input
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let key = key
        .split('-')
        .filter(|s| !s.trim().is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    if key.is_empty() {
        "item".to_string()
    } else {
        key
    }
}

fn extract_tier_sections(full: &str) -> [String; 3] {
    const TIER_HEADERS: [&str; 3] = [
        "## Tier 1 — Long term",
        "## Tier 2 — Mid term",
        "## Tier 3 — Short term",
    ];
    let mut sections = [String::new(), String::new(), String::new()];
    let mut current_tier: Option<usize> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    let mut flush_current = |tier_idx: usize, lines: &mut Vec<&str>| {
        let block = lines.join("\n").trim().to_string();
        lines.clear();
        if block.is_empty() {
            return;
        }
        if sections[tier_idx].is_empty() {
            sections[tier_idx] = block;
        } else {
            sections[tier_idx].push_str("\n\n");
            sections[tier_idx].push_str(&block);
        }
    };

    for line in full.lines() {
        if line.starts_with("## ") {
            if let Some(prev_idx) = current_tier {
                flush_current(prev_idx, &mut current_lines);
            }
            current_tier = TIER_HEADERS.iter().position(|h| line.trim() == *h);
            continue;
        }
        if current_tier.is_some() {
            current_lines.push(line);
        }
    }
    if let Some(prev_idx) = current_tier {
        flush_current(prev_idx, &mut current_lines);
    }
    sections
}

fn legacy_markdown_to_state(markdown: &str) -> PersonaMemoryState {
    let tiers = extract_tier_sections(markdown);
    let tier1_lines: Vec<String> = tiers[0]
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let tier2_projects: Vec<ActiveProjectMemory> = tiers[1]
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|summary| ActiveProjectMemory {
            id: deterministic_key_from_text(summary),
            status: "active".to_string(),
            summary: summary.to_string(),
            updated_at: Utc::now().to_rfc3339(),
        })
        .collect();
    let tier3_lines: Vec<String> = tiers[2]
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let mut state = PersonaMemoryState {
        meta: MemoryStateMeta {
            version: MEMORY_SCHEMA_VERSION,
            revision: 0,
            updated_at: Utc::now().to_rfc3339(),
        },
        identity: IdentityMemory::default(),
        tier1: Tier1Memory {
            stable_facts: tier1_lines,
            workflow_principles: Vec::new(),
        },
        tier2: Tier2Memory {
            active_projects: tier2_projects,
        },
        tier3: Tier3Memory {
            recent_focus: tier3_lines,
        },
        workflow_memory: WorkflowMemory::default(),
        links: MemoryLinks::default(),
    };
    state.normalize();
    state
}

pub fn render_memory_markdown(state: &PersonaMemoryState) -> String {
    let mut out = String::new();
    out.push_str("# Memory\n\n");

    out.push_str("## Tier 1 — Long term\n\n");
    if !state.identity.display_name.trim().is_empty() {
        out.push_str(&format!(
            "- Identity|display_name={}\n",
            state.identity.display_name.trim()
        ));
    }
    if !state.identity.self_model.trim().is_empty() {
        out.push_str(&format!(
            "- Identity|self_model={}\n",
            state.identity.self_model.trim()
        ));
    }
    if !state.identity.voice_style.trim().is_empty() {
        out.push_str(&format!(
            "- Identity|voice_style={}\n",
            state.identity.voice_style.trim()
        ));
    }
    for item in &state.identity.non_negotiables {
        out.push_str(&format!("- IdentityConstraint|{}\n", item));
    }
    for item in &state.tier1.stable_facts {
        out.push_str(&format!("{item}\n"));
    }
    for item in &state.tier1.workflow_principles {
        out.push_str(&format!("- WorkflowPrinciple|{}\n", item));
    }
    out.push('\n');

    out.push_str("## Tier 2 — Mid term\n\n");
    for project in &state.tier2.active_projects {
        out.push_str(&format!(
            "- ProjectState|id={}|status={}|updated={}|summary={}\n",
            project.id, project.status, project.updated_at, project.summary
        ));
    }
    out.push('\n');

    out.push_str("## Tier 3 — Short term\n\n");
    for line in &state.tier3.recent_focus {
        out.push_str(&format!("{line}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn test_memory_manager() -> (MemoryManager, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "finally_a_value_bot_mem_test_{}",
            uuid::Uuid::new_v4()
        ));
        let dir_str = dir.to_str().unwrap();
        let mm = MemoryManager::new(dir_str, dir_str);
        (mm, dir)
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_global_memory_path() {
        let (mm, dir) = test_memory_manager();
        let path = mm.global_memory_path();
        assert!(
            path.ends_with(Path::new("shared").join("AGENTS.md")),
            "path = {}",
            path.display()
        );
        cleanup(&dir);
    }

    #[test]
    fn test_chat_memory_path() {
        let (mm, dir) = test_memory_manager();
        let path = mm.chat_memory_path(12345);
        assert!(path.ends_with(Path::new("groups").join("12345").join("AGENTS.md")));
        cleanup(&dir);
    }

    #[test]
    fn test_persona_memory_path() {
        let (mm, dir) = test_memory_manager();
        let path = mm.persona_memory_path(997894126, 1);
        assert!(path.ends_with(Path::new("997894126").join("1").join("MEMORY.md")));
        cleanup(&dir);
    }

    #[test]
    fn test_groups_root_memory_path_display() {
        let (mm, dir) = test_memory_manager();
        let s = mm.groups_root_memory_path_display();
        assert!(s.contains("AGENTS.md"));
        cleanup(&dir);
    }

    #[test]
    fn test_groups_root_memory_path() {
        let (mm, dir) = test_memory_manager();
        let path = mm.groups_root_memory_path();
        assert!(path.ends_with("AGENTS.md"));
        cleanup(&dir);
    }

    #[test]
    fn test_read_nonexistent_memory() {
        let (mm, dir) = test_memory_manager();
        assert!(mm.read_global_memory().is_none());
        assert!(mm.read_chat_memory(100).is_none());
        assert!(mm.read_persona_memory(100, 1).is_none());
        cleanup(&dir);
    }

    #[test]
    fn test_read_persona_memory_migrates_legacy_markdown() {
        let (mm, dir) = test_memory_manager();
        let legacy_path = mm.persona_memory_path(42, 2);
        if let Some(p) = legacy_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        std::fs::write(
            &legacy_path,
            "# Memory\n\n## Tier 1 — Long term\n\n- old fact\n\n## Tier 2 — Mid term\n\n- project alpha\n\n## Tier 3 — Short term\n\n- recent focus\n",
        )
        .unwrap();

        let content = mm.read_persona_memory(42, 2).unwrap();
        assert!(content.contains("old fact"));
        assert!(content.contains("project alpha"));
        assert!(content.contains("recent focus"));

        let state_path = mm.persona_memory_state_path(42, 2);
        assert!(state_path.exists());
        cleanup(&dir);
    }

    #[test]
    fn test_write_and_read_global_memory() {
        let (mm, dir) = test_memory_manager();
        mm.write_global_memory("global notes").unwrap();
        let content = mm.read_global_memory().unwrap();
        assert_eq!(content, "global notes");
        cleanup(&dir);
    }

    #[test]
    fn test_write_and_read_chat_memory() {
        let (mm, dir) = test_memory_manager();
        mm.write_chat_memory(42, "chat 42 notes").unwrap();
        let content = mm.read_chat_memory(42).unwrap();
        assert_eq!(content, "chat 42 notes");

        // Different chat should be empty
        assert!(mm.read_chat_memory(99).is_none());
        cleanup(&dir);
    }

    #[test]
    fn test_build_memory_context_empty() {
        let (mm, dir) = test_memory_manager();
        let ctx = mm.build_memory_context(100, 1);
        assert!(ctx.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_build_memory_context_with_global_only() {
        let (mm, dir) = test_memory_manager();
        mm.write_global_memory("I am global memory").unwrap();
        let ctx = mm.build_memory_context(100, 1);
        // Global AGENTS.md is not part of memory_context; it is loaded separately as principles
        assert!(ctx.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_build_memory_context_persona_and_daily_only() {
        let (mm, dir) = test_memory_manager();
        mm.write_global_memory("global stuff").unwrap();
        let mut state = PersonaMemoryState::default();
        state.tier1.stable_facts = vec!["persona memory".to_string()];
        mm.write_persona_memory_state(100, 1, state).unwrap();
        let ctx = mm.build_memory_context(100, 1);
        assert!(ctx.contains("<memory_this_persona>"));
        assert!(ctx.contains("persona memory"));
        assert!(ctx.contains("<memory_state_json>"));
        assert!(!ctx.contains("global stuff"));
        cleanup(&dir);
    }

    #[test]
    fn test_build_memory_context_ignores_whitespace_only_persona() {
        let (mm, dir) = test_memory_manager();
        let mut state = PersonaMemoryState::default();
        state.tier1.stable_facts = vec![];
        state.tier2.active_projects = vec![];
        state.tier3.recent_focus = vec![];
        mm.write_persona_memory_state(100, 1, state).unwrap();
        let ctx = mm.build_memory_context(100, 1);
        assert!(ctx.contains("<memory_this_persona>"));
        cleanup(&dir);
    }

    #[test]
    fn test_groups_dir() {
        let (mm, dir) = test_memory_manager();
        assert!(mm.groups_dir().ends_with("groups"));
        cleanup(&dir);
    }

    #[test]
    fn test_daily_log_append_and_read() {
        let (mm, dir) = test_memory_manager();
        mm.append_daily_log(100, 1, "2025-01-15", "Note from day one.\n")
            .unwrap();
        mm.append_daily_log(100, 1, "2025-01-15", "Second line.")
            .unwrap();
        let content = mm.read_daily_log(100, 1, "2025-01-15").unwrap();
        assert!(content.contains("Note from day one."));
        assert!(content.contains("Second line."));
        assert!(mm.read_daily_log(100, 1, "2025-01-14").is_none());
        cleanup(&dir);
    }

    #[test]
    fn test_write_and_read_persona_memory_state() {
        let (mm, dir) = test_memory_manager();
        let mut state = PersonaMemoryState::default();
        state.identity.display_name = "Assistant".into();
        state.tier3.recent_focus = vec!["- one".into(), "- one".into(), "- two".into()];
        mm.write_persona_memory_state(10, 3, state).unwrap();
        let read_back = mm.read_persona_memory_state(10, 3).unwrap();
        assert_eq!(read_back.identity.display_name, "Assistant");
        assert_eq!(read_back.tier3.recent_focus.len(), 2);
        cleanup(&dir);
    }

    #[test]
    fn test_append_persona_memory_event() {
        let (mm, dir) = test_memory_manager();
        mm.append_persona_memory_event(
            10,
            1,
            "manual_edit",
            "user",
            json!({"field":"tier1.stable_facts"}),
        )
        .unwrap();
        let events_path = mm.persona_memory_events_path(10, 1);
        let content = std::fs::read_to_string(events_path).unwrap();
        assert!(content.contains("\"event_type\":\"manual_edit\""));
        cleanup(&dir);
    }

    #[test]
    fn test_validate_memory_state_confidence_range() {
        let (mm, dir) = test_memory_manager();
        let mut state = PersonaMemoryState::default();
        state.workflow_memory.intents.push(WorkflowMemoryEntry {
            intent_signature: "test".into(),
            confidence: 2.0,
            ..WorkflowMemoryEntry::default()
        });
        let err = mm.validate_memory_state(&state).unwrap_err();
        assert!(err.contains("confidence"));
        cleanup(&dir);
    }
}
