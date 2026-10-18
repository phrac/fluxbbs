//! Inside archives (new; the original only listed them): a ZIP, tar,
//! tar.gz or .gz from the file lister or `FV` opens as a list of its
//! files, and Enter shows one: text and ANSI in the scrolling viewer,
//! pictures as ANSI. Files are read straight out of the archive, within
//! the upload screening's unpack limit, and never written to disk.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use iniquity_term::codes::{resize, squish};
use iniquity_term::{Color, Slot};

use crate::files::commas;
use crate::input::Key;
use crate::session::Session;
use crate::upscan::Kind;

/// One file in an archive.
pub(crate) struct Entry {
    pub name: String,
    pub size: u64,
    pub packed: Option<u64>,
    pub date: String,
}

/// The bytes of `name` in the archive at `path`, at most `cap` of them
/// (counting, for tars, everything read on the way to it).
fn read_entry(path: &Path, kind: Kind, name: &str, cap: u64) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut out = Vec::new();
    match kind {
        Kind::Tar | Kind::TarGz => {
            let reader: Box<dyn Read> = if kind == Kind::Tar { Box::new(file) } else { Box::new(flate2::read::MultiGzDecoder::new(file)) };
            for e in tar::Archive::new(reader.take(cap)).entries()? {
                let mut e = e?;
                if e.path().is_ok_and(|p| p.to_string_lossy() == name) {
                    e.read_to_end(&mut out)?;
                    return Ok(out);
                }
            }
            Err(io::ErrorKind::NotFound.into())
        }
        Kind::Gzip => {
            flate2::read::MultiGzDecoder::new(file).take(cap).read_to_end(&mut out)?;
            Ok(out)
        }
        _ => {
            let mut z = zip::ZipArchive::new(file).map_err(io::Error::other)?;
            z.by_name(name).map_err(io::Error::other)?.take(cap).read_to_end(&mut out)?;
            Ok(out)
        }
    }
}

impl Session {
    /// The archive at `path` (called `title`) as a list to pick from.
    pub(crate) async fn archive_browse(&mut self, title: &str, path: &Path, kind: Kind, entries: Vec<Entry>) -> io::Result<()> {
        if entries.is_empty() {
            return self.show_line(&format!("|U6{title}|U5: |U4It's empty|U5.")).await;
        }
        let packed_known = entries.iter().all(|e| e.packed.is_some());
        let total: u64 = entries.iter().map(|e| e.size).sum();
        let (mut cur, mut top) = (0usize, 0usize);
        loop {
            let rows = self.screen.rows() as usize;
            let height = rows.saturating_sub(5).max(1);
            if cur < top {
                top = cur;
            } else if cur >= top + height {
                top = cur + 1 - height;
            }
            self.screen.set_color(Color::DEFAULT);
            self.clear_screen();
            self.show_line(&format!("|U4 Archive|U5: |U6{title}  |U5(|U6{}|U4 files, |U6{}|U4 bytes|U5)", entries.len(), commas(total))).await?;
            self.show_line(if packed_known { "|U4 Size        Packed      Date      Name" } else { "|U4 Size        Date      Name" }).await?;
            self.rule().await?;
            for (i, e) in entries.iter().enumerate().skip(top).take(height) {
                let comp = e.packed.map(|c| format!("{} ", resize(&commas(c), 11))).unwrap_or_default();
                let line = format!(" {} {comp}{} {}", resize(&commas(e.size), 11), resize(&e.date, 9), e.name);
                self.set_slot(if i == cur { Slot::ItemSel } else { Slot::Text });
                self.write(&resize(&squish(&line, 79), 79));
                self.screen.set_color(Color::DEFAULT);
                self.newline();
            }
            self.screen.goto(1, rows as u16);
            let help = self.strings().get("ArchiveHelp").to_string();
            self.show(&help).await?;
            let last = entries.len() - 1;
            match self.read_key().await? {
                Key::Up => cur = cur.saturating_sub(1),
                Key::Down => cur = (cur + 1).min(last),
                Key::PgUp => cur = cur.saturating_sub(height),
                Key::PgDn => cur = (cur + height).min(last),
                Key::Home => cur = 0,
                Key::End => cur = last,
                Key::Enter => self.archive_open(path, kind, &entries[cur]).await?,
                Key::Esc => break,
                k if k.upper() == Some('Q') => break,
                _ => {}
            }
        }
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        Ok(())
    }

    /// Show one file from an archive.
    async fn archive_open(&mut self, path: &Path, kind: Kind, e: &Entry) -> io::Result<()> {
        let cap = crate::upscan::Limits::from_config(&self.state.config().files).cap(std::fs::metadata(path)?.len());
        let name = e.name.rsplit('/').next().unwrap_or(&e.name).to_string();
        let (file, inner): (PathBuf, String) = (path.to_path_buf(), e.name.clone());
        if crate::picture::is_image(&name) {
            let read = move || read_entry(&file, kind, &inner, cap);
            self.show_picture(&name, path.to_path_buf(), e.name.clone(), read).await?;
            return Ok(());
        }
        // Text is all the viewer shows; 8MB of it is plenty.
        let read = tokio::task::spawn_blocking(move || read_entry(&file, kind, &inner, cap.min(8 << 20))).await.map_err(io::Error::other)?;
        let data = match read {
            Ok(d) => d,
            Err(err) => return self.archive_note(&format!("|U6{name}|U5: |U4Couldn't read it ({err})|U5.")).await,
        };
        let text = crate::upscan::text_part(&data);
        if !crate::upscan::looks_like_text(text) {
            let s = self.strings().fmt("ArchiveCantShow", &[&name]);
            return self.archive_note(&s).await;
        }
        let utf8 = std::str::from_utf8(text).is_ok_and(|t| !t.is_ascii());
        let grid = crate::pager::render(text, utf8);
        self.page_grid(&e.name, &grid).await
    }

    /// A message on the bottom line, until a key.
    async fn archive_note(&mut self, s: &str) -> io::Result<()> {
        self.screen.set_color(Color::DEFAULT);
        self.screen.goto(1, self.screen.rows());
        self.screen.clear_eol();
        self.show(s).await?;
        self.read_key().await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_come_out_of_archives() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("iqarchview{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tgz = dir.join("p.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&tgz).unwrap(), flate2::Compression::default());
            let mut t = tar::Builder::new(gz);
            for (n, body) in [("1.txt", &b"one"[..]), ("2.txt", b"two")] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_cksum();
                t.append_data(&mut h, n, body).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap();
        }
        assert_eq!(read_entry(&tgz, Kind::TarGz, "2.txt", 1 << 20).unwrap(), b"two");
        assert!(read_entry(&tgz, Kind::TarGz, "3.txt", 1 << 20).is_err());
        let zip = dir.join("p.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
            w.start_file("a/readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"hello there").unwrap();
            w.finish().unwrap();
        }
        assert_eq!(read_entry(&zip, Kind::Zip, "a/readme.txt", 5).unwrap(), b"hello", "capped");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
