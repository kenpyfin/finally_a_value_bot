import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Button, Callout, Flex, Select, Switch, Text, TextField } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type { CursorEngineConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
}

function statusColor(ok: boolean | undefined): 'green' | 'orange' | 'gray' {
  if (ok === true) return 'green'
  if (ok === false) return 'orange'
  return 'gray'
}

export function SettingsCursorPanel({ api, onError }: Props) {
  const [config, setConfig] = useState<CursorEngineConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [loadingModels, setLoadingModels] = useState(false)
  const [saveNotice, setSaveNotice] = useState<string | null>(null)
  const [availableModels, setAvailableModels] = useState<string[]>([])
  const [modelsNotice, setModelsNotice] = useState<string | null>(null)
  const [useCustomSdkModel, setUseCustomSdkModel] = useState(false)
  const autoLoadedModelsRef = useRef(false)

  const [sdkModel, setSdkModel] = useState('')
  const [cliPath, setCliPath] = useState('')
  const [cliModel, setCliModel] = useState('')
  const [cliRunnerUrl, setCliRunnerUrl] = useState('')
  const [timeoutSecs, setTimeoutSecs] = useState('3600')
  const [tmuxEnabled, setTmuxEnabled] = useState(true)

  const load = useCallback(async () => {
    setLoading(true)
    setSaveNotice(null)
    try {
      const data = await api<CursorEngineConfigResponse>('/api/cursor-engine')
      setConfig(data)
      setSdkModel(data.sdk_model ?? '')
      setCliPath(data.cli_path ?? '')
      setCliModel(data.cli_model ?? '')
      setCliRunnerUrl(data.cli_runner_url ?? '')
      setTimeoutSecs(String(data.timeout_secs ?? 3600))
      setTmuxEnabled(data.tmux_enabled ?? true)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      setConfig(null)
    } finally {
      setLoading(false)
    }
  }, [api, onError])

  useEffect(() => {
    void load()
  }, [load])

  const engineReady = config?.engine_ready === true
  const sidecarManaged = config?.sidecar_managed === true
  const canLoadModels =
    config?.sidecar_reachable === true && config?.api_key_configured === true

  const readinessSummary = useMemo(() => {
    if (!config) return ''
    if (engineReady) return 'Cursor SDK is ready (sidecar auto-started with the bot).'
    if (!config.sidecar_reachable) return 'Sidecar not reachable yet — check bot logs on startup.'
    if (!config.api_key_configured) {
      return 'Add CURSOR_API_KEY to repo-root .env and restart the bot.'
    }
    return 'Sidecar is up; waiting for health verification.'
  }, [config, engineReady])

  const loadModels = useCallback(
    async (opts?: { silent?: boolean }) => {
      if (!canLoadModels) {
        if (!opts?.silent) {
          if (config?.sidecar_reachable !== true) {
            onError('Sidecar is not reachable. Restart the bot and check logs.')
          } else {
            onError(
              'CURSOR_API_KEY is not set on the sidecar host. Add it to repo-root .env and restart the bot.',
            )
          }
        }
        return
      }

      setLoadingModels(true)
      setModelsNotice(null)
      try {
        const res = await api<{ models?: Array<{ id: string } | string> }>(
          '/api/cursor-engine/models',
        )
        const ids = (res.models ?? [])
          .map((m) => (typeof m === 'string' ? m : m.id))
          .filter((id): id is string => Boolean(id))
        if (ids.length === 0) {
          const msg = 'No models returned from Cursor API.'
          setModelsNotice(msg)
          if (!opts?.silent) onError(msg)
          return
        }
        setAvailableModels(ids)
        const current = sdkModel.trim()
        if (current && ids.includes(current)) {
          setUseCustomSdkModel(false)
        } else if (current) {
          setUseCustomSdkModel(true)
        } else if (ids[0]) {
          setSdkModel(ids[0])
          setUseCustomSdkModel(false)
        }
        setModelsNotice(`${ids.length} model${ids.length === 1 ? '' : 's'} loaded from Cursor.`)
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setModelsNotice(null)
        if (!opts?.silent) onError(msg)
      } finally {
        setLoadingModels(false)
      }
    },
    [api, canLoadModels, config?.sidecar_reachable, onError, sdkModel],
  )

  useEffect(() => {
    if (!canLoadModels || autoLoadedModelsRef.current) return
    autoLoadedModelsRef.current = true
    void loadModels({ silent: true })
  }, [canLoadModels, loadModels])

  async function save() {
    setSaving(true)
    setSaveNotice(null)
    try {
      const secs = Number.parseInt(timeoutSecs, 10)
      const res = await api<CursorEngineConfigResponse>('/api/cursor-engine', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          sdk_model: sdkModel,
          cli_path: cliPath,
          cli_model: cliModel,
          cli_runner_url: cliRunnerUrl,
          timeout_secs: Number.isFinite(secs) ? secs : 3600,
          tmux_enabled: tmuxEnabled,
        }),
      })
      setSaveNotice(res.message ?? 'Saved.')
      setConfig(res)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  if (loading) {
    return <SettingsPanelSkeleton />
  }

  if (!config?.ok) {
    return (
      <Text size="2" color="gray">
        Could not load Cursor configuration.
      </Text>
    )
  }

  return (
    <Flex direction="column" gap="3">
      <Callout.Root color={engineReady ? 'green' : 'orange'} size="1" variant="soft">
        <Callout.Text>{readinessSummary}</Callout.Text>
      </Callout.Root>

      <Flex gap="2" wrap="wrap" align="center">
        <Text size="1" color={statusColor(config.sidecar_reachable)}>
          Sidecar: {config.sidecar_reachable ? 'reachable' : 'down'}
        </Text>
        <Text size="1" color={statusColor(config.api_key_configured)}>
          CURSOR_API_KEY: {config.api_key_configured ? 'set' : 'missing in .env'}
        </Text>
        <Text size="1" color="gray">
          Runner: {config.sdk_runner_url ?? '—'}
          {sidecarManaged ? ' (managed by bot)' : ''}
        </Text>
      </Flex>

      <div className="rounded-md border p-3" style={{ borderColor: 'var(--gray-6)' }}>
        <Text size="2" weight="bold" className="mb-1 block">
          Cursor SDK engine
        </Text>
        <Text size="1" color="gray" className="mb-2 block">
          The sidecar starts automatically when the bot starts. The bot installs{' '}
          <code>cursor-sdk</code> and <code>aiohttp</code> into a runtime venv on first boot.
          The only required setup is <code>CURSOR_API_KEY</code> in repo-root <code>.env</code>{' '}
          (never commit the value).
        </Text>
        <Flex direction="column" gap="2">
          <Flex gap="2" wrap="wrap" align="center">
            {!useCustomSdkModel && availableModels.length > 0 ? (
              <Select.Root value={sdkModel} onValueChange={setSdkModel}>
                <Select.Trigger placeholder="Select model" style={{ flex: 1, minWidth: 160 }} />
                <Select.Content>
                  {availableModels.map((id) => (
                    <Select.Item key={id} value={id}>
                      {id}
                    </Select.Item>
                  ))}
                </Select.Content>
              </Select.Root>
            ) : (
              <TextField.Root
                placeholder="composer-2.5"
                value={sdkModel}
                onChange={(e) => setSdkModel(e.target.value)}
                style={{ flex: 1, minWidth: 160 }}
              />
            )}
            <Button
              size="1"
              variant="outline"
              disabled={loadingModels || !canLoadModels}
              onClick={() => void loadModels()}
            >
              {loadingModels ? 'Loading…' : 'Refresh models'}
            </Button>
          </Flex>
          {availableModels.length > 0 ? (
            <Button
              size="1"
              variant="ghost"
              type="button"
              onClick={() => setUseCustomSdkModel((v) => !v)}
            >
              {useCustomSdkModel ? 'Use model list' : 'Type custom model id'}
            </Button>
          ) : null}
          {!canLoadModels ? (
            <Text size="1" color="orange">
              Model list requires a reachable sidecar and <code>CURSOR_API_KEY</code> in{' '}
              <code>.env</code>. Without the key, the sidecar returns HTTP 503.
            </Text>
          ) : modelsNotice ? (
            <Text size="1" color="gray" role="status">
              {modelsNotice}
            </Text>
          ) : null}
        </Flex>
      </div>

      <div className="rounded-md border p-3" style={{ borderColor: 'var(--gray-6)' }}>
        <Text size="2" weight="bold" className="mb-1 block">
          cursor_agent CLI tool
        </Text>
        <Text size="1" color="gray" className="mb-2 block">
          Optional settings for the <code>cursor_agent</code> tool (separate from the Cursor engine).
          CLI on PATH: {config.cli_on_path ? 'yes' : 'no'}.
        </Text>
        <Flex direction="column" gap="2">
          <TextField.Root
            placeholder="cursor-agent"
            value={cliPath}
            onChange={(e) => setCliPath(e.target.value)}
          />
          <TextField.Root
            placeholder="Model override (optional)"
            value={cliModel}
            onChange={(e) => setCliModel(e.target.value)}
          />
          <TextField.Root
            placeholder="CLI runner URL (Docker host, optional)"
            value={cliRunnerUrl}
            onChange={(e) => setCliRunnerUrl(e.target.value)}
          />
          <TextField.Root
            placeholder="3600"
            value={timeoutSecs}
            onChange={(e) => setTimeoutSecs(e.target.value)}
          />
          <Flex align="center" justify="between" gap="3">
            <Text size="2">Tmux detach mode</Text>
            <Switch size="2" checked={tmuxEnabled} onCheckedChange={setTmuxEnabled} />
          </Flex>
        </Flex>
      </div>

      <Flex gap="2" align="center" wrap="wrap">
        <Button size="2" className="cursor-pointer" disabled={saving} onClick={() => void save()}>
          {saving ? 'Saving…' : 'Save Cursor settings'}
        </Button>
      </Flex>

      {saveNotice ? (
        <Callout.Root color="green" size="1" variant="soft">
          <Callout.Text role="status">{saveNotice}</Callout.Text>
        </Callout.Root>
      ) : null}
    </Flex>
  )
}
