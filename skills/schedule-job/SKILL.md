---
name: schedule-job
description: Safely plan and validate scheduled jobs with explicit UTC handling before calling schedule_task.
license: MIT
compatibility:
  os:
    - darwin
    - linux
    - windows
  deps:
    - python3
---

# Schedule Job

Use this skill whenever a user asks to create, update, or reason about a scheduled job.

## Non-negotiable policy

1. Always run this skill preflight before calling `schedule_task`.
2. Cron is normalized to **6 fields**: `sec min hour dom month dow`.
3. If timezone is unknown, assume **UTC** and say that explicitly to the user.
4. If user provides a timezone, include it in the `schedule_task.timezone` field.
5. For one-time jobs, use an ISO 8601 timestamp. Prefer explicit offset (for example `+07:00`).

## Scheduling preflight checklist

Before calling `schedule_task`, collect and confirm:

- the task prompt (`prompt`)
- schedule type (`cron` or `once`)
- schedule expression or timestamp (`schedule_value`)
- timezone context (`timezone`, or default to UTC with explicit notice)

Then:

- normalize cron from 5 fields to 6 fields by prepending `0`
- validate cron field count is exactly 6
- validate one-time timestamp is valid ISO 8601

## Helper script

Use the bundled script to normalize and validate schedule input:

```bash
python3 skills/schedule-job/schedule_helper.py cron "*/15 * * * *" --timezone "Asia/Bangkok"
python3 skills/schedule-job/schedule_helper.py once "2026-03-14T09:30:00" --timezone "Asia/Bangkok"
```

If no timezone is known:

```bash
python3 skills/schedule-job/schedule_helper.py cron "0 9 * * *"
```

The script prints JSON with normalized output and explicit timezone assumptions.

## Mapping to `schedule_task`

- `schedule_type="cron"`:
  - `schedule_value`: normalized 6-field cron
  - `timezone`: user timezone if known, otherwise `"UTC"`
- `schedule_type="once"`:
  - `schedule_value`: ISO 8601 timestamp (include offset when available)
  - `timezone`: optional; include `"UTC"` when timezone was unknown and defaulted

## Examples

- User: "Remind me every weekday at 9am"
  - If timezone unknown: ask to confirm default UTC phrasing in response and schedule with UTC.
  - Cron example: `0 0 9 * * 1-5`

- User: "Run this once on 2026-04-01 14:00 in Jakarta"
  - Convert/format as ISO 8601 with offset: `2026-04-01T14:00:00+07:00`
  - Use `schedule_type="once"` and `timezone="Asia/Jakarta"`.
