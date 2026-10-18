//! The art gallery (`AG`; new, after Mystic's): a folder of art, browsed
//! like a file manager, with ZIP packs opened as folders in place. Each
//! piece lists its SAUCE title, artist and group; Enter shows it in the
//! scrolling viewer, P plays it at a modem speed as it drew in its day
//! (animated ANSIs come alive this way), and D downloads the piece or
//! the whole pack.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use iniquity_term::Slot;
use iniquity_term::codes::{resize, squish};

use crate::filebase::FileRec;
use crate::input::Key;
use crate::session::Session;

/// Art the gallery shows; anything else in a pack is skipped.
const ART: [&str; 7] = ["ans", "ice", "asc", "txt", "nfo", "diz", "cia"];
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
    let s = iniquity_term::cp437::decode(b);
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

/// Where a folder is: a real directory, a file area (its sub-areas and
/// files), or a path inside a ZIP.
#[derive(Debug, Clone, PartialEq)]
enum Place {
    Dir(PathBuf),
    Area(u32),
    Zip { zip: PathBuf, prefix: String },
}

/// Where a piece is.
#[derive(Debug, Clone, PartialEq)]
enum Src {
    Disk(PathBuf),
    Zip(PathBuf, String),
}

/// A file area's record for an item that is one of its files: downloads
/// of these go through the area's checks and accounting.
type Rec = Option<(u32, FileRec)>;

#[derive(Debug, Clone)]
enum Item {
    Folder { name: String, place: Place, pack: bool, rec: Rec },
    Art { name: String, size: u64, sauce: Option<Sauce>, src: Src, rec: Rec },
}

fn is_art(name: &str) -> bool {
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
    }
}

/// What's in a folder: folders and packs first, then pieces, by name.
fn list(place: &Place) -> io::Result<Vec<Item>> {
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
                    folders.push(Item::Folder { name, place: Place::Dir(path), pack: false, rec: None });
                } else if is_zip(&name) {
                    folders.push(Item::Folder { name, place: Place::Zip { zip: path, prefix: String::new() }, pack: true, rec: None });
                } else if is_art(&name) {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    art.push(Item::Art { sauce: disk_sauce(&path), name, size, src: Src::Disk(path), rec: None });
                }
            }
        }
        // Listed by the session, which knows what the caller may see.
        Place::Area(_) => {}
        Place::Zip { zip, prefix } => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(zip)?).map_err(io::Error::other)?;
            let mut seen = Vec::new();
            for i in 0..z.len() {
                let Ok(mut f) = z.by_index(i) else { continue };
                let Some(full) = f.enclosed_name().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
                let Some(rest) = full.strip_prefix(prefix.as_str()) else { continue };
                if let Some((dir, _)) = rest.split_once('/') {
                    if !seen.contains(&dir.to_string()) {
                        seen.push(dir.to_string());
                        let place = Place::Zip { zip: zip.clone(), prefix: format!("{prefix}{dir}/") };
                        folders.push(Item::Folder { name: dir.to_string(), place, pack: false, rec: None });
                    }
                } else if !f.is_dir() && is_art(rest) {
                    // Reading the piece for its SAUCE: art files are small.
                    let mut data = Vec::new();
                    let _ = (&mut f).take(MAX_PIECE).read_to_end(&mut data);
                    art.push(Item::Art { name: rest.to_string(), size: f.size(), sauce: parse_sauce(&data), src: Src::Zip(zip.clone(), full), rec: None });
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
        let cfg = self.state.config();
        let dir = cfg.resolve(&cfg.gallery.path);
        let (acs, area, mut speed) = (cfg.gallery.acs.clone(), cfg.gallery.area.trim().to_string(), cfg.gallery.speed);
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
        // Where we are, and the cursor in each level above.
        let mut path: Vec<(String, Place, usize)> = vec![(String::new(), root, 0)];
        loop {
            let (_, place, _) = path.last().cloned().expect("root");
            let items = self.gallery_list(&place).await?;
            let crumbs: Vec<String> = path.iter().skip(1).map(|(n, _, _)| n.clone()).collect();
            let at = path.last().map(|l| l.2).unwrap_or(0);
            match self.gallery_pick(&crumbs.join("/"), &items, at, &mut speed).await? {
                Pick::Back if path.len() > 1 => {
                    path.pop();
                }
                Pick::Back => {
                    // Leave a clean screen for what comes next.
                    self.screen.set_color(iniquity_term::Color::DEFAULT);
                    self.clear_screen();
                    return Ok(());
                }
                Pick::Open(i) => {
                    path.last_mut().expect("root").2 = i;
                    match &items[i] {
                        Item::Folder { name, place, .. } => path.push((name.clone(), place.clone(), 0)),
                        Item::Art { name, sauce, src, .. } => {
                            // From the viewer, P plays and D downloads.
                            match self.gallery_view(name, sauce.as_ref(), src).await? {
                                Some('P') => self.gallery_play(src, speed).await?,
                                Some('D') => self.gallery_download(&items[i]).await?,
                                _ => {}
                            }
                        }
                    }
                }
                Pick::Play(i) => {
                    path.last_mut().expect("root").2 = i;
                    if let Item::Art { src, .. } = &items[i] {
                        self.gallery_play(src, speed).await?;
                    }
                }
                Pick::Download(i) => {
                    path.last_mut().expect("root").2 = i;
                    self.gallery_download(&items[i]).await?;
                }
            }
        }
    }

    /// What's in a place. A file area shows the sub-areas the caller can
    /// see, then its validated files: packs as folders, art as pieces.
    async fn gallery_list(&mut self, place: &Place) -> io::Result<Vec<Item>> {
        let Place::Area(n) = place else {
            let p = place.clone();
            return tokio::task::spawn_blocking(move || list(&p)).await.map_err(io::Error::other)?;
        };
        let fa = self.state.fareas();
        let mut out = Vec::new();
        for c in self.farea_children(Some(*n)) {
            out.push(Item::Folder { name: fa.areas[c as usize - 1].name.clone(), place: Place::Area(c), pack: false, rec: None });
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
            if is_zip(&f.name) {
                let place = Place::Zip { zip: path, prefix: String::new() };
                packs.push(Item::Folder { name: f.name.clone(), place, pack: true, rec: Some((*n, f)) });
            } else if is_art(&f.name) {
                let sauce = disk_sauce(&path);
                art.push(Item::Art { name: f.name.clone(), size: f.size, sauce, src: Src::Disk(path), rec: Some((*n, f)) });
            }
        }
        out.extend(packs);
        out.extend(art);
        Ok(out)
    }

    /// The listing, with a bar to move; returns what the caller chose.
    async fn gallery_pick(&mut self, crumbs: &str, items: &[Item], start: usize, speed: &mut u32) -> io::Result<Pick> {
        let rows = self.screen.rows().max(8);
        let height = (rows - 4) as usize;
        let mut cur = start.min(items.len().saturating_sub(1));
        let mut top = cur.saturating_sub(height / 2);
        let mut redraw = true;
        // A word in place of the help line, after a key that can't act.
        let mut note: Option<String> = None;
        loop {
            if cur < top {
                top = cur;
            } else if cur >= top + height {
                top = cur + 1 - height;
            }
            if redraw {
                self.clear_screen();
                let title = if crumbs.is_empty() { String::new() } else { format!(": |U3{crumbs}") };
                self.show(&format!("|U4 Art gallery{title}")).await?;
                self.screen.goto(1, 2);
                self.rule().await?;
                for (k, item) in items.iter().enumerate().skip(top).take(height) {
                    self.gallery_row(item, 3 + (k - top) as u16, k == cur);
                }
                if items.is_empty() {
                    self.screen.goto(2, 3);
                    self.set_slot(Slot::Info);
                    self.write("(nothing here)");
                }
                self.screen.goto(1, rows - 1);
                self.rule().await?;
                redraw = false;
            }
            self.screen.goto(1, rows);
            self.screen.set_color(iniquity_term::Color::DEFAULT);
            self.screen.clear_eol();
            let shown = if *speed == 0 { "instant".to_string() } else { format!("{speed}") };
            let s = match note.take() {
                Some(n) => n,
                None => self.strings().fmt("GalleryHelp", &[&shown]),
            };
            self.show(&s).await?;
            let key = self.read_key().await?;
            let prev = cur;
            match key {
                Key::Up => cur = cur.saturating_sub(1),
                Key::Down => cur = (cur + 1).min(items.len().saturating_sub(1)),
                Key::PgUp => cur = cur.saturating_sub(height),
                Key::PgDn => cur = (cur + height).min(items.len().saturating_sub(1)),
                Key::Home => cur = 0,
                Key::End => cur = items.len().saturating_sub(1),
                Key::Enter | Key::Right if !items.is_empty() => return Ok(Pick::Open(cur)),
                Key::Left | Key::Backspace | Key::Esc => return Ok(Pick::Back),
                k => match k.upper() {
                    // Up a level, like Esc; out of the gallery from the top.
                    Some('Q') => return Ok(Pick::Back),
                    Some('P') if matches!(items.get(cur), Some(Item::Art { .. })) => return Ok(Pick::Play(cur)),
                    Some('P') => note = Some(self.strings().get("GalleryPlayWhat").to_string()),
                    Some('D') if items.get(cur).is_some_and(downloadable) => return Ok(Pick::Download(cur)),
                    Some('D') => note = Some(self.strings().get("GalleryNotFile").to_string()),
                    Some('S') => {
                        let i = SPEEDS.iter().position(|s| s == speed).map_or(0, |i| (i + 1) % SPEEDS.len());
                        *speed = SPEEDS[i];
                        let shown = if *speed == 0 { "instant".to_string() } else { format!("{speed}") };
                        note = Some(self.strings().fmt("GallerySpeed", &[&shown]));
                    }
                    _ => {}
                },
            }
            if cur != prev {
                if cur < top || cur >= top + height {
                    redraw = true;
                } else {
                    self.gallery_row(&items[prev], 3 + (prev - top) as u16, false);
                    self.gallery_row(&items[cur], 3 + (cur - top) as u16, true);
                }
            }
        }
    }

    fn gallery_row(&mut self, item: &Item, y: u16, hi: bool) {
        self.screen.goto(1, y);
        self.set_slot(if hi { Slot::ItemSel } else { Slot::Item });
        let line = match item {
            Item::Folder { name, pack, rec, .. } => {
                // A pack from a file area shows its description.
                let what = match rec {
                    Some((_, f)) if !f.description.is_empty() => crate::web::blurb(&f.description),
                    _ if *pack => "<pack>".to_string(),
                    _ => "<folder>".to_string(),
                };
                format!(" {} {}", resize(&format!("{name}/"), 24), resize(&what, 53))
            }
            Item::Art { name, size, sauce, .. } => {
                let (title, by) = match sauce {
                    Some(s) => {
                        let by = match (s.author.is_empty(), s.group.is_empty()) {
                            (false, false) => format!("{}/{}", s.author, s.group),
                            (false, true) => s.author.clone(),
                            (true, false) => s.group.clone(),
                            (true, true) => String::new(),
                        };
                        (s.title.clone(), by)
                    }
                    None => (String::new(), String::new()),
                };
                let kb = format!("{}k", size.div_ceil(1024));
                format!(" {} {} {} {}", resize(name, 24), resize(&title, 26), resize(&by, 20), resize(&kb, 5))
            }
        };
        self.write(&squish(&line, 79));
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.screen.clear_eol();
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
        self.page_grid_keys(&title, &grid, "GalleryViewHelp", &['P', 'D']).await
    }

    /// Send a piece as it is, paced to a modem speed; any key stops it.
    async fn gallery_play(&mut self, src: &Src, speed: u32) -> io::Result<()> {
        let data = match read_piece(src) {
            Ok(d) => playable(&d),
            Err(e) => return self.show_line(&format!("|U0{e}")).await,
        };
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        // Bytes per 40ms tick (a modem sends a byte in 10 bits).
        let chunk = if speed == 0 { data.len().max(1) } else { (speed as usize / 250).max(1) };
        let mut stopped = false;
        for piece in data.chunks(chunk) {
            self.screen.write_bytes(piece);
            self.flush().await?;
            if speed > 0 && self.key_waiting().await? {
                self.read_key().await?;
                stopped = true;
                break;
            }
        }
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        if !stopped {
            self.screen.goto(1, self.screen.rows());
            self.prompt_key().await?;
        }
        Ok(())
    }

    /// Download a piece (pulled out of its pack if need be) or a pack.
    async fn gallery_download(&mut self, item: &Item) -> io::Result<()> {
        let dl = self.state.config().gallery.download_acs.clone();
        self.screen.goto(1, self.screen.rows());
        self.screen.set_color(iniquity_term::Color::DEFAULT);
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
                std::fs::write(&out, read_piece(src)?)?;
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
    Open(usize),
    Play(usize),
    Download(usize),
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
        let top = list(&Place::Dir(dir.clone())).unwrap();
        assert_eq!(names(&top), ["1995/", "acdu0895.zip/"]);
        let Item::Folder { place, .. } = &top[1] else { panic!() };
        let pack = list(place).unwrap();
        assert_eq!(names(&pack), ["extra/", "FILE_ID.DIZ", "SO-PG1.ANS"], "programs are skipped");
        let Item::Folder { place, .. } = &top[0] else { panic!() };
        let Item::Art { sauce, .. } = &list(place).unwrap()[0] else { panic!() };
        assert_eq!(sauce.as_ref().unwrap().author, "phrac");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The help line sits on the bottom row: 80 columns there would scroll
    /// the screen on every key.
    #[test]
    fn bottom_lines_fit() {
        let s = crate::strings::Strings::load(Path::new("/nonexistent")).unwrap();
        let visible = |t: &str| {
            let t = iniquity_term::cp437::encode(t);
            iniquity_term::tokenize(&t, iniquity_term::Source::String)
                .iter()
                .map(|tok| if let iniquity_term::Token::Text(b) = tok { b.len() } else { 0 })
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
}
