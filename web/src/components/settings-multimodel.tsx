import { useCallback, useEffect, useMemo, useState } from 'react'
import { Button, Callout, Flex, Switch, Text, TextField } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type { MultimodelConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
}

type StepId = 'enable' | 'configure' | 'test' | 'save'

export function SettingsMultimodelPanel({ api, onError }: Props) {
  const [config, setConfig] = useState<MultimodelConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [saveNotice, setSaveNotice] = useState<string | null>(null)
  const [testNotice, setTestNotice] = useState<string | null>(null)
  const [testPassed, setTestPassed] = useState<boolean | null>(null)

  const [enabled, setEnabled] = useState(false)
  const [localBaseUrl, setLocalBaseUrl] = useState('')
  const [localModel, setLocalModel] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    setSaveNotice(null)
    setTestNotice(null)
    try {
      const data = await api<MultimodelConfigResponse>('/api/multimodel')
      setConfig(data)
      setEnabled(data.enabled ?? false)
      setLocalBaseUrl(data.local_base_url ?? data.tier1_base_url ?? '')
      setLocalModel(data.local_model ?? data.tier1_model ?? '')
      setTestPassed(data.local_tools_ok === true ? true : null)
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

  const localToolsOk = config?.local_tools_ok === true
  const configured = localBaseUrl.trim().length > 0 && localModel.trim().length > 0

  const activeStep: StepId = useMemo(() => {
    if (!enabled) return 'enable'
    if (!configured) return 'configure'
    if (!localToolsOk) return 'test'
    return 'save'
  }, [enabled, configured, localToolsOk])

  const steps: { id: StepId; label: string }[] = [
    { id: 'enable', label: '1. Enable' },
    { id: 'configure', label: '2. Configure local' },
    { id: 'test', label: '3. Test tools' },
    { id: 'save', label: '4. Save' },
  ]

  async function save() {
    const toolsOk = config?.local_tools_ok === true
    if (enabled && !toolsOk) {
      onError(
        'Run the tool-calling test for the local model before enabling multi-model routing.',
      )
      return
    }
    setSaving(true)
    setSaveNotice(null)
    try {
      const res = await api<{ message?: string }>('/api/multimodel', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          enabled,
          local_base_url: localBaseUrl,
          local_model: localModel,
        }),
      })
      setSaveNotice(res.message ?? 'Saved.')
      await load()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function testLocal() {
    const base_url = localBaseUrl.trim()
    const model = localModel.trim()
    if (!base_url || !model) {
      onError('Enter server URL and model before testing.')
      return
    }
    setTesting(true)
    setTestNotice(null)
    setTestPassed(null)
    try {
      const res = await api<{ message?: string; tools_ok?: boolean }>('/api/multimodel/test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tier: 'local', base_url, model }),
      })
      const ok = res.tools_ok === true
      setTestPassed(ok)
      setTestNotice(res.message ?? (ok ? 'Local model OK.' : 'Test finished.'))
      await load()
    } catch (e) {
      setTestPassed(false)
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setTesting(false)
    }
  }

  if (loading) {
    return <SettingsPanelSkeleton />
  }

  if (!config?.ok) {
    return (
      <Text size="2" color="gray">
        Could not load multi-model configuration.
      </Text>
    )
  }

  return (
    <Flex direction="column" gap="3">
      <div className="mc-multimodel-stepper" aria-label="Setup progress">
        {steps.map((step) => {
          const done =
            (step.id === 'enable' && enabled) ||
            (step.id === 'configure' && configured) ||
            (step.id === 'test' && localToolsOk) ||
            (step.id === 'save' && enabled && localToolsOk)
          const active = step.id === activeStep
          return (
            <span
              key={step.id}
              className={`mc-multimodel-step ${active ? 'mc-multimodel-step--active' : ''} ${done ? 'mc-multimodel-step--done' : ''}`.trim()}
            >
              {step.label}
            </span>
          )
        })}
      </div>

      <Text size="1" color="gray">
        Route execution iterations to a local llama.cpp server for privacy and cost savings.
        Strategy (planning/synthesis) always uses your Settings → LLM provider.
      </Text>

      <Flex align="center" justify="between" gap="3" wrap="wrap">
        <Flex direction="column" gap="1" style={{ flex: 1, minWidth: 200 }}>
          <Text size="2" weight="medium">
            Enable phase-based routing
          </Text>
          <Text size="1" color="gray">
            Strategy plans, local model executes tool calls, strategy synthesizes the answer.
          </Text>
        </Flex>
        <Switch size="2" checked={enabled} onCheckedChange={setEnabled} />
      </Flex>

      {enabled && !localToolsOk ? (
        <Callout.Root color="orange" size="1" variant="soft">
          <Callout.Text>
            Tool calling is not verified for the local model. Test the connection below before
            saving with routing enabled.
          </Callout.Text>
        </Callout.Root>
      ) : null}

      <div
        className="rounded-md border p-3"
        style={{ borderColor: 'var(--gray-6)' }}
      >
        <Text size="2" weight="bold" className="mb-1 block">
          Local model (execute phase)
        </Text>
        <Text size="1" color="gray" className="mb-2 block">
          OpenAI-compatible server (llama.cpp, vLLM, Ollama).
        </Text>
        <Flex direction="column" gap="2">
          <TextField.Root
            placeholder="http://127.0.0.1:8080/v1"
            value={localBaseUrl}
            onChange={(e) => setLocalBaseUrl(e.target.value)}
          />
          <TextField.Root
            placeholder="qwen3-14b"
            value={localModel}
            onChange={(e) => setLocalModel(e.target.value)}
          />
          <Button
            size="1"
            variant="outline"
            className="cursor-pointer"
            disabled={testing}
            onClick={() => void testLocal()}
          >
            {testing ? 'Testing…' : 'Test local server'}
          </Button>
          <Text
            size="1"
            color={localToolsOk ? 'green' : 'gray'}
            role="status"
            aria-live="polite"
          >
            Tool calling: {localToolsOk ? 'verified' : 'not verified — run test'}
            {testNotice ? ` — ${testNotice}` : ''}
          </Text>
          {testPassed === false ? (
            <Text size="1" color="red" role="status">
              Last test did not pass. Check URL, model name, and server logs.
            </Text>
          ) : null}
        </Flex>
      </div>

      <div
        className="rounded-md border p-3"
        style={{ borderColor: 'var(--gray-6)' }}
      >
        <Text size="2" weight="bold" className="mb-1 block">
          Strategy (plan + synthesize)
        </Text>
        <Text size="1" color="gray">
          Uses{' '}
          <span className="font-mono">
            {config.strategy_provider ?? 'anthropic'} / {config.strategy_model ?? 'claude-sonnet'}
          </span>{' '}
          from Settings → LLM.
        </Text>
      </div>

      <Flex gap="2" align="center" wrap="wrap">
        <Button size="2" className="cursor-pointer" disabled={saving} onClick={() => void save()}>
          {saving ? 'Saving…' : 'Save multi-model settings'}
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
