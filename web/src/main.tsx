import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import {
  AssistantRuntimeProvider,
  CompositeAttachmentAdapter,
  MessagePrimitive,
  SimpleImageAttachmentAdapter,
  SimpleTextAttachmentAdapter,
  useMessage,
  useLocalRuntime,
  type AttachmentAdapter,
  type ChatModelAdapter,
  type ChatModelRunOptions,
  type ChatModelRunResult,
  type CompleteAttachment,
  type PendingAttachment,
  type ThreadMessageLike,
  type ToolCallMessagePartProps,
} from '@assistant-ui/react'
import {
  AssistantActionBar,
  AssistantMessage,
  BranchPicker,
  Thread,
  UserActionBar,
  UserMessage,
  makeMarkdownText,
} from '@assistant-ui/react-ui'
import {
  Button,
  Callout,
  Dialog,
  Flex,
  Heading,
  Select,
  Text,
  TextField,
  Theme,
} from '@radix-ui/themes'
import remarkGfm from 'remark-gfm'
import '@radix-ui/themes/styles.css'
import '@assistant-ui/react-ui/styles/index.css'
import './styles.css'
import { SessionSidebar } from './components/session-sidebar'
import type { Persona, ScheduleTask, ChannelBinding } from './types'

type BackendMessage = {
  id?: string
  sender_name?: string
  content?: string
  is_from_bot?: boolean
  timestamp?: string
}

type QueueLane = {
  chat_id: number
  pending: number
  active_for_ms: number
  oldest_wait_ms: number
  last_error?: string | null
  project_id?: number | null
  workflow_id?: number | null
}

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

const WEB_AUTH_STORAGE_KEY = 'web_auth_token'

function sanitizeHttpHeaderValue(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  if (trimmed.includes('\r') || trimmed.includes('\n')) return null
  for (let i = 0; i < trimmed.length; i += 1) {
    const code = trimmed.charCodeAt(i)
    // Browser header values must be ISO-8859-1 representable.
    if (code > 0xff) return null
  }
  return trimmed
}

function getStoredAuthToken(): string | null {
  if (typeof sessionStorage === 'undefined') return null
  try {
    const t = sessionStorage.getItem(WEB_AUTH_STORAGE_KEY)
    if (!t) return null
    const sanitized = sanitizeHttpHeaderValue(t)
    if (!sanitized) {
      sessionStorage.removeItem(WEB_AUTH_STORAGE_KEY)
      return null
    }
    return sanitized
  } catch {
    return null
  }
}

function makeHeaders(options: RequestInit = {}): HeadersInit {
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string> | undefined),
  }
  for (const [key, value] of Object.entries(headers)) {
    if (typeof value !== 'string') {
      delete headers[key]
      continue
    }
    const sanitized = sanitizeHttpHeaderValue(value)
    if (!sanitized) {
      delete headers[key]
      continue
    }
    headers[key] = sanitized
  }
  const token = getStoredAuthToken()
  if (token) {
    headers['Authorization'] = `Bearer ${token}`
  }
  if (options.body && !headers['Content-Type']) {
    headers['Content-Type'] = 'application/json'
  }
  return headers
}

export const AUTH_REQUIRED_EVENT = 'web-auth-required'

function messageForFailedResponse(status: number, data: Record<string, unknown>, bodyText?: string): string {
  if (status === 401) {
    return 'Unauthorized. Enter the API token (WEB_AUTH_TOKEN from .env).'
  }
  if (status === 429) {
    const serverMsg = String(data.error || data.message || bodyText || '').trim()
    return serverMsg
      ? `Too many requests: ${serverMsg} Please wait a moment before sending again.`
      : 'Too many requests. Please wait a moment before sending again.'
  }
  return String(data.error || data.message || bodyText || `HTTP ${status}`)
}

async function api<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const res = await fetch(path, { ...options, headers: makeHeaders(options) })
  const bodyText = await res.text()
  let data: Record<string, unknown> = {}
  try {
    data = bodyText ? (JSON.parse(bodyText) as Record<string, unknown>) : {}
  } catch {
    data = { message: bodyText || undefined }
  }
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent(AUTH_REQUIRED_EVENT))
    throw new Error(messageForFailedResponse(401, data, bodyText))
  }
  if (!res.ok) {
    throw new Error(messageForFailedResponse(res.status, data, bodyText))
  }
  return data as T
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

    const content = message.content
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

/** Catch-all for PDFs, archives, and other types not covered by image/text adapters. Keeps `file` on the attachment for upload extraction. */
class WebWildcardAttachmentAdapter implements AttachmentAdapter {
  readonly accept = '*'

  async add(state: { file: File }): Promise<PendingAttachment> {
    return {
      id: `${state.file.name}-${state.file.size}-${state.file.lastModified}`,
      type: 'document',
      name: state.file.name,
      contentType: state.file.type,
      file: state.file,
      status: { type: 'requires-action', reason: 'composer-send' },
    }
  }

  async send(attachment: PendingAttachment): Promise<CompleteAttachment> {
    return {
      ...attachment,
      status: { type: 'complete' },
      content: [{ type: 'text', text: '' }],
    }
  }

  async remove(): Promise<void> {
    // noop
  }
}

const webAttachmentAdapter = new CompositeAttachmentAdapter([
  new SimpleImageAttachmentAdapter(),
  new SimpleTextAttachmentAdapter(),
  new WebWildcardAttachmentAdapter(),
])

function mapBackendHistory(messages: BackendMessage[]): ThreadMessageLike[] {
  return messages.map((item, index) => ({
    id: item.id || `history-${index}`,
    role: item.is_from_bot ? 'assistant' : 'user',
    content: item.content || '',
    createdAt: item.timestamp ? new Date(item.timestamp) : new Date(),
  }))
}

/** Compare history for sync/remount decisions: id, role, content only — ignore `createdAt` (server timestamps can jitter between polls). */
function historiesEqual(a: ThreadMessageLike[], b: ThreadMessageLike[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i += 1) {
    const x = a[i]
    const y = b[i]
    if (x.id !== y.id) return false
    if (x.role !== y.role) return false
    if (x.content !== y.content) return false
  }
  return true
}

function shouldDeferHistoryRemount(): boolean {
  if (typeof document === 'undefined') return false
  const inComposer = Boolean(document.activeElement?.closest?.('.aui-composer-root'))
  const vp = document.querySelector('.aui-thread-viewport')
  if (!vp) return inComposer
  const el = vp as HTMLElement
  const gap = el.scrollHeight - el.scrollTop - el.clientHeight
  const scrolledAwayFromBottom = gap > 100
  return inComposer || scrolledAwayFromBottom
}

function asObject(value: unknown): Record<string, unknown> {
  if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
    return value as Record<string, unknown>
  }
  return {}
}

function formatUnknown(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function ToolCallCard(props: ToolCallMessagePartProps) {
  const result = asObject(props.result)
  const hasResult = Object.keys(result).length > 0
  const output = result.output
  const duration = result.duration_ms
  const bytes = result.bytes
  const statusCode = result.status_code
  const errorType = result.error_type

  return (
    <div className="tool-card">
      <div className="tool-card-head">
        <span className="tool-card-name">{props.toolName}</span>
        <span className={`tool-card-state ${hasResult ? (props.isError ? 'error' : 'ok') : 'running'}`}>
          {hasResult ? (props.isError ? 'error' : 'done') : 'running'}
        </span>
      </div>
      {Object.keys(props.args || {}).length > 0 ? (
        <pre className="tool-card-pre">{JSON.stringify(props.args, null, 2)}</pre>
      ) : null}
      {hasResult ? (
        <div className="tool-card-meta">
          {typeof duration === 'number' ? <span>{duration}ms</span> : null}
          {typeof bytes === 'number' ? <span>{bytes}b</span> : null}
          {typeof statusCode === 'number' ? <span>HTTP {statusCode}</span> : null}
          {typeof errorType === 'string' && errorType ? <span>{errorType}</span> : null}
        </div>
      ) : null}
      {output !== undefined ? <pre className="tool-card-pre">{formatUnknown(output)}</pre> : null}
    </div>
  )
}

function MessageTimestamp({ align }: { align: 'left' | 'right' }) {
  const createdAt = useMessage((m) => m.createdAt)
  const formatted = createdAt ? createdAt.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : ''
  return (
    <div className={align === 'right' ? 'mc-msg-time mc-msg-time-right' : 'mc-msg-time'}>
      {formatted}
    </div>
  )
}

function CustomAssistantMessage() {
  const hasRenderableContent = useMessage((m) =>
    Array.isArray(m.content)
      ? m.content.some((part) => {
        if (part.type === 'text') return Boolean(part.text?.trim())
        return part.type === 'tool-call'
      })
      : false,
  )

  return (
    <AssistantMessage.Root>
      <AssistantMessage.Avatar />
      {hasRenderableContent ? (
        <AssistantMessage.Content />
      ) : (
        <div className="mc-assistant-placeholder" aria-live="polite">
          <span className="mc-assistant-placeholder-dot" />
          <span className="mc-assistant-placeholder-dot" />
          <span className="mc-assistant-placeholder-dot" />
          <span className="mc-assistant-placeholder-text">Thinking</span>
        </div>
      )}
      <BranchPicker />
      <AssistantActionBar />
      <MessageTimestamp align="left" />
    </AssistantMessage.Root>
  )
}

function CustomUserMessage() {
  return (
    <UserMessage.Root>
      <UserMessage.Attachments />
      <MessagePrimitive.If hasContent>
        <UserActionBar />
        <div className="mc-user-content-wrap">
          <UserMessage.Content />
          <MessageTimestamp align="right" />
        </div>
      </MessagePrimitive.If>
      <BranchPicker />
    </UserMessage.Root>
  )
}

type ThreadPaneProps = {
  adapter: ChatModelAdapter
  initialMessages: ThreadMessageLike[]
  runtimeKey: string
}

function ThreadPane({ adapter, initialMessages, runtimeKey }: ThreadPaneProps) {
  const MarkdownText = makeMarkdownText({
    remarkPlugins: [remarkGfm],
    components: {
      table: ({ className, ...props }) => (
        <div className="mc-md-table-scroll">
          <table className={['aui-md-table', className].filter(Boolean).join(' ')} {...props} />
        </div>
      ),
    },
  })
  const runtime = useLocalRuntime(adapter, {
    initialMessages,
    maxSteps: 100,
    adapters: {
      attachments: webAttachmentAdapter,
    },
  })

  return (
    <AssistantRuntimeProvider key={runtimeKey} runtime={runtime}>
      <div className="aui-root h-full min-h-0">
        <Thread
          assistantMessage={{
            allowCopy: true,
            allowReload: false,
            allowSpeak: false,
            allowFeedbackNegative: false,
            allowFeedbackPositive: false,
            components: {
              Text: MarkdownText,
              ToolFallback: ToolCallCard,
            },
          }}
          userMessage={{ allowEdit: false }}
          composer={{ allowAttachments: true }}
          components={{
            AssistantMessage: CustomAssistantMessage,
            UserMessage: CustomUserMessage,
          }}
          strings={{
            composer: {
              input: { placeholder: 'Message FinallyAValueBot...' },
            },
          }}
          assistantAvatar={{ fallback: 'M' }}
        />
      </div>
    </AssistantRuntimeProvider>
  )
}

function App() {
  const [appearance, setAppearance] = useState<Appearance>(readAppearance())
  const [uiTheme, setUiTheme] = useState<UiTheme>(readUiTheme())
  const [chatId, setChatId] = useState<number | null>(null)
  const [historySeed, setHistorySeed] = useState<ThreadMessageLike[]>([])
  const [historyByDay, setHistoryByDay] = useState<Record<string, ThreadMessageLike[]>>({})
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [runtimeNonce, setRuntimeNonce] = useState<number>(0)
  const [error, setError] = useState<string>('')
  const [statusText, setStatusText] = useState<string>('Idle')
  const [replayNotice, setReplayNotice] = useState<string>('')
  const [authRequired, setAuthRequired] = useState<boolean>(false)
  const [authTokenInput, setAuthTokenInput] = useState<string>('')
  const [personas, setPersonas] = useState<Persona[]>([])
  const [activePersonaId, setActivePersonaId] = useState<number | null>(null)
  const [schedules, setSchedules] = useState<ScheduleTask[]>([])
  const [schedulesDialogOpen, setSchedulesDialogOpen] = useState<boolean>(false)
  const [memoryDialogOpen, setMemoryDialogOpen] = useState<boolean>(false)
  const [memoryContent, setMemoryContent] = useState<string>('')
  const [memoryMtimeMs, setMemoryMtimeMs] = useState<number | null>(null)
  const [memoryPathHint, setMemoryPathHint] = useState<string>('')
  const [memoryBusy, setMemoryBusy] = useState<boolean>(false)
  const [memoryError, setMemoryError] = useState<string>('')
  const [newSchedulePrompt, setNewSchedulePrompt] = useState('')
  const [newScheduleType, setNewScheduleType] = useState<'cron' | 'once'>('cron')
  const [newScheduleValue, setNewScheduleValue] = useState('0 9 * * *')
  const [newSchedulePersonaId, setNewSchedulePersonaId] = useState<number | null>(null)
  const [bindings, setBindings] = useState<ChannelBinding[]>([])
  const [pendingRunIds, setPendingRunIds] = useState<string[]>([])
  const [queueLane, setQueueLane] = useState<QueueLane | null>(null)
  const [personaReadNonce, setPersonaReadNonce] = useState<number>(0)
  const [historyPollUntilMs, setHistoryPollUntilMs] = useState<number>(0)

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

  async function refreshPersonas(cid: number | null = chatId): Promise<void> {
    if (cid == null) return
    try {
      const query = new URLSearchParams({ chat_id: String(cid) })
      const data = await api<{ personas?: { id: number; name: string; is_active: boolean; last_bot_message_at?: string | null }[] }>(`/api/personas?${query.toString()}`)
      const list = Array.isArray(data.personas) ? data.personas : []
      setPersonas(list.map((p) => ({ id: p.id, name: p.name, is_active: p.is_active, last_bot_message_at: p.last_bot_message_at ?? null })))
    } catch {
      // ignore refresh errors
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

  async function loadOlderDay(): Promise<void> {
    if (chatId == null || loadingOlder) return
    setLoadingOlder(true)
    try {
      const daysRes = await api<{ days?: string[] }>(`/api/history/days?chat_id=${chatId}${activePersonaId ? `&persona_id=${activePersonaId}` : ''}`)
      const allDays = Array.isArray(daysRes.days) ? daysRes.days : []
      if (allDays.length === 0) return
      const loadedDays = Object.keys(historyByDay).sort()
      const oldestLoaded = loadedDays.length > 0
        ? loadedDays[0]
        : (() => {
          const first = historySeed[0] as { createdAt?: Date } | undefined
          if (first?.createdAt) {
            return new Date(first.createdAt).toISOString().slice(0, 10)
          }
          return allDays[0]
        })()
      const idx = allDays.indexOf(oldestLoaded)
      const nextOlder = idx >= 0 && idx < allDays.length - 1 ? allDays[idx + 1] : null
      if (nextOlder) await loadHistory(chatId, activePersonaId ?? undefined, nextOlder, { force: true })
    } finally {
      setLoadingOlder(false)
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
        const data = await api<{ chat_id?: number; persona_id?: number }>('/api/chat')
        const cid = typeof data.chat_id === 'number' ? data.chat_id : null
        const pid = typeof data.persona_id === 'number' ? data.persona_id : null
        setChatId(cid)
        if (pid != null) setActivePersonaId(pid)
        if (cid != null) {
          const chosen = await loadPersonas(cid)
          loadBindings(cid).catch(() => { })
          loadSchedules(cid).catch(() => { })
          loadQueueDiagnostics(cid).catch(() => { })
          await loadHistory(cid, chosen?.id ?? pid, null, { force: true })
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
    if (chatId == null) return
    let cancelled = false
    const activePending = (queueLane?.pending ?? 0) > 0 || pendingRunIds.length > 0
    const intervalMs = activePending ? 2500 : 10000
    const interval = setInterval(() => {
      if (cancelled) return
      loadQueueDiagnostics(chatId).catch(() => { })
      refreshPersonas(chatId).catch(() => { })
    }, intervalMs)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId, pendingRunIds.length, queueLane?.pending])

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

  async function loadQueueDiagnostics(cid: number | null = chatId): Promise<void> {
    if (cid == null) return
    try {
      const data = await api<{ lanes?: QueueLane[] }>('/api/queue_diagnostics')
      const lanes = Array.isArray(data.lanes) ? data.lanes : []
      const lane = lanes.find((l) => l.chat_id === cid) ?? null
      setQueueLane(lane)
    } catch {
      setQueueLane(null)
    }
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
    patch: { status?: string; persona_id?: number },
  ): Promise<void> {
    await api(`/api/schedules/${taskId}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    })
    await loadSchedules(chatId)
  }

  useEffect(() => {
    if (chatId == null) return
    let cancelled = false
    async function init() {
      const chosen = await loadPersonas(chatId)
      if (cancelled) return
      loadBindings(chatId).catch(() => { })
      loadSchedules(chatId).catch(() => { })
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
    }
    init()
    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId])

  useEffect(() => {
    if (activePersonaId != null && activePersonaId > 0) {
      setNewSchedulePersonaId((prev) => (prev == null ? activePersonaId : prev))
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
    <Theme appearance={appearance} accentColor={radixAccent as never} grayColor="slate" radius="medium" scaling="100%">
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
            ? 'h-screen w-screen bg-[var(--mc-bg-main)]'
            : 'h-screen w-screen bg-[radial-gradient(1200px_560px_at_-8%_-10%,#d1fae5_0%,transparent_58%),radial-gradient(1200px_560px_at_108%_-12%,#e0f2fe_0%,transparent_58%),#f8fafc]'
        }
      >
        <div className="grid h-full min-h-0 grid-cols-[320px_minmax(0,1fr)]">
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
                  ? 'sticky top-0 z-10 border-b border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)]/95 px-4 py-3 backdrop-blur-sm'
                  : 'sticky top-0 z-10 border-b border-slate-200 bg-white/92 px-4 py-3 backdrop-blur-sm'
              }
            >
              <Flex justify="between" align="center" gap="3" wrap="wrap">
                <Heading size="6">
                  {selectedSessionLabel}
                </Heading>
                <Flex align="center" gap="3" wrap="wrap" justify="end">
                  <Text size="2" color="gray">
                    {statusText}
                  </Text>
                  <Text
                    size="2"
                    color={(queueLane?.last_error ? 'red' : 'gray') as never}
                    title={queueLane?.last_error ?? undefined}
                  >
                    Queue: {(queueLane?.pending ?? 0) > 0 ? String(queueLane?.pending ?? 0) : 'idle'}
                    {(queueLane?.pending ?? 0) > 0 && (queueLane?.oldest_wait_ms ?? 0) > 0
                      ? ` · ${Math.round((queueLane?.oldest_wait_ms ?? 0) / 1000)}s`
                      : ''}
                  </Text>

                  <Dialog.Root
                    open={schedulesDialogOpen}
                    onOpenChange={(open) => setSchedulesDialogOpen(open)}
                  >
                    <Dialog.Trigger>
                      <Button size="1" variant="soft">Schedules</Button>
                    </Dialog.Trigger>
                    <Dialog.Content style={{ maxWidth: 820 }}>
                      <Dialog.Title>Schedules</Dialog.Title>
                      <Dialog.Description size="2" mb="3">
                        Create and manage scheduled prompts for this chat.
                      </Dialog.Description>

                      <div className="rounded-md border p-3" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' } : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}>
                        <ul className="mb-3 list-none space-y-2">
                          {schedules.map((t) => (
                            <li key={t.id} className="flex flex-wrap items-center gap-2 rounded border p-2" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}>
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
                    open={memoryDialogOpen}
                    onOpenChange={(open) => {
                      setMemoryDialogOpen(open)
                      if (!open) {
                        setMemoryError('')
                        setMemoryBusy(false)
                      }
                    }}
                  >
                    <Dialog.Trigger>
                      <Button size="1" variant="soft">Memory</Button>
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
                </Flex>
              </Flex>
            </header>

            <div
              className={
                appearance === 'dark'
                  ? 'flex min-h-0 flex-1 flex-col bg-[linear-gradient(to_bottom,var(--mc-bg-panel),var(--mc-bg-main)_28%)]'
                  : 'flex min-h-0 flex-1 flex-col bg-[linear-gradient(to_bottom,#f8fafc,white_20%)]'
              }
            >
              <div className="mx-auto w-full max-w-5xl px-3 pt-3">
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

              <div className="min-h-0 flex-1 px-1 pb-1 flex flex-col">
                <div className="flex justify-center py-2">
                  <Button size="1" variant="soft" onClick={() => void loadOlderDay()} disabled={loadingOlder}>
                    {loadingOlder ? 'Loading…' : 'Load older'}
                  </Button>
                </div>
                <div className="min-h-0 flex-1">
                  <ThreadPane key={runtimeKey} adapter={adapter} initialMessages={historySeed} runtimeKey={runtimeKey} />
                </div>
              </div>
            </div>
          </main>
        </div>

      </div>
    </Theme>
  )
}

createRoot(document.getElementById('root')!).render(<App />)
