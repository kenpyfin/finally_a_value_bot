//! Hot-reloadable runtime flags persisted in `app_settings` (Web UI).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::db::Database;
use crate::error::FinallyAValueBotError;

pub const APP_SETTING_TOOL_OUTPUT_DEBUG: &str = "TOOL_OUTPUT_DEBUG";

#[derive(Debug)]
pub struct RuntimeToggles {
    tool_output_debug: AtomicBool,
}

impl RuntimeToggles {
    pub fn new(initial_tool_output_debug: bool) -> Arc<Self> {
        Arc::new(Self {
            tool_output_debug: AtomicBool::new(initial_tool_output_debug),
        })
    }

    pub fn tool_output_debug(&self) -> bool {
        self.tool_output_debug.load(Ordering::Relaxed)
    }

    pub fn set_tool_output_debug(&self, enabled: bool) {
        self.tool_output_debug.store(enabled, Ordering::Relaxed);
    }

    /// `true` when a non-empty row exists in `app_settings`.
    pub fn tool_output_debug_from_app_settings(
        db: &Database,
    ) -> Result<bool, FinallyAValueBotError> {
        let settings = db.list_app_settings()?;
        Ok(settings.iter().any(|s| {
            s.key.eq_ignore_ascii_case(APP_SETTING_TOOL_OUTPUT_DEBUG) && !s.value.trim().is_empty()
        }))
    }

    pub fn merge_tool_output_debug_from_app_settings(
        env_default: bool,
        db: &Database,
    ) -> Result<bool, FinallyAValueBotError> {
        let settings = db.list_app_settings()?;
        for s in settings {
            if s.key.eq_ignore_ascii_case(APP_SETTING_TOOL_OUTPUT_DEBUG) {
                let v = s.value.trim();
                if !v.is_empty() {
                    return Ok(crate::config::parse_bool_setting(v));
                }
                break;
            }
        }
        Ok(env_default)
    }

    pub fn persist_tool_output_debug(
        db: &Database,
        enabled: bool,
    ) -> Result<(), FinallyAValueBotError> {
        db.set_app_setting(
            APP_SETTING_TOOL_OUTPUT_DEBUG,
            if enabled { "true" } else { "false" },
        )
    }
}
