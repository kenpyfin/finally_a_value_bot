import React from 'react'
import { Button, Flex, Heading, IconButton } from '@radix-ui/themes'
import { SessionPicker } from '../components/session-picker'
import { SideChatsPicker } from '../components/side-chats-picker'
import { CockpitStatusChip } from '../components/ops-ui'
import { IconCockpit, IconInbox, IconOps, IconSettings } from '../components/icons'
import type { ChatSession, InstallationStatus, QueueLane, SideChatSummary } from '../types'

type Appearance = 'dark' | 'light'

export type AppHeaderNavProps = {
  mobileNavOpen: boolean
  onOpenMobileNav: () => void
  desktopSidebarOpen: boolean
  onToggleDesktopSidebar: () => void
}

export type AppHeaderSessionProps = {
  activePersonaId: number | null
  activePersonaName: string | null
  chatSessions: ChatSession[]
  activeSessionId: string | null
  historyLoading: boolean
  onSelectSession: (sessionId: string | null) => void
  onCreateSession: (intent: string, mirrorMainChat: boolean) => Promise<void>
  onDeleteSession: (sessionId: string) => Promise<void>
  sideChats: SideChatSummary[]
  activeSideChatId: string | null
  sideChatsLoading?: boolean
  onSelectSideChat: (sideChatId: string) => void
  onDeleteSideChat: (sideChatId: string) => Promise<boolean>
}

export type AppHeaderToolbarProps = {
  queueLane: QueueLane | null
  backgroundActiveCount: number
  installationStatus: InstallationStatus | null
  statusText: string
  cockpitExpanded?: boolean
  onToggleCockpit: () => void
  onOpenSettings: () => void
  onOpenInbox: () => void
  inboxBadgeCount?: number
  onOpenSchedules: () => void
  onOpenPrinciples: () => void
  onOpenArtifacts: () => void
  onOpenMemory: () => void
  onOpenAgentHistory: () => void
  onOpenTerminal?: () => void
  terminalAvailable?: boolean
  agentHistoryDisabled?: boolean
}

export type AppHeaderProps = {
  appearance: Appearance
  mobileChatHeaderCollapsed: boolean
  nav: AppHeaderNavProps
  session: AppHeaderSessionProps
  toolbar: AppHeaderToolbarProps
  onOpenMobileSettings: () => void
  onOpenMobileOps: () => void
}

export const AppHeader = React.memo(function AppHeader({
  appearance,
  mobileChatHeaderCollapsed,
  nav,
  session,
  toolbar,
  onOpenMobileSettings,
  onOpenMobileOps,
}: AppHeaderProps) {
  const {
    mobileNavOpen,
    onOpenMobileNav,
    desktopSidebarOpen,
    onToggleDesktopSidebar,
  } = nav
  const {
    activePersonaId,
    activePersonaName,
    chatSessions,
    activeSessionId,
    historyLoading,
    onSelectSession,
    onCreateSession,
    onDeleteSession,
    sideChats,
    activeSideChatId,
    sideChatsLoading,
    onSelectSideChat,
    onDeleteSideChat,
  } = session
  const {
    queueLane,
    backgroundActiveCount,
    installationStatus,
    statusText,
    cockpitExpanded = false,
    onToggleCockpit,
    onOpenSettings,
    onOpenInbox,
    inboxBadgeCount = 0,
    onOpenSchedules,
    onOpenPrinciples,
    onOpenArtifacts,
    onOpenMemory,
    onOpenAgentHistory,
    onOpenTerminal,
    terminalAvailable = false,
    agentHistoryDisabled = false,
  } = toolbar

  const cockpitBusyCount = (queueLane?.pending ?? 0) + backgroundActiveCount

  const renderSessionControls = (className?: string) =>
    activePersonaId == null ? null : (
      <Flex
        align="center"
        className={`mc-session-controls min-w-0 ${className ?? ''}`}
      >
        <SessionPicker
          compact
          sessions={chatSessions}
          activeSessionId={activeSessionId}
          onSelectSession={onSelectSession}
          onCreateSession={onCreateSession}
          onDeleteSession={onDeleteSession}
          loading={historyLoading}
        />
        <span className="mc-toolbar-divider" aria-hidden />
        <SideChatsPicker
          sideChats={sideChats}
          sessions={chatSessions}
          activeSideChatId={activeSideChatId}
          loading={sideChatsLoading}
          onSelect={onSelectSideChat}
          onDelete={onDeleteSideChat}
        />
      </Flex>
    )

  return (
    <header
      className={
        appearance === 'dark'
          ? 'sticky top-0 z-30 border-b border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)]/95 backdrop-blur-sm md:top-0'
          : 'sticky top-0 z-30 border-b border-[color:var(--mc-border-soft)] bg-[color:var(--mc-surface-elevated)]/92 backdrop-blur-sm md:top-0'
      }
    >
      <div className="mc-app-header-bar px-3 max-md:py-2 md:px-4 md:py-3">
        <Flex
          justify="between"
          align="center"
          gap="2"
          wrap="wrap"
          className="w-full flex-col md:flex-row md:flex-wrap"
        >
          <Flex
            align="center"
            gap="2"
            className="min-w-0 w-full min-h-[44px] md:flex-1"
          >
            <IconButton
              size="3"
              variant="soft"
              color="gray"
              className="shrink-0 min-h-10 min-w-10 md:!hidden"
              type="button"
              aria-expanded={mobileNavOpen}
              aria-haspopup="dialog"
              aria-controls="mobile-session-sidebar-panel"
              aria-label="Open personas and theme"
              title="Personas & theme"
              onClick={onOpenMobileNav}
            >
              <svg
                className="size-5 shrink-0"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden
              >
                <path d="M4 6h16M4 12h16M4 18h16" />
              </svg>
            </IconButton>
            <IconButton
              size="3"
              variant="soft"
              color="gray"
              className="!hidden shrink-0 md:!inline-flex"
              type="button"
              aria-expanded={desktopSidebarOpen}
              aria-label={desktopSidebarOpen ? 'Hide personas sidebar' : 'Show personas sidebar'}
              title={desktopSidebarOpen ? 'Hide personas' : 'Show personas'}
              onClick={onToggleDesktopSidebar}
            >
              <span aria-hidden className="text-base leading-none">
                {desktopSidebarOpen ? '⟨' : '⟩'}
              </span>
            </IconButton>
            <Heading
              size="6"
              className="min-w-0 shrink truncate max-md:[font-size:1.125rem]"
            >
              {activePersonaName ?? 'Chat'}
            </Heading>
            {renderSessionControls('max-md:!hidden')}
            <div className="ml-auto flex shrink-0 items-center gap-1 md:hidden">
              <button
                type="button"
                className="mc-cockpit-launch cursor-pointer"
                data-active={cockpitExpanded ? 'true' : 'false'}
                data-busy={cockpitBusyCount > 0 ? 'true' : 'false'}
                aria-pressed={cockpitExpanded}
                aria-label={
                  cockpitBusyCount > 0 ? `Cockpit, ${cockpitBusyCount} active` : 'Cockpit'
                }
                title="Cockpit"
                onClick={onToggleCockpit}
              >
                <IconCockpit className="size-4 shrink-0" />
                <span className="mc-cockpit-launch-label">Cockpit</span>
                {cockpitBusyCount > 0 ? (
                  <span className="mc-badge-counter" aria-hidden>
                    {cockpitBusyCount > 99 ? '99+' : cockpitBusyCount}
                  </span>
                ) : null}
              </button>
              <span className="mc-toolbar-divider" aria-hidden />
              <button
                type="button"
                className="mc-inbox-launch mc-inbox-launch--icon cursor-pointer min-h-10 min-w-10"
                data-has-items={inboxBadgeCount > 0 ? 'true' : 'false'}
                aria-label={
                  inboxBadgeCount > 0 ? `Inbox, ${inboxBadgeCount} items` : 'Inbox'
                }
                title="Inbox"
                onClick={onOpenInbox}
              >
                <IconInbox className="size-5 shrink-0" />
                {inboxBadgeCount > 0 ? (
                  <span className="mc-inbox-launch__badge" aria-hidden>
                    {inboxBadgeCount > 99 ? '99+' : inboxBadgeCount}
                  </span>
                ) : null}
              </button>
              <IconButton
                size="2"
                variant="soft"
                color="gray"
                type="button"
                className="cursor-pointer min-h-10 min-w-10"
                aria-label="Settings"
                title="Settings"
                onClick={onOpenMobileSettings}
              >
                <IconSettings />
              </IconButton>
              <IconButton
                size="2"
                variant="soft"
                color="gray"
                type="button"
                className="cursor-pointer min-h-10 min-w-10"
                aria-label="Operator tools"
                title="Operator tools"
                onClick={onOpenMobileOps}
              >
                <IconOps />
              </IconButton>
            </div>
          </Flex>
          {activePersonaId != null ? (
            <div
              className={`mc-mobile-session-subbar w-full ${
                mobileChatHeaderCollapsed ? 'mc-mobile-session-subbar--collapsed' : ''
              }`}
              aria-hidden={mobileChatHeaderCollapsed}
            >
              <div className="mc-mobile-session-subbar-inner">
                {renderSessionControls('w-full justify-start')}
              </div>
            </div>
          ) : null}
          <Flex align="center" gap="2" wrap="wrap" justify="end" className="w-full max-md:!hidden md:!flex">
            <CockpitStatusChip
              queueLane={queueLane}
              backgroundActiveCount={backgroundActiveCount}
              installationStatus={installationStatus}
              statusText={statusText}
              onClick={onToggleCockpit}
            />
            <button
              type="button"
              className="mc-inbox-launch !hidden md:!inline-flex"
              data-has-items={inboxBadgeCount > 0 ? 'true' : 'false'}
              aria-label={
                inboxBadgeCount > 0 ? `Inbox, ${inboxBadgeCount} items` : 'Open Inbox'
              }
              title="Inbox — new messages and todos"
              onClick={onOpenInbox}
            >
              Inbox
              {inboxBadgeCount > 0 ? (
                <span className="mc-inbox-launch__badge" aria-hidden>
                  {inboxBadgeCount > 99 ? '99+' : inboxBadgeCount}
                </span>
              ) : null}
            </button>
            <span className="mc-toolbar-divider !hidden md:!inline-block" aria-hidden />
            <Button size="1" variant="soft" className="!hidden md:!inline-flex" onClick={onOpenSettings}>
              Settings
            </Button>
            <Button size="1" variant="soft" className="!hidden md:!inline-flex" onClick={onOpenSchedules}>
              Schedules
            </Button>
            <Button size="1" variant="soft" className="!hidden md:!inline-flex" onClick={onOpenPrinciples}>
              Principles
            </Button>
            <Button size="1" variant="soft" className="!hidden md:!inline-flex" onClick={onOpenArtifacts}>
              Artifacts
            </Button>
            {terminalAvailable && onOpenTerminal ? (
              <Button size="1" variant="soft" className="!hidden md:!inline-flex" onClick={onOpenTerminal}>
                Terminal
              </Button>
            ) : null}
            <Button size="1" variant="soft" className="!hidden md:!inline-flex" onClick={onOpenMemory}>
              Memory
            </Button>
            <Button
              size="1"
              variant="soft"
              className="!hidden md:!inline-flex"
              disabled={agentHistoryDisabled}
              onClick={onOpenAgentHistory}
            >
              Last agent run
            </Button>
          </Flex>
        </Flex>
      </div>
    </header>
  )
})
