import React, { useCallback, useState } from 'react'
import {
  Button,
  Checkbox,
  Dialog,
  DropdownMenu,
  Flex,
  IconButton,
  Select,
  Text,
  TextArea,
} from '@radix-ui/themes'
import { ConfirmDialog } from './confirm-dialog'
import { IconMoreVertical } from './icons'
import type { ChatSession } from '../types'

type SessionPickerProps = {
  sessions: ChatSession[]
  activeSessionId: string | null
  onSelectSession: (sessionId: string | null) => void
  onCreateSession: (intent: string, mirrorMainChat: boolean) => Promise<void>
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
  onDeleteSession,
  loading,
  compact = false,
}: SessionPickerProps) {
  const [newDialogOpen, setNewDialogOpen] = useState(false)
  const [intentDraft, setIntentDraft] = useState('')
  const [mirrorMainChatDraft, setMirrorMainChatDraft] = useState(false)
  const [creating, setCreating] = useState(false)
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false)
  const [deleting, setDeleting] = useState(false)

  const handleCreate = useCallback(async () => {
    if (!intentDraft.trim()) return
    setCreating(true)
    try {
      await onCreateSession(intentDraft.trim(), mirrorMainChatDraft)
      setIntentDraft('')
      setMirrorMainChatDraft(false)
      setNewDialogOpen(false)
    } finally {
      setCreating(false)
    }
  }, [intentDraft, mirrorMainChatDraft, onCreateSession])

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
          {sessions.length > 0 && <Select.Separator />}
          {sessions.map((s) => (
            <Select.Item key={s.id} value={s.id}>
              {s.title}
              {s.mirror_main_chat ? ' · main' : ''} ({formatRelativeTime(s.last_active_at)})
            </Select.Item>
          ))}
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
            <DropdownMenu.Item
              color="red"
              onSelect={() => setDeleteConfirmOpen(true)}
            >
              Delete session
            </DropdownMenu.Item>
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
            <Text as="label" size="2">
              <Flex gap="2" align="start">
                <Checkbox
                  checked={mirrorMainChatDraft}
                  onCheckedChange={(checked) => setMirrorMainChatDraft(checked === true)}
                />
                <span>
                  Include messages in main chat
                  <Text as="div" size="1" color="gray">
                    Off by default — session history stays isolated unless you enable this.
                  </Text>
                </span>
              </Flex>
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
