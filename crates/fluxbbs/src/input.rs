//! Caller input: keys (`iReadKey`), line input (`iReadString`) and yes/no
//! prompts (`iYesNo`).

use std::io;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PgUp,
    PgDn,
    Ins,
    Del,
}

impl Key {
    /// The character, upper-cased, if this is a plain key.
    pub fn upper(self) -> Option<char> {
        match self {
            Key::Char(c) => Some(c.to_ascii_uppercase() as char),
            _ => None,
        }
    }
}

/// `tInFlag`: how typed text is capitalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Case {
    #[default]
    Normal,
    /// First letter upper-case.
    Capital,
    Upper,
    Lower,
    /// First letter of each word upper-case.
    Mixed,
    /// Vowels lower, the rest upper (MiKe FRiCKeR).
    Weird,
    /// Vowels upper, the rest lower (mIkE frIckEr).
    Warped,
    /// Only I's lower (FiEND).
    Cool,
}

impl Case {
    /// `AliasFormat` 1-8 from the config.
    pub fn from_alias_format(n: u8) -> Case {
        [Case::Normal, Case::Capital, Case::Upper, Case::Lower, Case::Mixed, Case::Weird, Case::Warped, Case::Cool]
            .get(n.saturating_sub(1) as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Transform `c` given the text typed before it.
    pub fn apply(self, before: &str, c: char) -> char {
        let vowel = "aeiouAEIOU".contains(c);
        let word_start = before.chars().last().is_none_or(|p| p == ' ');
        match self {
            Case::Normal => c,
            Case::Capital if before.is_empty() => c.to_ascii_uppercase(),
            Case::Capital => c,
            Case::Upper => c.to_ascii_uppercase(),
            Case::Lower => c.to_ascii_lowercase(),
            Case::Mixed if word_start => c.to_ascii_uppercase(),
            Case::Mixed => c.to_ascii_lowercase(),
            Case::Weird if vowel => c.to_ascii_lowercase(),
            Case::Weird => c.to_ascii_uppercase(),
            Case::Warped if vowel => c.to_ascii_uppercase(),
            Case::Warped => c.to_ascii_lowercase(),
            Case::Cool if c.eq_ignore_ascii_case(&'i') => 'i',
            Case::Cool => c.to_ascii_uppercase(),
        }
    }
}

/// Options for [`Session::read_string`].
#[derive(Debug, Clone, Default)]
pub struct ReadOpts {
    pub max: usize,
    pub case: Case,
    /// Echo the password character instead of the text.
    pub password: bool,
    /// Accept only these characters (empty = any printable).
    pub allowed: std::borrow::Cow<'static, str>,
    /// Keep asking until something is entered.
    pub required: bool,
    /// Pre-filled text.
    pub default: String,
    /// A pattern such as `mm/dd/yy` (new): shown dim where the answer
    /// goes, its letters taken by digits and its other characters filled
    /// in as the caller types.
    pub mask: &'static str,
    /// Tab completes a user's handle (2.0's alias completion).
    pub complete_users: bool,
    /// Esc gives up, answering nothing (new).
    pub esc_cancels: bool,
    /// Leading spaces kept and Tab as spaces to the next stop (new: a
    /// message's lines, so code keeps its indentation).
    pub indent: bool,
}

impl ReadOpts {
    pub fn new(max: usize) -> Self {
        Self { max, ..Self::default() }
    }

    pub fn case(mut self, case: Case) -> Self {
        self.case = case;
        self
    }

    /// Echo the password character. Input keeps its case (the original
    /// upper-cased it): account passwords are case-sensitive now, and the
    /// other passwords are compared ignoring case.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    pub fn allowed(mut self, chars: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        self.allowed = chars.into();
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn complete_users(mut self) -> Self {
        self.complete_users = true;
        self
    }

    pub fn prefill(mut self, s: impl Into<String>) -> Self {
        self.default = s.into();
        self
    }

    pub fn esc_cancels(mut self) -> Self {
        self.esc_cancels = true;
        self
    }

    pub fn mask(mut self, mask: &'static str) -> Self {
        self.mask = mask;
        self
    }

    pub fn indent(mut self) -> Self {
        self.indent = true;
        self
    }
}

/// Tab stops every this many columns, in a message's lines.
pub const TAB: usize = 4;

/// The spaces a Tab adds after `len` characters.
pub fn tab_spaces(len: usize) -> usize {
    TAB - len % TAB
}

pub const DIGITS: &str = "0123456789";

enum Wait {
    /// Block, subject to the inactivity timeout.
    Idle,
    /// Give up after a short time (escape sequence continuation).
    Brief,
}

impl Session {
    /// Seconds idle before the caller is let go (0: never): shorter
    /// before logging in, so idle connections from bots don't hold nodes.
    /// New: callers matching `idle_exempt_acs` are never timed out (a
    /// flag ACS like `FX`, or `s255`).
    pub(crate) fn idle_limit(&self) -> u64 {
        let cfg = self.state.config();
        if !self.logged_in {
            cfg.security.prelogin_idle_seconds
        } else if !cfg.bbs.idle_exempt_acs.trim().is_empty() && self.acs(&cfg.bbs.idle_exempt_acs) {
            0
        } else {
            cfg.bbs.inactivity_seconds
        }
    }

    async fn next_byte(&mut self, wait: Wait) -> io::Result<Option<u8>> {
        loop {
            if let Some(b) = self.input.pop_front() {
                let after_cr = std::mem::replace(&mut self.last_cr, b == b'\r');
                if after_cr && (b == b'\n' || b == 0) {
                    continue;
                }
                return Ok(Some(b));
            }
            self.flush().await?;
            let mut buf = [0u8; 256];
            let n = match wait {
                Wait::Brief => match tokio::time::timeout(Duration::from_millis(40), self.io.read(&mut buf)).await {
                    Ok(r) => r?,
                    Err(_) => return Ok(None),
                },
                Wait::Idle => {
                    let idle = self.idle_limit();
                    let since = self.idle_since.take().unwrap_or_else(tokio::time::Instant::now);
                    let deadline = (idle > 0).then(|| since + Duration::from_secs(idle));
                    // Wait for the caller, showing node messages as they
                    // arrive (unless something like the editor holds them).
                    loop {
                        // A forced event: go, whatever the caller's doing.
                        if let Some(text) = self.node.take_kick() {
                            self.newline();
                            self.write_with_controls(&text);
                            self.newline();
                            let _ = self.flush().await;
                            return Err(crate::session::kicked());
                        }
                        if !self.hold_node_msgs && !self.hold_account_changes {
                            Box::pin(self.take_account_changes()).await?;
                        }
                        if !self.hold_node_msgs && self.node.has_messages() {
                            self.show_node_messages();
                            self.flush().await?;
                        }
                        // Someone paging the SysOp here: ask, chat, then
                        // carry on waiting for whatever was being typed.
                        if !self.hold_node_msgs && let Some((from, user, reason)) = self.node.take_page() {
                            Box::pin(self.answer_page(from, user, reason)).await?;
                            self.flush().await?;
                            continue;
                        }
                        let waker = self.node.waker();
                        // Chat loops wait on the waker themselves (and
                        // hold node messages), so leave it to them.
                        let listen = !self.hold_node_msgs;
                        let timeout = async {
                            match deadline {
                                Some(d) => tokio::time::sleep_until(d).await,
                                None => std::future::pending().await,
                            }
                        };
                        tokio::select! {
                            r = self.io.read(&mut buf) => break r?,
                            _ = waker.notified(), if listen => continue,
                            _ = timeout => {
                                // Not `show`: that can wait for keys itself.
                                let msg = self.strings().get("TimeInactive").to_string();
                                self.newline();
                                self.write_with_controls(&msg);
                                self.newline();
                                let _ = self.flush().await;
                                return Err(io::Error::new(io::ErrorKind::TimedOut, "inactivity timeout"));
                            }
                        }
                    }
                }
            };
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            if self.caller.size.has_changed().unwrap_or(false) {
                let rows = self.caller.size.borrow_and_update().1;
                self.screen.set_rows(rows);
            }
            // A report can be split between reads (a slow link): while one
            // is expected, an unfinished one waits for the rest.
            let mut data = std::mem::take(&mut self.report_tail);
            data.extend_from_slice(&buf[..n]);
            let expecting = self.cursor_asks > self.cursor_reports;
            let (keys, reports, tail, da) = strip_cursor_reports(&data, expecting);
            self.cursor_reports += reports;
            // A terminal answers a few times at most; a flood of answers
            // is kept from filling memory.
            if self.device_attrs.len() < 32 {
                self.device_attrs.extend(da.iter().take(32 - self.device_attrs.len()).map(|a| String::from_utf8_lossy(a).into_owned()));
            }
            let keys = if self.screen.charset() == fluxbbs_term::Charset::Utf8 { utf8_keys(&keys, &mut self.utf8_tail) } else { keys };
            self.input.extend(keys);
            // No real report is this long: an unfinished one that keeps
            // growing is garbage, not a report.
            self.report_tail = if tail.len() > 1024 { Vec::new() } else { tail };
        }
    }

    /// Keep output from running ahead of the terminal: ask where the
    /// cursor is (it answers only once it has shown everything before the
    /// question) and wait, up to `wait`, until no more than `behind`
    /// questions are unanswered. Keeps fast output (animations) from
    /// piling up ahead of a slow link, where keys seem to do nothing
    /// until it drains; `behind` 1 lets one frame travel while the last
    /// is drawn. False if it timed out; true early if a key is waiting.
    pub async fn sync_output(&mut self, wait: Duration, behind: u32) -> io::Result<bool> {
        self.cursor_asks += 1;
        self.screen.write_bytes(b"\x1b[6n");
        let deadline = tokio::time::Instant::now() + wait;
        while self.cursor_asks.saturating_sub(self.cursor_reports) > behind {
            if !self.input.is_empty() {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                if self.cursor_reports > 0 {
                    tracing::warn!("{}: the terminal stopped answering cursor reports ({}s)", self.user.handle, wait.as_secs());
                }
                // Stop waiting on the unanswered ones.
                self.cursor_asks = self.cursor_reports + behind;
                return Ok(false);
            }
            if let Some(b) = self.next_byte(Wait::Brief).await? {
                self.input.push_front(b);
            }
        }
        self.flush().await?;
        Ok(true)
    }

    /// Has the caller pressed something? Waits a moment at most (40ms),
    /// so output can be paced by it and stopped by a key.
    pub async fn key_waiting(&mut self) -> io::Result<bool> {
        // The LF or NUL after a CR (telnet's Enter) isn't a key.
        if self.last_cr && matches!(self.input.front(), Some(b'\n' | 0)) {
            self.input.pop_front();
            self.last_cr = false;
        }
        if !self.input.is_empty() {
            return Ok(true);
        }
        Ok(match self.next_byte(Wait::Brief).await? {
            Some(b) => {
                self.input.push_front(b);
                true
            }
            None => false,
        })
    }

    /// Next key from the caller. Enter is a single `\r` whatever the
    /// client sends; VT/ANSI cursor-key sequences are decoded.
    pub async fn read_key(&mut self) -> io::Result<Key> {
        loop {
            let b = self.next_byte(Wait::Idle).await?.expect("idle wait returns a byte");
            self.last_key = std::time::Instant::now();
            return Ok(match b {
                b'\r' | b'\n' => Key::Enter,
                0x08 | 0x7F => Key::Backspace,
                b'\t' => Key::Tab,
                // A sequence we don't know (pasted colour, say) is
                // swallowed whole: as Esc, the letters after it would run
                // as commands.
                0x1B => match self.escape_sequence().await? {
                    Some(k) => k,
                    None => continue,
                },
                c => Key::Char(c),
            });
        }
    }

    async fn escape_sequence(&mut self) -> io::Result<Option<Key>> {
        let Some(intro) = self.next_byte(Wait::Brief).await? else { return Ok(Some(Key::Esc)) };
        if intro != b'[' && intro != b'O' {
            self.input.push_front(intro);
            return Ok(Some(Key::Esc));
        }
        let mut params = Vec::new();
        loop {
            let Some(b) = self.next_byte(Wait::Brief).await? else { return Ok(Some(Key::Esc)) };
            if (0x40..=0x7E).contains(&b) {
                return Ok(Some(match (b, params.as_slice()) {
                    (b'A', _) => Key::Up,
                    (b'B', _) => Key::Down,
                    (b'C', _) => Key::Right,
                    (b'D', _) => Key::Left,
                    (b'H', _) => Key::Home,
                    (b'F', _) | (b'K', _) => Key::End,
                    (b'~', b"1" | b"7") => Key::Home,
                    (b'~', b"4" | b"8") => Key::End,
                    (b'~', b"2") => Key::Ins,
                    (b'~', b"3") => Key::Del,
                    (b'~', b"5") => Key::PgUp,
                    (b'~', b"6") => Key::PgDn,
                    _ => return Ok(None),
                }));
            }
            if params.len() >= 16 {
                return Ok(None);
            }
            params.push(b);
        }
    }

    /// `iReadString`: read a line with editing. Backspace deletes,
    /// Ctrl-U/Ctrl-X clear, Enter finishes.
    pub async fn read_string(&mut self, opts: ReadOpts) -> io::Result<String> {
        if !opts.mask.is_empty() {
            return self.read_masked(opts.mask, opts.required).await;
        }
        let echo = self.state.config().prompts.password_echo;
        self.escaped = false;
        // New: in plain text the default isn't typed in for editing (a
        // bot's answer would be added to it): it's shown, and Enter takes it.
        let plain_default = self.plain() && !opts.default.is_empty();
        if plain_default {
            self.screen.write_text(&format!("[{}, - for none] ", opts.default));
        }
        loop {
            let mut s = String::new();
            for c in opts.default.chars().take(if plain_default { 0 } else { opts.max }) {
                s.push(c);
                self.screen.write_str(&c.to_string());
            }
            loop {
                match self.read_key().await? {
                    Key::Enter => break,
                    Key::Esc if opts.esc_cancels => {
                        self.escaped = true;
                        return Ok(String::new());
                    }
                    Key::Backspace => {
                        if s.pop().is_some() {
                            self.screen.backspace(1);
                        }
                    }
                    Key::Char(0x15 | 0x18) => {
                        self.screen.backspace(s.chars().count() as u16);
                        s.clear();
                    }
                    Key::Tab if opts.complete_users && !s.is_empty() => {
                        if let Some(h) = self.state.users.complete(&s).ok().flatten().filter(|h| h.chars().count() <= opts.max) {
                            self.screen.backspace(s.chars().count() as u16);
                            self.screen.write_str(&h);
                            s = h;
                        }
                    }
                    Key::Tab if opts.indent => {
                        for _ in 0..tab_spaces(s.chars().count()).min(opts.max.saturating_sub(s.chars().count())) {
                            s.push(' ');
                            self.screen.write_str(" ");
                        }
                    }
                    Key::Char(c) if s.chars().count() < opts.max && text_char(c).is_some() => {
                        let c = opts.case.apply(&s, text_char(c).unwrap_or(' '));
                        if !opts.allowed.is_empty() && !opts.allowed.contains(c) {
                            continue;
                        }
                        if c == ' ' && s.is_empty() && !opts.indent {
                            continue; // no leading spaces
                        }
                        s.push(c);
                        let shown = if opts.password { echo } else { c };
                        self.screen.write_str(&shown.to_string());
                    }
                    _ => {}
                }
            }
            let mut s = s.trim_end().to_string();
            if plain_default && s.is_empty() {
                s = opts.default.chars().take(opts.max).collect();
            } else if plain_default && s == "-" {
                s.clear();
            }
            if !opts.required || !s.is_empty() {
                return Ok(s);
            }
        }
    }

    /// `read_string` with a mask: digits fill its letters, and the rest
    /// (the `/` in `mm/dd/yy`) goes in by itself. Backspace takes back a
    /// digit and any filler before it, showing the mask again. Only a
    /// complete answer (or, unless required, none) is taken.
    async fn read_masked(&mut self, mask: &str, required: bool) -> io::Result<String> {
        let mask: Vec<char> = mask.chars().collect();
        let (ink, dim) = (self.screen.color(), fluxbbs_term::Color::new(8, 0));
        let (x0, y) = (self.screen.where_x(), self.screen.where_y());
        let mut s = String::new();
        // Show the mask from `from` on, and leave the cursor at the answer's end.
        let redraw = |me: &mut Self, s: &str, from: usize| {
            me.screen.goto(x0 + from as u16, y);
            me.screen.set_color(dim);
            me.screen.write_str(&mask[from..].iter().collect::<String>());
            me.screen.set_color(ink);
            me.screen.goto(x0 + s.chars().count() as u16, y);
        };
        redraw(self, &s, 0);
        loop {
            match self.read_key().await? {
                Key::Enter => {
                    let n = s.chars().count();
                    if n == mask.len() || n == 0 && !required {
                        self.screen.goto(x0 + n as u16, y);
                        self.screen.clear_eol();
                        return Ok(s);
                    }
                }
                Key::Backspace => {
                    while s.pop().is_some_and(|c| !c.is_ascii_digit()) {}
                    // Fillers left at the end go too.
                    while s.ends_with(|c: char| !c.is_ascii_digit()) {
                        s.pop();
                    }
                    let n = s.chars().count();
                    redraw(self, &s, n);
                }
                Key::Char(0x15 | 0x18) => {
                    s.clear();
                    redraw(self, &s, 0);
                }
                Key::Char(c) if c.is_ascii_digit() && s.chars().count() < mask.len() => {
                    s.push(c as char);
                    self.screen.write_str(&(c as char).to_string());
                    // Fill in what comes next, up to the next digit.
                    while let Some(&m) = mask.get(s.chars().count()) {
                        if m.is_ascii_alphabetic() {
                            break;
                        }
                        s.push(m);
                        self.screen.write_str(&m.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    /// New: a caller, by handle, with matches shown as it's typed: a box
    /// of those whose handles start with (then contain) what's typed, under
    /// the prompt or, near the bottom, over it. Arrows move, Tab fills the
    /// lit one in, Enter takes it (or exactly what's typed), Esc gives up.
    /// Only callers `keep` passes are offered. Plain text types a line
    /// (Tab completes).
    pub async fn pick_user(&mut self, keep: impl Fn(&crate::user::User) -> bool) -> io::Result<Option<crate::user::User>> {
        const ROWS: usize = 6;
        const WIDTH: usize = 30;
        if self.plain() {
            let s = self.read_string(ReadOpts::new(36).complete_users()).await?;
            self.newline();
            return Ok(self.state.users.find(&s, false).map_err(io::Error::other)?.filter(|u| keep(u)));
        }
        let (x0, y0) = (self.screen.where_x(), self.screen.where_y());
        let rows = self.screen.rows();
        let left = x0.min(80 - WIDTH as u16 - 1);
        let below = y0 as usize + ROWS + 2 < rows as usize;
        let top = if below { y0 + 1 } else { y0.saturating_sub(ROWS as u16 + 2).max(1) };
        let mut s = String::new();
        let mut at = 0;
        let picked = loop {
            let found: Vec<crate::user::User> = if s.is_empty() { Vec::new() } else { self.state.users.matching(&s, 20).map_err(io::Error::other)?.into_iter().filter(|u| keep(u)).take(ROWS).collect() };
            at = at.min(found.len().saturating_sub(1));
            // The box: a row a match, the lit one in the bar's colors.
            for i in 0..ROWS + 2 {
                self.screen.goto(left, top + i as u16);
                let line = match i {
                    0 => format!("|U5┌{}┐", "─".repeat(WIDTH - 2)),
                    i if i == ROWS + 1 => format!("|U5└{}┘", "─".repeat(WIDTH - 2)),
                    i => {
                        let name = found.get(i - 1).map_or(String::new(), |u| fluxbbs_term::codes::resize(&u.handle, WIDTH - 4));
                        let name = fluxbbs_term::codes::lit(&format!(" {name:<w$}", w = WIDTH - 3));
                        if i - 1 == at && i - 1 < found.len() { format!("|U5│|U8{name}|U5│") } else { format!("|U5│|U1{name}|U5│") }
                    }
                };
                self.write_with_controls(&line);
            }
            self.screen.goto(x0, y0);
            self.write_with_controls("|U3");
            self.screen.write_str(&s);
            self.screen.clear_eol();
            self.flush().await?;
            match self.read_key().await? {
                Key::Enter if !found.is_empty() => break found.get(at).cloned(),
                Key::Enter => break self.state.users.find(&s, false).map_err(io::Error::other)?.filter(|u| keep(u)),
                Key::Esc => break None,
                Key::Up => at = at.saturating_sub(1),
                Key::Down => at = (at + 1).min(found.len().saturating_sub(1)),
                Key::Tab => {
                    if let Some(u) = found.get(at) {
                        s = u.handle.clone();
                    }
                }
                Key::Backspace => {
                    s.pop();
                    at = 0;
                }
                Key::Char(c) if s.chars().count() < 36 && text_char(c).is_some_and(|c| c != ' ' || !s.is_empty()) => {
                    s.push(text_char(c).unwrap_or(' '));
                    at = 0;
                }
                _ => {}
            }
        };
        // Take the box away, and leave the answer on the prompt.
        for i in 0..ROWS + 2 {
            self.screen.goto(left, top + i as u16);
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.screen.write_str(&" ".repeat(WIDTH));
        }
        self.screen.goto(x0, y0);
        self.write_with_controls("|U3");
        self.screen.write_str(picked.as_ref().map_or("", |u| u.handle.as_str()));
        self.screen.clear_eol();
        self.newline();
        Ok(picked)
    }

    /// `iYesNo`: a yes/no question, as a lightbar if the user prefers.
    pub async fn yes_no(&mut self, default: bool) -> io::Result<bool> {
        if self.plain() {
            return self.yes_no_line(default).await;
        }
        if self.user.yes_no_bars {
            return self.yes_no_bar(default).await;
        }
        let p = &self.state.config().prompts;
        let (prompt, yes, no) = (if default { p.yes.clone() } else { p.no.clone() }, p.yes_word.clone(), p.no_word.clone());
        self.show(&prompt).await?;
        let answer = loop {
            match self.read_key().await? {
                Key::Enter => break default,
                k if k.upper() == Some('Y') => break true,
                k if k.upper() == Some('N') => break false,
                _ => {}
            }
        };
        self.show_line(if answer { &yes } else { &no }).await?;
        Ok(answer)
    }

    /// New: plain text's yes/no, answered with a line.
    async fn yes_no_line(&mut self, default: bool) -> io::Result<bool> {
        loop {
            self.write(if default { " (Y/n) " } else { " (y/N) " });
            let s = self.read_string(ReadOpts::new(3)).await?;
            self.newline();
            match s.to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => {}
            }
        }
    }

    async fn yes_no_bar(&mut self, default: bool) -> io::Result<bool> {
        let (yes_bar, no_bar) = (self.state.config().prompts.yes_bar.clone(), self.state.config().prompts.no_bar.clone());
        // New: a bar that would run past column 79 goes on the next line;
        // wrapped, its redraws landed a line down and showed it twice.
        let wide = self.visible_len(yes_bar.as_bytes()).max(self.visible_len(no_bar.as_bytes()));
        if self.screen.where_x() as usize + wide > 80 {
            self.newline();
            self.write("  ");
        }
        let x = self.screen.where_x();
        let mut yes = default;
        self.show(if yes { &yes_bar } else { &no_bar }).await?;
        loop {
            let key = self.read_key().await?;
            let c = key.upper();
            let toggle = matches!(key, Key::Left | Key::Right) || matches!(c, Some('4' | '6' | ' '));
            let pick = match c {
                Some('Y') => Some(true),
                Some('N') => Some(false),
                _ => None,
            };
            if toggle || pick.is_some_and(|p| p != yes) {
                yes = pick.unwrap_or(!yes);
                self.screen.pos_x(x);
                self.show(if yes { &yes_bar } else { &no_bar }).await?;
            }
            if key == Key::Enter || pick.is_some() {
                break;
            }
        }
        self.set_slot(fluxbbs_term::Slot::Info);
        self.newline();
        Ok(yes)
    }
}

/// Take cursor position reports (`ESC [ row ; col R`) out of what the
/// caller sent: the keys, how many reports there were, and (if `hold`) an
/// unfinished report at the end, kept back to join the next read.
/// Keys from a UTF-8 terminal as the CP437 bytes the board works in:
/// what CP437 can't hold (or holds only as a control glyph, `☺`) becomes
/// `?`. A character split between reads waits in `tail`.
fn utf8_keys(data: &[u8], tail: &mut Vec<u8>) -> Vec<u8> {
    let mut buf = std::mem::take(tail);
    buf.extend_from_slice(data);
    let mut out = Vec::with_capacity(buf.len());
    let mut i = 0;
    while i < buf.len() {
        let len = match buf[i] {
            b @ 0..=0x7F => {
                out.push(b);
                i += 1;
                continue;
            }
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => {
                i += 1;
                continue;
            }
        };
        if i + len > buf.len() {
            *tail = buf[i..].to_vec();
            break;
        }
        if let Ok(s) = std::str::from_utf8(&buf[i..i + len]) {
            out.extend(fluxbbs_term::cp437::encode(s).into_iter().map(|c| if c < 0x20 || c == 0x7F { b'?' } else { c }));
        }
        i += len;
    }
    out
}

/// A typed key as a character (CP437), if it's one that goes in text:
/// 32 to 254 but DEL, as the original's `chNormal`.
pub fn text_char(c: u8) -> Option<char> {
    (c >= 0x20 && c != 0x7F && c != 0xFF).then(|| fluxbbs_term::cp437::TO_UNICODE[c as usize])
}

/// Cursor reports (`ESC[24;80R`), device attributes (`ESC[=...c`,
/// `ESC[?...c`, `ESC[>...c`, `ESC[<...c`) and status strings (`ESC P 1
/// $ r ... ESC \`) out of what the caller sent: the keys, the reports
/// counted, an unfinished one held back, and the answers.
fn strip_cursor_reports(buf: &[u8], hold: bool) -> (Vec<u8>, u32, Vec<u8>, Vec<Vec<u8>>) {
    fn lead(b: &[u8]) -> &[u8] {
        match b.split_first() {
            Some((b'=' | b'?' | b'>' | b'<', rest)) => rest,
            _ => b,
        }
    }
    // A status string not ended yet, or `ESC`, `ESC [` or `ESC [ 24;8`
    // at the very end: the start of one.
    let dcs_open = (0..buf.len()).rev().find(|&p| status_string(&buf[p..]).is_some()).filter(|&p| status_string(&buf[p..]) == Some(None));
    let tail_at = dcs_open.filter(|_| hold).or_else(|| buf.iter().rposition(|&b| b == 0x1B).filter(|&p| {
        hold && match buf.get(p + 1) {
            None => true,
            Some(b'[') => lead(&buf[p + 2..]).iter().all(|b| b.is_ascii_digit() || *b == b';'),
            Some(_) => false,
        }
    }));
    let (buf, tail) = match tail_at {
        Some(p) => (&buf[..p], buf[p..].to_vec()),
        None => (buf, Vec::new()),
    };
    let mut keys = Vec::with_capacity(buf.len());
    let mut reports = 0;
    let mut da = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == 0x1B && buf.get(i + 1) == Some(&b'[') {
            let lead = matches!(buf.get(i + 2), Some(b'=' | b'?' | b'>' | b'<')) as usize;
            let body = buf[i + 2 + lead..].iter().take_while(|b| b.is_ascii_digit() || **b == b';').count();
            let end = i + 2 + lead + body;
            if lead == 0 && body > 0 && buf.get(end) == Some(&b'R') && buf[i + 2..end].contains(&b';') {
                reports += 1;
                i = end + 1;
                continue;
            }
            if lead == 1 && buf.get(end) == Some(&b'c') {
                da.push(buf[i + 2..end].to_vec());
                i = end + 1;
                continue;
            }
        }
        if let Some(Some(len)) = status_string(&buf[i..]) {
            da.push(buf[i + 2..i + len - 2].to_vec());
            i += len;
            continue;
        }
        keys.push(buf[i]);
        i += 1;
    }
    (keys, reports, tail, da)
}

/// A DECRQSS answer (`ESC P 0|1 $ r ... ESC \`) at the start of `b`:
/// `Some(Some(length))`, `Some(None)` if it hasn't ended yet, `None` if
/// it isn't one.
fn status_string(b: &[u8]) -> Option<Option<usize>> {
    let head = [0x1B, b'P'];
    if b.len() < 2 || b[..2] != head {
        return None;
    }
    let digits = b[2..].iter().take_while(|c| c.is_ascii_digit()).count();
    let rest = &b[2 + digits..];
    let n = rest.len().min(2);
    if rest[..n] != b"$r"[..n] || (digits == 0 && !rest.is_empty()) {
        return None;
    }
    Some(rest.windows(2).position(|w| w == b"\x1b\\").map(|p| 2 + digits + p + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(case: Case, s: &str) -> String {
        let mut out = String::new();
        for c in s.chars() {
            let t = case.apply(&out, c);
            out.push(t);
        }
        out
    }

    #[test]
    fn utf8_keys_become_cp437() {
        let mut tail = Vec::new();
        // "café ☺ ░" split inside the é; ESC sequences pass untouched.
        let mut out = utf8_keys(b"caf\xc3", &mut tail);
        out.extend(utf8_keys(b"\xa9 \xe2\x98\xba \xe2\x96\x91\x1b[A", &mut tail));
        assert_eq!(out, b"caf\x82 ? \xb0\x1b[A");
        assert!(tail.is_empty());
        assert_eq!(text_char(0x82), Some('\u{e9}'));
        assert_eq!((text_char(0x7f), text_char(0x1b)), (None, None));
    }

    #[test]
    fn tabs_go_to_the_next_stop() {
        assert_eq!((tab_spaces(0), tab_spaces(1), tab_spaces(3), tab_spaces(4), tab_spaces(6)), (4, 3, 1, 4, 2));
    }

    #[test]
    fn cases() {
        assert_eq!(typed(Case::Capital, "fiend"), "Fiend");
        assert_eq!(typed(Case::Mixed, "mIKE fricker"), "Mike Fricker");
        assert_eq!(typed(Case::Weird, "mike fricker"), "MiKe FRiCKeR");
        assert_eq!(typed(Case::Warped, "mike fricker"), "mIkE frIckEr");
        assert_eq!(typed(Case::Cool, "fiend"), "FiEND");
        assert_eq!(Case::from_alias_format(2), Case::Capital);
    }

    #[test]
    fn cursor_reports() {
        assert_eq!(strip_cursor_reports(b"a\x1b[24;80Rb", false), (b"ab".to_vec(), 1, Vec::new(), Vec::new()));
        assert_eq!(strip_cursor_reports(b"\x1b[1;1R\x1b[2;5R", false), (Vec::new(), 2, Vec::new(), Vec::new()));
        // Split between reads while one is expected: held, then joined.
        let (keys, n, tail, _) = strip_cursor_reports(b"x\x1b[24;", true);
        assert_eq!((keys, n, tail.clone()), (b"x".to_vec(), 0, b"\x1b[24;".to_vec()));
        let mut next = tail;
        next.extend_from_slice(b"80Ry");
        assert_eq!(strip_cursor_reports(&next, true), (b"y".to_vec(), 1, Vec::new(), Vec::new()));
        assert_eq!(strip_cursor_reports(b"\x1b", true).2, b"\x1b".to_vec());
        // Not expecting one: an Esc at the end is a key, now.
        assert_eq!(strip_cursor_reports(b"\x1b", false), (b"\x1b".to_vec(), 0, Vec::new(), Vec::new()));
        // A finished arrow key isn't held.
        assert_eq!(strip_cursor_reports(b"\x1b[A", true), (b"\x1b[A".to_vec(), 0, Vec::new(), Vec::new()));
        // Keys that look a little like one stay keys.
        assert_eq!(strip_cursor_reports(b"\x1b[A\x1b[5~\x1b[R", false), (b"\x1b[A\x1b[5~\x1b[R".to_vec(), 0, Vec::new(), Vec::new()));
        // Device attributes (IcyTerm's) come out too, kept for the probe.
        let (keys, n, tail, da) = strip_cursor_reports(b"\x1b[=73;99;121;84;101;114;109;4;0;0c\x1b[25;80Rx\x1b[<0;1;6c", true);
        assert_eq!((keys, n, tail), (b"x".to_vec(), 1, Vec::new()));
        assert_eq!(da, vec![b"=73;99;121;84;101;114;109;4;0;0".to_vec(), b"<0;1;6".to_vec()]);
        assert_eq!(strip_cursor_reports(b"\x1b[?1;2", true).2, b"\x1b[?1;2".to_vec());
        // A status string (DECRQSS), whole or split between reads.
        let (keys, n, tail, da) = strip_cursor_reports(b"a\x1bP1$r0;38;2;1;2;3m\x1b\\\x1b[1;1Rb", true);
        assert_eq!((keys, n, tail), (b"ab".to_vec(), 1, Vec::new()));
        assert_eq!(da, vec![b"1$r0;38;2;1;2;3m".to_vec()]);
        let (keys, _, tail, da) = strip_cursor_reports(b"a\x1bP1$r0;38;2", true);
        assert_eq!((keys, tail, da.len()), (b"a".to_vec(), b"\x1bP1$r0;38;2".to_vec(), 0));
        // Alt-P is a key, not the start of one.
        assert_eq!(strip_cursor_reports(b"\x1bPx", true).0, b"\x1bPx".to_vec());
    }
}

