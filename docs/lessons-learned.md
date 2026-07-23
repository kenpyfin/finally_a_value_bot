# Lessons Learned

Running log of incidents, root causes, and durable fixes. Newest first. Each entry should let a future reader avoid repeating the same mistake without re-doing the investigation.

Template:

```
### YYYY-MM-DD — Short title
- **Symptom:** what was observed
- **Root cause:** the underlying reason
- **Fix:** what resolved it (data and/or code)
- **Prevention:** how to avoid recurrence
- **Files/refs:** key paths, symbols, log signatures
```

---

### 2026-07-23 — Unstable props defeated ThreadPane React.memo (idle UI jank)

- **Symptom:** Web UI felt laggy while idle (typing/scroll/menus); chat remounted or re-rendered on the 2.5–10s ops poll cadence.
- **Root cause:** `React.memo(ThreadPane)` was load-bearing (journal 2026-04-10) but App passed **new inline callback identities** every render (`onLoadMoreHistory={() => …}`, etc.). Ops poll also called `setPersonas` whenever React Query data identity changed, even when persona fields were unchanged, so App re-rendered often and ThreadPane always followed.
- **Fix:** Pass stable `useCallback` handlers into ThreadPane; hoist `makeMarkdownText(...)` to module scope (avoid remounting markdown types); gate `setPersonas` with `personasSnapshotEqual`; memoize shell chrome; add `/api/ops_poll` + client single-fetch bundle; mild SSE yield throttle (80ms).
- **Prevention:** Never wrap memoized leaf props in fresh lambdas at the parent call site. When adding poll-driven `setState`, compare the fields that actually drive UI before updating. Prefer one combined poll endpoint over N parallel GETs on a short interval.
- **Files/refs:** `web/src/app/App.tsx` (ThreadPane props, SSE flush); `web/src/components/thread-pane.tsx` (`React.memo`, `MarkdownText`); `web/src/hooks/use-ops-poll.ts` (`personasSnapshotEqual`); `src/web.rs` (`api_ops_poll`); plan `web_ui_performance_920afed4`.

### 2026-07-07 — Scheduled tasks "disappeared" and stopped running

- **Symptom:** Web Schedules tab showed no tasks; scheduled jobs did not run for a day. Data was actually intact (193 rows, 14 active).
- **Root cause:** One `scheduled_tasks` row had its `prompt` stored with SQLite **BLOB** affinity (SQLite is dynamically typed; likely a manual `sqlite3` edit — Rust paths only bind `&str`). `row.get::<String>(3)` failed for that row, aborting the **entire** `get_due_tasks` / `get_tasks_for_chat` query every tick. Log signature: `ERROR scheduler: failed to query due tasks: Invalid column type Blob at index: 3, name: prompt`.
- **Fix:** Live data repair `UPDATE scheduled_tasks SET prompt = CAST(prompt AS TEXT) WHERE typeof(prompt)='blob';` (backed up the row first). Scheduler recovered on the next tick and caught up overdue tasks.
- **Prevention:** Made row reads blob-tolerant via `row_text` / `row_text_opt` / `map_scheduled_task_row` in `src/db.rs` so one malformed row can never take down a whole query. When a whole listing/query "returns nothing," check the logs for a per-row deserialization error before assuming data loss. Never hand-edit DB rows in a way that changes column affinity.
- **Files/refs:** `src/db.rs` (`row_text`, `row_text_opt`, `map_scheduled_task_row`), `src/scheduler.rs` (`run_due_tasks`, `get_due_tasks`). Code hardening requires rebuild + reinstall of `~/.local/bin/finally-a-value-bot`.
