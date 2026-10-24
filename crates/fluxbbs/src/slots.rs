//! New: the slot machine (`AL`). Three reels fill the screen (pictures in
//! `slotart`); a pull spins them and they stop one after another, each
//! with a jolt, the third holding out when the first two match. Wins pay
//! minutes on today's time and file points, as `[slots]` says (`*C`, Slot
//! Machine), which also sets the pulls a day and how often a pull wins.
//!
//! The result is drawn first, then shown: the board picks a line with the
//! odds set (three sevens at their own odds, else a win at `win_percent`,
//! weighted towards the small ones), and the reels stop on it. A losing
//! pull sometimes stops just short of three of a kind, as real ones do.
//! The pull is counted and the prize paid before the reels move, so a
//! dropped call keeps both. Plain-text callers get a line per pull.

use std::io;
use std::time::{Duration, Instant};

use rand::RngExt;

use crate::config::{Prize, Slots};
use crate::input::Key;
use crate::session::Session;
use crate::slotart::{
    BAR, BELL, CHERRY, CIRCLE, Canvas, Coin, Encoder, INK, LEMON, NAMES, ORANGE, PLUM, ReelView, SEVEN, STOPS, STRIPS, Scene, W, bulb_spots, pos_of,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    Jackpot,
    Bars,
    Bells,
    Plums,
    Oranges,
    Lemons,
    Cherries,
    TwoCherries,
    OneCherry,
}

/// Each line, its name, and how often it comes up among the wins.
const TIERS: [(Tier, &str, u32); 9] = [
    (Tier::Jackpot, "7 7 7", 0),
    (Tier::Bars, "BAR BAR BAR", 3),
    (Tier::Bells, "three bells", 5),
    (Tier::Plums, "three plums", 8),
    (Tier::Oranges, "three oranges", 10),
    (Tier::Lemons, "three lemons", 12),
    (Tier::Cherries, "three cherries", 12),
    (Tier::TwoCherries, "two cherries", 20),
    (Tier::OneCherry, "a cherry", 30),
];

fn tier_name(t: Tier) -> &'static str {
    TIERS.iter().find(|x| x.0 == t).map_or("", |x| x.1)
}

fn prize(cfg: &Slots, t: Tier) -> Prize {
    match t {
        Tier::Jackpot => cfg.jackpot,
        Tier::Bars => cfg.bars,
        Tier::Bells => cfg.bells,
        Tier::Plums => cfg.plums,
        Tier::Oranges => cfg.oranges,
        Tier::Lemons => cfg.lemons,
        Tier::Cherries => cfg.cherries,
        Tier::TwoCherries => cfg.two_cherries,
        Tier::OneCherry => cfg.one_cherry,
    }
}

/// What the payline pays: three of a kind, else cherries from the left.
fn pays(s: [usize; 3]) -> Option<Tier> {
    if s[0] == s[1] && s[1] == s[2] {
        return Some(match s[0] {
            SEVEN => Tier::Jackpot,
            BAR => Tier::Bars,
            BELL => Tier::Bells,
            PLUM => Tier::Plums,
            ORANGE => Tier::Oranges,
            LEMON => Tier::Lemons,
            _ => Tier::Cherries,
        });
    }
    match (s[0], s[1]) {
        (CHERRY, CHERRY) => Some(Tier::TwoCherries),
        (CHERRY, _) => Some(Tier::OneCherry),
        _ => None,
    }
}

/// A pull, decided before the reels move.
struct Pull {
    stops: [usize; 3],
    win: Option<Tier>,
    /// Lost, one short of three of a kind.
    near: bool,
}

fn three(sym: usize) -> [usize; 3] {
    [sym; 3]
}

fn any(rng: &mut impl RngExt) -> usize {
    rng.random_range(0..crate::slotart::SYMBOLS)
}

/// Draw the line, then where each reel stops to show it.
fn decide(cfg: &Slots, rng: &mut impl RngExt) -> Pull {
    let jackpot = cfg.jackpot_odds > 0 && rng.random_range(0..cfg.jackpot_odds) == 0;
    let mut near = false;
    let syms = if jackpot {
        three(SEVEN)
    } else if rng.random_range(0..100) < cfg.win_percent {
        let total: u32 = TIERS.iter().map(|t| t.2).sum();
        let mut roll = rng.random_range(0..total);
        let tier = TIERS.iter().find(|t| {
            if roll < t.2 {
                return true;
            }
            roll -= t.2;
            false
        });
        match tier.map(|t| t.0) {
            Some(Tier::Bars) => three(BAR),
            Some(Tier::Bells) => three(BELL),
            Some(Tier::Plums) => three(PLUM),
            Some(Tier::Oranges) => three(ORANGE),
            Some(Tier::Lemons) => three(LEMON),
            Some(Tier::Cherries) => three(CHERRY),
            Some(Tier::TwoCherries) => loop {
                let c = any(rng);
                if c != CHERRY {
                    break [CHERRY, CHERRY, c];
                }
            },
            _ => loop {
                let (b, c) = (any(rng), any(rng));
                if b != CHERRY {
                    break [CHERRY, b, c];
                }
            },
        }
    } else if rng.random_range(0..100) < 30 {
        // So near: two of a big one, the third just off the line.
        near = true;
        let s = [SEVEN, BAR, BELL][rng.random_range(0..3)];
        loop {
            let c = any(rng);
            if c != s {
                break [s, s, c];
            }
        }
    } else {
        loop {
            let s = [any(rng), any(rng), any(rng)];
            if pays(s).is_none() {
                break s;
            }
        }
    };
    let mut stops = [0; 3];
    for (reel, &sym) in syms.iter().enumerate() {
        let mut fits: Vec<usize> = (0..STOPS).filter(|&k| STRIPS[reel][k] == sym).collect();
        if near && reel == 2 {
            let by: Vec<usize> = fits.iter().copied().filter(|&k| STRIPS[2][(k + 1) % STOPS] == syms[0] || STRIPS[2][(k + STOPS - 1) % STOPS] == syms[0]).collect();
            if !by.is_empty() {
                fits = by;
            }
        }
        stops[reel] = fits[rng.random_range(0..fits.len())];
    }
    Pull { stops, win: pays(syms), near }
}

// ---- how a reel moves ----------------------------------------------------

/// Top speed (pixels a second), the kick back as the handle's pulled,
/// and the run up to speed.
const SPEED: f32 = 760.0;
const KICK: f32 = 3.5;
const KICK_T: f32 = 0.15;
const RUN_UP: f32 = 0.3;
/// The jolt at the stop: how fast it rings and how soon it dies.
const RING: f32 = 2.0 * std::f32::consts::PI / 0.17;
const DAMP: f32 = 0.32;
const SETTLE: f32 = 0.45;

struct Motion {
    start: f32,
    from: f32,
    brake: f32,
    brake_len: f32,
    /// Speed as it hits the stop.
    hit: f32,
    at_brake: f32,
    target: f32,
}

impl Motion {
    fn new(start: f32, from: f32) -> Motion {
        Motion { start, from, brake: f32::MAX, brake_len: 0.4, hit: SPEED * 0.1, at_brake: 0.0, target: 0.0 }
    }

    /// Where it would be at `t` if it never stopped, and how fast.
    fn spin(&self, t: f32) -> (f32, f32) {
        let tau = t - self.start;
        if tau <= 0.0 {
            return (self.from, 0.0);
        }
        if tau < KICK_T {
            let w = std::f32::consts::PI / KICK_T;
            return (self.from - KICK * (w * tau).sin(), -KICK * w * (w * tau).cos());
        }
        let tau = tau - KICK_T;
        if tau < RUN_UP {
            let a = SPEED / RUN_UP;
            return (self.from + 0.5 * a * tau * tau, a * tau);
        }
        (self.from + 0.5 * SPEED * RUN_UP + SPEED * (tau - RUN_UP), SPEED)
    }

    /// Start braking about `at` (later, so it lands on stop `k`), over
    /// `len` seconds, hitting the stop at `hit`.
    fn stop_at(&mut self, at: f32, len: f32, hit: f32, k: usize) {
        let at = at.max(self.start + KICK_T + RUN_UP);
        let dist = (SPEED + hit) / 2.0 * len;
        let land = self.spin(at).0 + dist;
        let target = land + (pos_of(k) - land).rem_euclid(CIRCLE);
        self.brake = at + (target - land) / SPEED;
        self.brake_len = len;
        self.hit = hit;
        self.at_brake = self.spin(self.brake).0;
        self.target = target;
    }

    fn stopped_by(&self) -> f32 {
        self.brake + self.brake_len
    }

    /// Where and how fast at `t`, and whether it has come to rest.
    fn at(&self, t: f32) -> (f32, f32, bool) {
        if t < self.brake {
            let (p, v) = self.spin(t);
            return (p, v, false);
        }
        let tau = t - self.brake;
        if tau < self.brake_len {
            let slow = (SPEED - self.hit) / self.brake_len;
            return (self.at_brake + SPEED * tau - 0.5 * slow * tau * tau, SPEED - slow * tau, false);
        }
        let tau = tau - self.brake_len;
        let wd = RING * (1.0 - DAMP * DAMP).sqrt();
        let e = (-DAMP * RING * tau).exp();
        let x = self.hit / wd * e * (wd * tau).sin();
        let v = self.hit * e * ((wd * tau).cos() - DAMP * RING / wd * (wd * tau).sin());
        (self.target + x, v, tau > SETTLE)
    }
}

// ---- the screen ------------------------------------------------------------

/// Rows under the reels: the message line and the status line.
const MSG_ROW: u16 = 23;
const STATUS_ROW: u16 = 24;

type Rgb = [u8; 3];

fn shade(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8)
}

fn painted(text: &str, color: Rgb) -> Vec<(char, Rgb)> {
    text.chars().map(|c| (c, color)).collect()
}

/// The day's tally: pulls, minutes and points won.
#[derive(Default)]
struct Tally {
    pulls: u32,
    minutes: u32,
    points: u32,
}

impl Session {
    fn slots_key(&self) -> String {
        format!("{}:slots", self.user.id)
    }

    fn slots_today(&self) -> Tally {
        let today = crate::clock::now().date_naive().to_string();
        let v = self.state.sys.script_get("slots", &self.slots_key()).ok().flatten().unwrap_or_default();
        let mut w = v.split_whitespace();
        if w.next() != Some(today.as_str()) {
            return Tally::default();
        }
        let mut n = || w.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        Tally { pulls: n(), minutes: n(), points: n() }
    }

    fn slots_save(&self, t: &Tally) {
        let today = crate::clock::now().date_naive();
        let v = format!("{today} {} {} {}", t.pulls, t.minutes, t.points);
        if let Err(e) = self.state.sys.script_set("slots", &self.slots_key(), Some(&v)) {
            tracing::error!("slots: saving {}'s pulls: {e}", self.user.handle);
        }
    }

    fn pulls_left(&self, cfg: &Slots) -> Option<u32> {
        (cfg.pulls_per_day > 0).then(|| cfg.pulls_per_day.saturating_sub(self.slots_today().pulls))
    }

    /// A pull: counted, decided and paid. None if none are left today.
    fn slots_pull(&mut self, cfg: &Slots) -> Option<Pull> {
        if self.pulls_left(cfg) == Some(0) {
            return None;
        }
        let pull = decide(cfg, &mut rand::rng());
        let mut t = self.slots_today();
        t.pulls += 1;
        if let Some(w) = pull.win {
            let p = prize(cfg, w);
            t.minutes += p.minutes;
            t.points += p.points;
            self.user.time_today += p.minutes;
            self.user.file_points = self.user.file_points.saturating_add(p.points);
            if let Err(e) = self.save_own_user() {
                tracing::error!("slots: paying {}: {e}", self.user.handle);
            }
            tracing::info!("{}: slots: {} (+{} min, +{} points)", self.user.handle, tier_name(w), p.minutes, p.points);
            if w == Tier::Jackpot {
                let when = crate::clock::now().naive_local().format("%Y-%m-%d %H:%M");
                let _ = self.state.sys.script_set("slots", "jackpot", Some(&format!("{when} {}", self.user.handle)));
            }
        }
        self.slots_save(&t);
        Some(pull)
    }

    /// "+20 min  +25 points", leaving out points on a board without them.
    fn prize_text(&self, p: Prize) -> String {
        let points = self.state.config().files.use_file_points;
        match (p.minutes, p.points) {
            (m, n) if n > 0 && points && m > 0 => format!("+{m} min  +{n} points"),
            (_, n) if n > 0 && points => format!("+{n} points"),
            (m, _) => format!("+{m} min"),
        }
    }

    /// `AL`: the slot machine.
    pub async fn slots(&mut self) -> io::Result<()> {
        let cfg = self.state.config().slots.clone();
        let (begun, pulled) = (crate::clock::now().naive_local(), self.slots_today().pulls);
        let result = self.slots_play(&cfg).await;
        // A visit with a pull in it counts as a play, for the door stats.
        if self.slots_today().pulls != pulled {
            self.record_play("Slots", begun);
        }
        result
    }

    async fn slots_play(&mut self, cfg: &Slots) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.slots_plain(cfg).await;
        }
        let truecolor = self.screen.truecolor() || self.screen.charset() == fluxbbs_term::Charset::Utf8;
        let mut game = Game::new(truecolor);
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.screen.write_bytes(b"\x1b[?25l");
        self.cursor_asks = self.cursor_asks.min(self.cursor_reports);
        let ticker = self.slots_ticker(cfg);
        let result = self.slots_loop(cfg, &mut game, &ticker).await;
        self.screen.write_bytes(b"\x1b[0m\x1b[?25h");
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.full_screen_done();
        result
    }

    async fn slots_loop(&mut self, cfg: &Slots, game: &mut Game, ticker: &[(char, Rgb)]) -> io::Result<()> {
        let idle_limit = match self.state.config().bbs.inactivity_seconds {
            0 => 900,
            n => n.min(900),
        };
        let mut message: Option<(Vec<(char, Rgb)>, Instant)> = None;
        // What's on the message line now, so it's drawn only when it moves.
        let mut shown = usize::MAX;
        let tally = self.slots_today();
        self.slots_status(cfg, game.truecolor, &tally);
        loop {
            // Waiting for a pull: the lights run, a glint now and then, the
            // ticker under the reels.
            let since = Instant::now();
            let key = loop {
                let t = game.clock.elapsed().as_secs_f32();
                let n = game.scene.bulbs.len();
                let step = (t * 3.0) as usize;
                for (i, b) in game.scene.bulbs.iter_mut().enumerate() {
                    *b = (i + n * 4 - step % n) % 6 < 2;
                }
                let cycle = t % 7.0;
                game.scene.glint = if cycle < 0.9 { -10.0 + cycle / 0.9 * 110.0 } else { -1.0 };
                self.slots_draw(game, false).await?;
                match &message {
                    Some((m, at)) if at.elapsed() < Duration::from_secs(6) => {
                        if shown != usize::MAX - 1 {
                            self.slots_line(m, game.truecolor);
                            shown = usize::MAX - 1;
                        }
                    }
                    _ => {
                        let off = (t * 8.0) as usize;
                        if off != shown {
                            let line: Vec<(char, Rgb)> = (0..78).map(|i| ticker[(off + i) % ticker.len()]).collect();
                            self.slots_line(&line, game.truecolor);
                            shown = off;
                        }
                    }
                }
                self.flush().await?;
                if self.key_waiting().await? {
                    break self.read_key().await?;
                }
                tokio::time::sleep(Duration::from_millis(60)).await;
                if since.elapsed() > Duration::from_secs(idle_limit) {
                    return Ok(());
                }
            };
            match key {
                Key::Char(b' ') | Key::Enter => {
                    let before = self.slots_today();
                    let Some(pull) = self.slots_pull(cfg) else {
                        let s = self.strings().fmt("SlotsNone", &[&cfg.pulls_per_day.to_string()]);
                        message = Some((painted(&crate::editor::strip_colours(&s), [255, 120, 140]), Instant::now()));
                        shown = usize::MAX;
                        continue;
                    };
                    // The pull counts now; what it won shows once it's seen.
                    self.slots_status(cfg, game.truecolor, &Tally { pulls: before.pulls + 1, ..before });
                    self.slots_spin(game, &pull).await?;
                    message = Some((self.slots_show(cfg, game, &pull).await?, Instant::now()));
                    shown = usize::MAX;
                    let tally = self.slots_today();
                    self.slots_status(cfg, game.truecolor, &tally);
                }
                Key::Esc => return Ok(()),
                k => match k.upper() {
                    Some('Q') => return Ok(()),
                    Some('P') => {
                        self.slots_paytable(cfg, game.truecolor);
                        self.flush().await?;
                        self.read_key().await?;
                        game.enc.forget();
                        let tally = self.slots_today();
                        self.slots_status(cfg, game.truecolor, &tally);
                        shown = usize::MAX;
                    }
                    _ => {}
                },
            }
        }
    }

    /// Draw the scene; with `paced`, hold the next frame until the
    /// terminal has shown this one.
    async fn slots_draw(&mut self, game: &mut Game, paced: bool) -> io::Result<()> {
        game.canvas.draw(&game.scene);
        let out = game.enc.frame(&game.canvas, 1);
        if out.is_empty() {
            return Ok(());
        }
        self.screen.write_bytes(&out);
        if paced && game.sync {
            let answering = self.cursor_reports > 0;
            let wait = Duration::from_secs(if answering { 10 } else { 3 });
            if !self.sync_output(wait, 1).await? && !answering {
                game.sync = false;
            }
        }
        Ok(())
    }

    /// The reels spin and stop on the pull. A key stops them sooner.
    async fn slots_spin(&mut self, game: &mut Game, pull: &Pull) -> io::Result<()> {
        let begin = game.clock.elapsed().as_secs_f32();
        let mut motions: Vec<Motion> = (0..3).map(|i| Motion::new(begin + i as f32 * 0.05, game.scene.reels[i].pos)).collect();
        // The third holds out when the first two could still make three.
        let tease = STRIPS[0][pull.stops[0]] == STRIPS[1][pull.stops[1]] && STRIPS[0][pull.stops[0]] != CHERRY;
        let mut at = begin + 0.95;
        for (i, m) in motions.iter_mut().enumerate() {
            let (len, hit) = if i == 2 && tease { (1.5, SPEED * 0.05) } else { (0.4, SPEED * 0.1) };
            m.stop_at(at, len, hit, pull.stops[i]);
            at = m.stopped_by() + if i == 1 && tease { 0.5 } else { 0.25 } - 0.4;
        }
        game.scene.payline = 0.0;
        game.scene.glint = -1.0;
        let mut slammed = false;
        loop {
            let t = game.clock.elapsed().as_secs_f32();
            let mut done = true;
            for (i, m) in motions.iter().enumerate() {
                let (pos, speed, rest) = m.at(t);
                game.scene.reels[i] = ReelView { pos, speed, glow: 0.0, dim: 0.0 };
                done &= rest;
            }
            let n = game.scene.bulbs.len();
            let step = ((t - begin) * 16.0) as usize;
            for (i, b) in game.scene.bulbs.iter_mut().enumerate() {
                *b = ((i + step) % n).is_multiple_of(4);
            }
            self.slots_draw(game, true).await?;
            self.flush().await?;
            if done {
                break;
            }
            if self.key_waiting().await? {
                let k = self.read_key().await?;
                // Pressing again stops the reels now, one after another.
                if matches!(k, Key::Char(b' ') | Key::Enter) && !slammed {
                    slammed = true;
                    let mut at = t;
                    for (i, m) in motions.iter_mut().enumerate() {
                        if m.brake > t + 0.05 {
                            m.stop_at(at, 0.22, SPEED * 0.1, pull.stops[i]);
                        }
                        at = m.stopped_by().max(at) + 0.12 - 0.22;
                    }
                }
            }
        }
        for (i, r) in game.scene.reels.iter_mut().enumerate() {
            r.pos = pos_of(pull.stops[i]);
            r.speed = 0.0;
        }
        Ok(())
    }

    /// The result: a win lights up (coins for the big ones), a loss says
    /// so. Returns the line left under the reels.
    async fn slots_show(&mut self, cfg: &Slots, game: &mut Game, pull: &Pull) -> io::Result<Vec<(char, Rgb)>> {
        let Some(win) = pull.win else {
            let s = if pull.near { "SlotsNear" } else { "SlotsLose" };
            let text = crate::editor::strip_colours(self.strings().get(s));
            let line = painted(&text, if pull.near { [255, 170, 90] } else { [150, 130, 170] });
            self.slots_line(&line, game.truecolor);
            self.flush().await?;
            return Ok(line);
        };
        let p = prize(cfg, win);
        let name = tier_name(win).to_uppercase();
        let text = if win == Tier::Jackpot {
            format!("*** J A C K P O T ***   {name}   {}", self.prize_text(p))
        } else {
            format!("{name} pays {}", self.prize_text(p))
        };
        // Which reels the win is on.
        let lit: [bool; 3] = match win {
            Tier::OneCherry => [true, false, false],
            Tier::TwoCherries => [true, true, false],
            _ => [true; 3],
        };
        let (len, coins) = match win {
            Tier::Jackpot => (7.0, 90),
            Tier::Bars => (4.5, 30),
            Tier::Bells => (3.5, 14),
            Tier::Plums | Tier::Oranges | Tier::Lemons | Tier::Cherries => (2.8, 0),
            _ => (2.0, 0),
        };
        let begin = game.clock.elapsed().as_secs_f32();
        let mut spawned = 0;
        let mut last = begin;
        loop {
            let t = game.clock.elapsed().as_secs_f32();
            let tau = t - begin;
            if tau > len {
                break;
            }
            // In steps, so the reels are drawn again only as it changes.
            let pulse = ((0.5 + 0.5 * (tau * 9.0).sin()) * 4.0).round() / 4.0;
            for (i, r) in game.scene.reels.iter_mut().enumerate() {
                r.glow = if lit[i] { 0.35 + 0.5 * pulse } else { 0.0 };
                r.dim = if lit[i] { 0.0 } else { 0.5 };
            }
            game.scene.payline = if ((tau * 6.0) as usize).is_multiple_of(2) { 1.0 } else { 0.35 };
            let n = game.scene.bulbs.len();
            let step = (tau * 14.0) as usize;
            let flash = ((tau * 6.0) as usize).is_multiple_of(2);
            for (i, b) in game.scene.bulbs.iter_mut().enumerate() {
                *b = match win {
                    Tier::Jackpot | Tier::Bars => (i % 2 == 0) == flash,
                    _ => ((i + step) % n).is_multiple_of(3),
                };
            }
            // Coins: a burst, or for the jackpot a shower.
            let dt = t - last;
            last = t;
            if spawned < coins && tau < len - 1.5 {
                let want = if win == Tier::Jackpot { ((tau * 18.0) as usize).min(coins) } else { coins };
                let mut rng = rand::rng();
                while spawned < want {
                    game.scene.coins.push(Coin {
                        x: rng.random_range(4.0..W as f32 - 4.0),
                        y: -4.0 - rng.random_range(0.0..12.0),
                        vx: rng.random_range(-9.0..9.0),
                        vy: rng.random_range(5.0..30.0),
                        spin: rng.random_range(0.0..6.3),
                    });
                    spawned += 1;
                }
            }
            for c in &mut game.scene.coins {
                c.vy += 95.0 * dt;
                c.x += c.vx * dt;
                c.y += c.vy * dt;
                c.spin += 9.0 * dt;
            }
            game.scene.coins.retain(|c| c.y < crate::slotart::H as f32 + 4.0);
            // The line shimmers gold.
            let line: Vec<(char, Rgb)> = text
                .chars()
                .enumerate()
                .map(|(i, ch)| {
                    let wave = 0.5 + 0.5 * ((i as f32) * 0.45 - tau * 8.0).sin();
                    let c = if win == Tier::Jackpot {
                        let hue = (i as f32 * 0.25 + tau * 4.0) % 3.0;
                        match hue as usize {
                            0 => shade([255, 60, 80], [255, 220, 60], hue.fract()),
                            1 => shade([255, 220, 60], [80, 200, 255], hue.fract()),
                            _ => shade([80, 200, 255], [255, 60, 80], hue.fract()),
                        }
                    } else {
                        shade([235, 160, 30], [255, 250, 210], wave)
                    };
                    (ch, c)
                })
                .collect();
            self.slots_line(&line, game.truecolor);
            self.slots_draw(game, true).await?;
            self.flush().await?;
            if self.key_waiting().await? {
                self.read_key().await?;
                break;
            }
        }
        // Back to rest, the win still glowing faintly.
        game.scene.coins.clear();
        game.scene.payline = 0.6;
        for (i, r) in game.scene.reels.iter_mut().enumerate() {
            r.glow = if lit[i] { 0.3 } else { 0.0 };
            r.dim = 0.0;
        }
        self.slots_draw(game, false).await?;
        let line = painted(&text, [255, 215, 110]);
        self.slots_line(&line, game.truecolor);
        self.flush().await?;
        Ok(line)
    }

    fn slots_color(&self, out: &mut Vec<u8>, c: Rgb, truecolor: bool) {
        if truecolor {
            out.extend(format!("\x1b[38;2;{};{};{}m", c[0], c[1], c[2]).as_bytes());
        } else {
            out.extend(format!("\x1b[38;5;{}m", crate::picture::xterm256(c)).as_bytes());
        }
    }

    /// The message line, centered, each character its own color.
    fn slots_line(&mut self, text: &[(char, Rgb)], truecolor: bool) {
        let text = &text[..text.len().min(78)];
        let mut out = format!("\x1b[{MSG_ROW};1H\x1b[0m\x1b[K\x1b[{MSG_ROW};{}H", (80 - text.len()) / 2 + 1).into_bytes();
        let mut last = None;
        for &(ch, c) in text {
            if last != Some(c) && ch != ' ' {
                self.slots_color(&mut out, c, truecolor);
                last = Some(c);
            }
            out.extend(fluxbbs_term::cp437::encode(&ch.to_string()));
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
    }

    /// The bottom line: pulls left, today's winnings, the keys.
    fn slots_status(&mut self, cfg: &Slots, truecolor: bool, t: &Tally) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        match (cfg.pulls_per_day > 0).then(|| cfg.pulls_per_day.saturating_sub(t.pulls)) {
            Some(left) => {
                let shown = cfg.pulls_per_day.min(20);
                for i in 0..shown {
                    let on = i < left.min(shown);
                    self.slots_color(&mut out, if on { [255, 200, 80] } else { [70, 50, 70] }, truecolor);
                    out.push(0xFE);
                }
                self.slots_color(&mut out, [200, 170, 210], truecolor);
                out.extend(format!(" {left} left").as_bytes());
            }
            None => {
                self.slots_color(&mut out, [200, 170, 210], truecolor);
                out.extend(b"free play");
            }
        }
        if t.minutes > 0 || t.points > 0 {
            self.slots_color(&mut out, [140, 120, 150], truecolor);
            out.extend(b"   won ");
            self.slots_color(&mut out, [255, 215, 110], truecolor);
            out.extend(self.prize_text(Prize { minutes: t.minutes, points: t.points }).as_bytes());
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let keys = self.strings().get("SlotsKeys").to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(&keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    /// What each line pays, round and round under the reels.
    fn slots_ticker(&self, cfg: &Slots) -> Vec<(char, Rgb)> {
        let dim = [150, 120, 170];
        let mut t = Vec::new();
        for (tier, name, _) in TIERS {
            let ink = match tier {
                Tier::Jackpot => INK[SEVEN],
                Tier::Bars => INK[BAR],
                Tier::Bells => INK[BELL],
                Tier::Plums => INK[PLUM],
                Tier::Oranges => INK[ORANGE],
                Tier::Lemons => INK[LEMON],
                _ => INK[CHERRY],
            };
            t.extend(painted(name, ink));
            t.extend(painted("  ", dim));
            t.extend(painted(&self.prize_text(prize(cfg, tier)), [255, 215, 110]));
            t.extend(painted("   \u{25A0}   ", [110, 60, 120]));
        }
        if let Some(v) = self.state.sys.script_get("slots", "jackpot").ok().flatten() {
            let mut w = v.splitn(3, ' ');
            if let (Some(day), Some(_), Some(who)) = (w.next(), w.next(), w.next()) {
                t.extend(painted(&self.strings().fmt("SlotsLastJackpot", &[who, day]), [255, 120, 170]));
                t.extend(painted("   \u{25A0}   ", [110, 60, 120]));
            }
        }
        t
    }

    /// The pay table, in a box over the reels.
    fn slots_paytable(&mut self, cfg: &Slots, truecolor: bool) {
        let (x, y, w) = (12u16, 4u16, 56usize);
        let frame = [200, 150, 255];
        let mut out = Vec::new();
        let row = |out: &mut Vec<u8>, n: u16| out.extend(format!("\x1b[{};{x}H\x1b[0;40m", y + n).as_bytes());
        row(&mut out, 0);
        self.slots_color(&mut out, frame, truecolor);
        out.extend(fluxbbs_term::cp437::encode(&format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(w))));
        let mut lines: Vec<(Vec<(char, Rgb)>, usize)> = Vec::new();
        let head = self.strings().get("SlotsPaysHead").to_string();
        lines.push((painted(&crate::editor::strip_colours(&head), [255, 255, 255]), 0));
        lines.push((Vec::new(), 0));
        for (tier, name, _) in TIERS {
            let ink = match tier {
                Tier::Jackpot => INK[SEVEN],
                Tier::Bars => INK[BAR],
                Tier::Bells => INK[BELL],
                Tier::Plums => INK[PLUM],
                Tier::Oranges => INK[ORANGE],
                Tier::Lemons => INK[LEMON],
                _ => INK[CHERRY],
            };
            let what = match tier {
                Tier::TwoCherries => format!("{name} (reels 1, 2)"),
                Tier::OneCherry => format!("{name} (reel 1)"),
                _ => name.to_string(),
            };
            let mut l = painted(&format!("{what:<27}"), ink);
            l.extend(painted(&self.prize_text(prize(cfg, tier)), [255, 215, 110]));
            lines.push((l, 2));
        }
        lines.push((Vec::new(), 0));
        let odds = if cfg.jackpot_odds > 0 { format!(", 7 7 7 one in {}", cfg.jackpot_odds) } else { String::new() };
        let pulls = if cfg.pulls_per_day > 0 { format!("{} pulls a day", cfg.pulls_per_day) } else { "free play".into() };
        lines.push((painted(&format!("{pulls}; {}% of pulls win{odds}", cfg.win_percent), [170, 150, 190]), 0));
        lines.push((painted(&crate::editor::strip_colours(self.strings().get("SlotsPaysKey")), [120, 100, 140]), 0));
        for (i, (l, indent)) in lines.iter().enumerate() {
            row(&mut out, i as u16 + 1);
            self.slots_color(&mut out, frame, truecolor);
            out.push(0xBA);
            let pad = if *indent == 0 { (w.saturating_sub(l.len())) / 2 } else { *indent + 2 };
            out.extend(" ".repeat(pad).as_bytes());
            for &(ch, c) in l.iter().take(w - pad) {
                self.slots_color(&mut out, c, truecolor);
                out.extend(fluxbbs_term::cp437::encode(&ch.to_string()));
            }
            out.extend(" ".repeat(w.saturating_sub(pad + l.len())).as_bytes());
            self.slots_color(&mut out, frame, truecolor);
            out.push(0xBA);
        }
        row(&mut out, lines.len() as u16 + 1);
        self.slots_color(&mut out, frame, truecolor);
        out.extend(fluxbbs_term::cp437::encode(&format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(w))));
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
    }

    /// Plain text: a line a pull.
    async fn slots_plain(&mut self, cfg: &Slots) -> io::Result<()> {
        self.newline();
        loop {
            let left = self.pulls_left(cfg).map_or("unlimited".to_string(), |n| n.to_string());
            let s = self.strings().fmt("SlotsPlainPrompt", &[&left]);
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(10)).await?;
            self.newline();
            match s.trim().to_ascii_uppercase().as_str() {
                "" => {
                    let Some(pull) = self.slots_pull(cfg) else {
                        let s = self.strings().fmt("SlotsNone", &[&cfg.pulls_per_day.to_string()]);
                        self.show_line(&s).await?;
                        return Ok(());
                    };
                    let syms: Vec<&str> = (0..3).map(|i| NAMES[STRIPS[i][pull.stops[i]]]).collect();
                    let result = match pull.win {
                        Some(w) => format!("{}: {}", tier_name(w), self.prize_text(prize(cfg, w))),
                        None => crate::editor::strip_colours(self.strings().get(if pull.near { "SlotsNear" } else { "SlotsLose" })),
                    };
                    self.write(&format!("[ {} | {} | {} ]  {result}", syms[0], syms[1], syms[2]));
                    self.newline();
                }
                "P" => {
                    for (tier, name, _) in TIERS {
                        self.write(&format!("  {name:<16} {}", self.prize_text(prize(cfg, tier))));
                        self.newline();
                    }
                }
                "Q" => return Ok(()),
                _ => {}
            }
        }
    }
}

/// The machine on screen: what's drawn, and what the terminal has.
struct Game {
    scene: Scene,
    canvas: Canvas,
    enc: Encoder,
    truecolor: bool,
    /// Pacing by the terminal's answers, until it doesn't answer.
    sync: bool,
    clock: Instant,
}

impl Game {
    fn new(truecolor: bool) -> Game {
        let mut rng = rand::rng();
        let mut scene = Scene { bulbs: vec![false; bulb_spots().len()], glint: -1.0, ..Default::default() };
        for r in &mut scene.reels {
            r.pos = pos_of(rng.random_range(0..STOPS));
        }
        Game { scene, canvas: Canvas::new(), enc: Encoder::new(truecolor), truecolor, sync: true, clock: Instant::now() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slotart::symbol_at;

    #[test]
    fn lines_pay_as_the_table_says() {
        assert_eq!(pays([SEVEN; 3]), Some(Tier::Jackpot));
        assert_eq!(pays([CHERRY, CHERRY, CHERRY]), Some(Tier::Cherries));
        assert_eq!(pays([CHERRY, CHERRY, BAR]), Some(Tier::TwoCherries));
        assert_eq!(pays([CHERRY, BAR, BAR]), Some(Tier::OneCherry));
        assert_eq!(pays([BAR, CHERRY, CHERRY]), None);
        assert_eq!(pays([SEVEN, SEVEN, BAR]), None);
    }

    #[test]
    fn pulls_win_about_as_often_as_set() {
        let cfg = Slots { win_percent: 40, jackpot_odds: 100, ..Default::default() };
        let mut rng = rand::rng();
        let (mut wins, mut jackpots) = (0, 0);
        let n = 20_000;
        for _ in 0..n {
            let p = decide(&cfg, &mut rng);
            let shown = [0, 1, 2].map(|i| STRIPS[i][p.stops[i]]);
            assert_eq!(pays(shown), p.win, "the reels show what was decided");
            wins += p.win.is_some() as u32;
            jackpots += (p.win == Some(Tier::Jackpot)) as u32;
        }
        let pct = wins * 100 / n;
        assert!((38..=43).contains(&pct), "{pct}% won");
        assert!((120..=290).contains(&jackpots), "{jackpots} jackpots");
        let never = Slots { win_percent: 0, jackpot_odds: 0, ..Default::default() };
        assert!((0..2000).all(|_| decide(&never, &mut rng).win.is_none()));
    }

    #[test]
    fn reels_land_where_they_were_told() {
        for (k, &sym) in STRIPS[0].iter().enumerate() {
            let mut m = Motion::new(0.0, 123.0);
            m.stop_at(1.0, 0.4, SPEED * 0.1, k);
            let (pos, _, rest) = m.at(m.stopped_by() + SETTLE + 0.01);
            assert!(rest);
            assert_eq!(symbol_at(0, pos), sym);
            assert!((pos - m.target).abs() < 0.5, "settled: {pos} vs {}", m.target);
            // Smooth into the brake: no jump in position.
            let (a, _, _) = m.at(m.brake - 0.0001);
            let (b, _, _) = m.at(m.brake + 0.0001);
            assert!((a - b).abs() < 0.5);
        }
    }
}
