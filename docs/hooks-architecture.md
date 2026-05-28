# Hook architecture

This document describes the deterministic hook system added to the shared agent path.

## Lifecycle events

Hooks are evaluated in `process_with_agent_with_events` at these boundaries:

- `BeforeTurn` (once before iteration loop)
- `PreToolUse` (before each tool execution)
- `PostToolUse` (after each tool execution)
- `PostToolBatch` (after the full tool batch in one iteration)
- `PreStop` (before finalizing an `end_turn` response)
- `PostDelivery` (after final response text is finalized)

Event evaluation entry point:

- `hook_runtime::run_hooks_for_event`

## Storage

Hook definitions live in SQLite:

- `hook_definitions`
  - `id`
  - `name`
  - `event_name`
  - `matcher`
  - `action_type`
  - `action_payload_json`
  - `enabled`
  - `updated_at`

## Supported action types

- `block`: stops the current lifecycle action; payload key `reason`.
- `add_context`: appends deterministic context; payload key `additional_context` (or `context`).

Unknown action types are ignored.

## Matcher behavior

- Empty matcher means match all for that event.
- `*` means match all.
- Otherwise matcher is a Rust regex.
- For tool events, matcher is evaluated against `tool_name`.
- For stop/delivery events, matcher is evaluated against `stop_reason`.

## Persona enforcement

Before a matched hook executes, runtime checks persona policy:

- `Database::is_hook_allowed_for_persona(chat_id, persona_id, hook_id)`

Default behavior is allow-all when no policy row exists or when `allowed_hook_ids_json` is `NULL`.

## Delivery guard relation

The hook system is separate from baseline delivery safeguards. Delivery correctness is still enforced in the shared agent path, and hooks are an additional deterministic control layer rather than the sole protection.
