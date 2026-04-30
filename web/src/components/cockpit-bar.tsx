import React, { useEffect, useId, useRef, useState } from 'react'
import { Button, Dialog, Flex, Text } from '@radix-ui/themes'
import remarkGfm from 'remark-gfm'
import ReactMarkdown from 'react-markdown'
import { api } from '../api/client'
import type {
  BackendMessage,
  InstallationStatus,
  PersonaBulletinFocus,
  PersonaMessageBookmark,
  QueueLane,
} from '../types'

export type CockpitBarProps = {
  appearance: 'dark' | 'light'
  statusText: string
  queueLane: QueueLane | null
  backgroundActiveCount: number
  installationStatus: InstallationStatus | null
  onQueueClick: () => void
  bulletinFocus: PersonaBulletinFocus | null
  bookmarks: PersonaMessageBookmark[]
  /** Used to load full message text for the bookmark reader. */
  activePersonaId: number | null
  /** Persisted removal via DELETE bookmark; return true when the bookmark was removed. */
  onRemoveBookmark?: (messageId: string) => Promise<boolean>
  floating?: boolean
}

/**
 * Operational strip: session activity, queue, background jobs, setup readiness.
 * Collapsed by default; expand from the centered control. Separate from tooling (Settings, etc.).
 */
export function CockpitBar({
  appearance,
  statusText,
  queueLane,
  backgroundActiveCount,
  installationStatus,
  onQueueClick,
  bulletinFocus,
  bookmarks,
  activePersonaId,
  onRemoveBookmark,
  floating = false,
}: CockpitBarProps) {
  const [expanded, setExpanded] = useState(false)
  const [selectedBookmark, setSelectedBookmark] = useState<PersonaMessageBookmark | null>(null)
  const [bookmarkMessage, setBookmarkMessage] = useState<BackendMessage | null>(null)
  const [bookmarkMessageLoading, setBookmarkMessageLoading] = useState(false)
  const [bookmarkMessageError, setBookmarkMessageError] = useState('')
  const [removeBookmarkBusy, setRemoveBookmarkBusy] = useState(false)
  const [removeBookmarkError, setRemoveBookmarkError] = useState('')
  const expandedRootRef = useRef<HTMLDivElement | null>(null)
  const panelId = useId()
  const toggleId = `${panelId}-toggle`
  const isDark = appearance === 'dark'
  const pending = queueLane?.pending ?? 0
  const oldestWaitMs = queueLane?.oldest_wait_ms ?? 0
  const queueError = queueLane?.last_error

  const queueLabel =
    pending > 0
      ? `${pending} pending${oldestWaitMs > 0 ? ` · ${Math.round(oldestWaitMs / 1000)}s wait` : ''}${queueError ? ' (!)' : ''}`
      : `idle${queueError ? ' (!)' : ''}`

  useEffect(() => {
    if (selectedBookmark == null) {
      setBookmarkMessage(null)
      setBookmarkMessageError('')
      setBookmarkMessageLoading(false)
      setRemoveBookmarkBusy(false)
      setRemoveBookmarkError('')
      return
    }
    if (activePersonaId == null) {
      setBookmarkMessage(null)
      setBookmarkMessageError('No active persona')
      setBookmarkMessageLoading(false)
      return
    }
    let cancelled = false
    const mid = selectedBookmark.message_id
    setBookmarkMessage(null)
    setBookmarkMessageError('')
    setBookmarkMessageLoading(true)
    void (async () => {
      try {
        const res = await api<{ message?: BackendMessage }>(
          `/api/personas/${activePersonaId}/messages/${encodeURIComponent(mid)}`,
        )
        if (cancelled) return
        const m = res.message
        if (m && typeof m.content === 'string') {
          setBookmarkMessage(m)
        } else {
          setBookmarkMessageError('Message not found')
        }
      } catch (e) {
        if (cancelled) return
        setBookmarkMessageError(e instanceof Error ? e.message : String(e))
      } finally {
        if (!cancelled) setBookmarkMessageLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [selectedBookmark, activePersonaId])

  useEffect(() => {
    if (!expanded) return
    const onPointerDown = (event: PointerEvent) => {
      if (selectedBookmark != null) return
      const target = event.target as Node | null
      if (!target) return
      if (expandedRootRef.current?.contains(target)) return
      setExpanded(false)
    }
    window.addEventListener('pointerdown', onPointerDown)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown)
    }
  }, [expanded, selectedBookmark])

  const stripClass = floating
    ? isDark
      ? 'rounded-xl border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-main)]/90 shadow-[0_8px_24px_rgba(0,0,0,0.35)] backdrop-blur'
      : 'rounded-xl border border-slate-300/90 bg-white/95 shadow-[0_8px_24px_rgba(15,23,42,0.12)] backdrop-blur'
    : isDark
      ? 'border-t border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-main)]/40'
      : 'border-t border-slate-200/90 bg-slate-50/80'

  if (!expanded) {
    return (
      <button
        id={toggleId}
        type="button"
        className={`mc-cockpit w-full px-4 py-1 ${stripClass} ${
          isDark
            ? 'cursor-pointer text-slate-400 transition-colors hover:text-slate-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[color:var(--mc-accent)]'
            : 'cursor-pointer text-slate-500 transition-colors hover:text-slate-800 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400'
        }`}
        aria-expanded={false}
        aria-controls={panelId}
        title="Show session status"
        onClick={() => setExpanded(true)}
      >
        <span className="sr-only">Show session status</span>
        <div className="flex">
          <span className="mx-auto flex h-7 w-full items-center justify-center rounded-md">
            <svg
              className="size-3.5 shrink-0 transition-transform duration-150"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden
            >
              <path d="M6 9l6 6 6-6" />
            </svg>
          </span>
        </div>
      </button>
    )
  }

  return (
    <div
      ref={expandedRootRef}
      className={`mc-cockpit py-2 ${stripClass}`}
      role="region"
      aria-label="Session status"
    >
      <div className="flex">
        <button
          id={toggleId}
          type="button"
          className={`flex h-7 w-full cursor-pointer items-center justify-center border-0 bg-transparent px-4 transition-colors ${
            isDark
              ? 'text-slate-400 hover:bg-white/5 hover:text-slate-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[color:var(--mc-accent)]'
              : 'text-slate-500 hover:bg-slate-200/60 hover:text-slate-800 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400'
          }`}
          aria-expanded={expanded}
          aria-controls={panelId}
          title="Hide session status"
          onClick={() => setExpanded(false)}
        >
          <span className="sr-only">Hide session status</span>
          <svg
            className="size-3.5 shrink-0 -rotate-180 transition-transform duration-150"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M6 9l6 6 6-6" />
          </svg>
        </button>
      </div>

      {expanded ? (
        <div id={panelId} aria-labelledby={toggleId} className="space-y-2 px-4">
          <Flex
            align="center"
            gap="3"
            wrap="wrap"
            className="min-h-[36px] text-[13px] leading-snug"
          >
            <Text size="1" color="gray" weight="medium" className="shrink-0">
              {statusText}
            </Text>

          <span className={isDark ? 'text-[color:var(--gray-8)]' : 'text-slate-300'} aria-hidden>
            ·
          </span>

          <button
            type="button"
            className={
              isDark
                ? 'm-0 inline-flex cursor-pointer items-center gap-1.5 border-0 bg-transparent p-0 text-left font-inherit text-[13px] text-slate-200 underline-offset-2 hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[color:var(--mc-accent)]'
                : 'm-0 inline-flex cursor-pointer items-center gap-1.5 border-0 bg-transparent p-0 text-left font-inherit text-[13px] text-slate-800 underline-offset-2 hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400'
            }
            title={queueError ?? 'Open run queue'}
            onClick={onQueueClick}
          >
            <span className={isDark ? 'text-slate-400' : 'text-slate-500'}>Queue</span>
            <span
              className={
                pending > 0 || queueError
                  ? isDark
                    ? 'font-medium text-amber-300'
                    : 'font-medium text-amber-900'
                  : isDark
                    ? 'font-medium text-slate-400'
                    : 'font-medium text-slate-500'
              }
            >
              {queueLabel}
            </span>
          </button>

          <span className={isDark ? 'text-[color:var(--gray-8)]' : 'text-slate-300'} aria-hidden>
            ·
          </span>

          <span className="shrink-0">
            <Text size="1" color="gray" weight="medium" as="span">
              Background{' '}
            </Text>
            <Text
              size="1"
              weight="medium"
              as="span"
              color={(backgroundActiveCount > 0 ? 'blue' : 'gray') as never}
            >
              {backgroundActiveCount > 0 ? `${backgroundActiveCount} active` : 'none'}
            </Text>
          </span>

            {installationStatus ? (
              <>
                <span className={isDark ? 'text-[color:var(--gray-8)]' : 'text-slate-300'} aria-hidden>
                  ·
                </span>
                <Flex align="center" gap="2" wrap="wrap" className="min-w-0">
                  <Text size="1" color={installationStatus.llm_ready ? 'green' : 'orange'} weight="medium">
                    LLM {installationStatus.llm_ready ? 'ready' : 'missing'}
                  </Text>
                  <Text size="1" color={installationStatus.channel_ready ? 'green' : 'orange'} weight="medium">
                    Channels {installationStatus.channel_ready ? 'ready' : 'missing'}
                  </Text>
                  {(installationStatus.requires_restart_for_env_changes ??
                    installationStatus.requires_restart_to_apply_runtime_settings) === true ? (
                    <Text size="1" color="orange" weight="medium">
                      Restart needed
                    </Text>
                  ) : null}
                </Flex>
              </>
            ) : (
              <>
                <span className={isDark ? 'text-[color:var(--gray-8)]' : 'text-slate-300'} aria-hidden>
                  ·
                </span>
                <Text size="1" color="gray">
                  Setup loading…
                </Text>
              </>
            )}
          </Flex>
          <div className={isDark ? 'rounded-md border border-[color:var(--mc-border-soft)] p-2' : 'rounded-md border border-slate-300 p-2'}>
            <Text size="1" weight="medium">Bulletin</Text>
            <div className="mt-1 whitespace-pre-wrap text-xs text-[color:var(--gray-11)]">
              {bulletinFocus
                ? `${bulletinFocus.title ? `${bulletinFocus.title}\n` : ''}${bulletinFocus.content}`
                : 'No bulletin focus yet.'}
            </div>
          </div>
          <div className={isDark ? 'rounded-md border border-[color:var(--mc-border-soft)] p-2' : 'rounded-md border border-slate-300 p-2'}>
            <Text size="1" weight="medium">Bookmarks</Text>
            {bookmarks.length > 0 ? (
              <div className="mt-1 flex flex-wrap gap-1.5">
                {bookmarks.slice(0, 8).map((b) => (
                  <button
                    key={b.message_id}
                    type="button"
                    className={
                      isDark
                        ? 'mc-cockpit-bookmark-btn rounded border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] px-2 py-1 text-left text-xs text-slate-200 hover:bg-white/5'
                        : 'mc-cockpit-bookmark-btn rounded border border-slate-300 bg-white px-2 py-1 text-left text-xs text-slate-700 hover:bg-slate-50'
                    }
                    onClick={() => {
                      setBookmarkMessage(null)
                      setBookmarkMessageError('')
                      setBookmarkMessageLoading(true)
                      setRemoveBookmarkError('')
                      setSelectedBookmark(b)
                    }}
                    title="Open bookmark details"
                  >
                    [{b.role}] {b.content_preview}
                  </button>
                ))}
              </div>
            ) : (
              <Text size="1" color="gray" className="block mt-1">
                No bookmarks yet.
              </Text>
            )}
          </div>
        </div>
      ) : null}
      <Dialog.Root open={selectedBookmark != null} onOpenChange={(open) => !open && setSelectedBookmark(null)}>
        <Dialog.Content maxWidth="42rem" className="max-h-[min(85vh,720px)] flex flex-col gap-3">
          <Dialog.Title>Bookmarked message</Dialog.Title>
          {selectedBookmark ? (
            <>
              <Text size="1" color="gray" className="shrink-0">
                {bookmarkMessage && typeof bookmarkMessage.is_from_bot === 'boolean'
                  ? bookmarkMessage.is_from_bot
                    ? 'ASSISTANT'
                    : 'USER'
                  : String(selectedBookmark.role).toUpperCase()}
                {(() => {
                  const ts =
                    (bookmarkMessage?.timestamp && bookmarkMessage.timestamp.trim()) ||
                    selectedBookmark.updated_at ||
                    selectedBookmark.created_at
                  if (!ts) return ''
                  const d = Date.parse(ts)
                  return Number.isFinite(d) ? ` · ${new Date(d).toLocaleString()}` : ''
                })()}
                {bookmarkMessage?.sender_name ? ` · ${bookmarkMessage.sender_name}` : ''}
              </Text>
              <div
                className={`min-h-0 flex-1 overflow-y-auto rounded-md border p-3 text-sm leading-relaxed ${
                  isDark
                    ? 'border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)]'
                    : 'border-slate-200 bg-slate-50'
                }`}
              >
                {bookmarkMessageLoading ? (
                  <Text size="2" color="gray">
                    Loading full message…
                  </Text>
                ) : (
                  <div className="aui-md-root">
                    <ReactMarkdown
                      remarkPlugins={[remarkGfm]}
                      components={{
                        table: ({ className, ...props }) => (
                          <div className="mc-md-table-scroll">
                            <table
                              className={['aui-md-table', className].filter(Boolean).join(' ')}
                              {...props}
                            />
                          </div>
                        ),
                      }}
                    >
                      {(() => {
                        if (bookmarkMessage) {
                          const c = bookmarkMessage.content ?? ''
                          return c.trim() ? c : '_Empty message._'
                        }
                        if (bookmarkMessageError) {
                          return `*Could not load full message (${bookmarkMessageError}). Showing saved preview:*\n\n${selectedBookmark.content_preview || '_No preview stored._'}`
                        }
                        return selectedBookmark.content_preview || '_Empty message._'
                      })()}
                    </ReactMarkdown>
                  </div>
                )}
              </div>
              {selectedBookmark.note ? (
                <Text as="p" size="1" color="gray" className="shrink-0">
                  Note: {selectedBookmark.note}
                </Text>
              ) : null}
              {removeBookmarkError ? (
                <Text size="1" color="red" className="shrink-0">
                  {removeBookmarkError}
                </Text>
              ) : null}
              <Flex justify="between" gap="2" align="center" wrap="wrap" className="shrink-0">
                {onRemoveBookmark && activePersonaId != null ? (
                  <Button
                    type="button"
                    size="2"
                    variant="solid"
                    color="red"
                    disabled={removeBookmarkBusy}
                    onClick={() => {
                      if (!selectedBookmark) return
                      setRemoveBookmarkError('')
                      setRemoveBookmarkBusy(true)
                      void (async () => {
                        try {
                          const ok = await onRemoveBookmark(selectedBookmark.message_id)
                          if (ok) setSelectedBookmark(null)
                          else setRemoveBookmarkError('Could not remove bookmark.')
                        } finally {
                          setRemoveBookmarkBusy(false)
                        }
                      })()
                    }}
                  >
                    {removeBookmarkBusy ? 'Removing…' : 'Remove bookmark'}
                  </Button>
                ) : (
                  <span />
                )}
                <Dialog.Close>
                  <Button type="button" size="2" variant="soft">
                    Close
                  </Button>
                </Dialog.Close>
              </Flex>
            </>
          ) : null}
        </Dialog.Content>
      </Dialog.Root>
    </div>
  )
}
