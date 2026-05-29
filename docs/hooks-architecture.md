# Hook architecture

This document describes bot-native hooks in the shared agent path (`process_with_agent_with_events`).

## Lifecycle events

Hooks are evaluated at these boundaries:

- `BeforeTurn`
- `PreToolUse`
- `PostToolUse`
- `PostToolBatch`
- `PreStop`
- `PostDelivery`

Runtime entry point:

- `hook_runtime::run_hooks_for_event_async`

## Storage

Hook definitions are persisted in SQLite `hook_definitions`:

- `id`
- `name`
- `event_name`
- `matcher`
- `action_type`
- `action_payload_json`
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
- `builtin_deferred_commitment_guard` (PreStop deferred-work guard)
- `builtin_loop_guard` (PostToolBatch discovery/edit loop guard)

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

- `Database::is_hook_allowed_for_persona(chat_id, persona_id, hook_id)`

Default behavior remains allow-all when no persona policy row exists (or allowlist is `NULL`).

## Built-in PZ hook

Built-in seed now creates `posttool-pz-terminal-cleanup` as:

- `action_type: command`
- script: `builtin_hooks/pz-terminal-cleanup.py`
- `enabled: false` (persona-specific opt-in)

The hook returns `effects.memory_tier3_prune`, and Rust applies memory writes via `hook_actions::apply_hook_memory_effects`.

## Built-in PostDelivery focus-sync hook

Built-in seed also creates `postdelivery-persona-focus-sync` as:

- `event_name: PostDelivery`
- `action_type: builtin_persona_focus_sync`
- `enabled: true`

When matched, runtime sets a built-in trigger that runs the post-delivery persona focus sync pipeline (`run_persona_focus_sync_after_delivery`) rather than requiring inline unconditional orchestration.

## Additional built-in policy hooks

Built-in seeds also create:

- `beforeturn-scheduler-policy-context` (`BeforeTurn`, `builtin_scheduler_policy_context`, enabled)
- `pretool-turn-skill-gate` (`PreToolUse`, `builtin_turn_skill_gate`, enabled)
- `prestop-deferred-commitment-guard` (`PreStop`, `builtin_deferred_commitment_guard`, enabled)
- `postbatch-loop-guard` (`PostToolBatch`, `builtin_loop_guard`, enabled)

These replace several former inline prompt/runtime branches with hook-evaluated deterministic policy.

## Separation from delivery safeguards

Hooks are additive policy controls. Baseline delivery correctness and final-response guardrails remain in the shared agent path.
