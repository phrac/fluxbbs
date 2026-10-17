//! System-wide records (SQLite): totals and daily history (STATS.DAT,
//! HISTORY.DAT), the callers log (CALLERS.DAT), the automessage, the
//! BBS list (BBSLIST.DAT), and what Lua scripts keep (`bbs.get`/`bbs.set`).

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS history (
    date        TEXT PRIMARY KEY,
    calls       INTEGER NOT NULL DEFAULT 0,
    new_users   INTEGER NOT NULL DEFAULT 0,
    posts       INTEGER NOT NULL DEFAULT 0,
    email       INTEGER NOT NULL DEFAULT 0,
    uploads     INTEGER NOT NULL DEFAULT 0,
    downloads   INTEGER NOT NULL DEFAULT 0,
    upload_kb   INTEGER NOT NULL DEFAULT 0,
    download_kb INTEGER NOT NULL DEFAULT 0
);
-- Totals carried over from an imported STATS.DAT, before our history.
CREATE TABLE IF NOT EXISTS totals_base (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    first_day   TEXT,
    calls       INTEGER NOT NULL,
    posts       INTEGER NOT NULL,
    email       INTEGER NOT NULL,
    uploads     INTEGER NOT NULL,
    downloads   INTEGER NOT NULL,
    upload_kb   INTEGER NOT NULL,
    download_kb INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS callers (
    id        INTEGER PRIMARY KEY,
    call_num  INTEGER NOT NULL,
    user_id   INTEGER NOT NULL,
    handle    TEXT NOT NULL,
    location  TEXT NOT NULL,
    at        TEXT NOT NULL,
    new_user  INTEGER NOT NULL,
    area_code TEXT NOT NULL,
    transport TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS automsg (
    id        INTEGER PRIMARY KEY CHECK (id = 1),
    user_id   INTEGER NOT NULL,
    author    TEXT NOT NULL,
    anonymous INTEGER NOT NULL,
    title     TEXT NOT NULL,
    posted    TEXT NOT NULL,
    body      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS bbslist (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL,
    address   TEXT NOT NULL,
    sysop     TEXT NOT NULL,
    speed     INTEGER NOT NULL,
    software  TEXT NOT NULL,
    storage   TEXT NOT NULL,
    info      TEXT NOT NULL,
    who_added TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS script_data (
    script    TEXT NOT NULL COLLATE NOCASE,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    PRIMARY KEY (script, key)
);
";

/// What a day (or all time) counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub calls: u64,
    pub new_users: u64,
    pub posts: u64,
    pub email: u64,
    pub uploads: u64,
    pub downloads: u64,
    pub upload_kb: u64,
    pub download_kb: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum Stat {
    Call,
    NewUser,
    Post,
    Email,
    Upload { kb: u64 },
    Download { kb: u64 },
}

#[derive(Debug, Clone)]
pub struct Call {
    pub call_num: u64,
    pub user_id: u32,
    pub handle: String,
    pub location: String,
    pub at: NaiveDateTime,
    pub new_user: bool,
    pub area_code: String,
    pub transport: String,
}

#[derive(Debug, Clone)]
pub struct AutoMsg {
    pub user_id: u32,
    pub author: String,
    pub anonymous: bool,
    pub title: String,
    pub posted: NaiveDateTime,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bbs {
    pub id: u32,
    pub name: String,
    /// Telnet/SSH address (a phone number in the original).
    pub address: String,
    pub sysop: String,
    pub speed: u32,
    pub software: String,
    pub storage: String,
    pub info: String,
    pub who_added: String,
}

pub struct SysData {
    db: Mutex<Connection>,
}

fn history_sum(db: &Connection) -> rusqlite::Result<Counts> {
    db.query_row(
        "SELECT COALESCE(SUM(calls),0) calls, COALESCE(SUM(new_users),0) new_users, COALESCE(SUM(posts),0) posts,
                COALESCE(SUM(email),0) email, COALESCE(SUM(uploads),0) uploads, COALESCE(SUM(downloads),0) downloads,
                COALESCE(SUM(upload_kb),0) upload_kb, COALESCE(SUM(download_kb),0) download_kb FROM history",
        [],
        counts,
    )
}

fn counts(r: &rusqlite::Row) -> rusqlite::Result<Counts> {
    let g = |n: &str| r.get::<_, i64>(n).map(|v| v.max(0) as u64);
    Ok(Counts {
        calls: g("calls")?,
        new_users: g("new_users")?,
        posts: g("posts")?,
        email: g("email")?,
        uploads: g("uploads")?,
        downloads: g("downloads")?,
        upload_kb: g("upload_kb")?,
        download_kb: g("download_kb")?,
    })
}

impl SysData {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// Count something against today (and so the totals).
    pub fn bump(&self, day: NaiveDate, stat: Stat) -> anyhow::Result<()> {
        let (col, n, kb_col, kb) = match stat {
            Stat::Call => ("calls", 1, None, 0),
            Stat::NewUser => ("new_users", 1, None, 0),
            Stat::Post => ("posts", 1, None, 0),
            Stat::Email => ("email", 1, None, 0),
            Stat::Upload { kb } => ("uploads", 1, Some("upload_kb"), kb),
            Stat::Download { kb } => ("downloads", 1, Some("download_kb"), kb),
        };
        let db = self.db.lock().unwrap();
        db.execute("INSERT OR IGNORE INTO history (date) VALUES (?1)", [day])?;
        db.execute(&format!("UPDATE history SET {col} = {col} + ?2 WHERE date = ?1"), params![day, n])?;
        if let Some(kc) = kb_col {
            db.execute(&format!("UPDATE history SET {kc} = {kc} + ?2 WHERE date = ?1"), params![day, kb as i64])?;
        }
        Ok(())
    }

    pub fn day(&self, day: NaiveDate) -> anyhow::Result<Counts> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT * FROM history WHERE date = ?1", [day], counts).optional()?.unwrap_or_default())
    }

    /// Every day, oldest first.
    pub fn history(&self) -> anyhow::Result<Vec<(NaiveDate, Counts)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM history ORDER BY date")?;
        let rows = stmt.query_map([], |r| Ok((r.get("date")?, counts(r)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// All-time totals: imported base plus our history.
    pub fn totals(&self) -> anyhow::Result<Counts> {
        let db = self.db.lock().unwrap();
        let sum = history_sum(&db)?;
        let base = db
            .query_row("SELECT *, 0 AS new_users FROM totals_base WHERE id = 1", [], counts)
            .optional()?
            .unwrap_or_default();
        Ok(Counts {
            calls: sum.calls + base.calls,
            new_users: sum.new_users,
            posts: sum.posts + base.posts,
            email: sum.email + base.email,
            uploads: sum.uploads + base.uploads,
            downloads: sum.downloads + base.downloads,
            upload_kb: sum.upload_kb + base.upload_kb,
            download_kb: sum.download_kb + base.download_kb,
        })
    }

    /// Import STATS.DAT totals (those already in imported history are
    /// subtracted, so they aren't counted twice).
    pub fn set_base(&self, first_day: Option<NaiveDate>, c: &Counts) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO totals_base (id, first_day, calls, posts, email, uploads, downloads, upload_kb, download_kb)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                first_day, c.calls as i64, c.posts as i64, c.email as i64, c.uploads as i64, c.downloads as i64,
                c.upload_kb as i64, c.download_kb as i64
            ],
        )?;
        Ok(())
    }

    /// The first day the BBS was online (`FirstDay`).
    pub fn first_day(&self) -> anyhow::Result<Option<NaiveDate>> {
        let db = self.db.lock().unwrap();
        let base: Option<Option<NaiveDate>> = db.query_row("SELECT first_day FROM totals_base WHERE id = 1", [], |r| r.get(0)).optional()?;
        match base.flatten() {
            Some(d) => Ok(Some(d)),
            None => Ok(db.query_row("SELECT MIN(date) FROM history", [], |r| r.get(0))?),
        }
    }

    /// The stats editor: make the all-time totals come to `want`. The
    /// daily history can't be undone, so a total can't go below what it
    /// already counts; returns the totals as they end up.
    pub fn set_totals(&self, first_day: Option<NaiveDate>, want: &Counts) -> anyhow::Result<Counts> {
        let sum = history_sum(&self.db.lock().unwrap())?;
        let base = Counts {
            calls: want.calls.saturating_sub(sum.calls),
            new_users: 0,
            posts: want.posts.saturating_sub(sum.posts),
            email: want.email.saturating_sub(sum.email),
            uploads: want.uploads.saturating_sub(sum.uploads),
            downloads: want.downloads.saturating_sub(sum.downloads),
            upload_kb: want.upload_kb.saturating_sub(sum.upload_kb),
            download_kb: want.download_kb.saturating_sub(sum.download_kb),
        };
        self.set_base(first_day, &base)?;
        self.totals()
    }

    pub fn set_day(&self, day: NaiveDate, c: &Counts) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO history (date, calls, new_users, posts, email, uploads, downloads, upload_kb, download_kb)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                day, c.calls as i64, c.new_users as i64, c.posts as i64, c.email as i64, c.uploads as i64,
                c.downloads as i64, c.upload_kb as i64, c.download_kb as i64
            ],
        )?;
        Ok(())
    }

    // ---- callers ----------------------------------------------------------

    pub fn add_call(&self, c: &Call) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO callers (call_num, user_id, handle, location, at, new_user, area_code, transport)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![c.call_num as i64, c.user_id, c.handle, c.location, c.at, c.new_user, c.area_code, c.transport],
        )?;
        Ok(())
    }

    /// The latest `n` calls, oldest first; or all of `day`'s.
    pub fn calls(&self, n: Option<u32>, day: Option<NaiveDate>) -> anyhow::Result<Vec<Call>> {
        let db = self.db.lock().unwrap();
        let map = |r: &rusqlite::Row| -> rusqlite::Result<Call> {
            Ok(Call {
                call_num: r.get::<_, i64>("call_num")?.max(0) as u64,
                user_id: r.get("user_id")?,
                handle: r.get("handle")?,
                location: r.get("location")?,
                at: r.get("at")?,
                new_user: r.get("new_user")?,
                area_code: r.get("area_code")?,
                transport: r.get("transport")?,
            })
        };
        let mut rows: Vec<Call> = match day {
            Some(d) => {
                let mut stmt = db.prepare("SELECT * FROM callers WHERE date(at) = ?1 ORDER BY id")?;
                stmt.query_map([d.to_string()], map)?.collect::<Result<_, _>>()?
            }
            None => {
                let mut stmt = db.prepare("SELECT * FROM callers ORDER BY id DESC LIMIT ?1")?;
                stmt.query_map([n.unwrap_or(10)], map)?.collect::<Result<_, _>>()?
            }
        };
        if day.is_none() {
            rows.reverse();
        }
        Ok(rows)
    }

    // ---- the automessage --------------------------------------------------

    pub fn automsg(&self) -> anyhow::Result<Option<AutoMsg>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT * FROM automsg WHERE id = 1", [], |r| {
                let body: String = r.get("body")?;
                Ok(AutoMsg {
                    user_id: r.get("user_id")?,
                    author: r.get("author")?,
                    anonymous: r.get("anonymous")?,
                    title: r.get("title")?,
                    posted: r.get("posted")?,
                    lines: body.split('\n').map(String::from).collect(),
                })
            })
            .optional()?)
    }

    pub fn set_automsg(&self, m: &AutoMsg) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO automsg (id, user_id, author, anonymous, title, posted, body)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![m.user_id, m.author, m.anonymous, m.title, m.posted, m.lines.join("\n")],
        )?;
        Ok(())
    }

    // ---- script data ------------------------------------------------------

    /// A value a script stored under `key`.
    pub fn script_get(&self, script: &str, key: &str) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT value FROM script_data WHERE script = ?1 AND key = ?2", params![script, key], |r| r.get(0))
            .optional()?)
    }

    /// Store a script's value; `None` removes it.
    pub fn script_set(&self, script: &str, key: &str, value: Option<&str>) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        match value {
            Some(v) => db.execute(
                "INSERT OR REPLACE INTO script_data (script, key, value) VALUES (?1, ?2, ?3)",
                params![script, key, v],
            )?,
            None => db.execute("DELETE FROM script_data WHERE script = ?1 AND key = ?2", params![script, key])?,
        };
        Ok(())
    }

    // ---- the BBS list -----------------------------------------------------

    pub fn bbs_list(&self) -> anyhow::Result<Vec<Bbs>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM bbslist ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Bbs {
                    id: r.get("id")?,
                    name: r.get("name")?,
                    address: r.get("address")?,
                    sysop: r.get("sysop")?,
                    speed: r.get("speed")?,
                    software: r.get("software")?,
                    storage: r.get("storage")?,
                    info: r.get("info")?,
                    who_added: r.get("who_added")?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn add_bbs(&self, b: &Bbs) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO bbslist (name, address, sysop, speed, software, storage, info, who_added)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![b.name, b.address, b.sysop, b.speed, b.software, b.storage, b.info, b.who_added],
        )?;
        Ok(())
    }

    pub fn update_bbs(&self, b: &Bbs) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE bbslist SET name = ?2, address = ?3, sysop = ?4, speed = ?5, software = ?6, storage = ?7,
                info = ?8, who_added = ?9 WHERE id = ?1",
            params![b.id, b.name, b.address, b.sysop, b.speed, b.software, b.storage, b.info, b.who_added],
        )?;
        Ok(())
    }

    pub fn delete_bbs(&self, id: u32) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM bbslist WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_bbs(&self) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM bbslist", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn stats_accumulate() {
        let sd = SysData::open_memory().unwrap();
        sd.set_base(Some(d("1997-12-01")), &Counts { calls: 100, posts: 5, ..Counts::default() }).unwrap();
        sd.bump(d("2026-10-17"), Stat::Call).unwrap();
        sd.bump(d("2026-10-17"), Stat::Call).unwrap();
        sd.bump(d("2026-10-17"), Stat::Upload { kb: 30 }).unwrap();
        sd.bump(d("2026-10-18"), Stat::Post).unwrap();
        let today = sd.day(d("2026-10-17")).unwrap();
        assert_eq!((today.calls, today.uploads, today.upload_kb), (2, 1, 30));
        let t = sd.totals().unwrap();
        assert_eq!((t.calls, t.posts, t.uploads), (102, 6, 1));
        assert_eq!(sd.history().unwrap().len(), 2);
    }

    #[test]
    fn callers_latest_and_by_day() {
        let sd = SysData::open_memory().unwrap();
        for (i, at) in ["2026-10-16T10:00:00", "2026-10-17T09:00:00", "2026-10-17T11:00:00"].iter().enumerate() {
            sd.add_call(&Call {
                call_num: i as u64 + 1,
                user_id: 1,
                handle: format!("u{i}"),
                location: String::new(),
                at: at.parse().unwrap(),
                new_user: false,
                area_code: String::new(),
                transport: "ssh".into(),
            })
            .unwrap();
        }
        let last2: Vec<_> = sd.calls(Some(2), None).unwrap().into_iter().map(|c| c.handle).collect();
        assert_eq!(last2, ["u1", "u2"]);
        assert_eq!(sd.calls(None, Some(d("2026-10-17"))).unwrap().len(), 2);
    }

    #[test]
    fn automsg_and_bbs_list() {
        let sd = SysData::open_memory().unwrap();
        assert!(sd.automsg().unwrap().is_none());
        sd.set_automsg(&AutoMsg {
            user_id: 1,
            author: "SysOp".into(),
            anonymous: false,
            title: "hi".into(),
            posted: "2026-10-17T10:00:00".parse().unwrap(),
            lines: vec!["a".into(), "b".into()],
        })
        .unwrap();
        assert_eq!(sd.automsg().unwrap().unwrap().lines, ["a", "b"]);
        sd.add_bbs(&Bbs { name: "Route 666".into(), ..Bbs::default() }).unwrap();
        let list = sd.bbs_list().unwrap();
        assert_eq!(list[0].name, "Route 666");
        sd.delete_bbs(list[0].id).unwrap();
        assert!(sd.bbs_list().unwrap().is_empty());
    }
}
