//! Message storage (SQLite), replacing the per-area .HDR/.DAT/.SCN files.
//!
//! Messages are numbered 1.. within their area, as in the original, and
//! keep their number when deleted (deletion is a flag). Each user's place
//! in an area is the highest message number they have read.

use std::path::Path;
use std::sync::Mutex;

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
    /// Row id: stable across packing, which renumbers `num`.
    pub id: i64,
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
    /// Row id of the message this one answers (new: threads); 0 if none.
    pub reply_to: i64,
    pub lines: Vec<String>,
}

/// A file attached to a message (`tAttachRec`).
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub desc: String,
    pub size: u64,
    pub uploaded: chrono::NaiveDate,
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
    reply_to   INTEGER NOT NULL DEFAULT 0,
    UNIQUE (area, num)
);
CREATE TABLE IF NOT EXISTS attachments (
    msg_id   INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    desc     TEXT NOT NULL,
    size     INTEGER NOT NULL,
    uploaded TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS seen (
    user_id INTEGER NOT NULL,
    msg_id  INTEGER NOT NULL,
    PRIMARY KEY (user_id, msg_id)
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
        id: r.get("id")?,
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
        reply_to: r.get("reply_to")?,
        lines: if body.is_empty() { Vec::new() } else { body.split('\n').map(String::from).collect() },
    })
}

pub struct MsgBase {
    db: Mutex<Connection>,
}

impl MsgBase {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = crate::sysdata::open_db(path)?;
        db.execute_batch(SCHEMA)?;
        // Databases from before email tags.
        let has_tag: bool = db.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('messages') WHERE name = 'tag'", [], |r| r.get(0))?;
        if !has_tag {
            db.execute("ALTER TABLE messages ADD COLUMN tag INTEGER NOT NULL DEFAULT 0", [])?;
        }
        // And from before threads.
        let has_reply_to: bool = db.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('messages') WHERE name = 'reply_to'", [], |r| r.get(0))?;
        if !has_reply_to {
            db.execute("ALTER TABLE messages ADD COLUMN reply_to INTEGER NOT NULL DEFAULT 0", [])?;
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
                anonymous, replies, sig_pos, body, tag, reply_to)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                m.area, m.num, m.from.user_id, m.from.alias, m.from.real_name, m.from.name, m.from.note,
                m.to.user_id, m.to.alias, m.to.real_name, m.to.name, m.to.note, m.subject, m.posted,
                m.deleted, m.private, m.anonymous, m.replies, m.sig_pos, m.lines.join("\n"), m.tag, m.reply_to
            ],
        )?;
        m.id = db.last_insert_rowid();
        Ok(())
    }

    /// A message by row id, wherever it is.
    pub fn by_id(&self, id: i64) -> anyhow::Result<Option<Message>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT * FROM messages WHERE id = ?1", [id], from_row).optional()?)
    }

    // ---- read markers (new) ---------------------------------------------
    //
    // Threads are read out of order, so besides `lastread` (everything up
    // to it is read) each user has the messages read beyond it.

    /// Ids of an area's messages the user has read past their lastread.
    pub fn seen(&self, user: u32, area: &str) -> anyhow::Result<std::collections::HashSet<i64>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT s.msg_id FROM seen s JOIN messages m ON m.id = s.msg_id WHERE s.user_id = ?1 AND m.area = ?2")?;
        let ids = stmt.query_map(params![user, area], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(ids)
    }

    pub fn mark_seen(&self, user: u32, msg_id: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("INSERT OR IGNORE INTO seen (user_id, msg_id) VALUES (?1, ?2)", params![user, msg_id])?;
        Ok(())
    }

    /// Lastread moved up to `num`: the markers at or below it are done with.
    pub fn forget_seen(&self, user: u32, area: &str, num: u32) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM seen WHERE user_id = ?1 AND msg_id IN (SELECT id FROM messages WHERE area = ?2 AND num <= ?3)",
            params![user, area, num],
        )?;
        Ok(())
    }

    // ---- attachments ----------------------------------------------------

    pub fn attachment(&self, msg_id: i64) -> anyhow::Result<Option<Attachment>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT name, desc, size, uploaded FROM attachments WHERE msg_id = ?1", [msg_id], |r| {
                Ok(Attachment { name: r.get(0)?, desc: r.get(1)?, size: r.get::<_, i64>(2)?.max(0) as u64, uploaded: r.get(3)? })
            })
            .optional()?)
    }

    pub fn set_attachment(&self, msg_id: i64, a: &Attachment) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO attachments (msg_id, name, desc, size, uploaded) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg_id, a.name, a.desc, a.size as i64, a.uploaded],
        )?;
        Ok(())
    }

    pub fn remove_attachment(&self, msg_id: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM attachments WHERE msg_id = ?1", [msg_id])?;
        Ok(())
    }

    /// Forget attachments whose message no longer exists; returns their
    /// (message id, file name) so the files can go too.
    pub fn orphan_attachments(&self) -> anyhow::Result<Vec<(i64, String)>> {
        let db = self.db.lock().unwrap();
        let gone: Vec<(i64, String)> = {
            let mut stmt = db.prepare("SELECT msg_id, name FROM attachments WHERE msg_id NOT IN (SELECT id FROM messages)")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        db.execute("DELETE FROM attachments WHERE msg_id NOT IN (SELECT id FROM messages)", [])?;
        Ok(gone)
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
        tx.execute("DELETE FROM seen WHERE msg_id IN (SELECT id FROM messages WHERE area = ?1)", [area])?;
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
            tx.execute("DELETE FROM seen WHERE msg_id = ?1", [id])?;
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

    /// New: what the public messages (not private, not deleted) in `areas`
    /// add up to, for the message menu's codes; `since` is where "loudest"
    /// starts counting.
    pub fn stats(&self, areas: &[String], since: NaiveDateTime) -> anyhow::Result<MsgStats> {
        let mut s = MsgStats::default();
        if areas.is_empty() {
            return Ok(s);
        }
        let db = self.db.lock().unwrap();
        let w = format!("NOT deleted AND NOT private AND area IN ({})", vec!["?"; areas.len()].join(","));
        let p = rusqlite::params_from_iter(areas);
        s.total = db.query_row(&format!("SELECT COUNT(*) FROM messages WHERE {w}"), p.clone(), |r| r.get(0))?;
        let one = |sql: String| db.query_row(&sql, p.clone(), from_row).optional();
        s.newest = one(format!("SELECT * FROM messages WHERE {w} ORDER BY posted DESC, id DESC LIMIT 1"))?;
        s.hot = one(format!("SELECT * FROM messages WHERE {w} AND replies > 0 ORDER BY replies DESC, posted DESC LIMIT 1"))?;
        s.random = db.prepare(&format!("SELECT * FROM messages WHERE {w} ORDER BY RANDOM() LIMIT 20"))?.query_map(p.clone(), from_row)?.collect::<Result<_, _>>()?;
        let with_since = || rusqlite::params_from_iter(areas.iter().map(|a| a as &dyn rusqlite::ToSql).chain([&since as &dyn rusqlite::ToSql]));
        s.loudest = db
            .query_row(&format!("SELECT area, COUNT(*) FROM messages WHERE {w} AND posted >= ? GROUP BY area ORDER BY 2 DESC, MAX(posted) DESC LIMIT 1"), with_since(), |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        s.mouth = db
            .query_row(
                &format!("SELECT from_name, COUNT(*) FROM messages WHERE {w} AND NOT anonymous AND from_name <> '' GROUP BY from_name COLLATE NOCASE ORDER BY 2 DESC, MAX(posted) DESC LIMIT 1"),
                p.clone(),
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((who, _)) = &s.mouth {
            let q = rusqlite::params_from_iter(areas.iter().map(|a| a as &dyn rusqlite::ToSql).chain([who as &dyn rusqlite::ToSql]));
            s.mouth_latest = db
                .query_row(&format!("SELECT * FROM messages WHERE {w} AND NOT anonymous AND from_name = ? COLLATE NOCASE ORDER BY posted DESC, id DESC LIMIT 1"), q, from_row)
                .optional()?;
        }
        Ok(s)
    }
}

/// See [`MsgBase::stats`].
#[derive(Clone, Debug, Default)]
pub struct MsgStats {
    pub total: u32,
    pub newest: Option<Message>,
    /// The message with the most replies.
    pub hot: Option<Message>,
    /// The area with the most posts lately, and how many.
    pub loudest: Option<(String, u32)>,
    /// The top poster (not anonymously) and their posts, and their latest.
    pub mouth: Option<(String, u32)>,
    pub mouth_latest: Option<Message>,
    /// A few at random.
    pub random: Vec<Message>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(area: &str, to: &str, text: &str) -> Message {
        Message {
            id: 0,
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
            reply_to: 0,
            lines: text.split('\n').map(String::from).collect(),
        }
    }

    #[test]
    fn stats() {
        let mb = MsgBase::open_memory().unwrap();
        let at = |d: u32| chrono::NaiveDate::from_ymd_opt(2026, 10, d).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut a = msg("GENERAL", "All", "hello there");
        a.posted = at(1);
        a.replies = 2;
        let mut b = msg("GENERAL", "All", "newest");
        b.posted = at(19);
        b.from.name = "phrac".into();
        let mut c = msg("GENERAL", "All", "also new");
        c.posted = at(18);
        c.from.name = "phrac".into();
        let mut d = msg("ANNOUNCE", "All", "old news");
        d.posted = at(2);
        let mut hidden = msg("GENERAL", "SysOp", "psst");
        hidden.private = true;
        hidden.posted = at(20);
        let mut anon = msg("GENERAL", "All", "who am i");
        anon.anonymous = true;
        anon.from.name = "phrac".into();
        anon.posted = at(3);
        for m in [&mut a, &mut b, &mut c, &mut d, &mut hidden, &mut anon] {
            mb.add(m).unwrap();
        }
        let s = mb.stats(&["general".into(), "announce".into()], at(10)).unwrap();
        assert_eq!(s.total, 5);
        assert_eq!(s.newest.unwrap().lines, ["newest"]);
        assert_eq!(s.hot.unwrap().lines, ["hello there"]);
        assert_eq!(s.loudest, Some(("GENERAL".into(), 2)));
        assert_eq!(s.mouth, Some(("phrac".into(), 2)));
        assert_eq!(s.mouth_latest.unwrap().lines, ["newest"]);
        assert_eq!(s.random.len(), 5);
        assert!(s.random.iter().all(|m| !m.private));
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
    fn attachments_follow_their_message_through_packing() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("GEN", "All", "first");
        a.deleted = true;
        mb.add(&mut a).unwrap();
        let mut b = msg("GEN", "All", "second");
        mb.add(&mut b).unwrap();
        assert!(a.id > 0 && b.id > a.id);
        let file = Attachment { name: "X.ZIP".into(), desc: "x".into(), size: 5, uploaded: chrono::NaiveDate::from_ymd_opt(2026, 10, 17).unwrap() };
        mb.set_attachment(a.id, &file).unwrap();
        mb.set_attachment(b.id, &file).unwrap();
        mb.pack("GEN", 0).unwrap();
        let kept = mb.get("GEN", 1).unwrap().unwrap();
        assert_eq!((kept.id, kept.lines[0].as_str()), (b.id, "second"), "renumbered, same row");
        assert_eq!(mb.attachment(kept.id).unwrap(), Some(file.clone()));
        assert_eq!(mb.orphan_attachments().unwrap(), [(a.id, "X.ZIP".to_string())]);
        assert!(mb.orphan_attachments().unwrap().is_empty());
        mb.remove_attachment(kept.id).unwrap();
        assert_eq!(mb.attachment(kept.id).unwrap(), None);
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
