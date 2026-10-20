//! User database (SQLite), replacing USERS.DAT / USERS.IDX.

use std::path::Path;
use std::sync::Mutex;

use chrono::NaiveDate;
use iniquity_term::{Color, Palette};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::user::User;

pub struct UserStore {
    db: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    handle        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    real_name     TEXT NOT NULL COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    phone         TEXT NOT NULL,
    birth_date    TEXT,
    location      TEXT NOT NULL,
    address       TEXT NOT NULL,
    zip_code      TEXT NOT NULL,
    note          TEXT NOT NULL,
    sysop_note    TEXT NOT NULL,
    sex           TEXT NOT NULL,
    sl            INTEGER NOT NULL,
    dsl           INTEGER NOT NULL,
    level         TEXT NOT NULL,
    flags         TEXT NOT NULL,
    hot_keys      INTEGER NOT NULL,
    expert        INTEGER NOT NULL,
    pause         INTEGER NOT NULL,
    quote         INTEGER NOT NULL,
    yes_no_bars   INTEGER NOT NULL,
    deleted       INTEGER NOT NULL,
    palette       TEXT NOT NULL,
    page_length   INTEGER NOT NULL,
    start_menu    TEXT NOT NULL,
    total_calls   INTEGER NOT NULL,
    calls_today   INTEGER NOT NULL,
    first_call    TEXT,
    last_call     TEXT,
    time_today    INTEGER NOT NULL,
    time_per_day  INTEGER NOT NULL,
    posts         INTEGER NOT NULL,
    email         INTEGER NOT NULL,
    uploads       INTEGER NOT NULL,
    downloads     INTEGER NOT NULL,
    upload_kb     INTEGER NOT NULL,
    download_kb   INTEGER NOT NULL,
    file_points   INTEGER NOT NULL,
    post_call     INTEGER NOT NULL,
    limit_dl      INTEGER NOT NULL,
    limit_dl_kb   INTEGER NOT NULL,
    today_dl      INTEGER NOT NULL,
    today_dl_kb   INTEGER NOT NULL,
    uldl_ratio    INTEGER NOT NULL,
    kb_ratio      INTEGER NOT NULL,
    cur_msg_area  INTEGER NOT NULL,
    cur_file_area INTEGER NOT NULL,
    conf_msg      INTEGER NOT NULL,
    conf_file     INTEGER NOT NULL,
    autosig       TEXT NOT NULL,
    vote_yes      INTEGER NOT NULL,
    vote_no       INTEGER NOT NULL,
    file_scan     TEXT,
    theme         TEXT NOT NULL DEFAULT '',
    last_rep      TEXT NOT NULL DEFAULT '',
    bank_time     INTEGER NOT NULL DEFAULT 0,
    bank_kb       INTEGER NOT NULL DEFAULT 0,
    bank_time_in  INTEGER NOT NULL DEFAULT 0,
    bank_time_out INTEGER NOT NULL DEFAULT 0,
    bank_kb_in    INTEGER NOT NULL DEFAULT 0,
    bank_kb_out   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS users_real_name ON users(real_name);
";

/// Every column but `id`, in the order `bind` supplies them.
const COLUMNS: &[&str] = &[
    "handle", "real_name", "password_hash", "phone", "birth_date", "location", "address",
    "zip_code", "note", "sysop_note", "sex", "sl", "dsl", "level", "flags", "hot_keys", "expert",
    "pause", "quote", "yes_no_bars", "deleted", "palette", "page_length", "start_menu",
    "total_calls", "calls_today", "first_call", "last_call", "time_today", "time_per_day", "posts",
    "email", "uploads", "downloads", "upload_kb", "download_kb", "file_points", "post_call",
    "limit_dl", "limit_dl_kb", "today_dl", "today_dl_kb", "uldl_ratio", "kb_ratio",
    "cur_msg_area", "cur_file_area", "conf_msg", "conf_file", "autosig", "vote_yes", "vote_no",
    "file_scan", "theme", "last_rep", "bank_time", "bank_kb", "bank_time_in", "bank_time_out",
    "bank_kb_in", "bank_kb_out",
];

/// Add columns newer than the database (`ALTER TABLE`, since SQLite
/// can't `CREATE TABLE IF NOT EXISTS` its way to new columns).
fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let have: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('users')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if !have.iter().any(|c| c == "file_scan") {
        db.execute("ALTER TABLE users ADD COLUMN file_scan TEXT", [])?;
    }
    if !have.iter().any(|c| c == "theme") {
        db.execute("ALTER TABLE users ADD COLUMN theme TEXT NOT NULL DEFAULT ''", [])?;
    }
    if !have.iter().any(|c| c == "last_rep") {
        db.execute("ALTER TABLE users ADD COLUMN last_rep TEXT NOT NULL DEFAULT ''", [])?;
    }
    for col in ["bank_time", "bank_kb", "bank_time_in", "bank_time_out", "bank_kb_in", "bank_kb_out"] {
        if !have.iter().any(|c| c == col) {
            db.execute(&format!("ALTER TABLE users ADD COLUMN {col} INTEGER NOT NULL DEFAULT 0"), [])?;
        }
    }
    Ok(())
}

fn palette_to_text(p: &Palette) -> String {
    p.0.iter().map(|c| format!("{},{},{}", c.fore, c.back, c.blink as u8)).collect::<Vec<_>>().join(";")
}

fn palette_from_text(s: &str) -> Palette {
    let mut p = Palette::default();
    for (slot, part) in p.0.iter_mut().zip(s.split(';')) {
        let n: Vec<u8> = part.split(',').filter_map(|v| v.parse().ok()).collect();
        if let [fore, back, blink] = n[..] {
            *slot = Color { fore: fore & 15, back: back & 7, blink: blink != 0 };
        }
    }
    p
}

fn char_col(s: String) -> char {
    s.chars().next().unwrap_or(' ')
}

fn from_row(r: &Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get("id")?,
        handle: r.get("handle")?,
        real_name: r.get("real_name")?,
        password_hash: r.get("password_hash")?,
        phone: r.get("phone")?,
        birth_date: r.get::<_, Option<NaiveDate>>("birth_date")?,
        location: r.get("location")?,
        address: r.get("address")?,
        zip_code: r.get("zip_code")?,
        note: r.get("note")?,
        sysop_note: r.get("sysop_note")?,
        sex: char_col(r.get("sex")?),
        sl: r.get("sl")?,
        dsl: r.get("dsl")?,
        level: char_col(r.get("level")?),
        flags: r.get::<_, String>("flags")?.chars().collect(),
        hot_keys: r.get("hot_keys")?,
        expert: r.get("expert")?,
        pause: r.get("pause")?,
        quote: r.get("quote")?,
        yes_no_bars: r.get("yes_no_bars")?,
        deleted: r.get("deleted")?,
        palette: palette_from_text(&r.get::<_, String>("palette")?),
        page_length: r.get("page_length")?,
        start_menu: r.get("start_menu")?,
        total_calls: r.get("total_calls")?,
        calls_today: r.get("calls_today")?,
        first_call: r.get("first_call")?,
        last_call: r.get("last_call")?,
        time_today: r.get("time_today")?,
        time_per_day: r.get("time_per_day")?,
        posts: r.get("posts")?,
        email: r.get("email")?,
        uploads: r.get("uploads")?,
        downloads: r.get("downloads")?,
        upload_kb: r.get::<_, i64>("upload_kb")?.max(0) as u64,
        download_kb: r.get::<_, i64>("download_kb")?.max(0) as u64,
        file_points: r.get("file_points")?,
        post_call: r.get("post_call")?,
        limit_dl: r.get("limit_dl")?,
        limit_dl_kb: r.get("limit_dl_kb")?,
        today_dl: r.get("today_dl")?,
        today_dl_kb: r.get("today_dl_kb")?,
        uldl_ratio: r.get("uldl_ratio")?,
        kb_ratio: r.get("kb_ratio")?,
        cur_msg_area: r.get("cur_msg_area")?,
        cur_file_area: r.get("cur_file_area")?,
        conf_msg: r.get("conf_msg")?,
        conf_file: r.get("conf_file")?,
        autosig: r.get::<_, String>("autosig")?.lines().map(String::from).collect(),
        vote_yes: r.get("vote_yes")?,
        vote_no: r.get("vote_no")?,
        file_scan: r.get("file_scan")?,
        theme: r.get("theme")?,
        last_rep: r.get("last_rep")?,
        bank_time: r.get("bank_time")?,
        bank_kb: r.get("bank_kb")?,
        bank_time_in: r.get("bank_time_in")?,
        bank_time_out: r.get("bank_time_out")?,
        bank_kb_in: r.get("bank_kb_in")?,
        bank_kb_out: r.get("bank_kb_out")?,
    })
}

/// Bind a user's fields in `COLUMNS` order.
fn bind(u: &User) -> Vec<Box<dyn rusqlite::ToSql + '_>> {
    vec![
        Box::new(&u.handle), Box::new(&u.real_name), Box::new(&u.password_hash), Box::new(&u.phone),
        Box::new(u.birth_date), Box::new(&u.location), Box::new(&u.address), Box::new(&u.zip_code),
        Box::new(&u.note), Box::new(&u.sysop_note), Box::new(u.sex.to_string()), Box::new(u.sl),
        Box::new(u.dsl), Box::new(u.level.to_string()), Box::new(u.flags.iter().collect::<String>()),
        Box::new(u.hot_keys), Box::new(u.expert), Box::new(u.pause), Box::new(u.quote),
        Box::new(u.yes_no_bars), Box::new(u.deleted), Box::new(palette_to_text(&u.palette)),
        Box::new(u.page_length), Box::new(&u.start_menu), Box::new(u.total_calls),
        Box::new(u.calls_today), Box::new(u.first_call), Box::new(u.last_call), Box::new(u.time_today),
        Box::new(u.time_per_day), Box::new(u.posts), Box::new(u.email), Box::new(u.uploads),
        Box::new(u.downloads), Box::new(u.upload_kb as i64), Box::new(u.download_kb as i64),
        Box::new(u.file_points), Box::new(u.post_call), Box::new(u.limit_dl), Box::new(u.limit_dl_kb),
        Box::new(u.today_dl), Box::new(u.today_dl_kb), Box::new(u.uldl_ratio), Box::new(u.kb_ratio),
        Box::new(u.cur_msg_area), Box::new(u.cur_file_area), Box::new(u.conf_msg), Box::new(u.conf_file),
        Box::new(u.autosig.join("\n")), Box::new(u.vote_yes), Box::new(u.vote_no), Box::new(u.file_scan),
        Box::new(u.theme.clone()),
        Box::new(u.last_rep.clone()),
        Box::new(u.bank_time),
        Box::new(u.bank_kb),
        Box::new(u.bank_time_in),
        Box::new(u.bank_time_out),
        Box::new(u.bank_kb_in),
        Box::new(u.bank_kb_out),
    ]
}

impl UserStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = crate::sysdata::open_db(path)?;
        db.execute_batch(SCHEMA)?;
        migrate(&db)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// The first handle (alphabetically) starting with `prefix`, for tab
    /// completion (2.0's alias completion).
    pub fn complete(&self, prefix: &str) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap();
        let mut pattern: String = prefix.chars().flat_map(|c| if matches!(c, '\\' | '%' | '_') { vec!['\\', c] } else { vec![c] }).collect();
        pattern.push('%');
        Ok(db
            .query_row(
                "SELECT handle FROM users WHERE handle LIKE ?1 ESCAPE '\\' AND NOT deleted ORDER BY handle COLLATE NOCASE LIMIT 1",
                [pattern],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// `userSearch`: by handle, real name, or user number.
    pub fn find(&self, name: &str, include_deleted: bool) -> anyhow::Result<Option<User>> {
        let name = name.trim();
        let id: i64 = name.parse().unwrap_or(-1);
        let db = self.db.lock().unwrap();
        let user = db
            .query_row(
                "SELECT * FROM users WHERE (handle = ?1 OR real_name = ?1 OR id = ?2)
                 AND (?3 OR NOT deleted) ORDER BY id LIMIT 1",
                params![name, id, include_deleted],
                from_row,
            )
            .optional()?;
        Ok(user)
    }

    pub fn get(&self, id: u32) -> anyhow::Result<Option<User>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT * FROM users WHERE id = ?1", [id], from_row).optional()?)
    }

    /// Insert a new user, assigning `u.id` (or keeping it, for imports).
    pub fn insert(&self, u: &mut User) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let mut cols = COLUMNS.to_vec();
        {
            let mut values = bind(u);
            if u.id != 0 {
                cols.push("id");
                values.push(Box::new(u.id));
            }
            let marks = vec!["?"; cols.len()].join(",");
            let sql = format!("INSERT INTO users ({}) VALUES ({marks})", cols.join(","));
            db.execute(&sql, rusqlite::params_from_iter(values.iter().map(|b| b.as_ref())))?;
        }
        u.id = db.last_insert_rowid() as u32;
        Ok(())
    }

    /// `userSave`.
    pub fn save(&self, u: &User) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let sets = COLUMNS.iter().map(|c| format!("{c} = ?")).collect::<Vec<_>>().join(",");
        let sql = format!("UPDATE users SET {sets} WHERE id = ?");
        let mut values = bind(u);
        values.push(Box::new(u.id));
        db.execute(&sql, rusqlite::params_from_iter(values.iter().map(|b| b.as_ref())))?;
        Ok(())
    }

    /// All users, by number.
    pub fn list(&self) -> anyhow::Result<Vec<User>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM users ORDER BY id")?;
        let users = stmt.query_map([], from_row)?.collect::<Result<_, _>>()?;
        Ok(users)
    }

    pub fn count(&self) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT COUNT(*) FROM users WHERE NOT deleted", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_find_save() {
        let store = UserStore::open_memory().unwrap();
        let mut u = User { handle: "Fiend".into(), real_name: "Mike Fricker".into(), ..User::default() };
        u.set_password("pass");
        u.flags.insert('A');
        u.palette.0[3] = Color::new(14, 1);
        store.insert(&mut u).unwrap();
        assert_eq!(u.id, 1);

        for key in ["fiend", "MIKE FRICKER", "1"] {
            assert_eq!(store.find(key, false).unwrap().unwrap().handle, "Fiend");
        }
        let mut got = store.find("fiend", false).unwrap().unwrap();
        assert!(got.check_password("pass") && !got.check_password("PASS"));
        assert!(got.flags.contains(&'A'));
        assert_eq!(got.palette.0[3], Color::new(14, 1));

        got.deleted = true;
        got.total_calls = 7;
        store.save(&got).unwrap();
        assert!(store.find("fiend", false).unwrap().is_none());
        assert_eq!(store.find("fiend", true).unwrap().unwrap().total_calls, 7);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn completes_handles() {
        let store = UserStore::open_memory().unwrap();
        for h in ["Zeta", "Phrac", "phreak_", "Gone"] {
            let mut u = User { handle: h.into(), real_name: h.into(), deleted: h == "Gone", ..User::default() };
            store.insert(&mut u).unwrap();
        }
        assert_eq!(store.complete("ph").unwrap().as_deref(), Some("Phrac"));
        assert_eq!(store.complete("PHRE").unwrap().as_deref(), Some("phreak_"));
        assert_eq!(store.complete("phreak_").unwrap().as_deref(), Some("phreak_"));
        assert_eq!(store.complete("%").unwrap(), None);
        assert_eq!(store.complete("Go").unwrap(), None);
    }
}
