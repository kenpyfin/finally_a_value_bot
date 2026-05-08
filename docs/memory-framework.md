# Memory Framework

This project now uses a single canonical persona memory source:

- `groups/{chat_id}/{persona_id}/memory_state.json`
- `groups/{chat_id}/{persona_id}/memory_events.jsonl`

Legacy `MEMORY.md` is read for migration compatibility and converted into canonical JSON state.

## Canonical Model

`memory_state.json` is the per-persona source of truth and has this top-level structure:

- `meta`
- `identity`
- `tier1`
- `tier2`
- `tier3`
- `workflow_memory`
- `links`

### `memory_state.json` detailed field spec

#### `meta` (state bookkeeping)

- `version` (`number`)
  - Meaning: schema version for compatibility checks.
  - Behavior: normalized to current schema version (`1`) during writes.
- `revision` (`number`)
  - Meaning: optimistic-concurrency and change-order counter for this state file.
  - Behavior: increments by 1 on each successful write.
- `updated_at` (`string`, RFC3339)
  - Meaning: timestamp of the latest successful state write.
  - Behavior: auto-populated/overwritten during writes.

#### `identity` (persona identity and response constraints)

- `display_name` (`string`)
  - Meaning: canonical persona name used in memory context.
- `self_model` (`string`)
  - Meaning: concise "who I am/how I operate" self-description.
- `voice_style` (`string`)
  - Meaning: response style/tone guidance.
- `non_negotiables` (`string[]`)
  - Meaning: hard persona constraints that should not be violated.
  - Behavior: trimmed, empties removed, deduped case-insensitively.

#### `tier1` (long-term stable memory)

- `stable_facts` (`string[]`)
  - Meaning: durable facts and background context with long retention.
  - Typical use: user preferences, persistent environment facts, evergreen instructions.
- `workflow_principles` (`string[]`)
  - Meaning: reusable high-confidence process rules learned from repeated success.
  - Promotion source: patterns in `workflow_memory.intents` with enough support.
  - Behavior: trimmed, empties removed, deduped case-insensitively.

#### `tier2` (mid-term project memory)

- `active_projects` (`object[]`)
  - Meaning: list of currently in-flight efforts.
  - Each item fields:
    - `id` (`string`): stable project key; auto-generated from `summary` if missing.
    - `status` (`string`): lifecycle flag (defaults to `"active"` if missing).
    - `summary` (`string`): human-readable project statement; required in practice (empty summaries are dropped).
    - `updated_at` (`string`, RFC3339): last modification timestamp for that project; auto-filled if missing.
  - Behavior: duplicate project IDs are collapsed to unique IDs.

#### `tier3` (short-term focus memory)

- `recent_focus` (`string[]`)
  - Meaning: most recent and quickly-changing focus items.
  - Constraint: capped to 15 entries.
  - Behavior: trimmed, empties removed, deduped case-insensitively, truncated to 15.

#### `workflow_memory` (intent-pattern retention)

- `intents` (`object[]`)
  - Meaning: one entry per recognized intent pattern.
  - Each entry fields:
    - `intent_signature` (`string`)
      - Meaning: canonical intent key (task pattern identifier).
      - Behavior: trimmed, lowercased; must be non-empty.
    - `approach_summary` (`string`)
      - Meaning: concise explanation of the selected strategy.
    - `step_trace` (`string[]`)
      - Meaning: ordered high-level step sequence/tool flow used for the attempt.
      - Behavior: trimmed/deduped.
    - `outcome` (`string`)
      - Meaning: result label for the attempt (`success`/`failure`/`unknown`).
      - Behavior: defaults to `"unknown"` when empty.
    - `failure_reason` (`string | null`)
      - Meaning: optional root cause when outcome is failed/partial.
    - `confidence` (`number`)
      - Meaning: confidence in this strategy for the given intent.
      - Constraint: must be in `[0.0, 1.0]`; clamped during normalization.
    - `support_count` (`number`)
      - Meaning: count of supporting observations (usually successful repetitions).
      - Use: ranking and promotion signal.
    - `last_seen_at` (`string`, RFC3339)
      - Meaning: last observation timestamp for this intent/strategy pair.
      - Behavior: auto-filled if missing.
    - `evidence_refs` (`string[]`)
      - Meaning: references to artifacts that justify the entry (messages/logs/notes).
      - Behavior: trimmed/deduped.
  - Collection behavior:
    - duplicate `intent_signature` entries are merged by keeping the strongest support (`support_count`)
    - entries are sorted by `support_count` descending

#### `links` (external retrieval alignment references)

- `mem_palace_refs` (`string[]`)
  - Meaning: references/paths/IDs for mem-palace notes and related snapshot artifacts.
  - Purpose: bridge canonical JSON memory with retrieval/index structures.
  - Behavior: trimmed, empties removed, deduped case-insensitively.

### `memory_events.jsonl` detailed field spec

`memory_events.jsonl` is newline-delimited JSON (JSONL). One event per line.

Per-event fields:

- `ts` (`string`, RFC3339)
  - Meaning: event creation timestamp.
- `event_type` (`string`)
  - Meaning: event classifier for downstream audit/debug processing.
  - Common values:
    - `memory_state_initialized`: initial empty state created.
    - `memory_migrated_from_markdown`: legacy `MEMORY.md` converted to JSON state.
    - `memory_parse_error`: state read failed and recovery unavailable.
    - `memory_parse_error_recovered`: primary read failed but `.json.bak` recovery succeeded.
    - `manual_edit`: explicit user/operator edit action.
- `actor` (`string`)
  - Meaning: initiator identity (`system`, `migration`, `user`, etc.).
- `chat_id` (`number`)
  - Meaning: chat/group namespace for this event.
- `persona_id` (`number`)
  - Meaning: persona namespace for this event.
- `payload` (`object`)
  - Meaning: event-specific metadata.
  - Typical payload fields by event type:
    - parse/migration events: `path`, `backup_path`, `legacy_path`, `schema_version`, `error`
    - manual edit events: changed section/field metadata (for example `field`)

### Example shape

`memory_state.json`:

```json
{
  "meta": {
    "version": 1,
    "revision": 12,
    "updated_at": "2026-05-08T16:00:00Z"
  },
  "identity": {
    "display_name": "Assistant",
    "self_model": "Pragmatic coding copilot",
    "voice_style": "concise, direct, collaborative",
    "non_negotiables": [
      "Never fabricate command output"
    ]
  },
  "tier1": {
    "stable_facts": [
      "Primary repo is Rust"
    ],
    "workflow_principles": [
      "Run validation after substantive edits"
    ]
  },
  "tier2": {
    "active_projects": [
      {
        "id": "memory-doc-spec",
        "status": "active",
        "summary": "Expand memory JSON documentation",
        "updated_at": "2026-05-08T16:00:00Z"
      }
    ]
  },
  "tier3": {
    "recent_focus": [
      "Document every nested key meaning"
    ]
  },
  "workflow_memory": {
    "intents": [
      {
        "intent_signature": "document-memory-schema",
        "approach_summary": "Read source structs then update docs",
        "step_trace": [
          "inspect memory structs",
          "write field-by-field spec"
        ],
        "outcome": "success",
        "failure_reason": null,
        "confidence": 0.9,
        "support_count": 4,
        "last_seen_at": "2026-05-08T16:00:00Z",
        "evidence_refs": [
          "README.md",
          "docs/memory-framework.md"
        ]
      }
    ]
  },
  "links": {
    "mem_palace_refs": [
      "shared/ORIGIN/MemorySnapshots/chat-123-persona-1.md"
    ]
  }
}
```

`memory_events.jsonl` line example:

```json
{"ts":"2026-05-08T16:05:00Z","event_type":"manual_edit","actor":"user","chat_id":123,"persona_id":1,"payload":{"field":"tier1.stable_facts","reason":"added durable preference"}}
```

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
