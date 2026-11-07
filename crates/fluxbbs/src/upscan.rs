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

/// The text part of a file: up to the DOS end-of-file mark (^Z), after
/// which ANSI and ASCII art keep a SAUCE record, which isn't text.
pub fn text_part(b: &[u8]) -> &[u8] {
    b.split(|&c| c == 0x1a).next().unwrap_or_default()
}

/// Text: hardly any control characters besides the usual ones (tabs,
/// line ends, form feed, ANSI escapes); at most 1% of it. NULs count
/// against that rather than ruling a file out, since ANSI editors save
/// some blank cells as NUL (a blank in CP437), while programs and other
/// binaries are full of them. Any byte above 127 is fine, CP437 or UTF-8.
pub(crate) fn looks_like_text(b: &[u8]) -> bool {
    let b = text_part(b);
    let odd = b.iter().filter(|&&c| c < 0x20 && !matches!(c, b'\t' | b'\n' | b'\r' | 0x0c | 0x1a | 0x1b | 0x07 | 0x08)).count();
    !b.is_empty() && odd * 100 <= b.len()
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
            let data = text_part(&data);
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
            // New: within LIST_SECS and LIST_BYTES of listing, or killed:
            // an .xz bomb would otherwise keep a core busy past the call.
            let mut cmd = std::process::Command::new("bsdtar");
            cmd.arg("-tf").arg(path);
            match run_bounded(cmd, std::time::Duration::from_secs(LIST_SECS), LIST_BYTES) {
                Some(out) => (|| {
                    for name in String::from_utf8_lossy(&out).lines().filter(|l| !l.is_empty()) {
                        if name.ends_with('/') {
                            continue;
                        }
                        seen.file(name, 0, None)?;
                    }
                    Ok(Some(()))
                })(),
                None => Ok(None),
            }
        }
    };
    Ok(result.map(|got| got.map(|_| seen.scan)))
}

/// How long `bsdtar -t` may list an archive, and how much it may print.
const LIST_SECS: u64 = 30;
const LIST_BYTES: u64 = 4 << 20;

/// What `cmd` prints, if it ends well within `limit` and `max` bytes of
/// output; otherwise it's killed and there's nothing.
pub(crate) fn run_bounded(cmd: std::process::Command, limit: std::time::Duration, max: u64) -> Option<Vec<u8>> {
    run_limited(cmd, limit, max).ok().filter(|(st, _, _)| st.success()).map(|(_, out, _)| out)
}

/// Why [`run_limited`] has nothing.
#[derive(Debug)]
pub(crate) enum RunError {
    Spawn(io::Error),
    TooLong,
    TooMuch,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Spawn(e) => write!(f, "{e}"),
            RunError::TooLong => write!(f, "it took too long"),
            RunError::TooMuch => write!(f, "it gave too much"),
        }
    }
}

/// New: run `cmd` (no input), killing it past `limit` or `max` bytes of
/// output: how it ended, its output and the start of its errors.
pub(crate) fn run_limited(mut cmd: std::process::Command, limit: std::time::Duration, max: u64) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), RunError> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(RunError::Spawn)?;
    let read = |r: Option<Box<dyn Read + Send>>, cap: u64, drain: bool| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let mut r = r.take(cap + 1);
                let _ = r.read_to_end(&mut buf);
                // Errors past the cap are drained, so the tool never blocks on them.
                if drain {
                    let _ = io::copy(&mut r.into_inner(), &mut io::sink());
                }
            }
            buf
        })
    };
    let out = read(child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>), max, false);
    let err = read(child.stderr.take().map(|e| Box::new(e) as Box<dyn Read + Send>), 4096, true);
    let deadline = Instant::now() + limit;
    let ended = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) if Instant::now() >= deadline => break Err(RunError::TooLong),
            // Past the cap, the reader stops: the tool would block on its output.
            Ok(None) if out.is_finished() => {
                std::thread::sleep(Duration::from_millis(100));
                match child.try_wait() {
                    Ok(Some(st)) => break Ok(st),
                    _ => break Err(RunError::TooMuch),
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => break Err(RunError::Spawn(e)),
        }
    };
    let st = match ended {
        Ok(st) => st,
        // The readers are left to end with the pipes (anything the tool
        // started may still hold them).
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if out.len() as u64 > max {
        return Err(RunError::TooMuch);
    }
    Ok((st, out, err))
}

/// A tar, tar.gz or gzip file's contents for the viewer: (name, size,
/// date), reading no more than `cap` unpacked bytes.
pub fn tar_listing(path: &Path, kind: Kind, cap: u64) -> io::Result<Vec<(String, u64, Option<NaiveDate>)>> {
    let file = std::fs::File::open(path)?;
    let reader: Box<dyn Read> = if kind == Kind::Tar { Box::new(file) } else { Box::new(flate2::read::MultiGzDecoder::new(file)) };
    let hit = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut capped = Capped { inner: reader, left: cap, hit };
    if kind == Kind::Gzip {
        let n = io::copy(&mut capped, &mut io::sink())?;
        let name = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".gz").to_string()).unwrap_or_default();
        return Ok(vec![(name, n, None)]);
    }
    let mut out = Vec::new();
    for e in tar::Archive::new(&mut capped).entries()? {
        let e = e?;
        let h = e.header();
        if !h.entry_type().is_file() {
            continue;
        }
        let date = h.mtime().ok().and_then(|t| DateTime::from_timestamp(t as i64, 0)).map(|d| d.date_naive());
        out.push((e.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), h.size().unwrap_or(0), date));
    }
    Ok(out)
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

    #[test]
    fn listings_are_bounded() {
        use std::process::Command;
        use std::time::{Duration, Instant};
        let sh = |c: &str| {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(c);
            cmd
        };
        assert_eq!(run_bounded(sh("echo hi"), Duration::from_secs(5), 100).as_deref(), Some(&b"hi\n"[..]));
        let t = Instant::now();
        assert!(run_bounded(sh("sleep 10"), Duration::from_millis(200), 100).is_none());
        assert!(run_bounded(sh("yes"), Duration::from_secs(10), 1000).is_none());
        assert!(t.elapsed() < Duration::from_secs(5), "killed, not waited for");
    }

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
        // A SAUCE record after ^Z has NULs in it, and isn't counted.
        let mut art = b"ACiD news\r\nissue 59\r\n\x1a".to_vec();
        art.extend_from_slice(b"SAUCE00ACiD news\x00\x00\x00\x01\x01");
        std::fs::write(&txt, &art).unwrap();
        assert_eq!(check_contents(&txt, Kind::Text).unwrap(), Ok(()));
        assert_eq!(scan(&txt, Kind::Text, &limits()).unwrap().unwrap().unwrap().lines, Some(2));
        // ANSI art with a few NUL cells is still text.
        let mut ans = b"\x1b[1;30m\xdb\xdb\x1b[0m  \xb0\xb1\xb2 \x1b[33mart\r\n".repeat(40);
        ans.extend_from_slice(b"\x00\x00\r\n\x1a");
        std::fs::write(&txt, &ans).unwrap();
        assert_eq!(check_contents(&txt, Kind::Text).unwrap(), Ok(()));
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

#[cfg(test)]
pub(crate) mod fuzz {
    use super::*;
    use crate::fuzzkit;
    use std::io::Write;

    pub(crate) const DICT: &[&[u8]] = &[b"PK\x03\x04", b"PK\x01\x02", b"PK\x05\x06", b"PK\x06\x06", b"PK\x06\x07", b"\x1f\x8b\x08", b"ustar\x0000", b"ustar  \x00", b"FILE_ID.DIZ", b"../", b"/", b"\\", b"SAUCE00", b"COMNT", b"\x1a"];

    /// An ANSI piece with a SAUCE record and a comment.
    pub(crate) fn art() -> Vec<u8> {
        let mut a = b"\x1b[1;31mhello\x1b[0m\r\n\x1a".to_vec();
        a.extend(b"COMNT");
        a.extend(format!("{:<64}", "a comment line").as_bytes());
        let mut rec = vec![b' '; 128];
        rec[..7].copy_from_slice(b"SAUCE00");
        rec[7..12].copy_from_slice(b"Title");
        rec[42..47].copy_from_slice(b"Artst");
        rec[62..67].copy_from_slice(b"Group");
        rec[82..90].copy_from_slice(b"19970401");
        rec[104] = 1;
        a.extend(rec);
        a
    }

    pub(crate) fn zips() -> Vec<Vec<u8>> {
        let make = |files: &[(&str, &[u8], zip::CompressionMethod)]| {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, body, how) in files {
                let opts = zip::write::SimpleFileOptions::default().compression_method(*how).last_modified_time(zip::DateTime::from_date_and_time(1997, 4, 1, 12, 0, 0).unwrap());
                if name.ends_with('/') {
                    z.add_directory(*name, opts).unwrap();
                } else {
                    z.start_file(*name, opts).unwrap();
                    z.write_all(body).unwrap();
                }
            }
            z.finish().unwrap().into_inner()
        };
        let art = art();
        let (d, s) = (zip::CompressionMethod::Deflated, zip::CompressionMethod::Stored);
        vec![
            make(&[("FILE_ID.DIZ", b"The release\r\nline two\r\n", d), ("art/", b"", s), ("art/PIECE.ANS", &art, d), ("README.NFO", b"info", s)]),
            make(&[("ONLY.ANS", &art, s)]),
            make(&[("big.txt", &[b'a'; 100_000], d), ("pic.gif", b"GIF89a", s), ("sub/dir/x.asc", b"x", d)]),
        ]
    }

    pub(crate) fn tars() -> Vec<Vec<u8>> {
        let mut b = tar::Builder::new(Vec::new());
        for (name, body) in [("./art/PIECE.ANS", art()), ("README.TXT", b"readme\n".to_vec()), ("dir/sub/x.nfo", b"nfo".to_vec())] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_mtime(860_000_000);
            h.set_cksum();
            b.append_data(&mut h, name, body.as_slice()).unwrap();
        }
        let tar = b.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar).unwrap();
        vec![tar, gz.finish().unwrap()]
    }

    fn limits() -> Limits {
        Limits { max_ratio: 100, max_unpacked: 64 << 20, refuse_unsafe: true, keep_names: 5 }
    }

    /// Where a test's archive goes: its own temp directory.
    pub(crate) fn place(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fluxbbs-fuzz-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A ZIP upload: screened, tested, its description read, files out
    /// of it, all within the cap.
    #[test]
    #[ignore]
    fn fuzz_zip_upload() {
        let dir = place("zip");
        let path = dir.join("UPLOAD.ZIP");
        let names = vec!["FILE_ID.DIZ".to_string(), "DESC.SDI".to_string()];
        fuzzkit::run("zip_upload", zips(), DICT, 20_000, |d| {
            std::fs::write(&path, d).unwrap();
            let cap = limits().cap(d.len() as u64);
            let _ = check_contents(&path, Kind::Zip);
            let _ = scan(&path, Kind::Zip, &limits());
            if let Ok(entries) = crate::upltest::zip_entries(&path, cap) {
                for (name, _) in entries.iter().take(20) {
                    let mut out = Vec::new();
                    if let Ok(n) = crate::archview::copy_entry(&path, Kind::Zip, name, cap, &mut out) {
                        assert!(n <= cap && out.len() as u64 == n);
                    }
                }
            }
            let _ = crate::files::zip_description(&path, &names);
            let _ = crate::files::nfo_description(&path);
            let _ = crate::files::art_description(&path, &names);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tars, tar.gz and .gz: screened, listed and read within the cap.
    #[test]
    #[ignore]
    fn fuzz_tar_upload() {
        let dir = place("tar");
        let mut corpus = tars();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&art()).unwrap();
        corpus.push(gz.finish().unwrap());
        fuzzkit::run("tar_upload", corpus, DICT, 20_000, |d| {
            let kind = match d.first() {
                Some(0x1f) => Kind::TarGz,
                _ => Kind::Tar,
            };
            for kind in [kind, Kind::Gzip] {
                let path = dir.join(if kind == Kind::Gzip { "up.ans.gz" } else { "up.tar" });
                std::fs::write(&path, d).unwrap();
                let cap = limits().cap(d.len() as u64);
                let _ = check_contents(&path, kind);
                let _ = scan(&path, kind, &limits());
                if let Ok(list) = tar_listing(&path, kind, cap) {
                    for (name, _, _) in list.iter().take(20) {
                        let mut out = Vec::new();
                        if let Ok(n) = crate::archview::copy_entry(&path, kind, name, cap, &mut out) {
                            assert!(n <= cap);
                        }
                    }
                }
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Text uploads: their kind, lines and look.
    #[test]
    #[ignore]
    fn fuzz_text_upload() {
        let dir = place("text");
        let path = dir.join("up.txt");
        fuzzkit::run("text_upload", vec![art(), b"plain text\nlines\r\n".to_vec()], DICT, 20_000, |d| {
            std::fs::write(&path, d).unwrap();
            let _ = check_contents(&path, Kind::Text);
            let _ = scan(&path, Kind::Text, &limits());
            let _ = looks_like_text(d);
            let s = String::from_utf8_lossy(d);
            let _ = kind_of(&s);
            let _ = type_allowed(&["zip".into(), ".tar.gz".into(), String::new()], &s);
            let _ = unsafe_path(&s);
            let _ = human_size(d.iter().fold(0u64, |a, &b| a.rotate_left(8) ^ b as u64));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
