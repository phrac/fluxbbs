//! New: the tarot and fortune door (`AT`), read by the board's AI. The
//! board draws the cards (the 78 of the Rider-Waite deck, each upright or
//! reversed) and the AI only reads them. The card of the day is the same
//! card all day for a caller; three cards (past, present, future) answer a
//! question; a fortune is a fortune cookie with lucky numbers. TAROT.ANS
//! heads the screen if the board has one. Each reading counts against the
//! AI chat's daily limit.

use std::io;

use fluxbbs_term::Slot;
use rand::seq::SliceRandom;

use crate::config::Ai;
use crate::session::Session;

const MAJOR: [&str; 22] = [
    "The Fool", "The Magician", "The High Priestess", "The Empress", "The Emperor", "The Hierophant", "The Lovers", "The Chariot",
    "Strength", "The Hermit", "Wheel of Fortune", "Justice", "The Hanged Man", "Death", "Temperance", "The Devil", "The Tower",
    "The Star", "The Moon", "The Sun", "Judgement", "The World",
];
const RANKS: [&str; 14] = ["Ace", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten", "Page", "Knight", "Queen", "King"];
const SUITS: [&str; 4] = ["Wands", "Cups", "Swords", "Pentacles"];
const ROMAN: [&str; 22] = ["0", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV", "XV", "XVI", "XVII", "XVIII", "XIX", "XX", "XXI"];
const DECK: usize = 78;
/// A card's width on screen, frame and all.
const CARD_W: usize = 22;

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
                    self.say("TarotMenu").await?;
                    let k = self.read_key().await?;
                    if k == crate::input::Key::Esc {
                        break;
                    }
                    k.upper().unwrap_or(' ')
                }
            };
            match pick {
                'D' => {
                    let day = crate::clock::now().date_naive().signed_duration_since(chrono::NaiveDate::default()).num_days();
                    let place = self.strings().get("TarotToday").to_string();
                    self.tarot_reading(&cfg, &[card_of_the_day(self.user.id, day)], &[place], "").await?;
                }
                'T' => {
                    self.tarot_screen().await?;
                    self.newline();
                    self.say("TarotAsk").await?;
                    let question = self.read_string(crate::input::ReadOpts::new(60)).await?;
                    if self.hangup {
                        break;
                    }
                    let places = ["TarotPast", "TarotPresent", "TarotFuture"].map(|s| self.strings().get(s).to_string());
                    self.tarot_reading(&cfg, &draw(3), &places, question.trim()).await?;
                }
                'F' => self.fortune(&cfg).await?,
                'Q' => break,
                _ => continue,
            }
            if once.is_some() || self.hangup {
                break;
            }
        }
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        Ok(())
    }

    async fn tarot_screen(&mut self) -> io::Result<()> {
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        if !self.show_file("TAROT").await? {
            self.say_line("TarotTitle").await?;
        }
        Ok(())
    }

    /// The cards, then what the AI makes of them.
    async fn tarot_reading(&mut self, cfg: &Ai, cards: &[Card], places: &[String], question: &str) -> io::Result<()> {
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
        self.tarot_ask(cfg, &system, &prompt).await
    }

    /// A fortune cookie: the AI's line and the board's lucky numbers.
    async fn fortune(&mut self, cfg: &Ai) -> io::Result<()> {
        self.tarot_screen().await?;
        self.newline();
        let bbs = self.state.config().bbs.clone();
        let system = format!(
            "Write one fortune-cookie fortune for {user}, a caller on {board}, a bulletin board system: one or two short sentences, \
             wise or witty, now and then with a nod to old BBSes and computers. Plain text only: no quotes around it, nothing else.",
            board = bbs.name,
            user = self.user.handle,
        );
        let mut nums: Vec<u32> = (1..=49).collect();
        nums.shuffle(&mut rand::rng());
        let mut lucky: Vec<u32> = nums.into_iter().take(6).collect();
        lucky.sort_unstable();
        let lucky: Vec<String> = lucky.iter().map(u32::to_string).collect();
        let s = self.strings().fmt("TarotLucky", &[&lucky.join(" ")]);
        self.tarot_ask_then(cfg, &system, "A fortune, please.", Some(s)).await
    }

    async fn tarot_ask(&mut self, cfg: &Ai, system: &str, prompt: &str) -> io::Result<()> {
        self.tarot_ask_then(cfg, system, prompt, None).await
    }

    /// Ask the AI and show its answer (and `after`, a line of the board's
    /// own), then wait for a key.
    async fn tarot_ask_then(&mut self, cfg: &Ai, system: &str, prompt: &str, after: Option<String>) -> io::Result<()> {
        if self.screen.where_x() != 1 {
            self.newline();
        }
        if !self.ai_count(cfg.daily_limit) {
            let s = self.strings().fmt("TarotLimit", &[&cfg.daily_limit.to_string()]);
            self.show_line(&s).await?;
            return self.prompt_key().await;
        }
        let y = self.screen.where_y();
        self.say("TarotWait").await?;
        self.flush().await?;
        let answer = crate::aimemory::complete(cfg, system, prompt).await;
        self.screen.goto(1, y);
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
        let text = match answer {
            Ok(t) if !t.trim().is_empty() => t,
            Ok(_) => {
                self.say_line("TarotNothing").await?;
                return self.prompt_key().await;
            }
            Err(e) => {
                tracing::warn!("{}: tarot: {e}", self.user.handle);
                let s = self.strings().fmt("AiError", &[&e.to_string()]);
                self.show_line(&s).await?;
                return self.prompt_key().await;
            }
        };
        let text = crate::editor::strip_colours(&text).replace("**", "");
        let mut lines = y.saturating_sub(1);
        self.set_slot(Slot::TextHi);
        for para in text.trim().lines() {
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

    /// The cards side by side, each with its place over it.
    fn tarot_cards(&mut self, cards: &[Card], places: &[String]) {
        if self.screen.where_x() != 1 {
            self.newline();
        }
        self.newline();
        let n = cards.len().max(1);
        let gap = 3;
        let left = (80 - (n * CARD_W + (n - 1) * gap)) / 2 + 1;
        let inner = CARD_W - 2;
        let reversed = self.strings().get("TarotReversed").to_string();
        for row in 0..9 {
            let y = self.screen.where_y();
            for (i, &(c, rev)) in cards.iter().enumerate() {
                self.screen.goto((left + i * (CARD_W + gap)) as u16, y);
                let name = crate::multinode::wrap(&card_name(c), inner - 2);
                let (slot, text) = match row {
                    0 => (Slot::Info, center(places.get(i).map_or("", |s| s.as_str()), CARD_W)),
                    1 => (Slot::Border, format!("┌{}┐", "─".repeat(inner))),
                    8 => (Slot::Border, format!("└{}┘", "─".repeat(inner))),
                    _ => {
                        let (slot, s) = match row {
                            2 => (Slot::InfoHi, card_top(c)),
                            4 => (Slot::TextHi, name.first().cloned().unwrap_or_default()),
                            5 => (Slot::TextHi, name.get(1).cloned().unwrap_or_default()),
                            7 if rev => (Slot::InfoLo, reversed.clone()),
                            _ => (Slot::Text, String::new()),
                        };
                        self.set_slot(Slot::Border);
                        self.write("│");
                        self.set_slot(slot);
                        self.write(&center(&s, inner));
                        self.set_slot(Slot::Border);
                        self.write("│");
                        continue;
                    }
                };
                self.set_slot(slot);
                self.write(&text);
            }
            self.newline();
        }
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
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
    }
}
