//! File records (SQLite), replacing the per-area .DIR files and
//! FILEDESC.DAT. The files themselves live in each area's directory.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

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

pub struct FileBase {
    db: Mutex<Connection>,
}

impl FileBase {
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

    pub fn count(&self, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT COUNT(*) FROM files WHERE area = ?1", [area], |r| r.get(0))?)
    }

    /// Every file in an area, in order.
    pub fn list(&self, area: &str) -> anyhow::Result<Vec<FileRec>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM files WHERE area = ?1 ORDER BY num")?;
        let rows = stmt.query_map([area], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn get(&self, area: &str, num: u32) -> anyhow::Result<Option<FileRec>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT * FROM files WHERE area = ?1 AND num = ?2", params![area, num], from_row).optional()?)
    }

    /// A file by name in an area (exact, case-insensitive).
    pub fn find(&self, area: &str, name: &str) -> anyhow::Result<Option<FileRec>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row(
                "SELECT * FROM files WHERE area = ?1 AND name = ?2 COLLATE NOCASE ORDER BY num LIMIT 1",
                params![area, name],
                from_row,
            )
            .optional()?)
    }

    pub fn add(&self, f: &mut FileRec) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
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

    pub fn update(&self, f: &FileRec) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
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
