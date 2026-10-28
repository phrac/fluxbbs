//! New: the lottery on the pixel stage (`!P`'s `b`, `!P lottery`; the
//! `casino` component). A glass ball machine tumbles its balls, tonight's
//! pot shines in a marquee of chasing bulbs, and the caller's tickets sit
//! under it as stubs; buying prints new ones and rolls the pot up. Last
//! night's draw replays for whoever held tickets in it (the first time
//! they come by, or at logon with the `lottery` step): the balls churn,
//! one drops down the chute into the tray, its number lights, and the
//! winner's name comes up, coins raining if it's them. Plain text gets
//! lines (`points.rs`).

use std::io;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rand::RngExt;

use crate::casino::{MSG_ROW, STATUS_ROW};
use crate::input::Key;
use crate::pixels::{Canvas, H, Ink, Rgb, Stage, W, hash, ink, mix, painted, rgb, scale, smooth};
use crate::points::{Draw, Tonight};
use crate::session::Session;
use crate::slotart::Coin;

/// The globe: centre and radius, in pixels.
const GLOBE: (f32, f32, f32) = (18.0, 19.0, 13.0);
/// Where a drawn ball comes to rest, and its size there.
const TRAY: (f32, f32, f32) = (35.0, 39.0, 4.0);
const BALLS: usize = 16;
/// The marquee: its left edge and bottom, in pixels; its middle column.
const PANEL_X: i32 = 41;
const PANEL_BOTTOM: i32 = 29;
const PANEL_MID: usize = 60;
/// Stubs shown before "+N more", five to a row.
const STUBS: usize = 15;

/// A replay's beats, in seconds: churning, the ball out, its number lit,
/// the winner named.
const CHURN_END: f32 = 2.8;
const OUT_END: f32 = 4.3;
const NAMED: f32 = 5.3;

/// Ball colors, as on the lotto shows.
const COLORS: [(u8, u8, u8); 6] = [(225, 45, 50), (40, 95, 220), (245, 200, 40), (45, 170, 80), (240, 120, 30), (150, 60, 200)];

fn ball_color(n: u32) -> Rgb {
    let (r, g, b) = COLORS[n as usize % COLORS.len()];
    rgb(r, g, b)
}

/// What the picture shows.
struct Scene {
    title: String,
    pot: u32,
    /// The pot as it rolls up to `pot`.
    shown_pot: f32,
    sold: u32,
    /// The caller's ticket numbers, and when each stub came (for fading in).
    tickets: Vec<(u32, f32)>,
    /// The winning ticket, once lit.
    winner: Option<u32>,
    /// How hard the machine churns (0 settled, 1 tumbling, 3 a draw), and
    /// how far round its balls have gone.
    churn: f32,
    phase: f32,
    /// A ball on its way out: how far (0 in the globe, 1 in the tray), and
    /// its number.
    out: Option<(f32, u32)>,
    /// A win: the bulbs flash and coins fall.
    party: bool,
    coins: Vec<Coin>,
}

impl Scene {
    fn tonight(title: &str, t: &Tonight) -> Scene {
        Scene {
            title: title.into(),
            pot: t.pot,
            shown_pot: t.pot as f32,
            sold: t.sold,
            tickets: t.mine.iter().map(|&n| (n, -10.0)).collect(),
            winner: None,
            churn: 1.0,
            phase: 0.0,
            out: None,
            party: false,
            coins: Vec::new(),
        }
    }

    fn step(&mut self, dt: f32) {
        self.phase += dt * (0.4 + self.churn * 1.6);
        self.shown_pot += (self.pot as f32 - self.shown_pot) * (dt * 5.0).min(1.0);
        if (self.pot as f32 - self.shown_pot).abs() < 0.5 {
            self.shown_pot = self.pot as f32;
        }
        let mut rng = rand::rng();
        for c in &mut self.coins {
            c.vy += 70.0 * dt;
            c.x += c.vx * dt;
            c.y += c.vy * dt;
            c.spin += 7.0 * dt;
            if self.party && c.y > H as f32 + 4.0 {
                *c = Coin { x: rng.random_range(0.0..W as f32), y: -4.0, vx: rng.random_range(-6.0..6.0), vy: rng.random_range(5.0..25.0), spin: rng.random_range(0.0..6.0) };
            }
        }
        self.coins.retain(|c| c.y < H as f32 + 4.0);
    }

    fn shower(&mut self) {
        let mut rng = rand::rng();
        self.party = true;
        for _ in 0..36 {
            self.coins.push(Coin { x: rng.random_range(0.0..W as f32), y: rng.random_range(-40.0..-2.0), vx: rng.random_range(-6.0..6.0), vy: rng.random_range(0.0..20.0), spin: rng.random_range(0.0..6.0) });
        }
    }
}

/// The room: a night sky going to purple, a few stars.
fn backdrop() -> &'static Canvas {
    static ROOM: OnceLock<Canvas> = OnceLock::new();
    ROOM.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let mut c = mix(rgb(8, 8, 32), rgb(46, 10, 52), y as f32 / H as f32);
                let s = hash(x, y);
                if s > 0.985 && y < 30 {
                    c = mix(c, rgb(200, 200, 255), (s - 0.985) * 40.0);
                }
                cv.set(x, y, c);
            }
        }
        cv
    })
}

/// A shaded ball; with `number`, a lotto ball's white face on the front.
fn ball(cv: &mut Canvas, x: f32, y: f32, r: f32, base: Rgb, face: bool) {
    for py in (y - r).floor() as i32..=(y + r).ceil() as i32 {
        for px in (x - r).floor() as i32..=(x + r).ceil() as i32 {
            let (dx, dy) = ((px as f32 + 0.5 - x) / r, (py as f32 + 0.5 - y) / r);
            let d2 = dx * dx + dy * dy;
            if d2 > 1.0 {
                continue;
            }
            let light = (1.0 - ((dx + 0.45).powi(2) + (dy + 0.5).powi(2)).sqrt() * 0.7).clamp(0.2, 1.0);
            let mut c = if face && d2 < 0.4 { scale(rgb(245, 245, 238), 0.8 + 0.25 * light) } else { scale(base, 0.35 + 0.75 * light) };
            if (dx + 0.42).powi(2) + (dy + 0.48).powi(2) < 0.05 {
                c = mix(c, rgb(255, 255, 255), 0.8);
            }
            if d2 > 0.8 {
                c = scale(c, 0.75);
            }
            cv.set(px, py, c);
        }
    }
}

/// Where ball `i` sits settled: in rows up from the bottom of the globe,
/// as many to a row as fit.
fn rest_at(i: usize) -> (f32, f32) {
    let (cx, cy, r) = GLOBE;
    let mut left = i;
    for k in 0..8 {
        let y = cy + r - 3.4 - k as f32 * 3.8;
        let half = ((r - 2.6).powi(2) - (y - cy).powi(2)).max(0.0).sqrt();
        let fit = (2.0 * half / 4.4) as usize + 1;
        if left < fit {
            return (cx - (fit - 1) as f32 * 2.2 + left as f32 * 4.4, y);
        }
        left -= fit;
    }
    (cx, cy)
}

/// Where ball `i` is: settled, tumbling, or on its way between.
fn ball_at(i: usize, phase: f32, churn: f32) -> (f32, f32) {
    let (cx, cy, r) = GLOBE;
    let rest = rest_at(i);
    let f = i as f32;
    let a = phase * (0.7 + 0.13 * f) + f * 2.4;
    let rr = r * (0.3 + 0.55 * hash(i as i32, 3));
    let (mut sx, mut sy) = (rr * a.cos(), rr * (a * 1.3 + f).sin());
    let d = (sx * sx + sy * sy).sqrt();
    if d > r - 3.0 {
        (sx, sy) = (sx * (r - 3.0) / d, sy * (r - 3.0) / d);
    }
    let k = smooth(0.0, 1.0, churn);
    (rest.0 + (cx + sx - rest.0) * k, rest.1 + (cy + sy - rest.1) * k)
}

/// The drawn ball's path: down the neck, then along the chute.
fn out_at(p: f32) -> (f32, f32, f32) {
    let (cx, cy, r) = GLOBE;
    if p < 0.3 {
        let k = p / 0.3;
        (cx, cy + r - 4.0 + 9.0 * k, 2.2)
    } else {
        let k = smooth(0.0, 1.0, (p - 0.3) / 0.7);
        (cx + (TRAY.0 - cx) * k, cy + r + 5.0 + (TRAY.1 - cy - r - 5.0) * k, 2.2 + (TRAY.2 - 2.2) * k)
    }
}

fn draw(cv: &mut Canvas, sc: &Scene, t: f32) {
    cv.px.clone_from(&backdrop().px);
    cv.clear_labels();
    draw_machine(cv, sc);
    draw_panel(cv, sc, t);
    for c in &sc.coins {
        crate::slotart::coin(cv, c);
    }
}

fn draw_machine(cv: &mut Canvas, sc: &Scene) {
    let (cx, cy, r) = GLOBE;
    let gold = rgb(240, 190, 60);
    // The stand: a neck, then a red base trimmed in gold.
    cv.rect(cx as i32 - 3, (cy + r) as i32, 7, 3, rgb(170, 170, 185));
    for y in 35..H as i32 {
        let inset = (43 - y) / 2;
        for x in 6 + inset..31 - inset {
            let edge = y == 35 || x == 6 + inset || x == 30 - inset;
            cv.set(x, y, if edge { gold } else { mix(rgb(170, 25, 40), rgb(90, 10, 20), (y - 35) as f32 / 8.0) });
        }
    }
    cv.label(cx as usize - 2, 18, "LOTTO", rgb(255, 225, 120), None);
    // The chute down to the tray, and the tray.
    for i in 0..=40 {
        let (x, y, br) = out_at(0.3 + 0.62 * i as f32 / 40.0);
        cv.set(x.round() as i32, (y + br + 0.6).round().min(H as f32 - 1.0) as i32, rgb(200, 200, 215));
    }
    for x in (TRAY.0 - 5.0) as i32..=(TRAY.0 + 5.0) as i32 {
        let dx = (x as f32 - TRAY.0) / 5.0;
        cv.set(x, (TRAY.1 + TRAY.2 + 1.0 - (1.0 - dx * dx) * 1.5).round().min(H as f32 - 1.0) as i32, rgb(210, 210, 225));
    }
    // The globe: tinted glass, the balls, the rim and a shine.
    for y in (cy - r) as i32..=(cy + r) as i32 {
        for x in (cx - r) as i32..=(cx + r) as i32 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() / r;
            if d <= 1.0 {
                cv.blend(x, y, rgb(120, 160, 255), 0.08 + 0.12 * d * d);
            }
        }
    }
    for i in 0..BALLS {
        let (x, y) = ball_at(i, sc.phase, sc.churn);
        ball(cv, x, y, 2.2, ball_color(i as u32 * 7 + 1), false);
    }
    for y in (cy - r - 1.0) as i32..=(cy + r + 1.0) as i32 {
        for x in (cx - r - 1.0) as i32..=(cx + r + 1.0) as i32 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if (d - r).abs() < 0.7 {
                let lit = 0.5 + 0.5 * (-(dx + dy) / (d.max(0.1) * 1.414));
                cv.set(x, y, mix(rgb(110, 120, 150), rgb(235, 240, 255), lit));
            } else if d < r * 0.82 && d > r * 0.66 && dx < -2.0 && dy < -2.0 {
                cv.blend(x, y, rgb(255, 255, 255), 0.3);
            }
        }
    }
    cv.rect(cx as i32 - 3, (cy - r) as i32 - 3, 7, 3, rgb(200, 200, 215));
    cv.rect(cx as i32 - 2, (cy - r) as i32 - 3, 5, 1, gold);
    // The ball on its way out, and in the tray, its number.
    if let Some((p, n)) = sc.out {
        let (x, y, br) = out_at(p.min(1.0));
        ball(cv, x, y, br, ball_color(n), true);
        if p >= 1.0 {
            let s = n.to_string();
            cv.label((x.round() as usize).saturating_sub(s.len() / 2), (y / 2.0) as usize, &s, rgb(20, 20, 30), Some(rgb(245, 245, 238)));
        }
    }
}

fn draw_panel(cv: &mut Canvas, sc: &Scene, t: f32) {
    let (x0, x1) = (PANEL_X, W as i32 - 1);
    for y in 1..=PANEL_BOTTOM {
        for x in x0..=x1 {
            let frame = y <= 2 || y >= PANEL_BOTTOM - 1 || x <= x0 + 1 || x >= x1 - 1;
            let c = if frame { rgb(120, 70, 20) } else { mix(rgb(110, 15, 35), rgb(50, 5, 20), (y - 3) as f32 / 26.0) };
            cv.set(x, y, c);
        }
    }
    // Bulbs round the frame, chasing; flashing on a win.
    let mut spots = Vec::new();
    for x in (x0 + 1..=x1 - 1).step_by(3) {
        spots.push((x, 1));
    }
    for y in (4..PANEL_BOTTOM - 1).step_by(3) {
        spots.push((x1 - 1, y));
    }
    for x in (x0 + 1..=x1 - 1).rev().step_by(3) {
        spots.push((x, PANEL_BOTTOM - 1));
    }
    for y in (4..PANEL_BOTTOM - 1).rev().step_by(3) {
        spots.push((x0 + 1, y));
    }
    let beat = (t * if sc.party { 14.0 } else { 6.0 }) as usize;
    for (i, &(x, y)) in spots.iter().enumerate() {
        let on = if sc.party { (i + beat).is_multiple_of(2) } else { (i + beat).is_multiple_of(4) };
        cv.set(x, y, if on { rgb(255, 235, 150) } else { rgb(110, 70, 40) });
        if on {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                cv.blend(x + dx, y + dy, rgb(255, 210, 110), 0.45);
            }
        }
    }
    let center = |s: &str| PANEL_MID.saturating_sub(s.chars().count() / 2);
    let title = format!(" {} ", sc.title);
    cv.label(center(&title), 2, &title, rgb(255, 235, 170), Some(rgb(120, 70, 20)));
    // The pot, big and gold.
    let pot = (sc.shown_pot.round() as u32).to_string();
    let inner = x1 - x0 - 5;
    let size = if crate::pixels::text_width(&pot) * 2 <= inner { 2 } else { 1 };
    let w = crate::pixels::text_width(&pot) * size;
    let top = if size == 2 { 8 } else { 12 };
    cv.print((x0 + x1) / 2 - w / 2 + 1, top, &pot, size, |_, v| mix(rgb(255, 245, 190), rgb(230, 150, 20), v));
    cv.label(center("points"), 11, "points", rgb(255, 200, 150), None);
    let sold = format!("{} ticket{} in it", sc.sold, if sc.sold == 1 { "" } else { "s" });
    cv.label(center(&sold), 12, &sold, rgb(220, 160, 170), None);
    // The caller's tickets.
    if sc.tickets.is_empty() {
        cv.label(center("no tickets"), 16, "no tickets", rgb(170, 140, 190), None);
        return;
    }
    cv.label(center("your tickets"), 15, "your tickets", rgb(200, 170, 220), None);
    for (i, &(n, at)) in sc.tickets.iter().take(STUBS).enumerate() {
        let s = format!(" #{n} ");
        let (col, row) = (42 + (i % 5) * 7, 16 + (i / 5) * 2);
        let fresh = ((t - at) / 0.5).clamp(0.0, 1.0);
        let won = sc.winner == Some(n);
        let bg = if won {
            mix(rgb(255, 210, 60), rgb(255, 250, 210), 0.5 + 0.5 * (t * 8.0).sin())
        } else {
            mix(rgb(110, 15, 35), rgb(245, 235, 205), fresh)
        };
        cv.label(col, row, &s, if won { rgb(120, 20, 10) } else { rgb(130, 25, 30) }, Some(bg));
    }
    if sc.tickets.len() > STUBS {
        let more = format!("+{} more", sc.tickets.len() - STUBS);
        cv.label(center(&more), 21, &more, rgb(200, 170, 220), None);
    }
}

impl Session {
    fn lottery_say(&mut self, st: &Stage, text: &str, color: Ink) {
        let line = painted(&crate::editor::strip_colours(text), color);
        self.stage_line(MSG_ROW, &line, st.truecolor);
    }

    /// The bottom line: points and tickets on the left, the keys on the
    /// right (`LotteryKeys`, or `LotteryReplayKeys` during a replay).
    fn lottery_status(&mut self, st: &Stage, left: &[(&str, String)], keys: &str) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        for (label, value) in left {
            ink(&mut out, [200, 150, 210], st.truecolor);
            out.extend(format!("{label} ").as_bytes());
            ink(&mut out, [255, 230, 140], st.truecolor);
            out.extend(format!("{value}  ").as_bytes());
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let keys = self.strings().get(keys).to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(&keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    fn buy_status(&mut self, st: &Stage, t: &Tonight, qty: u32) {
        let left = [("points", self.user.file_points.to_string()), ("tickets", t.mine.len().to_string()), ("buy", qty.to_string())];
        self.lottery_status(st, &left, "LotteryKeys");
    }

    /// The lottery: tonight's pot and tickets, buying more; last night's
    /// draw first if we held tickets in it and haven't seen it.
    pub(crate) async fn lottery_stage(&mut self) -> io::Result<()> {
        let mut st = self.stage_open();
        if let Some((d, mine)) = self.unseen_draw() {
            self.saw_draw(&d);
            self.replay_on(&mut st, &d, &mine).await?;
        }
        let title = self.strings().get("LotteryTonight").to_string();
        let mut t = self.tonight();
        let mut sc = Scene::tonight(&title, &t);
        let mut qty = 1;
        let hello = self.strings().fmt("LotteryHello", &[&t.price.to_string()]);
        self.lottery_say(&st, &hello, [255, 200, 120]);
        self.buy_status(&st, &t, qty);
        let since = Instant::now();
        let mut last = st.t();
        loop {
            let now = st.t();
            sc.step(now - last);
            last = now;
            draw(&mut st.cv, &sc, now);
            self.stage_show(&mut st, true).await?;
            self.flush().await?;
            if !self.key_waiting().await? {
                tokio::time::sleep(self.idle_pause(50)).await;
                if since.elapsed() > Duration::from_secs(900) {
                    break;
                }
                continue;
            }
            let most = t.room.max(1);
            match self.read_key().await? {
                Key::Char(b'+') | Key::Right | Key::Up => qty = (qty + 1).min(most),
                Key::Char(b'-') | Key::Left | Key::Down => qty = qty.saturating_sub(1).max(1),
                Key::Char(c @ b'1'..=b'9') => qty = ((c - b'0') as u32).min(most),
                Key::Enter | Key::Char(b' ') => {
                    match self.buy_tickets_now(qty)? {
                        Ok(()) => {
                            let had = t.mine.len();
                            t = self.tonight();
                            sc.pot = t.pot;
                            sc.sold = t.sold;
                            sc.churn = 2.0;
                            for &n in &t.mine[had..] {
                                sc.tickets.push((n, now));
                            }
                            let nums: Vec<String> = t.mine[had..].iter().map(|n| format!("#{n}")).collect();
                            let s = self.strings().fmt("LotteryPrinted", &[&nums.join(" ")]);
                            self.lottery_say(&st, &s, [255, 230, 160]);
                        }
                        Err(why) => self.lottery_say(&st, &why, [230, 150, 170]),
                    }
                    qty = qty.min(t.room.max(1));
                }
                Key::Esc => break,
                k => match k.upper() {
                    Some('Q') => break,
                    Some('R') => match crate::points::last_draw(&self.state) {
                        Some(d) => {
                            let mine = crate::points::ticket_numbers(&self.state, &d.day, self.user.id);
                            self.replay_on(&mut st, &d, &mine).await?;
                            t = self.tonight();
                            sc = Scene::tonight(&title, &t);
                            self.lottery_say(&st, &hello, [255, 200, 120]);
                        }
                        None => {
                            let s = self.strings().get("LotteryNoDraw").to_string();
                            self.lottery_say(&st, &s, [200, 170, 210]);
                        }
                    },
                    _ => {}
                },
            }
            if sc.churn > 1.0 {
                sc.churn = 1.0;
            }
            self.buy_status(&st, &t, qty);
        }
        self.stage_close();
        Ok(())
    }

    /// A draw replayed on its own stage (the logon step).
    pub(crate) async fn lottery_replay(&mut self, d: &Draw, mine: &[u32]) -> io::Result<()> {
        let mut st = self.stage_open();
        self.replay_on(&mut st, d, mine).await?;
        self.stage_close();
        Ok(())
    }

    /// Replay `d` on an open stage: churn, a ball out, its number, the
    /// winner. A key skips to the end; another leaves.
    async fn replay_on(&mut self, st: &mut Stage, d: &Draw, mine: &[u32]) -> io::Result<()> {
        let s = self.strings();
        let title = s.fmt("LotteryDrawOf", &[&d.date()]);
        let tonight = Tonight { pot: d.pot, sold: d.tickets, mine: mine.to_vec(), room: 0, price: 0 };
        let mut sc = Scene::tonight(&title, &tonight);
        let won = mine.contains(&d.number);
        self.lottery_say(st, &s.fmt("LotteryDrawing", &[&d.date()]), [200, 190, 255]);
        let left = [("tickets", d.tickets.to_string()), ("yours", mine.len().to_string())];
        self.lottery_status(st, &left, "LotteryReplayKeys");
        let start = st.t();
        let mut skip = 0.0;
        let mut last = start;
        let mut beat = 0;
        loop {
            let now = st.t();
            let e = now - start + skip;
            sc.step(now - last);
            last = now;
            sc.churn = if e < CHURN_END { 1.0 + 2.0 * smooth(0.0, 1.2, e) } else { 3.0 - 2.0 * smooth(CHURN_END, OUT_END, e) };
            if e >= CHURN_END {
                sc.out = Some((((e - CHURN_END) / (OUT_END - CHURN_END)).min(1.0), d.number));
            }
            if beat == 0 && e >= OUT_END {
                beat = 1;
                sc.winner = Some(d.number);
                self.lottery_say(st, &s.fmt("LotteryTicketDrawn", &[&d.number.to_string()]), [255, 235, 150]);
            }
            if beat == 1 && e >= NAMED {
                beat = 2;
                let said = if won { s.fmt("LotteryYouWin", &[&d.pot.to_string()]) } else { s.fmt("LotteryWinner", &[&d.winner, &d.pot.to_string()]) };
                self.lottery_say(st, &said, if won { [255, 215, 90] } else { [255, 200, 220] });
                if won {
                    sc.shower();
                }
            }
            draw(&mut st.cv, &sc, now);
            self.stage_show(st, true).await?;
            self.flush().await?;
            if !self.key_waiting().await? {
                tokio::time::sleep(Duration::from_millis(if beat < 2 { 40 } else { self.idle_pause(60).as_millis() as u64 })).await;
                if e > NAMED + 60.0 {
                    break;
                }
                continue;
            }
            self.read_key().await?;
            if beat == 2 {
                break;
            }
            // Straight to the result.
            skip += NAMED - e;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balls_stay_in_the_globe() {
        let (cx, cy, r) = GLOBE;
        for i in 0..BALLS {
            for k in 0..40 {
                for churn in [0.0, 1.0, 3.0] {
                    let (x, y) = ball_at(i, k as f32 * 0.37, churn);
                    assert!(((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= r - 2.0, "ball {i} at {x},{y}");
                }
            }
        }
        // The drawn ball ends in the tray.
        let (x, y, br) = out_at(1.0);
        assert!((x - TRAY.0).abs() < 0.01 && (y - TRAY.1).abs() < 0.01 && (br - TRAY.2).abs() < 0.01);
    }
}
