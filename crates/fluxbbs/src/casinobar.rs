//! New: the casino's bar and players club (`casino.rs`). The bar has a
//! back bar of bottles under a neon sign and a bartender: a drink
//! (poured into its own glass, on the house), a cigarette (lit, its
//! smoke curling up), or a word with the bartender, who knows what's
//! been happening on the board. A few drinks and the room starts to
//! sway. At the players club, chips played today earn comps: the buffet
//! (minutes on today's time), a suite (chips) and show tickets (file
//! points), once a day each, at the thresholds in `[casino]`.

use std::f32::consts::PI;
use std::io;
use std::sync::OnceLock;

use rand::RngExt;

use crate::casino::{H, Ink, MSG_ROW, Purse, STATUS_ROW, Stage, W, ink, painted, shade};
use crate::config::Casino;
use crate::input::Key;
use crate::pixels::{Canvas, Rgb, hash, mix, rgb, scale, smooth};
use crate::session::Session;

/// A figure drawn by hand: `s` skin, `k` hair and eyes, `w` shirt, `R`
/// the tie, `v` the jacket or vest.
fn figure(cv: &mut Canvas, rows: &[&str], x0: i32, y0: i32, coat: Rgb, tie: Rgb, blink: bool) {
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let c = match ch {
                's' => rgb(235, 190, 150),
                'S' => rgb(200, 150, 115),
                'k' => rgb(30, 22, 20),
                'e' => if blink { rgb(235, 190, 150) } else { rgb(30, 22, 20) },
                'w' => rgb(240, 240, 235),
                'R' => tie,
                'v' => coat,
                'V' => scale(coat, 0.7),
                _ => continue,
            };
            cv.set(x0 + x as i32, y0 + y as i32, c);
        }
    }
}

const BARTENDER: [&str; 18] = [
    "....kkkkk....",
    "...kkkkkkk...",
    "...kssssskk..",
    "...sesssess..",
    "...sssSsss...",
    "....skkks....",
    ".....sss.....",
    "..wwwwRwwww..",
    ".vvwwRRRwwvv.",
    "vvvvwwRwwvvvv",
    "vvvvvwwwvvvvv",
    "vvVvvvwvvvVvv",
    "vvVvvvvvvvVvv",
    "svVvvvvvvvVvs",
    "svvvvvvvvvvvs",
    "ss.vvvvvvv.ss",
    "...vvvvvvv...",
    "...vvvvvvv...",
];

// ---- the bar ---------------------------------------------------------------

/// The drinks: name, the glass, the drink's color, and what's in it.
#[derive(Clone, Copy, PartialEq)]
enum Glass {
    Martini,
    Rocks,
    Pint,
    Flute,
}

struct Drink {
    name: &'static str,
    glass: Glass,
    color: [u8; 3],
    /// 0 none, 1 olive, 2 cherry, 3 lime, 4 ice.
    garnish: u8,
    bubbles: bool,
}

const DRINKS: [Drink; 6] = [
    Drink { name: "martini", glass: Glass::Martini, color: [225, 235, 240], garnish: 1, bubbles: false },
    Drink { name: "old fashioned", glass: Glass::Rocks, color: [190, 105, 35], garnish: 2, bubbles: false },
    Drink { name: "cosmopolitan", glass: Glass::Martini, color: [240, 80, 130], garnish: 3, bubbles: false },
    Drink { name: "cold beer", glass: Glass::Pint, color: [235, 170, 40], garnish: 0, bubbles: true },
    Drink { name: "champagne", glass: Glass::Flute, color: [245, 225, 150], garnish: 0, bubbles: true },
    Drink { name: "whiskey, rocks", glass: Glass::Rocks, color: [175, 95, 25], garnish: 4, bubbles: false },
];

/// A glass's half-width `h` pixels up from its foot (none: the stem),
/// its height, and from where up it holds the drink.
fn glass_shape(g: Glass) -> (fn(f32) -> Option<f32>, i32, f32) {
    match g {
        Glass::Martini => (|h| if h < 1.0 { Some(2.5) } else if h < 4.0 { None } else { Some((h - 4.0) / 5.0 * 4.5 + 0.3) }, 10, 4.0),
        Glass::Rocks => (|_| Some(3.5), 7, 1.0),
        Glass::Pint => (|h| Some(2.5 + h / 12.0), 12, 1.0),
        Glass::Flute => (|h| if h < 1.0 { Some(2.0) } else if h < 4.0 { None } else { Some(1.6) }, 13, 4.0),
    }
}

/// A glass at (`cx`, `foot`), filled `level` of the way (0 to 1).
fn draw_glass(cv: &mut Canvas, d: &Drink, cx: f32, foot: i32, level: f32, t: f32, garnish: bool) {
    let (half, height, from) = glass_shape(d.glass);
    let drink = rgb(d.color[0], d.color[1], d.color[2]);
    let top = from + (height as f32 - from) * level;
    for h in 0..height {
        let y = foot - h;
        let hf = h as f32 + 0.5;
        match half(hf) {
            None => cv.set(cx.round() as i32, y, rgb(210, 220, 230)),
            Some(w) => {
                let (l, r) = ((cx - w).round() as i32, (cx + w).round() as i32);
                for x in l..=r {
                    let edge = x == l || x == r;
                    let inside = hf >= from && hf < top && !edge;
                    let c = if edge {
                        rgb(200, 215, 230)
                    } else if inside {
                        // The drink, darker at the foot, a shine up one side.
                        let mut c = mix(scale(drink, 0.7), drink, (hf - from) / (height as f32 - from).max(1.0));
                        if x == l + 1 {
                            c = mix(c, rgb(255, 255, 255), 0.35);
                        }
                        if d.glass == Glass::Pint && hf >= top - 1.5 && level > 0.95 {
                            c = rgb(250, 248, 240); // the head
                        }
                        c
                    } else {
                        // Empty glass: a hint of what's behind it, lighter.
                        let behind = cv.get(x, y).unwrap_or(rgb(0, 0, 0));
                        mix(behind, rgb(200, 220, 240), 0.18)
                    };
                    cv.set(x, y, c);
                }
            }
        }
    }
    if d.bubbles && level > 0.3 {
        for i in 0..5 {
            let rise = ((t * 0.9 + i as f32 * 0.37) % 1.0) * (top - from - 1.0);
            let wiggle = ((t * 5.0 + i as f32 * 2.1).sin() * 0.8).round();
            let x = cx.round() as i32 + wiggle as i32 + (i % 3) - 1;
            let y = foot - (from + 0.5 + rise) as i32;
            cv.blend(x, y, rgb(255, 255, 240), 0.6);
        }
    }
    if !garnish {
        return;
    }
    let rim = foot - height + 1;
    match d.garnish {
        1 => {
            // An olive on a pick.
            for k in 0..4 {
                cv.set(cx.round() as i32 + 2 - k / 2, rim - 2 + k, rgb(170, 130, 70));
            }
            cv.set(cx.round() as i32, rim + 1, rgb(110, 140, 40));
            cv.set(cx.round() as i32 + 1, rim + 1, rgb(90, 120, 30));
            cv.set(cx.round() as i32, rim + 2, rgb(200, 40, 40));
        }
        2 => {
            cv.set(cx.round() as i32 + 1, rim + 1, rgb(200, 20, 40));
            cv.set(cx.round() as i32 + 2, rim, rgb(110, 60, 20));
            cv.set(cx.round() as i32 - 2, rim + 1, rgb(255, 150, 40));
        }
        3 => {
            cv.set(cx.round() as i32 + 4, rim, rgb(120, 200, 60));
            cv.set(cx.round() as i32 + 5, rim, rgb(80, 160, 40));
            cv.set(cx.round() as i32 + 4, rim + 1, rgb(160, 230, 90));
        }
        4 => {
            for (dx, dy) in [(-1, 3), (1, 2)] {
                cv.set(cx.round() as i32 + dx, rim + dy, rgb(225, 240, 250));
                cv.set(cx.round() as i32 + dx + 1, rim + dy, rgb(190, 215, 235));
            }
        }
        _ => {}
    }
}

/// The room behind the bar: paneling, the mirror, the shelves of
/// bottles, the counter and the stools.
fn bar_room() -> &'static Canvas {
    static ROOM: OnceLock<Canvas> = OnceLock::new();
    ROOM.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let c = if y < 30 {
                    // Dark wood planks, the mirror behind the bottles.
                    let plank = mix(rgb(70, 36, 20), rgb(45, 22, 12), hash(x / 4, 0));
                    let grain = 0.92 + 0.12 * hash(x, y / 3);
                    let mut c = scale(plank, grain);
                    if (9..23).contains(&y) && !(28..53).contains(&x) {
                        let streak = (((x + y) as f32) * 0.25).sin() * 0.5 + 0.5;
                        c = mix(rgb(25, 30, 45), rgb(70, 80, 105), 0.25 + 0.25 * streak * (1.0 - (y - 9) as f32 / 14.0));
                    }
                    if (9..23).contains(&y) && (28..53).contains(&x) {
                        c = mix(rgb(25, 30, 45), rgb(60, 70, 95), 0.3);
                    }
                    if x % 4 == 0 && y >= 23 {
                        c = scale(c, 0.8);
                    }
                    c
                } else if y < 33 {
                    // The counter top, polished.
                    let shine = if y == 30 { 0.45 } else { 0.0 };
                    mix(mix(rgb(120, 55, 25), rgb(90, 40, 18), (y - 30) as f32 / 3.0), rgb(255, 220, 180), shine)
                } else {
                    let panel = (x % 10 == 0) || y == 34 || y == 42;
                    let c = mix(rgb(80, 35, 15), rgb(40, 16, 6), (y - 33) as f32 / 11.0);
                    if panel { scale(c, 0.7) } else { c }
                };
                cv.set(x, y, c);
            }
        }
        // Brass foot rail.
        for x in 0..W as i32 {
            cv.set(x, 40, rgb(210, 170, 70));
        }
        // Shelves and bottles.
        for shelf in [15, 22] {
            for x in 2..W as i32 - 2 {
                cv.set(x, shelf, rgb(150, 95, 50));
                cv.set(x, shelf + 1, rgb(60, 30, 15));
            }
            let mut x = 3;
            let mut k = shelf;
            while x < W as i32 - 4 {
                if (31..49).contains(&x) {
                    x = 49;
                    continue;
                }
                let colors = [rgb(40, 120, 50), rgb(170, 90, 20), rgb(180, 210, 220), rgb(150, 20, 30), rgb(40, 60, 150), rgb(220, 190, 80)];
                let c = colors[(hash(x, k) * 6.0) as usize % 6];
                let tall = 4 + (hash(x, k + 1) * 3.0) as i32;
                let wide = 2 + (hash(x, k + 2) * 2.0) as i32;
                for yy in shelf - tall..shelf {
                    for xx in x..x + wide {
                        let mut cc = c;
                        if xx == x {
                            cc = mix(c, rgb(255, 255, 255), 0.45);
                        }
                        if yy == shelf - tall / 2 {
                            cc = rgb(235, 225, 200);
                        }
                        cv.set(xx, yy, cc);
                    }
                }
                let neck = x + wide / 2;
                for yy in shelf - tall - 2..shelf - tall {
                    cv.set(neck, yy, scale(c, 0.8));
                }
                cv.set(neck, shelf - tall - 3, rgb(200, 170, 90));
                x += wide + 1 + (hash(x, k + 3) * 2.0) as i32;
                k += 1;
            }
        }
        // Stools in front.
        for sx in [10, 70] {
            for y in 36..44 {
                cv.set(sx, y, rgb(180, 180, 190));
            }
            for dx in -3i32..=3 {
                cv.set(sx + dx, 35, rgb(170, 25, 40));
                if dx.abs() < 3 {
                    cv.set(sx + dx, 34, rgb(210, 50, 60));
                }
            }
        }
        // The ashtray.
        for x in 12..18 {
            cv.set(x, 30, rgb(120, 120, 130));
            cv.set(x, 29, if x == 12 || x == 17 { rgb(150, 150, 160) } else { rgb(60, 60, 65) });
        }
        cv.clear_labels();
        cv
    })
}

/// What's going on at the bar.
#[derive(Default)]
struct Bar {
    /// The drink in front of you, when it was ordered.
    drink: Option<(usize, f32)>,
    /// When the cigarette was lit.
    smoke: Option<f32>,
    /// Drinks today: the room sways after a few.
    drinks: u32,
}

fn draw_bar(cv: &mut Canvas, t: f32, bar: &Bar, names: &[String]) {
    cv.px.clone_from(&bar_room().px);
    cv.clear_labels();
    // The neon sign.
    let flick = hash((t * 6.0) as i32, 3) < 0.05;
    let x0 = (W as i32 - crate::pixels::text_width("BAR")) / 2;
    let mut sign = Canvas::new(W, 8);
    sign.print(x0, 0, "BAR", 1, |_, _| rgb(255, 255, 255));
    let lit = |x: i32, y: i32| sign.px[(y * W as i32 + x) as usize][0] > 0.0;
    for y in 0..7 {
        for x in 0..W as i32 {
            if lit(x, y) && !flick {
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    cv.blend(x + dx, y + dy + 1, rgb(80, 220, 255), 0.18);
                }
            }
        }
    }
    for y in 0..7 {
        for x in 0..W as i32 {
            if lit(x, y) {
                let c = if flick { rgb(30, 70, 90) } else { mix(rgb(120, 235, 255), rgb(235, 255, 255), 0.4 + 0.2 * (t * 2.0).sin()) };
                cv.set(x, y + 1, c);
            }
        }
    }
    // The menu, chalk on slate.
    for y in 1..13 {
        for x in 2..25 {
            let frame = x == 2 || x == 24 || y == 1 || y == 12;
            cv.set(x, y, if frame { rgb(120, 70, 30) } else { mix(rgb(35, 40, 38), rgb(25, 28, 27), hash(x, y)) });
        }
    }
    cv.label(4, 1, "on the house", rgb(250, 230, 150), None);
    for (i, n) in names.iter().enumerate().take(4) {
        cv.label(3, 2 + i, &format!("{} {n}", i + 1), rgb(230, 230, 220), None);
    }
    for (i, n) in names.iter().enumerate().skip(4) {
        cv.label(57, i - 2, &format!("{} {n}", i + 1), rgb(230, 230, 220), None);
    }
    // The bartender, blinking now and then.
    let blink = (t % 4.0) < 0.15;
    figure(cv, &BARTENDER, 34, 12, rgb(30, 30, 40), rgb(200, 20, 40), blink);
    // His towel, wiping the counter while nobody's drinking.
    if bar.drink.is_none() {
        let tx = 36.0 + 4.0 * (t * 2.2).sin();
        for dx in 0..3 {
            cv.set(tx as i32 + dx, 29, rgb(245, 245, 245));
        }
    }
    if let Some((i, at)) = bar.drink {
        let d = &DRINKS[i];
        let age = t - at;
        // Set down by the bartender, slid along to you, then poured.
        let slide = smooth(0.0, 1.0, (age - 0.2) / 0.7);
        let x = 42.0 + 14.0 * slide;
        let pour = ((age - 1.1) / 1.4).clamp(0.0, 1.0);
        if (1.1..2.5).contains(&age) {
            let (_, height, _) = glass_shape(d.glass);
            for y in 18..(31 - height) {
                cv.set(x.round() as i32, y, rgb(d.color[0], d.color[1], d.color[2]));
            }
        }
        draw_glass(cv, d, x, 31, pour, t, age > 2.6);
    }
    if let Some(lit) = bar.smoke {
        draw_smoke(cv, t - lit, t);
    }
    // A few drinks in, the room starts to move.
    if bar.drinks >= 3 {
        let amount = (bar.drinks - 2).min(4) as f32 * 0.5;
        let src = cv.px.clone();
        for y in 0..H {
            let off = ((t * 1.6 + y as f32 * 0.18).sin() * amount).round() as i32;
            for x in 0..W as i32 {
                let sx = (x - off).clamp(0, W as i32 - 1);
                cv.px[y * W + x as usize] = src[y * W + sx as usize];
            }
        }
    }
}

/// A cigarette in the ashtray, `age` seconds after it was lit: the
/// lighter's flame, then the ember and the smoke going up.
fn draw_smoke(cv: &mut Canvas, age: f32, t: f32) {
    let burnt = (age / 30.0).min(0.7);
    let len = 6 - (burnt * 6.0) as i32;
    let (x0, y) = (13, 28);
    for x in x0..x0 + len {
        cv.set(x, y, if x < x0 + 2 { rgb(220, 160, 90) } else { rgb(245, 245, 240) });
    }
    let tip = x0 + len;
    if age < 1.2 {
        // The lighter: a flick, a spark, a flame.
        let flame = age > 0.3;
        cv.rect(tip + 2, y - 1, 2, 3, rgb(150, 150, 160));
        if flame {
            let h = 2 + ((t * 20.0).sin() > 0.0) as i32;
            for k in 0..h {
                cv.set(tip + 2, y - 2 - k, if k == 0 { rgb(255, 240, 150) } else { rgb(255, 150, 40) });
            }
        } else if age > 0.1 {
            cv.set(tip + 3, y - 3, rgb(255, 255, 200));
        }
    }
    if age > 0.6 {
        let glow = 0.6 + 0.4 * (t * 3.0).sin().abs();
        cv.set(tip, y, mix(rgb(120, 30, 10), rgb(255, 120, 30), glow));
        // Smoke: puffs rising and spreading, thinning as they go.
        for i in 0..28 {
            let born = (i as f32) * 0.32;
            let life = (age - 0.6 - born).rem_euclid(9.0);
            if age - 0.6 < born {
                continue;
            }
            let rise = life * 2.6;
            let drift = (life * 1.3 + i as f32).sin() * (1.0 + life * 0.6) + life * 0.8;
            let (sx, sy) = (tip as f32 + drift, y as f32 - 1.0 - rise);
            let a = (0.8 - life * 0.08).max(0.0);
            if a <= 0.0 || sy < 0.0 {
                continue;
            }
            let size = 0.9 + life * 0.3;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d <= size {
                        cv.blend((sx + dx as f32).round() as i32, (sy + dy as f32).round() as i32, rgb(200, 200, 210), a * (1.0 - d / (size + 0.5)));
                    }
                }
            }
        }
    }
}

// ---- the players club and its comps ---------------------------------------------

const HOST: [&str; 18] = [
    "....kkkkk....",
    "...kkkkkkkk..",
    "...kssssskk..",
    "...sesssess..",
    "...sssSsss...",
    "....sskss....",
    ".....sss.....",
    "..vvwwRwwvv..",
    ".vvvvwRwvvvv.",
    "vvvvvwRwvvvvv",
    "vvvvvwRwvvvvv",
    "vvVvvvwvvvVvv",
    "vvVvvvvvvvVvv",
    "svVvvvvvvvVvs",
    "svvvvvvvvvvvs",
    "ss.vvvvvvv.ss",
    "...vvvvvvv...",
    "...vvvvvvv...",
];

fn club_room() -> &'static Canvas {
    static ROOM: OnceLock<Canvas> = OnceLock::new();
    ROOM.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let c = if y < 30 {
                    // Blue velvet, a gold rail.
                    let fold = 0.85 + 0.15 * ((x as f32) * 0.8).sin();
                    let c = scale(mix(rgb(30, 35, 95), rgb(12, 14, 45), y as f32 / 30.0), fold);
                    if y == 11 || y == 29 { rgb(210, 170, 70) } else { c }
                } else {
                    // A marble desk.
                    let vein = ((x as f32 * 0.3 + (y as f32 * 0.7).sin() * 3.0).sin() * 0.5 + 0.5).powi(8);
                    let c = mix(rgb(235, 232, 225), rgb(150, 150, 160), vein * 0.6 + (y - 30) as f32 / 30.0);
                    if y == 30 { rgb(225, 185, 80) } else { c }
                };
                cv.set(x, y, c);
            }
        }
        let x0 = (W as i32 - crate::pixels::text_width("PLAYERS CLUB")) / 2;
        cv.print(x0, 2, "PLAYERS CLUB", 1, |u, v| mix(rgb(255, 240, 170), rgb(200, 140, 40), v * 0.8 + u * 0.1));
        // The rope and its posts.
        for px in [3, 76] {
            for y in 20..30 {
                cv.set(px, y, rgb(220, 180, 80));
            }
            cv.set(px, 19, rgb(255, 230, 140));
        }
        for x in 3..77 {
            if (12..68).contains(&x) {
                continue;
            }
            let sag = if x < 40 { ((x - 3) as f32 / 9.0 * PI).sin() } else { ((x - 68) as f32 / 8.0 * PI).sin() };
            cv.set(x, 21 + (sag * 3.0) as i32, rgb(170, 20, 40));
        }
        cv.clear_labels();
        cv
    })
}

/// The comps: name, the purse's bit for it, the chips to play first.
fn comps(cfg: &Casino) -> [(&'static str, u8, u32); 3] {
    [("the buffet", 1, cfg.comp_buffet), ("a suite upstairs", 2, cfg.comp_suite), ("show tickets", 4, cfg.comp_show)]
}

fn draw_club(cv: &mut Canvas, t: f32, cfg: &Casino, purse: &Purse) {
    cv.px.clone_from(&club_room().px);
    cv.clear_labels();
    let blink = (t % 5.0) < 0.15;
    figure(cv, &HOST, 34, 12, rgb(20, 20, 30), rgb(225, 180, 60), blink);
    let gold = rgb(255, 225, 140);
    cv.label(2, 7, &format!("played today: {} chips", purse.played), rgb(230, 230, 240), None);
    cv.label(52, 7, "drinks: always comped", rgb(230, 230, 240), None);
    for (i, (name, bit, need)) in comps(cfg).into_iter().enumerate() {
        let row = 16 + i * 2;
        let had = purse.comps & bit != 0;
        let ready = purse.played >= need;
        let fill = ((purse.played as f32 / need.max(1) as f32).min(1.0) * 20.0) as usize;
        let bar: String = "\u{2588}".repeat(fill) + &"\u{2591}".repeat(20 - fill);
        let (state, color) = match (had, ready) {
            (true, _) => ("enjoyed".to_string(), rgb(150, 150, 160)),
            (false, true) => ("ready!".to_string(), rgb(120, 255, 140)),
            (false, false) => (format!("{}/{need}", purse.played), rgb(200, 200, 210)),
        };
        let panel = Some(rgb(18, 20, 55));
        cv.label(3, row, &format!(" {} {name:<20}", i + 1), gold, panel);
        cv.label(26, row, &bar, if ready { rgb(120, 230, 130) } else { rgb(200, 170, 90) }, panel);
        cv.label(46, row, &format!("  {state:<12}"), color, panel);
    }
}

/// The buffet: the cover comes off a plate, steam rising.
fn draw_buffet(cv: &mut Canvas, age: f32) {
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let c = if y < 18 { mix(rgb(60, 30, 20), rgb(30, 14, 10), y as f32 / 18.0) } else { mix(rgb(245, 243, 238), rgb(200, 198, 195), (y - 18) as f32 / 26.0) };
            cv.set(x, y, c);
        }
    }
    let (cx, cy) = (40.0, 30.0);
    for y in 18..44 {
        for x in 10..70 {
            let (u, v) = ((x as f32 - cx) / 26.0, (y as f32 - cy) / 10.0);
            let d = u * u + v * v;
            if d <= 1.0 {
                cv.set(x, y, if d > 0.75 { rgb(225, 225, 230) } else { rgb(250, 250, 252) });
            }
        }
    }
    // A steak, potatoes, greens.
    for y in 25..34 {
        for x in 26..44 {
            let (u, v) = ((x as f32 - 35.0) / 9.0, (y as f32 - 29.5) / 4.5);
            if u * u + v * v <= 1.0 {
                let grill = (x + y) % 4 == 0;
                cv.set(x, y, if grill { rgb(60, 25, 10) } else { mix(rgb(150, 70, 35), rgb(110, 50, 25), v * 0.5 + 0.5) });
            }
        }
    }
    for (px, py) in [(47, 27), (51, 28), (49, 31)] {
        for dy in 0..3 {
            for dx in 0..4 {
                cv.set(px + dx, py + dy, if dy == 0 { rgb(255, 235, 140) } else { rgb(225, 190, 90) });
            }
        }
    }
    for k in 0..14 {
        let (x, y) = (52 + (k * 7) % 9, 32 + (k * 3) % 4);
        cv.set(x, y, if k % 2 == 0 { rgb(60, 150, 50) } else { rgb(100, 190, 70) });
    }
    // Steam, once the cover's off.
    if age > 1.0 {
        for i in 0..18 {
            let life = (age - 1.0 + i as f32 * 0.4) % 4.0;
            let x = 30.0 + (i * 5 % 22) as f32 + (life * 2.0 + i as f32).sin() * 1.5;
            let y = 25.0 - life * 5.0;
            cv.blend(x.round() as i32, y.round() as i32, rgb(255, 255, 255), (0.45 - life * 0.1).max(0.0));
        }
    }
    // The cover: up and away.
    let lift = smooth(0.2, 1.2, age) * 30.0;
    for y in 14..31 {
        for x in 14..66 {
            let (u, v) = ((x as f32 - cx) / 25.0, (y as f32 - 30.0) / 16.0);
            if u * u + v * v <= 1.0 && v <= 0.0 {
                let shine = (-(u + 0.4).powi(2) * 12.0).exp();
                cv.set(x, y - lift as i32, mix(rgb(170, 175, 185), rgb(255, 255, 255), shine * 0.8));
            }
        }
    }
}

/// The suite: the city at night through the window, a big bed.
fn draw_suite(cv: &mut Canvas, age: f32) {
    let t = age;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let c = if (4..28).contains(&y) && (6..74).contains(&x) {
                mix(rgb(10, 10, 40), rgb(60, 25, 70), y as f32 / 28.0)
            } else if y < 32 {
                mix(rgb(90, 70, 60), rgb(60, 45, 40), y as f32 / 32.0)
            } else {
                mix(rgb(120, 30, 40), rgb(70, 15, 25), (y - 32) as f32 / 12.0)
            };
            cv.set(x, y, c);
        }
    }
    // Stars, then the skyline with its windows lit and going out.
    for i in 0..30 {
        let (x, y) = (6 + (hash(i, 1) * 68.0) as i32, 4 + (hash(i, 2) * 10.0) as i32);
        if hash(i, (t * 2.0) as i32) > 0.3 {
            cv.set(x, y, rgb(230, 230, 255));
        }
    }
    let mut x = 6;
    let mut k = 0;
    while x < 74 {
        let w = 4 + (hash(k, 5) * 6.0) as i32;
        let h = 6 + (hash(k, 6) * 14.0) as i32;
        for yy in 28 - h..28 {
            for xx in x..(x + w).min(74) {
                let window = (xx - x) % 2 == 1 && (yy - (28 - h)) % 2 == 1;
                let on = hash(xx * 7 + yy, (t * 0.5) as i32 + k) > 0.45;
                let c = if window && on { rgb(255, 220, 120) } else { rgb(20, 18, 30) };
                cv.set(xx, yy, c);
            }
        }
        x += w + 1;
        k += 1;
    }
    // A tower with a beacon.
    for yy in 6..28 {
        cv.set(52, yy, rgb(40, 40, 60));
        cv.set(53, yy, rgb(30, 30, 50));
    }
    if (t * 1.5) as i32 % 2 == 0 {
        cv.set(52, 5, rgb(255, 60, 60));
        cv.blend(51, 5, rgb(255, 60, 60), 0.4);
        cv.blend(53, 5, rgb(255, 60, 60), 0.4);
    }
    // The window frame.
    for xx in 5..75 {
        cv.set(xx, 4, rgb(200, 190, 170));
        cv.set(xx, 28, rgb(200, 190, 170));
    }
    for yy in 4..29 {
        cv.set(5, yy, rgb(200, 190, 170));
        cv.set(40, yy, rgb(170, 160, 140));
        cv.set(74, yy, rgb(200, 190, 170));
    }
    // The bed.
    for y in 30..44 {
        for xx in 14..66 {
            let c = if y < 33 { rgb(110, 60, 30) } else if y < 36 && ((18..38).contains(&xx) || (42..62).contains(&xx)) { rgb(250, 250, 248) } else { mix(rgb(240, 240, 236), rgb(200, 200, 205), (y - 33) as f32 / 11.0) };
            cv.set(xx, y, c);
        }
    }
    for xx in 14..66 {
        cv.set(xx, 39, rgb(180, 30, 50));
    }
    // A mint on the pillow.
    cv.set(30, 34, rgb(60, 160, 90));
    cv.set(31, 34, rgb(60, 160, 90));
}

/// The show: the curtains part, spotlights sweep.
fn draw_show(cv: &mut Canvas, age: f32) {
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let c = if y > 36 { mix(rgb(90, 50, 25), rgb(50, 25, 10), (y - 36) as f32 / 8.0) } else { mix(rgb(25, 15, 40), rgb(5, 5, 15), y as f32 / 36.0) };
            cv.set(x, y, c);
        }
    }
    // Spotlights from the wings.
    for (i, base) in [(0, 10.0f32), (1, 70.0)] {
        let aim = 40.0 + 18.0 * (age * 1.1 + i as f32 * PI).sin();
        for y in 0..37 {
            let k = y as f32 / 36.0;
            let cx = base + (aim - base) * k;
            let w = 1.0 + 5.0 * k;
            for x in (cx - w) as i32..=(cx + w) as i32 {
                cv.blend(x, y, rgb(255, 245, 210), 0.22 * (1.0 - ((x as f32 - cx) / (w + 0.5)).abs()));
            }
        }
    }
    // A star on the stage, glittering.
    let (sx, sy) = (40, 26);
    for i in 0..16 {
        let a = i as f32 / 16.0 * 2.0 * PI + age;
        let r = if i % 2 == 0 { 7.0 } else { 3.0 };
        let (x, y) = (sx as f32 + a.cos() * r, sy as f32 + a.sin() * r * 0.8);
        cv.set(x.round() as i32, y.round() as i32, rgb(255, 230, 120));
    }
    for i in 0..20 {
        if hash(i, (age * 6.0) as i32) > 0.6 {
            cv.set((hash(i, 9) * 80.0) as i32, (hash(i, 8) * 34.0) as i32, rgb(255, 255, 255));
        }
    }
    // The curtains open from the middle.
    let open = smooth(0.3, 2.3, age) * 36.0;
    for y in 0..37 {
        for x in 0..W as i32 {
            let from_mid = (x as f32 - 39.5).abs();
            if from_mid > open {
                let fold = 0.7 + 0.3 * ((x as f32) * 1.1).sin();
                cv.set(x, y, scale(mix(rgb(200, 25, 40), rgb(110, 10, 20), y as f32 / 40.0), fold));
            }
        }
    }
    for x in 0..W as i32 {
        cv.set(x, 0, rgb(210, 170, 70));
        cv.set(x, 1, rgb(160, 20, 35));
    }
}

// ---- the places, played ------------------------------------------------------

impl Session {
    fn place_status(&mut self, st: &Stage, purse: &Purse, keys: &str) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        ink(&mut out, [200, 150, 210], st.truecolor);
        out.extend(b"chips ");
        ink(&mut out, [255, 230, 140], st.truecolor);
        out.extend(purse.chips.to_string().as_bytes());
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let len = self.visible_len(&fluxbbs_term::cp437::encode(keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    fn say_line_on(&mut self, st: &Stage, name: &str, args: &[&str], color: Ink) {
        let s = crate::editor::strip_colours(&self.strings().fmt(name, args));
        self.stage_line(MSG_ROW, &painted(&s, color), st.truecolor);
    }

    /// The bar.
    pub(crate) async fn casino_bar(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.bar_plain(purse).await;
        }
        let names: Vec<String> = DRINKS.iter().map(|d| d.name.to_string()).collect();
        let mut st = self.stage_open();
        let mut bar = Bar { drinks: purse.drinks, ..Default::default() };
        self.say_line_on(&st, "BarHello", &[], [120, 230, 255]);
        let keys = self.strings().get("BarKeys").to_string();
        self.place_status(&st, purse, &keys);
        let since = std::time::Instant::now();
        let _ = cfg;
        loop {
            let t = st.t();
            draw_bar(&mut st.cv, t, &bar, &names);
            self.stage_show(&mut st, true).await?;
            self.flush().await?;
            if !self.key_waiting().await? {
                tokio::time::sleep(self.idle_pause(50)).await;
                if since.elapsed() > std::time::Duration::from_secs(900) {
                    break;
                }
                continue;
            }
            let k = self.read_key().await?;
            match k {
                Key::Esc => break,
                Key::Char(c @ b'1'..=b'6') => {
                    let i = (c - b'1') as usize;
                    bar.drink = Some((i, st.t()));
                    bar.drinks += 1;
                    purse.drinks += 1;
                    self.purse_save(purse);
                    let key = match bar.drinks {
                        n if n >= 6 => "BarCutOff",
                        n if n >= 4 => "BarEasy",
                        _ => "BarServe",
                    };
                    self.say_line_on(&st, key, &[DRINKS[i].name], [255, 215, 140]);
                }
                k => match k.upper() {
                    Some('Q') => break,
                    Some('D') => self.say_line_on(&st, "BarWhich", &[], [230, 230, 240]),
                    Some('S') => {
                        bar.smoke = Some(st.t());
                        self.say_line_on(&st, "BarSmoke", &[], [220, 220, 230]);
                    }
                    Some('T') => {
                        let line = self.bar_talk();
                        self.stage_line(MSG_ROW, &painted(&line, [255, 200, 150]), st.truecolor);
                    }
                    _ => {}
                },
            }
        }
        self.stage_close();
        Ok(())
    }

    /// Something the bartender says: a line from the string table, or
    /// what's happened lately.
    fn bar_talk(&mut self) -> String {
        let mut lines: Vec<String> = (1..)
            .map(|k| format!("BarTalk{k}"))
            .take_while(|k| !self.strings().stock(k).is_empty())
            .map(|k| crate::editor::strip_colours(self.strings().get(&k)))
            .filter(|s| !s.trim().is_empty())
            .collect();
        if let Some(v) = self.state.sys.script_get("slots", "jackpot").ok().flatten() {
            let mut w = v.splitn(3, ' ');
            if let (Some(day), Some(_), Some(who)) = (w.next(), w.next(), w.next()) {
                lines.push(crate::editor::strip_colours(&self.strings().fmt("BarJackpot", &[who, day])));
            }
        }
        let online = self.state.nodes.online().len();
        if online > 1 {
            lines.push(crate::editor::strip_colours(&self.strings().fmt("BarBusy", &[&(online - 1).to_string()])));
        }
        if lines.is_empty() {
            return String::new();
        }
        lines[rand::rng().random_range(0..lines.len())].clone()
    }

    async fn bar_plain(&mut self, purse: &mut Purse) -> io::Result<()> {
        loop {
            let menu: Vec<String> = DRINKS.iter().enumerate().map(|(i, d)| format!("{} {}", i + 1, d.name)).collect();
            let s = self.strings().fmt("BarPlainPrompt", &[&menu.join(", ")]);
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(4)).await?;
            self.newline();
            let s = s.trim().to_ascii_lowercase();
            match s.as_str() {
                "q" | "" => return Ok(()),
                "s" => {
                    let l = crate::editor::strip_colours(self.strings().get("BarSmoke"));
                    self.write(&l);
                    self.newline();
                }
                "t" => {
                    let l = self.bar_talk();
                    self.write(&l);
                    self.newline();
                }
                n => {
                    if let Some(i) = n.parse::<usize>().ok().filter(|i| (1..=DRINKS.len()).contains(i)) {
                        purse.drinks += 1;
                        self.purse_save(purse);
                        let key = if purse.drinks >= 6 { "BarCutOff" } else if purse.drinks >= 4 { "BarEasy" } else { "BarServe" };
                        let l = crate::editor::strip_colours(&self.strings().fmt(key, &[DRINKS[i - 1].name]));
                        self.write(&l);
                        self.newline();
                    }
                }
            }
        }
    }

    /// The players club: what today's play has earned, and claiming it.
    pub(crate) async fn casino_club(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.club_plain(cfg, purse).await;
        }
        let mut st = self.stage_open();
        self.say_line_on(&st, "ClubHello", &[&self.user.handle.clone()], [255, 225, 140]);
        let keys = self.strings().get("ClubKeys").to_string();
        let since = std::time::Instant::now();
        loop {
            self.place_status(&st, purse, &keys);
            let k = loop {
                let t = st.t();
                draw_club(&mut st.cv, t, cfg, purse);
                self.stage_show(&mut st, true).await?;
                self.flush().await?;
                if self.key_waiting().await? {
                    break Some(self.read_key().await?);
                }
                tokio::time::sleep(self.idle_pause(60)).await;
                if since.elapsed() > std::time::Duration::from_secs(900) {
                    break None;
                }
            };
            let Some(k) = k else { break };
            let pick = match k {
                Key::Esc => break,
                Key::Char(c @ b'1'..=b'3') => (c - b'1') as usize,
                k if k.upper() == Some('Q') => break,
                _ => continue,
            };
            let (name, bit, need) = comps(cfg)[pick];
            if purse.comps & bit != 0 {
                self.say_line_on(&st, "ClubHad", &[name], [200, 200, 210]);
                continue;
            }
            if purse.played < need {
                self.say_line_on(&st, "ClubNotYet", &[&(need - purse.played).to_string(), name], [200, 200, 210]);
                continue;
            }
            let said = self.comp_claim(cfg, purse, pick);
            // The comp itself, on screen a while.
            let begin = st.t();
            while st.t() - begin < 6.0 {
                let age = st.t() - begin;
                match pick {
                    0 => draw_buffet(&mut st.cv, age),
                    1 => draw_suite(&mut st.cv, age),
                    _ => draw_show(&mut st.cv, age),
                }
                st.cv.clear_labels();
                let line: Vec<(char, Ink)> = said
                    .chars()
                    .enumerate()
                    .map(|(i, ch)| (ch, shade([235, 160, 30], [255, 250, 210], 0.5 + 0.5 * ((i as f32) * 0.45 - age * 8.0).sin())))
                    .collect();
                self.stage_line(MSG_ROW, &line, st.truecolor);
                self.stage_show(&mut st, true).await?;
                self.flush().await?;
                if age > 1.5 && self.key_waiting().await? {
                    self.read_key().await?;
                    break;
                }
            }
            self.stage_line(MSG_ROW, &painted(&said, [255, 215, 110]), st.truecolor);
        }
        self.stage_close();
        Ok(())
    }

    /// Take a comp: mark it had and give what it gives. Returns what to say.
    fn comp_claim(&mut self, cfg: &Casino, purse: &mut Purse, pick: usize) -> String {
        let (_, bit, _) = comps(cfg)[pick];
        purse.comps |= bit;
        let (key, amount) = match pick {
            0 => {
                self.user.time_today += cfg.buffet_minutes;
                ("ClubBuffet", cfg.buffet_minutes)
            }
            1 => {
                purse.chips += cfg.suite_chips;
                ("ClubSuite", cfg.suite_chips)
            }
            _ => {
                self.user.file_points = self.user.file_points.saturating_add(cfg.show_points);
                ("ClubShow", cfg.show_points)
            }
        };
        self.purse_save(purse);
        if pick != 1
            && let Err(e) = self.save_own_user() {
                tracing::error!("casino: comping {}: {e}", self.user.handle);
            }
        tracing::info!("{}: casino: comped {}", self.user.handle, comps(cfg)[pick].0);
        crate::editor::strip_colours(&self.strings().fmt(key, &[&amount.to_string()]))
    }

    async fn club_plain(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        loop {
            self.write(&format!("played today: {} chips", purse.played));
            self.newline();
            for (i, (name, bit, need)) in comps(cfg).into_iter().enumerate() {
                let state = if purse.comps & bit != 0 { "enjoyed".to_string() } else if purse.played >= need { "ready".into() } else { format!("{}/{need}", purse.played) };
                self.write(&format!("  {} {name}: {state}", i + 1));
                self.newline();
            }
            self.show(self.strings().get("ClubPlainPrompt").to_string().as_str()).await?;
            let s = self.read_string(crate::input::ReadOpts::new(4)).await?;
            self.newline();
            let Some(pick) = s.trim().parse::<usize>().ok().filter(|n| (1..=3).contains(n)).map(|n| n - 1) else { return Ok(()) };
            let (name, bit, need) = comps(cfg)[pick];
            let line = if purse.comps & bit != 0 {
                crate::editor::strip_colours(&self.strings().fmt("ClubHad", &[name]))
            } else if purse.played < need {
                crate::editor::strip_colours(&self.strings().fmt("ClubNotYet", &[&(need - purse.played).to_string(), name]))
            } else {
                self.comp_claim(cfg, purse, pick)
            };
            self.write(&line);
            self.newline();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pictures to look at: `SLOTS_OUT=dir cargo test casino_pictures --
    /// --ignored` writes ANSI files there.
    #[test]
    #[ignore]
    fn casino_pictures() {
        let dir = std::path::PathBuf::from(std::env::var("SLOTS_OUT").expect("SLOTS_OUT"));
        let names: Vec<String> = DRINKS.iter().map(|d| d.name.to_string()).collect();
        let mut shots: Vec<(String, Canvas)> = Vec::new();
        for (i, drinks) in [(0usize, 1u32), (3, 1), (4, 5)] {
            let mut cv = Canvas::new(W, H);
            draw_bar(&mut cv, 9.0, &Bar { drink: Some((i, 5.0)), smoke: Some(3.0), drinks }, &names);
            shots.push((format!("bar{i}"), cv));
        }
        for (name, f) in [("buffet", draw_buffet as fn(&mut Canvas, f32)), ("suite", draw_suite), ("show", draw_show)] {
            let mut cv = Canvas::new(W, H);
            f(&mut cv, 3.0);
            shots.push((name.into(), cv));
        }
        let mut cv = Canvas::new(W, H);
        draw_club(&mut cv, 1.0, &Casino::default(), &Purse { played: 260, comps: 0, ..Default::default() });
        shots.push(("club".into(), cv));
        for (name, cv) in shots {
            let mut out = b"\x1b[2J".to_vec();
            out.extend(crate::pixels::Encoder::new(true, W, H).frame(&cv, 1));
            std::fs::write(dir.join(format!("{name}.ans")), &out).unwrap();
        }
    }
}
