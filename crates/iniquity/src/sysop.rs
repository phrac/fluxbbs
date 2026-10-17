//! SysOp editors (the CONFIG units) and maintenance commands. Areas are
//! edited on a copy and saved to their TOML file when the SysOp leaves
//! an area, as the original saved its record.

use std::io;
use std::path::Path;

use iniquity_term::Slot;
use iniquity_term::codes::{resize, squish};

use crate::cfgedit::{Field, Form, yes_no};
use crate::config::Protocol;
use crate::extprog::{Archiver, Archivers, Protocols};
use crate::fileareas::{FileArea, FileAreas, SortBy};
use crate::msgareas::{Conf, MsgArea, MsgAreas};
use crate::filebase::{FileRec, wildcard};
use crate::files::{FAreaRef, commas, is_zip};
use crate::session::Session;
use crate::transfer::safe_name;
use crate::user::{fmt_date, parse_date};

/// Description lines shown under the file editor (`faLoadDesc` showed 6).
const DESC_SHOWN: usize = 6;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// A storage key not in `used`: `BASE`, `BASE2`, `BASE3`...
fn unused_key<'a>(used: impl Iterator<Item = &'a str> + Clone, base: &str) -> String {
    let taken = |k: &str| used.clone().any(|u| u.eq_ignore_ascii_case(k));
    (1..)
        .map(|n| if n == 1 { base.to_string() } else { format!("{base}{n}") })
        .find(|k| !taken(k))
        .expect("some key is free")
}

/// A fresh area for `*F` insert (`faReset`).
fn new_file_area(areas: &FileAreas) -> FileArea {
    let file = unused_key(areas.areas.iter().map(|a| a.file.as_str()), "NEWAREA");
    FileArea {
        name: "New Iniquity file area".into(),
        path: format!("files/{}", file.to_ascii_lowercase()).into(),
        file,
        sponsor: "SysOp".into(),
        acs: "s25".into(),
        upload_acs: "s50".into(),
        download_acs: "s50".into(),
        password: String::new(),
        sort: SortBy::None,
        sort_descending: false,
    }
}

/// `+` in the file editor: put the description into a ZIP as `name`,
/// replacing any copy already there. Returns the archive's new size.
pub fn zip_set_description(path: &Path, name: &str, lines: &[String]) -> anyhow::Result<u64> {
    use std::io::Write;
    let mut src = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let tmp = path.with_extension("iqtmp");
    let result = (|| -> anyhow::Result<()> {
        let mut out = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        for i in 0..src.len() {
            let entry = src.by_index_raw(i)?;
            let entry_name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
            if entry_name.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            out.raw_copy_file(entry)?;
        }
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        out.start_file(name, opts)?;
        let text: Vec<u8> = lines.iter().flat_map(|l| [iniquity_term::cp437::encode(l), b"\r\n".to_vec()].concat()).collect();
        out.write_all(&text)?;
        out.finish()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    Ok(std::fs::metadata(path)?.len())
}

/// `fMoveFile`: rename, or copy and delete across filesystems.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

/// `cfgMoveFileArea`: move item `from` to before `before` (both from 0;
/// `before` may be `len`).
fn move_before<T>(v: &mut Vec<T>, from: usize, before: usize) {
    let item = v.remove(from);
    let to = if before > from { before - 1 } else { before };
    v.insert(to, item);
}

impl Session {
    /// Area numbers in the download batch go stale when areas or files
    /// are reordered, so the original cleared it first.
    async fn clear_batch_ok(&mut self) -> io::Result<bool> {
        if self.batch.is_empty() {
            return Ok(true);
        }
        self.newline();
        self.show("|U5-- |U4Warning|U5:  |U1Download batch will be cleared|U2.  |U1Continue|U2? ").await?;
        if !self.yes_no(true).await? {
            return Ok(false);
        }
        self.batch.clear();
        Ok(true)
    }

    fn save_fareas(&self, areas: FileAreas) -> io::Result<()> {
        self.state.set_fareas(areas).map_err(io_err)
    }

    // ---- *@ / *!: conferences (CONFIG12) ------------------------------------

    fn save_confs(&self, msg: bool, confs: Vec<Conf>) -> io::Result<()> {
        if msg {
            let mut a = (*self.state.areas()).clone();
            a.confs = confs;
            self.state.set_areas(a).map_err(io_err)
        } else {
            let mut a = (*self.state.fareas()).clone();
            a.confs = confs;
            self.save_fareas(a)
        }
    }

    /// `cfgConferenceEditor`: message (`*@`) or file (`*!`) conferences.
    pub async fn conference_editor(&mut self, msg: bool) -> io::Result<()> {
        let kind = if msg { "message" } else { "file" };
        tracing::info!("{}: {kind} conference edit", self.user.handle);
        loop {
            let confs = self.confs(msg);
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Description                          ACS                  Key");
            self.newline();
            self.rule().await?;
            let mut lines = 3;
            for (i, c) in confs.iter().enumerate() {
                self.set_slot(Slot::Info);
                self.write(&format!(" {} {} {} {}", resize(&(i + 1).to_string(), 4), resize(&c.desc, 36), resize(&c.acs, 20), c.key));
                self.newline();
                lines += 1;
                if !self.more(&mut lines).await? {
                    break;
                }
            }
            if confs.is_empty() {
                self.write(&format!("No {kind} conferences currently exist."));
                self.newline();
            }
            self.rule().await?;
            let cmd = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await?;
            let mut confs = confs;
            let n = confs.len() as u32;
            self.newline();
            let renumbered = match cmd {
                Some('I') => {
                    let Some(at) = self.cfg_ask_number(&format!("Insert before which {kind} conference?"), 1, n + 1).await? else { continue };
                    let desc = if msg { "New Message Conference" } else { "New File Conference" };
                    confs.insert(at as usize - 1, Conf { key: 'X', desc: desc.into(), acs: String::new() });
                    self.save_confs(msg, confs)?;
                    at <= n
                }
                Some('D') => {
                    if n <= 1 {
                        self.show_line(&format!("|U0You must have at least one {kind} conference!")).await?;
                        self.prompt_key().await?;
                        continue;
                    }
                    let Some(which) = self.cfg_ask_number(&format!("Delete which {kind} conference?"), 1, n).await? else { continue };
                    confs.remove(which as usize - 1);
                    self.save_confs(msg, confs)?;
                    true
                }
                Some('M') => {
                    if n <= 1 {
                        self.show_line(&format!("|U0There is only one {kind} conference, no need to move it.")).await?;
                        self.prompt_key().await?;
                        continue;
                    }
                    let Some(from) = self.cfg_ask_number(&format!("Move which {kind} conference?"), 1, n).await? else { continue };
                    let Some(before) = self.cfg_ask_number(&format!("Move before which {kind} conference?"), 1, n + 1).await? else { continue };
                    move_before(&mut confs, from as usize - 1, before as usize - 1);
                    self.save_confs(msg, confs)?;
                    true
                }
                Some('E') => {
                    if n == 0 {
                        continue;
                    }
                    let Some(num) = self.cfg_ask_number(&format!("Begin edit with which {kind} conference?"), 1, n).await? else { continue };
                    self.conf_edit(msg, num).await?;
                    false
                }
                _ => return Ok(()),
            };
            if renumbered {
                self.show_line(&format!(
                    "|U1Areas join conferences by number (|U3x1|U1, |U3x2|U1... in their ACS); check your {kind} areas."
                ))
                .await?;
                self.prompt_key().await?;
            }
        }
    }

    async fn conf_edit(&mut self, msg: bool, mut num: u32) -> io::Result<()> {
        let kind = if msg { "Message" } else { "File" };
        loop {
            let mut confs = self.confs(msg);
            let n = confs.len() as u32;
            let idx = num as usize - 1;
            let mut c = confs[idx].clone();
            let mut f = Form::new(&format!("{} {kind} Conference Editor", self.state.config.bbs.name));
            f.label("Current conference", 12, format!("{num} of {n}"));
            f.item('A', "Key", 1, c.key.to_string(), "Key used to select and identify this conference.");
            f.item('B', "Description", 30, &c.desc, "This conference's description.");
            f.item('C', "ACS", 20, &c.acs, "Access condition required to enter this conference.");
            f.item('[', "Previous conference", 0, "", "");
            f.item(']', "Next conference", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => {
                        let field = Field { allowed: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789", ..Field::UPPER.max(1) };
                        match self.cfg_text(&mut f, key, field).await?.and_then(|v| v.chars().next()) {
                            Some(k) => c.key = k,
                            None => self.cfg_set(&mut f, key, c.key.to_string()),
                        }
                    }
                    'B' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(40)).await? {
                            c.desc = v;
                        }
                    }
                    'C' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::LOWER).await? {
                            c.acs = v;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if confs[idx] != c {
                confs[idx] = c;
                self.save_confs(msg, confs)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }

    // ---- *A: message areas (CONFIG5) ----------------------------------------

    fn save_areas(&self, areas: MsgAreas) -> io::Result<()> {
        self.state.set_areas(areas).map_err(io_err)
    }

    /// `cfgMsgAreaEditor`: list, insert, delete, edit and move message
    /// areas. (Email is its own area and isn't listed.)
    pub async fn msg_area_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: message area edit", self.user.handle);
        loop {
            let mut areas = (*self.state.areas()).clone();
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Description                Key       ACS    Post   Msgs   Max");
            self.newline();
            self.rule().await?;
            let mut lines = 3;
            for (i, a) in areas.areas.iter().enumerate() {
                let count = self.state.msgs.count(&a.file).map_err(io_err)?;
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&a.name, 26),
                    resize(&a.file, 9),
                    resize(&a.acs, 6),
                    resize(&a.post_acs, 6),
                    resize(&count.to_string(), 6),
                    if a.max_msgs == 0 { "-".to_string() } else { a.max_msgs.to_string() }
                ));
                self.newline();
                lines += 1;
                if !self.more(&mut lines).await? {
                    break;
                }
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            match cmd {
                'E' => {
                    self.newline();
                    let n = areas.areas.len() as u32;
                    if n > 0
                        && let Some(num) = self.cfg_ask_number("Begin edit with which message area?", 1, n).await?
                    {
                        self.msg_area_edit(num).await?;
                    }
                }
                'D' => {
                    self.newline();
                    let n = areas.areas.len() as u32;
                    if n <= 1 {
                        self.show_line("|U0You must have at least one message area!").await?;
                        self.prompt_key().await?;
                        continue;
                    }
                    let Some(which) = self.cfg_ask_number("Delete which message area?", 1, n).await? else { continue };
                    let gone = areas.areas.remove(which as usize - 1);
                    let shared = areas.areas.iter().any(|a| a.file.eq_ignore_ascii_case(&gone.file)) || areas.email.file.eq_ignore_ascii_case(&gone.file);
                    let msgs = self.state.msgs.count(&gone.file).map_err(io_err)?;
                    self.save_areas(areas)?;
                    tracing::info!("{}: deleted message area {which} ({})", self.user.handle, gone.name);
                    if msgs > 0 && !shared {
                        self.show(&format!("|U1Delete this area's {msgs} messages? ")).await?;
                        if self.yes_no(false).await? {
                            self.state.msgs.delete_area(&gone.file).map_err(io_err)?;
                        }
                    }
                }
                _ => {
                    let fresh = |list: &[MsgArea]| {
                        let used = list.iter().map(|a| a.file.as_str()).chain(std::iter::once(areas.email.file.as_str()));
                        MsgArea::new("New Iniquity Message Area", &unused_key(used, "NEWAREA"))
                    };
                    let mut list = areas.areas.clone();
                    if self.list_change(&mut list, cmd, "message area", fresh).await? {
                        areas.areas = list;
                        self.save_areas(areas)?;
                    }
                }
            }
        }
    }

    /// `cfgEditMsgArea`.
    async fn msg_area_edit(&mut self, mut num: u32) -> io::Result<()> {
        type Flag = (char, &'static str, &'static str, fn(&mut MsgArea) -> &mut bool);
        let flags: [Flag; 5] = [
            ('1', "Unhidden", "Display this area under any circumstances?", |a| &mut a.unhidden),
            ('2', "Real names", "Force use of user's REAL name when posting in this area?", |a| &mut a.real_names),
            ('3', "Private", "Allow private messages to be posted in this area?", |a| &mut a.private),
            ('4', "Mandatory", "Force user to read all new messages in this area?", |a| &mut a.mandatory),
            ('5', "Anonymous", "Allow anonymous messages to be posted in this area?", |a| &mut a.anonymous),
        ];
        loop {
            let mut areas = (*self.state.areas()).clone();
            let n = areas.areas.len() as u32;
            let idx = num as usize - 1;
            let mut a = areas.areas[idx].clone();
            let msgs = self.state.msgs.count(&a.file).map_err(io_err)?;
            let mut f = Form::new(&format!("{} Message Area Editor", self.state.config.bbs.name));
            f.value_col(20);
            f.label("Current area", 12, format!("{num} of {n}"));
            f.item('A', "Description", 40, &a.name, "This message area's description (shown in message area lists, etc).");
            f.item('B', "Storage key", 8, &a.file, "Name the area's messages are kept under (letters and digits).");
            f.item('G', "Area sponsor", 36, &a.sponsor, "Creator or manager of this message area (usually the SysOp).");
            f.item('H', "Access ACS", 20, &a.acs, "ACS required to access or read messages in this area.");
            f.item('I', "Post ACS", 20, &a.post_acs, "ACS required to post messages in this area.");
            f.item('J', "Max messages", 5, a.max_msgs.to_string(), "Most messages *P keeps in this area (0 = unlimited).");
            f.item('K', "Password", 20, &a.password, "Password required to access or read messages in this area.");
            f.item('L', "QWK name", 16, &a.qwk_name, "Shortened message area name (max 16 characters) for QWK indexing.");
            f.label("Messages", 6, msgs.to_string());
            // Two columns: 1 2 3 down the left, 4 and 5 beside 1 and 2.
            for i in [0, 3, 1, 4, 2] {
                let (key, label, info, get) = flags[i];
                let v = yes_no(*get(&mut a));
                if i < 3 { f.item(key, label, 3, v, info) } else { f.item_beside(40, key, label, 3, v, info) }
            }
            f.item('[', "Previous area", 0, "", "");
            f.item(']', "Next area", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            a.name = v;
                        }
                    }
                    'B' => {
                        let Some(v) = self.cfg_text(&mut f, key, Field::KEY).await? else { continue };
                        let clash = areas.areas.iter().enumerate().any(|(i, o)| i != idx && o.file.eq_ignore_ascii_case(&v))
                            || areas.email.file.eq_ignore_ascii_case(&v);
                        if v.is_empty() || v.eq_ignore_ascii_case(&a.file) {
                            self.cfg_set(&mut f, key, &a.file);
                        } else if clash {
                            self.cfg_info(&mut f, "Another area already uses that key.");
                            self.cfg_set(&mut f, key, &a.file);
                        } else {
                            self.state.msgs.rename_area(&a.file, &v).map_err(io_err)?;
                            if a.qwk_name.eq_ignore_ascii_case(&a.file) {
                                a.qwk_name = v.clone();
                                self.cfg_set(&mut f, 'L', &a.qwk_name);
                            }
                            a.file = v;
                            // Save now: the messages already moved.
                            areas.areas[idx] = a.clone();
                            self.save_areas(areas.clone())?;
                        }
                    }
                    'G' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            a.sponsor = v;
                        }
                    }
                    'H' | 'I' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::LOWER).await? {
                            *if key == 'H' { &mut a.acs } else { &mut a.post_acs } = v;
                        }
                    }
                    'J' => {
                        if let Some(v) = self.cfg_number(&mut f, key, 0, 99_999).await? {
                            a.max_msgs = v as u32;
                        }
                    }
                    'K' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::UPPER).await? {
                            a.password = v;
                        }
                    }
                    'L' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            a.qwk_name = v;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    k => {
                        if let Some((_, _, _, get)) = flags.iter().find(|fl| fl.0 == k) {
                            let cur = *get(&mut a);
                            *get(&mut a) = self.cfg_bool(&mut f, k, cur).await?;
                        }
                    }
                }
            };
            if areas.areas[idx] != a {
                areas.areas[idx] = a;
                self.save_areas(areas)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }

    /// `maPackAreaAsk` (`*P`): pack one area, or all (`A` as the
    /// parameter, or answering yes).
    pub async fn pack_msg_areas(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        let param = param.trim().to_ascii_uppercase();
        let areas = self.state.areas();
        let which: Vec<u32> = if param.starts_with('A') || param == "PACK" {
            (1..=areas.areas.len() as u32).collect()
        } else if let Ok(n) = param.parse::<u32>() {
            vec![n]
        } else {
            self.say("MsgPackAskAll").await?;
            let all = self.yes_no(true).await?;
            self.newline();
            if all { (1..=areas.areas.len() as u32).collect() } else { vec![self.user.cur_msg_area] }
        };
        let mut total = 0;
        for n in which {
            let Some(a) = areas.get(n).cloned() else { continue };
            let count = self.state.msgs.count(&a.file).map_err(io_err)?;
            let s = self.strings().fmt("MsgPackStart", &[&a.name, &count.to_string()]);
            self.show(&s).await?;
            self.flush().await?;
            total += self.state.msgs.pack(&a.file, a.max_msgs).map_err(io_err)?;
            let s = self.strings().fmt("MsgPackEnd", &[&a.name, &count.to_string()]);
            self.show_line(&s).await?;
        }
        tracing::info!("{}: packed message areas, {total} messages removed", self.user.handle);
        Ok(())
    }

    // ---- *F: file areas (CONFIG9) -------------------------------------------

    /// `*F`: list, insert, delete, edit and move file areas.
    pub async fn file_area_editor(&mut self) -> io::Result<()> {
        if !self.clear_batch_ok().await? {
            return Ok(());
        }
        tracing::info!("{}: file area edit", self.user.handle);
        loop {
            self.fa_list_all().await?;
            let cmd = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await?;
            match cmd {
                Some('I') => self.fa_insert().await?,
                Some('D') => self.fa_delete().await?,
                Some('E') => self.fa_edit().await?,
                Some('M') => self.fa_move().await?,
                _ => return Ok(()),
            }
        }
    }

    /// `cfgFaListThem`.
    async fn fa_list_all(&mut self) -> io::Result<()> {
        let areas = self.state.fareas();
        self.clear_screen();
        self.set_slot(Slot::Text);
        self.write(" Num  Description                Key       ACS    ulACS  dlACS  Files  Sort");
        self.newline();
        self.rule().await?;
        let mut lines = 3;
        for (i, a) in areas.areas.iter().enumerate() {
            let count = self.state.files.count(&a.file).map_err(io_err)?;
            self.set_slot(Slot::Info);
            let sort = match a.sort {
                SortBy::None => String::new(),
                s => format!("{}{}", s.label(), if a.sort_descending { " ↓" } else { "" }),
            };
            self.write(&format!(
                " {} {} {} {} {} {} {} {}",
                resize(&(i + 1).to_string(), 4),
                resize(&a.name, 26),
                resize(&a.file, 9),
                resize(&a.acs, 6),
                resize(&a.upload_acs, 6),
                resize(&a.download_acs, 6),
                resize(&count.to_string(), 6),
                squish(&sort, 11)
            ));
            self.newline();
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        if areas.areas.is_empty() {
            self.write("No file areas currently exist.");
            self.newline();
        }
        self.rule().await
    }

    async fn fa_insert(&mut self) -> io::Result<()> {
        let mut areas = (*self.state.fareas()).clone();
        let n = areas.areas.len() as u32;
        self.newline();
        let Some(at) = self.cfg_ask_number("Insert before which file area?", 1, n + 1).await? else { return Ok(()) };
        let count = self.cfg_ask_number("Number of areas to insert", 1, 50).await?.unwrap_or(1);
        for _ in 0..count {
            let a = new_file_area(&areas);
            areas.areas.insert(at as usize - 1, a);
        }
        self.save_fareas(areas)?;
        tracing::info!("{}: inserted {count} file area(s) at {at}", self.user.handle);
        Ok(())
    }

    async fn fa_delete(&mut self) -> io::Result<()> {
        let mut areas = (*self.state.fareas()).clone();
        let n = areas.areas.len() as u32;
        self.newline();
        if n <= 1 {
            self.show_line("|U0You must have at least one file area!").await?;
            return self.prompt_key().await;
        }
        let Some(which) = self.cfg_ask_number("Delete which file area?", 1, n).await? else { return Ok(()) };
        let gone = areas.areas.remove(which as usize - 1);
        // Keep the records if another area still uses the same key.
        let shared = areas.areas.iter().any(|a| a.file.eq_ignore_ascii_case(&gone.file));
        let records = self.state.files.count(&gone.file).map_err(io_err)?;
        self.save_fareas(areas)?;
        tracing::info!("{}: deleted file area {which} ({})", self.user.handle, gone.name);
        if records > 0 && !shared {
            self.show(&format!("|U1Delete this file area's {records} file records? ")).await?;
            if self.yes_no(false).await? {
                self.state.files.delete_area(&gone.file).map_err(io_err)?;
            }
            self.newline();
            self.show_line(&format!("|U2The files themselves are still in |U3{}|U2.", gone.path.display())).await?;
            self.prompt_key().await?;
        }
        Ok(())
    }

    async fn fa_move(&mut self) -> io::Result<()> {
        let mut areas = (*self.state.fareas()).clone();
        let n = areas.areas.len() as u32;
        self.newline();
        if n <= 1 {
            self.show_line("|U0There is only one file area, no need to move it.").await?;
            return self.prompt_key().await;
        }
        let Some(from) = self.cfg_ask_number("Move which file area?", 1, n).await? else { return Ok(()) };
        let Some(before) = self.cfg_ask_number("Move before which file area?", 1, n + 1).await? else { return Ok(()) };
        move_before(&mut areas.areas, from as usize - 1, before as usize - 1);
        self.save_fareas(areas)
    }

    /// `cfgEditFileArea`.
    async fn fa_edit(&mut self) -> io::Result<()> {
        let n = self.state.fareas().areas.len() as u32;
        self.newline();
        if n == 0 {
            return Ok(());
        }
        let Some(mut num) = self.cfg_ask_number("Begin edit with which file area?", 1, n).await? else { return Ok(()) };
        let sorts: Vec<&str> = SortBy::ALL.iter().map(|s| s.label()).collect();
        let directions = ["Ascending", "Descending"];
        loop {
            let mut areas = (*self.state.fareas()).clone();
            let idx = num as usize - 1;
            let mut a = areas.areas[idx].clone();
            let files = self.state.files.count(&a.file).map_err(io_err)?;
            let mut f = Form::new(&format!("{} File Area Editor", self.state.config.bbs.name));
            f.label("Current area", 12, format!("{num} of {n}"));
            f.item('A', "Description", 40, &a.name, "This file area's description (shown in file area lists, etc).");
            f.item('B', "Storage key", 8, &a.file, "Name the area's file records are kept under (letters and digits).");
            f.item('C', "File path", 40, a.path.display().to_string(), "Directory the files are stored in (relative to the config).");
            f.item('D', "Area sponsor", 36, &a.sponsor, "Creator or manager of this file area (usually the SysOp).");
            f.item('E', "Access ACS", 20, &a.acs, "ACS required to access this file area.");
            f.item('F', "Upload ACS", 20, &a.upload_acs, "ACS required to upload files to this area.");
            f.item('G', "Download ACS", 20, &a.download_acs, "ACS required to download files from this area.");
            f.item('H', "Password", 20, &a.password, "Password required to access this area (blank for none).");
            f.item('I', "Sort method", 20, a.sort.label(), "How *S sorts this file area.");
            f.item('J', "Sort direction", 20, directions[a.sort_descending as usize], "Direction to sort files.");
            f.label("Files", 6, files.to_string());
            f.item('[', "Previous area", 0, "", "");
            f.item(']', "Next area", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            a.name = v;
                        }
                    }
                    'B' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::KEY).await? {
                            let clash = areas.areas.iter().enumerate().any(|(i, o)| i != idx && o.file.eq_ignore_ascii_case(&v));
                            if v.is_empty() || v.eq_ignore_ascii_case(&a.file) {
                                self.cfg_set(&mut f, key, &a.file);
                            } else if clash {
                                self.cfg_info(&mut f, "Another area already uses that key.");
                                self.cfg_set(&mut f, key, &a.file);
                            } else {
                                self.state.files.rename_area(&a.file, &v).map_err(io_err)?;
                                a.file = v;
                                // Save now: the records already moved.
                                areas.areas[idx] = a.clone();
                                self.save_fareas(areas.clone())?;
                            }
                        }
                    }
                    'C' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(120)).await? {
                            if v.is_empty() {
                                self.cfg_set(&mut f, key, a.path.display().to_string());
                            } else {
                                let dir = self.state.config.resolve(std::path::Path::new(&v));
                                if !dir.is_dir() && !self.cfg_ask(&mut f, "Create directory?", true).await? {
                                    self.cfg_set(&mut f, key, a.path.display().to_string());
                                } else {
                                    a.path = v.into();
                                }
                            }
                        }
                    }
                    'D' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            a.sponsor = v;
                        }
                    }
                    'E' | 'F' | 'G' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::LOWER).await? {
                            *match key {
                                'E' => &mut a.acs,
                                'F' => &mut a.upload_acs,
                                _ => &mut a.download_acs,
                            } = v;
                        }
                    }
                    'H' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::UPPER).await? {
                            a.password = v;
                        }
                    }
                    'I' => {
                        let cur = SortBy::ALL.iter().position(|s| *s == a.sort).unwrap_or(0);
                        a.sort = SortBy::ALL[self.cfg_option(&mut f, key, &sorts, cur).await?];
                    }
                    'J' => {
                        a.sort_descending = self.cfg_option(&mut f, key, &directions, a.sort_descending as usize).await? == 1;
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            areas.areas[idx] = a;
            if *self.state.fareas() != areas {
                self.save_fareas(areas)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }

    // ---- *Y: files in the current area (CONFIG13) ---------------------------

    /// `cfgFileEditor`: edit the records of the current area's files.
    pub async fn file_editor(&mut self, param: &str) -> io::Result<()> {
        let Some(area) = self.current_farea() else { return Ok(()) };
        let count = self.state.files.count(&area.area.file).map_err(io_err)?;
        self.newline();
        if count == 0 {
            return self.show_line(&format!("|U1There are no files in |U3{}|U1.", area.area.name)).await;
        }
        if !self.clear_batch_ok().await? {
            return Ok(());
        }
        tracing::info!("{}: file edit in {}", self.user.handle, area.area.name);
        let mut cur = param.trim().parse::<u32>().unwrap_or(1).clamp(1, count);
        let mut note = String::new();
        'files: loop {
            let a = area.area.clone();
            let total = self.state.files.count(&a.file).map_err(io_err)?;
            if total == 0 {
                break;
            }
            cur = cur.clamp(1, total);
            let Some(mut fr) = self.state.files.get(&a.file, cur).map_err(io_err)? else { break };
            let dir = self.farea_dir(&a);
            let exists = dir.join(&fr.name).is_file();
            let mut f = Form::new(&format!("{} File Editor: {}", self.state.config.bbs.name, a.name));
            f.value_col(25);
            let missing = if exists { "" } else { " |U2(|U1does |U3not|U1 exist|U2)" };
            f.item('/', "Current file #", 30, format!("{cur} of {total}{missing}"), "");
            f.item_beside(40, '[', "Prev", 0, "", "");
            f.item_beside(60, ']', "Next", 0, "", "");
            f.item('A', "Filename", 12, &fr.name, "Filename; the file itself is renamed too.");
            f.item('B', "Size", 12, fr.size.to_string(), "Size of this file in bytes (should not normally be altered).");
            f.item('C', "Date", 8, fmt_date(fr.date), "File's date [mm/dd/yy].");
            f.item('D', "Downloads", 5, fr.downloads.to_string(), "Number of times this file has been downloaded.");
            f.item('E', "File points", 5, fr.points.to_string(), "This file's value in file points.");
            f.item('F', "Uploader", 36, &fr.uploader, "Name of the user that uploaded this file.");
            f.item('G', "Upload date", 8, fmt_date(fr.uploaded), "Date that this file was uploaded [mm/dd/yy].");
            f.item('H', "Validated", 3, yes_no(fr.valid), "Only validated files can be downloaded.");
            f.item('I', "Edit description", 12, format!("{} line(s)", fr.description.len()), "");
            f.item_beside(40, 'J', "View archive", 0, "", "");
            f.item('=', "Move file", 0, "", "");
            f.item_beside(40, '*', "Delete file", 0, "", "");
            f.item('+', "Apply description", 0, "", "");
            f.item_beside(40, '-', "Extract description", 0, "", "");
            let desc_row = f.next_row() + 1;
            f.gap(DESC_SHOWN as u16 + 1);
            self.cfg_draw(&f);
            self.set_slot(Slot::Info);
            for (i, line) in fr.description.iter().take(DESC_SHOWN).enumerate() {
                self.screen.goto(1, desc_row + i as u16);
                self.write_with_controls(line);
            }
            if !note.is_empty() {
                let n = std::mem::take(&mut note);
                self.cfg_info(&mut f, &n);
            }
            loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 'files };
                match key {
                    '[' => {
                        cur = if cur == 1 { total } else { cur - 1 };
                        continue 'files;
                    }
                    ']' => {
                        cur = if cur == total { 1 } else { cur + 1 };
                        continue 'files;
                    }
                    '/' => {
                        let want = self.cfg_read_info(&mut f, "File to edit (name/number): ", 13).await?.to_ascii_uppercase();
                        if want.is_empty() {
                            continue;
                        }
                        match want.parse::<u32>() {
                            Ok(n) if (1..=total).contains(&n) => cur = n,
                            _ => {
                                let list = self.state.files.list(&a.file).map_err(io_err)?;
                                match list.iter().find(|r| wildcard(&want, &r.name) || r.name.to_ascii_uppercase().contains(&want)) {
                                    Some(r) => cur = r.num,
                                    None => note = format!("No file matching {want}."),
                                }
                            }
                        }
                        continue 'files;
                    }
                    'A' => {
                        let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(64)).await? else { continue };
                        let Some(name) = safe_name(v.as_bytes()).filter(|n| *n != fr.name) else {
                            self.cfg_set(&mut f, key, &fr.name);
                            continue;
                        };
                        let (old, new) = (dir.join(&fr.name), dir.join(&name));
                        if new.exists() {
                            self.cfg_info(&mut f, "A file by that name is already there.");
                            self.cfg_set(&mut f, key, &fr.name);
                            continue;
                        }
                        if old.is_file() {
                            std::fs::rename(&old, &new)?;
                        }
                        fr.name = name;
                        self.cfg_set(&mut f, key, &fr.name);
                    }
                    'B' => match self.cfg_number(&mut f, key, 0, 999_999_999_999).await? {
                        Some(n) => fr.size = n,
                        None => continue,
                    },
                    'C' | 'G' => {
                        let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(8)).await? else { continue };
                        let d = if v.is_empty() { None } else { parse_date(&v) };
                        if !v.is_empty() && d.is_none() {
                            self.cfg_set(&mut f, key, fmt_date(if key == 'C' { fr.date } else { fr.uploaded }));
                            continue;
                        }
                        if key == 'C' { fr.date = d } else { fr.uploaded = d }
                        self.cfg_set(&mut f, key, fmt_date(d));
                    }
                    'D' => match self.cfg_number(&mut f, key, 0, 99_999).await? {
                        Some(n) => fr.downloads = n as u32,
                        None => continue,
                    },
                    'E' => match self.cfg_number(&mut f, key, 0, 99_999).await? {
                        Some(n) => fr.points = n as u32,
                        None => continue,
                    },
                    'F' => match self.cfg_text(&mut f, key, Field::TEXT).await? {
                        Some(v) => fr.uploader = v,
                        None => continue,
                    },
                    'H' => {
                        self.toggle_validated(&mut f, &mut fr).await?;
                        self.cfg_set(&mut f, key, yes_no(fr.valid));
                    }
                    'I' => {
                        self.edit_description(fr.clone()).await?;
                        continue 'files;
                    }
                    'J' => {
                        self.clear_screen();
                        self.view_archive_file(&FAreaRef { num: area.num, area: a.clone() }, &fr).await?;
                        self.prompt_key().await?;
                        continue 'files;
                    }
                    '=' => {
                        self.state.files.update(&fr).map_err(io_err)?;
                        self.move_file_to_area(&a, &fr).await?;
                        continue 'files;
                    }
                    '*' => {
                        if !self.cfg_ask(&mut f, "Delete this file from the area?", true).await? {
                            continue;
                        }
                        let path = dir.join(&fr.name);
                        if path.is_file() && self.cfg_ask(&mut f, "Destroy the file itself?", true).await? {
                            std::fs::remove_file(&path)?;
                        }
                        self.state.files.remove(&a.file, fr.num).map_err(io_err)?;
                        tracing::info!("{}: deleted {} from {}", self.user.handle, fr.name, a.name);
                        continue 'files;
                    }
                    '+' => {
                        let name = self.state.config.files.description_files.first().cloned().unwrap_or_else(|| "FILE_ID.DIZ".into());
                        let path = dir.join(&fr.name);
                        note = if fr.description.is_empty() {
                            "There's no description to apply.".into()
                        } else {
                            let result = if is_zip(&path) {
                                zip_set_description(&path, &name, &fr.description)
                            } else {
                                self.archiver_set_description(&path, &name, &fr.description).await
                            };
                            match result {
                                Ok(size) => {
                                    fr.size = size;
                                    format!("{name} written ({} bytes).", commas(size))
                                }
                                Err(e) => format!("Couldn't update the archive: {e}"),
                            }
                        };
                        self.state.files.update(&fr).map_err(io_err)?;
                        continue 'files;
                    }
                    '-' => {
                        let names = self.state.config.files.description_files.clone();
                        match self.archive_description(&dir.join(&fr.name), &names).await {
                            Some(lines) if !lines.is_empty() => {
                                fr.description = lines;
                                note = "Description extracted.".into();
                            }
                            _ => note = "No description found in the archive.".into(),
                        }
                        self.state.files.update(&fr).map_err(io_err)?;
                        continue 'files;
                    }
                    _ => continue,
                }
                self.state.files.update(&fr).map_err(io_err)?;
            }
        }
        self.screen.goto(1, self.screen.rows());
        self.newline();
        Ok(())
    }

    /// `+` for formats other than ZIP: the archiver's add command.
    async fn archiver_set_description(&mut self, path: &Path, name: &str, lines: &[String]) -> anyhow::Result<u64> {
        let Some(arc) = self.state.archivers().for_file(path).cloned().filter(|a| !a.add.is_empty()) else {
            anyhow::bail!("no archiver can add files to this kind of archive");
        };
        let dir = self.archive_scratch()?;
        let text: Vec<u8> = lines.iter().flat_map(|l| [iniquity_term::cp437::encode(l), b"\r\n".to_vec()].concat()).collect();
        std::fs::write(dir.join(name), text)?;
        let cmd = crate::extprog::expand(&arc.add, &std::path::absolute(path)?, &[name]);
        let ok = crate::extprog::run(&cmd, &dir).await;
        let _ = std::fs::remove_dir_all(&dir);
        if !ok?.0 {
            anyhow::bail!("the {} add command failed", arc.ext);
        }
        Ok(std::fs::metadata(path)?.len())
    }

    /// `H` in the file editor: validating credits the uploader, as the
    /// upload would have; unvalidating takes the credit back.
    async fn toggle_validated(&mut self, f: &mut Form, fr: &mut FileRec) -> io::Result<()> {
        let uploader = self.state.users.find(&fr.uploader, false).map_err(io_err)?;
        if let Some(mut u) = uploader {
            let question = if fr.valid {
                format!("{} uploaded this; remove their upload credit?", u.handle)
            } else {
                format!("Credit {} for uploading this file?", u.handle)
            };
            if self.cfg_ask(f, &question, true).await? {
                let fc = &self.state.config.files;
                let points = if fc.use_file_points && fc.upload_points_percent > 0 {
                    (fr.points as u64 * fc.upload_points_percent as u64 / 100) as u32
                } else {
                    0
                };
                // The caller's own record is live in the session.
                let me = u.id == self.user.id;
                let target = if me { &mut self.user } else { &mut u };
                if fr.valid {
                    target.uploads = target.uploads.saturating_sub(1);
                    target.upload_kb = target.upload_kb.saturating_sub(fr.size / 1024);
                    target.file_points = target.file_points.saturating_sub(points);
                } else {
                    target.uploads += 1;
                    target.upload_kb += fr.size / 1024;
                    target.file_points += points;
                }
                if me {
                    self.save_user();
                } else {
                    self.state.users.save(&u).map_err(io_err)?;
                }
            }
        }
        fr.valid = !fr.valid;
        Ok(())
    }

    /// `=` in the file editor.
    async fn move_file_to_area(&mut self, from: &FileArea, fr: &FileRec) -> io::Result<()> {
        self.fa_list_all().await?;
        let areas = self.state.fareas();
        let s = format!("|U2-- |U1Move |U3{}|U1 to which file area|U2? |U3", fr.name);
        let Some(n) = self.cfg_ask_number(&s, 1, areas.areas.len() as u32).await? else { return Ok(()) };
        let Some(to) = areas.get(n).cloned() else { return Ok(()) };
        if to.file.eq_ignore_ascii_case(&from.file) {
            return Ok(());
        }
        let (src, dst) = (self.farea_dir(from).join(&fr.name), self.farea_dir(&to).join(&fr.name));
        if src.is_file() {
            if dst.exists() {
                self.show_line(&format!("|U0{} already has a {}; the file itself stays put.", to.name, fr.name)).await?;
                self.prompt_key().await?;
            } else {
                self.show("|U2-- |U1Move the file itself|U2? ").await?;
                if self.yes_no(true).await? {
                    std::fs::create_dir_all(self.farea_dir(&to))?;
                    move_file(&src, &dst)?;
                }
            }
        }
        self.state.files.move_to(&from.file, fr.num, &to.file).map_err(io_err)?;
        tracing::info!("{}: moved {} from {} to {}", self.user.handle, fr.name, from.name, to.name);
        Ok(())
    }

    // ---- *>: download any file ---------------------------------------------

    /// `faDownloadAny`: send any file on the server, by path. The name may
    /// have wildcards (`/srv/files/*.zip`).
    pub async fn download_any(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        let mut want = param.trim().to_string();
        if want.is_empty() {
            self.show("|U1File(s) to download |U5(|U4full path|U5)|U2: |U3").await?;
            want = self.read_string(crate::input::ReadOpts::new(200)).await?;
            self.newline();
        }
        if want.is_empty() {
            return Ok(());
        }
        let path = std::path::PathBuf::from(&want);
        let files = match path.file_name().map(|n| n.to_string_lossy().into_owned()) {
            Some(name) if name.contains(['*', '?']) => {
                let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                let mut found: Vec<_> = std::fs::read_dir(dir)
                    .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
                    .unwrap_or_default();
                found.retain(|p| p.is_file() && p.file_name().is_some_and(|n| wildcard(&name, &n.to_string_lossy())));
                found.sort();
                found
            }
            _ if path.is_file() => vec![path],
            _ => Vec::new(),
        };
        if files.is_empty() {
            return self.show_line(&format!("|U0No such file: |U3{want}")).await;
        }
        let total: u64 = files.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        self.show_line(&format!("|U1{} file(s), |U3{}|U1 bytes.", files.len(), commas(total))).await?;
        let Some(proto) = self.select_protocol(files.len() > 1).await? else { return Ok(()) };
        tracing::info!("{}: download any: {}", self.user.handle, want);
        let sent = self.send_files(&proto, &files).await?;
        let ok = sent.iter().filter(|&&b| b).count();
        self.show_line(&format!("|U1Sent |U3{ok}|U1 of |U3{}|U1.", files.len())).await
    }

    // ---- list editors --------------------------------------------------------

    /// Insert, delete or move (the I/D/M of the list editors). `what`
    /// names an item ("protocol"). Returns whether the list changed.
    pub(crate) async fn list_change<T>(&mut self, items: &mut Vec<T>, cmd: char, what: &str, fresh: impl FnOnce(&[T]) -> T) -> io::Result<bool> {
        let n = items.len() as u32;
        self.newline();
        match cmd {
            'I' => {
                let Some(at) = self.cfg_ask_number(&format!("Insert before which {what}?"), 1, n + 1).await? else { return Ok(false) };
                let item = fresh(items);
                items.insert(at as usize - 1, item);
            }
            'D' => {
                if n == 0 {
                    return Ok(false);
                }
                let Some(which) = self.cfg_ask_number(&format!("Delete which {what}?"), 1, n).await? else { return Ok(false) };
                items.remove(which as usize - 1);
            }
            'M' => {
                if n <= 1 {
                    return Ok(false);
                }
                let Some(from) = self.cfg_ask_number(&format!("Move which {what}?"), 1, n).await? else { return Ok(false) };
                let Some(before) = self.cfg_ask_number(&format!("Move before which {what}?"), 1, n + 1).await? else { return Ok(false) };
                move_before(items, from as usize - 1, before as usize - 1);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    // ---- *X: transfer protocols (CONFIG7) -----------------------------------

    /// `cfgProtocolEditor`: the external protocols (Zmodem is built in).
    pub async fn protocol_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: protocol edit", self.user.handle);
        loop {
            let mut protos = self.state.protocols().list.clone();
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Key  Description                     Batch  ACS");
            self.newline();
            self.rule().await?;
            self.set_slot(Slot::InfoLo);
            self.write(" --   Z    Zmodem (built in)               Yes");
            self.newline();
            for (i, p) in protos.iter().enumerate() {
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&p.key.to_string(), 4),
                    resize(&p.desc, 31),
                    resize(yes_no(p.batch), 6),
                    squish(&p.acs, 20)
                ));
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            if cmd == 'E' {
                self.newline();
                let n = protos.len() as u32;
                if n == 0 {
                    continue;
                }
                if let Some(num) = self.cfg_ask_number("Begin edit with which protocol?", 1, n).await? {
                    self.protocol_edit(num).await?;
                }
                continue;
            }
            let fresh = |list: &[Protocol]| Protocol {
                key: ('A'..='Y').find(|c| !list.iter().any(|p| p.key.eq_ignore_ascii_case(c))).unwrap_or('X'),
                desc: "New protocol".into(),
                acs: String::new(),
                batch: false,
                send: "sz %F".into(),
                receive: "rz".into(),
            };
            if self.list_change(&mut protos, cmd, "protocol", fresh).await? {
                self.state.set_protocols(Protocols { list: protos }).map_err(io_err)?;
            }
        }
    }

    async fn protocol_edit(&mut self, mut num: u32) -> io::Result<()> {
        loop {
            let mut list = self.state.protocols().list.clone();
            let n = list.len() as u32;
            let idx = num as usize - 1;
            let mut p = list[idx].clone();
            let mut f = Form::new(&format!("{} Protocol Editor", self.state.config.bbs.name));
            f.value_col(25);
            f.label("Current protocol", 12, format!("{num} of {n}"));
            f.item('A', "Activation key", 1, p.key.to_string(), "Key pressed to select this protocol (Z is Zmodem's).");
            f.item('B', "Description", 36, &p.desc, "Name of this protocol.");
            f.item('C', "Access required", 20, &p.acs, "Access condition required to use this protocol.");
            f.item('D', "Batch protocol", 3, yes_no(p.batch), "Can it send several files at once?");
            f.item('E', "Receive command", 53, &p.receive, "Run to receive uploads, in the upload directory.");
            f.item('F', "Send command", 53, &p.send, "Run to send files; %F is replaced by the files.");
            f.item('[', "Previous protocol", 0, "", "");
            f.item(']', "Next protocol", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => {
                        let field = Field { allowed: "ABCDEFGHIJKLMNOPQRSTUVWXY0123456789", ..Field::UPPER.max(1) };
                        match self.cfg_text(&mut f, key, field).await?.and_then(|v| v.chars().next()) {
                            Some(k) if !list.iter().enumerate().any(|(i, o)| i != idx && o.key.eq_ignore_ascii_case(&k)) => p.key = k,
                            Some(_) => {
                                self.cfg_info(&mut f, "Another protocol already uses that key.");
                                self.cfg_set(&mut f, key, p.key.to_string());
                            }
                            None => self.cfg_set(&mut f, key, p.key.to_string()),
                        }
                    }
                    'B' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            p.desc = v;
                        }
                    }
                    'C' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::LOWER).await? {
                            p.acs = v;
                        }
                    }
                    'D' => p.batch = self.cfg_bool(&mut f, key, p.batch).await?,
                    'E' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(200)).await? {
                            p.receive = v;
                        }
                    }
                    'F' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(200)).await? {
                            p.send = v;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if list[idx] != p {
                list[idx] = p;
                self.state.set_protocols(Protocols { list }).map_err(io_err)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }

    // ---- *Z: archivers (CONFIG10) -------------------------------------------

    /// `cfgArchiverEditor`: archivers for formats other than ZIP.
    pub async fn archiver_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: archiver edit", self.user.handle);
        loop {
            let mut arcs = self.state.archivers().list.clone();
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Ext    Active  List command");
            self.newline();
            self.rule().await?;
            self.set_slot(Slot::InfoLo);
            self.write(" --   ZIP    Yes     (built in)");
            self.newline();
            for (i, a) in arcs.iter().enumerate() {
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&a.ext, 6),
                    resize(yes_no(a.active), 7),
                    squish(&a.list, 58)
                ));
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            if cmd == 'E' {
                self.newline();
                let n = arcs.len() as u32;
                if n == 0 {
                    continue;
                }
                if let Some(num) = self.cfg_ask_number("Begin edit with which archiver?", 1, n).await? {
                    self.archiver_edit(num).await?;
                }
                continue;
            }
            let fresh = |_: &[Archiver]| Archiver {
                ext: "NEW".into(),
                active: false,
                signature: String::new(),
                list: String::new(),
                extract: String::new(),
                add: String::new(),
            };
            if self.list_change(&mut arcs, cmd, "archiver", fresh).await? {
                self.state.set_archivers(Archivers { list: arcs }).map_err(io_err)?;
            }
        }
    }

    async fn archiver_edit(&mut self, mut num: u32) -> io::Result<()> {
        loop {
            let mut list = self.state.archivers().list.clone();
            let n = list.len() as u32;
            let idx = num as usize - 1;
            let mut a = list[idx].clone();
            let mut f = Form::new(&format!("{} Archiver Editor", self.state.config.bbs.name));
            f.value_col(25);
            f.label("Current archiver", 12, format!("{num} of {n}"));
            f.item('A', "Active archiver", 3, yes_no(a.active), "Is this archiver currently active or disabled?");
            f.item('B', "File extension", 5, &a.ext, "File extension that signifies use of this archiver (no dot).");
            f.item('C', "Archive signature", 24, &a.signature, "First bytes of the archive file, in hex (blank: go by the extension).");
            f.item('D', "List command", 53, &a.list, "Prints the contents of %A (for the archive viewer).");
            f.item('E', "Extract command", 53, &a.extract, "Extracts %F from %A into the current directory.");
            f.item('F', "Add command", 53, &a.add, "Adds %F (in the current directory) to %A.");
            f.item('[', "Previous archiver", 0, "", "");
            f.item(']', "Next archiver", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => a.active = self.cfg_bool(&mut f, key, a.active).await?,
                    'B' => match self.cfg_text(&mut f, key, Field::KEY.max(5)).await? {
                        Some(v) if !v.is_empty() => a.ext = v,
                        _ => self.cfg_set(&mut f, key, &a.ext),
                    },
                    'C' => {
                        let field = Field { allowed: "0123456789ABCDEF", ..Field::UPPER.max(24) };
                        if let Some(v) = self.cfg_text(&mut f, key, field).await? {
                            a.signature = v;
                        }
                    }
                    'D' | 'E' | 'F' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(200)).await? {
                            *match key {
                                'D' => &mut a.list,
                                'E' => &mut a.extract,
                                _ => &mut a.add,
                            } = v;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if list[idx] != a {
                list[idx] = a;
                self.state.set_archivers(Archivers { list }).map_err(io_err)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }

    // ---- *S: sort file areas (FILESORT) -------------------------------------

    /// `fsSortFileAreas`: sort one area, or every area that has a sort
    /// method (`A` as the parameter, or answering yes).
    pub async fn sort_file_areas(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        let param = param.trim().to_ascii_uppercase();
        let areas = self.state.fareas();
        let all = if param.starts_with('A') {
            true
        } else if let Ok(n) = param.parse::<u32>() {
            return self.sort_one_area(n).await;
        } else {
            self.say("FaSortAskAll").await?;
            let all = self.yes_no(true).await?;
            self.newline();
            all
        };
        if !self.clear_batch_ok().await? {
            return Ok(());
        }
        if !all {
            return self.sort_one_area(self.user.cur_file_area).await;
        }
        for n in 1..=areas.areas.len() as u32 {
            if areas.get(n).is_some_and(|a| a.sort != SortBy::None) {
                self.sort_one_area(n).await?;
            }
        }
        Ok(())
    }

    async fn sort_one_area(&mut self, n: u32) -> io::Result<()> {
        let Some(a) = self.state.fareas().get(n).cloned() else { return Ok(()) };
        let count = self.state.files.count(&a.file).map_err(io_err)?;
        let s = self.strings().fmt("FaSortStart", &[&a.name, &count.to_string()]);
        self.show(&s).await?;
        self.flush().await?;
        self.state.files.sort_area(&a.file, a.sort, a.sort_descending).map_err(io_err)?;
        let s = self.strings().fmt("FaSortEnd", &[&a.name, &count.to_string()]);
        self.show_line(&s).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_areas() {
        let mut v = vec!['a', 'b', 'c', 'd'];
        move_before(&mut v, 0, 3);
        assert_eq!(v, ['b', 'c', 'a', 'd']);
        move_before(&mut v, 3, 0);
        assert_eq!(v, ['d', 'b', 'c', 'a']);
        move_before(&mut v, 1, 4);
        assert_eq!(v, ['d', 'c', 'a', 'b']);
    }

    #[test]
    fn description_into_zip() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("iqdesc{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("TEST.ZIP");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("README.TXT", opts).unwrap();
        z.write_all(b"hello").unwrap();
        z.start_file("FILE_ID.DIZ", opts).unwrap();
        z.write_all(b"old").unwrap();
        z.finish().unwrap();
        let lines = vec!["New description".to_string(), "line two".to_string()];
        let size = zip_set_description(&path, "FILE_ID.DIZ", &lines).unwrap();
        assert_eq!(size, std::fs::metadata(&path).unwrap().len());
        assert_eq!(crate::files::zip_description(&path, &["FILE_ID.DIZ".into()]).unwrap(), lines);
        let names: Vec<String> = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap().file_names().map(|n| n.unwrap().into_owned()).collect();
        assert_eq!(names, ["README.TXT", "FILE_ID.DIZ"], "replaced, not duplicated");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn new_areas_get_free_keys() {
        let mut areas = FileAreas::default();
        let a = new_file_area(&areas);
        assert_eq!(a.file, "NEWAREA2");
        areas.areas.push(a);
        assert_eq!(new_file_area(&areas).file, "NEWAREA3");
    }
}
