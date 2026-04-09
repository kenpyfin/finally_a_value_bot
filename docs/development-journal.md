# Development journal

Chronological log of **non-trivial** implementation work: features, refactors, architectural decisions, and fixes that affect behavior or structure.

Use **newest entries first** (reverse chronological). Each entry should be self-contained enough that a future reader (or agent) can find code and rationale quickly.

## Template (copy per entry)

```markdown
### YYYY-MM-DD — Short title

- **Area:** e.g. channels / scheduler / agent / infra
- **Summary:** What changed in one or two sentences.
- **Rationale:** Why (problem, tradeoff, constraint).
- **Key files / symbols:** Paths and notable functions or types.
- **Follow-ups:** Optional; known gaps or next steps.
```

---

<!-- Add entries below this line, newest first. -->

### 2026-04-09 — Web chat file uploads (UI + `shared/upload` storage)

- **Area:** web / api / agent workspace
- **Summary:** Enabled the composer attachment flow by registering an `AttachmentAdapter` on `useLocalRuntime`, and moved persisted web uploads from `workspace_dir/uploads/web/` to `workspace_dir/shared/upload/web/<chat_id>/`. Injected `[document]` lines now include `tool_path=upload/web/...` (relative to the tool workspace `shared/`) alongside `saved_path`.
- **Rationale:** Local runtime only exposes `capabilities.attachments` when `adapters.attachments` is set, so the UI never offered uploads before. Saving under `shared/upload` aligns with `resolve_tool_working_dir` (`workspace_dir/shared`) so `read_file` and other tools can use normal relative paths.
- **Key files / symbols:**
  - `web/src/main.tsx` — `CompositeAttachmentAdapter` with `SimpleImageAttachmentAdapter`, `SimpleTextAttachmentAdapter`, `WebWildcardAttachmentAdapter` (`accept: "*"`), passed as `adapters.attachments` to `useLocalRuntime`.
  - `src/web.rs` — `process_web_attachments` directory `workspace_root_absolute().join("shared/upload/web/...")`; note format `tool_path=...`.
  - `web/dist/*` — rebuilt production bundle (`npm run build`).
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
  - `web/dist/*` — rebuilt production bundle.
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
- **Summary:** Every outbound bot message now starts with `[PersonaName] ` so users can see which persona sent it, across all channels (Telegram, Discord, web, WhatsApp, scheduler, background jobs, and send_message tool).
- **Rationale:** Users with multiple personas had no visual cue in the message text itself about which persona was active. The bracket-prefix format is lightweight, channel-agnostic, and always shown (including the default persona).
- **Key files / symbols:**
  - `src/channel.rs` — `with_persona_indicator(db, persona_id, text)`: shared helper that resolves persona name via `db.get_persona()` and prepends `[Name] `.
  - `src/channel.rs` — `deliver_and_store_bot_message`: calls helper before storing/sending (covers send_message tool and Telegram/web direct sends).
  - `src/channel.rs` — `deliver_to_contact`: calls helper before storing and fanning out to Telegram/Discord/web bindings.
  - `src/channels/whatsapp.rs` — agent response branch: calls helper before `send_whatsapp_message` and `store_message`.
- **Follow-ups:** If users want the indicator styled differently per channel (e.g., bold in Telegram HTML, or hidden in web UI via metadata), the helper can be extended with a channel-type parameter.
