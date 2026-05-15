export type ParsedAgentHistory = {
  runHeader: string
  iterations: { index: number; body: string }[]
  /** Pretty JSON from the server (`initial_llm_request_v1`); null for runs saved before snapshots existed. */
  initialPromptJson: string | null
}

/** Must match `agent_history::SNAPSHOT_SECTION_START` in Rust. */
export const AGENT_HISTORY_SNAPSHOT_SECTION_MARKER = '\n## Initial LLM prompt (debug snapshot)\n'

export function splitAgentHistoryRaw(content: string): {
  traceMarkdown: string
  initialPromptJson: string | null
} {
  const idx = content.indexOf(AGENT_HISTORY_SNAPSHOT_SECTION_MARKER)
  if (idx < 0) {
    return { traceMarkdown: content, initialPromptJson: null }
  }
  const traceMarkdown = content.slice(0, idx).trimEnd()
  const initialPromptJson = content.slice(idx + AGENT_HISTORY_SNAPSHOT_SECTION_MARKER.length).trim() || null
  return { traceMarkdown, initialPromptJson }
}

/**
 * Splits persisted agent run markdown (`AgentRunRecord::to_markdown`) into a run header
 * and per-iteration sections (`## Iteration N`). Pass **trace-only** markdown (no snapshot suffix)
 * if you already called `splitAgentHistoryRaw`.
 */
export function parseAgentHistoryMarkdown(traceMarkdown: string): Omit<ParsedAgentHistory, 'initialPromptJson'> {
  const re = /^## Iteration (\d+)\s*$/gm
  const parts = traceMarkdown.split(re)
  const runHeader = (parts[0] ?? '').trimEnd()
  const iterations: { index: number; body: string }[] = []
  for (let i = 1; i + 1 < parts.length; i += 2) {
    const idx = parseInt(parts[i]!, 10)
    const body = (parts[i + 1] ?? '').trim()
    if (Number.isFinite(idx)) {
      iterations.push({ index: idx, body })
    }
  }
  return { runHeader, iterations }
}
