//! New: TheDraw fonts (`.TDF`), the block and color faces the scene
//! made its logos in, read well enough to set a word: the screen saver
//! sets the board's name in one. A file holds one or more faces; a glyph
//! is rows of (character, attribute), 0x0D ending a row.

use std::path::Path;

/// A face: its name, the columns between letters, and its glyphs by
/// character (a block or outline face's cells are all in color 7).
pub struct Font {
    pub name: String,
    /// 0 outline, 1 block, 2 color.
    pub kind: u8,
    pub spacing: u8,
    glyphs: std::collections::HashMap<char, Glyph>,
}

struct Glyph {
    width: u8,
    /// Rows of (character, attribute: fg in the low nibble, bg above).
    rows: Vec<Vec<(u8, u8)>>,
}

/// A cell of a set word: where it goes (0-based, from the word's top
/// left), what it shows and its DOS colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub ch: u8,
    pub fg: u8,
    pub bg: u8,
}

/// The faces in a file; none if it isn't a TheDraw font file.
pub fn read(path: &Path) -> Vec<Font> {
    let Ok(d) = std::fs::read(path) else { return Vec::new() };
    if !d.starts_with(b"\x13TheDraw FONTS file\x1a") {
        return Vec::new();
    }
    let mut fonts = Vec::new();
    let mut p = 20;
    while p + 213 <= d.len() && &d[p..p + 4] == b"\x55\xaa\x00\xff" {
        let name_len = (d[p + 4] as usize).min(12);
        let name = String::from_utf8_lossy(&d[p + 5..p + 5 + name_len]).trim_end_matches(['\0', ' ']).to_string();
        let (kind, spacing) = (d[p + 21], d[p + 22]);
        let size = u16::from_le_bytes([d[p + 23], d[p + 24]]) as usize;
        let data = &d[p + 213..(p + 213 + size).min(d.len())];
        let mut glyphs = std::collections::HashMap::new();
        for i in 0..94 {
            let o = u16::from_le_bytes([d[p + 25 + i * 2], d[p + 26 + i * 2]]) as usize;
            if o == 0xFFFF || o + 2 > data.len() {
                continue;
            }
            let width = data[o];
            let mut q = o + 2;
            let (mut rows, mut row) = (Vec::new(), Vec::new());
            // No glyph is taller than a screen; a hostile file can't
            // make one that is.
            while q < data.len() && data[q] != 0 && rows.len() < 64 {
                if data[q] == 0x0D {
                    rows.push(std::mem::take(&mut row));
                    q += 1;
                } else if kind == 2 {
                    row.push((data[q], *data.get(q + 1).unwrap_or(&7)));
                    q += 2;
                } else {
                    row.push((data[q], 7));
                    q += 1;
                }
            }
            if !row.is_empty() {
                rows.push(row);
            }
            glyphs.insert((33 + i as u8) as char, Glyph { width, rows });
        }
        fonts.push(Font { name, kind, spacing, glyphs });
        p += 213 + size;
    }
    fonts
}

impl Font {
    /// The word set in the face: its drawn cells, blank rows over and
    /// under it dropped, and its width and height. Letters the face
    /// lacks are skipped; an uppercase one stands in for a missing
    /// lowercase one.
    pub fn set(&self, word: &str) -> (Vec<Cell>, u16, u16) {
        let (cells, w, h) = self.set_letters(word);
        (cells.into_iter().map(|(_, c)| c).collect(), w, h)
    }

    /// [`Font::set`], each cell with which of the word's letters (not
    /// counting spaces) it's part of.
    pub fn set_letters(&self, word: &str) -> (Vec<(usize, Cell)>, u16, u16) {
        let mut cells: Vec<(usize, Cell)> = Vec::new();
        let mut x0 = 0u16;
        // A space (no glyph in any face) is a gap of two columns.
        let glyphs: Vec<Option<&Glyph>> = word
            .chars()
            .filter_map(|c| if c == ' ' { Some(None) } else { self.glyphs.get(&c).or_else(|| self.glyphs.get(&c.to_ascii_uppercase())).map(Some) })
            .collect();
        let mut letter = 0;
        for (gi, g) in glyphs.iter().enumerate() {
            let Some(g) = g else {
                x0 = x0.saturating_add(2);
                continue;
            };
            letter += 1;
            for (y, row) in g.rows.iter().enumerate() {
                for (x, &(ch, attr)) in row.iter().take(g.width as usize).enumerate() {
                    let (fg, bg) = (attr & 15, (attr >> 4) & 7);
                    if ch != 0 && (ch != b' ' || bg != 0) {
                        cells.push((letter - 1, Cell { x: x0 + x as u16, y: y as u16, ch, fg, bg }));
                    }
                }
            }
            x0 = x0.saturating_add(g.width as u16 + if gi + 1 < glyphs.len() { self.spacing as u16 } else { 0 });
        }
        if cells.is_empty() {
            return (cells, 0, 0);
        }
        let (top, left) = (cells.iter().map(|c| c.1.y).min().unwrap_or(0), cells.iter().map(|c| c.1.x).min().unwrap_or(0));
        for (_, c) in &mut cells {
            c.x -= left;
            c.y -= top;
        }
        let w = cells.iter().map(|c| c.1.x).max().unwrap_or(0) + 1;
        let h = cells.iter().map(|c| c.1.y).max().unwrap_or(0) + 1;
        (cells, w, h)
    }
}

#[cfg_attr(not(feature = "screensavers"), allow(dead_code))]
impl Font {
    /// The first of `texts` that sets no wider than `max` columns, and
    /// which it was; the last of them (too wide, to be cut off) if none
    /// fits.
    pub fn set_fitting(&self, texts: &[String], max: u16) -> ((Vec<Cell>, u16, u16), String) {
        let mut last = None;
        for t in texts {
            let set = self.set(t);
            if set.1 <= max {
                return (set, t.clone());
            }
            last = Some((set, t.clone()));
        }
        last.unwrap_or_else(|| ((Vec::new(), 0, 0), String::new()))
    }
}

/// The DOS palette as RGB, by attribute color 0-15.
#[cfg_attr(not(feature = "screensavers"), allow(dead_code))]
pub fn rgb(color: u8) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 16] = [
        [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170], [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170],
        [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255], [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255],
    ];
    PALETTE[(color & 15) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-face file with a two-column `A` (a block, then a row with
    /// a colored block and a space) and a one-column `B`.
    fn file() -> Vec<u8> {
        let mut d = b"\x13TheDraw FONTS file\x1a".to_vec();
        d.extend(b"\x55\xaa\x00\xff");
        d.push(4);
        d.extend(b"Test        ");
        d.extend([0, 0, 0, 0]);
        d.extend([2, 1]); // color font, one column between letters
        let a = [2u8, 2, 0xDB, 0x0B, 0xDB, 0x0B, 0x0D, 0xDC, 0x03, b' ', 0x00, 0x0D, 0x00];
        let b = [1u8, 1, 0xDB, 0x0F, 0x0D, 0x00];
        let size = (a.len() + b.len()) as u16;
        d.extend(size.to_le_bytes());
        let mut offs = [0xFFFFu16; 94];
        offs[(b'A' - 33) as usize] = 0;
        offs[(b'B' - 33) as usize] = a.len() as u16;
        for o in offs {
            d.extend(o.to_le_bytes());
        }
        d.extend(a);
        d.extend(b);
        d
    }

    #[test]
    fn sets_a_word_from_a_font_file() {
        let dir = std::env::temp_dir().join(format!("fluxbbs-tdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("TEST.TDF");
        std::fs::write(&path, file()).unwrap();
        let fonts = read(&path);
        assert_eq!(fonts.len(), 1);
        assert_eq!((fonts[0].name.as_str(), fonts[0].spacing), ("Test", 1));
        let (cells, w, h) = fonts[0].set("AB");
        assert_eq!((w, h), (4, 2), "A is two wide, a column between, B one wide");
        assert!(cells.contains(&Cell { x: 0, y: 0, ch: 0xDB, fg: 11, bg: 0 }));
        assert!(cells.contains(&Cell { x: 0, y: 1, ch: 0xDC, fg: 3, bg: 0 }));
        assert!(cells.contains(&Cell { x: 3, y: 0, ch: 0xDB, fg: 15, bg: 0 }));
        assert!(!cells.iter().any(|c| c.x == 1 && c.y == 1), "a blank cell isn't drawn");
        assert_eq!(fonts[0].set("ab").0.len(), cells.len(), "lowercase takes the capitals");
        assert_eq!(fonts[0].set("A B").1, 6, "a space is a two-column gap");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_glyph_is_no_taller_than_a_screen() {
        // A face whose `A` is 10,000 rows of one block: read, but cut.
        let mut d = b"\x13TheDraw FONTS file\x1a".to_vec();
        d.extend(b"\x55\xaa\x00\xff");
        d.push(4);
        d.extend(b"Tall        ");
        d.extend([0, 0, 0, 0]);
        d.extend([1, 1]);
        let mut a = vec![1u8, 1];
        for _ in 0..10_000 {
            a.extend([0xDB, 0x0D]);
        }
        a.push(0);
        d.extend((a.len() as u16).to_le_bytes());
        let mut offs = [0xFFFFu16; 94];
        offs[(b'A' - 33) as usize] = 0;
        for o in offs {
            d.extend(o.to_le_bytes());
        }
        d.extend(a);
        let dir = std::env::temp_dir().join(format!("fluxbbs-tdf-tall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("TALL.TDF");
        std::fs::write(&path, d).unwrap();
        let fonts = read(&path);
        assert_eq!(fonts[0].set("A").2, 64);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
