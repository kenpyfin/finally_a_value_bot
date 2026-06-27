//! Hot-reloadable runtime flags persisted in `app_settings` (Web UI).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::db::Database;
use crate::error::FinallyAValueBotError;

pub const APP_SETTING_TOOL_OUTPUT_DEBUG: &str = "TOOL_OUTPUT_DEBUG";
pub const APP_SETTING_POST_TOOL_EVALUATOR_ENABLED: &str = "POST_TOOL_EVALUATOR_ENABLED";
pub const APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED: &str =
    "RESPONSE_QUALITY_EVALUATOR_ENABLED";

#[derive(Debug, Clone, Copy)]
pub struct RuntimeToggleInit {
    pub tool_output_debug: bool,
    pub post_tool_evaluator_enabled: bool,
    pub response_quality_evaluator_enabled: bool,
}

#[derive(Debug)]
pub struct RuntimeToggles {
    tool_output_debug: AtomicBool,
    post_tool_evaluator_enabled: AtomicBool,
    response_quality_evaluator_enabled: AtomicBool,
}

impl RuntimeToggles {
    pub fn new(tool_output_debug: bool) -> Arc<Self> {
        Self::from_init(RuntimeToggleInit {
            tool_output_debug,
            post_tool_evaluator_enabled: false,
            response_quality_evaluator_enabled: false,
        })
    }

    pub fn from_init(init: RuntimeToggleInit) -> Arc<Self> {
        Arc::new(Self {
            tool_output_debug: AtomicBool::new(init.tool_output_debug),
            post_tool_evaluator_enabled: AtomicBool::new(init.post_tool_evaluator_enabled),
            response_quality_evaluator_enabled: AtomicBool::new(
                init.response_quality_evaluator_enabled,
            ),
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
        })
    }

    pub fn bool_from_app_settings(db: &Database, key: &str) -> Result<bool, FinallyAValueBotError> {
        let settings = db.list_app_settings()?;
        Ok(settings
            .iter()
            .any(|s| s.key.eq_ignore_ascii_case(key) && !s.value.trim().is_empty()))
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

fn persist_bool(db: &Database, key: &str, enabled: bool) -> Result<(), FinallyAValueBotError> {
    db.set_app_setting(key, if enabled { "true" } else { "false" })
}
