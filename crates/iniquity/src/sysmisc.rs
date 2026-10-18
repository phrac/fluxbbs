//! SysOp editors for system data: the BBS list (`*B`, CONFIG15), the
//! system statistics (`*H`, CONFIG14) and the prompt strings (`*E`,
//! CONFIG3).

use std::io;

use chrono::NaiveDate;
use iniquity_term::Slot;
use iniquity_term::codes::{resize, squish};

use crate::cfgedit::{Field, Form};
use crate::session::Session;
use crate::strings::stock_names;
use crate::sysdata::{Bbs, Counts};
use crate::user::{fmt_date, parse_date};

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// `maxStr`: strings per page in the string editor.
const STRINGS_PER_PAGE: usize = 5;

type StatField = (char, &'static str, &'static str, fn(&mut Counts) -> &mut u64);

const STAT_FIELDS: [StatField; 7] = [
    ('B', "Calls", "Total number of calls to the board.", |c| &mut c.calls),
    ('C', "Posts", "System's total number of public posts.", |c| &mut c.posts),
    ('D', "Email", "Total number of email sent.", |c| &mut c.email),
    ('E', "Uploads", "Total number of uploads.", |c| &mut c.uploads),
    ('F', "Downloads", "Total number of downloads.", |c| &mut c.downloads),
    ('G', "Uploaded Kb", "Total number of uploaded kilobytes.", |c| &mut c.upload_kb),
    ('H', "Downloaded Kb", "Total number of downloaded kilobytes.", |c| &mut c.download_kb),
];

/// Where the next search starts: just after the current page, wrapping.
fn search(names: &[&str], text: impl Fn(&str) -> String, from: usize, want: &str) -> Option<usize> {
    let want = want.to_uppercase();
    (0..names.len()).map(|i| (from + i) % names.len()).find(|&i| {
        let t = text(names[i]).to_uppercase();
        t.contains(&want) || names[i].to_uppercase().contains(&want)
    })
}

impl Session {
    // ---- *B: the BBS list ---------------------------------------------------

    /// `cfgBbsListEditor`.
    pub async fn bbs_list_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: BBS list edit", self.user.handle);
        loop {
            let list = self.state.sys.bbs_list().map_err(io_err)?;
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Name                           SysOp            Address");
            self.newline();
            self.rule().await?;
            let mut lines = 3;
            for (i, b) in list.iter().enumerate() {
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&b.name, 30),
                    resize(&b.sysop, 16),
                    squish(&b.address, 25)
                ));
                self.newline();
                lines += 1;
                if !self.more(&mut lines).await? {
                    break;
                }
            }
            if list.is_empty() {
                self.write("There are currently no boards in the list.");
                self.newline();
            }
            self.rule().await?;
            let cmd = self.cfg_list_prompt(&[("A", "dd"), ("D", "elete"), ("E", "dit"), ("Esc", " Quit")]).await?;
            let n = list.len() as u32;
            self.newline();
            match cmd {
                Some('A') => {
                    let b = Bbs { name: "New BBS".into(), who_added: self.user.handle.clone(), ..Bbs::default() };
                    self.state.sys.add_bbs(&b).map_err(io_err)?;
                    self.bbs_edit(n + 1).await?;
                }
                Some('D') if n > 0 => {
                    let Some(which) = self.cfg_ask_number("Delete which BBS?", 1, n).await? else { continue };
                    let b = &list[which as usize - 1];
                    self.show(&format!("|U1Delete |U3{}|U1? ", b.name)).await?;
                    if self.yes_no(false).await? {
                        self.state.sys.delete_bbs(b.id).map_err(io_err)?;
                    }
                }
                Some('E') if n > 0 => {
                    if let Some(num) = self.cfg_ask_number("Begin edit with which BBS?", 1, n).await? {
                        self.bbs_edit(num).await?;
                    }
                }
                Some(_) => {}
                None => return Ok(()),
            }
        }
    }

    /// `cfgEditBBS`.
    async fn bbs_edit(&mut self, mut num: u32) -> io::Result<()> {
        type BbsText = (char, &'static str, usize, &'static str, fn(&mut Bbs) -> &mut String);
        let texts: [BbsText; 7] = [
            ('A', "Name", 40, "The name of this bulletin board system.", |b| &mut b.name),
            ('B', "SysOp", 36, "The SysOp's name or handle.", |b| &mut b.sysop),
            ('C', "Address", 40, "Where to reach it (telnet/SSH host, or a phone number).", |b| &mut b.address),
            ('E', "Software", 20, "Name of the bulletin board software this system is running.", |b| &mut b.software),
            ('F', "Storage", 20, "File area storage.", |b| &mut b.storage),
            ('G', "Other info", 58, "Any other information about this board (affiliations etc).", |b| &mut b.info),
            ('H', "Who added", 36, "The name of the user that added this BBS to the list.", |b| &mut b.who_added),
        ];
        loop {
            let list = self.state.sys.bbs_list().map_err(io_err)?;
            let n = list.len() as u32;
            if n == 0 {
                return Ok(());
            }
            num = num.clamp(1, n);
            let mut b = list[num as usize - 1].clone();
            let mut f = Form::new(&format!("{} BBS List Editor", self.state.config().bbs.name));
            f.value_col(20);
            f.label("Current", 12, format!("{num} of {n}"));
            for (key, label, len, info, get) in &texts[..3] {
                f.item(*key, label, *len, get(&mut b).clone(), info);
            }
            f.item('D', "Max speed", 9, b.speed.to_string(), "The fastest connection this BBS takes (baud, or 0).");
            for (key, label, len, info, get) in &texts[3..] {
                f.item(*key, label, *len, get(&mut b).clone(), info);
            }
            f.item('[', "Previous BBS", 0, "", "");
            f.item(']', "Next BBS", 0, "", "");
            self.cfg_draw(&f);
            let before = b.clone();
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'D' => {
                        if let Some(v) = self.cfg_number(&mut f, key, 0, 999_999_999).await? {
                            b.speed = v as u32;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    k => {
                        let Some((_, _, len, _, get)) = texts.iter().find(|t| t.0 == k) else { continue };
                        if let Some(v) = self.cfg_text(&mut f, k, Field::TEXT.max(*len)).await? {
                            *get(&mut b) = v;
                        }
                    }
                }
            };
            if b != before {
                self.state.sys.update_bbs(&b).map_err(io_err)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }

    // ---- *H: system statistics ----------------------------------------------

    /// `cfgStatEditor`: the all-time totals shown by the `T?` codes.
    pub async fn stat_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: statistics edit", self.user.handle);
        let mut totals = self.state.sys.totals().map_err(io_err)?;
        let mut first: Option<NaiveDate> = self.state.sys.first_day().map_err(io_err)?;
        let (orig_totals, orig_first) = (totals.clone(), first);
        let mut f = Form::new(&format!("{} System Statistics Editor", self.state.config().bbs.name));
        f.value_col(25);
        f.item('A', "First day", 8, fmt_date(first), "Date [mm/dd/yy] of the first day the BBS was online.");
        for (key, label, info, get) in &STAT_FIELDS {
            f.item(*key, label, 12, get(&mut totals).to_string(), info);
        }
        self.cfg_draw(&f);
        while let Some(key) = self.cfg_command(&mut f).await? {
            if key == 'A' {
                if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(8)).await? {
                    match parse_date(&v) {
                        Some(d) => first = Some(d),
                        None if v.is_empty() => first = None,
                        None => {}
                    }
                }
                self.cfg_set(&mut f, key, fmt_date(first));
            } else if let Some((_, _, _, get)) = STAT_FIELDS.iter().find(|s| s.0 == key)
                && let Some(v) = self.cfg_number(&mut f, key, 0, 999_999_999_999).await?
            {
                *get(&mut totals) = v;
            }
        }
        self.screen.goto(1, self.screen.rows());
        self.newline();
        if totals != orig_totals || first != orig_first {
            let got = self.state.sys.set_totals(first, &totals).map_err(io_err)?;
            if got != totals {
                self.show_line("|U1Some totals can't go below what the daily history already counts|U2:").await?;
                for (_, label, _, get) in &STAT_FIELDS {
                    let (want, have) = (*get(&mut totals), *get(&mut got.clone()));
                    if want != have {
                        self.show_line(&format!("|U1  {label}|U2: |U3{have}")).await?;
                    }
                }
            }
        }
        Ok(())
    }

    // ---- *E: prompt strings -------------------------------------------------

    /// `cfgStringEditor`: the strings a page at a time, with search and
    /// jump. Saved to strings.toml as they change.
    pub async fn string_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: string edit", self.user.handle);
        let names = stock_names();
        let total = names.len();
        let mut top = 0usize;
        let mut last_search = String::new();
        loop {
            let strings = self.strings();
            self.clear_screen();
            let mut rows = Vec::new();
            for (i, name) in names.iter().enumerate().skip(top).take(STRINGS_PER_PAGE) {
                self.write_with_controls(&format!("|U1{}|U2:|U1{:03}|U2, |U4{name}", i - top + 1, i + 1));
                self.newline();
                rows.push(self.screen.where_y());
                self.set_slot(Slot::Info);
                self.write(&squish(strings.get(name), 79));
                self.newline();
                self.newline();
            }
            let pick = self
                .cfg_list_prompt(&[("[", " Prev"), ("]", " Next"), ("1", "-5 Edit"), ("2", ""), ("3", ""), ("4", ""), ("5", ""), ("/", " Search"), ("=", " Jump"), ("Esc", " Quit")])
                .await?;
            match pick {
                None => return Ok(()),
                Some('[') => top = top.saturating_sub(STRINGS_PER_PAGE),
                Some(']') => top = (top + STRINGS_PER_PAGE).min(total.saturating_sub(STRINGS_PER_PAGE)),
                Some('=') => {
                    if let Some(n) = self.cfg_ask_number("Jump to string number", 1, total as u32).await? {
                        top = (n as usize - 1).min(total.saturating_sub(STRINGS_PER_PAGE));
                    }
                }
                Some('/') => {
                    self.show("|U2-- |U1Text to search for |U5[|U6enter|U5/|U4again|U5]: |U3").await?;
                    let want = self.read_string(crate::input::ReadOpts::new(60)).await?;
                    self.newline();
                    if !want.is_empty() {
                        last_search = want;
                    }
                    if !last_search.is_empty() {
                        let s = self.strings();
                        let from = (top + STRINGS_PER_PAGE) % total;
                        if let Some(i) = search(&names, |n| s.get(n).to_string(), from, &last_search) {
                            top = i.min(total.saturating_sub(STRINGS_PER_PAGE));
                        }
                    }
                }
                Some(c @ '1'..='5') => {
                    let row = c as usize - '1' as usize;
                    let Some(&y) = rows.get(row) else { continue };
                    let name = names[top + row];
                    let field = Field { max: 255, ..Field::TEXT };
                    if let Some(v) = self.edit_line(1, y, 79, strings.get(name), field).await? {
                        let mut new = (*self.strings()).clone();
                        new.set(name, v);
                        self.state.set_strings(new).map_err(io_err)?;
                        tracing::info!("{}: changed string {name}", self.user.handle);
                    }
                }
                Some(_) => {}
            }
        }
    }
}

impl Session {
    /// `*G`: who's banned and what failed lately, and lift a ban.
    pub async fn security_view(&mut self) -> io::Result<()> {
        let bans = self.state.guard.bans();
        let events = self.state.sys.security_events(20).map_err(io::Error::other)?;
        self.clear_screen();
        self.newline();
        self.set_slot(Slot::Info);
        self.show_line("|U4 Banned address                             Left").await?;
        self.rule().await?;
        if bans.is_empty() {
            self.show_line("|U1 (nobody)").await?;
        }
        for (ip, left) in &bans {
            let line = format!("|U1 {} |U3{}m", resize(&ip.to_string(), 40), left.as_secs().div_ceil(60));
            self.show_line(&line).await?;
        }
        self.newline();
        self.set_slot(Slot::Info);
        self.show_line("|U4 When            Address                      What").await?;
        self.rule().await?;
        if events.is_empty() {
            self.show_line("|U1 (nothing)").await?;
        }
        for (at, ip, what) in &events {
            let line = format!("|U1 {} {} |U3{}", resize(&at.format("%m/%d/%y %H:%M").to_string(), 15), resize(ip, 28), what);
            self.show_line(&line).await?;
        }
        self.rule().await?;
        if bans.is_empty() {
            return Ok(());
        }
        self.show("|U1Lift the ban on which address |U5[|U4enter|U5=|U4none|U5, |U4all|U5]: |U3").await?;
        let who = self.read_string(crate::input::ReadOpts::new(45)).await?;
        self.newline();
        let who = who.trim().to_string();
        if who.is_empty() {
            return Ok(());
        }
        let lifted: Vec<std::net::IpAddr> = if who.eq_ignore_ascii_case("all") {
            bans.iter().map(|(ip, _)| *ip).collect()
        } else {
            who.parse().into_iter().collect()
        };
        let mut n = 0;
        for ip in lifted {
            if self.state.guard.unban(ip) {
                n += 1;
                let _ = self.state.sys.clear_ban(&ip.to_string());
                tracing::info!("{}: lifted the ban on {ip}", self.user.handle);
            }
        }
        let s = if n > 0 { format!("|U1Lifted |U3{n}|U1 ban(s)|U2.") } else { "|U4No ban on that address|U5.".into() };
        self.show_line(&s).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_wraps_and_matches_names() {
        let names = ["Pause", "HitAKey", "LoginName"];
        let text = |n: &str| match n {
            "Pause" => "paused - stop".to_string(),
            "HitAKey" => "press a key".to_string(),
            _ => "your name".to_string(),
        };
        assert_eq!(search(&names, text, 0, "KEY"), Some(1));
        assert_eq!(search(&names, text, 2, "stop"), Some(0), "wraps around");
        assert_eq!(search(&names, text, 0, "loginname"), Some(2), "names match too");
        assert_eq!(search(&names, text, 0, "zzz"), None);
    }
}
