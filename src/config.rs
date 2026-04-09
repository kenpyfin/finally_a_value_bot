use crate::error::FinallyAValueBotError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_telegram_bot_token() -> String {
    String::new()
}
fn default_bot_username() -> String {
    String::new()
}
fn default_llm_provider() -> String {
    "anthropic".into()
}
fn default_api_key() -> String {
    String::new()
}
fn default_model() -> String {
    String::new()
}
fn default_max_tokens() -> u32 {
    8192
}
fn default_max_tool_iterations() -> usize {
    100
}
fn default_max_history_messages() -> usize {
    50
}
fn default_max_document_size_mb() -> u64 {
    100
}
fn default_workspace_dir() -> String {
    "./workspace".into()
}
fn default_timezone() -> String {
    "UTC".into()
}
fn default_whatsapp_webhook_port() -> u16 {
    8080
}
fn default_control_chat_ids() -> Vec<i64> {
    Vec::new()
}
fn default_web_enabled() -> bool {
    true
}
fn default_web_host() -> String {
    "127.0.0.1".into()
}
fn default_web_port() -> u16 {
    10961
}
fn default_web_max_inflight_per_session() -> usize {
    2
}
fn default_web_max_requests_per_window() -> usize {
    8
}
fn default_web_rate_window_seconds() -> u64 {
    10
}
fn default_web_run_history_limit() -> usize {
    512
}
fn default_web_session_idle_ttl_seconds() -> u64 {
    300
}
fn default_browser_managed() -> bool {
    false
}
fn default_browser_cdp_port_base() -> u16 {
    9222
}
fn default_browser_headless() -> bool {
    false
}
fn default_safety_output_guard_mode() -> String {
    "moderate".into()
}
fn default_safety_max_emojis_per_response() -> usize {
    12
}
fn default_safety_tail_repeat_limit() -> usize {
    8
}
fn default_safety_execution_mode() -> String {
    "warn_confirm".into()
}
fn default_safety_risky_categories() -> Vec<String> {
    vec![
        "destructive".into(),
        "system".into(),
        "network".into(),
        "package".into(),
    ]
}

#[cfg(target_os = "windows")]
pub(crate) fn default_cursor_agent_cli_path() -> String {
    "cursor-agent.cmd".into()
}
#[cfg(not(target_os = "windows"))]
pub(crate) fn default_cursor_agent_cli_path() -> String {
    "cursor-agent".into()
}

fn default_cursor_agent_model() -> String {
    String::new()
}

fn default_cursor_agent_timeout_secs() -> u64 {
    1500
}

fn default_scheduler_task_timeout_secs() -> u64 {
    3600
}

fn default_scheduler_stale_running_reclaim_secs() -> u64 {
    7200
}

fn default_scheduler_max_concurrent_tasks() -> usize {
    2
}

fn default_scheduler_poll_interval_secs() -> u64 {
    60
}

fn default_runtime_reliability_profile() -> String {
    "balanced".into()
}

fn default_workflow_auto_learn() -> bool {
    true
}

fn default_workflow_min_success_repetitions() -> usize {
    2
}

fn default_workflow_replay_strictness() -> String {
    "adaptive".into()
}

fn default_project_auto_association_strictness() -> String {
    "balanced".into()
}

fn default_orchestrator_enabled() -> bool {
    true
}

fn default_orchestrator_model() -> String {
    String::new()
}

fn default_tool_skill_agent_enabled() -> bool {
    false
}

fn default_tool_skill_agent_model() -> String {
    String::new()
}

fn default_post_tool_evaluator_enabled() -> bool {
    false
}

fn default_post_tool_evaluator_model() -> String {
    String::new()
}

fn default_cursor_agent_tmux_session_prefix() -> String {
    "finally_a_value_bot-cursor".into()
}

fn default_cursor_agent_tmux_enabled() -> bool {
    true
}

fn is_local_web_host(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h == "127.0.0.1" || h == "localhost" || h == "::1"
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SocialPlatformConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SocialConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub tiktok: SocialPlatformConfig,
    #[serde(default)]
    pub instagram: SocialPlatformConfig,
    #[serde(default)]
    pub linkedin: SocialPlatformConfig,
}

/// Optional vault/vector DB config for ORIGIN Obsidian vault integration.
/// Paths are relative to workspace_dir unless absolute.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VaultConfig {
    /// ORIGIN vault path relative to workspace_dir (e.g. "shared/ORIGIN").
    #[serde(default)]
    pub origin_vault_path: Option<String>,
    /// ChromaDB persistence dir relative to workspace_dir (e.g. "shared/vault_db").
    #[serde(default)]
    pub vector_db_path: Option<String>,
    /// Git repo URL to clone/pull vault (for sync service). Env: VAULT_ORIGIN_VAULT_REPO or VAULT_GIT_URL.
    #[serde(default)]
    pub origin_vault_repo: Option<String>,
    /// Embedding server URL (e.g. "http://10.0.1.211:8080" for llama.cpp).
    #[serde(default)]
    pub embedding_server_url: Option<String>,
    /// Search command; use "{query}" as placeholder for the query.
    #[serde(default)]
    pub vault_search_command: Option<String>,
    /// Index command to run after vault updates.
    #[serde(default)]
    pub vault_index_command: Option<String>,
    /// Override principles file path relative to workspace_dir (e.g. "shared/ORIGIN/AGENTS.md"). Default: "AGENTS.md" at workspace root.
    #[serde(default)]
    pub principles_path: Option<String>,
    /// ChromaDB HTTP server URL (e.g. "http://localhost:8000"). Required for the native search_vault tool.
    #[serde(default)]
    pub vector_db_url: Option<String>,
    /// ChromaDB collection name (default: "vault").
    #[serde(default)]
    pub vector_db_collection: Option<String>,
}

impl SocialConfig {
    pub fn is_platform_enabled(&self, platform: &str) -> bool {
        let (id, secret) = match platform {
            "tiktok" => (
                self.tiktok.client_id.as_deref().unwrap_or(""),
                self.tiktok.client_secret.as_deref().unwrap_or(""),
            ),
            "instagram" => (
                self.instagram.client_id.as_deref().unwrap_or(""),
                self.instagram.client_secret.as_deref().unwrap_or(""),
            ),
            "linkedin" => (
                self.linkedin.client_id.as_deref().unwrap_or(""),
                self.linkedin.client_secret.as_deref().unwrap_or(""),
            ),
            _ => return false,
        };
        !id.trim().is_empty() && !secret.trim().is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_telegram_bot_token")]
    pub telegram_bot_token: String,
    #[serde(default = "default_bot_username")]
    pub bot_username: String,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    #[serde(default = "default_api_key")]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub llm_base_url: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,
    #[serde(default = "default_max_history_messages")]
    pub max_history_messages: usize,
    #[serde(default = "default_max_document_size_mb")]
    pub max_document_size_mb: u64,
    /// Single root for runtime, skills, and tool workspace (shared). Layout: workspace_dir/runtime, workspace_dir/skills, workspace_dir/shared. Copy this folder to migrate.
    #[serde(default = "default_workspace_dir")]
    pub workspace_dir: String,
    #[serde(default)]
    pub openai_api_key: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub allowed_groups: Vec<i64>,
    #[serde(default = "default_control_chat_ids")]
    pub control_chat_ids: Vec<i64>,
    #[serde(default)]
    pub whatsapp_access_token: Option<String>,
    #[serde(default)]
    pub whatsapp_phone_number_id: Option<String>,
    #[serde(default)]
    pub whatsapp_verify_token: Option<String>,
    #[serde(default = "default_whatsapp_webhook_port")]
    pub whatsapp_webhook_port: u16,
    #[serde(default)]
    pub discord_bot_token: Option<String>,
    #[serde(default)]
    pub discord_allowed_channels: Vec<u64>,
    #[serde(default)]
    pub show_thinking: bool,
    #[serde(default = "default_web_enabled")]
    pub web_enabled: bool,
    #[serde(default = "default_web_host")]
    pub web_host: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
    #[serde(default)]
    pub web_auth_token: Option<String>,
    #[serde(default = "default_web_max_inflight_per_session")]
    pub web_max_inflight_per_session: usize,
    #[serde(default = "default_web_max_requests_per_window")]
    pub web_max_requests_per_window: usize,
    #[serde(default = "default_web_rate_window_seconds")]
    pub web_rate_window_seconds: u64,
    #[serde(default = "default_web_run_history_limit")]
    pub web_run_history_limit: usize,
    #[serde(default = "default_web_session_idle_ttl_seconds")]
    pub web_session_idle_ttl_seconds: u64,
    /// When set, web UI uses this chat_id for all requests (single universal contact across channels). Env: UNIVERSAL_CHAT_ID.
    #[serde(default)]
    pub universal_chat_id: Option<i64>,
    #[serde(default = "default_browser_managed")]
    pub browser_managed: bool,
    #[serde(default)]
    pub browser_executable_path: Option<String>,
    #[serde(default = "default_browser_cdp_port_base")]
    pub browser_cdp_port_base: u16,
    /// Optional idle timeout (seconds) for managed browser processes. 0 or None = no idle shutdown.
    #[serde(default)]
    pub browser_idle_timeout_secs: Option<u64>,
    #[serde(default = "default_browser_headless")]
    pub browser_headless: bool,
    /// Output repetition guard mode: off | moderate | strict.
    #[serde(default = "default_safety_output_guard_mode")]
    pub safety_output_guard_mode: String,
    /// Max emoji-like characters allowed in one assistant response before trimming.
    #[serde(default = "default_safety_max_emojis_per_response")]
    pub safety_max_emojis_per_response: usize,
    /// Max repeated tail-pattern count allowed before trimming repetitive suffixes.
    #[serde(default = "default_safety_tail_repeat_limit")]
    pub safety_tail_repeat_limit: usize,
    /// Execution safety mode: off | warn_confirm | strict.
    #[serde(default = "default_safety_execution_mode")]
    pub safety_execution_mode: String,
    /// Risky command categories monitored by execution safety.
    #[serde(default = "default_safety_risky_categories")]
    pub safety_risky_categories: Vec<String>,
    /// Full path to the agent-browser CLI (npm). If set, the browser tool uses this instead of looking up "agent-browser" on PATH. Use when the process PATH doesn't include agent-browser (e.g. when run as a service).
    #[serde(default)]
    pub agent_browser_path: Option<String>,
    /// Optional SearXNG instance URL for web_search (e.g. https://search.example.org). When set, web_search uses this instead of DuckDuckGo HTML. Env: SEARXNG_URL.
    #[serde(default)]
    pub web_search_searxng_url: Option<String>,
    /// Path to the cursor-agent CLI. Default: "cursor-agent" (or "cursor-agent.cmd" on Windows). Use when the process PATH doesn't include cursor-agent.
    #[serde(default = "default_cursor_agent_cli_path")]
    pub cursor_agent_cli_path: String,
    /// Model for cursor-agent (e.g. "gpt-5"). Leave empty to omit --model (cursor-agent uses its default / "auto").
    #[serde(default = "default_cursor_agent_model")]
    pub cursor_agent_model: String,
    /// Timeout in seconds for cursor-agent runs. Default: 1500.
    #[serde(default = "default_cursor_agent_timeout_secs")]
    pub cursor_agent_timeout_secs: u64,
    #[serde(default)]
    pub social: Option<SocialConfig>,
    /// Optional vault/vector DB config for ORIGIN Obsidian vault integration.
    #[serde(default)]
    pub vault: Option<VaultConfig>,
    /// When true, use orchestrator-first flow: orchestrator plans (direct or delegate), sub-agents run tools; no tools in main context. Default true.
    #[serde(default = "default_orchestrator_enabled")]
    pub orchestrator_enabled: bool,
    /// Optional model override for orchestrator (e.g. faster/cheaper). If empty, use main model.
    #[serde(default = "default_orchestrator_model")]
    pub orchestrator_model: String,
    /// [Legacy] When true and orchestrator disabled, gate tool use via TSA. Default false; orchestrator-first flow does not use TSA.
    #[serde(default = "default_tool_skill_agent_enabled")]
    pub tool_skill_agent_enabled: bool,
    /// Optional model for TSA (e.g. faster/cheaper). If empty, use orchestrator_model or main model.
    #[serde(default = "default_tool_skill_agent_model")]
    pub tool_skill_agent_model: String,
    /// Post-Tool Evaluator (PTE): evaluate task completion after each tool iteration. Default false.
    #[serde(default = "default_post_tool_evaluator_enabled")]
    pub post_tool_evaluator_enabled: bool,
    /// Optional model for PTE (e.g. faster/cheaper). If empty, use orchestrator_model or main model.
    #[serde(default = "default_post_tool_evaluator_model")]
    pub post_tool_evaluator_model: String,
    /// Tmux session name prefix for cursor_agent when detach=true (e.g. finally_a_value_bot-cursor).
    #[serde(default = "default_cursor_agent_tmux_session_prefix")]
    pub cursor_agent_tmux_session_prefix: String,
    /// Allow spawning cursor_agent in tmux when detach=true. Set false in Docker or when tmux unavailable.
    #[serde(default = "default_cursor_agent_tmux_enabled")]
    pub cursor_agent_tmux_enabled: bool,
    /// URL of a host runner that executes cursor-agent (e.g. http://host.docker.internal:3847). When set, the bot POSTs spawn requests instead of running cursor-agent locally.
    #[serde(default)]
    pub cursor_agent_runner_url: Option<String>,
    /// Max wall-clock time (seconds) for a single scheduled-agent run. Default 3600.
    #[serde(default = "default_scheduler_task_timeout_secs")]
    pub scheduler_task_timeout_secs: u64,
    /// Reclaim `scheduled_tasks` stuck in `running` if the claim timestamp is older than this (seconds). Default 7200.
    #[serde(default = "default_scheduler_stale_running_reclaim_secs")]
    pub scheduler_stale_running_reclaim_secs: u64,
    /// Max concurrent scheduled task runs (semaphore). Default 2.
    #[serde(default = "default_scheduler_max_concurrent_tasks")]
    pub scheduler_max_concurrent_tasks: usize,
    /// Seconds between scheduler ticks (reclaim + due-task scan). Default 60.
    #[serde(default = "default_scheduler_poll_interval_secs")]
    pub scheduler_poll_interval_secs: u64,
    /// Runtime reliability profile: balanced | aggressive_completion | safe_conservative.
    #[serde(default = "default_runtime_reliability_profile")]
    pub runtime_reliability_profile: String,
    /// Enable auto-learning workflows from successful repeated runs.
    #[serde(default = "default_workflow_auto_learn")]
    pub workflow_auto_learn: bool,
    /// Minimum repeated successful runs before workflow confidence is promoted.
    #[serde(default = "default_workflow_min_success_repetitions")]
    pub workflow_min_success_repetitions: usize,
    /// Workflow replay mode: strict | adaptive | loose.
    #[serde(default = "default_workflow_replay_strictness")]
    pub workflow_replay_strictness: String,
    /// Project auto-linking mode: strict | balanced | loose.
    #[serde(default = "default_project_auto_association_strictness")]
    pub project_auto_association_strictness: String,
}

impl Config {
    /// Data root directory (workspace root). Layout: runtime/, skills/, shared/ under this path.
    pub fn data_root_dir(&self) -> PathBuf {
        PathBuf::from(&self.workspace_dir)
    }

    /// Working directory for tools (same as workspace root; tools use workspace_dir/shared).
    pub fn working_dir(&self) -> &str {
        &self.workspace_dir
    }

    /// Runtime data directory (db, memory, exports, etc.).
    pub fn runtime_data_dir(&self) -> String {
        self.data_root_dir()
            .join("runtime")
            .to_string_lossy()
            .to_string()
    }

    /// Skills directory under data root.
    pub fn skills_data_dir(&self) -> String {
        self.data_root_dir()
            .join("skills")
            .to_string_lossy()
            .to_string()
    }

    /// Absolute path to the skills directory. Use this in the system prompt so the bot writes skill files to the real skills dir (file tools resolve relative paths from workspace_dir/shared).
    pub fn skills_data_dir_absolute(&self) -> std::path::PathBuf {
        self.workspace_root_absolute().join("skills")
    }

    /// Absolute path to the workspace root (workspace_dir resolved to absolute).
    pub fn workspace_root_absolute(&self) -> std::path::PathBuf {
        let root = PathBuf::from(&self.workspace_dir);
        if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| root.clone())
                .join(&self.workspace_dir)
        }
    }

    /// Resolve path to .env file. FINALLY_A_VALUE_BOT_CONFIG can override (points to .env).
    pub fn resolve_config_path() -> Result<Option<PathBuf>, FinallyAValueBotError> {
        if let Ok(custom) = std::env::var("FINALLY_A_VALUE_BOT_CONFIG") {
            let p = std::path::Path::new(&custom);
            if p.exists() {
                return Ok(Some(PathBuf::from(custom)));
            }
            return Err(FinallyAValueBotError::Config(format!(
                "FINALLY_A_VALUE_BOT_CONFIG points to non-existent file: {custom}"
            )));
        }
        if std::path::Path::new("./.env").exists() {
            return Ok(Some(PathBuf::from("./.env")));
        }
        Ok(None)
    }

    fn env(key: &str) -> Option<String> {
        std::env::var(key).ok().and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
    }

    fn env_u32(key: &str, default: u32) -> u32 {
        Self::env(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    fn env_u64(key: &str, default: u64) -> u64 {
        Self::env(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    fn env_usize(key: &str, default: usize) -> usize {
        Self::env(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    fn env_u16(key: &str, default: u16) -> u16 {
        Self::env(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    fn env_bool(key: &str, default: bool) -> bool {
        match Self::env(key).as_deref().map(|s| s.to_lowercase()) {
            Some(s) => match s.as_str() {
                "1" | "true" | "yes" => true,
                "0" | "false" | "no" => false,
                _ => default,
            },
            None => default,
        }
    }

    fn env_vec_i64(key: &str) -> Vec<i64> {
        Self::env(key)
            .map(|s| {
                s.split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn env_vec_u64(key: &str) -> Vec<u64> {
        Self::env(key)
            .map(|s| {
                s.split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }
    fn env_vec_string(key: &str) -> Vec<String> {
        Self::env(key)
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_ascii_lowercase())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Load config from environment (.env file + process env). Load .env from FINALLY_A_VALUE_BOT_CONFIG path or ./
    pub fn load() -> Result<Self, FinallyAValueBotError> {
        let env_path = Self::resolve_config_path()?;
        let load_path = env_path.as_deref().unwrap_or(std::path::Path::new("./.env"));
        if load_path.exists() {
            dotenvy::from_path(load_path)
                .map_err(|e| FinallyAValueBotError::Config(format!("Failed to load .env: {e}")))?;
        } else if env_path.is_none() {
            return Err(FinallyAValueBotError::Config(
                "No .env found. Run `finally_a_value_bot setup` to create one.".into(),
            ));
        }

        let mut config = Self::load_from_env();
        config.post_deserialize()?;
        Ok(config)
    }

    /// Load config from a specific .env file path (e.g. for config wizard).
    pub fn load_from_path(path: &std::path::Path) -> Result<Self, FinallyAValueBotError> {
        if path.exists() {
            dotenvy::from_path(path)
                .map_err(|e| FinallyAValueBotError::Config(format!("Failed to load .env: {e}")))?;
        }
        let mut config = Self::load_from_env();
        config.post_deserialize()?;
        Ok(config)
    }

    /// Build Config from current environment (after dotenvy has loaded .env).
    fn load_from_env() -> Self {
        let vault = {
            let has_vault = Self::env("VAULT_ORIGIN_VAULT_PATH").is_some()
                || Self::env("VAULT_VECTOR_DB_PATH").is_some()
                || Self::env("VAULT_ORIGIN_VAULT_REPO").is_some()
                || Self::env("VAULT_GIT_URL").is_some()
                || Self::env("VAULT_EMBEDDING_SERVER_URL").is_some()
                || Self::env("VAULT_VECTOR_DB_URL").is_some();
            if has_vault {
                Some(VaultConfig {
                    origin_vault_path: Self::env("VAULT_ORIGIN_VAULT_PATH"),
                    vector_db_path: Self::env("VAULT_VECTOR_DB_PATH"),
                    origin_vault_repo: Self::env("VAULT_ORIGIN_VAULT_REPO")
                        .or_else(|| Self::env("VAULT_GIT_URL")),
                    embedding_server_url: Self::env("VAULT_EMBEDDING_SERVER_URL"),
                    vault_search_command: Self::env("VAULT_SEARCH_COMMAND"),
                    vault_index_command: Self::env("VAULT_INDEX_COMMAND"),
                    principles_path: Self::env("VAULT_PRINCIPLES_PATH"),
                    vector_db_url: Self::env("VAULT_VECTOR_DB_URL"),
                    vector_db_collection: Self::env("VAULT_VECTOR_DB_COLLECTION"),
                })
            } else {
                None
            }
        };

        let social = {
            let has_social = Self::env("SOCIAL_BASE_URL").is_some()
                || Self::env("SOCIAL_TIKTOK_CLIENT_ID").is_some()
                || Self::env("SOCIAL_INSTAGRAM_CLIENT_ID").is_some()
                || Self::env("SOCIAL_LINKEDIN_CLIENT_ID").is_some();
            if has_social {
                Some(SocialConfig {
                    base_url: Self::env("SOCIAL_BASE_URL"),
                    tiktok: SocialPlatformConfig {
                        client_id: Self::env("SOCIAL_TIKTOK_CLIENT_ID"),
                        client_secret: Self::env("SOCIAL_TIKTOK_CLIENT_SECRET"),
                    },
                    instagram: SocialPlatformConfig {
                        client_id: Self::env("SOCIAL_INSTAGRAM_CLIENT_ID"),
                        client_secret: Self::env("SOCIAL_INSTAGRAM_CLIENT_SECRET"),
                    },
                    linkedin: SocialPlatformConfig {
                        client_id: Self::env("SOCIAL_LINKEDIN_CLIENT_ID"),
                        client_secret: Self::env("SOCIAL_LINKEDIN_CLIENT_SECRET"),
                    },
                })
            } else {
                None
            }
        };

        Config {
            telegram_bot_token: Self::env("TELEGRAM_BOT_TOKEN").unwrap_or_default(),
            bot_username: Self::env("BOT_USERNAME").unwrap_or_default(),
            llm_provider: Self::env("LLM_PROVIDER").unwrap_or_else(default_llm_provider),
            api_key: Self::env("LLM_API_KEY").unwrap_or_else(default_api_key),
            model: Self::env("LLM_MODEL").unwrap_or_default(),
            llm_base_url: Self::env("LLM_BASE_URL"),
            max_tokens: Self::env_u32("MAX_TOKENS", default_max_tokens()),
            max_tool_iterations: Self::env_usize("MAX_TOOL_ITERATIONS", default_max_tool_iterations()),
            max_history_messages: Self::env_usize("MAX_HISTORY_MESSAGES", default_max_history_messages()),
            max_document_size_mb: Self::env_u64("MAX_DOCUMENT_SIZE_MB", default_max_document_size_mb()),
            workspace_dir: Self::env("WORKSPACE_DIR")
                .unwrap_or_else(default_workspace_dir),
            openai_api_key: Self::env("OPENAI_API_KEY"),
            timezone: Self::env("TIMEZONE").unwrap_or_else(default_timezone),
            allowed_groups: Self::env_vec_i64("ALLOWED_GROUPS"),
            control_chat_ids: Self::env_vec_i64("CONTROL_CHAT_IDS"),
            whatsapp_access_token: Self::env("WHATSAPP_ACCESS_TOKEN"),
            whatsapp_phone_number_id: Self::env("WHATSAPP_PHONE_NUMBER_ID"),
            whatsapp_verify_token: Self::env("WHATSAPP_VERIFY_TOKEN"),
            whatsapp_webhook_port: Self::env_u16("WHATSAPP_WEBHOOK_PORT", default_whatsapp_webhook_port()),
            discord_bot_token: Self::env("DISCORD_BOT_TOKEN"),
            discord_allowed_channels: Self::env_vec_u64("DISCORD_ALLOWED_CHANNELS"),
            show_thinking: Self::env_bool("SHOW_THINKING", false),
            web_enabled: Self::env_bool("WEB_ENABLED", default_web_enabled()),
            web_host: Self::env("WEB_HOST").unwrap_or_else(default_web_host),
            web_port: Self::env_u16("WEB_PORT", default_web_port()),
            web_auth_token: Self::env("WEB_AUTH_TOKEN"),
            web_max_inflight_per_session: Self::env_usize(
                "WEB_MAX_INFLIGHT_PER_SESSION",
                default_web_max_inflight_per_session(),
            ),
            web_max_requests_per_window: Self::env_usize(
                "WEB_MAX_REQUESTS_PER_WINDOW",
                default_web_max_requests_per_window(),
            ),
            web_rate_window_seconds: Self::env_u64(
                "WEB_RATE_WINDOW_SECONDS",
                default_web_rate_window_seconds(),
            ),
            web_run_history_limit: Self::env_usize(
                "WEB_RUN_HISTORY_LIMIT",
                default_web_run_history_limit(),
            ),
            web_session_idle_ttl_seconds: Self::env_u64(
                "WEB_SESSION_IDLE_TTL_SECONDS",
                default_web_session_idle_ttl_seconds(),
            ),
            universal_chat_id: Self::env("UNIVERSAL_CHAT_ID").and_then(|s| s.parse().ok()),
            browser_managed: Self::env_bool("BROWSER_MANAGED", default_browser_managed()),
            browser_executable_path: Self::env("BROWSER_EXECUTABLE_PATH"),
            browser_cdp_port_base: Self::env_u16(
                "BROWSER_CDP_PORT_BASE",
                default_browser_cdp_port_base(),
            ),
            browser_idle_timeout_secs: Self::env("BROWSER_IDLE_TIMEOUT_SECS").and_then(|s| s.parse().ok()),
            browser_headless: Self::env_bool("BROWSER_HEADLESS", default_browser_headless()),
            safety_output_guard_mode: Self::env("SAFETY_OUTPUT_GUARD_MODE")
                .unwrap_or_else(default_safety_output_guard_mode),
            safety_max_emojis_per_response: Self::env_usize(
                "SAFETY_MAX_EMOJIS_PER_RESPONSE",
                default_safety_max_emojis_per_response(),
            ),
            safety_tail_repeat_limit: Self::env_usize(
                "SAFETY_TAIL_REPEAT_LIMIT",
                default_safety_tail_repeat_limit(),
            ),
            safety_execution_mode: Self::env("SAFETY_EXECUTION_MODE")
                .unwrap_or_else(default_safety_execution_mode),
            safety_risky_categories: {
                let parsed = Self::env_vec_string("SAFETY_RISKY_CATEGORIES");
                if parsed.is_empty() {
                    default_safety_risky_categories()
                } else {
                    parsed
                }
            },
            agent_browser_path: Self::env("AGENT_BROWSER_PATH"),
            web_search_searxng_url: Self::env("SEARXNG_URL"),
            cursor_agent_cli_path: Self::env("CURSOR_AGENT_CLI_PATH")
                .unwrap_or_else(default_cursor_agent_cli_path),
            cursor_agent_model: Self::env("CURSOR_AGENT_MODEL").unwrap_or_default(),
            cursor_agent_timeout_secs: Self::env_u64(
                "CURSOR_AGENT_TIMEOUT_SECS",
                default_cursor_agent_timeout_secs(),
            ),
            social,
            vault,
            orchestrator_enabled: Self::env_bool(
                "ORCHESTRATOR_ENABLED",
                default_orchestrator_enabled(),
            ),
            orchestrator_model: Self::env("ORCHESTRATOR_MODEL").unwrap_or_default(),
            tool_skill_agent_enabled: Self::env_bool(
                "TOOL_SKILL_AGENT_ENABLED",
                default_tool_skill_agent_enabled(),
            ),
            tool_skill_agent_model: Self::env("TOOL_SKILL_AGENT_MODEL").unwrap_or_default(),
            post_tool_evaluator_enabled: Self::env_bool(
                "POST_TOOL_EVALUATOR_ENABLED",
                default_post_tool_evaluator_enabled(),
            ),
            post_tool_evaluator_model: Self::env("POST_TOOL_EVALUATOR_MODEL").unwrap_or_default(),
            cursor_agent_tmux_session_prefix: Self::env("CURSOR_AGENT_TMUX_SESSION_PREFIX")
                .unwrap_or_else(default_cursor_agent_tmux_session_prefix),
            cursor_agent_tmux_enabled: Self::env_bool(
                "CURSOR_AGENT_TMUX_ENABLED",
                default_cursor_agent_tmux_enabled(),
            ),
            cursor_agent_runner_url: Self::env("CURSOR_AGENT_RUNNER_URL")
                .filter(|s| !s.trim().is_empty()),
            scheduler_task_timeout_secs: Self::env_u64(
                "SCHEDULER_TASK_TIMEOUT_SECS",
                default_scheduler_task_timeout_secs(),
            ),
            scheduler_stale_running_reclaim_secs: Self::env_u64(
                "SCHEDULER_STALE_RUNNING_RECLAIM_SECS",
                default_scheduler_stale_running_reclaim_secs(),
            ),
            scheduler_max_concurrent_tasks: Self::env_usize(
                "SCHEDULER_MAX_CONCURRENT_TASKS",
                default_scheduler_max_concurrent_tasks(),
            ),
            scheduler_poll_interval_secs: Self::env_u64(
                "SCHEDULER_POLL_INTERVAL_SECS",
                default_scheduler_poll_interval_secs(),
            ),
            runtime_reliability_profile: Self::env("RUNTIME_RELIABILITY_PROFILE")
                .unwrap_or_else(default_runtime_reliability_profile),
            workflow_auto_learn: Self::env_bool(
                "WORKFLOW_AUTO_LEARN",
                default_workflow_auto_learn(),
            ),
            workflow_min_success_repetitions: Self::env_usize(
                "WORKFLOW_MIN_SUCCESS_REPETITIONS",
                default_workflow_min_success_repetitions(),
            ),
            workflow_replay_strictness: Self::env("WORKFLOW_REPLAY_STRICTNESS")
                .unwrap_or_else(default_workflow_replay_strictness),
            project_auto_association_strictness: Self::env("PROJECT_AUTO_ASSOCIATION_STRICTNESS")
                .unwrap_or_else(default_project_auto_association_strictness),
        }
    }

    /// Apply post-deserialization normalization and validation.
    pub(crate) fn post_deserialize(&mut self) -> Result<(), FinallyAValueBotError> {
        self.llm_provider = self.llm_provider.trim().to_lowercase();
        self.safety_output_guard_mode = self.safety_output_guard_mode.trim().to_ascii_lowercase();
        self.safety_execution_mode = self.safety_execution_mode.trim().to_ascii_lowercase();
        self.runtime_reliability_profile = self.runtime_reliability_profile.trim().to_ascii_lowercase();
        self.workflow_replay_strictness = self.workflow_replay_strictness.trim().to_ascii_lowercase();
        self.project_auto_association_strictness = self
            .project_auto_association_strictness
            .trim()
            .to_ascii_lowercase();
        self.safety_risky_categories = self
            .safety_risky_categories
            .iter()
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty())
            .collect();

        // Apply provider-specific default model if empty
        if self.model.is_empty() {
            self.model = match self.llm_provider.as_str() {
                "anthropic" => "claude-sonnet-4-5-20250929".into(),
                "ollama" => "llama3.2".into(),
                "llama" | "llamacpp" => "local".into(),
                "google" => "gemini-2.5-flash".into(),
                _ => "gpt-5.2".into(),
            };
        }

        // Validate timezone
        self.timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| FinallyAValueBotError::Config(format!("Invalid timezone: {}", self.timezone)))?;

        // Filter empty llm_base_url
        if let Some(ref url) = self.llm_base_url {
            if url.trim().is_empty() {
                self.llm_base_url = None;
            }
        }
        if self.llm_base_url.is_none()
            && matches!(self.llm_provider.as_str(), "llama" | "llamacpp")
        {
            self.llm_base_url = Some("http://127.0.0.1:8080/v1".into());
        }
        if let Ok(dir) = std::env::var("FINALLY_A_VALUE_BOT_WORKSPACE_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                self.workspace_dir = trimmed.to_string();
            }
        }
        if self.workspace_dir.trim().is_empty() {
            self.workspace_dir = default_workspace_dir();
        }
        if self.web_host.trim().is_empty() {
            self.web_host = default_web_host();
        }
        if let Some(token) = &self.web_auth_token {
            if token.trim().is_empty() {
                self.web_auth_token = None;
            }
        }
        if self.web_enabled && !is_local_web_host(&self.web_host) && self.web_auth_token.is_none() {
            return Err(FinallyAValueBotError::Config(
                "web_auth_token is required when web_enabled=true and web_host is not local".into(),
            ));
        }
        if self.web_max_inflight_per_session == 0 {
            self.web_max_inflight_per_session = default_web_max_inflight_per_session();
        }
        if self.web_max_requests_per_window == 0 {
            self.web_max_requests_per_window = default_web_max_requests_per_window();
        }
        if self.web_rate_window_seconds == 0 {
            self.web_rate_window_seconds = default_web_rate_window_seconds();
        }
        if self.web_run_history_limit == 0 {
            self.web_run_history_limit = default_web_run_history_limit();
        }
        if self.web_session_idle_ttl_seconds == 0 {
            self.web_session_idle_ttl_seconds = default_web_session_idle_ttl_seconds();
        }
        if self.max_document_size_mb == 0 {
            self.max_document_size_mb = default_max_document_size_mb();
        }
        if self.safety_max_emojis_per_response == 0 {
            self.safety_max_emojis_per_response = default_safety_max_emojis_per_response();
        }
        if self.safety_tail_repeat_limit == 0 {
            self.safety_tail_repeat_limit = default_safety_tail_repeat_limit();
        }
        if self.safety_risky_categories.is_empty() {
            self.safety_risky_categories = default_safety_risky_categories();
        }
        let valid_guard_modes = ["off", "moderate", "strict"];
        if !valid_guard_modes.contains(&self.safety_output_guard_mode.as_str()) {
            return Err(FinallyAValueBotError::Config(format!(
                "Invalid safety_output_guard_mode: {} (expected off|moderate|strict)",
                self.safety_output_guard_mode
            )));
        }
        let valid_exec_modes = ["off", "warn_confirm", "strict"];
        if !valid_exec_modes.contains(&self.safety_execution_mode.as_str()) {
            return Err(FinallyAValueBotError::Config(format!(
                "Invalid safety_execution_mode: {} (expected off|warn_confirm|strict)",
                self.safety_execution_mode
            )));
        }
        let valid_risky_categories = ["destructive", "system", "network", "package"];
        for cat in &self.safety_risky_categories {
            if !valid_risky_categories.contains(&cat.as_str()) {
                return Err(FinallyAValueBotError::Config(format!(
                    "Invalid safety risky category: {} (expected one of destructive,system,network,package)",
                    cat
                )));
            }
        }
        match self.runtime_reliability_profile.as_str() {
            "aggressive_completion" => {
                if self.max_tool_iterations < 80 {
                    self.max_tool_iterations = 80;
                }
                self.post_tool_evaluator_enabled = true;
            }
            "safe_conservative" => {
                self.max_tool_iterations = self.max_tool_iterations.min(60);
                self.post_tool_evaluator_enabled = true;
            }
            _ => {
                self.runtime_reliability_profile = "balanced".to_string();
            }
        }
        if !["strict", "adaptive", "loose"].contains(&self.workflow_replay_strictness.as_str()) {
            self.workflow_replay_strictness = default_workflow_replay_strictness();
        }
        if !["strict", "balanced", "loose"].contains(&self.project_auto_association_strictness.as_str()) {
            self.project_auto_association_strictness =
                default_project_auto_association_strictness();
        }
        if self.workflow_min_success_repetitions == 0 {
            self.workflow_min_success_repetitions = default_workflow_min_success_repetitions();
        }
        // Expand ~ in agent_browser_path if present
        if let Some(ref p) = self.agent_browser_path {
            let trimmed = p.trim();
            if !trimmed.is_empty() && (trimmed == "~" || trimmed.starts_with("~/")) {
                if let Ok(home) = std::env::var("HOME") {
                    let expanded = if trimmed == "~" {
                        home
                    } else {
                        format!("{}{}", home, &trimmed[1..])
                    };
                    self.agent_browser_path = Some(expanded);
                }
            }
        }
        if let Some(ref mut social) = self.social {
            for platform_cfg in [
                &mut social.tiktok,
                &mut social.instagram,
                &mut social.linkedin,
            ] {
                if let Some(ref id) = platform_cfg.client_id {
                    if id.trim().is_empty() {
                        platform_cfg.client_id = None;
                    }
                }
                if let Some(ref secret) = platform_cfg.client_secret {
                    if secret.trim().is_empty() {
                        platform_cfg.client_secret = None;
                    }
                }
            }
        }

        // Validate required fields
        if self.telegram_bot_token.is_empty() && self.discord_bot_token.is_none() {
            return Err(FinallyAValueBotError::Config(
                "At least one of telegram_bot_token or discord_bot_token must be set".into(),
            ));
        }
        if self.api_key.is_empty()
            && !matches!(self.llm_provider.as_str(), "ollama" | "llama" | "llamacpp")
        {
            return Err(FinallyAValueBotError::Config("api_key is required".into()));
        }

        Ok(())
    }

    /// Save config as YAML to the given path (legacy; prefer save_env).
    #[allow(dead_code)]
    pub fn save_yaml(&self, path: &str) -> Result<(), FinallyAValueBotError> {
        let content = serde_yaml::to_string(self)
            .map_err(|e| FinallyAValueBotError::Config(format!("Failed to serialize config: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Save config as .env to the given path.
    pub fn save_env(&self, path: &std::path::Path) -> Result<(), FinallyAValueBotError> {
        fn esc(s: &str) -> String {
            if s.contains(' ') || s.contains('"') || s.contains('#') || s.is_empty() {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                s.to_string()
            }
        }
        let mut lines = Vec::new();
        lines.push("# FinallyAValueBot configuration".into());
        lines.push("".into());
        lines.push("# Telegram".into());
        lines.push(format!("TELEGRAM_BOT_TOKEN={}", esc(&self.telegram_bot_token)));
        lines.push(format!("BOT_USERNAME={}", esc(&self.bot_username)));
        lines.push("".into());
        lines.push("# LLM".into());
        lines.push(format!("LLM_PROVIDER={}", esc(&self.llm_provider)));
        lines.push(format!("LLM_API_KEY={}", esc(&self.api_key)));
        if !self.model.is_empty() {
            lines.push(format!("LLM_MODEL={}", esc(&self.model)));
        }
        if let Some(ref u) = self.llm_base_url {
            if !u.is_empty() {
                lines.push(format!("LLM_BASE_URL={}", esc(u)));
            }
        }
        lines.push(format!("MAX_TOKENS={}", self.max_tokens));
        lines.push(format!("MAX_TOOL_ITERATIONS={}", self.max_tool_iterations));
        lines.push(format!("MAX_HISTORY_MESSAGES={}", self.max_history_messages));
        lines.push(format!(
            "RUNTIME_RELIABILITY_PROFILE={}",
            esc(&self.runtime_reliability_profile)
        ));
        lines.push(format!(
            "WORKFLOW_AUTO_LEARN={}",
            if self.workflow_auto_learn { "true" } else { "false" }
        ));
        lines.push(format!(
            "WORKFLOW_MIN_SUCCESS_REPETITIONS={}",
            self.workflow_min_success_repetitions
        ));
        lines.push(format!(
            "WORKFLOW_REPLAY_STRICTNESS={}",
            esc(&self.workflow_replay_strictness)
        ));
        lines.push(format!(
            "PROJECT_AUTO_ASSOCIATION_STRICTNESS={}",
            esc(&self.project_auto_association_strictness)
        ));
        lines.push(format!("MAX_DOCUMENT_SIZE_MB={}", self.max_document_size_mb));
        lines.push(format!("SHOW_THINKING={}", if self.show_thinking { "true" } else { "false" }));
        lines.push("".into());
        lines.push("# Workspace".into());
        lines.push(format!("WORKSPACE_DIR={}", esc(&self.workspace_dir)));
        lines.push(format!("TIMEZONE={}", esc(&self.timezone)));
        if let Some(id) = self.universal_chat_id {
            lines.push(format!("UNIVERSAL_CHAT_ID={id}"));
        }
        lines.push("".into());
        lines.push("# Web".into());
        lines.push(format!("WEB_ENABLED={}", if self.web_enabled { "true" } else { "false" }));
        lines.push(format!("WEB_HOST={}", esc(&self.web_host)));
        lines.push(format!("WEB_PORT={}", self.web_port));
        if let Some(ref token) = self.web_auth_token {
            if !token.is_empty() {
                lines.push(format!("WEB_AUTH_TOKEN={}", esc(token)));
            }
        }
        lines.push(format!(
            "WEB_MAX_INFLIGHT_PER_SESSION={}",
            self.web_max_inflight_per_session
        ));
        lines.push(format!(
            "WEB_MAX_REQUESTS_PER_WINDOW={}",
            self.web_max_requests_per_window
        ));
        lines.push(format!(
            "WEB_RATE_WINDOW_SECONDS={}",
            self.web_rate_window_seconds
        ));
        lines.push(format!("WEB_RUN_HISTORY_LIMIT={}", self.web_run_history_limit));
        lines.push(format!(
            "WEB_SESSION_IDLE_TTL_SECONDS={}",
            self.web_session_idle_ttl_seconds
        ));
        lines.push("".into());
        lines.push("# Runtime safety".into());
        lines.push(format!(
            "SAFETY_OUTPUT_GUARD_MODE={}",
            esc(&self.safety_output_guard_mode)
        ));
        lines.push(format!(
            "SAFETY_MAX_EMOJIS_PER_RESPONSE={}",
            self.safety_max_emojis_per_response
        ));
        lines.push(format!(
            "SAFETY_TAIL_REPEAT_LIMIT={}",
            self.safety_tail_repeat_limit
        ));
        lines.push(format!(
            "SAFETY_EXECUTION_MODE={}",
            esc(&self.safety_execution_mode)
        ));
        lines.push(format!(
            "SAFETY_RISKY_CATEGORIES={}",
            esc(&self.safety_risky_categories.join(","))
        ));
        if let Some(ref v) = self.vault {
            lines.push("".into());
            lines.push("# ORIGIN vault".into());
            lines.push(format!(
                "VAULT_ORIGIN_VAULT_PATH={}",
                esc(v.origin_vault_path.as_deref().unwrap_or("shared/ORIGIN"))
            ));
            lines.push(format!(
                "VAULT_VECTOR_DB_PATH={}",
                esc(v.vector_db_path.as_deref().unwrap_or("shared/vault_db"))
            ));
            if let Some(ref r) = v.origin_vault_repo {
                if !r.is_empty() {
                    lines.push(format!("VAULT_ORIGIN_VAULT_REPO={}", esc(r)));
                }
            }
        }
        std::fs::write(path, lines.join("\n"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn test_config() -> Config {
        Config {
            telegram_bot_token: "tok".into(),
            bot_username: "bot".into(),
            llm_provider: "anthropic".into(),
            api_key: "key".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            llm_base_url: None,
            max_tokens: 8192,
            max_tool_iterations: 100,
            max_history_messages: 50,
            max_document_size_mb: 100,
            workspace_dir: "./workspace".into(),
            openai_api_key: None,
            timezone: "UTC".into(),
            allowed_groups: vec![],
            control_chat_ids: vec![],
            max_session_messages: 40,
            compact_keep_recent: 20,
            whatsapp_access_token: None,
            whatsapp_phone_number_id: None,
            whatsapp_verify_token: None,
            whatsapp_webhook_port: 8080,
            discord_bot_token: None,
            discord_allowed_channels: vec![],
            show_thinking: false,
            web_enabled: true,
            web_host: "127.0.0.1".into(),
            web_port: 10961,
            web_auth_token: None,
            web_max_inflight_per_session: 2,
            web_max_requests_per_window: 8,
            web_rate_window_seconds: 10,
            web_run_history_limit: 512,
            web_session_idle_ttl_seconds: 300,
            universal_chat_id: None,
            browser_managed: false,
            browser_executable_path: None,
            browser_cdp_port_base: 9222,
            browser_idle_timeout_secs: None,
            browser_headless: false,
            safety_output_guard_mode: "moderate".into(),
            safety_max_emojis_per_response: 12,
            safety_tail_repeat_limit: 8,
            safety_execution_mode: "warn_confirm".into(),
            safety_risky_categories: vec![
                "destructive".into(),
                "system".into(),
                "network".into(),
                "package".into(),
            ],
            agent_browser_path: None,
            web_search_searxng_url: None,
            cursor_agent_cli_path: default_cursor_agent_cli_path(),
            cursor_agent_model: String::new(),
            cursor_agent_timeout_secs: 1500,
            social: None,
            vault: None,
            orchestrator_enabled: true,
            orchestrator_model: String::new(),
            tool_skill_agent_enabled: true,
            tool_skill_agent_model: String::new(),
            post_tool_evaluator_enabled: false,
            post_tool_evaluator_model: String::new(),
            delegate_tool_enabled: true,
            delegate_max_iterations: 10,
            delegate_model: String::new(),
            cursor_agent_tmux_session_prefix: "finally_a_value_bot-cursor".into(),
            cursor_agent_tmux_enabled: true,
            cursor_agent_runner_url: None,
            scheduler_task_timeout_secs: default_scheduler_task_timeout_secs(),
            scheduler_stale_running_reclaim_secs: default_scheduler_stale_running_reclaim_secs(),
            scheduler_max_concurrent_tasks: default_scheduler_max_concurrent_tasks(),
            scheduler_poll_interval_secs: default_scheduler_poll_interval_secs(),
            runtime_reliability_profile: default_runtime_reliability_profile(),
            workflow_auto_learn: default_workflow_auto_learn(),
            workflow_min_success_repetitions: default_workflow_min_success_repetitions(),
            workflow_replay_strictness: default_workflow_replay_strictness(),
            project_auto_association_strictness: default_project_auto_association_strictness(),
        }
    }

    #[test]
    fn test_config_struct_clone_and_debug() {
        let config = test_config();
        let cloned = config.clone();
        assert_eq!(cloned.telegram_bot_token, "tok");
        assert_eq!(cloned.max_tokens, 8192);
        assert_eq!(cloned.max_tool_iterations, 100);
        assert_eq!(cloned.max_history_messages, 50);
        assert_eq!(cloned.max_document_size_mb, 100);
        assert!(cloned.openai_api_key.is_none());
        assert_eq!(cloned.timezone, "UTC");
        assert!(cloned.allowed_groups.is_empty());
        assert!(cloned.control_chat_ids.is_empty());
        assert_eq!(cloned.max_session_messages, 40);
        assert_eq!(cloned.compact_keep_recent, 20);
        assert!(cloned.discord_bot_token.is_none());
        assert!(cloned.discord_allowed_channels.is_empty());
        let _ = format!("{:?}", config);
    }

    #[test]
    fn test_config_default_values() {
        let mut config = test_config();
        config.openai_api_key = Some("sk-test".into());
        config.timezone = "US/Eastern".into();
        config.allowed_groups = vec![123, 456];
        config.control_chat_ids = vec![999];
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert_eq!(config.workspace_dir, "./workspace");
        assert_eq!(config.openai_api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.timezone, "US/Eastern");
        assert_eq!(config.allowed_groups, vec![123, 456]);
        assert_eq!(config.control_chat_ids, vec![999]);
        assert_eq!(config.safety_output_guard_mode, "moderate");
        assert_eq!(config.safety_max_emojis_per_response, 12);
        assert_eq!(config.safety_tail_repeat_limit, 8);
        assert_eq!(config.safety_execution_mode, "warn_confirm");
        assert_eq!(
            config.safety_risky_categories,
            vec!["destructive", "system", "network", "package"]
        );
    }

    #[test]
    fn test_config_yaml_roundtrip() {
        let config = test_config();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.telegram_bot_token, "tok");
        assert_eq!(parsed.max_tokens, 8192);
        assert_eq!(parsed.llm_provider, "anthropic");
    }

    #[test]
    fn test_config_yaml_defaults() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.llm_provider, "anthropic");
        assert_eq!(config.max_tokens, 8192);
        assert_eq!(config.max_tool_iterations, 100);
        assert_eq!(config.workspace_dir, "./workspace");
        assert_eq!(config.max_document_size_mb, 100);
        assert_eq!(config.timezone, "UTC");
    }

    #[test]
    fn test_post_deserialize_empty_workspace_dir_uses_default() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nworkspace_dir: '  '\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.workspace_dir, "./workspace");
    }

    #[test]
    fn test_config_post_deserialize() {
        let yaml =
            "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nllm_provider: ANTHROPIC\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.llm_provider, "anthropic");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_runtime_and_skills_dirs_from_workspace_dir() {
        let mut config = test_config();
        config.workspace_dir = "./workspace".into();
        assert!(config.runtime_data_dir().ends_with("workspace/runtime"));
        assert!(config.skills_data_dir().ends_with("workspace/skills"));
    }

    #[test]
    fn test_workspace_dir_default() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.workspace_dir, "./workspace");
    }

    #[test]
    fn test_post_deserialize_invalid_timezone() {
        let yaml =
            "telegram_bot_token: tok\nbot_username: bot\napi_key: key\ntimezone: Mars/Olympus\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.post_deserialize().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid timezone"));
    }

    #[test]
    fn test_post_deserialize_missing_api_key() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.post_deserialize().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("api_key is required"));
    }

    #[test]
    fn test_post_deserialize_missing_bot_tokens() {
        let yaml = "bot_username: bot\napi_key: key\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.post_deserialize().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("telegram_bot_token or discord_bot_token"));
    }

    #[test]
    fn test_post_deserialize_discord_only() {
        let yaml = "bot_username: bot\napi_key: key\ndiscord_bot_token: discord_tok\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        // Should succeed: discord_bot_token is set even though telegram_bot_token is empty
        config.post_deserialize().unwrap();
    }

    #[test]
    fn test_post_deserialize_openai_default_model() {
        let yaml =
            "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nllm_provider: openai\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.model, "gpt-5.2");
    }

    #[test]
    fn test_post_deserialize_ollama_default_model_and_empty_key() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\nllm_provider: ollama\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.model, "llama3.2");
    }

    #[test]
    fn test_post_deserialize_llama_default_model_base_url_and_empty_key() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\nllm_provider: llama\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.model, "local");
        assert_eq!(
            config.llm_base_url.as_deref(),
            Some("http://127.0.0.1:8080/v1")
        );
    }

    #[test]
    fn test_post_deserialize_empty_base_url_becomes_none() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nllm_base_url: '  '\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert!(config.llm_base_url.is_none());
    }

    #[test]
    fn test_post_deserialize_provider_case_insensitive() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nllm_provider: '  ANTHROPIC  '\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.llm_provider, "anthropic");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_post_deserialize_invalid_safety_output_guard_mode() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nsafety_output_guard_mode: noisy\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.post_deserialize().unwrap_err();
        assert!(err
            .to_string()
            .contains("Invalid safety_output_guard_mode"));
    }

    #[test]
    fn test_post_deserialize_invalid_safety_execution_mode() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nsafety_execution_mode: ask-first\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.post_deserialize().unwrap_err();
        assert!(err.to_string().contains("Invalid safety_execution_mode"));
    }

    #[test]
    fn test_post_deserialize_web_non_local_requires_token() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nweb_enabled: true\nweb_host: 0.0.0.0\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        let err = config.post_deserialize().unwrap_err();
        assert!(err
            .to_string()
            .contains("web_auth_token is required when web_enabled=true"));
    }

    #[test]
    fn test_post_deserialize_web_non_local_with_token_ok() {
        let yaml = "telegram_bot_token: tok\nbot_username: bot\napi_key: key\nweb_enabled: true\nweb_host: 0.0.0.0\nweb_auth_token: token123\n";
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.web_auth_token.as_deref(), Some("token123"));
    }

    #[test]
    fn test_config_yaml_with_all_optional_fields() {
        let yaml = r#"
telegram_bot_token: tok
bot_username: bot
api_key: key
openai_api_key: sk-test
timezone: US/Eastern
allowed_groups: [123, 456]
control_chat_ids: [999]
max_session_messages: 60
compact_keep_recent: 30
whatsapp_access_token: wa_token
whatsapp_phone_number_id: phone_id
whatsapp_verify_token: verify
whatsapp_webhook_port: 9090
discord_bot_token: discord_tok
discord_allowed_channels: [111, 222]
"#;
        let mut config: Config = serde_yaml::from_str(yaml).unwrap();
        config.post_deserialize().unwrap();
        assert_eq!(config.openai_api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.timezone, "US/Eastern");
        assert_eq!(config.allowed_groups, vec![123, 456]);
        assert_eq!(config.control_chat_ids, vec![999]);
        assert_eq!(config.max_session_messages, 60);
        assert_eq!(config.compact_keep_recent, 30);
        assert_eq!(config.whatsapp_webhook_port, 9090);
        assert_eq!(config.discord_allowed_channels, vec![111, 222]);
    }

    #[test]
    fn test_config_save_yaml() {
        let config = test_config();
        let dir = std::env::temp_dir();
        let path = dir.join("finally_a_value_bot_test_config.yaml");
        config.save_yaml(path.to_str().unwrap()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("telegram_bot_token"));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_config_save_env_includes_runtime_and_web_keys() {
        let mut config = test_config();
        config.max_tokens = 4096;
        config.max_tool_iterations = 55;
        config.max_document_size_mb = 77;
        config.show_thinking = true;
        config.web_enabled = true;
        config.web_host = "0.0.0.0".into();
        config.web_port = 11999;
        config.web_auth_token = Some("secret123".into());
        config.web_max_inflight_per_session = 4;
        config.web_max_requests_per_window = 12;
        config.web_rate_window_seconds = 30;
        config.web_run_history_limit = 900;
        config.web_session_idle_ttl_seconds = 600;

        let dir = std::env::temp_dir();
        let path = dir.join("finally_a_value_bot_test_config.env");
        config.save_env(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("MAX_TOKENS=4096"));
        assert!(content.contains("MAX_TOOL_ITERATIONS=55"));
        assert!(content.contains("MAX_DOCUMENT_SIZE_MB=77"));
        assert!(content.contains("SHOW_THINKING=true"));
        assert!(content.contains("WEB_ENABLED=true"));
        assert!(content.contains("WEB_HOST=0.0.0.0"));
        assert!(content.contains("WEB_PORT=11999"));
        assert!(content.contains("WEB_AUTH_TOKEN=secret123"));
        assert!(content.contains("WEB_MAX_INFLIGHT_PER_SESSION=4"));
        assert!(content.contains("WEB_MAX_REQUESTS_PER_WINDOW=12"));
        assert!(content.contains("WEB_RATE_WINDOW_SECONDS=30"));
        assert!(content.contains("WEB_RUN_HISTORY_LIMIT=900"));
        assert!(content.contains("WEB_SESSION_IDLE_TTL_SECONDS=600"));
        std::fs::remove_file(path).ok();
    }
}
