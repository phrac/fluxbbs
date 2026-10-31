//! File records (SQLite), replacing the per-area .DIR files and
//! FILEDESC.DAT. The files themselves live in each area's directory.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::fileareas::SortBy;

#[derive(Debug, Clone)]
pub struct FileRec {
    pub area: String,
    /// Position in the area, from 1.
    pub num: u32,
    pub name: String,
    pub size: u64,
    pub date: Option<NaiveDate>,
    pub uploader: String,
    pub uploaded: Option<NaiveDate>,
    pub downloads: u32,
    pub points: u32,
    /// Checked by a SysOp (or auto-validated); others can't download it.
    pub valid: bool,
    pub description: Vec<String>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
    id          INTEGER PRIMARY KEY,
    area        TEXT NOT NULL COLLATE NOCASE,
    num         INTEGER NOT NULL,
    name        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    date        TEXT,
    uploader    TEXT NOT NULL,
    uploaded    TEXT,
    downloads   INTEGER NOT NULL,
    points      INTEGER NOT NULL,
    valid       INTEGER NOT NULL,
    description TEXT NOT NULL,
    UNIQUE (area, num)
);
CREATE INDEX IF NOT EXISTS files_name ON files(name COLLATE NOCASE);
";

/// Added since a database was made; only ever at the end.
const STEPS: &[crate::sysdata::Step] = &[
    // New: a line about each file area (`*F`, `fluxbbs describe --areas`),
    // kept here rather than in fileareas.toml so it can be written while
    // the board runs.
    crate::sysdata::Step::Sql("CREATE TABLE IF NOT EXISTS area_info (area TEXT PRIMARY KEY COLLATE NOCASE, about TEXT NOT NULL)"),
];

fn from_row(r: &Row) -> rusqlite::Result<FileRec> {
    let desc: String = r.get("description")?;
    Ok(FileRec {
        area: r.get("area")?,
        num: r.get("num")?,
        name: r.get("name")?,
        size: r.get::<_, i64>("size")?.max(0) as u64,
        date: r.get("date")?,
        uploader: r.get("uploader")?,
        uploaded: r.get("uploaded")?,
        downloads: r.get("downloads")?,
        points: r.get("points")?,
        valid: r.get("valid")?,
        description: if desc.is_empty() { Vec::new() } else { desc.split('\n').map(String::from).collect() },
    })
}

/// DOS-style wildcard match (`*`, `?`), case-insensitive. A pattern
/// without a dot also matches any extension.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    fn m(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => m(&p[1..], n) || (!n.is_empty() && m(p, &n[1..])),
            (Some('?'), Some(_)) => m(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b) && m(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    m(&p, &n) || (!pattern.contains('.') && m(&format!("{pattern}.*").chars().collect::<Vec<_>>(), &n))
}

/// `text` as a LIKE pattern matching it literally.
fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// A DOS wildcard as a LIKE pattern (`*` any run, `?` any one).
fn wildcard_like(spec: &str) -> String {
    let mut out = String::with_capacity(spec.len());
    for c in spec.chars() {
        match c {
            '*' => out.push('%'),
            '?' => out.push('_'),
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// New: what a caller looks for, in words: every one must be in a file's
/// name or description (case doesn't matter), `"two words"` together, a
/// word with `*` or `?` a filespec for the name, and `-word` leaves out
/// what has it.
#[derive(Debug, Default, PartialEq)]
pub struct Find {
    pub want: Vec<String>,
    pub not: Vec<String>,
}

impl Find {
    pub fn parse(text: &str) -> Find {
        let mut find = Find::default();
        let mut chars = text.chars().peekable();
        loop {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            let Some(&c) = chars.peek() else { break };
            let not = c == '-';
            if not {
                chars.next();
            }
            let mut word = String::new();
            if chars.next_if_eq(&'"').is_some() {
                word.extend(chars.by_ref().take_while(|&c| c != '"'));
            } else {
                while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                    word.push(c);
                }
            }
            let word = word.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            if !word.is_empty() {
                if not { &mut find.not } else { &mut find.want }.push(word);
            }
        }
        find
    }

    pub fn is_empty(&self) -> bool {
        self.want.is_empty()
    }

    fn is_spec(word: &str) -> bool {
        word.contains(['*', '?'])
    }

    fn hit(word: &str, name: &str, text: &str) -> bool {
        if Find::is_spec(word) { wildcard(word, name) } else { name.to_lowercase().contains(word) || text.contains(word) }
    }

    /// Whether some text (an area's name and the line about it) has every
    /// word and none of the left-out ones. A filespec matches no area.
    pub fn matches_text(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        !self.want.is_empty() && self.want.iter().all(|w| !Find::is_spec(w) && text.contains(w.as_str())) && !self.not.iter().any(|w| text.contains(w.as_str()))
    }

    fn matches(&self, f: &FileRec) -> bool {
        let text = f.description.join("\n").to_lowercase();
        self.want.iter().all(|w| Find::hit(w, &f.name, &text)) && !self.not.iter().any(|w| Find::hit(w, &f.name, &text))
    }

    /// Best first: a word in the name counts most, more at its start; in
    /// the description, more for each time it's there (up to three).
    fn score(&self, f: &FileRec) -> u32 {
        let name = f.name.to_lowercase();
        let text = f.description.join("\n").to_lowercase();
        self.want
            .iter()
            .map(|w| {
                if Find::is_spec(w) {
                    return 4;
                }
                let mut s = 0;
                if name.starts_with(w.as_str()) {
                    s += 6;
                } else if name.contains(w.as_str()) {
                    s += 4;
                }
                s + text.matches(w.as_str()).count().min(3) as u32
            })
            .sum()
    }
}

pub struct FileBase {
    db: Mutex<Connection>,
}

impl FileBase {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut db = crate::sysdata::open_db(path)?;
        crate::sysdata::migrate(&mut db, "filebase", SCHEMA, STEPS)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let mut db = Connection::open_in_memory()?;
        crate::sysdata::migrate(&mut db, "filebase", SCHEMA, STEPS)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn count(&self, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COUNT(*) FROM files WHERE area = ?1", [area], |r| r.get(0))?)
    }

    /// How many validated files an area has, and the newest upload.
    pub fn summary(&self, area: &str) -> anyhow::Result<(u32, Option<NaiveDate>)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COUNT(*), MAX(uploaded) FROM files WHERE area = ?1 AND valid", [area], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    /// How many files an area has (checked or not), and its newest
    /// checked upload: what the area list shows, without reading the
    /// records.
    pub fn count_and_newest(&self, area: &str) -> anyhow::Result<(u32, Option<NaiveDate>)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COUNT(*), MAX(CASE WHEN valid THEN uploaded END) FROM files WHERE area = ?1", [area], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    /// New: how many files each area has had since `since` (every file,
    /// for None), by storage key in lower case; areas with none aren't
    /// in it. The newscan asks this once rather than reading every area.
    pub fn new_counts(&self, since: Option<NaiveDate>) -> anyhow::Result<HashMap<String, u32>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT area, COUNT(*) FROM files WHERE ?1 IS NULL OR uploaded >= ?1 GROUP BY area")?;
        let rows = stmt.query_map([since], |r| Ok((r.get::<_, String>(0)?.to_lowercase(), r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// `faSearchFile` over `areas` at once: the files whose name matches
    /// the DOS wildcard `spec`, or whose description has `text` in it
    /// (case doesn't matter), in area then number order. One query, so
    /// hundreds of areas cost no more than one.
    pub fn search(&self, areas: &[String], spec: Option<&str>, text: Option<&str>) -> anyhow::Result<Vec<FileRec>> {
        if areas.is_empty() {
            return Ok(Vec::new());
        }
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = areas.iter().map(|a| Box::new(a.clone()) as Box<dyn rusqlite::ToSql>).collect();
        let cond = match (spec, text) {
            (Some(s), _) => {
                let like = wildcard_like(s);
                params.push(Box::new(like.clone()));
                if s.contains('.') {
                    "name LIKE ? ESCAPE '\\'"
                } else {
                    params.push(Box::new(format!("{like}.%")));
                    "(name LIKE ? ESCAPE '\\' OR name LIKE ? ESCAPE '\\')"
                }
            }
            (None, Some(t)) => {
                params.push(Box::new(format!("%{}%", escape_like(t))));
                "description LIKE ? ESCAPE '\\'"
            }
            (None, None) => "1",
        };
        let sql = format!("SELECT * FROM files WHERE area IN ({}) AND {cond} ORDER BY area, num", vec!["?"; areas.len()].join(","));
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<FileRec> = db.prepare(&sql)?.query_map(rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())), from_row)?.collect::<Result<_, _>>()?;
        if let Some(s) = spec {
            rows.retain(|f| wildcard(s, &f.name));
        }
        Ok(rows)
    }

    /// New: the validated files in `areas` that `find` finds, best first
    /// (then newest), at most `limit`, and how many there were in all.
    /// The words narrow the query; the matching is done here, so a
    /// filespec and a phrase work as they do everywhere else.
    pub fn find_words(&self, areas: &[String], find: &Find, limit: usize) -> anyhow::Result<(Vec<FileRec>, usize)> {
        if areas.is_empty() || find.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut params: Vec<String> = areas.to_vec();
        let mut conds = Vec::new();
        for w in &find.want {
            if Find::is_spec(w) {
                params.push(wildcard_like(w));
                conds.push("name LIKE ? ESCAPE '\\'");
            } else {
                let like = format!("%{}%", escape_like(w));
                params.push(like.clone());
                params.push(like);
                conds.push("(name LIKE ? ESCAPE '\\' OR description LIKE ? ESCAPE '\\')");
            }
        }
        let sql = format!("SELECT * FROM files WHERE valid AND area IN ({}) AND {}", vec!["?"; areas.len()].join(","), conds.join(" AND "));
        let mut rows: Vec<FileRec> = {
            let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            db.prepare(&sql)?.query_map(rusqlite::params_from_iter(params.iter()), from_row)?.collect::<Result<_, _>>()?
        };
        rows.retain(|f| find.matches(f));
        let total = rows.len();
        rows.sort_by_cached_key(|f| (std::cmp::Reverse(find.score(f)), std::cmp::Reverse(f.uploaded), f.name.to_lowercase()));
        rows.truncate(limit);
        Ok((rows, total))
    }

    /// New: every area's line about it, by storage key in lower case.
    pub fn area_abouts(&self) -> anyhow::Result<HashMap<String, String>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT area, about FROM area_info")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?.to_lowercase(), r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every file in an area, in order.
    pub fn list(&self, area: &str) -> anyhow::Result<Vec<FileRec>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT * FROM files WHERE area = ?1 ORDER BY num")?;
        let rows = stmt.query_map([area], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn get(&self, area: &str, num: u32) -> anyhow::Result<Option<FileRec>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT * FROM files WHERE area = ?1 AND num = ?2", params![area, num], from_row).optional()?)
    }

    /// A file by name in an area (exact, case-insensitive).
    pub fn find(&self, area: &str, name: &str) -> anyhow::Result<Option<FileRec>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db
            .query_row(
                "SELECT * FROM files WHERE area = ?1 AND name = ?2 COLLATE NOCASE ORDER BY num LIMIT 1",
                params![area, name],
                from_row,
            )
            .optional()?)
    }

    pub fn add(&self, f: &mut FileRec) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        if f.num == 0 {
            f.num = db.query_row("SELECT COALESCE(MAX(num), 0) + 1 FROM files WHERE area = ?1", [&f.area], |r| r.get(0))?;
        }
        db.execute(
            "INSERT INTO files (area, num, name, size, date, uploader, uploaded, downloads, points, valid, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                f.area, f.num, f.name, f.size as i64, f.date, f.uploader, f.uploaded, f.downloads, f.points,
                f.valid, f.description.join("\n")
            ],
        )?;
        Ok(())
    }

    /// Add several records in one transaction (a bulk load holds the
    /// lock once, not once a file).
    pub fn add_all(&self, recs: &mut [FileRec]) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for f in recs.iter_mut() {
            if f.num == 0 {
                f.num = tx.query_row("SELECT COALESCE(MAX(num), 0) + 1 FROM files WHERE area = ?1", [&f.area], |r| r.get(0))?;
            }
            tx.execute(
                "INSERT INTO files (area, num, name, size, date, uploader, uploaded, downloads, points, valid, description)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    f.area, f.num, f.name, f.size as i64, f.date, f.uploader, f.uploaded, f.downloads, f.points,
                    f.valid, f.description.join("\n")
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn update(&self, f: &FileRec) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "UPDATE files SET name = ?3, size = ?4, date = ?5, uploader = ?6, uploaded = ?7,
                downloads = ?8, points = ?9, valid = ?10, description = ?11
             WHERE area = ?1 AND num = ?2",
            params![
                f.area, f.num, f.name, f.size as i64, f.date, f.uploader, f.uploaded, f.downloads, f.points,
                f.valid, f.description.join("\n")
            ],
        )?;
        Ok(())
    }

    /// Remove a file's record and close the gap in the numbering.
    pub fn remove(&self, area: &str, num: u32) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM files WHERE area = ?1 AND num = ?2", params![area, num])?;
        // Two steps so UNIQUE (area, num) holds after each statement.
        tx.execute("UPDATE files SET num = -(num - 1) WHERE area = ?1 AND num > ?2", params![area, num])?;
        tx.execute("UPDATE files SET num = -num WHERE area = ?1 AND num < 0", [area])?;
        tx.commit()?;
        Ok(())
    }

    /// `FilePack`: drop the records of files no longer in the area's
    /// directory (`dir`), closing up the numbering. Returns their names.
    /// (The original packed FILEDESC.DAT; descriptions live with their
    /// records here, so what's left to tidy is records of missing files.)
    pub fn prune_missing(&self, area: &str, dir: &Path) -> anyhow::Result<Vec<String>> {
        let mut gone = Vec::new();
        // Last first, so the numbers still to check don't move.
        for f in self.list(area)?.into_iter().rev() {
            if !dir.join(&f.name).is_file() {
                self.remove(area, f.num)?;
                gone.push(f.name);
            }
        }
        gone.reverse();
        Ok(gone)
    }

    /// Move a file's record to the end of another area.
    pub fn move_to(&self, area: &str, num: u32, to: &str) -> anyhow::Result<u32> {
        let Some(mut f) = self.get(area, num)? else { anyhow::bail!("no file {num} in {area}") };
        self.remove(area, num)?;
        f.area = to.to_string();
        f.num = 0;
        self.add(&mut f)?;
        Ok(f.num)
    }

    /// An area's storage key changed.
    pub fn rename_area(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE files SET area = ?2 WHERE area = ?1", params![from, to])?;
        db.execute("UPDATE area_info SET area = ?2 WHERE area = ?1", params![from, to])?;
        Ok(())
    }

    /// Forget every file record in an area.
    pub fn delete_area(&self, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("DELETE FROM area_info WHERE area = ?1", [area])?;
        Ok(db.execute("DELETE FROM files WHERE area = ?1", [area])? as u32)
    }

    /// New: the line about an area, empty if it has none.
    pub fn area_about(&self, area: &str) -> anyhow::Result<String> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT about FROM area_info WHERE area = ?1", [area], |r| r.get(0)).optional()?.unwrap_or_default())
    }

    /// New: set the line about an area; empty takes it away.
    pub fn set_area_about(&self, area: &str, about: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        if about.trim().is_empty() {
            db.execute("DELETE FROM area_info WHERE area = ?1", [area])?;
        } else {
            db.execute("INSERT OR REPLACE INTO area_info (area, about) VALUES (?1, ?2)", params![area, about.trim()])?;
        }
        Ok(())
    }

    /// `fsSortFileArea`: renumber an area's files in the given order.
    pub fn sort_area(&self, area: &str, by: SortBy, descending: bool) -> anyhow::Result<()> {
        if by == SortBy::None {
            return Ok(());
        }
        let mut files = self.list(area)?;
        let ext = |n: &str| n.rsplit_once('.').map(|(_, e)| e.to_ascii_uppercase()).unwrap_or_default();
        files.sort_by(|a, b| {
            let o = match by {
                SortBy::None => std::cmp::Ordering::Equal,
                SortBy::Name => a.name.to_ascii_uppercase().cmp(&b.name.to_ascii_uppercase()),
                SortBy::Extension => ext(&a.name).cmp(&ext(&b.name)),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Date => a.date.cmp(&b.date),
                SortBy::Uploaded => a.uploaded.cmp(&b.uploaded),
                SortBy::Uploader => a.uploader.to_ascii_uppercase().cmp(&b.uploader.to_ascii_uppercase()),
                SortBy::Downloads => a.downloads.cmp(&b.downloads),
            };
            if descending { o.reverse() } else { o }
        });
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("UPDATE files SET num = -num WHERE area = ?1", [area])?;
        for (i, f) in files.iter().enumerate() {
            tx.execute("UPDATE files SET num = ?3 WHERE area = ?1 AND num = ?2", params![area, -(f.num as i64), i as u32 + 1])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// New: `uploader`'s newest validated files in `areas`.
    pub fn by_uploader(&self, uploader: &str, areas: &[String], limit: u32) -> anyhow::Result<Vec<FileRec>> {
        if areas.is_empty() {
            return Ok(Vec::new());
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let sql = format!(
            "SELECT * FROM files WHERE valid AND uploader = ? COLLATE NOCASE AND area IN ({}) ORDER BY uploaded DESC, id DESC LIMIT {limit}",
            vec!["?"; areas.len()].join(",")
        );
        let p = rusqlite::params_from_iter(std::iter::once(&uploader as &dyn rusqlite::ToSql).chain(areas.iter().map(|a| a as &dyn rusqlite::ToSql)));
        let rows = db.prepare(&sql)?.query_map(p, from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// New: what the validated files in `areas` add up to, for the file
    /// menu's `%F?` codes. `since` counts the uploads on or after it.
    pub fn stats(&self, areas: &[String], since: Option<NaiveDate>) -> anyhow::Result<FileStats> {
        let mut s = FileStats { areas: areas.len() as u32, ..Default::default() };
        if areas.is_empty() {
            return Ok(s);
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let ins = vec!["?"; areas.len()].join(",");
        let w = format!("valid AND area IN ({ins})");
        let p = rusqlite::params_from_iter(areas);
        (s.files, s.bytes) = db.query_row(&format!("SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files WHERE {w}"), p.clone(), |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
        let one = |sql: String| db.query_row(&sql, p.clone(), from_row).optional();
        s.newest = one(format!("SELECT * FROM files WHERE {w} ORDER BY uploaded DESC, id DESC LIMIT 1"))?;
        s.popular = one(format!("SELECT * FROM files WHERE {w} AND downloads > 0 ORDER BY downloads DESC, uploaded DESC LIMIT 1"))?;
        s.biggest = one(format!("SELECT * FROM files WHERE {w} ORDER BY size DESC LIMIT 1"))?;
        s.random = db
            .prepare(&format!("SELECT * FROM files WHERE {w} ORDER BY TRIM(description) <> '' DESC, RANDOM() LIMIT 20"))?
            .query_map(p.clone(), from_row)?
            .collect::<Result<_, _>>()?;
        // The top uploader is a caller: an archive's files (uploader
        // "textfiles.com", say) don't make it the kingpin. The users
        // table shares the database; a bare file base has none.
        let callers = if db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'users'", [], |r| r.get::<_, u32>(0))? > 0 {
            " AND uploader COLLATE NOCASE IN (SELECT handle FROM users)"
        } else {
            ""
        };
        s.top_uploader = db
            .query_row(&format!("SELECT uploader, COUNT(*) FROM files WHERE {w} AND uploader <> ''{callers} GROUP BY uploader COLLATE NOCASE ORDER BY 2 DESC LIMIT 1"), p.clone(), |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((who, _)) = &s.top_uploader {
            s.kingpin_latest = db
                .query_row(
                    &format!("SELECT * FROM files WHERE {w} AND uploader = ? COLLATE NOCASE ORDER BY uploaded DESC, id DESC LIMIT 1"),
                    rusqlite::params_from_iter(areas.iter().map(|a| a as &dyn rusqlite::ToSql).chain([who as &dyn rusqlite::ToSql])),
                    from_row,
                )
                .optional()?;
        }
        if let Some(d) = since {
            s.since = db.query_row(&format!("SELECT COUNT(*) FROM files WHERE {w} AND uploaded >= ?"), rusqlite::params_from_iter(areas.iter().map(|a| a as &dyn rusqlite::ToSql).chain([&d as &dyn rusqlite::ToSql])), |r| r.get(0))?;
        } else {
            s.since = s.files;
        }
        Ok(s)
    }
}

/// See [`FileBase::stats`].
#[derive(Clone, Debug, Default)]
pub struct FileStats {
    /// How many areas were counted.
    pub areas: u32,
    pub files: u32,
    pub bytes: u64,
    /// Uploads since the date asked about.
    pub since: u32,
    pub newest: Option<FileRec>,
    /// The most downloaded.
    pub popular: Option<FileRec>,
    pub biggest: Option<FileRec>,
    /// A few at random, those with descriptions first.
    pub random: Vec<FileRec>,
    /// Handle and how many files.
    pub top_uploader: Option<(String, u32)>,
    /// The top uploader's latest.
    pub kingpin_latest: Option<FileRec>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(area: &str, name: &str) -> FileRec {
        FileRec {
            area: area.into(),
            num: 0,
            name: name.into(),
            size: 1234,
            date: None,
            uploader: "SysOp".into(),
            uploaded: NaiveDate::from_ymd_opt(2026, 10, 17),
            downloads: 0,
            points: 0,
            valid: true,
            description: vec!["A file".into(), "second line".into()],
        }
    }

    #[test]
    fn add_list_find_update() {
        let fb = FileBase::open_memory().unwrap();
        let mut a = rec("UTILS", "PKZ204G.EXE");
        let mut b = rec("UTILS", "IQ26.ZIP");
        fb.add(&mut a).unwrap();
        fb.add(&mut b).unwrap();
        assert_eq!((a.num, b.num), (1, 2));
        assert_eq!(fb.count("utils").unwrap(), 2);
        let mut got = fb.find("UTILS", "iq26.zip").unwrap().unwrap();
        assert_eq!(got.description, vec!["A file", "second line"]);
        got.downloads = 5;
        fb.update(&got).unwrap();
        assert_eq!(fb.get("UTILS", 2).unwrap().unwrap().downloads, 5);
        assert_eq!(fb.list("UTILS").unwrap().len(), 2);
    }

    fn names(fb: &FileBase, area: &str) -> Vec<String> {
        fb.list(area).unwrap().into_iter().map(|f| format!("{}:{}", f.num, f.name)).collect()
    }

    #[test]
    fn finds_by_words_best_first() {
        assert_eq!(Find::parse(r#" Tetris  -"demo  disk" *.ZIP "#), Find { want: vec!["tetris".into(), "*.zip".into()], not: vec!["demo disk".into()] });
        let fb = FileBase::open_memory().unwrap();
        let add = |area: &str, name: &str, desc: &str| {
            let mut f = rec(area, name);
            f.description = vec![desc.into()];
            fb.add(&mut f).unwrap();
        };
        add("GAMES", "BLOCKS.ZIP", "A Tetris clone for EGA");
        add("GAMES", "TETRIS.ZIP", "Falling blocks");
        add("GAMES", "TETDEMO.ZIP", "Tetris demo disk");
        add("TEXT", "TETRIS.TXT", "How to play Tetris");
        add("HIDDEN", "TETRIS2.ZIP", "Tetris again");
        let areas = vec!["GAMES".to_string(), "TEXT".to_string()];
        let names = |q: &str| fb.find_words(&areas, &Find::parse(q), 10).unwrap().0.into_iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names("tetris"), ["TETRIS.TXT", "TETRIS.ZIP", "BLOCKS.ZIP", "TETDEMO.ZIP"]);
        assert_eq!(names("tetris *.zip -\"demo disk\""), ["TETRIS.ZIP", "BLOCKS.ZIP"]);
        assert_eq!(names("ega clone"), ["BLOCKS.ZIP"]);
        assert_eq!(names("tetris -games"), names("tetris"));
        assert!(names("nothing").is_empty());
        assert_eq!(fb.find_words(&areas, &Find::parse("tetris"), 1).unwrap().1, 4);
        assert!(Find::parse("dos utils").matches_text("UTILS: DOS utilities"));
        assert!(!Find::parse("dos -utilities").matches_text("UTILS: DOS utilities"));
        assert!(!Find::parse("*.zip").matches_text("zip files"));
    }

    #[test]
    fn areas_have_a_line_about_them() {
        let fb = FileBase::open_memory().unwrap();
        assert_eq!(fb.area_about("UTILS").unwrap(), "");
        fb.set_area_about("UTILS", " DOS utilities ").unwrap();
        assert_eq!(fb.area_about("utils").unwrap(), "DOS utilities");
        fb.rename_area("UTILS", "TOOLS").unwrap();
        assert_eq!(fb.area_about("TOOLS").unwrap(), "DOS utilities");
        fb.set_area_about("TOOLS", "").unwrap();
        assert_eq!(fb.area_about("TOOLS").unwrap(), "");
        fb.set_area_about("TOOLS", "x").unwrap();
        fb.delete_area("TOOLS").unwrap();
        assert_eq!(fb.area_about("TOOLS").unwrap(), "");
    }

    #[test]
    fn prune_forgets_missing_files() {
        let fb = FileBase::open_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("iqprune{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["A.ZIP", "GONE1.ZIP", "B.ZIP", "GONE2.ZIP", "C.ZIP"] {
            fb.add(&mut rec("UTILS", n)).unwrap();
            if !n.starts_with("GONE") {
                std::fs::write(dir.join(n), b"x").unwrap();
            }
        }
        assert_eq!(fb.prune_missing("UTILS", &dir).unwrap(), ["GONE1.ZIP", "GONE2.ZIP"]);
        assert_eq!(names(&fb, "UTILS"), ["1:A.ZIP", "2:B.ZIP", "3:C.ZIP"]);
        assert!(fb.prune_missing("UTILS", &dir).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_move_sort() {
        let fb = FileBase::open_memory().unwrap();
        for (n, size) in [("C.ZIP", 3), ("A.TXT", 9), ("B.ARJ", 1), ("D.ZIP", 5)] {
            let mut f = rec("UTILS", n);
            f.size = size;
            fb.add(&mut f).unwrap();
        }
        fb.sort_area("UTILS", SortBy::Name, false).unwrap();
        assert_eq!(names(&fb, "UTILS"), ["1:A.TXT", "2:B.ARJ", "3:C.ZIP", "4:D.ZIP"]);
        fb.sort_area("UTILS", SortBy::Size, true).unwrap();
        assert_eq!(names(&fb, "UTILS"), ["1:A.TXT", "2:D.ZIP", "3:C.ZIP", "4:B.ARJ"]);
        fb.remove("UTILS", 2).unwrap();
        assert_eq!(names(&fb, "UTILS"), ["1:A.TXT", "2:C.ZIP", "3:B.ARJ"]);
        assert_eq!(fb.move_to("UTILS", 1, "TEXT").unwrap(), 1);
        assert_eq!(names(&fb, "UTILS"), ["1:C.ZIP", "2:B.ARJ"]);
        fb.rename_area("TEXT", "DOCS").unwrap();
        assert_eq!(names(&fb, "DOCS"), ["1:A.TXT"]);
        assert_eq!(fb.delete_area("UTILS").unwrap(), 2);
    }

    #[test]
    fn stats() {
        let fb = FileBase::open_memory().unwrap();
        let mut a = rec("UTILS", "OLD.ZIP");
        a.uploaded = NaiveDate::from_ymd_opt(2026, 1, 1);
        a.downloads = 9;
        let mut b = rec("UTILS", "NEW.ZIP");
        b.size = 99_999;
        b.uploader = "phrac".into();
        let mut c = rec("UTILS", "NEWER.ZIP");
        c.uploader = "phrac".into();
        let mut hidden = rec("SECRET", "HIDDEN.ZIP");
        hidden.size = 1 << 40;
        let mut bad = rec("UTILS", "BAD.ZIP");
        bad.valid = false;
        for f in [&mut a, &mut b, &mut c, &mut hidden, &mut bad] {
            fb.add(f).unwrap();
        }
        let s = fb.stats(&["utils".into()], NaiveDate::from_ymd_opt(2026, 10, 1)).unwrap();
        assert_eq!((s.files, s.bytes, s.since), (3, 1234 * 2 + 99_999, 2));
        assert_eq!(s.newest.unwrap().name, "NEWER.ZIP");
        assert_eq!(s.popular.unwrap().name, "OLD.ZIP");
        assert_eq!(s.biggest.unwrap().name, "NEW.ZIP");
        assert_eq!(s.top_uploader, Some(("phrac".into(), 2)));
        assert_eq!(s.kingpin_latest.unwrap().name, "NEWER.ZIP");
        // Only callers can be the kingpin: with a users table, an
        // uploader who isn't one doesn't count.
        fb.db.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("CREATE TABLE users (handle TEXT); INSERT INTO users VALUES ('sysop')").unwrap();
        let s = fb.stats(&["utils".into()], None).unwrap();
        assert_eq!(s.top_uploader, Some(("SysOp".into(), 1)));
        assert_eq!(s.kingpin_latest.unwrap().name, "OLD.ZIP");
        assert_eq!(s.random.len(), 3);
        assert!(s.random.iter().all(|f| f.area == "UTILS"));
        assert_eq!(fb.stats(&[], None).unwrap().files, 0);
    }

    #[test]
    fn search_counts_and_newest() {
        let fb = FileBase::open_memory().unwrap();
        for (area, name, desc, when) in [
            ("UTILS", "PKZ204G.EXE", "PKZip 2.04g", (2026, 1, 1)),
            ("UTILS", "IQ26.ZIP", "Iniquity 1.00a26 (the BBS)", (2026, 10, 17)),
            ("TEXT", "warfare", "How to Make a Landmine", (2001, 5, 5)),
            ("TEXT", "100%_true.txt", "Odd name", (2001, 5, 6)),
        ] {
            let mut f = rec(area, name);
            f.description = vec![desc.into()];
            f.uploaded = NaiveDate::from_ymd_opt(when.0, when.1, when.2);
            fb.add(&mut f).unwrap();
        }
        let both = ["utils".to_string(), "TEXT".to_string()];
        let names = |v: Vec<FileRec>| v.into_iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names(fb.search(&both, Some("*.zip"), None).unwrap()), ["IQ26.ZIP"]);
        assert_eq!(names(fb.search(&both, Some("WAR*"), None).unwrap()), ["warfare"]);
        assert_eq!(names(fb.search(&both, Some("IQ26"), None).unwrap()), ["IQ26.ZIP"]);
        assert_eq!(names(fb.search(&both, Some("100%_*"), None).unwrap()), ["100%_true.txt"]);
        // As the lister's wildcard has it, a bare name is a whole name.
        assert!(fb.search(&both, Some("PKZ"), None).unwrap().is_empty());
        assert_eq!(names(fb.search(&both, Some("PKZ204G"), None).unwrap()), ["PKZ204G.EXE"]);
        assert_eq!(names(fb.search(&both, None, Some("the bbs")).unwrap()), ["IQ26.ZIP"]);
        assert_eq!(names(fb.search(&both, None, Some("100%")).unwrap()), Vec::<String>::new());
        assert_eq!(names(fb.search(&both[..1], None, Some("landmine")).unwrap()), Vec::<String>::new());
        assert_eq!(names(fb.search(&both[1..], None, Some("landmine")).unwrap()), ["warfare"]);
        assert!(fb.search(&[], Some("*"), None).unwrap().is_empty());
        let since = fb.new_counts(NaiveDate::from_ymd_opt(2026, 1, 1)).unwrap();
        assert_eq!((since.get("utils"), since.get("text")), (Some(&2), None));
        assert_eq!(fb.new_counts(None).unwrap().get("text"), Some(&2));
        assert_eq!(fb.count_and_newest("TEXT").unwrap(), (2, NaiveDate::from_ymd_opt(2001, 5, 6)));
        assert_eq!(fb.count_and_newest("NONE").unwrap(), (0, None));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*.ZIP", "iq26.zip"));
        assert!(wildcard("IQ??.*", "IQ26.ZIP"));
        assert!(wildcard("IQ26", "IQ26.ZIP"));
        assert!(wildcard("*", "anything.txt"));
        assert!(!wildcard("*.ARJ", "IQ26.ZIP"));
        assert!(!wildcard("IQ2", "IQ26.ZIP"));
    }
}
