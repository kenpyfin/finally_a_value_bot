# Lessons Learned

Running log of incidents, root causes, and durable fixes. Newest first. Each entry should let a future reader avoid repeating the same mistake without re-doing the investigation.

Template:

```
### YYYY-MM-DD — Short title
- **Symptom:** what was observed
- **Root cause:** the underlying reason
- **Fix:** what resolved it (data and/or code)
- **Prevention:** how to avoid recurrence
- **Files/refs:** key paths, symbols, log signatures
```

---

### 2026-08-21 — Interactive runs hung with no reply / orphan Cursor bridges

- **Symptom:** User messages on PZ, selling_oversea, Videographer stayed unanswered for 40+ minutes to hours; timeline showed `run_started` / tools / sometimes `quality_eval_started` but never `run_finished`. Many hours-old `cursor-sdk-bridge.js` processes; sidecar logged `Cannot write to closing transport` and bridge discovery timeouts.
- **Root cause:** (1) Finish path could hang in PDQE setup or focus sync with no wall clock, so delivery + `run_finished` never ran; (2) queue hard timeout aborted the task without a user-visible notice; (3) warm bridges for `main` stayed pooled, but client disconnect / retries left OS bridge processes outside `_POOL` that the idle reaper never saw.
- **Fix:** Hard-abort hooks deliver a notice + `run_finished`; PDQE/focus-sync wall timeouts; await `run_finished` before completing the turn; sidecar orphan PID sweeper + disconnect eviction; tighter default idle TTL (600s) and pool max (16).
- **Prevention:** Never return from finish without `run_finished`; never leave `/run` client disconnect without pool eviction; alert when `/health` `os_bridge_pids` ≫ `persona_bridges_active`.
- **Files/refs:** `src/queue_abort.rs`; `src/chat_queue.rs` (`QUEUED_TASK_HARD_TIMEOUT`); `finish_turn_with_quality_gate` in `src/channels/telegram.rs`; `scripts/cursor-sdk-runner.py` (`_kill_orphan_bridge_processes`, `handle_run`).

### 2026-08-20 — WeCom stopped receiving after late Cursor replies

- **Symptom:** After `(Cursor agent completed with no text output.)` (or other long Cursor turns), group @-mentions no longer produced `aibot_msg_callback` frames; DM sometimes still worked briefly, then silence for hours with no reconnect log.
- **Root cause:** WeCom long-connection requires a timely `aibot_respond_msg` stream on the callback `req_id`. The gateway waited for the agent (tens of seconds to minutes) then responded with a non-stream markdown `aibot_respond_msg`, missing the window and stalling inbound delivery.
- **Fix:** Immediately ack with stream `finish=false`, finish with `finish=true` within 9 minutes, otherwise `aibot_send_msg`. Add pong-stale reconnect.
- **Prevention:** Never hold a WeCom callback open for a full agent run without a stream placeholder. Prefer `aibot_send_msg` for async completions after the stream window.
- **Files/refs:** `src/channels/wecom_aibot.rs` (`begin_stream_reply`); official doc path/101463 (10-minute stream finish limit).

### 2026-08-20 — Cursor resume turn delivered empty placeholder to WeCom

- **Symptom:** WeCom group replies showed `(Cursor agent completed with no text output.)` after a Cursor engine run; web history stored the same placeholder. Later retries sometimes produced real text.
- **Root cause:** Cursor SDK resume turns can end with tool activity and no assistant text block / empty `result`. The engine substituted a placeholder and delivered immediately with no retry.
- **Fix:** On empty assistant text, nudge the resumed agent (same budget as deferred-commitment nudges) to write a short chat reply; if still empty, recover short non-JSON tool result text; only then keep the placeholder. Harden sidecar text extraction.
- **Prevention:** Never treat empty Cursor finals as done when a resume agent id exists and nudge budget remains. Keep placeholder as last resort only.
- **Files/refs:** `src/cursor_engine.rs` (`CURSOR_EMPTY_OUTPUT_PLACEHOLDER`, empty-output nudge); `scripts/cursor-sdk-runner.py` (`_stream_agent_turn`); log `Cursor agent returned empty text; nudging`.

### 2026-08-19 — WeCom extra groups silently stopped replying

- **Symptom:** WeCom group @-mentions stored inbound messages but produced no bot reply; web UI showed user lines only on hashed `chat_id`s.
- **Root cause:** `resolve_wecom_canonical_chat_id` sent additional group handles to hashed contacts once the inbox already had another WeCom binding. Those contacts used a fresh `default` persona, not the Channels Single lock on the operator inbox (`selling_oversea`).
- **Fix:** Always `link_channel` WeCom handles to the operator inbox (or `UNIVERSAL_CHAT_ID`). Directional delivery already scopes outbound replies by handle, so multi-group inbox binding does not fan out replies.
- **Prevention:** Do not split WeCom groups onto hashed contacts for “main chat only” routing when directional reply exists. Hashed contacts orphan persona policy.
- **Files/refs:** `resolve_wecom_canonical_chat_id` (`src/channels/wecom.rs`); `channel_bindings` rows for `channel_type='wecom'`.

### 2026-08-18 — WeCom/Channels sync must stay on main chat

- **Symptom:** After WeCom was bound to the operator inbox so Channels policy worked, every WeCom handle and web focused-session reply also fan-out to that inbox’s channels.
- **Root cause:** `ingest_wecom_incoming` linked all `user:`/`chat:` handles to `operator_inbox_chat_id()`, and web `ContactWide` delivery did not distinguish main chat (`session_id` null) from focused sessions.
- **Fix:** ContactWide with a session id stores web-only. WeCom binds the first handle (or `UNIVERSAL_CHAT_ID`) to the inbox; additional handles resolve to hashed chats.
- **Prevention:** Do not attach every platform handle to the operator inbox. Channels policy is per canonical `chat_id`; only the main-chat contact should receive it. Keep `session_id: None` on channel ingest.
- **Files/refs:** `deliver_to_contact_with_origin` (`src/channel.rs`); `resolve_wecom_canonical_chat_id` (`src/channels/wecom.rs`).

### 2026-08-18 — WeCom Channels persona lock always ran as default

- **Symptom:** Settings → Channels Single persona (or switching personas) had no effect on WeCom; replies always used the `default` persona.
- **Root cause:** Channel policy and personas are keyed by canonical `chat_id`. The web UI writes policy for the operator inbox (`997894126`). WeCom created a hashed chat per `user:`/`chat:` handle, then `get_current_persona_id` created a separate `default` persona. `persona_exists(hashed_chat, web_persona_id)` could not succeed.
- **Fix:** Bind WeCom inbound to `Config::operator_inbox_chat_id()` (same inbox as the web UI) and link the WeCom handle there so policy lookup uses the personas the operator actually selected.
- **Prevention:** For this single-contact gateway, do not invent a second canonical chat for a channel whose persona routing UI is the web inbox. Keep `persona_exists` and `channel_persona_policy` on the same `chat_id`.
- **Files/refs:** `ingest_wecom_incoming` (`src/channels/wecom.rs`); `operator_inbox_chat_id` (`src/config.rs`); `resolve_incoming_run_persona_for_channel` (`src/persona.rs`).

### 2026-08-18 — Integrations allowed-group save did not take effect

- **Symptom:** Changing Settings → Integrations allowed groups/chats saved in the UI/DB but inbound traffic still used the old allowlist (or never matched the intended WeCom group).
- **Root cause:** WeCom copied `wecom_allowed_chats` into the dispatcher client at process start and never re-read SQLite. A group display name (e.g. `selling_oversea`) also does not equal WeCom’s opaque `chatid`. Telegram/Discord numeric parse silently dropped non-integer tokens, so a bad paste looked like “save did nothing.”
- **Fix:** Reload WeCom allowlist from `channel_bot_instances` on each inbound message; accept `chat:`/`user:` prefixes and case-insensitive ids; log the actual `raw_id` when dropping. Reject unparseable Telegram/Discord IDs with HTTP 400.
- **Prevention:** Channel access controls that operators change in the UI must be read from DB on the inbound path (same pattern as Telegram `allowed_groups`). Do not freeze allowlists on the dispatcher client. Label WeCom fields as chatid/userid, not group name.
- **Files/refs:** `load_wecom_allowed_chats`, `chat_allowed` (`src/channels/wecom.rs`); `handle_msg_callback` (`src/channels/wecom_aibot.rs`); log `dropped inbound: not in Integrations allowed chats`; `parse_id_list_i64` (`src/web.rs`).

### 2026-08-12 — Cursor bridge pool leaked until EMFILE

- **Symptom:** Cursor engine / sidecar failed with `OSError: [Errno 24] Too many open files`; `socket.accept() out of system resource` on `:3848`. Hundreds of `cursor-sdk-bridge.js` processes (many days old).
- **Root cause:** Pool keyed by persona + `session_scope`. Scheduled runs use unique `scheduled:<task_id>:<iso_ts>` scopes, so each fire launched a new warm bridge that was never closed (only on sidecar shutdown or retryable error). Tool-callback listen sockets + CLOSE-WAIT filled the sidecar’s 1024 FD soft limit.
- **Fix:** Evict ephemeral scopes after each `/run`; idle TTL + max pool size reaper; force-kill subprocess if `Client.close()` leaves it alive. Ops: terminate orphan bridges and restart sidecar.
- **Prevention:** Never pool one-shot `scheduled:` / UUID background scopes; keep `/health` `persona_bridges_active` bounded; alert if it climbs without bound.
- **Files/refs:** `scripts/cursor-sdk-runner.py`; `cursor_session_scope` in `src/cursor_engine.rs`; log `socket.accept() out of system resource` / `Too many open files` in `workspace/runtime/cursor-sdk-sidecar.stderr.log`.

### 2026-07-24 — Cursor SDK MCP discovery failed on bogus protocol version

- **Symptom:** Agent evaluation showed `Cursor SDK → MCP tool discovery Broken` / bot tools `Unavailable`. `GetMcpTools` for `finally-a-value-bot` returned *failed during live tool discovery*; only `mcp_auth` appeared. Loopback `POST /internal/cursor-mcp` was up; settings had `CURSOR_MCP_TOOLS_ENABLED=true`.
- **Root cause:** Bridge answered `initialize` with `protocolVersion: "2025-11-05"` (not a real MCP version — mix of `2024-11-05` + `2025-11-25`). Cursor client requests `2025-11-25` and disconnects when the negotiated version is unsupported.
- **Fix:** Negotiate real versions (`2025-11-25` default; echo client when supported). Add loopback `GET /internal/cursor-mcp` → `405` (Streamable HTTP). Log initialize/tools/list. Redeploy binary + restart gateway.
- **Prevention:** Never invent protocol version strings; unit-test negotiation; verify live turns log `Cursor MCP initialize … negotiated="2025-11-25"` and `tools/list completed`.
- **Files/refs:** `src/cursor_mcp_bridge.rs` (`negotiate_protocol_version`); `src/web.rs` (`api_cursor_mcp_get`); log signature `Cursor MCP initialize requested=… negotiated=…`.

### 2026-07-23 — Cursor/agent git bound the bot checkout from persona cwd

- **Symptom:** Development prompts (e.g. delete main branch) could mutate `finally-a-value-bot` itself when Cursor ran under persona cwd inside `WORKSPACE_DIR`.
- **Root cause:** Persona dirs live under the bot git tree; `git` walked up from cwd and discovered the bot `.git`. Soft “Git discipline” prompt was not enough.
- **Fix:** `GIT_CEILING_DIRECTORIES=<WORKSPACE_DIR>` on Cursor sidecar / cursor_agent / bash / background shells; hard-block bash git that names the self-repo path; prompt self-repo ban. Persona Tier-1 `Repo:` paths stay fully allowed via explicit `cd`.
- **Prevention:** Never clear `GIT_CEILING_DIRECTORIES` for agent shells; do not treat the bot checkout as a project workspace. Set `FINALLY_A_VALUE_BOT_SELF_REPO` if auto-detect fails.
- **Files/refs:** `src/self_repo.rs`, `src/cursor_sdk_sidecar.rs`, `src/tools/{bash,bash_safety,cursor_agent,command_runner}.rs`, `src/cursor_delegation_prompt.rs`.

### 2026-07-23 — Unstable props defeated ThreadPane React.memo (idle UI jank)

- **Symptom:** Web UI felt laggy while idle (typing/scroll/menus); chat remounted or re-rendered on the 2.5–10s ops poll cadence.
- **Root cause:** `React.memo(ThreadPane)` was load-bearing (journal 2026-04-10) but App passed **new inline callback identities** every render (`onLoadMoreHistory={() => …}`, etc.). Ops poll also called `setPersonas` whenever React Query data identity changed, even when persona fields were unchanged, so App re-rendered often and ThreadPane always followed.
- **Fix:** Pass stable `useCallback` handlers into ThreadPane; hoist `makeMarkdownText(...)` to module scope (avoid remounting markdown types); gate `setPersonas` with `personasSnapshotEqual`; memoize shell chrome; add `/api/ops_poll` + client single-fetch bundle; mild SSE yield throttle (80ms).
- **Prevention:** Never wrap memoized leaf props in fresh lambdas at the parent call site. When adding poll-driven `setState`, compare the fields that actually drive UI before updating. Prefer one combined poll endpoint over N parallel GETs on a short interval.
- **Files/refs:** `web/src/app/App.tsx` (ThreadPane props, SSE flush); `web/src/components/thread-pane.tsx` (`React.memo`, `MarkdownText`); `web/src/hooks/use-ops-poll.ts` (`personasSnapshotEqual`); `src/web.rs` (`api_ops_poll`); plan `web_ui_performance_920afed4`.

### 2026-07-07 — Scheduled tasks "disappeared" and stopped running

- **Symptom:** Web Schedules tab showed no tasks; scheduled jobs did not run for a day. Data was actually intact (193 rows, 14 active).
- **Root cause:** One `scheduled_tasks` row had its `prompt` stored with SQLite **BLOB** affinity (SQLite is dynamically typed; likely a manual `sqlite3` edit — Rust paths only bind `&str`). `row.get::<String>(3)` failed for that row, aborting the **entire** `get_due_tasks` / `get_tasks_for_chat` query every tick. Log signature: `ERROR scheduler: failed to query due tasks: Invalid column type Blob at index: 3, name: prompt`.
- **Fix:** Live data repair `UPDATE scheduled_tasks SET prompt = CAST(prompt AS TEXT) WHERE typeof(prompt)='blob';` (backed up the row first). Scheduler recovered on the next tick and caught up overdue tasks.
- **Prevention:** Made row reads blob-tolerant via `row_text` / `row_text_opt` / `map_scheduled_task_row` in `src/db.rs` so one malformed row can never take down a whole query. When a whole listing/query "returns nothing," check the logs for a per-row deserialization error before assuming data loss. Never hand-edit DB rows in a way that changes column affinity.
- **Files/refs:** `src/db.rs` (`row_text`, `row_text_opt`, `map_scheduled_task_row`), `src/scheduler.rs` (`run_due_tasks`, `get_due_tasks`). Code hardening requires rebuild + reinstall of `~/.local/bin/finally-a-value-bot`.
