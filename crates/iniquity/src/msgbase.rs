//! Message storage (SQLite), replacing the per-area .HDR/.DAT/.SCN files.
//!
//! Messages are numbered 1.. within their area, as in the original, and
//! keep their number when deleted (deletion is a flag). Each user's place
//! in an area is the highest message number they have read.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, Row, params};

/// One end of a message (`tMsgInfoRec`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Party {
    /// 0 if not a user here ("All", or an unknown name).
    pub user_id: u32,
    pub alias: String,
    pub real_name: String,
    /// The name shown.
    pub name: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub area: String,
    pub num: u32,
    pub from: Party,
    pub to: Party,
    pub subject: String,
    pub posted: NaiveDateTime,
    pub deleted: bool,
    pub private: bool,
    pub anonymous: bool,
    pub replies: u32,
    /// Line (from 1) where the auto-signature starts; 0 if none.
    pub sig_pos: u32,
    /// Email tag (`msgTag`): set from the writer's `E/` tag.
    pub tag: u16,
    pub lines: Vec<String>,
}

impl Message {
    /// Lines without the auto-signature (what gets edited or quoted).
    pub fn body(&self) -> &[String] {
        match self.sig_pos {
            0 => &self.lines,
            n => &self.lines[..(n as usize - 1).min(self.lines.len())],
        }
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY,
    area       TEXT NOT NULL COLLATE NOCASE,
    num        INTEGER NOT NULL,
    from_id    INTEGER NOT NULL,
    from_alias TEXT NOT NULL,
    from_real  TEXT NOT NULL,
    from_name  TEXT NOT NULL,
    from_note  TEXT NOT NULL,
    to_id      INTEGER NOT NULL,
    to_alias   TEXT NOT NULL,
    to_real    TEXT NOT NULL,
    to_name    TEXT NOT NULL,
    to_note    TEXT NOT NULL,
    subject    TEXT NOT NULL,
    posted     TEXT NOT NULL,
    deleted    INTEGER NOT NULL,
    private    INTEGER NOT NULL,
    anonymous  INTEGER NOT NULL,
    replies    INTEGER NOT NULL,
    sig_pos    INTEGER NOT NULL,
    body       TEXT NOT NULL,
    tag        INTEGER NOT NULL DEFAULT 0,
    UNIQUE (area, num)
);
CREATE TABLE IF NOT EXISTS lastread (
    user_id INTEGER NOT NULL,
    area    TEXT NOT NULL COLLATE NOCASE,
    num     INTEGER NOT NULL,
    PRIMARY KEY (user_id, area)
);
";

fn party(r: &Row, p: &str) -> rusqlite::Result<Party> {
    Ok(Party {
        user_id: r.get(format!("{p}_id").as_str())?,
        alias: r.get(format!("{p}_alias").as_str())?,
        real_name: r.get(format!("{p}_real").as_str())?,
        name: r.get(format!("{p}_name").as_str())?,
        note: r.get(format!("{p}_note").as_str())?,
    })
}

fn from_row(r: &Row) -> rusqlite::Result<Message> {
    let body: String = r.get("body")?;
    Ok(Message {
        area: r.get("area")?,
        num: r.get("num")?,
        from: party(r, "from")?,
        to: party(r, "to")?,
        subject: r.get("subject")?,
        posted: r.get("posted")?,
        deleted: r.get("deleted")?,
        private: r.get("private")?,
        anonymous: r.get("anonymous")?,
        replies: r.get("replies")?,
        sig_pos: r.get("sig_pos")?,
        tag: r.get("tag")?,
        lines: if body.is_empty() { Vec::new() } else { body.split('\n').map(String::from).collect() },
    })
}

pub struct MsgBase {
    db: Mutex<Connection>,
}

impl MsgBase {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        db.execute_batch(SCHEMA)?;
        // Databases from before email tags.
        let has_tag: bool = db.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('messages') WHERE name = 'tag'", [], |r| r.get(0))?;
        if !has_tag {
            db.execute("ALTER TABLE messages ADD COLUMN tag INTEGER NOT NULL DEFAULT 0", [])?;
        }
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// Highest message number in an area (`mArea^.Msgs`).
    pub fn count(&self, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT COALESCE(MAX(num), 0) FROM messages WHERE area = ?1", [area], |r| r.get(0))?)
    }

    pub fn get(&self, area: &str, num: u32) -> anyhow::Result<Option<Message>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT * FROM messages WHERE area = ?1 AND num = ?2", params![area, num], from_row)
            .optional()?)
    }

    /// Messages `from..=to` in an area, by number.
    pub fn range(&self, area: &str, from: u32, to: u32) -> anyhow::Result<Vec<Message>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT * FROM messages WHERE area = ?1 AND num BETWEEN ?2 AND ?3 ORDER BY num")?;
        let rows = stmt.query_map(params![area, from, to], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Add a message, numbering it after the area's last (or keeping
    /// `m.num` if set, for imports).
    pub fn add(&self, m: &mut Message) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        if m.num == 0 {
            m.num = db.query_row("SELECT COALESCE(MAX(num), 0) + 1 FROM messages WHERE area = ?1", [&m.area], |r| r.get(0))?;
        }
        db.execute(
            "INSERT INTO messages (area, num, from_id, from_alias, from_real, from_name, from_note,
                to_id, to_alias, to_real, to_name, to_note, subject, posted, deleted, private,
                anonymous, replies, sig_pos, body, tag)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                m.area, m.num, m.from.user_id, m.from.alias, m.from.real_name, m.from.name, m.from.note,
                m.to.user_id, m.to.alias, m.to.real_name, m.to.name, m.to.note, m.subject, m.posted,
                m.deleted, m.private, m.anonymous, m.replies, m.sig_pos, m.lines.join("\n"), m.tag
            ],
        )?;
        Ok(())
    }

    /// Save changes to an existing message (flags, replies, text).
    pub fn update(&self, m: &Message) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE messages SET from_name = ?3, subject = ?4, deleted = ?5, private = ?6,
                anonymous = ?7, replies = ?8, sig_pos = ?9, body = ?10, posted = ?11
             WHERE area = ?1 AND num = ?2",
            params![
                m.area, m.num, m.from.name, m.subject, m.deleted, m.private, m.anonymous, m.replies,
                m.sig_pos, m.lines.join("\n"), m.posted
            ],
        )?;
        Ok(())
    }

    /// An area's storage key changed: move its messages and lastreads.
    pub fn rename_area(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute("UPDATE messages SET area = ?2 WHERE area = ?1", params![from, to])?;
        tx.execute("UPDATE lastread SET area = ?2 WHERE area = ?1", params![from, to])?;
        tx.commit()?;
        Ok(())
    }

    /// Forget an area's messages and lastreads.
    pub fn delete_area(&self, area: &str) -> anyhow::Result<u32> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let n = tx.execute("DELETE FROM messages WHERE area = ?1", [area])?;
        tx.execute("DELETE FROM lastread WHERE area = ?1", [area])?;
        tx.commit()?;
        Ok(n as u32)
    }

    /// `maPackArea`: drop deleted messages, and the oldest beyond `max`
    /// (0: no limit), then number the rest from 1. Each user's lastread
    /// moves with the messages they had read. Returns how many went.
    pub fn pack(&self, area: &str, max: u32) -> anyhow::Result<u32> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let rows: Vec<(i64, u32, bool)> = {
            let mut stmt = tx.prepare("SELECT id, num, deleted FROM messages WHERE area = ?1 ORDER BY num")?;
            stmt.query_map([area], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
        };
        let mut keep: Vec<(i64, u32)> = rows.iter().filter(|r| !r.2).map(|r| (r.0, r.1)).collect();
        if max > 0 && keep.len() > max as usize {
            keep.drain(..keep.len() - max as usize);
        }
        let gone = (rows.len() - keep.len()) as u32;
        if gone == 0 && keep.iter().enumerate().all(|(i, k)| k.1 == i as u32 + 1) {
            return Ok(0);
        }
        let kept_nums: Vec<u32> = keep.iter().map(|k| k.1).collect();
        for (id, _, _) in rows.iter().filter(|r| !keep.iter().any(|k| k.0 == r.0)) {
            tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
        }
        tx.execute("UPDATE messages SET num = -num WHERE area = ?1", [area])?;
        for (i, (id, _)) in keep.iter().enumerate() {
            tx.execute("UPDATE messages SET num = ?2 WHERE id = ?1", params![id, i as u32 + 1])?;
        }
        let reads: Vec<(u32, u32)> = {
            let mut stmt = tx.prepare("SELECT user_id, num FROM lastread WHERE area = ?1")?;
            stmt.query_map([area], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        for (user, last) in reads {
            let now = kept_nums.iter().filter(|&&n| n <= last).count() as u32;
            tx.execute("UPDATE lastread SET num = ?3 WHERE user_id = ?1 AND area = ?2", params![user, area, now])?;
        }
        tx.commit()?;
        Ok(gone)
    }

    /// Highest message read in an area by a user.
    pub fn lastread(&self, user: u32, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT num FROM lastread WHERE user_id = ?1 AND area = ?2", params![user, area], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    pub fn set_lastread(&self, user: u32, area: &str, num: u32) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO lastread (user_id, area, num) VALUES (?1, ?2, ?3)
             ON CONFLICT (user_id, area) DO UPDATE SET num = excluded.num",
            params![user, area, num],
        )?;
        Ok(())
    }

    /// Number of the last message posted before `when` (for "set newscan
    /// date").
    pub fn last_before(&self, area: &str, when: NaiveDateTime) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row(
            "SELECT COALESCE(MAX(num), 0) FROM messages WHERE area = ?1 AND posted < ?2",
            params![area, when],
            |r| r.get(0),
        )?)
    }

    /// Messages addressed to a user (by alias or real name), by number.
    pub fn mail_for(&self, area: &str, alias: &str, real_name: &str, deleted_too: bool) -> anyhow::Result<Vec<Message>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT * FROM messages WHERE area = ?1
               AND (to_alias = ?2 COLLATE NOCASE OR to_real = ?3 COLLATE NOCASE)
               AND (?4 OR NOT deleted)
             ORDER BY num",
        )?;
        let rows = stmt.query_map(params![area, alias, real_name, deleted_too], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Every message in an area, by number.
    pub fn all(&self, area: &str) -> anyhow::Result<Vec<Message>> {
        self.range(area, 1, u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(area: &str, to: &str, text: &str) -> Message {
        Message {
            area: area.into(),
            num: 0,
            from: Party { user_id: 1, alias: "SysOp".into(), real_name: "System Operator".into(), name: "SysOp".into(), note: String::new() },
            to: Party { name: to.into(), alias: to.into(), real_name: to.into(), ..Party::default() },
            subject: "Hi".into(),
            posted: chrono::NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(12, 0, 0).unwrap(),
            deleted: false,
            private: false,
            anonymous: false,
            replies: 0,
            sig_pos: 0,
            tag: 0,
            lines: text.split('\n').map(String::from).collect(),
        }
    }

    #[test]
    fn tags_are_kept_and_old_databases_gain_them() {
        let mb = MsgBase::open_memory().unwrap();
        let mut m = msg("EMAIL", "phrac", "tagged");
        m.tag = 5;
        mb.add(&mut m).unwrap();
        assert_eq!(mb.get("EMAIL", 1).unwrap().unwrap().tag, 5);
        assert_eq!(mb.mail_for("EMAIL", "phrac", "", false).unwrap()[0].tag, 5);

        let path = std::env::temp_dir().join(format!("iqtags{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(&SCHEMA.replace("    tag        INTEGER NOT NULL DEFAULT 0,\n", "")).unwrap();
            db.execute("INSERT INTO messages (area, num, from_id, from_alias, from_real, from_name, from_note, to_id, to_alias, to_real, to_name, to_note, subject, posted, deleted, private, anonymous, replies, sig_pos, body) VALUES ('EMAIL', 1, 0, '', '', '', '', 0, 'x', '', 'x', '', 's', '2026-01-01T00:00:00', 0, 1, 0, 0, 0, 'old')", []).unwrap();
        }
        let mb = MsgBase::open(&path).unwrap();
        assert_eq!(mb.get("EMAIL", 1).unwrap().unwrap().tag, 0);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn packing() {
        let mb = MsgBase::open_memory().unwrap();
        for i in 1..=6 {
            let mut m = msg("GEN", "All", &format!("#{i}"));
            m.deleted = i == 2 || i == 5;
            mb.add(&mut m).unwrap();
        }
        mb.set_lastread(7, "GEN", 4).unwrap();
        mb.set_lastread(8, "GEN", 6).unwrap();
        // 2 and 5 are deleted; a max of 3 drops #1 as well.
        assert_eq!(mb.pack("GEN", 3).unwrap(), 3);
        let left: Vec<(u32, String)> = mb.all("GEN").unwrap().into_iter().map(|m| (m.num, m.lines[0].clone())).collect();
        assert_eq!(left, [(1, "#3".to_string()), (2, "#4".into()), (3, "#6".into())]);
        assert_eq!(mb.lastread(7, "GEN").unwrap(), 2, "had read up to #4");
        assert_eq!(mb.lastread(8, "GEN").unwrap(), 3);
        assert_eq!(mb.pack("GEN", 3).unwrap(), 0, "nothing more to do");
        mb.rename_area("GEN", "TALK").unwrap();
        assert_eq!(mb.lastread(8, "TALK").unwrap(), 3);
        assert_eq!(mb.delete_area("TALK").unwrap(), 3);
    }

    #[test]
    fn numbering_reading_updating() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("GENERAL", "All", "one\n\ntwo");
        let mut b = msg("GENERAL", "Fiend", "three");
        let mut c = msg("OTHER", "All", "x");
        mb.add(&mut a).unwrap();
        mb.add(&mut b).unwrap();
        mb.add(&mut c).unwrap();
        assert_eq!((a.num, b.num, c.num), (1, 2, 1));
        assert_eq!(mb.count("general").unwrap(), 2);
        let got = mb.get("GENERAL", 1).unwrap().unwrap();
        assert_eq!(got.lines, vec!["one", "", "two"]);

        let mut got = mb.get("GENERAL", 2).unwrap().unwrap();
        got.deleted = true;
        got.replies = 3;
        mb.update(&got).unwrap();
        let got = mb.get("GENERAL", 2).unwrap().unwrap();
        assert!(got.deleted && got.replies == 3);
        assert_eq!(mb.mail_for("GENERAL", "fiend", "", false).unwrap().len(), 0);
        assert_eq!(mb.mail_for("GENERAL", "fiend", "", true).unwrap().len(), 1);
    }

    #[test]
    fn lastread_pointers() {
        let mb = MsgBase::open_memory().unwrap();
        assert_eq!(mb.lastread(1, "GENERAL").unwrap(), 0);
        mb.set_lastread(1, "GENERAL", 5).unwrap();
        mb.set_lastread(1, "GENERAL", 7).unwrap();
        assert_eq!(mb.lastread(1, "general").unwrap(), 7);
        assert_eq!(mb.lastread(2, "GENERAL").unwrap(), 0);
    }

    #[test]
    fn body_excludes_signature() {
        let mut m = msg("A", "All", "hello\n\n-- sig");
        m.sig_pos = 2;
        assert_eq!(m.body(), ["hello"]);
    }
}
