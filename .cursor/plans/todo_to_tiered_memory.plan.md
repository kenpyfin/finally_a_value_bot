---
name: ""
overview: ""
todos:
  - id: todo-1771203828580-0os8hym7j
    content: ""
    status: pending
isProject: false
---

# Replace TODO with Per-Persona Tiered MEMORY (revised)

## Corrections from your feedback

1. **Each persona has its own memory file** — Path: `groups/{chat_id}/{persona_id}/MEMORY.md`. Personas are scoped to a chat (DB: `personas.chat_id`, `personas.id`), so the memory file is uniquely identified by `(chat_id, persona_id)`.
2. **Principles: only groups/AGENTS.md** — Remove `working_dir/shared/AGENTS.md` from the principles flow. Single source of truth for principles is `groups/AGENTS.md`.
3. **Per-chat AGENTS.md removed; handled by memory file** — No more `groups/{chat_id}/AGENTS.md`. All long-term and tiered memory for a persona lives in `groups/{chat_id}/{persona_id}/MEMORY.md`. Daily log can stay per-chat at `groups/{chat_id}/memory/YYYY-MM-DD.md` (shared across personas in that chat) or move to per-persona; keeping per-chat for daily log unless you prefer per-persona.

---

## Current state (for reference)

- **Principles:** Merged from `groups/AGENTS.md` + `working_dir/shared/AGENTS.md`; prompt shows path from `global_memory_path_display()` (working_dir/shared/AGENTS.md).
- **Per-chat memory:** `groups/{chat_id}/AGENTS.md` + daily logs `groups/{chat_id}/memory/YYYY-MM-DD.md`; injected as "Memories".
- **TODO:** Global `groups/TODO.json` + per-chat `groups/{chat_id}/TODO.json` with todo_read/todo_write.
- **Memory tools:** read_memory/write_memory with scopes global (working_dir/shared/AGENTS.md), chat (groups/{chat_id}/AGENTS.md), chat_daily (append to daily log).

---

## Target design

### Principles (single source)

- **Only** `data_dir/groups/AGENTS.md` is loaded and injected as "Principles (highest priority)".
- Do **not** load or merge `working_dir/shared/AGENTS.md` into principles. Display path in prompt: `groups/AGENTS.md` (or full path like `finally-a-value-bot.data/runtime/groups/AGENTS.md`).

### Per-persona tiered memory

- **Path:** `groups/{chat_id}/{persona_id}/MEMORY.md`.
- **Format:** Single markdown file with three sections:

```markdown
# Memory

## Tier 1 — Long term
(Principles-like; high bar: explicit user ask or long-term pattern.)

## Tier 2 — Mid term
(Active projects and goals.)

## Tier 3 — Short term
(Recent focus and mood; update often, e.g. daily. Not a todo list.)
```

- **Loading:** When building the system prompt, load `state.memory.read_persona_memory(chat_id, persona_id)` (new API) and inject as "Memory (this persona)" after Principles. Replace the current "Memories" block that used per-chat AGENTS.md with this tiered memory content; keep daily log injection (today/yesterday) as-is if still per-chat.

### What goes away

- **Principles:** Remove `read_global_memory()` from the principles merge; remove `global_memory_path_display()` for principles (use groups root path for display).
- **Per-chat AGENTS.md:** Remove `read_chat_memory(chat_id)` and `chat_memory_path(chat_id)`. Replace with per-persona MEMORY.md.
- **TODO:** Remove global and per-chat TODO loading and todo_read/todo_write tools (as in original plan).
- **Memory tools "global" and "chat" scope:** Repurpose or remove. Option A: Remove global/chat from read_memory/write_memory; keep only chat_daily for daily log. Option B: read_memory(global) → read-only groups/AGENTS.md; write_memory(global) removed. read_memory(chat) / write_memory(chat) → require persona_id and read/write full MEMORY.md for that persona (no tier param in old tool). New tiered tools handle tier-specific read/write. Recommended: add new tiered tools (read_tiered_memory, write_tiered_memory) with (chat_id, persona_id, tier); deprecate write_memory(chat) and read_memory(chat) in favor of tiered tools; remove write_memory(global); read_memory(global) can remain as "read principles file" (groups/AGENTS.md) for transparency.

---

## Implementation plan (revised)

### 1. Principles cleanup

- **[src/memory.rs](src/memory.rs):** Keep `read_groups_root_memory()` and `groups_root_memory_path()`. Add `groups_root_memory_path_display()` (or reuse a single "principles path" for prompt). Stop using `global_memory_path()` and `read_global_memory()` for principles. Optionally keep them for a "read only" global memory tool that reads groups/AGENTS.md.
- **[src/channels/telegram.rs](src/channels/telegram.rs):** Build principles from **only** `state.memory.read_groups_root_memory()`. Remove merge with `read_global_memory()`. Use `state.memory.groups_root_memory_path_display()` (or equivalent) for the path shown in the prompt instead of `global_memory_path_display()`.

### 2. Per-persona memory path and loading

- **[src/memory.rs](src/memory.rs):**
  - Add `persona_memory_path(chat_id, persona_id)` → `groups/{chat_id}/{persona_id}/MEMORY.md`.
  - Add `read_persona_memory(chat_id, persona_id) -> Option<String>`.
  - Change `build_memory_context(chat_id, persona_id)` to use `read_persona_memory(chat_id, persona_id)` instead of `read_chat_memory(chat_id)`. Keep daily log as `read_daily_logs_today_yesterday(chat_id)` (per-chat).
  - Remove or deprecate `chat_memory_path(chat_id)` and `read_chat_memory(chat_id)` once callers are updated.
- **Telegram (and any other caller):** Pass `persona_id` into `build_memory_context(chat_id, persona_id)`.

### 3. New tiered memory tools (per-persona)

- **New [src/tools/tiered_memory.rs](src/tools/tiered_memory.rs):**
  - **read_tiered_memory:** Params: `chat_id`, `persona_id` (and optional `tier` 1|2|3). Reads `groups/{chat_id}/{persona_id}/MEMORY.md`, returns full content or the requested tier section. Enforce auth: only current chat/persona or control chat.
  - **write_tiered_memory:** Params: `chat_id`, `persona_id`, `tier` (1|2|3), `content`. Replaces that tier’s section in MEMORY.md; preserves other tiers. Same auth as above.
- Tools need `chat_id` and `persona_id` from the request context (injected by the gateway when calling tools). Register in [src/tools/mod.rs](src/tools/mod.rs); add to `tool_risk` (write = Medium).

### 4. Memory tools (read_memory / write_memory) changes

- **read_memory:** Keep scope "global" only if we repurpose it to read `groups/AGENTS.md` (read-only principles). Remove or keep "chat"; if kept, require persona_id and read from `groups/{chat_id}/{persona_id}/MEMORY.md` (full file).
- **write_memory:** Remove scope "global" (bot must not overwrite principles). Scope "chat" → require persona_id, write to persona’s MEMORY.md (full replace) or remove "chat" in favor of write_tiered_memory only. Keep "chat_daily" for appending to daily log.
- **[src/tools/memory.rs](src/tools/memory.rs):** Update paths and schemas: add persona_id for chat scope; remove global write; optionally make global read point to groups/AGENTS.md.

### 5. Remove TODO system

- Same as original plan: remove [src/tools/todo.rs](src/tools/todo.rs), `load_global_todo_context`, todo_context from prompt, all todo mentions in capabilities and instructions.

### 6. System prompt changes (build_system_prompt in [src/channels/telegram.rs](src/channels/telegram.rs))

**Signature:** Add `persona_id: i64` (for the body text "The current chat_id is {chat_id} and persona_id is {persona_id}"). Remove the `todo_context: &str` parameter (no replacement; tiered memory is part of `memory_context`). Call site: pass `persona_id`; `memory_context = state.memory.build_memory_context(chat_id, persona_id)` already includes per-persona MEMORY.md + daily log; remove `load_global_todo_context` and the `&todo_context` argument.

**Capabilities list (the `caps` string):**

- **Remove:** `- Plan and track tasks with a todo list (todo_read, todo_write) — use this to break down complex tasks into steps, track progress, and stay organized`
- **Add:** One line for tiered memory, e.g. `- Read and update tiered memory (read_tiered_memory, write_tiered_memory) — per-persona MEMORY.md with Tier 1 (long-term principles-like), Tier 2 (active projects), Tier 3 (recent focus/mood); evaluate conversation flow and update tiers when appropriate; Tier 1 only on explicit user ask, Tier 3 often (e.g. daily). Not a todo list.`

**Body paragraphs:**

- **Change:** `The current chat_id is {chat_id}. Use this when calling send_message, schedule, export_chat, memory(chat scope), or todo tools.`  
→ `The current chat_id is {chat_id} and persona_id is {persona_id}. Use these when calling send_message, schedule, export_chat, tiered memory, or memory(chat_daily) tools.`  
(Add persona_id to the format args and to the sentence.)
- **Remove:** `For complex, multi-step tasks: use todo_write to create a plan first, then execute each step and update the todo list as you go. This helps you stay organized and lets the user see progress.`
- **Replace** the memory paragraph with something like:  
`When using memory: this persona's tiered memory is in groups/{chat_id}/{persona_id}/MEMORY.md (Tier 1 = long-term principles-like, Tier 2 = active projects, Tier 3 = recent focus/mood). Use read_tiered_memory and write_tiered_memory to read/update by tier. Update based on conversation flow: Tier 1 only on explicit user ask or long-term pattern; Tier 2 when projects/goals change; Tier 3 often as a general reminder of recent focus — not a todo list. Use write_memory with scope 'chat_daily' to append to the daily log (today/yesterday are injected at session start). Principles are in groups/AGENTS.md only; do not overwrite them.`

**Principles block (after capabilities):**

- **Change:** Header text that says "follow them over workspace, todo, or conversation" → "follow them over workspace, memory, or conversation".
- **Path shown:** Use groups/AGENTS.md path (from `groups_root_memory_path_display()`), not working_dir/shared/AGENTS.md.

**Memories block:**

- **Keep** the "# Memories" (or "# Memory (this persona)") block that injects `memory_context`. Content now comes from per-persona MEMORY.md + daily log (built by `build_memory_context(chat_id, persona_id)`). Optional: change header to "Memory (this persona)" and add one line: "The following is this persona's tiered memory and recent daily log. Use it as context; principles above take precedence."

**Todo block:**

- **Remove** the entire block that does `if !todo_context.is_empty() { ... "# Todo list (global)" ... }`. Replace with: when `tiered_memory_context` is non-empty, it is already part of `memory_context` (persona MEMORY.md is loaded in build_memory_context). So no separate "Todo list" section; the injected "Memories" content includes the tiered memory. If tiered memory is loaded separately from build_memory_context, then either (a) inject it as part of the same "# Memory" block, or (b) add a short "# Tiered memory" section with instructions only (e.g. "Update tiers via write_tiered_memory when appropriate") and inject the actual MEMORY.md content in the Memories block. Recommendation: single "# Memory (this persona)" block containing persona MEMORY.md content + daily log; no separate todo section.

**Call site (where build_system_prompt is called):**

- Compute `memory_context = state.memory.build_memory_context(chat_id, persona_id)` (so it includes persona MEMORY.md + daily log).
- Remove `todo_context = load_global_todo_context(...)` and the `&todo_context` argument.
- Pass `agents_md_path` = groups/AGENTS.md display path (e.g. `state.memory.groups_root_memory_path_display()`).
- Pass `principles_content` = only `state.memory.read_groups_root_memory().unwrap_or_default()`.

### 7. Tests (prompt-related)

- Update MemoryManager tests (remove chat_memory_path for chat_id only; add persona_memory_path, read_persona_memory, build_memory_context(chat_id, persona_id)). Update telegram tests: principles from groups only; memory from persona; no todo; prompt contains tiered memory instructions and persona_id; prompt does not contain "todo_read", "todo_write", or "Todo list (global)". Add tiered_memory tool tests.

### 8. Sub-agent and docs

- Sub-agent: Do not give write_tiered_memory; can give read_tiered_memory. Update docs (README, CLAUDE, TEST, backup, example config) for: single principles file (groups/AGENTS.md), per-persona MEMORY.md, no per-chat AGENTS.md, no TODO.

---

## File change summary (revised)


| Area                                                         | Action                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [src/memory.rs](src/memory.rs)                               | Add persona_memory_path, read_persona_memory; build_memory_context(chat_id, persona_id) using persona memory; add groups_root_memory_path_display(); stop using global_memory for principles.                                                                                                           |
| New [src/tools/tiered_memory.rs](src/tools/tiered_memory.rs) | read_tiered_memory, write_tiered_memory (chat_id, persona_id, tier).                                                                                                                                                                                                                                    |
| [src/tools/memory.rs](src/tools/memory.rs)                   | Remove write_memory(global); chat scope requires persona_id and uses groups/{chat_id}/{persona_id}/MEMORY.md; optional: global read = groups/AGENTS.md.                                                                                                                                                 |
| [src/channels/telegram.rs](src/channels/telegram.rs)         | Principles = groups/AGENTS.md only; pass persona_id to build_memory_context; **full system prompt edits** (see §6): caps (remove todo, add tiered memory), body (chat_id + persona_id, remove todo paragraph, replace memory paragraph), principles path, memories block, remove todo block, call site. |
| [src/tools/mod.rs](src/tools/mod.rs)                         | Remove todo; add tiered_memory; tool_risk for write_tiered_memory.                                                                                                                                                                                                                                      |
| Delete [src/tools/todo.rs](src/tools/todo.rs)                | Remove TODO system.                                                                                                                                                                                                                                                                                     |
| Docs                                                         | Single principle (groups/AGENTS.md); per-persona MEMORY; no per-chat AGENTS.md; no TODO.                                                                                                                                                                                                                |


---

## Elsewhere that needs updates (thorough check)

These locations reference the old TODO system, per-chat/global AGENTS.md, or memory tools and must be updated for the new design.

### Auth and tool execution

- **ToolAuthContext ([src/tools/mod.rs](src/tools/mod.rs)):** Add `persona_id: i64` to the struct. In `inject_auth_context`, add `"persona_id": auth.persona_id` to the injected JSON. In `auth_context_from_input`, read `persona_id` (default to 0 if missing for backward compat). Tiered memory tools will use `auth.caller_chat_id` and `auth.persona_id` when the agent omits them; authorization: allow (chat_id, persona_id) if caller_chat_id == chat_id and persona_id matches auth.persona_id, or if control chat allow any.
- **Tool auth construction:** In [src/channels/telegram.rs](src/channels/telegram.rs), both places that create `ToolAuthContext` (main loop ~797, and `run_memory_flush_before_compaction` ~1517) must set `persona_id` (from `context.persona_id` and from the new parameter for the flush function).
- **run_memory_flush_before_compaction ([src/channels/telegram.rs](src/channels/telegram.rs) ~1499–1595):** Pass `persona_id` into the function so the flush turn uses the correct persona’s memory. Update the flush prompt text from "write_memory tool" to "write_tiered_memory or write_memory(chat_daily)" (or similar) so the model updates tiered memory / daily log for this persona before compaction.

### Reset / session-clear user-facing messages

- **Telegram:** Line ~204: `"Conversation cleared. Stored memory (AGENTS.md) is unchanged."` → e.g. `"Conversation cleared. Principles and per-persona memory are unchanged."`
- **[src/web.rs](src/web.rs):** Lines ~1099, ~1302, ~1325: same message → same wording as above.
- **[src/channels/whatsapp.rs](src/channels/whatsapp.rs):** Line ~163: same.
- **[src/channels/discord.rs](src/channels/discord.rs):** Line ~55: same.

### Documentation

- **[README.md](README.md):** Update: remove "Plan & execute" / todo list (lines ~56, 90–91, 204, 212); update "Persistent memory" / CLAUDE.md references to principles (groups/AGENTS.md) + per-persona MEMORY.md (lines ~62, 75–76, 96–105, 148); table and control-chat note (455, 475, 477); memory example (518); layout (537, 548, 555).
- **[CLAUDE.md](CLAUDE.md):** Line 18: memory system description (global + per-chat → principles + per-persona tiered). Line 38: sub-agent "write_memory" → mention write_tiered_memory also excluded. Line 25: tools list read_memory, write_memory → add tiered_memory.
- **[TEST.md](TEST.md):** Section 12 (memory tests): paths and scopes (12.1 → per-persona MEMORY; 12.5/12.6 global write removed or reworded). Section 14 (todo): remove or replace with tiered memory tests. Section 21 matrix: todo_read/write → tiered memory; write_memory(global) row remove or adjust. 24.5 reset message. 16.4 sub-agent: "write memory" can say "write_tiered_memory/write_memory".
- **[backup.md](backup.md):** Already called out in §8; paths and descriptions (global/per-chat → principles + per-persona MEMORY).
- **[DEVELOP.md](DEVELOP.md):** MemoryManager description; read_memory/write_memory and todo_* (lines 62, 146); "Reset memory" path note.

### Config and comments

- **[finally-a-value-bot.config.example.yaml](finally-a-value-bot.config.example.yaml):** Line 46 comment: "memory global/export/todo" → "memory/export/tiered memory" or similar (no global write, no todo).

### Memory tool tests ([src/tools/memory.rs](src/tools/memory.rs))

- Tests that write/read "global" (e.g. `test_write_and_read_memory_global`): remove or change to "read principles" only (no write global). Tests that use "chat" with `chat_id`: add `persona_id`, path becomes `groups/{chat_id}/{persona_id}/MEMORY.md`. Permission tests for global write: remove or replace with "no global write" behavior. Update `test_tools` if constructor or paths change.

### Sub-agent ([src/tools/sub_agent.rs](src/tools/sub_agent.rs))

- Description string (line ~32): "read_memory" and "write memory" → add "read_tiered_memory", clarify no write_tiered_memory. In `ToolRegistry::new_sub_agent()` (in [src/tools/mod.rs](src/tools/mod.rs)): include `read_tiered_memory`, do not include `write_tiered_memory`. Sub-agent tests that assert tool names: add read_tiered_memory to allowed, assert write_tiered_memory not present.

### Integration tests

- **[tests/db_integration.rs](tests/db_integration.rs):** No direct memory/todo paths; only persona_id for sessions/messages. No change required unless we add a test that touches MEMORY.md or principles.

### build_system_prompt signature and callers

- **build_system_prompt:** Parameter `todo_context` removed; no replacement parameter if tiered memory is entirely inside `memory_context`. So signature drops `todo_context`; call site stops passing it. All tests that call `build_system_prompt(..., "", None)` or similar: remove the todo argument (one less string argument).
- **Telegram tests** that call `build_system_prompt` with 10 args: after removing todo, they take 9 args (or we keep a placeholder for backward compat — plan says remove, so 9 args). Update every test call (1863, 1874, 1883, 1891, 1898, 1906, 1954, 2183, 2218, 2412, 2421, 2428, 2434) to the new signature.

---

## Optional: daily log per-persona

If you want daily log to be per-persona (each persona has its own daily log), change `daily_log_path` to `groups/{chat_id}/{persona_id}/memory/YYYY-MM-DD.md` and update `append_daily_log` / `read_daily_logs_today_yesterday` to take `persona_id`. Otherwise keep daily log at `groups/{chat_id}/memory/YYYY-MM-DD.md` (shared by personas in that chat).