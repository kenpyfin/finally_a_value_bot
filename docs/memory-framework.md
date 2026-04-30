# Memory Framework

This project now uses a single canonical persona memory source:

- `groups/{chat_id}/{persona_id}/memory_state.json`
- `groups/{chat_id}/{persona_id}/memory_events.jsonl`

Legacy `MEMORY.md` is read for migration compatibility and converted into canonical JSON state.

## Canonical Model

`memory_state.json` includes:

- `meta` (version, revision, updated_at)
- `identity` (display_name, self_model, voice_style, non_negotiables)
- `tier1` (stable_facts, workflow_principles)
- `tier2` (active_projects)
- `tier3` (recent_focus, capped)
- `workflow_memory` (intent pattern memory entries)
- `links` (mem-palace refs)

## Safety and Manual Edits

Manual edits should use:

- `read_memory_state`
- `validate_memory_state`
- `write_memory_state` (optional `expected_revision`)
- `patch_memory_state` (deep-merge object patch)

Guardrails:

- schema normalization before write
- invariant checks (confidence range, non-empty intent signatures, tier3 cap)
- atomic write + backup
- optimistic revision conflict checks for manual writes

## Retention Learning

Workflow learning stores both compatibility and richer metadata:

- compatibility: `steps_json` (tool order)
- richer retention: `step_trace_json`, `approach_summary`, `last_outcome`, `failure_reason`, `evidence_json`

Repeated successful patterns are promoted into Tier 1 workflow principles once success support crosses configured thresholds.

## Mem-Palace Alignment

Canonical memory writes generate ORIGIN snapshot notes under:

- `shared/ORIGIN/MemorySnapshots/chat-{chat_id}-persona-{persona_id}.md`

The canonical JSON remains the operational source of truth.
Mem-Palace is used as retrieval/index acceleration on top of ORIGIN snapshots and vault notes.
