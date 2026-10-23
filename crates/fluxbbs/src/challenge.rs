//! The bot check: before anything else, the caller presses a short random
//! sequence of keys shown on screen. Scripts that connect and start
//! typing user names (or nothing) don't get past it. The keys are drawn
//! in a shuffled order and put in place with cursor movements, so the
//! sequence can't simply be read out of the byte stream.
//!
//! New, the picture check (`challenge_visual`): three big characters,
//! drawn not with letters but with background colour, in a field of
//! noise made of the same symbols inside and out. A person reads them at
//! a glance; a script reading the text, even through a terminal
//! emulator, gets only noise. The colours change every time, each
//! character sits a row higher or lower, some cells are flipped, the
//! field is drawn in shuffled pieces, an invisible decoy in the old
//! "press these keys" form comes first, and an answer quicker than a
//! person could read counts as wrong.

use std::io;
use std::time::{Duration, Instant};

use fluxbbs_term::Color;
use rand::{Rng, RngExt};
use rand::seq::SliceRandom;

use crate::input::Key;
use crate::session::Session;

/// Keys a person can find quickly; no I/1, O/0 or Q.
const POOL: &[u8] = b"ABCDEFGHJKLMNPRSTUVWXYZ23456789";

/// The picture check's characters, 5 by 7 (`#` lit), each pixel drawn two
/// cells wide. None that look alike at this size (B/8, S/5, Z/2, G/6...).
pub const FONT: &[(char, [&str; 7])] = &[
    ('A', [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('C', [" ####", "#    ", "#    ", "#    ", "#    ", "#    ", " ####"]),
    ('E', ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"]),
    ('F', ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "]),
    ('H', ["#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('J', ["  ###", "   # ", "   # ", "   # ", "   # ", "#  # ", " ##  "]),
    ('K', ["#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"]),
    ('L', ["#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"]),
    ('M', ["#   #", "## ##", "# # #", "# # #", "#   #", "#   #", "#   #"]),
    ('N', ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"]),
    ('P', ["#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "]),
    ('R', ["#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"]),
    ('T', ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('U', ["#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('V', ["#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  "]),
    ('W', ["#   #", "#   #", "#   #", "# # #", "# # #", "## ##", "#   #"]),
    ('X', ["#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"]),
    ('Y', ["#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('3', ["#### ", "    #", "    #", " ### ", "    #", "    #", "#### "]),
    ('4', ["#   #", "#   #", "#   #", "#####", "    #", "    #", "    #"]),
    ('7', ["#####", "    #", "   # ", "  #  ", "  #  ", "  #  ", "  #  "]),
];
/// The picture field: three characters with room around them. A
/// BOTCHECK screen marks its top left with `~BC` and the answer with
/// `~BP`.
pub const FIELD_W: u16 = 40;
pub const FIELD_H: u16 = 9;
/// Symbols the field is made of, the same inside the characters as out.
/// (No shading blocks: in colour they look like lit cells.)
const NOISE: &[char] = &['·', ':', ';', '+', '*', '=', '#', '&', '$', '<', '>', '/', '\\', '-', '"', '\''];
/// Quicker than this from the picture to the last key is a script.
const HUMAN_MS: u128 = 600;

/// Which cells of the field are lit for `chars`, each nudged by its row
/// offset (0..=2), with `flip` cells (field indices) turned over.
pub fn field_cells(chars: &[char], tops: &[u16], flip: &[usize]) -> Vec<bool> {
    let mut lit = vec![false; (FIELD_W * FIELD_H) as usize];
    for (i, (c, top)) in chars.iter().zip(tops).enumerate() {
        let Some((_, rows)) = FONT.iter().find(|(f, _)| f == c) else { continue };
        let x0 = 2 + i as u16 * 13;
        for (r, row) in rows.iter().enumerate() {
            for (k, b) in row.bytes().enumerate() {
                if b == b'#' {
                    let (x, y) = (x0 + k as u16 * 2, top + r as u16);
                    for dx in 0..2 {
                        lit[(y * FIELD_W + x + dx) as usize] = true;
                    }
                }
            }
        }
    }
    for &f in flip {
        if let Some(c) = lit.get_mut(f) {
            *c = !*c;
        }
    }
    lit
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ChallengeKey {
    Esc,
    Char(char),
}

impl ChallengeKey {
    fn label(self) -> String {
        match self {
            ChallengeKey::Esc => "ESC".into(),
            ChallengeKey::Char(c) => c.to_string(),
        }
    }

    fn matches(self, key: Key) -> bool {
        match self {
            ChallengeKey::Esc => key == Key::Esc,
            ChallengeKey::Char(c) => key.upper() == Some(c),
        }
    }
}

/// `n` keys: letters and digits, with at most one ESC.
pub fn pick(n: usize, rng: &mut impl Rng) -> Vec<ChallengeKey> {
    let mut keys: Vec<ChallengeKey> =
        (0..n).map(|_| ChallengeKey::Char(POOL[rng.random_range(0..POOL.len())] as char)).collect();
    if n > 1 && rng.random_bool(0.3) {
        let i = rng.random_range(0..n);
        keys[i] = ChallengeKey::Esc;
    }
    keys
}

impl Session {
    /// Run the bot check. True if the caller passed (or doesn't have to
    /// take it).
    pub async fn bot_check(&mut self) -> io::Result<bool> {
        let sec = self.state.config().security.clone();
        let trusted = self.caller.remote.is_some_and(|a| self.state.guard.trusted(&sec, a.ip()));
        if !sec.challenge || trusted || self.caller.transport == "local" {
            return Ok(true);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(sec.challenge_seconds.max(5) as u64);
        if sec.challenge_visual {
            return self.picture_check(deadline, sec.challenge_tries.max(1)).await;
        }
        let count = sec.challenge_keys.clamp(1, 8) as usize;
        for attempt in 0..sec.challenge_tries.max(1) {
            let keys = pick(count, &mut rand::rng());
            self.newline();
            if attempt > 0 {
                self.say_line("BotCheckAgain").await?;
            }
            self.say("BotCheck").await?;
            self.draw_challenge(&keys);
            self.screen.set_color(Color::new(7, 0));
            self.write("  ");
            // Whatever was typed before the keys appeared doesn't count.
            self.take_pending_input();
            // Take as many keys as were asked for before judging, so a
            // slip on the first doesn't spill the rest into the next go.
            let mut ok = true;
            for k in &keys {
                let got = match tokio::time::timeout_at(deadline, self.read_key()).await {
                    Ok(r) => r?,
                    Err(_) => {
                        self.newline();
                        self.say_line("BotCheckTimeout").await?;
                        return Ok(false);
                    }
                };
                self.write("*");
                ok &= k.matches(got);
            }
            if ok {
                self.newline();
                return Ok(true);
            }
            self.newline();
        }
        self.say_line("BotCheckFailed").await?;
        Ok(false)
    }

    /// The picture check: draw, read three keys (echoing `*`), judge.
    async fn picture_check(&mut self, deadline: tokio::time::Instant, tries: u8) -> io::Result<bool> {
        for attempt in 0..tries {
            let mut pool: Vec<char> = FONT.iter().map(|(c, _)| *c).collect();
            pool.shuffle(&mut rand::rng());
            let (chars, decoy) = (pool[..3].to_vec(), &pool[3..6]);
            // For scripts that read the text: the old form, unseen, with
            // the wrong keys.
            self.screen.set_color(Color::new(0, 0));
            self.write(&format!("[bot check] Press these keys, in order: [{}] [{}] [{}]", decoy[0], decoy[1], decoy[2]));
            self.screen.set_color(Color::DEFAULT);
            self.clear_screen();
            let (field, prompt) = self.picture_layout().await?;
            if attempt > 0 {
                self.screen.goto(prompt.0, prompt.1.saturating_sub(1));
                self.say("BotCheckAgain").await?;
            }
            self.draw_field(&chars, field.0, field.1);
            self.screen.goto(prompt.0, prompt.1);
            self.screen.set_color(Color::DEFAULT);
            let ask = self.strings().fmt("BotCheckPicture", &["3"]);
            self.show(&ask).await?;
            self.take_pending_input();
            self.flush().await?;
            let shown = Instant::now();
            let mut ok = true;
            for c in &chars {
                let got = match tokio::time::timeout_at(deadline, self.read_key()).await {
                    Ok(r) => r?,
                    Err(_) => {
                        self.newline();
                        self.say_line("BotCheckTimeout").await?;
                        return Ok(false);
                    }
                };
                self.write("*");
                ok &= got.upper() == Some(*c);
            }
            let ms = shown.elapsed().as_millis();
            if ok && ms < HUMAN_MS {
                tracing::info!("bot check answered in {ms} ms: too quick for a person");
                ok = false;
            }
            if ok {
                self.screen.set_color(Color::DEFAULT);
                self.clear_screen();
                return Ok(true);
            }
        }
        self.newline();
        self.say_line("BotCheckFailed").await?;
        Ok(false)
    }

    /// Where the field and the answer go: the BOTCHECK screen's `~BC` and
    /// `~BP`, or a frame of our own.
    async fn picture_layout(&mut self) -> io::Result<((u16, u16), (u16, u16))> {
        if self.show_file("BOTCHECK").await?
            && let Some(&(x, y, _)) = self.positions.get(b"BC")
        {
            let prompt = self.positions.get(b"BP").map_or((x, y + FIELD_H + 1), |p| (p.0, p.1));
            return Ok(((x, y), prompt));
        }
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        let (x, y) = ((80 - FIELD_W - 2) / 2, 4);
        let title = " bot check ";
        self.screen.goto(x, y - 1);
        self.screen.set_color(Color::new(8, 0));
        self.write("┌─[");
        self.screen.set_color(Color::new(15, 0));
        self.write(title);
        self.screen.set_color(Color::new(8, 0));
        self.write(&format!("]{}┐", "─".repeat(FIELD_W as usize - title.len() - 3)));
        for r in 0..FIELD_H {
            self.screen.goto(x, y + r);
            self.write("│");
            self.screen.goto(x + FIELD_W + 1, y + r);
            self.write("│");
        }
        self.screen.goto(x, y + FIELD_H);
        self.write(&format!("└{}┘", "─".repeat(FIELD_W as usize)));
        Ok(((x + 1, y), (x + 1, y + FIELD_H + 2)))
    }

    /// The field at (`x`, `y`): characters by background colour, the
    /// rest noise, drawn in pieces in a random order.
    fn draw_field(&mut self, chars: &[char], x: u16, y: u16) {
        let mut rng = rand::rng();
        let tops: Vec<u16> = chars.iter().map(|_| rng.random_range(0..=2)).collect();
        let flip: Vec<usize> = (0..(FIELD_W * FIELD_H) as usize).filter(|_| rng.random_bool(0.03)).collect();
        let lit = field_cells(chars, &tops, &flip);
        let back = rng.random_range(1..=6u8);
        let mut pieces = Vec::new();
        for r in 0..FIELD_H {
            let mut c = 0;
            while c < FIELD_W {
                let n = rng.random_range(2..=7).min(FIELD_W - c);
                pieces.push((r, c, n));
                c += n;
            }
        }
        pieces.shuffle(&mut rng);
        for (r, c, n) in pieces {
            self.screen.goto(x + c, y + r);
            for k in c..c + n {
                let colour = if lit[(r * FIELD_W + k) as usize] {
                    Color::new([15, 7, 0][rng.random_range(0..3)], back)
                } else {
                    Color::new([8, 1, 2, 3, 4, 5, 6][rng.random_range(0..7)], 0)
                };
                self.screen.set_color(colour);
                self.write(&NOISE[rng.random_range(0..NOISE.len())].to_string());
            }
        }
        self.screen.set_color(Color::DEFAULT);
    }

    /// Draw `[K] [7] [ESC]`, each in a colour of its own, writing the
    /// boxes in a random order.
    fn draw_challenge(&mut self, keys: &[ChallengeKey]) {
        let mut rng = rand::rng();
        let start = self.screen.where_x();
        let mut offsets = Vec::new();
        let mut x = start;
        for k in keys {
            offsets.push(x);
            x += k.label().len() as u16 + 3;
        }
        let end = x;
        let mut order: Vec<usize> = (0..keys.len()).collect();
        order.shuffle(&mut rng);
        const COLOURS: [u8; 6] = [10, 11, 12, 13, 14, 15];
        for i in order {
            self.screen.pos_x(offsets[i]);
            self.screen.set_color(Color::new(8, 0));
            self.write("[");
            self.screen.set_color(Color::new(COLOURS[rng.random_range(0..COLOURS.len())], 0));
            self.write(&keys[i].label());
            self.screen.set_color(Color::new(8, 0));
            self.write("]");
        }
        self.screen.pos_x(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_and_matches() {
        let mut rng = rand::rng();
        for _ in 0..200 {
            let keys = pick(3, &mut rng);
            assert_eq!(keys.len(), 3);
            assert!(keys.iter().filter(|k| **k == ChallengeKey::Esc).count() <= 1);
            for k in keys {
                if let ChallengeKey::Char(c) = k {
                    assert!(!"IOQ01".contains(c));
                }
            }
        }
        assert!(ChallengeKey::Char('K').matches(Key::Char(b'k')));
        assert!(!ChallengeKey::Char('K').matches(Key::Char(b'J')));
        assert!(ChallengeKey::Esc.matches(Key::Esc));
    }

    #[test]
    fn font_and_field() {
        for (c, rows) in FONT {
            assert!(rows.iter().all(|r| r.len() == 5), "{c}");
            assert!(!"IOQ01B8S5Z2G6".contains(*c));
        }
        let lit = field_cells(&['T', 'L', '7'], &[0, 2, 1], &[]);
        // T's bar along the top row, two cells a pixel.
        assert!((2..12).all(|x| lit[x as usize]));
        // L's foot on the bottom row (7 rows down from 2).
        let row = 8 * FIELD_W as usize;
        assert!((15..25).all(|x| lit[row + x]));
        assert!(!lit[0] && !lit[FIELD_W as usize - 1]);
        let flipped = field_cells(&['T', 'L', '7'], &[0, 2, 1], &[0, 2]);
        assert!(flipped[0] && !flipped[2]);
    }
}
