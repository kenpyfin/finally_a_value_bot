# Web UI design system

Dark-first operator cockpit styling for `web/`. Product tokens stay `--mc-*`; shadcn/ui tokens bridge from them so both systems share one palette during the phased migration.

## Tokens

| Role | Product token | shadcn bridge |
|------|---------------|---------------|
| Page background | `--mc-bg-main` | `--background` |
| Sidebar | `--mc-bg-sidebar` | `--sidebar` |
| Elevated panel | `--mc-bg-panel` | `--card`, `--popover` |
| Soft / strong border | `--mc-border-soft` / `--mc-border-strong` | `--border` / `--input` |
| Body / muted / faint text | `--mc-text-primary` / `--mc-text-muted` / `--mc-text-faint` | `--foreground` / `--muted-foreground` |
| Accent | `--mc-accent` (+ `--mc-accent-hsl`) | `--primary`, `--ring` |
| Radius | `--mc-radius` | `--radius` |

Accent variants live on `html[data-ui-theme="…"]` for both light (`html:not(.dark)`) and dark (`html.dark`). Appearance toggle writes `html.dark` and persists to `localStorage` (`finally-a-value-bot_appearance`).

## Typography

- **Sans:** Geist Variable (`@fontsource-variable/geist`), self-hosted
- **Mono:** JetBrains Mono (`@fontsource/jetbrains-mono`), self-hosted; also `--mc-font-mono` / `font-mono`
- Do not load Google Fonts from the binary-hosted UI

## Spacing and radius

- Prefer Tailwind spacing scale; dense operator UI favors `gap-2` / `p-2`–`p-3` over large marketing gaps
- Default radius: `0.5rem` (`--mc-radius`)

## Motion

- Micro interactions: 150–200ms
- Pane / sheet transitions: ≤250ms
- Honor `prefers-reduced-motion` (global rule in `styles.css`)

## Components

- Prefer `src/components/ui/*` (shadcn) for new chrome
- Existing `.mc-*` classes remain valid until Phase 5 cleanup
- Icons: Lucide via `src/components/icons.tsx` wrappers (stable export names). No emoji as UI icons
- Interactive elements: `cursor-pointer`, visible focus ring (`ring` / `outline-ring`), no layout-shifting hover scales

## Light mode

Light mode must keep body text ≥ 4.5:1 contrast. Use `--mc-text-primary` / `--mc-text-muted` — do not use faint text for body copy.

## Related

- Implementation plan: phased overhaul (foundation → shell → thread → settings → ops → cleanup)
- Styles entry: `web/src/styles.css`
- Primitives: `web/src/components/ui/`
