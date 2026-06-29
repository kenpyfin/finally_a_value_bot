import React, { useCallback, useState } from 'react'
import {
  Button,
  Dialog,
  DropdownMenu,
  Flex,
  IconButton,
  Select,
  Text,
  TextArea,
} from '@radix-ui/themes'
import { ConfirmDialog } from './confirm-dialog'
import { IconChevronDown, IconChevronUp, IconMoreVertical } from './icons'
import type { ChatSession } from '../types'

type SessionPickerProps = {
  sessions: ChatSession[]
  activeSessionId: string | null
  onSelectSession: (sessionId: string | null) => void
  onCreateSession: (intent: string) => Promise<void>
  onArchiveSession: (sessionId: string) => Promise<void>
  onReopenSession: (sessionId: string) => Promise<void>
  onDeleteSession: (sessionId: string) => Promise<void>
  loading?: boolean
  /** Tighter layout for header row */
  compact?: boolean
}

function formatRelativeTime(isoDate: string): string {
  const diff = Date.now() - new Date(isoDate).getTime()
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return 'now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}

export function SessionPicker({
  sessions,
  activeSessionId,
  onSelectSession,
  onCreateSession,
  onArchiveSession,
  onReopenSession,
  onDeleteSession,
  loading,
  compact = false,
}: SessionPickerProps) {
  const [newDialogOpen, setNewDialogOpen] = useState(false)
  const [intentDraft, setIntentDraft] = useState('')
  const [creating, setCreating] = useState(false)
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false)
  const [deleting, setDeleting] = useState(false)

  const activeSessions = sessions.filter((s) => s.status === 'active')
  const archivedSessions = sessions.filter((s) => s.status === 'archived')
  const [showArchived, setShowArchived] = useState(false)

  const handleCreate = useCallback(async () => {
    if (!intentDraft.trim()) return
    setCreating(true)
    try {
      await onCreateSession(intentDraft.trim())
      setIntentDraft('')
      setNewDialogOpen(false)
    } finally {
      setCreating(false)
    }
  }, [intentDraft, onCreateSession])

  const currentLabel = activeSessionId
    ? sessions.find((s) => s.id === activeSessionId)?.title || 'Session'
    : 'Main chat'
  const activeSession = activeSessionId
    ? sessions.find((s) => s.id === activeSessionId) ?? null
    : null

  const handleConfirmDelete = useCallback(async () => {
    if (!activeSessionId) return
    setDeleting(true)
    try {
      await onDeleteSession(activeSessionId)
      setDeleteConfirmOpen(false)
    } finally {
      setDeleting(false)
    }
  }, [activeSessionId, onDeleteSession])

  return (
    <Flex
      align="center"
      gap="2"
      className="mc-session-picker"
      data-compact={compact ? 'true' : 'false'}
      style={{ minWidth: 0 }}
    >
      <Select.Root
        size="2"
        value={activeSessionId ?? '__main__'}
        onValueChange={(val) => onSelectSession(val === '__main__' ? null : val)}
        disabled={loading}
      >
        <Select.Trigger variant="ghost" className="mc-session-picker-trigger cursor-pointer">
          {currentLabel}
        </Select.Trigger>
        <Select.Content position="popper" sideOffset={4}>
          <Select.Item value="__main__">Main chat</Select.Item>
          {activeSessions.length > 0 && <Select.Separator />}
          {activeSessions.map((s) => (
            <Select.Item key={s.id} value={s.id}>
              {s.title} ({formatRelativeTime(s.last_active_at)})
            </Select.Item>
          ))}
          {archivedSessions.length > 0 ? (
            <>
              <Select.Separator />
              <Select.Group>
                <Select.Label>
                  <button
                    type="button"
                    className="mc-archived-toggle cursor-pointer"
                    onClick={(e) => {
                      e.stopPropagation()
                      setShowArchived(!showArchived)
                    }}
                  >
                    Archived ({archivedSessions.length}){' '}
                    {showArchived ? <IconChevronUp /> : <IconChevronDown />}
                  </button>
                </Select.Label>
                {showArchived
                  ? archivedSessions.map((s) => (
                      <Select.Item key={s.id} value={s.id}>
                        {s.title} (archived)
                      </Select.Item>
                    ))
                  : null}
              </Select.Group>
            </>
          ) : (
            <>
              <Select.Separator />
              <Select.Item value="__no_archived__" disabled>
                No archived sessions
              </Select.Item>
            </>
          )}
        </Select.Content>
      </Select.Root>

      <Button
        variant="ghost"
        size="2"
        className="mc-session-picker-btn cursor-pointer"
        onClick={() => setNewDialogOpen(true)}
      >
        {compact ? (
          <>
            <span className="md:hidden">+</span>
            <span className="hidden md:inline">+ Session</span>
          </>
        ) : (
          '+ Session'
        )}
      </Button>

      {activeSessionId && activeSession ? (
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>
            <IconButton
              variant="ghost"
              size="2"
              className="mc-session-picker-btn cursor-pointer"
              aria-label="Session actions"
            >
              <IconMoreVertical />
            </IconButton>
          </DropdownMenu.Trigger>
          <DropdownMenu.Content>
            {activeSession.status === 'active' ? (
              <DropdownMenu.Item onSelect={() => void onArchiveSession(activeSessionId)}>
                Archive session
              </DropdownMenu.Item>
            ) : null}
            {activeSession.status === 'archived' ? (
              <>
                <DropdownMenu.Item onSelect={() => void onReopenSession(activeSessionId)}>
                  Restore session
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  color="red"
                  onSelect={() => setDeleteConfirmOpen(true)}
                >
                  Delete session
                </DropdownMenu.Item>
              </>
            ) : null}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      ) : null}

      <Dialog.Root open={newDialogOpen} onOpenChange={setNewDialogOpen}>
        <Dialog.Content maxWidth="420px">
          <Dialog.Title>New session</Dialog.Title>
          <Dialog.Description size="2" color="gray">
            Describe a focus area to spin up context from your vault and skills.
          </Dialog.Description>
          <Flex direction="column" gap="3" mt="4">
            <TextArea
              placeholder="e.g. Refactor the auth module to use JWT..."
              value={intentDraft}
              onChange={(e) => setIntentDraft(e.target.value)}
              rows={3}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                  void handleCreate()
                }
              }}
            />
            <Text size="1" color="gray">
              Max 500 characters. Press Cmd+Enter to create.
            </Text>
          </Flex>
          <Flex gap="3" mt="4" justify="end">
            <Dialog.Close>
              <Button variant="soft" color="gray">
                Cancel
              </Button>
            </Dialog.Close>
            <Button
              onClick={() => void handleCreate()}
              disabled={!intentDraft.trim() || intentDraft.length > 500 || creating}
            >
              {creating ? 'Creating…' : 'Create session'}
            </Button>
          </Flex>
        </Dialog.Content>
      </Dialog.Root>

      <ConfirmDialog
        open={deleteConfirmOpen}
        onOpenChange={setDeleteConfirmOpen}
        title="Delete session"
        description="This permanently removes the session and its messages from the database. This cannot be undone."
        confirmLabel="Delete session"
        destructive
        loading={deleting}
        onConfirm={handleConfirmDelete}
      />
    </Flex>
  )
}
