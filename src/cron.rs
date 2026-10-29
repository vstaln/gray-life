//! Reminders and check-ins ride gray's own cron (`gray cron add`). The
//! sidecar inherits `GRAY_CRON_ORIGIN`, so a job made in a chat comes back
//! to that chat. gray cron reads schedules in the HOST zone, so a user's
//! "08:00" is converted to host-local before the job exists.

use std::process::Command;

use chrono::{Datelike, Duration, Local, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

fn gray_bin() -> String {
    std::env::var("GRAY_LIFE_GRAY_BIN").unwrap_or_else(|_| "gray".into())
}

pub fn disabled() -> bool {
    std::env::var_os("GRAY_LIFE_NO_CRON").is_some()
}

/// `gray cron add …` → the new job id.
pub fn add(schedule: &str, prompt: &str, name: Option<&str>, reminder: bool) -> Result<String, String> {
    if disabled() {
        return Err("cron disabled (GRAY_LIFE_NO_CRON)".into());
    }
    let mut cmd = Command::new(gray_bin());
    cmd.args(["cron", "add", schedule, prompt]);
    if reminder {
        cmd.arg("--reminder");
    }
    if let Some(n) = name {
        cmd.args(["--name", n]);
    }
    let out = cmd.output().map_err(|e| format!("can't run gray cron: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("gray cron add failed: {}", err.trim().lines().last().unwrap_or("")));
    }
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("added ").and_then(|r| r.split_whitespace().next()))
        .map(str::to_string)
        .ok_or_else(|| format!("unexpected gray cron output: {}", stdout.trim()))
}

pub fn remove(job: &str) {
    if disabled() || job.is_empty() {
        return;
    }
    let _ = Command::new(gray_bin()).args(["cron", "remove", job]).output();
}

/// Is a chat binding present (so a reminder has somewhere to go)?
pub fn has_origin() -> bool {
    std::env::var("GRAY_CRON_ORIGIN").is_ok_and(|v| !v.trim().is_empty())
}

/// A 5-field host-local cron for the user's wall-clock `time` on `day`
/// (0 = Monday) or every day.
pub fn host_cron(tz: Tz, time: NaiveTime, day: Option<u32>) -> String {
    host_cron_in(tz, time, day, &Local, Utc::now())
}

pub fn host_cron_in<H: TimeZone>(tz: Tz, time: NaiveTime, day: Option<u32>, host: &H, now: chrono::DateTime<Utc>) -> String {
    let today = now.with_timezone(&tz).date_naive();
    let mut date = today;
    if let Some(d) = day {
        let ahead = (7 + d as i64 - today.weekday().num_days_from_monday() as i64) % 7;
        date += Duration::days(ahead);
    }
    let local = tz
        .from_local_datetime(&date.and_time(time))
        .earliest()
        .unwrap_or_else(|| tz.from_utc_datetime(&date.and_time(time)));
    let h = local.with_timezone(host);
    match day {
        None => format!("{} {} * * *", h.minute(), h.hour()),
        // cron day-of-week: 0 = Sunday.
        Some(_) => format!("{} {} * * {}", h.minute(), h.hour(), h.weekday().num_days_from_sunday()),
    }
}
