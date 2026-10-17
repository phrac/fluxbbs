//! SysOp editors (the CONFIG units) and maintenance commands. Areas are
//! edited on a copy and saved to their TOML file when the SysOp leaves
//! an area, as the original saved its record.

use std::io;
use std::path::Path;

use iniquity_term::Slot;
use iniquity_term::codes::{resize, squish};

use crate::cfgedit::{Field, Form, yes_no};
use crate::fileareas::{FileArea, FileAreas, SortBy};
use crate::filebase::{FileRec, wildcard};
use crate::files::{FAreaRef, commas, zip_description};
use crate::session::Session;
use crate::transfer::safe_name;
use crate::user::{fmt_date, parse_date};

/// Description lines shown under the file editor (`faLoadDesc` showed 6).
const DESC_SHOWN: usize = 6;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// A storage key not used by any area: `BASE`, `BASE2`, `BASE3`...
fn unused_key(areas: &FileAreas, base: &str) -> String {
    let taken = |k: &str| areas.areas.iter().any(|a| a.file.eq_ignore_ascii_case(k));
    (1..)
        .map(|n| if n == 1 { base.to_string() } else { format!("{base}{n}") })
        .find(|k| !taken(k))
        .expect("some key is free")
}

/// A fresh area for `*F` insert (`faReset`).
fn new_file_area(areas: &FileAreas) -> FileArea {
    let file = unused_key(areas, "NEWAREA");
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
                        } else if !fr.name.to_ascii_lowercase().ends_with(".zip") {
                            "Only ZIP archives can be changed.".into()
                        } else {
                            match zip_set_description(&path, &name, &fr.description) {
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
                        match zip_description(&dir.join(&fr.name), &names) {
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
        assert_eq!(zip_description(&path, &["FILE_ID.DIZ".into()]).unwrap(), lines);
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
