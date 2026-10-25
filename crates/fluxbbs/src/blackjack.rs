//! New: blackjack in the casino (`AC blackjack`). A green felt table in
//! half-block pixels (`pixels`): cards slide out of the shoe and the hole
//! card turns over; chips stack in the betting circle and go to whoever
//! won them. Six decks (`[casino] decks`), cut three-quarters in; the
//! dealer peeks for blackjack and stands on all 17s (or hits a soft 17,
//! `hit_soft_17`); blackjack pays 3 to 2; double on any two cards; one
//! split, and split aces take one card each. No insurance or surrender.
//! Bets are the day's free chips (`casino.rs`).

use std::f32::consts::PI;
use std::io;
use std::sync::OnceLock;

use rand::seq::SliceRandom;

use crate::casino::{H, Ink, MSG_ROW, Purse, STATUS_ROW, Stage, W, ink, painted, shade};
use crate::config::Casino;
use crate::input::Key;
use crate::pixels::{Canvas, Rgb, hash, mix, rgb, scale, smooth};
use crate::session::Session;

// ---- the rules -------------------------------------------------------------

/// A card: rank 1 (ace) to 13 (king), suit 0 to 3 (spades, hearts,
/// diamonds, clubs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub rank: u8,
    pub suit: u8,
}

impl Card {
    fn value(self) -> u32 {
        (self.rank as u32).min(10)
    }

    pub fn red(self) -> bool {
        matches!(self.suit, 1 | 2)
    }

    pub fn name(self) -> String {
        let r = match self.rank {
            1 => "A".to_string(),
            11 => "J".into(),
            12 => "Q".into(),
            13 => "K".into(),
            n => n.to_string(),
        };
        format!("{r}{}", ["s", "h", "d", "c"][self.suit as usize])
    }
}

/// A hand's total, and whether an ace in it counts 11 (soft).
pub fn total(cards: &[Card]) -> (u32, bool) {
    let hard: u32 = cards.iter().map(|c| c.value()).sum();
    if cards.iter().any(|c| c.rank == 1) && hard + 10 <= 21 { (hard + 10, true) } else { (hard, false) }
}

fn natural(cards: &[Card]) -> bool {
    cards.len() == 2 && total(cards).0 == 21
}

fn dealer_draws(cards: &[Card], hit_soft_17: bool) -> bool {
    let (t, soft) = total(cards);
    t < 17 || (t == 17 && soft && hit_soft_17)
}

struct Shoe {
    cards: Vec<Card>,
    cut: usize,
}

impl Shoe {
    fn new(decks: u32) -> Shoe {
        let mut cards: Vec<Card> =
            (0..decks.max(1)).flat_map(|_| (0..4u8).flat_map(|suit| (1..=13u8).map(move |rank| Card { rank, suit }))).collect();
        cards.shuffle(&mut rand::rng());
        let cut = cards.len() / 4;
        Shoe { cards, cut }
    }

    fn draw(&mut self) -> Card {
        self.cards.pop().unwrap_or(Card { rank: 10, suit: 0 })
    }
}

/// How a hand came out, for its stake.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    Blackjack,
    Win,
    Push,
    Lose,
}

fn settle(hand: &[Card], dealer: &[Card], split: bool) -> Outcome {
    let (p, d) = (total(hand).0, total(dealer).0);
    let (pbj, dbj) = (natural(hand) && !split, natural(dealer));
    match () {
        _ if p > 21 => Outcome::Lose,
        _ if pbj && dbj => Outcome::Push,
        _ if pbj => Outcome::Blackjack,
        _ if dbj => Outcome::Lose,
        _ if d > 21 || p > d => Outcome::Win,
        _ if p == d => Outcome::Push,
        _ => Outcome::Lose,
    }
}

/// What comes back for a stake: blackjack 3 to 2, a win even money.
fn returned(o: Outcome, bet: u32) -> u32 {
    match o {
        Outcome::Blackjack => bet + bet * 3 / 2,
        Outcome::Win => bet * 2,
        Outcome::Push => bet,
        Outcome::Lose => 0,
    }
}

// ---- the art ---------------------------------------------------------------

pub const CARD_W: i32 = 17;
pub const CARD_H: i32 = 20;

/// Does the suit's shape cover (`u`, `v`), each 0 to 1 across its box?
fn suit_covers(suit: u8, u: f32, v: f32) -> bool {
    let circle = |cx: f32, cy: f32, r: f32| (u - cx).powi(2) + (v - cy).powi(2) <= r * r;
    // Inside the triangle a, b, c?
    let tri = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        let s = |p: (f32, f32), q: (f32, f32)| (q.0 - p.0) * (v - p.1) - (q.1 - p.1) * (u - p.0);
        let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
        !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
    };
    match suit {
        1 => circle(0.29, 0.32, 0.25) || circle(0.71, 0.32, 0.25) || tri((0.05, 0.42), (0.95, 0.42), (0.5, 0.97)),
        2 => (u - 0.5).abs() / 0.43 + (v - 0.5).abs() / 0.5 <= 1.0,
        0 => {
            circle(0.29, 0.6, 0.23) || circle(0.71, 0.6, 0.23) || tri((0.07, 0.52), (0.93, 0.52), (0.5, 0.02)) || tri((0.5, 0.6), (0.3, 1.0), (0.7, 1.0))
        }
        _ => {
            circle(0.5, 0.27, 0.22)
                || circle(0.26, 0.6, 0.22)
                || circle(0.74, 0.6, 0.22)
                || circle(0.5, 0.52, 0.14)
                || tri((0.5, 0.5), (0.3, 1.0), (0.7, 1.0))
        }
    }
}

/// Small suits, drawn by hand: too few pixels to smooth.
const SMALL_SUITS: [[u8; 5]; 4] = [
    [0x04, 0x0E, 0x1F, 0x1F, 0x04],
    [0x0A, 0x1F, 0x1F, 0x0E, 0x04],
    [0x04, 0x0E, 0x1F, 0x0E, 0x04],
    [0x04, 0x0E, 0x1F, 0x15, 0x04],
];

/// A suit `size` pixels square at (`x`, `y`) on a card's pixels.
fn suit_on(img: &mut [Option<Rgb>], x: i32, y: i32, size: i32, suit: u8, ink: Rgb) {
    if size <= 5 {
        for (py, bits) in SMALL_SUITS[suit as usize % 4].iter().enumerate() {
            for px in 0..5 {
                let (cx, cy) = (x + px, y + py as i32);
                if bits >> (4 - px) & 1 == 1 && (0..CARD_W).contains(&cx) && (0..CARD_H).contains(&cy) {
                    img[(cy * CARD_W + cx) as usize] = Some(ink);
                }
            }
        }
        return;
    }
    for py in 0..size {
        for px in 0..size {
            let mut hit = 0;
            for j in 0..4 {
                for i in 0..4 {
                    if suit_covers(suit, (px as f32 + (i as f32 + 0.5) / 4.0) / size as f32, (py as f32 + (j as f32 + 0.5) / 4.0) / size as f32) {
                        hit += 1;
                    }
                }
            }
            let (cx, cy) = (x + px, y + py);
            if hit > 0 && (0..CARD_W).contains(&cx) && (0..CARD_H).contains(&cy) {
                let i = (cy * CARD_W + cx) as usize;
                if let Some(p) = img[i] {
                    img[i] = Some(mix(p, ink, hit as f32 / 16.0));
                }
            }
        }
    }
}

/// The court cards' portraits, 11 by 13: `R` is the suit's color.
const KING: [&str; 13] = [
    "..y.y.y.y..",
    "..yyyyyyy..",
    "..yRyyyRy..",
    "..sssssss..",
    "..sksssks..",
    "..sssSsss..",
    ".wwsssssww.",
    ".wwwwkwwww.",
    "..wwwwwww..",
    "RRRRwwwRRRR",
    "RRyRRRRRyRR",
    "RRRRRyRRRRR",
    "RrRRRyRRRrR",
];
const QUEEN: [&str; 13] = [
    "...y.y.y...",
    "...yYyYy...",
    "..hhhhhhh..",
    ".hhsssssshh",
    ".hsksssksh.",
    ".hssssssshh",
    ".hhsssmsshh",
    ".hh.sss.hh.",
    "..RRyyyRR..",
    ".RRRRyRRRR.",
    "RRRRRyRRRRR",
    "RRyRRRRRyRR",
    "RrRRRRRRRrR",
];
const JACK: [&str; 13] = [
    "...RRRRR...",
    "..RRRRRRRw.",
    ".RRRRRRRRww",
    "...hhhhh...",
    "..hsssssh..",
    "..sksssks..",
    "..sssSsss..",
    "..ssskkss..",
    "...sssss...",
    "RRRyyyyyRRR",
    "RRRRRyRRRRR",
    "RRrRRyRRrRR",
    "RRRRRyRRRRR",
];

fn portrait(rank: u8, red: bool) -> impl Iterator<Item = (i32, i32, Rgb)> {
    let rows = match rank {
        13 => &KING,
        12 => &QUEEN,
        _ => &JACK,
    };
    let robe = if red { rgb(200, 35, 45) } else { rgb(45, 70, 170) };
    rows.iter().enumerate().flat_map(move |(y, row)| {
        row.chars().enumerate().filter_map(move |(x, ch)| {
            let c = match ch {
                'y' => rgb(235, 180, 40),
                'Y' => rgb(255, 235, 150),
                's' => rgb(245, 205, 165),
                'S' => rgb(215, 165, 125),
                'k' => rgb(35, 25, 30),
                'w' => rgb(225, 225, 230),
                'h' => if red { rgb(150, 85, 35) } else { rgb(230, 190, 70) },
                'm' => rgb(210, 40, 60),
                'R' => robe,
                'r' => scale(robe, 0.65),
                _ => return None,
            };
            Some((x as i32, y as i32, c))
        })
    })
}

/// A card's pixels, `CARD_W` by `CARD_H` (none is see-through): its face,
/// or its back.
pub fn card_image(card: Option<Card>) -> Vec<Option<Rgb>> {
    let mut img = vec![None; (CARD_W * CARD_H) as usize];
    for y in 0..CARD_H {
        for x in 0..CARD_W {
            let corner = (x == 0 || x == CARD_W - 1) && (y == 0 || y == CARD_H - 1);
            if corner {
                continue;
            }
            let edge = x == 0 || y == 0 || x == CARD_W - 1 || y == CARD_H - 1;
            let c = match card {
                Some(_) => {
                    let paper = mix(rgb(252, 251, 246), rgb(228, 226, 218), y as f32 / CARD_H as f32);
                    if edge { scale(paper, 0.86) } else { paper }
                }
                None => {
                    // A red back, a white border round a lattice.
                    let inner = x == 1 || y == 1 || x == CARD_W - 2 || y == CARD_H - 2;
                    if edge {
                        rgb(220, 218, 214)
                    } else if inner {
                        rgb(250, 248, 244)
                    } else {
                        let lattice = (x + y) % 4 == 0 || (x - y).rem_euclid(4) == 0;
                        let base = mix(rgb(175, 22, 38), rgb(120, 10, 25), y as f32 / CARD_H as f32);
                        if lattice { mix(base, rgb(240, 120, 130), 0.55) } else { base }
                    }
                }
            };
            img[(y * CARD_W + x) as usize] = Some(c);
        }
    }
    let Some(card) = card else { return img };
    let color = if card.red() { rgb(205, 25, 40) } else { rgb(28, 28, 38) };
    // The rank in the corner, the suit under it.
    let rank = match card.rank {
        1 => "A".to_string(),
        11 => "J".into(),
        12 => "Q".into(),
        13 => "K".into(),
        n => n.to_string(),
    };
    let mut glyphs = Canvas::new(CARD_W as usize, 8);
    glyphs.rect(0, 0, CARD_W, 8, rgb(1, 2, 3));
    glyphs.print(2, 0, &rank, 1, |_, _| color);
    for y in 0..7 {
        for x in 0..CARD_W {
            let c = glyphs.px[(y * CARD_W + x) as usize];
            if c != rgb(1, 2, 3) {
                img[((y + 2) * CARD_W + x) as usize] = Some(c);
            }
        }
    }
    suit_on(&mut img, 2, 10, 5, card.suit, color);
    if card.rank >= 11 {
        // A gold frame round the portrait.
        for y in 4..18 {
            for x in 5..16 {
                if y == 4 || y == 17 || x == 5 || x == 15 {
                    img[(y * CARD_W + x) as usize] = Some(rgb(215, 170, 60));
                }
            }
        }
        for (x, y, c) in portrait(card.rank, card.red()) {
            if (6..15).contains(&(x + 5)) || x + 5 == 15 {
                let (px, py) = (x + 5, y + 4);
                if (0..CARD_W).contains(&px) && py < CARD_H - 1 {
                    img[(py * CARD_W + px) as usize] = Some(c);
                }
            }
        }

    } else {
        let size = if card.rank == 1 { 10 } else { 9 };
        suit_on(&mut img, 16 - size, 18 - size, size, card.suit, color);
    }
    img
}

/// Put a card's pixels at (`x`, `y`), squeezed across to `wide` (0 to 1,
/// a card turning over), with its shadow on the felt.
pub fn blit_card(cv: &mut Canvas, img: &[Option<Rgb>], x: f32, y: f32, wide: f32, lift: f32) {
    let wide = wide.clamp(0.02, 1.0);
    let (x0, y0) = (x.round() as i32, y.round() as i32);
    let cx = x0 as f32 + CARD_W as f32 / 2.0;
    let w = (CARD_W as f32 * wide).max(1.0);
    let left = (cx - w / 2.0).round() as i32;
    let right = (cx + w / 2.0).round() as i32;
    let shadow = 1 + lift.round() as i32;
    for py in 0..CARD_H {
        for px in left..right {
            cv.blend(px + shadow, y0 + py + shadow, rgb(0, 0, 0), 0.35);
        }
    }
    for py in 0..CARD_H {
        for px in left..right {
            let u = ((px as f32 + 0.5 - cx) / wide + CARD_W as f32 / 2.0).floor() as i32;
            if !(0..CARD_W).contains(&u) {
                continue;
            }
            if let Some(c) = img[(py * CARD_W + u) as usize] {
                // Edge-on, the card darkens as it turns.
                cv.set(px, y0 + py, scale(c, 0.55 + 0.45 * wide));
            }
        }
    }
}

/// Chips by value: the face, and the stripes round the edge.
const CHIPS: [(u32, [u8; 3], [u8; 3]); 4] =
    [(100, [32, 30, 38], [235, 200, 90]), (25, [30, 140, 70], [245, 245, 245]), (5, [200, 30, 45], [245, 245, 245]), (1, [235, 235, 230], [40, 90, 210])];

/// A pile of chips worth `amount`, stacked by value side by side, its
/// middle at `cx` and its foot on row `by`.
pub fn draw_pile(cv: &mut Canvas, amount: u32, cx: f32, by: f32) {
    let mut stacks: Vec<(usize, u32)> = Vec::new();
    let mut left = amount;
    for (i, &(v, _, _)) in CHIPS.iter().enumerate() {
        let n = left / v;
        left -= n * v;
        let mut n = n;
        while n > 0 {
            let k = n.min(12);
            stacks.push((i, k));
            n -= k;
        }
    }
    let width = stacks.len() as f32 * 8.0 - 1.0;
    for (s, &(i, n)) in stacks.iter().enumerate() {
        let x = (cx - width / 2.0 + s as f32 * 8.0 + 3.0).round() as i32;
        let (face, stripe) = (CHIPS[i].1, CHIPS[i].2);
        let (face, stripe) = (rgb(face[0], face[1], face[2]), rgb(stripe[0], stripe[1], stripe[2]));
        let foot = by.round() as i32;
        cv.rect(x - 3, foot + 1, 7, 1, rgb(0, 0, 0));
        for k in 0..n as i32 {
            let y = foot - k;
            for dx in -3..=3 {
                let c = if (dx + k).rem_euclid(3) == 0 { stripe } else { scale(face, 0.7) };
                cv.set(x + dx, y, c);
            }
        }
        let top = foot - n as i32;
        for (dy, half) in [(-2i32, 2i32), (-1, 3), (0, 3)] {
            for dx in -half..=half {
                let ring = dx.abs() == half || (dy == -2 && dx.abs() == half - 1);
                let c = if ring { stripe } else { mix(face, rgb(255, 255, 255), if dy == -2 { 0.25 } else { 0.1 }) };
                cv.set(x + dx, top + dy, c);
            }
        }
    }
}

/// The felt, the arc printed on it, the betting circle and the shoe.
fn table() -> &'static Vec<Rgb> {
    static TABLE: OnceLock<Vec<Rgb>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                // A lamp over the middle of the table.
                let d = (((x as f32 - 42.0) / 46.0).powi(2) + ((y as f32 - 24.0) / 30.0).powi(2)).sqrt();
                let mut c = mix(rgb(34, 128, 68), rgb(6, 40, 22), smooth(0.2, 1.15, d));
                c = scale(c, 0.96 + 0.08 * hash(x, y));
                cv.set(x, y, c);
            }
        }
        // The insurance arc, in gold paint.
        for x in 0..W as i32 {
            let u = (x as f32 - 42.0) / 50.0;
            let y = 2.0 + 19.0 * (1.0 - u * u).max(0.0).sqrt();
            cv.blend(x, y.round() as i32, rgb(220, 185, 90), 0.55);
        }
        // The betting circle.
        for a in 0..96 {
            let t = a as f32 / 96.0 * 2.0 * PI;
            cv.blend((8.0 + 6.5 * t.cos()).round() as i32, (36.0 + 4.0 * t.sin()).round() as i32, rgb(230, 230, 220), 0.6);
        }
        // The shoe: a dark wooden box, the next card's back at its mouth.
        for y in 1..13 {
            for x in 67..80 {
                let front = x < 70 + (y - 1) / 3;
                let wood = mix(rgb(70, 30, 25), rgb(30, 10, 10), (y - 1) as f32 / 12.0);
                cv.set(x, y, if front { scale(wood, 1.35) } else { wood });
            }
        }
        for y in 3..11 {
            for x in 68..72 {
                cv.set(x, y, if (x + y) % 3 == 0 { rgb(220, 120, 130) } else { rgb(165, 20, 35) });
            }
        }
        cv.rect(67, 13, 13, 1, rgb(0, 20, 8));
        cv.px
    })
}

// ---- the table on screen -----------------------------------------------------

/// A card on the table: where it's going, from where and when, and when
/// it turns face up (never: face down).
#[derive(Clone, Copy)]
struct Placed {
    card: Card,
    from: (f32, f32),
    to: (f32, f32),
    t0: f32,
    dur: f32,
    face_at: f32,
}

/// A pile of chips moving, or sitting (its move done); gone at `gone`.
#[derive(Clone, Copy)]
struct Pile {
    amount: u32,
    from: (f32, f32),
    to: (f32, f32),
    t0: f32,
    dur: f32,
    gone: f32,
}

const SHOE: (f32, f32) = (66.0, 3.0);
const DEALER_Y: f32 = 0.0;
const PLAYER_Y: f32 = 24.0;
/// The bank the player's chips come from, and the dealer's tray.
const BANK: (f32, f32) = (8.0, 52.0);
const TRAY: (f32, f32) = (40.0, -8.0);
const CIRCLE: (f32, f32) = (8.0, 38.0);
const FLIP: f32 = 0.3;

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Default)]
struct Table {
    dealer: Vec<Placed>,
    hands: Vec<Vec<Placed>>,
    piles: Vec<Pile>,
    /// What each hand's played for.
    bets: Vec<u32>,
    /// The hand being played.
    turn: Option<usize>,
}

impl Table {
    /// Where each card of a hand of `n` sits, its middle at `cx`, in
    /// `room` pixels.
    fn spots(n: usize, cx: f32, room: f32) -> Vec<f32> {
        let step = if n > 1 { ((room - CARD_W as f32) / (n - 1) as f32).min(10.0) } else { 0.0 };
        let width = CARD_W as f32 + step * (n.max(1) - 1) as f32;
        (0..n).map(|i| cx - width / 2.0 + i as f32 * step).collect()
    }

    fn hand_x(&self, h: usize) -> (f32, f32) {
        match (self.hands.len(), h) {
            (1, _) => (48.0, 60.0),
            (_, 0) => (32.0, 30.0),
            _ => (64.0, 30.0),
        }
    }

    /// Move every card to where it belongs now (after a deal or a split).
    fn lay_out(&mut self, now: f32) {
        let xs = Table::spots(self.dealer.len(), 40.0, 48.0);
        for (p, x) in self.dealer.iter_mut().zip(xs) {
            settle_to(p, (x, DEALER_Y), now);
        }
        for h in 0..self.hands.len() {
            let (cx, room) = self.hand_x(h);
            let xs = Table::spots(self.hands[h].len(), cx, room);
            for (p, x) in self.hands[h].iter_mut().zip(xs) {
                settle_to(p, (x, PLAYER_Y), now);
            }
        }
    }

    fn deal_to(&mut self, who: Option<usize>, card: Card, face_up: bool, now: f32) {
        let p = Placed { card, from: SHOE, to: SHOE, t0: now, dur: 0.3, face_at: if face_up { now + 0.12 } else { f32::MAX } };
        match who {
            None => self.dealer.push(p),
            Some(h) => self.hands[h].push(p),
        }
        // The new card flies from the shoe; the rest shuffle along.
        self.lay_out(now);
        let last = match who {
            None => self.dealer.last_mut(),
            Some(h) => self.hands[h].last_mut(),
        };
        if let Some(p) = last {
            p.from = SHOE;
            p.t0 = now;
            p.dur = 0.3;
        }
    }

    fn busy_until(&self) -> f32 {
        let cards = self.dealer.iter().chain(self.hands.iter().flatten()).map(|p| (p.t0 + p.dur).max(if p.face_at < f32::MAX { p.face_at + FLIP } else { 0.0 }));
        let piles = self.piles.iter().map(|p| p.t0 + p.dur);
        cards.chain(piles).fold(0.0, f32::max)
    }

    fn draw(&self, cv: &mut Canvas, t: f32, purse: u32) {
        cv.px.clone_from(table());
        cv.clear_labels();
        for p in self.piles.iter().filter(|p| t < p.gone) {
            let k = ease((t - p.t0) / p.dur);
            draw_pile(cv, p.amount, p.from.0 + (p.to.0 - p.from.0) * k, p.from.1 + (p.to.1 - p.from.1) * k);
        }
        let back = card_image(None);
        let mut draw = |p: &Placed| {
            let k = ease((t - p.t0) / p.dur);
            let (x, y) = (p.from.0 + (p.to.0 - p.from.0) * k, p.from.1 + (p.to.1 - p.from.1) * k);
            // Turning: narrow to its edge, then open out on the other side.
            let turn = ((t - (p.face_at - FLIP / 2.0)) / FLIP).clamp(0.0, 1.0);
            let wide = (turn * PI).cos().abs();
            let face = turn >= 0.5;
            let lift = if k < 1.0 { 1.0 } else { 0.0 };
            if face {
                blit_card(cv, &card_image(Some(p.card)), x, y, wide, lift);
            } else {
                blit_card(cv, &back, x, y, wide, lift);
            }
        };
        for p in &self.dealer {
            draw(p);
        }
        for h in &self.hands {
            for p in h {
                draw(p);
            }
        }
        let gold = rgb(235, 200, 110);
        cv.label(30, 10, "BLACKJACK PAYS 3 TO 2", gold, None);
        // Totals of what's showing.
        let shown: Vec<Card> = self.dealer.iter().filter(|p| t >= p.face_at).map(|p| p.card).collect();
        if !shown.is_empty() {
            let (n, soft) = total(&shown);
            let s = if soft && n < 21 && shown.len() == self.dealer.len() { format!("dealer {}/{n}", n - 10) } else { format!("dealer {n}") };
            cv.label(1, 10, &s, rgb(255, 255, 255), None);
        }
        for (h, hand) in self.hands.iter().enumerate() {
            let cards: Vec<Card> = hand.iter().filter(|p| t >= p.face_at).map(|p| p.card).collect();
            if cards.is_empty() {
                continue;
            }
            let (n, soft) = total(&cards);
            let s = if soft && n < 21 { format!("{}/{n}", n - 10) } else { n.to_string() };
            let mark = if self.turn == Some(h) && self.hands.len() > 1 { "\u{25BA}" } else { "" };
            let (cx, _) = self.hand_x(h);
            let col = (cx as usize).saturating_sub(s.len() / 2 + 1);
            cv.label(col, 11, &format!("{mark}{s}"), if n > 21 { rgb(255, 120, 120) } else { rgb(255, 255, 255) }, None);
        }
        let _ = purse;
    }
}

fn settle_to(p: &mut Placed, to: (f32, f32), now: f32) {
    if p.to != to {
        let k = ease((now - p.t0) / p.dur);
        p.from = (p.from.0 + (p.to.0 - p.from.0) * k, p.from.1 + (p.to.1 - p.from.1) * k);
        p.to = to;
        p.t0 = now;
        p.dur = 0.25;
    }
}

// ---- the game ----------------------------------------------------------------

impl Session {
    /// Play until the caller leaves the table.
    pub(crate) async fn blackjack(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.blackjack_plain(cfg, purse).await;
        }
        let mut st = self.stage_open();
        let r = self.blackjack_table(cfg, purse, &mut st).await;
        self.stage_close();
        r
    }

    async fn bj_frame(&mut self, st: &mut Stage, tb: &Table, purse: u32) -> io::Result<()> {
        let t = st.t();
        tb.draw(&mut st.cv, t, purse);
        self.stage_show(st, true).await?;
        self.flush().await
    }

    /// Draw until everything moving has stopped.
    async fn bj_settle(&mut self, st: &mut Stage, tb: &Table, purse: u32) -> io::Result<()> {
        loop {
            self.bj_frame(st, tb, purse).await?;
            if st.t() > tb.busy_until() + 0.05 {
                return Ok(());
            }
            if self.key_waiting().await? {
                // Keys wait for the cards.
            }
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        }
    }

    fn bj_status(&mut self, st: &Stage, purse: u32, bet: u32, keys: &str) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        ink(&mut out, [140, 200, 150], st.truecolor);
        out.extend(b"chips ");
        ink(&mut out, [255, 230, 140], st.truecolor);
        out.extend(purse.to_string().as_bytes());
        ink(&mut out, [140, 200, 150], st.truecolor);
        out.extend(b"   bet ");
        ink(&mut out, [255, 230, 140], st.truecolor);
        out.extend(bet.to_string().as_bytes());
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let len = self.visible_len(&fluxbbs_term::cp437::encode(keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    async fn blackjack_table(&mut self, cfg: &Casino, purse: &mut Purse, st: &mut Stage) -> io::Result<()> {
        let mut shoe = Shoe::new(cfg.decks);
        let mut bet = cfg.min_bet.max(10.min(cfg.max_bet));
        let mut tb = Table::default();
        let hello = painted(&crate::editor::strip_colours(self.strings().get("BjHello")), [235, 200, 110]);
        self.stage_line(MSG_ROW, &hello, st.truecolor);
        loop {
            // Betting.
            if purse.chips < cfg.min_bet {
                let s = painted(&crate::editor::strip_colours(self.strings().get("CasinoBroke")), [255, 130, 140]);
                self.stage_line(MSG_ROW, &s, st.truecolor);
                self.bj_frame(st, &tb, purse.chips).await?;
                self.read_key().await?;
                return Ok(());
            }
            bet = bet.clamp(cfg.min_bet, cfg.max_bet.min(purse.chips));
            tb.piles.retain(|p| p.gone > st.t());
            tb.piles.push(Pile { amount: bet, from: CIRCLE, to: CIRCLE, t0: 0.0, dur: 0.01, gone: f32::MAX });
            let keys = self.strings().get("BjBetKeys").to_string();
            self.bj_status(st, purse.chips, bet, &keys);
            self.bj_frame(st, &tb, purse.chips).await?;
            let k = self.read_key().await?;
            tb.piles.pop();
            match k {
                Key::Esc => return Ok(()),
                Key::Up | Key::Right => bet = (bet + cfg.min_bet).min(cfg.max_bet.min(purse.chips)),
                Key::Down | Key::Left => bet = bet.saturating_sub(cfg.min_bet).max(cfg.min_bet),
                Key::Enter | Key::Char(b' ') => {
                    // Clear the last round away, then deal.
                    let now = st.t();
                    for p in tb.dealer.iter_mut().chain(tb.hands.iter_mut().flatten()) {
                        p.from = p.to;
                        p.to = (-20.0, -24.0);
                        p.t0 = now;
                        p.dur = 0.35;
                    }
                    self.bj_settle(st, &tb, purse.chips).await?;
                    tb = Table::default();
                    if shoe.cards.len() < shoe.cut {
                        shoe = Shoe::new(cfg.decks);
                        let s = painted(&crate::editor::strip_colours(self.strings().get("BjShuffle")), [200, 200, 220]);
                        self.stage_line(MSG_ROW, &s, st.truecolor);
                    } else {
                        self.stage_line(MSG_ROW, &[], st.truecolor);
                    }
                    purse.chips -= bet;
                    self.purse_save(purse);
                    self.bj_round(cfg, purse, st, &mut tb, &mut shoe, bet).await?;
                }
                k => match k.upper() {
                    Some('Q') => return Ok(()),
                    Some('+' | '=') => bet = (bet + cfg.min_bet).min(cfg.max_bet.min(purse.chips)),
                    Some('-' | '_') => bet = bet.saturating_sub(cfg.min_bet).max(cfg.min_bet),
                    _ => {}
                },
            }
        }
    }

    async fn bj_round(&mut self, cfg: &Casino, purse: &mut Purse, st: &mut Stage, tb: &mut Table, shoe: &mut Shoe, bet: u32) -> io::Result<()> {
        let now = st.t();
        tb.hands = vec![Vec::new()];
        tb.bets = vec![bet];
        tb.piles.push(Pile { amount: bet, from: BANK, to: CIRCLE, t0: now, dur: 0.35, gone: f32::MAX });
        // Two each, the dealer's second face down.
        for (i, who) in [Some(0), None, Some(0), None].into_iter().enumerate() {
            let at = st.t() + if i == 0 { 0.3 } else { 0.0 };
            let card = shoe.draw();
            tb.deal_to(who, card, !(who.is_none() && i == 3), at);
            self.bj_settle(st, tb, purse.chips).await?;
        }
        let up = tb.dealer[0].card;
        let dealer_bj = natural(&[tb.dealer[0].card, tb.dealer[1].card]);
        let player_bj = natural(&[tb.hands[0][0].card, tb.hands[0][1].card]);
        let peek = up.rank == 1 || up.value() == 10;
        // A blackjack either side ends the hand before it's played.
        let over = player_bj || (peek && dealer_bj);
        if !over {
            self.bj_player(purse, st, tb, shoe).await?;
        }
        tb.turn = None;
        // The dealer's turn, if a hand's still standing.
        let now = st.t();
        tb.dealer[1].face_at = now + 0.1;
        self.bj_settle(st, tb, purse.chips).await?;
        let alive = tb.hands.iter().any(|h| total(&h.iter().map(|p| p.card).collect::<Vec<_>>()).0 <= 21);
        if alive && !player_bj && !dealer_bj {
            loop {
                let cards: Vec<Card> = tb.dealer.iter().map(|p| p.card).collect();
                if !dealer_draws(&cards, cfg.hit_soft_17) {
                    break;
                }
                let at = st.t() + 0.35;
                tb.deal_to(None, shoe.draw(), true, at);
                self.bj_settle(st, tb, purse.chips).await?;
            }
        }
        self.bj_settle_bets(purse, st, tb).await
    }

    async fn bj_player(&mut self, purse: &mut Purse, st: &mut Stage, tb: &mut Table, shoe: &mut Shoe) -> io::Result<()> {
        let mut h = 0;
        while h < tb.hands.len() {
            tb.turn = Some(h);
            loop {
                let split = tb.hands.len() > 1;
                let cards: Vec<Card> = tb.hands[h].iter().map(|p| p.card).collect();
                let (n, _) = total(&cards);
                // Split aces take one card each.
                let aces_done = split && cards[0].rank == 1 && cards.len() >= 2;
                if n >= 21 || aces_done {
                    break;
                }
                let two = cards.len() == 2;
                let can_double = two && purse.chips >= tb.bets[h];
                let can_split = two && !split && cards[0].value() == cards[1].value() && purse.chips >= tb.bets[h];
                let mut keys = String::from(self.strings().get("BjKeysHitStand"));
                if can_double {
                    keys.push_str(self.strings().get("BjKeyDouble"));
                }
                if can_split {
                    keys.push_str(self.strings().get("BjKeySplit"));
                }
                self.bj_status(st, purse.chips, tb.bets.iter().sum(), &keys);
                self.bj_frame(st, tb, purse.chips).await?;
                let k = self.read_key().await?;
                match k.upper() {
                    Some('H') | Some(' ') => {
                        let at = st.t();
                        tb.deal_to(Some(h), shoe.draw(), true, at);
                        self.bj_settle(st, tb, purse.chips).await?;
                    }
                    Some('S') => break,
                    Some('D') if can_double => {
                        purse.chips -= tb.bets[h];
                        self.purse_save(purse);
                        let now = st.t();
                        tb.piles.push(Pile { amount: tb.bets[h], from: BANK, to: (CIRCLE.0 + h as f32 * 8.0 + 8.0, CIRCLE.1), t0: now, dur: 0.35, gone: f32::MAX });
                        tb.bets[h] *= 2;
                        tb.deal_to(Some(h), shoe.draw(), true, now + 0.2);
                        self.bj_settle(st, tb, purse.chips).await?;
                        break;
                    }
                    Some('P') if can_split => {
                        purse.chips -= tb.bets[h];
                        self.purse_save(purse);
                        let second = tb.hands[h].pop().expect("two cards");
                        tb.hands.push(vec![second]);
                        tb.bets.push(tb.bets[h]);
                        let now = st.t();
                        tb.piles.push(Pile { amount: tb.bets[h], from: BANK, to: (CIRCLE.0 + 8.0, CIRCLE.1), t0: now, dur: 0.35, gone: f32::MAX });
                        tb.lay_out(now);
                        self.bj_settle(st, tb, purse.chips).await?;
                        for hand in 0..2 {
                            let at = st.t();
                            tb.deal_to(Some(hand), shoe.draw(), true, at);
                            self.bj_settle(st, tb, purse.chips).await?;
                        }
                        // Play the first of the two from the top.
                        continue;
                    }
                    _ => {}
                }
            }
            h += 1;
        }
        Ok(())
    }

    /// Pay or take each hand's stake, chips moving to say which.
    async fn bj_settle_bets(&mut self, purse: &mut Purse, st: &mut Stage, tb: &mut Table) -> io::Result<()> {
        let dealer: Vec<Card> = tb.dealer.iter().map(|p| p.card).collect();
        let split = tb.hands.len() > 1;
        let now = st.t();
        // The stakes on the table now go as the hands came out.
        let staked: Vec<usize> = (0..tb.piles.len()).filter(|&i| tb.piles[i].gone == f32::MAX).collect();
        for &i in &staked {
            tb.piles[i].gone = now;
        }
        let mut back = 0;
        let mut won = 0i64;
        let mut outcomes = Vec::new();
        for (h, hand) in tb.hands.iter().enumerate() {
            let cards: Vec<Card> = hand.iter().map(|p| p.card).collect();
            let o = settle(&cards, &dealer, split);
            let bet = tb.bets[h];
            let r = returned(o, bet);
            back += r;
            won += r as i64 - bet as i64;
            outcomes.push(o);
        }
        let total_bet: u32 = tb.bets.iter().sum();
        // What was bet sits in the circle; winnings come from the tray to
        // join it, then it all goes home (or to the tray, lost).
        let lost = total_bet.saturating_sub(back.min(total_bet));
        let kept = total_bet - lost;
        if kept > 0 {
            tb.piles.push(Pile { amount: kept, from: CIRCLE, to: CIRCLE, t0: now, dur: 0.01, gone: now + 1.0 });
            tb.piles.push(Pile { amount: kept, from: CIRCLE, to: BANK, t0: now + 1.0, dur: 0.45, gone: now + 1.5 });
        }
        if lost > 0 {
            tb.piles.push(Pile { amount: lost, from: CIRCLE, to: TRAY, t0: now + 0.2, dur: 0.45, gone: now + 0.7 });
        }
        let profit = back.saturating_sub(total_bet);
        if profit > 0 {
            let beside = (CIRCLE.0 + 9.0, CIRCLE.1);
            tb.piles.push(Pile { amount: profit, from: TRAY, to: beside, t0: now + 0.1, dur: 0.5, gone: now + 1.0 });
            tb.piles.push(Pile { amount: profit, from: beside, to: BANK, t0: now + 1.0, dur: 0.45, gone: now + 1.5 });
        }
        purse.chips += back;
        self.purse_save(purse);
        let key = match (outcomes.as_slice(), won) {
            ([Outcome::Blackjack], _) => "BjBlackjack",
            ([Outcome::Lose], _) if natural(&dealer) => "BjDealerBlackjack",
            (_, w) if w > 0 => {
                if total(&dealer).0 > 21 { "BjDealerBusts" } else { "BjWin" }
            }
            (_, 0) => "BjPush",
            _ => {
                if tb.hands.iter().all(|h| total(&h.iter().map(|p| p.card).collect::<Vec<_>>()).0 > 21) { "BjBust" } else { "BjLose" }
            }
        };
        let text = crate::editor::strip_colours(&self.strings().fmt(key, &[&won.unsigned_abs().to_string()]));
        let good = won > 0;
        // The line shimmers on a win.
        let until = st.t() + 1.6;
        while st.t() < until {
            let t = st.t();
            let line: Vec<(char, Ink)> = text
                .chars()
                .enumerate()
                .map(|(i, ch)| {
                    let wave = 0.5 + 0.5 * ((i as f32) * 0.45 - t * 8.0).sin();
                    (ch, if good { shade([235, 160, 30], [255, 250, 210], wave) } else { [190, 170, 200] })
                })
                .collect();
            self.stage_line(MSG_ROW, &line, st.truecolor);
            self.bj_frame(st, tb, purse.chips).await?;
            if self.key_waiting().await? {
                break;
            }
        }
        let line = painted(&text, if good { [255, 215, 110] } else { [190, 170, 200] });
        self.stage_line(MSG_ROW, &line, st.truecolor);
        self.bj_settle(st, tb, purse.chips).await
    }

    /// Plain text: a line a deal.
    async fn blackjack_plain(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        let mut shoe = Shoe::new(cfg.decks);
        let names = |cards: &[Card]| cards.iter().map(|c| c.name()).collect::<Vec<_>>().join(" ");
        loop {
            if purse.chips < cfg.min_bet {
                return self.say_line("CasinoBroke").await;
            }
            if shoe.cards.len() < shoe.cut {
                shoe = Shoe::new(cfg.decks);
            }
            let s = self.strings().fmt("BjPlainBet", &[&purse.chips.to_string(), &cfg.min_bet.to_string(), &cfg.max_bet.min(purse.chips).to_string()]);
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(8)).await?;
            self.newline();
            let s = s.trim();
            if s.eq_ignore_ascii_case("q") {
                return Ok(());
            }
            let bet = if s.is_empty() { cfg.min_bet } else { s.parse().unwrap_or(0) };
            if bet < cfg.min_bet || bet > cfg.max_bet.min(purse.chips) {
                continue;
            }
            purse.chips -= bet;
            let mut hands = vec![vec![shoe.draw()]];
            let mut dealer = vec![shoe.draw()];
            hands[0].push(shoe.draw());
            dealer.push(shoe.draw());
            let mut bets = vec![bet];
            let peek = dealer[0].rank == 1 || dealer[0].value() == 10;
            let over = natural(&hands[0]) || (peek && natural(&dealer));
            if !over {
                let mut h = 0;
                while h < hands.len() {
                    loop {
                        let (n, _) = total(&hands[h]);
                        if n >= 21 || (hands.len() > 1 && hands[h][0].rank == 1 && hands[h].len() >= 2) {
                            break;
                        }
                        let two = hands[h].len() == 2;
                        self.write(&format!("dealer: {} ??   you: {} ({n})  h)it s)tand", dealer[0].name(), names(&hands[h])));
                        if two && purse.chips >= bets[h] {
                            self.write(" d)ouble");
                        }
                        if two && hands.len() == 1 && hands[h][0].value() == hands[h][1].value() && purse.chips >= bets[h] {
                            self.write(" p)split");
                        }
                        self.write("> ");
                        let a = self.read_string(crate::input::ReadOpts::new(4)).await?;
                        self.newline();
                        match a.trim().to_ascii_lowercase().as_str() {
                            "h" | "" => hands[h].push(shoe.draw()),
                            "s" => break,
                            "d" if two && purse.chips >= bets[h] => {
                                purse.chips -= bets[h];
                                bets[h] *= 2;
                                hands[h].push(shoe.draw());
                                break;
                            }
                            "p" if two && hands.len() == 1 && hands[h][0].value() == hands[h][1].value() && purse.chips >= bets[h] => {
                                purse.chips -= bets[h];
                                let c = hands[h].pop().expect("two cards");
                                hands.push(vec![c]);
                                bets.push(bets[h]);
                                hands[0].push(shoe.draw());
                                hands[1].push(shoe.draw());
                            }
                            _ => {}
                        }
                    }
                    h += 1;
                }
                if hands.iter().any(|h| total(h).0 <= 21) {
                    while dealer_draws(&dealer, cfg.hit_soft_17) {
                        dealer.push(shoe.draw());
                    }
                }
            }
            let split = hands.len() > 1;
            let mut won = 0i64;
            for (h, hand) in hands.iter().enumerate() {
                let r = returned(settle(hand, &dealer, split), bets[h]);
                purse.chips += r;
                won += r as i64 - bets[h] as i64;
                self.write(&format!("you: {} ({})  ", names(hand), total(hand).0));
            }
            self.write(&format!("dealer: {} ({})", names(&dealer), total(&dealer).0));
            self.newline();
            self.purse_save(purse);
            let s = match won {
                w if w > 0 => format!("you win {w}."),
                0 => "push.".into(),
                w => format!("you lose {}.", -w),
            };
            self.write(&s);
            self.newline();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rank: u8) -> Card {
        Card { rank, suit: 0 }
    }

    #[test]
    fn totals_count_aces_both_ways() {
        assert_eq!(total(&[c(1), c(6)]), (17, true));
        assert_eq!(total(&[c(1), c(6), c(10)]), (17, false));
        assert_eq!(total(&[c(1), c(1), c(9)]), (21, true));
        assert_eq!(total(&[c(13), c(12)]), (20, false));
        assert!(natural(&[c(1), c(11)]));
        assert!(!dealer_draws(&[c(1), c(6)], false));
        assert!(dealer_draws(&[c(1), c(6)], true));
        assert!(dealer_draws(&[c(10), c(6)], false));
    }

    #[test]
    fn hands_settle_as_the_house_does() {
        assert_eq!(settle(&[c(1), c(13)], &[c(10), c(9)], false), Outcome::Blackjack);
        assert_eq!(settle(&[c(1), c(13)], &[c(10), c(9)], true), Outcome::Win, "21 after a split isn't blackjack");
        assert_eq!(settle(&[c(1), c(13)], &[c(1), c(12)], false), Outcome::Push);
        assert_eq!(settle(&[c(10), c(9)], &[c(1), c(12)], false), Outcome::Lose);
        assert_eq!(settle(&[c(10), c(5), c(9)], &[c(10), c(6), c(10)], false), Outcome::Lose, "the player busts first");
        assert_eq!(settle(&[c(10), c(8)], &[c(10), c(6), c(10)], false), Outcome::Win);
        assert_eq!(returned(Outcome::Blackjack, 10), 25);
        assert_eq!(returned(Outcome::Win, 10), 20);
    }

    #[test]
    fn a_shoe_is_whole_decks() {
        let s = Shoe::new(6);
        assert_eq!(s.cards.len(), 312);
        assert_eq!(s.cards.iter().filter(|c| c.rank == 1 && c.suit == 2).count(), 6);
    }
}
