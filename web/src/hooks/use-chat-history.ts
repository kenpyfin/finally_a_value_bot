import { useCallback, useEffect, useRef, useState } from 'react'
import type { ThreadMessageLike } from '@assistant-ui/react'
import { api } from '../api/client'
import { historiesEqual, mapBackendHistory } from '../lib/history-sync'
import { formatReplyForSend, makeReplySnippet, type PendingReplyQuote } from '../lib/reply-quote'
import { HISTORY_PAGE_SIZE } from '../app/constants'
import type { BackendMessage, PersonaMessageBookmark } from '../types'
import type { PendingConfirm } from '../components/confirm-dialog'

export type UseChatHistoryOptions = {
  chatId: number | null
  activePersonaId: number | null
  activeSessionId: string | null
  setError: (message: string) => void
  setStatusText: (message: string) => void
  requestConfirm: (opts: PendingConfirm) => void
  setPersonaBookmarks: React.Dispatch<React.SetStateAction<PersonaMessageBookmark[]>>
}

export function useChatHistory({
  chatId,
  activePersonaId,
  activeSessionId,
  setError,
  setStatusText,
  requestConfirm,
  setPersonaBookmarks,
}: UseChatHistoryOptions) {
  const [historySeed, setHistorySeed] = useState<ThreadMessageLike[]>([])
  const [historyByDay, setHistoryByDay] = useState<Record<string, ThreadMessageLike[]>>({})
  const [historyVisibleLimit, setHistoryVisibleLimit] = useState<number>(HISTORY_PAGE_SIZE)
  const [historyHasMore, setHistoryHasMore] = useState<boolean>(false)
  const [historyLoadingMore, setHistoryLoadingMore] = useState<boolean>(false)
  const [historyLoading, setHistoryLoading] = useState<boolean>(true)
  const [draftByThreadKey, setDraftByThreadKey] = useState<Record<string, string>>({})
  const [pendingReplyByThreadKey, setPendingReplyByThreadKey] = useState<
    Record<string, PendingReplyQuote>
  >({})

  const activeSessionIdRef = useRef<string | null>(null)
  activeSessionIdRef.current = activeSessionId
  const historySeedRef = useRef<ThreadMessageLike[]>([])
  const historyVisibleLimitRef = useRef<number>(historyVisibleLimit)
  const pendingReplyRef = useRef<PendingReplyQuote | null>(null)

  useEffect(() => {
    historySeedRef.current = historySeed
  }, [historySeed])
  useEffect(() => {
    historyVisibleLimitRef.current = historyVisibleLimit
  }, [historyVisibleLimit])
  useEffect(() => {
    const threadKey = `${chatId ?? 0}:${activePersonaId ?? 0}`
    pendingReplyRef.current = pendingReplyByThreadKey[threadKey] ?? null
  }, [chatId, activePersonaId, pendingReplyByThreadKey])

  const resetHistoryPagination = useCallback(() => {
    setHistoryVisibleLimit(HISTORY_PAGE_SIZE)
    setHistoryHasMore(false)
  }, [])

  const loadHistory = useCallback(
    async (
      cid: number | null = chatId,
      personaId?: number | null,
      day?: string | null,
      opts?: {
        force?: boolean
        limitOverride?: number
        sessionId?: string | null
        aroundId?: string
        aroundBefore?: number
        aroundAfter?: number
      },
    ): Promise<void> => {
      if (cid == null) return
      const pid =
        personaId != null && personaId > 0
          ? personaId
          : activePersonaId != null && activePersonaId > 0
            ? activePersonaId
            : null
      if (pid == null) {
        setHistorySeed([])
        setHistoryByDay({})
        setHistoryHasMore(false)
        return
      }
      const query = new URLSearchParams({ chat_id: String(cid), persona_id: String(pid) })
      const sid = opts?.sessionId !== undefined ? opts.sessionId : activeSessionIdRef.current
      if (sid) query.set('session_id', sid)
      if (opts?.aroundId) {
        query.set('around_id', opts.aroundId)
        query.set('around_before', String(opts.aroundBefore ?? 15))
        query.set('around_after', String(opts.aroundAfter ?? 15))
      } else if (day) {
        query.set('day', day)
      } else {
        const visibleLimit = opts?.limitOverride ?? historyVisibleLimitRef.current
        query.set('limit', String(visibleLimit + 1))
      }
      const data = await api<{ messages?: BackendMessage[] }>(`/api/history?${query.toString()}`)
      const rawMessages = Array.isArray(data.messages) ? data.messages : []
      const mapped = mapBackendHistory(rawMessages)
      if (opts?.aroundId) {
        setHistoryByDay({})
        setHistoryHasMore(true)
        setHistoryVisibleLimit(Math.max(historyVisibleLimitRef.current, mapped.length))
        if (!historiesEqual(historySeedRef.current, mapped)) {
          setHistorySeed(mapped)
        }
      } else if (day) {
        setHistoryByDay((prev) => {
          const nextByDay = { ...prev, [day]: mapped }
          const allDays = Object.keys(nextByDay).sort()
          const combined = allDays.flatMap((d) => nextByDay[d] ?? [])
          setHistoryHasMore(false)
          if (!historiesEqual(historySeedRef.current, combined)) {
            setHistorySeed(combined)
          }
          return nextByDay
        })
      } else {
        const visibleLimit = opts?.limitOverride ?? historyVisibleLimitRef.current
        const hasMore = mapped.length > visibleLimit
        const bounded = hasMore ? mapped.slice(mapped.length - visibleLimit) : mapped
        setHistoryByDay({})
        setHistoryHasMore(hasMore)
        if (!historiesEqual(historySeedRef.current, bounded)) {
          setHistorySeed(bounded)
        }
      }
    },
    [activePersonaId, chatId],
  )

  const loadMoreHistory = useCallback(async () => {
    if (chatId == null) return
    if (historyLoadingMore) return
    const previousLimit = historyVisibleLimitRef.current
    const nextLimit = previousLimit + HISTORY_PAGE_SIZE
    setHistoryLoadingMore(true)
    setHistoryVisibleLimit(nextLimit)
    try {
      await loadHistory(chatId, activePersonaId ?? undefined, null, {
        force: true,
        limitOverride: nextLimit,
      })
    } catch (e) {
      setHistoryVisibleLimit(previousLimit)
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setHistoryLoadingMore(false)
    }
  }, [activePersonaId, chatId, historyLoadingMore, loadHistory, setError])

  /** Ensure a message is in the loaded history window (raise limit or around_id). */
  const ensureMessageVisible = useCallback(
    async (
      messageId: string,
    ): Promise<{ ok: boolean; sessionId: string | null }> => {
      if (chatId == null || activePersonaId == null) {
        return { ok: false, sessionId: null }
      }
      try {
        const data = await api<{
          message?: BackendMessage
          messages_from?: number
        }>(`/api/personas/${activePersonaId}/messages/${encodeURIComponent(messageId)}`)
        const m = data.message
        if (!m?.id) return { ok: false, sessionId: null }
        const sessionId =
          typeof m.session_id === 'string' && m.session_id.trim()
            ? m.session_id.trim()
            : null
        const fromCount =
          typeof data.messages_from === 'number' && data.messages_from > 0
            ? data.messages_from
            : null
        const HISTORY_REVEAL_CAP = 300
        const pad = 10
        if (fromCount != null && fromCount + pad <= HISTORY_REVEAL_CAP) {
          const nextLimit = Math.max(historyVisibleLimitRef.current, fromCount + pad)
          setHistoryVisibleLimit(nextLimit)
          await loadHistory(chatId, activePersonaId, null, {
            force: true,
            limitOverride: nextLimit,
            sessionId,
          })
        } else {
          await loadHistory(chatId, activePersonaId, null, {
            force: true,
            sessionId,
            aroundId: messageId,
            aroundBefore: 20,
            aroundAfter: 20,
          })
        }
        return { ok: true, sessionId }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
        return { ok: false, sessionId: null }
      }
    },
    [activePersonaId, chatId, loadHistory, setError],
  )

  const handleReplyToMessage = useCallback(
    async (messageId: string) => {
      if (activePersonaId == null) return
      try {
        setStatusText('Loading quote…')
        const data = await api<{ message?: BackendMessage }>(
          `/api/personas/${activePersonaId}/messages/${encodeURIComponent(messageId)}`,
        )
        const m = data.message
        const raw = typeof m?.content === 'string' ? m.content.trim() : ''
        if (!raw) {
          setError('Cannot reply: message has no text content')
          setStatusText('Idle')
          return
        }
        const threadKey = `${chatId ?? 0}:${activePersonaId ?? 0}`
        const quote: PendingReplyQuote = {
          messageId,
          snippet: makeReplySnippet(raw),
          fullContent: raw,
          senderName: typeof m?.sender_name === 'string' ? m.sender_name : '',
          isFromBot: Boolean(m?.is_from_bot),
        }
        setPendingReplyByThreadKey((prev) => ({ ...prev, [threadKey]: quote }))
        pendingReplyRef.current = quote
        setStatusText('Quote ready — add your reply')
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
        setStatusText('Idle')
      }
    },
    [activePersonaId, chatId, setError, setStatusText],
  )

  const handleDismissPendingReply = useCallback(() => {
    const threadKey = `${chatId ?? 0}:${activePersonaId ?? 0}`
    setPendingReplyByThreadKey((prev) => {
      if (!prev[threadKey]) return prev
      const next = { ...prev }
      delete next[threadKey]
      return next
    })
    pendingReplyRef.current = null
  }, [activePersonaId, chatId])

  const handleDeleteMessage = useCallback(
    (messageId: string) => {
      if (activePersonaId == null) return
      requestConfirm({
        title: 'Delete message',
        description:
          'This permanently removes the message from the database. Bookmarks for this message are also removed.',
        confirmLabel: 'Delete message',
        destructive: true,
        onConfirm: async () => {
          try {
            await api(`/api/personas/${activePersonaId}/messages/${encodeURIComponent(messageId)}`, {
              method: 'DELETE',
            })
            setHistorySeed((prev) => prev.filter((m) => m.id !== messageId))
            setPersonaBookmarks((prev) => prev.filter((b) => b.message_id !== messageId))
            setStatusText('Message deleted')
            setError('')
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            throw e
          }
        },
      })
    },
    [activePersonaId, requestConfirm, setError, setPersonaBookmarks, setStatusText],
  )

  const handleSaveMessageEdit = useCallback(
    async (messageId: string, content: string) => {
      if (activePersonaId == null) return
      const trimmed = content.trim()
      if (!trimmed) {
        setError('Edited message cannot be empty')
        return
      }
      try {
        setStatusText('Saving edit…')
        await api(`/api/personas/${activePersonaId}/messages/${encodeURIComponent(messageId)}`, {
          method: 'PATCH',
          body: JSON.stringify({ content: trimmed }),
        })
        setHistorySeed((prev) =>
          prev.map((m) => {
            if (m.id !== messageId) return m
            return {
              ...m,
              content: [{ type: 'text' as const, text: trimmed }],
            }
          }),
        )
        setPersonaBookmarks((prev) =>
          prev.map((b) => {
            if (b.message_id !== messageId) return b
            const preview =
              Array.from(trimmed).length > 280
                ? `${Array.from(trimmed).slice(0, 280).join('')}…`
                : trimmed
            return { ...b, content_preview: preview }
          }),
        )
        setStatusText('Message updated')
        setError('')
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
        setStatusText('Idle')
        throw e
      }
    },
    [activePersonaId, setError, setPersonaBookmarks, setStatusText],
  )

  const activeDraftKey = `${chatId ?? 0}:${activePersonaId ?? 0}`
  const activeDraftText = draftByThreadKey[activeDraftKey] ?? ''
  const activePendingReply = pendingReplyByThreadKey[activeDraftKey] ?? null

  const handleDraftTextChange = useCallback(
    (nextText: string) => {
      setDraftByThreadKey((prev) => {
        const current = prev[activeDraftKey] ?? ''
        if (current === nextText) return prev
        return { ...prev, [activeDraftKey]: nextText }
      })
    },
    [activeDraftKey],
  )

  return {
    historySeed,
    setHistorySeed,
    historyByDay,
    historyHasMore,
    historyLoadingMore,
    historyLoading,
    setHistoryLoading,
    loadHistory,
    loadMoreHistory,
    ensureMessageVisible,
    resetHistoryPagination,
    handleReplyToMessage,
    handleDismissPendingReply,
    handleDeleteMessage,
    handleSaveMessageEdit,
    activeDraftText,
    activePendingReply,
    handleDraftTextChange,
    pendingReplyRef,
    formatReplyForSend,
    setDraftByThreadKey,
    setPendingReplyByThreadKey,
  }
}
