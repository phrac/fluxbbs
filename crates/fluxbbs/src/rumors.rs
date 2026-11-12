//! New: rumors, after ENiGMA½'s rumorz (MIT). Callers add one-liners
//! that go unsigned; one at random shows at logon (`RS`, the `rumor`
//! logon step) or on any screen (`%RM`, the newest `%RL`). `RL` lists
//! them and offers to add one; SysOps also see who wrote each, and
//! delete them there. The original had none.

use std::io;

use fluxbbs_term::codes::{defuse, resize, unlit};
use fluxbbs_term::{Slot, lit};

use crate::input::{DIGITS, ReadOpts};
use crate::session::{Session, Waves, waves};
use crate::sysdata::Rumor;

/// How long a rumor may be (as typed, colour codes and all).
pub const RUMOR_LEN: usize = 70;
/// How many are kept: adding one past it drops the oldest.
pub const RUMORS_KEPT: usize = 100;
/// How many a caller may add a day (SysOps, any number).
pub const RUMORS_DAILY: u32 = 3;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// A line as typed, made fit to keep: no control characters or escapes
/// (nor CP437's `←`, which is one), nor `lit` marks, spaces run
/// together. Colour codes stay, as in the oneliners; everything else is
/// made harmless where it's shown (`defuse`, or `lit` in prompts).
pub fn clean(s: &str) -> String {
    let s: String = unlit(s).chars().map(|c| if c.is_control() || c == fluxbbs_term::cp437::TO_UNICODE[0x1b] { ' ' } else { c }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a rumor says, for telling two apart: no colours, any case.
fn gist(s: &str) -> String {
    crate::editor::strip_colours(s).to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Session {
    /// `%RM` (at random) and `%RL` (the newest): the text, empty if
    /// there are none or rumors are off.
    pub(crate) fn rumor_code(&self, newest: bool) -> String {
        if !self.state.config().features.rumors {
            return String::new();
        }
        let pick = || self.state.sys.rumor(newest).ok().flatten().map(|r| r.text).unwrap_or_default();
        if newest {
            return pick();
        }
        let mut kept = self.rumor_pick.borrow_mut();
        match &*kept {
            Some((at, text)) if at.elapsed() < std::time::Duration::from_secs(10) => text.clone(),
            _ => {
                let text = pick();
                *kept = Some((std::time::Instant::now(), text.clone()));
                text
            }
        }
    }

    /// `RS`: one rumor on a line of its own (`RumorShow`), at random or
    /// with `new` the newest; nothing if there are none.
    pub async fn rumor_show(&mut self, param: &str) -> io::Result<()> {
        let newest = param.trim().to_ascii_lowercase().starts_with("new");
        let Some(r) = self.state.sys.rumor(newest).map_err(io_err)? else { return Ok(()) };
        // New: over the board's picture, if it has one (RUMORART).
        if self.rumor_art_one(&r).await? {
            return Ok(());
        }
        let s = crate::strings::substitute(self.strings().get("RumorShow"), &[&defuse(&r.text)]);
        self.newline();
        self.show_line(&s).await
    }

    /// `RA`: add a rumor.
    pub async fn rumor_add(&mut self) -> io::Result<()> {
        if self.user.id == 0 {
            return Ok(());
        }
        self.newline();
        let since = crate::clock::now().naive_local() - chrono::Duration::days(1);
        if !self.is_sysop() && self.state.sys.rumors_since(self.user.id, since).map_err(io_err)? >= RUMORS_DAILY {
            let s = self.strings().fmt("RumorDaily", &[&RUMORS_DAILY.to_string()]);
            return self.show_line(&s).await;
        }
        let s = self.strings().fmt("RumorAsk", &[&RUMOR_LEN.to_string()]);
        self.show(&s).await?;
        let typed = self.read_string(ReadOpts::new(RUMOR_LEN)).await?;
        self.newline();
        let text = clean(&typed);
        if gist(&text).chars().filter(|c| c.is_alphanumeric()).count() < 3 {
            return Ok(());
        }
        let all = self.state.sys.rumors().map_err(io_err)?;
        if all.iter().any(|r| gist(&r.text) == gist(&text)) {
            return self.say_line("RumorDup").await;
        }
        self.state.sys.add_rumor(self.user.id, &self.user.handle, &text, RUMORS_KEPT).map_err(io_err)?;
        tracing::info!("{}: added a rumor: {text:?}", self.user.handle);
        self.say_line("RumorAdded").await
    }

    /// `RL`: the rumors, newest first (RUMORTOP/MID/BOT if the board has
    /// them, `~NM` `~RU`, and for SysOps `~AU` `~DT`), then a SysOp may
    /// delete one and anyone may add one.
    pub async fn rumor_list(&mut self) -> io::Result<()> {
        let sysop = self.is_sysop();
        let list = self.state.sys.rumors().map_err(io_err)?;
        let mut pictured = false;
        if list.is_empty() {
            self.newline();
            self.say_line("RumorNone").await?;
        } else {
            pictured = self.rumor_art(&list, sysop).await?;
            if !pictured {
                self.rumor_rows(&list, sysop).await?;
            }
        }
        if sysop && !list.is_empty() {
            let s = self.strings().fmt("RumorAskDelete", &[&list.len().to_string()]);
            self.show(&s).await?;
            let n = self.read_string(ReadOpts::new(4).allowed(DIGITS)).await?;
            self.newline();
            if let Some(r) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| list.get(i)) {
                self.state.sys.delete_rumor(r.id).map_err(io_err)?;
                tracing::info!("{}: deleted a rumor by {}: {:?}", self.user.handle, r.handle, r.text);
                self.say_line("RumorDeleted").await?;
            }
        }
        if self.user.id != 0 {
            self.say("RumorAskAdd").await?;
            if self.yes_no(false).await? {
                // Typing one takes the width: off the picture first.
                if pictured {
                    self.clear_screen();
                }
                self.rumor_add().await?;
            }
        }
        Ok(())
    }

    /// New: the rumors over a picture (RUMORART, if the board has one):
    /// wrapped into the space between its `~TL` and `~BR` markers, a page
    /// at a time, with the prompts after at `~PR` (else under the space).
    /// False, having shown nothing, without the screen or its markers.
    async fn rumor_art(&mut self, list: &[Rumor], sysop: bool) -> io::Result<bool> {
        let mut i = 0;
        loop {
            let Some((x0, y0, x1, y1, prompt)) = self.rumor_art_screen(list.len()).await? else {
                if i == 0 {
                    return Ok(false);
                }
                // (The screen went between pages: the rest as a list.)
                self.rumor_rows(&list[i..], sysop).await?;
                return Ok(true);
            };
            let width = usize::from(x1.saturating_sub(x0) + 1).max(12);
            let mut y = y0;
            while let Some(r) = list.get(i) {
                let mut lines: Vec<String> = wrap_shown(&r.text, width - 4)
                    .iter()
                    .enumerate()
                    .map(|(j, l)| if j == 0 { format!("|U3{} |U1{}", resize(&(i + 1).to_string(), 3), defuse(l)) } else { format!("|U1    {}", defuse(l)) })
                    .collect();
                if sysop {
                    lines.push(format!("|U5    {}, {}", lit(&r.handle), self.local(r.at).format("%m/%d/%y %H:%M")));
                }
                // A rumor too long for an empty page is cut, not skipped.
                let room = usize::from(y1 + 1 - y);
                if lines.len() > room && y > y0 {
                    break;
                }
                for l in lines.iter().take(room) {
                    self.screen.goto(x0, y);
                    self.show(l).await?;
                    y += 1;
                }
                i += 1;
            }
            self.screen.goto(prompt.0, prompt.1);
            if i >= list.len() {
                return Ok(true);
            }
            self.say("RumorMore").await?;
            if !self.yes_no(true).await? {
                self.screen.goto(prompt.0, prompt.1);
                return Ok(true);
            }
        }
    }

    /// RUMORART with one rumor in its space (`RS`, at logon), what
    /// follows at `~PR`. False, having shown nothing, as `rumor_art`.
    async fn rumor_art_one(&mut self, r: &Rumor) -> io::Result<bool> {
        let Some((x0, y0, x1, y1, prompt)) = self.rumor_art_screen(1).await? else { return Ok(false) };
        let width = usize::from(x1.saturating_sub(x0) + 1).max(12);
        // In the middle of the space, not lost at its top.
        let lines = wrap_shown(&r.text, width);
        let top = y0 + (y1 + 1 - y0).saturating_sub(lines.len() as u16) / 2;
        for (y, l) in (top..=y1).zip(lines) {
            self.screen.goto(x0, y);
            self.show(&format!("|U1{}", defuse(&l))).await?;
        }
        self.screen.goto(prompt.0, prompt.1);
        Ok(true)
    }

    /// RUMORART shown: its space (`~TL`, `~BR`) and where the prompts go
    /// (`~PR`, else under the space); None, with nothing left on the
    /// screen, if the board hasn't it or its markers.
    async fn rumor_art_screen(&mut self, count: usize) -> io::Result<Option<(u16, u16, u16, u16, (u16, u16))>> {
        if self.plain() || self.find_text_file("RUMORART").is_none() {
            return Ok(None);
        }
        if !self.show_file_with("RUMORART", &waves([("RC", count.to_string())])).await? {
            return Ok(None);
        }
        let (Some(&(x0, y0, _)), Some(&(x1, y1, _))) = (self.positions.get(b"TL"), self.positions.get(b"BR")) else {
            self.clear_screen();
            return Ok(None);
        };
        let prompt = self.positions.get(b"PR").map_or((x0, y1 + 1), |&(x, y, _)| (x, y));
        Ok(Some((x0, y0, x1, y1, prompt)))
    }

    fn rumor_waves(&self, r: &Rumor, n: usize, sysop: bool, au: bool) -> Waves {
        let when = if sysop { self.local(r.at).format("%m/%d/%y %H:%M").to_string() } else { String::new() };
        // A SysOp sees who started it ahead of the rumor itself, where a
        // screen has no ~AU of its own.
        let text = if sysop && !au { format!("|08{}|07: {}", lit(&r.handle), r.text) } else { r.text.clone() };
        waves([("NM", n.to_string()), ("RU", text), ("AU", if sysop { r.handle.clone() } else { String::new() }), ("DT", when)])
    }

    async fn rumor_rows(&mut self, list: &[Rumor], sysop: bool) -> io::Result<()> {
        let mut rep = self.repeat_begin_with("RUMOR", false, &waves([("RC", list.len().to_string())])).await?;
        if rep.is_none() {
            self.clear_screen();
            self.newline();
            self.set_slot(Slot::Info);
            let head = if sysop { " Num  Rumor (who, when)" } else { " Num  Rumor" };
            self.show_line(&format!("|U4{head}")).await?;
            self.rule().await?;
        }
        let mut lines = 3;
        for (i, r) in list.iter().enumerate() {
            let more = match rep.as_mut() {
                Some(rp) => {
                    let au = rp.mid.windows(3).any(|w| w == b"~AU");
                    let w = self.rumor_waves(r, i + 1, sysop, au);
                    self.repeat_row(rp, &w, false).await?
                }
                None => {
                    let line = format!("|U3 {} |U1{}", resize(&(i + 1).to_string(), 4), defuse(&r.text));
                    self.show_line(&line).await?;
                    lines += 1;
                    // A SysOp gets who and when on a line of its own, so a
                    // long rumor keeps to one.
                    if sysop {
                        let who = format!("|U5      {}, {}", lit(&r.handle), self.local(r.at).format("%m/%d/%y %H:%M"));
                        self.show_line(&who).await?;
                        lines += 1;
                    }
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await,
            None => self.rule().await,
        }
    }
}

/// Text with colour codes in lines of at most `width` columns on screen,
/// at spaces where it can (a longer word is cut).
fn wrap_shown(text: &str, width: usize) -> Vec<String> {
    use fluxbbs_term::codes::{squish, width as shown};
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut rest = word.to_string();
        while shown(&rest) > width {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            let head = squish(&rest, width);
            rest = rest[head.len()..].to_string();
            out.push(head);
        }
        if !line.is_empty() && shown(&line) + 1 + shown(&rest) > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&rest);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_with_colours() {
        assert_eq!(wrap_shown("the |12sysop|07 runs three boards", 12), ["the |12sysop|07", "runs three", "boards"]);
        assert_eq!(wrap_shown("http://example.com/x", 8), ["http://e", "xample.c", "om/x"]);
        assert!(wrap_shown("   ", 5).is_empty());
    }

    #[test]
    fn typed_lines_are_cleaned() {
        assert_eq!(clean("  the  sysop\tis a\x1b[2Jbot  "), "the sysop is a [2Jbot");
        assert_eq!(clean("|04red |CS stays"), "|04red |CS stays", "codes are made harmless where shown");
        assert_eq!(gist("|12The SysOp  IS a bot"), "the sysop is a bot");
    }

    #[test]
    fn rumor_codes() {
        let me = crate::user::User { id: 2, handle: "Ann".into(), ..crate::user::User::default() };
        let s = Session::for_test(crate::tests::state(), me);
        let code = |s: &Session, c: &str| s.mci_value(c.as_bytes().try_into().unwrap()).unwrap();
        assert_eq!((code(&s, "RM"), code(&s, "RL")), (String::new(), String::new()));
        s.state.sys.add_rumor(2, "Ann", "the old one", RUMORS_KEPT).unwrap();
        s.state.sys.add_rumor(3, "Bob", "the |12new|07 one", RUMORS_KEPT).unwrap();
        assert_eq!(code(&s, "RL"), "the |12new|07 one");
        // The pick holds a while, so a screen measuring it first or
        // showing it twice shows the same one.
        *s.rumor_pick.borrow_mut() = None;
        let first = code(&s, "RM");
        assert!(first.starts_with("the "));
        assert!((0..20).all(|_| code(&s, "RM") == first));
        let mut cfg = (*s.state.config()).clone();
        cfg.features.rumors = false;
        s.state.set_config(cfg);
        assert_eq!(code(&s, "RL"), "");
    }

    #[test]
    fn shown_rumors_cant_act() {
        // A rumor's codes other than colours show as typed.
        let shown = defuse(&clean("|CS%!script|04 hi"));
        assert!(shown.contains("|ZA|CS|ZB") && shown.contains("|ZA%!s|ZB") && shown.contains("|04 hi"));
    }
}
