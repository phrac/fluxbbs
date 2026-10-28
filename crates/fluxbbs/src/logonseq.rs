//! `*O`: the logon sequence. New; the original's was the STARTUP menu's
//! FIRSTCMD list, edited like any menu. Here the SysOp orders the steps
//! and turns them on and off in `logon.toml`, kept with the board (the
//! menus may be shared, or baked into an image), and the start menu's
//! commands are built from it. Without the file STARTUP runs as it is.

use std::io;
use std::path::PathBuf;

use fluxbbs_term::Slot;
use fluxbbs_term::codes::resize;
use serde::{Deserialize, Serialize};

use crate::cfgedit::yes_no;
use crate::config::Config;
use crate::menu::{Command, FIRST_CMD, Menu};
use crate::session::Session;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// One of `KINDS`.
    pub kind: String,
    #[serde(default = "yes")]
    pub on: bool,
    /// The screen a `screen` step shows.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub screen: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

/// The steps there are: kind, what the editor calls it.
pub const KINDS: [(&str, &str); 17] = [
    ("infoforms", "Infoforms to fill in"),
    ("mail", "Waiting mail"),
    ("newuser", "New users stop here (NEWUSER)"),
    ("voting", "New user voting"),
    ("callers", "Last callers"),
    ("pause", "Pause"),
    ("mandatory", "Mandatory messages"),
    ("newposts", "New posts"),
    ("online", "Who's online"),
    ("automsg", "Automessage"),
    ("stats", "Your stats (USERSTAT)"),
    ("oneliners", "Oneliners"),
    ("screen", "Show a screen"),
    ("fastlogon", "Fast logon: offer to skip the rest"),
    ("slots", "A free slot pull (each call, a few a day)"),
    ("email", "Email address, confirmed by a code ([email])"),
    ("lottery", "Last night's lottery draw, for those with tickets in it"),
];

/// STARTUP's stock order.
const STOCK: [&str; 13] = [
    "infoforms", "mail", "newuser", "voting", "callers", "pause", "mandatory", "newposts", "online", "pause",
    "automsg", "stats", "oneliners",
];

impl Sequence {
    pub fn stock() -> Self {
        let steps = STOCK.iter().map(|k| Step { kind: k.to_string(), on: true, screen: String::new() }).collect();
        Self { steps }
    }

    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        crate::extprog::load(&cfg.resolve(&cfg.paths.logon))
    }

    pub fn save(&self, cfg: &Config) -> anyhow::Result<()> {
        let text = format!(
            "# The logon sequence (*O): the steps a caller goes through after\n\
             # logging in, in order. `on = false` skips one. A screen step shows\n\
             # `screen` from the text directory. Kinds:\n# {}\n\n{}",
            KINDS.iter().map(|k| k.0).collect::<Vec<_>>().join(", "),
            toml::to_string_pretty(self).expect("sequence serializes")
        );
        let path = cfg.resolve(&cfg.paths.logon);
        let tmp = path.with_extension("toml.new");
        std::fs::write(&tmp, text)?;
        crate::keep_mode(&path, &tmp);
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// The start menu's commands: each step that's on, then the menu's
    /// fallback (the original went there from the oneliners).
    pub fn commands(&self, menus: &[PathBuf], fallback: &str) -> Vec<Command> {
        let mut out: Vec<Command> = self.steps.iter().filter(|s| s.on).flat_map(|s| step_commands(s, menus, fallback)).collect();
        out.push(first("-^", fallback, "", "Go on to the main menu"));
        out
    }
}

pub fn label(s: &Step) -> String {
    let name = KINDS.iter().find(|k| k.0 == s.kind).map_or(s.kind.as_str(), |k| k.1);
    if s.kind == "screen" { format!("Show {}", s.screen.to_ascii_uppercase()) } else { name.to_string() }
}

fn first(command: &str, param: &str, acs: &str, desc: &str) -> Command {
    Command { keys: FIRST_CMD.into(), command: command.into(), param: param.into(), acs: acs.into(), desc: desc.into(), ..Command::default() }
}

/// What STARTUP ran for each step.
fn step_commands(s: &Step, menus: &[PathBuf], fallback: &str) -> Vec<Command> {
    match s.kind.as_str() {
        "infoforms" => vec![first("IM", "", "", "Check for infoforms")],
        "mail" => vec![first("EC", "", "", "Scan for waiting mail")],
        "newuser" => vec![first("-^", "NEWUSER", "!s50", "Check if new user...")],
        "voting" => vec![first("NV", "", "s50", "Do new user voting (if enabled)")],
        "callers" => vec![first("LC", "", "", "Display last callers")],
        "pause" => vec![first("-P", "", "", "[pause]")],
        "mandatory" => vec![first("MM", "", "", "Scan for mandatory messages")],
        "newposts" => vec![first("MW", "", "", "Offer new posts")],
        "online" => vec![first("XL", "", "", "Show who's online")],
        "automsg" => vec![
            first("AS", "", "", "Display automessage"),
            first("?N", "|u2:|u1:|u3: |u1Change the automessage|u2? ", "", "Ask change automessage"),
            first("AW", "", "*", "If OkFlag set, alter automessage"),
        ],
        "stats" => vec![first("-F", "USERSTAT", "", "Display user's statistics")],
        // The oneliner menu's FIRSTCMDs, up to where it goes on to MAIN,
        // so it can be anywhere in the sequence.
        "oneliners" => Menu::load(menus, "ONELINER")
            .map(|m| {
                m.commands
                    .into_iter()
                    .filter(|c| c.keys == FIRST_CMD)
                    .take_while(|c| !matches!(c.command.as_str(), "-^" | "-/" | "-\\"))
                    .collect()
            })
            .unwrap_or_default(),
        "screen" if !s.screen.is_empty() => vec![first("-F", &s.screen, "", "Show a screen")],
        "slots" => vec![first("AL", "free", "", "A free slot pull")],
        "email" => vec![first("UV", "logon", "", "Email address and its code")],
        "lottery" => vec![first("!P", "draw", "", "Last night's lottery draw")],
        // New: a validated caller may go straight on to the main menu.
        "fastlogon" => vec![
            first("?N", "|u2:|u1:|u3: |u1Fast logon, straight to the main menu|u2? ", "s50", "Ask fast logon"),
            first("-^", fallback, "s50*", "If OkFlag set, skip the rest"),
        ],
        _ => Vec::new(),
    }
}

/// `menuLoad`'s hook: the start menu runs the sequence, if there is one.
pub fn apply(cfg: &Config, menus: &[PathBuf], name: &str, menu: &mut Menu) {
    if !name.eq_ignore_ascii_case(&cfg.logon.start_menu) {
        return;
    }
    match Sequence::load(cfg) {
        Ok(Some(seq)) => menu.commands = seq.commands(menus, &menu.fallback),
        Ok(None) => {}
        Err(e) => tracing::warn!("{e:#}"),
    }
}

impl Session {
    /// `*O`: order the logon steps and turn them on and off.
    pub async fn logon_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: logon sequence edit", self.user.handle);
        loop {
            let cfg = self.state.config();
            let mut seq = match Sequence::load(&cfg) {
                Ok(s) => s.unwrap_or_else(Sequence::stock),
                Err(e) => {
                    self.show_line(&format!("|U0{e:#}")).await?;
                    return Ok(());
                }
            };
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(&format!("   {} ", cfg.bbs.name));
            self.set_slot(Slot::TextHi);
            self.write("Logon Sequence");
            self.newline();
            self.rule().await?;
            self.set_slot(Slot::Text);
            self.write(" Num  On   Step");
            self.newline();
            for (i, s) in seq.steps.iter().enumerate() {
                self.set_slot(if s.on { Slot::Info } else { Slot::InfoLo });
                self.write(&format!(" {} {} {}", resize(&(i + 1).to_string(), 4), resize(yes_no(s.on), 4), label(s)));
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self
                .cfg_list_prompt(&[("T", "oggle"), ("M", "ove"), ("I", "nsert"), ("D", "elete"), ("R", "eset"), ("Esc", " Quit")])
                .await?
            else {
                return Ok(());
            };
            let n = seq.steps.len() as u32;
            let changed = match cmd {
                'T' => {
                    self.newline();
                    match self.cfg_ask_number("Turn which step on or off?", 1, n.max(1)).await? {
                        Some(i) if n > 0 => {
                            let s = &mut seq.steps[i as usize - 1];
                            s.on = !s.on;
                            true
                        }
                        _ => false,
                    }
                }
                'I' => self.logon_insert(&mut seq).await?,
                'R' => {
                    self.newline();
                    self.write_with_controls("|U1Put back the stock sequence? ");
                    let yes = self.yes_no(false).await?;
                    if yes {
                        seq = Sequence::stock();
                    }
                    yes
                }
                _ => self.list_change(&mut seq.steps, cmd, "step", |_| unreachable!()).await?,
            };
            if changed && let Err(e) = seq.save(&cfg) {
                self.show_line(&format!("|U0{e:#}")).await?;
                self.prompt_key().await?;
            }
        }
    }

    /// Insert a step: which kind, where, and for a screen, which.
    async fn logon_insert(&mut self, seq: &mut Sequence) -> io::Result<bool> {
        self.newline();
        for (i, k) in KINDS.iter().enumerate() {
            self.set_slot(Slot::Info);
            self.write(&format!(" {} {}", resize(&(i + 1).to_string(), 4), k.1));
            self.newline();
        }
        let Some(kind) = self.cfg_ask_number("Insert which step?", 1, KINDS.len() as u32).await? else { return Ok(false) };
        let kind = KINDS[kind as usize - 1].0;
        let mut screen = String::new();
        if kind == "screen" {
            self.write_with_controls("|U1Screen's name (e.g. BULLETIN)|U2: |U3");
            screen = self.read_string(crate::input::ReadOpts::new(8).case(crate::input::Case::Upper)).await?;
            self.newline();
            if screen.is_empty() {
                return Ok(false);
            }
        }
        let n = seq.steps.len() as u32;
        let Some(at) = self.cfg_ask_number("Insert before which step?", 1, n + 1).await? else { return Ok(false) };
        seq.steps.insert(at as usize - 1, Step { kind: kind.into(), on: true, screen });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_matches_startup() {
        let menus = [PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../menus")];
        let startup = Menu::load(&menus, "STARTUP").unwrap();
        let want: Vec<(String, String)> = startup.commands.iter().map(|c| (c.command.clone(), c.param.clone())).collect();
        let built = Sequence::stock().commands(&menus, "MAIN");
        let got: Vec<(String, String)> = built.iter().map(|c| (c.command.clone(), c.param.clone())).collect();
        // Stock STARTUP goes to ONELINER, which ends by going to MAIN.
        let mut want_inline: Vec<(String, String)> = want[..want.len() - 1].to_vec();
        let one = Menu::load(&menus, "ONELINER").unwrap();
        want_inline.extend(one.commands.iter().filter(|c| c.keys == FIRST_CMD).map(|c| (c.command.clone(), c.param.clone())));
        assert_eq!(got, want_inline);
    }

    #[test]
    fn off_steps_are_skipped_and_round_trip() {
        let mut seq = Sequence::stock();
        seq.steps.retain(|s| s.kind != "oneliners");
        seq.steps[0].on = false;
        seq.steps.push(Step { kind: "screen".into(), on: true, screen: "BULLETIN".into() });
        let back: Sequence = toml::from_str(&toml::to_string_pretty(&seq).unwrap()).unwrap();
        assert_eq!(back, seq);
        let cmds = seq.commands(&[PathBuf::from("/nonexistent")], "MAIN");
        assert!(!cmds.iter().any(|c| c.command == "IM"));
        assert_eq!(cmds[cmds.len() - 2].param, "BULLETIN");
        assert_eq!(cmds.last().unwrap().param, "MAIN");
    }

    #[test]
    fn fast_logon_asks_then_skips_to_the_fallback() {
        let mut seq = Sequence::stock();
        seq.steps.insert(0, Step { kind: "fastlogon".into(), on: true, screen: String::new() });
        let cmds = seq.commands(&[PathBuf::from("/nonexistent")], "MAIN");
        assert_eq!((cmds[0].command.as_str(), cmds[0].acs.as_str()), ("?N", "s50"));
        assert_eq!((cmds[1].command.as_str(), cmds[1].param.as_str(), cmds[1].acs.as_str()), ("-^", "MAIN", "s50*"));
        assert_eq!(cmds[2].command, "IM");
    }
}
