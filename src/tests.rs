use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::json;

use crate::ctx::{Ctx, route_is_dm};
use crate::store::{Habit, Store};
use crate::when::{self, Schedule, When};
use crate::{cron, tool};

const JKT: Tz = chrono_tz::Asia::Jakarta;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}
fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
}

// Thursday 29 Oct 2026, 09:30 local.
fn now() -> NaiveDateTime {
    dt("2026-10-29 09:30")
}

#[test]
fn parses_wall_clock_times() {
    let p = |s| when::parse(s, now(), JKT).unwrap();
    assert_eq!(p("tomorrow 9am"), When::At(dt("2026-10-30 09:00")));
    assert_eq!(p("today"), When::Day(d("2026-10-29")));
    assert_eq!(p("fri 14:00"), When::At(dt("2026-10-30 14:00")));
    assert_eq!(p("next mon"), When::Day(d("2026-11-02")));
    assert_eq!(p("next fri"), When::Day(d("2026-11-06")));
    assert_eq!(p("fri"), When::Day(d("2026-10-30")));
    assert_eq!(p("in 2h"), When::At(dt("2026-10-29 11:30")));
    assert_eq!(p("31 oct"), When::Day(d("2026-10-31")));
    assert_eq!(p("2026-11-01 14:00"), When::At(dt("2026-11-01 14:00")));
    assert!(when::parse("blursday", now(), JKT).is_err());
}

#[test]
fn parses_schedules() {
    assert_eq!(Schedule::parse("daily").unwrap(), Schedule::Daily);
    assert_eq!(Schedule::parse("3/week").unwrap(), Schedule::PerWeek(3));
    let wd = Schedule::parse("weekdays").unwrap();
    assert!(wd.scheduled_on(d("2026-10-30"))); // fri
    assert!(!wd.scheduled_on(d("2026-10-31"))); // sat
    let mwf = Schedule::parse("mon,wed,fri").unwrap();
    assert!(mwf.scheduled_on(d("2026-10-28")));
    assert!(!mwf.scheduled_on(d("2026-10-29")));
    assert!(Schedule::parse("sometimes").is_err());
}

fn habit(schedule: Schedule, created: &str, log: &[&str]) -> Habit {
    Habit {
        id: "h1".into(),
        name: "x".into(),
        schedule,
        created: format!("{created}T08:00"),
        log: log.iter().map(|s| s.to_string()).collect(),
        shared: false,
        note: None,
    }
}

#[test]
fn streaks() {
    let today = d("2026-10-29");
    // Backfilled day before creation counts.
    let h = habit(Schedule::Daily, "2026-10-29", &["2026-10-28", "2026-10-29"]);
    assert_eq!(h.streak(today).0, 2);
    // Today not done yet doesn't break it; a gap does.
    let h = habit(Schedule::Daily, "2026-10-01", &["2026-10-25", "2026-10-27", "2026-10-28"]);
    assert_eq!(h.streak(today).0, 2);
    // Unscheduled days are skipped: mon,wed,fri done on 23 (fri), 26 (mon), 28 (wed).
    let h = habit(Schedule::parse("mon,wed,fri").unwrap(), "2026-10-01", &["2026-10-23", "2026-10-26", "2026-10-28"]);
    assert_eq!(h.streak(today).0, 3);
    // 2/week: last week (19–25 Oct) met, this week not yet → 1 week.
    let h = habit(Schedule::PerWeek(2), "2026-10-01", &["2026-10-20", "2026-10-22", "2026-10-27"]);
    assert_eq!(h.streak(today), (1, "weeks"));
    assert!(h.due_today(today));
}

#[test]
fn host_cron_converts_user_zone() {
    let now = Utc.with_ymd_and_hms(2026, 10, 29, 2, 0, 0).unwrap();
    let t = |s| NaiveTime::parse_from_str(s, "%H:%M").unwrap();
    // 08:00 WIB = 01:00 UTC.
    assert_eq!(cron::host_cron_in(JKT, t("08:00"), None, &Utc, now), "0 1 * * *");
    // Sunday 18:00 WIB = Sunday 11:00 UTC.
    assert_eq!(cron::host_cron_in(JKT, t("18:00"), Some(6), &Utc, now), "0 11 * * 0");
    // Monday 05:00 WIB = Sunday 22:00 UTC: the weekday shifts too.
    assert_eq!(cron::host_cron_in(JKT, t("05:00"), Some(0), &Utc, now), "0 22 * * 0");
}

#[test]
fn dm_detection() {
    assert!(route_is_dm(&json!({"platform": "discord", "route": "1", "kind": "dm"})));
    assert!(!route_is_dm(&json!({"platform": "discord", "route": "1", "kind": "channel"})));
    assert!(!route_is_dm(&json!({"platform": "telegram", "route": "1"})));
}

#[test]
fn tool_round_trip_and_privacy() {
    // SAFETY: tests touching env run in this one test only.
    unsafe { std::env::set_var("GRAY_LIFE_NO_CRON", "1") };
    let dir = std::env::temp_dir().join(format!("gray-life-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store { dir: dir.clone() };
    let private = Ctx { private: true, chat: true, cron: false };
    let shared = Ctx { private: false, chat: true, cron: false };
    let call = |ctx, v| tool::call(&store, ctx, &v);

    call(private, json!({"action": "settings", "timezone": "Asia/Jakarta"})).unwrap();
    let out = call(private, json!({"action": "add", "kind": "todo", "text": "secret thing"})).unwrap();
    assert!(out.contains("t1"), "{out}");
    call(private, json!({"action": "add", "kind": "todo", "text": "team standup", "shared": true})).unwrap();
    call(private, json!({"action": "add", "kind": "habit", "text": "gym", "schedule": "daily"})).unwrap();

    let p = call(private, json!({"action": "today"})).unwrap();
    assert!(p.contains("secret thing") && p.contains("team standup"));
    let s = call(shared, json!({"action": "today"})).unwrap();
    assert!(!s.contains("secret thing") && s.contains("team standup"), "{s}");
    assert!(s.contains("hidden"));
    // Can't touch private items from a shared channel.
    assert!(call(shared, json!({"action": "done", "id": "t1"})).is_err());

    call(private, json!({"action": "done", "id": "h3"})).unwrap();
    assert!(call(private, json!({"action": "today"})).unwrap().contains("done today"));
    call(private, json!({"action": "undo", "id": "h3"})).unwrap();
    assert!(!call(private, json!({"action": "today"})).unwrap().contains("done today"));

    call(private, json!({"action": "remove", "id": "t1"})).unwrap();
    assert!(!call(private, json!({"action": "list"})).unwrap().contains("secret thing"));
    assert!(call(private, json!({"action": "add", "kind": "todo", "text": "x", "when": "blursday"})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
