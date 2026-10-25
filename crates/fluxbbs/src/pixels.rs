//! New: pictures in half-block pixels, for the casino (`slots`,
//! `blackjack`, `craps`, `casino`). A canvas is pixels two to a cell
//! (so they're square), with a layer of text cells over them for
//! labels; frames go out as only the cells that changed, in 24-bit color
//! or the nearest of xterm's 256.

pub type Rgb = [f32; 3];

pub fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    [r as f32, g as f32, b as f32]
}

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn scale(a: Rgb, k: f32) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn add(a: Rgb, b: Rgb, k: f32) -> Rgb {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

pub fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A steady pseudo-random 0..1 for a point (textures).
pub fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// A text cell over the pixels: a CP437 byte, its color, and its
/// background (none: the pixels under it, averaged).
#[derive(Clone, Copy, PartialEq)]
pub struct Glyph {
    pub ch: u8,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
}

#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Rgb>,
    pub text: Vec<Option<Glyph>>,
}

impl Canvas {
    /// `w` by `h` pixels (`h` even: two to a row).
    pub fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![[0.0; 3]; w * h], text: vec![None; w * h / 2] }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgb> {
        (x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h).then(|| self.px[y as usize * self.w + x as usize])
    }

    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// `c` over what's there, `a` of the way.
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if let Some(p) = self.get(x, y) {
            self.set(x, y, mix(p, c, a));
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    /// Text in the 5x7 font, its top left at (`x`, `y`), `size` pixels a
    /// font pixel; each pixel colored by `ink` (its place in the text, 0
    /// to 1 across and down). Returns the width.
    pub fn print(&mut self, x: i32, y: i32, s: &str, size: i32, ink: impl Fn(f32, f32) -> Rgb) -> i32 {
        let w = text_width(s) * size;
        let mut at = x;
        for ch in s.chars() {
            let Some((rows, l, r)) = glyph(ch) else {
                at += 3 * size;
                continue;
            };
            for (gy, bits) in rows.iter().enumerate() {
                for gx in l..=r {
                    if bits >> (4 - gx) & 1 == 1 {
                        for dy in 0..size {
                            for dx in 0..size {
                                let (px, py) = (at + (gx - l) as i32 * size + dx, y + gy as i32 * size + dy);
                                let c = ink((px - x) as f32 / w.max(1) as f32, (py - y) as f32 / (7 * size) as f32);
                                self.set(px, py, c);
                            }
                        }
                    }
                }
            }
            at += (r - l + 2) as i32 * size;
        }
        w
    }

    /// Text cells over the pixels, from cell (`col`, `row`), 0-based.
    pub fn label(&mut self, col: usize, row: usize, s: &str, fg: Rgb, bg: Option<Rgb>) {
        let cols = self.w;
        for (i, b) in fluxbbs_term::cp437::encode(s).into_iter().enumerate() {
            if col + i < cols && row < self.h / 2 {
                self.text[row * cols + col + i] = Some(Glyph { ch: b, fg, bg });
            }
        }
    }

    pub fn clear_labels(&mut self) {
        self.text.iter_mut().for_each(|t| *t = None);
    }
}

/// The 5x7 font: rows, top first, five bits each.
fn glyph(c: char) -> Option<([u8; 7], usize, usize)> {
    let rows: [u8; 7] = match c.to_ascii_uppercase() {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '$' => [0x04, 0x0F, 0x14, 0x0E, 0x05, 0x1E, 0x04],
        _ => return None,
    };
    // Trimmed to its ink, so text is spaced by what's drawn.
    let used = rows.iter().fold(0u8, |a, r| a | r);
    let l = (0..5).find(|i| used >> (4 - i) & 1 == 1)?;
    let r = (0..5).rev().find(|i| used >> (4 - i) & 1 == 1)?;
    Some((rows, l, r))
}

/// Width of text in the 5x7 font at size 1.
pub fn text_width(s: &str) -> i32 {
    let w: i32 = s.chars().map(|c| glyph(c).map_or(3, |(_, l, r)| (r - l + 2) as i32)).sum();
    (w - 1).max(0)
}

// ---- output ------------------------------------------------------------

/// A cell as sent: top and bottom colors (24-bit, or xterm indexes), or
/// for text, its color, background and character.
type Cell = (u32, u32, u8);

pub struct Encoder {
    pub truecolor: bool,
    prev: Vec<Option<Cell>>,
}

impl Encoder {
    pub fn new(truecolor: bool, w: usize, h: usize) -> Encoder {
        Encoder { truecolor, prev: vec![None; w * h / 2] }
    }

    /// Everything is drawn again next frame.
    pub fn forget(&mut self) {
        self.prev.iter_mut().for_each(|c| *c = None);
    }

    pub fn code(&self, c: Rgb) -> u32 {
        let q = |v: f32| ((v.round().clamp(0.0, 255.0) as u32 + 2) & !3).min(255);
        let (r, g, b) = (q(c[0]), q(c[1]), q(c[2]));
        if self.truecolor {
            r << 16 | g << 8 | b
        } else {
            crate::picture::xterm256([r as u8, g as u8, b as u8]) as u32
        }
    }

    fn sgr(&self, out: &mut Vec<u8>, fore: bool, c: u32) {
        let k = if fore { 38 } else { 48 };
        let s = if self.truecolor { format!("{k};2;{};{};{}", c >> 16, (c >> 8) & 255, c & 255) } else { format!("{k};5;{c}") };
        out.extend(s.as_bytes());
    }

    /// The ANSI for the cells that changed since the last frame, the
    /// canvas's top row at screen row `top`.
    pub fn frame(&mut self, cv: &Canvas, top: u16) -> Vec<u8> {
        let mut out = Vec::new();
        let w = cv.w;
        let (mut fg, mut bg): (Option<u32>, Option<u32>) = (None, None);
        for row in 0..cv.h / 2 {
            let mut at: Option<usize> = None;
            for x in 0..w {
                let (up, down) = (cv.px[row * 2 * w + x], cv.px[(row * 2 + 1) * w + x]);
                let cell = match cv.text[row * w + x] {
                    Some(g) => (self.code(g.fg), self.code(g.bg.unwrap_or_else(|| scale(add(up, down, 1.0), 0.5))), g.ch),
                    None => (self.code(up), self.code(down), 0),
                };
                let i = row * w + x;
                if self.prev[i] == Some(cell) {
                    continue;
                }
                self.prev[i] = Some(cell);
                match at {
                    Some(c) if c == x => {}
                    Some(c) if x - c <= 4 => out.extend(format!("\x1b[{}C", x - c).as_bytes()),
                    _ => out.extend(format!("\x1b[{};{}H", top as usize + row, x + 1).as_bytes()),
                }
                let mut sets: Vec<(bool, u32)> = Vec::new();
                let glyph = if cell.2 != 0 {
                    // Text: its color on its background.
                    let (ink, back) = (cell.0, cell.1);
                    if bg != Some(back) {
                        sets.push((false, back));
                    }
                    if cell.2 != b' ' && fg != Some(ink) {
                        sets.push((true, ink));
                    }
                    cell.2
                } else {
                    let (up, down) = (cell.0, cell.1);
                    // The glyph that needs the fewest changes: a space or a full
                    // block for one color, else a half block either way up.
                    if up == down {
                        if bg == Some(up) {
                            b' '
                        } else if fg == Some(up) {
                            0xDB
                        } else {
                            sets.push((false, up));
                            b' '
                        }
                    } else if fg == Some(up) && bg == Some(down) {
                        0xDF
                    } else if bg == Some(up) && fg == Some(down) {
                        0xDC
                    } else if fg == Some(up) || bg == Some(down) {
                        if fg != Some(up) {
                            sets.push((true, up));
                        }
                        if bg != Some(down) {
                            sets.push((false, down));
                        }
                        0xDF
                    } else {
                        if bg != Some(up) {
                            sets.push((false, up));
                        }
                        if fg != Some(down) {
                            sets.push((true, down));
                        }
                        0xDC
                    }
                };
                if !sets.is_empty() {
                    // A sequence each: NetRunner reads only one 24-bit
                    // colour per sequence.
                    for &(fore, c) in &sets {
                        out.extend(b"\x1b[");
                        self.sgr(&mut out, fore, c);
                        out.push(b'm');
                        if fore { fg = Some(c) } else { bg = Some(c) }
                    }
                }
                out.push(glyph);
                at = Some(x + 1);
            }
        }
        if !out.is_empty() {
            out.extend(b"\x1b[0m");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_sits_on_the_pixels_and_is_sent_once() {
        let mut cv = Canvas::new(10, 4);
        cv.rect(0, 0, 10, 4, rgb(0, 100, 0));
        cv.label(2, 1, "hi", rgb(255, 255, 255), None);
        let mut enc = Encoder::new(true, 10, 4);
        let out = enc.frame(&cv, 1);
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("48;2;0;100;0") && s.contains("hi"), "{s}");
        assert!(enc.frame(&cv, 1).is_empty());
        cv.label(2, 1, "ho", rgb(255, 255, 255), None);
        let s = String::from_utf8_lossy(&enc.frame(&cv, 1)).to_string();
        assert!(s.ends_with("o\x1b[0m") && !s.contains('h'), "{s:?}");
    }

    #[test]
    fn the_font_is_spaced_by_its_ink() {
        assert_eq!(text_width("1"), 3);
        assert_eq!(text_width("10"), 3 + 1 + 5);
        let mut cv = Canvas::new(20, 8);
        assert_eq!(cv.print(0, 0, "A", 1, |_, _| rgb(255, 0, 0)), 5);
        assert_eq!(cv.px[3 * 20], rgb(255, 0, 0), "A's crossbar");
    }
}
