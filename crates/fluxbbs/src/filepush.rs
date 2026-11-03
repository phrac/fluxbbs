//! New: pushes on files, as on posts (`pushes.rs`): a vote up (`+`) or
//! down (`-`), one a caller, or a one-line comment (`>`), with `x` in the
//! file lister, the browser and gallery, and the reading room's reader.
//! A downvote is how callers flag a file with bad information in it. A
//! file's score shows beside it in the lists and its pushes under its
//! information, and the hot files (`FW votes`) are those pushed most
//! lately. Pushes hang on the file's record (its id), so they follow it
//! through renames and moves. An upvote pays points only to a caller who
//! uploaded the file through the board, never on imported files. Pushes
//! stay here: TIC, FTN and QWK never carry them.

use std::io;

use chrono::NaiveDateTime;

use crate::filebase::{FilePush, FileRec, FileScore};
use crate::input::{Key, ReadOpts};
use crate::pushes::{PUSH_GAP, PUSH_WIDTH, heats};
use crate::session::Session;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// How far back the hot files look, and how many there are.
const HOT_HOURS: i64 = 72;
pub const HOT_FILES: usize = 40;
/// The pushes a file's information shows (the latest).
const SHOWN: usize = 8;

impl Session {
    /// The caller can see the file's area, so may push it.
    fn may_push_file(&self, f: &FileRec) -> bool {
        let fa = self.state.fareas();
        fa.areas.iter().position(|a| a.file.eq_ignore_ascii_case(&f.area)).is_some_and(|i| self.farea_visible(i as u32 + 1))
    }

    /// Is the file the caller's own upload?
    fn own_file(&self, f: &FileRec) -> bool {
        match self.state.files.caller_upload(f.id) {
            Ok(Some(uid)) => uid == self.user.id,
            _ => f.uploader.eq_ignore_ascii_case(&self.user.handle) && !f.uploader.eq_ignore_ascii_case(&self.state.config().bbs.sysop_alias),
        }
    }

    /// A file's votes in sum.
    pub(crate) fn file_score(&self, f: &FileRec) -> FileScore {
        self.state.files.file_score(f.id).unwrap_or_default()
    }

    /// The pushes on a file as lines for its information: a rule with
    /// its score, then the latest few, and how many earlier.
    pub(crate) fn file_push_lines(&self, f: &FileRec) -> Vec<String> {
        let pushes = self.state.files.file_pushes(f.id).unwrap_or_default();
        if pushes.is_empty() {
            return Vec::new();
        }
        let rows: Vec<(i8, &str, &str, NaiveDateTime)> = pushes.iter().map(|p| (p.kind, p.handle.as_str(), p.text.as_str(), p.at)).collect();
        let mut out = self.push_rows(&rows);
        // The score is all of them; the lines, the latest.
        let earlier = out.len().saturating_sub(1 + SHOWN);
        if earlier > 0 {
            out.drain(1..1 + earlier);
            out.insert(1, self.strings().fmt("FilePushEarlier", &[&earlier.to_string()]));
        }
        out
    }

    /// The pushes on a file as plain rows for a card: the sign, who,
    /// what, and when, the latest `most`.
    #[cfg_attr(not(feature = "library"), allow(dead_code))]
    pub(crate) fn file_push_plain(&self, f: &FileRec, most: usize) -> (FileScore, Vec<String>) {
        let pushes = self.state.files.file_pushes(f.id).unwrap_or_default();
        let rows = pushes
            .iter()
            .rev()
            .take(most)
            .rev()
            .map(|p| {
                let sign = match p.kind {
                    1 => '+',
                    -1 => '-',
                    _ => '>',
                };
                let who: String = p.handle.chars().take(12).collect();
                let text: String = p.text.chars().take(PUSH_WIDTH).collect();
                format!("{sign} {who:<12} {text:<w$} {}", self.local(p.at).format("%m/%d"), w = PUSH_WIDTH)
            })
            .collect();
        (self.file_score(f), rows)
    }

    /// `x` on a file: vote it up or down, or comment on it, on the bottom
    /// line; a co-SysOp can take a caller's pushes off. True if anything
    /// changed.
    pub(crate) async fn file_push_prompt(&mut self, f: &FileRec) -> io::Result<bool> {
        if f.id == 0 || !self.may_push_file(f) {
            return self.bar_note("PushNotHere").await;
        }
        let moderator = self.is_cosysop();
        self.bar_line();
        self.say(if moderator { "FilePushAskSysop" } else { "FilePushAsk" }).await?;
        let kind: i8 = match self.read_key().await? {
            Key::Char(b'+') => 1,
            Key::Char(b'-') => -1,
            Key::Char(b'>') => 0,
            k if moderator && k.upper() == Some('D') => return self.file_push_remove(f).await,
            _ => return Ok(false),
        };
        if kind != 0 && self.own_file(f) {
            return self.bar_note("FilePushOwn").await;
        }
        let now = crate::clock::now().naive_local();
        if let Some(last) = self.state.files.last_file_push_at(self.user.id).map_err(io_err)?
            // (Not when the clocks went back since: that's no hurry.)
            && (0..PUSH_GAP).contains(&(now - last).num_seconds())
        {
            return self.bar_note("PushTooFast").await;
        }
        self.bar_line();
        self.say(match kind {
            1 => "PushTextUp",
            -1 => "FilePushTextDown",
            _ => "FilePushTextComment",
        })
        .await?;
        let text = self.read_string(ReadOpts::new(PUSH_WIDTH)).await?;
        let text = crate::editor::strip_colours(&text).trim().to_string();
        if kind == 0 && text.is_empty() {
            return Ok(false);
        }
        let p = FilePush { id: 0, file_id: f.id, user_id: self.user.id, handle: self.user.handle.clone(), kind, text, at: now };
        self.state.files.add_file_push(&p).map_err(io_err)?;
        tracing::info!("{}: pushed file {}/{} ({kind})", self.user.handle, f.area, f.name);
        if kind == 1 {
            self.pay_file_upvote(f);
        }
        Ok(true)
    }

    /// A co-SysOp's `d`: take one caller's pushes off a file.
    async fn file_push_remove(&mut self, f: &FileRec) -> io::Result<bool> {
        let pushes = self.state.files.file_pushes(f.id).map_err(io_err)?;
        let voters: Vec<(u32, &str)> = pushes.iter().map(|p| (p.user_id, p.handle.as_str())).collect();
        let Some(uid) = self.ask_whose_pushes(&voters).await? else { return Ok(false) };
        let n = self.state.files.remove_file_pushes(f.id, uid).map_err(io_err)?;
        tracing::info!("{}: took {n} pushes by user {uid} off {}/{}", self.user.handle, f.area, f.name);
        Ok(n > 0)
    }

    /// The hot files the caller can see: the most pushed lately, then
    /// (if that's few) the best scored ever. With the area each is in.
    pub(crate) fn hot_files(&self, n: usize) -> io::Result<Vec<(u32, FileRec)>> {
        let now = crate::clock::now().naive_local();
        let recent = self.state.files.file_pushes_since(now - chrono::Duration::hours(HOT_HOURS)).map_err(io_err)?;
        let mut hot: Vec<(i64, f64)> = heats(&recent, now, HOT_HOURS).into_iter().filter(|(_, h)| *h > 0.0).collect();
        hot.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut ids: Vec<i64> = hot.into_iter().map(|(id, _)| id).collect();
        for id in self.state.files.top_scored_files(n * 2).map_err(io_err)? {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let fa = self.state.fareas();
        let visible: std::collections::HashMap<String, u32> =
            self.visible_fareas().iter().filter_map(|&(n, _)| fa.get(n).filter(|a| a.password.is_empty()).map(|a| (a.file.to_lowercase(), n))).collect();
        let mut out = Vec::new();
        for id in ids {
            if let Ok(Some(f)) = self.state.files.by_id(id)
                && f.valid
                && let Some(&num) = visible.get(&f.area.to_lowercase())
            {
                out.push((num, f));
            }
            if out.len() >= n {
                break;
            }
        }
        Ok(out)
    }
}
