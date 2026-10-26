//! User database (SQLite), replacing USERS.DAT / USERS.IDX.

use std::path::Path;
use std::sync::Mutex;

use chrono::NaiveDate;
use fluxbbs_term::{Color, Palette};
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
    bank_kb_out   INTEGER NOT NULL DEFAULT 0,
    ssh_keys      TEXT NOT NULL DEFAULT '',
    plan          TEXT NOT NULL DEFAULT '',
    ssh_user      TEXT NOT NULL DEFAULT '',
    time_zone     TEXT NOT NULL DEFAULT '',
    thread_sort   TEXT NOT NULL DEFAULT '',
    last_call_at  TEXT,
    area_sort     TEXT NOT NULL DEFAULT '',
    fav_boards    TEXT NOT NULL DEFAULT '',
    profile_header TEXT NOT NULL DEFAULT '',
    show_real_name INTEGER NOT NULL DEFAULT 0,
    ai_profile    TEXT NOT NULL DEFAULT '',
    text_mode     TEXT NOT NULL DEFAULT '',
    code_bg       TEXT NOT NULL DEFAULT '',
    code_theme    TEXT NOT NULL DEFAULT '',
    md_theme      TEXT NOT NULL DEFAULT '',
    animate       TEXT NOT NULL DEFAULT ''
);
-- New: a SysOp's private notes on a caller, dated, for SysOps only.
CREATE TABLE IF NOT EXISTS user_notes (
    id       INTEGER PRIMARY KEY,
    user_id  INTEGER NOT NULL,
    by       TEXT NOT NULL,
    at       TEXT NOT NULL,
    text     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS user_notes_user ON user_notes(user_id);
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
    "bank_kb_in", "bank_kb_out", "ssh_keys", "plan", "ssh_user", "time_zone", "thread_sort", "last_call_at", "area_sort",
    "fav_boards", "profile_header", "show_real_name", "ai_profile", "text_mode", "code_bg", "code_theme", "md_theme", "animate",
];

fn write(db: &Connection, u: &User) -> anyhow::Result<()> {
    let sets = COLUMNS.iter().map(|c| format!("{c} = ?")).collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE users SET {sets} WHERE id = ?");
    let mut values = bind(u);
    values.push(Box::new(u.id));
    db.execute(&sql, rusqlite::params_from_iter(values.iter().map(|b| b.as_ref())))?;
    Ok(())
}

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
    if !have.iter().any(|c| c == "last_call_at") {
        db.execute("ALTER TABLE users ADD COLUMN last_call_at TEXT", [])?;
    }
    if !have.iter().any(|c| c == "last_rep") {
        db.execute("ALTER TABLE users ADD COLUMN last_rep TEXT NOT NULL DEFAULT ''", [])?;
    }
    if !have.iter().any(|c| c == "show_real_name") {
        db.execute("ALTER TABLE users ADD COLUMN show_real_name INTEGER NOT NULL DEFAULT 0", [])?;
    }
    for col in ["ssh_keys", "plan", "ssh_user", "time_zone", "thread_sort", "area_sort", "fav_boards", "profile_header", "ai_profile", "text_mode", "code_bg", "code_theme", "md_theme", "animate"] {
        if !have.iter().any(|c| c == col) {
            db.execute(&format!("ALTER TABLE users ADD COLUMN {col} TEXT NOT NULL DEFAULT ''"), [])?;
        }
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
        ssh_keys: r.get::<_, String>("ssh_keys")?.lines().map(String::from).collect(),
        plan: r.get::<_, String>("plan")?.lines().map(String::from).collect(),
        ssh_user: r.get("ssh_user")?,
        time_zone: r.get("time_zone")?,
        thread_sort: r.get("thread_sort")?,
        last_call_at: r.get("last_call_at")?,
        area_sort: r.get("area_sort")?,
        fav_boards: r.get("fav_boards")?,
        profile_header: r.get::<_, String>("profile_header")?.lines().map(String::from).collect(),
        show_real_name: r.get("show_real_name")?,
        ai_profile: r.get("ai_profile")?,
        text_mode: r.get("text_mode")?,
        code_bg: r.get("code_bg")?,
        code_theme: r.get("code_theme")?,
        md_theme: r.get("md_theme")?,
        animate: r.get("animate")?,
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
        Box::new(u.ssh_keys.join("\n")),
        Box::new(u.plan.join("\n")),
        Box::new(u.ssh_user.clone()),
        Box::new(u.time_zone.clone()),
        Box::new(u.thread_sort.clone()),
        Box::new(u.last_call_at),
        Box::new(u.area_sort.clone()),
        Box::new(u.fav_boards.clone()),
        Box::new(u.profile_header.join("\n")),
        Box::new(u.show_real_name),
        Box::new(u.ai_profile.clone()),
        Box::new(u.text_mode.clone()),
        Box::new(u.code_bg.clone()),
        Box::new(u.code_theme.clone()),
        Box::new(u.md_theme.clone()),
        Box::new(u.animate.clone()),
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
    /// New: the (undeleted) user with exactly this handle, not a real name
    /// or number: for what's kept by handle, like moderators and bans.
    pub fn find_handle(&self, handle: &str) -> anyhow::Result<Option<User>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT * FROM users WHERE handle = ?1 AND NOT deleted ORDER BY id LIMIT 1", [handle.trim()], from_row).optional()?)
    }

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

    /// New: the account whose SSH user name is `name`.
    pub fn by_ssh_user(&self, name: &str) -> anyhow::Result<Option<User>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT * FROM users WHERE ssh_user = ?1 COLLATE NOCASE AND ssh_user <> '' AND NOT deleted ORDER BY id LIMIT 1", [name.trim()], from_row)
            .optional()?)
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
        write(&db, u)
    }

    /// New: put into the record as it is now what changed from `base` to
    /// `mine` (`User::merge_edit`), reading and writing it in one go, so
    /// two sessions (or `fluxbbs` beside the board) can't lose each
    /// other's changes. Returns the record as saved.
    pub fn merge(&self, base: &User, mine: &User) -> anyhow::Result<User> {
        self.merge_with(base, mine, false)
    }

    /// [`Self::merge`]; `exact`: counters changed are set to `mine`'s
    /// values rather than moved by the change.
    pub fn merge_with(&self, base: &User, mine: &User, exact: bool) -> anyhow::Result<User> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = tx.query_row("SELECT * FROM users WHERE id = ?1", [mine.id], from_row).optional()?;
        let u = match now {
            Some(mut n) => {
                if exact { n.merge_values(base, mine) } else { n.merge_edit(base, mine) }
                n
            }
            None => mine.clone(),
        };
        write(&tx, &u)?;
        tx.commit()?;
        Ok(u)
    }

    /// New: a SysOp's notes on a caller, oldest first: (id, by, when,
    /// text).
    pub fn notes(&self, user_id: u32) -> anyhow::Result<Vec<(i64, String, chrono::NaiveDateTime, String)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT id, by, at, text FROM user_notes WHERE user_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn add_note(&self, user_id: u32, by: &str, text: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT INTO user_notes (user_id, by, at, text) VALUES (?1, ?2, ?3, ?4)", params![user_id, by, crate::clock::now().naive_local(), text])?;
        Ok(())
    }

    pub fn delete_note(&self, id: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM user_notes WHERE id = ?1", [id])?;
        Ok(())
    }

    /// All users, by number.
    pub fn list(&self) -> anyhow::Result<Vec<User>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM users ORDER BY id")?;
        let users = stmt.query_map([], from_row)?.collect::<Result<_, _>>()?;
        Ok(users)
    }

    /// Every live user's handle, in alphabetical order.
    pub fn handles(&self) -> anyhow::Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT handle FROM users WHERE NOT deleted ORDER BY handle COLLATE NOCASE")?;
        let names = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(names)
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
    fn merges_from_one_base_both_count() {
        let store = UserStore::open_memory().unwrap();
        let mut u = User { handle: "Ann".into(), posts: 3, ..User::default() };
        store.insert(&mut u).unwrap();
        // Two nodes (or a vote and the caller) from the same copy.
        let (mut a, mut b) = (u.clone(), u.clone());
        a.posts += 1;
        b.posts += 1;
        b.time_per_day = 120;
        store.merge(&u, &a).unwrap();
        let saved = store.merge(&u, &b).unwrap();
        assert_eq!((saved.posts, saved.time_per_day), (5, 120));
        assert_eq!(store.get(u.id).unwrap().unwrap().posts, 5);
    }

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

        assert_eq!(got.last_call_at, None);
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 22).unwrap().and_hms_opt(20, 41, 5).unwrap();
        got.last_call_at = Some(at);
        got.deleted = true;
        got.total_calls = 7;
        store.save(&got).unwrap();
        assert!(store.find("fiend", false).unwrap().is_none());
        let back = store.find("fiend", true).unwrap().unwrap();
        assert_eq!((back.total_calls, back.last_call_at), (7, Some(at)));
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
