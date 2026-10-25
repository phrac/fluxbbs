//! New: roulette in the casino (`AC roulette`). A European wheel (one
//! zero) seen across the table in half-block pixels (`pixels`): it turns
//! one way and the ball runs the other round the track, slows, drops,
//! rattles over the frets and settles in its pocket. The layout below
//! takes chips on a number (35 to 1), a column or dozen (2 to 1), and
//! red or black, odd or even, 1-18 or 19-36 (even money); zero takes
//! the outside bets. Bets are the day's free chips (`casino.rs`).

use std::f32::consts::PI;
use std::io;
use std::sync::OnceLock;

use rand::RngExt;

use crate::casino::{H, Ink, MSG_ROW, Purse, STATUS_ROW, Stage, W, ink, painted, shade};
use crate::config::Casino;
use crate::input::Key;
use crate::pixels::{Canvas, Rgb, hash, mix, rgb, scale, smooth};
use crate::session::Session;

// ---- the rules -------------------------------------------------------------

/// The pockets in order round the wheel.
pub const WHEEL: [u8; 37] = [0, 32, 15, 19, 4, 21, 2, 25, 17, 34, 6, 27, 13, 36, 11, 30, 8, 23, 10, 5, 24, 16, 33, 1, 20, 14, 31, 9, 22, 18, 29, 7, 28, 12, 35, 3, 26];

pub fn red(n: u8) -> bool {
    matches!(n, 1 | 3 | 5 | 7 | 9 | 12 | 14 | 16 | 18 | 19 | 21 | 23 | 25 | 27 | 30 | 32 | 34 | 36)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bet {
    Number(u8),
    Red,
    Black,
    Odd,
    Even,
    Low,
    High,
    /// 0, 1 or 2: 1-12, 13-24, 25-36.
    Dozen(u8),
    /// 0, 1 or 2: the column with 1, 2 or 3 at its foot.
    Column(u8),
}

impl Bet {
    /// What a winning bet pays, to one.
    fn odds(self) -> u32 {
        match self {
            Bet::Number(_) => 35,
            Bet::Dozen(_) | Bet::Column(_) => 2,
            _ => 1,
        }
    }

    fn wins(self, n: u8) -> bool {
        if n == 0 {
            return self == Bet::Number(0);
        }
        match self {
            Bet::Number(k) => k == n,
            Bet::Red => red(n),
            Bet::Black => !red(n),
            Bet::Odd => n % 2 == 1,
            Bet::Even => n.is_multiple_of(2),
            Bet::Low => n <= 18,
            Bet::High => n >= 19,
            Bet::Dozen(d) => (n - 1) / 12 == d,
            Bet::Column(c) => (n - 1) % 3 == c,
        }
    }

    /// What comes back for `stake` when `n` comes up.
    pub fn returned(self, n: u8, stake: u32) -> u32 {
        if self.wins(n) { stake * (self.odds() + 1) } else { 0 }
    }

    fn name(self) -> String {
        match self {
            Bet::Number(n) => n.to_string(),
            Bet::Red => "red".into(),
            Bet::Black => "black".into(),
            Bet::Odd => "odd".into(),
            Bet::Even => "even".into(),
            Bet::Low => "1-18".into(),
            Bet::High => "19-36".into(),
            Bet::Dozen(d) => ["1st 12", "2nd 12", "3rd 12"][d as usize].into(),
            Bet::Column(c) => format!("column {}", c + 1),
        }
    }

    /// A bet as typed in plain text: a number, red, even, 1st, col2...
    fn parse(s: &str) -> Option<Bet> {
        let s = s.to_ascii_lowercase();
        Some(match s.as_str() {
            "red" | "r" => Bet::Red,
            "black" | "b" => Bet::Black,
            "odd" => Bet::Odd,
            "even" => Bet::Even,
            "low" | "1-18" => Bet::Low,
            "high" | "19-36" => Bet::High,
            "1st" => Bet::Dozen(0),
            "2nd" => Bet::Dozen(1),
            "3rd" => Bet::Dozen(2),
            "col1" => Bet::Column(0),
            "col2" => Bet::Column(1),
            "col3" => Bet::Column(2),
            n => Bet::Number(n.parse::<u8>().ok().filter(|&n| n <= 36)?),
        })
    }
}

fn color_name(n: u8) -> &'static str {
    match n {
        0 => "green",
        n if red(n) => "red",
        _ => "black",
    }
}

// ---- the layout ----------------------------------------------------------------

/// Where the layout's spots are, in pixels: x, y, width, height.
fn spot_box(b: Bet) -> (i32, i32, i32, i32) {
    match b {
        Bet::Number(0) => (4, 26, 5, 12),
        Bet::Number(n) => {
            let (c, r) = ((n as i32 - 1) / 3, 2 - (n as i32 - 1) % 3);
            (9 + c * 5, 26 + r * 4, 5, 4)
        }
        Bet::Column(c) => (69, 26 + (2 - c as i32) * 4, 6, 4),
        Bet::Dozen(d) => (9 + d as i32 * 20, 38, 20, 2),
        Bet::Low => (9, 40, 10, 2),
        Bet::Even => (19, 40, 10, 2),
        Bet::Red => (29, 40, 10, 2),
        Bet::Black => (39, 40, 10, 2),
        Bet::Odd => (49, 40, 10, 2),
        Bet::High => (59, 40, 10, 2),
    }
}

/// The spot under the cursor, on a grid of 14 across and 5 down: the
/// numbers' three rows (zero at the left, the columns' bets at the
/// right), then the dozens, then the even-money bets.
fn spot_at(gx: i32, gy: i32) -> Bet {
    match gy {
        0..=2 => match gx {
            0 => Bet::Number(0),
            13 => Bet::Column(2 - gy as u8),
            x => Bet::Number(((x - 1) * 3 + (3 - gy)) as u8),
        },
        3 => Bet::Dozen((((gx - 1).clamp(0, 11)) / 4) as u8),
        _ => [Bet::Low, Bet::Even, Bet::Red, Bet::Black, Bet::Odd, Bet::High][((gx - 1).clamp(0, 11) / 2) as usize],
    }
}

/// The felt and the layout printed on it.
fn felt() -> &'static Canvas {
    static FELT: OnceLock<Canvas> = OnceLock::new();
    FELT.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let d = (((x as f32 - 40.0) / 50.0).powi(2) + ((y as f32 - 26.0) / 30.0).powi(2)).sqrt();
                let c = mix(rgb(32, 125, 66), rgb(6, 40, 22), smooth(0.2, 1.2, d));
                cv.set(x, y, scale(c, 0.96 + 0.08 * hash(x, y)));
            }
        }
        let gold = rgb(220, 185, 95);
        let mut spots: Vec<Bet> = (0..=36).map(Bet::Number).collect();
        spots.extend([Bet::Column(0), Bet::Column(1), Bet::Column(2), Bet::Dozen(0), Bet::Dozen(1), Bet::Dozen(2)]);
        spots.extend([Bet::Low, Bet::Even, Bet::Red, Bet::Black, Bet::Odd, Bet::High]);
        for b in spots {
            let (x, y, w, h) = spot_box(b);
            let fill = match b {
                Bet::Number(0) => Some(rgb(25, 130, 65)),
                Bet::Number(n) if red(n) => Some(rgb(180, 22, 34)),
                Bet::Number(_) => Some(rgb(28, 26, 32)),
                Bet::Red => Some(rgb(180, 22, 34)),
                Bet::Black => Some(rgb(28, 26, 32)),
                _ => None,
            };
            for yy in y..y + h {
                for xx in x..x + w {
                    if let Some(f) = fill {
                        cv.set(xx, yy, f);
                    }
                }
            }
            // Gold lines round each spot.
            for xx in x..=x + w {
                cv.blend(xx, y, gold, 0.75);
                cv.blend(xx, y + h, gold, 0.75);
            }
            for yy in y..=y + h {
                cv.blend(x, yy, gold, 0.75);
                cv.blend(x + w, yy, gold, 0.75);
            }
        }
        cv.clear_labels();
        cv
    })
}

fn spot_label(b: Bet) -> String {
    match b {
        Bet::Number(n) => format!("{n:>2}"),
        Bet::Column(_) => "2to1".into(),
        Bet::Dozen(d) => ["1st 12", "2nd 12", "3rd 12"][d as usize].into(),
        Bet::Low => "1-18".into(),
        Bet::Even => "EVEN".into(),
        Bet::Red => "RED".into(),
        Bet::Black => "BLACK".into(),
        Bet::Odd => "ODD".into(),
        Bet::High => "19-36".into(),
    }
}

// ---- the wheel -------------------------------------------------------------

/// Where to draw a wheel: its middle and its radii (across, and up and
/// down: it's seen at a slant).
#[derive(Clone, Copy)]
pub struct Wheel {
    pub cx: f32,
    pub cy: f32,
    pub rx: f32,
    pub ry: f32,
}

pub const TABLE_WHEEL: Wheel = Wheel { cx: 39.5, cy: 12.5, rx: 37.0, ry: 11.6 };

/// The pocket band, and the track the ball runs round.
const POCKET_IN: f32 = 0.56;
const POCKET_OUT: f32 = 0.76;
const POCKET_R: f32 = 0.665;
const TRACK_R: f32 = 0.86;

/// The middle of pocket `i` (its place round the wheel), as an angle on
/// the wheel.
fn pocket_angle(i: usize) -> f32 {
    (i as f32 + 0.5) / 37.0 * 2.0 * PI
}

fn wheel_color(u: f32, v: f32, turn: f32) -> Option<Rgb> {
    let r = (u * u + v * v).sqrt();
    if r > 1.0 {
        return None;
    }
    let a = v.atan2(u);
    // Light from above the table: the far side of the bowl catches it.
    let lit = 1.0 - 0.25 * v;
    let c = if r > 0.93 {
        mix(rgb(150, 85, 40), rgb(70, 32, 12), (r - 0.93) / 0.07) // the rim
    } else if r > 0.8 {
        // The track, polished: a shine along the far side.
        let shine = (-((a + 1.9).rem_euclid(2.0 * PI) - PI).powi(2) * 2.0).exp();
        mix(rgb(60, 28, 14), rgb(200, 150, 100), 0.5 * shine)
    } else if r > POCKET_OUT {
        rgb(205, 205, 215)
    } else if r > POCKET_IN {
        let rel = (a - turn).rem_euclid(2.0 * PI) / (2.0 * PI) * 37.0;
        let (i, f) = (rel.floor() as usize % 37, rel.fract());
        if !(0.1..0.9).contains(&f) {
            rgb(150, 150, 160) // a fret
        } else {
            match WHEEL[i] {
                0 => rgb(25, 150, 70),
                n if red(n) => rgb(200, 25, 35),
                _ => rgb(25, 22, 28),
            }
        }
    } else if r > 0.5 {
        rgb(190, 190, 200)
    } else if r > 0.17 {
        // The cone, and the turret's arms turning with it.
        let arm = (2.0 * (a - turn)).sin().abs() < 0.12 && r < 0.42;
        if arm { rgb(220, 220, 230) } else { mix(rgb(140, 80, 35), rgb(85, 45, 18), r / 0.5) }
    } else {
        mix(rgb(255, 255, 255), rgb(150, 150, 165), ((u + 0.05) * (u + 0.05) + (v + 0.08) * (v + 0.08)).sqrt() / 0.17)
    };
    Some(scale(c, lit))
}

/// The wheel, turned `turn`, and the ball at `ball` (an angle round the
/// wheel and how far out, 0 to 1), if it's on.
pub fn draw_wheel(cv: &mut Canvas, w: Wheel, turn: f32, ball: Option<(f32, f32)>) {
    // Its shadow on the felt first.
    for y in (w.cy - w.ry - 1.0) as i32..=(w.cy + w.ry + 2.0) as i32 {
        for x in (w.cx - w.rx - 1.0) as i32..=(w.cx + w.rx + 2.0) as i32 {
            let (u, v) = ((x as f32 + 0.5 - w.cx - 1.0) / w.rx, (y as f32 + 0.5 - w.cy - 1.5) / w.ry);
            if u * u + v * v <= 1.0 {
                cv.blend(x, y, rgb(0, 0, 0), 0.4);
            }
        }
    }
    for y in (w.cy - w.ry) as i32..=(w.cy + w.ry) as i32 {
        for x in (w.cx - w.rx) as i32..=(w.cx + w.rx) as i32 {
            let mut acc = [0.0f32; 3];
            let mut n = 0.0;
            for j in 0..2 {
                for i in 0..2 {
                    let (u, v) = ((x as f32 + 0.25 + i as f32 * 0.5 - w.cx) / w.rx, (y as f32 + 0.25 + j as f32 * 0.5 - w.cy) / w.ry);
                    if let Some(c) = wheel_color(u, v, turn) {
                        acc = [acc[0] + c[0], acc[1] + c[1], acc[2] + c[2]];
                        n += 1.0;
                    }
                }
            }
            if n > 0.0 {
                cv.blend(x, y, scale(acc, 1.0 / n), n / 4.0);
            }
        }
    }
    if let Some((a, r)) = ball {
        let (x, y) = (w.cx + r * w.rx * a.cos(), w.cy + r * w.ry * a.sin());
        let (px, py) = (x.floor() as i32, y.floor() as i32);
        // An ivory ball, two pixels, its shadow under it.
        cv.blend(px, py + 1, rgb(0, 0, 0), 0.6);
        cv.blend(px + 1, py + 1, rgb(0, 0, 0), 0.6);
        cv.set(px, py, rgb(255, 255, 255));
        cv.set(px + 1, py, rgb(205, 205, 215));
    }
}

/// The ball's path on a spin landing in pocket `target`: where the
/// wheel's turned and the ball is at `t` seconds, and whether it's
/// settled.
struct Spin {
    turn0: f32,
    ball0: f32,
    drop: f32,
    rel_drop: f32,
    rel_vel: f32,
    rel_end: f32,
}

const WHEEL_SPEED: f32 = 1.6;
const WHEEL_SLOW: f32 = 8.0;
const BALL_SPEED: f32 = 9.5;
const BALL_SLOW: f32 = 2.4;
const DROP_AT: f32 = 3.4;
const SETTLE: f32 = 1.9;

impl Spin {
    fn new(turn0: f32, ball0: f32, target: u8) -> Spin {
        let mut s = Spin { turn0, ball0, drop: DROP_AT, rel_drop: 0.0, rel_vel: 0.0, rel_end: 0.0 };
        let (turn, ball) = (s.turn(DROP_AT), s.track(DROP_AT));
        s.rel_drop = ball - turn;
        s.rel_vel = -BALL_SPEED * (-DROP_AT / BALL_SLOW).exp() - WHEEL_SPEED * (-DROP_AT / WHEEL_SLOW).exp();
        // Land in the pocket about where its speed would carry it.
        let i = WHEEL.iter().position(|&n| n == target).unwrap_or(0);
        let want = s.rel_drop + s.rel_vel * SETTLE * 0.5;
        let p = pocket_angle(i);
        s.rel_end = p + ((want - p) / (2.0 * PI)).round() * 2.0 * PI;
        s
    }

    fn turn(&self, t: f32) -> f32 {
        self.turn0 + WHEEL_SPEED * WHEEL_SLOW * (1.0 - (-t / WHEEL_SLOW).exp())
    }

    fn track(&self, t: f32) -> f32 {
        self.ball0 - BALL_SPEED * BALL_SLOW * (1.0 - (-t / BALL_SLOW).exp())
    }

    /// The wheel's turn, and the ball: its angle and how far out.
    fn at(&self, t: f32) -> (f32, f32, f32) {
        let turn = self.turn(t);
        if t < self.drop {
            return (turn, self.track(t), TRACK_R);
        }
        let s = ((t - self.drop) / SETTLE).min(1.0);
        // Down off the track, then smoothly to its pocket, the speed it
        // had carried in; bouncing over the frets as it goes.
        let (h00, h10, h01) = (2.0 * s.powi(3) - 3.0 * s * s + 1.0, s.powi(3) - 2.0 * s * s + s, -2.0 * s.powi(3) + 3.0 * s * s);
        let wobble = 0.12 * (1.0 - s).powi(2) * (s * 23.0).sin();
        let rel = h00 * self.rel_drop + h10 * self.rel_vel * SETTLE + h01 * self.rel_end + wobble;
        let tau = t - self.drop;
        let r = POCKET_R + (TRACK_R - POCKET_R) * (-2.6 * tau).exp() * (tau * 7.5).cos().abs();
        (turn, turn + rel, r)
    }

    fn done(&self, t: f32) -> bool {
        t > self.drop + SETTLE + 0.6
    }
}

// ---- the table on screen -----------------------------------------------------

struct Table {
    bets: Vec<(Bet, u32)>,
    cursor: (i32, i32),
    turn: f32,
    ball: Option<(f32, f32)>,
    /// The last numbers, newest first.
    history: Vec<u8>,
    /// The number just up, lit on the layout.
    lit: Option<u8>,
}

impl Table {
    fn staked(&self, b: Bet) -> u32 {
        self.bets.iter().filter(|x| x.0 == b).map(|x| x.1).sum()
    }

    fn draw(&self, cv: &mut Canvas, t: f32) {
        cv.px.clone_from(&felt().px);
        cv.clear_labels();
        draw_wheel(cv, TABLE_WHEEL, self.turn, self.ball);
        // The number up, on the turret.
        if let (Some(n), None) = (self.lit, self.ball.filter(|_| false)) {
            let bg = match n {
                0 => rgb(25, 130, 65),
                n if red(n) => rgb(185, 25, 35),
                _ => rgb(25, 22, 28),
            };
            cv.label(38, 6, &format!("{n:>2} "), rgb(255, 255, 255), Some(bg));
        }
        let cur = spot_at(self.cursor.0, self.cursor.1);
        let mut spots: Vec<Bet> = (0..=36).map(Bet::Number).collect();
        spots.extend([Bet::Column(0), Bet::Column(1), Bet::Column(2), Bet::Dozen(0), Bet::Dozen(1), Bet::Dozen(2)]);
        spots.extend([Bet::Low, Bet::Even, Bet::Red, Bet::Black, Bet::Odd, Bet::High]);
        let pulse = 0.5 + 0.5 * (t * 6.0).sin();
        for b in spots {
            let (x, y, w, h) = spot_box(b);
            let won = self.lit.is_some_and(|n| b.wins(n)) && self.ball.is_some();
            if won {
                for yy in y + 1..y + h {
                    for xx in x + 1..x + w {
                        cv.blend(xx, yy, rgb(255, 220, 120), 0.35 + 0.3 * pulse);
                    }
                }
            }
            if b == cur {
                // The cursor: a bright frame.
                let c = mix(rgb(255, 255, 255), rgb(255, 210, 90), pulse);
                for xx in x..=x + w {
                    cv.set(xx, y, c);
                    cv.set(xx, y + h, c);
                }
                for yy in y..=y + h {
                    cv.set(x, yy, c);
                    cv.set(x + w, yy, c);
                }
            }
            let label = spot_label(b);
            let (row, col) = ((y / 2) as usize, (x + 1) as usize);
            let staked = self.staked(b);
            let fg = if b == cur { rgb(255, 240, 160) } else { rgb(245, 245, 235) };
            match b {
                Bet::Number(0) => {
                    cv.label(col + 1, row + 2, "0", fg, None);
                    if staked > 0 {
                        cv.label(col, row + 4, &format!("{staked:<3}"), rgb(20, 20, 20), Some(rgb(240, 200, 80)));
                    }
                }
                Bet::Number(_) | Bet::Column(_) => {
                    // The number on the cell's lower half (the upper has its
                    // border), the chips over it.
                    let under = cv.get(x + 2, y + 2);
                    cv.label(col, row + 1, &label, fg, under);
                    if staked > 0 {
                        cv.label(col, row, &format!("{staked:<3}"), rgb(20, 20, 20), Some(rgb(240, 200, 80)));
                    }
                }
                _ => {
                    let width = w as usize - 1;
                    let text = if staked > 0 { format!("{label} {staked}") } else { label };
                    let pad = width.saturating_sub(text.chars().count()) / 2;
                    let under = cv.get(x + 2, y + 1);
                    cv.label(col + pad, row, &text, fg, under);
                    if staked > 0 {
                        let s = staked.to_string();
                        cv.label(col + pad + text.chars().count() - s.len(), row, &s, rgb(20, 20, 20), Some(rgb(240, 200, 80)));
                    }
                }
            }
        }
        // The board of the last numbers.
        cv.label(76, 13, "last", rgb(200, 200, 190), None);
        for (i, &n) in self.history.iter().take(7).enumerate() {
            let bg = match n {
                0 => rgb(25, 130, 65),
                n if red(n) => rgb(185, 25, 35),
                _ => rgb(25, 22, 28),
            };
            let col = if red(n) || n == 0 { 76 } else { 78 };
            cv.label(col, 14 + i, &format!("{n:>2}"), rgb(255, 255, 255), Some(bg));
        }
    }
}

// ---- the game ----------------------------------------------------------------

impl Session {
    pub(crate) async fn roulette(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.roulette_plain(cfg, purse).await;
        }
        let mut st = self.stage_open();
        let r = self.roulette_table(cfg, purse, &mut st).await;
        self.stage_close();
        r
    }

    async fn roulette_frame(&mut self, st: &mut Stage, tb: &Table) -> io::Result<()> {
        let t = st.t();
        tb.draw(&mut st.cv, t);
        self.stage_show(st, true).await?;
        self.flush().await
    }

    fn roulette_status(&mut self, st: &Stage, purse: u32, unit: u32, tb: &Table) {
        let on: u32 = tb.bets.iter().map(|b| b.1).sum();
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        for (label, value) in [("chips ", purse), ("  on table ", on), ("  chip ", unit)] {
            ink(&mut out, [140, 200, 150], st.truecolor);
            out.extend(label.as_bytes());
            ink(&mut out, [255, 230, 140], st.truecolor);
            out.extend(value.to_string().as_bytes());
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let keys = self.strings().get("RouletteKeys").to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(&keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    async fn roulette_table(&mut self, cfg: &Casino, purse: &mut Purse, st: &mut Stage) -> io::Result<()> {
        let mut tb = Table { bets: Vec::new(), cursor: (7, 4), turn: 0.4, ball: None, history: Vec::new(), lit: None };
        let mut unit = cfg.min_bet;
        let hello = painted(&crate::editor::strip_colours(self.strings().get("RouletteHello")), [235, 200, 110]);
        self.stage_line(MSG_ROW, &hello, st.truecolor);
        loop {
            self.roulette_status(st, purse.chips, unit, &tb);
            // The cursor glows while it waits.
            let k = loop {
                self.roulette_frame(st, &tb).await?;
                if self.key_waiting().await? {
                    break self.read_key().await?;
                }
                tokio::time::sleep(self.idle_pause(60)).await;
            };
            let (gx, gy) = tb.cursor;
            let step = |dx: i32| {
                // Along the row to the next spot that's different.
                let here = spot_at(gx, gy);
                let mut x = gx;
                loop {
                    let next = x + dx;
                    if !(0..=13).contains(&next) {
                        return x;
                    }
                    x = next;
                    if spot_at(x, gy) != here {
                        return x;
                    }
                }
            };
            match k {
                Key::Left => tb.cursor.0 = step(-1),
                Key::Right => tb.cursor.0 = step(1),
                Key::Up => tb.cursor.1 = (gy - 1).max(0),
                Key::Down => tb.cursor.1 = (gy + 1).min(4),
                Key::Enter | Key::Char(b' ') => {
                    let b = spot_at(gx, gy);
                    let room = cfg.max_bet.saturating_sub(tb.staked(b));
                    let add = unit.min(room).min(purse.chips);
                    if add > 0 {
                        purse.stake(add);
                        tb.bets.push((b, add));
                        self.purse_save(purse);
                    }
                }
                Key::Esc => {
                    self.roulette_refund(purse, &mut tb);
                    return Ok(());
                }
                k => match k.upper() {
                    Some('Q') => {
                        self.roulette_refund(purse, &mut tb);
                        return Ok(());
                    }
                    Some('+' | '=') => unit = crate::craps::next_unit(unit, cfg, true),
                    Some('-' | '_') => unit = crate::craps::next_unit(unit, cfg, false),
                    Some('X') => {
                        let b = spot_at(gx, gy);
                        purse.unstake(tb.staked(b));
                        tb.bets.retain(|x| x.0 != b);
                        self.purse_save(purse);
                    }
                    Some('C') => self.roulette_refund(purse, &mut tb),
                    Some('S') | Some('R') => {
                        if tb.bets.is_empty() {
                            let s = painted(&crate::editor::strip_colours(self.strings().get("RouletteNoBet")), [190, 170, 200]);
                            self.stage_line(MSG_ROW, &s, st.truecolor);
                            continue;
                        }
                        self.roulette_spin(purse, st, &mut tb).await?;
                    }
                    _ => {}
                },
            }
        }
    }

    fn roulette_refund(&mut self, purse: &mut Purse, tb: &mut Table) {
        purse.unstake(tb.bets.iter().map(|b| b.1).sum::<u32>());
        tb.bets.clear();
        self.purse_save(purse);
    }

    async fn roulette_spin(&mut self, purse: &mut Purse, st: &mut Stage, tb: &mut Table) -> io::Result<()> {
        let n = WHEEL[rand::rng().random_range(0..37)];
        tb.lit = None;
        let spin = Spin::new(tb.turn, rand::rng().random_range(0.0..2.0 * PI), n);
        let begin = st.t();
        let call = painted(&crate::editor::strip_colours(self.strings().get("RouletteNoMore")), [215, 200, 225]);
        self.stage_line(MSG_ROW, &call, st.truecolor);
        loop {
            let t = st.t() - begin;
            let (turn, a, r) = spin.at(t);
            tb.turn = turn;
            tb.ball = Some((a, r));
            self.roulette_frame(st, tb).await?;
            if spin.done(t) {
                break;
            }
        }
        // Pay what won, take what lost.
        let mut back = 0;
        let staked: u32 = tb.bets.iter().map(|b| b.1).sum();
        for &(b, stake) in &tb.bets {
            back += b.returned(n, stake);
        }
        purse.chips += back;
        self.purse_save(purse);
        tb.lit = Some(n);
        tb.history.insert(0, n);
        let won = back as i64 - staked as i64;
        let text = match won {
            w if w > 0 => format!("{n} {}!  you win {w}", color_name(n)),
            0 => format!("{n} {}.  even", color_name(n)),
            w => format!("{n} {}.  the house takes {}", color_name(n), -w),
        };
        let good = won > 0;
        let until = st.t() + 2.0;
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
            self.roulette_frame(st, tb).await?;
            if self.key_waiting().await? {
                break;
            }
        }
        self.stage_line(MSG_ROW, &painted(&text, if good { [255, 215, 110] } else { [215, 200, 225] }), st.truecolor);
        // The table's cleared for the next spin; the ball stays where it fell.
        tb.bets.clear();
        tb.ball = None;
        let (_, a, r) = spin.at(spin.drop + SETTLE + 0.6);
        tb.ball = Some((a, r));
        self.roulette_frame(st, tb).await
    }

    /// Plain text: bets typed, a line a spin.
    async fn roulette_plain(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        let mut bets: Vec<(Bet, u32)> = Vec::new();
        loop {
            let on = if bets.is_empty() { "none".to_string() } else { bets.iter().map(|(b, s)| format!("{} {s}", b.name())).collect::<Vec<_>>().join(", ") };
            let s = self.strings().fmt("RoulettePlainPrompt", &[&purse.chips.to_string(), &on]);
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(20)).await?;
            self.newline();
            let mut w = s.split_whitespace();
            let first = w.next().unwrap_or("").to_ascii_lowercase();
            match first.as_str() {
                "q" => {
                    purse.unstake(bets.iter().map(|b| b.1).sum::<u32>());
                    self.purse_save(purse);
                    return Ok(());
                }
                "" | "s" if !bets.is_empty() => {
                    let n = WHEEL[rand::rng().random_range(0..37)];
                    let staked: u32 = bets.iter().map(|b| b.1).sum();
                    let back: u32 = bets.iter().map(|&(b, s)| b.returned(n, s)).sum();
                    purse.chips += back;
                    let won = back as i64 - staked as i64;
                    self.write(&format!("the ball lands on {n} {}. {}", color_name(n), if won >= 0 { format!("+{won}") } else { won.to_string() }));
                    self.newline();
                    bets.clear();
                }
                "x" => {
                    purse.unstake(bets.iter().map(|b| b.1).sum::<u32>());
                    bets.clear();
                }
                spot => {
                    if let Some(b) = Bet::parse(spot) {
                        let amount: u32 = w.next().and_then(|n| n.parse().ok()).unwrap_or(cfg.min_bet);
                        let staked: u32 = bets.iter().filter(|x| x.0 == b).map(|x| x.1).sum();
                        let add = amount.max(cfg.min_bet).min(cfg.max_bet.saturating_sub(staked)).min(purse.chips);
                        if add > 0 {
                            purse.stake(add);
                            bets.push((b, add));
                        }
                    }
                }
            }
            self.purse_save(purse);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bets_pay_as_the_table_says() {
        assert_eq!(Bet::Number(17).returned(17, 10), 360);
        assert_eq!(Bet::Number(17).returned(16, 10), 0);
        assert_eq!(Bet::Red.returned(1, 10), 20);
        assert_eq!(Bet::Red.returned(0, 10), 0, "zero takes the outside");
        assert_eq!(Bet::Dozen(1).returned(24, 10), 30);
        assert_eq!(Bet::Column(0).returned(34, 10), 30, "34 is in the first column");
        assert_eq!(Bet::Even.returned(0, 10), 0);
        assert_eq!(Bet::Number(0).returned(0, 5), 180);
        assert_eq!(WHEEL.iter().filter(|&&n| red(n)).count(), 18);
        let mut all: Vec<u8> = WHEEL.to_vec();
        all.sort();
        assert_eq!(all, (0..=36).collect::<Vec<u8>>());
    }

    #[test]
    fn the_cursor_reaches_every_spot() {
        let mut seen = std::collections::HashSet::new();
        for gy in 0..5 {
            for gx in 0..14 {
                seen.insert(format!("{:?}", spot_at(gx, gy)));
            }
        }
        assert_eq!(seen.len(), 37 + 3 + 3 + 6);
        assert_eq!(spot_at(1, 2), Bet::Number(1));
        assert_eq!(spot_at(12, 0), Bet::Number(36));
        assert_eq!(spot_at(13, 2), Bet::Column(0));
    }

    #[test]
    fn the_ball_lands_in_its_pocket() {
        for &n in &[0u8, 17, 32, 26, 5] {
            let s = Spin::new(1.0, 2.0, n);
            let t = s.drop + SETTLE + 0.6;
            let (turn, a, r) = s.at(t);
            let rel = (a - turn).rem_euclid(2.0 * PI) / (2.0 * PI) * 37.0;
            assert_eq!(WHEEL[rel.floor() as usize % 37], n);
            assert!((r - POCKET_R).abs() < 0.02);
            // No jump as it leaves the track.
            let (_, a0, _) = s.at(s.drop - 0.001);
            let (_, a1, _) = s.at(s.drop + 0.001);
            assert!((a0 - a1).abs() < 0.05, "{a0} {a1}");
        }
        assert!(Bet::parse("col2") == Some(Bet::Column(1)) && Bet::parse("37").is_none());
    }
}
