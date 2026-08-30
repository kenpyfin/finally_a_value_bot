# Development journal

Chronological log of **non-trivial** implementation work: features, refactors, architectural decisions, and fixes that affect behavior or structure.

Use **newest entries first** (reverse chronological). Each entry should be self-contained enough that a future reader (or agent) can find code and rationale quickly.

### 2026-08-27 — Background command notices follow the focused session

- **Area:** background_shell / focused sessions / delivery
- **Summary:** `spawn_background_command` start/finish/cancel notices (and the agent follow-up after the job) now store on the focused web session that spawned them, not always main chat. `session_id` is on `ToolAuthContext` and persisted on `background_jobs`.
- **Rationale:** Operators running long shell work from a focused session never saw the “Background command started…” ack in that thread (`deliver_to_contact` used `session_id: None`).
- **Key files / symbols:** `ToolAuthContext.session_id`; `create_background_shell_job` / `create_background_job`; `try_enqueue_background_shell`; `deliver_shell_notification`; `try_enqueue_background_handoff`; `spawn_background_job`.
- **Note:** Restart the gateway so the SQLite `background_jobs.session_id` migration runs. Telegram/Discord/WeCom inbound stays main-chat (`session_id: None`).

### 2026-08-26 — Mixed license: proprietary edits, MIT original

- **Area:** licensing
- **Summary:** Replaced the dual-copyright MIT file with a mixed license. kenpyfin modifications and new files are proprietary source-available (personal non-commercial run/modify only). Original everettjf portions stay MIT. Cargo.toml now uses `license-file` instead of `license = "MIT"`.
- **Rationale:** MIT cannot be removed from the upstream code, but new work does not have to be MIT. The combined tree is no longer permissively licensed.
- **Key files / symbols:** `LICENSE` Parts 1–2; `Cargo.toml` `license-file`; `README.md` License section.

### 2026-08-26 — Dual copyright on MIT license

- **Area:** licensing
- **Summary:** LICENSE now lists kenpyfin (2026) alongside the original everettjf (2025) MIT copyright, with a short derivative-work note. README License section matches.
- **Rationale:** This tree started from everettjf's MIT-licensed project and now contains substantial original work; MIT requires keeping the upstream notice while adding the current author's copyright.
- **Key files / symbols:** `LICENSE`; `README.md` License section.
- **Note:** License type remains MIT (`Cargo.toml` `license = "MIT"`).

### 2026-08-27 — Steel health path + unmanaged wait blocked Web UI

- **Area:** Steel browser bootstrap / gateway startup
- **Summary:** Gateway waited ~90s (wrong `/api/health` 404) before binding Web UI even when `BROWSER_MANAGED` was off. Probe now tries `/v1/health` then `/api/health`; health wait only runs when managed mode is enabled.
- **Key files / symbols:** `probe_steel_health`, `bootstrap` in `src/steel_browser_sidecar.rs`; doctor check in `src/doctor.rs`.

### 2026-08-27 — Wider assistant bubbles; flat user messages

- **Area:** Web UI / thread pane
- **Summary:** `--aui-thread-max-width` is `100%` of the chat column (was vendor `42rem` / our `72rem`). Assistant bubbles fill that width; user messages are flat right-aligned text (no muted bubble) with higher-specificity overrides against `@assistant-ui` defaults.
- **Key files / symbols:** `.aui-assistant-message-content`, `.aui-user-message-content` in `web/src/styles.css`.
- **Note:** Rebuild `web/dist` and restart gateway (assets are embedded). Hard-refresh the browser.

### 2026-08-26 — Cursor duplicated stream + SDK result in one reply

- **Area:** Cursor sidecar / delivery coalesce
- **Summary:** Influencer_PZ_3 summary reply stored the same write-up twice (broken token stream, then clean markdown). Sidecar now delivers SDK `wait().result` alone when available; Rust dedupes repeated sections if both still arrive.
- **Key files / symbols:** `streamAgentTurn` in `scripts/cursor-sdk-runner.mjs`; `coalesce_cursor_delivery_text` / `dedupe_cursor_delivery_text` in `src/cursor_engine.rs`.
- **Note:** Recycle sidecar + restart gateway. Existing duplicated DB rows are not auto-fixed.

### 2026-08-26 — Web chat message formatting (bubbles, tables, actions)

- **Area:** Web UI / thread pane
- **Summary:** Improved chat readability: rounded grouped bubbles on mobile and desktop (consecutive same-sender messages visually link via shared corner radii and tighter spacing), capped assistant line width (~42rem), and restored assistant bubbles on mobile instead of flat text. Markdown tables use a shared wrapper with sticky headers, cell wrapping, and horizontal scroll. Message actions are a hover-reveal icon pill (copy/reply/bookmark/delete) with fixed clipboard copy (API + execCommand fallback) and full reply text on copy.
- **Key files / symbols:** `thread-pane.tsx`, `styles.css`, `markdown-table.tsx`, `copy-to-clipboard.ts`, `message-group.ts`, `reply-quote.ts`.
- **Note:** Rebuild `web/dist`.

### 2026-08-26 — Cursor word-per-line replies (fragment stream coalescing)

- **Area:** Cursor engine / sidecar delivery
- **Summary:** Cursor SDK sometimes emits many tiny `text` events (one token/word each). Rust and the sidecar treated each as a separate utterance and joined with `\n\n`, producing unreadable one-word-per-line chat. Short fragments now append in-place (space-aware, including token continuations like `hot`+`ify`), `done.result` with per-word newlines is repaired, and full progress sentences still get paragraph breaks.
- **Key files / symbols:** `cursor_text_event_is_stream_fragment`, `join_cursor_stream_fragments`, `coalesce_cursor_delivery_text` in `src/cursor_engine.rs`; `pushCursorTextPart`, `joinCursorUtterances` in `scripts/cursor-sdk-runner.mjs`.
- **Note:** Recycle sidecar (script mtime) and restart gateway.

### 2026-08-26 — Web agent failures left user messages unanswered

- **Area:** Web delivery / Cursor engine
- **Summary:** A stored web user message could finish with no bot row when Cursor (or classic LLM) returned `Err` or empty text. Stream showed a transient error, history poll then looked like the bot ignored the user. After the user message is stored, web always delivers a visible notice on failure/empty; empty `deliver_agent_final` Skip now stores `EMPTY_TURN_NOTICE` instead of dropping the turn. The chat UI reloads history on stream `error` as well as `done`.
- **Key files / symbols:** `send_and_store_response_with_events` in `src/web.rs`; `failed_turn_notice` / `ensure_visible_turn_text` in `src/final_delivery_dedupe.rs`; `AgentFinalDeliveryPlan::Skip` in `src/channel.rs`; stream `error` in `web/src/app/App.tsx`.
- **Note:** Restart gateway and rebuild `web/dist`. Residual risk: a hard process kill after `store_message(user)` and before the notice still orphans a turn (queue hard-abort already covers timeout/panic).

### 2026-08-26 — Cursor follow-up failed with "already has active run"

- **Area:** Cursor SDK sidecar
- **Summary:** Sending a new chat message reused a local Cursor agent whose previous run was still marked active (stream ended / wait timed out / cancel incomplete). `agent.send()` then threw `Agent … already has active run`. Sidecar now passes `local.force` on every send to expire the leftover run, retries busy errors after dropping the pooled handle, and skips idle-reaping in-use slots. Rust treats the same error like a stale agent id (clear + full-slim retry).
- **Key files / symbols:** `buildSendOptions` / `isBusyAgentError` / `streamRun` in `scripts/cursor-sdk-runner.mjs`; `_is_busy_agent_error` in `scripts/cursor-sdk-runner.py`; `is_busy_cursor_agent_error` in `src/cursor_engine.rs`.
- **Note:** Recycle sidecar (script mtime). Rebuild gateway for the Rust fallback.

### 2026-08-26 — Cap Cursor run.wait so sidecar recycle cannot stall

- **Area:** Cursor SDK sidecar
- **Summary:** Joining `wait().result` after the stream could hang forever, keeping `runs_in_flight > 0` so idle recycle (`POST /admin/request_recycle`, max uptime, script mtime) never fired and pooled handles stayed warm. `run.wait()` is now a **post-stream hang watchdog** (`CURSOR_RUN_WAIT_TIMEOUT_MS`, default 120s): timeout cancels the SDK run, delivers streamed text, and evicts the pooled agent/bridge **after** the slot lock is released. This does not limit tool rounds or long replies (those stay in the unbounded stream loop). Cancelled disconnects also evict. Node default runner does not spawn `cursor-sdk-bridge.js`; Python rollback uses the same wait bound.
- **Key files / symbols:** `waitForRunResult` / `streamAgentTurn` in `scripts/cursor-sdk-runner.mjs`; `_wait_sdk_run` in `scripts/cursor-sdk-runner.py`.
- **Note:** Recycle sidecar (script mtime). Optional `CURSOR_RUN_WAIT_TIMEOUT_MS`.

### 2026-08-26 — Cursor replies were glued progress fragments (sourdough)

- **Area:** Cursor engine / delivery
- **Summary:** Sourdough (persona 26, dense delivery **off**) stored mid-turn Cursor status lines as the chat reply: `process.The ratio screenshot is in.` The SDK emits one assistant `text` event per tool round; the runner/Rust concatenated them with no break and ignored `wait().result` once any text existed. Delivery now treats each `text` event as an utterance (`\n\n`), and persists the last real write-up when it looks like a final answer.
- **Key files / symbols:** `coalesce_cursor_delivery_text` / `consume_sidecar_stream` in `src/cursor_engine.rs`; `joinCursorUtterances` in `scripts/cursor-sdk-runner.mjs`.
- **Note:** Recycle sidecar (script mtime). Not dense delivery.

### 2026-08-26 — Dense delivery: public PDF link after catbox 403

- **Area:** hooks / delivery
- **Summary:** Catbox-only uploads with the default reqwest User-Agent were 403ing, so replies had no link (or an internal path). Uploader now chains catbox → litterbox → tmpfiles → pixeldrain with a browser UA, parses messy JSON/HTML responses, and rejects localhost/`/api/uploads/`. The chat summary no longer owns the URL: a verified public https:// link is always appended as the last line.
- **Key files / symbols:** `PublicHostUploader`, `extract_public_https_url`, `finalize_delivery_message` in `src/dense_delivery_guard.rs`.
- **Note:** Restart gateway. Leave `DELIVERY_UPLOAD_PROVIDER` unset or `catbox` (not `none`). Optional `CATBOX_USERHASH`.

### 2026-08-26 — Dense delivery: natural LLM summary + public PDF

- **Area:** hooks / delivery
- **Summary:** Dense-delivery chat text is no longer a rigid “title — summary / bullets / character count” template. A second LLM call (hook/orchestrator/main model; extractive fallback) writes a natural reply and offers the public PDF URL. The PDF is the **full** original report (tables and other distinctive payload preserved; env secrets redacted) rendered then uploaded to catbox (retry, pandoc fallback after md2pdf). User-facing text never offers an internal/workspace path.
- **Key files / symbols:** `maybe_apply_dense_delivery_with` / `LlmDeliverySummarizer` / `upload_public_with_retry` in `src/dense_delivery_guard.rs`; `builtin_dense_delivery_guard` in `hook_runtime.rs`.
- **Note:** Restart gateway. Keep `DELIVERY_UPLOAD_PROVIDER=catbox` (not `none`) so the reply can include an external HTTPS link.

### 2026-08-26 — Node sidecar polyfills global Web Crypto on Node 18

- **Area:** Cursor SDK sidecar
- **Summary:** Cursor turns failed with `crypto is not defined`. `@cursor/sdk` calls global `crypto.randomUUID()`; systemd's `/usr/bin/node` 18.19 does not define that global for `.mjs` files. Runner now assigns `node:crypto.webcrypto` to `globalThis.crypto` before loading the SDK.
- **Key files / symbols:** `scripts/cursor-sdk-runner.mjs` (webcrypto polyfill; self-test `globalThis.crypto.randomUUID`).
- **Note:** Recycle sidecar (script mtime / idle recycle). No Node upgrade required. Complements JsonlLocalAgentStore for Node < 22.13.

### 2026-08-26 — Node sidecar uses JsonlLocalAgentStore on Node 20

- **Area:** Cursor SDK sidecar
- **Summary:** `@cursor/sdk` local runtime defaults to `node:sqlite`, which needs Node >= 22.13. Host is Node 20, so `Agent.create`/`resume` failed with that error. Sidecar now passes `local.store = new JsonlLocalAgentStore(dir)` per persona/session under `{runtime}/cursor-sdk-state/`.
- **Key files / symbols:** `localStoreFor` / `agentStoreRoot` in `scripts/cursor-sdk-runner.mjs`.
- **Note:** Recycle sidecar (script mtime) so the runner reloads. No Node upgrade required.

### 2026-08-25 — Agent engine settings apply to the active persona only

- **Area:** Settings / personas
- **Summary:** Settings → Agent engine no longer lists every persona. The original engine-button UX is back, scoped to the sidebar’s current persona (name shown on the tab). Other personas keep their stored override or inherit the global default.
- **Key files / symbols:** `settings-agent-engine.tsx`; PATCH `/api/personas/:id/bulletin` `agent_engine_override`.
- **Note:** Rebuild `web/dist`. Switch persona in the sidebar, then open Settings to change that persona.

### 2026-08-25 — Node Cursor SDK sidecar (no cursor-sdk-bridge)

- **Area:** Cursor SDK sidecar / Settings
- **Summary:** Default Cursor engine sidecar is now Node `scripts/cursor-sdk-runner.mjs` using `@cursor/sdk` in-process. Settings **Refresh models** (`GET /models` → `Cursor.models.list`) and turns no longer spawn Python's `cursor-sdk-bridge.js`, which was the source of connection-fail / bridge-discovery errors. Loopback MCP and persona cwd are unchanged. Python runner remains for rollback via `CURSOR_SDK_RUNNER_SCRIPT`. Supervisor npm-installs into `{runtime}/cursor-sdk-node` and dropped orphan-bridge recycle.
- **Key files / symbols:** `scripts/cursor-sdk-runner.mjs`; `ensure_sidecar_node_prefix` / `resolve_sidecar_script` in `src/cursor_sdk_sidecar.rs`; `CURSOR_SDK_NODE`.
- **Note:** Recycle/rebuild gateway. Need Node 20+ and npm on PATH. Optional rollback: `CURSOR_SDK_RUNNER_SCRIPT=scripts/cursor-sdk-runner.py`. Rebuild `web/dist` for Settings copy.

### 2026-08-25 — Per-persona agent engine in Settings

- **Area:** Settings / personas / cockpit
- **Summary:** Agent engine is no longer chosen in the cockpit. Settings → Agent engine lists every persona with its own engine (or inherit). The previous global picker is the inherit default only. Runtime still resolves `personas.agent_engine_override` at `process_with_agent_with_events`. `GET /api/personas` now includes `agent_engine_override` / `agent_engine_effective`.
- **Key files / symbols:** `settings-agent-engine.tsx`; `api_personas` in `src/web.rs`; cockpit engine UI removed from `cockpit-bar.tsx`.
- **Note:** Rebuild `web/dist`. Existing persona overrides keep working.

### 2026-08-25 — Live model catalog for Agent engine (Classic LLM)

- **Area:** Settings / LLM catalog
- **Summary:** Agent engine → Single turn (Classic) now loads live model ids from the provider API (`GET /api/llm/models`) instead of only the curated in-code list. OpenAI-compatible `/models` (with Bearer), Anthropic `/v1/models`, and Gemini model list; local Ollama/llama.cpp uses the existing `/v1/models` probe. Curated rows stay for cost hints and as a fail-open fallback. PATCH `/api/llm` accepts live ids (no longer requires `custom=true`). Azure/Bedrock/custom stay curated-only. Refresh models in `settings-llm.tsx`.
- **Key files / symbols:** `fetch_live_provider_models` in `src/llm.rs`; `merge_live_model_ids` in `src/llm_catalog.rs`; `api_llm_models_get` in `src/web.rs`; `SettingsLlmPanel`.
- **Note:** Restart gateway. Rebuild `web/dist` for Settings. Keys remain in `.env` only.

### 2026-08-25 — Dense Delivery Guard + per-persona agent engine

- **Area:** hooks / delivery / personas / Settings
- **Summary:** Persona-gated dense delivery spills over-limit replies (2k messaging / 1k web) to markdown+PDF, uploads to catbox, and replaces the delivered text with a summary + public HTTPS URL. Runs as PreDelivery **after PDQE** so quality eval still sees the full report. Cockpit toggle (seeded on for persona 28). Per-persona `agent_engine_override` is resolved at `process_with_agent_with_events` (NULL/invalid inherit global). Settings folds LLM / Local delegate / Cursor / Deterministic into one **Agent engine** tab.
- **Key files / symbols:** `dense_delivery_guard.rs`; `builtin_dense_delivery_guard`; `run_pre_delivery_hooks_after_gate` in `telegram.rs`; `resolve_run_agent_engine`; `personas.dense_delivery_*` / `agent_engine_override`; cockpit bulletin PATCH; `settings-agent-engine.tsx`.
- **Note:** Restart gateway so migrate adds columns and seeds persona 28. Rebuild `web/dist` for Settings/cockpit. Optional: `DELIVERY_UPLOAD_PROVIDER=catbox|none`.

### 2026-08-25 — Fix ops_poll UTF-8 panic (cockpit queue blank)

- **Area:** web / cockpit ops poll
- **Summary:** Cockpit queue went blank because `/api/ops_poll` panicked when truncating a background job `result_text` at byte 200 inside a multi-byte em dash. Queue data was fine (`/api/queue_diagnostics` still worked). Fixed with `floor_char_boundary`.
- **Key files / symbols:** `background_job_result_preview` / `json_background_job` in `src/web.rs`.
- **Note:** Restart gateway to pick up.

### 2026-08-25 — Cursor sidecar idle-safe auto-recycle

- **Area:** Cursor SDK sidecar / deploy
- **Summary:** Sidecar exposes idle-only `POST /admin/request_recycle` plus uptime fields on `/health`. Rust supervisor soft-recycles on max uptime / script mtime / orphan bridge pressure when `runs_in_flight==0`, and force-recycles after two wedged health probes. Bootstrap no longer attaches forever to a stale runner. `reload.sh` soft-drains then force-kills sidecar + bridges before gateway restart (`scripts/recycle-cursor-sidecar.sh`).
- **Key files / symbols:** `handle_request_recycle` in `scripts/cursor-sdk-runner.py`; `supervise_sidecar` / `SidecarHandle` in `src/cursor_sdk_sidecar.rs`; `reload.sh`.
- **Note:** Recycle/rebuild gateway to pick up. Optional: `CURSOR_SIDECAR_MAX_UPTIME_SECS` (default 86400).

### 2026-08-25 — Remove unfinished PreDelivery char-limit PDF guard

- **Area:** hooks / delivery
- **Summary:** Stripped the incomplete global PreDelivery PDF-spill attempt (module was never committed; wiring left the tree broken). Removed `PreDelivery` event, `updated_assistant_text`, `builtin_char_limit_pdf_guard`, `DELIVERY_CHAR_LIMIT_*` config, and docs/UI catalog entries. Migrate deletes any seeded `predelivery-char-limit-pdf-guard` row. Finish path goes straight to PostDelivery again. Prep for a future persona-gated dense-delivery rebuild.
- **Key files / symbols:** `hook_runtime.rs`, `pipeline_finish_turn` in `telegram.rs`, `ensure_builtin_hook_definitions` in `db.rs`.
- **Note:** Restart gateway so migrate clears the orphaned hook row if still present.

### 2026-08-24 — Cursor sidecar: offload sync turns + cancel-safe disconnect

- **Area:** Cursor SDK sidecar / cursor engine
- **Summary:** Long sync `agent.send`/`messages`/`wait` no longer blocks the aiohttp loop (queue-from-thread). Disconnect requests `run.cancel()` instead of evicting the bridge under `pooled.lock`. Ping runs outside `_POOL_GUARD`. `/run` concurrency capped (`CURSOR_RUN_CONCURRENCY`, default 4). Rust stream cancel polls every 250ms; MCP tokens revoke on Drop if `finish_run` did not take them.
- **Key files / symbols:** `scripts/cursor-sdk-runner.py` (`_stream_agent_turn_async`, `_ActiveTurn`, `_try_begin_run`); `McpTokenGuard` / `consume_sidecar_stream` in `src/cursor_engine.rs`.
- **Note:** Recycle `cursor-sdk-runner.py` after deploy. Optional: `CURSOR_RUN_CONCURRENCY`.

### 2026-08-21 — Always-finish turns + Cursor bridge orphan sweeper

- **Area:** chat queue / PDQE / Cursor SDK sidecar
- **Summary:** Stuck interactive turns (PZ / selling_oversea / Videographer) started tools then never wrote `run_finished` or a bot reply. Hardened: (1) queue hard-timeout/panic now invokes `on_hard_abort` to store a user-visible notice + `run_finished` (web also publishes run_hub `error`); (2) PDQE has an outer wall timeout (evaluator timeout + 30s) fail-open; focus sync is wall-timed at 120s; `run_finished` is awaited before return; (3) sidecar idle TTL default 600s / pool max 16, reaper kills untracked `cursor-sdk-bridge.js` PIDs, and client disconnect during `/run` evicts the active pool key.
- **Key files / symbols:** `src/queue_abort.rs`; `QueueHardAbortHook` in `src/chat_queue.rs`; finish path in `src/channels/telegram.rs`; `scripts/cursor-sdk-runner.py` (`_kill_orphan_bridge_processes`, handle_run eviction).
- **Note:** Restart gateway + recycle `cursor-sdk-runner.py`. Optionally set `CURSOR_BRIDGE_IDLE_TTL_SECS` / `CURSOR_BRIDGE_POOL_MAX` / `CURSOR_BRIDGE_ORPHAN_GRACE_SECS`. Kill any pre-existing orphan bridge PIDs once after deploy.

### 2026-08-20 — Shared processing ack on all external channels

- **Area:** channels / Telegram / Discord / WhatsApp / WeCom
- **Summary:** All external channels now show `Processing…` when a queued inbound run starts. Telegram seeds the editable status message (then tool updates / delete); Discord sends then deletes; WhatsApp sends a short interim; WeCom AI Bot stream placeholder uses the same string; WeCom callback sends a proactive interim.
- **Key files / symbols:** `CHANNEL_PROCESSING_ACK` in `src/channels/mod.rs`; wired in `telegram.rs`, `discord.rs`, `whatsapp.rs`, `wecom.rs`, `wecom_aibot.rs`.
- **Note:** Restart gateway. Web UI keeps its own loading state (no chat spam).

### 2026-08-20 — WeCom long-connection: immediate stream ack + delayed send_msg

- **Area:** channels / WeCom AI Bot
- **Summary:** After a long Cursor turn (incl. empty-output placeholder), WeCom stopped delivering group callbacks. Root cause: replies used late `aibot_respond_msg` without opening a stream within WeCom’s callback window. Now: `begin_stream_reply` sends stream `finish=false` (“处理中…”) before the agent run; final reply finishes the stream within 9 minutes or falls back to `aibot_send_msg`. Heartbeat pong watchdog reconnects half-open sockets.
- **Key files / symbols:** `begin_stream_reply`, `respond_stream_frame`, `PendingReply` in `src/channels/wecom_aibot.rs`.
- **Note:** Restart the gateway. After restart, @ the bot in the group; you should see “处理中…” then the final answer.

### 2026-08-20 — Cursor empty-output nudge for WeCom/channels

- **Area:** cursor engine / WeCom
- **Summary:** Resume turns sometimes finished with no assistant text; the gateway delivered `(Cursor agent completed with no text output.)` to WeCom. Now: nudge up to `DEFERRED_COMMITMENT_MAX_NUDGES` for a user-facing reply when empty and resume id exists; else recover short prose from tool results; placeholder only as last resort. Sidecar also accepts broader assistant message shapes and prefers `wait().result` when stream text is empty.
- **Key files / symbols:** `empty_output_nudge_prompt`, `recover_user_text_from_tool_records` in `src/cursor_engine.rs`; `_stream_agent_turn` in `scripts/cursor-sdk-runner.py`.
- **Note:** Restart gateway + recycle `cursor-sdk-runner.py` so both pick up the change.

### 2026-08-19 — WeCom group chat: re-bind all handles to operator inbox

- **Area:** channels / WeCom
- **Summary:** Extra WeCom groups were routed to hashed canonical contacts (`resolve_wecom_canonical_chat_id` refused a second handle on the inbox). Those groups missed Settings → Channels Single persona (`selling_oversea`) and showed inbound messages with zero bot replies. All handles now re-link to the operator inbox on each inbound; directional `platform_reply` still replies only to the sender’s handle. Direct `send_text` fallback if unified delivery fails.
- **Key files / symbols:** `resolve_wecom_canonical_chat_id`, WeCom agent delivery in `src/channels/wecom.rs`.
- **Note:** Restart gateway. @ the bot in the group; stale hashed bindings migrate on the next inbound message.

### 2026-08-19 — Directional channel replies (WeCom included)

- **Area:** channels / delivery
- **Summary:** External integrations (Telegram, Discord, WhatsApp, WeCom) now reply only on the inbound binding handle via `DeliveryScope::platform_reply`. Web main chat and focused sessions use `StoreOnly` (history in UI, no fan-out). Scheduler/background delivery is web-only. WeCom multi-group on one inbox no longer broadcasts replies to every linked handle.
- **Key files / symbols:** `DeliveryScope` / `platform_reply` in `src/channel.rs`; inbound handlers in `src/channels/{telegram,discord,whatsapp,wecom}.rs`; web send in `src/web.rs`.
- **Note:** Restart the gateway. Rebuild `web/dist` if using embedded UI.

### 2026-08-18 — Only main chat syncs with Channels

- **Area:** channels / WeCom / web sessions
- **Summary:** Channels persona routing and outbound fan-out belong to the operator inbox main chat. Focused sessions store web-only (`DeliveryScope::StoreOnly`). WeCom links at most one handle to that inbox (first inbound, or `UNIVERSAL_CHAT_ID`); extra WeCom groups/DMs keep hashed contacts so they do not inherit the inbox policy.
- **Key files / symbols:** `deliver_to_contact_with_origin` in `src/channel.rs`; `resolve_wecom_canonical_chat_id` in `src/channels/wecom.rs`; `lookup_canonical_chat_id` in `src/db.rs`.
- **Note:** Restart the gateway. Existing extra WeCom handles already linked to `997894126` stay until unlinked.

### 2026-08-18 — Deprecate focused-session archive

- **Area:** web UI / chat sessions / scheduler
- **Summary:** Removed focused-session archive and the 15-minute TTL auto-archive sweep. Sessions stay until the operator deletes them. Startup reopens any previously archived rows. PATCH `status=archived` returns 410; list no longer hides rows; create default `ttl_hours` is 0. Session picker drops archive/restore and can delete the current focused session. `/archive` (markdown dump of the conversation) is unchanged.
- **Key files / symbols:** `migrate_chat_sessions_schema` / `list_chat_sessions` in `src/db.rs`; `api_chat_sessions_list` / `api_chat_sessions_patch` in `src/web.rs`; TTL loop removed from `spawn_scheduler` in `src/scheduler.rs`; `SessionPicker` in `web/src/components/session-picker.tsx`; `handleDeleteSession` in `web/src/hooks/use-persona-session.ts`.
- **Note:** Rebuild `web/dist` and restart the gateway. Previously archived sessions reappear in the picker after restart.

### 2026-08-18 — WeCom honors Settings → Channels persona policy

- **Area:** channels / WeCom / personas
- **Summary:** Settings → Channels is saved on the web inbox (`997894126`), but WeCom inbound hashed its own canonical chat and `get_or_create_default_persona` always ran there. Single-persona locks never matched (`persona_exists` is per chat). WeCom now links into `Config::operator_inbox_chat_id()` so All/Single policy and the selected persona apply without restart.
- **Key files / symbols:** `ingest_wecom_incoming` in `src/channels/wecom.rs`; `operator_inbox_chat_id` / `DEFAULT_UNIVERSAL_CHAT_ID` in `src/config.rs`.
- **Note:** Restart once to pick up the bind. Then change Channels in the web UI; WeCom uses that contact’s policy on the next message.

### 2026-08-18 — WeCom inbound frames were dropped silently

- **Area:** channels / WeCom
- **Summary:** AI Bot websocket subscribed but inbound traffic produced no logs or replies. Parser required string `from.userid` + `chattype=="group"` and returned without logging; Integrations allowlist `selling_oversea` (group name, not chatid) would also drop group messages. Now log every `aibot_*` frame, accept numeric chattype/userid, keep the connection on a bad JSON frame, and warn on allowlist/persona/mention drops.
- **Key files / symbols:** `handle_msg_callback`, `parse_chattype`, `json_text` in `src/channels/wecom_aibot.rs`; ingest warnings in `src/channels/wecom.rs`.
- **Note:** Restart gateway. In a group the user must @ the bot. If allowlist is set, copy `chatid=` from `WeCom inbound message` logs into Integrations (empty = all).

### 2026-08-18 — Integrations allowlists apply without restart

- **Area:** channels / WeCom / Telegram / integrations
- **Summary:** Settings → Integrations allowlists now apply on save. WeCom was still using the list copied into the dispatcher at boot, so changing allowed chats did nothing until restart (and a group *name* never matched WeCom’s `chatid`). Inbound WeCom now reloads `wecom_allowed_chats` from the bot instance row, matches `chat:`/`user:` prefixes case-insensitively, and logs the real id when a message is dropped. Telegram/Discord already read from DB; invalid numeric IDs now return 400 instead of being silently dropped.
- **Key files / symbols:** `load_wecom_allowed_chats` / `chat_allowed` in `src/channels/wecom.rs`; `handle_msg_callback` in `src/channels/wecom_aibot.rs`; `parse_id_list_i64` in `src/web.rs`; Integrations labels in `web/src/components/settings-integrations.tsx`.
- **Note:** Rebuild `web/dist` and restart once so the binary picks up the live-reload code. After that, allowlist edits apply immediately. WeCom allowlist must be the callback `chatid`/`userid` (check logs `dropped inbound: not in Integrations allowed chats` for the real id), not the group display name.

### 2026-08-18 — WeCom inbound frames were dropped silently

- **Area:** channels / WeCom
- **Summary:** AI Bot websocket subscribed but inbound traffic produced no logs or replies. Parser required string `from.userid` + `chattype=="group"` and returned without logging; Integrations allowlist `selling_oversea` (group name, not chatid) would also drop group messages. Now log every `aibot_*` frame, accept numeric chattype/userid, keep the connection on a bad JSON frame, and warn on allowlist/persona/mention drops.
- **Key files / symbols:** `handle_msg_callback`, `parse_chattype`, `json_text` in `src/channels/wecom_aibot.rs`; ingest warnings in `src/channels/wecom.rs`.
- **Note:** Restart gateway. In a group the user must @ the bot. If allowlist is set, copy `chatid=` from `WeCom inbound message` logs into Integrations (empty = all).

### 2026-08-18 — WeCom AI Bot long-connection adapter

- **Area:** channels / WeCom / integrations
- **Summary:** Added the 智能机器人 WebSocket adapter (`wss://openws.work.weixin.qq.com`) as the preferred WeCom path. Platform remains `wecom` / instance id 4. When Bot ID + Secret are set (mode `aibot`, or Bot ID present), the gateway subscribes with `aibot_subscribe`, heartbeats `ping` every 30s, ingests `aibot_msg_callback` (groups already @-filtered), replies with `aibot_respond_msg` / fanout `aibot_send_msg`, and decrypts media via per-file `aeskey`. Self-built app HTTPS `/callback` stays as fallback. One live connection per bot (new subscribe kicks the old one). Settings → Integrations: Connection = AI Bot long connection vs callback; token field is the long-connection Secret.
- **Key files / symbols:** `src/channels/wecom_aibot.rs` (`WecomAiBotClient`, `start_wecom_aibot`); `WecomGateway` in `src/channels/wecom.rs`; `Config::wecom_uses_aibot`; DB `wecom_aibot_id` / `wecom_mode`; `WecomExtraFields` in `web/src/components/settings-integrations.tsx`.
- **Note:** Restart after saving. No SWAG `/callback` needed for AI Bot. Rebuild `web/dist` so embedded UI includes the Connection selector. Official Account (公众号) and 客户联系 remain out of scope.

### 2026-08-18 — WeCom Integrations add form fields

- **Area:** web UI / WeCom
- **Summary:** Settings → Integrations “Add Bot Instance” only showed label + corp secret for WeCom. Corp ID, Agent ID, callback token, EncodingAESKey, port, and allowed chats now appear on create (and with labels on the saved instance card). POST `/api/channel_bot_instances` already accepted those fields.
- **Key files / symbols:** `WecomExtraFields` in `web/src/components/settings-integrations.tsx`.
- **Note:** Rebuild `web/dist` and restart the gateway so embedded assets pick this up.

### 2026-08-17 — WeCom (企业微信) channel

- **Area:** channels / WeCom / integrations
- **Summary:** Added a WeCom self-built app channel (`wecom`) with encrypted callback verification (WXBizMsgCrypt), async agent turns, and outbound `message/send` / `appchat/send` through `deliver_to_contact` so the unified inbox fans out. Single primary instance id `4`; string handles `user:{userid}` / `chat:{chatid}` with hashed canonical ids when `UNIVERSAL_CHAT_ID` is unset. Settings → Integrations form plus Channels All/Single (not WhatsApp-locked). Dedicated callback port default 8081 (`GET|POST /callback`); HTTPS reverse proxy required by WeCom.
- **Key files / symbols:** `src/channels/wecom.rs`, `src/channels/wecom_crypt.rs`, `WecomClient` on `AppState`; `BOT_INSTANCE_WECOM_PRIMARY` + `wecom_*` columns in `src/db.rs`; `deliver_to_contact` wecom arm in `src/channel.rs`; Integrations UI in `web/src/components/settings-integrations.tsx`.
- **Note:** Restart after saving WeCom credentials. Official Account (公众号) and 客户联系 are out of scope.

### 2026-08-12 — Cursor sidecar: close ephemeral bridges (EMFILE fix)

- **Area:** cursor engine / sidecar
- **Summary:** Sidecar hit `OSError: [Errno 24] Too many open files` (1023/1024 FDs) after ~340 orphan `cursor-sdk-bridge` processes accumulated. Session-scoped pool keyed every scheduled fire as `scheduled:<id>:<ts>` and never evicted. Now: close ephemeral scopes after each `/run`, idle TTL reaper (900s) + pool cap (32), hardened `client.close()` with force-kill fallback, parallel pool drain on shutdown.
- **Key files / symbols:** `_is_ephemeral_session_scope`, `_evict_idle_and_over_cap_bridges`, `_close_pooled_bridge`, `_bridge_idle_reaper` in `scripts/cursor-sdk-runner.py`.
- **Note:** Sidecar script is read on process start — recycle `cursor-sdk-runner.py` (or restart the bot) to pick up. Ops recovery killed orphan bridges and restarted the sidecar with `.env` `CURSOR_API_KEY`.

### 2026-07-30 — Web UI: remove redundant toolbar Queue button

- **Area:** web UI / queue
- **Summary:** Removed the desktop header **Queue** button. Run queue remains reachable from the cockpit strip (Queue / Background links) and from mobile Operator tools.
- **Key files / symbols:** `AppHeader` toolbar props; `CockpitBar` `onQueueClick`; `MobileOpsSheet` `onOpenQueue`.
- **Note:** Rebuild web assets to pick up the UI change.

### 2026-07-24 — Fix Cursor MCP tool discovery (protocol version)

- **Area:** cursor engine / MCP bridge
- **Summary:** Cursor SDK live tool discovery for `finally-a-value-bot` failed because `initialize` returned non-existent protocol `2025-11-05`. Bridge now negotiates official versions (default/echo `2025-11-25`), exposes Streamable HTTP GET as `405`, and logs initialize/`tools/list`. Verified on persona 24: `GetMcpTools` → `MCP_OK` / `serverStatus=ready`.
- **Key files / symbols:** `negotiate_protocol_version` / `handle_cursor_mcp` / `handle_cursor_mcp_get` in `src/cursor_mcp_bridge.rs`; `GET|POST /internal/cursor-mcp` in `src/web.rs`; default in `src/mcp.rs`.
- **Note:** Restart gateway after deploying the binary so in-memory MCP registry and the new negotiation code load together.

### 2026-07-23 — Web UI: mobile inbox icon, chat switch loading, inbox→session, scroll-to-latest

- **Area:** web UI / inbox / chat sessions
- **Summary:** Mobile header Inbox is icon-only (badge kept) so session controls are not crowded. Persona/session switches show a full thread skeleton while history loads instead of leaving the previous chat visible. Inbox Open jumps to the session of the latest bot message (main vs focused), using new `last_bot_message_session_id` / `_title` from personas/`ops_poll`. Scroll-to-latest is centered above the composer; `Composer.Input` sets `unstable_focusOnScrollToBottom={false}` so assistant-ui does not focus the textarea (which opened the mobile keyboard).
- **Key files / symbols:** `AppHeader` + `IconInbox`; `ThreadHistorySkeleton` when `historyLoading`; `InboxPanel.onOpenTarget` / `switchPersona(..., { sessionId })`; `PersonaLastBotInfo` / `list_persona_last_bot_message_at` in `src/db.rs`; `DraftAwareComposer` + `ScrollToLatest` + `.mc-thread-composer-stack`.
- **Note:** Rebuild web assets + restart bot so API session fields and UI ship together.

### 2026-07-23 — Ban Cursor/agent from self-repo; allow Tier-1 target repos

- **Area:** security / cursor engine / bash
- **Summary:** Prevent Cursor SDK, `cursor_agent`, and agent shells from treating the finally-a-value-bot checkout as a project repo (git discovery from persona cwd under `WORKSPACE_DIR`). Set `GIT_CEILING_DIRECTORIES` to the workspace root on sidecar and agent commands; block bash/background git that explicitly targets the self-repo; refuse Cursor cwd on self-repo source paths. Persona Tier-1 `Repo: /absolute/path` targets remain fully allowed. Override via `FINALLY_A_VALUE_BOT_SELF_REPO`.
- **Key files / symbols:** `src/self_repo.rs`; `apply_git_ceiling_env` / `check_agent_cwd_allowed` / `command_targets_self_repo_git`; `src/cursor_sdk_sidecar.rs`; `src/tools/{bash_safety,bash,cursor_agent,command_runner}.rs`; `src/cursor_engine.rs`; `src/cursor_delegation_prompt.rs`; `scripts/cursor-agent-runner.py`.
- **Note:** Restart bot so the Cursor sidecar is respawned with the new ceiling env.

### 2026-07-23 — Web UI performance: ThreadPane isolation, poll churn, bundle split, ops_poll

- **Area:** web frontend / web API
- **Summary:** Fixed idle jank from ops/history polling defeating chat isolation. Stabilized ThreadPane callbacks (`useCallback`), hoisted `makeMarkdownText` to module scope, equality-gated `setPersonas` (`personasSnapshotEqual`), memoized Header/Sidebar/Cockpit, lazy-loaded settings/terminal panels, Vite `manualChunks` + `font-display: optional`. Ops poll refreshes personas only every 10s. Phase 4: SSE delta yield throttle 50→80ms; new `GET /api/ops_poll` returns lanes + background jobs (+ optional personas) so the client no longer fans out three GETs per tick. Message-list virtualization deferred unless residual jank remains.
- **Key files / symbols:** `ThreadPane` / `MarkdownText` in `web/src/components/thread-pane.tsx`; `personasSnapshotEqual` / `useOpsPoll` in `web/src/hooks/use-ops-poll.ts`; `fetchOpsPollBundle` in `web/src/api/ops-fetch.ts`; `api_ops_poll` in `src/web.rs`; `manualChunks` in `web/vite.config.ts`; SSE flush in `web/src/app/App.tsx`.
- **Note:** Rebuild web (`cd web && npm run build`) and restart bot so embedded assets + `/api/ops_poll` are live.

### 2026-07-22 — Web persistent auth + Inbox (unread + agent todos)

- **Area:** web UI / auth / personas / agent tools
- **Summary:** Web auth token now persists in `localStorage` (with one-time migrate from `sessionStorage`) so closing the browser no longer forces re-entry. Persona unread dots no longer light up for all history: missing last-read is baselined on persona load, and boot `markPersonaRead` accepts an explicit chat id. Added per-persona operator todos (`persona_todos` SQLite) with agent tools `add_todo` / `list_todos` / `complete_todo`, `GET/PATCH /api/todos`, and an Inbox dialog (new messages + open todos) with header/mobile badge.
- **Key files / symbols:** `getStoredAuthToken` / `setStoredAuthToken` in `web/src/api/client.ts`; `baselinePersonaLastReadIfMissing` in `web/src/lib/persona-storage.ts`; `markPersonaRead` in `use-persona-session.ts`; `PersonaTodo` + `migrate_persona_todos` in `src/db.rs`; `src/tools/persona_todo.rs`; `api_todos_list` / `api_todos_patch` in `src/web.rs`; `InboxPanel` in `web/src/components/inbox-panel.tsx`.
- **Note:** Rebuild web assets + restart bot. Distinct from removed `todo_*` / `TODO.json` and from Tier 3 memory. Operator completes todos in Inbox; agent creates them.

### 2026-07-15 — Integrations tab unified around bot instances

- **Area:** channels / web settings / config
- **Summary:** Settings → Integrations no longer shows separate primary Telegram/Discord/WhatsApp forms plus a duplicate all-bots list. `channel_bot_instances` now carries per-instance platform options (`bot_username`, Telegram group allowlist, Discord channel allowlist, WhatsApp phone/verify/webhook settings); `CONTROL_CHAT_IDS` stays global shared access. Telegram/Discord runtime reads allowlists from the active bot instance. Settings → Channels remains the persona-routing surface and now includes WhatsApp; WhatsApp is single-persona by default because this gateway supports one WhatsApp Business number, not multiple independent WhatsApp dispatchers.
- **Key files / symbols:** `ChannelBotInstance` / `migrate_channel_bot_instances_and_policy` in `src/db.rs`; `GET/PATCH /api/channels/integration` and `/api/channel_bot_instances` in `src/web.rs`; `SettingsIntegrationsPanel` in `web/src/components/settings-integrations.tsx`; `resolve_incoming_run_persona_for_channel` in `src/persona.rs`; `deliver_and_store_bot_message` in `src/channel.rs`.
- **Note:** Restart gateway after changing bot tokens or webhook fields. Legacy app settings/env values are migrated/backfilled to primary instance rows; extra WhatsApp rows are not created by the UI because only one WhatsApp number is supported.

### 2026-07-14 — Channel integrations configured in Web UI (not .env)

- **Area:** channels / web settings / config
- **Summary:** Telegram, Discord, and WhatsApp tokens plus platform options (`BOT_USERNAME`, allowlists, WhatsApp phone/verify/port, control chats) are now configured in Settings → Integrations and persisted in SQLite (`channel_bot_instances` + `app_settings`). Startup one-time-migrates legacy channel env vars when `CHANNEL_INTEGRATION_SEEDED` is unset, then merges DB into `Config`. Env sync no longer overwrites primary bot rows every boot. Bootstrap (`WEB_*`, `WORKSPACE_DIR`) and LLM API keys stay in `.env`.
- **Key files / symbols:** `src/channel_integration_config.rs` (`migrate_from_env_if_empty`, `merge_into_config`, `GET/PATCH /api/channels/integration`); `web/src/components/settings-integrations.tsx`; `main.rs` startup sequence; `is_channel_ready` DB-aware in `web.rs`.
- **Note:** Restart gateway after saving tokens. Remove channel secrets from `.env` after confirming Integrations. Headless installs need a prior migrate or DB seed.

### 2026-07-12 — Resume-delta continuation context + PostDelivery focus sync hardening

- **Area:** cursor engine / PostDelivery hooks / persona bulletin
- **Summary:** Sourdough main-chat incident: resume-delta sent only `[current_request]` (~1.8k chars) so a generic "commit and push to dev" lost the prior sourdough `index.html` fix; PostDelivery focus sync left bulletin stale (Instagram promo) because the strategy LLM sub-call no-op'd. Resume-delta now always injects `[continuation_context]` (last prior_turn user+assistant pair) and Tier 1 anchor + git discipline in the minimal header. Focus sync uses a narrow context (delivered reply + fresh DB bulletin), lightweight sync system prompt, required bulletin update on task deliveries (2 iterations + `tool_choice: required` when supported), and structured logging (`focus_sync_started` / `focus_sync_completed` / `focus_sync_noop`).
- **Key files / symbols:** `build_resume_delta_messages`, `extract_tier1_anchor`, `extract_last_prior_turn_pair` in `src/cursor_delegation_prompt.rs`; `run_persona_focus_sync_after_delivery`, `build_focus_sync_messages`, `focus_sync_task_delivery` in `src/channels/telegram.rs`; `LlmHandle::send_message_with_options` in `src/llm.rs`.
- **Note:** Rebuild + restart bot. Gemini focus-sync may still ignore `tool_choice`; narrowed prompt + second iteration mitigates.

### 2026-07-10 — Cursor sidecar: global bridge launch queue + scheduled Classic fallback

- **Area:** cursor engine / sidecar / scheduler
- **Summary:** Scheduled tasks (#6 vault index, etc.) failed with `Timed out waiting for bridge discovery` when multiple crons claimed together and each cold-started a `cursor-sdk-bridge`. Sidecar now: (1) global `asyncio.Semaphore(1)` serializes all `Client.launch_bridge` calls; (2) bridge discovery timeouts are retryable (up to 3 attempts with pool eviction); (3) default launch timeout raised from 30s → **60s** (`CURSOR_BRIDGE_LAUNCH_TIMEOUT_SECS` override); (4) `launch_bridge` runs in `asyncio.to_thread` so the event loop stays responsive while queued. Rust: `is_cursor_sidecar_recoverable_error` matches bridge-discovery failures; **scheduled tasks fall back to Classic** (same as background jobs) when Cursor/sidecar is unavailable instead of hard-failing.
- **Key files / symbols:** `_BRIDGE_LAUNCH_SEM`, `_bridge_launch_timeout_secs`, `_is_retryable_bridge_error` in `scripts/cursor-sdk-runner.py`; `cursor_engine_classic_fallback`, `is_cursor_sidecar_recoverable_error` in `src/cursor_engine.rs`.
- **Note:** Sidecar script changes apply on next bot restart (managed sidecar respawns from repo `scripts/cursor-sdk-runner.py`). Rust changes require rebuild/reinstall.

### 2026-07-10 — Web file links: materialize `file://` + Cursor delivery reminder

- **Area:** delivery / final_delivery_media / cursor engine / PEP
- **Summary:** PEP GN session showed Cursor engine emitting `file://` markdown links and fabricated `/api/uploads/...` paths — both broken in the web UI. Added `normalize_local_artifact_ref` (strip `file://`, `file://localhost/…`, and `?`/`#` suffixes) before artifact resolution so `materialize_response_file_links` rewrites them to fresh upload URLs. Strengthened shared system prompt (never `file://`; include absolute path when user asks for a link) and Cursor delegation (`CURSOR_FILE_DELIVERY_REMINDER` in slim system prompt + resume-delta runtime header).
- **Key files / symbols:** `normalize_local_artifact_ref`, `resolve_workspace_artifact_path` in `src/final_delivery_media.rs`; `CURSOR_FILE_DELIVERY_REMINDER`, `slim_delegation_system_prompt`, `build_minimal_runtime_header` in `src/cursor_delegation_prompt.rs`; `test_materialize_response_file_links_rewrites_file_url_target` in `src/web.rs`.
- **Note:** Requires rebuild + bot restart. Re-ask for a file link after deploy — stored messages with old `file://` text stay broken until a new reply.

### 2026-07-09 — Web image display: backtick-wrapped persona filenames

- **Area:** delivery / final_delivery_media / Influencer_PZ_3
- **Summary:** History review for persona 24 (`Influencer_PZ_3`) showed repeated “show me the image” turns where the assistant named `PZ-….png` in backticks or prose but the web thread had no `<img>` — delivery normalization only rewrote **bare filename lines**, not `` `PZ-foo.png` `` inline. Jul 8 Crissy thread: first reply had zero markdown images (text only); second reply after user complaint included `/api/uploads/…` URLs that do exist on disk. Jul 9 Lands End: `` `PZ-20260709-LANDSEND-HOTIFY.png` `` never materialized because of the backtick gap. Extended `normalize_assistant_artifact_references` with backtick filename pass → markdown image before `materialize_response_file_links`.
- **Key files / symbols:** `inline_image_backtick_regex`, `markdown_image_for_basename` in `src/final_delivery_media.rs`.
- **Note:** Stored messages without image markup stay text-only until user asks again (post-rebuild). Rebuild + restart bot.

### 2026-07-08 — Cursor engine: session-scoped bridge pool per persona

- **Area:** cursor engine / sidecar
- **Summary:** Bridge pool is now keyed by **persona cwd + session_scope** (not persona alone). Rust sends `session_scope` on each sidecar `/run` (matches `cursor_engine_agents` DB key). Each focused session gets its own warm `cursor-sdk-bridge` subprocess and on-disk `state_root`, so `agent_id` resume and resume-delta prompts stay isolated per session. Main chat uses empty scope (`main`). Per-pool `asyncio.Lock` serializes turns within one session; different sessions/personas run concurrently.
- **Key files / symbols:** `SidecarRunRequest.session_scope` in `src/cursor_engine.rs`; `_bridge_pool_key`, `_get_pooled_bridge` in `scripts/cursor-sdk-runner.py`.
- **Note:** Sidecar script + Rust changes apply on next bot restart/rebuild.

### 2026-07-08 — Cursor engine: bridge crash recovery + session cleanup

- **Area:** cursor engine / sidecar
- **Summary:** `Bridge request failed: ConnectError: [Errno 111] Connection refused` came from the Cursor SDK's internal `cursor-sdk-bridge` subprocess dying while the Python sidecar kept a stale singleton client. Fixes: (1) each `/run` now launches an isolated `Client.launch_bridge` with a stable per-cwd `state_root` under `runtime/cursor-sdk-state/`, then tears it down in `finally` via `agent.close()` + `client.close()` + `close_default_client()`; (2) up to 3 retries with backoff on retryable bridge/network errors; (3) serialized `/run` via `asyncio.Lock` so concurrent turns cannot share or kill each other's bridge; (4) Rust `is_cursor_sidecar_recoverable_error` broadens Classic fallback to include bridge failures (not only transport errors to the sidecar); (5) sidecar stderr is appended to `runtime/cursor-sdk-sidecar.stderr.log` instead of `/dev/null`.
- **Key files / symbols:** `scripts/cursor-sdk-runner.py` (`_release_cursor_bridge`, `_bridge_state_root`, `_RUN_LOCK`, `BRIDGE_RETRY_*`); `is_cursor_sidecar_recoverable_error` in `src/cursor_engine.rs`; `sidecar_stderr_log_path`, `spawn_sidecar_process` in `src/cursor_sdk_sidecar.rs`.
- **Note:** Sidecar script changes apply on next bot restart (managed sidecar is spawned from repo `scripts/cursor-sdk-runner.py`). Rust changes require rebuild/reinstall.

### 2026-07-08 — Web upload could hang the composer forever

- **Area:** web UI (attachments)
- **Summary:** Uploading a file could leave the app stuck on `Uploading…` indefinitely and lock the composer. Two causes: (1) `uploadAttachmentFile` used `fetch` with **no timeout**, so a stalled/large upload or a busy server never resolved; (2) in the chat adapter's `run` generator, `extractLatestUserInput` (which performs the multipart upload) ran **before** the `try/catch`, so an upload error/abort never reset `statusText`/`error` and the composer stayed in `isRunning`. Fix: added a safety-net timeout (`DEFAULT_UPLOAD_TIMEOUT_MS = 180s`) that combines an internal `AbortController` with the caller signal, distinguishing timeout vs. user-cancel vs. network errors with actionable messages; wrapped attachment extraction in `try/catch` that resets state (silent on user abort, otherwise surfaces the error and ends the run so the composer unlocks).
- **Key files / symbols:** `web/src/lib/attachments.ts` (`uploadAttachmentFile`, `linkSignalWithTimeout`, `DEFAULT_UPLOAD_TIMEOUT_MS`); `web/src/app/App.tsx` (chat `adapter.run` extraction guard); `web/src/lib/attachments.test.ts` (timeout + abort coverage).
- **Note:** Frontend-only; requires a web asset rebuild (`cd web && npm run build`) which is embedded via `include_dir!` at Rust build time.

### 2026-07-07 — Scheduler resilience: blob-typed prompt broke all task queries

- **Area:** scheduler / db
- **Summary:** All scheduled tasks stopped running and the web Schedules tab showed empty. Root cause: one `scheduled_tasks` row (task #35) had its `prompt` stored with BLOB affinity (SQLite is dynamically typed; likely a manual `sqlite3` edit — Rust insert/update paths only bind `&str`). `row.get::<String>(3)` then failed for that row, aborting the entire `get_due_tasks` / `get_tasks_for_chat` query every scheduler tick (`ERROR scheduler: failed to query due tasks: Invalid column type Blob at index: 3, name: prompt`). Fixed live data with `UPDATE scheduled_tasks SET prompt = CAST(prompt AS TEXT) WHERE typeof(prompt)='blob'`; scheduler recovered on the next tick and caught up 8 overdue tasks. Hardened reads with blob-tolerant helpers so one malformed row can no longer take down the scheduler/API.
- **Key files / symbols:** `row_text`, `row_text_opt`, `map_scheduled_task_row` in `src/db.rs` (replace 8 inline `ScheduledTask` mappings + `get_task_by_id`); `run_due_tasks` in `src/scheduler.rs`.
- **Note:** Data fix applies to the running instance immediately; the code hardening requires rebuild + reinstall of `~/.local/bin/finally-a-value-bot` to take effect.

### 2026-07-07 — Cursor engine: context deduplication for sidecar delegation

- **Area:** cursor engine / web settings
- **Summary:** Cursor-only prompt shaping reduces duplicated context sent to the SDK sidecar: when MCP is live, strip the long `## Tool groups` catalog from the delegation system prompt (schemas come from MCP `tools/list`); on resumed sessions, send resume-delta prompts (runtime header + trusted message tags only) instead of re-flattening full history. `prep.system_prompt` stays full for `pipeline_finish_turn`. Stale `agent_id` clears DB and retries with full slim. Settings → Cursor toggles **Slim sidecar prompt** and **Resume delta prompts** (`CURSOR_DELEGATION_SLIM_PROMPT`, `CURSOR_DELEGATION_RESUME_DELTA`).
- **Key files / symbols:** `src/cursor_delegation_prompt.rs` (`slim_delegation_system_prompt`, `build_cursor_delegation_prompt`, `DelegationPromptMode`); `run_cursor_engine` in `src/cursor_engine.rs`; `CursorEngineSettings` in `src/cursor_engine_config.rs`; `settings-cursor.tsx`.
- **Scope:** Classic and Deterministic engines unchanged.

### 2026-07-06 — Docs: Cursor engine integration (tools, skills, hooks)

- **Area:** documentation
- **Summary:** Added [`docs/cursor-engine-integration.md`](cursor-engine-integration.md) — how ToolRegistry, skills, and bot-native hooks link to Cursor via loopback MCP (not `.cursor/*` sync). Indexed in [`DEVELOP.md`](../DEVELOP.md); cross-links from [`hooks-architecture.md`](hooks-architecture.md) and updated Cursor section in [`agent-harness-research.md`](agent-harness-research.md).

### 2026-07-03 — Cursor engine: MCP tool bridge + full hook parity

- **Area:** cursor engine / hooks / tools / web
- **Summary:** Cursor SDK runs now expose the bot `ToolRegistry` via loopback MCP (`POST /internal/cursor-mcp`) with run-scoped Bearer tokens. `PreToolUse`/`PostToolUse` run in MCP `tools/call`; `PostToolBatch` at turn end. Assistant text is appended to `messages` before `pipeline_finish_turn` (fixes focus sync). Sidecar passes `mcp_servers` on each `agent.send`; streams `tool_use`/`tool_result` for observability. Settings → Cursor toggles MCP + optional `send_message`.
- **Key files / symbols:** `src/cursor_mcp_bridge.rs`, `src/tool_hook_dispatch.rs`, `run_cursor_engine` in `src/cursor_engine.rs`, `scripts/cursor-sdk-runner.py`, `web/src/components/settings-cursor.tsx`.
- **Follow-ups:** ~~Optional trim of tool catalog from flattened Cursor prompt~~ (done 2026-07-07); per-persona engine override.

### 2026-07-02 — Web UI interactive terminal (PTY + WebSocket)

- **Area:** web UI / web server / config
- **Summary:** Added an optional interactive browser terminal: `POST /api/terminal/sessions` (Bearer auth + short-lived `ws_ticket`) and `GET /api/terminal/ws` (auth JSON, then binary PTY I/O). Frontend uses `@xterm/xterm` in a new **Terminal** dialog (header + mobile ops). Off by default (`WEB_TERMINAL_ENABLED=false`); requires `WEB_AUTH_TOKEN` even on localhost; blocked in Docker unless `WEB_TERMINAL_ALLOW_IN_DOCKER=true`. Session caps and idle timeout configurable.
- **Rationale:** Operators sometimes need a real shell on the gateway host/workspace without SSH. This is **not** agent-mediated `bash` — no per-command `bash_safety`; treat as operator-equivalent access for anyone holding the shared web token.
- **Key files / symbols:** `src/web_terminal.rs` (`TerminalHub`, `handle_websocket`); `Config::web_terminal_*` in `src/config.rs`; routes in `src/web.rs`; `web/src/components/terminal-pane.tsx`; `installation_status.terminal` on `GET /api/settings` and `terminal` on `GET /api/runtime`.
- **Follow-ups:** Optional persona-scoped cwd; background-job log attach from Queue UI; wire SSE `tool_result` into live chat tool cards.

### 2026-07-01 — Cursor engine: bot-native hook bridge (Phase 1)

- **Area:** hooks / cursor engine
- **Summary:** Cursor engine now runs bot `BeforeTurn` and `PreStop` hooks via shared `hook_turn_bridge` (block, `[hook_context]` injection, deferred-commitment nudge retries with resumed `agent_id`). `PostDelivery` unchanged via `pipeline_finish_turn`. Tool hooks (`PreToolUse`/`PostToolUse`/`PostToolBatch`) deferred to Phase 2 (sidecar tool streaming).
- **Key files / symbols:** `src/channels/hook_turn_bridge.rs`; `run_cursor_engine`, `invoke_sidecar_turn`, `build_cursor_prompt` in `src/cursor_engine.rs`; Classic path uses same bridge in `src/channels/telegram.rs`.

### 2026-07-01 — Hooks/skills settings: enriched catalog + persona filter

- **Area:** web UI / hooks & skills settings
- **Summary:** Settings → Hooks & Skills catalogs show richer metadata and persona-scoped filtering (**Show all personas**). Hooks catalog adds an **Event** dropdown per hook (save via `POST /api/hooks`); removed the separate event filter bar.
- **Key files / symbols:** `SettingsHooksSkillsPanel` in `web/src/components/settings-hooks-skills.tsx`; `SkillCatalogEntry` in `web/src/types.ts`.

### 2026-07-01 — Web chat: message copy + text selection

- **Area:** web UI / thread pane
- **Summary:** Added explicit **Copy** actions on user and assistant message rows (replacing the hidden assistant-ui action bar copy). Reply messages copy the readable snippet + follow-up, not the raw `[quoted_message]` block. Mobile message actions now open on **long-press** instead of tap so text selection works; message bodies use `user-select: text`.
- **Key files / symbols:** `MessageCopyButton`, `messageTextForClipboard`, long-press `useMobileMessageTapProps` in `web/src/components/thread-pane.tsx`; `web/src/lib/reply-quote.ts`.

### 2026-07-01 — Web reply bubbles: snippet-only quote display

- **Area:** web UI / reply quotes
- **Summary:** Sent reply messages no longer show the full `[quoted_message]` body in the user bubble. `parseReplyForDisplay` in `reply-quote.ts` extracts the quote metadata and renders the same snippet chip as the composer plus optional follow-up text; the full quote is still sent to the agent unchanged.
- **Key files / symbols:** `parseReplyForDisplay`, `SentReplyQuoteChip`, `UserMessageDisplayBody` in `web/src/components/thread-pane.tsx`; `web/src/lib/reply-quote.ts`; `mc-reply-quote-sent` in `web/src/styles.css`.

### 2026-06-30 — Focused sessions: optional main-chat mirroring

- **Area:** DB / web API / web UI
- **Summary:** New focused sessions default to **isolated** history (messages only in the session thread). **New session** dialog adds **Include messages in main chat** (`mirror_main_chat`). Main chat history/agent context queries now exclude non-mirrored session messages via `MAIN_CHAT_MESSAGE_VISIBILITY` in `src/db.rs`. `PATCH /api/chat_sessions/:id` accepts `mirror_main_chat` for later toggles.
- **Key files / symbols:** `mirror_main_chat` on `ChatSession`; `create_chat_session`, message list queries in `src/db.rs`; `CreateChatSessionRequest` / `chat_session_json` in `src/web.rs`; `session-picker.tsx`, `use-persona-session.ts`, `types.ts`.

### 2026-06-30 — Web sessions: instant create + auto-select in picker

- **Area:** web UI / chat sessions API
- **Summary:** Creating a focused session no longer blocks on the vault/skills bootstrap agent turn — bootstrap runs in a background task after the DB row exists. `POST /api/chat_sessions` now returns a `session` object (same shape as list items) so the UI can switch immediately. Fixed `handleCreateSession`, which previously checked `data.session` while the API only returned flat `session_id` fields, so the picker never updated until a full page refresh.
- **Key files / symbols:** `bootstrap_chat_session_context`, `api_chat_sessions_create` in `src/web.rs`; `handleCreateSession` in `web/src/hooks/use-persona-session.ts`.

### 2026-06-30 — Web chat: preserve scroll when loading earlier messages

- **Area:** web UI / chat thread
- **Summary:** Clicking **Load earlier messages** no longer jumps the thread viewport to the top. When older history is prepended, the viewport keeps the same reading position by capturing `scrollTop`/`scrollHeight` before `runtime.thread.reset` and restoring offset after the DOM grows.
- **Key files / symbols:** `isHistoryPrepend` in `web/src/lib/history-sync.ts`; scroll-restore `useLayoutEffect` / `useEffect` in `web/src/components/thread-pane.tsx`.

### 2026-07-01 — PDQE runs on ask_clarification replies

- **Area:** response quality evaluator
- **Summary:** Clarification turns are user-visible; removed `ask_clarification` from `should_skip_pdqe` and dropped the auto-pass fast path so PDQE evaluates them like other deliveries. `cancelled` still skips PDQE.
- **Key files / symbols:** `should_skip_pdqe`, `fast_path_verdict` in `response_quality_evaluator.rs`.

### 2026-06-30 — Classic cost routing: fix silent stop after local read-only turn

- **Area:** classic agent / local delegate
- **Summary:** After inverse routing sent read-only tool results to the local model, an empty `end_turn` did not trigger strategy fallback (only hallucinated-action text did), so runs could finish with `"Done."` or no streamed web reply. `should_fallback_local_tier_to_strategy` now treats empty assistant text after tool results as fallback-worthy; failed strategy retry continues the loop with `local_error_streak` instead of stopping; end-turn handler continues on strategy when cost routing + tools ran + empty final text.
- **Key files / symbols:** `should_fallback_local_tier_to_strategy`, tier fallback block, end_turn guard in `telegram.rs`.

### 2026-06-30 — Classic · Cost routing replaces phase-based multimodel

- **Area:** classic agent / local delegate / web settings
- **Summary:** Removed Plan→Execute→Synthesize phase machine from Classic. New runtime engine **`classic_cost_routing`**: inverse routing (strategy for iter 0 and mutations; local for read-only continuations), `delegate_local_subjob` tool, mutation guard on local route. **`classic`** (Single turn) unchanged — always strategy. `src/multimodel.rs` → `src/local_delegate/` (`resolve_inverse_route`, `cost_routing_active`, `subjob.rs`). Web: Runtime four engine options, Local delegate tab, verification callouts; `/api/runtime` exposes `local_delegate_ready`, `cost_routing_effective`. Boot migration: legacy `MULTIMODEL_ENABLED` + classic → `classic_cost_routing`.
- **Key files / symbols:** `AgentEngine::ClassicCostRouting` in `runtime_toggles.rs`; Classic loop in `telegram.rs`; `delegate_local_subjob.rs`; `settings-runtime.tsx`, `settings-local-delegate.tsx`; `docs/local-delegate-routing.md`.

### 2026-06-29 — Deterministic pipeline: drop legacy technical/knowledge model routes

- **Area:** agent pipeline / web
- **Summary:** Settings → Deterministic model route picker now offers only `inherit_global`, `strategy`, and `local` (multi-model is single local tier today). Stored profiles with `technical` or `knowledge` migrate to `local` (schema v6). `resolve_model_tier` maps legacy route values to the same local-or-strategy fallback as `local`.
- **Key files / symbols:** `ModelRoute` in `profile.rs`; `MODEL_ROUTES` in `settings-deterministic-pipeline.tsx`.

### 2026-06-29 — Scheduled jobs: Cursor engine (no silent classic fallback)

- **Area:** scheduler / cursor engine
- **Summary:** Scheduled tasks already route through `process_with_agent_with_events` (same as chat). Hardened Cursor path for scheduler runs: isolated Cursor session scope per `run_key`, scheduled-task preamble in sidecar prompt, **no silent fallback to classic** when the sidecar is unavailable (fail with actionable error). Agent history now records `agent_engine: cursor` correctly. Scheduler logs selected engine at task start.
- **Key files / symbols:** `run_scheduled_agent_and_finalize` in `scheduler.rs`; `cursor_session_scope`, `cursor_engine_classic_fallback` in `cursor_engine.rs`; `PipelineFinishExtras.agent_engine` in `agent_history.rs`.

### 2026-06-29 — Plan phase: flexible ephemeral plans (no auto SOP bind)

- **Area:** agent pipeline / web
- **Summary:** Planner no longer auto-attaches persona Tier 2 SOPs from memory. Default plan `source` is ephemeral; SOP reference injection is off unless intent names a vault SOP (`candidate_sop_hint`) or policy `bind_persona_sops_in_plan` is enabled. Planner prompts prefer `search_vault` tool over `run_skill_script` for vault lookup; `search_chat_history` in default tool allowlist. Schema v5 migrates existing profiles (`include_sop_reference=false`, `bind_persona_sops_in_plan=false`). Settings → Deterministic → **Bind persona SOPs in plan**.
- **Key files / symbols:** `collect_sop_candidates`, `find_sop_reference` in `plan.rs`; `PolicyConfig.bind_persona_sops_in_plan` in `profile.rs`; `settings-deterministic-pipeline.tsx`.

### 2026-06-29 — Intent phase: always → plan (remove shortcut exits)

- **Area:** agent pipeline
- **Summary:** Default intent phase now has a **single transition** (`always` → `plan`). Removed default exits to `direct_answer` / `clarify` from intent. Heuristic intent fast-path removed from `run_intent_phase` and disabled by default — it bypassed the LLM and routed conversational/question messages to direct answer, which could surface raw structured output. Schema v4 migrates stored profiles to the new intent transitions.
- **Key files / symbols:** `default_intent_phase_transitions()` in `profile.rs`; `run_intent_phase` in `runner.rs`.

### 2026-06-29 — Prior-step handoff: full output vs LLM summary (configurable)

- **Area:** agent pipeline / web
- **Summary:** Execute phases now store **full step output** (assistant text + tool I/O) in `StepResult.full_output`. Default handoff to the next step is **full** (`prior_step_feed_mode: full`). Optional **summary** mode runs an LLM with a user-defined `prior_step_summary_prompt` (builtin default when empty). Settings → Deterministic → Execute → expand → **Prior step handoff**. Schema v3.
- **Key files / symbols:** `PriorStepFeedMode`, `prepare_prior_step_feed`, `summarize_prior_step_output` in `execute.rs`; `PhaseContextIncludes.prior_step_*` in `profile.rs`.

### 2026-06-29 — Per-phase context toggles (deterministic pipeline settings)

- **Area:** agent pipeline / web
- **Summary:** Each pipeline phase now has `context_includes` toggles (system prompt, agent prep prompt, skills catalog, conversation, persona memory, workspace, SOP reference, current request, prior step summaries, step contract, execution summary). Runtime respects these in intent/plan cloud context, execute step messages, consolidate, and direct-answer paths. Schema v2 with `migrate()` from v1 profiles. Settings → Deterministic → expand phase → **Context includes**.
- **Key files / symbols:** `PhaseContextIncludes` in `profile.rs`; `PipelineCloudContext::format_for`; `compose_system_prompt`; `settings-deterministic-pipeline.tsx`.

### 2026-06-29 — Deterministic pipeline: cloud-rich context, local step contracts

- **Area:** agent pipeline
- **Summary:** Inverted context distribution to match harness design: **intent/plan (cloud)** now receive a `pipeline_cloud_context` block (skills catalog, ~16k conversation excerpt, persona memory, workspace paths). **Execute (local)** receives only `[current_request]`, prior step summaries, and the explicit step contract (`skill_name`, `skill_script`, `skill_args_hint`, inputs) — no truncated full system prompt or collapsed chat history. Planner prompts require catalog-exact `skill_name`; `normalize_plan` strips unknown skills (e.g. hallucinated `write_professional_summary`).
- **Key files / symbols:** `src/agent_pipeline/cloud_context.rs` (`build_pipeline_cloud_context`); `intent.rs` / `plan.rs` (cloud context in LLM user messages); `execute.rs` (`build_local_step_messages`, slim `build_step_system`); `validate_plan_skill_names` in `plan.rs`; `runner.rs` (build context once per run).

### 2026-06-29 — LLM thinking settings + multi-model test UI fix

- **Area:** web / llm / multimodel
- **Summary:** Settings → LLM now exposes **Enable extended thinking** and **Show thinking in replies** (persisted as `LLM_THINKING_ENABLED` / `SHOW_THINKING` in `app_settings`). Gemini requests send `thinkingConfig` when enabled; thought parts render as `<think>` when show is on. Multi-model test no longer calls full config reload (which overwrote unsaved model picks); dropdown stays when the model is in the loaded list.
- **Key files / symbols:** `apply_thinking_settings`, `gemini_thinking_config`, `build_gemini_request` in `src/llm.rs`; `PATCH /api/llm` in `src/web.rs`; `web/src/components/settings-llm.tsx`, `settings-multimodel.tsx`.

### 2026-06-29 — Cursor settings: thinking effort and context window params

- **Area:** web / cursor engine
- **Summary:** Settings → Cursor now loads full model metadata from `Cursor.models.list()` (parameters + variants) and exposes per-model dropdowns for SDK params such as thinking effort (`thinking` / `reasoning` / `effort`) and context window (`context`). Values persist as `CURSOR_SDK_MODEL_PARAMS` in `app_settings` and are passed through the sidecar to `Agent.create` via `ModelSelection.params`.
- **Key files / symbols:** `CursorModelParam`, `fetch_sidecar_model_catalog` in `src/cursor_engine_config.rs`; `SidecarRunRequest.model_params` in `src/cursor_engine.rs`; `_build_model_selection` in `scripts/cursor-sdk-runner.py`; `web/src/components/settings-cursor.tsx`.
- **Cloud strategy LLM note:** Multi-model strategy / Settings → LLM cloud provider does **not** enable Anthropic extended thinking (no `thinking` block in API requests). `SHOW_THINKING` only controls whether `<think>` tags are shown in channel output when a model emits them.

### 2026-06-29 — Multi-model settings: local model dropdown

- **Area:** web / multimodel
- **Summary:** Settings → Multi-model now fetches model ids from the configured OpenAI-compatible local server (`GET /v1/models`) and shows a Select dropdown (with optional custom id), matching the Cursor settings pattern. New `GET /api/multimodel/models?base_url=...` proxies the list server-side with the same probe timeouts as connection tests.
- **Key files / symbols:** `fetch_openai_compatible_models` in `src/llm.rs`; `api_multimodel_models_get` in `src/web.rs`; `web/src/components/settings-multimodel.tsx`.

### 2026-06-29 — PDQE failure surfaced in user-facing replies

- **Area:** evaluators / agent loop
- **Summary:** When pre-delivery quality evaluation (PDQE) fails, the bot now tells the user instead of showing only a generic timeout. Deliver-anyway paths append a structured notice (issues, review summary, confidence). LLM timeouts after a PDQE-triggered revision use a quality-review-specific message. Classic agent `finish_turn!` now `continue`s the loop on PDQE retry instead of falling through to the unknown stop-reason path. Deterministic/Cursor finish loops no longer short-circuit before the shared gate delivers.
- **Key files / symbols:** `format_pdqe_user_delivery_notice`, `format_agent_llm_timeout_message` in `src/response_quality_evaluator.rs`; `append_pdqe_delivery_notice` in `finish_turn_with_quality_gate`; `finish_turn!` macro in `src/channels/telegram.rs`; `finish_pipeline` in `src/agent_pipeline/runner.rs`.

### 2026-06-29 — Customizable deterministic pipeline (Web UI, 4 phases)

- **Area:** agent pipeline / web / runtime config
- **Summary:** Replaced the hardcoded intent→plan→execute→consolidate DAG with a hot-reloadable `PipelineProfile` (max 4 phases, custom transitions, three customization layers). **Layer 1:** operational knobs (timeouts, iteration caps, plan step cap, context limits). **Layer 2:** policy toggles (heuristic intent, merged classify+plan, clarify-on-web/scheduler, skip-consolidate, retry/escalation, local JSON stages). **Layer 3:** per-phase system prompts + optional execute preamble (empty = builtin default). Default profile mirrors prior behavior. API: `GET/PATCH /api/deterministic-pipeline`; Settings → **Deterministic** tab.
- **Key files / symbols:** `src/agent_pipeline/profile.rs` (`PipelineProfile`, `TransitionCondition`, `load_from_db` / `persist_to_db`, key `DETERMINISTIC_PIPELINE_CONFIG`); `src/agent_pipeline/runner.rs` (`run_profiled_pipeline`); `AppState.pipeline_profile`; `web/src/components/settings-deterministic-pipeline.tsx`.
- **Boundaries:** Global profile only (not per-persona). Model endpoints stay in Settings → LLM / Multi-model; phases pick `model_route` (`strategy` | `local` | `technical` | `knowledge` | `inherit_global`). Transition DSL is allowlisted (no arbitrary code).

### 2026-06-29 — Generalized skill script CLI contracts

- **Area:** agent pipeline / run_skill_script
- **Summary:** Replaced persona-specific hotify arg patching with `skill_script_contract` module: parses required CLI flags from skill scripts (`argparse required=True`) and SKILL.md; builds plan-step `skill_args_hint` from contract; enriches missing `run_skill_script` args from step hints, prior tool previews, and artifact paths; augments argparse failures with contract guidance. Works for any skill/persona with a bundled CLI script.
- **Key files / symbols:** `src/agent_pipeline/skill_script_contract.rs`; `src/agent_pipeline/execute.rs` (`prior_step_snapshots`, contract-aware preamble); `src/agent_pipeline/plan.rs` (`normalize_plan(plan, state)`, `apply_skill_contract_defaults`).

### 2026-06-29 — Deterministic pipeline runtime + skill/SOP binding

- **Area:** agent pipeline / skills / deterministic engine
- **Summary:** Implemented the optimization plan: heuristic-first intent, merged `classify_and_plan` single LLM call, skip consolidate when output is already good, plan step cap (4), per-step context collapse, circuit breakers (iteration/retry/escalation gates), parallel read-only tools, local tier + 45s timeouts for JSON stages, rich pipeline stage telemetry. Skill binding: extended `format_run_skill_script_hint` for `## Scripts` / `*_cli.py`, planner `skill_name`/`skill_script`/`skill_args_hint` fields, execute preamble + invalid-script correction, gated retry/escalation.
- **Key files / symbols:** `src/agent_pipeline/{mod,intent,plan,execute,consolidate}.rs`; `src/tools/run_skill_script.rs` (`runnable_script_candidates`, `is_shell_like_script_name`, `runnable_script_hint_for_skill`).

### 2026-06-28 — Agent harness research doc (Classic / Deterministic / Cursor vs industry)

- **Area:** architecture / agent engines / docs
- **Summary:** Added `docs/agent-harness-research.md` — consolidated research on the harness principle (model decides, harness enforces), six harness subsystems, FinallyAValueBot's three engines, Cursor SDK five-layer blackbox, Claude Code v2.1.88 leak analyses, OSS harness landscape, gap assessment, and upgrade priorities.
- **Key files / symbols:** `docs/agent-harness-research.md`; cross-refs to `src/channels/telegram.rs`, `src/agent_pipeline/`, `src/cursor_engine.rs`, `scripts/cursor-sdk-runner.py`.

### 2026-06-28 — Cursor SDK sidecar auto-start on bot boot

- **Area:** cursor integration / startup
- **Summary:** Bot now auto-starts `scripts/cursor-sdk-runner.py` on native installs (`CURSOR_SDK_AUTO_START=true` default), waits for `/health`, persists runner URL + `sdk_runner_ok`, and keeps the child alive for the bot lifetime. Default URL `http://127.0.0.1:3848`. Skips spawn inside Docker (still probes configured URL). Web UI simplified: sidecar is managed; only `CURSOR_API_KEY` in `.env` + model selection remain user-facing.
- **Key files / symbols:** `src/cursor_sdk_sidecar.rs` (`bootstrap`, `SidecarHandle`); `AppState.cursor_sidecar`; startup in `run_bot`; `SettingsCursorPanel` simplified.

### 2026-06-28 — Cursor engine settings in Web UI

- **Area:** web / cursor integration / runtime settings
- **Summary:** Added Settings → **Cursor** tab with stepper UI for SDK sidecar install, runner URL, model picker (via sidecar `/models`), health check, and CLI tool fields. Settings persist in `app_settings` and hot-reload via `AppState.cursor_settings`. API: `GET/PATCH /api/cursor-engine`, `POST /api/cursor-engine/health`, `GET /api/cursor-engine/models`. Overview shows `cursor_engine_ready`; Runtime panel warns when Cursor engine is selected but not ready.
- **Key files / symbols:** `src/cursor_engine_config.rs`; `AppState.cursor_settings`; web handlers in `src/web.rs`; `SettingsCursorPanel` in `web/src/components/settings-cursor.tsx`; sidecar `GET /models` in `scripts/cursor-sdk-runner.py`.
- **Deploy:** Rebuild `web/dist`, restart Rust binary. Sidecar host still needs `CURSOR_API_KEY` in environment (never stored in UI/DB).

### 2026-06-28 — Cursor SDK agent engine (local sidecar)

- **Area:** agent loop / runtime toggles / cursor integration / web
- **Summary:** Added a third selectable agent engine **Cursor** that delegates a full turn to a local Python sidecar wrapping `cursor_sdk` (`scripts/cursor-sdk-runner.py`). Rust flattens `prepare_agent_run` into a prompt, POSTs to `CURSOR_SDK_RUNNER_URL/run`, streams NDJSON text deltas to web channels, persists resume `agent_id` per chat/persona/session in `cursor_engine_agents`, and finishes via `pipeline_finish_turn`. Falls back to Classic when the sidecar URL is unset or unreachable. Image input is noted as unsupported in v1.
- **Key files / symbols:** `src/cursor_engine.rs` (`run_cursor_engine`); `AgentEngine::Cursor` in `src/runtime_toggles.rs`; dispatch in `process_with_agent_with_events`; `get/set_cursor_engine_agent_id` + `migrate_cursor_engine_agents` in `src/db.rs`; `cursor_sdk_runner_url` / `cursor_sdk_model` in `src/config.rs`; `SettingsRuntimePanel` Cursor option in `web/src/components/settings-runtime.tsx`.
- **Deploy:** On host: `pip install cursor-sdk aiohttp`, `export CURSOR_API_KEY=...`, `python3 scripts/cursor-sdk-runner.py 3848`. Bot `.env`: `CURSOR_SDK_RUNNER_URL=http://127.0.0.1:3848`. Web UI → Settings → Runtime → Agent engine → Cursor (SDK). Rebuild `web/dist` and restart Rust binary.

### 2026-06-27 — Deterministic agent pipeline (web-selectable engine)

- **Area:** agent loop / multimodel / runtime toggles / web
- **Summary:** Added a second agent engine selectable in Settings → Runtime: **Deterministic** runs a structured pipeline (cloud intent → clarification gate → vault-SOP or ephemeral plan → per-step local execution with retry/escalation → cloud synthesis → PDQE). **Classic** remains the default heuristic tool loop. Multi-model routing: Strategy for intent/plan/synthesis/escalation; Local for bounded step execution when `ready_for_routing()`. Agent history records pipeline stages and cloud-call count.
- **Key files / symbols:** `src/agent_pipeline/` (`run_deterministic_pipeline`, `intent`, `plan`, `execute`, `consolidate`); `prepare_agent_run` in `src/channels/agent_run_prep.rs`; dispatch in `process_with_agent_with_events`; `AgentEngine` + `APP_SETTING_AGENT_ENGINE` in `src/runtime_toggles.rs`; `PipelineStageRecord` / `PipelineFinishExtras` in `src/agent_history.rs`; `SettingsRuntimePanel` engine selector in `web/src/components/settings-runtime.tsx`.
- **Deploy:** Rebuild `web/dist`, rebuild/restart Rust binary. Enable via Settings → Runtime → Agent engine → Deterministic.

### 2026-06-27 — Learn & optimize: operator notes + PDQE input

- **Area:** run optimizer / agent history / web
- **Summary:** Learn & optimize now accepts optional operator guidance via POST body (`operator_notes`) and a textarea in the Agent run debug dialog. Optimizer user message includes the PDQE timeline section from the run file (issues, confidence, retry reasons) alongside the iteration trace; initial LLM snapshot remains excluded. System suffix instructs the model to weigh PDQE and operator input when updating memory.
- **Key files / symbols:** `split_pdqe_for_optimize` in `src/agent_history.rs`; `build_optimize_user_message(..., operator_notes, ...)` in `src/run_optimizer.rs`; `AgentHistoryOptimizeRequest` + `api_persona_agent_history_optimize` in `src/web.rs`; `agentHistoryOptimizeNotes` + textarea in `web/src/app/AppDialogs.tsx`.
- **Deploy:** Rebuild `web/dist`, rebuild/restart Rust binary.

### 2026-06-26 — Web UI: hot-reload PTE and PDQE toggles

- **Area:** web / runtime toggles / evaluators
- **Summary:** Settings → Overview → **Runtime toggles** exposes switches for post-tool evaluator (PTE) and pre-delivery quality (PDQE), alongside verbose pipeline logging. Values persist in `app_settings` and apply immediately to the agent loop (no restart). `.env` remains the default when no app override exists.
- **Key files:** `RuntimeToggles` + `APP_SETTING_POST_TOOL_EVALUATOR_ENABLED` / `APP_SETTING_RESPONSE_QUALITY_EVALUATOR_ENABLED` in `src/runtime_toggles.rs`; `GET/PATCH /api/runtime` in `src/web.rs`; `SettingsRuntimePanel` in `web/src/components/settings-runtime.tsx`.

### 2026-06-26 — Evaluators tab: structured verdict details (issues, feedback)

- **Area:** evaluators / agent history / web
- **Summary:** PDQE steps now persist structured JSON (`verdict`, `confidence`, `issues`, `feedback`, `note`) on indented `eval:` lines in agent history. Web Evaluators tab renders issues list, evaluator feedback block, confidence %, skip/error reasons — not just pass/fail badges. PTE cards label source (LLM vs heuristic) and show rationale prominently. Skipped PDQE runs record `quality_eval_skipped` with reason.
- **Key files:** `format_pdqe_verdict_detail` in `src/response_quality_evaluator.rs`; `EvaluatorStepRecord::format_pdqe_line`; `parsePdqeEvalDetail` / `EvalDetailBlock` in web.

### 2026-06-26 — PTE/PDQE observability + local-first evaluator provider

- **Area:** evaluators / agent history / web Last agent run
- **Summary:** PTE and PDQE now prefer the local multimodel OpenAI-compat endpoint (`resolve_local_evaluator_endpoint`), falling back to Perplexity; evaluator calls cap `max_tokens` at 512 and use a 120s timeout (PTE previously had none). Agent history persists PTE per iteration (`IterationRecord.pte`) and PDQE steps inline before the snapshot (`AgentRunRecord.pdqe_steps`), fixing the PDQE basename ordering bug. Web **Last agent run** dialog adds an **Evaluators** tab (PTE list + PDQE timeline) via extended `parse-agent-history.ts`.
- **Key files / symbols:** `create_evaluator_provider`, `resolve_evaluator_provider_label`, `EVALUATOR_MAX_TOKENS` in `src/llm.rs`; `EvaluatorStepRecord`, `split_trace_for_optimize` in `src/agent_history.rs`; `record_pte_on_last_iteration`, `push_pdqe_step` in `src/channels/telegram.rs`; `evaluate_completion` / `evaluate_delivery_quality`; `AgentHistoryEvaluatorsPanel` in `web/src/app/AppDialogs.tsx`.
- **Deploy:** Rebuild `web/dist`, rebuild/restart Rust binary. New history fields appear on runs saved after deploy.

### 2026-06-26 — Run optimizer: pin memory scope for tool auth

- **Area:** run optimizer
- **Summary:** Learn & optimize completed but wrote no memory when the local model passed wrong `persona_id` (often `997894126` from trace paths). Optimizer system prompt now states required `chat_id`/`persona_id`; memory tool inputs are force-pinned before `execute_with_auth`; tool errors log at warn with preview.
- **Key files / symbols:** `build_optimize_system_prompt`, `pin_memory_tool_scope` in `src/run_optimizer.rs`.
- **Follow-ups:** Rebuild/restart bot; retry Learn & optimize on `20260626-164700.md`.

### 2026-06-26 — Run optimizer: 1000s timeout + run-optimizer subagent

- **Area:** run optimizer / llm HTTP client / Cursor subagent
- **Summary:** Learn & optimize jobs failed at exactly 120s because `OPTIMIZE_LLM_TIMEOUT_SECS` and the reqwest client both capped at 120s while local 30B + tool-use with inherited `max_tokens=8192 could exceed that. Raised optimizer timeout to **1000s** and route optimizer LLM calls through `create_openai_compatible_provider_with_timeout` so HTTP and tokio limits align. Added project subagent `.cursor/agents/run-optimizer.md` for future optimizer/evaluation debugging.
- **Key files / symbols:** `OPTIMIZE_LLM_TIMEOUT_SECS`, `send_local_tier_message`, `resolve_optimizer_local_endpoint` in `src/run_optimizer.rs`; `create_openai_compatible_provider_with_timeout`, `OpenAiProvider::new_with_request_timeout` in `src/llm.rs`.
- **Follow-ups:** Optional dedicated optimizer `max_tokens` cap (512–1024) to avoid slow generation; fix web enqueue gate to check `local_*` not only legacy `tier2_*`.

### 2026-06-25 — Jump to latest: refined scroll FAB

- **Area:** web frontend
- **Summary:** Replaced default `Thread.ScrollToBottom` (outline lucide arrow) with custom `ScrollToLatest` using `ThreadPrimitive.ScrollToBottom`, matching `--mc-*` tokens: circular 40px FAB above the composer, backdrop blur, accent hover, fade-in animation, and larger mobile touch target.
- **Key files / symbols:** `web/src/components/scroll-to-latest.tsx` (`ScrollToLatest`); `web/src/components/thread-pane.tsx`; `web/src/styles.css` (`.mc-scroll-to-latest-wrap`, `.mc-scroll-to-latest-btn`).
- **Deploy:** Rebuild `web/dist` then rebuild/restart the Rust binary.

### 2026-06-25 — Load earlier messages: thread-integrated control

- **Area:** web frontend
- **Summary:** Replaced the awkward top-chrome `Load 30 older messages` soft button with a `LoadEarlierMessages` pill at the top of the thread viewport: shorter copy, chevron-up icon, loading spinner, and placement inside `ThreadPane` where pagination belongs.
- **Key files / symbols:** `web/src/components/load-earlier-messages.tsx` (`LoadEarlierMessages`); `web/src/components/thread-pane.tsx` (`historyHasMore`, `onLoadMoreHistory`); `web/src/app/App.tsx` (removed button from status stack); `web/src/styles.css` (`.mc-load-earlier`, `.mc-load-earlier-btn`).
- **Deploy:** Rebuild `web/dist` then rebuild/restart the Rust binary.

### 2026-06-25 — Header session picker: larger touch targets

- **Area:** web frontend
- **Summary:** Enlarged header `SessionPicker` controls to match adjacent header icon buttons: Radix size `2`, removed `font-size-1` overrides, 36px desktop / 40px mobile min touch targets, wider session label truncation (220px compact), responsive `+ Session` label on `md+`, and `IconMoreVertical` on an `IconButton` for the session actions menu.
- **Key files / symbols:** `web/src/components/session-picker.tsx` (`SessionPicker`, `data-compact`); `web/src/components/icons.tsx` (`IconMoreVertical`); `web/src/styles.css` (`.mc-session-picker`, `.mc-session-picker-trigger`, `.mc-session-picker-btn`).
- **Deploy:** Rebuild `web/dist` then rebuild/restart the Rust binary for embedded assets.

### 2026-06-25 — Web UI blank page: missing `usePersonaSession` import

- **Area:** web frontend
- **Summary:** After the App refactor, `usePersonaSession` was called in `App.tsx` but never imported, causing a runtime `ReferenceError` and blank page. Also removed duplicate `useDocumentVisible` import; cross-hook refs (`loadHistory`, bulletin, pagination) sync via `useLayoutEffect` instead of render-time assignment. Pinned `vite` back to `^5.4.10` — `^8.1.0` broke `npm install` in `reload.sh` (`@tailwindcss/vite@4.1.18` peer requires vite ≤7).
- **Key files:** `web/src/app/App.tsx` (`LoadHistoryFn`, `usePersonaSession` import, `useLayoutEffect` ref bridge); `web/package.json`, `web/package-lock.json`.
- **Deploy:** Rebuild `web/dist` (`npm run build` in `web/`) then rebuild/restart the Rust binary — assets are embedded via `include_dir!` in `src/web.rs`.

### 2026-06-24 — Web UX pass closed: last confirm + dialog skeletons

- **Area:** web frontend
- **Summary:** Replaced final `window.confirm` (bot instance delete) with `ConfirmDialog`; settings overview, artifacts list, artifact preview, and agent-history dialogs now use skeleton loaders; mobile shell gets `overflow-x: hidden` on `html`/`body`/`#root` and `.mc-layout-grid`.
- **Key files:** `web/src/app/App.tsx` (`removeBotInstance`), `web/src/app/AppDialogs.tsx`, `web/src/components/skeleton.tsx` (`OverviewStatusSkeleton`, `ArtifactListSkeleton`, `ContentPreviewSkeleton`), `web/src/styles.css`.

### 2026-06-24 — Web UX pass (continued): hooks, AuthContext, AppHeader/AppDialogs split

- **Area:** web frontend
- **Summary:** Completed remaining UX plan items: `usePersonaSession`, `useChatHistory`, `useOperatorOps` hooks; `AuthProvider` + `AuthDialog`; `AppHeader` (header chrome + toolbar triggers) and `AppDialogs` (~1.6k lines); `lib/persona-storage`, `lib/bulletin`; settings skeletons on all panels; mobile message tap action sheet; `ThreadWelcomeHints` with shortcut footer; archived-sessions empty label in session picker.
- **Key files / symbols:** `web/src/hooks/use-persona-session.ts`, `use-chat-history.ts`, `use-operator-ops.ts`; `web/src/context/AuthContext.tsx`; `web/src/app/AppHeader.tsx`, `AppDialogs.tsx`; `web/src/lib/persona-storage.ts`, `lib/bulletin.ts`; `thread-pane.tsx`, `thread-welcome-hints.tsx`.
- **Follow-ups:** `App.tsx` still ~2k lines (adapter/SSE/scheduling); optional further split of `AppDialogs` tabs.

### 2026-06-28 — Cursor engine: recover from stale resume agent ids

- **Area:** cursor engine / sidecar
- **Summary:** Cursor SDK sessions expire or vanish across API key / sidecar changes. Sidecar now falls back from `Agent.resume` to `Agent.create` when the stored id is missing; Rust clears `cursor_engine_agents` and retries once without `agent_id` if the sidecar still returns an agent-not-found error.
- **Key files / symbols:** `_open_agent`, `_stream_run` in `scripts/cursor-sdk-runner.py`; `is_stale_cursor_agent_error`, `consume_sidecar_stream` in `src/cursor_engine.rs`; `clear_cursor_engine_agent_id` in `src/db.rs`.
- **Follow-ups:** Restart bot so the sidecar reloads the updated Python script (Rust retry covers stale ids even before restart).

### 2026-06-28 — Cursor SDK sidecar auto-install (runtime venv)

- **Area:** cursor sidecar bootstrap
- **Summary:** Bot now creates `{runtime}/cursor-sdk-venv`, pip-installs `cursor-sdk` + `aiohttp`, and restarts a local sidecar missing those deps. Sidecar `/health` reports `cursor_sdk_installed`; `engine_ready` requires it. Opt out with `CURSOR_SDK_AUTO_INSTALL=false`.
- **Key files / symbols:** `ensure_sidecar_venv`, `bootstrap` in `src/cursor_sdk_sidecar.rs`; `SidecarHealth::cursor_sdk_installed` in `src/cursor_engine_config.rs`; `handle_health` in `scripts/cursor-sdk-runner.py`; `CURSOR_SDK_AUTO_INSTALL` in `src/config.rs`.
- **Follow-ups:** Restart bot once to migrate off a stale system-python sidecar on :3848.

### 2026-06-28 — Cursor settings: model dropdown + clearer sidecar 503 errors

- **Area:** web frontend / cursor sidecar
- **Summary:** Settings → Cursor now auto-fetches available SDK models when the sidecar is reachable and `CURSOR_API_KEY` is set, shows a Select dropdown (with optional custom id), and surfaces sidecar error text instead of bare HTTP codes. `fetch_sidecar_models` parses JSON `error` from the Python runner.
- **Key files / symbols:** `web/src/components/settings-cursor.tsx`; `fetch_sidecar_models` in `src/cursor_engine_config.rs`; `handle_models` in `scripts/cursor-sdk-runner.py` (503 when key or `cursor-sdk` missing).
- **Follow-ups:** Restart bot after adding `CURSOR_API_KEY`; `pip install cursor-sdk aiohttp` on the sidecar host.

### 2026-06-24 — Web UX full pass: feedback, IA, mobile ops, App split

- **Area:** web frontend
- **Summary:** Operator cockpit UX pass: `ConfirmDialog` + `useConfirmDialog` for destructive deletes (message, session, persona); `ErrorBanner` (`role="alert"`) and `StatusRegion` (`aria-live`); SVG bookmark icons and larger touch targets; session picker in header with actions menu; mobile Settings + Ops sheet (replaces "More"); header `CockpitStatusChip`; thread/history skeletons; multi-model settings stepper; keyboard shortcuts (`?`, `/`, Esc); light-mode `--mc-text-faint` contrast fix; `main.tsx` → thin bootstrap + `app/App.tsx`.
- **Key files / symbols:** `web/src/components/confirm-dialog.tsx`, `error-banner.tsx`, `status-region.tsx`, `skeleton.tsx`, `empty-state.tsx`, `ops-ui.tsx`, `icons.tsx`; `web/src/hooks/use-keyboard-shortcuts.ts`; `web/src/app/App.tsx`, `web/src/app/constants.ts`; `session-picker.tsx`, `thread-pane.tsx`, `settings-multimodel.tsx`, `cockpit-bar.tsx`, `styles.css`.
- **Design:** Persisted ui-ux-pro-max rules under `design-system/finallyavaluebot/MASTER.md`; kept existing `--mc-*` tokens and Instrument Sans (no rebrand).
- **Follow-ups:** Extract `AppHeader` / `usePersonaSession` hooks from `App.tsx` when next touching shell layout; optional controlled CockpitBar collapse animation audit on iOS.

### 2026-06-24 — Web Reply: full fetch with composer quote chip

- **Area:** web frontend
- **Summary:** Reply now `GET`s the full message by id; composer shows a quote chip with a 200-char snippet only; send injects `[quoted_message …]…[/quoted_message]` plus user follow-up into the agent request body.
- **Key files / symbols:** `web/src/lib/reply-quote.ts` (`makeReplySnippet`, `formatReplyForSend`, `PendingReplyQuote`); `ComposerQuotePreview` in `web/src/components/thread-pane.tsx`; `pendingReplyByThreadKey` / `handleReplyToMessage` in `web/src/main.tsx`.
- **Follow-ups:** Optional system-prompt line describing `[quoted_message]` blocks for models that ignore unfamiliar tags.

### 2026-06-24 — PEP Part 1: scheduled history isolation + web reply/delete/session persistence

- **Area:** db / scheduler / channels / web
- **Summary:** Added `messages.origin` (`interactive` | `scheduled`); scheduler deliveries tag `scheduled` and agent history loaders exclude them while web `/api/history` stays unfiltered. Web UI: Reply injects `> snippet` into composer draft; Delete calls `DELETE /api/personas/:id/messages/:message_id` (full DB delete + bookmark cleanup); persona session id persists in `localStorage` and restores on persona switch.
- **Key files / symbols:** `MESSAGE_ORIGIN_*`, `migrate_messages_origin`, `get_recent_messages(..., exclude_scheduled)`, `delete_message` in `src/db.rs`; `MessageStoreOrigin`, `deliver_*_with_origin` in `src/channel.rs`; scheduler `MessageStoreOrigin::Scheduled`; `load_messages_from_db` in `src/channels/telegram.rs`; `api_persona_message_delete` in `src/web.rs`; `ThreadPane` reply/delete in `web/src/components/thread-pane.tsx`; `PERSONA_SESSION_STORAGE_KEY` + `resolveStoredSessionId` in `web/src/main.tsx`.
- **Rationale:** Scheduled job listings were bloating agent context; operators need quote-reply and message delete in web chat; persona switches should return to the last active session, not always Main.
- **Follow-ups:** Part 2 agent-loop fixes (Execute→Synthesize handoff, degenerate `?` guard) remain deferred.

### 2026-06-20 — Env redaction: skip non-secret config keys and values

- **Area:** safety redaction
- **Summary:** `EnvSecretRedactor::discover` now catalogs values only from credential-like env keys (`*TOKEN*`, `*SECRET*`, `*API_KEY*`, `*_KEY`, `DATABASE_URL`, etc.) and skips known config keys (`WORKSPACE_DIR`, models, ports, paths, limits). Path-like, boolean, numeric, and plain `http(s)://` URL values are skipped even for secret key names. Fixes `[REDACTED_SECRET]` appearing in tool output paths when `WORKSPACE_DIR` or other config values matched as needles.
- **Key files / symbols:** `should_redact_env_key`, `is_non_secret_config_value`, `NON_SECRET_ENV_KEYS` in `src/safety_redaction.rs`.
- **Rationale:** Env-only redaction was still false-positiving on workspace paths and operational config echoed in tool results; TSA removal did not change this layer.
- **Follow-ups:** Restart bot after deploy so catalog rebuilds; add skill-specific key names if a non-standard secret key is missed.

### 2026-06-19 — Multi-model redesign: phase-based routing with single local executor

- **Area:** multimodel / agent loop / frontend
- **Summary:** Replaced the backward-looking tool-name routing (Technical/Knowledge tiers, two local models) with a phase-based state machine: Plan (Strategy) → Execute (Local) → Synthesize (Strategy). Consolidated two local tiers into one (`ModelTier::Local`). Added Anthropic prompt caching (`cache_control: ephemeral` on system + last tool definition). Frontend settings panel simplified to single Local Model section.
- **Key files / symbols:**
  - `src/multimodel.rs` — `AgentPhase`, `PhaseTransition`, `advance_phase()`, `is_mutation_tool()`, `ModelTier::Local`, `local_routable()`, `ready_for_routing()`
  - `src/channels/telegram.rs` — `use_phases`, `current_phase`, `EXECUTE_PREAMBLE`, `effective_system`, phase escalation handling
  - `src/claude.rs` — `CacheControl`, `SystemContent`, `SystemBlock`, `ToolDefinition::new()`
  - `src/llm.rs` — `build_cached_system()`, `anthropic-beta: prompt-caching-2024-07-31` header, unified `tier_endpoint_snapshot`
  - `src/run_optimizer.rs` — `send_local_tier_message`, `local_tier_config_ready`
  - `src/web.rs` — `local_base_url`/`local_model`/`local_tools_ok` in GET/PATCH/POST multimodel endpoints
  - `web/src/components/settings-multimodel.tsx` — single Local Model UI
  - `web/src/types.ts` — `local_base_url`, `local_model`, `local_tools_ok` fields
- **Rationale:** Previous design forced two local models competing for 16GB VRAM and classified by tool names backward-looking. Phase-based routing means one model gets full VRAM, Strategy only runs when needed (plan/synthesize), prompt caching cuts repeated input tokens ~90%.
- **Design:** Plan phase stays on Strategy until a mutation tool is called → transitions to Execute on Local → transitions to Synthesize on: natural completion, error streak ≥2, `[ESCALATE]` text, or iteration cap. Legacy `Technical`/`Knowledge` variants kept as aliases for DB migration.
- **Follow-ups:** Step 4 (legacy code removal) deferred to after production validation.

### 2026-06-18 — Remove TSA (Tool and Skill Agent) gatekeeper

- **Area:** agent loop / config
- **Summary:** Removed the legacy TSA layer that ran an extra LLM call before every tool execution, redacted/truncated conversation context (including emails), and could deny tool use. Deleted `src/tool_skill_agent.rs`; dropped `TOOL_SKILL_AGENT_ENABLED` / `TOOL_SKILL_AGENT_MODEL` config. Tools now execute directly after hooks, as in the orchestrator-first flow.
- **Key files / symbols:** `src/channels/telegram.rs` (removed `evaluate_tool_use` block); `src/config.rs`, `src/config_wizard.rs`, `src/hook_executor.rs`, `.env.example`.
- **Rationale:** Operator feedback — TSA redaction/truncation was mangling emails and blocking normal tool chains; feature was already default-off and superseded by orchestrator routing.
- **Follow-ups:** Remove stale TSA mentions from `ARCHITECTURE.md` when that doc is next edited.

### 2026-06-18 — Remove bash/glob listing-only routing gate

- **Area:** multimodel routing
- **Summary:** Removed `last_tools_are_listing_only` from `resolve_route`. After `bash` or `glob`, the next iteration routes to **Tier 1 (technical)** again instead of forcing strategy — `bash`/`glob` are too common and the gate effectively disabled local LLM use. Safety nets unchanged: `local_tier_error_streak`, strategy tool fallback, `read_file` binary guard, `tool_choice`, probe gate.
- **Key files / symbols:** `resolve_route` in `src/multimodel.rs`; `docs/multimodel-local-tiers.md`.
- **Rationale:** Operator feedback; multi-model should route to local tiers for typical shell/file discovery chains.
- **Follow-ups:** `./reload.sh` on deploy hosts.

### 2026-06-18 — Multi-model local tiers operator guide

- **Area:** docs / multimodel / operations
- **Summary:** Added [`docs/multimodel-local-tiers.md`](multimodel-local-tiers.md) — canonical guide for consistent local tool calling across installs: wire vs behavioral consistency, three-tier architecture, fresh-install checklist, reference llama.cpp setup (`--jinja`, `tool_choice` curl probes), `tools_ok` gate, agent-loop safety nets, troubleshooting, and cross-machine deployment. Linked from `DEVELOP.md` related documentation table.
- **Key files / symbols:** `docs/multimodel-local-tiers.md`; `tool_choice_for_tier`, `persist_tier_tools_ok`, `resolve_route` in `src/multimodel.rs`; Settings → Multi-model UI.
- **Rationale:** Operator knowledge was split across journal entries and ad-hoc probes; new hosts need one checklist without archaeology.
- **Follow-ups:** Optional reference `docker compose` for llama tiers; auto-probe on startup; tier-restricted tool lists (listed as roadmap in doc).

### 2026-06-18 — Local tier tool calling (Qwen `tool_choice`, probe, strategy fallback)

- **Area:** multimodel / llm / agent loop / web settings
- **Summary:** Qwen2.5-Coder on llama.cpp returns `<tools>{...}</tools>` in `content` instead of `tool_calls` unless the request includes `tool_choice: "required"`. Bot now sends tier-appropriate `tool_choice` for local OpenAI-compat tiers (Tier 1 `required`, Tier 2 `auto`) via `LlmSendOptions` and `build_oai_chat_request_body`. Defense-in-depth: `parse_embedded_tool_calls_from_content` promotes Qwen markup to `ToolUse` blocks. Settings → Multi-model **Test** runs a tool probe and persists `MULTIMODEL_TIER1_TOOLS_OK` / `MULTIMODEL_TIER2_TOOLS_OK`; routing to local tiers is gated until verified. Agent loop retries once on strategy when a local tier returns `end_turn` without tools after tool results and text claims unbacked actions (PZ hallucinated upload URLs).
- **Key files / symbols:** `tool_choice_for_tier`, `persist_tier_tools_ok`, `tier1_routable` in `src/multimodel.rs`; `LlmSendOptions`, `test_multimodel_tools`, `parse_embedded_tool_calls_from_content` in `src/llm.rs`; `should_fallback_local_tier_to_strategy`, `assistant_text_claims_unbacked_actions` in `src/channels/telegram.rs`; `POST /api/multimodel/test` in `src/web.rs`; `settings-multimodel.tsx`.
- **Rationale:** Live probe on `10.0.1.217:8080` confirmed server OK; missing `tool_choice` was the PZ failure mode. Probe + route gate prevent silent routing to models that cannot call tools; strategy fallback is a safety net for regressions.
- **Follow-ups:** Re-run tier tests after llama.cpp upgrades; rebuild `web/dist` on deploy.

### 2026-06-18 — LLM settings save hang + fast local server connection tests

- **Area:** llm / web API / Settings → LLM / Settings → Multi-model
- **Summary:** Saving LLM provider (e.g. llama.cpp) could hang forever: `apply_selection` held a read lock on `multimodel_config` while calling `apply_multimodel_config`, which needs a write lock on the same `RwLock` (self-deadlock). Fixed by cloning config via `multimodel_config()` before re-applying. Multi-model / persona connection tests no longer run a full chat completion against local servers with unbounded HTTP waits — local providers use `GET /v1/models` with 5s connect + 15s overall timeout; cloud probes use a minimal chat with the same cap. All LLM HTTP clients now set connect/request timeouts.
- **Key files / symbols:** `LlmHandle::apply_selection`, `test_model`, `probe_openai_compatible_server`, `build_llm_http_client` in `src/llm.rs`; `POST /api/multimodel/test` unchanged but benefits from `test_model`.
- **Rationale:** Settings save must never block on multimodel lock ordering; connection tests should fail fast when llama.cpp is down or slow to load.
- **Follow-ups:** Rebuild/restart gateway after deploy.

### 2026-06-18 — Multi-model tier settings: no defaults, persist when disabled

- **Area:** multimodel config / web UI / LlmHandle
- **Summary:** Tier 1 and Tier 2 URL/model fields start empty (placeholders only) instead of pre-filled defaults. Saved values persist in `app_settings` and survive toggling multi-model off. `LlmHandle` now keeps `multimodel_config` separately from the optional runtime so GET/PATCH still return stored tier URLs when routing is disabled. Enabling routing requires both tiers configured; `normalize()` trims and appends `/v1` but does not inject default models/URLs.
- **Key files / symbols:** `MultimodelConfig::normalize`, `tier1_configured`, `ready_for_routing` in `src/multimodel.rs`; `multimodel_config` field + `apply_multimodel_config` in `src/llm.rs`; `GET/PATCH /api/multimodel` in `src/web.rs`; `web/src/components/settings-multimodel.tsx`.
- **Rationale:** Operators should explicitly configure local servers; disabling routing must not wipe or mask previously entered endpoints.
- **Follow-ups:** Rebuild `web/dist` on deploy.

### 2026-06-17 — Strip internal dialogue XML from chat delivery

- **Area:** agent turn context / channel delivery / web history API
- **Summary:** Internal LLM history wrappers (`<assistant_message context="prior_turn" at="...">`) were leaking into operator chat when the model echoed them. New `strip_stored_dialogue_markup` in `src/agent_turn_context.rs` unwraps echoed assistant/user XML (including persona-prefixed lines) and unescapes entities. Applied on bot message delivery (`deliver_and_store_bot_message`, `deliver_to_contact`, `normalize_final_for_delivery` in `src/channel.rs`) and when serving `/api/history` in `src/web.rs` so existing DB rows render cleanly.
- **Key files / symbols:** `strip_stored_dialogue_markup`, `parse_wrapped_assistant_message` in `src/agent_turn_context.rs`; delivery hooks in `src/channel.rs`; history mapping in `src/web.rs`.
- **Rationale:** Wrappers are prompt-only scaffolding for the model; they must never appear in user-visible chat.
- **Follow-ups:** None.

### 2026-06-17 — Learn & optimize from Last agent run (Tier 2 background job)

- **Area:** run optimizer / web API / web UI / background jobs
- **Summary:** **Learn & optimize** button in the Last agent run dialog enqueues a background job (`job_kind: run_optimize`) that analyzes the latest saved run markdown with **local Tier 2 (Knowledge)** llama and updates persona memory via a restricted tool loop (`read_tiered_memory`, `write_tiered_memory`, `update_bulletin_focus`). Uses saved Tier 2 URL/model from Settings → Multi-model as-is (no default URL changes). Progress in Background jobs panel; completion message delivered to chat.
- **Key files / symbols:** `src/run_optimizer.rs` (`try_enqueue_run_optimize`, `run_optimizer_from_history`, `build_optimize_user_message`); `split_trace_for_optimize` in `src/agent_history.rs`; `create_background_run_optimize_job` in `src/db.rs`; `JobType::RunOptimize` in `src/job_heartbeat.rs`; `POST /api/personas/:persona_id/agent_history/latest/optimize` in `src/web.rs`; button in `web/src/main.tsx`.
- **Rationale:** Operators can turn a finished run trace into durable efficiency learnings without a full agent turn or cloud API cost; manual trigger keeps memory writes intentional.
- **Follow-ups:** Optional filename picker for non-latest runs; extract shared restricted tool loop with persona focus sync.

### 2026-06-17 — Agent run history: multi-model tier tracing

- **Area:** agent history / agent loop / web UI
- **Summary:** Persisted agent run markdown and the web **Last agent run** dialog now record **per-iteration tier routing**: tier name, provider, resolved model, and endpoint. Run header includes a multi-model summary block (enabled/disabled + tier1/2/strategy endpoints). First-turn LLM snapshot JSON adds optional `routing_v1` (iter0 tier + tier config). PTE final synthesis appends a hook line on the iteration with strategy-tier endpoint info.
- **Key files / symbols:** `TierEndpointSnapshot`, `MultimodelRunSummary`, `format_tier_line` in `src/multimodel.rs`; `LlmHandle::tier_endpoint_snapshot`, `multimodel_run_summary` in `src/llm.rs`; `IterationRecord` tier fields + `AgentRunRecord::multimodel_summary` + `format_initial_llm_snapshot_json(..., routing_v1)` in `src/agent_history.rs`; population in `src/channels/telegram.rs`; `parseTierLine`, `formatTierBadgeLabel`, `AgentHistoryTierBadge` in `web/src/parse-agent-history.ts` + `web/src/main.tsx`.
- **Rationale:** Runtime logs showed `model_tier=` but saved history and the cockpit debug dialog did not — operators could not verify local vs strategy routing after a run without reading log files.
- **Follow-ups:** Rebuild `web/dist` on deploy hosts without `npm` in PATH (`node ./node_modules/vite/bin/vite.js build`); optional run-timeline SSE event for active tier during live runs.

### 2026-06-17 — Multi-model routing (local llama.cpp + strategy API)

- **Area:** llm / agent loop / web API / web UI
- **Summary:** Three-tier model routing for privacy/cost: **Tier 1 (technical)** and **Tier 2 (knowledge)** use configurable local llama.cpp OpenAI-compatible servers; **Tier 3 (strategy)** uses the existing Settings → LLM provider/model (e.g. Anthropic Claude). Heuristic routing in the shared agent loop: iteration 0 and final synthesis always strategy; chained tool rounds route to local tiers when all tools in the previous iteration are technical (`bash`, file edits, grep, …) or knowledge (`search_vault`, `read_tiered_memory`, …). Disabled by default — unchanged behavior when off. Web UI **Settings → Multi-model** configures tier base URLs/models, enable toggle, and per-tier connection test. Hot-reloads via `app_settings` without gateway restart.
- **Key files / symbols:** `MultimodelConfig`, `resolve_route`, `ModelTier` in `src/multimodel.rs`; `LlmHandle::send_message_for_tier`, `apply_multimodel_config` in `src/llm.rs`; agent loop routing + `last_iteration_tools` in `src/channels/telegram.rs`; `GET/PATCH /api/multimodel`, `POST /api/multimodel/test` in `src/web.rs`; `web/src/components/settings-multimodel.tsx`.
- **Rationale:** RTX 5060 Ti 16GB can run Qwen-Coder + Mistral-Nemo locally for tool-heavy iterations while reserving API spend for planning and final answers; routing is automatic so operators do not pick models per message.
- **Follow-ups:** Run two llama.cpp instances (ports 8080/8081) with the GGUF models loaded; set Settings → LLM to Anthropic + `claude-sonnet-4-5` (or preferred strategy model); rebuild `web/dist`; tune tool classification lists if new tools need tier hints.

### 2026-06-16 — Focused chat sessions (web-only, per persona)

- **Area:** DB / agent loop / web API / web UI / scheduler
- **Summary:** Per-persona **focused sessions** let the web operator spin up intent-scoped sub-threads without polluting main chat history. New `chat_sessions` table (title, intent, status, TTL, `bootstrap_context_json`) plus `messages.session_id` (nullable; legacy rows stay `NULL` = main chat). Web send/history pass `session_id`; Telegram/Discord/WhatsApp/scheduler/background jobs always use `session_id: None`. On create, a bootstrap agent turn searches vault/skills and stores compact JSON context; each run injects `[session_context]` into the prompt and loads session-scoped history. Sessions skip `trim_to_recent_balanced` and use a 40k (vs 12k) token budget. Scheduler runs a 15-minute TTL sweep that auto-archives expired active sessions (`ttl_hours` default 72; `0` = no expiry). Web UI adds `SessionPicker` (main chat vs active/archived sessions, create/archive/delete).
- **Key files / symbols:** `ChatSession`, `migrate_chat_sessions_schema`, CRUD + `get_all_messages_for_session` / `get_expired_chat_sessions` in `src/db.rs`; `AgentRequestContext::session_id`, session history/context/token paths in `src/channels/telegram.rs`; `session_id` on `deliver_to_contact` / `deliver_agent_final_to_contact` in `src/channel.rs`; `GET/POST /api/chat_sessions`, `GET/PATCH/DELETE /api/chat_sessions/:session_id`, bootstrap in `api_chat_sessions_create` in `src/web.rs`; TTL sweep in `src/scheduler.rs`; `web/src/components/session-picker.tsx`, `web/src/main.tsx`, `web/src/types.ts` (`ChatSession`).
- **Rationale:** Long-running or exploratory work (refactors, research threads) was mixing into the persona’s main timeline and getting trimmed away; isolated sessions keep main chat lean while allowing full session history and richer bootstrap context for focused tasks.
- **Follow-ups:** Rebuild `web/dist` for deploy; fix `handleCreateSession` — API returns `session_id` but UI checks `data.session` so post-create switch may not run; revert accidental `f#` typo in `TEST.md`; consider whether Telegram/Discord should ever bind inbound messages to a session; smoke bootstrap + archive/TTL + main-chat regression.

### 2026-06-07 — Web UI mobile polish (operator cockpit)

- **Area:** web UI (`web/`)
- **Summary:** Expanded session cockpit on small screens uses a fixed scrollable panel with backdrop dismiss instead of overlapping the thread; status strip uses a 2-column grid with hidden separators; controls stack full-width on mobile. Added `viewport-fit=cover`, safe-area padding on header/drawer/composer/dialogs, user bubble right alignment, and tighter tool-card/bookmark overflow handling. Cockpit stays visible when header collapses on scroll if the panel is open (`cockpitExpanded`).
- **Key files / symbols:** `web/index.html`; `web/src/styles.css` (mobile `@media`, `.mc-app-header-bar`, `.rt-DialogContent`); `web/src/components/cockpit-bar.tsx` (`onExpandedChange`, mobile overlay); `web/src/main.tsx` (`cockpitExpanded`, drawer safe-area).
- **Follow-ups:** Rebuild `web/dist` for deploy; smoke expanded cockpit + settings dialogs on a phone or narrow viewport.

### 2026-06-07 — Web UI craft polish (operator cockpit)

- **Area:** web UI (`web/`)
- **Summary:** Unified light/dark styling on `--mc-*` semantic tokens (`--mc-text-*`, `--mc-surface-*`, `--mc-border-strong`); removed decorative body gradients and heavy message/dock shadows; differentiated desktop user vs assistant bubbles; added `prefers-reduced-motion` for thinking dots; refactored expanded `CockpitBar` (signal row, two-column controls, collapsible bulletin); flattened sidebar persona list with SVG icon controls and grouped accent theme picker; removed unused `chat-timeline`, `message-markdown`, `lib/ui.ts`.
- **Key files / symbols:** `web/src/styles.css`; `web/src/components/cockpit-bar.tsx`, `session-sidebar.tsx`, `initial-run-prompt-view.tsx`, `settings-llm.tsx`; `web/src/main.tsx`.
- **Follow-ups:** Rebuild `web/dist` for deploy; smoke expanded cockpit + theme menu in both appearances.

### 2026-06-04 — Multi-bot Telegram/Discord: Channels tab, auto-link, scoped delivery

- **Area:** DB / channels / web API / web UI
- **Summary:** Settings → **Channels** lists every `channel_bot_instances` row for telegram/discord immediately (`list_contact_channel_integration_rows`, `GET /api/contacts/bindings`) with `linked` + optional handle. New bots auto-provision sibling `channel_bindings` on create and at startup (`provision_bindings_for_instance`, `provision_all_missing_sibling_bindings`). Extra instance ids are `>= 4`; migration moves misplaced rows off reserved ids 2–3. Outbound replies use `DeliveryScope::PlatformInstance` from inbound telegram/discord/whatsapp handlers (web/scheduler still `ContactWide`); messages always stored for web history. At most one bot per platform per contact may use effective “all personas” (`validate_all_persona_slot`). Telegram startup logs/`getMe` per instance; `telegram_enabled` when any DB telegram token exists.
- **Key files / symbols:** `src/db.rs` (`ContactChannelIntegrationRow`, `migrate_misplaced_channel_bot_instance_ids`); `src/channel.rs` (`DeliveryScope`); `src/channels/telegram.rs` (`reply_bot_instance_id`, dispatcher logs); `src/web.rs` (`api_contacts_bindings`, `api_channel_bot_instances_post`); `web/src/main.tsx`, `web/src/types.ts`.
- **Follow-ups:** Rebuild `web/dist`; restart gateway after adding integrations; per-bot `@username` for group mentions still uses `BOT_USERNAME` from env (primary only).

### 2026-06-04 — Web image delivery: repair fabricated `-bot-` upload URLs

- **Area:** web / delivery / final_delivery_media
- **Summary:** When the assistant emits `/api/uploads/.../YYYYMMDD-HHMMSS-bot-<artifact>` URLs that were never persisted (or points at a missing `-bot-` copy path), `materialize_response_file_links` now strips delivery-copy and hash prefixes and resolves the persona artifact (e.g. `PZ-20260608-PARK-HOTIFY-MEDIUM.png`) before re-uploading. Thread markdown renderers include explicit `img` styling.
- **Rationale:** Influencer_PZ_3 (persona 24) runs showed repeated “can't see the image” — LLM-invented upload URLs 404'd because fallback only matched exact basenames, not canonical artifact names under `shared/personas/997894126/24/`.
- **Key files / symbols:** `artifact_basename_fallback_candidates` in `src/final_delivery_media.rs`; `resolve_response_local_file_path` in `src/web.rs`; `web/src/components/thread-pane.tsx`, `message-markdown.tsx`.
- **Follow-ups:** Rebuild `web/dist` after deploy; existing stored chat rows with broken URLs stay broken until the user asks for a resend (materialization runs on new replies only).

### 2026-06-03 — LLM provider/model Web UI only (removed from .env)

- **Area:** config / web / main
- **Summary:** `LLM_PROVIDER` and `LLM_MODEL` are no longer read from or written to `.env`. Startup merges selection from `app_settings` (or auto-picks first provider with a key and persists defaults). `.env` holds API keys only (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, etc.). Config wizard CLI no longer prompts for provider/model.
- **Key files / symbols:** `src/config.rs` (`merge_llm_selection_from_app_settings`, `save_env`), `src/main.rs`, `src/llm_catalog.rs` (`first_provider_with_api_key`), `.env.example`, `web/src/components/settings-llm.tsx`.

### 2026-06-03 — Per-provider API keys (fix Gemini INVALID_ARGUMENT)

- **Area:** config / llm_catalog
- **Summary:** Stopped falling back to `LLM_API_KEY` for non-Anthropic providers. Google requires `GEMINI_API_KEY` or `GOOGLE_API_KEY` (`Config.gemini_api_key`). `LLM_API_KEY` / `ANTHROPIC_API_KEY` apply only to Anthropic.
- **Key files / symbols:** `src/llm_catalog.rs` (`provider_key_env_var_list`, `llm_api_key_applies_to_provider`); `src/config.rs` (`gemini_api_key`).

### 2026-06-03 — xAI Grok tool-call stop_reason fix

- **Area:** llm / agent loop
- **Summary:** xAI (and some OpenAI-compat gateways) return `finish_reason: "stop"` or `"completed"` while still populating `tool_calls`. `translate_oai_response` previously only mapped `tool_calls` → `tool_use`, so the agent loop treated tool turns as `end_turn` and dropped tool blocks. Now infers `tool_use` when response content includes `ToolUse` (same pattern as Gemini). Agent loop also forces `tool_use` when tool blocks are present regardless of stop reason.
- **Key files / symbols:** `src/llm.rs` (`oai_stop_reason_from_content`, `translate_oai_response`, OpenAI stream path, `build_stream_response`); `src/channels/telegram.rs` (stop_reason override before end_turn branch).

### 2026-06-03 — Web UI: switch LLM provider + model (keys in .env only)

- **Area:** web / config / llm
- **Summary:** Settings → LLM now has provider and model dropdowns. `LLM_PROVIDER` and `LLM_MODEL` persist in `app_settings`; API keys are read only from `.env` per provider (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `XAI_API_KEY`, `GEMINI_API_KEY`, etc.). `GET/PATCH /api/llm` return all curated providers with key status; `LlmHandle::apply_selection` hot-swaps provider, model, base URL, and resolved key.
- **Key files / symbols:** `src/llm_catalog.rs` (`APP_SETTING_LLM_PROVIDER`, `providers_catalog_json`, `resolve_api_key_for_provider`), `src/config.rs` (`merge_llm_selection_from_app_settings`, `apply_llm_provider_switch`), `src/llm.rs` (`apply_selection`), `src/web.rs`, `web/src/components/settings-llm.tsx`.
- **Follow-ups:** Rebuild `web/dist` after deploy.

### 2026-06-03 — Tier 2 memory: `sops[]` replaces `known_steps`

- **Area:** memory / agent prompt
- **Summary:** Restructured `tier2`: `known_steps` removed from persisted schema; new `sops[]` entries are `{ id, vault_path, summary }` with required `ORIGIN/…` paths. Legacy `known_steps` deserialize and migrate on `normalize`. Persona context renders **### SOPs (Tier 2)** with vault paths. `write_tiered_memory` Tier 2 format: `- SOP|id|vault_path|summary`.
- **Key files / symbols:** `SopPointer`, `Tier2Memory::sops` in `src/memory.rs`; `src/tools/tiered_memory.rs`; persona 24 `memory_state.json`
- **Follow-ups:** Bulk-migrate other personas on next read/write.

### 2026-06-03 — Rollback YAML workflow engine → vault SOPs

- **Area:** workflow / agent prompt / web / memory
- **Summary:** Removed deterministic authored workflows entirely (`src/workflow_engine/`, `run_workflow` tools, `create-workflow`/`modify-workflow` builtin skills, Settings → Workflows UI, `WORKFLOW_*` config). **Workflow** now means vault markdown SOPs with memory pointers; hook steer via `sop_context_gate.rs`. Migrated PZ pipeline to `ORIGIN/Operations/SOPs/PZ-Post-Pipeline.md`; persona 24 memory updated.
- **Rationale:** YAML executor caused pathing/identity drift and extra tool iterations; direct `run_skill_script` following a vault SOP was faster and more reliable (see agent history 20260603-200509 vs 194845).
- **Key files / symbols:** `src/sop_context_gate.rs`, `sops_prompt_sections` in `src/channels/telegram.rs`; deleted `src/workflow_engine/`, `src/tools/workflow.rs`; `docs/sops.md`; `.cursor/rules/sops.mdc`
- **Follow-ups:** Mine new SOP into Mem-Palace; tune `SOP_PHRASES` if steer is too broad.

### 2026-06-03 — Harden OpenAI and xAI (Grok) on OpenAI-compatible Chat Completions

- **Area:** LLM / config / doctor
- **Summary:** OpenAI and xAI use the shared `OpenAiProvider` (`/chat/completions`). `Config::post_deserialize` now normalizes `grok` → `xai`, fills `llm_base_url` and default model from `llm_catalog`, and falls back `LLM_API_KEY` from `OPENAI_API_KEY` (openai) or `XAI_API_KEY` (xai). `OpenAiProvider` resolves base URL via catalog; GPT-5/o-series and Grok reasoning models send `max_completion_tokens` (with one-shot 400 retry). Catalog adds `grok-4.3`, `gpt-5.4`. Doctor warns when `LLM_PROVIDER=xai` still points at the OpenAI default host.
- **Key files / symbols:** `src/llm_catalog.rs` (`resolve_catalog_provider_id`, `default_base_url_for_provider`), `src/config.rs` (`post_deserialize`), `src/llm.rs` (`oai_resolve_base_url`, `build_oai_chat_request_body`, `OpenAiProvider`), `src/doctor.rs` (`check_llm_provider_base_url`); presets in `src/config_wizard.rs`, `src/setup.rs`.
- **Follow-ups:** Native OpenAI/xAI Responses API deferred; live model list fetching not implemented.

### 2026-06-03 — PZ post pipeline YAML fix + deprecate learned workflows

- **Area:** workflow engine / config / docs
- **Summary:** Rewrote `workspace/workflows/pz-post-pipeline.workflow.yaml` to valid schema (`type: script` / `bash` / `deliver`) using allowed skills `image-generator` and `pz-hotify`. Added semantic validation in `lint_workflow_yaml_raw` / `validate_tool_input` (reject `args` on tool steps, empty tool `input`). Removed SQLite learned-workflow runtime surface: `WORKFLOW_AUTO_LEARN` config, dead DB APIs (`get_best_workflow_for_intent`, `upsert_workflow_learning`, `log_workflow_execution`), `AgentEvent::WorkflowSelected`. Docs/rules now point to authored workflows only (`docs/deterministic-workflows.md`; `docs/workflow.md` is a deprecation stub).
- **Key files / symbols:** `src/workflow_engine/schema.rs`, `workspace/workflows/pz-post-pipeline.workflow.yaml`, `src/config.rs`, `src/db.rs`, `builtin_skills/create-workflow/SKILL.md`, `.cursor/rules/authored-workflows.mdc`
- **Follow-ups:** Optional `pz-post-pipeline-publish` workflow for post-approval scheduling; drop legacy `workflows` DB tables in a future migration.

### 2026-06-03 — Authored workflows in system prompt (catalog + tool groups)

- **Area:** agent prompt / tools
- **Summary:** When `workflow_engine_enabled`, `build_system_prompt` now injects a **Tool groups** line for workflow tools, a prose block (run/create/modify), and a live **Workflows on disk** list from `WorkflowCatalog::list_entries`. Question-intent tool filter also exposes `list_workflows`, `read_workflow`, `validate_workflow`.
- **Rationale:** Model often confused authored YAML workflows with Tier 1 “workflow principles” or SQLite learned workflows; it had no workflow ids unless it called `list_workflows`.
- **Key files / symbols:** `authored_workflows_prompt_sections`, `build_system_prompt` in `src/channels/telegram.rs`; `definitions_filtered` in `src/tools/mod.rs`
- **Follow-ups:** Optional `AGENTS.md` routing line per persona; Phase 2 scheduler `[workflow:id]`.

### 2026-06-03 — Cross-channel image delivery normalization

- **Area:** channels / delivery / web
- **Summary:** Added `src/final_delivery_media.rs` with persona-aware `resolve_workspace_artifact_path` (searches `shared/personas/{chat}/{persona}/`, `runtime/groups/…`, and unique basename under persona tree) and `normalize_assistant_artifact_references` (bare image filename lines → `![basename](abs_path)`). `deliver_agent_final_to_contact` normalizes before store/deliver; web runs normalize then `materialize_response_file_links`. Telegram `prepare_telegram_workspace_auto_images` takes `WorkspaceAutoImageContext { root, chat_id, persona_id }` for scoped resolution and `send_photo` before text.
- **Rationale:** Assistants often emit bare basenames (e.g. `PZ-….png`) while artifacts live under persona cwd; prior delivery only detected markdown images and `shared/<file>`, so Telegram and web showed text only.
- **Key files / symbols:** `src/final_delivery_media.rs`; `src/channel.rs` (`normalize_final_for_delivery`, `deliver_agent_final_to_contact`); `src/channels/telegram.rs` (`WorkspaceAutoImageContext`, `prepare_telegram_workspace_auto_images`); `src/web.rs` (normalize + materialize order, `resolve_response_local_file_path`).
- **Follow-ups:** Materialize at store time when a contact has web bindings but the run was Telegram-only, if cross-channel history should always show `/api/uploads/…` inline images.

### 2026-06-02 — Pre-delivery PDQE gate + remove main-agent `send_message`

- **Area:** agent loop / channels / delivery / evaluators
- **Summary:** Removed `SendMessageTool` from the main agent registry so users receive a single user-visible reply per run. PDQE now runs synchronously in-loop via `finish_turn_with_quality_gate` before `AgentEvent::FinalResponse` and channel delivery; on fail with budget, injects `[quality_eval_feedback]` and continues the loop. Async corrective runs (`maybe_spawn_post_delivery_quality_eval`, `enqueue_quality_corrective_run_for_contact`) and `AgentProcessResult::post_delivery_eval` were removed. Persona focus sync runs only after PDQE pass. Tool trace for PDQE uses up to 48 messages from the current run (`build_pdqe_tool_trace` with `protected_message_count`). `send_message`-anchor final dedupe was dropped; `plan_agent_final_delivery` is always full unless empty body.
- **Rationale:** Mid-run `send_message` plus final reply and async PDQE corrective runs caused duplicate bubbles (especially PZ hotify). QC-first delivery should not show drafts or second full agent messages.
- **Key files / symbols:** `src/tools/mod.rs` (unregister tool; `send_message.rs` kept for internal/helpers); `src/channels/telegram.rs` (`finish_turn_with_quality_gate`, `try_finish_agent_turn`, `run_post_delivery_hooks_before_gate`, `build_system_prompt`); `src/response_quality_evaluator.rs`; `src/channel.rs`, `src/final_delivery_dedupe.rs`; `src/web.rs`, `src/channels/{discord,whatsapp}.rs`, `src/scheduler.rs`; `workspace/skills/send-attachment/SKILL.md`; `ARCHITECTURE.md`, `TEST.md` §11.
- **Follow-ups:** Optional shared `prepare_final_for_delivery` for non-web channels; tune `QUALITY_EVAL_*` latency vs false positives in production.

### 2026-06-02 — Deterministic authored workflows (tool + skill)

- **Area:** workflow engine / tools / skills / hooks / config
- **Summary:** Added YAML-based authored workflows with a Rust step executor (`tool`, `script`, `bash`, `set`, `deliver`). Exposed `list_workflows`, `read_workflow`, `write_workflow`, `validate_workflow`, and `run_workflow` tools; builtin skills `create-workflow` and `modify-workflow` with activation gates (mirror `modify-skill`). LLM invokes workflows via `run_workflow` in the normal agent loop — no pre-LLM shortcut.
- **Rationale:** Operators need repeatable step sequences without probabilistic tool ordering; skills teach authoring while tools validate, persist, and execute.
- **Key files / symbols:** `src/workflow_engine/` (`execute_workflow`, `WorkflowCatalog`), `src/tools/workflow.rs`, `src/workflow_activation_gate.rs`, `src/hook_runtime.rs` (`builtin_turn_skill_gate` workflow signals), `src/channels/telegram.rs` (registration + turn gates + system prompt), `builtin_skills/create-workflow/`, `builtin_skills/modify-workflow/`, `docs/deterministic-workflows.md`, `workspace/workflows/echo-demo.workflow.yaml`
- **Follow-ups:** Phase 2: `agent`/`when`/`foreach` steps; scheduler `[workflow:id]` unattended path; web cockpit list/run.

### 2026-06-02 — Web Settings: edit authored workflows

- **Area:** web / workflow engine
- **Summary:** Settings dialog adds a **Workflows** tab: list global/persona YAML workflows, edit in a textarea, validate, save, delete. REST: `GET/POST /api/workflows`, `GET/DELETE /api/workflows/:id`, `POST /api/workflows/validate`.
- **Key files / symbols:** `src/web.rs` (`api_workflows_*`, `workflow_catalog`), `src/workflow_engine/catalog.rs` (`delete`), `web/src/components/settings-workflows.tsx`, `web/src/main.tsx`

### 2026-06-02 — Post-delivery quality evaluation (PDQE) + Perplexity evaluators

- **Area:** agent loop / channels / config / evaluators
- **Summary:** Added async post-delivery QC after the first reply is delivered: Perplexity sidecar (`PERPLEXITY_API_KEY`, `EVALUATOR_MODEL`, `EVALUATOR_BASE_URL`) judges the answer against `[current_request]` (`SessionGoalContext`). On fail, one corrective foreground run per `run_key` via `ChatRunQueue` with `[quality_eval_feedback]`. PTE now uses the same evaluator provider and session goal. Derived `ask_clarification` stop reason skips deferred-commitment nudge and PDQE.
- **Rationale:** Catch incomplete or off-goal replies without blocking the first delivery; align evaluators with task-first context instead of first history message.
- **Key files / symbols:** `src/response_quality_evaluator.rs`, `src/agent_turn_context.rs`, `src/llm.rs` (`create_evaluator_provider`), `src/channels/telegram.rs` (`AgentProcessResult`, `maybe_spawn_post_delivery_quality_eval`, `enqueue_quality_corrective_run_for_contact`, `effective_stop_reason`); wired delivery spawn in web/discord/whatsapp/scheduler; `src/post_tool_evaluator.rs`; `src/hook_runtime.rs` (`builtin_deferred_commitment_guard`); config env in `src/config.rs`, `.env.example`, `src/doctor.rs`.
- **Follow-ups:** Tune `QUALITY_EVAL_MIN_CONFIDENCE` / channel allowlist from production false-positive rate; optional web timeline UI for `quality_eval_*` events. PDQE steps append to the same run's `agent_history/*.md` under `## Post-delivery quality evaluation` (`append_pdqe_step_to_agent_history`).

### 2026-06-02 — Strip inline `[bulletin_focus]` from assistant dialogue history

- **Area:** channels / message delivery / agent prompt
- **Summary:** Added `strip_embedded_bulletin_focus` to remove LLM-appended `[bulletin_focus]` appendix blocks from stored assistant messages at delivery time and when loading history for the model. Bulletin remains once in `[persona_context]` from DB; prior turns no longer replay stale operator snapshots.
- **Rationale:** The model was appending multiline bulletin cards to every assistant reply; those blocks were stored verbatim and repeated across all `prior_turn` messages, bloating context and duplicating the live bulletin.
- **Key files / symbols:** `src/channels/telegram.rs` (`strip_embedded_bulletin_focus`, `sanitize_bot_dialogue_content`, `history_to_claude_messages`); `src/channel.rs` (`deliver_and_store_bot_message`, `deliver_to_contact`, `deliver_agent_final_to_contact`); `src/tools/bulletin.rs` (tool description); system prompt Bulletin + memory sync line.
- **Follow-ups:** Existing DB rows are cleaned at read time; optional one-off DB migration if web UI should hide historical appendix blocks too.

### 2026-06-02 — Env-only secret redaction (no regex heuristics)

- **Area:** safety redaction / tools / channels / hooks
- **Summary:** Replaced `redact_secrets_internal` / `redact_secrets_user_visible` regex and long-token heuristics with `EnvSecretRedactor`: at startup, parse values from env-like files (`.env`, `.env.local`, `*.env`, etc.) under config root, workspace, and `builtin_skills/`, then redact only exact literal substrings (longest-first). LinkedIn URLs and other benign long tokens are no longer masked unless they exactly match an env value.
- **Rationale:** Heuristic redaction false-positived on URLs (`?token=`), assignment-like keys, and 40+ character path segments echoed in tool output; users saw `[REDACTED_SECRET]` in normal links.
- **Key files / symbols:** `src/safety_redaction.rs` (`EnvSecretRedactor::discover`, `redact`); `src/tools/path_guard.rs` (`pub fn is_env_like_name`); `AppState.env_redactor`; wired through `ToolRegistry::execute`, `apply_output_safeguards`, scheduler, TSA, PTE, hooks, background shell.
- **Follow-ups:** Restart bot after editing any env-like file to refresh the catalog; optional hot-reload if operators need it.

### 2026-06-02 — Omit assistant bookmarks from persona context

- **Area:** channels / agent prompt
- **Summary:** `format_bookmarks_section` now includes user-pinned user messages only; assistant bookmark previews are excluded from `[persona_context]` because bulletin focus and prior_turn assistant history already carry that context.
- **Rationale:** Bookmarked assistant snippets duplicated bulletin/status focus inside the background block on every run.
- **Key files / symbols:** `src/channels/telegram.rs` (`format_bookmarks_section`, Conversation Memory bullet in `build_system_prompt`).
- **Follow-ups:** None.

### 2026-06-02 — Task-first context restructuring

- **Area:** channels / agent prompt / memory rendering
- **Summary:** Split the triggering user message into a final `[current_request]` block; prior turns are tagged `context="prior_turn"`. `[persona_context]` gets a background-only banner; memory/bulletin section labels and system prompt **Task scope** clarify that the current ask is the primary goal. `workspace/AGENTS.md` adds turn-scope guidance.
- **Rationale:** Bulletin, memory, and history competed as implicit tasks; the model did extra work when the latest ask was indistinguishable from background context.
- **Key files / symbols:** `src/channels/telegram.rs` (`split_trailing_user_request`, `build_current_request_from_message`, `format_current_request_message`, `latest_user_text`, `build_persona_context_message`, `build_system_prompt`); `src/memory.rs` (`render_persona_context_memory_with_options` heading); `workspace/AGENTS.md`; `docs/architecture_review.md`.
- **Follow-ups:** Monitor whether operators want a cockpit toggle for minimal history tail (1+1) on task-heavy personas.

### 2026-06-02 — Per-persona agent queue lanes

- **Area:** queue / channels / web
- **Summary:** `ChatRunQueue` now uses one FIFO lane per `(chat_id, persona_id)` instead of per `chat_id`, so different personas in the same contact can run foreground agent turns concurrently. Same persona remains serialized. Diagnostics lanes include `persona_id`; web cockpit and queue dialog are persona-aware (optional “All personas” merge view).
- **Rationale:** Sessions, history, memory, and tool cwd were already per persona; chat-wide serialization forced Persona B to wait on Persona A’s long runs.
- **Key files / symbols:** `src/chat_queue.rs` (`QueueLaneKey`, `PersonaLane`, unit tests); `src/web.rs` (`api_queue_diagnostics`); `web/src/api/ops-fetch.ts`, `web/src/hooks/use-ops-poll.ts`, `web/src/main.tsx`, `web/src/components/cockpit-bar.tsx`; `src/channels/telegram.rs` (enqueue comment).
- **Follow-ups:** Background handoff still limits one active job per chat (`count_active_background_jobs_for_chat`); split by persona if operators need parallel background work across personas.

### 2026-06-01 — Message dates in web UI and bot history context

- **Area:** web / channels / agent prompt
- **Summary:** Web thread timestamps now show date when the message is not from today (with full datetime on hover). Chat history fed to the LLM includes ISO `at` on `<user_message>` and `<assistant_message>` wrappers so the agent can tell when each stored turn was sent.
- **Rationale:** UI only showed clock time; the model had order and “now” but not per-message dates unless it searched or exported.
- **Key files / symbols:** `web/src/lib/format-message-time.ts`, `web/src/components/thread-pane.tsx`, `src/channels/telegram.rs` (`format_user_message`, `format_assistant_history_message`, `history_to_claude_messages`, system prompt bullet).
- **Follow-ups:** Optional day-separator chips in the thread viewport if operators want stronger visual grouping.

## Template (copy per entry)

```markdown
### YYYY-MM-DD — Short title

- **Area:** e.g. channels / scheduler / agent / infra
- **Summary:** What changed in one or two sentences.
- **Rationale:** Why (problem, tradeoff, constraint).
- **Key files / symbols:** Paths and notable functions or types.
- **Follow-ups:** Optional; known gaps or next steps.
```

### 2026-05-31 — Hardened skills path resolution across bash/glob and hook command cwd

- **Area:** tools / hooks / prompt-path discipline
- **Summary:** Added skill/runtime path remapping for bash launcher commands and glob patterns so persona-scoped cwd no longer sends `skills/...` lookups into `shared/personas/...`. Command hooks now default to workspace root cwd (instead of `shared/`) so relative `skills/...` probes resolve to canonical workspace paths unless a hook explicitly requests `cwd: "shared"`.
- **Rationale:** Recent runs showed `activate_skill` returning the right skill directory while subsequent bash/glob calls searched under persona folders; hook command defaults had a parallel risk for relative skill paths.
- **Key files / symbols:** `src/tools/bash.rs` (`maybe_rewrite_leading_tool_path`), `src/tools/glob.rs` (`split_workspace_prefixed_pattern`), `src/hook_executor.rs` (`execute_command_hook` default cwd), `src/agent_path_discipline.rs` (persona-cwd wording), `src/channels/telegram.rs` (caps shell-cwd bullet), plus new tests in `bash.rs`, `glob.rs`, and `hook_executor.rs`.
- **Follow-ups:** Consider adding a first-class `run_skill_script` tool to remove shell path parsing ambiguity entirely, and enrich bash error text when it detects persona `.../skills/...` misses.

### 2026-05-31 — Persona-centric hook availability UI (skills-style)

- **Area:** hooks / web / frontend
- **Summary:** Extended `GET /api/hooks` with optional `persona_id` to return per-hook `scoped_for_persona`, `allowed_for_persona`, and `active_for_persona` fields. Updated the Hooks & Skills settings panel to remove the Global/Persona scope editor and instead present a persona-relative hooks catalog, mirroring the skills UX (Restrict hooks to selected + Save policy).
- **Rationale:** Operators care about “is this hook available for the persona I’m editing right now?”, and the previous Global/Persona toggle was confusing and redundant.
- **Key files / symbols:** `src/web.rs` (`HooksQuery`, `api_hooks_get`, `hook_definition_to_json`), `src/db.rs` (`HookDefinitionRecord::scoped_for_persona/persona_status`, `test_hook_persona_status_fields`), `web/src/components/settings-hooks-skills.tsx` (hooks catalog + removal of scope/global toggles), `web/src/types.ts` (new optional fields), `web/src/web.rs` tests for persona fields (added `test_api_hooks_get_persona_fields`), `docs/persona-hook-skill-policy.md` (document `GET /api/hooks?persona_id=:id`).
- **Follow-ups:** Rebuild `web/dist` as part of the deploy flow and consider adding multi-persona scope editing for agent-created hooks if ever needed.

### 2026-05-30 — Persona-scoped hook definitions with creator-default registration

- **Area:** hooks / db / tools / web
- **Summary:** Added hook-level scope (`scoped_persona_ids_json`) so custom hooks can be restricted to explicit persona ids while shipped `builtin_*` hooks remain global. Hook dispatch now enforces scope before persona allowlist/matcher checks. New `register_hook` tool defaults new hooks to the caller persona unless `global: true` is set.
- **Rationale:** Global hook registration with default allow-all persona policy let one persona's hook unexpectedly affect other personas.
- **Key files / symbols:** `src/db.rs` (`HookDefinitionRecord.scoped_persona_ids`, `migrate_hook_policy_schema`, `upsert_hook_definition`, `get_hook_definition`), `src/hook_runtime.rs` (`hook_scope_matches_persona`, `run_hooks_for_event_async`), `src/tools/register_hook.rs`, `src/tools/mod.rs`, `src/web.rs` (`HookDefinitionUpsertBody`, `api_hooks_get`, `api_hooks_post`), `src/builtin_hooks.rs` (force shipped hooks to global scope), `web/src/components/settings-hooks-skills.tsx`, `web/src/types.ts`, `builtin_skills/create-hook/SKILL.md`, `docs/hooks-architecture.md`.
- **Follow-ups:** Expand UI scope editor to support multi-persona assignment by name (current quick actions are global or active persona).

### 2026-05-29 — Shipped hooks catalog in `builtin_hooks/*.hook.json` (PZ not shipped)

- **Area:** hooks / db / docs
- **Summary:** Shipped policy hooks are five `*.hook.json` manifests under repository `builtin_hooks/` (present on fresh clone). `sync_shipped_hook_definitions` upserts them into SQLite on migrate. Removed hardcoded SQL seeds, template hooks, and PZ from the shipped catalog. PZ/optional command hooks belong under `{WORKSPACE_DIR}/hooks/` only.
- **Rationale:** Fresh installs should ship hook definitions as files in `builtin_hooks/`, not only as Rust INSERTs; PZ is persona-specific and must not be treated as a built-in shipped hook.
- **Key files / symbols:** `src/builtin_hooks.rs` (`sync_shipped_hook_definitions`, `load_shipped_manifests`); `builtin_hooks/*.hook.json`; `src/db.rs` (`ensure_builtin_hook_definitions`); `docs/hooks-architecture.md`.

### 2026-05-29 — Persona-scoped shared cwd with global skill artifacts

- **Area:** tools / web / startup migration / prompt-path discipline
- **Summary:** Switched tool working directories from flat `shared/` to persona-scoped `shared/personas/{chat_id}/{persona_id}/` using auth context, while keeping shared access for `ORIGIN/`, `vault_db/`, `.venv-vault/`, `skills/`, and `shared/skills/`. Added path-jail enforcement to block persona cross-read/write and reject writes to deprecated flat `shared/scripts/` and `shared/parking/`.
- **Rationale:** Personas were leaking into each other’s ad-hoc files via broad glob/grep from shared root. Shared skill code remains intentionally global and discoverable across personas.
- **Key files / symbols:** `src/tools/mod.rs` (`persona_shared_dir`, `resolve_tool_working_dir_for_auth`, `resolve_tool_path`, `assert_persona_tool_path_allowed`); tool call sites in `src/tools/{read_file,write_file,edit_file,apply_search_replace,symbol_edit,glob,grep,bash,browser,spawn_background_command,read_repo_map,cursor_agent,search_vault}.rs`; `src/channels/telegram.rs` (`process_with_agent_with_events`, prompt workspace path bullets); `src/web.rs` (persona-scoped upload paths and URL/tool_path generation); `src/persona_shared_migrate.rs`; `src/main.rs`; `src/doctor.rs`; `src/skill_activation_gate.rs`.
- **Follow-ups:** Legacy skill docs that still mention flat `shared/scripts/...` should be incrementally updated to `skills/<skill>/...` or `shared/skills/<skill>/...`.

### 2026-05-28 — Migrate remaining inline guards into built-in hooks

- **Area:** hooks / agent loop / prompt context
- **Summary:** Migrated remaining deterministic inline policies to hook-owned built-ins: scheduled-run policy context, turn skill gating, deferred-commitment pre-stop guard, and post-tool-batch loop guard. Removed scheduled policy prefix injection and switched guard routing to `runtime_signals` passed through `HookRunInput`.
- **Rationale:** Complete prompt-to-hook migration so recurring policy behavior is centralized in lifecycle hooks, reducing prompt bloat and duplicated inline branches.
- **Key files / symbols:** `src/hook_runtime.rs` (`HookRunInput::runtime_signals`, builtin action handlers), `src/db.rs` (`ensure_builtin_hook_definitions`, action-type validation), `src/channels/telegram.rs` (PreToolUse/PreStop/PostToolBatch hook signal wiring; removed scheduler policy prepend), `docs/hooks-architecture.md`.
- **Follow-ups:** If operator configurability is needed, decide which built-in policy hooks should remain always-on vs persona-allowlist-gated.

### 2026-05-28 — PostDelivery focus sync moved under hook runtime + prompt prefix trim

- **Area:** agent / hooks / prompt context / memory
- **Summary:** Converted post-delivery persona focus sync to a built-in hook action (`builtin_persona_focus_sync`) and wired delivery exits through a shared post-delivery pipeline that runs hook contexts and focus sync only when the hook triggers. Removed synthetic assistant acknowledgment prefix turns, filtered `send_message` out of scheduled-run tool definitions, and trimmed duplicated scheduler/skill/persistence prompt prose.
- **Rationale:** Reduce recurring token overhead and move deterministic behavior ownership from prompt narration/inline orchestration to hook/runtime contracts.
- **Key files / symbols:** `src/channels/telegram.rs` (`run_post_delivery_hooks_and_builtin_focus_sync`, scheduled tool filtering, prefix cleanup, `build_system_prompt`, `build_persona_context_message`); `src/hook_runtime.rs` (`HookRunResult::run_persona_focus_sync`, `builtin_persona_focus_sync` branch); `src/db.rs` (`ensure_builtin_hook_definitions`, `upsert_hook_definition` action validation); `src/tools/send_message.rs` (`test_send_message_blocks_scheduled_same_chat`); `docs/hooks-architecture.md`; `docs/memory-framework.md`; `DEVELOP.md`.
- **Follow-ups:** Consider whether persona hook allowlists should be able to disable `postdelivery-persona-focus-sync` or whether it should remain always-on framework behavior.

### 2026-05-28 — Hook observability in run history and latest run trace

- **Area:** agent loop / web streaming / run history
- **Summary:** Added explicit hook execution observability to the shared agent event stream, DB run timeline (`run_timeline_events`), and persisted agent-history markdown iteration traces. Hook entries now include lifecycle event, optional tool name, matched hook IDs, block reason (if any), and added-context count.
- **Rationale:** Operators need to see why hooks influenced a run (or blocked actions) when reviewing “Last agent run” and historical run traces, not just infer hook effects from downstream behavior.
- **Key files / symbols:** `src/channels/telegram.rs` (`AgentEvent::Hook`, `publish_hook_event_observability`, `hook_event_summary`, `run_post_delivery_hooks_and_builtin_focus_sync`), `src/agent_history.rs` (`IterationRecord.hook_events`, markdown rendering), `src/web.rs` (stream forwarding for `hook` events), `src/job_heartbeat.rs` (hook progress signal mapping).
- **Follow-ups:** Optionally add a dedicated hook section/filter in the web run viewer so hook events can be toggled independently from tool lines.

### 2026-05-28 — `modify-skill` builtin + mandatory activation for skill updates

- **Area:** skills / agent loop / builtin_skills
- **Summary:** Added `builtin_skills/modify-skill/SKILL.md` and runtime gate (`skill_activation_gate`) so `build_skill` (when the skill already exists) and file edits under `skills/<name>/` require `activate_skill` `modify-skill` in the same turn—mirroring `schedule-job` enforcement. System prompt and `create-skill` now route updates to `modify-skill`.
- **Rationale:** Skill edits were easy to do ad hoc without reading current SKILL.md or path discipline; a dedicated modify flow reduces shadow-tree mistakes and frontmatter drift.
- **Key files / symbols:** `builtin_skills/modify-skill/SKILL.md`; `src/skill_activation_gate.rs` (`requires_modify_skill_activation`, `REQUIRED_MODIFY_SKILL`); `src/channels/telegram.rs` (turn-local `modify_skill_activated_this_turn`, `skill_required` deny); `builtin_skills/create-skill/SKILL.md`; `src/tools/cursor_agent.rs` (`BuildSkillTool` description).
- **Follow-ups:** Optionally enforce `create-skill` activation for brand-new `build_skill` calls the same way.

### 2026-05-28 — Hook runtime expanded to command/prompt executor model

- **Area:** hooks / agent loop / db / web / config
- **Summary:** Reworked hook evaluation to support Cursor-style `command` and `prompt` hooks with structured JSON I/O, richer hook input context, command path allowlists, and optional tool-input rewrite/memory effects. Replaced framework-level `pz_terminal_cleanup` action type with a disabled built-in command hook script (`builtin_hooks/pz-terminal-cleanup.py`) and Rust-side validated effect application.
- **Rationale:** The previous deterministic runtime (`block`/`add_context` plus special `pz_terminal_cleanup`) was not extensible and encoded persona-specific behavior as a core action type. The new model keeps framework primitives generic and moves edge-case behavior into normal hooks.
- **Key files / symbols:** `src/hook_executor.rs` (`execute_command_hook`, `execute_prompt_hook`, `resolve_hook_command_path`, `parse_hook_output`), `src/hook_runtime.rs` (`run_hooks_for_event_async`, `HookRunInput`, `HookMemoryEffects`), `src/hook_actions.rs` (`apply_hook_memory_effects`), `src/channels/telegram.rs` (async hook dispatch + pre/post tool integration), `src/db.rs` (`ensure_builtin_hook_definitions`, `upsert_hook_definition` validation), `src/web.rs` (`api_hooks_post` command payload validation), `src/config.rs` (`HOOK_*` config), `docs/hooks-architecture.md`, `builtin_skills/create-hook/SKILL.md`, `builtin_hooks/pz-terminal-cleanup.py`.
- **Follow-ups:** Add richer operator-facing handling for hook `ask` semantics and optional Windows PowerShell companion for `builtin_hooks/pz-terminal-cleanup.py` if shell parity requirements expand.

### 2026-05-28 — PostToolUse PZ cleanup registered as DB hook

- **Area:** hooks / memory / db
- **Summary:** Moved terminal PZ post Tier 3 hygiene from hardcoded PostToolUse logic into an enabled built-in hook (`posttool-pz-terminal-cleanup`, action `pz_terminal_cleanup`). Extraction and memory writes live in `hook_actions`; the agent path applies side effects when the hook matches.
- **Rationale:** Keeps deterministic cleanup visible in the hook catalog and persona allowlists instead of hidden inline code.
- **Key files / symbols:** `src/hook_actions.rs` (`extract_terminal_pz_post_ids`, `apply_post_tool_hook_side_effects`); `src/hook_runtime.rs` (`HookRunResult::pz_terminal_cleanup`); `src/db.rs` (`ensure_builtin_hook_definitions`); `src/channels/telegram.rs` (PostToolUse dispatch); `docs/hooks-architecture.md`.
- **Follow-ups:** None.

### 2026-05-28 — Hooks settings made read-only + default hook templates seeded

- **Area:** web / hooks / db
- **Summary:** Simplified Hooks & Skills settings by removing hook CRUD controls from the UI and keeping hook catalog visibility + persona assignment only. Added DB bootstrap seeding for three disabled template hook definitions so new installs immediately show existing hooks.
- **Rationale:** Operators found in-UI hook authoring confusing; hook creation should be agent-driven via the `create-hook` skill while Settings mirrors the skills-style catalog/assignment workflow.
- **Key files / symbols:** `web/src/components/settings-hooks-skills.tsx` (removed create/toggle/delete controls, added read-only hook metadata note), `src/db.rs` (`seed_default_hook_definitions`, startup call in `Database::new`).
- **Follow-ups:** Optionally expose a dedicated “Run create-hook skill” action in Settings for guided hook authoring without direct form editing.

### 2026-05-28 — Tier 2 knowledge schema + bulletin-only active focus

- **Area:** memory / tools / agent prompt / docs
- **Summary:** Refactored Tier 2 from `active_projects` to a durable knowledge layer (`user_terminology`, `known_steps`, `preferences`) and aligned runtime prompts so bulletin remains the canonical active/recent focus source. Added legacy `active_projects` migration with event logging and updated tiered-memory read/write grammar.
- **Rationale:** Active execution status in Tier 2 duplicated bulletin and caused drift. Splitting responsibilities hardens memory semantics: bulletin for active focus, Tier 2 for durable reusable knowledge.
- **Key files / symbols:** `src/memory.rs` (`Tier2Memory`, `Tier2LegacyMigrationStats`, `PersonaMemoryState::normalize`, `read_persona_memory_state`, `render_persona_context_memory_with_options`, `render_memory_markdown`), `src/tools/tiered_memory.rs` (`parse_tier_content`, `parse_tier2_knowledge_lines`, `apply_tier_write`), `src/channels/telegram.rs` (`build_system_prompt`, `run_persona_focus_sync_after_delivery`, `apply_deterministic_persona_memory_hygiene`), `docs/memory-framework.md`, `DEVELOP.md`, `workspace/AGENTS.md`.
- **Follow-ups:** Consider stricter validation for malformed Tier 2 lines and a dedicated migration report endpoint for large persona fleets.

### 2026-05-28 — Hooks/skills settings: checkbox allowlists (no typing)

- **Area:** web / settings
- **Summary:** Replaced comma-separated text fields for persona hook/skill policy with restriction toggles, checkboxes on hook rows and the skills catalog, select-all/clear, and skill search filter. Hook creation uses payload presets via dropdown (JSON still editable).
- **Rationale:** Operators had to remember numeric hook IDs and exact skill slugs; error-prone and unnecessary when the catalog is already loaded from the API.
- **Key files / symbols:** `web/src/components/settings-hooks-skills.tsx` (`restrictHooks`, `restrictSkills`, `selectedHookIds`, `selectedSkillNames`, `HOOK_PAYLOAD_PRESETS`); `web/dist/` (rebuilt).
- **Follow-ups:** None.

### 2026-05-28 — Bulletin-first persona context + lifecycle focus sync

- **Area:** agent / memory / bulletin / skills
- **Summary:** Persona context now reads bulletin focus into `[persona_context]`, filters Tier 2 prompt rendering to in-flight projects, and can suppress Tier 3 when bulletin exists. Replaced post-response memory maintenance with a post-delivery lifecycle focus-sync hook (hybrid: deterministic hygiene always, bounded LLM sync only on non-trivial turns) and added deterministic PostToolUse cleanup for terminal PZ post IDs.
- **Rationale:** Persona 24 showed triple redundancy (Tier 2 + Tier 3 + bulletin), stale terminal items marked `active`, and write-only bulletin continuity gaps. The new flow makes bulletin the canonical episodic focus while reducing token bloat and persistence drift.
- **Key files / symbols:** `src/channels/telegram.rs` (`format_bulletin_focus_section`, `build_persona_context_message`, `build_system_prompt`, `run_persona_focus_sync_after_delivery`, `apply_deterministic_persona_memory_hygiene`, `extract_terminal_pz_post_ids`); `src/memory.rs` (`render_persona_context_memory_with_options`, terminal project filtering); `workspace/AGENTS.md`; `workspace/skills/pz-publisher/SKILL.md`; `workspace/skills/pz-inventory-manager/SKILL.md`; `docs/memory-framework.md`; `DEVELOP.md`; `workspace/runtime/groups/997894126/24/memory_state.json`.
- **Follow-ups:** Consider a strict status vocabulary validator in `write_tiered_memory` and optional `PostToolUse` hook criteria tuning beyond PZ-style post IDs.

### 2026-05-28 — Frontend design Cursor rule (from agent-skills)

- **Area:** web / cursor rules
- **Summary:** Added `.cursor/rules/frontend-design.mdc` — file-scoped guidance for `web/**/*.{tsx,css,html}` adapted from [frontend-design-principles](https://github.com/joshuadavidthomas/agent-skills/blob/main/frontend-design-principles/SKILL.md). Covers intent/domain/signature checks, craft principles, app vs marketing routing, anti-patterns, and alignment with existing `--mc-*` tokens.
- **Rationale:** Reduce generic AI dashboard defaults when building or reviewing the operator cockpit UI.
- **Key files / symbols:** `.cursor/rules/frontend-design.mdc`; `web/src/styles.css` (`--mc-*`).

### 2026-05-28 — Settings skills catalog lists all discovered skills

- **Area:** web / skills API
- **Summary:** `GET /api/skills` returns every skill with valid `SKILL.md` plus `remote` (API/cross-platform) and `allowed_for_persona`. Removed local `deps` binary checks from discovery/activation; settings label is **remote skill**, not unavailable on host.
- **Rationale:** Many skills are API-backed; missing local CLIs should not hide or block them. Operators need the full catalog for allowlists.
- **Key files / symbols:** `src/skills.rs` (`discover_all_skills`, `skill_availability`); `src/web.rs` (`api_skills_get`); `web/src/components/settings-hooks-skills.tsx`.
- **Follow-ups:** Shadow-tree skills still require migration to `workspace/skills/` to appear.

### 2026-05-28 — Strict path discipline in agent system prompts

- **Area:** agent / prompts / skills
- **Summary:** Added a dedicated **Path discipline (strict)** section to `build_system_prompt` (allowed/forbidden path table, shadow-workspace warning, skills checklist). Shared text lives in `src/agent_path_discipline.rs` and is also appended to `build_skill` cursor-agent prompts. Updated `workspace/AGENTS.md` and `builtin_skills/create-skill/SKILL.md`.
- **Rationale:** Personas were still writing skills under `shared/workspace/skills/` via `workspace/skills/...` prefixes in bash and file tools; file-tool guards alone do not cover bash or detached cursor-agent runs.
- **Key files / symbols:** `src/agent_path_discipline.rs` (`strict_path_discipline_section`, `build_skill_path_discipline_footer`); `src/channels/telegram.rs` (`build_system_prompt`); `src/tools/cursor_agent.rs` (`BuildSkillTool`); `workspace/AGENTS.md`; `builtin_skills/create-skill/SKILL.md`.
- **Follow-ups:** Optional code enforcement: bash shadow-path rejection; cursor-agent cwd at workspace root.

### 2026-05-28 — Remove learned-workflow memory influence from agent runs

- **Area:** agent loop / memory
- **Summary:** Removed post-run learned-workflow persistence/promotion from the shared agent path and stopped rendering `tier1.workflow_principles` into the active memory prompt block.
- **Rationale:** Learned workflow hints were adding stale/overfit execution guidance that could steer the bot away from current user intent.
- **Key files / symbols:** `src/channels/telegram.rs` (`save_run_history!`), `src/memory.rs` (`append_tier1_sections`).
- **Follow-ups:** Existing `tier1.workflow_principles` and SQL `workflows` rows remain on disk for audit/backward compatibility but no longer affect run-time prompting.

### 2026-05-27 — Restrict grep tool to workspace root

- **Area:** tools / security
- **Summary:** Added a hard workspace-root boundary in the `grep` tool so absolute/external paths are rejected before traversal. The tool now resolves the candidate path and blocks searches outside the configured workspace data root (`workspace/`), preventing accidental whole-disk scans.
- **Rationale:** Agents could pass absolute paths to `grep` and recursively scan outside project scope, causing slow runs and unnecessary exposure beyond intended workspace files.
- **Key files / symbols:** `src/tools/grep.rs` (`GrepTool::execute`, `is_path_within_workspace_root`, workspace-boundary tests).
- **Follow-ups:** If needed, apply the same workspace-root guard pattern to other read/search tools that currently accept absolute paths.

### 2026-05-27 — Deterministic hook runtime + persona hook/skill policy in Web UI

- **Area:** agent loop / hooks / db / web / skills
- **Summary:** Added a deterministic hook runtime across turn/tool/stop/delivery boundaries with DB-backed hook definitions, plus Web API/UI to manage hooks and per-persona hook/skill allowlists. Default policy remains allow-all for backward compatibility.
- **Rationale:** Operators need non-LLM automation controls and persona-scoped capability governance without consuming agent turn context, while preventing bulletin updates from being mistaken as user delivery.
- **Key files / symbols:** `src/hook_runtime.rs` (`HookEventName`, `run_hooks_for_event`); `src/channels/telegram.rs` (hook dispatch at `BeforeTurn`/`PreToolUse`/`PostToolUse`/`PostToolBatch`/`PreStop`/`PostDelivery`, delivery recovery helpers); `src/db.rs` (`hook_definitions`, `persona_hook_skill_policy`, policy/query helpers); `src/tools/activate_skill.rs` (persona policy enforcement); `src/tools/mod.rs` (activate-skill tool wiring with DB); `src/web.rs` (`/api/hooks`, `/api/skills`, `/api/personas/:id/policy`); `web/src/components/settings-hooks-skills.tsx`; `web/src/main.tsx`; `web/src/types.ts`; `docs/hooks-architecture.md`; `docs/persona-hook-skill-policy.md`; `docs/workflow.md`.
- **Follow-ups:** Re-run `cargo fmt --all --check` and `cargo clippy -- -D warnings` after shell environment recovery; add richer hook action types/UI validation as needed.

### 2026-05-26 — Faster web image uploads + drag-and-drop

- **Area:** web / channels
- **Summary:** Web attachments now upload via `POST /api/uploads` (multipart, streamed to disk) before a small JSON `POST /api/send_stream` with `tool_path`/`url` refs. Composer supports drag-and-drop (`ComposerPrimitive.AttachmentDropzone`), upload status hints, and clearer 413 errors. Axum body limits scale with `MAX_DOCUMENT_SIZE_MB`. Reference attachments skip re-decode/re-write; first image is read from disk once for vision.
- **Rationale:** 10 MB images were slow because base64-in-JSON inflated payload size and caused multiple encode/decode passes; drag-and-drop was not wired in the UI.
- **Key files / symbols:** `src/web.rs` (`api_upload`, `process_web_attachments`, `web_json_body_limit_bytes`); `web/src/lib/attachments.ts`; `web/src/components/thread-pane.tsx` (`DraftAwareComposer` dropzone); `web/src/main.tsx` (multipart-first send path); `web/src/api/client.ts` (`apiForm`).
- **Follow-ups:** Rebuild `web/dist` after UI changes; optional removal of legacy inline `data_base64` path once all clients migrate.

### 2026-05-22 — Gate background-shell chat output on pipeline logging

- **Area:** background_shell / runtime_toggles / web
- **Summary:** When `tool_output_debug` is off, `finalize_shell_job` no longer appends stdout/stderr to user-facing completion messages; full output remains in DB and agent success/retry handoff prompts. Settings help text updated.
- **Rationale:** Pipeline logging is a gateway UX control, not per-script `pz_log` discipline; users should not see raw shell dumps when debug is off.
- **Key files / symbols:** `background_shell::{format_delivery_message, finalize_shell_job}`; `web/src/components/settings-runtime.tsx`.
- **Follow-ups:** Rebuild `web/dist`; optional: gate in-loop `bash` tool echoes similarly if needed.

### 2026-05-22 — Suppress run_pz_i2i_pure queue poll spam when debug off

- **Area:** workspace scripts
- **Summary:** `run_pz_i2i_pure` ComfyUI queue/WebSocket wait lines (e.g. repeated “Waiting for ComfyUI job … RUNNING”) now use `debug_only=True` via `pz_log`, matching other PZ scripts. Default-off pipeline logging no longer floods background-job completion messages.
- **Rationale:** UI toggle only affects scripts that honor `TOOL_OUTPUT_DEBUG`; this script left poll status at default visibility.
- **Key files / symbols:** `workspace/shared/run_pz_i2i_pure.py` (`_maybe_log_queue_wait`, `wait_for_prompt_result`, `_recv_ws_message`).
- **Follow-ups:** Wrapper scripts (`pz_v8_i2i_pipeline.py`, etc.) still print Gemini/scene lines unconditionally if those should be gated too.

### 2026-05-22 — Pipeline logging toggle copy (global scope)

- **Area:** web
- **Summary:** Settings → Overview “Pipeline logging” switch label/help text now describes a gateway-wide verbose shell setting, not PZ/face-swap-only. Behavior unchanged (`TOOL_OUTPUT_DEBUG` / `tool_output_debug`).
- **Rationale:** The toggle was always global; PZ-specific wording implied a single-persona feature.
- **Key files / symbols:** `web/src/components/settings-runtime.tsx`; rebuild `web/dist`.
- **Follow-ups:** None.

### 2026-05-21 — Increase long-running tool timeouts to 1 hour

- **Area:** agent loop / tools / config
- **Summary:** Raised the main in-loop tool execution timeout and default `bash`/`cursor_agent` timeout defaults from 1500s to 3600s; updated `.env.example` and config-wizard defaults to match.
- **Rationale:** ComfyUI/PZ recovery and generation flows can exceed 25 minutes under load; default 1500s cut off legitimate long-running jobs before they completed.
- **Key files / symbols:** `src/channels/telegram.rs` (`TOOL_EXECUTION_TIMEOUT_SECS`), `src/tools/bash.rs` (`timeout_secs` schema + default), `src/config.rs` (`default_cursor_agent_timeout_secs`, `cursor_agent_timeout_secs`), `src/config_wizard.rs`, `.env.example`.
- **Follow-ups:** Restart running bot processes so the new defaults are loaded from code/env.

### 2026-05-20 — Web UI toggle for PZ/ComfyUI debug logging

- **Area:** web / runtime_toggles / tools
- **Summary:** Settings → Overview includes **PZ / ComfyUI debug logging** switch (`GET`/`PATCH /api/runtime`). Value persists in `app_settings` (`TOOL_OUTPUT_DEBUG`) and hot-updates `RuntimeToggles` for bash and background shell without restart.
- **Rationale:** Operators should not edit `.env` for a UI-facing verbosity flag.
- **Key files / symbols:** `runtime_toggles::{RuntimeToggles, APP_SETTING_TOOL_OUTPUT_DEBUG}`; `web::{api_runtime_get, api_runtime_patch}`; `web/src/components/settings-runtime.tsx`.
- **Follow-ups:** Rebuild `web/dist` and restart gateway.

### 2026-05-20 — PZ/ComfyUI verbose logging behind `TOOL_OUTPUT_DEBUG`

- **Area:** config / tools / workspace scripts
- **Summary:** WebSocket timeout / history-poll lines from `run_pz_*` and `comfy_swap_cli.py` are suppressed unless debug is on. Bot sets `TOOL_OUTPUT_DEBUG=1` when `tool_output_debug` is true (env `TOOL_OUTPUT_DEBUG=true`). Shared `workspace/shared/pz_log.py`; scripts accept `--debug` for manual runs.
- **Rationale:** Long ComfyUI waits spam bash output; milestones (queued, saved) stay visible.
- **Key files / symbols:** `config::tool_output_debug`; `command_runner::apply_tool_output_debug_env`; `bash` / `background_shell` env injection; `pz_log::{is_debug, pipeline_log}`.
- **Follow-ups:** Restart bot after enabling in `.env`.

### 2026-05-20 — Longer background job lease defaults

- **Area:** config / background_jobs / scheduler
- **Summary:** Raised `background_job_lease_ttl_secs` default 120→1800, fallback renew 180→60, pending-start stale threshold 120→300. Documented env vars in `.env.example`.
- **Rationale:** Long ComfyUI/bash tool calls emit no agent events for many minutes; lease expired at 120s while fallback renewal ticked every 180s, so `reconcile_expired_background_job_leases` could mark GPU jobs failed mid-run.
- **Key files / symbols:** `config::{default_background_job_lease_ttl_secs, default_background_job_lease_fallback_renew_secs}`; `job_heartbeat::spawn_shared_heartbeat`; `scheduler::run_due_tasks` → `reconcile_expired_background_job_leases`.
- **Follow-ups:** Restart bot after deploy; override via `BACKGROUND_JOB_LEASE_TTL_SECS` / `BACKGROUND_JOB_LEASE_FALLBACK_RENEW_SECS` if jobs routinely exceed 30m.

### 2026-05-20 — Persona-scoped schedules and chat history

- **Area:** tools / web / db / channels
- **Summary:** Scheduled-task list/mutate tools and chat-history surfaces now enforce caller persona (control chats list all personas in a chat for schedules). `GET /api/history` and `/api/history/days` require `persona_id`; web `loadHistory` always sends it. `search_chat_history` uses `authorize_chat_persona_access`. `send_message` cannot override `persona_id` to another persona unless control chat. Slash `/schedule` filters to active persona per channel.
- **Rationale:** Multi-persona chats should not leak another persona's cron jobs or message history to agents or the wrong sidebar thread.
- **Key files / symbols:** `src/db.rs` (`get_scheduled_tasks_for_chat_and_persona`, `get_tasks_for_chat_and_persona`); `src/tools/schedule.rs` (`list_schedules_for_caller`, `authorize_scheduled_task_access`); `src/tools/search_history.rs`; `src/tools/send_message.rs` (`resolve_send_message_persona_id`); `src/web.rs` (`resolve_history_persona_id`); `web/src/main.tsx` (`loadHistory`); `src/channels/{telegram,discord,whatsapp}.rs` (`SlashCommand::Schedule`).
- **Follow-ups:** Optional `cursor_agent_runs.persona_id` if run listing should match persona isolation.

### 2026-05-20 — Internal redaction: preserve long basenames before image/media extensions

- **Area:** safety redaction / tools / agent loop
- **Summary:** Extended the long-token fallback in `redact_secrets_internal` so segments immediately before common image/video extensions (e.g. `.jpg`, `.png`, `.webp`) are not masked — same idea as the existing `.safetensors` carve-out.
- **Rationale:** Instagram-style parking filenames (`ayana.s_official_<long_id>.jpg`) were redacted to `ayana.[REDACTED_SECRET].jpg` in tool results; the agent then reused the redacted path in bash and hit file-not-found. User-visible redaction was already safe; the bug was internal masking on tool echoes fed back into the loop.
- **Key files / symbols:** `src/safety_redaction.rs` (`extension_after_dot`, `is_followed_by_preservable_file_extension`, `is_common_media_extension`), test `internal_preserves_long_instagram_style_jpeg_basename`.
- **Follow-ups:** None unless other dotted artifacts (e.g. `.json` config basenames) need the same treatment.

### 2026-05-20 — Chat-scoped scheduled task listing

- **Area:** channels / tools / db
- **Summary:** Scheduled task listing (via slash commands and the `list_scheduled_tasks` tool) is now filtered to the current chat ID context. Global listing is restricted, and formatted outputs no longer display the `chat_id` unless explicitly requested.
- **Rationale:** Previously, users and agents listing scheduled tasks saw active/completed tasks across all chats, leading to information leakage and cluttered layouts.
- **Key files / symbols:** `src/db.rs` (`Database::get_scheduled_tasks_for_chat_for_display`), `src/tools/schedule.rs` (`ListTasksTool`, `format_tasks_list_impl`), `src/channels/telegram.rs` (`SlashCommand::Schedule`), `src/channels/discord.rs` (`SlashCommand::Schedule`), `src/channels/whatsapp.rs` (`SlashCommand::Schedule`), `src/web.rs` (`SlashCommand::Schedule`).

### 2026-05-20 — Bulletin update required every agent turn

- **Area:** agent / prompt / tools
- **Summary:** System prompt now states `update_bulletin_focus` is mandatory on every run (not optional), including short replies and when `send_message` already delivered the narrative. Tool schema description reinforced.
- **Rationale:** Operators rely on the web cockpit Bulletin card; the bot was skipping updates without explicit must-do guidance.
- **Key files / symbols:** `src/channels/telegram.rs` (`build_system_prompt`); `src/tools/bulletin.rs` (`UpdateBulletinFocusTool::definition`); `test_build_system_prompt_requires_bulletin_every_turn`.

### 2026-05-19 — Shell success: auto agent follow-up reply

- **Area:** background_shell / background_jobs / config
- **Summary:** Successful `spawn_background_command` jobs now enqueue an agent background handoff (like failure auto-retry) so the bot sends a summary reply after the raw completion message. Config: `BACKGROUND_SHELL_AUTO_AGENT_ON_SUCCESS` (default true).
- **Rationale:** Users saw the `[PZ] Your background command finished.` log dump but no agent turn to summarize images, inventory warnings, or next steps.
- **Key files / symbols:** `background_shell::{finalize_shell_job, maybe_enqueue_shell_success_agent_followup}`, `db::{count_shell_success_agent_followups, mark_background_shell_agent_success_followup_enqueued}`, `config::background_shell_auto_agent_on_success`.
- **Follow-ups:** Rebuild/restart bot; set env to `false` to restore log-only completion.

### 2026-05-18 — Web UI LLM model picker with cost reference

- **Area:** web / config / llm
- **Summary:** Provider and API key stay in `.env` (`LLM_PROVIDER`, `LLM_API_KEY`). Model selection moves to Web UI **Settings → LLM**: catalog from `llm_catalog` with approximate USD/1M-token cost hints, persisted as `LLM_MODEL` in `app_settings`, merged at startup and hot-swapped via `LlmHandle` without restart.
- **Rationale:** Operators configure secrets in env but need a safe, guided model switch with pricing context.
- **Key files / symbols:** `src/llm_catalog.rs`, `src/llm.rs` (`LlmHandle`), `src/web.rs` (`GET`/`PATCH /api/llm`), `web/src/components/settings-llm.tsx`, `config::merge_llm_model_from_app_settings`, `is_llm_related_runtime_setting_key` (excludes `LLM_MODEL`).
- **Follow-ups:** Rebuild `web/dist` and restart bot after deploy.

### 2026-05-16 — Identity + Tier 1 memory in system prompt

- **Area:** agent / memory / channels / web
- **Summary:** Split persona memory rendering: **`render_identity_and_tier1_for_system`** (identity, stable facts, capped workflow principles) is appended in **`build_system_prompt`** under `# Identity and long-term memory (Tier 1)`. Tier 2/3, learned workflows, and Mem-Palace links stay in **`[persona_context]`** via **`render_persona_context_memory`** (with operator memo and bookmarks). Token trim uses **`protected_prefix_count`** so runtime/persona prepends are not dropped. System prompt carve-out treats `[persona_context]` / `[system_runtime_context]` / `[scheduler_policy]` as trusted operator context.
- **Rationale:** Stable persona identity should sit in the system role (higher authority, better cache behavior); volatile Tier 2/3 and bookmarks remain in the message prefix.
- **Key files / symbols:** `src/memory.rs` (`render_identity_and_tier1_for_system`, `render_persona_context_memory`), `src/channels/telegram.rs` (`build_system_prompt` `identity_tier1_memory`, `trim_to_token_budget`), `web/src/components/initial-run-prompt-view.tsx`.
- **Follow-ups:** Redeploy + rebuild `web/dist` to pick up First-turn prompt labels.

### 2026-05-16 — Deferred-commitment guard: reject end_turn with “checking logs” placeholders

- **Area:** agent loop / tools
- **Summary:** When the main agent ends with `end_turn` but prose promises imminent work (“checking logs”, “one moment”, run #148, etc.) after a recent tool error or incomplete-work context, the loop injects a routing hint and continues (max 2 nudges) instead of returning to the user. System prompt and `list_cursor_agent_runs` now point at `read_agent_history` / capture-pane for post-mortems.
- **Rationale:** Models often narrate follow-up steps without tool calls; PTE only runs after `tool_use`, so `end_turn` exited immediately (e.g. “Checking run logs…” with no action).
- **Key files / symbols:** `src/channels/telegram.rs` (`assistant_text_defers_work`, `should_reject_premature_end_turn`, `DEFERRED_COMMITMENT_*`), `src/tools/cursor_agent.rs` (`ListCursorAgentRunsTool`).
- **Follow-ups:** Optional cursor-agent completion watcher to persist tmux scrollback into `output_path`.

### 2026-05-16 — Anti-stall: block expensive shell search + discovery loop guard

- **Area:** tools / agent loop
- **Summary:** Blocked interactive `bash` recursive `grep -r` and unbounded `find` (redirect to `glob`/`grep` tools); capped confirmed expensive searches at 120s. Tightened `grep` tool (skip binaries/`vault_db`, output caps). Added discovery-streak routing hint + stall after repeated list/grep archaeology.
- **Rationale:** Status-check runs were hanging ~15+ minutes on `grep -r workspace/shared` (1500s tool timeout) before users cancelled; learned workflows reinforced `list_scheduled_tasks → list_cursor_agent_runs → bash` loops.
- **Key files / symbols:** `src/tools/bash_safety.rs` (`is_expensive_shell_search`, `check_expensive_shell_search`), `src/tools/bash.rs`, `src/tools/grep.rs`, `src/channels/telegram.rs` (`is_discovery_tool_use`, `discovery_streak_count`).
- **Follow-ups:** Truncate `list_scheduled_tasks` output for web UI; tune learned workflow promotion so it does not reinforce broad bash grep.

### 2026-05-15 — Background shell: normalize relative `workdir` vs workspace root

- **Area:** background_shell / spawn_background_command
- **Summary:** Relative `workdir` values like `./workspace/shared` were joined onto `WORKSPACE_DIR` already ending in `workspace`, producing a bogus `workspace/workspace/shared` path and immediate `cd` failure in generated `command.sh`.
- **Rationale:** Same redundancy rule as `workspace_data_path_display`: strip leading segments that duplicate the workspace root’s final path component (repeat until stable).
- **Key files / symbols:** `background_shell::{join_workspace_relative_dir, resolve_shell_workdir}`, unit test `resolve_shell_workdir_drops_redundant_workspace_prefix`.
- **Follow-ups:** Agents should still prefer tool cwd (`shared/` or absolute paths); this hardens mistaken relative paths.

### 2026-05-15 — Tracked external job ids in background queue

- **Area:** tools / db / cockpit
- **Summary:** New tool `register_tracked_job` inserts `job_kind=tracked` rows (e.g. ComfyUI `prompt_id`) so user-visible ids match `background_jobs` in the UI. `count_active_background_jobs_for_chat` excludes `tracked` so handoff/shell slots unchanged.
- **Key files / symbols:** `tools/register_tracked_job.rs`, `db::create_background_tracked_job`, `list_active_background_jobs_for_chat` / `count_active_background_jobs_for_chat` filters.
- **Follow-ups:** Optional `complete_tracked_job` tool or auto-expiry.

### 2026-05-15 — Shell failure auto-retry: unblock handoff + tmux wait-session fallback

- **Area:** background_shell
- **Summary:** Agent retry after shell failure called `try_enqueue_background_handoff` while the shell row was still `running`, so `count_active_background_jobs_for_chat` blocked every time (`Shell failure agent retry blocked`). `finalize_shell_job` now calls `mark_background_shell_finished` for failures before enqueueing retry. Older tmux without `wait-session` no longer triggers immediate finalize from the watcher (poll monitor only); detect via combined stdout/stderr `unknown command`.
- **Key files / symbols:** `background_shell::{finalize_shell_job, spawn_tmux_completion_watcher}`.
- **Follow-ups:** Upgrade tmux on hosts where possible for faster shell completion.

### 2026-05-15 — Shell background: absolute log paths + auto agent retry on failure

- **Area:** background_shell / background_jobs / config
- **Summary:** Shell jobs now write `stdout.log`/`exit_code` using absolute paths and tmux cwd = workspace root (fixes empty logs when default workdir was `shared/`). On non-zero exit, the server delivers the failure output to the user and enqueues one agent background job (`shell_failure_retry:{parent_id}:N`) to diagnose and re-run via `spawn_background_command`. Config: `BACKGROUND_SHELL_AUTO_RETRY_ON_FAILURE` (default true), `BACKGROUND_SHELL_AUTO_RETRY_MAX` (default 1). Handoff agent runs use the chat’s real channel (not hardcoded `web`).
- **Key files / symbols:** `background_shell::{resolve_shell_workdir, maybe_enqueue_shell_failure_agent_retry}`, `db::{count_shell_failure_agent_retries, mark_background_shell_agent_retry_enqueued}`, `background_jobs::try_enqueue_background_handoff` (+ `caller_channel` param).
- **Follow-ups:** Optional `--reference-image` on `run_generation.py` for user uploads; rebuild/restart bot to pick up changes.

### 2026-05-15 — Shadow workspace: path normalization, write guard, migration

- **Area:** tools / agent prompt / doctor / workspace data
- **Summary:** Agents doubling path prefixes (`workspace/shared/...` from cwd `shared/`) created a nested mistaken tree at `shared/workspace/`. `resolve_tool_path` now strips redundant prefixes and resolves `runtime/` and `skills/` against `WORKSPACE_DIR`. Write tools reject paths under `shared/workspace/`; startup and `doctor` warn if the shadow dir exists. Migrated 4 ORIGIN notes and the newer browser profile from the shadow tree, then removed `workspace/shared/workspace` (backup at `workspace/shared/workspace.bak-20260515`).
- **Rationale:** Display-only dedup (`workspace_data_path_display`) did not fix tool resolution; shadow tree split vault/runtime data from canonical layout.
- **Key files / symbols:** `src/tools/mod.rs` (`normalize_tool_relative_path`, `resolve_tool_path`, `check_shadow_workspace_write`, `shadow_workspace_path`), write tools, `src/main.rs` (`warn_shadow_workspace_if_present`), `src/doctor.rs` (`check_shadow_workspace`), `src/channels/telegram.rs` (Repository layout bullets), `workspace/AGENTS.md`.
- **Follow-ups:** Re-index vault if merged ORIGIN files need search coverage; delete `workspace/shared/workspace.bak-*` when satisfied.

### 2026-05-15 — Shell background: fix tmux→bot feedback loop

- **Area:** background_shell
- **Summary:** Fixed wrapper running `command.sh` from the wrong cwd (scripts live in `runtime/background_jobs/{id}/` but wrapper `cd`'d to shared/ first). Added per-job `tmux wait-session` watcher as primary completion signal; poll monitor is backup. Shell jobs no longer get `failed` from expired leases while tmux is still alive; monitor also tracks prematurely-failed rows until notified.
- **Key files / symbols:** `background_shell::{spawn_tmux_completion_watcher, reconcile_shell_background_job_leases}`, `db::{list_shell_jobs_for_monitor, list_shell_jobs_with_expired_lease}`, `reconcile_expired_background_job_leases` excludes `job_kind=shell`.
- **Follow-ups:** None.

### 2026-05-15 — Shell background failures: always notify user

- **Area:** background_shell / scheduler
- **Summary:** Fixed silent `failed` shell jobs: reconcile paths (expired lease, orphan, stale pending, lost tmux session) now call `notify_shell_jobs_by_ids` / `finalize_shell_job` so users get a chat message. `finalize_shell_job` no longer skips delivery when status is already `failed` without `result_text`. Failure messages are clearer (FAILED + retry hint); cancel and enqueue failures also notify.
- **Key files / symbols:** `background_shell::{shell_job_needs_user_notification, notify_shell_jobs_by_ids, finalize_shell_job}`, `db::record_background_shell_user_notification`, `scheduler::run_due_tasks`.
- **Follow-ups:** None.

### 2026-05-15 — Shell background jobs: tmux, monitor, core tool

- **Area:** background jobs / tools / web / scheduler
- **Summary:** Added `spawn_background_command` (core tool) to run long shell work in tmux, persist rows in `background_jobs` (`job_kind=shell`), monitor session exit, and deliver results via `deliver_agent_final_to_contact`. Agent background jobs unchanged (tokio). Queue diagnostics and background APIs expose `job_kind`, tmux session, and `background_by_chat`.
- **Rationale:** Operators need durable shell/GPU jobs with automatic user notification; prior tmux use was cursor-agent-only with no completion delivery.
- **Key files / symbols:** `src/background_shell.rs`, `src/tools/spawn_background_command.rs`, `src/tools/bash_safety.rs`, `src/db.rs` (`BackgroundJob` shell fields), `src/job_heartbeat.rs` (`ShellBackground`), `src/web.rs` (`json_background_job`, `api_queue_diagnostics`), `builtin_skills/background-handoff/SKILL.md`, config `BACKGROUND_SHELL_*`.
- **Follow-ups:** Optional cross-channel agent handoff; unify `cursor_agent_runs` into the same monitor later.

### 2026-05-15 — Persona context in messages (compiled memory, not system JSON)

- **Area:** agent / memory / channels
- **Summary:** Persona memory is no longer injected as raw `memory_state.json` in the system prompt. **`render_memory_for_llm`** compiles non-empty fields into markdown prose (empty sections omitted; `WORKFLOW_PRINCIPLES_PROMPT_MAX` still caps workflow principles). Memory, operator memo, and bookmarks are prepended as **`[persona_context]`** user message + assistant ack (after runtime/scheduler prepends). Removed **Active Project Context** (per-run `upsert_project`, system block, `derive_project_title` / `infer_project_type`). PTE uses the same prose renderer. `build_system_prompt` no longer takes `memory_context` or `operator_memo`.
- **Rationale:** Smaller cacheable system prompt; LLM-friendly memory text; persona steering grouped with runtime context in messages.
- **Key files / symbols:** `src/memory.rs` (`render_memory_for_llm`), `src/channels/telegram.rs` (`build_persona_context_message`, `format_bookmarks_section`, `process_with_agent_with_events`), `docs/architecture_review.md`.
- **Follow-ups:** None required; SQLite `projects` tables remain for future use.

### 2026-05-14 — System prompt: memory modes, workflow cap, vault path display, skills catalog

- **Area:** agent / channels / memory / skills / workspace skills content
- **Summary:** Persona memory in the system prompt defaults to **JSON-only** (`MEMORY_PROMPT_MODE`, default `json`) with optional legend and `markdown`/`both` modes via `build_memory_context_with_options` / env. **`WORKFLOW_PRINCIPLES_PROMPT_MAX`** (default 25, `0` = unlimited) truncates `tier1.workflow_principles` in the prompt only, with `<memory_prompt_note>` when truncated; disk state unchanged. Vault-related lines use **`workspace_data_path_display`** to avoid `workspace/workspace` doubling. **`SKILLS_CATALOG_MODE=compact`** drops `when_to_use` from `<available_skills>`. Shortened **`workspace/skills/xlsx`** YAML `description`. `build_system_prompt` deduped skills/vault bullets slightly.
- **Rationale:** Triple-shipping memory and huge skill descriptions dominated tokens; doubled vault paths misled operators; compact catalog is opt-in for routing vs cost.
- **Key files / symbols:** `src/memory.rs` (`MemoryPromptMode`, `MemoryPromptBuildOptions`, `cap_workflow_principles_for_prompt`, `build_memory_context_with_options`), `src/channels/telegram.rs` (`workspace_data_path_display`, `build_system_prompt`, path tests), `src/skills.rs` (`SkillsCatalogMode`, `build_skills_catalog_with_mode`), `workspace/skills/xlsx/SKILL.md`.
- **Follow-ups:** Tune defaults per deployment; document env vars in operator-facing config docs if needed.

### 2026-05-14 — Skills catalog: frontmatter-only + `when_to_use`

- **Area:** agent / skills / system prompt
- **Summary:** `<available_skills>` is built from YAML metadata only (no SKILL.md body truncation). Added optional `when_to_use` in frontmatter, surfaced in the catalog (capped) and in `activate_skill` output. Standardized in-repo `SKILL.md` files (built-ins + `workspace/skills/background-handoff`) with `description` + `when_to_use` + existing compatibility fields; updated `create-skill` authoring guide.
- **Rationale:** Loading partial bodies into the system prompt duplicated `activate_skill`, inflated tokens, and drifted from the canonical skill text.
- **Key files / symbols:** `src/skills.rs` (`SkillMetadata::when_to_use`, `build_skills_catalog`, `parse_skill_md`), `src/channels/telegram.rs` (`build_system_prompt` Agent Skills blurb), `src/tools/activate_skill.rs`, `builtin_skills/*/SKILL.md`, `workspace/skills/background-handoff/SKILL.md`, `builtin_skills/create-skill/SKILL.md`, `ARCHITECTURE.md`, `docs/architecture_review.md`.
- **Follow-ups:** Optional frontmatter-only reads during discovery for very large SKILL files. All `workspace/skills/*/SKILL.md` copies now include `when_to_use` (and `read-email` / `reddit-ninja-search` gained valid YAML discovery headers).

### 2026-05-14 — Token-budget trim honored cockpit history mins

- **Area:** channels / agent prompt assembly
- **Summary:** `trim_to_token_budget` previously kept a hard minimum of **6** messages, so after `trim_to_recent_balanced` (persona/env depth) a second pass could still drop the thread to six turns—making cockpit depth and prompt-debug snapshots look ineffective. The safety net now stops removing from the front when the remainder would have fewer than the configured minimum **user** or **assistant** text messages (same `min_user_suffix` / `min_asst_suffix` as balanced trim).
- **Rationale:** Operators tune depth for continuity; the 12k-token heuristic must not silently override that.
- **Key files / symbols:** `src/channels/telegram.rs` (`trim_to_token_budget`, `process_with_agent_with_events`), unit test `test_trim_to_token_budget_respects_min_user_assistant`.
- **Follow-ups:** If prompts still exceed provider limits, raise `budget_tokens` or add a separate token cap knob; tool schema size still counts toward the estimate.

### 2026-05-14 — Web: view last run’s first-turn system prompt + messages

- **Area:** agent history / web UI / channels
- **Summary:** Persist a JSON snapshot (`initial_llm_request_v1`: `system_prompt`, `tool_names_first_turn`, `messages` with image payloads summarized) at the end of each `AgentRunRecord` markdown file under `SNAPSHOT_SECTION_START`. Web loads it via existing `GET /api/personas/:id/agent_history/latest`; dialog adds tabs **Run trace** / **First-turn prompt**, a desktop **Last run prompt** button, and mobile menu entry.
- **Rationale:** Operators need to inspect the exact first LLM request for debugging and prompt tuning without spelunking disk.
- **Key files / symbols:** `src/agent_history.rs` (`SNAPSHOT_SECTION_START`, `format_initial_llm_snapshot_json`, `AgentRunRecord::initial_llm_snapshot`), `src/channels/telegram.rs` (`save_run_history!`), `web/src/parse-agent-history.ts` (`splitAgentHistoryRaw`), `web/src/main.tsx`.
- **Follow-ups:** Optional redaction pass on snapshot JSON for shared exports; cap tuning if prompts grow further.

### 2026-05-13 — Conversation continuity: configurable history tail + operator memo

- **Area:** agent / DB / web / config / docs
- **Summary:** Added per-persona nullable `recent_history_min_user`, `recent_history_min_assistant`, and `operator_memo` (SQLite migration). Global defaults `RECENT_HISTORY_MIN_USER_MESSAGES` / `RECENT_HISTORY_MIN_ASSISTANT_MESSAGES` (default 2, clamp 1–25) feed `trim_to_recent_balanced` when overrides are unset. `build_system_prompt` accepts optional operator memo (redacted, length-capped) inserted after `# Principles`, before `# Memory`. Web `GET`/`PATCH /api/personas/:id/bulletin` exposes `history_suffix` and `operator_memo`; cockpit expanded strip adds depth presets and memo editor.
- **Rationale:** The prior fixed 2+2 balanced tail often hid most of `MAX_HISTORY_MESSAGES` from the model; operators need a steering field separate from tiered memory and the header Memory JSON editor.
- **Key files / symbols:** `src/db.rs` (`migrate_personas_prompt_context`, `Persona`, `set_persona_prompt_overrides`, `OPERATOR_MEMO_MAX_CHARS`), `src/config.rs` / `src/config_wizard.rs`, `src/channels/telegram.rs` (`trim_to_recent_balanced`, `build_system_prompt`, persona load in `process_with_agent_with_events`), `src/web.rs` (`api_persona_bulletin_get` / `_patch`), `web/src/main.tsx`, `web/src/components/cockpit-bar.tsx`, `web/src/types.ts`, `src/setup.rs`, `.env.example`, `DEVELOP.md`, `docs/architecture_review.md`.
- **Follow-ups:** Telegram-native editing of memo/overrides; token-budget trim (plan out of scope).

### 2026-05-13 — Learned workflows: remove run-start SQL hint from system prompt

- **Area:** agent / channels / memory / docs
- **Summary:** Removed the pre-loop `get_best_workflow_for_intent(..., 0.6)` lookup that appended `# Learned Workflow Hint` and emitted `AgentEvent::WorkflowSelected`. `save_run_history!` no longer passes a run-selected `workflow_id` into `log_workflow_execution`. Post-run `upsert_workflow_learning` and promotion into `tier1.workflow_principles` are unchanged.
- **Rationale:** Recurring workflow guidance is carried in tiered memory (`WorkflowPrinciple|` lines); the duplicate SQL-sourced hint block was redundant and added prompt noise.
- **Key files / symbols:** `src/channels/telegram.rs` (`process_with_agent_with_events` prompt assembly, `save_run_history!` `selected_workflow_id`), `docs/workflow.md`, `ARCHITECTURE.md`, `docs/architecture_review.md`.
- **Follow-ups:** None unless operators want to drop `workflow_executions` / unused `AgentEvent::WorkflowSelected` handling entirely.

### 2026-05-13 — Agent final vs `send_message`: fuzzy dedupe + memory-tail-only delivery

- **Area:** agent loop / channels / web / scheduler / WhatsApp
- **Summary:** Added `final_delivery_dedupe` (token-bag similarity, attachment echo stripping, optional `---` / **Memory** tail split) and `channel::deliver_agent_final_to_contact`, which picks the newest recent **`send_message`-style** DB row as anchor (skips newer plain bot rows like memory patches). Agent finals can be skipped or reduced to the memory suffix so web/Telegram/Discord no longer double-post paraphrases after attachment sends.
- **Rationale:** `deliver_to_contact` always ran after `end_turn`; `send_message` + attachment already stored user-visible text; exact-match dedupe missed paraphrases. Anchor scan fixes `patch_memory` (or similar) sitting as the latest bot message after `send_message`.
- **Key files / symbols:** `src/final_delivery_dedupe.rs` (`find_send_message_dedupe_anchor`, `plan_agent_final_delivery`, `strip_send_message_echo`, `split_memory_tail`), `src/channel.rs` (`deliver_agent_final_to_contact`, `AgentFinalDeliveryOutcome`), call sites in `src/web.rs`, `src/channels/telegram.rs`, `src/channels/discord.rs`, `src/channels/whatsapp.rs`, `src/scheduler.rs`, `src/background_jobs.rs`; system prompt bullet in `src/channels/telegram.rs` (`build_system_prompt`).
- **Follow-ups:** `/api/send` returns `response: ""` when the final is fully suppressed (client should rely on chat history). `Database::should_skip_duplicate_final_delivery` is now unused by the hot path but kept for compatibility.

### 2026-05-13 — Background handoff: unified enqueue, scheduler/web fixes, quiet heartbeats

- **Area:** web / scheduler / background jobs / job heartbeat
- **Summary:** Centralized web handoff handling in `try_enqueue_background_handoff` + `send_and_store_response_with_events` so the `##BACKGROUND_JOB_HANDOFF##` sentinel never appears in JSON, run hub, or delivered chat; scheduler web-typed runs enqueue `background_jobs` the same way. Agent handoff payloads now embed `timeout` vs `pte_handoff` for `trigger_reason`. Manual-background heartbeat no longer spams `deliver_to_contact` by default (`BACKGROUND_JOB_NOTIFY_CHAT_PROGRESS` opt-in).
- **Rationale:** Scheduler and `/api/send` skipped the stream-only handoff branch, leaving users with a raw sentinel and no DB row; heartbeat progress duplicated chat noise while DB heartbeat + ops APIs already exist.
- **Key files / symbols:** `src/background_jobs.rs` (`is_background_handoff_response`, `handoff_trigger_for_db`, `try_enqueue_background_handoff`, `await_handoff_startup_ack`), `src/web.rs` (`send_and_store_response_with_events`, `api_send_stream`, `api_send`), `src/scheduler.rs` (`run_scheduled_agent_and_finalize`), `src/channels/telegram.rs` (handoff return format), `src/job_heartbeat.rs` (`spawn_shared_heartbeat` + `notify_chat_progress`), `src/config.rs` (`background_job_notify_chat_progress`).
- **Follow-ups:** None.

### 2026-05-12 — Internal redaction: preserve long `*.safetensors` / checkpoint basenames

- **Area:** safety redaction / tools / TSA
- **Summary:** The long-token fallback in `redact_secrets_internal` no longer masks basenames when they are immediately followed by a known model-weight extension (e.g. `.safetensors`, `.ckpt`, `.gguf`), so schedule-tool echoes and other tool results keep readable LoRA filenames.
- **Rationale:** The heuristic treated long snake_case names with digits and multiple underscores as secrets; tool outputs are always passed through internal redaction, which produced `[REDACTED_SECRET].safetensors` for legitimate workflow text. Rust `regex` does not support look-around here, so matching uses `find_iter` plus a small suffix parser instead of a lookahead regex.
- **Key files / symbols:** `src/safety_redaction.rs` (`is_followed_by_model_weight_extension`, `apply_long_token_fallback`), new unit test `internal_preserves_long_lora_basename_before_safetensors`.
- **Follow-ups:** None unless other benign “long token + punctuation” patterns show up in tool echoes (e.g. other dotted artifacts).

### 2026-05-07 — Framework activation fix: portable validator + routing/telemetry hardening

- **Area:** agent loop / validation / observability
- **Summary:** Fixed post-edit validator portability by replacing the Python default command with `compileall`-based validation and adding skip-on-missing-tool behavior for default validator profiles. Added stronger routing pressure toward `read_repo_map` + `apply_search_replace`, plus startup telemetry that logs effective framework toggles and registered edit-tool set.
- **Rationale:** Live runs showed the new hook was active but failing due to `rg` not being available in target runtime shells (`python -m py_compile $(rg --files ...)`). This produced false-negative validation failures and made the framework appear inactive.
- **Key files / symbols:** `src/channels/telegram.rs` (`validator_commands_for_path`, `configured_validator_commands`, `run_post_edit_validation`, `should_skip_validator_failure`, routing-hint injection in tool loop, startup logs in `run_bot`), `src/config.rs` (new regression tests for framework fields/override cleanup).
- **Follow-ups:** Optionally add a dedicated integration smoke command that emits one synthetic post-edit validation block into agent history for easier operator verification after deploy/restart.

### 2026-05-07 — Agentic framework hardening: precise edits, repo map, post-edit validation

- **Area:** agent loop / tools / config / docs
- **Summary:** Added three new tools (`apply_search_replace`, `read_repo_map`, `symbol_edit`), expanded `read_file` with centered adaptive windows, and wired automatic post-edit validator feedback into the shared agent loop after successful code edits. Updated prompt guidance/capabilities and docs to promote map-first retrieval and exact-first block editing.
- **Rationale:** Baseline validation showed the loop relied on exact `edit_file` string replacement, lacked automatic linter-in-the-loop checks, and had no dedicated repository-map retrieval path. This change closes those gaps with conservative defaults: exact matching by default, fuzzy matching opt-in, and symbol editing gated behind config.
- **Key files / symbols:** `src/tools/apply_search_replace.rs`, `src/tools/read_repo_map.rs`, `src/tools/symbol_edit.rs`, `src/tools/read_file.rs` (`center_line`, `context_before`, `context_after`), `src/tools/mod.rs` (tool registration, read-only filtering, risk levels), `src/channels/telegram.rs` (`should_validate_post_edit`, `run_post_edit_validation`, prompt/code-edit strategy updates), `src/config.rs` / `src/config_wizard.rs` (new env-backed toggles), `DEVELOP.md`, `docs/workflow.md`.
- **Follow-ups:** Consider replacing regex/brace heuristics in `symbol_edit` with a true parser-backed implementation (tree-sitter) once language coverage requirements are clearer; refine default JS/TS validator behavior for monorepo layouts.

### 2026-05-07 — Redact scheduler failure details before delivery

- **Area:** scheduler / safety redaction / channel delivery
- **Summary:** Added explicit secret redaction on scheduler delivery boundaries so scheduled-run outputs and failure callouts are sanitized before `deliver_to_contact`. Success responses now pass through user-visible redaction, while scheduler error logs/heartbeat/DB summaries use internal redaction.
- **Rationale:** Scheduled task failure messages were composed directly from raw error strings and could include credential-bearing URLs (for example `...?key=...`) that bypassed `apply_output_safeguards`.
- **Key files / symbols:** `src/scheduler.rs` (`run_scheduled_agent_and_finalize`), `redact_secrets_user_visible`, `redact_secrets_internal`.
- **Follow-ups:** Consider applying the same delivery-boundary sanitization in any other code paths that call `deliver_to_contact` with externally sourced error text.

### 2026-05-06 — Split secret redaction: user-visible vs internal

- **Area:** agent safety / channels / tools
- **Summary:** Replaced a single `redact_secrets` entry point with `redact_secrets_user_visible` (known patterns only, no long-token heuristic) and `redact_secrets_internal` (targeted + long-token fallback). Final assistant output sanitization uses the user-visible path; tool outputs, bash logging, PTE/TSA excerpts, and tool preview/history artifacts use internal redaction.
- **Rationale:** The long-token fallback was masking benign user-facing strings (e.g., long résumé PDF basenames). Narrowing user-visible masking preserves filenames and prose while retaining stricter masking where leaks are likelier (tool results, logs, evaluator prompts).
- **Key files / symbols:** `src/safety_redaction.rs` (`redact_secrets_user_visible`, `redact_secrets_internal`, `redact_targeted_secrets`, `apply_long_token_fallback`), `src/channels/telegram.rs` (`apply_output_safeguards`, tool preview logging / `ToolCallRecord`), `src/tools/mod.rs` (`ToolRegistry::execute`), `src/tools/bash.rs`, `src/post_tool_evaluator.rs` (`build_tool_results_summary`), `src/tool_skill_agent.rs` (`evaluate_tool_use`).
- **Follow-ups:** Consider routing any remaining stray final-text paths through `apply_output_safeguards` for consistent user-visible masking; optionally refine `assignment_secret_regex` (`PASS` / `AUTH` suffix false positives).

### 2026-05-06 — Short-circuit memory-only tool iterations

- **Area:** agent loop / channels
- **Summary:** Updated the main agent loop so iterations that only execute memory-write tools now return the already-authored assistant text immediately, without forcing a follow-up LLM iteration just to end the turn.
- **Rationale:** Memory persistence side effects (for example `patch_memory_state`) were causing an extra model round-trip that could replace a complete Iteration 1 answer with a weaker Iteration 2 follow-up.
- **Key files / symbols:** `src/channels/telegram.rs` (`is_memory_write_tool`, memory-write short-circuit branch in `process_with_agent_with_events` tool-use handling).
- **Follow-ups:** Consider extending the same short-circuit to other non-user-facing state-write tools if they show similar second-turn regressions.

### 2026-05-05 — Mobile web shell overhaul: stable header + single-scroll thread

- **Area:** web UI / mobile shell / thread viewport
- **Summary:** Reworked the mobile chat shell to remove the dual-header collapse pattern and use one sticky top bar with compact state transitions instead of `max-height` hide/show swaps. Hardened thread scroll signaling with source-aware events and mount guards, and enforced mobile scroll ownership so the thread viewport remains the primary vertical scroller while the app root/body stay non-scrolling.
- **Rationale:** Refresh-time layout jitter and occasional document scrolling came from mixed header geometry transitions and loose root overflow behavior; the overhaul makes mobile behavior deterministic across iOS Safari and Android Chrome while keeping desktop flows intact.
- **Key files / symbols:** `web/src/main.tsx` (`handleMobileThreadScroll`, sticky mobile header classes, root overflow locking, cockpit transition behavior), `web/src/components/thread-pane.tsx` (`onMobileThreadScroll` source metadata, scroll guard timing, threshold tuning), `web/src/styles.css` (mobile `@media (max-width: 767px)` overflow contract, viewport scroll behavior, composer/readability touch adjustments).
- **Follow-ups:** Validate on physical devices for keyboard open/close edge cases and adjust collapse thresholds if needed for very short chats.

### 2026-05-04 — Prevent duplicate cross-channel deliveries from duplicate bot instances

- **Area:** channels / delivery fanout / startup
- **Summary:** Added startup dedupe for Telegram and Discord bot instances that share the same token so only one dispatcher/client is launched per token, and added fanout dedupe in `deliver_to_contact` so each `(channel_type, channel_handle)` target receives at most one copy of a reply even if duplicate bindings exist.
- **Rationale:** Duplicate bot-instance rows or duplicate bindings could cause the same run to be delivered multiple times, producing repeated bot replies in Telegram and duplicate entries observed in the web UI history.
- **Key files / symbols:** `src/channels/telegram.rs` (`run_bot` token-owner dedupe for Telegram/Discord instances), `src/channel.rs` (`deliver_to_contact` target dedupe via `HashSet`), `docs/development-journal.md`.
- **Follow-ups:** Consider a DB-level cleanup/migration utility to remove duplicate `channel_bindings` rows and duplicate bot-instance tokens automatically.

### 2026-05-04 — Secret redaction + sensitive path hardening for TSA/tool loop

- **Area:** agent safety / tools / channels
- **Summary:** Added centralized secret redaction with deterministic masking and applied it across tool outputs, TSA and PTE previews, bash command logging, main-agent tool preview logging, agent-history tool-call previews, and final output safeguards. Also tightened sensitive path checks by blocking `.env`-like filenames (e.g. `mercari.env`) outside `skills/` and enforcing `path_guard` for `send_message` attachment paths.
- **Rationale:** Debug runs could surface API keys/tokens via tool previews and logs, and non-standard env filenames were not blocked by the previous path guard list.
- **Key files / symbols:** `src/safety_redaction.rs` (`redact_secrets`), `src/tools/mod.rs` (`ToolRegistry::execute`), `src/tool_skill_agent.rs` (`evaluate_tool_use`), `src/post_tool_evaluator.rs` (`build_tool_results_summary`), `src/channels/telegram.rs` (`apply_output_safeguards`, tool preview logging/history capture), `src/tools/path_guard.rs` (`is_env_like_name`), `src/tools/send_message.rs` (attachment `check_path`), `src/tools/bash.rs` (redacted command logging), `src/lib.rs` (module export).
- **Follow-ups:** Consider applying the same redaction helper to additional structured telemetry payloads and web-facing debug previews if those expand.

### 2026-05-04 — Web chat: mobile composer zoom guard + centered thread content

- **Area:** web UI / chat thread
- **Summary:** Restored centered assistant message layout in the web thread by aligning the custom viewport override with assistant-ui defaults (`align-items: center`) and added a mobile composer safeguard (`font-size: 16px` at `max-width: 639px`) to prevent browser auto-zoom when focusing the input.
- **Rationale:** A recent custom viewport rule made assistant message content appear left-shifted on desktop, and sub-16px composer text could trigger iOS-style focus zoom that caused apparent viewport enlargement and edge overflow on mobile.
- **Key files / symbols:** `web/src/styles.css` (`.mc-thread-viewport.aui-thread-viewport`, mobile `.mc-thread-composer-dock .aui-composer-input` rule).
- **Follow-ups:** If zoom still reproduces on larger phones/tablets, expand the 16px composer rule to the `max-width: 767px` breakpoint.

### 2026-05-04 — Mobile chat header hide + docked composer

- **Area:** web UI / chat thread / main shell
- **Summary:** Replaced the default `<Thread />` layout with `Thread.Root` + `Thread.Viewport` (messages + welcome + follow-ups only) and a sibling **composer dock** below the scroll area so the input is no longer `sticky` inside the message list. On narrow viewports, scrolling the thread down collapses the large header (max-height + opacity); a fixed **compact strip** (hamburger, truncated title, More) stays reachable. `ThreadPane` reports scroll via `onMobileThreadScroll`; `main.tsx` owns `mobileChatHeaderCollapsed` and resets it on thread/persona change, mobile nav open, and composer focus.
- **Rationale:** Sticky composer over messages felt disconnected on phones; collapsing chrome while reading matches common chat apps while preserving navigation when collapsed.
- **Key files / symbols:** `web/src/components/thread-pane.tsx` (`Thread.Root`, `Thread.Viewport`, `bindThreadViewport`, `onMobileThreadScroll`); `web/src/main.tsx` (`mobileChatHeaderCollapsed`, `handleMobileThreadScroll`, compact bar, header wrapper); `web/src/styles.css` (`.mc-thread-shell`, `.mc-thread-viewport`, `.mc-thread-composer-dock`).
- **Follow-ups:** Optional: animate compact bar; tune scroll thresholds.

### 2026-05-03 — Edge-to-edge assistant text on mobile web chat

- **Area:** web UI / chat thread
- **Summary:** Removed the assistant avatar from `CustomAssistantMessage` and collapsed the assistant message grid to a single column. On viewports ≤639px, assistant bubbles lose border/background/padding so markdown reads nearly full-width; thread viewport horizontal padding is 0; user bubbles stay styled but widen to 92% with tighter padding. Header and thread-area chrome use smaller padding on mobile (`main.tsx`). `assistantAvatar={{ fallback: undefined }}` on `<Thread>` prevents the empty-state welcome letter (the library defaults to `"A"` when the prop is omitted).
- **Rationale:** Reading long bot replies is the primary use case; avatars and bubble chrome consumed horizontal space on phones.
- **Key files / symbols:** `web/src/components/thread-pane.tsx` (`CustomAssistantMessage`, `Thread` props); `web/src/styles.css` (`.aui-assistant-message-root`, branch/placeholder/meta-row grid placement, `@media (max-width: 639px)`); `web/src/main.tsx` (header wrapper, hero `pt-14` → `pt-10` on small screens, thread `px-0`).
- **Follow-ups:** Optional: hide or compact the floating cockpit strip for even more vertical space.

### 2026-05-03 — Web thread reload when manual background jobs finish

- **Area:** web UI / background jobs
- **Summary:** When a manual background job’s status moves from an active state to `done` / `failed` / `cancelled`, the web app now calls `loadHistory` (and refreshes the persona bulletin when the job matches the active persona). While any background job is active for the chat, `historyPollUntilMs` is extended so the 10s history poll keeps running as a safety net.
- **Rationale:** The server already persisted the final reply via `deliver_to_contact` in `spawn_background_job`, but the UI only reloaded history briefly after the foreground run ended; long jobs completed after that window so users never saw the result in-thread.
- **Key files / symbols:** `web/src/main.tsx` (`isTerminalBackgroundJobStatus`, `prevBgJobStatusByIdRef`, effects after `setPendingRunIds` reset).
- **Follow-ups:** Optional: persist the web handoff interim line via `deliver_to_contact` in `web.rs` so history shows the “queued in background” copy without relying on heartbeat updates.

### 2026-04-24 — Web chat history pagination restored in hero area

- **Area:** web UI / chat history loading
- **Summary:** Replaced fixed-window history loading with segmented pagination in the chat hero area. The web app now loads the newest page first and shows a **Load older messages** button to progressively pull more history in fixed-size increments.
- **Rationale:** Loading large chats in one pass is wasteful and can hurt responsiveness; progressive loading restores the previous “load more” UX while keeping default polling/history sync lighter.
- **Key files / symbols:** `web/src/main.tsx` (`HISTORY_PAGE_SIZE`, `historyVisibleLimit`, `historyHasMore`, `loadHistory(..., { limitOverride })`, `loadMoreHistory`, hero-area load-more `Button`).

### 2026-04-23 — Bulletin converted to bot-authored focus card

- **Area:** db / tools / web API / web UI / prompt guidance
- **Summary:** Reworked Bulletin from auto-generated memory/file event snippets into a single per-persona focus card authored intentionally by the bot via new `update_bulletin_focus` tool. `GET /api/personas/:persona_id/bulletin` now returns `focus` + bookmarks, and cockpit renders **Bulletin** content as multiline long-term highlights.
- **Rationale:** Users wanted Bulletin to communicate durable current focus rather than noisy tool event traces, while keeping bookmarks as a separate navigation feature.
- **Key files / symbols:** `src/db.rs` (`PersonaBulletinFocus`, `upsert_persona_bulletin_focus`, `get_persona_bulletin_focus`); `src/tools/bulletin.rs` (`UpdateBulletinFocusTool`); `src/tools/mod.rs` (tool registration + risk); `src/channels/telegram.rs` (removed auto bulletin event writes, added capability instruction for `update_bulletin_focus`); `src/web.rs` (`api_persona_bulletin_get` now returns `focus`); `web/src/types.ts` (`PersonaBulletinFocus`); `web/src/main.tsx` (`bulletinFocus` state); `web/src/components/cockpit-bar.tsx` (label renamed to `Bulletin` and focus rendering).

### 2026-04-23 — Cockpit bulletin layout + bookmark jump UX

- **Area:** web UI / cockpit / chat thread
- **Summary:** Moved bulletin rendering into the cockpit expand/collapse container and split it into two explicit sections: **Bot bulletin** (tool-driven updates) and **Bookmarks** (user-pinned bubbles). Bookmark chips in cockpit are now clickable and scroll to the original message in-thread with a short highlight animation.
- **Rationale:** Operators requested updates to live with status/cockpit controls, clearer separation between bot-authored bulletin updates and user bookmarks, and fast navigation back to the source bubble.
- **Key files / symbols:** `web/src/components/cockpit-bar.tsx` (`bulletinUpdates`, `bookmarks`, `onBookmarkClick` sections); `web/src/main.tsx` (`jumpToBookmarkedMessage`, cockpit props, removed inline bulletin callout); `web/src/components/thread-pane.tsx` (stable per-message anchor ids via `data-message-id` / `id`); `web/src/styles.css` (`mc-jump-highlight`, action-bar positioning tweak, bookmark/button spacing).

### 2026-04-22 — Persona bulletin + message bookmarks with prompt injection

- **Area:** db / agent loop / web API / web UI
- **Summary:** Added per-persona bulletin events for memory/file updates, persona-scoped message bookmarks, and web bulletin/bookmark APIs + UI controls. Bookmarked bubbles now flow into `build_system_prompt` context each run as a dedicated “Bookmarked Conversation Context” section.
- **Rationale:** Users need an at-a-glance persona update panel and a way to pin high-signal conversation bubbles so future turns keep that context without manually repeating it.
- **Key files / symbols:** `src/db.rs` (`PersonaBulletinEvent`, `PersonaMessageBookmark`, `append_persona_bulletin_event`, `list_persona_bulletin_events`, `upsert_persona_message_bookmark`, `list_persona_message_bookmarks`); `src/channels/telegram.rs` (`bulletin_event_from_tool_use`, prompt bookmark section in `process_with_agent_with_events`); `src/web.rs` (`api_persona_bulletin_get`, `api_persona_bookmarks_get/post/delete` routes); `web/src/components/thread-pane.tsx` (bookmark toggle in user/assistant bubbles); `web/src/main.tsx` (`loadPersonaBulletin`, `toggleMessageBookmark`, bulletin callout); `web/src/types.ts` (bulletin/bookmark types).

### 2026-04-22 — Browser tool downloads now stay under workspace

- **Area:** tools / browser / runtime paths
- **Summary:** Updated `BrowserTool` to execute `agent-browser` with an explicit current working directory at `WORKSPACE_DIR/shared` instead of inheriting the process cwd. This keeps relative output paths (for example `screenshot foo.png`, `pdf out.pdf`, and browser-triggered downloads that honor process cwd) inside the workspace tree rather than the repository root.
- **Rationale:** Users observed downloaded artifacts landing in the project root. The browser tool previously launched without `current_dir`, so Node inherited whatever directory the bot process was started from.
- **Key files / symbols:** `src/tools/browser.rs` (`BrowserTool::new`, `command_working_dir`, command spawn `current_dir`), `src/tools/mod.rs` (pass `config.working_dir()` into `BrowserTool`), `tools::browser::tests::test_browser_command_working_dir_uses_workspace_shared`.

### 2026-04-20 — DB: remove unused `project_artifacts` table

- **Area:** db / agent (Telegram tool loop)
- **Summary:** Dropped the `project_artifacts` table and `upsert_project_artifact`; nothing in the app read that data. New installs no longer create the table; existing DBs run `migrate_drop_project_artifacts` on open. Removed the post-tool `upsert_project_artifact` calls from the main agent loop.
- **Rationale:** Redundant write-only bookkeeping with no UX or prompt consumption.
- **Key files / symbols:** `src/db.rs` — `migrate_drop_project_artifacts`, initial schema; `src/channels/telegram.rs` tool completion path; `docs/runtime-gap-analysis.md`.

### 2026-04-19 — Scheduled tasks: PATCH schedule fields, `update_scheduled_task`, skill sync rule

- **Area:** db / web API / agent tools / Telegram prompts / web UI / docs
- **Summary:** Added `Database::update_task_schedule` for `schedule_type` + `schedule_value` + `next_run` after preflight. Extended `PATCH /api/schedules/:id` with optional `schedule_type`, `schedule_value`, and `timezone`. New agent tool `update_scheduled_task` (mirrors PATCH). Telegram requires `schedule-job` for `update_scheduled_task` like `schedule_task`; capability strings and tool preview updated. Web schedule **Details** dialog can edit cron/once expression (Save schedule). Documented learned workflows vs cron schedules in `schedule-job` SKILL (both [`builtin_skills/schedule-job/SKILL.md`](builtin_skills/schedule-job/SKILL.md) and [`skills/schedule-job/SKILL.md`](skills/schedule-job/SKILL.md)). Added [`.cursor/rules/builtin-skills-workspace-sync.mdc`](.cursor/rules/builtin-skills-workspace-sync.mdc) for builtin ↔ `skills/` mirror sync.
- **Key files / symbols:** `db::update_task_schedule`; `web.rs` `ScheduleUpdateRequest`, `api_schedules_update`; `schedule::UpdateScheduledTaskTool`, `tools::mod` registration and `tool_risk`; `telegram.rs` missing-schedule-skill gate; `web/src/main.tsx` schedule detail editor.

### 2026-04-16 — API: `requires_restart_for_env_changes` no longer always true

- **Area:** web API / web UI cockpit
- **Summary:** `GET /api/settings` embedded `installation_status.requires_restart_for_env_changes: true` unconditionally, so the cockpit (and Settings overview) always showed **Restart needed**. It is now **`false`**: generic settings PATCH is disabled and runtime env is not merged from `app_settings`, so there is no server-derived “pending restart” signal until we implement a real one.
- **Key files / symbols:** `src/web.rs` (`api_settings_get` JSON).

### 2026-04-16 — Web: retractable desktop persona sidebar

- **Area:** web UI
- **Summary:** `desktopSidebarOpen` persisted in `localStorage` (`finally-a-value-bot_desktop_sidebar_open`, default open). `md+` grid shows the `SessionSidebar` column only when open; collapsed layout is single-column full-width chat. Header **⟨** / **⟩** `IconButton` toggles visibility; sidebar header gets optional **⟨** “Hide sidebar” when `onRequestCollapse` is passed (desktop column only, not the mobile slide-over).
- **Key files / symbols:** `web/src/main.tsx` (`readDesktopSidebarOpen` / `saveDesktopSidebarOpen`, grid conditional); `web/src/components/session-sidebar.tsx` (`onRequestCollapse`).

### 2026-04-16 — Web: cockpit status strip (queue, background, setup)

- **Area:** web UI
- **Summary:** Added `web/src/components/cockpit-bar.tsx`: a full-width strip under the main header toolbar showing **session status text**, **queue** (click opens run-queue dialog; no duplicate toolbar button), **background job count**, and compact **LLM / Channels / restart** readiness from `installationStatus`. Desktop toolbar and mobile title row no longer repeat queue/background/status; mobile **More** moved beside the session title. Header split into padded toolbar `div` + cockpit (operational state separate from Settings / Schedules / etc.).
- **Rationale:** Matches “cockpit vs tooling” — operators see live ops state at a glance without crowding action buttons.
- **Key files / symbols:** `web/src/main.tsx` (`<CockpitBar />`, `setQueueDialogOpen`); `web/src/components/cockpit-bar.tsx`.

### 2026-04-16 — Web polish: TanStack Query ops poll, schedules filter, Vitest, visual refresh

- **Area:** web UI
- **Summary:** Replaced manual `setInterval` ops polling with `@tanstack/react-query` (`useOpsPoll`, `invalidateOps`) and shared fetch helpers under `web/src/api/ops-fetch.ts`. Schedules dialog: **Show completed / cancelled** switch, `schedulesFiltered` list, dashed empty state when the filter hides everything. Added Vitest (`vitest.config.ts`, `npm run test`) and `web/src/lib/history-sync.test.ts` for `mapBackendHistory` / `historiesEqual`. Visual refresh: Instrument Sans + JetBrains Mono (Google Fonts), radial page backgrounds tied to `--mc-accent`, dark-mode scrollbar tint, translucent Radix panels (`Theme` `radius="large"` `panelBackground="translucent"`), sidebar brand block + subtitle. Removed unused `QueueLane` import from `main.tsx`.
- **Rationale:** Centralized polling improves backoff alignment with `useDocumentVisible`; tests lock history-sync behavior; typography and surfaces reduce the flat single-color shell feel without rewriting Radix layouts.
- **Key files / symbols:** `web/src/query-client.ts`, `web/src/hooks/use-ops-poll.ts`, `web/src/api/ops-fetch.ts`, `web/src/main.tsx` (`QueryClientProvider`, schedules UI), `web/src/styles.css`, `web/index.html`, `web/src/components/session-sidebar.tsx`, `web/vitest.config.ts`.
- **Follow-ups:** Optional split of `main.tsx` into `app-shell` + dialog modules; expand Vitest beyond `history-sync`.

### 2026-04-16 — Web UI overhaul: responsive shell, settings tabs, modular front-end

- **Area:** web UI
- **Summary:** Responsive layout: `md+` keeps the persona sidebar; narrow viewports use a hamburger that opens a slide-over with the same `SessionSidebar`. Header uses a **More** dropdown on small screens; Queue stays one tap away. Settings dialog split into **Overview / Integrations / Channels / Legacy** tabs; queue runs show as cards on small screens; artifacts list stacks above preview on mobile. Extracted `web/src/api/client.ts`, `web/src/components/thread-pane.tsx`, `web/src/lib/history-sync.ts`, `web/src/hooks/use-document-visible.ts`; shared `BackendMessage` / queue types in `web/src/types.ts`. Onboarding callout when LLM or channels are not ready; `loadSettings()` on boot for `installation_status`; polling uses a 60s interval when the document tab is hidden.
- **Rationale:** The UI was desktop-fixed (`grid-cols-[320px_…]`) and operationally dense in the header; modular pieces reduce risk around `ThreadPane` `React.memo` and future changes.
- **Key files / symbols:** `web/src/main.tsx` (shell, `DropdownMenu`, `Tabs`, onboarding); `web/src/components/session-sidebar.tsx` (`onCloseRequest`); `web/dist/*` (rebuilt bundle).
- **Follow-ups:** Further split `main.tsx` into dialog components; optional TanStack Query for polls.

### 2026-04-16 — Web restart: gateway service instead of env hook

- **Area:** web API / gateway / web UI
- **Summary:** `POST /api/restart` now schedules a restart of the user-level gateway installed via `finally_a_value_bot gateway install` (Linux: `systemctl --user restart finally_a_value_bot-gateway.service`; macOS: `launchctl kickstart -k` on the launchd label), after a short delay so the HTTP response can flush. Removed `FINALLY_A_VALUE_BOT_RESTART_COMMAND` / `restart_hook_configured`. Settings shows **Restart gateway** only; 400 if the unit/plist is missing.
- **Rationale:** Operators should not configure a separate supervisor command; restart should match the documented gateway install path.
- **Key files / symbols:** `src/gateway.rs` (`user_gateway_service_installed`, `schedule_user_gateway_restart`, `restart_user_gateway_now`); `src/web.rs` (`api_restart_post`, `installation_status` JSON); `web/src/main.tsx`, `web/src/types.ts` (`InstallationStatus`).

### 2026-04-15 — Documentation: learned workflows

- **Area:** docs / Cursor rules
- **Summary:** Added canonical [`docs/workflow.md`](workflow.md) (storage, intent signatures, selection threshold, confidence smoothing, learning vs execution log, config, interactions with scheduler/skills, fresh install). Linked from [`docs/runtime-gap-analysis.md`](runtime-gap-analysis.md), [`DEVELOP.md`](../DEVELOP.md), [`docs/architecture_review.md`](architecture_review.md), and [`.cursor/rules/development-doc-references.mdc`](../.cursor/rules/development-doc-references.mdc). Added optional [`.cursor/rules/learned-workflows.mdc`](../.cursor/rules/learned-workflows.mdc) for agent/DB workflow changes.
- **Rationale:** Operators and agents needed a single place describing auto-learned workflows distinct from GitHub Actions and from skills; reduces confusion about confidence, `workflow_min_success_repetitions`, and prompt vs post-run learning.
- **Key files / symbols:** `docs/workflow.md`; `learned-workflows.mdc` → `docs/workflow.md`.

### 2026-04-14 — Runtime DB merge removed, restart hook, bot instances in Settings

- **Area:** config / web API / web UI / docs
- **Summary:** Startup no longer merges `app_settings` into env; `GET /api/settings` reports `runtime_env_merge_from_app_settings: false`, `requires_restart_for_env_changes`, and `restart_hook_configured`; `PATCH /api/settings` returns 501. Added `FINALLY_A_VALUE_BOT_RESTART_COMMAND` + `POST /api/restart` (501 with copy-paste examples when unset; 202 when hook runs). Exposed authenticated CRUD for `channel_bot_instances` (`GET/POST /api/channel_bot_instances`, `PATCH/DELETE /api/channel_bot_instances/:id`, tokens redacted on GET). Web Settings: legacy `app_settings` read-only section, **Restart** button, **Bot integrations** list/add/delete (non–env-primary rows). `DEVELOP.md` config line no longer claims DB runtime overrides.
- **Rationale:** Align runtime config with `.env`-only mental model; give operators a safe optional one-click restart; manage extra Telegram/Discord bot rows without SQL.
- **Key files / symbols:** `src/main.rs` (no `apply_runtime_settings_from_db`); `src/web.rs` (`api_settings_get`/`patch`, `restart_hook_command`, `api_restart`, `api_channel_bot_instances_`*); `src/db.rs` (`list_all_channel_bot_instances`, `create_channel_bot_instance`, `update_channel_bot_instance`, `delete_channel_bot_instance`); `web/src/main.tsx`, `web/src/types.ts` (`InstallationStatus`, `BotInstanceRow`), `web/src/vite-env.d.ts`; `ARCHITECTURE.md` §8; `DEVELOP.md` (project structure blurb).
- **Follow-ups:** Multi-contact web sessions and optional behavior when `UNIVERSAL_CHAT_ID` is unset (see `ARCHITECTURE.md` “Universal chat id”); optional UI for `PATCH` bot instance label/token.

### 2026-04-14 — LLM env-only, per-bot-instance persona policy, multi Telegram/Discord dispatch

- **Area:** config / DB / channels / web UI
- **Summary:** LLM-related keys are no longer loaded from `app_settings` at startup or accepted via `PATCH /api/settings` (use repo-root `.env`). Channel bindings and persona scope policy are keyed by `bot_instance_id` (`channel_bot_instances` table, seeded from env for primary Telegram/Discord/WhatsApp). Multiple Telegram dispatchers and Discord clients run when multiple rows exist. Web chat is excluded from external “single vs all persona” policy; `/api/contacts/bindings` omits web rows for persona controls.
- **Rationale:** Operators asked for LLM config strictly in `.env`, independent web persona selection, and separate all/single-persona policy per external bot instance.
- **Key files / symbols:** `src/config.rs` (`is_llm_related_runtime_setting_key`), `src/main.rs` (`apply_runtime_settings_from_db`, `sync_channel_bot_instances_from_config`), `src/db.rs` (`channel_bot_instances`, `BOT_INSTANCE_`*, migrations, `sync_channel_bot_instances_from_config`, binding/policy APIs), `src/persona.rs` (`resolve_incoming_run_persona_for_channel` skips policy for web / `bot_instance_id == 0`), `src/channel.rs` (`deliver_to_contact` uses per-instance `Bot` / `Http` maps), `src/channels/telegram.rs` (`AppState.telegram_bots`, dispatcher `deps` include `telegram_bot_instance_id`), `src/channels/discord.rs` (`Handler.discord_bot_instance_id`, `start_discord_bot(..., id)`), `src/web.rs` (settings GET filter, PATCH reject LLM; persona API uses `bot_instance_id`), `web/src/main.tsx` (Settings copy; persona policy by `bot_instance_id`).

### 2026-04-14 — Web-first settings/onboarding + channel persona mode

- **Area:** config / startup / web / channels / onboarding
- **Summary:** Added SQLite-backed runtime settings (`app_settings`) editable from Web UI (`GET/PATCH /api/settings`) and startup merge from DB into effective config. Startup no longer auto-launches CLI setup; `config`/`setup` commands are retired with Web UI guidance, and Telegram dispatcher is optional when no token is configured.
- **Rationale:** Move operator-facing setup from CLI to web-first onboarding while keeping only bootstrap env values in repo-root `.env`; allow installations to start in web-only mode and complete channel/LLM setup from UI.
- **Key files / symbols:** `src/db.rs` (`AppSetting`, `channel_persona_policy`, CRUD methods), `src/main.rs` (`apply_runtime_settings_from_db`, retired CLI config/setup flow), `src/config.rs` (relaxed validation for web-first bootstrap), `src/web.rs` (`/api/settings`, `/api/channel_persona_policy`, enriched `/api/contacts/bindings`), `web/src/main.tsx` (Settings dialog + channel persona controls), `src/persona.rs` (`resolve_incoming_run_persona_for_channel`), `src/channel.rs` (`deliver_to_contact` policy filtering), `src/channels/{telegram,discord,whatsapp}.rs` (policy-aware persona resolve), `install.sh`, `install.ps1`, `README.md`, `DEVELOP.md`, `ARCHITECTURE.md`.
- **Follow-ups:** Add optional encryption-at-rest for secret settings and selective hot-reload so some runtime settings can apply without restart.

### 2026-04-14 — Cursor rule: prevent behavior-test drift in Rust changes

- **Area:** repo policy / CI / tests
- **Summary:** Extended `[.cursor/rules/rustfmt-ci.mdc](.cursor/rules/rustfmt-ci.mdc)` with a new behavior-test drift section: when tool contracts change, tests must update in the same change (notably `ToolAuthContext`-dependent attachment tests and parser boundary assertions for URL extraction).
- **Rationale:** Recent failures came from stale test assumptions after contract updates (`caller_channel` auth requirement and markdown URL token boundaries), not compiler/lint issues.
- **Key files / symbols:** `.cursor/rules/rustfmt-ci.mdc` (`Preventing behavior-test drift`); references `__finally_a_value_bot_auth`, `ToolAuthContext`, URL parser boundary assertions.

### 2026-04-14 — Cursor rule: require clippy preflight for Rust CI

- **Area:** repo policy / CI / linting
- **Summary:** Updated `[.cursor/rules/rustfmt-ci.mdc](.cursor/rules/rustfmt-ci.mdc)` to require both `cargo fmt --all --check` and `cargo clippy -- -D warnings` before Rust work is considered complete, and added guidance to fix `clippy::type_complexity` with `type` aliases instead of suppressing lints.
- **Rationale:** A recent CI failure (`-D warnings`) from a complex Rust type made it clear formatting-only guidance was insufficient; explicit clippy preflight prevents repeat regressions.
- **Key files / symbols:** `.cursor/rules/rustfmt-ci.mdc` (`cargo clippy -- -D warnings`, `type_complexity` prevention section).

### 2026-04-14 — Channel-local attachments + web artifacts viewer

- **Area:** tools / web / channel delivery
- **Summary:** Changed `send_message` attachment routing to follow `ToolAuthContext.caller_channel` (active channel) rather than `chats.chat_type`, so attachments remain channel-local by default. Added a web artifacts feature: backend `GET /api/artifacts`, safer upload serving with preview/download semantics, and a new **Artifacts** dialog in web UI with list/filter/preview/open/download.
- **Rationale:** Cross-channel attachment behavior was inconsistent and surprising; channel-local delivery matches user intent and avoids silent fanout mismatches. Web needed a first-class way to inspect generated/uploaded files without forcing re-send from other channels.
- **Key files / symbols:** `src/tools/send_message.rs` — `resolve_active_attachment_target`, `ActiveAttachmentTarget`; `src/web.rs` — `api_artifacts`, `list_chat_artifacts`, `process_web_attachments` (`url=` + renderable links), `upload_file` (`preview`/`download`, shared+legacy roots), `guess_upload_content_type`; `web/src/main.tsx` — Artifacts dialog and preview UX; `web/src/types.ts` — `ArtifactItem`.
- **Follow-ups:** Consider adding pagination and server-side search for very large artifact sets; add frontend automated tests for artifact dialog interactions when UI test harness is available.

### 2026-04-14 — Cursor rules: rustfmt CI + cross-platform tests

- **Area:** repo policy / CI / tests
- **Summary:** Added `[.cursor/rules/rustfmt-ci.mdc](.cursor/rules/rustfmt-ci.mdc)` to enforce `cargo fmt --all` and `cargo fmt --all --check` before completion, and `[.cursor/rules/cross-platform-tests.mdc](.cursor/rules/cross-platform-tests.mdc)` to require OS-agnostic path assertions plus shell-aware test commands (PowerShell vs `/bin/sh`) with Windows path normalization guidance.
- **Rationale:** Recent CI failures repeated around Windows path separators, PowerShell output differences, and rustfmt drift; explicit always-apply rules reduce regressions and rework.
- **Key files / symbols:** `.cursor/rules/rustfmt-ci.mdc`, `.cursor/rules/cross-platform-tests.mdc`; guidance references `Path::ends_with`, `PathBuf::from`, and Windows `\\?\` prefix handling.

### 2026-04-14 — Web: queue detail + cancel, schedule prompt edit, AGENTS.md editor

- **Area:** web / API / queue / agent / memory
- **Summary:** Extended `ChatRunQueue` with per-run metadata (`QueueEnqueueMeta`, `QueueSource`), ordered `items` in diagnostics, and `request_cancel` via `Arc<AtomicBool>`. `process_with_agent_with_events` cooperatively exits when cancelled between iterations. Added `GET /api/queue_diagnostics` item rows (persona name, label, state), `POST /api/queue/cancel`, `PATCH /api/schedules/:id` with `prompt`, and `GET`/`PUT /api/workspace/agents_md` using `MemoryManager::write_groups_root_memory`. Web header: **Queue** dialog with **Stop**, **Schedules** row **Details** + prompt editor, **Principles** dialog for workspace AGENTS.md.
- **Rationale:** Operators need visibility into FIFO agent work, safe stop, schedule prompt edits without SQL, and in-UI editing of shared principles path.
- **Key files / symbols:** `chat_queue::{ChatRunQueue, QueueEnqueueMeta, QueueSource, request_cancel}`; `telegram::process_with_agent_with_events(..., cancel)`; `web::{api_queue_diagnostics, api_queue_cancel, api_workspace_agents_md_get/put, api_schedules_update}`; `db::update_task_prompt`; `memory::write_groups_root_memory`, `groups_root_memory_path` pub; `web/src/main.tsx` dialogs.
- **Follow-ups:** Optional SSE `done` publish on cancel for streaming runs; optional editing schedule expressions with preflight.

### 2026-04-14 — Cursor rule: require clippy preflight for Rust CI

- **Area:** repo policy / CI / linting
- **Summary:** Updated [`.cursor/rules/rustfmt-ci.mdc`](.cursor/rules/rustfmt-ci.mdc) to require both `cargo fmt --all --check` and `cargo clippy -- -D warnings` before Rust work is considered complete, and added guidance to fix `clippy::type_complexity` with `type` aliases instead of suppressing lints.
- **Rationale:** A recent CI failure (`-D warnings`) from a complex Rust type made it clear formatting-only guidance was insufficient; explicit clippy preflight prevents repeat regressions.
- **Key files / symbols:** `.cursor/rules/rustfmt-ci.mdc` (`cargo clippy -- -D warnings`, `type_complexity` prevention section).

### 2026-04-14 — Channel-local attachments + web artifacts viewer

- **Area:** tools / web / channel delivery
- **Summary:** Changed `send_message` attachment routing to follow `ToolAuthContext.caller_channel` (active channel) rather than `chats.chat_type`, so attachments remain channel-local by default. Added a web artifacts feature: backend `GET /api/artifacts`, safer upload serving with preview/download semantics, and a new **Artifacts** dialog in web UI with list/filter/preview/open/download.
- **Rationale:** Cross-channel attachment behavior was inconsistent and surprising; channel-local delivery matches user intent and avoids silent fanout mismatches. Web needed a first-class way to inspect generated/uploaded files without forcing re-send from other channels.
- **Key files / symbols:** `src/tools/send_message.rs` — `resolve_active_attachment_target`, `ActiveAttachmentTarget`; `src/web.rs` — `api_artifacts`, `list_chat_artifacts`, `process_web_attachments` (`url=` + renderable links), `upload_file` (`preview`/`download`, shared+legacy roots), `guess_upload_content_type`; `web/src/main.tsx` — Artifacts dialog and preview UX; `web/src/types.ts` — `ArtifactItem`.
- **Follow-ups:** Consider adding pagination and server-side search for very large artifact sets; add frontend automated tests for artifact dialog interactions when UI test harness is available.

### 2026-04-14 — Cursor rules: rustfmt CI + cross-platform tests

- **Area:** repo policy / CI / tests
- **Summary:** Added [`.cursor/rules/rustfmt-ci.mdc`](.cursor/rules/rustfmt-ci.mdc) to enforce `cargo fmt --all` and `cargo fmt --all --check` before completion, and [`.cursor/rules/cross-platform-tests.mdc`](.cursor/rules/cross-platform-tests.mdc) to require OS-agnostic path assertions plus shell-aware test commands (PowerShell vs `/bin/sh`) with Windows path normalization guidance.
- **Rationale:** Recent CI failures repeated around Windows path separators, PowerShell output differences, and rustfmt drift; explicit always-apply rules reduce regressions and rework.
- **Key files / symbols:** `.cursor/rules/rustfmt-ci.mdc`, `.cursor/rules/cross-platform-tests.mdc`; guidance references `Path::ends_with`, `PathBuf::from`, and Windows `\\?\` prefix handling.

### 2026-04-14 — Web: queue detail + cancel, schedule prompt edit, AGENTS.md editor

- **Area:** web / API / queue / agent / memory
- **Summary:** Extended `ChatRunQueue` with per-run metadata (`QueueEnqueueMeta`, `QueueSource`), ordered `items` in diagnostics, and `request_cancel` via `Arc<AtomicBool>`. `process_with_agent_with_events` cooperatively exits when cancelled between iterations. Added `GET /api/queue_diagnostics` item rows (persona name, label, state), `POST /api/queue/cancel`, `PATCH /api/schedules/:id` with `prompt`, and `GET`/`PUT /api/workspace/agents_md` using `MemoryManager::write_groups_root_memory`. Web header: **Queue** dialog with **Stop**, **Schedules** row **Details** + prompt editor, **Principles** dialog for workspace AGENTS.md.
- **Rationale:** Operators need visibility into FIFO agent work, safe stop, schedule prompt edits without SQL, and in-UI editing of shared principles path.
- **Key files / symbols:** `chat_queue::{ChatRunQueue, QueueEnqueueMeta, QueueSource, request_cancel}`; `telegram::process_with_agent_with_events(..., cancel)`; `web::{api_queue_diagnostics, api_queue_cancel, api_workspace_agents_md_get/put, api_schedules_update}`; `db::update_task_prompt`; `memory::write_groups_root_memory`, `groups_root_memory_path` pub; `web/src/main.tsx` dialogs.
- **Follow-ups:** Optional SSE `done` publish on cancel for streaming runs; optional editing schedule expressions with preflight.

### 2026-04-13 — Unit tests: align with Telegram HTML, trim, tools, web limits

- **Area:** tests / telegram / llm / web / builtin_skills
- **Summary:** Fixed 13 failing `--lib` tests: `trim_to_recent_balanced` restored to ≥2 user and ≥2 assistant (was incorrectly ≥3/≥3); `message_to_text` prefixes tool results again; `has_new_swap_evidence` ignores “no files found … found matching”; Gemini `normalize_stop_reason` maps case-insensitive `stop`/`STOP` to `end_turn`; setup env save test expects unquoted tokens when unnecessary; send_message web test expects `[default]` persona prefix; builtin_skills test drops missing `social-feed`; web stream/concurrency tests match queued `/api/send` + SSE replay; markdown/trim tests updated to current formatter output.
- **Key files / symbols:** `trim_to_recent_balanced`, `message_to_text`, `has_new_swap_evidence`, `normalize_stop_reason`, `src/web.rs` tests, `src/builtin_skills.rs` test list.

### 2026-04-13 — CI: fix Clippy + tests after Config API drift

- **Area:** infra / config / tests
- **Summary:** Aligned `test_config` and test-only `Config` literals with the current `[Config](src/config.rs)` struct (removed obsolete `max_session_messages` / `compact_keep_recent` / delegate fields). Exposed `pub fn test_config()` at the `config` module root under `cfg(test)` for unit tests; integration tests use YAML-based minimal config. Updated `history_to_claude_messages` unit tests for the `keep_trailing_assistant` parameter. Resolved Clippy 1.94 (`is_some_and` / `is_none_or`, `cursor_agent` init, `channel` test module placement, `[lints.clippy]` allows for noisy lints, `DummyTool` dead_code).
- **Rationale:** `cargo clippy -- -D warnings` on CI was failing on private `config::tests`, stale struct fields, and new Clippy suggestions.
- **Key files / symbols:** `src/config.rs` — `test_config`; `src/channels/telegram.rs`; `src/llm.rs` / `src/web.rs` tests use `crate::config::test_config()`; `tests/config_validation.rs`; `Cargo.toml` `[lints.clippy]`; `src/channel.rs` — `mod tests` at EOF; `src/tools/cursor_agent.rs`.

### 2026-04-13 — GitHub Actions: action bumps, Dependabot, CI polish

- **Area:** infra / CI
- **Summary:** Pinned first-party actions to current majors (`actions/checkout@v6`, `actions/setup-node@v6`, `actions/upload-artifact@v6`, `actions/download-artifact@v6`) in `[.github/workflows/ci.yml](.github/workflows/ci.yml)` and `[.github/workflows/release-assets.yml](.github/workflows/release-assets.yml)`. Added `[.github/dependabot.yml](.github/dependabot.yml)` (weekly) for `github-actions`, `cargo`, and `npm` under `web/`. CI now uses `concurrency` (cancel in-progress on same ref), `workflow_dispatch`, and passes `web/dist` from the web job to the release build job via artifacts so `npm ci` + web build run once per run.
- **Rationale:** Stay on supported action runtimes; reduce duplicate web builds; automated dependency PRs for workflows and lockfiles.
- **Key files / symbols:** `.github/workflows/ci.yml` — `web-dist` artifact; `.github/workflows/release-assets.yml`; `.github/dependabot.yml`.

### 2026-04-10 — Background jobs: visibility, terminal heartbeats, stale reconciliation

- **Area:** web / API / scheduler / db / job heartbeat
- **Summary:** Closed gaps where job heartbeats could stay `active` after worker disconnect; scheduled duplicate-delivery skip now sends `Finished` to the shared heartbeat. `/api/run_status` merges optional DB heartbeat + background job snapshot (and falls back to DB-only when the in-memory run hub has no channel, e.g. polling by background `job_id`). `GET /api/background_jobs` returns per-job `heartbeat`, `active_heartbeats`, and `GET /api/background_jobs/:job_id` returns merged job + heartbeat + recent timeline rows. Scheduler tick calls `reconcile_stale_active_job_heartbeats` and `reconcile_orphan_stale_background_jobs` using the same threshold as `scheduler_stale_running_reclaim_secs`. Web header shows **Background: N active** and polls background visibility when the queue is busy or background work is active.
- **Rationale:** Operators need a single place to see liveness and stage; dangling `active` rows after crashes or dropped senders misrepresented system state; process-kill leaves DB stale until a reconciler runs.
- **Key files / symbols:** `job_heartbeat::spawn_shared_heartbeat` (disconnect → `aborted` + timeline); `scheduler::run_scheduled_agent_and_finalize` (skip_dup → `Finished`); `scheduler::run_due_tasks` (reconcile calls); `db::{list_active_job_heartbeats_for_chat, list_job_heartbeats_for_chat, reconcile_stale_active_job_heartbeats, reconcile_orphan_stale_background_jobs}`; `web::{json_job_heartbeat, api_run_status, api_background_jobs_list, api_background_job_get}`; `web/src/main.tsx` — `loadBackgroundVisibility`, header indicator.

### 2026-04-10 — Web chat: `React.memo(ThreadPane)` vs parent polling

- **Area:** web / UX
- **Summary:** Wrapped the chat thread in `React.memo` so it does not re-render when unrelated App state updates (persona list refresh every ~2.5–10s, queue diagnostics, schedules, etc.). `@assistant-ui/react`’s `useLocalRuntime` runs a `useEffect` with no dependency array each render, updating options and calling `__internal_load`; those extra passes correlated with composer text loss and scroll jumping.
- **Rationale:** Isolate the assistant runtime from header/sidebar polling re-renders; defer/history equality alone was not sufficient.
- **Key files / symbols:** `web/src/main.tsx` — `ThreadPane` (`React.memo`).

### 2026-04-09 — Web UI: “Last agent run” modal (latest trace per persona)

- **Area:** web / API / agent
- **Summary:** Added `GET /api/personas/:persona_id/agent_history/latest` (same auth/binding/persona guards as memory) returning the newest `YYYYMMDD-HHMMSS.md` under `runtime_data_dir/groups/{chat_id}/{persona_id}/agent_history/`. Helpers in `agent_history` list/validate basenames, enforce a 4 MiB read cap, and `read_latest_agent_history`. Web header opens a **Last agent run** dialog that parses `## Iteration N` sections and renders Markdown with Prev/Next and ←/→.
- **Rationale:** Operators can review the latest agentic loop (iterations and tool lines) without opening files on disk.
- **Key files / symbols:** `agent_history::{list_agent_history_md_basenames_sorted, read_latest_agent_history, is_valid_agent_history_filename, ReadLatestAgentHistoryError}`; `web.rs` — `api_persona_agent_history_latest`; `web/src/main.tsx` — dialog, `AgentHistoryMarkdownBody`; `web/src/parse-agent-history.ts` — `parseAgentHistoryMarkdown`; `tools/agent_history.rs` — reuses shared listing.
- **Follow-ups:** Optional API to list or fetch older runs; optional persisted per-iteration notes.

### 2026-04-09 — Web chat: avoid composer reset while typing / reading history

- **Area:** web / UX
- **Summary:** History sync compared messages including `createdAt`, so polling could remount the thread when timestamps jittered. Remounting bumped `runtimeNonce` and wiped the composer + scrolled to bottom. Equality now uses id/role/content only; background `loadHistory` defers applying updates (and the remount) while the composer is focused or the thread viewport is scrolled away from the bottom, then flushes on focus-out or scroll-to-bottom. Explicit actions (initial load, persona switch, bind, load older, delete persona) use `force: true`.
- **Rationale:** Polling and post-send history refresh must not disrupt typing or reading older messages.
- **Key files / symbols:** `web/src/main.tsx` — `historiesEqual`, `shouldDeferHistoryRemount`, `deferredHistoryRef`, `flushDeferredHistory`, `loadHistory(..., { force })`.

### 2026-04-09 — Persona routing: requester prefix + run-scoped tool defaults

- **Area:** channels / agent / tools / persona
- **Summary:** Inbound Telegram, Discord, and WhatsApp messages resolve **run persona** via optional leading `[PersonaName]` (case-insensitive match to a persona in that chat; reserved `image`/`document`/`location`/`voice`); storage and `process_with_agent` use that id **without** calling `set_active_persona`. Added `default_persona_id_for_chat` so `send_message` and `export_chat` default to `ToolAuthContext.caller_persona_id` for the same chat instead of DB active at tool time (mid-run active switch no longer mis-attributes).
- **Rationale:** Users can address a non-active persona per message while keeping UI “active” unchanged; agentic tool calls must stay on the run’s persona.
- **Key files / symbols:** `persona::resolve_incoming_run_persona`; `tools::default_persona_id_for_chat`; `send_message` / `export_chat`; `channels/telegram.rs`, `discord.rs`, `whatsapp.rs` ingress.
- **Follow-ups:** Optional same prefix convention on web; optional DRY with `schedule_task` persona resolution.

### 2026-04-09 — README, install scripts, `.env.example`, Docker doc posture

- **Area:** docs / onboarding / config
- **Summary:** Rebuilt [.env.example](.env.example) from `Config::load_from_env` (sections for channels, LLM, web, scheduler, agent stack, runtime/workflow, cursor-agent, browser, safety, optional vault/social/git); removed obsolete `DELEGATE`_*; vault block commented so a copy does not enable vault by default. Refreshed [README.md](README.md) for native install, `config` / `setup`, `doctor`, minimum Telegram-or-Discord + LLM rules, web URL, vault pointer; Docker no longer recommended (legacy pointer to [DOCKER.md](DOCKER.md)). [install.sh](install.sh) / [install.ps1](install.ps1) next steps now emphasize `config`/`setup`. [DOCKER.md](DOCKER.md) opens with a non-recommended banner and clarifies host-prepared `.env`. Fixed misleading `setup` success message in [src/main.rs](src/main.rs) (saves `.env`, not YAML).
- **Rationale:** Onboarding docs and the example env had drifted from the actual config surface and implied Docker as a first-class path; operators need one accurate reference aligned with `src/config.rs`.
- **Key files / symbols:** `.env.example`; `README.md`; `install.sh`, `install.ps1`; `DOCKER.md`; `main.rs` — `Some("setup")` branch `println!`.

### 2026-04-09 — System prompt: repository layout and `.env` resolution

- **Area:** agent / channels / workspace principles
- **Summary:** Extended `build_system_prompt` with explicit **Repository layout and environment variables** text: config root / `FINALLY_A_VALUE_BOT_CONFIG`, `WORKSPACE_DIR` as data root, `shared/` as tool cwd, skills under `skills/`, where to put secrets vs bot-wide keys, and honest skill `load_dotenv` behavior (fills unset vars; does not override existing process env by default). The call site passes absolute workspace data root and a resolved config-path summary from `Config::resolve_config_path`.
- **Rationale:** Gives the model a single, accurate mental model for paths and env layering aligned with `workspace_root_absolute` and bundled Python skills.
- **Key files / symbols:** `src/channels/telegram.rs` — `process_with_agent_with_events`, `build_system_prompt`; `workspace/AGENTS.md` — layout bullet under on-demand tools; handling-security-keys bullet clarified for skill vs config `.env`.
- **Follow-ups:** Optional: stricter skill-overrides-repo semantics in skill scripts if product requires it.

### 2026-04-09 — Added always-applied documentation reference rule

- **Area:** repo policy / agent rules
- **Summary:** Added a new Cursor rule requiring development work to consult `docs/`, `DEVELOP.md`, and `TEST.md` before implementing/refactoring/fixing code.
- **Rationale:** Makes documentation consultation explicit and consistent during development turns, reducing drift from project conventions.
- **Key files / symbols:** `.cursor/rules/development-doc-references.mdc` (`alwaysApply: true`).

### 2026-04-09 — Vault Python: skill-directory `.env` only, embedding URL required

- **Area:** vault / builtin skills
- **Summary:** Reverted the “canonical repo-root `.env` walk” for vault Python. `index_vault.py` and `query_vault.py` (in both `builtin_skills/*/…` and `scripts/vault/`) now call `load_dotenv(SCRIPT_DIR / ".env")` only. Neither `VAULT_EMBEDDING_SERVER_URL` nor `VAULT_EMBED_URL` has a default URL; if both are missing/empty after load, the script prints to stderr and exits with code 1. Standard `python-dotenv` behavior applies: variables already set in the process environment are not overridden by the file.
- **Rationale:** Skills stay self-contained for manual runs; avoids implying a fixed localhost embedding server. Operators set embedding URL explicitly in the skill’s `.env` or export it; when the bot spawns the script, inherited env still works.
- **Key files / symbols:** `builtin_skills/index-vault/index_vault.py`, `builtin_skills/search-vault/query_vault.py`, `scripts/vault/index_vault.py`, `scripts/vault/query_vault.py` — `_require_embed_openai_base()`; `.env.example`, `builtin_skills/*/SKILL.md`, `scripts/vault/.env.example`.
- **Follow-ups:** Restart or re-sync builtin skills into `workspace/skills/` so deployed copies pick up script changes; native Rust `search_vault` path still uses app config for `VAULT_EMBEDDING_SERVER_URL` separately from these scripts.

### 2026-04-09 — Web chat file uploads (UI + `shared/upload` storage)

- **Area:** web / api / agent workspace
- **Summary:** Enabled the composer attachment flow by registering an `AttachmentAdapter` on `useLocalRuntime`, and moved persisted web uploads from `workspace_dir/uploads/web/` to `workspace_dir/shared/upload/web/<chat_id>/`. Injected `[document]` lines now include `tool_path=upload/web/...` (relative to the tool workspace `shared/`) alongside `saved_path`.
- **Rationale:** Local runtime only exposes `capabilities.attachments` when `adapters.attachments` is set, so the UI never offered uploads before. Saving under `shared/upload` aligns with `resolve_tool_working_dir` (`workspace_dir/shared`) so `read_file` and other tools can use normal relative paths.
- **Key files / symbols:**
  - `web/src/main.tsx` — `CompositeAttachmentAdapter` with `SimpleImageAttachmentAdapter`, `SimpleTextAttachmentAdapter`, `WebWildcardAttachmentAdapter` (`accept: "*"`), passed as `adapters.attachments` to `useLocalRuntime`.
  - `src/web.rs` — `process_web_attachments` directory `workspace_root_absolute().join("shared/upload/web/...")`; note format `tool_path=...`.
  - `web/dist/`* — rebuilt production bundle (`npm run build`).
- **Follow-ups:** Optional migration of files left in legacy `uploads/web/`; consider size limits for very large JSON bodies on `/api/send_stream`.

### 2026-04-08 — Web chat “master view” (queue, schedules modal, persona indicators, memory editor)

- **Area:** web / api / db
- **Summary:** Refocused the web chat into a master control view: removed background-jobs UI, moved schedules into a standalone modal, added a live queue indicator, added per-persona “new message” dots, and added a per-persona memory file viewer/editor.
- **Rationale:** Keep the main thread as the primary surface while still exposing the key operational signals and controls (queue + schedules + memory) without clutter. The persona indicator reduces missed activity across personas.
- **Key files / symbols:**
  - `web/src/main.tsx` — header control strip (status + queue), schedules modal, memory modal, time-bounded history refresh after sends.
  - `web/src/components/session-sidebar.tsx` — persona new-message dot rendering.
  - `web/src/types.ts` — `Persona.last_bot_message_at`.
  - `src/db.rs` — `list_persona_last_bot_message_at`.
  - `src/web.rs` — `api_personas` includes `last_bot_message_at`; new routes `GET/PUT /api/personas/:persona_id/memory`.
  - `web/dist/`* — rebuilt production bundle.
- **Follow-ups:** Consider tier-aware memory editing (Tier 1/2/3) in the UI; consider SSE-driven history refresh to avoid periodic polling.

### 2026-04-01 — Global projects/workflows and unified runtime timeline

- **Area:** agent / runtime / db / queue / web / config
- **Summary:** Added a first-class global `project` model, auto-learned global `workflow` model, and DB-backed run timeline events. The shared agent path now attaches project/workflow context to runs, logs timeline events, and learns reusable workflow step patterns from successful tool runs.
- **Rationale:** Continuous development tasks (single file/image/app over time) and repeated request classes need durable memory beyond transient turn context. Explicit project/workflow persistence plus deterministic loop controls reduce repeated process invention and improve long-run reliability.
- **Key files / symbols:**
  - `src/db.rs` — new tables/records/methods: `projects`, `project_artifacts`, `project_runs`, `workflows`, `workflow_executions`, `run_timeline_events`; methods `upsert_project`, `upsert_project_artifact`, `link_project_run`, `get_best_workflow_for_intent`, `upsert_workflow_learning`, `log_workflow_execution`, `append_run_timeline_event`.
  - `src/channels/telegram.rs` — `AgentRequestContext.run_key`, `AgentEvent::WorkflowSelected`, project/workflow context injection into system prompt, run timeline writes during iteration/tool execution, and workflow auto-learning persistence in `save_run_history!`.
  - `src/post_tool_evaluator.rs` — new PTE actions (`AskUser`, `HandoffBackground`, `StopWithSummary`) and deterministic no-progress signature detection.
  - `src/job_heartbeat.rs` — heartbeat writes now also append to `run_timeline_events`; workflow selection progress mapping added.
  - `src/chat_queue.rs` — queue lane metadata and diagnostics (`QueueTaskMeta`, `LaneDiagnostic`, `diagnostics`, `enqueue_with_meta`) plus long-wait warning.
  - `src/web.rs` — web runs pass `run_key` into agent context, `/api/run_status` returns timeline event count, and `/api/queue_diagnostics` exposes lane diagnostics.
  - `src/config.rs`, `src/config_wizard.rs` — reliability/learning controls: `runtime_reliability_profile`, `workflow_auto_learn`, `workflow_min_success_repetitions`, `workflow_replay_strictness`, `project_auto_association_strictness`.
  - `docs/runtime-gap-analysis.md` — new runtime parity/debt tracking doc for project/workflow learning.
- **Follow-ups:** Tighten project matching heuristics, enforce workflow replay strictness in deterministic execution policy (currently prompt-guided), and add first-class project/workflow management tools for explicit user control.

### 2026-04-01 — Memory loop guards and shared job heartbeat

- **Area:** agent / memory / background jobs / scheduler
- **Summary:** Added memory hygiene normalization for tiered writes, runtime loop guards for repeated no-evidence tool cycles, and a shared heartbeat mechanism used by both manual background jobs and scheduled runs. Added a built-in `background-handoff` skill definition to standardize delegation behavior and status contract.
- **Rationale:** Repeated "monitoring" loops and stale pending states caused user-facing repetition and unnecessary retries. A shared heartbeat model plus strict memory/status normalization reduces loop risk and improves progress visibility for long-running work.
- **Key files / symbols:**
  - `src/tools/tiered_memory.rs` — `normalize_tier2_task_states`, `normalize_tier3_recent_focus`; normalization integrated into `WriteTieredMemoryTool::execute`.
  - `src/channels/telegram.rs` — loop/evidence helpers (`is_swap_related_tool_use`, `has_new_swap_evidence`), loop-stall short-circuit in main tool loop, `mark_swap_task_stalled_best_effort`, and stricter memory-maintenance prompt contract.
  - `src/post_tool_evaluator.rs` — `has_repeated_stalled_failures` fast-path to return `complete` on repeated stalled failures (so the loop can stop and ask for user decision).
  - `src/job_heartbeat.rs` — new shared heartbeat engine (`spawn_shared_heartbeat`), policy by `JobType`, event mapping via `signal_from_agent_event`.
  - `src/background_jobs.rs` — switched to `process_with_agent_with_events` and heartbeat signaling for manual background runs (with periodic user progress updates).
  - `src/scheduler.rs` — wired scheduled runs through the same heartbeat engine with quieter policy.
  - `src/db.rs` — new `job_heartbeats` table and DB methods `upsert_job_heartbeat`, `get_job_heartbeat`.
  - `workspace/skills/background-handoff/SKILL.md` — built-in skill instructions for background delegation contract.
- **Follow-ups:** Consider exposing `job_heartbeats` in web `run_status`/SSE for a unified UI timeline that can merge foreground run events and background/scheduled heartbeat snapshots.

### 2026-04-01 — Restrict write_memory and harden tiered writes

- **Area:** agent / memory tools
- **Summary:** Limited `write_memory` to `chat_daily` appends only, removed full `MEMORY.md` replacement via that tool, and tightened post-response memory maintenance to use only tiered memory tools. Also hardened tiered memory writes to canonicalize sections and merge duplicate tier blocks instead of propagating duplicate headers.
- **Rationale:** Full-file `MEMORY.md` writes from non-tiered context risk accidental overwrites. Canonical tier writes reduce corruption/duplication risk and keep per-tier updates deterministic.
- **Key files / symbols:**
  - `src/tools/memory.rs` — `WriteMemoryTool::definition`, `WriteMemoryTool::execute`, and tests now enforce `scope: "chat_daily"` only.
  - `src/tools/tiered_memory.rs` — added `extract_tier_sections`, `render_memory_document`, and updated `replace_tier_content` to canonicalize one section per tier while preserving content from duplicate tier headers.
  - `src/channels/telegram.rs` — `run_memory_maintenance_after_response` now allows only `read_tiered_memory` and `write_tiered_memory`.
- **Follow-ups:** Consider deprecating `read_memory(scope="chat")` from prompts in favor of tiered reads only once downstream agents/tools no longer rely on it.

### 2026-04-01 — Remove web runtime config editor

- **Area:** web / frontend / api
- **Summary:** Removed the web chat runtime config feature so the UI no longer fetches or updates config at runtime. The web API config endpoints were also removed to keep behavior aligned with the frontend.
- **Rationale:** Runtime config editing in the chat UI adds an admin control path directly in the frontend and encourages mutable-in-place server config from browser sessions. This change keeps web chat focused on conversation and uses normal config files/deploy flow instead.
- **Key files / symbols:**
  - `web/src/main.tsx` — removed `Runtime Config` dialog/state and `/api/config` calls.
  - `web/src/components/session-sidebar.tsx` — removed runtime config action/button and `onOpenConfig` prop.
  - `src/web.rs` — removed `UpdateConfigRequest`, `api_get_config`, `api_update_config`, and `/api/config` router entry.
  - `web/dist/index.html`, `web/dist/assets/index-BXcaORuE.js` — rebuilt frontend bundle after source removal.
- **Follow-ups:** If admin config changes are still needed, move them behind an explicit admin interface outside the chat app (or keep them CLI/env-only).

### 2026-03-29 — Raise long-run timeout defaults to 1500s

- **Area:** agent / tools / config
- **Summary:** Increased the 600-second execution guardrails to 1500 seconds for long-running tool workflows, including the main agent tool execution timeout and tool defaults used by `bash` and `cursor_agent`.
- **Rationale:** Legitimate long processes were being cut off at 600s. The queue architecture now protects foreground responsiveness, so a larger bounded timeout improves completion rate without removing safety limits.
- **Key files / symbols:**
  - `src/channels/telegram.rs` — `TOOL_EXECUTION_TIMEOUT_SECS` changed from `600` to `1500`.
  - `src/tools/bash.rs` — default `timeout_secs` changed from `600` to `1500` (schema + runtime default).
  - `src/config.rs` — `default_cursor_agent_timeout_secs()` now `1500`; updated config field docs/default test fixture value.
  - `src/tools/cursor_agent.rs` — tool schema text updated to reflect `1500`.
  - `src/config_wizard.rs`, `src/web.rs`, `src/llm.rs` — aligned embedded default/test config values for `cursor_agent_timeout_secs`.
- **Follow-ups:** If needed, make the main tool execution timeout (`TOOL_EXECUTION_TIMEOUT_SECS`) configurable via `.env` to tune per deployment without code changes.

### 2026-03-28 — Chat-scoped background queue for agent runs

- **Area:** channels / web / scheduler / queueing
- **Summary:** Added a centralized per-`chat_id` FIFO queue and routed Telegram, Discord, WhatsApp, web send endpoints, and scheduler executions through it so agent runs are accepted immediately and processed asynchronously in deterministic order per chat.
- **Rationale:** Foreground/awaited processing blocked users from continuing conversation while a run was in progress. A shared queue removes that UX bottleneck and prevents overlapping agent loops in the same chat.
- **Key files / symbols:**
  - `src/chat_queue.rs` — `ChatRunQueue::enqueue`, per-chat lane worker lifecycle and pending-position tracking.
  - `src/channels/telegram.rs` — `AppState.chat_queue`; `handle_message` now enqueues the existing evented agent run/delivery pipeline.
  - `src/channels/discord.rs` — message handler now enqueues run execution and delivery by canonical chat.
  - `src/channels/whatsapp.rs` — webhook processing now enqueues persona runs and WhatsApp response delivery.
  - `src/scheduler.rs` — due-task execution now enqueues `run_scheduled_agent_and_finalize` into the shared chat lane.
  - `src/web.rs` — `/api/send` and `/api/send_stream` now enqueue runs and return queued acknowledgements with `run_id` + `queue_position`; request inflight accounting is released on accept.
  - `web/src/main.tsx` — adapter switched to enqueue-ack behavior (no per-token wait), tracks pending run IDs, polls `/api/run_status`, and refreshes history on completion.
- **Follow-ups:** Web SSE endpoints still emit run events, but the UI now uses queue ack + completion polling; if richer live queue dashboards are needed, add explicit queue-state API fields beyond run completion.

### 2026-03-27 — Enable markdown tables in web chat

- **Area:** web / frontend rendering
- **Summary:** Enabled GFM markdown parsing for assistant messages in the active web chat thread and added table-specific rendering/styling so pipe-table markdown is displayed as a proper, scrollable table.
- **Rationale:** The live web chat path used `makeMarkdownText()` without `remark-gfm`, so pipe-table markdown was not parsed into table nodes and could not render with readable table layout.
- **Key files / symbols:**
  - `web/src/main.tsx` — `ThreadPane`, `makeMarkdownText({ remarkPlugins, components })`, table override with `mc-md-table-scroll`.
  - `web/src/styles.css` — `.mc-md-table-scroll`, dark/light `.aui-assistant-message-content .aui-md-table/.aui-md-th/.aui-md-td` table presentation rules.
- **Follow-ups:** If users also want markdown tables for user-authored messages, wire the user message text renderer to markdown as a separate change.

### 2026-03-25 — Fix persona prefix duplication and scheduled repeat delivery

- **Area:** channels / scheduler / history shaping
- **Summary:** Made persona prefixing idempotent, stripped transport persona tags from assistant history before LLM context, preserved trailing assistant history for scheduled runs, and added duplicate-final suppression to scheduler delivery.
- **Rationale:** Repeated `[Persona]` prefixes and repeated scheduled outputs were caused by feeding prefixed transport text back into model context and missing dedupe checks on scheduler delivery paths.
- **Key files / symbols:**
  - `src/channel.rs` — `with_persona_indicator`, `normalize_persona_prefixed_text`, `strip_leading_persona_tokens`.
  - `src/channels/telegram.rs` — `load_messages_from_db(..., is_scheduled_task)`, `history_to_claude_messages(..., keep_trailing_assistant)`, `strip_transport_persona_prefix`, and interactive `should_skip_duplicate_final_delivery` check now using persona-prefixed comparison text.
  - `src/scheduler.rs` — `run_scheduled_agent_and_finalize`: duplicate-final check before `deliver_to_contact`.
- **Follow-ups:** Consider moving output safeguards (`apply_output_safeguards`) to a shared delivery boundary to fully cover tool-driven and background-job sends.

### 2026-03-23 — Memory Hygiene & Structural Integrity clause

- **Area:** agent / AGENTS.md
- **Summary:** Added rules 7-11 under a new `## Memory Hygiene & Structural Integrity` subsection in Ways of Working. Introduces vault-first archiving, rejection handling with audit trail, Tier 3 volatility cap (15 lines), stale status eviction, loop prevention, explicit cleanup triggers, a fallback policy, a pre-response checklist, and a one-time migration step.
- **Rationale:** The bot's tiered memory (MEMORY.md) was accumulating stale statuses, rejected proposals, and repeated pending-task references across sessions, leading to context pollution and repetitive outputs. The original proposal ("purge everything on rejection") conflicted with Absolute Capture and Chronological Logging, so rejection handling was rewritten to keep a one-line audit record in the ORIGIN vault.
- **Key files / symbols:**
  - `workspace/AGENTS.md` — `## Memory Hygiene & Structural Integrity` (lines 24-52): tier definitions, rules 7-11, cleanup triggers, fallback, pre-response checklist, one-time migration.
- **Follow-ups:** Formal acceptance criteria deferred to a future iteration. Once the bot has a MEMORY.md file, verify tier size limits are enforced in practice.

### 2026-03-22 — Persona indicator on all bot messages

- **Area:** channels / delivery
- **Summary:** Every outbound bot message now starts with `[PersonaName]`  so users can see which persona sent it, across all channels (Telegram, Discord, web, WhatsApp, scheduler, background jobs, and send_message tool).
- **Rationale:** Users with multiple personas had no visual cue in the message text itself about which persona was active. The bracket-prefix format is lightweight, channel-agnostic, and always shown (including the default persona).
- **Key files / symbols:**
  - `src/channel.rs` — `with_persona_indicator(db, persona_id, text)`: shared helper that resolves persona name via `db.get_persona()` and prepends `[Name]` .
  - `src/channel.rs` — `deliver_and_store_bot_message`: calls helper before storing/sending (covers send_message tool and Telegram/web direct sends).
  - `src/channel.rs` — `deliver_to_contact`: calls helper before storing and fanning out to Telegram/Discord/web bindings.
  - `src/channels/whatsapp.rs` — agent response branch: calls helper before `send_whatsapp_message` and `store_message`.
- **Follow-ups:** If users want the indicator styled differently per channel (e.g., bold in Telegram HTML, or hidden in web UI via metadata), the helper can be extended with a channel-type parameter.

