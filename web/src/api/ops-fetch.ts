import { api } from './client'
import type { BackgroundJobItem, Persona, QueueLane } from '../types'

export async function fetchQueueLaneForChat(chatId: number): Promise<QueueLane | null> {
  const data = await api<import('../types').QueueDiagnosticsResponse>('/api/queue_diagnostics')
  const lanes = Array.isArray(data.lanes) ? data.lanes : []
  return lanes.find((l) => l.chat_id === chatId) ?? null
}

export async function fetchBackgroundLaneForChat(
  chatId: number,
): Promise<BackgroundJobItem[]> {
  const data = await api<import('../types').QueueDiagnosticsResponse>('/api/queue_diagnostics')
  const map = data.background_by_chat
  if (!map || typeof map !== 'object') return []
  const key = String(chatId)
  const items = map[key]
  return Array.isArray(items) ? items : []
}

export type BackgroundJobsSnapshot = {
  jobs: BackgroundJobItem[]
  activeCount: number
}

export async function fetchBackgroundJobsSnapshot(chatId: number): Promise<BackgroundJobsSnapshot> {
  const q = new URLSearchParams({ chat_id: String(chatId) })
  const data = await api<{ jobs?: BackgroundJobItem[]; active_heartbeats?: unknown[]; active_count?: number }>(
    `/api/background_jobs?${q.toString()}`,
  )
  const jobs: BackgroundJobItem[] = Array.isArray(data.jobs) ? data.jobs : []
  const activeCountFromApi = typeof data.active_count === 'number' && Number.isFinite(data.active_count)
    ? Math.max(0, Math.floor(data.active_count))
    : null
  const activeByStatus = jobs.filter((j) =>
    ['pending', 'running', 'completed_raw', 'main_agent_processing'].includes(j.status),
  ).length
  return { jobs, activeCount: activeCountFromApi ?? activeByStatus }
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
  backgroundJobs: BackgroundJobItem[]
  personasSnapshot: Persona[]
}

export async function fetchOpsPollBundle(chatId: number): Promise<OpsPollBundle> {
  const [queueLane, background, personasSnapshot] = await Promise.all([
    fetchQueueLaneForChat(chatId),
    fetchBackgroundJobsSnapshot(chatId),
    fetchPersonasSnapshot(chatId),
  ])
  return {
    queueLane,
    backgroundActiveCount: background.activeCount,
    backgroundJobs: background.jobs,
    personasSnapshot,
  }
}
