### 2026-09-03 — PTE “if in doubt, continue” burned Classic turns

- **Symptom:** After switching to `selling_oversea` (Classic override), forwarding a WeCom mail (`【企业微信邮件】… 邮件id：mailcode_…`) ran the Classic tool loop for a long time instead of stopping to ask.
- **Root cause:** PTE system prompt listed only `continue`/`complete` and said “If in doubt, say continue.” `ask_user` existed in code but the evaluator was instructed not to use it. Opaque `mailcode_` tokens have no fetch tool, so “continue” meant endless discovery.
- **Fix:** Prompt includes `ask_user`; doubt → clarification question in `reason`. User-visible `ask_user` copy is that question (stall classifier still uses the stalled retry/wait line).
- **Prevention:** Do not tell quality gates to keep looping when the next step is a guess. Keep PTE infra fail-open (`continue` on timeout/skip) separate from task-level doubt.
- **Files/refs:** `build_pte_system_prompt` / `format_pte_ask_user_reply` in `src/post_tool_evaluator.rs`; WeCom inbound text `【企业微信邮件】`; persona 28 `agent_engine_override=classic`.

### 2026-09-03 — Agent engine settings looked unsaved

- **Symptom:** Settings → Agent engine clicks did not feel like they saved (pills stuck disabled, selection snapped back to Classic, or changing the second dropdown did nothing).
- **Root cause:** (1) Engine pills held `savingKey` until `GET /api/cursor-engine` finished (sidecar `/health` can take up to 10s). (2) A failed reload after a successful PATCH nulled `personaEngine`, so the UI showed Classic. (3) **Show settings for** used the same engine names as the real picker but never persisted. (4) Inherit was removed, so there was no way to clear a persona override or edit the global default from this tab. (5) Radix Select crashes on empty/`value=""` or unmatched items, which unmounted the whole Agent engine panel.
- **Fix:** Optimistic persona PATCH, release the busy state before sidecar health, keep the last good selection on silent reload failure, restore Inherit + global default save, relabel the preview dropdown, isolate knob-panel crashes, and only mount Selects when the value exists in the item list.
- **Prevention:** Do not await sidecar health on the engine-save path. Do not reuse the engine picker control for a non-saving preview. Never pass an empty string to Radix `Select.Root`/`Select.Item`.
- **Files/refs:** `patchPersonaEngine` / `patchGlobalEngine` in `web/src/components/settings-agent-engine.tsx`; Select guards in `settings-cursor.tsx` / `settings-llm.tsx`.

### 2026-09-03 — PTE/PDQE Classic+Deterministic only (Cursor skipped)

- **Symptom:** Cursor turns waited on PDQE after the sidecar reply (and previously could hang or fail-open with a user-visible notice).
- **Root cause:** `pipeline_finish_turn` ran PDQE for every engine. Cursor has no classic tool loop for PTE and cannot re-enter the sidecar for a PDQE retry.
- **Fix:** Skip PDQE when `PipelineFinishExtras.agent_engine` starts with `cursor`. Remove the Cursor fail-open retry branch. PTE stays on the Classic tool loop only.
- **Prevention:** Do not re-enable PDQE for Cursor without a real sidecar revise path. Keep evaluator toggles for Classic/Deterministic.
- **Files/refs:** `finish_turn_with_quality_gate` in `src/channels/telegram.rs`; history step `quality_eval_skipped` `{"reason":"cursor_engine"}`.

### 2026-09-03 — Cursor FullSlim follow-ups felt disconnected from the prior turn

- **Symptom:** Influencer_PZ_3 (persona 24, Cursor) treated a short follow-up (`V6`) as a new task (Alamo HOTIFY / daily SOP) instead of the Fort Mason FILL series the user was answering.
- **Root cause:** Every Cursor message is a fresh session (intentional). FullSlim flattened history but (1) told the model to treat `[current_request]` as standalone unless it was a short reply, (2) did not highlight the last interactive pair or `[quoted_message]`, (3) could truncate the **end** of the 120k prompt, which is where the live ask sits. A scheduled assistant dump immediately before the follow-up also became the disambiguation target.
- **Fix:** Keep `Agent.create` per message. Inject `[continuation_context]` on FullSlim (quote, else last interactive pair). Skip scheduler/shell history for that pair and for PDQE short-reply disambiguation. Drop oldest `prior_turn` text when over budget instead of cutting the tail.
- **Prevention:** Do not re-enable Cursor resume/jsonl to fix chat continuity. Continuity belongs in the gateway prompt pack. Recycle sidecar is not required for this Rust-only change; rebuild/restart gateway (`./reload.sh`).
- **Files/refs:** `attach_full_slim_continuation` / `flatten_preserving_recent` in `src/cursor_delegation_prompt.rs`; `parse_quoted_message_block` in `src/agent_turn_context.rs`; PZ3 `V6` message `eae46cab-eeb9-4e97-9a0e-5f688103ac9e`.

### 2026-09-01 — Sidecar resume store duplicated gateway context and OOMed

- **Symptom:** PZ3 Cursor turns stalled ~15 minutes then stream-dropped. Sidecar later `FATAL ERROR: Reached heap limit` during `JSON.parse`. `workspace/runtime/cursor-sdk-state/` was ~1.1 GB; one chat's `checkpoints.ndjson` ~280 MB.
- **Root cause:** `Agent.resume` + persistent `JsonlLocalAgentStore` keyed by cwd+session accumulated every tool I/O. Gateway already sends full persona + bounded chat via `prepare_agent_run` each turn, so the store duplicated context and never shrank. After HTTP drop/recycle the next turn often `Agent.create` anyway but left the giant jsonl for the next resume.
- **Fix:** Every message is a new Cursor session. Sidecar always `Agent.create`, uses `{runtime}/cursor-sdk-ephemeral/{runId}`, disposes the agent and deletes the store when `/run` finishes. Rust no longer loads/saves `cursor_engine_agents`. Always FullSlim; resume-delta UI removed.
- **Prevention:** Do not persist Cursor SDK checkpoints across user messages. Close the session when the reply is delivered. Recycle sidecar after runner edits.
- **Files/refs:** `scripts/cursor-sdk-runner.mjs` (`openAgent`, `ephemeralStoreDir`); `run_cursor_engine`; log `pruned …/cursor-sdk-state`.

### 2026-09-01 — PZ3 status phrases still launched 15-minute empty Cursor turns

- **Symptom:** After reload, `Check what's the status?` and `what happened to you. you are not responding` still burned ~930s with `tools=` empty, then stream-drop. Exact `check again` ran the status path but reported no files (6h window missed 21:56 V2). Sidecar later wrote Conservatory V3 at 05:35 without delivering. Sidecar OOM'd (~4GB JSON.parse).
- **Root cause:** Status matcher required the entire message to equal a phrase. PDQE still ran (and hung 35s on dead :8080) after interrupt/status. Queue empties when the HTTP turn ends, not when the user task finishes. Silent 15-min sidecar streams record no tools.
- **Fix:** Containment match for short status/health lines; 24h then newest-files fallback; skip PDQE on `cursor_status_recheck` / `cursor_engine_stream_interrupted`.
- **Prevention:** Do not start Cursor for “are you there / what happened / check status” variants. Empty `tools=` + 930s is a wedged sidecar, not user error.
- **Files/refs:** `is_status_recheck_request` / `collect_persona_images` in `src/cursor_engine.rs`; `should_skip_pdqe`; history `20260901-045511.md`, `20260901-051330.md`, `20260901-053022.md`; sidecar heap OOM in `cursor-sdk-sidecar.stderr.log`.

### 2026-08-31 — “check again” after Cursor stream drop started another 15-minute generation

- **Symptom:** Influencer_PZ_3 Comfy queue was already empty. User sent `check again` and got another “live Cursor stream dropped” notice ~15 minutes later. They thought the request was dropped or they had made a user error.
- **Root cause:** `check again` is a short Task intent, so Cursor started a full sidecar turn. The previous Flux Fill had already written `PZ-20260831-ATTICWINDOW-V2-*` at 21:47–21:56 after the HTTP stream died at 21:23. Interrupt copy said “try a narrower request,” which encouraged resend. Interrupt also skipped `pipeline_finish_turn`, so those turns never wrote history.
- **Fix:** Status phrases (`check again`, `what happened`, …) skip the sidecar and deliver recent persona images (last 6h) plus an explicit “did not start a new generation” line. Interrupt notice now points at `check again` as summarize-only. Interrupt/status paths write history via `finish_cursor_engine_turn`.
- **Prevention:** Never treat a status poll as a new Comfy/hotify job. Empty GPU queue after a stream drop means look at disk, do not relaunch. Log `cursor_status_recheck_delivered`.
- **Files/refs:** `is_status_recheck_request` / `deliver_cursor_interrupt` in `src/cursor_engine.rs`; `cursor_stream_interrupt_notice` in `src/cursor_engine_config.rs`; PZ3 messages `ac39d612`…`a2a258b9`; artifacts `PZ-20260831-ATTICWINDOW-V2-FLUXFILL.png`.

### 2026-08-31 — Cursor persona history showed Gemini; finish stalled on PDQE retry

- **Symptom:** Influencer_PZ_3 (persona 24, engine override Cursor) showed Gemini in agent history for one run, and long PZ actions often failed to finalize.
- **Root cause:** (1) Cursor history always stamped classic `LocalDelegateRunSummary` (Settings → LLM Google/Gemini) in the run header; `Engine:` was only written inside the pipeline section. (2) Recoverable sidecar errors (503 at capacity, connect/bridge) silently called `cursor_engine_classic_fallback` → Classic Gemini, while `agent_engine` still recorded the persona override `cursor`. (3) PDQE `Retry` re-entered `pipeline_finish_turn` with the same Cursor `final_text` (no sidecar nudge), adding ~150s per retry.
- **Fix:** Always emit `Engine: {actual}`. Cursor runs record `Cursor sidecar: model @ url`. Interactive/scheduled/background Cursor no longer falls back to Classic; user sees a sidecar-unavailable notice (`cursor_engine_fallback_suppressed`). PDQE fail on Cursor fail-opens with the existing delivery notice.
- **Prevention:** Never stamp the classic strategy LLM on a Cursor run. Never treat sidecar 503 as permission to switch engines. Cursor has no classic tool loop — do not return `FinishTurnOutcome::Retry` for `extras.agent_engine` starting with `cursor`.
- **Files/refs:** `prepare_agent_run` in `src/channels/agent_run_prep.rs`; `cursor_engine_unavailable_result` in `src/cursor_engine.rs`; PDQE fail-open in `finish_turn_with_quality_gate`; `parseEngineLine` in `web/src/parse-agent-history.ts`; log `cursor_engine_fallback_suppressed`.

### 2026-08-31 — Cursor stream decode error while background jobs still ran

- **Symptom:** Videographer chat showed `This run failed before a reply was ready: Cursor sidecar stream error: error decoding response body` / `Please send your request again` (~06:40Z) while shell job `75f6fb2c` (Wait LTX Tokyo10 FLF2V concat) and several tracked Comfy jobs stayed `running`.
- **Root cause:** Interactive reqwest client uses `interactive_timeout_secs + 30` (default 930s). When that wall clock fires mid-NDJSON body, reqwest Display is `error decoding response body` (not `timed out`), so the turn was treated as a hard `Err`. Web wrapped it with `failed_turn_notice`. Spawned tmux/tracked jobs are independent of the HTTP stream and kept running.
- **Fix:** Map body-read timeout/reset to `http_timeout` / `stream_interrupted` instead of `Err`. If unfinished background jobs exist (or this turn called `spawn_background_command` / `register_tracked_job`), deliver a “work still running, do not resend” notice and keep any partial stream text.
- **Prevention:** Never treat a sidecar HTTP body decode failure as “the job is dead.” Check `background_jobs.finished_at IS NULL` before telling the user to send again. Reqwest timeout-during-body must be classified via `Error::is_timeout()`, not Display.
- **Files/refs:** `consume_sidecar_stream` / `cursor_interrupt_result` in `src/cursor_engine.rs`; `cursor_stream_interrupt_notice` in `src/cursor_engine_config.rs`; `has_unfinished_background_jobs_for_chat_persona` in `src/db.rs`; message `bffaf78a-c4e2-43f4-960a-e9c02c9b06cc`; job `75f6fb2c-65d5-4011-92e1-b59cd5bb417c`.

### 2026-08-30 — Cursor engine slow / wedged sidecar capacity

- **Symptom:** Interactive Cursor turns waited 45–60+ minutes; `/health` showed `runs_in_flight: 3` with only two live TCP connections; pep and Influencer_PZ_3 runs had `run_started` but no `run_finished`.
- **Root cause:** SDK stream loop had no inactivity watchdog (only post-stream `run.wait()` 120s bound). Hung streams pinned `runs_in_flight`, blocking idle recycle and filling concurrency. Scheduled crons shared the pool with chat under a 3600s wall clock.
- **Fix:** Sidecar stream idle timeout (`CURSOR_STREAM_IDLE_TIMEOUT_MS`, default 15m), tracked active runs + reaper force-cancel, interactive slot reserve for scheduled traffic, shorter interactive HTTP budget (`interactive_timeout_secs`, default 900s), supervisor force-recycle when `oldest_run_age_secs` stays high.
- **Prevention:** Never rely on post-stream `wait()` alone for hang detection. Alert when `runs_in_flight` exceeds open `/run` sockets or `oldest_run_age_secs` ≫ interactive timeout. Recycle sidecar after runner edits.
- **Files/refs:** `streamAgentTurn` / `tryBeginRun` in `scripts/cursor-sdk-runner.mjs`; `cursor_turn_timeout_secs` in `src/cursor_engine_config.rs`; `supervise_sidecar` in `src/cursor_sdk_sidecar.rs`; log `stream idle timeout`; `/health` `oldest_run_age_secs`.

### 2026-08-27 — Background command notices landed in main chat
- **Symptom:** In a focused web session, the "Background command started (job …). You'll receive another message when it finishes." notice (and later finished/failed notices) appeared in Main instead of that session.
- **Root cause:** `ToolAuthContext` had no session id. `deliver_to_contact` / `deliver_agent_final_to_contact` for shell jobs always passed `session_id: None`.
- **Fix:** Thread `session_id` through tool auth, persist it on `background_jobs`, and use it for start/finish/cancel delivery plus the post-shell agent handoff.
- **Prevention:** Any delayed bot notice (shell, handoff, optimizer) must copy the originating run’s `session_id`. Do not hardcode `None` on `deliver_to_contact` for work started from a focused session.
- **Files/refs:** `ToolAuthContext.session_id`; `background_jobs.session_id`; `try_enqueue_background_shell`; `deliver_shell_notification`; log `Background shell job started in tmux`.

### 2026-08-27 — Web UI delayed ~90s: Steel probe used wrong health path
- **Symptom:** After reload, gateway logged Cursor sidecar ready then stayed silent; `:10961` refused connections for ~2–3 minutes.
- **Root cause:** `steel_browser_sidecar::bootstrap` always called `wait_for_steel_health`, which probed `{STEEL_API_URL}/api/health` (404 on current Steel images; healthy path is `/v1/health`). Polls ran the full 90s even when `BROWSER_MANAGED=false`.
- **Fix:** Probe `/v1/health` then `/api/health`. Skip the health wait unless `browser_managed` is true.
- **Prevention:** Do not block gateway startup on optional browser sidecars when unmanaged. Keep doctor checks aligned with the live health path.
- **Files/refs:** `probe_steel_health` / `bootstrap` in `src/steel_browser_sidecar.rs`; `deps.steel_browser` in `src/doctor.rs`; log gap after `Cursor SDK sidecar ready` before `Starting Web UI`.

### 2026-08-26 — Cursor reply duplicated (stream + SDK result glued)
- **Symptom:** Influencer_PZ_3 stored a ~11k-char reply that repeated the same experiment summary twice: first half had token-fragment spacing (`R 4`, `CLIP Seg`); second half was the clean markdown write-up with tables and upload links.
- **Root cause:** Sidecar `streamAgentTurn` joined streamed token fragments via `joinCursorUtterances(textParts)`, then appended SDK `wait().result` and emitted it again. `done.result` carried both; Rust coalesce treated the combined blob as one final answer.
- **Fix:** Sidecar `done.result` now uses SDK `wait().result` when present, else streamed text only (never both). Rust coalesce prefers authoritative `done.result`, detects stream+result duplicates, and dedupes repeated section markers.
- **Prevention:** Never concatenate Cursor stream accumulation with SDK `result`. Recycle sidecar after runner edits; rebuild gateway for Rust coalesce.
- **Files/refs:** `streamAgentTurn` in `scripts/cursor-sdk-runner.mjs`; `coalesce_cursor_delivery_text`, `dedupe_cursor_delivery_text` in `src/cursor_engine.rs`; message `0396401f-f5ad-40d7-848e-7339cebdeb6f` persona 24.

### 2026-08-26 — Web turns could store a user message and never reply
- **Symptom:** Influencer_PZ_3 (and other Cursor web chats) showed user messages with no bot reply. Stream finished in ~1–7s (`Accepted stream run` then `Stream run finished`); no agent-history file. User had to resend (“yes. do it”, “what happened?”).
- **Root cause:** `/api/send_stream` stores the user row *before* `process_with_agent`. Cursor/sidecar `Err` mapped to HTTP 500 / stream `error` only — nothing written to `messages`. Empty finals hit `plan_agent_final_delivery` → `Skip` (Telegram substitutes `"Done."`; web did not). The UI yielded `Error: …` in the live thread, did not `loadHistory` on `error`, then the 30s history poll replaced the thread with DB (user row only).
- **Fix:** After a stored user turn, always persist a bot row: failure notice on agent `Err`, empty-turn notice instead of Skip. Stream `error` also reloads history.
- **Prevention:** Never `store_message(user)` without a following bot row for that turn. Do not treat stream-only errors as delivery. Recycle sidecar after runner edits.
- **Files/refs:** `send_and_store_response_with_events` in `src/web.rs`; `failed_turn_notice` / `EMPTY_TURN_NOTICE` in `src/final_delivery_dedupe.rs`; Skip arm in `src/channel.rs`; `web/src/app/App.tsx` stream `error`; log `agent run failed after user message stored`

### 2026-08-26 — Cursor follow-up: Agent already has active run
- **Symptom:** Sending a chat message failed with `Error: Agent agent-… already has active run`.
- **Root cause:** Local Cursor agents allow only one run at a time. The sidecar reused a pooled/resumed handle after the previous stream ended without a fully terminal `wait()`/`cancel()`, so `agent.send()` rejected the follow-up.
- **Fix:** Pass `local.force` on every sidecar send (SDK: expire stuck local run). Retry busy errors after dropping the handle; skip idle-reaping in-use pool slots. Rust clears the stored `agent_id` and retries with a full slim prompt if the error still surfaces.
- **Prevention:** Never call `agent.send()` on a local handle without `local.force` in this chatbot path. Recycle the sidecar after runner edits.
- **Files/refs:** `buildSendOptions` / `isBusyAgentError` in `scripts/cursor-sdk-runner.mjs`; `_is_busy_agent_error` in `scripts/cursor-sdk-runner.py`; `is_busy_cursor_agent_error` in `src/cursor_engine.rs`; log `agent busy`; error `already has active run`.

### 2026-08-26 — Cursor chat replies looked fragmented (progress glued together)
- **Symptom:** Sourdough web replies were run-on planning scraps (`process.The ratio screenshot is in`) even with dense delivery off. User had to ask “what's the diagnosis?” twice.
- **Root cause:** Cursor SDK streams one assistant message per tool round. Sidecar joined those with `""` and Rust `push_str`’d every `text` event into `final_text`. `done.result` was ignored once any stream text existed. Dense delivery was a no-op (`personas.dense_delivery_enabled=0`).
- **Fix:** Join utterances with `\n\n`; persist the last substantial write-up when it looks like a final answer, otherwise the paragraph-joined progress. After the stream ends, `run.wait()` is a hang watchdog (`CURSOR_RUN_WAIT_TIMEOUT_MS`, default 120s — not a turn budget) and cancelled/timed-out turns evict the pooled agent/bridge so `runs_in_flight` cannot stay > 0 and block idle recycle.
- **Prevention:** Never treat Cursor `text` events as token deltas. Never await unbounded SDK finish while holding the run counter. Recycle sidecar after runner edits.
- **Files/refs:** `coalesce_cursor_delivery_text` in `src/cursor_engine.rs`; `waitForRunResult` in `scripts/cursor-sdk-runner.mjs`; `_wait_sdk_run` in `scripts/cursor-sdk-runner.py`; sourdough history `20260826-202917.md` / `20260826-203201.md`.

### 2026-08-26 — Dense delivery dropped public PDF link
- **Symptom:** Dense-delivery replies omitted the PDF URL, or offered an internal `/api/uploads/` / localhost path instead of a public https:// link.
- **Root cause:** Upload used only catbox with reqwest's default User-Agent (Cloudflare 403/HTML). Failed uploads left `public_url=None`, so the message had no link. URL validation accepted any `https://` string, including localhost. The LLM was asked to "naturally" include the URL and often omitted it; truncation could drop it.
- **Fix:** Public-host chain (catbox → litterbox → tmpfiles → pixeldrain) with a browser UA; parse/validate only public https hosts; always append the verified URL on its own last line with a reserved character budget.
- **Prevention:** Never treat a single anonymous host as sufficient; never let the LLM be the only place the URL is inserted; reject loopback and `/api/uploads/` as "public".
- **Files/refs:** `PublicHostUploader` / `extract_public_https_url` / `finalize_delivery_message` in `src/dense_delivery_guard.rs`; log `dense_delivery: public host rejected upload` / `public HTTPS upload succeeded`.

### 2026-08-26 — Node sidecar: crypto is not defined
- **Symptom:** Cursor engine turns failed with `Error: crypto is not defined` after the Node `@cursor/sdk` sidecar upgrade.
- **Root cause:** `@cursor/sdk` 1.0.28 calls global `crypto.randomUUID()` for agent/run ids. The sidecar is `node scripts/cursor-sdk-runner.mjs`. Node 18.19 (`/usr/bin/node`, typical systemd PATH) exposes global Web Crypto for `node -e` but **not** for `.mjs` files. Interactive nvm Node 20 does have the global, so the bug only shows on the supervised sidecar.
- **Fix:** At runner startup, if `globalThis.crypto` is missing, assign `node:crypto.webcrypto` before the dynamic `import("@cursor/sdk")`.
- **Prevention:** Do not assume Node 18 file ESM has browser-like `crypto`. Keep the polyfill next to JsonlLocalAgentStore (the other Node < 22.13 workaround). Recycle the sidecar after runner edits (script mtime).
- **Files/refs:** `scripts/cursor-sdk-runner.mjs` (webcrypto polyfill); log `webcrypto=function`; error `crypto is not defined`.

### 2026-08-26 — Cursor Node sidecar failed: node:sqlite required
- **Symptom:** Cursor engine `/run` failed: `Default local agent storage requires the built-in node:sqlite module (Node >= 22.13…)`.
- **Root cause:** `@cursor/sdk` local agents default to SQLite via `node:sqlite`. The host runs Node 20.20, which does not implement that module. Docs say the default *should* fall back to JSONL, but this SDK build throws instead.
- **Fix:** Always pass `JsonlLocalAgentStore` on `Agent.create` / `Agent.resume` (`local.store`), keyed per cwd+session under `runtime/cursor-sdk-state/`.
- **Prevention:** Do not rely on SDK sqlite auto-fallback. Pin an explicit store on every local create/resume when Node < 22.13.
- **Files/refs:** `scripts/cursor-sdk-runner.mjs` (`localStoreFor`); log `local store=JsonlLocalAgentStore`.

### 2026-08-25 — Settings Cursor model list hit cursor-sdk-bridge connection fail
- **Symptom:** Settings → Agent engine → Cursor **Refresh models** often failed with a sidecar 502 (`ConnectError` / connection refused / timed out waiting for bridge discovery). Turns could flake the same way.
- **Root cause:** Python `cursor-sdk` wraps TypeScript `@cursor/sdk` by spawning `cursor-sdk-bridge.js` (Connect-RPC). `Cursor.models.list()` and every `/run` went through that subprocess. The bridge died or failed discovery while the Python sidecar kept a stale client.
- **Fix:** Default sidecar is Node `scripts/cursor-sdk-runner.mjs` running `@cursor/sdk` in-process. `/models` is a direct API list (no agent, no bridge). Local cwd + loopback MCP unchanged so bot tools still work.
- **Prevention:** Do not route Settings/model catalog or turns through `Client.launch_bridge`. Keep the Python runner only as `CURSOR_SDK_RUNNER_SCRIPT` rollback.
- **Files/refs:** `scripts/cursor-sdk-runner.mjs`; `src/cursor_sdk_sidecar.rs`; log `Bridge request failed: ConnectError` / `Timed out waiting for bridge discovery`.

### 2026-08-25 — Cockpit queue idle while runs are queued
- **Symptom:** Cockpit showed `idle` / no queue; backend still accepted runs (`Accepted stream run … queue_position=…`). `/api/queue_diagnostics` returned lanes; `/api/ops_poll` closed the connection.
- **Root cause:** `json_background_job` sliced `result_text` with `&t[..200]` mid–UTF-8 character (em dash `—`). Panics on every ops poll: `end byte index 200 is not a char boundary`.
- **Fix:** Truncate with `floor_char_boundary(200)` via `background_job_result_preview`.
- **Prevention:** Never byte-slice user/job text without a char boundary helper; cover em-dash-at-boundary in a unit test.
- **Files/refs:** `src/web.rs` (`json_background_job`, `background_job_result_preview`); journalctl `panicked at src/web.rs:1544`.

### 2026-08-25 — PreDelivery PDF guard shipped without module
- **Symptom:** Tree failed to build / hook stubbed; `pub mod delivery_char_limit_pdf_guard` referenced a file that was never committed.
- **Root cause:** Incomplete PreDelivery char-limit PDF spill was mixed into a larger hang-fix commit; only wiring landed, not the implementation module.
- **Fix:** Removed PreDelivery event, config knobs, builtin action, finish-path hooks, and docs/UI entries; migrate deletes `predelivery-char-limit-pdf-guard` rows.
- **Prevention:** Do not declare `pub mod` or ship builtin hook manifests until the handler module and tests land in the same change. Prefer persona-gated dense delivery as a clean rebuild.
- **Files/refs:** `src/hook_runtime.rs`; `ensure_builtin_hook_definitions` DELETE in `src/db.rs`; plan `dense_delivery_guard_d62ee57e`.

### 2026-08-25 — Stale Cursor sidecar survived gateway reloads
- **Symptom:** Cursor turns stayed slow / `/health` timed out after days of uptime; `reload.sh` restarted the gateway but left a multi-day `cursor-sdk-runner.py` attached.
- **Root cause:** Bootstrap attached to any reachable local runner and never recycled process-level state; sync SDK I/O could starve the sidecar loop.
- **Fix:** Idle-only `/admin/request_recycle`; Rust supervisor (soft on uptime/mtime/orphans, force after two wedged probes); `reload.sh` soft-then-force recycle via `scripts/recycle-cursor-sidecar.sh`.
- **Prevention:** Never attach forever to a reachable sidecar without age/script checks; deploy paths must recycle the runner, not only the Rust binary.
- **Files/refs:** `src/cursor_sdk_sidecar.rs` (`supervise_sidecar`); `scripts/cursor-sdk-runner.py`; `reload.sh`; log `shutting down for recycle`.

---

### 2026-08-24 — Cursor timeouts climbed after many runs (event-loop starvation)

- **Symptom:** After long uptime with many Cursor turns, bridge discovery / request timeouts rose; `/health` lagged; disconnect left `Cannot write to closing transport` and orphan bridges.
- **Root cause:** Sync SDK streaming ran on the aiohttp event loop, starving the idle/orphan reaper and other `/run`s. Client disconnect evicted the bridge while the turn still held `pooled.lock`. Cancel only checked after the next NDJSON chunk; hard abort skipped MCP revoke.
- **Fix:** Offload `_stream_agent_turn` via a worker thread + queue; cancel with `run.cancel()` on disconnect without lock-held eviction; ping outside `_POOL_GUARD`; cap concurrent `/run`s; Rust `select!` cancel poll + MCP token Drop guard.
- **Prevention:** Never run long sync Cursor SDK I/O on the sidecar event loop; never `_evict_pooled_bridge` from the write-error path while a turn holds the pool lock. Alert when `/health` `os_bridge_pids` ≫ `persona_bridges_active`.
- **Files/refs:** `scripts/cursor-sdk-runner.py`; `src/cursor_engine.rs` (`McpTokenGuard`); log `client disconnected during /run … requesting turn cancel`.

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
