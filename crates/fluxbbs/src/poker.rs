//! New: video poker in the casino (`AC poker`). Jacks or Better on a
//! blue screen in a cabinet: the pay table over five cards, the column
//! for the coins in lit, the line that paid flashing. Up to five coins a
//! hand, a coin being the tables' smallest bet (`[casino] min_bet`); the
//! full-pay 9/6 table, the royal flush 4000 for five coins. Cards turn
//! over one after another on the deal and the draw, held ones staying.
//! Plain-text callers get a line per step. Bets are the day's free chips
//! (`casino.rs`).

use std::io;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;

use crate::blackjack::{CARD_H, Card, blit_card, card_image};
use crate::casino::{H, MSG_ROW, Purse, STATUS_ROW, Stage, W, ink, painted};
use crate::config::Casino;
use crate::input::Key;
use crate::pixels::{Canvas, Rgb, mix, rgb, scale};
use crate::session::Session;

// ---- the rules -------------------------------------------------------------

/// The hands that pay, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hand {
    JacksOrBetter,
    TwoPair,
    ThreeOfAKind,
    Straight,
    Flush,
    FullHouse,
    FourOfAKind,
    StraightFlush,
    RoyalFlush,
}

/// The pay table, top line first: the hand, its name, what it pays a coin.
const HANDS: [(Hand, &str, u32); 9] = [
    (Hand::RoyalFlush, "ROYAL FLUSH", 250),
    (Hand::StraightFlush, "STRAIGHT FLUSH", 50),
    (Hand::FourOfAKind, "FOUR OF A KIND", 25),
    (Hand::FullHouse, "FULL HOUSE", 9),
    (Hand::Flush, "FLUSH", 6),
    (Hand::Straight, "STRAIGHT", 4),
    (Hand::ThreeOfAKind, "THREE OF A KIND", 3),
    (Hand::TwoPair, "TWO PAIR", 2),
    (Hand::JacksOrBetter, "JACKS OR BETTER", 1),
];
pub const MAX_COINS: u32 = 5;
/// The royal with every coin in.
const ROYAL_MAX: u32 = 800;

fn hand_name(h: Hand) -> &'static str {
    HANDS.iter().find(|x| x.0 == h).map_or("", |x| x.1)
}

/// What a hand pays in coins for `coins` played.
pub fn pays(h: Hand, coins: u32) -> u32 {
    if h == Hand::RoyalFlush && coins >= MAX_COINS {
        return ROYAL_MAX * coins;
    }
    HANDS.iter().find(|x| x.0 == h).map_or(0, |x| x.2) * coins
}

/// The best hand in five cards, if it pays.
pub fn rank(cards: &[Card; 5]) -> Option<Hand> {
    let mut counts = [0u8; 14];
    for c in cards {
        counts[c.rank as usize] += 1;
    }
    let flush = cards.iter().all(|c| c.suit == cards[0].suit);
    let mut ranks: Vec<u8> = cards.iter().map(|c| if c.rank == 1 { 14 } else { c.rank }).collect();
    ranks.sort_unstable();
    let distinct = ranks.windows(2).all(|w| w[0] != w[1]);
    let straight = distinct && (ranks[4] - ranks[0] == 4 || ranks == [2, 3, 4, 5, 14]);
    let royal = straight && ranks[0] == 10;
    let mut groups: Vec<u8> = counts.iter().copied().filter(|&n| n > 0).collect();
    groups.sort_unstable_by(|a, b| b.cmp(a));
    let high_pair = [1usize, 11, 12, 13].iter().any(|&r| counts[r] == 2);
    Some(match (flush, straight, groups.as_slice()) {
        (true, true, _) if royal => Hand::RoyalFlush,
        (true, true, _) => Hand::StraightFlush,
        (_, _, [4, ..]) => Hand::FourOfAKind,
        (_, _, [3, 2]) => Hand::FullHouse,
        (true, _, _) => Hand::Flush,
        (_, true, _) => Hand::Straight,
        (_, _, [3, ..]) => Hand::ThreeOfAKind,
        (_, _, [2, 2, ..]) => Hand::TwoPair,
        (_, _, [2, ..]) if high_pair => Hand::JacksOrBetter,
        _ => return None,
    })
}

/// A fresh deck, shuffled.
fn deck() -> Vec<Card> {
    let mut cards: Vec<Card> = (0..4u8).flat_map(|suit| (1..=13u8).map(move |rank| Card { rank, suit })).collect();
    cards.shuffle(&mut rand::rng());
    cards
}

// ---- the screen --------------------------------------------------------------

/// Where the cards sit: five across, overlapping a little to fit.
const CARD_X0: i32 = 1;
const CARD_STEP: i32 = 15;
const CARD_Y: i32 = 22;
/// A card turning over, and the gap between one card's turn and the next.
const FLIP: f32 = 0.28;
const STAGGER: f32 = 0.14;
/// The pay table's columns: the names, and the first amount column, 9
/// apart and 7 wide.
const NAME_COL: usize = 3;
/// The first coin column: the name padded to 29 after `NAME_COL`.
const PAY_COL: usize = NAME_COL + 29;

fn image(c: Option<Card>) -> &'static [Option<Rgb>] {
    static IMAGES: OnceLock<Vec<Vec<Option<Rgb>>>> = OnceLock::new();
    let all = IMAGES.get_or_init(|| {
        (0..4u8).flat_map(|s| (1..=13u8).map(move |r| Some(Card { rank: r, suit: s }))).chain([None]).map(card_image).collect()
    });
    &all[c.map_or(52, |c| c.suit as usize * 13 + c.rank as usize - 1)]
}

/// A card's place on the screen: what it showed, what it shows now, and
/// when it began turning from one to the other.
#[derive(Clone, Copy)]
struct Slot {
    from: Option<Card>,
    to: Option<Card>,
    t0: f32,
}

struct Machine {
    slots: [Slot; 5],
    held: [bool; 5],
    coins: u32,
    /// The line that paid, and when it lit.
    win: Option<(Hand, f32)>,
    /// The caller is picking cards to hold.
    holding: bool,
}

impl Machine {
    fn new() -> Machine {
        Machine { slots: [Slot { from: None, to: None, t0: -1.0 }; 5], held: [false; 5], coins: 1, win: None, holding: false }
    }

    /// Turn card `k` over to `card`, starting at `t0`.
    fn turn(&mut self, k: usize, card: Card, t0: f32) {
        let s = &mut self.slots[k];
        s.from = s.to;
        s.to = Some(card);
        s.t0 = t0;
    }

    fn busy_until(&self) -> f32 {
        self.slots.iter().map(|s| s.t0 + FLIP).fold(0.0, f32::max)
    }

    fn cards(&self) -> [Card; 5] {
        self.slots.map(|s| s.to.unwrap_or(Card { rank: 1, suit: 0 }))
    }

    fn draw(&self, cv: &mut Canvas, t: f32) {
        // The cabinet's bezel at the sides and foot of a blue screen,
        // darker at the bottom and towards the edges.
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let c = if !(1..W as i32 - 1).contains(&x) || y == H as i32 - 1 {
                    rgb(95, 95, 110)
                } else {
                    let edge = ((x as f32 - 40.0) / 40.0).powi(2) * 0.25;
                    scale(mix(rgb(10, 18, 135), rgb(4, 8, 70), y as f32 / H as f32), 1.0 - edge)
                };
                cv.set(x, y, c);
            }
        }
        cv.clear_labels();
        // The pay table: the coins' column lit, the line that paid flashing.
        let (yellow, white, red, navy) = (rgb(255, 215, 60), rgb(255, 255, 255), rgb(150, 20, 30), rgb(4, 8, 70));
        let flash = self.win.is_some_and(|(_, t0)| ((t - t0) * 4.0) as i32 % 2 == 0);
        let lit_col = PAY_COL + (self.coins as usize - 1) * 9;
        cv.label(NAME_COL, 0, "JACKS OR BETTER", white, None);
        cv.label(lit_col, 0, "       ", white, Some(red));
        for (i, &(h, name, _)) in HANDS.iter().enumerate() {
            let row = i + 1;
            let won = self.win.is_some_and(|(w, _)| w == h);
            let (fg, bg) = if won { (if flash { white } else { navy }, Some(if flash { red } else { yellow })) } else { (yellow, None) };
            let mut line = format!("{name:<29}");
            for c in 1..=MAX_COINS {
                line.push_str(&format!("{:>7}  ", pays(h, c)));
            }
            cv.label(NAME_COL, row, line.trim_end(), fg, bg);
            if !won {
                cv.label(lit_col, row, &format!("{:>7}", pays(h, self.coins)), white, Some(red));
            }
        }
        // The cards, right to left so each lies over the next one's edge.
        for k in (0..5).rev() {
            let s = self.slots[k];
            let p = ((t - s.t0) / FLIP).clamp(0.0, 1.0);
            let (card, wide) = if p < 0.5 { (s.from, 1.0 - 2.0 * p) } else { (s.to, 2.0 * p - 1.0) };
            blit_card(cv, image(card), (CARD_X0 + k as i32 * CARD_STEP) as f32, CARD_Y as f32, wide, 0.0);
            if self.held[k] {
                cv.label((CARD_X0 + k as i32 * CARD_STEP) as usize + 6, (CARD_Y + CARD_H) as usize / 2, " HELD ", white, Some(red));
            }
        }
    }
}

// ---- the game ----------------------------------------------------------------

impl Session {
    pub(crate) async fn video_poker(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        if self.plain() || self.screen.rows() < 24 {
            return self.poker_plain(cfg, purse).await;
        }
        let mut st = self.stage_open();
        let r = self.poker_machine(cfg, purse, &mut st).await;
        self.stage_close();
        r
    }

    async fn poker_frame(&mut self, st: &mut Stage, m: &Machine) -> io::Result<()> {
        let t = st.t();
        m.draw(&mut st.cv, t);
        self.stage_show(st, true).await?;
        self.flush().await
    }

    /// Draw until the cards have stopped turning.
    async fn poker_settle(&mut self, st: &mut Stage, m: &Machine) -> io::Result<()> {
        loop {
            self.poker_frame(st, m).await?;
            if st.t() > m.busy_until() + 0.05 {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }

    /// Draw (the win line flashing) until a key comes; none in a quarter
    /// of an hour and the caller has gone.
    async fn poker_key(&mut self, st: &mut Stage, m: &Machine) -> io::Result<Option<Key>> {
        let since = Instant::now();
        loop {
            self.poker_frame(st, m).await?;
            if self.key_waiting().await? {
                return Ok(Some(self.read_key().await?));
            }
            if since.elapsed() > Duration::from_secs(900) {
                return Ok(None);
            }
            tokio::time::sleep(self.idle_pause(120)).await;
        }
    }

    fn poker_status(&mut self, st: &Stage, purse: u32, coin: u32, coins: u32, keys: &str) {
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        for (label, n) in [("chips ", purse), ("   coin ", coin), ("   bet ", coins * coin)] {
            ink(&mut out, [150, 170, 230], st.truecolor);
            out.extend(label.as_bytes());
            ink(&mut out, [255, 230, 140], st.truecolor);
            out.extend(n.to_string().as_bytes());
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        let len = self.visible_len(&fluxbbs_term::cp437::encode(keys));
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    fn poker_say(&mut self, st: &Stage, name: &str, args: &[&str], color: [u8; 3]) {
        let s = self.strings().fmt(name, args);
        let s = painted(&crate::editor::strip_colours(&s), color);
        self.stage_line(MSG_ROW, &s, st.truecolor);
    }

    async fn poker_machine(&mut self, cfg: &Casino, purse: &mut Purse, st: &mut Stage) -> io::Result<()> {
        let coin = cfg.min_bet.max(1);
        let mut m = Machine::new();
        self.poker_say(st, "PokerHello", &[], [150, 200, 255]);
        loop {
            if purse.chips < coin {
                self.poker_say(st, "CasinoBroke", &[], [255, 130, 140]);
                self.poker_frame(st, &m).await?;
                self.read_key().await?;
                return Ok(());
            }
            let most = MAX_COINS.min(purse.chips / coin);
            m.coins = m.coins.clamp(1, most);
            m.holding = false;
            let keys = self.strings().get("PokerBetKeys").to_string();
            self.poker_status(st, purse.chips, coin, m.coins, &keys);
            let Some(k) = self.poker_key(st, &m).await? else { return Ok(()) };
            let deal = match k {
                Key::Esc => return Ok(()),
                Key::Up | Key::Right => {
                    m.coins = (m.coins + 1).min(most);
                    false
                }
                Key::Down | Key::Left => {
                    m.coins = (m.coins - 1).max(1);
                    false
                }
                Key::Enter | Key::Char(b' ') => true,
                k => match k.upper() {
                    Some('Q') => return Ok(()),
                    Some('+' | '=') => {
                        m.coins = (m.coins + 1).min(most);
                        false
                    }
                    Some('-' | '_') => {
                        m.coins = (m.coins - 1).max(1);
                        false
                    }
                    Some('M') => {
                        m.coins = most;
                        true
                    }
                    Some(d @ '1'..='5') => {
                        m.coins = (d as u32 - '0' as u32).min(most);
                        false
                    }
                    _ => false,
                },
            };
            if !deal {
                continue;
            }
            // The deal: the bet goes in before the cards turn, so a
            // dropped call keeps the house honest.
            purse.stake(m.coins * coin);
            self.purse_save(purse);
            let cards = deck();
            m.win = None;
            m.held = [false; 5];
            let now = st.t();
            for (k, &card) in cards[..5].iter().enumerate() {
                m.turn(k, card, now + k as f32 * STAGGER);
            }
            self.stage_line(MSG_ROW, &[], st.truecolor);
            self.poker_status(st, purse.chips, coin, m.coins, "");
            self.poker_settle(st, &m).await?;
            // Holding.
            m.holding = true;
            let keys = self.strings().get("PokerHoldKeys").to_string();
            self.poker_status(st, purse.chips, coin, m.coins, &keys);
            loop {
                let Some(k) = self.poker_key(st, &m).await? else { return Ok(()) };
                match k {
                    Key::Enter | Key::Char(b' ') => break,
                    Key::Char(d @ b'1'..=b'5') => m.held[(d - b'1') as usize] = !m.held[(d - b'1') as usize],
                    _ => {}
                }
            }
            m.holding = false;
            let (mut next, now) = (5, st.t());
            for k in 0..5 {
                if !m.held[k] {
                    m.turn(k, cards[next], now + (next - 5) as f32 * STAGGER);
                    next += 1;
                }
            }
            self.poker_status(st, purse.chips, coin, m.coins, "");
            self.poker_settle(st, &m).await?;
            match rank(&m.cards()) {
                Some(h) => {
                    let won = pays(h, m.coins) * coin;
                    purse.chips += won;
                    self.purse_save(purse);
                    m.win = Some((h, st.t()));
                    let name = hand_name(h).to_ascii_lowercase();
                    self.poker_say(st, "PokerWin", &[&name, &won.to_string()], [255, 230, 120]);
                    if h >= Hand::FourOfAKind {
                        tracing::info!("{}: video poker: {name} for {won} chips", self.user.handle);
                    }
                }
                None => self.poker_say(st, "PokerLose", &[], [170, 180, 220]),
            }
        }
    }

    async fn poker_plain(&mut self, cfg: &Casino, purse: &mut Purse) -> io::Result<()> {
        let coin = cfg.min_bet.max(1);
        let names = |cards: &[Card]| cards.iter().enumerate().map(|(i, c)| format!("{}:{}", i + 1, c.name())).collect::<Vec<_>>().join(" ");
        loop {
            if purse.chips < coin {
                return self.say_line("CasinoBroke").await;
            }
            let most = MAX_COINS.min(purse.chips / coin);
            let s = self.strings().fmt("PokerPlainPrompt", &[&purse.chips.to_string(), &coin.to_string(), &most.to_string()]);
            self.show(&s).await?;
            let s = self.read_string(crate::input::ReadOpts::new(4)).await?;
            self.newline();
            let s = s.trim().to_ascii_lowercase();
            let coins = match s.as_str() {
                "q" => return Ok(()),
                "" => 1,
                "m" => most,
                n => n.parse().unwrap_or(0),
            };
            if coins < 1 || coins > most {
                continue;
            }
            purse.stake(coins * coin);
            self.purse_save(purse);
            let cards = deck();
            let mut hand: [Card; 5] = cards[..5].try_into().expect("five");
            self.write(&names(&hand));
            self.newline();
            self.say("PokerPlainHold").await?;
            let s = self.read_string(crate::input::ReadOpts::new(8)).await?;
            self.newline();
            let mut next = 5;
            for (k, c) in hand.iter_mut().enumerate() {
                if !s.contains(char::from(b'1' + k as u8)) {
                    *c = cards[next];
                    next += 1;
                }
            }
            self.write(&names(&hand));
            self.write("  ");
            match rank(&hand) {
                Some(h) => {
                    let won = pays(h, coins) * coin;
                    purse.chips += won;
                    self.purse_save(purse);
                    let s = self.strings().fmt("PokerWin", &[&hand_name(h).to_ascii_lowercase(), &won.to_string()]);
                    self.write(&crate::editor::strip_colours(&s));
                }
                None => {
                    let s = self.strings().get("PokerLose").to_string();
                    self.write(&crate::editor::strip_colours(&s));
                }
            }
            self.newline();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> [Card; 5] {
        let one = |w: &str| {
            let (r, s) = w.split_at(w.len() - 1);
            let rank = match r {
                "A" => 1,
                "J" => 11,
                "Q" => 12,
                "K" => 13,
                n => n.parse().unwrap(),
            };
            Card { rank, suit: "shdc".find(s).unwrap() as u8 }
        };
        s.split_whitespace().map(one).collect::<Vec<_>>().try_into().unwrap()
    }

    #[test]
    fn hands_rank_as_the_table_says() {
        assert_eq!(rank(&h("10s Js Qs Ks As")), Some(Hand::RoyalFlush));
        assert_eq!(rank(&h("5h 6h 7h 8h 9h")), Some(Hand::StraightFlush));
        assert_eq!(rank(&h("Ah 2h 3h 4h 5h")), Some(Hand::StraightFlush), "the wheel, suited");
        assert_eq!(rank(&h("9s 9h 9d 9c 2s")), Some(Hand::FourOfAKind));
        assert_eq!(rank(&h("9s 9h 9d 2c 2s")), Some(Hand::FullHouse));
        assert_eq!(rank(&h("2c 5c 9c Jc Kc")), Some(Hand::Flush));
        assert_eq!(rank(&h("As 2h 3d 4c 5s")), Some(Hand::Straight), "the wheel");
        assert_eq!(rank(&h("10s Jh Qd Kc As")), Some(Hand::Straight));
        assert_eq!(rank(&h("Ks Ah 2d 3c 4s")), None, "no wrapping round");
        assert_eq!(rank(&h("7s 7h 7d Kc 4s")), Some(Hand::ThreeOfAKind));
        assert_eq!(rank(&h("7s 7h Kd Kc 4s")), Some(Hand::TwoPair));
        assert_eq!(rank(&h("Js Jh 2d 5c 9s")), Some(Hand::JacksOrBetter));
        assert_eq!(rank(&h("As Ah 2d 5c 9s")), Some(Hand::JacksOrBetter), "aces too");
        assert_eq!(rank(&h("10s 10h 2d 5c 9s")), None, "tens don't");
        assert_eq!(rank(&h("2s 4h 6d 8c 10s")), None);
    }

    #[test]
    fn the_table_pays_nine_six_and_the_royal_bonus() {
        assert_eq!(pays(Hand::FullHouse, 1), 9);
        assert_eq!(pays(Hand::Flush, 3), 18);
        assert_eq!(pays(Hand::RoyalFlush, 4), 1000);
        assert_eq!(pays(Hand::RoyalFlush, 5), 4000);
        assert_eq!(pays(Hand::JacksOrBetter, 5), 5);
    }

    #[test]
    fn a_deck_is_whole() {
        let mut d = deck();
        assert_eq!(d.len(), 52);
        d.sort_by_key(|c| (c.suit, c.rank));
        d.dedup();
        assert_eq!(d.len(), 52);
    }
}
