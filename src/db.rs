use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::error::FinallyAValueBotError;

pub struct Database {
    conn: Mutex<Connection>,
}

pub async fn call_blocking<T, F>(
    db: std::sync::Arc<Database>,
    f: F,
) -> Result<T, FinallyAValueBotError>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> Result<T, FinallyAValueBotError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || f(db.as_ref()))
        .await
        .map_err(|e| FinallyAValueBotError::ToolExecution(format!("DB task join error: {e}")))?
}

pub const MESSAGE_ORIGIN_INTERACTIVE: &str = "interactive";
pub const MESSAGE_ORIGIN_SCHEDULED: &str = "scheduled";

pub fn message_origin_interactive() -> String {
    MESSAGE_ORIGIN_INTERACTIVE.to_string()
}

pub fn message_origin_scheduled() -> String {
    MESSAGE_ORIGIN_SCHEDULED.to_string()
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub session_id: Option<String>,
    pub sender_name: String,
    pub content: String,
    pub is_from_bot: bool,
    pub timestamp: String,
    /// `interactive` (default) or `scheduled` (scheduler final delivery).
    pub origin: String,
}

fn stored_message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        persona_id: row.get(2)?,
        session_id: row.get(3)?,
        sender_name: row.get(4)?,
        content: row.get(5)?,
        is_from_bot: row.get::<_, i32>(6)? != 0,
        timestamp: row.get(7)?,
        origin: row
            .get::<_, Option<String>>(8)?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(message_origin_interactive),
    })
}

const MESSAGE_SELECT_COLS: &str =
    "id, chat_id, persona_id, session_id, sender_name, content, is_from_bot, timestamp, origin";

/// Main chat timeline: legacy rows plus focused sessions opted into main chat.
const MAIN_CHAT_MESSAGE_VISIBILITY: &str = "(
    session_id IS NULL
    OR EXISTS (
        SELECT 1 FROM chat_sessions cs
        WHERE cs.id = session_id AND cs.mirror_main_chat = 1
    )
)";

const CHAT_SESSION_SELECT_COLS: &str =
    "id, chat_id, persona_id, title, intent, status, created_at, last_active_at, archived_at, ttl_hours, bootstrap_context_json, mirror_main_chat";

fn chat_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        persona_id: row.get(2)?,
        title: row.get(3)?,
        intent: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        last_active_at: row.get(7)?,
        archived_at: row.get(8)?,
        ttl_hours: row.get(9)?,
        bootstrap_context_json: row.get(10)?,
        mirror_main_chat: row.get::<_, i64>(11)? != 0,
    })
}

#[derive(Debug, Clone)]
pub struct ChatSession {
    pub id: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub title: String,
    pub intent: String,
    pub status: String,
    pub created_at: String,
    pub last_active_at: String,
    pub archived_at: Option<String>,
    pub ttl_hours: i64,
    pub bootstrap_context_json: Option<String>,
    /// When true, session messages also appear on the persona main chat timeline.
    pub mirror_main_chat: bool,
}

#[derive(Debug, Clone)]
pub struct Persona {
    pub id: i64,
    pub chat_id: i64,
    pub name: String,
    pub model_override: Option<String>,
    /// When set, overrides global default for `trim_to_recent_balanced` minimum user messages.
    pub recent_history_min_user: Option<i64>,
    /// When set, overrides global default for `trim_to_recent_balanced` minimum assistant messages.
    pub recent_history_min_assistant: Option<i64>,
    /// Operator-authored steering note injected into the system prompt (web cockpit).
    pub operator_memo: Option<String>,
}

/// Maximum `operator_memo` length (characters) for storage and prompt injection.
pub const OPERATOR_MEMO_MAX_CHARS: usize = 4000;

#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub chat_id: i64,
    pub chat_title: Option<String>,
    pub chat_type: String,
    pub last_message_time: String,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TaskRunLog {
    pub id: i64,
    pub task_id: i64,
    pub chat_id: i64,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: i64,
    pub success: bool,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SocialOAuthToken {
    pub platform: String,
    pub chat_id: i64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScheduledTask {
    pub id: i64,
    pub chat_id: i64,
    pub persona_id: i64,
    pub prompt: String,
    pub schedule_type: String,  // "cron" or "once"
    pub schedule_value: String, // cron expression or ISO timestamp
    pub next_run: String,       // ISO timestamp
    pub last_run: Option<String>,
    pub status: String, // "active", "running", "paused", "completed", "cancelled"
    pub created_at: String,
}

/// Read a column as text, tolerating rows whose value was stored with BLOB affinity
/// (e.g. a prompt hand-edited via the sqlite CLI). SQLite is dynamically typed, so a
/// single BLOB in a TEXT column otherwise makes `row.get::<String>` fail and aborts the
/// entire query (which previously took down the scheduler and Schedules API).
fn row_text(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<String> {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx)? {
        ValueRef::Null => Ok(String::new()),
        ValueRef::Text(t) => Ok(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Ok(String::from_utf8_lossy(b).into_owned()),
        ValueRef::Integer(i) => Ok(i.to_string()),
        ValueRef::Real(f) => Ok(f.to_string()),
    }
}

/// Optional variant of [`row_text`]; `NULL` maps to `None`.
fn row_text_opt(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx)? {
        ValueRef::Null => Ok(None),
        ValueRef::Text(t) => Ok(Some(String::from_utf8_lossy(t).into_owned())),
        ValueRef::Blob(b) => Ok(Some(String::from_utf8_lossy(b).into_owned())),
        ValueRef::Integer(i) => Ok(Some(i.to_string())),
        ValueRef::Real(f) => Ok(Some(f.to_string())),
    }
}

/// Shared, blob-tolerant row mapping for `scheduled_tasks` queries. All list/lookup queries
/// must select columns in this order: id, chat_id, persona_id, prompt, schedule_type,
/// schedule_value, next_run, last_run, status, created_at.
fn map_scheduled_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        persona_id: row.get(2)?,
        prompt: row_text(row, 3)?,
        schedule_type: row_text(row, 4)?,
        schedule_value: row_text(row, 5)?,
        next_run: row_text(row, 6)?,
        last_run: row_text_opt(row, 7)?,
        status: row_text(row, 8)?,
        created_at: row_text(row, 9)?,
    })
}

/// External channel identity for delivery and persona policy. `0` = web (no bot token).
pub const BOT_INSTANCE_WEB: i64 = 0;
pub const BOT_INSTANCE_TELEGRAM_PRIMARY: i64 = 1;
pub const BOT_INSTANCE_DISCORD_PRIMARY: i64 = 2;
pub const BOT_INSTANCE_WHATSAPP_PRIMARY: i64 = 3;

#[derive(Debug, Clone)]
pub struct ChannelBotInstance {
    pub id: i64,
    pub platform: String,
    pub label: String,
    pub token: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ChannelBinding {
    pub canonical_chat_id: i64,
    pub bot_instance_id: i64,
    pub channel_type: String,
    pub channel_handle: String,
}

/// Merged bot instance + per-contact binding/policy for Settings → Channels.
#[derive(Debug, Clone)]
pub struct ContactChannelIntegrationRow {
    pub bot_instance_id: i64,
    pub platform: String,
    pub label: String,
    pub channel_handle: Option<String>,
    pub linked: bool,
    pub persona_mode: ChannelPersonaMode,
    pub persona_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct HookDefinitionRecord {
    pub id: i64,
    pub name: String,
    pub event_name: String,
    pub matcher: Option<String>,
    pub action_type: String,
    pub action_payload_json: String,
    /// `None` means global hook scope; `Some(vec![...])` means explicit persona allowlist.
    pub scoped_persona_ids: Option<Vec<i64>>,
    pub enabled: bool,
    pub updated_at: String,
}

impl HookDefinitionRecord {
    pub fn scoped_for_persona(&self, persona_id: i64) -> bool {
        self.scoped_persona_ids
            .as_ref()
            .map_or(true, |ids| ids.contains(&persona_id))
    }

    pub fn persona_status(
        &self,
        db: &Database,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<(bool, bool, bool), FinallyAValueBotError> {
        let scoped_for_persona = self.scoped_for_persona(persona_id);
        let allowed_for_persona = db.is_hook_allowed_for_persona(chat_id, persona_id, self.id)?;
        let active_for_persona = self.enabled && scoped_for_persona && allowed_for_persona;
        Ok((scoped_for_persona, allowed_for_persona, active_for_persona))
    }
}

#[derive(Debug, Clone)]
pub struct PersonaHookSkillPolicy {
    pub chat_id: i64,
    pub persona_id: i64,
    /// `None` means default allow-all. `Some(vec![])` means explicit allow-none.
    pub allowed_hook_ids: Option<Vec<i64>>,
    /// `None` means default allow-all. `Some(vec![])` means explicit allow-none.
    pub allowed_skill_names: Option<Vec<String>>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPersonaMode {
    All,
    Single,
}

impl ChannelPersonaMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Single => "single",
        }
    }
}

impl TryFrom<&str> for ChannelPersonaMode {
    type Error = FinallyAValueBotError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "all" => Ok(Self::All),
            "single" => Ok(Self::Single),
            other => Err(FinallyAValueBotError::ToolExecution(format!(
                "Invalid channel persona mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChannelPersonaPolicy {
    pub canonical_chat_id: i64,
    pub bot_instance_id: i64,
    pub mode: ChannelPersonaMode,
    pub persona_id: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct BackgroundJob {
    pub id: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub prompt: String,
    pub status: String, // "pending", "running", "completed_raw", "main_agent_processing", "done", "failed", "cancelled"
    pub trigger_reason: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub result_text: Option<String>,
    pub error_text: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub last_progress_at: Option<String>,
    pub last_stage: Option<String>,
    /// `agent` (default) or `shell`.
    pub job_kind: String,
    pub shell_command: Option<String>,
    pub workdir: Option<String>,
    pub tmux_session: Option<String>,
    pub output_path: Option<String>,
    pub exit_code: Option<i32>,
    pub label: Option<String>,
}

const BG_JOB_SELECT: &str = "SELECT id, chat_id, persona_id, prompt, status, trigger_reason, created_at, started_at, finished_at, result_text, error_text, lease_owner, lease_expires_at, last_progress_at, last_stage, job_kind, shell_command, workdir, tmux_session, output_path, exit_code, label";

fn map_background_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BackgroundJob> {
    Ok(BackgroundJob {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        persona_id: row.get(2)?,
        prompt: row.get(3)?,
        status: row.get(4)?,
        trigger_reason: row.get(5)?,
        created_at: row.get(6)?,
        started_at: row.get(7)?,
        finished_at: row.get(8)?,
        result_text: row.get(9)?,
        error_text: row.get(10)?,
        lease_owner: row.get(11)?,
        lease_expires_at: row.get(12)?,
        last_progress_at: row.get(13)?,
        last_stage: row.get(14)?,
        job_kind: row
            .get::<_, Option<String>>(15)?
            .unwrap_or_else(|| "agent".into()),
        shell_command: row.get(16)?,
        workdir: row.get(17)?,
        tmux_session: row.get(18)?,
        output_path: row.get(19)?,
        exit_code: row.get(20)?,
        label: row.get(21)?,
    })
}

#[derive(Debug, Clone)]
pub struct JobHeartbeat {
    pub run_key: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub job_type: String,
    pub stage: String,
    pub message: String,
    pub active: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ProjectRecord {
    pub id: i64,
    pub owner_chat_id: i64,
    pub title: String,
    pub project_type: String,
    pub status: String,
    pub canonical_path: Option<String>,
    pub metadata_json: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct RunTimelineEvent {
    pub id: i64,
    pub run_key: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PersonaBulletinEvent {
    pub id: i64,
    pub chat_id: i64,
    pub persona_id: i64,
    pub run_key: Option<String>,
    pub event_type: String,
    pub title: String,
    pub detail: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PersonaBulletinFocus {
    pub chat_id: i64,
    pub persona_id: i64,
    pub title: Option<String>,
    pub content: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct PersonaMessageBookmark {
    pub chat_id: i64,
    pub persona_id: i64,
    pub message_id: String,
    pub role: String,
    pub content_preview: String,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct CursorAgentRun {
    pub id: i64,
    pub chat_id: i64,
    pub channel: String,
    pub prompt_preview: String,
    pub workdir: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output_preview: Option<String>,
    pub output_path: Option<String>,
    /// When set, run was spawned in tmux (detach=true); session may still be running.
    pub tmux_session: Option<String>,
}

impl Database {
    pub fn new(data_dir: &str) -> Result<Self, FinallyAValueBotError> {
        let db_path = Path::new(data_dir).join("finally_a_value_bot.db");
        std::fs::create_dir_all(data_dir)?;

        let conn = Connection::open(db_path)?;
        // PRAGMA journal_mode returns a row; use query_row to consume it (execute_batch fails with extra_check)
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chats (
                chat_id INTEGER PRIMARY KEY,
                chat_title TEXT,
                chat_type TEXT NOT NULL DEFAULT 'private',
                last_message_time TEXT NOT NULL,
                active_persona_id INTEGER
            );

            CREATE TABLE IF NOT EXISTS personas (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                model_override TEXT,
                UNIQUE(chat_id, name)
            );

            CREATE INDEX IF NOT EXISTS idx_personas_chat_id
                ON personas(chat_id);

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                sender_name TEXT NOT NULL,
                content TEXT NOT NULL,
                is_from_bot INTEGER NOT NULL DEFAULT 0,
                timestamp TEXT NOT NULL,
                origin TEXT NOT NULL DEFAULT 'interactive',
                PRIMARY KEY (id, chat_id, persona_id)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_chat_timestamp
                ON messages(chat_id, persona_id, timestamp);

            CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                prompt TEXT NOT NULL,
                schedule_type TEXT NOT NULL DEFAULT 'cron',
                schedule_value TEXT NOT NULL,
                next_run TEXT NOT NULL,
                last_run TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_status_next
                ON scheduled_tasks(status, next_run);

            CREATE TABLE IF NOT EXISTS task_run_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                chat_id INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                success INTEGER NOT NULL DEFAULT 1,
                result_summary TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_task_run_logs_task_id
                ON task_run_logs(task_id);

            CREATE TABLE IF NOT EXISTS sessions (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                messages_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id)
            );

            CREATE TABLE IF NOT EXISTS social_oauth_tokens (
                platform TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                access_token TEXT NOT NULL,
                refresh_token TEXT,
                expires_at TEXT,
                PRIMARY KEY (platform, chat_id)
            );

            CREATE TABLE IF NOT EXISTS oauth_pending_states (
                state_token TEXT PRIMARY KEY,
                platform TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cursor_agent_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                channel TEXT NOT NULL,
                prompt_preview TEXT NOT NULL,
                workdir TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT NOT NULL,
                success INTEGER NOT NULL,
                exit_code INTEGER,
                output_preview TEXT,
                output_path TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_cursor_agent_runs_chat_id
                ON cursor_agent_runs(chat_id);
            CREATE INDEX IF NOT EXISTS idx_cursor_agent_runs_finished_at
                ON cursor_agent_runs(finished_at DESC);

            CREATE TABLE IF NOT EXISTS background_jobs (
                id TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                trigger_reason TEXT NOT NULL DEFAULT 'timeout',
                created_at TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                result_text TEXT,
                error_text TEXT,
                lease_owner TEXT,
                lease_expires_at TEXT,
                last_progress_at TEXT,
                last_stage TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_background_jobs_chat_id
                ON background_jobs(chat_id);
            CREATE INDEX IF NOT EXISTS idx_background_jobs_status
                ON background_jobs(status);

            CREATE TABLE IF NOT EXISTS job_heartbeats (
                run_key TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                job_type TEXT NOT NULL,
                stage TEXT NOT NULL,
                message TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_job_heartbeats_chat_id
                ON job_heartbeats(chat_id);
            CREATE INDEX IF NOT EXISTS idx_job_heartbeats_updated
                ON job_heartbeats(updated_at DESC);

            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                owner_chat_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                project_type TEXT NOT NULL DEFAULT 'general',
                status TEXT NOT NULL DEFAULT 'active',
                canonical_path TEXT,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                updated_at TEXT NOT NULL,
                UNIQUE(owner_chat_id, title)
            );
            CREATE INDEX IF NOT EXISTS idx_projects_owner_updated
                ON projects(owner_chat_id, updated_at DESC);

            CREATE TABLE IF NOT EXISTS project_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                run_key TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(project_id, run_key)
            );
            CREATE INDEX IF NOT EXISTS idx_project_runs_run_key
                ON project_runs(run_key);

            CREATE TABLE IF NOT EXISTS workflows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                owner_chat_id INTEGER NOT NULL,
                intent_signature TEXT NOT NULL,
                steps_json TEXT NOT NULL,
                step_trace_json TEXT NOT NULL DEFAULT '[]',
                approach_summary TEXT NOT NULL DEFAULT '',
                last_outcome TEXT NOT NULL DEFAULT 'unknown',
                failure_reason TEXT,
                evidence_json TEXT NOT NULL DEFAULT '[]',
                confidence REAL NOT NULL DEFAULT 0.0,
                version INTEGER NOT NULL DEFAULT 1,
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                last_used_at TEXT,
                updated_at TEXT NOT NULL,
                UNIQUE(owner_chat_id, intent_signature)
            );
            CREATE INDEX IF NOT EXISTS idx_workflows_owner_conf
                ON workflows(owner_chat_id, confidence DESC, updated_at DESC);

            CREATE TABLE IF NOT EXISTS workflow_executions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workflow_id INTEGER NOT NULL,
                run_key TEXT NOT NULL,
                outcome TEXT NOT NULL,
                score REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workflow_executions_workflow
                ON workflow_executions(workflow_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS run_timeline_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_key TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_run_timeline_events_run_key
                ON run_timeline_events(run_key, id ASC);
            CREATE INDEX IF NOT EXISTS idx_run_timeline_events_chat
                ON run_timeline_events(chat_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS channel_bindings (
                canonical_chat_id INTEGER NOT NULL,
                channel_type TEXT NOT NULL,
                channel_handle TEXT NOT NULL,
                PRIMARY KEY (channel_type, channel_handle),
                FOREIGN KEY (canonical_chat_id) REFERENCES chats(chat_id)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_bindings_canonical
                ON channel_bindings(canonical_chat_id);

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS hook_definitions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                event_name TEXT NOT NULL,
                matcher TEXT,
                action_type TEXT NOT NULL,
                action_payload_json TEXT NOT NULL DEFAULT '{}',
                enabled INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_hook_definitions_event_enabled
                ON hook_definitions(event_name, enabled, id);

            CREATE TABLE IF NOT EXISTS persona_hook_skill_policy (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                allowed_hook_ids_json TEXT,
                allowed_skill_names_json TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id)
            );
            CREATE INDEX IF NOT EXISTS idx_persona_hook_skill_policy_chat
                ON persona_hook_skill_policy(chat_id);

            CREATE TABLE IF NOT EXISTS channel_persona_policy (
                canonical_chat_id INTEGER NOT NULL,
                channel_type TEXT NOT NULL,
                mode TEXT NOT NULL DEFAULT 'all',
                persona_id INTEGER,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (canonical_chat_id, channel_type)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_persona_policy_chat
                ON channel_persona_policy(canonical_chat_id);",
        )?;

        Self::migrate_persona_schema(&conn)?;
        Self::migrate_scheduled_tasks_persona_schema(&conn)?;
        Self::migrate_channel_bindings(&conn)?;
        Self::migrate_channel_bot_instances_and_policy(&conn)?;
        Self::migrate_fts(&conn)?;
        Self::migrate_cursor_agent_runs_tmux(&conn)?;
        Self::migrate_drop_project_artifacts(&conn)?;
        Self::migrate_persona_bulletin_and_bookmarks(&conn)?;
        Self::migrate_workflow_learning_schema(&conn)?;
        Self::migrate_background_jobs_lease_schema(&conn)?;
        Self::migrate_background_jobs_shell_schema(&conn)?;
        Self::migrate_personas_prompt_context(&conn)?;
        Self::migrate_hook_policy_schema(&conn)?;
        Self::ensure_builtin_hook_definitions(&conn)?;
        Self::migrate_chat_sessions_schema(&conn)?;
        Self::migrate_messages_origin(&conn)?;
        Self::migrate_cursor_engine_agents(&conn)?;

        Ok(Database {
            conn: Mutex::new(conn),
        })
    }

    fn column_exists(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let pragma = format!("PRAGMA table_info({table})");
        let mut stmt = conn
            .prepare(&pragma)
            .map_err(|e| FinallyAValueBotError::ToolExecution(format!("pragma {table}: {e}")))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for r in rows {
            if r? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn migrate_channel_bot_instances_and_policy(
        conn: &Connection,
    ) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_bot_instances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                token TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_channel_bot_instances_platform
                ON channel_bot_instances(platform);",
        )?;

        let has_bi = Self::column_exists(conn, "channel_bindings", "bot_instance_id")?;
        if !has_bi {
            conn.execute_batch(
                "CREATE TABLE channel_bindings_new (
                    bot_instance_id INTEGER NOT NULL,
                    canonical_chat_id INTEGER NOT NULL,
                    channel_type TEXT NOT NULL,
                    channel_handle TEXT NOT NULL,
                    PRIMARY KEY (bot_instance_id, channel_type, channel_handle)
                );
                CREATE INDEX IF NOT EXISTS idx_channel_bindings_new_canonical
                    ON channel_bindings_new(canonical_chat_id);",
            )?;
            conn.execute(
                "INSERT INTO channel_bindings_new (bot_instance_id, canonical_chat_id, channel_type, channel_handle)
                 SELECT
                    CASE channel_type
                        WHEN 'web' THEN 0
                        WHEN 'telegram' THEN 1
                        WHEN 'discord' THEN 2
                        WHEN 'whatsapp' THEN 3
                        ELSE 1
                    END,
                    canonical_chat_id, channel_type, channel_handle
                 FROM channel_bindings",
                [],
            )?;
            conn.execute_batch(
                "DROP TABLE channel_bindings;
                 ALTER TABLE channel_bindings_new RENAME TO channel_bindings;
                 CREATE INDEX IF NOT EXISTS idx_channel_bindings_canonical
                     ON channel_bindings(canonical_chat_id);",
            )?;
        }

        let policy_has_bot =
            Self::column_exists(conn, "channel_persona_policy", "bot_instance_id")?;
        if !policy_has_bot {
            let has_channel_type =
                Self::column_exists(conn, "channel_persona_policy", "channel_type")?;
            if has_channel_type {
                conn.execute_batch(
                    "CREATE TABLE channel_persona_policy_new (
                        canonical_chat_id INTEGER NOT NULL,
                        bot_instance_id INTEGER NOT NULL,
                        mode TEXT NOT NULL DEFAULT 'all',
                        persona_id INTEGER,
                        updated_at TEXT NOT NULL,
                        PRIMARY KEY (canonical_chat_id, bot_instance_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_channel_persona_policy_new_chat
                        ON channel_persona_policy_new(canonical_chat_id);",
                )?;
                conn.execute(
                    "INSERT INTO channel_persona_policy_new (canonical_chat_id, bot_instance_id, mode, persona_id, updated_at)
                     SELECT canonical_chat_id,
                        CASE channel_type
                            WHEN 'telegram' THEN 1
                            WHEN 'discord' THEN 2
                            WHEN 'whatsapp' THEN 3
                            ELSE 0
                        END,
                        mode, persona_id, updated_at
                     FROM channel_persona_policy
                     WHERE channel_type IN ('telegram', 'discord', 'whatsapp')",
                    [],
                )?;
                conn.execute_batch(
                    "DROP TABLE channel_persona_policy;
                     ALTER TABLE channel_persona_policy_new RENAME TO channel_persona_policy;
                     CREATE INDEX IF NOT EXISTS idx_channel_persona_policy_chat
                         ON channel_persona_policy(canonical_chat_id);",
                )?;
            }
        }

        Self::migrate_misplaced_channel_bot_instance_ids(conn)?;

        Ok(())
    }

    /// Reassign extra bot rows that occupy reserved primary ids 2 (Discord) or 3 (WhatsApp).
    fn migrate_misplaced_channel_bot_instance_ids(
        conn: &Connection,
    ) -> Result<(), FinallyAValueBotError> {
        let expected: [(i64, &str); 2] = [
            (BOT_INSTANCE_DISCORD_PRIMARY, "discord"),
            (BOT_INSTANCE_WHATSAPP_PRIMARY, "whatsapp"),
        ];
        for (reserved_id, expected_platform) in expected {
            let platform: Option<String> = conn
                .query_row(
                    "SELECT platform FROM channel_bot_instances WHERE id = ?1",
                    params![reserved_id],
                    |row| row.get(0),
                )
                .ok();
            let Some(actual) = platform else {
                continue;
            };
            if actual == expected_platform {
                continue;
            }
            let new_id = Self::next_extra_bot_instance_id_conn(conn)?;
            Self::reassign_channel_bot_instance_id_conn(conn, reserved_id, new_id)?;
            tracing::info!(
                "Migrated channel_bot_instances id {} ({}) -> id {} (reserved for {})",
                reserved_id,
                actual,
                new_id,
                expected_platform
            );
        }
        Ok(())
    }

    fn next_extra_bot_instance_id_conn(conn: &Connection) -> Result<i64, FinallyAValueBotError> {
        let max_id: i64 = conn.query_row(
            "SELECT COALESCE(MAX(id), 0) FROM channel_bot_instances",
            [],
            |row| row.get(0),
        )?;
        Ok(max_id.max(BOT_INSTANCE_WHATSAPP_PRIMARY) + 1)
    }

    fn reassign_channel_bot_instance_id_conn(
        conn: &Connection,
        old_id: i64,
        new_id: i64,
    ) -> Result<(), FinallyAValueBotError> {
        let row: (String, String, String, String) = conn.query_row(
            "SELECT platform, label, token, created_at FROM channel_bot_instances WHERE id = ?1",
            params![old_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        conn.execute(
            "INSERT INTO channel_bot_instances (id, platform, label, token, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_id, row.0, row.1, row.2, row.3],
        )?;
        conn.execute(
            "UPDATE channel_bindings SET bot_instance_id = ?1 WHERE bot_instance_id = ?2",
            params![new_id, old_id],
        )?;
        conn.execute(
            "UPDATE channel_persona_policy SET bot_instance_id = ?1 WHERE bot_instance_id = ?2",
            params![new_id, old_id],
        )?;
        conn.execute(
            "DELETE FROM channel_bot_instances WHERE id = ?1",
            params![old_id],
        )?;
        Ok(())
    }

    fn migrate_persona_schema(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        // Check if messages has persona_id (new schema)
        let has_persona = conn
            .prepare("PRAGMA table_info(messages)")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
                Ok(rows.filter_map(|r| r.ok()).any(|c| c == "persona_id"))
            })
            .unwrap_or(false);

        if has_persona {
            return Ok(());
        }

        // Add active_persona_id to chats if missing
        let has_active = conn
            .prepare("PRAGMA table_info(chats)")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
                Ok(rows
                    .filter_map(|r| r.ok())
                    .any(|c| c == "active_persona_id"))
            })
            .unwrap_or(false);
        if !has_active {
            conn.execute("ALTER TABLE chats ADD COLUMN active_persona_id INTEGER", [])?;
        }

        // Create personas table if not exists (might not exist in very old DB)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS personas (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                model_override TEXT,
                UNIQUE(chat_id, name)
            );
            CREATE INDEX IF NOT EXISTS idx_personas_chat_id ON personas(chat_id);",
        )?;

        // Collect all chat_ids
        let chat_ids: Vec<i64> = {
            let mut out = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT chat_id FROM chats UNION SELECT chat_id FROM sessions UNION SELECT chat_id FROM messages",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            for r in rows {
                if let Ok(id) = r {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
            out
        };

        // Create default persona for each chat, set active
        let now = chrono::Utc::now().to_rfc3339();
        for cid in &chat_ids {
            conn.execute(
                "INSERT OR IGNORE INTO chats (chat_id, chat_title, chat_type, last_message_time, active_persona_id)
                 VALUES (?1, NULL, 'private', ?2, NULL)",
                params![cid, now],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO personas (chat_id, name, model_override) VALUES (?1, 'default', NULL)",
                params![cid],
            )?;
            let persona_id: i64 = conn.query_row(
                "SELECT id FROM personas WHERE chat_id = ?1 AND name = 'default'",
                params![cid],
                |row| row.get(0),
            )?;
            conn.execute(
                "UPDATE chats SET active_persona_id = ?1 WHERE chat_id = ?2",
                params![persona_id, cid],
            )?;
        }

        // Migrate sessions
        conn.execute_batch(
            "CREATE TABLE sessions_new (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                messages_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id)
            );
            INSERT INTO sessions_new (chat_id, persona_id, messages_json, updated_at)
            SELECT s.chat_id, p.id, s.messages_json, s.updated_at
            FROM sessions s
            JOIN personas p ON p.chat_id = s.chat_id AND p.name = 'default';
            DROP TABLE sessions;
            ALTER TABLE sessions_new RENAME TO sessions;",
        )?;

        // Migrate messages
        conn.execute_batch(
            "CREATE TABLE messages_new (
                id TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                sender_name TEXT NOT NULL,
                content TEXT NOT NULL,
                is_from_bot INTEGER NOT NULL DEFAULT 0,
                timestamp TEXT NOT NULL,
                PRIMARY KEY (id, chat_id, persona_id)
            );
            CREATE INDEX idx_messages_new_chat_ts ON messages_new(chat_id, persona_id, timestamp);
            INSERT INTO messages_new SELECT m.id, m.chat_id, p.id, m.sender_name, m.content, m.is_from_bot, m.timestamp
            FROM messages m
            JOIN personas p ON p.chat_id = m.chat_id AND p.name = 'default';
            DROP TABLE messages;
            ALTER TABLE messages_new RENAME TO messages;
            CREATE INDEX IF NOT EXISTS idx_messages_chat_timestamp ON messages(chat_id, persona_id, timestamp);",
        )?;

        Ok(())
    }

    fn migrate_scheduled_tasks_persona_schema(
        conn: &Connection,
    ) -> Result<(), FinallyAValueBotError> {
        let has_persona_id = conn
            .prepare("PRAGMA table_info(scheduled_tasks)")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
                Ok(rows.filter_map(|r| r.ok()).any(|c| c == "persona_id"))
            })
            .unwrap_or(false);
        if has_persona_id {
            return Ok(());
        }

        conn.execute(
            "ALTER TABLE scheduled_tasks ADD COLUMN persona_id INTEGER",
            [],
        )?;
        conn.execute_batch(
            "UPDATE scheduled_tasks
             SET persona_id = (
                 SELECT active_persona_id
                 FROM chats
                 WHERE chats.chat_id = scheduled_tasks.chat_id
             )
             WHERE persona_id IS NULL;
             UPDATE scheduled_tasks
             SET persona_id = (
                 SELECT p.id
                 FROM personas p
                 WHERE p.chat_id = scheduled_tasks.chat_id
                 ORDER BY CASE WHEN p.name = 'default' THEN 0 ELSE 1 END, p.id
                 LIMIT 1
             )
             WHERE persona_id IS NULL;",
        )?;

        let mut stmt =
            conn.prepare("SELECT DISTINCT chat_id FROM scheduled_tasks WHERE persona_id IS NULL")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        for row in rows {
            let chat_id = row?;
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO chats (chat_id, chat_title, chat_type, last_message_time, active_persona_id)
                 VALUES (?1, NULL, 'private', ?2, NULL)
                 ON CONFLICT(chat_id) DO NOTHING",
                params![chat_id, now],
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO personas (chat_id, name, model_override) VALUES (?1, 'default', NULL)",
                params![chat_id],
            )?;
            let persona_id: i64 = conn.query_row(
                "SELECT id FROM personas WHERE chat_id = ?1 AND name = 'default'",
                params![chat_id],
                |r| r.get(0),
            )?;
            conn.execute(
                "UPDATE chats SET active_persona_id = COALESCE(active_persona_id, ?1) WHERE chat_id = ?2",
                params![persona_id, chat_id],
            )?;
            conn.execute(
                "UPDATE scheduled_tasks SET persona_id = ?1 WHERE chat_id = ?2 AND persona_id IS NULL",
                params![persona_id, chat_id],
            )?;
        }

        // Enforce NOT NULL via table rebuild (SQLite cannot alter nullability in place).
        conn.execute_batch(
            "CREATE TABLE scheduled_tasks_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                prompt TEXT NOT NULL,
                schedule_type TEXT NOT NULL DEFAULT 'cron',
                schedule_value TEXT NOT NULL,
                next_run TEXT NOT NULL,
                last_run TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL
            );
            INSERT INTO scheduled_tasks_new (id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at)
            SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
            FROM scheduled_tasks;
            DROP TABLE scheduled_tasks;
            ALTER TABLE scheduled_tasks_new RENAME TO scheduled_tasks;
            CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_status_next
                ON scheduled_tasks(status, next_run);",
        )?;

        Ok(())
    }

    fn migrate_fts(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        // Create FTS5 virtual table and triggers (after all table migrations)
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content,
                content='messages',
                content_rowid='rowid'
            );
            CREATE TRIGGER IF NOT EXISTS messages_fts_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_fts_bd BEFORE DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS messages_fts_au AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
                INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
            END;",
        )?;

        // One-time migration: populate FTS from existing messages if FTS is empty but messages has data
        let fts_count: i64 =
            conn.query_row("SELECT count(*) FROM messages_fts", [], |r| r.get(0))?;
        let msg_count: i64 = conn.query_row("SELECT count(*) FROM messages", [], |r| r.get(0))?;
        if fts_count == 0 && msg_count > 0 {
            conn.execute(
                "INSERT INTO messages_fts(rowid, content) SELECT rowid, content FROM messages",
                [],
            )?;
        }

        Ok(())
    }

    fn migrate_cursor_agent_runs_tmux(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        let has_tmux = conn
            .prepare("PRAGMA table_info(cursor_agent_runs)")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
                Ok(rows.filter_map(|r| r.ok()).any(|c| c == "tmux_session"))
            })
            .unwrap_or(false);
        if !has_tmux {
            conn.execute(
                "ALTER TABLE cursor_agent_runs ADD COLUMN tmux_session TEXT",
                [],
            )?;
        }
        Ok(())
    }

    /// Removes unused `project_artifacts` (never read by the app; only written).
    fn migrate_drop_project_artifacts(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_project_artifacts_project;
             DROP TABLE IF EXISTS project_artifacts;",
        )?;
        Ok(())
    }

    fn migrate_workflow_learning_schema(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        if !Self::column_exists(conn, "workflows", "step_trace_json")? {
            conn.execute(
                "ALTER TABLE workflows ADD COLUMN step_trace_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        if !Self::column_exists(conn, "workflows", "approach_summary")? {
            conn.execute(
                "ALTER TABLE workflows ADD COLUMN approach_summary TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        if !Self::column_exists(conn, "workflows", "last_outcome")? {
            conn.execute(
                "ALTER TABLE workflows ADD COLUMN last_outcome TEXT NOT NULL DEFAULT 'unknown'",
                [],
            )?;
        }
        if !Self::column_exists(conn, "workflows", "failure_reason")? {
            conn.execute("ALTER TABLE workflows ADD COLUMN failure_reason TEXT", [])?;
        }
        if !Self::column_exists(conn, "workflows", "evidence_json")? {
            conn.execute(
                "ALTER TABLE workflows ADD COLUMN evidence_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        Ok(())
    }

    fn migrate_personas_prompt_context(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        if !Self::column_exists(conn, "personas", "recent_history_min_user")? {
            conn.execute(
                "ALTER TABLE personas ADD COLUMN recent_history_min_user INTEGER",
                [],
            )?;
        }
        if !Self::column_exists(conn, "personas", "recent_history_min_assistant")? {
            conn.execute(
                "ALTER TABLE personas ADD COLUMN recent_history_min_assistant INTEGER",
                [],
            )?;
        }
        if !Self::column_exists(conn, "personas", "operator_memo")? {
            conn.execute("ALTER TABLE personas ADD COLUMN operator_memo TEXT", [])?;
        }
        Ok(())
    }

    fn migrate_hook_policy_schema(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS hook_definitions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                event_name TEXT NOT NULL,
                matcher TEXT,
                action_type TEXT NOT NULL,
                action_payload_json TEXT NOT NULL DEFAULT '{}',
                scoped_persona_ids_json TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_hook_definitions_event_enabled
                ON hook_definitions(event_name, enabled, id);
            CREATE TABLE IF NOT EXISTS persona_hook_skill_policy (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                allowed_hook_ids_json TEXT,
                allowed_skill_names_json TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id)
            );
            CREATE INDEX IF NOT EXISTS idx_persona_hook_skill_policy_chat
                ON persona_hook_skill_policy(chat_id);",
        )?;
        if !Self::column_exists(conn, "hook_definitions", "scoped_persona_ids_json")? {
            conn.execute(
                "ALTER TABLE hook_definitions ADD COLUMN scoped_persona_ids_json TEXT",
                [],
            )?;
        }
        Ok(())
    }

    /// Sync shipped hooks from repository `builtin_hooks/*.hook.json` into SQLite.
    ///
    /// PZ and other optional command hooks are not shipped; operators add those under
    /// `{WORKSPACE_DIR}/hooks/`.
    fn ensure_builtin_hook_definitions(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        conn.execute(
            "DELETE FROM hook_definitions WHERE name LIKE 'template-%'",
            [],
        )?;
        let dir = crate::builtin_hooks::resolve_builtin_hooks_dir_fallback().ok_or_else(|| {
            FinallyAValueBotError::ToolExecution(
                "builtin_hooks catalog not found: expected repository builtin_hooks/ with *.hook.json manifests".into(),
            )
        })?;
        let synced = crate::builtin_hooks::sync_shipped_hook_definitions(conn, &dir)?;
        if synced == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "no shipped hook manifests in '{}'",
                dir.display()
            )));
        }
        Ok(())
    }

    fn migrate_chat_sessions_schema(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chat_sessions (
                id TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                intent TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL,
                last_active_at TEXT NOT NULL,
                archived_at TEXT,
                ttl_hours INTEGER NOT NULL DEFAULT 72,
                bootstrap_context_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_persona
                ON chat_sessions(chat_id, persona_id, status);
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_last_active
                ON chat_sessions(last_active_at DESC);",
        )?;
        if !Self::column_exists(conn, "messages", "session_id")? {
            conn.execute("ALTER TABLE messages ADD COLUMN session_id TEXT", [])?;
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_messages_session
                    ON messages(session_id, timestamp ASC);",
            )?;
        }
        if !Self::column_exists(conn, "chat_sessions", "mirror_main_chat")? {
            conn.execute(
                "ALTER TABLE chat_sessions ADD COLUMN mirror_main_chat INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    fn migrate_messages_origin(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        if !Self::column_exists(conn, "messages", "origin")? {
            conn.execute(
                "ALTER TABLE messages ADD COLUMN origin TEXT NOT NULL DEFAULT 'interactive'",
                [],
            )?;
        }
        Ok(())
    }

    fn migrate_cursor_engine_agents(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cursor_engine_agents (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                session_scope TEXT NOT NULL DEFAULT '',
                agent_id TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id, session_scope)
            );
            CREATE INDEX IF NOT EXISTS idx_cursor_engine_agents_updated
                ON cursor_engine_agents(updated_at DESC);",
        )?;
        Ok(())
    }

    fn migrate_background_jobs_lease_schema(
        conn: &Connection,
    ) -> Result<(), FinallyAValueBotError> {
        if !Self::column_exists(conn, "background_jobs", "lease_owner")? {
            conn.execute(
                "ALTER TABLE background_jobs ADD COLUMN lease_owner TEXT",
                [],
            )?;
        }
        if !Self::column_exists(conn, "background_jobs", "lease_expires_at")? {
            conn.execute(
                "ALTER TABLE background_jobs ADD COLUMN lease_expires_at TEXT",
                [],
            )?;
        }
        if !Self::column_exists(conn, "background_jobs", "last_progress_at")? {
            conn.execute(
                "ALTER TABLE background_jobs ADD COLUMN last_progress_at TEXT",
                [],
            )?;
        }
        if !Self::column_exists(conn, "background_jobs", "last_stage")? {
            conn.execute("ALTER TABLE background_jobs ADD COLUMN last_stage TEXT", [])?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_background_jobs_lease_expires_at
             ON background_jobs(lease_expires_at)",
            [],
        )?;
        Ok(())
    }

    fn migrate_background_jobs_shell_schema(
        conn: &Connection,
    ) -> Result<(), FinallyAValueBotError> {
        if !Self::column_exists(conn, "background_jobs", "job_kind")? {
            conn.execute(
                "ALTER TABLE background_jobs ADD COLUMN job_kind TEXT NOT NULL DEFAULT 'agent'",
                [],
            )?;
        }
        for col in [
            "shell_command",
            "workdir",
            "tmux_session",
            "output_path",
            "label",
        ] {
            if !Self::column_exists(conn, "background_jobs", col)? {
                conn.execute(
                    &format!("ALTER TABLE background_jobs ADD COLUMN {col} TEXT"),
                    [],
                )?;
            }
        }
        if !Self::column_exists(conn, "background_jobs", "exit_code")? {
            conn.execute(
                "ALTER TABLE background_jobs ADD COLUMN exit_code INTEGER",
                [],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_background_jobs_job_kind_status
             ON background_jobs(job_kind, status)",
            [],
        )?;
        Ok(())
    }

    fn migrate_persona_bulletin_and_bookmarks(
        conn: &Connection,
    ) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS persona_bulletin_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                run_key TEXT,
                event_type TEXT NOT NULL,
                title TEXT NOT NULL,
                detail TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_persona_bulletin_events_persona_time
                ON persona_bulletin_events(chat_id, persona_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS persona_bulletin_focus (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                title TEXT,
                content TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id)
            );

            CREATE TABLE IF NOT EXISTS persona_message_bookmarks (
                chat_id INTEGER NOT NULL,
                persona_id INTEGER NOT NULL,
                message_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content_preview TEXT NOT NULL,
                note TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, persona_id, message_id)
            );
            CREATE INDEX IF NOT EXISTS idx_persona_message_bookmarks_persona_time
                ON persona_message_bookmarks(chat_id, persona_id, updated_at DESC);",
        )?;
        Ok(())
    }

    fn migrate_channel_bindings(conn: &Connection) -> Result<(), FinallyAValueBotError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_bindings (
                canonical_chat_id INTEGER NOT NULL,
                channel_type TEXT NOT NULL,
                channel_handle TEXT NOT NULL,
                PRIMARY KEY (channel_type, channel_handle)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_bindings_canonical
                ON channel_bindings(canonical_chat_id);",
        )?;
        // Backfill: each existing chat gets one binding (canonical = chat_id)
        let mut stmt = conn.prepare("SELECT chat_id, chat_type, chat_title FROM chats")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            let (chat_id, chat_type, chat_title) = row?;
            let (ch_type, handle) = match chat_type.as_str() {
                "telegram" => ("telegram", chat_id.to_string()),
                "discord" => ("discord", chat_id.to_string()),
                "web" => ("web", chat_title.unwrap_or_else(|| chat_id.to_string())),
                _ => continue,
            };
            conn.execute(
                "INSERT OR IGNORE INTO channel_bindings (canonical_chat_id, channel_type, channel_handle) VALUES (?1, ?2, ?3)",
                params![chat_id, ch_type, handle],
            )?;
        }
        Ok(())
    }

    pub fn upsert_chat(
        &self,
        chat_id: i64,
        chat_title: Option<&str>,
        chat_type: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO chats (chat_id, chat_title, chat_type, last_message_time)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat_id) DO UPDATE SET
                chat_title = COALESCE(?2, chat_title),
                chat_type = ?3,
                last_message_time = ?4",
            params![chat_id, chat_title, chat_type, now],
        )?;
        Ok(())
    }

    pub fn store_message(&self, msg: &StoredMessage) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let origin = if msg.origin.trim().is_empty() {
            MESSAGE_ORIGIN_INTERACTIVE
        } else {
            msg.origin.as_str()
        };
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, chat_id, persona_id, session_id, sender_name, content, is_from_bot, timestamp, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                msg.id,
                msg.chat_id,
                msg.persona_id,
                msg.session_id,
                msg.sender_name,
                msg.content,
                msg.is_from_bot as i32,
                msg.timestamp,
                origin,
            ],
        )?;
        Ok(())
    }

    pub fn delete_message(
        &self,
        chat_id: i64,
        persona_id: i64,
        message_id: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM persona_message_bookmarks
             WHERE chat_id = ?1 AND persona_id = ?2 AND message_id = ?3",
            params![chat_id, persona_id, message_id],
        )?;
        let rows = conn.execute(
            "DELETE FROM messages WHERE chat_id = ?1 AND persona_id = ?2 AND id = ?3",
            params![chat_id, persona_id, message_id],
        )?;
        Ok(rows > 0)
    }

    /// True when the **latest** row for this chat is a bot message with the same body as `content`
    /// and a recent timestamp. That usually means `send_message` already posted this text and the
    /// main agent is about to deliver the same final reply again.
    pub fn should_skip_duplicate_final_delivery(
        &self,
        chat_id: i64,
        content: &str,
        max_age_secs: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        use rusqlite::OptionalExtension;

        let conn = self.conn.lock().unwrap();
        let last: Option<(bool, String, String)> = conn
            .query_row(
                "SELECT is_from_bot, content, timestamp FROM messages
                 WHERE chat_id = ?1
                 ORDER BY timestamp DESC
                 LIMIT 1",
                params![chat_id],
                |row| {
                    Ok((
                        row.get::<_, i32>(0)? != 0,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((is_bot, last_content, ts)) = last else {
            return Ok(false);
        };
        if !is_bot || last_content != content {
            return Ok(false);
        }
        let Ok(parsed) = DateTime::parse_from_rfc3339(&ts) else {
            return Ok(false);
        };
        let parsed = parsed.with_timezone(&Utc);
        let age = Utc::now().signed_duration_since(parsed);
        Ok(age.num_seconds() >= 0 && age.num_seconds() <= max_age_secs)
    }

    pub fn get_recent_messages(
        &self,
        chat_id: i64,
        persona_id: i64,
        limit: usize,
        exclude_scheduled: bool,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut messages = if exclude_scheduled {
            let mut stmt = conn.prepare(&format!(
                "SELECT {MESSAGE_SELECT_COLS}
                 FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2 AND origin != ?4
                   AND {MAIN_CHAT_MESSAGE_VISIBILITY}
                 ORDER BY timestamp DESC
                 LIMIT ?3"
            ))?;
            let rows = stmt.query_map(
                params![chat_id, persona_id, limit as i64, MESSAGE_ORIGIN_SCHEDULED],
                stored_message_from_row,
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(&format!(
                "SELECT {MESSAGE_SELECT_COLS}
                 FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2
                   AND {MAIN_CHAT_MESSAGE_VISIBILITY}
                 ORDER BY timestamp DESC
                 LIMIT ?3"
            ))?;
            let rows = stmt.query_map(
                params![chat_id, persona_id, limit as i64],
                stored_message_from_row,
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // Reverse so oldest first
        messages.reverse();
        Ok(messages)
    }

    pub fn get_all_messages(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_SELECT_COLS}
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
               AND {MAIN_CHAT_MESSAGE_VISIBILITY}
             ORDER BY timestamp ASC"
        ))?;
        let messages = stmt
            .query_map(params![chat_id, persona_id], stored_message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn get_messages_for_date_range(
        &self,
        chat_id: i64,
        persona_id: i64,
        from_date: Option<&str>,
        to_date: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_SELECT_COLS}
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
               AND {MAIN_CHAT_MESSAGE_VISIBILITY}
               AND (?3 IS NULL OR timestamp >= ?3)
               AND (?4 IS NULL OR timestamp <= ?4)
             ORDER BY timestamp ASC
             LIMIT ?5"
        ))?;
        let messages = stmt
            .query_map(
                params![chat_id, persona_id, from_date, to_date, limit as i64],
                stored_message_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn get_message_days(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Vec<String>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT date(timestamp) AS d FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
               AND {MAIN_CHAT_MESSAGE_VISIBILITY}
             ORDER BY d DESC",
        ))?;
        let days: Vec<String> = stmt
            .query_map(params![chat_id, persona_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(days)
    }

    pub fn get_chats_by_type(
        &self,
        chat_type: &str,
        limit: usize,
    ) -> Result<Vec<ChatSummary>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
                c.chat_id,
                c.chat_title,
                c.chat_type,
                c.last_message_time,
                (
                    SELECT m.content
                    FROM messages m
                    WHERE m.chat_id = c.chat_id
                    ORDER BY m.timestamp DESC
                    LIMIT 1
                ) AS last_message_preview
             FROM chats c
             WHERE c.chat_type = ?1
             ORDER BY c.last_message_time DESC
             LIMIT ?2",
        )?;
        let chats = stmt
            .query_map(params![chat_type, limit as i64], |row| {
                Ok(ChatSummary {
                    chat_id: row.get(0)?,
                    chat_title: row.get(1)?,
                    chat_type: row.get(2)?,
                    last_message_time: row.get(3)?,
                    last_message_preview: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(chats)
    }

    pub fn get_recent_chats(
        &self,
        limit: usize,
    ) -> Result<Vec<ChatSummary>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
                c.chat_id,
                c.chat_title,
                c.chat_type,
                c.last_message_time,
                (
                    SELECT m.content
                    FROM messages m
                    WHERE m.chat_id = c.chat_id
                    ORDER BY m.timestamp DESC
                    LIMIT 1
                ) AS last_message_preview
             FROM chats c
             ORDER BY c.last_message_time DESC
             LIMIT ?1",
        )?;
        let chats = stmt
            .query_map(params![limit as i64], |row| {
                Ok(ChatSummary {
                    chat_id: row.get(0)?,
                    chat_title: row.get(1)?,
                    chat_type: row.get(2)?,
                    last_message_time: row.get(3)?,
                    last_message_preview: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(chats)
    }

    pub fn get_chat_type(&self, chat_id: i64) -> Result<Option<String>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT chat_type FROM chats WHERE chat_id = ?1",
            params![chat_id],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // --- Channel bindings (unified contact) ---

    /// Resolve (`bot_instance_id`, `channel_type`, `channel_handle`) to canonical_chat_id. If no binding exists, creates one:
    /// - telegram/discord: use handle (as i64) as canonical_chat_id, ensure chat exists, insert binding.
    /// - web: use create_with_canonical_id as the new canonical (caller provides e.g. hash-based id), ensure chat exists, insert binding.
    pub fn resolve_canonical_chat_id(
        &self,
        bot_instance_id: i64,
        channel_type: &str,
        channel_handle: &str,
        create_with_canonical_id: Option<i64>,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        if let Some(canonical) = conn
            .query_row(
                "SELECT canonical_chat_id FROM channel_bindings WHERE bot_instance_id = ?1 AND channel_type = ?2 AND channel_handle = ?3",
                params![bot_instance_id, channel_type, channel_handle],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        {
            return Ok(canonical);
        }
        let canonical = match channel_type {
            "telegram" | "discord" => channel_handle.parse::<i64>().map_err(|_| {
                FinallyAValueBotError::ToolExecution(format!(
                    "invalid handle for {}: {}",
                    channel_type, channel_handle
                ))
            })?,
            "web" => create_with_canonical_id.ok_or_else(|| {
                FinallyAValueBotError::ToolExecution(
                    "web resolve requires create_with_canonical_id".into(),
                )
            })?,
            _ => {
                return Err(FinallyAValueBotError::ToolExecution(format!(
                    "unknown channel_type: {}",
                    channel_type
                )))
            }
        };
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR IGNORE INTO chats (chat_id, chat_title, chat_type, last_message_time) VALUES (?1, NULL, ?2, ?3)",
            params![canonical, channel_type, now],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO channel_bindings (bot_instance_id, canonical_chat_id, channel_type, channel_handle) VALUES (?1, ?2, ?3, ?4)",
            params![bot_instance_id, canonical, channel_type, channel_handle],
        )?;
        Ok(canonical)
    }

    /// Add a binding from (`bot_instance_id`, `channel_type`, `channel_handle`) to canonical_chat_id.
    pub fn link_channel(
        &self,
        canonical_chat_id: i64,
        bot_instance_id: i64,
        channel_type: &str,
        channel_handle: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO channel_bindings (bot_instance_id, canonical_chat_id, channel_type, channel_handle) VALUES (?1, ?2, ?3, ?4)",
            params![bot_instance_id, canonical_chat_id, channel_type, channel_handle],
        )?;
        Ok(())
    }

    /// Remove the binding for (`bot_instance_id`, `channel_type`, `channel_handle`).
    pub fn unlink_channel(
        &self,
        bot_instance_id: i64,
        channel_type: &str,
        channel_handle: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM channel_bindings WHERE bot_instance_id = ?1 AND channel_type = ?2 AND channel_handle = ?3",
            params![bot_instance_id, channel_type, channel_handle],
        )?;
        Ok(rows > 0)
    }

    /// List all channel bindings for this contact (canonical_chat_id).
    pub fn list_bindings_for_contact(
        &self,
        canonical_chat_id: i64,
    ) -> Result<Vec<ChannelBinding>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT canonical_chat_id, bot_instance_id, channel_type, channel_handle FROM channel_bindings WHERE canonical_chat_id = ?1",
        )?;
        let rows = stmt.query_map(params![canonical_chat_id], |row| {
            Ok(ChannelBinding {
                canonical_chat_id: row.get(0)?,
                bot_instance_id: row.get(1)?,
                channel_type: row.get(2)?,
                channel_handle: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Upsert primary bot rows from `.env` so Telegram/Discord/WhatsApp dispatchers can list tokens.
    pub fn sync_channel_bot_instances_from_config(
        &self,
        config: &crate::config::Config,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        if !config.telegram_bot_token.trim().is_empty() {
            conn.execute(
                "INSERT INTO channel_bot_instances (id, platform, label, token, created_at)
                 VALUES (1, 'telegram', 'Primary (TELEGRAM_BOT_TOKEN)', ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET
                    platform=excluded.platform,
                    label=excluded.label,
                    token=excluded.token,
                    created_at=excluded.created_at",
                params![config.telegram_bot_token.trim(), now],
            )?;
        }
        if let Some(ref t) = config.discord_bot_token {
            if !t.trim().is_empty() {
                conn.execute(
                    "INSERT INTO channel_bot_instances (id, platform, label, token, created_at)
                     VALUES (2, 'discord', 'Primary (DISCORD_BOT_TOKEN)', ?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET
                        platform=excluded.platform,
                        label=excluded.label,
                        token=excluded.token,
                        created_at=excluded.created_at",
                    params![t.trim(), now],
                )?;
            }
        }
        let wa = config.whatsapp_access_token.as_deref().unwrap_or("").trim();
        if !wa.is_empty() {
            conn.execute(
                "INSERT INTO channel_bot_instances (id, platform, label, token, created_at)
                 VALUES (3, 'whatsapp', 'Primary (WHATSAPP_ACCESS_TOKEN)', ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET
                    platform=excluded.platform,
                    label=excluded.label,
                    token=excluded.token,
                    created_at=excluded.created_at",
                params![wa, now],
            )?;
        }
        Ok(())
    }

    pub fn list_channel_bot_instances_by_platform(
        &self,
        platform: &str,
    ) -> Result<Vec<ChannelBotInstance>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, platform, label, token, created_at FROM channel_bot_instances
             WHERE platform = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![platform], |row| {
            Ok(ChannelBotInstance {
                id: row.get(0)?,
                platform: row.get(1)?,
                label: row.get(2)?,
                token: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_channel_bot_instance(
        &self,
        id: i64,
    ) -> Result<Option<ChannelBotInstance>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT id, platform, label, token, created_at FROM channel_bot_instances WHERE id = ?1",
            params![id],
            |row| {
                Ok(ChannelBotInstance {
                    id: row.get(0)?,
                    platform: row.get(1)?,
                    label: row.get(2)?,
                    token: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        );
        match row {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// All bot instances (Telegram, Discord, WhatsApp, etc.), ordered by id.
    pub fn list_all_channel_bot_instances(
        &self,
    ) -> Result<Vec<ChannelBotInstance>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, platform, label, token, created_at FROM channel_bot_instances ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ChannelBotInstance {
                id: row.get(0)?,
                platform: row.get(1)?,
                label: row.get(2)?,
                token: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Insert a non-primary bot instance. Ids 1–3 are reserved for env sync; extras use id >= 4.
    pub fn create_channel_bot_instance(
        &self,
        platform: &str,
        label: &str,
        token: &str,
    ) -> Result<i64, FinallyAValueBotError> {
        let p = platform.trim().to_ascii_lowercase();
        if !matches!(p.as_str(), "telegram" | "discord") {
            return Err(FinallyAValueBotError::ToolExecution(
                "platform must be telegram or discord".into(),
            ));
        }
        if token.trim().is_empty() {
            return Err(FinallyAValueBotError::ToolExecution(
                "token cannot be empty".into(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let new_id = Self::next_extra_bot_instance_id_conn(&conn)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO channel_bot_instances (id, platform, label, token, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_id, p, label.trim(), token.trim(), now],
        )?;
        Ok(new_id)
    }

    pub fn get_channel_bot_instance_platform(
        &self,
        bot_instance_id: i64,
    ) -> Result<Option<String>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT platform FROM channel_bot_instances WHERE id = ?1",
            params![bot_instance_id],
            |r| r.get::<_, String>(0),
        );
        match row {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Copy channel handles from sibling bindings on the same platform to `new_bot_instance_id`.
    pub fn provision_bindings_for_instance(
        &self,
        platform: &str,
        new_bot_instance_id: i64,
    ) -> Result<u32, FinallyAValueBotError> {
        let platform = platform.trim().to_ascii_lowercase();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT canonical_chat_id, channel_handle
             FROM channel_bindings
             WHERE channel_type = ?1 AND bot_instance_id != ?2",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map(params![platform, new_bot_instance_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut linked = 0u32;
        for (canonical_chat_id, handle) in rows {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM channel_bindings
                     WHERE bot_instance_id = ?1 AND channel_type = ?2 AND channel_handle = ?3",
                    params![new_bot_instance_id, platform, handle],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if exists {
                continue;
            }
            conn.execute(
                "INSERT OR REPLACE INTO channel_bindings (bot_instance_id, canonical_chat_id, channel_type, channel_handle)
                 VALUES (?1, ?2, ?3, ?4)",
                params![new_bot_instance_id, canonical_chat_id, platform, handle],
            )?;
            linked += 1;
        }
        Ok(linked)
    }

    /// Idempotent: ensure every bot instance has sibling bindings for contacts already linked on that platform.
    pub fn provision_all_missing_sibling_bindings(&self) -> Result<u32, FinallyAValueBotError> {
        let instances = self.list_all_channel_bot_instances()?;
        let mut total = 0u32;
        for inst in instances {
            if !matches!(inst.platform.as_str(), "telegram" | "discord") {
                continue;
            }
            total += self.provision_bindings_for_instance(&inst.platform, inst.id)?;
        }
        Ok(total)
    }

    /// Rows for Settings → Channels: every telegram/discord instance plus binding/policy state for a contact.
    pub fn list_contact_channel_integration_rows(
        &self,
        canonical_chat_id: i64,
    ) -> Result<Vec<ContactChannelIntegrationRow>, FinallyAValueBotError> {
        let bindings = self.list_bindings_for_contact(canonical_chat_id)?;
        let policies = self.list_channel_persona_policies(canonical_chat_id)?;
        let mut policy_by_instance: HashMap<i64, ChannelPersonaPolicy> = HashMap::new();
        for p in policies {
            policy_by_instance.insert(p.bot_instance_id, p);
        }
        let mut binding_by_instance: HashMap<i64, ChannelBinding> = HashMap::new();
        for b in bindings {
            if b.channel_type == "web" {
                continue;
            }
            binding_by_instance.insert(b.bot_instance_id, b);
        }
        let mut rows = Vec::new();
        for platform in ["telegram", "discord"] {
            for inst in self.list_channel_bot_instances_by_platform(platform)? {
                let binding = binding_by_instance.get(&inst.id);
                let (persona_mode, persona_id) = policy_by_instance
                    .get(&inst.id)
                    .map(|p| (p.mode, p.persona_id))
                    .unwrap_or((ChannelPersonaMode::All, None));
                rows.push(ContactChannelIntegrationRow {
                    bot_instance_id: inst.id,
                    platform: inst.platform.clone(),
                    label: inst.label.clone(),
                    channel_handle: binding.map(|b| b.channel_handle.clone()),
                    linked: binding.is_some(),
                    persona_mode,
                    persona_id,
                });
            }
        }
        Ok(rows)
    }

    fn another_all_persona_bot_on_platform(
        &self,
        canonical_chat_id: i64,
        platform: &str,
        exclude_bot_instance_id: i64,
    ) -> Result<Option<i64>, FinallyAValueBotError> {
        let instances = self.list_channel_bot_instances_by_platform(platform)?;
        for inst in instances {
            if inst.id == exclude_bot_instance_id {
                continue;
            }
            let effective_all = match self.get_channel_persona_policy(canonical_chat_id, inst.id)? {
                None => true,
                Some(p) => p.mode == ChannelPersonaMode::All,
            };
            if effective_all {
                return Ok(Some(inst.id));
            }
        }
        Ok(None)
    }

    fn validate_all_persona_slot(
        &self,
        canonical_chat_id: i64,
        bot_instance_id: i64,
    ) -> Result<(), FinallyAValueBotError> {
        let platform = self
            .get_channel_bot_instance_platform(bot_instance_id)?
            .ok_or_else(|| {
                FinallyAValueBotError::ToolExecution(format!(
                    "unknown bot_instance_id {bot_instance_id}"
                ))
            })?;
        if let Some(other) =
            self.another_all_persona_bot_on_platform(canonical_chat_id, &platform, bot_instance_id)?
        {
            let label = self
                .get_channel_bot_instance(other)?
                .map(|i| i.label)
                .unwrap_or_else(|| format!("#{other}"));
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "Only one {platform} bot can use 'all personas' for this contact (already set on {label}). Lock the other bot to a single persona first."
            )));
        }
        Ok(())
    }

    pub fn update_channel_bot_instance(
        &self,
        id: i64,
        label: &str,
        token: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        if token.trim().is_empty() {
            return Err(FinallyAValueBotError::ToolExecution(
                "token cannot be empty".into(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE channel_bot_instances SET label = ?1, token = ?2 WHERE id = ?3",
            params![label.trim(), token.trim(), id],
        )?;
        Ok(rows > 0)
    }

    /// Deletes a bot instance. Ids 1–3 are reserved for primary env-backed rows and cannot be deleted here.
    pub fn delete_channel_bot_instance(&self, id: i64) -> Result<bool, FinallyAValueBotError> {
        if (1..=3).contains(&id) {
            return Err(FinallyAValueBotError::ToolExecution(
                "Cannot delete primary bot instances (ids 1–3) managed from .env".into(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM channel_bot_instances WHERE id = ?1",
            params![id],
        )?;
        Ok(rows > 0)
    }

    pub fn list_app_settings(&self) -> Result<Vec<AppSetting>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT key, value, updated_at FROM app_settings ORDER BY key COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AppSetting {
                key: row.get(0)?,
                value: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn set_app_setting(&self, key: &str, value: &str) -> Result<(), FinallyAValueBotError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(FinallyAValueBotError::ToolExecution(
                "App setting key cannot be empty".to_string(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, now],
        )?;
        Ok(())
    }

    pub fn remove_app_setting(&self, key: &str) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
        Ok(rows > 0)
    }

    fn parse_i64_array_json(
        raw: Option<String>,
    ) -> Result<Option<Vec<i64>>, FinallyAValueBotError> {
        let Some(raw) = raw else {
            return Ok(None);
        };
        let parsed: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|e| {
            FinallyAValueBotError::ToolExecution(format!("Invalid allowed_hook_ids_json: {e}"))
        })?;
        let arr = parsed.as_array().ok_or_else(|| {
            FinallyAValueBotError::ToolExecution(
                "Invalid allowed_hook_ids_json: expected JSON array".to_string(),
            )
        })?;
        let mut out = Vec::with_capacity(arr.len());
        for v in arr {
            let Some(id) = v.as_i64() else {
                return Err(FinallyAValueBotError::ToolExecution(
                    "Invalid allowed_hook_ids_json: array must contain integers".to_string(),
                ));
            };
            out.push(id);
        }
        Ok(Some(out))
    }

    fn parse_string_array_json(
        raw: Option<String>,
    ) -> Result<Option<Vec<String>>, FinallyAValueBotError> {
        let Some(raw) = raw else {
            return Ok(None);
        };
        let parsed: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|e| {
            FinallyAValueBotError::ToolExecution(format!("Invalid allowed_skill_names_json: {e}"))
        })?;
        let arr = parsed.as_array().ok_or_else(|| {
            FinallyAValueBotError::ToolExecution(
                "Invalid allowed_skill_names_json: expected JSON array".to_string(),
            )
        })?;
        let mut out = Vec::with_capacity(arr.len());
        for v in arr {
            let Some(name) = v.as_str() else {
                return Err(FinallyAValueBotError::ToolExecution(
                    "Invalid allowed_skill_names_json: array must contain strings".to_string(),
                ));
            };
            out.push(name.trim().to_string());
        }
        Ok(Some(out))
    }

    pub fn list_hook_definitions(
        &self,
    ) -> Result<Vec<HookDefinitionRecord>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, event_name, matcher, action_type, action_payload_json, scoped_persona_ids_json, enabled, updated_at
             FROM hook_definitions
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let scoped_persona_ids_raw: Option<String> = row.get(6)?;
            let scoped_persona_ids =
                Self::parse_i64_array_json(scoped_persona_ids_raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            Ok(HookDefinitionRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                event_name: row.get(2)?,
                matcher: row.get(3)?,
                action_type: row.get(4)?,
                action_payload_json: row.get(5)?,
                scoped_persona_ids,
                enabled: row.get::<_, i64>(7)? != 0,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_hook_definition(
        &self,
        id: i64,
    ) -> Result<Option<HookDefinitionRecord>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, event_name, matcher, action_type, action_payload_json, scoped_persona_ids_json, enabled, updated_at
             FROM hook_definitions
             WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![id], |row| {
            let scoped_persona_ids_raw: Option<String> = row.get(6)?;
            let scoped_persona_ids =
                Self::parse_i64_array_json(scoped_persona_ids_raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            Ok(HookDefinitionRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                event_name: row.get(2)?,
                matcher: row.get(3)?,
                action_type: row.get(4)?,
                action_payload_json: row.get(5)?,
                scoped_persona_ids,
                enabled: row.get::<_, i64>(7)? != 0,
                updated_at: row.get(8)?,
            })
        });
        match row {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn upsert_hook_definition(
        &self,
        id: Option<i64>,
        name: &str,
        event_name: &str,
        matcher: Option<&str>,
        action_type: &str,
        action_payload_json: &str,
        scoped_persona_ids: Option<&[i64]>,
        enabled: bool,
    ) -> Result<i64, FinallyAValueBotError> {
        let name = name.trim();
        let event_name = event_name.trim();
        let action_type = action_type.trim().to_ascii_lowercase();
        if name.is_empty() || event_name.is_empty() || action_type.is_empty() {
            return Err(FinallyAValueBotError::ToolExecution(
                "name, event_name, and action_type are required".to_string(),
            ));
        }
        let valid_action_type = matches!(
            action_type.as_str(),
            "block"
                | "add_context"
                | "command"
                | "prompt"
                | "builtin_persona_focus_sync"
                | "builtin_scheduler_policy_context"
                | "builtin_turn_skill_gate"
                | "builtin_deferred_commitment_guard"
                | "builtin_loop_guard"
        );
        if !valid_action_type {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "Unsupported action_type '{}'. Expected one of: block, add_context, command, prompt, builtin_persona_focus_sync, builtin_scheduler_policy_context, builtin_turn_skill_gate, builtin_deferred_commitment_guard, builtin_loop_guard",
                action_type
            )));
        }
        // Validate payload JSON eagerly.
        let _: serde_json::Value = serde_json::from_str(action_payload_json).map_err(|e| {
            FinallyAValueBotError::ToolExecution(format!("Invalid action_payload_json: {e}"))
        })?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let enabled_i = if enabled { 1 } else { 0 };
        let matcher_norm = matcher.map(|m| m.trim()).filter(|m| !m.is_empty());
        let scoped_persona_ids_json = if let Some(ids) = scoped_persona_ids {
            let mut normalized: Vec<i64> = ids.iter().copied().filter(|id| *id > 0).collect();
            normalized.sort_unstable();
            normalized.dedup();
            Some(serde_json::to_string(&normalized).map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "Invalid scoped_persona_ids payload: {e}"
                ))
            })?)
        } else {
            None
        };
        if let Some(id) = id {
            let rows = conn.execute(
                "UPDATE hook_definitions
                 SET name = ?1,
                     event_name = ?2,
                     matcher = ?3,
                     action_type = ?4,
                     action_payload_json = ?5,
                     scoped_persona_ids_json = ?6,
                     enabled = ?7,
                     updated_at = ?8
                 WHERE id = ?9",
                params![
                    name,
                    event_name,
                    matcher_norm,
                    action_type.as_str(),
                    action_payload_json,
                    scoped_persona_ids_json,
                    enabled_i,
                    now,
                    id
                ],
            )?;
            if rows == 0 {
                return Err(FinallyAValueBotError::ToolExecution(format!(
                    "Hook definition {id} not found"
                )));
            }
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO hook_definitions
                 (name, event_name, matcher, action_type, action_payload_json, scoped_persona_ids_json, enabled, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    name,
                    event_name,
                    matcher_norm,
                    action_type.as_str(),
                    action_payload_json,
                    scoped_persona_ids_json,
                    enabled_i,
                    now
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }

    pub fn delete_hook_definition(&self, id: i64) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM hook_definitions WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    pub fn get_persona_hook_skill_policy(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Option<PersonaHookSkillPolicy>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chat_id, persona_id, allowed_hook_ids_json, allowed_skill_names_json, updated_at
             FROM persona_hook_skill_policy
             WHERE chat_id = ?1 AND persona_id = ?2",
        )?;
        let row = stmt.query_row(params![chat_id, persona_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        });
        let (chat_id, persona_id, hooks_raw, skills_raw, updated_at) = match row {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let allowed_hook_ids = Self::parse_i64_array_json(hooks_raw)?;
        let allowed_skill_names = Self::parse_string_array_json(skills_raw)?;
        Ok(Some(PersonaHookSkillPolicy {
            chat_id,
            persona_id,
            allowed_hook_ids,
            allowed_skill_names,
            updated_at,
        }))
    }

    pub fn set_persona_hook_skill_policy(
        &self,
        chat_id: i64,
        persona_id: i64,
        allowed_hook_ids: Option<&[i64]>,
        allowed_skill_names: Option<&[String]>,
    ) -> Result<(), FinallyAValueBotError> {
        if !self.persona_exists(chat_id, persona_id)? {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "persona_id {persona_id} does not exist for chat {chat_id}"
            )));
        }
        let hooks_json = allowed_hook_ids
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "Failed to serialize allowed_hook_ids: {e}"
                ))
            })?;
        let skills_json = allowed_skill_names
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "Failed to serialize allowed_skill_names: {e}"
                ))
            })?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO persona_hook_skill_policy
             (chat_id, persona_id, allowed_hook_ids_json, allowed_skill_names_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(chat_id, persona_id) DO UPDATE SET
               allowed_hook_ids_json = excluded.allowed_hook_ids_json,
               allowed_skill_names_json = excluded.allowed_skill_names_json,
               updated_at = excluded.updated_at",
            params![chat_id, persona_id, hooks_json, skills_json, now],
        )?;
        Ok(())
    }

    pub fn is_skill_allowed_for_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
        skill_name: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let Some(policy) = self.get_persona_hook_skill_policy(chat_id, persona_id)? else {
            return Ok(true);
        };
        let Some(allowed) = policy.allowed_skill_names else {
            return Ok(true);
        };
        Ok(allowed.iter().any(|s| s.eq_ignore_ascii_case(skill_name)))
    }

    pub fn is_hook_allowed_for_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
        hook_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let Some(policy) = self.get_persona_hook_skill_policy(chat_id, persona_id)? else {
            return Ok(true);
        };
        let Some(allowed) = policy.allowed_hook_ids else {
            return Ok(true);
        };
        Ok(allowed.contains(&hook_id))
    }

    pub fn list_channel_persona_policies(
        &self,
        canonical_chat_id: i64,
    ) -> Result<Vec<ChannelPersonaPolicy>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT canonical_chat_id, bot_instance_id, mode, persona_id, updated_at
             FROM channel_persona_policy
             WHERE canonical_chat_id = ?1
             ORDER BY bot_instance_id ASC",
        )?;
        let rows = stmt.query_map(params![canonical_chat_id], |row| {
            let mode_raw: String = row.get(2)?;
            let mode = ChannelPersonaMode::try_from(mode_raw.as_str()).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(ChannelPersonaPolicy {
                canonical_chat_id: row.get(0)?,
                bot_instance_id: row.get(1)?,
                mode,
                persona_id: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_channel_persona_policy(
        &self,
        canonical_chat_id: i64,
        bot_instance_id: i64,
    ) -> Result<Option<ChannelPersonaPolicy>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT canonical_chat_id, bot_instance_id, mode, persona_id, updated_at
             FROM channel_persona_policy
             WHERE canonical_chat_id = ?1 AND bot_instance_id = ?2",
        )?;
        let row = stmt.query_row(params![canonical_chat_id, bot_instance_id], |row| {
            let mode_raw: String = row.get(2)?;
            let mode = ChannelPersonaMode::try_from(mode_raw.as_str()).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(ChannelPersonaPolicy {
                canonical_chat_id: row.get(0)?,
                bot_instance_id: row.get(1)?,
                mode,
                persona_id: row.get(3)?,
                updated_at: row.get(4)?,
            })
        });
        match row {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_channel_persona_policy(
        &self,
        canonical_chat_id: i64,
        bot_instance_id: i64,
        mode: ChannelPersonaMode,
        persona_id: Option<i64>,
    ) -> Result<(), FinallyAValueBotError> {
        if bot_instance_id == BOT_INSTANCE_WEB {
            return Err(FinallyAValueBotError::ToolExecution(
                "Persona scope policy does not apply to web chat; the Web UI selects persona."
                    .into(),
            ));
        }
        if mode == ChannelPersonaMode::All {
            self.validate_all_persona_slot(canonical_chat_id, bot_instance_id)?;
        }
        if mode == ChannelPersonaMode::Single {
            let pid = persona_id.ok_or_else(|| {
                FinallyAValueBotError::ToolExecution(
                    "persona_id is required for single persona mode".to_string(),
                )
            })?;
            if !self.persona_exists(canonical_chat_id, pid)? {
                return Err(FinallyAValueBotError::ToolExecution(format!(
                    "persona_id {pid} does not exist for chat {canonical_chat_id}"
                )));
            }
        }
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let pid = if mode == ChannelPersonaMode::Single {
            persona_id
        } else {
            None
        };
        conn.execute(
            "INSERT INTO channel_persona_policy (canonical_chat_id, bot_instance_id, mode, persona_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(canonical_chat_id, bot_instance_id)
             DO UPDATE SET mode=excluded.mode, persona_id=excluded.persona_id, updated_at=excluded.updated_at",
            params![canonical_chat_id, bot_instance_id, mode.as_str(), pid, now],
        )?;
        Ok(())
    }

    pub fn clear_channel_persona_policy(
        &self,
        canonical_chat_id: i64,
        bot_instance_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        self.validate_all_persona_slot(canonical_chat_id, bot_instance_id)?;
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM channel_persona_policy WHERE canonical_chat_id = ?1 AND bot_instance_id = ?2",
            params![canonical_chat_id, bot_instance_id],
        )?;
        Ok(rows > 0)
    }

    /// Get messages since the bot's last response in this chat/persona.
    /// Falls back to `fallback_limit` most recent messages if bot never responded.
    pub fn get_messages_since_last_bot_response(
        &self,
        chat_id: i64,
        persona_id: i64,
        max: usize,
        fallback: usize,
        exclude_scheduled: bool,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();

        let last_bot_ts: Option<String> = if exclude_scheduled {
            conn.query_row(
                &format!(
                    "SELECT timestamp FROM messages
                     WHERE chat_id = ?1 AND persona_id = ?2 AND is_from_bot = 1
                       AND origin != ?3
                       AND {MAIN_CHAT_MESSAGE_VISIBILITY}
                     ORDER BY timestamp DESC LIMIT 1"
                ),
                params![chat_id, persona_id, MESSAGE_ORIGIN_SCHEDULED],
                |row| row.get(0),
            )
            .ok()
        } else {
            conn.query_row(
                &format!(
                    "SELECT timestamp FROM messages
                     WHERE chat_id = ?1 AND persona_id = ?2 AND is_from_bot = 1
                       AND {MAIN_CHAT_MESSAGE_VISIBILITY}
                     ORDER BY timestamp DESC LIMIT 1"
                ),
                params![chat_id, persona_id],
                |row| row.get(0),
            )
            .ok()
        };

        let origin_filter = if exclude_scheduled {
            " AND origin != ?5"
        } else {
            ""
        };

        let mut messages = if let Some(ts) = last_bot_ts {
            let sql = format!(
                "SELECT {MESSAGE_SELECT_COLS}
                 FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2 AND timestamp >= ?3
                   AND {MAIN_CHAT_MESSAGE_VISIBILITY}{origin_filter}
                 ORDER BY timestamp DESC
                 LIMIT ?4"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = if exclude_scheduled {
                stmt.query_map(
                    params![
                        chat_id,
                        persona_id,
                        ts,
                        max as i64,
                        MESSAGE_ORIGIN_SCHEDULED
                    ],
                    stored_message_from_row,
                )?
            } else {
                stmt.query_map(
                    params![chat_id, persona_id, ts, max as i64],
                    stored_message_from_row,
                )?
            };
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let sql = format!(
                "SELECT {MESSAGE_SELECT_COLS}
                 FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2
                   AND {MAIN_CHAT_MESSAGE_VISIBILITY}{origin_filter}
                 ORDER BY timestamp DESC
                 LIMIT ?3"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = if exclude_scheduled {
                stmt.query_map(
                    params![
                        chat_id,
                        persona_id,
                        fallback as i64,
                        MESSAGE_ORIGIN_SCHEDULED
                    ],
                    stored_message_from_row,
                )?
            } else {
                stmt.query_map(
                    params![chat_id, persona_id, fallback as i64],
                    stored_message_from_row,
                )?
            };
            rows.collect::<Result<Vec<_>, _>>()?
        };

        messages.reverse();
        Ok(messages)
    }

    // --- Scheduled tasks ---

    pub fn create_scheduled_task(
        &self,
        chat_id: i64,
        prompt: &str,
        schedule_type: &str,
        schedule_value: &str,
        next_run: &str,
    ) -> Result<i64, FinallyAValueBotError> {
        let persona_id = self.get_current_persona_id(chat_id)?;
        self.create_scheduled_task_for_persona(
            chat_id,
            persona_id,
            prompt,
            schedule_type,
            schedule_value,
            next_run,
        )
    }

    pub fn create_scheduled_task_for_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
        prompt: &str,
        schedule_type: &str,
        schedule_value: &str,
        next_run: &str,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO scheduled_tasks (chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
            params![
                chat_id,
                persona_id,
                prompt,
                schedule_type,
                schedule_value,
                next_run,
                now
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn ensure_unique_cron_task_by_prompt_prefix(
        &self,
        chat_id: i64,
        persona_id: i64,
        prompt_prefix: &str,
        prompt: &str,
        cron_expr: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let like_pattern = format!("{prompt_prefix}%");
        let mut stmt = conn.prepare(
            "SELECT id FROM scheduled_tasks
             WHERE chat_id = ?1
               AND persona_id = ?2
               AND status = 'active'
               AND schedule_type = 'cron'
               AND prompt LIKE ?3
             ORDER BY id ASC",
        )?;
        let existing_ids: Vec<i64> = stmt
            .query_map(params![chat_id, persona_id, like_pattern], |row| row.get(0))?
            .filter_map(Result::ok)
            .collect();

        let now = chrono::Utc::now().to_rfc3339();
        if let Some(primary_id) = existing_ids.first().copied() {
            conn.execute(
                "UPDATE scheduled_tasks
                 SET prompt = ?1, schedule_value = ?2, status = 'active'
                 WHERE id = ?3",
                params![prompt, cron_expr, primary_id],
            )?;
            for dup_id in existing_ids.into_iter().skip(1) {
                conn.execute(
                    "UPDATE scheduled_tasks SET status = 'inactive' WHERE id = ?1",
                    params![dup_id],
                )?;
            }
        } else {
            conn.execute(
                "INSERT INTO scheduled_tasks (chat_id, prompt, schedule_type, schedule_value, next_run, status, created_at)
                 VALUES (?1, ?2, ?3, 'cron', ?4, ?5, 'active', ?5)",
                params![chat_id, persona_id, prompt, cron_expr, now],
            )?;
        }
        Ok(())
    }

    pub fn ensure_indexing_task(
        &self,
        chat_id: i64,
        persona_id: i64,
        prompt: &str,
        cron_expr: &str,
    ) -> Result<(), FinallyAValueBotError> {
        self.ensure_unique_cron_task_by_prompt_prefix(
            chat_id,
            persona_id,
            "Run the vault indexing script:",
            prompt,
            cron_expr,
        )
    }

    pub fn ensure_vault_push_task(
        &self,
        chat_id: i64,
        persona_id: i64,
        prompt: &str,
        cron_expr: &str,
    ) -> Result<(), FinallyAValueBotError> {
        self.ensure_unique_cron_task_by_prompt_prefix(
            chat_id,
            persona_id,
            "Sync ORIGIN vault to git remote:",
            prompt,
            cron_expr,
        )
    }

    pub fn ensure_onboarding_task(
        &self,
        chat_id: i64,
        persona_id: i64,
        prompt: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        // Only seed if no messages exist yet (fresh install)
        let message_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;

        if message_count == 0 {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM scheduled_tasks WHERE prompt = ?1 AND status = 'active')",
                params![prompt],
                |row| row.get(0),
            )?;

            if !exists {
                let now = chrono::Utc::now().to_rfc3339();
                conn.execute(
                    "INSERT INTO scheduled_tasks (chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, status, created_at)
                     VALUES (?1, ?2, ?3, 'once', ?4, ?4, 'active', ?4)",
                    params![chat_id, persona_id, prompt, now],
                )?;
            }
        }
        Ok(())
    }

    pub fn get_due_tasks(&self, now: &str) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE status = 'active' AND next_run <= ?1
             ORDER BY next_run ASC, id ASC",
        )?;
        let tasks = stmt
            .query_map(params![now], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    pub fn get_all_active_tasks(&self) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE status IN ('active', 'running', 'paused')
             ORDER BY id",
        )?;
        let tasks = stmt
            .query_map([], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// All scheduled tasks for /schedule and list_scheduled_tasks:
    /// active, running, paused, and completed (all chats/personas).
    pub fn get_all_scheduled_tasks_for_display(
        &self,
    ) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE status IN ('active', 'running', 'paused', 'completed')
             ORDER BY id",
        )?;
        let tasks = stmt
            .query_map([], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// All scheduled tasks for display filtered by chat:
    /// active, running, paused, and completed.
    pub fn get_scheduled_tasks_for_chat_for_display(
        &self,
        chat_id: i64,
    ) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE chat_id = ?1 AND status IN ('active', 'running', 'paused', 'completed')
             ORDER BY id",
        )?;
        let tasks = stmt
            .query_map(params![chat_id], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Scheduled tasks for display filtered by chat and persona (active, running, paused, completed).
    pub fn get_scheduled_tasks_for_chat_and_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE chat_id = ?1 AND persona_id = ?2 AND status IN ('active', 'running', 'paused', 'completed')
             ORDER BY id",
        )?;
        let tasks = stmt
            .query_map(params![chat_id, persona_id], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    pub fn get_tasks_for_chat(
        &self,
        chat_id: i64,
    ) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE chat_id = ?1 AND status IN ('active', 'running', 'paused')
             ORDER BY id",
        )?;
        let tasks = stmt
            .query_map(params![chat_id], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Active/running/paused scheduled tasks for one persona in a chat.
    pub fn get_tasks_for_chat_and_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Vec<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE chat_id = ?1 AND persona_id = ?2 AND status IN ('active', 'running', 'paused')
             ORDER BY id",
        )?;
        let tasks = stmt
            .query_map(params![chat_id, persona_id], map_scheduled_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    pub fn get_task_by_id(
        &self,
        task_id: i64,
    ) -> Result<Option<ScheduledTask>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, chat_id, persona_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at
             FROM scheduled_tasks
             WHERE id = ?1",
            params![task_id],
            map_scheduled_task_row,
        );
        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_task_status(
        &self,
        task_id: i64,
        status: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE scheduled_tasks SET status = ?1 WHERE id = ?2",
            params![status, task_id],
        )?;
        Ok(rows > 0)
    }

    pub fn update_task_persona(
        &self,
        task_id: i64,
        persona_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE scheduled_tasks SET persona_id = ?1 WHERE id = ?2",
            params![persona_id, task_id],
        )?;
        Ok(rows > 0)
    }

    pub fn update_task_prompt(
        &self,
        task_id: i64,
        prompt: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE scheduled_tasks SET prompt = ?1 WHERE id = ?2",
            params![prompt, task_id],
        )?;
        Ok(rows > 0)
    }

    /// Update cron/once schedule fields and next run (after preflight). Does not clear `last_run`.
    pub fn update_task_schedule(
        &self,
        task_id: i64,
        schedule_type: &str,
        schedule_value: &str,
        next_run: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE scheduled_tasks SET schedule_type = ?1, schedule_value = ?2, next_run = ?3 WHERE id = ?4",
            params![schedule_type, schedule_value, next_run, task_id],
        )?;
        Ok(rows > 0)
    }

    pub fn update_task_after_run(
        &self,
        task_id: i64,
        last_run: &str,
        next_run: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        match next_run {
            Some(next) => {
                conn.execute(
                    "UPDATE scheduled_tasks SET last_run = ?1, next_run = ?2 WHERE id = ?3",
                    params![last_run, next, task_id],
                )?;
            }
            None => {
                // One-shot task, mark completed
                conn.execute(
                    "UPDATE scheduled_tasks SET last_run = ?1, status = 'completed' WHERE id = ?2",
                    params![last_run, task_id],
                )?;
            }
        }
        Ok(())
    }

    /// Mark a due task as running so it does not get picked again while executing.
    /// For cron tasks, next_run should be precomputed and stored here.
    pub fn mark_task_running(
        &self,
        task_id: i64,
        started_at: &str,
        next_run: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        match next_run {
            Some(next) => {
                conn.execute(
                    "UPDATE scheduled_tasks
                     SET last_run = ?1, next_run = ?2, status = 'running'
                     WHERE id = ?3",
                    params![started_at, next, task_id],
                )?;
            }
            None => {
                conn.execute(
                    "UPDATE scheduled_tasks
                     SET last_run = ?1, status = 'running'
                     WHERE id = ?2",
                    params![started_at, task_id],
                )?;
            }
        }
        Ok(())
    }

    /// Atomic conditional claim: only marks running if the task is still active and due.
    /// Returns true iff exactly one row was updated. Callers should skip spawn when false.
    pub fn try_mark_task_running(
        &self,
        task_id: i64,
        started_at: &str,
        next_run: Option<&str>,
        now_upper_bound: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = match next_run {
            Some(next) => conn.execute(
                "UPDATE scheduled_tasks
                 SET last_run = ?1, next_run = ?2, status = 'running'
                 WHERE id = ?3 AND status = 'active' AND next_run <= ?4",
                params![started_at, next, task_id, now_upper_bound],
            )?,
            None => conn.execute(
                "UPDATE scheduled_tasks
                 SET last_run = ?1, status = 'running'
                 WHERE id = ?2 AND status = 'active' AND next_run <= ?3",
                params![started_at, task_id, now_upper_bound],
            )?,
        };
        Ok(rows == 1)
    }

    /// Reset tasks stuck in `running` (e.g. process crash or hung agent) back to `active`.
    /// `last_run` holds the claim/start time from `mark_task_running`. Does not change `next_run`.
    /// Returns IDs of reclaimed tasks.
    pub fn reclaim_stale_running_tasks(
        &self,
        now_rfc3339: &str,
        max_age_secs: i64,
    ) -> Result<Vec<i64>, FinallyAValueBotError> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_rfc3339)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "reclaim_stale_running_tasks: invalid now timestamp: {e}"
                ))
            })?;

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, last_run FROM scheduled_tasks WHERE status = 'running' AND last_run IS NOT NULL",
        )?;
        let pending: Vec<(i64, String)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut reclaimed = Vec::new();
        for (id, last_run) in pending {
            let Ok(started) = DateTime::parse_from_rfc3339(&last_run) else {
                continue;
            };
            let started = started.with_timezone(&Utc);
            if now.signed_duration_since(started).num_seconds() > max_age_secs {
                conn.execute(
                    "UPDATE scheduled_tasks SET status = 'active' WHERE id = ?1",
                    params![id],
                )?;
                reclaimed.push(id);
            }
        }
        Ok(reclaimed)
    }

    /// Finalize a running task after execution.
    /// - Cron tasks (Some next_run) return to active with the provided next run.
    /// - One-shot tasks (None) are marked completed.
    pub fn finalize_task_run(
        &self,
        task_id: i64,
        next_run: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        match next_run {
            Some(next) => {
                conn.execute(
                    "UPDATE scheduled_tasks
                     SET next_run = ?1, status = 'active'
                     WHERE id = ?2",
                    params![next, task_id],
                )?;
            }
            None => {
                conn.execute(
                    "UPDATE scheduled_tasks SET status = 'completed' WHERE id = ?1",
                    params![task_id],
                )?;
            }
        }
        Ok(())
    }

    // --- Task run logs ---

    #[allow(clippy::too_many_arguments)]
    pub fn log_task_run(
        &self,
        task_id: i64,
        chat_id: i64,
        started_at: &str,
        finished_at: &str,
        duration_ms: i64,
        success: bool,
        result_summary: Option<&str>,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO task_run_logs (task_id, chat_id, started_at, finished_at, duration_ms, success, result_summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                task_id,
                chat_id,
                started_at,
                finished_at,
                duration_ms,
                success as i32,
                result_summary,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_task_run_logs(
        &self,
        task_id: i64,
        limit: usize,
    ) -> Result<Vec<TaskRunLog>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, chat_id, started_at, finished_at, duration_ms, success, result_summary
             FROM task_run_logs
             WHERE task_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let logs = stmt
            .query_map(params![task_id, limit as i64], |row| {
                Ok(TaskRunLog {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    chat_id: row.get(2)?,
                    started_at: row.get(3)?,
                    finished_at: row.get(4)?,
                    duration_ms: row.get(5)?,
                    success: row.get::<_, i32>(6)? != 0,
                    result_summary: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    // --- Cursor agent runs ---

    pub fn get_cursor_engine_agent_id(
        &self,
        chat_id: i64,
        persona_id: i64,
        session_scope: &str,
    ) -> Result<Option<String>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT agent_id FROM cursor_engine_agents
             WHERE chat_id = ?1 AND persona_id = ?2 AND session_scope = ?3",
        )?;
        let mut rows = stmt.query(params![chat_id, persona_id, session_scope])?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            if id.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(id))
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_cursor_engine_agent_id(
        &self,
        chat_id: i64,
        persona_id: i64,
        session_scope: &str,
        agent_id: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let updated_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO cursor_engine_agents (chat_id, persona_id, session_scope, agent_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(chat_id, persona_id, session_scope)
             DO UPDATE SET agent_id = excluded.agent_id, updated_at = excluded.updated_at",
            params![chat_id, persona_id, session_scope, agent_id, updated_at],
        )?;
        Ok(())
    }

    pub fn clear_cursor_engine_agent_id(
        &self,
        chat_id: i64,
        persona_id: i64,
        session_scope: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM cursor_engine_agents
             WHERE chat_id = ?1 AND persona_id = ?2 AND session_scope = ?3",
            params![chat_id, persona_id, session_scope],
        )?;
        Ok(())
    }

    pub fn insert_cursor_agent_run(
        &self,
        chat_id: i64,
        channel: &str,
        prompt_preview: &str,
        workdir: Option<&str>,
        started_at: &str,
        finished_at: &str,
        success: bool,
        exit_code: Option<i32>,
        output_preview: Option<&str>,
        output_path: Option<&str>,
        tmux_session: Option<&str>,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO cursor_agent_runs (chat_id, channel, prompt_preview, workdir, started_at, finished_at, success, exit_code, output_preview, output_path, tmux_session)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                chat_id,
                channel,
                prompt_preview,
                workdir,
                started_at,
                finished_at,
                success as i32,
                exit_code,
                output_preview,
                output_path,
                tmux_session,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent cursor-agent runs, optionally filtered by chat_id. Ordered by finished_at DESC.
    pub fn get_cursor_agent_runs(
        &self,
        chat_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<CursorAgentRun>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let runs: Vec<CursorAgentRun> = match chat_id {
            Some(cid) => {
                let mut stmt = conn.prepare(
                    "SELECT id, chat_id, channel, prompt_preview, workdir, started_at, finished_at, success, exit_code, output_preview, output_path, tmux_session
                     FROM cursor_agent_runs WHERE chat_id = ?1 ORDER BY finished_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![cid, limit as i64], |row| {
                    Ok(CursorAgentRun {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        channel: row.get(2)?,
                        prompt_preview: row.get(3)?,
                        workdir: row.get(4)?,
                        started_at: row.get(5)?,
                        finished_at: row.get(6)?,
                        success: row.get::<_, i32>(7)? != 0,
                        exit_code: row.get(8)?,
                        output_preview: row.get(9)?,
                        output_path: row.get(10)?,
                        tmux_session: row.get(11)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, chat_id, channel, prompt_preview, workdir, started_at, finished_at, success, exit_code, output_preview, output_path, tmux_session
                     FROM cursor_agent_runs ORDER BY finished_at DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit as i64], |row| {
                    Ok(CursorAgentRun {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        channel: row.get(2)?,
                        prompt_preview: row.get(3)?,
                        workdir: row.get(4)?,
                        started_at: row.get(5)?,
                        finished_at: row.get(6)?,
                        success: row.get::<_, i32>(7)? != 0,
                        exit_code: row.get(8)?,
                        output_preview: row.get(9)?,
                        output_path: row.get(10)?,
                        tmux_session: row.get(11)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(runs)
    }

    // --- Projects / workflows / timeline ---

    pub fn upsert_project(
        &self,
        owner_chat_id: i64,
        title: &str,
        project_type: &str,
        status: &str,
        canonical_path: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO projects (owner_chat_id, title, project_type, status, canonical_path, metadata_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(owner_chat_id, title) DO UPDATE SET
               project_type = excluded.project_type,
               status = excluded.status,
               canonical_path = excluded.canonical_path,
               metadata_json = excluded.metadata_json,
               updated_at = excluded.updated_at",
            params![
                owner_chat_id,
                title,
                project_type,
                status,
                canonical_path,
                metadata_json.unwrap_or("{}"),
                now
            ],
        )?;
        let id = conn.query_row(
            "SELECT id FROM projects WHERE owner_chat_id = ?1 AND title = ?2",
            params![owner_chat_id, title],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(id)
    }

    pub fn get_recent_project_for_contact(
        &self,
        owner_chat_id: i64,
    ) -> Result<Option<ProjectRecord>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, owner_chat_id, title, project_type, status, canonical_path, metadata_json, updated_at
             FROM projects
             WHERE owner_chat_id = ?1
             ORDER BY updated_at DESC
             LIMIT 1",
            params![owner_chat_id],
            |row| {
                Ok(ProjectRecord {
                    id: row.get(0)?,
                    owner_chat_id: row.get(1)?,
                    title: row.get(2)?,
                    project_type: row.get(3)?,
                    status: row.get(4)?,
                    canonical_path: row.get(5)?,
                    metadata_json: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        );
        match result {
            Ok(project) => Ok(Some(project)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn touch_project_status(
        &self,
        project_id: i64,
        status: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE projects SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, project_id],
        )?;
        Ok(())
    }

    pub fn link_project_run(
        &self,
        project_id: i64,
        run_key: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR IGNORE INTO project_runs (project_id, run_key, created_at) VALUES (?1, ?2, ?3)",
            params![project_id, run_key, now],
        )?;
        Ok(())
    }

    pub fn append_run_timeline_event(
        &self,
        run_key: &str,
        chat_id: i64,
        persona_id: i64,
        event_type: &str,
        payload_json: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO run_timeline_events (run_key, chat_id, persona_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_key,
                chat_id,
                persona_id,
                event_type,
                payload_json.unwrap_or("{}"),
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_run_timeline_events(
        &self,
        run_key: &str,
        limit: usize,
    ) -> Result<Vec<RunTimelineEvent>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, run_key, chat_id, persona_id, event_type, payload_json, created_at
             FROM run_timeline_events
             WHERE run_key = ?1
             ORDER BY id ASC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![run_key, limit as i64], |row| {
                Ok(RunTimelineEvent {
                    id: row.get(0)?,
                    run_key: row.get(1)?,
                    chat_id: row.get(2)?,
                    persona_id: row.get(3)?,
                    event_type: row.get(4)?,
                    payload_json: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn append_persona_bulletin_event(
        &self,
        chat_id: i64,
        persona_id: i64,
        run_key: Option<&str>,
        event_type: &str,
        title: &str,
        detail: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO persona_bulletin_events (chat_id, persona_id, run_key, event_type, title, detail, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![chat_id, persona_id, run_key, event_type, title, detail, now],
        )?;
        Ok(())
    }

    pub fn upsert_persona_bulletin_focus(
        &self,
        chat_id: i64,
        persona_id: i64,
        title: Option<&str>,
        content: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO persona_bulletin_focus (chat_id, persona_id, title, content, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(chat_id, persona_id) DO UPDATE SET
               title = excluded.title,
               content = excluded.content,
               updated_at = excluded.updated_at",
            params![chat_id, persona_id, title, content, now],
        )?;
        Ok(())
    }

    pub fn get_persona_bulletin_focus(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Option<PersonaBulletinFocus>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT chat_id, persona_id, title, content, updated_at
             FROM persona_bulletin_focus
             WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
            |row| {
                Ok(PersonaBulletinFocus {
                    chat_id: row.get(0)?,
                    persona_id: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_persona_bulletin_events(
        &self,
        chat_id: i64,
        persona_id: i64,
        limit: usize,
    ) -> Result<Vec<PersonaBulletinEvent>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, run_key, event_type, title, detail, created_at
             FROM persona_bulletin_events
             WHERE chat_id = ?1 AND persona_id = ?2
             ORDER BY created_at DESC, id DESC
             LIMIT ?3",
        )?;
        let items = stmt
            .query_map(params![chat_id, persona_id, limit as i64], |row| {
                Ok(PersonaBulletinEvent {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    run_key: row.get(3)?,
                    event_type: row.get(4)?,
                    title: row.get(5)?,
                    detail: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn upsert_persona_message_bookmark(
        &self,
        chat_id: i64,
        persona_id: i64,
        message_id: &str,
        role: &str,
        content_preview: &str,
        note: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO persona_message_bookmarks (
                chat_id, persona_id, message_id, role, content_preview, note, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(chat_id, persona_id, message_id) DO UPDATE SET
                role = excluded.role,
                content_preview = excluded.content_preview,
                note = excluded.note,
                updated_at = excluded.updated_at",
            params![
                chat_id,
                persona_id,
                message_id,
                role,
                content_preview,
                note,
                now
            ],
        )?;
        Ok(())
    }

    pub fn delete_persona_message_bookmark(
        &self,
        chat_id: i64,
        persona_id: i64,
        message_id: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM persona_message_bookmarks
             WHERE chat_id = ?1 AND persona_id = ?2 AND message_id = ?3",
            params![chat_id, persona_id, message_id],
        )?;
        Ok(rows > 0)
    }

    pub fn list_persona_message_bookmarks(
        &self,
        chat_id: i64,
        persona_id: i64,
        limit: usize,
    ) -> Result<Vec<PersonaMessageBookmark>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chat_id, persona_id, message_id, role, content_preview, note, created_at, updated_at
             FROM persona_message_bookmarks
             WHERE chat_id = ?1 AND persona_id = ?2
             ORDER BY updated_at DESC
             LIMIT ?3",
        )?;
        let items = stmt
            .query_map(params![chat_id, persona_id, limit as i64], |row| {
                Ok(PersonaMessageBookmark {
                    chat_id: row.get(0)?,
                    persona_id: row.get(1)?,
                    message_id: row.get(2)?,
                    role: row.get(3)?,
                    content_preview: row.get(4)?,
                    note: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn message_exists_in_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
        message_id: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM messages
                WHERE chat_id = ?1 AND persona_id = ?2 AND id = ?3
            )",
            params![chat_id, persona_id, message_id],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    pub fn get_message_for_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
        message_id: &str,
    ) -> Result<Option<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            &format!(
                "SELECT {MESSAGE_SELECT_COLS}
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2 AND id = ?3
             LIMIT 1"
            ),
            params![chat_id, persona_id, message_id],
            stored_message_from_row,
        );
        match result {
            Ok(msg) => Ok(Some(msg)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // --- Background jobs ---

    pub fn create_background_job(
        &self,
        id: &str,
        chat_id: i64,
        persona_id: i64,
        prompt: &str,
        trigger_reason: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO background_jobs (id, chat_id, persona_id, prompt, status, trigger_reason, created_at, last_progress_at, last_stage, job_kind)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?6, 'pending', 'agent')",
            params![id, chat_id, persona_id, prompt, trigger_reason, now],
        )?;
        Ok(())
    }

    pub fn create_background_run_optimize_job(
        &self,
        id: &str,
        chat_id: i64,
        persona_id: i64,
        history_filename: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let label = "Learn & optimize";
        conn.execute(
            "INSERT INTO background_jobs (id, chat_id, persona_id, prompt, status, trigger_reason, created_at, last_progress_at, last_stage, job_kind, label)
             VALUES (?1, ?2, ?3, ?4, 'pending', 'run_optimize', ?5, ?5, 'pending', 'run_optimize', ?6)",
            params![id, chat_id, persona_id, history_filename, now, label],
        )?;
        Ok(())
    }

    pub fn create_background_shell_job(
        &self,
        id: &str,
        chat_id: i64,
        persona_id: i64,
        label: &str,
        shell_command: &str,
        workdir: &str,
        tmux_session: &str,
        output_path: &str,
        trigger_reason: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO background_jobs (
                id, chat_id, persona_id, prompt, status, trigger_reason, created_at, last_progress_at, last_stage,
                job_kind, shell_command, workdir, tmux_session, output_path, label
             ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?6, 'pending', 'shell', ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                chat_id,
                persona_id,
                label,
                trigger_reason,
                now,
                shell_command,
                workdir,
                tmux_session,
                output_path,
                label,
            ],
        )?;
        Ok(())
    }

    /// External system id only (ComfyUI `prompt_id`, etc.): visible in queue, does not block handoff/shell slots.
    pub fn create_background_tracked_job(
        &self,
        id: &str,
        chat_id: i64,
        persona_id: i64,
        prompt: &str,
        label: Option<&str>,
        trigger_reason: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO background_jobs (
                id, chat_id, persona_id, prompt, status, trigger_reason,
                created_at, started_at, last_progress_at, last_stage, job_kind, label
             ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6, ?6, ?6, 'external_queue', 'tracked', ?7)",
            params![id, chat_id, persona_id, prompt, trigger_reason, now, label],
        )?;
        Ok(())
    }

    pub fn set_background_shell_paths(
        &self,
        id: &str,
        tmux_session: &str,
        output_path: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE background_jobs SET tmux_session = ?1, output_path = ?2 WHERE id = ?3",
            params![tmux_session, output_path, id],
        )?;
        if rows == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "set_background_shell_paths: no job row for id {id}"
            )));
        }
        Ok(())
    }

    /// Record that the user was notified about a terminal shell job (e.g. after silent reconcile).
    pub fn record_background_shell_user_notification(
        &self,
        id: &str,
        result_text: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE background_jobs
             SET result_text = ?1,
                 last_progress_at = ?2,
                 last_stage = 'user_notified'
             WHERE id = ?3",
            params![result_text, now, id],
        )?;
        Ok(())
    }

    /// Agent background jobs enqueued after a successful shell job (`shell_success_followup:{parent_id}:N`).
    pub fn count_shell_success_agent_followups(
        &self,
        parent_shell_job_id: &str,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("shell_success_followup:{parent_shell_job_id}:%");
        let count = conn.query_row(
            "SELECT COUNT(*) FROM background_jobs
             WHERE job_kind = 'agent' AND trigger_reason LIKE ?1",
            params![pattern],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count)
    }

    pub fn mark_background_shell_agent_success_followup_enqueued(
        &self,
        id: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE background_jobs
             SET last_progress_at = ?1, last_stage = 'agent_success_followup_enqueued'
             WHERE id = ?2 AND job_kind = 'shell'",
            params![now, id],
        )?;
        Ok(())
    }

    /// Agent background jobs enqueued after a shell failure (`shell_failure_retry:{parent_id}:N`).
    pub fn count_shell_failure_agent_retries(
        &self,
        parent_shell_job_id: &str,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("shell_failure_retry:{parent_shell_job_id}:%");
        let count = conn.query_row(
            "SELECT COUNT(*) FROM background_jobs
             WHERE job_kind = 'agent' AND trigger_reason LIKE ?1",
            params![pattern],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count)
    }

    pub fn mark_background_shell_agent_retry_enqueued(
        &self,
        id: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE background_jobs
             SET last_progress_at = ?1, last_stage = 'agent_retry_enqueued'
             WHERE id = ?2 AND job_kind = 'shell'",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn mark_background_shell_finished(
        &self,
        id: &str,
        exit_code: i32,
        result_text: &str,
        success: bool,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        if success {
            conn.execute(
                "UPDATE background_jobs
                 SET status = 'done',
                     finished_at = ?1,
                     result_text = ?2,
                     exit_code = ?3,
                     lease_owner = NULL,
                     lease_expires_at = NULL,
                     last_progress_at = ?1,
                     last_stage = 'done'
                 WHERE id = ?4",
                params![now, result_text, exit_code, id],
            )?;
        } else {
            conn.execute(
                "UPDATE background_jobs
                 SET status = 'failed',
                     finished_at = ?1,
                     error_text = ?2,
                     result_text = ?2,
                     exit_code = ?3,
                     lease_owner = NULL,
                     lease_expires_at = NULL,
                     last_progress_at = ?1,
                     last_stage = 'failed'
                 WHERE id = ?4",
                params![now, result_text, exit_code, id],
            )?;
        }
        Ok(())
    }

    pub fn list_shell_jobs_needing_notification(
        &self,
    ) -> Result<Vec<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{BG_JOB_SELECT}
             FROM background_jobs
             WHERE job_kind = 'shell'
               AND status = 'failed'
               AND (result_text IS NULL OR TRIM(result_text) = '')"
        ))?;
        let jobs = stmt
            .query_map([], map_background_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn list_running_shell_background_jobs(
        &self,
    ) -> Result<Vec<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{BG_JOB_SELECT}
             FROM background_jobs
             WHERE job_kind = 'shell' AND status = 'running' AND tmux_session IS NOT NULL"
        ))?;
        let jobs = stmt
            .query_map([], map_background_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    /// Running shell jobs plus prematurely-failed rows still awaiting user notification.
    pub fn list_shell_jobs_for_monitor(&self) -> Result<Vec<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{BG_JOB_SELECT}
             FROM background_jobs
             WHERE job_kind = 'shell'
               AND tmux_session IS NOT NULL
               AND (
                    status = 'running'
                    OR (
                        status = 'failed'
                        AND (result_text IS NULL OR TRIM(result_text) = '')
                    )
               )"
        ))?;
        let jobs = stmt
            .query_map([], map_background_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn list_shell_jobs_with_expired_lease(
        &self,
        now_rfc3339: &str,
    ) -> Result<Vec<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{BG_JOB_SELECT}
             FROM background_jobs
             WHERE job_kind = 'shell'
               AND status = 'running'
               AND lease_expires_at IS NOT NULL
               AND lease_expires_at < ?1"
        ))?;
        let jobs = stmt
            .query_map(params![now_rfc3339], map_background_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn list_active_background_jobs_for_chat(
        &self,
        chat_id: i64,
        now_rfc3339: &str,
        pending_timeout_secs: i64,
    ) -> Result<Vec<BackgroundJob>, FinallyAValueBotError> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_rfc3339)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "list_active_background_jobs_for_chat: invalid now timestamp: {e}"
                ))
            })?;
        let pending_cutoff =
            (now - chrono::Duration::seconds(pending_timeout_secs.max(1))).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{BG_JOB_SELECT}
             FROM background_jobs
             WHERE chat_id = ?1
               AND (
                    (job_kind IS NULL OR job_kind NOT IN ('tracked'))
                    AND (
                         (status = 'pending' AND created_at >= ?2)
                         OR (
                             status IN ('running', 'completed_raw', 'main_agent_processing')
                             AND COALESCE(lease_expires_at, '9999-12-31T23:59:59+00:00') >= ?3
                         )
                    )
                    OR (job_kind = 'tracked' AND status = 'running')
               )
             ORDER BY created_at ASC"
        ))?;
        let jobs = stmt
            .query_map(
                params![chat_id, pending_cutoff, now_rfc3339],
                map_background_job_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn claim_background_job_running(
        &self,
        id: &str,
        lease_owner: &str,
        lease_ttl_secs: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let lease_expires =
            (now_dt + chrono::Duration::seconds(lease_ttl_secs.max(1))).to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET status = 'running',
                 started_at = COALESCE(started_at, ?1),
                 lease_owner = ?2,
                 lease_expires_at = ?3,
                 last_progress_at = ?1,
                 last_stage = 'running'
             WHERE id = ?4 AND status = 'pending'",
            params![now, lease_owner, lease_expires, id],
        )?;
        Ok(rows == 1)
    }

    pub fn renew_background_job_lease(
        &self,
        id: &str,
        lease_ttl_secs: i64,
        stage: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let lease_expires =
            (now_dt + chrono::Duration::seconds(lease_ttl_secs.max(1))).to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET lease_expires_at = ?1,
                 last_progress_at = ?2,
                 last_stage = ?3
             WHERE id = ?4
               AND status IN ('running', 'completed_raw', 'main_agent_processing')",
            params![lease_expires, now, stage, id],
        )?;
        if rows == 0 {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn mark_background_job_completed_raw(
        &self,
        id: &str,
        result_text: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let lease_expires = (now_dt + chrono::Duration::seconds(120)).to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET status = 'completed_raw',
                 finished_at = ?1,
                 result_text = ?2,
                 lease_expires_at = ?3,
                 last_progress_at = ?1,
                 last_stage = 'completed_raw'
             WHERE id = ?4",
            params![now, result_text, lease_expires, id],
        )?;
        if rows == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "mark_background_job_completed_raw: no job row for id {id}"
            )));
        }
        Ok(())
    }

    pub fn mark_background_job_main_agent_processing(
        &self,
        id: &str,
        lease_ttl_secs: i64,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now_dt = chrono::Utc::now();
        let now = now_dt.to_rfc3339();
        let lease_expires =
            (now_dt + chrono::Duration::seconds(lease_ttl_secs.max(1))).to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET status = 'main_agent_processing',
                 lease_expires_at = ?1,
                 last_progress_at = ?2,
                 last_stage = 'main_agent_processing'
             WHERE id = ?3",
            params![lease_expires, now, id],
        )?;
        if rows == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "mark_background_job_main_agent_processing: no job row for id {id}"
            )));
        }
        Ok(())
    }

    pub fn mark_background_job_done(&self, id: &str) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET status = 'done',
                 finished_at = COALESCE(finished_at, ?1),
                 lease_owner = NULL,
                 lease_expires_at = NULL,
                 last_progress_at = ?1,
                 last_stage = 'done'
             WHERE id = ?2",
            params![now, id],
        )?;
        if rows == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "mark_background_job_done: no job row for id {id}"
            )));
        }
        Ok(())
    }

    pub fn fail_background_job(
        &self,
        id: &str,
        error_text: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET status = 'failed',
                 finished_at = ?1,
                 error_text = ?2,
                 lease_owner = NULL,
                 lease_expires_at = NULL,
                 last_progress_at = ?1,
                 last_stage = 'failed'
             WHERE id = ?3",
            params![now, error_text, id],
        )?;
        if rows == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "fail_background_job: no job row for id {id}"
            )));
        }
        Ok(())
    }

    pub fn mark_background_job_cancelled(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let rows = conn.execute(
            "UPDATE background_jobs
             SET status = 'cancelled',
                 finished_at = ?1,
                 error_text = ?2,
                 lease_owner = NULL,
                 lease_expires_at = NULL,
                 last_progress_at = ?1,
                 last_stage = 'cancelled'
             WHERE id = ?3",
            params![now, reason, id],
        )?;
        if rows == 0 {
            return Err(FinallyAValueBotError::ToolExecution(format!(
                "mark_background_job_cancelled: no job row for id {id}"
            )));
        }
        Ok(())
    }

    pub fn count_active_background_jobs_for_chat(
        &self,
        chat_id: i64,
        now_rfc3339: &str,
        pending_timeout_secs: i64,
    ) -> Result<i64, FinallyAValueBotError> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_rfc3339)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "count_active_background_jobs_for_chat: invalid now timestamp: {e}"
                ))
            })?;
        let pending_cutoff =
            (now - chrono::Duration::seconds(pending_timeout_secs.max(1))).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let count = conn.query_row(
            "SELECT COUNT(*) FROM background_jobs
             WHERE chat_id = ?1
               AND (job_kind IS NULL OR job_kind NOT IN ('tracked'))
               AND (
                    (status = 'pending' AND created_at >= ?2)
                    OR (
                        status IN ('running', 'completed_raw', 'main_agent_processing')
                        AND COALESCE(lease_expires_at, '9999-12-31T23:59:59+00:00') >= ?3
                    )
               )",
            params![chat_id, pending_cutoff, now_rfc3339],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count)
    }

    pub fn list_background_jobs_for_chat(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{BG_JOB_SELECT}
             FROM background_jobs
             WHERE chat_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2"
        ))?;
        let jobs = stmt
            .query_map(params![chat_id, limit as i64], map_background_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn get_background_job(
        &self,
        id: &str,
    ) -> Result<Option<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            &format!("{BG_JOB_SELECT} FROM background_jobs WHERE id = ?1"),
            params![id],
            map_background_job_row,
        );
        match result {
            Ok(job) => Ok(Some(job)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn upsert_job_heartbeat(
        &self,
        run_key: &str,
        chat_id: i64,
        persona_id: i64,
        job_type: &str,
        stage: &str,
        message: &str,
        active: bool,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO job_heartbeats (run_key, chat_id, persona_id, job_type, stage, message, active, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(run_key) DO UPDATE SET
               stage = excluded.stage,
               message = excluded.message,
               active = excluded.active,
               updated_at = excluded.updated_at",
            params![
                run_key,
                chat_id,
                persona_id,
                job_type,
                stage,
                message,
                if active { 1 } else { 0 },
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_job_heartbeat(
        &self,
        run_key: &str,
    ) -> Result<Option<JobHeartbeat>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT run_key, chat_id, persona_id, job_type, stage, message, active, updated_at
             FROM job_heartbeats
             WHERE run_key = ?1",
            params![run_key],
            |row| {
                Ok(JobHeartbeat {
                    run_key: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    job_type: row.get(3)?,
                    stage: row.get(4)?,
                    message: row.get(5)?,
                    active: row.get::<_, i32>(6)? != 0,
                    updated_at: row.get(7)?,
                })
            },
        );
        match result {
            Ok(h) => Ok(Some(h)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Active heartbeats for a chat (e.g. operator visibility, dashboards).
    pub fn list_active_job_heartbeats_for_chat(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<JobHeartbeat>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let lim = limit.max(1).min(100) as i64;
        let mut stmt = conn.prepare(
            "SELECT run_key, chat_id, persona_id, job_type, stage, message, active, updated_at
             FROM job_heartbeats
             WHERE chat_id = ?1 AND active = 1
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![chat_id, lim], |row| {
                Ok(JobHeartbeat {
                    run_key: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    job_type: row.get(3)?,
                    stage: row.get(4)?,
                    message: row.get(5)?,
                    active: row.get::<_, i32>(6)? != 0,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Recent heartbeats for a chat (including completed), for merging into job lists.
    pub fn list_job_heartbeats_for_chat(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<JobHeartbeat>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let lim = limit.max(1).min(200) as i64;
        let mut stmt = conn.prepare(
            "SELECT run_key, chat_id, persona_id, job_type, stage, message, active, updated_at
             FROM job_heartbeats
             WHERE chat_id = ?1
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![chat_id, lim], |row| {
                Ok(JobHeartbeat {
                    run_key: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    job_type: row.get(3)?,
                    stage: row.get(4)?,
                    message: row.get(5)?,
                    active: row.get::<_, i32>(6)? != 0,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Mark active heartbeats stale if `updated_at` is older than `max_age_secs`, append timeline
    /// events, and fail matching `manual_background` rows in `background_jobs`.
    pub fn reconcile_stale_active_job_heartbeats(
        &self,
        now_rfc3339: &str,
        max_age_secs: i64,
    ) -> Result<Vec<String>, FinallyAValueBotError> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_rfc3339)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "reconcile_stale_active_job_heartbeats: invalid now timestamp: {e}"
                ))
            })?;

        let stale_msg = "stale — no recent heartbeat (process may have exited)";
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_key, chat_id, persona_id, job_type, updated_at
             FROM job_heartbeats
             WHERE active = 1",
        )?;
        let rows: Vec<(String, i64, i64, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut reconciled = Vec::new();
        let now_str = now.to_rfc3339();

        for (run_key, chat_id, persona_id, job_type, updated_at) in rows {
            let Ok(updated) = DateTime::parse_from_rfc3339(&updated_at) else {
                continue;
            };
            let updated = updated.with_timezone(&Utc);
            if now.signed_duration_since(updated).num_seconds() <= max_age_secs {
                continue;
            }

            conn.execute(
                "UPDATE job_heartbeats
                 SET stage = 'failed', message = ?1, active = 0, updated_at = ?2
                 WHERE run_key = ?3 AND active = 1",
                params![stale_msg, now_str, run_key],
            )?;

            let payload = format!(
                r#"{{"stage":"failed","message":"{}","reason":"stale_reconcile"}}"#,
                stale_msg.replace('"', "'")
            );
            conn.execute(
                "INSERT INTO run_timeline_events (run_key, chat_id, persona_id, event_type, payload_json, created_at)
                 VALUES (?1, ?2, ?3, 'heartbeat', ?4, ?5)",
                params![run_key, chat_id, persona_id, payload, now_str],
            )?;

            if job_type == "manual_background" {
                let _ = conn.execute(
                    "UPDATE background_jobs
                     SET status = 'failed',
                         finished_at = ?1,
                         error_text = ?2,
                         lease_owner = NULL,
                         lease_expires_at = NULL,
                         last_progress_at = ?1,
                         last_stage = 'failed'
                     WHERE id = ?3
                       AND status IN ('pending', 'running', 'completed_raw', 'main_agent_processing')",
                    params![now_str, stale_msg, run_key],
                );
            }
            reconciled.push(run_key);
        }

        Ok(reconciled)
    }

    /// Fail web background jobs that are still pending and never started within the allowed age.
    pub fn reconcile_stale_pending_background_jobs(
        &self,
        now_rfc3339: &str,
        max_pending_secs: i64,
    ) -> Result<Vec<String>, FinallyAValueBotError> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_rfc3339)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "reconcile_stale_pending_background_jobs: invalid now timestamp: {e}"
                ))
            })?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created_at
             FROM background_jobs
             WHERE status = 'pending'
               AND started_at IS NULL",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let stale_msg = "stale pending job — worker never confirmed start";
        let now_str = now.to_rfc3339();
        let mut out = Vec::new();

        for (id, created_at) in rows {
            let Ok(created) = DateTime::parse_from_rfc3339(&created_at) else {
                continue;
            };
            let created = created.with_timezone(&Utc);
            if now.signed_duration_since(created).num_seconds() <= max_pending_secs {
                continue;
            }
            let n = conn.execute(
                "UPDATE background_jobs
                 SET status = 'failed',
                     finished_at = ?1,
                     error_text = ?2,
                     lease_owner = NULL,
                     lease_expires_at = NULL,
                     last_progress_at = ?1,
                     last_stage = 'failed'
                 WHERE id = ?3
                   AND status = 'pending'
                   AND started_at IS NULL",
                params![now_str, stale_msg, id],
            )?;
            if n > 0 {
                out.push(id);
            }
        }

        Ok(out)
    }

    /// Fail active web background jobs with expired leases.
    pub fn reconcile_expired_background_job_leases(
        &self,
        now_rfc3339: &str,
    ) -> Result<Vec<String>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let stale_msg = "stale lease expired — worker heartbeat not renewed";
        let mut stmt = conn.prepare(
            "SELECT id
             FROM background_jobs
             WHERE status IN ('running', 'completed_raw', 'main_agent_processing')
               AND COALESCE(job_kind, 'agent') != 'shell'
               AND lease_expires_at IS NOT NULL
               AND lease_expires_at < ?1",
        )?;
        let rows: Vec<String> = stmt
            .query_map(params![now_rfc3339], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut out = Vec::new();
        for id in rows {
            let n = conn.execute(
                "UPDATE background_jobs
                 SET status = 'failed',
                     finished_at = ?1,
                     error_text = ?2,
                     lease_owner = NULL,
                     lease_expires_at = NULL,
                     last_progress_at = ?1,
                     last_stage = 'failed'
                 WHERE id = ?3
                   AND status IN ('running', 'completed_raw', 'main_agent_processing')
                   AND COALESCE(job_kind, 'agent') != 'shell'",
                params![now_rfc3339, stale_msg, id],
            )?;
            if n > 0 {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// Fail web `background_jobs` rows that never got a heartbeat row but stayed active too long.
    pub fn reconcile_orphan_stale_background_jobs(
        &self,
        now_rfc3339: &str,
        max_age_secs: i64,
    ) -> Result<Vec<String>, FinallyAValueBotError> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(now_rfc3339)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                FinallyAValueBotError::ToolExecution(format!(
                    "reconcile_orphan_stale_background_jobs: invalid now timestamp: {e}"
                ))
            })?;

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT b.id, b.chat_id, b.persona_id, b.started_at
             FROM background_jobs b
             LEFT JOIN job_heartbeats h ON h.run_key = b.id
             WHERE b.status IN ('pending', 'running', 'completed_raw', 'main_agent_processing')
               AND b.started_at IS NOT NULL
               AND h.run_key IS NULL",
        )?;
        let rows: Vec<(String, i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let stale_msg = "stale — no heartbeat record (worker may have crashed before registration)";
        let mut out = Vec::new();
        let now_str = now.to_rfc3339();

        for (id, _chat_id, _persona_id, started_at) in rows {
            let Ok(started) = DateTime::parse_from_rfc3339(&started_at) else {
                continue;
            };
            let started = started.with_timezone(&Utc);
            if now.signed_duration_since(started).num_seconds() <= max_age_secs {
                continue;
            }
            let n = conn.execute(
                "UPDATE background_jobs
                 SET status = 'failed',
                     finished_at = ?1,
                     error_text = ?2,
                     lease_owner = NULL,
                     lease_expires_at = NULL,
                     last_progress_at = ?1,
                     last_stage = 'failed'
                 WHERE id = ?3
                   AND status IN ('pending', 'running', 'completed_raw', 'main_agent_processing')",
                params![now_str, stale_msg, id],
            )?;
            if n > 0 {
                out.push(id);
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn delete_task(&self, task_id: i64) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM scheduled_tasks WHERE id = ?1",
            params![task_id],
        )?;
        Ok(rows > 0)
    }

    // --- Sessions ---

    pub fn save_session(
        &self,
        chat_id: i64,
        persona_id: i64,
        messages_json: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO sessions (chat_id, persona_id, messages_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat_id, persona_id) DO UPDATE SET
                messages_json = ?3,
                updated_at = ?4",
            params![chat_id, persona_id, messages_json, now],
        )?;
        Ok(())
    }

    pub fn load_session(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Option<(String, String)>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT messages_json, updated_at FROM sessions WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );
        match result {
            Ok(pair) => Ok(Some(pair)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_session(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM sessions WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        Ok(rows > 0)
    }

    pub fn delete_chat_data(&self, chat_id: i64) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let mut affected = 0usize;

        affected += tx.execute(
            "UPDATE chats SET active_persona_id = NULL WHERE chat_id = ?1",
            params![chat_id],
        )?;
        affected += tx.execute("DELETE FROM sessions WHERE chat_id = ?1", params![chat_id])?;
        affected += tx.execute("DELETE FROM messages WHERE chat_id = ?1", params![chat_id])?;
        affected += tx.execute("DELETE FROM personas WHERE chat_id = ?1", params![chat_id])?;
        affected += tx.execute(
            "DELETE FROM scheduled_tasks WHERE chat_id = ?1",
            params![chat_id],
        )?;
        affected += tx.execute(
            "DELETE FROM social_oauth_tokens WHERE chat_id = ?1",
            params![chat_id],
        )?;
        affected += tx.execute(
            "DELETE FROM channel_bindings WHERE canonical_chat_id = ?1",
            params![chat_id],
        )?;
        affected += tx.execute("DELETE FROM chats WHERE chat_id = ?1", params![chat_id])?;

        tx.commit()?;
        Ok(affected > 0)
    }

    // --- Social OAuth tokens ---

    pub fn upsert_social_token(
        &self,
        platform: &str,
        chat_id: i64,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO social_oauth_tokens (platform, chat_id, access_token, refresh_token, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(platform, chat_id) DO UPDATE SET
                access_token = ?3,
                refresh_token = ?4,
                expires_at = ?5",
            params![platform, chat_id, access_token, refresh_token, expires_at],
        )?;
        Ok(())
    }

    pub fn get_social_token(
        &self,
        platform: &str,
        chat_id: i64,
    ) -> Result<Option<SocialOAuthToken>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT platform, chat_id, access_token, refresh_token, expires_at
             FROM social_oauth_tokens
             WHERE platform = ?1 AND chat_id = ?2",
            params![platform, chat_id],
            |row| {
                Ok(SocialOAuthToken {
                    platform: row.get(0)?,
                    chat_id: row.get(1)?,
                    access_token: row.get(2)?,
                    refresh_token: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            },
        );
        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_social_token(
        &self,
        platform: &str,
        chat_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM social_oauth_tokens WHERE platform = ?1 AND chat_id = ?2",
            params![platform, chat_id],
        )?;
        Ok(rows > 0)
    }

    // --- OAuth pending states (short-lived mapping from state param to chat_id) ---

    pub fn create_oauth_pending_state(
        &self,
        state_token: &str,
        platform: &str,
        chat_id: i64,
        expires_at: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO oauth_pending_states (state_token, platform, chat_id, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![state_token, platform, chat_id, expires_at],
        )?;
        Ok(())
    }

    pub fn consume_oauth_pending_state(
        &self,
        state_token: &str,
    ) -> Result<Option<(String, i64)>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT platform, chat_id FROM oauth_pending_states
             WHERE state_token = ?1 AND expires_at > datetime('now')",
            params![state_token],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        );
        let pair = match result {
            Ok(p) => p,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        conn.execute(
            "DELETE FROM oauth_pending_states WHERE state_token = ?1",
            params![state_token],
        )?;
        Ok(Some(pair))
    }

    pub fn get_new_user_messages_since(
        &self,
        chat_id: i64,
        persona_id: i64,
        since: &str,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_SELECT_COLS}
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2 AND timestamp > ?3 AND is_from_bot = 0
             ORDER BY timestamp ASC"
        ))?;
        let messages = stmt
            .query_map(params![chat_id, persona_id, since], stored_message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    // --- Personas ---

    pub fn get_or_create_default_persona(
        &self,
        chat_id: i64,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result: Option<i64> = conn
            .query_row(
                "SELECT active_persona_id FROM chats WHERE chat_id = ?1",
                params![chat_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        if let Some(pid) = result {
            if pid > 0 {
                return Ok(pid);
            }
        }
        conn.execute(
            "INSERT OR IGNORE INTO personas (chat_id, name, model_override) VALUES (?1, 'default', NULL)",
            params![chat_id],
        )?;
        let persona_id: i64 = conn.query_row(
            "SELECT id FROM personas WHERE chat_id = ?1 AND name = 'default'",
            params![chat_id],
            |row| row.get(0),
        )?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO chats (chat_id, chat_title, chat_type, last_message_time, active_persona_id)
             VALUES (?1, NULL, 'private', ?2, ?3)
             ON CONFLICT(chat_id) DO UPDATE SET active_persona_id = ?3",
            params![chat_id, now, persona_id],
        )?;
        Ok(persona_id)
    }

    pub fn get_active_persona_id(
        &self,
        chat_id: i64,
    ) -> Result<Option<i64>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT active_persona_id FROM chats WHERE chat_id = ?1",
            params![chat_id],
            |row| row.get::<_, Option<i64>>(0),
        );
        match result {
            Ok(Some(pid)) if pid > 0 => Ok(Some(pid)),
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Resolve the persona to use for this run: active when set, else create/set default.
    pub fn get_current_persona_id(&self, chat_id: i64) -> Result<i64, FinallyAValueBotError> {
        if let Ok(Some(pid)) = self.get_active_persona_id(chat_id) {
            return Ok(pid);
        }
        self.get_or_create_default_persona(chat_id)
    }

    pub fn persona_exists(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM personas
                WHERE chat_id = ?1 AND id = ?2
            )",
            params![chat_id, persona_id],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    pub fn set_active_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE chats SET active_persona_id = ?1 WHERE chat_id = ?2",
            params![persona_id, chat_id],
        )?;
        Ok(rows > 0)
    }

    pub fn list_personas(&self, chat_id: i64) -> Result<Vec<Persona>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, name, model_override, recent_history_min_user, recent_history_min_assistant, operator_memo FROM personas WHERE chat_id = ?1 ORDER BY id",
        )?;
        let personas = stmt
            .query_map(params![chat_id], |row| {
                Ok(Persona {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    name: row.get(2)?,
                    model_override: row.get(3)?,
                    recent_history_min_user: row.get(4)?,
                    recent_history_min_assistant: row.get(5)?,
                    operator_memo: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(personas)
    }

    /// Returns a `(persona_id, last_bot_message_at)` row for each persona that has at least one bot message.
    /// `last_bot_message_at` is the max `messages.timestamp` for rows where `is_from_bot = 1`.
    pub fn list_persona_last_bot_message_at(
        &self,
        chat_id: i64,
    ) -> Result<Vec<(i64, String)>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT persona_id, MAX(timestamp) AS last_at
             FROM messages
             WHERE chat_id = ?1 AND is_from_bot = 1
             GROUP BY persona_id",
        )?;
        let rows = stmt
            .query_map(params![chat_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn create_persona(
        &self,
        chat_id: i64,
        name: &str,
        model_override: Option<&str>,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO personas (chat_id, name, model_override) VALUES (?1, ?2, ?3)",
            params![chat_id, name, model_override],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_persona_by_name(
        &self,
        chat_id: i64,
        name: &str,
    ) -> Result<Option<Persona>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, chat_id, name, model_override, recent_history_min_user, recent_history_min_assistant, operator_memo FROM personas WHERE chat_id = ?1 AND name = ?2",
            params![chat_id, name],
            |row| {
                Ok(Persona {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    name: row.get(2)?,
                    model_override: row.get(3)?,
                    recent_history_min_user: row.get(4)?,
                    recent_history_min_assistant: row.get(5)?,
                    operator_memo: row.get(6)?,
                })
            },
        );
        match result {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_persona(&self, id: i64) -> Result<Option<Persona>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, chat_id, name, model_override, recent_history_min_user, recent_history_min_assistant, operator_memo FROM personas WHERE id = ?1",
            params![id],
            |row| {
                Ok(Persona {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    name: row.get(2)?,
                    model_override: row.get(3)?,
                    recent_history_min_user: row.get(4)?,
                    recent_history_min_assistant: row.get(5)?,
                    operator_memo: row.get(6)?,
                })
            },
        );
        match result {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_persona(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let name: String = conn
            .query_row(
                "SELECT name FROM personas WHERE id = ?1 AND chat_id = ?2",
                params![persona_id, chat_id],
                |row| row.get(0),
            )
            .map_err(|_| FinallyAValueBotError::ToolExecution("Persona not found".into()))?;
        if name == "default" {
            return Err(FinallyAValueBotError::ToolExecution(
                "Cannot delete the default persona".into(),
            ));
        }
        let tx = conn.unchecked_transaction()?;
        let _ = tx.execute(
            "DELETE FROM sessions WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        let _ = tx.execute(
            "DELETE FROM messages WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        let _ = tx.execute(
            "DELETE FROM persona_bulletin_events WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        let _ = tx.execute(
            "DELETE FROM persona_bulletin_focus WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        let _ = tx.execute(
            "DELETE FROM persona_message_bookmarks WHERE chat_id = ?1 AND persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        let rows = tx.execute(
            "DELETE FROM personas WHERE id = ?1 AND chat_id = ?2",
            params![persona_id, chat_id],
        )?;
        tx.execute(
            "UPDATE chats SET active_persona_id = (SELECT id FROM personas WHERE chat_id = ?1 AND name = 'default' LIMIT 1) WHERE chat_id = ?1 AND active_persona_id = ?2",
            params![chat_id, persona_id],
        )?;
        tx.commit()?;
        Ok(rows > 0)
    }

    pub fn update_persona_model(
        &self,
        chat_id: i64,
        persona_id: i64,
        model_override: Option<&str>,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE personas SET model_override = ?1 WHERE id = ?2 AND chat_id = ?3",
            params![model_override, persona_id, chat_id],
        )?;
        Ok(rows > 0)
    }

    /// Sets per-persona prompt controls. `None` for min fields writes SQL NULL (use server defaults).
    /// `operator_memo` `None` clears memo to NULL; `Some("")` also clears.
    pub fn set_persona_prompt_overrides(
        &self,
        chat_id: i64,
        persona_id: i64,
        recent_history_min_user: Option<i64>,
        recent_history_min_assistant: Option<i64>,
        operator_memo: Option<&str>,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let memo_db: Option<String> = match operator_memo {
            None => None,
            Some(s) if s.trim().is_empty() => None,
            Some(s) => Some(s.to_string()),
        };
        let rows = conn.execute(
            "UPDATE personas SET recent_history_min_user = ?1, recent_history_min_assistant = ?2, operator_memo = ?3 WHERE id = ?4 AND chat_id = ?5",
            params![
                recent_history_min_user,
                recent_history_min_assistant,
                memo_db,
                persona_id,
                chat_id
            ],
        )?;
        Ok(rows > 0)
    }

    /// Full-text search over message history for a specific chat/persona.
    /// Returns messages ranked by relevance (FTS5 rank).
    pub fn search_messages(
        &self,
        chat_id: i64,
        persona_id: i64,
        query: &str,
        limit: usize,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.chat_id, m.persona_id, m.session_id, m.sender_name, m.content, m.is_from_bot, m.timestamp, m.origin
             FROM messages_fts
             JOIN messages m ON m.rowid = messages_fts.rowid
             WHERE messages_fts MATCH ?1
               AND m.chat_id = ?2
               AND m.persona_id = ?3
               AND (?4 IS NULL OR m.timestamp >= ?4)
               AND (?5 IS NULL OR m.timestamp <= ?5)
             ORDER BY messages_fts.rank
             LIMIT ?6",
        )?;
        let messages = stmt
            .query_map(
                params![query, chat_id, persona_id, from_date, to_date, limit as i64],
                stored_message_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    // --- Chat Sessions ---

    pub fn create_chat_session(
        &self,
        id: &str,
        chat_id: i64,
        persona_id: i64,
        title: &str,
        intent: &str,
        ttl_hours: i64,
        mirror_main_chat: bool,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO chat_sessions (id, chat_id, persona_id, title, intent, status, created_at, last_active_at, ttl_hours, mirror_main_chat)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6, ?7, ?8)",
            params![
                id,
                chat_id,
                persona_id,
                title,
                intent,
                now,
                ttl_hours,
                i64::from(mirror_main_chat)
            ],
        )?;
        Ok(())
    }

    pub fn get_chat_session(
        &self,
        session_id: &str,
    ) -> Result<Option<ChatSession>, FinallyAValueBotError> {
        use rusqlite::OptionalExtension;
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                &format!(
                    "SELECT {CHAT_SESSION_SELECT_COLS}
                 FROM chat_sessions WHERE id = ?1"
                ),
                params![session_id],
                chat_session_from_row,
            )
            .optional()?;
        Ok(result)
    }

    pub fn list_chat_sessions(
        &self,
        chat_id: i64,
        persona_id: i64,
        include_archived: bool,
    ) -> Result<Vec<ChatSession>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let query = if include_archived {
            format!(
                "SELECT {CHAT_SESSION_SELECT_COLS}
             FROM chat_sessions WHERE chat_id = ?1 AND persona_id = ?2
             ORDER BY last_active_at DESC"
            )
        } else {
            format!(
                "SELECT {CHAT_SESSION_SELECT_COLS}
             FROM chat_sessions WHERE chat_id = ?1 AND persona_id = ?2 AND status = 'active'
             ORDER BY last_active_at DESC"
            )
        };
        let mut stmt = conn.prepare(&query)?;
        let sessions = stmt
            .query_map(params![chat_id, persona_id], chat_session_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn update_chat_session_last_active(
        &self,
        session_id: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET last_active_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        Ok(())
    }

    pub fn update_chat_session_title(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE chat_sessions SET title = ?1 WHERE id = ?2",
            params![title, session_id],
        )?;
        Ok(rows > 0)
    }

    pub fn update_chat_session_ttl(
        &self,
        session_id: &str,
        ttl_hours: i64,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_sessions SET ttl_hours = ?1 WHERE id = ?2",
            params![ttl_hours, session_id],
        )?;
        Ok(())
    }

    pub fn update_chat_session_mirror_main_chat(
        &self,
        session_id: &str,
        mirror_main_chat: bool,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_sessions SET mirror_main_chat = ?1 WHERE id = ?2",
            params![i64::from(mirror_main_chat), session_id],
        )?;
        Ok(())
    }

    pub fn update_chat_session_bootstrap_context(
        &self,
        session_id: &str,
        bootstrap_context_json: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_sessions SET bootstrap_context_json = ?1 WHERE id = ?2",
            params![bootstrap_context_json, session_id],
        )?;
        Ok(())
    }

    pub fn archive_chat_session(&self, session_id: &str) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let rows = conn.execute(
            "UPDATE chat_sessions SET status = 'archived', archived_at = ?1 WHERE id = ?2 AND status = 'active'",
            params![now, session_id],
        )?;
        Ok(rows > 0)
    }

    pub fn reopen_chat_session(&self, session_id: &str) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let rows = conn.execute(
            "UPDATE chat_sessions SET status = 'active', archived_at = NULL, last_active_at = ?1 WHERE id = ?2 AND status = 'archived'",
            params![now, session_id],
        )?;
        Ok(rows > 0)
    }

    pub fn delete_chat_session(&self, session_id: &str) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        let rows = conn.execute(
            "DELETE FROM chat_sessions WHERE id = ?1",
            params![session_id],
        )?;
        Ok(rows > 0)
    }

    pub fn get_all_messages_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_SELECT_COLS}
             FROM messages
             WHERE session_id = ?1
             ORDER BY timestamp ASC"
        ))?;
        let messages = stmt
            .query_map(params![session_id], stored_message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn get_expired_chat_sessions(&self) -> Result<Vec<ChatSession>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {CHAT_SESSION_SELECT_COLS}
             FROM chat_sessions
             WHERE status = 'active'
               AND ttl_hours > 0
               AND datetime(last_active_at, '+' || ttl_hours || ' hours') < datetime('now')",
        ))?;
        let sessions = stmt
            .query_map([], chat_session_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> (Database, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("finally_a_value_bot_test_{}", uuid::Uuid::new_v4()));
        let db = Database::new(dir.to_str().unwrap()).unwrap();
        (db, dir)
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn test_persona(db: &Database, chat_id: i64) -> i64 {
        db.upsert_chat(chat_id, None, "private").unwrap();
        db.get_or_create_default_persona(chat_id).unwrap()
    }

    #[test]
    fn test_new_database_creates_tables() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 1);
        let msgs = db.get_recent_messages(1, pid, 10, false).unwrap();
        assert!(msgs.is_empty());
        let tasks = db.get_due_tasks("2099-01-01T00:00:00Z").unwrap();
        assert!(tasks.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_upsert_chat_insert_and_update() {
        let (db, dir) = test_db();
        db.upsert_chat(100, Some("Test Chat"), "group").unwrap();
        // Update title
        db.upsert_chat(100, Some("New Title"), "group").unwrap();
        // Insert without title
        db.upsert_chat(200, None, "private").unwrap();
        cleanup(&dir);
    }

    #[test]
    fn test_store_and_retrieve_message() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        let msg = StoredMessage {
            id: "msg1".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "hello".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:00Z".into(),
            origin: crate::db::message_origin_interactive(),
        };
        db.store_message(&msg).unwrap();

        let messages = db.get_recent_messages(100, pid, 10, false).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, "msg1");
        assert_eq!(messages[0].sender_name, "alice");
        assert_eq!(messages[0].content, "hello");
        assert!(!messages[0].is_from_bot);
        cleanup(&dir);
    }

    #[test]
    fn test_store_message_upsert() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        let msg = StoredMessage {
            id: "msg1".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "original".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:00Z".into(),
            origin: crate::db::message_origin_interactive(),
        };
        db.store_message(&msg).unwrap();

        // Store same id again with different content (INSERT OR REPLACE)
        let msg2 = StoredMessage {
            id: "msg1".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "updated".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
            origin: crate::db::message_origin_interactive(),
        };
        db.store_message(&msg2).unwrap();

        let messages = db.get_recent_messages(100, pid, 10, false).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "updated");
        cleanup(&dir);
    }

    #[test]
    fn test_get_recent_messages_ordering_and_limit() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        for i in 0..5 {
            let msg = StoredMessage {
                id: format!("msg{i}"),
                chat_id: 100,
                persona_id: pid,
                session_id: None,
                sender_name: "alice".into(),
                content: format!("message {i}"),
                is_from_bot: false,
                timestamp: format!("2024-01-01T00:00:0{i}Z"),
                origin: crate::db::message_origin_interactive(),
            };
            db.store_message(&msg).unwrap();
        }

        // Limit to 3 - should get the 3 most recent, but reversed to oldest-first
        let messages = db.get_recent_messages(100, pid, 3, false).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "message 2"); // oldest of the 3 most recent
        assert_eq!(messages[1].content, "message 3");
        assert_eq!(messages[2].content, "message 4"); // most recent

        // Different chat_id should be empty
        let pid2 = test_persona(&db, 200);
        let messages = db.get_recent_messages(200, pid2, 10, false).unwrap();
        assert!(messages.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_get_messages_since_last_bot_response_with_bot_msg() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);

        // User message 1
        db.store_message(&StoredMessage {
            id: "m1".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "hi".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        // Bot response
        db.store_message(&StoredMessage {
            id: "m2".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "bot".into(),
            content: "hello!".into(),
            is_from_bot: true,
            timestamp: "2024-01-01T00:00:02Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        // User message 2 (after bot response)
        db.store_message(&StoredMessage {
            id: "m3".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "how are you?".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:03Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        // User message 3
        db.store_message(&StoredMessage {
            id: "m4".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "bob".into(),
            content: "me too".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:04Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        let messages = db
            .get_messages_since_last_bot_response(100, pid, 50, 10, false)
            .unwrap();
        // Should include the bot message and everything after it
        assert!(messages.len() >= 2);
        // First should be the bot msg or after it
        assert_eq!(messages[0].id, "m2"); // the bot message (timestamp >= bot's timestamp)
        assert_eq!(messages[1].id, "m3");
        assert_eq!(messages[2].id, "m4");
        cleanup(&dir);
    }

    #[test]
    fn test_get_messages_since_last_bot_response_no_bot_msg() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);

        for i in 0..5 {
            db.store_message(&StoredMessage {
                id: format!("m{i}"),
                chat_id: 100,
                persona_id: pid,
                session_id: None,
                sender_name: "alice".into(),
                content: format!("msg {i}"),
                is_from_bot: false,
                timestamp: format!("2024-01-01T00:00:0{i}Z"),
                origin: crate::db::message_origin_interactive(),
            })
            .unwrap();
        }

        // Fallback to last 3
        let messages = db
            .get_messages_since_last_bot_response(100, pid, 50, 3, false)
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "msg 2");
        assert_eq!(messages[2].content, "msg 4");
        cleanup(&dir);
    }

    #[test]
    fn test_create_and_get_scheduled_task() {
        let (db, dir) = test_db();
        let persona_id = test_persona(&db, 100);
        let id = db
            .create_scheduled_task(
                100,
                "say hello",
                "cron",
                "0 */5 * * * *",
                "2024-06-01T00:05:00Z",
            )
            .unwrap();
        assert!(id > 0);

        let tasks = db.get_tasks_for_chat(100).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].persona_id, persona_id);
        assert_eq!(tasks[0].prompt, "say hello");
        assert_eq!(tasks[0].schedule_type, "cron");
        assert_eq!(tasks[0].status, "active");
        cleanup(&dir);
    }

    #[test]
    fn test_create_scheduled_task_for_persona_binds_explicit_persona() {
        let (db, dir) = test_db();
        let default_pid = test_persona(&db, 100);
        let alt_pid = db.create_persona(100, "alt", None).unwrap();
        db.set_active_persona(100, default_pid).unwrap();

        let id = db
            .create_scheduled_task_for_persona(
                100,
                alt_pid,
                "run as alt persona",
                "once",
                "2099-12-31T00:00:00Z",
                "2099-12-31T00:00:00Z",
            )
            .unwrap();

        let task = db.get_task_by_id(id).unwrap().unwrap();
        assert_eq!(task.persona_id, alt_pid);
        cleanup(&dir);
    }

    #[test]
    fn test_get_due_tasks() {
        let (db, dir) = test_db();
        db.create_scheduled_task(100, "task1", "cron", "0 * * * * *", "2024-01-01T00:00:00Z")
            .unwrap();
        db.create_scheduled_task(
            100,
            "task2",
            "once",
            "2099-12-31T00:00:00Z",
            "2099-12-31T00:00:00Z",
        )
        .unwrap();

        // Only task1 is due
        let due = db.get_due_tasks("2024-06-01T00:00:00Z").unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].prompt, "task1");

        // Both are due in the far future
        let due = db.get_due_tasks("2100-01-01T00:00:00Z").unwrap();
        assert_eq!(due.len(), 2);
        cleanup(&dir);
    }

    #[test]
    fn test_get_tasks_for_chat_filters_status() {
        let (db, dir) = test_db();
        let id1 = db
            .create_scheduled_task(
                100,
                "active task",
                "cron",
                "0 * * * * *",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        let id2 = db
            .create_scheduled_task(
                100,
                "to cancel",
                "once",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
        db.update_task_status(id2, "cancelled").unwrap();

        // Only active/paused tasks should be returned
        let tasks = db.get_tasks_for_chat(100).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, id1);

        // Pause the active one
        db.update_task_status(id1, "paused").unwrap();
        let tasks = db.get_tasks_for_chat(100).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "paused");
        cleanup(&dir);
    }

    #[test]
    fn test_update_task_status() {
        let (db, dir) = test_db();
        let id = db
            .create_scheduled_task(100, "test", "cron", "0 * * * * *", "2024-01-01T00:00:00Z")
            .unwrap();

        assert!(db.update_task_status(id, "paused").unwrap());
        assert!(db.update_task_status(id, "active").unwrap());
        assert!(db.update_task_status(id, "cancelled").unwrap());

        // Non-existent task
        assert!(!db.update_task_status(9999, "paused").unwrap());
        cleanup(&dir);
    }

    #[test]
    fn test_update_task_after_run_cron() {
        let (db, dir) = test_db();
        let id = db
            .create_scheduled_task(100, "test", "cron", "0 * * * * *", "2024-01-01T00:00:00Z")
            .unwrap();

        db.update_task_after_run(id, "2024-01-01T00:01:00Z", Some("2024-01-01T00:02:00Z"))
            .unwrap();

        let tasks = db.get_tasks_for_chat(100).unwrap();
        assert_eq!(tasks[0].last_run.as_deref(), Some("2024-01-01T00:01:00Z"));
        assert_eq!(tasks[0].next_run, "2024-01-01T00:02:00Z");
        assert_eq!(tasks[0].status, "active");
        cleanup(&dir);
    }

    #[test]
    fn test_update_task_after_run_one_shot() {
        let (db, dir) = test_db();
        let id = db
            .create_scheduled_task(
                100,
                "test",
                "once",
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:00Z",
            )
            .unwrap();

        // One-shot: no next_run, should mark as completed
        db.update_task_after_run(id, "2024-01-01T00:00:00Z", None)
            .unwrap();

        // Should not appear in active/paused list
        let tasks = db.get_tasks_for_chat(100).unwrap();
        assert!(tasks.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_delete_task() {
        let (db, dir) = test_db();
        let id = db
            .create_scheduled_task(100, "test", "cron", "0 * * * * *", "2024-01-01T00:00:00Z")
            .unwrap();

        assert!(db.delete_task(id).unwrap());
        assert!(!db.delete_task(id).unwrap()); // already deleted

        let tasks = db.get_tasks_for_chat(100).unwrap();
        assert!(tasks.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_get_all_messages() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        for i in 0..5 {
            db.store_message(&StoredMessage {
                id: format!("msg{i}"),
                chat_id: 100,
                persona_id: pid,
                session_id: None,
                sender_name: "alice".into(),
                content: format!("message {i}"),
                is_from_bot: false,
                timestamp: format!("2024-01-01T00:00:0{i}Z"),
                origin: crate::db::message_origin_interactive(),
            })
            .unwrap();
        }

        let messages = db.get_all_messages(100, pid).unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].content, "message 0");
        assert_eq!(messages[4].content, "message 4");

        // Different chat should be empty
        let pid2 = test_persona(&db, 200);
        assert!(db.get_all_messages(200, pid2).unwrap().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_log_task_run() {
        let (db, dir) = test_db();
        let task_id = db
            .create_scheduled_task(100, "test", "cron", "0 * * * * *", "2024-01-01T00:00:00Z")
            .unwrap();

        let log_id = db
            .log_task_run(
                task_id,
                100,
                "2024-01-01T00:00:00Z",
                "2024-01-01T00:00:05Z",
                5000,
                true,
                Some("Success"),
            )
            .unwrap();
        assert!(log_id > 0);

        let logs = db.get_task_run_logs(task_id, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].task_id, task_id);
        assert_eq!(logs[0].duration_ms, 5000);
        assert!(logs[0].success);
        assert_eq!(logs[0].result_summary.as_deref(), Some("Success"));
        cleanup(&dir);
    }

    #[test]
    fn test_get_task_run_logs_ordering_and_limit() {
        let (db, dir) = test_db();
        let task_id = db
            .create_scheduled_task(100, "test", "cron", "0 * * * * *", "2024-01-01T00:00:00Z")
            .unwrap();

        for i in 0..5 {
            db.log_task_run(
                task_id,
                100,
                &format!("2024-01-01T00:0{i}:00Z"),
                &format!("2024-01-01T00:0{i}:05Z"),
                5000,
                true,
                Some(&format!("Run {i}")),
            )
            .unwrap();
        }

        // Limit to 3, most recent first
        let logs = db.get_task_run_logs(task_id, 3).unwrap();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].result_summary.as_deref(), Some("Run 4")); // most recent
        assert_eq!(logs[2].result_summary.as_deref(), Some("Run 2"));
        cleanup(&dir);
    }

    #[test]
    fn test_save_and_load_session() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        let json = r#"[{"role":"user","content":"hello"}]"#;
        db.save_session(100, pid, json).unwrap();

        let result = db.load_session(100, pid).unwrap();
        assert!(result.is_some());
        let (loaded_json, updated_at) = result.unwrap();
        assert_eq!(loaded_json, json);
        assert!(!updated_at.is_empty());

        // Upsert: save again with different data
        let json2 = r#"[{"role":"user","content":"hello"},{"role":"assistant","content":"hi"}]"#;
        db.save_session(100, pid, json2).unwrap();
        let (loaded_json2, _) = db.load_session(100, pid).unwrap().unwrap();
        assert_eq!(loaded_json2, json2);

        cleanup(&dir);
    }

    #[test]
    fn test_load_session_nonexistent() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 999);
        let result = db.load_session(999, pid).unwrap();
        assert!(result.is_none());
        cleanup(&dir);
    }

    #[test]
    fn test_delete_session() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        db.save_session(100, pid, "[]").unwrap();
        assert!(db.delete_session(100, pid).unwrap());
        assert!(db.load_session(100, pid).unwrap().is_none());
        // Delete again returns false
        assert!(!db.delete_session(100, pid).unwrap());
        cleanup(&dir);
    }

    #[test]
    fn test_get_new_user_messages_since() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);

        // Messages before the cutoff
        db.store_message(&StoredMessage {
            id: "m1".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "old msg".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        // Bot message at the cutoff
        db.store_message(&StoredMessage {
            id: "m2".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "bot".into(),
            content: "response".into(),
            is_from_bot: true,
            timestamp: "2024-01-01T00:00:02Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        // User messages after cutoff
        db.store_message(&StoredMessage {
            id: "m3".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "new msg 1".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:03Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        db.store_message(&StoredMessage {
            id: "m4".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "bob".into(),
            content: "new msg 2".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:04Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        // Bot message after cutoff (should be excluded - only non-bot)
        db.store_message(&StoredMessage {
            id: "m5".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "bot".into(),
            content: "bot again".into(),
            is_from_bot: true,
            timestamp: "2024-01-01T00:00:05Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();

        let msgs = db
            .get_new_user_messages_since(100, pid, "2024-01-01T00:00:02Z")
            .unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "new msg 1");
        assert_eq!(msgs[1].content, "new msg 2");

        cleanup(&dir);
    }

    #[test]
    fn test_persona_bulletin_events_latest_first() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        db.append_persona_bulletin_event(
            100,
            pid,
            Some("run:1"),
            "memory_update",
            "Updated memory",
            Some("Tiered memory written"),
        )
        .unwrap();
        db.append_persona_bulletin_event(
            100,
            pid,
            Some("run:2"),
            "file_update",
            "Updated file",
            Some("Edited src/main.rs"),
        )
        .unwrap();

        let items = db.list_persona_bulletin_events(100, pid, 1).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].event_type, "file_update");
        assert_eq!(items[0].title, "Updated file");
        cleanup(&dir);
    }

    #[test]
    fn test_persona_bulletin_focus_upsert_and_get() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        db.upsert_persona_bulletin_focus(
            100,
            pid,
            Some("Current focus"),
            "Wing: origin\nRooms: 19",
        )
        .unwrap();
        db.upsert_persona_bulletin_focus(100, pid, None, "Updated focus content")
            .unwrap();
        let focus = db.get_persona_bulletin_focus(100, pid).unwrap().unwrap();
        assert_eq!(focus.chat_id, 100);
        assert_eq!(focus.persona_id, pid);
        assert_eq!(focus.title, None);
        assert_eq!(focus.content, "Updated focus content");
        assert!(!focus.updated_at.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_persona_message_bookmark_crud() {
        let (db, dir) = test_db();
        let pid = test_persona(&db, 100);
        db.store_message(&StoredMessage {
            id: "m-bookmark-1".into(),
            chat_id: 100,
            persona_id: pid,
            session_id: None,
            sender_name: "alice".into(),
            content: "bookmark me".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
            origin: crate::db::message_origin_interactive(),
        })
        .unwrap();
        assert!(db
            .message_exists_in_persona(100, pid, "m-bookmark-1")
            .unwrap());
        assert!(!db.message_exists_in_persona(100, pid, "missing").unwrap());

        db.upsert_persona_message_bookmark(
            100,
            pid,
            "m-bookmark-1",
            "user",
            "bookmark me",
            Some("important"),
        )
        .unwrap();
        db.upsert_persona_message_bookmark(
            100,
            pid,
            "m-bookmark-1",
            "user",
            "bookmark me updated",
            Some("still important"),
        )
        .unwrap();

        let items = db.list_persona_message_bookmarks(100, pid, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].message_id, "m-bookmark-1");
        assert_eq!(items[0].content_preview, "bookmark me updated");
        assert_eq!(items[0].note.as_deref(), Some("still important"));

        assert!(db
            .delete_persona_message_bookmark(100, pid, "m-bookmark-1")
            .unwrap());
        assert!(!db
            .delete_persona_message_bookmark(100, pid, "m-bookmark-1")
            .unwrap());
        cleanup(&dir);
    }

    #[test]
    fn test_builtin_postdelivery_focus_sync_hook_seeded() {
        let (db, dir) = test_db();
        let hooks = db.list_hook_definitions().unwrap();
        let focus_hook = hooks
            .iter()
            .find(|h| h.name == "postdelivery-persona-focus-sync")
            .expect("focus sync hook must be seeded");
        assert_eq!(focus_hook.event_name, "PostDelivery");
        assert_eq!(focus_hook.action_type, "builtin_persona_focus_sync");
        assert!(focus_hook.enabled);
        cleanup(&dir);
    }

    #[test]
    fn test_shipped_hooks_loaded_from_builtin_hooks_catalog() {
        let (db, dir) = test_db();
        let hooks = db.list_hook_definitions().unwrap();
        assert!(
            !hooks.iter().any(|h| h.name.starts_with("template-")),
            "template example hooks must not be seeded"
        );
        assert!(
            !hooks
                .iter()
                .any(|h| h.name == "posttool-pz-terminal-cleanup"),
            "PZ hook must not be shipped in builtin catalog"
        );

        let builtin_names = [
            "postdelivery-persona-focus-sync",
            "beforeturn-scheduler-policy-context",
            "pretool-turn-skill-gate",
            "prestop-deferred-commitment-guard",
            "postbatch-loop-guard",
        ];
        for name in builtin_names {
            let hook = hooks
                .iter()
                .find(|h| h.name == name)
                .unwrap_or_else(|| panic!("missing shipped builtin hook {name}"));
            assert!(
                hook.action_type.starts_with("builtin_"),
                "{} must use builtin_* action_type, got {}",
                name,
                hook.action_type
            );
            assert!(hook.enabled, "{name} should be enabled by default");
            assert!(
                hook.scoped_persona_ids.is_none(),
                "{name} should be globally scoped"
            );
        }
        assert_eq!(
            hooks.len(),
            builtin_names.len(),
            "fresh DB should contain only the five shipped catalog hooks: {:?}",
            hooks.iter().map(|h| &h.name).collect::<Vec<_>>()
        );
        cleanup(&dir);
    }

    #[test]
    fn test_hook_persona_status_fields() {
        let (db, dir) = test_db();
        let chat_id = 9007;
        let owner_persona_id = db
            .create_persona(chat_id, "owner", None)
            .expect("create owner persona");
        let other_persona_id = db
            .create_persona(chat_id, "other", None)
            .expect("create other persona");
        let global_hook_id = db
            .upsert_hook_definition(
                None,
                "global-hook",
                "BeforeTurn",
                None,
                "add_context",
                r#"{"additional_context":"global"}"#,
                None,
                true,
            )
            .expect("upsert global hook");
        let scoped_hook_id = db
            .upsert_hook_definition(
                None,
                "scoped-hook",
                "BeforeTurn",
                None,
                "add_context",
                r#"{"additional_context":"scoped"}"#,
                Some(&[owner_persona_id]),
                true,
            )
            .expect("upsert scoped hook");
        let disabled_hook_id = db
            .upsert_hook_definition(
                None,
                "disabled-hook",
                "BeforeTurn",
                None,
                "add_context",
                r#"{"additional_context":"disabled"}"#,
                None,
                false,
            )
            .expect("upsert disabled hook");

        let hooks = db.list_hook_definitions().expect("list hooks");
        let global_hook = hooks
            .iter()
            .find(|h| h.id == global_hook_id)
            .expect("global hook");
        let scoped_hook = hooks
            .iter()
            .find(|h| h.id == scoped_hook_id)
            .expect("scoped hook");
        let disabled_hook = hooks
            .iter()
            .find(|h| h.id == disabled_hook_id)
            .expect("disabled hook");

        let (scoped, allowed, active) = global_hook
            .persona_status(&db, chat_id, owner_persona_id)
            .expect("global owner status");
        assert!(scoped);
        assert!(allowed);
        assert!(active);

        let (scoped, allowed, active) = scoped_hook
            .persona_status(&db, chat_id, owner_persona_id)
            .expect("scoped owner status");
        assert!(scoped);
        assert!(allowed);
        assert!(active);

        let (scoped, allowed, active) = scoped_hook
            .persona_status(&db, chat_id, other_persona_id)
            .expect("scoped other status");
        assert!(!scoped);
        assert!(allowed);
        assert!(!active);

        db.set_persona_hook_skill_policy(chat_id, owner_persona_id, Some(&[]), None)
            .expect("block all hooks for owner");
        let (_, allowed, active) = global_hook
            .persona_status(&db, chat_id, owner_persona_id)
            .expect("global owner blocked status");
        assert!(!allowed);
        assert!(!active);

        let (_, _, active) = disabled_hook
            .persona_status(&db, chat_id, other_persona_id)
            .expect("disabled status");
        assert!(!active);

        cleanup(&dir);
    }
}
