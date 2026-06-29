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
      opts?: { force?: boolean; limitOverride?: number; sessionId?: string | null },
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
      if (day) query.set('day', day)
      else {
        const visibleLimit = opts?.limitOverride ?? historyVisibleLimitRef.current
        query.set('limit', String(visibleLimit + 1))
      }
      const data = await api<{ messages?: BackendMessage[] }>(`/api/history?${query.toString()}`)
      const rawMessages = Array.isArray(data.messages) ? data.messages : []
      const mapped = mapBackendHistory(rawMessages)
      if (day) {
        setHistoryByDay((prev) => {
          const nextByDay = { ...prev, [day]: mapped }
          const allDays = Object.keys(nextByDay).sort()
          const combined = allDays.flatMap((d) => (nextByDay[d] ?? []))
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
    resetHistoryPagination,
    handleReplyToMessage,
    handleDismissPendingReply,
    handleDeleteMessage,
    activeDraftText,
    activePendingReply,
    handleDraftTextChange,
    pendingReplyRef,
    formatReplyForSend,
    setDraftByThreadKey,
    setPendingReplyByThreadKey,
  }
}
