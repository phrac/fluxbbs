//! New: Carrier Lost (`AP`), rebuilt as an arcade war-dialing run. One
//! long 1994 night at a modem: tune a dial until the needle sits on a
//! live carrier and lock on, then catch a three-signal handshake in its
//! sync band for the haul. Every dial raises a trace meter; cash out to
//! fence your bytes for file points and a place on the board, or let the
//! trace reach 100 and lose the lot till tomorrow.
//!
//! It plays on the casino's pixel stage (`pixels`, `casino::Stage`): a
//! CRT with a tuning track and bobbing signal columns, drawn in
//! `carrierart.rs`. No typed commands and no AI: it's a reflex game, all
//! period make-believe.

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::RngExt;
use rusqlite::{Connection, OptionalExtension, params};

use crate::carrierart::{self, Kind};
use crate::casino::{Ink, Stage, ink, painted};
use crate::config::CarrierLost as Cfg;
use crate::config::Prize;
use crate::input::Key;
use crate::pixels::rgb;
use crate::session::Session;

const MSG_ROW: u16 = 23;
const STATUS_ROW: u16 = 24;

const PROSE: Ink = [150, 230, 170];
const WARN: Ink = [255, 128, 104];
const BANK: Ink = [255, 214, 110];
const DIM: Ink = [140, 150, 160];

/// The gear a caller can rack, quietest last: each level slows the trace
/// and widens the handshake's sync band a little.
const GEAR: [&str; 5] = ["a bare line", "a borrowed calling card", "a line diverter", "a blue box", "a looped trunk"];

// ---- the store -----------------------------------------------------------------

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS clw_player (
    user    INTEGER PRIMARY KEY,
    handle  TEXT NOT NULL,
    gear    INTEGER NOT NULL DEFAULT 0,
    best    INTEGER NOT NULL DEFAULT 0,
    runs    INTEGER NOT NULL DEFAULT 0,
    day     TEXT NOT NULL DEFAULT '',
    dials   INTEGER NOT NULL DEFAULT 0,
    busted  TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS clw_score (
    id     INTEGER PRIMARY KEY,
    handle TEXT NOT NULL,
    haul   INTEGER NOT NULL,
    at     TEXT NOT NULL
);
";

#[derive(Clone, Default)]
pub struct Player {
    pub gear: u32,
    pub best: u32,
    pub runs: u32,
    pub day: String,
    pub dials: u32,
    pub busted: String,
}

pub struct CarrierStore {
    db: Mutex<Connection>,
}

impl CarrierStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = crate::sysdata::open_db(path)?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn player(&self, user: u32) -> anyhow::Result<Player> {
        let db = self.db.lock().unwrap();
        let p = db
            .query_row("SELECT gear, best, runs, day, dials, busted FROM clw_player WHERE user = ?1", params![user], |r| {
                Ok(Player { gear: r.get(0)?, best: r.get(1)?, runs: r.get(2)?, day: r.get(3)?, dials: r.get(4)?, busted: r.get(5)? })
            })
            .optional()?;
        Ok(p.unwrap_or_default())
    }

    pub fn save_player(&self, user: u32, handle: &str, p: &Player) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO clw_player (user, handle, gear, best, runs, day, dials, busted) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(user) DO UPDATE SET handle=?2, gear=?3, best=?4, runs=?5, day=?6, dials=?7, busted=?8",
            params![user, handle, p.gear, p.best, p.runs, p.day, p.dials, p.busted],
        )?;
        Ok(())
    }

    /// Record a fenced haul on the board.
    pub fn add_score(&self, handle: &str, haul: u32, at: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT INTO clw_score (handle, haul, at) VALUES (?1,?2,?3)", params![handle, haul, at])?;
        Ok(())
    }

    /// The biggest hauls fenced, most first.
    pub fn top(&self, n: u32) -> anyhow::Result<Vec<(String, u32, String)>> {
        let db = self.db.lock().unwrap();
        let mut st = db.prepare("SELECT handle, haul, at FROM clw_score ORDER BY haul DESC, id ASC LIMIT ?1")?;
        let rows = st.query_map(params![n], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }
}

// ---- the run -------------------------------------------------------------------

/// A carrier on the dial.
#[derive(Clone)]
struct Notch {
    pos: f32,
    live: bool,
    taken: bool,
    kind: Kind,
    tier: u32,
}

/// The phase of play.
enum Phase {
    Dial,
    Handshake,
}

/// One night's run: the trace, the haul so far, and whichever phase is up.
struct Run {
    trace: u32,
    haul: u32,
    found: u32,
    notches: Vec<Notch>,
    exchange: u32,
    phase: Phase,
    // The handshake in progress.
    bars: [f32; 3],
    speed: [f32; 3],
    phase_off: [f32; 3],
    locked: [Option<bool>; 3],
    cur: usize,
    hs_kind: Kind,
    hs_tier: u32,
}

impl Run {
    fn new() -> Run {
        let mut r = Run {
            trace: 0,
            haul: 0,
            found: 0,
            notches: Vec::new(),
            exchange: 0,
            phase: Phase::Dial,
            bars: [0.5; 3],
            speed: [0.0; 3],
            phase_off: [0.0; 3],
            locked: [None; 3],
            cur: 0,
            hs_kind: Kind::Unix,
            hs_tier: 1,
        };
        r.new_exchange();
        r
    }

    /// A fresh row of carriers to tune across.
    fn new_exchange(&mut self) {
        let mut rng = rand::rng();
        self.exchange += 1;
        let live = rng.random_range(2..=4);
        let dead = rng.random_range(2..=4);
        let mut n = Vec::new();
        for i in 0..live {
            n.push(Notch {
                pos: 0.0,
                live: true,
                taken: false,
                kind: Kind::ALL[rng.random_range(0..Kind::ALL.len())],
                tier: 1 + ((i + self.found) % 4),
            });
        }
        for _ in 0..dead {
            n.push(Notch { pos: 0.0, live: false, taken: false, kind: Kind::Unix, tier: 0 });
        }
        // Spread them across the track, a little jittered.
        let count = n.len();
        for (i, notch) in n.iter_mut().enumerate() {
            let base = (i as f32 + 0.5) / count as f32;
            notch.pos = (base + rng.random_range(-0.03..0.03)).clamp(0.02, 0.98);
        }
        n.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        self.notches = n;
    }

    /// How fast the needle sweeps: a touch quicker as the night wears on.
    fn needle_speed(&self) -> f32 {
        0.45 + 0.03 * self.found as f32
    }

    /// The needle's place, 0..1, a triangle wave of time.
    fn needle(&self, t: f32) -> f32 {
        let x = (t * self.needle_speed()) % 1.0;
        if x < 0.5 { x * 2.0 } else { 2.0 - x * 2.0 }
    }

    /// Any live carriers left on this exchange?
    fn any_live(&self) -> bool {
        self.notches.iter().any(|n| n.live && !n.taken)
    }

    /// The half-height of the sync band, wider with better gear.
    fn zone(&self, gear: u32) -> f32 {
        0.14 + 0.03 * gear as f32
    }

    fn dial_view(&self) -> Vec<(f32, u8)> {
        self.notches
            .iter()
            .map(|n| (n.pos, if n.taken { 2 } else if n.live { 1 } else { 0 }))
            .collect()
    }
}

impl Session {
    /// `AP`: Carrier Lost.
    pub async fn carrier_lost(&mut self, _param: &str) -> io::Result<()> {
        let cfg = self.state.config().carrier.clone();
        let begun = crate::clock::now().naive_local();
        let today = crate::clock::now().date_naive().to_string();
        let mut p = self.state.carrier.player(self.user.id).unwrap_or_default();
        if p.day != today {
            p.day = today.clone();
            p.dials = 0;
        }
        self.node.update(|n| n.activity = "on the wires (Carrier Lost)".into());

        if self.plain() {
            let r = self.cl_plain(&cfg, &mut p, &today).await;
            self.cl_save(&p);
            self.record_play("Carrier Lost", begun);
            return r;
        }

        let mut st = self.stage_open();
        let r = self.cl_play(&cfg, &mut p, &mut st, &today).await;
        self.cl_save(&p);
        self.record_play("Carrier Lost", begun);
        self.stage_close();
        r
    }

    fn cl_save(&self, p: &Player) {
        if let Err(e) = self.state.carrier.save_player(self.user.id, &self.user.handle, p) {
            tracing::error!("carrier: saving {}: {e}", self.user.handle);
        }
    }

    async fn cl_play(&mut self, cfg: &Cfg, p: &mut Player, st: &mut Stage, today: &str) -> io::Result<()> {
        if p.busted == today {
            carrierart::draw_busted(&mut st.cv, 0.0);
            self.stage_show(st, false).await?;
            self.cl_say(st, "ClBustedToday", &[], WARN);
            self.cl_status(st, p, 0, 0);
            self.cl_anykey(st).await?;
            return Ok(());
        }
        let splash = Instant::now();
        while splash.elapsed() < Duration::from_millis(1400) {
            let t = st.t();
            carrierart::draw_desk(&mut st.cv, t, None, 0);
            carrierart::vignette(&mut st.cv);
            self.stage_show(st, false).await?;
            tokio::time::sleep(Duration::from_millis(90)).await;
            if self.key_waiting().await? {
                self.read_key().await?;
                break;
            }
        }
        st.enc.forget();
        self.cl_say(st, "ClHello", &[], PROSE);
        let mut run = Run::new();
        loop {
            // Draw and wait for a key, animating meanwhile.
            let key = self.cl_wait(st, p, &run).await?;
            let Some(key) = key else { return Ok(()) };
            if self.escaped {
                return self.cl_cash_out(st, p, &run).await;
            }
            if matches!(key, Key::Esc) {
                return self.cl_cash_out(st, p, &run).await;
            }
            match key.upper() {
                Some('C') | Some('Q') => return self.cl_cash_out(st, p, &run).await,
                Some('?') | Some('H') => self.cl_help(st, p, &run).await?,
                Some('T') => self.cl_top(st, p, &run).await?,
                Some('S') if matches!(run.phase, Phase::Dial) => self.cl_shop(st, p, &mut run).await?,
                _ => {}
            }
            // SPACE (or Enter) acts on the current phase.
            let act = matches!(key, Key::Enter) || key.upper() == Some(' ') || matches!(key, Key::Char(b' '));
            if !act {
                continue;
            }
            match run.phase {
                Phase::Dial => {
                    if self.cl_dial(cfg, p, st, &mut run).await? {
                        return self.cl_busted(st, p, &mut run, today).await;
                    }
                }
                Phase::Handshake => {
                    if self.cl_lock(st, p, &mut run).await? {
                        return self.cl_busted(st, p, &mut run, today).await;
                    }
                }
            }
        }
    }

    /// Draw the current phase and wait for a key, animating until one
    /// comes or the caller's long gone. `None` means they vanished.
    async fn cl_wait(&mut self, st: &mut Stage, p: &Player, run: &Run) -> io::Result<Option<Key>> {
        let since = Instant::now();
        loop {
            self.cl_frame(st, p, run).await?;
            if self.key_waiting().await? {
                return Ok(Some(self.read_key().await?));
            }
            if since.elapsed() > Duration::from_secs(900) {
                return Ok(None);
            }
            tokio::time::sleep(self.idle_pause(90)).await;
        }
    }

    async fn cl_frame(&mut self, st: &mut Stage, p: &Player, run: &Run) -> io::Result<()> {
        let t = st.t();
        match run.phase {
            Phase::Dial => {
                let tint = rgb(90, 255, 120);
                carrierart::draw_dial(&mut st.cv, t, tint, &run.dial_view(), run.needle(t), run.found);
            }
            Phase::Handshake => {
                let k = kind_rgb(run.hs_kind);
                let tint = rgb(k[0], k[1], k[2]);
                let mut bars = run.bars;
                for (i, b) in bars.iter_mut().enumerate() {
                    if run.locked[i].is_none() {
                        *b = 0.5 + 0.5 * (t * run.speed[i] + run.phase_off[i]).sin();
                    }
                }
                carrierart::draw_handshake(&mut st.cv, t, tint, &bars, &run.locked, run.cur, run.zone(p.gear));
            }
        }
        carrierart::vignette(&mut st.cv);
        self.stage_show(st, false).await?;
        self.cl_status(st, p, run.haul, run.trace);
        Ok(())
    }

    /// Dial where the needle sits. Returns true if the caller got busted.
    async fn cl_dial(&mut self, cfg: &Cfg, p: &mut Player, st: &mut Stage, run: &mut Run) -> io::Result<bool> {
        if cfg.dials_per_day > 0 && p.dials >= cfg.dials_per_day {
            self.cl_say(st, "ClNoDials", &[], WARN);
            return Ok(false);
        }
        p.dials += 1;
        let t = st.t();
        let needle = run.needle(t);
        // The nearest carrier the needle's close to.
        let hit = run
            .notches
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.taken)
            .min_by(|(_, a), (_, b)| (a.pos - needle).abs().total_cmp(&(b.pos - needle).abs()))
            .filter(|(_, n)| (n.pos - needle).abs() < 0.05);
        match hit {
            Some((i, n)) if n.live => {
                let (kind, tier) = (n.kind, n.tier);
                run.notches[i].taken = true;
                run.found += 1;
                self.cl_connect(st, run, kind, tier).await?;
                Ok(false)
            }
            Some((i, _)) => {
                run.notches[i].taken = true;
                run.trace = (run.trace + 2).min(100);
                self.cl_say(st, "ClNoCarrier", &[], DIM);
                if !run.any_live() {
                    run.new_exchange();
                }
                Ok(run.trace >= 100)
            }
            None => {
                run.trace = (run.trace + 1).min(100);
                self.cl_say(st, "ClNoCarrier", &[], DIM);
                Ok(run.trace >= 100)
            }
        }
    }

    /// A carrier answered: flash its title card, then set the handshake up.
    async fn cl_connect(&mut self, st: &mut Stage, run: &mut Run, kind: Kind, tier: u32) -> io::Result<()> {
        let t = st.t();
        carrierart::draw_system(&mut st.cv, t, kind, kind_name(kind), run.trace, false);
        carrierart::vignette(&mut st.cv);
        self.stage_show(st, false).await?;
        self.cl_say(st, "ClConnect", &[kind_name(kind)], PROSE);
        self.cl_status(st, p_none(), run.haul, run.trace);
        tokio::time::sleep(Duration::from_millis(700)).await;
        let mut rng = rand::rng();
        run.phase = Phase::Handshake;
        run.hs_kind = kind;
        run.hs_tier = tier;
        run.cur = 0;
        run.locked = [None; 3];
        for (sp, off) in run.speed.iter_mut().zip(run.phase_off.iter_mut()) {
            *sp = rng.random_range(2.2..3.6) + tier as f32 * 0.2;
            *off = rng.random_range(0.0..std::f32::consts::TAU);
        }
        st.enc.forget();
        Ok(())
    }

    /// Lock the current signal. Returns true if the resolved handshake
    /// busted the caller.
    async fn cl_lock(&mut self, st: &mut Stage, p: &mut Player, run: &mut Run) -> io::Result<bool> {
        let t = st.t();
        let v = 0.5 + 0.5 * (t * run.speed[run.cur] + run.phase_off[run.cur]).sin();
        let hit = (v - 0.5).abs() <= run.zone(p.gear);
        run.bars[run.cur] = v;
        run.locked[run.cur] = Some(hit);
        run.cur += 1;
        if run.cur < 3 {
            self.cl_say(st, "ClHandshake", &[], PROSE);
            return Ok(false);
        }
        // Resolve: hits make the haul, misses make the heat.
        let hits = run.locked.iter().filter(|l| **l == Some(true)).count() as u32;
        let misses = 3 - hits;
        let bytes = run.hs_tier * (6 + hits * 14);
        run.haul += bytes;
        let gear_cut = (p.gear * 2).min(8);
        let rise = (6 + misses * 6).saturating_sub(gear_cut).max(2);
        run.trace = (run.trace + rise).min(100);
        // Show the result on the card a moment.
        self.cl_frame(st, p, run).await?;
        self.cl_say(st, "ClHaul", &[&hits.to_string(), &bytes.to_string(), kind_name(run.hs_kind)], BANK);
        tokio::time::sleep(Duration::from_millis(900)).await;
        run.phase = Phase::Dial;
        if !run.any_live() {
            run.new_exchange();
        }
        st.enc.forget();
        if run.trace >= 100 {
            return Ok(true);
        }
        if run.trace >= 70 {
            self.cl_say(st, "ClTraceWarn", &[], WARN);
        }
        Ok(false)
    }

    async fn cl_busted(&mut self, st: &mut Stage, p: &mut Player, run: &mut Run, today: &str) -> io::Result<()> {
        p.busted = today.to_string();
        p.runs += 1;
        run.haul = 0;
        let begun = Instant::now();
        while begun.elapsed() < Duration::from_millis(2600) {
            let t = st.t();
            carrierart::draw_busted(&mut st.cv, t);
            self.stage_show(st, false).await?;
            self.cl_status(st, p, 0, 100);
            tokio::time::sleep(Duration::from_millis(80)).await;
            if self.key_waiting().await? {
                self.read_key().await?;
                break;
            }
        }
        self.cl_say(st, "ClBusted", &[], WARN);
        self.cl_anykey(st).await
    }

    async fn cl_cash_out(&mut self, st: &mut Stage, p: &mut Player, run: &Run) -> io::Result<()> {
        self.escaped = false;
        p.runs += 1;
        if run.haul == 0 {
            self.cl_say(st, "ClBankedNothing", &[], DIM);
            self.cl_anykey(st).await?;
            return Ok(());
        }
        if run.haul > p.best {
            p.best = run.haul;
        }
        let at = crate::clock::now().naive_local().format("%Y-%m-%d %H:%M").to_string();
        if let Err(e) = self.state.carrier.add_score(&self.user.handle, run.haul, &at) {
            tracing::error!("carrier: scoring: {e}");
        }
        let prize = self.cl_pay(p, run.haul);
        let pt = self.casino_prize_text(prize);
        self.cl_say(st, "ClBanked", &[&run.haul.to_string(), &pt], BANK);
        self.cl_status(st, p, run.haul, run.trace);
        self.cl_anykey(st).await
    }

    /// Fencing the haul: file points and a little time, by the config's
    /// bytes-per-point.
    fn cl_pay(&mut self, _p: &Player, haul: u32) -> Prize {
        let per = self.state.config().carrier.bytes_per_point.max(1);
        let points = haul / per;
        let minutes = points;
        if points > 0 {
            self.user.time_today += minutes;
            self.user.file_points = self.user.file_points.saturating_add(points);
            if let Err(e) = self.save_own_user() {
                tracing::error!("carrier: paying {}: {e}", self.user.handle);
            }
        }
        Prize { minutes, points }
    }

    async fn cl_shop(&mut self, st: &mut Stage, p: &mut Player, run: &mut Run) -> io::Result<()> {
        if (p.gear as usize) + 1 >= GEAR.len() {
            self.cl_say(st, "ClGearMax", &[], DIM);
            self.cl_status(st, p, run.haul, run.trace);
            return Ok(());
        }
        let cost = self.state.config().carrier.gear_cost.max(1) * (p.gear + 1);
        self.cl_say(st, "ClGearNext", &[GEAR[p.gear as usize + 1], &cost.to_string()], PROSE);
        self.cl_status(st, p, run.haul, run.trace);
        self.cl_shop_wait(st, p, run, cost).await
    }

    /// At the shop: `b` buys the next gear if the haul covers it, any
    /// other key leaves.
    async fn cl_shop_wait(&mut self, st: &mut Stage, p: &mut Player, run: &mut Run, cost: u32) -> io::Result<()> {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K").into_bytes();
        let keys = self.strings().get("ClKeysShop").to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        out.extend(format!("\x1b[{STATUS_ROW};{}H", 80u16.saturating_sub(len as u16).max(1)).as_bytes());
        self.screen.write_bytes(&out);
        self.write_with_controls(&keys);
        let key = self.read_key().await?;
        if key.upper() == Some('B') && (p.gear as usize) + 1 < GEAR.len() && run.haul >= cost {
            run.haul -= cost;
            p.gear += 1;
            self.cl_say(st, "ClBought", &[GEAR[p.gear as usize]], BANK);
        } else if key.upper() == Some('B') {
            self.cl_say(st, "ClCantAfford", &[], WARN);
        }
        st.enc.forget();
        Ok(())
    }

    async fn cl_top(&mut self, st: &mut Stage, p: &Player, run: &Run) -> io::Result<()> {
        let rows = self.state.carrier.top(8).unwrap_or_default();
        st.enc.forget();
        self.screen.write_bytes(b"\x1b[2J");
        let mut y = 2u16;
        self.cl_center(st, y, "ClTopHead", &[], BANK);
        y += 2;
        if rows.is_empty() {
            self.cl_center(st, y, "ClTopNone", &[], DIM);
        }
        for (i, (handle, haul, at)) in rows.iter().enumerate() {
            let line = self.strings().fmt("ClTopRow", &[&format!("{:>2}", i + 1), handle, &haul.to_string(), at]);
            let line = crate::editor::strip_colours(&line);
            self.stage_line(y, &painted(&line, if i == 0 { BANK } else { PROSE }), st.truecolor);
            y += 1;
        }
        self.cl_status(st, p, run.haul, run.trace);
        self.cl_anykey(st).await?;
        st.enc.forget();
        self.screen.write_bytes(b"\x1b[2J");
        Ok(())
    }

    async fn cl_help(&mut self, st: &mut Stage, p: &Player, run: &Run) -> io::Result<()> {
        st.enc.forget();
        self.screen.write_bytes(b"\x1b[2J");
        let mut y = 3u16;
        self.cl_center(st, y, "ClHelpHead", &[], BANK);
        y += 2;
        for k in ["ClHelp1", "ClHelp2", "ClHelp3"] {
            self.cl_center(st, y, k, &[], PROSE);
            y += 2;
        }
        self.cl_status(st, p, run.haul, run.trace);
        self.cl_anykey(st).await?;
        st.enc.forget();
        self.screen.write_bytes(b"\x1b[2J");
        Ok(())
    }

    fn cl_center(&mut self, st: &Stage, row: u16, name: &str, args: &[&str], color: Ink) {
        let s = self.strings().fmt(name, args);
        let s = crate::editor::strip_colours(&s);
        self.stage_line(row, &painted(&s, color), st.truecolor);
    }

    fn cl_say(&mut self, st: &Stage, name: &str, args: &[&str], color: Ink) {
        let s = self.strings().fmt(name, args);
        let s = painted(&crate::editor::strip_colours(&s), color);
        self.stage_line(MSG_ROW, &s, st.truecolor);
    }

    fn cl_status(&mut self, st: &Stage, p: &Player, haul: u32, trace: u32) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        for (label, n) in [("haul ", haul), ("   best ", p.best)] {
            ink(&mut out, [150, 170, 230], st.truecolor);
            out.extend(label.as_bytes());
            ink(&mut out, [255, 230, 140], st.truecolor);
            out.extend(n.to_string().as_bytes());
        }
        ink(&mut out, [150, 170, 230], st.truecolor);
        out.extend(b"   trace ");
        let tc: Ink = if trace >= 70 { [255, 90, 70] } else { [120, 220, 140] };
        ink(&mut out, tc, st.truecolor);
        out.extend(format!("{trace:>3}%").as_bytes());
        ink(&mut out, DIM, st.truecolor);
        out.extend(format!("   {}", GEAR[(p.gear as usize).min(GEAR.len() - 1)]).as_bytes());
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let keys = self.strings().get("ClKeys").to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(&keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    async fn cl_anykey(&mut self, st: &mut Stage) -> io::Result<()> {
        let s = self.strings().get("ClAnyKey").to_string();
        let s = painted(&crate::editor::strip_colours(&s), DIM);
        self.stage_line(MSG_ROW, &s, st.truecolor);
        self.read_key().await?;
        Ok(())
    }

    // ---- plain text -----------------------------------------------------------

    async fn cl_plain(&mut self, cfg: &Cfg, p: &mut Player, today: &str) -> io::Result<()> {
        if p.busted == today {
            return self.show_line(self.strings().get("ClBustedToday")).await;
        }
        self.show_line(self.strings().get("ClHello")).await?;
        let (mut haul, mut trace) = (0u32, 0u32);
        loop {
            let prompt = self.strings().fmt("ClPlainPrompt", &[&haul.to_string(), &trace.to_string()]);
            self.show(&prompt).await?;
            let line = self.read_string(crate::input::ReadOpts::new(10)).await?;
            self.newline();
            if self.escaped {
                break;
            }
            match line.trim().to_ascii_lowercase().chars().next() {
                None | Some('q') | Some('c') => break,
                Some('t') => {
                    for (i, (h, haul, _)) in self.state.carrier.top(8).unwrap_or_default().into_iter().enumerate() {
                        self.show_line(&format!("{:>2}. {h} -- {haul} bytes", i + 1)).await?;
                    }
                }
                Some('?') | Some('h') => {
                    for k in ["ClHelp1", "ClHelp2", "ClHelp3"] {
                        self.show_line(self.strings().get(k)).await?;
                    }
                }
                Some('d') => {
                    if cfg.dials_per_day > 0 && p.dials >= cfg.dials_per_day {
                        self.show_line(self.strings().get("ClNoDials")).await?;
                        continue;
                    }
                    p.dials += 1;
                    // Roll it all before any await: ThreadRng isn't Send.
                    let roll = {
                        let mut rng = rand::rng();
                        if rng.random_range(0..3) == 0 {
                            None
                        } else {
                            let tier = 1 + rng.random_range(0..4);
                            let hits = rng.random_range(0..=3u32);
                            let kind = Kind::ALL[rng.random_range(0..Kind::ALL.len())];
                            Some((tier, hits, kind))
                        }
                    };
                    match roll {
                        None => {
                            trace = (trace + 1).min(100);
                            self.show_line(self.strings().get("ClNoCarrier")).await?;
                        }
                        Some((tier, hits, kind)) => {
                            let bytes = tier * (6 + hits * 14);
                            haul += bytes;
                            trace = (trace + (6 + (3 - hits) * 6).saturating_sub((p.gear * 2).min(8)).max(2)).min(100);
                            self.show_line(&self.strings().fmt("ClHaul", &[&hits.to_string(), &bytes.to_string(), kind_name(kind)])).await?;
                            if trace >= 100 {
                                p.busted = today.to_string();
                                p.runs += 1;
                                self.show_line(self.strings().get("ClBusted")).await?;
                                return Ok(());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.escaped = false;
        p.runs += 1;
        if haul == 0 {
            return self.show_line(self.strings().get("ClBankedNothing")).await;
        }
        if haul > p.best {
            p.best = haul;
        }
        let at = crate::clock::now().naive_local().format("%Y-%m-%d %H:%M").to_string();
        let _ = self.state.carrier.add_score(&self.user.handle, haul, &at);
        let prize = self.cl_pay(p, haul);
        let pt = self.casino_prize_text(prize);
        self.show_line(&self.strings().fmt("ClBanked", &[&haul.to_string(), &pt])).await
    }
}

/// A throwaway `Player` for a status line drawn mid-connect, where the
/// real one is borrowed. Gear 0 is fine for that single frame.
fn p_none() -> &'static Player {
    static P: std::sync::OnceLock<Player> = std::sync::OnceLock::new();
    P.get_or_init(Player::default)
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Unix => "unix box",
        Kind::Vax => "vax cluster",
        Kind::Bank => "wire terminal",
        Kind::Switch => "phone switch",
        Kind::Pager => "paging host",
        Kind::Bbs => "pirate board",
    }
}

fn kind_rgb(k: Kind) -> Ink {
    match k {
        Kind::Unix => [90, 255, 120],
        Kind::Vax => [130, 215, 255],
        Kind::Bank => [255, 196, 70],
        Kind::Switch => [255, 120, 90],
        Kind::Pager => [150, 255, 220],
        Kind::Bbs => [255, 150, 230],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_tunes_and_refreshes_the_exchange() {
        let mut run = Run::new();
        assert!(run.any_live(), "a fresh exchange has live carriers");
        // Take every carrier; the needle wraps between 0 and 1.
        for i in 0..run.notches.len() {
            run.notches[i].taken = true;
        }
        assert!(!run.any_live());
        let ex = run.exchange;
        run.new_exchange();
        assert_eq!(run.exchange, ex + 1);
        assert!(run.any_live());
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let n = run.needle(t);
            assert!((0.0..=1.0).contains(&n), "needle stays on the track at t={t}: {n}");
        }
    }

    #[test]
    fn the_board_keeps_the_biggest_hauls_first() {
        let s = CarrierStore::open_memory().unwrap();
        s.add_score("qtest", 300, "now").unwrap();
        s.add_score("push", 900, "now").unwrap();
        s.add_score("una", 120, "now").unwrap();
        let top = s.top(8).unwrap();
        assert_eq!(top[0].0, "push");
        assert_eq!(top[0].1, 900);
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn a_player_round_trips() {
        let s = CarrierStore::open_memory().unwrap();
        let p = Player { gear: 2, best: 450, runs: 3, day: "2026-10-26".into(), dials: 12, busted: String::new() };
        s.save_player(7, "push", &p).unwrap();
        let got = s.player(7).unwrap();
        assert_eq!((got.gear, got.best, got.dials), (2, 450, 12));
        assert_eq!(s.player(99).unwrap().best, 0, "an unknown player is fresh");
    }
}
