//! New: plain text, for bots (flag B) and anyone who sets it in user
//! config: no colors, cursor moves, art or pauses. Menus are listed and
//! answered a line at a time, lists are numbered lines, messages are
//! written a line at a time, and the message boards are line commands
//! (`plainmsg.rs`). Doors still draw their own screens.

use std::io;

use crate::editor::{EditCtx, EditResult};
use crate::input::ReadOpts;
use crate::menu::{FIRST_CMD, Menu};
use crate::session::{BarPick, BarSpec, Session};

/// Longest line typed into a message; longer ones are wrapped at 79.
const LINE_MAX: usize = 250;
/// Lines in a message written in plain text.
const MAX_LINES: usize = 400;

/// Whether `text_mode` (blank, `plain` or `ansi`) means plain text for a
/// caller who is (or isn't) a bot.
pub fn wants_plain(text_mode: &str, bot: bool) -> bool {
    match text_mode {
        "plain" => true,
        "ansi" => false,
        _ => bot,
    }
}

impl Session {
    pub(crate) fn plain(&self) -> bool {
        self.screen.plain()
    }

    /// Plain text or not, from the caller's setting.
    pub(crate) fn apply_text_mode(&mut self) {
        let on = wants_plain(&self.user.text_mode, crate::aimemory::is_bot(&self.user));
        if on != self.plain() {
            // The menu's listed again in the new mode.
            self.menu.plain_listed.clear();
        }
        self.screen.set_plain(on);
    }

    /// After logging in: what's waiting, in a line, in place of the logon
    /// sequence.
    pub(crate) async fn plain_logon(&mut self) -> io::Result<()> {
        let mail = self.mail_waiting();
        let new = self.msg_stats(false).unread;
        let online = self.state.nodes.online().len();
        self.newline();
        let name = self.state.config().bbs.name.clone();
        self.write(&format!("{name}: logged in as {}, plain text. Type ? at a prompt for the commands.", self.user.handle));
        self.newline();
        self.write(&format!("Mail waiting: {mail}. New posts: {new}. Callers online: {online}. Minutes left: {}.", self.minutes_left()));
        self.newline();
        Ok(())
    }

    /// A menu as a list of its commands (when `list`), then a prompt
    /// answered with a line. Returns the command typed.
    pub(crate) async fn plain_menu(&mut self, menu: &Menu, list: bool) -> io::Result<String> {
        let name = if menu.prompt_name.is_empty() { self.menu.current.to_lowercase() } else { menu.prompt_name.clone() };
        if list {
            self.newline();
            let title = if menu.title.is_empty() { name.clone() } else { crate::editor::strip_colours(&menu.title) };
            self.write(&title);
            self.newline();
            let mut listed: Vec<&str> = Vec::new();
            for c in &menu.commands {
                // A key once: the menu's own command and GLOBAL's may share it.
                if c.hidden || c.desc.is_empty() || c.keys == FIRST_CMD || !self.acs(&c.acs) || listed.contains(&c.keys.as_str()) {
                    continue;
                }
                listed.push(&c.keys);
                self.write(&format!("  {:<6} {}", c.keys.to_lowercase(), crate::editor::strip_colours(&c.desc)));
                self.newline();
            }
        }
        self.write(&format!("{name}> "));
        let s = self.read_string(ReadOpts::new(60)).await?;
        self.newline();
        Ok(s.trim().to_uppercase())
    }

    /// `area_bar` in plain text: the rows numbered, then a line: a number
    /// picks one, a key letter (with a number after it for a row) runs
    /// that, `<` goes up a level, `q` quits.
    pub(crate) async fn area_bar_plain(&mut self, spec: &BarSpec<'_>) -> io::Result<BarPick> {
        let row_of = |label: u32| spec.labels.iter().find(|l| l.0 == label).map(|l| l.1);
        self.newline();
        self.write(spec.header.trim_end());
        self.newline();
        for r in spec.rows {
            self.write(crate::editor::strip_colours(&r.plain).trim_end());
            self.newline();
        }
        let mut keys: Vec<String> = spec.keys.chars().map(|c| c.to_ascii_lowercase().to_string()).collect();
        keys.push("q".into());
        if spec.can_go_up {
            keys.push("<".into());
        }
        let cur = spec.rows.get(spec.start).map_or(0, |r| r.n);
        loop {
            self.write(&format!("number, or {}> ", keys.join(" ")));
            let s = self.read_string(ReadOpts::new(30)).await?;
            self.newline();
            let mut words = s.split_whitespace();
            let first = words.next().unwrap_or("").to_ascii_uppercase();
            let then = words.next().and_then(|w| w.parse::<u32>().ok());
            if let Ok(label) = first.parse::<u32>() {
                match row_of(label) {
                    Some(n) => return Ok(BarPick::Area(n)),
                    None => continue,
                }
            }
            match first.as_str() {
                "Q" => return Ok(BarPick::Quit),
                "<" if spec.can_go_up => return Ok(BarPick::Up),
                "" => return Ok(BarPick::Stay),
                k if k.chars().count() == 1 && spec.keys.contains(k) => {
                    let row = then.and_then(row_of).unwrap_or(cur);
                    return Ok(BarPick::Key(k.chars().next().unwrap_or(' '), row));
                }
                _ => self.write("What? "),
            }
        }
    }

    /// The message editor in plain text: lines typed one at a time, `.`
    /// alone to save, `/abort` to give up.
    pub(crate) async fn plain_edit(&mut self, ctx: &EditCtx<'_>) -> io::Result<Option<EditResult>> {
        self.newline();
        self.write(&format!("Title: {}   To: {}", ctx.subject, ctx.to));
        self.newline();
        if !ctx.text.is_empty() {
            self.write("Now:");
            self.newline();
            for l in &ctx.text {
                self.write(&crate::editor::strip_colours(l));
                self.newline();
            }
            self.write("Type the whole text again.");
            self.newline();
        }
        self.write("Type the message a line at a time. A line with only . saves it; /abort gives up.");
        self.newline();
        let mut lines: Vec<String> = Vec::new();
        loop {
            let l = self.read_string(ReadOpts::new(LINE_MAX)).await?;
            self.newline();
            if self.hangup {
                return Ok(None);
            }
            match l.trim() {
                "." => break,
                "/abort" => {
                    self.write("Not saved.");
                    self.newline();
                    return Ok(None);
                }
                _ => {}
            }
            if l.chars().count() > 79 {
                lines.extend(crate::multinode::wrap(&l, 79));
            } else {
                lines.push(l);
            }
            if lines.len() >= MAX_LINES {
                self.write(&format!("That's {MAX_LINES} lines, the most; saving."));
                self.newline();
                break;
            }
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            self.write("Nothing written; not saved.");
            self.newline();
            return Ok(None);
        }
        Ok(Some(EditResult { lines, subject: ctx.subject.clone(), anonymous: ctx.anonymous, autosig: ctx.autosig }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bots_get_plain_unless_they_say() {
        assert!(wants_plain("", true));
        assert!(!wants_plain("", false));
        assert!(wants_plain("plain", false));
        assert!(!wants_plain("ansi", true));
    }
}
