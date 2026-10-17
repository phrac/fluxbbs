//! Output side of a caller's terminal.
//!
//! Every byte the BBS sends goes through [`Screen`], which keeps a model of
//! the cursor and current colour (what Iniquity's local console emulation
//! did) and encodes for the caller's terminal:
//!
//! * [`Charset::Cp437`]: raw bytes, for BBS terminals (SyncTERM, NetRunner).
//! * [`Charset::Utf8`]: CP437 translated to Unicode, with DOS behaviour
//!   that modern terminals lack emulated: hard wrap at column 80 and
//!   `ESC[2J` homing the cursor.

use crate::color::Color;
use crate::cp437;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charset {
    Cp437,
    Utf8,
}

/// Width every Iniquity screen is drawn for.
pub const COLUMNS: u16 = 80;

#[derive(Debug)]
pub struct Screen {
    charset: Charset,
    rows: u16,
    out: Vec<u8>,
    /// 1-based cursor position.
    x: u16,
    y: u16,
    color: Color,
    saved_cursor: (u16, u16),
    esc: Option<Vec<u8>>,
}

impl Screen {
    pub fn new(charset: Charset, rows: u16) -> Self {
        Self {
            charset,
            rows: rows.max(1),
            out: Vec::new(),
            x: 1,
            y: 1,
            color: Color::DEFAULT,
            saved_cursor: (1, 1),
            esc: None,
        }
    }

    pub fn charset(&self) -> Charset {
        self.charset
    }

    pub fn set_rows(&mut self, rows: u16) {
        self.rows = rows.max(1);
        self.y = self.y.min(self.rows);
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn where_x(&self) -> u16 {
        self.x
    }

    pub fn where_y(&self) -> u16 {
        self.y
    }

    pub fn color(&self) -> Color {
        self.color
    }

    /// Bytes ready to send; clears the buffer.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    /// Write CP437 bytes, which may contain ANSI escape sequences.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.put(b);
        }
    }

    /// Write a Rust string; characters outside CP437 become `?`.
    pub fn write_str(&mut self, s: &str) {
        self.write_bytes(&cp437::encode(s));
    }

    pub fn newline(&mut self) {
        self.write_bytes(b"\r\n");
    }

    pub fn set_color(&mut self, c: Color) {
        let seq = self.color.transition(c);
        self.raw(seq.as_bytes());
        self.color = c;
    }

    pub fn clear_screen(&mut self) {
        self.write_bytes(b"\x1b[2J");
    }

    pub fn clear_eol(&mut self) {
        self.raw(b"\x1b[K");
    }

    pub fn goto(&mut self, x: u16, y: u16) {
        let x = x.clamp(1, COLUMNS);
        let y = y.clamp(1, self.rows);
        self.raw(format!("\x1b[{y};{x}H").as_bytes());
        self.x = x;
        self.y = y;
    }

    // Relative moves, so a slightly wrong row/column model can't throw the
    // cursor somewhere absolute.
    pub fn move_up(&mut self, n: u16) {
        self.write_bytes(format!("\x1b[{n}A").as_bytes());
    }

    pub fn move_down(&mut self, n: u16) {
        self.write_bytes(format!("\x1b[{n}B").as_bytes());
    }

    pub fn move_left(&mut self, n: u16) {
        self.write_bytes(format!("\x1b[{n}D").as_bytes());
    }

    pub fn move_right(&mut self, n: u16) {
        self.write_bytes(format!("\x1b[{n}C").as_bytes());
    }

    /// `oPosX`: move to column `x` on the current row.
    pub fn pos_x(&mut self, x: u16) {
        let x = x.clamp(1, COLUMNS);
        match x.cmp(&self.x) {
            std::cmp::Ordering::Greater => self.move_right(x - self.x),
            std::cmp::Ordering::Less => self.move_left(self.x - x),
            std::cmp::Ordering::Equal => {}
        }
    }

    /// `oPosY`: move to row `y` in the current column.
    pub fn pos_y(&mut self, y: u16) {
        let y = y.clamp(1, self.rows);
        match y.cmp(&self.y) {
            std::cmp::Ordering::Greater => self.move_down(y - self.y),
            std::cmp::Ordering::Less => self.move_up(self.y - y),
            std::cmp::Ordering::Equal => {}
        }
    }

    pub fn save_cursor(&mut self) {
        self.saved_cursor = (self.x, self.y);
        self.raw(b"\x1b[s");
    }

    pub fn restore_cursor(&mut self) {
        let (x, y) = self.saved_cursor;
        self.goto(x, y);
    }

    /// Erase `n` characters to the left of the cursor (`oBack`).
    pub fn backspace(&mut self, n: u16) {
        for _ in 0..n.min(self.x - 1) {
            self.raw(b"\x08 \x08");
            self.x -= 1;
        }
    }

    /// Append bytes with no cursor tracking.
    fn raw(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
    }

    fn put(&mut self, b: u8) {
        if let Some(seq) = &mut self.esc {
            seq.push(b);
            let done = match seq.len() {
                1 => b != b'[',
                _ => (0x40..=0x7E).contains(&b),
            };
            if done {
                let seq = self.esc.take().unwrap();
                self.escape(&seq);
            }
            return;
        }
        match b {
            0x1B => self.esc = Some(Vec::new()),
            b'\r' => {
                self.x = 1;
                self.raw(b"\r");
            }
            b'\n' => {
                self.line_feed();
                self.raw(b"\n");
            }
            0x08 => {
                self.x = self.x.saturating_sub(1).max(1);
                self.raw(b"\x08");
            }
            0x07 => self.raw(b"\x07"),
            0x09 => {
                let next = ((self.x - 1) / 8 + 1) * 8 + 1;
                self.x = next.min(COLUMNS);
                self.raw(b"\t");
            }
            0x00 => {}
            _ => self.glyph(b),
        }
    }

    fn line_feed(&mut self) {
        if self.y < self.rows {
            self.y += 1;
        }
    }

    fn glyph(&mut self, b: u8) {
        match self.charset {
            Charset::Cp437 => self.out.push(b),
            Charset::Utf8 => {
                let mut buf = [0; 4];
                let s = cp437::TO_UNICODE[b as usize].encode_utf8(&mut buf);
                self.out.extend_from_slice(s.as_bytes());
            }
        }
        self.x += 1;
        if self.x > COLUMNS {
            // DOS wraps immediately. A Unicode terminal may be wider than
            // 80 columns or defer the wrap, so make it explicit.
            self.x = 1;
            self.line_feed();
            if self.charset == Charset::Utf8 {
                self.raw(b"\r\n");
            }
        }
    }

    /// Track an escape sequence (`seq` excludes the ESC) and pass it on.
    fn escape(&mut self, seq: &[u8]) {
        self.raw(b"\x1b");
        self.raw(seq);
        let [b'[', body @ .., fin] = seq else { return };
        let params: Vec<u16> = if body.is_empty() {
            Vec::new()
        } else {
            body.split(|&c| c == b';')
                .map(|p| std::str::from_utf8(p).ok().and_then(|s| s.parse().ok()).unwrap_or(0))
                .collect()
        };
        let n = |i: usize| params.get(i).copied().filter(|&v| v > 0).unwrap_or(1);
        match fin {
            b'A' => self.y = self.y.saturating_sub(n(0)).max(1),
            b'B' => self.y = (self.y + n(0)).min(self.rows),
            b'C' => self.x = (self.x + n(0)).min(COLUMNS),
            b'D' => self.x = self.x.saturating_sub(n(0)).max(1),
            b'H' | b'f' => {
                self.y = n(0).min(self.rows);
                self.x = n(1).min(COLUMNS);
            }
            b'G' => self.x = n(0).min(COLUMNS),
            b'd' => self.y = n(0).min(self.rows),
            b'J' if params.first() == Some(&2) => {
                // ANSI.SYS homes the cursor on clear; VT terminals don't.
                self.x = 1;
                self.y = 1;
                if self.charset == Charset::Utf8 {
                    self.raw(b"\x1b[H");
                }
            }
            b's' => self.saved_cursor = (self.x, self.y),
            b'u' => (self.x, self.y) = self.saved_cursor,
            b'm' => self.color.apply_sgr(&params),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp437_passthrough() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(&[0xC9, 0xCD]);
        assert_eq!(s.take_output(), vec![0xC9, 0xCD]);
        assert_eq!(s.where_x(), 3);
    }

    #[test]
    fn utf8_translation() {
        let mut s = Screen::new(Charset::Utf8, 25);
        s.write_bytes(&[0xB0, 0x10]);
        assert_eq!(String::from_utf8(s.take_output()).unwrap(), "░►");
    }

    #[test]
    fn hard_wrap_at_80_in_utf8() {
        let mut s = Screen::new(Charset::Utf8, 25);
        s.write_bytes(&[b'x'; 81]);
        let out = String::from_utf8(s.take_output()).unwrap();
        assert_eq!(out, format!("{}\r\nx", "x".repeat(80)));
        assert_eq!((s.where_x(), s.where_y()), (2, 2));
    }

    #[test]
    fn tracks_ansi_cursor_and_color() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[5;10H\x1b[1;32mab\x1b[2C");
        assert_eq!((s.where_x(), s.where_y()), (14, 5));
        assert_eq!(s.color(), Color::new(10, 0));
        s.write_bytes(b"\x1b[2J");
        assert_eq!((s.where_x(), s.where_y()), (1, 1));
    }

    #[test]
    fn clear_homes_in_utf8() {
        let mut s = Screen::new(Charset::Utf8, 25);
        s.clear_screen();
        assert_eq!(s.take_output(), b"\x1b[2J\x1b[H");
    }

    #[test]
    fn set_color_only_emits_changes() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.set_color(Color::new(11, 0));
        s.set_color(Color::new(11, 0));
        assert_eq!(s.take_output(), b"\x1b[1;36m");
    }

    #[test]
    fn bottom_row_scrolls() {
        let mut s = Screen::new(Charset::Cp437, 3);
        s.write_bytes(b"\r\n\r\n\r\n\r\n");
        assert_eq!(s.where_y(), 3);
    }
}
