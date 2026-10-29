//! New: plain text, for bots (flag B) and anyone who sets it in user
//! config: no colors, cursor moves, art or pauses. Menus are listed and
//! answered a line at a time, lists are numbered lines, messages are
//! written a line at a time, and the message boards are line commands
//! (`plainmsg.rs`). Doors still draw their own screens.

use std::io;

use crate::editor::{EditCtx, EditResult, strip_colours};
use crate::filebase::FileRec;
use crate::files::{FAreaRef, commas};
use crate::input::{Key, ReadOpts};
use crate::menu::{FIRST_CMD, Menu};
use crate::session::{BarPick, BarSpec, Session};

/// Longest line taken from a plain-text caller (a paragraph); messages
/// wrap it at 79.
pub(crate) const LINE_MAX: usize = 4000;
/// Lines in a message written in plain text.
const MAX_LINES: usize = 400;
/// Files a page of the file list.
const FILE_PAGE: usize = 25;
/// Description lines shown under each file.
const DESC_SHOWN: usize = 4;

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

    /// New: how this caller sees code and Markdown (`markup::Look`):
    /// their themes and code background, in 24-bit color where the
    /// terminal takes it, else the classic 16; no colors in plain text.
    pub(crate) fn look(&self) -> crate::markup::Look {
        let code = crate::markup::theme(&self.user.code_theme);
        let md = crate::markup::theme(&self.user.md_theme);
        let rgb = !self.plain() && (self.screen.truecolor() || self.screen.charset() == fluxbbs_term::Charset::Utf8);
        let (name, back_rgb, dos) = crate::markup::code_back(&self.user.code_bg);
        let shade = if self.plain() {
            String::new()
        } else if !rgb {
            dos.to_string()
        } else if name == "theme" {
            let [r, g, b] = code.colors.as_ref().map_or([58, 58, 58], |c| c.bg);
            format!("48;2;{r};{g};{b}")
        } else {
            back_rgb.to_string()
        };
        crate::markup::Look { code, md, shade, rgb }
    }

    /// A key, or in plain text the first character of a line (Enter
    /// alone for an empty one): for prompts that take a single key.
    pub(crate) async fn read_choice(&mut self) -> io::Result<Key> {
        if !self.plain() {
            return self.read_key().await;
        }
        let s = self.read_string(ReadOpts::new(10)).await?;
        Ok(match s.trim().bytes().next() {
            Some(c) => Key::Char(c),
            None => Key::Enter,
        })
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
            let mut rows = Vec::new();
            for c in &menu.commands {
                // A key once: the menu's own command and GLOBAL's may share it.
                if c.hidden || c.dark || c.desc.is_empty() || c.keys == FIRST_CMD || !self.acs(&c.acs) || listed.contains(&c.keys.as_str()) {
                    continue;
                }
                listed.push(&c.keys);
                rows.push(format!("  {:<6} {}", c.keys.to_lowercase(), crate::editor::strip_colours(&c.desc).trim()));
            }
            // A long menu in two columns, so it fits a screen with the
            // prompt under it (nothing scrolls off the top).
            let half = if rows.len() > 18 && rows.iter().all(|r| r.chars().count() <= 39) { rows.len().div_ceil(2) } else { rows.len() };
            for i in 0..half {
                let line = match rows.get(i + half).filter(|_| half < rows.len()) {
                    Some(right) => format!("{:<40}{right}", rows[i]),
                    None => rows[i].clone(),
                };
                self.write(line.trim_end());
                self.newline();
            }
        }
        // A lightbar menu's Enter picks what the bar starts on, its first
        // option, as in ANSI; the prompt says which.
        let first = (menu.menu_type == crate::menu::MenuType::Lightbar)
            .then(|| menu.commands.iter().find(|c| !c.hidden && !c.dark && !c.desc.is_empty() && c.keys != FIRST_CMD && self.acs(&c.acs)))
            .flatten()
            .map(|c| c.keys.clone());
        match &first {
            Some(k) => self.write(&format!("{name} [{}]> ", k.to_lowercase())),
            None => self.write(&format!("{name}> ")),
        }
        let s = self.read_string(ReadOpts::new(60)).await?;
        self.newline();
        let s = s.trim().to_uppercase();
        Ok(if s.is_empty() { first.unwrap_or_default().to_uppercase() } else { s })
    }

    /// `area_bar` in plain text: the rows numbered, then a line: a number
    /// picks one, a key letter (with a number after it for a row) runs
    /// that, `<` goes up a level, `q` quits.
    pub(crate) async fn area_bar_plain(&mut self, spec: &BarSpec<'_>) -> io::Result<BarPick> {
        // Rows without numbers of their own are numbered from 1.
        let numbered = spec.labels.is_empty();
        let row_of = |label: u32| {
            if numbered {
                spec.rows.get((label as usize).checked_sub(1)?).map(|r| r.n)
            } else {
                spec.labels.iter().find(|l| l.0 == label).map(|l| l.1)
            }
        };
        self.newline();
        self.write(spec.header.trim_end());
        self.newline();
        for (i, r) in spec.rows.iter().enumerate() {
            let text = strip_colours(&r.plain);
            let line = if numbered { format!("{:>4}.{}", i + 1, text.trim_end()) } else { text.trim_end().to_string() };
            self.write(&line);
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
            let then = words.next();
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
                // The row named, else the current one; a number that names
                // no row is asked again.
                k if k.chars().count() == 1 && spec.keys.contains(k) => {
                    let row = match then {
                        None => Some(cur),
                        Some(w) => w.parse::<u32>().ok().and_then(row_of),
                    };
                    match row {
                        Some(row) => return Ok(BarPick::Key(k.chars().next().unwrap_or(' '), row)),
                        None => self.write("No such row. "),
                    }
                }
                _ => self.write("What? "),
            }
        }
    }

    /// The file lister in plain text: each file on a line with its link
    /// (`file:AREA/NAME`, for comments and messages), its description
    /// under it, a page at a time. False if the caller quit.
    pub(crate) async fn plain_file_list(&mut self, a: &FAreaRef, files: &[FileRec], skip_ok: bool) -> io::Result<bool> {
        let s = format!("{} file{} in {}:", files.len(), if files.len() == 1 { "" } else { "s" }, strip_colours(&a.area.name));
        self.write(&s);
        self.newline();
        let pages = files.len().div_ceil(FILE_PAGE);
        for (p, page) in files.chunks(FILE_PAGE).enumerate() {
            for f in page {
                let date = crate::user::fmt_date(f.date);
                let line = format!("{:<18} {:>11} {date}  {:>3} dl  {}", f.name, commas(f.size), f.downloads, crate::links::link_text(&a.area.file, &f.name));
                self.write(&line);
                self.newline();
                for l in f.description.iter().take(DESC_SHOWN) {
                    let l = format!("    {}", strip_colours(l).trim_end());
                    self.write(&l);
                    self.newline();
                }
                if !f.valid {
                    self.write("    (not checked by the SysOp yet)");
                    self.newline();
                }
            }
            if p + 1 < pages {
                let ask = if skip_ok { "Enter for more, s skips this area, q stops> " } else { "Enter for more, q stops> " };
                self.write(ask);
                let s = self.read_string(ReadOpts::new(5)).await?;
                self.newline();
                match s.trim().to_ascii_lowercase().as_str() {
                    "q" => return Ok(false),
                    "s" if skip_ok => return Ok(true),
                    _ => {}
                }
            }
        }
        Ok(true)
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
        let mut fenced = false;
        loop {
            // New: lines keep their indentation (code does), Tab included.
            let l = self.read_string(ReadOpts::new(LINE_MAX).indent()).await?;
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
            add_line(&mut lines, l, &mut fenced);
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

/// A typed line into the message: prose over 79 characters wrapped, but
/// not a fenced code block's lines (from a line of ``` to the next),
/// which wrapping would break.
fn add_line(lines: &mut Vec<String>, l: String, fenced: &mut bool) {
    let fence = l.trim_start().starts_with("```");
    if fence || *fenced || l.chars().count() <= 79 {
        lines.push(l);
    } else {
        lines.extend(crate::multinode::wrap(&l, 79));
    }
    if fence {
        *fenced = !*fenced;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_keeps_its_lines() {
        let long = format!("    let x = \"{}\";", "a".repeat(90));
        let prose = "word ".repeat(20);
        let mut lines = Vec::new();
        let mut fenced = false;
        for l in [prose.clone(), "```rust".into(), "fn main() {".into(), long.clone(), "}".into(), "```".into(), prose.clone()] {
            add_line(&mut lines, l, &mut fenced);
        }
        assert!(!fenced);
        assert_eq!(lines[2..7], ["```rust".to_string(), "fn main() {".into(), long, "}".into(), "```".into()]);
        assert_eq!(lines.len(), 2 + 5 + 2, "prose either side wrapped to 79");
        assert!(lines.iter().filter(|l| l.starts_with("word")).all(|l| l.chars().count() <= 79));
    }

    #[test]
    fn bots_get_plain_unless_they_say() {
        assert!(wants_plain("", true));
        assert!(!wants_plain("", false));
        assert!(wants_plain("plain", false));
        assert!(!wants_plain("ansi", true));
    }
}
