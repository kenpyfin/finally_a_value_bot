# Persona hook and skill policy

Per-persona hook and skill access is controlled by `persona_hook_skill_policy`.

## Table

- `chat_id`
- `persona_id`
- `allowed_hook_ids_json`
- `allowed_skill_names_json`
- `updated_at`

Primary key: `(chat_id, persona_id)`.

## Default-all semantics

Policy is intentionally permissive by default:

- No row for `(chat_id, persona_id)` => allow all hooks and all skills.
- Row exists with `allowed_hook_ids_json = NULL` => allow all hooks.
- Row exists with `allowed_skill_names_json = NULL` => allow all skills.

This preserves backward compatibility for existing personas.

## Explicit allowlist semantics

- `allowed_hook_ids_json = []` => allow no hooks.
- `allowed_skill_names_json = []` => allow no skills.
- Non-empty arrays are explicit allowlists.

## Enforcement points

- Skills:
  - Prompt catalog exposure in `process_with_agent_with_events`
  - `activate_skill` tool execution
- Hooks:
  - Hook dispatch in `hook_runtime::run_hooks_for_event`

## Web API

- `GET /api/personas/:persona_id/policy`
- `PATCH /api/personas/:persona_id/policy`
- `GET /api/skills` — full discovered catalog (`remote` for API/cross-platform skills)
- `GET /api/skills?persona_id=:id` — same catalog plus `allowed_for_persona` per row (does not hide skills)
- `GET/POST/DELETE /api/hooks` and `/api/hooks/:id` (global hook catalog)
