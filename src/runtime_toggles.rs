//! Hot-reloadable runtime flags persisted in `app_settings` (Web UI).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::db::Database;
use crate::error::FinallyAValueBotError;

pub const APP_SETTING_TOOL_OUTPUT_DEBUG: &str = "TOOL_OUTPUT_DEBUG";
pub const APP_SETTING_POST_TOOL_EVALUATOR_ENABLED: &str = "POST_TOOL_EVALUATOR_ENABLED";
pub const APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED: &str =
    "RESPONSE_QUALITY_EVALUATOR_ENABLED";
pub const APP_SETTING_AGENT_ENGINE: &str = "AGENT_ENGINE";

const AGENT_ENGINE_CLASSIC: u8 = 0;
const AGENT_ENGINE_DETERMINISTIC: u8 = 1;
const AGENT_ENGINE_CURSOR: u8 = 2;
const AGENT_ENGINE_CLASSIC_COST_ROUTING: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEngine {
    /// Single cloud model for the full turn (default).
    Classic,
    /// Classic tool loop with inverse local cost routing when local delegate is verified.
    ClassicCostRouting,
    Deterministic,
    Cursor,
}

impl AgentEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::ClassicCostRouting => "classic_cost_routing",
            Self::Deterministic => "deterministic",
            Self::Cursor => "cursor",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "deterministic" | "pipeline" => Self::Deterministic,
            "cursor" | "cursor_sdk" | "cursor-sdk" => Self::Cursor,
            "classic_cost_routing" | "classic_routed" | "cost_routing" => Self::ClassicCostRouting,
            _ => Self::Classic,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            AGENT_ENGINE_DETERMINISTIC => Self::Deterministic,
            AGENT_ENGINE_CURSOR => Self::Cursor,
            AGENT_ENGINE_CLASSIC_COST_ROUTING => Self::ClassicCostRouting,
            _ => Self::Classic,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Classic => AGENT_ENGINE_CLASSIC,
            Self::Deterministic => AGENT_ENGINE_DETERMINISTIC,
            Self::Cursor => AGENT_ENGINE_CURSOR,
            Self::ClassicCostRouting => AGENT_ENGINE_CLASSIC_COST_ROUTING,
        }
    }

    pub fn is_classic_loop(self) -> bool {
        matches!(self, Self::Classic | Self::ClassicCostRouting)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeToggleInit {
    pub tool_output_debug: bool,
    pub post_tool_evaluator_enabled: bool,
    pub response_quality_evaluator_enabled: bool,
    pub agent_engine: AgentEngine,
}

#[derive(Debug)]
pub struct RuntimeToggles {
    tool_output_debug: AtomicBool,
    post_tool_evaluator_enabled: AtomicBool,
    response_quality_evaluator_enabled: AtomicBool,
    agent_engine: AtomicU8,
}

impl RuntimeToggles {
    pub fn new(tool_output_debug: bool) -> Arc<Self> {
        Self::from_init(RuntimeToggleInit {
            tool_output_debug,
            post_tool_evaluator_enabled: false,
            response_quality_evaluator_enabled: false,
            agent_engine: AgentEngine::Classic,
        })
    }

    pub fn from_init(init: RuntimeToggleInit) -> Arc<Self> {
        Arc::new(Self {
            tool_output_debug: AtomicBool::new(init.tool_output_debug),
            post_tool_evaluator_enabled: AtomicBool::new(init.post_tool_evaluator_enabled),
            response_quality_evaluator_enabled: AtomicBool::new(
                init.response_quality_evaluator_enabled,
            ),
            agent_engine: AtomicU8::new(init.agent_engine.to_u8()),
        })
    }

    pub fn tool_output_debug(&self) -> bool {
        self.tool_output_debug.load(Ordering::Relaxed)
    }

    pub fn post_tool_evaluator_enabled(&self) -> bool {
        self.post_tool_evaluator_enabled.load(Ordering::Relaxed)
    }

    pub fn response_quality_evaluator_enabled(&self) -> bool {
        self.response_quality_evaluator_enabled
            .load(Ordering::Relaxed)
    }

    pub fn agent_engine(&self) -> AgentEngine {
        AgentEngine::from_u8(self.agent_engine.load(Ordering::Relaxed))
    }

    pub fn set_tool_output_debug(&self, enabled: bool) {
        self.tool_output_debug.store(enabled, Ordering::Relaxed);
    }

    pub fn set_post_tool_evaluator_enabled(&self, enabled: bool) {
        self.post_tool_evaluator_enabled
            .store(enabled, Ordering::Relaxed);
    }

    pub fn set_response_quality_evaluator_enabled(&self, enabled: bool) {
        self.response_quality_evaluator_enabled
            .store(enabled, Ordering::Relaxed);
    }

    pub fn set_agent_engine(&self, engine: AgentEngine) {
        self.agent_engine.store(engine.to_u8(), Ordering::Relaxed);
    }

    pub fn merge_from_app_settings(
        env_defaults: RuntimeToggleInit,
        db: &Database,
    ) -> Result<RuntimeToggleInit, FinallyAValueBotError> {
        Ok(RuntimeToggleInit {
            tool_output_debug: merge_bool_from_app_settings(
                db,
                APP_SETTING_TOOL_OUTPUT_DEBUG,
                env_defaults.tool_output_debug,
            )?,
            post_tool_evaluator_enabled: merge_bool_from_app_settings(
                db,
                APP_SETTING_POST_TOOL_EVALUATOR_ENABLED,
                env_defaults.post_tool_evaluator_enabled,
            )?,
            response_quality_evaluator_enabled: merge_bool_from_app_settings(
                db,
                APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED,
                env_defaults.response_quality_evaluator_enabled,
            )?,
            agent_engine: merge_agent_engine_from_app_settings(db, env_defaults.agent_engine)?,
        })
    }

    pub fn bool_from_app_settings(db: &Database, key: &str) -> Result<bool, FinallyAValueBotError> {
        let settings = db.list_app_settings()?;
        Ok(settings
            .iter()
            .any(|s| s.key.eq_ignore_ascii_case(key) && !s.value.trim().is_empty()))
    }

    pub fn agent_engine_from_app_settings(
        db: &Database,
    ) -> Result<Option<AgentEngine>, FinallyAValueBotError> {
        let settings = db.list_app_settings()?;
        for s in settings {
            if s.key.eq_ignore_ascii_case(APP_SETTING_AGENT_ENGINE) {
                let v = s.value.trim();
                if !v.is_empty() {
                    return Ok(Some(AgentEngine::parse(v)));
                }
                break;
            }
        }
        Ok(None)
    }

    pub fn persist_tool_output_debug(
        db: &Database,
        enabled: bool,
    ) -> Result<(), FinallyAValueBotError> {
        persist_bool(db, APP_SETTING_TOOL_OUTPUT_DEBUG, enabled)
    }

    pub fn persist_post_tool_evaluator_enabled(
        db: &Database,
        enabled: bool,
    ) -> Result<(), FinallyAValueBotError> {
        persist_bool(db, APP_SETTING_POST_TOOL_EVALUATOR_ENABLED, enabled)
    }

    pub fn persist_response_quality_evaluator_enabled(
        db: &Database,
        enabled: bool,
    ) -> Result<(), FinallyAValueBotError> {
        persist_bool(db, APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED, enabled)
    }

    pub fn persist_agent_engine(
        db: &Database,
        engine: AgentEngine,
    ) -> Result<(), FinallyAValueBotError> {
        db.set_app_setting(APP_SETTING_AGENT_ENGINE, engine.as_str())
    }

    /// `true` when a non-empty row exists in `app_settings`.
    pub fn tool_output_debug_from_app_settings(
        db: &Database,
    ) -> Result<bool, FinallyAValueBotError> {
        Self::bool_from_app_settings(db, APP_SETTING_TOOL_OUTPUT_DEBUG)
    }

    pub fn post_tool_evaluator_from_app_settings(
        db: &Database,
    ) -> Result<bool, FinallyAValueBotError> {
        Self::bool_from_app_settings(db, APP_SETTING_POST_TOOL_EVALUATOR_ENABLED)
    }

    pub fn response_quality_evaluator_from_app_settings(
        db: &Database,
    ) -> Result<bool, FinallyAValueBotError> {
        Self::bool_from_app_settings(db, APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED)
    }

    /// Backward-compatible helper.
    pub fn merge_tool_output_debug_from_app_settings(
        env_default: bool,
        db: &Database,
    ) -> Result<bool, FinallyAValueBotError> {
        merge_bool_from_app_settings(db, APP_SETTING_TOOL_OUTPUT_DEBUG, env_default)
    }
}

fn merge_bool_from_app_settings(
    db: &Database,
    key: &str,
    env_default: bool,
) -> Result<bool, FinallyAValueBotError> {
    let settings = db.list_app_settings()?;
    for s in settings {
        if s.key.eq_ignore_ascii_case(key) {
            let v = s.value.trim();
            if !v.is_empty() {
                return Ok(crate::config::parse_bool_setting(v));
            }
            break;
        }
    }
    Ok(env_default)
}

fn merge_agent_engine_from_app_settings(
    db: &Database,
    env_default: AgentEngine,
) -> Result<AgentEngine, FinallyAValueBotError> {
    Ok(RuntimeToggles::agent_engine_from_app_settings(db)?.unwrap_or(env_default))
}

fn persist_bool(db: &Database, key: &str, enabled: bool) -> Result<(), FinallyAValueBotError> {
    db.set_app_setting(key, if enabled { "true" } else { "false" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_engine_from_str() {
        assert_eq!(
            AgentEngine::parse("deterministic"),
            AgentEngine::Deterministic
        );
        assert_eq!(AgentEngine::parse("cursor"), AgentEngine::Cursor);
        assert_eq!(AgentEngine::parse("classic"), AgentEngine::Classic);
        assert_eq!(
            AgentEngine::parse("classic_cost_routing"),
            AgentEngine::ClassicCostRouting
        );
        assert_eq!(AgentEngine::parse(""), AgentEngine::Classic);
    }
}
