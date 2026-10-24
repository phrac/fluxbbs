//! New, after PTT's personal boards: boards callers make and run. A caller
//! who passes `[messages] board_create_acs` makes one with `c` in the
//! message center (`boards_per_caller` each) and is its moderator: its
//! sponsor, so they may delete and edit posts and take votes off, and with
//! `o` in its thread list they change its about line and tags, pin the
//! thread under the bar, ban callers from posting and voting there, or
//! delete it. The boards go in
//! `msgareas.toml` like any area, under `board_folder` if that's set, and
//! `pack-messages` removes one that's had no posts `board_empty_days`
//! after it was made.

use std::io;

use crate::BbsState;
use crate::input::{Key, ReadOpts};
use crate::messages::{AreaRef, io_err};
use crate::msgareas::MsgArea;
use crate::msgbase::Message;
use crate::session::Session;

/// Characters in a board's name and about line.
const NAME_WIDTH: usize = 20;
const ABOUT_WIDTH: usize = 40;

impl Session {
    /// The caller may make boards (`board_create_acs`).
    pub(crate) fn may_create_board(&self) -> bool {
        let acs = self.state.config().messages.board_create_acs.clone();
        !acs.is_empty() && self.acs(&acs)
    }

    /// The caller is banned from posting and voting in `a`.
    pub(crate) fn banned_from(&self, a: &AreaRef) -> bool {
        a.area.banned_ids.contains(&self.user.id) || a.area.banned.iter().any(|h| h.eq_ignore_ascii_case(&self.user.handle))
    }

    /// `c` in the message center: make a board. Returns its number.
    pub(crate) async fn create_board(&mut self) -> io::Result<Option<u32>> {
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        if !self.may_create_board() {
            self.say_line("BoardCreateNo").await?;
            self.prompt_key().await?;
            return Ok(None);
        }
        let limit = self.state.config().messages.boards_per_caller;
        let mine = self.state.areas().areas.iter().filter(|a| !a.created.is_empty() && if a.sponsor_id != 0 { a.sponsor_id == self.user.id } else { a.sponsor.eq_ignore_ascii_case(&self.user.handle) }).count();
        if mine >= limit as usize {
            let s = self.strings().fmt("BoardLimit", &[&limit.to_string()]);
            self.show_line(&s).await?;
            self.prompt_key().await?;
            return Ok(None);
        }
        self.say_line("BoardIntro").await?;
        self.newline();
        self.say("BoardAskName").await?;
        let name = crate::editor::strip_colours(&self.read_string(ReadOpts::new(NAME_WIDTH)).await?).trim().to_string();
        self.newline();
        if name.is_empty() {
            return Ok(None);
        }
        if self.state.areas().areas.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
            self.say_line("BoardNameTaken").await?;
            self.prompt_key().await?;
            return Ok(None);
        }
        self.say("BoardAskAbout").await?;
        let about = crate::editor::strip_colours(&self.read_string(ReadOpts::new(ABOUT_WIDTH)).await?).trim().to_string();
        self.newline();
        let s = self.strings().fmt("BoardAskSure", &[&name]);
        self.show(&s).await?;
        if !self.yes_no(true).await? {
            return Ok(None);
        }
        let base: String = name.chars().filter(char::is_ascii_alphanumeric).take(6).collect::<String>().to_ascii_uppercase();
        let base = if base.is_empty() { "BOARD".to_string() } else { base };
        let folder = self.state.config().messages.board_folder.clone();
        let ftn = self.state.config().ftn.areas.clone();
        let (state, sponsor, sponsor_id) = (self.state.clone(), self.user.handle.clone(), self.user.id);
        let msgs = &state.msgs;
        let made = state
            .update_areas(|areas| -> anyhow::Result<Option<(u32, String)>> {
                // Someone else may have taken the name meanwhile.
                if areas.areas.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
                    return Ok(None);
                }
                // Not a key an old area's messages (deleted, messages kept)
                // or an echo are still under: the board would inherit them.
                let taken = |k: &str| -> anyhow::Result<bool> {
                    Ok(areas.areas.iter().any(|a| a.file.eq_ignore_ascii_case(k))
                        || areas.email.file.eq_ignore_ascii_case(k)
                        || ftn.iter().any(|m| m.area.eq_ignore_ascii_case(k))
                        || msgs.count(k)? > 0)
                };
                // A database error stops it (the list is locked meanwhile).
                let mut file = None;
                for n in 1..1000 {
                    let k = if n == 1 { base.clone() } else { format!("{base}{n}") };
                    match taken(&k) {
                        Ok(false) => {
                            file = Some(k);
                            break;
                        }
                        Ok(true) => {}
                        Err(e) => return Err(e),
                    }
                }
                let Some(file) = file else { return Ok(None) };
                let parent = areas.areas.iter().find(|a| !folder.is_empty() && a.file.eq_ignore_ascii_case(&folder)).map(|a| a.file.clone()).unwrap_or_default();
                areas.areas.push(MsgArea {
                    about,
                    parent,
                    sponsor,
                    sponsor_id,
                    created: crate::clock::now().date_naive().format("%Y-%m-%d").to_string(),
                    private: false,
                    anonymous: false,
                    ..MsgArea::new(&name, &file)
                });
                Ok(Some((areas.areas.len() as u32, file)))
            })
            .map_err(io_err)?
            .map_err(io_err)?;
        let Some((n, file)) = made else {
            self.say_line("BoardNameTaken").await?;
            self.prompt_key().await?;
            return Ok(None);
        };
        tracing::info!("{}: made the board {name} ({file})", self.user.handle);
        let s = self.strings().fmt("BoardMade", &[&name]);
        self.show_line(&s).await?;
        self.prompt_key().await?;
        Ok(Some(n))
    }

    /// `o` in a board's thread list, for its moderator (or a co-SysOp):
    /// its about line, tags, bans, pinning `thread` (the one under the
    /// bar), or deleting it. True if it was deleted.
    pub(crate) async fn moderate_board(&mut self, a: &AreaRef, thread: Option<Message>) -> io::Result<bool> {
        let key = a.key().to_string();
        loop {
            let Some(area) = self.state.areas().areas.iter().find(|x| x.file.eq_ignore_ascii_case(&key)).cloned() else {
                return Ok(true);
            };
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.clear_screen();
            let s = self.strings().fmt("BoardModHead", &[&area.name]);
            self.show_line(&s).await?;
            self.newline();
            let none = self.strings().get("BoardNone").to_string();
            let about = if area.about.is_empty() { "-".to_string() } else { area.about.clone() };
            // Those banned by number under their handles now; older bans by
            // the handle they had.
            let mut names: Vec<String> = area.banned_ids.iter().filter_map(|&id| self.state.users.get(id).ok().flatten().map(|u| u.handle)).collect();
            for h in &area.banned {
                if !names.iter().any(|n| n.eq_ignore_ascii_case(h)) && self.state.users.find_handle(h).ok().flatten().is_none_or(|u| !area.banned_ids.contains(&u.id)) {
                    names.push(h.clone());
                }
            }
            let banned = if names.is_empty() { none } else { names.join(", ") };
            let s = self.strings().fmt("BoardModAbout", &[&fluxbbs_term::lit(&about)]);
            self.show_line(&s).await?;
            let tags = if area.tags.is_empty() { "-".to_string() } else { area.tags.join(", ") };
            let s = self.strings().fmt("BoardModTags", &[&fluxbbs_term::lit(&tags)]);
            self.show_line(&s).await?;
            let s = self.strings().fmt("BoardModBanned", &[&fluxbbs_term::lit(&banned)]);
            self.show_line(&s).await?;
            self.newline();
            let pinned = match &thread {
                Some(m) => Some(self.state.msgs.pinned(&area.file).map_err(io_err)?.contains(&m.id)),
                None => None,
            };
            if let (Some(m), Some(on)) = (&thread, pinned) {
                let verb = self.strings().get(if on { "BoardUnpin" } else { "BoardPin" }).to_string();
                let s = self.strings().fmt("BoardModPin", &[&fluxbbs_term::lit(&fluxbbs_term::codes::squish(&m.subject, 50)), &verb]);
                self.show_line(&s).await?;
            }
            // Only callers' boards are theirs to delete.
            let can_delete = !area.created.is_empty();
            self.say(if can_delete { "BoardModAskDelete" } else { "BoardModAsk" }).await?;
            let k = self.read_key().await?;
            self.newline();
            self.newline();
            // Changes go to the board as it is when they're made (found by
            // its key: others may have changed the list meanwhile).
            let state = self.state.clone();
            let edit = |f: &dyn Fn(&mut MsgArea)| {
                state.update_areas(|areas| {
                    if let Some(x) = areas.areas.iter_mut().find(|x| x.file.eq_ignore_ascii_case(&key)) {
                        f(x);
                    }
                })
            };
            match k.upper() {
                Some('A') => {
                    self.say("BoardAskAbout").await?;
                    let v = self.read_string(ReadOpts::new(ABOUT_WIDTH).prefill(area.about.clone())).await?;
                    let about = crate::editor::strip_colours(&v).trim().to_string();
                    edit(&|x| x.about = about.clone()).map_err(io_err)?;
                }
                Some('T') => {
                    self.say("BoardAskTags").await?;
                    let v = self.read_string(ReadOpts::new(60).prefill(area.tags.join(", "))).await?;
                    let tags = crate::msgcenter::parse_tags(&crate::editor::strip_colours(&v));
                    edit(&|x| x.tags = tags.clone()).map_err(io_err)?;
                }
                Some('P') if thread.is_some() => {
                    if let (Some(m), Some(on)) = (&thread, pinned) {
                        self.state.msgs.set_pinned(m.id, !on).map_err(io_err)?;
                        tracing::info!("{}: {} \"{}\" in {}", self.user.handle, if on { "unpinned" } else { "pinned" }, m.subject, area.name);
                    }
                }
                Some(c @ ('B' | 'U')) => {
                    self.say(if c == 'B' { "BoardBanWho" } else { "BoardUnbanWho" }).await?;
                    let mut opts = ReadOpts::new(36);
                    opts.complete_users = true;
                    let who = self.read_string(opts).await?;
                    self.newline();
                    let who = who.trim().to_string();
                    if who.is_empty() {
                    } else if c == 'U' {
                        // By the handle shown: whoever has it now, and the
                        // handle as it was banned.
                        let id = self.state.users.find_handle(&who).map_err(io_err)?.map(|u| u.id);
                        edit(&|x| {
                            x.banned.retain(|h| !h.eq_ignore_ascii_case(&who));
                            x.banned_ids.retain(|i| Some(*i) != id);
                        })
                        .map_err(io_err)?;
                    } else {
                        match self.state.users.find_handle(&who).map_err(io_err)? {
                            Some(u) if u.id == self.user.id || if area.sponsor_id != 0 { u.id == area.sponsor_id } else { u.handle.eq_ignore_ascii_case(&area.sponsor) } => {
                                self.say_line("BoardBanSelf").await?;
                                self.prompt_key().await?;
                            }
                            Some(u) => {
                                tracing::info!("{}: banned {} from {}", self.user.handle, u.handle, area.name);
                                edit(&|x| {
                                    if !x.banned.iter().any(|h| h.eq_ignore_ascii_case(&u.handle)) {
                                        x.banned.push(u.handle.clone());
                                    }
                                    if !x.banned_ids.contains(&u.id) {
                                        x.banned_ids.push(u.id);
                                    }
                                })
                                .map_err(io_err)?;
                            }
                            None => {
                                self.say_line("FingerUnknown").await?;
                                self.prompt_key().await?;
                            }
                        }
                    }
                }
                Some('D') if can_delete => {
                    let posts = self.state.msgs.count(&area.file).map_err(io_err)?;
                    let s = self.strings().fmt("BoardDeleteSure", &[&area.name, &posts.to_string()]);
                    self.show(&s).await?;
                    if !self.yes_no(false).await? {
                        continue;
                    }
                    self.state.update_areas(|areas| areas.areas.retain(|x| !x.file.eq_ignore_ascii_case(&key))).map_err(io_err)?;
                    self.state.msgs.delete_area(&area.file).map_err(io_err)?;
                    crate::attach::remove_orphans(&self.state).map_err(io_err)?;
                    tracing::info!("{}: deleted the board {} ({posts} posts)", self.user.handle, area.name);
                    return Ok(true);
                }
                _ if k == Key::Esc || k.upper().is_some() || k == Key::Enter => return Ok(false),
                _ => {}
            }
        }
    }
}

/// `pack-messages`: callers' boards with no posts `board_empty_days` after
/// they were made go. Returns how many.
pub fn remove_empty_boards(state: &BbsState) -> anyhow::Result<usize> {
    let days = state.config().messages.board_empty_days;
    if days == 0 {
        return Ok(0);
    }
    let today = crate::clock::now().date_naive();
    let areas = state.areas();
    let mut gone: Vec<String> = Vec::new();
    for a in &areas.areas {
        let Ok(made) = chrono::NaiveDate::parse_from_str(&a.created, "%Y-%m-%d") else { continue };
        let parent_of_some = areas.areas.iter().any(|o| o.parent.eq_ignore_ascii_case(&a.file));
        if (today - made).num_days() >= days as i64 && !parent_of_some && state.msgs.count(&a.file)? == 0 {
            gone.push(a.file.clone());
        }
    }
    if gone.is_empty() {
        return Ok(0);
    }
    // Looked at again as they go: a post made meanwhile keeps a board.
    let gone: Vec<String> = state.update_areas(|areas| {
        let mut went = Vec::new();
        areas.areas.retain(|a| {
            let go = gone.contains(&a.file) && state.msgs.count(&a.file).is_ok_and(|n| n == 0);
            if go {
                went.push(a.file.clone());
            }
            !go
        });
        went
    })?;
    for file in &gone {
        state.msgs.delete_area(file)?;
        tracing::info!("event: removed the empty board {file}");
    }
    Ok(gone.len())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::msgareas::MsgAreas;

    #[test]
    fn empty_boards_go_after_a_while() {
        let root = std::env::temp_dir().join(format!("inq-boards-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let state = crate::tests::state();
        let mut cfg = (*state.config()).clone();
        cfg.root = root.clone();
        *state.config.write().unwrap() = Arc::new(cfg);
        let today = crate::clock::now().date_naive();
        let ago = |d: i64| (today - chrono::Duration::days(d)).format("%Y-%m-%d").to_string();
        let board = |name: &str, file: &str, made: String| MsgArea { created: made, sponsor: "kingpin".into(), ..MsgArea::new(name, file) };
        let areas = MsgAreas {
            areas: vec![
                MsgArea::new("General", "GEN"),
                board("Old", "OLD", ago(40)),
                board("New", "NEW", ago(2)),
                board("Busy", "BUSY", ago(40)),
            ],
            ..MsgAreas::default()
        };
        state.set_areas(areas).unwrap();
        let mut m = crate::msgbase::Message {
            id: 0,
            area: "BUSY".into(),
            num: 0,
            from: Default::default(),
            to: Default::default(),
            subject: "hi".into(),
            posted: chrono::NaiveDateTime::default(),
            deleted: false,
            private: false,
            anonymous: false,
            replies: 0,
            sig_pos: 0,
            tag: 0,
            reply_to: 0,
            lines: vec!["hi".into()],
        };
        state.msgs.add(&mut m).unwrap();
        assert_eq!(remove_empty_boards(&state).unwrap(), 1);
        let left: Vec<String> = state.areas().areas.iter().map(|a| a.file.clone()).collect();
        assert_eq!(left, ["GEN", "NEW", "BUSY"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
