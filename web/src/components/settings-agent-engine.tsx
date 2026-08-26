import { useCallback, useEffect, useRef, useState } from 'react'
import { Callout, Flex, Select, Text } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import { SettingsLlmPanel } from './settings-llm'
import { SettingsLocalDelegatePanel } from './settings-local-delegate'
import { SettingsCursorPanel } from './settings-cursor'
import { SettingsDeterministicPipelinePanel } from './settings-deterministic-pipeline'
import type { CursorEngineConfigResponse, RuntimeConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  chatId: number | null
  activePersonaId: number | null
}

type EngineId = 'classic' | 'classic_cost_routing' | 'deterministic' | 'cursor'
type EngineSelectValue = 'inherit' | EngineId

const ENGINE_OPTIONS: { id: EngineId; label: string; subtitle: string }[] = [
  { id: 'classic', label: 'Single turn', subtitle: 'One cloud model — best reasoning continuity' },
  {
    id: 'classic_cost_routing',
    label: 'Classic · Cost routing',
    subtitle: 'Local read-only discovery + delegate sub-jobs',
  },
  { id: 'deterministic', label: 'Deterministic pipeline', subtitle: 'Intent → plan → execute → consolidate' },
  { id: 'cursor', label: 'Cursor (SDK)', subtitle: 'Full turn via Cursor sidecar' },
]

type PersonaEngineRow = {
  id: number
  name: string
  is_active: boolean
  agent_engine_override?: string | null
  agent_engine_effective?: string
}

function sourceLabel(source?: 'env' | 'app_settings'): string {
  return source === 'app_settings' ? 'saved in app' : 'from .env default'
}

function asEngineId(raw: string | undefined | null): EngineId {
  switch (raw) {
    case 'classic_cost_routing':
    case 'deterministic':
    case 'cursor':
      return raw
    default:
      return 'classic'
  }
}

function engineLabel(id: string): string {
  return ENGINE_OPTIONS.find((opt) => opt.id === id)?.label ?? id
}

function overrideSelectValue(row: PersonaEngineRow): EngineSelectValue {
  const ov = row.agent_engine_override?.trim()
  if (!ov) return 'inherit'
  return asEngineId(ov)
}

export function SettingsAgentEnginePanel({ api, onError, chatId, activePersonaId }: Props) {
  const [runtime, setRuntime] = useState<RuntimeConfigResponse | null>(null)
  const [cursorStatus, setCursorStatus] = useState<CursorEngineConfigResponse | null>(null)
  const [personaRows, setPersonaRows] = useState<PersonaEngineRow[]>([])
  const [loading, setLoading] = useState(true)
  const [savingKey, setSavingKey] = useState<string | null>(null)
  const [configureEngine, setConfigureEngine] = useState<EngineId>('classic')
  const [followEffective, setFollowEffective] = useState(true)
  const followEffectiveRef = useRef(true)
  followEffectiveRef.current = followEffective
  const loadOnceRef = useRef(false)

  const load = useCallback(
    async (opts?: { silent?: boolean }) => {
      if (!opts?.silent) setLoading(true)
      try {
        const data = await api<RuntimeConfigResponse>('/api/runtime')
        setRuntime(data)
        const qs = chatId != null ? `?chat_id=${chatId}` : ''
        const personaRes = await api<{ personas?: PersonaEngineRow[] }>(`/api/personas${qs}`)
        const rows = Array.isArray(personaRes.personas) ? personaRes.personas : []
        setPersonaRows(rows)

        const activeRow =
          rows.find((p) => p.id === activePersonaId) ?? rows.find((p) => p.is_active) ?? rows[0]
        const effective = asEngineId(activeRow?.agent_engine_effective ?? data.agent_engine)
        setConfigureEngine((prev) => (followEffectiveRef.current ? effective : prev))

        const needCursor =
          effective === 'cursor' ||
          data.agent_engine === 'cursor' ||
          rows.some((p) => asEngineId(p.agent_engine_effective) === 'cursor')
        if (needCursor) {
          try {
            const cursor = await api<CursorEngineConfigResponse>('/api/cursor-engine')
            setCursorStatus(cursor)
          } catch {
            setCursorStatus(null)
          }
        } else {
          setCursorStatus(null)
        }
      } catch (e) {
        onError(e instanceof Error ? e.message : String(e))
        setRuntime(null)
        setCursorStatus(null)
      } finally {
        setLoading(false)
      }
    },
    [activePersonaId, api, chatId, onError],
  )

  useEffect(() => {
    const silent = loadOnceRef.current
    loadOnceRef.current = true
    void load({ silent })
  }, [load])

  async function patchRuntime(body: Record<string, boolean | string>, key: string) {
    setSavingKey(key)
    try {
      const res = await api<RuntimeConfigResponse>('/api/runtime', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      setRuntime(res)
      if (res.warnings?.length) {
        onError(res.warnings.join(' '))
      }
      await load({ silent: true })
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      await load({ silent: true })
    } finally {
      setSavingKey(null)
    }
  }

  async function patchPersonaEngine(personaId: number, value: EngineSelectValue) {
    setSavingKey(`persona:${personaId}`)
    try {
      await api(`/api/personas/${personaId}/bulletin`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          agent_engine_override: value === 'inherit' ? null : value,
        }),
      })
      await load({ silent: true })
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      await load({ silent: true })
    } finally {
      setSavingKey(null)
    }
  }

  if (loading) {
    return <SettingsPanelSkeleton />
  }

  const sources = runtime?.sources ?? {}
  const globalEngine = asEngineId(runtime?.agent_engine)
  const costRoutingSelected =
    personaRows.some((p) => asEngineId(p.agent_engine_effective) === 'classic_cost_routing') ||
    globalEngine === 'classic_cost_routing'
  const localReady = runtime?.local_delegate_ready === true
  const localConfigured = runtime?.local_delegate_configured === true
  const toolsOk = runtime?.local_delegate_tools_ok === true
  const activeRow =
    personaRows.find((p) => p.id === activePersonaId) ??
    personaRows.find((p) => p.is_active) ??
    personaRows[0]
  const activeEffective = asEngineId(activeRow?.agent_engine_effective ?? runtime?.agent_engine)
  const panelEngine = followEffective ? activeEffective : configureEngine
  const cursorInUse =
    panelEngine === 'cursor' ||
    globalEngine === 'cursor' ||
    personaRows.some((p) => asEngineId(p.agent_engine_effective) === 'cursor')

  return (
    <Flex direction="column" gap="4">
      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Persona engines
        </Text>
        <Text size="1" color="gray">
          Each persona runs its own engine. Inherit uses the global default below. LLM keys, local
          delegate, Cursor sidecar, and the deterministic profile stay shared.
        </Text>
        {personaRows.length === 0 ? (
          <Text size="1" color="gray">
            No personas in this chat yet.
          </Text>
        ) : (
          <div className="rounded-md border divide-y" style={{ borderColor: 'var(--gray-6)' }}>
            {personaRows.map((row) => {
              const selected = overrideSelectValue(row)
              const effective = asEngineId(row.agent_engine_effective ?? runtime?.agent_engine)
              const isActive = activePersonaId != null ? row.id === activePersonaId : row.is_active
              return (
                <Flex
                  key={row.id}
                  align="center"
                  justify="between"
                  gap="3"
                  wrap="wrap"
                  className="px-3 py-2"
                >
                  <Flex direction="column" gap="1" style={{ minWidth: 140, flex: 1 }}>
                    <Text size="2">
                      {row.name}
                      {isActive ? (
                        <Text size="1" color="gray">
                          {' '}
                          (active)
                        </Text>
                      ) : null}
                    </Text>
                    <Text size="1" color="gray">
                      Effective: {engineLabel(effective)}
                      {selected === 'inherit' ? ' (inherit)' : ''}
                    </Text>
                  </Flex>
                  <Select.Root
                    value={selected}
                    onValueChange={(v) => void patchPersonaEngine(row.id, v as EngineSelectValue)}
                    disabled={savingKey != null}
                  >
                    <Select.Trigger className="w-full md:max-w-xs" />
                    <Select.Content position="popper">
                      <Select.Item value="inherit">
                        Inherit global ({engineLabel(globalEngine)})
                      </Select.Item>
                      {ENGINE_OPTIONS.map((opt) => (
                        <Select.Item key={opt.id} value={opt.id}>
                          {opt.label}
                        </Select.Item>
                      ))}
                    </Select.Content>
                  </Select.Root>
                </Flex>
              )
            })}
          </div>
        )}
      </Flex>

      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Global default (inherit)
        </Text>
        <Text size="1" color="gray">
          Used when a persona is set to inherit. Applies immediately (
          {sourceLabel(sources.agent_engine)}).
        </Text>
        <Flex gap="2" wrap="wrap">
          {ENGINE_OPTIONS.map((opt) => (
            <button
              key={opt.id}
              type="button"
              disabled={savingKey != null}
              className={
                runtime?.agent_engine === opt.id
                  ? 'mc-engine-option mc-engine-option--active'
                  : 'mc-engine-option'
              }
              title={opt.subtitle}
              onClick={() => void patchRuntime({ agent_engine: opt.id }, 'agent_engine')}
            >
              {opt.label}
            </button>
          ))}
        </Flex>

        {costRoutingSelected && !localReady ? (
          <Callout.Root color="orange" size="1" variant="soft" role="alert">
            <Callout.Text>
              {!localConfigured
                ? 'A persona uses cost routing but no local URL/model is configured. Those runs use the cloud model only until you configure Local delegate below.'
                : !toolsOk
                  ? 'A persona uses cost routing but local tool calling is not verified. Those runs use the cloud model only until you run Test in Local delegate.'
                  : 'A persona uses cost routing but the local delegate is not ready. Those runs use the cloud model only.'}
            </Callout.Text>
          </Callout.Root>
        ) : null}

        {cursorInUse && cursorStatus && !cursorStatus.engine_ready ? (
          <Callout.Root color="orange" size="1" variant="soft">
            <Callout.Text>
              Cursor engine is selected for a persona but not ready (
              {cursorStatus.sidecar_reachable ? 'sidecar up' : 'sidecar down'}, API key{' '}
              {cursorStatus.api_key_configured ? 'ok' : 'missing'}, health{' '}
              {cursorStatus.sdk_runner_ok ? 'verified' : 'not verified'}). Finish setup in the Cursor
              panel below.
            </Callout.Text>
          </Callout.Root>
        ) : null}
      </Flex>

      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Configure
        </Text>
        <Text size="1" color="gray">
          Shared knobs for each engine (API keys, local URL, Cursor sidecar, deterministic profile).
          Defaults to the active persona&apos;s effective engine.
        </Text>
        <Select.Root
          value={panelEngine}
          onValueChange={(v) => {
            setFollowEffective(false)
            setConfigureEngine(asEngineId(v))
          }}
        >
          <Select.Trigger className="w-full md:max-w-sm" />
          <Select.Content position="popper">
            {ENGINE_OPTIONS.map((opt) => (
              <Select.Item key={opt.id} value={opt.id}>
                {opt.label}
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Root>
      </Flex>

      {panelEngine === 'classic' ? <SettingsLlmPanel api={api} onError={onError} /> : null}
      {panelEngine === 'classic_cost_routing' ? (
        <Flex direction="column" gap="4">
          <SettingsLlmPanel api={api} onError={onError} />
          <SettingsLocalDelegatePanel api={api} onError={onError} />
        </Flex>
      ) : null}
      {panelEngine === 'cursor' ? <SettingsCursorPanel api={api} onError={onError} /> : null}
      {panelEngine === 'deterministic' ? (
        <SettingsDeterministicPipelinePanel api={api} onError={onError} />
      ) : null}
    </Flex>
  )
}
