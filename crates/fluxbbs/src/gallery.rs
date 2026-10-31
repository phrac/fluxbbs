//! The art gallery (`AG`; new, after Mystic's): a folder of art, browsed
//! like a file manager, with ZIP packs opened as folders in place. Each
//! piece lists its SAUCE title, artist and group; Enter shows it in the
//! scrolling viewer, P plays it at a modem speed as it drew in its day
//! (animated ANSIs come alive this way), and D downloads the piece or
//! the whole pack.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use fluxbbs_term::Slot;
use fluxbbs_term::codes::{resize, squish};

use crate::filebase::FileRec;
use crate::picture::{self, is_image};
use crate::files::{Layout, MenuBar};
use crate::input::Key;
use crate::session::{Session, waves};
use crate::upscan::Kind;

/// Art the gallery shows; anything else in a pack is skipped.
/// (`art`, `pic`, `pix` and `vt` are what textfiles.com's artwork is
/// called.)
const ART: [&str; 11] = ["ans", "ice", "asc", "txt", "nfo", "diz", "cia", "art", "pic", "pix", "vt"];
/// The largest piece read into memory.
const MAX_PIECE: u64 = 8 << 20;
/// Play speeds to cycle through (bits per second; 0 = all at once).
const SPEEDS: [u32; 8] = [0, 2400, 9600, 14400, 28800, 38400, 57600, 115200];

/// A SAUCE record (the 128 bytes at the end of an art file).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sauce {
    pub title: String,
    pub author: String,
    pub group: String,
    /// `YYYY-MM-DD`, or empty.
    pub date: String,
}

fn field(b: &[u8]) -> String {
    let s = fluxbbs_term::cp437::decode(b);
    s.trim_end_matches(['\0', ' ']).trim().to_string()
}

pub fn parse_sauce(data: &[u8]) -> Option<Sauce> {
    let rec = data.get(data.len().checked_sub(128)?..)?;
    if !rec.starts_with(b"SAUCE00") {
        return None;
    }
    let d = field(&rec[82..90]);
    let date = if d.len() == 8 && d.bytes().all(|c| c.is_ascii_digit()) { format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]) } else { String::new() };
    Some(Sauce { title: field(&rec[7..42]), author: field(&rec[42..62]), group: field(&rec[62..82]), date })
}

/// The comment lines (`COMNT` block) before a SAUCE record.
fn sauce_comments(data: &[u8]) -> Vec<String> {
    let Some(rec) = data.len().checked_sub(128).and_then(|i| data.get(i..)) else { return Vec::new() };
    let n = rec[104] as usize;
    let Some(start) = data.len().checked_sub(128 + 5 + n * 64) else { return Vec::new() };
    if n == 0 || !data[start..].starts_with(b"COMNT") {
        return Vec::new();
    }
    data[start + 5..start + 5 + n * 64].chunks(64).map(field).collect()
}

/// An upload's description from its SAUCE: the title, who made it and
/// when, then its comments, wrapped to `width`. None without any of them.
pub fn sauce_description(data: &[u8], width: usize, max: usize) -> Option<Vec<String>> {
    let s = parse_sauce(data)?;
    let mut lines = Vec::new();
    if !s.title.is_empty() {
        lines.push(s.title.clone());
    }
    let by = match (byline(&s), s.date.is_empty()) {
        (b, true) if b.is_empty() => String::new(),
        (b, false) if b.is_empty() => s.date.clone(),
        (b, true) => format!("by {b}"),
        (b, false) => format!("by {b}, {}", s.date),
    };
    if !by.is_empty() {
        lines.push(by);
    }
    for c in sauce_comments(data).iter().filter(|c| !c.is_empty()) {
        let mut line = String::new();
        for word in c.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    let lines: Vec<String> = lines.into_iter().map(|l| l.chars().take(width).collect()).take(max).collect();
    (!lines.is_empty()).then_some(lines)
}

/// Where a folder is: a real directory, a file area (its sub-areas and
/// files), or a path inside a ZIP.
#[derive(Debug, Clone, PartialEq)]
enum Place {
    Dir(PathBuf),
    Area(u32),
    /// The top-level file areas the caller can see.
    Areas,
    Zip { zip: PathBuf, prefix: String },
    /// A tar, tar.gz or .gz, read in place like a ZIP.
    Tar { tar: PathBuf, kind: Kind, prefix: String },
}

/// Where a piece is.
#[derive(Debug, Clone, PartialEq)]
enum Src {
    Disk(PathBuf),
    Zip(PathBuf, String),
    Tar(PathBuf, Kind, String),
}

/// What a browse lists: art and pictures (the gallery), or every file (an
/// archive opened from a file area). `cap` bounds reading a tar.
#[derive(Clone, Copy)]
struct Mode {
    all: bool,
    cap: u64,
}

/// What a file is, for the description column of a file that isn't art.
fn kind_word(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let ext = n.rsplit_once('.').map_or("", |(_, e)| e);
    if is_image(name) {
        "picture"
    } else if is_art(name) || ["txt", "doc", "md", "me", "1st", "log", "cfg", "ini", "bat"].contains(&ext) {
        "text"
    } else if ["zip", "gz", "tgz", "tar", "rar", "7z", "arj", "lha", "lzh", "bz2", "xz"].contains(&ext) {
        "archive"
    } else if ["exe", "com"].contains(&ext) {
        "program"
    } else {
        "file"
    }
}

/// A file area's record for an item that is one of its files: downloads
/// of these go through the area's checks and accounting.
type Rec = Option<(u32, FileRec)>;

#[derive(Debug, Clone)]
enum Item {
    Folder { name: String, place: Place, pack: bool, rec: Rec, when: Option<NaiveDate> },
    Art { name: String, size: u64, sauce: Option<Sauce>, src: Src, rec: Rec, when: Option<NaiveDate> },
}

/// New: the order the gallery lists a place in. Folders stay first, by name;
/// the rest - packs and pieces - go in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Order {
    #[default]
    Name,
    Newest,
    Oldest,
    Type,
    Size,
}

impl Order {
    /// The next order, for the key that cycles them.
    fn next(self) -> Order {
        match self {
            Order::Name => Order::Newest,
            Order::Newest => Order::Oldest,
            Order::Oldest => Order::Type,
            Order::Type => Order::Size,
            Order::Size => Order::Name,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Order::Name => "name",
            Order::Newest => "newest first",
            Order::Oldest => "oldest first",
            Order::Type => "type",
            Order::Size => "size, biggest first",
        }
    }
}

/// When a file was put there: its record's upload date, or the file's own.
fn mtime(path: &Path) -> Option<NaiveDate> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(chrono::DateTime::<chrono::Utc>::from(t).date_naive())
}

fn rec_date(f: &FileRec, path: &Path) -> Option<NaiveDate> {
    f.uploaded.or(f.date).or_else(|| mtime(path))
}

/// Put `items` in `order`: folders that are not packs first, by name, as
/// they were; then everything else.
fn sort_items(items: &mut [Item], order: Order) {
    let folders = items.iter().take_while(|i| matches!(i, Item::Folder { pack: false, .. })).count();
    let rest = &mut items[folders..];
    let name = |i: &Item| match i {
        Item::Folder { name, .. } | Item::Art { name, .. } => name.to_lowercase(),
    };
    let when = |i: &Item| match i {
        Item::Folder { when, .. } | Item::Art { when, .. } => *when,
    };
    let size = |i: &Item| match i {
        Item::Art { size, .. } => *size,
        Item::Folder { rec, .. } => rec.as_ref().map_or(0, |r| r.1.size),
    };
    let ext = |i: &Item| name(i).rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default();
    match order {
        Order::Name => rest.sort_by_key(name),
        // Undated ones last either way: an unknown date is not "the oldest".
        Order::Newest => rest.sort_by_key(|i| (when(i).is_none(), std::cmp::Reverse(when(i)), name(i))),
        Order::Oldest => rest.sort_by_key(|i| (when(i).is_none(), when(i), name(i))),
        Order::Type => rest.sort_by_key(|i| (ext(i), name(i))),
        Order::Size => rest.sort_by_key(|i| (std::cmp::Reverse(size(i)), name(i))),
    }
}

pub(crate) fn is_art(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| ART.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

fn is_zip(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".zip")
}

/// The SAUCE of a file on disk, from its last 128 bytes.
fn disk_sauce(path: &Path) -> Option<Sauce> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.checked_sub(128)?)).ok()?;
    let mut rec = [0u8; 128];
    f.read_exact(&mut rec).ok()?;
    parse_sauce(&rec)
}

/// A piece's bytes.
fn read_piece(src: &Src) -> io::Result<Vec<u8>> {
    match src {
        Src::Disk(p) => {
            let mut out = Vec::new();
            std::fs::File::open(p)?.take(MAX_PIECE).read_to_end(&mut out)?;
            Ok(out)
        }
        Src::Zip(zip, name) => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(zip)?).map_err(io::Error::other)?;
            let mut out = Vec::new();
            z.by_name(name).map_err(io::Error::other)?.take(MAX_PIECE).read_to_end(&mut out)?;
            Ok(out)
        }
        Src::Tar(tar, kind, name) => crate::archview::read_entry(tar, *kind, name, MAX_PIECE),
    }
}

/// Up to `max` bytes of a piece (pictures can be bigger than art).
fn read_limited(src: &Src, max: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match src {
        Src::Disk(p) => {
            std::fs::File::open(p)?.take(max).read_to_end(&mut out)?;
        }
        Src::Zip(zip, name) => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(zip)?).map_err(io::Error::other)?;
            z.by_name(name).map_err(io::Error::other)?.take(max).read_to_end(&mut out)?;
        }
        Src::Tar(tar, kind, name) => return crate::archview::read_entry(tar, *kind, name, max),
    }
    Ok(out)
}

/// What's in a folder: folders and packs first, then pieces, by name.
fn list(place: &Place, mode: Mode) -> io::Result<Vec<Item>> {
    let mut folders = Vec::new();
    let mut art = Vec::new();
    match place {
        Place::Dir(dir) => {
            for e in std::fs::read_dir(dir)?.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    folders.push(Item::Folder { name, when: mtime(&path), place: Place::Dir(path), pack: false, rec: None });
                } else if is_zip(&name) {
                    folders.push(Item::Folder { name, when: mtime(&path), place: Place::Zip { zip: path, prefix: String::new() }, pack: true, rec: None });
                } else if is_art(&name) || is_image(&name) {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    let sauce = if is_image(&name) { None } else { disk_sauce(&path) };
                    art.push(Item::Art { sauce, name, size, when: mtime(&path), src: Src::Disk(path), rec: None });
                }
            }
        }
        // Listed by the session, which knows what the caller may see.
        Place::Area(_) | Place::Areas => {}
        Place::Zip { zip, prefix } => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(zip)?).map_err(io::Error::other)?;
            let mut seen = Vec::new();
            for i in 0..z.len() {
                let Ok(mut f) = z.by_index(i) else { continue };
                let when = f.last_modified().and_then(|d| NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32));
                let Some(full) = f.enclosed_name().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
                let Some(rest) = full.strip_prefix(prefix.as_str()) else { continue };
                if let Some((dir, _)) = rest.split_once('/') {
                    if !seen.contains(&dir.to_string()) {
                        seen.push(dir.to_string());
                        let place = Place::Zip { zip: zip.clone(), prefix: format!("{prefix}{dir}/") };
                        folders.push(Item::Folder { name: dir.to_string(), place, pack: false, rec: None, when: None });
                    }
                } else if !f.is_dir() && (is_image(rest) || mode.all && !is_art(rest)) {
                    art.push(Item::Art { name: rest.to_string(), size: f.size(), sauce: None, src: Src::Zip(zip.clone(), full), rec: None, when });
                } else if !f.is_dir() && is_art(rest) {
                    // Reading the piece for its SAUCE: art files are small.
                    let mut data = Vec::new();
                    let _ = (&mut f).take(MAX_PIECE).read_to_end(&mut data);
                    art.push(Item::Art { name: rest.to_string(), size: f.size(), sauce: parse_sauce(&data), src: Src::Zip(zip.clone(), full), rec: None, when });
                }
            }
        }
        Place::Tar { tar, kind, prefix } => {
            let mut seen = Vec::new();
            for (full, size, when) in crate::upscan::tar_listing(tar, *kind, mode.cap)? {
                // Tars often name everything "./something".
                let shown = full.trim_start_matches("./");
                let Some(rest) = shown.strip_prefix(prefix.as_str()) else { continue };
                if let Some((dir, _)) = rest.split_once('/') {
                    if !seen.contains(&dir.to_string()) {
                        seen.push(dir.to_string());
                        let place = Place::Tar { tar: tar.clone(), kind: *kind, prefix: format!("{prefix}{dir}/") };
                        folders.push(Item::Folder { name: dir.to_string(), place, pack: false, rec: None, when: None });
                    }
                } else if !rest.is_empty() && (mode.all || is_art(rest) || is_image(rest)) {
                    art.push(Item::Art { name: rest.to_string(), size, sauce: None, src: Src::Tar(tar.clone(), *kind, full.clone()), rec: None, when });
                }
            }
        }
    }
    let key = |i: &Item| match i {
        Item::Folder { name, .. } | Item::Art { name, .. } => name.to_lowercase(),
    };
    folders.sort_by_key(key);
    art.sort_by_key(key);
    folders.extend(art);
    Ok(folders)
}

/// Bytes of a piece fit to send as it is: colour and cursor codes kept,
/// other escape sequences and control characters dropped (NULs are blank
/// cells), stopping at the SAUCE.
fn playable(data: &[u8]) -> Vec<u8> {
    let text = data.split(|&c| c == 0x1a).next().unwrap_or_default();
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            0x1b if text.get(i + 1) == Some(&b'[') => {
                let mut j = i + 2;
                while j < text.len() && (0x20..=0x3f).contains(&text[j]) {
                    j += 1;
                }
                let plain = text[i + 2..j].iter().all(|c| c.is_ascii_digit() || *c == b';');
                if j < text.len() && plain && b"mHfABCDJKsu".contains(&text[j]) {
                    out.extend_from_slice(&text[i..=j]);
                }
                i = j + 1;
            }
            0 => {
                out.push(b' ');
                i += 1;
            }
            c if c < 0x20 && !matches!(c, b'\r' | b'\n' | b'\t') => i += 1,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

impl Session {
    /// `AG`: browse the art gallery.
    pub async fn art_gallery(&mut self) -> io::Result<()> {
        self.full_screen_done();
        let cfg = self.state.config();
        let dir = cfg.resolve(&cfg.gallery.path);
        let (acs, area, speed) = (cfg.gallery.acs.clone(), cfg.gallery.area.trim().to_string(), cfg.gallery.speed);
        drop(cfg);
        if !self.acs(&acs) {
            return self.say_line("GalleryNoAccess").await;
        }
        // A file area, when one is named, else the folder.
        let root = if area.is_empty() {
            if !dir.is_dir() {
                return self.say_line("GalleryEmpty").await;
            }
            Place::Dir(dir)
        } else {
            let n = self.state.fareas().areas.iter().position(|a| a.file.eq_ignore_ascii_case(&area)).map(|i| i as u32 + 1);
            match n {
                Some(n) if self.farea_visible(n) => Place::Area(n),
                Some(_) => return self.say_line("GalleryNoAccess").await,
                None => {
                    tracing::warn!("[gallery] area {area} doesn't exist");
                    return self.say_line("GalleryEmpty").await;
                }
            }
        };
        tracing::info!("{}: art gallery", self.user.handle);
        self.node.update(|n| n.activity = "Art gallery".into());
        self.gallery_browse(root, "art gallery", Mode { all: false, cap: MAX_PIECE }, speed).await
    }

    /// An archive from a file area, browsed like the gallery but with
    /// every file listed: folders open, Enter shows a file (text, ANSI or a
    /// picture), D downloads one.
    pub(crate) async fn browse_archive(&mut self, path: &Path, name: &str, kind: Kind) -> io::Result<()> {
        let cap = crate::upscan::Limits::from_config(&self.state.config().files).cap(std::fs::metadata(path)?.len());
        let root = match kind {
            Kind::Zip => Place::Zip { zip: path.to_path_buf(), prefix: String::new() },
            _ => Place::Tar { tar: path.to_path_buf(), kind, prefix: String::new() },
        };
        let speed = self.state.config().gallery.speed;
        self.gallery_browse(root, name, Mode { all: true, cap }, speed).await
    }

    /// `FW`: the file areas, browsed like the gallery (new; it replaces the
    /// lister on the file menu). It opens in the current area; back from
    /// there goes up to the list of areas. With `newest`, `hot`, `heavy`,
    /// `dig` or `kingpin` it opens on that file from the file menu's stats
    /// (the newest, most downloaded, biggest, random, top uploader's latest).
    pub async fn file_browser(&mut self, param: &str) -> io::Result<()> {
        let cap = crate::upscan::Limits::from_config(&self.state.config().files).max_unpacked;
        let mode = Mode { all: true, cap: if cap == 0 { u64::MAX } else { cap } };
        let speed = self.state.config().gallery.speed;
        let stats = self.file_stats(true);
        let target = match param.trim().to_ascii_lowercase().as_str() {
            "newest" => stats.newest.as_ref(),
            "hot" => stats.popular.as_ref(),
            "heavy" => stats.biggest.as_ref(),
            "dig" => crate::mci::dig(&stats),
            "kingpin" => stats.kingpin_latest.as_ref(),
            _ => None,
        };
        // New: plain text lists the area's files, or shows the one named.
        if self.plain() {
            return match target.cloned() {
                Some(f) => self.file_info(&f).await,
                None => self.list_files_cmd("").await,
            };
        }
        let fa = self.state.fareas();
        let (cur, focus) = match target.and_then(|f| fa.areas.iter().position(|a| a.file.eq_ignore_ascii_case(&f.area)).map(|i| (i as u32 + 1, f.name.clone()))) {
            Some((n, name)) if self.farea_visible(n) => (n, Some(name)),
            _ => (self.user.cur_file_area, None),
        };
        let start = self.farea(cur).map(|a| (a.area.name.clone(), Place::Area(cur)));
        self.node.update(|n| n.activity = "Browsing files".into());
        self.gallery_browse_from(Place::Areas, "file areas", mode, speed, start, focus).await
    }

    async fn gallery_browse(&mut self, root: Place, title: &str, mode: Mode, speed: u32) -> io::Result<()> {
        self.gallery_browse_from(root, title, mode, speed, None, None).await
    }

    /// Browse from `root`, starting one level down at `start` if given,
    /// with the bar on the item called `focus`.
    async fn gallery_browse_from(&mut self, root: Place, title: &str, mode: Mode, mut speed: u32, start: Option<(String, Place)>, mut focus: Option<String>) -> io::Result<()> {
        self.full_screen_done();
        // Where we are, and the cursor in each level above.
        let mut path: Vec<(String, Place, usize)> = vec![(String::new(), root, 0)];
        if let Some((name, place)) = start {
            path.push((name, place, 0));
        }
        loop {
            let (_, place, _) = path.last().cloned().expect("root");
            // New: the file area being browsed (or the one an open archive is
            // in) becomes the current one, so leaving stays there. Not one
            // with a password: `FA` asks for that.
            if let Some(n) = path.iter().rev().find_map(|(_, p, _)| if let Place::Area(n) = p { Some(*n) } else { None })
                && n != self.user.cur_file_area
                && self.farea_visible(n)
                && self.farea(n).is_some_and(|a| a.area.password.is_empty())
            {
                self.user.cur_file_area = n;
            }
            let mut items = self.gallery_list(&place, mode).await?;
            sort_items(&mut items, self.gallery_order);
            if let Some(name) = focus.take()
                && let Some(i) = items.iter().position(|it| match it {
                    Item::Folder { name: n, .. } | Item::Art { name: n, .. } => n.eq_ignore_ascii_case(&name),
                })
            {
                path.last_mut().expect("root").2 = i;
            }
            let crumbs: Vec<String> = path.iter().skip(1).map(|(n, _, _)| n.clone()).collect();
            let at = path.last().map(|l| l.2).unwrap_or(0);
            let place = if crumbs.is_empty() { title.to_string() } else { format!("{title}/{}", crumbs.join("/")) };
            // The file browser and archives look like the file lister.
            let screen = if mode.all { "LISTFILE" } else { "GALLERY" };
            match self.gallery_pick(screen, &place, &items, at, &mut speed).await? {
                Pick::Back if path.len() > 1 => {
                    path.pop();
                }
                Pick::Back | Pick::Quit => {
                    // Leave a clean screen for what comes next.
                    self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                    self.clear_screen();
                    return Ok(());
                }
                Pick::Open(i) => {
                    path.last_mut().expect("root").2 = i;
                    match &items[i] {
                        Item::Folder { name, place, .. } => path.push((name.clone(), place.clone(), 0)),
                        Item::Art { name, sauce, src, .. } => {
                            // From the viewer, P plays and D downloads.
                            let picked = if is_pdf(name) {
                                self.pdf_open(name, src, false).await?
                            } else if is_image(name) {
                                self.gallery_picture(name, src).await?
                            } else if is_art(name) {
                                self.gallery_view(name, sauce.as_ref(), src).await?
                            } else {
                                self.gallery_file(name, src, crumbs.last().map_or(title, |c| c.as_str())).await?
                            };
                            match picked {
                                // Q leaves the gallery from a viewer too.
                                Some('Q') => {
                                    self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                                    self.clear_screen();
                                    return Ok(());
                                }
                                Some('P') if is_image(name) => {}
                                Some('P') => self.gallery_played(&items[i], speed).await?,
                                Some('D') => self.gallery_download(&items[i]).await?,
                                _ => {}
                            }
                        }
                    }
                }
                Pick::Play(i) => {
                    path.last_mut().expect("root").2 = i;
                    if let Item::Art { src, name, .. } = &items[i] {
                        if is_pdf(name) {
                            if self.pdf_open(name, src, true).await? == Some('D') {
                                self.gallery_download(&items[i]).await?;
                            }
                        } else if is_image(name) {
                            if self.gallery_picture(name, src).await? == Some('D') {
                                self.gallery_download(&items[i]).await?;
                            }
                        } else {
                            self.gallery_played(&items[i], speed).await?;
                        }
                    }
                }
                Pick::Download(i) => {
                    path.last_mut().expect("root").2 = i;
                    self.gallery_download(&items[i]).await?;
                }
                Pick::Info(i) => {
                    path.last_mut().expect("root").2 = i;
                    if let Some((_, f)) = items.get(i).and_then(record).cloned() {
                        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                        self.clear_screen();
                        self.file_info(&f).await?;
                        self.prompt_key().await?;
                    }
                }
                // A SysOp gets the whole file editor; a co-SysOp, the description.
                Pick::Edit(i) => {
                    path.last_mut().expect("root").2 = i;
                    if let Some((n, f)) = items.get(i).and_then(record).cloned() {
                        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                        self.clear_screen();
                        if self.is_sysop() {
                            if let Some(a) = self.farea(n) {
                                // Boxed: the editor can open the browser, which can open the editor.
                                Box::pin(self.file_editor_at(a, f.num)).await?;
                            }
                        } else {
                            self.edit_description(f).await?;
                        }
                    }
                }
                // The next order, keeping the bar on the same piece.
                Pick::Resort(i) => {
                    self.gallery_order = self.gallery_order.next();
                    focus = items.get(i).map(|it| match it {
                        Item::Folder { name, .. } | Item::Art { name, .. } => name.clone(),
                    });
                }
            }
        }
    }

    /// What's in a place. A file area shows the sub-areas the caller can
    /// see, then its validated files: packs as folders, art as pieces.
    async fn gallery_list(&mut self, place: &Place, mode: Mode) -> io::Result<Vec<Item>> {
        if *place == Place::Areas {
            let fa = self.state.fareas();
            return Ok(self
                .farea_children(None)
                .into_iter()
                .map(|c| Item::Folder { name: fa.areas[c as usize - 1].name.clone(), place: Place::Area(c), pack: false, rec: None, when: None })
                .collect());
        }
        let Place::Area(n) = place else {
            let p = place.clone();
            return tokio::task::spawn_blocking(move || list(&p, mode)).await.map_err(io::Error::other)?;
        };
        let fa = self.state.fareas();
        let mut out = Vec::new();
        for c in self.farea_children(Some(*n)) {
            out.push(Item::Folder { name: fa.areas[c as usize - 1].name.clone(), place: Place::Area(c), pack: false, rec: None, when: None });
        }
        let Some(a) = self.farea(*n) else { return Ok(out) };
        let dir = self.farea_dir(&a.area);
        let mut files = self.state.files.list(&a.area.file).map_err(io::Error::other)?;
        files.retain(|f| f.valid);
        files.sort_by_key(|f| f.name.to_lowercase());
        let (mut packs, mut art) = (Vec::new(), Vec::new());
        for f in files {
            let path = dir.join(&f.name);
            if !path.is_file() {
                continue;
            }
            let when = rec_date(&f, &path);
            let tar = match crate::upscan::kind_of(&f.name) {
                k @ (Kind::Tar | Kind::TarGz | Kind::Gzip) if mode.all => Some(k),
                _ => None,
            };
            if is_zip(&f.name) {
                let place = Place::Zip { zip: path, prefix: String::new() };
                packs.push(Item::Folder { name: f.name.clone(), place, pack: true, rec: Some((*n, f)), when });
            } else if let Some(kind) = tar {
                let place = Place::Tar { tar: path, kind, prefix: String::new() };
                packs.push(Item::Folder { name: f.name.clone(), place, pack: true, rec: Some((*n, f)), when });
            } else if mode.all && !is_art(&f.name) {
                art.push(Item::Art { name: f.name.clone(), size: f.size, sauce: None, src: Src::Disk(path), rec: Some((*n, f)), when });
            } else if is_art(&f.name) || is_image(&f.name) {
                let sauce = if is_image(&f.name) { None } else { disk_sauce(&path) };
                art.push(Item::Art { name: f.name.clone(), size: f.size, sauce, src: Src::Disk(path), rec: Some((*n, f)), when });
            }
        }
        out.extend(packs);
        out.extend(art);
        Ok(out)
    }

    /// The listing, dressed like the file lister: `screen` (GALLERY.ANS or
    /// LISTFILE.ANS) if there is one, else LISTFILE.ANS (the same markers: ~TL/~BL the list rows,
    /// ~FN/~FS/~FD the columns, ~PI the info line, ~PM the menu bar,
    /// ~C1-~C6 the colours, ~FA where we are), else a plain screen. Up and
    /// down move the bar, left and right the menu; Enter runs the menu's
    /// choice, or a key its first letter.
    async fn gallery_pick(&mut self, screen: &str, place: &str, items: &[Item], start: usize, speed: &mut u32) -> io::Result<Pick> {
        // New: info and edit, as the file lister has them; edit for a co-SysOp and up.
        let cosysop = self.acs(&self.state.config().acs.cosysop.clone());
        let mut menu = MenuBar {
            items: &["open", "play", "speed", "reorder", "get", "info", "edit", "back"],
            off: vec![false, false, false, false, false, false, !cosysop, false],
            pos: 0,
            xs: Vec::new(),
        };
        let mut cur = start.min(items.len().saturating_sub(1));
        let mut first = 0usize;
        let mut l = self.lister_layout(false);
        let mut redraw = true;
        // A word in place of the info line, after a key that can't act - or, on
        // a fresh listing in an order other than by name, which order it is.
        let mut note: Option<String> = (self.gallery_order != Order::Name)
            .then(|| format!("in order of {} - R to reorder", self.gallery_order.word()));
        // New: the screen's effects (GALLERY.FX) run while it waits.
        let mut amb: Option<crate::ambient::Ambient> = None;
        loop {
            let height = (l.bot - l.top + 1) as usize;
            if cur < first || cur >= first + height {
                redraw = true;
            }
            if redraw {
                if cur < first {
                    first = cur;
                } else if cur >= first + height {
                    first = cur + 1 - height;
                }
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.clear_screen();
                // New: what's here, for the screen's header.
                // Packs open as folders but count as files.
                let files = items.iter().filter_map(|i| match i {
                    Item::Art { size, .. } => Some(*size),
                    Item::Folder { pack: true, rec, .. } => Some(rec.as_ref().map_or(0, |r| r.1.size)),
                    Item::Folder { .. } => None,
                });
                let w = waves([
                    ("FA", place.to_string()),
                    ("NA", crate::files::commas(files.clone().count() as u64)),
                    ("NF", crate::files::commas(items.iter().filter(|i| matches!(i, Item::Folder { pack: false, .. })).count() as u64)),
                    ("SZ", crate::upscan::human_size(files.sum())),
                ]);
                let mut from_screen = self.show_file_with(screen, &w).await?;
                if !from_screen {
                    from_screen = self.show_file_with("LISTFILE", &w).await?;
                }
                if !from_screen {
                    self.screen.goto(1, 1);
                    self.show(&format!("|U4 Art gallery |U5- |U6{}", fluxbbs_term::lit(place))).await?;
                    self.screen.goto(1, 2);
                    self.set_slot(Slot::Border);
                    self.write(&"─".repeat(79));
                }
                l = self.lister_layout(from_screen);
                let height = (l.bot - l.top + 1) as usize;
                if cur >= first + height {
                    first = cur + 1 - height;
                }
                for y in l.top..=l.bot {
                    self.screen.goto(1, y);
                    self.screen.set_color(l.cl[0]);
                    self.screen.clear_eol();
                }
                for (k, item) in items.iter().enumerate().skip(first).take(height) {
                    self.gallery_row(&l, item, l.top + (k - first) as u16, k == cur);
                }
                if items.is_empty() {
                    self.screen.goto(l.name_x.unwrap_or(2), l.top);
                    self.screen.set_color(l.cl[1]);
                    self.write("(nothing here)");
                }
                let last = (first + height).min(items.len()).saturating_sub(1);
                self.lister_scroll(&l, first, last, items.len().max(1));
                self.lister_menu(&l, &mut menu);
                redraw = false;
                amb = if from_screen && self.animating() {
                    self.find_text_file(screen).and_then(|p| crate::ambient::sidecar(&p)).and_then(|fx| crate::ambient::Ambient::new(&self.screen, &self.paced(&fx)))
                } else {
                    None
                };
            }
            let info = match note.take() {
                Some(n) => n,
                None => gallery_info(items.get(cur), *speed),
            };
            self.lister_info(&l, &info).await?;
            self.lister_park(&l);
            // New: a file's whole description pops up once the bar rests on
            // it, for callers who turned that on (file popups, in user config).
            let popped = match items.get(cur).and_then(record) {
                Some((_, f)) if self.user.file_popups && !f.description.is_empty() => self.gallery_popup(&l, f, l.top + (cur - first) as u16).await?,
                _ => None,
            };
            let Some(key) = self.read_key_ambient(&mut amb, false).await? else {
                redraw = true;
                continue;
            };
            // The rows it covered, as they were.
            if let Some((y0, y1)) = popped {
                for y in y0..=y1 {
                    self.screen.goto(1, y);
                    self.screen.set_color(l.cl[0]);
                    self.screen.clear_eol();
                    let k = first + (y - l.top) as usize;
                    if let Some(item) = items.get(k) {
                        self.gallery_row(&l, item, y, k == cur);
                    }
                }
            }
            let prev = cur;
            let height = (l.bot - l.top + 1) as usize;
            let choice = match key {
                Key::Up => {
                    cur = cur.saturating_sub(1);
                    None
                }
                Key::Down => {
                    cur = (cur + 1).min(items.len().saturating_sub(1));
                    None
                }
                Key::PgUp => {
                    cur = cur.saturating_sub(height);
                    None
                }
                Key::PgDn => {
                    cur = (cur + height).min(items.len().saturating_sub(1));
                    None
                }
                Key::Home => {
                    cur = 0;
                    None
                }
                Key::End => {
                    cur = items.len().saturating_sub(1);
                    None
                }
                Key::Left | Key::Right => {
                    let n = menu.items.len();
                    menu.pos = if key == Key::Right { (menu.pos + 1) % n } else { (menu.pos + n - 1) % n };
                    self.lister_menu(&l, &mut menu);
                    None
                }
                Key::Enter => Some(menu.items[menu.pos]),
                Key::Esc | Key::Backspace => Some("back"),
                k => match k.upper() {
                    Some('O') => Some("open"),
                    Some('P') => Some("play"),
                    Some('S') => Some("speed"),
                    // New: R cycles the order (S is speed, O is open).
                    Some('R') => Some("reorder"),
                    Some('G' | 'D') => Some("get"),
                    Some('I') => Some("info"),
                    Some('E') if cosysop => Some("edit"),
                    Some('B') => Some("back"),
                    // Q leaves altogether; back goes up a level.
                    Some('Q') => return Ok(Pick::Quit),
                    _ => None,
                },
            };
            if let Some(c) = choice {
                if let Some(i) = menu.items.iter().position(|it| *it == c)
                    && i != menu.pos
                {
                    menu.pos = i;
                    self.lister_menu(&l, &mut menu);
                }
                match c {
                    "open" if !items.is_empty() => return Ok(Pick::Open(cur)),
                    "play" if matches!(items.get(cur), Some(Item::Art { name, .. }) if is_art(name) || is_image(name) || is_pdf(name)) => return Ok(Pick::Play(cur)),
                    "play" => note = Some(self.strings().get("GalleryPlayWhat").to_string()),
                    "get" if items.get(cur).is_some_and(downloadable) => return Ok(Pick::Download(cur)),
                    "get" => note = Some(self.strings().get("GalleryNotFile").to_string()),
                    "speed" => {
                        let i = SPEEDS.iter().position(|s| s == speed).map_or(0, |i| (i + 1) % SPEEDS.len());
                        *speed = SPEEDS[i];
                        let shown = if *speed == 0 { "instant".to_string() } else { format!("{speed}") };
                        note = Some(self.strings().fmt("GallerySpeed", &[&shown]));
                    }
                    "reorder" => return Ok(Pick::Resort(cur)),
                    "info" if items.get(cur).is_some_and(|i| record(i).is_some()) => return Ok(Pick::Info(cur)),
                    "edit" if items.get(cur).is_some_and(|i| record(i).is_some()) => return Ok(Pick::Edit(cur)),
                    "info" | "edit" => note = Some(self.strings().get("GalleryNotRecord").to_string()),
                    "back" => return Ok(Pick::Back),
                    _ => {}
                }
            }
            if cur != prev && cur >= first && cur < first + height {
                self.gallery_row(&l, &items[prev], l.top + (prev - first) as u16, false);
                self.gallery_row(&l, &items[cur], l.top + (cur - first) as u16, true);
            }
        }
    }

    /// One row in the lister's columns: name (highlighted when it's the
    /// bar's), size, and title by artist (or a pack's description).
    /// New: once the bar has rested on a file for a moment, its whole
    /// description in a box beside the bar, kept within the list's rows so
    /// nothing scrolls; the rows it covers, or None if a key came first.
    async fn gallery_popup(&mut self, l: &Layout, f: &crate::filebase::FileRec, bar: u16) -> io::Result<Option<(u16, u16)>> {
        self.flush().await?;
        let rest = tokio::time::Instant::now();
        while rest.elapsed() < POPUP_AFTER {
            if self.key_waiting().await? {
                return Ok(None);
            }
        }
        let x0 = l.name_x.unwrap_or(2).max(2) + 2;
        let w = (80 - x0 as usize).min(66);
        let inner = w - 4;
        // Whichever side of the bar has more room, never over the bar itself.
        let (below, above) = ((l.bot - bar) as usize, (bar - l.top) as usize);
        let room = below.max(above).min(POPUP_ROWS);
        if room < 3 {
            return Ok(None);
        }
        let mut lines: Vec<String> = f.description.iter().flat_map(|d| crate::multinode::wrap(d, inner)).collect();
        if lines.len() > room - 2 {
            lines.truncate(room - 2);
            if let Some(last) = lines.last_mut() {
                *last = format!("{}...", last.chars().take(inner - 3).collect::<String>());
            }
        }
        let h = lines.len() as u16 + 2;
        let y0 = if below >= h as usize || below >= above { bar + 1 } else { bar - h };
        let title: String = f.name.chars().take(inner - 2).collect();
        self.screen.goto(x0, y0);
        self.set_slot(Slot::Border);
        self.write(&format!("┌─ {title} {}┐", "─".repeat(w - 5 - title.chars().count())));
        for (i, line) in lines.iter().enumerate() {
            self.screen.goto(x0, y0 + 1 + i as u16);
            self.set_slot(Slot::Border);
            self.write("│ ");
            self.set_slot(Slot::Text);
            let pad = inner.saturating_sub(line.chars().count());
            self.write(&format!("{line}{}", " ".repeat(pad)));
            self.set_slot(Slot::Border);
            self.write(" │");
        }
        self.screen.goto(x0, y0 + h - 1);
        self.set_slot(Slot::Border);
        self.write(&format!("└{}┘", "─".repeat(w - 2)));
        self.lister_park(l);
        self.flush().await?;
        Ok(Some((y0, y0 + h - 1)))
    }

    fn gallery_row(&mut self, l: &Layout, item: &Item, y: u16, hi: bool) {
        let name_x = l.name_x.unwrap_or(2);
        let size_x = l.size_x.filter(|&x| x > name_x);
        let desc_x = l.desc_x.unwrap_or(30).max(name_x + 13);
        let name_w = (size_x.unwrap_or(desc_x) - name_x).saturating_sub(1) as usize;
        let (name, size, title, by) = match item {
            Item::Folder { name, place: Place::Area(c), .. } => {
                // A file area: how many files, and the newest, counting
                // the sub-areas under it the caller can see (as the
                // change-area list does).
                let fa = self.state.fareas();
                let visible = self.visible_fareas();
                let (mut count, mut newest) = (0, None);
                for m in fa.subtree(*c).into_iter().filter(|m| visible.iter().any(|&(v, _)| v == *m)) {
                    if let Some(a) = fa.get(m)
                        && let Ok((n, d)) = self.state.files.summary(&a.file) {
                            count += n;
                            newest = newest.max(d);
                        }
                }
                let count = if count == 1 { "1 file".to_string() } else { format!("{count} files") };
                let newest = newest.map(|d| format!("newest {}", d.format("%Y-%m-%d"))).unwrap_or_else(|| "empty".into());
                (format!("{name}/"), count, newest, String::new())
            }
            Item::Folder { name, pack, rec, .. } => {
                let what = match rec {
                    Some((_, f)) if !f.description.is_empty() => crate::web::blurb(&f.description),
                    _ if *pack => "archive".to_string(),
                    _ => "folder".to_string(),
                };
                let size = rec.as_ref().map(|(_, f)| crate::files::short_size(f.size)).unwrap_or_default();
                (format!("{name}/"), size, what, String::new())
            }
            Item::Art { name, size, sauce, rec, .. } => {
                let (title, by) = match (sauce, rec) {
                    // A title that's only the file's name (or a tool's
                    // hash) gives way to the area's description.
                    (Some(s), _) if !crate::aidesc::says_nothing(&s.title, name) => (s.title.clone(), byline(s)),
                    // A file of a file area: its description.
                    (_, Some((_, f))) if !f.description.is_empty() => (crate::web::blurb(&f.description), String::new()),
                    (Some(s), _) => (s.title.clone(), byline(s)),
                    (None, _) => (kind_word(name).to_string(), String::new()),
                };
                (name.clone(), crate::files::short_size(*size), title, by)
            }
        };
        self.screen.goto(name_x.saturating_sub(1).max(1), y);
        self.screen.set_color(if hi { l.cl[3] } else { l.cl[0] });
        // New: a name cut short says so, as DOS's short names did.
        let name = if name.chars().count() > name_w && name_w > 1 { format!("{}~", name.chars().take(name_w - 1).collect::<String>()) } else { name };
        self.write(&format!(" {} ", resize(&name, name_w)));
        if let Some(x) = size_x {
            self.screen.goto(x, y);
            self.screen.set_color(l.cl[0]);
            self.write(&resize(&size, (desc_x - x).saturating_sub(1) as usize));
        }
        // New: the piece's group and date (its SAUCE) at the far right,
        // so the row reaches the edge.
        let tail = match item {
            Item::Art { sauce: Some(s), .. } => {
                let year: String = s.date.chars().take(4).collect();
                [s.group.as_str(), year.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" · ")
            }
            _ => String::new(),
        };
        self.screen.goto(desc_x, y);
        let room = 79usize.saturating_sub(desc_x as usize).saturating_sub(if tail.is_empty() { 0 } else { tail.chars().count() + 2 });
        self.screen.set_color(l.cl[0]);
        let title: String = title.chars().take(room).collect();
        self.write(&title);
        let left = room.saturating_sub(title.chars().count());
        if !by.is_empty() && left > 4 {
            self.screen.set_color(l.cl[1]);
            self.write(&squish(&format!(" by {by}"), left));
        }
        self.screen.set_color(l.cl[0]);
        self.screen.clear_eol();
        if !tail.is_empty() {
            self.screen.goto((80 - tail.chars().count()) as u16, y);
            self.screen.set_color(l.cl[1]);
            self.write(&tail);
            self.screen.set_color(l.cl[0]);
        }
    }

    /// A picture or a PDF in a file area, from the file lister or editor
    /// (new); false if it's neither, for the caller to view another way.
    pub(crate) async fn view_disk_media(&mut self, name: &str, path: &Path) -> io::Result<bool> {
        let src = Src::Disk(path.to_path_buf());
        if is_pdf(name) {
            self.pdf_open(name, &src, false).await?;
        } else if is_image(name) {
            self.gallery_picture(name, &src).await?;
        } else {
            return Ok(false);
        }
        self.clear_screen();
        Ok(true)
    }

    /// Without PDFs built in (`pdf`) there's no PDF to open; `is_pdf`
    /// never says there is.
    #[cfg(not(feature = "pdf"))]
    async fn pdf_open(&mut self, _name: &str, _src: &Src, _pages: bool) -> io::Result<Option<char>> {
        Ok(None)
    }

    /// A PDF: its text (or with `pages`, its pages), switching between the
    /// two with P and T. D is passed back to download it. One inside an
    /// archive is copied out first (Poppler reads files).
    #[cfg(feature = "pdf")]
    async fn pdf_open(&mut self, name: &str, src: &Src, mut pages: bool) -> io::Result<Option<char>> {
        let path = match src {
            Src::Disk(p) => p.clone(),
            Src::Zip(z, n) | Src::Tar(z, _, n) => {
                let cfg = self.state.config();
                let dir = cfg.resolve(&cfg.paths.data).join("cache").join("pdf");
                let max = crate::upscan::Limits::from_config(&cfg.files).max_unpacked.max(MAX_PIECE);
                drop(cfg);
                let out = dir.join(format!("{:016x}.pdf", {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (z, n).hash(&mut h);
                    std::fs::metadata(z).and_then(|m| m.modified()).ok().hash(&mut h);
                    h.finish()
                }));
                if !out.exists() {
                    let data = match read_limited(src, max) {
                        Ok(d) => d,
                        Err(e) => return self.archive_note(&format!("|U6{name}|U5: |U4Couldn't read it ({e})|U5.")).await.map(|_| None),
                    };
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(&out, data)?;
                }
                out
            }
        };
        loop {
            let got = if pages { self.pdf_pages(name, &path).await? } else { self.pdf_text(name, &path).await? };
            match got {
                Some('P') => pages = true,
                Some('T') => pages = false,
                other => return Ok(other),
            }
        }
    }

    /// Any other file: text in the viewer, else a word that it can't be
    /// shown. D is passed back to download it.
    async fn gallery_file(&mut self, name: &str, src: &Src, area: &str) -> io::Result<Option<char>> {
        let data = match read_piece(src) {
            Ok(d) => d,
            Err(e) => return self.archive_note(&format!("|U6{name}|U5: |U4Couldn't read it ({e})|U5.")).await.map(|_| None),
        };
        let text = crate::upscan::text_part(&data);
        if !crate::upscan::looks_like_text(text) {
            let s = self.strings().fmt("ArchiveCantShow", &[name]);
            return self.archive_note(&s).await.map(|_| None);
        }
        let utf8 = std::str::from_utf8(text).is_ok_and(|t| !t.is_ascii());
        let grid = crate::pager::trimmed(crate::pager::render_named(text, utf8, name, &self.look()));
        let title = format!("{name} in {}", crate::editor::strip_colours(area));
        self.page_grid_keys(&title, &grid, "GalleryViewHelp", &['D', 'Q']).await
    }

    /// A piece in the scrolling viewer, titled from its SAUCE.
    async fn gallery_view(&mut self, name: &str, sauce: Option<&Sauce>, src: &Src) -> io::Result<Option<char>> {
        let data = match read_piece(src) {
            Ok(d) => d,
            Err(e) => return self.show_line(&format!("|U0{name}: {e}")).await.map(|_| None),
        };
        let text = crate::upscan::text_part(&data);
        let utf8 = std::str::from_utf8(text).is_ok_and(|t| !t.is_ascii());
        let grid = crate::pager::render(text, utf8);
        let title = match sauce {
            Some(s) if !s.title.is_empty() && !s.author.is_empty() => format!("{name}: {} by {}", s.title, s.author),
            Some(s) if !s.title.is_empty() => format!("{name}: {}", s.title),
            _ => name.to_string(),
        };
        self.page_grid_keys(&title, &grid, "GalleryViewHelp", &['P', 'D', 'Q']).await
    }

    /// A picture as ANSI, fitted to the screen; an animated GIF plays until
    /// a key (or five minutes). P plays it again; D is passed back to
    /// download it.
    async fn gallery_picture(&mut self, name: &str, src: &Src) -> io::Result<Option<char>> {
        let (file, inner) = match src {
            Src::Disk(p) => (p.clone(), String::new()),
            Src::Zip(z, n) | Src::Tar(z, _, n) => (z.clone(), n.clone()),
        };
        let max = self.state.config().files.image_limits().max_bytes.max(MAX_PIECE) + 1;
        let src = src.clone();
        self.show_picture(name, file, inner, move || read_limited(&src, max)).await
    }

    /// A picture whose bytes `read` gets (off the async threads), cached
    /// by `file` and `inner` (a name inside it, if any). See
    /// [`Session::gallery_picture`].
    pub(crate) async fn show_picture(
        &mut self,
        name: &str,
        file: PathBuf,
        inner: String,
        read: impl FnOnce() -> io::Result<Vec<u8>> + Send + 'static,
    ) -> io::Result<Option<char>> {
        Ok(match self.show_picture_key(name, None, file, inner, read).await? {
            Some(k) if k.upper() == Some('D') => Some('D'),
            _ => None,
        })
    }

    /// [`Session::show_picture`], giving back the key that ended it (P,
    /// play again, is handled here); `None` if it couldn't be shown.
    /// `caption`, if given, is the bottom line in place of the usual one.
    pub(crate) async fn show_picture_key(
        &mut self,
        name: &str,
        caption: Option<String>,
        file: PathBuf,
        inner: String,
        read: impl FnOnce() -> io::Result<Vec<u8>> + Send + 'static,
    ) -> io::Result<Option<Key>> {
        let (cols, rows) = (80usize, self.screen.rows().saturating_sub(1).max(2) as usize);
        let lim = self.state.config().files.image_limits();
        let stamp = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let key = (file, inner, stamp, cols, rows);
        let pic = match picture::cached(&key) {
            Some(p) => p,
            None => {
                let loaded = tokio::task::spawn_blocking(move || -> io::Result<Result<picture::Picture, String>> {
                    let data = read()?;
                    Ok(picture::load(&data, cols, rows, lim))
                })
                .await
                .map_err(io::Error::other)??;
                match loaded {
                    Ok(p) => {
                        let p = std::sync::Arc::new(p);
                        picture::remember(key, p.clone());
                        p
                    }
                    Err(why) => {
                        self.screen.goto(1, self.screen.rows());
                        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                        self.screen.clear_eol();
                        // One line and no "press a key" after it, as below:
                        // a wrap on the bottom row scrolls the screen.
                        let mut s = self.strings().fmt("GalleryPictureBad", &[name, &why]);
                        if self.visible_len(&fluxbbs_term::cp437::encode(&s)) > 79 {
                            s = self.strings().fmt("GalleryPictureBad", &[&squish(name, 16), &why]);
                        }
                        self.show(&s).await?;
                        self.flush().await?;
                        self.read_key().await?;
                        return Ok(None);
                    }
                }
            }
        };
        let truecolor = self.screen.truecolor() || self.screen.charset() == fluxbbs_term::Charset::Utf8;
        let first = &pic.frames[0];
        let left = ((cols - first.w) / 2 + 1) as u16;
        let top = ((rows - first.h / 2) / 2 + 1) as u16;
        loop {
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.clear_screen();
            self.screen.goto(1, self.screen.rows());
            // Kept to one line: the bottom row mustn't scroll.
            let mut s = caption.clone().unwrap_or_else(|| self.strings().fmt("GalleryPicture", &[name, &pic.describe()]));
            if caption.is_none() && self.visible_len(&fluxbbs_term::cp437::encode(&s)) > 79 {
                s = self.strings().fmt("GalleryPicture", &[&squish(name, 16), &pic.describe()]);
            }
            self.show(&s).await?;
            let mut prev = None;
            let started = std::time::Instant::now();
            // Frames go out no faster than the terminal shows them (one
            // drawing, one on the way); one that never answers is paced by
            // the clock alone.
            let mut sync = true;
            // Start counting afresh: nothing old is still owed.
            self.cursor_asks = self.cursor_asks.min(self.cursor_reports);
            let key = 'play: loop {
                for f in &pic.frames {
                    self.screen.write_bytes(&picture::draw(f, prev, truecolor, left, top));
                    self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                    self.screen.goto(1, self.screen.rows());
                    self.flush().await?;
                    prev = Some(f);
                    if pic.frames.len() == 1 {
                        break 'play self.read_key().await?;
                    }
                    let until = std::time::Instant::now() + std::time::Duration::from_millis(f.delay_ms);
                    if sync {
                        let answering = self.cursor_reports > 0;
                        let wait = std::time::Duration::from_secs(if answering { 10 } else { 3 });
                        if !self.sync_output(wait, 1).await? && !answering {
                            sync = false;
                        }
                    }
                    loop {
                        if self.key_waiting().await? {
                            break 'play self.read_key().await?;
                        }
                        if std::time::Instant::now() >= until {
                            break;
                        }
                    }
                }
                if started.elapsed() > std::time::Duration::from_secs(300) {
                    break 'play Key::Esc;
                }
            };
            match key.upper() {
                Some('P') => continue,
                _ => return Ok(Some(key)),
            }
        }
    }

    /// Send a piece as it is, paced to a modem speed; any key stops it.
    async fn gallery_play(&mut self, src: &Src, speed: u32) -> io::Result<()> {
        let data = match read_piece(src) {
            Ok(d) => playable(&fluxbbs_term::cp437::dos_art(&d)),
            Err(e) => return self.show_line(&format!("|U0{e}")).await,
        };
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        // Bytes per 40ms tick (a modem sends a byte in 10 bits).
        let chunk = if speed == 0 { data.len().max(1) } else { (speed as usize / 250).max(1) };
        for piece in data.chunks(chunk) {
            self.screen.write_bytes(piece);
            self.flush().await?;
            if speed > 0 && self.key_waiting().await? {
                self.read_key().await?;
                break;
            }
        }
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        Ok(())
    }

    /// Play a piece, then offer what's next on the bottom line (new; the
    /// original just paused): R plays it again, V (or the arrow and page
    /// keys) opens it in the scrolling viewer, D downloads it.
    async fn gallery_played(&mut self, item: &Item, speed: u32) -> io::Result<()> {
        let Item::Art { name, sauce, src, .. } = item else { return Ok(()) };
        loop {
            self.gallery_play(src, speed).await?;
            self.screen.goto(1, self.screen.rows());
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.screen.clear_eol();
            let bar = self.strings().get("GalleryPlayed").to_string();
            self.show(&bar).await?;
            let key = self.read_key().await?;
            let next = match key {
                Key::Up | Key::Down | Key::PgUp | Key::PgDn | Key::Home | Key::End => Some('V'),
                k => k.upper(),
            };
            match next {
                Some('R' | 'P') => continue,
                Some('V' | 'S') => match self.gallery_view(name, sauce.as_ref(), src).await? {
                    Some('P') => continue,
                    Some('D') => return self.gallery_download(item).await,
                    _ => return Ok(()),
                },
                Some('D') => return self.gallery_download(item).await,
                _ => return Ok(()),
            }
        }
    }

    /// Download a piece (pulled out of its pack if need be) or a pack.
    async fn gallery_download(&mut self, item: &Item) -> io::Result<()> {
        let dl = self.state.config().gallery.download_acs.clone();
        self.screen.goto(1, self.screen.rows());
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
        // A file of a file area: its usual download, checks and all.
        if let Item::Folder { rec: Some((n, f)), .. } | Item::Art { rec: Some((n, f)), .. } = item
            && let Some(a) = self.farea(*n)
        {
            self.newline();
            self.download_record(&a, f.clone()).await?;
            return self.prompt_key().await;
        }
        if !self.acs(&dl) {
            self.say("GalleryNoDownload").await?;
            return self.prompt_key().await;
        }
        let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("gallery{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&temp);
        let file = match item {
            Item::Folder { place: Place::Zip { zip, prefix }, .. } if prefix.is_empty() => zip.clone(),
            Item::Folder { .. } => return Ok(()),
            Item::Art { src: Src::Disk(p), .. } => p.clone(),
            Item::Art { name, src, .. } => {
                std::fs::create_dir_all(&temp)?;
                let out = temp.join(crate::transfer::safe_name(name.rsplit('/').next().unwrap_or(name).as_bytes()).unwrap_or_else(|| "art.ans".into()));
                // Out of its archive, within the upload screening's unpack limit.
                let (Src::Zip(arc, _) | Src::Tar(arc, _, _) | Src::Disk(arc)) = src;
                let cap = crate::upscan::Limits::from_config(&self.state.config().files).cap(std::fs::metadata(arc)?.len());
                let src = src.clone();
                let data = tokio::task::spawn_blocking(move || read_limited(&src, cap)).await.map_err(io::Error::other)??;
                std::fs::write(&out, data)?;
                out
            }
        };
        self.newline();
        if let Some(proto) = self.select_protocol(false).await? {
            tracing::info!("{}: gallery download {}", self.user.handle, file.display());
            self.send_files(&proto, &[file]).await?;
        }
        let _ = std::fs::remove_dir_all(&temp);
        self.prompt_key().await
    }
}

/// "artist/group", or whichever is known.
fn byline(s: &Sauce) -> String {
    match (s.author.is_empty(), s.group.is_empty()) {
        (false, false) => format!("{}/{}", s.author, s.group),
        (false, true) => s.author.clone(),
        (true, false) => s.group.clone(),
        (true, true) => String::new(),
    }
}

/// The info line for the item under the bar.
fn gallery_info(item: Option<&Item>, speed: u32) -> String {
    let speed = if speed == 0 { "instant".to_string() } else { speed.to_string() };
    match item {
        Some(Item::Art { rec: Some((_, f)), .. }) => {
            let date = f.uploaded.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
            let dls = if f.downloads == 1 { "1 download".to_string() } else { format!("{} downloads", f.downloads) };
            format!("|U3{} |U5- |U1{} |U5- |U1up by |U3{}|U1 {date} |U5- |U1{dls}", squish(&f.name, 30), crate::files::short_size(f.size), squish(&f.uploader, 16))
        }
        Some(Item::Art { name, size, .. }) if !is_art(name) => {
            let what = if is_image(name) { "|U3enter|U1 shows it" } else { "|U3enter|U1 views, |U3d|U1 downloads" };
            format!("|U3{} |U5- |U1{} |U5- |U1{} |U5- {what}", squish(name, 40), crate::files::short_size(*size), kind_word(name))
        }
        Some(Item::Art { name, size, sauce, .. }) => {
            let s = sauce.clone().unwrap_or_default();
            let mut line = if s.title.is_empty() { name.clone() } else { s.title.clone() };
            if !byline(&s).is_empty() {
                line.push_str(&format!(" |U5by |U3{}", byline(&s)));
            }
            if !s.date.is_empty() {
                line.push_str(&format!(" |U5({})", s.date));
            }
            format!("|U3{} |U5- |U1{} |U5- |U1play at |U3{speed}", squish(&line, 52), crate::files::short_size(*size))
        }
        Some(Item::Folder { name, pack: true, .. }) => format!("|U3{name} |U5- |U1a pack; |U3enter|U1 opens it"),
        Some(Item::Folder { name, .. }) => format!("|U3{name} |U5- |U1a folder; |U3enter|U1 opens it"),
        None => String::new(),
    }
}

/// Can D act on it: a piece, or a pack (not a folder inside one, nor a
/// plain folder or area).
fn downloadable(item: &Item) -> bool {
    match item {
        Item::Art { .. } => true,
        Item::Folder { rec: Some(_), .. } => true,
        Item::Folder { place: Place::Zip { prefix, .. }, .. } => prefix.is_empty(),
        Item::Folder { .. } => false,
    }
}

enum Pick {
    Back,
    Quit,
    Open(usize),
    Play(usize),
    Download(usize),
    /// New: list the place again in the next order, the bar on this item.
    Resort(usize),
    /// New: the file's record (`faFileInfo`), and the file editor, as in the file lister.
    Info(usize),
    Edit(usize),
}

/// The file base's record for an item, when it is a file in an area (not a
/// folder, or a piece inside a pack): its area number and the record.
/// How long the bar rests on a file before its description pops up, and
/// the most rows the box takes.
const POPUP_AFTER: std::time::Duration = std::time::Duration::from_millis(2500);
const POPUP_ROWS: usize = 12;

fn record(item: &Item) -> Option<&(u32, crate::filebase::FileRec)> {
    match item {
        Item::Art { rec: Some(r), .. } | Item::Folder { rec: Some(r), .. } => Some(r),
        _ => None,
    }
}

/// A PDF, shown as one (with PDFs built in, `pdf`); without, it's just a
/// file to download.
fn is_pdf(name: &str) -> bool {
    #[cfg(feature = "pdf")]
    return crate::pdf::is_pdf(name);
    #[cfg(not(feature = "pdf"))]
    {
        let _ = name;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sauce_rec(title: &str, author: &str, group: &str, date: &str) -> Vec<u8> {
        let mut r = b"SAUCE00".to_vec();
        for (s, n) in [(title, 35), (author, 20), (group, 20), (date, 8)] {
            let mut f = s.as_bytes().to_vec();
            f.resize(n, b' ');
            r.extend(f);
        }
        r.resize(128, 0);
        r
    }

    #[test]
    fn sauce_records() {
        let mut art = b"\x1b[1;31mhi\r\n\x1a".to_vec();
        art.extend(sauce_rec("The PowerGrid", "Somms", "ACiD Productions", "19950601"));
        let s = parse_sauce(&art).unwrap();
        assert_eq!((s.title.as_str(), s.author.as_str(), s.group.as_str(), s.date.as_str()), ("The PowerGrid", "Somms", "ACiD Productions", "1995-06-01"));
        assert!(parse_sauce(b"no sauce here").is_none());
    }

    #[test]
    fn descriptions_from_sauce() {
        let mut art = b"art\r\n\x1aCOMNT".to_vec();
        for c in ["for the disflux file menu", "thanks for looking at this one, it took a while"] {
            let mut l = c.as_bytes().to_vec();
            l.resize(64, b' ');
            art.extend(l);
        }
        let mut rec = sauce_rec("disflux", "phrac", "", "20261018");
        rec[104] = 2;
        art.extend(rec);
        assert_eq!(
            sauce_description(&art, 30, 20).unwrap(),
            ["disflux", "by phrac, 2026-10-18", "for the disflux file menu", "thanks for looking at this", "one, it took a while"]
        );
        assert_eq!(sauce_description(&art, 30, 2).unwrap().len(), 2);
        let mut blank = b"art\x1a".to_vec();
        blank.extend(sauce_rec("", "", "", ""));
        assert!(sauce_description(&blank, 50, 20).is_none(), "an empty SAUCE says nothing");
    }

    #[test]
    fn packs_open_as_folders() {
        let dir = std::env::temp_dir().join(format!("iqgal{}", std::process::id()));
        std::fs::create_dir_all(dir.join("1995")).unwrap();
        let mut art = b"art\r\n\x1a".to_vec();
        art.extend(sauce_rec("Logo", "phrac", "", ""));
        std::fs::write(dir.join("1995/logo.ans"), &art).unwrap();
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(dir.join("acdu0895.zip")).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for n in ["FILE_ID.DIZ", "SO-PG1.ANS", "extra/BIG.ANS", "PLAYER.EXE"] {
                w.start_file(n, o).unwrap();
                w.write_all(b"x").unwrap();
            }
            w.finish().unwrap();
        }
        let names = |items: &[Item]| -> Vec<String> {
            items.iter().map(|i| match i { Item::Folder { name, .. } => format!("{name}/"), Item::Art { name, .. } => name.clone() }).collect()
        };
        let art = Mode { all: false, cap: u64::MAX };
        let top = list(&Place::Dir(dir.clone()), art).unwrap();
        assert_eq!(names(&top), ["1995/", "acdu0895.zip/"]);
        let Item::Folder { place, .. } = &top[1] else { panic!() };
        let pack = list(place, art).unwrap();
        assert_eq!(names(&pack), ["extra/", "FILE_ID.DIZ", "SO-PG1.ANS"], "programs are skipped");
        let Item::Folder { place, .. } = &top[0] else { panic!() };
        let Item::Art { sauce, .. } = &list(place, art).unwrap()[0] else { panic!() };
        assert_eq!(sauce.as_ref().unwrap().author, "phrac");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The help line sits on the bottom row: 80 columns there would scroll
    /// the screen on every key.
    #[test]
    fn bottom_lines_fit() {
        let s = crate::strings::Strings::load(Path::new("/nonexistent")).unwrap();
        let visible = |t: &str| {
            let t = fluxbbs_term::cp437::encode(t);
            fluxbbs_term::tokenize(&t, fluxbbs_term::Source::String)
                .iter()
                .map(|tok| if let fluxbbs_term::Token::Text(b) = tok { b.len() } else { 0 })
                .sum::<usize>()
        };
        assert!(visible(&s.fmt("GalleryHelp", &["115200"])) < 80);
        assert!(visible(s.get("ViewerHelp")) < 80);
        assert!(visible(s.get("GalleryViewHelp")) < 80);
        for k in ["GalleryPlayWhat", "GalleryNotFile"] {
            assert!(visible(s.get(k)) < 80, "{k}");
        }
        assert!(visible(&s.fmt("GallerySpeed", &["115200"])) < 80);
        assert!(visible(&s.fmt("ViewerTitle", &[&"x".repeat(38), "9977-9999 of 9999"])) < 80);
    }

    #[test]
    fn playback_is_defanged() {
        let p = playable(b"\x1b[1;31mA\x1b]0;t\x07\x1b[6n\x00B\r\n\x1aSAUCE00");
        assert_eq!(p, b"\x1b[1;31mA]0;t B\r\n".to_vec());
    }

    fn piece(name: &str, size: u64, when: Option<(i32, u32, u32)>) -> Item {
        Item::Art {
            name: name.into(),
            size,
            sauce: None,
            src: Src::Disk(PathBuf::from(name)),
            rec: None,
            when: when.and_then(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d)),
        }
    }

    fn names(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                Item::Folder { name, .. } | Item::Art { name, .. } => name.clone(),
            })
            .collect()
    }

    #[test]
    fn the_gallery_sorts_by_newest_oldest_type_and_size_with_folders_first() {
        let folder = Item::Folder { name: "packs".into(), place: Place::Areas, pack: false, rec: None, when: None };
        let mut items = vec![
            folder.clone(),
            piece("b.ans", 300, Some((2026, 10, 1))),
            piece("a.png", 900, Some((2026, 10, 30))),
            piece("c.ans", 50, None),
            piece("d.gif", 500, Some((2025, 1, 1))),
        ];
        sort_items(&mut items, Order::Newest);
        assert_eq!(names(&items), ["packs", "a.png", "b.ans", "d.gif", "c.ans"], "undated last");
        sort_items(&mut items, Order::Oldest);
        assert_eq!(names(&items), ["packs", "d.gif", "b.ans", "a.png", "c.ans"]);
        sort_items(&mut items, Order::Type);
        assert_eq!(names(&items), ["packs", "b.ans", "c.ans", "d.gif", "a.png"]);
        sort_items(&mut items, Order::Size);
        assert_eq!(names(&items), ["packs", "a.png", "d.gif", "b.ans", "c.ans"]);
        sort_items(&mut items, Order::Name);
        assert_eq!(names(&items), ["packs", "a.png", "b.ans", "c.ans", "d.gif"]);
        assert_eq!(Order::Size.next(), Order::Name, "the orders go round");
    }
}
