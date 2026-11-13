//! Where this turn is happening: a private place (the local REPL, a DM) or
//! a shared one (a server channel, a group). Personal items only show in
//! private places — OpenClaw keeps MEMORY.md out of shared contexts for the
//! same reason.

use std::path::PathBuf;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ctx {
    pub private: bool,
    /// A chat app conversation (vs the local REPL / CLI).
    pub chat: bool,
    /// A scheduled run (`GRAY_TURN_ORIGIN=cron`): nobody is there to talk to.
    pub cron: bool,
}

impl Ctx {
    pub fn detect() -> Ctx {
        let cron = std::env::var("GRAY_TURN_ORIGIN").is_ok_and(|v| v == "cron");
        let route = std::env::var_os("GRAY_HOME")
            .map(PathBuf::from)
            .and_then(|h| std::fs::read(h.join("route.json")).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let mut ctx = match &route {
            None => Ctx {
                private: true,
                chat: false,
                cron,
            },
            Some(r) => Ctx {
                private: route_is_dm(r),
                chat: true,
                cron,
            },
        };
        match std::env::var("GRAY_LIFE_CONTEXT").ok().as_deref() {
            Some("private") => ctx.private = true,
            Some("shared") => ctx.private = false,
            _ => {}
        }
        ctx
    }
}

/// `kind: "dm"` when the chat plugin says so. Older Discord builds write no
/// kind; then the route counts as a DM only when it is the configured home
/// channel (`/sethome` points that at the owner's DM).
pub(crate) fn route_is_dm(r: &Value) -> bool {
    match r.get("kind").and_then(Value::as_str) {
        Some(k) => matches!(k, "dm" | "direct" | "private"),
        None => {
            let (Some(platform), Some(route)) = (
                r.get("platform").and_then(Value::as_str),
                r.get("route").and_then(Value::as_str),
            ) else {
                return false;
            };
            platform == "discord" && discord_home().as_deref() == Some(route)
        }
    }
}

fn discord_home() -> Option<String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let v: Value =
        serde_json::from_slice(&std::fs::read(base.join("gray-discord/config.json")).ok()?).ok()?;
    v.get("channel_id")
        .and_then(Value::as_str)
        .map(str::to_string)
}
