import React, { useEffect, useMemo, useRef, useState } from 'react'
import { Badge, Button, Flex, ScrollArea, Separator, Text } from '@radix-ui/themes'
import type { Persona } from '../types'

const PRIMARY_THEME_KEYS = new Set(['green', 'slate', 'blue'])

function IconPalette({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
      <path strokeLinecap="round" strokeLinejoin="round" d="M12 2a10 10 0 1 0 0 20 4 4 0 0 0 0-8 2.5 2.5 0 0 1-2.4-2.4 4 4 0 0 0-1.6-3.2A10 10 0 0 0 12 2Z" />
      <circle cx="7.5" cy="10.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="10.5" cy="7.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="14.5" cy="8.5" r="1" fill="currentColor" stroke="none" />
    </svg>
  )
}

function IconSun({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
      <circle cx="12" cy="12" r="4" />
      <path strokeLinecap="round" d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41" />
    </svg>
  )
}

function IconMoon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
      <path strokeLinecap="round" strokeLinejoin="round" d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  )
}

function IconTrash({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden>
      <path strokeLinecap="round" strokeLinejoin="round" d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
    </svg>
  )
}

const iconBtnClass =
  'inline-flex h-8 w-8 items-center justify-center rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] text-[color:var(--mc-text-muted)] hover:text-[color:var(--mc-text-primary)] hover:brightness-110'

type SessionSidebarProps = {
  appearance: 'dark' | 'light'
  onToggleAppearance: () => void
  uiTheme: string
  onUiThemeChange: (theme: string) => void
  uiThemeOptions: Array<{ key: string; label: string; color: string }>
  personas: Persona[]
  personaHasNew?: Record<number, boolean>
  selectedPersonaId: number | null
  onPersonaSelect: (personaName: string) => void
  onCreatePersona: () => void
  onDeletePersona: (personaId: number) => void
  onCloseRequest?: () => void
}

function ThemeSwatchButton({
  theme,
  selected,
  onSelect,
}: {
  theme: { key: string; label: string; color: string }
  selected: boolean
  onSelect: () => void
}) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation()
        onSelect()
      }}
      className={
        selected
          ? 'flex items-center gap-2 rounded-md border border-[color:var(--mc-accent)] bg-[color:var(--mc-bg-panel)] px-2 py-1 text-left text-xs text-[color:var(--mc-text-primary)]'
          : 'flex items-center gap-2 rounded-md border border-transparent px-2 py-1 text-left text-xs text-[color:var(--mc-text-muted)] hover:border-[color:var(--mc-border-soft)] hover:bg-[color:var(--mc-bg-panel)]'
      }
      style={
        selected
          ? { borderColor: 'var(--mc-accent)', backgroundColor: 'color-mix(in srgb, var(--mc-accent) 12%, var(--mc-surface-elevated))' }
          : undefined
      }
    >
      <span
        className="h-3 w-3 rounded-sm border border-[color:var(--mc-border-soft)]"
        style={{ backgroundColor: theme.color }}
        aria-hidden="true"
      />
      {theme.label}
    </button>
  )
}

export function SessionSidebar({
  appearance,
  onToggleAppearance,
  uiTheme,
  onUiThemeChange,
  uiThemeOptions,
  personas,
  personaHasNew,
  selectedPersonaId,
  onPersonaSelect,
  onCreatePersona,
  onDeletePersona,
  onCloseRequest,
}: SessionSidebarProps) {
  const isDark = appearance === 'dark'
  const [themeMenuOpen, setThemeMenuOpen] = useState(false)
  const [moreThemesOpen, setMoreThemesOpen] = useState(false)
  const themeMenuRef = useRef<HTMLDivElement | null>(null)
  const themeButtonRef = useRef<HTMLButtonElement | null>(null)

  const { primaryThemes, moreThemes } = useMemo(() => {
    const primary = uiThemeOptions.filter((t) => PRIMARY_THEME_KEYS.has(t.key))
    const more = uiThemeOptions.filter((t) => !PRIMARY_THEME_KEYS.has(t.key))
    return { primaryThemes: primary, moreThemes: more }
  }, [uiThemeOptions])

  useEffect(() => {
    if (moreThemes.some((t) => t.key === uiTheme)) {
      setMoreThemesOpen(true)
    }
  }, [uiTheme, moreThemes])

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null
      if (!target) return
      if (themeButtonRef.current?.contains(target)) return
      if (themeMenuRef.current?.contains(target)) return
      setThemeMenuOpen(false)
    }
    const closeOnScroll = () => setThemeMenuOpen(false)
    window.addEventListener('pointerdown', onPointerDown)
    window.addEventListener('scroll', closeOnScroll, true)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown)
      window.removeEventListener('scroll', closeOnScroll, true)
    }
  }, [])

  return (
    <aside
      className="flex h-full min-h-0 flex-col border-r border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-sidebar)] p-4"
    >
      <Flex justify="between" align="center" className="mb-4">
        <div className="min-w-0">
          <Text size="5" weight="bold" className="tracking-tight">
            FinallyAValueBot
          </Text>
          <Text size="1" color="gray" className="mt-0.5 block">
            Personas & sessions
          </Text>
        </div>
        <div className="relative flex items-center gap-2">
          {onCloseRequest ? (
            <button
              type="button"
              onClick={() => onCloseRequest()}
              aria-label="Close menu"
              title="Close"
              className={`${iconBtnClass} h-10 w-10 shrink-0 md:hidden`}
            >
              <svg className="size-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden>
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          ) : null}
          <button
            ref={themeButtonRef}
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              setThemeMenuOpen((v) => !v)
            }}
            aria-label="Change UI theme color"
            title="Theme color"
            className={iconBtnClass}
          >
            <IconPalette className="size-4" />
          </button>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              onToggleAppearance()
            }}
            aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
            title={isDark ? 'Light mode' : 'Dark mode'}
            className={iconBtnClass}
          >
            {isDark ? <IconSun className="size-4" /> : <IconMoon className="size-4" />}
          </button>
          {themeMenuOpen ? (
            <div
              ref={themeMenuRef}
              className="absolute right-0 top-10 z-50 w-56 rounded-lg border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-sidebar)] p-2"
            >
              <Text size="1" color="gray">Theme</Text>
              <div className="mt-2 grid grid-cols-2 gap-1">
                {primaryThemes.map((theme) => (
                  <ThemeSwatchButton
                    key={theme.key}
                    theme={theme}
                    selected={uiTheme === theme.key}
                    onSelect={() => {
                      onUiThemeChange(theme.key)
                      setThemeMenuOpen(false)
                    }}
                  />
                ))}
              </div>
              {moreThemes.length > 0 ? (
                <div className="mt-2">
                  <button
                    type="button"
                    className="w-full rounded-md px-1 py-1 text-left text-[11px] text-[color:var(--mc-text-muted)] hover:text-[color:var(--mc-text-primary)]"
                    onClick={(e) => {
                      e.stopPropagation()
                      setMoreThemesOpen((v) => !v)
                    }}
                  >
                    {moreThemesOpen ? 'Fewer colors' : 'More colors'}
                  </button>
                  {moreThemesOpen ? (
                    <div className="mt-1 grid grid-cols-2 gap-1">
                      {moreThemes.map((theme) => (
                        <ThemeSwatchButton
                          key={theme.key}
                          theme={theme}
                          selected={uiTheme === theme.key}
                          onSelect={() => {
                            onUiThemeChange(theme.key)
                            setThemeMenuOpen(false)
                          }}
                        />
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      </Flex>

      <Flex justify="between" align="center" className="mb-2">
        <Text size="2" weight="medium" color="gray">
          Persona
        </Text>
        <Button size="1" variant="soft" onClick={onCreatePersona} title="New persona">
          + New
        </Button>
      </Flex>

      <Separator size="4" className="my-2" />

      <ScrollArea type="auto" className="min-h-0 flex-1">
        <div className="flex flex-col pr-1">
          {personas.length === 0 ? (
            <Text size="1" color="gray">Loading…</Text>
          ) : (
            personas.map((p, index) => (
              <div
                key={p.id}
                className={
                  index < personas.length - 1
                    ? 'border-b border-[color:var(--mc-border-soft)]'
                    : undefined
                }
              >
                <div
                  className={
                    selectedPersonaId === p.id
                      ? 'flex w-full items-center justify-between gap-1 border-l-2 border-[color:var(--mc-accent)] bg-[color:var(--mc-bg-panel)] px-3 py-2'
                      : 'flex w-full items-center justify-between gap-1 border-l-2 border-transparent px-3 py-2 text-[color:var(--mc-text-muted)] hover:bg-[color:var(--mc-bg-panel)]/60'
                  }
                >
                  <button
                    type="button"
                    className="min-w-0 flex-1 text-left text-sm font-medium text-[color:var(--mc-text-primary)]"
                    onClick={() => {
                      onPersonaSelect(p.name)
                      onCloseRequest?.()
                    }}
                  >
                    <span className="inline-flex items-center gap-2">
                      <span className="truncate">{p.name}</span>
                      {personaHasNew?.[p.id] ? (
                        <span
                          className="h-2 w-2 rounded-full bg-[color:var(--mc-accent)]"
                          aria-label="New messages"
                          title="New messages"
                        />
                      ) : null}
                    </span>
                  </button>
                  {p.is_active ? <Badge size="1" variant="soft">active</Badge> : null}
                  {p.name !== 'default' ? (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation()
                        onDeletePersona(p.id)
                      }}
                      title={`Delete persona "${p.name}"`}
                      className="rounded p-1 text-[color:var(--mc-text-muted)] hover:bg-red-900/20 hover:text-red-400"
                      aria-label={`Delete ${p.name}`}
                    >
                      <IconTrash className="size-3.5" />
                    </button>
                  ) : null}
                </div>
              </div>
            ))
          )}
        </div>
      </ScrollArea>

      <div className="mt-4 border-t border-[color:var(--mc-border-soft)] pt-3">
        <div className="mt-3 flex flex-col items-center gap-1">
          <a
            href="https://finally-a-value-bot.ai"
            target="_blank"
            rel="noreferrer"
            className="text-xs text-[color:var(--mc-text-muted)] hover:text-[color:var(--mc-text-primary)]"
          >
            finally-a-value-bot.ai
          </a>
        </div>
      </div>
    </aside>
  )
}
