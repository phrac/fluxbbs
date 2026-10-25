//! New: craps in the casino (`AC craps`). The layout's printed on green
//! felt in half-block pixels (`pixels`), with the puck on the point; two
//! red dice tumble down the table, bounce off the back wall and settle.
//! Bets: the pass line and don't pass (on the come-out roll; don't pass
//! bars 12), odds behind the pass line at true odds (up to `[casino]
//! max_odds` times the line), and the field, one roll (2 pays double,
//! 12 triple). Bets are the day's free chips (`casino.rs`).

use std::f32::consts::PI;
use std::io;
use std::sync::OnceLock;

use rand::RngExt;

use crate::blackjack::draw_pile;
use crate::casino::{H, Ink, MSG_ROW, Purse, STATUS_ROW, Stage, W, ink, painted, shade};
use crate::config::Casino;
use crate::input::Key;
use crate::pixels::{Canvas, hash, mix, rgb, scale, smooth};
use crate::session::Session;

// ---- the rules -------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bets {
    pub pass: u32,
    pub dont: u32,
    pub odds: u32,
    pub field: u32,
}

impl Bets {
    fn total(&self) -> u32 {
        self.pass + self.dont + self.odds + self.field
    }
}

/// What a roll does: chips back for each bet (stake included; 0 lost,
/// the stake alone a push or still riding), the point after it, and
/// which bets are settled.
#[derive(Debug, PartialEq)]
pub struct Roll {
    pub back: Bets,
    pub settled: Bets,
    pub point: Option<u8>,
}

/// True odds on a point: what a winning odds bet pays, over its stake.
fn true_odds(point: u8, stake: u32) -> u32 {
    match point {
        4 | 10 => stake * 2,
        5 | 9 => stake * 3 / 2,
        _ => stake * 6 / 5,
    }
}

pub fn roll(bets: Bets, point: Option<u8>, sum: u8) -> Roll {
    let mut back = Bets::default();
    let mut settled = Bets::default();
    // The field, one roll.
    if bets.field > 0 {
        settled.field = bets.field;
        back.field = match sum {
            2 => bets.field * 3,
            12 => bets.field * 4,
            3 | 4 | 9 | 10 | 11 => bets.field * 2,
            _ => 0,
        };
    }
    let next = match point {
        None => match sum {
            7 | 11 => {
                (settled.pass, settled.dont) = (bets.pass, bets.dont);
                back.pass = bets.pass * 2;
                None
            }
            2 | 3 => {
                (settled.pass, settled.dont) = (bets.pass, bets.dont);
                back.dont = bets.dont * 2;
                None
            }
            12 => {
                // Don't pass bars 12: a push.
                (settled.pass, settled.dont) = (bets.pass, bets.dont);
                back.dont = bets.dont;
                None
            }
            n => Some(n),
        },
        Some(p) if sum == p => {
            (settled.pass, settled.dont, settled.odds) = (bets.pass, bets.dont, bets.odds);
            back.pass = bets.pass * 2;
            back.odds = bets.odds + true_odds(p, bets.odds);
            None
        }
        Some(_) if sum == 7 => {
            (settled.pass, settled.dont, settled.odds) = (bets.pass, bets.dont, bets.odds);
            back.dont = bets.dont * 2;
            None
        }
        p => p,
    };
    Roll { back, settled, point: next }
}

// ---- the art ---------------------------------------------------------------

/// Where the bets sit on the layout, and the point boxes' middles.
const PASS_SPOT: (f32, f32) = (14.0, 41.0);
const DONT_SPOT: (f32, f32) = (14.0, 34.0);
const ODDS_SPOT: (f32, f32) = (23.0, 41.0);
const FIELD_SPOT: (f32, f32) = (12.0, 27.0);
const BANK: (f32, f32) = (8.0, 52.0);
const TRAY: (f32, f32) = (40.0, -8.0);
const POINTS: [u8; 6] = [4, 5, 6, 8, 9, 10];
const BOX_X0: i32 = 5;
const BOX_W: i32 = 11;
/// The back wall the dice bounce off.
const WALL: f32 = 72.0;

fn box_x(point: u8) -> i32 {
    BOX_X0 + POINTS.iter().position(|&p| p == point).unwrap_or(0) as i32 * BOX_W
}

/// The table: felt, rails, the wall, and the layout printed on it.
fn layout() -> &'static Canvas {
    static LAYOUT: OnceLock<Canvas> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let d = (((x as f32 - 38.0) / 48.0).powi(2) + ((y as f32 - 22.0) / 28.0).powi(2)).sqrt();
                let mut c = mix(rgb(30, 122, 62), rgb(6, 42, 22), smooth(0.25, 1.2, d));
                c = scale(c, 0.96 + 0.08 * hash(x, y));
                cv.set(x, y, c);
            }
        }
        let white = rgb(235, 235, 225);
        let line = |cv: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32| {
            for x in x0..=x1 {
                cv.blend(x, y0, white, 0.8);
                cv.blend(x, y1, white, 0.8);
            }
            for y in y0..=y1 {
                cv.blend(x0, y, white, 0.8);
                cv.blend(x1, y, white, 0.8);
            }
        };
        // The point boxes.
        for &p in &POINTS {
            let x = box_x(p);
            line(&mut cv, x, 2, x + BOX_W, 12);
            let label = p.to_string();
            let tw = crate::pixels::text_width(&label);
            cv.print(x + (BOX_W + 1 - tw) / 2, 4, &label, 1, |_, v| mix(rgb(255, 235, 120), rgb(235, 170, 40), v));
        }
        // Come, field, don't pass, pass line: their words go on as text.
        line(&mut cv, BOX_X0, 12, BOX_X0 + 6 * BOX_W, 20);
        line(&mut cv, BOX_X0, 20, BOX_X0 + 6 * BOX_W, 30);
        line(&mut cv, BOX_X0, 30, BOX_X0 + 6 * BOX_W, 35);
        line(&mut cv, BOX_X0, 35, BOX_X0 + 6 * BOX_W, 43);
        // The back wall: rubber diamonds over a padded edge.
        for y in 0..H as i32 {
            for x in WALL as i32..W as i32 {
                let k = ((x - WALL as i32) + y).rem_euclid(4).min((y - (x - WALL as i32)).rem_euclid(4));
                let c = if x == WALL as i32 { rgb(60, 25, 20) } else { mix(rgb(70, 60, 70), rgb(150, 140, 155), k as f32 / 3.0) };
                cv.set(x, y, c);
            }
        }
        // The rail along the near and far sides.
        for x in 0..W as i32 {
            cv.set(x, 0, rgb(95, 45, 20));
            cv.set(x, 1, rgb(60, 25, 12));
        }
        cv.clear_labels();
        cv
    })
}

/// The labels printed on the layout, in text (the field's numbers,
/// don't pass), put back each frame.
fn layout_labels(cv: &mut Canvas) {
    let gold = rgb(255, 235, 120);
    cv.label(36, 8, "C  O  M  E", rgb(240, 60, 60), None);
    cv.label(22, 11, "F I E L D", gold, None);
    cv.label(36, 11, "2  3  4  9  10  11  12", gold, None);
    cv.label(36, 13, "2 pays double, 12 triple", rgb(220, 220, 210), None);
    cv.label(26, 16, "DON'T PASS BAR 12", rgb(235, 235, 225), None);
    cv.label(32, 19, "P A S S   L I N E", rgb(250, 250, 240), None);
}

/// A die: where it is and how high, how it's moving, which way it's
/// turned, the face it shows.
#[derive(Clone, Copy)]
struct Die {
    x: f32,
    y: f32,
    z: f32,
    vx: f32,
    vy: f32,
    vz: f32,
    angle: f32,
    spin: f32,
    face: u8,
    /// Distance rolled since the face last changed.
    rolled: f32,
}

/// The pips of a face, on a die 2 across (-1 to 1).
fn pips(face: u8) -> &'static [(f32, f32)] {
    const L: f32 = 0.52;
    match face {
        1 => &[(0.0, 0.0)],
        2 => &[(-L, -L), (L, L)],
        3 => &[(-L, -L), (0.0, 0.0), (L, L)],
        4 => &[(-L, -L), (L, -L), (-L, L), (L, L)],
        5 => &[(-L, -L), (L, -L), (0.0, 0.0), (-L, L), (L, L)],
        _ => &[(-L, -L), (L, -L), (-L, 0.0), (L, 0.0), (-L, L), (L, L)],
    }
}

/// A red casino die, `size` pixels across, its middle at (`x`, `y`),
/// turned `angle`, `z` up off the felt (its shadow falls aside).
pub fn draw_die(cv: &mut Canvas, x: f32, y: f32, z: f32, angle: f32, face: u8, size: f32) {
    let size = size * (1.0 + z * 0.02);
    let half = size / 2.0;
    let (s, c) = angle.sin_cos();
    let reach = half * 1.5 + 2.0;
    // The shadow, offset by the height.
    for py in (y - reach) as i32..=(y + reach + 2.0) as i32 {
        for px in (x - reach) as i32..=(x + reach + 2.0) as i32 {
            let (dx, dy) = (px as f32 + 0.5 - x - 1.0 - z * 0.3, py as f32 + 0.5 - y - 1.0 - z * 0.3);
            let (u, v) = (dx * c + dy * s, -dx * s + dy * c);
            if u.abs() < half && v.abs() < half {
                cv.blend(px, py, rgb(0, 0, 0), 0.3 / (1.0 + z * 0.08));
            }
        }
    }
    let (y, x) = (y - z, x);
    for py in (y - reach) as i32..=(y + reach) as i32 {
        for px in (x - reach) as i32..=(x + reach) as i32 {
            let mut acc = [0.0f32; 3];
            let mut cover = 0.0;
            for j in 0..2 {
                for i in 0..2 {
                    let (dx, dy) = (px as f32 + 0.25 + i as f32 * 0.5 - x, py as f32 + 0.25 + j as f32 * 0.5 - y);
                    let (u, v) = (dx * c + dy * s, -dx * s + dy * c);
                    // A rounded square.
                    let r = half * 0.3;
                    let (qx, qy) = ((u.abs() - (half - r)).max(0.0), (v.abs() - (half - r)).max(0.0));
                    if qx * qx + qy * qy > r * r {
                        continue;
                    }
                    let (nu, nv) = (u / half, v / half);
                    // Translucent red, lit from the upper left, a bright rim.
                    let mut col = mix(rgb(235, 40, 55), rgb(130, 5, 20), (nu + nv) * 0.3 + 0.45);
                    let rim = (half - u.abs().max(v.abs())).max(0.0);
                    if rim < 0.9 {
                        col = mix(col, rgb(255, 150, 160), 0.45 * (1.0 - rim / 0.9));
                    }
                    for &(ppx, ppy) in pips(face) {
                        if (nu - ppx).powi(2) + (nv - ppy).powi(2) < 0.07 {
                            col = rgb(250, 248, 240);
                        }
                    }
                    acc = [acc[0] + col[0], acc[1] + col[1], acc[2] + col[2]];
                    cover += 1.0;
                }
            }
            if cover > 0.0 {
                let col = scale(acc, 1.0 / cover);
                cv.blend(px, py, col, cover / 4.0);
            }
        }
    }
}

/// The puck: white ON on the point, black OFF beside the boxes.
fn draw_puck(cv: &mut Canvas, point: Option<u8>) {
    let (x, y, on) = match point {
        Some(p) => (box_x(p) as f32 + BOX_W as f32 / 2.0 + 0.5, 3.0, true),
        None => (11.0, 14.0, false),
    };
    let (face, rim) = if on { (rgb(245, 245, 240), rgb(160, 160, 160)) } else { (rgb(30, 30, 34), rgb(110, 110, 115)) };
    for py in 0..4 {
        for px in -4..=4 {
            let d = (px as f32 / 4.3).powi(2) + ((py as f32 - 1.5) / 2.2).powi(2);
            if d <= 1.0 {
                cv.set((x + px as f32).round() as i32, y as i32 + py, if d > 0.7 { rim } else { face });
            }
        }
    }
    let row = (y as usize).div_ceil(2);
    if on {
        cv.label((x - 1.0).round() as usize, row, "ON", rgb(20, 20, 20), Some(face));
    } else {
        cv.label((x - 1.5).round() as usize, row, "OFF", rgb(240, 240, 240), Some(face));
    }
}

/// Chips on the table, moving or sitting; gone at `gone`.
#[derive(Clone, Copy)]
struct Pile {
    amount: u32,
    from: (f32, f32),
    to: (f32, f32),
    t0: f32,
    dur: f32,
    gone: f32,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Default)]
struct Table {
    dice: Vec<Die>,
    piles: Vec<Pile>,
    point: Option<u8>,
}

impl Table {
    fn draw(&self, cv: &mut Canvas, t: f32, bets: &Bets) {
        cv.px.clone_from(&layout().px);
        cv.clear_labels();
        layout_labels(cv);
        draw_puck(cv, self.point);
        // Bets riding, then chips on the move.
        for (amount, spot) in [(bets.pass, PASS_SPOT), (bets.dont, DONT_SPOT), (bets.odds, ODDS_SPOT), (bets.field, FIELD_SPOT)] {
            if amount > 0 {
                draw_pile(cv, amount, spot.0, spot.1);
            }
        }
        for p in self.piles.iter().filter(|p| t < p.gone && t >= p.t0 - 0.001) {
            let k = ease((t - p.t0) / p.dur);
            draw_pile(cv, p.amount, p.from.0 + (p.to.0 - p.from.0) * k, p.from.1 + (p.to.1 - p.from.1) * k);
        }
        for d in &self.dice {
            draw_die(cv, d.x, d.y, d.z, d.angle, d.face, 9.0);
        }
    }

    fn busy_until(&self) -> f32 {
        self.piles.iter().map(|p| p.t0 + p.dur).fold(0.0, f32::max)
    }
}

/// Throw the dice from the near end: they fly, bounce off the wall and
/// roll to a stop showing `faces`. Steps the physics by `dt`; true while
/// they're moving.
fn step_dice(dice: &mut [Die], faces: [u8; 2], dt: f32, rng: &mut impl RngExt) -> bool {
    let mut moving = false;
    for (i, d) in dice.iter_mut().enumerate() {
        d.vz -= 260.0 * dt;
        d.z += d.vz * dt;
        if d.z <= 0.0 {
            d.z = 0.0;
            // A bounce, less each time, and a kick to the spin.
            d.vz = if d.vz < -25.0 { -d.vz * 0.42 } else { 0.0 };
            d.spin += rng.random_range(-4.0..4.0);
            // The felt drags.
            let drag: f32 = 0.86;
            d.vx *= drag;
            d.vy *= drag;
        }
        if d.z == 0.0 {
            let f = (1.0 - 2.6 * dt).max(0.0);
            d.vx *= f;
            d.vy *= f;
            d.spin *= f;
        }
        d.x += d.vx * dt;
        d.y += d.vy * dt;
        if d.x > WALL - 6.0 {
            d.x = WALL - 6.0;
            d.vx = -d.vx.abs() * 0.55;
            d.vy += rng.random_range(-25.0..25.0);
            d.vz = d.vz.max(40.0);
            d.spin += rng.random_range(-10.0..10.0);
        }
        if d.x < 9.0 {
            d.x = 9.0;
            d.vx = d.vx.abs() * 0.5;
        }
        if d.y < 5.0 {
            d.y = 5.0;
            d.vy = d.vy.abs() * 0.5;
        }
        if d.y > 39.0 {
            d.y = 39.0;
            d.vy = -d.vy.abs() * 0.5;
        }
        d.angle += d.spin * dt;
        let speed = (d.vx * d.vx + d.vy * d.vy).sqrt();
        d.rolled += speed * dt;
        if speed > 12.0 || d.z > 0.5 {
            moving = true;
            if d.rolled > 7.0 {
                d.rolled = 0.0;
                d.face = rng.random_range(1..=6);
            }
        } else {
            // Coming to rest: the face it was meant to show, turning square.
            d.face = faces[i];
            d.vx = 0.0;
            d.vy = 0.0;
            d.spin = 0.0;
            let square = (d.angle / (PI / 2.0)).round() * (PI / 2.0);
            d.angle += (square - d.angle) * (8.0 * dt).min(1.0);
            if (square - d.angle).abs() > 0.02 {
                moving = true;
            }
        }
    }
    // Keep the two apart.
    if dice.len() == 2 {
        let (dx, dy) = (dice[1].x - dice[0].x, dice[1].y - dice[0].y);
        let d = (dx * dx + dy * dy).sqrt();
        if d < 11.0 && d > 0.01 {
            let push = (11.0 - d) / 2.0;
            dice[0].x -= dx / d * push;
            dice[0].y -= dy / d * push;
            dice[1].x += dx / d * push;
            dice[1].y += dy / d * push;
        }
    }
    moving
}

/// What the stickman calls.
fn call(sum: u8, point: Option<u8>) -> String {
    let name = match sum {
        2 => "aces",
        3 => "ace-deuce",
        4 => "four",
        5 => "five",
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        10 => "ten",
        11 => "yo-leven",
        _ => "twelve",
    };
    match (point, sum) {
        (None, 7 | 11) => format!("{name}! winner on the pass line"),
        (None, 2 | 3 | 12) => format!("{name}, craps!"),
        (None, n) => format!("{name}, the point is {n}"),
        (Some(p), n) if n == p => format!("{name}! the point, winner"),
        (Some(_), 7) => "seven out, line away".into(),
        (Some(_), 2 | 3 | 4 | 9 | 10 | 11 | 12) => format!("{name}, field's a winner"),
        _ => name.to_string(),
    }
}

// ---- the game ----------------------------------------------------------------

impl Session {
    pub(crate) async fn craps(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.craps_plain(cfg, purse).await;
        }
        let mut st = self.stage_open();
        let r = self.craps_table(cfg, purse, &mut st).await;
        self.stage_close();
        r
    }

    async fn craps_frame(&mut self, st: &mut Stage, tb: &Table, bets: &Bets) -> io::Result<()> {
        let t = st.t();
        tb.draw(&mut st.cv, t, bets);
        self.stage_show(st, true).await?;
        self.flush().await
    }

    fn craps_status(&mut self, st: &Stage, purse: u32, unit: u32, keys: &str) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        ink(&mut out, [140, 200, 150], st.truecolor);
        out.extend(b"chips ");
        ink(&mut out, [255, 230, 140], st.truecolor);
        out.extend(purse.to_string().as_bytes());
        ink(&mut out, [140, 200, 150], st.truecolor);
        out.extend(b"  chip ");
        ink(&mut out, [255, 230, 140], st.truecolor);
        out.extend(unit.to_string().as_bytes());
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let len = self.visible_len(&fluxbbs_term::cp437::encode(keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    async fn craps_table(&mut self, cfg: &Casino, purse: &mut Purse, st: &mut Stage) -> io::Result<()> {
        let mut tb = Table::default();
        let mut bets = Bets::default();
        let mut unit = cfg.min_bet;
        let hello = painted(&crate::editor::strip_colours(self.strings().get("CrapsHello")), [235, 200, 110]);
        self.stage_line(MSG_ROW, &hello, st.truecolor);
        // Two dice resting at the near end to start.
        tb.dice = vec![
            Die { x: 50.0, y: 22.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, angle: 0.2, spin: 0.0, face: 6, rolled: 0.0 },
            Die { x: 62.0, y: 26.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, angle: -0.3, spin: 0.0, face: 1, rolled: 0.0 },
        ];
        loop {
            let max = cfg.max_bet;
            let keys = self.strings().get(if tb.point.is_some() { "CrapsKeysPoint" } else { "CrapsKeysComeOut" }).to_string();
            self.craps_status(st, purse.chips, unit, &keys);
            self.craps_frame(st, &tb, &bets).await?;
            let k = self.read_key().await?;
            let place = |amount: &mut u32, room: u32, purse: &mut Purse| {
                let add = unit.min(room.saturating_sub(*amount)).min(purse.chips);
                *amount += add;
                purse.chips -= add;
            };
            match k {
                Key::Up | Key::Right => unit = next_unit(unit, cfg, true),
                Key::Down | Key::Left => unit = next_unit(unit, cfg, false),
                Key::Enter | Key::Char(b' ') => {
                    if bets.total() == 0 {
                        let s = painted(&crate::editor::strip_colours(self.strings().get("CrapsNoBet")), [190, 170, 200]);
                        self.stage_line(MSG_ROW, &s, st.truecolor);
                        continue;
                    }
                    self.purse_save(purse);
                    self.craps_roll(purse, st, &mut tb, &mut bets).await?;
                }
                Key::Esc => {
                    if self.craps_leave(purse, st, &tb, &mut bets) {
                        return Ok(());
                    }
                }
                k => match k.upper() {
                    Some('Q') => {
                        if self.craps_leave(purse, st, &tb, &mut bets) {
                            return Ok(());
                        }
                    }
                    Some('+' | '=') => unit = next_unit(unit, cfg, true),
                    Some('-' | '_') => unit = next_unit(unit, cfg, false),
                    Some('P') if tb.point.is_none() => place(&mut bets.pass, max, purse),
                    Some('D') if tb.point.is_none() => place(&mut bets.dont, max, purse),
                    Some('O') if tb.point.is_some() && bets.pass > 0 => place(&mut bets.odds, bets.pass * cfg.max_odds, purse),
                    Some('F') => place(&mut bets.field, max, purse),
                    // Take down what may come down: the field, the odds, and
                    // line bets before a point.
                    Some('X') => {
                        purse.chips += bets.field + bets.odds;
                        (bets.field, bets.odds) = (0, 0);
                        if tb.point.is_none() {
                            purse.chips += bets.pass + bets.dont;
                            (bets.pass, bets.dont) = (0, 0);
                        }
                    }
                    _ => {}
                },
            }
            self.purse_save(purse);
        }
    }

    /// Leave if no point is riding on line bets; what's down comes back.
    fn craps_leave(&mut self, purse: &mut Purse, st: &Stage, tb: &Table, bets: &mut Bets) -> bool {
        if tb.point.is_some() && bets.pass + bets.dont > 0 {
            let s = painted(&crate::editor::strip_colours(self.strings().get("CrapsFinish")), [255, 170, 120]);
            self.stage_line(MSG_ROW, &s, st.truecolor);
            return false;
        }
        purse.chips += bets.total();
        *bets = Bets::default();
        self.purse_save(purse);
        true
    }

    async fn craps_roll(&mut self, purse: &mut Purse, st: &mut Stage, tb: &mut Table, bets: &mut Bets) -> io::Result<()> {
        let faces = {
            let mut rng = rand::rng();
            [rng.random_range(1..=6u8), rng.random_range(1..=6u8)]
        };
        {
            let mut rng = rand::rng();
            for (i, d) in tb.dice.iter_mut().enumerate() {
                *d = Die {
                    x: 6.0 + i as f32 * 3.0,
                    y: 30.0 + i as f32 * 6.0,
                    z: 6.0,
                    vx: rng.random_range(150.0..190.0),
                    vy: rng.random_range(-45.0..-15.0),
                    vz: rng.random_range(60.0..90.0),
                    angle: rng.random_range(0.0..6.3),
                    spin: rng.random_range(-14.0..14.0),
                    face: rng.random_range(1..=6),
                    rolled: 0.0,
                };
            }
        }
        self.stage_line(MSG_ROW, &[], st.truecolor);
        let mut last = st.t();
        let begin = last;
        loop {
            let now = st.t();
            let moving = {
                let mut rng = rand::rng();
                let mut dt = (now - last).min(0.1);
                let mut moving = true;
                // Small steps, so a slow frame doesn't throw them through walls.
                while dt > 0.0 {
                    let h = dt.min(0.01);
                    moving = step_dice(&mut tb.dice, faces, h, &mut rng);
                    dt -= h;
                }
                moving
            };
            last = now;
            self.craps_frame(st, tb, bets).await?;
            if !moving || now - begin > 6.0 {
                break;
            }
        }
        for (d, &f) in tb.dice.iter_mut().zip(&faces) {
            d.face = f;
        }
        let sum = faces[0] + faces[1];
        let r = roll(*bets, tb.point, sum);
        let text = format!("{}  ({} + {})", call(sum, tb.point), faces[0], faces[1]);
        // Chips go: winnings from the tray to the bet and home; losses to
        // the tray.
        let now = st.t() + 0.3;
        let spots = [PASS_SPOT, DONT_SPOT, ODDS_SPOT, FIELD_SPOT];
        let get = |b: &Bets, i: usize| [b.pass, b.dont, b.odds, b.field][i];
        let mut won = 0i64;
        for (i, &spot) in spots.iter().enumerate() {
            let (stake, back) = (get(&r.settled, i), get(&r.back, i));
            if stake == 0 {
                continue;
            }
            won += back as i64 - stake as i64;
            if back == 0 {
                tb.piles.push(Pile { amount: stake, from: spot, to: TRAY, t0: now, dur: 0.5, gone: now + 0.5 });
            } else {
                let profit = back - stake.min(back);
                if profit > 0 {
                    let beside = (spot.0 + 8.0, spot.1);
                    tb.piles.push(Pile { amount: profit, from: TRAY, to: beside, t0: now, dur: 0.5, gone: now + 1.0 });
                    tb.piles.push(Pile { amount: profit, from: beside, to: BANK, t0: now + 1.0, dur: 0.45, gone: now + 1.45 });
                }
                tb.piles.push(Pile { amount: back.min(stake), from: spot, to: spot, t0: now, dur: 0.01, gone: now + 1.0 });
                tb.piles.push(Pile { amount: back.min(stake), from: spot, to: BANK, t0: now + 1.0, dur: 0.45, gone: now + 1.45 });
            }
        }
        for (i, field) in [&mut bets.pass, &mut bets.dont, &mut bets.odds, &mut bets.field].into_iter().enumerate() {
            if get(&r.settled, i) > 0 {
                *field = 0;
            }
        }
        purse.chips += r.back.total();
        tb.point = r.point;
        self.purse_save(purse);
        let good = won > 0;
        let text = if won != 0 { format!("{text}   {}{}", if good { "+" } else { "-" }, won.unsigned_abs()) } else { text };
        let until = (tb.busy_until() + 0.1).max(st.t() + 1.2);
        while st.t() < until {
            let t = st.t();
            let line: Vec<(char, Ink)> = text
                .chars()
                .enumerate()
                .map(|(i, ch)| {
                    let wave = 0.5 + 0.5 * ((i as f32) * 0.45 - t * 8.0).sin();
                    (ch, if good { shade([235, 160, 30], [255, 250, 210], wave) } else { [215, 200, 225] })
                })
                .collect();
            self.stage_line(MSG_ROW, &line, st.truecolor);
            self.craps_frame(st, tb, bets).await?;
        }
        let line = painted(&text, if good { [255, 215, 110] } else { [215, 200, 225] });
        self.stage_line(MSG_ROW, &line, st.truecolor);
        tb.piles.clear();
        self.craps_frame(st, tb, bets).await
    }

    /// Plain text: a line a roll.
    async fn craps_plain(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        let mut bets = Bets::default();
        let mut point = None;
        loop {
            let on = point.map_or("off".to_string(), |p: u8| p.to_string());
            let s = self.strings().fmt(
                "CrapsPlainPrompt",
                &[&purse.chips.to_string(), &on, &format!("pass {} don't {} odds {} field {}", bets.pass, bets.dont, bets.odds, bets.field)],
            );
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(12)).await?;
            self.newline();
            let mut w = s.split_whitespace();
            let cmd = w.next().unwrap_or("").to_ascii_lowercase();
            let amount: u32 = w.next().and_then(|n| n.parse().ok()).unwrap_or(cfg.min_bet);
            let put = |slot: &mut u32, room: u32, purse: &mut Purse| {
                let add = amount.min(room.saturating_sub(*slot)).min(purse.chips);
                *slot += add;
                purse.chips -= add;
            };
            match cmd.as_str() {
                "q" => {
                    if point.is_some() && bets.pass + bets.dont > 0 {
                        self.say_line("CrapsFinish").await?;
                        continue;
                    }
                    purse.chips += bets.total();
                    self.purse_save(purse);
                    return Ok(());
                }
                "p" if point.is_none() => put(&mut bets.pass, cfg.max_bet, purse),
                "d" if point.is_none() => put(&mut bets.dont, cfg.max_bet, purse),
                "o" if point.is_some() && bets.pass > 0 => put(&mut bets.odds, bets.pass * cfg.max_odds, purse),
                "f" => put(&mut bets.field, cfg.max_bet, purse),
                "" | "r" if bets.total() > 0 => {
                    let (a, b) = {
                        let mut rng = rand::rng();
                        (rng.random_range(1..=6u8), rng.random_range(1..=6u8))
                    };
                    let r = roll(bets, point, a + b);
                    let won = r.back.total() as i64 - r.settled.total() as i64;
                    self.write(&format!("rolled {a} + {b}: {}.", call(a + b, point)));
                    if r.settled.total() > 0 {
                        self.write(&format!(" {}", if won >= 0 { format!("+{won}") } else { won.to_string() }));
                    }
                    self.newline();
                    purse.chips += r.back.total();
                    for (slot, s) in [(&mut bets.pass, r.settled.pass), (&mut bets.dont, r.settled.dont), (&mut bets.odds, r.settled.odds), (&mut bets.field, r.settled.field)] {
                        if s > 0 {
                            *slot = 0;
                        }
                    }
                    point = r.point;
                }
                _ => {}
            }
            self.purse_save(purse);
        }
    }
}

/// Chip sizes to bet with: 1, 5, 25, 100, within the table's limits.
fn next_unit(unit: u32, cfg: &Casino, up: bool) -> u32 {
    let sizes: Vec<u32> = [1, 5, 10, 25, 50, 100, 500].into_iter().filter(|&s| s >= cfg.min_bet && s <= cfg.max_bet).collect();
    let i = sizes.iter().position(|&s| s >= unit).unwrap_or(0);
    let j = if up { (i + 1).min(sizes.len().saturating_sub(1)) } else { i.saturating_sub(1) };
    sizes.get(j).copied().unwrap_or(cfg.min_bet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(pass: u32, dont: u32, odds: u32, field: u32) -> Bets {
        Bets { pass, dont, odds, field }
    }

    #[test]
    fn the_come_out_roll() {
        let r = roll(b(10, 0, 0, 0), None, 7);
        assert_eq!((r.back.pass, r.point), (20, None));
        let r = roll(b(10, 10, 0, 0), None, 3);
        assert_eq!((r.back.pass, r.back.dont), (0, 20));
        let r = roll(b(0, 10, 0, 0), None, 12);
        assert_eq!(r.back.dont, 10, "bar 12: a push");
        let r = roll(b(10, 0, 0, 0), None, 6);
        assert_eq!((r.point, r.settled.pass), (Some(6), 0), "the bet rides on the point");
    }

    #[test]
    fn the_point_and_the_odds() {
        let r = roll(b(10, 0, 20, 0), Some(4), 4);
        assert_eq!((r.back.pass, r.back.odds, r.point), (20, 60, None), "odds on 4 pay 2 to 1");
        let r = roll(b(10, 0, 10, 0), Some(6), 6);
        assert_eq!(r.back.odds, 22, "6 to 5");
        let r = roll(b(10, 10, 20, 0), Some(9), 7);
        assert_eq!((r.back.pass, r.back.odds, r.back.dont, r.point), (0, 0, 20, None), "seven out");
        let r = roll(b(10, 0, 0, 0), Some(9), 8);
        assert_eq!((r.settled.pass, r.point), (0, Some(9)));
    }

    #[test]
    fn the_field() {
        assert_eq!(roll(b(0, 0, 0, 5), None, 2).back.field, 15);
        assert_eq!(roll(b(0, 0, 0, 5), None, 12).back.field, 20);
        assert_eq!(roll(b(0, 0, 0, 5), Some(5), 9).back.field, 10);
        assert_eq!(roll(b(0, 0, 0, 5), None, 7).back.field, 0);
    }

    #[test]
    fn dice_come_to_rest_on_their_faces() {
        let mut rng = rand::rng();
        let mut dice = vec![
            Die { x: 6.0, y: 30.0, z: 6.0, vx: 170.0, vy: -30.0, vz: 80.0, angle: 0.0, spin: 10.0, face: 1, rolled: 0.0 },
            Die { x: 9.0, y: 36.0, z: 6.0, vx: 160.0, vy: -20.0, vz: 70.0, angle: 1.0, spin: -8.0, face: 2, rolled: 0.0 },
        ];
        let mut t = 0.0;
        while step_dice(&mut dice, [3, 5], 0.01, &mut rng) {
            t += 0.01;
            assert!(t < 10.0, "they stop");
        }
        assert_eq!((dice[0].face, dice[1].face), (3, 5));
        assert!(dice.iter().all(|d| (9.0..=WALL - 6.0).contains(&d.x) && (5.0..=39.0).contains(&d.y)));
    }
}
