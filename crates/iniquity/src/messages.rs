//! Message areas, reading, posting, newscans and email (`MsgArea.pas`,
//! `Email.pas`, `Conf.pas`).

use std::collections::HashSet;
use std::io;

use chrono::Local;
use iniquity_term::Slot;
use iniquity_term::codes::{resize, squish};

use crate::editor::EditCtx;
use crate::input::{DIGITS, Key, ReadOpts};
use crate::msgareas::MsgArea;
use crate::msgbase::{Message, Party};
use crate::session::{BarPick, BarRow, BarSpec, Session, waves};
use crate::user::parse_date;

/// An area being worked on: its number (0 = email) and definition.
#[derive(Clone)]
pub struct AreaRef {
    pub num: u32,
    pub area: MsgArea,
}

impl AreaRef {
    pub(crate) fn is_email(&self) -> bool {
        self.num == 0
    }

    pub(crate) fn key(&self) -> &str {
        &self.area.file
    }
}

pub(crate) fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

impl Session {
    // ---- areas and access ---------------------------------------------

    pub fn email_area(&self) -> AreaRef {
        AreaRef { num: 0, area: self.state.areas().email.clone() }
    }

    pub fn msg_area(&self, n: u32) -> Option<AreaRef> {
        self.state.areas().get(n).map(|a| AreaRef { num: n, area: a.clone() })
    }

    /// The current area, if it's open to the caller (`area_open`).
    pub fn current_area(&self) -> Option<AreaRef> {
        self.msg_area(self.user.cur_msg_area).filter(|a| self.area_open(a))
    }

    /// The caller may read the area now: its ACS passes and, if it has a
    /// password, that was given this call.
    pub(crate) fn area_open(&self, a: &AreaRef) -> bool {
        self.acs(&a.area.acs) && (a.area.password.is_empty() || self.opened_areas.contains(&a.key().to_ascii_uppercase()))
    }

    /// The original matched names; a party with a user number is matched
    /// by it instead, so a caller whose handle is "None" (the alias of
    /// every party that isn't a user) or matches a name from an echo isn't
    /// taken for them. Parties without a number (echomail) go by alias.
    pub(crate) fn is_me(&self, p: &Party) -> bool {
        if p.user_id != 0 {
            return p.user_id == self.user.id;
        }
        !p.alias.eq_ignore_ascii_case("None") && p.alias.eq_ignore_ascii_case(&self.user.handle)
    }

    /// Wrote it here: only a user number gives the right to edit or delete.
    fn wrote(&self, p: &Party) -> bool {
        p.user_id != 0 && p.user_id == self.user.id
    }

    /// By handle, or by real name if no one before them has it (real
    /// names aren't unique: a new caller could give the sponsor's).
    pub(crate) fn is_sponsor(&self, a: &AreaRef) -> bool {
        let s = a.area.sponsor.trim();
        !s.is_empty()
            && self.acs(&a.area.acs)
            && (s.eq_ignore_ascii_case(&self.user.handle)
                || (s.eq_ignore_ascii_case(&self.user.real_name) && self.state.users.find(s, false).ok().flatten().is_some_and(|u| u.id == self.user.id)))
    }

    pub(crate) fn is_cosysop(&self) -> bool {
        self.acs(&self.state.config().acs.cosysop.clone())
    }

    /// `maOKtoRead`.
    pub(crate) fn ok_to_read(&self, a: &AreaRef, m: &Message) -> bool {
        (!m.private && !m.deleted) || self.is_me(&m.to) || self.is_me(&m.from) || self.is_cosysop() || self.is_sponsor(a)
    }

    pub(crate) fn ok_to_delete(&self, a: &AreaRef, m: &Message) -> bool {
        self.is_cosysop() || self.is_sponsor(a) || (m.private && self.is_me(&m.to)) || self.wrote(&m.from)
    }

    pub(crate) fn ok_to_edit(&self, a: &AreaRef, m: &Message) -> bool {
        self.is_cosysop() || self.is_sponsor(a) || self.wrote(&m.from)
    }

    /// `maChangeArea`: switch to an area if the caller may, asking for its
    /// password.
    pub async fn change_area(&mut self, n: u32) -> io::Result<bool> {
        let Some(a) = self.msg_area(n) else { return Ok(false) };
        if !self.acs(&a.area.acs) {
            return Ok(false);
        }
        if !a.area.password.is_empty() && !self.opened_areas.contains(&a.key().to_ascii_uppercase()) {
            self.say("MsgAskPassword").await?;
            let pw = self.read_string(ReadOpts::new(20).password()).await?;
            self.newline();
            if !pw.eq_ignore_ascii_case(&a.area.password) {
                return Ok(false);
            }
            self.opened_areas.insert(a.key().to_ascii_uppercase());
        }
        self.user.cur_msg_area = n;
        tracing::info!("{}: changed to message area {}", self.user.handle, a.area.name);
        Ok(true)
    }

    /// `maFindAreaWithAccess`: make sure the current area is one the caller
    /// can use. A passworded area saved from the last call isn't open
    /// until its password is given again.
    pub fn find_area_with_access(&mut self) {
        if self.current_area().is_some() {
            return;
        }
        if let Some(n) = (1..=self.state.areas().areas.len() as u32).find(|&n| self.msg_area(n).is_some_and(|a| self.area_open(&a))) {
            self.user.cur_msg_area = n;
        }
    }

    /// `maPrevArea` / `maNextArea`.
    pub async fn step_area(&mut self, forward: bool) -> io::Result<()> {
        let total = self.state.areas().areas.len() as u32;
        let mut n = self.user.cur_msg_area;
        loop {
            n = if forward { n + 1 } else { n.saturating_sub(1) };
            if n == 0 || n > total {
                return self.say_line(if forward { "MsgHighestArea" } else { "MsgLowestArea" }).await;
            }
            if self.change_area(n).await? {
                let name = self.msg_area(n).map(|a| a.area.name).unwrap_or_default();
                let s = self.strings().fmt("MsgChangedAreas", &[&name]);
                return self.show_line(&s).await;
            }
        }
    }

    /// (number shown, area number) for each area the caller sees listed:
    /// with `compress_area_numbers`, listed areas are numbered 1, 2, 3...
    pub(crate) fn msg_area_labels(&self) -> Vec<(u32, u32)> {
        let compress = self.state.config().messages.compress_area_numbers;
        let mut out = Vec::new();
        for (i, a) in self.state.areas().areas.iter().enumerate() {
            let n = i as u32 + 1;
            if self.acs(&a.acs) || a.unhidden {
                out.push((if compress { out.len() as u32 + 1 } else { n }, n));
            }
        }
        out
    }

    /// `M#` / `F#`: change to the area whose listed number was typed at
    /// the menu (the `#` key).
    pub async fn jump_area(&mut self, msg: bool, typed: &str) -> io::Result<()> {
        let Ok(label) = typed.trim().parse::<u32>() else { return Ok(()) };
        let labels = if msg { self.msg_area_labels() } else { self.file_area_labels() };
        let target = labels.iter().find(|(l, _)| *l == label).map(|&(_, n)| n);
        let changed = match target {
            Some(n) if msg => self.change_area(n).await?,
            Some(n) => self.change_farea(n).await?,
            None => false,
        };
        if !changed {
            return self.say_line("AreaNotFound").await;
        }
        // An area with sub-areas opens, to pick one of them.
        if let Some(n) = target.filter(|&n| !msg && !self.farea_children(Some(n)).is_empty()) {
            return self.list_fareas(true, Some(n)).await;
        }
        let (name, key) = if msg {
            (self.current_area().map(|a| a.area.name).unwrap_or_default(), "MsgChangedAreas")
        } else {
            (self.current_farea().map(|a| a.area.name).unwrap_or_default(), "FaChangedAreas")
        };
        let s = self.strings().fmt(key, &[&name]);
        self.show_line(&s).await
    }

    /// `maListAreas` (`MA`, and `MC` without a number). `MC` picks with a
    /// bar (`area_bar`).
    pub async fn list_areas(&mut self, change: bool) -> io::Result<()> {
        let areas = self.state.areas();
        let rows: Vec<(u32, u32, MsgArea)> =
            self.msg_area_labels().into_iter().map(|(label, n)| (label, n, areas.areas[n as usize - 1].clone())).collect();
        if change {
            let mut bar = Vec::new();
            for (label, n, a) in &rows {
                let count = self.state.msgs.count(&a.file).map_err(io_err)?;
                let waves = waves([("AT", a.name.clone()), ("AN", label.to_string()), ("NM", count.to_string()), ("SP", a.sponsor.clone())]);
                let plain = format!(" {} {} {} {}", resize(&label.to_string(), 6), resize(&a.name, 30), resize(&count.to_string(), 7), squish(&a.sponsor, 36));
                bar.push(BarRow { n: *n, waves, plain });
            }
            let start = bar.iter().position(|r| r.n == self.user.cur_msg_area).unwrap_or(0);
            let labels: Vec<(u32, u32)> = rows.iter().map(|(label, n, _)| (*label, *n)).collect();
            let help = self.strings().get("MsgBarKeys").to_string();
            let header = " Num    Area Title                     Msgs    Sponsor";
            let spec = BarSpec { prefix: "MAREA", header, rows: &bar, labels: &labels, start, help: &help, ask: "MsgAskChangeArea", can_go_up: false, keys: "" };
            if let BarPick::Area(n) = self.area_bar(&spec).await?
                && self.change_area(n).await?
            {
                let name = self.msg_area(n).map(|a| a.area.name).unwrap_or_default();
                let s = self.strings().fmt("MsgChangedAreas", &[&name]);
                self.show_line(&s).await?;
            }
            return Ok(());
        }
        let mut rep = self.repeat_begin("MAREA", true).await?;
        if rep.is_none() {
            self.clear_screen();
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line(" Num    Area Title                     Msgs    Sponsor").await?;
            self.rule().await?;
        }
        let mut lines = 4;
        for (label, n, a) in rows {
            let count = self.state.msgs.count(&a.file).map_err(io_err)?;
            let here = n == self.user.cur_msg_area;
            let more = match rep.as_mut() {
                Some(r) => {
                    let w = waves([("AT", a.name.clone()), ("AN", label.to_string()), ("NM", count.to_string()), ("SP", a.sponsor.clone())]);
                    self.repeat_row(r, &w, here).await?
                }
                None => {
                    self.set_slot(if here { Slot::TextHi } else { Slot::Text });
                    let line = format!(" {} {} {} {}", resize(&label.to_string(), 6), resize(&a.name, 30), resize(&count.to_string(), 7), squish(&a.sponsor, 36));
                    self.write(&line);
                    self.newline();
                    lines += 1;
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => self.rule().await?,
        }
        Ok(())
    }

    pub(crate) async fn rule(&mut self) -> io::Result<()> {
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(79));
        self.newline();
        Ok(())
    }

    // ---- conferences ----------------------------------------------------

    pub(crate) fn confs(&self, msg: bool) -> Vec<crate::msgareas::Conf> {
        if msg { self.state.areas().confs.clone() } else { self.state.fareas().confs.clone() }
    }

    /// `confSelect` (`MJ`, `FJ`): by key, or ask (`?` lists).
    pub async fn select_conf(&mut self, msg: bool, param: &str) -> io::Result<()> {
        let confs = self.confs(msg);
        let key_of = |s: &Self, c: char| confs.iter().position(|f| f.key.eq_ignore_ascii_case(&c) && s.acs(&f.acs)).map(|i| i as u32 + 1);
        let set = |s: &mut Self, n: u32| {
            if msg {
                s.user.conf_msg = n;
                s.find_area_with_access();
            } else {
                s.user.conf_file = n;
                s.find_farea_with_access();
            }
        };
        let p = param.trim();
        if !p.is_empty() && !p.eq_ignore_ascii_case("LIST") {
            if let Some(n) = key_of(self, p.chars().next().unwrap()) {
                set(self, n);
            }
            return Ok(());
        }
        if p.eq_ignore_ascii_case("LIST") {
            self.list_confs(msg).await?;
        }
        let c = loop {
            self.say(if msg { "ConfMsgSelect" } else { "ConfFileSelect" }).await?;
            match self.read_key().await? {
                Key::Enter => break None,
                Key::Char(b'?') => {
                    self.write("?");
                    self.newline();
                    self.list_confs(msg).await?;
                }
                k => break k.upper(),
            }
        };
        let cur_n = if msg { self.user.conf_msg } else { self.user.conf_file };
        let cur = confs.get(cur_n.saturating_sub(1) as usize).map(|c| c.key.to_ascii_uppercase());
        match c {
            None => self.newline(),
            Some(c) if Some(c) == cur => {
                self.write(&c.to_string());
                self.newline();
            }
            Some(c) => match key_of(self, c) {
                Some(n) => {
                    set(self, n);
                    self.write(&c.to_string());
                    self.newline();
                    let desc = confs[n as usize - 1].desc.clone();
                    let s = self.strings().fmt(if msg { "ConfMsgChanged" } else { "ConfFileChanged" }, &[&desc]);
                    self.show_line(&s).await?;
                }
                None => {
                    self.write("Abort");
                    self.newline();
                }
            },
        }
        Ok(())
    }

    /// `confList` (`MZ`, `FZ`).
    pub async fn list_confs(&mut self, msg: bool) -> io::Result<()> {
        let confs: Vec<_> = self.confs(msg).into_iter().filter(|c| self.acs(&c.acs)).collect();
        let mut rep = self.repeat_begin(if msg { "MCONF" } else { "FCONF" }, false).await?;
        if rep.is_none() {
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line(" Key  Conference").await?;
            self.rule().await?;
        }
        let mut lines = 3;
        for c in &confs {
            let more = match rep.as_mut() {
                Some(r) => self.repeat_row(r, &waves([("CK", c.key.to_string()), ("CN", c.desc.clone())]), false).await?,
                None => {
                    self.set_slot(Slot::Text);
                    self.write(&format!(" {} {}", resize(&c.key.to_string(), 4), c.desc));
                    self.newline();
                    lines += 1;
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

    // ---- showing messages ---------------------------------------------

    pub(crate) fn status_str(m: &Message) -> String {
        let mut s = Vec::new();
        if m.deleted {
            s.push("[Deleted]");
        }
        if m.private {
            s.push("[Private]");
        }
        if m.anonymous {
            s.push("[Anonymous]");
        }
        if s.is_empty() { "[Normal]".into() } else { s.join(" ") }
    }

    /// `maShowMessage`: header screen, then the text.
    /// The header above a message (MSGHEAD, or MAILHEAD for email), from a
    /// clear screen. Returns the lines it took.
    pub(crate) async fn msg_header(&mut self, a: &AreaRef, m: &Message, pos: u32, total: u32) -> io::Result<u16> {
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        let date = m.posted.format("%m/%d/%y").to_string();
        let time = m.posted.format("%-I:%M%P").to_string();
        let note = if m.anonymous { "Unknown".to_string() } else { m.from.note.clone() };
        let reply_to = self.reply_to_text(m)?;
        let w = waves([
            ("TI", time.clone()),
            ("DT", date.clone()),
            ("SN", crate::editor::printable(&m.from.name).into_owned()),
            ("RN", crate::editor::printable(&m.to.name).into_owned()),
            ("MT", crate::editor::printable(&m.subject).into_owned()),
            ("MN", format!("{pos}/{total}")),
            ("MA", a.area.name.clone()),
            ("RP", m.replies.to_string()),
            ("ST", Self::status_str(m)),
            ("UN", note),
            ("RT", reply_to),
        ]);
        let screen = if a.is_email() { "MAILHEAD" } else { "MSGHEAD" };
        let lines = if self.show_file_with(screen, &w).await? {
            self.goto_pos(*b"TL");
            self.screen.where_y()
        } else {
            let replies = if m.replies == 0 { "None".to_string() } else { m.replies.to_string() };
            let rows = [
                format!("|U4Time  |U5: |U6{}   |U4Date |U5: |U6{date}        |U4Number   |U5: |U6{pos}|U4 of |U6{total}", resize(&time, 7)),
                format!("|U4From  |U5: |U6{} |U4Msg Area |U5: |U6{}", resize(&crate::editor::printable(&m.from.name), 32), squish(&a.area.name, 28)),
                format!("|U4To    |U5: |U6{} |U4Replies  |U5: |U6{replies}", resize(&crate::editor::printable(&m.to.name), 32)),
                format!("|U4Title |U5: |U6{} |U4Status   |U5: |U6{}", resize(&crate::editor::printable(&m.subject), 32), squish(&Self::status_str(m), 28)),
            ];
            for r in rows {
                self.write_with_controls(&r);
                self.newline();
            }
            self.newline();
            5
        };
        Ok(lines)
    }

    async fn show_message(&mut self, a: &AreaRef, m: &Message, pos: u32, total: u32) -> io::Result<()> {
        let mut lines = self.msg_header(a, m, pos, total).await?;
        if m.anonymous && (self.is_sponsor(a) || self.is_cosysop()) {
            let s = format!("|U5Anonymous message by: |U4{} |U5[{}]", m.from.alias, m.from.real_name);
            self.write_with_controls(&s);
            self.newline();
            lines += 1;
        }
        self.set_slot(Slot::TextHi);
        for line in &m.lines {
            if line.contains('\x1b') {
                self.screen.write_bytes(&crate::editor::ansi_bytes(line));
                self.newline();
            } else {
                let line = crate::editor::printable(line);
                let quote = line.chars().take(3).any(|c| c == '>');
                let s = if quote {
                    format!("|U1{line}|U3")
                } else if line.starts_with("---") && (line.len() == 3 || line.as_bytes()[3] == b' ') {
                    format!("|U2{line}")
                } else {
                    line.into_owned()
                };
                self.write_with_controls(&s);
                self.newline();
            }
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        self.offer_attachment(m).await
    }

    /// `~RT` (new): "#12 by phrac" for a reply, empty otherwise.
    pub(crate) fn reply_to_text(&self, m: &Message) -> io::Result<String> {
        if m.reply_to == 0 {
            return Ok(String::new());
        }
        Ok(match self.state.msgs.by_id(m.reply_to).map_err(io_err)? {
            Some(p) => self.strings().fmt("MsgReplyTo", &[&p.num.to_string(), &p.from.name]),
            None => String::new(),
        })
    }

    // ---- reading --------------------------------------------------------

    /// First message after the caller's pointer that they may read and
    /// haven't read in the message centre (`maNewScanMsgNum`), or 0.
    fn first_new(&self, a: &AreaRef) -> io::Result<u32> {
        let last = self.state.msgs.lastread(self.user.id, a.key()).map_err(io_err)?;
        let msgs = self.state.msgs.range(a.key(), last + 1, u32::MAX).map_err(io_err)?;
        let seen = self.state.msgs.seen(self.user.id, a.key()).map_err(io_err)?;
        Ok(msgs.iter().find(|m| !seen.contains(&m.id) && self.ok_to_read(a, m)).map_or(0, |m| m.num))
    }

    /// The nearest readable message at or after/before `n`, not one in
    /// `skip`.
    fn readable(&self, a: &AreaRef, n: u32, forward: bool, total: u32, skip: &HashSet<i64>) -> io::Result<Option<Message>> {
        let mut n = n;
        while (1..=total).contains(&n) {
            if let Some(m) = self.state.msgs.get(a.key(), n).map_err(io_err)?
                && !skip.contains(&m.id)
                && self.ok_to_read(a, &m) {
                    return Ok(Some(m));
                }
            n = if forward { n + 1 } else { n - 1 };
        }
        Ok(None)
    }

    /// `maReadMessages`. `start` 0 asks where to begin. Returns false if
    /// the caller quit (which ends a global newscan). A scan (`!show_empty`)
    /// passes over messages already read in the message centre.
    pub async fn read_messages(&mut self, a: &AreaRef, start: u32, mandatory: bool, show_empty: bool) -> io::Result<bool> {
        let mut total = self.state.msgs.count(a.key()).map_err(io_err)?;
        if total == 0 {
            if show_empty {
                self.newline();
                self.say_line("MsgReadNoMsgs").await?;
                self.newline();
            }
            return Ok(true);
        }
        let mut cur = start;
        if cur == 0 {
            let hi = match self.first_new(a)? {
                0 => total,
                n => n,
            };
            let s = self.strings().fmt("MsgReadStartAt", &[&total.to_string(), &hi.to_string()]);
            self.show(&s).await?;
            let s = self.read_string(ReadOpts::new(total.to_string().len()).allowed(DIGITS).prefill(hi.to_string())).await?;
            self.newline();
            cur = s.parse().unwrap_or(1);
        }
        cur = cur.clamp(1, total);
        let none = HashSet::new();
        let seen = if show_empty { HashSet::new() } else { self.state.msgs.seen(self.user.id, a.key()).map_err(io_err)? };
        let Some(mut m) = self.readable(a, cur, true, total, &seen)? else { return Ok(true) };
        cur = m.num;
        let may_quit = self.state.config().messages.abort_mandatory_ok || !mandatory;
        let mut show = true;
        loop {
            if self.hangup {
                return Ok(true);
            }
            self.menu_msg = cur;
            if show {
                self.show_message(a, &m, cur, total).await?;
                // By row: a pack meanwhile may have renumbered it.
                self.state.msgs.set_lastread_id(self.user.id, a.key(), m.id).map_err(io_err)?;
            }
            show = false;
            let s = self.strings().fmt("MsgReadPrompt", &[&cur.to_string(), &total.to_string()]);
            self.show(&s).await?;
            let (key, typed) = self.read_command_or_number(total, "QPA?RDEHLIS-").await?;
            // Where this message is now, should another node have packed.
            if let Some(n) = self.state.msgs.num_of(m.id).map_err(io_err)? {
                cur = n;
            }
            total = self.state.msgs.count(a.key()).map_err(io_err)?;
            if let Some(n) = typed {
                self.newline();
                if let Some(found) = self.readable(a, n.clamp(1, total), false, total, &none)? {
                    m = found;
                    cur = m.num;
                }
                show = true;
                continue;
            }
            match key {
                'Q' => {
                    if may_quit {
                        self.full_screen_done();
                        return Ok(false);
                    }
                    self.say_line("MsgReadMandatory").await?;
                }
                '\r' => match self.readable(a, cur + 1, true, total, &seen)? {
                    Some(next) => {
                        m = next;
                        cur = m.num;
                        show = true;
                    }
                    None => {
                        self.offer_post(a).await?;
                        return Ok(true);
                    }
                },
                'I' | 'S' => {
                    if !may_quit {
                        self.say_line("MsgReadMandatory").await?;
                        continue;
                    }
                    if key == 'I' {
                        self.state.msgs.set_lastread(self.user.id, a.key(), total).map_err(io_err)?;
                    }
                    return Ok(true);
                }
                '-' => {
                    if let Some(prev) = self.readable(a, cur.saturating_sub(1), false, total, &none)? {
                        m = prev;
                        cur = m.num;
                    }
                    show = true;
                }
                'P' => {
                    self.post(a, "", "", None).await?;
                    total = self.state.msgs.count(a.key()).map_err(io_err)?;
                    show = true;
                }
                '?' => {
                    if self.show_file("READHELP").await? {
                        self.prompt_key().await?;
                        show = true;
                    }
                }
                'A' => show = true,
                'R' => {
                    if self.post(a, "", "", Some(&m)).await?.is_some() {
                        m.replies += 1;
                        total = self.state.msgs.count(a.key()).map_err(io_err)?;
                        self.newline();
                    }
                }
                'D' if self.ok_to_delete(a, &m) => {
                    m.deleted = !m.deleted;
                    self.state.msgs.set_deleted(m.id, m.deleted).map_err(io_err)?;
                    self.say_line(if m.deleted { "MsgDeleted" } else { "MsgUndeleted" }).await?;
                    tracing::info!("{}: message {} in {} {}", self.user.handle, cur, a.area.name, if m.deleted { "deleted" } else { "undeleted" });
                    show = true;
                }
                'E' if self.ok_to_edit(a, &m) => {
                    self.edit_existing(a, &mut m).await?;
                    show = true;
                }
                'H' => {
                    self.say_line("MsgSetPointer").await?;
                    self.state.msgs.set_lastread(self.user.id, a.key(), cur).map_err(io_err)?;
                    show = true;
                }
                'L' => {
                    cur = self.list_messages(a, cur, total).await?;
                    if let Some(found) = self.readable(a, cur, false, total, &none)? {
                        m = found;
                        cur = m.num;
                    }
                }
                _ => {}
            }
        }
    }

    /// Read a reader command, or a message number (digits, then Enter or
    /// enough digits). Returns the command key (Enter = '\r', Esc = 'Q').
    async fn read_command_or_number(&mut self, total: u32, keys: &str) -> io::Result<(char, Option<u32>)> {
        let width = total.to_string().len();
        let mut digits = String::new();
        loop {
            let key = self.read_key().await?;
            match key {
                Key::Char(c @ b'0'..=b'9') => {
                    digits.push(c as char);
                    self.write(&(c as char).to_string());
                    if digits.len() >= width {
                        return Ok(('\r', digits.parse().ok()));
                    }
                }
                Key::Backspace if !digits.is_empty() => {
                    digits.pop();
                    self.screen.backspace(1);
                }
                Key::Enter if !digits.is_empty() => return Ok(('\r', digits.parse().ok())),
                Key::Enter => {
                    self.newline();
                    return Ok(('\r', None));
                }
                Key::Esc => {
                    self.newline();
                    return Ok(('Q', None));
                }
                k => {
                    if let Some(c) = k.upper().filter(|c| keys.contains(*c))
                        && digits.is_empty() {
                            self.write(&c.to_string());
                            self.newline();
                            return Ok((c, None));
                        }
                }
            }
        }
    }

    /// After the last message: "post in this area?"
    async fn offer_post(&mut self, a: &AreaRef) -> io::Result<()> {
        if !self.state.config().messages.ask_post_after_read || !self.acs(&a.area.post_acs) {
            return Ok(());
        }
        self.newline();
        self.say("MsgReadAskPost").await?;
        if self.yes_no(false).await? {
            self.post(a, "", "", None).await?;
        }
        Ok(())
    }

    /// `rmListMessages` (`L` in the reader): a page of headers from `cur`.
    /// Returns the last message listed.
    async fn list_messages(&mut self, a: &AreaRef, cur: u32, total: u32) -> io::Result<u32> {
        let mut rep = self.repeat_begin("MSGL", true).await?;
        // New: a page is what fits under MSGLTOP, however tall it is.
        let span = match &rep {
            Some(r) => (self.screen.rows() + 1).saturating_sub(self.screen.where_y() + r.bot_lines + 2).saturating_sub(1),
            None => self.user.page_length.saturating_sub(5),
        };
        let last = (cur + span.max(1) as u32).min(total);
        let msgs = self.state.msgs.range(a.key(), cur, last).map_err(io_err)?;
        if rep.is_none() {
            self.clear_screen();
            self.set_slot(Slot::Info);
            self.show_line("|U4 Num   Sender                 Receiver               Subject").await?;
            self.rule().await?;
        }
        let msgs: Vec<_> = msgs.into_iter().filter(|m| self.ok_to_read(a, m)).collect();
        for m in &msgs {
            let mine = self.is_me(&m.to) || self.is_me(&m.from);
            match rep.as_mut() {
                Some(r) => {
                    let w = waves([
                        ("MN", m.num.to_string()),
                        ("SN", m.from.name.clone()),
                        ("RN", m.to.name.clone()),
                        ("TI", m.posted.format("%-I:%M%P").to_string()),
                        ("DT", m.posted.format("%m/%d/%y").to_string()),
                        ("MT", m.subject.clone()),
                    ]);
                    self.repeat_row(r, &w, mine).await?;
                }
                None => {
                    self.set_slot(if mine { Slot::TextHi } else { Slot::Text });
                    let line = format!(" {} {} {} {}", resize(&m.num.to_string(), 5), resize(&m.from.name, 22), resize(&m.to.name, 22), squish(&m.subject, 25));
                    self.write(&line);
                    self.newline();
                }
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => self.rule().await?,
        }
        Ok(last)
    }

    // ---- posting --------------------------------------------------------

    /// `maPostMessage`: write a new message or a reply. Returns its number.
    pub async fn post(&mut self, a: &AreaRef, to: &str, subject: &str, reply: Option<&Message>) -> io::Result<Option<u32>> {
        let mcfg = &self.state.config().messages;
        let (ask_private, ask_private_reply, ask_quote, acs_anon, acs_sig) =
            (mcfg.ask_private, mcfg.ask_private_reply, mcfg.ask_auto_quote, mcfg.acs_anonymous.clone(), mcfg.acs_autosig.clone());
        if !a.is_email() && (!self.acs(&a.area.acs) || !self.acs(&a.area.post_acs)) {
            self.say_line("MsgNoPostAcs").await?;
            self.newline();
            return Ok(None);
        }
        let mut private = a.is_email();
        let (to_party, default_subject) = match reply {
            // Mail from elsewhere (FTN netmail) has nobody here to answer;
            // writing netmail isn't done yet.
            Some(r) if a.is_email() && r.from.user_id == 0 => {
                self.say_line("EmailUserUnknown").await?;
                return Ok(None);
            }
            Some(r) => {
                let subj = if r.subject.to_ascii_uppercase().starts_with("RE:") { r.subject.clone() } else { format!("Re: {}", r.subject) };
                // A reply to a private message is private unless the
                // writer says otherwise (`maPostMessage` asked).
                private = a.is_email() || r.private;
                if a.area.private && !a.is_email() && ask_private_reply {
                    self.say("MsgAskPrivate").await?;
                    private = self.yes_no(r.private).await?;
                }
                // Replies to anonymous messages still reach the author (by
                // number), but the name shown stays "Anonymous".
                (r.from.clone(), subj)
            }
            None => {
                self.say("MsgAskWhoTo").await?;
                let name = if to.is_empty() {
                    let default = if a.is_email() { "" } else { "All" };
                    let s = self.read_string(ReadOpts::new(36).prefill(default).complete_users()).await?;
                    self.newline();
                    s
                } else {
                    let found = self.state.users.find(to, false).map_err(io_err)?;
                    let shown = found.map_or(to.to_string(), |u| u.handle);
                    self.write(&shown);
                    self.newline();
                    to.to_string()
                };
                if name.is_empty() {
                    return Ok(None);
                }
                let party = match self.state.users.find(&name, false).map_err(io_err)? {
                    Some(u) => {
                        if a.area.private && !a.is_email() && ask_private {
                            self.say("MsgAskPrivate").await?;
                            private = self.yes_no(false).await?;
                        }
                        Party { user_id: u.id, alias: u.handle.clone(), real_name: u.real_name.clone(), name: u.handle.clone(), note: u.note.clone() }
                    }
                    None if a.is_email() => {
                        self.say_line("EmailUserUnknown").await?;
                        return Ok(None);
                    }
                    None => Party { user_id: 0, alias: "None".into(), real_name: "None".into(), name, note: "None".into() },
                };
                (party, String::new())
            }
        };
        self.say("MsgAskTitle").await?;
        let subject = if subject.is_empty() {
            let s = self.read_string(ReadOpts::new(40).prefill(default_subject)).await?;
            self.newline();
            s
        } else {
            self.write(subject);
            self.newline();
            subject.to_string()
        };
        if subject.is_empty() {
            return Ok(None);
        }
        let u = &self.user;
        let mut from = Party {
            user_id: u.id,
            alias: u.handle.clone(),
            real_name: u.real_name.clone(),
            name: if a.area.real_names { u.real_name.clone() } else { u.handle.clone() },
            note: u.note.clone(),
        };
        let can_anonymous = a.area.anonymous && !private && self.acs(&acs_anon);
        let mut anonymous = false;
        if can_anonymous {
            self.say("MsgAskAnonymous").await?;
            anonymous = self.yes_no(false).await?;
        }
        let mut auto_quote = reply.is_some() && self.user.quote;
        if reply.is_some() && ask_quote {
            self.say("MsgAskQuote").await?;
            auto_quote = self.yes_no(self.user.quote).await?;
        }
        let can_autosig = !self.user.autosig.is_empty() && self.acs(&acs_sig);
        // A prepared message, uploaded, goes into the editor to look over.
        let mut text = Vec::new();
        let (acs_upload, ask_reply, ask_email) = {
            let m = &self.state.config().messages;
            (m.acs_upload_message.clone(), m.ask_upload_reply, m.ask_upload_email)
        };
        let offer_upload = self.acs(&acs_upload) && if reply.is_some() { ask_reply } else if a.is_email() { ask_email } else { true };
        if offer_upload {
            self.say("MsgAskUploadMsg").await?;
            if self.yes_no(false).await?
                && let Some(lines) = self.receive_text_file().await? {
                    text = lines;
                }
        }
        let now = Local::now().naive_local();
        let ctx = EditCtx {
            subject,
            from: if anonymous { self.strings().get("Anonymous").to_string() } else { from.name.clone() },
            to: to_party.name.clone(),
            date: now,
            anonymous,
            can_anonymous,
            can_autosig,
            autosig: can_autosig && !anonymous,
            reply,
            auto_quote,
            text,
        };
        self.node.update(|n| n.activity = format!("Writing a message in {}", a.area.name));
        let Some(done) = self.edit_message(ctx).await? else {
            self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
            return Ok(None);
        };
        if done.anonymous {
            from.name = self.strings().get("Anonymous").to_string();
        }
        let mut lines = done.lines;
        let mut sig_pos = 0;
        if done.autosig && !self.user.autosig.is_empty() {
            lines.push(String::new());
            sig_pos = lines.len() as u32 + 1;
            lines.extend(self.user.autosig.iter().cloned());
        }
        let mut m = Message {
            id: 0,
            area: a.key().to_string(),
            num: 0,
            from,
            to: to_party,
            subject: done.subject,
            posted: now,
            deleted: false,
            private,
            anonymous: done.anonymous,
            replies: 0,
            sig_pos,
            tag: if a.is_email() { self.email_tag } else { 0 },
            reply_to: reply.map_or(0, |r| r.id),
            lines,
        };
        self.state.msgs.add(&mut m).map_err(io_err)?;
        if !private {
            self.state.mesh.post(&m);
            crate::mentions::record(&self.state, &m, &a.area.name, self.node.number());
        }
        // Your own message isn't new to you.
        if !a.is_email() {
            self.mark_read(a, &m)?;
        }
        if self.may_attach(a) {
            self.say("AtchAskAttach").await?;
            if self.yes_no(false).await? {
                self.attach_file(&m).await?;
            }
        }
        if private {
            self.user.email += 1;
            self.bump(crate::sysdata::Stat::Email);
        } else {
            self.user.posts += 1;
            self.bump(crate::sysdata::Stat::Post);
        }
        if let Err(e) = self.state.users.save(&self.user) {
            tracing::error!("saving {}: {e}", self.user.handle);
        }
        if a.is_email() {
            tracing::info!("{}: email to {}: {:?}", self.user.handle, m.to.name, m.subject);
            let s = self.strings().fmt("EmailSent", &[&m.to.name]);
            self.show_line(&s).await?;
        } else {
            tracing::info!("{}: posted #{} in {}: {:?}", self.user.handle, m.num, a.area.name, m.subject);
            let s = self.strings().fmt("MsgPublicPosted", &[&a.area.name]);
            self.show_line(&s).await?;
        }
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        Ok(Some(m.num))
    }

    /// `maEditMessage` (`E` in the reader): re-edit a message's text.
    pub(crate) async fn edit_existing(&mut self, a: &AreaRef, m: &mut Message) -> io::Result<()> {
        // The editor would cut what it can't hold (echomail, ANSI).
        let body = m.body();
        if body.len() > crate::editor::MAX_LINES || body.iter().any(|l| l.contains('\x1b') || l.chars().count() > crate::editor::WIDTH) {
            return self.say_line("MsgCantEdit").await;
        }
        let ctx = EditCtx {
            subject: m.subject.clone(),
            from: m.from.name.clone(),
            to: m.to.name.clone(),
            date: m.posted,
            anonymous: m.anonymous,
            can_anonymous: false,
            can_autosig: false,
            autosig: false,
            reply: None,
            auto_quote: false,
            text: m.body().to_vec(),
        };
        if let Some(done) = self.edit_message(ctx).await? {
            let sig: Vec<String> = if m.sig_pos > 0 { m.lines[(m.sig_pos as usize - 1).min(m.lines.len())..].to_vec() } else { Vec::new() };
            m.lines = done.lines;
            m.subject = done.subject;
            if !sig.is_empty() {
                m.lines.push(String::new());
                m.sig_pos = m.lines.len() as u32 + 1;
                m.lines.extend(sig);
            }
            self.state.msgs.update_text(m).map_err(io_err)?;
            tracing::info!("{}: edited #{} in {}", self.user.handle, m.num, a.area.name);
        }
        Ok(())
    }

    // ---- scans ----------------------------------------------------------

    /// `maNewScanAsk` (`MN`): `All`, an area number, or ask.
    pub async fn newscan_cmd(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        let p = param.trim();
        let all = p.to_ascii_uppercase().starts_with('A');
        let num: u32 = if all { 0 } else { p.parse().unwrap_or(0) };
        let all = if !all && num == 0 {
            self.say("MsgNewScanAskAll").await?;
            let a = self.yes_no(true).await?;
            self.newline();
            a
        } else {
            all
        };
        self.newscan(all, num).await
    }

    async fn newscan(&mut self, all: bool, num: u32) -> io::Result<()> {
        let total = self.state.areas().areas.len() as u32;
        if total == 0 {
            return Ok(());
        }
        let areas: Vec<u32> = if all {
            self.say_line("MsgNewScanInit").await?;
            (1..=total).collect()
        } else {
            vec![if num == 0 { self.user.cur_msg_area } else { num }.clamp(1, total)]
        };
        // A scan of all areas leaves out those taken out with `ME`.
        let off = if all { self.scan_off('M') } else { Default::default() };
        for n in areas {
            let Some(a) = self.msg_area(n) else { continue };
            if !self.area_open(&a) || off.contains(a.key()) {
                continue;
            }
            let count = self.state.msgs.count(a.key()).map_err(io_err)?.to_string();
            let s = self.strings().fmt("MsgNewScanStart", &[&a.area.name, &count]);
            self.show(&s).await?;
            self.newline();
            let first = self.first_new(&a)?;
            let go_on = first == 0 || self.read_messages(&a, first, false, false).await?;
            let s = self.strings().fmt("MsgNewScanEnd", &[&a.area.name, &count]);
            self.show_line(&s).await?;
            if !go_on || self.hangup {
                break;
            }
        }
        if all {
            self.say_line("MsgNewScanDone").await?;
        }
        Ok(())
    }

    /// `maScanForMandatory` (`MM`).
    pub async fn mandatory_scan(&mut self, param: &str) -> io::Result<()> {
        self.say("MsgScanMandatory").await?;
        let total = self.state.areas().areas.len() as u32;
        let range = match param.trim().parse::<u32>() {
            Ok(n) if n > 0 => n.min(total)..=n.min(total),
            _ => 1..=total,
        };
        for n in range {
            let Some(a) = self.msg_area(n) else { continue };
            if !a.area.mandatory || !self.area_open(&a) {
                continue;
            }
            let first = self.first_new(&a)?;
            if first > 0 {
                self.read_messages(&a, first, true, false).await?;
            }
        }
        self.newline();
        Ok(())
    }

    /// `maSetPointerDate` (`MS`). Starts from the date the pointer is at
    /// in this area (2.20), or the last call.
    pub async fn set_pointer_date(&mut self) -> io::Result<()> {
        self.say("MsgAskPointer").await?;
        let at = self.current_area().and_then(|a| {
            let last = self.state.msgs.lastread(self.user.id, a.key()).ok()?;
            self.state.msgs.get(a.key(), last).ok().flatten().map(|m| m.posted.date())
        });
        let cur = crate::user::fmt_date(at.or(self.user.last_call));
        let s = self.read_string(ReadOpts::new(8).allowed("0123456789/").prefill(cur)).await?;
        self.newline();
        let Some(date) = parse_date(&s) else { return Ok(()) };
        let when = date.and_hms_opt(0, 0, 0).unwrap_or_default();
        self.say("MsgAskAllPointers").await?;
        let areas: Vec<u32> = if self.yes_no(true).await? {
            (1..=self.state.areas().areas.len() as u32).collect()
        } else {
            vec![self.user.cur_msg_area]
        };
        for n in areas {
            if let Some(a) = self.msg_area(n) {
                let num = self.state.msgs.last_before(a.key(), when).map_err(io_err)?;
                self.state.msgs.set_lastread(self.user.id, a.key(), num).map_err(io_err)?;
            }
        }
        Ok(())
    }

    // ---- email ----------------------------------------------------------

    /// `emailEnterEmail` (`EE`): `to;subject`.
    pub async fn enter_email(&mut self, to: &str, subject: &str) -> io::Result<()> {
        if !self.acs(&self.state.config().messages.acs_post_email.clone()) {
            self.say_line("EmailNoAccess").await?;
            return Ok(());
        }
        let a = self.email_area();
        self.post(&a, to, subject, None).await?;
        Ok(())
    }

    /// Unread (undeleted) email for the caller (`%MW`).
    pub fn mail_waiting(&self) -> u32 {
        self.mailbox().map_or(0, |v| v.len() as u32)
    }

    /// The caller's email: all of it, or with `E+` on, only what carries
    /// the `E\` read tag.
    pub(crate) fn mailbox(&self) -> anyhow::Result<Vec<Message>> {
        let mail = self.state.msgs.mail_for(&self.state.areas().email.file, self.user.id, &self.user.handle, &self.user.real_name, false)?;
        Ok(if self.use_tag { mail.into_iter().filter(|m| m.tag == self.read_tag).collect() } else { mail })
    }

    /// `emailReadEmail`: `ER` (all = false, ask = false), `EC` (ask first,
    /// quiet if none), `*R` (every message, sysop).
    pub async fn read_email(&mut self, all: bool, ask: bool) -> io::Result<()> {
        let a = self.email_area();
        let mut mail = if all {
            self.state.msgs.all(a.key()).map_err(io_err)?
        } else {
            self.mailbox().map_err(io_err)?
        };
        if mail.is_empty() {
            if !ask {
                self.say_line("EmailNoMail").await?;
            }
            return Ok(());
        }
        if ask {
            self.say("EmailAskRead").await?;
            if !self.yes_no(true).await? {
                return Ok(());
            }
        }
        self.list_mail(&mail).await?;
        let total = mail.len() as u32;
        let s = self.strings().fmt("EmailSelectToRead", &[&total.to_string()]);
        self.show(&s).await?;
        let s = self.read_string(ReadOpts::new(4).allowed(DIGITS).prefill("1")).await?;
        self.newline();
        let mut cur: u32 = match s.parse() {
            Ok(n) if (1..=total).contains(&n) => n,
            _ => return Ok(()),
        };
        let mut show = true;
        loop {
            if self.hangup {
                return Ok(());
            }
            let idx = cur as usize - 1;
            if show {
                let m = mail[idx].clone();
                self.show_message(&a, &m, cur, total).await?;
            }
            show = false;
            let s = self.strings().fmt("EmailReadPrompt", &[&cur.to_string(), &total.to_string()]);
            self.show(&s).await?;
            let (key, typed) = self.read_command_or_number(total, "QA?RDE-").await?;
            if let Some(n) = typed {
                self.newline();
                if (1..=total).contains(&n) {
                    cur = n;
                }
                show = true;
                continue;
            }
            match key {
                'Q' => {
                    self.full_screen_done();
                    break;
                }
                '\r' => {
                    if cur < total {
                        cur += 1;
                        show = true;
                    } else if !all {
                        break;
                    } else {
                        show = true;
                    }
                }
                '-' => {
                    if cur > 1 {
                        cur -= 1;
                    }
                    show = true;
                }
                '?' => {
                    if self.show_file("EMAILHLP").await? {
                        self.prompt_key().await?;
                        show = true;
                    }
                }
                'A' => show = true,
                'R' => {
                    let orig = mail[idx].clone();
                    if self.post(&a, "", "", Some(&orig)).await?.is_some() {
                        let m = &mut mail[idx];
                        m.replies += 1;
                        self.newline();
                        if self.state.config().messages.ask_delete_replied && !m.deleted {
                            self.say("EmailKillOriginal").await?;
                            if self.yes_no(true).await? {
                                mail[idx].deleted = true;
                                self.state.msgs.set_deleted(mail[idx].id, true).map_err(io_err)?;
                            }
                            self.newline();
                        }
                    }
                }
                'D' => {
                    let m = &mut mail[idx];
                    m.deleted = !m.deleted;
                    let deleted = m.deleted;
                    self.state.msgs.set_deleted(m.id, deleted).map_err(io_err)?;
                    self.say_line(if deleted { "MsgDeleted" } else { "MsgUndeleted" }).await?;
                    show = true;
                }
                'E' if all => {
                    let mut m = mail[idx].clone();
                    self.edit_existing(&a, &mut m).await?;
                    mail[idx] = m;
                    show = true;
                }
                _ => {}
            }
            if !all && mail[cur as usize - 1].deleted {
                if cur < total {
                    cur += 1;
                    show = true;
                } else {
                    break;
                }
            }
        }
        if self.state.config().messages.ask_delete_all_read && !all {
            let left: Vec<usize> = (0..mail.len()).filter(|&i| !mail[i].deleted).collect();
            if !left.is_empty() {
                self.newline();
                self.say("EmailAskKillAll").await?;
                if self.yes_no(false).await? {
                    for i in left {
                        mail[i].deleted = true;
                        self.state.msgs.set_deleted(mail[i].id, true).map_err(io_err)?;
                    }
                }
            }
        }
        self.newline();
        Ok(())
    }

    async fn list_mail(&mut self, mail: &[Message]) -> io::Result<()> {
        let mut rep = self.repeat_begin("MAIL", false).await?;
        if rep.is_none() {
            self.clear_screen();
            self.newline();
            self.show_line("|U4 Num  Message Title          Sender          Receiver        Date      Time").await?;
            self.rule().await?;
            self.set_slot(Slot::Text);
        }
        let mut lines = 4;
        for (i, m) in mail.iter().enumerate() {
            let date = m.posted.format("%m/%d/%y").to_string();
            let time = m.posted.format("%-I:%M%P").to_string();
            let more = match rep.as_mut() {
                Some(r) => {
                    let w = waves([
                        ("MT", m.subject.clone()),
                        ("SN", m.from.name.clone()),
                        ("RN", m.to.name.clone()),
                        ("TI", time),
                        ("DT", date),
                        ("MN", (i + 1).to_string()),
                    ]);
                    self.repeat_row(r, &w, false).await?
                }
                None => {
                    let line = format!(
                        " {} {} {} {} {} {}",
                        resize(&(i + 1).to_string(), 4),
                        resize(&m.subject, 22),
                        resize(&m.from.name, 15),
                        resize(&m.to.name, 15),
                        resize(&date, 9),
                        squish(&time, 8)
                    );
                    self.write(&line);
                    self.newline();
                    lines += 1;
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => self.rule().await?,
        }
        if self.screen.where_x() != 1 {
            self.newline();
        }
        Ok(())
    }
}
