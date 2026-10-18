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

    pub fn prefill(mut self, s: impl Into<String>) -> Self {
        self.default = s.into();
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
            self.input.extend(&buf[..n]);
        }
    }

    /// Next key from the caller. Enter is a single `\r` whatever the
    /// client sends; VT/ANSI cursor-key sequences are decoded.
    pub async fn read_key(&mut self) -> io::Result<Key> {
        let b = self.next_byte(Wait::Idle).await?.expect("idle wait returns a byte");
        Ok(match b {
            b'\r' | b'\n' => Key::Enter,
            0x08 | 0x7F => Key::Backspace,
            b'\t' => Key::Tab,
            0x1B => self.escape_sequence().await?,
            c => Key::Char(c),
        })
    }

    async fn escape_sequence(&mut self) -> io::Result<Key> {
        let Some(intro) = self.next_byte(Wait::Brief).await? else { return Ok(Key::Esc) };
        if intro != b'[' && intro != b'O' {
            self.input.push_front(intro);
            return Ok(Key::Esc);
        }
        let mut params = Vec::new();
        loop {
            let Some(b) = self.next_byte(Wait::Brief).await? else { return Ok(Key::Esc) };
            if (0x40..=0x7E).contains(&b) {
                return Ok(match (b, params.as_slice()) {
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
                    _ => Key::Esc,
                });
            }
            params.push(b);
        }
    }

    /// `iReadString`: read a line with editing. Backspace deletes,
    /// Ctrl-U/Ctrl-X clear, Enter finishes.
    pub async fn read_string(&mut self, opts: ReadOpts) -> io::Result<String> {
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
                    Key::Char(c @ 0x20..=0x7E) if s.chars().count() < opts.max => {
                        let c = opts.case.apply(&s, c as char);
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
    fn cases() {
        assert_eq!(typed(Case::Capital, "fiend"), "Fiend");
        assert_eq!(typed(Case::Mixed, "mIKE fricker"), "Mike Fricker");
        assert_eq!(typed(Case::Weird, "mike fricker"), "MiKe FRiCKeR");
        assert_eq!(typed(Case::Warped, "mike fricker"), "mIkE frIckEr");
        assert_eq!(typed(Case::Cool, "fiend"), "FiEND");
        assert_eq!(Case::from_alias_format(2), Case::Capital);
    }
}
