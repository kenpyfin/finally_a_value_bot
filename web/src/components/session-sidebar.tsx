import React, { useEffect, useRef, useState } from 'react'
import { Badge, Button, Flex, ScrollArea, Separator, Text } from '@radix-ui/themes'
import type { Persona } from '../types'

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
}: SessionSidebarProps) {
  const isDark = appearance === 'dark'
  const [themeMenuOpen, setThemeMenuOpen] = useState(false)
  const themeMenuRef = useRef<HTMLDivElement | null>(null)
  const themeButtonRef = useRef<HTMLButtonElement | null>(null)

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
      className={isDark ? 'flex h-full min-h-0 flex-col border-r p-4' : 'flex h-full min-h-0 flex-col border-r border-slate-200 bg-white p-4'}
      style={isDark ? { borderColor: 'var(--mc-border-soft)', background: 'var(--mc-bg-sidebar)' } : undefined}
    >
      <Flex justify="between" align="center" className="mb-4">
        <div className="flex items-center gap-2">
          <img
            src="/icon.png"
            alt="FinallyAValueBot"
            className="h-7 w-7 rounded-md border border-black/10 object-cover"
            loading="eager"
            decoding="async"
          />
          <Text size="5" weight="bold">
            FinallyAValueBot
          </Text>
        </div>
        <div className="relative flex items-center gap-2">
          <button
            ref={themeButtonRef}
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              setThemeMenuOpen((v) => !v)
            }}
            aria-label="Change UI theme color"
            className={
              isDark
                ? 'inline-flex h-8 w-8 items-center justify-center rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] text-slate-200 hover:brightness-110'
                : 'inline-flex h-8 w-8 items-center justify-center rounded-md border border-slate-300 bg-white text-slate-700 hover:bg-slate-100'
            }
          >
            <span className="text-sm">🎨</span>
          </button>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              onToggleAppearance()
            }}
            aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
            className={
              isDark
                ? 'inline-flex h-8 w-8 items-center justify-center rounded-md border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] text-slate-200 hover:brightness-110'
                : 'inline-flex h-8 w-8 items-center justify-center rounded-md border border-slate-300 bg-white text-slate-700 hover:bg-slate-100'
            }
          >
            <span className="text-sm">{isDark ? '☀' : '☾'}</span>
          </button>
          {themeMenuOpen ? (
            <div
              ref={themeMenuRef}
              className={
                isDark
                  ? 'absolute right-0 top-10 z-50 w-56 rounded-lg border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-sidebar)] p-2 shadow-xl'
                  : 'absolute right-0 top-10 z-50 w-56 rounded-lg border border-slate-300 bg-white p-2 shadow-xl'
              }
            >
              <Text size="1" color="gray">Theme</Text>
              <div className="mt-2 grid grid-cols-2 gap-1">
                {uiThemeOptions.map((theme) => (
                  <button
                    key={theme.key}
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation()
                      onUiThemeChange(theme.key)
                      setThemeMenuOpen(false)
                    }}
                    className={
                      uiTheme === theme.key
                        ? isDark
                          ? 'flex items-center gap-2 rounded-md border border-[color:var(--mc-accent)] bg-[color:var(--mc-bg-panel)] px-2 py-1 text-left text-xs text-slate-100'
                          : 'flex items-center gap-2 rounded-md border px-2 py-1 text-left text-xs text-slate-900'
                        : isDark
                          ? 'flex items-center gap-2 rounded-md border border-transparent px-2 py-1 text-left text-xs text-slate-300 hover:border-[color:var(--mc-border-soft)] hover:bg-[color:var(--mc-bg-panel)]'
                          : 'flex items-center gap-2 rounded-md border border-transparent px-2 py-1 text-left text-xs text-slate-600 hover:border-slate-200 hover:bg-slate-50'
                    }
                    style={!isDark && uiTheme === theme.key ? { borderColor: 'var(--mc-accent)', backgroundColor: 'color-mix(in srgb, var(--mc-accent) 12%, white)' } : undefined}
                  >
                    <span
                      className={isDark ? 'h-3 w-3 rounded-sm border border-white/20' : 'h-3 w-3 rounded-sm border border-slate-300'}
                      style={{ backgroundColor: theme.color }}
                      aria-hidden="true"
                    />
                    {theme.label}
                  </button>
                ))}
              </div>
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

      <div
        className={
          isDark
            ? 'min-h-0 flex-1 rounded-xl border border-[color:var(--mc-border-soft)] bg-[color:var(--mc-bg-panel)] p-2'
            : 'min-h-0 flex-1 rounded-xl border border-slate-200 bg-slate-50/70 p-2'
        }
      >
        <ScrollArea type="auto" style={{ height: '100%' }}>
          <div className="flex flex-col gap-1 pr-1">
            {personas.length === 0 ? (
              <Text size="1" color="gray">Loading…</Text>
            ) : (
              personas.map((p) => (
                <div
                  key={p.id}
                  className={
                    selectedPersonaId === p.id
                      ? isDark
                        ? 'flex w-full items-center justify-between gap-1 rounded-lg border border-[color:var(--mc-accent)] bg-[color:var(--mc-bg-panel)] px-3 py-2 shadow-sm'
                        : 'flex w-full items-center justify-between gap-1 rounded-lg border bg-white px-3 py-2 shadow-sm'
                      : isDark
                        ? 'flex w-full items-center justify-between gap-1 rounded-lg border border-transparent px-3 py-2 text-slate-300 hover:border-[color:var(--mc-border-soft)] hover:bg-[color:var(--mc-bg-panel)]'
                        : 'flex w-full items-center justify-between gap-1 rounded-lg border border-transparent px-3 py-2 text-slate-600 hover:border-slate-200 hover:bg-white'
                  }
                  style={
                    !isDark && selectedPersonaId === p.id
                      ? { borderColor: 'color-mix(in srgb, var(--mc-accent) 36%, #94a3b8)' }
                      : undefined
                  }
                >
                  <button
                    type="button"
                    className="min-w-0 flex-1 text-left text-sm font-medium"
                    onClick={() => onPersonaSelect(p.name)}
                  >
                    <span className="inline-flex items-center gap-2">
                      <span className="truncate">{p.name}</span>
                      {personaHasNew?.[p.id] ? (
                        <span
                          className={isDark ? 'h-2 w-2 rounded-full bg-[color:var(--mc-accent)]' : 'h-2 w-2 rounded-full bg-[color:var(--mc-accent)]'}
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
                      onClick={(e) => { e.stopPropagation(); onDeletePersona(p.id) }}
                      title={`Delete persona "${p.name}"`}
                      className={
                        isDark
                          ? 'rounded p-1 text-slate-400 hover:bg-red-900/30 hover:text-red-400'
                          : 'rounded p-1 text-slate-400 hover:bg-red-50 hover:text-red-600'
                      }
                      aria-label={`Delete ${p.name}`}
                    >
                      🗑
                    </button>
                  ) : null}
                </div>
              ))
            )}
          </div>
        </ScrollArea>
      </div>

      <div className={isDark ? 'mt-4 rounded-lg border border-[color:var(--mc-border-soft)] p-3' : 'mt-4 rounded-lg border border-slate-200 p-3'}>
        <Text size="2" weight="bold" className="mb-2 block">Human–AI relationship</Text>
        <img
          src="/human-ai-relationship.png"
          alt="Human and AI collaboration"
          className="w-full rounded-md border border-black/10 object-contain"
          loading="lazy"
        />
      </div>

      <div className={isDark ? 'mt-4 border-t border-[color:var(--mc-border-soft)] pt-3' : 'mt-4 border-t border-slate-200 pt-3'}>
        <div className="mt-3 flex flex-col items-center gap-1">
          <a
            href="https://finally-a-value-bot.ai"
            target="_blank"
            rel="noreferrer"
            className={isDark ? 'text-xs text-slate-400 hover:text-slate-200' : 'text-xs text-slate-600 hover:text-slate-900'}
          >
            finally-a-value-bot.ai
          </a>
        </div>
      </div>

    </aside>
  )
}
