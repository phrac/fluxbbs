//! New: the casino (`AC`): a lobby under a neon sign, and the slot
//! machine (`slots`), video poker (`poker`), blackjack (`blackjack`),
//! roulette (`roulette`) and craps (`craps`) off it.
//! Its games share a stage: a canvas of half-block pixels on rows 1 to 22
//! (`pixels`), a message line under it and a status line at the bottom.
//!
//! The tables play for chips: a free stack a day (`[casino]
//! daily_chips`). Leaving a table up pays the winnings in file points and
//! minutes and puts the stack back to the day's; down, the rest of the
//! stack is all there is till tomorrow. Nobody loses anything real.

use std::io;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::config::{Casino, Prize};
use crate::input::Key;
use crate::pixels::{Canvas, Encoder, hash, mix, rgb, scale, smooth};
use crate::session::Session;

/// The canvas: the whole width, rows 1 to 22.
pub const W: usize = 80;
pub const H: usize = 44;
/// Rows under the canvas: the message line and the status line.
pub const MSG_ROW: u16 = 23;
pub const STATUS_ROW: u16 = 24;

/// A color for text: 8-bit channels.
pub type Ink = [u8; 3];

pub fn shade(a: Ink, b: Ink, t: f32) -> Ink {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8)
}

pub fn painted(text: &str, color: Ink) -> Vec<(char, Ink)> {
    text.chars().map(|c| (c, color)).collect()
}

/// What's on screen: the canvas, what the terminal has of it, and the
/// clock the animations run by.
pub struct Stage {
    pub cv: Canvas,
    pub enc: Encoder,
    pub truecolor: bool,
    /// Pacing by the terminal's answers, until it doesn't answer.
    pub sync: bool,
    pub clock: Instant,
}

impl Stage {
    /// Seconds since the stage went up.
    pub fn t(&self) -> f32 {
        self.clock.elapsed().as_secs_f32()
    }
}

/// The SGR for a text color.
pub fn ink(out: &mut Vec<u8>, c: Ink, truecolor: bool) {
    if truecolor {
        out.extend(format!("\x1b[38;2;{};{};{}m", c[0], c[1], c[2]).as_bytes());
    } else {
        out.extend(format!("\x1b[38;5;{}m", crate::picture::xterm256(c)).as_bytes());
    }
}

impl Session {
    /// Clear the screen for a stage, cursor hidden.
    pub(crate) fn stage_open(&mut self) -> Stage {
        let truecolor = self.screen.truecolor() || self.screen.charset() == fluxbbs_term::Charset::Utf8;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.screen.write_bytes(b"\x1b[?25l");
        // Start counting afresh: nothing old is still owed.
        self.cursor_asks = self.cursor_asks.min(self.cursor_reports);
        Stage { cv: Canvas::new(W, H), enc: Encoder::new(truecolor, W, H), truecolor, sync: true, clock: Instant::now() }
    }

    pub(crate) fn stage_close(&mut self) {
        self.screen.write_bytes(b"\x1b[0m\x1b[?25h");
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.full_screen_done();
    }

    /// Send what changed on the canvas; with `paced`, hold the next frame
    /// until the terminal has shown this one.
    pub(crate) async fn stage_show(&mut self, st: &mut Stage, paced: bool) -> io::Result<()> {
        let out = st.enc.frame(&st.cv, 1);
        if out.is_empty() {
            return Ok(());
        }
        self.screen.write_bytes(&out);
        if paced && st.sync {
            let answering = self.cursor_reports > 0;
            let wait = Duration::from_secs(if answering { 10 } else { 3 });
            if !self.sync_output(wait, 1).await? && !answering {
                st.sync = false;
            }
        }
        Ok(())
    }

    /// A line of text centered on `row`, each character its own color.
    pub(crate) fn stage_line(&mut self, row: u16, text: &[(char, Ink)], truecolor: bool) {
        let text = &text[..text.len().min(78)];
        let mut out = format!("\x1b[{row};1H\x1b[0m\x1b[K\x1b[{row};{}H", (80 - text.len()) / 2 + 1).into_bytes();
        let mut last = None;
        for &(ch, c) in text {
            if last != Some(c) && ch != ' ' {
                ink(&mut out, c, truecolor);
                last = Some(c);
            }
            out.extend(fluxbbs_term::cp437::encode(&ch.to_string()));
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
    }
}

/// The caller's chips today, and what winnings have paid so far.
#[derive(Clone, Copy, Default)]
pub struct Purse {
    pub chips: u32,
    pub paid: Prize,
    /// Chips played today (bets made, slot pulls), for comps.
    pub played: u32,
    /// Comps had today: 1 the buffet, 2 the suite, 4 the show.
    pub comps: u8,
    pub drinks: u32,
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
        let today = crate::clock::now().date_naive().to_string();
        let v = self.state.sys.script_get("casino", &self.purse_key()).ok().flatten().unwrap_or_default();
        let mut w = v.split_whitespace();
        let day = w.next().unwrap_or("").to_string();
        let mut n = || w.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
        let (chips, minutes, points, played, comps, drinks) = (n(), n(), n(), n(), n(), n());
        if day == today {
            return Purse { chips, paid: Prize { minutes, points }, played, comps: comps as u8, drinks };
        }
        if !day.is_empty() && chips > cfg.daily_chips {
            let p = winnings(cfg, chips - cfg.daily_chips);
            self.casino_pay(p);
        }
        let p = Purse { chips: cfg.daily_chips, ..Default::default() };
        self.purse_save(&p);
        p
    }

    pub(crate) fn purse_save(&self, p: &Purse) {
        let today = crate::clock::now().date_naive();
        let v = format!("{today} {} {} {} {} {} {}", p.chips, p.paid.minutes, p.paid.points, p.played, p.comps, p.drinks);
        if let Err(e) = self.state.sys.script_set("casino", &self.purse_key(), Some(&v)) {
            tracing::error!("casino: saving {}'s chips: {e}", self.user.handle);
        }
    }

    /// Leaving a table up: what's over the day's stack is paid, and the
    /// stack goes back to that. Returns what was paid.
    pub(crate) fn cash_out(&mut self, cfg: &Casino, p: &mut Purse) -> Option<Prize> {
        let up = p.chips.checked_sub(cfg.daily_chips).filter(|&n| n > 0)?;
        let prize = winnings(cfg, up);
        p.chips = cfg.daily_chips;
        p.paid.minutes += prize.minutes;
        p.paid.points += prize.points;
        self.purse_save(p);
        self.casino_pay(prize);
        tracing::info!("{}: casino: cashed out {up} chips (+{} min, +{} points)", self.user.handle, prize.minutes, prize.points);
        Some(prize)
    }

    /// Play outside the tables (a slot pull) counted toward comps.
    pub(crate) fn casino_played(&mut self, chips: u32) {
        let cfg = self.state.config().casino.clone();
        let mut p = self.purse(&cfg);
        p.played += chips;
        self.purse_save(&p);
    }

    /// The pause between frames of an animation waiting on the caller:
    /// `busy` while they're playing, slower once they've left it alone a
    /// while (less to send down a slow line).
    pub(crate) fn idle_pause(&self, busy: u64) -> Duration {
        let quiet = self.last_key.elapsed();
        Duration::from_millis(if quiet > Duration::from_secs(60) {
            2000
        } else if quiet > Duration::from_secs(15) {
            busy.max(500)
        } else {
            busy
        })
    }

    fn casino_pay(&mut self, p: Prize) {
        if p.minutes == 0 && p.points == 0 {
            return;
        }
        self.user.time_today += p.minutes;
        self.user.file_points = self.user.file_points.saturating_add(p.points);
        if let Err(e) = self.save_own_user() {
            tracing::error!("casino: paying {}: {e}", self.user.handle);
        }
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

fn draw_lobby(cv: &mut Canvas, t: f32, pick: usize) {
    cv.px.clone_from(&lobby_room().px);
    cv.clear_labels();
    // The spotlight on the game picked.
    let cx = GAMES[pick].2;
    for y in 0..H as i32 {
        let spread = 4.0 + y as f32 * 0.32;
        for x in 0..W as i32 {
            let d = ((x as f32 - cx).abs() - spread).max(0.0);
            let k = (1.0 - d / 3.0).clamp(0.0, 1.0) * if y < 33 { 0.13 } else { 0.25 };
            if k > 0.0 {
                cv.blend(x, y, rgb(255, 230, 190), k);
            }
        }
    }
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
    draw_slot_machine(cv, GAMES[1].2, t);
    draw_poker_machine(cv, GAMES[2].2, t);
    draw_bj_table(cv, GAMES[3].2);
    draw_wheel_table(cv, GAMES[4].2, t);
    draw_dice_table(cv, GAMES[5].2);
    for (i, &(key, name, x)) in GAMES.iter().enumerate() {
        let s = format!("{} {name}", key.to_ascii_lowercase());
        let (fg, bg) = if i == pick { (rgb(255, 255, 255), Some(rgb(150, 40, 120))) } else { (rgb(170, 140, 170), None) };
        // The bar and the club are through doors at the bottom.
        let (col, row) = match name {
            "bar" => (1, 21),
            "players club" => (W - s.len() - 3, 21),
            _ => ((x as usize).saturating_sub(s.len() / 2 + 1), 20),
        };
        cv.label(col, row, &format!(" {s} "), fg, bg);
    }
}

/// A little roulette wheel, turning, the ball going round it.
fn draw_wheel_table(cv: &mut Canvas, cx: f32, t: f32) {
    for y in 26..40 {
        for x in (cx - 11.0) as i32..=(cx + 11.0) as i32 {
            let (u, v) = ((x as f32 - cx) / 11.0, (y as f32 - 33.0) / 6.0);
            if u * u + v * v <= 1.0 {
                cv.set(x, y, mix(rgb(35, 130, 70), rgb(12, 60, 30), smooth(0.0, 1.0, u * u + v * v)));
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
fn draw_bj_table(cv: &mut Canvas, cx: f32) {
    for y in 32..40 {
        for x in (cx - 14.0) as i32..=(cx + 14.0) as i32 {
            let u = (x as f32 - cx) / 14.0;
            let v = (y as f32 - 32.0) / 7.5;
            let d = u * u + v * v;
            if d <= 1.0 {
                let rail = d > 0.78;
                cv.set(x, y, if rail { mix(rgb(120, 60, 25), rgb(60, 25, 10), v) } else { mix(rgb(35, 130, 70), rgb(15, 70, 35), v) });
            }
        }
    }
    let ace = crate::blackjack::card_image(Some(crate::blackjack::Card { rank: 1, suit: 0 }));
    let king = crate::blackjack::card_image(Some(crate::blackjack::Card { rank: 13, suit: 1 }));
    crate::blackjack::blit_card(cv, &king, cx - 11.0, 17.0, 1.0, 0.0);
    crate::blackjack::blit_card(cv, &ace, cx - 4.0, 15.0, 1.0, 0.0);
}

/// Two red dice on felt, showing seven.
fn draw_dice_table(cv: &mut Canvas, cx: f32) {
    for y in 30..40 {
        for x in (cx - 10.0) as i32..=(cx + 10.0) as i32 {
            let (u, v) = ((x as f32 - cx) / 10.0, (y as f32 - 35.0) / 5.0);
            if u * u + v * v <= 1.0 {
                cv.set(x, y, mix(rgb(35, 130, 70), rgb(12, 60, 30), smooth(0.0, 1.0, u * u + v * v)));
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
        let cfg = self.state.config().casino.clone();
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
                draw_lobby(&mut st.cv, t, pick);
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
        let up = purse.chips.saturating_sub(cfg.daily_chips);
        let said = match self.cash_out(cfg, &mut purse) {
            Some(p) => {
                let s = self.strings().fmt("CasinoCashOut", &[&up.to_string(), &self.casino_prize_text(p)]);
                Some(painted(&crate::editor::strip_colours(&s), [255, 215, 110]))
            }
            None if purse.chips < before => {
                let s = self.strings().fmt("CasinoDown", &[&(before - purse.chips).to_string(), &purse.chips.to_string()]);
                Some(painted(&crate::editor::strip_colours(&s), [200, 170, 210]))
            }
            None => None,
        };
        if self.plain()
            && let Some(s) = &said {
                let line: String = s.iter().map(|c| c.0).collect();
                self.write(&line);
                self.newline();
            }
        Ok(said)
    }

    /// "+20 points  +8 min", leaving out what's nothing.
    pub(crate) fn casino_prize_text(&self, p: Prize) -> String {
        let mut parts = Vec::new();
        if p.points > 0 && self.state.config().files.use_file_points {
            parts.push(format!("+{} points", p.points));
        }
        if p.minutes > 0 {
            parts.push(format!("+{} min", p.minutes));
        }
        if parts.is_empty() { "nothing yet".into() } else { parts.join("  ") }
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
            out.extend(self.casino_prize_text(purse.paid).as_bytes());
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
