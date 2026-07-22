import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { ChatModelAdapter, ChatModelRunOptions, ChatModelRunResult } from '@assistant-ui/react'
import { Button, Callout, Flex, Theme } from '@radix-ui/themes'
import '@radix-ui/themes/styles.css'
import '@assistant-ui/react-ui/styles/index.css'
import '../styles.css'
import { api, makeHeaders } from '../api/client'
import { CockpitBar } from '../components/cockpit-bar'
import { SessionSidebar } from '../components/session-sidebar'
import { ThreadPane } from '../components/thread-pane'
import { useConfirmDialog } from '../components/confirm-dialog'
import { ErrorBanner } from '../components/error-banner'
import { StatusRegion } from '../components/status-region'
import { MobileOpsSheet, ShortcutsDialog } from '../components/ops-ui'
import { AppHeader } from './AppHeader'
import { AppDialogs } from './AppDialogs'
import { AuthDialog } from '../context/AuthContext'
import { useChatHistory } from '../hooks/use-chat-history'
import { useDocumentVisible } from '../hooks/use-document-visible'
import { useKeyboardShortcuts } from '../hooks/use-keyboard-shortcuts'
import { useOperatorOps } from '../hooks/use-operator-ops'
import { useOpsPoll } from '../hooks/use-ops-poll'
import { usePersonaSession } from '../hooks/use-persona-session'
import {
  dataUrlToFile,
  splitDataUrl,
  uploadAttachmentFile,
  type SendAttachmentRef,
} from '../lib/attachments'
import { HISTORY_PAGE_SIZE } from './constants'
import { resolveStoredSessionId } from '../lib/persona-storage'
import {
  parseAgentHistoryMarkdown,
  parsePdqeSteps,
  parsePteDecisions,
  splitAgentHistoryRaw,
  type ParsedAgentHistory,
} from '../parse-agent-history'
import {
  OPERATOR_MEMO_MAX_CHARS,
  type ArtifactItem,
  type AgentHistoryOptimizeRequest,
  type AgentHistoryOptimizeResponse,
  type BackgroundJobItem,
  type ChannelBinding,
  type InstallationStatus,
  type QueueItem,
  type ScheduleTask,
} from '../types'

type Appearance = 'dark' | 'light'

type PendingRun = { runId: string; personaId: number }
type UiTheme =
  | 'green'
  | 'blue'
  | 'slate'
  | 'amber'
  | 'violet'
  | 'rose'
  | 'cyan'
  | 'teal'
  | 'orange'
  | 'indigo'


const UI_THEME_OPTIONS: { key: UiTheme; label: string; color: string }[] = [
  { key: 'green', label: 'Green', color: '#34d399' },
  { key: 'blue', label: 'Blue', color: '#60a5fa' },
  { key: 'slate', label: 'Slate', color: '#94a3b8' },
  { key: 'amber', label: 'Amber', color: '#fbbf24' },
  { key: 'violet', label: 'Violet', color: '#a78bfa' },
  { key: 'rose', label: 'Rose', color: '#fb7185' },
  { key: 'cyan', label: 'Cyan', color: '#22d3ee' },
  { key: 'teal', label: 'Teal', color: '#2dd4bf' },
  { key: 'orange', label: 'Orange', color: '#fb923c' },
  { key: 'indigo', label: 'Indigo', color: '#818cf8' },
]

const RADIX_ACCENT_BY_THEME: Record<UiTheme, string> = {
  green: 'green',
  blue: 'blue',
  slate: 'gray',
  amber: 'amber',
  violet: 'violet',
  rose: 'ruby',
  cyan: 'cyan',
  teal: 'teal',
  orange: 'orange',
  indigo: 'indigo',
}

function readAppearance(): Appearance {
  const saved = localStorage.getItem('finally-a-value-bot_appearance')
  return saved === 'light' ? 'light' : 'dark'
}

function saveAppearance(value: Appearance): void {
  localStorage.setItem('finally-a-value-bot_appearance', value)
}

function readUiTheme(): UiTheme {
  const saved = localStorage.getItem('finally-a-value-bot_ui_theme') as UiTheme | null
  return UI_THEME_OPTIONS.some((t) => t.key === saved) ? (saved as UiTheme) : 'green'
}

function saveUiTheme(value: UiTheme): void {
  localStorage.setItem('finally-a-value-bot_ui_theme', value)
}

const DESKTOP_SIDEBAR_OPEN_KEY = 'finally-a-value-bot_desktop_sidebar_open'
const DESKTOP_SIDEBAR_WIDTH_KEY = 'finally-a-value-bot_desktop_sidebar_width'
const DESKTOP_SIDEBAR_DEFAULT_WIDTH = 320
const DESKTOP_SIDEBAR_MIN_WIDTH = 260
const DESKTOP_SIDEBAR_MAX_WIDTH = 520
const DESKTOP_MAIN_PANEL_MIN_WIDTH = 480

function readDesktopSidebarOpen(): boolean {
  if (typeof window === 'undefined') return true
  try {
    return localStorage.getItem(DESKTOP_SIDEBAR_OPEN_KEY) !== '0'
  } catch {
    return true
  }
}

function saveDesktopSidebarOpen(open: boolean): void {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(DESKTOP_SIDEBAR_OPEN_KEY, open ? '1' : '0')
  } catch {
    /* ignore */
  }
}

function desktopSidebarViewportMax(viewportWidth: number): number {
  return Math.min(
    DESKTOP_SIDEBAR_MAX_WIDTH,
    Math.max(DESKTOP_SIDEBAR_MIN_WIDTH, Math.round(viewportWidth - DESKTOP_MAIN_PANEL_MIN_WIDTH)),
  )
}

function clampDesktopSidebarWidth(value: number, viewportWidth?: number): number {
  const fallback = Number.isFinite(value) ? value : DESKTOP_SIDEBAR_DEFAULT_WIDTH
  const hardClamped = Math.min(
    DESKTOP_SIDEBAR_MAX_WIDTH,
    Math.max(DESKTOP_SIDEBAR_MIN_WIDTH, Math.round(fallback)),
  )
  if (typeof viewportWidth !== 'number') return hardClamped
  return Math.min(hardClamped, desktopSidebarViewportMax(viewportWidth))
}

function readDesktopSidebarWidth(): number {
  if (typeof window === 'undefined') return DESKTOP_SIDEBAR_DEFAULT_WIDTH
  try {
    const raw = localStorage.getItem(DESKTOP_SIDEBAR_WIDTH_KEY)
    const parsed = raw == null ? DESKTOP_SIDEBAR_DEFAULT_WIDTH : Number(raw)
    return clampDesktopSidebarWidth(parsed, window.innerWidth)
  } catch {
    return DESKTOP_SIDEBAR_DEFAULT_WIDTH
  }
}

function saveDesktopSidebarWidth(width: number): void {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(DESKTOP_SIDEBAR_WIDTH_KEY, String(clampDesktopSidebarWidth(width)))
  } catch {
    /* ignore */
  }
}

if (typeof document !== 'undefined') {
  document.documentElement.classList.toggle('dark', readAppearance() === 'dark')
  document.documentElement.setAttribute('data-ui-theme', readUiTheme())
}

type ExtractAttachmentOptions = {
  chatId: number | null
  signal?: AbortSignal
  onUploadProgress?: (message: string) => void
}

async function extractAttachmentFromUnknown(
  part: unknown,
  opts: ExtractAttachmentOptions,
): Promise<SendAttachmentRef | null> {
  if (!part || typeof part !== 'object') return null
  const obj = part as Record<string, unknown>

  const fileVal = obj.file
  if (fileVal instanceof File) {
    return uploadAttachmentFile(fileVal, opts.chatId, {
      signal: opts.signal,
      onProgress: opts.onUploadProgress,
    })
  }

  const candidateData =
    (typeof obj.data === 'string' ? obj.data : null) ||
    (typeof obj.url === 'string' && String(obj.url).startsWith('data:') ? String(obj.url) : null) ||
    (typeof obj.image === 'string' && String(obj.image).startsWith('data:') ? String(obj.image) : null) ||
    (typeof obj.source === 'string' && String(obj.source).startsWith('data:') ? String(obj.source) : null)

  if (!candidateData) return null

  const filename = typeof obj.filename === 'string' ? obj.filename : undefined
  const mediaType =
    (typeof obj.mediaType === 'string' ? obj.mediaType : undefined) ||
    (typeof obj.mimeType === 'string' ? obj.mimeType : undefined) ||
    (typeof obj.contentType === 'string' ? obj.contentType : undefined) ||
    splitDataUrl(candidateData)?.mimeType

  const file = dataUrlToFile(candidateData, filename)
  if (mediaType && !file.type) {
    return uploadAttachmentFile(
      new File([file], file.name, { type: mediaType }),
      opts.chatId,
      { signal: opts.signal, onProgress: opts.onUploadProgress },
    )
  }
  return uploadAttachmentFile(file, opts.chatId, {
    signal: opts.signal,
    onProgress: opts.onUploadProgress,
  })
}

async function extractLatestUserInput(
  messages: readonly ChatModelRunOptions['messages'][number][],
  opts: ExtractAttachmentOptions,
): Promise<{ text: string; attachments: SendAttachmentRef[] }> {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i]
    if (message.role !== 'user') continue

    // Runtime may supply string or non-array shapes; library types are array-only for user messages.
    const content = message.content as unknown
    let text = ''
    const attachments: SendAttachmentRef[] = []

    if (typeof content === 'string') {
      text = content.trim()
    } else if (Array.isArray(content)) {
      const textParts = content
        .map((part) => {
          if (part && typeof part === 'object' && part.type === 'text' && 'text' in part) {
            return typeof (part as { text?: unknown }).text === 'string' ? (part as { text: string }).text : ''
          }
          return ''
        })
      text = textParts.join('\n').trim()
      for (const part of content) {
        const att = await extractAttachmentFromUnknown(part, opts)
        if (att) attachments.push(att)
      }
    } else if (content && typeof content === 'object' && !Array.isArray(content)) {
      // Single part object: { type: 'text', text: '...' }
      const part = content as { type?: string; text?: unknown }
      if (part.type === 'text' && typeof part.text === 'string') {
        text = part.text.trim()
      } else {
        const att = await extractAttachmentFromUnknown(content, opts)
        if (att) attachments.push(att)
      }
    }

    const extraAttachments = (message as { attachments?: unknown }).attachments
    if (Array.isArray(extraAttachments)) {
      for (const part of extraAttachments) {
        const att = await extractAttachmentFromUnknown(part, opts)
        if (att) attachments.push(att)
      }
    }

    if (text.length > 0 || attachments.length > 0) {
      if (import.meta.env?.DEV && typeof console !== 'undefined' && console.debug) {
        console.debug(
          '[extractLatestUserInput]',
          text.slice(0, 80) + (text.length > 80 ? '…' : ''),
          `attachments=${attachments.length}`,
        )
      }
      return { text, attachments }
    }
  }
  return { text: '', attachments: [] }
}

type LoadHistoryFn = (
  cid?: number | null,
  personaId?: number | null,
  day?: string | null,
  opts?: { force?: boolean; limitOverride?: number; sessionId?: string | null },
) => Promise<void>

function artifactPreviewUrl(item: ArtifactItem): string {
  if (item.kind === 'html') return item.preview_url || `${item.url}?preview=1`
  return item.url
}

export function App({
  onAuthErrorRef,
}: {
  onAuthErrorRef?: React.MutableRefObject<(message: string) => void>
}) {
  const { requestConfirm, confirmDialog } = useConfirmDialog()
  const [appearance, setAppearance] = useState<Appearance>(readAppearance())
  const [uiTheme, setUiTheme] = useState<UiTheme>(readUiTheme())
  const [chatId, setChatId] = useState<number | null>(null)
  const [error, setError] = useState<string>('')
  const [statusText, setStatusText] = useState<string>('Idle')

  useEffect(() => {
    if (onAuthErrorRef) onAuthErrorRef.current = setError
  }, [onAuthErrorRef])

  const loadHistoryRef = useRef<LoadHistoryFn | null>(null)
  const loadPersonaBulletinRef = useRef<((pid: number) => Promise<void>) | null>(null)
  const resetHistoryPaginationRef = useRef<(() => void) | null>(null)
  const setHistoryLoadingRef = useRef<(loading: boolean) => void>(() => {})

  const persona = usePersonaSession({
    chatId,
    setHistoryLoading: (loading) => setHistoryLoadingRef.current(loading),
    loadHistory: (...args) => {
      const fn = loadHistoryRef.current
      return fn ? fn(...args) : Promise.resolve()
    },
    resetHistoryPagination: () => resetHistoryPaginationRef.current?.(),
    loadPersonaBulletin: (pid) => {
      const fn = loadPersonaBulletinRef.current
      return fn ? fn(pid) : Promise.resolve()
    },
    requestConfirm,
    setError,
    setStatusText,
  })

  const ops = useOperatorOps({
    activePersonaId: persona.activePersonaId,
    setError,
    setStatusText,
  })

  const chat = useChatHistory({
    chatId,
    activePersonaId: persona.activePersonaId,
    activeSessionId: persona.activeSessionId,
    setError,
    setStatusText,
    requestConfirm,
    setPersonaBookmarks: ops.setPersonaBookmarks,
  })

  useLayoutEffect(() => {
    loadHistoryRef.current = chat.loadHistory
    loadPersonaBulletinRef.current = ops.loadPersonaBulletin
    resetHistoryPaginationRef.current = chat.resetHistoryPagination
    setHistoryLoadingRef.current = chat.setHistoryLoading
  }, [
    chat.loadHistory,
    chat.resetHistoryPagination,
    chat.setHistoryLoading,
    ops.loadPersonaBulletin,
  ])

  const {
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
  } = persona

  const {
    bulletinFocus,
    personaBookmarks,
    bulletinHistorySuffix,
    bulletinOperatorMemo,
    loadPersonaBulletin,
    reloadPersonaBulletin,
    removePersonaBookmark,
    toggleMessageBookmark,
  } = ops

  const {
    historySeed,
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
  } = chat

  const [replayNotice, setReplayNotice] = useState<string>('')
  const [schedules, setSchedules] = useState<ScheduleTask[]>([])
  const [schedulesDialogOpen, setSchedulesDialogOpen] = useState<boolean>(false)
  const [schedulesShowArchived, setSchedulesShowArchived] = useState(false)
  const [memoryDialogOpen, setMemoryDialogOpen] = useState<boolean>(false)
  const [artifactsDialogOpen, setArtifactsDialogOpen] = useState<boolean>(false)
  const [terminalDialogOpen, setTerminalDialogOpen] = useState<boolean>(false)
  const [terminalError, setTerminalError] = useState<string>('')
  const [artifacts, setArtifacts] = useState<ArtifactItem[]>([])
  const [artifactsBusy, setArtifactsBusy] = useState<boolean>(false)
  const [artifactsError, setArtifactsError] = useState<string>('')
  const [artifactKindFilter, setArtifactKindFilter] = useState<string>('all')
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(null)
  const [artifactTextPreview, setArtifactTextPreview] = useState<string>('')
  const [artifactTextBusy, setArtifactTextBusy] = useState<boolean>(false)
  const [artifactTextError, setArtifactTextError] = useState<string>('')
  const [memoryContent, setMemoryContent] = useState<string>('')
  const [memoryMtimeMs, setMemoryMtimeMs] = useState<number | null>(null)
  const [memoryPathHint, setMemoryPathHint] = useState<string>('')
  const [memoryBusy, setMemoryBusy] = useState<boolean>(false)
  const [memoryError, setMemoryError] = useState<string>('')
  const [agentHistoryDialogOpen, setAgentHistoryDialogOpen] = useState(false)
  const [agentHistoryTab, setAgentHistoryTab] = useState<'trace' | 'prompt' | 'evaluators'>('trace')
  const [agentHistoryBusy, setAgentHistoryBusy] = useState(false)
  const [agentHistoryError, setAgentHistoryError] = useState('')
  const [agentHistoryRaw, setAgentHistoryRaw] = useState('')
  const [agentHistoryParsed, setAgentHistoryParsed] = useState<ParsedAgentHistory | null>(null)
  const [agentHistoryPathHint, setAgentHistoryPathHint] = useState('')
  const [agentHistoryFilename, setAgentHistoryFilename] = useState('')
  const [agentHistoryMtimeMs, setAgentHistoryMtimeMs] = useState<number | null>(null)
  const [agentHistoryIterationIdx, setAgentHistoryIterationIdx] = useState(0)
  const [agentHistoryOptimizeBusy, setAgentHistoryOptimizeBusy] = useState(false)
  const [agentHistoryOptimizeNotes, setAgentHistoryOptimizeNotes] = useState('')
  const [newSchedulePrompt, setNewSchedulePrompt] = useState('')
  const [newScheduleType, setNewScheduleType] = useState<'cron' | 'once'>('cron')
  const [newScheduleValue, setNewScheduleValue] = useState('0 9 * * *')
  const [bindings, setBindings] = useState<ChannelBinding[]>([])
  const [pendingRuns, setPendingRuns] = useState<PendingRun[]>([])
  const [stoppingRunIds, setStoppingRunIds] = useState<string[]>([])
  const [stoppingBackgroundJobIds, setStoppingBackgroundJobIds] = useState<string[]>([])
  const [queueDialogOpen, setQueueDialogOpen] = useState(false)
  const [queueShowAllPersonas, setQueueShowAllPersonas] = useState(false)
  const [scheduleDetailTask, setScheduleDetailTask] = useState<ScheduleTask | null>(null)
  const [scheduleDetailPrompt, setScheduleDetailPrompt] = useState('')
  const [scheduleDetailScheduleType, setScheduleDetailScheduleType] = useState<'cron' | 'once'>('cron')
  const [scheduleDetailScheduleValue, setScheduleDetailScheduleValue] = useState('')
  const [scheduleDetailBusy, setScheduleDetailBusy] = useState(false)
  const [agentsMdOpen, setAgentsMdOpen] = useState(false)
  const [agentsMdContent, setAgentsMdContent] = useState('')
  const [agentsMdMtimeMs, setAgentsMdMtimeMs] = useState<number | null>(null)
  const [agentsMdPath, setAgentsMdPath] = useState('')
  const [agentsMdBusy, setAgentsMdBusy] = useState(false)
  const [agentsMdError, setAgentsMdError] = useState('')
  const [settingsDialogOpen, setSettingsDialogOpen] = useState(false)
  const [settingsError, setSettingsError] = useState('')
  const [installationStatus, setInstallationStatus] = useState<InstallationStatus | null>(null)
  const [restartBusy, setRestartBusy] = useState(false)
  const [restartNotice, setRestartNotice] = useState<string | null>(null)
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
  const [mobileOpsOpen, setMobileOpsOpen] = useState(false)
  const [shortcutsOpen, setShortcutsOpen] = useState(false)
  const [mobileChatHeaderCollapsed, setMobileChatHeaderCollapsed] = useState(false)
  const [cockpitExpanded, setCockpitExpanded] = useState(false)
  const [desktopSidebarOpen, setDesktopSidebarOpen] = useState<boolean>(readDesktopSidebarOpen)
  const [desktopSidebarWidth, setDesktopSidebarWidth] = useState<number>(readDesktopSidebarWidth)
  const [desktopSidebarResizing, setDesktopSidebarResizing] = useState(false)
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => {
    if (typeof sessionStorage === 'undefined') return false
    try {
      return sessionStorage.getItem('finally-a-value-bot_onboarding_banner_dismissed') === '1'
    } catch {
      return false
    }
  })
  useEffect(() => {
    saveDesktopSidebarOpen(desktopSidebarOpen)
  }, [desktopSidebarOpen])
  useEffect(() => {
    saveDesktopSidebarWidth(desktopSidebarWidth)
  }, [desktopSidebarWidth])
  useEffect(() => {
    const onResize = () => {
      setDesktopSidebarWidth((current) => clampDesktopSidebarWidth(current, window.innerWidth))
    }
    window.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('resize', onResize)
    }
  }, [])

  const beginDesktopSidebarResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    const startX = event.clientX
    const startWidth = desktopSidebarWidth
    setDesktopSidebarResizing(true)
    const previousCursor = document.body.style.cursor
    const previousUserSelect = document.body.style.userSelect
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'

    const onPointerMove = (moveEvent: PointerEvent) => {
      const deltaX = moveEvent.clientX - startX
      setDesktopSidebarWidth(clampDesktopSidebarWidth(startWidth + deltaX, window.innerWidth))
    }
    const stopResizing = () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', stopResizing)
      window.removeEventListener('pointercancel', stopResizing)
      document.body.style.cursor = previousCursor
      document.body.style.userSelect = previousUserSelect
      setDesktopSidebarResizing(false)
    }

    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', stopResizing)
    window.addEventListener('pointercancel', stopResizing)
  }, [desktopSidebarWidth])

  const docVisible = useDocumentVisible()
  const pendingRunsForActivePersona = useMemo(
    () =>
      pendingRuns.filter(
        (r) => activePersonaId != null && activePersonaId > 0 && r.personaId === activePersonaId,
      ),
    [pendingRuns, activePersonaId],
  )
  const { queueLane, queueLanesAll, otherPersonasPending, backgroundActiveCount, backgroundJobs, invalidateOps } =
    useOpsPoll({
      chatId,
      activePersonaId,
      docVisible,
      pendingRunsForActivePersona: pendingRunsForActivePersona.length,
      setPersonas,
    })

  const queueDialogItems = useMemo(() => {
    if (queueShowAllPersonas) {
      const merged: QueueItem[] = []
      for (const lane of queueLanesAll) {
        for (const it of lane.items ?? []) {
          merged.push(it)
        }
      }
      return merged
    }
    return queueLane?.items ?? []
  }, [queueShowAllPersonas, queueLanesAll, queueLane?.items])

  const selectedSessionReadOnly = false

  const schedulesFiltered = useMemo(() => {
    if (schedulesShowArchived) return schedules
    return schedules.filter((t) => t.status !== 'completed' && t.status !== 'cancelled')
  }, [schedules, schedulesShowArchived])

  const backgroundJobsVisible = useMemo<BackgroundJobItem[]>(() => backgroundJobs.slice(0, 20), [backgroundJobs])

  function isActiveBackgroundJobStatus(status: string): boolean {
    return (
      status === 'pending'
      || status === 'running'
      || status === 'completed_raw'
      || status === 'main_agent_processing'
    )
  }

  function isTerminalBackgroundJobStatus(status: string): boolean {
    return status === 'done' || status === 'failed' || status === 'cancelled'
  }

  const prevBgJobStatusByIdRef = useRef<Map<string, string>>(new Map())

  const selectedArtifact = useMemo(
    () => artifacts.find((it) => it.id === selectedArtifactId) ?? null,
    [artifacts, selectedArtifactId],
  )

  async function loadArtifacts(cid: number | null = chatId, personaId: number | null = activePersonaId): Promise<void> {
    if (cid == null) return
    setArtifactsBusy(true)
    setArtifactsError('')
    try {
      const query = new URLSearchParams({ chat_id: String(cid), kind: artifactKindFilter || 'all' })
      if (personaId != null) query.set('persona_id', String(personaId))
      const data = await api<{ artifacts?: ArtifactItem[] }>(`/api/artifacts?${query.toString()}`)
      const list = Array.isArray(data.artifacts) ? data.artifacts : []
      setArtifacts(list)
      setSelectedArtifactId((prev) => {
        if (prev && list.some((it) => it.id === prev)) return prev
        return list.length > 0 ? list[0].id : null
      })
    } catch (e) {
      setArtifactsError(e instanceof Error ? e.message : String(e))
      setArtifacts([])
      setSelectedArtifactId(null)
    } finally {
      setArtifactsBusy(false)
    }
  }

  async function loadPersonaMemory(pid: number): Promise<void> {
    setMemoryBusy(true)
    setMemoryError('')
    try {
      const data = await api<{ content?: string; mtime_ms?: number; path?: string }>(`/api/personas/${pid}/memory`)
      setMemoryContent(typeof data.content === 'string' ? data.content : '')
      setMemoryMtimeMs(typeof data.mtime_ms === 'number' ? data.mtime_ms : null)
      setMemoryPathHint(typeof data.path === 'string' ? data.path : '')
    } catch (e) {
      setMemoryError(e instanceof Error ? e.message : String(e))
    } finally {
      setMemoryBusy(false)
    }
  }

  async function savePersonaMemory(pid: number): Promise<void> {
    setMemoryBusy(true)
    setMemoryError('')
    try {
      const res = await api<{ mtime_ms?: number }>(`/api/personas/${pid}/memory`, {
        method: 'PUT',
        body: JSON.stringify({
          content: memoryContent,
          if_match_mtime_ms: memoryMtimeMs ?? undefined,
        }),
      })
      if (typeof res.mtime_ms === 'number') {
        setMemoryMtimeMs(res.mtime_ms)
      }
      setStatusText('Memory saved')
    } catch (e) {
      setMemoryError(e instanceof Error ? e.message : String(e))
    } finally {
      setMemoryBusy(false)
    }
  }

  async function loadAgentHistoryLatest(pid: number): Promise<void> {
    setAgentHistoryBusy(true)
    setAgentHistoryError('')
    try {
      const data = await api<{
        content?: string
        path?: string
        filename?: string
        mtime_ms?: number
      }>(`/api/personas/${pid}/agent_history/latest`)
      const raw = typeof data.content === 'string' ? data.content : ''
      setAgentHistoryPathHint(typeof data.path === 'string' ? data.path : '')
      setAgentHistoryFilename(typeof data.filename === 'string' ? data.filename : '')
      setAgentHistoryMtimeMs(typeof data.mtime_ms === 'number' ? data.mtime_ms : null)
      const { traceMarkdown, qualityEvalMarkdown, initialPromptJson } = splitAgentHistoryRaw(raw)
      const parsedTrace = parseAgentHistoryMarkdown(traceMarkdown)
      setAgentHistoryRaw(traceMarkdown)
      setAgentHistoryParsed({
        ...parsedTrace,
        initialPromptJson,
        pteDecisions: parsePteDecisions(parsedTrace),
        pdqeSteps: parsePdqeSteps(qualityEvalMarkdown),
      })
      setAgentHistoryIterationIdx(0)
      setAgentHistoryTab((prev) => (prev === 'prompt' && initialPromptJson == null ? 'trace' : prev))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      const isEmpty = /no agent history for this persona/i.test(msg)
      setAgentHistoryError(
        isEmpty ? 'No saved agent run history for this persona yet.' : msg,
      )
      setAgentHistoryRaw('')
      setAgentHistoryParsed(null)
      setAgentHistoryPathHint('')
      setAgentHistoryFilename('')
      setAgentHistoryMtimeMs(null)
    } finally {
      setAgentHistoryBusy(false)
    }
  }

  async function optimizeAgentHistoryLatest(
    pid: number,
    operatorNotes?: string,
  ): Promise<void> {
    setAgentHistoryOptimizeBusy(true)
    setAgentHistoryError('')
    try {
      const body: AgentHistoryOptimizeRequest | undefined =
        operatorNotes && operatorNotes.trim()
          ? { operator_notes: operatorNotes.trim() }
          : undefined
      const data = await api<AgentHistoryOptimizeResponse>(
        `/api/personas/${pid}/agent_history/latest/optimize`,
        {
          method: 'POST',
          ...(body
            ? {
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
              }
            : {}),
        },
      )
      const jobId = typeof data.job_id === 'string' ? data.job_id : ''
      const msg =
        typeof data.message === 'string' && data.message.trim()
          ? data.message.trim()
          : 'Learn & optimize queued.'
      setReplayNotice(jobId ? `${msg} (job ${jobId})` : msg)
      if (chatId != null) void invalidateOps(chatId)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setAgentHistoryError(msg)
    } finally {
      setAgentHistoryOptimizeBusy(false)
    }
  }

  const adapter = useMemo<ChatModelAdapter>(
    () => ({
      run: async function* (options): AsyncGenerator<ChatModelRunResult, void> {
        let userText = ''
        let attachments: SendAttachmentRef[] = []
        try {
          const extracted = await extractLatestUserInput(options.messages, {
            chatId,
            signal: options.abortSignal,
            onUploadProgress: (msg) => setStatusText(msg),
          })
          userText = extracted.text
          attachments = extracted.attachments
        } catch (e) {
          // Upload failed/stalled/aborted while extracting attachments. Reset the
          // composer state so the app does not stay stuck on "Uploading…".
          if (options.abortSignal.aborted) {
            setStatusText('Idle')
            return
          }
          const msg = e instanceof Error ? e.message : String(e)
          setError(`Attachment upload failed: ${msg}`)
          setStatusText('Error')
          yield { content: [{ type: 'text', text: `Error: ${msg}` }] }
          return
        }
        const pendingReply = pendingReplyRef.current
        const messageText = pendingReply ? formatReplyForSend(pendingReply, userText) : userText
        if (!messageText.trim() && attachments.length === 0) return

        setStatusText(attachments.length > 0 ? 'Sending message…' : 'Sending...')
        setReplayNotice('')
        setError('')

        try {
          if (selectedSessionReadOnly) {
            setStatusText('Read-only channel')
            throw new Error('This chat is read-only. Switch to a web session or create a new chat to send messages.')
          }

          const sendBody: {
            chat_id?: number
            persona_id?: number
            sender_name: string
            message: string
            attachments?: SendAttachmentRef[]
          } = {
            sender_name: 'web-user',
            message: messageText,
          }
          if (chatId != null) sendBody.chat_id = chatId
          if (activePersonaId != null && activePersonaId > 0) sendBody.persona_id = activePersonaId
          if (activeSessionIdRef.current) (sendBody as Record<string, unknown>).session_id = activeSessionIdRef.current
          if (attachments.length > 0) sendBody.attachments = attachments
          const sendResponse = await api<{ run_id?: string }>('/api/send_stream', {
            method: 'POST',
            body: JSON.stringify(sendBody),
            signal: options.abortSignal,
          })

          const runId = sendResponse.run_id
          if (!runId) {
            throw new Error('missing run_id')
          }
          const threadKey = `${chatId ?? 0}:${activePersonaId ?? 0}`
          setDraftByThreadKey((prev) => {
            if (!prev[threadKey]) return prev
            return { ...prev, [threadKey]: '' }
          })
          setPendingReplyByThreadKey((prev) => {
            if (!prev[threadKey]) return prev
            const next = { ...prev }
            delete next[threadKey]
            return next
          })
          pendingReplyRef.current = null
          const pid = activePersonaId != null && activePersonaId > 0 ? activePersonaId : 0
          setPendingRuns((prev) =>
            prev.some((r) => r.runId === runId)
              ? prev
              : [...prev, { runId, personaId: pid }],
          )
          setStatusText('Queued')
          // Create assistant bubble immediately; SSE deltas will append real text.
          yield { content: [{ type: 'text', text: '' }] }

          const chatIdForRun = chatId
          const personaIdForRun = activePersonaId

          const parseJsonObject = (raw: string): Record<string, unknown> | null => {
            try {
              const parsed = JSON.parse(raw) as Record<string, unknown>
              if (parsed && typeof parsed === 'object') return parsed
              return null
            } catch {
              return null
            }
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

          let completedOrError = false
          let seenAnyDelta = false

          let pendingDelta = ''
          let lastFlushMs = Date.now()
          const sseSubscribeStartMs = Date.now()
          let firstDeltaLatencyMs: number | null = null

          const sseUrl = `/api/stream?run_id=${encodeURIComponent(runId)}`
          const sseResp = await fetch(sseUrl, { headers: makeHeaders(), signal: options.abortSignal })
          if (!sseResp.ok) {
            throw new Error(`stream subscribe failed (HTTP ${sseResp.status})`)
          }

          try {
            for await (const evt of parseSseEvents(sseResp)) {
              if (options.abortSignal.aborted) break

              if (evt.event === 'status') {
                const obj = parseJsonObject(evt.data)
                const message = typeof obj?.message === 'string' ? obj.message : null
                if (message) setStatusText(message)
                continue
              }

              if (evt.event === 'delta') {
                const obj = parseJsonObject(evt.data)
                const delta = typeof obj?.delta === 'string' ? obj.delta : ''
                if (!delta) continue

                seenAnyDelta = true
                if (firstDeltaLatencyMs == null) {
                  firstDeltaLatencyMs = Date.now() - sseSubscribeStartMs
                }
                pendingDelta += delta

                // Throttle yields to avoid token-by-token re-renders.
                const nowMs = Date.now()
                if (nowMs - lastFlushMs >= 50) {
                  yield { content: [{ type: 'text', text: pendingDelta }] }
                  pendingDelta = ''
                  lastFlushMs = nowMs
                }
                continue
              }

              if (evt.event === 'done') {
                const doneObj = parseJsonObject(evt.data)
                const responseText =
                  typeof doneObj?.response === 'string' ? doneObj.response : ''
                if (pendingDelta) {
                  yield { content: [{ type: 'text', text: pendingDelta }] }
                  pendingDelta = ''
                } else if (responseText && !seenAnyDelta) {
                  // Agent runs do not emit TextDelta today; the final text arrives on `done`.
                  yield { content: [{ type: 'text', text: responseText }] }
                  seenAnyDelta = true
                }
                completedOrError = true
                if (chatIdForRun != null) {
                  try {
                    await loadHistory(chatIdForRun, personaIdForRun ?? undefined)
                    if (personaIdForRun != null && personaIdForRun > 0) {
                      await loadPersonaBulletin(personaIdForRun)
                    }
                  } catch {
                    // History sync failed; streamed `response` text (if any) remains visible.
                  }
                }
                setPendingRuns((prev) => prev.filter((r) => r.runId !== runId))
                setStatusText('Done')
                const doneLatencyMs = Date.now() - sseSubscribeStartMs
                if (import.meta.env?.DEV && typeof console !== 'undefined' && console.debug) {
                  console.debug('[web][stream]', {
                    runId,
                    chatId: chatIdForRun,
                    personaId: personaIdForRun,
                    firstDeltaLatencyMs,
                    doneLatencyMs,
                  })
                }
                break
              }

              if (evt.event === 'error') {
                if (pendingDelta) {
                  yield { content: [{ type: 'text', text: pendingDelta }] }
                  pendingDelta = ''
                }
                completedOrError = true
                const obj = parseJsonObject(evt.data)
                const errorText = typeof obj?.error === 'string' ? obj.error : 'unknown error'
                if (!seenAnyDelta) {
                  yield { content: [{ type: 'text', text: `Error: ${errorText}` }] }
                }
                setPendingRuns((prev) => prev.filter((r) => r.runId !== runId))
                setStatusText('Error')
                const doneLatencyMs = Date.now() - sseSubscribeStartMs
                if (import.meta.env?.DEV && typeof console !== 'undefined' && console.debug) {
                  console.debug('[web][stream][error]', {
                    runId,
                    chatId: chatIdForRun,
                    personaId: personaIdForRun,
                    firstDeltaLatencyMs,
                    doneLatencyMs,
                    errorText,
                  })
                }
                break
              }
            }
          } catch (e) {
            if (!options.abortSignal.aborted) {
              const msg = e instanceof Error ? e.message : String(e)
              if (!seenAnyDelta) {
                yield { content: [{ type: 'text', text: `Error: ${msg}` }] }
              }
              setStatusText('Error')
              completedOrError = true
            }
          } finally {
            if (!completedOrError && !options.abortSignal.aborted) {
              // Stream ended without `done`/`error`; reconcile from DB once.
              setStatusText('Done')
              if (chatIdForRun != null) {
                try {
                  await loadHistory(chatIdForRun, personaIdForRun ?? undefined)
                } catch {
                  // ignore
                }
              }
            }
            setPendingRuns((prev) => prev.filter((r) => r.runId !== runId))
          }
        } finally {
          // No-op: keep existing structure for future error instrumentation.
        }
      },
    }),
    [chatId, selectedSessionReadOnly, activePersonaId, formatReplyForSend, loadHistory, loadPersonaBulletin],
  )

  function toggleAppearance(): void {
    setAppearance((prev) => (prev === 'dark' ? 'light' : 'dark'))
  }

  useEffect(() => {
    saveAppearance(appearance)
    document.documentElement.classList.toggle('dark', appearance === 'dark')
  }, [appearance])

  useEffect(() => {
    saveUiTheme(uiTheme)
    document.documentElement.setAttribute('data-ui-theme', uiTheme)
  }, [uiTheme])

  useEffect(() => {
    ; (async () => {
      try {
        setError('')
        setHistoryLoading(true)
        void loadSettings()
        const data = await api<{ chat_id?: number; persona_id?: number }>('/api/chat')
        const cid = typeof data.chat_id === 'number' ? data.chat_id : null
        const pid = typeof data.persona_id === 'number' ? data.persona_id : null
        setChatId(cid)
        if (pid != null) setActivePersonaId(pid)
        if (cid != null) {
          const chosen = await loadPersonas(cid)
          loadBindings(cid).catch(() => { })
          loadSchedules(cid).catch(() => { })
          void invalidateOps(cid)
          const personaForSession = chosen?.id ?? pid ?? null
          const sessions = await loadSessions(cid, personaForSession)
          const restoredSessionId =
            personaForSession != null ? resolveStoredSessionId(sessions, personaForSession) : null
          if (restoredSessionId) setActiveSessionId(restoredSessionId)
          resetHistoryPagination()
          await loadHistory(cid, chosen?.id ?? pid, null, {
            force: true,
            limitOverride: HISTORY_PAGE_SIZE,
            sessionId: restoredSessionId,
          })
          if (chosen?.id != null) {
            await loadPersonaBulletin(chosen.id)
          } else if (pid != null) {
            await loadPersonaBulletin(pid)
          }
          const readId = chosen?.id ?? pid ?? null
          if (readId != null) markPersonaRead(readId)
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        setHistoryLoading(false)
      }
    })()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function loadSchedules(cid: number | null = chatId): Promise<void> {
    if (cid == null) return
    try {
      const query = new URLSearchParams({ chat_id: String(cid) })
      const data = await api<{ tasks?: ScheduleTask[] }>(`/api/schedules?${query.toString()}`)
      setSchedules(Array.isArray(data.tasks) ? data.tasks : [])
    } catch {
      setSchedules([])
    }
  }

  useEffect(() => {
    if (!schedulesDialogOpen) return
    void loadSchedules(chatId)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [schedulesDialogOpen])

  useEffect(() => {
    if (!memoryDialogOpen) return
    if (activePersonaId == null) return
    void loadPersonaMemory(activePersonaId)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [memoryDialogOpen, activePersonaId])

  useEffect(() => {
    if (!artifactsDialogOpen) return
    void loadArtifacts(chatId, activePersonaId)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [artifactsDialogOpen, chatId, activePersonaId, artifactKindFilter])

  useEffect(() => {
    const item = selectedArtifact
    setArtifactTextPreview('')
    setArtifactTextError('')
    if (!item) return
    if (!(item.kind === 'markdown' || item.kind === 'text')) return
    setArtifactTextBusy(true)
    fetch(artifactPreviewUrl(item), { headers: makeHeaders() })
      .then(async (res) => {
        if (!res.ok) throw new Error(`Failed to load preview (HTTP ${res.status})`)
        return res.text()
      })
      .then((text) => setArtifactTextPreview(text))
      .catch((e) => setArtifactTextError(e instanceof Error ? e.message : String(e)))
      .finally(() => setArtifactTextBusy(false))
  }, [selectedArtifact])

  useEffect(() => {
    if (!agentsMdOpen) return
    void loadWorkspaceAgentsMd()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentsMdOpen])

  useEffect(() => {
    if (!queueDialogOpen || chatId == null) return
    void invalidateOps(chatId)
    const id = setInterval(() => {
      void invalidateOps(chatId)
    }, 2500)
    return () => clearInterval(id)
  }, [queueDialogOpen, chatId, invalidateOps])

  useEffect(() => {
    if (!agentHistoryDialogOpen) return
    if (activePersonaId == null) return
    void loadAgentHistoryLatest(activePersonaId)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentHistoryDialogOpen, activePersonaId])

  useEffect(() => {
    if (!settingsDialogOpen) return
    void loadSettings()
    if (chatId != null) {
      void loadBindings(chatId)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settingsDialogOpen, chatId])

  useEffect(() => {
    if (!agentHistoryDialogOpen) return
    const n = agentHistoryParsed?.iterations.length ?? 0
    if (n === 0 || agentHistoryTab !== 'trace') return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'ArrowLeft') {
        e.preventDefault()
        setAgentHistoryIterationIdx((i) => Math.max(0, i - 1))
      } else if (e.key === 'ArrowRight') {
        e.preventDefault()
        setAgentHistoryIterationIdx((i) => Math.min(n - 1, i + 1))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [agentHistoryDialogOpen, agentHistoryParsed?.iterations.length, agentHistoryTab])

  async function loadBindings(cid: number | null = chatId): Promise<void> {
    if (cid == null) return
    try {
      const query = new URLSearchParams({ chat_id: String(cid) })
      const data = await api<{ bindings?: ChannelBinding[] }>(`/api/contacts/bindings?${query.toString()}`)
      setBindings(Array.isArray(data.bindings) ? data.bindings : [])
    } catch {
      setBindings([])
    }
  }

  async function loadSettings(): Promise<void> {
    setSettingsError('')
    try {
      const data = await api<{
        installation_status?: InstallationStatus
      }>('/api/settings')
      setInstallationStatus(data.installation_status ?? null)
    } catch (e) {
      setSettingsError(e instanceof Error ? e.message : String(e))
      setInstallationStatus(null)
    }
  }


  async function requestRestart(): Promise<void> {
    setSettingsError('')
    setRestartNotice(null)
    setRestartBusy(true)
    try {
      const data = await api<{ ok?: boolean; message?: string }>('/api/restart', { method: 'POST' })
      setRestartNotice(data.message ?? 'Restart initiated.')
    } catch (e) {
      setSettingsError(e instanceof Error ? e.message : String(e))
    } finally {
      setRestartBusy(false)
    }
  }



  async function updateChannelPersonaPolicy(
    botInstanceId: number,
    mode: 'all' | 'single',
    personaId?: number,
  ): Promise<void> {
    if (chatId == null) return
    setSettingsError('')
    try {
      if (mode === 'all') {
        await api('/api/channel_persona_policy', {
          method: 'DELETE',
          body: JSON.stringify({ chat_id: chatId, bot_instance_id: botInstanceId }),
        })
      } else {
        await api('/api/channel_persona_policy', {
          method: 'POST',
          body: JSON.stringify({
            chat_id: chatId,
            bot_instance_id: botInstanceId,
            mode: 'single',
            persona_id: personaId,
          }),
        })
      }
      await loadBindings(chatId)
    } catch (e) {
      setSettingsError(e instanceof Error ? e.message : String(e))
    }
  }

  async function loadQueueDiagnostics(cid: number | null = chatId): Promise<void> {
    await invalidateOps(cid)
  }

  async function loadBackgroundVisibility(cid: number | null = chatId): Promise<void> {
    await invalidateOps(cid)
  }

  async function bindToContact(contactChatId: number): Promise<void> {
    await api('/api/contacts/bind', {
      method: 'POST',
      body: JSON.stringify({ contact_chat_id: contactChatId }),
    })
    await loadBindings(chatId)
    resetHistoryPagination()
    if (activePersonaId != null && activePersonaId > 0) {
      await loadHistory(chatId, activePersonaId, null, {
        force: true,
        limitOverride: HISTORY_PAGE_SIZE,
      })
    }
  }

  async function unlinkContact(): Promise<void> {
    await api('/api/contacts/unlink', {
      method: 'POST',
      body: JSON.stringify({}),
    })
    await loadBindings(chatId)
  }

  async function createSchedule(
    prompt: string,
    scheduleType: string,
    scheduleValue: string,
    personaId?: number | null,
  ): Promise<void> {
    await api('/api/schedules', {
      method: 'POST',
      body: JSON.stringify({
        chat_id: chatId ?? undefined,
        prompt,
        schedule_type: scheduleType,
        schedule_value: scheduleValue,
        persona_id: personaId && personaId > 0 ? personaId : undefined,
      }),
    })
    await loadSchedules(chatId)
  }

  async function updateSchedule(
    taskId: number,
    patch: {
      status?: string
      persona_id?: number
      prompt?: string
      schedule_type?: string
      schedule_value?: string
      timezone?: string
    },
  ): Promise<void> {
    await api(`/api/schedules/${taskId}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    })
    await loadSchedules(chatId)
  }

  async function cancelQueueRun(runId: string): Promise<void> {
    await api('/api/queue/cancel', {
      method: 'POST',
      body: JSON.stringify({ run_id: runId, chat_id: chatId ?? undefined }),
    })
    await loadQueueDiagnostics(chatId)
  }

  async function removeQueueRun(runId: string): Promise<void> {
    await api('/api/queue/remove', {
      method: 'POST',
      body: JSON.stringify({ run_id: runId, chat_id: chatId ?? undefined }),
    })
    await loadQueueDiagnostics(chatId)
  }

  async function handleQueueAction(runId: string, state: string): Promise<void> {
    const id = runId.trim()
    if (!id) return
    if (stoppingRunIds.includes(id)) return
    setStoppingRunIds((prev) => (prev.includes(id) ? prev : [...prev, id]))
    try {
      if (state === 'running') {
        await cancelQueueRun(id)
      } else {
        await removeQueueRun(id)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setStoppingRunIds((prev) => prev.filter((x) => x !== id))
      await loadQueueDiagnostics(chatId)
    }
  }

  async function cancelBackgroundJob(jobId: string): Promise<void> {
    await api('/api/background_jobs/cancel', {
      method: 'POST',
      body: JSON.stringify({ job_id: jobId, chat_id: chatId ?? undefined }),
    })
    await loadBackgroundVisibility(chatId)
  }

  async function handleBackgroundJobStop(jobId: string): Promise<void> {
    const id = jobId.trim()
    if (!id) return
    if (stoppingBackgroundJobIds.includes(id)) return
    setStoppingBackgroundJobIds((prev) => (prev.includes(id) ? prev : [...prev, id]))
    try {
      await cancelBackgroundJob(id)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setStoppingBackgroundJobIds((prev) => prev.filter((x) => x !== id))
      await loadBackgroundVisibility(chatId)
    }
  }

  async function loadWorkspaceAgentsMd(): Promise<void> {
    setAgentsMdError('')
    setAgentsMdBusy(true)
    try {
      const data = await api<{ content?: string; mtime_ms?: number; path?: string }>('/api/workspace/agents_md')
      setAgentsMdContent(typeof data.content === 'string' ? data.content : '')
      setAgentsMdMtimeMs(typeof data.mtime_ms === 'number' ? data.mtime_ms : null)
      setAgentsMdPath(typeof data.path === 'string' ? data.path : '')
    } catch (e) {
      setAgentsMdError(e instanceof Error ? e.message : String(e))
    } finally {
      setAgentsMdBusy(false)
    }
  }

  async function saveWorkspaceAgentsMd(): Promise<void> {
    setAgentsMdError('')
    setAgentsMdBusy(true)
    try {
      const data = await api<{ mtime_ms?: number }>('/api/workspace/agents_md', {
        method: 'PUT',
        body: JSON.stringify({
          content: agentsMdContent,
          if_match_mtime_ms: agentsMdMtimeMs ?? undefined,
        }),
      })
      if (typeof data.mtime_ms === 'number') {
        setAgentsMdMtimeMs(data.mtime_ms)
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes('409') || msg.toLowerCase().includes('conflict')) {
        setAgentsMdError('File changed on disk. Reload and retry.')
      } else {
        setAgentsMdError(msg)
      }
    } finally {
      setAgentsMdBusy(false)
    }
  }

  useEffect(() => {
    if (chatId == null) return
    let cancelled = false
    async function init() {
      const chosen = await loadPersonas(chatId)
      if (cancelled) return
      loadBindings(chatId).catch(() => { })
      loadSchedules(chatId).catch(() => { })
      void invalidateOps(chatId)
      if (chosen) {
        try {
          await api('/api/personas/switch', {
            method: 'POST',
            body: JSON.stringify({ chat_id: chatId ?? undefined, persona_name: chosen.name }),
          })
        } catch {
          // ignore; we still load history for the chosen persona
        }
      }
      resetHistoryPagination()
      loadHistory(chatId, chosen?.id, null, { force: true, limitOverride: HISTORY_PAGE_SIZE }).catch((e) =>
        setError(e instanceof Error ? e.message : String(e)),
      )
      if (chosen?.id != null) {
        loadPersonaBulletin(chosen.id).catch(() => { })
      }
    }
    init()
    return () => { cancelled = true }
  }, [chatId, invalidateOps, resetHistoryPagination])

  useEffect(() => {
    if (activePersonaId != null && activePersonaId > 0) {
      setNewSchedulePersonaId((prev) => (prev == null ? activePersonaId : prev))
    }
  }, [activePersonaId, setNewSchedulePersonaId])

  useEffect(() => {
    setPendingRuns([])
  }, [chatId])

  useEffect(() => {
    prevBgJobStatusByIdRef.current = new Map()
  }, [chatId])

  useEffect(() => {
    if (chatId == null) return
    const prev = prevBgJobStatusByIdRef.current
    let shouldReloadThread = false
    let reloadBulletin = false
    for (const job of backgroundJobs) {
      const last = prev.get(job.id)
      if (
        last !== undefined
        && isActiveBackgroundJobStatus(last)
        && isTerminalBackgroundJobStatus(job.status)
      ) {
        if (activePersonaId == null || job.persona_id === activePersonaId) {
          shouldReloadThread = true
        }
        if (activePersonaId != null && job.persona_id === activePersonaId) {
          reloadBulletin = true
        }
      }
    }
    const next = new Map<string, string>()
    for (const job of backgroundJobs) {
      next.set(job.id, job.status)
    }
    prevBgJobStatusByIdRef.current = next
    if (!shouldReloadThread) return
    setStatusText('Done')
    void loadHistory(chatId, activePersonaId ?? undefined)
    if (reloadBulletin && activePersonaId != null) {
      void loadPersonaBulletin(activePersonaId)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId, activePersonaId, backgroundJobs])

  // Lightweight refresh for cross-channel / background replies (no SSE for those paths).
  useEffect(() => {
    if (chatId == null) return
    if (!docVisible) return
    if (pendingRunsForActivePersona.length > 0) return
    const shouldPollFast = backgroundActiveCount > 0 || (queueLane?.pending ?? 0) > 0
    const intervalMs = shouldPollFast ? 5000 : 30000
    const interval = setInterval(() => {
      void loadHistory(chatId, activePersonaId ?? undefined).catch(() => { })
    }, intervalMs)
    return () => clearInterval(interval)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    chatId,
    activePersonaId,
    docVisible,
    pendingRunsForActivePersona.length,
    backgroundActiveCount,
    queueLane?.pending,
  ])

  const runtimeKey = `${chatId ?? 0}-${activePersonaId ?? 0}`

  const handleMobileThreadScroll = useCallback((opts: {
    collapseHeader: boolean
    source: 'scroll' | 'reset' | 'focus' | 'media-change'
    scrollTop?: number
  }) => {
    if (opts.source !== 'scroll') {
      setMobileChatHeaderCollapsed(false)
      return
    }
    if ((opts.scrollTop ?? 0) < 28) {
      setMobileChatHeaderCollapsed(false)
      return
    }
    setMobileChatHeaderCollapsed((prev) => (prev === opts.collapseHeader ? prev : opts.collapseHeader))
  }, [])

  useEffect(() => {
    setMobileChatHeaderCollapsed(false)
  }, [runtimeKey])

  useEffect(() => {
    if (mobileNavOpen) setMobileChatHeaderCollapsed(false)
  }, [mobileNavOpen])

  useEffect(() => {
    const mq = window.matchMedia('(min-width: 768px)')
    const clearCollapsed = () => {
      if (mq.matches) setMobileChatHeaderCollapsed(false)
    }
    mq.addEventListener('change', clearCollapsed)
    clearCollapsed()
    return () => mq.removeEventListener('change', clearCollapsed)
  }, [])

  const bookmarkedMessageIds = useMemo(
    () => new Set(personaBookmarks.map((b) => b.message_id)),
    [personaBookmarks],
  )

  useKeyboardShortcuts({
    activePendingReply,
    onDismissPendingReply: handleDismissPendingReply,
    settingsDialogOpen,
    mobileOpsOpen,
    shortcutsOpen,
    onOpenShortcuts: () => setShortcutsOpen(true),
  })

  const radixAccent = RADIX_ACCENT_BY_THEME[uiTheme] ?? 'green'

  return (
    <Theme
      appearance={appearance}
      accentColor={radixAccent as never}
      grayColor="slate"
      radius="large"
      panelBackground="translucent"
      scaling="100%"
    >
      <AuthDialog />

      <div
        className={
          appearance === 'dark'
            ? 'h-[100dvh] min-w-0 w-full overflow-hidden bg-[var(--mc-bg-main)] pb-[env(safe-area-inset-bottom,0px)] pt-[env(safe-area-inset-top,0px)]'
            : 'h-[100dvh] min-w-0 w-full overflow-hidden bg-[radial-gradient(1200px_560px_at_-8%_-10%,#d1fae5_0%,transparent_58%),radial-gradient(1200px_560px_at_108%_-12%,#e0f2fe_0%,transparent_58%),#f8fafc] pb-[env(safe-area-inset-bottom,0px)] pt-[env(safe-area-inset-top,0px)]'
        }
      >
        {mobileNavOpen ? (
          <div
            className="fixed inset-0 z-[100] flex md:hidden"
            role="dialog"
            aria-modal="true"
            aria-label="Persona and theme"
          >
            <button
              type="button"
              className="absolute inset-0 bg-black/50"
              aria-label="Close menu"
              onClick={() => setMobileNavOpen(false)}
            />
            <div
              id="mobile-session-sidebar-panel"
              className="relative z-[101] flex h-full min-h-0 w-[min(320px,100vw)] max-w-[90vw] flex-col border-r border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-sidebar)] pt-[env(safe-area-inset-top,0px)] pl-[env(safe-area-inset-left,0px)]"
            >
              <SessionSidebar
                appearance={appearance}
                onToggleAppearance={toggleAppearance}
                uiTheme={uiTheme}
                onUiThemeChange={(theme) => setUiTheme(theme as UiTheme)}
                uiThemeOptions={UI_THEME_OPTIONS}
                personas={personas}
                personaHasNew={personaHasNew}
                selectedPersonaId={activePersonaId}
                onPersonaSelect={(name) => void switchPersona(name)}
                onCreatePersona={() => void onCreatePersona()}
                onDeletePersona={(id) => void onDeletePersona(id)}
                onCloseRequest={() => setMobileNavOpen(false)}
              />
            </div>
          </div>
        ) : null}

        <div
          className={desktopSidebarOpen ? 'mc-layout-grid mc-layout-grid--sidebar-open' : 'mc-layout-grid'}
          style={desktopSidebarOpen ? ({ '--mc-sidebar-width': `${desktopSidebarWidth}px` } as React.CSSProperties) : undefined}
        >
          {desktopSidebarOpen ? (
            <div id="desktop-session-sidebar" className="relative hidden min-h-0 md:flex md:flex-col">
              <SessionSidebar
                appearance={appearance}
                onToggleAppearance={toggleAppearance}
                uiTheme={uiTheme}
                onUiThemeChange={(theme) => setUiTheme(theme as UiTheme)}
                uiThemeOptions={UI_THEME_OPTIONS}
                personas={personas}
                personaHasNew={personaHasNew}
                selectedPersonaId={activePersonaId}
                onPersonaSelect={(name) => void switchPersona(name)}
                onCreatePersona={() => void onCreatePersona()}
                onDeletePersona={(id) => void onDeletePersona(id)}
              />
              <div
                role="separator"
                tabIndex={0}
                aria-label="Resize personas sidebar"
                aria-orientation="vertical"
                aria-valuemin={DESKTOP_SIDEBAR_MIN_WIDTH}
                aria-valuemax={DESKTOP_SIDEBAR_MAX_WIDTH}
                aria-valuenow={desktopSidebarWidth}
                className={desktopSidebarResizing ? 'mc-sidebar-resize-handle mc-sidebar-resize-handle--active' : 'mc-sidebar-resize-handle'}
                onPointerDown={beginDesktopSidebarResize}
                onKeyDown={(event) => {
                  if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
                  event.preventDefault()
                  const delta = event.key === 'ArrowRight' ? 16 : -16
                  setDesktopSidebarWidth((current) => clampDesktopSidebarWidth(current + delta, window.innerWidth))
                }}
              />
            </div>
          ) : null}

          <main
            className={
              appearance === 'dark'
                ? 'flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-[var(--mc-bg-panel)]'
                : 'flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-[color:var(--mc-surface-elevated)]/95'
            }
          >
            <AppHeader
              appearance={appearance}
              mobileChatHeaderCollapsed={mobileChatHeaderCollapsed}
              nav={{
                mobileNavOpen,
                onOpenMobileNav: () => setMobileNavOpen(true),
                desktopSidebarOpen,
                onToggleDesktopSidebar: () => setDesktopSidebarOpen((v) => !v),
              }}
              session={{
                activePersonaId,
                activePersonaName,
                chatSessions,
                activeSessionId,
                historyLoading,
                onSelectSession: (sid) => void handleSelectSession(sid),
                onCreateSession: handleCreateSession,
                onArchiveSession: handleArchiveSession,
                onReopenSession: handleReopenSession,
                onDeleteSession: handleDeleteSession,
              }}
              toolbar={{
                queueLane,
                backgroundActiveCount,
                installationStatus,
                statusText,
                onExpandCockpit: () => setCockpitExpanded(true),
                onOpenSettings: () => setSettingsDialogOpen(true),
                onOpenSchedules: () => setSchedulesDialogOpen(true),
                onOpenQueue: () => setQueueDialogOpen(true),
                onOpenPrinciples: () => setAgentsMdOpen(true),
                onOpenArtifacts: () => setArtifactsDialogOpen(true),
                onOpenTerminal: () => {
                  setTerminalError('')
                  setTerminalDialogOpen(true)
                },
                terminalAvailable: installationStatus?.terminal?.web_terminal_available === true,
                onOpenMemory: () => setMemoryDialogOpen(true),
                onOpenAgentHistory: () => setAgentHistoryDialogOpen(true),
                agentHistoryDisabled: activePersonaId == null,
              }}
              onOpenMobileSettings={() => setSettingsDialogOpen(true)}
              onOpenMobileOps={() => setMobileOpsOpen(true)}
            />
            <div
              className={
                appearance === 'dark'
                  ? 'relative flex min-h-0 min-w-0 flex-1 flex-col bg-[linear-gradient(to_bottom,var(--mc-bg-panel),var(--mc-bg-main)_28%)]'
                  : 'relative flex min-h-0 min-w-0 flex-1 flex-col bg-[linear-gradient(to_bottom,#f8fafc,white_20%)]'
              }
            >
              <div
                className={`pointer-events-none absolute left-0 right-0 top-2 z-20 flex justify-center px-2 transition-all duration-200 max-md:ease-out ${
                  mobileChatHeaderCollapsed && !cockpitExpanded
                    ? 'max-md:-translate-y-2 max-md:opacity-0'
                    : 'max-md:translate-y-0 max-md:opacity-100'
                }`}
              >
                <div className="pointer-events-auto w-full max-w-5xl">
                  <CockpitBar
                    appearance={appearance}
                    statusText={statusText}
                    queueLane={queueLane}
                    otherPersonasPending={otherPersonasPending}
                    backgroundActiveCount={backgroundActiveCount}
                    installationStatus={installationStatus}
                    onQueueClick={() => setQueueDialogOpen(true)}
                    bulletinFocus={bulletinFocus}
                    bookmarks={personaBookmarks}
                    activePersonaId={activePersonaId}
                    onRemoveBookmark={removePersonaBookmark}
                    historySuffix={bulletinHistorySuffix}
                    operatorMemoServer={bulletinOperatorMemo}
                    reloadBulletin={reloadPersonaBulletin}
                    onBulletinStatus={(msg) => setStatusText(msg)}
                    onExpandedChange={setCockpitExpanded}
                    expanded={cockpitExpanded}
                    floating
                  />
                </div>
              </div>
              <div className="mx-auto w-full max-w-5xl px-2 pt-6 md:px-3 md:pt-8">
                <StatusRegion message={statusText} />
                {installationStatus != null &&
                !onboardingDismissed &&
                (!installationStatus.llm_ready || !installationStatus.channel_ready) ? (
                  <Callout.Root color="orange" size="1" variant="soft" className="mb-2">
                    <Flex direction="column" gap="2">
                      <Callout.Text>
                        Finish setup: configure <code className="text-xs">.env</code> with at least one channel (Telegram or Discord) and LLM keys, then restart the gateway if needed. See Settings for status.
                      </Callout.Text>
                      <Flex gap="2" align="center" wrap="wrap">
                        <Button size="1" variant="solid" onClick={() => setSettingsDialogOpen(true)}>
                          Open Settings
                        </Button>
                        <Button
                          size="1"
                          variant="soft"
                          onClick={() => {
                            try {
                              sessionStorage.setItem('finally-a-value-bot_onboarding_banner_dismissed', '1')
                            } catch {
                              /* ignore */
                            }
                            setOnboardingDismissed(true)
                          }}
                        >
                          Dismiss
                        </Button>
                      </Flex>
                    </Flex>
                  </Callout.Root>
                ) : null}
                {replayNotice ? (
                  <Callout.Root color="orange" size="1" variant="soft">
                    <Callout.Text>{replayNotice}</Callout.Text>
                  </Callout.Root>
                ) : null}
                {error ? (
                  <ErrorBanner
                    message={error}
                    className={replayNotice ? 'mt-2' : ''}
                    onDismiss={() => setError('')}
                  />
                ) : null}
              </div>

              <div className="flex min-h-0 min-w-0 flex-1 flex-col px-0 pb-1 md:px-1">
                <div className="min-h-0 min-w-0 flex-1">
                  <ThreadPane
                    key={runtimeKey}
                    adapter={adapter}
                    initialMessages={historySeed}
                    runtimeKey={runtimeKey}
                    isStreaming={pendingRunsForActivePersona.length > 0}
                    historyLoading={historyLoading}
                    historyHasMore={historyHasMore}
                    historyLoadingMore={historyLoadingMore}
                    onLoadMoreHistory={() => void loadMoreHistory()}
                    draftText={activeDraftText}
                    onDraftTextChange={handleDraftTextChange}
                    bookmarkedMessageIds={bookmarkedMessageIds}
                    onToggleBookmark={toggleMessageBookmark}
                    onReplyToMessage={(id) => void handleReplyToMessage(id)}
                    onDeleteMessage={(id) => void handleDeleteMessage(id)}
                    pendingReply={activePendingReply}
                    onDismissPendingReply={handleDismissPendingReply}
                    onMobileThreadScroll={handleMobileThreadScroll}
                    onShowShortcuts={() => setShortcutsOpen(true)}
                    uploadHint={
                      statusText.startsWith('Uploading') || statusText.startsWith('Sending message')
                        ? statusText
                        : undefined
                    }
                  />
                </div>
              </div>
            </div>
          </main>
        </div>

      </div>
      <AppDialogs
        appearance={appearance}
        api={api}
        chatId={chatId}
        activePersonaId={activePersonaId}
        personas={personas}
        settings={{
          open: settingsDialogOpen,
          onOpenChange: setSettingsDialogOpen,
          error: settingsError,
          setError: setSettingsError,
          restartNotice,
          installationStatus,
          restartBusy,
          requestRestart,
          bindings,
          updateChannelPersonaPolicy,
          reloadInstallationStatus: loadSettings,
        }}
        queue={{
          open: queueDialogOpen,
          onOpenChange: setQueueDialogOpen,
          showAllPersonas: queueShowAllPersonas,
          setShowAllPersonas: setQueueShowAllPersonas,
          items: queueDialogItems,
          stoppingRunIds,
          handleQueueAction,
          backgroundJobs: backgroundJobsVisible,
          stoppingBackgroundJobIds,
          isActiveBackgroundJobStatus,
          handleBackgroundJobStop,
          onOpenSchedules: () => setSchedulesDialogOpen(true),
        }}
        schedules={{
          open: schedulesDialogOpen,
          onOpenChange: setSchedulesDialogOpen,
          showArchived: schedulesShowArchived,
          setShowArchived: setSchedulesShowArchived,
          schedules,
          filtered: schedulesFiltered,
          newPrompt: newSchedulePrompt,
          setNewPrompt: setNewSchedulePrompt,
          newType: newScheduleType,
          setNewType: setNewScheduleType,
          newValue: newScheduleValue,
          setNewValue: setNewScheduleValue,
          newPersonaId: newSchedulePersonaId,
          setNewPersonaId: setNewSchedulePersonaId,
          createSchedule,
          updateSchedule,
          openDetail: (t) => {
            setScheduleDetailTask(t)
            setScheduleDetailPrompt(t.prompt)
            setScheduleDetailScheduleType(t.schedule_type === 'once' ? 'once' : 'cron')
            setScheduleDetailScheduleValue(t.schedule_value)
          },
        }}
        scheduleDetail={{
          task: scheduleDetailTask,
          onOpenChange: (o) => {
            if (!o) {
              setScheduleDetailTask(null)
              setScheduleDetailBusy(false)
              setScheduleDetailScheduleValue('')
            }
          },
          prompt: scheduleDetailPrompt,
          setPrompt: setScheduleDetailPrompt,
          scheduleType: scheduleDetailScheduleType,
          setScheduleType: setScheduleDetailScheduleType,
          scheduleValue: scheduleDetailScheduleValue,
          setScheduleValue: setScheduleDetailScheduleValue,
          busy: scheduleDetailBusy,
          setBusy: setScheduleDetailBusy,
          updateSchedule,
          close: () => setScheduleDetailTask(null),
        }}
        agentsMd={{
          open: agentsMdOpen,
          onOpenChange: (o) => {
            setAgentsMdOpen(o)
            if (!o) {
              setAgentsMdError('')
              setAgentsMdBusy(false)
            }
          },
          path: agentsMdPath,
          error: agentsMdError,
          setError: setAgentsMdError,
          setBusy: setAgentsMdBusy,
          content: agentsMdContent,
          setContent: setAgentsMdContent,
          mtimeMs: agentsMdMtimeMs,
          busy: agentsMdBusy,
          load: loadWorkspaceAgentsMd,
          save: saveWorkspaceAgentsMd,
        }}
        artifacts={{
          open: artifactsDialogOpen,
          onOpenChange: (open) => {
            setArtifactsDialogOpen(open)
            if (!open) {
              setArtifactsError('')
              setArtifactTextError('')
            }
          },
          setError: setArtifactsError,
          setTextError: setArtifactTextError,
          kindFilter: artifactKindFilter,
          setKindFilter: setArtifactKindFilter,
          load: loadArtifacts,
          busy: artifactsBusy,
          error: artifactsError,
          items: artifacts,
          selectedId: selectedArtifactId,
          setSelectedId: setSelectedArtifactId,
          selected: selectedArtifact,
          textPreview: artifactTextPreview,
          textBusy: artifactTextBusy,
          textError: artifactTextError,
        }}
        memory={{
          open: memoryDialogOpen,
          onOpenChange: (open) => {
            setMemoryDialogOpen(open)
            if (!open) {
              setMemoryError('')
              setMemoryBusy(false)
            }
          },
          setError: setMemoryError,
          setBusy: setMemoryBusy,
          pathHint: memoryPathHint,
          error: memoryError,
          content: memoryContent,
          setContent: setMemoryContent,
          mtimeMs: memoryMtimeMs,
          busy: memoryBusy,
          load: loadPersonaMemory,
          save: savePersonaMemory,
        }}
        agentHistory={{
          open: agentHistoryDialogOpen,
          onOpenChange: (open) => {
            setAgentHistoryDialogOpen(open)
            if (open) {
              setAgentHistoryTab('trace')
            }
            if (!open) {
              setAgentHistoryError('')
              setAgentHistoryBusy(false)
            }
          },
          setTab: setAgentHistoryTab,
          setError: setAgentHistoryError,
          setBusy: setAgentHistoryBusy,
          pathHint: agentHistoryPathHint,
          filename: agentHistoryFilename,
          mtimeMs: agentHistoryMtimeMs,
          busy: agentHistoryBusy,
          error: agentHistoryError,
          parsed: agentHistoryParsed,
          tab: agentHistoryTab,
          iterationIdx: agentHistoryIterationIdx,
          setIterationIdx: setAgentHistoryIterationIdx,
          raw: agentHistoryRaw,
          optimizeBusy: agentHistoryOptimizeBusy,
          optimizeNotes: agentHistoryOptimizeNotes,
          setOptimizeNotes: setAgentHistoryOptimizeNotes,
          load: loadAgentHistoryLatest,
          optimize: optimizeAgentHistoryLatest,
        }}
        terminal={{
          open: terminalDialogOpen,
          onOpenChange: (open) => {
            setTerminalDialogOpen(open)
            if (!open) setTerminalError('')
          },
          error: terminalError,
          setError: setTerminalError,
        }}
      />
      {confirmDialog}
      <MobileOpsSheet
        open={mobileOpsOpen}
        onOpenChange={setMobileOpsOpen}
        onOpenQueue={() => setQueueDialogOpen(true)}
        onOpenSchedules={() => setSchedulesDialogOpen(true)}
        onOpenPrinciples={() => setAgentsMdOpen(true)}
        onOpenArtifacts={() => setArtifactsDialogOpen(true)}
        onOpenTerminal={() => {
          setTerminalError('')
          setTerminalDialogOpen(true)
        }}
        terminalAvailable={installationStatus?.terminal?.web_terminal_available === true}
        onOpenMemory={() => setMemoryDialogOpen(true)}
        onOpenAgentHistory={() => setAgentHistoryDialogOpen(true)}
        agentHistoryDisabled={activePersonaId == null}
      />
      <ShortcutsDialog open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
    </Theme>
  )
}
