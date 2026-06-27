import { useCallback, useEffect, useState } from 'react'
import { Flex, Switch, Text } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type { RuntimeConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
}

function sourceLabel(source?: 'env' | 'app_settings'): string {
  return source === 'app_settings' ? 'saved in app' : 'from .env default'
}

export function SettingsRuntimePanel({ api, onError }: Props) {
  const [runtime, setRuntime] = useState<RuntimeConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [savingKey, setSavingKey] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data = await api<RuntimeConfigResponse>('/api/runtime')
      setRuntime(data)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      setRuntime(null)
    } finally {
      setLoading(false)
    }
  }, [api, onError])

  useEffect(() => {
    void load()
  }, [load])

  async function patchRuntime(body: Record<string, boolean>, key: string) {
    setSavingKey(key)
    try {
      const res = await api<RuntimeConfigResponse>('/api/runtime', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      setRuntime(res)
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
            Can exit early or add latency on tool-heavy runs. Uses local multimodel when
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
    </Flex>
  )
}
