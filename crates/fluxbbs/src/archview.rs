//! Files inside archives (new; the original only listed them): read
//! straight out of a ZIP, tar, tar.gz or .gz, within the upload
//! screening's unpack limit, for the gallery-style browser
//! (`gallery.rs`) to show.

use std::io::{self, Read, Write};
use std::path::Path;

use fluxbbs_term::Color;

use crate::session::Session;
use crate::upscan::Kind;

/// The bytes of `name` in the archive at `path`, at most `cap` of them
/// (counting, for tars, everything read on the way to it).
#[cfg(test)]
pub(crate) fn read_entry(path: &Path, kind: Kind, name: &str, cap: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    copy_entry(path, kind, name, cap, &mut out)?;
    Ok(out)
}

/// As [`read_entry`], written to `out` as it's read; the bytes copied.
pub(crate) fn copy_entry(path: &Path, kind: Kind, name: &str, cap: u64, out: &mut dyn Write) -> io::Result<u64> {
    let file = std::fs::File::open(path)?;
    match kind {
        Kind::Tar | Kind::TarGz => {
            let reader: Box<dyn Read> = if kind == Kind::Tar { Box::new(file) } else { Box::new(flate2::read::MultiGzDecoder::new(file)) };
            for e in tar::Archive::new(reader.take(cap)).entries()? {
                let mut e = e?;
                if e.path().is_ok_and(|p| p.to_string_lossy() == name) {
                    return io::copy(&mut e, out);
                }
            }
            Err(io::ErrorKind::NotFound.into())
        }
        Kind::Gzip => io::copy(&mut flate2::read::MultiGzDecoder::new(file).take(cap), out),
        _ => {
            let mut z = zip::ZipArchive::new(file).map_err(io::Error::other)?;
            let mut e = z.by_name(name).map_err(io::Error::other)?;
            io::copy(&mut (&mut e).take(cap), out)
        }
    }
}

impl Session {
    /// A message on the bottom line, until a key.
    pub(crate) async fn archive_note(&mut self, s: &str) -> io::Result<()> {
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
