//! Menu commands (`MenuCmd.pas`). Each command is two characters plus a
//! parameter; see section 5.4 of the Iniquity manual.

use std::io;

use fluxbbs_term::{Slot, lit};
use fluxbbs_term::codes::resize;

use crate::input::Key;
use crate::session::Session;

/// Commands the original had that aren't ported yet. They report that,
/// rather than "invalid command", so stock menus degrade honestly.
const NOT_YET: &[&str] = &[
    "TP", "TS",
];

impl Session {
    /// `menuCommand`: run one command.
    pub async fn run_command(&mut self, code: &str, param: &str) -> io::Result<()> {
        let code = code.to_uppercase();
        let semis: Vec<&str> = param.split(';').collect();
        let semi = |i: usize| semis.get(i).copied().unwrap_or("");
        let num = |s: &str| s.trim().parse::<u16>().unwrap_or(0);
        let mut known = true;
        self.menu.reload = false;

        match code.as_str() {
            // -- input string and OK flag
            "?=" => self.input_string = param.into(),
            "?+" => self.input_string.push_str(param),
            "?/" => self.input_string.insert_str(0, param),
            "?%" => self.input_string = self.expand_mci(&self.input_string.clone(), num(param) as usize),
            "?E" => self.ok_flag = self.text_file_exists(param),
            "?F" => self.ok_flag = param.trim_start().starts_with(['Y', 'y', 'T', 't']),
            "?L" => self.input_string = self.minutes_left().to_string(),
            "?N" | "?Y" => {
                if !param.is_empty() {
                    self.show(param).await?;
                }
                self.ok_flag = self.yes_no(code == "?Y").await?;
            }
            "?S" => self.ok_flag = param == self.input_string,
            "?U" => self.ok_flag = param.eq_ignore_ascii_case(&self.input_string),
            "?X" => self.input_string = "0".into(),
            "?B" => self.x_prompt(&semis).await?,
            "?I" => self.menu_input(&semis).await?,
            "?T" => self.menu_append(&semis)?,

            // -- menus and display
            "-^" | "-/" | "-\\" => self.menu_change(code.chars().nth(1).unwrap(), param),
            "-<" | "->" | "-Z" => {} // remote I/O toggles and buffer drain: nothing to do
            "-=" => {
                let key = param.trim().chars().next().unwrap_or('A').to_ascii_uppercase();
                if let Some(level) = self.state.level(key) {
                    self.user.apply_level(key, &level);
                    self.save_user();
                }
            }
            "-?" => self.menu_cmd_help().await?,
            "-C" => self.clear_screen(),
            "-D" => self.delay(num(param) as u64 * 1000).await?,
            "-E" => self.show(param).await?,
            "-F" => {
                self.show_file(param).await?;
            }
            "-K" => {
                self.show(param).await?;
                self.any_key().await?;
            }
            "-L" => self.show_line(param).await?,
            "-P" => self.prompt_key().await?,
            "-R" => {
                for _ in 0..num(param).max(1) {
                    self.newline();
                }
            }
            "-S" => {
                let mut on = true;
                for c in param.to_uppercase().chars() {
                    match c {
                        '+' => on = true,
                        '-' => on = false,
                        'A'..='Z' if on => {
                            self.user.flags.insert(c);
                        }
                        'A'..='Z' => {
                            self.user.flags.remove(&c);
                        }
                        _ => {}
                    }
                }
            }
            "-W" => tracing::info!("{}: {param}", self.user.handle),

            // -- cursor
            "|B" => self.screen.move_down(25),
            "|C" => self.screen.goto(1, 1),
            "|D" => self.screen.move_down(num(semi(0)).max(1)),
            "|E" => self.screen.move_right(80),
            "|H" => self.screen.move_left(80),
            "|L" => self.screen.move_left(num(semi(0)).max(1)),
            "|M" => self.screen.goto(num(semi(0)), num(semi(1))),
            "|R" => self.screen.move_right(num(semi(0)).max(1)),
            "|U" => self.screen.move_up(num(semi(0)).max(1)),
            "|T" => self.screen.move_up(25),
            "|Z" => self.screen.clear_eol(),

            // -- matrix logon
            ">A" => self.apply().await?,
            ">H" => self.hangup = true,
            ">C" => self.page_sysop(true).await?,
            "CP" => self.page_sysop(false).await?,
            ">L" => self.logon_system().await?,

            // -- logoff
            "HC" => {
                self.show(param).await?;
                if self.yes_no(true).await? {
                    self.clear_screen();
                    self.show_file("LOGOFF").await?;
                    self.hangup = true;
                }
            }
            "HI" => {
                if !param.is_empty() {
                    self.show_line(param).await?;
                }
                self.hangup = true;
            }
            "HM" => {
                self.show(param).await?;
                self.show_file("LOGOFF").await?;
                self.hangup = true;
            }

            // -- messages
            "MA" => self.list_areas(false).await?,
            "MC" => {
                let ok = match param.trim().parse::<u32>() {
                    Ok(n) => self.change_area(n).await?,
                    Err(_) => false,
                };
                if !ok {
                    self.list_areas(true).await?;
                }
            }
            "M#" => self.jump_area(true, param).await?,
            "M[" => self.step_area(false).await?,
            "M]" => self.step_area(true).await?,
            "MJ" => self.select_conf(true, param).await?,
            "MZ" => self.list_confs(true).await?,
            "MN" => self.newscan_cmd(param).await?,
            "MM" => self.mandatory_scan(param).await?,
            "MP" => {
                if let Some(a) = self.current_area() {
                    self.post(&a, semi(0), semi(1), None).await?;
                }
            }
            "MR" => {
                if let Some(a) = self.current_area() {
                    let mandatory = param.to_ascii_uppercase().contains('M');
                    self.read_messages(&a, 0, mandatory, true).await?;
                }
            }
            "MS" => self.set_pointer_date().await?,
            "ME" => self.scan_select(true).await?,
            "MO" => self.msg_search(param).await?,
            "MH" => {
                self.msg_center().await?;
                self.full_screen_done();
            }
            // These end in the reader or a list the caller has left.
            "MX" => {
                self.msg_jump(param).await?;
                self.full_screen_done();
            }
            "M@" => {
                self.read_mentions(param).await?;
                self.full_screen_done();
            }
            "MW" if param.eq_ignore_ascii_case("read") => {
                self.read_all_new().await?;
                self.full_screen_done();
            }
            "MW" => self.logon_new_check().await?,

            // -- files
            "FA" => self.list_fareas(false, None).await?,
            "FC" => {
                let ok = match param.trim().parse::<u32>() {
                    Ok(n) => self.change_farea(n).await?,
                    Err(_) => false,
                };
                if !ok {
                    self.list_fareas(true, None).await?;
                }
            }
            "F#" => self.jump_area(false, param).await?,
            "F[" => self.step_farea(false).await?,
            "F]" => self.step_farea(true).await?,
            "FJ" => self.select_conf(false, param).await?,
            "FZ" => self.list_confs(false).await?,
            "FL" => self.list_files_cmd(param).await?,
            "FN" => self.file_newscan_cmd(param).await?,
            "FS" => self.set_file_scan().await?,
            "FE" => self.scan_select(false).await?,
            "FD" => self.download(param).await?,
            "F+" => self.flag_file(param).await?,
            "FB" => self.batch_editor().await?,
            "FI" => self.file_info_cmd(param).await?,
            "AI" => self.ai_chat().await?,
            "AT" => self.tarot(param).await?,
            "AL" => self.slots(param).await?,
            "AC" => self.casino(param).await?,
            "MF" => self.top_fillers().await?,
            "FW" => self.file_browser(param).await?,
            "FV" => self.view_archive(param).await?,
            "FF" => self.search_files(true).await?,
            "FT" => self.search_files(false).await?,
            "FU" => self.upload().await?,
            "*%" if self.is_sysop() => self.local_upload().await?,

            // -- email
            "EE" => self.enter_email(semi(0), semi(1)).await?,
            "E+" => self.use_tag = true,
            "E-" => self.use_tag = false,
            "E/" => self.email_tag = num(semi(0)),
            "E\\" => self.read_tag = num(semi(0)),
            "ER" => self.read_email(false, false).await?,
            "EC" => self.read_email(false, true).await?,
            "*R" if self.is_sysop() => self.read_email(true, false).await?,
            "*F" if self.is_sysop() => self.file_area_editor().await?,
            "*S" if self.is_sysop() => self.sort_file_areas(param).await?,
            "*Y" if self.is_sysop() => self.file_editor(param).await?,
            "*U" if self.is_sysop() => self.user_editor(param).await?,
            // New: private notes on a caller.
            "*N" if self.is_sysop() => self.user_notes_cmd(param).await?,
            "*@" if self.is_sysop() => self.conference_editor(true).await?,
            "*!" if self.is_sysop() => self.conference_editor(false).await?,
            "*>" if self.is_sysop() => self.download_any(param).await?,
            "*X" if self.is_sysop() => self.protocol_editor().await?,
            "*Z" if self.is_sysop() => self.archiver_editor().await?,
            "*A" if self.is_sysop() => self.msg_area_editor().await?,
            "*K" if self.is_sysop() => self.pack_file_areas(param).await?,
            "*V" if self.is_sysop() => self.event_editor().await?,
            "*O" if self.is_sysop() => self.logon_editor().await?,
            "*G" if self.is_sysop() => self.security_view().await?,
            "*L" if self.is_sysop() => self.level_editor().await?,
            "*B" if self.is_sysop() => self.bbs_list_editor().await?,
            "*H" if self.is_sysop() => self.stat_editor().await?,
            "*E" if self.is_sysop() => self.string_editor().await?,
            "*#" if self.is_sysop() => self.menu_editor().await?,
            "*C" if self.is_sysop() => self.system_config().await?,
            "*I" if self.is_sysop() => self.infoform_editor().await?,
            "*T" if self.is_sysop() => self.theme_editor().await?,
            "AG" => self.art_gallery().await?,
            "UT" => self.choose_theme().await?,
            "UK" => self.ssh_keys_edit().await?,
            "UF" => self.finger(param).await?,
            "-X" => Box::pin(self.script_command(param)).await?,
            "!D" => self.bank_transaction(crate::timebank::Unit::Minutes, true).await?,
            "!W" => self.bank_transaction(crate::timebank::Unit::Minutes, false).await?,
            "!E" => self.bank_transaction(crate::timebank::Unit::Kb, true).await?,
            "!I" => self.bank_transaction(crate::timebank::Unit::Kb, false).await?,
            "!V" => self.bank_stats().await?,
            "QD" => self.qwk_download().await?,
            "QU" => self.qwk_upload().await?,
            "D0" | "D1" | "D2" | "D3" | "D4" | "D7" | "DZ" => self.door_execute(param, code.chars().nth(1).unwrap()).await?,
            // New: a BBSLink door, by its code.
            "DB" => self.bbslink_door(param).await?,
            "IN" => self.infoforms(false).await?,
            "IM" => self.infoforms(true).await?,
            "NV" => self.nuv_vote(param.to_ascii_uppercase().contains('S')).await?,

            // -- automessage, BBS list, callers
            "AS" => self.show_automsg().await?,
            "AW" => self.write_automsg().await?,
            "AR" => self.reply_automsg().await?,
            "BA" => self.bbs_add().await?,
            "BL" => {
                self.bbs_list().await?;
            }
            "BS" => self.bbs_show(param).await?,
            "BD" => self.bbs_delete(param).await?,
            "BC" => self.bbs_clear(param).await?,
            "LC" => self.last_callers(param).await?,
            "LH" => self.show_history().await?,
            "UC" => self.user_config().await?,
            "SD" => self.delete_autosig().await?,
            "SE" => self.edit_autosig().await?,
            "SS" => self.show_autosig().await?,
            "SU" => self.upload_autosig().await?,

            // -- users and nodes
            "UL" => self.user_list().await?,
            "UP" => self.change_password().await?,
            "US" => self.scan_all_on().await?,
            "VV" => self.vote_all().await?,
            "VA" => self.vote_one().await?,
            "VR" => self.vote_results().await?,
            "VN" => self.vote_add().await?,
            "VD" => self.vote_delete().await?,
            "XL" => self.node_list().await?,
            "XM" => self.node_message().await?,
            "XB" => self.node_broadcast().await?,
            "XC" => self.node_chat().await?,
            "XI" => self.invisible(param).await?,
            "XS" => self.node_status(param),

            _ if NOT_YET.contains(&code.as_str()) || code.starts_with('*') => {
                self.newline();
                self.show_line(&format!("|U4That feature isn't available yet |U5(|U6{code}|U5)|U4.")).await?;
                tracing::debug!("unported command {code} {param:?}");
            }
            _ => known = false,
        }

        self.menu.last_cmd_good = known;
        if !known {
            self.newline();
            self.set_slot(Slot::Error);
            let s = self.strings().fmt("InvalidMnuCmd", &[&code]);
            self.show_line(&s).await?;
        }
        if self.menu.reload && !self.hangup {
            self.menu.reload = false;
            self.menu_install().await?;
            self.menu.last_cmd_good = false;
            self.queue_first_cmd();
        }
        Ok(())
    }

    /// `?B` (`iXprompt`): a horizontal lightbar of choices. The parameter
    /// is `choice;choice;...;default`; the pick goes in the input string.
    pub(crate) async fn x_prompt(&mut self, semis: &[&str]) -> io::Result<()> {
        self.x_pick(semis).await?;
        if !self.input_string.is_empty() {
            self.newline();
        }
        Ok(())
    }

    /// [`Session::x_prompt`] without the line break after it.
    pub(crate) async fn x_pick(&mut self, semis: &[&str]) -> io::Result<()> {
        self.input_string.clear();
        let Some((def, opts)) = semis.split_last() else { return Ok(()) };
        let def: usize = def.trim().parse().unwrap_or(0);
        if opts.is_empty() || def == 0 || def > opts.len() {
            return Ok(());
        }
        // New: in plain text, the choices in words, and a line naming one
        // (or its first letters); Enter for the default.
        if self.plain() {
            loop {
                self.write(&format!(" ({}) [{}] ", opts.join("/"), opts[def - 1]));
                let s = self.read_string(crate::input::ReadOpts::new(20)).await?;
                self.newline();
                let s = s.trim().to_lowercase();
                let pick = if s.is_empty() { Some(opts[def - 1]) } else { opts.iter().copied().find(|o| o.to_lowercase().starts_with(&s)) };
                if let Some(o) = pick {
                    self.input_string = o.to_string();
                    return Ok(());
                }
            }
        }
        let mut xs = Vec::new();
        self.set_slot(Slot::Item);
        for o in opts {
            xs.push(self.screen.where_x());
            self.show(&format!(" {o} ")).await?;
        }
        let mut cur = def - 1;
        let draw = |s: &mut Self, i: usize, hi: bool| {
            s.screen.pos_x(xs[i]);
            s.set_slot(if hi { Slot::ItemSel } else { Slot::Item });
            s.write_with_controls(&format!(" {} ", opts[i]));
        };
        draw(self, cur, true);
        loop {
            let key = self.read_key().await?;
            let next = match key {
                Key::Right | Key::Char(b'6' | b' ') => (cur + 1) % opts.len(),
                Key::Left | Key::Char(b'4') => (cur + opts.len() - 1) % opts.len(),
                Key::Enter => break,
                _ => cur,
            };
            if next != cur {
                draw(self, cur, false);
                cur = next;
                draw(self, cur, true);
            }
        }
        self.set_slot(Slot::Info);
        self.input_string = opts[cur].to_string();
        Ok(())
    }

    /// `mciProcess`: replace `%XX` codes that have values, leaving all
    /// other text (colour codes included) as it is. `limit` > 0 cuts each
    /// value to that length.
    pub(crate) fn expand_mci(&self, s: &str, limit: usize) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        let at = |i: usize, z: char| chars.get(i) == Some(&'|') && chars.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&'Z')) && chars.get(i + 2).is_some_and(|c| c.eq_ignore_ascii_case(&z));
        while i < chars.len() {
            // `|ZA`...`|ZB` (what a caller typed, `defuse`d) stays as it is.
            if at(i, 'A') {
                let end = (i + 3..chars.len()).find(|&j| at(j, 'B')).map_or(chars.len(), |j| j + 3);
                out.extend(&chars[i..end]);
                i = end;
                continue;
            }
            if chars[i] == '%' && i + 2 < chars.len() {
                let code = [chars[i + 1] as u8, chars[i + 2] as u8];
                if chars[i + 1].is_ascii() && chars[i + 2].is_ascii()
                    && let Some(v) = self.mci_value(code) {
                        // Defused: a value (a handle's initials can be
                        // `|CS`) mustn't run codes where the text ends up,
                        // a screen file say.
                        let v = if limit > 0 { fluxbbs_term::codes::squish(&v, limit) } else { v };
                        out.push_str(&fluxbbs_term::defuse(&v));
                        i += 3;
                        continue;
                    }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    pub(crate) fn save_user(&self) {
        if self.user.id != 0
            && let Err(e) = self.save_own_user() {
                tracing::error!("saving {}: {e}", self.user.handle);
            }
    }

    /// New: save the caller's record, writing only what this session
    /// changed since it last loaded or saved it, onto the record as it is
    /// now; so a SysOp's edit meanwhile (their daily time, say) stays.
    pub(crate) fn save_own_user(&self) -> anyhow::Result<()> {
        let base = self.user_base.borrow().clone().filter(|b| b.id == self.user.id);
        match base {
            Some(base) => {
                self.state.users.merge(&base, &self.user)?;
            }
            None => self.state.users.save(&self.user)?,
        }
        *self.user_base.borrow_mut() = Some(self.user.clone());
        Ok(())
    }

    /// New: save someone else's record, changed from `original` (the
    /// SysOp's editors, votes), and if they're online tell their session.
    /// The record is read again and only what changed from `original`
    /// goes in, so whatever the caller (or anyone) saved meanwhile stays.
    /// Returns the record as saved.
    pub(crate) fn save_other_user(&self, original: &crate::user::User, u: &crate::user::User) -> anyhow::Result<crate::user::User> {
        self.save_other(original, u, false)
    }

    /// [`Self::save_other_user`] where what changed is a value set, not a
    /// change made (the user editor, a level's limits): counters too.
    pub(crate) fn save_other_user_exact(&self, original: &crate::user::User, u: &crate::user::User) -> anyhow::Result<crate::user::User> {
        self.save_other(original, u, true)
    }

    fn save_other(&self, original: &crate::user::User, u: &crate::user::User, exact: bool) -> anyhow::Result<crate::user::User> {
        let now = self.state.users.merge_with(original, u, exact)?;
        if u.id != self.user.id {
            // Every node they're on (by either name, if it changed).
            for (n, info) in self.state.nodes.online() {
                if info.user.eq_ignore_ascii_case(&original.handle) || info.user.eq_ignore_ascii_case(&u.handle) {
                    let ev = crate::nodes::NodeEvent::AccountChanged { original: Box::new(original.clone()), edited: Box::new(u.clone()) };
                    self.state.nodes.send(n, ev);
                }
            }
        }
        Ok(now)
    }

    /// New: read our record again after someone else changed it (keeping
    /// what this session changed and hasn't saved). Returns what changed,
    /// in words, for each change a caller would notice.
    pub(crate) fn absorb_account_changes(&mut self) -> Vec<String> {
        use crate::nodes::NodeEvent;
        let mut told = Vec::new();
        // Only this caller's (in the WFC mailbox view, the user shown is
        // someone else: the SysOp's own wait their turn).
        let me = self.user.id;
        while let Some(NodeEvent::AccountChanged { original, edited }) = self.node.take_if(|e| matches!(e, NodeEvent::AccountChanged { edited, .. } if edited.id == me)) {
            let db = match self.state.users.get(self.user.id) {
                Ok(Some(db)) => db,
                _ => continue,
            };
            let base = self.user_base.borrow().clone().filter(|b| b.id == self.user.id).unwrap_or_else(|| (*original).clone());
            let mine = self.user.clone();
            let mut now = db.clone();
            now.merge_edit(&base, &mine);
            self.user = now;
            // Who they are to other nodes (a SysOp gets pages) follows.
            let (sysop, handle) = (self.is_sysop(), self.user.handle.clone());
            self.node.update(|n| {
                n.sysop = sysop;
                n.user = handle;
            });
            *self.user_base.borrow_mut() = Some(db);
            tracing::info!("{}: account changed elsewhere, read again", self.user.handle);
            let what = account_changes(&original, &edited, &self.strings());
            if !what.is_empty() {
                told.push(what);
            }
        }
        told
    }

    /// [`Self::absorb_account_changes`], telling the caller.
    pub(crate) async fn take_account_changes(&mut self) -> io::Result<()> {
        for what in self.absorb_account_changes() {
            let s = self.strings().fmt("AccountChanged", &[&what]);
            if self.screen.where_x() != 1 {
                self.newline();
            }
            self.newline();
            self.show_line(&s).await?;
            self.flush().await?;
        }
        Ok(())
    }

    /// `userShowList` (`UL`), text version.
    async fn user_list(&mut self) -> io::Result<()> {
        let users = self.state.users.list().map_err(io::Error::other)?;
        self.clear_screen();
        self.set_slot(Slot::Info);
        self.show_line("|U4 Num   Username                 Location              Calls  Level").await?;
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(79));
        self.newline();
        let mut lines = 3;
        for u in users.iter().filter(|u| !u.deleted) {
            let line = format!(
                "|U1 {} {} {} {} {}",
                resize(&u.id.to_string(), 5),
                lit(&resize(&u.handle, 24)),
                lit(&resize(&u.location, 21)),
                resize(&u.total_calls.to_string(), 6),
                u.level
            );
            self.show_line(&line).await?;
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(79));
        self.newline();
        // New: finger someone from the list (a number or a handle).
        self.say("UserListFinger").await?;
        let mut opts = crate::input::ReadOpts::new(36);
        opts.complete_users = true;
        let who = self.read_string(opts).await?;
        self.newline();
        if !who.is_empty() {
            self.finger(&who).await?;
        }
        Ok(())
    }

    /// `nodeList` (`XL`), text version.
    pub(crate) async fn node_list(&mut self) -> io::Result<()> {
        self.newline();
        self.set_slot(Slot::Info);
        self.show_line("|U4 Node  User                     Activity").await?;
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(60));
        self.newline();
        let (sysop, me) = (self.is_sysop(), self.node.number());
        let online: Vec<_> = self.state.nodes.online().into_iter().filter(|(m, i)| !i.invisible || sysop || *m == me).collect();
        for (n, info) in online {
            let user = if info.user.is_empty() { self.strings().get("NodeLoggingIn").to_string() } else { info.user };
            let line = format!("|U1 {} |U3{}|U1 {}", resize(&n.to_string(), 5), lit(&resize(&user, 24)), lit(&info.activity));
            self.show_line(&line).await?;
        }
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(60));
        self.newline();
        Ok(())
    }
}

/// New: what a caller would want to know changed in their account, in
/// words ("daily time 60 to 120 min, level C to D"); empty if nothing
/// they'd notice.
fn account_changes(a: &crate::user::User, b: &crate::user::User, strings: &crate::strings::Strings) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut num = |what: &str, x: u64, y: u64, unit: &str| {
        if x != y {
            out.push(format!("{what} {x} to {y}{unit}"));
        }
    };
    num("daily time", a.time_per_day as u64, b.time_per_day as u64, " min");
    num("security level", a.sl as u64, b.sl as u64, "");
    num("download level", a.dsl as u64, b.dsl as u64, "");
    num("file points", a.file_points as u64, b.file_points as u64, "");
    num("upload/download ratio", a.uldl_ratio as u64, b.uldl_ratio as u64, "");
    num("kb ratio", a.kb_ratio as u64, b.kb_ratio as u64, "");
    num("downloads a day", a.limit_dl as u64, b.limit_dl as u64, "");
    num("download kb a day", a.limit_dl_kb as u64, b.limit_dl_kb as u64, "");
    num("posts per call", a.post_call as u64, b.post_call as u64, "");
    if a.level != b.level {
        out.push(format!("level {} to {}", a.level, b.level));
    }
    if a.flags != b.flags {
        out.push("flags".into());
    }
    if b.time_today > a.time_today {
        out.push(strings.fmt("AccountTimeToday", &[&(b.time_today - a.time_today).to_string()]));
    }
    out.join(", ")
}
