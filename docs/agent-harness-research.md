# Agent Harness Research

Research notes comparing FinallyAValueBot's agent engines (Classic, Deterministic, Cursor) against the Cursor SDK blackbox, Claude Code's leaked architecture, and leading open-source harnesses.

**Date:** 2026-06-28  
**Scope:** Architecture analysis only — no proprietary source redistribution. Claude Code references cite community analyses and pseudocode writeups, not the leaked TypeScript tree.

---

## Executive summary

1. **The harness principle:** The model decides; the harness enforces. ~98% of production agent engineering is deterministic infrastructure (context, permissions, recovery, persistence), not the ReAct loop itself.
2. **FinallyAValueBot** ships three engines behind one shared entry point (`process_with_agent_with_events`): **Classic** (flexible in-process loop), **Deterministic** (staged pipeline), **Cursor** (black-box delegation via Python sidecar + Cursor SDK).
3. **Classic** aligns with the industry-standard Anthropic-style tool loop (same family as Claude Code, Cline, OpenCode).
4. **Deterministic** is architecturally unusual — fixed `intent → plan → execute → consolidate` — while the market converged on one flexible loop plus a deep harness.
5. **Cursor engine** exports the entire turn to Cursor's hosted runtime; local execution is limited to sandboxed shell (`cursorsandbox`) and ripgrep against persona `cwd`.
6. **Highest-leverage gaps vs. mature harnesses:** graduated compaction, streaming tool execution, deny-first graduated permissions, append-only session persistence, diminishing-returns circuit breaker.

---

## The harness principle (high level)

> **The model decides; the harness enforces.**

As frontier models converge in raw capability, differentiation moves from the model to the **deterministic software around the model** — the code that feeds it the right context, executes actions safely, verifies results, and recovers from failure.

### Quantified in Claude Code (v2.1.88 analysis)

| Layer | Share of codebase |
|-------|-------------------|
| AI decision logic | ~1.6% |
| Deterministic infrastructure | ~98.4% |

The agent loop itself is a plain `while(true)`. The hard part is the thousand lines of careful engineering *around* it.

### Three corollaries

1. **The loop is trivial; the harness is not.** `call model → run tools → feed results back → repeat` is ~50 lines. Compaction, permissions, recovery, streaming, and memory injection are the product.
2. **The LLM only does two things:** emit text and emit tool-call JSON. Everything else is the program being careful.
3. **Minimal scaffolding, maximal harness.** Don't hard-code task-specific procedures. Invest in reusable cross-cutting infrastructure.

### Six harness contracts

A mature harness converts an unreliable, stateless text generator into a reliable, stateful actor through six enforced contracts:

```
assemble (context)  →  remember (state/compaction)  →  reach (tools)
   →  verify (feedback)  →  permit (scope/safety)  →  persist (lifecycle)
```

Wrapped around a trivial loop.

---

## How the harness is done (six subsystems)

### 1. Context assembly — what the model sees

**Principle:** Treat context as a scarce, managed resource with provenance and lifecycle.

**Claude Code (reference):**

- `getSystemPrompt()` returns an **ordered array of segments**, not one string — each becomes a separate API block with its own cache scope.
- Literal `__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__` splits **static** (globally cacheable) from **dynamic** (per-session: memory, env, MCP).
- CLAUDE.md instructions are injected as **user context**, not system prompt — deterministic rules in system, probabilistic-compliance guidance in user.
- Key files (from analyses): `prompts.ts`, `context.ts`, `claudemd.ts`, `attachments.ts`.

**FinallyAValueBot:**

- `prepare_agent_run` assembles principles (AGENTS.md) + identity/Tier-1 (system) + Tier-2+ (`[persona_context]`).
- **Gap:** No cache-scope-aware block splitting for prompt-cache efficiency.
- **Lead:** Vault + embeddings + MemPalace for long-term retrieval (richer than Claude Code's LLM header-scan of markdown files).

### 2. State & memory — what persists and how it is compressed

**Principle:** Graduated, lazy degradation (cheapest compaction first). Persist durable state append-only.

**Claude Code (reference):**

- **5 compaction shapers** before every model call (cheapest first): Budget Reduction → Snip → Microcompact → Context Collapse (non-destructive read-time projection) → Auto-Compact (LLM summary, last resort).
- Sessions: **append-only JSONL**; compaction patches the message chain at read time (`headUuid`/`anchorUuid`/`tailUuid`) — disk never destructively edited.

**FinallyAValueBot:**

- Token-aware trimming + compaction in `process_classic_agent_with_events` (fewer tiers than Claude Code).
- SQLite messages/sessions (mutable rows).
- Tiered MEMORY.md + Obsidian vault + bulletin.
- **Gap:** Graduated compaction tiers; append-only + read-time chain patching.
- **Lead:** Vector/semantic vault retrieval.

### 3. Tools & dispatch — what the model can reach

**Principle:** Tools are a supply chain — enumeration, filtering, allowlists, dedup. Optimize execution path, not just definitions.

**Claude Code (reference):**

- 5-step tool pool: enumerate (~54 tools) → mode filter → deny pre-filter → MCP integrate → dedup.
- **StreamingToolExecutor:** begins executing tools *while the model is still streaming* (latency win).
- Fallback `runTools` classifies concurrent-safe vs. exclusive tools.

**FinallyAValueBot:**

- `ToolRegistry` + `ToolAuthContext` (control-chat scoping, per-chat permissions).
- Deterministic: per-step `allowed_tools` via `filter_tool_defs` in `agent_pipeline/execute.rs`.
- Skills system under `skills/<name>/`.
- **Gap:** Streaming tool execution during generation; MCP-native dispatch.
- **Lead:** Per-step tool allowlisting in Deterministic pipeline.

### 4. Verification / feedback — closing the loop

**Principle:** Gather → act → **verify**. Traces should feed evaluation and repair.

**Claude Code (reference):**

- 27 hook events (shell, LLM-evaluated, webhook, subagent verifier).
- Dedicated Verification subagent.
- **Diminishing-returns circuit breaker:** after 3 consecutive continuations each producing <500 tokens, stop.

**FinallyAValueBot:**

- Post-tool evaluator (PTE) and **PDQE** (post-delivery quality evaluator) with corrective re-run via `pipeline_finish_turn`.
- **Lead:** PDQE post-delivery quality loop has no direct Claude Code equivalent.
- **Gap:** Stall/diminishing-returns detector on low-yield iterations.

### 5. Scope & safety — whether and how actions run

**Principle:** Execution boundary = safety boundary. Deny-first, graduated trust, reversibility-weighted oversight.

**Claude Code (reference):**

- 7 permission modes: `plan` → `default` → `acceptEdits` → `auto` → `dontAsk` → `bypassPermissions`.
- Deny-first rule engine (broadest deny wins).
- LLM **yoloClassifier** for auto-mode (two-stage: fast filter + CoT, temperature 0).
- Permissions **never restored on resume**.
- 7 overlapping safety layers; shell sandboxing.

**FinallyAValueBot:**

- `control_chat_ids`, `ToolAuthContext`, `path_guard`.
- `cursorsandbox` only in Cursor engine path.
- **Gap:** Graduated/reversibility-weighted permissions; container isolation for native engines.

### 6. Session lifecycle & subagents — isolation and continuity

**Principle:** Sub-work in isolated contexts; **summaries only** return to parent. Auditability > query power.

**Claude Code (reference):**

- Sidechain transcripts; summary-only return to parent.
- `SkillTool` (inject into current context, cheap) vs `AgentTool` (spawn isolated context, expensive).
- Isolation: worktree / remote / in-process; POSIX `flock()` coordination.

**FinallyAValueBot:**

- `sub_agent.rs` with restricted tools.
- Skills mirror SkillTool/AgentTool split conceptually.
- Shared `prepare_agent_run` + `pipeline_finish_turn` for all engines.

---

## FinallyAValueBot: three agent engines

All channels use the same entry point: `crate::channels::telegram::process_with_agent_with_events`. Engine selection is hot-reloadable via `AgentEngine` in `src/runtime_toggles.rs` (`Classic` / `Deterministic` / `Cursor`).

### Dispatch

```text
process_with_agent_with_events
  ├─ AgentEngine::Deterministic → agent_pipeline::run_deterministic_pipeline
  ├─ AgentEngine::Cursor        → cursor_engine::run_cursor_engine
  └─ default                    → process_classic_agent_with_events
```

Shared spine: `prepare_agent_run` (prep) → engine-specific loop → `pipeline_finish_turn` (compaction, persistence, PDQE).

### Classic engine

| Aspect | Implementation |
|--------|------------------|
| Loop | `'agent_loop: for iteration in 0..max_tool_iterations` in `telegram.rs` |
| Driver | LLM `stop_reason` (`tool_use` vs `end_turn`); also detects tool-use blocks directly when `stop_reason` unreliable |
| Tools | Full `ToolRegistry` |
| LLM | `ClaudeClient` / multimodel router |
| Observability | Full `IterationRecord`s, tool traces |
| Family | Same as Claude Code, Cline, OpenCode |

### Deterministic engine

| Aspect | Implementation |
|--------|------------------|
| Loop | Fixed DAG: `intent → (clarify\|direct) → plan → execute → consolidate` |
| Location | `src/agent_pipeline/` (`mod.rs`, `intent.rs`, `plan.rs`, `execute.rs`, `consolidate.rs`) |
| Tools | Same registry; **per-step `allowed_tools`**; `STEP_MAX_ITERATIONS = 3` |
| LLM tiering | Local for step execution when `ready_for_routing()`; Strategy for intent/plan/synthesis |
| Observability | `PipelineStageRecord`s per stage |
| Family | Planner-executor (rare in OSS; closest: OpenHands Planning Mode, SWE-agent) |

### Cursor engine

| Aspect | Implementation |
|--------|------------------|
| Loop | **Opaque** — delegated to Cursor SDK / hosted backend |
| Bridge | Rust → HTTP → Python sidecar → `cursor_sdk` → Node bridge → Cursor API |
| Tools | Cursor's built-in tools; zero visibility in Rust |
| Input | Flattened text prompt (`flatten_turn_prompt`, 120k cap); images unsupported v1 |
| State | Resume `agent_id` per chat/persona/session in `cursor_engine_agents` |
| Fallback | Auto-fallback to Classic on sidecar failure |
| Observability | Single synthetic `cursor_sdk` pipeline stage; no per-tool trace |

---

## Cursor SDK blackbox (five nested layers)

When Cursor engine is selected, one turn traverses:

```text
[1] Rust bot          run_cursor_engine (src/cursor_engine.rs)
      │  HTTP POST /run → NDJSON stream
      ▼
[2] Python sidecar    scripts/cursor-sdk-runner.py (aiohttp)
      │  cursor_sdk: Agent.create / resume / send
      ▼
[3] Python → Node     cursor_sdk/_bridge.py spawns bundled node runtime
      │  dist/bin/cursor-sdk-bridge.js (Connect-RPC server)
      ▼
[4] Node bridge       @cursor/sdk (TS) + Connect-RPC
      │  talks to Cursor hosted backend
      ▼
[5] Cursor backend    model + agentic loop (remote)
      │  local tool execution only:
      └─ cursorsandbox + rg (ripgrep) against persona cwd
```

### Verified on disk (runtime venv)

Path: `workspace/runtime/cursor-sdk-venv/lib/python3.12/site-packages/cursor_sdk/`

- `_vendor/bridge/` ships bundled **~129 MB Node** + JS bundle.
- `@cursor/sdk-linux-x64/bin`: **`cursorsandbox`** (4.6 MB), **`rg`** (6.5 MB) — no local model binary.
- SDK version observed: **1.0.19** (`manifest.json`).

### Implication

"Local runtime" = **local file/shell access with remote brains**. Planning, tool-calling loop, and model routing run on Cursor's servers.

### Sidecar filtering (self-imposed opacity)

`cursor-sdk-runner.py` `_stream_agent_turn` forwards only `type:"assistant"` **text** blocks. The SDK exposes richer types (`SDKToolUseMessage`, `ShellCommand`, `SDKThinkingMessage`, artifacts) that the sidecar drops. Observability could be improved at layer 2 without changing engines.

### Security notes from SDK source

- Callback auth tokens passed via **argv, not env** (agent shell tools inherit env).
- `Agent.send` injects `api_key` for server-side validation.
- Stale `agent_id` recovery: sidecar + Rust both retry with fresh session.

---

## Claude Code source leak (March 2026)

### What happened

- **Date:** 2026-03-31
- **Package:** `@anthropic-ai/claude-code` v2.1.88
- **Cause:** Bun-generated `.map` file referenced unobfuscated TypeScript on Anthropic R2 bucket
- **Scope:** ~1,884 TypeScript files, ~512K lines — **application layer only** (not model weights)
- **Fix:** v2.1.89 removed source map

### What was exposed

- Full agent loop (`query.ts`)
- System prompt assembly pipeline
- Tool definitions and 14-step tool execution pipeline
- Permission system, compaction, subagent architecture
- Feature flags, hooks, MCP integration

### Core architecture (from community analyses)

**Single entry point:** `query()` → `queryLoop()` — one ~1,730-line `async function* while(true)` used by REPL, SDK, IDE, subagents, headless.

**9-step turn pipeline:**

1. Settings resolution
2. State init
3. Context assembly
4. Five pre-model compaction shapers
5. Model call (+ StreamingToolExecutor)
6. Tool dispatch
7. Permission gate (deny-first)
8. Tool execution (14-step pipeline in `toolExecution.ts`)
9. Stop condition check

**Five stop conditions:** no tool use, max turns, context overflow, hook intervention, explicit abort.

**Key design answers (VILA-Lab / Dive-into-Claude-Code):**

| Question | Claude Code's answer |
|----------|---------------------|
| Where does reasoning live? | Model reasons; harness enforces |
| How many execution engines? | One `queryLoop` for all interfaces |
| Default safety posture? | Deny-first: deny > ask > allow |
| Binding resource constraint? | Context window; 5 compaction layers before every call |

### Trusted analysis repos (read these, not raw leak)

| Repository | Description |
|------------|-------------|
| [VILA-Lab/Dive-into-Claude-Code](https://github.com/VILA-Lab/Dive-into-Claude-Code) | Systematic paper (arXiv 2604.14228); values → principles → implementation |
| [alejandrobalderas/claude-code-from-source](https://github.com/alejandrobalderas/claude-code-from-source) | 18-chapter book, original pseudocode only |
| [Piebald-AI/claude-code-system-prompts](https://github.com/Piebald-AI/claude-code-system-prompts) | Version-tracked prompt corpus (170+ releases) |
| [Yuyz0112/claude-code-reverse](https://github.com/Yuyz0112/claude-code-reverse) | LLM traffic visualization (prompts, compaction, tool calls) |
| [thtskaran/claude-code-analysis](https://github.com/thtskaran/claude-code-analysis) | Comprehensive subsystem map from leak snapshot |

**Note:** Leaked source remains Anthropic proprietary. Use architectural analyses for design decisions; do not vendor leaked code.

---

## Open-source harness landscape (2026)

### Leaders by GitHub activity / adoption

| Harness | License | Interface | Loop | Sandbox | ~Stars |
|---------|---------|-----------|------|---------|--------|
| **OpenCode** | MIT | Terminal, 75+ providers | Flexible + plan/execute sub-agents | Native | ~165–176k |
| **OpenHands** | MIT | Web + CLI + SDK | ReAct + Planning Mode | **Docker/chroot** | ~74–77k |
| **Cline** | Apache-2.0 | IDE + CLI | ReAct + approval gates | Editor-native | ~61–63k |
| **Aider** | Apache-2.0 | Terminal | Git-aware custom loop | Native | ~40–46k |
| **Goose** | Apache-2.0 | Desktop + CLI | ReAct | Native | ~45k |

### Four comparison axes (RunLocalAI framework)

1. **Planning loop shape** — ReAct vs Anthropic-style vs custom git-aware
2. **Tool dispatch model** — monolithic vs MCP-first
3. **Sandbox isolation** — native vs container
4. **Memory + MCP** — file-based vs vector vs hybrid

### FinallyAValueBot positioning

| Dimension | Classic | Deterministic | vs OSS leaders |
|-----------|---------|---------------|----------------|
| Loop | Anthropic-style (mainstream) | Fixed pipeline (unusual) | Classic = Cline/OpenCode family |
| Tools | Closed `ToolRegistry` + skills | + per-step allowlists | OSS = MCP-first; you lead on step allowlists |
| Sandbox | Native + path_guard | Same | OpenHands leads on Docker |
| Memory | Vault + tiers + embeddings | Same | Richer long-term than most OSS |
| Multi-channel | Telegram/Discord/Web/WhatsApp/scheduler | Same | Most OSS = single interface |

---

## Comparative matrix: all engines + references

| Dimension | **Classic (FAVB)** | **Deterministic (FAVB)** | **Cursor (FAVB)** | **Claude Code** | **OpenHands** |
|-----------|-------------------|-------------------------|-------------------|-----------------|---------------|
| Loop ownership | In-process | In-process staged | Remote (Cursor) | In-process single `queryLoop` | In-process ReAct |
| Tool visibility | Full trace | Per-step trace | Text only | Full (in app) | Full in sandbox |
| Permissions | ToolAuthContext | + allowlists | None (external) | Deny-first 7 modes | Docker + policy |
| Compaction | ~1 tier | Same | N/A (flattened prompt) | 5 graduated tiers | Provider memory |
| Quality eval | PDQE | PDQE | PDQE on finish | Hooks + verifier agent | — |
| Subagents | `sub_agent.rs` | Pipeline steps | — | Sidechain summaries | Autonomous PR |
| Persistence | SQLite | SQLite | Cursor agent_id | Append-only JSONL | Docker state |

---

## Gap assessment and recommended upgrades

Ranked by leverage for FinallyAValueBot Classic engine (harness depth, not loop rewrite):

| Priority | Upgrade | Rationale | Reference |
|----------|---------|-----------|-----------|
| 1 | **Graduated compaction** (5 lazy tiers, non-destructive collapse before LLM summary) | Binding constraint is context window; cheapest fixes first | Claude Code Snip → Microcompact → Context Collapse → Auto-Compact |
| 2 | **Diminishing-returns circuit breaker** | Stop "one more fix" token burn | Claude Code: 3 turns <500 tokens → stop |
| 3 | **Streaming tool execution** for read-only tools | Latency during generation | Claude Code StreamingToolExecutor |
| 4 | **Append-only session log** + read-time compaction patching | Auditability, safe resume | Claude Code JSONL + chain patching |
| 5 | **MCP-native tool dispatch** | Interop with growing tool catalog | Cline, OpenHands, OpenCode |
| 6 | **Optional container sandbox** for autonomous runs | Safety for hands-off work | OpenHands Docker runtime |
| 7 | **Cursor sidecar observability** | Forward tool/thinking events as NDJSON | SDK types exist; sidecar filters them |
| 8 | **Git-as-state** for workspace edits | Free reversibility | Aider auto-commit model |

### On Deterministic engine

The Claude Code leak and OSS landscape both suggest the market invested in **one flexible loop + deep harness**, not separate rigid pipelines. Deterministic's auditability and cost bounding are real; consider:

- Keeping it for scheduled/background jobs where predictability matters.
- Adding "planning mode" inside Classic (OpenHands-style) for complex tasks without maintaining a wholly separate engine.

---

## Key code references (FinallyAValueBot)

| Symbol / path | Role |
|---------------|------|
| `src/channels/telegram.rs` — `process_with_agent_with_events` | Engine dispatch |
| `src/channels/telegram.rs` — `process_classic_agent_with_events` | Classic loop |
| `src/agent_pipeline/mod.rs` — `run_deterministic_pipeline` | Deterministic pipeline |
| `src/cursor_engine.rs` — `run_cursor_engine` | Cursor delegation |
| `scripts/cursor-sdk-runner.py` | Python sidecar |
| `src/cursor_sdk_sidecar.rs` — `bootstrap` | Sidecar lifecycle |
| `src/runtime_toggles.rs` — `AgentEngine` | Hot-reloadable engine toggle |
| `src/channels/agent_run_prep.rs` — `prepare_agent_run` | Shared prep |
| `telegram.rs` — `pipeline_finish_turn` | Shared finish (PDQE, persistence) |
| `src/tools/mod.rs` — `ToolRegistry` | Tool dispatch |
| `src/agent_pipeline/execute.rs` | Per-step execution + allowlists |

---

## External references

### Claude Code analyses

- [Dive into Claude Code (VILA-Lab)](https://github.com/VILA-Lab/Dive-into-Claude-Code) — arXiv:2604.14228
- [Claude Code from Source (book)](https://claude-code-from-source.com/)
- [HarrisonSec: The 1,421-Line While Loop](https://harrisonsec.com/blog/claude-code-deep-dive-query-loop/)
- [claude-wiki.com query.ts](https://claude-wiki.com/query-ts.html)
- [DEV: Reverse-Engineered 12 Versions… Then It Leaked](https://dev.to/kolkov/we-reverse-engineered-12-versions-of-claude-code-then-it-leaked-its-own-source-code-pij)

### Open-source harness comparisons

- [RunLocalAI: Agent execution systems compared](https://www.runlocalai.co/systems/agent-execution-systems)
- [Open source AI coding assistants 2026](https://www.opensourcealternatives.to/blog/best-open-source-ai-coding-assistants)
- [MorphLLM: Best AI coding agent ranked](https://www.morphllm.com/ai-coding-agent)

### Cursor SDK

- [Cursor SDK docs (Python)](https://cursor.com/docs/sdk/python)
- Local skill: `~/.cursor/skills-cursor/sdk/SKILL.md`
- Installed package (runtime): `workspace/runtime/cursor-sdk-venv/.../cursor_sdk/`

### Anthropic harness engineering (official)

- [Harness Design for Long-Running Application Development](https://www.anthropic.com/engineering/harness-design-for-long-running-tasks)
- [Effective Context Engineering for AI Agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)

---

## Related FinallyAValueBot docs

- [`docs/runtime-gap-analysis.md`](runtime-gap-analysis.md) — runtime improvement tracking
- [`docs/deterministic-workflows.md`](deterministic-workflows.md) — deterministic pipeline details
- [`docs/mcp-sdk-evaluation.md`](mcp-sdk-evaluation.md) — MCP integration evaluation
- [`docs/hooks-architecture.md`](hooks-architecture.md) — hooks system
- [`docs/development-journal.md`](development-journal.md) — implementation chronology
