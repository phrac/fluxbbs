//! New: the casino's underground look (`[casino] theme = "underground"`):
//! a circuit-board wall, a neon grid for a floor, the sign in acid green,
//! and neon skulls and crossbones, floppies and viruses where the classic
//! look has bulbs and red carpet. The rooms ask `on` and draw from here.

use std::sync::OnceLock;

use crate::config::Casino;
use crate::pixels::{Canvas, H, W, hash, mix, rgb, scale, smooth};

type Rgb = [f32; 3];

/// Is the underground look on?
pub fn on(cfg: &Casino) -> bool {
    cfg.theme.eq_ignore_ascii_case("underground")
}

pub const ACID: [u8; 3] = [90, 255, 120];
pub const CYAN: [u8; 3] = [40, 230, 255];
pub const PINK: [u8; 3] = [255, 50, 200];

fn c(k: [u8; 3]) -> Rgb {
    rgb(k[0], k[1], k[2])
}

/// The label of the place picked: black on acid green.
pub fn pick_colors() -> (Rgb, Rgb) {
    (rgb(0, 0, 0), rgb(60, 220, 110))
}

/// The labels not picked.
pub fn label_color() -> Rgb {
    rgb(120, 200, 160)
}

// ---- sprites ---------------------------------------------------------------

/// Skull and crossbones: S skull, B bone, '.' nothing.
const JOLLY: [&str; 9] = [
    "B.........B",
    ".B.SSSSS.B.",
    "..SSSSSSS..",
    "..S..S..S..",
    "..SSS.SSS..",
    "..BSSSSSB..",
    ".B.S.S.S.B.",
    "B...SSS...B",
    "...........",
];

/// A small one, for beside the sign.
const JOLLY_SMALL: [&str; 8] = ["B.....B", ".BSSSB.", "SSSSSSS", "S..S..S", "SSS.SSS", ".SSSSS.", ".BS.SB.", "B.....B"];

/// A 3.5" floppy: B body, S shutter, L label, H the hole.
const FLOPPY: [&str; 8] = ["BBSSSSBB", "BBSHSSBB", "BBSSSSBB", "BBBBBBBB", "BLLLLLLB", "BLPPPPLB", "BLLLLLLB", "BBBBBBBB"];

/// A virus: V its body, T the tips of its spikes, E its eye.
const VIRUS: [&str; 9] = [
    "T...T...T",
    ".V..V..V.",
    "..VVVVV..",
    "..VVEVV..",
    "TVVEEEVVT",
    "..VVEVV..",
    "..VVVVV..",
    ".V..V..V.",
    "T...T...T",
];

/// A small one, crawling along the tube.
const VIRUS_SMALL: [&str; 5] = ["T.T.T", ".VVV.", "TVEVT", ".VVV.", "T.T.T"];

/// The slot machine's reels: a skull, a virus, a floppy (K skull, V
/// virus, B floppy, L its label).
const REELS: [[&str; 5]; 3] = [
    [".KK.", "KKKK", "K.K.", "KKKK", ".KK."],
    ["V..V", ".VV.", "VVVV", ".VV.", "V..V"],
    ["BBBB", "BLLB", "BBBB", "BLLB", "BBBB"],
];

/// Put a sprite at (`x`, `y`), each letter's color from `ink`; with a
/// glow of `glow` (0: none) around what's lit.
fn sprite(cv: &mut Canvas, rows: &[&str], x: i32, y: i32, glow: f32, ink: impl Fn(char) -> Option<Rgb>) {
    if glow > 0.0 {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, ch) in row.chars().enumerate() {
                let Some(k) = ink(ch) else { continue };
                for gy in -2..=2i32 {
                    for gx in -2..=2i32 {
                        let d = ((gx * gx + gy * gy) as f32).sqrt();
                        if d > 0.0 {
                            cv.blend(x + dx as i32 + gx, y + dy as i32 + gy, k, glow / (d * d));
                        }
                    }
                }
            }
        }
    }
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if let Some(k) = ink(ch) {
                cv.set(x + dx as i32, y + dy as i32, k);
            }
        }
    }
}

/// A neon skull and crossbones, `lit` 0 (off) to 1; `small`, the one
/// seven pixels wide.
pub fn jolly(cv: &mut Canvas, x: i32, y: i32, lit: f32, small: bool) {
    let (skull, bone) = (mix(scale(c(PINK), 0.25), c(PINK), lit), mix(scale(c(CYAN), 0.25), c(CYAN), lit));
    let rows: &[&str] = if small { &JOLLY_SMALL } else { &JOLLY };
    sprite(cv, rows, x, y, 0.06 * lit, |ch| match ch {
        'S' => Some(skull),
        'B' => Some(bone),
        _ => None,
    });
}

pub fn floppy(cv: &mut Canvas, x: i32, y: i32, body: Rgb, lit: f32) {
    sprite(cv, &FLOPPY, x, y, 0.035 * lit, |ch| match ch {
        'B' => Some(body),
        'S' => Some(rgb(170, 175, 190)),
        'H' => Some(rgb(40, 40, 50)),
        'L' => Some(rgb(235, 235, 225)),
        'P' => Some(scale(c(PINK), 0.9)),
        _ => None,
    });
}

/// A virus, its spikes' tips pulsing with `beat` (0 to 1).
pub fn virus(cv: &mut Canvas, x: i32, y: i32, beat: f32) {
    let body = mix(rgb(40, 160, 60), c(ACID), beat);
    sprite(cv, &VIRUS, x, y, 0.04 + 0.04 * beat, |ch| match ch {
        'V' => Some(body),
        'T' => Some(mix(c(ACID), rgb(255, 255, 200), beat)),
        'E' => Some(rgb(255, 40, 90)),
        _ => None,
    });
}

/// The skull and crossbones printed big on a table's felt, as a casino
/// prints its name: `size` pixels a sprite pixel, faint (`alpha`).
pub fn felt_logo(cv: &mut Canvas, cx: f32, cy: f32, size: i32, alpha: f32) {
    let (w, h) = (JOLLY[0].len() as i32 * size, JOLLY.len() as i32 * size);
    let (x0, y0) = (cx as i32 - w / 2, cy as i32 - h / 2);
    for (dy, row) in JOLLY.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            let k = match ch {
                'S' => c(PINK),
                'B' => c(CYAN),
                _ => continue,
            };
            for py in 0..size {
                for px in 0..size {
                    cv.blend(x0 + dx as i32 * size + px, y0 + dy as i32 * size + py, k, alpha);
                }
            }
        }
    }
}

// ---- the lobby ---------------------------------------------------------------

/// The wall (a circuit board in the dark) and the floor (a neon grid
/// running to the back wall).
pub fn lobby_room() -> &'static Canvas {
    static ROOM: OnceLock<Canvas> = OnceLock::new();
    ROOM.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let k = if y < 33 {
                    let base = mix(rgb(8, 10, 16), rgb(3, 4, 8), y as f32 / 33.0);
                    // Traces: runs along every fourth row and column, here
                    // and there, and a via where they meet.
                    let (row, col) = (y.rem_euclid(4) == 1, x.rem_euclid(6) == 2);
                    let run_h = row && hash(x / 9, y / 4) > 0.6;
                    let run_v = col && hash(x / 6 + 50, y / 5) > 0.7;
                    let via = row && col && hash(x / 6, y / 4 + 7) > 0.75;
                    if via {
                        mix(base, rgb(50, 120, 80), 0.4)
                    } else if run_h || run_v {
                        mix(base, rgb(20, 70, 45), 0.3)
                    } else {
                        base
                    }
                } else {
                    // Lines across, closer together toward the back, and lines
                    // running to a point behind the wall.
                    let depth = (y - 32) as f32;
                    let across = [34.0, 36.0, 39.0, 43.0].iter().any(|&r| (y as f32 - r).abs() < 0.5);
                    let u = (x as f32 + 0.5 - 40.0) / (depth * 1.6);
                    let along = (u - u.round()).abs() < 0.5 / (depth * 1.6) * 1.2;
                    let near = smooth(0.0, 11.0, depth);
                    if across || along {
                        mix(rgb(40, 6, 40), c(PINK), 0.1 + 0.35 * near)
                    } else {
                        mix(rgb(6, 2, 10), rgb(16, 4, 22), near)
                    }
                };
                cv.set(x, y, k);
            }
        }
        cv
    })
}

/// What the lobby has over the room: the sign, the neon on the wall.
pub fn draw_lobby(cv: &mut Canvas, t: f32, sign: &[(i32, i32, usize)]) {
    // The sign: acid green tubes in a cyan haze, a letter that stutters.
    let beat = (t * 7.0) as i32;
    let flicker = hash(beat, 11) < 0.1;
    let stutter = (hash(beat / 20, 3) * 6.0) as usize;
    for &(x, y, letter) in sign {
        if flicker && letter == stutter {
            continue;
        }
        for dy in -2..=2 {
            for dx in -2..=2 {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if d > 0.0 {
                    cv.blend(x + dx, y + dy, c(CYAN), 0.05 / d);
                }
            }
        }
    }
    for &(x, y, letter) in sign {
        let off = flicker && letter == stutter;
        let k = if off { rgb(20, 60, 30) } else { mix(c(ACID), rgb(230, 255, 235), 0.3 + 0.2 * (t * 3.0 + x as f32 * 0.2).sin()) };
        cv.set(x, y, k);
    }
    // Under it, a cyan tube with data running along it.
    for x in 4..W as i32 - 4 {
        let pulse = ((x as f32 * 0.35 - t * 9.0).sin() * 0.5 + 0.5).powi(6);
        cv.set(x, 17, mix(rgb(10, 80, 100), rgb(220, 255, 255), pulse));
        cv.blend(x, 16, c(CYAN), 0.12 + 0.3 * pulse);
        cv.blend(x, 18, c(CYAN), 0.12 + 0.3 * pulse);
    }
    // Neon on the wall: skulls and crossbones that buzz, floppies, and
    // viruses with their spikes going.
    let buzz = |k: i32| if hash((t * 5.0) as i32, k) < 0.06 { 0.3 } else { 1.0 };
    jolly(cv, 0, 5, buzz(1), true);
    jolly(cv, 73, 5, buzz(2), true);
    jolly(cv, 57, 16, buzz(3), false);
    let pulse = 0.5 + 0.5 * (t * 4.0).sin();
    floppy(cv, 70, 17, rgb(40, 70, 210), 1.0);
    // A virus crawls the tube, end to end, and round again.
    let x = 4 + (t * 5.0) as i32 % (W as i32 - 13);
    let body = mix(rgb(40, 160, 60), c(ACID), pulse);
    sprite(cv, &VIRUS_SMALL, x, 15 + ((t * 6.0).sin() * 0.6).round() as i32, 0.05, |ch| match ch {
        'V' => Some(body),
        'T' => Some(mix(c(ACID), rgb(255, 255, 200), pulse)),
        'E' => Some(rgb(255, 40, 90)),
        _ => None,
    });
}

/// A card's back in green: its reds and greens traded.
pub fn green_back(img: Vec<Option<Rgb>>) -> Vec<Option<Rgb>> {
    img.into_iter().map(|p| p.map(|k| if k[0] > k[1] + 40.0 { [k[1], k[0], k[2] * 0.8] } else { k })).collect()
}

/// Felt for a table: deep purple, darker at its edge, `d` 0 (middle) to 1.
pub fn felt(d: f32) -> Rgb {
    mix(rgb(55, 20, 85), rgb(18, 6, 32), smooth(0.0, 1.0, d))
}

/// A table's rail: a cyan neon tube.
pub fn rail(v: f32) -> Rgb {
    mix(c(CYAN), rgb(10, 90, 110), v.clamp(0.0, 1.0))
}

/// A slot machine: black, edged in acid green, three skulls on its reels.
pub fn slot_machine(cv: &mut Canvas, cx: f32, t: f32) {
    let x0 = cx as i32 - 8;
    let lamp = if (t * 2.0) as i32 % 2 == 0 { c(PINK) } else { rgb(90, 20, 70) };
    for x in x0 + 4..x0 + 13 {
        cv.set(x, 21, lamp);
    }
    for y in 22..39 {
        for x in x0..x0 + 17 {
            let edge = x == x0 || x == x0 + 16 || y == 22;
            cv.set(x, y, if edge { c(ACID) } else { scale(rgb(22, 24, 30), 1.0 - (x - x0) as f32 / 40.0) });
        }
    }
    for y in 24..32 {
        for x in x0 + 1..x0 + 16 {
            let edge = ((y as f32 - 27.5) / 4.0).abs();
            cv.set(x, y, mix(rgb(8, 30, 14), rgb(2, 8, 4), edge * edge));
        }
    }
    for (k, rows) in REELS.iter().enumerate() {
        sprite(cv, rows, x0 + 2 + k as i32 * 5, 25, 0.0, |ch| match ch {
            'K' => Some(rgb(240, 240, 250)),
            'V' => Some(c(ACID)),
            'B' => Some(rgb(60, 100, 255)),
            'L' => Some(rgb(235, 235, 225)),
            _ => None,
        });
    }
    cv.rect(x0 + 2, 34, 13, 2, rgb(10, 40, 20));
    for y in 25..32 {
        cv.set(x0 + 18, y, rgb(150, 150, 165));
    }
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        cv.set(x0 + 17 + dx, 23 + dy, c(PINK));
    }
}

/// The video poker cabinet: a pink marquee, a green CRT.
pub fn poker_machine(cv: &mut Canvas, cx: f32, t: f32) {
    let x0 = cx as i32 - 3;
    cv.rect(x0, 21, 7, 17, rgb(20, 20, 28));
    let lamp = if (t * 2.0 + 1.0) as i32 % 2 == 0 { c(PINK) } else { rgb(90, 20, 70) };
    cv.rect(x0 + 1, 21, 5, 1, lamp);
    cv.rect(x0 + 1, 23, 5, 7, rgb(4, 30, 10));
    for k in 0..5 {
        cv.set(x0 + 1 + k, 24, c(ACID));
        let card = if k % 2 == 1 { rgb(60, 200, 90) } else { rgb(170, 255, 190) };
        cv.set(x0 + 1 + k, 26, card);
        cv.set(x0 + 1 + k, 27, card);
    }
    for k in 0..5 {
        cv.set(x0 + 1 + k, 32, if k == 4 { c(PINK) } else { c(CYAN) });
    }
    cv.rect(x0 + 1, 34, 5, 4, rgb(12, 12, 18));
}

/// A canvas as an ANSI file, for the rooms' picture tests.
#[cfg(test)]
pub fn write_picture(cv: &Canvas, path: &str) {
    let mut enc = crate::pixels::Encoder::new(true, cv.w, cv.h);
    let mut bytes = b"\x1b[2J".to_vec();
    bytes.extend(enc.frame(cv, 1));
    bytes.extend(b"\x1b[0m\x1b[24;1H");
    std::fs::write(path, bytes).unwrap();
}
