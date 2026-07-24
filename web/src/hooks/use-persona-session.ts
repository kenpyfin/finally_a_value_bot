import { useCallback, useMemo, useRef, useState } from 'react'
import { api } from '../api/client'
import { HISTORY_PAGE_SIZE } from '../app/constants'
import {
  baselinePersonaLastReadIfMissing,
  readPersonaLastReadAt,
  readStoredPersonaId,
  resolveStoredSessionId,
  toMs,
  writePersonaLastReadAt,
  writeStoredPersonaId,
  writeStoredSessionForPersona,
} from '../lib/persona-storage'
import type { ChatSession, Persona } from '../types'
import type { PendingConfirm } from '../components/confirm-dialog'

export type UsePersonaSessionOptions = {
  chatId: number | null
  setHistoryLoading: (loading: boolean) => void
  loadHistory: (
    cid?: number | null,
    personaId?: number | null,
    day?: string | null,
    opts?: { force?: boolean; limitOverride?: number; sessionId?: string | null },
  ) => Promise<void>
  resetHistoryPagination: () => void
  loadPersonaBulletin: (pid: number) => Promise<void>
  requestConfirm: (opts: PendingConfirm) => void
  setError: (message: string) => void
  setStatusText: (message: string) => void
  onPersonaLoaded?: (personaId: number | null) => void
}

export function usePersonaSession({
  chatId,
  setHistoryLoading,
  loadHistory,
  resetHistoryPagination,
  loadPersonaBulletin,
  requestConfirm,
  setError,
  setStatusText,
  onPersonaLoaded,
}: UsePersonaSessionOptions) {
  const [personas, setPersonas] = useState<Persona[]>([])
  const [activePersonaId, setActivePersonaId] = useState<number | null>(null)
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null)
  const [chatSessions, setChatSessions] = useState<ChatSession[]>([])
  const [personaReadNonce, setPersonaReadNonce] = useState(0)
  const [newSchedulePersonaId, setNewSchedulePersonaId] = useState<number | null>(null)

  const activeSessionIdRef = useRef<string | null>(null)
  activeSessionIdRef.current = activeSessionId

  const activePersonaName = personas.find((p) => p.id === activePersonaId)?.name ?? null

  const personaHasNew = useMemo<Record<number, boolean>>(() => {
    if (chatId == null) return {}
    const out: Record<number, boolean> = {}
    for (const p of personas) {
      if (p.id === activePersonaId) {
        out[p.id] = false
        continue
      }
      const lastBotMs = toMs(p.last_bot_message_at ?? null)
      if (lastBotMs == null) {
        out[p.id] = false
        continue
      }
      const lastReadMs = toMs(readPersonaLastReadAt(chatId, p.id))
      out[p.id] = lastReadMs == null ? true : lastBotMs > lastReadMs
    }
    return out
  }, [chatId, personas, activePersonaId, personaReadNonce])

  const markPersonaRead = useCallback(
    (personaId: number, chatIdOverride?: number | null) => {
      const cid = chatIdOverride ?? chatId
      if (cid == null) return
      writePersonaLastReadAt(cid, personaId, new Date().toISOString())
      setPersonaReadNonce((x) => x + 1)
    },
    [chatId],
  )

  const loadPersonas = useCallback(
    async (cid: number | null = chatId): Promise<{ id: number; name: string } | null> => {
      if (cid == null) return null
      try {
        const query = new URLSearchParams({ chat_id: String(cid) })
        const data = await api<{
          personas?: {
            id: number
            name: string
            is_active: boolean
            last_bot_message_at?: string | null
            last_bot_message_session_id?: string | null
            last_bot_message_session_title?: string | null
          }[]
        }>(`/api/personas?${query.toString()}`)
        const list = Array.isArray(data.personas) ? data.personas : []
        const personaList = list.map((p) => ({
          id: p.id,
          name: p.name,
          is_active: p.is_active,
          last_bot_message_at: p.last_bot_message_at ?? null,
          last_bot_message_session_id: p.last_bot_message_session_id ?? null,
          last_bot_message_session_title: p.last_bot_message_session_title ?? null,
        }))
        if (baselinePersonaLastReadIfMissing(cid, personaList)) {
          setPersonaReadNonce((x) => x + 1)
        }
        setPersonas(personaList)
        const active = list.find((p) => p.is_active)
        const defaultChoice = active ?? list[0]
        const storedId = readStoredPersonaId()
        const storedInList = storedId !== null && list.some((p) => p.id === storedId)
        const chosen = storedInList && list.find((p) => p.id === storedId)
          ? { id: list.find((p) => p.id === storedId)!.id, name: list.find((p) => p.id === storedId)!.name }
          : defaultChoice
            ? { id: defaultChoice.id, name: defaultChoice.name }
            : null
        if (chosen) {
          setActivePersonaId(chosen.id)
          setNewSchedulePersonaId((prev) => prev ?? chosen.id)
          if (!storedInList) writeStoredPersonaId(chosen.id)
        } else {
          setActivePersonaId(null)
        }
        onPersonaLoaded?.(chosen?.id ?? null)
        return chosen
      } catch {
        setPersonas([])
        setActivePersonaId(null)
        return null
      }
    },
    [chatId, onPersonaLoaded],
  )

  const loadSessions = useCallback(
    async (cid?: number | null, personaId?: number | null): Promise<ChatSession[]> => {
      const c = cid ?? chatId
      const p = personaId ?? activePersonaId
      if (c == null || p == null) {
        setChatSessions([])
        return []
      }
      try {
        const data = await api<{ sessions?: ChatSession[] }>(
          `/api/chat_sessions?chat_id=${c}&persona_id=${p}&include_archived=true`,
        )
        const sessions = Array.isArray(data.sessions) ? data.sessions : []
        setChatSessions(sessions)
        return sessions
      } catch {
        setChatSessions([])
        return []
      }
    },
    [activePersonaId, chatId],
  )

  const switchPersona = useCallback(
    async (
      personaName: string,
      opts?: { sessionId?: string | null },
    ): Promise<void> => {
      if (chatId == null) return
      setHistoryLoading(true)
      try {
        await api('/api/personas/switch', {
          method: 'POST',
          body: JSON.stringify({ chat_id: chatId, persona_name: personaName }),
        })
        const p = personas.find((x) => x.name === personaName)
        if (p) writeStoredPersonaId(p.id)
        await loadPersonas(chatId)
        const sessions = await loadSessions(chatId, p?.id)
        let targetSessionId: string | null
        if (opts !== undefined && 'sessionId' in opts) {
          const want = opts.sessionId ?? null
          if (want == null) {
            targetSessionId = null
          } else {
            const match = sessions.find((s) => s.id === want && s.status === 'active')
            targetSessionId = match ? match.id : null
          }
        } else {
          targetSessionId = p ? resolveStoredSessionId(sessions, p.id) : null
        }
        setActiveSessionId(targetSessionId)
        if (p) writeStoredSessionForPersona(p.id, targetSessionId)
        resetHistoryPagination()
        await loadHistory(chatId, p?.id ?? undefined, null, {
          force: true,
          limitOverride: HISTORY_PAGE_SIZE,
          sessionId: targetSessionId,
        })
        if (p) markPersonaRead(p.id)
        if (p) await loadPersonaBulletin(p.id)
      } finally {
        setHistoryLoading(false)
      }
    },
    [
      chatId,
      loadHistory,
      loadPersonaBulletin,
      loadPersonas,
      loadSessions,
      markPersonaRead,
      personas,
      resetHistoryPagination,
      setHistoryLoading,
    ],
  )

  const handleSelectSession = useCallback(
    async (sessionId: string | null) => {
      setHistoryLoading(true)
      try {
        setActiveSessionId(sessionId)
        if (activePersonaId != null) {
          writeStoredSessionForPersona(activePersonaId, sessionId)
        }
        resetHistoryPagination()
        await loadHistory(chatId, activePersonaId ?? undefined, null, {
          force: true,
          limitOverride: HISTORY_PAGE_SIZE,
          sessionId,
        })
      } finally {
        setHistoryLoading(false)
      }
    },
    [activePersonaId, chatId, loadHistory, resetHistoryPagination, setHistoryLoading],
  )

  const handleCreateSession = useCallback(
    async (intent: string, mirrorMainChat = false) => {
      if (chatId == null || activePersonaId == null) return
      setHistoryLoading(true)
      try {
        const data = await api<{
          session?: ChatSession
          session_id?: string
          title?: string
        }>('/api/chat_sessions', {
          method: 'POST',
          body: JSON.stringify({
            chat_id: chatId,
            persona_id: activePersonaId,
            intent,
            mirror_main_chat: mirrorMainChat,
          }),
        })
        const session =
          data.session ??
          (data.session_id
            ? {
                id: data.session_id,
                chat_id: chatId,
                persona_id: activePersonaId,
                title: data.title ?? intent.slice(0, 60),
                intent,
                status: 'active' as const,
                created_at: new Date().toISOString(),
                last_active_at: new Date().toISOString(),
                ttl_hours: 72,
                mirror_main_chat: mirrorMainChat,
              }
            : null)
        if (!session) return

        setChatSessions((prev) => {
          if (prev.some((s) => s.id === session.id)) return prev
          return [session, ...prev]
        })
        setActiveSessionId(session.id)
        writeStoredSessionForPersona(activePersonaId, session.id)
        resetHistoryPagination()
        await loadHistory(chatId, activePersonaId, null, {
          force: true,
          limitOverride: HISTORY_PAGE_SIZE,
          sessionId: session.id,
        })
        void loadSessions()
      } finally {
        setHistoryLoading(false)
      }
    },
    [activePersonaId, chatId, loadHistory, loadSessions, resetHistoryPagination, setHistoryLoading],
  )

  const handleArchiveSession = useCallback(
    async (sessionId: string) => {
      await api(`/api/chat_sessions/${encodeURIComponent(sessionId)}`, {
        method: 'PATCH',
        body: JSON.stringify({ status: 'archived' }),
      })
      if (activeSessionId === sessionId) {
        setHistoryLoading(true)
        try {
          setActiveSessionId(null)
          if (activePersonaId != null) {
            writeStoredSessionForPersona(activePersonaId, null)
          }
          resetHistoryPagination()
          await loadHistory(chatId, activePersonaId ?? undefined, null, {
            force: true,
            limitOverride: HISTORY_PAGE_SIZE,
            sessionId: null,
          })
        } finally {
          setHistoryLoading(false)
        }
      }
      await loadSessions()
    },
    [
      activePersonaId,
      activeSessionId,
      chatId,
      loadHistory,
      loadSessions,
      resetHistoryPagination,
      setHistoryLoading,
    ],
  )

  const handleReopenSession = useCallback(
    async (sessionId: string) => {
      setHistoryLoading(true)
      try {
        await api(`/api/chat_sessions/${encodeURIComponent(sessionId)}`, {
          method: 'PATCH',
          body: JSON.stringify({ status: 'active' }),
        })
        setActiveSessionId(sessionId)
        if (activePersonaId != null) {
          writeStoredSessionForPersona(activePersonaId, sessionId)
        }
        await loadSessions()
        resetHistoryPagination()
        await loadHistory(chatId, activePersonaId ?? undefined, null, {
          force: true,
          limitOverride: HISTORY_PAGE_SIZE,
          sessionId,
        })
      } finally {
        setHistoryLoading(false)
      }
    },
    [activePersonaId, chatId, loadHistory, loadSessions, resetHistoryPagination, setHistoryLoading],
  )

  const handleDeleteSession = useCallback(
    async (sessionId: string) => {
      await api(`/api/chat_sessions/${encodeURIComponent(sessionId)}`, { method: 'DELETE' })
      if (activeSessionId === sessionId) {
        setHistoryLoading(true)
        try {
          setActiveSessionId(null)
          if (activePersonaId != null) {
            writeStoredSessionForPersona(activePersonaId, null)
          }
          resetHistoryPagination()
          await loadHistory(chatId, activePersonaId ?? undefined, null, {
            force: true,
            limitOverride: HISTORY_PAGE_SIZE,
            sessionId: null,
          })
        } finally {
          setHistoryLoading(false)
        }
      }
      await loadSessions()
    },
    [
      activePersonaId,
      activeSessionId,
      chatId,
      loadHistory,
      loadSessions,
      resetHistoryPagination,
      setHistoryLoading,
    ],
  )

  const onCreatePersona = useCallback(async () => {
    if (chatId == null) return
    const name = window.prompt('New persona name?')
    if (!name?.trim()) return
    try {
      await api('/api/personas/create', {
        method: 'POST',
        body: JSON.stringify({ chat_id: chatId, name: name.trim() }),
      })
      await loadPersonas(chatId)
      setStatusText(`Persona "${name.trim()}" created`)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [chatId, loadPersonas, setError, setStatusText])

  const onDeletePersona = useCallback(
    (personaId: number) => {
      if (chatId == null) return
      const personaName = personas.find((p) => p.id === personaId)?.name ?? 'this persona'
      requestConfirm({
        title: 'Delete persona',
        description: `Delete "${personaName}"? Its messages and session will be removed permanently.`,
        confirmLabel: 'Delete persona',
        destructive: true,
        onConfirm: async () => {
          try {
            await api('/api/personas/delete', {
              method: 'POST',
              body: JSON.stringify({ chat_id: chatId, persona_id: personaId }),
            })
            await loadPersonas(chatId)
            if (activePersonaId === personaId) {
              resetHistoryPagination()
              if (activePersonaId != null && activePersonaId > 0) {
                await loadHistory(chatId, activePersonaId, null, {
                  force: true,
                  limitOverride: HISTORY_PAGE_SIZE,
                })
              }
            }
            setStatusText('Persona deleted')
            setError('')
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            throw e
          }
        },
      })
    },
    [
      activePersonaId,
      chatId,
      loadHistory,
      loadPersonas,
      personas,
      requestConfirm,
      resetHistoryPagination,
      setError,
      setStatusText,
    ],
  )

  return {
    personas,
    setPersonas,
    activePersonaId,
    setActivePersonaId,
    activeSessionId,
    setActiveSessionId,
    activeSessionIdRef,
    chatSessions,
    activePersonaName,
    personaHasNew,
    markPersonaRead,
    loadPersonas,
    loadSessions,
    switchPersona,
    handleSelectSession,
    handleCreateSession,
    handleArchiveSession,
    handleReopenSession,
    handleDeleteSession,
    onCreatePersona,
    onDeletePersona,
    newSchedulePersonaId,
    setNewSchedulePersonaId,
  }
}
