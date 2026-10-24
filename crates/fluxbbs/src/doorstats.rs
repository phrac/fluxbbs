//! New: the doors menu's figures (`%G?`), from a log of every door played:
//! who played what, when and for how long.

use std::sync::Arc;

use chrono::{Datelike, NaiveDateTime, Timelike};

use crate::mci::ago;
use crate::session::Session;
use crate::sysdata::{DoorStats, Play};

/// The second letters of the door codes.
pub(crate) const DOOR_STATS: &[u8] = b"ACDEFGHJKLNOPQUVWXYZ";

/// Minutes as "45m", "3h 05m", or past 100 hours "120h".
pub(crate) fn span(min: u64) -> String {
    match min {
        0..=59 => format!("{min}m"),
        60..=5999 => format!("{}h {:02}m", min / 60, min % 60),
        _ => format!("{}h", min / 60),
    }
}

/// How long ago, down to the minute on the day.
pub(crate) fn ago_time(at: NaiveDateTime, now: NaiveDateTime) -> String {
    match (now - at).num_minutes() {
        ..=0 => "just now".into(),
        m @ 1..=59 => format!("{m}m ago"),
        m @ 60..=1439 => format!("{}h ago", m / 60),
        _ => ago(Some(at.date()), now.date()),
    }
}

/// A door command (`D0`-`D7`, `DZ`, BBSLink's `DB`).
fn is_door(cmd: &str) -> bool {
    let c = cmd.as_bytes();
    c.len() == 2 && c[0].eq_ignore_ascii_case(&b'D') && (c[1].is_ascii_digit() || b"ZzBb".contains(&c[1]))
}

impl Session {
    /// The figures, worked out again once they're a minute old, or after
    /// the caller plays something.
    fn door_stats(&self) -> Arc<DoorStats> {
        let cached = self.door_stats.borrow().as_ref().filter(|(at, _)| at.elapsed().as_secs() < 60).map(|(_, s)| s.clone());
        cached.unwrap_or_else(|| {
            let day = crate::clock::now().date_naive().and_hms_opt(0, 0, 0).unwrap_or_default();
            let s = Arc::new(self.state.sys.door_stats(self.user.id, day).unwrap_or_default());
            *self.door_stats.borrow_mut() = Some((std::time::Instant::now(), s.clone()));
            s
        })
    }

    /// Log a play (not on a hidden call: it would show who was on).
    pub(crate) fn record_play(&mut self, door: &str, started: NaiveDateTime) {
        if self.user.id == 0 || self.hidden_call {
            return;
        }
        let minutes = (crate::clock::now().naive_local() - started).num_minutes().max(0) as u32;
        let started = started.with_nanosecond(0).unwrap_or(started);
        let p = Play { handle: self.user.handle.clone(), door: door.into(), started, minutes };
        if let Err(e) = self.state.sys.add_play(self.user.id, &p) {
            tracing::warn!("door play: {e}");
        }
        *self.door_stats.borrow_mut() = None;
    }

    /// The doors on the current menu the caller can run, by description.
    fn menu_doors(&self) -> Vec<String> {
        let Some(m) = &self.menu.menu else { return Vec::new() };
        let mut v: Vec<String> = Vec::new();
        for c in &m.commands {
            let d = c.desc.trim();
            if is_door(&c.command) && !c.hidden && !d.is_empty() && self.acs(&c.acs) && !v.iter().any(|x| x.eq_ignore_ascii_case(d)) {
                v.push(d.to_string());
            }
        }
        v
    }

    /// Who's in a door now: "handle in DOOR", "+N" for more.
    fn now_playing(&self) -> String {
        let sysop = self.is_sysop();
        let me = self.node.number();
        let playing: Vec<String> = self
            .state
            .nodes
            .online()
            .into_iter()
            .filter(|(n, i)| *n != me && (sysop || !i.invisible) && !i.user.is_empty())
            .filter_map(|(_, i)| i.activity.strip_prefix("Running ").filter(|d| *d != "a door").map(|d| format!("{} in {d}", i.user)))
            .collect();
        match playing.len() {
            0 => "nobody".into(),
            1 => playing[0].clone(),
            n => format!("{} +{}", playing[0], n - 1),
        }
    }

    /// `%G?`: the last door played `L`, by whom `W`, how long ago `A`;
    /// the most played `P` and its plays `C`; who has spent the most time
    /// in doors `K` and how long `Q`; all the time spent in doors `H`;
    /// plays today `N`; the longest play's door `X`, player `J` and length
    /// `Z`; who's in a door now `O`. Of the doors on the current menu, the
    /// one left longest unplayed `D` and when it last was `G`, and one
    /// the caller hasn't tried `E`. The caller's own: favorite `F`, plays
    /// `Y`, last door `U` and when `V`.
    pub(crate) fn door_stat(&self, c: u8) -> String {
        let s = self.door_stats();
        let now = crate::clock::now().naive_local();
        let nothing = || "nothing yet".to_string();
        // Something from today's list that stays put through the call.
        let pick = |v: &[&String]| v.get((now.ordinal() as usize + self.user.id as usize) % v.len().max(1)).map(|d| d.to_string());
        match c {
            b'L' => s.last.as_ref().map_or_else(nothing, |p| p.door.clone()),
            b'W' => s.last.as_ref().map(|p| p.handle.clone()).unwrap_or_default(),
            b'A' => s.last.as_ref().map(|p| ago_time(p.started, now)).unwrap_or_default(),
            b'P' => s.popular.as_ref().map_or_else(nothing, |p| p.0.clone()),
            b'C' => s.popular.as_ref().map_or(0, |p| p.1).to_string(),
            b'K' => s.top_player.as_ref().map_or_else(|| "nobody yet".into(), |p| p.0.clone()),
            b'Q' => span(s.top_player.as_ref().map_or(0, |p| p.1)),
            b'H' => span(s.minutes),
            b'N' => s.plays_today.to_string(),
            b'X' => s.marathon.as_ref().map_or_else(nothing, |p| p.door.clone()),
            b'J' => s.marathon.as_ref().map(|p| p.handle.clone()).unwrap_or_default(),
            b'Z' => s.marathon.as_ref().map(|p| span(p.minutes as u64)).unwrap_or_default(),
            b'O' => self.now_playing(),
            b'D' | b'G' => {
                let doors = self.menu_doors();
                let when = |d: &String| s.last_by_door.get(&d.to_uppercase()).copied();
                let never: Vec<&String> = doors.iter().filter(|d| when(d).is_none()).collect();
                let dusty = pick(&never).map(|d| (d, None)).or_else(|| doors.iter().min_by_key(|d| when(d)).map(|d| (d.clone(), when(d))));
                match (c, dusty) {
                    (b'D', Some((d, _))) => d,
                    (_, Some((_, Some(at)))) => ago_time(at, now),
                    (_, Some((_, None))) => "never played".into(),
                    _ => String::new(),
                }
            }
            b'E' => {
                let doors = self.menu_doors();
                let untried: Vec<&String> = doors.iter().filter(|d| !s.my_doors.contains(&d.to_uppercase())).collect();
                pick(&untried).unwrap_or_else(|| "nothing left".into())
            }
            b'F' => s.my_favorite.as_ref().map_or_else(nothing, |p| p.0.clone()),
            b'Y' => s.my_plays.to_string(),
            b'U' => s.my_last.as_ref().map_or_else(nothing, |p| p.door.clone()),
            b'V' => s.my_last.as_ref().map(|p| ago_time(p.started, now)).unwrap_or_default(),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(span(45), "45m");
        assert_eq!(span(185), "3h 05m");
        assert_eq!(span(7200), "120h");
        let at = |d, h, m| chrono::NaiveDate::from_ymd_opt(2026, 10, d).unwrap().and_hms_opt(h, m, 0).unwrap();
        assert_eq!(ago_time(at(24, 10, 0), at(24, 10, 0)), "just now");
        assert_eq!(ago_time(at(24, 9, 48), at(24, 10, 0)), "12m ago");
        assert_eq!(ago_time(at(24, 1, 0), at(24, 10, 0)), "9h ago");
        assert_eq!(ago_time(at(20, 1, 0), at(24, 10, 0)), "4d ago");
        assert!(is_door("D1") && is_door("dz") && !is_door("DA") && !is_door("-/"));
    }
}
