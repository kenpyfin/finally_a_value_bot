//! Cursor SDK engine + cursor-agent CLI settings (persisted in `app_settings`, hot-reloaded).

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::Config;
use crate::db::Database;
use crate::error::FinallyAValueBotError;

pub const APP_SETTING_CURSOR_SDK_RUNNER_URL: &str = "CURSOR_SDK_RUNNER_URL";
pub const APP_SETTING_CURSOR_SDK_MODEL: &str = "CURSOR_SDK_MODEL";
pub const APP_SETTING_CURSOR_SDK_RUNNER_OK: &str = "CURSOR_SDK_RUNNER_OK";
pub const APP_SETTING_CURSOR_AGENT_CLI_PATH: &str = "CURSOR_AGENT_CLI_PATH";
pub const APP_SETTING_CURSOR_AGENT_MODEL: &str = "CURSOR_AGENT_MODEL";
pub const APP_SETTING_CURSOR_AGENT_RUNNER_URL: &str = "CURSOR_AGENT_RUNNER_URL";
pub const APP_SETTING_CURSOR_AGENT_TIMEOUT_SECS: &str = "CURSOR_AGENT_TIMEOUT_SECS";
pub const APP_SETTING_CURSOR_AGENT_TMUX_ENABLED: &str = "CURSOR_AGENT_TMUX_ENABLED";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CursorEngineSettings {
    pub sdk_runner_url: String,
    pub sdk_model: String,
    pub sdk_runner_ok: bool,
    pub cli_path: String,
    pub cli_model: String,
    pub cli_runner_url: String,
    pub timeout_secs: u64,
    pub tmux_enabled: bool,
}

impl Default for CursorEngineSettings {
    fn default() -> Self {
        Self {
            sdk_runner_url: String::new(),
            sdk_model: crate::config::default_cursor_sdk_model(),
            sdk_runner_ok: false,
            cli_path: crate::config::default_cursor_agent_cli_path(),
            cli_model: String::new(),
            cli_runner_url: String::new(),
            timeout_secs: 3600,
            tmux_enabled: true,
        }
    }
}

impl CursorEngineSettings {
    pub fn from_env(config: &Config) -> Self {
        let port = config.cursor_sdk_runner_port;
        let sdk_runner_url = config
            .cursor_sdk_runner_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| crate::cursor_sdk_sidecar::default_local_runner_url(port));
        Self {
            sdk_runner_url,
            sdk_model: config.cursor_sdk_model.trim().to_string(),
            sdk_runner_ok: false,
            cli_path: config.cursor_agent_cli_path.trim().to_string(),
            cli_model: config.cursor_agent_model.trim().to_string(),
            cli_runner_url: config
                .cursor_agent_runner_url
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string(),
            timeout_secs: config.cursor_agent_timeout_secs,
            tmux_enabled: config.cursor_agent_tmux_enabled,
        }
    }

    pub fn sdk_configured(&self) -> bool {
        !self.sdk_runner_url.trim().is_empty() && !self.sdk_model.trim().is_empty()
    }

    pub fn engine_ready(&self, health: &SidecarHealth) -> bool {
        self.sdk_configured()
            && self.sdk_runner_ok
            && health.reachable
            && health.api_key_configured
            && health.cursor_sdk_installed
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SidecarHealth {
    pub reachable: bool,
    pub api_key_configured: bool,
    pub cursor_sdk_installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

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

pub fn load_from_db(
    db: &Database,
    env: &Config,
) -> Result<CursorEngineSettings, FinallyAValueBotError> {
    let rows: Vec<(String, String)> = db
        .list_app_settings()?
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect();
    let mut cfg = CursorEngineSettings::from_env(env);

    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_SDK_RUNNER_URL) {
        cfg.sdk_runner_url = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_SDK_MODEL) {
        cfg.sdk_model = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_SDK_RUNNER_OK) {
        cfg.sdk_runner_ok = parse_bool(&v);
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_AGENT_CLI_PATH) {
        cfg.cli_path = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_AGENT_MODEL) {
        cfg.cli_model = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_AGENT_RUNNER_URL) {
        cfg.cli_runner_url = v;
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_AGENT_TIMEOUT_SECS) {
        if let Ok(n) = v.parse::<u64>() {
            cfg.timeout_secs = n.clamp(60, 86_400);
        }
    }
    if let Some(v) = read_setting(&rows, APP_SETTING_CURSOR_AGENT_TMUX_ENABLED) {
        cfg.tmux_enabled = parse_bool(&v);
    }

    if cfg.sdk_model.trim().is_empty() {
        cfg.sdk_model = crate::config::default_cursor_sdk_model();
    }
    if cfg.cli_path.trim().is_empty() {
        cfg.cli_path = crate::config::default_cursor_agent_cli_path();
    }
    if cfg.sdk_runner_url.trim().is_empty() {
        cfg.sdk_runner_url =
            crate::cursor_sdk_sidecar::default_local_runner_url(env.cursor_sdk_runner_port);
    }

    Ok(cfg)
}

pub fn persist_to_db(
    db: &Database,
    cfg: &CursorEngineSettings,
) -> Result<(), FinallyAValueBotError> {
    db.set_app_setting(APP_SETTING_CURSOR_SDK_RUNNER_URL, cfg.sdk_runner_url.trim())?;
    db.set_app_setting(APP_SETTING_CURSOR_SDK_MODEL, cfg.sdk_model.trim())?;
    db.set_app_setting(
        APP_SETTING_CURSOR_SDK_RUNNER_OK,
        if cfg.sdk_runner_ok { "true" } else { "false" },
    )?;
    db.set_app_setting(APP_SETTING_CURSOR_AGENT_CLI_PATH, cfg.cli_path.trim())?;
    db.set_app_setting(APP_SETTING_CURSOR_AGENT_MODEL, cfg.cli_model.trim())?;
    db.set_app_setting(
        APP_SETTING_CURSOR_AGENT_RUNNER_URL,
        cfg.cli_runner_url.trim(),
    )?;
    db.set_app_setting(
        APP_SETTING_CURSOR_AGENT_TIMEOUT_SECS,
        &cfg.timeout_secs.to_string(),
    )?;
    db.set_app_setting(
        APP_SETTING_CURSOR_AGENT_TMUX_ENABLED,
        if cfg.tmux_enabled { "true" } else { "false" },
    )?;
    Ok(())
}

pub fn validate_runner_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("Runner URL is required".into());
    }
    if !u.starts_with("http://") && !u.starts_with("https://") {
        return Err("Runner URL must start with http:// or https://".into());
    }
    Ok(())
}

pub async fn probe_sidecar_health(base_url: &str) -> SidecarHealth {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return SidecarHealth {
            reachable: false,
            api_key_configured: false,
            cursor_sdk_installed: false,
            error: Some("Runner URL is not configured".into()),
        };
    }

    let url = format!("{trimmed}/health");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return SidecarHealth {
                reachable: false,
                api_key_configured: false,
                cursor_sdk_installed: false,
                error: Some(format!("HTTP client error: {e}")),
            };
        }
    };

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(body) => SidecarHealth {
                reachable: true,
                api_key_configured: body
                    .get("api_key_configured")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                cursor_sdk_installed: body
                    .get("cursor_sdk_installed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                error: None,
            },
            Err(e) => SidecarHealth {
                reachable: true,
                api_key_configured: false,
                cursor_sdk_installed: false,
                error: Some(format!("Invalid health JSON: {e}")),
            },
        },
        Ok(resp) => SidecarHealth {
            reachable: false,
            api_key_configured: false,
            cursor_sdk_installed: false,
            error: Some(format!("Sidecar health returned HTTP {}", resp.status())),
        },
        Err(e) => SidecarHealth {
            reachable: false,
            api_key_configured: false,
            cursor_sdk_installed: false,
            error: Some(format!("Sidecar unreachable: {e}")),
        },
    }
}

pub async fn fetch_sidecar_models(base_url: &str) -> Result<Vec<String>, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Runner URL is not configured".into());
    }
    let url = format!("{trimmed}/models");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Sidecar models request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let detail = body
            .get("error")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("message").and_then(|v| v.as_str()))
            .unwrap_or("");
        if !detail.is_empty() {
            return Err(detail.to_string());
        }
        return Err(format!("Sidecar models returned HTTP {status}"));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid models JSON: {e}"))?;
    let models = body
        .get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    m.get("id")
                        .and_then(|id| id.as_str())
                        .map(str::to_string)
                        .or_else(|| m.as_str().map(str::to_string))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(models)
}

pub fn cli_on_path(cli_path: &str) -> bool {
    let path = cli_path.trim();
    if path.is_empty() {
        return false;
    }
    if path.contains('/') || path.contains('\\') {
        return std::path::Path::new(path).exists();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(path).exists()))
}

#[derive(Debug, Deserialize)]
pub struct CursorEnginePatchRequest {
    #[serde(default)]
    pub sdk_runner_url: Option<String>,
    #[serde(default)]
    pub sdk_model: Option<String>,
    #[serde(default)]
    pub cli_path: Option<String>,
    #[serde(default)]
    pub cli_model: Option<String>,
    #[serde(default)]
    pub cli_runner_url: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub tmux_enabled: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_runner_url_requires_http_scheme() {
        assert!(validate_runner_url("http://127.0.0.1:3848").is_ok());
        assert!(validate_runner_url("ftp://bad").is_err());
        assert!(validate_runner_url("").is_err());
    }

    #[test]
    fn engine_ready_requires_health_and_flag() {
        let cfg = CursorEngineSettings {
            sdk_runner_url: "http://127.0.0.1:3848".into(),
            sdk_model: "composer-2.5".into(),
            sdk_runner_ok: true,
            ..Default::default()
        };
        let health = SidecarHealth {
            reachable: true,
            api_key_configured: true,
            cursor_sdk_installed: true,
            error: None,
        };
        assert!(cfg.engine_ready(&health));
    }
}
