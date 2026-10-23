//! Paging the SysOp and chatting (`CHATMODE.PAS`: `CP`, `>C`).
//!
//! The original paged the local console and chatted on it. Here the
//! SysOp is a caller too: a page goes to any node where a SysOp is logged
//! in; if they answer, the two nodes chat on the split screen
//! (CHATMODE.ANS: `~st`-`~sb` is the SysOp's window, `~ut`-`~ub` the
//! caller's), each keystroke sent to the other node as it's typed, so
//! both see the same two windows. `/q` ends the chat; `/bye` from the
//! SysOp hangs the caller up; `/cls` clears both windows; `/?` shows
//! CHATHELP.
//!
//! Availability (`mSysOpAvail` isn't in the surviving source) is the chat
//! hours in the config, and a SysOp being online.

use std::io;
use std::time::{Duration, Instant};

use chrono::{NaiveTime};
use fluxbbs_term::{Color, Screen, Slot, cp437};

use crate::input::{Key, ReadOpts};
use crate::nodes::NodeEvent;
use crate::session::{Session, waves};

/// A page beeps this often, for `[chat] page_beeps` rounds.
const BEEP_EVERY: Duration = Duration::from_secs(5);

/// Is `now` within the chat hours? Equal or unparsable times mean
/// always; hours may wrap past midnight.
pub fn in_hours(start: &str, end: &str, now: NaiveTime) -> bool {
    let parse = |s: &str| NaiveTime::parse_from_str(s.trim(), "%H:%M").ok();
    match (parse(start), parse(end)) {
        (Some(a), Some(b)) if a < b => (a..b).contains(&now),
        (Some(a), Some(b)) if a > b => now >= a || now < b,
        _ => true,
    }
}

/// One party's window on the chat screen. Both sides apply the same
/// keystrokes in the same order, so their screens agree.
pub struct Window {
    x: u16,
    y: u16,
    width: usize,
    height: u16,
    color: Color,
    row: u16,
    line: String,
}

/// Keys sent between the two nodes besides text: clear both windows.
const KEY_CLS: u8 = 0x0C;

impl Window {
    pub fn new(x: u16, y: u16, right: u16, bottom: u16, color: Color) -> Self {
        Self { x, y, width: (right.max(x) - x + 1) as usize, height: bottom.max(y) - y + 1, color, row: 0, line: String::new() }
    }

    pub fn clear(&mut self, s: &mut Screen) {
        s.set_color(self.color);
        for r in 0..self.height {
            s.goto(self.x, self.y + r);
            s.write_str(&" ".repeat(self.width));
        }
        self.row = 0;
        self.line.clear();
        self.cursor(s);
    }

    /// Put the cursor where this window's text continues (only moving it
    /// if it isn't there already: every keystroke comes through here).
    pub fn cursor(&self, s: &mut Screen) {
        let (x, y) = (self.x + self.line.chars().count() as u16, self.y + self.row);
        if (s.where_x(), s.where_y()) != (x, y) {
            s.goto(x, y);
        }
        s.set_color(self.color);
    }

    fn newline(&mut self, s: &mut Screen) {
        self.line.clear();
        self.row += 1;
        if self.row >= self.height {
            self.clear(s);
        } else {
            self.cursor(s);
        }
    }

    /// Apply one key: a CP437 character, backspace, enter or [`KEY_CLS`].
    pub fn apply(&mut self, s: &mut Screen, key: u8) {
        self.cursor(s);
        match key {
            b'\r' => self.newline(s),
            0x08 => {
                if self.line.pop().is_some() {
                    self.cursor(s);
                    s.write_str(" ");
                    self.cursor(s);
                }
            }
            KEY_CLS => self.clear(s),
            0..=0x1F | 0x7F => {}
            c => {
                if self.line.chars().count() >= self.width {
                    // Wrap at the last space, taking the word along.
                    match self.line.rfind(' ').filter(|&i| i > 0) {
                        Some(i) => {
                            let tail = self.line[i + 1..].to_string();
                            s.goto(self.x + i as u16, self.y + self.row);
                            s.write_str(&" ".repeat(tail.chars().count() + 1));
                            self.newline(s);
                            s.write_str(&tail);
                            self.line = tail;
                        }
                        None => self.newline(s),
                    }
                }
                s.write_bytes(&[c]);
                self.line.push(cp437::TO_UNICODE[c as usize]);
            }
        }
    }

    pub fn line(&self) -> &str {
        &self.line
    }
}

impl Session {
    /// A node, other than this one, with a SysOp logged in.
    fn sysop_node(&self) -> Option<(usize, String)> {
        let me = self.node.number();
        self.state.nodes.online().into_iter().find(|(n, i)| *n != me && i.sysop && !i.user.is_empty()).map(|(n, i)| (n, i.user))
    }

    fn node_online(&self, n: usize) -> bool {
        self.state.nodes.online().iter().any(|(m, _)| *m == n)
    }

    /// `cpMail`: offer to leave the SysOp email instead.
    async fn page_mail(&mut self, matrix: bool, sysop: &str) -> io::Result<()> {
        let cfg = self.state.config();
        let (ask, acs) = (cfg.chat.ask_email, cfg.messages.acs_post_email.clone());
        if matrix || !ask || !self.acs(&acs) {
            return Ok(());
        }
        let s = self.strings().fmt("PageLeaveEmail", &[sysop]);
        self.show(&s).await?;
        if self.yes_no(false).await? {
            self.enter_email(sysop, "").await?;
        }
        Ok(())
    }

    /// `chatPage` (`CP`, and `>C` from the matrix).
    pub async fn page_sysop(&mut self, matrix: bool) -> io::Result<()> {
        self.newline();
        let cfg = self.state.config();
        let ch = cfg.chat.clone();
        let alias = cfg.bbs.sysop_alias.clone();
        drop(cfg);
        if !matrix && ch.max_page_times != 0 && self.pages >= ch.max_page_times as u32 {
            let s = self.strings().fmt("PageNoMore", &[&ch.max_page_times.to_string()]);
            return self.show_line(&s).await;
        }
        let Some((sysop_node, sysop_name)) = self.sysop_node() else {
            self.say_line("PageSysOpNotAvail").await?;
            return self.page_mail(matrix, &alias).await;
        };
        if !in_hours(&ch.start, &ch.end, crate::clock::now().time()) {
            if matrix || !self.acs(&ch.override_acs) {
                self.say_line("PageSysOpNotAvail").await?;
                return self.page_mail(matrix, &sysop_name).await;
            }
            self.say("PageAskOverride").await?;
            if !self.yes_no(false).await? {
                return self.page_mail(matrix, &sysop_name).await;
            }
            if !ch.override_password.is_empty() {
                self.say("PageAskPassword").await?;
                let pw = self.read_string(ReadOpts::new(20).password()).await?;
                self.newline();
                if !pw.eq_ignore_ascii_case(&ch.override_password) {
                    tracing::warn!("{}: wrong chat override password", self.user.handle);
                    return self.say_line("PageBadPw").await;
                }
                tracing::info!("{}: chat override password accepted", self.user.handle);
            }
        }
        self.say("PageAskWhy").await?;
        let why = self.read_string(ReadOpts::new(70)).await?;
        self.newline();
        if why.trim().is_empty() {
            return Ok(());
        }
        tracing::info!("{}: paged the sysop{}; reason: {why:?}", self.user.handle, if matrix { " from the matrix" } else { "" });
        if !matrix {
            self.pages += 1;
        }
        let me = self.node.number();
        let nodes = self.state.nodes.clone();
        // Anything left from an earlier chat shouldn't end this one.
        while self.node.take_if(|e| matches!(e, NodeEvent::ChatKey { .. } | NodeEvent::ChatEnd { .. } | NodeEvent::PageAnswer { .. })).is_some() {}
        nodes.send(sysop_node, NodeEvent::Page { from: me, user: self.user.handle.clone(), reason: why.trim().to_string() });
        self.node.update(|n| n.activity = "Paging the SysOp".into());
        self.say_line("PageBegin").await?;
        let waker = self.node.waker();
        let mut answer: Option<bool> = None;
        for _ in 0..ch.page_beeps.max(1) {
            self.say("PagePaging").await?;
            self.screen.write_bytes(b"\x07");
            self.flush().await?;
            tokio::select! {
                k = self.read_key() => {
                    k?;
                    break;
                }
                _ = waker.notified() => {}
                _ = tokio::time::sleep(BEEP_EVERY) => {}
            }
            if let Some(NodeEvent::PageAnswer { accept, .. }) = self.node.take_if(|e| matches!(e, NodeEvent::PageAnswer { from, .. } if *from == sysop_node)) {
                answer = Some(accept);
                break;
            }
        }
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        self.newline();
        if answer == Some(true) {
            return self.sysop_chat(sysop_node, &sysop_name, false).await;
        }
        // Tell a SysOp who answers late that we've gone.
        nodes.send(sysop_node, NodeEvent::ChatEnd { from: me, hangup: false });
        self.say_line("PageNoAnswer").await?;
        self.page_mail(matrix, &sysop_name).await?;
        if matrix {
            self.prompt_key().await?;
        }
        Ok(())
    }

    /// A page has arrived for this (SysOp's) node: ask, and chat.
    pub(crate) async fn answer_page(&mut self, from: usize, user: String, reason: String) -> io::Result<()> {
        if self.screen.where_x() != 1 {
            self.newline();
        }
        self.newline();
        let s = self.strings().fmt("PageIncoming", &[&user, &from.to_string(), &reason]);
        self.show_line(&s).await?;
        self.say("PageAnswerAsk").await?;
        let yes = self.yes_no(true).await?;
        self.newline();
        let gave_up = self.node.take_if(|e| matches!(e, NodeEvent::ChatEnd { from: f, .. } if *f == from)).is_some() || !self.node_online(from);
        if gave_up {
            return self.say_line("PageGaveUp").await;
        }
        self.state.nodes.send(from, NodeEvent::PageAnswer { from: self.node.number(), accept: yes });
        if yes {
            self.sysop_chat(from, &user, true).await?;
        }
        Ok(())
    }

    /// Draw CHATMODE (or a plain split) and make the two windows:
    /// (the SysOp's, the caller's).
    async fn chat_windows(&mut self, peer_name: &str, i_am_sysop: bool) -> io::Result<(Window, Window)> {
        self.set_slot(Slot::Text);
        self.clear_screen();
        // The screen names the caller with %UN; on the SysOp's side that's
        // the other party.
        let mine = if i_am_sysop { Some(std::mem::replace(&mut self.user.handle, peer_name.to_string())) } else { None };
        let w = waves([("ND", self.node.number().to_string())]);
        let shown = self.show_file_with("CHATMODE", &w).await;
        if let Some(h) = mine {
            self.user.handle = h;
        }
        let pos = |s: &Self, k: &[u8; 2], upper: &[u8; 2]| s.positions.get(k).or_else(|| s.positions.get(upper)).copied();
        if shown?
            && let (Some(st), Some(sb), Some(ut), Some(ub)) =
                (pos(self, b"st", b"ST"), pos(self, b"sb", b"SB"), pos(self, b"ut", b"UT"), pos(self, b"ub", b"UB"))
            && sb.1 >= st.1
            && ub.1 >= ut.1
        {
            return Ok((Window::new(st.0, st.1, sb.0.max(st.0 + 20), sb.1, st.2), Window::new(ut.0, ut.1, ub.0.max(ut.0 + 20), ub.1, ut.2)));
        }
        let rows = self.screen.rows().max(8);
        let mid = rows / 2;
        self.clear_screen();
        let sysop_alias = self.state.config().bbs.sysop_alias.clone();
        let (top_name, bottom_name) = if i_am_sysop { (self.user.handle.clone(), peer_name.to_string()) } else { (sysop_alias, self.user.handle.clone()) };
        self.set_slot(Slot::Border);
        self.screen.goto(1, 1);
        self.write(&format!("─── {top_name} {}", "─".repeat(74usize.saturating_sub(top_name.chars().count()))));
        self.screen.goto(1, mid);
        self.write(&format!("─── {bottom_name} {}", "─".repeat(74usize.saturating_sub(bottom_name.chars().count()))));
        let hi = self.user.palette.get(Slot::InfoHi);
        let lo = self.user.palette.get(Slot::Info);
        Ok((Window::new(1, 2, 79, mid - 1, hi), Window::new(1, mid + 1, 79, rows, lo)))
    }

    /// `chatSplitScreen`: chat with `peer` until either side ends it.
    pub async fn sysop_chat(&mut self, peer: usize, peer_name: &str, i_am_sysop: bool) -> io::Result<()> {
        let started = Instant::now();
        tracing::info!("{}: chat with {peer_name} (node {peer}) started", self.user.handle);
        self.hold_node_msgs = true;
        self.node.update(|n| n.activity = format!("Chatting with {peer_name}"));
        let result = self.sysop_chat_inner(peer, peer_name, i_am_sysop).await;
        // Whatever ended it (a hangup included), the other side is told.
        self.state.nodes.send(peer, NodeEvent::ChatEnd { from: self.node.number(), hangup: false });
        self.hold_node_msgs = false;
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        let minutes = (started.elapsed().as_secs() / 60) as u32;
        if !i_am_sysop && self.state.config().chat.restore_time {
            self.user.time_today += minutes;
        }
        tracing::info!("{}: chat with {peer_name} ended after {minutes} min", self.user.handle);
        let hangup = result?;
        self.set_slot(Slot::Text);
        self.screen.goto(1, self.screen.rows());
        self.newline();
        self.say_line("ChatTerminate").await?;
        if hangup {
            self.hangup = true;
        }
        Ok(())
    }

    /// Ok(true): the caller is to be hung up.
    async fn sysop_chat_inner(&mut self, peer: usize, peer_name: &str, i_am_sysop: bool) -> io::Result<bool> {
        let (mut sysop_win, mut user_win) = self.chat_windows(peer_name, i_am_sysop).await?;
        sysop_win.clear(&mut self.screen);
        user_win.clear(&mut self.screen);
        let (mut mine, mut theirs) = if i_am_sysop { (sysop_win, user_win) } else { (user_win, sysop_win) };
        let me = self.node.number();
        let nodes = self.state.nodes.clone();
        let waker = self.node.waker();
        let send = |key: u8| {
            nodes.send(peer, NodeEvent::ChatKey { from: me, key });
        };
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            mine.cursor(&mut self.screen);
            self.flush().await?;
            tokio::select! {
                key = self.read_key() => match key? {
                    Key::Esc => return Ok(false),
                    Key::Enter => {
                        let typed = mine.line().trim().to_string();
                        mine.apply(&mut self.screen, b'\r');
                        send(b'\r');
                        let cmd = typed.strip_prefix('/').map(|c| c.trim().to_ascii_lowercase());
                        match cmd.as_deref() {
                            Some("q" | "x" | "quit" | "exit") => return Ok(false),
                            Some("bye") if i_am_sysop => {
                                nodes.send(peer, NodeEvent::ChatEnd { from: me, hangup: true });
                                return Ok(false);
                            }
                            Some("bye") => return Ok(true),
                            Some("cls") => {
                                mine.clear(&mut self.screen);
                                theirs.clear(&mut self.screen);
                                send(KEY_CLS);
                            }
                            // CHATHELP, then the screen again (only this
                            // side's windows start over).
                            Some("?") => {
                                self.set_slot(Slot::Text);
                                self.clear_screen();
                                if self.show_file("CHATHELP").await? {
                                    self.prompt_key().await?;
                                }
                                let (s_win, u_win) = self.chat_windows(peer_name, i_am_sysop).await?;
                                let (m, t) = if i_am_sysop { (s_win, u_win) } else { (u_win, s_win) };
                                mine = m;
                                theirs = t;
                                mine.clear(&mut self.screen);
                                theirs.clear(&mut self.screen);
                            }
                            _ => {}
                        }
                    }
                    Key::Backspace => {
                        mine.apply(&mut self.screen, 0x08);
                        send(0x08);
                    }
                    Key::Char(c @ 0x20..=0xFF) if c != 0x7F => {
                        mine.apply(&mut self.screen, c);
                        send(c);
                    }
                    _ => {}
                },
                _ = waker.notified() => {}
                _ = tick.tick() => {
                    if !self.node_online(peer) {
                        return Ok(false);
                    }
                }
            }
            while let Some(ev) = self.node.take_if(|e| matches!(e, NodeEvent::ChatKey { .. } | NodeEvent::ChatEnd { .. } | NodeEvent::Kick { .. })) {
                match ev {
                    NodeEvent::ChatKey { key: KEY_CLS, .. } => {
                        mine.clear(&mut self.screen);
                        theirs.clear(&mut self.screen);
                    }
                    NodeEvent::ChatKey { key, .. } => theirs.apply(&mut self.screen, key),
                    NodeEvent::ChatEnd { hangup, .. } => return Ok(hangup),
                    NodeEvent::Kick { .. } => return Err(crate::session::kicked()),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxbbs_term::Charset;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn chat_hours() {
        assert!(in_hours("11:00", "20:00", t(11, 0)));
        assert!(in_hours("11:00", "20:00", t(19, 59)));
        assert!(!in_hours("11:00", "20:00", t(20, 0)));
        assert!(!in_hours("11:00", "20:00", t(3, 0)));
        assert!(in_hours("22:00", "02:00", t(23, 0)), "wraps midnight");
        assert!(in_hours("22:00", "02:00", t(1, 0)));
        assert!(!in_hours("22:00", "02:00", t(12, 0)));
        assert!(in_hours("", "", t(12, 0)) && in_hours("09:00", "09:00", t(3, 0)), "unset: always");
    }

    #[test]
    fn window_wraps_and_scrolls() {
        let mut s = Screen::new(Charset::Utf8, 25);
        let mut w = Window::new(3, 2, 12, 3, Color::DEFAULT);
        w.clear(&mut s);
        assert_eq!((s.where_x(), s.where_y()), (3, 2));
        for b in b"hello there" {
            w.apply(&mut s, *b);
        }
        // "hello " filled the 10 columns; "there" moved to row 2.
        assert_eq!(w.line(), "there");
        assert_eq!((s.where_x(), s.where_y()), (8, 3));
        w.apply(&mut s, 0x08);
        assert_eq!((w.line(), s.where_x()), ("ther", 7));
        w.apply(&mut s, b'\r');
        // Past the last row: the window clears and starts over.
        assert_eq!((w.line(), s.where_x(), s.where_y()), ("", 3, 2));
        w.apply(&mut s, 0xB0);
        assert_eq!(w.line(), "░");
    }
}
