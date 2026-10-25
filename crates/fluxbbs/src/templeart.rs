//! New: the temple's pictures (`temple`): a chamber seen from where the
//! caller stands, by the light of their torch, in half-block pixels.
//! The walls that are open are the ways on; what the chamber is (a hall,
//! a crypt, a flooded one...) dresses it, and what's in it (a creature,
//! a chest, gold, a grave, writing on the wall) is drawn in.

use crate::pixels::{Canvas, Rgb, add, hash, mix, rgb, scale, smooth};

/// The picture: the whole width, rows 1 to 13.
pub const W: usize = 80;
pub const H: usize = 26;

/// Where the far wall is: its middle and half its size.
const CX: f32 = 40.0;
const CY: f32 = 11.0;
const HW: f32 = 17.0;
const HH: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Entrance,
    Hall,
    Crypt,
    Flooded,
    Shrine,
    Cavern,
    Chasm,
    Treasury,
    Stairs,
}

impl Kind {
    pub const ALL: [Kind; 9] =
        [Kind::Entrance, Kind::Hall, Kind::Crypt, Kind::Flooded, Kind::Shrine, Kind::Cavern, Kind::Chasm, Kind::Treasury, Kind::Stairs];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Entrance => "entrance",
            Kind::Hall => "hall",
            Kind::Crypt => "crypt",
            Kind::Flooded => "flooded",
            Kind::Shrine => "shrine",
            Kind::Cavern => "cavern",
            Kind::Chasm => "chasm",
            Kind::Treasury => "treasury",
            Kind::Stairs => "stairs",
        }
    }

    pub fn from_name(s: &str) -> Kind {
        Kind::ALL.into_iter().find(|k| k.name() == s).unwrap_or(Kind::Hall)
    }

    /// Torches in sconces on the walls.
    fn sconces(self) -> bool {
        matches!(self, Kind::Hall | Kind::Crypt | Kind::Shrine | Kind::Stairs | Kind::Treasury)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Beast {
    Skeleton,
    Serpent,
    Spider,
    Bats,
    Cultist,
    Wraith,
    Golem,
    Scarab,
}

impl Beast {
    pub const ALL: [Beast; 8] =
        [Beast::Skeleton, Beast::Serpent, Beast::Spider, Beast::Bats, Beast::Cultist, Beast::Wraith, Beast::Golem, Beast::Scarab];

    pub fn name(self) -> &'static str {
        match self {
            Beast::Skeleton => "skeleton",
            Beast::Serpent => "serpent",
            Beast::Spider => "spider",
            Beast::Bats => "bats",
            Beast::Cultist => "cultist",
            Beast::Wraith => "wraith",
            Beast::Golem => "golem",
            Beast::Scarab => "scarab",
        }
    }

    pub fn from_name(s: &str) -> Option<Beast> {
        Beast::ALL.into_iter().find(|k| k.name() == s)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Exits {
    pub north: bool,
    pub east: bool,
    pub south: bool,
    pub west: bool,
    pub up: bool,
    pub down: bool,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub kind: Kind,
    pub depth: u32,
    pub exits: Exits,
    pub beast: Option<Beast>,
    pub chest: bool,
    pub gold: bool,
    pub grave: bool,
    pub marks: bool,
    /// Steady variety: where the cracks and stones fall.
    pub seed: u32,
}

/// The stone, by how deep: sandstone, then gray, moss, slate, obsidian.
fn stone(depth: u32) -> Rgb {
    match depth {
        0 | 1 => rgb(150, 118, 78),
        2 => rgb(118, 108, 96),
        3 => rgb(88, 106, 76),
        4 => rgb(72, 80, 104),
        _ => rgb(92, 52, 48),
    }
}

/// What a point on the picture is.
enum Place {
    /// The far wall: across and down, -1 to 1.
    Back(f32, f32),
    /// A side wall: how far in (1 the far wall, more nearer), and down
    /// it, -1 to 1.
    Side(bool, f32, f32),
    /// The floor or ceiling: across, -1 to 1, and how far in.
    Floor(f32, f32),
    Ceiling(f32, f32),
}

fn place(x: f32, y: f32, rough: f32) -> Place {
    let (dx, dy) = (x - CX, y - CY);
    let (ax, ay) = (dx.abs() / (HW + rough), dy.abs() / (HH + rough * 0.5));
    let s = ax.max(ay);
    if s <= 1.0 {
        return Place::Back(dx / HW, dy / HH);
    }
    if ax >= ay {
        Place::Side(dx < 0.0, s, dy / (HH * s))
    } else if dy > 0.0 {
        Place::Floor(dx / (HW * s), s)
    } else {
        Place::Ceiling(dx / (HW * s), s)
    }
}

/// Where a point on the floor (across, how far in) is on the picture.
fn floor_at(u: f32, s: f32) -> (f32, f32) {
    (CX + u * HW * s, CY + HH * s)
}

/// Torchlight: brighter nearer (`s`, how far in: 1 at the far wall).
fn light(s: f32, flicker: f32) -> f32 {
    let d = 1.0 / s;
    (1.25 / (1.0 + (d * 1.45).powi(2)) + 0.04) * flicker
}

/// Bricks: `u` along, `v` down, with a mortar line between; a shade.
fn bricks(u: f32, v: f32, rows: f32, cols: f32, seed: u32) -> f32 {
    let r = (v * rows).floor();
    let off = if r as i32 % 2 == 0 { 0.0 } else { 0.5 };
    let c = (u * cols + off).floor();
    let (fu, fv) = ((u * cols + off).fract(), (v * rows).fract());
    let mortar = fu < 0.08 || fv < 0.14;
    let h = hash(c as i32 + seed as i32 * 7, r as i32);
    if mortar { 0.55 } else { 0.85 + h * 0.25 }
}

pub fn draw(cv: &mut Canvas, sc: &Scene, t: f32) {
    let base = stone(sc.depth);
    let flick = 0.94 + 0.06 * (t * 9.0).sin() * (t * 5.3).cos();
    let cave = sc.kind == Kind::Cavern;
    let seed = sc.seed;
    for y in 0..H {
        for x in 0..W {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let rough = if cave { (hash(x as i32 / 3 + seed as i32, y as i32 / 2) - 0.5) * 3.0 } else { 0.0 };
            let c = match place(fx, fy, rough) {
                Place::Back(u, v) => back_wall(sc, base, u, v, t) * light(1.0, flick),
                Place::Side(left, s, v) => side_wall(sc, base, left, s, v, t) * light(s, flick),
                Place::Floor(u, s) => floor(sc, base, u, s, t, x as i32, y as i32) * light(s, flick),
                Place::Ceiling(u, s) => ceiling(sc, base, u, s) * light(s, flick),
            };
            cv.set(x as i32, y as i32, c.color);
            if c.glow > 0.0 {
                cv.set(x as i32, y as i32, add(cv.get(x as i32, y as i32).unwrap_or(c.color), c.glow_color, c.glow));
            }
        }
    }
    dressing(cv, sc, t, flick);
    if sc.marks {
        scratches(cv, sc);
    }
    if sc.grave {
        grave(cv, sc.seed);
    }
    if sc.chest {
        chest(cv, light(1.6, flick));
    }
    if sc.gold {
        gold_heap(cv, t, light(1.7, flick));
    }
    if sc.exits.up {
        shaft(cv, sc, t);
    }
    if sc.kind.sconces() {
        for left in [true, false] {
            sconce(cv, left, t);
        }
    }
    if let Some(b) = sc.beast {
        beast(cv, b, t, light(1.75, flick));
    }
}

/// A pixel before the torchlight, and light of its own (flames, daylight).
#[derive(Clone, Copy)]
struct Px {
    color: Rgb,
    glow: f32,
    glow_color: Rgb,
}

impl std::ops::Mul<f32> for Px {
    type Output = Px;
    fn mul(self, k: f32) -> Px {
        Px { color: scale(self.color, k), ..self }
    }
}

fn px(c: Rgb) -> Px {
    Px { color: c, glow: 0.0, glow_color: [0.0; 3] }
}

/// The dark of a way on.
fn passage(depth_into: f32) -> Px {
    px(scale(rgb(30, 22, 20), (1.0 - depth_into).clamp(0.05, 1.0)))
}

fn back_wall(sc: &Scene, base: Rgb, u: f32, v: f32, t: f32) -> Px {
    // The way north: a dark doorway with a lintel.
    let door_at = if sc.kind == Kind::Shrine { 0.62 } else { 0.0 };
    if sc.exits.north {
        let du = (u - door_at).abs();
        if du < 0.22 && v > -0.4 {
            return passage(0.55 + (0.22 - du) * 2.0);
        }
        if du < 0.3 && v > -0.55 {
            return px(scale(base, 0.75 + 0.1 * hash((u * 20.0) as i32, 3)));
        }
    }
    let shade = match sc.kind {
        Kind::Cavern => 0.7 + 0.35 * hash((u * 22.0) as i32, (v * 9.0) as i32),
        _ => bricks(u * 0.5 + 0.5, v * 0.5 + 0.5, 5.0, 7.0, sc.seed),
    };
    let mut c = scale(base, shade);
    match sc.kind {
        Kind::Shrine => {
            // A sunburst carved behind the idol.
            let (du, dv) = (u * 2.6, v + 0.15);
            let a = dv.atan2(du);
            let r = (du * du + dv * dv).sqrt();
            if r < 0.95 && (a * 6.0).sin() > 0.55 {
                c = scale(mix(base, rgb(200, 160, 70), 0.35), 1.1);
            }
        }
        Kind::Crypt => {
            // Niches with skulls in them.
            let n = ((u + 1.0) * 3.0).floor();
            let fu = ((u + 1.0) * 3.0).fract();
            if (0.25..0.75).contains(&fu) && (-0.6..0.0).contains(&v) && !(sc.exits.north && n as i32 == 3) {
                c = scale(base, 0.25);
                let (su, sv) = ((fu - 0.5) * 6.0, (v + 0.2) * 5.0);
                if su * su + sv * sv < 1.2 {
                    c = rgb(196, 186, 160);
                    if sv < 0.1 && (su.abs() - 0.45).abs() < 0.3 && sv > -0.6 {
                        c = rgb(40, 10, 8);
                    }
                }
            }
        }
        Kind::Treasury => {
            let glint = hash((u * 40.0) as i32, (v * 12.0) as i32);
            if glint > 0.985 && (t * 3.0 + glint * 50.0).sin() > 0.6 {
                c = rgb(255, 230, 150);
            }
        }
        _ => {}
    }
    px(c)
}

fn side_wall(sc: &Scene, base: Rgb, left: bool, s: f32, v: f32, _t: f32) -> Px {
    let open = if left { sc.exits.west } else { sc.exits.east };
    // A doorway part way in.
    if open && (1.3..1.75).contains(&s) && v > -0.45 {
        return passage(0.6);
    }
    if open && (1.22..1.83).contains(&s) && v > -0.58 {
        return px(scale(base, 0.7));
    }
    let shade = match sc.kind {
        Kind::Cavern => 0.65 + 0.4 * hash((s * 9.0) as i32 + left as i32 * 50, (v * 8.0) as i32),
        _ => bricks(1.0 / s * 3.0, v * 0.5 + 0.5, 5.0, 2.2, sc.seed + left as u32),
    };
    let mut c = scale(base, shade * 0.85);
    if sc.kind == Kind::Crypt {
        let fs = (s * 2.5).fract();
        if (0.3..0.7).contains(&fs) && (-0.55..-0.05).contains(&v) && s < 2.3 {
            c = scale(base, 0.22);
            let (su, sv) = ((fs - 0.5) * 5.0, (v + 0.25) * 5.0);
            if su * su + sv * sv < 1.0 {
                c = rgb(190, 180, 154);
                if (-0.6..0.15).contains(&sv) && (su.abs() - 0.4).abs() < 0.28 {
                    c = rgb(40, 10, 8);
                }
            }
        }
    }
    px(c)
}

fn floor(sc: &Scene, base: Rgb, u: f32, s: f32, t: f32, x: i32, y: i32) -> Px {
    // The way down: a stairwell in the floor.
    let (hu, s0, s1) = if sc.kind == Kind::Stairs { (0.42, 1.08, 1.6) } else { (0.3, 1.12, 1.45) };
    if sc.exits.down && u.abs() < hu && (s0..s1).contains(&s) {
        let k = (s - s0) / (s1 - s0);
        let step = ((k * 5.0).fract() < 0.22) as i32 as f32;
        return px(scale(rgb(70, 58, 46), (0.15 + 0.55 * k) * (1.0 - 0.35 * step)));
    }
    match sc.kind {
        Kind::Flooded => {
            let ripple = ((u * 9.0 + t * 1.3).sin() * (s * 14.0 - t * 2.1).sin()).max(0.0);
            let c = mix(rgb(18, 48, 52), rgb(60, 110, 110), ripple * 0.5);
            let shine = if ripple > 0.85 { 0.4 } else { 0.0 };
            return Px { color: c, glow: shine, glow_color: rgb(120, 110, 70) };
        }
        Kind::Chasm if (1.25..1.85).contains(&s) => {
            if u.abs() < 0.1 {
                let plank = ((s * 18.0).fract() < 0.25) as i32 as f32;
                return px(scale(rgb(110, 78, 46), 1.0 - 0.4 * plank));
            }
            // Black but for the lip on either side.
            let lip = !(1.3..1.8).contains(&s);
            return px(if lip { scale(base, 1.1) } else { rgb(4, 3, 3) });
        }
        _ => {}
    }
    let shade = match sc.kind {
        Kind::Cavern => 0.55 + 0.35 * hash(x / 2, y),
        _ => {
            let (tu, tv) = ((u * 4.0).fract(), (s * 3.0).fract());
            if tu < 0.06 || tv < 0.1 { 0.5 } else { 0.72 + 0.15 * hash((u * 4.0) as i32, (s * 3.0) as i32) }
        }
    };
    px(scale(base, shade * 0.8))
}

fn ceiling(sc: &Scene, base: Rgb, u: f32, s: f32) -> Px {
    let k = if sc.kind == Kind::Cavern { 0.4 + 0.2 * hash((u * 10.0) as i32, (s * 6.0) as i32) } else { 0.42 };
    let beam = sc.kind != Kind::Cavern && (s * 2.0).fract() < 0.12;
    px(scale(base, if beam { k * 0.6 } else { k }))
}

/// What each kind of chamber has in it.
fn dressing(cv: &mut Canvas, sc: &Scene, t: f32, flick: f32) {
    match sc.kind {
        Kind::Hall | Kind::Stairs | Kind::Treasury => {
            for &s in &[1.35, 2.05] {
                for &u in &[-0.82, 0.82] {
                    pillar(cv, u, s, stone(sc.depth), light(s, flick));
                }
            }
        }
        Kind::Crypt => sarcophagus(cv, stone(sc.depth), light(1.25, flick)),
        Kind::Shrine => idol(cv, t, light(1.05, flick)),
        Kind::Cavern => stalactites(cv, sc.seed, stone(sc.depth)),
        Kind::Entrance => {
            for &u in &[-0.8, 0.8] {
                pillar(cv, u, 1.5, stone(sc.depth), light(1.5, flick));
            }
            bones(cv, sc.seed);
        }
        _ => {}
    }
    if sc.kind == Kind::Treasury {
        for (u, s) in [(-0.55, 1.5), (0.6, 1.3), (-0.2, 1.15)] {
            let (x, y) = floor_at(u, s);
            heap(cv, x, y, s * 2.6, t);
        }
    }
}

fn pillar(cv: &mut Canvas, u: f32, s: f32, base: Rgb, lit: f32) {
    let (x, bottom) = floor_at(u, s);
    let top = CY - HH * s;
    let half = 1.3 * s;
    for y in top.max(0.0) as i32..bottom.min(H as f32) as i32 {
        for xx in (x - half) as i32..=(x + half) as i32 {
            let k = (xx as f32 + 0.5 - x) / half;
            if k.abs() > 1.0 {
                continue;
            }
            let round = 1.0 - k * k * 0.5 + if k < -0.3 { 0.15 } else { 0.0 };
            let band = if (y as f32 - top) < 1.5 * s || (bottom - y as f32) < 1.5 * s { 1.12 } else { 1.0 };
            cv.set(xx, y, scale(base, round * band * lit * 1.1));
        }
    }
}

fn sarcophagus(cv: &mut Canvas, base: Rgb, lit: f32) {
    let (x, y) = floor_at(0.0, 1.25);
    for yy in (y - 5.0) as i32..y as i32 {
        for xx in (x - 9.0) as i32..(x + 9.0) as i32 {
            let lid = yy < (y - 3.0) as i32;
            let c = if lid { scale(base, 1.15) } else { scale(base, 0.8 + 0.08 * hash(xx, yy)) };
            cv.set(xx, yy, scale(c, lit));
        }
    }
    for xx in (x - 6.0) as i32..(x + 6.0) as i32 {
        cv.set(xx, (y - 2.0) as i32, scale(base, 0.5 * lit));
    }
}

const IDOL: [&str; 15] = [
    "....ggggg....",
    "...gGGGGGg...",
    "..gGGGGGGGg..",
    "..gGRGGGRGg..",
    "..gGGGgGGGg..",
    "...gGGGGGg...",
    "....gGgGg....",
    "..ggGGGGGgg..",
    ".g.gGGGGGg.g.",
    "g..gGGGGGg..g",
    "...gGGGGGg...",
    "...gGGgGGg...",
    "...gg...gg...",
    ".aaaaaaaaaaa.",
    "aaaaaaaaaaaaa",
];

fn idol(cv: &mut Canvas, t: f32, lit: f32) {
    let (x0, y0) = (CX as i32 - 6, (CY + HH) as i32 - 14);
    for (y, row) in IDOL.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let c = match ch {
                'g' => scale(rgb(170, 120, 30), lit * 1.3),
                'G' => scale(rgb(235, 180, 60), lit * 1.4),
                'R' => rgb(255, 40, 30),
                'a' => scale(rgb(90, 80, 70), lit),
                _ => continue,
            };
            cv.set(x0 + x as i32, y0 + y as i32, c);
        }
    }
    // A glint that crosses it now and then.
    let g = (t * 0.7).fract();
    if g < 0.25 {
        let gx = x0 + 2 + (g * 4.0 * 9.0) as i32;
        cv.blend(gx, y0 + 2, rgb(255, 250, 220), 0.9);
        cv.blend(gx, y0 + 8, rgb(255, 250, 220), 0.6);
    }
    // Candles either side.
    for cx in [x0 - 4, x0 + 16] {
        cv.set(cx, y0 + 11, rgb(220, 210, 180));
        cv.set(cx, y0 + 12, rgb(200, 190, 160));
        let f = 0.7 + 0.3 * (t * 11.0 + cx as f32).sin();
        cv.set(cx, y0 + 10, scale(rgb(255, 190, 80), f));
    }
}

fn stalactites(cv: &mut Canvas, seed: u32, base: Rgb) {
    for i in 0..9 {
        let x = (hash(i, seed as i32) * 78.0) as i32 + 1;
        let len = 2 + (hash(i, seed as i32 + 1) * 5.0) as i32;
        for y in 0..len {
            let w = ((len - y) as f32 / len as f32 * 1.6) as i32;
            for dx in -w..=w {
                cv.set(x + dx, y, scale(base, 0.3 + 0.1 * (dx == -w) as i32 as f32));
            }
        }
    }
}

fn bones(cv: &mut Canvas, seed: u32) {
    // A pile of skulls by the wall.
    for i in 0..6 {
        let x = 8 + (i % 3) * 3 + (hash(i, seed as i32) * 2.0) as i32;
        let y = 22 - (i / 3) * 2;
        for (dx, dy) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)] {
            cv.set(x + dx, y + dy, rgb(176, 164, 138));
        }
        cv.set(x, y, rgb(40, 14, 10));
        cv.set(x + 2, y, rgb(40, 14, 10));
    }
}

/// Writing scratched into the left wall, near the caller.
fn scratches(cv: &mut Canvas, sc: &Scene) {
    for i in 0..12 {
        let x = 1 + (i % 4) * 2 + (hash(i, sc.seed as i32 + 9) * 1.5) as i32;
        let y = 8 + (i / 4) * 3 + (hash(i, 5) * 1.5) as i32;
        for k in 0..2 {
            cv.blend(x + k, y + k, rgb(230, 216, 186), 0.55);
        }
    }
}

fn grave(cv: &mut Canvas, seed: u32) {
    let x0 = 60 + (hash(1, seed as i32) * 4.0) as i32;
    for y in 17..23 {
        let w = if y < 18 { 2 } else { 3 };
        for x in x0 - w..=x0 + w {
            cv.set(x, y, scale(rgb(140, 136, 128), 0.6 + 0.1 * hash(x, y)));
        }
    }
    cv.set(x0, 18, rgb(60, 56, 50));
    cv.set(x0 - 1, 19, rgb(60, 56, 50));
    cv.set(x0, 19, rgb(60, 56, 50));
    cv.set(x0 + 1, 19, rgb(60, 56, 50));
    cv.set(x0, 20, rgb(60, 56, 50));
    for x in x0 - 5..=x0 + 5 {
        cv.set(x, 23, rgb(70, 56, 40));
    }
}

const CHEST: [&str; 6] = ["..bbbbbbb..", ".bwwwwwwwb.", "bbbbbybbbbb", "bwwwwywwwwb", "bwwwwwwwwwb", "bbbbbbbbbbb"];

fn chest(cv: &mut Canvas, lit: f32) {
    let (x0, y0) = (18, 18);
    for (y, row) in CHEST.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let c = match ch {
                'b' => rgb(70, 46, 26),
                'w' => rgb(140, 92, 50),
                'y' => rgb(230, 190, 90),
                _ => continue,
            };
            cv.set(x0 + x as i32, y0 + y as i32, scale(c, lit * 1.2));
        }
    }
}

fn heap(cv: &mut Canvas, x: f32, y: f32, r: f32, t: f32) {
    for yy in (y - r) as i32..y as i32 {
        for xx in (x - r * 1.6) as i32..=(x + r * 1.6) as i32 {
            let (du, dv) = ((xx as f32 + 0.5 - x) / (r * 1.6), (y - yy as f32 - 0.5) / r);
            if du * du + dv * dv > 1.0 {
                continue;
            }
            let h = hash(xx, yy);
            let mut c = mix(rgb(150, 100, 20), rgb(240, 196, 70), h * 0.7 + (1.0 - dv) * 0.3);
            if h > 0.93 && (t * 2.5 + h * 40.0).sin() > 0.5 {
                c = rgb(255, 245, 200);
            }
            cv.set(xx, yy, c);
        }
    }
}

fn gold_heap(cv: &mut Canvas, t: f32, lit: f32) {
    let _ = lit;
    heap(cv, 54.0, 23.0, 2.6, t);
}

/// Light down a shaft from above: the way up (daylight at the top).
fn shaft(cv: &mut Canvas, sc: &Scene, t: f32) {
    let day = sc.depth <= 1;
    let tint = if day { rgb(255, 236, 190) } else { rgb(150, 160, 190) };
    for y in 0..H as i32 {
        let w = 4.0 + y as f32 * 0.25;
        for x in (CX - 14.0) as i32..(CX + 14.0) as i32 {
            let off = x as f32 + 0.5 - (CX - 8.0) - y as f32 * 0.35;
            if off.abs() > w {
                continue;
            }
            let k = (1.0 - off.abs() / w) * (1.0 - y as f32 / H as f32 * 0.6) * if day { 0.38 } else { 0.18 };
            cv.blend(x, y, tint, k);
        }
    }
    // A rope down from the hole.
    for y in 0..18 {
        let x = (CX - 8.0 + y as f32 * 0.35 + (t * 1.2 + y as f32 * 0.3).sin() * 0.4) as i32;
        cv.set(x, y, if y % 2 == 0 { rgb(150, 120, 70) } else { rgb(120, 92, 52) });
    }
    if day {
        // Dust in the light.
        for i in 0..10 {
            let p = (t * 0.05 + hash(i, 7)).fract();
            let x = (CX - 8.0 + hash(i, 3) * 8.0 - 4.0 + p * 6.0) as i32;
            let y = (p * 24.0) as i32;
            cv.blend(x, y, rgb(255, 250, 230), 0.7);
        }
    }
}

fn sconce(cv: &mut Canvas, left: bool, t: f32) {
    let (s, v) = (1.6, -0.25);
    let dx = HW * s * if left { -1.0 } else { 1.0 };
    let (x, y) = ((CX + dx * 0.93) as i32, (CY + v * HH * s) as i32);
    cv.set(x, y + 1, rgb(60, 44, 30));
    cv.set(x, y + 2, rgb(60, 44, 30));
    let f = (t * 8.0 + left as i32 as f32 * 2.0).sin() * 0.5 + 0.5;
    cv.set(x, y, mix(rgb(255, 150, 40), rgb(255, 220, 120), f));
    cv.set(x, y - 1, scale(rgb(255, 120, 30), 0.6 + 0.4 * f));
    // The glow on the wall around it.
    for gy in y - 4..=y + 4 {
        for gx in x - 5..=x + 5 {
            let d = (((gx - x) * (gx - x) + (gy - y) * (gy - y) * 2) as f32).sqrt();
            if d > 0.5 && d < 5.5 {
                cv.blend(gx, gy, rgb(255, 160, 60), (1.0 - d / 5.5) * (0.25 + 0.08 * f));
            }
        }
    }
}

// ---- creatures --------------------------------------------------------------

/// Each creature by hand: a letter a color (see `beast_color`).
fn sprite(b: Beast, t: f32) -> &'static [&'static str] {
    let alt = (t * 2.0) as i32 % 2 == 1;
    match b {
        Beast::Skeleton => &[
            ".....www.....",
            "....wwwww....",
            "....wRwRw....",
            "....wwwww....",
            ".....wkw.....",
            "......w......",
            "...wwwwwww...",
            "..w.wwwww.w..",
            "..w.w.w.w.w..",
            ".w..wwwww..w.",
            ".k...www...k.",
            ".....w.w.....",
            ".....w.w.....",
            ".....w.w.....",
            "....ww.ww....",
        ],
        Beast::Serpent => {
            if alt {
                &[
                    ".......gggg..",
                    "......gGGRGg.",
                    "......gGGGGgr",
                    ".......gGGg..",
                    "........gGg..",
                    ".......gGg...",
                    "......gGg....",
                    ".....gGg.....",
                    "....gGGg.....",
                    "...gGGGGg....",
                    "..gGg..gGg...",
                    ".gGg....gGgg.",
                    ".gGGg..gGGGg.",
                    "..gGGGGGGGg..",
                ]
            } else {
                &[
                    "......gggg...",
                    ".....gGGRGg..",
                    ".....gGGGGg..",
                    "......gGGg...",
                    ".......gGg...",
                    "......gGg....",
                    ".....gGg.....",
                    ".....gGg.....",
                    "....gGGg.....",
                    "...gGGGGg....",
                    "..gGg..gGg...",
                    ".gGg....gGgg.",
                    ".gGGg..gGGGg.",
                    "..gGGGGGGGg..",
                ]
            }
        }
        Beast::Spider => {
            if alt {
                &[
                    ".............",
                    ".k.........k.",
                    "..k.......k..",
                    "...k.kkk.k...",
                    "kk..kKKKk..kk",
                    "..kkKKKKKkk..",
                    "....kRkRk....",
                    "..kkKKKKKkk..",
                    ".k..kKKKk..k.",
                    "k..k.kkk.k..k",
                    "..k.......k..",
                    ".k.........k.",
                ]
            } else {
                &[
                    ".............",
                    "..k.......k..",
                    "...k.....k...",
                    "....kkkkk....",
                    "kkk.kKKKk.kkk",
                    "...kKKKKKk...",
                    "....kRkRk....",
                    "..kkKKKKKkk..",
                    ".k..kKKKk..k.",
                    "k...kkkkk...k",
                    "...k.....k...",
                    "..k.......k..",
                ]
            }
        }
        Beast::Bats => {
            if alt {
                &[
                    "k.k......k.k.",
                    ".kRk....kRk..",
                    "..k......k...",
                    ".....k.k.....",
                    "....kkRkk....",
                    ".....kkk.....",
                    "k.k......k.k.",
                    ".kRk....kRk..",
                    "..k......k...",
                ]
            } else {
                &[
                    ".............",
                    "kkRkk..kkRkk.",
                    "..k......k...",
                    ".............",
                    "...kk.R.kk...",
                    ".....kkk.....",
                    ".............",
                    "kkRkk..kkRkk.",
                    "..k......k...",
                ]
            }
        }
        Beast::Cultist => &[
            ".....rrr.....",
            "....rrrrr....",
            "...rrkkkrr...",
            "...rkYkYkr...",
            "...rrkkkrr...",
            "....rrrrr....",
            "...rrrrrrr...",
            "..rrrRrRrrr..",
            "..rrrrRrrrr..",
            "..srrrRrrrs..",
            "...rrrRrrr...",
            "...rrrrrrr...",
            "..rrrrrrrrr..",
            "..rrrrrrrrr..",
            ".rrrrrrrrrrr.",
        ],
        Beast::Wraith => {
            if alt {
                &[
                    "....ppppp....",
                    "...pPPPPPp...",
                    "...pBPPPBp...",
                    "...pPPPPPp...",
                    "....pPPPp....",
                    ".pp.pPPPp.pp.",
                    "p..ppPPPpp..p",
                    "....pPPPPp...",
                    "....pPPPPp...",
                    "...pPPpPPp...",
                    "...p.pP.pPp..",
                    "..p..p..p..p.",
                ]
            } else {
                &[
                    "....ppppp....",
                    "...pPPPPPp...",
                    "...pBPPPBp...",
                    "...pPPPPPp...",
                    "....pPPPp....",
                    "..ppPPPPPpp..",
                    ".p..pPPPp..p.",
                    "....pPPPPp...",
                    "...pPPPPPp...",
                    "...pPPpPPp...",
                    "..pP.pP.pPp..",
                    ".p..p..p...p.",
                ]
            }
        }
        Beast::Golem => &[
            "....ooooo....",
            "....oOOOo....",
            "....oCOCo....",
            "....ooooo....",
            ".oooooooooo..",
            "ooOOOOcOOOoo.",
            "ooOOOcccOOoo.",
            "ooOOOOcOOOoo.",
            "oo.oOOOOOo.oo",
            "oo.oOOOOOo.oo",
            "...ooo.ooo...",
            "...oOo.oOo...",
            "...oOo.oOo...",
            "..oooo.oooo..",
        ],
        Beast::Scarab => &[
            ".............",
            "...k.....k...",
            "....k...k....",
            ".....hhh.....",
            "....hRhRh....",
            "..kHHHHHHHk..",
            ".k.HHHhHHH.k.",
            "...HHHhHHH...",
            ".k.HHHhHHH.k.",
            "..k.HHhHH.k..",
            ".....HhH.....",
        ],
    }
}

fn beast_color(ch: char, t: f32) -> Option<(Rgb, bool)> {
    let pulse = 0.75 + 0.25 * (t * 3.0).sin();
    Some(match ch {
        'w' => (rgb(214, 206, 180), false),
        'k' => (rgb(34, 26, 30), false),
        'K' => (rgb(70, 52, 60), false),
        'R' => (scale(rgb(255, 40, 20), pulse), true),
        'Y' => (scale(rgb(255, 210, 60), pulse), true),
        'B' => (scale(rgb(140, 220, 255), pulse), true),
        'C' => (scale(rgb(120, 255, 200), pulse), true),
        'c' => (scale(rgb(60, 200, 150), pulse * 0.8), true),
        'g' => (rgb(40, 96, 40), false),
        'G' => (rgb(84, 150, 64), false),
        'r' => (rgb(96, 20, 26), false),
        's' => (rgb(200, 160, 130), false),
        'p' => (rgb(120, 140, 170), false),
        'P' => (rgb(190, 205, 230), false),
        'o' => (rgb(80, 76, 70), false),
        'O' => (rgb(130, 124, 112), false),
        'h' => (rgb(30, 80, 70), false),
        'H' => (rgb(60, 150, 120), false),
        _ => return None,
    })
}

fn beast(cv: &mut Canvas, b: Beast, t: f32, lit: f32) {
    let rows = sprite(b, t);
    let bob = if matches!(b, Beast::Wraith | Beast::Bats) { ((t * 2.2).sin() * 1.2) as i32 } else { 0 };
    let (x0, y0) = (CX as i32 - 6, (CY + HH * 1.75) as i32 - rows.len() as i32 + 1 + bob);
    let ghost = b == Beast::Wraith;
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let Some((c, own)) = beast_color(ch, t) else { continue };
            let c = if own { c } else { scale(c, lit * 1.25) };
            if ghost && !own {
                cv.blend(x0 + x as i32, y0 + y as i32, c, 0.7);
            } else {
                cv.set(x0 + x as i32, y0 + y as i32, c);
            }
        }
    }
    // A shadow under what walks.
    if !matches!(b, Beast::Wraith | Beast::Bats) {
        let y = y0 + rows.len() as i32;
        for x in x0..x0 + 13 {
            if let Some(p) = cv.get(x, y) {
                cv.set(x, y, scale(p, 0.6));
            }
        }
    }
}

/// The picture smoothed a touch at the far end, where torchlight gives out.
pub fn vignette(cv: &mut Canvas) {
    for y in 0..H {
        for x in 0..W {
            let (dx, dy) = ((x as f32 - CX) / 46.0, (y as f32 - CY) / 22.0);
            let k = 1.0 - smooth(0.7, 1.15, (dx * dx + dy * dy).sqrt()) * 0.5;
            let i = y * W + x;
            cv.px[i] = scale(cv.px[i], k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TEMPLE_OUT=dir cargo test --release -p fluxbbs temple_pictures --
    /// --ignored` writes ANSI files there.
    #[test]
    #[ignore]
    fn temple_pictures() {
        let dir = std::path::PathBuf::from(std::env::var("TEMPLE_OUT").expect("TEMPLE_OUT"));
        let all = Exits { north: true, east: true, south: true, west: true, up: false, down: false };
        let beasts = [None, Some(Beast::Skeleton), Some(Beast::Serpent), Some(Beast::Spider), Some(Beast::Bats), Some(Beast::Cultist), Some(Beast::Wraith), Some(Beast::Golem), Some(Beast::Scarab)];
        for (i, kind) in Kind::ALL.into_iter().enumerate() {
            let exits = match kind {
                Kind::Entrance => Exits { up: true, south: false, ..all },
                Kind::Stairs => Exits { down: true, ..all },
                Kind::Crypt => Exits { east: false, ..all },
                _ => all,
            };
            let sc = Scene {
                kind,
                depth: 1 + i as u32 % 5,
                exits,
                beast: beasts[i],
                chest: i % 3 == 0,
                gold: i % 2 == 0,
                grave: i % 4 == 1,
                marks: i % 3 == 1,
                seed: i as u32 * 31,
            };
            let mut cv = Canvas::new(W, H);
            draw(&mut cv, &sc, 2.0);
            vignette(&mut cv);
            let mut out = b"\x1b[2J".to_vec();
            out.extend(crate::pixels::Encoder::new(true, W, H).frame(&cv, 1));
            std::fs::write(dir.join(format!("{}.ans", kind.name())), &out).unwrap();
        }
    }
}
