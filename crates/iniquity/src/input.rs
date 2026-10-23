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

    pub fn mask(mut self, mask: &'static str) -> Self {
        self.mask = mask;
        self
    }
}

pub const DIGITS: &str = "0123456789";

enum Wait {
    /// Block, subject to the inactivity timeout.
    Idle,
    /// Give up after a short time (escape sequence continuation).
    Brief,
}

impl Session {
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
                    let cfg = self.state.config();
                    // Shorter before logging in, so idle connections from
                    // bots don't hold nodes.
                    let idle = if self.logged_in { cfg.bbs.inactivity_seconds } else { cfg.security.prelogin_idle_seconds };
                    let deadline = (idle > 0).then(|| tokio::time::Instant::now() + Duration::from_secs(idle));
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
                        if !self.hold_node_msgs {
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
            let (keys, reports, tail) = strip_cursor_reports(&data, expecting);
            self.cursor_reports += reports;
            let keys = if self.screen.charset() == iniquity_term::Charset::Utf8 { utf8_keys(&keys, &mut self.utf8_tail) } else { keys };
            self.input.extend(keys);
            self.report_tail = tail;
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
        loop {
            let mut s = String::new();
            for c in opts.default.chars().take(opts.max) {
                s.push(c);
                self.screen.write_str(&c.to_string());
            }
            loop {
                match self.read_key().await? {
                    Key::Enter => break,
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
                    Key::Char(c) if s.chars().count() < opts.max && text_char(c).is_some() => {
                        let c = opts.case.apply(&s, text_char(c).unwrap_or(' '));
                        if !opts.allowed.is_empty() && !opts.allowed.contains(c) {
                            continue;
                        }
                        if c == ' ' && s.is_empty() {
                            continue; // no leading spaces
                        }
                        s.push(c);
                        let shown = if opts.password { echo } else { c };
                        self.screen.write_str(&shown.to_string());
                    }
                    _ => {}
                }
            }
            let s = s.trim_end().to_string();
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
        let (ink, dim) = (self.screen.color(), iniquity_term::Color::new(8, 0));
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

    /// `iYesNo`: a yes/no question, as a lightbar if the user prefers.
    pub async fn yes_no(&mut self, default: bool) -> io::Result<bool> {
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

    async fn yes_no_bar(&mut self, default: bool) -> io::Result<bool> {
        let (yes_bar, no_bar) = (self.state.config().prompts.yes_bar.clone(), self.state.config().prompts.no_bar.clone());
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
        self.set_slot(iniquity_term::Slot::Info);
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
            out.extend(iniquity_term::cp437::encode(s).into_iter().map(|c| if c < 0x20 || c == 0x7F { b'?' } else { c }));
        }
        i += len;
    }
    out
}

/// A typed key as a character (CP437), if it's one that goes in text:
/// 32 to 254 but DEL, as the original's `chNormal`.
pub fn text_char(c: u8) -> Option<char> {
    (c >= 0x20 && c != 0x7F && c != 0xFF).then(|| iniquity_term::cp437::TO_UNICODE[c as usize])
}

fn strip_cursor_reports(buf: &[u8], hold: bool) -> (Vec<u8>, u32, Vec<u8>) {
    // `ESC`, `ESC [` or `ESC [ 24;8` at the very end: the start of one.
    let tail_at = buf.iter().rposition(|&b| b == 0x1B).filter(|&p| {
        hold && match buf.get(p + 1) {
            None => true,
            Some(b'[') => buf[p + 2..].iter().all(|b| b.is_ascii_digit() || *b == b';'),
            Some(_) => false,
        }
    });
    let (buf, tail) = match tail_at {
        Some(p) => (&buf[..p], buf[p..].to_vec()),
        None => (buf, Vec::new()),
    };
    let mut keys = Vec::with_capacity(buf.len());
    let mut reports = 0;
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == 0x1B && buf.get(i + 1) == Some(&b'[') {
            let body = buf[i + 2..].iter().take_while(|b| b.is_ascii_digit() || **b == b';').count();
            let end = i + 2 + body;
            if body > 0 && buf.get(end) == Some(&b'R') && buf[i + 2..end].contains(&b';') {
                reports += 1;
                i = end + 1;
                continue;
            }
        }
        keys.push(buf[i]);
        i += 1;
    }
    (keys, reports, tail)
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
        assert_eq!(strip_cursor_reports(b"a\x1b[24;80Rb", false), (b"ab".to_vec(), 1, Vec::new()));
        assert_eq!(strip_cursor_reports(b"\x1b[1;1R\x1b[2;5R", false), (Vec::new(), 2, Vec::new()));
        // Split between reads while one is expected: held, then joined.
        let (keys, n, tail) = strip_cursor_reports(b"x\x1b[24;", true);
        assert_eq!((keys, n, tail.clone()), (b"x".to_vec(), 0, b"\x1b[24;".to_vec()));
        let mut next = tail;
        next.extend_from_slice(b"80Ry");
        assert_eq!(strip_cursor_reports(&next, true), (b"y".to_vec(), 1, Vec::new()));
        assert_eq!(strip_cursor_reports(b"\x1b", true).2, b"\x1b".to_vec());
        // Not expecting one: an Esc at the end is a key, now.
        assert_eq!(strip_cursor_reports(b"\x1b", false), (b"\x1b".to_vec(), 0, Vec::new()));
        // A finished arrow key isn't held.
        assert_eq!(strip_cursor_reports(b"\x1b[A", true), (b"\x1b[A".to_vec(), 0, Vec::new()));
        // Keys that look a little like one stay keys.
        assert_eq!(strip_cursor_reports(b"\x1b[A\x1b[5~\x1b[R", false), (b"\x1b[A\x1b[5~\x1b[R".to_vec(), 0, Vec::new()));
    }
}

