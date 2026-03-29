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
