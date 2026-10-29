//! The first-run ritual and the check-in prompts.
//!
//! The ritual is OpenClaw's BOOTSTRAP.md shape ("a ritual, not a gate";
//! the user's request comes first; persist what you learn; never repeat)
//! with Hermes' first-run conduct (one question at a time, never a form,
//! a skip is taken once and not re-asked). The check-ins are Hermes'
//! automation blueprints (morning brief, evening wind-down, weekly review,
//! habit check-in), rewritten around gray-life's own data. Cron runs have
//! no chat context, so each prompt is self-contained; `[SILENT]` drops the
//! message when there is nothing worth sending.

pub fn ritual(detected_tz: &str, known_name: Option<&str>) -> String {
    let name_beat = match known_name {
        Some(n) => format!("1. Name: you already call them {n}. Skip this beat."),
        None => "1. Name: ask what they'd like you to call them. Save it with `life settings` (name).".into(),
    };
    format!(
        "gray-life first-run (pending). You're becoming this person's always-on assistant and you don't know them yet.\n\
Their request always comes first: if this message asks for real work, do it fully and save this for a quiet moment. \
This is a short conversation, not a questionnaire. One question per message, in your own words, and never list these steps.\n\
{name_beat}\n\
2. Timezone: this machine says {detected_tz}. Check it with them in passing (\"it's 14:05 for you, right?\"). Save it with `life settings` (timezone, IANA name).\n\
3. What to keep track of: ask what's going on for them right now and what they want help staying on top of (habits, things to do, things coming up). \
Create the first entries right away with `life add`, then tell them in one line what you saved.\n\
4. Check-ins: offer a morning and an evening check-in and a weekly review; ask what times suit them and when they don't want to hear from you. \
Restate it as one plain sentence (\"Every day at 08:00 your time I'll send…\"), and on a yes create them with `life checkins` and set quiet_hours with `life settings`.\n\
5. Close with one line on what they can just tell you (\"I did the gym\", \"dentist friday 4pm\"), then call `life onboarding done`.\n\
Durable preferences you learn (how they like to be talked to, what to never do) go to the `memory` tool (scope user) as directives: Prefer… / Always… / Never….\n\
If they want to skip or stop, call `life onboarding skip` and don't bring it up again."
    )
}

/// The `/life onboard` kickoff turn.
pub const KICKOFF: &str = "Let's do the gray-life first-run setup now: get to know me, ask one thing at a time, and set up what I want tracked.";

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
