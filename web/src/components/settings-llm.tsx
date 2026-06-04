import { useCallback, useEffect, useMemo, useState } from 'react'
import { Button, Callout, Flex, Select, Text, TextField } from '@radix-ui/themes'
import type { LlmConfigResponse, LlmProviderOption } from '../types'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  onSaved?: (model: string) => void
}

export function SettingsLlmPanel({ api, onError, onSaved }: Props) {
  const [llm, setLlm] = useState<LlmConfigResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [selectedProvider, setSelectedProvider] = useState('')
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
      const available = data.providers ?? []
      const activeId = data.provider?.id ?? ''
      const providerId = available.some((p) => p.id === activeId)
        ? activeId
        : (available[0]?.id ?? '')
      setSelectedProvider(providerId)
      const providerEntry = data.providers?.find((p) => p.id === providerId)
      const catalog = providerEntry?.models ?? data.catalog ?? []
      const current = data.model ?? ''
      const inCatalog = catalog.some((m) => m.id === current)
      if (inCatalog || !current) {
        setUseCustom(false)
        setSelectedModel(current || catalog[0]?.id || '')
        setCustomModel('')
      } else {
        setUseCustom(true)
        setCustomModel(current)
        setSelectedModel(catalog[0]?.id || '')
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

  const activeProviderEntry = useMemo(
    () => llm?.providers?.find((p) => p.id === selectedProvider),
    [llm?.providers, selectedProvider],
  )

  const catalogForProvider = activeProviderEntry?.models ?? llm?.catalog ?? []

  function onProviderChange(nextProvider: string) {
    setSelectedProvider(nextProvider)
    setSaveNotice(null)
    const entry = llm?.providers?.find((p) => p.id === nextProvider)
    const firstModel = entry?.models?.[0]?.id ?? ''
    if (!useCustom) {
      setSelectedModel(firstModel)
    }
  }

  async function saveSelection() {
    const model = (useCustom ? customModel : selectedModel).trim()
    const provider = selectedProvider.trim()
    if (!provider) {
      onError('Pick a provider.')
      return
    }
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
        body: JSON.stringify({ provider, model, custom: useCustom }),
      })
      setSaveNotice(res.message ?? 'Saved.')
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

  const selectedCatalog = catalogForProvider.find((m) => m.id === selectedModel)

  return (
    <Flex direction="column" gap="3">
      <Text size="1" color="gray">
        Put API keys in repo-root <code className="text-xs">.env</code> only (never in this UI).
        Provider and model are configured here and saved in the app database — not in{' '}
        <code className="text-xs">.env</code>. Lists are curated in code, not live from provider
        APIs; use custom model id for newer releases.
      </Text>

      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Provider
        </Text>
        {(llm.providers ?? []).length === 0 ? (
          <Callout.Root color="orange" size="1" variant="soft">
            <Callout.Text>
              No provider API keys found in <code className="text-xs">.env</code>. Add keys such as{' '}
              <code className="text-xs">ANTHROPIC_API_KEY</code>,{' '}
              <code className="text-xs">OPENAI_API_KEY</code>, or{' '}
              <code className="text-xs">XAI_API_KEY</code>, then reload this page.
            </Callout.Text>
          </Callout.Root>
        ) : (
          <>
            <Select.Root value={selectedProvider} onValueChange={onProviderChange}>
              <Select.Trigger placeholder="Select provider" />
              <Select.Content>
                {(llm.providers ?? []).map((p: LlmProviderOption) => (
                  <Select.Item key={p.id} value={p.id}>
                    {p.label}
                  </Select.Item>
                ))}
              </Select.Content>
            </Select.Root>
            {activeProviderEntry ? (
              <Text size="1" color="green">
                API key found in .env ({activeProviderEntry.api_key_env_hints.join(' or ')})
              </Text>
            ) : null}
          </>
        )}
      </Flex>

      <Flex direction="column" gap="2">
        <Text size="2" weight="medium">
          Model
        </Text>
        {!useCustom ? (
          <Select.Root value={selectedModel} onValueChange={setSelectedModel}>
            <Select.Trigger placeholder="Select model" />
            <Select.Content>
              {catalogForProvider.map((m) => (
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
        <Button
          size="2"
          disabled={saving || (llm.providers ?? []).length === 0}
          onClick={() => void saveSelection()}
        >
          {saving ? 'Saving…' : 'Save provider & model'}
        </Button>
        <Text size="1" color="gray">
          Active:{' '}
          <span className="font-mono">
            {llm.provider?.label ?? llm.provider?.id} / {llm.model}
          </span>
          {llm.provider_source === 'app_settings' && llm.model_source === 'app_settings'
            ? ' (saved in app)'
            : ' (auto-selected — save to confirm)'}
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
