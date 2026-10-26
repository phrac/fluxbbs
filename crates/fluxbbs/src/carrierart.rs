//! New: the pictures for Carrier Lost (`carrierlost`), a 1994 dial-up
//! game. A banner across the top of the screen in half-block pixels with
//! text cells over it: the caller's desk (a modem, a CRT, a wardialer)
//! while they're home, the modem's lights cycling while it dials, the
//! phosphor title card of whatever system answered once they're on it,
//! and the alert when a trace runs them down. All period make-believe.

use crate::pixels::{Canvas, Rgb, hash, mix, rgb, scale, smooth};

/// The banner: the whole width, rows 1 to 12 (so 12 text rows over it).
pub const W: usize = 80;
pub const H: usize = 24;

/// The kind of system a number answers with, each its own phosphor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Unix,
    Vax,
    Bank,
    Switch,
    Pager,
    Bbs,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Unix, Kind::Vax, Kind::Bank, Kind::Switch, Kind::Pager, Kind::Bbs];

    /// The phosphor the system's screen glows in.
    fn phosphor(self) -> Rgb {
        match self {
            Kind::Unix => rgb(90, 255, 120),
            Kind::Vax => rgb(130, 215, 255),
            Kind::Bank => rgb(255, 196, 70),
            Kind::Switch => rgb(255, 120, 90),
            Kind::Pager => rgb(150, 255, 220),
            Kind::Bbs => rgb(255, 150, 230),
        }
    }

    /// A line shown under the name, in the system's own voice.
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Unix => "UNIX System V/386  Release 3.2",
            Kind::Vax => "VAX/VMS  Version 5.4",
            Kind::Bank => "FEDWIRE FUNDS TRANSFER TERMINAL",
            Kind::Switch => "CENTRAL OFFICE SWITCH -- AUTHORIZED USE ONLY",
            Kind::Pager => "ALPHANUMERIC PAGING TERMINAL",
            Kind::Bbs => "another bulletin board, somewhere out there",
        }
    }
}

// ---- helpers --------------------------------------------------------------------

/// Phosphor glow of a point lit `v` (0..1) on glass tinted `c`, with a
/// scanline every other row.
fn glass(c: Rgb, v: f32, y: i32) -> Rgb {
    let scan = if y % 2 == 0 { 1.0 } else { 0.84 };
    mix(scale(c, 0.1 * scan), c, v)
}

/// The CRT bezel and dark glass; returns the glass rectangle in text
/// cells (col0, row0, cols, rows).
fn crt(cv: &mut Canvas, tint: Rgb) -> (usize, usize, usize, usize) {
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            cv.set(x, y, rgb(8, 8, 12));
        }
    }
    let (bx, by, bw, bh) = (4, 0, 72, H as i32 - 2);
    for y in by..by + bh {
        for x in bx..bx + bw {
            let bevel = if x < bx + 2 || x >= bx + bw - 2 || y < by + 1 || y >= by + bh - 1 { 0.65 } else { 1.0 };
            cv.set(x, y, scale(rgb(150, 142, 120), bevel));
        }
    }
    let (gx, gy, gw, gh) = (bx + 3, by + 2, bw - 6, bh - 4);
    for y in gy..gy + gh {
        for x in gx..gx + gw {
            cv.set(x, y, glass(tint, 0.0, y));
        }
    }
    (gx as usize, (gy as usize) / 2, gw as usize, (gh as usize) / 2)
}

/// Centres a label on a glass row.
fn centre(cv: &mut Canvas, col0: usize, cols: usize, row: usize, s: &str, c: Rgb) {
    let s: String = s.chars().take(cols).collect();
    let at = col0 + cols.saturating_sub(s.chars().count()) / 2;
    cv.label(at, row, &s, c, None);
}

// ---- the desk ------------------------------------------------------------------

/// The caller's desk at home. `dialing` is (progress, number) while a
/// number is being tried; `found` is carriers found so far this call.
pub fn draw_desk(cv: &mut Canvas, t: f32, dialing: Option<(f32, &str)>, found: u32) {
    cv.clear_labels();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            cv.set(x, y, scale(rgb(44, 40, 58), 0.7 + 0.1 * hash(x / 4, y / 4)));
        }
    }
    for y in (H as i32 - 4)..H as i32 {
        for x in 0..W as i32 {
            cv.set(x, y, scale(rgb(80, 60, 42), 0.7 + 0.1 * hash(x, y)));
        }
    }
    // The monitor, left: a beige shell and a small green screen.
    let (mx, my, mw, mh) = (3, 2, 30, 16);
    for y in my..my + mh {
        for x in mx..mx + mw {
            let edge = x < mx + 2 || x >= mx + mw - 2 || y < my + 1 || y >= my + mh - 2;
            cv.set(x, y, scale(rgb(150, 142, 120), if edge { 0.6 } else { 1.0 }));
        }
    }
    let (sx, sy, sw, sh) = (mx + 3, my + 2, mw - 6, mh - 5);
    for y in sy..sy + sh {
        for x in sx..sx + sw {
            cv.set(x, y, glass(rgb(90, 255, 120), 0.0, y));
        }
    }
    let (tc, tr, tw) = (sx as usize, (sy as usize) / 2, sw as usize);
    cv.label(tc, tr, &"READ ME.1ST".chars().take(tw).collect::<String>(), rgb(120, 230, 150), None);
    cv.label(tc, tr + 1, &"-".repeat(tw.min(11)), scale(rgb(90, 255, 120), 0.6), None);
    cv.label(tc, tr + 3, &"the scene never".chars().take(tw).collect::<String>(), rgb(110, 220, 140), None);
    cv.label(tc, tr + 4, "sleeps_", rgb(110, 220, 140), None);
    // The modem box, right, with its LED row.
    let (bx, by, bw) = (40, H as i32 - 10, 36);
    for y in by..by + 7 {
        for x in bx..bx + bw {
            cv.set(x, y, scale(rgb(32, 32, 40), if y == by || y == by + 6 { 0.6 } else { 1.0 }));
        }
    }
    cv.label(bx as usize + 1, (by as usize) / 2, "USRobotics Courier", rgb(150, 160, 185), None);
    let labels = ["HS", "CD", "OH", "RD", "SD", "TR", "MR"];
    let lit = modem_lights(t, dialing.is_some());
    for (i, lab) in labels.iter().enumerate() {
        let lx = bx + 2 + i as i32 * 5;
        let c = if i == 2 { rgb(255, 90, 90) } else if i < 2 { rgb(90, 255, 120) } else { rgb(255, 200, 70) };
        cv.rect(lx, by + 3, 3, 2, if lit[i] { c } else { scale(c, 0.12) });
        cv.label(lx as usize, (by as usize) / 2 + 3, lab, rgb(130, 130, 140), None);
    }
    // The wardialer readout, upper right.
    cv.label(40, 1, "== WARDIALER v2.1 ==", rgb(130, 200, 255), None);
    match dialing {
        Some((_, num)) => {
            cv.label(40, 3, &format!("dialing  {num}"), rgb(150, 255, 170), None);
            let dots = ".".repeat((t * 4.0) as usize % 4);
            cv.label(40, 4, &format!("carrier{dots}     "), rgb(255, 200, 70), None);
        }
        None => {
            cv.label(40, 3, "idle -- scan <exchange>", scale(rgb(150, 255, 170), 0.8), None);
        }
    }
    cv.label(40, 5, &format!("carriers this call: {found}"), rgb(150, 255, 170), None);
}

fn modem_lights(t: f32, dialing: bool) -> [bool; 7] {
    let mut l = [false; 7];
    l[5] = true;
    l[6] = true;
    if dialing {
        l[2] = true;
        l[3] = (t * 12.0) as i32 % 2 == 0;
        l[4] = (t * 12.0) as i32 % 2 == 1;
    }
    l
}

// ---- a system's title card ------------------------------------------------------

/// The phosphor title card of the system the caller is on: its name, its
/// tag and a trace bar that fills and reddens as the heat climbs.
pub fn draw_system(cv: &mut Canvas, t: f32, kind: Kind, name: &str, trace: u32, backdoor: bool) {
    cv.clear_labels();
    let tint = kind.phosphor();
    let (c0, r0, cols, rows) = crt(cv, tint);
    let name = name.to_uppercase();
    centre(cv, c0, cols, r0 + 1, &name, tint);
    centre(cv, c0, cols, r0 + 2, kind.tag(), scale(tint, 0.85));
    centre(cv, c0, cols, r0 + 4, "- CONNECT 2400 -", scale(tint, 0.75));
    if backdoor {
        centre(cv, c0, cols, r0 + 5, "[ your back door still open ]", rgb(255, 150, 230));
    }
    let bar_row = r0 + rows - 1;
    let danger = trace >= 70;
    let col = if danger {
        mix(rgb(255, 180, 40), rgb(255, 40, 40), (trace.saturating_sub(70) as f32) / 30.0)
    } else {
        rgb(90, 220, 120)
    };
    let blink = if danger { 0.5 + 0.5 * (t * 8.0).sin().abs() } else { 1.0 };
    let inner = cols.saturating_sub(14);
    let fill = inner * trace.min(100) as usize / 100;
    let mut bar = String::from("TRACE [");
    for i in 0..inner {
        bar.push(if i < fill { '#' } else { '.' });
    }
    bar.push_str(&format!("] {trace:>3}%"));
    cv.label(c0, bar_row, &bar.chars().take(cols).collect::<String>(), scale(col, blink), None);
}

/// A small emblem for the kind (unused for now; kept for later).
#[allow(dead_code)]
fn emblem(cv: &mut Canvas, kind: Kind, t: f32) {
    let tint = kind.phosphor();
    let (ox, oy) = (12, 5);
    let put = |cv: &mut Canvas, dx: i32, dy: i32, v: f32| cv.set(ox + dx, oy + dy, scale(tint, v));
    match kind {
        Kind::Unix => {
            for dy in -2i32..=2 {
                put(cv, -4, dy, 0.9);
            }
            if (t * 2.0) as i32 % 2 == 0 {
                cv.rect(ox, oy - 1, 3, 3, scale(tint, 0.9));
            }
        }
        Kind::Vax => {
            for dy in -3i32..=3 {
                let w = 3 - dy.abs();
                put(cv, -w, dy, 0.9);
                put(cv, w, dy, 0.9);
            }
        }
        Kind::Bank => {
            for a in 0..32 {
                let ang = a as f32 / 32.0 * std::f32::consts::TAU;
                put(cv, (ang.cos() * 4.0) as i32, (ang.sin() * 3.0) as i32, 0.85);
            }
        }
        Kind::Switch => {
            for gx in -1..=1 {
                for gy in -1..=1 {
                    let on = ((gx + gy + (t * 2.0) as i32) % 2) == 0;
                    put(cv, gx * 3, gy * 2, if on { 0.95 } else { 0.25 });
                }
            }
        }
        Kind::Pager => {
            for dx in -4..=4 {
                put(cv, dx, ((dx as f32 * 0.8 + t * 4.0).sin() * 2.0) as i32, 0.95);
            }
        }
        Kind::Bbs => {
            for dy in -2i32..=2 {
                put(cv, -3 + dy.abs(), dy, 0.9);
                put(cv, 3 - dy.abs(), dy, 0.9);
            }
        }
    }
}

/// The alert when a trace catches the caller.
pub fn draw_busted(cv: &mut Canvas, t: f32) {
    cv.clear_labels();
    let flash = 0.4 + 0.6 * (t * 6.0).sin().abs();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let stripe = ((x + (t * 10.0) as i32) / 6) % 2 == 0;
            cv.set(x, y, if stripe { scale(rgb(255, 40, 40), flash) } else { rgb(24, 4, 4) });
        }
    }
    cv.rect(10, 6, 60, 12, rgb(10, 0, 0));
    centre(cv, 5, 70, 4, "*** TRACE COMPLETE ***", scale(rgb(255, 220, 120), (flash + 0.2).min(1.0)));
    centre(cv, 5, 70, 6, "they have your number", rgb(255, 180, 140));
}

/// Smooths the far edges a touch.
pub fn vignette(cv: &mut Canvas) {
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    for y in 0..H {
        for x in 0..W {
            let (dx, dy) = ((x as f32 - cx) / (W as f32 * 0.64), (y as f32 - cy) / (H as f32 * 0.64));
            let k = 1.0 - smooth(0.85, 1.15, (dx * dx + dy * dy).sqrt()) * 0.3;
            let i = y * W + x;
            cv.px[i] = scale(cv.px[i], k);
        }
    }
}

/// The glass rectangle `crt` leaves, in pixels (x, y, w, h).
pub const GLASS: (i32, i32, i32, i32) = (7, 2, 66, 18);

/// A row of the glass, as a text-cell row (0-based).
fn grow(dy: usize) -> usize {
    (GLASS.1 as usize) / 2 + dy
}

/// The dial: a tuning track across the glass with carrier notches and a
/// sweeping needle. `notches` is (position 0..1, state: 0 dead, 1 live,
/// 2 taken); `needle` is 0..1 across the track.
pub fn draw_dial(cv: &mut Canvas, t: f32, tint: Rgb, notches: &[(f32, u8)], needle: f32, found: u32) {
    cv.clear_labels();
    crt(cv, tint);
    let (gx, _, gw, _) = GLASS;
    let (x0, x1) = (gx + 5, gx + gw - 5);
    let span = (x1 - x0) as f32;
    centre(cv, gx as usize, gw as usize, grow(0), "W A R D I A L E R", tint);
    centre(cv, gx as usize, gw as usize, grow(1), "scanning the exchange", scale(tint, 0.7));
    let ytk = 11;
    // The track.
    for x in x0..=x1 {
        cv.set(x, ytk, scale(tint, 0.35));
        cv.set(x, ytk + 1, scale(tint, 0.18));
    }
    // Tick marks along it.
    let mut m = x0;
    while m <= x1 {
        cv.set(m, ytk - 2, scale(tint, 0.3));
        m += 4;
    }
    // The carriers.
    for &(pos, state) in notches {
        let x = x0 + (pos.clamp(0.0, 1.0) * span) as i32;
        let c = match state {
            1 => rgb(90, 255, 120),
            2 => scale(tint, 0.2),
            _ => scale(rgb(120, 90, 90), 0.5),
        };
        let h = if state == 1 { 4 } else { 2 };
        let glow = if state == 1 { 0.6 + 0.4 * (t * 5.0 + pos * 9.0).sin().abs() } else { 1.0 };
        for dy in -h..=h {
            cv.set(x, ytk + dy, scale(c, glow));
        }
    }
    // The needle.
    let nx = x0 + (needle.clamp(0.0, 1.0) * span) as i32;
    for y in (ytk - 4)..=(ytk + 6) {
        cv.blend(nx, y, rgb(255, 255, 255), 0.9);
    }
    cv.blend(nx - 1, ytk - 4, rgb(255, 255, 255), 0.4);
    cv.blend(nx + 1, ytk - 4, rgb(255, 255, 255), 0.4);
    centre(cv, gx as usize, gw as usize, grow(7), &format!("carriers raised tonight: {found}"), scale(tint, 0.75));
}

/// The handshake: three signal columns bobbing, a sync band across the
/// middle, lock each in the band. `bars` is each column's value 0..1,
/// `locked` whether each has been locked and if it hit, `cur` the column
/// being aimed at, `zone` the half-height of the band.
pub fn draw_handshake(cv: &mut Canvas, t: f32, tint: Rgb, bars: &[f32; 3], locked: &[Option<bool>; 3], cur: usize, zone: f32) {
    cv.clear_labels();
    crt(cv, tint);
    let (gx, gy, gw, gh) = GLASS;
    centre(cv, gx as usize, gw as usize, grow(0), "H A N D S H A K E", tint);
    let (top, bot) = (gy + 3, gy + gh - 2);
    let band_mid = (top + bot) / 2;
    let half = (zone * (bot - top) as f32 / 2.0).max(1.0) as i32;
    // The sync band.
    for y in (band_mid - half)..=(band_mid + half) {
        for x in (gx + 3)..(gx + gw - 3) {
            cv.blend(x, y, tint, 0.18);
        }
    }
    for x in (gx + 3)..(gx + gw - 3) {
        cv.blend(x, band_mid - half, tint, 0.5);
        cv.blend(x, band_mid + half, tint, 0.5);
    }
    let cols = [gx + gw / 4 - 3, gx + gw / 2 - 3, gx + 3 * gw / 4 - 3];
    for (i, &cx) in cols.iter().enumerate() {
        let v = bars[i].clamp(0.0, 1.0);
        let y = bot - (v * (bot - top) as f32) as i32;
        let c = match locked[i] {
            Some(true) => rgb(90, 255, 130),
            Some(false) => rgb(255, 80, 80),
            None if i == cur => mix(tint, rgb(255, 255, 255), 0.3 + 0.3 * (t * 6.0).sin().abs()),
            None => scale(tint, 0.5),
        };
        // The column's slider block.
        cv.rect(cx, y - 1, 6, 3, c);
        // A faint shaft.
        for yy in top..bot {
            cv.blend(cx + 2, yy, tint, 0.1);
            cv.blend(cx + 3, yy, tint, 0.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CL_OUT=dir cargo test --release -p fluxbbs carrier_pictures --
    /// --ignored` writes ANSI files there.
    #[test]
    #[ignore]
    fn carrier_pictures() {
        let dir = std::path::PathBuf::from(std::env::var("CL_OUT").expect("CL_OUT"));
        let mut shots: Vec<(String, Canvas)> = Vec::new();
        let mut cv = Canvas::new(W, H);
        draw_desk(&mut cv, 1.0, None, 3);
        shots.push(("desk".into(), cv));
        let mut cv = Canvas::new(W, H);
        draw_desk(&mut cv, 1.0, Some((0.5, "555-0148")), 3);
        shots.push(("dialing".into(), cv));
        for (i, k) in Kind::ALL.into_iter().enumerate() {
            let mut cv = Canvas::new(W, H);
            draw_system(&mut cv, 1.0, k, &format!("node-{i}"), (i as u32 * 17) % 100, i == 5);
            shots.push((format!("sys-{i}"), cv));
        }
        let mut cv = Canvas::new(W, H);
        let notches = [(0.12, 1u8), (0.3, 0u8), (0.5, 1u8), (0.68, 2u8), (0.86, 1u8)];
        draw_dial(&mut cv, 0.7, rgb(90, 255, 120), &notches, 0.5, 4);
        shots.push(("dial".into(), cv));
        let mut cv = Canvas::new(W, H);
        draw_handshake(&mut cv, 0.4, rgb(130, 215, 255), &[0.52, 0.2, 0.8], &[Some(true), Some(false), None], 2, 0.2);
        shots.push(("handshake".into(), cv));
        let mut cv = Canvas::new(W, H);
        draw_busted(&mut cv, 0.5);
        shots.push(("busted".into(), cv));
        for (name, cv) in shots {
            let mut cv = cv;
            vignette(&mut cv);
            let mut out = b"\x1b[2J".to_vec();
            out.extend(crate::pixels::Encoder::new(true, W, H).frame(&cv, 1));
            std::fs::write(dir.join(format!("{name}.ans")), &out).unwrap();
        }
    }
}
