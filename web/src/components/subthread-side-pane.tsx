import React, { useCallback, useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { api, makeHeaders } from '../api/client'
import type { BackendMessage, PersonaBulletinHistorySuffix } from '../types'
import { MarkdownTable } from './markdown-table'
import { makeReplySnippet } from '../lib/reply-quote'

type SubthreadTurn = {
  id: string
  role: 'user' | 'assistant'
  content: string
  streaming?: boolean
}

export type SubthreadSidePaneProps = {
  chatId: number | null
  personaId: number | null
  sessionId: string | null
  anchorMessageId: string
  anchorMessage: BackendMessage
  historySuffix: PersonaBulletinHistorySuffix | null
  onClose: () => void
}

type SseEvent = { event: string; data: string; id?: string }

async function* parseSseEvents(resp: Response): AsyncGenerator<SseEvent, void, unknown> {
  const body = resp.body
  if (!body) return

  const reader = body.getReader()
  const decoder = new TextDecoder('utf-8')

  let buffer = ''
  let eventName: string | undefined
  let eventId: string | undefined
  let dataLines: string[] = []

  while (true) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })

    while (true) {
      const newlineIdx = buffer.indexOf('\n')
      if (newlineIdx < 0) break

      const line = buffer.slice(0, newlineIdx).replace(/\r$/, '')
      buffer = buffer.slice(newlineIdx + 1)

      if (line === '') {
        if (dataLines.length > 0) {
          yield {
            event: eventName ?? 'message',
            data: dataLines.join('\n'),
            id: eventId,
          }
        }
        eventName = undefined
        eventId = undefined
        dataLines = []
        continue
      }

      if (line.startsWith(':')) continue
      if (line.startsWith('event:')) {
        eventName = line.slice('event:'.length).trim()
        continue
      }
      if (line.startsWith('id:')) {
        eventId = line.slice('id:'.length).trim()
        continue
      }
      if (line.startsWith('data:')) {
        dataLines.push(line.slice('data:'.length).trimStart())
      }
    }
  }

  if (dataLines.length > 0) {
    yield { event: eventName ?? 'message', data: dataLines.join('\n'), id: eventId }
  }
}

function parseJsonObject(raw: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>
    if (parsed && typeof parsed === 'object') return parsed
    return null
  } catch {
    return null
  }
}

export function SubthreadSidePane({
  chatId,
  personaId,
  sessionId,
  anchorMessageId,
  anchorMessage,
  historySuffix,
  onClose,
}: SubthreadSidePaneProps) {
  const [turns, setTurns] = useState<SubthreadTurn[]>([])
  const [draft, setDraft] = useState('')
  const [status, setStatus] = useState('Ready')
  const [error, setError] = useState('')
  const [sending, setSending] = useState(false)
  const [contextWindow, setContextWindow] = useState<{
    min_user: number
    min_assistant: number
  } | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  const listRef = useRef<HTMLDivElement | null>(null)
  const inputRef = useRef<HTMLTextAreaElement | null>(null)

  const anchorSnippet = makeReplySnippet(
    typeof anchorMessage.content === 'string' ? anchorMessage.content : '',
  )

  useEffect(() => {
    inputRef.current?.focus()
  }, [anchorMessageId])

  useEffect(() => {
    const el = listRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [turns, sending])

  useEffect(() => {
    return () => {
      abortRef.current?.abort()
    }
  }, [])

  const windowLabel = (() => {
    if (contextWindow) {
      return `${contextWindow.min_user} user / ${contextWindow.min_assistant} assistant`
    }
    if (historySuffix) {
      return `${historySuffix.min_user.effective} user / ${historySuffix.min_assistant.effective} assistant`
    }
    return 'cockpit window'
  })()

  const send = useCallback(async () => {
    const text = draft.trim()
    if (!text || sending || chatId == null || personaId == null) return

    const userTurn: SubthreadTurn = {
      id: `u-${Date.now()}`,
      role: 'user',
      content: text,
    }
    const assistantId = `a-${Date.now()}`
    const historyPayload = turns.map((t) => ({ role: t.role, content: t.content }))

    setDraft('')
    setError('')
    setSending(true)
    setStatus('Sending…')
    setTurns((prev) => [
      ...prev,
      userTurn,
      { id: assistantId, role: 'assistant', content: '', streaming: true },
    ])

    const abort = new AbortController()
    abortRef.current = abort

    try {
      const sendResponse = await api<{
        run_id?: string
        context_window?: { min_user?: number; min_assistant?: number }
      }>('/api/subthread_stream', {
        method: 'POST',
        body: JSON.stringify({
          chat_id: chatId,
          persona_id: personaId,
          session_id: sessionId,
          anchor_message_id: anchorMessageId,
          message: text,
          history: historyPayload,
        }),
        signal: abort.signal,
      })

      const runId = sendResponse.run_id
      if (!runId) throw new Error('missing run_id')

      if (
        sendResponse.context_window &&
        typeof sendResponse.context_window.min_user === 'number' &&
        typeof sendResponse.context_window.min_assistant === 'number'
      ) {
        setContextWindow({
          min_user: sendResponse.context_window.min_user,
          min_assistant: sendResponse.context_window.min_assistant,
        })
      }

      setStatus('Queued')
      const sseResp = await fetch(`/api/stream?run_id=${encodeURIComponent(runId)}`, {
        headers: makeHeaders(),
        signal: abort.signal,
      })
      if (!sseResp.ok) {
        throw new Error(`stream subscribe failed (HTTP ${sseResp.status})`)
      }

      let completed = false
      for await (const evt of parseSseEvents(sseResp)) {
        if (abort.signal.aborted) break
        if (evt.event === 'status') {
          const obj = parseJsonObject(evt.data)
          const message = typeof obj?.message === 'string' ? obj.message : null
          if (message) setStatus(message)
          continue
        }
        if (evt.event === 'delta') {
          const obj = parseJsonObject(evt.data)
          const delta = typeof obj?.delta === 'string' ? obj.delta : ''
          if (!delta) continue
          setTurns((prev) =>
            prev.map((t) =>
              t.id === assistantId ? { ...t, content: `${t.content}${delta}`, streaming: true } : t,
            ),
          )
          continue
        }
        if (evt.event === 'done') {
          const obj = parseJsonObject(evt.data)
          const response = typeof obj?.response === 'string' ? obj.response : ''
          setTurns((prev) =>
            prev.map((t) =>
              t.id === assistantId
                ? {
                    ...t,
                    content: response || t.content,
                    streaming: false,
                  }
                : t,
            ),
          )
          completed = true
          setStatus('Idle')
          break
        }
        if (evt.event === 'error') {
          const obj = parseJsonObject(evt.data)
          const err = typeof obj?.error === 'string' ? obj.error : 'Side chat failed'
          throw new Error(err)
        }
      }

      if (!completed && !abort.signal.aborted) {
        setTurns((prev) =>
          prev.map((t) => (t.id === assistantId ? { ...t, streaming: false } : t)),
        )
        setStatus('Idle')
      }
    } catch (e) {
      if (abort.signal.aborted) {
        setStatus('Cancelled')
        setTurns((prev) =>
          prev.map((t) =>
            t.id === assistantId
              ? { ...t, content: t.content || '(cancelled)', streaming: false }
              : t,
          ),
        )
      } else {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        setStatus('Error')
        setTurns((prev) =>
          prev.map((t) =>
            t.id === assistantId
              ? { ...t, content: t.content || `Error: ${msg}`, streaming: false }
              : t,
          ),
        )
      }
    } finally {
      setSending(false)
      abortRef.current = null
    }
  }, [
    anchorMessageId,
    chatId,
    draft,
    personaId,
    sending,
    sessionId,
    turns,
  ])

  const cancel = useCallback(() => {
    abortRef.current?.abort()
  }, [])

  return (
    <aside className="mc-subthread-pane" aria-label="Side chat">
      <header className="mc-subthread-header">
        <div className="mc-subthread-header-text">
          <div className="mc-subthread-title">Side chat</div>
          <div className="mc-subthread-meta">Ephemeral · context {windowLabel}</div>
        </div>
        <button
          type="button"
          className="mc-subthread-close"
          onClick={onClose}
          aria-label="Close side chat"
          title="Close"
        >
          ×
        </button>
      </header>

      <div className="mc-subthread-anchor" role="note">
        <div className="mc-subthread-anchor-label">Anchored to assistant reply</div>
        <div className="mc-subthread-anchor-snippet">{anchorSnippet}</div>
      </div>

      <div className="mc-subthread-list" ref={listRef}>
        {turns.length === 0 ? (
          <div className="mc-subthread-empty">
            Ask a follow-up about this reply. Side-chat turns stay out of the main timeline.
          </div>
        ) : (
          turns.map((turn) => (
            <div
              key={turn.id}
              className={
                turn.role === 'user' ? 'mc-subthread-bubble mc-subthread-bubble-user' : 'mc-subthread-bubble'
              }
            >
              <div className="mc-subthread-bubble-role">
                {turn.role === 'user' ? 'You' : 'Assistant'}
                {turn.streaming ? ' · streaming' : ''}
              </div>
              {turn.role === 'assistant' ? (
                <div className="mc-subthread-markdown">
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={{
                      table: MarkdownTable,
                      a: (props) => {
                        const mergedRel = [props.rel, 'noopener', 'noreferrer']
                          .filter(Boolean)
                          .join(' ')
                        return <a {...props} target="_blank" rel={mergedRel} />
                      },
                    }}
                  >
                    {turn.content || (turn.streaming ? '…' : '')}
                  </ReactMarkdown>
                </div>
              ) : (
                <div className="mc-subthread-plain">{turn.content}</div>
              )}
            </div>
          ))
        )}
      </div>

      {error ? <div className="mc-subthread-error">{error}</div> : null}

      <div className="mc-subthread-composer">
        <textarea
          ref={inputRef}
          className="mc-subthread-input"
          value={draft}
          rows={3}
          placeholder="Ask about this reply…"
          disabled={sending}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              void send()
            }
          }}
        />
        <div className="mc-subthread-composer-actions">
          <span className="mc-subthread-status">{status}</span>
          {sending ? (
            <button type="button" className="mc-subthread-send" onClick={cancel}>
              Stop
            </button>
          ) : (
            <button
              type="button"
              className="mc-subthread-send mc-subthread-send-primary"
              onClick={() => void send()}
              disabled={!draft.trim() || chatId == null || personaId == null}
            >
              Send
            </button>
          )}
        </div>
      </div>
    </aside>
  )
}
