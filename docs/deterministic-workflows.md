# Deterministic authored workflows

Authored workflows are **YAML files** on disk that define a **fixed step sequence** executed by Rust (`run_workflow` tool). The LLM chooses when to create, edit, or run workflows; **step order inside a run is not LLM-planned**.

This is separate from:

- **Learned workflows** ([`workflow.md`](workflow.md)) — SQLite observational memory of past tool order
- **GitHub Actions** (`.github/workflows/`)
- **Cron schedules** (`schedule_task`) — use `run_workflow` inside a scheduled agent prompt when needed

## Tools

| Tool | Purpose |
| --- | --- |
| `list_workflows` | Discover workflow ids (global + persona scope) |
| `read_workflow` | Load definition + raw YAML |
| `validate_workflow` | Parse/validate without writing |
| `write_workflow` | Create or replace a workflow file |
| `run_workflow` | Execute steps deterministically |

Disabled when `WORKFLOW_ENGINE_ENABLED=false`.

## Skills

| Skill | When |
| --- | --- |
| `create-workflow` | Before `write_workflow` for a **new** id |
| `modify-workflow` | Before `write_workflow` when the id **already exists** |

Enforced via `builtin_turn_skill_gate` and `workflow_activation_gate.rs`.

## Storage

| Scope | Path |
| --- | --- |
| Global | `{WORKSPACE_DIR}/workflows/{id}.workflow.yaml` |
| Persona | `{WORKSPACE_DIR}/shared/personas/{chat_id}/{persona_id}/workflows/{id}.workflow.yaml` |

Persona definitions override the same id when resolved with persona scope.

## Configuration

| Variable | Default |
| --- | --- |
| `WORKFLOW_ENGINE_ENABLED` | `true` |
| `WORKFLOW_DEFINITIONS_DIR` | `workflows` |
| `WORKFLOW_ALLOW_PERSONA_SCOPE` | `true` |
| `WORKFLOW_MAX_STEPS` | `50` |

## Execution

`run_workflow` runs in the normal agent tool loop. Each step uses the same auth, hooks (`PreToolUse` / `PostToolUse`), and redaction as interactive tools. Timeline events: `workflow_start`, `workflow_step_start`, `workflow_step_end`, `workflow_complete`, `workflow_failed`.

## Code map

| Component | Location |
| --- | --- |
| Engine | [`src/workflow_engine/`](../src/workflow_engine/) |
| Tools | [`src/tools/workflow.rs`](../src/tools/workflow.rs) |
| Activation gate | [`src/workflow_activation_gate.rs`](../src/workflow_activation_gate.rs) |
| Builtin skills | [`builtin_skills/create-workflow/`](../builtin_skills/create-workflow/), [`builtin_skills/modify-workflow/`](../builtin_skills/modify-workflow/) |

## Example

See [`workspace/workflows/echo-demo.workflow.yaml`](../workspace/workflows/echo-demo.workflow.yaml).
