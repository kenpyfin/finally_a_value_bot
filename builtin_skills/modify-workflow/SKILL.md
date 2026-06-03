---
name: modify-workflow
description: Safely update an existing deterministic workflow YAML file.
when_to_use: |
  Use when the user asks to change, extend, fix, or disable an **existing** workflow under `workflows/`.
  Do not use for brand-new workflows — activate `create-workflow` instead.
license: MIT
platforms:
  - linux
  - darwin
  - windows
---

# Modify workflow

Use this skill before mutating an **existing** workflow file. The runtime requires `activate_skill` with `modify-workflow` in the same turn before `write_workflow` when the workflow id already exists.

## Preflight (required)

1. **Identify the workflow** — `list_workflows` or user-provided id.
2. **Read current state** — `read_workflow(workflow_id: "...")` (returns parsed definition + raw YAML).
3. **Plan the delta** — step ids must stay unique; prefer additive steps over reordering unless requested.
4. **Validate** — `validate_workflow` with the full updated YAML.
5. **Write** — `write_workflow` with the complete document (replace, not patch).

## Rules

- Keep `id` in the file matching the filename (`<id>.workflow.yaml`).
- Use `enabled: false` to disable without deleting.
- Template syntax: `{{ inputs.name }}`, `{{ steps.step_id.stdout }}`, `{{ vars.name }}`.
- After edits, offer to `run_workflow` to verify when safe.

## When not to use

- Creating a new workflow id → `create-workflow` + `write_workflow`.
