# Learned workflows

This document describes **auto-learned workflows**: rows in SQLite that record which **tools** ran successfully for a **normalized user intent**, updated after tool-using runs. **Run-start injection** of a `# Learned Workflow Hint` block into the system prompt is **disabled**; recurring patterns are intended to surface via **tiered memory** (`tier1.workflow_principles`, promoted after successful runs when thresholds are met). This is **not** GitHub Actions CI (`.github/workflows/`).

## What this feature is (and is not)

| | |
| --- | --- |
| **Is** | Per-chat, per-intent **memory of approach patterns** (tool order + structured step traces + outcome metadata), learned after agent runs that used tools. |
| **Is** | A promotion path into **`tier1.workflow_principles`** (MEMORY) after repeated successful runs, using DB row stats read post-upsert. |
| **Is not** | A user-authored workflow file in the workspace, a skill, or a deterministic executor that forces tool order. |
| **Is not** | Updated by parsing the system prompt; learning uses **post-run facts** from the agent trace. |

All channels that use `process_with_agent` / `process_with_agent_with_events` in [`src/channels/telegram.rs`](../src/channels/telegram.rs) share this behavior (see [`.cursor/rules/shared-agent-path.mdc`](../.cursor/rules/shared-agent-path.mdc)).

## Storage

- **Database file:** `{workspace_dir}/runtime/finally_a_value_bot.db` (see `Database::new` in [`src/db.rs`](../src/db.rs)).
- **Tables:**
  - **`workflows`** — one row per `(owner_chat_id, intent_signature)` with `steps_json`, `confidence`, `version`, counts, timestamps.
  - **`workflow_executions`** — audit log keyed to a `workflow_id` + `run_key` (`outcome`, `score`). The shared agent path **no longer** selects a workflow at run start, so **new** rows from that selection path are not written; legacy rows may remain.

`steps_json` remains a JSON array of tool names for compatibility. Newer rows also persist:
- `step_trace_json`: structured per-call trace (iteration/tool/duration/error/input preview).
- `approach_summary`: concise reusable approach text.
- `last_outcome` + `failure_reason`: latest outcome classification.
- `evidence_json`: evidence refs (e.g. run keys) used to justify pattern retention.

## Intent signature

Before the agent loop, `normalize_intent_signature` derives a key from the **latest user text** (after scheduler/runtime prepends): lowercase, split on non-alphanumeric, words with length ≥ 3, up to 12 words, joined with `_`. Empty input becomes `general`.

The same string is used for **learning** on that run and for **post-run promotion** (re-read after upsert).

## Run-start system prompt (disabled)

Previously, `process_with_agent` / `process_with_agent_with_events` queried `get_best_workflow_for_intent(..., min_confidence = 0.6)` and appended `# Learned Workflow Hint` plus emitted `AgentEvent::WorkflowSelected`. **That path is removed.** The `workflows` table is still updated by post-run learning (below), and successful patterns can be promoted into **`tier1.workflow_principles`** in tiered memory (see [`src/channels/telegram.rs`](../src/channels/telegram.rs) `save_run_history!` and [`src/memory.rs`](../src/memory.rs) memory rendering).

## Learning trigger (after each run)

In `save_run_history!`, after writing agent history to disk:

- **Gated by** `workflow_auto_learn` (`WORKFLOW_AUTO_LEARN`, default in config/wizard).
- **Requires** at least one tool call in the run (`tool_names` non-empty).
- **Calls** `upsert_workflow_learning(...)` with compatibility fields (`steps_json`) plus richer metadata (`step_trace_json`, `approach_summary`, `last_outcome`, `failure_reason`, `evidence_json`) and `score = 1.0` on success / `0.0` on failure.

The system prompt text does **not** drive `upsert_workflow_learning`; only the **observed tool sequence** and **stop reason** do.

## Confidence and evolution

**`workflows.confidence`** (evolves on each `upsert_workflow_learning`; used when re-reading the row for **memory promotion** after a successful run):

- **First insert:** `confidence = score` if the run is a success (`1.0`), else `0.0`.
- **On conflict** (same chat + intent): exponential smoothing, clamped to `[0, 1]`:
  - **Success:** `new = old * 0.7 + score * 0.3` (with `score` 1.0 from caller).
  - **Failure:** `new = old * 0.7` (no positive addition).

**Latest-run overwrite behavior:** compatibility `steps_json` and richer payload fields are replaced by latest run evidence on upsert conflict. **`version`** increments on each upsert conflict.

**`workflow_min_success_repetitions`** (`WORKFLOW_MIN_SUCCESS_REPETITIONS`) is applied when promoting a line into **`tier1.workflow_principles`**: after a successful tool-using run, code loads the row with `get_best_workflow_for_intent(..., min_confidence = 0.0)` and compares `success_count` to this threshold before appending a principle string.

## `workflow_executions` vs `confidence`

`log_workflow_execution` (outcome + score `1.0` / `0.2`) **does not** update `workflows.confidence`; it is a per-run log tied to `workflow_id`. With run-start selection removed, the shared agent path no longer calls it for new runs.

## Configuration (env)

| Variable | Role |
| --- | --- |
| `WORKFLOW_AUTO_LEARN` | Enable/disable post-run learning. |
| `WORKFLOW_MIN_SUCCESS_REPETITIONS` | Reserved for future promotion logic (not wired to DB updates). |
| `WORKFLOW_REPLAY_STRICTNESS` | `strict` \| `adaptive` \| `loose` — captured for policy; replay is still **prompt-guided** (see [`runtime-gap-analysis.md`](runtime-gap-analysis.md)). |

## Related agentic editing/retrieval controls

The learned-workflow feature is independent from file-edit precision controls, but they often appear together during tool-heavy runs in `process_with_agent_with_events`.

Relevant env toggles in `.env`:

| Variable | Role |
| --- | --- |
| `ALLOW_FUZZY_SEARCH_REPLACE` | Enables opt-in fuzzy fallback for `apply_search_replace` blocks (exact matching remains default). |
| `SYMBOL_EDIT_ENABLED` | Enables the `symbol_edit` tool for symbol-span replacement. |
| `POST_EDIT_VALIDATION_ENABLED` | Enables automatic post-edit validator checks in the tool loop. |
| `POST_EDIT_VALIDATION_COMMANDS` | Optional command override list (`;;` separated) for post-edit validation. |

## Interaction with skills and scheduled tasks

- **Skills:** Loaded into the base system prompt alongside tiered memory; reconcile any conflicting guidance with current constraints.
- **Scheduled tasks:** User text often includes `[scheduler]: …`; intent normalization may differ from interactive phrasing for the same underlying task. Scheduler policy messages (e.g. avoid `send_message`) are separate user/assistant turns.

## New machine / empty workspace

Workflows are **not** installed from the repo. A fresh DB has an empty `workflows` table until tool-using runs occur (with auto-learn on) or you **copy** `runtime/finally_a_value_bot.db` from another install.

## Code map

| Component | Location |
| --- | --- |
| Schema | [`src/db.rs`](../src/db.rs) — `CREATE TABLE workflows`, `workflow_executions` |
| Lookup / upsert / execution log | [`src/db.rs`](../src/db.rs) — `get_best_workflow_for_intent`, `upsert_workflow_learning`, `log_workflow_execution` |
| Intent normalization, post-run learning, promotion | [`src/channels/telegram.rs`](../src/channels/telegram.rs) — `normalize_intent_signature`, `latest_user_text`, `save_run_history!` |
| Config defaults | [`src/config.rs`](../src/config.rs), [`src/config_wizard.rs`](../src/config_wizard.rs) |
| Events / UI | `AgentEvent::WorkflowSelected` (handled in [`src/web.rs`](../src/web.rs), [`src/job_heartbeat.rs`](../src/job_heartbeat.rs); **not** emitted for workflow selection on new runs) |

## Related docs

- [`runtime-gap-analysis.md`](runtime-gap-analysis.md) — deferred items (e.g. strict replay, management tools).
- [`development-journal.md`](development-journal.md) — 2026-04-01 entry (projects/workflows/timeline).
