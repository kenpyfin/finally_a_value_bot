import { useCallback, useEffect, useRef, useState } from 'react'
import { Callout, Flex, Select, Text } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import { SettingsLlmPanel } from './settings-llm'
import { SettingsLocalDelegatePanel } from './settings-local-delegate'
import { SettingsCursorPanel } from './settings-cursor'
import { SettingsDeterministicPipelinePanel } from './settings-deterministic-pipeline'
import type {
  CursorEngineConfigResponse,
  Persona,
  PersonaAgentEngineInfo,
  RuntimeConfigResponse,
} from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  activePersonaId: number | null
  personas: Persona[]
}

type EngineId = 'classic' | 'classic_cost_routing' | 'deterministic' | 'cursor'

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

export function SettingsAgentEnginePanel({ api, onError, activePersonaId, personas }: Props) {
  const [runtime, setRuntime] = useState<RuntimeConfigResponse | null>(null)
  const [cursorStatus, setCursorStatus] = useState<CursorEngineConfigResponse | null>(null)
  const [personaEngine, setPersonaEngine] = useState<PersonaAgentEngineInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [savingKey, setSavingKey] = useState<string | null>(null)
  const [configureEngine, setConfigureEngine] = useState<EngineId>('classic')
  const [followPersona, setFollowPersona] = useState(true)
  const followPersonaRef = useRef(true)
  followPersonaRef.current = followPersona
  const loadOnceRef = useRef(false)

  const activePersona = personas.find((p) => p.id === activePersonaId) ?? null
  const personaName = activePersona?.name?.trim() || (activePersonaId != null ? `#${activePersonaId}` : '')

  const load = useCallback(
    async (opts?: { silent?: boolean }) => {
      if (!opts?.silent) setLoading(true)
      try {
        const data = await api<RuntimeConfigResponse>('/api/runtime')
        setRuntime(data)

        let effective = asEngineId(data.agent_engine)
        if (activePersonaId != null) {
          const bulletin = await api<{
            agent_engine?: PersonaAgentEngineInfo | null
            agent_engine_override?: string | null
            agent_engine_global?: string
            agent_engine_effective?: string
          }>(`/api/personas/${activePersonaId}/bulletin`)
          const info: PersonaAgentEngineInfo =
            bulletin.agent_engine && typeof bulletin.agent_engine.effective === 'string'
              ? bulletin.agent_engine
              : {
                  override: bulletin.agent_engine_override ?? null,
                  global: bulletin.agent_engine_global ?? data.agent_engine ?? 'classic',
                  effective:
                    bulletin.agent_engine_effective ??
                    bulletin.agent_engine_global ??
                    data.agent_engine ??
                    'classic',
                  uses_default: !bulletin.agent_engine_override,
                }
          setPersonaEngine(info)
          effective = asEngineId(info.effective)
        } else {
          setPersonaEngine(null)
        }

        setConfigureEngine((prev) => (followPersonaRef.current ? effective : prev))

        if (effective === 'cursor') {
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
        setPersonaEngine(null)
      } finally {
        setLoading(false)
      }
    },
    [activePersonaId, api, onError],
  )

  useEffect(() => {
    const silent = loadOnceRef.current
    loadOnceRef.current = true
    void load({ silent })
  }, [load])

  async function patchPersonaEngine(engine: EngineId) {
    if (activePersonaId == null) {
      onError('Select a persona to set its agent engine.')
      return
    }
    setSavingKey('agent_engine')
    try {
      await api(`/api/personas/${activePersonaId}/bulletin`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent_engine_override: engine }),
      })
      setFollowPersona(true)
      setConfigureEngine(engine)
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

  const selectedEngine = asEngineId(personaEngine?.effective ?? runtime?.agent_engine)
  const costRoutingSelected = selectedEngine === 'classic_cost_routing'
  const localReady = runtime?.local_delegate_ready === true
  const localConfigured = runtime?.local_delegate_configured === true
  const toolsOk = runtime?.local_delegate_tools_ok === true
  const panelEngine = followPersona ? selectedEngine : configureEngine

  return (
    <Flex direction="column" gap="4">
      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Agent engine
        </Text>
        {activePersonaId == null ? (
          <Text size="1" color="gray">
            Select a persona in the sidebar to set its engine.
          </Text>
        ) : (
          <Text size="1" color="gray">
            Applies to the current persona{' '}
            <span className="font-medium">{personaName}</span>
            . Single turn uses one cloud model for the full Classic loop. Cost routing keeps the
            same loop but routes read-only tool chains to a verified local model. Deterministic runs
            a structured pipeline. Cursor delegates the turn to a local SDK sidecar. Applies
            immediately.
          </Text>
        )}
        <Flex gap="2" wrap="wrap">
          {ENGINE_OPTIONS.map((opt) => (
            <button
              key={opt.id}
              type="button"
              disabled={savingKey != null || activePersonaId == null}
              className={
                selectedEngine === opt.id
                  ? 'mc-engine-option mc-engine-option--active'
                  : 'mc-engine-option'
              }
              title={opt.subtitle}
              onClick={() => void patchPersonaEngine(opt.id)}
            >
              {opt.label}
            </button>
          ))}
        </Flex>

        {costRoutingSelected && !localReady ? (
          <Callout.Root color="orange" size="1" variant="soft" role="alert">
            <Callout.Text>
              {!localConfigured
                ? 'Cost routing is selected but no local URL/model is configured. Runs use the cloud model only until you configure Local delegate below.'
                : !toolsOk
                  ? 'Cost routing is selected but local tool calling is not verified. Runs use the cloud model only until you run Test in Local delegate.'
                  : 'Cost routing is selected but the local delegate is not ready. Runs use the cloud model only.'}
            </Callout.Text>
          </Callout.Root>
        ) : null}

        {selectedEngine === 'cursor' && cursorStatus && !cursorStatus.engine_ready ? (
          <Callout.Root color="orange" size="1" variant="soft">
            <Callout.Text>
              Cursor engine is active but not ready ({cursorStatus.sidecar_reachable ? 'sidecar up' : 'sidecar down'}
              , API key {cursorStatus.api_key_configured ? 'ok' : 'missing'}, health{' '}
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
          Show knobs for another engine without changing this persona&apos;s engine (for example
          Cursor settings while this persona is Classic).
        </Text>
        <Select.Root
          value={panelEngine}
          onValueChange={(v) => {
            setFollowPersona(false)
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
