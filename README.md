# gray-life

A personal life tracker for gray: todos, habits (with streaks), what's going
on (events), a daily log, morning/evening/weekly check-ins, and a first-run
ritual for always-on chat use. One store for every chat and the REPL
(`$GRAY_USER_HOME/life/`). See DESIGN.md for what was ported from OpenClaw
and Hermes.

Install:

    cargo build --release
    gray plugin install -y ./target/release/gray-life
    gray plugin enable life --global   # so Discord/Telegram chat homes load it

Use: just talk ("I did the gym", "dentist fri 4pm"), or `/life`,
`/life onboard`, `/life review`, `gray life today`.
