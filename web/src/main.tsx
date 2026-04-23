import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { QueryClientProvider } from '@tanstack/react-query'
import { createRoot } from 'react-dom/client'
import type { ChatModelAdapter, ChatModelRunOptions, ChatModelRunResult, ThreadMessageLike } from '@assistant-ui/react'
import {
  Button,
  Callout,
  Dialog,
  DropdownMenu,
  Flex,
  Heading,
  IconButton,
  Select,
  Switch,
  Tabs,
  Text,
  TextField,
  Theme,
} from '@radix-ui/themes'
import remarkGfm from 'remark-gfm'
import ReactMarkdown from 'react-markdown'
import '@radix-ui/themes/styles.css'
import '@assistant-ui/react-ui/styles/index.css'
import './styles.css'
import { api, AUTH_REQUIRED_EVENT, makeHeaders, sanitizeHttpHeaderValue, WEB_AUTH_STORAGE_KEY } from './api/client'
import { CockpitBar } from './components/cockpit-bar'
import { SessionSidebar } from './components/session-sidebar'
import { ThreadPane } from './components/thread-pane'
import { useDocumentVisible } from './hooks/use-document-visible'
import { useOpsPoll } from './hooks/use-ops-poll'
import { queryClient } from './query-client'
import { historiesEqual, mapBackendHistory, shouldDeferHistoryRemount } from './lib/history-sync'
import { parseAgentHistoryMarkdown, type ParsedAgentHistory } from './parse-agent-history'
import type {
  ArtifactItem,
  BackendMessage,
  BotInstanceRow,
  ChannelBinding,
  InstallationStatus,
  Persona,
  PersonaBulletinUpdate,
  PersonaMessageBookmark,
  RuntimeSettingItem,
  ScheduleTask,
} from './types'

type Appearance = 'dark' | 'light'
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

const PERSONA_STORAGE_KEY = 'finally-a-value-bot_selected_persona_id'
const PERSONA_LAST_READ_STORAGE_KEY = 'finally-a-value-bot_persona_last_read_v1'

function readStoredPersonaId(): number | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = localStorage.getItem(PERSONA_STORAGE_KEY)
    if (raw === null) return null
    const n = parseInt(raw, 10)
    return Number.isFinite(n) ? n : null
  } catch {
    return null
  }
}

function writeStoredPersonaId(id: number): void {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(PERSONA_STORAGE_KEY, String(id))
  } catch {
    // ignore
  }
}

function readPersonaLastReadAt(chatId: number, personaId: number): string | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = localStorage.getItem(PERSONA_LAST_READ_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Record<string, unknown>
    const key = `${chatId}:${personaId}`
    const v = parsed[key]
    return typeof v === 'string' ? v : null
  } catch {
    return null
  }
}

function writePersonaLastReadAt(chatId: number, personaId: number, isoTimestamp: string): void {
  if (typeof window === 'undefined') return
  try {
    const raw = localStorage.getItem(PERSONA_LAST_READ_STORAGE_KEY)
    const parsed: Record<string, unknown> = raw ? JSON.parse(raw) : {}
    parsed[`${chatId}:${personaId}`] = isoTimestamp
    localStorage.setItem(PERSONA_LAST_READ_STORAGE_KEY, JSON.stringify(parsed))
  } catch {
    // ignore
  }
}

function toMs(iso: string | null | undefined): number | null {
  if (!iso) return null
  const ms = Date.parse(iso)
  return Number.isFinite(ms) ? ms : null
}

if (typeof document !== 'undefined') {
  document.documentElement.classList.toggle('dark', readAppearance() === 'dark')
  document.documentElement.setAttribute('data-ui-theme', readUiTheme())
}

type SendAttachmentPayload = {
  filename?: string
  media_type?: string
  data_base64: string
}

function splitDataUrl(value: string): { mimeType?: string; base64: string } | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  if (!trimmed.startsWith('data:')) return { base64: trimmed }
  const comma = trimmed.indexOf(',')
  if (comma < 0) return null
  const header = trimmed.slice(5, comma)
  const base64 = trimmed.slice(comma + 1)
  const mimeType = header.split(';')[0] || undefined
  return { mimeType, base64 }
}

async function fileToBase64(file: File): Promise<string> {
  const buf = await file.arrayBuffer()
  let binary = ''
  const bytes = new Uint8Array(buf)
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i])
  }
  return btoa(binary)
}

async function extractAttachmentFromUnknown(part: unknown): Promise<SendAttachmentPayload | null> {
  if (!part || typeof part !== 'object') return null
  const obj = part as Record<string, unknown>

  const fileVal = obj.file
  if (fileVal instanceof File) {
    return {
      filename: fileVal.name || undefined,
      media_type: fileVal.type || undefined,
      data_base64: await fileToBase64(fileVal),
    }
  }

  const candidateData =
    (typeof obj.data === 'string' ? obj.data : null) ||
    (typeof obj.url === 'string' && String(obj.url).startsWith('data:') ? String(obj.url) : null) ||
    (typeof obj.image === 'string' && String(obj.image).startsWith('data:') ? String(obj.image) : null) ||
    (typeof obj.source === 'string' && String(obj.source).startsWith('data:') ? String(obj.source) : null)

  if (!candidateData) return null
  const parsed = splitDataUrl(candidateData)
  if (!parsed || !parsed.base64) return null

  const filename = typeof obj.filename === 'string' ? obj.filename : undefined
  const mediaType =
    (typeof obj.mediaType === 'string' ? obj.mediaType : undefined) ||
    (typeof obj.mimeType === 'string' ? obj.mimeType : undefined) ||
    (typeof obj.contentType === 'string' ? obj.contentType : undefined) ||
    parsed.mimeType

  return {
    filename,
    media_type: mediaType,
    data_base64: parsed.base64,
  }
}

async function extractLatestUserInput(
  messages: readonly ChatModelRunOptions['messages'][number][],
): Promise<{ text: string; attachments: SendAttachmentPayload[] }> {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i]
    if (message.role !== 'user') continue

    // Runtime may supply string or non-array shapes; library types are array-only for user messages.
    const content = message.content as unknown
    let text = ''
    const attachments: SendAttachmentPayload[] = []

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
        const att = await extractAttachmentFromUnknown(part)
        if (att) attachments.push(att)
      }
    } else if (content && typeof content === 'object' && !Array.isArray(content)) {
      // Single part object: { type: 'text', text: '...' }
      const part = content as { type?: string; text?: unknown }
      if (part.type === 'text' && typeof part.text === 'string') {
        text = part.text.trim()
      } else {
        const att = await extractAttachmentFromUnknown(content)
        if (att) attachments.push(att)
      }
    }

    const extraAttachments = (message as { attachments?: unknown }).attachments
    if (Array.isArray(extraAttachments)) {
      for (const part of extraAttachments) {
        const att = await extractAttachmentFromUnknown(part)
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

function formatBytes(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 'unknown size'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / (1024 * 1024)).toFixed(1)} MB`
}

function artifactPreviewUrl(item: ArtifactItem): string {
  if (item.kind === 'html') return item.preview_url || `${item.url}?preview=1`
  return item.url
}

function AgentHistoryMarkdownBody({ markdown }: { markdown: string }) {
  return (
    <div className="aui-md-root text-sm leading-relaxed">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          table: ({ className, ...props }) => (
            <div className="mc-md-table-scroll">
              <table className={['aui-md-table', className].filter(Boolean).join(' ')} {...props} />
            </div>
          ),
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  )
}

function App() {
  const [appearance, setAppearance] = useState<Appearance>(readAppearance())
  const [uiTheme, setUiTheme] = useState<UiTheme>(readUiTheme())
  const [chatId, setChatId] = useState<number | null>(null)
  const [historySeed, setHistorySeed] = useState<ThreadMessageLike[]>([])
  const [historyByDay, setHistoryByDay] = useState<Record<string, ThreadMessageLike[]>>({})
  const [runtimeNonce, setRuntimeNonce] = useState<number>(0)
  const [error, setError] = useState<string>('')
  const [statusText, setStatusText] = useState<string>('Idle')
  const [replayNotice, setReplayNotice] = useState<string>('')
  const [authRequired, setAuthRequired] = useState<boolean>(false)
  const [authTokenInput, setAuthTokenInput] = useState<string>('')
  const [personas, setPersonas] = useState<Persona[]>([])
  const [bulletinUpdates, setBulletinUpdates] = useState<PersonaBulletinUpdate[]>([])
  const [personaBookmarks, setPersonaBookmarks] = useState<PersonaMessageBookmark[]>([])
  const [activePersonaId, setActivePersonaId] = useState<number | null>(null)
  const [schedules, setSchedules] = useState<ScheduleTask[]>([])
  const [schedulesDialogOpen, setSchedulesDialogOpen] = useState<boolean>(false)
  const [schedulesShowArchived, setSchedulesShowArchived] = useState(false)
  const [memoryDialogOpen, setMemoryDialogOpen] = useState<boolean>(false)
  const [artifactsDialogOpen, setArtifactsDialogOpen] = useState<boolean>(false)
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
  const [agentHistoryBusy, setAgentHistoryBusy] = useState(false)
  const [agentHistoryError, setAgentHistoryError] = useState('')
  const [agentHistoryRaw, setAgentHistoryRaw] = useState('')
  const [agentHistoryParsed, setAgentHistoryParsed] = useState<ParsedAgentHistory | null>(null)
  const [agentHistoryPathHint, setAgentHistoryPathHint] = useState('')
  const [agentHistoryFilename, setAgentHistoryFilename] = useState('')
  const [agentHistoryMtimeMs, setAgentHistoryMtimeMs] = useState<number | null>(null)
  const [agentHistoryIterationIdx, setAgentHistoryIterationIdx] = useState(0)
  const [newSchedulePrompt, setNewSchedulePrompt] = useState('')
  const [newScheduleType, setNewScheduleType] = useState<'cron' | 'once'>('cron')
  const [newScheduleValue, setNewScheduleValue] = useState('0 9 * * *')
  const [newSchedulePersonaId, setNewSchedulePersonaId] = useState<number | null>(null)
  const [bindings, setBindings] = useState<ChannelBinding[]>([])
  const [pendingRunIds, setPendingRunIds] = useState<string[]>([])
  const [queueDialogOpen, setQueueDialogOpen] = useState(false)
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
  const [personaReadNonce, setPersonaReadNonce] = useState<number>(0)
  const [historyPollUntilMs, setHistoryPollUntilMs] = useState<number>(0)
  const [settingsDialogOpen, setSettingsDialogOpen] = useState(false)
  const [settingsBusy, setSettingsBusy] = useState(false)
  const [settingsError, setSettingsError] = useState('')
  const [runtimeSettings, setRuntimeSettings] = useState<RuntimeSettingItem[]>([])
  const [installationStatus, setInstallationStatus] = useState<InstallationStatus | null>(null)
  const [botInstances, setBotInstances] = useState<BotInstanceRow[]>([])
  const [restartBusy, setRestartBusy] = useState(false)
  const [botFormBusy, setBotFormBusy] = useState(false)
  const [newBotPlatform, setNewBotPlatform] = useState<'telegram' | 'discord'>('telegram')
  const [newBotLabel, setNewBotLabel] = useState('')
  const [newBotToken, setNewBotToken] = useState('')
  const [restartNotice, setRestartNotice] = useState<string | null>(null)
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
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
  const { queueLane, backgroundActiveCount, invalidateOps } = useOpsPoll({
    chatId,
    docVisible,
    pendingRunIdsLength: pendingRunIds.length,
    setPersonas,
  })

  const historySeedRef = useRef<ThreadMessageLike[]>([])
  const deferredHistoryRef = useRef<ThreadMessageLike[] | null>(null)

  useEffect(() => {
    historySeedRef.current = historySeed
  }, [historySeed])

  const flushDeferredHistory = useCallback(() => {
    const pending = deferredHistoryRef.current
    if (!pending) return
    deferredHistoryRef.current = null
    if (historiesEqual(historySeedRef.current, pending)) return
    setHistorySeed(pending)
    setRuntimeNonce((x) => x + 1)
  }, [])

  const schedulesFiltered = useMemo(() => {
    if (schedulesShowArchived) return schedules
    return schedules.filter((t) => t.status !== 'completed' && t.status !== 'cancelled')
  }, [schedules, schedulesShowArchived])

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

  function markPersonaRead(personaId: number): void {
    if (chatId == null) return
    writePersonaLastReadAt(chatId, personaId, new Date().toISOString())
    setPersonaReadNonce((x) => x + 1)
  }

  React.useEffect(() => {
    const onAuthRequired = () => setAuthRequired(true)
    window.addEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
    return () => window.removeEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
  }, [])

  const activePersonaName = personas.find((p) => p.id === activePersonaId)?.name ?? null
  const selectedSessionLabel = activePersonaName ? `Chat · ${activePersonaName}` : 'Chat'
  const selectedSessionReadOnly = false

  /** Loads personas and applies stored preference; returns the chosen persona id and name for history/switch. */
  async function loadPersonas(cid: number | null = chatId): Promise<{ id: number; name: string } | null> {
    if (cid == null) return null
    try {
      const query = new URLSearchParams({ chat_id: String(cid) })
      const data = await api<{ personas?: { id: number; name: string; is_active: boolean; last_bot_message_at?: string | null }[] }>(`/api/personas?${query.toString()}`)
      const list = Array.isArray(data.personas) ? data.personas : []
      const personaList = list.map((p) => ({ id: p.id, name: p.name, is_active: p.is_active, last_bot_message_at: p.last_bot_message_at ?? null }))
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
        if (newSchedulePersonaId == null) {
          setNewSchedulePersonaId(chosen.id)
        }
        if (!storedInList) writeStoredPersonaId(chosen.id)
      } else {
        setActivePersonaId(null)
        if (newSchedulePersonaId == null) {
          setNewSchedulePersonaId(null)
        }
      }
      return chosen
    } catch {
      setPersonas([])
      setActivePersonaId(null)
      return null
    }
  }

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

  async function switchPersona(personaName: string): Promise<void> {
    if (chatId == null) return
    await api('/api/personas/switch', {
      method: 'POST',
      body: JSON.stringify({ chat_id: chatId, persona_name: personaName }),
    })
    const p = personas.find((x) => x.name === personaName)
    if (p) writeStoredPersonaId(p.id)
    await loadPersonas(chatId)
    await loadHistory(chatId, p?.id ?? undefined, null, { force: true })
    if (p) markPersonaRead(p.id)
    if (p) await loadPersonaBulletin(p.id)
    setRuntimeNonce((x) => x + 1)
  }

  async function loadHistory(
    cid: number | null = chatId,
    personaId?: number | null,
    day?: string | null,
    opts?: { force?: boolean },
  ): Promise<void> {
    if (cid == null) return
    const force = opts?.force === true
    const query = new URLSearchParams({ chat_id: String(cid) })
    if (personaId != null && personaId > 0) query.set('persona_id', String(personaId))
    if (day) query.set('day', day)
    else query.set('limit', '500')
    const data = await api<{ messages?: BackendMessage[] }>(`/api/history?${query.toString()}`)
    const rawMessages = Array.isArray(data.messages) ? data.messages : []
    const mapped = mapBackendHistory(rawMessages)
    if (day) {
      const nextByDay = { ...historyByDay, [day]: mapped }
      const allDays = Object.keys(nextByDay).sort()
      const combined = allDays.flatMap((d) => (nextByDay[d] ?? []))
      setHistoryByDay(nextByDay)
      if (!historiesEqual(historySeedRef.current, combined)) {
        deferredHistoryRef.current = null
        setHistorySeed(combined)
        setRuntimeNonce((x) => x + 1)
      }
    } else {
      setHistoryByDay({})
      if (!historiesEqual(historySeedRef.current, mapped)) {
        if (!force && shouldDeferHistoryRemount()) {
          deferredHistoryRef.current = mapped
          return
        }
        deferredHistoryRef.current = null
        setHistorySeed(mapped)
        setRuntimeNonce((x) => x + 1)
      }
    }
  }

  async function loadPersonaBulletin(pid: number): Promise<void> {
    try {
      const data = await api<{
        updates?: PersonaBulletinUpdate[]
        bookmarks?: PersonaMessageBookmark[]
      }>(`/api/personas/${pid}/bulletin`)
      setBulletinUpdates(Array.isArray(data.updates) ? data.updates : [])
      setPersonaBookmarks(Array.isArray(data.bookmarks) ? data.bookmarks : [])
    } catch {
      setBulletinUpdates([])
      setPersonaBookmarks([])
    }
  }

  async function toggleMessageBookmark(messageId: string, role: 'user' | 'assistant'): Promise<void> {
    if (activePersonaId == null) return
    const alreadyBookmarked = personaBookmarks.some((b) => b.message_id === messageId)
    try {
      if (alreadyBookmarked) {
        await api(`/api/personas/${activePersonaId}/bookmarks/${encodeURIComponent(messageId)}`, {
          method: 'DELETE',
        })
        setPersonaBookmarks((prev) => prev.filter((b) => b.message_id !== messageId))
        setStatusText('Bookmark removed')
      } else {
        const res = await api<{
          bookmark?: PersonaMessageBookmark
        }>(`/api/personas/${activePersonaId}/bookmarks`, {
          method: 'POST',
          body: JSON.stringify({ message_id: messageId }),
        })
        const next = res.bookmark
        if (next) {
          setPersonaBookmarks((prev) => [next, ...prev.filter((b) => b.message_id !== messageId)])
        } else {
          await loadPersonaBulletin(activePersonaId)
        }
        setStatusText(`Bookmarked ${role} message`)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
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
      setAgentHistoryRaw(raw)
      setAgentHistoryParsed(parseAgentHistoryMarkdown(raw))
      setAgentHistoryIterationIdx(0)
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

  const adapter = useMemo<ChatModelAdapter>(
    () => ({
      run: async function* (options): AsyncGenerator<ChatModelRunResult, void> {
        const { text: userText, attachments } = await extractLatestUserInput(options.messages)
        if (!userText && attachments.length === 0) return

        setStatusText('Sending...')
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
            attachments?: SendAttachmentPayload[]
          } = {
            sender_name: 'web-user',
            message: userText,
          }
          if (chatId != null) sendBody.chat_id = chatId
          if (activePersonaId != null && activePersonaId > 0) sendBody.persona_id = activePersonaId
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
          setPendingRunIds((prev) => (prev.includes(runId) ? prev : [...prev, runId]))
          setStatusText('Queued')
          // A background-handoff run can finish quickly while its final reply arrives later.
          // Keep history fresh for a short window after sending.
          setHistoryPollUntilMs(Date.now() + 2 * 60 * 1000)
          yield {
            content: [
              {
                type: 'text',
                text: 'Queued. I will send the final reply when this run completes.',
              },
            ],
          }
        } finally {
        }
      },
    }),
    [chatId, selectedSessionReadOnly, activePersonaId],
  )

  function toggleAppearance(): void {
    setAppearance((prev) => (prev === 'dark' ? 'light' : 'dark'))
  }

  async function onCreatePersona(): Promise<void> {
    if (chatId == null) return
    const name = window.prompt('Persona name (e.g. work, creative):')
    if (name == null || !name.trim()) return
    try {
      await api<{ persona_id?: number }>('/api/personas/create', {
        method: 'POST',
        body: JSON.stringify({ chat_id: chatId, name: name.trim() }),
      })
      await loadPersonas(chatId)
      setStatusText(`Persona "${name.trim()}" created`)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  async function onDeletePersona(personaId: number): Promise<void> {
    if (chatId == null) return
    if (!window.confirm('Delete this persona? Its messages and session will be removed.')) return
    try {
      await api('/api/personas/delete', {
        method: 'POST',
        body: JSON.stringify({ chat_id: chatId, persona_id: personaId }),
      })
      await loadPersonas(chatId)
      if (activePersonaId === personaId) await loadHistory(chatId, undefined, null, { force: true })
      setStatusText('Persona deleted')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
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
          await loadHistory(cid, chosen?.id ?? pid, null, { force: true })
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
    void loadBotInstances()
    if (chatId != null) {
      void loadBindings(chatId)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settingsDialogOpen, chatId])

  useEffect(() => {
    if (!agentHistoryDialogOpen) return
    const n = agentHistoryParsed?.iterations.length ?? 0
    if (n === 0) return
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
  }, [agentHistoryDialogOpen, agentHistoryParsed?.iterations.length])

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
    setSettingsBusy(true)
    setSettingsError('')
    try {
      const data = await api<{
        settings?: RuntimeSettingItem[]
        installation_status?: InstallationStatus
      }>('/api/settings')
      const items = Array.isArray(data.settings) ? data.settings : []
      setRuntimeSettings(items)
      setInstallationStatus(data.installation_status ?? null)
    } catch (e) {
      setSettingsError(e instanceof Error ? e.message : String(e))
      setRuntimeSettings([])
      setInstallationStatus(null)
    } finally {
      setSettingsBusy(false)
    }
  }

  async function loadBotInstances(): Promise<void> {
    try {
      const data = await api<{ instances?: BotInstanceRow[] }>('/api/channel_bot_instances')
      setBotInstances(Array.isArray(data.instances) ? data.instances : [])
    } catch {
      setBotInstances([])
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

  async function addBotInstance(): Promise<void> {
    const label = newBotLabel.trim()
    const token = newBotToken.trim()
    if (!label || !token) {
      setSettingsError('Label and token are required.')
      return
    }
    setSettingsError('')
    setBotFormBusy(true)
    try {
      await api('/api/channel_bot_instances', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          platform: newBotPlatform,
          label,
          token,
        }),
      })
      setNewBotLabel('')
      setNewBotToken('')
      await loadBotInstances()
    } catch (e) {
      setSettingsError(e instanceof Error ? e.message : String(e))
    } finally {
      setBotFormBusy(false)
    }
  }

  async function removeBotInstance(id: number): Promise<void> {
    if (!window.confirm('Delete this bot instance? This cannot be undone.')) return
    setSettingsError('')
    setBotFormBusy(true)
    try {
      await api(`/api/channel_bot_instances/${id}`, { method: 'DELETE' })
      await loadBotInstances()
    } catch (e) {
      setSettingsError(e instanceof Error ? e.message : String(e))
    } finally {
      setBotFormBusy(false)
    }
  }

  async function updateChannelPersonaPolicy(
    botInstanceId: number,
    mode: 'all' | 'single',
    personaId?: number,
  ): Promise<void> {
    if (chatId == null) return
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
    await loadHistory(chatId, undefined, null, { force: true })
    setRuntimeNonce((x) => x + 1)
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
      loadHistory(chatId, chosen?.id, null, { force: true }).catch((e) =>
        setError(e instanceof Error ? e.message : String(e)),
      )
      if (chosen?.id != null) {
        loadPersonaBulletin(chosen.id).catch(() => { })
      }
    }
    init()
    return () => { cancelled = true }
  }, [chatId, invalidateOps])

  useEffect(() => {
    if (activePersonaId != null && activePersonaId > 0) {
      setNewSchedulePersonaId((prev) => (prev == null ? activePersonaId : prev))
      loadPersonaBulletin(activePersonaId).catch(() => { })
    } else {
      setBulletinUpdates([])
      setPersonaBookmarks([])
    }
  }, [activePersonaId])

  useEffect(() => {
    setPendingRunIds([])
  }, [chatId, activePersonaId])

  useEffect(() => {
    if (pendingRunIds.length === 0) return
    let cancelled = false
    const interval = setInterval(() => {
      ; (async () => {
        const completed: string[] = []
        for (const runId of pendingRunIds) {
          try {
            const status = await api<{ done?: boolean }>(
              `/api/run_status?run_id=${encodeURIComponent(runId)}`,
            )
            if (status.done === true) completed.push(runId)
          } catch {
            // run not found / auth issue / transient error: leave pending
          }
        }
        if (cancelled || completed.length === 0) return
        setPendingRunIds((prev) => prev.filter((id) => !completed.includes(id)))
        setStatusText('Done')
        void loadHistory(chatId, activePersonaId ?? undefined)
        if (activePersonaId != null) {
          void loadPersonaBulletin(activePersonaId)
        }
        setHistoryPollUntilMs(Date.now() + 2 * 60 * 1000)
      })()
    }, 2500)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingRunIds, chatId, activePersonaId])

  useEffect(() => {
    if (chatId == null) return
    if (historyPollUntilMs <= Date.now()) return
    let cancelled = false
    const interval = setInterval(() => {
      if (cancelled) return
      if (historyPollUntilMs <= Date.now()) return
      loadHistory(chatId, activePersonaId ?? undefined).catch(() => { })
    }, 10000)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId, activePersonaId, historyPollUntilMs])

  const runtimeKey = `${chatId ?? 0}-${activePersonaId ?? 0}-${runtimeNonce}`
  const bookmarkedMessageIds = useMemo(
    () => new Set(personaBookmarks.map((b) => b.message_id)),
    [personaBookmarks],
  )
  const radixAccent = RADIX_ACCENT_BY_THEME[uiTheme] ?? 'green'

  useEffect(() => {
    const onFocusOut = (e: FocusEvent) => {
      const t = e.target as HTMLElement | null
      if (!t?.closest?.('.aui-composer-root')) return
      const rt = e.relatedTarget as HTMLElement | null
      if (!rt?.closest?.('.aui-composer-root')) {
        flushDeferredHistory()
      }
    }
    document.addEventListener('focusout', onFocusOut, true)
    return () => document.removeEventListener('focusout', onFocusOut, true)
  }, [flushDeferredHistory])

  useEffect(() => {
    const vp = document.querySelector('.aui-thread-viewport')
    if (!vp) return
    const onScroll = () => {
      const el = vp as HTMLElement
      if (el.scrollHeight - el.scrollTop - el.clientHeight <= 100) {
        flushDeferredHistory()
      }
    }
    vp.addEventListener('scroll', onScroll, { passive: true })
    return () => vp.removeEventListener('scroll', onScroll)
  }, [flushDeferredHistory, runtimeNonce])

  function submitAuthToken() {
    const token = sanitizeHttpHeaderValue(authTokenInput)
    if (!token) return
    if (token.length !== authTokenInput.trim().length) {
      setError('Invalid API token: unsupported header characters.')
      return
    }
    sessionStorage.setItem(WEB_AUTH_STORAGE_KEY, token)
    setAuthRequired(false)
    setAuthTokenInput('')
    window.location.reload()
  }

  return (
    <Theme
      appearance={appearance}
      accentColor={radixAccent as never}
      grayColor="slate"
      radius="large"
      panelBackground="translucent"
      scaling="100%"
    >
      <Dialog.Root open={authRequired} onOpenChange={(open) => !open && setAuthRequired(false)}>
        <Dialog.Content>
          <Dialog.Title>API token required</Dialog.Title>
          <Dialog.Description size="2" mb="3">
            This server requires an API token. Use the same value as <code>WEB_AUTH_TOKEN</code> in your .env.
          </Dialog.Description>
          <Flex direction="column" gap="3">
            <TextField.Root
              type="password"
              placeholder="Enter token"
              value={authTokenInput}
              onChange={(e) => setAuthTokenInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submitAuthToken()}
            />
            <Button onClick={() => submitAuthToken()}>Continue</Button>
          </Flex>
        </Dialog.Content>
      </Dialog.Root>

      <div
        className={
          appearance === 'dark'
            ? 'h-[100dvh] min-w-0 w-full overflow-x-hidden bg-[var(--mc-bg-main)] pb-[env(safe-area-inset-bottom,0px)] pt-[env(safe-area-inset-top,0px)]'
            : 'h-[100dvh] min-w-0 w-full overflow-x-hidden bg-[radial-gradient(1200px_560px_at_-8%_-10%,#d1fae5_0%,transparent_58%),radial-gradient(1200px_560px_at_108%_-12%,#e0f2fe_0%,transparent_58%),#f8fafc] pb-[env(safe-area-inset-bottom,0px)] pt-[env(safe-area-inset-top,0px)]'
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
              className={
                appearance === 'dark'
                  ? 'relative z-[101] flex h-full min-h-0 w-[min(320px,100vw)] max-w-[90vw] flex-col border-r border-[color:var(--mc-border-soft)] shadow-xl'
                  : 'relative z-[101] flex h-full min-h-0 w-[min(320px,100vw)] max-w-[90vw] flex-col border-r border-slate-200 bg-white shadow-xl'
              }
              style={appearance === 'dark' ? { background: 'var(--mc-bg-sidebar)' } : undefined}
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
                : 'flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-white/95'
            }
          >
            <header
              className={
                appearance === 'dark'
                  ? 'sticky top-0 z-10 border-b border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)]/95 backdrop-blur-sm'
                  : 'sticky top-0 z-10 border-b border-slate-200 bg-white/92 backdrop-blur-sm'
              }
            >
              <div className="px-4 py-3">
              <Flex
                justify="between"
                align="center"
                gap="2"
                wrap="wrap"
                className="w-full flex-col md:flex-row md:flex-wrap"
              >
                <Flex align="center" gap="2" className="min-h-[44px] min-w-0 w-full md:flex-1">
                  <IconButton
                    size="3"
                    variant="soft"
                    color="gray"
                    className="shrink-0 md:!hidden min-h-10 min-w-10"
                    type="button"
                    aria-expanded={mobileNavOpen}
                    aria-haspopup="dialog"
                    aria-controls="mobile-session-sidebar-panel"
                    aria-label="Open personas and theme"
                    title="Personas & theme"
                    onClick={() => setMobileNavOpen(true)}
                  >
                    <svg
                      className="size-5 shrink-0"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden
                    >
                      <path d="M4 6h16M4 12h16M4 18h16" />
                    </svg>
                  </IconButton>
                  <IconButton
                    size="3"
                    variant="soft"
                    color="gray"
                    className="!hidden shrink-0 md:!inline-flex"
                    type="button"
                    aria-expanded={desktopSidebarOpen}
                    aria-label={desktopSidebarOpen ? 'Hide personas sidebar' : 'Show personas sidebar'}
                    title={desktopSidebarOpen ? 'Hide personas' : 'Show personas'}
                    onClick={() => setDesktopSidebarOpen((v) => !v)}
                  >
                    <span aria-hidden className="text-base leading-none">
                      {desktopSidebarOpen ? '⟨' : '⟩'}
                    </span>
                  </IconButton>
                  <Heading size="6" className="min-w-0 flex-1 truncate max-md:[font-size:1.125rem]">
                    {selectedSessionLabel}
                  </Heading>
                  <div className="ml-auto shrink-0 md:hidden">
                    <DropdownMenu.Root>
                      <DropdownMenu.Trigger>
                        <Button size="2" variant="soft" type="button" className="min-h-10">
                          More
                        </Button>
                      </DropdownMenu.Trigger>
                      <DropdownMenu.Content size="2">
                        <DropdownMenu.Item onSelect={() => setSettingsDialogOpen(true)}>Settings</DropdownMenu.Item>
                        <DropdownMenu.Item onSelect={() => setSchedulesDialogOpen(true)}>Schedules</DropdownMenu.Item>
                        <DropdownMenu.Item onSelect={() => setAgentsMdOpen(true)}>Principles</DropdownMenu.Item>
                        <DropdownMenu.Item onSelect={() => setArtifactsDialogOpen(true)}>Artifacts</DropdownMenu.Item>
                        <DropdownMenu.Item onSelect={() => setMemoryDialogOpen(true)}>Memory</DropdownMenu.Item>
                        <DropdownMenu.Item
                          disabled={activePersonaId == null}
                          onSelect={() => {
                            if (activePersonaId != null) setAgentHistoryDialogOpen(true)
                          }}
                        >
                          Last agent run
                        </DropdownMenu.Item>
                      </DropdownMenu.Content>
                    </DropdownMenu.Root>
                  </div>
                </Flex>
                <Flex align="center" gap="2" wrap="wrap" justify="end" className="!hidden md:!flex">
                  <Dialog.Root open={settingsDialogOpen} onOpenChange={setSettingsDialogOpen}>
                    <Dialog.Trigger className="!hidden md:!inline-flex">
                      <Button size="1" variant="soft">
                        Settings
                      </Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 920 }}>
                      <Dialog.Title>Web UI configuration</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        LLM provider, API keys, and models are configured in repo-root <code className="text-xs">.env</code> only (for example <code className="text-xs">LLM_PROVIDER</code>, <code className="text-xs">LLM_API_KEY</code>, <code className="text-xs">LLM_MODEL</code>). Changing <code className="text-xs">.env</code> requires a process restart. Generic key-value writes to <code className="text-xs">app_settings</code> are disabled; the list below is read-only.
                      </Dialog.Description>
                      {settingsError ? (
                        <Callout.Root color="red" size="1" variant="soft" className="mb-2">
                          <Callout.Text>{settingsError}</Callout.Text>
                        </Callout.Root>
                      ) : null}
                      {restartNotice ? (
                        <Callout.Root color="green" size="1" variant="soft" className="mb-2">
                          <Callout.Text>{restartNotice}</Callout.Text>
                        </Callout.Root>
                      ) : null}
                      <Tabs.Root defaultValue="overview">
                        <Tabs.List size="1" className="mb-3 flex-wrap">
                          <Tabs.Trigger value="overview">Overview</Tabs.Trigger>
                          <Tabs.Trigger value="integrations">Integrations</Tabs.Trigger>
                          <Tabs.Trigger value="channels">Channels</Tabs.Trigger>
                          <Tabs.Trigger value="legacy">Legacy</Tabs.Trigger>
                        </Tabs.List>
                        <Tabs.Content value="overview">
                          {installationStatus ? (
                            <Flex direction="column" gap="2" mb="2">
                              <Flex gap="2" wrap="wrap" align="center">
                                <Text size="1" color={installationStatus.llm_ready ? 'green' : 'orange'}>
                                  LLM: {installationStatus.llm_ready ? 'ready' : 'missing'}
                                </Text>
                                <Text size="1" color={installationStatus.channel_ready ? 'green' : 'orange'}>
                                  Channels: {installationStatus.channel_ready ? 'ready' : 'missing'}
                                </Text>
                                <Text size="1" color="gray">
                                  Env restart needed:{' '}
                                  {(installationStatus.requires_restart_for_env_changes ??
                                    installationStatus.requires_restart_to_apply_runtime_settings) === true
                                    ? 'yes'
                                    : 'no'}
                                </Text>
                              </Flex>
                              <Text size="1" color="gray">
                                Stop requests between LLM/tool iterations (cooperative cancel). Use Queue for FIFO visibility.
                              </Text>
                              <div>
                                <Button
                                  size="1"
                                  variant="solid"
                                  disabled={restartBusy}
                                  onClick={() => void requestRestart()}
                                >
                                  {restartBusy ? 'Restarting…' : 'Restart gateway'}
                                </Button>
                              </div>
                            </Flex>
                          ) : (
                            <Text size="2" color="gray" mb="2">
                              Loading installation status…
                            </Text>
                          )}
                        </Tabs.Content>
                        <Tabs.Content value="legacy">
                      <div
                        className="rounded-md border p-3"
                        style={appearance === 'dark'
                          ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
                          : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
                      >
                        <Text size="2" weight="bold" className="mb-1">Legacy app_settings (read-only)</Text>
                        <Text size="1" color="gray" className="mb-2 block">
                          Rows are not merged into process env at startup. Use <code className="text-xs">.env</code> for configuration. LLM keys are not stored here.
                        </Text>
                        <Flex justify="between" align="center" gap="2" wrap="wrap">
                          <Text size="1" color="gray">
                            Rows: {runtimeSettings.length}
                          </Text>
                          <Button size="1" variant="soft" onClick={() => void loadSettings()} disabled={settingsBusy}>
                            Reload
                          </Button>
                        </Flex>
                      </div>
                        </Tabs.Content>
                        <Tabs.Content value="integrations">
                      <div
                        className="rounded-md border p-3"
                        style={appearance === 'dark'
                          ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
                          : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
                      >
                        <Text size="2" weight="bold" className="mb-1">Bot integrations</Text>
                        <Text size="1" color="gray" className="mb-2 block">
                          Additional Telegram or Discord bots beyond env-seeded instances. Tokens are stored in the database; primary instances from config may be read-only here.
                        </Text>
                        <div className="mb-3 space-y-1">
                          {botInstances.length === 0 ? (
                            <Text size="1" color="gray">No instances loaded (check auth).</Text>
                          ) : (
                            botInstances.map((row) => (
                              <Flex
                                key={row.id}
                                gap="2"
                                align="center"
                                wrap="wrap"
                                className="border-t border-[color:var(--gray-6)] pt-2 first:border-t-0 first:pt-0"
                              >
                                <Text size="1" className="min-w-[120px] font-mono">
                                  #{row.id} {row.platform}
                                </Text>
                                <Text size="1" className="min-w-[100px]">
                                  {row.label}
                                </Text>
                                <Text size="1" color="gray">
                                  {row.token_redacted}
                                </Text>
                                {row.env_primary ? (
                                  <Text size="1" color="gray">(from env)</Text>
                                ) : (
                                  <Button
                                    size="1"
                                    color="red"
                                    variant="soft"
                                    disabled={botFormBusy}
                                    onClick={() => void removeBotInstance(row.id)}
                                  >
                                    Delete
                                  </Button>
                                )}
                              </Flex>
                            ))
                          )}
                        </div>
                        <Text size="1" weight="bold" className="mb-1 block">Add bot instance</Text>
                        <Flex gap="2" wrap="wrap" align="end">
                          <div>
                            <Text size="1" color="gray" className="mb-1 block">Platform</Text>
                            <Select.Root
                              value={newBotPlatform}
                              onValueChange={(v) => setNewBotPlatform(v === 'discord' ? 'discord' : 'telegram')}
                            >
                              <Select.Trigger className="w-[140px]" />
                              <Select.Content>
                                <Select.Item value="telegram">telegram</Select.Item>
                                <Select.Item value="discord">discord</Select.Item>
                              </Select.Content>
                            </Select.Root>
                          </div>
                          <TextField.Root
                            className="min-w-[160px] flex-1"
                            placeholder="Label"
                            value={newBotLabel}
                            onChange={(e) => setNewBotLabel(e.target.value)}
                          />
                          <TextField.Root
                            className="min-w-[200px] flex-1"
                            type="password"
                            placeholder="Bot token"
                            value={newBotToken}
                            onChange={(e) => setNewBotToken(e.target.value)}
                          />
                          <Button
                            size="1"
                            disabled={botFormBusy}
                            onClick={() => void addBotInstance()}
                          >
                            {botFormBusy ? '…' : 'Add'}
                          </Button>
                        </Flex>
                      </div>
                        </Tabs.Content>
                        <Tabs.Content value="channels">
                      <div
                        className="rounded-md border p-3"
                        style={appearance === 'dark'
                          ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
                          : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
                      >
                        <Text size="2" weight="bold">External channel persona mode</Text>
                        <Text size="1" color="gray" className="mb-2 block">
                          Per Telegram/Discord/WhatsApp bot instance: allow all personas or lock to one. Web chat is not listed here — use the persona selector in the chat UI.
                        </Text>
                        <div className="space-y-2">
                          {bindings.length === 0 ? (
                            <Text size="1" color="gray">No channel bindings found for this contact.</Text>
                          ) : bindings.map((b) => {
                            const currentMode = b.persona_mode === 'single' ? 'single' : 'all'
                            const currentPersonaId = b.persona_id ?? activePersonaId ?? personas[0]?.id ?? null
                            return (
                              <Flex key={`${b.bot_instance_id}:${b.channel_type}:${b.channel_handle}`} gap="2" align="center" wrap="wrap">
                                <Text size="1" className="min-w-[200px]">
                                  {b.channel_type} (bot #{b.bot_instance_id}): {b.channel_handle}
                                </Text>
                                <Select.Root
                                  value={currentMode}
                                  onValueChange={(mode) => {
                                    if (mode === 'all') {
                                      void updateChannelPersonaPolicy(b.bot_instance_id, 'all')
                                    } else if (currentPersonaId != null) {
                                      void updateChannelPersonaPolicy(b.bot_instance_id, 'single', currentPersonaId)
                                    }
                                  }}
                                >
                                  <Select.Trigger className="w-[140px]" />
                                  <Select.Content>
                                    <Select.Item value="all">All personas</Select.Item>
                                    <Select.Item value="single">Single persona</Select.Item>
                                  </Select.Content>
                                </Select.Root>
                                {currentMode === 'single' ? (
                                  <Select.Root
                                    value={currentPersonaId != null ? String(currentPersonaId) : ''}
                                    onValueChange={(value) => {
                                      const pid = Number(value)
                                      if (Number.isFinite(pid) && pid > 0) {
                                        void updateChannelPersonaPolicy(b.bot_instance_id, 'single', pid)
                                      }
                                    }}
                                  >
                                    <Select.Trigger className="w-[180px]" placeholder="Persona" />
                                    <Select.Content>
                                      {personas.map((p) => (
                                        <Select.Item key={p.id} value={String(p.id)}>
                                          {p.name}
                                        </Select.Item>
                                      ))}
                                    </Select.Content>
                                  </Select.Root>
                                ) : null}
                              </Flex>
                            )
                          })}
                        </div>
                      </div>
                        </Tabs.Content>
                      </Tabs.Root>
                      <Flex justify="end" mt="4">
                        <Dialog.Close>
                          <Button variant="soft">Close</Button>
                        </Dialog.Close>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>
                  <Dialog.Root open={queueDialogOpen} onOpenChange={setQueueDialogOpen}>
                    <Dialog.Content style={{ maxWidth: 920 }}>
                      <Dialog.Title>Run queue</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        Pending and running agent work for this chat (FIFO). Stop requests cooperative cancellation between iterations.
                      </Dialog.Description>
                      <div className="max-h-[min(420px,60vh)] overflow-auto rounded-md border p-2" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}>
                        {(queueLane?.items?.length ?? 0) === 0 ? (
                          <Text size="2" color="gray">No queued runs (lane idle or diagnostics loading).</Text>
                        ) : (
                          <>
                            <table className="hidden w-full border-collapse text-left text-sm md:table">
                              <thead>
                                <tr className={appearance === 'dark' ? 'text-slate-400' : 'text-slate-600'}>
                                  <th className="p-1 pr-2">#</th>
                                  <th className="p-1 pr-2">State</th>
                                  <th className="p-1 pr-2">Persona</th>
                                  <th className="p-1 pr-2">Source</th>
                                  <th className="p-1 min-w-[120px]">Context</th>
                                  <th className="p-1 pr-2">Project</th>
                                  <th className="p-1 pr-2">Workflow</th>
                                  <th className="p-1 text-right"> </th>
                                </tr>
                              </thead>
                              <tbody>
                                {(queueLane?.items ?? []).map((it) => (
                                  <tr key={it.run_id} className="border-t border-[color:var(--gray-6)] align-top">
                                    <td className="p-1 pr-2 font-mono text-xs">{it.position}</td>
                                    <td className="p-1 pr-2">{it.state}</td>
                                    <td className="p-1 pr-2">{it.persona_name}</td>
                                    <td className="p-1 pr-2">{it.source}</td>
                                    <td className="p-1 max-w-[280px] break-words" title={it.label}>{it.label || '—'}</td>
                                    <td className="p-1 pr-2 font-mono text-xs">{it.project_id ?? '—'}</td>
                                    <td className="p-1 pr-2 font-mono text-xs">{it.workflow_id ?? '—'}</td>
                                    <td className="p-1 text-right">
                                      <Button
                                        size="1"
                                        variant="soft"
                                        color="red"
                                        onClick={() => void cancelQueueRun(it.run_id)}
                                      >
                                        Stop
                                      </Button>
                                    </td>
                                  </tr>
                                ))}
                              </tbody>
                            </table>
                            <div className="flex flex-col gap-2 md:hidden">
                              {(queueLane?.items ?? []).map((it) => (
                                <div
                                  key={it.run_id}
                                  className={
                                    appearance === 'dark'
                                      ? 'rounded-lg border border-[color:var(--mc-border-soft)] p-3 text-sm'
                                      : 'rounded-lg border border-slate-200 p-3 text-sm'
                                  }
                                >
                                  <Flex justify="between" align="start" gap="2" mb="2">
                                    <Text size="2" weight="bold">
                                      #{it.position} · {it.state}
                                    </Text>
                                    <Button
                                      size="1"
                                      variant="soft"
                                      color="red"
                                      onClick={() => void cancelQueueRun(it.run_id)}
                                    >
                                      Stop
                                    </Button>
                                  </Flex>
                                  <Text size="1" color="gray" className="mb-1 block">
                                    {it.persona_name} · {it.source}
                                  </Text>
                                  <Text size="1" className="break-words">
                                    {it.label || '—'}
                                  </Text>
                                  <Text size="1" color="gray" className="mt-1 block font-mono">
                                    project {it.project_id ?? '—'} · workflow {it.workflow_id ?? '—'}
                                  </Text>
                                </div>
                              ))}
                            </div>
                          </>
                        )}
                      </div>
                      <Flex justify="end" mt="3">
                        <Dialog.Close>
                          <Button variant="soft">Close</Button>
                        </Dialog.Close>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>

                  <Dialog.Root
                    open={schedulesDialogOpen}
                    onOpenChange={(open) => setSchedulesDialogOpen(open)}
                  >
                    <Dialog.Trigger className="!hidden md:!inline-flex">
                      <Button size="1" variant="soft">
                        Schedules
                      </Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 820 }}>
                      <Dialog.Title>Schedules</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        Create and manage scheduled prompts for this chat.
                      </Dialog.Description>

                      <Flex align="center" justify="between" gap="3" mb="3" wrap="wrap">
                        <Text size="2" weight="medium">
                          Active schedules
                        </Text>
                        <label htmlFor="sched-archived" className="flex cursor-pointer items-center gap-2">
                          <Text size="1" color="gray">
                            Show completed / cancelled
                          </Text>
                          <Switch
                            id="sched-archived"
                            checked={schedulesShowArchived}
                            onCheckedChange={setSchedulesShowArchived}
                          />
                        </label>
                      </Flex>

                      <div className="rounded-lg border p-3" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' } : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}>
                        <ul className="mb-3 list-none space-y-3">
                          {schedulesFiltered.length === 0 ? (
                            <li
                              className="rounded-lg border border-dashed px-4 py-10 text-center"
                              style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}
                            >
                              <Text size="2" color="gray">
                                {schedules.length === 0
                                  ? 'No schedules yet. Add one below.'
                                  : 'No active schedules. Enable “Show completed / cancelled” to see finished runs.'}
                              </Text>
                            </li>
                          ) : null}
                          {schedulesFiltered.map((t) => (
                            <li key={t.id} className="flex flex-wrap items-center gap-2 rounded-lg border p-2" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}>
                              <span className="min-w-0 flex-1 truncate" title={t.prompt}>{t.prompt}</span>
                              <Select.Root
                                value={String(t.persona_id)}
                                onValueChange={(v) => void updateSchedule(t.id, { persona_id: Number(v) })}
                              >
                                <Select.Trigger className="w-[120px]" />
                                <Select.Content>
                                  {personas.map((p) => (
                                    <Select.Item key={p.id} value={String(p.id)}>
                                      {p.name}
                                    </Select.Item>
                                  ))}
                                </Select.Content>
                              </Select.Root>
                              <Text size="1" color="gray">{t.schedule_type} · {t.next_run ?? '—'}</Text>
                              <Text size="1" color={
                                t.status === 'active' || t.status === 'running' ? 'green' :
                                  t.status === 'paused' ? 'orange' :
                                    t.status === 'completed' ? 'gray' :
                                      t.status === 'cancelled' ? 'red' : 'gray'
                              }>
                                {t.status === 'running' ? 'active' : t.status}
                              </Text>
                              <Button
                                size="1"
                                variant="soft"
                                onClick={() => {
                                  setScheduleDetailTask(t)
                                  setScheduleDetailPrompt(t.prompt)
                                  setScheduleDetailScheduleType(t.schedule_type === 'once' ? 'once' : 'cron')
                                  setScheduleDetailScheduleValue(t.schedule_value)
                                }}
                              >
                                Details
                              </Button>
                              {t.status === 'active' ? (
                                <Button size="1" variant="soft" onClick={() => void updateSchedule(t.id, { status: 'paused' })}>Pause</Button>
                              ) : t.status === 'paused' ? (
                                <Button size="1" variant="soft" onClick={() => void updateSchedule(t.id, { status: 'active' })}>Resume</Button>
                              ) : null}
                              {t.status !== 'cancelled' ? (
                                <Button size="1" variant="soft" color="red" onClick={() => void updateSchedule(t.id, { status: 'cancelled' })}>Cancel</Button>
                              ) : null}
                            </li>
                          ))}
                        </ul>

                        <Flex gap="2" align="end" wrap="wrap">
                          <TextField.Root
                            placeholder="Prompt"
                            value={newSchedulePrompt}
                            onChange={(e) => setNewSchedulePrompt(e.target.value)}
                            className="min-w-[220px]"
                          />
                          <Select.Root value={newScheduleType} onValueChange={(v) => setNewScheduleType(v as 'cron' | 'once')}>
                            <Select.Trigger className="w-[100px]" />
                            <Select.Content>
                              <Select.Item value="cron">Cron</Select.Item>
                              <Select.Item value="once">Once</Select.Item>
                            </Select.Content>
                          </Select.Root>
                          <TextField.Root
                            placeholder={newScheduleType === 'cron' ? '0 9 * * *' : '2025-12-31T09:00:00Z'}
                            value={newScheduleValue}
                            onChange={(e) => setNewScheduleValue(e.target.value)}
                            className="min-w-[200px]"
                          />
                          <Select.Root
                            value={newSchedulePersonaId != null ? String(newSchedulePersonaId) : ''}
                            onValueChange={(v) => setNewSchedulePersonaId(Number(v))}
                          >
                            <Select.Trigger className="w-[140px]" placeholder="Persona" />
                            <Select.Content>
                              {personas.map((p) => (
                                <Select.Item key={p.id} value={String(p.id)}>
                                  {p.name}
                                </Select.Item>
                              ))}
                            </Select.Content>
                          </Select.Root>
                          <Button
                            size="1"
                            onClick={() => {
                              if (newSchedulePrompt.trim()) {
                                void createSchedule(
                                  newSchedulePrompt.trim(),
                                  newScheduleType,
                                  newScheduleValue,
                                  newSchedulePersonaId ?? activePersonaId,
                                )
                                setNewSchedulePrompt('')
                              }
                            }}
                          >
                            Add
                          </Button>
                        </Flex>
                      </div>

                      <Flex justify="end" mt="4" gap="2">
                        <Dialog.Close>
                          <Button variant="soft">Close</Button>
                        </Dialog.Close>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>

                  <Dialog.Root
                    open={scheduleDetailTask != null}
                    onOpenChange={(o) => {
                      if (!o) {
                        setScheduleDetailTask(null)
                        setScheduleDetailBusy(false)
                        setScheduleDetailScheduleValue('')
                      }
                    }}
                  >
                    <Dialog.Content style={{ maxWidth: 720 }}>
                      <Dialog.Title>
                        {scheduleDetailTask != null ? `Schedule #${scheduleDetailTask.id}` : 'Schedule'}
                      </Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        View metadata, edit the prompt, or change the cron/once expression (server runs the same preflight as new schedules).
                      </Dialog.Description>
                      {scheduleDetailTask != null ? (
                        <>
                          <div className="mb-3 grid grid-cols-[120px_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm">
                            <Text size="2" color="gray" className="block">Persona</Text>
                            <Text size="2" className="block">
                              {personas.find((p) => p.id === scheduleDetailTask.persona_id)?.name ?? scheduleDetailTask.persona_id}
                            </Text>
                            <Text size="2" color="gray" className="block">Type</Text>
                            <Text size="2" className="block">{scheduleDetailTask.schedule_type}</Text>
                            <Text size="2" color="gray" className="block">Schedule</Text>
                            <Text size="2" className="block break-all">{scheduleDetailTask.schedule_value}</Text>
                            <Text size="2" color="gray" className="block">Next run</Text>
                            <Text size="2" className="block break-all">{scheduleDetailTask.next_run ?? '—'}</Text>
                            <Text size="2" color="gray" className="block">Last run</Text>
                            <Text size="2" className="block break-all">{scheduleDetailTask.last_run ?? '—'}</Text>
                            <Text size="2" color="gray" className="block">Status</Text>
                            <Text size="2" className="block">{scheduleDetailTask.status}</Text>
                            <Text size="2" color="gray" className="block">Created</Text>
                            <Text size="2" className="block break-all">{scheduleDetailTask.created_at ?? '—'}</Text>
                          </div>
                          <Text size="2" weight="bold" mb="1">Prompt</Text>
                          <textarea
                            value={scheduleDetailPrompt}
                            onChange={(e) => setScheduleDetailPrompt(e.target.value)}
                            spellCheck={false}
                            disabled={scheduleDetailTask.status === 'cancelled'}
                            className={appearance === 'dark'
                              ? 'min-h-[160px] w-full rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3 font-mono text-xs text-slate-100'
                              : 'min-h-[160px] w-full rounded-md border border-slate-300 bg-white p-3 font-mono text-xs text-slate-900'}
                          />
                          <Text size="2" weight="bold" mb="1" mt="3">Schedule</Text>
                          <Flex gap="2" align="center" wrap="wrap" mb="2">
                            <Select.Root
                              value={scheduleDetailScheduleType}
                              onValueChange={(v) => setScheduleDetailScheduleType(v as 'cron' | 'once')}
                              disabled={scheduleDetailTask.status === 'cancelled'}
                            >
                              <Select.Trigger className="w-[100px]" />
                              <Select.Content>
                                <Select.Item value="cron">Cron</Select.Item>
                                <Select.Item value="once">Once</Select.Item>
                              </Select.Content>
                            </Select.Root>
                            <input
                              type="text"
                              value={scheduleDetailScheduleValue}
                              onChange={(e) => setScheduleDetailScheduleValue(e.target.value)}
                              spellCheck={false}
                              disabled={scheduleDetailTask.status === 'cancelled'}
                              placeholder={scheduleDetailScheduleType === 'cron' ? '0 9 * * * *' : '2099-12-31T23:59:59+00:00'}
                              className={appearance === 'dark'
                                ? 'min-w-[200px] flex-1 rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] px-2 py-1 font-mono text-xs text-slate-100'
                                : 'min-w-[200px] flex-1 rounded-md border border-slate-300 bg-white px-2 py-1 font-mono text-xs text-slate-900'}
                            />
                          </Flex>
                          <Flex justify="end" gap="2" mt="3" wrap="wrap">
                            <Dialog.Close>
                              <Button variant="soft" size="1">Close</Button>
                            </Dialog.Close>
                            <Button
                              size="1"
                              disabled={
                                scheduleDetailBusy
                                || scheduleDetailTask.status === 'cancelled'
                                || (
                                  scheduleDetailScheduleType === (scheduleDetailTask.schedule_type === 'once' ? 'once' : 'cron')
                                  && scheduleDetailScheduleValue.trim() === scheduleDetailTask.schedule_value.trim()
                                )
                                || scheduleDetailScheduleValue.trim().length === 0
                              }
                              onClick={() => {
                                if (scheduleDetailTask == null) return
                                setScheduleDetailBusy(true)
                                updateSchedule(scheduleDetailTask.id, {
                                  schedule_type: scheduleDetailScheduleType,
                                  schedule_value: scheduleDetailScheduleValue.trim(),
                                })
                                  .then(() => setScheduleDetailTask(null))
                                  .catch(() => { /* api throws */ })
                                  .finally(() => setScheduleDetailBusy(false))
                              }}
                            >
                              {scheduleDetailBusy ? 'Saving…' : 'Save schedule'}
                            </Button>
                            <Button
                              size="1"
                              disabled={
                                scheduleDetailBusy
                                || scheduleDetailTask.status === 'cancelled'
                                || scheduleDetailPrompt.trim() === scheduleDetailTask.prompt.trim()
                                || scheduleDetailPrompt.trim().length === 0
                              }
                              onClick={() => {
                                if (scheduleDetailTask == null) return
                                setScheduleDetailBusy(true)
                                updateSchedule(scheduleDetailTask.id, { prompt: scheduleDetailPrompt.trim() })
                                  .then(() => setScheduleDetailTask(null))
                                  .catch(() => { /* api throws */ })
                                  .finally(() => setScheduleDetailBusy(false))
                              }}
                            >
                              {scheduleDetailBusy ? 'Saving…' : 'Save prompt'}
                            </Button>
                          </Flex>
                        </>
                      ) : null}
                    </Dialog.Content>
                  </Dialog.Root>

                  <Dialog.Root
                    open={agentsMdOpen}
                    onOpenChange={(o) => {
                      setAgentsMdOpen(o)
                      if (!o) {
                        setAgentsMdError('')
                        setAgentsMdBusy(false)
                      }
                    }}
                  >
                    <Dialog.Trigger className="!hidden md:!inline-flex">
                      <Button size="1" variant="soft">
                        Principles
                      </Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 900 }}>
                      <Dialog.Title>Workspace principles (AGENTS.md)</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        Shared agent principles for this workspace. Same file the bot loads from your configured workspace path.
                      </Dialog.Description>
                      {agentsMdPath ? (
                        <Text size="1" color="gray" className="mb-2 block break-all">
                          {agentsMdPath}
                        </Text>
                      ) : null}
                      {agentsMdError ? (
                        <Callout.Root color="red" size="1" variant="soft" className="mb-2">
                          <Callout.Text>{agentsMdError}</Callout.Text>
                        </Callout.Root>
                      ) : null}
                      <textarea
                        value={agentsMdContent}
                        onChange={(e) => setAgentsMdContent(e.target.value)}
                        spellCheck={false}
                        className={appearance === 'dark'
                          ? 'h-[420px] w-full rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3 font-mono text-xs text-slate-100'
                          : 'h-[420px] w-full rounded-md border border-slate-300 bg-white p-3 font-mono text-xs text-slate-900'}
                      />
                      <Flex justify="between" align="center" mt="3" wrap="wrap" gap="2">
                        <Text size="1" color="gray">
                          {agentsMdMtimeMs != null ? `mtime: ${agentsMdMtimeMs}` : ''}
                        </Text>
                        <Flex gap="2">
                          <Button size="1" variant="soft" onClick={() => void loadWorkspaceAgentsMd()} disabled={agentsMdBusy}>
                            Reload
                          </Button>
                          <Button size="1" onClick={() => void saveWorkspaceAgentsMd()} disabled={agentsMdBusy}>
                            {agentsMdBusy ? 'Saving…' : 'Save'}
                          </Button>
                          <Dialog.Close>
                            <Button size="1" variant="soft">Close</Button>
                          </Dialog.Close>
                        </Flex>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>

                  <Dialog.Root
                    open={artifactsDialogOpen}
                    onOpenChange={(open) => {
                      setArtifactsDialogOpen(open)
                      if (!open) {
                        setArtifactsError('')
                        setArtifactTextError('')
                      }
                    }}
                  >
                    <Dialog.Trigger className="!hidden md:!inline-flex">
                      <Button size="1" variant="soft">
                        Artifacts
                      </Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 980 }}>
                      <Dialog.Title>Artifacts</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        View files produced or referenced in this chat persona. Attachments stay channel-local; web can preview them here.
                      </Dialog.Description>
                      <Flex gap="3" align="start" wrap="wrap" className="flex-col md:flex-row">
                        <div className="min-w-0 w-full flex-1 md:min-w-[250px]">
                          <Flex justify="between" align="center" mb="2" gap="2" wrap="wrap">
                            <Select.Root value={artifactKindFilter} onValueChange={setArtifactKindFilter}>
                              <Select.Trigger className="w-[150px]" />
                              <Select.Content>
                                <Select.Item value="all">All kinds</Select.Item>
                                <Select.Item value="image">Images</Select.Item>
                                <Select.Item value="markdown">Markdown</Select.Item>
                                <Select.Item value="html">HTML</Select.Item>
                                <Select.Item value="text">Text</Select.Item>
                                <Select.Item value="other">Other</Select.Item>
                              </Select.Content>
                            </Select.Root>
                            <Button size="1" variant="soft" onClick={() => void loadArtifacts(chatId, activePersonaId)} disabled={artifactsBusy}>
                              Refresh
                            </Button>
                          </Flex>
                          <div className={appearance === 'dark'
                            ? 'max-h-[min(440px,65vh)] overflow-auto rounded-md border border-[color:var(--mc-border-soft)]'
                            : 'max-h-[min(440px,65vh)] overflow-auto rounded-md border border-slate-300'
                          }>
                            {artifactsBusy ? (
                              <Text size="2" color="gray" className="block p-2">Loading artifacts...</Text>
                            ) : artifactsError ? (
                              <Callout.Root color="red" size="1" variant="soft" className="m-2">
                                <Callout.Text>{artifactsError}</Callout.Text>
                              </Callout.Root>
                            ) : artifacts.length === 0 ? (
                              <Text size="2" color="gray" className="block p-2">No artifacts found for this persona.</Text>
                            ) : (
                              <ul className="list-none m-0 p-0">
                                {artifacts.map((it) => (
                                  <li key={it.id}>
                                    <button
                                      type="button"
                                      onClick={() => setSelectedArtifactId(it.id)}
                                      className={selectedArtifactId === it.id
                                        ? 'w-full border-0 border-b text-left p-2 bg-[var(--accent-3)]'
                                        : 'w-full border-0 border-b text-left p-2'}
                                      style={appearance === 'dark' ? { borderBottomColor: 'var(--mc-border-soft)' } : { borderBottomColor: 'var(--gray-6)' }}
                                    >
                                      <div className="flex items-center justify-between gap-2">
                                        <Text size="2" className="truncate">{it.name}</Text>
                                        <Text size="1" color="gray">{it.kind}</Text>
                                      </div>
                                      <Text size="1" color="gray">
                                        {formatBytes(it.size_bytes ?? null)} · {it.source}
                                      </Text>
                                    </button>
                                  </li>
                                ))}
                              </ul>
                            )}
                          </div>
                        </div>
                        <div className={appearance === 'dark'
                          ? 'min-h-[200px] min-w-0 w-full flex-[2] rounded-md border border-[color:var(--mc-border-soft)] p-2 md:min-w-[320px]'
                          : 'min-h-[200px] min-w-0 w-full flex-[2] rounded-md border border-slate-300 p-2 md:min-w-[320px]'
                        }>
                          {selectedArtifact == null ? (
                            <Text size="2" color="gray">Select an artifact to preview.</Text>
                          ) : (
                            <>
                              <Flex justify="between" align="center" mb="2" wrap="wrap" gap="2">
                                <div>
                                  <Text size="2" weight="bold">{selectedArtifact.name}</Text>
                                  <Text size="1" color="gray" className="block">
                                    {selectedArtifact.created_at ?? 'unknown time'} · {selectedArtifact.kind}
                                  </Text>
                                </div>
                                <Flex gap="2">
                                  <Button size="1" variant="soft" onClick={() => window.open(selectedArtifact.url, '_blank', 'noopener,noreferrer')}>
                                    Open
                                  </Button>
                                  <Button size="1" variant="soft" onClick={() => window.open(`${selectedArtifact.url}${selectedArtifact.url.includes('?') ? '&' : '?'}download=1`, '_blank', 'noopener,noreferrer')}>
                                    Download
                                  </Button>
                                </Flex>
                              </Flex>
                              {selectedArtifact.kind === 'image' ? (
                                <img src={artifactPreviewUrl(selectedArtifact)} alt={selectedArtifact.name} className="max-h-[56vh] w-full object-contain" />
                              ) : selectedArtifact.kind === 'markdown' ? (
                                artifactTextBusy ? (
                                  <Text size="2" color="gray">Loading preview...</Text>
                                ) : artifactTextError ? (
                                  <Callout.Root color="red" size="1" variant="soft">
                                    <Callout.Text>{artifactTextError}</Callout.Text>
                                  </Callout.Root>
                                ) : (
                                  <div className="aui-md-root max-h-[56vh] overflow-auto text-sm leading-relaxed">
                                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                      {artifactTextPreview}
                                    </ReactMarkdown>
                                  </div>
                                )
                              ) : selectedArtifact.kind === 'html' ? (
                                <iframe
                                  title={selectedArtifact.name}
                                  src={artifactPreviewUrl(selectedArtifact)}
                                  sandbox="allow-same-origin"
                                  className="h-[56vh] w-full rounded border border-[color:var(--gray-6)]"
                                />
                              ) : selectedArtifact.kind === 'text' ? (
                                artifactTextBusy ? (
                                  <Text size="2" color="gray">Loading preview...</Text>
                                ) : artifactTextError ? (
                                  <Callout.Root color="red" size="1" variant="soft">
                                    <Callout.Text>{artifactTextError}</Callout.Text>
                                  </Callout.Root>
                                ) : (
                                  <pre className="max-h-[56vh] overflow-auto whitespace-pre-wrap text-xs">{artifactTextPreview}</pre>
                                )
                              ) : (
                                <Text size="2" color="gray">
                                  Preview unavailable for this file type. Use Open or Download.
                                </Text>
                              )}
                            </>
                          )}
                        </div>
                      </Flex>
                      <Flex justify="end" mt="3">
                        <Dialog.Close>
                          <Button variant="soft">Close</Button>
                        </Dialog.Close>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>

                  <Dialog.Root
                    open={memoryDialogOpen}
                    onOpenChange={(open) => {
                      setMemoryDialogOpen(open)
                      if (!open) {
                        setMemoryError('')
                        setMemoryBusy(false)
                      }
                    }}
                  >
                    <Dialog.Trigger className="!hidden md:!inline-flex">
                      <Button size="1" variant="soft">
                        Memory
                      </Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 900 }}>
                      <Dialog.Title>Persona memory</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        Edit this persona’s tiered memory file. Memory is context, not a task queue.
                      </Dialog.Description>

                      {memoryPathHint ? (
                        <Text size="1" color="gray" className="mb-2 block">
                          {memoryPathHint}
                        </Text>
                      ) : null}

                      {memoryError ? (
                        <Callout.Root color="red" size="1" variant="soft" className="mb-2">
                          <Callout.Text>{memoryError}</Callout.Text>
                        </Callout.Root>
                      ) : null}

                      <textarea
                        value={memoryContent}
                        onChange={(e) => setMemoryContent(e.target.value)}
                        spellCheck={false}
                        className={appearance === 'dark'
                          ? 'h-[420px] w-full rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3 font-mono text-xs text-slate-100'
                          : 'h-[420px] w-full rounded-md border border-slate-300 bg-white p-3 font-mono text-xs text-slate-900'}
                      />

                      <Flex justify="between" align="center" mt="3" wrap="wrap" gap="2">
                        <Text size="1" color="gray">
                          {memoryMtimeMs != null ? `mtime: ${memoryMtimeMs}` : ''}
                        </Text>
                        <Flex gap="2">
                          <Button
                            size="1"
                            variant="soft"
                            onClick={() => {
                              if (activePersonaId != null) void loadPersonaMemory(activePersonaId)
                            }}
                            disabled={memoryBusy || activePersonaId == null}
                          >
                            Reload
                          </Button>
                          <Button
                            size="1"
                            onClick={() => {
                              if (activePersonaId != null) void savePersonaMemory(activePersonaId)
                            }}
                            disabled={memoryBusy || activePersonaId == null}
                          >
                            {memoryBusy ? 'Saving…' : 'Save'}
                          </Button>
                          <Dialog.Close>
                            <Button size="1" variant="soft">Close</Button>
                          </Dialog.Close>
                        </Flex>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>

                  <Dialog.Root
                    open={agentHistoryDialogOpen}
                    onOpenChange={(open) => {
                      setAgentHistoryDialogOpen(open)
                      if (!open) {
                        setAgentHistoryError('')
                        setAgentHistoryBusy(false)
                      }
                    }}
                  >
                    <Dialog.Trigger className="!hidden md:!inline-flex">
                      <Button size="1" variant="soft" disabled={activePersonaId == null}>
                        Last agent run
                      </Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 900 }}>
                      <Dialog.Title>Last agent run</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        Latest saved trace for this persona (iterations and tool calls). Use Prev/Next or arrow keys to step through iterations.
                      </Dialog.Description>

                      {agentHistoryPathHint ? (
                        <Text size="1" color="gray" className="mb-1 block">
                          {agentHistoryPathHint}
                        </Text>
                      ) : null}
                      {agentHistoryFilename ? (
                        <Text size="1" color="gray" className="mb-2 block">
                          File: {agentHistoryFilename}
                          {agentHistoryMtimeMs != null ? ` · mtime: ${agentHistoryMtimeMs}` : ''}
                        </Text>
                      ) : null}

                      {agentHistoryBusy ? (
                        <Text size="2" color="gray" mb="2">
                          Loading…
                        </Text>
                      ) : null}

                      {agentHistoryError ? (
                        <Callout.Root color="orange" size="1" variant="soft" className="mb-2">
                          <Callout.Text>{agentHistoryError}</Callout.Text>
                        </Callout.Root>
                      ) : null}

                      {!agentHistoryBusy && !agentHistoryError && agentHistoryParsed != null ? (
                        <>
                          {agentHistoryParsed.runHeader.trim() ? (
                            <div
                              className={
                                appearance === 'dark'
                                  ? 'mb-3 max-h-32 overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-2'
                                  : 'mb-3 max-h-32 overflow-auto rounded-md border border-slate-300 bg-slate-50 p-2'
                              }
                            >
                              <AgentHistoryMarkdownBody markdown={agentHistoryParsed.runHeader} />
                            </div>
                          ) : null}

                          {agentHistoryParsed.iterations.length > 0 ? (
                            <>
                              <Flex justify="between" align="center" mb="2" wrap="wrap" gap="2">
                                <Text size="2">
                                  Iteration {agentHistoryIterationIdx + 1} of {agentHistoryParsed.iterations.length}
                                </Text>
                                <Flex gap="2">
                                  <Button
                                    size="1"
                                    variant="soft"
                                    disabled={agentHistoryIterationIdx <= 0}
                                    onClick={() =>
                                      setAgentHistoryIterationIdx((i) => Math.max(0, i - 1))
                                    }
                                  >
                                    Prev
                                  </Button>
                                  <Button
                                    size="1"
                                    variant="soft"
                                    disabled={
                                      agentHistoryIterationIdx >= agentHistoryParsed.iterations.length - 1
                                    }
                                    onClick={() =>
                                      setAgentHistoryIterationIdx((i) =>
                                        Math.min(agentHistoryParsed.iterations.length - 1, i + 1),
                                      )
                                    }
                                  >
                                    Next
                                  </Button>
                                </Flex>
                              </Flex>
                              <Text size="1" color="gray" mb="2" className="block">
                                Keyboard: ← →
                              </Text>
                              <div
                                className={
                                  appearance === 'dark'
                                    ? 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3'
                                    : 'max-h-[420px] overflow-auto rounded-md border border-slate-300 bg-white p-3'
                                }
                              >
                                <AgentHistoryMarkdownBody
                                  markdown={
                                    agentHistoryParsed.iterations[agentHistoryIterationIdx]?.body ?? ''
                                  }
                                />
                              </div>
                            </>
                          ) : (
                            <div
                              className={
                                appearance === 'dark'
                                  ? 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3'
                                  : 'max-h-[420px] overflow-auto rounded-md border border-slate-300 bg-white p-3'
                              }
                            >
                              <AgentHistoryMarkdownBody markdown={agentHistoryRaw} />
                            </div>
                          )}
                        </>
                      ) : null}

                      <Flex justify="end" mt="3" gap="2">
                        <Button
                          size="1"
                          variant="soft"
                          onClick={() => {
                            if (activePersonaId != null) void loadAgentHistoryLatest(activePersonaId)
                          }}
                          disabled={agentHistoryBusy || activePersonaId == null}
                        >
                          Reload
                        </Button>
                        <Dialog.Close>
                          <Button size="1" variant="soft">Close</Button>
                        </Dialog.Close>
                      </Flex>
                    </Dialog.Content>
                  </Dialog.Root>
                </Flex>
              </Flex>
              </div>
            </header>

            <div
              className={
                appearance === 'dark'
                  ? 'relative flex min-h-0 min-w-0 flex-1 flex-col bg-[linear-gradient(to_bottom,var(--mc-bg-panel),var(--mc-bg-main)_28%)]'
                  : 'relative flex min-h-0 min-w-0 flex-1 flex-col bg-[linear-gradient(to_bottom,#f8fafc,white_20%)]'
              }
            >
              <div className="pointer-events-none absolute left-0 right-0 top-2 z-20 flex justify-center px-2">
                <div className="pointer-events-auto w-full max-w-5xl">
                  <CockpitBar
                    appearance={appearance}
                    statusText={statusText}
                    queueLane={queueLane}
                    backgroundActiveCount={backgroundActiveCount}
                    installationStatus={installationStatus}
                    onQueueClick={() => setQueueDialogOpen(true)}
                    bulletinUpdates={bulletinUpdates}
                    bookmarks={personaBookmarks}
                    activePersonaId={activePersonaId}
                    floating
                  />
                </div>
              </div>
              <div className="mx-auto w-full max-w-5xl px-3 pt-14">
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
                  <Callout.Root color="red" size="1" variant="soft" className={replayNotice ? 'mt-2' : ''}>
                    <Callout.Text>{error}</Callout.Text>
                  </Callout.Root>
                ) : null}
              </div>

              <div className="flex min-h-0 min-w-0 flex-1 flex-col px-1 pb-1">
                <div className="min-h-0 min-w-0 flex-1">
                  <ThreadPane
                    key={runtimeKey}
                    adapter={adapter}
                    initialMessages={historySeed}
                    runtimeKey={runtimeKey}
                    bookmarkedMessageIds={bookmarkedMessageIds}
                    onToggleBookmark={toggleMessageBookmark}
                  />
                </div>
              </div>
            </div>
          </main>
        </div>

      </div>
    </Theme>
  )
}

createRoot(document.getElementById('root')!).render(
  <QueryClientProvider client={queryClient}>
    <App />
  </QueryClientProvider>,
)
