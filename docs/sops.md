# Standard Operating Procedures (vault SOPs)

**Workflow** in product vocabulary means an **operating procedure** documented in the **ORIGIN vault** (markdown), with structured pointers in persona memory (`tier2.sops[]`, `tier1.workflow_principles`).

The deterministic YAML workflow engine (`run_workflow`, `*.workflow.yaml`) was **removed** (2026-06-03). Execution stays in the normal agent loop: `search_vault` / `read_file` on `ORIGIN/...`, then `activate_skill` + `run_skill_script`.

## Vocabulary

| Term | Meaning |
| --- | --- |
| **SOP / workflow / pipeline** | Vault markdown procedure under `ORIGIN/...` |
| **Tier 1 `workflow_principles`** | Compressed rules in `memory_state.json` |
| **Tier 2 `sops[]`** | `{ id, vault_path, summary }` — vault path is authoritative |
| **Cron** | `schedule_task` — timing only; SOP defines execution |
| **GitHub Actions** | `.github/workflows/` — unrelated |

## Authoring SOPs

1. Create or edit markdown under the vault, e.g. `ORIGIN/Operations/SOPs/PZ-Post-Pipeline.md`.
2. Add or update `tier2.sops[]` via `patch_memory_state` / `write_tiered_memory` (Tier 2 line format: `- SOP|<id>|ORIGIN/Operations/SOPs/My-SOP.md|<summary>`).
3. Promote durable rules to `tier1.workflow_principles` when they are non-negotiable.

Use `write-vault` / Mem-Palace mining after significant updates.

## PZ post generation (canonical)

- **Vault:** [`workspace/shared/ORIGIN/Operations/SOPs/PZ-Post-Pipeline.md`](../workspace/shared/ORIGIN/Operations/SOPs/PZ-Post-Pipeline.md)
- **Trigger:** Schedule job #164 (cron) or explicit user request
- **Tools:** `image-generator`, `pz-hotify`, `schedule-job`, vault logging skills

## Code map

| Component | Location |
| --- | --- |
| SOP hook steer | [`src/sop_context_gate.rs`](../src/sop_context_gate.rs) |
| System prompt | [`src/channels/telegram.rs`](../src/channels/telegram.rs) (`sops_prompt_sections`) |
| Memory schema | [`docs/memory-framework.md`](memory-framework.md) |

## Deprecated

- [`docs/deterministic-workflows.md`](deterministic-workflows.md) — stub pointing here
- [`docs/workflow.md`](workflow.md) — SQLite learned workflows (already deprecated)
