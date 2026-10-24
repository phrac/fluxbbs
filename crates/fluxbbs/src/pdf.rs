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

/// Run one of Poppler's tools; its output, or why not.
fn poppler(tool: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>, String> {
    let out = Command::new(tool).args(args).output().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => format!("{tool} isn't installed"),
        _ => e.to_string(),
    })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("it failed").to_string());
    }
    Ok(out.stdout)
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
    let root = dir.join(format!("{}-p{n}", cache_key(path)));
    let png = root.with_extension("png");
    if !png.exists() {
        std::fs::create_dir_all(dir)?;
        let (n, px) = (n.to_string(), PAGE_PIXELS.to_string());
        poppler(
            "pdftoppm",
            &["-f".as_ref(), n.as_ref(), "-l".as_ref(), n.as_ref(), "-png".as_ref(), "-singlefile".as_ref(), "-scale-to".as_ref(), px.as_ref(), path.as_os_str(), root.as_os_str()],
        )
        .map_err(io::Error::other)?;
    }
    std::fs::read(&png)
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
            Ok(t) => Ok(t),
            Err(_) => {
                let p = path.to_path_buf();
                let got = tokio::task::spawn_blocking(move || text(&p)).await.map_err(io::Error::other)?;
                if let Ok(t) = &got {
                    let _ = std::fs::create_dir_all(cached.parent().unwrap_or(Path::new(".")));
                    let _ = std::fs::write(&cached, t);
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
    /// and T (the text) are passed back, anything else leaves.
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
        let mut n = 1;
        loop {
            let caption = self.strings().fmt("PdfPageBar", &[&fluxbbs_term::codes::squish(name, 24), &n.to_string(), &total.to_string()]);
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
                    _ => return Ok(None),
                },
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
