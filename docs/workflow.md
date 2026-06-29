# Learned workflows (deprecated)

**Status:** Removed. SQLite auto-learned workflows (`workflows` / `workflow_executions` tables, `WORKFLOW_AUTO_LEARN`, post-run tool-sequence learning) are no longer part of the agent runtime.

Legacy rows may remain in existing databases; they are not read or updated.

## Use instead

- **Vault SOPs** — [`sops.md`](sops.md) (canonical procedures in ORIGIN markdown)
- **Tier 1 `workflow_principles`** — manual operator memory in `memory_state.json`
- **Cron scheduling** — `schedule_task` / `schedule-job` skill (not GitHub Actions `.github/workflows/`)

Authored YAML workflows (`run_workflow`) were also removed; see [`deterministic-workflows.md`](deterministic-workflows.md).
