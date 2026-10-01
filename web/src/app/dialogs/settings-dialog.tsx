import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from '@/components/ui/select'
import { Dialog, DialogContent, DialogTitle, DialogDescription, DialogClose } from '@/components/ui/dialog'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import React from 'react'

import { SettingsPanelSkeleton, OverviewStatusSkeleton } from '../../components/skeleton'
import type { SettingsDialogProps } from './types'

const SettingsAgentEnginePanel = React.lazy(() =>
  import('../../components/settings-agent-engine').then((m) => ({ default: m.SettingsAgentEnginePanel })),
)
const SettingsPersonaPanel = React.lazy(() =>
  import('../../components/settings-persona').then((m) => ({ default: m.SettingsPersonaPanel })),
)
const SettingsHooksSkillsPanel = React.lazy(() =>
  import('../../components/settings-hooks-skills').then((m) => ({ default: m.SettingsHooksSkillsPanel })),
)
const SettingsRuntimePanel = React.lazy(() =>
  import('../../components/settings-runtime').then((m) => ({ default: m.SettingsRuntimePanel })),
)
const SettingsIntegrationsPanel = React.lazy(() =>
  import('../../components/settings-integrations').then((m) => ({ default: m.SettingsIntegrationsPanel })),
)

type VaultSecretRow = {
  name: string
  kind: string
  skill: string | null
  created_at: string
}

function VaultedSecretsPanel({
  api,
  open,
  chatId,
}: {
  api: SettingsDialogProps['api']
  open: boolean
  chatId: number | null
}) {
  const [items, setItems] = React.useState<VaultSecretRow[]>([])
  const [error, setError] = React.useState('')
  const [busyName, setBusyName] = React.useState<string | null>(null)

  const load = React.useCallback(async () => {
    try {
      const data = await api<{ secrets: VaultSecretRow[] }>('/api/secrets')
      setItems(data.secrets)
      setError('')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not load vaulted secrets')
    }
  }, [api])

  React.useEffect(() => {
    if (open) void load()
  }, [open, load])

  async function forget(name: string) {
    setBusyName(name)
    try {
      await api(`/api/secrets?name=${encodeURIComponent(name)}`, { method: 'DELETE' })
      await load()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not forget that secret')
    } finally {
      setBusyName(null)
    }
  }

  return (
    <section className="mb-4 rounded-lg border border-border p-3">
      <h3 className="text-sm font-medium">Vaulted secrets</h3>
      <p className="mt-1 text-xs text-muted-foreground">
        Names stored for chat {chatId ?? '—'} after the agent calls vault_secret. Values stay in the
        runtime secrets file and are not shown here.
      </p>
      {error ? <p className="mt-2 text-sm text-destructive">{error}</p> : null}
      {items.length === 0 ? (
        <p className="mt-2 text-sm text-muted-foreground">No vaulted secrets.</p>
      ) : (
        <ul className="mt-2 space-y-2">
          {items.map((item) => (
            <li key={item.name} className="flex items-center justify-between gap-3 text-sm">
              <span>
                <code className="text-xs">{item.name}</code>
                <span className="ml-2 text-xs text-muted-foreground">
                  {item.kind}
                  {item.skill ? ` · ${item.skill}` : ''}
                </span>
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={busyName === item.name}
                onClick={() => void forget(item.name)}
              >
                Forget
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

export function SettingsDialog({ appearance, api, chatId, activePersonaId, personas, settings }: SettingsDialogProps) {
  const settingsDialogOpen = settings.open
  const setSettingsDialogOpen = settings.onOpenChange
  const settingsError = settings.error
  const setSettingsError = settings.setError
  const restartNotice = settings.restartNotice
  const installationStatus = settings.installationStatus
  const restartBusy = settings.restartBusy
  const requestRestart = settings.requestRestart
  const bindings = settings.bindings
  const updateChannelPersonaPolicy = settings.updateChannelPersonaPolicy
  const reloadInstallationStatus = settings.reloadInstallationStatus
  const onPersonaBulletinChanged = settings.onPersonaBulletinChanged
  const onCreatePersona = settings.onCreatePersona
  const onRenamePersona = settings.onRenamePersona
  const onDeletePersona = settings.onDeletePersona

  return (
    <Dialog open={settingsDialogOpen} onOpenChange={setSettingsDialogOpen}>
      <DialogContent className="max-w-[920px]">
        <DialogTitle>Web UI configuration</DialogTitle>
        <DialogDescription className="mb-3">
          Put LLM API keys in repo-root <code className="text-xs">.env</code> (e.g.{' '}
          <code className="text-xs">ANTHROPIC_API_KEY</code>, <code className="text-xs">OPENAI_API_KEY</code>).
          Configure Telegram, Discord, and WhatsApp under the Integrations tab (saved in the app database).
          Restart the gateway after changing API keys or channel tokens.
        </DialogDescription>
        {settingsError ? (
          <div role="status" className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive mb-2">{settingsError}</div>
        ) : null}
        {restartNotice ? (
          <div role="status" className="rounded-lg border border-green-500/30 bg-green-500/10 p-3 text-sm text-green-900 dark:text-green-100 mb-2">{restartNotice}</div>
        ) : null}
        <Tabs defaultValue="overview">
          <TabsList className="mc-settings-tabs-sticky mb-3 h-auto flex-wrap">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="agent-engine">Agent engine</TabsTrigger>
            <TabsTrigger value="hooks-skills">Hooks & Skills</TabsTrigger>
            <TabsTrigger value="integrations">Integrations</TabsTrigger>
            <TabsTrigger value="channels">Channels</TabsTrigger>
          </TabsList>
          <React.Suspense fallback={<SettingsPanelSkeleton />}>
          <TabsContent value="overview">
            <VaultedSecretsPanel api={api} open={settingsDialogOpen} chatId={chatId} />
            {onCreatePersona && onRenamePersona && onDeletePersona ? (
              <div className="mb-3">
                <SettingsPersonaPanel
                  activePersonaId={activePersonaId}
                  personas={personas}
                  onCreatePersona={onCreatePersona}
                  onRenamePersona={onRenamePersona}
                  onDeletePersona={onDeletePersona}
                />
              </div>
            ) : null}
            {installationStatus ? (
              <div className="flex flex-col gap-2 mb-2">
                <div className="flex gap-2 flex-wrap items-center">
                  <span>
                    LLM: {installationStatus.llm_ready ? 'ready' : 'missing'}
                  </span>
                  <span>
                    Channels: {installationStatus.channel_ready ? 'ready' : 'missing'}
                  </span>
                  <span>
                    Cursor engine:{' '}
                    {installationStatus.cursor_engine_ready ? 'ready' : 'not ready'}
                  </span>
                  {installationStatus.agent_engine === 'classic_cost_routing' ? (
                    <span>
                      Local delegate:{' '}
                      {installationStatus.local_delegate_ready ? 'verified' : 'not verified'}
                    </span>
                  ) : null}
                  <span>
                    Env restart needed:{' '}
                    {(installationStatus.requires_restart_for_env_changes ??
                      installationStatus.requires_restart_to_apply_runtime_settings) === true
                      ? 'yes'
                      : 'no'}
                  </span>
                </div>
                <span>
                  Stop requests between LLM/tool iterations (cooperative cancel). Use Queue for FIFO visibility.
                </span>
                <div>
                  <Button
                    size="sm"
                    variant="default"
                    disabled={restartBusy}
                    onClick={() => void requestRestart()}
                  >
                    {restartBusy ? 'Restarting…' : 'Restart gateway'}
                  </Button>
                </div>
              </div>
            ) : (
              <OverviewStatusSkeleton />
            )}
            <div
              className="rounded-md border p-3 mt-3"
              style={appearance === 'dark'
                ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
                : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
            >
              <span className="font-semibold mb-2 block">
                Runtime toggles
              </span>
              <SettingsRuntimePanel api={api} onError={setSettingsError} />
            </div>
          </TabsContent>
          <TabsContent value="agent-engine">
            <SettingsAgentEnginePanel
              api={api}
              onError={setSettingsError}
              activePersonaId={activePersonaId}
              personas={personas}
              onPersonaBulletinChanged={onPersonaBulletinChanged}
            />
          </TabsContent>
          <TabsContent value="hooks-skills">
            <SettingsHooksSkillsPanel
              api={api}
              onError={setSettingsError}
              activePersonaId={activePersonaId}
            />
          </TabsContent>
          <TabsContent value="integrations">
            <SettingsIntegrationsPanel
              api={api}
              appearance={appearance}
              onError={setSettingsError}
              onSaved={() => void reloadInstallationStatus()}
              requestRestart={requestRestart}
              restartBusy={restartBusy}
            />
          </TabsContent>
          <TabsContent value="channels">
        <div
          className="rounded-md border p-3"
          style={appearance === 'dark'
            ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
            : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
        >
          <span className="font-semibold">External channel persona mode</span>
          <span className="mb-2 block">
            Bot integrations appear here for this contact’s main chat. Replies are directional: each channel only receives responses to messages it sent. All traffic still appears in the web UI. Telegram, Discord, and WeCom can use all personas or a single persona. WhatsApp is single-persona by design. Focused sessions stay in the web UI.
          </span>
          <div className="space-y-2">
            {bindings.length === 0 ? (
              <span>No bot integrations configured. Add bots under the Integrations tab.</span>
            ) : bindings.map((b) => {
              const platform = b.platform ?? b.channel_type
              const currentMode = platform === 'whatsapp'
                ? 'single'
                : b.persona_mode === 'single' ? 'single' : 'all'
              const currentPersonaId = b.persona_id ?? activePersonaId ?? personas[0]?.id ?? null
              const handleLabel = b.linked && b.channel_handle
                ? b.channel_handle
                : 'pending link'
              const otherAllOnPlatform = bindings.some(
                (other) =>
                  other.bot_instance_id !== b.bot_instance_id &&
                  (other.platform ?? other.channel_type) === platform &&
                  other.persona_mode !== 'single',
              )
              const allPersonasDisabled = otherAllOnPlatform && currentMode !== 'all'
              return (
                <div className="flex gap-2 items-center flex-wrap" key={`${b.bot_instance_id}:${b.channel_type}:${b.channel_handle ?? 'pending'}`}>
                  <span className="min-w-[220px]">
                    {b.label ? `${b.label} · ` : ''}{platform} (bot #{b.bot_instance_id}): {handleLabel}
                  </span>
                  <Select
                    value={currentMode}
                    onValueChange={(mode) => {
                      if (platform === 'whatsapp') {
                        if (currentPersonaId != null) {
                          void updateChannelPersonaPolicy(b.bot_instance_id, 'single', currentPersonaId)
                        }
                        return
                      }
                      if (mode === 'all') {
                        if (allPersonasDisabled) {
                          setSettingsError(
                            `Only one ${platform} bot can use all personas for this contact. Lock the other bot to a single persona first.`,
                          )
                          return
                        }
                        void updateChannelPersonaPolicy(b.bot_instance_id, 'all')
                      } else if (currentPersonaId != null) {
                        void updateChannelPersonaPolicy(b.bot_instance_id, 'single', currentPersonaId)
                      }
                    }}
                  >
                    <SelectTrigger className="w-[140px]" ><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {platform === 'whatsapp' ? null : (
                        <SelectItem value="all" disabled={allPersonasDisabled}>
                          All personas
                        </SelectItem>
                      )}
                      {platform === 'whatsapp' ? null : (
                        <SelectItem value="single">Single persona</SelectItem>
                      )}
                      {platform === 'whatsapp' ? (
                        <SelectItem value="single">Single persona</SelectItem>
                      ) : null}
                    </SelectContent>
                  </Select>
                  {currentMode === 'single' ? (
                    <Select
                      value={currentPersonaId != null ? String(currentPersonaId) : ''}
                      onValueChange={(value) => {
                        const pid = Number(value)
                        if (Number.isFinite(pid) && pid > 0) {
                          void updateChannelPersonaPolicy(b.bot_instance_id, 'single', pid)
                        }
                      }}
                    >
                      <SelectTrigger className="w-[180px]" placeholder="Persona" ><SelectValue /></SelectTrigger>
                      <SelectContent>
                        {personas.map((p) => (
                          <SelectItem key={p.id} value={String(p.id)}>
                            {p.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  ) : null}
                </div>
              )
            })}
          </div>
        </div>
          </TabsContent>
          </React.Suspense>
        </Tabs>
        <div className="flex justify-end mt-4">
          <DialogClose>
            <Button variant="secondary">Close</Button>
          </DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  )
}
