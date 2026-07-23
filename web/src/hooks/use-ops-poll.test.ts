import { describe, expect, it } from 'vitest'
import { personasSnapshotEqual } from './use-ops-poll'
import type { Persona } from '../types'

function persona(partial: Partial<Persona> & Pick<Persona, 'id' | 'name'>): Persona {
  return {
    is_active: false,
    last_bot_message_at: null,
    ...partial,
  }
}

describe('personasSnapshotEqual', () => {
  it('returns true for identical snapshots', () => {
    const a = [persona({ id: 1, name: 'A', is_active: true, last_bot_message_at: '2026-01-01' })]
    const b = [persona({ id: 1, name: 'A', is_active: true, last_bot_message_at: '2026-01-01' })]
    expect(personasSnapshotEqual(a, b)).toBe(true)
  })

  it('returns false when last_bot_message_at changes', () => {
    const a = [persona({ id: 1, name: 'A', last_bot_message_at: '2026-01-01' })]
    const b = [persona({ id: 1, name: 'A', last_bot_message_at: '2026-01-02' })]
    expect(personasSnapshotEqual(a, b)).toBe(false)
  })

  it('treats null and undefined last_bot_message_at as equal', () => {
    const a = [persona({ id: 1, name: 'A', last_bot_message_at: null })]
    const b = [persona({ id: 1, name: 'A', last_bot_message_at: undefined })]
    expect(personasSnapshotEqual(a, b)).toBe(true)
  })

  it('returns false when lengths differ', () => {
    expect(personasSnapshotEqual([persona({ id: 1, name: 'A' })], [])).toBe(false)
  })
})
