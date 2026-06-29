import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo } from 'react'
import type { BackgroundJobItem, Persona, QueueLane } from '../types'
import {
  fetchOpsPollBundle,
  pickQueueLaneForPersona,
  sumPendingOnOtherPersonas,
  type OpsPollBundle,
} from '../api/ops-fetch'

type UseOpsPollArgs = {
  chatId: number | null
  activePersonaId: number | null
  docVisible: boolean
  pendingRunsForActivePersona: number
  setPersonas: React.Dispatch<React.SetStateAction<Persona[]>>
}

/**
 * Single combined poll for queue lane, background heartbeats, and persona list refresh.
 * Replaces manual setInterval polling.
 */
export function useOpsPoll({
  chatId,
  activePersonaId,
  docVisible,
  pendingRunsForActivePersona,
  setPersonas,
}: UseOpsPollArgs): {
  queueLane: QueueLane | null
  queueLanesAll: QueueLane[]
  otherPersonasPending: number
  backgroundActiveCount: number
  backgroundJobs: BackgroundJobItem[]
  invalidateOps: (chatIdOverride?: number | null) => Promise<void>
} {
  const queryClient = useQueryClient()

  const query = useQuery({
    queryKey: ['opsPoll', chatId],
    queryFn: async (): Promise<OpsPollBundle> => {
      if (chatId == null) {
        throw new Error('opsPoll: missing chatId')
      }
      return fetchOpsPollBundle(chatId)
    },
    enabled: chatId != null,
    refetchInterval: (q) => {
      if (chatId == null) return false
      const d = q.state.data
      const lanes = d?.queueLanes ?? []
      const activeLane = pickQueueLaneForPersona(lanes, activePersonaId)
      const qp = (activeLane?.pending ?? 0) > 0
      const otherPending = sumPendingOnOtherPersonas(lanes, activePersonaId) > 0
      const activePending =
        qp
        || otherPending
        || pendingRunsForActivePersona > 0
        || (d?.backgroundActiveCount ?? 0) > 0
      const baseMs = activePending ? 2500 : 10000
      return docVisible ? baseMs : 60000
    },
  })

  useEffect(() => {
    const snap = query.data?.personasSnapshot
    if (snap && snap.length >= 0) {
      setPersonas(snap)
    }
  }, [query.data?.personasSnapshot, setPersonas])

  const queueLanesAll = query.data?.queueLanes ?? []
  const queueLane = useMemo(
    () => pickQueueLaneForPersona(queueLanesAll, activePersonaId),
    [queueLanesAll, activePersonaId],
  )
  const otherPersonasPending = useMemo(
    () => sumPendingOnOtherPersonas(queueLanesAll, activePersonaId),
    [queueLanesAll, activePersonaId],
  )
  const backgroundActiveCount = query.data?.backgroundActiveCount ?? 0
  const backgroundJobs = query.data?.backgroundJobs ?? []

  const invalidateOps = useCallback(
    async (chatIdOverride?: number | null) => {
      const id = chatIdOverride ?? chatId
      if (id == null) return
      await queryClient.invalidateQueries({ queryKey: ['opsPoll', id] })
    },
    [chatId, queryClient],
  )

  return {
    queueLane,
    queueLanesAll,
    otherPersonasPending,
    backgroundActiveCount,
    backgroundJobs,
    invalidateOps,
  }
}
