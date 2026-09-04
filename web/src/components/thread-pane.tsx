import React from 'react'
import { ThreadHistorySkeleton } from './skeleton'
import { IconCopy, IconPencil, IconReply, IconSideChat, IconStar, IconTrash } from './icons'
import {
  AssistantRuntimeProvider,
  CompositeAttachmentAdapter,
  ComposerPrimitive,
  MessagePrimitive,
  SimpleImageAttachmentAdapter,
  SimpleTextAttachmentAdapter,
  useAui,
  useAuiState,
  useMessage,
  useLocalRuntime,
  type AttachmentAdapter,
  type ChatModelAdapter,
  type CompleteAttachment,
  type PendingAttachment,
  type ThreadMessageLike,
  type ToolCallMessagePartProps,
} from '@assistant-ui/react'
import { ThreadWelcomeHints } from './thread-welcome-hints'
import { LoadEarlierMessages } from './load-earlier-messages'
import { ScrollToLatest } from './scroll-to-latest'
import {
  AssistantMessage,
  BranchPicker,
  Composer,
  Thread,
  UserMessage,
  makeMarkdownText,
} from '@assistant-ui/react-ui'
import remarkGfm from 'remark-gfm'
import { historiesEqual, isHistoryPrepend } from '../lib/history-sync'
import { messageTextForClipboard, parseReplyForDisplay, type DisplayReplyQuote, type PendingReplyQuote } from '../lib/reply-quote'
import { MarkdownTable } from './markdown-table'
import { copyTextToClipboard } from '../lib/copy-to-clipboard'
import { messageGroupClass, messageGroupPosition } from '../lib/message-group'

import { formatMessageTimestamp, formatMessageTimestampTitle } from '../lib/format-message-time'

/** Module-scoped so ThreadPane re-renders do not remount every markdown message. */
const MarkdownText = makeMarkdownText({
  remarkPlugins: [remarkGfm],
  components: {
    img: ({ alt, className, ...props }) => (
      <img
        {...props}
        alt={alt ?? ''}
        className={['my-2 max-h-[70vh] max-w-full rounded-lg', className].filter(Boolean).join(' ')}
        loading="lazy"
      />
    ),
    a: (props) => {
      const mergedRel = [props.rel, 'noopener', 'noreferrer'].filter(Boolean).join(' ')
      return <a {...props} target="_blank" rel={mergedRel} />
    },
    table: MarkdownTable,
  },
})

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

type ScrollAnchor = { scrollTop: number; scrollHeight: number }

function captureScrollAnchor(el: HTMLElement): ScrollAnchor {
  return { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight }
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
  const formatted = createdAt ? formatMessageTimestamp(createdAt) : ''
  const title = createdAt ? formatMessageTimestampTitle(createdAt) : undefined
  return (
    <div
      className={align === 'right' ? 'mc-msg-time mc-msg-time-right' : 'mc-msg-time'}
      title={title}
    >
      {formatted}
    </div>
  )
}

type ThreadPaneUiContextValue = {
  bookmarkedMessageIds?: Set<string>
  onToggleBookmark?: (messageId: string, role: 'user' | 'assistant') => void
  onReplyToMessage?: (messageId: string) => void | Promise<void>
  onDeleteMessage?: (messageId: string) => void | Promise<void>
  onSaveMessageEdit?: (messageId: string, content: string) => void | Promise<void>
  onOpenSubthread?: (messageId: string) => void | Promise<void>
  editingMessageId?: string | null
  onEditingMessageIdChange?: (messageId: string | null) => void
  activeSubthreadMessageId?: string | null
  pendingReply?: PendingReplyQuote | null
  onDismissPendingReply?: () => void
  draftText: string
  onDraftTextChange?: (text: string) => void
  uploadHint?: string
  mobileActionMessageId?: string | null
  onMobileMessageTap?: (messageId: string) => void
}

const ThreadPaneUiContext = React.createContext<ThreadPaneUiContextValue>({
  bookmarkedMessageIds: undefined,
  onToggleBookmark: undefined,
  onReplyToMessage: undefined,
  onDeleteMessage: undefined,
  onSaveMessageEdit: undefined,
  onOpenSubthread: undefined,
  editingMessageId: null,
  onEditingMessageIdChange: undefined,
  activeSubthreadMessageId: null,
  pendingReply: null,
  onDismissPendingReply: undefined,
  draftText: '',
  onDraftTextChange: undefined,
  uploadHint: undefined,
  mobileActionMessageId: null,
  onMobileMessageTap: undefined,
})

function ComposerQuotePreview() {
  const { pendingReply, onDismissPendingReply } = React.useContext(ThreadPaneUiContext)
  if (!pendingReply) return null
  return (
    <SentReplyQuoteChip
      quote={pendingReply}
      onDismiss={onDismissPendingReply}
      className="mc-reply-quote-composer"
    />
  )
}

function replyQuoteLabel(quote: { isFromBot?: boolean; role?: 'user' | 'assistant'; senderName?: string; sender?: string }): string {
  const isBot = quote.isFromBot ?? quote.role === 'assistant'
  if (isBot) return 'assistant'
  const name = (quote.senderName ?? quote.sender ?? '').trim()
  return name || 'user'
}

function SentReplyQuoteChip({
  quote,
  onDismiss,
  className,
}: {
  quote: PendingReplyQuote | DisplayReplyQuote
  onDismiss?: () => void
  className?: string
}) {
  const label = replyQuoteLabel(
    'isFromBot' in quote
      ? quote
      : { role: quote.role, sender: quote.sender },
  )
  const snippet = quote.snippet
  return (
    <div
      className={['mc-reply-quote-preview', className].filter(Boolean).join(' ')}
      role="note"
      aria-label={`Replying to ${label}`}
    >
      <div className="mc-reply-quote-bar" aria-hidden />
      <div className="mc-reply-quote-body">
        <div className="mc-reply-quote-label">Replying to {label}</div>
        <div className="mc-reply-quote-snippet">{snippet}</div>
      </div>
      {onDismiss ? (
        <button
          type="button"
          className="mc-reply-quote-dismiss"
          onClick={onDismiss}
          aria-label="Remove quoted message"
          title="Remove quote"
        >
          ×
        </button>
      ) : null}
    </div>
  )
}

function messageTextContent(content: unknown, joiner = ''): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .filter((part): part is { type: 'text'; text: string } => {
        return typeof part === 'object' && part !== null && (part as { type?: string }).type === 'text'
      })
      .map((part) => part.text)
      .join(joiner)
  }
  return ''
}

function getMessageClipboardText(content: unknown): string {
  return messageTextForClipboard(messageTextContent(content, '\n'))
}

function UserMessageDisplayBody() {
  const rawText = useMessage((m) => messageTextContent(m.content))
  const parsed = React.useMemo(() => parseReplyForDisplay(rawText), [rawText])
  if (!parsed) {
    return <UserMessage.Content />
  }
  return (
    <div className="aui-user-message-content">
      <SentReplyQuoteChip quote={parsed.quote} className="mc-reply-quote-sent" />
      {parsed.followUp.trim() ? <div className="mc-reply-follow-up">{parsed.followUp}</div> : null}
    </div>
  )
}

function MessageMobileActionSheet({ role }: { role: 'user' | 'assistant' }) {
  const {
    mobileActionMessageId,
    bookmarkedMessageIds,
    onToggleBookmark,
    onReplyToMessage,
    onDeleteMessage,
    onSaveMessageEdit,
    onOpenSubthread,
    onEditingMessageIdChange,
    onMobileMessageTap,
  } = React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const isBookmarked = useMessage((m) => {
    const id = typeof m.id === 'string' ? m.id : ''
    return id.length > 0 && (bookmarkedMessageIds?.has(id) ?? false)
  })
  if (!messageId || mobileActionMessageId !== messageId) return null
  return (
    <div className="mc-msg-mobile-actions" role="toolbar" aria-label="Message actions">
      <MessageCopyButton showLabel />
      {onReplyToMessage ? (
        <button
          type="button"
          className="mc-msg-action-btn cursor-pointer"
          onClick={() => {
            onReplyToMessage(messageId)
            onMobileMessageTap?.('')
          }}
          title="Reply with quote"
          aria-label="Reply with quote"
        >
          <IconReply />
          <span className="mc-msg-action-label">Reply</span>
        </button>
      ) : null}
      {role === 'assistant' && onSaveMessageEdit && onEditingMessageIdChange ? (
        <button
          type="button"
          className="mc-msg-action-btn cursor-pointer"
          onClick={() => {
            onEditingMessageIdChange(messageId)
            onMobileMessageTap?.('')
          }}
          title="Edit message"
          aria-label="Edit message"
        >
          <IconPencil />
          <span className="mc-msg-action-label">Edit</span>
        </button>
      ) : null}
      {role === 'assistant' && onOpenSubthread ? (
        <button
          type="button"
          className="mc-msg-action-btn cursor-pointer"
          onClick={() => {
            onOpenSubthread(messageId)
            onMobileMessageTap?.('')
          }}
          title="Open side chat"
          aria-label="Open side chat"
        >
          <IconSideChat />
          <span className="mc-msg-action-label">Side chat</span>
        </button>
      ) : null}
      {onToggleBookmark ? (
        <button
          type="button"
          className="mc-bookmark-btn cursor-pointer"
          onClick={() => onToggleBookmark(messageId, role)}
          aria-label={isBookmarked ? 'Remove bookmark' : 'Bookmark message'}
          title={isBookmarked ? 'Remove bookmark' : 'Bookmark message'}
        >
          <IconStar filled={isBookmarked} />
          <span className="mc-msg-action-label">{isBookmarked ? 'Saved' : 'Save'}</span>
        </button>
      ) : null}
      {onDeleteMessage ? (
        <button
          type="button"
          className="mc-msg-action-btn mc-msg-action-btn-danger cursor-pointer"
          onClick={() => {
            onDeleteMessage(messageId)
            onMobileMessageTap?.('')
          }}
          title="Delete message"
          aria-label="Delete message"
        >
          <IconTrash />
          <span className="mc-msg-action-label">Delete</span>
        </button>
      ) : null}
      <button
        type="button"
        className="mc-msg-action-btn mc-msg-action-btn-muted cursor-pointer"
        onClick={() => onMobileMessageTap?.('')}
        aria-label="Close actions"
        title="Close"
      >
        <span className="mc-msg-action-label">Close</span>
      </button>
    </div>
  )
}

function useMobileMessageTapProps(messageId: string) {
  const { onMobileMessageTap } = React.useContext(ThreadPaneUiContext)
  const longPressRef = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const pointerStartRef = React.useRef<{ x: number; y: number } | null>(null)

  const clearLongPress = React.useCallback(() => {
    if (longPressRef.current !== null) {
      clearTimeout(longPressRef.current)
      longPressRef.current = null
    }
    pointerStartRef.current = null
  }, [])

  React.useEffect(() => () => clearLongPress(), [clearLongPress])

  return {
    onPointerDown: (e: React.PointerEvent) => {
      if (typeof window !== 'undefined' && window.matchMedia('(min-width: 768px)').matches) return
      const target = e.target as HTMLElement
      if (target.closest('button, a, input, textarea, [role="toolbar"], pre, code')) return
      if (!messageId || !onMobileMessageTap) return
      pointerStartRef.current = { x: e.clientX, y: e.clientY }
      clearLongPress()
      longPressRef.current = setTimeout(() => {
        onMobileMessageTap(messageId)
        clearLongPress()
      }, 450)
    },
    onPointerMove: (e: React.PointerEvent) => {
      const start = pointerStartRef.current
      if (!start) return
      const dx = e.clientX - start.x
      const dy = e.clientY - start.y
      if (dx * dx + dy * dy > 100) clearLongPress()
    },
    onPointerUp: () => {
      clearLongPress()
    },
    onPointerCancel: () => {
      clearLongPress()
    },
  }
}

function useMessageGroupProps(role: 'user' | 'assistant') {
  const groupPosition = useAuiState(({ thread, message }) =>
    messageGroupPosition(thread.messages, message.index),
  )
  return {
    'data-group': groupPosition,
    className: messageGroupClass(groupPosition, role),
  } as const
}

function MessageCopyButton({ showLabel = false }: { showLabel?: boolean }) {
  const textToCopy = useMessage((m) => {
    if (m.role === 'assistant' && m.status?.type === 'running') return ''
    return getMessageClipboardText(m.content)
  })
  const [copied, setCopied] = React.useState(false)
  const [failed, setFailed] = React.useState(false)
  if (!textToCopy.trim()) return null

  const label = copied ? 'Copied' : failed ? 'Copy failed' : 'Copy message'
  const shortLabel = copied ? 'Copied' : failed ? 'Failed' : 'Copy'

  return (
    <button
      type="button"
      className="mc-msg-action-btn"
      onClick={() => {
        void copyTextToClipboard(textToCopy).then((ok) => {
          if (ok) {
            setCopied(true)
            setFailed(false)
            window.setTimeout(() => setCopied(false), 2000)
            return
          }
          setFailed(true)
          window.setTimeout(() => setFailed(false), 2500)
        })
      }}
      title={label}
      aria-label={label}
    >
      <IconCopy className={copied ? 'mc-msg-action-icon-success' : undefined} />
      {showLabel ? <span className="mc-msg-action-label">{shortLabel}</span> : null}
    </button>
  )
}

function MessageBookmarkButton({
  role,
}: {
  role: 'user' | 'assistant'
}) {
  const { bookmarkedMessageIds, onToggleBookmark } = React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const isBookmarked = useMessage((m) => {
    const id = typeof m.id === 'string' ? m.id : ''
    return id.length > 0 && (bookmarkedMessageIds?.has(id) ?? false)
  })
  if (!onToggleBookmark || !messageId) return null
  return (
    <button
      type="button"
      className="mc-bookmark-btn"
      onClick={() => onToggleBookmark(messageId, role)}
      title={isBookmarked ? 'Remove bookmark' : 'Bookmark this bubble'}
      aria-label={isBookmarked ? 'Remove bookmark' : 'Bookmark message'}
    >
      <IconStar filled={isBookmarked} />
    </button>
  )
}

function MessageReplyButton({ showLabel = false }: { showLabel?: boolean }) {
  const { onReplyToMessage } = React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  if (!onReplyToMessage || !messageId) return null
  return (
    <button
      type="button"
      className="mc-msg-action-btn"
      onClick={() => onReplyToMessage(messageId)}
      title="Reply with quote"
      aria-label="Reply with quote"
    >
      {showLabel ? 'Reply' : <IconReply />}
    </button>
  )
}

function MessageDeleteButton({ showLabel = false }: { showLabel?: boolean }) {
  const { onDeleteMessage } = React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  if (!onDeleteMessage || !messageId) return null
  return (
    <button
      type="button"
      className="mc-msg-action-btn mc-msg-action-btn-danger"
      onClick={() => onDeleteMessage(messageId)}
      title="Delete message"
      aria-label="Delete message"
    >
      {showLabel ? 'Delete' : <IconTrash />}
    </button>
  )
}

function MessageEditButton({ showLabel = false }: { showLabel?: boolean }) {
  const { onSaveMessageEdit, onEditingMessageIdChange, editingMessageId } =
    React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const isRunning = useMessage((m) => m.role === 'assistant' && m.status?.type === 'running')
  if (!onSaveMessageEdit || !onEditingMessageIdChange || !messageId || isRunning) return null
  const active = editingMessageId === messageId
  return (
    <button
      type="button"
      className="mc-msg-action-btn"
      onClick={() => onEditingMessageIdChange(active ? null : messageId)}
      title={active ? 'Cancel edit' : 'Edit message'}
      aria-label={active ? 'Cancel edit' : 'Edit message'}
      aria-pressed={active}
    >
      {showLabel ? (active ? 'Cancel' : 'Edit') : <IconPencil />}
    </button>
  )
}

function MessageSideChatButton({ showLabel = false }: { showLabel?: boolean }) {
  const { onOpenSubthread, activeSubthreadMessageId } = React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const isRunning = useMessage((m) => m.role === 'assistant' && m.status?.type === 'running')
  if (!onOpenSubthread || !messageId || isRunning) return null
  const active = activeSubthreadMessageId === messageId
  return (
    <button
      type="button"
      className={active ? 'mc-msg-action-btn mc-msg-action-btn-active' : 'mc-msg-action-btn'}
      onClick={() => onOpenSubthread(messageId)}
      title="Open side chat"
      aria-label="Open side chat"
      aria-pressed={active}
    >
      {showLabel ? 'Side chat' : <IconSideChat />}
    </button>
  )
}

function MessageActionBar({
  role,
  showLabels = false,
}: {
  role: 'user' | 'assistant'
  showLabels?: boolean
}) {
  const groupPosition = useAuiState(({ thread, message }) =>
    messageGroupPosition(thread.messages, message.index),
  )
  const showTimestamp = groupPosition === 'single' || groupPosition === 'end'

  return (
    <div
      className={[
        'mc-msg-footer',
        role === 'user' ? 'mc-msg-footer-user' : 'mc-msg-footer-assistant',
      ].join(' ')}
    >
      <div className="mc-msg-actions mc-msg-meta-row-desktop" role="toolbar" aria-label="Message actions">
        <MessageCopyButton showLabel={showLabels} />
        <MessageReplyButton showLabel={showLabels} />
        {role === 'assistant' ? <MessageEditButton showLabel={showLabels} /> : null}
        {role === 'assistant' ? <MessageSideChatButton showLabel={showLabels} /> : null}
        <MessageBookmarkButton role={role} />
        <MessageDeleteButton showLabel={showLabels} />
      </div>
      {showTimestamp ? (
        <MessageTimestamp align={role === 'user' ? 'right' : 'left'} />
      ) : null}
    </div>
  )
}

function AssistantInlineEditor() {
  const { editingMessageId, onEditingMessageIdChange, onSaveMessageEdit } =
    React.useContext(ThreadPaneUiContext)
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const initialText = useMessage((m) => messageTextContent(m.content, '\n'))
  const [draft, setDraft] = React.useState(initialText)
  const [saving, setSaving] = React.useState(false)
  const [saveError, setSaveError] = React.useState('')

  React.useEffect(() => {
    setDraft(initialText)
    setSaveError('')
  }, [initialText, messageId])

  if (!messageId || editingMessageId !== messageId || !onSaveMessageEdit) return null

  const cancel = () => {
    onEditingMessageIdChange?.(null)
    setDraft(initialText)
    setSaveError('')
  }

  const save = async () => {
    if (!draft.trim() || saving) return
    setSaving(true)
    setSaveError('')
    try {
      await onSaveMessageEdit(messageId, draft)
      onEditingMessageIdChange?.(null)
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="mc-inline-editor">
      <textarea
        className="mc-inline-editor-textarea"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        rows={Math.min(16, Math.max(4, draft.split('\n').length + 1))}
        disabled={saving}
        aria-label="Edit assistant message"
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
            e.preventDefault()
            void save()
          }
          if (e.key === 'Escape') {
            e.preventDefault()
            cancel()
          }
        }}
      />
      <div className="mc-inline-editor-actions">
        <button type="button" className="mc-inline-editor-btn" onClick={cancel} disabled={saving}>
          Cancel
        </button>
        <button
          type="button"
          className="mc-inline-editor-btn mc-inline-editor-btn-primary"
          onClick={() => void save()}
          disabled={saving || !draft.trim() || draft.trim() === initialText.trim()}
        >
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
      {saveError ? <div className="mc-inline-editor-error">{saveError}</div> : null}
      <div className="mc-inline-editor-hint">Ctrl/Cmd+Enter to save · Esc to cancel</div>
    </div>
  )
}

function CustomAssistantMessage() {
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const mobileTap = useMobileMessageTapProps(messageId)
  const groupProps = useMessageGroupProps('assistant')
  const { editingMessageId, activeSubthreadMessageId } = React.useContext(ThreadPaneUiContext)
  const isEditing = Boolean(messageId && editingMessageId === messageId)
  const isSubthreadAnchor = Boolean(messageId && activeSubthreadMessageId === messageId)
  const hasRenderableContent = useMessage((m) =>
    Array.isArray(m.content)
      ? m.content.some((part) => {
        if (part.type === 'text') return Boolean(part.text?.trim())
        return part.type === 'tool-call'
      })
      : false,
  )

  return (
    <AssistantMessage.Root
      data-message-id={messageId || undefined}
      data-subthread-anchor={isSubthreadAnchor ? 'true' : undefined}
      {...groupProps}
      {...mobileTap}
      className={[
        groupProps.className,
        isSubthreadAnchor ? 'mc-msg-subthread-anchor' : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      {isEditing ? (
        <AssistantInlineEditor />
      ) : hasRenderableContent ? (
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
      {!isEditing ? <MessageActionBar role="assistant" /> : null}
      <MessageMobileActionSheet role="assistant" />
    </AssistantMessage.Root>
  )
}

function CustomUserMessage() {
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const mobileTap = useMobileMessageTapProps(messageId)
  const groupProps = useMessageGroupProps('user')
  return (
    <UserMessage.Root data-message-id={messageId || undefined} {...groupProps} {...mobileTap}>
      <UserMessage.Attachments />
      <MessagePrimitive.If hasContent>
        <div className="mc-user-content-wrap">
          <UserMessageDisplayBody />
        </div>
        <MessageActionBar role="user" />
        <MessageMobileActionSheet role="user" />
      </MessagePrimitive.If>
      <BranchPicker />
    </UserMessage.Root>
  )
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

export type ThreadPaneProps = {
  adapter: ChatModelAdapter
  initialMessages: ThreadMessageLike[]
  runtimeKey: string
  draftText: string
  /** If true, avoid resetting thread runtime while new assistant text is streaming in. */
  isStreaming?: boolean
  /** If true, show a loading indicator while initial chat history is being fetched. */
  historyLoading?: boolean
  /** When true, older messages exist above the current window. */
  historyHasMore?: boolean
  historyLoadingMore?: boolean
  onLoadMoreHistory?: () => void | Promise<void>
  onDraftTextChange?: (text: string) => void
  bookmarkedMessageIds?: Set<string>
  onToggleBookmark?: (messageId: string, role: 'user' | 'assistant') => void
  onReplyToMessage?: (messageId: string) => void | Promise<void>
  onDeleteMessage?: (messageId: string) => void | Promise<void>
  onSaveMessageEdit?: (messageId: string, content: string) => void | Promise<void>
  onOpenSubthread?: (messageId: string) => void | Promise<void>
  editingMessageId?: string | null
  onEditingMessageIdChange?: (messageId: string | null) => void
  activeSubthreadMessageId?: string | null
  pendingReply?: PendingReplyQuote | null
  onDismissPendingReply?: () => void
  /** Mobile (max-width 767px): report scroll direction so the app shell can collapse the main header. */
  onMobileThreadScroll?: (opts: {
    collapseHeader: boolean
    source: 'scroll' | 'reset' | 'focus' | 'media-change'
    scrollTop?: number
  }) => void
  /** Shown under the composer during multipart uploads (e.g. "Uploading photo.png (10.2 MB)…"). */
  uploadHint?: string
  onShowShortcuts?: () => void
}

function DraftAwareComposer() {
  const { draftText, onDraftTextChange, uploadHint } = React.useContext(ThreadPaneUiContext)
  const aui = useAui()
  const composerText = useAuiState(({ composer }) => composer.text)
  const lastAppliedDraftRef = React.useRef<string | null>(null)

  React.useEffect(() => {
    if (lastAppliedDraftRef.current === draftText) return
    aui.composer().setText(draftText)
    lastAppliedDraftRef.current = draftText
  }, [aui, draftText])

  React.useEffect(() => {
    onDraftTextChange?.(composerText)
  }, [composerText, onDraftTextChange])

  return (
    <ComposerPrimitive.AttachmentDropzone className="mc-composer-dropzone">
      <ComposerQuotePreview />
      {/* Custom composer so scroll-to-latest does not focus the textarea (opens mobile keyboard). */}
      <Composer.Root>
        <Composer.Attachments />
        <Composer.AddAttachment />
        <Composer.Input autoFocus unstable_focusOnScrollToBottom={false} />
        <Composer.Action />
      </Composer.Root>
      {uploadHint ? (
        <div className="mc-upload-hint" aria-live="polite">
          {uploadHint}
        </div>
      ) : (
        <div className="mc-upload-hint mc-upload-hint-idle">
          Drop files here or use the attach button
        </div>
      )}
    </ComposerPrimitive.AttachmentDropzone>
  )
}

/** Isolated from App re-renders (persona poll, queue lane, schedules, etc.). `useLocalRuntime` runs an effect after every render that touches options/load; re-rendering on unrelated parent state was resetting the composer and scroll. */
export const ThreadPane = React.memo(function ThreadPane({
  adapter,
  initialMessages,
  runtimeKey,
  draftText,
  isStreaming = false,
  historyLoading = false,
  historyHasMore = false,
  historyLoadingMore = false,
  onLoadMoreHistory,
  onDraftTextChange,
  bookmarkedMessageIds,
  onToggleBookmark,
  onReplyToMessage,
  onDeleteMessage,
  onSaveMessageEdit,
  onOpenSubthread,
  editingMessageId = null,
  onEditingMessageIdChange,
  activeSubthreadMessageId = null,
  pendingReply,
  onDismissPendingReply,
  onMobileThreadScroll,
  uploadHint,
  onShowShortcuts,
}: ThreadPaneProps) {
  const [mobileActionMessageId, setMobileActionMessageId] = React.useState<string | null>(null)
  const onMobileMessageTap = React.useCallback((messageId: string) => {
    setMobileActionMessageId((prev) => (messageId && prev === messageId ? null : messageId || null))
  }, [])
  const runtime = useLocalRuntime(adapter, {
    initialMessages,
    maxSteps: 100,
    adapters: {
      attachments: webAttachmentAdapter,
    },
  })
  const lastInitialMessagesRef = React.useRef<ThreadMessageLike[]>(initialMessages)
  const lastRuntimeKeyRef = React.useRef(runtimeKey)
  const viewportRef = React.useRef<HTMLDivElement | null>(null)
  const viewportScrollCleanupRef = React.useRef<(() => void) | null>(null)
  const lastViewportScrollTopRef = React.useRef(0)
  const scrollGuardUntilRef = React.useRef(0)
  const pendingScrollRestoreRef = React.useRef<ScrollAnchor | null>(null)
  React.useLayoutEffect(() => {
    const runtimeKeyChanged = lastRuntimeKeyRef.current !== runtimeKey
    if (!runtimeKeyChanged && isStreaming) return
    if (
      !runtimeKeyChanged
      && historiesEqual(lastInitialMessagesRef.current, initialMessages)
    ) {
      return
    }
    const prev = lastInitialMessagesRef.current
    const prepend =
      !runtimeKeyChanged && isHistoryPrepend(prev, initialMessages)
    if (prepend && viewportRef.current) {
      pendingScrollRestoreRef.current = captureScrollAnchor(viewportRef.current)
    } else {
      pendingScrollRestoreRef.current = null
    }
    runtime.thread.reset(initialMessages)
    lastInitialMessagesRef.current = initialMessages
    lastRuntimeKeyRef.current = runtimeKey
  }, [initialMessages, runtime, runtimeKey, isStreaming])

  React.useLayoutEffect(() => {
    const anchor = pendingScrollRestoreRef.current
    const el = viewportRef.current
    if (!anchor || !el) return
    const apply = () => {
      const heightDelta = el.scrollHeight - anchor.scrollHeight
      if (heightDelta <= 0) return false
      el.scrollTop = anchor.scrollTop + heightDelta
      pendingScrollRestoreRef.current = null
      scrollGuardUntilRef.current = Date.now() + 700
      lastViewportScrollTopRef.current = el.scrollTop
      return true
    }
    if (apply()) return
    requestAnimationFrame(() => {
      if (apply()) return
      requestAnimationFrame(apply)
    })
  }, [initialMessages])

  React.useEffect(() => {
    const anchor = pendingScrollRestoreRef.current
    const el = viewportRef.current
    if (!anchor || !el) return
    const heightDelta = el.scrollHeight - anchor.scrollHeight
    if (heightDelta <= 0) return
    el.scrollTop = anchor.scrollTop + heightDelta
    pendingScrollRestoreRef.current = null
    scrollGuardUntilRef.current = Date.now() + 700
    lastViewportScrollTopRef.current = el.scrollTop
  }, [initialMessages, historyLoadingMore])
  const uiContextValue = React.useMemo<ThreadPaneUiContextValue>(
    () => ({
      bookmarkedMessageIds,
      onToggleBookmark,
      onReplyToMessage,
      onDeleteMessage,
      onSaveMessageEdit,
      onOpenSubthread,
      editingMessageId,
      onEditingMessageIdChange,
      activeSubthreadMessageId,
      pendingReply,
      onDismissPendingReply,
      draftText,
      onDraftTextChange,
      uploadHint,
      mobileActionMessageId,
      onMobileMessageTap,
    }),
    [
      activeSubthreadMessageId,
      bookmarkedMessageIds,
      draftText,
      editingMessageId,
      mobileActionMessageId,
      onMobileMessageTap,
      onDeleteMessage,
      onDismissPendingReply,
      onDraftTextChange,
      onEditingMessageIdChange,
      onOpenSubthread,
      onReplyToMessage,
      onSaveMessageEdit,
      onToggleBookmark,
      pendingReply,
      uploadHint,
    ],
  )

  const bindThreadViewport = React.useCallback(
    (el: HTMLDivElement | null) => {
      viewportRef.current = el
      viewportScrollCleanupRef.current?.()
      viewportScrollCleanupRef.current = null
      if (!el || !onMobileThreadScroll) return

      const mq = window.matchMedia('(max-width: 767px)')
      lastViewportScrollTopRef.current = el.scrollTop
      scrollGuardUntilRef.current = Date.now() + 550

      const onScroll = () => {
        if (!mq.matches) {
          onMobileThreadScroll({ collapseHeader: false, source: 'media-change', scrollTop: el.scrollTop })
          return
        }
        if (Date.now() < scrollGuardUntilRef.current) {
          return
        }
        const st = el.scrollTop
        const delta = st - lastViewportScrollTopRef.current
        lastViewportScrollTopRef.current = st
        if (st < 28) {
          onMobileThreadScroll({ collapseHeader: false, source: 'scroll', scrollTop: st })
          return
        }
        if (delta > 14) {
          onMobileThreadScroll({ collapseHeader: true, source: 'scroll', scrollTop: st })
        } else if (delta < -12) {
          onMobileThreadScroll({ collapseHeader: false, source: 'scroll', scrollTop: st })
        }
      }

      const onMqChange = () => {
        if (!mq.matches) {
          onMobileThreadScroll({ collapseHeader: false, source: 'media-change', scrollTop: el.scrollTop })
        }
      }

      el.addEventListener('scroll', onScroll, { passive: true })
      mq.addEventListener('change', onMqChange)
      viewportScrollCleanupRef.current = () => {
        el.removeEventListener('scroll', onScroll)
        mq.removeEventListener('change', onMqChange)
      }
    },
    [onMobileThreadScroll],
  )

  React.useEffect(() => {
    scrollGuardUntilRef.current = Date.now() + 700
    onMobileThreadScroll?.({ collapseHeader: false, source: 'reset' })
  }, [runtimeKey, onMobileThreadScroll])

  React.useEffect(
    () => () => {
      viewportScrollCleanupRef.current?.()
      viewportScrollCleanupRef.current = null
    },
    [],
  )

  return (
    <ThreadPaneUiContext.Provider value={uiContextValue}>
      <AssistantRuntimeProvider key={runtimeKey} runtime={runtime}>
        <Thread.Root
          config={{
            assistantMessage: {
              allowReload: false,
              allowSpeak: false,
              allowFeedbackNegative: false,
              allowFeedbackPositive: false,
              components: {
                Text: MarkdownText,
                ToolFallback: ToolCallCard,
              },
            },
            userMessage: { allowEdit: false },
            composer: { allowAttachments: true },
            components: {
              Composer: DraftAwareComposer,
              AssistantMessage: CustomAssistantMessage,
              UserMessage: CustomUserMessage,
            },
            strings: {
              composer: {
                input: { placeholder: 'Message FinallyAValueBot...' },
              },
            },
            assistantAvatar: {},
          }}
          className="h-full min-h-0 min-w-0"
        >
          <div className="mc-thread-shell flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
            {historyLoading ? (
              <ThreadHistorySkeleton />
            ) : (
            <Thread.Viewport ref={bindThreadViewport} className="aui-thread-viewport mc-thread-viewport">
              {historyHasMore && onLoadMoreHistory ? (
                <LoadEarlierMessages
                  loading={historyLoadingMore}
                  onLoadMore={() => void onLoadMoreHistory()}
                />
              ) : null}
              <ThreadWelcomeHints onShowShortcuts={onShowShortcuts} />
              <Thread.Messages
                components={{
                  AssistantMessage: CustomAssistantMessage,
                  UserMessage: CustomUserMessage,
                }}
              />
              <Thread.FollowupSuggestions />
            </Thread.Viewport>
            )}
            <div className="mc-thread-composer-stack">
              {!historyLoading ? (
                <div className="mc-scroll-to-latest-wrap">
                  <ScrollToLatest />
                </div>
              ) : null}
              <div
                className="mc-thread-composer-dock"
                onFocusCapture={() =>
                  onMobileThreadScroll?.({ collapseHeader: false, source: 'focus' })
                }
              >
                <div className="relative mx-auto w-full max-w-[var(--aui-thread-max-width)] px-2 pb-1 pt-1 md:px-3">
                  <DraftAwareComposer />
                </div>
              </div>
            </div>
          </div>
        </Thread.Root>
      </AssistantRuntimeProvider>
    </ThreadPaneUiContext.Provider>
  )
})
