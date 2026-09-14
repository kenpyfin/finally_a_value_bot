# Gemini ADK engine (native multi-agent)

How the bot runs when **Settings → Agent engine** is **Gemini ADK** for the current persona.

This is a **native Rust** re-implementation of Google ADK-style multi-agent orchestration. There is **no sidecar**, **no Python**, and **no MCP bridge**. Tools execute in-process through `ToolRegistry` + `dispatch_tool_with_hooks`.

Upstream Google ADK ships Python / TypeScript / Go / Java / Kotlin only — there is no official Rust ADK. This engine mirrors ADK concepts (`LlmAgent`, `SequentialAgent`, `ParallelAgent`, `LoopAgent`, `transfer_to_agent`, session state) inside the bot process.

## Prerequisites

| Requirement | Why |
| --- | --- |
| Persona engine `gemini_adk` | Selects `run_gemini_adk_engine` |
| `GEMINI_API_KEY` or `GOOGLE_API_KEY` in repo-root `.env` | Never stored in SQLite |
| Valid topology (global or per-persona) | Root agent + agent graph |

## Turn flow

```text
process_with_agent_with_events
  → prepare_agent_run
  → run_gemini_adk_engine
       → load topology (persona override else global)
       → execute root agent
            LlmAgent: Gemini tool loop + transfer_to_agent / exit_loop
            Sequential / Parallel / Loop: run children
       → pipeline_finish_turn (agent_engine=gemini_adk, PDQE on)
```

## Topology

Stored as JSON:

- Global: `app_settings.GEMINI_ADK_TOPOLOGY_CONFIG`
- Per persona: `personas.gemini_adk_topology` (NULL inherits global)

Each agent has: `id`, `label`, `kind`, `model` (empty → topology default), `instruction`, `sub_agents`, optional `allowed_tools`, `output_key`, `max_iterations`.

Default topology: coordinator `LlmAgent` with researcher + executor sub-agents.

## Settings UI

Choosing **Gemini ADK** on the Agent engine page saves the persona engine and reveals:

- Default Gemini model (`GEMINI_ADK_DEFAULT_MODEL`)
- Per-persona topology editor (`GET/PATCH /api/gemini-adk/topology?persona_id=`)

There is no separate "Show settings for" preview — the selected engine is the settings panel.

## PTE / PDQE

| Evaluator | Gemini ADK |
| --- | --- |
| PTE | Never (not Classic tool loop) |
| PDQE | Runs on final delivery (same as former Deterministic) |

## Key files

| File | Role |
| --- | --- |
| [`src/gemini_adk/runner.rs`](../src/gemini_adk/runner.rs) | `run_gemini_adk_engine` |
| [`src/gemini_adk/profile.rs`](../src/gemini_adk/profile.rs) | Topology schema + DB |
| [`src/gemini_adk/config.rs`](../src/gemini_adk/config.rs) | Default model / readiness |
| [`src/gemini_adk/agent.rs`](../src/gemini_adk/agent.rs) | Synthetic tools |
| [`web/src/components/settings-gemini-adk.tsx`](../web/src/components/settings-gemini-adk.tsx) | Settings panel |

## Related

- [`cursor-engine-integration.md`](cursor-engine-integration.md) — external-runtime engine contrast
- [`local-delegate-routing.md`](local-delegate-routing.md) — Classic cost routing (orthogonal)
