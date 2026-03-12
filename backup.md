# AGENTS.md

## Project overview

FinallyAValueBot is a Rust Telegram bot that connects Claude AI to Telegram chats with agentic tool execution, web browsing, scheduled tasks, and persistent memory. Inspired by [nanoclaw](https://github.com/gavrielc/nanoclaw/) (TypeScript/WhatsApp), incorporating some of its design ideas and using Telegram as the messaging platform.

## Tech stack

- **Language:** Rust (2021 edition)
- **Async runtime:** Tokio
- **Telegram:** teloxide 0.17
- **AI:** Anthropic Messages API via reqwest (direct HTTP, no SDK)
- **Database:** SQLite via rusqlite (bundled)
- **Serialization:** serde + serde_json
- **Scheduling:** cron 0.13 (6-field cron expressions)
- **Web:** reqwest for HTTP, regex for HTML parsing, urlencoding for query params

## Project structure

```
src/
    main.rs          -- Entry point. Initializes config, DB, memory manager, starts bot.
    config.rs        -- Loads all settings from finally-a-value-bot.config.yaml.
    error.rs         -- FinallyAValueBotError enum (thiserror). All error variants for the app.
    telegram.rs      -- Telegram message handler. Contains the agentic tool-use loop
                        (process_with_claude), session resume (load/save full message
                        state), context compaction (summarize old messages), continuous
                        typing indicator, group chat catch-up, and response splitting.
    claude.rs        -- Anthropic Messages API client. Request/response types, HTTP calls
                        with retry on 429.
    db.rs            -- SQLite database. Four tables: chats, messages, scheduled_tasks,
                        sessions. Uses Mutex<Connection> for thread safety. Shared as
                        Arc<Database>.
    memory.rs        -- MemoryManager. Reads/writes AGENTS.md files at global and per-chat
                        scopes. Builds memory context injected into system prompts.
    scheduler.rs     -- Background scheduler. Spawns a tokio task that polls every 60s
                        for due tasks, executes the agent loop, sends results to chat.
    tools/
        mod.rs       -- Tool trait (async_trait), ToolRegistry, ToolResult type.
                        Registry constructor takes (Config, Bot, Arc<Database>).
                        17+ tools registered total. new_sub_agent() creates restricted
                        registry with 9 tools (no side-effect or recursive tools).
        bash.rs      -- Executes shell commands via tokio::process::Command.
        read_file.rs -- Reads files with line numbers, offset/limit support.
        write_file.rs-- Writes files, auto-creates parent directories.
        edit_file.rs -- Find/replace editing. Validates old_string is unique.
        glob.rs      -- File pattern matching via the glob crate.
        grep.rs      -- Recursive regex search with directory traversal.
        memory.rs    -- read_memory / write_memory tools for AGENTS.md persistence.
        web_search.rs-- DuckDuckGo HTML search. GET html.duckduckgo.com/html/?q=...,
                        regex parse result__a (links) and result__snippet (descriptions).
        web_fetch.rs -- Fetch URL, strip HTML tags via regex, return plain text (max 20KB).
        browser.rs   -- Browser automation via agent-browser CLI. Uses per-chat sessions
                        and profiles.
        send_message.rs -- Send Telegram message mid-conversation. Holds Bot instance.
                           Chat ID passed via tool input (system prompt tells Claude the ID).
        schedule.rs  -- 5 scheduling tools: schedule_task, list_scheduled_tasks,
                        pause_scheduled_task, resume_scheduled_task, cancel_scheduled_task.
                        Each holds Arc<Database>.
        social_feed.rs -- fetch_tiktok_feed, fetch_instagram_feed, fetch_linkedin_feed.
                         Fetches user feeds via official APIs. Own-feed requires one-time
                         OAuth per user; registered only when social config enables each platform.
        sub_agent.rs -- Sub-agent tool. Spawns a fresh agentic loop with restricted
                        tools (9 tools: bash, file ops, glob, grep, web, read_memory).
                        No send_message, write_memory, schedule, or recursive sub_agent.
```

## Key patterns

### Agentic tool-use loop (`telegram.rs:process_with_claude`)

The core loop:
1. Try loading saved session (full `Vec<Message>` with tool blocks) from `sessions` table
   - If session exists: deserialize, append new user messages since `updated_at`
   - If no session: fall back to DB history:
     - Private chats: last N messages (`get_recent_messages`)
     - Groups: all messages since last bot response (`get_messages_since_last_bot_response`)
2. Build system prompt with memory context and chat_id
3. If `override_prompt` is set (from scheduler), append as user message
4. Compact if messages exceed `max_session_messages` (summarize old messages via Claude, keep recent verbatim)
5. Call Claude API with tool definitions
6. If `stop_reason == "tool_use"` -> execute tools -> append results -> loop back to step 5
7. If `stop_reason == "end_turn"` -> extract text -> strip image base64 -> save session -> return
8. Loop up to `max_tool_iterations` times

### Session resume (`db.rs` sessions table + `telegram.rs`)

Full conversation state (including tool_use and tool_result blocks) is serialized to JSON and persisted in the `sessions` table after each agentic loop. On the next invocation, the session is loaded and new user messages are appended. Image base64 data is stripped before saving to avoid bloat. Send `/reset` to clear a session.

### Context compaction (`telegram.rs:compact_messages`)

When session message count exceeds `max_session_messages` (default 40):
1. Split messages into old (to summarize) and recent (to keep, default 20)
2. Call Claude with a summarization prompt (no tools)
3. Replace old messages with `[Conversation Summary]` + assistant ack
4. Append recent messages with role alternation fix
5. On API failure: fall back to simple truncation (discard old, keep recent)

### Sub-agent (`tools/sub_agent.rs`)

The `sub_agent` tool spawns an independent agentic loop (max 10 iterations) with a restricted `ToolRegistry` (9 tools). Excluded: send_message, write_memory, schedule tools, export_chat, sub_agent (prevents recursion). Used for delegating self-contained research or coding tasks.

### Typing indicator (`telegram.rs:handle_message`)

A `tokio::spawn` task sends `ChatAction::Typing` every 4 seconds. The handle is `abort()`ed when processing completes. This keeps the typing indicator visible for the entire duration of multi-tool interactions.

### Scheduler (`scheduler.rs`)

Spawned in `run_bot()` as a background task:
1. Sleep 60 seconds
2. Query `scheduled_tasks WHERE status='active' AND next_run <= now`
3. For each due task, call `process_with_claude(state, chat_id, "scheduler", "private", Some(prompt))`
4. Send response to chat
5. Update task: for cron tasks compute next_run, for one-shot tasks set status='completed'

### Tool system (`tools/mod.rs`)

All tools implement the `Tool` trait:
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: serde_json::Value) -> ToolResult;
}
```

`ToolRegistry` holds all tools and dispatches execution by name. Tool definitions are passed to Claude as JSON Schema.

Constructor signatures:
- `ToolRegistry::new(config: &Config, bot: Bot, db: Arc<Database>)` -- full registry (17 tools)
- `ToolRegistry::new_sub_agent(config: &Config)` -- restricted registry (9 tools, no side-effects/recursion)

### Memory system (`memory.rs`)

Two scopes:
- **Global:** `data/groups/AGENTS.md` -- shared across all chats
- **Per-chat:** `data/groups/{chat_id}/AGENTS.md` -- specific to one conversation
- **Daily log (optional):** `data/groups/{chat_id}/memory/YYYY-MM-DD.md` -- append-only; today and yesterday are injected as `<recent_context>` at session start. Use `write_memory` with scope `chat_daily` to append.

Memory content is injected into the system prompt wrapped in `<global_memory>` / `<chat_memory>` XML tags. Claude reads/writes memory via the `read_memory` and `write_memory` tools.

**Memory persistence:** Memory is file-based and survives session resets. `/reset` (and API reset) clear only the conversation transcript; AGENTS.md and daily logs are unchanged. To keep something across resets, ask the bot to remember it (e.g. "remember this") so it uses `write_memory`. Before context compaction, the bot runs a silent turn that prompts it to write important facts to memory so they are not lost when old messages are summarized.

### Social media feed tools (`tools/social_feed.rs`)

Three tools: `fetch_tiktok_feed`, `fetch_instagram_feed`, `fetch_linkedin_feed`. Each fetches the user's own feed via official platform APIs. Requires one-time OAuth authorization per user per platform.

- **Setup:** Add `social` block to `finally-a-value-bot.config.yaml` with `base_url` (reachable for OAuth callbacks) and per-platform `client_id`/`client_secret`. Register apps at TikTok/Instagram/Meta/LinkedIn developer portals.
- **OAuth flow:** When the user asks for their feed and no token exists, the tool returns an authorize URL. User clicks, authorizes on the platform, and is redirected to `/api/oauth/callback/{platform}`. Tokens are stored in `social_oauth_tokens` table.
- **Limitations:** Only own-feed is supported (public profile by username is not available in these APIs). X (Twitter) is excluded due to paid API requirements.

### Database (`db.rs`)

Tables include:
- `chats` -- chat metadata (id, title, type, last message time)
- `messages` -- all messages (id, chat_id, sender, content, is_from_bot, timestamp)
- `scheduled_tasks` -- scheduled tasks (id, chat_id, prompt, schedule_type, schedule_value, next_run, last_run, status, created_at)
- `sessions` -- session state (chat_id PK, messages_json, updated_at) for session resume
- `social_oauth_tokens` -- OAuth tokens per platform per chat (platform, chat_id PK, access_token, refresh_token, expires_at)
- `oauth_pending_states` -- short-lived mapping for OAuth state param (state_token PK, platform, chat_id, expires_at)

Uses WAL mode for performance. `Database` struct wraps `Mutex<Connection>`, shared as `Arc<Database>`.

### Claude API (`claude.rs`)

Direct HTTP to `https://api.anthropic.com/v1/messages` with:
- `x-api-key` header for auth
- `anthropic-version: 2023-06-01` header
- Exponential backoff retry on HTTP 429 (up to 3 attempts)
- Content blocks use tagged enums: `Text`, `ToolUse`, `ToolResult`

### Message handling (`telegram.rs`)

- **Private chats:** always respond
- **Groups:** only respond when `@bot_username` is mentioned
- All messages are stored regardless of whether the bot responds
- Consecutive same-role messages are merged before sending to Claude
- Responses over 4096 chars are split at newline boundaries
- Empty responses are not sent (agent may have used send_message tool)

## Build and run

```sh
cargo build              # dev build
cargo build --release    # release build
cargo run -- start       # run (requires finally-a-value-bot.config.yaml)
cargo run -- help        # CLI help
```

Requires a `finally-a-value-bot.config.yaml` with `telegram_bot_token`, `api_key`, and `bot_username`.

## Adding a new tool

1. Create `src/tools/my_tool.rs`
2. Implement the `Tool` trait (name, definition with JSON Schema, execute)
3. Add `pub mod my_tool;` to `src/tools/mod.rs`
4. Register it in `ToolRegistry::new()` with `Box::new(my_tool::MyTool::new(...))`
5. If the tool needs `Bot` or `Arc<Database>`, add a constructor that accepts them

## Common tasks

- **Change the model:** set `model` in `finally-a-value-bot.config.yaml`
- **Increase context:** set `max_history_messages` higher in `finally-a-value-bot.config.yaml` (costs more tokens)
- **Increase tool iterations:** set `max_tool_iterations` higher in `finally-a-value-bot.config.yaml` (default: 100)
- **Debug logging:** run with `RUST_LOG=debug cargo run -- start`
- **Reset memory:** delete files under `finally-a-value-bot.data/runtime/groups/`
- **Reset all data:** delete the `finally-a-value-bot.data/` directory
- **Cancel all tasks:** `sqlite3 finally-a-value-bot.data/runtime/finally-a-value-bot.db "UPDATE scheduled_tasks SET status='cancelled' WHERE status='active';"`
- **Tune compaction:** set `max_session_messages` (default 40) and `compact_keep_recent` (default 20) in `finally-a-value-bot.config.yaml`
- **Reset a chat session:** send `/reset` in chat, or `sqlite3 finally-a-value-bot.data/runtime/finally-a-value-bot.db "DELETE FROM sessions WHERE chat_id=XXXX;"` — the bot replies that stored memory (AGENTS.md) is unchanged. Session = chat history only; the workspace (tools, files, builds under `working_dir`) is persistent by default.
- **Make the bot remember something:** say e.g. "remember this" or "save this to memory" so it uses the `write_memory` tool; that content then persists across resets and restarts.
- **Workspace:** File/bash/search tools use a single shared directory (`working_dir/shared`). Tools and builds there persist regardless of session or /reset; there is no per-chat workspace.
- **Making new sessions aware of workspace tools:** The bot injects `working_dir/shared/WORKSPACE.md` and `working_dir/shared/TOOLS.md` into the system prompt at session start (if present). Document your custom scripts, tools, and rules there so every new session knows they exist and how to use them. You can also use `write_memory` to record tool descriptions; the bot is instructed to update these when it creates new tools.
- **Social media feeds:** To fetch TikTok, Instagram, or LinkedIn feeds, add a `social` block in config with `base_url` and each platform's `client_id`/`client_secret`. Users must authorize once per platform via the OAuth link the bot provides when they first request their feed.