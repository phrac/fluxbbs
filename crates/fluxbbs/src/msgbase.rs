//! Message storage (SQLite), replacing the per-area .HDR/.DAT/.SCN files.
//!
//! Messages are numbered 1.. within their area, as in the original, and
//! keep their number when deleted (deletion is a flag). Each user's place
//! in an area is the highest message number they have read.

use std::path::Path;
use std::sync::Mutex;

use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::sysdata::{Step, migrate};

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
    /// Row id: never used again, even once the message is gone.
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

/// New: a post in the trash (deleted, not yet purged).
#[derive(Debug, Clone)]
pub struct Trashed {
    pub id: i64,
    pub area: String,
    pub num: u32,
    pub from: String,
    pub subject: String,
    /// When it was deleted (None if that wasn't kept).
    pub at: Option<NaiveDateTime>,
    pub by: String,
}

/// A push (new, after PTT's 推/噓/→): a one-line reaction under a message,
/// up (1), down (-1) or a comment (0). A caller has one vote on a
/// message; comments may repeat.
#[derive(Debug, Clone, PartialEq)]
pub struct Push {
    pub id: i64,
    pub msg_id: i64,
    pub user_id: u32,
    pub handle: String,
    pub kind: i8,
    pub text: String,
    pub at: NaiveDateTime,
}

/// New: a request board's request marked filled: who said so (its asker,
/// or a co-SysOp), when, whose answer filled it (`filler`; `reply_id` the
/// message, and `push_id` the comment under it if it was one; none if it
/// was filled some other way) and a note. `told`: its asker has
/// heard (they marked it themselves, or have seen it since).
#[derive(Debug, Clone, PartialEq)]
pub struct Filled {
    pub msg_id: i64,
    pub user_id: u32,
    pub handle: String,
    pub note: String,
    pub at: NaiveDateTime,
    pub told: bool,
    pub filler_id: u32,
    pub filler: String,
    pub reply_id: i64,
    pub push_id: i64,
}

/// Marking a request filled: by whom, the reply that filled it (if one
/// did) and a note.
pub struct FillMark<'a> {
    pub by_id: u32,
    pub by: &'a str,
    pub filler_id: u32,
    pub filler: &'a str,
    pub reply_id: i64,
    pub push_id: i64,
    pub note: &'a str,
}

const FILLED_COLS: &str = "msg_id, user_id, handle, note, at, told, filler_id, filler, reply_id, push_id";

fn filled_row(r: &Row) -> rusqlite::Result<Filled> {
    Ok(Filled {
        msg_id: r.get(0)?,
        user_id: r.get(1)?,
        handle: r.get(2)?,
        note: r.get(3)?,
        at: r.get(4)?,
        told: r.get(5)?,
        filler_id: r.get(6)?,
        filler: r.get(7)?,
        reply_id: r.get(8)?,
        push_id: r.get(9)?,
    })
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

/// Databases from before email tags, threads, and fillers; then what
/// came after.
const STEPS: &[Step] = &[
    Step::Column("messages", "tag", "INTEGER NOT NULL DEFAULT 0"),
    Step::Column("messages", "reply_to", "INTEGER NOT NULL DEFAULT 0"),
    Step::Column("filled", "filler_id", "INTEGER NOT NULL DEFAULT 0"),
    Step::Column("filled", "filler", "TEXT NOT NULL DEFAULT ''"),
    Step::Column("filled", "reply_id", "INTEGER NOT NULL DEFAULT 0"),
    Step::Column("filled", "push_id", "INTEGER NOT NULL DEFAULT 0"),
    // `stats` reads only this index, not the bodies.
    Step::Sql(
        "CREATE INDEX IF NOT EXISTS messages_reply_to ON messages(reply_to);
         CREATE INDEX IF NOT EXISTS messages_stats ON messages(area, deleted, private, anonymous, from_name, posted, replies);",
    ),
    // From before row ids were kept unique: start above every id
    // anything still names, so a new post can't take over an old
    // one's MSGID, mentions or attachment.
    Step::Sql(
        "INSERT OR IGNORE INTO ftn_state (key, value) SELECT 'last_msg_id', MAX(
            COALESCE((SELECT MAX(id) FROM messages), 0), COALESCE((SELECT MAX(msg_id) FROM ftn_msgs), 0),
            COALESCE((SELECT MAX(msg_id) FROM mentions), 0), COALESCE((SELECT MAX(msg_id) FROM seen), 0),
            COALESCE((SELECT MAX(msg_id) FROM attachments), 0))",
    ),
    // When a reply began waiting for its parent (they're given up a
    // while after they came, not after they were written).
    Step::Column("ftn_replies", "at", "TEXT NOT NULL DEFAULT ''"),
    // The trash: when a post was deleted (board time) and by whom; it
    // goes for good `keep_deleted_days` later.
    Step::Column("messages", "deleted_at", "TEXT NOT NULL DEFAULT ''"),
    Step::Column("messages", "deleted_by", "TEXT NOT NULL DEFAULT ''"),
];

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
    deleted_at TEXT NOT NULL DEFAULT '',
    deleted_by TEXT NOT NULL DEFAULT '',
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
CREATE TABLE IF NOT EXISTS ftn_msgs (
    msg_id INTEGER PRIMARY KEY,
    msgid  TEXT NOT NULL,
    area   TEXT NOT NULL COLLATE NOCASE,
    local  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ftn_msgs_msgid ON ftn_msgs(msgid);
CREATE TABLE IF NOT EXISTS ftn_replies (
    msg_id INTEGER PRIMARY KEY,
    area   TEXT NOT NULL COLLATE NOCASE,
    reply  TEXT NOT NULL,
    at     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS ftn_replies_reply ON ftn_replies(reply);
CREATE TABLE IF NOT EXISTS ftn_state (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS qwk_confs (
    user_id INTEGER NOT NULL,
    conf    INTEGER NOT NULL,
    area    TEXT NOT NULL,
    PRIMARY KEY (user_id, conf)
);
CREATE TABLE IF NOT EXISTS mentions (
    user_id INTEGER NOT NULL,
    msg_id  INTEGER NOT NULL,
    seen    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, msg_id)
);
CREATE TABLE IF NOT EXISTS pushes (
    id      INTEGER PRIMARY KEY,
    msg_id  INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    handle  TEXT NOT NULL,
    kind    INTEGER NOT NULL,
    text    TEXT NOT NULL,
    at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS pushes_msg ON pushes(msg_id);
CREATE INDEX IF NOT EXISTS pushes_at ON pushes(at);
CREATE TABLE IF NOT EXISTS pins (
    msg_id INTEGER PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS filled (
    msg_id  INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    handle  TEXT NOT NULL,
    note    TEXT NOT NULL,
    at      TEXT NOT NULL,
    told    INTEGER NOT NULL DEFAULT 0,
    filler_id INTEGER NOT NULL DEFAULT 0,
    filler  TEXT NOT NULL DEFAULT '',
    reply_id INTEGER NOT NULL DEFAULT 0,
    push_id INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS fills (
    msg_id    INTEGER NOT NULL,
    filler_id INTEGER NOT NULL,
    filler    TEXT NOT NULL,
    area      TEXT NOT NULL,
    subject   TEXT NOT NULL,
    points    INTEGER NOT NULL,
    at        TEXT NOT NULL,
    told      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (msg_id, filler_id)
);
CREATE INDEX IF NOT EXISTS fills_at ON fills(at);
CREATE TABLE IF NOT EXISTS retracted (
    msg_id  INTEGER PRIMARY KEY,
    subject TEXT NOT NULL,
    body    TEXT NOT NULL,
    sig_pos INTEGER NOT NULL,
    at      TEXT NOT NULL
);
";

/// Whether a reply (private: `?2`) counts on the parent row it updates:
/// a private reply in a public area mustn't show that it exists.
const COUNTS: &str = "(NOT ?2 OR private)";

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
    /// Bumped by whatever can change what's new to someone (a post, a
    /// read, a delete), so figures kept from before know to look again.
    changes: std::sync::atomic::AtomicU64,
    /// New: bumped by each post that lands, whoever wrote it (a caller,
    /// FTN, QWK), so a list on someone's screen can redraw.
    posted: std::sync::atomic::AtomicU64,
    /// New: each post as it lands, for `newposts` to tell its board's
    /// followers.
    news: tokio::sync::broadcast::Sender<Message>,
}

impl MsgBase {
    /// How many times what's new may have changed.
    pub fn changes(&self) -> u64 {
        self.changes.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn changed(&self) {
        self.changes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// How many posts have landed since the board started.
    pub fn posted(&self) -> u64 {
        self.posted.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Posts as they land, from now on.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Message> {
        self.news.subscribe()
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut db = crate::sysdata::open_db(path)?;
        migrate(&mut db, "msgbase", SCHEMA, STEPS)?;
        Ok(Self { db: Mutex::new(db), changes: Default::default(), posted: Default::default(), news: tokio::sync::broadcast::channel(64).0 })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let mut db = Connection::open_in_memory()?;
        migrate(&mut db, "msgbase", SCHEMA, STEPS)?;
        Ok(Self { db: Mutex::new(db), changes: Default::default(), posted: Default::default(), news: tokio::sync::broadcast::channel(64).0 })
    }

    /// Highest message number in an area (`mArea^.Msgs`).
    pub fn count(&self, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COALESCE(MAX(num), 0) FROM messages WHERE area = ?1", [area], |r| r.get(0))?)
    }

    pub fn get(&self, area: &str, num: u32) -> anyhow::Result<Option<Message>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db
            .query_row("SELECT * FROM messages WHERE area = ?1 AND num = ?2", params![area, num], from_row)
            .optional()?)
    }

    /// Messages `from..=to` in an area, by number.
    pub fn range(&self, area: &str, from: u32, to: u32) -> anyhow::Result<Vec<Message>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT * FROM messages WHERE area = ?1 AND num BETWEEN ?2 AND ?3 ORDER BY num")?;
        let rows = stmt.query_map(params![area, from, to], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Add a message, numbering it after the area's last (or keeping
    /// `m.num` if set, for imports).
    pub fn add(&self, m: &mut Message) -> anyhow::Result<()> {
        self.add_ftn(m, None)
    }

    /// `add`, recording the message's MSGID (or other FTN key) in the same
    /// transaction (`ftn_record`), so a crash can't leave an imported
    /// message looking like one written here.
    pub fn add_ftn(&self, m: &mut Message, ftn: Option<&str>) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        // Immediate: `fluxbbs ftn` beside the board writes here too, and
        // both mustn't pick the same number.
        let db = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if m.num == 0 {
            m.num = db.query_row("SELECT COALESCE(MAX(num), 0) + 1 FROM messages WHERE area = ?1", [&m.area], |r| r.get(0))?;
        }
        // Row ids are never used twice, even after the newest message is
        // purged: FTN's MSGIDs, mentions and read marks name rows.
        let id: i64 = db.query_row(
            "SELECT MAX(COALESCE((SELECT MAX(id) FROM messages), 0), COALESCE((SELECT value FROM ftn_state WHERE key = 'last_msg_id'), 0)) + 1",
            [],
            |r| r.get(0),
        )?;
        db.execute(
            "INSERT INTO messages (id, area, num, from_id, from_alias, from_real, from_name, from_note,
                to_id, to_alias, to_real, to_name, to_note, subject, posted, deleted, private,
                anonymous, replies, sig_pos, body, tag, reply_to)
             VALUES (?23, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                m.area, m.num, m.from.user_id, m.from.alias, m.from.real_name, m.from.name, m.from.note,
                m.to.user_id, m.to.alias, m.to.real_name, m.to.name, m.to.note, m.subject, m.posted,
                m.deleted, m.private, m.anonymous, m.replies, m.sig_pos, m.lines.join("\n"), m.tag, m.reply_to, id
            ],
        )?;
        db.execute("INSERT OR REPLACE INTO ftn_state (key, value) VALUES ('last_msg_id', ?1)", [id])?;
        // Replies are counted here, on the row, so a stale copy saved
        // elsewhere can't lose one.
        if m.reply_to != 0 && !m.deleted {
            db.execute(&format!("UPDATE messages SET replies = replies + 1 WHERE id = ?1 AND {COUNTS}"), params![m.reply_to, m.private])?;
        }
        if let Some(msgid) = ftn {
            db.execute("INSERT OR REPLACE INTO ftn_msgs (msg_id, msgid, area, local) VALUES (?1, ?2, ?3, 0)", params![id, msgid, m.area])?;
        }
        db.commit()?;
        m.id = id;
        self.changed();
        self.posted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Nobody listening (a tool, a test) is no error.
        let _ = self.news.send(m.clone());
        Ok(())
    }

    /// A message by row id, wherever it is.
    pub fn by_id(&self, id: i64) -> anyhow::Result<Option<Message>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT * FROM messages WHERE id = ?1", [id], from_row).optional()?)
    }

    // ---- read markers (new) ---------------------------------------------
    //
    // Threads are read out of order, so besides `lastread` (everything up
    // to it is read) each user has the messages read beyond it.

    /// Ids of an area's messages the user has read past their lastread.
    pub fn seen(&self, user: u32, area: &str) -> anyhow::Result<std::collections::HashSet<i64>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT s.msg_id FROM seen s JOIN messages m ON m.id = s.msg_id WHERE s.user_id = ?1 AND m.area = ?2")?;
        let ids = stmt.query_map(params![user, area], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(ids)
    }

    pub fn mark_seen(&self, user: u32, msg_id: i64) -> anyhow::Result<()> {
        self.changed();
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("INSERT OR IGNORE INTO seen (user_id, msg_id) VALUES (?1, ?2)", params![user, msg_id])?;
        Ok(())
    }

    /// Lastread moved up to `num`: the markers at or below it are done with.
    pub fn forget_seen(&self, user: u32, area: &str, num: u32) -> anyhow::Result<()> {
        self.changed();
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "DELETE FROM seen WHERE user_id = ?1 AND msg_id IN (SELECT id FROM messages WHERE area = ?2 AND num <= ?3)",
            params![user, area, num],
        )?;
        Ok(())
    }

    // ---- attachments ----------------------------------------------------

    pub fn attachment(&self, msg_id: i64) -> anyhow::Result<Option<Attachment>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db
            .query_row("SELECT name, desc, size, uploaded FROM attachments WHERE msg_id = ?1", [msg_id], |r| {
                Ok(Attachment { name: r.get(0)?, desc: r.get(1)?, size: r.get::<_, i64>(2)?.max(0) as u64, uploaded: r.get(3)? })
            })
            .optional()?)
    }

    pub fn set_attachment(&self, msg_id: i64, a: &Attachment) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "INSERT OR REPLACE INTO attachments (msg_id, name, desc, size, uploaded) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg_id, a.name, a.desc, a.size as i64, a.uploaded],
        )?;
        Ok(())
    }

    pub fn remove_attachment(&self, msg_id: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("DELETE FROM attachments WHERE msg_id = ?1", [msg_id])?;
        Ok(())
    }

    /// Forget attachments whose message no longer exists; returns their
    /// (message id, file name) so the files can go too.
    pub fn orphan_attachments(&self) -> anyhow::Result<Vec<(i64, String)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let gone: Vec<(i64, String)> = {
            let mut stmt = db.prepare("SELECT msg_id, name FROM attachments WHERE msg_id NOT IN (SELECT id FROM messages)")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        db.execute("DELETE FROM attachments WHERE msg_id NOT IN (SELECT id FROM messages)", [])?;
        Ok(gone)
    }

    /// Save changes to an existing message (flags, text), by row. The
    /// reply count is kept by `add`.
    /// An edit: the text, subject and signature only, so flags changed
    /// meanwhile (a delete on another node) stay.
    pub fn update_text(&self, m: &Message) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "UPDATE messages SET subject = ?2, sig_pos = ?3, body = ?4 WHERE id = ?1",
            params![m.id, m.subject, m.sig_pos, m.lines.join("\n")],
        )?;
        Ok(())
    }

    /// Where a message is (its number), if it's still there.
    pub fn num_of(&self, id: i64) -> anyhow::Result<Option<u32>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT num FROM messages WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    /// Move a lastread pointer to a message by row id (to its number now),
    /// only ever forward.
    pub fn set_lastread_id(&self, user: u32, area: &str, id: i64) -> anyhow::Result<()> {
        self.changed();
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "INSERT INTO lastread (user_id, area, num) SELECT ?1, ?2, num FROM messages WHERE id = ?3 AND area = ?2
             ON CONFLICT (user_id, area) DO UPDATE SET num = MAX(num, excluded.num)",
            params![user, area, id],
        )?;
        Ok(())
    }

    /// After a scan's packet is written: its MSGIDs recorded as sent and
    /// the areas' marks moved, together.
    pub fn ftn_sent(&self, sent: &[(i64, String, String)], marks: &[(String, i64)]) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for (id, msgid, area) in sent {
            tx.execute("INSERT OR REPLACE INTO ftn_msgs (msg_id, msgid, area, local) VALUES (?1, ?2, ?3, 1)", params![id, msgid, area])?;
        }
        for (key, last) in marks {
            tx.execute("INSERT OR REPLACE INTO ftn_state (key, value) VALUES (?1, ?2)", params![key, last])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn update(&self, m: &Message) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "UPDATE messages SET from_name = ?2, subject = ?3, deleted = ?4, private = ?5,
                anonymous = ?6, sig_pos = ?7, body = ?8, posted = ?9
             WHERE id = ?1",
            params![m.id, m.from.name, m.subject, m.deleted, m.private, m.anonymous, m.sig_pos, m.lines.join("\n"), m.posted],
        )?;
        Ok(())
    }

    /// The conference numbers (`conf`, area key) in the user's last QWK
    /// packet, so its replies find their areas after areas move.
    pub fn set_qwk_confs(&self, user: u32, confs: &[(u16, String)]) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM qwk_confs WHERE user_id = ?1", [user])?;
        for (conf, area) in confs {
            tx.execute("INSERT INTO qwk_confs (user_id, conf, area) VALUES (?1, ?2, ?3)", params![user, conf, area])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn qwk_conf(&self, user: u32, conf: u32) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT area FROM qwk_confs WHERE user_id = ?1 AND conf = ?2", params![user, conf], |r| r.get(0)).optional()?)
    }

    /// The newest `n` public posts (not private or deleted) in `areas`.
    pub fn newest_public(&self, areas: &[String], n: usize) -> anyhow::Result<Vec<Message>> {
        if areas.is_empty() || n == 0 {
            return Ok(Vec::new());
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let marks = vec!["?"; areas.len()].join(", ");
        let mut stmt = db.prepare(&format!(
            "SELECT * FROM messages WHERE area COLLATE NOCASE IN ({marks}) AND NOT private AND NOT deleted ORDER BY posted DESC, id DESC LIMIT {n}"
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(areas), from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Delete or undelete a message, leaving the rest of the row alone (a
    /// copy read earlier mustn't undo an edit made since). New: a deleted
    /// one is in the trash, with when and `by` whom, till it's purged.
    pub fn set_deleted(&self, id: i64, deleted: bool, by: &str) -> anyhow::Result<()> {
        self.changed();
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let row: Option<(i64, bool, bool)> =
            tx.query_row("SELECT reply_to, private, deleted FROM messages WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        if let Some((parent, private, was)) = row
            && was != deleted
        {
            let (at, by) = if deleted { (crate::clock::now().naive_local().to_string(), by) } else { (String::new(), "") };
            tx.execute("UPDATE messages SET deleted = ?2, deleted_at = ?3, deleted_by = ?4 WHERE id = ?1", params![id, deleted, at, by])?;
            if parent != 0 {
                let by = if deleted { "MAX(replies - 1, 0)" } else { "replies + 1" };
                tx.execute(&format!("UPDATE messages SET replies = {by} WHERE id = ?1 AND {COUNTS}"), params![parent, private])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// `Z` in the reader: a new date for a message.
    pub fn set_posted(&self, id: i64, posted: NaiveDateTime) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE messages SET posted = ?2 WHERE id = ?1", params![id, posted])?;
        Ok(())
    }

    /// An area's storage key changed: move its messages and lastreads.
    pub fn rename_area(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("UPDATE messages SET area = ?2 WHERE area = ?1", params![from, to])?;
        tx.execute("UPDATE lastread SET area = ?2 WHERE area = ?1", params![from, to])?;
        // FTN's records and scan mark go with it.
        tx.execute("UPDATE ftn_msgs SET area = ?2 WHERE area = ?1", params![from, to])?;
        tx.execute("UPDATE ftn_replies SET area = ?2 WHERE area = ?1", params![from, to])?;
        tx.execute("UPDATE qwk_confs SET area = ?2 WHERE area = ?1 COLLATE NOCASE", params![from, to])?;
        let (old, new) = (format!("sent:{}", from.to_ascii_uppercase()), format!("sent:{}", to.to_ascii_uppercase()));
        if old != new {
            tx.execute("DELETE FROM ftn_state WHERE key = ?1", [&new])?;
            tx.execute("UPDATE ftn_state SET key = ?2 WHERE key = ?1", params![old, new])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Forget an area's messages and lastreads. Its FTN records stay, as
    /// dupe history, should the echo be linked again.
    pub fn delete_area(&self, area: &str) -> anyhow::Result<u32> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for t in ["seen", "mentions", "pushes", "pins", "filled", "retracted"] {
            tx.execute(&format!("DELETE FROM {t} WHERE msg_id IN (SELECT id FROM messages WHERE area = ?1)"), [area])?;
        }
        let n = tx.execute("DELETE FROM messages WHERE area = ?1", [area])?;
        tx.execute("DELETE FROM lastread WHERE area = ?1", [area])?;
        tx.execute("DELETE FROM qwk_confs WHERE area = ?1 COLLATE NOCASE", [area])?;
        tx.execute("DELETE FROM ftn_replies WHERE area = ?1", [area])?;
        tx.execute("DELETE FROM ftn_state WHERE key = ?1", [format!("sent:{}", area.to_ascii_uppercase())])?;
        tx.commit()?;
        Ok(n as u32)
    }

    /// New: a message gone for good (deleted email: nobody moderates
    /// mail, and there's no undeleting it), with what hangs off it. Its
    /// attachment's file goes with `attach::remove_orphans`.
    pub fn purge(&self, id: i64) -> anyhow::Result<()> {
        self.changed();
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let row: Option<(i64, bool, bool)> =
            tx.query_row("SELECT reply_to, private, deleted FROM messages WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let Some((parent, private, deleted)) = row else { return Ok(()) };
        if parent != 0 && !deleted {
            tx.execute(&format!("UPDATE messages SET replies = MAX(replies - 1, 0) WHERE id = ?1 AND {COUNTS}"), params![parent, private])?;
        }
        tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
        for t in ["pins", "filled", "retracted", "seen", "mentions", "pushes"] {
            tx.execute(&format!("DELETE FROM {t} WHERE msg_id = ?1"), [id])?;
        }
        tx.execute("UPDATE messages SET reply_to = 0 WHERE reply_to = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// New: the trash, the latest deleted first.
    pub fn trash(&self) -> anyhow::Result<Vec<Trashed>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare(
            "SELECT id, area, num, from_name, subject, deleted_at, deleted_by FROM messages WHERE deleted ORDER BY deleted_at DESC, id DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let at: String = r.get(5)?;
                Ok(Trashed {
                    id: r.get(0)?,
                    area: r.get(1)?,
                    num: r.get(2)?,
                    from: r.get(3)?,
                    subject: r.get(4)?,
                    at: NaiveDateTime::parse_from_str(&at, "%Y-%m-%d %H:%M:%S%.f").ok(),
                    by: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// New: posts deleted with no date kept (before the trash) are dated
    /// `now`, so their days start from it.
    pub fn date_trash(&self, now: NaiveDateTime) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute("UPDATE messages SET deleted_at = ?1 WHERE deleted AND deleted_at = ''", [now.to_string()])? as u32)
    }

    /// New: empty the trash of what was deleted before `before`. Returns
    /// how many went.
    pub fn purge_trash(&self, before: NaiveDateTime) -> anyhow::Result<u32> {
        let ids: Vec<i64> = {
            let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            let mut stmt = db.prepare("SELECT id FROM messages WHERE deleted AND deleted_at != '' AND deleted_at < ?1")?;
            stmt.query_map([before.to_string()], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in &ids {
            self.purge(*id)?;
        }
        Ok(ids.len() as u32)
    }

    /// Purge an area's deleted messages (email earlier versions kept
    /// until a pack). Returns how many went.
    pub fn purge_deleted(&self, area: &str) -> anyhow::Result<u32> {
        let ids: Vec<i64> = {
            let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            let mut stmt = db.prepare("SELECT id FROM messages WHERE area = ?1 AND deleted")?;
            stmt.query_map([area], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in &ids {
            self.purge(*id)?;
        }
        Ok(ids.len() as u32)
    }

    /// Highest message read in an area by a user.
    pub fn lastread(&self, user: u32, area: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db
            .query_row("SELECT num FROM lastread WHERE user_id = ?1 AND area = ?2", params![user, area], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    pub fn set_lastread(&self, user: u32, area: &str, num: u32) -> anyhow::Result<()> {
        self.changed();
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
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
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row(
            "SELECT COALESCE(MAX(num), 0) FROM messages WHERE area = ?1 AND posted < ?2",
            params![area, when],
            |r| r.get(0),
        )?)
    }

    /// Messages addressed to a user (by alias or real name), by number.
    /// Mail to user number `user`; mail with no user number (from
    /// elsewhere, or old) by name, never "None".
    pub fn mail_for(&self, area: &str, user: u32, alias: &str, real_name: &str, deleted_too: bool) -> anyhow::Result<Vec<Message>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare(
            "SELECT * FROM messages WHERE area = ?1
               AND ((?5 <> 0 AND to_id = ?5)
                    OR (to_id = 0 AND to_alias <> 'None' COLLATE NOCASE
                        AND (to_alias = ?2 COLLATE NOCASE OR (?3 <> '' AND to_real = ?3 COLLATE NOCASE))))
               AND (?4 OR NOT deleted)
             ORDER BY num",
        )?;
        let rows = stmt.query_map(params![area, alias, real_name, deleted_too, user], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Every message in an area, by number.
    pub fn all(&self, area: &str) -> anyhow::Result<Vec<Message>> {
        self.range(area, 1, u32::MAX)
    }

    // ---- FTN (new) -------------------------------------------------------

    /// The message still here whose MSGID is `msgid` in `area` (REPLY).
    pub fn ftn_find(&self, area: &str, msgid: &str) -> anyhow::Result<Option<i64>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db
            .query_row("SELECT f.msg_id FROM ftn_msgs f JOIN messages m ON m.id = f.msg_id WHERE f.msgid = ?1 AND f.area = ?2", params![msgid, area], |r| r.get(0))
            .optional()?)
    }

    /// Has a message with this MSGID come to `area`, even one packed away
    /// since (dupes)?
    pub fn ftn_known(&self, area: &str, msgid: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT 1 FROM ftn_msgs WHERE msgid = ?1 AND area = ?2", params![msgid, area], |_| Ok(())).optional()?.is_some())
    }

    /// Where an echo's first scan starts: after the area's last message
    /// written before `before` (its history isn't sent; what was posted
    /// since, as the echo was being set up, is).
    pub fn ftn_start(&self, area: &str, before: NaiveDateTime) -> anyhow::Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COALESCE(MAX(id), 0) FROM messages WHERE area = ?1 AND posted < ?2", params![area, before], |r| r.get(0))?)
    }

    /// The first message after `after` in `area`, written since `since`,
    /// that's deleted or private and was never sent: the scan's mark stays
    /// below it, so it goes out if it's restored (or made public) soon.
    pub fn ftn_held(&self, area: &str, after: i64, since: NaiveDateTime) -> anyhow::Result<Option<i64>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row(
            "SELECT MIN(id) FROM messages m WHERE area = ?1 AND id > ?2 AND (deleted OR private) AND posted >= ?3
             AND NOT EXISTS (SELECT 1 FROM ftn_msgs f WHERE f.msg_id = m.id)",
            params![area, after, since],
            |r| r.get(0),
        )?)
    }

    /// Stop waiting for parents that never came: replies that came
    /// before `before` (not written then: a rescan brings old ones), or
    /// gone.
    pub fn ftn_forget_waits(&self, before: NaiveDateTime) -> anyhow::Result<usize> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute("DELETE FROM ftn_replies WHERE at < ?1 OR msg_id NOT IN (SELECT id FROM messages)", params![before])?)
    }


    /// An imported reply whose parent (`reply`, its MSGID) hasn't come yet.
    pub fn ftn_wait_reply(&self, msg_id: i64, area: &str, reply: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let now = crate::clock::now().naive_local();
        db.execute("INSERT OR REPLACE INTO ftn_replies (msg_id, area, reply, at) VALUES (?1, ?2, ?3, ?4)", params![msg_id, area, reply, now])?;
        Ok(())
    }

    /// Message `id` (MSGID `msgid`) has come: thread the replies that were
    /// waiting for it.
    pub fn ftn_link_replies(&self, area: &str, msgid: &str, id: i64) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE messages SET reply_to = ?3 WHERE reply_to = 0 AND id IN (SELECT msg_id FROM ftn_replies WHERE area = ?1 AND reply = ?2)",
            params![area, msgid, id],
        )?;
        let parent_private: bool = tx.query_row("SELECT private FROM messages WHERE id = ?1", [id], |r| r.get(0))?;
        let n: i64 = tx.query_row(
            "SELECT COUNT(*) FROM messages WHERE NOT deleted AND (NOT private OR ?3) AND id IN (SELECT msg_id FROM ftn_replies WHERE area = ?1 AND reply = ?2)",
            params![area, msgid, parent_private],
            |r| r.get(0),
        )?;
        if n > 0 {
            tx.execute("UPDATE messages SET replies = replies + ?2 WHERE id = ?1", params![id, n])?;
        }
        tx.execute("DELETE FROM ftn_replies WHERE area = ?1 AND reply = ?2", params![area, msgid])?;
        tx.commit()?;
        Ok(())
    }

    /// A MSGID given to a message about to be sent: kept if the packet is
    /// never written, so the resend has the same one. `ftn_unsent` still
    /// lists the message.
    pub fn ftn_pending(&self, msg_id: i64, msgid: &str, area: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("INSERT OR REPLACE INTO ftn_msgs (msg_id, msgid, area, local) VALUES (?1, ?2, ?3, 2)", params![msg_id, msgid, area])?;
        Ok(())
    }

    /// A message's MSGID, if it has one.
    pub fn ftn_msgid(&self, msg_id: i64) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT msgid FROM ftn_msgs WHERE msg_id = ?1", [msg_id], |r| r.get(0)).optional()?)
    }

    /// Remember message `msg_id`'s MSGID; `local` if it was written here.
    pub fn ftn_record(&self, msg_id: i64, msgid: &str, area: &str, local: bool) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("INSERT OR REPLACE INTO ftn_msgs (msg_id, msgid, area, local) VALUES (?1, ?2, ?3, ?4)", params![msg_id, msgid, area, local])?;
        Ok(())
    }

    /// A stored number (watermarks, the MSGID serial).
    pub fn ftn_get(&self, key: &str) -> anyhow::Result<Option<i64>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT value FROM ftn_state WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn ftn_set(&self, key: &str, value: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("INSERT OR REPLACE INTO ftn_state (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    /// Public messages in `area` after row `after` with no MSGID yet:
    /// written here and not sent, by row.
    pub fn ftn_unsent(&self, area: &str, after: i64) -> anyhow::Result<Vec<Message>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare(
            "SELECT * FROM messages m WHERE area = ?1 AND id > ?2 AND NOT deleted AND NOT private
             AND NOT EXISTS (SELECT 1 FROM ftn_msgs f WHERE f.msg_id = m.id AND f.local <> 2) ORDER BY id",
        )?;
        let rows = stmt.query_map(params![area, after], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// The newest row id (0 if none).
    pub fn max_id(&self) -> anyhow::Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |r| r.get(0))?)
    }

    /// New: `user`'s latest public post (not anonymous) in `areas`.
    pub fn latest_from(&self, user: u32, areas: &[String]) -> anyhow::Result<Option<Message>> {
        if areas.is_empty() {
            return Ok(None);
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let sql = format!(
            "SELECT * FROM messages WHERE from_id = ? AND NOT deleted AND NOT private AND NOT anonymous AND area IN ({}) ORDER BY posted DESC, id DESC LIMIT 1",
            vec!["?"; areas.len()].join(",")
        );
        let p = rusqlite::params_from_iter(std::iter::once(&user as &dyn rusqlite::ToSql).chain(areas.iter().map(|a| a as &dyn rusqlite::ToSql)));
        Ok(db.query_row(&sql, p, from_row).optional()?)
    }

    /// New: `user` was @mentioned in message `msg_id`.
    /// Record a mention; false if it was already recorded.
    pub fn add_mention(&self, user: u32, msg_id: i64) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute("INSERT OR IGNORE INTO mentions (user_id, msg_id) VALUES (?1, ?2)", params![user, msg_id])? > 0)
    }

    /// The messages `user` was mentioned in (newest first), unseen ones
    /// only if `unseen`, still there and not deleted.
    pub fn mentions(&self, user: u32, unseen: bool) -> anyhow::Result<Vec<Message>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare(
            "SELECT m.* FROM mentions n JOIN messages m ON m.id = n.msg_id
             WHERE n.user_id = ?1 AND (n.seen = 0 OR NOT ?2) AND NOT m.deleted ORDER BY m.posted DESC, m.id DESC LIMIT 100",
        )?;
        let rows = stmt.query_map(params![user, unseen], from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn mention_seen(&self, user: u32, msg_id: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE mentions SET seen = 1 WHERE user_id = ?1 AND msg_id = ?2", params![user, msg_id])?;
        Ok(())
    }

    // ---- pushes (new) ----------------------------------------------------

    /// A message's pushes, oldest first.
    pub fn pushes(&self, msg_id: i64) -> anyhow::Result<Vec<Push>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT id, msg_id, user_id, handle, kind, text, at FROM pushes WHERE msg_id = ?1 ORDER BY id")?;
        let rows = stmt
            .query_map([msg_id], |r| {
                Ok(Push { id: r.get(0)?, msg_id: r.get(1)?, user_id: r.get(2)?, handle: r.get(3)?, kind: r.get(4)?, text: r.get(5)?, at: r.get(6)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Add a push; a vote takes the place of the caller's last one on
    /// that message.
    pub fn add_push(&self, p: &Push) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if p.kind != 0 {
            tx.execute("DELETE FROM pushes WHERE msg_id = ?1 AND user_id = ?2 AND kind <> 0", params![p.msg_id, p.user_id])?;
        }
        tx.execute(
            "INSERT INTO pushes (msg_id, user_id, handle, kind, text, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![p.msg_id, p.user_id, p.handle, p.kind, p.text, p.at],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Take a caller's pushes off a message (a sponsor's clean-up);
    /// returns how many went.
    pub fn remove_pushes(&self, msg_id: i64, user_id: u32) -> anyhow::Result<usize> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute("DELETE FROM pushes WHERE msg_id = ?1 AND user_id = ?2", params![msg_id, user_id])?)
    }

    /// When a caller last pushed anything.
    pub fn last_push_at(&self, user_id: u32) -> anyhow::Result<Option<NaiveDateTime>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT MAX(at) FROM pushes WHERE user_id = ?1", [user_id], |r| r.get(0))?)
    }

    /// The score (ups less downs) of each message in an area with pushes.
    pub fn scores(&self, area: &str) -> anyhow::Result<std::collections::HashMap<i64, i32>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT p.msg_id, SUM(p.kind) FROM pushes p JOIN messages m ON m.id = p.msg_id WHERE m.area = ?1 GROUP BY p.msg_id")?;
        let rows = stmt.query_map([area], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// New: how many comments each message in an area has.
    pub fn comment_counts(&self, area: &str) -> anyhow::Result<std::collections::HashMap<i64, u32>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT p.msg_id, COUNT(*) FROM pushes p JOIN messages m ON m.id = p.msg_id WHERE m.area = ?1 AND p.kind = 0 GROUP BY p.msg_id")?;
        let rows = stmt.query_map([area], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// New: how many messages a caller has in an area.
    pub fn count_from(&self, area: &str, user_id: u32) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COUNT(*) FROM messages WHERE area = ?1 AND from_id = ?2 AND deleted = 0", params![area, user_id], |r| r.get(0))?)
    }

    /// New: the messages a moderator has pinned in an area.
    pub fn pinned(&self, area: &str) -> anyhow::Result<std::collections::HashSet<i64>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT p.msg_id FROM pins p JOIN messages m ON m.id = p.msg_id WHERE m.area = ?1")?;
        let rows = stmt.query_map([area], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn set_pinned(&self, msg_id: i64, pinned: bool) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        if pinned {
            db.execute("INSERT OR IGNORE INTO pins (msg_id) VALUES (?1)", [msg_id])?;
        } else {
            db.execute("DELETE FROM pins WHERE msg_id = ?1", [msg_id])?;
        }
        Ok(())
    }

    /// New: an area's filled requests, by the request's row id.
    pub fn filled(&self, area: &str) -> anyhow::Result<std::collections::HashMap<i64, Filled>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT f.msg_id, f.user_id, f.handle, f.note, f.at, f.told, f.filler_id, f.filler, f.reply_id, f.push_id FROM filled f JOIN messages m ON m.id = f.msg_id WHERE m.area = ?1")?;
        let rows = stmt.query_map([area], |r| filled_row(r).map(|f| (f.msg_id, f)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn filled_one(&self, msg_id: i64) -> anyhow::Result<Option<Filled>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row(&format!("SELECT {FILLED_COLS} FROM filled WHERE msg_id = ?1"), [msg_id], filled_row).optional()?)
    }

    /// Mark a request filled, or open again (None). Marked by its own
    /// asker, there's nobody to tell.
    pub fn set_filled(&self, msg_id: i64, mark: Option<FillMark>, at: NaiveDateTime) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        match mark {
            Some(m) => {
                db.execute(
                    "INSERT OR REPLACE INTO filled (msg_id, user_id, handle, note, at, told, filler_id, filler, reply_id, push_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?2 = (SELECT from_id FROM messages WHERE id = ?1), ?6, ?7, ?8, ?9)",
                    rusqlite::params![msg_id, m.by_id, m.by, m.note, at, m.filler_id, m.filler, m.reply_id, m.push_id],
                )?;
            }
            None => {
                db.execute("DELETE FROM filled WHERE msg_id = ?1", [msg_id])?;
            }
        }
        Ok(())
    }

    /// Requests `user` asked that were filled and they haven't heard:
    /// (row id, area, subject).
    pub fn filled_untold(&self, user: u32) -> anyhow::Result<Vec<(i64, String, String)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare(
            "SELECT m.id, m.area, m.subject FROM filled f JOIN messages m ON m.id = f.msg_id
             WHERE m.from_id = ?1 AND NOT f.told AND NOT m.deleted ORDER BY f.at",
        )?;
        let rows = stmt.query_map([user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn filled_told(&self, msg_id: i64) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE filled SET told = 1 WHERE msg_id = ?1", [msg_id])?;
        Ok(())
    }

    /// New: what's been said to a message: comments and votes under it,
    /// and replies to it still there.
    pub fn responses(&self, msg_id: i64) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row(
            "SELECT (SELECT COUNT(*) FROM pushes WHERE msg_id = ?1) + (SELECT COUNT(*) FROM messages WHERE reply_to = ?1 AND NOT deleted)",
            [msg_id],
            |r| r.get(0),
        )?)
    }

    /// New: an author retracts a message. Its text is kept here and the
    /// message shows `subject` and `body` (a note) in its place.
    pub fn retract(&self, m: &Message, subject: &str, body: &str, at: NaiveDateTime) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR IGNORE INTO retracted (msg_id, subject, body, sig_pos, at) SELECT id, subject, body, sig_pos, ?2 FROM messages WHERE id = ?1",
            params![m.id, at],
        )?;
        tx.execute("UPDATE messages SET subject = ?2, body = ?3, sig_pos = 0 WHERE id = ?1", params![m.id, subject, body])?;
        tx.commit()?;
        Ok(())
    }

    /// A retracted message's own text: (subject, lines, when retracted).
    pub fn retracted(&self, msg_id: i64) -> anyhow::Result<Option<(String, Vec<String>, NaiveDateTime)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db
            .query_row("SELECT subject, body, at FROM retracted WHERE msg_id = ?1", [msg_id], |r| {
                let body: String = r.get(1)?;
                Ok((r.get(0)?, body.split('\n').map(String::from).collect(), r.get(2)?))
            })
            .optional()?)
    }

    /// Undo a retraction: the message has its text back.
    pub fn unretract(&self, msg_id: i64) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE messages SET (subject, body, sig_pos) = (SELECT subject, body, sig_pos FROM retracted WHERE msg_id = ?1)
             WHERE id = ?1 AND EXISTS (SELECT 1 FROM retracted WHERE msg_id = ?1)",
            [msg_id],
        )?;
        tx.execute("DELETE FROM retracted WHERE msg_id = ?1", [msg_id])?;
        tx.commit()?;
        Ok(())
    }

    /// New: an author takes a message back or burns it: its text (and any
    /// kept by a retraction), what's under it and its marks are gone for
    /// good; a stub (`subject`, `body`) stays, deleted, for moderators. Returns the attachment it had, if any, to remove.
    pub fn burn(&self, msg_id: i64, subject: &str, body: &str) -> anyhow::Result<Option<String>> {
        let attached = {
            let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let attached: Option<String> = tx.query_row("SELECT name FROM attachments WHERE msg_id = ?1", [msg_id], |r| r.get(0)).optional()?;
            tx.execute("UPDATE messages SET subject = ?2, body = ?3, sig_pos = 0 WHERE id = ?1", params![msg_id, subject, body])?;
            for t in ["pushes", "mentions", "pins", "filled", "retracted", "attachments"] {
                tx.execute(&format!("DELETE FROM {t} WHERE msg_id = ?1"), [msg_id])?;
            }
            tx.commit()?;
            attached
        };
        self.set_deleted(msg_id, true, "its author")?;
        Ok(attached)
    }

    /// New: points for a fill, once per request and filler: they're never
    /// taken back (reopening doesn't), and marking it filled again doesn't
    /// pay twice. True if this one is new. The ledger outlasts the request.
    pub fn award_fill(&self, msg_id: i64, (filler_id, filler): (u32, &str), area: &str, subject: &str, points: u32, at: NaiveDateTime) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n = db.execute(
            "INSERT OR IGNORE INTO fills (msg_id, filler_id, filler, area, subject, points, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![msg_id, filler_id, filler, area, subject, points, at],
        )?;
        Ok(n > 0)
    }

    /// The fillers since `since` (None: ever), most points first: (user
    /// number, handle, fills, points).
    pub fn fill_board(&self, since: Option<NaiveDateTime>, limit: u32) -> anyhow::Result<Vec<(u32, String, u32, u32)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let since = since.unwrap_or_default();
        let mut stmt = db.prepare(
            "SELECT filler_id, MAX(filler), COUNT(*), SUM(points) FROM fills WHERE at >= ?1
             GROUP BY filler_id ORDER BY SUM(points) DESC, COUNT(*) DESC, MAX(at) LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![since, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// A caller's fills and points, ever.
    pub fn fills_of(&self, user: u32) -> anyhow::Result<(u32, u32)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT COUNT(*), COALESCE(SUM(points), 0) FROM fills WHERE filler_id = ?1", [user], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    /// A caller's fills they haven't heard about: (request, subject, points).
    pub fn fills_untold(&self, user: u32) -> anyhow::Result<Vec<(i64, String, u32)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT msg_id, subject, points FROM fills WHERE filler_id = ?1 AND NOT told ORDER BY at")?;
        let rows = stmt.query_map([user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn fill_told(&self, msg_id: i64, user: u32) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE fills SET told = 1 WHERE msg_id = ?1 AND filler_id = ?2", rusqlite::params![msg_id, user])?;
        Ok(())
    }

    /// Pushes since `since` on messages still there: (message, kind, when).
    pub fn recent_pushes(&self, since: NaiveDateTime) -> anyhow::Result<Vec<(i64, i8, NaiveDateTime)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT p.msg_id, p.kind, p.at FROM pushes p JOIN messages m ON m.id = p.msg_id WHERE p.at >= ?1 AND NOT m.deleted")?;
        let rows = stmt.query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// A caller's karma: the up and down votes on their public posts
    /// (not anonymous ones), and how many posts they're on.
    pub fn karma(&self, user_id: u32) -> anyhow::Result<(u32, u32, u32)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row(
            "SELECT COALESCE(SUM(p.kind > 0), 0), COALESCE(SUM(p.kind < 0), 0), COUNT(DISTINCT p.msg_id)
             FROM pushes p JOIN messages m ON m.id = p.msg_id
             WHERE p.kind <> 0 AND m.from_id = ?1 AND NOT m.anonymous AND NOT m.private AND NOT m.deleted",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }

    /// New: what the public messages (not private, not deleted) in `areas`
    /// add up to, for the message menu's codes; `since` is where "loudest"
    /// starts counting.
    pub fn stats(&self, areas: &[String], since: NaiveDateTime) -> anyhow::Result<MsgStats> {
        let mut s = MsgStats::default();
        if areas.is_empty() {
            return Ok(s);
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let w = format!("NOT deleted AND NOT private AND area IN ({})", vec!["?"; areas.len()].join(","));
        let p = rusqlite::params_from_iter(areas);
        s.total = db.query_row(&format!("SELECT COUNT(*) FROM messages WHERE {w}"), p.clone(), |r| r.get(0))?;
        let one = |sql: String| db.query_row(&sql, p.clone(), from_row).optional();
        // Rows are picked by id from the index, then read.
        s.newest = one(format!("SELECT * FROM messages WHERE id = (SELECT id FROM messages WHERE {w} ORDER BY posted DESC, id DESC LIMIT 1)"))?;
        s.hot = one(format!("SELECT * FROM messages WHERE id = (SELECT id FROM messages WHERE {w} AND replies > 0 ORDER BY replies DESC, posted DESC LIMIT 1)"))?;
        s.random = db
            .prepare(&format!("SELECT * FROM messages WHERE id IN (SELECT id FROM messages WHERE {w} ORDER BY RANDOM() LIMIT 20)"))?
            .query_map(p.clone(), from_row)?
            .collect::<Result<_, _>>()?;
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
                .query_row(
                    &format!("SELECT * FROM messages WHERE id = (SELECT id FROM messages WHERE {w} AND NOT anonymous AND from_name = ? COLLATE NOCASE ORDER BY posted DESC, id DESC LIMIT 1)"),
                    q,
                    from_row,
                )
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

    #[test]
    fn waits_are_given_up_by_when_they_came() {
        let b = MsgBase::open_memory().unwrap();
        let old = chrono::NaiveDate::from_ymd_opt(2025, 4, 28).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut m = Message { subject: "Re: hi".into(), posted: old, ..msg("AGN", "All", "x") };
        b.add_ftn(&mut m, Some("46:1/109 00000001")).unwrap();
        b.ftn_wait_reply(m.id, "AGN", "46:1/700 00000002").unwrap();
        // Written long ago (a rescan), but only just come: still waiting.
        let now = crate::clock::now().naive_local();
        assert_eq!(b.ftn_forget_waits(now - chrono::Duration::days(30)).unwrap(), 0);
        assert_eq!(b.ftn_forget_waits(now + chrono::Duration::days(1)).unwrap(), 1);
    }

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
        assert_eq!(mb.mail_for("EMAIL", 0, "phrac", "", false).unwrap()[0].tag, 5);

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
    fn attachments_outlive_only_their_message() {
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
        mb.purge(a.id).unwrap();
        let kept = mb.get("GEN", 2).unwrap().unwrap();
        assert_eq!((kept.id, kept.lines[0].as_str()), (b.id, "second"), "not renumbered");
        assert_eq!(mb.attachment(kept.id).unwrap(), Some(file.clone()));
        assert_eq!(mb.orphan_attachments().unwrap(), [(a.id, "X.ZIP".to_string())]);
        assert!(mb.orphan_attachments().unwrap().is_empty());
        mb.remove_attachment(kept.id).unwrap();
        assert_eq!(mb.attachment(kept.id).unwrap(), None);
    }

    #[test]
    fn row_ids_are_never_used_twice() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("GEN", "All", "first");
        mb.add(&mut a).unwrap();
        let mut b = msg("GEN", "All", "newest");
        b.deleted = true;
        mb.add(&mut b).unwrap();
        mb.ftn_record(b.id, "21:1/999 00000001", "GEN", true).unwrap();
        mb.purge(b.id).unwrap();
        assert_eq!(mb.ftn_find("GEN", "21:1/999 00000001").unwrap(), None, "nothing to reply to");
        assert!(mb.ftn_known("GEN", "21:1/999 00000001").unwrap(), "still a dupe");
        let mut c = msg("GEN", "All", "after purging");
        mb.add(&mut c).unwrap();
        assert!(c.id > b.id);
    }

    #[test]
    fn updates_follow_the_row_and_replies_count_themselves() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("GEN", "All", "gone");
        a.deleted = true;
        mb.add(&mut a).unwrap();
        let mut b = msg("GEN", "All", "kept");
        mb.add(&mut b).unwrap();
        let mut c = msg("GEN", "All", "third");
        mb.add(&mut c).unwrap();
        // A reader has #2 open while #1 goes.
        let mut open = mb.get("GEN", 2).unwrap().unwrap();
        mb.purge(a.id).unwrap();
        open.deleted = true;
        mb.update(&open).unwrap();
        assert!(mb.by_id(b.id).unwrap().unwrap().deleted);
        assert!(!mb.by_id(c.id).unwrap().unwrap().deleted, "only that row");
        let mut r = msg("GEN", "All", "a reply");
        r.reply_to = c.id;
        mb.add(&mut r).unwrap();
        // A stale copy saved afterwards doesn't undo the count.
        let stale = mb.by_id(c.id).unwrap().unwrap();
        let mut r2 = msg("GEN", "All", "another");
        r2.reply_to = c.id;
        mb.add(&mut r2).unwrap();
        mb.update(&Message { replies: 1, ..stale }).unwrap();
        assert_eq!(mb.by_id(c.id).unwrap().unwrap().replies, 2);
    }

    #[test]
    fn retracted_kept_and_burned_gone() {
        let mb = MsgBase::open_memory().unwrap();
        let mut p = msg("GEN", "All", "i regret this");
        mb.add(&mut p).unwrap();
        let mut r = msg("GEN", "All", "a reply");
        r.reply_to = p.id;
        mb.add(&mut r).unwrap();
        assert_eq!(mb.responses(p.id).unwrap(), 1);
        let at = NaiveDateTime::default();
        mb.retract(&p, "[retracted]", "[retracted by its author]", at).unwrap();
        let now = mb.by_id(p.id).unwrap().unwrap();
        assert_eq!((now.subject.as_str(), now.lines.clone()), ("[retracted]", vec!["[retracted by its author]".to_string()]));
        let kept = mb.retracted(p.id).unwrap().unwrap();
        assert_eq!(kept.1, vec!["i regret this".to_string()]);
        mb.unretract(p.id).unwrap();
        assert_eq!(mb.by_id(p.id).unwrap().unwrap().lines, vec!["i regret this".to_string()]);
        assert!(mb.retracted(p.id).unwrap().is_none());
        mb.retract(&p, "[retracted]", "[retracted by its author]", at).unwrap();
        mb.burn(p.id, "[burned]", "[burned by its author]").unwrap();
        let gone = mb.by_id(p.id).unwrap().unwrap();
        assert!(gone.deleted && gone.lines == vec!["[burned by its author]".to_string()]);
        assert!(mb.retracted(p.id).unwrap().is_none(), "nothing of it is kept");
    }

    #[test]
    fn reading_and_posting_are_changes() {
        let mb = MsgBase::open_memory().unwrap();
        let n = mb.changes();
        let mut a = msg("GEN", "All", "first");
        mb.add(&mut a).unwrap();
        assert!(mb.changes() > n);
        let n = mb.changes();
        mb.set_lastread(1, "GEN", 1).unwrap();
        assert!(mb.changes() > n);
        let n = mb.changes();
        mb.mark_seen(1, a.id).unwrap();
        assert!(mb.changes() > n);
        let n = mb.changes();
        let _ = mb.all("GEN").unwrap();
        assert_eq!(mb.changes(), n, "reading the base isn't");
    }

    #[test]
    fn the_trash_keeps_then_empties() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("GEN", "All", "first");
        mb.add(&mut a).unwrap();
        let mut b = msg("GEN", "All", "second");
        mb.add(&mut b).unwrap();
        assert!(mb.trash().unwrap().is_empty());
        mb.set_deleted(a.id, true, "phrac").unwrap();
        let t = mb.trash().unwrap();
        assert_eq!((t.len(), t[0].id, t[0].by.as_str(), t[0].num), (1, a.id, "phrac", a.num));
        assert!(t[0].at.is_some(), "when it went is kept");
        // Restored, it's out of the trash and keeps nothing of it.
        mb.set_deleted(a.id, false, "phrac").unwrap();
        assert!(mb.trash().unwrap().is_empty());
        mb.set_deleted(a.id, true, "phrac").unwrap();
        mb.set_deleted(b.id, true, "Fiend").unwrap();
        // Only what was deleted before the cutoff goes.
        let now = crate::clock::now().naive_local();
        assert_eq!(mb.purge_trash(now - chrono::Duration::days(1)).unwrap(), 0);
        assert_eq!(mb.purge_trash(now + chrono::Duration::seconds(5)).unwrap(), 2);
        assert!(mb.by_id(a.id).unwrap().is_none() && mb.by_id(b.id).unwrap().is_none());
        assert!(mb.trash().unwrap().is_empty());
    }

    #[test]
    fn private_and_deleted_replies_dont_count() {
        let mb = MsgBase::open_memory().unwrap();
        let mut p = msg("GEN", "All", "public");
        mb.add(&mut p).unwrap();
        let mut hush = msg("GEN", "Fiend", "psst");
        (hush.private, hush.reply_to) = (true, p.id);
        mb.add(&mut hush).unwrap();
        let mut r = msg("GEN", "All", "open reply");
        r.reply_to = p.id;
        mb.add(&mut r).unwrap();
        assert_eq!(mb.by_id(p.id).unwrap().unwrap().replies, 1, "the private reply isn't shown");
        mb.set_deleted(r.id, true, "sysop").unwrap();
        mb.set_deleted(r.id, true, "sysop").unwrap();
        assert_eq!(mb.by_id(p.id).unwrap().unwrap().replies, 0);
        mb.set_deleted(r.id, false, "sysop").unwrap();
        assert_eq!(mb.by_id(p.id).unwrap().unwrap().replies, 1);
        // Between private messages, it counts.
        let mut q = msg("EMAIL", "Fiend", "letter");
        q.private = true;
        mb.add(&mut q).unwrap();
        let mut a = msg("EMAIL", "phrac", "answer");
        (a.private, a.reply_to) = (true, q.id);
        mb.add(&mut a).unwrap();
        assert_eq!(mb.by_id(q.id).unwrap().unwrap().replies, 1);
    }

    #[test]
    fn qwk_conferences_follow_their_areas() {
        let mb = MsgBase::open_memory().unwrap();
        mb.set_qwk_confs(5, &[(1, "GEN".into()), (2, "FSX_GEN".into())]).unwrap();
        mb.rename_area("FSX_GEN", "FSXGEN").unwrap();
        assert_eq!(mb.qwk_conf(5, 2).unwrap().as_deref(), Some("FSXGEN"));
        assert_eq!(mb.qwk_conf(6, 2).unwrap(), None);
        mb.delete_area("GEN").unwrap();
        assert_eq!(mb.qwk_conf(5, 1).unwrap(), None);
    }

    #[test]
    fn deleting_an_area_keeps_its_dupe_history() {
        let mb = MsgBase::open_memory().unwrap();
        let mut m = msg("FSX", "All", "from afar");
        mb.add_ftn(&mut m, Some("21:1/100 0001")).unwrap();
        mb.delete_area("FSX").unwrap();
        assert!(mb.ftn_known("FSX", "21:1/100 0001").unwrap());
    }

    #[test]
    fn purging() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("EMAIL", "Fiend", "hi");
        mb.add(&mut a).unwrap();
        let mut b = msg("EMAIL", "Fiend", "re: hi");
        b.reply_to = a.id;
        mb.add(&mut b).unwrap();
        let mut c = msg("EMAIL", "Fiend", "later");
        mb.add(&mut c).unwrap();
        assert_eq!(mb.by_id(a.id).unwrap().unwrap().replies, 1);
        mb.purge(b.id).unwrap();
        assert!(mb.by_id(b.id).unwrap().is_none());
        assert_eq!(mb.by_id(a.id).unwrap().unwrap().replies, 0, "the reply no longer counts");
        mb.set_deleted(c.id, true, "sysop").unwrap();
        assert_eq!(mb.purge_deleted("EMAIL").unwrap(), 1);
        let left: Vec<String> = mb.all("EMAIL").unwrap().into_iter().map(|m| m.lines[0].clone()).collect();
        assert_eq!(left, ["hi"]);
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
        assert!(got.deleted && got.replies == 0, "replies are counted by add");
        assert_eq!(mb.mail_for("GENERAL", 0, "fiend", "", false).unwrap().len(), 0);
        assert_eq!(mb.mail_for("GENERAL", 0, "fiend", "", true).unwrap().len(), 1);
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
    fn pushes_vote_once_and_go_with_their_message() {
        let mb = MsgBase::open_memory().unwrap();
        let mut a = msg("GENERAL", "All", "first");
        let mut b = msg("GENERAL", "All", "second");
        b.anonymous = true;
        mb.add(&mut a).unwrap();
        mb.add(&mut b).unwrap();
        let at = |h: u32| chrono::NaiveDate::from_ymd_opt(2026, 10, 23).unwrap().and_hms_opt(h, 0, 0).unwrap();
        let push = |msg_id: i64, user_id: u32, kind: i8, h: u32| {
            mb.add_push(&Push { id: 0, msg_id, user_id, handle: format!("u{user_id}"), kind, text: String::new(), at: at(h) }).unwrap()
        };
        push(a.id, 2, 1, 1);
        push(a.id, 3, 1, 2);
        push(a.id, 2, -1, 3);
        push(a.id, 2, 0, 4);
        push(a.id, 2, 0, 5);
        push(b.id, 2, 1, 6);
        // User 2's down took the place of their up; comments add up.
        let p = mb.pushes(a.id).unwrap();
        assert_eq!(p.iter().map(|p| (p.user_id, p.kind)).collect::<Vec<_>>(), vec![(3, 1), (2, -1), (2, 0), (2, 0)]);
        assert_eq!(mb.scores("GENERAL").unwrap(), std::collections::HashMap::from([(a.id, 0), (b.id, 1)]));
        assert_eq!(mb.last_push_at(2).unwrap(), Some(at(6)));
        // Anonymous posts keep their karma to themselves.
        assert_eq!(mb.karma(1).unwrap(), (1, 1, 1));
        assert_eq!(mb.recent_pushes(at(5)).unwrap().len(), 2);
        assert_eq!(mb.remove_pushes(a.id, 2).unwrap(), 3);
        mb.set_deleted(b.id, true, "sysop").unwrap();
        assert!(mb.recent_pushes(at(0)).unwrap().iter().all(|p| p.0 == a.id));
        mb.purge(b.id).unwrap();
        assert!(mb.pushes(b.id).unwrap().is_empty());
        assert_eq!(mb.pushes(a.id).unwrap().len(), 1);
    }

    #[test]
    fn body_excludes_signature() {
        let mut m = msg("A", "All", "hello\n\n-- sig");
        m.sig_pos = 2;
        assert_eq!(m.body(), ["hello"]);
    }
}
