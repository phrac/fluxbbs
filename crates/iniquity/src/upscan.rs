//! Upload screening, new: which types may be uploaded, whether a file's
//! contents match its name, and a look inside (counts, sizes, dates) for
//! the summary line added to its description. Archives are read, never
//! unpacked to disk, and the reading stops at a size limit so a zip bomb
//! can't run away with it.

use std::io::{self, Read};
use std::path::Path;

use chrono::{DateTime, NaiveDate};

/// What the name says a file is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Zip,
    TarGz,
    Gzip,
    Tar,
    Text,
    Other,
}

pub fn kind_of(name: &str) -> Kind {
    let n = name.to_ascii_lowercase();
    let ends = |s: &[&str]| s.iter().any(|e| n.ends_with(e));
    if ends(&[".zip"]) {
        Kind::Zip
    } else if ends(&[".tar.gz", ".tgz"]) {
        Kind::TarGz
    } else if ends(&[".gz"]) {
        Kind::Gzip
    } else if ends(&[".tar"]) {
        Kind::Tar
    } else if ends(&[".txt", ".nfo", ".diz", ".asc", ".ans", ".md"]) {
        Kind::Text
    } else {
        Kind::Other
    }
}

/// Is `name` one of the `allowed` types (suffixes like `zip`, `tar.gz`)?
/// An empty list allows anything.
pub fn type_allowed(allowed: &[String], name: &str) -> bool {
    let types: Vec<String> = allowed.iter().map(|t| t.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
    let n = name.to_ascii_lowercase();
    types.is_empty() || types.iter().any(|t| n.ends_with(&format!(".{t}")))
}

/// Text: no NULs, and hardly any control characters besides the usual
/// ones (tabs, line ends, form feed, EOF, ANSI escapes). Any byte above
/// 127 is fine, CP437 or UTF-8.
fn looks_like_text(b: &[u8]) -> bool {
    let odd = b.iter().filter(|&&c| c < 0x20 && !matches!(c, b'\t' | b'\n' | b'\r' | 0x0c | 0x1a | 0x1b | 0x07 | 0x08)).count();
    !b.contains(&0) && odd * 100 <= b.len()
}

/// Do the contents match the name? `Err` says why not.
pub fn check_contents(path: &Path, kind: Kind) -> io::Result<Result<(), &'static str>> {
    let mut head = vec![0u8; if kind == Kind::Text { 1 << 20 } else { 512 }];
    let mut f = std::fs::File::open(path)?;
    let mut n = 0;
    while n < head.len() {
        let got = f.read(&mut head[n..])?;
        if got == 0 {
            break;
        }
        n += got;
    }
    let head = &head[..n];
    Ok(match kind {
        Kind::Zip if !(head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06")) => Err("ZIP"),
        Kind::TarGz | Kind::Gzip if !head.starts_with(&[0x1f, 0x8b]) => Err("gzip"),
        Kind::Tar if head.get(257..262) != Some(b"ustar") => Err("tar"),
        Kind::Text if !looks_like_text(head) => Err("text"),
        _ => Ok(()),
    })
}

/// What's inside.
#[derive(Debug, Default, PartialEq)]
pub struct Scan {
    pub files: u64,
    pub unpacked: u64,
    pub newest: Option<NaiveDate>,
    pub encrypted: bool,
    /// For text files.
    pub lines: Option<u64>,
    /// The first few file names, if asked for.
    pub names: Vec<String>,
}

pub struct Limits {
    /// Unpacked size may be at most this many times the file's (0 = any),
    /// once it's over a megabyte...
    pub max_ratio: u64,
    /// ...and at most this many bytes (0 = any).
    pub max_unpacked: u64,
    pub refuse_unsafe: bool,
    pub keep_names: usize,
}

/// Why an upload was refused.
#[derive(Debug, PartialEq)]
pub enum Refused {
    Bomb,
    Unsafe(String),
    Damaged(String),
}

/// A name that would land outside the folder it's unpacked in.
fn unsafe_path(name: &str) -> bool {
    let n = name.replace('\\', "/");
    n.starts_with('/') || n.split('/').any(|c| c == "..") || n.as_bytes().get(1) == Some(&b':')
}

impl Limits {
    pub fn from_config(f: &crate::config::Files) -> Self {
        Limits {
            max_ratio: f.max_unpacked_ratio as u64,
            max_unpacked: f.max_unpacked_bytes,
            refuse_unsafe: f.refuse_unsafe_paths,
            keep_names: f.scan_names as usize,
        }
    }

    /// The most an archive of `packed` bytes may unpack to.
    pub fn cap(&self, packed: u64) -> u64 {
        let by_ratio = if self.max_ratio > 0 { (packed.saturating_mul(self.max_ratio)).max(1 << 20) } else { u64::MAX };
        let cap = if self.max_unpacked > 0 { self.max_unpacked } else { u64::MAX };
        by_ratio.min(cap)
    }
}

/// Reads at most `left` bytes, then fails, so a bomb stops early.
struct Capped<R> {
    inner: R,
    left: u64,
    hit: std::rc::Rc<std::cell::Cell<bool>>,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n as u64 > self.left {
            self.hit.set(true);
            return Err(io::Error::other("too large"));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

struct Seen<'a> {
    scan: Scan,
    limits: &'a Limits,
}

impl Seen<'_> {
    fn file(&mut self, name: &str, size: u64, date: Option<NaiveDate>) -> Result<(), Refused> {
        if unsafe_path(name) && self.limits.refuse_unsafe {
            return Err(Refused::Unsafe(name.to_string()));
        }
        self.scan.files += 1;
        self.scan.unpacked += size;
        self.scan.newest = self.scan.newest.max(date);
        if self.scan.names.len() < self.limits.keep_names {
            self.scan.names.push(name.to_string());
        }
        Ok(())
    }
}

/// Look inside an upload. `None`: nothing to say about this kind.
pub fn scan(path: &Path, kind: Kind, limits: &Limits) -> io::Result<Result<Option<Scan>, Refused>> {
    let packed = std::fs::metadata(path)?.len();
    let cap = limits.cap(packed);
    let mut seen = Seen { scan: Scan::default(), limits };
    let damaged = |e: &dyn std::fmt::Display| Refused::Damaged(e.to_string());
    let result = match kind {
        Kind::Zip => (|| {
            let mut z = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| damaged(&e))?).map_err(|e| damaged(&e))?;
            for i in 0..z.len() {
                let f = z.by_index_raw(i).map_err(|e| damaged(&e))?;
                let name = f.name().map(|n| n.into_owned()).unwrap_or_default();
                if f.is_dir() {
                    if unsafe_path(&name) && limits.refuse_unsafe {
                        return Err(Refused::Unsafe(name));
                    }
                    continue;
                }
                seen.scan.encrypted |= f.encrypted();
                let date = f.last_modified().and_then(|d| NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32));
                seen.file(&name, f.size(), date)?;
            }
            // The sizes are the archive's own claims; the integrity test
            // that follows unpacks it (to nowhere) under the same cap.
            if seen.scan.unpacked > cap {
                return Err(Refused::Bomb);
            }
            Ok(Some(()))
        })(),
        Kind::TarGz | Kind::Tar | Kind::Gzip => {
            let hit = std::rc::Rc::new(std::cell::Cell::new(false));
            let file = std::fs::File::open(path)?;
            let reader: Box<dyn Read> = match kind {
                Kind::Tar => Box::new(file),
                _ => Box::new(flate2::read::MultiGzDecoder::new(file)),
            };
            let mut capped = Capped { inner: reader, left: cap, hit: hit.clone() };
            let r = if kind == Kind::Gzip {
                let name = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".gz").to_string()).unwrap_or_default();
                io::copy(&mut capped, &mut io::sink()).map(|n| seen.file(&name, n, None)).map_err(|e| damaged(&e)).and_then(|r| r)
            } else {
                (|| {
                    let mut ar = tar::Archive::new(&mut capped);
                    for e in ar.entries().map_err(|e| damaged(&e))? {
                        let e = e.map_err(|e| damaged(&e))?;
                        let name = e.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                        let h = e.header();
                        if !h.entry_type().is_file() {
                            if unsafe_path(&name) && limits.refuse_unsafe {
                                return Err(Refused::Unsafe(name));
                            }
                            continue;
                        }
                        let date = h.mtime().ok().and_then(|t| DateTime::from_timestamp(t as i64, 0)).map(|d| d.date_naive());
                        seen.file(&name, h.size().unwrap_or(0), date)?;
                    }
                    Ok(())
                })()
            };
            if hit.get() { Err(Refused::Bomb) } else { r.map(Some) }
        }
        Kind::Text => {
            let data = std::fs::read(path)?;
            let mut lines = data.iter().filter(|&&c| c == b'\n').count() as u64;
            if data.last().is_some_and(|&c| c != b'\n') {
                lines += 1;
            }
            seen.scan.lines = Some(lines);
            Ok(Some(()))
        }
        Kind::Other => {
            // libarchive reads most other archive formats (RAR, 7Z, LHA,
            // tar.bz2 and .xz); without it there's just no summary.
            match std::process::Command::new("bsdtar").arg("-tf").arg(path).stderr(std::process::Stdio::null()).output() {
                Ok(out) if out.status.success() => (|| {
                    for name in String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()) {
                        if name.ends_with('/') {
                            continue;
                        }
                        seen.file(name, 0, None)?;
                    }
                    Ok(Some(()))
                })(),
                _ => Ok(None),
            }
        }
    };
    Ok(result.map(|got| got.map(|_| seen.scan)))
}

/// "1.2 MB" and so on.
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} bytes");
    }
    let mut v = n as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v < 10.0 { format!("{v:.1} {}", UNITS[u]) } else { format!("{v:.0} {}", UNITS[u]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn limits() -> Limits {
        Limits { max_ratio: 100, max_unpacked: 0, refuse_unsafe: true, keep_names: 2 }
    }

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("iqscan{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn types_and_contents() {
        let allowed: Vec<String> = ["zip", "tar.gz", ".tgz", "txt"].map(String::from).to_vec();
        assert!(type_allowed(&allowed, "PHRACK70.TAR.GZ"));
        assert!(type_allowed(&allowed, "issue.tgz"));
        assert!(!type_allowed(&allowed, "game.exe"));
        assert!(!type_allowed(&allowed, "zip"));
        assert!(type_allowed(&[], "game.exe"));
        assert_eq!(kind_of("a.TAR.GZ"), Kind::TarGz);

        let d = dir();
        let fake = d.join("cool.zip");
        std::fs::write(&fake, b"MZ\x90\x00 an exe").unwrap();
        assert_eq!(check_contents(&fake, Kind::Zip).unwrap(), Err("ZIP"));
        let txt = d.join("read.txt");
        std::fs::write(&txt, "hello\r\n\x1b[1mthere\x1b[0m\r\n\u{00e9}\n").unwrap();
        assert_eq!(check_contents(&txt, Kind::Text).unwrap(), Ok(()));
        assert_eq!(scan(&txt, Kind::Text, &limits()).unwrap().unwrap().unwrap().lines, Some(3));
        std::fs::write(&txt, b"\x7fELF\x02\x01\x01\x00\x00\x00").unwrap();
        assert_eq!(check_contents(&txt, Kind::Text).unwrap(), Err("text"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn zips_are_counted_and_bombs_refused() {
        let d = dir();
        let path = d.join("z.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.add_directory("p70/", opts).unwrap();
            for (n, body) in [("p70/one.txt", "first"), ("p70/two.txt", "second file"), ("p70/three.txt", "3")] {
                w.start_file(n, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let s = scan(&path, Kind::Zip, &limits()).unwrap().unwrap().unwrap();
        assert_eq!((s.files, s.unpacked, s.names.len()), (3, 17, 2));

        // Four megabytes of zeros squeeze to a few kilobytes.
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            w.start_file("zeros", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&vec![0u8; 4 << 20]).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(scan(&path, Kind::Zip, &limits()).unwrap(), Err(Refused::Bomb));

        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            w.start_file("../../etc/evil", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        assert!(matches!(scan(&path, Kind::Zip, &limits()).unwrap(), Err(Refused::Unsafe(_))));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tar_gz_is_read_through_and_capped() {
        let d = dir();
        let path = d.join("t.tar.gz");
        let make = |files: &[(&str, Vec<u8>)]| {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&path).unwrap(), flate2::Compression::default());
            let mut t = tar::Builder::new(gz);
            for (name, body) in files {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_mtime(1_760_000_000);
                h.set_cksum();
                t.append_data(&mut h, name, body.as_slice()).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap();
        };
        make(&[("zine/a.txt", b"aaaa".to_vec()), ("zine/b.txt", b"bb".to_vec())]);
        assert_eq!(check_contents(&path, Kind::TarGz).unwrap(), Ok(()));
        let s = scan(&path, Kind::TarGz, &limits()).unwrap().unwrap().unwrap();
        assert_eq!((s.files, s.unpacked), (2, 6));
        assert_eq!(s.newest, NaiveDate::from_ymd_opt(2025, 10, 9));

        make(&[("big", vec![0u8; 8 << 20])]);
        assert_eq!(scan(&path, Kind::TarGz, &limits()).unwrap(), Err(Refused::Bomb));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(900), "900 bytes");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1_288_490_189), "1.2 GB");
        assert_eq!(human_size(52 << 20), "52 MB");
    }
}
