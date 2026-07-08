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

### 2026-07-07 — Scheduled tasks "disappeared" and stopped running

- **Symptom:** Web Schedules tab showed no tasks; scheduled jobs did not run for a day. Data was actually intact (193 rows, 14 active).
- **Root cause:** One `scheduled_tasks` row had its `prompt` stored with SQLite **BLOB** affinity (SQLite is dynamically typed; likely a manual `sqlite3` edit — Rust paths only bind `&str`). `row.get::<String>(3)` failed for that row, aborting the **entire** `get_due_tasks` / `get_tasks_for_chat` query every tick. Log signature: `ERROR scheduler: failed to query due tasks: Invalid column type Blob at index: 3, name: prompt`.
- **Fix:** Live data repair `UPDATE scheduled_tasks SET prompt = CAST(prompt AS TEXT) WHERE typeof(prompt)='blob';` (backed up the row first). Scheduler recovered on the next tick and caught up overdue tasks.
- **Prevention:** Made row reads blob-tolerant via `row_text` / `row_text_opt` / `map_scheduled_task_row` in `src/db.rs` so one malformed row can never take down a whole query. When a whole listing/query "returns nothing," check the logs for a per-row deserialization error before assuming data loss. Never hand-edit DB rows in a way that changes column affinity.
- **Files/refs:** `src/db.rs` (`row_text`, `row_text_opt`, `map_scheduled_task_row`), `src/scheduler.rs` (`run_due_tasks`, `get_due_tasks`). Code hardening requires rebuild + reinstall of `~/.local/bin/finally-a-value-bot`.
