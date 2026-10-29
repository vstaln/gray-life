//! The first-run setup and the check-in prompts.
//!
//! Setup borrows OpenClaw's BOOTSTRAP.md conduct (the user's request comes
//! first, never repeat once done) and Hermes' first-run style (one question
//! per message, a skip is taken once), but it only collects the settings
//! the tracker needs: timezone (required), check-ins, quiet hours, an
//! optional name. No get-to-know-you questions. The check-ins are Hermes'
//! automation blueprints (morning brief, evening wind-down, weekly review)
//! rewritten around gray-life's data; each is self-contained because cron
//! runs have no chat context, and `[SILENT]` drops an empty one.

use crate::store::Settings;

pub fn ritual(settings: &Settings) -> String {
    let tz = match &settings.timezone {
        Some(z) => format!("- Timezone: set to {z}. Skip."),
        None => "- Timezone (required): ask which timezone or city they're in. Never assume it from this machine's clock; \
the server may be anywhere. Save it with `life settings` (timezone, IANA name). Nothing with a time can be scheduled until this is set."
            .into(),
    };
    let name = match &settings.name {
        Some(n) => format!("- Name: set to {n}. Skip."),
        None => "- Name (optional): what they'd like to be called. `life settings` (name). Fine to skip.".into(),
    };
    let checkins = if settings.checkins.is_empty() {
        "- Check-ins: offer a morning summary, an evening wrap-up and a weekly review; ask which they want and at what time. \
Repeat it back in one sentence and create them with `life checkins` on a yes. None is a fine answer."
    } else {
        "- Check-ins: already set. Skip."
    };
    let quiet = match &settings.quiet_hours {
        Some(q) => format!("- Quiet hours: {q}. Skip."),
        None => "- Quiet hours: ask when you shouldn't message them (e.g. 23:00-08:00). `life settings` (quiet_hours).".into(),
    };
    format!(
        "gray-life setup (pending). A few settings are needed before this tracker can schedule anything.\n\
If this message asks for something else, do that first and bring setup up afterwards. Ask one thing per message, briefly; \
it's settings, not an interview, so don't ask about their life, goals or personality.\n\
{tz}\n{name}\n{checkins}\n{quiet}\n\
Finish with one line on how to use it (\"add dentist fri 4pm\", \"I did the gym\", /life), then call `life onboarding done`. \
If they want to skip, call `life onboarding skip`, but a missing timezone will still be asked for before anything is scheduled."
    )
}

/// The `/life onboard` kickoff turn.
pub const KICKOFF: &str = "Set up gray-life with me now: go through the settings it needs (timezone first), one question at a time.";

pub fn checkin_prompt(kind: &str) -> &'static str {
    match kind {
        "morning" => "Morning check-in (gray-life). Run the `life` tool with action \"today\". Send one short, warm message: \
what's on today (events, due and overdue todos), which habits are due, and the one thing worth focusing on first. \
No headings, no lists longer than a few items. If there is nothing tracked at all, reply exactly [SILENT].",
        "evening" => "Evening check-in (gray-life). Run the `life` tool with action \"today\". Send one short, calm message: \
ask about the habits still open today (warm, never judgmental, one line each at most), mention anything still due or happening tomorrow, \
and invite them to tell you how the day went so you can log it. If nothing is tracked and nothing is open, reply exactly [SILENT].",
        "weekly" => "Weekly review (gray-life). Run the `life` tool with action \"review\". Send one message: what got done this week, \
how each habit went (streaks, kind about misses), what's slipping or overdue, and what's coming next week. \
End with 1-3 suggested focuses sized to what's actually on their plate, and ask if they want to adjust anything. \
Suggestions only; don't change entries yourself. If nothing is tracked, reply exactly [SILENT].",
        _ => "",
    }
}

pub const CHECKIN_KINDS: [&str; 3] = ["morning", "evening", "weekly"];
