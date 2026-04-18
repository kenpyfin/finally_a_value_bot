import { api } from './client'
import type { Persona, QueueLane } from '../types'

export async function fetchQueueLaneForChat(chatId: number): Promise<QueueLane | null> {
  const data = await api<{ lanes?: QueueLane[] }>('/api/queue_diagnostics')
  const lanes = Array.isArray(data.lanes) ? data.lanes : []
  return lanes.find((l) => l.chat_id === chatId) ?? null
}

export async function fetchBackgroundActiveCount(chatId: number): Promise<number> {
  const q = new URLSearchParams({ chat_id: String(chatId) })
  const data = await api<{ active_heartbeats?: unknown[] }>(`/api/background_jobs?${q.toString()}`)
  const arr = Array.isArray(data.active_heartbeats) ? data.active_heartbeats : []
  return arr.length
}

export async function fetchPersonasSnapshot(chatId: number): Promise<Persona[]> {
  const query = new URLSearchParams({ chat_id: String(chatId) })
  const data = await api<{
    personas?: { id: number; name: string; is_active: boolean; last_bot_message_at?: string | null }[]
  }>(`/api/personas?${query.toString()}`)
  const list = Array.isArray(data.personas) ? data.personas : []
  return list.map((p) => ({
    id: p.id,
    name: p.name,
    is_active: p.is_active,
    last_bot_message_at: p.last_bot_message_at ?? null,
  }))
}

export type OpsPollBundle = {
  queueLane: QueueLane | null
  backgroundActiveCount: number
  personasSnapshot: Persona[]
}

export async function fetchOpsPollBundle(chatId: number): Promise<OpsPollBundle> {
  const [queueLane, backgroundActiveCount, personasSnapshot] = await Promise.all([
    fetchQueueLaneForChat(chatId),
    fetchBackgroundActiveCount(chatId),
    fetchPersonasSnapshot(chatId),
  ])
  return { queueLane, backgroundActiveCount, personasSnapshot }
}
