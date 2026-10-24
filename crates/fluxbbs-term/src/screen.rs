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
    /// The bottom of a scroll region, if one is set.
    region: Option<u16>,
    out: Vec<u8>,
    /// 1-based cursor position.
    x: u16,
    y: u16,
    color: Color,
    saved_cursor: (u16, u16),
    esc: Option<Vec<u8>>,
    /// An aixterm bright background (SGR 100-107) is showing.
    bright_bg: bool,
    /// The foreground was set with an aixterm bright code (90-97), so a
    /// plain 30-37 after it means normal intensity, not "keep bold".
    aix_fg: bool,
    /// A 256-colour or 24-bit colour is showing; `color` only approximates it.
    ext_color: bool,
    /// The terminal takes 24-bit colour: bright backgrounds go out as
    /// that rather than the iCE blink bit.
    truecolor: bool,
    /// The text's own blink attribute (`5` until `25` or `0`), which DOS
    /// art means as a bright background.
    ice_blink: bool,
    /// New: plain text (for bots): no colors, cursor moves or clears go
    /// out, and a clear screen starts a new line.
    plain: bool,
    /// New: lines with text on them written so far (for the menu's
    /// pause: only after a command that showed something), and whether
    /// the current one has any yet.
    text_lines: u64,
    dirty: bool,
}

impl Screen {
    pub fn new(charset: Charset, rows: u16) -> Self {
        Self {
            charset,
            rows: rows.max(1),
            region: None,
            out: Vec::new(),
            x: 1,
            y: 1,
            color: Color::DEFAULT,
            saved_cursor: (1, 1),
            esc: None,
            bright_bg: false,
            aix_fg: false,
            ext_color: false,
            truecolor: false,
            ice_blink: false,
            plain: false,
            text_lines: 0,
            dirty: false,
        }
        .init()
    }

    fn init(mut self) -> Self {
        if self.charset == Charset::Cp437 {
            // iCE colours: the blink bit means a bright background
            // (SyncTERM), so art with bright backgrounds shows right.
            self.raw(b"\x1b[?33h");
        }
        self
    }

    pub fn charset(&self) -> Charset {
        self.charset
    }

    pub fn set_truecolor(&mut self, on: bool) {
        self.truecolor = on;
    }

    /// A CP437 terminal known to take 24-bit colour.
    pub fn truecolor(&self) -> bool {
        self.truecolor
    }

    /// Switch character sets mid-call (the caller says detection got it
    /// wrong).
    pub fn set_charset(&mut self, charset: Charset) {
        self.charset = charset;
        if charset == Charset::Cp437 {
            self.raw(b"\x1b[?33h");
        }
    }

    pub fn set_plain(&mut self, on: bool) {
        self.plain = on;
    }

    pub fn plain(&self) -> bool {
        self.plain
    }

    /// Lines with text on them written so far.
    pub fn text_lines(&self) -> u64 {
        self.text_lines
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

    /// Forget what we think the terminal looks like after something else
    /// (a file transfer) wrote to it: reset its colour and our model.
    pub fn resync(&mut self) {
        self.raw(b"\x1b[0m");
        self.color = Color::DEFAULT;
        self.esc = None;
    }

    /// Bytes ready to send; clears the buffer.
    pub fn take_output(&mut self) -> Vec<u8> {
        let out = std::mem::take(&mut self.out);
        if self.plain { strip_escapes(&out) } else { out }
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

    /// Text that mustn't drive the terminal: no control characters, nor
    /// CP437's glyphs for them, which encode to the control bytes (`•`
    /// becomes `∙`, as it looks the same).
    pub fn write_text(&mut self, s: &str) {
        let mut b = cp437::encode(&s.replace('\u{2022}', "\u{2219}"));
        b.retain(|&c| c >= 0x20 && c != 0x7F);
        self.write_bytes(&b);
    }

    pub fn newline(&mut self) {
        self.write_bytes(b"\r\n");
    }

    pub fn set_color(&mut self, c: Color) {
        if self.bright_bg || self.ext_color {
            // Our model can't express these colours exactly; start over.
            self.raw(b"\x1b[0m");
            self.color = Color::DEFAULT;
            self.bright_bg = false;
            self.ext_color = false;
        }
        if c.blink && self.truecolor && self.charset == Charset::Cp437 {
            // A bright background, as `sgr` sends it to these terminals.
            let d = [0, 4, 2, 6, 1, 5, 3, 7];
            let [r, g, b] = VGA[(c.back & 7 | 8) as usize];
            let seq = format!("\x1b[0;{}3{};48;2;{r};{g};{b}m", if c.fore >= 8 { "1;" } else { "" }, d[(c.fore & 7) as usize]);
            self.raw(seq.as_bytes());
            self.color = c;
            self.bright_bg = true;
            return;
        }
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
                if std::mem::take(&mut self.dirty) {
                    self.text_lines += 1;
                }
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
        // At the bottom of a scroll region the region scrolls instead.
        if self.y < self.rows && Some(self.y) != self.region {
            self.y += 1;
        }
    }

    /// Scroll only rows `top` to `bottom`, leaving those above and below
    /// fixed (a header, a status line). The cursor goes home, as terminals
    /// put it.
    pub fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        let bottom = bottom.clamp(1, self.rows);
        let top = top.clamp(1, bottom);
        self.raw(format!("\x1b[{top};{bottom}r").as_bytes());
        self.region = Some(bottom);
        self.x = 1;
        self.y = 1;
    }

    /// The whole screen scrolls again.
    pub fn reset_scroll_region(&mut self) {
        if self.region.take().is_some() {
            self.raw(b"\x1b[r");
            self.x = 1;
            self.y = 1;
        }
    }

    fn glyph(&mut self, b: u8) {
        if b != b' ' {
            self.dirty = true;
        }
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
        if let [b'[', body @ .., b'm'] = seq {
            return self.sgr(body);
        }
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
            b'B' => self.y = self.y.saturating_add(n(0)).min(self.rows),
            b'C' => self.x = self.x.saturating_add(n(0)).min(COLUMNS),
            b'D' => self.x = self.x.saturating_sub(n(0)).max(1),
            b'H' | b'f' => {
                self.y = n(0).min(self.rows);
                self.x = n(1).min(COLUMNS);
            }
            b'G' => self.x = n(0).min(COLUMNS),
            b'd' => self.y = n(0).min(self.rows),
            b'J' if params.first() == Some(&2) => {
                self.dirty = false;
                // Plain text has no screens: a new one starts a new line.
                if self.plain && self.x > 1 {
                    self.raw(b"\r\n");
                }
                // ANSI.SYS homes the cursor on clear; VT terminals don't.
                self.x = 1;
                self.y = 1;
                if self.charset == Charset::Utf8 {
                    self.raw(b"\x1b[H");
                }
            }
            b's' => self.saved_cursor = (self.x, self.y),
            b'm' => unreachable!("handled by sgr"),
            b'u' => (self.x, self.y) = self.saved_cursor,
            _ => {}
        }
    }
}

impl Screen {
    /// A colour change. Classic codes pass through; aixterm bright
    /// colours (90-97, 100-107) pass through to Unicode terminals, which
    /// understand them, but become DOS codes for CP437 terminals: bold for
    /// a bright foreground, the iCE blink bit for a bright background (or
    /// the 24-bit colour, where the terminal takes it, since a terminal
    /// that misses the iCE switch shows blinking instead).
    fn sgr(&mut self, body: &[u8]) {
        let params: Vec<u16> = if body.is_empty() {
            vec![0]
        } else {
            body.split(|&c| c == b';')
                .map(|p| std::str::from_utf8(p).ok().and_then(|s| s.parse().ok()).unwrap_or(0))
                .collect()
        };
        if params.iter().any(|&p| p == 38 || p == 48) {
            // xterm 256-colour / 24-bit colour (image converters and modern
            // editors). SyncTERM understands these too, so they go out as
            // they are and the tracker keeps the nearest DOS colour.
            let mut k = 0;
            while k < params.len() {
                let p = params[k];
                k += 1;
                match p {
                    0 => {
                        self.color = Color::DEFAULT;
                        self.bright_bg = false;
                        self.aix_fg = false;
                        self.ext_color = false;
                    }
                    38 | 48 => {
                        let rgb = match params.get(k) {
                            Some(5) => params.get(k + 1).map(|&n| xterm256(n)),
                            Some(2) if k + 3 < params.len() => {
                                Some([params[k + 1], params[k + 2], params[k + 3]].map(|v| v.min(255) as u8))
                            }
                            _ => None,
                        };
                        k += if params.get(k) == Some(&2) { 4 } else { 2 };
                        if let Some(rgb) = rgb {
                            self.ext_color = true;
                            if p == 38 {
                                self.color.fore = nearest_dos(rgb, 16);
                            } else {
                                self.color.back = nearest_dos(rgb, 8);
                            }
                        }
                    }
                    _ => self.color.apply_sgr(&[p]),
                }
            }
            self.raw(b"\x1b[");
            self.raw(body);
            self.raw(b"m");
            return;
        }
        if params.contains(&0) {
            self.ext_color = false;
        }
        let aixterm = params.iter().any(|p| matches!(p, 90..=97 | 100..=107));
        let was_bright = self.bright_bg;
        let was_aix_fg = self.aix_fg;
        let was_blink = self.ice_blink;
        for &p in &params {
            match p {
                0 => (self.bright_bg, self.ice_blink) = (false, false),
                5 | 6 => self.ice_blink = true,
                25 => self.ice_blink = false,
                40..=47 | 49 => self.bright_bg = false,
                100..=107 => {
                    self.bright_bg = true;
                    self.color.back = [0, 4, 2, 6, 1, 5, 3, 7][(p - 100) as usize];
                }
                _ => {}
            }
        }
        self.color.apply_sgr(&params);
        let plain_fg = params.iter().any(|p| (30..=37).contains(p));
        if plain_fg && was_aix_fg && !params.contains(&1) {
            self.color.fore &= 7;
        }
        if params.iter().any(|p| (90..=97).contains(p)) {
            self.aix_fg = true;
        } else if plain_fg || params.contains(&0) {
            self.aix_fg = false;
        }
        // DOS art means blink as a bright background (iCE colours); a
        // terminal getting 24-bit colour gets it that way too.
        let ice = self.ice_blink;
        let ice_rgb = self.truecolor && (ice || was_blink);
        if self.charset == Charset::Cp437 && (aixterm || self.bright_bg || was_bright || was_aix_fg || ice_rgb) {
            self.color.blink = self.bright_bg || ice;
            let c = self.color;
            let d = [0, 4, 2, 6, 1, 5, 3, 7];
            let rgb = c.blink && self.truecolor;
            let back = if rgb {
                let [r, g, b] = VGA[(c.back & 7 | 8) as usize];
                format!("48;2;{r};{g};{b}")
            } else {
                format!("4{}", d[(c.back & 7) as usize])
            };
            let seq = format!(
                "\x1b[0;{}{}3{};{back}m",
                if c.fore >= 8 { "1;" } else { "" },
                if c.blink && !rgb { "5;" } else { "" },
                d[(c.fore & 7) as usize]
            );
            self.raw(seq.as_bytes());
        } else {
            self.raw(b"\x1b[");
            self.raw(body);
            self.raw(b"m");
        }
    }
}

/// The VGA text palette in DOS order.
const VGA: [[u8; 3]; 16] = [
    [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170],
    [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170],
    [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255],
    [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255],
];

/// RGB of an xterm 256-colour palette entry.
/// Output without escape sequences or bells, for plain text.
fn strip_escapes(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            0x1B if b.get(i + 1) == Some(&b'[') => {
                i += 2;
                while i < b.len() && !(0x40..=0x7E).contains(&b[i]) {
                    i += 1;
                }
                i += 1;
            }
            0x1B => i += 2,
            0x07 => i += 1,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn xterm256(n: u16) -> [u8; 3] {
    match n {
        0..=15 => VGA[([0, 4, 2, 6, 1, 5, 3, 7][(n & 7) as usize] | (n & 8)) as usize],
        16..=231 => {
            let n = n - 16;
            let level = |c: u16| if c == 0 { 0 } else { (55 + 40 * c) as u8 };
            [level(n / 36), level(n / 6 % 6), level(n % 6)]
        }
        _ => {
            let v = (8 + 10 * (n.min(255) - 232)) as u8;
            [v, v, v]
        }
    }
}

/// The closest of the first `count` DOS colours to `rgb`.
fn nearest_dos(rgb: [u8; 3], count: usize) -> u8 {
    let dist = |c: &[u8; 3]| -> i32 { (0..3).map(|i| (c[i] as i32 - rgb[i] as i32).pow(2)).sum() };
    (0..count).min_by_key(|&i| dist(&VGA[i])).unwrap_or(7) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp437_passthrough() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.take_output();
        s.write_bytes(&[0xC9, 0xCD]);
        assert_eq!(s.take_output(), vec![0xC9, 0xCD]);
        assert_eq!(s.where_x(), 3);
    }

    #[test]
    fn text_never_carries_controls() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.take_output();
        s.write_text("a\u{2190}[2J\u{2022}\u{266a}\u{25d9}\x07\x1b]0;x\u{2191}b");
        assert_eq!(s.take_output(), b"a[2J\xf9]0;xb".to_vec());
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
        s.take_output();
        s.set_color(Color::new(11, 0));
        s.set_color(Color::new(11, 0));
        assert_eq!(s.take_output(), b"\x1b[1;36m");
    }

    #[test]
    fn extended_colours_pass_through() {
        for cs in [Charset::Utf8, Charset::Cp437] {
            let mut s = Screen::new(cs, 25);
            s.take_output();
            // The 102 and 5 inside these must not read as iCE green / blink.
            s.write_bytes(b"\x1b[48;2;102;5;40m\x1b[38;5;196mX");
            assert_eq!(s.take_output(), b"\x1b[48;2;102;5;40m\x1b[38;5;196mX");
            assert_eq!(s.color, Color::new(4, 4));
            // Moving back to a scheme colour resets first.
            s.set_color(Color::new(7, 0));
            assert_eq!(s.take_output(), b"\x1b[0m");
        }
    }

    #[test]
    fn aixterm_colours() {
        // Unicode terminals get them as they are.
        let mut s = Screen::new(Charset::Utf8, 25);
        s.write_bytes(b"\x1b[104;91mX");
        assert_eq!(s.take_output(), b"\x1b[104;91mX");
        // CP437 terminals get DOS codes, with iCE for the background.
        let mut s = Screen::new(Charset::Cp437, 25);
        s.take_output();
        s.write_bytes(b"\x1b[104;91mX\x1b[40;31mY");
        assert_eq!(s.take_output(), b"\x1b[0;1;5;31;44mX\x1b[0;31;40mY");
        // A later colour change starts from a clean slate.
        let mut s = Screen::new(Charset::Utf8, 25);
        s.write_bytes(b"\x1b[100m");
        s.take_output();
        s.set_color(Color::new(7, 0));
        assert_eq!(s.take_output(), b"\x1b[0m");
    }

    #[test]
    fn truecolor_bright_backgrounds() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.set_truecolor(true);
        s.take_output();
        s.write_bytes(b"\x1b[102;30mX\x1b[40mY");
        assert_eq!(s.take_output(), b"\x1b[0;30;48;2;85;255;85mX\x1b[0;30;40mY");
        // Blink in DOS art (an iCE export) is a bright background too.
        s.write_bytes(b"\x1b[0;5;30;47mA\x1b[25mB");
        assert_eq!(s.take_output(), b"\x1b[0;30;48;2;255;255;255mA\x1b[0;30;47mB");
        // set_color too (a light drawn at a marker of the art's colour).
        s.set_color(Color { fore: 15, back: 7, blink: true });
        assert_eq!(s.take_output(), b"\x1b[0;1;37;48;2;255;255;255m");
    }

    #[test]
    fn ice_mode_at_connect() {
        let mut s = Screen::new(Charset::Cp437, 25);
        assert_eq!(s.take_output(), b"\x1b[?33h");
    }

    #[test]
    fn scroll_regions_keep_the_bottom_line() {
        let mut s = Screen::new(Charset::Cp437, 5);
        s.set_scroll_region(1, 4);
        s.write_bytes(b"\r\n\r\n\r\n\r\n\r\n");
        assert_eq!(s.where_y(), 4, "the region scrolls; the cursor stays on its last row");
        s.goto(1, 5);
        s.write_bytes(b"status");
        s.reset_scroll_region();
        s.write_bytes(b"\r\n\r\n\r\n\r\n\r\n");
        assert_eq!(s.where_y(), 5);
    }

    #[test]
    fn bottom_row_scrolls() {
        let mut s = Screen::new(Charset::Cp437, 3);
        s.write_bytes(b"\r\n\r\n\r\n\r\n");
        assert_eq!(s.where_y(), 3);
    }

    #[test]
    fn plain_text_has_no_escapes() {
        let mut s = Screen::new(Charset::Utf8, 25);
        s.set_plain(true);
        s.set_color(Color { fore: 12, back: 0, blink: false });
        s.write_bytes(b"\x1b[1;31mhi\x07\x1b[5;10H there");
        s.goto(3, 4);
        s.write_bytes(b"\x1b[2Jnext");
        assert_eq!(s.take_output(), b"hi there\r\nnext");
    }
}
