//! New: a screen, animated while it waits for a key: a menu's
//! (`[[effect]]` in the menu's TOML), or any screen with a `NAME.FX` beside
//! it, at its `|PK`. A glint crossing the art now and then, a pulse of
//! light running along the box lines, firelight, eyes that glow, dust in
//! a shaft of light. The effects work on what's on
//! screen, cell by cell (`Screen::shown`), so they fit whatever the
//! screen drew, codes and all; only cells that change go out, and the
//! menu's text and lightbar are left alone. Off in plain text, on
//! terminals without 24-bit color, and for callers who turn animations
//! off (user config).

use std::io;
use std::time::Duration;
#[cfg(feature = "effects")]
use std::time::Instant;

#[cfg(feature = "effects")]
use fluxbbs_term::Shown;
use serde::{Deserialize, Serialize};

use crate::session::Session;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    /// `glint`: a band of light sweeps across the art. `ripple`: a pulse
    /// runs along the box-drawing lines. `flicker`: firelight, the cells
    /// brightening and dimming. `pulse`: the cells of one color (`matches`)
    /// glowing toward another (`tint`) and back. `dust`: motes drifting
    /// down through the area, catching the light. `type`: the text in the
    /// area typed out again every `every` seconds, `delay` into each
    /// round, `amount` 1 a hundred characters a second (12 to start), with
    /// a block cursor in `tint` if there is one, left blinking at the end
    /// until the next `type` begins. `blink`: the cells of one
    /// color (`matches`) going out and on, each now and then (`every`
    /// seconds apart, on `amount` of the time): lamps, windows. `roll`: a
    /// bright bar rolling down the area every `every` seconds, a tube's.
    /// `flash`: the whole area going dark and back together, every
    /// `every` seconds, lit `amount` of the time (half to start): a
    /// warning sign.
    /// `decode`: the area's letters scrambling and settling into the next
    /// of `messages`, its letters at cells picked afresh each round and
    /// the rest noise, `delay` into every `every` seconds, the scramble
    /// lasting `amount` seconds (2 to start). `wave`, `drips`, `sparkle`
    /// and `breathe`: other ways over a logo; `random`: one of those or
    /// a glint, drawn each time the screen comes up.
    pub kind: String,
    /// The screen rows it covers, first and last (1-based), and the
    /// columns.
    pub rows: [u16; 2],
    #[serde(default = "all_cols")]
    pub cols: [u16; 2],
    /// Seconds from one to the next, and before the first.
    #[serde(default = "every")]
    pub every: f32,
    #[serde(default = "delay")]
    pub delay: f32,
    /// How strong, 0 to 1 (0: the kind's own).
    #[serde(default)]
    pub amount: f32,
    /// `pulse`: the color (`#rrggbb`) to glow toward, and the color of the
    /// cells that glow.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tint: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub matches: String,
    /// `decode`: what the letters settle into, in turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
}

/// What `random` draws from.
const RANDOM_KINDS: [&str; 5] = ["glint", "wave", "drips", "sparkle", "breathe"];

fn all_cols() -> [u16; 2] {
    [1, 80]
}

fn every() -> f32 {
    8.0
}

fn delay() -> f32 {
    1.5
}

#[cfg(feature = "effects")]
fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[cfg(feature = "effects")]
fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).sum::<i32>() < 24
}

/// A steady pseudo-random 0..1.
#[cfg(feature = "effects")]
fn noise(a: i32, b: i32) -> f32 {
    crate::pixels::hash(a, b)
}

#[cfg(feature = "effects")]
struct Target {
    x: u16,
    y: u16,
    base: Shown,
    /// The effects on this cell, in order, each applied to what the one
    /// before made of it.
    effects: Vec<usize>,
    /// For a `type`: seconds into its round when the cell shows, and
    /// when the cursor gets to it.
    typed: Option<(f32, f32)>,
    last: Option<Shown>,
}

/// Seconds into a `type` round that each cell shows (and the cursor
/// reaches it): row by row, a pause at each line's end, the cursor
/// ending after the last line.
#[cfg(feature = "effects")]
fn type_times(cells: &[(u16, u16, u8)], e: &Effect) -> std::collections::HashMap<(u16, u16), (f32, f32)> {
    let step = 1.0 / if e.amount > 0.0 { e.amount * 100.0 } else { 12.0 };
    let pause = (step * 6.0).min(0.6);
    let mut times = std::collections::HashMap::new();
    let mut now = 0.0f32;
    let mut cursor_at = None;
    for y in e.rows[0]..=e.rows[1] {
        let row: Vec<&(u16, u16, u8)> = cells.iter().filter(|c| c.1 == y).collect();
        let Some(last) = row.iter().rposition(|c| c.2 != b' ') else { continue };
        for c in &row[..=last] {
            times.insert((c.0, c.1), (now + step, now));
            now += step;
        }
        cursor_at = row.get(last + 1).map(|c| (c.0, c.1, now));
        if let Some((x, y, _)) = cursor_at {
            times.insert((x, y), (now + pause, now));
        }
        now += pause;
    }
    // The cursor stays after the last line, blinking.
    if let Some((x, y, from)) = cursor_at {
        times.insert((x, y), (f32::INFINITY, from));
    }
    times
}

#[cfg(feature = "effects")]
pub struct Ambient {
    targets: Vec<Target>,
    effects: Vec<Effect>,
    /// For each effect, a `decode`'s letter cells in reading order.
    letters: Vec<Vec<(u16, u16)>>,
    /// What those cells spelled as drawn: all of them, and the bright
    /// ones (the phrase hidden in noise), so a round doesn't settle into
    /// what's already there.
    drawn: Vec<(Vec<u8>, Vec<u8>)>,
    start: Instant,
}

#[cfg(feature = "effects")]
fn box_line(ch: u8) -> bool {
    (0xB3..=0xDA).contains(&ch)
}

#[cfg(feature = "effects")]
fn blank(c: &Shown) -> bool {
    c.ch == b' ' && c.bg.iter().map(|&v| v as u32).sum::<u32>() < 30
}

/// `c` lit `k` of the way toward white.
#[cfg(feature = "effects")]
pub(crate) fn lift(c: [u8; 3], k: f32) -> [u8; 3] {
    toward(c, [255, 255, 255], k)
}

#[cfg(feature = "effects")]
fn toward(c: [u8; 3], to: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (c[i] as f32 + (to[i] as f32 - c[i] as f32) * k).round() as u8)
}

#[cfg(feature = "effects")]
pub(crate) fn times(c: [u8; 3], f: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * f).round().clamp(0.0, 255.0) as u8)
}

#[cfg(feature = "effects")]
impl Ambient {
    /// The cells the effects work on, as the screen shows them now.
    pub fn new(screen: &fluxbbs_term::Screen, effects: &[Effect]) -> Option<Ambient> {
        let mut targets: Vec<Target> = Vec::new();
        let mut at = std::collections::HashMap::new();
        for (i, e) in effects.iter().enumerate() {
            let matches = hex(&e.matches);
            let mut cells = Vec::new();
            for y in e.rows[0].max(1)..=e.rows[1].min(screen.rows()) {
                for x in e.cols[0].max(1)..=e.cols[1].min(80) {
                    let Some(c) = screen.shown(x, y) else { continue };
                    let wanted = match e.kind.as_str() {
                        "glint" | "flicker" | "wave" | "drips" | "sparkle" | "breathe" | "flash" => !blank(&c),
                        "ripple" => box_line(c.ch),
                        "pulse" | "blink" => matches.is_some_and(|m| near(c.fg, m) || near(c.bg, m)),
                        "dust" | "type" | "roll" => true,
                        "decode" => c.ch != b' ',
                        _ => false,
                    };
                    if wanted {
                        cells.push((x, y, c.ch));
                    }
                }
            }
            let times = if e.kind == "type" { type_times(&cells, e) } else { Default::default() };
            for (x, y, _) in cells {
                // A cell's typed by one `type` only, and the first one has it.
                if e.kind == "type" && !times.contains_key(&(x, y)) {
                    continue;
                }
                let k = *at.entry((x, y)).or_insert_with(|| {
                    let base = screen.shown(x, y).unwrap_or(Shown { ch: b' ', fg: [0; 3], bg: [0; 3] });
                    targets.push(Target { x, y, base, effects: Vec::new(), typed: None, last: None });
                    targets.len() - 1
                });
                let g = &mut targets[k];
                if e.kind == "type" {
                    if g.typed.is_some() {
                        continue;
                    }
                    g.typed = times.get(&(x, y)).copied();
                }
                g.effects.push(i);
            }
        }
        // In screen order, so a frame's cells run on from one another.
        targets.sort_by_key(|g| (g.y, g.x));
        let letters: Vec<Vec<(u16, u16)>> = (0..effects.len())
            .map(|i| if effects[i].kind == "decode" { targets.iter().filter(|g| g.effects.contains(&i)).map(|g| (g.x, g.y)).collect() } else { Vec::new() })
            .collect();
        let sum = |c: [u8; 3]| c.iter().map(|&v| v as u32).sum::<u32>();
        let drawn = (0..effects.len())
            .map(|i| {
                let bases: Vec<Shown> = targets.iter().filter(|g| g.effects.contains(&i)).map(|g| g.base).collect();
                let bright = bases.iter().map(|b| sum(b.fg)).max().unwrap_or(0);
                (bases.iter().map(|b| b.ch).collect(), bases.iter().filter(|b| sum(b.fg) == bright).map(|b| b.ch).collect())
            })
            .collect();
        (!targets.is_empty()).then(|| Ambient { targets, effects: effects.to_vec(), letters, drawn, start: Instant::now() })
    }

    /// How lit cell (`x`, `y`) of a glint or ripple is at `t`, 0 to 1.
    fn sweep(e: &Effect, x: u16, y: u16, phase: f32) -> f32 {
        let (c0, c1) = (e.cols[0] as f32, e.cols[1] as f32);
        let (run, width, slant, peak) = if e.kind == "glint" { (1.6, 2.6, 1.6, 0.7) } else { (2.4, 3.2, 0.8, 0.75) };
        if phase > run {
            return 0.0;
        }
        let at = c0 - 12.0 + phase / run * (c1 - c0 + 24.0);
        let d = x as f32 + (y - e.rows[0]) as f32 * slant - at;
        let amount = if e.amount > 0.0 { e.amount } else { peak };
        amount * (-(d / width).powi(2)).exp()
    }

    /// Where the dust is at `t`: the cells a mote's in.
    fn motes(e: &Effect, i: usize, t: f32) -> Vec<(u16, u16)> {
        // The area as given, whichever way round, and no bigger than a
        // screen can be.
        let (c0, c1) = (e.cols[0].min(e.cols[1]).max(1), e.cols[0].max(e.cols[1]).min(80));
        let (r0, r1) = (e.rows[0].min(e.rows[1]).max(1), e.rows[0].max(e.rows[1]).min(60));
        let (w, h) = ((c1 - c0 + 1) as f32, (r1 - r0 + 1) as f32);
        let n = ((w * h) / 7.0).ceil().max(1.0) as i32;
        (0..n)
            .map(|k| {
                let seed = i as i32 * 101 + k;
                let x = c0 as f32 + noise(seed, 1) * w + (t * 0.5 + k as f32).sin() * 1.2;
                let fall = t * (0.35 + noise(seed, 3) * 0.5);
                let y = r0 as f32 + (noise(seed, 2) * h + fall).rem_euclid(h);
                (x.round().clamp(c0 as f32, c1 as f32) as u16, y.floor() as u16)
            })
            .collect()
    }

    /// A cell under its effects at `t`.
    fn shade(&self, g: &Target, t: f32, dust: &std::collections::HashSet<(u16, u16)>) -> Shown {
        g.effects.iter().fold(g.base, |now, &i| self.shade_one(i, g, now, t, dust))
    }

    /// What effect `i` makes of a cell (`b`, as the effects before it
    /// left it) at `t`.
    fn shade_one(&self, i: usize, g: &Target, b: Shown, t: f32, dust: &std::collections::HashSet<(u16, u16)>) -> Shown {
        let e = &self.effects[i];
        if e.kind == "decode" {
            return self.decode(i, g, b, t);
        }
        let since = t - e.delay;
        let phase = since.rem_euclid(e.every.max(0.5));
        let q = |k: f32, steps: f32| (k * steps).round() / steps;
        let (fg, bg) = match e.kind.as_str() {
            // New: more ways across a logo, so a menu can draw one at
            // random (`random`). A wave: a sweep rippled into bands. Drips:
            // a bright drop running down each column, columns a little
            // apart. Sparkle: cells flashing here and there. Breathe: the
            // whole thing dimming and coming back.
            "wave" => {
                let (c0, c1) = (e.cols[0] as f32, e.cols[1] as f32);
                let run = 2.2;
                if since < 0.0 || phase > run {
                    return b;
                }
                let at = c0 - 14.0 + phase / run * (c1 - c0 + 28.0);
                let d = g.x as f32 + (g.y - e.rows[0]) as f32 * 1.2 - at;
                let amount = if e.amount > 0.0 { e.amount } else { 0.7 };
                let k = q(amount * (-(d / 9.0).powi(2)).exp() * (0.5 + 0.5 * (d * 0.9).cos()), 8.0);
                let fg = if b.ch == b' ' { b.fg } else { lift(b.fg, k) };
                let lit_bg = b.bg.iter().map(|&v| v as u32).sum::<u32>() >= 30;
                (fg, if lit_bg { lift(b.bg, k * 0.5) } else { b.bg })
            }
            "drips" => {
                let run = 2.4;
                if since < 0.0 || phase > run {
                    return b;
                }
                let span = (e.rows[1] - e.rows[0] + 1) as f32 + 4.0;
                let start = noise(g.x as i32, 41) * 0.8;
                let head = e.rows[0] as f32 - 2.0 + ((phase - start) / (run - 0.8)).clamp(0.0, 1.0) * span;
                let behind = head - g.y as f32;
                let amount = if e.amount > 0.0 { e.amount } else { 0.8 };
                let k = if (0.0..3.0).contains(&behind) { q(amount * (1.0 - behind / 3.0), 8.0) } else { 0.0 };
                let fg = if b.ch == b' ' { b.fg } else { lift(b.fg, k) };
                (fg, lift(b.bg, k * 0.4))
            }
            "sparkle" => {
                if since < 0.0 || phase > 3.0 {
                    return b;
                }
                let tick = (t * 8.0) as i32;
                let amount = if e.amount > 0.0 { e.amount } else { 0.9 };
                let on = noise(tick, g.x as i32 * 7 + g.y as i32 * 131) < 0.04;
                let k = if on { amount } else { 0.0 };
                (lift(b.fg, k), lift(b.bg, k * 0.5))
            }
            "breathe" => {
                let amount = if e.amount > 0.0 { e.amount } else { 0.6 };
                let wave = if since < 0.0 { 0.0 } else { (0.5 - 0.5 * (phase / e.every.max(0.5) * 2.0 * std::f32::consts::PI).cos()).powi(2) };
                let k = q(1.0 - amount * wave, 12.0);
                (times(b.fg, k), times(b.bg, k))
            }
            "glint" | "ripple" => {
                let k = if since < 0.0 { 0.0 } else { q(Self::sweep(e, g.x, g.y, phase), 8.0) };
                let fg = if b.ch == b' ' { b.fg } else { lift(b.fg, k) };
                let lit_bg = e.kind == "glint" && b.bg.iter().map(|&v| v as u32).sum::<u32>() >= 30;
                (fg, if lit_bg { lift(b.bg, k * 0.5) } else { b.bg })
            }
            "flicker" => {
                // The flame as a whole, and a little each cell on its own.
                let amount = if e.amount > 0.0 { e.amount } else { 0.3 };
                let tick = (t * 9.0) as i32;
                let f = 0.7 * noise(tick, i as i32) + 0.3 * noise(tick + g.x as i32 * 31, g.y as i32);
                let f = q(1.0 + amount * (f - 0.5) * 2.0, 16.0);
                let warm = |c: [u8; 3]| {
                    let c = times(c, f);
                    if f > 1.0 { toward(c, [255, 210, 110], (f - 1.0) * 0.8) } else { c }
                };
                (warm(b.fg), warm(b.bg))
            }
            "pulse" => {
                let (Some(m), Some(to)) = (hex(&e.matches), hex(&e.tint)) else { return b };
                let amount = if e.amount > 0.0 { e.amount } else { 0.9 };
                let wave = if since < 0.0 { 0.0 } else { (0.5 - 0.5 * (phase / e.every.max(0.5) * 2.0 * std::f32::consts::PI).cos()).powi(2) };
                let k = q(amount * wave, 8.0);
                let fg = if near(b.fg, m) { toward(b.fg, to, k) } else { b.fg };
                let bg = if near(b.bg, m) { toward(b.bg, to, k) } else { b.bg };
                (fg, bg)
            }
            "dust" => {
                if dust.contains(&(g.x, g.y)) {
                    let amount = if e.amount > 0.0 { e.amount } else { 0.35 };
                    (toward(b.fg, [255, 240, 205], amount), toward(b.bg, [255, 240, 205], amount))
                } else {
                    (b.fg, b.bg)
                }
            }
            "type" => {
                let Some((shows, cursor)) = g.typed else { return b };
                // Seconds into this round's typing, which starts `delay`
                // seconds into each round (a delay past `every` wraps,
                // so the text isn't hidden for good).
                let every = e.every.max(1.0);
                let local = t.rem_euclid(every) - e.delay.rem_euclid(every);
                if local >= shows {
                    return b;
                }
                // The cursor: steady while it types, blinking once done
                // until the next `type` starts.
                let done = shows.is_infinite();
                let moved_on = || self.effects[i + 1..].iter().any(|f| f.kind == "type" && t.rem_euclid(f.every.max(1.0)) - f.delay.rem_euclid(f.every.max(1.0)) >= 0.0);
                if let Some(c) = hex(&e.tint)
                    && !(done && moved_on())
                    && local >= cursor
                    && (!done || (local * 2.0).rem_euclid(2.0) < 1.0)
                {
                    return Shown { ch: b.ch, fg: c, bg: c };
                }
                (b.bg, b.bg)
            }
            "flash" => {
                let on = if e.amount > 0.0 { e.amount } else { 0.5 };
                if ((t - e.delay).max(0.0) / e.every.max(0.1)).fract() < on {
                    return b;
                }
                (times(b.fg, 0.12), times(b.bg, 0.12))
            }
            "blink" => {
                let Some(m) = hex(&e.matches) else { return b };
                let on = if e.amount > 0.0 { e.amount } else { 0.5 };
                let tick = (t / e.every.max(0.05) + noise(g.x as i32, g.y as i32 * 7) * 4.0).floor() as i32;
                if noise(tick, g.x as i32 * 131 + g.y as i32) < on {
                    return b;
                }
                let dim = |c: [u8; 3]| if near(c, m) { times(c, 0.22) } else { c };
                (dim(b.fg), dim(b.bg))
            }
            "roll" => {
                let span = (e.rows[1] - e.rows[0] + 1) as f32 + 4.0;
                let at = e.rows[0] as f32 - 2.0 + (t - e.delay).rem_euclid(e.every.max(0.5)) / e.every.max(0.5) * span;
                let amount = if e.amount > 0.0 { e.amount } else { 0.18 };
                let k = q(amount * (-((g.y as f32 - at) / 0.9).powi(2)).exp(), 8.0);
                let fg = if b.ch == b' ' { b.fg } else { lift(b.fg, k) };
                (fg, lift(b.bg, k * 0.7))
            }
            _ => (b.fg, b.bg),
        };
        Shown { ch: b.ch, fg, bg }
    }

    /// `decode`: a letter cell at `t`. Each round picks the cells that
    /// carry the message's letters (as many as it has, in reading
    /// order); the rest are noise. For the first `amount` seconds of a
    /// round the cells run through glyphs, each settling in its own
    /// time. The letters take `tint`, or the brightest color the area was
    /// drawn in; noise the darkest. When the message fills every cell
    /// (a word down a margin), each keeps its own color.
    fn decode(&self, i: usize, g: &Target, b: Shown, t: f32) -> Shown {
        let e = &self.effects[i];
        let since = t - e.delay;
        let cells = &self.letters[i];
        if since < 0.0 || e.messages.is_empty() || cells.is_empty() {
            return b;
        }
        let every = e.every.max(1.0);
        let round = (since / every).floor() as i32;
        let phase = since - round as f32 * every;
        let dur = if e.amount > 0.0 { e.amount } else { 2.0 };
        let Some(me) = cells.iter().position(|&p| p == (g.x, g.y)) else { return b };
        let message = |r: i32| -> Vec<u8> { fluxbbs_term::cp437::encode(&e.messages[r.rem_euclid(e.messages.len() as i32) as usize]).into_iter().filter(|&c| c != b' ').collect() };
        // Not back into what the screen already says (the first round
        // would, when the list starts with it).
        let mut msg = message(round);
        let (all, bright) = &self.drawn[i];
        if msg == *if msg.len() >= cells.len() { all } else { bright } {
            msg = message(round + 1);
        }
        let n = msg.len().min(cells.len());
        // This round's letter cells: the n with the lowest draw (ties
        // broken by place, so exactly n are lit and the letter index
        // never runs past the message).
        let draws: Vec<f32> = (0..cells.len()).map(|k| noise(round.wrapping_mul(7919) + k as i32, 17)).collect();
        let mut order: Vec<usize> = (0..cells.len()).collect();
        order.sort_by(|&a, &b| draws[a].total_cmp(&draws[b]).then(a.cmp(&b)));
        let rank_of = |k: usize| order.iter().position(|&o| o == k).unwrap_or(usize::MAX);
        let lit = |k: usize| rank_of(k) < n;
        let letter = lit(me).then(|| msg[(0..me).filter(|&k| lit(k)).count().min(n - 1)]);
        // The colors the area was drawn in.
        let bases = || self.targets.iter().filter(|o| o.effects.contains(&i)).map(|o| o.base.fg);
        let sum = |c: [u8; 3]| c.iter().map(|&v| v as u32).sum::<u32>();
        let dim = bases().min_by_key(|&c| sum(c)).unwrap_or(b.fg);
        let bright = hex(&e.tint).or_else(|| bases().max_by_key(|&c| sum(c))).unwrap_or(b.fg);
        let settle = dur * (0.25 + 0.75 * noise(round.wrapping_mul(31) + me as i32, 5));
        if phase < settle {
            let tick = (t * 12.0) as i32;
            let k = (noise(tick, me as i32 * 13 + round) * 4.0).round() / 4.0;
            return Shown { ch: rain_glyph(noise(tick.wrapping_mul(3), me as i32)), fg: toward(dim, bright, k), bg: b.bg };
        }
        match letter {
            Some(ch) => Shown { ch, fg: if n == cells.len() { g.base.fg } else { bright }, bg: b.bg },
            None => {
                const SET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ0123456789";
                let r = noise(round.wrapping_mul(101) + me as i32, 3);
                Shown { ch: SET[((r * SET.len() as f32) as usize).min(SET.len() - 1)], fg: dim, bg: b.bg }
            }
        }
    }

    /// The ANSI that puts every cell back as it was drawn, after a
    /// moment's animation.
    pub fn restore(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut at: Option<(u16, u16)> = None;
        let mut pen = None;
        for g in &mut self.targets {
            if g.last.is_none_or(|l| l == g.base) {
                continue;
            }
            g.last = Some(g.base);
            if at != Some((g.x, g.y)) {
                out.extend(format!("\x1b[{};{}H", g.y, g.x).as_bytes());
            }
            let (fg, bg) = (g.base.fg, g.base.bg);
            if pen != Some((fg, bg)) {
                out.extend(format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]).as_bytes());
                pen = Some((fg, bg));
            }
            out.push(g.base.ch);
            at = Some((g.x + 1, g.y));
        }
        out
    }

    /// The ANSI for what's changed since the last frame.
    pub fn frame(&mut self) -> Vec<u8> {
        let t = self.start.elapsed().as_secs_f32();
        let dust: std::collections::HashSet<(u16, u16)> = self
            .effects
            .iter()
            .enumerate()
            .filter(|(i, e)| e.kind == "dust" && t >= e.delay && self.targets.iter().any(|g| g.effects.contains(i)))
            .flat_map(|(i, e)| Self::motes(e, i, t))
            .collect();
        let cells: Vec<Shown> = self.targets.iter().map(|g| self.shade(g, t, &dust)).collect();
        let mut out = Vec::new();
        let mut at: Option<(u16, u16)> = None;
        // Colors go out only when they change from the cell before.
        let mut pen = None;
        for (g, now) in self.targets.iter_mut().zip(cells) {
            if g.last == Some(now) || (g.last.is_none() && g.base == now) {
                g.last = Some(now);
                continue;
            }
            g.last = Some(now);
            if at != Some((g.x, g.y)) {
                out.extend(format!("\x1b[{};{}H", g.y, g.x).as_bytes());
            }
            let (fg, bg) = (now.fg, now.bg);
            if pen != Some((fg, bg)) {
                out.extend(format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]).as_bytes());
                pen = Some((fg, bg));
            }
            out.push(now.ch);
            at = Some((g.x + 1, g.y));
        }
        out
    }
}

/// Without the `effects` component nothing animates: a stand-in that
/// never starts.
#[cfg(not(feature = "effects"))]
pub struct Ambient;

#[cfg(not(feature = "effects"))]
impl Ambient {
    pub fn new(_screen: &fluxbbs_term::Screen, _effects: &[Effect]) -> Option<Ambient> {
        None
    }

    pub fn restore(&mut self) -> Vec<u8> {
        Vec::new()
    }

    pub fn frame(&mut self) -> Vec<u8> {
        Vec::new()
    }
}

#[cfg(feature = "effects")]
/// What falls: digits, letters, and CP437's Greek and math signs.
pub(crate) fn rain_glyph(r: f32) -> u8 {
    const SET: &[u8] = b"0123456789ABCDEFGHJKLMNPQRSTUVWXYZ0123456789\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf7";
    SET[((r * SET.len() as f32) as usize).min(SET.len() - 1)]
}

/// Effects kept beside a screen (`NAME.FX`, a TOML file of `[[effect]]`),
/// for the screen's `|PK` wait.
#[derive(Deserialize)]
struct Sidecar {
    #[serde(default, rename = "effect")]
    effects: Vec<Effect>,
}

pub fn sidecar(screen: &std::path::Path) -> Option<Vec<Effect>> {
    let path = ["FX", "fx"].iter().map(|e| screen.with_extension(e)).find(|p| p.exists())?;
    match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| toml::from_str::<Sidecar>(&t).map_err(|e| e.to_string())) {
        Ok(s) if !s.effects.is_empty() => Some(s.effects),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("{}: {e}", path.display());
            None
        }
    }
}

impl Session {
    /// New: a screen's effects paced to the board's `glint_seconds`:
    /// menus and screens time them for a glint every 10 seconds, so
    /// every `every` and `delay` is scaled by `glint_seconds / 10`,
    /// which keeps the decodes in step with the glints.
    pub(crate) fn paced(&self, effects: &[Effect]) -> Vec<Effect> {
        use rand::RngExt;
        let glint = self.state.config().terminal.glint_seconds;
        let k = if glint == 0 { 1.0 } else { glint as f32 / 10.0 };
        effects
            .iter()
            .map(|e| {
                // `random`: one of the ways across a logo, drawn each
                // time the screen comes up.
                let kind = if e.kind == "random" { RANDOM_KINDS[rand::rng().random_range(0..RANDOM_KINDS.len())].to_string() } else { e.kind.clone() };
                Effect { kind, every: e.every * k, delay: e.delay * k, ..e.clone() }
            })
            .collect()
    }

    /// Do this caller's screen and settings allow animating?
    pub(crate) fn animating(&self) -> bool {
        cfg!(feature = "effects")
            && !self.plain()
            && self.user.animate != "off"
            && (self.screen.truecolor() || self.screen.palette256() || self.screen.charset() == fluxbbs_term::Charset::Utf8)
    }

    /// A key, the screen animating while it waits; idle long enough
    /// (`screensaver_seconds`), with `saver`, the screen saver, and then
    /// None: the screen wants drawing again. Node messages, pages and the
    /// inactivity timeout work as ever: when something's for the caller,
    /// the animation stops and the usual wait takes over.
    pub(crate) async fn read_key_ambient(&mut self, amb: &mut Option<Ambient>, saver: bool) -> io::Result<Option<crate::input::Key>> {
        let since = tokio::time::Instant::now();
        let idle = self.idle_limit();
        // No screen saver unless one's built in (`screensavers`).
        let saver_after = if saver && cfg!(feature = "screensavers") { self.state.config().terminal.screensaver_seconds } else { 0 };
        if amb.is_some() || saver_after > 0 {
            let waker = self.node.waker();
            loop {
                if self.key_waiting().await? {
                    self.restore_effects(amb.as_mut()).await?;
                    break;
                }
                let stale = idle > 0 && since.elapsed() >= Duration::from_secs(idle);
                if stale || self.node.has_messages() {
                    self.restore_effects(amb.as_mut()).await?;
                    *amb = None;
                    break;
                }
                if saver_after > 0 && since.elapsed() >= Duration::from_secs(saver_after) {
                    #[cfg(feature = "screensavers")]
                    self.screensaver(since, idle).await?;
                    // The saver ends at the inactivity timeout too: the
                    // caller goes, as they would from a prompt.
                    if idle > 0 && since.elapsed() >= Duration::from_secs(idle) && !self.key_waiting().await? {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "inactivity timeout"));
                    }
                    return Ok(None);
                }
                if let Some(a) = amb.as_mut() {
                    let out = a.frame();
                    if !out.is_empty() {
                        let (x, y, color) = (self.screen.where_x(), self.screen.where_y(), self.screen.color());
                        self.screen.write_bytes(&out);
                        self.screen.write_bytes(b"\x1b[0m");
                        self.screen.goto(x, y);
                        self.screen.set_color(color);
                        // One frame on the way at a time, on a slow line.
                        self.sync_output(Duration::from_secs(3), 1).await?;
                        self.flush().await?;
                    }
                }
                tokio::select! {
                    // A steady pace: frames only go out while something
                    // moves, each waiting for the last to be drawn.
                    _ = tokio::time::sleep(Duration::from_millis(70)) => {}
                    _ = waker.notified() => {
                        // Something for the caller: the usual wait shows it.
                        self.restore_effects(amb.as_mut()).await?;
                        *amb = None;
                        break;
                    }
                }
            }
        }
        self.idle_since = Some(since);
        let k = self.read_key().await;
        self.idle_since = None;
        k.map(Some)
    }

    /// Put the cells an animation changed back as they were drawn, when
    /// its wait ends (a key, a message), so nothing is left mid-effect.
    async fn restore_effects(&mut self, amb: Option<&mut Ambient>) -> io::Result<()> {
        let Some(a) = amb else { return Ok(()) };
        let out = a.restore();
        if !out.is_empty() {
            let (x, y, color) = (self.screen.where_x(), self.screen.where_y(), self.screen.color());
            self.screen.write_bytes(&out);
            self.screen.write_bytes(b"\x1b[0m");
            self.screen.goto(x, y);
            self.screen.set_color(color);
            self.flush().await?;
        }
        Ok(())
    }

    /// Without screen savers built in (`screensavers`), Backspace on a
    /// menu has nothing to start.
    #[cfg(not(feature = "screensavers"))]
    pub(crate) async fn screensaver_now(&mut self) -> io::Result<bool> {
        Ok(false)
    }

    /// New: run effects over what's on screen for `secs` seconds, not
    /// waiting for a key (a key pressed meanwhile cuts it short and is
    /// kept). For a moment's animation inside a screen, where
    /// `read_key_ambient` would wait.
    pub(crate) async fn play_effects(&mut self, effects: &[Effect], secs: f32) -> io::Result<()> {
        if !self.animating() {
            return Ok(());
        }
        let Some(mut amb) = Ambient::new(&self.screen, effects) else { return Ok(()) };
        let start = tokio::time::Instant::now();
        while start.elapsed().as_secs_f32() < secs {
            if self.key_waiting().await? || self.node.has_messages() {
                break;
            }
            let out = amb.frame();
            if !out.is_empty() {
                let (x, y, color) = (self.screen.where_x(), self.screen.where_y(), self.screen.color());
                self.screen.write_bytes(&out);
                self.screen.write_bytes(b"\x1b[0m");
                self.screen.goto(x, y);
                self.screen.set_color(color);
                self.sync_output(Duration::from_secs(3), 1).await?;
                self.flush().await?;
            }
            tokio::time::sleep(Duration::from_millis(70)).await;
        }
        let out = amb.restore();
        if !out.is_empty() {
            let (x, y, color) = (self.screen.where_x(), self.screen.where_y(), self.screen.color());
            self.screen.write_bytes(&out);
            self.screen.write_bytes(b"\x1b[0m");
            self.screen.goto(x, y);
            self.screen.set_color(color);
            self.flush().await?;
        }
        Ok(())
    }
}

// The effects' own tests (the saver's are in `savers`).
#[cfg(all(test, feature = "effects"))]
mod tests {
    use super::*;
    use fluxbbs_term::{Charset, Screen};

    fn fx(kind: &str, rows: [u16; 2]) -> Effect {
        Effect { kind: kind.into(), rows, cols: all_cols(), every: 8.0, delay: 1.5, amount: 0.0, tint: String::new(), matches: String::new(), messages: Vec::new() }
    }

    fn colors(s: Shown) -> ([u8; 3], [u8; 3]) {
        (s.fg, s.bg)
    }

    #[test]
    fn effects_touch_only_their_cells() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;200;0;200mLOGO\x1b[3;1H\x1b[31m\xDA\xC4\xC4\xBF  menu text");
        let a = Ambient::new(&s, &[fx("glint", [1, 1]), fx("ripple", [3, 3])]).unwrap();
        let cells: Vec<(u16, u16)> = a.targets.iter().map(|g| (g.x, g.y)).collect();
        assert_eq!(cells, [(1, 1), (2, 1), (3, 1), (4, 1), (1, 3), (2, 3), (3, 3), (4, 3)], "the logo and the box line, not the text");
        assert!(Ambient::new(&s, &[fx("ripple", [1, 1])]).is_none());
    }

    #[test]
    fn a_glint_comes_and_goes() {
        let e = fx("glint", [1, 9]);
        assert!((0..80).any(|x| Ambient::sweep(&e, x, 1, 0.8) > 0.01));
        assert!((0..80).all(|x| Ambient::sweep(&e, x, 1, 4.0) == 0.0));
    }

    #[test]
    fn a_flash_darkens_all_at_once() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;255;85;85mWARN");
        let e = Effect { every: 1.0, delay: 0.0, amount: 0.6, ..fx("flash", [1, 1]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        assert_eq!(a.targets.len(), 4);
        let lit = |t: f32| a.targets.iter().map(|g| colors(a.shade(g, t, &Default::default())).0).collect::<Vec<_>>();
        assert!(lit(0.3).iter().all(|c| *c == [255, 85, 85]), "lit {:?}", lit(0.3));
        assert!(lit(0.8).iter().all(|c| c[0] < 60), "dark together {:?}", lit(0.8));
    }

    #[test]
    fn a_pulse_lights_only_its_color() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;60;10;8;48;2;200;200;190m\xDC\x1b[38;2;200;200;190m\xDC");
        let e = Effect { matches: "#3c0a08".into(), tint: "#ff3018".into(), ..fx("pulse", [1, 1]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        assert_eq!(a.targets.len(), 1, "only the cell with the eye's color");
        let (fg, bg) = colors(a.shade(&a.targets[0], 1.5 + 4.0, &Default::default()));
        assert!(fg[0] > 200 && bg == [200, 200, 190], "the eye glows, the bone doesn't: {fg:?} {bg:?}");
    }

    #[test]
    fn text_types_itself_out_with_a_cursor() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;0;255;0;48;2;0;0;20mATDT\x1b[2;1Hlogin:");
        let e = Effect { cols: [1, 10], every: 20.0, delay: 1.0, amount: 0.1, tint: "#00ff00".into(), ..fx("type", [1, 2]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        let at = |x: u16, y: u16, t: f32| {
            let g = a.targets.iter().find(|g| (g.x, g.y) == (x, y)).unwrap();
            colors(a.shade(g, t, &Default::default()))
        };
        let (bg, green) = ([0, 0, 20], [0, 255, 0]);
        assert_eq!(at(1, 1, 0.5), (bg, bg), "nothing before the delay");
        assert_eq!(at(1, 1, 1.05), (green, green), "the cursor on the first letter");
        assert_eq!(at(1, 1, 1.15), (green, bg), "then the letter");
        assert_eq!(at(4, 1, 1.15), (bg, bg), "the rest still to come");
        // After the last line the cursor sits after `login:`, blinking.
        let done: Vec<_> = [10.0, 10.5, 11.0, 11.5].iter().map(|&t| at(7, 2, t).1).collect();
        assert!(done.contains(&green) && done.contains(&[0, 0, 0]), "{done:?}");
        assert_eq!(at(6, 2, 10.0), (green, bg));
        // And it starts over each round.
        assert_eq!(at(6, 2, 20.5), (bg, bg));
    }

    #[test]
    fn effects_stack_on_a_cell() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;0;200;0;48;2;0;0;0mHI");
        let roll = Effect { every: 4.0, delay: 0.0, ..fx("roll", [1, 1]) };
        let typed = Effect { every: 30.0, delay: 0.0, amount: 1.0, ..fx("type", [1, 1]) };
        let a = Ambient::new(&s, &[roll, typed]).unwrap();
        assert_eq!(a.targets.len(), 80, "one target a cell, with both effects");
        assert_eq!(a.targets[0].effects, [0, 1]);
        // The bar over the typed text lights it.
        let (fg, bg) = colors(a.shade(&a.targets[0], 4.0 * 2.0 / 5.0, &Default::default()));
        assert!(fg[1] > 200 && bg[0] > 0, "{fg:?} {bg:?}");
    }

    #[test]
    fn letters_decode_into_the_next_message() {
        let mut s = Screen::new(Charset::Cp437, 25);
        // Four noise letters in gray, one lit in pink.
        s.write_bytes(b"\x1b[2J\x1b[38;2;85;85;85;48;2;0;0;0mAB \x1b[38;2;255;85;255mC\x1b[38;2;85;85;85mD");
        let e = Effect { every: 10.0, delay: 1.0, amount: 1.0, messages: vec!["XY".into(), "QRS".into()], ..fx("decode", [1, 1]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        assert_eq!(a.letters[0], [(1, 1), (2, 1), (4, 1), (5, 1)], "the letters, not the space");
        let row = |t: f32| -> Vec<Shown> { a.targets.iter().map(|g| a.shade(g, t, &Default::default())).collect() };
        assert!(row(0.5).iter().zip(&a.targets).all(|(s, g)| *s == g.base), "as drawn before the first round");
        let lit = |t: f32| row(t).iter().filter(|s| s.fg == [255, 85, 255]).map(|s| s.ch).collect::<Vec<u8>>();
        assert_eq!(lit(1.0 + 1.5), b"XY", "the message's letters, in order, in the lit color");
        assert_eq!(lit(11.0 + 1.5), b"QRS", "then the next");
        assert!(row(11.0 + 1.5).iter().all(|s| s.fg == [255, 85, 255] || s.fg == [85, 85, 85]), "the rest noise in the dim color");
        assert!(row(1.05) != row(1.15), "scrambling at the start of a round");
    }

    #[test]
    fn a_decode_never_settles_into_what_is_drawn() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;255;85;255;48;2;0;0;0mI\x1b[2;1HN\x1b[3;1HS\x1b[4;1HI\x1b[5;1HD\x1b[6;1HE");
        let e = Effect { cols: [1, 1], every: 10.0, delay: 1.0, amount: 1.0, messages: vec!["INSIDE".into(), "SILENT".into()], ..fx("decode", [1, 6]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        let word = |t: f32| a.targets.iter().map(|g| a.shade(g, t, &Default::default()).ch).collect::<Vec<u8>>();
        assert_eq!(word(1.0 + 1.5), b"SILENT", "the first round skips the word already there");
        assert_eq!(word(11.0 + 1.5), b"SILENT", "and so does the second, whose message is that word");
    }

    #[test]
    fn blinking_lamps_go_out_and_on() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;255;40;40;48;2;30;30;30m\xDC");
        let e = Effect { matches: "#ff2828".into(), every: 0.2, ..fx("blink", [1, 1]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        let seen: std::collections::HashSet<_> = (0..60).map(|i| a.shade(&a.targets[0], i as f32 * 0.1, &Default::default()).fg).collect();
        assert!(seen.len() == 2, "lit and out: {seen:?}");
    }

    #[test]
    fn dust_stays_in_its_area() {
        let e = Effect { cols: [30, 40], ..fx("dust", [5, 15]) };
        for t in [0.0, 3.3, 17.0] {
            assert!(Ambient::motes(&e, 0, t).iter().all(|&(x, y)| (30..=40).contains(&x) && (5..=15).contains(&y)));
        }
        // Bounds given backwards, or past the screen, are taken as meant.
        let e = Effect { cols: [40, 30], ..fx("dust", [900, 5]) };
        assert!(Ambient::motes(&e, 0, 1.0).iter().all(|&(x, y)| (30..=40).contains(&x) && (5..=60).contains(&y)));
    }

    #[test]
    fn a_long_decode_row_lights_exactly_its_message() {
        // 76 letters, 16-bit draws: ties are certain over enough rounds;
        // each round still lights exactly the message's letters.
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;85;85;85;48;2;0;0;0m");
        s.write_bytes(&[b'A'; 76]);
        let msgs: Vec<String> = (0..40).map(|i| format!("MSG{i:02}")).collect();
        let e = Effect { cols: [1, 76], every: 4.0, delay: 0.0, amount: 1.0, tint: "#ff55ff".into(), messages: msgs, ..fx("decode", [1, 1]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        for round in 0..400 {
            let t = round as f32 * 4.0 + 3.5;
            let lit: Vec<u8> = a.targets.iter().map(|g| a.shade(g, t, &Default::default())).filter(|s| s.fg != [85, 85, 85]).map(|s| s.ch).collect();
            assert_eq!(lit.len(), 5, "round {round}: {}", String::from_utf8_lossy(&lit));
        }
    }
}
