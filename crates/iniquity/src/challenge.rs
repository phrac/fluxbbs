//! The bot check: before anything else, the caller presses a short random
//! sequence of keys shown on screen. Scripts that connect and start
//! typing user names (or nothing) don't get past it. The keys are drawn
//! in a shuffled order and put in place with cursor movements, so the
//! sequence can't simply be read out of the byte stream.

use std::io;
use std::time::Duration;

use iniquity_term::Color;
use rand::{Rng, RngExt};
use rand::seq::SliceRandom;

use crate::input::Key;
use crate::session::Session;

/// Keys a person can find quickly; no I/1, O/0 or Q.
const POOL: &[u8] = b"ABCDEFGHJKLMNPRSTUVWXYZ23456789";

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
}
