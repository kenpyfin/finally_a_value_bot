# Hook architecture

This document describes bot-native hooks in the shared agent path (`process_with_agent_with_events`).

## Lifecycle events

Hooks are evaluated at these boundaries:

- `BeforeTurn`
- `PreToolUse`
- `PostToolUse`
- `PostToolBatch`
- `PreStop`
- `PreDelivery`
- `PostDelivery`

Runtime entry point:

- `hook_runtime::run_hooks_for_event_async`

## Shipped hook catalog

On startup/migrate, `Database::ensure_builtin_hook_definitions` syncs manifests from repository **`builtin_hooks/*.hook.json`** into SQLite (see `src/builtin_hooks.rs`). A fresh clone already includes five manifests:

| Manifest file | Event | `action_type` |
|---------------|-------|---------------|
| `postdelivery-persona-focus-sync.hook.json` | PostDelivery | `builtin_persona_focus_sync` |
| `beforeturn-scheduler-policy-context.hook.json` | BeforeTurn | `builtin_scheduler_policy_context` |
| `pretool-turn-skill-gate.hook.json` | PreToolUse | `builtin_turn_skill_gate` |
| `prestop-deferred-commitment-guard.hook.json` | PreStop | `builtin_deferred_commitment_guard` |
| `postbatch-loop-guard.hook.json` | PostToolBatch | `builtin_loop_guard` |
| `predelivery-char-limit-pdf-guard.hook.json` | PreDelivery | `builtin_char_limit_pdf_guard` |

Handlers run in Rust (`hook_runtime.rs`). Manifests are the install-time catalog, not subprocess scripts.

**Not shipped:** PZ terminal cleanup and other optional **command** hooks — operators add scripts under `{WORKSPACE_DIR}/hooks/` and register via API. Legacy `template-*` rows are deleted on migrate.

## Storage

Hook definitions are persisted in SQLite `hook_definitions`:

- `id`
- `name`
- `event_name`
- `matcher`
- `action_type`
- `action_payload_json`
- `scoped_persona_ids_json` (`NULL` = global, JSON array = persona-scoped allowlist)
- `enabled`
- `updated_at`

## Action types

Supported `action_type` values:

- `block` (inline)
- `add_context` (inline)
- `command` (subprocess hook script)
- `prompt` (LLM-based policy hook)
- `builtin_persona_focus_sync` (built-in PostDelivery bulletin/memory sync trigger)
- `builtin_scheduler_policy_context` (BeforeTurn scheduled-run policy context)
- `builtin_turn_skill_gate` (PreToolUse schedule/modify activation gate)
- `builtin_deferred_commitment_guard` (PreStop deferred-work guard; no-op when `stop_reason` is `ask_clarification`)
- `builtin_loop_guard` (PostToolBatch discovery/edit loop guard)
- `builtin_char_limit_pdf_guard` (PreDelivery: spill over-limit replies to PDF + summary)

`pz_terminal_cleanup` is no longer a framework action type. PZ cleanup is implemented as a command hook script.

## Matcher behavior

- Empty matcher => match all for event.
- `*` => match all.
- Otherwise matcher is Rust regex.
- For tool events, matcher target includes tool name plus serialized tool input/output.
- For stop/delivery events, matcher target is stop reason (fallback to assistant text).

## Hook input (stdin JSON / prompt `$ARGUMENTS`)

Hooks receive serialized runtime context, including:

- `event`
- `chat_id`, `persona_id`, `caller_channel`, `is_scheduled_task`
- `tool_name`, `tool_input`, `tool_output`, `tool_is_error`
- `stop_reason`, `assistant_text`

## Hook output (stdout JSON)

Hooks may return:

- `permission`: `allow` | `deny` | `ask`
- `reason`, `user_message`
- `agent_message`, `additional_context`
- `updated_tool_input`
- `updated_assistant_text` (PreDelivery: replace the outgoing assistant reply)
- `effects.memory_tier3_prune.terminal_pz_post_ids`

Notes:

- Exit code `2` from command hook is treated as deny.
- Other non-zero exits fail open unless `fail_closed: true`.
- `ask` is treated as a deny-style block in current runtime.

## Command hook path policy

`action_payload_json.command` must be a relative path under:

- `WORKSPACE_DIR/hooks/`
- repository `builtin_hooks/` (or `FINALLY_A_VALUE_BOT_BUILTIN_HOOKS`)

Rejected:

- absolute paths
- `..` traversal
- non-executable files

Default command-hook cwd is `WORKSPACE_DIR/shared/` unless payload sets `cwd` to `workspace`.

## Prompt hooks

Prompt hooks run a small LLM check with JSON-only contract.

- Payload keys: `prompt`, optional `timeout_secs`, `fail_closed`, `model`
- Model fallback order: payload model -> `HOOK_PROMPT_MODEL` -> `TOOL_SKILL_AGENT_MODEL` -> `ORCHESTRATOR_MODEL` -> main model

## Persona enforcement

Before a matched hook executes, runtime checks:

- Hook scope gate (`scoped_persona_ids_json`) must include the current `persona_id` unless `NULL` (global).
- `Database::is_hook_allowed_for_persona(chat_id, persona_id, hook_id)`

Default policy behavior remains allow-all when no persona policy row exists (or allowlist is `NULL`), but persona-scoped hooks still do not run outside their explicit scope.

## Optional command hooks (e.g. PZ)

Command hooks use scripts under `{WORKSPACE_DIR}/hooks/` or (for shared examples) a path resolved via `builtin_hooks/` only when you place custom scripts there yourself. **PZ is not part of the shipped `*.hook.json` catalog.** Register optional hooks with `register_hook` (preferred) or `POST /api/hooks`.

Command hooks may return `effects.memory_tier3_prune`; Rust applies writes via `hook_actions::apply_hook_memory_effects`.

## Built-in Rust policy hooks

`builtin_*` action types run inline in `hook_runtime.rs`. PostDelivery focus sync triggers `run_persona_focus_sync_after_delivery` when `builtin_persona_focus_sync` matches.

## Separation from delivery safeguards

Hooks are additive policy controls. Baseline delivery correctness and final-response guardrails remain in the shared agent path.

## Cursor engine (bot-native hooks)

When **Settings → Runtime → Agent engine** is **Cursor**, bot hooks still run in Rust — they are **not** Cursor IDE `.cursor/hooks.json` hooks.

**Full integration guide** (tools via MCP, skills prompt + execution, hook dispatch per event): [`cursor-engine-integration.md`](cursor-engine-integration.md).

| Event | Cursor engine | Classic engine |
|-------|---------------|----------------|
| `BeforeTurn` | Yes — block or inject `[hook_context]` into flattened prompt | Yes |
| `PreToolUse` | Yes — loopback MCP `tools/call` (`tool_hook_dispatch`) | Yes |
| `PostToolUse` | Yes — loopback MCP `tools/call` | Yes |
| `PostToolBatch` | Yes — end of Cursor sidecar turn | Yes |
| `PreStop` | Yes — `end_turn` + deferred-commitment nudge loop (max 2 sidecar resumes) | Yes |
| `PreDelivery` | Yes — via `pipeline_finish_turn` (transform reply before PDQE/delivery) | Yes |
| `PostDelivery` | Yes — via `pipeline_finish_turn` (assistant message appended first) | Yes |

Shared turn-boundary helpers live in `src/channels/hook_turn_bridge.rs`. Tool hooks: `src/tool_hook_dispatch.rs`, `src/cursor_mcp_bridge.rs` (`POST /internal/cursor-mcp`). Cursor dispatch: `src/cursor_engine.rs` (`run_cursor_engine`).
