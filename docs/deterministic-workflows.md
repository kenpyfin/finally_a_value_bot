# Deterministic authored workflows (removed)

**Status:** Removed as of 2026-06-03. The YAML workflow engine (`run_workflow`, `list_workflows`, Settings → Workflows) is no longer part of the runtime.

## Use instead

- **Vault SOPs** — [`sops.md`](sops.md)
- **Tier 1 `workflow_principles` / Tier 2 `sops[]`** — pointers in `memory_state.json`
- **Cron** — `schedule_task` / `schedule-job` skill

Legacy `workspace/workflows/*.workflow.yaml` files were deleted; procedure content was migrated to vault markdown (e.g. `ORIGIN/Operations/SOPs/PZ-Post-Pipeline.md`).
