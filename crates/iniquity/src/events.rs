//! Events (`EVENTS.PAS`, `*V`): jobs run at a time each day.
//!
//! The original checked for events on each node between calls, ran them
//! through DOS, and could take the modem off the hook meanwhile. Here one
//! scheduler in the server runs them (so there's no per-node setting, and
//! no modem), and what each last ran is kept in the database. A command
//! is run through the shell, except the built-in jobs `pack-messages`
//! and `pack-files`, which run inside the server: a second program
//! packing the same database while the board runs would fight it for
//! locks; and `dos COMMAND`, which runs a DOS program in the built-in
//! machine (a door's nightly maintenance). `force` still disconnects everyone first, for jobs that want
//! the board to themselves.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use iniquity_term::Slot;
use iniquity_term::codes::{resize, squish};
use serde::{Deserialize, Serialize};

use crate::BbsState;
use crate::cfgedit::{Field, Form, yes_no};
use crate::nodes::NodeEvent;
use crate::session::Session;

/// `tEventRec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub desc: String,
    #[serde(default = "yes")]
    pub active: bool,
    /// HH:MM, 24-hour.
    pub time: String,
    /// Disconnect everyone first.
    #[serde(default)]
    pub force: bool,
    /// If the BBS was down at the time, run it when it's back (that day).
    #[serde(default)]
    pub run_missed: bool,
    /// A shell command, `pack-messages` / `pack-files`, or `dos COMMAND`.
    pub command: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Events {
    #[serde(rename = "event", default)]
    pub list: Vec<Event>,
}

impl Events {
    pub fn to_toml(&self) -> String {
        format!(
            "# Events: jobs run once a day at `time` (HH:MM). `command` goes to the\n\
             # shell (in the BBS directory), except the built-in jobs pack-messages\n\
             # and pack-files, and `dos COMMAND` (a DOS program or batch file in the\n\
             # built-in machine). `force` disconnects callers first; `run_missed`\n\
             # runs an event the BBS was down for, when it's back up the same day.\n\n{}",
            toml::to_string_pretty(self).expect("events serialize")
        )
    }
}

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// Is `e` due at `now`, having last run on `last`? An event runs once a
/// day, at its time (or within the scheduler's two-minute window after
/// it), or any time later that day with `run_missed`.
pub fn due(e: &Event, last: Option<NaiveDate>, now: NaiveDateTime) -> bool {
    let Some(t) = parse_time(&e.time) else { return false };
    let at = now.date().and_time(t);
    e.active && last != Some(now.date()) && now >= at && (e.run_missed || now - at < chrono::Duration::minutes(2))
}

/// How often the scheduler looks.
const TICK: Duration = Duration::from_secs(20);
/// How long a forced event waits for callers to go.
const FORCE_WAIT: Duration = Duration::from_secs(60);

/// The scheduler: started with the server, runs for as long as it does.
pub async fn scheduler(state: Arc<BbsState>) {
    let mut tick = tokio::time::interval(TICK);
    loop {
        tick.tick().await;
        let now = crate::clock::now().naive_local();
        for e in state.events().list.iter() {
            let last = match state.sys.event_last(&e.desc) {
                Ok(l) => l,
                Err(err) => {
                    tracing::error!("event {}: {err}", e.desc);
                    continue;
                }
            };
            if !due(e, last, now) {
                continue;
            }
            // Marked first, so a slow job isn't started twice.
            if let Err(err) = state.sys.set_event_last(&e.desc, now.date()) {
                tracing::error!("event {}: {err}", e.desc);
                continue;
            }
            tokio::spawn(run(state.clone(), e.clone()));
        }
    }
}

/// `eventRun`.
pub async fn run(state: Arc<BbsState>, e: Event) {
    tracing::info!("event: {} @ {}", e.desc, crate::clock::now().format("%H:%M"));
    if e.force {
        let text = state.strings().get("s299").to_string();
        let kicked = state.nodes.send_all(0, NodeEvent::Kick { text });
        if kicked > 0 {
            tracing::info!("event {}: disconnecting {kicked} caller(s)", e.desc);
            let deadline = tokio::time::Instant::now() + FORCE_WAIT;
            while !state.nodes.online().is_empty() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let left = state.nodes.online().len();
            if left > 0 {
                tracing::warn!("event {}: {left} caller(s) still on (in a door or script?); running anyway", e.desc);
            }
        }
    }
    match run_command(&state, &e.command).await {
        Ok(()) => tracing::info!("event {}: done", e.desc),
        Err(err) => tracing::error!("event {}: {err:#}", e.desc),
    }
}

async fn run_command(state: &BbsState, cmd: &str) -> anyhow::Result<()> {
    match cmd.trim() {
        "" => Ok(()),
        "pack-messages" => {
            let mut total = 0;
            // Email too: deleted mail would otherwise stay for ever.
            let areas = state.areas();
            for a in areas.areas.iter().chain([&areas.email]) {
                total += state.msgs.pack(&a.file, a.max_msgs)?;
            }
            crate::attach::remove_orphans(state)?;
            tracing::info!("event: packed message areas, {total} messages removed");
            Ok(())
        }
        "pack-files" => {
            let cfg = state.config();
            let mut total = 0;
            for a in &state.fareas().areas {
                let gone = state.files.prune_missing(&a.file, &cfg.resolve(&a.path))?;
                for name in &gone {
                    tracing::info!("event: file area {}: {name} is gone, record removed", a.name);
                }
                total += gone.len();
            }
            tracing::info!("event: packed file areas, {total} records removed");
            Ok(())
        }
        dos if dos.starts_with("dos ") => crate::doors::run_dos_event(state, dos[4..].trim()).await,
        shell => {
            let dir = state.config().resolve(Path::new("."));
            let out = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(shell)
                .current_dir(&dir)
                .stdin(std::process::Stdio::null())
                .output()
                .await
                .with_context(|| format!("running {shell:?}"))?;
            for line in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) {
                tracing::info!("event: | {line}");
            }
            anyhow::ensure!(out.status.success(), "{shell:?} exited with {}", out.status);
            Ok(())
        }
    }
}

impl Session {
    /// `cfgEventEditor` (`*V`).
    pub async fn event_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: event edit", self.user.handle);
        loop {
            let mut events = self.state.events().list.clone();
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Active  Time   Force  Description                     Command");
            self.newline();
            self.rule().await?;
            for (i, e) in events.iter().enumerate() {
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(yes_no(e.active), 7),
                    resize(&e.time, 6),
                    resize(yes_no(e.force), 6),
                    resize(&e.desc, 31),
                    squish(&e.command, 22)
                ));
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            if cmd == 'E' {
                self.newline();
                let n = events.len() as u32;
                if n == 0 {
                    continue;
                }
                if let Some(num) = self.cfg_ask_number("Begin edit with which event?", 1, n).await? {
                    self.event_edit(num).await?;
                }
                continue;
            }
            let fresh = |list: &[Event]| Event {
                desc: format!("New event {}", list.len() + 1),
                active: false,
                time: "04:00".into(),
                force: false,
                run_missed: true,
                command: String::new(),
            };
            if self.list_change(&mut events, cmd, "event", fresh).await? {
                self.state.set_events(Events { list: events }).map_err(io::Error::other)?;
            }
        }
    }

    async fn event_edit(&mut self, mut num: u32) -> io::Result<()> {
        loop {
            let mut list = self.state.events().list.clone();
            let n = list.len() as u32;
            let idx = num as usize - 1;
            let mut e = list[idx].clone();
            let mut f = Form::new(&format!("{} Event Editor", self.state.config().bbs.name));
            f.value_col(25);
            f.label("Current event", 12, format!("{num} of {n}"));
            f.item('A', "Description", 30, &e.desc, "What this event does (also how its last run is remembered).");
            f.item('B', "Active", 3, yes_no(e.active), "Run this event?");
            f.item('C', "Time", 5, &e.time, "When to run it each day [format: HH:MM, 24-hour].");
            f.item('D', "Force callers off", 3, yes_no(e.force), "Disconnect everyone before it runs?");
            f.item('E', "Run if missed", 3, yes_no(e.run_missed), "If the BBS was down at the time, run it when it's back that day?");
            f.item('F', "Command", 53, &e.command, "Shell command, run in the BBS directory; pack-messages / pack-files; or dos COMMAND.");
            f.item('!', "Run it now", 0, "", "");
            f.item('[', "Previous event", 0, "", "");
            f.item(']', "Next event", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(30)).await?.filter(|v| !v.trim().is_empty()) {
                            e.desc = v;
                        }
                    }
                    'B' => e.active = self.cfg_bool(&mut f, key, e.active).await?,
                    'C' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field { allowed: "0123456789:", ..Field::TEXT.max(5) }).await? {
                            match parse_time(&v) {
                                Some(t) => e.time = t.format("%H:%M").to_string(),
                                None => self.cfg_info(&mut f, "That's not a time: use HH:MM, 00:00 to 23:59."),
                            }
                            self.cfg_set(&mut f, key, e.time.clone());
                        }
                    }
                    'D' => e.force = self.cfg_bool(&mut f, key, e.force).await?,
                    'E' => e.run_missed = self.cfg_bool(&mut f, key, e.run_missed).await?,
                    'F' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(200)).await? {
                            e.command = v;
                        }
                    }
                    '!' => {
                        if self.cfg_ask(&mut f, "Run this event now?", false).await? {
                            tracing::info!("{}: running event {} by hand", self.user.handle, e.desc);
                            tokio::spawn(run(self.state.clone(), e.clone()));
                            self.cfg_info(&mut f, "Started; see the log for how it went.");
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if list[idx] != e {
                list[idx] = e;
                self.state.set_events(Events { list }).map_err(io::Error::other)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn ev(time: &str, run_missed: bool) -> Event {
        Event { desc: "Nightly".into(), active: true, time: time.into(), force: false, run_missed, command: String::new() }
    }

    #[test]
    fn when_events_are_due() {
        let e = ev("04:00", false);
        let yesterday = NaiveDate::from_ymd_opt(2026, 10, 16);
        assert!(!due(&e, yesterday, at(3, 59)), "not yet");
        assert!(due(&e, yesterday, at(4, 0)));
        assert!(due(&e, yesterday, at(4, 1)), "within the window");
        assert!(!due(&e, yesterday, at(9, 0)), "missed, and not run_missed");
        assert!(!due(&e, Some(at(4, 0).date()), at(4, 1)), "already ran today");
        assert!(due(&ev("04:00", true), yesterday, at(9, 0)), "run_missed catches up");
        assert!(due(&ev("04:00", true), None, at(9, 0)), "never ran");
        assert!(!due(&Event { active: false, ..e.clone() }, yesterday, at(4, 0)));
        assert!(!due(&ev("25:00", true), yesterday, at(9, 0)), "a bad time never runs");
    }

    #[test]
    fn events_round_trip() {
        let list = Events { list: vec![ev("23:30", true), Event { force: true, command: "pack-files".into(), ..ev("04:00", false) }] };
        assert_eq!(toml::from_str::<Events>(&list.to_toml()).unwrap(), list);
        assert_eq!(parse_time(" 7:05 "), NaiveTime::from_hms_opt(7, 5, 0));
    }

    #[tokio::test]
    async fn shell_commands_report_failure() {
        let state = crate::tests::state();
        assert!(run_command(&state, "true").await.is_ok());
        assert!(run_command(&state, "exit 3").await.unwrap_err().to_string().contains("exited"));
        assert!(run_command(&state, "pack-messages").await.is_ok());
    }

    #[tokio::test]
    async fn dos_commands_run_in_the_machine() {
        let root = std::env::temp_dir().join(format!("inq-dos-event-{}", std::process::id()));
        std::fs::create_dir_all(root.join("doors/dos")).unwrap();
        let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../iniquity-dos/tests/dos/hello.com");
        std::fs::copy(hello, root.join("doors/dos/HELLO.COM")).unwrap();
        let state = crate::tests::state();
        let mut cfg = (*state.config()).clone();
        cfg.root = root.clone();
        *state.config.write().unwrap() = Arc::new(cfg);
        // HELLO.COM prints and exits with 7.
        let err = run_command(&state, "dos HELLO.COM").await.unwrap_err().to_string();
        assert!(err.contains("exited with 7"), "{err}");
        assert!(run_command(&state, "dos NOTHERE.EXE").await.unwrap_err().to_string().contains("no NOTHERE.EXE"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
