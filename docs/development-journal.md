# Development journal

Canonical log of **user-visible**, **architectural**, and **hard-to-infer** changes.
Use **newest entries first**. Prefer specialized docs for durable design detail:
[`lessons-learned.md`](lessons-learned.md) (incidents), [`cursor-engine-integration.md`](cursor-engine-integration.md),
[`hooks-architecture.md`](hooks-architecture.md), [`persona-hook-skill-policy.md`](persona-hook-skill-policy.md),
[`deterministic-workflows.md`](deterministic-workflows.md), [`multimodel-local-tiers.md`](multimodel-local-tiers.md),
[`local-delegate-routing.md`](local-delegate-routing.md), [`memory-framework.md`](memory-framework.md),
[`sops.md`](sops.md), [`agent-harness-research.md`](agent-harness-research.md).

Older eras below are **digests** (titles + pointers). Incident write-ups that already live in
`lessons-learned.md` are not repeated here in full. Condense periodically: keep Recent as full
entries, one-line the previous era, digest older months, and link out instead of duplicating
specialized docs (see `.cursor/rules/development-journal.mdc`).

## Template (copy per new entry)

```markdown
### YYYY-MM-DD — Short title

- **Area:** e.g. channels / scheduler / agent / infra
- **Summary:** What changed in one or two sentences.
- **Rationale:** Why (problem, tradeoff, constraint).
- **Key files / symbols:** Paths and notable functions or types.
- **Follow-ups:** Optional; known gaps or next steps.
```

## Recent

### 2026-09-30 — Personas ordered by latest message

- **Area:** web / persona activity
- **Summary:** Persona lists sort by the newest persisted user or agent message across main chat, sessions, and side chats. Empty personas appear last, with ID breaking timestamp ties. Sends and completed replies refresh activity without waiting for the persona polling throttle.
- **Rationale:** ID order and bot-only activity did not reflect the most recently used conversation. Unread cursors and inbox links continue using bot-only metadata.
- **Key files / symbols:** `src/db.rs` (`list_persona_last_message_at`); `src/web.rs` (`order_personas_by_activity`, both persona response paths, `message_stored` SSE event); web persona API mapping, ops polling, and send callbacks.
- **Follow-ups:** Side chats retain their existing persistence behavior: user and assistant turns are saved together after completion.

### 2026-09-29 — Agent-driven secret vault

- **Area:** agent / safety / channels / web
- **Summary:** When a user message contains a password, login, or API key, the agent calls `vault_secret`. The value is stored in `runtime/secrets/<chat_id>.env` (and optionally a skill `.env`), injected into bash and skill scripts, and replaced in that chat's messages and agent_history with `[SECRET:NAME stored in runtime/secrets/<chat_id>.env]`. `/secrets` and Settings → Overview list names only.
- **Rationale:** Shape matching at ingest misses ordinary passwords. The model sees the value once on the turn it arrives; persisted history after that turn does not.
- **Key files / symbols:** `src/secret_vault.rs` (`upsert`, `drain_pending_scrubs`, `resolve_placeholders`); `vault_secret` tool; `EnvSecretRedactor::register_trusted` / `redact_with_patterns`; `replace_literal_in_chat_messages`; `builtin_secret_vault_scrub`; `GET/DELETE /api/secrets`.
- **Follow-ups:** Rotate keys already pushed to GitHub (`GEMINI_API_KEY`, `CONTRACTORTALK_PASSWORD`, and the local-history tokens listed in the secret-vault plan). Chat platforms still keep the original user message.

### 2026-09-26 — Side chat resize grows with the split; unread cursors are server-side

- **Area:** web UI / side chat / inbox unread
- **Summary:** The side chat pane fills its resizable column, so dragging the split wider grows the pane (it already shrank). Persona unread dots use `persona_read_cursors` on the gateway instead of `localStorage`, so every browser on this bot shares the same read state.
- **Rationale:** `.mc-subthread-pane` was `width: min(420px, 100%)`, which capped growth. Read cursors in `finally-a-value-bot_persona_last_read_v1` never left the browser, so another device did not show the same dots.
- **Key files / symbols:** `.mc-subthread-pane` in `web/src/styles.css`; `persona_read_cursors`, `apply_persona_read_cursors` in `src/db.rs`; `api_personas_read` (`POST /api/personas/read`) in `src/web.rs`; `syncMissingReadCursors` / `markPersonaRead` in `web/src/hooks/use-persona-session.ts`; `last_read_at` on `Persona`.
- **Follow-ups:** None.

### 2026-09-25 — Cursor-based chat history windows for bookmark jumps

- **Area:** web UI / history API / bookmarks
- **Summary:** Replaced tip-counted `historyVisibleLimit` pagination with cursor windows (`before_id` / `after_id` / `around_id` + `has_older` / `has_newer`). Bookmark jumps load a light anchored window around the target; Load earlier / Load newer / Jump to latest navigate without dumping the whole timeline. Poller no longer clobbers anchored views. Bookmark list joins message `session_id` / timestamp / `missing`.
- **Rationale:** Scroll-to-bookmark could not survive chopped history; arming the target before fetch raced a short frame retry; tip refreshes wiped around-message windows.
- **Key files / symbols:** `MessagePage`, `get_messages_before_id`, `get_messages_after_id`, `get_messages_around_id` in `src/db.rs`; `api_history` in `src/web.rs`; `use-chat-history.ts` (`historyMode`, `loadWindowAround`, `jumpToLatest`); `revealMessageInThread` in `App.tsx`; `LoadNewerMessages` / `JumpToLatestPill` / ThreadPane props; cockpit bookmark cards.
- **Follow-ups:** Optional session title on bookmark cards; infinite scroll instead of buttons.

### 2026-09-25 — Fix queue dialog blank page after shadcn migration


- **Area:** web UI / queue / dialogs
- **Summary:** Queue dialog crashed into a blank page when opened. Fixed by always mounting shadcn `DialogContent` (controlled `open` only), removing the broken TanStack `DataTable`/`getRowId` path from the queue view (plain HTML tables for runs + background jobs), and applying the same DialogContent mount fix to other dialogs that used `{open ? <DialogContent/> : null}`.
- **Rationale:** Conditional `DialogContent` under the new Radix Dialog portal left an empty/broken surface; `DataTable` also unwrapped `row.original` incorrectly for TanStack’s `getRowId`, which could throw when rows were present.
- **Key files / symbols:** `web/src/app/dialogs/queue-dialog.tsx`; other `app/dialogs/*-dialog.tsx`; `web/src/components/data-table.tsx` (`getRowId` corrected for future use).
- **Follow-ups:** Reintroduce DataTable for queue once typed against TanStack Table v9 legacy APIs and covered by a smoke test.

### 2026-09-25 — Web UI overhaul follow-ups (CSS prune, props, Streamdown lazy)

- **Area:** web UI / styles / settings / markdown
- **Summary:** Removed leftover Radix Themes `.rt-*` CSS, unused history-loading / persona-delete / sheet-close rules, and obsolete `.mc-sheet.rt-DialogContent` selectors. Fixed invalid migration props (`Separator size`, `Badge size/color`, `Textarea size`, `Button color`). Streamdown loads via `React.lazy` with a plain-text Suspense fallback; mermaid controls disabled.
- **Rationale:** Finish overhaul follow-ups: dead CSS from Themes era, DOM props that only worked on Radix Themes components, and defer the ~370KB Streamdown chunk until a message renders.
- **Key files / symbols:** `web/src/styles.css`; `settings-hooks-skills.tsx` (`StatusBadge`); `session-sidebar.tsx`; `agent-history-dialog.tsx`; `settings-gemini-adk.tsx`; `markdown-stream.tsx`.
- **Follow-ups:** Further `.mc-*` prune as screens move fully to Tailwind utilities; optional `@streamdown/code` if syntax highlighting is desired later.

### 2026-09-24 — Web UI design overhaul (phased, complete)

- **Area:** web UI / design system / shell / thread / settings / ops
- **Summary:** Full phased overhaul: shadcn/ui + token bridge (`--mc-*` ↔ shadcn), Geist/JetBrains Mono, Lucide icons, ⌘K palette, Sonner toasts, resizable thread/subthread, Streamdown + agent blocks, split `app/dialogs/`, DataTable queue, token-driven xterm, removal of `@radix-ui/themes` and `@assistant-ui/react-ui` (local `aui-thread.tsx`). See also same-day entries below for migration slices. Design rules: [`design-system.md`](design-system.md).
- **Rationale:** Operator cockpit needed a maintainable design system without Radix Themes’ locked look and huge CSS chunk.
- **Key files / symbols:** `docs/design-system.md`; `web/src/styles.css`; `web/src/components/ui/*`; `command-palette.tsx`; `aui-thread.tsx`; `markdown-stream.tsx`; `agent-blocks.tsx`; `app/dialogs/*`; `lib/xterm-theme.ts`; `lib/thread-ui-mode.ts`.
- **Follow-ups:** Done 2026-09-25 (CSS prune, invalid props, Streamdown lazy-load).

### 2026-09-23 — Web UI: drop `@assistant-ui/react-ui` (local thread primitives)

- **Area:** web UI / thread
- **Summary:** Replaced `@assistant-ui/react-ui` (`Thread`, `Composer`, messages, `makeMarkdownText`, `BranchPicker`) with `web/src/components/aui-thread.tsx` built on `@assistant-ui/react` + `@assistant-ui/react-markdown` / Streamdown. Layout CSS vendored into `web/src/aui-layout.css`; removed react-ui stylesheet import from `App.tsx`. Uninstalled `@assistant-ui/react-ui` and `@radix-ui/themes`.
- **Rationale:** Remove unused Radix Themes peer and shrink vendor CSS while preserving `.aui-*` hooks, scroll anchoring, and `finally-a-value-bot_thread_ui` legacy vs next markdown paths.
- **Key files / symbols:** `components/aui-thread.tsx`, `components/thread-pane.tsx`, `aui-layout.css`, `vite.config.ts` (`vendor-streamdown` chunk).
- **Follow-ups:** None required; `.rt-*` trim done 2026-09-25.

### 2026-09-23 — Web UI: complete `@radix-ui/themes` → shadcn/ui migration

- **Area:** web UI / App shell / settings / dialogs / pickers
- **Summary:** Removed all `@radix-ui/themes` imports from `web/src` (including `Theme` wrapper and `styles.css` in `App.tsx`). Replaced with `@/components/ui/*` (Button, Dialog, Select, Tabs, Switch, Checkbox, ScrollArea, DropdownMenu, etc.) and Tailwind layout primitives; callouts use bordered status panels.
- **Rationale:** Finish Phase 5 of the shadcn migration; theming stays on `--mc-*` / `data-ui-theme` without Radix Themes runtime.
- **Key files / symbols:** `app/App.tsx`, `app/AppHeader.tsx`, `app/dialogs/*`, `components/settings-*.tsx`, `session-picker.tsx`, `side-chats-picker.tsx`, `inbox-panel.tsx`, `ops-ui.tsx`, `cockpit-bar.tsx`, `session-sidebar.tsx`, `error-banner.tsx`, `bulletin-strip.tsx`, `initial-run-prompt-view.tsx`.
- **Follow-ups:** Invalid Theme props cleaned 2026-09-25.

### 2026-09-23 — Web UI phases 2–4 (thread, dialogs split, shadcn migration start)

- **Area:** web UI / thread / dialogs / settings / terminal
- **Summary:** Thread pane supports `localStorage` flag `finally-a-value-bot_thread_ui` (`legacy` | `next`, default `next`): `next` uses Streamdown markdown and shadcn agent tool cards. `AppDialogs` split into `web/src/app/dialogs/*` with thin orchestrator; confirm/new-persona/auth dialogs on shadcn; queue desktop table uses `@tanstack/react-table` legacy + `DataTable`. Terminal xterm theme reads `--mc-*` tokens.
- **Rationale:** Presentation-only overhaul aligned with shadcn/tokens; keep assistant-ui thread behavior and scroll logic unchanged.
- **Key files / symbols:** `markdown-stream.tsx`, `agent-blocks.tsx`, `thread-ui-mode.ts`, `app/dialogs/*.tsx`, `data-table.tsx`, `xterm-theme.ts`, `settings-runtime.tsx`, `settings-persona.tsx`.
- **Follow-ups:** Phase 5 — migrate remaining `@radix-ui/themes` in settings panels, inbox, session/side-chats pickers, and dialog shells to shadcn Dialog.

### 2026-09-24 — Schedules dialog width and overflow

- **Area:** web UI / dialogs
- **Summary:** Removed the default `sm:max-w-sm` from shared `DialogContent` (it was overriding every dialog’s larger `max-w-[…]` on desktop). Schedules now opens at up to 920px with a viewport max-height, scrollable list, and stacked rows so long prompts/paths wrap instead of stretching the panel sideways.
- **Rationale:** `cn`/`twMerge` cannot drop a conflicting `sm:` variant when the override is unprefixed, so operator dialogs stayed ~384px wide while list rows with shell paths blew past 10k px.
- **Key files / symbols:** `web/src/components/ui/dialog.tsx` `DialogContent`; `web/src/app/dialogs/schedules-dialog.tsx`.
- **Follow-ups:** Other operator dialogs inherit the wider default automatically; spot-check Queue / Settings / Artifacts on a large viewport.

### 2026-09-23 — Cockpit launcher, bulletin stack, bookmark cards

- **Area:** web UI / header / bulletin / cockpit
- **Summary:** Desktop header uses the same **Cockpit** launcher as mobile (icon + label + busy badge) instead of the text status chip. Bulletin strip is a single stacked column (label+title, then preview); the separate Show full button is gone — click the bar to expand. Cockpit bookmarks are larger multi-line cards with a trash control; new bookmark previews store up to 480 characters.
- **Rationale:** Status-chip vs mobile launcher mismatched the same control; side-by-side title/preview wasted horizontal space and duplicated the expand affordance; chip bookmarks truncated away the useful context.
- **Key files / symbols:** `AppHeader` cockpit launch; `BulletinStrip` stacked layout; `CockpitBar` bookmark cards + `onRemoveBookmark`; `truncate_chars(..., 480)` in `api_persona_bookmarks_post`.
- **Follow-ups:** Existing bookmarks keep their shorter stored preview until re-bookmarked.

### 2026-09-23 — Persona sidebar and dialog polish

- **Area:** web UI / sidebar / dialogs
- **Summary:** Persona list no longer scrolls sideways; long names truncate with a tooltip. New persona uses a Radix dialog with duplicate/reserved-name validation instead of `window.prompt`. Integrations bot-instance delete uses `useConfirmDialog` instead of `window.confirm`. Removed the duplicate cockpit launcher from the bulletin strip (the header status chip and mobile header button already open it).
- **Rationale:** Radix Themes sets `.rt-ScrollAreaViewport > * { width: fit-content }`, and the default `scrollbars="both"` let the widest row widen the list; the name's `truncate` sat inside an unconstrained `inline-flex`. Native prompt/confirm dialogs broke theme and focus handling.
- **Key files / symbols:** `.mc-persona-list` in `styles.css`; `SessionSidebar`; `NewPersonaDialog`; `use-persona-session.onCreatePersona(name)`; `settings-integrations.removeBotInstance`.
- **Follow-ups:** None.

### 2026-09-23 — Persona rename/delete live in Settings

- **Area:** web UI / settings / personas
- **Summary:** Sidebar persona rows are select-only (plus **+ New**). Rename and delete for the active persona live under **Settings → Overview → Active persona**, backed by `POST /api/personas/rename` and the existing delete API. The reserved `default` persona cannot be renamed or deleted.
- **Rationale:** Destructive/edit controls next to every sidebar row were easy to miss or tap by accident after the hover-reveal pass; Settings already owns persona-scoped configuration.
- **Key files / symbols:** `Database::rename_persona`; `api_personas_rename`; `SettingsPersonaPanel`; `use-persona-session.onRenamePersona`; sidebar delete removed from `SessionSidebar`.
- **Follow-ups:** None.

### 2026-09-23 — Cockpit / bulletin UI makeover

- **Area:** web UI / cockpit / settings / mobile
- **Summary:** Bulletin focus is always visible in an in-flow strip under the header. The cockpit is a slim session-status panel (memo + bookmarks + queue) opened from the strip on desktop or a bottom sheet on mobile. Chat context depth and dense delivery moved to Settings → Agent engine (“Run context”). Side-chat “Add to main chat”, persona delete, and sidebar toggle got interaction polish.
- **Rationale:** The old floating cockpit hid the bulletin, buried persona knobs under session status, and reflowed/jittered on mobile scroll. Settings already owned persona-scoped engine config.
- **Key files / symbols:** `BulletinStrip`, `MobileSheet`, slimmed `CockpitBar`, `SettingsRunContextPanel`; `App.tsx` strip wiring; `IconSidebar`; `.mc-bulletin-strip` / `.mc-sheet` / `.mc-subthread-promote` / `.mc-persona-delete` in `styles.css`.
- **Follow-ups:** Optional desktop keyboard shortcut to open cockpit; verify Radix Dialog bottom-sheet animation on older iOS Safari.

### 2026-09-21 — Promote side-chat assistant reply to main timeline

- **Area:** web UI / side chat
- **Summary:** Side-pane assistant turns can be deposited into the main timeline as a new bot message (no agent run) via **Add to main chat**; UI refreshes history and scrolls to the inserted message.
- **Rationale:** Side chats stay out of the main record by design; operators needed a deliberate way to keep a useful side-chat answer in the session timeline.
- **Key files / symbols:** `Database::get_side_chat_turn`; `POST /api/personas/:persona_id/side_chats/:side_chat_id/promote_turn` (`api_persona_side_chat_promote_turn`); `SubthreadSidePane` `onAddToMainChat`; `App.handleAddSideChatTurnToMain`.
- **Follow-ups:** None.

### 2026-09-15 — Classic strategy LLM is per-persona

- **Area:** settings / agent / llm
- **Summary:** Settings → Agent engine → LLM now loads and saves provider/model (and local base URL) for the **current persona**. Classic runs resolve an ephemeral strategy client from persona overrides so concurrent personas do not stomp the global `LlmHandle`. Thinking toggles and cost-routing local URL/model stay shared.
- **Rationale:** Agent engine and Cursor SDK model were already persona-scoped; classic LLM looked persona-scoped in the UI but wrote global `app_settings`. `personas.model_override` existed but was unused at runtime.
- **Key files / symbols:** `personas.llm_provider_override` / `llm_base_url_override` + `model_override`; `Database::set_persona_llm_selection`; `LlmHandle::resolve_strategy_selection` / `send_message_for_tier_with_strategy`; `GET|PATCH /api/llm` `persona_id`; `SettingsLlmPanel` `activePersonaId`.
- **Follow-ups:** Optional clear-to-inherit control in the UI; per-persona cost-routing local models if needed.

### 2026-09-15 — Context-first file locator (declared roots before walk)

- **Area:** tools / path resolution / agent prompt
- **Summary:** Added a declared-root registry and `locate_file` tool so agents resolve against persona ORIGIN, shared ORIGIN, skills, runtime, Tier-1 `Repo:` paths, and SOP pointers before any filesystem walk. Fixed persona-vs-shared `ORIGIN/` resolution and “permission denied” for missing in-scope files. Blocked name-filtered `find` outside declared roots and pointed the model at `locate_file` + ask-user instead of widening.
- **Rationale:** Uninformed `find` over `/home/ken/big_storage` hung chat after a false permission error; locations were already in memory/config.
- **Key files / symbols:** `src/tools/locate.rs` (`build_registry`, `locate_in_registry`, `LocateFileTool`); `resolve_prefixed_path` / `absolutize_for_compare` / `assert_persona_tool_path_allowed_with_memory` in `src/tools/mod.rs`; `is_expensive_shell_search_scoped` in `src/tools/bash_safety.rs`; locate roots section in `build_system_prompt`.
- **Follow-ups:** Optionally index vault paths for content search; migrate persona ORIGIN trees into shared ORIGIN when desired.

### 2026-09-14 — Kill Cursor MCP tool process trees on cancel/timeout

- **Area:** cursor / tools / safety
- **Summary:** Cursor run cancel and bash timeouts now terminate the full subprocess tree (process group / Job Object). MCP runs expose a cancel flag to in-flight `tools/call`s; broad `find $HOME -name …` is blocked without `CONFIRM_EXECUTE`.
- **Rationale:** Sidecar reapers cancelled SDK sessions while gateway-owned `find`/`sleep` children kept running, so the engine looked dead and load stayed high.
- **Key files / symbols:** `run_managed_command`, `KillOnDropGroup`, `CursorMcpRegistry::cancel_and_drain`, `scope_tool_cancel`, `find_targets_broad_root`.
- **Follow-ups:** Rebuild/restart gateway (`./reload.sh`). Optionally kill any leftover pre-fix `find /home/...` orphans once.

### 2026-09-12 — Persona-explicit agent engine; Cursor model per persona

- **Area:** agent / settings / web
- **Summary:** Removed “Inherit default” from Agent engine settings. Every persona stores an explicit engine (NULL/empty backfilled to Classic). Cursor SDK model and model parameters are stored on the persona and overlaid at run time / in settings when `persona_id` is passed.
- **Rationale:** Operators should see and edit what this persona actually uses, not a global inherit indirection. Model choice is persona-scoped; sidecar/MCP/timeouts stay shared.
- **Key files / symbols:** `migrate_persona_explicit_engine_and_cursor_model`, `set_persona_cursor_sdk_model`, `apply_persona_cursor_model_override`, `resolve_run_agent_engine`, `GET/PATCH /api/cursor-engine?persona_id=`, `settings-agent-engine.tsx`, `settings-cursor.tsx`.
- **Follow-ups:** Optional UI to clear persona model back to global default without typing empty.

### 2026-09-11 — Gemini ADK engine replaces Deterministic pipeline

- **Area:** agent / settings / web
- **Summary:** Removed the Deterministic pipeline (`src/agent_pipeline/`). Added a native in-process Gemini ADK multi-agent engine (`src/gemini_adk/`) with per-persona topology editor. Agent engine settings UI now reveals settings for the selected engine (no preview dropdown). New personas default to Single turn (`classic`).
- **Rationale:** User wanted ADK-style multi-agent orchestration integrated into the bot (no sidecar/Python), and a simpler settings page where choosing an engine is choosing its settings.
- **Key files / symbols:** `AgentEngine::GeminiAdk`, `run_gemini_adk_engine`, `AdkTopologyProfile`, `/api/gemini-adk`, `/api/gemini-adk/topology`, `settings-agent-engine.tsx`, `settings-gemini-adk.tsx`, `create_persona` default override.
- **Follow-ups:** Optional parallel child execution with isolated tool scratch; richer allowed_tools UI picker.

### 2026-09-09 — Web UI npm audit: Vite 6 / Vitest 4

- **Area:** web UI / deps
- **Summary:** Patched `web/` npm advisories without `npm audit fix --force`. Transitive fixes first, then Vite `5.4` → `6.4.3` and Vitest `3.2` → `4.1.11` (`@vitest/coverage-v8` matched). `npm audit` is now clean and `vite build` succeeds.
- **Rationale:** `reload.sh` prints the audit warning after `npm install`; it is not a failed build. `--force` would have jumped to Vite 8, which `@tailwindcss/vite` and `@vitejs/plugin-react` do not support.
- **Key files / symbols:** `web/package.json` (`vite`, `vitest`, `@vitest/coverage-v8`); `web/package-lock.json`.
- **Follow-ups:** Rebuild/restart gateway if you want the Vite 6 `web/dist` embedded (`./reload.sh`).

### 2026-09-09 — Load-earlier keeps the thread still

- **Area:** web UI / history
- **Summary:** “Load earlier messages” freezes a clone of the current viewport on click, remounts history underneath, restores scroll, then lifts the clone after paint so there is no flash to the top. Side-chat jumps skip the freeze.
- **Rationale:** `runtime.thread.reset()` after a prepend fired initialize-scroll-to-bottom, so height-delta restore lost the race and the viewport jumped. Jumping must keep its own path.
- **Key files / symbols:** `captureMessageScrollAnchor` / `restoreMessageScrollAnchor` in `web/src/lib/scroll-anchor.ts`; prepend pin + `scrollToBottomOnInitialize={false}` in `web/src/components/thread-pane.tsx`.
- **Follow-ups:** Rebuild/restart gateway so `web/dist` ships (`./reload.sh`).

### 2026-09-09 — Mobile cockpit is a header button

- **Area:** web UI / header
- **Summary:** On phone widths the cockpit is a labeled header chip (with a queue/jobs badge), separated from Inbox/Settings/Ops. Session status was removed from the Operator tools sheet.
- **Rationale:** Opening the cockpit required the ops sheet after the floating pill auto-hides; operators need a persistent, dedicated control.
- **Key files / symbols:** `.mc-cockpit-launch` in `web/src/styles.css`; mobile launch button in `web/src/app/AppHeader.tsx`; `handleToggleCockpit` in `web/src/app/App.tsx`; `IconCockpit`.
- **Follow-ups:** Rebuild/restart gateway so `web/dist` ships (`./reload.sh`).

### 2026-09-09 — Hide duplicate session controls on desktop

- **Area:** web UI / header
- **Summary:** The mobile session sub-bar (Main chat / + Session / Side chats) no longer stays visible next to the desktop header cluster. It is `display: none` by default and `display: grid` only below 768px.
- **Rationale:** Unlayered `.mc-mobile-session-subbar { display: grid }` beat Tailwind `md:hidden` (utilities layer), so both copies rendered on `md+`.
- **Key files / symbols:** `.mc-mobile-session-subbar` in `web/src/styles.css`; `renderSessionControls` in `web/src/app/AppHeader.tsx`.
- **Follow-ups:** Rebuild/restart gateway so `web/dist` ships (`./reload.sh`).

### 2026-09-05 — Operator UI: header controls, cockpit float, mobile scroll, bookmark jump

- **Area:** web UI / header / thread
- **Summary:** Session and Side-chat controls are now visually separated (segmented session pill + divider + icon/badge side-chats button), with a dedicated mobile two-tier header (title/actions on top, a collapsible session sub-bar below). The floating CockpitBar pill now auto-hides when the thread is scrolled away from the top or a side chat is open, and can be opened from the header status chip or the mobile ops sheet (Session status). Mobile header height is fixed (no per-scroll resize) and collapse uses accumulated-delta hysteresis plus a post-toggle guard to stop the jitter/oscillation. Bookmark chips now reliably center the target message instead of snapping to the bottom.
- **Rationale:** The two picker groups read as one undifferentiated button string and crowded on mobile; the floating cockpit hovered over content and the side-chat split; in-flow header resizing during momentum scroll cancelled the fling and oscillated; and assistant-ui's auto-scroll-to-bottom (on `thread.initialize` / resize) clobbered the bookmark `scrollIntoView`, so jumps landed at the bottom of the session.
- **Key files / symbols:** `renderSessionControls` + two-tier layout in `web/src/app/AppHeader.tsx`; `mc-session-controls` / `mc-session-picker` (segmented) / `mc-side-chats-btn` / `mc-badge-counter` / `mc-mobile-session-subbar` in `web/src/styles.css`; `IconSideChat`+badge in `side-chats-picker.tsx`; `onThreadScrolledDownChange`, accumulated-delta scroll handler, and `targetScrollMessageId` centering effect + `ThreadPrimitive.Viewport` with `autoScroll`/`scrollToBottomOnInitialize`/`scrollToBottomOnThreadSwitch` in `web/src/components/thread-pane.tsx`; `centerMessageInViewport` / `flashMessageElement` in `web/src/lib/reveal-message.ts`; `threadScrolledDown` state, floating-pill visibility gate, and rewritten `revealMessageInThread` / `handleTargetScrollHandled` in `web/src/app/App.tsx`; `onOpenSessionStatus` in `ops-ui.tsx`.
- **Follow-ups:** Rebuild/restart gateway so `web/dist` ships (`./reload.sh`).

### 2026-09-04 — Persist side chats; keep history scroll; jump bookmarks

- **Area:** web UI / history
- **Summary:** Side chats are stored in SQLite (`side_chats` / `side_chat_turns`) and listed in a header registry; closing the pane no longer drops the conversation. “Load earlier messages” keeps the viewport still (scroll restore no longer re-captures after DOM growth). Bookmark chips jump to the message in the main thread instead of opening a reader dialog.
- **Rationale:** Ephemeral side chats were lost on close; history prepend scrolled to the oldest row; bookmarks opened a separate dialog instead of in-thread navigation.
- **Key files / symbols:** `ensure_side_chat` / `append_side_chat_turns` / `get_messages_around_id` in `src/db.rs`; `api_persona_side_chats_*` / `api_subthread_stream` / history `around_id` in `src/web.rs`; `useSideChats`, `SideChatsPicker`, `revealMessageInThread` / `ensureMessageVisible`; scroll restore in `thread-pane.tsx`.
- **Follow-ups:** Rebuild/restart gateway so `web/dist` is compiled in (`./reload.sh`).

### 2026-09-04 — Stuck Cursor runs cancel only the old session

- **Area:** cursor sidecar / supervisor
- **Summary:** When `/health` shows a `/run` older than 20 minutes, the supervisor now `POST /admin/cancel_old_runs` instead of killing the sidecar process. Younger Cursor sessions stay. The sidecar reaper uses the same wall-age cap (`CURSOR_MAX_RUN_WALL_SECS`, default 1200). Unreachable `/health` (`wedged_health`) still force-recycles because individual sessions cannot be addressed.
- **Rationale:** Process recycle was dropping live chats (e.g. sourdough) as collateral for hung PZ3/videographer slots.
- **Key files / symbols:** `cancelRunsOlderThan` / `handleCancelOldRuns` in `scripts/cursor-sdk-runner.mjs`; `request_cancel_old_runs` / `supervise_sidecar` in `src/cursor_sdk_sidecar.rs`.
- **Follow-ups:** Recycle sidecar (script mtime) and rebuild/restart gateway (`./reload.sh`).

### 2026-09-04 — Cursor stream-drop copy asks for a resend

- **Area:** cursor engine / delivery
- **Summary:** Idle/timeout interrupt notices no longer mention Comfy or `check again`. They ask the user to send the request again. Background-job copy is unchanged (do not resend while work is still running).
- **Rationale:** The PZ3 Comfy status line was global, so sourdough (and any other Cursor persona) saw GPU-queue advice after a sidecar drop.
- **Key files / symbols:** `cursor_turn_timeout_notice` / `cursor_stream_interrupt_notice` in `src/cursor_engine_config.rs`.
- **Follow-ups:** Rebuild/restart the gateway (`./reload.sh`). `check again` still skips the sidecar as a status poll; it is just no longer advertised on every drop.

### 2026-09-03 — PTE asks on doubt instead of continuing

- **Area:** agent / PTE
- **Summary:** Classic post-tool evaluator no longer defaults to another tool round when unsure. The prompt now emits `ask_user` with a clarification question; delivery uses that question instead of the stall “retry/wait/adjust” template (stall fast-path still uses the stall copy).
- **Rationale:** “If in doubt, continue” kept Classic looping (e.g. WeCom `mailcode_` forwards with no fetch tool) up to `max_tool_iterations`.
- **Key files / symbols:** `build_pte_system_prompt` / `format_pte_ask_user_reply` in `src/post_tool_evaluator.rs`; `PteAction::AskUser` in `src/channels/telegram.rs`.
- **Follow-ups:** Rebuild/restart the gateway (`./reload.sh`). Evaluator infra failures still fail-open to `continue` so a dead PTE host does not halt every turn.

### 2026-09-03 — Agent engine selection actually saves

- **Area:** web UI / settings
- **Summary:** Settings → Agent engine pills save immediately with a confirmation, Inherit + global default are back, and the preview dropdown no longer looks like the engine picker. Sidecar health no longer blocks the save; LLM/Cursor Selects no longer crash the tab.
- **Rationale:** Operators could not tell whether an engine click persisted, and a hung sidecar health probe left the pills disabled.
- **Key files / symbols:** `patchPersonaEngine` / `patchGlobalEngine` in `web/src/components/settings-agent-engine.tsx`.
- **Follow-ups:** Rebuild/restart the gateway so `web/dist` is compiled in (`./reload.sh` when no Cursor turn is in flight).

### 2026-09-03 — Bot response edit + ephemeral side chat

- **Area:** web UI / agent context
- **Summary:** Assistant messages can be edited in place (`PATCH /api/personas/:id/messages/:mid`) and saved to SQLite so later turns see the revised text. A Side chat action opens an ephemeral docked pane (`POST /api/subthread_stream`) seeded with the anchor reply plus the cockpit history window; side-chat turns are not written to main history.
- **Rationale:** Operators needed to correct bot text without regenerating, and to ask focused follow-ups about one reply without polluting the main timeline.
- **Key files / symbols:** `update_message_content` / `get_messages_up_to` in `src/db.rs`; `AgentRequestContext.history_override`; `api_persona_message_patch` / `api_subthread_stream` in `src/web.rs`; `SubthreadSidePane`; edit/side-chat actions in `web/src/components/thread-pane.tsx`.
- **Follow-ups:** Persist side chats later if operators want reopen; optional regenerate-from-edit.

### 2026-09-03 — PTE/PDQE are Classic and Deterministic only

- **Area:** agent quality gates / cursor
- **Summary:** Cursor finish no longer runs PDQE (records `quality_eval_skipped` / `cursor_engine`). The Cursor-only fail-open retry branch is removed. PTE remains Classic-only (tool loop). Deterministic keeps PDQE on `pipeline_finish_turn`.
- **Rationale:** Cursor already has no classic tool loop for PTE; waiting on local PDQE after a sidecar reply only delayed delivery and could hang finish.
- **Key files / symbols:** `finish_turn_with_quality_gate` in `src/channels/telegram.rs`; `docs/cursor-engine-integration.md`.

### 2026-09-03 — Cursor FullSlim keeps live chat when packing the prompt

- **Area:** cursor engine / continuity
- **Summary:** Each Cursor message is still a fresh `Agent.create` session. FullSlim now injects `[continuation_context]` (quoted reply if present, else last interactive prior pair, skipping scheduler/shell dumps) and, when over the 120k cap, drops oldest `prior_turn` history instead of truncating the end where `[current_request]` lives.
- **Rationale:** Short follow-ups like `V6` were treated as standalone tasks (or latched onto a scheduled dump). Resume/bridges stay off.
- **Key files / symbols:** `attach_full_slim_continuation` / `flatten_preserving_recent` in `src/cursor_delegation_prompt.rs`; `parse_quoted_message_block` / `extract_session_goal` in `src/agent_turn_context.rs`.

### 2026-09-01 — Cursor: no resume store; fresh session per message

- **Area:** cursor engine / sidecar
- **Summary:** Removed the sidecar resume store (`workspace/runtime/cursor-sdk-state/`, `Agent.resume`, `cursor_engine_agents` on the run path). Each user message is `Agent.create` with an ephemeral Jsonl store that is deleted when the reply is generated and the `/run` HTTP response ends. Gateway always sends FullSlim (persona + bounded chat). Empty/PreStop nudges are a new `/run` with FullSlim plus a follow-up, not a resumed agent. Settings resume-delta toggle is gone; the flag is forced off.
- **Key files / symbols:** `openAgent` / `ephemeralStoreDir` / `pruneLegacyResumeStores` in `scripts/cursor-sdk-runner.mjs`; `run_cursor_engine` in `src/cursor_engine.rs`; `select_delegation_prompt_mode` always `FullSlim`.
- **Note:** Recycle sidecar after runner edits; rebuild/restart gateway (`./reload.sh`). Startup prunes leftover `cursor-sdk-state` and `cursor-sdk-ephemeral` dirs.

### 2026-09-01 — PZ3 status matching and skip PDQE on interrupt

- **Area:** cursor engine / PDQE
- **Summary:** Status recheck now matches short health/status lines (`Check what's the status?`, `what happened to you…`), lists newest persona images (24h, then any), and skips PDQE on status/interrupt so a dead local eval host cannot add ~35s. Exact-phrase-only matching had still launched 15-minute empty sidecar turns.
- **Key files / symbols:** `is_status_recheck_request`, `collect_persona_images` in `src/cursor_engine.rs`; `should_skip_pdqe` in `src/response_quality_evaluator.rs`.
- **Note:** `./reload.sh`. Does not raise the 900s interactive budget or fix sidecar heap OOM.

### 2026-08-31 — Cursor “check again” is status-only, not a new generation

- **Area:** cursor engine / delivery
- **Summary:** Short status polls (`check again`, `what happened`, …) no longer start a Cursor sidecar / Comfy job. They list recent persona images and say the queue is idle. Stream-interrupt copy no longer says “try a narrower request”; it tells the user `check again` only summarizes disk. Interrupt and status-check turns now write agent history.
- **Key files / symbols:** `is_status_recheck_request`, `format_status_recheck_reply`, `finish_cursor_engine_turn`, `deliver_cursor_interrupt` in `src/cursor_engine.rs`; `cursor_stream_interrupt_notice` in `src/cursor_engine_config.rs`.
- **Note:** Rebuild/restart gateway (`./reload.sh`). The 15-minute interactive HTTP budget is unchanged; the first long turn can still lose the live stream, but the follow-up is now a cheap delivery.

### 2026-08-31 — Cursor history engine line; no silent Classic/Gemini fallback

- **Area:** cursor engine / agent history / PDQE
- **Summary:** Agent history now always prints `Engine: …`. Cursor runs label the sidecar (`cursor_sdk` / model / runner URL) instead of the classic Gemini strategy. Recoverable sidecar errors return a notice instead of Classic fallback (interactive, scheduled, and background). PDQE fail on Cursor fail-opens instead of spinning the same text. History UI shows an engine badge. Cursor finish uses real conversational intent.
- **Key files / symbols:** `AgentRunRecord::to_markdown`; `LocalDelegateRunSummary::for_cursor_sidecar`; `cursor_engine_unavailable_result`; `finish_turn_with_quality_gate` cursor fail-open; `parseEngineLine` / `AgentHistoryEngineBadge`.
- **Note:** Rebuild/restart gateway (`./reload.sh`). Existing history markdown is not rewritten.

### 2026-08-31 — Sidecar stream drop no longer fails an in-flight background job

- **Area:** cursor engine / background jobs / web delivery
- **Summary:** Interactive Cursor HTTP timeouts were surfacing as `error decoding response body` and becoming `failed_turn_notice` (“send your request again”) even after `spawn_background_command` / tracked jobs were already running. Stream body failures now return an interrupt notice; if unfinished jobs exist for that chat+persona, the user is told work is still running and not to resend.
- **Key files / symbols:** `consume_sidecar_stream`, `cursor_interrupt_result`, `compose_cursor_interrupt_reply` in `src/cursor_engine.rs`; `cursor_stream_interrupt_notice` in `src/cursor_engine_config.rs`; `has_unfinished_background_jobs_for_chat_persona` in `src/db.rs`.
- **Note:** Rebuild/restart gateway (`./reload.sh`). The LTX wait job that was running at incident time is not cancelled by this change.

### 2026-08-30 — Cursor engine latency: idle watchdog + interactive timeout

- **Area:** cursor engine / sidecar / settings
- **Summary:** Sidecar cancels SDK streams after `CURSOR_STREAM_IDLE_TIMEOUT_MS` (default 15m), tracks active `/run`s for reaper reconciliation, reserves one concurrency slot for interactive chat, and exposes `oldest_run_age_secs` on `/health`. Rust uses `interactive_timeout_secs` (default 900s) for chat vs `timeout_secs` for scheduled/background; supervisor force-recycles when runs stay old. Settings → Cursor adds interactive timeout.
- **Key files / symbols:** `streamAgentTurn`, `tryBeginRun`, `cancelStuckRuns` in `scripts/cursor-sdk-runner.mjs`; `cursor_turn_timeout_secs` in `src/cursor_engine_config.rs`; `supervise_sidecar` stuck-run recycle in `src/cursor_sdk_sidecar.rs`; `settings-cursor.tsx`.
- **Note:** Recycle sidecar after runner edits; rebuild/restart gateway for Rust changes (`./reload.sh`).

### 2026-08-27 — Background command notices follow the focused session

- **Area:** background_shell / focused sessions / delivery
- **Summary:** `spawn_background_command` start/finish/cancel notices (and the agent follow-up after the job) now store on the focused web session that spawned them, not always main chat. `session_id` is on `ToolAuthContext` and persisted on `background_jobs`.
- **Rationale:** Operators running long shell work from a focused session never saw the “Background command started…” ack in that thread (`deliver_to_contact` used `session_id: None`).
- **Key files / symbols:** `ToolAuthContext.session_id`; `create_background_shell_job` / `create_background_job`; `try_enqueue_background_shell`; `deliver_shell_notification`; `try_enqueue_background_handoff`; `spawn_background_job`.
- **Note:** Restart the gateway so the SQLite `background_jobs.session_id` migration runs. Telegram/Discord/WeCom inbound stays main-chat (`session_id: None`).

### 2026-08-26 — Mixed license: proprietary edits, MIT original

- **Area:** licensing
- **Summary:** Replaced the dual-copyright MIT file with a mixed license. kenpyfin modifications and new files are proprietary source-available (personal non-commercial run/modify only). Original everettjf portions stay MIT. Cargo.toml now uses `license-file` instead of `license = "MIT"`.
- **Rationale:** MIT cannot be removed from the upstream code, but new work does not have to be MIT. The combined tree is no longer permissively licensed.
- **Key files / symbols:** `LICENSE` Parts 1–2; `Cargo.toml` `license-file`; `README.md` License section.

### 2026-08-26 — Dual copyright on MIT license

- **Area:** licensing
- **Summary:** LICENSE now lists kenpyfin (2026) alongside the original everettjf (2025) MIT copyright, with a short derivative-work note. README License section matches.
- **Rationale:** This tree started from everettjf's MIT-licensed project and now contains substantial original work; MIT requires keeping the upstream notice while adding the current author's copyright.
- **Key files / symbols:** `LICENSE`; `README.md` License section.
- **Note:** License type remains MIT (`Cargo.toml` `license = "MIT"`).

### 2026-08-27 — Steel health path + unmanaged wait blocked Web UI

- **Area:** Steel browser bootstrap / gateway startup
- **Summary:** Gateway waited ~90s (wrong `/api/health` 404) before binding Web UI even when `BROWSER_MANAGED` was off. Probe now tries `/v1/health` then `/api/health`; health wait only runs when managed mode is enabled.
- **Key files / symbols:** `probe_steel_health`, `bootstrap` in `src/steel_browser_sidecar.rs`; doctor check in `src/doctor.rs`.

### 2026-08-27 — Wider assistant bubbles; flat user messages

- **Area:** Web UI / thread pane
- **Summary:** `--aui-thread-max-width` is `100%` of the chat column (was vendor `42rem` / our `72rem`). Assistant bubbles fill that width; user messages are flat right-aligned text (no muted bubble) with higher-specificity overrides against `@assistant-ui` defaults.
- **Key files / symbols:** `.aui-assistant-message-content`, `.aui-user-message-content` in `web/src/styles.css`.
- **Note:** Rebuild `web/dist` and restart gateway (assets are embedded). Hard-refresh the browser.

### 2026-08-26 — Cursor duplicated stream + SDK result in one reply

- **Area:** Cursor sidecar / delivery coalesce
- **Summary:** Influencer_PZ_3 summary reply stored the same write-up twice (broken token stream, then clean markdown). Sidecar now delivers SDK `wait().result` alone when available; Rust dedupes repeated sections if both still arrive.
- **Key files / symbols:** `streamAgentTurn` in `scripts/cursor-sdk-runner.mjs`; `coalesce_cursor_delivery_text` / `dedupe_cursor_delivery_text` in `src/cursor_engine.rs`.
- **Note:** Recycle sidecar + restart gateway. Existing duplicated DB rows are not auto-fixed.

### 2026-08-26 — Web chat message formatting (bubbles, tables, actions)

- **Area:** Web UI / thread pane
- **Summary:** Improved chat readability: rounded grouped bubbles on mobile and desktop (consecutive same-sender messages visually link via shared corner radii and tighter spacing), capped assistant line width (~42rem), and restored assistant bubbles on mobile instead of flat text. Markdown tables use a shared wrapper with sticky headers, cell wrapping, and horizontal scroll. Message actions are a hover-reveal icon pill (copy/reply/bookmark/delete) with fixed clipboard copy (API + execCommand fallback) and full reply text on copy.
- **Key files / symbols:** `thread-pane.tsx`, `styles.css`, `markdown-table.tsx`, `copy-to-clipboard.ts`, `message-group.ts`, `reply-quote.ts`.
- **Note:** Rebuild `web/dist`.

### 2026-08-26 — Cursor word-per-line replies (fragment stream coalescing)

- **Area:** Cursor engine / sidecar delivery
- **Summary:** Cursor SDK sometimes emits many tiny `text` events (one token/word each). Rust and the sidecar treated each as a separate utterance and joined with `\n\n`, producing unreadable one-word-per-line chat. Short fragments now append in-place (space-aware, including token continuations like `hot`+`ify`), `done.result` with per-word newlines is repaired, and full progress sentences still get paragraph breaks.
- **Key files / symbols:** `cursor_text_event_is_stream_fragment`, `join_cursor_stream_fragments`, `coalesce_cursor_delivery_text` in `src/cursor_engine.rs`; `pushCursorTextPart`, `joinCursorUtterances` in `scripts/cursor-sdk-runner.mjs`.
- **Note:** Recycle sidecar (script mtime) and restart gateway.

### 2026-08-26 — Web agent failures left user messages unanswered

- **Area:** Web delivery / Cursor engine
- **Summary:** A stored web user message could finish with no bot row when Cursor (or classic LLM) returned `Err` or empty text. Stream showed a transient error, history poll then looked like the bot ignored the user. After the user message is stored, web always delivers a visible notice on failure/empty; empty `deliver_agent_final` Skip now stores `EMPTY_TURN_NOTICE` instead of dropping the turn. The chat UI reloads history on stream `error` as well as `done`.
- **Key files / symbols:** `send_and_store_response_with_events` in `src/web.rs`; `failed_turn_notice` / `ensure_visible_turn_text` in `src/final_delivery_dedupe.rs`; `AgentFinalDeliveryPlan::Skip` in `src/channel.rs`; stream `error` in `web/src/app/App.tsx`.
- **Note:** Restart gateway and rebuild `web/dist`. Residual risk: a hard process kill after `store_message(user)` and before the notice still orphans a turn (queue hard-abort already covers timeout/panic).

### 2026-08-26 — Cursor follow-up failed with "already has active run"

- **Area:** Cursor SDK sidecar
- **Summary:** Sending a new chat message reused a local Cursor agent whose previous run was still marked active (stream ended / wait timed out / cancel incomplete). `agent.send()` then threw `Agent … already has active run`. Sidecar now passes `local.force` on every send to expire the leftover run, retries busy errors after dropping the pooled handle, and skips idle-reaping in-use slots. Rust treats the same error like a stale agent id (clear + full-slim retry).
- **Key files / symbols:** `buildSendOptions` / `isBusyAgentError` / `streamRun` in `scripts/cursor-sdk-runner.mjs`; `_is_busy_agent_error` in `scripts/cursor-sdk-runner.py`; `is_busy_cursor_agent_error` in `src/cursor_engine.rs`.
- **Note:** Recycle sidecar (script mtime). Rebuild gateway for the Rust fallback.

### 2026-08-26 — Cap Cursor run.wait so sidecar recycle cannot stall

- **Area:** Cursor SDK sidecar
- **Summary:** Joining `wait().result` after the stream could hang forever, keeping `runs_in_flight > 0` so idle recycle (`POST /admin/request_recycle`, max uptime, script mtime) never fired and pooled handles stayed warm. `run.wait()` is now a **post-stream hang watchdog** (`CURSOR_RUN_WAIT_TIMEOUT_MS`, default 120s): timeout cancels the SDK run, delivers streamed text, and evicts the pooled agent/bridge **after** the slot lock is released. This does not limit tool rounds or long replies (those stay in the unbounded stream loop). Cancelled disconnects also evict. Node default runner does not spawn `cursor-sdk-bridge.js`; Python rollback uses the same wait bound.
- **Key files / symbols:** `waitForRunResult` / `streamAgentTurn` in `scripts/cursor-sdk-runner.mjs`; `_wait_sdk_run` in `scripts/cursor-sdk-runner.py`.
- **Note:** Recycle sidecar (script mtime). Optional `CURSOR_RUN_WAIT_TIMEOUT_MS`.

### 2026-08-26 — Cursor replies were glued progress fragments (sourdough)

- **Area:** Cursor engine / delivery
- **Summary:** Sourdough (persona 26, dense delivery **off**) stored mid-turn Cursor status lines as the chat reply: `process.The ratio screenshot is in.` The SDK emits one assistant `text` event per tool round; the runner/Rust concatenated them with no break and ignored `wait().result` once any text existed. Delivery now treats each `text` event as an utterance (`\n\n`), and persists the last real write-up when it looks like a final answer.
- **Key files / symbols:** `coalesce_cursor_delivery_text` / `consume_sidecar_stream` in `src/cursor_engine.rs`; `joinCursorUtterances` in `scripts/cursor-sdk-runner.mjs`.
- **Note:** Recycle sidecar (script mtime). Not dense delivery.

### 2026-08-26 — Dense delivery: public PDF link after catbox 403

- **Area:** hooks / delivery
- **Summary:** Catbox-only uploads with the default reqwest User-Agent were 403ing, so replies had no link (or an internal path). Uploader now chains catbox → litterbox → tmpfiles → pixeldrain with a browser UA, parses messy JSON/HTML responses, and rejects localhost/`/api/uploads/`. The chat summary no longer owns the URL: a verified public https:// link is always appended as the last line.
- **Key files / symbols:** `PublicHostUploader`, `extract_public_https_url`, `finalize_delivery_message` in `src/dense_delivery_guard.rs`.
- **Note:** Restart gateway. Leave `DELIVERY_UPLOAD_PROVIDER` unset or `catbox` (not `none`). Optional `CATBOX_USERHASH`.

### 2026-08-26 — Dense delivery: natural LLM summary + public PDF

- **Area:** hooks / delivery
- **Summary:** Dense-delivery chat text is no longer a rigid “title — summary / bullets / character count” template. A second LLM call (hook/orchestrator/main model; extractive fallback) writes a natural reply and offers the public PDF URL. The PDF is the **full** original report (tables and other distinctive payload preserved; env secrets redacted) rendered then uploaded to catbox (retry, pandoc fallback after md2pdf). User-facing text never offers an internal/workspace path.
- **Key files / symbols:** `maybe_apply_dense_delivery_with` / `LlmDeliverySummarizer` / `upload_public_with_retry` in `src/dense_delivery_guard.rs`; `builtin_dense_delivery_guard` in `hook_runtime.rs`.
- **Note:** Restart gateway. Keep `DELIVERY_UPLOAD_PROVIDER=catbox` (not `none`) so the reply can include an external HTTPS link.

### 2026-08-26 — Node sidecar polyfills global Web Crypto on Node 18

- **Area:** Cursor SDK sidecar
- **Summary:** Cursor turns failed with `crypto is not defined`. `@cursor/sdk` calls global `crypto.randomUUID()`; systemd's `/usr/bin/node` 18.19 does not define that global for `.mjs` files. Runner now assigns `node:crypto.webcrypto` to `globalThis.crypto` before loading the SDK.
- **Key files / symbols:** `scripts/cursor-sdk-runner.mjs` (webcrypto polyfill; self-test `globalThis.crypto.randomUUID`).
- **Note:** Recycle sidecar (script mtime / idle recycle). No Node upgrade required. Complements JsonlLocalAgentStore for Node < 22.13.

### 2026-08-26 — Node sidecar uses JsonlLocalAgentStore on Node 20

- **Area:** Cursor SDK sidecar
- **Summary:** `@cursor/sdk` local runtime defaults to `node:sqlite`, which needs Node >= 22.13. Host is Node 20, so `Agent.create`/`resume` failed with that error. Sidecar now passes `local.store = new JsonlLocalAgentStore(dir)` per persona/session under `{runtime}/cursor-sdk-state/`.
- **Key files / symbols:** `localStoreFor` / `agentStoreRoot` in `scripts/cursor-sdk-runner.mjs`.
- **Note:** Recycle sidecar (script mtime) so the runner reloads. No Node upgrade required.

### 2026-08-25 — Agent engine settings apply to the active persona only

- **Area:** Settings / personas
- **Summary:** Settings → Agent engine no longer lists every persona. The original engine-button UX is back, scoped to the sidebar’s current persona (name shown on the tab). Other personas keep their stored override or inherit the global default.
- **Key files / symbols:** `settings-agent-engine.tsx`; PATCH `/api/personas/:id/bulletin` `agent_engine_override`.
- **Note:** Rebuild `web/dist`. Switch persona in the sidebar, then open Settings to change that persona.

### 2026-08-25 — Node Cursor SDK sidecar (no cursor-sdk-bridge)

- **Area:** Cursor SDK sidecar / Settings
- **Summary:** Default Cursor engine sidecar is now Node `scripts/cursor-sdk-runner.mjs` using `@cursor/sdk` in-process. Settings **Refresh models** (`GET /models` → `Cursor.models.list`) and turns no longer spawn Python's `cursor-sdk-bridge.js`, which was the source of connection-fail / bridge-discovery errors. Loopback MCP and persona cwd are unchanged. Python runner remains for rollback via `CURSOR_SDK_RUNNER_SCRIPT`. Supervisor npm-installs into `{runtime}/cursor-sdk-node` and dropped orphan-bridge recycle.
- **Key files / symbols:** `scripts/cursor-sdk-runner.mjs`; `ensure_sidecar_node_prefix` / `resolve_sidecar_script` in `src/cursor_sdk_sidecar.rs`; `CURSOR_SDK_NODE`.
- **Note:** Recycle/rebuild gateway. Need Node 20+ and npm on PATH. Optional rollback: `CURSOR_SDK_RUNNER_SCRIPT=scripts/cursor-sdk-runner.py`. Rebuild `web/dist` for Settings copy.

### 2026-08-25 — Per-persona agent engine in Settings

- **Area:** Settings / personas / cockpit
- **Summary:** Agent engine is no longer chosen in the cockpit. Settings → Agent engine lists every persona with its own engine (or inherit). The previous global picker is the inherit default only. Runtime still resolves `personas.agent_engine_override` at `process_with_agent_with_events`. `GET /api/personas` now includes `agent_engine_override` / `agent_engine_effective`.
- **Key files / symbols:** `settings-agent-engine.tsx`; `api_personas` in `src/web.rs`; cockpit engine UI removed from `cockpit-bar.tsx`.
- **Note:** Rebuild `web/dist`. Existing persona overrides keep working.

### 2026-08-25 — Live model catalog for Agent engine (Classic LLM)

- **Area:** Settings / LLM catalog
- **Summary:** Agent engine → Single turn (Classic) now loads live model ids from the provider API (`GET /api/llm/models`) instead of only the curated in-code list. OpenAI-compatible `/models` (with Bearer), Anthropic `/v1/models`, and Gemini model list; local Ollama/llama.cpp uses the existing `/v1/models` probe. Curated rows stay for cost hints and as a fail-open fallback. PATCH `/api/llm` accepts live ids (no longer requires `custom=true`). Azure/Bedrock/custom stay curated-only. Refresh models in `settings-llm.tsx`.
- **Key files / symbols:** `fetch_live_provider_models` in `src/llm.rs`; `merge_live_model_ids` in `src/llm_catalog.rs`; `api_llm_models_get` in `src/web.rs`; `SettingsLlmPanel`.
- **Note:** Restart gateway. Rebuild `web/dist` for Settings. Keys remain in `.env` only.

### 2026-08-25 — Dense Delivery Guard + per-persona agent engine

- **Area:** hooks / delivery / personas / Settings
- **Summary:** Persona-gated dense delivery spills over-limit replies (2k messaging / 1k web) to markdown+PDF, uploads to catbox, and replaces the delivered text with a summary + public HTTPS URL. Runs as PreDelivery **after PDQE** so quality eval still sees the full report. Cockpit toggle (seeded on for persona 28). Per-persona `agent_engine_override` is resolved at `process_with_agent_with_events` (NULL/invalid inherit global). Settings folds LLM / Local delegate / Cursor / Deterministic into one **Agent engine** tab.
- **Key files / symbols:** `dense_delivery_guard.rs`; `builtin_dense_delivery_guard`; `run_pre_delivery_hooks_after_gate` in `telegram.rs`; `resolve_run_agent_engine`; `personas.dense_delivery_*` / `agent_engine_override`; cockpit bulletin PATCH; `settings-agent-engine.tsx`.
- **Note:** Restart gateway so migrate adds columns and seeds persona 28. Rebuild `web/dist` for Settings/cockpit. Optional: `DELIVERY_UPLOAD_PROVIDER=catbox|none`.

### 2026-08-25 — Fix ops_poll UTF-8 panic (cockpit queue blank)

- **Area:** web / cockpit ops poll
- **Summary:** Cockpit queue went blank because `/api/ops_poll` panicked when truncating a background job `result_text` at byte 200 inside a multi-byte em dash. Queue data was fine (`/api/queue_diagnostics` still worked). Fixed with `floor_char_boundary`.
- **Key files / symbols:** `background_job_result_preview` / `json_background_job` in `src/web.rs`.
- **Note:** Restart gateway to pick up.

### 2026-08-25 — Cursor sidecar idle-safe auto-recycle

- **Area:** Cursor SDK sidecar / deploy
- **Summary:** Sidecar exposes idle-only `POST /admin/request_recycle` plus uptime fields on `/health`. Rust supervisor soft-recycles on max uptime / script mtime / orphan bridge pressure when `runs_in_flight==0`, and force-recycles after two wedged health probes. Bootstrap no longer attaches forever to a stale runner. `reload.sh` soft-drains then force-kills sidecar + bridges before gateway restart (`scripts/recycle-cursor-sidecar.sh`).
- **Key files / symbols:** `handle_request_recycle` in `scripts/cursor-sdk-runner.py`; `supervise_sidecar` / `SidecarHandle` in `src/cursor_sdk_sidecar.rs`; `reload.sh`.
- **Note:** Recycle/rebuild gateway to pick up. Optional: `CURSOR_SIDECAR_MAX_UPTIME_SECS` (default 86400).

### 2026-08-25 — Remove unfinished PreDelivery char-limit PDF guard

- **Area:** hooks / delivery
- **Summary:** Stripped the incomplete global PreDelivery PDF-spill attempt (module was never committed; wiring left the tree broken). Removed `PreDelivery` event, `updated_assistant_text`, `builtin_char_limit_pdf_guard`, `DELIVERY_CHAR_LIMIT_*` config, and docs/UI catalog entries. Migrate deletes any seeded `predelivery-char-limit-pdf-guard` row. Finish path goes straight to PostDelivery again. Prep for a future persona-gated dense-delivery rebuild.
- **Key files / symbols:** `hook_runtime.rs`, `pipeline_finish_turn` in `telegram.rs`, `ensure_builtin_hook_definitions` in `db.rs`.
- **Note:** Restart gateway so migrate clears the orphaned hook row if still present.

### 2026-08-24 — Cursor sidecar: offload sync turns + cancel-safe disconnect

- **Area:** Cursor SDK sidecar / cursor engine
- **Summary:** Long sync `agent.send`/`messages`/`wait` no longer blocks the aiohttp loop (queue-from-thread). Disconnect requests `run.cancel()` instead of evicting the bridge under `pooled.lock`. Ping runs outside `_POOL_GUARD`. `/run` concurrency capped (`CURSOR_RUN_CONCURRENCY`, default 4). Rust stream cancel polls every 250ms; MCP tokens revoke on Drop if `finish_run` did not take them.
- **Key files / symbols:** `scripts/cursor-sdk-runner.py` (`_stream_agent_turn_async`, `_ActiveTurn`, `_try_begin_run`); `McpTokenGuard` / `consume_sidecar_stream` in `src/cursor_engine.rs`.
- **Note:** Recycle `cursor-sdk-runner.py` after deploy. Optional: `CURSOR_RUN_CONCURRENCY`.

### 2026-08-21 — Always-finish turns + Cursor bridge orphan sweeper

- **Area:** chat queue / PDQE / Cursor SDK sidecar
- **Summary:** Stuck interactive turns (PZ / selling_oversea / Videographer) started tools then never wrote `run_finished` or a bot reply. Hardened: (1) queue hard-timeout/panic now invokes `on_hard_abort` to store a user-visible notice + `run_finished` (web also publishes run_hub `error`); (2) PDQE has an outer wall timeout (evaluator timeout + 30s) fail-open; focus sync is wall-timed at 120s; `run_finished` is awaited before return; (3) sidecar idle TTL default 600s / pool max 16, reaper kills untracked `cursor-sdk-bridge.js` PIDs, and client disconnect during `/run` evicts the active pool key.
- **Key files / symbols:** `src/queue_abort.rs`; `QueueHardAbortHook` in `src/chat_queue.rs`; finish path in `src/channels/telegram.rs`; `scripts/cursor-sdk-runner.py` (`_kill_orphan_bridge_processes`, handle_run eviction).
- **Note:** Restart gateway + recycle `cursor-sdk-runner.py`. Optionally set `CURSOR_BRIDGE_IDLE_TTL_SECS` / `CURSOR_BRIDGE_POOL_MAX` / `CURSOR_BRIDGE_ORPHAN_GRACE_SECS`. Kill any pre-existing orphan bridge PIDs once after deploy.

### 2026-08-20 — Shared processing ack on all external channels

- **Area:** channels / Telegram / Discord / WhatsApp / WeCom
- **Summary:** All external channels now show `Processing…` when a queued inbound run starts. Telegram seeds the editable status message (then tool updates / delete); Discord sends then deletes; WhatsApp sends a short interim; WeCom AI Bot stream placeholder uses the same string; WeCom callback sends a proactive interim.
- **Key files / symbols:** `CHANNEL_PROCESSING_ACK` in `src/channels/mod.rs`; wired in `telegram.rs`, `discord.rs`, `whatsapp.rs`, `wecom.rs`, `wecom_aibot.rs`.
- **Note:** Restart gateway. Web UI keeps its own loading state (no chat spam).

### 2026-08-20 — WeCom long-connection: immediate stream ack + delayed send_msg

- **Area:** channels / WeCom AI Bot
- **Summary:** After a long Cursor turn (incl. empty-output placeholder), WeCom stopped delivering group callbacks. Root cause: replies used late `aibot_respond_msg` without opening a stream within WeCom’s callback window. Now: `begin_stream_reply` sends stream `finish=false` (“处理中…”) before the agent run; final reply finishes the stream within 9 minutes or falls back to `aibot_send_msg`. Heartbeat pong watchdog reconnects half-open sockets.
- **Key files / symbols:** `begin_stream_reply`, `respond_stream_frame`, `PendingReply` in `src/channels/wecom_aibot.rs`.
- **Note:** Restart the gateway. After restart, @ the bot in the group; you should see “处理中…” then the final answer.

### 2026-08-20 — Cursor empty-output nudge for WeCom/channels

- **Area:** cursor engine / WeCom
- **Summary:** Resume turns sometimes finished with no assistant text; the gateway delivered `(Cursor agent completed with no text output.)` to WeCom. Now: nudge up to `DEFERRED_COMMITMENT_MAX_NUDGES` for a user-facing reply when empty and resume id exists; else recover short prose from tool results; placeholder only as last resort. Sidecar also accepts broader assistant message shapes and prefers `wait().result` when stream text is empty.
- **Key files / symbols:** `empty_output_nudge_prompt`, `recover_user_text_from_tool_records` in `src/cursor_engine.rs`; `_stream_agent_turn` in `scripts/cursor-sdk-runner.py`.
- **Note:** Restart gateway + recycle `cursor-sdk-runner.py` so both pick up the change.

### 2026-08-19 — WeCom group chat: re-bind all handles to operator inbox

- **Area:** channels / WeCom
- **Summary:** Extra WeCom groups were routed to hashed canonical contacts (`resolve_wecom_canonical_chat_id` refused a second handle on the inbox). Those groups missed Settings → Channels Single persona (`selling_oversea`) and showed inbound messages with zero bot replies. All handles now re-link to the operator inbox on each inbound; directional `platform_reply` still replies only to the sender’s handle. Direct `send_text` fallback if unified delivery fails.
- **Key files / symbols:** `resolve_wecom_canonical_chat_id`, WeCom agent delivery in `src/channels/wecom.rs`.
- **Note:** Restart gateway. @ the bot in the group; stale hashed bindings migrate on the next inbound message.

### 2026-08-19 — Directional channel replies (WeCom included)

- **Area:** channels / delivery
- **Summary:** External integrations (Telegram, Discord, WhatsApp, WeCom) now reply only on the inbound binding handle via `DeliveryScope::platform_reply`. Web main chat and focused sessions use `StoreOnly` (history in UI, no fan-out). Scheduler/background delivery is web-only. WeCom multi-group on one inbox no longer broadcasts replies to every linked handle.
- **Key files / symbols:** `DeliveryScope` / `platform_reply` in `src/channel.rs`; inbound handlers in `src/channels/{telegram,discord,whatsapp,wecom}.rs`; web send in `src/web.rs`.
- **Note:** Restart the gateway. Rebuild `web/dist` if using embedded UI.

### 2026-08-18 — Only main chat syncs with Channels

- **Area:** channels / WeCom / web sessions
- **Summary:** Channels persona routing and outbound fan-out belong to the operator inbox main chat. Focused sessions store web-only (`DeliveryScope::StoreOnly`). WeCom links at most one handle to that inbox (first inbound, or `UNIVERSAL_CHAT_ID`); extra WeCom groups/DMs keep hashed contacts so they do not inherit the inbox policy.
- **Key files / symbols:** `deliver_to_contact_with_origin` in `src/channel.rs`; `resolve_wecom_canonical_chat_id` in `src/channels/wecom.rs`; `lookup_canonical_chat_id` in `src/db.rs`.
- **Note:** Restart the gateway. Existing extra WeCom handles already linked to `997894126` stay until unlinked.

### 2026-08-18 — Deprecate focused-session archive

- **Area:** web UI / chat sessions / scheduler
- **Summary:** Removed focused-session archive and the 15-minute TTL auto-archive sweep. Sessions stay until the operator deletes them. Startup reopens any previously archived rows. PATCH `status=archived` returns 410; list no longer hides rows; create default `ttl_hours` is 0. Session picker drops archive/restore and can delete the current focused session. `/archive` (markdown dump of the conversation) is unchanged.
- **Key files / symbols:** `migrate_chat_sessions_schema` / `list_chat_sessions` in `src/db.rs`; `api_chat_sessions_list` / `api_chat_sessions_patch` in `src/web.rs`; TTL loop removed from `spawn_scheduler` in `src/scheduler.rs`; `SessionPicker` in `web/src/components/session-picker.tsx`; `handleDeleteSession` in `web/src/hooks/use-persona-session.ts`.
- **Note:** Rebuild `web/dist` and restart the gateway. Previously archived sessions reappear in the picker after restart.

### 2026-08-18 — WeCom honors Settings → Channels persona policy

- **Area:** channels / WeCom / personas
- **Summary:** Settings → Channels is saved on the web inbox (`997894126`), but WeCom inbound hashed its own canonical chat and `get_or_create_default_persona` always ran there. Single-persona locks never matched (`persona_exists` is per chat). WeCom now links into `Config::operator_inbox_chat_id()` so All/Single policy and the selected persona apply without restart.
- **Key files / symbols:** `ingest_wecom_incoming` in `src/channels/wecom.rs`; `operator_inbox_chat_id` / `DEFAULT_UNIVERSAL_CHAT_ID` in `src/config.rs`.
- **Note:** Restart once to pick up the bind. Then change Channels in the web UI; WeCom uses that contact’s policy on the next message.

### 2026-08-18 — WeCom inbound frames were dropped silently

- **Area:** channels / WeCom
- **Summary:** AI Bot websocket subscribed but inbound traffic produced no logs or replies. Parser required string `from.userid` + `chattype=="group"` and returned without logging; Integrations allowlist `selling_oversea` (group name, not chatid) would also drop group messages. Now log every `aibot_*` frame, accept numeric chattype/userid, keep the connection on a bad JSON frame, and warn on allowlist/persona/mention drops.
- **Key files / symbols:** `handle_msg_callback`, `parse_chattype`, `json_text` in `src/channels/wecom_aibot.rs`; ingest warnings in `src/channels/wecom.rs`.
- **Note:** Restart gateway. In a group the user must @ the bot. If allowlist is set, copy `chatid=` from `WeCom inbound message` logs into Integrations (empty = all).

### 2026-08-18 — Integrations allowlists apply without restart

- **Area:** channels / WeCom / Telegram / integrations
- **Summary:** Settings → Integrations allowlists now apply on save. WeCom was still using the list copied into the dispatcher at boot, so changing allowed chats did nothing until restart (and a group *name* never matched WeCom’s `chatid`). Inbound WeCom now reloads `wecom_allowed_chats` from the bot instance row, matches `chat:`/`user:` prefixes case-insensitively, and logs the real id when a message is dropped. Telegram/Discord already read from DB; invalid numeric IDs now return 400 instead of being silently dropped.
- **Key files / symbols:** `load_wecom_allowed_chats` / `chat_allowed` in `src/channels/wecom.rs`; `handle_msg_callback` in `src/channels/wecom_aibot.rs`; `parse_id_list_i64` in `src/web.rs`; Integrations labels in `web/src/components/settings-integrations.tsx`.
- **Note:** Rebuild `web/dist` and restart once so the binary picks up the live-reload code. After that, allowlist edits apply immediately. WeCom allowlist must be the callback `chatid`/`userid` (check logs `dropped inbound: not in Integrations allowed chats` for the real id), not the group display name.

### 2026-08-18 — WeCom AI Bot long-connection adapter

- **Area:** channels / WeCom / integrations
- **Summary:** Added the 智能机器人 WebSocket adapter (`wss://openws.work.weixin.qq.com`) as the preferred WeCom path. Platform remains `wecom` / instance id 4. When Bot ID + Secret are set (mode `aibot`, or Bot ID present), the gateway subscribes with `aibot_subscribe`, heartbeats `ping` every 30s, ingests `aibot_msg_callback` (groups already @-filtered), replies with `aibot_respond_msg` / fanout `aibot_send_msg`, and decrypts media via per-file `aeskey`. Self-built app HTTPS `/callback` stays as fallback. One live connection per bot (new subscribe kicks the old one). Settings → Integrations: Connection = AI Bot long connection vs callback; token field is the long-connection Secret.
- **Key files / symbols:** `src/channels/wecom_aibot.rs` (`WecomAiBotClient`, `start_wecom_aibot`); `WecomGateway` in `src/channels/wecom.rs`; `Config::wecom_uses_aibot`; DB `wecom_aibot_id` / `wecom_mode`; `WecomExtraFields` in `web/src/components/settings-integrations.tsx`.
- **Note:** Restart after saving. No SWAG `/callback` needed for AI Bot. Rebuild `web/dist` so embedded UI includes the Connection selector. Official Account (公众号) and 客户联系 remain out of scope.

### 2026-08-18 — WeCom Integrations add form fields

- **Area:** web UI / WeCom
- **Summary:** Settings → Integrations “Add Bot Instance” only showed label + corp secret for WeCom. Corp ID, Agent ID, callback token, EncodingAESKey, port, and allowed chats now appear on create (and with labels on the saved instance card). POST `/api/channel_bot_instances` already accepted those fields.
- **Key files / symbols:** `WecomExtraFields` in `web/src/components/settings-integrations.tsx`.
- **Note:** Rebuild `web/dist` and restart the gateway so embedded assets pick this up.

### 2026-08-17 — WeCom (企业微信) channel

- **Area:** channels / WeCom / integrations
- **Summary:** Added a WeCom self-built app channel (`wecom`) with encrypted callback verification (WXBizMsgCrypt), async agent turns, and outbound `message/send` / `appchat/send` through `deliver_to_contact` so the unified inbox fans out. Single primary instance id `4`; string handles `user:{userid}` / `chat:{chatid}` with hashed canonical ids when `UNIVERSAL_CHAT_ID` is unset. Settings → Integrations form plus Channels All/Single (not WhatsApp-locked). Dedicated callback port default 8081 (`GET|POST /callback`); HTTPS reverse proxy required by WeCom.
- **Key files / symbols:** `src/channels/wecom.rs`, `src/channels/wecom_crypt.rs`, `WecomClient` on `AppState`; `BOT_INSTANCE_WECOM_PRIMARY` + `wecom_*` columns in `src/db.rs`; `deliver_to_contact` wecom arm in `src/channel.rs`; Integrations UI in `web/src/components/settings-integrations.tsx`.
- **Note:** Restart after saving WeCom credentials. Official Account (公众号) and 客户联系 are out of scope.

## 2026-07 → mid-August (condensed)

One line each. Cursor MCP/hooks/sidecar detail: [`cursor-engine-integration.md`](cursor-engine-integration.md).

- **2026-08-12 — Cursor sidecar: close ephemeral bridges (EMFILE fix):** Sidecar hit `OSError: [Errno 24] Too many open files` (1023/1024 FDs) after ~340 orphan `cursor-sdk-bridge` processes accumulated.
- **2026-07-30 — Web UI: remove redundant toolbar Queue button:** Removed the desktop header **Queue** button.
- **2026-07-24 — Fix Cursor MCP tool discovery (protocol version):** Cursor SDK live tool discovery for `finally-a-value-bot` failed because `initialize` returned non-existent protocol `2025-11-05`.
- **2026-07-23 — Web UI: mobile inbox icon, chat switch loading, inbox→session, scroll-to-latest:** Mobile header Inbox is icon-only (badge kept) so session controls are not crowded.
- **2026-07-23 — Ban Cursor/agent from self-repo; allow Tier-1 target repos:** Prevent Cursor SDK, `cursor_agent`, and agent shells from treating the finally-a-value-bot checkout as a project repo (git discovery from persona cwd under `WORKSPACE_DIR`).
- **2026-07-23 — Web UI performance: ThreadPane isolation, poll churn, bundle split, ops_poll:** Fixed idle jank from ops/history polling defeating chat isolation.
- **2026-07-22 — Web persistent auth + Inbox (unread + agent todos):** Web auth token now persists in `localStorage` (with one-time migrate from `sessionStorage`) so closing the browser no longer forces re-entry.
- **2026-07-15 — Integrations tab unified around bot instances:** Settings → Integrations no longer shows separate primary Telegram/Discord/WhatsApp forms plus a duplicate all-bots list.
- **2026-07-14 — Channel integrations configured in Web UI (not .env):** Telegram, Discord, and WhatsApp tokens plus platform options (`BOT_USERNAME`, allowlists, WhatsApp phone/verify/port, control chats) are now configured in Settings → Integration…
- **2026-07-12 — Resume-delta continuation context + PostDelivery focus sync hardening:** Sourdough main-chat incident: resume-delta sent only `[current_request]` (~1.8k chars) so a generic "commit and push to dev" lost the prior sourdough `index.html` fix; PostDeliv…
- **2026-07-10 — Cursor sidecar: global bridge launch queue + scheduled Classic fallback:** Scheduled tasks (#6 vault index, etc.) failed with `Timed out waiting for bridge discovery` when multiple crons claimed together and each cold-started a `cursor-sdk-bridge`.
- **2026-07-10 — Web file links: materialize `file://` + Cursor delivery reminder:** PEP GN session showed Cursor engine emitting `file://` markdown links and fabricated `/api/uploads/...` paths — both broken in the web UI.
- **2026-07-09 — Web image display: backtick-wrapped persona filenames:** History review for persona 24 (`Influencer_PZ_3`) showed repeated “show me the image” turns where the assistant named `PZ-….png` in backticks or prose but the web thread had no …
- **2026-07-08 — Cursor engine: session-scoped bridge pool per persona:** Bridge pool is now keyed by **persona cwd + session_scope** (not persona alone).
- **2026-07-08 — Cursor engine: bridge crash recovery + session cleanup:** `Bridge request failed: ConnectError: [Errno 111] Connection refused` came from the Cursor SDK's internal `cursor-sdk-bridge` subprocess dying while the Python sidecar kept a st…
- **2026-07-08 — Web upload could hang the composer forever:** Uploading a file could leave the app stuck on `Uploading…` indefinitely and lock the composer.
- **2026-07-07 — Scheduler resilience: blob-typed prompt broke all task queries:** All scheduled tasks stopped running and the web Schedules tab showed empty.
- **2026-07-07 — Cursor engine: context deduplication for sidecar delegation:** Cursor-only prompt shaping reduces duplicated context sent to the SDK sidecar: when MCP is live, strip the long `## Tool groups` catalog from the delegation system prompt (schem…
- **2026-07-06 — Docs: Cursor engine integration (tools, skills, hooks):** Added [`docs/cursor-engine-integration.md`](cursor-engine-integration.md) — how ToolRegistry, skills, and bot-native hooks link to Cursor via loopback MCP (not `.cursor/*` sync).
- **2026-07-03 — Cursor engine: MCP tool bridge + full hook parity:** Cursor SDK runs now expose the bot `ToolRegistry` via loopback MCP (`POST /internal/cursor-mcp`) with run-scoped Bearer tokens.
- **2026-07-02 — Web UI interactive terminal (PTY + WebSocket):** Added an optional interactive browser terminal: `POST /api/terminal/sessions` (Bearer auth + short-lived `ws_ticket`) and `GET /api/terminal/ws` (auth JSON, then binary PTY I/O).
- **2026-07-01 — Cursor engine: bot-native hook bridge (Phase 1):** Cursor engine now runs bot `BeforeTurn` and `PreStop` hooks via shared `hook_turn_bridge` (block, `[hook_context]` injection, deferred-commitment nudge retries with resumed `age…
- **2026-07-01 — Hooks/skills settings: enriched catalog + persona filter:** Settings → Hooks & Skills catalogs show richer metadata and persona-scoped filtering (**Show all personas**).
- **2026-07-01 — Web chat: message copy + text selection:** Added explicit **Copy** actions on user and assistant message rows (replacing the hidden assistant-ui action bar copy).
- **2026-07-01 — Web reply bubbles: snippet-only quote display:** Sent reply messages no longer show the full `[quoted_message]` body in the user bubble.
- **2026-07-01 — PDQE runs on ask_clarification replies:** Clarification turns are user-visible; removed `ask_clarification` from `should_skip_pdqe` and dropped the auto-pass fast path so PDQE evaluates them like other deliveries.


## Archive digests (2026-03 → 2026-06)

Full day-by-day write-ups for this period were collapsed. Use the docs linked above plus git history when you need archaeology.

### 2026-06 — Engines, sessions, PDQE, multimodel, channels

- **Agent engines:** Classic / Deterministic / Cursor selectable; Cursor SDK local sidecar + Web settings; scheduled Cursor without silent Classic fallback (later policy evolved — see recent + lessons-learned). Research: [`agent-harness-research.md`](agent-harness-research.md). Integration: [`cursor-engine-integration.md`](cursor-engine-integration.md).
- **Deterministic pipeline:** 4-phase Web-configurable runtime (intent → plan → execute → consolidate), skill/SOP binding, prior-step handoff, per-phase context toggles, cloud-rich intent/plan vs local step contracts. See [`deterministic-workflows.md`](deterministic-workflows.md), [`sops.md`](sops.md).
- **Classic cost routing:** Replaced phase-based multimodel with cost routing + local executor; silent-stop fix after local read-only turns. See [`local-delegate-routing.md`](local-delegate-routing.md), [`multimodel-local-tiers.md`](multimodel-local-tiers.md).
- **Multimodel / LLM UI:** Local llama.cpp tiers + strategy API; tool_choice/probe/fallback; provider/model moved to Web UI (keys stay in `.env`); Grok/OpenAI hardening; TSA and listing-only routing gates removed.
- **PDQE / PTE:** Pre-delivery quality gate; Perplexity + local-first evaluators; failure surfaced to users; observability in Last agent run; Learn & optimize from history.
- **Focused sessions:** Web-only per-persona sessions (`chat_sessions` / `messages.session_id`); optional main-chat mirroring; UX polish (create/select, scroll preserve, reply quote chip).
- **Channels / delivery:** Multi-bot Telegram/Discord Channels tab; image URL repair; cross-channel image normalization; per-persona queue lanes; message dates in UI + LLM context.
- **Memory / workflows:** Tier 2 `sops[]` replaces `known_steps`; YAML learned-workflow engine rolled back to vault SOPs; authored workflows → prompt catalog then deprecated path cleaned.
- **Safety:** Env-only secret redaction (skip non-secret keys); strip internal dialogue XML and bulletin_focus from history; omit assistant bookmarks from persona context; task-first context.

### 2026-05 — Hooks, bulletin, shell, path discipline

- **Hooks:** Deterministic hook runtime; shipped `builtin_hooks/*.hook.json`; persona-scoped definitions + skills-style availability UI; command/prompt executors; PostDelivery focus sync; observability in run history. Canonical: [`hooks-architecture.md`](hooks-architecture.md), [`persona-hook-skill-policy.md`](persona-hook-skill-policy.md).
- **Bulletin / memory:** Bulletin-first persona context; Tier 2 knowledge schema; identity + Tier 1 in system prompt; memory modes / vault path / skills catalog; frontmatter-only skill discovery.
- **Background shell:** tmux jobs, monitor, failure notify + auto-retry, success agent follow-up, tracked external job ids, workdir normalization, shadow workspace path guards.
- **Path / stall guards:** Skill path remapping for bash/glob/hooks; grep limited to workspace root; deferred-commitment + discovery-loop guards; long tool timeouts (1h).
- **Redaction / delivery:** User-visible vs internal secret split; preserve long media/checkpoint basenames; scheduler failure redaction; fuzzy final vs `send_message` dedupe.
- **Web:** Mobile shell overhaul; LLM model picker; last-run first-turn prompt viewer; history continuity settings.

### 2026-04 — Cockpit, queue, projects, CI foundations

- **Web cockpit:** Responsive shell, settings tabs, TanStack ops poll, schedules/background visibility, Last agent run modal, ThreadPane memo isolation, history pagination, composer remount guards, uploads under `shared/upload`.
- **Queue / heartbeats:** Per-chat FIFO agent queue; shared job heartbeat for background + scheduled; stale reconciliation; run timeline events.
- **Runtime models (later trimmed):** Global projects/workflows learning landed then partially rolled back toward vault SOPs (see June). Gap tracking: [`runtime-gap-analysis.md`](runtime-gap-analysis.md). Deprecated SQLite workflows: [`workflow.md`](workflow.md).
- **Persona / channels:** Requester `[PersonaName]` prefix; run-scoped tool defaults; multi-instance Telegram/Discord; web-first settings/onboarding.
- **Memory hygiene:** Tiered write hardening; loop guards; AGENTS.md Memory Hygiene clause. See [`memory-framework.md`](memory-framework.md).
- **Infra / docs:** Clippy/CI polish, Dependabot, README/`.env.example` refresh, documentation-reference Cursor rule.

### 2026-03 — Delivery and memory baselines

- Persona indicator `[Name]` on all outbound messages; duplicate-final / scheduled-repeat delivery fixes; markdown tables in web chat; chat-scoped background queue; long-run timeouts raised to 1500s; Memory Hygiene & Structural Integrity in `workspace/AGENTS.md`.
