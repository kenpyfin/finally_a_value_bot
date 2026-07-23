import React from 'react'
import { Button, Dialog, Flex, Text } from '@radix-ui/themes'
import type { Persona, PersonaTodo } from '../types'

export type InboxUnreadItem = {
  personaId: number
  personaName: string
  lastBotMessageAt: string | null
}

export type InboxPanelProps = {
  appearance: 'dark' | 'light'
  open: boolean
  onOpenChange: (open: boolean) => void
  unread: InboxUnreadItem[]
  todos: PersonaTodo[]
  personas: Persona[]
  loading: boolean
  busyTodoId: number | null
  onRefresh: () => void
  onOpenPersona: (personaId: number) => void
  onCompleteTodo: (todoId: number) => void
}

function personaName(personas: Persona[], personaId: number): string {
  return personas.find((p) => p.id === personaId)?.name ?? `Persona ${personaId}`
}

function formatWhen(iso: string | null | undefined): string {
  if (!iso) return ''
  const ms = Date.parse(iso)
  if (!Number.isFinite(ms)) return iso
  try {
    return new Date(ms).toLocaleString()
  } catch {
    return iso
  }
}

export function InboxPanel({
  appearance,
  open,
  onOpenChange,
  unread,
  todos,
  personas,
  loading,
  busyTodoId,
  onRefresh,
  onOpenPersona,
  onCompleteTodo,
}: InboxPanelProps) {
  const borderStyle =
    appearance === 'dark'
      ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
      : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }
  const itemBorder =
    appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content style={{ maxWidth: 720 }}>
        <Dialog.Title>Inbox</Dialog.Title>
        <Dialog.Description size="2" mb="3">
          New bot messages across personas and open operator todos from conversations.
        </Dialog.Description>

        <Flex align="center" justify="between" gap="3" mb="3" wrap="wrap">
          <Text size="2" color="gray">
            {unread.length} unread · {todos.length} open todos
          </Text>
          <Button size="1" variant="soft" disabled={loading} onClick={onRefresh}>
            {loading ? 'Refreshing…' : 'Refresh'}
          </Button>
        </Flex>

        <Flex direction="column" gap="4">
          <section>
            <Text size="2" weight="medium" className="mb-2 block">
              New messages
            </Text>
            <div className="rounded-lg border p-3" style={borderStyle}>
              {unread.length === 0 ? (
                <Text size="2" color="gray">
                  No unread bot messages.
                </Text>
              ) : (
                <ul className="list-none space-y-2">
                  {unread.map((item) => (
                    <li
                      key={item.personaId}
                      className="flex flex-wrap items-center gap-2 rounded-lg border p-2"
                      style={itemBorder}
                    >
                      <span className="min-w-0 flex-1 truncate font-medium">{item.personaName}</span>
                      <Text size="1" color="gray">
                        {formatWhen(item.lastBotMessageAt) || 'recent'}
                      </Text>
                      <Button
                        size="1"
                        variant="soft"
                        onClick={() => {
                          onOpenPersona(item.personaId)
                          onOpenChange(false)
                        }}
                      >
                        Open
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>

          <section>
            <Text size="2" weight="medium" className="mb-2 block">
              Todos
            </Text>
            <div className="rounded-lg border p-3" style={borderStyle}>
              {todos.length === 0 ? (
                <Text size="2" color="gray">
                  No open todos. The agent can create them with add_todo.
                </Text>
              ) : (
                <ul className="list-none space-y-2">
                  {todos.map((todo) => (
                    <li
                      key={todo.id}
                      className="flex flex-wrap items-start gap-2 rounded-lg border p-2"
                      style={itemBorder}
                    >
                      <div className="min-w-0 flex-1">
                        <Text size="2" weight="medium" className="block">
                          {todo.title}
                        </Text>
                        <Text size="1" color="gray" className="block">
                          {personaName(personas, todo.persona_id)}
                          {todo.source_hint ? ` · ${todo.source_hint}` : ''}
                          {todo.updated_at ? ` · ${formatWhen(todo.updated_at)}` : ''}
                        </Text>
                      </div>
                      <Button
                        size="1"
                        variant="soft"
                        onClick={() => {
                          onOpenPersona(todo.persona_id)
                          onOpenChange(false)
                        }}
                      >
                        Open
                      </Button>
                      <Button
                        size="1"
                        variant="solid"
                        disabled={busyTodoId === todo.id}
                        onClick={() => onCompleteTodo(todo.id)}
                      >
                        {busyTodoId === todo.id ? '…' : 'Complete'}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </section>
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
