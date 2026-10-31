//! New: profiles. Finger (`UF`, `f` in the message reader, and after the
//! user list) shows a caller's public side: where they're from, their
//! note, since when, when last on (or on now), what they've posted and
//! uploaded lately, and their plan, a few lines of their own (P in user
//! config). Real name, phone, address and birth date stay private. A
//! FINGER screen dresses it (`~UN` handle, `~LO` location, `~UO` note,
//! `~FC` first call, `~LS` last seen, `~CA` calls, `~PO` posts, `~UL`
//! uploads, `~DL` downloads, `~LP` latest post, `~LU` latest uploads,
//! `~KA` karma, from pushes, `~FI` requests filled, `~P1`-`~P6` the plan);
//! without it, a plain one.

use std::io;

use fluxbbs_term::lit;
use fluxbbs_term::codes::{resize, squish};

use crate::input::ReadOpts;
use crate::messages::io_err;
use crate::session::Session;
use crate::user::{User, fmt_date};

/// Lines a profile's ANSI header may have.
pub const HEADER_LINES: usize = 8;

/// An uploaded header: the text before SAUCE, lines (CP437, ESC kept as
/// itself), less blank ones at the ends, at most `HEADER_LINES`. Shown
/// through `editor::ansi_bytes`, so only colours and cursor-rights reach
/// a screen.
pub fn parse_header(data: &[u8]) -> Vec<String> {
    let data = data.split(|&b| b == 0x1A).next().unwrap_or_default();
    let mut lines: Vec<String> = data
        .split(|&b| b == b'\n')
        .map(|l| fluxbbs_term::cp437::decode(l.strip_suffix(b"\r").unwrap_or(l)).replace('\u{2190}', "\x1b").trim_end().to_string())
        .collect();
    let blank = |l: &String| crate::editor::strip_colours(l).trim().is_empty();
    while lines.last().is_some_and(blank) {
        lines.pop();
    }
    let top = lines.iter().take_while(|l| blank(l)).count();
    lines.drain(..top);
    lines.truncate(HEADER_LINES);
    lines
}

/// Lines in a plan, and their width.
pub const PLAN_LINES: usize = 6;
const PLAN_WIDTH: usize = 70;

impl Session {
    /// `UF` (new): finger the handle given, or asked (Tab completes).
    pub async fn finger(&mut self, param: &str) -> io::Result<()> {
        let mut who = param.trim().to_string();
        if who.is_empty() {
            self.newline();
            self.say("FingerWho").await?;
            let mut opts = ReadOpts::new(36);
            opts.complete_users = true;
            who = self.read_string(opts).await?;
            self.newline();
            if who.is_empty() {
                return Ok(());
            }
        }
        let found = match who.parse::<u32>() {
            Ok(n) => self.state.users.get(n).map_err(io_err)?,
            Err(_) => self.state.users.find(&who, false).map_err(io_err)?,
        };
        match found.filter(|u| !u.deleted) {
            Some(u) => self.finger_user(&u).await,
            None => self.say_line("FingerUnknown").await,
        }
    }

    /// Show `u`'s profile.
    pub async fn finger_user(&mut self, u: &User) -> io::Result<()> {
        let today = crate::clock::now().date_naive();
        let sysop = self.is_sysop();
        let online = self.state.nodes.online().into_iter().find(|(_, i)| i.user.eq_ignore_ascii_case(&u.handle) && (!i.invisible || sysop));
        let seen = match online {
            Some((n, _)) => self.strings().fmt("FingerOnNow", &[&n.to_string()]),
            None => crate::mci::ago(u.last_call, today),
        };
        let msg_areas: Vec<String> = self.readable_areas().into_iter().filter(|(_, a)| !a.is_email()).map(|(_, a)| a.key().to_string()).collect();
        let latest_post = match self.state.msgs.latest_from(u.id, &msg_areas).map_err(io_err)? {
            Some(m) => {
                let area = self.readable_areas().into_iter().find(|(_, a)| a.key().eq_ignore_ascii_case(&m.area)).map(|(_, a)| a.area.name).unwrap_or_default();
                self.strings().fmt("FingerPost", &[&m.subject, &area, &crate::mci::ago(Some(m.posted.date()), today)])
            }
            None => self.strings().get("FingerNothing").to_string(),
        };
        let fa = self.state.fareas();
        let file_areas: Vec<String> = self.visible_fareas().into_iter().filter_map(|(n, _)| fa.get(n).map(|a| a.file.clone())).collect();
        let uploads = self.state.files.by_uploader(&u.handle, &file_areas, 3).map_err(io_err)?;
        let latest_uploads = if uploads.is_empty() {
            self.strings().get("FingerNothing").to_string()
        } else {
            uploads.iter().map(|f| f.name.clone()).collect::<Vec<_>>().join(", ")
        };
        let karma = self.karma_text(u.id)?;
        // New: requests they've filled, and the points.
        let (fills, fill_pts) = self.state.msgs.fills_of(u.id).map_err(io::Error::other)?;
        let fills = if fills == 0 { String::new() } else { self.strings().fmt("FingerFillsAre", &[&fills.to_string(), &fill_pts.to_string()]) };
        // New: their real name, if they show it.
        let real = if u.show_real_name { u.real_name.clone() } else { String::new() };
        let mut w = vec![
            ("RN", real.clone()),
            ("UN", u.handle.clone()),
            ("LO", u.location.clone()),
            ("UO", u.note.clone()),
            ("FC", fmt_date(u.first_call)),
            ("LS", seen.clone()),
            ("CA", u.total_calls.to_string()),
            ("PO", u.posts.to_string()),
            ("UL", u.uploads.to_string()),
            ("DL", u.downloads.to_string()),
            ("LP", latest_post.clone()),
            ("LU", latest_uploads.clone()),
            ("KA", karma.clone()),
            ("FI", fills.clone()),
        ];
        let plan_codes = ["P1", "P2", "P3", "P4", "P5", "P6"];
        for (i, code) in plan_codes.iter().enumerate() {
            w.push((code, u.plan.get(i).cloned().unwrap_or_default()));
        }
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        let w: crate::session::Waves = w.into_iter().map(|(k, v)| (k.as_bytes().try_into().expect("two-letter code"), v)).collect();
        if self.show_file_with("FINGER", &w).await? {
            self.finger_notes(u).await?;
            return self.prompt_key().await;
        }
        // The plain one, under their header if they've one.
        for l in &u.profile_header {
            self.screen.write_bytes(&crate::editor::ansi_bytes(l));
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.newline();
        }
        self.newline();
        let name = if real.is_empty() { lit(&u.handle) } else { format!("{} |U2({}|U2)", lit(&u.handle), lit(&real)) };
        self.show_line(&format!("|U4 {name}")).await?;
        self.rule().await?;
        let pair = |a: &str, b: &str, c: &str, d: &str| format!("|U1 {} |U3{}|U1{} |U3{}", resize(a, 11), lit(&resize(b, 26)), resize(c, 12), lit(&squish(d, 26)));
        let s = self.strings();
        let mut lines = vec![
            pair(s.get("FingerFrom"), &u.location, s.get("FingerNote"), &u.note),
            pair(s.get("FingerSince"), &fmt_date(u.first_call), s.get("FingerSeen"), &seen),
            pair(s.get("FingerCalls"), &u.total_calls.to_string(), s.get("FingerPosts"), &u.posts.to_string()),
            pair(s.get("FingerUploads"), &u.uploads.to_string(), s.get("FingerDownloads"), &u.downloads.to_string()),
            format!("|U1 {} |U3{}", resize(s.get("FingerLatest"), 11), lit(&squish(&latest_post, 66))),
            format!("|U1 {} |U3{}", resize(s.get("FingerFiles"), 11), lit(&squish(&latest_uploads, 66))),
            format!("|U1 {} |U3{}", resize(s.get("FingerKarma"), 11), squish(&karma, 66)),
        ];
        if !fills.is_empty() {
            lines.push(format!("|U1 {} |U3{}", resize(s.get("FingerFills"), 11), squish(&fills, 66)));
        }
        drop(s);
        for l in lines {
            self.show_line(&l).await?;
        }
        self.rule().await?;
        if u.plan.is_empty() {
            self.say_line("FingerNoPlan").await?;
        }
        for l in &u.plan {
            self.write(" ");
            self.write_with_controls(l);
            self.newline();
        }
        self.rule().await?;
        self.finger_notes(u).await?;
        self.prompt_key().await
    }

    /// For a SysOp: their private notes on `u`, under the profile.
    async fn finger_notes(&mut self, u: &User) -> io::Result<()> {
        if !self.is_sysop() {
            return Ok(());
        }
        let notes = self.state.users.notes(u.id).map_err(io_err)?;
        if notes.is_empty() {
            return Ok(());
        }
        self.say_line("FingerNotes").await?;
        for (_, by, at, text) in notes.iter().rev().take(5).rev() {
            let line = format!("|U2 {} {} |U3{}", self.local(*at).format("%m/%d/%y"), lit(&resize(by, 10)), lit(&squish(text, 56)));
            self.show_line(&line).await?;
        }
        Ok(())
    }

    /// `Q` in user config (new): your profile: your plan, whether your
    /// real name shows, and an ANSI header you upload.
    pub async fn profile_edit(&mut self) -> io::Result<()> {
        loop {
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.clear_screen();
            self.say_line("ProfileHead").await?;
            self.rule().await?;
            let yn = |b: bool| if b { "yes" } else { "no" }.to_string();
            let lines = |n: usize| match n {
                0 => "none".to_string(),
                1 => "1 line".to_string(),
                n => format!("{n} lines"),
            };
            let items = [
                ("p", "ProfilePlan", lines(self.user.plan.len())),
                ("r", "ProfileRealName", yn(self.user.show_real_name)),
                ("u", "ProfileUpload", lines(self.user.profile_header.len())),
                ("c", "ProfileClear", String::new()),
                ("m", "ProfileMemory", if !self.user.ai_memory { "off".into() } else if self.user.ai_profile.trim().is_empty() { "nothing yet".into() } else { "on".into() }),
                ("v", "ProfileView", String::new()),
            ];
            for (k, name, v) in &items {
                let label = self.strings().get(name).to_string();
                let s = self.strings().fmt("ProfileItem", &[k, &label, v]);
                self.show_line(&s).await?;
            }
            self.rule().await?;
            self.say("ProfileAsk").await?;
            let k = self.read_key().await?;
            self.newline();
            match k.upper() {
                Some('P') => self.plan_edit().await?,
                Some('R') => {
                    self.user.show_real_name = !self.user.show_real_name;
                    self.save_own_user().map_err(io_err)?;
                }
                Some('U') => self.upload_profile_header().await?,
                Some('C') => {
                    self.user.profile_header.clear();
                    self.save_own_user().map_err(io_err)?;
                }
                Some('V') => {
                    let me = self.user.clone();
                    self.finger_user(&me).await?;
                }
                Some('M') => self.memory_view().await?,
                _ => return Ok(()),
            }
        }
    }

    /// What the board remembers of your interests (`aimemory.rs`): off
    /// until you turn it on, and turning it off forgets it.
    async fn memory_view(&mut self) -> io::Result<()> {
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.say_line("MemoryHead").await?;
        self.rule().await?;
        // Saved by the board after the last call: read it as it is now.
        if let Some(u) = self.state.users.get(self.user.id).map_err(io_err)? {
            self.user.ai_profile = u.ai_profile;
        }
        if crate::aimemory::is_bot(&self.user) {
            self.rule().await?;
            self.say_line("MemoryBot").await?;
            return self.prompt_key().await;
        }
        if !self.user.ai_memory {
            self.say_line("MemoryOff").await?;
            self.rule().await?;
            self.say("MemoryAskOn").await?;
            if self.yes_no(false).await? {
                self.set_memory(true)?;
                tracing::info!("{}: turned on what the board remembers", self.user.handle);
            }
            return Ok(());
        }
        if self.user.ai_profile.trim().is_empty() {
            self.say_line("MemoryNone").await?;
        }
        for l in self.user.ai_profile.clone().lines() {
            self.show_line(&format!("|U3 {}", lit(l))).await?;
        }
        self.rule().await?;
        self.say("MemoryAskOff").await?;
        if self.yes_no(false).await? {
            self.set_memory(false)?;
            tracing::info!("{}: turned off and cleared what the board remembers", self.user.handle);
        }
        Ok(())
    }

    /// Turn the memory on or off; off forgets what was kept.
    fn set_memory(&mut self, on: bool) -> io::Result<()> {
        let before = self.user.clone();
        self.user.ai_memory = on;
        if !on {
            self.user.ai_profile.clear();
        }
        self.state.users.merge(&before, &self.user).map_err(io_err)?;
        *self.user_base.borrow_mut() = Some(self.user.clone());
        Ok(())
    }

    /// Upload an ANSI header for your profile (a file, as an
    /// auto-signature is): colours and cursor moves only, at most
    /// `HEADER_LINES` lines.
    async fn upload_profile_header(&mut self) -> io::Result<()> {
        self.newline();
        let Some(proto) = self.select_protocol(false).await? else { return Ok(()) };
        let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("node{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&temp);
        let got = self.receive_files(&proto, &temp).await?;
        let data = match got.first() {
            Some(p) => std::fs::read(p).map(|mut d| {
                d.truncate(64 * 1024);
                d
            })?,
            None => Vec::new(),
        };
        let _ = std::fs::remove_dir_all(&temp);
        let header = parse_header(&data);
        if header.is_empty() {
            return self.say_line("ProfileHeaderNone").await;
        }
        self.user.profile_header = header;
        self.save_own_user().map_err(io_err)?;
        tracing::info!("{}: uploaded a profile header", self.user.handle);
        self.say_line("ProfileHeaderSaved").await?;
        self.prompt_key().await
    }

    /// P in user config (new): write your plan, a line at a time, each
    /// starting as it was; an empty line ends it.
    pub async fn plan_edit(&mut self) -> io::Result<()> {
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        let s = self.strings().fmt("PlanAsk", &[&PLAN_LINES.to_string()]);
        self.show_line(&s).await?;
        self.newline();
        let mut plan = Vec::new();
        for i in 0..PLAN_LINES {
            self.show(&format!("|U2{}|U1: |U3", i + 1)).await?;
            let old = self.user.plan.get(i).cloned().unwrap_or_default();
            let line = self.read_string(ReadOpts::new(PLAN_WIDTH).prefill(old)).await?;
            self.newline();
            if line.trim().is_empty() {
                break;
            }
            plan.push(crate::editor::strip_colours(&line).trim_end().to_string());
        }
        self.user.plan = plan;
        self.save_own_user().map_err(io_err)?;
        Ok(())
    }

    /// `*N` (new): a SysOp's private notes on a caller, `param` naming
    /// them (or asked).
    pub async fn user_notes_cmd(&mut self, param: &str) -> io::Result<()> {
        let mut who = param.trim().to_string();
        if who.is_empty() {
            self.newline();
            self.say("NotesWho").await?;
            let mut opts = ReadOpts::new(36);
            opts.complete_users = true;
            who = self.read_string(opts).await?;
            self.newline();
            if who.is_empty() {
                return Ok(());
            }
        }
        match self.state.users.find(&who, true).map_err(io_err)? {
            Some(u) => self.user_notes(&u).await,
            None => self.say_line("FingerUnknown").await,
        }
    }

    /// A caller's notes, oldest first, numbered; type one to add it, or
    /// `-N` to remove note N. Only for SysOps: callers never see them (the
    /// AI chat is given them to suit itself to the caller, and told never
    /// to show them).
    pub async fn user_notes(&mut self, u: &User) -> io::Result<()> {
        loop {
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.clear_screen();
            let s = self.strings().fmt("NotesHead", &[&lit(&u.handle)]);
            self.show_line(&s).await?;
            self.rule().await?;
            let notes = self.state.users.notes(u.id).map_err(io_err)?;
            if notes.is_empty() {
                self.say_line("NotesNone").await?;
            }
            for (i, (_, by, at, text)) in notes.iter().enumerate() {
                let line = format!("|U2{:>3}. |U1{} {} |U3{}", i + 1, self.local(*at).format("%m/%d/%y"), lit(&resize(by, 12)), lit(text));
                self.show_line(&line).await?;
            }
            self.rule().await?;
            self.say("NotesAsk").await?;
            let v = self.read_string(ReadOpts::new(70)).await?;
            self.newline();
            let v = crate::editor::strip_colours(v.trim());
            if v.is_empty() {
                return Ok(());
            }
            if let Some(n) = v.strip_prefix('-').and_then(|n| n.trim().parse::<usize>().ok()) {
                if let Some((id, ..)) = notes.get(n.wrapping_sub(1)) {
                    self.state.users.delete_note(*id).map_err(io_err)?;
                    tracing::info!("{}: removed a note on {}", self.user.handle, u.handle);
                }
                continue;
            }
            self.state.users.add_note(u.id, &self.user.handle, &v).map_err(io_err)?;
            tracing::info!("{}: noted on {}", self.user.handle, u.handle);
        }
    }
}
