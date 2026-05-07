import React from 'react'
import {
  AssistantRuntimeProvider,
  CompositeAttachmentAdapter,
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
import {
  AssistantActionBar,
  AssistantMessage,
  BranchPicker,
  Composer,
  Thread,
  ThreadWelcome,
  UserActionBar,
  UserMessage,
  makeMarkdownText,
} from '@assistant-ui/react-ui'
import remarkGfm from 'remark-gfm'

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

type ThreadPaneUiContextValue = {
  bookmarkedMessageIds?: Set<string>
  onToggleBookmark?: (messageId: string, role: 'user' | 'assistant') => void
  draftText: string
  onDraftTextChange?: (text: string) => void
}

const ThreadPaneUiContext = React.createContext<ThreadPaneUiContextValue>({
  bookmarkedMessageIds: undefined,
  onToggleBookmark: undefined,
  draftText: '',
  onDraftTextChange: undefined,
})

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
      {isBookmarked ? '★' : '☆'}
    </button>
  )
}

function CustomAssistantMessage() {
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  const hasRenderableContent = useMessage((m) =>
    Array.isArray(m.content)
      ? m.content.some((part) => {
        if (part.type === 'text') return Boolean(part.text?.trim())
        return part.type === 'tool-call'
      })
      : false,
  )

  return (
    <AssistantMessage.Root data-message-id={messageId || undefined}>
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
      <div className="mc-msg-meta-row">
        <MessageBookmarkButton role="assistant" />
        <AssistantActionBar />
        <MessageTimestamp align="left" />
      </div>
    </AssistantMessage.Root>
  )
}

function CustomUserMessage() {
  const messageId = useMessage((m) => (typeof m.id === 'string' ? m.id : ''))
  return (
    <UserMessage.Root data-message-id={messageId || undefined}>
      <UserMessage.Attachments />
      <MessagePrimitive.If hasContent>
        <div className="mc-msg-meta-row mc-msg-meta-row-user">
          <MessageBookmarkButton role="user" />
          <UserActionBar />
        </div>
        <div className="mc-user-content-wrap">
          <UserMessage.Content />
          <MessageTimestamp align="right" />
        </div>
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
  onDraftTextChange?: (text: string) => void
  bookmarkedMessageIds?: Set<string>
  onToggleBookmark?: (messageId: string, role: 'user' | 'assistant') => void
  /** Mobile (max-width 767px): report scroll direction so the app shell can collapse the main header. */
  onMobileThreadScroll?: (opts: {
    collapseHeader: boolean
    source: 'scroll' | 'reset' | 'focus' | 'media-change'
    scrollTop?: number
  }) => void
}

function DraftAwareComposer() {
  const { draftText, onDraftTextChange } = React.useContext(ThreadPaneUiContext)
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

  return <Composer />
}

/** Isolated from App re-renders (persona poll, queue lane, schedules, etc.). `useLocalRuntime` runs an effect after every render that touches options/load; re-rendering on unrelated parent state was resetting the composer and scroll. */
export const ThreadPane = React.memo(function ThreadPane({
  adapter,
  initialMessages,
  runtimeKey,
  draftText,
  onDraftTextChange,
  bookmarkedMessageIds,
  onToggleBookmark,
  onMobileThreadScroll,
}: ThreadPaneProps) {
  const MarkdownText = makeMarkdownText({
    remarkPlugins: [remarkGfm],
    components: {
      a: (props) => {
        const mergedRel = [props.rel, 'noopener', 'noreferrer'].filter(Boolean).join(' ')
        return <a {...props} target="_blank" rel={mergedRel} />
      },
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
  const lastInitialMessagesRef = React.useRef<ThreadMessageLike[]>(initialMessages)
  React.useEffect(() => {
    if (lastInitialMessagesRef.current === initialMessages) return
    runtime.thread.reset(initialMessages)
    lastInitialMessagesRef.current = initialMessages
  }, [initialMessages, runtime])
  const uiContextValue = React.useMemo<ThreadPaneUiContextValue>(
    () => ({
      bookmarkedMessageIds,
      onToggleBookmark,
      draftText,
      onDraftTextChange,
    }),
    [bookmarkedMessageIds, draftText, onDraftTextChange, onToggleBookmark],
  )

  const viewportScrollCleanupRef = React.useRef<(() => void) | null>(null)
  const lastViewportScrollTopRef = React.useRef(0)
  const scrollGuardUntilRef = React.useRef(0)

  const bindThreadViewport = React.useCallback(
    (el: HTMLDivElement | null) => {
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
              allowCopy: false,
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
            <Thread.Viewport ref={bindThreadViewport} className="aui-thread-viewport mc-thread-viewport">
              <ThreadWelcome />
              <Thread.Messages
                components={{
                  AssistantMessage: CustomAssistantMessage,
                  UserMessage: CustomUserMessage,
                }}
              />
              <Thread.FollowupSuggestions />
            </Thread.Viewport>
            <div
              className="mc-thread-composer-dock"
              onFocusCapture={() =>
                onMobileThreadScroll?.({ collapseHeader: false, source: 'focus' })
              }
            >
              <div className="relative mx-auto w-full max-w-[var(--aui-thread-max-width)] px-2 pb-1 pt-1 md:px-3">
                <Thread.ScrollToBottom />
                <DraftAwareComposer />
              </div>
            </div>
          </div>
        </Thread.Root>
      </AssistantRuntimeProvider>
    </ThreadPaneUiContext.Provider>
  )
})
