//! Rendering: the full "today" view, the compact prompt block, the weekly
//! review, and lists.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;

use crate::store::{Event, Habit, Life, Store, Todo, when_of};
use crate::when::When;

pub struct Now {
    pub tz: Tz,
    pub at: NaiveDateTime,
}

impl Now {
    pub fn today(&self) -> NaiveDate {
        self.at.date()
    }
}

fn visible<T>(private: bool, shared: bool, _t: &T) -> bool {
    private || shared
}

fn open_todos(life: &Life, private: bool) -> Vec<&Todo> {
    life.todos
        .iter()
        .filter(|t| t.done_at.is_none() && visible(private, t.shared, t))
        .collect()
}

fn todo_line(t: &Todo, today: NaiveDate) -> String {
    let mut s = format!("{} {}", t.id, t.text);
    if let Some(w) = when_of(&t.due) {
        let shown = match w {
            When::At(at) if at.date() == today => at.format("%H:%M").to_string(),
            _ => w.show(today),
        };
        s.push_str(&format!(" — due {shown}"));
    }
    s
}

fn event_line(e: &Event, today: NaiveDate) -> String {
    let mut s = format!("{} {}", e.id, e.text);
    if let Some(w) = When::decode(&e.at) {
        let shown = match w {
            When::At(at) if at.date() == today => at.format("%H:%M").to_string(),
            When::Day(d) if d == today => "today".into(),
            _ => w.show(today),
        };
        s.push_str(&format!(" — {shown}"));
    }
    if let Some(end) = e.end.as_deref().and_then(When::decode) {
        s.push_str(&format!(" → {}", end.show(today)));
    }
    s
}

pub fn habit_line(h: &Habit, today: NaiveDate) -> String {
    let (n, unit) = h.streak(today);
    let state = if h.done_on(today) {
        "done today".to_string()
    } else if h.due_today(today) {
        "not yet today".to_string()
    } else {
        "not due today".to_string()
    };
    let (d7, o7) = h.last7(today);
    let unit = if n == 1 {
        unit.trim_end_matches('s')
    } else {
        unit
    };
    format!(
        "{} {} — {} · streak {n} {unit} · {d7}/{o7} this week · {state}",
        h.id,
        h.name,
        h.schedule.show()
    )
}

/// An event is "on" a day when it starts that day, or spans it.
fn event_on(e: &Event, day: NaiveDate) -> bool {
    let Some(start) = When::decode(&e.at) else {
        return false;
    };
    let end = e
        .end
        .as_deref()
        .and_then(When::decode)
        .map(|w| w.date())
        .unwrap_or(start.date());
    start.date() <= day && day <= end
}

pub struct Sections {
    pub overdue: Vec<String>,
    pub today: Vec<String>,
    pub habits: Vec<String>,
    pub coming: Vec<String>,
    pub undated: Vec<String>,
    pub hidden: usize,
}

pub fn sections(life: &Life, now: &Now, private: bool) -> Sections {
    let today = now.today();
    let horizon = today + Duration::days(7);
    let mut overdue: Vec<(NaiveDateTime, String)> = Vec::new();
    let mut today_items: Vec<(NaiveDateTime, String)> = Vec::new();
    let mut coming: Vec<(NaiveDateTime, String)> = Vec::new();
    let mut undated = Vec::new();
    for t in open_todos(life, private) {
        match when_of(&t.due) {
            None => undated.push(todo_line(t, today)),
            Some(w) => {
                let late = w.date() < today;
                let line = (w.key(), todo_line(t, today));
                if late {
                    overdue.push(line);
                } else if w.date() == today {
                    today_items.push(line);
                } else if w.date() <= horizon {
                    coming.push(line);
                }
            }
        }
    }
    for e in life
        .events
        .iter()
        .filter(|e| visible(private, e.shared, *e))
    {
        let Some(w) = When::decode(&e.at) else {
            continue;
        };
        let line = (w.key(), event_line(e, today));
        if event_on(e, today) {
            today_items.push(line);
        } else if w.date() > today && w.date() <= horizon {
            coming.push(line);
        }
    }
    overdue.sort();
    today_items.sort();
    coming.sort();
    let habits = life
        .habits
        .iter()
        .filter(|h| visible(private, h.shared, *h))
        .map(|h| habit_line(h, today))
        .collect();
    let hidden = if private {
        0
    } else {
        life.todos
            .iter()
            .filter(|t| t.done_at.is_none() && !t.shared)
            .count()
            + life.events.iter().filter(|e| !e.shared).count()
            + life.habits.iter().filter(|h| !h.shared).count()
    };
    Sections {
        overdue: overdue.into_iter().map(|x| x.1).collect(),
        today: today_items.into_iter().map(|x| x.1).collect(),
        habits,
        coming: coming.into_iter().map(|x| x.1).collect(),
        undated,
        hidden,
    }
}

fn push_section(out: &mut String, title: &str, lines: &[String], cap: usize) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n{title}\n"));
    for l in lines.iter().take(cap) {
        out.push_str(&format!("  {l}\n"));
    }
    if lines.len() > cap {
        out.push_str(&format!("  … {} more\n", lines.len() - cap));
    }
}

pub fn header(now: &Now) -> String {
    format!("{} · {}", now.at.format("%a %-d %b %Y"), now.tz.name())
}

/// The full view the `today` action and `/life` return.
pub fn today(store: &Store, life: &Life, now: &Now, private: bool) -> String {
    let s = sections(life, now, private);
    let mut out = format!("{} · {}\n", header(now), now.at.format("%H:%M"));
    push_section(&mut out, "Overdue", &s.overdue, 50);
    push_section(&mut out, "Today", &s.today, 50);
    push_section(&mut out, "Habits", &s.habits, 50);
    push_section(&mut out, "Coming up (7 days)", &s.coming, 50);
    push_section(&mut out, "Todos without a date", &s.undated, 50);
    if private {
        let log = store.read_log(now.today());
        if !log.is_empty() {
            push_section(&mut out, "Today's log", &log, 20);
        }
    }
    if s.overdue.is_empty()
        && s.today.is_empty()
        && s.habits.is_empty()
        && s.coming.is_empty()
        && s.undated.is_empty()
    {
        out.push_str(if s.hidden > 0 {
            "\nNothing shared here.\n"
        } else {
            "\nNothing tracked yet.\n"
        });
    }
    if s.hidden > 0 {
        out.push_str(&format!(
            "\n({} personal item(s) hidden here; they show in the owner's DM.)\n",
            s.hidden
        ));
    }
    out
}

/// The compact block injected into context each turn. Date, not clock, so
/// it only changes when the data does.
pub fn prompt_block(store: &Store, life: &Life, now: &Now, private: bool) -> String {
    let s = sections(life, now, private);
    let mut out = format!(
        "gray-life — {}. This is the user's own tracker (todos, habits, what's going on, a daily log). \
Keep it current with the `life` tool as things come up: add what they plan or what's coming, mark done what they did, \
note what happened. Small updates need no permission; deletions do.\n",
        header(now)
    );
    push_section(&mut out, "Overdue", &s.overdue, 6);
    push_section(&mut out, "Today", &s.today, 8);
    push_section(&mut out, "Habits", &s.habits, 8);
    push_section(&mut out, "Next 7 days", &s.coming, 6);
    if !s.undated.is_empty() {
        out.push_str(&format!(
            "\n{} todo(s) without a date (life list).\n",
            s.undated.len()
        ));
    }
    if private {
        let log = store.read_log(now.today());
        if let Some(last) = log.last() {
            out.push_str(&format!(
                "\nToday's log: {} note(s), last: {}\n",
                log.len(),
                clip(last, 120)
            ));
        }
    }
    if s.hidden > 0 {
        out.push_str("\nShared channel: personal items are hidden here. Don't reveal them; offer to continue in DM.\n");
    }
    out
}

pub fn review(store: &Store, life: &Life, now: &Now, private: bool) -> String {
    let today = now.today();
    let from = today - Duration::days(6);
    let mut out = format!(
        "Week in review · {} – {} · {}\n",
        from.format("%a %-d %b"),
        today.format("%a %-d %b"),
        now.tz.name()
    );
    let done: Vec<String> = life
        .todos
        .iter()
        .filter(|t| visible(private, t.shared, *t))
        .filter(|t| {
            t.done_at
                .as_deref()
                .and_then(|d| NaiveDate::parse_from_str(d.get(..10).unwrap_or(""), "%Y-%m-%d").ok())
                .is_some_and(|d| d >= from)
        })
        .map(|t| format!("{} {}", t.id, t.text))
        .collect();
    push_section(&mut out, "Done", &done, 40);
    let habits: Vec<String> = life
        .habits
        .iter()
        .filter(|h| visible(private, h.shared, *h))
        .map(|h| habit_line(h, today))
        .collect();
    push_section(&mut out, "Habits", &habits, 40);
    let s = sections(life, now, private);
    push_section(&mut out, "Still overdue", &s.overdue, 40);
    let next: Vec<String> = s.today.iter().chain(s.coming.iter()).cloned().collect();
    push_section(&mut out, "Next 7 days", &next, 40);
    if private {
        let mut days = Vec::new();
        for i in (0..7).rev() {
            let d = today - Duration::days(i);
            let log = store.read_log(d);
            if !log.is_empty() {
                days.push(format!(
                    "{} — {} note(s): {}",
                    d.format("%a %-d"),
                    log.len(),
                    clip(&log.join(" · "), 200)
                ));
            }
        }
        push_section(&mut out, "Log", &days, 7);
    }
    out
}

pub fn list(
    life: &Life,
    now: &Now,
    private: bool,
    kind: Option<&str>,
    include_done: bool,
) -> String {
    let today = now.today();
    let mut out = String::new();
    if kind.is_none_or(|k| k.starts_with("todo")) {
        let mut v: Vec<&Todo> = life
            .todos
            .iter()
            .filter(|t| visible(private, t.shared, *t) && (include_done || t.done_at.is_none()))
            .collect();
        v.sort_by_key(|t| (t.done_at.is_some(), when_of(&t.due).map(|w| w.key())));
        let lines: Vec<String> = v
            .iter()
            .map(|t| {
                let mut l = todo_line(t, today);
                if let Some(d) = &t.done_at {
                    l.push_str(&format!(" ✓ {}", d.get(..10).unwrap_or(d)));
                }
                if let Some(n) = &t.note {
                    l.push_str(&format!(" ({})", clip(n, 80)));
                }
                l
            })
            .collect();
        push_section(&mut out, "Todos", &lines, 200);
    }
    if kind.is_none_or(|k| k.starts_with("event")) {
        let mut v: Vec<&Event> = life
            .events
            .iter()
            .filter(|e| visible(private, e.shared, *e))
            .filter(|e| {
                include_done
                    || e.end
                        .as_deref()
                        .or(Some(e.at.as_str()))
                        .and_then(When::decode)
                        .is_some_and(|w| w.date() >= today)
            })
            .collect();
        v.sort_by_key(|e| When::decode(&e.at).map(|w| w.key()));
        let lines: Vec<String> = v.iter().map(|e| event_line(e, today)).collect();
        push_section(&mut out, "Events", &lines, 200);
    }
    if kind.is_none_or(|k| k.starts_with("habit")) {
        let lines: Vec<String> = life
            .habits
            .iter()
            .filter(|h| visible(private, h.shared, *h))
            .map(|h| habit_line(h, today))
            .collect();
        push_section(&mut out, "Habits", &lines, 200);
    }
    if out.is_empty() {
        "Nothing here.".into()
    } else {
        out.trim_start().to_string()
    }
}

pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
