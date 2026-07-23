import { useCallback, useEffect, useMemo, useState } from 'react'
import { Button, Callout, Flex, IconButton, Select, Text, TextField, Tooltip } from '@radix-ui/themes'
import { SettingsPanelSkeleton } from './skeleton'
import type { BotInstanceRow, ChannelIntegrationSettings } from '../types'

type Platform = 'telegram' | 'discord' | 'whatsapp'

type Props = {
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  onError: (message: string) => void
  onSaved?: () => void
  requestRestart: () => Promise<void>
  restartBusy: boolean
  appearance: 'dark' | 'light'
}

type InstanceDraft = {
  label: string
  token: string
  botUsername: string
  allowedGroups: string
  discordAllowedChannels: string
  whatsappPhoneNumberId: string
  whatsappVerifyToken: string
  whatsappWebhookPort: string
}

function draftFromRow(row: BotInstanceRow): InstanceDraft {
  return {
    label: row.label ?? '',
    token: '',
    botUsername: row.bot_username ?? '',
    allowedGroups: row.allowed_groups ?? '',
    discordAllowedChannels: row.discord_allowed_channels ?? '',
    whatsappPhoneNumberId: row.whatsapp_phone_number_id ?? '',
    whatsappVerifyToken: '',
    whatsappWebhookPort: String(row.whatsapp_webhook_port ?? 8080),
  }
}

function platformLabel(platform: string): string {
  switch (platform) {
    case 'telegram':
      return 'Telegram'
    case 'discord':
      return 'Discord'
    case 'whatsapp':
      return 'WhatsApp Cloud API'
    default:
      return platform
  }
}

function defaultLabel(platform: Platform): string {
  switch (platform) {
    case 'telegram':
      return 'Telegram bot'
    case 'discord':
      return 'Discord bot'
    case 'whatsapp':
      return 'WhatsApp number'
  }
}

function integrationDescription(row: BotInstanceRow): string {
  if (row.platform === 'telegram') {
    return 'Bot token, @mention username, and optional group allowlist.'
  }
  if (row.platform === 'discord') {
    return 'Bot token and optional Discord channel allowlist.'
  }
  if (row.platform === 'whatsapp') {
    return 'Single supported WhatsApp Business number. Persona routing lives on the Channels tab.'
  }
  return 'External chat bot integration.'
}

export function SettingsIntegrationsPanel({
  api,
  onError,
  onSaved,
  requestRestart,
  restartBusy,
  appearance,
}: Props) {
  const [loading, setLoading] = useState(true)
  const [busyId, setBusyId] = useState<number | 'new' | 'shared' | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [cfg, setCfg] = useState<ChannelIntegrationSettings | null>(null)
  const [instances, setInstances] = useState<BotInstanceRow[]>([])
  const [drafts, setDrafts] = useState<Record<number, InstanceDraft>>({})
  const [controlChatIds, setControlChatIds] = useState('')
  const [newPlatform, setNewPlatform] = useState<Platform>('telegram')
  const [newLabel, setNewLabel] = useState('')
  const [newToken, setNewToken] = useState('')

  const panelStyle =
    appearance === 'dark'
      ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
      : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }

  const applyCfg = useCallback((data: ChannelIntegrationSettings) => {
    const rows = Array.isArray(data.instances) ? data.instances : []
    setCfg(data)
    setInstances(rows)
    setControlChatIds(data.control_chat_ids ?? '')
    setDrafts(Object.fromEntries(rows.map((row) => [row.id, draftFromRow(row)])))
    if (rows.some((row) => row.platform === 'whatsapp') && newPlatform === 'whatsapp') {
      setNewPlatform('telegram')
    }
  }, [newPlatform])

  const load = useCallback(async () => {
    setLoading(true)
    setNotice(null)
    try {
      const data = await api<ChannelIntegrationSettings>('/api/channels/integration')
      applyCfg(data)
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [api, applyCfg, onError])

  useEffect(() => {
    void load()
  }, [load])

  const whatsappExists = useMemo(
    () => instances.some((row) => row.platform === 'whatsapp'),
    [instances],
  )

  function updateDraft(id: number, patch: Partial<InstanceDraft>): void {
    setDrafts((current) => ({
      ...current,
      [id]: { ...(current[id] ?? draftFromRow(instances.find((row) => row.id === id)!)), ...patch },
    }))
  }

  async function saveShared(): Promise<void> {
    setBusyId('shared')
    setNotice(null)
    onError('')
    try {
      const data = await api<ChannelIntegrationSettings>('/api/channels/integration', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ control_chat_ids: controlChatIds }),
      })
      applyCfg(data)
      setNotice(data.message ?? 'Shared access saved.')
      onSaved?.()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyId(null)
    }
  }

  async function saveInstance(row: BotInstanceRow): Promise<void> {
    const draft = drafts[row.id] ?? draftFromRow(row)
    if (!draft.label.trim()) {
      onError('Label is required.')
      return
    }
    setBusyId(row.id)
    setNotice(null)
    onError('')
    try {
      const body: Record<string, unknown> = {
        label: draft.label.trim(),
      }
      if (draft.token.trim()) body.token = draft.token.trim()
      if (row.platform === 'telegram') {
        body.bot_username = draft.botUsername.trim()
        body.allowed_groups = draft.allowedGroups.trim()
      }
      if (row.platform === 'discord') {
        body.discord_allowed_channels = draft.discordAllowedChannels.trim()
      }
      if (row.platform === 'whatsapp') {
        body.whatsapp_phone_number_id = draft.whatsappPhoneNumberId.trim()
        body.whatsapp_webhook_port = Number.parseInt(draft.whatsappWebhookPort, 10) || 8080
        if (draft.whatsappVerifyToken.trim()) {
          body.whatsapp_verify_token = draft.whatsappVerifyToken.trim()
        }
      }
      await api(`/api/channel_bot_instances/${row.id}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      setNotice('Integration saved. Restart the gateway to apply dispatcher changes.')
      await load()
      onSaved?.()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyId(null)
    }
  }

  async function addBotInstance(): Promise<void> {
    const label = newLabel.trim() || defaultLabel(newPlatform)
    const token = newToken.trim()
    if (!token) {
      onError('Token is required.')
      return
    }
    setBusyId('new')
    setNotice(null)
    onError('')
    try {
      const data = await api<{ message?: string }>('/api/channel_bot_instances', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ platform: newPlatform, label, token }),
      })
      setNewLabel('')
      setNewToken('')
      setNotice(data.message ?? 'Bot instance created. Restart the gateway to activate it.')
      await load()
      onSaved?.()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyId(null)
    }
  }

  async function removeBotInstance(row: BotInstanceRow): Promise<void> {
    if (!window.confirm(`Delete ${row.label || platformLabel(row.platform)}? Related channel bindings will be removed.`)) {
      return
    }
    setBusyId(row.id)
    onError('')
    try {
      await api(`/api/channel_bot_instances/${row.id}`, { method: 'DELETE' })
      setNotice('Bot instance removed. Restart the gateway to stop its dispatcher.')
      await load()
      onSaved?.()
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyId(null)
    }
  }

  if (loading || !cfg) {
    return <SettingsPanelSkeleton />
  }

  return (
    <div className="space-y-3">
      {notice ? (
        <Callout.Root color="green" size="1" variant="soft">
          <Callout.Text>{notice}</Callout.Text>
        </Callout.Root>
      ) : null}

      <Callout.Root color="blue" size="1" variant="soft">
        <Callout.Text>
          Integrations are bot credentials and platform access controls. Persona routing for a
          contact lives on the Channels tab.
        </Callout.Text>
      </Callout.Root>

      {instances.length === 0 ? (
        <div className="rounded-md border p-3" style={panelStyle}>
          <Text size="2" color="gray">
            No bot instances configured yet.
          </Text>
        </div>
      ) : null}

      {instances.map((row) => {
        const draft = drafts[row.id] ?? draftFromRow(row)
        const tokenLabel = row.token_set ? `Current token: ${row.token_redacted}` : 'No token set'
        return (
          <div key={row.id} className="rounded-md border p-3" style={panelStyle}>
            <Flex justify="between" gap="2" wrap="wrap" align="start" className="mb-2">
              <div>
                <Text size="2" weight="bold" className="block">
                  {platformLabel(row.platform)} #{row.id}
                  {row.is_primary ? ' · primary' : ''}
                </Text>
                <Text size="1" color="gray">
                  {integrationDescription(row)}
                </Text>
              </div>
              <Text size="1" color={row.token_set ? 'green' : 'orange'}>
                {tokenLabel}
              </Text>
            </Flex>
            <Flex direction="column" gap="2">
              <TextField.Root
                placeholder="Label"
                value={draft.label}
                onChange={(e) => updateDraft(row.id, { label: e.target.value })}
              />
              <TextField.Root
                type="password"
                placeholder={row.token_set ? 'New token (optional)' : 'Bot token'}
                value={draft.token}
                onChange={(e) => updateDraft(row.id, { token: e.target.value })}
                autoComplete="off"
              />
              {row.platform === 'telegram' ? (
                <>
                  <TextField.Root
                    placeholder="Bot username for @mentions (without @)"
                    value={draft.botUsername}
                    onChange={(e) => updateDraft(row.id, { botUsername: e.target.value })}
                  />
                  <TextField.Root
                    placeholder="Allowed group chat IDs (comma-separated; empty = all)"
                    value={draft.allowedGroups}
                    onChange={(e) => updateDraft(row.id, { allowedGroups: e.target.value })}
                  />
                </>
              ) : null}
              {row.platform === 'discord' ? (
                <TextField.Root
                  placeholder="Allowed Discord channel IDs (comma-separated; empty = all)"
                  value={draft.discordAllowedChannels}
                  onChange={(e) => updateDraft(row.id, { discordAllowedChannels: e.target.value })}
                />
              ) : null}
              {row.platform === 'whatsapp' ? (
                <>
                  <TextField.Root
                    placeholder="Phone number ID"
                    value={draft.whatsappPhoneNumberId}
                    onChange={(e) => updateDraft(row.id, { whatsappPhoneNumberId: e.target.value })}
                  />
                  <TextField.Root
                    type="password"
                    placeholder={
                      row.whatsapp_verify_token_set
                        ? `New verify token (optional; current ${row.whatsapp_verify_token_redacted})`
                        : 'Verify token'
                    }
                    value={draft.whatsappVerifyToken}
                    onChange={(e) => updateDraft(row.id, { whatsappVerifyToken: e.target.value })}
                    autoComplete="off"
                  />
                  <TextField.Root
                    placeholder="Webhook port"
                    value={draft.whatsappWebhookPort}
                    onChange={(e) => updateDraft(row.id, { whatsappWebhookPort: e.target.value })}
                  />
                </>
              ) : null}
            </Flex>
            <Flex gap="2" mt="3" wrap="wrap">
              <Button size="1" disabled={busyId === row.id} onClick={() => void saveInstance(row)}>
                {busyId === row.id ? 'Saving…' : 'Save'}
              </Button>
              <Button
                size="1"
                color="red"
                variant="soft"
                disabled={busyId === row.id}
                onClick={() => void removeBotInstance(row)}
              >
                Delete
              </Button>
            </Flex>
          </div>
        )
      })}

      <div className="rounded-md border p-3" style={panelStyle}>
        <Text size="2" weight="bold" className="mb-1 block">
          Add Bot Instance
        </Text>
        <Text size="1" color="gray" className="mb-2 block">
          Telegram and Discord can have multiple bot instances. WhatsApp supports one Business
          number in this gateway.
        </Text>
        <Flex gap="2" wrap="wrap" align="end">
          <div>
            <Text size="1" color="gray" className="mb-1 block">
              Platform
            </Text>
            <Select.Root
              value={newPlatform}
              onValueChange={(value) =>
                setNewPlatform(
                  value === 'discord' ? 'discord' : value === 'whatsapp' ? 'whatsapp' : 'telegram',
                )
              }
            >
              <Select.Trigger className="w-[150px]" />
              <Select.Content>
                <Select.Item value="telegram">telegram</Select.Item>
                <Select.Item value="discord">discord</Select.Item>
                <Select.Item value="whatsapp" disabled={whatsappExists}>
                  whatsapp
                </Select.Item>
              </Select.Content>
            </Select.Root>
          </div>
          <TextField.Root
            className="min-w-[160px] flex-1"
            placeholder={defaultLabel(newPlatform)}
            value={newLabel}
            onChange={(e) => setNewLabel(e.target.value)}
          />
          <TextField.Root
            className="min-w-[220px] flex-1"
            type="password"
            placeholder="Bot token"
            value={newToken}
            onChange={(e) => setNewToken(e.target.value)}
            autoComplete="off"
          />
          <Button size="1" disabled={busyId === 'new'} onClick={() => void addBotInstance()}>
            {busyId === 'new' ? 'Adding…' : 'Add'}
          </Button>
        </Flex>
      </div>

      <div className="rounded-md border p-3" style={panelStyle}>
        <Flex align="center" gap="1" className="mb-1">
          <Text size="2" weight="bold">
            Shared Access
          </Text>
          <Tooltip
            maxWidth="320px"
            side="top"
            content={
              <>
                Chats listed here are control chats. From them, the agent can use cross-chat tools
                (for example send_message, schedule tasks, or read another chat&apos;s history).
                <br />
                <br />
                Enter comma-separated chat IDs (Telegram group/user IDs, Discord channel IDs, etc.).
                This setting is global across all integrations. Restart the gateway after saving.
              </>
            }
          >
            <IconButton
              size="1"
              variant="ghost"
              color="gray"
              type="button"
              aria-label="About shared access"
            >
              <svg
                className="size-3.5 shrink-0"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden
              >
                <circle cx="12" cy="12" r="10" />
                <path d="M12 16v-4" />
                <path d="M12 8h.01" />
              </svg>
            </IconButton>
          </Tooltip>
        </Flex>
        <Text size="1" color="gray" className="mb-2 block">
          Privileged chat IDs for cross-chat tools. This is global, not platform-specific.
        </Text>
        <Flex gap="2" wrap="wrap" align="center">
          <TextField.Root
            className="min-w-[260px] flex-1"
            placeholder="Control chat IDs"
            value={controlChatIds}
            onChange={(e) => setControlChatIds(e.target.value)}
          />
          <Button size="1" disabled={busyId === 'shared'} onClick={() => void saveShared()}>
            {busyId === 'shared' ? 'Saving…' : 'Save Shared Access'}
          </Button>
          <Button size="1" variant="soft" disabled={restartBusy} onClick={() => void requestRestart()}>
            {restartBusy ? 'Restarting…' : 'Restart Gateway'}
          </Button>
        </Flex>
      </div>
    </div>
  )
}
