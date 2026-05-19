import { useCallback, useEffect, useState } from 'react'
import { Button, Callout, Flex, Select, Text, TextField } from '@radix-ui/themes'
import type { LlmConfigResponse } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  onSaved?: (model: string) => void
}

export function SettingsLlmPanel({ api, onError, onSaved }: Props) {
  const [llm, setLlm] = useState<LlmConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [selectedModel, setSelectedModel] = useState('')
  const [customModel, setCustomModel] = useState('')
  const [useCustom, setUseCustom] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveNotice, setSaveNotice] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setSaveNotice(null)
    try {
      const data = await api<LlmConfigResponse>('/api/llm')
      setLlm(data)
      const current = data.model ?? ''
      const inCatalog = data.catalog?.some((m) => m.id === current) ?? false
      if (inCatalog || !current) {
        setUseCustom(false)
        setSelectedModel(current || data.catalog?.[0]?.id || '')
        setCustomModel('')
      } else {
        setUseCustom(true)
        setCustomModel(current)
        setSelectedModel(data.catalog?.[0]?.id || '')
      }
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
      setLlm(null)
    } finally {
      setLoading(false)
    }
  }, [api, onError])

  useEffect(() => {
    void load()
  }, [load])

  async function saveModel() {
    const model = (useCustom ? customModel : selectedModel).trim()
    if (!model) {
      onError('Pick a model or enter a custom model id.')
      return
    }
    setSaving(true)
    setSaveNotice(null)
    try {
      const res = await api<{ ok?: boolean; message?: string; model?: string }>('/api/llm', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model, custom: useCustom }),
      })
      setSaveNotice(res.message ?? 'Model saved.')
      await load()
      onSaved?.(res.model ?? model)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  if (loading) {
    return (
      <Text size="2" color="gray">
        Loading LLM configuration…
      </Text>
    )
  }

  if (!llm?.ok) {
    return (
      <Text size="2" color="gray">
        Could not load LLM configuration.
      </Text>
    )
  }

  const selectedCatalog = llm.catalog?.find((m) => m.id === selectedModel)

  return (
    <Flex direction="column" gap="3">
      <Text size="1" color="gray">
        Provider and API key come from repo-root <code className="text-xs">.env</code> (
        <code className="text-xs">LLM_PROVIDER</code>, <code className="text-xs">LLM_API_KEY</code>
        ). Model choice is saved here and applies to new agent runs without editing{' '}
        <code className="text-xs">.env</code>. The dropdown is a curated list (Anthropic / Google
        models we ship in code), not a live API listing — use custom id for newer model names.
      </Text>

      <Flex gap="2" wrap="wrap" align="center">
        <Text size="2" weight="medium">
          Provider:
        </Text>
        <Text size="2">{llm.provider?.label ?? llm.provider?.id ?? '—'}</Text>
        <Text size="1" color={llm.api_key_configured ? 'green' : 'orange'}>
          API key: {llm.api_key_configured ? 'configured' : 'missing'}
        </Text>
      </Flex>

      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Model
        </Text>
        {!useCustom ? (
          <Select.Root value={selectedModel} onValueChange={setSelectedModel}>
            <Select.Trigger placeholder="Select model" />
            <Select.Content>
              {(llm.catalog ?? []).map((m) => (
                <Select.Item key={m.id} value={m.id}>
                  {m.from_active_config ? `${m.id} (active, from config)` : m.id}
                </Select.Item>
              ))}
            </Select.Content>
          </Select.Root>
        ) : (
          <TextField.Root
            placeholder="Custom model id"
            value={customModel}
            onChange={(e) => setCustomModel(e.target.value)}
          />
        )}
        {llm.custom_model_allowed ? (
          <Button
            size="1"
            variant="ghost"
            type="button"
            onClick={() => {
              setUseCustom((v) => !v)
              setSaveNotice(null)
            }}
          >
            {useCustom ? 'Use catalog list' : 'Use custom model id'}
          </Button>
        ) : null}
      </Flex>

      {(useCustom ? customModel : selectedCatalog) ? (
        <div
          className="rounded-md border p-3 text-sm"
          style={{ borderColor: 'var(--gray-6)' }}
        >
          <Text size="1" weight="bold" className="mb-1 block">
            Cost reference
          </Text>
          {useCustom ? (
            <Text size="1" color="gray">
              Custom model — check your provider&apos;s pricing page.
            </Text>
          ) : selectedCatalog ? (
            <Flex direction="column" gap="1">
              <Text size="1">
                Tier: <span className="capitalize">{selectedCatalog.cost_tier}</span>
              </Text>
              <Text size="1" color="gray">
                {selectedCatalog.cost_summary}
              </Text>
            </Flex>
          ) : null}
          {llm.cost_reference_note ? (
            <Text size="1" color="gray" className="mt-2 block">
              {llm.cost_reference_note}
            </Text>
          ) : null}
        </div>
      ) : null}

      <Flex gap="2" align="center" wrap="wrap">
        <Button size="2" disabled={saving} onClick={() => void saveModel()}>
          {saving ? 'Saving…' : 'Save model'}
        </Button>
        <Text size="1" color="gray">
          Active: <span className="font-mono">{llm.model}</span>
          {llm.model_source === 'app_settings' ? ' (saved in app)' : ' (from .env)'}
        </Text>
      </Flex>

      {saveNotice ? (
        <Callout.Root color="green" size="1" variant="soft">
          <Callout.Text>{saveNotice}</Callout.Text>
        </Callout.Root>
      ) : null}
    </Flex>
  )
}
