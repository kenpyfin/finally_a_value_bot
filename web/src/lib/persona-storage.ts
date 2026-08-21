import type { ChatSession } from '../types'
import { PERSONA_SESSION_STORAGE_KEY } from '../app/constants'

const PERSONA_STORAGE_KEY = 'finally-a-value-bot_selected_persona_id'
const PERSONA_LAST_READ_STORAGE_KEY = 'finally-a-value-bot_persona_last_read_v1'

export function readStoredPersonaId(): number | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = localStorage.getItem(PERSONA_STORAGE_KEY)
    if (raw === null) return null
    const n = parseInt(raw, 10)
    return Number.isFinite(n) ? n : null
  } catch {
    return null
  }
}

export function writeStoredPersonaId(id: number): void {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(PERSONA_STORAGE_KEY, String(id))
  } catch {
    // ignore
  }
}

function readPersonaSessionMap(): Record<string, string> {
  if (typeof window === 'undefined') return {}
  try {
    const raw = localStorage.getItem(PERSONA_SESSION_STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as unknown
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    const out: Record<string, string> = {}
    for (const [key, value] of Object.entries(parsed)) {
      if (typeof value === 'string' && value.trim()) out[key] = value
    }
    return out
  } catch {
    return {}
  }
}

function writePersonaSessionMap(map: Record<string, string>): void {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(PERSONA_SESSION_STORAGE_KEY, JSON.stringify(map))
  } catch {
    // ignore
  }
}

export function readStoredSessionForPersona(personaId: number): string | null {
  const stored = readPersonaSessionMap()[String(personaId)]
  return stored && stored.trim() ? stored : null
}

export function writeStoredSessionForPersona(personaId: number, sessionId: string | null): void {
  const map = readPersonaSessionMap()
  const key = String(personaId)
  if (sessionId) {
    map[key] = sessionId
  } else {
    delete map[key]
  }
  writePersonaSessionMap(map)
}

export function resolveStoredSessionId(sessions: ChatSession[], personaId: number): string | null {
  const stored = readStoredSessionForPersona(personaId)
  if (!stored) return null
  const match = sessions.find((s) => s.id === stored)
  return match ? match.id : null
}

export function readPersonaLastReadAt(chatId: number, personaId: number): string | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = localStorage.getItem(PERSONA_LAST_READ_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Record<string, unknown>
    const key = `${chatId}:${personaId}`
    const v = parsed[key]
    return typeof v === 'string' ? v : null
  } catch {
    return null
  }
}

export function writePersonaLastReadAt(chatId: number, personaId: number, isoTimestamp: string): void {
  if (typeof window === 'undefined') return
  try {
    const raw = localStorage.getItem(PERSONA_LAST_READ_STORAGE_KEY)
    const parsed: Record<string, unknown> = raw ? JSON.parse(raw) : {}
    parsed[`${chatId}:${personaId}`] = isoTimestamp
    localStorage.setItem(PERSONA_LAST_READ_STORAGE_KEY, JSON.stringify(parsed))
  } catch {
    // ignore
  }
}

/**
 * Seed last-read for personas that have never been stamped so historical bot
 * messages are not treated as "new". Only later messages light the unread dot.
 */
export function baselinePersonaLastReadIfMissing(
  chatId: number,
  personas: { id: number; last_bot_message_at?: string | null }[],
): boolean {
  if (typeof window === 'undefined') return false
  let changed = false
  const nowIso = new Date().toISOString()
  for (const p of personas) {
    if (readPersonaLastReadAt(chatId, p.id) != null) continue
    writePersonaLastReadAt(chatId, p.id, p.last_bot_message_at ?? nowIso)
    changed = true
  }
  return changed
}

export function toMs(iso: string | null | undefined): number | null {
  if (!iso) return null
  const ms = Date.parse(iso)
  return Number.isFinite(ms) ? ms : null
}
