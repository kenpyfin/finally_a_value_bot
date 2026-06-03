---
name: create-workflow
description: Author new deterministic workflows (YAML step sequences under workflows/).
when_to_use: |
  Use when the user asks to create a new workflow, define a repeatable multi-step procedure, or persist a fixed tool/script/bash sequence.
  Do not use for editing an existing workflow — activate `modify-workflow` instead.
license: MIT
platforms:
  - linux
  - darwin
  - windows
---

# Create workflow

Use this skill before **creating** a new authored workflow. The runtime requires `activate_skill` with `create-workflow` in the same turn before `write_workflow`.

## Layout

Global workflows:

```
workflows/<id>.workflow.yaml
```

Persona-scoped (optional):

```
shared/personas/{chat_id}/{persona_id}/workflows/<id>.workflow.yaml
```

## YAML schema (required fields)

```yaml
id: my-workflow-id
version: 1
description: One-line summary
enabled: true
inputs:
  optional_key:
    type: string
    default: "value"
steps:
  - id: step_one
    type: bash
    command: echo "hello"
  - id: deliver
    type: deliver
    text: "{{ steps.step_one.stdout }}"
on_error: fail
```

## Step types (Phase 1)

| type | fields |
|------|--------|
| `tool` | `tool`, `input` (JSON object, supports `{{ inputs.* }}`, `{{ steps.<id>.stdout }}`) |
| `script` | `skill_name`, `script`, optional `args`, `interpreter`, `timeout_secs` |
| `bash` | `command`, optional `timeout_secs` |
| `set` | `var`, `value` (template string) |
| `deliver` | `text`; optional `send_message: true` + `message_input` for `send_message` tool |

## Workflow

1. Clarify inputs and step order with the user when ambiguous.
2. Prefer `run_skill_script` targets in `script` steps over raw bash when a skill already exists.
3. Call `validate_workflow` with the full YAML before `write_workflow`.
4. Call `write_workflow` with the YAML string (`scope`: `global` unless persona-specific).

## Running

After creation, run with `run_workflow(workflow_id: "<id>", inputs: { ... })`. Do not re-run the same steps manually with bash in the same turn unless debugging.
