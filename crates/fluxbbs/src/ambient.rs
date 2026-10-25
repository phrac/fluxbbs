//! New: a menu's screen, animated while it waits for a key (`[[effect]]`
//! in the menu's TOML): a glint crossing the art now and then, a pulse of
//! light running along the box lines. The effects work on what's on
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
    /// runs along the box-drawing lines.
    pub kind: String,
    /// The screen rows it covers, first and last (1-based).
    pub rows: [u16; 2],
    /// Seconds from one to the next, and before the first.
    #[serde(default = "every")]
    pub every: f32,
    #[serde(default = "delay")]
    pub delay: f32,
}

fn every() -> f32 {
    8.0
}

fn delay() -> f32 {
    1.5
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
    c.map(|v| (v as f32 + (255.0 - v as f32) * k).round() as u8)
}

impl Ambient {
    /// The cells the effects work on, as the screen shows them now.
    pub fn new(screen: &fluxbbs_term::Screen, effects: &[Effect]) -> Option<Ambient> {
        let mut targets = Vec::new();
        let mut taken = std::collections::HashSet::new();
        for (i, e) in effects.iter().enumerate() {
            for y in e.rows[0].max(1)..=e.rows[1].min(screen.rows()) {
                for x in 1..=80 {
                    let Some(c) = screen.shown(x, y) else { continue };
                    let wanted = match e.kind.as_str() {
                        "glint" => !blank(&c),
                        "ripple" => box_line(c.ch),
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

    /// How lit cell (`x`, `y`) of effect `e` is at `t`, 0 to 1, in steps.
    fn light(&self, e: &Effect, x: u16, y: u16, t: f32) -> f32 {
        if t < e.delay {
            return 0.0;
        }
        let phase = (t - e.delay).rem_euclid(e.every.max(1.0));
        let k = match e.kind.as_str() {
            "glint" => {
                // A slanted band from the left edge past the right.
                let run = 1.6;
                if phase > run {
                    return 0.0;
                }
                let at = -12.0 + phase / run * 104.0;
                let d = x as f32 + (y - e.rows[0]) as f32 * 1.6 - at;
                0.7 * (-(d / 2.6).powi(2)).exp()
            }
            "ripple" => {
                let run = 2.4;
                if phase > run {
                    return 0.0;
                }
                let at = -8.0 + phase / run * 100.0;
                let d = x as f32 + (y - e.rows[0]) as f32 * 0.8 - at;
                0.75 * (-(d / 3.2).powi(2)).exp()
            }
            _ => 0.0,
        };
        (k * 8.0).round() / 8.0
    }

    /// The ANSI for what's changed since the last frame.
    pub fn frame(&mut self) -> Vec<u8> {
        let t = self.start.elapsed().as_secs_f32();
        let lights: Vec<f32> = self.targets.iter().map(|g| self.light(&self.effects[g.effect], g.x, g.y, t)).collect();
        let mut out = Vec::new();
        let mut at: Option<(u16, u16)> = None;
        for (g, k) in self.targets.iter_mut().zip(lights) {
            let b = g.base;
            let glint = self.effects[g.effect].kind == "glint";
            let fg = if b.ch == b' ' { b.fg } else { lift(b.fg, k) };
            let bg = if glint && b.bg.iter().map(|&v| v as u32).sum::<u32>() >= 30 { lift(b.bg, k * 0.5) } else { b.bg };
            let now = (fg, bg);
            if g.last == Some(now) || (g.last.is_none() && k == 0.0) {
                g.last = Some(now);
                continue;
            }
            g.last = Some(now);
            if at != Some((g.x, g.y)) {
                out.extend(format!("\x1b[{};{}H", g.y, g.x).as_bytes());
            }
            out.extend(format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]).as_bytes());
            out.push(b.ch);
            at = Some((g.x + 1, g.y));
        }
        out
    }
}

impl Session {
    /// Do this caller's screen and settings allow animating?
    pub(crate) fn animating(&self) -> bool {
        !self.plain() && self.user.animate != "off" && (self.screen.truecolor() || self.screen.charset() == fluxbbs_term::Charset::Utf8)
    }

    /// A key, the screen animating while it waits. Node messages, pages
    /// and the inactivity timeout work as ever: when something's for the
    /// caller, the animation stops and the usual wait takes over.
    pub(crate) async fn read_key_ambient(&mut self, amb: &mut Option<Ambient>) -> io::Result<crate::input::Key> {
        let since = tokio::time::Instant::now();
        let idle = self.state.config().bbs.inactivity_seconds;
        if let Some(a) = amb.as_mut() {
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
                let out = a.frame();
                if !out.is_empty() {
                    let (x, y, color) = (self.screen.where_x(), self.screen.where_y(), self.screen.color());
                    self.screen.write_bytes(&out);
                    self.screen.write_bytes(b"\x1b[0m");
                    self.screen.goto(x, y);
                    self.screen.set_color(color);
                    self.flush().await?;
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(70)) => {}
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
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxbbs_term::{Charset, Screen};

    #[test]
    fn effects_touch_only_their_cells() {
        let mut s = Screen::new(Charset::Cp437, 25);
        s.write_bytes(b"\x1b[2J\x1b[38;2;200;0;200mLOGO\x1b[3;1H\x1b[31m\xDA\xC4\xC4\xBF  menu text");
        let fx = [Effect { kind: "glint".into(), rows: [1, 1], every: 8.0, delay: 1.5 }, Effect { kind: "ripple".into(), rows: [3, 3], every: 8.0, delay: 1.5 }];
        let a = Ambient::new(&s, &fx).unwrap();
        let cells: Vec<(u16, u16)> = a.targets.iter().map(|g| (g.x, g.y)).collect();
        assert_eq!(cells, [(1, 1), (2, 1), (3, 1), (4, 1), (1, 3), (2, 3), (3, 3), (4, 3)], "the logo and the box line, not the text");
        assert!(Ambient::new(&s, &[Effect { kind: "ripple".into(), rows: [1, 1], every: 8.0, delay: 1.5 }]).is_none());
    }

    #[test]
    fn a_glint_comes_and_goes() {
        let e = Effect { kind: "glint".into(), rows: [1, 9], every: 8.0, delay: 1.5 };
        let a = Ambient { targets: Vec::new(), effects: vec![e.clone()], start: Instant::now() };
        // Mid-sweep it's lit somewhere along its path, and dark after.
        assert!((0..80).any(|x| a.light(&e, x, 1, 1.5 + 0.8) > 0.0));
        assert!((0..80).all(|x| a.light(&e, x, 1, 1.5 + 4.0) == 0.0));
    }
}
