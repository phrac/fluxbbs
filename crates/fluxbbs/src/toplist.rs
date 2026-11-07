//! New: the top lists (`LT`), after ENiGMA½'s top X (MIT): the top ten
//! callers, posters, uploaders, downloaders, gamers and point earners
//! this week, in the last 30 days and of all time, side by side, a
//! category a screen. Left and right (or its number) change category.
//! The week and month come from what's logged with a date: the callers
//! log, the messages, the files' upload dates, the download log (kept
//! since the top lists came), door plays and the points ledger; all time
//! from the callers' own counts. Hidden calls and invisible plays are in
//! no log, so they don't count here; bots and deleted callers aren't
//! ranked; posts and uploads count only in areas the viewer can see.

use std::collections::HashMap;
use std::io;

use chrono::NaiveDateTime;
use fluxbbs_term::codes::resize;
use fluxbbs_term::{Slot, lit};

use crate::input::Key;
use crate::messages::io_err;
use crate::session::{Session, waves};
use crate::user::User;

/// Callers on each list.
const TOP: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cat {
    Callers,
    Posters,
    Uploaders,
    Downloaders,
    Gamers,
    Points,
}

impl Cat {
    pub const ALL: [Cat; 6] = [Cat::Callers, Cat::Posters, Cat::Uploaders, Cat::Downloaders, Cat::Gamers, Cat::Points];

    /// By its name or number (`LT posts`, `LT 2`).
    pub fn parse(s: &str) -> Option<Cat> {
        let s = s.trim().to_ascii_lowercase();
        if let Ok(n) = s.parse::<usize>() {
            return Cat::ALL.get(n.checked_sub(1)?).copied();
        }
        let p = |w: &str| !s.is_empty() && w.starts_with(&s[..s.len().min(4)]);
        Cat::ALL.into_iter().find(|c| match c {
            Cat::Callers => p("callers") || p("calls"),
            Cat::Posters => p("posters") || p("posts"),
            Cat::Uploaders => p("uploaders"),
            Cat::Downloaders => p("downloaders"),
            Cat::Gamers => p("gamers") || p("doors"),
            Cat::Points => p("points"),
        })
    }

    fn index(self) -> usize {
        Cat::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }
}

/// One list: (user, handle, figure), best first.
pub type Ranks = Vec<(u32, String, u64)>;

/// Rank `values` (user, figure) among the callers in `users`, those with
/// nothing left out, ties by handle.
pub fn rank(values: impl IntoIterator<Item = (u32, u64)>, users: &HashMap<u32, &User>) -> Ranks {
    let mut v: Ranks = values.into_iter().filter(|&(_, n)| n > 0).filter_map(|(id, n)| users.get(&id).map(|u| (id, u.handle.clone(), n))).collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase())));
    v
}

/// Who's ranked: callers, not bots, not deleted.
fn ranked(u: &User) -> bool {
    u.id != 0 && !u.deleted && !crate::aimemory::is_bot(u)
}

impl Session {
    /// A category's three lists: this week, the last 30 days, all time.
    fn top_lists(&self, cat: Cat, users: &HashMap<u32, &User>) -> anyhow::Result<[Ranks; 3]> {
        let now = crate::clock::now().naive_local();
        let since = [now - chrono::Duration::days(7), now - chrono::Duration::days(30)];
        let by_handle: HashMap<String, u32> = users.values().map(|u| (u.handle.to_ascii_lowercase(), u.id)).collect();
        let s = &self.state;
        let period = |f: &dyn Fn(NaiveDateTime) -> anyhow::Result<Vec<(u32, u64)>>| -> anyhow::Result<[Ranks; 2]> { Ok([rank(f(since[0])?, users), rank(f(since[1])?, users)]) };
        let ever = |f: fn(&User) -> u64| rank(users.values().map(|u| (u.id, f(u))), users);
        let [week, month] = match cat {
            Cat::Callers => period(&|t| s.sys.calls_by_user(t))?,
            Cat::Posters => {
                let areas: Vec<String> = self.readable_areas().into_iter().filter(|(_, a)| !a.is_email()).map(|(_, a)| a.key().to_string()).collect();
                period(&|t| s.msgs.posts_by_user(&areas, t))?
            }
            Cat::Uploaders => {
                let fa = s.fareas();
                let areas: Vec<String> = self.visible_fareas().into_iter().filter_map(|(n, _)| fa.get(n).map(|a| a.file.clone())).collect();
                period(&|t| Ok(s.files.uploads_by_handle(&areas, t)?.into_iter().filter_map(|(h, n)| Some((*by_handle.get(&h.to_ascii_lowercase())?, n))).collect()))?
            }
            Cat::Downloaders => period(&|t| s.sys.downloads_by_user(t))?,
            Cat::Gamers => period(&|t| s.sys.door_minutes_by_user(Some(t)))?,
            Cat::Points => period(&|t| s.sys.points_got_by_user(t))?,
        };
        let all = match cat {
            Cat::Callers => ever(|u| u.total_calls as u64),
            Cat::Posters => ever(|u| u.posts as u64),
            Cat::Uploaders => ever(|u| u.uploads as u64),
            Cat::Downloaders => ever(|u| u.downloads as u64),
            Cat::Gamers => rank(s.sys.door_minutes_by_user(None)?, users),
            Cat::Points => ever(|u| u.file_points as u64),
        };
        Ok([week, month, all])
    }

    /// `LT` (new): the top lists, starting at the category given (a name
    /// or number; callers if none). TOPL TOP/MID/BOT dress it.
    pub async fn top_list(&mut self, param: &str) -> io::Result<()> {
        let mut cats: Vec<Cat> = Cat::ALL.to_vec();
        if !self.state.config().files.use_file_points {
            cats.retain(|&c| c != Cat::Points);
        }
        let all = self.state.users.list().map_err(io_err)?;
        let users: HashMap<u32, &User> = all.iter().filter(|u| ranked(u)).map(|u| (u.id, u)).collect();
        let mut at = Cat::parse(param).and_then(|c| cats.iter().position(|&x| x == c)).unwrap_or(0);
        // Plain text: every category, one after another.
        if self.plain() {
            let only = Cat::parse(param).filter(|c| cats.contains(c));
            for cat in cats.clone().into_iter().filter(|c| only.is_none_or(|o| o == *c)) {
                let lists = self.top_lists(cat, &users).map_err(io_err)?;
                self.top_draw(cat, &lists).await?;
            }
            return Ok(());
        }
        loop {
            let cat = cats[at];
            let lists = self.top_lists(cat, &users).map_err(io_err)?;
            self.top_draw(cat, &lists).await?;
            let keys = self.strings().fmt("TopKeys", &[&cats.len().to_string()]);
            self.show(&keys).await?;
            loop {
                let k = self.read_key().await?;
                let next = match k {
                    Key::Right | Key::Down | Key::Char(b' ') | Key::Tab => (at + 1) % cats.len(),
                    Key::Left | Key::Up => (at + cats.len() - 1) % cats.len(),
                    Key::Char(c @ b'1'..=b'9') if ((c - b'1') as usize) < cats.len() => (c - b'1') as usize,
                    Key::Enter | Key::Esc | Key::Char(b'q' | b'Q') => {
                        self.newline();
                        self.full_screen_done();
                        return Ok(());
                    }
                    _ => continue,
                };
                if next != at {
                    at = next;
                    break;
                }
            }
        }
    }

    /// A category's lists on the screen.
    async fn top_draw(&mut self, cat: Cat, lists: &[Ranks; 3]) -> io::Result<()> {
        let s = self.strings();
        let names: Vec<String> = s.get("TopCats").split('|').map(String::from).collect();
        let name = names.get(cat.index()).cloned().unwrap_or_default();
        let mut heads = ["TopWeek", "TopMonth", "TopEver"].map(|h| s.get(h).to_string());
        if cat == Cat::Points {
            heads[2] = s.get("TopHeld").to_string();
        }
        drop(s);
        let me = self.user.id;
        let yours: Vec<String> = lists.iter().map(|l| l.iter().position(|e| e.0 == me).map_or("-".into(), |i| format!("#{}", i + 1))).collect();
        let fig = |n: u64| if cat == Cat::Gamers { crate::doorstats::span(n) } else { crate::files::commas(n) };
        let cell = |l: &Ranks, i: usize| match l.get(i) {
            Some((id, h, n)) => (format!("{}{}", if *id == me { '\u{00bb}' } else { ' ' }, i + 1), h.clone(), fig(*n)),
            None => (String::new(), String::new(), String::new()),
        };
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        if !self.plain() {
            self.clear_screen();
        }
        let top = waves([
            ("CT", name.clone()),
            ("YR", yours.join(" ")),
            ("W1", heads[0].clone()),
            ("W2", heads[1].clone()),
            ("W3", heads[2].clone()),
        ]);
        let mut rep = self.repeat_begin_with("TOPL", false, &top).await?;
        if rep.is_none() {
            self.newline();
            let t = self.strings().fmt("TopTitle", &[&name]);
            self.show_line(&t).await?;
            self.newline();
            self.set_slot(Slot::Info);
            self.write(&heads.iter().map(|h| format!("  {}", resize(h, 24))).collect::<String>());
            self.newline();
        }
        let rows = lists.iter().map(Vec::len).max().unwrap_or(0).clamp(1, TOP);
        for i in 0..rows {
            let c: Vec<(String, String, String)> = lists.iter().map(|l| cell(l, i)).collect();
            match rep.as_mut() {
                Some(r) => {
                    let w = waves([
                        ("R1", c[0].0.clone()),
                        ("H1", c[0].1.clone()),
                        ("V1", format!("{:>7}", c[0].2)),
                        ("R2", c[1].0.clone()),
                        ("H2", c[1].1.clone()),
                        ("V2", format!("{:>7}", c[1].2)),
                        ("R3", c[2].0.clone()),
                        ("H3", c[2].1.clone()),
                        ("V3", format!("{:>7}", c[2].2)),
                    ]);
                    self.repeat_row(r, &w, false).await?;
                }
                None => {
                    let mut line = String::new();
                    for (j, (r, h, v)) in c.iter().enumerate() {
                        let hi = lists[j].get(i).is_some_and(|e| e.0 == me);
                        line.push_str(&format!("  {}{:>3} {} {:>7}", if hi { "|U6" } else { "|U3" }, r, lit(&resize(h, 12)), v));
                    }
                    self.show_line(&line).await?;
                }
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => {
                let s = self.strings().fmt("TopYours", &[&yours[0], &yours[1], &yours[2]]);
                self.newline();
                self.show_line(&s).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_by_name_or_number() {
        assert_eq!(Cat::parse("2"), Some(Cat::Posters));
        assert_eq!(Cat::parse("posts"), Some(Cat::Posters));
        assert_eq!(Cat::parse("Doors"), Some(Cat::Gamers));
        assert_eq!(Cat::parse("down"), Some(Cat::Downloaders));
        assert_eq!(Cat::parse(""), None);
        assert_eq!(Cat::parse("9"), None);
    }

    #[test]
    fn ranks_leave_out_who_isnt_listed() {
        let mk = |id: u32, h: &str| User { id, handle: h.into(), ..User::default() };
        let (a, b, c) = (mk(1, "zed"), mk(2, "amy"), mk(3, "bob"));
        let users: HashMap<u32, &User> = [(1, &a), (2, &b), (3, &c)].into_iter().collect();
        // 9 isn't a caller (a bot, deleted); 3 has nothing.
        let r = rank([(1, 5), (2, 5), (9, 50), (3, 0)], &users);
        assert_eq!(r, vec![(2, "amy".into(), 5), (1, "zed".into(), 5)]);
    }
}
