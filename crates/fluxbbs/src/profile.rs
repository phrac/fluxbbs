//! New: profiles. Finger (`UF`, `f` in the message reader, and after the
//! user list) shows a caller's public side: where they're from, their
//! note, since when, when last on (or on now), what they've posted and
//! uploaded lately, and their plan, a few lines of their own (P in user
//! config). Real name, phone, address and birth date stay private. A
//! FINGER screen dresses it (`~UN` handle, `~LO` location, `~UO` note,
//! `~FC` first call, `~LS` last seen, `~CA` calls, `~PO` posts, `~UL`
//! uploads, `~DL` downloads, `~LP` latest post, `~LU` latest uploads,
//! `~KA` karma, from pushes, `~P1`-`~P6` the plan); without it, a plain one.

use std::io;

use fluxbbs_term::lit;
use fluxbbs_term::codes::{resize, squish};

use crate::input::ReadOpts;
use crate::messages::io_err;
use crate::session::Session;
use crate::user::{User, fmt_date};

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
        let mut w = vec![
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
        ];
        let plan_codes = ["P1", "P2", "P3", "P4", "P5", "P6"];
        for (i, code) in plan_codes.iter().enumerate() {
            w.push((code, u.plan.get(i).cloned().unwrap_or_default()));
        }
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        let w: crate::session::Waves = w.into_iter().map(|(k, v)| (k.as_bytes().try_into().expect("two-letter code"), v)).collect();
        if self.show_file_with("FINGER", &w).await? {
            return self.prompt_key().await;
        }
        // The plain one.
        self.newline();
        self.show_line(&format!("|U4 {}", lit(&u.handle))).await?;
        self.rule().await?;
        let pair = |a: &str, b: &str, c: &str, d: &str| format!("|U1 {} |U3{}|U1{} |U3{}", resize(a, 11), lit(&resize(b, 26)), resize(c, 12), lit(&squish(d, 26)));
        let s = self.strings();
        let lines = [
            pair(s.get("FingerFrom"), &u.location, s.get("FingerNote"), &u.note),
            pair(s.get("FingerSince"), &fmt_date(u.first_call), s.get("FingerSeen"), &seen),
            pair(s.get("FingerCalls"), &u.total_calls.to_string(), s.get("FingerPosts"), &u.posts.to_string()),
            pair(s.get("FingerUploads"), &u.uploads.to_string(), s.get("FingerDownloads"), &u.downloads.to_string()),
            format!("|U1 {} |U3{}", resize(s.get("FingerLatest"), 11), lit(&squish(&latest_post, 66))),
            format!("|U1 {} |U3{}", resize(s.get("FingerFiles"), 11), lit(&squish(&latest_uploads, 66))),
            format!("|U1 {} |U3{}", resize(s.get("FingerKarma"), 11), squish(&karma, 66)),
        ];
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
}
