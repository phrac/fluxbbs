//! New: the tarot and fortune door (`AT`), read by the board's AI. The
//! board draws the cards (the 78 of the Rider-Waite deck, each upright or
//! reversed) and the AI only reads them. The card of the day is the same
//! card all day for a caller; three cards (past, present, future) answer a
//! question; a fortune is a fortune cookie with lucky numbers, on a slip
//! under the crystal ball. The pictures are in `tarotart`. The stock
//! TAROT menu offers each reading (`AT day`, `AT three`, `AT fortune`);
//! `AT` alone asks itself. TAROTHDR heads each screen if the board has one. A caller has `tarot_daily` readings
//! and `fortune_daily` fortunes a day (the card of the day's reading is
//! kept, so seeing it again is free), and each counts against the AI
//! chat's daily limit too.

use std::io;

use fluxbbs_term::{Color, Slot};
use rand::seq::SliceRandom;

use crate::config::Ai;
use crate::session::Session;
use crate::tarotart::{Cell, PIC_H, PIC_W, PIPS, crystal_ball, picture};

const MAJOR: [&str; 22] = [
    "The Fool", "The Magician", "The High Priestess", "The Empress", "The Emperor", "The Hierophant", "The Lovers", "The Chariot",
    "Strength", "The Hermit", "Wheel of Fortune", "Justice", "The Hanged Man", "Death", "Temperance", "The Devil", "The Tower",
    "The Star", "The Moon", "The Sun", "Judgement", "The World",
];
const RANKS: [&str; 14] = ["Ace", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten", "Page", "Knight", "Queen", "King"];
const SUITS: [&str; 4] = ["Wands", "Cups", "Swords", "Pentacles"];
const ROMAN: [&str; 22] = ["0", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV", "XV", "XVI", "XVII", "XVIII", "XIX", "XX", "XXI"];
const DECK: usize = 78;
/// A card's size on screen, frame and all.
const CARD_W: usize = 21;
const CARD_H: usize = PIC_H + 6;
/// The fortune slip's width.
const SLIP_W: usize = 60;

/// A card drawn: its place in the deck (majors first), and reversed.
type Card = (usize, bool);

pub fn card_name(i: usize) -> String {
    match i {
        0..22 => MAJOR[i].to_string(),
        _ => format!("{} of {}", RANKS[(i - 22) % 14], SUITS[(i - 22) / 14]),
    }
}

/// The line over a card's name: a major's number, a minor's suit.
fn card_top(i: usize) -> String {
    match i {
        0..22 => ROMAN[i].to_string(),
        _ => SUITS[(i - 22) / 14].to_lowercase(),
    }
}

fn mix(mut x: u64) -> u64 {
    // splitmix64.
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// The caller's card for the day `day` (days since the epoch): the same
/// all day, another tomorrow.
pub fn card_of_the_day(user: u32, day: i64) -> Card {
    let x = mix(((user as u64) << 32) ^ day as u64);
    ((x % DECK as u64) as usize, (x >> 63) == 1)
}

/// `n` different cards from a shuffled deck.
fn draw(n: usize) -> Vec<Card> {
    let mut deck: Vec<usize> = (0..DECK).collect();
    deck.shuffle(&mut rand::rng());
    deck.into_iter().take(n).map(|i| (i, rand::random::<bool>())).collect()
}

/// A card as cells: its frame (double for the majors), its number or
/// suit, its picture, its name and, if reversed, `rev_label`.
fn card_face(card: usize, reversed: bool, rev_label: &str) -> Vec<Vec<Cell>> {
    let inner = CARD_W - 2;
    let (frame, top_color) = match card {
        0..22 => (6, 14),
        _ => ([4, 3, 7, 6][(card - 22) / 14], PIPS[(card - 22) / 14].1),
    };
    let [tl, h, tr, v, bl, br] = if card < 22 { ['╔', '═', '╗', '║', '╚', '╝'] } else { ['┌', '─', '┐', '│', '└', '┘'] };
    let text = |s: &str, k: u8| center(s, inner).chars().map(|c| (c, k)).collect::<Vec<Cell>>();
    let name = crate::multinode::wrap(&card_name(card), inner - 2);
    let mut body: Vec<Vec<Cell>> = vec![text(&card_top(card), top_color)];
    let pad = (inner - PIC_W) / 2;
    for row in picture(card, reversed) {
        let mut r = vec![(' ', 7); pad];
        r.extend(row);
        r.resize(inner, (' ', 7));
        body.push(r);
    }
    body.push(text(name.first().map_or("", |s| s.as_str()), 15));
    body.push(text(name.get(1).map_or("", |s| s.as_str()), 15));
    body.push(text(if reversed { rev_label } else { "" }, 12));
    let mut rows = vec![std::iter::once((tl, frame)).chain(std::iter::repeat_n((h, frame), inner)).chain([(tr, frame)]).collect::<Vec<Cell>>()];
    for b in body {
        rows.push(std::iter::once((v, frame)).chain(b).chain([(v, frame)]).collect());
    }
    rows.push(std::iter::once((bl, frame)).chain(std::iter::repeat_n((h, frame), inner)).chain([(br, frame)]).collect());
    rows
}

fn center(s: &str, w: usize) -> String {
    let n = s.chars().count().min(w);
    let left = (w - n) / 2;
    format!("{}{}{}", " ".repeat(left), s.chars().take(w).collect::<String>(), " ".repeat(w - n - left))
}

impl Session {
    /// `AT` (new): the tarot and fortune door. `param` may name one
    /// reading (`day`, `three`, `fortune`) to go straight to it.
    pub async fn tarot(&mut self, param: &str) -> io::Result<()> {
        let cfg = self.state.config().ai.clone();
        if !cfg.enabled || cfg.base_url.is_empty() || cfg.model.is_empty() {
            self.newline();
            return self.say_line("AiOff").await;
        }
        if !self.acs(&cfg.acs) {
            self.newline();
            return self.say_line("AiNoAccess").await;
        }
        tracing::info!("{}: tarot", self.user.handle);
        self.node.update(|n| n.activity = "Reading the tarot".into());
        self.seen.doors.insert("Tarot".into());
        let once = match param.trim().chars().next().map(|c| c.to_ascii_uppercase()) {
            Some(c @ ('D' | 'T' | 'F')) => Some(c),
            _ => None,
        };
        loop {
            let pick = match once {
                Some(c) => c,
                None => {
                    self.tarot_screen().await?;
                    self.newline();
                    if let (Some(t), Some(f)) = (self.tarot_left("tarot", cfg.tarot_daily), self.tarot_left("fortune", cfg.fortune_daily)) {
                        let s = self.strings().fmt("TarotLeft", &[&t.to_string(), &f.to_string()]);
                        self.show_line(&s).await?;
                    }
                    self.say("TarotMenu").await?;
                    let k = self.read_key().await?;
                    if k == crate::input::Key::Esc {
                        break;
                    }
                    k.upper().unwrap_or(' ')
                }
            };
            match pick {
                'D' => self.card_of_the_day(&cfg).await?,
                'T' => {
                    if self.tarot_spent("tarot", cfg.tarot_daily, "TarotDailyLimit").await? {
                        continue;
                    }
                    self.tarot_screen().await?;
                    self.newline();
                    self.say("TarotAsk").await?;
                    let question = self.read_string(crate::input::ReadOpts::new(60)).await?;
                    if self.hangup {
                        break;
                    }
                    let places = ["TarotPast", "TarotPresent", "TarotFuture"].map(|s| self.strings().get(s).to_string());
                    if let Some(text) = self.tarot_reading(&cfg, &draw(3), &places, question.trim()).await? {
                        self.tarot_use("tarot");
                        self.tarot_show(&text, None).await?;
                    }
                }
                'F' => {
                    if !self.tarot_spent("fortune", cfg.fortune_daily, "FortuneDailyLimit").await? {
                        self.fortune(&cfg).await?;
                    }
                }
                'Q' => break,
                _ => continue,
            }
            if once.is_some() || self.hangup {
                break;
            }
        }
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        Ok(())
    }

    async fn tarot_screen(&mut self) -> io::Result<()> {
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        if !self.show_file("TAROTHDR").await? {
            self.say_line("TarotTitle").await?;
        }
        Ok(())
    }

    /// Readings of a kind (`tarot`, `fortune`) the caller has left today;
    /// None if there's no limit (0, or a SysOp).
    pub(crate) fn tarot_left(&self, kind: &str, limit: u32) -> Option<u32> {
        (limit > 0 && !self.is_sysop()).then(|| limit.saturating_sub(self.tarot_used(kind)))
    }

    fn tarot_used(&self, kind: &str) -> u32 {
        let today = crate::clock::now().date_naive().to_string();
        let v = self.state.sys.script_get("tarot", &format!("{}:{kind}", self.user.id)).ok().flatten().unwrap_or_default();
        v.split_once(' ').filter(|(day, _)| *day == today).and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
    }

    fn tarot_use(&mut self, kind: &str) {
        self.record_play("Tarot", crate::clock::now().naive_local());
        let today = crate::clock::now().date_naive().to_string();
        let n = self.tarot_used(kind) + 1;
        if let Err(e) = self.state.sys.script_set("tarot", &format!("{}:{kind}", self.user.id), Some(&format!("{today} {n}"))) {
            tracing::warn!("tarot: {e}");
        }
    }

    /// True (after saying so) if the day's readings of a kind are used up.
    async fn tarot_spent(&mut self, kind: &str, limit: u32, say: &str) -> io::Result<bool> {
        if self.tarot_left(kind, limit) != Some(0) {
            return Ok(false);
        }
        self.tarot_screen().await?;
        self.newline();
        let s = self.strings().fmt(say, &[&limit.to_string()]);
        self.show_line(&s).await?;
        self.newline();
        self.prompt_key().await?;
        Ok(true)
    }

    /// The card of the day, and its reading: the AI's the first time, kept
    /// for the rest of the day.
    async fn card_of_the_day(&mut self, cfg: &Ai) -> io::Result<()> {
        let today = crate::clock::now().date_naive();
        let day = today.signed_duration_since(chrono::NaiveDate::default()).num_days();
        let card = card_of_the_day(self.user.id, day);
        let place = self.strings().get("TarotToday").to_string();
        let key = format!("{}:day", self.user.id);
        let kept = self.state.sys.script_get("tarot", &key).ok().flatten().and_then(|v| {
            let (d, text) = v.split_once('\n')?;
            (d == today.to_string()).then(|| text.to_string())
        });
        if let Some(text) = kept {
            self.tarot_screen().await?;
            self.tarot_cards(&[card], &[place]);
            return self.tarot_show(&text, None).await;
        }
        if self.tarot_spent("tarot", cfg.tarot_daily, "TarotDailyLimit").await? {
            return Ok(());
        }
        if let Some(text) = self.tarot_reading(cfg, &[card], &[place], "").await? {
            self.tarot_use("tarot");
            if let Err(e) = self.state.sys.script_set("tarot", &key, Some(&format!("{today}\n{text}"))) {
                tracing::warn!("tarot: {e}");
            }
            self.tarot_show(&text, None).await?;
        }
        Ok(())
    }

    /// The cards, and what the AI makes of them, to show under them (once
    /// it's counted: a caller who hangs up halfway has had it).
    async fn tarot_reading(&mut self, cfg: &Ai, cards: &[Card], places: &[String], question: &str) -> io::Result<Option<String>> {
        self.tarot_screen().await?;
        self.tarot_cards(cards, places);
        let reversed = self.strings().get("TarotReversed").to_string();
        let drawn: Vec<String> = cards
            .iter()
            .zip(places)
            .map(|(&(c, rev), place)| format!("- {place}: {}{}", card_name(c), if rev { format!(", {reversed}") } else { String::new() }))
            .collect();
        let bbs = self.state.config().bbs.clone();
        let words = if cards.len() == 1 { "80 to 120" } else { "180 to 250" };
        let system = format!(
            "You read tarot on {board}, a bulletin board system. Read the cards drawn for {user}: what each means in its place, \
             upright or reversed, then how they speak together{about}. About {words} words in short paragraphs, plain text for an \
             80-column terminal: no markdown, no emoji, no headings or lists. Be warm, a little mysterious, and grounded. It's for \
             fun and reflection: never foretell death, illness or disaster, give no medical, legal or financial advice, and don't \
             claim to know the future.",
            board = bbs.name,
            user = self.user.handle,
            about = if question.is_empty() { "" } else { " about their question" },
        );
        let prompt = format!(
            "{}Cards:\n{}",
            if question.is_empty() { String::new() } else { format!("Question: {question}\n") },
            drawn.join("\n")
        );
        self.tarot_answer(cfg, &system, &prompt, "TarotWait").await
    }

    /// A fortune cookie: the AI's words and the board's lucky numbers, on
    /// a slip of paper under the crystal ball.
    async fn fortune(&mut self, cfg: &Ai) -> io::Result<()> {
        self.tarot_screen().await?;
        self.newline();
        let left = (80 - PIC_W) / 2 + 1;
        for row in crystal_ball() {
            let y = self.screen.where_y();
            self.screen.goto(left as u16, y);
            self.tarot_cells(&row);
            self.newline();
        }
        self.screen.set_color(Color::DEFAULT);
        self.newline();
        let bbs = self.state.config().bbs.clone();
        let system = format!(
            "Write one fortune-cookie fortune for {user}, a caller on {board}, a bulletin board system: one or two short sentences, \
             wise or witty, now and then with a nod to old BBSes and computers. Plain text only: no quotes around it, nothing else.",
            board = bbs.name,
            user = self.user.handle,
        );
        let Some(text) = self.tarot_answer(cfg, &system, "A fortune, please.", "FortuneWait").await? else { return Ok(()) };
        self.tarot_use("fortune");
        let mut nums: Vec<u32> = (1..=49).collect();
        nums.shuffle(&mut rand::rng());
        let mut lucky: Vec<u32> = nums.into_iter().take(6).collect();
        lucky.sort_unstable();
        let lucky: Vec<String> = lucky.iter().map(|n| format!("{n:>2}")).collect();
        // Plain: it's printed on the slip in the slip's colors.
        let lucky = self.strings().get("FortuneLucky").replace("%S1", &lucky.join("  "));
        // The slip: dark ink on paper, the numbers in red.
        let left = ((80 - SLIP_W) / 2 + 1) as u16;
        let mut lines: Vec<(String, u8)> = vec![(String::new(), 0)];
        for para in text.trim().lines().filter(|l| !l.trim().is_empty()) {
            lines.extend(crate::multinode::wrap(para.trim(), SLIP_W - 8).into_iter().map(|l| (l, 0)));
        }
        lines.push((String::new(), 0));
        lines.push((lucky, 4));
        lines.push((String::new(), 0));
        let edge = |s: &mut Self, c: &str| {
            let y = s.screen.where_y();
            s.screen.goto(left, y);
            s.screen.set_color(Color::new(7, 0));
            s.write(&c.repeat(SLIP_W));
            s.newline();
        };
        edge(self, "▄");
        for (l, k) in lines {
            let y = self.screen.where_y();
            self.screen.goto(left, y);
            self.screen.set_color(Color::new(k, 7));
            self.write(&center(&l, SLIP_W));
            self.newline();
        }
        edge(self, "▀");
        self.screen.set_color(Color::DEFAULT);
        self.newline();
        self.prompt_key().await
    }

    /// Ask the AI, with `wait` showing meanwhile; None (after saying why)
    /// if there's no answer.
    async fn tarot_answer(&mut self, cfg: &Ai, system: &str, prompt: &str, wait: &str) -> io::Result<Option<String>> {
        if self.screen.where_x() != 1 {
            self.newline();
        }
        if !self.ai_count(cfg.daily_limit) {
            let s = self.strings().fmt("TarotLimit", &[&cfg.daily_limit.to_string()]);
            self.show_line(&s).await?;
            self.prompt_key().await?;
            return Ok(None);
        }
        let y = self.screen.where_y();
        self.say(wait).await?;
        self.flush().await?;
        let answer = crate::aimemory::complete(cfg, system, prompt).await;
        self.screen.goto(1, y);
        self.screen.set_color(Color::DEFAULT);
        self.screen.clear_eol();
        match answer {
            Ok(t) if !t.trim().is_empty() => Ok(Some(crate::editor::strip_colours(&t).replace("**", "").trim().to_string())),
            Ok(_) => {
                self.say_line("TarotNothing").await?;
                self.prompt_key().await?;
                Ok(None)
            }
            Err(e) => {
                tracing::warn!("{}: tarot: {e}", self.user.handle);
                let s = self.strings().fmt("AiError", &[&e.to_string()]);
                self.show_line(&s).await?;
                self.prompt_key().await?;
                Ok(None)
            }
        }
    }

    /// A reading under the cards, paged, then a key.
    async fn tarot_show(&mut self, text: &str, after: Option<String>) -> io::Result<()> {
        if self.screen.where_x() != 1 {
            self.newline();
        }
        let mut lines = self.screen.where_y().saturating_sub(1);
        for para in text.lines() {
            let wrapped = if para.trim().is_empty() { vec![String::new()] } else { crate::multinode::wrap(para.trim(), 76) };
            for l in wrapped {
                self.set_slot(Slot::TextHi);
                self.write(&format!("  {l}"));
                self.newline();
                lines += 1;
                if !self.more(&mut lines).await? {
                    return Ok(());
                }
            }
        }
        if let Some(s) = after {
            self.newline();
            self.show_line(&s).await?;
        }
        self.newline();
        self.prompt_key().await
    }

    /// Cells of art: `write_str`, as `write` would drop the suits and
    /// faces (CP437 glyphs below 0x20; the pictures use only the ones
    /// terminals draw).
    fn tarot_cells(&mut self, cells: &[Cell]) {
        let mut last = None;
        for &(c, k) in cells {
            if last != Some(k) {
                self.screen.set_color(Color::new(k, 0));
                last = Some(k);
            }
            self.screen.write_str(&c.to_string());
        }
    }

    /// The cards side by side, each with its place over it.
    fn tarot_cards(&mut self, cards: &[Card], places: &[String]) {
        if self.screen.where_x() != 1 {
            self.newline();
        }
        let n = cards.len().max(1);
        let gap = 3;
        let left = (80 - (n * CARD_W + (n - 1) * gap)) / 2 + 1;
        let reversed = self.strings().get("TarotReversed").to_string();
        let faces: Vec<Vec<Vec<Cell>>> = cards.iter().map(|&(c, rev)| card_face(c, rev, &reversed)).collect();
        let y = self.screen.where_y();
        for i in 0..cards.len() {
            let place = places.get(i).map_or("", |s| s.as_str());
            let label: Vec<Cell> = center(&format!("· {place} ·"), CARD_W).chars().map(|c| (c, if c == '·' { 8 } else { 15 })).collect();
            self.screen.goto((left + i * (CARD_W + gap)) as u16, y);
            self.tarot_cells(&label);
        }
        self.newline();
        for row in 0..CARD_H {
            let y = self.screen.where_y();
            for (i, face) in faces.iter().enumerate() {
                self.screen.goto((left + i * (CARD_W + gap)) as u16, y);
                self.tarot_cells(&face[row]);
            }
            self.newline();
        }
        self.screen.set_color(Color::DEFAULT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_deck_and_a_card_a_day() {
        let names: std::collections::HashSet<String> = (0..DECK).map(card_name).collect();
        assert_eq!(names.len(), 78);
        assert_eq!(card_name(0), "The Fool");
        assert_eq!(card_name(22), "Ace of Wands");
        assert_eq!(card_name(77), "King of Pentacles");
        assert_eq!(card_top(16), "XVI");
        assert_eq!(card_top(50), "swords");
        assert_eq!(card_of_the_day(7, 20_000), card_of_the_day(7, 20_000));
        // Over a month, more than one card comes up.
        let month: std::collections::HashSet<usize> = (0..30).map(|d| card_of_the_day(7, 20_000 + d).0).collect();
        assert!(month.len() > 10 && month.iter().all(|&c| c < DECK));
        let three = draw(3);
        assert_eq!(three.iter().map(|c| c.0).collect::<std::collections::HashSet<_>>().len(), 3);
        assert_eq!(center("ab", 6), "  ab  ");
        for card in [0, 2, 22, 35, 77] {
            let face = card_face(card, card == 35, "reversed");
            assert_eq!(face.len(), CARD_H);
            assert!(face.iter().all(|r| r.len() == CARD_W));
        }
    }
}
