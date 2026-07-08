import { useCallback, useEffect, useState } from 'react'
import { Callout, Flex, Switch, Text } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type { CursorEngineConfigResponse, RuntimeConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
}

function sourceLabel(source?: 'env' | 'app_settings'): string {
  return source === 'app_settings' ? 'saved in app' : 'from .env default'
}

export function SettingsRuntimePanel({ api, onError }: Props) {
  const [runtime, setRuntime] = useState<RuntimeConfigResponse | null>(null)
  const [cursorStatus, setCursorStatus] = useState<CursorEngineConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [savingKey, setSavingKey] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data = await api<RuntimeConfigResponse>('/api/runtime')
      setRuntime(data)
      if (data.agent_engine === 'cursor') {
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
  }, [api, onError])

  useEffect(() => {
    void load()
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
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      await load()
    } finally {
      setSavingKey(null)
    }
  }

  if (loading) {
    return <SettingsPanelSkeleton />
  }

  const sources = runtime?.sources ?? {}
  const costRoutingSelected = runtime?.agent_engine === 'classic_cost_routing'
  const localReady = runtime?.local_delegate_ready === true
  const localConfigured = runtime?.local_delegate_configured === true
  const toolsOk = runtime?.local_delegate_tools_ok === true

  return (
    <Flex direction="column" gap="4">
      <Flex align="center" justify="between" gap="3" wrap="wrap">
        <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 200 }}>
          <Text size="2" weight="medium">
            Verbose pipeline logging
          </Text>
          <Text size="1" color="gray">
            When on: verbose shell logs appear in chat (including background-job completion
            messages). When off: full logs are kept for the agent only. Applies immediately (
            {sourceLabel(sources.tool_output_debug)}).
          </Text>
        </Flex>
        <Switch
          size="2"
          checked={runtime?.tool_output_debug ?? false}
          disabled={savingKey != null}
          onCheckedChange={(checked) =>
            void patchRuntime({ tool_output_debug: checked }, 'tool_output_debug')
          }
        />
      </Flex>

      <Flex align="center" justify="between" gap="3" wrap="wrap">
        <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 200 }}>
          <Text size="2" weight="medium">
            Post-tool evaluator (PTE)
          </Text>
          <Text size="1" color="gray">
            After each tool iteration, ask a sidecar model whether the session goal is fulfilled.
            Can exit early or add latency on tool-heavy runs. Uses the local delegate endpoint when
            configured, else Perplexity. Applies immediately (
            {sourceLabel(sources.post_tool_evaluator_enabled)}).
          </Text>
        </Flex>
        <Switch
          size="2"
          checked={runtime?.post_tool_evaluator_enabled ?? false}
          disabled={savingKey != null}
          onCheckedChange={(checked) =>
            void patchRuntime({ post_tool_evaluator_enabled: checked }, 'post_tool_evaluator_enabled')
          }
        />
      </Flex>

      <Flex align="center" justify="between" gap="3" wrap="wrap">
        <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 200 }}>
          <Text size="2" weight="medium">
            Pre-delivery quality (PDQE)
          </Text>
          <Text size="1" color="gray">
            Before the user sees a reply, judge the draft against the session goal. On fail with
            sufficient confidence, injects feedback and retries once. Applies immediately (
            {sourceLabel(sources.response_quality_evaluator_enabled)}).
          </Text>
        </Flex>
        <Switch
          size="2"
          checked={runtime?.response_quality_evaluator_enabled ?? false}
          disabled={savingKey != null}
          onCheckedChange={(checked) =>
            void patchRuntime(
              { response_quality_evaluator_enabled: checked },
              'response_quality_evaluator_enabled',
            )
          }
        />
      </Flex>

      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Agent engine
        </Text>
        <Text size="1" color="gray">
          Single turn uses one cloud model for the full Classic loop. Cost routing keeps the same
          loop but routes read-only tool chains to a verified local model. Deterministic runs a
          structured pipeline. Cursor delegates the turn to a local SDK sidecar. Applies immediately
          ({sourceLabel(sources.agent_engine)}).
        </Text>
        <Flex gap="2" wrap="wrap">
          {(
            [
              ['classic', 'Single turn', 'One cloud model — best reasoning continuity'],
              [
                'classic_cost_routing',
                'Classic · Cost routing',
                'Local read-only discovery + delegate sub-jobs',
              ],
              ['deterministic', 'Deterministic pipeline', 'Intent → plan → execute → consolidate'],
              ['cursor', 'Cursor (SDK)', 'Full turn via Cursor sidecar'],
            ] as const
          ).map(([engine, label, subtitle]) => (
            <button
              key={engine}
              type="button"
              disabled={savingKey != null}
              className={
                runtime?.agent_engine === engine
                  ? 'mc-engine-option mc-engine-option--active'
                  : 'mc-engine-option'
              }
              title={subtitle}
              onClick={() => void patchRuntime({ agent_engine: engine }, 'agent_engine')}
            >
              {label}
            </button>
          ))}
        </Flex>

        {costRoutingSelected && !localReady ? (
          <Callout.Root color="orange" size="1" variant="soft" role="alert">
            <Callout.Text>
              {!localConfigured
                ? 'Cost routing is selected but no local URL/model is configured. Runs use the cloud model only until you configure Local delegate settings.'
                : !toolsOk
                  ? 'Cost routing is selected but local tool calling is not verified. Runs use the cloud model only until you run Test in Local delegate.'
                  : 'Cost routing is selected but the local delegate is not ready. Runs use the cloud model only.'}
            </Callout.Text>
          </Callout.Root>
        ) : null}

        {runtime?.agent_engine === 'cursor' && cursorStatus && !cursorStatus.engine_ready ? (
          <Callout.Root color="orange" size="1" variant="soft">
            <Callout.Text>
              Cursor engine is active but not ready ({cursorStatus.sidecar_reachable ? 'sidecar up' : 'sidecar down'}
              , API key {cursorStatus.api_key_configured ? 'ok' : 'missing'}, health{' '}
              {cursorStatus.sdk_runner_ok ? 'verified' : 'not verified'}). Open Settings → Cursor to
              finish setup.
            </Callout.Text>
          </Callout.Root>
        ) : null}
      </Flex>
    </Flex>
  )
}
