//! New, after PTT's personal boards: boards callers make and run. A caller
//! who passes `[messages] board_create_acs` makes one with `c` in the
//! message center (`boards_per_caller` each) and is its moderator: its
//! sponsor, so they may delete and edit posts and take votes off, and with
//! `o` in its thread list they change its about line, ban callers from
//! posting and voting there, or delete it. The boards go in
//! `msgareas.toml` like any area, under `board_folder` if that's set, and
//! `pack-messages` removes one that's had no posts `board_empty_days`
//! after it was made.

use std::io;

use crate::BbsState;
use crate::input::{Key, ReadOpts};
use crate::messages::{AreaRef, io_err};
use crate::msgareas::MsgArea;
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
        a.area.banned.iter().any(|h| h.eq_ignore_ascii_case(&self.user.handle))
    }

    /// `c` in the message center: make a board. Returns its number.
    pub(crate) async fn create_board(&mut self) -> io::Result<Option<u32>> {
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        if !self.may_create_board() {
            self.say_line("BoardCreateNo").await?;
            self.prompt_key().await?;
            return Ok(None);
        }
        let limit = self.state.config().messages.boards_per_caller;
        let mine = self.state.areas().areas.iter().filter(|a| !a.created.is_empty() && a.sponsor.eq_ignore_ascii_case(&self.user.handle)).count();
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
        let mut areas = (*self.state.areas()).clone();
        let base: String = name.chars().filter(char::is_ascii_alphanumeric).take(6).collect::<String>().to_ascii_uppercase();
        let base = if base.is_empty() { "BOARD".to_string() } else { base };
        let file = crate::sysop::unused_key(areas.areas.iter().map(|a| a.file.as_str()).chain(std::iter::once(areas.email.file.as_str())), &base);
        let folder = self.state.config().messages.board_folder.clone();
        let parent = areas.areas.iter().find(|a| !folder.is_empty() && a.file.eq_ignore_ascii_case(&folder)).map(|a| a.file.clone()).unwrap_or_default();
        areas.areas.push(MsgArea {
            about,
            parent,
            sponsor: self.user.handle.clone(),
            created: crate::clock::now().date_naive().format("%Y-%m-%d").to_string(),
            private: false,
            anonymous: false,
            ..MsgArea::new(&name, &file)
        });
        let n = areas.areas.len() as u32;
        self.state.set_areas(areas).map_err(io_err)?;
        tracing::info!("{}: made the board {name} ({file})", self.user.handle);
        let s = self.strings().fmt("BoardMade", &[&name]);
        self.show_line(&s).await?;
        self.prompt_key().await?;
        Ok(Some(n))
    }

    /// `o` in a board's thread list, for its moderator (or a co-SysOp):
    /// its about line, bans, or deleting it. True if it was deleted.
    pub(crate) async fn moderate_board(&mut self, a: &AreaRef) -> io::Result<bool> {
        let key = a.key().to_string();
        loop {
            let Some((i, area)) = self.state.areas().areas.iter().enumerate().find(|(_, x)| x.file.eq_ignore_ascii_case(&key)).map(|(i, x)| (i, x.clone())) else {
                return Ok(true);
            };
            self.screen.set_color(iniquity_term::Color::DEFAULT);
            self.clear_screen();
            let s = self.strings().fmt("BoardModHead", &[&area.name]);
            self.show_line(&s).await?;
            self.newline();
            let none = self.strings().get("BoardNone").to_string();
            let about = if area.about.is_empty() { "-".to_string() } else { area.about.clone() };
            let banned = if area.banned.is_empty() { none } else { area.banned.join(", ") };
            let s = self.strings().fmt("BoardModAbout", &[&iniquity_term::lit(&about)]);
            self.show_line(&s).await?;
            let s = self.strings().fmt("BoardModBanned", &[&iniquity_term::lit(&banned)]);
            self.show_line(&s).await?;
            self.newline();
            // Only callers' boards are theirs to delete.
            let can_delete = !area.created.is_empty();
            self.say(if can_delete { "BoardModAskDelete" } else { "BoardModAsk" }).await?;
            let k = self.read_key().await?;
            self.newline();
            self.newline();
            let mut areas = (*self.state.areas()).clone();
            let changed = match k.upper() {
                Some('A') => {
                    self.say("BoardAskAbout").await?;
                    let v = self.read_string(ReadOpts::new(ABOUT_WIDTH).prefill(area.about.clone())).await?;
                    areas.areas[i].about = crate::editor::strip_colours(&v).trim().to_string();
                    true
                }
                Some(c @ ('B' | 'U')) => {
                    self.say(if c == 'B' { "BoardBanWho" } else { "BoardUnbanWho" }).await?;
                    let mut opts = ReadOpts::new(36);
                    opts.complete_users = true;
                    let who = self.read_string(opts).await?;
                    self.newline();
                    let who = who.trim();
                    let list = &mut areas.areas[i].banned;
                    if who.is_empty() {
                        false
                    } else if c == 'U' {
                        let before = list.len();
                        list.retain(|h| !h.eq_ignore_ascii_case(who));
                        list.len() != before
                    } else {
                        match self.state.users.find(who, false).map_err(io_err)? {
                            Some(u) if u.id == self.user.id || u.handle.eq_ignore_ascii_case(&area.sponsor) => {
                                self.say_line("BoardBanSelf").await?;
                                self.prompt_key().await?;
                                false
                            }
                            Some(u) if !list.iter().any(|h| h.eq_ignore_ascii_case(&u.handle)) => {
                                tracing::info!("{}: banned {} from {}", self.user.handle, u.handle, area.name);
                                list.push(u.handle);
                                true
                            }
                            Some(_) => false,
                            None => {
                                self.say_line("FingerUnknown").await?;
                                self.prompt_key().await?;
                                false
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
                    areas.areas.remove(i);
                    self.state.set_areas(areas).map_err(io_err)?;
                    self.state.msgs.delete_area(&area.file).map_err(io_err)?;
                    crate::attach::remove_orphans(&self.state).map_err(io_err)?;
                    tracing::info!("{}: deleted the board {} ({posts} posts)", self.user.handle, area.name);
                    return Ok(true);
                }
                _ if k == Key::Esc || k.upper().is_some() || k == Key::Enter => return Ok(false),
                _ => false,
            };
            if changed {
                self.state.set_areas(areas).map_err(io_err)?;
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
    let mut areas = (*state.areas()).clone();
    let mut gone = Vec::new();
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
    areas.areas.retain(|a| !gone.contains(&a.file));
    state.set_areas(areas)?;
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
