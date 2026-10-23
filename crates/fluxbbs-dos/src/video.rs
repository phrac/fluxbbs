//! The text screen: 80x25 cells at B800:0000 (character, attribute), the
//! BIOS data area's cursor, and INT 10h on top of them. Both direct
//! video-memory writes and BIOS calls end up in the same buffer, which
//! [`Screen::render`] turns into ANSI for a terminal.

use crate::cpu::Cpu;
use crate::{Memory, VIDEO_BASE};

pub const COLS: u16 = 80;
pub const ROWS: u16 = 25;

/// BIOS data area fields (segment 0040h).
pub const BDA: u32 = 0x400;
const BDA_MODE: u32 = BDA + 0x49;
const BDA_COLS: u32 = BDA + 0x4A;
const BDA_PAGE_SIZE: u32 = BDA + 0x4C;
const BDA_CURSOR: u32 = BDA + 0x50;
const BDA_CURSOR_SHAPE: u32 = BDA + 0x60;
const BDA_ROWS: u32 = BDA + 0x84;
const BDA_CRTC: u32 = BDA + 0x63;

/// What the terminal was last told, to send only changes.
pub struct Screen {
    shown: Vec<u16>,
    shown_cursor: (u16, u16),
    shown_color: Option<u8>,
    pub cursor_hidden: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        // The caller's screen counts as blank to start with, so only what
        // the program draws goes out (not 2,000 spaces).
        Self { shown: vec![0x0720; (COLS * ROWS) as usize], shown_cursor: (0, 0), shown_color: None, cursor_hidden: false }
    }

    /// Clear the screen (attribute 7) and home the cursor.
    pub fn clear(mem: &mut Memory) {
        for i in 0..(COLS * ROWS) as u32 {
            mem.write16(VIDEO_BASE + i * 2, 0x0720);
        }
        Self::set_cursor(mem, 0, 0);
    }

    /// Set up mode 3 in the BIOS data area and clear the screen.
    pub fn init(mem: &mut Memory) {
        mem.write8(BDA_MODE, 3);
        mem.write16(BDA_COLS, COLS);
        mem.write16(BDA_PAGE_SIZE, 4096);
        mem.write16(BDA_CURSOR, 0);
        mem.write16(BDA_CURSOR_SHAPE, 0x0607);
        mem.write8(BDA_ROWS, (ROWS - 1) as u8);
        mem.write16(BDA_CRTC, 0x3D4);
        for i in 0..(COLS * ROWS) as u32 {
            mem.write16(VIDEO_BASE + i * 2, 0x0720);
        }
    }

    pub fn cursor(mem: &Memory) -> (u16, u16) {
        let v = mem.read16(BDA_CURSOR);
        ((v & 0xFF), (v >> 8))
    }

    pub fn set_cursor(mem: &mut Memory, x: u16, y: u16) {
        mem.write16(BDA_CURSOR, (x & 0xFF) | (y << 8));
    }

    #[inline]
    fn cell_addr(x: u16, y: u16) -> u32 {
        VIDEO_BASE + (y as u32 * COLS as u32 + x as u32) * 2
    }

    /// Write one character at the cursor without moving it (`AH=0Ah`),
    /// keeping or setting the attribute.
    pub fn put(mem: &mut Memory, x: u16, y: u16, ch: u8, attr: Option<u8>) {
        if x >= COLS || y >= ROWS {
            return;
        }
        let a = Self::cell_addr(x, y);
        mem.write8(a, ch);
        if let Some(attr) = attr {
            mem.write8(a + 1, attr);
        }
    }

    /// Scroll a window up (`n > 0`) or down (`n < 0`); 0 clears it.
    pub fn scroll(mem: &mut Memory, x1: u16, y1: u16, x2: u16, y2: u16, n: i32, attr: u8) {
        let (x1, y1, x2, y2) = (x1.min(COLS - 1), y1.min(ROWS - 1), x2.min(COLS - 1), y2.min(ROWS - 1));
        if x1 > x2 || y1 > y2 {
            return;
        }
        let height = (y2 - y1 + 1) as i32;
        let n = if n == 0 || n.abs() >= height { height } else { n };
        let blank = 0x20 | (attr as u16) << 8;
        let rows: Vec<u16> = if n > 0 { (y1..=y2).collect() } else { (y1..=y2).rev().collect() };
        for y in rows {
            let src = y as i32 + n;
            for x in x1..=x2 {
                let v = if src >= y1 as i32 && src <= y2 as i32 { mem.read16(Self::cell_addr(x, src as u16)) } else { blank };
                mem.write16(Self::cell_addr(x, y), v);
            }
        }
    }

    /// Teletype output (`AH=0Eh`, DOS console writes): control characters
    /// act, the cursor moves and the screen scrolls.
    pub fn teletype(mem: &mut Memory, ch: u8, attr: u8) {
        let (mut x, mut y) = Self::cursor(mem);
        match ch {
            0x07 => {}
            0x08 => x = x.saturating_sub(1),
            0x0A => y += 1,
            0x0D => x = 0,
            0x09 => {
                x = (x / 8 + 1) * 8;
                if x >= COLS {
                    x = 0;
                    y += 1;
                }
            }
            _ => {
                Self::put(mem, x, y, ch, Some(attr));
                x += 1;
                if x >= COLS {
                    x = 0;
                    y += 1;
                }
            }
        }
        if y >= ROWS {
            Self::scroll(mem, 0, 0, COLS - 1, ROWS - 1, 1, attr);
            y = ROWS - 1;
        }
        Self::set_cursor(mem, x, y);
    }

    /// `INT 10h`.
    pub fn int10(cpu: &mut Cpu, mem: &mut Memory) {
        let (ah, al) = (cpu.ah(), cpu.al());
        match ah {
            0x00 => {
                // Any mode asked for is text mode 3 here.
                Self::init(mem);
            }
            0x01 => {
                let shape = cpu.cx();
                mem.write16(BDA_CURSOR_SHAPE, shape);
            }
            0x02 => Self::set_cursor(mem, cpu.r8(2) as u16, cpu.r8(6) as u16),
            0x03 => {
                let (x, y) = Self::cursor(mem);
                cpu.set_r16(crate::cpu::DX, x | y << 8);
                cpu.set_r16(crate::cpu::CX, mem.read16(BDA_CURSOR_SHAPE));
            }
            0x05 => {}
            0x06 | 0x07 => {
                let n = if ah == 0x06 { al as i32 } else { -(al as i32) };
                Self::scroll(mem, cpu.r8(1) as u16, cpu.r8(5) as u16, cpu.r8(2) as u16, cpu.r8(6) as u16, n, cpu.r8(7));
            }
            0x08 => {
                let (x, y) = Self::cursor(mem);
                let v = mem.read16(Self::cell_addr(x.min(COLS - 1), y.min(ROWS - 1)));
                cpu.set_ax(v);
            }
            0x09 | 0x0A => {
                let (x, y) = Self::cursor(mem);
                let attr = if ah == 0x09 { Some(cpu.r8(3)) } else { None };
                let mut n = cpu.cx();
                let (mut cx, mut cy) = (x, y);
                while n > 0 {
                    Self::put(mem, cx, cy, al, attr);
                    n -= 1;
                    cx += 1;
                    if cx >= COLS {
                        cx = 0;
                        cy += 1;
                        if cy >= ROWS {
                            break;
                        }
                    }
                }
            }
            0x0E => Self::teletype(mem, al, 0x07),
            0x0F => {
                cpu.set_ah(COLS as u8);
                cpu.set_al(3);
                cpu.set_r8(7, 0);
            }
            0x10..=0x12 => {
                // Palette, fonts, EGA/VGA extras: nothing to do, but a few
                // return values keep callers honest.
                if ah == 0x11 && al == 0x30 {
                    // Font info: 16-line characters, 24 rows.
                    cpu.set_r16(crate::cpu::CX, 16);
                    cpu.set_r16(crate::cpu::DX, ROWS - 1);
                }
                if ah == 0x12 && cpu.r8(3) == 0x10 {
                    // EGA info: colour, 256 KB.
                    cpu.set_r8(3, 0);
                    cpu.set_r8(7, 3);
                }
            }
            0x13 => {
                // Write string from ES:BP; AL bit 0 moves the cursor, bit 1
                // means the string carries attributes.
                let (mut x, mut y) = (cpu.r8(2) as u16, cpu.r8(6) as u16);
                let (start_x, start_y) = (x, y);
                let n = cpu.cx();
                let with_attr = al & 2 != 0;
                let mut off = cpu.bp() as u32;
                let es = cpu.es();
                for _ in 0..n {
                    let ch = mem.read8(crate::linear(es, off));
                    off += 1;
                    let attr = if with_attr {
                        let a = mem.read8(crate::linear(es, off));
                        off += 1;
                        a
                    } else {
                        cpu.r8(3)
                    };
                    Self::set_cursor(mem, x, y);
                    Self::teletype(mem, ch, attr);
                    let (nx, ny) = Self::cursor(mem);
                    x = nx;
                    y = ny;
                }
                if al & 1 == 0 {
                    Self::set_cursor(mem, start_x, start_y);
                }
            }
            0x1A => {
                // Display combination: VGA with colour display.
                cpu.set_al(0x1A);
                cpu.set_r16(crate::cpu::BX, 0x0008);
            }
            0xFE => {
                // Some programs ask for the real video segment: ES:DI as given.
            }
            _ => {}
        }
    }

    /// ANSI for what changed since the last render (the whole screen the
    /// first time). The terminal must be at least 80x25.
    pub fn render(&mut self, mem: &Memory) -> Vec<u8> {
        let mut out = Vec::new();
        let (cx, cy) = Self::cursor(mem);
        let mut at: Option<(u16, u16)> = None;
        for y in 0..ROWS {
            for x in 0..COLS {
                let i = (y * COLS + x) as usize;
                let v = mem.read16(Self::cell_addr(x, y));
                if self.shown[i] == v {
                    // An unchanged cell between changed ones is cheaper
                    // written than skipped over.
                    let bridge = at == Some((x, y)) && x + 1 < COLS && self.shown[i + 1] != mem.read16(Self::cell_addr(x + 1, y));
                    if !bridge {
                        continue;
                    }
                }
                self.shown[i] = v;
                if at != Some((x, y)) {
                    out.extend(format!("\x1b[{};{}H", y + 1, x + 1).into_bytes());
                }
                let attr = (v >> 8) as u8;
                if self.shown_color != Some(attr) {
                    out.extend(sgr(attr).into_bytes());
                    self.shown_color = Some(attr);
                }
                let ch = v as u8;
                out.push(if ch == 0 { b' ' } else { ch });
                at = if x + 1 < COLS { Some((x + 1, y)) } else { None };
            }
        }
        if self.shown_cursor != (cx, cy) || !out.is_empty() {
            out.extend(format!("\x1b[{};{}H", cy.min(ROWS - 1) + 1, cx.min(COLS - 1) + 1).into_bytes());
            self.shown_cursor = (cx, cy);
        }
        out
    }

    /// Send everything next time.
    pub fn invalidate(&mut self) {
        self.shown.fill(0xFFFF);
        self.shown_color = None;
        self.shown_cursor = (0xFFFF, 0xFFFF);
    }
}

/// An ANSI colour sequence for a text-mode attribute (blink as bright
/// background, the iCE convention).
pub fn sgr(attr: u8) -> String {
    const MAP: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
    let fg = MAP[(attr & 7) as usize];
    let bg = MAP[((attr >> 4) & 7) as usize];
    let bright = attr & 8 != 0;
    let blink = attr & 0x80 != 0;
    format!("\x1b[0;{}{};{}m", if bright { "1;" } else { "" }, 30 + fg as u32, if blink { 100 + bg as u32 } else { 40 + bg as u32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teletype_scrolls_and_renders_changes() {
        let mut mem = Memory::new();
        Screen::init(&mut mem);
        for _ in 0..26 {
            for &b in b"ab\r\n" {
                Screen::teletype(&mut mem, b, 0x07);
            }
        }
        // 26 lines on a 25-row screen: the top line scrolled off, the last
        // written line is row 24.
        assert_eq!(Screen::cursor(&mem), (0, 24));
        assert_eq!(mem.read8(VIDEO_BASE + 23 * 160), b'a');
        let mut scr = Screen::new();
        let first = scr.render(&mem);
        // Only the written cells go out, not the blank ones.
        assert!(first.starts_with(b"\x1b[1;1H") && first.len() < 600, "{}", first.len());
        assert!(scr.render(&mem).is_empty(), "nothing changed");
        Screen::put(&mut mem, 5, 5, b'X', Some(0x4E));
        let s = String::from_utf8_lossy(&scr.render(&mem)).into_owned();
        assert!(s.starts_with("\x1b[6;6H\x1b[0;1;33;41mX"), "{s:?}");
    }

    #[test]
    fn int10_cursor_and_scroll() {
        let mut mem = Memory::new();
        Screen::init(&mut mem);
        let mut cpu = Cpu::new();
        cpu.set_ax(0x0200);
        cpu.set_r16(crate::cpu::DX, 0x0310); // row 3, col 16
        Screen::int10(&mut cpu, &mut mem);
        assert_eq!(Screen::cursor(&mem), (16, 3));
        cpu.set_ax(0x0E41); // teletype 'A'
        Screen::int10(&mut cpu, &mut mem);
        assert_eq!(mem.read8(VIDEO_BASE + 3 * 160 + 16 * 2), b'A');
        // Scroll the whole screen up one line.
        cpu.set_ax(0x0601);
        cpu.set_r16(crate::cpu::CX, 0);
        cpu.set_r16(crate::cpu::DX, 0x184F);
        cpu.set_r8(7, 0x07);
        Screen::int10(&mut cpu, &mut mem);
        assert_eq!(mem.read8(VIDEO_BASE + 2 * 160 + 16 * 2), b'A');
    }
}
