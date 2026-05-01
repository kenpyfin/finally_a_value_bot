import { api } from './client'
import type { BackgroundJobItem, Persona, QueueLane } from '../types'

export async function fetchQueueLaneForChat(chatId: number): Promise<QueueLane | null> {
  const data = await api<{ lanes?: QueueLane[] }>('/api/queue_diagnostics')
  const lanes = Array.isArray(data.lanes) ? data.lanes : []
  return lanes.find((l) => l.chat_id === chatId) ?? null
}

export type BackgroundJobsSnapshot = {
  jobs: BackgroundJobItem[]
  activeCount: number
}

export async function fetchBackgroundJobsSnapshot(chatId: number): Promise<BackgroundJobsSnapshot> {
  const q = new URLSearchParams({ chat_id: String(chatId) })
  const data = await api<{ jobs?: BackgroundJobItem[]; active_heartbeats?: unknown[] }>(
    `/api/background_jobs?${q.toString()}`,
  )
  const jobs = Array.isArray(data.jobs) ? data.jobs : []
  const activeHeartbeats = Array.isArray(data.active_heartbeats) ? data.active_heartbeats : []
  const activeByStatus = jobs.filter((j) =>
    ['pending', 'running', 'completed_raw', 'main_agent_processing'].includes(j.status),
  ).length
  return { jobs, activeCount: Math.max(activeByStatus, activeHeartbeats.length) }
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
