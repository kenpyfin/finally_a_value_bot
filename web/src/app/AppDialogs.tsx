import React from 'react'
import {
  Button,
  Callout,
  Dialog,
  Flex,
  Select,
  Switch,
  Tabs,
  Text,
  TextArea,
  TextField,
} from '@radix-ui/themes'
import remarkGfm from 'remark-gfm'
import ReactMarkdown from 'react-markdown'
import { InboxPanel } from '../components/inbox-panel'
import { InitialRunPromptView } from '../components/initial-run-prompt-view'
import {
  ArtifactListSkeleton,
  ContentPreviewSkeleton,
  OverviewStatusSkeleton,
  SettingsPanelSkeleton,
} from '../components/skeleton'
import {
  formatConfidence,
  formatTierBadgeLabel,
  pdqeStepBadgeKind,
  pdqeStepLabel,
  type ParsedAgentHistory,
  type PdqeEvalDetail,
  type TierRouteInfo,
} from '../parse-agent-history'
import type {
  ArtifactItem,
  BackgroundJobItem,
  ChannelBinding,
  InstallationStatus,
  Persona,
  QueueItem,
  ScheduleTask,
} from '../types'

const SettingsLlmPanel = React.lazy(() =>
  import('../components/settings-llm').then((m) => ({ default: m.SettingsLlmPanel })),
)
const SettingsLocalDelegatePanel = React.lazy(() =>
  import('../components/settings-local-delegate').then((m) => ({ default: m.SettingsLocalDelegatePanel })),
)
const SettingsCursorPanel = React.lazy(() =>
  import('../components/settings-cursor').then((m) => ({ default: m.SettingsCursorPanel })),
)
const SettingsDeterministicPipelinePanel = React.lazy(() =>
  import('../components/settings-deterministic-pipeline').then((m) => ({
    default: m.SettingsDeterministicPipelinePanel,
  })),
)
const SettingsHooksSkillsPanel = React.lazy(() =>
  import('../components/settings-hooks-skills').then((m) => ({ default: m.SettingsHooksSkillsPanel })),
)
const SettingsIntegrationsPanel = React.lazy(() =>
  import('../components/settings-integrations').then((m) => ({ default: m.SettingsIntegrationsPanel })),
)
const SettingsRuntimePanel = React.lazy(() =>
  import('../components/settings-runtime').then((m) => ({ default: m.SettingsRuntimePanel })),
)
const TerminalPane = React.lazy(() =>
  import('../components/terminal-pane').then((m) => ({ default: m.TerminalPane })),
)
export type Appearance = 'dark' | 'light'

export interface AppDialogsSettingsProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  error: string
  setError: (value: string) => void
  restartNotice: string | null
  installationStatus: InstallationStatus | null
  restartBusy: boolean
  requestRestart: () => Promise<void>
  bindings: ChannelBinding[]
  updateChannelPersonaPolicy: (
    botInstanceId: number,
    mode: 'all' | 'single',
    personaId?: number,
  ) => Promise<void>
  reloadInstallationStatus: () => Promise<void>
}

export interface AppDialogsQueueProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  showAllPersonas: boolean
  setShowAllPersonas: (v: boolean) => void
  items: QueueItem[]
  stoppingRunIds: string[]
  handleQueueAction: (runId: string, state: string) => Promise<void>
  backgroundJobs: BackgroundJobItem[]
  stoppingBackgroundJobIds: string[]
  isActiveBackgroundJobStatus: (status: string) => boolean
  handleBackgroundJobStop: (jobId: string) => Promise<void>
  onOpenSchedules: () => void
}

export interface AppDialogsSchedulesProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  showArchived: boolean
  setShowArchived: (v: boolean) => void
  schedules: ScheduleTask[]
  filtered: ScheduleTask[]
  newPrompt: string
  setNewPrompt: (v: string) => void
  newType: 'cron' | 'once'
  setNewType: (v: 'cron' | 'once') => void
  newValue: string
  setNewValue: (v: string) => void
  newPersonaId: number | null
  setNewPersonaId: (v: number) => void
  createSchedule: (
    prompt: string,
    scheduleType: 'cron' | 'once',
    scheduleValue: string,
    personaId: number | null,
  ) => Promise<void>
  updateSchedule: (id: number, patch: Partial<ScheduleTask>) => Promise<void>
  openDetail: (task: ScheduleTask) => void
}

export interface AppDialogsInboxProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  unread: {
    personaId: number
    personaName: string
    lastBotMessageAt: string | null
    sessionId: string | null
    sessionTitle: string | null
  }[]
  todos: import('../types').PersonaTodo[]
  loading: boolean
  busyTodoId: number | null
  onRefresh: () => void
  onOpenTarget: (target: { personaId: number; sessionId: string | null }) => void
  onCompleteTodo: (todoId: number) => void
}

export interface AppDialogsScheduleDetailProps {
  task: ScheduleTask | null
  onOpenChange: (open: boolean) => void
  prompt: string
  setPrompt: (v: string) => void
  scheduleType: 'cron' | 'once'
  setScheduleType: (v: 'cron' | 'once') => void
  scheduleValue: string
  setScheduleValue: (v: string) => void
  busy: boolean
  setBusy: (v: boolean) => void
  updateSchedule: (id: number, patch: Partial<ScheduleTask>) => Promise<void>
  close: () => void
}

export interface AppDialogsAgentsMdProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  path: string
  error: string
  setError: (v: string) => void
  setBusy: (v: boolean) => void
  content: string
  setContent: (v: string) => void
  mtimeMs: number | null
  busy: boolean
  load: () => Promise<void>
  save: () => Promise<void>
}

export interface AppDialogsArtifactsProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  setError: (v: string) => void
  setTextError: (v: string) => void
  kindFilter: string
  setKindFilter: (v: string) => void
  load: (chatId: number | null, personaId: number | null) => Promise<void>
  busy: boolean
  error: string
  items: ArtifactItem[]
  selectedId: string | null
  setSelectedId: (id: string | null) => void
  selected: ArtifactItem | null
  textPreview: string
  textBusy: boolean
  textError: string
}

export interface AppDialogsMemoryProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  setError: (v: string) => void
  setBusy: (v: boolean) => void
  pathHint: string
  error: string
  content: string
  setContent: (v: string) => void
  mtimeMs: number | null
  busy: boolean
  load: (personaId: number) => Promise<void>
  save: (personaId: number) => Promise<void>
}

export interface AppDialogsAgentHistoryProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  setTab: (tab: 'trace' | 'prompt' | 'evaluators') => void
  setError: (v: string) => void
  setBusy: (v: boolean) => void
  pathHint: string
  filename: string
  mtimeMs: number | null
  busy: boolean
  error: string
  parsed: ParsedAgentHistory | null
  tab: 'trace' | 'prompt' | 'evaluators'
  iterationIdx: number
  setIterationIdx: React.Dispatch<React.SetStateAction<number>>
  raw: string
  optimizeBusy: boolean
  optimizeNotes: string
  setOptimizeNotes: (v: string) => void
  load: (personaId: number) => Promise<void>
  optimize: (personaId: number, operatorNotes?: string) => Promise<void>
}

export interface AppDialogsTerminalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  error: string
  setError: (v: string) => void
}

export interface AppDialogsProps {
  appearance: Appearance
  api: <T>(path: string, init?: RequestInit) => Promise<T>
  chatId: number | null
  activePersonaId: number | null
  personas: Persona[]
  settings: AppDialogsSettingsProps
  queue: AppDialogsQueueProps
  schedules: AppDialogsSchedulesProps
  inbox: AppDialogsInboxProps
  scheduleDetail: AppDialogsScheduleDetailProps
  agentsMd: AppDialogsAgentsMdProps
  artifacts: AppDialogsArtifactsProps
  memory: AppDialogsMemoryProps
  agentHistory: AppDialogsAgentHistoryProps
  terminal: AppDialogsTerminalProps
}

function formatBytes(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 'unknown size'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / (1024 * 1024)).toFixed(1)} MB`
}

function artifactPreviewUrl(item: ArtifactItem): string {
  if (item.kind === 'html') return item.preview_url || `${item.url}?preview=1`
  return item.url
}

function MarkdownExternalLink(props: React.ComponentPropsWithoutRef<'a'>) {
  const mergedRel = [props.rel, 'noopener', 'noreferrer'].filter(Boolean).join(' ')
  return <a {...props} target="_blank" rel={mergedRel} />
}

function AgentHistoryMarkdownBody({ markdown }: { markdown: string }) {
  return (
    <div className="aui-md-root text-sm leading-relaxed">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: (props) => <MarkdownExternalLink {...props} />,
          table: ({ className, ...props }) => (
            <div className="mc-md-table-scroll">
              <table className={['aui-md-table', className].filter(Boolean).join(' ')} {...props} />
            </div>
          ),
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  )
}

function tierBadgeClass(tier: string): string {
  switch (tier) {
    case 'technical':
      return 'border-[color:var(--mc-accent-primary)]/35 bg-[color:var(--mc-accent-primary)]/10 text-[color:var(--mc-accent-primary)]'
    case 'knowledge':
      return 'border-[color:var(--mc-accent-secondary)]/35 bg-[color:var(--mc-accent-secondary)]/10 text-[color:var(--mc-text-secondary)]'
    case 'strategy':
    default:
      return 'border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] text-[color:var(--mc-text-primary)]'
  }
}

function EvalDetailBlock({ evalDetail }: { evalDetail: PdqeEvalDetail }) {
  const confidenceLabel = formatConfidence(evalDetail.confidence)
  return (
    <Flex direction="column" gap="2" className="mt-2">
      {evalDetail.verdict ? (
        <Text size="1" color="gray">
          Verdict: <Text weight="medium">{evalDetail.verdict}</Text>
          {confidenceLabel ? ` · confidence ${confidenceLabel}` : ''}
        </Text>
      ) : confidenceLabel ? (
        <Text size="1" color="gray">
          Confidence: <Text weight="medium">{confidenceLabel}</Text>
        </Text>
      ) : null}
      {evalDetail.note ? (
        <Text size="1" color="gray">
          {evalDetail.note}
        </Text>
      ) : null}
      {evalDetail.reason ? (
        <Text size="2">Skip reason: {evalDetail.reason}</Text>
      ) : null}
      {evalDetail.error ? (
        <Text size="2" color="orange">
          {evalDetail.error}
        </Text>
      ) : null}
      {evalDetail.issues && evalDetail.issues.length > 0 ? (
        <div>
          <Text size="1" weight="medium" className="mb-1 block">
            Issues
          </Text>
          <ul className="mc-eval-issues list-disc pl-4">
            {evalDetail.issues.map((issue, idx) => (
              <li key={idx}>
                <Text size="2">{issue}</Text>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {evalDetail.feedback ? (
        <div className="mc-eval-feedback rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-surface-elevated)] p-2">
          <Text size="1" weight="medium" className="mb-1 block">
            Evaluator feedback
          </Text>
          <Text size="2" className="whitespace-pre-wrap">
            {evalDetail.feedback}
          </Text>
        </div>
      ) : null}
      {!evalDetail.verdict &&
      !evalDetail.feedback &&
      !evalDetail.issues?.length &&
      evalDetail.raw &&
      !evalDetail.raw.startsWith('{') ? (
        <Text size="2">{evalDetail.raw}</Text>
      ) : null}
    </Flex>
  )
}

function pteSourceLabel(source?: string): string | null {
  switch (source) {
    case 'llm':
      return 'LLM evaluation'
    case 'heuristic':
      return 'Heuristic stall detector'
    case 'disabled':
      return 'Disabled'
    case 'error':
      return 'Skipped / error'
    default:
      return null
  }
}

function AgentHistoryEvaluatorsPanel({ parsed }: { parsed: ParsedAgentHistory }) {
  const pte = parsed.pteDecisions ?? []
  const pdqe = parsed.pdqeSteps ?? []

  return (
    <Flex direction="column" gap="4">
      <div>
        <Text size="2" weight="medium" className="mb-2 block">
          Post-tool evaluator (PTE)
        </Text>
        {pte.length === 0 ? (
          <Text size="2" color="gray">
            PTE disabled or no tool iterations evaluated.
          </Text>
        ) : (
          <Flex direction="column" gap="2">
            {pte.map((row, i) => {
              const sourceLabel = pteSourceLabel(row.source)
              return (
                <div
                  key={`pte-${row.iteration}-${i}`}
                  className="mc-eval-row rounded-md border border-[color:var(--mc-border-soft)] p-2"
                >
                  <Flex justify="between" align="start" gap="2" wrap="wrap">
                    <Text size="2" weight="medium">
                      Iteration {row.iteration}
                    </Text>
                    <span
                      className={`mc-eval-badge mc-eval-badge--${
                        row.action === 'complete'
                          ? 'pass'
                          : row.action === 'disabled' || row.action === 'skipped'
                            ? 'skip'
                            : 'neutral'
                      }`}
                    >
                      {row.action}
                    </span>
                  </Flex>
                  {sourceLabel ? (
                    <Text size="1" color="gray" className="mt-1 block">
                      {sourceLabel}
                      {row.durationMs != null ? ` · ${row.durationMs}ms` : ''}
                    </Text>
                  ) : row.durationMs != null ? (
                    <Text size="1" color="gray" className="mt-1 block">
                      {row.durationMs}ms
                    </Text>
                  ) : null}
                  {row.reason ? (
                    <div className="mt-2">
                      <Text size="1" weight="medium" className="mb-1 block">
                        Rationale
                      </Text>
                      <Text size="2" className="whitespace-pre-wrap">
                        {row.reason}
                      </Text>
                    </div>
                  ) : null}
                  {row.providerLabel && row.providerLabel !== 'heuristic' ? (
                    <Text size="1" color="gray" className="mt-1 block">
                      {row.providerLabel}
                    </Text>
                  ) : null}
                </div>
              )
            })}
          </Flex>
        )}
      </div>

      <div>
        <Text size="2" weight="medium" className="mb-2 block">
          Pre-delivery quality (PDQE)
        </Text>
        {pdqe.length === 0 ? (
          <Text size="2" color="gray">
            No PDQE steps recorded for this run (evaluator disabled, skipped, or run saved before this feature).
          </Text>
        ) : (
          <Flex direction="column" gap="2">
            {pdqe.map((step, i) => {
              const kind = pdqeStepBadgeKind(step.step, step.eval)
              const title = pdqeStepLabel(step.step)
              return (
                <div
                  key={`pdqe-${step.step}-${i}`}
                  className="mc-eval-row rounded-md border border-[color:var(--mc-border-soft)] p-2"
                >
                  <Flex justify="between" align="start" gap="2" wrap="wrap">
                    <Text size="2" weight="medium">
                      {title}
                    </Text>
                    <span className={`mc-eval-badge mc-eval-badge--${kind}`}>
                      {step.eval?.verdict ?? kind}
                    </span>
                  </Flex>
                  {step.at ? (
                    <Text size="1" color="gray" className="mt-1 block">
                      {step.at}
                    </Text>
                  ) : null}
                  {step.providerLabel ? (
                    <Text size="1" color="gray" className="mt-1 block">
                      {step.providerLabel}
                    </Text>
                  ) : null}
                  {step.eval ? (
                    <EvalDetailBlock evalDetail={step.eval} />
                  ) : step.detail ? (
                    <Text size="2" className="mt-2 block whitespace-pre-wrap">
                      {step.detail}
                    </Text>
                  ) : null}
                </div>
              )
            })}
          </Flex>
        )}
      </div>
    </Flex>
  )
}

function AgentHistoryTierBadge({ tier }: { tier: TierRouteInfo }) {
  return (
    <span
      className={`inline-flex max-w-full items-center rounded-md border px-2 py-0.5 font-mono text-[11px] leading-snug ${tierBadgeClass(tier.tier)}`}
      title={`${tier.provider} · ${tier.endpoint}`}
    >
      {formatTierBadgeLabel(tier)}
    </span>
  )
}

export function AppDialogs({
  appearance,
  api,
  chatId,
  activePersonaId,
  personas,
  settings,
  queue,
  schedules,
  inbox,
  scheduleDetail,
  agentsMd,
  artifacts,
  memory,
  agentHistory,
  terminal,
}: AppDialogsProps) {
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

  const queueDialogOpen = queue.open
  const setQueueDialogOpen = queue.onOpenChange
  const queueShowAllPersonas = queue.showAllPersonas
  const setQueueShowAllPersonas = queue.setShowAllPersonas
  const queueDialogItems = queue.items
  const stoppingRunIds = queue.stoppingRunIds
  const handleQueueAction = queue.handleQueueAction
  const backgroundJobsVisible = queue.backgroundJobs
  const stoppingBackgroundJobIds = queue.stoppingBackgroundJobIds
  const isActiveBackgroundJobStatus = queue.isActiveBackgroundJobStatus
  const handleBackgroundJobStop = queue.handleBackgroundJobStop

  const schedulesDialogOpen = schedules.open
  const setSchedulesDialogOpen = schedules.onOpenChange
  const schedulesShowArchived = schedules.showArchived
  const setSchedulesShowArchived = schedules.setShowArchived
  const schedulesFiltered = schedules.filtered
  const schedulesList = schedules.schedules
  const newSchedulePrompt = schedules.newPrompt
  const setNewSchedulePrompt = schedules.setNewPrompt
  const newScheduleType = schedules.newType
  const setNewScheduleType = schedules.setNewType
  const newScheduleValue = schedules.newValue
  const setNewScheduleValue = schedules.setNewValue
  const newSchedulePersonaId = schedules.newPersonaId
  const setNewSchedulePersonaId = schedules.setNewPersonaId
  const createSchedule = schedules.createSchedule
  const updateSchedule = schedules.updateSchedule

  const scheduleDetailTask = scheduleDetail.task
  const scheduleDetailPrompt = scheduleDetail.prompt
  const setScheduleDetailPrompt = scheduleDetail.setPrompt
  const scheduleDetailScheduleType = scheduleDetail.scheduleType
  const setScheduleDetailScheduleType = scheduleDetail.setScheduleType
  const scheduleDetailScheduleValue = scheduleDetail.scheduleValue
  const setScheduleDetailScheduleValue = scheduleDetail.setScheduleValue
  const scheduleDetailBusy = scheduleDetail.busy
  const setScheduleDetailBusy = scheduleDetail.setBusy
  const updateScheduleDetail = scheduleDetail.updateSchedule

  const agentsMdOpen = agentsMd.open
  const setAgentsMdOpen = agentsMd.onOpenChange
  const agentsMdPath = agentsMd.path
  const agentsMdError = agentsMd.error
  const setAgentsMdError = agentsMd.setError
  const setAgentsMdBusy = agentsMd.setBusy
  const agentsMdContent = agentsMd.content
  const setAgentsMdContent = agentsMd.setContent
  const agentsMdMtimeMs = agentsMd.mtimeMs
  const agentsMdBusy = agentsMd.busy
  const loadWorkspaceAgentsMd = agentsMd.load
  const saveWorkspaceAgentsMd = agentsMd.save

  const artifactsDialogOpen = artifacts.open
  const setArtifactsDialogOpen = artifacts.onOpenChange
  const setArtifactsError = artifacts.setError
  const setArtifactTextError = artifacts.setTextError
  const artifactKindFilter = artifacts.kindFilter
  const setArtifactKindFilter = artifacts.setKindFilter
  const loadArtifacts = artifacts.load
  const artifactsBusy = artifacts.busy
  const artifactsError = artifacts.error
  const artifactsList = artifacts.items
  const selectedArtifactId = artifacts.selectedId
  const setSelectedArtifactId = artifacts.setSelectedId
  const selectedArtifact = artifacts.selected
  const artifactTextPreview = artifacts.textPreview
  const artifactTextBusy = artifacts.textBusy
  const artifactTextError = artifacts.textError

  const memoryDialogOpen = memory.open
  const setMemoryDialogOpen = memory.onOpenChange
  const setMemoryError = memory.setError
  const setMemoryBusy = memory.setBusy
  const memoryPathHint = memory.pathHint
  const memoryError = memory.error
  const memoryContent = memory.content
  const setMemoryContent = memory.setContent
  const memoryMtimeMs = memory.mtimeMs
  const memoryBusy = memory.busy
  const loadPersonaMemory = memory.load
  const savePersonaMemory = memory.save

  const agentHistoryDialogOpen = agentHistory.open
  const setAgentHistoryDialogOpen = agentHistory.onOpenChange
  const setAgentHistoryTab = agentHistory.setTab
  const setAgentHistoryError = agentHistory.setError
  const setAgentHistoryBusy = agentHistory.setBusy
  const agentHistoryPathHint = agentHistory.pathHint
  const agentHistoryFilename = agentHistory.filename
  const agentHistoryMtimeMs = agentHistory.mtimeMs
  const agentHistoryBusy = agentHistory.busy
  const agentHistoryError = agentHistory.error
  const agentHistoryParsed = agentHistory.parsed
  const agentHistoryTab = agentHistory.tab
  const agentHistoryIterationIdx = agentHistory.iterationIdx
  const setAgentHistoryIterationIdx = agentHistory.setIterationIdx
  const agentHistoryRaw = agentHistory.raw
  const agentHistoryOptimizeBusy = agentHistory.optimizeBusy
  const agentHistoryOptimizeNotes = agentHistory.optimizeNotes
  const setAgentHistoryOptimizeNotes = agentHistory.setOptimizeNotes

  const terminalDialogOpen = terminal.open
  const setTerminalDialogOpen = terminal.onOpenChange
  const terminalError = terminal.error
  const setTerminalError = terminal.setError

  const loadAgentHistoryLatest = agentHistory.load
  const optimizeAgentHistoryLatest = agentHistory.optimize

  const openScheduleDetail = schedules.openDetail

  return (
    <>
<Dialog.Root open={settingsDialogOpen} onOpenChange={setSettingsDialogOpen}>
{settingsDialogOpen ? (
<Dialog.Content style={{ maxWidth: 920 }}>
    <Dialog.Title>Web UI configuration</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Put LLM API keys in repo-root <code className="text-xs">.env</code> (e.g.{' '}
      <code className="text-xs">ANTHROPIC_API_KEY</code>, <code className="text-xs">OPENAI_API_KEY</code>).
      Configure Telegram, Discord, and WhatsApp under the Integrations tab (saved in the app database).
      Restart the gateway after changing API keys or channel tokens.
    </Dialog.Description>
    {settingsError ? (
      <Callout.Root color="red" size="1" variant="soft" className="mb-2">
        <Callout.Text>{settingsError}</Callout.Text>
      </Callout.Root>
    ) : null}
    {restartNotice ? (
      <Callout.Root color="green" size="1" variant="soft" className="mb-2">
        <Callout.Text>{restartNotice}</Callout.Text>
      </Callout.Root>
    ) : null}
    <Tabs.Root defaultValue="overview">
      <Tabs.List size="1" className="mb-3 flex-wrap mc-settings-tabs-sticky">
        <Tabs.Trigger value="overview">Overview</Tabs.Trigger>
        <Tabs.Trigger value="llm">LLM</Tabs.Trigger>
        <Tabs.Trigger value="local-delegate">Local delegate</Tabs.Trigger>
        <Tabs.Trigger value="cursor">Cursor</Tabs.Trigger>
        <Tabs.Trigger value="deterministic">Deterministic</Tabs.Trigger>
        <Tabs.Trigger value="hooks-skills">Hooks & Skills</Tabs.Trigger>
        <Tabs.Trigger value="integrations">Integrations</Tabs.Trigger>
        <Tabs.Trigger value="channels">Channels</Tabs.Trigger>
      </Tabs.List>
      <React.Suspense fallback={<SettingsPanelSkeleton />}>
      <Tabs.Content value="overview">
        {installationStatus ? (
          <Flex direction="column" gap="2" mb="2">
            <Flex gap="2" wrap="wrap" align="center">
              <Text size="1" color={installationStatus.llm_ready ? 'green' : 'orange'}>
                LLM: {installationStatus.llm_ready ? 'ready' : 'missing'}
              </Text>
              <Text size="1" color={installationStatus.channel_ready ? 'green' : 'orange'}>
                Channels: {installationStatus.channel_ready ? 'ready' : 'missing'}
              </Text>
              <Text
                size="1"
                color={installationStatus.cursor_engine_ready ? 'green' : 'orange'}
              >
                Cursor engine:{' '}
                {installationStatus.cursor_engine_ready ? 'ready' : 'not ready'}
              </Text>
              {installationStatus.agent_engine === 'classic_cost_routing' ? (
                <Text
                  size="1"
                  color={installationStatus.local_delegate_ready ? 'green' : 'orange'}
                >
                  Local delegate:{' '}
                  {installationStatus.local_delegate_ready ? 'verified' : 'not verified'}
                </Text>
              ) : null}
              <Text size="1" color="gray">
                Env restart needed:{' '}
                {(installationStatus.requires_restart_for_env_changes ??
                  installationStatus.requires_restart_to_apply_runtime_settings) === true
                  ? 'yes'
                  : 'no'}
              </Text>
            </Flex>
            <Text size="1" color="gray">
              Stop requests between LLM/tool iterations (cooperative cancel). Use Queue for FIFO visibility.
            </Text>
            <div>
              <Button
                size="1"
                variant="solid"
                disabled={restartBusy}
                onClick={() => void requestRestart()}
              >
                {restartBusy ? 'Restarting…' : 'Restart gateway'}
              </Button>
            </div>
          </Flex>
        ) : (
          <OverviewStatusSkeleton />
        )}
        <div
          className="rounded-md border p-3 mt-3"
          style={appearance === 'dark'
            ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
            : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
        >
          <Text size="2" weight="bold" className="mb-2 block">
            Runtime toggles
          </Text>
          <SettingsRuntimePanel api={api} onError={setSettingsError} />
        </div>
      </Tabs.Content>
      <Tabs.Content value="llm">
        <SettingsLlmPanel
          api={api}
          onError={setSettingsError}
        />
      </Tabs.Content>
      <Tabs.Content value="local-delegate">
        <SettingsLocalDelegatePanel api={api} onError={setSettingsError} />
      </Tabs.Content>
      <Tabs.Content value="cursor">
        <SettingsCursorPanel api={api} onError={setSettingsError} />
      </Tabs.Content>
      <Tabs.Content value="deterministic">
        <SettingsDeterministicPipelinePanel api={api} onError={setSettingsError} />
      </Tabs.Content>
      <Tabs.Content value="hooks-skills">
        <SettingsHooksSkillsPanel
          api={api}
          onError={setSettingsError}
          activePersonaId={activePersonaId}
        />
      </Tabs.Content>
      <Tabs.Content value="integrations">
        <SettingsIntegrationsPanel
          api={api}
          appearance={appearance}
          onError={setSettingsError}
          onSaved={() => void reloadInstallationStatus()}
          requestRestart={requestRestart}
          restartBusy={restartBusy}
        />
      </Tabs.Content>
      <Tabs.Content value="channels">
    <div
      className="rounded-md border p-3"
      style={appearance === 'dark'
        ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' }
        : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}
    >
      <Text size="2" weight="bold">External channel persona mode</Text>
      <Text size="1" color="gray" className="mb-2 block">
        Bot integrations appear here for this contact. Telegram and Discord can use all personas or a single persona. WhatsApp is single-persona by design; Web chat uses the persona selector in the main UI.
      </Text>
      <div className="space-y-2">
        {bindings.length === 0 ? (
          <Text size="1" color="gray">No bot integrations configured. Add bots under the Integrations tab.</Text>
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
            <Flex key={`${b.bot_instance_id}:${b.channel_type}:${b.channel_handle ?? 'pending'}`} gap="2" align="center" wrap="wrap">
              <Text size="1" className="min-w-[220px]">
                {b.label ? `${b.label} · ` : ''}{platform} (bot #{b.bot_instance_id}): {handleLabel}
              </Text>
              <Select.Root
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
                <Select.Trigger className="w-[140px]" />
                <Select.Content>
                  {platform === 'whatsapp' ? null : (
                    <Select.Item value="all" disabled={allPersonasDisabled}>
                      All personas
                    </Select.Item>
                  )}
                  {platform === 'whatsapp' ? null : (
                    <Select.Item value="single">Single persona</Select.Item>
                  )}
                  {platform === 'whatsapp' ? (
                    <Select.Item value="single">Single persona</Select.Item>
                  ) : null}
                </Select.Content>
              </Select.Root>
              {currentMode === 'single' ? (
                <Select.Root
                  value={currentPersonaId != null ? String(currentPersonaId) : ''}
                  onValueChange={(value) => {
                    const pid = Number(value)
                    if (Number.isFinite(pid) && pid > 0) {
                      void updateChannelPersonaPolicy(b.bot_instance_id, 'single', pid)
                    }
                  }}
                >
                  <Select.Trigger className="w-[180px]" placeholder="Persona" />
                  <Select.Content>
                    {personas.map((p) => (
                      <Select.Item key={p.id} value={String(p.id)}>
                        {p.name}
                      </Select.Item>
                    ))}
                  </Select.Content>
                </Select.Root>
              ) : null}
            </Flex>
          )
        })}
      </div>
    </div>
      </Tabs.Content>
      </React.Suspense>
    </Tabs.Root>
    <Flex justify="end" mt="4">
      <Dialog.Close>
        <Button variant="soft">Close</Button>
      </Dialog.Close>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>
<Dialog.Root open={queueDialogOpen} onOpenChange={setQueueDialogOpen}>
{queueDialogOpen ? (
<Dialog.Content style={{ maxWidth: 920 }}>
    <Dialog.Title>Run queue</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Pending and running agent work (FIFO per persona). Queued items can be removed immediately; running items can be stopped.
    </Dialog.Description>
    <Flex align="center" gap="3" mb="2">
      <label className="flex cursor-pointer items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={queueShowAllPersonas}
          onChange={(e) => setQueueShowAllPersonas(e.target.checked)}
        />
        All personas
      </label>
    </Flex>
    <div className="max-h-[min(420px,60vh)] overflow-auto rounded-md border p-2" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}>
      {queueDialogItems.length === 0 ? (
        <Flex direction="column" gap="2">
          <Text size="2" color="gray">No queued runs (lane idle or diagnostics loading).</Text>
          <Button
            size="1"
            variant="soft"
            className="cursor-pointer self-start"
            onClick={() => {
              setQueueDialogOpen(false)
              queue.onOpenSchedules()
            }}
          >
            Open schedules
          </Button>
        </Flex>
      ) : (
        <>
          <table className="hidden w-full border-collapse text-left text-sm md:table">
            <thead>
              <tr className={appearance === 'dark' ? 'text-[color:var(--mc-text-muted)]' : 'text-[color:var(--mc-text-muted)]'}>
                <th className="p-1 pr-2">#</th>
                <th className="p-1 pr-2">State</th>
                <th className="p-1 pr-2">Persona</th>
                <th className="p-1 pr-2">Source</th>
                <th className="p-1 min-w-[120px]">Context</th>
                <th className="p-1 pr-2">Project</th>
                <th className="p-1 pr-2">Workflow</th>
                <th className="p-1 text-right"> </th>
              </tr>
            </thead>
            <tbody>
              {queueDialogItems.map((it) => {
                const isStopping = stoppingRunIds.includes(it.run_id)
                const isRunning = it.state === 'running'
                return (
                  <tr key={it.run_id} className="border-t border-[color:var(--gray-6)] align-top">
                    <td className="p-1 pr-2 font-mono text-xs">{it.position}</td>
                    <td className="p-1 pr-2">{it.state}</td>
                    <td className="p-1 pr-2">{it.persona_name}</td>
                    <td className="p-1 pr-2">{it.source}</td>
                    <td className="p-1 max-w-[280px] break-words" title={it.label}>{it.label || '-'}</td>
                    <td className="p-1 pr-2 font-mono text-xs">{it.project_id ?? '-'}</td>
                    <td className="p-1 pr-2 font-mono text-xs">{it.workflow_id ?? '-'}</td>
                    <td className="p-1 text-right">
                      <Button
                        size="1"
                        variant="soft"
                        color="red"
                        disabled={isStopping}
                        onClick={() => void handleQueueAction(it.run_id, it.state)}
                      >
                        {isStopping
                          ? (isRunning ? 'Stopping...' : 'Removing...')
                          : (isRunning ? 'Stop' : 'Remove')}
                      </Button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          <div className="flex flex-col gap-2 md:hidden">
            {queueDialogItems.map((it) => {
              const isStopping = stoppingRunIds.includes(it.run_id)
              const isRunning = it.state === 'running'
              return (
                <div
                  key={it.run_id}
                  className={
                    appearance === 'dark'
                      ? 'rounded-lg border border-[color:var(--mc-border-soft)] p-3 text-sm'
                      : 'rounded-lg border border-[color:var(--mc-border-soft)] p-3 text-sm'
                  }
                >
                  <Flex justify="between" align="start" gap="2" mb="2">
                    <Text size="2" weight="bold">
                      #{it.position} · {it.state}
                    </Text>
                    <Button
                      size="1"
                      variant="soft"
                      color="red"
                      disabled={isStopping}
                      onClick={() => void handleQueueAction(it.run_id, it.state)}
                    >
                      {isStopping
                        ? (isRunning ? 'Stopping...' : 'Removing...')
                        : (isRunning ? 'Stop' : 'Remove')}
                    </Button>
                  </Flex>
                  <Text size="1" color="gray" className="mb-1 block">
                    {it.persona_name} · {it.source}
                  </Text>
                  <Text size="1" className="break-words">
                    {it.label || '-'}
                  </Text>
                  <Text size="1" color="gray" className="mt-1 block font-mono">
                    project {it.project_id ?? '-'} · workflow {it.workflow_id ?? '-'}
                  </Text>
                </div>
              )
            })}
          </div>
        </>
      )}
    </div>
    <Text size="2" mt="3" mb="1" weight="medium">
      Background jobs
    </Text>
    <Text size="1" color="gray" mb="2">
      Recent background jobs for this chat. Stop requests cooperative cancellation.
    </Text>
    <div className="max-h-[min(320px,45vh)] overflow-auto rounded-md border p-2" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}>
      {backgroundJobsVisible.length === 0 ? (
        <Text size="2" color="gray">No background jobs found for this chat.</Text>
      ) : (
        <>
          <table className="hidden w-full border-collapse text-left text-sm md:table">
            <thead>
              <tr className={appearance === 'dark' ? 'text-[color:var(--mc-text-muted)]' : 'text-[color:var(--mc-text-muted)]'}>
                <th className="p-1 pr-2">Status</th>
                <th className="p-1 pr-2">Kind</th>
                <th className="p-1 pr-2">ID</th>
                <th className="p-1 pr-2">Label</th>
                <th className="p-1 pr-2">Updated</th>
                <th className="p-1 text-right"> </th>
              </tr>
            </thead>
            <tbody>
              {backgroundJobsVisible.map((job) => {
                const isActive = isActiveBackgroundJobStatus(job.status)
                const isStopping = stoppingBackgroundJobIds.includes(job.id)
                const updatedAt = job.finished_at || job.started_at || job.created_at
                return (
                  <tr key={job.id} className="border-t border-[color:var(--gray-6)] align-top">
                    <td className="p-1 pr-2">{job.status}</td>
                    <td className="p-1 pr-2 text-xs">
                      {job.job_kind === 'shell'
                        ? 'shell'
                        : job.job_kind === 'run_optimize'
                          ? 'optimize'
                          : 'agent'}
                    </td>
                    <td className="p-1 pr-2 font-mono text-xs">{job.id}</td>
                    <td className="p-1 pr-2 max-w-[260px] break-words" title={job.label || job.prompt}>{job.label || job.prompt || '-'}</td>
                    <td className="p-1 pr-2 text-xs">{updatedAt ? new Date(updatedAt).toLocaleString() : '-'}</td>
                    <td className="p-1 text-right">
                      {isActive ? (
                        <Button
                          size="1"
                          variant="soft"
                          color="red"
                          disabled={isStopping}
                          onClick={() => void handleBackgroundJobStop(job.id)}
                        >
                          {isStopping ? 'Stopping...' : 'Stop'}
                        </Button>
                      ) : (
                        <Text size="1" color="gray">'</Text>
                      )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          <div className="flex flex-col gap-2 md:hidden">
            {backgroundJobsVisible.map((job) => {
              const isActive = isActiveBackgroundJobStatus(job.status)
              const isStopping = stoppingBackgroundJobIds.includes(job.id)
              const updatedAt = job.finished_at || job.started_at || job.created_at
              return (
                <div
                  key={job.id}
                  className={
                    appearance === 'dark'
                      ? 'rounded-lg border border-[color:var(--mc-border-soft)] p-3 text-sm'
                      : 'rounded-lg border border-[color:var(--mc-border-soft)] p-3 text-sm'
                  }
                >
                  <Flex justify="between" align="start" gap="2" mb="2">
                    <Text size="2" weight="bold">
                      {job.status} ·{' '}
                      {job.job_kind === 'shell'
                        ? 'shell'
                        : job.job_kind === 'run_optimize'
                          ? 'optimize'
                          : 'agent'}
                    </Text>
                    {isActive ? (
                      <Button
                        size="1"
                        variant="soft"
                        color="red"
                        disabled={isStopping}
                        onClick={() => void handleBackgroundJobStop(job.id)}
                      >
                        {isStopping ? 'Stopping...' : 'Stop'}
                      </Button>
                    ) : null}
                  </Flex>
                  <Text size="1" color="gray" className="mb-1 block font-mono">
                    {job.id}
                  </Text>
                  <Text size="1" className="break-words">
                    {job.prompt || '-'}
                  </Text>
                  <Text size="1" color="gray" className="mt-1 block">
                    {updatedAt ? new Date(updatedAt).toLocaleString() : '-'}
                  </Text>
                </div>
              )
            })}
          </div>
        </>
      )}
    </div>
    <Flex justify="end" mt="3">
      <Dialog.Close>
        <Button variant="soft">Close</Button>
      </Dialog.Close>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>

<InboxPanel
  appearance={appearance}
  open={inbox.open}
  onOpenChange={inbox.onOpenChange}
  unread={inbox.unread}
  todos={inbox.todos}
  personas={personas}
  loading={inbox.loading}
  busyTodoId={inbox.busyTodoId}
  onRefresh={inbox.onRefresh}
  onOpenTarget={inbox.onOpenTarget}
  onCompleteTodo={inbox.onCompleteTodo}
/>

<Dialog.Root
  open={schedulesDialogOpen}
  onOpenChange={(open) => setSchedulesDialogOpen(open)}
>
{schedulesDialogOpen ? (
<Dialog.Content style={{ maxWidth: 820 }}>
    <Dialog.Title>Schedules</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Create and manage scheduled prompts for this chat.
    </Dialog.Description>

    <Flex align="center" justify="between" gap="3" mb="3" wrap="wrap">
      <Text size="2" weight="medium">
        Active schedules
      </Text>
      <label htmlFor="sched-archived" className="flex cursor-pointer items-center gap-2">
        <Text size="1" color="gray">
          Show completed / cancelled
        </Text>
        <Switch
          id="sched-archived"
          checked={schedulesShowArchived}
          onCheckedChange={setSchedulesShowArchived}
        />
      </label>
    </Flex>

    <div className="rounded-lg border p-3" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-panel)' } : { borderColor: 'var(--gray-6)', background: 'var(--gray-2)' }}>
      <ul className="mb-3 list-none space-y-3">
        {schedulesFiltered.length === 0 ? (
          <li
            className="rounded-lg border border-dashed px-4 py-10 text-center"
            style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}
          >
            <Text size="2" color="gray">
              {schedulesList.length === 0
                ? 'No schedules yet. Add one below.'
                : 'No active schedules. Enable “Show completed / cancelled” to see finished runs.'}
            </Text>
          </li>
        ) : null}
        {schedulesFiltered.map((t) => (
          <li key={t.id} className="flex flex-wrap items-center gap-2 rounded-lg border p-2" style={appearance === 'dark' ? { borderColor: 'var(--mc-border-soft)' } : { borderColor: 'var(--gray-6)' }}>
            <span className="min-w-0 flex-1 truncate" title={t.prompt}>{t.prompt}</span>
            <Select.Root
              value={String(t.persona_id)}
              onValueChange={(v) => void updateSchedule(t.id, { persona_id: Number(v) })}
            >
              <Select.Trigger className="w-[120px]" />
              <Select.Content>
                {personas.map((p) => (
                  <Select.Item key={p.id} value={String(p.id)}>
                    {p.name}
                  </Select.Item>
                ))}
              </Select.Content>
            </Select.Root>
            <Text size="1" color="gray">{t.schedule_type} · {t.next_run ?? '-'}</Text>
            <Text size="1" color={
              t.status === 'active' || t.status === 'running' ? 'green' :
                t.status === 'paused' ? 'orange' :
                  t.status === 'completed' ? 'gray' :
                    t.status === 'cancelled' ? 'red' : 'gray'
            }>
              {t.status === 'running' ? 'active' : t.status}
            </Text>
            <Button
              size="1"
              variant="soft"
              onClick={() => openScheduleDetail(t)}
            >
              Details
            </Button>
            {t.status === 'active' ? (
              <Button size="1" variant="soft" onClick={() => void updateSchedule(t.id, { status: 'paused' })}>Pause</Button>
            ) : t.status === 'paused' ? (
              <Button size="1" variant="soft" onClick={() => void updateSchedule(t.id, { status: 'active' })}>Resume</Button>
            ) : null}
            {t.status !== 'cancelled' ? (
              <Button size="1" variant="soft" color="red" onClick={() => void updateSchedule(t.id, { status: 'cancelled' })}>Cancel</Button>
            ) : null}
          </li>
        ))}
      </ul>

      <Flex gap="2" align="end" wrap="wrap">
        <TextField.Root
          placeholder="Prompt"
          value={newSchedulePrompt}
          onChange={(e) => setNewSchedulePrompt(e.target.value)}
          className="min-w-[220px]"
        />
        <Select.Root value={newScheduleType} onValueChange={(v) => setNewScheduleType(v as 'cron' | 'once')}>
          <Select.Trigger className="w-[100px]" />
          <Select.Content>
            <Select.Item value="cron">Cron</Select.Item>
            <Select.Item value="once">Once</Select.Item>
          </Select.Content>
        </Select.Root>
        <TextField.Root
          placeholder={newScheduleType === 'cron' ? '0 9 * * *' : '2025-12-31T09:00:00Z'}
          value={newScheduleValue}
          onChange={(e) => setNewScheduleValue(e.target.value)}
          className="min-w-[200px]"
        />
        <Select.Root
          value={newSchedulePersonaId != null ? String(newSchedulePersonaId) : ''}
          onValueChange={(v) => setNewSchedulePersonaId(Number(v))}
        >
          <Select.Trigger className="w-[140px]" placeholder="Persona" />
          <Select.Content>
            {personas.map((p) => (
              <Select.Item key={p.id} value={String(p.id)}>
                {p.name}
              </Select.Item>
            ))}
          </Select.Content>
        </Select.Root>
        <Button
          size="1"
          onClick={() => {
            if (newSchedulePrompt.trim()) {
              void createSchedule(
                newSchedulePrompt.trim(),
                newScheduleType,
                newScheduleValue,
                newSchedulePersonaId ?? activePersonaId,
              )
              setNewSchedulePrompt('')
            }
          }}
        >
          Add
        </Button>
      </Flex>
    </div>

    <Flex justify="end" mt="4" gap="2">
      <Dialog.Close>
        <Button variant="soft">Close</Button>
      </Dialog.Close>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>

<Dialog.Root
  open={scheduleDetailTask != null}
  onOpenChange={scheduleDetail.onOpenChange}
>
{scheduleDetailTask != null ? (
<Dialog.Content style={{ maxWidth: 720 }}>
    <Dialog.Title>
      {scheduleDetailTask != null ? `Schedule #${scheduleDetailTask.id}` : 'Schedule'}
    </Dialog.Title>
    <Dialog.Description size="2" mb="3">
      View metadata, edit the prompt, or change the cron/once expression (server runs the same preflight as new schedules).
    </Dialog.Description>
    {scheduleDetailTask != null ? (
      <>
        <div className="mb-3 grid grid-cols-[120px_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm">
          <Text size="2" color="gray" className="block">Persona</Text>
          <Text size="2" className="block">
            {personas.find((p) => p.id === scheduleDetailTask.persona_id)?.name ?? scheduleDetailTask.persona_id}
          </Text>
          <Text size="2" color="gray" className="block">Type</Text>
          <Text size="2" className="block">{scheduleDetailTask.schedule_type}</Text>
          <Text size="2" color="gray" className="block">Schedule</Text>
          <Text size="2" className="block break-all">{scheduleDetailTask.schedule_value}</Text>
          <Text size="2" color="gray" className="block">Next run</Text>
          <Text size="2" className="block break-all">{scheduleDetailTask.next_run ?? '-'}</Text>
          <Text size="2" color="gray" className="block">Last run</Text>
          <Text size="2" className="block break-all">{scheduleDetailTask.last_run ?? '-'}</Text>
          <Text size="2" color="gray" className="block">Status</Text>
          <Text size="2" className="block">{scheduleDetailTask.status}</Text>
          <Text size="2" color="gray" className="block">Created</Text>
          <Text size="2" className="block break-all">{scheduleDetailTask.created_at ?? '-'}</Text>
        </div>
        <Text size="2" weight="bold" mb="1">Prompt</Text>
        <textarea
          value={scheduleDetailPrompt}
          onChange={(e) => setScheduleDetailPrompt(e.target.value)}
          spellCheck={false}
          disabled={scheduleDetailTask.status === 'cancelled'}
          className={appearance === 'dark'
            ? 'min-h-[160px] w-full rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3 font-mono text-xs text-[color:var(--mc-text-primary)]'
            : 'min-h-[160px] w-full rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] p-3 font-mono text-xs text-[color:var(--mc-text-primary)]'}
        />
        <Text size="2" weight="bold" mb="1" mt="3">Schedule</Text>
        <Flex gap="2" align="center" wrap="wrap" mb="2">
          <Select.Root
            value={scheduleDetailScheduleType}
            onValueChange={(v) => setScheduleDetailScheduleType(v as 'cron' | 'once')}
            disabled={scheduleDetailTask.status === 'cancelled'}
          >
            <Select.Trigger className="w-[100px]" />
            <Select.Content>
              <Select.Item value="cron">Cron</Select.Item>
              <Select.Item value="once">Once</Select.Item>
            </Select.Content>
          </Select.Root>
          <input
            type="text"
            value={scheduleDetailScheduleValue}
            onChange={(e) => setScheduleDetailScheduleValue(e.target.value)}
            spellCheck={false}
            disabled={scheduleDetailTask.status === 'cancelled'}
            placeholder={scheduleDetailScheduleType === 'cron' ? '0 9 * * * *' : '2099-12-31T23:59:59+00:00'}
            className={appearance === 'dark'
              ? 'min-w-[200px] flex-1 rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] px-2 py-1 font-mono text-xs text-[color:var(--mc-text-primary)]'
              : 'min-w-[200px] flex-1 rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] px-2 py-1 font-mono text-xs text-[color:var(--mc-text-primary)]'}
          />
        </Flex>
        <Flex justify="end" gap="2" mt="3" wrap="wrap">
          <Dialog.Close>
            <Button variant="soft" size="1">Close</Button>
          </Dialog.Close>
          <Button
            size="1"
            disabled={
              scheduleDetailBusy
              || scheduleDetailTask.status === 'cancelled'
              || (
                scheduleDetailScheduleType === (scheduleDetailTask.schedule_type === 'once' ? 'once' : 'cron')
                && scheduleDetailScheduleValue.trim() === scheduleDetailTask.schedule_value.trim()
              )
              || scheduleDetailScheduleValue.trim().length === 0
            }
            onClick={() => {
              if (scheduleDetailTask == null) return
              setScheduleDetailBusy(true)
              updateScheduleDetail(scheduleDetailTask.id, {
                schedule_type: scheduleDetailScheduleType,
                schedule_value: scheduleDetailScheduleValue.trim(),
              })
                .then(() => scheduleDetail.close())
                .catch(() => { /* api throws */ })
                .finally(() => setScheduleDetailBusy(false))
            }}
          >
            {scheduleDetailBusy ? 'Saving…' : 'Save schedule'}
          </Button>
          <Button
            size="1"
            disabled={
              scheduleDetailBusy
              || scheduleDetailTask.status === 'cancelled'
              || scheduleDetailPrompt.trim() === scheduleDetailTask.prompt.trim()
              || scheduleDetailPrompt.trim().length === 0
            }
            onClick={() => {
              if (scheduleDetailTask == null) return
              setScheduleDetailBusy(true)
              updateScheduleDetail(scheduleDetailTask.id, { prompt: scheduleDetailPrompt.trim() })
                .then(() => scheduleDetail.close())
                .catch(() => { /* api throws */ })
                .finally(() => setScheduleDetailBusy(false))
            }}
          >
            {scheduleDetailBusy ? 'Saving…' : 'Save prompt'}
          </Button>
        </Flex>
      </>
    ) : null}
  </Dialog.Content>
) : null}
</Dialog.Root>

<Dialog.Root
  open={agentsMdOpen}
  onOpenChange={agentsMd.onOpenChange}
>
{agentsMdOpen ? (
<Dialog.Content style={{ maxWidth: 900 }}>
    <Dialog.Title>Workspace principles (AGENTS.md)</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Shared agent principles for this workspace. Same file the bot loads from your configured workspace path.
    </Dialog.Description>
    {agentsMdPath ? (
      <Text size="1" color="gray" className="mb-2 block break-all">
        {agentsMdPath}
      </Text>
    ) : null}
    {agentsMdError ? (
      <Callout.Root color="red" size="1" variant="soft" className="mb-2">
        <Callout.Text>{agentsMdError}</Callout.Text>
      </Callout.Root>
    ) : null}
    <textarea
      value={agentsMdContent}
      onChange={(e) => setAgentsMdContent(e.target.value)}
      spellCheck={false}
      className={appearance === 'dark'
        ? 'h-[420px] w-full rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3 font-mono text-xs text-[color:var(--mc-text-primary)]'
        : 'h-[420px] w-full rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] p-3 font-mono text-xs text-[color:var(--mc-text-primary)]'}
    />
    <Flex justify="between" align="center" mt="3" wrap="wrap" gap="2">
      <Text size="1" color="gray">
        {agentsMdMtimeMs != null ? `mtime: ${agentsMdMtimeMs}` : ''}
      </Text>
      <Flex gap="2">
        <Button size="1" variant="soft" onClick={() => void loadWorkspaceAgentsMd()} disabled={agentsMdBusy}>
          Reload
        </Button>
        <Button size="1" onClick={() => void saveWorkspaceAgentsMd()} disabled={agentsMdBusy}>
          {agentsMdBusy ? 'Saving…' : 'Save'}
        </Button>
        <Dialog.Close>
          <Button size="1" variant="soft">Close</Button>
        </Dialog.Close>
      </Flex>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>

<Dialog.Root
  open={artifactsDialogOpen}
  onOpenChange={artifacts.onOpenChange}
>
{artifactsDialogOpen ? (
<Dialog.Content style={{ maxWidth: 980 }}>
    <Dialog.Title>Artifacts</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      View files produced or referenced in this chat persona. Attachments stay channel-local; web can preview them here.
    </Dialog.Description>
    <Flex gap="3" align="start" wrap="wrap" className="flex-col md:flex-row">
      <div className="min-w-0 w-full flex-1 md:min-w-[250px]">
        <Flex justify="between" align="center" mb="2" gap="2" wrap="wrap">
          <Select.Root value={artifactKindFilter} onValueChange={setArtifactKindFilter}>
            <Select.Trigger className="w-[150px]" />
            <Select.Content>
              <Select.Item value="all">All kinds</Select.Item>
              <Select.Item value="image">Images</Select.Item>
              <Select.Item value="markdown">Markdown</Select.Item>
              <Select.Item value="html">HTML</Select.Item>
              <Select.Item value="text">Text</Select.Item>
              <Select.Item value="other">Other</Select.Item>
            </Select.Content>
          </Select.Root>
          <Button size="1" variant="soft" onClick={() => void loadArtifacts(chatId, activePersonaId)} disabled={artifactsBusy}>
            Refresh
          </Button>
        </Flex>
        <div className={appearance === 'dark'
          ? 'max-h-[min(440px,65vh)] overflow-auto rounded-md border border-[color:var(--mc-border-soft)]'
          : 'max-h-[min(440px,65vh)] overflow-auto rounded-md border border-[color:var(--mc-border-strong)]'
        }>
          {artifactsBusy ? (
            <ArtifactListSkeleton />
          ) : artifactsError ? (
            <Callout.Root color="red" size="1" variant="soft" className="m-2">
              <Callout.Text>{artifactsError}</Callout.Text>
            </Callout.Root>
          ) : artifactsList.length === 0 ? (
            <Text size="2" color="gray" className="block p-2">No artifacts found for this persona.</Text>
          ) : (
            <ul className="list-none m-0 p-0">
              {artifactsList.map((it) => (
                <li key={it.id}>
                  <button
                    type="button"
                    onClick={() => setSelectedArtifactId(it.id)}
                    className={selectedArtifactId === it.id
                      ? 'w-full border-0 border-b text-left p-2 bg-[var(--accent-3)]'
                      : 'w-full border-0 border-b text-left p-2'}
                    style={appearance === 'dark' ? { borderBottomColor: 'var(--mc-border-soft)' } : { borderBottomColor: 'var(--gray-6)' }}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <Text size="2" className="truncate">{it.name}</Text>
                      <Text size="1" color="gray">{it.kind}</Text>
                    </div>
                    <Text size="1" color="gray">
                      {formatBytes(it.size_bytes ?? null)} · {it.source}
                    </Text>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      <div className={appearance === 'dark'
        ? 'min-h-[200px] min-w-0 w-full flex-[2] rounded-md border border-[color:var(--mc-border-soft)] p-2 md:min-w-[320px]'
        : 'min-h-[200px] min-w-0 w-full flex-[2] rounded-md border border-[color:var(--mc-border-strong)] p-2 md:min-w-[320px]'
      }>
        {selectedArtifact == null ? (
          <Text size="2" color="gray">Select an artifact to preview.</Text>
        ) : (
          <>
            <Flex justify="between" align="center" mb="2" wrap="wrap" gap="2">
              <div>
                <Text size="2" weight="bold">{selectedArtifact.name}</Text>
                <Text size="1" color="gray" className="block">
                  {selectedArtifact.created_at ?? 'unknown time'} · {selectedArtifact.kind}
                </Text>
              </div>
              <Flex gap="2">
                <Button size="1" variant="soft" onClick={() => window.open(selectedArtifact.url, '_blank', 'noopener,noreferrer')}>
                  Open
                </Button>
                <Button size="1" variant="soft" onClick={() => window.open(`${selectedArtifact.url}${selectedArtifact.url.includes('?') ? '&' : '?'}download=1`, '_blank', 'noopener,noreferrer')}>
                  Download
                </Button>
              </Flex>
            </Flex>
            {selectedArtifact.kind === 'image' ? (
              <img src={artifactPreviewUrl(selectedArtifact)} alt={selectedArtifact.name} className="max-h-[56vh] w-full object-contain" />
            ) : selectedArtifact.kind === 'markdown' ? (
              artifactTextBusy ? (
                <ContentPreviewSkeleton />
              ) : artifactTextError ? (
                <Callout.Root color="red" size="1" variant="soft">
                  <Callout.Text>{artifactTextError}</Callout.Text>
                </Callout.Root>
              ) : (
                <div className="aui-md-root max-h-[56vh] overflow-auto text-sm leading-relaxed">
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={{
                      a: (props) => <MarkdownExternalLink {...props} />,
                    }}
                  >
                    {artifactTextPreview}
                  </ReactMarkdown>
                </div>
              )
            ) : selectedArtifact.kind === 'html' ? (
              <iframe
                title={selectedArtifact.name}
                src={artifactPreviewUrl(selectedArtifact)}
                sandbox="allow-same-origin"
                className="h-[56vh] w-full rounded border border-[color:var(--gray-6)]"
              />
            ) : selectedArtifact.kind === 'text' ? (
              artifactTextBusy ? (
                <ContentPreviewSkeleton />
              ) : artifactTextError ? (
                <Callout.Root color="red" size="1" variant="soft">
                  <Callout.Text>{artifactTextError}</Callout.Text>
                </Callout.Root>
              ) : (
                <pre className="max-h-[56vh] overflow-auto whitespace-pre-wrap text-xs">{artifactTextPreview}</pre>
              )
            ) : (
              <Text size="2" color="gray">
                Preview unavailable for this file type. Use Open or Download.
              </Text>
            )}
          </>
        )}
      </div>
    </Flex>
    <Flex justify="end" mt="3">
      <Dialog.Close>
        <Button variant="soft">Close</Button>
      </Dialog.Close>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>

<Dialog.Root
  open={memoryDialogOpen}
  onOpenChange={memory.onOpenChange}
>
{memoryDialogOpen ? (
<Dialog.Content style={{ maxWidth: 900 }}>
    <Dialog.Title>Persona memory</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Edit this persona’s tiered memory file. Memory is context, not a task queue.
    </Dialog.Description>

    {memoryPathHint ? (
      <Text size="1" color="gray" className="mb-2 block">
        {memoryPathHint}
      </Text>
    ) : null}

    {memoryError ? (
      <Callout.Root color="red" size="1" variant="soft" className="mb-2">
        <Callout.Text>{memoryError}</Callout.Text>
      </Callout.Root>
    ) : null}

    <textarea
      value={memoryContent}
      onChange={(e) => setMemoryContent(e.target.value)}
      spellCheck={false}
      className={appearance === 'dark'
        ? 'h-[420px] w-full rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3 font-mono text-xs text-[color:var(--mc-text-primary)]'
        : 'h-[420px] w-full rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] p-3 font-mono text-xs text-[color:var(--mc-text-primary)]'}
    />

    <Flex justify="between" align="center" mt="3" wrap="wrap" gap="2">
      <Text size="1" color="gray">
        {memoryMtimeMs != null ? `mtime: ${memoryMtimeMs}` : ''}
      </Text>
      <Flex gap="2">
        <Button
          size="1"
          variant="soft"
          onClick={() => {
            if (activePersonaId != null) void loadPersonaMemory(activePersonaId)
          }}
          disabled={memoryBusy || activePersonaId == null}
        >
          Reload
        </Button>
        <Button
          size="1"
          onClick={() => {
            if (activePersonaId != null) void savePersonaMemory(activePersonaId)
          }}
          disabled={memoryBusy || activePersonaId == null}
        >
          {memoryBusy ? 'Saving…' : 'Save'}
        </Button>
        <Dialog.Close>
          <Button size="1" variant="soft">Close</Button>
        </Dialog.Close>
      </Flex>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>

<Dialog.Root
  open={agentHistoryDialogOpen}
  onOpenChange={agentHistory.onOpenChange}
>
{agentHistoryDialogOpen ? (
<Dialog.Content style={{ maxWidth: 960 }}>
    <Dialog.Title>Agent run debug</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Latest saved run for this persona: tool/iteration trace and the system prompt plus messages
      sent on the first LLM call (new runs only). On Run trace, use Prev/Next or ← → to step
      through iterations.
    </Dialog.Description>

    {agentHistoryPathHint ? (
      <Text size="1" color="gray" className="mb-1 block">
        {agentHistoryPathHint}
      </Text>
    ) : null}
    {agentHistoryFilename ? (
      <Text size="1" color="gray" className="mb-2 block">
        File: {agentHistoryFilename}
        {agentHistoryMtimeMs != null ? ` · mtime: ${agentHistoryMtimeMs}` : ''}
      </Text>
    ) : null}

    {agentHistoryBusy ? (
      <SettingsPanelSkeleton />
    ) : null}

    {agentHistoryError ? (
      <Callout.Root color="orange" size="1" variant="soft" className="mb-2">
        <Callout.Text>{agentHistoryError}</Callout.Text>
      </Callout.Root>
    ) : null}

    {!agentHistoryBusy && !agentHistoryError && agentHistoryParsed != null ? (
      <Tabs.Root
        value={agentHistoryTab}
        onValueChange={(v) =>
          setAgentHistoryTab(v === 'prompt' ? 'prompt' : v === 'evaluators' ? 'evaluators' : 'trace')
        }
      >
        <Tabs.List size="1" className="mb-3 flex-wrap">
          <Tabs.Trigger value="trace">Run trace</Tabs.Trigger>
          <Tabs.Trigger value="evaluators">Evaluators</Tabs.Trigger>
          <Tabs.Trigger value="prompt" disabled={agentHistoryParsed.initialPromptJson == null}>
            First-turn prompt
          </Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="trace">
          <>
            {agentHistoryParsed.runHeader.trim() ? (
              <div
                className={
                  appearance === 'dark'
                    ? 'mb-3 max-h-32 overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-2'
                    : 'mb-3 max-h-32 overflow-auto rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-main)] p-2'
                }
              >
                <AgentHistoryMarkdownBody markdown={agentHistoryParsed.runHeader} />
              </div>
            ) : null}

            {agentHistoryParsed.iterations.length > 0 ? (
              <>
                <Flex justify="between" align="center" mb="2" wrap="wrap" gap="2">
                  <Flex direction="column" gap="1">
                    <Text size="2">
                      Iteration {agentHistoryIterationIdx + 1} of{' '}
                      {agentHistoryParsed.iterations.length}
                    </Text>
                    {agentHistoryParsed.iterations[agentHistoryIterationIdx]?.tier ? (
                      <AgentHistoryTierBadge
                        tier={
                          agentHistoryParsed.iterations[agentHistoryIterationIdx]!.tier!
                        }
                      />
                    ) : null}
                  </Flex>
                  <Flex gap="2">
                    <Button
                      size="1"
                      variant="soft"
                      disabled={agentHistoryIterationIdx <= 0}
                      onClick={() =>
                        setAgentHistoryIterationIdx((i) => Math.max(0, i - 1))
                      }
                    >
                      Prev
                    </Button>
                    <Button
                      size="1"
                      variant="soft"
                      disabled={
                        agentHistoryIterationIdx >= agentHistoryParsed.iterations.length - 1
                      }
                      onClick={() =>
                        setAgentHistoryIterationIdx((i) =>
                          Math.min(agentHistoryParsed.iterations.length - 1, i + 1),
                        )
                      }
                    >
                      Next
                    </Button>
                  </Flex>
                </Flex>
                <Text size="1" color="gray" mb="2" className="block">
                  Keyboard: ← →
                </Text>
                <div
                  className={
                    appearance === 'dark'
                      ? 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3'
                      : 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] p-3'
                  }
                >
                  <AgentHistoryMarkdownBody
                    markdown={
                      agentHistoryParsed.iterations[agentHistoryIterationIdx]?.body ?? ''
                    }
                  />
                </div>
              </>
            ) : (
              <div
                className={
                  appearance === 'dark'
                    ? 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3'
                    : 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] p-3'
                }
              >
                <AgentHistoryMarkdownBody markdown={agentHistoryRaw} />
              </div>
            )}
          </>
        </Tabs.Content>
        <Tabs.Content value="evaluators">
          <div
            className={
              appearance === 'dark'
                ? 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-3'
                : 'max-h-[420px] overflow-auto rounded-md border border-[color:var(--mc-border-strong)] bg-[color:var(--mc-surface-elevated)] p-3'
            }
          >
            <AgentHistoryEvaluatorsPanel parsed={agentHistoryParsed} />
          </div>
        </Tabs.Content>
        <Tabs.Content value="prompt">
          {agentHistoryParsed.initialPromptJson ? (
            <InitialRunPromptView
              jsonText={agentHistoryParsed.initialPromptJson}
              appearance={appearance}
            />
          ) : (
            <Callout.Root color="blue" size="1" variant="soft">
              <Callout.Text>
                No first-turn prompt snapshot in this file. Run a new agent turn after upgrading
                the gateway; older history files only contain the iteration trace.
              </Callout.Text>
            </Callout.Root>
          )}
        </Tabs.Content>
      </Tabs.Root>
    ) : null}

    <Flex direction="column" gap="1" mt="3" mb="2">
      <Text size="1" color="gray">
        Optional guidance for Learn &amp; optimize (combined with PDQE feedback from this run).
      </Text>
      <TextArea
        size="1"
        rows={3}
        placeholder="e.g. Focus on vault search habits and reducing repeated bash calls…"
        value={agentHistoryOptimizeNotes}
        onChange={(e) => setAgentHistoryOptimizeNotes(e.target.value)}
        disabled={agentHistoryBusy || agentHistoryOptimizeBusy || activePersonaId == null}
      />
    </Flex>

    <Flex justify="end" mt="3" gap="2" wrap="wrap">
      <Button
        size="1"
        variant="soft"
        color="teal"
        onClick={() => {
          if (activePersonaId != null) {
            void optimizeAgentHistoryLatest(
              activePersonaId,
              agentHistoryOptimizeNotes.trim() || undefined,
            )
          }
        }}
        disabled={
          agentHistoryBusy
          || agentHistoryOptimizeBusy
          || activePersonaId == null
          || !agentHistoryFilename
        }
      >
        {agentHistoryOptimizeBusy ? 'Queuing…' : 'Learn & optimize'}
      </Button>
      <Button
        size="1"
        variant="soft"
        onClick={() => {
          if (activePersonaId != null) void loadAgentHistoryLatest(activePersonaId)
        }}
        disabled={agentHistoryBusy || activePersonaId == null}
      >
        Reload
      </Button>
      <Dialog.Close>
        <Button size="1" variant="soft">Close</Button>
      </Dialog.Close>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>

<Dialog.Root open={terminalDialogOpen} onOpenChange={setTerminalDialogOpen}>
{terminalDialogOpen ? (
<Dialog.Content style={{ maxWidth: 1080, width: 'min(96vw, 1080px)' }} className="flex max-h-[min(88vh,900px)] flex-col">
    <Dialog.Title>Terminal</Dialog.Title>
    <Dialog.Description size="2" mb="3">
      Interactive shell in the gateway workspace. Operator-only; requires WEB_AUTH_TOKEN and WEB_TERMINAL_ENABLED.
    </Dialog.Description>
    {terminalError ? (
      <Callout.Root color="red" size="1" variant="soft" className="mb-2 shrink-0">
        <Callout.Text>{terminalError}</Callout.Text>
      </Callout.Root>
    ) : null}
    <React.Suspense fallback={<SettingsPanelSkeleton />}>
      <TerminalPane
        active={terminalDialogOpen}
        onError={(message) => setTerminalError(message)}
      />
    </React.Suspense>
    <Flex justify="end" mt="3" className="shrink-0">
      <Dialog.Close>
        <Button size="1" variant="soft">Close</Button>
      </Dialog.Close>
    </Flex>
  </Dialog.Content>
) : null}
</Dialog.Root>
    </>
  )
}
