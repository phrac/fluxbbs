//! New: the slot machine's pictures (`slots.rs`). The screen is a canvas
//! of half-block pixels (80 by 44, two to a cell, so they're square):
//! three reels behind glass, a chrome bezel and a ring of bulbs. The
//! symbols are drawn as shaded shapes, smoothed, once; a reel is a lit
//! cylinder, so the symbols above and below the payline bend away and
//! darken, and a spinning one is blurred by its speed. Frames go out
//! as only the cells that changed, in 24-bit color or xterm's 256.

use std::f32::consts::PI;
use std::sync::OnceLock;

pub const W: usize = 80;
pub const H: usize = 44;
/// Symbols on each reel, and the distance between them (pixels).
pub const STOPS: usize = 20;
pub const PITCH: f32 = 22.0;
pub const CIRCLE: f32 = STOPS as f32 * PITCH;

pub const CHERRY: usize = 0;
pub const LEMON: usize = 1;
pub const ORANGE: usize = 2;
pub const PLUM: usize = 3;
pub const BELL: usize = 4;
pub const BAR: usize = 5;
pub const SEVEN: usize = 6;
pub const SYMBOLS: usize = 7;
pub const NAMES: [&str; SYMBOLS] = ["cherry", "lemon", "orange", "plum", "bell", "bar", "seven"];
/// Each symbol's color, for text about it.
pub const INK: [[u8; 3]; SYMBOLS] = [[235, 40, 60], [255, 225, 60], [255, 150, 30], [190, 90, 230], [255, 200, 60], [235, 235, 235], [255, 60, 50]];

/// The reels, top to bottom as they pass the payline.
pub const STRIPS: [[usize; STOPS]; 3] = [
    [SEVEN, CHERRY, LEMON, BAR, PLUM, CHERRY, ORANGE, BELL, LEMON, CHERRY, PLUM, BAR, ORANGE, LEMON, BELL, CHERRY, PLUM, ORANGE, LEMON, CHERRY],
    [CHERRY, BELL, ORANGE, SEVEN, LEMON, PLUM, CHERRY, BAR, LEMON, ORANGE, CHERRY, PLUM, BELL, LEMON, CHERRY, ORANGE, BAR, PLUM, LEMON, CHERRY],
    [LEMON, CHERRY, PLUM, BAR, ORANGE, LEMON, SEVEN, CHERRY, BELL, PLUM, LEMON, ORANGE, CHERRY, BAR, PLUM, BELL, LEMON, ORANGE, CHERRY, PLUM],
];

/// The reel windows: left edges, and the rows they span.
pub const WIN_X: [usize; 3] = [4, 29, 54];
pub const WIN_W: usize = 22;
const WIN_Y: usize = 4;
const WIN_H: usize = 36;
/// How far round the reel the window sees, each way from the payline.
const VIEW: f32 = 58.0 * PI / 180.0;

type Rgb = [f32; 3];

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    [r as f32, g as f32, b as f32]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(a: Rgb, k: f32) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn add(a: Rgb, b: Rgb, k: f32) -> Rgb {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

// ---- the symbols -------------------------------------------------------

/// A symbol's art: 20 pixels wide, 18 high, kept at two texels a pixel
/// down (for the reel's bending), color with coverage.
const ART_W: usize = 20;
const ART_H: f32 = 18.0;
const TEX_H: usize = 36;

struct Art {
    px: Vec<[f32; 4]>,
}

/// Light from the upper left, in front.
const LIGHT: [f32; 3] = [-0.45, -0.6, 0.66];

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}

/// A lit ball (or egg): base color, the shadow side's, the outline's.
#[allow(clippy::too_many_arguments)]
fn ball(u: f32, v: f32, cx: f32, cy: f32, rx: f32, ry: f32, base: Rgb, dark: Rgb, shine: f32, power: f32) -> Option<Rgb> {
    let (dx, dy) = ((u - cx) / rx, (v - cy) / ry);
    let d2 = dx * dx + dy * dy;
    if d2 > 1.0 {
        return None;
    }
    let nz = (1.0 - d2).sqrt();
    let l = norm(LIGHT);
    let diff = (dx * l[0] + dy * l[1] + nz * l[2]).max(0.0);
    let h = norm([l[0], l[1], l[2] + 1.0]);
    let spec = (dx * h[0] + dy * h[1] + nz * h[2]).max(0.0).powf(power);
    let mut c = mix(dark, base, diff.powf(0.7) * 1.1);
    // Light thrown back from below.
    c = add(c, base, 0.18 * smooth(0.3, 0.9, dy) * smooth(0.5, 0.95, d2.sqrt()));
    c = mix(c, rgb(255, 255, 255), spec * shine);
    // The outline, about a pixel.
    let edge = (1.0 - d2.sqrt()) * rx.min(ry);
    if edge < 0.8 {
        c = mix(scale(dark, 0.45), c, edge / 0.8 * 0.5);
    }
    Some(c)
}

/// Distance from `p` to the segment `a`-`b`, and how far along it.
fn seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (bx, by) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * bx + (p.1 - a.1) * by) / (bx * bx + by * by)).clamp(0.0, 1.0);
    let (dx, dy) = (p.0 - a.0 - bx * t, p.1 - a.1 - by * t);
    ((dx * dx + dy * dy).sqrt(), t)
}

/// Distance to a quadratic curve.
fn curve(p: (f32, f32), a: (f32, f32), c: (f32, f32), b: (f32, f32)) -> f32 {
    let at = |t: f32| {
        let s = 1.0 - t;
        (s * s * a.0 + 2.0 * s * t * c.0 + t * t * b.0, s * s * a.1 + 2.0 * s * t * c.1 + t * t * b.1)
    };
    let mut best = f32::MAX;
    let mut prev = a;
    for i in 1..=16 {
        let next = at(i as f32 / 16.0);
        best = best.min(seg(p, prev, next).0);
        prev = next;
    }
    best
}

/// A leaf from `a` to `b`, `w` at its widest.
fn leaf(u: f32, v: f32, a: (f32, f32), b: (f32, f32), w: f32) -> Option<Rgb> {
    let (d, t) = seg((u, v), a, b);
    let half = w * (PI * t).sin().powf(0.7);
    if d > half || t <= 0.0 || t >= 1.0 {
        return None;
    }
    // Which side of the vein: the upper one catches the light.
    let side = (b.0 - a.0) * (v - a.1) - (b.1 - a.1) * (u - a.0);
    let mut c = if side < 0.0 { rgb(120, 215, 70) } else { rgb(50, 150, 40) };
    if d < 0.3 {
        c = rgb(40, 110, 30);
    }
    if half - d < 0.45 {
        c = rgb(20, 70, 15);
    }
    Some(c)
}

fn cherry(u: f32, v: f32) -> Option<Rgb> {
    let (red, dark) = (rgb(220, 18, 42), rgb(95, 0, 20));
    if let Some(c) = ball(u, v, 13.9, 13.1, 4.5, 4.5, red, dark, 0.9, 24.0) {
        return Some(c);
    }
    if let Some(c) = ball(u, v, 5.9, 12.4, 4.5, 4.5, red, dark, 0.9, 24.0) {
        return Some(c);
    }
    let stem = curve((u, v), (6.4, 8.4), (7.0, 3.6), (11.6, 1.9)).min(curve((u, v), (13.6, 9.0), (13.0, 4.4), (11.6, 1.9)));
    if stem < 0.75 {
        return Some(if stem < 0.4 { rgb(120, 140, 40) } else { rgb(60, 70, 15) });
    }
    leaf(u, v, (11.4, 2.0), (18.8, 2.8), 2.1)
}

fn lemon(u: f32, v: f32) -> Option<Rgb> {
    let (s, c) = (-0.33f32).sin_cos();
    let (x, y) = (u - 10.0, v - 9.6);
    let (lx, ly) = (x * c - y * s, x * s + y * c);
    let (rx, ry) = (7.4, 5.7);
    let (base, dark) = (rgb(255, 226, 40), rgb(180, 120, 0));
    let inside = (lx / rx).powi(2) + (ly / ry).powi(2) <= 1.0;
    let tip = ((lx.abs() - 7.6).powi(2) + ly * ly).sqrt() < 1.35;
    if !inside && !tip {
        return None;
    }
    let mut col = if inside {
        ball(lx + 10.0, ly + 9.6, 10.0, 9.6, rx, ry, base, dark, 0.75, 16.0)?
    } else {
        let edge = 1.35 - ((lx.abs() - 7.6).powi(2) + ly * ly).sqrt();
        if edge < 0.6 { rgb(110, 70, 0) } else { mix(dark, base, 0.55) }
    };
    // The peel's pores.
    if hash((u * 2.0) as i32, (v * 2.0) as i32) < 0.12 {
        col = scale(col, 0.93);
    }
    Some(col)
}

fn orange(u: f32, v: f32) -> Option<Rgb> {
    if let Some(c) = leaf(u, v, (10.6, 3.6), (16.6, 1.0), 1.7) {
        return Some(c);
    }
    if ((u - 10.0).powi(2) + (v - 3.6).powi(2)).sqrt() < 0.95 {
        return Some(rgb(70, 95, 25));
    }
    let mut c = ball(u, v, 10.0, 10.7, 7.1, 7.1, rgb(255, 142, 8), rgb(150, 50, 0), 0.55, 12.0)?;
    if hash((u * 1.5) as i32, (v * 1.5) as i32) < 0.2 {
        c = scale(c, 0.9);
    }
    Some(c)
}

fn plum(u: f32, v: f32) -> Option<Rgb> {
    let (d, _) = seg((u, v), (10.3, 4.6), (11.8, 1.1));
    if d < 0.55 {
        return Some(rgb(105, 70, 35));
    }
    let mut c = ball(u, v, 10.0, 10.9, 6.9, 6.9, rgb(135, 45, 175), rgb(40, 5, 60), 0.0, 1.0)?;
    // A dusty bloom rather than a shine.
    let (dx, dy) = ((u - 7.6) / 3.2, (v - 7.8) / 2.6);
    c = mix(c, rgb(215, 180, 255), 0.42 * (-(dx * dx + dy * dy)).exp());
    // The crease.
    let dy = (v - 10.9) / 6.9;
    let crease = 10.0 + 2.2 * (1.0 - dy * dy).max(0.0).sqrt() - 0.6;
    if (u - crease).abs() < 0.4 && dy < 0.55 {
        c = scale(c, 0.7);
    }
    Some(c)
}

/// Gold, as a cylinder lit from the upper left: `nx` from -1 (left
/// edge) to 1.
fn gold(nx: f32) -> Rgb {
    let (hi, mid, dark) = (rgb(255, 248, 205), rgb(245, 185, 35), rgb(125, 72, 0));
    let body = 0.5 + 0.5 * (PI * (nx + 0.42) * 0.8).cos();
    let mut c = mix(dark, mid, body);
    c = mix(c, hi, 0.9 * (-((nx + 0.42) / 0.15).powi(2)).exp());
    // Light thrown back on the right.
    mix(c, mid, 0.35 * smooth(0.7, 0.95, nx))
}

fn bell(u: f32, v: f32) -> Option<Rgb> {
    let outline = rgb(90, 50, 0);
    // The clapper, under the rim.
    if let Some(c) = ball(u, v, 10.0, 16.8, 1.2, 1.2, rgb(230, 160, 20), rgb(100, 55, 0), 0.6, 10.0) {
        return Some(c);
    }
    // The rim.
    if (14.3..=15.9).contains(&v) {
        let half = 8.9 - (1.0 - ((v - 15.1) / 0.8).powi(2)).max(0.0).sqrt().mul_add(-0.8, 0.8);
        let x = (u - 10.0).abs();
        if x <= half {
            let mut c = gold((u - 10.0) / half);
            if v < 14.8 {
                c = mix(c, rgb(255, 245, 200), 0.35);
            } else if v > 15.5 || half - x < 0.6 {
                c = mix(c, outline, 0.6);
            }
            return Some(c);
        }
        return None;
    }
    // The knob on top.
    let k = ((u - 10.0).powi(2) + (v - 2.5).powi(2)).sqrt();
    if k < 1.35 {
        return Some(if k > 0.85 { outline } else { gold((u - 10.0) / 1.35) });
    }
    // The body: a round crown, straight sides widening a little, then
    // the flare out to the rim.
    let (top, bot) = (3.4, 14.3);
    if !(top..=bot).contains(&v) {
        return None;
    }
    let t = (v - top) / (bot - top);
    let half = if t < 0.34 {
        5.1 * (1.0 - ((0.34 - t) / 0.34).powi(2)).max(0.0).sqrt()
    } else if t < 0.72 {
        5.1 + 0.6 * (t - 0.34) / 0.38
    } else {
        5.7 + 3.2 * ((t - 0.72) / 0.28).powi(2)
    };
    let x = u - 10.0;
    if x.abs() > half {
        return None;
    }
    let mut c = gold(x / half);
    if (0.62..0.69).contains(&t) {
        c = scale(c, 0.72);
    }
    if half - x.abs() < 0.6 || (t < 0.34 && v - top < 0.5) {
        c = mix(c, outline, 0.7);
    }
    Some(c)
}

/// "BAR" in a 5x7 font.
const BAR_FONT: [[u8; 7]; 3] = [
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
    [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
];

fn bar_letter(u: f32, v: f32) -> bool {
    let (x, y) = ((u - 1.0).floor() as i32, (v - 5.0).floor() as i32);
    if x < 0 || !(0..7).contains(&y) || x >= 17 || x % 6 == 5 {
        return false;
    }
    BAR_FONT[(x / 6) as usize][y as usize] >> (4 - x % 6) & 1 == 1
}

fn bar(u: f32, v: f32) -> Option<Rgb> {
    // A rounded plate.
    let (x0, x1, y0, y1, r) = (0.0, 20.0, 3.2, 14.8, 1.6);
    let qx = (x0 + r - u).max(u - (x1 - r)).max(0.0);
    let qy = (y0 + r - v).max(v - (y1 - r)).max(0.0);
    let d = (qx * qx + qy * qy).sqrt() - r;
    let inner = (u - x0).min(x1 - u).min(v - y0).min(y1 - v);
    if d > 0.0 {
        return None;
    }
    if inner < 1.0 {
        // The gold frame, lit from above.
        let t = (v - y0) / (y1 - y0);
        return Some(mix(rgb(255, 235, 150), rgb(150, 90, 0), t));
    }
    if bar_letter(u, v) {
        let t = (v - 5.0) / 7.0;
        return Some(mix(rgb(255, 255, 250), rgb(215, 205, 185), t));
    }
    // Glossy black.
    let gloss = smooth(7.6, 4.0, v) * 0.5;
    Some(mix(rgb(14, 12, 22), rgb(70, 70, 95), gloss))
}

/// How far `p` is outside the 7 (negative inside).
fn seven_shape(u: f32, v: f32) -> f32 {
    let (x0, x1, y0, y1) = (3.6, 16.6, 3.4, 5.8);
    let dx = (x0 - u).max(u - x1);
    let dy = (y0 - v).max(v - y1);
    let bar = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0);
    let stem = seg((u, v), (15.3, 5.0), (8.6, 14.4)).0 - 1.45;
    bar.min(stem)
}

fn seven(u: f32, v: f32) -> Option<Rgb> {
    let d = seven_shape(u, v);
    if d >= 1.9 {
        return None;
    }
    if d >= 0.85 {
        return Some(mix(rgb(40, 90, 235), rgb(10, 25, 110), (d - 0.85) / 1.05));
    }
    if d >= 0.0 {
        return Some(rgb(255, 255, 255));
    }
    let mut c = mix(rgb(255, 80, 60), rgb(165, 0, 15), (v - 3.0) / 12.0);
    // A bevel: the edges facing the light are lighter.
    if d > -0.7 {
        let e = 0.05;
        let (gx, gy) = (seven_shape(u + e, v) - seven_shape(u - e, v), seven_shape(u, v + e) - seven_shape(u, v - e));
        let l = (gx * gx + gy * gy).sqrt().max(1e-6);
        let facing = -(gx * LIGHT[0] + gy * LIGHT[1]) / l;
        c = if facing < 0.0 { mix(c, rgb(255, 190, 170), -facing * 0.6) } else { scale(c, 1.0 - facing * 0.35) };
    }
    Some(c)
}

fn art(sym: usize) -> &'static Art {
    static ARTS: OnceLock<Vec<Art>> = OnceLock::new();
    &ARTS.get_or_init(|| {
        let shapes: [fn(f32, f32) -> Option<Rgb>; SYMBOLS] = [cherry, lemon, orange, plum, bell, bar, seven];
        shapes
            .iter()
            .map(|f| {
                let mut px = vec![[0.0; 4]; ART_W * TEX_H];
                for ty in 0..TEX_H {
                    for tx in 0..ART_W {
                        // Four by four samples a texel.
                        let mut acc = [0.0f32; 4];
                        for j in 0..4 {
                            for i in 0..4 {
                                let u = tx as f32 + (i as f32 + 0.5) / 4.0;
                                let v = (ty as f32 + (j as f32 + 0.5) / 4.0) * ART_H / TEX_H as f32;
                                if let Some(c) = f(u, v) {
                                    acc = [acc[0] + c[0], acc[1] + c[1], acc[2] + c[2], acc[3] + 1.0];
                                }
                            }
                        }
                        px[ty * ART_W + tx] = [acc[0] / 16.0, acc[1] / 16.0, acc[2] / 16.0, acc[3] / 16.0];
                    }
                }
                Art { px }
            })
            .collect()
    })[sym]
}

/// The art at column `x`, `v` pixels down (premultiplied), blended
/// between texel rows.
fn art_at(sym: usize, x: usize, v: f32) -> [f32; 4] {
    let a = art(sym);
    let t = (v * TEX_H as f32 / ART_H - 0.5).clamp(0.0, TEX_H as f32 - 1.0);
    let (i, f) = (t.floor() as usize, t.fract());
    let (p, q) = (a.px[i * ART_W + x], a.px[(i + 1).min(TEX_H - 1) * ART_W + x]);
    [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f, p[2] + (q[2] - p[2]) * f, p[3] + (q[3] - p[3]) * f]
}

// ---- the canvas --------------------------------------------------------

pub struct Canvas {
    pub px: Vec<Rgb>,
}

/// A reel as it's drawn: where it is, how fast it's going (pixels a
/// second, positive down), and how it's lit.
#[derive(Clone, Copy, Default)]
pub struct ReelView {
    pub pos: f32,
    pub speed: f32,
    /// A win's glow (0 to 1) on the strip behind the symbols.
    pub glow: f32,
    /// Dimmed while another reel shows a win.
    pub dim: f32,
}

/// The scene's moving parts.
#[derive(Clone, Default)]
pub struct Scene {
    pub reels: [ReelView; 3],
    /// Which bulbs are lit, by their place round the ring.
    pub bulbs: Vec<bool>,
    /// A glint crossing the glass: its distance across (none if < 0).
    pub glint: f32,
    /// The payline's brightness, 0 to 1.
    pub payline: f32,
    pub coins: Vec<Coin>,
}

#[derive(Clone, Copy)]
pub struct Coin {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Turning: the angle it's at.
    pub spin: f32,
}

/// The bulbs round the edge, clockwise from the top left.
pub fn bulb_spots() -> Vec<(usize, usize)> {
    let mut spots = Vec::new();
    for x in (1..W - 2).step_by(4) {
        spots.push((x, 0));
    }
    for y in (4..H - 4).step_by(4).filter(|y| !(18..26).contains(y)) {
        spots.push((W - 2, y));
    }
    for x in (1..W - 2).step_by(4).rev() {
        spots.push((x, H - 2));
    }
    for y in (4..H - 4).step_by(4).rev().filter(|y| !(18..26).contains(y)) {
        spots.push((0, y));
    }
    spots
}

fn in_window(x: usize, y: usize) -> Option<usize> {
    if !(WIN_Y..WIN_Y + WIN_H).contains(&y) {
        return None;
    }
    WIN_X.iter().position(|&w| (w..w + WIN_W).contains(&x))
}

/// The cabinet's face and bezel: everything but the reels and lights.
fn backdrop() -> &'static Vec<Rgb> {
    static BACK: OnceLock<Vec<Rgb>> = OnceLock::new();
    BACK.get_or_init(|| {
        let mut px = vec![[0.0; 3]; W * H];
        for y in 0..H {
            for x in 0..W {
                let t = y as f32 / H as f32;
                // A deep plum face.
                let mut c = mix(rgb(46, 10, 48), rgb(14, 3, 20), t);
                let (bx0, bx1, by0, by1) = (2, W - 3, 2, H - 3);
                if (bx0..=bx1).contains(&x) && (by0..=by1).contains(&y) {
                    // Chrome: a bright band of sky up top, dark below.
                    let sky = 0.55 + 0.45 * (-((y as f32 - 9.0) / 7.0).powi(2)).exp() - 0.25 * t;
                    let mut m = scale(rgb(200, 196, 220), sky);
                    // Bevels: lit on the top and left edges of the plate,
                    // dark on the bottom and right.
                    if y == by0 || x == bx0 {
                        m = mix(m, rgb(255, 255, 255), 0.55);
                    } else if y == by1 || x == bx1 {
                        m = scale(m, 0.45);
                    }
                    // Round each window: shadow over it, light under it.
                    for &wx in &WIN_X {
                        let (l, r) = (wx as i32 - 1, (wx + WIN_W) as i32);
                        let (xi, yi) = (x as i32, y as i32);
                        let (top, bottom) = (WIN_Y as i32 - 1, (WIN_Y + WIN_H) as i32);
                        if (l..=r).contains(&xi) && (top..=bottom).contains(&yi) {
                            if yi == top || xi == l {
                                m = scale(m, 0.5);
                            } else if yi == bottom || xi == r {
                                m = mix(m, rgb(255, 255, 255), 0.4);
                            }
                        }
                    }
                    // A pink cast from the face round it.
                    c = mix(m, rgb(255, 120, 220), 0.08);
                }
                px[y * W + x] = c;
            }
        }
        // The payline's jewels, pointing in from each side.
        for (i, &(dx, w)) in [(0usize, 1usize), (0, 2), (0, 3), (0, 3), (0, 2), (0, 1)].iter().enumerate() {
            let y = 19 + i;
            for k in 0..w {
                let shade = if i < 3 { 1.0 } else { 0.7 };
                let c = scale(if k == 0 && i == 2 { rgb(255, 160, 160) } else { rgb(230, 20, 40) }, shade);
                px[y * W + dx + k] = c;
                px[y * W + W - 1 - dx - k] = c;
            }
        }
        px
    })
}

impl Canvas {
    pub fn new() -> Canvas {
        Canvas { px: vec![[0.0; 3]; W * H] }
    }

    pub fn draw(&mut self, s: &Scene) {
        self.px.clone_from(backdrop());
        for (i, r) in s.reels.iter().enumerate() {
            self.reel(i, r, s);
        }
        self.bulbs(s);
        for c in &s.coins {
            self.coin(c);
        }
    }

    fn reel(&mut self, i: usize, r: &ReelView, s: &Scene) {
        let radius = WIN_H as f32 / 2.0 / VIEW.sin();
        let strip = &STRIPS[i];
        let ivory = mix(rgb(247, 241, 224), rgb(255, 215, 120), r.glow);
        // Motion blur: what the reel passes in a 25th of a second.
        let blur = r.speed / 25.0;
        let taps = ((blur.abs() / 1.5).ceil() as usize).clamp(1, 10);
        for y in 0..WIN_H {
            let yc = y as f32 + 0.5 - WIN_H as f32 / 2.0;
            let theta = (yc / radius).clamp(-1.0, 1.0).asin();
            // Lit from above the window, falling off round the reel.
            let light = 0.16 + 0.95 * (theta + 0.12).cos().max(0.0).powf(2.2);
            let shine = 0.12 * (-((yc + 9.0) / 2.6).powi(2)).exp();
            for x in 0..WIN_W {
                let mut acc = [0.0f32; 3];
                let n = taps * 2;
                for k in 0..n {
                    // Two samples down each pixel, along the blur.
                    let sy = yc + if k % 2 == 0 { -0.25 } else { 0.25 };
                    let arc = radius * (sy / radius).clamp(-1.0, 1.0).asin();
                    let pos = r.pos - blur * ((k / 2) as f32 / taps as f32);
                    let q = (arc - pos).rem_euclid(CIRCLE);
                    let slot = ((q + PITCH / 2.0) / PITCH).floor();
                    let sym = strip[(slot as usize) % STOPS];
                    let within = q - slot * PITCH + PITCH / 2.0 - (PITCH - ART_H) / 2.0;
                    let ax = x as i32 - ((WIN_W - ART_W) / 2) as i32;
                    let c = if (0.0..ART_H).contains(&within) && (0..ART_W as i32).contains(&ax) {
                        let a = art_at(sym, ax as usize, within);
                        add([a[0], a[1], a[2]], ivory, 1.0 - a[3])
                    } else {
                        ivory
                    };
                    acc = add(acc, c, 1.0);
                }
                let mut c = scale(acc, 1.0 / n as f32);
                // The bezel's shadow in from each side.
                let side = 1.0 - 0.35 * (-(x as f32) / 1.4).exp() - 0.35 * (-((WIN_W - 1 - x) as f32) / 1.4).exp();
                c = scale(c, light * side * (1.0 - 0.5 * r.dim));
                c = add(c, rgb(255, 255, 255), shine);
                // A streak of reflection on the glass, and the glint.
                let d = x as f32 + 0.55 * y as f32;
                if (25.0..27.5).contains(&d) {
                    c = mix(c, rgb(255, 255, 255), 0.09);
                }
                if s.glint >= 0.0 {
                    let g = (d + WIN_X[i] as f32 - s.glint).abs();
                    c = mix(c, rgb(255, 255, 255), 0.45 * (-(g / 1.3).powi(2)).exp());
                }
                // The payline, a red line on the glass.
                if (y == WIN_H / 2 - 1 || y == WIN_H / 2) && s.payline > 0.0 {
                    c = mix(c, rgb(255, 30, 50), 0.45 * s.payline);
                }
                self.px[(WIN_Y + y) * W + WIN_X[i] + x] = c;
            }
        }
        // Spinning fast it's a blur anyway: a color a cell, in fewer
        // shades, is all a slow line needs to carry.
        if r.speed.abs() > 250.0 {
            for y in (0..WIN_H).step_by(2) {
                for x in 0..WIN_W {
                    let (a, b) = ((WIN_Y + y) * W + WIN_X[i] + x, (WIN_Y + y + 1) * W + WIN_X[i] + x);
                    let c = scale(add(self.px[a], self.px[b], 1.0), 0.5).map(|v| (v / 20.0).round() * 20.0);
                    self.px[a] = c;
                    self.px[b] = c;
                }
            }
        }
    }

    fn bulbs(&mut self, s: &Scene) {
        for (n, &(bx, by)) in bulb_spots().iter().enumerate() {
            let on = s.bulbs.get(n).copied().unwrap_or(false);
            if on {
                // A glow round it, on whatever is there.
                for y in by.saturating_sub(1)..(by + 3).min(H) {
                    for x in bx.saturating_sub(1)..(bx + 3).min(W) {
                        if in_window(x, y).is_some() {
                            continue;
                        }
                        let d = ((x as f32 - bx as f32 - 0.5).powi(2) + (y as f32 - by as f32 - 0.5).powi(2)).sqrt();
                        let p = &mut self.px[y * W + x];
                        *p = mix(*p, rgb(255, 180, 80), 0.8 * (-(d / 1.3).powi(2)).exp());
                    }
                }
            }
            for (k, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                let c = match (on, k) {
                    (true, 0) => rgb(255, 255, 240),
                    (true, 3) => rgb(255, 190, 80),
                    (true, _) => rgb(255, 228, 150),
                    (false, 0) => rgb(110, 80, 60),
                    (false, 3) => rgb(45, 25, 20),
                    (false, _) => rgb(75, 50, 35),
                };
                self.px[(by + dy) * W + bx + dx] = c;
            }
        }
    }

    fn coin(&mut self, c: &Coin) {
        let (rx, ry) = (3.3 * c.spin.cos().abs() + 0.4, 3.3);
        for y in (c.y - ry).floor() as i32..=(c.y + ry).ceil() as i32 {
            for x in (c.x - rx).floor() as i32..=(c.x + rx).ceil() as i32 {
                if !(0..W as i32).contains(&x) || !(0..H as i32).contains(&y) {
                    continue;
                }
                let (dx, dy) = ((x as f32 + 0.5 - c.x) / rx, (y as f32 + 0.5 - c.y) / ry);
                let d = dx * dx + dy * dy;
                if d > 1.0 {
                    continue;
                }
                let mut col = gold(dx * 0.9 - dy * 0.2);
                if d > 0.72 {
                    col = mix(col, rgb(120, 70, 0), 0.6);
                } else if (0.2..0.34).contains(&d) {
                    col = scale(col, 0.82);
                }
                if c.spin.cos().abs() < 0.25 {
                    col = mix(rgb(230, 170, 40), rgb(140, 85, 5), dy * 0.5 + 0.5);
                }
                self.px[y as usize * W + x as usize] = col;
            }
        }
    }
}

/// The payline symbol at `pos` (what a stopped reel shows).
#[cfg(test)]
pub fn symbol_at(reel: usize, pos: f32) -> usize {
    let q = (-pos).rem_euclid(CIRCLE);
    STRIPS[reel][((q + PITCH / 2.0) / PITCH).floor() as usize % STOPS]
}

/// Where a reel stands to show stop `k` on the payline.
pub fn pos_of(k: usize) -> f32 {
    (-(k as f32) * PITCH).rem_euclid(CIRCLE)
}

// ---- output ------------------------------------------------------------

/// A cell as sent: its top and bottom colors (24-bit, or xterm indexes).
pub type Cell = (u32, u32);

pub struct Encoder {
    pub truecolor: bool,
    prev: Vec<Option<Cell>>,
}

impl Encoder {
    pub fn new(truecolor: bool) -> Encoder {
        Encoder { truecolor, prev: vec![None; W * H / 2] }
    }

    /// Everything is drawn again next frame.
    pub fn forget(&mut self) {
        self.prev.iter_mut().for_each(|c| *c = None);
    }

    fn code(&self, c: Rgb) -> u32 {
        let q = |v: f32| (v.round().clamp(0.0, 255.0) as u32 + 2) & !3;
        let (r, g, b) = (q(c[0]).min(255), q(c[1]).min(255), q(c[2]).min(255));
        if self.truecolor {
            r << 16 | g << 8 | b
        } else {
            crate::picture::xterm256([r as u8, g as u8, b as u8]) as u32
        }
    }

    fn sgr(&self, out: &mut Vec<u8>, fore: bool, c: u32) {
        let k = if fore { 38 } else { 48 };
        let s = if self.truecolor { format!("{k};2;{};{};{}", c >> 16, (c >> 8) & 255, c & 255) } else { format!("{k};5;{c}") };
        out.extend(s.as_bytes());
    }

    /// The ANSI for the cells that changed since the last frame, the
    /// canvas's top row at screen row `top`.
    pub fn frame(&mut self, cv: &Canvas, top: u16) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut fg, mut bg): (Option<u32>, Option<u32>) = (None, None);
        for row in 0..H / 2 {
            let mut at: Option<usize> = None;
            for x in 0..W {
                let cell = (self.code(cv.px[row * 2 * W + x]), self.code(cv.px[(row * 2 + 1) * W + x]));
                let i = row * W + x;
                if self.prev[i] == Some(cell) {
                    continue;
                }
                self.prev[i] = Some(cell);
                match at {
                    Some(c) if c == x => {}
                    Some(c) if x - c <= 4 => out.extend(format!("\x1b[{}C", x - c).as_bytes()),
                    _ => out.extend(format!("\x1b[{};{}H", top as usize + row, x + 1).as_bytes()),
                }
                let (up, down) = cell;
                // The glyph that needs the fewest changes: a space or a full
                // block for one color, else a half block either way up.
                let mut sets: Vec<(bool, u32)> = Vec::new();
                let glyph = if up == down {
                    if bg == Some(up) {
                        b' '
                    } else if fg == Some(up) {
                        0xDB
                    } else {
                        sets.push((false, up));
                        b' '
                    }
                } else if fg == Some(up) && bg == Some(down) {
                    0xDF
                } else if bg == Some(up) && fg == Some(down) {
                    0xDC
                } else if fg == Some(up) || bg == Some(down) {
                    if fg != Some(up) {
                        sets.push((true, up));
                    }
                    if bg != Some(down) {
                        sets.push((false, down));
                    }
                    0xDF
                } else {
                    if bg != Some(up) {
                        sets.push((false, up));
                    }
                    if fg != Some(down) {
                        sets.push((true, down));
                    }
                    0xDC
                };
                if !sets.is_empty() {
                    out.extend(b"\x1b[");
                    for (n, &(fore, c)) in sets.iter().enumerate() {
                        if n > 0 {
                            out.push(b';');
                        }
                        self.sgr(&mut out, fore, c);
                        if fore { fg = Some(c) } else { bg = Some(c) }
                    }
                    out.push(b'm');
                }
                out.push(glyph);
                at = Some(x + 1);
            }
        }
        if !out.is_empty() {
            out.extend(b"\x1b[0m");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_land_on_their_symbols() {
        for (reel, strip) in STRIPS.iter().enumerate() {
            for (k, &sym) in strip.iter().enumerate() {
                assert_eq!(symbol_at(reel, pos_of(k)), sym);
                assert_eq!(symbol_at(reel, pos_of(k) + CIRCLE * 3.0), sym);
            }
            for s in 0..SYMBOLS {
                assert!(strip.contains(&s), "every symbol on reel {reel}");
            }
        }
    }

    #[test]
    fn frames_send_only_what_changed() {
        let mut cv = Canvas::new();
        let mut scene = Scene { payline: 0.5, ..Default::default() };
        cv.draw(&scene);
        let mut enc = Encoder::new(true);
        let first = enc.frame(&cv, 1);
        assert!(!first.is_empty());
        assert!(enc.frame(&cv, 1).is_empty(), "nothing changed");
        scene.reels[1].pos = 7.0;
        cv.draw(&scene);
        let second = enc.frame(&cv, 1);
        assert!(!second.is_empty() && second.len() < first.len() / 2, "{} vs {}", second.len(), first.len());
    }

    /// Pictures to look at: `SLOTS_OUT=dir cargo test slot_pictures --
    /// --ignored` writes ANSI files there.
    #[test]
    #[ignore]
    fn slot_pictures() {
        let dir = std::path::PathBuf::from(std::env::var("SLOTS_OUT").expect("SLOTS_OUT"));
        let spots = bulb_spots().len();
        let shots: Vec<(&str, Scene)> = vec![
            ("still", Scene {
                reels: [ReelView { pos: pos_of(0), ..Default::default() }, ReelView { pos: pos_of(1), ..Default::default() }, ReelView { pos: pos_of(6), ..Default::default() }],
                bulbs: (0..spots).map(|i| i % 3 == 0).collect(),
                glint: -1.0,
                payline: 0.3,
                coins: vec![],
            }),
            ("bars", Scene {
                reels: [ReelView { pos: pos_of(3), ..Default::default() }, ReelView { pos: pos_of(0), ..Default::default() }, ReelView { pos: pos_of(2), ..Default::default() }],
                bulbs: (0..spots).map(|i| i % 3 == 2).collect(),
                glint: -1.0,
                payline: 0.0,
                coins: vec![],
            }),
            ("between", Scene {
                reels: [ReelView { pos: pos_of(3) + 9.0, ..Default::default() }, ReelView { pos: pos_of(4) - 6.0, ..Default::default() }, ReelView { pos: pos_of(11), ..Default::default() }],
                bulbs: (0..spots).map(|i| i % 3 == 1).collect(),
                glint: 30.0,
                payline: 0.3,
                coins: vec![],
            }),
            ("spinning", Scene {
                reels: [ReelView { pos: 100.0, speed: 770.0, ..Default::default() }, ReelView { pos: 210.0, speed: 300.0, ..Default::default() }, ReelView { pos: pos_of(2), speed: 60.0, ..Default::default() }],
                bulbs: (0..spots).map(|i| i % 2 == 0).collect(),
                glint: -1.0,
                payline: 0.3,
                coins: vec![],
            }),
            ("jackpot", Scene {
                reels: [ReelView { pos: pos_of(0), glow: 0.8, ..Default::default() }, ReelView { pos: pos_of(3), glow: 0.8, ..Default::default() }, ReelView { pos: pos_of(6), glow: 0.8, ..Default::default() }],
                bulbs: vec![true; spots],
                glint: -1.0,
                payline: 1.0,
                coins: (0..12).map(|i| Coin { x: 6.0 + i as f32 * 6.3, y: 6.0 + (i * 7 % 30) as f32, vx: 0.0, vy: 0.0, spin: i as f32 * 0.7 }).collect(),
            }),
        ];
        for (name, scene) in shots {
            let mut cv = Canvas::new();
            cv.draw(&scene);
            let mut enc = Encoder::new(true);
            let mut out = b"\x1b[2J".to_vec();
            out.extend(enc.frame(&cv, 1));
            std::fs::write(dir.join(format!("{name}.ans")), &out).unwrap();
            eprintln!("{name}: {} bytes", out.len());
            let mut out = b"\x1b[2J".to_vec();
            out.extend(Encoder::new(false).frame(&cv, 1));
            std::fs::write(dir.join(format!("{name}-256.ans")), &out).unwrap();
        }
    }
}
