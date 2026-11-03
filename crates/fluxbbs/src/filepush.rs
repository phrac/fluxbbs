//! New: pushes on files, as on posts (`pushes.rs`): a vote up (`+`) or
//! down (`-`), one a caller, or a one-line comment (`>`), with `x` in the
//! file lister, the browser and gallery, and the reading room's reader.
//! A downvote is how callers flag a file with bad information in it. A
//! file's score shows beside it in the lists and its pushes under its
//! information, and the hot files (`FW votes`) are those pushed most
//! lately. An upvote pays points only to a caller who uploaded the file
//! through the board, never on imported files. Pushes stay here: TIC, FTN
//! and QWK never carry them.

use std::collections::HashMap;
use std::io;

use chrono::NaiveDateTime;
use fluxbbs_term::codes::{lit, resize};

use crate::filebase::{FilePush, FileRec, FileScore};
use crate::input::{Key, ReadOpts};
use crate::pushes::{PUSH_GAP, PUSH_WIDTH, score_label};
use crate::session::Session;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// How far back the hot files look, and how many there are.
const HOT_HOURS: i64 = 72;
pub const HOT_FILES: usize = 40;

/// Each file's heat: its pushes since `HOT_HOURS` ago, votes one each way
/// and comments a half, fading with age. Keyed `area/name`, lower case.
pub fn file_heats(recent: &[(String, String, i8, NaiveDateTime)], now: NaiveDateTime) -> HashMap<String, f64> {
    let mut h = HashMap::new();
    for (area, name, kind, at) in recent {
        let age = (now - *at).num_minutes() as f64 / 60.0;
        let fade = (1.0 - age / HOT_HOURS as f64).clamp(0.0, 1.0);
        let w = match kind {
            1 => 1.0,
            -1 => -1.0,
            _ => 0.5,
        };
        *h.entry(format!("{}/{}", area.to_lowercase(), name.to_lowercase())).or_insert(0.0) += w * fade;
    }
    h
}

impl Session {
    /// The caller can see the file's area, so may push it.
    fn may_push_file(&self, f: &FileRec) -> bool {
        let fa = self.state.fareas();
        fa.areas.iter().position(|a| a.file.eq_ignore_ascii_case(&f.area)).is_some_and(|i| self.farea_visible(i as u32 + 1))
    }

    /// Is the file the caller's own upload?
    fn own_file(&self, f: &FileRec) -> bool {
        match self.state.files.caller_upload(&f.area, &f.name) {
            Ok(Some(uid)) => uid == self.user.id,
            _ => f.uploader.eq_ignore_ascii_case(&self.user.handle) && !f.uploader.eq_ignore_ascii_case(&self.state.config().bbs.sysop_alias),
        }
    }

    /// A file's pushes in sum.
    pub(crate) fn file_score(&self, f: &FileRec) -> FileScore {
        self.state.files.file_scores(&f.area).ok().and_then(|m| m.get(&f.name.to_lowercase()).copied()).unwrap_or_default()
    }

    /// The lines about a file's pushes: a rule with its score, then each.
    pub(crate) fn file_push_lines(&self, f: &FileRec) -> Vec<String> {
        let pushes = self.state.files.file_pushes(&f.area, &f.name).unwrap_or_default();
        if pushes.is_empty() {
            return Vec::new();
        }
        let score: i32 = pushes.iter().map(|p| p.kind as i32).sum();
        let s = self.strings();
        let mut out = vec![s.fmt("PushHead", &[&score_label(Some(score))])];
        for p in &pushes {
            let name = match p.kind {
                1 => "PushUp",
                -1 => "PushDown",
                _ => "PushComment",
            };
            let when = self.local(p.at).format("%m/%d %H:%M").to_string();
            out.push(s.fmt(name, &[&lit(&resize(&p.handle, 12)), &lit(&resize(&p.text, PUSH_WIDTH)), &when]));
        }
        out
    }

    /// `x` on a file: vote it up or down, or comment on it, on the bottom
    /// line; a co-SysOp can take a caller's pushes off. True if anything
    /// changed.
    pub(crate) async fn file_push_prompt(&mut self, f: &FileRec) -> io::Result<bool> {
        if !self.may_push_file(f) {
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
            && (now - last).num_seconds() < PUSH_GAP
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
        let p = FilePush { id: 0, area: f.area.clone(), name: f.name.clone(), user_id: self.user.id, handle: self.user.handle.clone(), kind, text, at: now };
        self.state.files.add_file_push(&p).map_err(io_err)?;
        tracing::info!("{}: pushed file {}/{} ({kind})", self.user.handle, f.area, f.name);
        if kind == 1 {
            self.pay_file_upvote(f);
        }
        Ok(true)
    }

    /// A co-SysOp's `d`: take one caller's pushes off a file.
    async fn file_push_remove(&mut self, f: &FileRec) -> io::Result<bool> {
        self.bar_line();
        self.say("PushRemoveWho").await?;
        let mut opts = ReadOpts::new(36);
        opts.complete_users = true;
        let who = self.read_string(opts).await?;
        if who.trim().is_empty() {
            return Ok(false);
        }
        let pushes = self.state.files.file_pushes(&f.area, &f.name).map_err(io_err)?;
        let Some(uid) = pushes.iter().find(|p| p.handle.eq_ignore_ascii_case(who.trim())).map(|p| p.user_id) else {
            return self.bar_note("PushRemoveNone").await;
        };
        let n = self.state.files.remove_file_pushes(&f.area, &f.name, uid).map_err(io_err)?;
        tracing::info!("{}: took {n} pushes by {} off {}/{}", self.user.handle, who.trim(), f.area, f.name);
        Ok(n > 0)
    }

    /// The hot files the caller can see: the most pushed lately, then
    /// (if that's few) the best scored ever.
    pub(crate) fn hot_files(&self, n: usize) -> io::Result<Vec<(u32, FileRec)>> {
        let now = crate::clock::now().naive_local();
        let recent = self.state.files.file_pushes_since(now - chrono::Duration::hours(HOT_HOURS)).map_err(io_err)?;
        let heats = file_heats(&recent, now);
        let mut keys: Vec<(String, f64)> = heats.into_iter().filter(|(_, h)| *h > 0.0).collect();
        keys.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut wanted: Vec<(String, String)> = keys.into_iter().filter_map(|(k, _)| k.split_once('/').map(|(a, b)| (a.to_string(), b.to_string()))).collect();
        for (a, name, _) in self.state.files.top_scored_files(n * 2).map_err(io_err)? {
            let k = (a.to_lowercase(), name.to_lowercase());
            if !wanted.contains(&k) {
                wanted.push(k);
            }
        }
        let fa = self.state.fareas();
        let visible: HashMap<String, u32> = self.visible_fareas().iter().filter_map(|&(n, _)| fa.get(n).filter(|a| a.password.is_empty()).map(|a| (a.file.to_lowercase(), n))).collect();
        let mut out = Vec::new();
        for (area, name) in wanted {
            let Some(&num) = visible.get(&area) else { continue };
            if let Ok(Some(f)) = self.state.files.find(&area, &name)
                && f.valid
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heat_fades_and_votes_count_both_ways() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let ago = |h| now - chrono::Duration::hours(h);
        let recent = vec![
            ("A".to_string(), "x.txt".to_string(), 1, ago(1)),
            ("A".to_string(), "X.TXT".to_string(), 0, ago(2)),
            ("A".to_string(), "y.txt".to_string(), -1, ago(1)),
            ("B".to_string(), "z.txt".to_string(), 1, ago(HOT_HOURS + 1)),
        ];
        let h = file_heats(&recent, now);
        assert!(h["a/x.txt"] > 1.0, "a vote and a comment, either case");
        assert!(h["a/y.txt"] < 0.0);
        assert_eq!(h["b/z.txt"], 0.0, "too old to count");
    }
}
