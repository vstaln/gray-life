//! gray-life: a personal life tracker plugin for gray.
//!
//! - `gray-life manifest` prints the manifest (install probe).
//! - no args on a pipe: the JSON-lines sidecar (`tool/call` for `life`,
//!   `prompt/context` for the "today" block + first-run ritual,
//!   `command/run` for `/life`).
//! - `gray-life today|list|review|log|tool '<json>'`: a small CLI over the
//!   same store, for humans and scripts.

mod cron;
mod ctx;
mod ritual;
mod store;
mod tool;
mod view;
mod when;
#[cfg(test)]
mod tests;

use std::io::{BufRead, IsTerminal, Write};

use serde_json::{Value, json};

use ctx::Ctx;
use store::Store;

const USAGE: &str = "gray-life: todos, habits, what's going on, a daily log and check-ins.\n\
usage: gray-life [today | list [todo|event|habit] | review | log [date] | onboard | tool '<json args>']\n\
In chat: /life (today) · /life onboard (first-run setup) · /life log [date] · /life review";

fn manifest() -> Value {
    json!({
        "name": "life",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "1.1",
        "tools": [tool::schema()],
        "commands": ["/life"],
        "hooks": ["prompt/context"],
        "completion": ["today", "onboard", "list", "review", "log", "help"],
    })
}

fn tool_result(r: Result<String, String>) -> Value {
    match r {
        Ok(text) => json!({ "content": text, "is_error": false }),
        Err(e) => json!({ "content": e, "is_error": true }),
    }
}

/// Map CLI-ish words onto a `life` tool call.
fn words_to_args(argv: &[String]) -> Result<Value, String> {
    let first = argv.first().map(String::as_str).unwrap_or("today");
    Ok(match first {
        "today" | "" => json!({"action": "today"}),
        "review" => json!({"action": "review"}),
        "list" => match argv.get(1) {
            Some(k) => json!({"action": "list", "kind": k.trim_end_matches('s')}),
            None => json!({"action": "list"}),
        },
        "log" if argv.len() > 1 => json!({"action": "log", "date": argv[1..].join(" ")}),
        "log" => json!({"action": "log"}),
        "settings" => json!({"action": "settings"}),
        "checkins" => json!({"action": "checkins"}),
        "tool" => {
            let raw = argv.get(1).ok_or("tool needs a JSON object of args")?;
            serde_json::from_str(raw).map_err(|e| format!("bad JSON: {e}"))?
        }
        other => return Err(format!("unknown: {other}\n{USAGE}")),
    })
}

fn command_run(store: &Store, params: &Value) -> Value {
    let argv: Vec<String> = params
        .get("argv")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|e| e.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let ctx = Ctx::detect();
    match argv.first().map(String::as_str) {
        Some("onboard" | "setup" | "start") => {
            let mut settings = store.load_settings();
            if settings.onboarding.is_some() {
                settings.onboarding = None;
                if let Err(e) = store.save_settings(&settings) {
                    return json!({ "text": format!("life: can't save settings: {e}") });
                }
            }
            json!({ "prompt": ritual::KICKOFF })
        }
        Some("help" | "-h" | "--help") => json!({ "text": USAGE }),
        _ => match words_to_args(&argv).and_then(|a| tool::call(store, ctx, &a)) {
            Ok(t) => json!({ "text": t }),
            Err(e) => json!({ "text": format!("life: {e}") }),
        },
    }
}

fn sidecar_loop(store: &Store) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "plugin/shutdown" {
            break;
        }
        let Some(id) = v.get("id").cloned().filter(|i| !i.is_null()) else { continue };
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let reply = match method {
            "plugin/manifest" => json!({ "id": id, "result": manifest() }),
            "prompt/context" => {
                let text = tool::context_text(store, Ctx::detect());
                if text.is_empty() {
                    json!({ "id": id, "result": {} })
                } else {
                    json!({ "id": id, "result": { "text": text } })
                }
            }
            "tool/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let result = if name == "life" {
                    let args = params.get("args").cloned().unwrap_or(Value::Null);
                    tool_result(tool::call(store, Ctx::detect(), &args))
                } else {
                    json!({ "content": format!("unknown tool: {name}"), "is_error": true })
                };
                json!({ "id": id, "result": result })
            }
            "command/run" => json!({ "id": id, "result": command_run(store, &params) }),
            other => json!({ "id": id, "error": { "code": -32601, "message": format!("method not found: {other}") } }),
        };
        let mut o = stdout.lock();
        writeln!(o, "{reply}")?;
        o.flush()?;
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let store = Store::locate();
    match args.first().map(String::as_str) {
        Some("manifest") => println!("{}", manifest()),
        Some("help" | "-h" | "--help") => println!("{USAGE}"),
        Some("onboard") => println!("{}", ritual::ritual(tool::now_for(&store.load_settings()).tz.name(), store.load_settings().name.as_deref())),
        None if !std::io::stdin().is_terminal() => {
            if let Err(e) = sidecar_loop(&store) {
                eprintln!("gray-life: {e}");
                std::process::exit(1);
            }
        }
        _ => match words_to_args(&args).and_then(|a| tool::call(&store, Ctx::detect(), &a)) {
            Ok(t) => println!("{t}"),
            Err(e) => {
                eprintln!("gray-life: {e}");
                std::process::exit(1);
            }
        },
    }
}
