import React, { useId, useState } from 'react'
import { Flex, Text } from '@radix-ui/themes'
import type { InstallationStatus, QueueLane } from '../types'

export type CockpitBarProps = {
  appearance: 'dark' | 'light'
  statusText: string
  queueLane: QueueLane | null
  backgroundActiveCount: number
  installationStatus: InstallationStatus | null
  onQueueClick: () => void
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
}: CockpitBarProps) {
  const [expanded, setExpanded] = useState(false)
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

  const stripClass = isDark
    ? 'border-t border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-main)]/40'
    : 'border-t border-slate-200/90 bg-slate-50/80'

  const toggleBtnClass = isDark
    ? 'mx-auto flex h-7 w-10 shrink-0 cursor-pointer items-center justify-center rounded-md border-0 bg-transparent text-slate-400 transition-colors hover:bg-white/5 hover:text-slate-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[color:var(--mc-accent)]'
    : 'mx-auto flex h-7 w-10 shrink-0 cursor-pointer items-center justify-center rounded-md border-0 bg-transparent text-slate-500 transition-colors hover:bg-slate-200/60 hover:text-slate-800 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400'

  return (
    <div
      className={`mc-cockpit px-4 ${expanded ? 'py-2' : 'py-1'} ${stripClass}`}
      role="region"
      aria-label="Session status"
    >
      <div className="flex justify-center">
        <button
          id={toggleId}
          type="button"
          className={toggleBtnClass}
          aria-expanded={expanded}
          aria-controls={panelId}
          title={expanded ? 'Hide session status' : 'Show session status'}
          onClick={() => setExpanded((v) => !v)}
        >
          <span className="sr-only">{expanded ? 'Hide session status' : 'Show session status'}</span>
          <svg
            className={`size-3.5 shrink-0 transition-transform duration-150 ${expanded ? '-rotate-180' : ''}`}
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
        </button>
      </div>

      {expanded ? (
        <Flex
          id={panelId}
          align="center"
          gap="3"
          wrap="wrap"
          className="min-h-[36px] text-[13px] leading-snug"
          aria-labelledby={toggleId}
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
      ) : null}
    </div>
  )
}
