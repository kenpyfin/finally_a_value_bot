# Learned workflows

This document describes **auto-learned workflows**: rows in SQLite that record which **tools** ran successfully for a **normalized user intent**, and optionally inject a **hint** into the agent system prompt. It is **not** GitHub Actions CI (`.github/workflows/`).

## What this feature is (and is not)

| | |
| --- | --- |
| **Is** | Per-chat, per-intent **memory of approach patterns** (tool order + structured step traces + outcome metadata), learned after agent runs that used tools. |
| **Is** | A **soft hint** appended to the system prompt when confidence is high enough. |
| **Is not** | A user-authored workflow file in the workspace, a skill, or a deterministic executor that forces tool order. |
| **Is not** | Updated by parsing the system prompt; learning uses **post-run facts** from the agent trace. |

All channels that use `process_with_agent` / `process_with_agent_with_events` in [`src/channels/telegram.rs`](../src/channels/telegram.rs) share this behavior (see [`.cursor/rules/shared-agent-path.mdc`](../.cursor/rules/shared-agent-path.mdc)).

## Storage

- **Database file:** `{workspace_dir}/runtime/finally_a_value_bot.db` (see `Database::new` in [`src/db.rs`](../src/db.rs)).
- **Tables:**
  - **`workflows`** — one row per `(owner_chat_id, intent_signature)` with `steps_json`, `confidence`, `version`, counts, timestamps.
  - **`workflow_executions`** — audit log when a workflow row was **selected** for a run (`workflow_id`, `run_key`, outcome, score).

`steps_json` remains a JSON array of tool names for compatibility. Newer rows also persist:
- `step_trace_json`: structured per-call trace (iteration/tool/duration/error/input preview).
- `approach_summary`: concise reusable approach text.
- `last_outcome` + `failure_reason`: latest outcome classification.
- `evidence_json`: evidence refs (e.g. run keys) used to justify pattern retention.

## Intent signature

Before the agent loop, `normalize_intent_signature` derives a key from the **latest user text** (after scheduler/runtime prepends): lowercase, split on non-alphanumeric, words with length ≥ 3, up to 12 words, joined with `_`. Empty input becomes `general`.

The same string is used for **lookup** and for **learning** on that run.

## Selection and system prompt

1. `get_best_workflow_for_intent(owner_chat_id, intent_signature, min_confidence)` reads from `workflows`.
2. Call sites use **`min_confidence = 0.6`** (hardcoded in `telegram.rs` next to workflow selection).
3. If a row is returned, the system prompt gains a structured hint with:
- intent signature
- confidence
- approach summary
- tool-order memory
- recent outcome/failure reason (when available)

If confidence is below the threshold, **no hint** is added (table may still be updated by learning on tool-using runs).

## Learning trigger (after each run)

In `save_run_history!`, after writing agent history to disk:

- **Gated by** `workflow_auto_learn` (`WORKFLOW_AUTO_LEARN`, default in config/wizard).
- **Requires** at least one tool call in the run (`tool_names` non-empty).
- **Calls** `upsert_workflow_learning(...)` with compatibility fields (`steps_json`) plus richer metadata (`step_trace_json`, `approach_summary`, `last_outcome`, `failure_reason`, `evidence_json`) and `score = 1.0` on success / `0.0` on failure.

The system prompt text does **not** drive `upsert_workflow_learning`; only the **observed tool sequence** and **stop reason** do.

## Confidence and evolution

**`workflows.confidence`** (used for hint selection):

- **First insert:** `confidence = score` if the run is a success (`1.0`), else `0.0`.
- **On conflict** (same chat + intent): exponential smoothing, clamped to `[0, 1]`:
  - **Success:** `new = old * 0.7 + score * 0.3` (with `score` 1.0 from caller).
  - **Failure:** `new = old * 0.7` (no positive addition).

**Latest-run overwrite behavior:** compatibility `steps_json` and richer payload fields are replaced by latest run evidence on upsert conflict. **`version`** increments on each upsert conflict.

**`workflow_min_success_repetitions`** (`WORKFLOW_MIN_SUCCESS_REPETITIONS`) is defined in config but **not** applied inside `upsert_workflow_learning` or selection today; promotion is entirely the formula above plus the **0.6** hint threshold.

## `workflow_executions` vs `confidence`

When a workflow was **selected** at the start of a run, `log_workflow_execution` records outcome and a score (`1.0` success / `0.2` failure). That **does not** update `workflows.confidence`; it is a per-run log tied to `workflow_id`.

## Configuration (env)

| Variable | Role |
| --- | --- |
| `WORKFLOW_AUTO_LEARN` | Enable/disable post-run learning. |
| `WORKFLOW_MIN_SUCCESS_REPETITIONS` | Reserved for future promotion logic (not wired to DB updates). |
| `WORKFLOW_REPLAY_STRICTNESS` | `strict` \| `adaptive` \| `loose` — captured for policy; replay is still **prompt-guided** (see [`runtime-gap-analysis.md`](runtime-gap-analysis.md)). |

## Interaction with skills and scheduled tasks

- **Skills:** Loaded into the base system prompt; the learned hint is a **separate** append. Both can coexist; the model may need to reconcile conflicting guidance.
- **Scheduled tasks:** User text often includes `[scheduler]: …`; intent normalization may differ from interactive phrasing, so a different or no workflow row may match. Scheduler policy messages (e.g. avoid `send_message`) are separate user/assistant turns; the model should follow policy even if a hint lists a tool that is inappropriate for that run.

## New machine / empty workspace

Workflows are **not** installed from the repo. A fresh DB has an empty `workflows` table until tool-using runs occur (with auto-learn on) or you **copy** `runtime/finally_a_value_bot.db` from another install.

## Code map

| Component | Location |
| --- | --- |
| Schema | [`src/db.rs`](../src/db.rs) — `CREATE TABLE workflows`, `workflow_executions` |
| Lookup / upsert / execution log | [`src/db.rs`](../src/db.rs) — `get_best_workflow_for_intent`, `upsert_workflow_learning`, `log_workflow_execution` |
| Intent normalization, hint injection, learning | [`src/channels/telegram.rs`](../src/channels/telegram.rs) — `normalize_intent_signature`, `latest_user_text`, `save_run_history!` |
| Config defaults | [`src/config.rs`](../src/config.rs), [`src/config_wizard.rs`](../src/config_wizard.rs) |
| Events / UI | `AgentEvent::WorkflowSelected`, [`src/web.rs`](../src/web.rs), [`src/job_heartbeat.rs`](../src/job_heartbeat.rs) |

## Related docs

- [`runtime-gap-analysis.md`](runtime-gap-analysis.md) — deferred items (e.g. strict replay, management tools).
- [`development-journal.md`](development-journal.md) — 2026-04-01 entry (projects/workflows/timeline).
