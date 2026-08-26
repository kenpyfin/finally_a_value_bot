import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Button, Callout, Flex, Select, Switch, Text, TextField } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type { LlmCatalogModel, LlmConfigResponse, LlmLiveCatalogResponse, LlmProviderOption } from '../types'

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
  const [serverUrl, setServerUrl] = useState('')
  const [thinkingEnabled, setThinkingEnabled] = useState(false)
  const [showThinking, setShowThinking] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveNotice, setSaveNotice] = useState<string | null>(null)
  const [liveModels, setLiveModels] = useState<LlmCatalogModel[] | null>(null)
  const [catalogSource, setCatalogSource] = useState<'live' | 'static_fallback' | 'static_curated'>(
    'static_curated',
  )
  const [loadingModels, setLoadingModels] = useState(false)
  const [modelsNotice, setModelsNotice] = useState<string | null>(null)
  const lastLoadedKeyRef = useRef('')

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
      const activeIsLocal =
        providerId === 'ollama' || providerId === 'llama' || providerId === 'llamacpp'
      if (activeIsLocal) {
        const entry = data.providers?.find((p) => p.id === providerId)
        setServerUrl(
          data.base_url?.trim() ||
            entry?.default_base_url?.trim() ||
            data.default_base_url?.trim() ||
            '',
        )
      } else {
        setServerUrl('')
      }
      setThinkingEnabled(data.thinking_enabled === true)
      setShowThinking(data.show_thinking === true)
      setLiveModels(null)
      setCatalogSource('static_curated')
      setModelsNotice(null)
      lastLoadedKeyRef.current = ''
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

  const staticCatalog = activeProviderEntry?.models ?? llm?.catalog ?? []
  const catalogForProvider = liveModels ?? staticCatalog

  const isLocalProvider =
    selectedProvider === 'ollama' ||
    selectedProvider === 'llama' ||
    selectedProvider === 'llamacpp'

  const thinkingSupported =
    selectedProvider === 'google' ||
    selectedProvider === 'gemini' ||
    llm?.thinking_supported === true

  const applyCatalogSelection = useCallback(
    (models: LlmCatalogModel[]) => {
      const current = (useCustom ? customModel : selectedModel).trim() || (llm?.model ?? '').trim()
      if (current && models.some((m) => m.id === current)) {
        setUseCustom(false)
        setSelectedModel(current)
        setCustomModel('')
        return
      }
      if (!current && models[0]) {
        setUseCustom(false)
        setSelectedModel(models[0].id)
      }
    },
    [customModel, llm?.model, selectedModel, useCustom],
  )

  const loadLiveModels = useCallback(
    async (opts?: { silent?: boolean; provider?: string; baseUrl?: string }) => {
      const provider = (opts?.provider ?? selectedProvider).trim()
      if (!provider) return
      const local =
        provider === 'ollama' || provider === 'llama' || provider === 'llamacpp'
      const baseUrl = (opts?.baseUrl ?? serverUrl).trim()
      if (local && !baseUrl) {
        if (!opts?.silent) {
          onError('Enter the local server URL before loading models.')
        }
        return
      }

      const loadKey = `${provider}|${local ? baseUrl : ''}`
      setLoadingModels(true)
      setModelsNotice(null)
      try {
        const qs = new URLSearchParams({ provider })
        if (local) qs.set('base_url', baseUrl)
        const res = await api<LlmLiveCatalogResponse>(`/api/llm/models?${qs.toString()}`)
        const models = res.models ?? []
        lastLoadedKeyRef.current = loadKey
        setLiveModels(models)
        setCatalogSource(res.source === 'live' ? 'live' : 'static_fallback')
        applyCatalogSelection(models)
        if (res.source === 'live') {
          const n = res.live_count ?? models.filter((m) => m.from_live).length
          const truncated = res.truncated ? ' (truncated; use custom id for others)' : ''
          setModelsNotice(`${n} model${n === 1 ? '' : 's'} loaded from provider API${truncated}.`)
        } else {
          const msg = res.message?.trim() || 'Provider API unavailable; showing curated list.'
          setModelsNotice(msg)
          if (!opts?.silent) onError(msg)
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setLiveModels(null)
        setCatalogSource('static_fallback')
        setModelsNotice(null)
        if (!opts?.silent) onError(msg)
      } finally {
        setLoadingModels(false)
      }
    },
    [api, applyCatalogSelection, onError, selectedProvider, serverUrl],
  )

  useEffect(() => {
    if (!selectedProvider || loading) return
    const local =
      selectedProvider === 'ollama' ||
      selectedProvider === 'llama' ||
      selectedProvider === 'llamacpp'
    if (local && !serverUrl.trim()) {
      lastLoadedKeyRef.current = ''
      return
    }
    const loadKey = `${selectedProvider}|${local ? serverUrl.trim() : ''}`
    if (loadKey === lastLoadedKeyRef.current) return

    const timer = window.setTimeout(() => {
      void loadLiveModels({ silent: true, provider: selectedProvider, baseUrl: serverUrl })
    }, local ? 500 : 0)

    return () => window.clearTimeout(timer)
  }, [loadLiveModels, loading, selectedProvider, serverUrl])

  function onProviderChange(nextProvider: string) {
    setSelectedProvider(nextProvider)
    setSaveNotice(null)
    setLiveModels(null)
    setCatalogSource('static_curated')
    setModelsNotice(null)
    lastLoadedKeyRef.current = ''
    const entry = llm?.providers?.find((p) => p.id === nextProvider)
    const firstModel = entry?.models?.[0]?.id ?? ''
    if (!useCustom) {
      setSelectedModel(firstModel)
    }
    const local =
      nextProvider === 'ollama' || nextProvider === 'llama' || nextProvider === 'llamacpp'
    if (local) {
      setServerUrl(entry?.default_base_url?.trim() || '')
    } else {
      setServerUrl('')
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
    if (isLocalProvider && !serverUrl.trim()) {
      onError('Enter the local server URL (OpenAI-compatible /v1 base).')
      return
    }
    setSaving(true)
    setSaveNotice(null)
    try {
      const res = await api<{ ok?: boolean; message?: string; model?: string }>('/api/llm', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          provider,
          model,
          custom: useCustom,
          thinking_enabled: thinkingEnabled,
          show_thinking: showThinking,
          ...(isLocalProvider ? { base_url: serverUrl.trim() } : {}),
        }),
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
    return <SettingsPanelSkeleton />
  }

  if (!llm?.ok) {
    return (
      <Text size="2" color="gray">
        Could not load LLM configuration.
      </Text>
    )
  }

  const selectedCatalog = catalogForProvider.find((m) => m.id === selectedModel)
  const catalogLabel = (m: LlmCatalogModel) => {
    if (m.from_active_config) return `${m.id} (active, from config)`
    return m.id
  }

  return (
    <Flex direction="column" gap="3">
      <Text size="1" color="gray">
        Put API keys in repo-root <code className="text-xs">.env</code> only (never in this UI).
        Provider and model are configured here and saved in the app database - not in{' '}
        <code className="text-xs">.env</code>. Model lists load live from the provider API; curated
        cost hints are shown when the id matches.
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
              isLocalProvider ? (
                <Text size="1" color="green">
                  Local provider — no API key required.
                </Text>
              ) : (
                <Text size="1" color="green">
                  API key found in .env ({activeProviderEntry.api_key_env_hints.join(' or ')})
                </Text>
              )
            ) : null}
          </>
        )}
      </Flex>

      {isLocalProvider ? (
        <Flex direction="column" gap="2">
          <Text size="2" weight="medium">
            Server URL
          </Text>
          <TextField.Root
            placeholder={
              selectedProvider === 'ollama'
                ? 'http://127.0.0.1:11434/v1'
                : 'http://127.0.0.1:8080/v1'
            }
            value={serverUrl}
            onChange={(e) => setServerUrl(e.target.value)}
          />
          <Text size="1" color="gray">
            OpenAI-compatible API base (include <code className="text-xs">/v1</code>). Saved in the
            app database — not read from <code className="text-xs">.env</code>.
          </Text>
        </Flex>
      ) : null}

      <Flex direction="column" gap="2">
        <Flex align="center" justify="between" gap="3">
          <Text size="2" weight="medium">
            Model
          </Text>
          <Button
            size="1"
            variant="soft"
            type="button"
            disabled={loadingModels || (llm.providers ?? []).length === 0}
            onClick={() => void loadLiveModels()}
          >
            {loadingModels ? 'Loading…' : 'Refresh models'}
          </Button>
        </Flex>
        {!useCustom ? (
          <Select.Root value={selectedModel} onValueChange={setSelectedModel}>
            <Select.Trigger placeholder="Select model" />
            <Select.Content>
              {catalogForProvider.map((m) => (
                <Select.Item key={m.id} value={m.id}>
                  {catalogLabel(m)}
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
        {modelsNotice ? (
          <Text size="1" color={catalogSource === 'live' ? 'green' : 'gray'}>
            {modelsNotice}
          </Text>
        ) : (
          <Text size="1" color="gray">
            {catalogSource === 'live'
              ? 'Showing live models from the provider API.'
              : 'Showing curated fallback until the provider API responds.'}
          </Text>
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
              Custom model - check your provider&apos;s pricing page.
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

      <div className="rounded-md border p-3" style={{ borderColor: 'var(--gray-6)' }}>
        <Text size="2" weight="bold" className="mb-1 block">
          Thinking
        </Text>
        <Flex direction="column" gap="2">
          <Flex align="center" justify="between" gap="3">
            <Flex direction="column" gap="1" style={{ flex: 1 }}>
              <Text size="2">Enable extended thinking</Text>
              <Text size="1" color="gray">
                {thinkingSupported
                  ? 'Sends provider thinking config (Gemini thinkingLevel / thinkingBudget).'
                  : 'Currently supported for Google (Gemini API) only.'}
              </Text>
            </Flex>
            <Switch
              size="2"
              checked={thinkingEnabled}
              disabled={!thinkingSupported}
              onCheckedChange={(checked) => {
                setThinkingEnabled(checked)
                if (!checked) setShowThinking(false)
              }}
            />
          </Flex>
          <Flex align="center" justify="between" gap="3">
            <Flex direction="column" gap="1" style={{ flex: 1 }}>
              <Text size="2">Show thinking in replies</Text>
              <Text size="1" color="gray">
                When enabled, reasoning is included in channel output instead of being hidden.
              </Text>
            </Flex>
            <Switch
              size="2"
              checked={showThinking}
              disabled={!thinkingEnabled}
              onCheckedChange={setShowThinking}
            />
          </Flex>
        </Flex>
      </div>

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
            {llm.is_local_provider && llm.base_url ? ` @ ${llm.base_url}` : ''}
          </span>
          {llm.provider_source === 'app_settings' && llm.model_source === 'app_settings'
            ? ' (saved in app)'
            : ' (auto-selected - save to confirm)'}
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
