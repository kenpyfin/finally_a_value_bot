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
