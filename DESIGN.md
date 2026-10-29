# gray-life — design

A personal tracker the agent keeps for one person: todos, habits, the things
going on in their life, and a daily log. One store per user, shared by every
chat and the REPL. Built after reading OpenClaw (2026-09-01, `aec22ecc`) and
Hermes Agent (2026-10-28, `f2cc0131`); what was ported is cited inline.

## Why not gray-todo / gray-kanban / gray-tasks

Those are per project (keyed by cwd). Each Discord conversation runs in its
own `GRAY_HOME`, so a per-cwd list is invisible from the next chat. gray-life
stores under `$GRAY_USER_HOME/life/` (else `~/.gray/life/`), which every
conversation home shares.

## Store

```
life/
  life.json        todos, events, habits (one file, flock'd read-modify-write)
  settings.json    name, timezone, quiet hours, check-in job ids, onboarding state
  log/YYYY-MM-DD.md  the daily log: "- HH:MM text" lines
```

- **todo** `t<N>`: text, optional `due` (date or local datetime), done/cancelled.
- **event** `e<N>` — "what's going on": text, `at` (date or datetime), optional
  `end`. This is the calendar: the agent's own record, no Google sync.
- **habit** `h<N>`: name, schedule (`daily`, `weekdays`, `weekends`,
  `mon,wed,fri`, `3/week`), a log of done dates, streak computed on read.
- Items are **hidden in shared channels** unless marked `shared: true`.
  Ported from OpenClaw AGENTS.md: "MEMORY.md — load only in the main session…
  never in shared contexts (Discord, group chats)". Personal by default.
- Removal is explicit (`remove`), never inferred from conversation — OpenClaw
  standing-intents: "cancellation is durable state rather than a model judgment".
- Daily log = OpenClaw `memory/YYYY-MM-DD.md` daily notes ("raw logs of what
  happened"); today and yesterday are read on demand, the prompt block only
  carries a count and the last line.

## Context

`prompt/context` (plugin must be **global** to reach lean-mode chat homes)
injects a compact "today" block: overdue, due today, today's habits with
streaks, the next 7 days of events. Date only, no clock, so the block only
changes when the data does (gray re-sends a `[Context update]` only when the
text changes). Private contexts: the local REPL (no `route.json`), or a chat
whose `route.json` has `kind: "dm"`. Anything else is shared.

## Profile

No second profile file. Name/timezone/quiet hours live in settings; durable
preferences go to gray's existing user memory (`memory` tool, scope user).
The ritual tells the model to write them as directives ("Prefer…", "Never…")
— OpenClaw USER.md's directive form; gray memory already supersedes by key.

## Onboarding ritual

Ported from OpenClaw BOOTSTRAP.md ("a ritual, not a gate"; "the user's
request always comes first"; persist results; never repeat) and Hermes'
first-run beats (`agent/onboarding_text.py`: one question at a time, never
list the options as a form, skip on request and take the default).

Runs automatically only in a private **chat** (DM), not the coding REPL;
`/life onboard` starts it anywhere. Beats: name → timezone (confirm the
detected one) → what to keep track of (create the first habits/todos now) →
check-ins (morning/evening/weekly, quiet hours) → one closing line. Then
`life onboarding done`; skipped with `life onboarding skip`.

## Check-ins and reminders

- Timed todos/events get a verbatim reminder via `gray cron add <RFC3339>
  <text> --reminder`; the job id is stored and removed on done/remove. The
  sidecar inherits `GRAY_CRON_ORIGIN`, so it comes back to the chat it was
  made in.
- Check-ins are recurring cron jobs (Hermes `cron/blueprint_catalog.py`
  morning-brief / evening-winddown / weekly-review / habit-checkin): self-
  contained prompts (cron runs have no chat context), answer `[SILENT]` when
  there is nothing worth saying (Hermes `[SILENT]`, OpenClaw `NO_REPLY`).
- gray cron evaluates schedules in the **host** zone; the user's time is
  converted to host-local before the job is created (graytest runs UTC).
- Confirmation restates the schedule in a plain sentence, not cron syntax
  (OpenClaw cron-jobs "Promoting a repeated job").

## Not ported

OpenClaw's dreaming sweeps, task flows and standing intents (needs the user
message; `prompt/context` doesn't get it), Hermes' kanban/state DB and
Google Workspace OAuth. Candidates for later: standing intents via
`input/submit`, a weekly log → memory distillation job.
