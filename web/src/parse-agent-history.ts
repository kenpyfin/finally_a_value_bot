export type ParsedAgentHistory = {
  runHeader: string
  iterations: { index: number; body: string }[]
}

/**
 * Splits persisted agent run markdown (`AgentRunRecord::to_markdown`) into a run header
 * and per-iteration sections (`## Iteration N`).
 */
export function parseAgentHistoryMarkdown(content: string): ParsedAgentHistory {
  const re = /^## Iteration (\d+)\s*$/gm
  const parts = content.split(re)
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
