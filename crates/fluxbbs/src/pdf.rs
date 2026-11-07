//! New: PDFs in the file browser. Enter shows the text (`pdftotext`, in
//! reading order, a rule between pages), kept under the data directory's
//! `cache/pdf` so the next look is instant; P shows the pages as
//! pictures (`pdftoppm`), one at a time, left and right to turn them.
//! Needs Poppler's tools on the PATH (the Docker image has them); without
//! them a PDF can still be downloaded.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::input::Key;
use crate::session::Session;

/// Pictures of pages are this many pixels on their longer side, plenty
/// for a terminal's half-blocks.
const PAGE_PIXELS: u32 = 480;

/// The zoomed view's widest page and tallest window, in pixels.
const ZOOM_MAX: u32 = 2048;

pub fn is_pdf(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pdf")
}

/// A name for what's cached from `path`: changes when the file does.
fn cache_key(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(m) = std::fs::metadata(path) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// What the cache of PDFs' text, pages and copies out of archives may hold.
const CACHE_BYTES: u64 = 512 << 20;

/// New: oldest files out of the cache `dir` until it's within CACHE_BYTES,
/// keeping `keep` (just made).
pub(crate) fn prune_cache(dir: &Path, keep: &Path) {
    prune_to(dir, CACHE_BYTES, keep)
}

/// New: a PDF copied out of an archive is kept this long after its last
/// use however full the cache is (a caller may be reading it), and a
/// copy still being made (`.part…`) this long.
const PDF_IN_USE: std::time::Duration = std::time::Duration::from_secs(3600);
const PART_IN_USE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// New: mark a cache file used now (its access time: the modified time
/// is part of a copy's [`cache_key`]), so pruning takes it last.
pub(crate) fn touch(path: &Path) {
    if let Ok(f) = std::fs::File::options().write(true).open(path) {
        let _ = f.set_times(std::fs::FileTimes::new().set_accessed(std::time::SystemTime::now()));
    }
}

/// When a cache file was last made or used.
fn last_used(m: &std::fs::Metadata) -> std::time::SystemTime {
    let made = m.modified().unwrap_or(std::time::UNIX_EPOCH);
    m.accessed().map_or(made, |a| a.max(made))
}

fn prune_to(dir: &Path, max: u64, keep: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let now = std::time::SystemTime::now();
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (last_used(&m), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (used, len, path) in files {
        if total <= max {
            break;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let age = now.duration_since(used).unwrap_or_default();
        let in_use = (ext.starts_with("part") && age < PART_IN_USE) || (ext == "pdf" && age < PDF_IN_USE);
        if path != keep && !in_use && std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// How long one of Poppler's tools may run, and how much it may print.
const TOOL_SECS: u64 = 60;
const TOOL_BYTES: u64 = 64 << 20;

/// Run one of Poppler's tools; its output, or why not.
fn poppler(tool: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>, String> {
    // New: killed past TOOL_SECS or TOOL_BYTES (a crafted PDF can keep
    // one busy for ever).
    let mut cmd = Command::new(tool);
    cmd.args(args);
    let out = crate::upscan::run_limited(cmd, std::time::Duration::from_secs(TOOL_SECS), TOOL_BYTES).map_err(|e| match e {
        crate::upscan::RunError::Spawn(e) if e.kind() == io::ErrorKind::NotFound => format!("{tool} isn't installed"),
        e => e.to_string(),
    })?;
    if !out.0.success() {
        let err = String::from_utf8_lossy(&out.2);
        return Err(err.lines().next().unwrap_or("it failed").to_string());
    }
    Ok(out.1)
}

/// The text of a PDF, in reading order, with a rule where each page
/// starts.
pub fn text(path: &Path) -> Result<String, String> {
    let raw = poppler("pdftotext", &["-enc".as_ref(), "UTF-8".as_ref(), "-q".as_ref(), path.as_os_str(), "-".as_ref()])?;
    Ok(tidy(&String::from_utf8_lossy(&raw)))
}

/// Pages split at form feeds and ruled, control characters gone, and no
/// more than one blank line in a row.
fn tidy(raw: &str) -> String {
    let mut out = String::new();
    for (i, page) in raw.split('\x0c').enumerate() {
        if page.trim().is_empty() {
            continue;
        }
        let head = format!("── page {} ", i + 1);
        out.push_str(&format!("{head}{}\n\n", "─".repeat(78 - head.chars().count())));
        let mut blank = 0;
        for line in page.lines() {
            let line: String = line.chars().filter(|c| !c.is_control()).collect();
            let line = line.trim_end();
            if line.is_empty() {
                blank += 1;
                if blank > 1 {
                    continue;
                }
            } else {
                blank = 0;
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// How many pages.
pub fn pages(path: &Path) -> Result<u32, String> {
    let info = String::from_utf8_lossy(&poppler("pdfinfo", &[path.as_os_str()])?).into_owned();
    info.lines().find_map(|l| l.strip_prefix("Pages:")).and_then(|n| n.trim().parse().ok()).ok_or_else(|| "no page count".into())
}

/// Page `n` as a PNG, made into `dir` the first time.
pub fn page_png(path: &Path, n: u32, dir: &Path) -> io::Result<Vec<u8>> {
    render(path, n, dir, &format!("{}-p{n}", cache_key(path)), &["-scale-to".into(), PAGE_PIXELS.to_string()])
}

/// New: page `n` `width` pixels wide (its height as it comes), for the
/// zoomed view.
pub fn page_png_wide(path: &Path, n: u32, dir: &Path, width: u32) -> io::Result<Vec<u8>> {
    render(path, n, dir, &format!("{}-p{n}-w{width}", cache_key(path)), &["-scale-to-x".into(), width.to_string(), "-scale-to-y".into(), "-1".into()])
}

fn render(path: &Path, n: u32, dir: &Path, stem: &str, size: &[String]) -> io::Result<Vec<u8>> {
    let root = dir.join(stem);
    let png = root.with_extension("png");
    if path.starts_with(dir) {
        touch(path);
    }
    if png.exists() {
        touch(&png);
    } else {
        std::fs::create_dir_all(dir)?;
        let n = n.to_string();
        let mut args: Vec<&std::ffi::OsStr> = vec!["-f".as_ref(), n.as_ref(), "-l".as_ref(), n.as_ref(), "-png".as_ref(), "-singlefile".as_ref()];
        args.extend(size.iter().map(std::ffi::OsStr::new));
        args.extend([path.as_os_str(), root.as_os_str()]);
        poppler("pdftoppm", &args).map_err(io::Error::other)?;
        prune_cache(dir, &png);
    }
    std::fs::read(&png)
}

/// New: rows `top..top + h` of a page's PNG as sixels (fewer if the page
/// ends first), and the page's height. 64 colors: a scan's paper grain
/// in 256 made a screenful 390 KB; in 64, 230 KB, the text as clear.
pub fn page_slice(png: &[u8], top: u32, h: u32) -> Result<(Vec<u8>, u32), String> {
    let img = image::load_from_memory(png).map_err(|e| e.to_string())?.to_rgb8();
    let (w, ph) = img.dimensions();
    let top = top.min(ph.saturating_sub(1));
    let rows = h.min(ph - top);
    let px: Vec<[u8; 3]> = (top..top + rows).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| img.get_pixel(x, y).0).collect();
    Ok((crate::picture::sixel(&px, w as usize, rows as usize, 64), ph))
}

impl Session {
    /// Where PDFs' text and pages are kept.
    fn pdf_cache(&self) -> PathBuf {
        let cfg = self.state.config();
        cfg.resolve(&cfg.paths.data).join("cache").join("pdf")
    }

    /// A PDF's text in the viewer. P (pages) and D (download) are passed
    /// back.
    pub(crate) async fn pdf_text(&mut self, name: &str, path: &Path) -> io::Result<Option<char>> {
        let cached = self.pdf_cache().join(format!("{}.txt", cache_key(path)));
        let text = match std::fs::read_to_string(&cached) {
            Ok(t) => {
                touch(&cached);
                Ok(t)
            }
            Err(_) => {
                let p = path.to_path_buf();
                let got = tokio::task::spawn_blocking(move || text(&p)).await.map_err(io::Error::other)?;
                if let Ok(t) = &got {
                    let _ = std::fs::create_dir_all(cached.parent().unwrap_or(Path::new(".")));
                    let _ = std::fs::write(&cached, t);
                    prune_cache(cached.parent().unwrap_or(Path::new(".")), &cached);
                }
                got
            }
        };
        match text {
            Ok(t) if !t.trim().is_empty() => {
                let grid = crate::pager::trimmed(crate::pager::render(t.as_bytes(), true));
                self.page_grid_keys(name, &grid, "PdfViewHelp", &['P', 'D']).await
            }
            // A scan with no text in it: straight to the pages.
            Ok(_) => Ok(Some('P')),
            Err(why) => {
                let s = self.strings().fmt("PdfCantRead", &[name, &why]);
                self.archive_note(&s).await.map(|_| None)
            }
        }
    }

    /// A PDF's pages as pictures, from the first: right, space, enter or
    /// pgdn the next, left or pgup the one before, home and end; D (download)
    /// and T (the text) are passed back, anything else leaves. New: in
    /// sixels, Z zooms to the screen's width ([`Session::pdf_zoomed`]).
    pub(crate) async fn pdf_pages(&mut self, name: &str, path: &Path) -> io::Result<Option<char>> {
        let p = path.to_path_buf();
        let total = match tokio::task::spawn_blocking(move || pages(&p)).await.map_err(io::Error::other)? {
            Ok(n) if n > 0 => n,
            Ok(_) => return Ok(None),
            Err(why) => {
                let s = self.strings().fmt("PdfCantRead", &[name, &why]);
                return self.archive_note(&s).await.map(|_| None);
            }
        };
        let dir = self.pdf_cache();
        let sixel = self.user.sixel && !self.plain();
        let (mut n, mut zoom) = (1, false);
        loop {
            if zoom {
                match self.pdf_zoomed(name, path, &mut n, total).await? {
                    Some('Z') => zoom = false,
                    other => return Ok(other),
                }
                continue;
            }
            let caption = if sixel {
                self.strings().fmt("PdfPageBarZoom", &[&fluxbbs_term::codes::squish(name, 20), &n.to_string(), &total.to_string()])
            } else {
                self.strings().fmt("PdfPageBar", &[&fluxbbs_term::codes::squish(name, 24), &n.to_string(), &total.to_string()])
            };
            let (p, d) = (path.to_path_buf(), dir.clone());
            let read = move || page_png(&p, n, &d);
            let Some(key) = self.show_picture_key(name, Some(caption), path.to_path_buf(), format!("page {n}"), read).await? else { return Ok(None) };
            n = match key {
                Key::Right | Key::Down | Key::PgDn | Key::Enter | Key::Char(b' ') => (n + 1).min(total),
                Key::Left | Key::Up | Key::PgUp => n.saturating_sub(1).max(1),
                Key::Home => 1,
                Key::End => total,
                k => match k.upper() {
                    Some(c @ ('D' | 'T')) => return Ok(Some(c)),
                    Some('N') => (n + 1).min(total),
                    Some('B') => n.saturating_sub(1).max(1),
                    Some('Z') if sixel => {
                        zoom = true;
                        n
                    }
                    _ => return Ok(None),
                },
            };
        }
    }

    /// New: pages at the screen's full width in sixels, a screenful at a
    /// time: up and down scroll half a screen, pgup, pgdn, space and enter
    /// a whole one (on past a page's end to the next), left and right
    /// turn pages, home and end. Z (back to whole pages), D and T are
    /// passed back, anything else leaves.
    async fn pdf_zoomed(&mut self, name: &str, path: &Path, n: &mut u32, total: u32) -> io::Result<Option<char>> {
        let (cw, ch) = self.cell_pixels().await?;
        let rows = self.screen.rows().saturating_sub(1).max(2) as u32;
        // The cell size is the terminal's word: capped, so a claimed giant
        // cell can't make a page tens of thousands of pixels big.
        let (width, view) = ((80 * cw).min(ZOOM_MAX), (rows * ch).min(ZOOM_MAX) / 6 * 6);
        let (half, full) = (view / 12 * 6, view.saturating_sub(4 * ch) / 6 * 6);
        let dir = self.pdf_cache();
        let (mut top, mut fresh) = (0u32, true);
        loop {
            let (p, d, page) = (path.to_path_buf(), dir.clone(), *n);
            let got = tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, u32), String> {
                let png = page_png_wide(&p, page, &d, width).map_err(|e| e.to_string())?;
                page_slice(&png, top, view)
            })
            .await
            .map_err(io::Error::other)?;
            let (six, height) = match got {
                Ok(g) => g,
                Err(why) => {
                    let s = self.strings().fmt("PdfCantRead", &[name, &why]);
                    return self.archive_note(&s).await.map(|_| None);
                }
            };
            let last = height.saturating_sub(view);
            if fresh {
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.clear_screen();
                fresh = false;
            }
            self.screen.goto(1, 1);
            self.screen.write_raw(&six);
            let shown = ((top + view).min(height) * 100 / height.max(1)).to_string() + "%";
            let bar = self.strings().fmt("PdfZoomBar", &[&fluxbbs_term::codes::squish(name, 20), &n.to_string(), &total.to_string(), &shown]);
            self.screen.goto(1, self.screen.rows());
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.screen.clear_eol();
            self.show(&bar).await?;
            self.flush().await?;
            let key = self.read_key().await?;
            let mut to = *n;
            match key {
                Key::Down => top = (top + half).min(last),
                Key::Up => top = top.saturating_sub(half),
                Key::PgDn | Key::Enter | Key::Char(b' ') if top >= last => to = (*n + 1).min(total),
                Key::PgDn | Key::Enter | Key::Char(b' ') => top = (top + full).min(last),
                Key::PgUp => top = top.saturating_sub(full),
                Key::Right => to = (*n + 1).min(total),
                Key::Left => to = n.saturating_sub(1).max(1),
                Key::Home => to = 1,
                Key::End => to = total,
                k => match k.upper() {
                    Some(c @ ('D' | 'T' | 'Z')) => return Ok(Some(c)),
                    Some('N') => to = (*n + 1).min(total),
                    Some('B') => to = n.saturating_sub(1).max(1),
                    _ => return Ok(None),
                },
            }
            if to != *n {
                *n = to;
                top = 0;
                fresh = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_prunes_oldest() {
        let dir = std::env::temp_dir().join(format!("fluxbbs-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let names = ["a", "b", "c", "d.pdf", "e.part9-1-0", "f.pdf"];
        for (i, n) in names.iter().enumerate() {
            let p = dir.join(n);
            std::fs::write(&p, [0u8; 100]).unwrap();
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i as u64);
            let t = if n.starts_with('d') || n.starts_with('e') { std::time::SystemTime::now() } else { t };
            let times = std::fs::FileTimes::new().set_modified(t).set_accessed(t);
            std::fs::File::options().write(true).open(&p).unwrap().set_times(times).unwrap();
        }
        prune_to(&dir, 150, &dir.join("a"));
        let left: Vec<bool> = names.iter().map(|n| dir.join(n).exists()).collect();
        assert_eq!(left, [true, false, false, true, true, false], "oldest go first; the one kept, a PDF in use and a copy being made stay");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_hit_goes_last() {
        let dir = std::env::temp_dir().join(format!("fluxbbs-touch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, n) in ["x.png", "y.png"].iter().enumerate() {
            let p = dir.join(n);
            std::fs::write(&p, [0u8; 100]).unwrap();
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i as u64);
            std::fs::File::options().write(true).open(&p).unwrap().set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t)).unwrap();
        }
        touch(&dir.join("x.png"));
        let m = std::fs::metadata(dir.join("x.png")).unwrap();
        assert!(m.modified().unwrap() < std::time::UNIX_EPOCH + std::time::Duration::from_secs(2_000_000), "a copy's key stays");
        prune_to(&dir, 100, &dir.join("none"));
        assert!(dir.join("x.png").exists() && !dir.join("y.png").exists(), "the one used just now stays");
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn slices_pages() {
        let img = image::RgbImage::from_fn(64, 100, |_, y| image::Rgb(if y < 50 { [255, 255, 255] } else { [0, 0, 0] }));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (six, h) = page_slice(&png, 40, 30).unwrap();
        assert_eq!(h, 100);
        assert!(six.starts_with(b"\x1bP9;1q\"1;1;64;30"));
        let (end, _) = page_slice(&png, 90, 30).unwrap();
        assert!(end.starts_with(b"\x1bP9;1q\"1;1;64;10"), "cut at the page's end");
    }

    #[test]
    fn tidies_pages() {
        let t = tidy("one\n\n\n\ntwo\x0cthree\n\x0c   \n");
        assert!(t.starts_with("── page 1 "));
        assert!(t.contains("one\n\ntwo\n"), "{t}");
        assert!(t.contains("── page 2 "));
        assert!(!t.contains("page 3"));
        assert!(t.lines().all(|l| l.chars().count() <= 78));
        assert!(is_pdf("BW-1994.PDF") && !is_pdf("a.pdf.zip"));
    }
}
