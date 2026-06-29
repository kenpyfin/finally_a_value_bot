import React from 'react'
import { Button, Dialog, Flex, Text } from '@radix-ui/themes'
import type { InstallationStatus, QueueLane } from '../types'

export type CockpitStatusChipProps = {
  queueLane: QueueLane | null
  backgroundActiveCount: number
  installationStatus: InstallationStatus | null
  statusText: string
  onClick?: () => void
  className?: string
}

export function CockpitStatusChip({
  queueLane,
  backgroundActiveCount,
  installationStatus,
  statusText,
  onClick,
  className,
}: CockpitStatusChipProps) {
  const pending = queueLane?.pending ?? 0
  const setupOk =
    installationStatus?.llm_ready === true && installationStatus?.channel_ready === true
  const parts: string[] = []
  if (pending > 0) parts.push(`Queue: ${pending}`)
  if (backgroundActiveCount > 0) parts.push(`Jobs: ${backgroundActiveCount}`)
  parts.push(setupOk ? 'Setup OK' : 'Setup needed')
  const busy =
    statusText.startsWith('Uploading') ||
    statusText.startsWith('Sending') ||
    statusText === 'Queued' ||
    statusText.startsWith('Loading quote')
  if (busy && statusText !== 'Idle') {
    parts.unshift(statusText)
  }

  return (
    <button
      type="button"
      className={`mc-cockpit-status-chip cursor-pointer ${className ?? ''}`.trim()}
      onClick={onClick}
      aria-label={`Session status: ${parts.join(', ')}. Expand for details.`}
      title="Session status"
    >
      {parts.join(' · ')}
    </button>
  )
}

export type MobileOpsSheetProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  onOpenQueue: () => void
  onOpenSchedules: () => void
  onOpenPrinciples: () => void
  onOpenArtifacts: () => void
  onOpenMemory: () => void
  onOpenAgentHistory: () => void
  agentHistoryDisabled?: boolean
}

export function MobileOpsSheet({
  open,
  onOpenChange,
  onOpenQueue,
  onOpenSchedules,
  onOpenPrinciples,
  onOpenArtifacts,
  onOpenMemory,
  onOpenAgentHistory,
  agentHistoryDisabled,
}: MobileOpsSheetProps) {
  const pick = (fn: () => void) => () => {
    onOpenChange(false)
    fn()
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content maxWidth="400px" className="mc-mobile-ops-sheet">
        <Dialog.Title>Operator tools</Dialog.Title>
        <Dialog.Description size="2" color="gray" mb="3">
          Queue, schedules, memory, and diagnostics.
        </Dialog.Description>
        <Flex direction="column" gap="2">
          <Button variant="soft" className="cursor-pointer justify-start" onClick={pick(onOpenQueue)}>
            Queue
          </Button>
          <Button variant="soft" className="cursor-pointer justify-start" onClick={pick(onOpenSchedules)}>
            Schedules
          </Button>
          <Button variant="soft" className="cursor-pointer justify-start" onClick={pick(onOpenPrinciples)}>
            Principles
          </Button>
          <Button variant="soft" className="cursor-pointer justify-start" onClick={pick(onOpenArtifacts)}>
            Artifacts
          </Button>
          <Button variant="soft" className="cursor-pointer justify-start" onClick={pick(onOpenMemory)}>
            Memory
          </Button>
          <Button
            variant="soft"
            className="cursor-pointer justify-start"
            disabled={agentHistoryDisabled}
            onClick={pick(onOpenAgentHistory)}
          >
            Last agent run
          </Button>
        </Flex>
        <Flex justify="end" mt="4">
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Close
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  )
}

export type ShortcutsDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const SHORTCUTS = [
  { keys: '?', action: 'Show keyboard shortcuts' },
  { keys: '/', action: 'Focus message composer' },
  { keys: 'Esc', action: 'Dismiss quoted reply' },
  { keys: 'Cmd/Ctrl + Enter', action: 'Send message (in composer)' },
  { keys: 'Cmd/Ctrl + Enter', action: 'Create session (in new session dialog)' },
]

export function ShortcutsDialog({ open, onOpenChange }: ShortcutsDialogProps) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content maxWidth="440px">
        <Dialog.Title>Keyboard shortcuts</Dialog.Title>
        <Dialog.Description size="2" color="gray" mb="3">
          Desktop only. Composer must be focused for send shortcut.
        </Dialog.Description>
        <ul className="mc-shortcuts-list">
          {SHORTCUTS.map((row, i) => (
            <li key={`${row.keys}-${i}`} className="mc-shortcuts-row">
              <kbd className="mc-shortcuts-kbd">{row.keys}</kbd>
              <Text size="2">{row.action}</Text>
            </li>
          ))}
        </ul>
        <Flex justify="end" mt="4">
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Close
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  )
}
