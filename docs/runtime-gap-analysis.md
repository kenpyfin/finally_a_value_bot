# Runtime Gap Analysis

Tracking runtime improvements for project continuity, workflow reuse, and reliability.

**Learned workflows (intent → tool-sequence hints, confidence, learning):** see [`docs/workflow.md`](workflow.md). (This doc’s “workflow” means that feature, not GitHub Actions.)

## Completed

- Added global `projects` and `project_runs` persistence in `src/db.rs`.
- Added global `workflows` and `workflow_executions` persistence in `src/db.rs`.
- Added unified `run_timeline_events` persistence in `src/db.rs`.
- Added project/workflow-aware runtime context bootstrap in `src/channels/telegram.rs`.
- Added workflow auto-learning hooks from executed tool sequences in `src/channels/telegram.rs`.
- Extended post-tool evaluator actions in `src/post_tool_evaluator.rs` (`ask_user`, `stop_with_summary`, `handoff_background`).
- Added queue lane diagnostics metadata and API-ready snapshots in `src/chat_queue.rs`.
- Added reliability profile and workflow learning controls in `src/config.rs`.
- Exposed timeline event count in `/api/run_status` and queue diagnostics in `/api/queue_diagnostics` in `src/web.rs`.

## In Progress

- Better project matching heuristics (current strategy uses lightweight title/type inference from latest user request).
- Workflow replay strictness enforcement (currently captured in config and prompt hints; deterministic replay policies can be tightened). `WORKFLOW_REPLAY_STRICTNESS` and `WORKFLOW_MIN_SUCCESS_REPETITIONS` are not fully enforced in code yet (see [`docs/workflow.md`](workflow.md)).

## Deferred

- Dedicated project/workflow management tools in the LLM tool registry (`list_projects`, `switch_project`, `pin_workflow`).
- Timeline streaming from DB as first-class SSE event source (currently web uses in-memory run hub and DB timeline count).
- UI visualization for run timeline + queue diagnostics.

## Acceptance Tracking

- Continuous project development context across runs: implemented.
- Auto-learned workflow memory from repeated successful runs: implemented with confidence updates.
- Deterministic anti-loop behavior before free-form continuation: implemented via loop guards + extended PTE actions.
- Unified run timeline vocabulary persisted to DB: implemented.
- Queue diagnostics with project/workflow linkage fields: implemented (metadata path present; enrichment can be expanded).