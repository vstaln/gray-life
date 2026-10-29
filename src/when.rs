//! Parsing the user's words for time ("tomorrow 9am", "fri", "in 2h",
//! "2026-11-01 14:00") and habit schedules ("daily", "mon,wed,fri",
//! "3/week"). All wall-clock values are in the user's zone.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// A point on the user's calendar: a whole day, or a wall-clock minute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum When {
    Day(NaiveDate),
    At(NaiveDateTime),
}

impl When {
    pub fn date(&self) -> NaiveDate {
        match self {
            When::Day(d) => *d,
            When::At(t) => t.date(),
        }
    }
    pub fn encode(&self) -> String {
        match self {
            When::Day(d) => d.format("%Y-%m-%d").to_string(),
            When::At(t) => t.format("%Y-%m-%dT%H:%M").to_string(),
        }
    }
    pub fn decode(s: &str) -> Option<When> {
        if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
            return Some(When::At(t));
        }
        NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().map(When::Day)
    }
    /// Sort key: a whole day sorts at its start.
    pub fn key(&self) -> NaiveDateTime {
        match self {
            When::Day(d) => d.and_hms_opt(0, 0, 0).expect("midnight"),
            When::At(t) => *t,
        }
    }
    /// "Sat 31 Oct 07:00", "today 14:00", "Tue 27 Oct".
    pub fn show(&self, today: NaiveDate) -> String {
        let d = self.date();
        let day = if d == today {
            "today".to_string()
        } else if d == today + Duration::days(1) {
            "tomorrow".to_string()
        } else if d == today - Duration::days(1) {
            "yesterday".to_string()
        } else if d.year() == today.year() {
            d.format("%a %-d %b").to_string()
        } else {
            d.format("%a %-d %b %Y").to_string()
        };
        match self {
            When::Day(_) => day,
            When::At(t) => format!("{day} {}", t.format("%H:%M")),
        }
    }
    /// The instant, for scheduling. None for a whole day.
    pub fn instant(&self, tz: Tz) -> Option<chrono::DateTime<chrono::Utc>> {
        match self {
            When::Day(_) => None,
            When::At(t) => tz
                .from_local_datetime(t)
                .earliest()
                .map(|d| d.with_timezone(&chrono::Utc)),
        }
    }
}

pub const WHEN_FORMS: &str = "today, tomorrow 9am, fri 14:00, next mon, in 2h, in 3d, 31 oct, oct 31 18:30, 2026-11-01, 2026-11-01 14:00, or RFC3339";

fn weekday(s: &str) -> Option<Weekday> {
    let s = s.trim_end_matches('.');
    Some(match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

fn month(s: &str) -> Option<u32> {
    let s = s.trim_end_matches('.');
    let names = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    names
        .iter()
        .position(|m| s.starts_with(m) && s.len() >= 3)
        .map(|i| i as u32 + 1)
}

/// "14:00", "9am", "2:30pm", "noon", "midnight".
pub fn clock(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "noon" => return NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        _ => {}
    }
    let (body, pm) = if let Some(b) = s.strip_suffix("pm") {
        (b.trim(), Some(true))
    } else if let Some(b) = s.strip_suffix("am") {
        (b.trim(), Some(false))
    } else {
        (s.as_str(), None)
    };
    let (h, m) = match body.split_once([':', '.']) {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if pm.is_some() => (body.parse::<u32>().ok()?, 0),
        None if body.len() == 4 && body.chars().all(|c| c.is_ascii_digit()) => {
            (body[..2].parse().ok()?, body[2..].parse().ok()?)
        }
        None => return None,
    };
    let h = match pm {
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        Some(_) if h > 12 => return None,
        _ => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

/// Parse a user-facing time phrase relative to `now` (user's wall clock).
pub fn parse(raw: &str, now: NaiveDateTime, tz: Tz) -> Result<When, String> {
    let s = raw.trim().to_ascii_lowercase();
    let bad = || format!("can't read time {raw:?}; use {WHEN_FORMS}");
    if s.is_empty() {
        return Err(bad());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw.trim()) {
        return Ok(When::At(trim_secs(dt.with_timezone(&tz).naive_local())));
    }
    if let Some(w) = When::decode(raw.trim()) {
        return Ok(w);
    }
    for f in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H.%M"] {
        if let Ok(t) = NaiveDateTime::parse_from_str(raw.trim(), f) {
            return Ok(When::At(t));
        }
    }
    let today = now.date();
    if let Some(rest) = s.strip_prefix("in ") {
        let rest = rest.trim();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n: i64 = digits.parse().map_err(|_| bad())?;
        let unit = rest[digits.len()..].trim();
        let unit = unit.trim_start_matches(' ');
        return Ok(match unit.chars().next() {
            Some('m') if !unit.starts_with("mo") => {
                When::At(trim_secs(now + Duration::minutes(n)))
            }
            Some('h') => When::At(trim_secs(now + Duration::hours(n))),
            Some('d') => When::Day(today + Duration::days(n)),
            Some('w') => When::Day(today + Duration::weeks(n)),
            _ => return Err(bad()),
        });
    }
    let words: Vec<&str> = s.split_whitespace().filter(|w| *w != "at" && *w != "on").collect();
    // Find the date part (first 1-2 words), then an optional clock.
    let (date, used): (Option<NaiveDate>, usize) = match words.as_slice() {
        ["today", ..] | ["tonight", ..] => (Some(today), 1),
        ["tomorrow", ..] | ["tmr", ..] | ["tmrw", ..] => (Some(today + Duration::days(1)), 1),
        ["yesterday", ..] => (Some(today - Duration::days(1)), 1),
        ["next", w, ..] if weekday(w).is_some() => {
            // That weekday in next calendar week (plain "fri" is the coming one).
            let wd = weekday(w).expect("checked");
            (Some(week_start(today) + Duration::days(7 + wd.num_days_from_monday() as i64)), 2)
        }
        ["next", "week", ..] => (Some(today + Duration::days(7)), 2),
        [w, ..] if weekday(w).is_some() => (Some(next_weekday(today, weekday(w).expect("checked"))), 1),
        [a, b, ..] if month(a).is_some() && b.trim_end_matches([',', 's', 't', 'n', 'd', 'r', 'h']).parse::<u32>().is_ok() => {
            let d = b.trim_end_matches([',', 's', 't', 'n', 'd', 'r', 'h']).parse().expect("checked");
            (Some(month_day(today, month(a).expect("checked"), d).ok_or_else(bad)?), 2)
        }
        [a, b, ..] if month(b).is_some() && a.trim_end_matches(['s', 't', 'n', 'd', 'r', 'h']).parse::<u32>().is_ok() => {
            let d = a.trim_end_matches(['s', 't', 'n', 'd', 'r', 'h']).parse().expect("checked");
            (Some(month_day(today, month(b).expect("checked"), d).ok_or_else(bad)?), 2)
        }
        _ => (None, 0),
    };
    let rest = words[used..].join(" ");
    let time = if rest.is_empty() {
        None
    } else {
        Some(clock(&rest).ok_or_else(bad)?)
    };
    match (date, time) {
        (Some(d), None) => Ok(When::Day(d)),
        (Some(d), Some(t)) => Ok(When::At(d.and_time(t))),
        (None, Some(t)) => {
            let at = today.and_time(t);
            Ok(When::At(if at > now { at } else { at + Duration::days(1) }))
        }
        (None, None) => Err(bad()),
    }
}

fn trim_secs(t: NaiveDateTime) -> NaiveDateTime {
    use chrono::Timelike;
    t.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(t)
}

/// The next `wd` on or after today.
fn next_weekday(today: NaiveDate, wd: Weekday) -> NaiveDate {
    let ahead = (7 + wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64) % 7;
    today + Duration::days(ahead)
}

/// This year's month/day, or next year's when it already passed.
fn month_day(today: NaiveDate, m: u32, d: u32) -> Option<NaiveDate> {
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this >= today {
        Some(this)
    } else {
        NaiveDate::from_ymd_opt(today.year() + 1, m, d)
    }
}

/// How often a habit is due.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    Daily,
    Days(Vec<u8>),
    PerWeek(u8),
}

pub const SCHEDULE_FORMS: &str = "daily, weekdays, weekends, mon,wed,fri, or 3/week";

impl Schedule {
    pub fn parse(raw: &str) -> Result<Schedule, String> {
        let s = raw.trim().to_ascii_lowercase();
        let bad = || format!("can't read schedule {raw:?}; use {SCHEDULE_FORMS}");
        match s.as_str() {
            "" | "daily" | "every day" | "everyday" => return Ok(Schedule::Daily),
            "weekdays" | "workdays" => return Ok(Schedule::Days(vec![0, 1, 2, 3, 4])),
            "weekends" | "weekend" => return Ok(Schedule::Days(vec![5, 6])),
            _ => {}
        }
        let compact: String = s.replace(' ', "");
        for sep in ["/week", "xweek", "x/week", "timesaweek", "/w", "xaweek"] {
            if let Some(n) = compact.strip_suffix(sep) {
                let n = n.trim_end_matches('x');
                let n: u8 = n.parse().map_err(|_| bad())?;
                if (1..=7).contains(&n) {
                    return Ok(if n == 7 { Schedule::Daily } else { Schedule::PerWeek(n) });
                }
                return Err(bad());
            }
        }
        let mut days: Vec<u8> = s
            .split([',', ' ', '/', '&'])
            .filter(|w| !w.is_empty() && *w != "and" && *w != "every")
            .map(|w| weekday(w).map(|d| d.num_days_from_monday() as u8).ok_or_else(bad))
            .collect::<Result<_, _>>()?;
        days.sort_unstable();
        days.dedup();
        if days.is_empty() {
            return Err(bad());
        }
        Ok(if days.len() == 7 { Schedule::Daily } else { Schedule::Days(days) })
    }

    pub fn show(&self) -> String {
        const N: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
        match self {
            Schedule::Daily => "daily".into(),
            Schedule::Days(d) if d == &[0, 1, 2, 3, 4] => "weekdays".into(),
            Schedule::Days(d) if d == &[5, 6] => "weekends".into(),
            Schedule::Days(d) => d.iter().map(|i| N[*i as usize]).collect::<Vec<_>>().join(","),
            Schedule::PerWeek(n) => format!("{n}/week"),
        }
    }

    /// Is `date` one of the scheduled days (PerWeek: every day is eligible).
    pub fn scheduled_on(&self, date: NaiveDate) -> bool {
        match self {
            Schedule::Daily | Schedule::PerWeek(_) => true,
            Schedule::Days(d) => d.contains(&(date.weekday().num_days_from_monday() as u8)),
        }
    }
}

/// Monday of `d`'s ISO week.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// The user's zone: an explicit setting, then TZ, then /etc/localtime, then UTC.
pub fn resolve_tz(setting: Option<&str>) -> Tz {
    let from = |s: &str| s.trim().trim_start_matches(':').parse::<Tz>().ok();
    if let Some(tz) = setting.and_then(from) {
        return tz;
    }
    if let Some(tz) = std::env::var("TZ").ok().as_deref().and_then(from) {
        return tz;
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let s = link.to_string_lossy();
        if let Some(i) = s.find("zoneinfo/")
            && let Some(tz) = from(&s[i + 9..])
        {
            return tz;
        }
    }
    if let Ok(s) = std::fs::read_to_string("/etc/timezone")
        && let Some(tz) = from(&s)
    {
        return tz;
    }
    Tz::UTC
}
