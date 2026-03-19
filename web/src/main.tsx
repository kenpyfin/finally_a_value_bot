import React, { useEffect, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import type { ReadonlyJSONObject, ReadonlyJSONValue } from 'assistant-stream/utils'
import {
  AssistantRuntimeProvider,
  MessagePrimitive,
  useMessage,
  useLocalRuntime,
  type ChatModelAdapter,
  type ChatModelRunOptions,
  type ChatModelRunResult,
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
  Switch,
  Text,
  TextField,
  Theme,
} from '@radix-ui/themes'
import '@radix-ui/themes/styles.css'
import '@assistant-ui/react-ui/styles/index.css'
import './styles.css'
import { SessionSidebar } from './components/session-sidebar'
import type { Persona, ScheduleTask, ChannelBinding } from './types'

type ConfigPayload = Record<string, unknown>

type StreamEvent = {
  event: string
  payload: Record<string, unknown>
}

type BackendMessage = {
  id?: string
  sender_name?: string
  content?: string
  is_from_bot?: boolean
  timestamp?: string
}

type ToolStartPayload = {
  tool_use_id: string
  name: string
  input?: unknown
}

type ToolResultPayload = {
  tool_use_id: string
  name: string
  is_error?: boolean
  output?: unknown
  duration_ms?: number
  bytes?: number
  status_code?: number
  error_type?: string
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

const PROVIDER_SUGGESTIONS = [
  'openai',
  'ollama',
  'openrouter',
  'anthropic',
  'google',
  'alibaba',
  'deepseek',
  'moonshot',
  'mistral',
  'azure',
  'bedrock',
  'zhipu',
  'minimax',
  'cohere',
  'tencent',
  'xai',
  'huggingface',
  'together',
  'custom',
]

const MODEL_OPTIONS: Record<string, string[]> = {
  anthropic: ['claude-sonnet-4-5-20250929', 'claude-opus-4-1-20250805', 'claude-3-7-sonnet-latest'],
  openai: ['gpt-5.2', 'gpt-5', 'gpt-4.1'],
  ollama: ['llama3.2', 'qwen2.5', 'deepseek-r1'],
  openrouter: ['openai/gpt-5', 'anthropic/claude-sonnet-4-5', 'google/gemini-2.5-pro'],
  deepseek: ['deepseek-chat', 'deepseek-reasoner'],
  google: ['gemini-2.5-pro', 'gemini-2.5-flash'],
}

const DEFAULT_CONFIG_VALUES = {
  llm_provider: 'anthropic',
  max_tokens: 8192,
  max_tool_iterations: 100,
  max_document_size_mb: 100,
  show_thinking: false,
  web_enabled: true,
  web_host: '127.0.0.1',
  web_port: 10961,
  safety_output_guard_mode: 'moderate',
  safety_max_emojis_per_response: 12,
  safety_tail_repeat_limit: 8,
  safety_execution_mode: 'warn_confirm',
  safety_risky_categories: ['destructive', 'system', 'network', 'package'],
}

const OUTPUT_GUARD_MODE_OPTIONS = ['off', 'moderate', 'strict'] as const
const EXECUTION_MODE_OPTIONS = ['off', 'warn_confirm', 'strict'] as const
const RISKY_CATEGORY_OPTIONS = ['destructive', 'system', 'network', 'package'] as const

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

function defaultModelForProvider(providerRaw: string): string {
  const provider = providerRaw.trim().toLowerCase()
  if (provider === 'anthropic') return 'claude-sonnet-4-5-20250929'
  if (provider === 'ollama') return 'llama3.2'
  return 'gpt-5.2'
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

async function* parseSseFrames(
  response: Response,
  signal: AbortSignal,
): AsyncGenerator<StreamEvent, void> {
  if (!response.body) {
    throw new Error('empty stream body')
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let pending = ''
  let eventName = 'message'
  let dataLines: string[] = []

  const flush = (): StreamEvent | null => {
    if (dataLines.length === 0) return null
    const raw = dataLines.join('\n')
    dataLines = []

    let payload: Record<string, unknown> = {}
    try {
      payload = JSON.parse(raw) as Record<string, unknown>
    } catch {
      payload = { raw }
    }

    const event: StreamEvent = { event: eventName, payload }
    eventName = 'message'
    return event
  }

  const handleLine = (line: string): StreamEvent | null => {
    if (line === '') return flush()
    if (line.startsWith(':')) return null

    const sep = line.indexOf(':')
    const field = sep >= 0 ? line.slice(0, sep) : line
    let value = sep >= 0 ? line.slice(sep + 1) : ''
    if (value.startsWith(' ')) value = value.slice(1)

    if (field === 'event') eventName = value
    if (field === 'data') dataLines.push(value)

    return null
  }

  while (true) {
    if (signal.aborted) return

    const { done, value } = await reader.read()
    pending += decoder.decode(value || new Uint8Array(), { stream: !done })

    while (true) {
      const idx = pending.indexOf('\n')
      if (idx < 0) break
      let line = pending.slice(0, idx)
      pending = pending.slice(idx + 1)
      if (line.endsWith('\r')) line = line.slice(0, -1)
      const event = handleLine(line)
      if (event) yield event
    }

    if (done) {
      if (pending.length > 0) {
        let line = pending
        if (line.endsWith('\r')) line = line.slice(0, -1)
        const event = handleLine(line)
        if (event) yield event
      }
      const event = flush()
      if (event) yield event
      return
    }
  }
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

function mapBackendHistory(messages: BackendMessage[]): ThreadMessageLike[] {
  return messages.map((item, index) => ({
    id: item.id || `history-${index}`,
    role: item.is_from_bot ? 'assistant' : 'user',
    content: item.content || '',
    createdAt: item.timestamp ? new Date(item.timestamp) : new Date(),
  }))
}

function asObject(value: unknown): Record<string, unknown> {
  if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
    return value as Record<string, unknown>
  }
  return {}
}

function toJsonValue(value: unknown): ReadonlyJSONValue {
  try {
    return JSON.parse(JSON.stringify(value)) as ReadonlyJSONValue
  } catch {
    return String(value)
  }
}

function toJsonObject(value: unknown): ReadonlyJSONObject {
  const normalized = toJsonValue(value)
  if (typeof normalized === 'object' && normalized !== null && !Array.isArray(normalized)) {
    return normalized as ReadonlyJSONObject
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

function readStringArray(value: unknown, fallback: readonly string[]): string[] {
  if (!Array.isArray(value)) return [...fallback]
  const out = value
    .map((v) => (typeof v === 'string' ? v.trim().toLowerCase() : ''))
    .filter((v) => v.length > 0)
  return out.length > 0 ? out : [...fallback]
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
  const MarkdownText = makeMarkdownText()
  const runtime = useLocalRuntime(adapter, {
    initialMessages,
    maxSteps: 100,
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
  const [sending, setSending] = useState<boolean>(false)
  const [configOpen, setConfigOpen] = useState<boolean>(false)
  const [config, setConfig] = useState<ConfigPayload | null>(null)
  const [configDraft, setConfigDraft] = useState<Record<string, unknown>>({})
  const [saveStatus, setSaveStatus] = useState<string>('')
  const [authRequired, setAuthRequired] = useState<boolean>(false)
  const [authTokenInput, setAuthTokenInput] = useState<string>('')
  const [personas, setPersonas] = useState<Persona[]>([])
  const [activePersonaId, setActivePersonaId] = useState<number | null>(null)
  const [schedules, setSchedules] = useState<ScheduleTask[]>([])
  const [schedulesOpen, setSchedulesOpen] = useState<boolean>(false)
  const [newSchedulePrompt, setNewSchedulePrompt] = useState('')
  const [newScheduleType, setNewScheduleType] = useState<'cron' | 'once'>('cron')
  const [newScheduleValue, setNewScheduleValue] = useState('0 9 * * *')
  const [newSchedulePersonaId, setNewSchedulePersonaId] = useState<number | null>(null)
  const [bindings, setBindings] = useState<ChannelBinding[]>([])
  const sendingRef = React.useRef<boolean>(false)

  React.useEffect(() => {
    const onAuthRequired = () => setAuthRequired(true)
    window.addEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
    return () => window.removeEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
  }, [])

  const selectedSessionLabel = 'Chat'
  const selectedSessionReadOnly = false

  /** Loads personas and applies stored preference; returns the chosen persona id and name for history/switch. */
  async function loadPersonas(cid: number | null = chatId): Promise<{ id: number; name: string } | null> {
    if (cid == null) return null
    try {
      const query = new URLSearchParams({ chat_id: String(cid) })
      const data = await api<{ personas?: { id: number; name: string; is_active: boolean }[] }>(`/api/personas?${query.toString()}`)
      const list = Array.isArray(data.personas) ? data.personas : []
      const personaList = list.map((p) => ({ id: p.id, name: p.name, is_active: p.is_active }))
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

  async function switchPersona(personaName: string): Promise<void> {
    if (chatId == null) return
    await api('/api/personas/switch', {
      method: 'POST',
      body: JSON.stringify({ chat_id: chatId, persona_name: personaName }),
    })
    const p = personas.find((x) => x.name === personaName)
    if (p) writeStoredPersonaId(p.id)
    await loadPersonas(chatId)
    await loadHistory(chatId, p?.id ?? undefined)
    setRuntimeNonce((x) => x + 1)
  }

  async function loadHistory(cid: number | null = chatId, personaId?: number | null, day?: string | null): Promise<void> {
    if (cid == null) return
    const query = new URLSearchParams({ chat_id: String(cid) })
    if (personaId != null && personaId > 0) query.set('persona_id', String(personaId))
    if (day) query.set('day', day)
    else query.set('limit', '500')
    const data = await api<{ messages?: BackendMessage[] }>(`/api/history?${query.toString()}`)
    const rawMessages = Array.isArray(data.messages) ? data.messages : []
    const mapped = mapBackendHistory(rawMessages)
    if (day) {
      setHistoryByDay((prev) => {
        const next = { ...prev, [day]: mapped }
        const allDays = Object.keys(next).sort()
        const combined = allDays.flatMap((d) => (next[d] ?? []))
        setHistorySeed(combined)
        return next
      })
    } else {
      setHistoryByDay({})
      setHistorySeed(mapped)
    }
    setRuntimeNonce((x) => x + 1)
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
      if (nextOlder) await loadHistory(chatId, activePersonaId ?? undefined, nextOlder)
    } finally {
      setLoadingOlder(false)
    }
  }

  const adapter = useMemo<ChatModelAdapter>(
    () => ({
      run: async function* (options): AsyncGenerator<ChatModelRunResult, void> {
        const { text: userText, attachments } = await extractLatestUserInput(options.messages)
        if (!userText && attachments.length === 0) return
        if (sendingRef.current) {
          throw new Error('A response is already in progress. Please wait for it to finish.')
        }
        sendingRef.current = true

        setSending(true)
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

          let receivedDone = false

          const query = new URLSearchParams({ run_id: runId })
          const streamResponse = await fetch(`/api/stream?${query.toString()}`, {
            method: 'GET',
            headers: makeHeaders(),
            cache: 'no-store',
            signal: options.abortSignal,
          })

          if (streamResponse.status === 401) {
            window.dispatchEvent(new CustomEvent(AUTH_REQUIRED_EVENT))
            throw new Error('Unauthorized. Enter the API token (WEB_AUTH_TOKEN from .env).')
          }
          if (!streamResponse.ok) {
            const text = await streamResponse.text().catch(() => '')
            const msg =
              streamResponse.status === 429
                ? 'Too many requests. Please wait a moment before sending again.'
                : messageForFailedResponse(streamResponse.status, { message: text || undefined }, text)
            throw new Error(msg)
          }

          let assistantText = ''
          const toolState = new Map<
            string,
            {
              name: string
              args: ReadonlyJSONObject
              result?: ReadonlyJSONValue
              isError?: boolean
            }
          >()

          const makeContent = () => {
            const toolParts = Array.from(toolState.entries()).map(([toolCallId, tool]) => ({
              type: 'tool-call' as const,
              toolCallId,
              toolName: tool.name,
              args: tool.args,
              argsText: JSON.stringify(tool.args),
              ...(tool.result ? { result: tool.result } : {}),
              ...(tool.isError !== undefined ? { isError: tool.isError } : {}),
            }))

            return [
              ...(assistantText ? [{ type: 'text' as const, text: assistantText }] : []),
              ...toolParts,
            ]
          }

          for await (const event of parseSseFrames(streamResponse, options.abortSignal)) {
            const data = event.payload

            if (event.event === 'replay_meta') {
              if (data.replay_truncated === true) {
                const oldest = typeof data.oldest_event_id === 'number' ? data.oldest_event_id : null
                const message =
                  oldest !== null
                    ? `Stream history was truncated. Recovery resumed from event #${oldest}.`
                    : 'Stream history was truncated. Recovery resumed from the earliest available event.'
                setReplayNotice(message)
              }
              continue
            }

            if (event.event === 'status') {
              const message = typeof data.message === 'string' ? data.message : ''
              if (message) setStatusText(message)
              continue
            }

            if (event.event === 'tool_start') {
              const payload = data as ToolStartPayload
              if (!payload.tool_use_id || !payload.name) continue
              toolState.set(payload.tool_use_id, {
                name: payload.name,
                args: toJsonObject(payload.input),
              })
              setStatusText(`tool: ${payload.name}...`)
              const content = makeContent()
              if (content.length > 0) yield { content }
              continue
            }

            if (event.event === 'tool_result') {
              const payload = data as ToolResultPayload
              if (!payload.tool_use_id || !payload.name) continue

              const previous = toolState.get(payload.tool_use_id)
              const resultPayload: ReadonlyJSONObject = toJsonObject({
                output: payload.output ?? '',
                duration_ms: payload.duration_ms ?? null,
                bytes: payload.bytes ?? null,
                status_code: payload.status_code ?? null,
                error_type: payload.error_type ?? null,
              })

              toolState.set(payload.tool_use_id, {
                name: payload.name,
                args: previous?.args ?? {},
                result: resultPayload,
                isError: Boolean(payload.is_error),
              })

              const ms = typeof payload.duration_ms === 'number' ? payload.duration_ms : 0
              const bytes = typeof payload.bytes === 'number' ? payload.bytes : 0
              setStatusText(`tool: ${payload.name} ${payload.is_error ? 'error' : 'ok'} ${ms}ms ${bytes}b`)
              const content = makeContent()
              if (content.length > 0) yield { content }
              continue
            }

            if (event.event === 'delta') {
              const delta = typeof data.delta === 'string' ? data.delta : ''
              if (!delta) continue
              assistantText += delta
              const content = makeContent()
              if (content.length > 0) yield { content }
              continue
            }

            if (event.event === 'error') {
              const message = typeof data.error === 'string' ? data.error : 'stream error'
              throw new Error(message)
            }

            if (event.event === 'done') {
              receivedDone = true
              // Command shortcuts (e.g. /persona, /reset) return full response in done only, no deltas
              const doneResponse =
                typeof (data as { response?: string }).response === 'string'
                  ? (data as { response: string }).response
                  : ''
              if (doneResponse && assistantText.length === 0) {
                assistantText = doneResponse
                const content = makeContent()
                if (content.length > 0) yield { content }
              }
              setStatusText('Done')
              break
            }
          }

          // If stream ended without "done" (disconnect, tab close, timeout), poll until run completes so the user sees the result without sending a follow-up message.
          if (!receivedDone && runId) {
            const pollIntervalMs = 2500
            const pollMaxMs = 10 * 60 * 1000 // 10 minutes
            const start = Date.now()
            while (Date.now() - start < pollMaxMs) {
              await new Promise((r) => setTimeout(r, pollIntervalMs))
              try {
                const status = await api<{ done?: boolean }>(
                  `/api/run_status?run_id=${encodeURIComponent(runId)}`,
                )
                if (status.done === true) {
                  setStatusText('Done')
                  await loadHistory(chatId)
                  break
                }
              } catch {
                // Run not found (404) or other error — stop polling
                break
              }
            }
          }
        } finally {
          sendingRef.current = false
          setSending(false)
          void loadHistory(chatId, activePersonaId ?? undefined)
        }
      },
    }),
    [chatId, selectedSessionReadOnly, activePersonaId, sendingRef],
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
      if (activePersonaId === personaId) await loadHistory(chatId)
      setStatusText('Persona deleted')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  async function openConfig(): Promise<void> {
    setSaveStatus('')
    const data = await api<{ config?: ConfigPayload }>('/api/config')
    setConfig(data.config || null)
    setConfigDraft({
      llm_provider: data.config?.llm_provider || '',
      model: data.config?.model || defaultModelForProvider(String(data.config?.llm_provider || 'anthropic')),
      llm_base_url: String(data.config?.llm_base_url || ''),
      api_key: '',
      max_tokens: Number(data.config?.max_tokens ?? 8192),
      max_tool_iterations: Number(data.config?.max_tool_iterations ?? 100),
      max_document_size_mb: Number(data.config?.max_document_size_mb ?? DEFAULT_CONFIG_VALUES.max_document_size_mb),
      show_thinking: Boolean(data.config?.show_thinking),
      web_enabled: Boolean(data.config?.web_enabled),
      web_host: String(data.config?.web_host || '127.0.0.1'),
      web_port: Number(data.config?.web_port ?? 10961),
      safety_output_guard_mode: String(
        data.config?.safety_output_guard_mode || DEFAULT_CONFIG_VALUES.safety_output_guard_mode,
      ),
      safety_max_emojis_per_response: Number(
        data.config?.safety_max_emojis_per_response ?? DEFAULT_CONFIG_VALUES.safety_max_emojis_per_response,
      ),
      safety_tail_repeat_limit: Number(
        data.config?.safety_tail_repeat_limit ?? DEFAULT_CONFIG_VALUES.safety_tail_repeat_limit,
      ),
      safety_execution_mode: String(
        data.config?.safety_execution_mode || DEFAULT_CONFIG_VALUES.safety_execution_mode,
      ),
      safety_risky_categories: readStringArray(
        data.config?.safety_risky_categories,
        DEFAULT_CONFIG_VALUES.safety_risky_categories,
      ),
    })
    setConfigOpen(true)
  }

  function setConfigField(field: string, value: unknown): void {
    setConfigDraft((prev) => ({ ...prev, [field]: value }))
  }

  function resetConfigField(field: string): void {
    setConfigDraft((prev) => {
      const next = { ...prev }
      switch (field) {
        case 'llm_provider':
          next.llm_provider = DEFAULT_CONFIG_VALUES.llm_provider
          next.model = defaultModelForProvider(DEFAULT_CONFIG_VALUES.llm_provider)
          break
        case 'model':
          next.model = defaultModelForProvider(String(next.llm_provider || DEFAULT_CONFIG_VALUES.llm_provider))
          break
        case 'llm_base_url':
          next.llm_base_url = ''
          break
        case 'max_tokens':
          next.max_tokens = DEFAULT_CONFIG_VALUES.max_tokens
          break
        case 'max_tool_iterations':
          next.max_tool_iterations = DEFAULT_CONFIG_VALUES.max_tool_iterations
          break
        case 'max_document_size_mb':
          next.max_document_size_mb = DEFAULT_CONFIG_VALUES.max_document_size_mb
          break
        case 'show_thinking':
          next.show_thinking = DEFAULT_CONFIG_VALUES.show_thinking
          break
        case 'web_enabled':
          next.web_enabled = DEFAULT_CONFIG_VALUES.web_enabled
          break
        case 'web_host':
          next.web_host = DEFAULT_CONFIG_VALUES.web_host
          break
        case 'web_port':
          next.web_port = DEFAULT_CONFIG_VALUES.web_port
          break
        case 'safety_output_guard_mode':
          next.safety_output_guard_mode = DEFAULT_CONFIG_VALUES.safety_output_guard_mode
          break
        case 'safety_max_emojis_per_response':
          next.safety_max_emojis_per_response = DEFAULT_CONFIG_VALUES.safety_max_emojis_per_response
          break
        case 'safety_tail_repeat_limit':
          next.safety_tail_repeat_limit = DEFAULT_CONFIG_VALUES.safety_tail_repeat_limit
          break
        case 'safety_execution_mode':
          next.safety_execution_mode = DEFAULT_CONFIG_VALUES.safety_execution_mode
          break
        case 'safety_risky_categories':
          next.safety_risky_categories = [...DEFAULT_CONFIG_VALUES.safety_risky_categories]
          break
        default:
          break
      }
      return next
    })
  }

  async function saveConfigChanges(): Promise<void> {
    try {
      const payload: Record<string, unknown> = {
        llm_provider: String(configDraft.llm_provider || ''),
        model: String(configDraft.model || ''),
        max_tokens: Number(configDraft.max_tokens || 8192),
        max_tool_iterations: Number(configDraft.max_tool_iterations || 100),
        max_document_size_mb: Number(
          configDraft.max_document_size_mb || DEFAULT_CONFIG_VALUES.max_document_size_mb,
        ),
        show_thinking: Boolean(configDraft.show_thinking),
        web_enabled: Boolean(configDraft.web_enabled),
        web_host: String(configDraft.web_host || '127.0.0.1'),
        web_port: Number(configDraft.web_port || 10961),
        safety_output_guard_mode: String(
          configDraft.safety_output_guard_mode || DEFAULT_CONFIG_VALUES.safety_output_guard_mode,
        ),
        safety_max_emojis_per_response: Number(
          configDraft.safety_max_emojis_per_response || DEFAULT_CONFIG_VALUES.safety_max_emojis_per_response,
        ),
        safety_tail_repeat_limit: Number(
          configDraft.safety_tail_repeat_limit || DEFAULT_CONFIG_VALUES.safety_tail_repeat_limit,
        ),
        safety_execution_mode: String(
          configDraft.safety_execution_mode || DEFAULT_CONFIG_VALUES.safety_execution_mode,
        ),
        safety_risky_categories: readStringArray(
          configDraft.safety_risky_categories,
          DEFAULT_CONFIG_VALUES.safety_risky_categories,
        ),
      }
      if (String(configDraft.llm_provider || '').trim().toLowerCase() === 'custom') {
        payload.llm_base_url = String(configDraft.llm_base_url || '').trim() || null
      }
      const apiKey = String(configDraft.api_key || '').trim()
      if (apiKey) payload.api_key = apiKey

      await api('/api/config', { method: 'PUT', body: JSON.stringify(payload) })
      setSaveStatus('Saved. Restart finally-a-value-bot to apply changes.')
    } catch (e) {
      setSaveStatus(`Save failed: ${e instanceof Error ? e.message : String(e)}`)
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
    ;(async () => {
      try {
        setError('')
        const data = await api<{ chat_id?: number; persona_id?: number }>('/api/chat')
        const cid = typeof data.chat_id === 'number' ? data.chat_id : null
        const pid = typeof data.persona_id === 'number' ? data.persona_id : null
        setChatId(cid)
        if (pid != null) setActivePersonaId(pid)
        if (cid != null) {
          const chosen = await loadPersonas(cid)
          loadBindings(cid).catch(() => {})
          loadSchedules(cid).catch(() => {})
          await loadHistory(cid, chosen?.id ?? pid)
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

  async function bindToContact(contactChatId: number): Promise<void> {
    await api('/api/contacts/bind', {
      method: 'POST',
      body: JSON.stringify({ contact_chat_id: contactChatId }),
    })
    await loadBindings(chatId)
    await loadHistory(chatId)
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
      loadBindings(chatId).catch(() => {})
      loadSchedules(chatId).catch(() => {})
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
      loadHistory(chatId, chosen?.id).catch((e) => setError(e instanceof Error ? e.message : String(e)))
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


  const runtimeKey = `${chatId ?? 0}-${activePersonaId ?? 0}-${runtimeNonce}`
  const radixAccent = RADIX_ACCENT_BY_THEME[uiTheme] ?? 'green'
  const currentProvider = String(configDraft.llm_provider || DEFAULT_CONFIG_VALUES.llm_provider).trim().toLowerCase()
  const providerOptions = Array.from(
    new Set([currentProvider, ...PROVIDER_SUGGESTIONS.map((p) => p.toLowerCase())].filter(Boolean)),
  )
  const modelOptions = MODEL_OPTIONS[currentProvider] || []
  const sectionCardClass = appearance === 'dark'
    ? 'rounded-xl border p-5'
    : 'rounded-xl border border-slate-200/80 p-5'
  const sectionCardStyle = appearance === 'dark'
    ? { borderColor: 'color-mix(in srgb, var(--mc-border-soft) 68%, transparent)' }
    : undefined
  const toggleCardClass = appearance === 'dark'
    ? 'rounded-lg border p-3'
    : 'rounded-lg border border-slate-200/80 p-3'
  const toggleCardStyle = appearance === 'dark'
    ? { borderColor: 'color-mix(in srgb, var(--mc-border-soft) 60%, transparent)' }
    : undefined

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
            selectedPersonaId={activePersonaId}
            onPersonaSelect={(name) => void switchPersona(name)}
            onCreatePersona={() => void onCreatePersona()}
            onDeletePersona={(id) => void onDeletePersona(id)}
            onOpenConfig={openConfig}
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
              <Heading size="6">
                {selectedSessionLabel}
              </Heading>
            </header>

            <div className="mx-3 mt-2">
              <Button size="1" variant="soft" onClick={() => setSchedulesOpen((o) => !o)}>
                {schedulesOpen ? 'Hide' : 'Show'} Schedules
              </Button>
              {schedulesOpen ? (
                <div className="mt-2 rounded-md border p-3" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' } : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}>
                  <Text size="2" weight="bold" className="mb-2 block">Schedules</Text>
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
                        <Text size="1" color="gray">{t.status}</Text>
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
                      className="min-w-[180px]"
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
                      className="min-w-[160px]"
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
              ) : null}
            </div>

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

        <Dialog.Root open={configOpen} onOpenChange={setConfigOpen}>
          <Dialog.Content maxWidth="760px">
            <Dialog.Title>Runtime Config</Dialog.Title>
            <Dialog.Description size="2" mb="3">
              Save writes to .env. Restart may be required.
            </Dialog.Description>
            {config ? (
              <Flex direction="column" gap="4">
                <div className={sectionCardClass} style={sectionCardStyle}>
                  <Text size="3" weight="bold">
                    LLM
                  </Text>
                  <div className="mt-3 grid grid-cols-1 gap-3">
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Provider</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('llm_provider')}>Reset</Button>
                      </Flex>
                      <Select.Root
                        value={String(configDraft.llm_provider || DEFAULT_CONFIG_VALUES.llm_provider)}
                        onValueChange={(value) => setConfigField('llm_provider', value)}
                      >
                        <Select.Trigger placeholder="Select provider" />
                        <Select.Content>
                          {providerOptions.map((provider) => (
                            <Select.Item key={provider} value={provider}>
                              {provider}
                            </Select.Item>
                          ))}
                        </Select.Content>
                      </Select.Root>
                    </div>

                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Model</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('model')}>Reset</Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.model || defaultModelForProvider(String(configDraft.llm_provider || DEFAULT_CONFIG_VALUES.llm_provider)))}
                        onChange={(e) => setConfigField('model', e.target.value)}
                        placeholder="claude-sonnet-4-5-20250929"
                      />
                      {modelOptions.length > 0 ? (
                        <Text size="1" color="gray" className="mt-1 block">
                          Suggested: {modelOptions.join(' / ')}
                        </Text>
                      ) : null}
                    </div>

                    {currentProvider === 'custom' ? (
                      <div>
                        <Flex justify="between" align="center" mb="1">
                          <Text size="1" color="gray">API Host</Text>
                          <Button size="1" variant="ghost" onClick={() => resetConfigField('llm_base_url')}>Reset</Button>
                        </Flex>
                        <TextField.Root
                          value={String(configDraft.llm_base_url || '')}
                          onChange={(e) => setConfigField('llm_base_url', e.target.value)}
                          placeholder="https://your-provider.example/v1"
                        />
                      </div>
                    ) : null}

                    <div>
                      <Text size="1" color="gray">API key (leave blank to keep existing)</Text>
                      <TextField.Root
                        className="mt-2"
                        value={String(configDraft.api_key || '')}
                        onChange={(e) => setConfigField('api_key', e.target.value)}
                        placeholder="api_key"
                      />
                    </div>
                  </div>
                </div>

                <div className={sectionCardClass} style={sectionCardStyle}>
                  <Text size="3" weight="bold">
                    Runtime
                  </Text>
                  <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Max tokens</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('max_tokens')}>Reset</Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.max_tokens || DEFAULT_CONFIG_VALUES.max_tokens)}
                        onChange={(e) => setConfigField('max_tokens', e.target.value)}
                        placeholder="max_tokens"
                      />
                    </div>
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Max tool iterations</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('max_tool_iterations')}>Reset</Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.max_tool_iterations || DEFAULT_CONFIG_VALUES.max_tool_iterations)}
                        onChange={(e) => setConfigField('max_tool_iterations', e.target.value)}
                        placeholder="max_tool_iterations"
                      />
                    </div>
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Max document size (MB)</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('max_document_size_mb')}>Reset</Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.max_document_size_mb || DEFAULT_CONFIG_VALUES.max_document_size_mb)}
                        onChange={(e) => setConfigField('max_document_size_mb', e.target.value)}
                        placeholder="max_document_size_mb"
                      />
                    </div>
                  </div>
                </div>

                <div className={sectionCardClass} style={sectionCardStyle}>
                  <Text size="3" weight="bold">
                    Safety
                  </Text>
                  <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Output guard mode</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('safety_output_guard_mode')}>
                          Reset
                        </Button>
                      </Flex>
                      <Select.Root
                        value={String(configDraft.safety_output_guard_mode || DEFAULT_CONFIG_VALUES.safety_output_guard_mode)}
                        onValueChange={(value) => setConfigField('safety_output_guard_mode', value)}
                      >
                        <Select.Trigger />
                        <Select.Content>
                          {OUTPUT_GUARD_MODE_OPTIONS.map((mode) => (
                            <Select.Item key={mode} value={mode}>
                              {mode}
                            </Select.Item>
                          ))}
                        </Select.Content>
                      </Select.Root>
                    </div>
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Execution safety mode</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('safety_execution_mode')}>
                          Reset
                        </Button>
                      </Flex>
                      <Select.Root
                        value={String(configDraft.safety_execution_mode || DEFAULT_CONFIG_VALUES.safety_execution_mode)}
                        onValueChange={(value) => setConfigField('safety_execution_mode', value)}
                      >
                        <Select.Trigger />
                        <Select.Content>
                          {EXECUTION_MODE_OPTIONS.map((mode) => (
                            <Select.Item key={mode} value={mode}>
                              {mode}
                            </Select.Item>
                          ))}
                        </Select.Content>
                      </Select.Root>
                    </div>
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Max emojis per response</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('safety_max_emojis_per_response')}>
                          Reset
                        </Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.safety_max_emojis_per_response || DEFAULT_CONFIG_VALUES.safety_max_emojis_per_response)}
                        onChange={(e) => setConfigField('safety_max_emojis_per_response', e.target.value)}
                        placeholder="12"
                      />
                    </div>
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Tail repeat limit</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('safety_tail_repeat_limit')}>
                          Reset
                        </Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.safety_tail_repeat_limit || DEFAULT_CONFIG_VALUES.safety_tail_repeat_limit)}
                        onChange={(e) => setConfigField('safety_tail_repeat_limit', e.target.value)}
                        placeholder="8"
                      />
                    </div>
                  </div>
                  <div className="mt-3">
                    <Flex justify="between" align="center" mb="2">
                      <Text size="1" color="gray">Risky command categories</Text>
                      <Button size="1" variant="ghost" onClick={() => resetConfigField('safety_risky_categories')}>
                        Reset
                      </Button>
                    </Flex>
                    <Flex gap="2" wrap="wrap">
                      {RISKY_CATEGORY_OPTIONS.map((category) => {
                        const current = readStringArray(
                          configDraft.safety_risky_categories,
                          DEFAULT_CONFIG_VALUES.safety_risky_categories,
                        )
                        const enabled = current.includes(category)
                        return (
                          <Button
                            key={category}
                            size="1"
                            variant={enabled ? 'solid' : 'soft'}
                            onClick={() => {
                              const next = enabled
                                ? current.filter((c) => c !== category)
                                : [...current, category]
                              setConfigField('safety_risky_categories', next)
                            }}
                          >
                            {category}
                          </Button>
                        )
                      })}
                    </Flex>
                  </div>
                </div>

                <div className={sectionCardClass} style={sectionCardStyle}>
                  <Text size="3" weight="bold">
                    Web
                  </Text>
                  <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Host</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('web_host')}>Reset</Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.web_host || DEFAULT_CONFIG_VALUES.web_host)}
                        onChange={(e) => setConfigField('web_host', e.target.value)}
                        placeholder="web_host"
                      />
                    </div>
                    <div>
                      <Flex justify="between" align="center" mb="1">
                        <Text size="1" color="gray">Port</Text>
                        <Button size="1" variant="ghost" onClick={() => resetConfigField('web_port')}>Reset</Button>
                      </Flex>
                      <TextField.Root
                        value={String(configDraft.web_port || DEFAULT_CONFIG_VALUES.web_port)}
                        onChange={(e) => setConfigField('web_port', e.target.value)}
                        placeholder="web_port"
                      />
                    </div>
                  </div>
                  <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
                    <div className={toggleCardClass} style={toggleCardStyle}>
                      <Flex justify="between" align="center">
                        <Text size="2">show_thinking</Text>
                        <Switch
                          checked={Boolean(configDraft.show_thinking)}
                          onCheckedChange={(checked) => setConfigField('show_thinking', checked)}
                        />
                      </Flex>
                      <Button size="1" variant="ghost" className="mt-2" onClick={() => resetConfigField('show_thinking')}>
                        Reset to default
                      </Button>
                    </div>
                    <div className={toggleCardClass} style={toggleCardStyle}>
                      <Flex justify="between" align="center">
                        <Text size="2">web_enabled</Text>
                        <Switch
                          checked={Boolean(configDraft.web_enabled)}
                          onCheckedChange={(checked) => setConfigField('web_enabled', checked)}
                        />
                      </Flex>
                      <Button size="1" variant="ghost" className="mt-2" onClick={() => resetConfigField('web_enabled')}>
                        Reset to default
                      </Button>
                    </div>
                  </div>
                </div>

                {saveStatus ? (
                  <Text size="2" color={saveStatus.startsWith('Save failed') ? 'red' : 'green'}>
                    {saveStatus}
                  </Text>
                ) : null}
                <Flex justify="end" gap="2" mt="1">
                  <Dialog.Close>
                    <Button variant="soft">Close</Button>
                  </Dialog.Close>
                  <Button onClick={() => void saveConfigChanges()}>Save</Button>
                </Flex>
              </Flex>
            ) : (
              <Text size="2" color="gray">
                Loading...
              </Text>
            )}
          </Dialog.Content>
        </Dialog.Root>
      </div>
    </Theme>
  )
}

createRoot(document.getElementById('root')!).render(<App />)
