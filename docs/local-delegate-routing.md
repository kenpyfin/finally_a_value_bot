# Local delegate and Classic · Cost routing

Classic agent runs can use a **local OpenAI-compatible** endpoint for read-only discovery while keeping **mutations on the strategy (cloud) model**.

## Agent engines

| Runtime setting | Behavior |
|-----------------|----------|
| **Single turn** (`classic`) | Always uses Settings → LLM for every iteration. Default, best reasoning continuity. |
| **Classic · Cost routing** (`classic_cost_routing`) | Same tool loop; after a read-only tool chain, the next iteration may route to the local model. Mutations always stay on strategy. |
| **Deterministic** | Unchanged — per-phase model picks in the Deterministic pipeline settings tab. |
| **Cursor** | Full turn via Cursor SDK sidecar; bot tools/skills/hooks per [`cursor-engine-integration.md`](cursor-engine-integration.md). |

Cost routing is active only when:

```text
agent_engine == classic_cost_routing
  AND local_delegate.routing_enabled
  AND local_delegate.local_routable()   // URL + model + tools_ok
```

If cost routing is selected but the local delegate is **not verified**, the main loop falls back to strategy-only (same as Single turn) and logs `cost_routing_requested_but_local_unverified`.

## Inverse routing

After iteration 0 (always strategy):

- If the **previous** iteration used **only read-only tools** → next iteration may use `local_readonly`.
- If any mutation ran, on the last iteration, after local errors, or when routing is inactive → **strategy**.

Read-only tools include `grep`, `read_file`, `glob`, `web_search`, memory reads, etc. Mutations include `bash`, `write_file`, `run_skill_script`, `delegate_local_subjob`, etc.

## `delegate_local_subjob`

Registered only when cost routing is active. The strategy model can delegate a **bounded read-only** inner loop to the local model (default 3 iterations, max 5). Implementation: `src/local_delegate/subjob.rs`, tool definition `src/tools/delegate_local_subjob.rs`, execution in `src/channels/telegram.rs`.

## Configuration

- **Settings → Agent engine**: choose the engine for the **current persona**. Local URL/model live on the Cost routing configure panel.
- DB keys remain `MULTIMODEL_*` for backward compatibility (`src/local_delegate/mod.rs`).

PTE, PDQE, Learn & Optimize, and Deterministic local phases may still use the configured local endpoint independently of the Classic engine choice.

## API

- `GET/PATCH /api/multimodel` — local delegate config (compat name)
- `GET/PATCH /api/runtime` — includes `local_delegate_ready`, `cost_routing_effective`, engine `classic_cost_routing`

## Migration

On boot, if `MULTIMODEL_ENABLED=true` and engine was `classic`, engine upgrades to `classic_cost_routing` once (`src/channels/telegram.rs`).
