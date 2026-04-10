use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
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

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub sender_name: String,
    pub content: String,
    pub is_from_bot: bool,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct Persona {
    pub id: i64,
    pub chat_id: i64,
    pub name: String,
    pub model_override: Option<String>,
}

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

#[derive(Debug, Clone)]
pub struct ChannelBinding {
    pub canonical_chat_id: i64,
    pub channel_type: String,
    pub channel_handle: String,
}

#[derive(Debug, Clone)]
pub struct BackgroundJob {
    pub id: String,
    pub chat_id: i64,
    pub persona_id: i64,
    pub prompt: String,
    pub status: String, // "pending", "running", "completed_raw", "main_agent_processing", "done", "failed"
    pub trigger_reason: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub result_text: Option<String>,
    pub error_text: Option<String>,
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
pub struct WorkflowRecord {
    pub id: i64,
    pub owner_chat_id: i64,
    pub intent_signature: String,
    pub steps_json: String,
    pub confidence: f64,
    pub version: i64,
    pub success_count: i64,
    pub failure_count: i64,
    pub last_used_at: Option<String>,
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
                error_text TEXT
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

            CREATE TABLE IF NOT EXISTS project_artifacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                artifact_type TEXT NOT NULL,
                artifact_ref TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                updated_at TEXT NOT NULL,
                UNIQUE(project_id, artifact_type, artifact_ref)
            );
            CREATE INDEX IF NOT EXISTS idx_project_artifacts_project
                ON project_artifacts(project_id, updated_at DESC);

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
                ON channel_bindings(canonical_chat_id);",
        )?;

        Self::migrate_persona_schema(&conn)?;
        Self::migrate_scheduled_tasks_persona_schema(&conn)?;
        Self::migrate_channel_bindings(&conn)?;
        Self::migrate_fts(&conn)?;
        Self::migrate_cursor_agent_runs_tmux(&conn)?;

        Ok(Database {
            conn: Mutex::new(conn),
        })
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
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                msg.id,
                msg.chat_id,
                msg.persona_id,
                msg.sender_name,
                msg.content,
                msg.is_from_bot as i32,
                msg.timestamp,
            ],
        )?;
        Ok(())
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
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
             ORDER BY timestamp DESC
             LIMIT ?3",
        )?;

        let messages = stmt
            .query_map(params![chat_id, persona_id, limit as i64], |row| {
                Ok(StoredMessage {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    sender_name: row.get(3)?,
                    content: row.get(4)?,
                    is_from_bot: row.get::<_, i32>(5)? != 0,
                    timestamp: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Reverse so oldest first
        let mut messages = messages;
        messages.reverse();
        Ok(messages)
    }

    pub fn get_all_messages(
        &self,
        chat_id: i64,
        persona_id: i64,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
             ORDER BY timestamp ASC",
        )?;
        let messages = stmt
            .query_map(params![chat_id, persona_id], |row| {
                Ok(StoredMessage {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    sender_name: row.get(3)?,
                    content: row.get(4)?,
                    is_from_bot: row.get::<_, i32>(5)? != 0,
                    timestamp: row.get(6)?,
                })
            })?
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
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
               AND (?3 IS NULL OR timestamp >= ?3)
               AND (?4 IS NULL OR timestamp <= ?4)
             ORDER BY timestamp ASC
             LIMIT ?5",
        )?;
        let messages = stmt
            .query_map(
                params![chat_id, persona_id, from_date, to_date, limit as i64],
                |row| {
                    Ok(StoredMessage {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        persona_id: row.get(2)?,
                        sender_name: row.get(3)?,
                        content: row.get(4)?,
                        is_from_bot: row.get::<_, i32>(5)? != 0,
                        timestamp: row.get(6)?,
                    })
                },
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
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date(timestamp) AS d FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2
             ORDER BY d DESC",
        )?;
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

    /// Resolve (channel_type, channel_handle) to canonical_chat_id. If no binding exists, creates one:
    /// - telegram/discord: use handle (as i64) as canonical_chat_id, ensure chat exists, insert binding.
    /// - web: use create_with_canonical_id as the new canonical (caller provides e.g. hash-based id), ensure chat exists, insert binding.
    pub fn resolve_canonical_chat_id(
        &self,
        channel_type: &str,
        channel_handle: &str,
        create_with_canonical_id: Option<i64>,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        if let Some(canonical) = conn
            .query_row(
                "SELECT canonical_chat_id FROM channel_bindings WHERE channel_type = ?1 AND channel_handle = ?2",
                params![channel_type, channel_handle],
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
            "INSERT OR REPLACE INTO channel_bindings (canonical_chat_id, channel_type, channel_handle) VALUES (?1, ?2, ?3)",
            params![canonical, channel_type, channel_handle],
        )?;
        Ok(canonical)
    }

    /// Add a binding from (channel_type, channel_handle) to canonical_chat_id. If that (type, handle) already exists, updates to this contact.
    pub fn link_channel(
        &self,
        canonical_chat_id: i64,
        channel_type: &str,
        channel_handle: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO channel_bindings (canonical_chat_id, channel_type, channel_handle) VALUES (?1, ?2, ?3)",
            params![canonical_chat_id, channel_type, channel_handle],
        )?;
        Ok(())
    }

    /// Remove the binding for (channel_type, channel_handle).
    pub fn unlink_channel(
        &self,
        channel_type: &str,
        channel_handle: &str,
    ) -> Result<bool, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM channel_bindings WHERE channel_type = ?1 AND channel_handle = ?2",
            params![channel_type, channel_handle],
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
            "SELECT canonical_chat_id, channel_type, channel_handle FROM channel_bindings WHERE canonical_chat_id = ?1",
        )?;
        let rows = stmt.query_map(params![canonical_chat_id], |row| {
            Ok(ChannelBinding {
                canonical_chat_id: row.get(0)?,
                channel_type: row.get(1)?,
                channel_handle: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get messages since the bot's last response in this chat/persona.
    /// Falls back to `fallback_limit` most recent messages if bot never responded.
    pub fn get_messages_since_last_bot_response(
        &self,
        chat_id: i64,
        persona_id: i64,
        max: usize,
        fallback: usize,
    ) -> Result<Vec<StoredMessage>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();

        // Find timestamp of last bot message
        let last_bot_ts: Option<String> = conn
            .query_row(
                "SELECT timestamp FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2 AND is_from_bot = 1
                 ORDER BY timestamp DESC LIMIT 1",
                params![chat_id, persona_id],
                |row| row.get(0),
            )
            .ok();

        let mut messages = if let Some(ts) = last_bot_ts {
            let mut stmt = conn.prepare(
                "SELECT id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp
                 FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2 AND timestamp >= ?3
                 ORDER BY timestamp DESC
                 LIMIT ?4",
            )?;
            let rows = stmt
                .query_map(params![chat_id, persona_id, ts, max as i64], |row| {
                    Ok(StoredMessage {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        persona_id: row.get(2)?,
                        sender_name: row.get(3)?,
                        content: row.get(4)?,
                        is_from_bot: row.get::<_, i32>(5)? != 0,
                        timestamp: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp
                 FROM messages
                 WHERE chat_id = ?1 AND persona_id = ?2
                 ORDER BY timestamp DESC
                 LIMIT ?3",
            )?;
            let rows = stmt
                .query_map(params![chat_id, persona_id, fallback as i64], |row| {
                    Ok(StoredMessage {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        persona_id: row.get(2)?,
                        sender_name: row.get(3)?,
                        content: row.get(4)?,
                        is_from_bot: row.get::<_, i32>(5)? != 0,
                        timestamp: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
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
            .query_map(params![now], |row| {
                Ok(ScheduledTask {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    prompt: row.get(3)?,
                    schedule_type: row.get(4)?,
                    schedule_value: row.get(5)?,
                    next_run: row.get(6)?,
                    last_run: row.get(7)?,
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
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
            .query_map([], |row| {
                Ok(ScheduledTask {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    prompt: row.get(3)?,
                    schedule_type: row.get(4)?,
                    schedule_value: row.get(5)?,
                    next_run: row.get(6)?,
                    last_run: row.get(7)?,
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
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
            .query_map([], |row| {
                Ok(ScheduledTask {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    prompt: row.get(3)?,
                    schedule_type: row.get(4)?,
                    schedule_value: row.get(5)?,
                    next_run: row.get(6)?,
                    last_run: row.get(7)?,
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
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
            .query_map(params![chat_id], |row| {
                Ok(ScheduledTask {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    prompt: row.get(3)?,
                    schedule_type: row.get(4)?,
                    schedule_value: row.get(5)?,
                    next_run: row.get(6)?,
                    last_run: row.get(7)?,
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
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
            |row| {
                Ok(ScheduledTask {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    prompt: row.get(3)?,
                    schedule_type: row.get(4)?,
                    schedule_value: row.get(5)?,
                    next_run: row.get(6)?,
                    last_run: row.get(7)?,
                    status: row.get(8)?,
                    created_at: row.get(9)?,
                })
            },
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

    pub fn upsert_project_artifact(
        &self,
        project_id: i64,
        artifact_type: &str,
        artifact_ref: &str,
        metadata_json: Option<&str>,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_artifacts (project_id, artifact_type, artifact_ref, metadata_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id, artifact_type, artifact_ref) DO UPDATE SET
               metadata_json = excluded.metadata_json,
               updated_at = excluded.updated_at",
            params![
                project_id,
                artifact_type,
                artifact_ref,
                metadata_json.unwrap_or("{}"),
                now
            ],
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

    pub fn get_best_workflow_for_intent(
        &self,
        owner_chat_id: i64,
        intent_signature: &str,
        min_confidence: f64,
    ) -> Result<Option<WorkflowRecord>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, owner_chat_id, intent_signature, steps_json, confidence, version, success_count, failure_count, last_used_at, updated_at
             FROM workflows
             WHERE owner_chat_id = ?1
               AND intent_signature = ?2
               AND confidence >= ?3
             ORDER BY confidence DESC, updated_at DESC
             LIMIT 1",
            params![owner_chat_id, intent_signature, min_confidence],
            |row| {
                Ok(WorkflowRecord {
                    id: row.get(0)?,
                    owner_chat_id: row.get(1)?,
                    intent_signature: row.get(2)?,
                    steps_json: row.get(3)?,
                    confidence: row.get(4)?,
                    version: row.get(5)?,
                    success_count: row.get(6)?,
                    failure_count: row.get(7)?,
                    last_used_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            },
        );
        match result {
            Ok(wf) => Ok(Some(wf)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn upsert_workflow_learning(
        &self,
        owner_chat_id: i64,
        intent_signature: &str,
        steps_json: &str,
        success: bool,
        score: f64,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO workflows (
                owner_chat_id, intent_signature, steps_json, confidence, version, success_count, failure_count, last_used_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, CASE WHEN ?4 THEN ?5 ELSE 0.0 END, 1,
                CASE WHEN ?4 THEN 1 ELSE 0 END,
                CASE WHEN ?4 THEN 0 ELSE 1 END,
                ?6, ?6
             )
             ON CONFLICT(owner_chat_id, intent_signature) DO UPDATE SET
               steps_json = excluded.steps_json,
               success_count = workflows.success_count + CASE WHEN ?4 THEN 1 ELSE 0 END,
               failure_count = workflows.failure_count + CASE WHEN ?4 THEN 0 ELSE 1 END,
               confidence = MIN(
                   1.0,
                   MAX(
                       0.0,
                       (workflows.confidence * 0.7) + (CASE WHEN ?4 THEN ?5 ELSE 0.0 END * 0.3)
                   )
               ),
               version = workflows.version + 1,
               updated_at = excluded.updated_at",
            params![owner_chat_id, intent_signature, steps_json, success, score, now],
        )?;
        let id = conn.query_row(
            "SELECT id FROM workflows WHERE owner_chat_id = ?1 AND intent_signature = ?2",
            params![owner_chat_id, intent_signature],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(id)
    }

    pub fn log_workflow_execution(
        &self,
        workflow_id: i64,
        run_key: &str,
        outcome: &str,
        score: f64,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO workflow_executions (workflow_id, run_key, outcome, score, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![workflow_id, run_key, outcome, score, now],
        )?;
        conn.execute(
            "UPDATE workflows SET last_used_at = ?1 WHERE id = ?2",
            params![now, workflow_id],
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
            "INSERT INTO background_jobs (id, chat_id, persona_id, prompt, status, trigger_reason, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6)",
            params![id, chat_id, persona_id, prompt, trigger_reason, now],
        )?;
        Ok(())
    }

    pub fn mark_background_job_running(&self, id: &str) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE background_jobs SET status = 'running', started_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn mark_background_job_completed_raw(
        &self,
        id: &str,
        result_text: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE background_jobs
             SET status = 'completed_raw', finished_at = ?1, result_text = ?2
             WHERE id = ?3",
            params![now, result_text, id],
        )?;
        Ok(())
    }

    pub fn mark_background_job_main_agent_processing(
        &self,
        id: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE background_jobs SET status = 'main_agent_processing' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_background_job_done(&self, id: &str) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE background_jobs SET status = 'done' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn fail_background_job(
        &self,
        id: &str,
        error_text: &str,
    ) -> Result<(), FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE background_jobs SET status = 'failed', finished_at = ?1, error_text = ?2 WHERE id = ?3",
            params![now, error_text, id],
        )?;
        Ok(())
    }

    pub fn count_active_background_jobs_for_chat(
        &self,
        chat_id: i64,
    ) -> Result<i64, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let count = conn.query_row(
            "SELECT COUNT(*) FROM background_jobs
             WHERE chat_id = ?1
               AND status IN ('pending', 'running', 'completed_raw', 'main_agent_processing')",
            params![chat_id],
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
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, prompt, status, trigger_reason, created_at, started_at, finished_at, result_text, error_text
             FROM background_jobs
             WHERE chat_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;
        let jobs = stmt
            .query_map(params![chat_id, limit as i64], |row| {
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn get_background_job(
        &self,
        id: &str,
    ) -> Result<Option<BackgroundJob>, FinallyAValueBotError> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, chat_id, persona_id, prompt, status, trigger_reason, created_at, started_at, finished_at, result_text, error_text
             FROM background_jobs WHERE id = ?1",
            params![id],
            |row| {
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
                })
            },
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
                     SET status = 'failed', finished_at = ?1, error_text = ?2
                     WHERE id = ?3
                       AND status IN ('pending', 'running', 'completed_raw', 'main_agent_processing')",
                    params![now_str, stale_msg, run_key],
                );
            }
            reconciled.push(run_key);
        }

        Ok(reconciled)
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
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
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
                 SET status = 'failed', finished_at = ?1, error_text = ?2
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
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, persona_id, sender_name, content, is_from_bot, timestamp
             FROM messages
             WHERE chat_id = ?1 AND persona_id = ?2 AND timestamp > ?3 AND is_from_bot = 0
             ORDER BY timestamp ASC",
        )?;
        let messages = stmt
            .query_map(params![chat_id, persona_id, since], |row| {
                Ok(StoredMessage {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    persona_id: row.get(2)?,
                    sender_name: row.get(3)?,
                    content: row.get(4)?,
                    is_from_bot: row.get::<_, i32>(5)? != 0,
                    timestamp: row.get(6)?,
                })
            })?
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
            "SELECT id, chat_id, name, model_override FROM personas WHERE chat_id = ?1 ORDER BY id",
        )?;
        let personas = stmt
            .query_map(params![chat_id], |row| {
                Ok(Persona {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    name: row.get(2)?,
                    model_override: row.get(3)?,
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
            "SELECT id, chat_id, name, model_override FROM personas WHERE chat_id = ?1 AND name = ?2",
            params![chat_id, name],
            |row| {
                Ok(Persona {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    name: row.get(2)?,
                    model_override: row.get(3)?,
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
            "SELECT id, chat_id, name, model_override FROM personas WHERE id = ?1",
            params![id],
            |row| {
                Ok(Persona {
                    id: row.get(0)?,
                    chat_id: row.get(1)?,
                    name: row.get(2)?,
                    model_override: row.get(3)?,
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
            "SELECT m.id, m.chat_id, m.persona_id, m.sender_name, m.content, m.is_from_bot, m.timestamp
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
                |row| {
                    Ok(StoredMessage {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        persona_id: row.get(2)?,
                        sender_name: row.get(3)?,
                        content: row.get(4)?,
                        is_from_bot: row.get::<_, i32>(5)? != 0,
                        timestamp: row.get(6)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
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
        let msgs = db.get_recent_messages(1, pid, 10).unwrap();
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
            sender_name: "alice".into(),
            content: "hello".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:00Z".into(),
        };
        db.store_message(&msg).unwrap();

        let messages = db.get_recent_messages(100, pid, 10).unwrap();
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
            sender_name: "alice".into(),
            content: "original".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:00Z".into(),
        };
        db.store_message(&msg).unwrap();

        // Store same id again with different content (INSERT OR REPLACE)
        let msg2 = StoredMessage {
            id: "msg1".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "alice".into(),
            content: "updated".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
        };
        db.store_message(&msg2).unwrap();

        let messages = db.get_recent_messages(100, pid, 10).unwrap();
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
                sender_name: "alice".into(),
                content: format!("message {i}"),
                is_from_bot: false,
                timestamp: format!("2024-01-01T00:00:0{i}Z"),
            };
            db.store_message(&msg).unwrap();
        }

        // Limit to 3 - should get the 3 most recent, but reversed to oldest-first
        let messages = db.get_recent_messages(100, pid, 3).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "message 2"); // oldest of the 3 most recent
        assert_eq!(messages[1].content, "message 3");
        assert_eq!(messages[2].content, "message 4"); // most recent

        // Different chat_id should be empty
        let pid2 = test_persona(&db, 200);
        let messages = db.get_recent_messages(200, pid2, 10).unwrap();
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
            sender_name: "alice".into(),
            content: "hi".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
        })
        .unwrap();

        // Bot response
        db.store_message(&StoredMessage {
            id: "m2".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "bot".into(),
            content: "hello!".into(),
            is_from_bot: true,
            timestamp: "2024-01-01T00:00:02Z".into(),
        })
        .unwrap();

        // User message 2 (after bot response)
        db.store_message(&StoredMessage {
            id: "m3".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "alice".into(),
            content: "how are you?".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:03Z".into(),
        })
        .unwrap();

        // User message 3
        db.store_message(&StoredMessage {
            id: "m4".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "bob".into(),
            content: "me too".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:04Z".into(),
        })
        .unwrap();

        let messages = db
            .get_messages_since_last_bot_response(100, pid, 50, 10)
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
                sender_name: "alice".into(),
                content: format!("msg {i}"),
                is_from_bot: false,
                timestamp: format!("2024-01-01T00:00:0{i}Z"),
            })
            .unwrap();
        }

        // Fallback to last 3
        let messages = db
            .get_messages_since_last_bot_response(100, pid, 50, 3)
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
                sender_name: "alice".into(),
                content: format!("message {i}"),
                is_from_bot: false,
                timestamp: format!("2024-01-01T00:00:0{i}Z"),
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
            sender_name: "alice".into(),
            content: "old msg".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:01Z".into(),
        })
        .unwrap();

        // Bot message at the cutoff
        db.store_message(&StoredMessage {
            id: "m2".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "bot".into(),
            content: "response".into(),
            is_from_bot: true,
            timestamp: "2024-01-01T00:00:02Z".into(),
        })
        .unwrap();

        // User messages after cutoff
        db.store_message(&StoredMessage {
            id: "m3".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "alice".into(),
            content: "new msg 1".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:03Z".into(),
        })
        .unwrap();

        db.store_message(&StoredMessage {
            id: "m4".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "bob".into(),
            content: "new msg 2".into(),
            is_from_bot: false,
            timestamp: "2024-01-01T00:00:04Z".into(),
        })
        .unwrap();

        // Bot message after cutoff (should be excluded - only non-bot)
        db.store_message(&StoredMessage {
            id: "m5".into(),
            chat_id: 100,
            persona_id: pid,
            sender_name: "bot".into(),
            content: "bot again".into(),
            is_from_bot: true,
            timestamp: "2024-01-01T00:00:05Z".into(),
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
}
