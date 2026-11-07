//! New: achievements (`UA`), after ENiGMA½'s (MIT): milestones callers
//! earn as their figures grow (posts, uploads, downloads, calls, a run
//! of days called, doors played, oneliners, votes, requests filled, days
//! a member). Each tier has a title and maybe points, and is earned
//! once. They're worked out at the menus (a safe point: never in the
//! editor or a door), and told there, on the help line. What a caller
//! had done before the board had achievements (or before a SysOp added
//! one) is given quietly, without points (`back_points`), and told as
//! one line. `achievements.toml` holds them; without it, the stock set.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::sync::Mutex;

use chrono::{NaiveDate, NaiveDateTime};
use fluxbbs_term::codes::resize;
use fluxbbs_term::{Slot, lit};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::messages::io_err;
use crate::session::{Session, waves};
use crate::user::User;

/// What an achievement counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    Posts,
    Uploads,
    Downloads,
    Calls,
    /// The most days in a row the caller has called.
    Streak,
    /// Door games played (and the built-in games).
    Doors,
    Oneliners,
    /// Answers in the voting booth and new user votes.
    Votes,
    /// Requests filled.
    Fills,
    /// Days since the first call.
    Days,
}

impl Stat {
    pub const ALL: [Stat; 10] = [Stat::Posts, Stat::Uploads, Stat::Downloads, Stat::Calls, Stat::Streak, Stat::Doors, Stat::Oneliners, Stat::Votes, Stat::Fills, Stat::Days];

    pub fn key(self) -> &'static str {
        match self {
            Stat::Posts => "posts",
            Stat::Uploads => "uploads",
            Stat::Downloads => "downloads",
            Stat::Calls => "calls",
            Stat::Streak => "streak",
            Stat::Doors => "doors",
            Stat::Oneliners => "oneliners",
            Stat::Votes => "votes",
            Stat::Fills => "fills",
            Stat::Days => "days",
        }
    }

    fn from_key(s: &str) -> Option<Stat> {
        Stat::ALL.into_iter().find(|t| t.key() == s)
    }

    /// `n` of it in words: "1 post", "7 days in a row".
    pub fn count(self, n: u64) -> String {
        let one = n == 1;
        let unit = match self {
            Stat::Posts => if one { "post" } else { "posts" },
            Stat::Uploads => if one { "upload" } else { "uploads" },
            Stat::Downloads => if one { "download" } else { "downloads" },
            Stat::Calls => if one { "call" } else { "calls" },
            Stat::Streak => if one { "day in a row" } else { "days in a row" },
            Stat::Doors => if one { "game played" } else { "games played" },
            Stat::Oneliners => if one { "oneliner" } else { "oneliners" },
            Stat::Votes => if one { "vote" } else { "votes" },
            Stat::Fills => if one { "request filled" } else { "requests filled" },
            Stat::Days => if one { "day a member" } else { "days a member" },
        };
        format!("{n} {unit}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    /// The figure that earns it.
    pub at: u32,
    pub title: String,
    /// Points paid for it (with `[files] use_file_points`).
    #[serde(default)]
    pub points: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Achievement {
    pub stat: Stat,
    /// What the list calls it.
    pub name: String,
    #[serde(rename = "tier", default)]
    pub tiers: Vec<Tier>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Achievements {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Tell the other nodes when someone earns one.
    #[serde(default = "yes")]
    pub announce: bool,
    /// Pay points for what was done before achievements (or a new one)
    /// began, too.
    #[serde(default)]
    pub back_points: bool,
    #[serde(rename = "achievement", default)]
    pub list: Vec<Achievement>,
}

fn yes() -> bool {
    true
}

impl Default for Achievements {
    fn default() -> Self {
        Self::stock()
    }
}

impl Achievements {
    /// The stock set.
    pub fn stock() -> Self {
        let a = |stat, name: &str, tiers: &[(u32, &str, u32)]| Achievement {
            stat,
            name: name.into(),
            tiers: tiers.iter().map(|&(at, title, points)| Tier { at, title: title.into(), points }).collect(),
        };
        Achievements {
            enabled: true,
            announce: true,
            back_points: false,
            list: vec![
                a(Stat::Posts, "Poster", &[(1, "First Words", 5), (10, "Regular", 10), (100, "Prolific", 25), (500, "Wordsmith", 50), (1000, "Board Legend", 100)]),
                a(Stat::Uploads, "Uploader", &[(1, "Sharer", 5), (10, "Courier", 15), (50, "Archivist", 40), (100, "Curator", 75)]),
                a(Stat::Downloads, "Downloader", &[(1, "First Grab", 0), (25, "Collector", 0), (100, "Hoarder", 0), (500, "Data Vault", 0)]),
                a(Stat::Calls, "Caller", &[(10, "Familiar Face", 10), (50, "Regular Caller", 20), (100, "Fixture", 40), (500, "Furniture", 100), (1000, "Part of the Board", 150)]),
                a(Stat::Streak, "Streak", &[(3, "Three in a Row", 5), (7, "Week Straight", 15), (14, "Fortnight", 25), (30, "Every Day a Month", 50), (100, "Unbroken", 150)]),
                a(Stat::Doors, "Gamer", &[(1, "Door Opener", 5), (10, "Player", 10), (50, "Door Warrior", 25), (100, "Door Legend", 50)]),
                a(Stat::Oneliners, "Oneliners", &[(1, "Graffiti", 2), (10, "Wall Writer", 10), (50, "Tagger", 25)]),
                a(Stat::Votes, "Voter", &[(1, "Voice Heard", 2), (10, "Opinionated", 10), (25, "Pollster", 20)]),
                a(Stat::Fills, "Filler", &[(1, "Helping Hand", 10), (10, "Fixer", 25), (50, "Supplier", 75)]),
                a(Stat::Days, "Member", &[(30, "One Month In", 10), (182, "Half a Year", 25), (365, "One Year", 50), (730, "Two Years", 100), (1825, "Five Years", 200)]),
            ],
        }
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# Achievements callers earn (UA shows them). stat: posts, uploads,\n\
             # downloads, calls, streak (days in a row), doors (games played),\n\
             # oneliners, votes, fills (requests filled) or days (a member).\n\
             # Each tier is earned once; its points are paid with [files]\n\
             # use_file_points. Edits take effect at the next menu.\n\n{}",
            toml::to_string_pretty(self).expect("achievements serialize")
        )
    }

    /// The tiers in order, each achievement's lowest first.
    fn sorted(&self) -> Vec<(&Achievement, Vec<&Tier>)> {
        self.list
            .iter()
            .map(|a| {
                let mut t: Vec<&Tier> = a.tiers.iter().collect();
                t.sort_by_key(|t| t.at);
                (a, t)
            })
            .collect()
    }
}

/// A tier earned.
#[derive(Debug, Clone, PartialEq)]
pub struct Award {
    pub stat: Stat,
    pub at: u32,
    pub title: String,
    pub points: u32,
    /// Given for what was done before: quietly, and without points.
    pub back: bool,
}

/// The tiers reached and not yet earned. A stat this caller hasn't been
/// looked at for (`seen`) gives what it has reached as back awards,
/// unless they're `fresh` (new this call).
pub fn due(defs: &Achievements, value: &dyn Fn(Stat) -> u64, earned: &HashSet<(Stat, u32)>, seen: &HashSet<Stat>, fresh: bool) -> Vec<Award> {
    let mut out = Vec::new();
    for (a, tiers) in defs.sorted() {
        let v = value(a.stat);
        let back = !fresh && !seen.contains(&a.stat);
        for t in tiers {
            if t.at as u64 <= v && t.at > 0 && !earned.contains(&(a.stat, t.at)) && !out.iter().any(|w: &Award| w.stat == a.stat && w.at == t.at) {
                let points = if back && !defs.back_points { 0 } else { t.points };
                out.push(Award { stat: a.stat, at: t.at, title: t.title.clone(), points, back });
            }
        }
    }
    out
}

/// The run of days called that ends `today` or yesterday, and the
/// longest run, from the days called (any order, repeats allowed).
pub fn runs(days: &[NaiveDate], today: NaiveDate) -> (u32, u32) {
    let mut d: Vec<NaiveDate> = days.to_vec();
    d.sort();
    d.dedup();
    let (mut best, mut run) = (0u32, 0u32);
    let mut prev: Option<NaiveDate> = None;
    for &day in &d {
        run = if prev.is_some_and(|p| p.succ_opt() == Some(day)) { run + 1 } else { 1 };
        best = best.max(run);
        prev = Some(day);
    }
    let current = match prev {
        Some(p) if p == today || p.succ_opt() == Some(today) => run,
        _ => 0,
    };
    (current, best)
}

/// One a caller has, as kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Earned {
    pub stat: Stat,
    pub at: u32,
    pub title: String,
    pub points: u32,
    pub when: NaiveDateTime,
    pub back: bool,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS achievements (
    user_id INTEGER NOT NULL,
    stat    TEXT NOT NULL,
    at      INTEGER NOT NULL,
    title   TEXT NOT NULL,
    points  INTEGER NOT NULL,
    earned  TEXT NOT NULL,
    back    INTEGER NOT NULL,
    told    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, stat, at)
);
CREATE TABLE IF NOT EXISTS achieve_seen (
    user_id INTEGER NOT NULL,
    stat    TEXT NOT NULL,
    PRIMARY KEY (user_id, stat)
);
CREATE TABLE IF NOT EXISTS achieve_counts (
    user_id INTEGER NOT NULL,
    stat    TEXT NOT NULL,
    n       INTEGER NOT NULL,
    PRIMARY KEY (user_id, stat)
);
CREATE TABLE IF NOT EXISTS streaks (
    user_id  INTEGER PRIMARY KEY,
    last_day TEXT NOT NULL,
    days     INTEGER NOT NULL,
    best     INTEGER NOT NULL
);
";

/// What achievements keep: who has which, which stats each caller has
/// been looked at for, counts kept nowhere else (oneliners), and runs of
/// days called.
pub struct AchieveStore {
    db: Mutex<Connection>,
}

impl AchieveStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut db = crate::sysdata::open_db(path)?;
        crate::sysdata::migrate(&mut db, "achieve", SCHEMA, &[])?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let mut db = Connection::open_in_memory()?;
        crate::sysdata::migrate(&mut db, "achieve", SCHEMA, &[])?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// A caller's, oldest first.
    pub fn earned(&self, user: u32) -> anyhow::Result<Vec<Earned>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT stat, at, title, points, earned, back FROM achievements WHERE user_id = ?1 ORDER BY earned, at")?;
        let rows = stmt.query_map([user], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (stat, at, title, points, when, back) = row?;
            if let Some(stat) = Stat::from_key(&stat) {
                out.push(Earned { stat, at, title, points, when, back });
            }
        }
        Ok(out)
    }

    pub fn seen(&self, user: u32) -> anyhow::Result<HashSet<Stat>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT stat FROM achieve_seen WHERE user_id = ?1")?;
        let rows = stmt.query_map([user], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows.iter().filter_map(|s| Stat::from_key(s)).collect())
    }

    /// Keep awards and mark their stats looked at, in one go: the awards
    /// newly kept (one already there isn't given twice).
    pub fn add(&self, user: u32, awards: &[Award], stats: &[Stat], at: NaiveDateTime) -> anyhow::Result<Vec<Award>> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        let mut kept = Vec::new();
        for a in awards {
            let n = tx.execute(
                "INSERT OR IGNORE INTO achievements (user_id, stat, at, title, points, earned, back) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![user, a.stat.key(), a.at, a.title, a.points, at, a.back],
            )?;
            if n > 0 {
                kept.push(a.clone());
            }
        }
        for s in stats {
            tx.execute("INSERT OR IGNORE INTO achieve_seen (user_id, stat) VALUES (?1, ?2)", params![user, s.key()])?;
        }
        tx.commit()?;
        Ok(kept)
    }

    /// Those not yet told, oldest first.
    pub fn untold(&self, user: u32) -> anyhow::Result<Vec<Award>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT stat, at, title, points, back FROM achievements WHERE user_id = ?1 AND NOT told ORDER BY earned, at")?;
        let rows = stmt.query_map([user], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (stat, at, title, points, back) = row?;
            if let Some(stat) = Stat::from_key(&stat) {
                out.push(Award { stat, at, title, points, back });
            }
        }
        Ok(out)
    }

    pub fn told(&self, user: u32) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE achievements SET told = 1 WHERE user_id = ?1 AND NOT told", [user])?;
        Ok(())
    }

    pub fn count(&self, user: u32, stat: Stat) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT n FROM achieve_counts WHERE user_id = ?1 AND stat = ?2", params![user, stat.key()], |r| r.get::<_, i64>(0)).optional()?.unwrap_or(0).max(0) as u64)
    }

    pub fn bump(&self, user: u32, stat: Stat) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "INSERT INTO achieve_counts (user_id, stat, n) VALUES (?1, ?2, 1) ON CONFLICT (user_id, stat) DO UPDATE SET n = n + 1",
            params![user, stat.key()],
        )?;
        Ok(())
    }

    /// The longest run of days called.
    pub fn best_streak(&self, user: u32) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT best FROM streaks WHERE user_id = ?1", [user], |r| r.get(0)).optional()?.unwrap_or(0))
    }

    /// A call on `today`: the run goes on (or starts again). A caller
    /// with no run kept yet starts from `history`, the days they're in
    /// the callers log. Returns (the run now, the longest).
    pub fn called(&self, user: u32, today: NaiveDate, history: impl FnOnce() -> Vec<NaiveDate>) -> anyhow::Result<(u32, u32)> {
        let kept: Option<(NaiveDate, u32, u32)> = {
            let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            db.query_row("SELECT last_day, days, best FROM streaks WHERE user_id = ?1", [user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?
        };
        let (days, best) = match kept {
            Some((last, days, best)) if last == today => (days, best),
            Some((last, days, best)) => {
                let days = if last.succ_opt() == Some(today) { days + 1 } else { 1 };
                (days, best.max(days))
            }
            None => {
                let mut h = history();
                h.push(today);
                runs(&h, today)
            }
        };
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("INSERT OR REPLACE INTO streaks (user_id, last_day, days, best) VALUES (?1, ?2, ?3, ?4)", params![user, today, days, best])?;
        Ok((days, best))
    }
}

/// When the session last looked, and what it saw then, so the menus
/// don't work it all out on every key.
#[derive(Default)]
pub struct Tracker {
    last: Option<std::time::Instant>,
    sig: (u32, u32, u32, u32),
    /// Something changed that the record doesn't show (a door, a vote).
    pub due: bool,
    /// Awards wait to be told.
    pending: bool,
}

/// How often the menus look again when nothing seems to have changed
/// (fills, days a member and the SysOp's edits change from elsewhere).
const LOOK_EVERY: u64 = 120;

/// A caller's figure for `stat`.
pub fn value(state: &crate::BbsState, u: &User, stat: Stat) -> u64 {
    let today = crate::clock::now().date_naive();
    match stat {
        Stat::Posts => u.posts as u64,
        Stat::Uploads => u.uploads as u64,
        Stat::Downloads => u.downloads as u64,
        Stat::Calls => u.total_calls as u64,
        Stat::Streak => state.achieve.best_streak(u.id).unwrap_or(0) as u64,
        Stat::Doors => state.sys.plays_by(u.id).unwrap_or(0),
        Stat::Oneliners => state.achieve.count(u.id, Stat::Oneliners).unwrap_or(0),
        Stat::Votes => state.sys.votes_by(u.id).unwrap_or(0) + state.forms.nuv_votes_by(u.id).unwrap_or(0),
        Stat::Fills => state.msgs.fills_of(u.id).map_or(0, |f| f.0 as u64),
        Stat::Days => u.first_call.map_or(0, |d| (today - d).num_days().max(0) as u64),
    }
}

/// Whether `u` takes part: a caller, not a bot, on a board with them on.
fn takes_part(state: &crate::BbsState, u: &User) -> bool {
    u.id != 0 && !crate::aimemory::is_bot(u) && state.achievements().enabled
}

impl Session {
    /// Logon: the call counts toward the streak, and what was reached
    /// before this call is settled now, so all done on it is earned as
    /// it happens (told at the first menu).
    pub(crate) fn achieve_logon(&mut self) {
        if !takes_part(&self.state, &self.user) {
            return;
        }
        let today = crate::clock::now().date_naive();
        let (id, sys) = (self.user.id, &self.state.sys);
        if let Err(e) = self.state.achieve.called(id, today, || sys.call_days(id).unwrap_or_default()) {
            tracing::error!("streak for {}: {e}", self.user.handle);
        }
        if let Err(e) = self.achieve_award() {
            tracing::error!("achievements for {}: {e:#}", self.user.handle);
        }
        self.achieve.sig = (self.user.posts, self.user.uploads, self.user.downloads, self.user.total_calls);
        self.achieve.last = Some(std::time::Instant::now());
    }

    /// A oneliner added (`?T`).
    pub(crate) fn achieve_oneliner(&mut self) {
        if !takes_part(&self.state, &self.user) {
            return;
        }
        if let Err(e) = self.state.achieve.bump(self.user.id, Stat::Oneliners) {
            tracing::error!("oneliners for {}: {e}", self.user.handle);
        }
        self.achieve.due = true;
    }

    /// At a menu: give what's been reached, pay for it, and tell the
    /// caller (the help line, or the line before the prompt).
    pub(crate) async fn achieve_check(&mut self) -> io::Result<()> {
        if !self.logged_in || !takes_part(&self.state, &self.user) {
            return Ok(());
        }
        let u = &self.user;
        let sig = (u.posts, u.uploads, u.downloads, u.total_calls);
        let stale = self.achieve.last.is_none_or(|t| t.elapsed().as_secs() >= LOOK_EVERY);
        if self.achieve.due || stale || sig != self.achieve.sig {
            self.achieve.due = false;
            self.achieve.sig = sig;
            self.achieve.last = Some(std::time::Instant::now());
            if let Err(e) = self.achieve_award() {
                tracing::error!("achievements for {}: {e:#}", self.user.handle);
            }
        }
        if self.achieve.pending && self.note.is_none() {
            self.achieve_tell().await?;
        }
        Ok(())
    }

    fn achieve_award(&mut self) -> anyhow::Result<()> {
        let defs = self.state.achievements();
        let id = self.user.id;
        let earned: HashSet<(Stat, u32)> = self.state.achieve.earned(id)?.into_iter().map(|e| (e.stat, e.at)).collect();
        let seen = self.state.achieve.seen(id)?;
        let mut values: HashMap<Stat, u64> = HashMap::new();
        for a in &defs.list {
            values.entry(a.stat).or_insert_with(|| value(&self.state, &self.user, a.stat));
        }
        let fresh = self.new_user || self.user.total_calls <= 1;
        let awards = due(&defs, &|s| values.get(&s).copied().unwrap_or(0), &earned, &seen, fresh);
        let unseen: Vec<Stat> = values.keys().copied().filter(|s| !seen.contains(s)).collect();
        if awards.is_empty() && unseen.is_empty() {
            return Ok(());
        }
        let now = crate::clock::now().naive_local();
        let kept = self.state.achieve.add(id, &awards, &unseen, now)?;
        if kept.is_empty() {
            return Ok(());
        }
        self.achieve.pending = true;
        let paying = self.state.config().files.use_file_points;
        let mut paid = 0u32;
        for a in kept.iter().filter(|a| a.points > 0 && paying) {
            paid = paid.saturating_add(a.points);
            crate::points::record(&self.state, "achieve", (id, &self.user.handle), a.points, a.stat.key(), &a.title);
        }
        if paid > 0 {
            self.user.file_points = self.user.file_points.saturating_add(paid);
            self.save_user();
        }
        let live: Vec<&Award> = kept.iter().filter(|a| !a.back).collect();
        for a in &kept {
            tracing::info!("{}: achievement {} ({}){}", self.user.handle, a.title, a.stat.count(a.at as u64), if a.back { ", for before" } else { "" });
        }
        // The others hear of the best one, unless the caller's hidden.
        if let Some(best) = live.iter().max_by_key(|a| a.at)
            && defs.announce
            && !self.hidden_call
        {
            let me = self.node.number();
            let online = self.state.nodes.online();
            let invisible = online.iter().any(|(n, i)| *n == me && i.invisible);
            if !invisible {
                let text = self.strings().fmt("AchAnnounce", &[&self.user.handle, &best.title, &best.stat.count(best.at as u64)]);
                for (n, info) in online {
                    if n != me && !info.user.is_empty() {
                        self.state.nodes.send(n, crate::nodes::NodeEvent::Notice { text: text.clone() });
                    }
                }
            }
        }
        Ok(())
    }

    /// Tell the caller what they've earned that they haven't heard of.
    async fn achieve_tell(&mut self) -> io::Result<()> {
        let untold = self.state.achieve.untold(self.user.id).map_err(io_err)?;
        self.achieve.pending = false;
        if untold.is_empty() {
            return Ok(());
        }
        let _ = self.state.achieve.told(self.user.id);
        let paying = self.state.config().files.use_file_points;
        let (back, live): (Vec<Award>, Vec<Award>) = untold.into_iter().partition(|a| a.back);
        let key = self.achieve_key();
        let s = self.strings();
        let line = match live.as_slice() {
            [] => s.fmt("AchBack", &[&back.len().to_string(), if back.len() == 1 { "" } else { "s" }, &key]),
            [a] => {
                let what = a.stat.count(a.at as u64);
                if a.points > 0 && paying { s.fmt("AchEarnedPts", &[&a.title, &what, &a.points.to_string()]) } else { s.fmt("AchEarned", &[&a.title, &what]) }
            }
            many => {
                let titles: Vec<&str> = many.iter().map(|a| a.title.as_str()).collect();
                let pts: u32 = if paying { many.iter().map(|a| a.points).sum() } else { 0 };
                if pts > 0 {
                    s.fmt("AchEarnedManyPts", &[&many.len().to_string(), &titles.join(", "), &pts.to_string()])
                } else {
                    s.fmt("AchEarnedMany", &[&many.len().to_string(), &titles.join(", ")])
                }
            }
        };
        drop(s);
        if self.plain() {
            return self.show_line(&line).await;
        }
        self.note = Some(line);
        Ok(())
    }

    /// The key on this menu for `UA`, hidden or not, as "(&)"; else
    /// blank.
    fn achieve_key(&self) -> String {
        let Some(menu) = &self.menu.menu else { return String::new() };
        menu.commands
            .iter()
            .find(|c| c.command.eq_ignore_ascii_case("UA") && c.keys != "FIRSTCMD" && self.acs(&c.acs))
            .map(|c| self.strings().fmt("HintKey", &[&c.keys.to_ascii_lowercase()]))
            .unwrap_or_default()
    }

    /// For finger (`~AC`): how many `u` has, and the latest.
    pub(crate) fn achieve_summary(&self, u: &User) -> String {
        let defs = self.state.achievements();
        if !defs.enabled {
            return String::new();
        }
        let all = self.state.achieve.earned(u.id).unwrap_or_default();
        let total: usize = defs.list.iter().map(|a| a.tiers.len()).sum();
        let Some(last) = all.iter().max_by_key(|e| (e.when, e.at)) else { return String::new() };
        self.strings().fmt("AchSummary", &[&all.len().to_string(), &total.to_string(), &last.title])
    }

    /// `UA` (new): your achievements, earned and next, a line each; an
    /// ACHV list screen (TOP/MID/BOT) if the board has one.
    pub async fn achievements_screen(&mut self) -> io::Result<()> {
        let defs = self.state.achievements();
        if !defs.enabled || self.user.id == 0 {
            self.newline();
            return self.say_line("AchOff").await;
        }
        // What's earned now, before the list.
        if let Err(e) = self.achieve_award() {
            tracing::error!("achievements for {}: {e:#}", self.user.handle);
        }
        let _ = self.state.achieve.told(self.user.id);
        self.achieve.pending = false;
        let earned = self.state.achieve.earned(self.user.id).map_err(io_err)?;
        let have: HashMap<(Stat, u32), &Earned> = earned.iter().map(|e| ((e.stat, e.at), e)).collect();
        let total: usize = defs.list.iter().map(|a| a.tiers.len()).sum();
        let got = defs.list.iter().flat_map(|a| a.tiers.iter().map(move |t| (a.stat, t.at))).filter(|k| have.contains_key(k)).count();
        let pts: u32 = earned.iter().map(|e| e.points).sum();
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        let top = waves([("EA", format!("{got}/{total}")), ("EP", pts.to_string())]);
        let mut rep = self.repeat_begin_with("ACHV", false, &top).await?;
        if rep.is_none() {
            self.newline();
            let s = self.strings().fmt("AchTitle", &[&got.to_string(), &total.to_string()]);
            self.show_line(&s).await?;
            self.set_slot(Slot::Info);
            self.show_line(&format!(" {} {} {} {} {}", resize("achievement", 14), resize("have", 7), resize("tiers", 6), resize("latest", 21), "next")).await?;
            self.rule().await?;
        }
        let mut lines = 4;
        let sorted = defs.sorted();
        for (a, tiers) in sorted {
            let v = value(&self.state, &self.user, a.stat);
            let marks: String = tiers.iter().map(|t| if have.contains_key(&(a.stat, t.at)) { '\u{25a0}' } else { '\u{00b7}' }).collect();
            let latest = tiers.iter().rev().find(|t| have.contains_key(&(a.stat, t.at))).map(|t| t.title.clone()).unwrap_or_default();
            let next = match tiers.iter().find(|t| !have.contains_key(&(a.stat, t.at))) {
                Some(t) => {
                    let left = (t.at as u64).saturating_sub(v);
                    if left == 0 { t.title.clone() } else { self.strings().fmt("AchNext", &[&t.title, &left.to_string()]) }
                }
                None => self.strings().get("AchAllDone").to_string(),
            };
            let more = match rep.as_mut() {
                Some(r) => {
                    let w = waves([("AN", a.name.clone()), ("AV", v.to_string()), ("AM", marks.clone()), ("AL", latest.clone()), ("AX", next.clone())]);
                    self.repeat_row(r, &w, false).await?
                }
                None => {
                    let line = format!(
                        "|U3 {} |U1{} |U2{} |U3{} |U1{}",
                        lit(&resize(&a.name, 14)),
                        resize(&v.to_string(), 7),
                        resize(&marks, 6),
                        lit(&resize(&latest, 21)),
                        lit(&fluxbbs_term::codes::squish(&next, 28))
                    );
                    self.show_line(&line).await?;
                    lines += 1;
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        // The screen's top has the totals; the plain one ends with them.
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => {
                self.rule().await?;
                let s = self.strings().fmt("AchYours", &[&got.to_string(), &total.to_string(), &pts.to_string()]);
                self.show_line(&s).await?;
            }
        }
        self.prompt_key().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn stock_round_trips() {
        let s = Achievements::stock();
        let back: Achievements = toml::from_str(&s.to_toml()).unwrap();
        assert_eq!(back, s);
        // A file with only settings has no achievements.
        let bare: Achievements = toml::from_str("announce = false").unwrap();
        assert!(bare.enabled && !bare.announce && bare.list.is_empty());
    }

    #[test]
    fn tiers_reached_are_due_once() {
        let defs = Achievements::stock();
        let v = |s: Stat| if s == Stat::Posts { 120 } else { 0 };
        let mut earned = HashSet::new();
        let seen: HashSet<Stat> = Stat::ALL.into_iter().collect();
        let got = due(&defs, &v, &earned, &seen, false);
        assert_eq!(got.iter().map(|a| a.at).collect::<Vec<_>>(), [1, 10, 100]);
        assert!(got.iter().all(|a| !a.back && a.points > 0));
        earned.insert((Stat::Posts, 1));
        earned.insert((Stat::Posts, 10));
        assert_eq!(due(&defs, &v, &earned, &seen, false).len(), 1);
    }

    #[test]
    fn what_was_done_before_is_quiet_and_unpaid() {
        let mut defs = Achievements::stock();
        let v = |s: Stat| if s == Stat::Calls { 600 } else { 0 };
        let got = due(&defs, &v, &HashSet::new(), &HashSet::new(), false);
        assert_eq!(got.len(), 4);
        assert!(got.iter().all(|a| a.back && a.points == 0));
        // New this call: earned as it happens.
        assert!(due(&defs, &v, &HashSet::new(), &HashSet::new(), true).iter().all(|a| !a.back && a.points > 0));
        defs.back_points = true;
        assert!(due(&defs, &v, &HashSet::new(), &HashSet::new(), false).iter().all(|a| a.back && a.points > 0));
    }

    #[test]
    fn runs_of_days() {
        let today = d("2026-11-07");
        let days = [d("2026-10-01"), d("2026-10-02"), d("2026-10-03"), d("2026-10-03"), d("2026-11-05"), d("2026-11-06")];
        assert_eq!(runs(&days, today), (2, 3));
        assert_eq!(runs(&[d("2026-11-01")], today), (0, 1));
        assert_eq!(runs(&[], today), (0, 0));
    }

    #[test]
    fn old_callers_get_theirs_quietly_then_earn_with_points() {
        let state = crate::tests::state();
        let mut cfg = (*state.config()).clone();
        cfg.files.use_file_points = true;
        state.set_config(cfg);
        let mut u = User { handle: "phrac".into(), posts: 120, total_calls: 5, ..Default::default() };
        state.users.insert(&mut u).unwrap();
        let mut s = Session::for_test(state.clone(), u);
        s.achieve_award().unwrap();
        let got = state.achieve.earned(s.user.id).unwrap();
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|e| e.back && e.points == 0));
        assert_eq!(s.user.file_points, 0);
        // Nothing more till something changes.
        s.achieve_award().unwrap();
        assert_eq!(state.achieve.earned(s.user.id).unwrap().len(), 3);
        s.user.posts = 500;
        s.achieve_award().unwrap();
        let untold = state.achieve.untold(s.user.id).unwrap();
        assert_eq!(untold.iter().filter(|a| !a.back).map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Wordsmith"]);
        assert_eq!(s.user.file_points, 50);
        assert_eq!(state.users.get(s.user.id).unwrap().unwrap().file_points, 50);
        assert_eq!(state.sys.points_recent(s.user.id, 5).unwrap()[0].kind, "achieve");
        // A bot earns none.
        let mut b = User { id: 9, handle: "bot".into(), ..Default::default() };
        b.flags.insert(crate::aimemory::BOT_FLAG);
        assert!(!takes_part(&state, &b));
    }

    #[test]
    fn the_store_keeps_awards_and_runs() {
        let s = AchieveStore::open_memory().unwrap();
        let now = d("2026-11-07").and_hms_opt(12, 0, 0).unwrap();
        let a = Award { stat: Stat::Posts, at: 10, title: "Regular".into(), points: 10, back: false };
        assert_eq!(s.add(7, std::slice::from_ref(&a), &[Stat::Posts], now).unwrap().len(), 1);
        assert!(s.add(7, &[a], &[], now).unwrap().is_empty());
        assert_eq!(s.seen(7).unwrap(), HashSet::from([Stat::Posts]));
        assert_eq!(s.untold(7).unwrap().len(), 1);
        s.told(7).unwrap();
        assert!(s.untold(7).unwrap().is_empty());
        assert_eq!(s.earned(7).unwrap()[0].title, "Regular");
        // The first call starts from the callers log; then day by day.
        assert_eq!(s.called(7, d("2026-11-07"), || vec![d("2026-11-05"), d("2026-11-06")]).unwrap(), (3, 3));
        assert_eq!(s.called(7, d("2026-11-07"), Vec::new).unwrap(), (3, 3));
        assert_eq!(s.called(7, d("2026-11-08"), Vec::new).unwrap(), (4, 4));
        assert_eq!(s.called(7, d("2026-11-10"), Vec::new).unwrap(), (1, 4));
        assert_eq!(s.best_streak(7).unwrap(), 4);
        s.bump(7, Stat::Oneliners).unwrap();
        s.bump(7, Stat::Oneliners).unwrap();
        assert_eq!(s.count(7, Stat::Oneliners).unwrap(), 2);
    }
}
