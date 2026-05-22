import { useCallback, useEffect, useState } from 'react'
import { Flex, Switch, Text } from '@radix-ui/themes'
import type { RuntimeConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
}

export function SettingsRuntimePanel({ api, onError }: Props) {
  const [runtime, setRuntime] = useState<RuntimeConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)

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

  async function onDebugChange(checked: boolean) {
    setSaving(true)
    try {
      const res = await api<RuntimeConfigResponse>('/api/runtime', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tool_output_debug: checked }),
      })
      setRuntime(res)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      await load()
    } finally {
      setSaving(false)
    }
  }

  if (loading) {
    return (
      <Text size="2" color="gray">
        Loading runtime options…
      </Text>
    )
  }

  const enabled = runtime?.tool_output_debug ?? false
  const source = runtime?.source === 'app_settings' ? 'saved in app' : 'from .env default'

  return (
    <Flex direction="column" gap="2">
      <Flex align="center" justify="between" gap="3" wrap="wrap">
        <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 200 }}>
          <Text size="2" weight="medium">
            PZ / ComfyUI debug logging
          </Text>
          <Text size="1" color="gray">
            Show WebSocket timeout and history-polling lines from face-swap scripts. Applies to new
            bash and background shell commands immediately ({source}).
          </Text>
        </Flex>
        <Switch
          size="2"
          checked={enabled}
          disabled={saving}
          onCheckedChange={(checked) => void onDebugChange(checked)}
        />
      </Flex>
    </Flex>
  )
}
