//! File areas (`FileArea.pas`): areas, the file lister, downloads and
//! batches, uploads, searches and newscans.

use std::io;
use std::path::{Path, PathBuf};

use fluxbbs_term::codes::{resize, squish};
use fluxbbs_term::{Color, Slot, lit};

use crate::editor::EditCtx;
use crate::fileareas::{FileArea, children_of, subtree_of};
use crate::filebase::{FileRec, wildcard};
use crate::input::{DIGITS, Key, ReadOpts};
use crate::session::{BarPick, BarRow, BarSpec, Session, Waves, waves};
use crate::transfer::{Proto, safe_name};
use crate::user::{fmt_date, parse_date};

pub const MAX_BATCH: usize = 20;
pub(crate) const MAX_DESC_LINES: usize = 20;
pub(crate) const DESC_WIDTH: usize = 70;
const MAX_UPLOAD_NAMES: usize = 60;

/// A file area being worked on: its number and definition.
#[derive(Clone)]
pub struct FAreaRef {
    pub num: u32,
    pub area: FileArea,
}

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// `Stc`: a number with thousands separators.
pub fn commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A file's description from FILE_ID.DIZ (or another named file) inside a
/// ZIP archive.
pub fn zip_description(path: &Path, names: &[String]) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    for want in names {
        let idx = (0..zip.len()).find(|&i| zip.by_index(i).is_ok_and(|e| e.name().is_ok_and(|n| n.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(want)))))?;
        let mut entry = zip.by_index(idx).ok()?;
        let mut raw = Vec::new();
        io::Read::read_to_end(&mut entry, &mut raw).ok()?;
        let lines = parse_description(&raw);
        if !lines.is_empty() {
            return Some(lines);
        }
    }
    None
}

/// A description from the SAUCE of a single piece of art: the file
/// itself, or the only file in a ZIP (bar any description file).
pub fn art_description(path: &Path, desc_files: &[String]) -> Option<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    // Enough for a SAUCE record and a full comment block.
    const TAIL: u64 = 128 + 5 + 255 * 64;
    let data = if is_zip(path) {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let files: Vec<usize> = (0..zip.len())
            .filter(|&i| {
                zip.by_index(i).is_ok_and(|e| {
                    let name = e.name().unwrap_or_default().rsplit('/').next().unwrap_or_default().to_string();
                    e.is_file() && !desc_files.iter().any(|d| d.eq_ignore_ascii_case(&name))
                })
            })
            .collect();
        let [only] = files[..] else { return None };
        let entry = zip.by_index(only).ok()?;
        if !crate::gallery::is_art(&entry.name().ok()?) {
            return None;
        }
        let mut raw = Vec::new();
        entry.take(8 << 20).read_to_end(&mut raw).ok()?;
        raw
    } else {
        let mut f = std::fs::File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        f.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
        let mut raw = Vec::new();
        f.read_to_end(&mut raw).ok()?;
        raw
    };
    crate::gallery::sauce_description(&data, DESC_WIDTH, MAX_DESC_LINES)
}

/// Description lines from a FILE_ID.DIZ-style file.
pub fn parse_description(raw: &[u8]) -> Vec<String> {
    // Split before decoding: CP437 decoding turns CR/LF into glyphs.
    let raw = raw.split(|&b| b == 0x1a).next().unwrap_or_default();
    let mut lines: Vec<String> = raw
        .split(|&b| b == b'\n')
        .map(|l| {
            let l: Vec<u8> = l.iter().copied().filter(|&b| b >= 0x20 || b == 0x1b).collect();
            fluxbbs_term::cp437::decode(&l).trim_end().chars().take(DESC_WIDTH).collect()
        })
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.truncate(MAX_DESC_LINES);
    lines
}

/// ZIPs are handled natively; everything else goes to an archiver.
pub fn is_zip(path: &Path) -> bool {
    let mut head = [0u8; 4];
    let n = std::fs::File::open(path).and_then(|mut f| io::Read::read(&mut f, &mut head)).unwrap_or(0);
    &head[..n] == b"PK\x03\x04" || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// The file lister's screen layout (from LISTFILE.ANS, or built in).
/// Where things go on LISTFILE (and GALLERY, which shares its markers).
pub(crate) struct Layout {
    pub(crate) top: u16,
    pub(crate) bot: u16,
    pub(crate) menu_y: u16,
    pub(crate) info_y: u16,
    /// Where the menu and info lines start (their markers' columns).
    menu_x: u16,
    info_x: u16,
    /// New: `~PC`, where the cursor waits (a prompt bar's slot).
    cursor: Option<(u16, u16)>,
    num_x: Option<u16>,
    pub(crate) name_x: Option<u16>,
    pub(crate) size_x: Option<u16>,
    pts_x: Option<u16>,
    pub(crate) desc_x: Option<u16>,
    /// Normal, dim, bright (bracket/hot key), highlight, dim highlight,
    /// bright highlight.
    pub(crate) cl: [Color; 6],
    /// Scroll bar: start x, end x, y, empty char/colour, full char/colour.
    scroll: Option<(u16, u16, u16, char, Color, char, Color)>,
}

impl Layout {
    /// Room for a name before the next column (less the flag brackets).
    pub(crate) fn name_width(&self) -> usize {
        let x = self.name_x.unwrap_or(2);
        let next = [self.size_x, self.pts_x, self.desc_x].into_iter().flatten().filter(|&n| n > x).min().unwrap_or(80);
        (next - x).saturating_sub(2).max(8) as usize
    }
}

pub(crate) struct MenuBar {
    pub(crate) items: &'static [&'static str],
    pub(crate) off: Vec<bool>,
    pub(crate) pos: usize,
    pub(crate) xs: Vec<u16>,
}

/// One file shown on the current page.
struct Row {
    idx: usize,
    y: u16,
}

/// One row of the file area list.
struct FaRow {
    label: u32,
    n: u32,
    name: String,
    count: String,
    sponsor: String,
    newest: String,
}

impl FaRow {
    fn waves(&self) -> Waves {
        waves([("AT", self.name.clone()), ("AN", self.label.to_string()), ("NF", self.count.clone()), ("SP", self.sponsor.clone()), ("NW", self.newest.clone())])
    }

    fn plain(&self) -> String {
        format!(" {} {} {} {}", resize(&self.label.to_string(), 6), resize(&self.name, 30), resize(&self.count, 7), squish(&self.sponsor, 36))
    }
}

impl Session {
    // ---- areas ----------------------------------------------------------

    pub fn farea(&self, n: u32) -> Option<FAreaRef> {
        self.state.fareas().get(n).map(|a| FAreaRef { num: n, area: a.clone() })
    }

    pub fn current_farea(&self) -> Option<FAreaRef> {
        self.farea(self.user.cur_file_area)
    }

    pub(crate) fn farea_dir(&self, a: &FileArea) -> PathBuf {
        self.state.config().resolve(&a.path)
    }

    fn farea_count(&self) -> u32 {
        self.state.fareas().areas.len() as u32
    }

    /// `faChangeArea`.
    pub async fn change_farea(&mut self, n: u32) -> io::Result<bool> {
        let Some(a) = self.farea(n) else { return Ok(false) };
        if !self.farea_visible(n) {
            return Ok(false);
        }
        if !a.area.password.is_empty() {
            self.say("FaAskPassword").await?;
            let pw = self.read_string(ReadOpts::new(20).password()).await?;
            self.newline();
            if !pw.eq_ignore_ascii_case(&a.area.password) {
                return Ok(false);
            }
        }
        self.user.cur_file_area = n;
        Ok(true)
    }

    /// `faFindAreaWithAccess`.
    pub fn find_farea_with_access(&mut self) {
        let visible = self.visible_fareas();
        if visible.iter().any(|&(n, _)| n == self.user.cur_file_area) {
            return;
        }
        if let Some(&(n, _)) = visible.first() {
            self.user.cur_file_area = n;
        }
    }

    /// `faPrevArea` / `faNextArea`.
    /// Areas go in tree order, so this walks into and out of sub-areas.
    pub async fn step_farea(&mut self, forward: bool) -> io::Result<()> {
        let order: Vec<u32> = self.state.fareas().tree().into_iter().map(|(n, _)| n).collect();
        let candidates: Vec<u32> = match order.iter().position(|&n| n == self.user.cur_file_area) {
            Some(p) if forward => order[p + 1..].to_vec(),
            Some(p) => order[..p].iter().rev().copied().collect(),
            None if forward => order,
            None => Vec::new(),
        };
        for n in candidates {
            if self.change_farea(n).await? {
                let name = self.farea(n).map(|a| a.area.name).unwrap_or_default();
                let s = self.strings().fmt("FaChangedAreas", &[&name]);
                return self.show_line(&s).await;
            }
        }
        self.say_line(if forward { "FaHighestArea" } else { "FaLowestArea" }).await
    }

    /// The file areas the caller can use, as (number, depth) in tree
    /// order. A sub-area needs the access of every area above it.
    pub(crate) fn visible_fareas(&self) -> Vec<(u32, usize)> {
        let fa = self.state.fareas();
        let mut ok_at: Vec<bool> = Vec::new();
        let mut out = Vec::new();
        for (n, depth) in fa.tree() {
            ok_at.truncate(depth);
            let ok = ok_at.last().copied().unwrap_or(true) && fa.get(n).is_some_and(|a| self.acs(&a.acs));
            ok_at.push(ok);
            if ok {
                out.push((n, depth));
            }
        }
        out
    }

    pub(crate) fn farea_visible(&self, n: u32) -> bool {
        self.visible_fareas().iter().any(|&(m, _)| m == n)
    }

    /// Visible sub-areas directly under `of` (top-level areas for `None`).
    pub(crate) fn farea_children(&self, of: Option<u32>) -> Vec<u32> {
        children_of(&self.visible_fareas(), of)
    }

    /// (number shown, area number) for each file area the caller can use.
    pub(crate) fn file_area_labels(&self) -> Vec<(u32, u32)> {
        let compress = self.state.config().messages.compress_area_numbers;
        let mut out = Vec::new();
        for (n, _) in self.visible_fareas() {
            out.push((if compress { out.len() as u32 + 1 } else { n }, n));
        }
        out
    }

    /// `faListAreas` (`FA`, and `FC` without a number). `FA` lists every
    /// area, sub-areas indented under theirs; `FC` shows one level at a
    /// time from `start` (the top for `None`). Choosing an area with
    /// sub-areas changes to it and opens it; Enter stays where you are.
    pub async fn list_fareas(&mut self, change: bool, start: Option<u32>) -> io::Result<()> {
        let mut level = start;
        loop {
            let fareas = self.state.fareas();
            let labels = self.file_area_labels();
            // Worked out once: with an archive's hundreds of areas, asking
            // per row would take seconds.
            let visible = self.visible_fareas();
            let shown: Vec<(u32, usize)> = if change {
                children_of(&visible, level).into_iter().map(|n| (n, 0)).collect()
            } else {
                visible.clone()
            };
            let suffix = self.strings().get("FaGroupSuffix").to_string();
            let mut rows = Vec::new();
            for (n, depth) in shown {
                let label = labels.iter().find(|&&(_, m)| m == n).map_or(n, |l| l.0);
                let subs = children_of(&visible, Some(n));
                // One level at a time, a group counts everything under it;
                // in the full list each area counts its own.
                let counted = if change { subtree_of(&visible, n) } else { vec![n] };
                let mut count = 0;
                let mut newest: Option<chrono::NaiveDate> = None;
                for m in counted {
                    if let Some(a) = fareas.get(m) {
                        let (c, latest) = self.state.files.count_and_newest(&a.file).map_err(io_err)?;
                        count += c;
                        newest = newest.max(latest);
                    }
                }
                let newest = newest.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
                let mut name = format!("{}{}", "  ".repeat(depth), fareas.areas[n as usize - 1].name);
                if !subs.is_empty() {
                    name.push_str(&suffix);
                }
                rows.push(FaRow { label, n, name, count: count.to_string(), sponsor: fareas.areas[n as usize - 1].sponsor.clone(), newest });
            }
            if change {
                let here = self.user.cur_file_area;
                let start = rows.iter().position(|r| subtree_of(&visible, r.n).contains(&here)).unwrap_or(0);
                let bar: Vec<BarRow> = rows.iter().map(|r| BarRow { n: r.n, waves: r.waves(), plain: r.plain() }).collect();
                let help = match level.and_then(|g| self.farea(g)) {
                    Some(g) => self.strings().fmt("FaBarInGroup", &[&g.area.name]),
                    None => self.strings().get("FaBarKeys").to_string(),
                };
                let header = " Num    Area Title                     Files   Sponsor";
                let spec = BarSpec { prefix: "FAREA", header, rows: &bar, labels: &labels, start, help: &help, ask: "FaAskChangeArea", can_go_up: level.is_some(), keys: "" };
                let pick = match self.area_bar(&spec).await? {
                    BarPick::Area(n) => n,
                    BarPick::Up => {
                        level = level.and_then(|g| fareas.parent_of(g));
                        continue;
                    }
                    BarPick::Stay | BarPick::Quit | BarPick::Key(..) => {
                        if level.is_some() {
                            self.announce_farea().await?;
                        }
                        return Ok(());
                    }
                };
                if !self.change_farea(pick).await? {
                    return Ok(());
                }
                if children_of(&visible, Some(pick)).is_empty() {
                    return self.announce_farea().await;
                }
                level = Some(pick);
                continue;
            }
            let mut rep = self.repeat_begin("FAREA", true).await?;
            if rep.is_none() {
                self.clear_screen();
                self.newline();
                self.set_slot(Slot::Info);
                self.show_line(" Num    Area Title                     Files   Sponsor").await?;
                self.rule().await?;
            }
            let mut lines = 4;
            for row in &rows {
                let here = row.n == self.user.cur_file_area;
                let more = match rep.as_mut() {
                    Some(r) => self.repeat_row(r, &row.waves(), here).await?,
                    None => {
                        self.set_slot(if here { Slot::TextHi } else { Slot::Text });
                        self.write(&row.plain());
                        self.newline();
                        lines += 1;
                        self.more(&mut lines).await?
                    }
                };
                if !more {
                    break;
                }
            }
            match rep {
                Some(r) => self.repeat_end(r).await?,
                None => self.rule().await?,
            }
            return Ok(());
        }
    }

    async fn announce_farea(&mut self) -> io::Result<()> {
        let name = self.current_farea().map(|a| a.area.name).unwrap_or_default();
        let s = self.strings().fmt("FaChangedAreas", &[&name]);
        self.show_line(&s).await
    }

    // ---- finding files --------------------------------------------------

    /// `faFileSearch`: a file by name (wildcards allowed) in the current
    /// area, then any other area the caller can open without a password.
    async fn search_file(&mut self, name: &str) -> io::Result<Option<(FAreaRef, FileRec)>> {
        let mut order: Vec<u32> = vec![self.user.cur_file_area];
        order.extend((1..=self.farea_count()).filter(|&n| n != self.user.cur_file_area));
        for n in order {
            let Some(a) = self.farea(n) else { continue };
            if !self.acs(&a.area.acs) || (n != self.user.cur_file_area && !a.area.password.is_empty()) {
                continue;
            }
            let files = self.state.files.list(&a.area.file).map_err(io_err)?;
            if let Some(f) = files.into_iter().find(|f| wildcard(name, &f.name)) {
                return Ok(Some((a, f)));
            }
        }
        Ok(None)
    }

    /// Ask for (or take) a file name and find it, with the original's
    /// messages. `ask` is the prompt string.
    async fn locate_file(&mut self, param: &str, ask: &str) -> io::Result<Option<(FAreaRef, FileRec)>> {
        self.say(ask).await?;
        let name = if param.trim().is_empty() {
            let s = self.read_string(ReadOpts::new(128)).await?;
            self.newline();
            s
        } else {
            self.write(&param.to_uppercase());
            self.newline();
            param.trim().to_string()
        };
        if name.is_empty() {
            return Ok(None);
        }
        self.say("FaSearching").await?;
        let found = self.search_file(&name).await?;
        self.newline();
        if found.is_none() {
            self.say_line("FaFileNotFound").await?;
        }
        Ok(found)
    }

    // ---- file information -----------------------------------------------

    fn xfer_secs(&self, size: u64) -> u64 {
        size / self.state.config().files.transfer_cps.max(1)
    }

    /// `faFileInfo`.
    pub(crate) async fn file_info(&mut self, f: &FileRec) -> io::Result<()> {
        let date = fmt_date(f.date);
        let ul = fmt_date(f.uploaded);
        let mins = (self.xfer_secs(f.size) / 60).to_string();
        let mut w: Waves = waves([
            ("FN", f.name.clone()),
            ("SZ", commas(f.size)),
            ("DT", date.clone()),
            ("UL", f.uploader.clone()),
            ("UD", ul.clone()),
            ("DL", commas(f.downloads as u64)),
            ("TM", mins.clone()),
            ("FP", f.points.to_string()),
        ]);
        let no_desc = self.state.config().files.no_description.clone();
        for i in 1..=99u8 {
            let line = match f.description.get(i as usize - 1) {
                Some(l) => l.clone(),
                None if i == 1 => no_desc.clone(),
                None => String::new(),
            };
            w.insert(crate::session::pos_code(i), line);
        }
        // Older screens (Menuset #1) have no ~01.. lines: the description
        // goes under them.
        let has_desc = self.find_text_file("FILEINFO").and_then(|p| std::fs::read(p).ok()).is_some_and(|d| d.windows(3).any(|w| w == b"~01"));
        if self.show_file_with("FILEINFO", &w).await? {
            if !has_desc {
                for l in &f.description {
                    self.write_with_controls(&format!("|U1 {l}"));
                    self.newline();
                }
            }
        } else {
            let rows = [
                format!("|U5{}", "─".repeat(79)),
                format!("|U4 Filename      |U5: |U6{}", f.name),
                format!("|U4 Downloads     |U5: |U6{}", commas(f.downloads as u64)),
                format!("|U4 Size          |U5: |U6{}", commas(f.size)),
                format!("|U4 Uploader      |U5: |U6{}", f.uploader),
                format!("|U4 Date          |U5: |U6{date}"),
                format!("|U4 Upload date   |U5: |U6{ul}"),
                format!("|U4 Transfer time |U5: |U6{mins} |U4min"),
                format!("|U5{}", "─".repeat(79)),
            ];
            for r in rows {
                self.write_with_controls(&r);
                self.newline();
            }
            // New: plain text gives its link, for comments and messages.
            if self.plain() {
                let link = crate::links::link_text(&f.area, &f.name);
                self.write(&format!(" Link          : {link}"));
                self.newline();
            }
            for l in &f.description {
                self.write_with_controls(&format!("|U1 {l}"));
                self.newline();
            }
        }
        Ok(())
    }

    // ---- downloading ----------------------------------------------------

    /// `faReqDownload`: may this file be downloaded, on top of `add`
    /// (seconds, files, kb, points) already selected?
    async fn may_download(&mut self, a: &FAreaRef, f: &FileRec, add: (u64, u32, u64, u32)) -> io::Result<bool> {
        let fc = &self.state.config().files;
        let (use_points, use_limit, use_kb_limit, use_ratio, use_kb_ratio) =
            (fc.use_file_points, fc.use_daily_limit, fc.use_daily_kb_limit, fc.use_ratio, fc.use_kb_ratio);
        let u = self.user.clone();
        let fail = |key: &'static str, args: Vec<String>| (key, args);
        let problem = if !self.acs(&a.area.acs) || !self.acs(&a.area.download_acs) {
            Some(fail("FaDLaccessDenied", vec![f.name.clone()]))
        } else if !self.farea_dir(&a.area).join(&f.name).is_file() {
            Some(fail("FaFileNotThere", vec![f.name.clone()]))
        } else if !f.valid && !self.acs(&self.state.config().acs.cosysop.clone()) {
            Some(fail("FaFileNotValid", vec![f.name.clone()]))
        } else if (self.minutes_left() as u64 * 60).saturating_sub(add.0) < self.xfer_secs(f.size) {
            Some(fail("FaNoTimeForDL", vec![f.name.clone()]))
        } else if use_points && f.points + add.3 > u.file_points {
            Some(fail("FaNoFilePts", vec![u.file_points.to_string(), f.points.to_string(), f.name.clone()]))
        } else if use_limit && u.today_dl + add.1 >= u.limit_dl {
            Some(fail("FaOverDLlimit", vec![u.limit_dl.to_string(), f.name.clone()]))
        } else if use_kb_limit && u.today_dl_kb as u64 + add.2 >= u.dl_kb_allowance() {
            Some(fail("FaOverDLkbLimit", vec![u.dl_kb_allowance().to_string(), f.name.clone()]))
        } else if use_ratio && u.downloads + add.1 >= u.uploads * u.uldl_ratio + u.uldl_ratio {
            Some(fail("FaULDLratioBad", vec![f.name.clone()]))
        } else if use_kb_ratio && u.download_kb + add.2 >= u.upload_kb * u.kb_ratio as u64 + u.kb_ratio as u64 {
            Some(fail("FaKBratioBad", vec![f.name.clone()]))
        } else {
            None
        };
        if let Some((key, args)) = problem {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let s = self.strings().fmt(key, &args);
            self.show_line(&s).await?;
            tracing::info!("{}: download of {} refused ({key})", self.user.handle, f.name);
            return Ok(false);
        }
        Ok(true)
    }

    /// "Press enter to download, escape to abort", then the protocol.
    async fn confirm_and_pick(&mut self, batch: bool) -> io::Result<Option<Proto>> {
        self.say("FaDownloadPrompt").await?;
        loop {
            match self.read_key().await? {
                Key::Enter => break,
                Key::Esc => {
                    self.write("Abort");
                    self.newline();
                    return Ok(None);
                }
                k => match k.upper() {
                    Some('D' | 'C') => break,
                    Some('A') => {
                        self.write("Abort");
                        self.newline();
                        return Ok(None);
                    }
                    _ => {}
                },
            }
        }
        self.write("Continue");
        self.newline();
        self.select_protocol(batch).await
    }

    /// Credit a completed download.
    fn count_download(&mut self, f: &mut FileRec) -> io::Result<()> {
        let kb = (f.size / 1024) as u32;
        let u = &mut self.user;
        u.downloads += 1;
        u.download_kb += kb as u64;
        u.today_dl += 1;
        u.today_dl_kb += kb;
        if self.state.config().files.use_file_points {
            u.file_points = u.file_points.saturating_sub(f.points);
        }
        f.downloads += 1;
        let area = self.state.fareas().areas.iter().find(|a| a.file.eq_ignore_ascii_case(&f.area)).map_or_else(|| f.area.clone(), |a| crate::editor::strip_colours(&a.name));
        *self.seen.downloads.entry(area).or_default() += 1;
        self.bump(crate::sysdata::Stat::Download { kb: kb as u64 });
        self.state.files.update(f).map_err(io_err)?;
        self.save_own_user().map_err(io_err)?;
        Ok(())
    }

    /// Receive one text file (a prepared message) and give its lines, or
    /// None if nothing came.
    pub async fn receive_text_file(&mut self) -> io::Result<Option<Vec<String>>> {
        let Some(proto) = self.select_protocol(false).await? else { return Ok(None) };
        let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("text{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&temp);
        let got = self.receive_files(&proto, &temp).await?;
        let lines = got.first().and_then(|p| std::fs::read(p).ok()).map(|b| text_lines(&b));
        let _ = std::fs::remove_dir_all(&temp);
        Ok(lines.filter(|l| !l.is_empty()))
    }

    /// `s272`: ask before a download whether to log off after it.
    async fn ask_auto_logoff(&mut self) -> io::Result<bool> {
        if !self.state.config().files.ask_auto_logoff {
            return Ok(false);
        }
        self.say("s272").await?;
        self.yes_no(false).await
    }

    /// `s273`/`s274`: the countdown after a transfer. Enter stays online,
    /// `H` or the time running out hangs up.
    async fn auto_logoff(&mut self) -> io::Result<()> {
        let secs = self.state.config().files.auto_logoff_seconds.max(1) as u64;
        let s = self.strings().fmt("s273", &[&secs.to_string()]);
        self.show_line(&s).await?;
        self.flush().await?;
        let stay = tokio::select! {
            k = self.read_key() => !matches!(k?, Key::Char(b'h' | b'H')),
            _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => false,
        };
        if !stay {
            self.say_line("s274").await?;
            tracing::info!("{}: auto-logoff after transfer", self.user.handle);
            self.hangup = true;
        }
        Ok(())
    }

    /// `faDownload` (`FD`): the batch if there is one (and no file was
    /// named), otherwise one file.
    pub async fn download(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        if param.trim().is_empty() && !self.batch.is_empty() {
            let s = self.strings().fmt("FaBatchAskDL", &[&self.batch.len().to_string()]);
            self.show(&s).await?;
            if self.yes_no(true).await? {
                return self.download_batch().await;
            }
        }
        let Some((a, f)) = self.locate_file(param, "FaAskFileDownload").await? else { return Ok(()) };
        if a.num != self.user.cur_file_area {
            let s = self.strings().fmt("FaFoundInArea", &[&f.name, &a.area.name]);
            self.show_line(&s).await?;
        }
        self.file_info(&f).await?;
        self.download_record(&a, f).await
    }

    /// Download one file of an area, with the usual checks (access,
    /// points, limits, ratios) and accounting.
    pub(crate) async fn download_record(&mut self, a: &FAreaRef, mut f: FileRec) -> io::Result<()> {
        if !self.may_download(a, &f, (0, 0, 0, 0)).await? {
            return Ok(());
        }
        let Some(proto) = self.confirm_and_pick(false).await? else { return Ok(()) };
        let logoff = self.ask_auto_logoff().await?;
        let path = self.farea_dir(&a.area).join(&f.name);
        let ok = self.send_files(&proto, std::slice::from_ref(&path)).await?;
        if ok.first() == Some(&true) {
            self.count_download(&mut f)?;
            tracing::info!("{}: downloaded {} ({})", self.user.handle, f.name, a.area.name);
            self.say_line("FaXferSuccess").await?;
            if logoff {
                self.auto_logoff().await?;
            }
        } else {
            tracing::info!("{}: download of {} failed", self.user.handle, f.name);
            self.say_line("FaXferFailed").await?;
        }
        Ok(())
    }

    async fn download_batch(&mut self) -> io::Result<()> {
        self.newline();
        let mut picked: Vec<(FAreaRef, FileRec)> = Vec::new();
        let (mut secs, mut kb, mut pts, mut size) = (0u64, 0u64, 0u32, 0u64);
        for (an, fnum) in self.batch.clone() {
            let (Some(a), Some(f)) = (self.farea(an), self.farea(an).and_then(|a| self.state.files.get(&a.area.file, fnum).ok().flatten())) else {
                continue;
            };
            if self.may_download(&a, &f, (secs, picked.len() as u32, kb, pts)).await? {
                secs += self.xfer_secs(f.size);
                kb += f.size / 1024;
                pts += f.points;
                size += f.size;
                let s = self.strings().fmt("FaBatchFileDL", &[&f.name, &commas(f.size), &fmt_secs(self.xfer_secs(f.size))]);
                self.show_line(&s).await?;
                picked.push((a, f));
            }
        }
        if picked.is_empty() {
            return Ok(());
        }
        self.newline();
        let s = self.strings().fmt("FaBatchDLtotal", &[&picked.len().to_string(), &commas(size), &fmt_secs(secs)]);
        self.show_line(&s).await?;
        self.newline();
        let Some(proto) = self.confirm_and_pick(picked.len() > 1).await? else { return Ok(()) };
        let logoff = self.ask_auto_logoff().await?;
        let paths: Vec<PathBuf> = picked.iter().map(|(a, f)| self.farea_dir(&a.area).join(&f.name)).collect();
        let ok = self.send_files(&proto, &paths).await?;
        let mut sent = (0u32, 0u64);
        for ((_, f), good) in picked.iter_mut().zip(ok) {
            if good {
                self.count_download(f)?;
                sent.0 += 1;
                sent.1 += f.size;
            }
        }
        self.newline();
        if sent.0 == 0 {
            self.say_line("FaXferFailed").await?;
        } else {
            tracing::info!("{}: batch download, {} files, {} bytes", self.user.handle, sent.0, sent.1);
            self.say_line("FaXferSuccess").await?;
            let s = self.strings().fmt("FaXferFilesSent", &[&sent.0.to_string(), &commas(sent.1)]);
            self.show_line(&s).await?;
            self.batch.clear();
            if logoff {
                self.auto_logoff().await?;
            }
        }
        Ok(())
    }

    // ---- the batch ------------------------------------------------------

    /// 0 added, 1 full, 2 already there (`faAddToBatch`).
    fn add_to_batch(&mut self, area: u32, num: u32) -> u8 {
        if self.batch.contains(&(area, num)) {
            2
        } else if self.batch.len() >= MAX_BATCH {
            1
        } else {
            self.batch.push((area, num));
            0
        }
    }

    /// `faBatchAdd` (`F+`).
    pub async fn flag_file(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        let Some((a, f)) = self.locate_file(param, "FaBatchAskAdd").await? else { return Ok(()) };
        let key = match self.add_to_batch(a.num, f.num) {
            0 => {
                let s = self.strings().fmt("FaBatchAdded", &[&f.name]);
                return self.show_line(&s).await;
            }
            1 => "FaBatchFull",
            _ => "FaBatchAlready",
        };
        self.say_line(key).await
    }

    /// `FB`: the download batch editor (r5).
    pub async fn batch_editor(&mut self) -> io::Result<()> {
        loop {
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line("|U4 Num  Filename                   Size          Area").await?;
            self.rule().await?;
            for (i, (an, fnum)) in self.batch.clone().into_iter().enumerate() {
                let Some(a) = self.farea(an) else { continue };
                let Some(f) = self.state.files.get(&a.area.file, fnum).map_err(io_err)? else { continue };
                let line = format!("|U1 {} {} {} {}", resize(&(i + 1).to_string(), 4), resize(&f.name, 26), resize(&commas(f.size), 13), a.area.name);
                self.show_line(&line).await?;
            }
            self.rule().await?;
            self.say_line("s284").await?;
            let key = self.read_key().await?;
            match (key, key.upper()) {
                (Key::Esc, _) | (Key::Enter, _) | (_, Some('Q')) => return Ok(()),
                (_, Some('A')) => {
                    self.flag_file("").await?;
                }
                (_, Some('R')) => {
                    self.show("|U1Remove which|U2? |U3").await?;
                    let n: usize = self.read_string(ReadOpts::new(2).allowed(DIGITS)).await?.parse().unwrap_or(0);
                    self.newline();
                    if (1..=self.batch.len()).contains(&n) {
                        let (an, fnum) = self.batch.remove(n - 1);
                        let name = self.farea(an).and_then(|a| self.state.files.get(&a.area.file, fnum).ok().flatten()).map(|f| f.name).unwrap_or_default();
                        let s = self.strings().fmt("s285", &[&name]);
                        self.show_line(&s).await?;
                    }
                }
                (_, Some('C')) => {
                    self.batch.clear();
                    self.say_line("s286").await?;
                }
                _ => {}
            }
        }
    }

    // ---- the file lister ------------------------------------------------

    pub(crate) fn lister_layout(&self, from_screen: bool) -> Layout {
        let p = |c: &[u8; 2]| self.positions.get(c).copied();
        let pal = &self.user.palette;
        let default_cl = [pal.0[1], pal.0[2], pal.0[3], pal.0[4], pal.0[5], pal.0[6]];
        if !from_screen {
            return Layout {
                top: 3,
                bot: 20,
                menu_y: 22,
                info_y: 21,
                menu_x: 1,
                info_x: 1,
                cursor: None,
                num_x: Some(1),
                name_x: Some(6),
                size_x: Some(20),
                pts_x: None,
                desc_x: Some(27),
                cl: default_cl,
                scroll: None,
            };
        }
        let mut cl = default_cl;
        for (i, code) in [b"C1", b"C2", b"C3", b"C4", b"C5", b"C6"].iter().enumerate() {
            if let Some((_, _, c)) = p(code) {
                cl[i] = c;
            }
        }
        // ~S? and ~E? mark the scroll bar's ends; the second character is
        // the bar's fill (empty / full).
        let find = |first: u8| self.positions.iter().find(|(k, _)| k[0] == first && !k[1].is_ascii_digit()).map(|(k, v)| (k[1], *v));
        let scroll = match (find(b'S'), find(b'E')) {
            (Some((a, (sx, sy, sc))), Some((b, (ex, _, ec)))) if ex >= sx => {
                let ch = |c: u8| fluxbbs_term::cp437::TO_UNICODE[c as usize];
                Some((sx, ex, sy, ch(a), sc, ch(b), ec))
            }
            _ => None,
        };
        let top = p(b"TL").map_or(3, |q| q.1);
        let bot = p(b"BL").map_or(20, |q| q.1).max(top);
        Layout {
            top,
            bot,
            menu_y: p(b"PM").map_or(bot + 2, |q| q.1),
            info_y: p(b"PI").map_or(bot + 1, |q| q.1),
            menu_x: p(b"PM").map_or(1, |q| q.0),
            info_x: p(b"PI").map_or(1, |q| q.0),
            cursor: p(b"PC").map(|q| (q.0, q.1)),
            num_x: p(b"FI").map(|q| q.0),
            name_x: p(b"FN").map(|q| q.0),
            size_x: p(b"FS").map(|q| q.0),
            pts_x: p(b"FP").map(|q| q.0),
            desc_x: p(b"FD").map(|q| q.0),
            cl,
            scroll,
        }
    }

    fn lister_name(&mut self, l: &Layout, f: &FileRec, y: u16, hi: bool, flagged: bool) {
        let Some(x) = l.name_x else { return };
        // A name too long for the column gets the row to itself.
        let shown = lister_shown_name(&f.name, l.name_width()).unwrap_or_else(|| squish(&f.name, 78usize.saturating_sub(x as usize)));
        let (text, mark) = if hi { (l.cl[3], l.cl[5]) } else { (l.cl[0], l.cl[2]) };
        self.screen.goto(x.saturating_sub(1).max(1), y);
        self.screen.set_color(if flagged { mark } else { text });
        self.write(if flagged { "(" } else { " " });
        self.screen.set_color(text);
        self.write(&resize(&shown, shown.chars().count().max(12)));
        self.screen.set_color(if flagged { mark } else { text });
        self.write(if flagged { ")" } else { " " });
    }

    pub(crate) fn lister_menu(&mut self, l: &Layout, m: &mut MenuBar) {
        self.screen.goto(l.menu_x, l.menu_y);
        self.screen.set_color(l.cl[0]);
        self.screen.clear_eol();
        self.write("  ");
        m.xs.clear();
        for i in 0..m.items.len() {
            m.xs.push(self.screen.where_x());
            self.lister_menu_item(l, m, i);
        }
        self.screen.set_color(l.cl[0]);
    }

    pub(crate) fn lister_menu_item(&mut self, l: &Layout, m: &MenuBar, i: usize) {
        if let Some(&x) = m.xs.get(i) {
            self.screen.goto(x, l.menu_y);
        }
        let sel = m.pos == i;
        let (first, rest) = match (m.off[i], sel) {
            (true, true) => (l.cl[3], l.cl[4]),
            (true, false) => (l.cl[0], l.cl[1]),
            (false, true) => (l.cl[5], l.cl[3]),
            (false, false) => (l.cl[2], l.cl[0]),
        };
        let item = m.items[i];
        self.screen.set_color(first);
        self.write(&format!(" {}", &item[..1]));
        self.screen.set_color(rest);
        self.write(&format!("{} ", &item[1..]));
    }

    pub(crate) async fn lister_info(&mut self, l: &Layout, text: &str) -> io::Result<()> {
        self.screen.goto(l.info_x, l.info_y);
        self.screen.set_color(l.cl[0]);
        self.screen.clear_eol();
        // Kept to its row: a prompt bar may be under it.
        let room = 80usize.saturating_sub(l.info_x as usize);
        self.show(&fluxbbs_term::codes::squish(text, room)).await
    }

    /// Put the cursor where the screen says it waits (`~PC`), if it does.
    pub(crate) fn lister_park(&mut self, l: &Layout) {
        if let Some((x, y)) = l.cursor {
            self.screen.goto(x, y);
        }
    }

    /// Draw a page of files from `first`; returns the rows shown.
    fn lister_page(&mut self, l: &Layout, files: &[FileRec], first: usize, area: u32) -> Vec<Row> {
        for y in l.top..=l.bot {
            self.screen.goto(1, y);
            self.screen.set_color(l.cl[0]);
            self.screen.clear_eol();
        }
        let lines = (l.bot - l.top + 1) as usize;
        let no_desc = self.state.config().files.no_description.clone();
        let mut rows = Vec::new();
        let mut used = 0;
        for (idx, f) in files.iter().enumerate().skip(first) {
            let desc: Vec<String> = if f.description.is_empty() { vec![no_desc.clone()] } else { f.description.clone() };
            // A long name has its own row; the rest starts below it.
            let below = lister_shown_name(&f.name, l.name_width()).is_none() as usize;
            let need = desc.len().max(1) + below;
            // A file always gets the first row, even if its description
            // must be cut short.
            if used > 0 && used + need > lines {
                break;
            }
            let y = l.top + used as u16;
            let flagged = self.batch.contains(&(area, f.num));
            if let Some(x) = l.num_x {
                self.screen.goto(x, y);
                self.screen.set_color(l.cl[0]);
                self.write(&f.num.to_string());
            }
            self.lister_name(l, f, y, false, flagged);
            if below > 0 && used + 1 >= lines {
                rows.push(Row { idx, y });
                break;
            }
            let (y, used_name) = (y + below as u16, used + below);
            if let Some(x) = l.size_x {
                self.screen.goto(x, y);
                self.screen.set_color(l.cl[0]);
                self.write(&short_size(f.size));
            }
            if let Some(x) = l.pts_x {
                self.screen.goto(x, y);
                self.screen.set_color(l.cl[0]);
                self.write(&f.points.to_string());
            }
            if let Some(x) = l.desc_x {
                for (i, d) in desc.iter().take(lines - used_name).enumerate() {
                    self.screen.goto(x, y + i as u16);
                    self.screen.set_color(l.cl[0]);
                    self.write_with_controls(&squish(d, 79usize.saturating_sub(x as usize)));
                }
            }
            rows.push(Row { idx, y: y - below as u16 });
            used += need.min(lines - used);
            if used >= lines {
                break;
            }
        }
        rows
    }

    pub(crate) fn lister_scroll(&mut self, l: &Layout, first: usize, last: usize, total: usize) {
        let Some((sx, ex, y, empty, ec, full, fc)) = l.scroll else { return };
        let len = (ex - sx + 1) as usize;
        let a = if first == 0 { 1 } else { (len * (first + 1) / total).max(1) };
        let b = if last + 1 >= total { len } else { (len * (last + 1) / total).clamp(a, len) };
        self.screen.goto(sx, y);
        for i in 1..=len {
            let on = (a..=b).contains(&i);
            self.screen.set_color(if on { fc } else { ec });
            self.write(&if on { full } else { empty }.to_string());
        }
    }

    /// `faListFiles`: browse files with the LISTFILE screen. Returns false
    /// if the caller quit (ending a multi-area scan).
    pub async fn file_lister(&mut self, a: &FAreaRef, listed: &[FileRec], skip_ok: bool) -> io::Result<bool> {
        self.full_screen_done();
        if self.plain() {
            return self.plain_file_list(a, listed, skip_ok).await;
        }
        if listed.is_empty() {
            return Ok(true);
        }
        // A SysOp's edits change the files under the list.
        let mut files = listed.to_vec();
        let cosysop = self.acs(&self.state.config().acs.cosysop.clone());
        let mut list = MenuBar {
            items: &["next", "previous", "flag", "download", "view", "info", "edit", "help", "skip", "quit"],
            off: vec![false, false, false, false, false, false, !cosysop, false, !skip_ok, false],
            pos: 0,
            xs: Vec::new(),
        };
        let mut barm = MenuBar {
            items: &["flag", "download", "view", "info", "edit", "quit"],
            off: vec![false, false, false, false, !cosysop, false],
            pos: 0,
            xs: Vec::new(),
        };
        let mut first = 0usize;
        let mut bar: Option<usize> = None;
        let mut redraw_screen = true;
        let mut l = self.lister_layout(false);
        let mut rows: Vec<Row> = Vec::new();
        let mut info_dirty = false;
        loop {
            if redraw_screen {
                self.screen.set_color(Color::DEFAULT);
                self.clear_screen();
                // New: the listing's numbers, for the screen's header.
                let w = waves([
                    ("FA", a.area.name.clone()),
                    ("NA", commas(files.len() as u64)),
                    ("NF", "-".into()),
                    ("SZ", crate::upscan::human_size(files.iter().map(|f| f.size).sum())),
                ]);
                let from_screen = self.show_file_with("LISTFILE", &w).await?;
                if !from_screen {
                    self.screen.goto(1, 1);
                    self.show(&format!("|U4 Num  Filename      Size   Description |U5- |U6{}", a.area.name)).await?;
                    self.screen.goto(1, 2);
                    self.set_slot(Slot::Border);
                    self.write(&"─".repeat(79));
                }
                l = self.lister_layout(from_screen);
                rows = self.lister_page(&l, &files, first, a.num);
                let last = rows.last().map_or(first, |r| r.idx);
                self.lister_scroll(&l, first, last, files.len());
                redraw_screen = false;
                if bar.is_some_and(|b| b >= rows.len()) {
                    bar = None;
                }
                let m = if bar.is_some() { &mut barm } else { &mut list };
                self.lister_menu(&l, m);
                let info = self.strings().get(if bar.is_some() { "FaListInfoBar" } else { "FaListInfo" }).to_string();
                self.lister_info(&l, &info).await?;
                if let Some(b) = bar {
                    self.lister_name(&l, &files[rows[b].idx], rows[b].y, true, self.batch.contains(&(a.num, files[rows[b].idx].num)));
                }
            }
            self.lister_park(&l);
            let key = self.read_key().await?;
            if info_dirty {
                let info = self.strings().get(if bar.is_some() { "FaListInfoBar" } else { "FaListInfo" }).to_string();
                self.lister_info(&l, &info).await?;
                info_dirty = false;
            }
            let menu_was_bar = bar.is_some();
            match key {
                Key::Up | Key::Down => {
                    if rows.is_empty() {
                        continue;
                    }
                    if let Some(b) = bar {
                        let f = &files[rows[b].idx];
                        self.lister_name(&l, f, rows[b].y, false, self.batch.contains(&(a.num, f.num)));
                    }
                    bar = match (bar, key) {
                        (None, Key::Up) => Some(rows.len() - 1),
                        (None, _) => Some(0),
                        (Some(0), Key::Up) => None,
                        (Some(b), Key::Up) => Some(b - 1),
                        (Some(b), _) if b + 1 >= rows.len() => None,
                        (Some(b), _) => Some(b + 1),
                    };
                    if let Some(b) = bar {
                        let f = &files[rows[b].idx];
                        self.lister_name(&l, f, rows[b].y, true, self.batch.contains(&(a.num, f.num)));
                    }
                    if menu_was_bar != bar.is_some() {
                        let m = if bar.is_some() { &mut barm } else { &mut list };
                        self.lister_menu(&l, m);
                        let info = self.strings().get(if bar.is_some() { "FaListInfoBar" } else { "FaListInfo" }).to_string();
                        self.lister_info(&l, &info).await?;
                    }
                    continue;
                }
                Key::Left | Key::Right => {
                    let m = if bar.is_some() { &mut barm } else { &mut list };
                    let n = m.items.len();
                    m.pos = if key == Key::Right { (m.pos + 1) % n } else { (m.pos + n - 1) % n };
                    self.lister_menu(&l, m);
                    continue;
                }
                _ => {}
            }
            let c = match key {
                Key::Esc | Key::Backspace => Some('Q'),
                Key::Enter => None,
                k => k.upper().map(|c| match c {
                    '?' => 'H',
                    '+' => 'F',
                    c => c,
                }),
            };
            let m = if bar.is_some() { &mut barm } else { &mut list };
            let choice = match c {
                None => Some(m.items[m.pos]),
                Some(c) => m.items.iter().enumerate().find(|(i, it)| it.starts_with(c.to_ascii_lowercase()) && !m.off[*i]).map(|(i, it)| {
                    m.pos = i;
                    *it
                }),
            };
            let Some(choice) = choice else { continue };
            if m.off.get(m.pos) == Some(&true) {
                continue;
            }
            let selected = bar.map(|b| files[rows[b].idx].clone());
            match choice {
                "next" => {
                    let last = rows.last().map_or(first, |r| r.idx);
                    if last + 1 >= files.len() {
                        return Ok(true);
                    }
                    first = last + 1;
                    bar = None;
                    redraw_screen = true;
                }
                "previous" => {
                    if first > 0 {
                        // Step back a page's worth of files.
                        let lines = (l.bot - l.top + 1) as usize;
                        let mut used = 0;
                        while first > 0 {
                            let need = files[first - 1].description.len().max(1);
                            if used + need > lines {
                                break;
                            }
                            used += need;
                            first -= 1;
                        }
                        bar = None;
                        redraw_screen = true;
                    }
                }
                "flag" => {
                    match selected {
                        Some(f) => {
                            let key = match self.add_to_batch(a.num, f.num) {
                                0 => None,
                                1 => Some("FaBatchFull"),
                                _ => Some("FaBatchAlready"),
                            };
                            let msg = match key {
                                None => self.strings().fmt("FaBatchAdded", &[&f.name]),
                                Some(k) => self.strings().get(k).to_string(),
                            };
                            self.lister_info(&l, &msg).await?;
                            info_dirty = true;
                            let b = bar.unwrap();
                            self.lister_name(&l, &f, rows[b].y, !self.state.config().files.advance_bar_on_flag, true);
                            if self.state.config().files.advance_bar_on_flag {
                                bar = if b + 1 < rows.len() { Some(b + 1) } else { None };
                                if let Some(nb) = bar {
                                    let nf = &files[rows[nb].idx];
                                    self.lister_name(&l, nf, rows[nb].y, true, self.batch.contains(&(a.num, nf.num)));
                                } else {
                                    self.lister_menu(&l, &mut list);
                                }
                            }
                        }
                        None => {
                            self.screen.goto(1, l.info_y);
                            self.screen.clear_eol();
                            self.flag_file("").await?;
                            redraw_screen = true;
                        }
                    }
                }
                "download" => {
                    self.clear_screen();
                    let name = selected.map(|f| f.name).unwrap_or_default();
                    self.download(&name).await?;
                    self.prompt_key().await?;
                    redraw_screen = true;
                }
                "view" => {
                    self.clear_screen();
                    match selected {
                        Some(f) => self.view_archive_file(a, &f).await?,
                        None => self.view_archive("").await?,
                    }
                    self.prompt_key().await?;
                    redraw_screen = true;
                }
                "info" => {
                    self.clear_screen();
                    match selected {
                        Some(f) => self.file_info(&f).await?,
                        None => {
                            if let Some((_, f)) = self.locate_file("", "FaInfoAskfile").await? {
                                self.file_info(&f).await?;
                            }
                        }
                    }
                    self.prompt_key().await?;
                    redraw_screen = true;
                }
                // New: a SysOp gets the whole file editor (`*Y`) on the
                // file; a co-SysOp, its description as before.
                "edit" => {
                    let target = selected.or_else(|| rows.first().map(|r| files[r.idx].clone()));
                    if let Some(f) = target {
                        if self.is_sysop() {
                            let renamed = self.file_editor_at(a.clone(), f.num).await?;
                            self.relist(&mut files, &renamed);
                            if files.is_empty() {
                                return Ok(true);
                            }
                            first = first.min(files.len() - 1);
                        } else {
                            self.edit_description(f).await?;
                        }
                    }
                    redraw_screen = true;
                }
                "help" => {
                    self.clear_screen();
                    self.show_file("LISTHELP").await?;
                    self.prompt_key().await?;
                    redraw_screen = true;
                }
                "skip" => return Ok(true),
                "quit" => {
                    self.screen.goto(1, l.menu_y + 1);
                    self.screen.set_color(Color::DEFAULT);
                    self.newline();
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    /// The listed files as they are now: renamed ones under their new
    /// names, and those moved or deleted gone.
    fn relist(&self, files: &mut Vec<FileRec>, renamed: &[(String, String, String)]) {
        files.retain_mut(|f| {
            let mut name = f.name.clone();
            for (area, old, new) in renamed {
                if area.eq_ignore_ascii_case(&f.area) && *old == name {
                    name = new.clone();
                }
            }
            match self.state.files.find(&f.area, &name) {
                Ok(Some(now)) => {
                    *f = now;
                    true
                }
                _ => false,
            }
        });
    }

    /// `faEditFileDesc` (co-SysOp `edit` in the lister).
    pub(crate) async fn edit_description(&mut self, mut f: FileRec) -> io::Result<()> {
        let ctx = EditCtx {
            subject: format!("Desc [{}]", f.name),
            from: self.user.handle.clone(),
            to: "n/a".into(),
            date: crate::clock::now().naive_local(),
            anonymous: false,
            can_anonymous: false,
            can_autosig: false,
            autosig: false,
            reply: None,
            auto_quote: false,
            text: f.description.clone(),
        };
        if let Some(done) = self.edit_message(ctx).await? {
            f.description = done.lines.into_iter().take(MAX_DESC_LINES).collect();
            self.state.files.update(&f).map_err(io_err)?;
        }
        Ok(())
    }

    // ---- listing, scanning, searching -----------------------------------

    /// `FL`: `Ask` prompts for a filespec; otherwise lists everything.
    pub async fn list_files_cmd(&mut self, param: &str) -> io::Result<()> {
        let Some(a) = self.current_farea() else { return Ok(()) };
        if !self.acs(&a.area.acs) {
            return Ok(());
        }
        let mut files = self.state.files.list(&a.area.file).map_err(io_err)?;
        if param.trim().eq_ignore_ascii_case("ASK") {
            self.say("FaAskListFiles").await?;
            let spec = self.read_string(ReadOpts::new(128)).await?;
            self.newline();
            if !matches!(spec.as_str(), "" | "*" | "*.*") {
                files.retain(|f| wildcard(&spec, &f.name));
                if files.is_empty() {
                    return self.say_line("FaNoMatchingFiles").await;
                }
            }
        }
        if files.is_empty() {
            return self.say_line("FaNoFilesInArea").await;
        }
        self.file_lister(&a, &files, false).await?;
        Ok(())
    }

    /// `faNewScanAsk` (`FN`): `All`, an area number, or ask.
    pub async fn file_newscan_cmd(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        let p = param.trim();
        let mut all = p.to_ascii_uppercase().starts_with('A');
        let num: u32 = if all { 0 } else { p.parse().unwrap_or(0) };
        if !all && num == 0 {
            self.say("FaNewScanAskAll").await?;
            all = self.yes_no(true).await?;
            self.newline();
        }
        let since = self.user.file_scan;
        let total = self.farea_count();
        let areas: Vec<u32> = if all { self.visible_fareas().into_iter().map(|(n, _)| n).collect() } else { vec![if num == 0 { self.user.cur_file_area } else { num }.clamp(1, total.max(1))] };
        let skip_ok = areas.len() > 1;
        let off = if all { self.scan_off('F') } else { Default::default() };
        // Scanning them all, areas with nothing new are passed over
        // without a word (the original named each; an archive has
        // hundreds), found in one query rather than by reading each.
        let news = if all { Some(self.state.files.new_counts(since).map_err(io_err)?) } else { None };
        let mut scanned = false;
        for n in areas {
            let Some(a) = self.farea(n) else { continue };
            if !self.acs(&a.area.acs) || off.contains(&a.area.file) {
                continue;
            }
            if news.as_ref().is_some_and(|c| !c.contains_key(&a.area.file.to_lowercase())) {
                continue;
            }
            let files: Vec<FileRec> = self
                .state
                .files
                .list(&a.area.file)
                .map_err(io_err)?
                .into_iter()
                .filter(|f| since.is_none_or(|d| f.uploaded.is_some_and(|u| u >= d)))
                .collect();
            scanned = true;
            let count = self.state.files.count(&a.area.file).map_err(io_err)?;
            let s = self.strings().fmt("FaNewScanning", &[&a.area.name, &count.to_string()]);
            self.show(&s).await?;
            self.newline();
            if !files.is_empty() && !self.file_lister(&a, &files, skip_ok).await? {
                break;
            }
        }
        if all && !scanned {
            self.say_line("FaNewScanNothing").await?;
        }
        Ok(())
    }

    /// `faSetNewScanDate` (`FS`).
    pub async fn set_file_scan(&mut self) -> io::Result<()> {
        self.say("FaAskNewScanDate").await?;
        let cur = fmt_date(self.user.file_scan);
        let s = self.read_string(ReadOpts::new(8).allowed("0123456789/").prefill(cur)).await?;
        self.newline();
        if let Some(d) = parse_date(&s) {
            self.user.file_scan = Some(d);
            self.save_own_user().map_err(io_err)?;
        }
        Ok(())
    }

    /// `faSearchFile`: `FF` by filespec, `FT` by description text.
    pub async fn search_files(&mut self, by_name: bool) -> io::Result<()> {
        self.newline();
        self.say(if by_name { "FaSearchAskFile" } else { "FaSearchAskDesc" }).await?;
        let what = self.read_string(ReadOpts::new(if by_name { 128 } else { 70 })).await?;
        self.newline();
        if what.is_empty() {
            return Ok(());
        }
        self.say("FaSearchAskAll").await?;
        let all = self.yes_no(true).await?;
        let areas: Vec<u32> = if all { self.visible_fareas().into_iter().map(|(n, _)| n).collect() } else { vec![self.user.cur_file_area] };
        let areas: Vec<FAreaRef> = areas
            .into_iter()
            .filter_map(|n| self.farea(n))
            .filter(|a| self.acs(&a.area.acs) && (a.area.password.is_empty() || a.num == self.user.cur_file_area))
            .collect();
        // One query over every area (the original read each area's list,
        // naming it as it went; with an archive's hundreds that's a
        // screenful of names for nothing): only areas with a match are
        // named.
        let keys: Vec<String> = areas.iter().map(|a| a.area.file.clone()).collect();
        let found = self.state.files.search(&keys, by_name.then_some(what.as_str()), (!by_name).then_some(what.as_str())).map_err(io_err)?;
        let mut by_area: std::collections::HashMap<String, Vec<FileRec>> = std::collections::HashMap::new();
        for f in found {
            by_area.entry(f.area.to_lowercase()).or_default().push(f);
        }
        let mut any = false;
        for a in areas {
            let Some(files) = by_area.remove(&a.area.file.to_lowercase()) else { continue };
            let s = self.strings().fmt("FaSearchArea", &[&a.area.name]);
            self.show(&s).await?;
            self.newline();
            any = true;
            if !self.file_lister(&a, &files, true).await? {
                break;
            }
        }
        if !any {
            self.say_line("FaSearchNone").await?;
        }
        Ok(())
    }

    /// `FI`: show a file's details.
    pub async fn file_info_cmd(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        if let Some((_, f)) = self.locate_file(param, "FaInfoAskfile").await? {
            self.file_info(&f).await?;
        }
        Ok(())
    }

    // ---- archives -------------------------------------------------------

    /// `FV`: list what's inside an archive.
    pub async fn view_archive(&mut self, param: &str) -> io::Result<()> {
        self.newline();
        if let Some((a, f)) = self.locate_file(param, "FaViewAskFile").await? {
            self.view_archive_file(&a, &f).await?;
        }
        Ok(())
    }

    pub(crate) async fn view_archive_file(&mut self, a: &FAreaRef, f: &FileRec) -> io::Result<()> {
        let path = self.farea_dir(&a.area).join(&f.name);
        // Don't carry the lister's bar colour into a fresh screen.
        self.screen.set_color(Color::DEFAULT);
        match crate::upscan::kind_of(&f.name) {
            crate::upscan::Kind::Text => self.view_text_file(&path, &f.name, &a.area.name).await,
            k @ (crate::upscan::Kind::Zip | crate::upscan::Kind::TarGz | crate::upscan::Kind::Tar | crate::upscan::Kind::Gzip) => {
                self.browse_archive(&path, &f.name, k).await
            }
            // Not an archive we know: text if it reads as text, whatever
            // it's called (a textfile archive has `.ana`, `.hum`, no
            // extension at all).
            _ if self.state.archivers().for_file(&path).is_none() && std::fs::read(&path).is_ok_and(|d| crate::upscan::looks_like_text(&d)) => {
                self.view_text_file(&path, &f.name, &a.area.name).await
            }
            _ => self.view_with_archiver(&path, &f.name).await,
        }
    }

    /// Show an uploaded text file (ASCII or ANSI) in the scrolling
    /// viewer: the text before any SAUCE record, CP437 unless it's UTF-8.
    /// New: code and Markdown drawn (`markup.rs`), and the area named.
    async fn view_text_file(&mut self, path: &Path, name: &str, area: &str) -> io::Result<()> {
        let data = std::fs::read(path)?;
        let text = crate::upscan::text_part(&data);
        let utf8 = std::str::from_utf8(text).is_ok_and(|t| !t.is_ascii());
        let grid = crate::pager::trimmed(crate::pager::render_named(text, utf8, name, &self.look()));
        let title = format!("{name} in {}", crate::editor::strip_colours(area));
        self.page_grid(&title, &grid).await
    }

    /// A scratch directory of this node's own for archiver work.
    pub(crate) fn archive_scratch(&self) -> io::Result<PathBuf> {
        let dir = self.state.config().resolve(&self.state.config().paths.temp).join(format!("arc{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// FILE_ID.DIZ (or another of `names`) from any archive we can open.
    pub(crate) async fn archive_description(&mut self, path: &Path, names: &[String]) -> Option<Vec<String>> {
        if is_zip(path) {
            return zip_description(path, names);
        }
        let arc = self.state.archivers().for_file(path)?.clone();
        if arc.extract.is_empty() {
            return None;
        }
        let dir = self.archive_scratch().ok()?;
        // Tar-style tools match names exactly: ask for lower case too.
        let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let mut wanted: Vec<&str> = names.iter().map(String::as_str).collect();
        wanted.extend(lower.iter().map(String::as_str).filter(|l| !names.iter().any(|n| n == l)));
        let cmd = crate::extprog::expand(&arc.extract, &std::path::absolute(path).ok()?, &wanted);
        let found = match crate::extprog::run(&cmd, &dir).await {
            Ok(_) => {
                let got: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.filter_map(Result::ok).map(|e| e.path()).collect();
                names.iter().find_map(|want| {
                    let p = got.iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(want)))?;
                    let lines = parse_description(&std::fs::read(p).ok()?);
                    (!lines.is_empty()).then_some(lines)
                })
            }
            Err(e) => {
                tracing::warn!("{}: {e}", arc.ext);
                None
            }
        };
        let _ = std::fs::remove_dir_all(&dir);
        found
    }

    /// The archive viewer for formats other than ZIP: whatever the
    /// archiver's list command prints.
    async fn view_with_archiver(&mut self, path: &Path, name: &str) -> io::Result<()> {
        let arc = self.state.archivers().for_file(path).cloned().filter(|a| !a.list.is_empty());
        let Some(arc) = arc else {
            return self.show_line(&format!("|U6{name}|U5: |U4Can't view that kind of file|U5.")).await;
        };
        let dir = self.archive_scratch()?;
        let cmd = crate::extprog::expand(&arc.list, &std::path::absolute(path)?, &[]);
        let out = crate::extprog::run(&cmd, &dir).await;
        let _ = std::fs::remove_dir_all(&dir);
        let out = match out {
            Ok((_, out)) if !out.is_empty() => out,
            _ => return self.show_line(&format!("|U6{}|U5: |U4The {} lister isn't working|U5.", lit(name), arc.ext)).await,
        };
        self.show_line(&format!("|U4 Archive|U5: |U6{}", lit(name))).await?;
        self.rule().await?;
        self.set_slot(Slot::Text);
        let mut lines = 3;
        for l in out.split(|&b| b == b'\n') {
            let l: Vec<u8> = l.iter().copied().filter(|&b| b >= 0x20).collect();
            self.write(&fluxbbs_term::cp437::decode(&l).chars().take(79).collect::<String>());
            self.newline();
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        Ok(())
    }

    // ---- uploading --------------------------------------------------------

    /// Is a file of this name already here (`faUploadSearch`)?
    fn upload_dupe(&self, name: &str) -> io::Result<bool> {
        let level = self.state.config().files.upload_dupe_check;
        let areas: Vec<u32> = match level {
            0 | 1 => return Ok(false),
            2 => vec![self.user.cur_file_area],
            _ => (1..=self.farea_count()).collect(),
        };
        for n in areas {
            if let Some(a) = self.farea(n)
                && (self.state.files.find(&a.area.file, name).map_err(io_err)?.is_some() || self.farea_dir(&a.area).join(name).exists()) {
                    return Ok(true);
                }
        }
        Ok(false)
    }

    /// Ask for a description, a line at a time (`ulGetDesc`).
    async fn ask_description(&mut self, name: &str) -> io::Result<Vec<String>> {
        self.newline();
        let s = self.strings().fmt("FaULdescribe", &[name]);
        self.show_line(&s).await?;
        let mut lines = Vec::new();
        while lines.len() < MAX_DESC_LINES {
            let s = self.strings().fmt("FaULdescLine", &[&(lines.len() + 1).to_string()]);
            self.show(&s).await?;
            let l = self.read_string(ReadOpts::new(DESC_WIDTH)).await?;
            self.newline();
            if l.is_empty() {
                break;
            }
            lines.push(l);
        }
        Ok(lines)
    }

    /// A picture upload within `[files]`'s picture limits, decoded to make
    /// sure; None (having said why) if it's refused.
    async fn check_picture(&mut self, path: &Path, name: &str) -> io::Result<Option<crate::picture::Picture>> {
        let lim = self.state.config().files.image_limits();
        let p = path.to_path_buf();
        let loaded = tokio::task::spawn_blocking(move || -> io::Result<Result<crate::picture::Picture, String>> {
            let data = std::fs::read(&p)?;
            Ok(crate::picture::load(&data, 80, 24, lim))
        })
        .await
        .map_err(io::Error::other)??;
        match loaded {
            Ok(p) => Ok(Some(p)),
            Err(why) => {
                tracing::info!("{}: upload {name} refused: {why}", self.user.handle);
                let s = self.strings().fmt("FaULimageRefused", &[name, &why]);
                self.show_line(&s).await?;
                Ok(None)
            }
        }
    }

    /// Clear the upload prompt line and start at its beginning.
    fn upload_line(&mut self, y: u16) {
        // Plain text has no line to go back to: a new one.
        if self.plain() {
            if self.screen.where_x() > 1 {
                self.newline();
            }
            return;
        }
        self.screen.set_color(Color::DEFAULT);
        self.screen.goto(1, y);
        self.screen.clear_eol();
    }

    /// A message on the upload prompt line, held a moment.
    async fn upload_note_at(&mut self, y: u16, s: &str) -> io::Result<()> {
        self.upload_line(y);
        self.show(s).await?;
        self.flush().await?;
        self.delay(1500).await
    }

    /// The protocol on the upload prompt line, as `[Z]modem ...`; the
    /// usual list when they won't fit.
    async fn upload_protocol(&mut self, y: u16) -> io::Result<Option<Proto>> {
        let protos = self.protocols(true);
        let list: String = protos.iter().map(|(k, p)| format!("|U5[|U6{k}|U5]|U4{} ", p.name())).collect();
        let ask = self.strings().get("XferSelectProtocol").to_string();
        let width = self.visible_len(&fluxbbs_term::cp437::encode(&format!("{list}{ask}")));
        if width > 70 {
            self.upload_line(y);
            return self.select_protocol(true).await;
        }
        self.upload_line(y);
        self.show(&format!("{list}{ask}")).await?;
        loop {
            match self.read_choice().await? {
                Key::Enter | Key::Esc => {
                    self.upload_line(y);
                    return Ok(None);
                }
                k => {
                    if let Some(found) = k.upper().and_then(|c| protos.iter().find(|(key, _)| *key == c)) {
                        self.write(&found.0.to_string());
                        self.newline();
                        return Ok(Some(found.1.clone()));
                    }
                }
            }
        }
    }

    /// `faUpload` (`FU`).
    pub async fn upload(&mut self) -> io::Result<()> {
        // The prompts share one line, each replacing the last, so the menu
        // stays on screen. New; the original scrolled.
        let y = self.screen.where_y();
        let Some(a) = self.current_farea() else { return Ok(()) };
        if !self.acs(&a.area.upload_acs) {
            self.upload_line(y);
            return self.say_line("FaULnoAccess").await;
        }
        let allow_blind = self.state.config().files.allow_blind_upload;
        let blind = if allow_blind {
            self.upload_line(y);
            self.say("FaULaskBlind").await?;
            self.yes_no(false).await?
        } else {
            false
        };
        // Names the caller says they'll send, with any descriptions.
        let mut planned: Vec<(String, Vec<String>)> = Vec::new();
        if !blind {
            let s = self.strings().fmt("FaULenterFiles", &[&MAX_UPLOAD_NAMES.to_string()]);
            self.upload_note_at(y, &s).await?;
            while planned.len() < MAX_UPLOAD_NAMES {
                self.upload_line(y);
                let s = self.strings().fmt("FaULenterFilename", &[&(planned.len() + 1).to_string()]);
                self.show(&s).await?;
                let raw = self.read_string(ReadOpts::new(64)).await?;
                if raw.is_empty() {
                    break;
                }
                let Some(name) = safe_name(raw.as_bytes()) else { continue };
                if planned.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                if self.upload_dupe(&name)? {
                    let s = self.strings().fmt("FaULfileExists", &[&name]);
                    self.upload_note_at(y, &s).await?;
                    continue;
                }
                let types = self.state.config().files.allowed_types.clone();
                if !crate::upscan::type_allowed(&types, &name) {
                    tracing::info!("{}: upload {name} refused: not one of {}", self.user.handle, types.join(", "));
                    let s = self.strings().fmt("FaULtypeRefused", &[&name, &types.join(", ")]);
                    self.upload_note_at(y, &s).await?;
                    continue;
                }
                planned.push((name, Vec::new()));
            }
            if planned.is_empty() {
                self.upload_line(y);
                return Ok(());
            }
            self.upload_line(y);
            self.say("FaULaskDescNow").await?;
            self.x_pick(&["Now", "Later", "1"]).await?;
            if self.input_string == "Now" {
                self.newline();
                for (name, desc) in planned.iter_mut() {
                    *desc = self.ask_description(name).await?;
                }
                self.newline();
            }
        }
        let y = self.screen.where_y();
        let Some(proto) = self.upload_protocol(y).await? else { return Ok(()) };
        let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("node{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&temp);
        let got = self.receive_files(&proto, &temp).await?;
        let today = crate::clock::now().date_naive();
        let fc = &self.state.config().files;
        let (auto_valid, kb_pt, pct, use_points, import, desc_files, max_lines) = (
            fc.auto_validate,
            fc.kb_per_point.max(1) as u64,
            fc.upload_points_percent as u64,
            fc.use_file_points,
            fc.import_descriptions,
            fc.description_files.clone(),
            fc.max_import_lines as usize,
        );
        let dir = self.farea_dir(&a.area);
        let mut added = (0u32, 0u64);
        for path in got {
            self.end_test_screen().await?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let plan = planned.iter().position(|(n, _)| n.eq_ignore_ascii_case(&name));
            if plan.is_none() {
                if !allow_blind {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                let s = self.strings().fmt("FaULfileFound", &[&name]);
                self.show_line(&s).await?;
                if self.upload_dupe(&name)? {
                    let s = self.strings().fmt("FaULfileExists", &[&name]);
                    self.show_line(&s).await?;
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
            }
            let Some(scan) = self.screen_upload(&path, &name).await? else {
                let _ = std::fs::remove_file(&path);
                continue;
            };
            let picture = if crate::picture::is_image(&name) {
                match self.check_picture(&path, &name).await? {
                    Some(p) => Some(p),
                    None => {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                }
            } else {
                None
            };
            if !self.test_upload(&path, &name).await? {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if picture.as_ref().is_some_and(|p| p.frames.len() > 1) {
                let s = self.strings().get("FaULimageAnimated").to_string();
                self.upload_note(&s).await?;
            }
            let mut desc = plan.map(|i| planned[i].1.clone()).unwrap_or_default();
            if desc.is_empty() && import {
                let screen = self.test_screen;
                self.light(screen, b"L8").await?;
                let found = match self.archive_description(&path, &desc_files).await {
                    Some(d) => Some(d),
                    None => art_description(&path, &desc_files),
                };
                self.light_done(screen, b"L8", found.is_some());
                if let Some(mut d) = found {
                    let s = self.strings().get("FaTestDescFound").to_string();
                    self.upload_note(&s).await?;
                    if max_lines > 0 {
                        d.truncate(max_lines);
                    }
                    desc = d;
                }
            }
            if desc.is_empty()
                && let Some(p) = &picture
            {
                desc = vec![p.describe()];
            }
            if desc.is_empty() {
                self.end_test_screen().await?;
                desc = self.ask_description(&name).await?;
            }
            self.add_summary(&mut desc, scan.as_ref());
            if !self.test_screen {
                self.newline();
            }
            let s = self.strings().fmt("FaULaddingFile", &[&name]);
            self.upload_note(&s).await?;
            let meta = std::fs::metadata(&path)?;
            let dest = dir.join(&name);
            if dest.exists() || std::fs::rename(&path, &dest).is_err() {
                std::fs::copy(&path, &dest)?;
                let _ = std::fs::remove_file(&path);
            }
            let mut rec = FileRec {
                area: a.area.file.clone(),
                num: 0,
                name: name.clone(),
                size: meta.len(),
                date: meta.modified().ok().map(|t| crate::clock::from_utc(chrono::DateTime::<chrono::Utc>::from(t).naive_utc()).date()),
                uploader: self.user.handle.clone(),
                uploaded: Some(today),
                downloads: 0,
                points: (meta.len() / (kb_pt * 1024)) as u32,
                valid: auto_valid,
                description: desc,
            };
            self.state.files.add(&mut rec).map_err(io_err)?;
            self.bump(crate::sysdata::Stat::Upload { kb: rec.size / 1024 });
            if rec.valid {
                self.user.uploads += 1;
                self.user.upload_kb += rec.size / 1024;
                if use_points && pct > 0 {
                    let award = (rec.points as u64 * pct / 100) as u32;
                    self.user.file_points += award;
                    let s = self.strings().fmt("s269", &[&award.to_string()]);
                    self.upload_note(&s).await?;
                }
            }
            added.0 += 1;
            added.1 += rec.size;
            tracing::info!("{}: uploaded {} to {}", self.user.handle, name, a.area.name);
            let s = self.strings().fmt("FaULfileAdded", &[&name, &rec.description.len().to_string()]);
            self.upload_note(&s).await?;
        }
        let _ = std::fs::remove_dir_all(&temp);
        self.save_own_user().map_err(io_err)?;
        let summary = if added.0 > 0 {
            self.strings().fmt("s271", &[&added.0.to_string(), &commas(added.1)])
        } else {
            self.strings().get("FaXferFailed").to_string()
        };
        // The last file's FILETEST stays up with the summary in its status
        // field; the menu's pause comes up under it.
        if self.test_screen {
            self.upload_note(&summary).await?;
            self.test_screen = false;
            let below = self.positions.get(b"PI").map_or(self.screen.rows(), |p| p.1 + 2).min(self.screen.rows());
            self.screen.goto(1, below);
            return Ok(());
        }
        self.show_line(&summary).await?;
        self.newline();
        Ok(())
    }

    /// `*%` (`faLocalUpload`): add files a SysOp copied into the current
    /// area's directory.
    pub async fn local_upload(&mut self) -> io::Result<()> {
        let Some(a) = self.current_farea() else { return Ok(()) };
        let dir = self.farea_dir(&a.area);
        let known: Vec<String> = self.state.files.list(&a.area.file).map_err(io_err)?.into_iter().map(|f| f.name.to_lowercase()).collect();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect();
        entries.sort();
        let desc_files = self.state.config().files.description_files.clone();
        let kb_pt = self.state.config().files.kb_per_point.max(1) as u64;
        let mut recs = Vec::new();
        self.newline();
        for path in entries {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if known.contains(&name.to_lowercase()) || name.starts_with('.') {
                continue;
            }
            let cfg = self.state.config();
            let mut rec = local_record(&a.area.file, &path, &self.user.handle, &desc_files, kb_pt, cfg.files.image_limits())?;
            // Other archives need their archiver to give up a description.
            if rec.description.is_empty() && !is_zip(&path)
                && let Some(d) = self.archive_description(&path, &desc_files).await {
                    rec.description = d;
                }
            recs.push(rec);
        }
        // Descriptions are read first; the records then go in at once, so
        // other nodes aren't kept waiting a file at a time.
        self.state.files.add_all(&mut recs).map_err(io_err)?;
        for rec in &recs {
            let s = self.strings().fmt("FaULfileAdded", &[&rec.name, &rec.description.len().to_string()]);
            self.show_line(&s).await?;
        }
        self.show_line(&format!("|U1Added |U3{}|U1 file(s) to |U3{}|U2.", recs.len(), a.area.name)).await
    }
}

/// A file record for a file put into an area's directory by hand
/// (`*%`, `fluxbbs add-files`): its size, date and points, and a
/// description from its FILE_ID.DIZ (ZIPs), its SAUCE (a piece of art) or
/// what it is (a picture).
pub fn local_record(area: &str, path: &Path, uploader: &str, desc_files: &[String], kb_per_point: u64, lim: crate::picture::Limits) -> io::Result<FileRec> {
    let meta = std::fs::metadata(path)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut description = if is_zip(path) { zip_description(path, desc_files) } else { None }
        .or_else(|| art_description(path, desc_files))
        .unwrap_or_default();
    if description.is_empty() && crate::picture::is_image(&name)
        && let Ok(p) = std::fs::read(path).map_err(|e| e.to_string()).and_then(|d| crate::picture::load(&d, 80, 24, lim)) {
            description = vec![p.describe()];
        }
    if description.is_empty() && is_zip(path) {
        description = zip_contents(path);
    }
    Ok(FileRec {
        area: area.to_string(),
        num: 0,
        name,
        size: meta.len(),
        date: meta.modified().ok().map(|t| crate::clock::from_utc(chrono::DateTime::<chrono::Utc>::from(t).naive_utc()).date()),
        uploader: uploader.to_string(),
        uploaded: Some(crate::clock::now().date_naive()),
        downloads: 0,
        points: (meta.len() / (kb_per_point.max(1) * 1024)) as u32,
        valid: true,
        description,
    })
}

/// For a ZIP with no FILE_ID.DIZ (most early art packs): how many files
/// it holds and their names, in a few lines.
fn zip_contents(path: &Path) -> Vec<String> {
    let Ok(z) = std::fs::File::open(path).map_err(|_| ()).and_then(|f| zip::ZipArchive::new(f).map_err(|_| ())) else { return Vec::new() };
    let names: Vec<String> = z.file_names().flatten().filter(|n| !n.ends_with('/')).map(|n| n.rsplit('/').next().unwrap_or(&n).to_string()).collect();
    if names.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![if names.len() == 1 { "1 file:".to_string() } else { format!("{} files:", names.len()) }];
    for n in &names {
        let last = lines.last_mut().expect("a line");
        if last.len() + 1 + n.len() <= DESC_WIDTH {
            last.push(' ');
            last.push_str(n);
        } else if lines.len() < 4 {
            lines.push(n.clone());
        } else {
            break;
        }
    }
    lines
}

/// "m:ss" for a transfer estimate (`mTimeSec`).
fn fmt_secs(s: u64) -> String {
    format!("{}:{:02}", s / 60, s % 60)
}

/// Lines of an uploaded text file: UTF-8 or CP437, tabs expanded, long
/// lines wrapped to the message width, at most 500 lines.
pub fn text_lines(bytes: &[u8]) -> Vec<String> {
    let utf8 = std::str::from_utf8(bytes).is_ok();
    let mut out = Vec::new();
    for src in bytes.split(|&b| b == b'\n') {
        let mut raw: Vec<u8> = Vec::with_capacity(src.len());
        for &b in src.iter().filter(|&&b| b != b'\r' && b != 0x1a) {
            if b == b'\t' { raw.extend_from_slice(b"    ") } else { raw.push(b) }
        }
        let line = if utf8 { String::from_utf8_lossy(&raw).into_owned() } else { fluxbbs_term::cp437::decode(&raw) };
        if line.chars().count() <= 79 {
            out.push(line);
        } else {
            out.extend(crate::multinode::wrap(&line, 79));
        }
        if out.len() >= 500 {
            break;
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}


/// A name as the lister shows it: whole, if it fits the name column;
/// None and it gets a row of its own.
fn lister_shown_name(name: &str, width: usize) -> Option<String> {
    (name.chars().count() <= width).then(|| name.to_string())
}

/// A size in at most seven columns: "512 B", "4.5 KB", "12 MB".
pub(crate) fn short_size(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    for unit in ["KB", "MB", "GB"] {
        if v < 1024.0 {
            return if v < 10.0 { format!("{v:.1} {unit}") } else { format!("{v:.0} {unit}") };
        }
        v /= 1024.0;
    }
    format!("{v:.0} TB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_descriptions_from_one_piece() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("iqartdesc{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut art = b"art\r\n\x1aSAUCE00".to_vec();
        art.extend(format!("{:35}{:20}{:20}20261018", "disflux", "phrac", "").as_bytes());
        art.resize(art.len() + 128 - 90, 0);
        std::fs::write(dir.join("FILE.ANS"), &art).unwrap();
        let diz = ["FILE_ID.DIZ".to_string()];
        let want = ["disflux", "by phrac, 2026-10-18"];
        assert_eq!(art_description(&dir.join("FILE.ANS"), &diz).unwrap(), want);
        let zip = |name: &str, entries: &[&str]| {
            let mut w = zip::ZipWriter::new(std::fs::File::create(dir.join(name)).unwrap());
            for e in entries {
                w.start_file(*e, zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(&art).unwrap();
            }
            w.finish().unwrap();
            dir.join(name)
        };
        assert_eq!(art_description(&zip("one.zip", &["FILE.ANS", "file_id.diz"]), &diz).unwrap(), want);
        assert!(art_description(&zip("pack.zip", &["A.ANS", "B.ANS"]), &diz).is_none(), "a pack has no one piece");
        assert!(art_description(&zip("prog.zip", &["PROG.EXE"]), &diz).is_none());
        // A pack with no DIZ is described by what's in it.
        assert_eq!(zip_contents(&zip("pack.zip", &["A.ANS", "art/B.ANS"])), ["2 files: A.ANS B.ANS"]);
        let many: Vec<String> = (0..40).map(|i| format!("PIECE{i:03}.ANS")).collect();
        let lines = zip_contents(&zip("big.zip", &many.iter().map(String::as_str).collect::<Vec<_>>()));
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|l| l.len() <= DESC_WIDTH) && lines[0].starts_with("40 files: PIECE000.ANS"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lister_names() {
        assert_eq!(lister_shown_name("PHRACK1.ZIP", 12).as_deref(), Some("PHRACK1.ZIP"), "names are shown whole");
        assert_eq!(lister_shown_name("phrack2.tar.gz", 12), None, "too long: a row of its own");
        assert_eq!((short_size(512), short_size(4608), short_size(12 << 20)), ("512 B".into(), "4.5 KB".into(), "12 MB".into()));
    }

    #[test]
    fn uploaded_text_is_decoded_and_wrapped() {
        let l = text_lines(b"one\r\ntwo\tx\r\n\xb0 box\r\n\r\n\x1a");
        assert_eq!(l, ["one", "two    x", "\u{2591} box"]);
        let l = text_lines("caf\u{e9}\n".as_bytes());
        assert_eq!(l, ["caf\u{e9}"]);
        let long = text_lines(format!("{}\n", "word ".repeat(40)).as_bytes());
        assert!(long.len() > 2 && long.iter().all(|l| l.chars().count() <= 79));
    }

    #[test]
    fn thousands() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(1234567), "1,234,567");
    }

    #[test]
    fn diz_from_zip() {
        let dir = std::env::temp_dir().join(format!("iqdiz{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("T.ZIP");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        w.start_file("file_id.diz", zip::write::SimpleFileOptions::default()).unwrap();
        io::Write::write_all(&mut w, b"Line one\r\nLine two  \r\n\r\n\x1a").unwrap();
        w.finish().unwrap();
        let got = zip_description(&path, &["FILE_ID.DIZ".into()]).unwrap();
        assert_eq!(got, vec!["Line one", "Line two"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
