//! New: the casino (`AC`): a lobby under a neon sign, and the slot
//! machine (`slots`), video poker (`poker`), blackjack (`blackjack`),
//! roulette (`roulette`) and craps (`craps`) off it.
//! Its games share a stage: a canvas of half-block pixels on rows 1 to 22
//! (`pixels`), a message line under it and a status line at the bottom.
//!
//! The tables play for chips: a free stack a day (`[casino]
//! daily_chips`). Chips won stay on the stack from table to table (new:
//! they were paid out on leaving each one) until the players club's
//! cashier cashes what's over the day's stack in for file points and
//! minutes; whatever's left over at midnight is paid the next day. Down,
//! the rest of the stack is all there is till tomorrow. Nobody loses
//! anything real.

use std::io;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::config::{Casino, Prize};
use crate::input::Key;
use crate::pixels::{Canvas, hash, mix, rgb, scale, smooth};
use crate::session::Session;

/// Rows under the canvas: the message line and the status line.
pub const MSG_ROW: u16 = 23;
pub const STATUS_ROW: u16 = 24;

// The stage the games share lives in `pixels`.
pub use crate::pixels::{H, Ink, Stage, W, ink, painted, shade};

/// The caller's chips today, and what winnings have paid so far.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Purse {
    pub chips: u32,
    pub paid: Prize,
    /// Chips played today (bets made, slot pulls), for comps.
    pub played: u32,
    /// Comps had today: 1 the buffet, 2 the suite, 4 the show.
    pub comps: u8,
    pub drinks: u32,
    /// The day it's for (None: today). A table played past midnight
    /// keeps the day it began on, so the next day still starts fresh.
    pub day: Option<chrono::NaiveDate>,
}

impl Purse {
    /// Chips onto the table.
    pub fn stake(&mut self, n: u32) {
        self.chips -= n;
        self.played += n;
    }

    /// Chips taken back before they were played.
    pub fn unstake(&mut self, n: u32) {
        self.chips += n;
        self.played = self.played.saturating_sub(n);
    }

    /// As stored: "day chips minutes points played comps drinks".
    fn parse(v: &str) -> Option<Purse> {
        let mut w = v.split_whitespace();
        let day = w.next()?.parse().ok()?;
        let mut n = || w.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
        let (chips, minutes, points, played, comps, drinks) = (n(), n(), n(), n(), n(), n());
        Some(Purse { chips, paid: Prize { minutes, points }, played, comps: comps as u8, drinks, day: Some(day) })
    }

    fn encode(&self) -> String {
        let day = self.day.unwrap_or_else(|| crate::clock::now().date_naive());
        format!("{day} {} {} {} {} {} {}", self.chips, self.paid.minutes, self.paid.points, self.played, self.comps, self.drinks)
    }
}

/// What winnings of `chips` pay.
pub fn winnings(cfg: &Casino, chips: u32) -> Prize {
    let per = |n: u32| chips.checked_div(n).unwrap_or(0);
    Prize { minutes: per(cfg.chips_per_minute), points: per(cfg.chips_per_point) }
}

impl Session {
    fn purse_key(&self) -> String {
        format!("{}:chips", self.user.id)
    }

    /// Today's purse: a fresh stack the first time each day (winnings
    /// left on the table yesterday are paid then).
    pub(crate) fn purse(&mut self, cfg: &Casino) -> Purse {
        let today = crate::clock::now().date_naive();
        let fresh = Purse { chips: cfg.daily_chips, day: Some(today), ..Default::default() };
        // Read and turned over in one step, so yesterday's are paid once.
        let r = self.state.sys.script_update("casino", &self.purse_key(), |v| match v.as_deref().and_then(Purse::parse) {
            Some(p) if p.day == Some(today) => (p, 0),
            old => {
                *v = Some(fresh.encode());
                (fresh, old.map_or(0, |o| o.chips.saturating_sub(cfg.daily_chips)))
            }
        });
        let (p, left) = r.unwrap_or_else(|e| {
            tracing::error!("casino: reading {}'s chips: {e}", self.user.handle);
            (fresh, 0)
        });
        if left > 0 {
            self.casino_pay(winnings(cfg, left), left);
        }
        p
    }

    /// The purse as a table has it, stored under the day it's for.
    pub(crate) fn purse_save(&self, p: &Purse) {
        if let Err(e) = self.state.sys.script_set("casino", &self.purse_key(), Some(&p.encode())) {
            tracing::error!("casino: saving {}'s chips: {e}", self.user.handle);
        }
    }

    /// New: change the purse as stored, read and written in one step, so
    /// a stale copy can't pay twice. The purse after, and what `f` said;
    /// None if there's none stored.
    pub(crate) fn purse_change<R>(&self, f: impl FnOnce(&mut Purse) -> R) -> Option<(Purse, R)> {
        let r = self.state.sys.script_update("casino", &self.purse_key(), |v| {
            let mut p = v.as_deref().and_then(Purse::parse)?;
            let r = f(&mut p);
            *v = Some(p.encode());
            Some((p, r))
        });
        r.unwrap_or_else(|e| {
            tracing::error!("casino: changing {}'s chips: {e}", self.user.handle);
            None
        })
    }

    /// Leaving a table up: what's over the day's stack is paid, and the
    /// stack goes back to that. Returns the chips cashed and what they
    /// paid. What's cashed is the purse as stored, which `p` becomes.
    pub(crate) fn cash_out(&mut self, cfg: &Casino, p: &mut Purse) -> Option<(u32, Prize)> {
        let (now, up) = self.purse_change(|s| {
            let up = s.chips.saturating_sub(cfg.daily_chips);
            if up > 0 {
                let prize = winnings(cfg, up);
                s.chips = cfg.daily_chips;
                s.paid.minutes += prize.minutes;
                s.paid.points += prize.points;
            }
            up
        })?;
        *p = now;
        if up == 0 {
            return None;
        }
        let prize = winnings(cfg, up);
        self.casino_pay(prize, up);
        tracing::info!("{}: casino: cashed out {up} chips (+{} min, +{} points)", self.user.handle, prize.minutes, prize.points);
        Some((up, prize))
    }

    /// The cashier (the players club's `c`): what's over the day's stack
    /// paid in file points and minutes. Returns what to say.
    pub(crate) fn cashier(&mut self, cfg: &Casino, p: &mut Purse) -> String {
        let s = match self.cash_out(cfg, p) {
            Some((up, prize)) => self.strings().fmt("ClubCashed", &[&up.to_string(), &self.winnings_text(prize)]),
            None => self.strings().fmt("ClubNothingToCash", &[&cfg.daily_chips.to_string()]),
        };
        crate::editor::strip_colours(&fluxbbs_term::codes::unlit(&s))
    }

    /// Play outside the tables (a slot pull) counted toward comps.
    pub(crate) fn casino_played(&mut self, chips: u32) {
        let cfg = self.state.config().casino.tables();
        self.purse(&cfg);
        self.purse_change(|p| p.played += chips);
    }

    /// Winnings paid for `chips` cashed. New: the points go in the ledger
    /// too, so they show in "got today" and the history.
    fn casino_pay(&mut self, p: Prize, chips: u32) {
        if p.minutes == 0 && p.points == 0 {
            return;
        }
        self.user.time_today += p.minutes;
        self.user.file_points = self.user.file_points.saturating_add(p.points);
        if let Err(e) = self.save_own_user() {
            tracing::error!("casino: paying {}: {e}", self.user.handle);
        }
        crate::points::record(&self.state, "casino", (self.user.id, &self.user.handle), p.points, "", &chips.to_string());
    }
}

// ---- the lobby ---------------------------------------------------------------

/// The places off the lobby, in the order the arrows walk them: key,
/// name, where (the games on the floor; the bar and the club by the
/// doors at the bottom).
const GAMES: [(char, &str, f32); 7] = [
    ('D', "bar", 8.0),
    ('S', "slots", 9.0),
    ('V', "video poker", 24.0),
    ('B', "blackjack", 39.0),
    ('R', "roulette", 58.0),
    ('C', "craps", 72.0),
    ('P', "players club", 70.0),
];

/// The room without what moves: the wall, the carpet, the sign's glow.
fn lobby_room() -> &'static Canvas {
    static ROOM: OnceLock<Canvas> = OnceLock::new();
    ROOM.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let c = if y < 33 {
                    // A dark wall, columns of light falling down it.
                    let column = 0.5 + 0.5 * ((x as f32) * 0.39).sin();
                    let wall = mix(rgb(40, 8, 46), rgb(14, 3, 22), y as f32 / 33.0);
                    mix(wall, rgb(90, 30, 90), 0.12 * column * (1.0 - y as f32 / 33.0))
                } else {
                    // Casino carpet: rings and dots on deep red.
                    let (tx, ty) = ((x.rem_euclid(8)) as f32 - 3.5, (y.rem_euclid(6)) as f32 - 2.5);
                    let d = (tx * tx / 1.6 + ty * ty).sqrt();
                    let base = mix(rgb(95, 12, 22), rgb(40, 4, 10), (y - 33) as f32 / 11.0 * 0.4 + 0.3);
                    let c = if (2.0..2.7).contains(&d) {
                        mix(base, rgb(30, 110, 110), 0.55)
                    } else if d < 0.7 {
                        mix(base, rgb(220, 170, 60), 0.7)
                    } else {
                        base
                    };
                    // Further back, darker.
                    scale(c, 0.65 + 0.35 * (y - 33) as f32 / 10.0)
                };
                cv.set(x, y, c);
            }
        }
        cv
    })
}

/// The neon sign's tubes: where each lit pixel is, and which letter.
fn sign_pixels() -> &'static Vec<(i32, i32, usize)> {
    static SIGN: OnceLock<Vec<(i32, i32, usize)>> = OnceLock::new();
    SIGN.get_or_init(|| {
        let word = "CASINO";
        let x0 = (W as i32 - crate::pixels::text_width(word) * 2) / 2;
        let mut out = Vec::new();
        let mut at = x0;
        for (i, ch) in word.chars().enumerate() {
            let mut cv = Canvas::new(W, 16);
            let w = cv.print(at, 1, &ch.to_string(), 2, |_, _| rgb(255, 255, 255));
            for y in 0..16 {
                for x in 0..W as i32 {
                    if cv.px[(y * W as i32 + x) as usize][0] > 0.0 {
                        out.push((x, y + 1, i));
                    }
                }
            }
            at += w + 2;
        }
        out
    })
}

fn draw_lobby(cv: &mut Canvas, t: f32, pick: usize, ug: bool) {
    cv.px.clone_from(&if ug { crate::casinoug::lobby_room() } else { lobby_room() }.px);
    cv.clear_labels();
    // The spotlight on the game picked.
    let cx = GAMES[pick].2;
    for y in 0..H as i32 {
        let spread = 4.0 + y as f32 * 0.32;
        for x in 0..W as i32 {
            let d = ((x as f32 - cx).abs() - spread).max(0.0);
            let k = (1.0 - d / 3.0).clamp(0.0, 1.0) * if y < 33 { 0.13 } else { 0.25 };
            if k > 0.0 {
                cv.blend(x, y, if ug { rgb(190, 255, 230) } else { rgb(255, 230, 190) }, k);
            }
        }
    }
    if ug {
        crate::casinoug::draw_lobby(cv, t, sign_pixels());
    } else {
        draw_lobby_sign(cv, t);
    }
    if ug {
        crate::casinoug::slot_machine(cv, GAMES[1].2, t);
        crate::casinoug::poker_machine(cv, GAMES[2].2, t);
    } else {
        draw_slot_machine(cv, GAMES[1].2, t);
        draw_poker_machine(cv, GAMES[2].2, t);
    }
    draw_bj_table(cv, GAMES[3].2, ug);
    draw_wheel_table(cv, GAMES[4].2, t, ug);
    draw_dice_table(cv, GAMES[5].2, ug);
    for (i, &(key, name, x)) in GAMES.iter().enumerate() {
        let s = format!("{} {name}", key.to_ascii_lowercase());
        let (fg, bg) = match (i == pick, ug) {
            (true, true) => {
                let (fg, bg) = crate::casinoug::pick_colors();
                (fg, Some(bg))
            }
            (true, false) => (rgb(255, 255, 255), Some(rgb(150, 40, 120))),
            (false, true) => (crate::casinoug::label_color(), None),
            (false, false) => (rgb(170, 140, 170), None),
        };
        // The bar and the club are through doors at the bottom.
        let (col, row) = match name {
            "bar" => (1, 21),
            "players club" => (W - s.len() - 3, 21),
            _ => ((x as usize).saturating_sub(s.len() / 2 + 1), 20),
        };
        cv.label(col, row, &format!(" {s} "), fg, bg);
    }
}

/// The classic sign: pink tubes and the bulbs chasing under them.
fn draw_lobby_sign(cv: &mut Canvas, t: f32) {
    // The sign: pink tubes with a glow, its S flickering now and then.
    let flicker = {
        let beat = (t * 7.0) as i32;
        hash(beat, 7) < 0.08
    };
    let tubes = sign_pixels();
    for &(x, y, letter) in tubes {
        let dim = flicker && letter == 2;
        if dim {
            continue;
        }
        for dy in -2..=2 {
            for dx in -2..=2 {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if d > 0.0 {
                    cv.blend(x + dx, y + dy, rgb(255, 60, 190), 0.05 / d);
                }
            }
        }
    }
    for &(x, y, letter) in tubes {
        let dim = flicker && letter == 2;
        let c = if dim { rgb(90, 30, 70) } else { mix(rgb(255, 110, 215), rgb(255, 235, 250), 0.35 + 0.15 * (t * 3.0 + x as f32 * 0.2).sin()) };
        cv.set(x, y, c);
    }
    // Bulbs chasing under the sign.
    for (i, x) in (6..W as i32 - 6).step_by(3).enumerate() {
        let on = (i + (t * 6.0) as usize).is_multiple_of(4);
        cv.set(x, 18, if on { rgb(255, 230, 140) } else { rgb(90, 60, 40) });
        if on {
            cv.blend(x, 17, rgb(255, 200, 100), 0.35);
            cv.blend(x, 19, rgb(255, 200, 100), 0.35);
        }
    }
}

/// A little roulette wheel, turning, the ball going round it.
fn draw_wheel_table(cv: &mut Canvas, cx: f32, t: f32, ug: bool) {
    for y in 26..40 {
        for x in (cx - 11.0) as i32..=(cx + 11.0) as i32 {
            let (u, v) = ((x as f32 - cx) / 11.0, (y as f32 - 33.0) / 6.0);
            let d = u * u + v * v;
            if d <= 1.0 {
                let k = if !ug {
                    mix(rgb(35, 130, 70), rgb(12, 60, 30), smooth(0.0, 1.0, d))
                } else if d > 0.86 {
                    crate::casinoug::rail(0.0)
                } else {
                    crate::casinoug::felt(d)
                };
                cv.set(x, y, k);
            }
        }
    }
    // Too small for 37 pockets: a dozen, red and black, and the zero.
    let (cy, rx, ry) = (31.5, 9.5, 4.5);
    for y in (cy - ry) as i32..=(cy + ry) as i32 {
        for x in (cx - rx) as i32..=(cx + rx) as i32 {
            let (u, v) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
            let r = (u * u + v * v).sqrt();
            if r > 1.0 {
                continue;
            }
            let a = v.atan2(u) - t * 0.8;
            let seg = ((a.rem_euclid(2.0 * std::f32::consts::PI)) / (2.0 * std::f32::consts::PI) * 12.0) as i32;
            let c = if r > 0.82 {
                rgb(120, 65, 30)
            } else if r > 0.45 {
                match seg {
                    0 => rgb(30, 150, 70),
                    s if s % 2 == 1 => rgb(205, 30, 40),
                    _ => rgb(25, 22, 28),
                }
            } else if r > 0.2 {
                rgb(130, 75, 35)
            } else {
                rgb(230, 230, 240)
            };
            cv.set(x, y, c);
        }
    }
    let a = -t * 2.3;
    cv.set((cx + 0.9 * rx * a.cos()).round() as i32, (cy + 0.9 * ry * a.sin()).round() as i32, rgb(255, 255, 255));
}

/// A little slot machine: red and gold, three sevens, a handle.
fn draw_slot_machine(cv: &mut Canvas, cx: f32, t: f32) {
    let x0 = cx as i32 - 8;
    let lamp = if (t * 2.0) as i32 % 2 == 0 { rgb(255, 220, 90) } else { rgb(160, 90, 30) };
    for x in x0 + 4..x0 + 13 {
        cv.set(x, 21, lamp);
    }
    for y in 22..39 {
        for x in x0..x0 + 17 {
            let edge = x == x0 || x == x0 + 16 || y == 22;
            let shade = 1.0 - (x - x0) as f32 / 34.0;
            cv.set(x, y, if edge { rgb(230, 180, 60) } else { scale(rgb(185, 25, 40), shade) });
        }
    }
    for y in 24..32 {
        for x in x0 + 1..x0 + 16 {
            let edge = ((y as f32 - 27.5) / 4.0).abs();
            cv.set(x, y, mix(rgb(250, 245, 230), rgb(150, 140, 130), edge * edge));
        }
    }
    // Three sevens, four pixels wide.
    const SEVEN: [u8; 6] = [0b1111, 0b0001, 0b0010, 0b0100, 0b0100, 0b0100];
    for k in 0..3 {
        for (dy, bits) in SEVEN.iter().enumerate() {
            for dx in 0..4 {
                if bits >> (3 - dx) & 1 == 1 {
                    cv.set(x0 + 2 + k * 5 + dx, 25 + dy as i32, mix(rgb(240, 40, 40), rgb(150, 0, 20), dy as f32 / 6.0));
                }
            }
        }
    }
    cv.rect(x0 + 2, 34, 13, 2, rgb(40, 10, 15));
    // The handle.
    for y in 25..32 {
        cv.set(x0 + 18, y, rgb(190, 190, 200));
    }
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        cv.set(x0 + 17 + dx, 23 + dy, rgb(230, 30, 40));
    }
}

/// A video poker cabinet: a lit marquee, a blue screen with five cards
/// on it, a row of buttons.
fn draw_poker_machine(cv: &mut Canvas, cx: f32, t: f32) {
    let x0 = cx as i32 - 3;
    cv.rect(x0, 21, 7, 17, rgb(38, 36, 52));
    let lamp = if (t * 2.0 + 1.0) as i32 % 2 == 0 { rgb(255, 220, 90) } else { rgb(160, 90, 30) };
    cv.rect(x0 + 1, 21, 5, 1, lamp);
    cv.rect(x0 + 1, 23, 5, 7, rgb(10, 18, 135));
    for k in 0..5 {
        cv.set(x0 + 1 + k, 24, rgb(255, 215, 60));
        let card = if k % 2 == 1 { rgb(230, 60, 70) } else { rgb(245, 245, 240) };
        cv.set(x0 + 1 + k, 26, card);
        cv.set(x0 + 1 + k, 27, card);
    }
    for k in 0..5 {
        cv.set(x0 + 1 + k, 32, if k == 4 { rgb(230, 40, 40) } else { rgb(255, 210, 70) });
    }
    cv.rect(x0 + 1, 34, 5, 4, rgb(24, 22, 34));
}

/// A half-moon of felt with two cards on it: blackjack.
fn draw_bj_table(cv: &mut Canvas, cx: f32, ug: bool) {
    for y in 32..40 {
        for x in (cx - 14.0) as i32..=(cx + 14.0) as i32 {
            let u = (x as f32 - cx) / 14.0;
            let v = (y as f32 - 32.0) / 7.5;
            let d = u * u + v * v;
            if d <= 1.0 {
                let rail = d > 0.78;
                let k = match (rail, ug) {
                    (true, false) => mix(rgb(120, 60, 25), rgb(60, 25, 10), v),
                    (false, false) => mix(rgb(35, 130, 70), rgb(15, 70, 35), v),
                    (true, true) => crate::casinoug::rail(v),
                    (false, true) => crate::casinoug::felt(v),
                };
                cv.set(x, y, k);
            }
        }
    }
    let ace = crate::blackjack::card_image(Some(crate::blackjack::Card { rank: 1, suit: 0 }));
    let king = crate::blackjack::card_image(Some(crate::blackjack::Card { rank: 13, suit: 1 }));
    crate::blackjack::blit_card(cv, &king, cx - 11.0, 17.0, 1.0, 0.0);
    crate::blackjack::blit_card(cv, &ace, cx - 4.0, 15.0, 1.0, 0.0);
}

/// Two red dice on felt, showing seven.
fn draw_dice_table(cv: &mut Canvas, cx: f32, ug: bool) {
    for y in 30..40 {
        for x in (cx - 10.0) as i32..=(cx + 10.0) as i32 {
            let (u, v) = ((x as f32 - cx) / 10.0, (y as f32 - 35.0) / 5.0);
            let d = u * u + v * v;
            if d <= 1.0 {
                let k = if !ug {
                    mix(rgb(35, 130, 70), rgb(12, 60, 30), smooth(0.0, 1.0, d))
                } else if d > 0.84 {
                    crate::casinoug::rail(0.0)
                } else {
                    crate::casinoug::felt(d)
                };
                cv.set(x, y, k);
            }
        }
    }
    crate::craps::draw_die(cv, cx - 3.5, 32.0, 0.0, 0.25, 5, 8.0);
    crate::craps::draw_die(cv, cx + 4.5, 28.0, 0.0, -0.35, 2, 8.0);
}

impl Session {
    /// `AC`: the casino. `AC slots`, `AC poker`, `AC blackjack`, `AC
    /// roulette`, `AC craps`, `AC bar` or `AC club` goes straight there.
    pub async fn casino(&mut self, param: &str) -> io::Result<()> {
        let cfg = self.state.config().casino.tables();
        let p = param.trim().to_ascii_lowercase();
        if let Some(i) = GAMES.iter().position(|g| g.1 == p || (p == "club" && g.0 == 'P') || (p == "poker" && g.0 == 'V')) {
            self.casino_game(&cfg, i).await?;
            return Ok(());
        }
        if self.plain() || self.screen.rows() < 24 {
            return self.casino_plain(&cfg).await;
        }
        let mut pick = 1;
        let mut message: Option<Vec<(char, Ink)>> = None;
        loop {
            let mut st = self.stage_open();
            let purse = self.purse(&cfg);
            self.lobby_status(&st, &purse);
            let hello = match message.take() {
                Some(m) => m,
                None => painted(&crate::editor::strip_colours(&self.strings().fmt("CasinoHello", &[&self.user.handle.clone()])), [255, 150, 220]),
            };
            self.stage_line(MSG_ROW, &hello, st.truecolor);
            let since = Instant::now();
            let go = loop {
                let t = st.t();
                draw_lobby(&mut st.cv, t, pick, crate::casinoug::on(&cfg));
                self.stage_show(&mut st, true).await?;
                self.flush().await?;
                if !self.key_waiting().await? {
                    tokio::time::sleep(self.idle_pause(50)).await;
                    if since.elapsed() > Duration::from_secs(900) {
                        break None;
                    }
                    continue;
                }
                match self.read_key().await? {
                    Key::Left | Key::Up => pick = (pick + GAMES.len() - 1) % GAMES.len(),
                    Key::Right | Key::Down | Key::Tab => pick = (pick + 1) % GAMES.len(),
                    Key::Enter | Key::Char(b' ') => break Some(pick),
                    Key::Esc => break None,
                    k => match k.upper() {
                        Some('Q') => break None,
                        Some(c) => {
                            if let Some(i) = GAMES.iter().position(|g| g.0 == c) {
                                break Some(i);
                            }
                        }
                        None => {}
                    },
                }
            };
            self.stage_close();
            let Some(i) = go else { return Ok(()) };
            pick = i;
            message = self.casino_game(&cfg, i).await?;
        }
    }

    /// Play a game; a table cashes out as the caller leaves it. Returns
    /// what to say back in the lobby.
    async fn casino_game(&mut self, cfg: &Casino, i: usize) -> io::Result<Option<Vec<(char, Ink)>>> {
        let name = GAMES[i].1;
        tracing::info!("{}: casino: {name}", self.user.handle);
        match name {
            "slots" => {
                self.slots("").await?;
                return Ok(None);
            }
            "bar" | "players club" => {
                let mut purse = self.purse(cfg);
                if name == "bar" {
                    self.casino_bar(cfg, &mut purse).await?;
                } else {
                    self.casino_club(cfg, &mut purse).await?;
                }
                return Ok(None);
            }
            _ => {}
        }
        let mut purse = self.purse(cfg);
        let begun = crate::clock::now().naive_local();
        let before = purse.chips;
        match name {
            "video poker" => self.video_poker(cfg, &mut purse).await?,
            "blackjack" => self.blackjack(cfg, &mut purse).await?,
            "roulette" => self.roulette(cfg, &mut purse).await?,
            _ => self.craps(cfg, &mut purse).await?,
        }
        let title = match name {
            "video poker" => "Video Poker",
            "blackjack" => "Blackjack",
            "roulette" => "Roulette",
            _ => "Craps",
        };
        self.record_play(title, begun);
        // Chips won stay on the stack (the cashier, at the players club,
        // cashes them in).
        self.purse_save(&purse);
        let said = match purse.chips.cmp(&before) {
            std::cmp::Ordering::Greater => {
                let s = self.strings().fmt("CasinoUp", &[&(purse.chips - before).to_string(), &purse.chips.to_string()]);
                Some(painted(&crate::editor::strip_colours(&s), [255, 215, 110]))
            }
            std::cmp::Ordering::Less => {
                let s = self.strings().fmt("CasinoDown", &[&(before - purse.chips).to_string(), &purse.chips.to_string()]);
                Some(painted(&crate::editor::strip_colours(&s), [200, 170, 210]))
            }
            std::cmp::Ordering::Equal => None,
        };
        if self.plain()
            && let Some(s) = &said {
                let line: String = s.iter().map(|c| c.0).collect();
                self.write(&line);
                self.newline();
            }
        Ok(said)
    }

    fn lobby_status(&mut self, st: &Stage, purse: &Purse) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        ink(&mut out, [200, 150, 210], st.truecolor);
        out.extend(b"chips ");
        ink(&mut out, [255, 230, 140], st.truecolor);
        out.extend(purse.chips.to_string().as_bytes());
        if purse.paid.points > 0 || purse.paid.minutes > 0 {
            ink(&mut out, [200, 150, 210], st.truecolor);
            out.extend(b"   won today ");
            ink(&mut out, [255, 215, 110], st.truecolor);
            out.extend(self.winnings_text(purse.paid).as_bytes());
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let keys = self.strings().get("CasinoKeys").to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(&keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    async fn casino_plain(&mut self, cfg: &Casino) -> io::Result<()> {
        loop {
            let purse = self.purse(cfg);
            let s = self.strings().fmt("CasinoPlainPrompt", &[&purse.chips.to_string()]);
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(10)).await?;
            self.newline();
            let c = s.trim().chars().next().map(|c| c.to_ascii_uppercase());
            match c {
                Some('Q') | None => return Ok(()),
                Some(c) => {
                    if let Some(i) = GAMES.iter().position(|g| g.0 == c) {
                        self.casino_game(cfg, i).await?;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LOBBY_OUT=lobby.ans LOBBY_THEME=underground cargo test --features full -- --ignored casino::tests::lobby`
    /// draws the lobby (a frame at `LOBBY_T` seconds) for `tools/ansi2png.py`.
    #[test]
    #[ignore]
    fn lobby() {
        let Ok(out) = std::env::var("LOBBY_OUT") else { return };
        let t: f32 = std::env::var("LOBBY_T").ok().and_then(|t| t.parse().ok()).unwrap_or(1.0);
        let ug = std::env::var("LOBBY_THEME").is_ok_and(|v| v == "underground");
        let mut cv = Canvas::new(W, H);
        draw_lobby(&mut cv, t, 1, ug);
        let mut enc = crate::pixels::Encoder::new(true, W, H);
        let mut bytes = b"\x1b[2J".to_vec();
        bytes.extend(enc.frame(&cv, 1));
        bytes.extend(b"\x1b[0m\x1b[24;1H");
        std::fs::write(out, bytes).unwrap();
    }

    #[test]
    fn cashing_out_goes_in_the_ledger() {
        let state = crate::tests::state();
        let mut u = crate::user::User { handle: "phrac".into(), ..Default::default() };
        state.users.insert(&mut u).unwrap();
        let mut s = Session::for_test(state, u);
        let cfg = Casino::default();
        let mut p = Purse { chips: cfg.daily_chips + 50, ..Default::default() };
        s.purse_save(&p);
        let (up, prize) = s.cash_out(&cfg, &mut p).unwrap();
        assert_eq!(up, 50);
        assert!(prize.points > 0);
        assert_eq!(s.user.file_points, prize.points);
        let got = s.state.sys.points_recent(s.user.id, 5).unwrap();
        assert_eq!(got.iter().map(|e| (e.kind.as_str(), e.amount, e.note.as_str())).collect::<Vec<_>>(), [("casino", prize.points, "50")]);
        assert_eq!(fluxbbs_term::codes::unlit(&s.ledger_what(&got[0])), "cashed 50 chips at the casino");
    }

    fn player() -> Session {
        let state = crate::tests::state();
        let mut u = crate::user::User { handle: "phrac".into(), ..Default::default() };
        state.users.insert(&mut u).unwrap();
        Session::for_test(state, u)
    }

    #[test]
    fn a_stale_purse_cant_cash_out_twice() {
        let mut s = player();
        let cfg = Casino::default();
        let mut p = s.purse(&cfg);
        p.chips += 50;
        s.purse_save(&p);
        let mut stale = p;
        assert!(s.cash_out(&cfg, &mut p).is_some());
        assert!(s.cash_out(&cfg, &mut stale).is_none(), "the stored purse is what's cashed");
        assert_eq!(stale.chips, cfg.daily_chips);
        let points = s.user.file_points;
        assert!(points > 0);
        assert!(s.cash_out(&cfg, &mut p).is_none());
        assert_eq!(s.user.file_points, points);
    }

    #[test]
    fn a_table_past_midnight_leaves_the_new_day_fresh() {
        let mut s = player();
        let cfg = Casino::default();
        let yesterday = crate::clock::now().date_naive().pred_opt().unwrap();
        // Played from yesterday into today: saved under the day it began.
        let p = Purse { chips: cfg.daily_chips + 20, played: 500, comps: 7, drinks: 3, day: Some(yesterday), ..Default::default() };
        s.purse_save(&p);
        let today = s.purse(&cfg);
        assert_eq!((today.chips, today.played, today.comps, today.drinks), (cfg.daily_chips, 0, 0, 0));
        assert_eq!(today.day, Some(crate::clock::now().date_naive()));
        let paid = s.user.file_points;
        assert_eq!(paid, winnings(&cfg, 20).points, "yesterday's winnings paid");
        // Once.
        s.purse(&cfg);
        assert_eq!(s.user.file_points, paid);
    }

    #[test]
    fn comps_are_claimed_once_from_the_stored_purse() {
        let mut s = player();
        let cfg = Casino::default();
        let mut p = s.purse(&cfg);
        p.played = cfg.comp_suite;
        s.purse_save(&p);
        let mut stale = p;
        let (_, had) = s.comp_claim(&cfg, &mut p, 1);
        assert!(had);
        assert_eq!(p.chips, cfg.daily_chips + cfg.suite_chips);
        let (_, again) = s.comp_claim(&cfg, &mut stale, 1);
        assert!(!again, "a stale copy can't claim it again");
        assert_eq!(stale.chips, cfg.daily_chips + cfg.suite_chips);
        let (_, show) = s.comp_claim(&cfg, &mut p, 2);
        assert!(!show, "not played enough for the show");
    }

    #[test]
    fn bets_are_kept_sane() {
        let mut c = Casino { min_bet: 0, max_bet: 0, ..Casino::default() };
        c.normalize();
        assert_eq!((c.min_bet, c.max_bet), (1, 1));
        let c = Casino { min_bet: 50, max_bet: 10, ..Casino::default() }.tables();
        assert_eq!((c.min_bet, c.max_bet), (50, 50));
        assert_eq!(crate::craps::next_unit(1, &c, true), 50);
    }
}
