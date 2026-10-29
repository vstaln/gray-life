//! The `life` tool: one action-dispatched tool, like Hermes' memory/cron
//! tools and gray-todo.

use chrono::{Duration, NaiveTime, Utc};
use serde_json::{Value, json};

use crate::ctx::Ctx;
use crate::store::{Checkin, Event, Habit, Life, Settings, Store, Todo, day_key, when_of};
use crate::view::{self, Now, clip};
use crate::when::{self, Schedule, When, resolve_tz};
use crate::{cron, ritual};

pub fn schema() -> Value {
    json!({
        "name": "life",
        "description": "The user's personal tracker, shared across every chat: todos (optionally due), events (what's going on / coming up — the calendar), habits (with streaks), and a daily log. \
Actions: today (overview) · add {kind: todo|event|habit, text, when?, end?, schedule?, remind?, shared?} · done {id, date?} (finish a todo / check in a habit) · undo {id, date?} · \
edit {id, text?, when?, end?, schedule?, note?, shared?} · remove {id} (only when the user asks) · list {kind?, include_done?} · note {text} (append to today's log: what happened, how they feel) · \
log {date?} · review (last 7 days) · settings {name?, timezone?, quiet_hours?} · checkins {morning?, evening?, weekly?} (\"08:00\", \"sun 18:00\", \"off\") · onboarding {status: done|skip|restart}. \
Times are the user's wall clock: today, tomorrow 9am, fri 14:00, in 2h, 31 oct, 2026-11-01 14:00. Habit schedules: daily, weekdays, weekends, mon,wed,fri, 3/week. \
Items are private (hidden in shared channels) unless shared=true.",
        "parameters": {
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["today", "add", "done", "undo", "edit", "remove", "list", "note", "log", "review", "settings", "checkins", "onboarding"]},
                "kind": {"type": "string", "enum": ["todo", "event", "habit"], "description": "add: what to create; list: filter"},
                "text": {"type": "string", "description": "add: the todo/event text or habit name; note: the log line"},
                "id": {"type": "string", "description": "t3 / e5 / h1"},
                "when": {"type": "string", "description": "todo due or event start"},
                "end": {"type": "string", "description": "event end"},
                "schedule": {"type": "string", "description": "habit schedule"},
                "date": {"type": "string", "description": "done/undo: which day for a habit (default today); log: which day"},
                "remind": {"description": "timed items: true/false, or a lead like \"15m\" / \"1h\". Default: on when this chat can receive reminders"},
                "shared": {"type": "boolean", "description": "show in shared channels too (default false)"},
                "note": {"type": "string", "description": "edit: a note on the item"},
                "include_done": {"type": "boolean"},
                "name": {"type": "string"},
                "timezone": {"type": "string", "description": "IANA zone, e.g. Asia/Jakarta"},
                "quiet_hours": {"type": "string", "description": "e.g. 23:00-08:00"},
                "morning": {"type": "string"},
                "evening": {"type": "string"},
                "weekly": {"type": "string"},
                "status": {"type": "string", "enum": ["done", "skip", "restart"]}
            },
            "required": ["action"]
        }
    })
}

fn s<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

pub fn now_for(settings: &Settings) -> Now {
    let tz = resolve_tz(settings.timezone.as_deref());
    Now { tz, at: Utc::now().with_timezone(&tz).naive_local() }
}

pub fn call(store: &Store, ctx: Ctx, args: &Value) -> Result<String, String> {
    let _lock = store.lock().map_err(|e| format!("can't lock {}: {e}", store.dir.display()))?;
    let mut life = store.load_life();
    let mut settings = store.load_settings();
    let now = now_for(&settings);
    let action = s(args, "action").ok_or("missing action")?;
    let out = match action {
        "today" => view::today(store, &life, &now, ctx.private),
        "review" => view::review(store, &life, &now, ctx.private),
        "list" => view::list(&life, &now, ctx.private, s(args, "kind"), args.get("include_done").and_then(Value::as_bool).unwrap_or(false)),
        "add" => add(store, &mut life, &now, ctx, args)?,
        "done" => done(store, &mut life, &now, ctx, args, true)?,
        "undo" => done(store, &mut life, &now, ctx, args, false)?,
        "edit" => edit(&mut life, &now, ctx, args)?,
        "remove" => remove(&mut life, ctx, args)?,
        "note" => {
            if !ctx.private {
                return Err("the daily log is private; add notes from the owner's DM".into());
            }
            let text = s(args, "text").ok_or("note needs text")?;
            store
                .append_log(now.today(), &now.at.format("%H:%M").to_string(), text)
                .map_err(|e| format!("can't write log: {e}"))?;
            return Ok(format!("Logged for {}.", now.at.format("%a %-d %b")));
        }
        "log" => {
            if !ctx.private {
                return Err("the daily log is private; read it from the owner's DM".into());
            }
            let day = match s(args, "date") {
                Some(d) => when::parse(d, now.at, now.tz)?.date(),
                None => now.today(),
            };
            let lines = store.read_log(day);
            if lines.is_empty() {
                return Ok(format!("No log for {}.", day.format("%a %-d %b %Y")));
            }
            return Ok(format!("{}\n{}", day.format("%A %-d %B %Y"), lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")));
        }
        "settings" => return settings_action(store, &mut settings, ctx, args),
        "checkins" => return checkins(store, &mut settings, ctx, args),
        "onboarding" => {
            let st = s(args, "status").ok_or("onboarding needs status: done|skip|restart")?;
            settings.onboarding = match st {
                "done" => Some("done".into()),
                "skip" | "skipped" => Some("skipped".into()),
                "restart" | "pending" => None,
                o => return Err(format!("unknown status {o:?}")),
            };
            store.save_settings(&settings).map_err(|e| e.to_string())?;
            return Ok(match st {
                "restart" | "pending" => format!("First-run ritual is pending again.\n\n{}", ritual::ritual(now.tz.name(), settings.name.as_deref())),
                _ => "Onboarding recorded; it won't come up again.".into(),
            });
        }
        other => return Err(format!("unknown action {other:?}")),
    };
    store.save_life(&life).map_err(|e| format!("can't save: {e}"))?;
    Ok(out)
}

fn reminder_lead(args: &Value) -> Option<Option<Duration>> {
    // None = don't remind; Some(None) = at the time; Some(Some(d)) = d before.
    match args.get("remind") {
        Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(None),
        Some(Value::String(v)) => {
            let v = v.trim().to_ascii_lowercase();
            match v.as_str() {
                "" | "true" | "yes" | "on" | "at" => Some(None),
                "false" | "no" | "off" | "none" => None,
                _ => {
                    let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
                    let n: i64 = digits.parse().ok()?;
                    let unit = v[digits.len()..].trim();
                    Some(Some(match unit.chars().next() {
                        Some('h') => Duration::hours(n),
                        Some('d') => Duration::days(n),
                        _ => Duration::minutes(n),
                    }))
                }
            }
        }
        _ => cron::has_origin().then_some(None),
    }
}

/// Schedule a verbatim reminder for a timed item. Returns (job, note).
fn schedule_reminder(now: &Now, when: When, lead: Option<Option<Duration>>, text: &str) -> (Option<String>, String) {
    let Some(lead) = lead else { return (None, String::new()) };
    let Some(at) = when.instant(now.tz) else { return (None, String::new()) };
    let fire = at - lead.unwrap_or_else(Duration::zero);
    if fire <= Utc::now() + Duration::seconds(30) {
        return (None, " (no reminder: that time has passed)".into());
    }
    match cron::add(&fire.to_rfc3339_opts(chrono::SecondsFormat::Secs, true), text, None, true) {
        Ok(job) => {
            let local = fire.with_timezone(&now.tz).naive_local();
            (Some(job), format!(" · reminder {}", When::At(local).show(now.today())))
        }
        Err(e) => (None, format!(" (reminder not scheduled: {e})")),
    }
}

fn add(store: &Store, life: &mut Life, now: &Now, ctx: Ctx, args: &Value) -> Result<String, String> {
    let kind = s(args, "kind").ok_or("add needs kind: todo|event|habit")?;
    let text = s(args, "text").ok_or("add needs text")?.to_string();
    let shared = args.get("shared").and_then(Value::as_bool).unwrap_or(false);
    let created = now.at.format("%Y-%m-%dT%H:%M").to_string();
    let privacy = if shared || ctx.private { "" } else { " (private: shows in your DM, not here)" };
    let lead = reminder_lead(args);
    Ok(match kind {
        "todo" => {
            let due = s(args, "when").map(|w| when::parse(w, now.at, now.tz)).transpose()?;
            let id = life.mint('t');
            let (job, rnote) = match due {
                Some(w) => schedule_reminder(now, w, lead, &format!("⏰ Reminder: {text}")),
                None => (None, String::new()),
            };
            let due_s = due.map(|w| format!(" — due {}", w.show(now.today()))).unwrap_or_default();
            life.todos.push(Todo { id: id.clone(), text: text.clone(), due: due.map(|w| w.encode()), created, shared, reminder_job: job, ..Default::default() });
            format!("Added {id} {text}{due_s}{rnote}{privacy}")
        }
        "event" => {
            let at = when::parse(s(args, "when").ok_or("an event needs when")?, now.at, now.tz)?;
            let end = s(args, "end").map(|w| when::parse(w, now.at, now.tz)).transpose()?;
            let id = life.mint('e');
            let label = match at {
                When::At(t) => format!("⏰ {text} at {}", t.format("%H:%M")),
                When::Day(_) => format!("⏰ {text} today"),
            };
            let (job, rnote) = schedule_reminder(now, at, lead, &label);
            life.events.push(Event { id: id.clone(), text: text.clone(), at: at.encode(), end: end.map(|w| w.encode()), created, shared, reminder_job: job, ..Default::default() });
            format!("Added {id} {text} — {}{rnote}{privacy}", at.show(now.today()))
        }
        "habit" => {
            let schedule = Schedule::parse(s(args, "schedule").unwrap_or("daily"))?;
            if let Some(h) = life.habits.iter().find(|h| h.name.eq_ignore_ascii_case(&text)) {
                return Err(format!("already tracking {} {}", h.id, h.name));
            }
            let id = life.mint('h');
            let line = format!("Tracking {id} {text} — {}{privacy}", schedule.show());
            life.habits.push(Habit { id, name: text, schedule, created, log: Vec::new(), shared, note: None });
            let _ = store;
            line
        }
        o => return Err(format!("unknown kind {o:?}; use todo, event or habit")),
    })
}

enum Found {
    Todo(usize),
    Event(usize),
    Habit(usize),
}

fn find(life: &Life, ctx: Ctx, args: &Value) -> Result<Found, String> {
    let id = s(args, "id").ok_or("needs id (t3 / e5 / h1)")?.to_ascii_lowercase();
    let vis = |shared: bool| ctx.private || shared;
    if let Some(i) = life.todos.iter().position(|t| t.id == id && vis(t.shared)) {
        return Ok(Found::Todo(i));
    }
    if let Some(i) = life.events.iter().position(|e| e.id == id && vis(e.shared)) {
        return Ok(Found::Event(i));
    }
    if let Some(i) = life.habits.iter().position(|h| h.id == id && vis(h.shared)) {
        return Ok(Found::Habit(i));
    }
    Err(format!("no item {id}; `life list` shows ids"))
}

fn done(store: &Store, life: &mut Life, now: &Now, ctx: Ctx, args: &Value, on: bool) -> Result<String, String> {
    let clock = now.at.format("%H:%M").to_string();
    match find(life, ctx, args)? {
        Found::Todo(i) => {
            let t = &mut life.todos[i];
            if on {
                if t.done_at.is_some() {
                    return Ok(format!("{} was already done.", t.id));
                }
                t.done_at = Some(now.at.format("%Y-%m-%dT%H:%M").to_string());
                if let Some(j) = t.reminder_job.take() {
                    cron::remove(&j);
                }
                if ctx.private {
                    let _ = store.append_log(now.today(), &clock, &format!("done: {}", t.text));
                }
                Ok(format!("Done: {} {}", t.id, t.text))
            } else {
                t.done_at = None;
                Ok(format!("Reopened {} {}", t.id, t.text))
            }
        }
        Found::Event(i) => Err(format!("{} is an event; edit or remove it instead", life.events[i].id)),
        Found::Habit(i) => {
            let day = match s(args, "date") {
                Some(d) => when::parse(d, now.at, now.tz)?.date(),
                None => now.today(),
            };
            if day > now.today() {
                return Err("can't check in a habit for a future day".into());
            }
            let h = &mut life.habits[i];
            let changed = if on { h.mark(day) } else { h.unmark(day) };
            if on && changed && ctx.private {
                let suffix = if day == now.today() { String::new() } else { format!(" ({})", day.format("%a %-d %b")) };
                let _ = store.append_log(now.today(), &clock, &format!("habit: {}{suffix}", h.name));
            }
            let what = match (on, changed) {
                (true, true) => "Checked in",
                (true, false) => "Already checked in",
                (false, true) => "Removed check-in",
                (false, false) => "No check-in to remove",
            };
            Ok(format!("{what}: {}\n{}", day.format("%a %-d %b"), view::habit_line(h, now.today())))
        }
    }
}

fn edit(life: &mut Life, now: &Now, ctx: Ctx, args: &Value) -> Result<String, String> {
    let shared = args.get("shared").and_then(Value::as_bool);
    let note = s(args, "note").map(str::to_string);
    let text = s(args, "text").map(str::to_string);
    match find(life, ctx, args)? {
        Found::Todo(i) => {
            let due = match s(args, "when") {
                Some("none") | Some("clear") => Some(None),
                Some(w) => Some(Some(when::parse(w, now.at, now.tz)?)),
                None => None,
            };
            let t = &mut life.todos[i];
            if let Some(v) = text { t.text = v; }
            if let Some(v) = shared { t.shared = v; }
            if note.is_some() { t.note = note; }
            let mut rnote = String::new();
            if let Some(d) = due {
                t.due = d.map(|w| w.encode());
                if let Some(j) = t.reminder_job.take() { cron::remove(&j); }
                if let Some(w) = d {
                    let (job, n) = schedule_reminder(now, w, reminder_lead(args), &format!("⏰ Reminder: {}", t.text));
                    t.reminder_job = job;
                    rnote = n;
                }
            }
            let due_s = when_of(&t.due).map(|w| format!(" — due {}", w.show(now.today()))).unwrap_or_default();
            Ok(format!("Updated {} {}{due_s}{rnote}", t.id, t.text))
        }
        Found::Event(i) => {
            let at = s(args, "when").map(|w| when::parse(w, now.at, now.tz)).transpose()?;
            let end = match s(args, "end") {
                Some("none") | Some("clear") => Some(None),
                Some(w) => Some(Some(when::parse(w, now.at, now.tz)?)),
                None => None,
            };
            let e = &mut life.events[i];
            if let Some(v) = text { e.text = v; }
            if let Some(v) = shared { e.shared = v; }
            if note.is_some() { e.note = note; }
            if let Some(v) = end { e.end = v.map(|w| w.encode()); }
            let mut rnote = String::new();
            if let Some(w) = at {
                e.at = w.encode();
                if let Some(j) = e.reminder_job.take() { cron::remove(&j); }
                let label = match w {
                    When::At(t) => format!("⏰ {} at {}", e.text, t.format("%H:%M")),
                    When::Day(_) => format!("⏰ {} today", e.text),
                };
                let (job, n) = schedule_reminder(now, w, reminder_lead(args), &label);
                e.reminder_job = job;
                rnote = n;
            }
            let shown = When::decode(&e.at).map(|w| w.show(now.today())).unwrap_or_default();
            Ok(format!("Updated {} {} — {shown}{rnote}", e.id, e.text))
        }
        Found::Habit(i) => {
            let sched = s(args, "schedule").map(Schedule::parse).transpose()?;
            let h = &mut life.habits[i];
            if let Some(v) = text { h.name = v; }
            if let Some(v) = shared { h.shared = v; }
            if note.is_some() { h.note = note; }
            if let Some(v) = sched { h.schedule = v; }
            Ok(format!("Updated {}", view::habit_line(h, now.today())))
        }
    }
}

fn remove(life: &mut Life, ctx: Ctx, args: &Value) -> Result<String, String> {
    Ok(match find(life, ctx, args)? {
        Found::Todo(i) => {
            let t = life.todos.remove(i);
            if let Some(j) = &t.reminder_job { cron::remove(j); }
            format!("Removed {} {}", t.id, t.text)
        }
        Found::Event(i) => {
            let e = life.events.remove(i);
            if let Some(j) = &e.reminder_job { cron::remove(j); }
            format!("Removed {} {}", e.id, e.text)
        }
        Found::Habit(i) => {
            let h = life.habits.remove(i);
            format!("Stopped tracking {} {} ({} check-ins kept nowhere; gone)", h.id, h.name, h.log.len())
        }
    })
}

fn settings_action(store: &Store, settings: &mut Settings, ctx: Ctx, args: &Value) -> Result<String, String> {
    let mut changed = Vec::new();
    if let Some(n) = s(args, "name") {
        settings.name = Some(n.to_string());
        changed.push(format!("name {n}"));
    }
    let mut tz_changed = false;
    if let Some(z) = s(args, "timezone") {
        let tz: chrono_tz::Tz = z.parse().map_err(|_| format!("unknown timezone {z:?}; use an IANA name like Asia/Jakarta"))?;
        tz_changed = settings.timezone.as_deref() != Some(tz.name());
        settings.timezone = Some(tz.name().to_string());
        changed.push(format!("timezone {}", tz.name()));
    }
    if let Some(q) = s(args, "quiet_hours") {
        let ok = q.split_once('-').is_some_and(|(a, b)| when::clock(a).is_some() && when::clock(b).is_some());
        if !ok && q != "off" {
            return Err(format!("quiet_hours {q:?}: use HH:MM-HH:MM, e.g. 23:00-08:00, or off"));
        }
        settings.quiet_hours = (q != "off").then(|| q.to_string());
        changed.push(format!("quiet hours {q}"));
    }
    let mut extra = String::new();
    if tz_changed && !settings.checkins.is_empty() {
        // Host-local cron times were derived from the old zone.
        let kinds: Vec<(String, Checkin)> = settings.checkins.clone().into_iter().collect();
        for (k, c) in kinds {
            let spec = match &c.day { Some(d) => format!("{d} {}", c.time), None => c.time.clone() };
            match make_checkin(settings, &k, &spec) {
                Ok(line) => extra.push_str(&format!("\n{line}")),
                Err(e) => extra.push_str(&format!("\n{k}: {e}")),
            }
        }
    }
    if changed.is_empty() {
        let now = now_for(settings);
        let checkins: Vec<String> = settings
            .checkins
            .iter()
            .map(|(k, c)| format!("{k} {}{} (job {})", c.day.as_deref().map(|d| format!("{d} ")).unwrap_or_default(), c.time, c.job))
            .collect();
        return Ok(format!(
            "name: {}\ntimezone: {} (now {})\nquiet hours: {}\ncheck-ins: {}\nonboarding: {}\ncontext: {}",
            settings.name.as_deref().unwrap_or("(unset)"),
            now.tz.name(),
            now.at.format("%a %H:%M"),
            settings.quiet_hours.as_deref().unwrap_or("(none)"),
            if checkins.is_empty() { "(none)".into() } else { checkins.join(", ") },
            settings.onboarding.as_deref().unwrap_or("pending"),
            if ctx.private { "private" } else { "shared" },
        ));
    }
    store.save_settings(settings).map_err(|e| e.to_string())?;
    Ok(format!("Saved: {}{extra}", changed.join(", ")))
}

/// Create (or replace) one check-in job. `spec`: "08:00" or "sun 18:00".
fn make_checkin(settings: &mut Settings, kind: &str, spec: &str) -> Result<String, String> {
    let tz = resolve_tz(settings.timezone.as_deref());
    let (day, time_s) = if kind == "weekly" {
        let mut it = spec.split_whitespace();
        let d = it.next().ok_or("weekly needs a day and time, e.g. \"sun 18:00\"")?;
        let t = it.collect::<Vec<_>>().join(" ");
        (Some(d.to_ascii_lowercase()), if t.is_empty() { "18:00".to_string() } else { t })
    } else {
        (None, spec.to_string())
    };
    let time: NaiveTime = when::clock(&time_s).ok_or_else(|| format!("can't read time {time_s:?}"))?;
    let dow = match &day {
        Some(d) => Some(match Schedule::parse(d)? {
            Schedule::Days(v) if v.len() == 1 => v[0] as u32,
            _ => return Err(format!("weekly needs one day, got {d:?}")),
        }),
        None => None,
    };
    let expr = cron::host_cron(tz, time, dow);
    if let Some(old) = settings.checkins.remove(kind) {
        cron::remove(&old.job);
    }
    let job = cron::add(&expr, ritual::checkin_prompt(kind), Some(&format!("life-{kind}")), false)?;
    let hm = time.format("%H:%M").to_string();
    let sentence = match &day {
        Some(d) => format!("Every {d} at {hm} {}: weekly review (job {job})", tz.name()),
        None => format!("Every day at {hm} {}: {kind} check-in (job {job})", tz.name()),
    };
    settings.checkins.insert(kind.to_string(), Checkin { time: hm, day, job });
    Ok(sentence)
}

fn checkins(store: &Store, settings: &mut Settings, ctx: Ctx, args: &Value) -> Result<String, String> {
    if !ctx.private {
        return Err("check-ins carry personal items; set them up from the owner's DM".into());
    }
    let mut lines = Vec::new();
    for kind in ritual::CHECKIN_KINDS {
        let Some(spec) = s(args, kind) else { continue };
        if spec.eq_ignore_ascii_case("off") {
            if let Some(old) = settings.checkins.remove(kind) {
                cron::remove(&old.job);
                lines.push(format!("{kind} check-in off"));
            }
            continue;
        }
        lines.push(make_checkin(settings, kind, spec)?);
    }
    store.save_settings(settings).map_err(|e| e.to_string())?;
    if lines.is_empty() {
        return Ok(if settings.checkins.is_empty() {
            "No check-ins. Pass morning/evening (\"08:00\") or weekly (\"sun 18:00\").".into()
        } else {
            settings.checkins.iter().map(|(k, c)| format!("{k}: {}{} (job {})", c.day.as_deref().map(|d| format!("{d} ")).unwrap_or_default(), c.time, c.job)).collect::<Vec<_>>().join("\n")
        });
    }
    if !cron::has_origin() && !ctx.chat {
        lines.push("Note: no chat is bound here, so results stay in the local cron inbox. Set check-ins up from your DM to get them as messages.".into());
    }
    Ok(lines.join("\n"))
}

/// The `prompt/context` text.
pub fn context_text(store: &Store, ctx: Ctx) -> String {
    let settings = store.load_settings();
    let life = store.load_life();
    let now = now_for(&settings);
    if !ctx.chat && !ctx.cron {
        // Local REPL: usually coding. One line, only when something is live.
        let sec = view::sections(&life, &now, true);
        let open_habits = life.habits.iter().filter(|h| h.due_today(now.today()) && !h.done_on(now.today())).count();
        if sec.overdue.is_empty() && sec.today.is_empty() && open_habits == 0 {
            return String::new();
        }
        return format!(
            "gray-life ({}): {} overdue, {} today, {} habit(s) open. `life today` for details; only bring it up if relevant.",
            now.at.format("%a %-d %b"),
            sec.overdue.len(),
            sec.today.len(),
            open_habits
        );
    }
    let mut text = view::prompt_block(store, &life, &now, ctx.private);
    if settings.onboarding.is_none() && ctx.private && ctx.chat && !ctx.cron {
        text.push('\n');
        text.push_str(&ritual::ritual(now.tz.name(), settings.name.as_deref()));
    } else if let Some(name) = &settings.name {
        text.push_str(&format!("\nThey go by {name}.\n"));
    }
    if let Some(q) = &settings.quiet_hours {
        text.push_str(&format!("Quiet hours {q}: unless it's urgent, don't nudge then.\n"));
    }
    let _ = day_key;
    clip(text.trim_end(), 3500)
}
