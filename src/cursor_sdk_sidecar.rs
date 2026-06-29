//! Auto-start the embedded Cursor SDK Python sidecar when the bot starts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::config::Config;
use crate::cursor_engine_config::{
    self, CursorEngineSettings, SidecarHealth, APP_SETTING_CURSOR_SDK_RUNNER_OK,
    APP_SETTING_CURSOR_SDK_RUNNER_URL,
};
use crate::db::Database;
use crate::error::FinallyAValueBotError;

const HEALTH_POLL_INTERVAL_MS: u64 = 250;
const HEALTH_WAIT_MAX_SECS: u64 = 45;
const SIDECAR_VENV_DIR_NAME: &str = "cursor-sdk-venv";
const SIDECAR_PID_FILE: &str = "cursor-sdk-sidecar.pid";
const SIDECAR_PYTHON_PACKAGES: &[&str] = &["cursor-sdk", "aiohttp"];
const VENV_CREATE_TIMEOUT_SECS: u64 = 120;
const PIP_INSTALL_TIMEOUT_SECS: u64 = 180;

/// Keeps the sidecar child process alive for the bot lifetime (`kill_on_drop` on the child).
pub struct SidecarHandle {
    _child: Mutex<Option<Child>>,
    pub managed_locally: bool,
    pub runner_url: String,
}

pub fn default_local_runner_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn resolve_sidecar_script() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("CURSOR_SDK_RUNNER_SCRIPT") {
        let path = PathBuf::from(raw.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("scripts").join("cursor-sdk-runner.py");
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().take(6) {
            let path = ancestor.join("scripts").join("cursor-sdk-runner.py");
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn is_local_runner_url(url: &str, port: u16) -> bool {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed.is_empty()
        || trimmed == default_local_runner_url(port).trim_end_matches('/')
        || trimmed.starts_with("http://127.0.0.1:")
        || trimmed.starts_with("http://localhost:")
}

fn sidecar_venv_dir(config: &Config) -> PathBuf {
    PathBuf::from(config.runtime_data_dir()).join(SIDECAR_VENV_DIR_NAME)
}

fn sidecar_pid_path(config: &Config) -> PathBuf {
    PathBuf::from(config.runtime_data_dir()).join(SIDECAR_PID_FILE)
}

fn venv_python_executable(venv_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

fn base_python_executable(config: &Config) -> String {
    let python = config.cursor_sdk_python.trim();
    if python.is_empty() {
        "python3".to_string()
    } else {
        python.to_string()
    }
}

async fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    timeout_secs: u64,
) -> Result<(), String> {
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        Command::new(program).args(args).output(),
    )
    .await
    .map_err(|_| {
        format!(
            "timed out after {timeout_secs}s: {program} {}",
            args.join(" ")
        )
    })?
    .map_err(|e| format!("failed to run {program}: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "{program} {} failed (status={}): {}{}",
            args.join(" "),
            output.status,
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!(" | stdout: {}", stdout.trim())
            }
        ))
    }
}

async fn python_can_import(python: &Path, modules: &str) -> bool {
    let python_cmd = python.to_string_lossy().to_string();
    run_command_with_timeout(&python_cmd, &["-c", &format!("import {modules}")], 15)
        .await
        .is_ok()
}

async fn ensure_sidecar_venv(config: &Config) -> Result<PathBuf, String> {
    let venv_dir = sidecar_venv_dir(config);
    let python = venv_python_executable(&venv_dir);
    if python.is_file() && python_can_import(&python, "cursor_sdk, aiohttp").await {
        return Ok(python);
    }

    std::fs::create_dir_all(config.runtime_data_dir())
        .map_err(|e| format!("failed to create runtime dir for Cursor SDK venv: {e}"))?;

    let base_python = base_python_executable(config);
    if !python.is_file() {
        info!(
            "Creating Cursor SDK venv at {} using {base_python}",
            venv_dir.display()
        );
        let venv_path = venv_dir.to_string_lossy().to_string();
        run_command_with_timeout(
            &base_python,
            &["-m", "venv", &venv_path],
            VENV_CREATE_TIMEOUT_SECS,
        )
        .await
        .map_err(|e| format!("Cursor SDK venv creation failed: {e}"))?;
    }

    if !python.is_file() {
        return Err(format!(
            "Cursor SDK venv python not found at {}",
            python.display()
        ));
    }

    info!(
        "Installing Cursor SDK sidecar packages ({}) into {}",
        SIDECAR_PYTHON_PACKAGES.join(", "),
        venv_dir.display()
    );
    let python_cmd = python.to_string_lossy().to_string();
    run_command_with_timeout(
        &python_cmd,
        &["-m", "pip", "install", "--upgrade", "pip"],
        PIP_INSTALL_TIMEOUT_SECS,
    )
    .await
    .map_err(|e| format!("Cursor SDK pip bootstrap failed: {e}"))?;

    let mut pip_args: Vec<&str> = vec!["-m", "pip", "install"];
    pip_args.extend(SIDECAR_PYTHON_PACKAGES);
    run_command_with_timeout(&python_cmd, &pip_args, PIP_INSTALL_TIMEOUT_SECS)
        .await
        .map_err(|e| format!("Cursor SDK dependency install failed: {e}"))?;

    if !python_can_import(&python, "cursor_sdk, aiohttp").await {
        return Err(
            "Cursor SDK dependencies installed but import check failed (cursor_sdk, aiohttp)"
                .into(),
        );
    }

    info!(
        "Cursor SDK sidecar Python environment ready at {}",
        python.display()
    );
    Ok(python)
}

async fn resolve_sidecar_python(config: &Config) -> Result<PathBuf, String> {
    if config.cursor_sdk_auto_install {
        ensure_sidecar_venv(config).await
    } else {
        Ok(PathBuf::from(base_python_executable(config)))
    }
}

fn write_sidecar_pid(config: &Config, pid: u32) {
    let path = sidecar_pid_path(config);
    if let Err(e) = std::fs::write(&path, pid.to_string()) {
        warn!(
            "Failed to write Cursor SDK sidecar pid file {}: {e}",
            path.display()
        );
    }
}

fn read_sidecar_pid(config: &Config) -> Option<u32> {
    let raw = std::fs::read_to_string(sidecar_pid_path(config)).ok()?;
    raw.trim().parse().ok()
}

async fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = run_command_with_timeout("kill", &["-TERM", &pid.to_string()], 5).await;
    }
    #[cfg(windows)]
    {
        let _ = run_command_with_timeout("taskkill", &["/PID", &pid.to_string(), "/F"], 10).await;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
}

#[cfg(unix)]
async fn terminate_listener_on_port(port: u16) {
    let script = format!(
        "fuser -k {port}/tcp 2>/dev/null || lsof -ti:{port} | xargs -r kill -TERM 2>/dev/null"
    );
    let _ = run_command_with_timeout("sh", &["-c", &script], 10).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
}

#[cfg(not(unix))]
async fn terminate_listener_on_port(_port: u16) {}

async fn stop_recorded_sidecar(config: &Config) {
    if let Some(pid) = read_sidecar_pid(config) {
        info!("Stopping recorded Cursor SDK sidecar process (pid={pid})");
        terminate_pid(pid).await;
    }
    let _ = std::fs::remove_file(sidecar_pid_path(config));
}

async fn wait_for_sidecar_health(url: &str) -> SidecarHealth {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(HEALTH_WAIT_MAX_SECS);
    loop {
        let health = cursor_engine_config::probe_sidecar_health(url).await;
        if health.reachable {
            return health;
        }
        if tokio::time::Instant::now() >= deadline {
            return health;
        }
        tokio::time::sleep(Duration::from_millis(HEALTH_POLL_INTERVAL_MS)).await;
    }
}

fn in_docker() -> bool {
    std::env::var("FINALLY_A_VALUE_BOT_IN_DOCKER").as_deref() == Ok("1")
        || Path::new("/.dockerenv").exists()
}

impl SidecarHandle {
    pub fn inactive() -> Arc<Self> {
        Arc::new(Self {
            _child: Mutex::new(None),
            managed_locally: false,
            runner_url: default_local_runner_url(3848),
        })
    }
}

async fn spawn_sidecar_process(script: &Path, port: u16, python: &Path) -> Result<Child, String> {
    let mut cmd = Command::new(python);
    cmd.arg(script)
        .arg(port.to_string())
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    if let Ok(key) = std::env::var("CURSOR_API_KEY") {
        let key = key.trim();
        if !key.is_empty() {
            cmd.env("CURSOR_API_KEY", key);
        }
    }

    cmd.spawn().map_err(|e| {
        format!(
            "failed to spawn Cursor SDK sidecar ({}): {e}",
            python.display()
        )
    })
}

fn persist_bootstrap(
    db: &Database,
    runner_url: &str,
    runner_ok: bool,
) -> Result<(), FinallyAValueBotError> {
    db.set_app_setting(APP_SETTING_CURSOR_SDK_RUNNER_URL, runner_url.trim())?;
    db.set_app_setting(
        APP_SETTING_CURSOR_SDK_RUNNER_OK,
        if runner_ok { "true" } else { "false" },
    )?;
    Ok(())
}

fn sidecar_needs_restart(health: &SidecarHealth) -> bool {
    !health.reachable || !health.cursor_sdk_installed
}

/// Start (or attach to) the local sidecar, wait for health, and update `cursor_settings`.
pub async fn bootstrap(
    config: &Config,
    db: &Database,
    cursor_settings: Arc<std::sync::RwLock<CursorEngineSettings>>,
) -> Arc<SidecarHandle> {
    let port = config.cursor_sdk_runner_port;
    let mut settings = cursor_settings
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| CursorEngineSettings::from_env(config));

    if settings.sdk_runner_url.trim().is_empty() {
        settings.sdk_runner_url = default_local_runner_url(port);
    }

    let runner_url = settings.sdk_runner_url.clone();
    let mut managed_locally = false;
    let mut child: Option<Child> = None;

    if config.cursor_sdk_auto_start && !in_docker() && is_local_runner_url(&runner_url, port) {
        let sidecar_python = match resolve_sidecar_python(config).await {
            Ok(python) => python,
            Err(e) => {
                warn!("Cursor SDK sidecar Python setup failed: {e}");
                PathBuf::from(base_python_executable(config))
            }
        };

        let mut health = cursor_engine_config::probe_sidecar_health(&runner_url).await;
        if sidecar_needs_restart(&health) {
            if health.reachable && !health.cursor_sdk_installed {
                info!(
                    "Cursor SDK sidecar at {runner_url} is missing cursor-sdk; restarting with managed venv"
                );
                stop_recorded_sidecar(config).await;
                terminate_listener_on_port(port).await;
                health = cursor_engine_config::probe_sidecar_health(&runner_url).await;
            }

            if sidecar_needs_restart(&health) {
                if let Some(script) = resolve_sidecar_script() {
                    match spawn_sidecar_process(&script, port, &sidecar_python).await {
                        Ok(proc) => {
                            if let Some(pid) = proc.id() {
                                write_sidecar_pid(config, pid);
                            }
                            info!(
                                "Started Cursor SDK sidecar on {} (script={}, python={})",
                                runner_url,
                                script.display(),
                                sidecar_python.display()
                            );
                            child = Some(proc);
                            managed_locally = true;
                        }
                        Err(e) => {
                            warn!("Cursor SDK sidecar auto-start failed: {e}");
                        }
                    }
                } else {
                    warn!(
                        "Cursor SDK sidecar script not found (expected scripts/cursor-sdk-runner.py); \
                         set CURSOR_SDK_RUNNER_SCRIPT or run from repo root"
                    );
                }
            } else {
                info!("Cursor SDK sidecar already reachable at {runner_url}");
            }
        } else {
            info!("Cursor SDK sidecar already reachable at {runner_url}");
        }
    }

    let health = wait_for_sidecar_health(&runner_url).await;
    let runner_ok = health.reachable && health.api_key_configured && health.cursor_sdk_installed;
    settings.sdk_runner_url = runner_url.clone();
    settings.sdk_runner_ok = runner_ok;

    if let Err(e) = persist_bootstrap(db, &runner_url, runner_ok) {
        warn!("Failed to persist Cursor SDK bootstrap settings: {e}");
    }

    if let Ok(mut guard) = cursor_settings.write() {
        *guard = settings;
    }

    if runner_ok {
        info!("Cursor SDK sidecar ready at {runner_url}");
    } else if health.reachable && !health.cursor_sdk_installed {
        warn!(
            "Cursor SDK sidecar is up at {runner_url} but cursor-sdk is not installed in the sidecar Python; \
             restart the bot to auto-install (CURSOR_SDK_AUTO_INSTALL=true)"
        );
    } else if health.reachable {
        warn!(
            "Cursor SDK sidecar is up at {runner_url} but CURSOR_API_KEY is not set; \
             add it to repo-root .env for Cursor engine runs"
        );
    } else if config.cursor_sdk_auto_start {
        warn!(
            "Cursor SDK sidecar not ready at {runner_url}. \
             Ensure python3 is on PATH or set CURSOR_SDK_PYTHON."
        );
    }

    Arc::new(SidecarHandle {
        _child: Mutex::new(child),
        managed_locally,
        runner_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_runner_url_detection() {
        assert!(is_local_runner_url("http://127.0.0.1:3848", 3848));
        assert!(is_local_runner_url("", 3848));
        assert!(!is_local_runner_url("http://192.168.1.5:3848", 3848));
    }

    #[test]
    fn default_url_uses_port() {
        assert_eq!(default_local_runner_url(3848), "http://127.0.0.1:3848");
    }

    #[test]
    fn venv_python_path_unix_layout() {
        if cfg!(windows) {
            return;
        }
        let venv = PathBuf::from("/tmp/cursor-sdk-venv");
        assert_eq!(
            venv_python_executable(&venv),
            PathBuf::from("/tmp/cursor-sdk-venv/bin/python")
        );
    }

    #[test]
    fn sidecar_needs_restart_when_sdk_missing() {
        let health = SidecarHealth {
            reachable: true,
            api_key_configured: true,
            cursor_sdk_installed: false,
            error: None,
        };
        assert!(sidecar_needs_restart(&health));
    }
}
