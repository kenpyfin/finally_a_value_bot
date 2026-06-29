# Runtime Gap Analysis

Tracking runtime improvements for project continuity, workflow reuse, and reliability.

**Vault SOPs:** see [`docs/sops.md`](sops.md). (Not GitHub Actions `.github/workflows/`.)

## Completed

- Added global `projects` and `project_runs` persistence in `src/db.rs`.
- Added unified `run_timeline_events` persistence in `src/db.rs`.
- Added project-aware runtime context bootstrap in `src/channels/telegram.rs`.
- Extended post-tool evaluator actions in `src/post_tool_evaluator.rs` (`ask_user`, `stop_with_summary`, `handoff_background`).
- Added queue lane diagnostics metadata and API-ready snapshots in `src/chat_queue.rs`.
- Added reliability profile controls in `src/config.rs`.
- Exposed timeline event count in `/api/run_status` and queue diagnostics in `/api/queue_diagnostics` in `src/web.rs`.
- Vault SOP hook steer (`src/sop_context_gate.rs`); YAML workflow engine removed.

## In Progress

- Better project matching heuristics (current strategy uses lightweight title/type inference from latest user request).

## Deferred

- Dedicated project management tools in the LLM tool registry (`list_projects`, `switch_project`).
- Timeline streaming from DB as first-class SSE event source (currently web uses in-memory run hub and DB timeline count).
- UI visualization for run timeline + queue diagnostics.
- Scheduler unattended `[workflow:id]` path.

## Removed / deprecated

- SQLite auto-learned workflows (`WORKFLOW_AUTO_LEARN`, post-run intent → tool-sequence learning). Legacy DB tables may remain; see [`docs/workflow.md`](workflow.md).

## Acceptance Tracking

- Continuous project development context across runs: implemented.
- Deterministic anti-loop behavior before free-form continuation: implemented via loop guards + extended PTE actions.
- Unified run timeline vocabulary persisted to DB: implemented.
- Queue diagnostics with project linkage fields: implemented (metadata path present; enrichment can be expanded).