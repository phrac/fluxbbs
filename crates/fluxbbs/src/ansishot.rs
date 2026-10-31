//! New: a picture of a piece of ANSI art, for the AI to look at when it
//! describes a file (`aidesc`). The art is played onto a grid of cells as
//! a terminal would (colors 16, 256 and 24-bit, iCE backgrounds, cursor
//! moves) and each cell drawn as an 8x16 block: the block characters
//! exactly, the shades blended, and anything else as a tint of its
//! color, which is enough to see what a piece shows. Its words go to the
//! AI as text besides.

use image::{Rgb, RgbImage};

const W: usize = 8;
const H: usize = 16;
/// The tallest picture made, in rows of text.
const MAX_ROWS: usize = 120;

const DOS: [[u8; 3]; 16] = [
    [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170], [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170],
    [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255], [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255],
];
/// ANSI color numbers to DOS ones.
const ANSI: [usize; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

#[derive(Clone, Copy)]
struct Cell {
    ch: u8,
    fg: [u8; 3],
    bg: [u8; 3],
}

fn xterm(n: u8) -> [u8; 3] {
    match n {
        0..=15 => DOS[if n < 8 { ANSI[n as usize] } else { ANSI[n as usize - 8] + 8 }],
        16..=231 => {
            let n = n - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            [c(n / 36), c(n / 6 % 6), c(n % 6)]
        }
        _ => {
            let v = 8 + 10 * (n - 232);
            [v, v, v]
        }
    }
}

/// The art's width: its SAUCE's (a character file's TInfo1), else 80.
fn width(data: &[u8]) -> usize {
    let w = data
        .len()
        .checked_sub(128)
        .map(|i| &data[i..])
        .filter(|r| r.starts_with(b"SAUCE00") && r[94] == 1)
        .map_or(0, |r| u16::from_le_bytes([r[96], r[97]]) as usize);
    if (1..=255).contains(&w) { w } else { 80 }
}

/// The art as a PNG, or None if nothing in it shows.
pub fn png(data: &[u8]) -> Option<Vec<u8>> {
    let cols = width(data);
    let body = &data[..data.iter().position(|&b| b == 0x1A).unwrap_or(data.len())];
    let blank = Cell { ch: b' ', fg: DOS[7], bg: DOS[0] };
    let mut grid: Vec<Vec<Cell>> = Vec::new();
    let (mut x, mut y, mut saved) = (0usize, 0usize, (0usize, 0usize));
    // A full row wraps at the next character, not at once, as terminals do:
    // a line exactly as wide as the art, then CR LF, is one row.
    let mut wrap = false;
    let (mut fg, mut bg, mut bold, mut blink) = (7usize, 0usize, false, false);
    let (mut tfg, mut tbg): (Option<[u8; 3]>, Option<[u8; 3]>) = (None, None);
    let mut i = 0;
    while i < body.len() && y < MAX_ROWS {
        let b = body[i];
        i += 1;
        match b {
            0x1B if body.get(i) == Some(&b'[') => {
                let start = i + 1;
                let Some(end) = body[start..].iter().position(|c| (0x40..=0x7E).contains(c)).map(|p| start + p) else { break };
                let params: Vec<usize> = std::str::from_utf8(&body[start..end]).unwrap_or("").split(';').map(|p| p.trim_start_matches('?').parse().unwrap_or(0)).collect();
                let n = |k: usize, d: usize| params.get(k).copied().filter(|&v| v > 0).unwrap_or(d);
                match body[end] {
                    b'H' | b'f' => {
                        y = n(0, 1) - 1;
                        x = (n(1, 1) - 1).min(cols - 1);
                    }
                    b'A' => y = y.saturating_sub(n(0, 1)),
                    b'B' => y += n(0, 1),
                    b'C' => x = (x + n(0, 1)).min(cols - 1),
                    b'D' => x = x.saturating_sub(n(0, 1)),
                    b's' => saved = (x, y),
                    b'u' => (x, y) = saved,
                    b'J' if params.first() == Some(&2) => {
                        grid.clear();
                        (x, y) = (0, 0);
                    }
                    b'm' => {
                        let mut k = 0;
                        while k < params.len() {
                            match params[k] {
                                0 => {
                                    (fg, bg, bold, blink) = (7, 0, false, false);
                                    (tfg, tbg) = (None, None);
                                }
                                1 => bold = true,
                                5 => blink = true,
                                22 => bold = false,
                                25 => blink = false,
                                30..=37 => {
                                    fg = ANSI[params[k] - 30];
                                    tfg = None;
                                }
                                40..=47 => {
                                    bg = ANSI[params[k] - 40];
                                    tbg = None;
                                }
                                90..=97 => {
                                    fg = ANSI[params[k] - 90] + 8;
                                    tfg = None;
                                }
                                100..=107 => {
                                    bg = ANSI[params[k] - 100] + 8;
                                    tbg = None;
                                }
                                code @ (38 | 48) => {
                                    let c = match params.get(k + 1) {
                                        Some(2) => {
                                            let c = [k + 2, k + 3, k + 4].map(|j| params.get(j).copied().unwrap_or(0).min(255) as u8);
                                            k += 4;
                                            Some(c)
                                        }
                                        Some(5) => {
                                            let c = xterm(params.get(k + 2).copied().unwrap_or(0).min(255) as u8);
                                            k += 2;
                                            Some(c)
                                        }
                                        _ => None,
                                    };
                                    if code == 38 {
                                        tfg = c.or(tfg);
                                    } else {
                                        tbg = c.or(tbg);
                                    }
                                }
                                39 => tfg = None,
                                49 => tbg = None,
                                _ => {}
                            }
                            k += 1;
                        }
                    }
                    _ => {}
                }
                // A cursor move ends a pending wrap; a color change doesn't.
                if body[end] != b'm' {
                    wrap = false;
                }
                i = end + 1;
            }
            b'\r' => {
                x = 0;
                wrap = false;
            }
            b'\n' => {
                y += 1;
                wrap = false;
            }
            b'\t' => x = ((x / 8 + 1) * 8).min(cols - 1),
            0x1B => {}
            _ => {
                if wrap {
                    (x, y, wrap) = (0, y + 1, false);
                }
                while grid.len() <= y {
                    grid.push(vec![blank; cols]);
                }
                let f = tfg.unwrap_or(DOS[if bold && fg < 8 { fg + 8 } else { fg }]);
                // iCE colors: blink is a bright background.
                let k = tbg.unwrap_or(DOS[if blink && bg < 8 { bg + 8 } else { bg }]);
                grid[y][x] = Cell { ch: b, fg: f, bg: k };
                x += 1;
                if x >= cols {
                    x = cols - 1;
                    wrap = true;
                }
            }
        }
    }
    while grid.last().is_some_and(|r| r.iter().all(|c| c.ch == b' ' && c.bg == [0, 0, 0])) {
        grid.pop();
    }
    if grid.is_empty() {
        return None;
    }
    let mut img = RgbImage::new((cols * W) as u32, (grid.len() * H) as u32);
    for (ry, row) in grid.iter().enumerate() {
        for (rx, c) in row.iter().enumerate() {
            for py in 0..H {
                for px in 0..W {
                    let mix = |t: f32| {
                        let m = |a: u8, b: u8| (a as f32 * t + b as f32 * (1.0 - t)) as u8;
                        [m(c.fg[0], c.bg[0]), m(c.fg[1], c.bg[1]), m(c.fg[2], c.bg[2])]
                    };
                    let p = match c.ch {
                        0xDB => c.fg,
                        0xDF => if py < H / 2 { c.fg } else { c.bg },
                        0xDC => if py >= H / 2 { c.fg } else { c.bg },
                        0xDD => if px < W / 2 { c.fg } else { c.bg },
                        0xDE => if px >= W / 2 { c.fg } else { c.bg },
                        0xB0 => mix(0.25),
                        0xB1 => mix(0.5),
                        0xB2 => mix(0.75),
                        b' ' | 0 | 0xFF => c.bg,
                        _ => mix(0.4),
                    };
                    img.put_pixel((rx * W + px) as u32, (ry * H + py) as u32, Rgb(p));
                }
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_colors_land_where_they_should() {
        // A red full block, then a half block: blue over (bright) green.
        let art = b"\x1b[31m\xdb\x1b[0;34;102m\xdf";
        let img = image::load_from_memory(&png(art).unwrap()).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (80 * W as u32, H as u32));
        assert_eq!(img.get_pixel(2, 2).0, [170, 0, 0]);
        assert_eq!(img.get_pixel(W as u32 + 2, 2).0, [0, 0, 170]);
        assert_eq!(img.get_pixel(W as u32 + 2, H as u32 - 2).0, [85, 255, 85]);
        assert!(png(b"   \r\n").is_none());
    }

    /// `ANSISHOT_IN=art.ans ANSISHOT_OUT=art.png cargo test --features full -- --ignored shot`
    #[test]
    #[ignore]
    fn shot() {
        let data = std::fs::read(std::env::var("ANSISHOT_IN").unwrap()).unwrap();
        std::fs::write(std::env::var("ANSISHOT_OUT").unwrap(), png(&data).unwrap()).unwrap();
    }
}
