//! New: more screen savers beside the digital rain (`ambient`): a warp
//! starfield and a demoscene fire, each with the board's message (or
//! name) in the middle, set in the saver's font if it has one. Like the
//! rain, a frame sends only the cells that changed, in 24-bit color.
//! Callers pick one in their configuration (`X`); the board's default is
//! `[terminal] screensaver_style`.

use std::io;
use std::time::Duration;

use crate::ambient::{lift, rain_glyph, times};
use crate::session::Session;
use crate::tdf::Cell as FontCell;

/// A cell as shown: its character, foreground and background.
type Shown = (u8, [u8; 3], [u8; 3]);

/// A cell of the message: where it goes (1-based), what it shows, and
/// its own color if it's set in a font.
type WordCell = (u16, u16, u8, Option<[u8; 3]>);

const BLACK: [u8; 3] = [0, 0, 0];

/// The message's cells, centered across the screen with its middle at
/// `at` of the way down: set in a font (`word`), or as spaced plain
/// letters.
fn place(rows: u16, at: f32, plain: &str, word: Option<(Vec<FontCell>, u16, u16)>) -> Vec<WordCell> {
    let mid = (rows as f32 * at) as u16;
    match word {
        Some((cells, w, h)) if !cells.is_empty() => {
            let x0 = 80u16.saturating_sub(w) / 2 + 1;
            let y0 = mid.saturating_sub(h / 2).max(1);
            cells
                .iter()
                .filter(|c| y0 + c.y <= rows && x0 + c.x <= 80)
                .map(|c| (x0 + c.x, y0 + c.y, c.ch, Some(crate::tdf::rgb(c.fg))))
                .collect()
        }
        _ => {
            let spaced = plain.to_lowercase().chars().map(String::from).collect::<Vec<_>>().join(" ");
            let text = fluxbbs_term::cp437::encode(spaced.trim());
            let x0 = (80usize.saturating_sub(text.len()) / 2 + 1) as u16;
            text.iter()
                .enumerate()
                .filter(|(_, c)| **c != b' ')
                .map(|(i, &c)| (x0 + i as u16, mid.max(1), c, None))
                .collect()
        }
    }
}

fn scale(c: [u8; 3], k: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * k).round().clamp(0.0, 255.0) as u8)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// The ANSI for the cells of `grid` that differ from `sent` (and now
/// are): rows of 80, `None` a black space.
fn emit(grid: &[Option<Shown>], sent: &mut [Option<Shown>], rows: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at: Option<(u16, u16)> = None;
    let mut colors: Option<([u8; 3], [u8; 3])> = None;
    for y in 0..rows {
        for x in 0..80u16 {
            let i = y as usize * 80 + x as usize;
            let cell = grid[i];
            if sent[i] == cell {
                continue;
            }
            sent[i] = cell;
            if at != Some((x, y)) {
                out.extend(format!("\x1b[{};{}H", y + 1, x + 1).as_bytes());
            }
            let (ch, fg, bg) = cell.unwrap_or((b' ', BLACK, BLACK));
            if colors != Some((fg, bg)) {
                out.extend(format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]).as_bytes());
                colors = Some((fg, bg));
            }
            out.push(ch);
            at = Some((x + 1, y));
        }
    }
    out
}

/// A small steady generator: savers live across awaits, where the
/// thread's own generator can't go.
struct Rnd(u32);

impl Rnd {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

// ---- the starfield --------------------------------------------------------

/// Stars streaming out of the middle of the screen, as at warp speed,
/// the nearest drawing a short trail; the message holds in the middle
/// over them, fading in, then gently pulsing.
pub struct Stars {
    rows: u16,
    /// Each star's place across, up and away (z: 1 far, 0 here).
    stars: Vec<[f32; 3]>,
    word: Vec<WordCell>,
    tint: [u8; 3],
    sent: Vec<Option<Shown>>,
    t: f32,
    rnd: Rnd,
}

impl Stars {
    pub fn new(rows: u16, plain: &str, word: Option<(Vec<FontCell>, u16, u16)>, palette: [[u8; 3]; 6]) -> Stars {
        let mut rnd = Rnd(0x51A2_7E35);
        let stars = (0..170).map(|_| [rnd.next() * 2.0 - 1.0, rnd.next() * 2.0 - 1.0, 0.05 + rnd.next() * 0.95]).collect();
        Stars { rows, stars, word: place(rows, 0.5, plain, word), tint: palette[0], sent: vec![None; 80 * rows as usize], t: 0.0, rnd }
    }

    /// Where a star at (x, y, z) shows, if on screen (0-based).
    fn project(&self, s: [f32; 3]) -> Option<(usize, usize)> {
        let (cx, cy) = (40.0, self.rows as f32 / 2.0);
        let (px, py) = (cx + s[0] / s[2] * 16.0, cy + s[1] / s[2] * 7.0);
        (px >= 0.0 && py >= 0.0 && px < 80.0 && py < self.rows as f32).then_some((px as usize, py as usize))
    }

    pub fn step(&mut self, dt: f32) -> Vec<u8> {
        self.t += dt;
        let mut grid: Vec<Option<Shown>> = vec![None; 80 * self.rows as usize];
        for i in 0..self.stars.len() {
            let mut s = self.stars[i];
            s[2] -= dt * 0.42;
            if s[2] <= 0.03 || self.project(s).is_none() {
                s = [self.rnd.next() * 2.0 - 1.0, self.rnd.next() * 2.0 - 1.0, 1.0];
            }
            self.stars[i] = s;
            let near = (1.0 - s[2]).clamp(0.0, 1.0);
            let color = mix(mix([40, 40, 60], self.tint, 0.25), [255, 255, 255], near);
            // The nearest leave a dim trail back toward the middle.
            if s[2] < 0.3
                && let Some((x, y)) = self.project([s[0], s[1], s[2] + 0.07])
            {
                grid[y * 80 + x] = Some((0xFA, scale(color, 0.45), BLACK));
            }
            if let Some((x, y)) = self.project(s) {
                let ch = match s[2] {
                    z if z > 0.7 => b'.',
                    z if z > 0.4 => 0xFA,
                    z if z > 0.18 => b'+',
                    _ => b'*',
                };
                grid[y * 80 + x] = Some((ch, color, BLACK));
            }
        }
        // The message, over the stars: fading in, then breathing.
        let k = (self.t / 1.5).min(1.0) * (0.88 + 0.12 * (self.t * 1.8).sin());
        for &(x, y, ch, own) in &self.word {
            let i = (y as usize - 1) * 80 + x as usize - 1;
            grid[i] = Some((ch, scale(own.unwrap_or(self.tint), k), BLACK));
        }
        emit(&grid, &mut self.sent, self.rows)
    }
}

// ---- the fire -----------------------------------------------------------

/// The demoscene fire: heat rising off the bottom of the screen in
/// half-block pixels, cooling as it climbs, from black through red and
/// orange to a yellow-white core; the message hangs above it, lit by
/// the flicker.
pub struct Fire {
    rows: u16,
    /// Heat, 0 to 1, two pixels to a cell down, 80 across.
    heat: Vec<f32>,
    /// The first pixel row the fire reaches.
    top: usize,
    word: Vec<WordCell>,
    tint: [u8; 3],
    sent: Vec<Option<Shown>>,
    since: f32,
    t: f32,
    rnd: Rnd,
}

/// Fire's colors by heat.
fn flame(v: f32) -> [u8; 3] {
    const STOPS: [(f32, [u8; 3]); 6] =
        [(0.0, [0, 0, 0]), (0.2, [60, 0, 0]), (0.4, [170, 20, 0]), (0.6, [240, 90, 0]), (0.8, [255, 190, 40]), (1.0, [255, 250, 200])];
    // In steps, so cells that barely change aren't sent again.
    let v = (v.clamp(0.0, 1.0) * 14.0).round() / 14.0;
    for w in STOPS.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if v <= b {
            return mix(ca, cb, (v - a) / (b - a));
        }
    }
    STOPS[5].1
}

impl Fire {
    pub fn new(rows: u16, plain: &str, word: Option<(Vec<FontCell>, u16, u16)>, palette: [[u8; 3]; 6]) -> Fire {
        let h = rows as usize * 2;
        let top = h - (h as f32 * 0.55) as usize;
        Fire {
            rows,
            heat: vec![0.0; 80 * h],
            top,
            word: place(rows, 0.24, plain, word),
            tint: palette[0],
            sent: vec![None; 80 * rows as usize],
            since: 0.0,
            t: 0.0,
            rnd: Rnd(0x0F17_E5ED),
        }
    }

    /// One tick: fresh heat along the bottom, the rest rising from below,
    /// spreading and cooling.
    fn tick(&mut self) {
        let h = self.rows as usize * 2;
        for x in 0..80 {
            let r = self.rnd.next();
            self.heat[(h - 1) * 80 + x] = if r < 0.15 { 0.25 } else { 0.78 + r * 0.22 };
        }
        let cool = 1.15 / (h - self.top) as f32;
        for y in self.top..h - 1 {
            for x in 0..80 {
                let below = |dx: usize, dy: usize| self.heat[((y + dy).min(h - 1)) * 80 + (x + dx) % 80];
                let v = (below(79, 1) + below(0, 1) + below(1, 1) + below(0, 2)) / 4.0;
                let r = self.rnd.next();
                self.heat[y * 80 + x] = (v - cool * (0.4 + r * 1.2)).max(0.0);
            }
        }
    }

    pub fn step(&mut self, dt: f32) -> Vec<u8> {
        self.t += dt;
        self.since += dt;
        // At 24 ticks a second, whatever the frame rate, a few at most.
        let mut ticks = 0;
        while self.since >= 1.0 / 24.0 && ticks < 3 {
            self.tick();
            self.since -= 1.0 / 24.0;
            ticks += 1;
        }
        self.since = self.since.min(1.0 / 24.0);
        let mut grid: Vec<Option<Shown>> = vec![None; 80 * self.rows as usize];
        for row in 0..self.rows as usize {
            for x in 0..80 {
                let (up, down) = (flame(self.heat[row * 2 * 80 + x]), flame(self.heat[(row * 2 + 1) * 80 + x]));
                if up != BLACK || down != BLACK {
                    grid[row * 80 + x] = Some((0xDF, up, down));
                }
            }
        }
        // The message, lit by the fire's flicker.
        let k = (self.t / 1.2).min(1.0) * (0.82 + 0.18 * ((self.t * 9.0).sin() * (self.t * 3.7).sin()).abs());
        for &(x, y, ch, own) in &self.word {
            let i = (y as usize - 1) * 80 + x as usize - 1;
            grid[i] = Some((ch, scale(mix(own.unwrap_or(self.tint), [255, 200, 120], 0.15), k), BLACK));
        }
        emit(&grid, &mut self.sent, self.rows)
    }
}

impl Session {
    /// New: the screen saver on demand (Backspace on a lightbar menu),
    /// for callers whose menus animate; false if it can't run here.
    pub(crate) async fn screensaver_now(&mut self) -> io::Result<bool> {
        if !self.animating() {
            return Ok(false);
        }
        let idle = self.state.config().bbs.inactivity_seconds;
        self.screensaver(tokio::time::Instant::now(), idle).await?;
        Ok(true)
    }

    /// The screen saver: digital rain in the colors of the screen it
    /// covers, the board's name showing through as the drops pass (set
    /// in a TheDraw font, if the board names one), until Enter or Esc
    /// (or something for the caller, or the inactivity timeout).
    pub(crate) async fn screensaver(&mut self, since: tokio::time::Instant, idle: u64) -> io::Result<()> {
        tracing::debug!("{}: screen saver", self.user.handle);
        let rows = self.screen.rows();
        let cfg = self.state.config();
        // New: the saver spells out a custom message if set, else the
        // board's name; where that won't fit 80 columns (in its font, or
        // as spaced plain letters), the short name, then the super short.
        let msg = cfg.terminal.screensaver_message.trim();
        let text = if msg.is_empty() { cfg.bbs.name.clone() } else { msg.to_string() };
        let tries = cfg.bbs.fallbacks(&text);
        let palette = rain_palette(&self.screen);
        let font = cfg.terminal.screensaver_font.trim();
        let word = if font.is_empty() {
            None
        } else {
            let fonts = crate::tdf::read(&cfg.resolve(std::path::Path::new(font)));
            let face = cfg.terminal.screensaver_face.trim();
            fonts.iter().find(|f| face.is_empty() || f.name.eq_ignore_ascii_case(face)).map(|f| f.set_fitting(&tries, 80).0)
        };
        let plain = tries.iter().find(|t| t.chars().count() * 2 <= 80).or(tries.last()).cloned().unwrap_or_default();
        // New: the caller's own saver, else the board's; random (blank
        // too) is a different one each time.
        let style = saver_style(&self.user.saver, &cfg.terminal.screensaver_style);
        let lines = rows.saturating_sub(1).max(1);
        let mut saver = match style {
            "stars" => Saver::Stars(crate::savers::Stars::new(lines, &plain, word, palette)),
            "fire" => Saver::Fire(crate::savers::Fire::new(lines, &plain, word, palette)),
            _ => Saver::Rain(Box::new(Rain::new(lines, &plain, word, palette))),
        };
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
            let out = saver.step(dt);
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

/// The rain's colors, the head of a drop first, in the board's pinks:
/// what the rain falls in where the screen it covers has no color.
const RAIN: [[u8; 3]; 6] = [[255, 235, 255], [255, 120, 240], [235, 60, 205], [165, 25, 160], [95, 10, 100], [50, 5, 60]];

/// The rain's colors from what's on screen: the color most of the
/// screen's colored cells are drawn in, from a bright head down to a
/// dark tail; the pinks where nothing on screen has a color.
fn rain_palette(screen: &fluxbbs_term::Screen) -> [[u8; 3]; 6] {
    let mut counts: std::collections::HashMap<[u8; 3], u32> = std::collections::HashMap::new();
    for y in 1..=screen.rows() {
        for x in 1..=80 {
            let Some(c) = screen.shown(x, y) else { continue };
            for col in [c.fg, c.bg] {
                let (hi, lo) = (*col.iter().max().unwrap_or(&0), *col.iter().min().unwrap_or(&0));
                if hi >= 100 && hi - lo >= 70 {
                    *counts.entry(col).or_default() += 1;
                }
            }
        }
    }
    let Some((base, _)) = counts.into_iter().max_by_key(|&(c, n)| (n, c)) else { return RAIN };
    let hi = *base.iter().max().unwrap_or(&1) as f32;
    let base = times(base, 235.0 / hi.max(1.0));
    [lift(base, 0.85), lift(base, 0.35), base, times(base, 0.7), times(base, 0.4), times(base, 0.2)]
}


#[derive(Clone, Copy)]
struct Drop {
    head: f32,
    speed: f32,
    len: f32,
    wait: f32,
}

/// A cell of the name: column, row, character, its own color if set
/// in a font, and when a drop last lit it (`NEVER`: it's not showing).
type RainCell = (u16, u16, u8, Option<[u8; 3]>, f32);
const NEVER: f32 = -1.0;

/// The screen saver running: the rain, or one of `savers`.
enum Saver {
    Rain(Box<Rain>),
    Stars(crate::savers::Stars),
    Fire(crate::savers::Fire),
}

impl Saver {
    fn step(&mut self, dt: f32) -> Vec<u8> {
        match self {
            Saver::Rain(r) => r.step(dt),
            Saver::Stars(s) => s.step(dt),
            Saver::Fire(f) => f.step(dt),
        }
    }
}

/// The saver to run: the caller's pick if they made one, else the
/// board's; `random` (or nothing set) picks one of them afresh.
fn saver_style(user: &str, board: &str) -> &'static str {
    const ALL: [&str; 3] = ["rain", "stars", "fire"];
    let want = if user.is_empty() { board } else { user };
    match ALL.iter().find(|s| **s == want) {
        Some(s) => s,
        None => {
            use rand::RngExt;
            ALL[rand::rng().random_range(0..ALL.len())]
        }
    }
}

struct Rain {
    rows: u16,
    drops: Vec<Drop>,
    glyphs: Vec<u8>,
    sent: Vec<Option<(u8, [u8; 3])>>,
    /// The name, across the middle: where each of its cells is, what it
    /// shows, its own color if set in a font, and when a drop last lit
    /// it.
    word: Vec<RainCell>,
    palette: [[u8; 3]; 6],
    /// The name fills in as drops cross its cells; full, it holds a
    /// moment, then the drops take it away again, and so on.
    filling: bool,
    filled_at: f32,
    t: f32,
    seed: u32,
}

impl Rain {
    /// `word`: the name set in a font (its cells, width and height), or
    /// None for plain spaced letters.
    fn new(rows: u16, name: &str, word: Option<(Vec<crate::tdf::Cell>, u16, u16)>, palette: [[u8; 3]; 6]) -> Rain {
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
        let word = match word {
            Some((cells, w, h)) if !cells.is_empty() => {
                let (x0, y0) = ((80u16.saturating_sub(w) / 2 + 1), (rows.saturating_sub(h) / 2 + 1).max(1));
                cells
                    .iter()
                    .filter(|c| y0 as u32 + c.y as u32 <= rows as u32 && x0 as u32 + c.x as u32 <= 80)
                    .map(|c| (x0 + c.x, y0 + c.y, c.ch, Some(crate::tdf::rgb(c.fg)), NEVER))
                    .collect()
            }
            _ => {
                let text: Vec<u8> = fluxbbs_term::cp437::encode(name.to_lowercase().split("").collect::<Vec<_>>().join(" ").trim());
                let x0 = (80usize.saturating_sub(text.len()) / 2 + 1) as u16;
                text.iter().enumerate().filter(|(_, c)| **c != b' ').map(|(i, &c)| (x0 + i as u16, rows / 2, c, None, NEVER)).collect()
            }
        };
        Rain { rows, drops, glyphs, sent: vec![None; 80 * rows as usize], word, palette, filling: true, filled_at: 0.0, t: 0.0, seed: 0x1234_5678 }
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
                }
                if d.head - d.len > rows {
                    d = Drop { head: 0.0, speed: 7.0 + self.rnd() * 13.0, len: 6.0 + self.rnd() * 14.0, wait: self.rnd() * 3.0 };
                }
            }
            self.drops[x] = d;
        }
        // Full: hold it, then let the drops take it; empty: fill again.
        if self.filling && self.word.iter().all(|w| w.4 >= 0.0) {
            self.filling = false;
            self.filled_at = self.t;
        } else if !self.filling && self.word.iter().all(|w| w.4 < 0.0) {
            self.filling = true;
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
                // The name assembles out of the rain: a cell of it stays
                // once a drop has passed over it (until the drops take it
                // away again), in its own color if it's set in a font
                // (brightening again under each drop that follows), else
                // in the rain's, fading from bright to the tail's tone.
                let mut own = None;
                let (filling, hold_over, t) = (self.filling, self.t - self.filled_at > 3.0, self.t);
                if let Some(w) = self.word.iter_mut().find(|w| w.0 == x + 1 && w.1 == y + 1) {
                    // Under a drop: filling, the cell comes on (and
                    // brightens again under each later drop); emptying,
                    // once the full name has held a moment, it goes.
                    if level.is_some() {
                        if filling {
                            w.4 = t;
                        } else if hold_over {
                            w.4 = NEVER;
                        }
                    }
                    let age = t - w.4;
                    if w.4 >= 0.0 {
                        glyph = w.2;
                        let lit = if age < 2.5 { 1 } else { (2 + ((age - 2.5) / 0.7) as u8).min(3) };
                        level = Some(level.map_or(lit, |l| l.min(lit)));
                        if let Some(c) = w.3 {
                            own = Some(if age < 0.4 { lift(c, 0.6) } else { c });
                        }
                    }
                }
                let cell = level.map(|l| (glyph, own.unwrap_or(self.palette[l as usize])));
                let i = y as usize * 80 + x as usize;
                if self.sent[i] == cell || (self.sent[i].is_none() && cell.is_none()) {
                    continue;
                }
                self.sent[i] = cell;
                if at != Some((x, y)) {
                    out.extend(format!("\x1b[{};{}H", y + 1, x + 1).as_bytes());
                }
                match cell {
                    Some((g, c)) => {
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

    const PAL: [[u8; 3]; 6] = [[255, 120, 230], [200, 90, 180], [170, 60, 150], [120, 40, 110], [70, 20, 60], [35, 10, 30]];

    fn shown(out: &[u8]) -> String {
        String::from_utf8_lossy(out).to_string()
    }

    #[test]
    fn the_starfield_draws_its_message_and_keeps_its_stars_on_screen() {
        let mut s = Stars::new(23, "disflux", None, PAL);
        let first = s.step(0.1);
        assert!(!first.is_empty());
        for _ in 0..200 {
            s.step(0.05);
        }
        assert!(s.stars.iter().all(|st| st[2] > 0.0 && st[2] <= 1.0));
        assert!(shown(&first).contains('d') && shown(&first).contains('x'), "the name is there");
        // Nothing changed: nothing sent.
        let mut still = Stars::new(23, "disflux", None, PAL);
        still.step(0.0);
        assert!(still.step(0.0).len() < first.len());
    }

    #[test]
    fn the_fire_burns_upward_and_cools() {
        let mut f = Fire::new(23, "disflux", None, PAL);
        for _ in 0..60 {
            f.step(1.0 / 24.0);
        }
        let h = 46;
        let row_heat = |y: usize| (0..80).map(|x| f.heat[y * 80 + x]).sum::<f32>() / 80.0;
        assert!(row_heat(h - 2) > row_heat(f.top + 2), "hotter at the bottom than the top");
        assert!(f.heat.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!((0..f.top * 80).all(|i| f.heat[i] == 0.0), "nothing above where the fire reaches");
        assert_eq!(flame(0.0), BLACK);
        assert!(flame(1.0)[0] == 255);
    }

    #[test]
    fn a_font_word_is_centered() {
        let cells = vec![FontCell { x: 0, y: 0, ch: b'A', fg: 13, bg: 0 }, FontCell { x: 9, y: 1, ch: b'B', fg: 13, bg: 0 }];
        let w = place(24, 0.5, "x", Some((cells, 10, 2)));
        assert_eq!(w[0].0, 36, "a 10-wide word starts at column 36");
        assert_eq!(w[1].1 - w[0].1, 1);
    }

    #[test]
    fn the_callers_saver_wins_then_the_boards_then_chance() {
        assert_eq!(saver_style("fire", "stars"), "fire");
        assert_eq!(saver_style("", "stars"), "stars");
        for _ in 0..20 {
            assert!(["rain", "stars", "fire"].contains(&saver_style("", "")));
            assert!(["rain", "stars", "fire"].contains(&saver_style("random", "rain")));
        }
    }

    #[test]
    fn the_name_fills_from_the_rain_then_empties() {
        let cells: Vec<crate::tdf::Cell> = (0..30u16).map(|i| crate::tdf::Cell { x: i % 10, y: i / 10, ch: b'#', fg: 10, bg: 0 }).collect();
        let mut rain = Rain::new(24, "x", Some((cells, 10, 3)), RAIN);
        let lit = |r: &Rain| r.word.iter().filter(|w| w.4 >= 0.0).count();
        let mut history = Vec::new();
        for i in 0..(40.0 / 0.08) as usize {
            rain.step(0.08);
            if i % 25 == 0 {
                history.push((lit(&rain), rain.filling));
            }
        }
        let full_at = history.iter().position(|&(n, _)| n == 30);
        assert!(full_at.is_some_and(|i| i <= 6), "fills within a dozen seconds: {history:?}");
        let after = &history[full_at.unwrap()..];
        let emptied = after.iter().position(|&(n, _)| n < 10);
        assert!(emptied.is_some(), "then the drops take it away: {history:?}");
        assert!(after[emptied.unwrap()..].iter().any(|&(n, _)| n == 30), "and it fills again: {history:?}");
    }

    #[test]
    fn the_board_name_in_a_real_font_fills_in_seconds() {
        let path = std::path::Path::new("/home/derek/code/disflux_bbs/art/fonts/AMNESIA.TDF");
        if !path.exists() {
            return;
        }
        let fonts = crate::tdf::read(path);
        let word = fonts[0].set("disflux");
        let total = word.0.len();
        let mut rain = Rain::new(24, "disflux", Some(word), RAIN);
        let mut lit_at = Vec::new();
        for i in 0..(30.0 / 0.08) as usize {
            rain.step(0.08);
            if i % 25 == 24 {
                lit_at.push(rain.word.iter().filter(|w| w.4 >= 0.0).count());
            }
        }
        println!("of {total} cells, lit every 2 s: {lit_at:?}");
        assert!(lit_at[4] > total / 2, "half the name within 10 s: {lit_at:?}");
    }

}
