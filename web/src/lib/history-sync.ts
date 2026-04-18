import type { ThreadMessageLike } from '@assistant-ui/react'
import type { BackendMessage } from '../types'

export function mapBackendHistory(messages: BackendMessage[]): ThreadMessageLike[] {
  return messages.map((item, index) => ({
    id: item.id || `history-${index}`,
    role: item.is_from_bot ? 'assistant' : 'user',
    content: item.content || '',
    createdAt: item.timestamp ? new Date(item.timestamp) : new Date(),
  }))
}

/** Compare history for sync/remount decisions: id, role, content only — ignore `createdAt` (server timestamps can jitter between polls). */
export function historiesEqual(a: ThreadMessageLike[], b: ThreadMessageLike[]): boolean {
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

export function shouldDeferHistoryRemount(): boolean {
  if (typeof document === 'undefined') return false
  const inComposer = Boolean(document.activeElement?.closest?.('.aui-composer-root'))
  const vp = document.querySelector('.aui-thread-viewport')
  if (!vp) return inComposer
  const el = vp as HTMLElement
  const gap = el.scrollHeight - el.scrollTop - el.clientHeight
  const scrolledAwayFromBottom = gap > 100
  return inComposer || scrolledAwayFromBottom
}
