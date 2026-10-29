//! The on-disk store: `life.json` (todos, events, habits), `settings.json`,
//! and the daily log under `log/`. One per user, shared by every chat home.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::when::{Schedule, When, week_start};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    pub created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder_job: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub text: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    pub created: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminder_job: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Habit {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
    pub created: String,
    /// Dates done, "YYYY-MM-DD", sorted, unique.
    #[serde(default)]
    pub log: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Life {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default = "one64")]
    pub next_id: u64,
    #[serde(default)]
    pub todos: Vec<Todo>,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub habits: Vec<Habit>,
}

fn one() -> u32 {
    1
}
fn one64() -> u64 {
    1
}

impl Life {
    pub fn mint(&mut self, prefix: char) -> String {
        let n = self.next_id.max(1);
        self.next_id = n + 1;
        format!("{prefix}{n}")
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Checkin {
    /// "08:00" in the user's zone.
    pub time: String,
    /// Weekly only: "sun".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<String>,
    pub job: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// "23:00-08:00"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<String>,
    /// pending (absent) | done | skipped
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboarding: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub checkins: std::collections::BTreeMap<String, Checkin>,
}

pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    /// `$GRAY_LIFE_DIR`, else `$GRAY_USER_HOME/life`, else `~/.gray/life`.
    /// Chat homes set their own `GRAY_HOME` but pass `GRAY_USER_HOME`, so
    /// every conversation lands on the same store.
    pub fn locate() -> Store {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let dir = env("GRAY_LIFE_DIR")
            .or_else(|| env("GRAY_USER_HOME").map(|h| h.join("life")))
            .or_else(|| env("HOME").map(|h| h.join(".gray").join("life")))
            .unwrap_or_else(|| PathBuf::from(".gray/life"));
        Store { dir }
    }

    fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    /// Hold the store's lock for a read-modify-write. Every chat home's
    /// gray runs its own sidecar, so two can write at once.
    pub fn lock(&self) -> std::io::Result<File> {
        self.ensure()?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join(".lock"))?;
        f.lock()?;
        Ok(f)
    }

    pub fn load_life(&self) -> Life {
        read_json(&self.dir.join("life.json")).unwrap_or_default()
    }
    pub fn save_life(&self, life: &Life) -> std::io::Result<()> {
        self.ensure()?;
        let mut life = life.clone();
        life.version = 1;
        write_json(&self.dir.join("life.json"), &life)
    }
    pub fn load_settings(&self) -> Settings {
        read_json(&self.dir.join("settings.json")).unwrap_or_default()
    }
    pub fn save_settings(&self, s: &Settings) -> std::io::Result<()> {
        write_json(&self.dir.join("settings.json"), s)
    }

    pub fn log_path(&self, day: NaiveDate) -> PathBuf {
        self.dir.join("log").join(format!("{}.md", day.format("%Y-%m-%d")))
    }
    pub fn read_log(&self, day: NaiveDate) -> Vec<String> {
        std::fs::read_to_string(self.log_path(day))
            .map(|s| {
                s.lines()
                    .filter(|l| l.starts_with("- "))
                    .map(|l| l[2..].to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn append_log(&self, day: NaiveDate, clock: &str, text: &str) -> std::io::Result<()> {
        let path = self.log_path(day);
        std::fs::create_dir_all(path.parent().expect("log dir"))?;
        let fresh = !path.exists();
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        if fresh {
            writeln!(f, "# {}\n", day.format("%A %-d %B %Y"))?;
        }
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        writeln!(f, "- {clock} {one_line}")
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = File::create(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(&serde_json::to_vec_pretty(v).map_err(std::io::Error::other)?)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, path)
}

// ---------- habits ----------

impl Habit {
    pub fn done_on(&self, d: NaiveDate) -> bool {
        self.log.binary_search(&day_key(d)).is_ok()
    }
    pub fn mark(&mut self, d: NaiveDate) -> bool {
        let k = day_key(d);
        match self.log.binary_search(&k) {
            Ok(_) => false,
            Err(i) => {
                self.log.insert(i, k);
                true
            }
        }
    }
    pub fn unmark(&mut self, d: NaiveDate) -> bool {
        match self.log.binary_search(&day_key(d)) {
            Ok(i) => {
                self.log.remove(i);
                true
            }
            Err(_) => false,
        }
    }
    fn done_in_week(&self, monday: NaiveDate) -> usize {
        (0..7).filter(|i| self.done_on(monday + Duration::days(*i))).count()
    }
    fn created_date(&self) -> NaiveDate {
        NaiveDate::parse_from_str(self.created.get(..10).unwrap_or(""), "%Y-%m-%d")
            .unwrap_or(NaiveDate::MIN)
    }

    /// Still owed today? PerWeek: while this week's count is short.
    pub fn due_today(&self, today: NaiveDate) -> bool {
        match &self.schedule {
            Schedule::PerWeek(n) => self.done_in_week(week_start(today)) < *n as usize,
            s => s.scheduled_on(today),
        }
    }

    /// (streak, unit). Today not done yet doesn't break the streak — the
    /// day isn't over.
    pub fn streak(&self, today: NaiveDate) -> (u32, &'static str) {
        // Backfilled check-ins can predate creation; count from whichever is first.
        let first_log = self.log.first().and_then(|k| NaiveDate::parse_from_str(k, "%Y-%m-%d").ok());
        let born = first_log.map_or(self.created_date(), |f| f.min(self.created_date()));
        match &self.schedule {
            Schedule::PerWeek(n) => {
                let mut wk = week_start(today);
                if self.done_in_week(wk) < *n as usize {
                    wk -= Duration::days(7);
                }
                let mut count = 0;
                while wk + Duration::days(6) >= born && self.done_in_week(wk) >= *n as usize {
                    count += 1;
                    wk -= Duration::days(7);
                }
                (count, "weeks")
            }
            s => {
                let mut d = today;
                if !self.done_on(d) {
                    d -= Duration::days(1);
                }
                let mut count = 0;
                let mut guard = 0;
                while d >= born && guard < 3660 {
                    guard += 1;
                    if s.scheduled_on(d) {
                        if !self.done_on(d) {
                            break;
                        }
                        count += 1;
                    }
                    d -= Duration::days(1);
                }
                (count, "days")
            }
        }
    }

    /// Done / scheduled over the 7 days ending today.
    pub fn last7(&self, today: NaiveDate) -> (usize, usize) {
        let days: Vec<NaiveDate> = (0..7).map(|i| today - Duration::days(i)).collect();
        let done = days.iter().filter(|d| self.done_on(**d)).count();
        let owed = match &self.schedule {
            Schedule::PerWeek(n) => *n as usize,
            s => days.iter().filter(|d| s.scheduled_on(**d)).count(),
        };
        (done, owed)
    }
}

pub fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn when_of(s: &Option<String>) -> Option<When> {
    s.as_deref().and_then(When::decode)
}
