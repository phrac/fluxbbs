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
use std::time::{Duration, Instant};

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
    /// down through the area, catching the light.
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
}

fn all_cols() -> [u16; 2] {
    [1, 80]
}

fn every() -> f32 {
    8.0
}

fn delay() -> f32 {
    1.5
}

fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).sum::<i32>() < 24
}

/// A steady pseudo-random 0..1.
fn noise(a: i32, b: i32) -> f32 {
    crate::pixels::hash(a, b)
}

struct Target {
    x: u16,
    y: u16,
    base: Shown,
    effect: usize,
    last: Option<([u8; 3], [u8; 3])>,
}

pub struct Ambient {
    targets: Vec<Target>,
    effects: Vec<Effect>,
    start: Instant,
}

fn box_line(ch: u8) -> bool {
    (0xB3..=0xDA).contains(&ch)
}

fn blank(c: &Shown) -> bool {
    c.ch == b' ' && c.bg.iter().map(|&v| v as u32).sum::<u32>() < 30
}

/// `c` lit `k` of the way toward white.
fn lift(c: [u8; 3], k: f32) -> [u8; 3] {
    toward(c, [255, 255, 255], k)
}

fn toward(c: [u8; 3], to: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (c[i] as f32 + (to[i] as f32 - c[i] as f32) * k).round() as u8)
}

fn times(c: [u8; 3], f: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * f).round().clamp(0.0, 255.0) as u8)
}

impl Ambient {
    /// The cells the effects work on, as the screen shows them now.
    pub fn new(screen: &fluxbbs_term::Screen, effects: &[Effect]) -> Option<Ambient> {
        let mut targets = Vec::new();
        let mut taken = std::collections::HashSet::new();
        for (i, e) in effects.iter().enumerate() {
            let matches = hex(&e.matches);
            for y in e.rows[0].max(1)..=e.rows[1].min(screen.rows()) {
                for x in e.cols[0].max(1)..=e.cols[1].min(80) {
                    let Some(c) = screen.shown(x, y) else { continue };
                    let wanted = match e.kind.as_str() {
                        "glint" | "flicker" => !blank(&c),
                        "ripple" => box_line(c.ch),
                        "pulse" => matches.is_some_and(|m| near(c.fg, m) || near(c.bg, m)),
                        "dust" => true,
                        _ => false,
                    };
                    if wanted && taken.insert((x, y)) {
                        targets.push(Target { x, y, base: c, effect: i, last: None });
                    }
                }
            }
        }
        (!targets.is_empty()).then(|| Ambient { targets, effects: effects.to_vec(), start: Instant::now() })
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
        let (w, h) = ((e.cols[1] - e.cols[0] + 1) as f32, (e.rows[1] - e.rows[0] + 1) as f32);
        let n = ((w * h) / 7.0).ceil().max(1.0) as i32;
        (0..n)
            .map(|k| {
                let seed = i as i32 * 101 + k;
                let x = e.cols[0] as f32 + noise(seed, 1) * w + (t * 0.5 + k as f32).sin() * 1.2;
                let fall = t * (0.35 + noise(seed, 3) * 0.5);
                let y = e.rows[0] as f32 + (noise(seed, 2) * h + fall).rem_euclid(h);
                (x.round().clamp(e.cols[0] as f32, e.cols[1] as f32) as u16, y.floor() as u16)
            })
            .collect()
    }

    /// A cell's colors under its effect at `t`.
    fn shade(&self, g: &Target, t: f32, dust: &std::collections::HashSet<(u16, u16)>) -> ([u8; 3], [u8; 3]) {
        let e = &self.effects[g.effect];
        let b = g.base;
        let since = t - e.delay;
        let phase = since.rem_euclid(e.every.max(0.5));
        let q = |k: f32, steps: f32| (k * steps).round() / steps;
        match e.kind.as_str() {
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
                let f = 0.7 * noise(tick, g.effect as i32) + 0.3 * noise(tick + g.x as i32 * 31, g.y as i32);
                let f = q(1.0 + amount * (f - 0.5) * 2.0, 16.0);
                let warm = |c: [u8; 3]| {
                    let c = times(c, f);
                    if f > 1.0 { toward(c, [255, 210, 110], (f - 1.0) * 0.8) } else { c }
                };
                (warm(b.fg), warm(b.bg))
            }
            "pulse" => {
                let (Some(m), Some(to)) = (hex(&e.matches), hex(&e.tint)) else { return (b.fg, b.bg) };
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
            _ => (b.fg, b.bg),
        }
    }

    /// The ANSI for what's changed since the last frame.
    pub fn frame(&mut self) -> Vec<u8> {
        let t = self.start.elapsed().as_secs_f32();
        let dust: std::collections::HashSet<(u16, u16)> =
            self.effects.iter().enumerate().filter(|(_, e)| e.kind == "dust" && t >= e.delay).flat_map(|(i, e)| Self::motes(e, i, t)).collect();
        let colors: Vec<([u8; 3], [u8; 3])> = self.targets.iter().map(|g| self.shade(g, t, &dust)).collect();
        let mut out = Vec::new();
        let mut at: Option<(u16, u16)> = None;
        for (g, now) in self.targets.iter_mut().zip(colors) {
            let b = g.base;
            let unchanged = (b.fg, b.bg) == now;
            if g.last == Some(now) || (g.last.is_none() && unchanged) {
                g.last = Some(now);
                continue;
            }
            g.last = Some(now);
            if at != Some((g.x, g.y)) {
                out.extend(format!("\x1b[{};{}H", g.y, g.x).as_bytes());
            }
            let (fg, bg) = now;
            out.extend(format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]).as_bytes());
            out.push(b.ch);
            at = Some((g.x + 1, g.y));
        }
        out
    }
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
    /// Do this caller's screen and settings allow animating?
    pub(crate) fn animating(&self) -> bool {
        !self.plain()
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
        let idle = self.state.config().bbs.inactivity_seconds;
        let saver_after = if saver { self.state.config().terminal.screensaver_seconds } else { 0 };
        if amb.is_some() || saver_after > 0 {
            let waker = self.node.waker();
            loop {
                if self.key_waiting().await? {
                    break;
                }
                let stale = idle > 0 && since.elapsed() >= Duration::from_secs(idle);
                if stale || self.node.has_messages() {
                    *amb = None;
                    break;
                }
                if saver_after > 0 && since.elapsed() >= Duration::from_secs(saver_after) {
                    self.screensaver(since, idle).await?;
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
                    _ = tokio::time::sleep(self.idle_pause(70)) => {}
                    _ = waker.notified() => {
                        // Something for the caller: the usual wait shows it.
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

    /// The screen saver: digital rain in the board's colors, its name
    /// showing through as the drops pass, until Enter or Esc (or
    /// something for the caller, or the inactivity timeout).
    async fn screensaver(&mut self, since: tokio::time::Instant, idle: u64) -> io::Result<()> {
        tracing::debug!("{}: screen saver", self.user.handle);
        let rows = self.screen.rows();
        let name = self.state.config().bbs.name.clone();
        let mut rain = Rain::new(rows.saturating_sub(1).max(1), &name);
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.screen.write_bytes(b"\x1b[?25l");
        let notice = crate::editor::strip_colours(self.strings().get("ScreenSaverExit"));
        let col = (80usize.saturating_sub(notice.chars().count()) / 2 + 1) as u16;
        let mut bar = format!("\x1b[{rows};1H\x1b[0m\x1b[K\x1b[{rows};{col}H\x1b[38;2;255;150;235;48;2;45;5;55m").into_bytes();
        bar.extend(fluxbbs_term::cp437::encode(&notice));
        bar.extend(b"\x1b[0m");
        self.screen.write_bytes(&bar);
        self.cursor_asks = self.cursor_asks.min(self.cursor_reports);
        let waker = self.node.waker();
        let mut last = std::time::Instant::now();
        loop {
            if self.key_waiting().await? {
                match self.read_key().await? {
                    crate::input::Key::Enter | crate::input::Key::Esc => break,
                    _ => continue,
                }
            }
            if (idle > 0 && since.elapsed() >= Duration::from_secs(idle)) || self.node.has_messages() {
                break;
            }
            let dt = last.elapsed().as_secs_f32().min(0.25);
            last = std::time::Instant::now();
            let out = rain.step(dt);
            if !out.is_empty() {
                self.screen.write_bytes(&out);
                self.screen.write_bytes(b"\x1b[0m");
                self.screen.goto(1, rows);
                // One frame on the way at a time, on a slow line.
                self.sync_output(Duration::from_secs(3), 1).await?;
                self.flush().await?;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(80)) => {}
                _ = waker.notified() => break,
            }
        }
        self.screen.write_bytes(b"\x1b[0m\x1b[?25h");
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.full_screen_done();
        Ok(())
    }
}

// ---- the screen saver ------------------------------------------------------

/// The rain's colors, the head of a drop first, in the board's pinks.
const RAIN: [[u8; 3]; 6] = [[255, 235, 255], [255, 120, 240], [235, 60, 205], [165, 25, 160], [95, 10, 100], [50, 5, 60]];

/// What falls: digits, letters, and CP437's Greek and math signs.
fn rain_glyph(r: f32) -> u8 {
    const SET: &[u8] = b"0123456789ABCDEFGHJKLMNPQRSTUVWXYZ0123456789\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf7";
    SET[((r * SET.len() as f32) as usize).min(SET.len() - 1)]
}

#[derive(Clone, Copy)]
struct Drop {
    head: f32,
    speed: f32,
    len: f32,
    wait: f32,
}

struct Rain {
    rows: u16,
    drops: Vec<Drop>,
    glyphs: Vec<u8>,
    sent: Vec<Option<(u8, u8)>>,
    /// The name, across the middle: where each letter is, and when a
    /// drop last lit it.
    word: Vec<(u16, u16, u8, f32)>,
    t: f32,
    seed: u32,
}

impl Rain {
    fn new(rows: u16, name: &str) -> Rain {
        let mut seed = 0x9E37_79B9u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let drops = (0..80)
            .map(|_| Drop { head: -rnd() * rows as f32, speed: 7.0 + rnd() * 13.0, len: 6.0 + rnd() * 14.0, wait: rnd() * 4.0 })
            .collect();
        let glyphs = (0..80 * rows as usize).map(|_| rain_glyph(rnd())).collect();
        let text: Vec<u8> = fluxbbs_term::cp437::encode(name.to_lowercase().split("").collect::<Vec<_>>().join(" ").trim());
        let x0 = (80usize.saturating_sub(text.len()) / 2 + 1) as u16;
        let word = text.iter().enumerate().filter(|(_, c)| **c != b' ').map(|(i, &c)| (x0 + i as u16, rows / 2, c, -100.0)).collect();
        Rain { rows, drops, glyphs, sent: vec![None; 80 * rows as usize], word, t: 0.0, seed: 0x1234_5678 }
    }

    fn rnd(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Move the rain on `dt` seconds; the ANSI for what changed.
    fn step(&mut self, dt: f32) -> Vec<u8> {
        self.t += dt;
        let rows = self.rows as f32;
        for x in 0..80 {
            let mut d = self.drops[x];
            if d.wait > 0.0 {
                d.wait -= dt;
            } else {
                let before = d.head.floor() as i32;
                d.head += d.speed * dt;
                // A new glyph in each cell the head reaches.
                for y in (before + 1).max(0)..=(d.head.floor() as i32).min(self.rows as i32 - 1) {
                    let r = self.rnd();
                    self.glyphs[y as usize * 80 + x] = rain_glyph(r);
                    for w in &mut self.word {
                        if w.0 as usize == x + 1 && w.1 as i32 == y + 1 {
                            w.3 = self.t;
                        }
                    }
                }
                if d.head - d.len > rows {
                    d = Drop { head: 0.0, speed: 7.0 + self.rnd() * 13.0, len: 6.0 + self.rnd() * 14.0, wait: self.rnd() * 3.0 };
                }
            }
            self.drops[x] = d;
        }
        // Now and then a glyph in a trail changes.
        for _ in 0..6 {
            let i = (self.rnd() * self.glyphs.len() as f32) as usize % self.glyphs.len();
            let r = self.rnd();
            self.glyphs[i] = rain_glyph(r);
        }
        let mut out = Vec::new();
        let mut at: Option<(u16, u16)> = None;
        for y in 0..self.rows {
            for x in 0..80u16 {
                let d = self.drops[x as usize];
                let behind = d.head - (y as f32 + 0.5);
                let mut level = if d.wait <= 0.0 && behind >= 0.0 && behind < d.len {
                    if behind < 1.0 { Some(0u8) } else { Some((1.0 + (behind - 1.0) / (d.len - 1.0).max(1.0) * 5.0).min(5.0) as u8) }
                } else {
                    None
                };
                let mut glyph = self.glyphs[y as usize * 80 + x as usize];
                // The name shows where a drop has just passed over it.
                if let Some(w) = self.word.iter().find(|w| w.0 == x + 1 && w.1 == y + 1) {
                    let age = self.t - w.3;
                    if age < 4.5 {
                        glyph = w.2;
                        let lit = if age < 2.5 { 1 } else { 2 + ((age - 2.5) / 0.7) as u8 };
                        level = Some(level.map_or(lit, |l| l.min(lit)).min(5));
                    }
                }
                let cell = level.map(|l| (glyph, l));
                let i = y as usize * 80 + x as usize;
                if self.sent[i] == cell || (self.sent[i].is_none() && cell.is_none()) {
                    continue;
                }
                self.sent[i] = cell;
                if at != Some((x, y)) {
                    out.extend(format!("\x1b[{};{}H", y + 1, x + 1).as_bytes());
                }
                match cell {
                    Some((g, l)) => {
                        let c = RAIN[l as usize];
                        out.extend(format!("\x1b[38;2;{};{};{};48;2;0;0;0m", c[0], c[1], c[2]).as_bytes());
                        out.push(g);
                    }
                    None => out.extend(b"\x1b[48;2;0;0;0m "),
                }
                at = Some((x + 1, y));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxbbs_term::{Charset, Screen};

    fn fx(kind: &str, rows: [u16; 2]) -> Effect {
        Effect { kind: kind.into(), rows, cols: all_cols(), every: 8.0, delay: 1.5, amount: 0.0, tint: String::new(), matches: String::new() }
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
    fn a_pulse_lights_only_its_color() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;60;10;8;48;2;200;200;190m\xDC\x1b[38;2;200;200;190m\xDC");
        let e = Effect { matches: "#3c0a08".into(), tint: "#ff3018".into(), ..fx("pulse", [1, 1]) };
        let a = Ambient::new(&s, &[e]).unwrap();
        assert_eq!(a.targets.len(), 1, "only the cell with the eye's color");
        let (fg, bg) = a.shade(&a.targets[0], 1.5 + 4.0, &Default::default());
        assert!(fg[0] > 200 && bg == [200, 200, 190], "the eye glows, the bone doesn't: {fg:?} {bg:?}");
    }

    #[test]
    fn dust_stays_in_its_area() {
        let e = Effect { cols: [30, 40], ..fx("dust", [5, 15]) };
        for t in [0.0, 3.3, 17.0] {
            assert!(Ambient::motes(&e, 0, t).iter().all(|&(x, y)| (30..=40).contains(&x) && (5..=15).contains(&y)));
        }
    }
}
