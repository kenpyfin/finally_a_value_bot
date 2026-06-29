//! Multi-model routing: phase-based state machine with single local executor tier.
//!
//! Architecture: Strategy (cloud) plans → Local executes → Strategy synthesizes.
//! Configuration is stored in `app_settings` and hot-reloaded from the Web UI.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config::Config;
use crate::db::Database;
use crate::error::FinallyAValueBotError;
use crate::llm::{create_openai_compatible_provider, LlmProvider};

// DB keys
pub const APP_SETTING_MULTIMODEL_ENABLED: &str = "MULTIMODEL_ENABLED";
pub const APP_SETTING_LOCAL_BASE_URL: &str = "MULTIMODEL_LOCAL_BASE_URL";
pub const APP_SETTING_LOCAL_MODEL: &str = "MULTIMODEL_LOCAL_MODEL";
pub const APP_SETTING_LOCAL_TOOLS_OK: &str = "MULTIMODEL_LOCAL_TOOLS_OK";
// Legacy keys (read for migration)
pub const APP_SETTING_TIER1_BASE_URL: &str = "MULTIMODEL_TIER1_BASE_URL";
pub const APP_SETTING_TIER1_MODEL: &str = "MULTIMODEL_TIER1_MODEL";
pub const APP_SETTING_TIER2_BASE_URL: &str = "MULTIMODEL_TIER2_BASE_URL";
pub const APP_SETTING_TIER2_MODEL: &str = "MULTIMODEL_TIER2_MODEL";
pub const APP_SETTING_TIER1_TOOLS_OK: &str = "MULTIMODEL_TIER1_TOOLS_OK";
pub const APP_SETTING_TIER2_TOOLS_OK: &str = "MULTIMODEL_TIER2_TOOLS_OK";

pub const DEFAULT_TIER1_BASE_URL: &str = "http://127.0.0.1:8080/v1";
pub const DEFAULT_TIER2_BASE_URL: &str = "http://127.0.0.1:8081/v1";
pub const DEFAULT_TIER1_MODEL: &str = "qwen2.5-coder-14b-instruct";
pub const DEFAULT_TIER2_MODEL: &str = "mistral-nemo-12b-instruct";

// ---------------------------------------------------------------------------
// Model Tier
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Local,
    Strategy,
    Technical,
    Knowledge,
}

impl ModelTier {
    pub fn is_local(self) -> bool {
        matches!(self, Self::Local | Self::Technical | Self::Knowledge)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Local | Self::Technical | Self::Knowledge => "local",
            Self::Strategy => "strategy",
        }
    }
}

// ---------------------------------------------------------------------------
// Agent Phase State Machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPhase {
    Plan,
    Execute,
    Synthesize,
}

impl AgentPhase {
    pub fn model_tier(self) -> ModelTier {
        match self {
            Self::Plan | Self::Synthesize => ModelTier::Strategy,
            Self::Execute => ModelTier::Local,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::Synthesize => "synthesize",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PhaseTransition {
    MutationToolCalled,
    NaturalCompletion,
    IterationCapReached,
    LocalEscalation,
}

impl PhaseTransition {
    pub fn label(self) -> &'static str {
        match self {
            Self::MutationToolCalled => "mutation_tool_called",
            Self::NaturalCompletion => "natural_completion",
            Self::IterationCapReached => "iteration_cap_reached",
            Self::LocalEscalation => "local_escalation",
        }
    }
}

pub fn advance_phase(
    cfg: &MultimodelConfig,
    current: AgentPhase,
    iteration: usize,
    max_iterations: usize,
    is_conversational: bool,
    executed_tools: &[String],
    stop_reason: &str,
    assistant_text: &str,
    local_error_streak: u32,
) -> (AgentPhase, Option<PhaseTransition>) {
    if is_conversational || !cfg.ready_for_routing() {
        return (current, None);
    }
    match current {
        AgentPhase::Plan => {
            if executed_tools.iter().any(|t| is_mutation_tool(t)) {
                return (
                    AgentPhase::Execute,
                    Some(PhaseTransition::MutationToolCalled),
                );
            }
            (AgentPhase::Plan, None)
        }
        AgentPhase::Execute => {
            if local_error_streak >= 2 {
                return (
                    AgentPhase::Synthesize,
                    Some(PhaseTransition::LocalEscalation),
                );
            }
            if assistant_text.contains("[ESCALATE]") {
                return (
                    AgentPhase::Synthesize,
                    Some(PhaseTransition::LocalEscalation),
                );
            }
            if iteration + 1 >= max_iterations {
                return (
                    AgentPhase::Synthesize,
                    Some(PhaseTransition::IterationCapReached),
                );
            }
            if stop_reason == "end_turn" && executed_tools.is_empty() {
                return (
                    AgentPhase::Synthesize,
                    Some(PhaseTransition::NaturalCompletion),
                );
            }
            (AgentPhase::Execute, None)
        }
        AgentPhase::Synthesize => (AgentPhase::Synthesize, None),
    }
}

pub fn is_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "bash"
            | "write_file"
            | "edit_file"
            | "apply_search_replace"
            | "symbol_edit"
            | "spawn_background_command"
            | "schedule_task"
            | "write_tiered_memory"
            | "write_memory_state"
            | "patch_memory_state"
            | "update_bulletin_focus"
            | "write_memory"
            | "send_message"
            | "vault_add"
            | "register_tracked_job"
            | "run_skill_script"
    )
}

// ---------------------------------------------------------------------------
// Tier Endpoint Snapshot (observability)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierEndpointSnapshot {
    pub tier: ModelTier,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
}

impl TierEndpointSnapshot {
    pub fn format_tier_line(&self) -> String {
        format!(
            "Model tier: {} | provider: {} | model: {} | endpoint: {}",
            self.tier.label(),
            self.provider,
            self.model,
            self.endpoint
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "tier": self.tier.label(),
            "provider": self.provider,
            "model": self.model,
            "endpoint": self.endpoint,
        })
    }
}

// ---------------------------------------------------------------------------
// Run Summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MultimodelRunSummary {
    pub enabled: bool,
    pub strategy_provider: String,
    pub strategy_model: String,
    pub strategy_endpoint: String,
    pub local_model: String,
    pub local_endpoint: String,
    // Legacy compat
    pub tier1_model: String,
    pub tier1_endpoint: String,
    pub tier2_model: String,
    pub tier2_endpoint: String,
}

impl MultimodelRunSummary {
    pub fn format_markdown_block(&self) -> String {
        if self.enabled {
            format!(
                "Multi-model: enabled\n\
                 - Local: {} @ {}\n\
                 - Strategy: {} / {} @ {}\n",
                self.local_model,
                self.local_endpoint,
                self.strategy_provider,
                self.strategy_model,
                self.strategy_endpoint,
            )
        } else {
            format!(
                "Multi-model: disabled (strategy only: {} / {} @ {})\n",
                self.strategy_provider, self.strategy_model, self.strategy_endpoint,
            )
        }
    }

    pub fn routing_v1_json(&self, iter0: &TierEndpointSnapshot) -> serde_json::Value {
        serde_json::json!({
            "multimodel_enabled": self.enabled,
            "iter0": iter0.to_json(),
            "tiers": {
                "local": {
                    "tier": "local",
                    "provider": "llama",
                    "model": self.local_model,
                    "endpoint": self.local_endpoint,
                },
                "tier1": {
                    "tier": "local",
                    "provider": "llama",
                    "model": self.tier1_model,
                    "endpoint": self.tier1_endpoint,
                },
                "tier2": {
                    "tier": "local",
                    "provider": "llama",
                    "model": self.tier2_model,
                    "endpoint": self.tier2_endpoint,
                },
                "strategy": {
                    "tier": "strategy",
                    "provider": self.strategy_provider,
                    "model": self.strategy_model,
                    "endpoint": self.strategy_endpoint,
                },
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MultimodelConfig {
    pub enabled: bool,
    // Unified local tier
    pub local_base_url: String,
    pub local_model: String,
    pub local_tools_ok: bool,
    // Legacy (kept for migration/backward compat)
    pub tier1_base_url: String,
    pub tier1_model: String,
    pub tier1_tools_ok: bool,
    pub tier2_base_url: String,
    pub tier2_model: String,
    pub tier2_tools_ok: bool,
}

impl MultimodelConfig {
    pub fn local_configured(&self) -> bool {
        !self.local_base_url.trim().is_empty() && !self.local_model.trim().is_empty()
    }

    pub fn local_routable(&self) -> bool {
        self.local_configured() && self.local_tools_ok
    }

    pub fn ready_for_routing(&self) -> bool {
        self.enabled && self.local_routable()
    }

    // Legacy compat helpers
    pub fn tier1_configured(&self) -> bool {
        !self.tier1_base_url.trim().is_empty() && !self.tier1_model.trim().is_empty()
    }

    pub fn tier2_configured(&self) -> bool {
        !self.tier2_base_url.trim().is_empty() && !self.tier2_model.trim().is_empty()
    }

    pub fn tier1_routable(&self) -> bool {
        self.tier1_configured() && self.tier1_tools_ok
    }

    pub fn tier2_routable(&self) -> bool {
        self.tier2_configured() && self.tier2_tools_ok
    }

    pub fn normalize(mut self) -> Self {
        self.local_base_url = normalize_base_url(&self.local_base_url, "");
        self.local_model = self.local_model.trim().to_string();
        self.tier1_base_url = normalize_base_url(&self.tier1_base_url, "");
        self.tier2_base_url = normalize_base_url(&self.tier2_base_url, "");
        self.tier1_model = self.tier1_model.trim().to_string();
        self.tier2_model = self.tier2_model.trim().to_string();
        self
    }
}

/// Local OpenAI-compat endpoint for sidecar evaluators (PTE/PDQE). Does not require `tools_ok`.
pub fn resolve_local_evaluator_endpoint(mm: &MultimodelConfig) -> Option<(String, String)> {
    if mm.local_configured() {
        Some((mm.local_base_url.clone(), mm.local_model.clone()))
    } else if mm.tier1_configured() {
        Some((mm.tier1_base_url.clone(), mm.tier1_model.clone()))
    } else if mm.tier2_configured() {
        Some((mm.tier2_base_url.clone(), mm.tier2_model.clone()))
    } else {
        None
    }
}

pub fn normalize_base_url_for_provider(raw: &str, fallback: &str) -> String {
    normalize_base_url(raw, fallback)
}

fn normalize_base_url(raw: &str, fallback: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return fallback.to_string();
    }
    let base = t.trim_end_matches('/');
    if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    }
}

// ---------------------------------------------------------------------------
// DB persistence
// ---------------------------------------------------------------------------

fn read_setting(settings: &[(String, String)], key: &str) -> Option<String> {
    settings
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub fn load_from_db(db: &Database) -> Result<MultimodelConfig, FinallyAValueBotError> {
    let rows: Vec<(String, String)> = db
        .list_app_settings()?
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect();
    let mut cfg = MultimodelConfig::default();

    if let Some(v) = read_setting(&rows, APP_SETTING_MULTIMODEL_ENABLED) {
        cfg.enabled = parse_bool(&v);
    }
    // New unified local fields
    if let Some(v) = read_setting(&rows, APP_SETTING_LOCAL_BASE_URL) {
        cfg.local_base_url = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_LOCAL_MODEL) {
        cfg.local_model = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_LOCAL_TOOLS_OK) {
        cfg.local_tools_ok = parse_bool(&v);
    }
    // Legacy fields
    if let Some(v) = read_setting(&rows, APP_SETTING_TIER1_BASE_URL) {
        cfg.tier1_base_url = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_TIER1_MODEL) {
        cfg.tier1_model = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_TIER2_BASE_URL) {
        cfg.tier2_base_url = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_TIER2_MODEL) {
        cfg.tier2_model = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_TIER1_TOOLS_OK) {
        cfg.tier1_tools_ok = parse_bool(&v);
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_TIER2_TOOLS_OK) {
        cfg.tier2_tools_ok = parse_bool(&v);
    }

    // Auto-migration: promote tier1 config to unified local if local is empty
    if cfg.local_base_url.is_empty() && !cfg.tier1_base_url.is_empty() {
        cfg.local_base_url = cfg.tier1_base_url.clone();
        cfg.local_model = cfg.tier1_model.clone();
        cfg.local_tools_ok = cfg.tier1_tools_ok;
    }

    Ok(cfg.normalize())
}

pub fn persist_to_db(db: &Database, cfg: &MultimodelConfig) -> Result<(), FinallyAValueBotError> {
    let cfg = cfg.clone().normalize();
    db.set_app_setting(
        APP_SETTING_MULTIMODEL_ENABLED,
        if cfg.enabled { "true" } else { "false" },
    )?;
    // New unified local fields
    db.set_app_setting(APP_SETTING_LOCAL_BASE_URL, &cfg.local_base_url)?;
    db.set_app_setting(APP_SETTING_LOCAL_MODEL, &cfg.local_model)?;
    db.set_app_setting(
        APP_SETTING_LOCAL_TOOLS_OK,
        if cfg.local_tools_ok { "true" } else { "false" },
    )?;
    // Legacy fields (kept in sync)
    db.set_app_setting(APP_SETTING_TIER1_BASE_URL, &cfg.tier1_base_url)?;
    db.set_app_setting(APP_SETTING_TIER1_MODEL, &cfg.tier1_model)?;
    db.set_app_setting(APP_SETTING_TIER2_BASE_URL, &cfg.tier2_base_url)?;
    db.set_app_setting(APP_SETTING_TIER2_MODEL, &cfg.tier2_model)?;
    db.set_app_setting(
        APP_SETTING_TIER1_TOOLS_OK,
        if cfg.tier1_tools_ok { "true" } else { "false" },
    )?;
    db.set_app_setting(
        APP_SETTING_TIER2_TOOLS_OK,
        if cfg.tier2_tools_ok { "true" } else { "false" },
    )?;
    Ok(())
}

pub fn persist_tier_tools_ok(
    db: &Database,
    tier: ModelTier,
    ok: bool,
) -> Result<(), FinallyAValueBotError> {
    let key = match tier {
        ModelTier::Local => APP_SETTING_LOCAL_TOOLS_OK,
        ModelTier::Technical => APP_SETTING_TIER1_TOOLS_OK,
        ModelTier::Knowledge => APP_SETTING_TIER2_TOOLS_OK,
        ModelTier::Strategy => return Ok(()),
    };
    db.set_app_setting(key, if ok { "true" } else { "false" })
}

// ---------------------------------------------------------------------------
// tool_choice
// ---------------------------------------------------------------------------

pub fn tool_choice_for_tier(tier: ModelTier, has_tools: bool) -> Option<String> {
    if !has_tools {
        return None;
    }
    match tier {
        ModelTier::Local | ModelTier::Technical | ModelTier::Knowledge => Some("auto".to_string()),
        ModelTier::Strategy => None,
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

pub struct MultimodelRuntime {
    pub config: MultimodelConfig,
    local: Arc<dyn LlmProvider>,
}

impl MultimodelRuntime {
    pub fn new(base_config: &Config, config: MultimodelConfig) -> Self {
        let config = config.normalize();
        let local = Arc::from(create_openai_compatible_provider(
            base_config,
            &config.local_base_url,
            &config.local_model,
        ));
        Self { config, local }
    }

    pub fn provider_for_tier(
        &self,
        tier: ModelTier,
        strategy: &Arc<dyn LlmProvider>,
    ) -> Arc<dyn LlmProvider> {
        if tier.is_local() {
            self.local.clone()
        } else {
            strategy.clone()
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy routing (kept for backward compat during transition)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct RouteContext<'a> {
    pub iteration: usize,
    pub is_conversational: bool,
    pub last_iteration_tools: &'a [String],
    pub max_iterations: usize,
    pub force_strategy: bool,
    pub local_tier_error_streak: u32,
}

pub fn resolve_route(cfg: &MultimodelConfig, ctx: &RouteContext<'_>) -> ModelTier {
    if !cfg.ready_for_routing() || ctx.force_strategy {
        return ModelTier::Strategy;
    }
    if ctx.is_conversational || ctx.iteration == 0 {
        return ModelTier::Strategy;
    }
    if ctx.iteration + 1 >= ctx.max_iterations {
        return ModelTier::Strategy;
    }
    if ctx.local_tier_error_streak > 0 {
        return ModelTier::Strategy;
    }
    if ctx.last_iteration_tools.is_empty() {
        return ModelTier::Strategy;
    }
    ModelTier::Local
}

pub fn is_technical_tool(name: &str) -> bool {
    matches!(
        name,
        "bash"
            | "read_file"
            | "write_file"
            | "edit_file"
            | "apply_search_replace"
            | "symbol_edit"
            | "grep"
            | "glob"
            | "read_repo_map"
            | "run_skill_script"
            | "spawn_background_command"
            | "cursor_agent"
    )
}

pub fn is_knowledge_tool(name: &str) -> bool {
    matches!(
        name,
        "search_vault"
            | "search_history"
            | "read_tiered_memory"
            | "read_memory"
            | "web_search"
            | "web_fetch"
            | "web_html"
            | "export_chat"
            | "agent_history"
    )
}

// ---------------------------------------------------------------------------
// Web API request types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MultimodelPatchRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub local_base_url: Option<String>,
    #[serde(default)]
    pub local_model: Option<String>,
    // Legacy fields (accepted for backward compat)
    #[serde(default)]
    pub tier1_base_url: Option<String>,
    #[serde(default)]
    pub tier1_model: Option<String>,
    #[serde(default)]
    pub tier2_base_url: Option<String>,
    #[serde(default)]
    pub tier2_model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultimodelTestTier {
    Local,
    Technical,
    Knowledge,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> MultimodelConfig {
        MultimodelConfig {
            enabled: true,
            local_base_url: DEFAULT_TIER1_BASE_URL.into(),
            local_model: DEFAULT_TIER1_MODEL.into(),
            local_tools_ok: true,
            tier1_base_url: DEFAULT_TIER1_BASE_URL.into(),
            tier1_model: DEFAULT_TIER1_MODEL.into(),
            tier1_tools_ok: true,
            tier2_base_url: DEFAULT_TIER2_BASE_URL.into(),
            tier2_model: DEFAULT_TIER2_MODEL.into(),
            tier2_tools_ok: true,
        }
    }

    #[test]
    fn resolve_route_first_iteration_is_strategy() {
        let cfg = test_cfg();
        let ctx = RouteContext {
            iteration: 0,
            is_conversational: false,
            last_iteration_tools: &[],
            max_iterations: 10,
            force_strategy: false,
            local_tier_error_streak: 0,
        };
        assert_eq!(resolve_route(&cfg, &ctx), ModelTier::Strategy);
    }

    #[test]
    fn resolve_route_tools_chain_goes_local() {
        let cfg = test_cfg();
        let tools = vec!["bash".into(), "read_file".into()];
        let ctx = RouteContext {
            iteration: 2,
            is_conversational: false,
            last_iteration_tools: &tools,
            max_iterations: 10,
            force_strategy: false,
            local_tier_error_streak: 0,
        };
        assert_eq!(resolve_route(&cfg, &ctx), ModelTier::Local);
    }

    #[test]
    fn resolve_route_local_error_streak_stays_strategy() {
        let cfg = test_cfg();
        let tools = vec!["read_file".into()];
        let ctx = RouteContext {
            iteration: 2,
            is_conversational: false,
            last_iteration_tools: &tools,
            max_iterations: 10,
            force_strategy: false,
            local_tier_error_streak: 1,
        };
        assert_eq!(resolve_route(&cfg, &ctx), ModelTier::Strategy);
    }

    #[test]
    fn resolve_route_stays_strategy_when_tools_not_verified() {
        let cfg = MultimodelConfig {
            enabled: true,
            local_base_url: DEFAULT_TIER1_BASE_URL.into(),
            local_model: DEFAULT_TIER1_MODEL.into(),
            local_tools_ok: false,
            ..Default::default()
        };
        let tools = vec!["bash".into()];
        let ctx = RouteContext {
            iteration: 2,
            is_conversational: false,
            last_iteration_tools: &tools,
            max_iterations: 10,
            force_strategy: false,
            local_tier_error_streak: 0,
        };
        assert_eq!(resolve_route(&cfg, &ctx), ModelTier::Strategy);
    }

    #[test]
    fn advance_phase_plan_to_execute_on_mutation() {
        let cfg = test_cfg();
        let tools = vec!["edit_file".into()];
        let (phase, transition) = advance_phase(
            &cfg,
            AgentPhase::Plan,
            0,
            10,
            false,
            &tools,
            "tool_use",
            "",
            0,
        );
        assert_eq!(phase, AgentPhase::Execute);
        assert!(matches!(
            transition,
            Some(PhaseTransition::MutationToolCalled)
        ));
    }

    #[test]
    fn advance_phase_execute_natural_completion() {
        let cfg = test_cfg();
        let tools: Vec<String> = vec![];
        let (phase, transition) = advance_phase(
            &cfg,
            AgentPhase::Execute,
            3,
            10,
            false,
            &tools,
            "end_turn",
            "",
            0,
        );
        assert_eq!(phase, AgentPhase::Synthesize);
        assert!(matches!(
            transition,
            Some(PhaseTransition::NaturalCompletion)
        ));
    }

    #[test]
    fn advance_phase_execute_escalation() {
        let cfg = test_cfg();
        let tools = vec!["bash".into()];
        let (phase, transition) = advance_phase(
            &cfg,
            AgentPhase::Execute,
            3,
            10,
            false,
            &tools,
            "end_turn",
            "Cannot proceed [ESCALATE] file not found",
            0,
        );
        assert_eq!(phase, AgentPhase::Synthesize);
        assert!(matches!(transition, Some(PhaseTransition::LocalEscalation)));
    }

    #[test]
    fn advance_phase_execute_error_streak() {
        let cfg = test_cfg();
        let tools = vec!["bash".into()];
        let (phase, _) = advance_phase(
            &cfg,
            AgentPhase::Execute,
            3,
            10,
            false,
            &tools,
            "tool_use",
            "",
            2,
        );
        assert_eq!(phase, AgentPhase::Synthesize);
    }

    #[test]
    fn advance_phase_stays_plan_on_read_only_tools() {
        let cfg = test_cfg();
        let tools = vec!["read_file".into(), "grep".into()];
        let (phase, transition) = advance_phase(
            &cfg,
            AgentPhase::Plan,
            0,
            10,
            false,
            &tools,
            "tool_use",
            "",
            0,
        );
        assert_eq!(phase, AgentPhase::Plan);
        assert!(transition.is_none());
    }

    #[test]
    fn normalize_base_url_appends_v1() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:8080", DEFAULT_TIER1_BASE_URL),
            "http://127.0.0.1:8080/v1"
        );
    }
}
