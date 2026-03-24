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
