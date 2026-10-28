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
CREATE TABLE IF NOT EXISTS bans (
    ip      TEXT PRIMARY KEY,
    until   TEXT NOT NULL,
    strikes INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS signups (
    at TEXT NOT NULL,
    ip TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS security_log (
    id   INTEGER PRIMARY KEY,
    at   TEXT NOT NULL,
    ip   TEXT NOT NULL,
    what TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS event_runs (
    event     TEXT PRIMARY KEY,
    last_run  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS script_data (
    script    TEXT NOT NULL COLLATE NOCASE,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    PRIMARY KEY (script, key)
);
CREATE TABLE IF NOT EXISTS vote_questions (
    id        INTEGER PRIMARY KEY,
    text      TEXT NOT NULL,
    author    TEXT NOT NULL,
    user_add  INTEGER NOT NULL,
    created   TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS vote_answers (
    id        INTEGER PRIMARY KEY,
    question  INTEGER NOT NULL,
    text      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS votes (
    user_id   INTEGER NOT NULL,
    question  INTEGER NOT NULL,
    answer    INTEGER NOT NULL,
    PRIMARY KEY (user_id, question)
);
CREATE TABLE IF NOT EXISTS scan_off (
    user_id   INTEGER NOT NULL,
    kind      TEXT NOT NULL,
    area      TEXT NOT NULL,
    PRIMARY KEY (user_id, kind, area)
);
CREATE TABLE IF NOT EXISTS door_plays (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL,
    handle    TEXT NOT NULL,
    door      TEXT NOT NULL,
    started   TEXT NOT NULL,
    minutes   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS points (
    id          INTEGER PRIMARY KEY,
    at          TEXT NOT NULL,
    kind        TEXT NOT NULL,
    from_id     INTEGER NOT NULL,
    from_handle TEXT NOT NULL,
    to_id       INTEGER NOT NULL,
    to_handle   TEXT NOT NULL,
    amount      INTEGER NOT NULL,
    on_what     TEXT NOT NULL,
    note        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS points_to ON points (to_id, kind, at);
CREATE INDEX IF NOT EXISTS points_from ON points (from_id, kind, at);
CREATE INDEX IF NOT EXISTS points_on ON points (kind, on_what);
";

/// A voting booth question (2.0).
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub id: i64,
    pub text: String,
    pub author: String,
    /// Callers may add answers of their own.
    pub user_add: bool,
    pub answers: Vec<Answer>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub id: i64,
    pub text: String,
    pub votes: u32,
}

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
    /// Telnet address, `host:port` (a phone number in the original; the
    /// column is still `address`).
    pub telnet: String,
    /// New: SSH address, `host:port` (in place of the original's baud
    /// rate, whose column stays unused).
    pub ssh: String,
    pub sysop: String,
    pub software: String,
    pub storage: String,
    pub info: String,
    pub who_added: String,
}

/// A door game played (new).
#[derive(Debug, Clone, PartialEq)]
pub struct Play {
    pub handle: String,
    pub door: String,
    pub started: NaiveDateTime,
    pub minutes: u32,
}

/// New: file points moving (`points.rs`): from whom (0: the house) to
/// whom (0: the lottery's pot), how many, what for (`upvote`, `door`,
/// `tip`, `ticket`, `lottery`), and on what (a message's id, a door, the
/// lottery's day).
#[derive(Debug, Clone, PartialEq)]
pub struct PointsEntry {
    pub at: NaiveDateTime,
    pub kind: String,
    pub from_id: u32,
    pub from: String,
    pub to_id: u32,
    pub to: String,
    pub amount: u32,
    pub on: String,
    pub note: String,
}

fn points_row(r: &rusqlite::Row) -> rusqlite::Result<PointsEntry> {
    Ok(PointsEntry {
        at: r.get("at")?,
        kind: r.get("kind")?,
        from_id: r.get("from_id")?,
        from: r.get("from_handle")?,
        to_id: r.get("to_id")?,
        to: r.get("to_handle")?,
        amount: r.get("amount")?,
        on: r.get("on_what")?,
        note: r.get("note")?,
    })
}

fn points_insert(db: &Connection, e: &PointsEntry) -> rusqlite::Result<usize> {
    db.execute(
        "INSERT INTO points (at, kind, from_id, from_handle, to_id, to_handle, amount, on_what, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![e.at, e.kind, e.from_id, e.from, e.to_id, e.to, e.amount, e.on, e.note],
    )
}

/// The doors menu's figures (new), over every play and the caller's own.
#[derive(Debug, Clone, Default)]
pub struct DoorStats {
    pub last: Option<Play>,
    /// The most played door and its plays.
    pub popular: Option<(String, u32)>,
    /// Who has spent the most time in doors, and their minutes.
    pub top_player: Option<(String, u64)>,
    pub minutes: u64,
    pub plays_today: u32,
    /// The longest single play.
    pub marathon: Option<Play>,
    /// When each door (upper case) was last played.
    pub last_by_door: std::collections::HashMap<String, NaiveDateTime>,
    pub my_favorite: Option<(String, u32)>,
    pub my_plays: u32,
    pub my_last: Option<Play>,
    /// The doors (upper case) the caller has played.
    pub my_doors: std::collections::HashSet<String>,
}

fn play(r: &rusqlite::Row) -> rusqlite::Result<Play> {
    Ok(Play { handle: r.get(0)?, door: r.get(1)?, started: r.get(2)?, minutes: r.get(3)? })
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

/// Open the board's database. Write-ahead logging, so a writer (another
/// node, a bulk load) never holds up readers; waits up to 5
/// seconds for a lock.
pub fn open_db(path: &std::path::Path) -> anyhow::Result<Connection> {
    let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    // Switching an old database to WAL needs it to itself: another
    // process opening it at the same moment is waited out.
    let mut tries = 0;
    while let Err(e) = db.pragma_update(None, "journal_mode", "WAL") {
        tries += 1;
        if tries > 50 {
            return Err(e.into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    db.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(db)
}

/// Columns added since a database was made.
fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let has_ssh: bool = db.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('bbslist') WHERE name = 'ssh'", [], |r| r.get(0))?;
    if !has_ssh {
        db.execute("ALTER TABLE bbslist ADD COLUMN ssh TEXT NOT NULL DEFAULT ''", [])?;
    }
    Ok(())
}

impl SysData {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = crate::sysdata::open_db(path)?;
        db.execute_batch(SCHEMA)?;
        migrate(&db)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        migrate(&db)?;
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
        totals(&self.db.lock().unwrap())
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

    /// `callAdd` for a call starting now: count it today and number it,
    /// under one lock so two nodes logging on together get different
    /// numbers (2.0 fixed "caller list numbering with multinode").
    pub fn log_call(&self, mut c: Call) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let day = c.at.date();
        db.execute("INSERT OR IGNORE INTO history (date) VALUES (?1)", [day])?;
        db.execute("UPDATE history SET calls = calls + 1 WHERE date = ?1", [day])?;
        c.call_num = totals(&db)?.calls;
        insert_call(&db, &c)
    }

    pub fn add_call(&self, c: &Call) -> anyhow::Result<()> {
        insert_call(&self.db.lock().unwrap(), c)
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

    // ---- bans and failures --------------------------------------------------

    /// Bans on record: (address, when it ends, how many bans so far).
    pub fn bans(&self) -> anyhow::Result<Vec<(String, NaiveDateTime, u32)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT ip, until, strikes FROM bans ORDER BY until")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn set_ban(&self, ip: &str, until: NaiveDateTime, strikes: u32) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT OR REPLACE INTO bans (ip, until, strikes) VALUES (?1, ?2, ?3)", params![ip, until, strikes])?;
        Ok(())
    }

    /// Lift a ban (`"all"` lifts every one). Returns how many went.
    pub fn clear_ban(&self, ip: &str) -> anyhow::Result<usize> {
        let db = self.db.lock().unwrap();
        Ok(if ip == "all" { db.execute("DELETE FROM bans", [])? } else { db.execute("DELETE FROM bans WHERE ip = ?1", [ip])? })
    }

    /// Note a failed bot check or password, or a ban.
    pub fn log_security(&self, ip: &str, what: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT INTO security_log (at, ip, what) VALUES (?1, ?2, ?3)", params![crate::clock::now().naive_local(), ip, what])?;
        // Keep the log to a few thousand lines.
        db.execute("DELETE FROM security_log WHERE id < (SELECT MAX(id) FROM security_log) - 5000", [])?;
        Ok(())
    }

    /// A new account was made from `ip` (empty if unknown).
    pub fn record_signup(&self, ip: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let now = crate::clock::now().naive_local();
        db.execute("INSERT INTO signups (at, ip) VALUES (?1, ?2)", params![now, ip])?;
        db.execute("DELETE FROM signups WHERE at < ?1", [now - chrono::Duration::days(30)])?;
        Ok(())
    }

    /// Sign-ups since `when`: from `ip`, and in all.
    pub fn signups_since(&self, ip: &str, when: NaiveDateTime) -> anyhow::Result<(u32, u32)> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row(
            "SELECT COALESCE(SUM(ip = ?1), 0), COUNT(*) FROM signups WHERE at > ?2",
            params![ip, when],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// The latest security events, newest first: (when, address, what).
    pub fn security_events(&self, limit: u32) -> anyhow::Result<Vec<(NaiveDateTime, String, String)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT at, ip, what FROM security_log ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// How many events since `when`, and how many of them were bans.
    pub fn security_since(&self, when: NaiveDateTime) -> anyhow::Result<(u32, u32)> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(what LIKE 'banned%'), 0) FROM security_log WHERE at > ?1",
            [when],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    // ---- events -----------------------------------------------------------

    /// The day an event (by description) last ran.
    pub fn event_last(&self, event: &str) -> anyhow::Result<Option<NaiveDate>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT last_run FROM event_runs WHERE event = ?1", [event], |r| r.get(0)).optional()?)
    }

    pub fn set_event_last(&self, event: &str, day: NaiveDate) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT OR REPLACE INTO event_runs (event, last_run) VALUES (?1, ?2)", params![event, day])?;
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

    // ---- door plays (new) ---------------------------------------------------

    pub fn add_play(&self, user: u32, p: &Play) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO door_plays (user_id, handle, door, started, minutes) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user, p.handle, p.door, p.started, p.minutes],
        )?;
        Ok(())
    }

    /// The doors menu's figures, with `user`'s own; "today" starts at `day`.
    pub fn door_stats(&self, user: u32, day: NaiveDateTime) -> anyhow::Result<DoorStats> {
        let db = self.db.lock().unwrap();
        const COLS: &str = "SELECT handle, door, started, minutes FROM door_plays";
        let one = |sql: &str, p: &[&dyn rusqlite::ToSql]| db.query_row(&format!("{COLS} {sql} LIMIT 1"), p, play).optional();
        // A door's plays are counted by name, whatever its case.
        let most = |sql: &str, p: &[&dyn rusqlite::ToSql]| {
            db.query_row(&format!("SELECT door, COUNT(*) n FROM door_plays {sql} GROUP BY UPPER(door) ORDER BY n DESC, MAX(started) DESC LIMIT 1"), p, |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
        };
        let mut s = DoorStats {
            last: one("ORDER BY started DESC, id DESC", &[])?,
            popular: most("", &[])?,
            // The handle comes from their latest play.
            top_player: db
                .query_row(
                    "SELECT handle, SUM(minutes) m, MAX(id) FROM door_plays GROUP BY user_id HAVING m > 0 ORDER BY m DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)),
                )
                .optional()?,
            minutes: db.query_row("SELECT COALESCE(SUM(minutes), 0) FROM door_plays", [], |r| r.get::<_, i64>(0))? as u64,
            plays_today: db.query_row("SELECT COUNT(*) FROM door_plays WHERE started >= ?1", [day], |r| r.get(0))?,
            marathon: one("WHERE minutes > 0 ORDER BY minutes DESC, started DESC", &[])?,
            my_favorite: most("WHERE user_id = ?1", &[&user])?,
            my_plays: db.query_row("SELECT COUNT(*) FROM door_plays WHERE user_id = ?1", [user], |r| r.get(0))?,
            my_last: one("WHERE user_id = ?1 ORDER BY started DESC, id DESC", &[&user])?,
            ..DoorStats::default()
        };
        let mut stmt = db.prepare("SELECT UPPER(door), MAX(started) FROM door_plays GROUP BY UPPER(door)")?;
        s.last_by_door = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        let mut stmt = db.prepare("SELECT DISTINCT UPPER(door) FROM door_plays WHERE user_id = ?1")?;
        s.my_doors = stmt.query_map([user], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(s)
    }

    // ---- newscan selections (2.0's `ME`/`FE`) ------------------------------

    /// The areas (by file name) a user has taken out of newscans; `kind`
    /// is `M` (messages) or `F` (files).
    pub fn scan_off(&self, user: u32, kind: char) -> anyhow::Result<std::collections::HashSet<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT area FROM scan_off WHERE user_id = ?1 AND kind = ?2")?;
        let rows = stmt.query_map(params![user, kind.to_string()], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn set_scan(&self, user: u32, kind: char, area: &str, on: bool) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        if on {
            db.execute("DELETE FROM scan_off WHERE user_id = ?1 AND kind = ?2 AND area = ?3", params![user, kind.to_string(), area])?;
        } else {
            db.execute("INSERT OR IGNORE INTO scan_off (user_id, kind, area) VALUES (?1, ?2, ?3)", params![user, kind.to_string(), area])?;
        }
        Ok(())
    }

    /// `US`: every area back in everyone's newscans. Returns how many
    /// selections were cleared.
    pub fn scan_all_on(&self) -> anyhow::Result<usize> {
        Ok(self.db.lock().unwrap().execute("DELETE FROM scan_off", [])?)
    }

    // ---- the voting booth (2.0) ---------------------------------------------

    /// Every question, oldest first, with its answers and their votes.
    pub fn questions(&self) -> anyhow::Result<Vec<Question>> {
        let db = self.db.lock().unwrap();
        let mut qs: Vec<Question> = db
            .prepare("SELECT id, text, author, user_add FROM vote_questions ORDER BY id")?
            .query_map([], |r| Ok(Question { id: r.get(0)?, text: r.get(1)?, author: r.get(2)?, user_add: r.get(3)?, answers: Vec::new() }))?
            .collect::<Result<_, _>>()?;
        let mut stmt = db.prepare(
            "SELECT a.id, a.text, (SELECT COUNT(*) FROM votes v WHERE v.answer = a.id) FROM vote_answers a WHERE a.question = ?1 ORDER BY a.id",
        )?;
        for q in &mut qs {
            q.answers = stmt.query_map([q.id], |r| Ok(Answer { id: r.get(0)?, text: r.get(1)?, votes: r.get(2)? }))?.collect::<Result<_, _>>()?;
        }
        Ok(qs)
    }

    pub fn add_question(&self, text: &str, author: &str, user_add: bool, answers: &[String]) -> anyhow::Result<i64> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO vote_questions (text, author, user_add, created) VALUES (?1, ?2, ?3, ?4)",
            params![text, author, user_add, crate::clock::now().naive_local()],
        )?;
        let id = tx.last_insert_rowid();
        for a in answers {
            tx.execute("INSERT INTO vote_answers (question, text) VALUES (?1, ?2)", params![id, a])?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn add_answer(&self, question: i64, text: &str) -> anyhow::Result<i64> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT INTO vote_answers (question, text) VALUES (?1, ?2)", params![question, text])?;
        Ok(db.last_insert_rowid())
    }

    pub fn delete_question(&self, id: i64) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM votes WHERE question = ?1", [id])?;
        tx.execute("DELETE FROM vote_answers WHERE question = ?1", [id])?;
        tx.execute("DELETE FROM vote_questions WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// The answer a user gave to a question, if any.
    pub fn my_vote(&self, user: u32, question: i64) -> anyhow::Result<Option<i64>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT answer FROM votes WHERE user_id = ?1 AND question = ?2", params![user, question], |r| r.get(0)).optional()?)
    }

    pub fn vote(&self, user: u32, question: i64, answer: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT OR REPLACE INTO votes (user_id, question, answer) VALUES (?1, ?2, ?3)", params![user, question, answer])?;
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
                    telnet: r.get("address")?,
                    ssh: r.get("ssh")?,
                    sysop: r.get("sysop")?,
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
            "INSERT INTO bbslist (name, address, ssh, sysop, speed, software, storage, info, who_added)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)",
            params![b.name, b.telnet, b.ssh, b.sysop, b.software, b.storage, b.info, b.who_added],
        )?;
        Ok(())
    }

    pub fn update_bbs(&self, b: &Bbs) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE bbslist SET name = ?2, address = ?3, ssh = ?4, sysop = ?5, software = ?6, storage = ?7,
                info = ?8, who_added = ?9 WHERE id = ?1",
            params![b.id, b.name, b.telnet, b.ssh, b.sysop, b.software, b.storage, b.info, b.who_added],
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

fn insert_call(db: &Connection, c: &Call) -> anyhow::Result<()> {
    db.execute(
        "INSERT INTO callers (call_num, user_id, handle, location, at, new_user, area_code, transport)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![c.call_num as i64, c.user_id, c.handle, c.location, c.at, c.new_user, c.area_code, c.transport],
    )?;
    Ok(())
}

fn totals(db: &Connection) -> anyhow::Result<Counts> {
    let sum = history_sum(db)?;
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

// ---- points (new) -----------------------------------------------------------

impl SysData {
    /// Points of a kind given (`by_giver`) or had by `user` since `since`.
    pub fn points_since(&self, kind: &str, user: u32, by_giver: bool, since: NaiveDateTime) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        let who = if by_giver { "from_id" } else { "to_id" };
        let sql = format!("SELECT COALESCE(SUM(amount), 0) FROM points WHERE kind = ?1 AND {who} = ?2 AND at >= ?3");
        Ok(db.query_row(&sql, params![kind, user, since], |r| r.get::<_, i64>(0))? as u32)
    }

    /// Everything `user` was given since `since`.
    pub fn points_got_since(&self, user: u32, since: NaiveDateTime) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT COALESCE(SUM(amount), 0) FROM points WHERE to_id = ?1 AND at >= ?2", params![user, since], |r| r.get::<_, i64>(0))? as u32)
    }

    pub fn add_points(&self, e: &PointsEntry) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        points_insert(&db, e)?;
        Ok(())
    }

    /// Record `e` cut to what's left of the receiver's `cap` for its kind
    /// since `since`; with `once`, not at all if its giver already gave
    /// that kind on the same thing (an upvote paid once, however often
    /// the vote changes). What went in (0: nothing).
    pub fn add_points_capped(&self, e: &PointsEntry, cap: u32, since: NaiveDateTime, once: bool) -> anyhow::Result<u32> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if once {
            let had: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM points WHERE kind = ?1 AND from_id = ?2 AND on_what = ?3)",
                params![e.kind, e.from_id, e.on],
                |r| r.get(0),
            )?;
            if had {
                return Ok(0);
            }
        }
        let got: i64 = tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM points WHERE kind = ?1 AND to_id = ?2 AND at >= ?3",
            params![e.kind, e.to_id, since],
            |r| r.get(0),
        )?;
        let amount = e.amount.min(cap.saturating_sub(got as u32));
        if amount == 0 {
            return Ok(0);
        }
        points_insert(&tx, &PointsEntry { amount, ..e.clone() })?;
        tx.commit()?;
        Ok(amount)
    }

    /// What `user` got or spent lately, newest first (an upvote they
    /// gave cost them nothing).
    pub fn points_recent(&self, user: u32, n: usize) -> anyhow::Result<Vec<PointsEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM points WHERE to_id = ?1 OR (from_id = ?1 AND kind IN ('tip', 'ticket')) ORDER BY id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![user, n as i64], points_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// A lottery day's tickets, by caller: (id, handle, tickets).
    pub fn lottery_tickets(&self, day: &str) -> anyhow::Result<Vec<(u32, String, u32)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT from_id, MAX(from_handle), SUM(CAST(note AS INTEGER)) FROM points WHERE kind = 'ticket' AND on_what = ?1 GROUP BY from_id ORDER BY MIN(id)",
        )?;
        let rows = stmt.query_map([day], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u32)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// A lottery day's purchases in the order made: (id, handle,
    /// tickets). Tickets are numbered from 1 in this order.
    pub fn lottery_entries(&self, day: &str) -> anyhow::Result<Vec<(u32, String, u32)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT from_id, from_handle, CAST(note AS INTEGER) FROM points WHERE kind = 'ticket' AND on_what = ?1 ORDER BY id")?;
        let rows = stmt.query_map([day], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u32)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// A lottery day's pot: what its tickets cost.
    pub fn lottery_pot(&self, day: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT COALESCE(SUM(amount), 0) FROM points WHERE kind = 'ticket' AND on_what = ?1", [day], |r| r.get::<_, i64>(0))? as u32)
    }

    /// Days before `today` with tickets sold and nobody drawn.
    pub fn lottery_undrawn(&self, today: &str) -> anyhow::Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT DISTINCT on_what FROM points WHERE kind = 'ticket' AND on_what < ?1
             AND on_what NOT IN (SELECT on_what FROM points WHERE kind = 'lottery') ORDER BY on_what",
        )?;
        let rows = stmt.query_map([today], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Record a draw, unless that day was drawn already. True if recorded.
    pub fn lottery_record(&self, e: &PointsEntry) -> anyhow::Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let drawn: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM points WHERE kind = 'lottery' AND on_what = ?1)", [&e.on], |r| r.get(0))?;
        if drawn {
            return Ok(false);
        }
        points_insert(&tx, e)?;
        tx.commit()?;
        Ok(true)
    }

    /// The last draw.
    pub fn lottery_last(&self) -> anyhow::Result<Option<PointsEntry>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT * FROM points WHERE kind = 'lottery' ORDER BY id DESC LIMIT 1", [], points_row).optional()?)
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
    fn bans_and_security_log() {
        let sys = SysData::open_memory().unwrap();
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let past = crate::clock::now().naive_local() - chrono::Duration::hours(1);
        sys.record_signup("10.0.0.5").unwrap();
        sys.record_signup("10.0.0.6").unwrap();
        assert_eq!(sys.signups_since("10.0.0.5", past).unwrap(), (1, 2));
        sys.log_security("10.0.0.5", "wrong password").unwrap();
        sys.log_security("10.0.0.5", "banned for 15 minutes").unwrap();
        sys.set_ban("10.0.0.5", t, 1).unwrap();
        assert_eq!(sys.bans().unwrap(), [("10.0.0.5".to_string(), t, 1)]);
        assert_eq!(sys.security_since(t).unwrap(), (2, 1));
        assert_eq!(sys.security_events(1).unwrap()[0].2, "banned for 15 minutes");
        assert_eq!(sys.clear_ban("10.0.0.9").unwrap(), 0);
        assert_eq!(sys.clear_ban("all").unwrap(), 1);
        assert!(sys.bans().unwrap().is_empty());
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
    fn logged_calls_are_counted_and_numbered() {
        let sd = std::sync::Arc::new(SysData::open_memory().unwrap());
        sd.set_base(None, &Counts { calls: 10, ..Counts::default() }).unwrap();
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let sd = sd.clone();
                std::thread::spawn(move || {
                    let at = "2026-10-17T10:00:00".parse().unwrap();
                    sd.log_call(Call { call_num: 0, user_id: i, handle: String::new(), location: String::new(), at, new_user: false, area_code: String::new(), transport: "ssh".into() }).unwrap();
                })
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        let mut nums: Vec<_> = sd.calls(None, None).unwrap().into_iter().map(|c| c.call_num).collect();
        nums.sort();
        assert_eq!(nums, (11..=18).collect::<Vec<_>>());
        assert_eq!(sd.day(d("2026-10-17")).unwrap().calls, 8);
    }

    #[test]
    fn voting_booth() {
        let sd = SysData::open_memory().unwrap();
        let q = sd.add_question("Best door?", "SysOp", true, &["LORD".into(), "TW2002".into()]).unwrap();
        let qs = sd.questions().unwrap();
        let (lord, tw) = (qs[0].answers[0].id, qs[0].answers[1].id);
        sd.vote(1, q, lord).unwrap();
        sd.vote(2, q, lord).unwrap();
        sd.vote(2, q, tw).unwrap();
        let usurper = sd.add_answer(q, "Usurper").unwrap();
        sd.vote(3, q, usurper).unwrap();
        let votes: Vec<u32> = sd.questions().unwrap()[0].answers.iter().map(|a| a.votes).collect();
        assert_eq!(votes, [1, 1, 1]);
        assert_eq!(sd.my_vote(2, q).unwrap(), Some(tw));
        assert_eq!(sd.my_vote(4, q).unwrap(), None);
        sd.delete_question(q).unwrap();
        assert!(sd.questions().unwrap().is_empty());
        assert_eq!(sd.my_vote(1, q).unwrap(), None);
    }

    #[test]
    fn scan_selections() {
        let sd = SysData::open_memory().unwrap();
        sd.set_scan(1, 'M', "GENERAL", false).unwrap();
        sd.set_scan(1, 'M', "GENERAL", false).unwrap();
        sd.set_scan(1, 'F', "UTILS", false).unwrap();
        sd.set_scan(2, 'M', "SYSOP", false).unwrap();
        assert_eq!(sd.scan_off(1, 'M').unwrap().into_iter().collect::<Vec<_>>(), ["GENERAL"]);
        sd.set_scan(1, 'M', "GENERAL", true).unwrap();
        assert!(sd.scan_off(1, 'M').unwrap().is_empty());
        assert_eq!(sd.scan_all_on().unwrap(), 2);
        assert!(sd.scan_off(2, 'M').unwrap().is_empty());
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

    #[test]
    fn bbs_list_gets_ssh() {
        // A database from before SSH addresses: the column is added and
        // the old address reads as the telnet one.
        let dir = std::env::temp_dir().join(format!("inq-bbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        let _ = std::fs::remove_file(&path);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE bbslist (id INTEGER PRIMARY KEY, name TEXT NOT NULL, address TEXT NOT NULL, sysop TEXT NOT NULL,
                speed INTEGER NOT NULL, software TEXT NOT NULL, storage TEXT NOT NULL, info TEXT NOT NULL, who_added TEXT NOT NULL);
             INSERT INTO bbslist VALUES (1, 'Route 666', 'r666.example:23', 'x', 33600, 'Iniquity', '', '', '');",
        )
        .unwrap();
        drop(db);
        let sd = SysData::open(&path).unwrap();
        let mut b = sd.bbs_list().unwrap().remove(0);
        assert_eq!((b.telnet.as_str(), b.ssh.as_str()), ("r666.example:23", ""));
        b.ssh = "r666.example:2222".into();
        sd.update_bbs(&b).unwrap();
        assert_eq!(sd.bbs_list().unwrap()[0].ssh, "r666.example:2222");
        drop(sd);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn door_plays() {
        let sd = SysData::open_memory().unwrap();
        let at = |h: u32| d("2026-10-24").and_hms_opt(h, 0, 0).unwrap();
        let p = |handle: &str, door: &str, h, minutes| Play { handle: handle.into(), door: door.into(), started: at(h), minutes };
        sd.add_play(1, &p("phrac", "LORD", 1, 20)).unwrap();
        sd.add_play(2, &p("subcarrier", "lord", 2, 5)).unwrap();
        sd.add_play(2, &p("subcarrier", "BRE", 9, 50)).unwrap();
        sd.add_play(1, &p("phrac", "Tarot", 10, 0)).unwrap();
        let s = sd.door_stats(1, at(5)).unwrap();
        assert_eq!(s.last.unwrap().door, "Tarot");
        assert_eq!(s.popular.map(|(d, n)| (d.to_uppercase(), n)), Some(("LORD".into(), 2)));
        assert_eq!(s.top_player, Some(("subcarrier".into(), 55)));
        assert_eq!((s.minutes, s.plays_today), (75, 2));
        assert_eq!(s.marathon.unwrap().door, "BRE");
        assert_eq!(s.last_by_door["LORD"], at(2));
        assert_eq!((s.my_plays, s.my_last.unwrap().door), (2, "Tarot".to_string()));
        assert_eq!(s.my_favorite, Some(("Tarot".into(), 1)));
        assert!(s.my_doors.contains("LORD") && !s.my_doors.contains("BRE"));
    }

    #[test]
    fn points_ledger() {
        let sd = SysData::open_memory().unwrap();
        let at = d("2026-10-27").and_hms_opt(12, 0, 0).unwrap();
        let day = d("2026-10-27").and_hms_opt(0, 0, 0).unwrap();
        let e = |kind: &str, from: u32, to: u32, amount: u32, on: &str, note: &str| PointsEntry {
            at,
            kind: kind.into(),
            from_id: from,
            from: format!("u{from}"),
            to_id: to,
            to: format!("u{to}"),
            amount,
            on: on.into(),
            note: note.into(),
        };
        // An upvote pays once per voter and post, up to the cap.
        assert_eq!(sd.add_points_capped(&e("upvote", 2, 1, 1, "10", ""), 2, day, true).unwrap(), 1);
        assert_eq!(sd.add_points_capped(&e("upvote", 2, 1, 1, "10", ""), 2, day, true).unwrap(), 0);
        assert_eq!(sd.add_points_capped(&e("upvote", 3, 1, 1, "10", ""), 2, day, true).unwrap(), 1);
        assert_eq!(sd.add_points_capped(&e("upvote", 4, 1, 1, "10", ""), 2, day, true).unwrap(), 0);
        // Doors are cut to what's left of the cap.
        assert_eq!(sd.add_points_capped(&e("door", 0, 1, 7, "LORD", "70"), 10, day, false).unwrap(), 7);
        assert_eq!(sd.add_points_capped(&e("door", 0, 1, 7, "BRE", "70"), 10, day, false).unwrap(), 3);
        assert_eq!(sd.points_since("upvote", 1, false, day).unwrap(), 2);
        assert_eq!(sd.points_since("upvote", 2, true, day).unwrap(), 1);
        // The lottery: tickets by caller, the pot, and each day drawn once.
        sd.add_points(&e("ticket", 1, 0, 20, "2026-10-26", "2")).unwrap();
        sd.add_points(&e("ticket", 2, 0, 10, "2026-10-26", "1")).unwrap();
        sd.add_points(&e("ticket", 1, 0, 10, "2026-10-26", "1")).unwrap();
        sd.add_points(&e("ticket", 2, 0, 10, "2026-10-27", "1")).unwrap();
        assert_eq!(sd.lottery_tickets("2026-10-26").unwrap(), vec![(1, "u1".into(), 3), (2, "u2".into(), 1)]);
        assert_eq!(sd.lottery_pot("2026-10-26").unwrap(), 40);
        assert_eq!(sd.lottery_undrawn("2026-10-27").unwrap(), vec!["2026-10-26".to_string()]);
        assert!(sd.lottery_record(&e("lottery", 0, 2, 90, "2026-10-26", "4")).unwrap());
        assert!(!sd.lottery_record(&e("lottery", 0, 1, 90, "2026-10-26", "4")).unwrap());
        assert!(sd.lottery_undrawn("2026-10-28").unwrap() == vec!["2026-10-27".to_string()]);
        assert_eq!(sd.lottery_last().unwrap().unwrap().to_id, 2);
        // A caller's ledger: what they got, and what they spent.
        let mine: Vec<String> = sd.points_recent(2, 10).unwrap().iter().map(|e| e.kind.clone()).collect();
        assert_eq!(mine, ["lottery", "ticket", "ticket"]);
    }
}
