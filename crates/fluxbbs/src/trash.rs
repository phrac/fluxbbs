//! New: the trash. A deleted post (`D` in the reader) stays restorable
//! for `[messages] keep_deleted_days`, then goes for good. The original
//! kept deleted messages till the SysOp packed the area (`maPackArea`);
//! here the board empties the trash itself, at startup and daily, and
//! `*-` lists what's in it to restore or purge now.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use fluxbbs_term::Slot;
use fluxbbs_term::codes::resize;

use crate::BbsState;
use crate::editor::strip_colours;
use crate::input::ReadOpts;
use crate::msgbase::Trashed;
use crate::session::Session;

/// Empty the trash of what's been in it `keep_deleted_days` (0: nothing
/// goes), and the attachments that leaves behind. Returns how many went.
pub fn empty(state: &BbsState) -> anyhow::Result<u32> {
    let days = state.config().messages.keep_deleted_days;
    if days == 0 {
        state.msgs.date_trash(crate::clock::now().naive_local())?;
        return Ok(0);
    }
    let now = crate::clock::now().naive_local();
    state.msgs.date_trash(now)?;
    let n = state.msgs.purge_trash(now - chrono::Duration::days(days as i64))?;
    if n > 0 {
        crate::attach::remove_orphans(state)?;
        tracing::info!("trash: {n} post{} deleted over {days} days ago gone for good", if n == 1 { "" } else { "s" });
    }
    Ok(n)
}

/// At startup, then every day.
pub async fn keeper(state: Arc<BbsState>) {
    loop {
        if let Err(e) = empty(&state) {
            tracing::warn!("trash: emptying it: {e:#}");
        }
        tokio::time::sleep(Duration::from_secs(86_400)).await;
    }
}

/// Days left before a post deleted `at` goes, with `keep` days kept (None:
/// kept for good).
fn days_left(at: Option<NaiveDateTime>, keep: u32, now: NaiveDateTime) -> Option<i64> {
    if keep == 0 {
        return None;
    }
    let at = at.unwrap_or(now);
    let left = (at + chrono::Duration::days(keep as i64) - now).num_hours();
    Some((left as f64 / 24.0).ceil().max(0.0) as i64)
}

impl Session {
    /// `*-`: the trash, newest first; a number to restore, read or purge
    /// a post now.
    pub async fn message_trash(&mut self) -> io::Result<()> {
        loop {
            let items = self.state.msgs.trash().map_err(io::Error::other)?;
            let keep = self.state.config().messages.keep_deleted_days;
            let now = crate::clock::now().naive_local();
            self.clear_screen();
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line("|U4   # Area     Msg   From          Subject                  Deleted     Left").await?;
            self.rule().await?;
            if items.is_empty() {
                self.say_line("TrashEmpty").await?;
                self.rule().await?;
                return Ok(());
            }
            let mut lines = 3u16;
            for (i, t) in items.iter().enumerate() {
                let when = t.at.map_or_else(|| "?".into(), |at| self.local(at).format("%m/%d %H:%M").to_string());
                let left = days_left(t.at, keep, now).map_or_else(|| "kept".into(), |d| format!("{d}d"));
                let line = format!(
                    "|U3{:>4} |U1{} |U1{:<5} {} {} |U2{} |U3{}",
                    i + 1,
                    resize(&t.area, 8),
                    t.num,
                    resize(&strip_colours(&t.from), 13),
                    resize(&strip_colours(&t.subject), 24),
                    resize(&when, 11),
                    left
                );
                self.show_line(&line).await?;
                lines += 1;
                if !self.more(&mut lines).await? {
                    break;
                }
            }
            self.rule().await?;
            self.say("TrashWhich").await?;
            let n = self.read_string(ReadOpts::new(5).allowed(crate::input::DIGITS)).await?;
            self.newline();
            let Some(t) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| items.get(i)).cloned() else {
                return Ok(());
            };
            self.trash_one(&t).await?;
        }
    }

    /// One post from the trash: who deleted it, then restore, read or
    /// purge it.
    async fn trash_one(&mut self, t: &Trashed) -> io::Result<()> {
        loop {
            let by = if t.by.is_empty() { "?".to_string() } else { t.by.clone() };
            let s = self.strings().fmt("TrashOne", &[&t.area, &t.num.to_string(), &strip_colours(&t.subject), &by]);
            self.show_line(&s).await?;
            self.say("TrashWhat").await?;
            let k = self.read_string(ReadOpts::new(1)).await?;
            self.newline();
            match k.to_ascii_uppercase().as_str() {
                "R" => {
                    self.state.msgs.set_deleted(t.id, false, "").map_err(io::Error::other)?;
                    tracing::info!("{}: message {} in {} restored from the trash", self.user.handle, t.num, t.area);
                    return Ok(());
                }
                "P" => {
                    self.say("TrashSure").await?;
                    if self.yes_no(false).await? {
                        self.state.msgs.purge(t.id).map_err(io::Error::other)?;
                        crate::attach::remove_orphans(&self.state).map_err(io::Error::other)?;
                        tracing::info!("{}: message {} in {} purged from the trash", self.user.handle, t.num, t.area);
                    }
                    return Ok(());
                }
                "V" => {
                    if let Some(m) = self.state.msgs.by_id(t.id).map_err(io::Error::other)? {
                        let mut lines = 1u16;
                        self.show_line(&format!("|U4{} |U1» |U3{}", strip_colours(&m.from.name), strip_colours(&m.subject))).await?;
                        for l in &m.lines {
                            self.set_slot(Slot::TextHi);
                            self.write_with_controls(&crate::editor::printable(&strip_colours(l)));
                            self.newline();
                            lines += 1;
                            if !self.more(&mut lines).await? {
                                break;
                            }
                        }
                        self.rule().await?;
                    }
                }
                _ => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_trash_has_left() {
        let at = |d, h| chrono::NaiveDate::from_ymd_opt(2026, 10, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
        assert_eq!(days_left(Some(at(1, 12)), 30, at(1, 13)), Some(30));
        assert_eq!(days_left(Some(at(1, 12)), 30, at(30, 13)), Some(1));
        assert_eq!(days_left(Some(at(1, 12)), 30, at(31, 11)), Some(1), "an hour to go");
        assert_eq!(days_left(Some(at(1, 12)), 30, at(31, 13)), Some(0));
        assert_eq!(days_left(Some(at(1, 12)), 3, at(20, 0)), Some(0), "overdue, going at the next emptying");
        assert_eq!(days_left(Some(at(1, 12)), 0, at(20, 0)), None, "kept for good");
        assert_eq!(days_left(None, 30, at(20, 0)), Some(30));
    }

    #[test]
    fn emptying_minds_the_days() {
        let state = crate::tests::state();
        let mut m = crate::msgbase::Message {
            id: 0,
            area: "GEN".into(),
            num: 0,
            from: Default::default(),
            to: Default::default(),
            subject: "gone".into(),
            posted: chrono::NaiveDateTime::default(),
            deleted: false,
            private: false,
            anonymous: false,
            replies: 0,
            sig_pos: 0,
            tag: 0,
            reply_to: 0,
            lines: vec!["gone".into()],
        };
        state.msgs.add(&mut m).unwrap();
        state.msgs.set_deleted(m.id, true, "phrac").unwrap();
        assert_eq!(empty(&state).unwrap(), 0, "deleted just now: still in the trash");
        assert_eq!(state.msgs.trash().unwrap().len(), 1);
        // One deleted before the trash kept dates starts its days now.
        let mut old = m.clone();
        old.num = 0;
        state.msgs.add(&mut old).unwrap();
        state.msgs.set_deleted(old.id, true, "").unwrap();
        state.msgs.date_trash(chrono::NaiveDateTime::default()).unwrap();
        assert!(state.msgs.trash().unwrap().iter().all(|t| t.at.is_some()));
    }
}
