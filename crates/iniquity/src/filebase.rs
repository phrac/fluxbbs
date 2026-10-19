//! File records (SQLite), replacing the per-area .DIR files and
//! FILEDESC.DAT. The files themselves live in each area's directory.

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
        let db = crate::sysdata::open_db(path)?;
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

    /// Add several records in one transaction (a bulk load holds the
    /// lock once, not once a file).
    pub fn add_all(&self, recs: &mut [FileRec]) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
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

    /// Remove a file's record and close the gap in the numbering.
    pub fn remove(&self, area: &str, num: u32) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
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
        let db = self.db.lock().unwrap();
        db.execute("UPDATE files SET area = ?2 WHERE area = ?1", params![from, to])?;
        Ok(())
    }

    /// Forget every file record in an area.
    pub fn delete_area(&self, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.execute("DELETE FROM files WHERE area = ?1", [area])? as u32)
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
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute("UPDATE files SET num = -num WHERE area = ?1", [area])?;
        for (i, f) in files.iter().enumerate() {
            tx.execute("UPDATE files SET num = ?3 WHERE area = ?1 AND num = ?2", params![area, -(f.num as i64), i as u32 + 1])?;
        }
        tx.commit()?;
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

    fn names(fb: &FileBase, area: &str) -> Vec<String> {
        fb.list(area).unwrap().into_iter().map(|f| format!("{}:{}", f.num, f.name)).collect()
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
    fn wildcards() {
        assert!(wildcard("*.ZIP", "iq26.zip"));
        assert!(wildcard("IQ??.*", "IQ26.ZIP"));
        assert!(wildcard("IQ26", "IQ26.ZIP"));
        assert!(wildcard("*", "anything.txt"));
        assert!(!wildcard("*.ARJ", "IQ26.ZIP"));
        assert!(!wildcard("IQ2", "IQ26.ZIP"));
    }
}
