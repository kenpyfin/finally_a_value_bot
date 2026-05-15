import { Flex, ScrollArea, Text } from '@radix-ui/themes'
import React, { useMemo } from 'react'

/** Matches `initial_llm_request_v1` from the gateway snapshot. */
export type InitialLlmRequestV1 = {
  schema?: string
  system_prompt?: string
  tool_names_first_turn?: string[]
  messages?: Array<{ role?: string; content?: unknown }>
}

function formatMessageContent(content: unknown): string {
  if (content === null || content === undefined) return ''
  if (typeof content === 'string') return content
  try {
    return JSON.stringify(content, null, 2)
  } catch {
    return String(content)
  }
}

type Props = {
  jsonText: string
  appearance: 'dark' | 'light'
}

/**
 * Renders the first-turn LLM snapshot JSON as readable sections (system, tools, messages).
 */
export function InitialRunPromptView({ jsonText, appearance }: Props) {
  const isDark = appearance === 'dark'

  const parsed = useMemo((): InitialLlmRequestV1 | null => {
    try {
      const v = JSON.parse(jsonText) as unknown
      return v && typeof v === 'object' ? (v as InitialLlmRequestV1) : null
    } catch {
      return null
    }
  }, [jsonText])

  const panel =
    isDark
      ? 'rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-main)]/80'
      : 'rounded-md border border-slate-200 bg-white'

  const preBody =
    isDark
      ? 'whitespace-pre-wrap break-words p-3 font-mono text-[12px] leading-relaxed text-slate-100'
      : 'whitespace-pre-wrap break-words p-3 font-mono text-[12px] leading-relaxed text-slate-800'

  const roleBadge = (role: string) => {
    const r = role.toLowerCase()
    if (r === 'assistant') {
      return isDark
        ? 'rounded bg-emerald-950/80 px-2 py-0.5 text-[11px] font-medium text-emerald-200'
        : 'rounded bg-emerald-100 px-2 py-0.5 text-[11px] font-medium text-emerald-900'
    }
    if (r === 'user') {
      return isDark
        ? 'rounded bg-sky-950/80 px-2 py-0.5 text-[11px] font-medium text-sky-200'
        : 'rounded bg-sky-100 px-2 py-0.5 text-[11px] font-medium text-sky-900'
    }
    return isDark
      ? 'rounded bg-slate-800 px-2 py-0.5 text-[11px] font-medium text-slate-300'
      : 'rounded bg-slate-200 px-2 py-0.5 text-[11px] font-medium text-slate-700'
  }

  if (!parsed || typeof parsed !== 'object') {
    return (
      <div className={panel}>
        <pre className={`max-h-[min(72vh,560px)] overflow-auto ${preBody}`}>{jsonText}</pre>
      </div>
    )
  }

  const tools = Array.isArray(parsed.tool_names_first_turn) ? parsed.tool_names_first_turn : []
  const messages = Array.isArray(parsed.messages) ? parsed.messages : []

  return (
    <ScrollArea type="auto" scrollbars="vertical" className="max-h-[min(72vh,560px)] w-full">
      <Flex direction="column" gap="4" className="pr-3 pb-1">
        {parsed.schema ? (
          <Text size="1" color="gray">
            Schema: <code className="text-xs">{parsed.schema}</code>
          </Text>
        ) : null}

        {typeof parsed.system_prompt === 'string' ? (
          <section>
            <Text size="4" weight="bold" className="mb-2 block tracking-tight">
              System prompt
            </Text>
            <div className={panel}>
              <pre className={preBody}>{parsed.system_prompt}</pre>
            </div>
          </section>
        ) : null}

        <section>
          <Text size="4" weight="bold" className="mb-2 block tracking-tight">
            Tools (first turn)
          </Text>
          {tools.length > 0 ? (
            <Flex wrap="wrap" gap="2" className={`${panel} p-3`}>
              {tools.map((name) => (
                <code
                  key={name}
                  className={
                    isDark
                      ? 'rounded border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] px-2 py-1 text-[11px] text-slate-200'
                      : 'rounded border border-slate-200 bg-slate-50 px-2 py-1 text-[11px] text-slate-800'
                  }
                >
                  {name}
                </code>
              ))}
            </Flex>
          ) : (
            <Text size="2" color="gray" className={`block ${panel} p-3`}>
              None — conversational turn (no tools on first call).
            </Text>
          )}
        </section>

        <section>
          <Text size="4" weight="bold" className="mb-2 block tracking-tight">
            Messages
          </Text>
          <Flex direction="column" gap="3">
            {messages.length === 0 ? (
              <Text size="2" color="gray" className={`${panel} p-3`}>
                No messages in snapshot.
              </Text>
            ) : (
              messages.map((m, idx) => {
                const role = typeof m.role === 'string' && m.role.trim() ? m.role.trim() : 'unknown'
                const body = formatMessageContent(m.content)
                return (
                  <Flex
                    key={`${idx}-${role}`}
                    direction="column"
                    gap="2"
                    className={
                      idx > 0
                        ? isDark
                          ? 'border-t border-[color:var(--mc-border-soft)] pt-4'
                          : 'border-t border-slate-200 pt-4'
                        : undefined
                    }
                  >
                    <Flex align="center" gap="2" wrap="wrap">
                      <span className={roleBadge(role)}>{role}</span>
                      <Text size="1" color="gray">
                        Message {idx + 1}
                      </Text>
                    </Flex>
                    <div className={panel}>
                      <pre className={preBody}>{body || '_(empty)_'}</pre>
                    </div>
                  </Flex>
                )
              })
            )}
          </Flex>
        </section>
      </Flex>
    </ScrollArea>
  )
}
