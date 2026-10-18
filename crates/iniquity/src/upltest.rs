//! Testing uploads (the original's internal file tester, shown on
//! FILETEST.ANS): an integrity check, the age limit, an outside test
//! command (what the virus-scanner setting was for), stripping files on
//! the delete list and adding the archive comment. Archive work is for
//! ZIPs, which the board reads itself; other formats only get the
//! outside test. Decompression, external maintenance and SAUCE checks
//! aren't done (lights L2, L7, L9 stay dark).

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use chrono::{Datelike, Local, NaiveDate};

use crate::extprog::shell_quote;
use crate::filebase::wildcard;
use crate::config::SummaryAt;
use crate::files::{DESC_WIDTH, MAX_DESC_LINES, commas, is_zip};
use crate::upscan::{Limits, Refused, Scan, check_contents, human_size, kind_of, type_allowed};
use crate::session::{Session, waves};

/// Read every entry (which checks each CRC) and give (name, date).
/// Unpacking stops past `cap` bytes, whatever the headers claimed.
pub fn zip_entries(path: &Path, cap: u64) -> anyhow::Result<Vec<(String, Option<NaiveDate>)>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut out = Vec::new();
    let mut left = cap;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let date = f.last_modified().and_then(|d| NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32));
        let name = f.name().map(|n| n.into_owned()).unwrap_or_default();
        if !f.is_dir() {
            let n = io::copy(&mut io::Read::take(&mut f, left.saturating_add(1)), &mut io::sink())?;
            if n > left {
                anyhow::bail!("unpacks to more than {cap} bytes");
            }
            left -= n;
        }
        out.push((name, date));
    }
    Ok(out)
}

/// The age test: with `strict`, every file must be newer than the limit;
/// otherwise the newest one must be. Gives (passed, the date judged).
pub fn age_test(dates: &[NaiveDate], limit_years: u32, strict: bool, today: NaiveDate) -> (bool, Option<NaiveDate>) {
    let Some(judged) = (if strict { dates.iter().min() } else { dates.iter().max() }).copied() else { return (true, None) };
    let cutoff = today.with_year(today.year() - limit_years as i32).unwrap_or(today);
    (judged >= cutoff, Some(judged))
}

/// Rewrite a ZIP without the entries `drop` picks, with `comment` as its
/// comment if given. Returns how many entries went.
pub fn zip_rewrite(path: &Path, drop: impl Fn(&str) -> bool, comment: Option<&[u8]>) -> anyhow::Result<usize> {
    let mut src = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let tmp = path.with_extension("iqtmp");
    let mut dropped = 0;
    let result = (|| -> anyhow::Result<()> {
        let mut out = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        if let Some(c) = comment {
            out.set_raw_comment(c.to_vec().into_boxed_slice())?;
        }
        for i in 0..src.len() {
            let entry = src.by_index_raw(i)?;
            let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
            let base = name.rsplit('/').next().unwrap_or(&name);
            if !name.ends_with('/') && drop(base) {
                dropped += 1;
                continue;
            }
            out.raw_copy_file(entry)?;
        }
        out.finish()?.flush()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    Ok(dropped)
}

/// Patterns in a list file (one per line; `;` and `#` start comments).
pub fn read_list(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .map(|l| l.split([';', '#']).next().unwrap_or("").trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl Session {
    /// An upload whose type isn't allowed here.
    pub(crate) async fn say_type_refused(&mut self, name: &str) -> io::Result<()> {
        let types = self.state.config().files.allowed_types.join(", ");
        tracing::info!("{}: upload {name} refused: not one of {types}", self.user.handle);
        let s = self.strings().fmt("FaULtypeRefused", &[name, &types]);
        self.show_line(&s).await
    }

    /// The checks before `test_upload`: an allowed type, contents that
    /// match the name, and a look inside (which also catches zip bombs
    /// and unsafe paths). `None` if refused, having said why; otherwise
    /// what was found, if anything.
    pub(crate) async fn screen_upload(&mut self, path: &Path, name: &str) -> io::Result<Option<Option<Scan>>> {
        let f = self.state.config().files.clone();
        if !type_allowed(&f.allowed_types, name) {
            self.say_type_refused(name).await?;
            return Ok(None);
        }
        let kind = kind_of(name);
        if let Err(what) = check_contents(path, kind)? {
            tracing::info!("{}: upload {name} refused: not really {what}", self.user.handle);
            let s = self.strings().fmt("FaULnotWhatItSays", &[name, what]);
            self.show_line(&s).await?;
            return Ok(None);
        }
        let limits = Limits::from_config(&f);
        let p = path.to_path_buf();
        let found = tokio::task::spawn_blocking(move || crate::upscan::scan(&p, kind, &limits)).await.map_err(io::Error::other)??;
        let (refused, why) = match found {
            Ok(scan) => return Ok(Some(scan)),
            Err(Refused::Bomb) => (self.strings().fmt("FaULbomb", &[name]), "unpacks too large".to_string()),
            Err(Refused::Unsafe(inside)) => (self.strings().fmt("FaULunsafe", &[name, &inside]), format!("unsafe path {inside:?}")),
            Err(Refused::Damaged(e)) => (self.strings().fmt("FaULdamaged", &[name, &e]), format!("unreadable: {e}")),
        };
        tracing::info!("{}: upload {name} refused: {why}", self.user.handle);
        self.show_line(&refused).await?;
        Ok(None)
    }

    /// Put the summary of what's inside into a description, first or
    /// last as `[files] scan_summary` says, within the line limit.
    pub(crate) fn add_summary(&self, desc: &mut Vec<String>, scan: Option<&Scan>) {
        let at = self.state.config().files.scan_summary;
        let Some(s) = scan.filter(|_| at != SummaryAt::Off) else { return };
        let st = self.strings();
        let mut lines = Vec::new();
        if let Some(n) = s.lines {
            lines.push(st.fmt("FaScanText", &[&commas(n)]));
        } else {
            let files = if s.files == 1 { "1 file".to_string() } else { format!("{} files", commas(s.files)) };
            lines.push(match (s.unpacked, s.newest) {
                (0, _) => st.fmt("FaScanCount", &[&files]),
                (_, Some(d)) => st.fmt("FaScanArchive", &[&files, &human_size(s.unpacked), &d.format("%m/%d/%y").to_string()]),
                (_, None) => st.fmt("FaScanUndated", &[&files, &human_size(s.unpacked)]),
            });
            if s.encrypted {
                lines.push(st.get("FaScanLocked").to_string());
            }
            lines.extend(s.names.iter().map(|n| st.fmt("FaScanName", &[n])));
        }
        let lines: Vec<String> = lines.into_iter().map(|l| l.chars().take(DESC_WIDTH).collect()).collect();
        desc.truncate(MAX_DESC_LINES.saturating_sub(lines.len()));
        match at {
            SummaryAt::First => {
                desc.splice(0..0, lines);
            }
            _ => desc.extend(lines),
        }
    }

    /// Test an upload, with its lights and lines on FILETEST. Ok(false):
    /// the file is to be refused.
    pub async fn test_upload(&mut self, path: &Path, name: &str) -> io::Result<bool> {
        let cfg = self.state.config();
        let f = cfg.files.clone();
        let data = cfg.resolve(&cfg.paths.data);
        drop(cfg);
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let zip = is_zip(path);
        let anything = zip || !f.test_command.trim().is_empty();
        if !anything {
            return Ok(true);
        }
        self.newline();
        let screen = self.show_file_with("FILETEST", &waves([("FN", name.to_string()), ("FS", commas(size))])).await?;
        if screen {
            self.goto_pos(*b"PI");
        }
        let mut entries = Vec::new();
        if zip {
            self.light(screen, b"L1").await?;
            let cap = crate::upscan::Limits::from_config(&f).cap(size);
            match zip_entries(path, cap) {
                Ok(e) => {
                    entries = e;
                    self.light_done(screen, b"L1", true);
                    let s = self.strings().fmt("FaTestCRCok", &[name]);
                    self.show_line(&s).await?;
                }
                Err(e) => {
                    tracing::info!("{}: upload {name} failed the integrity test: {e}", self.user.handle);
                    self.light_done(screen, b"L1", false);
                    let s = self.strings().fmt("FaTestCRCfail", &[name]);
                    self.show_line(&s).await?;
                    return Ok(false);
                }
            }
        }
        if zip && f.age_limit_years > 0 {
            self.light(screen, b"L4").await?;
            let dates: Vec<NaiveDate> = entries.iter().filter_map(|(_, d)| *d).collect();
            let (ok, judged) = age_test(&dates, f.age_limit_years as u32, f.strict_age, Local::now().date_naive());
            let shown = judged.map(|d| d.format("%m/%d/%y").to_string()).unwrap_or_default();
            self.light_done(screen, b"L4", ok);
            let s = self.strings().fmt(if ok { "FaTestAgeOk" } else { "FaTestAgeFail" }, &[name, &shown]);
            self.show_line(&s).await?;
            if !ok {
                tracing::info!("{}: upload {name} failed the age test ({shown})", self.user.handle);
                return Ok(false);
            }
        }
        if !f.test_command.trim().is_empty() {
            self.light(screen, b"L3").await?;
            self.flush().await?;
            let cmd = f.test_command.replace("%F", &shell_quote(&path.to_string_lossy()));
            let status = tokio::time::timeout(
                Duration::from_secs(300),
                tokio::process::Command::new("sh").arg("-c").arg(&cmd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status(),
            )
            .await;
            let ok = matches!(status, Ok(Ok(st)) if st.code() == Some(f.test_success_code));
            if let Ok(Err(e)) = &status {
                tracing::error!("upload test command {cmd:?}: {e}");
            }
            self.light_done(screen, b"L3", ok);
            let s = self.strings().fmt(if ok { "FaTestVirusOk" } else { "FaTestVirusFail" }, &[name]);
            self.show_line(&s).await?;
            if !ok {
                tracing::info!("{}: upload {name} failed the test command", self.user.handle);
                return Ok(false);
            }
        }
        if zip {
            let patterns = if f.delete_list.trim().is_empty() { Vec::new() } else { read_list(&data.join(f.delete_list.trim())) };
            let comment = if f.archive_comment.trim().is_empty() { None } else { std::fs::read(data.join(f.archive_comment.trim())).ok() };
            if !patterns.is_empty() || comment.is_some() {
                let drop = |n: &str| patterns.iter().any(|p| wildcard(p, n));
                let stripping = entries.iter().any(|(n, _)| drop(n.rsplit('/').next().unwrap_or(n)));
                if stripping {
                    self.light(screen, b"L5").await?;
                }
                if comment.is_some() {
                    self.light(screen, b"L6").await?;
                }
                match zip_rewrite(path, drop, comment.as_deref()) {
                    Ok(n) => {
                        if stripping {
                            self.light_done(screen, b"L5", true);
                            let s = self.strings().fmt("FaTestFilesRemoved", &[&n.to_string(), name]);
                            self.show_line(&s).await?;
                        }
                        if comment.is_some() {
                            self.light_done(screen, b"L6", true);
                            let s = self.strings().fmt("FaTestCommented", &[name]);
                            self.show_line(&s).await?;
                        }
                    }
                    Err(e) => {
                        // Not the uploader's fault: keep the file as it is.
                        tracing::warn!("rewriting upload {name}: {e}");
                        self.light_done(screen, b"L5", false);
                    }
                }
            }
        }
        Ok(true)
    }

    async fn light(&mut self, screen: bool, code: &[u8; 2]) -> io::Result<()> {
        if screen {
            let c = self.state.config().prompts.light;
            let here = (self.screen.where_x(), self.screen.where_y());
            if self.goto_pos(*code) {
                self.write(&c.to_string());
            }
            self.screen.goto(here.0, here.1);
        }
        self.flush().await
    }

    fn light_done(&mut self, screen: bool, code: &[u8; 2], ok: bool) {
        if screen {
            let p = &self.state.config().prompts;
            let c = if ok { p.light_ok } else { p.light_fail };
            let here = (self.screen.where_x(), self.screen.where_y());
            if self.goto_pos(*code) {
                self.write(&c.to_string());
            }
            self.screen.goto(here.0, here.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn age_limits() {
        let today = d(2026, 10, 17);
        let dates = [d(2026, 1, 1), d(2020, 5, 5)];
        assert_eq!(age_test(&dates, 4, false, today), (true, Some(d(2026, 1, 1))), "newest is fresh");
        assert_eq!(age_test(&dates, 4, true, today), (false, Some(d(2020, 5, 5))), "strict: oldest is stale");
        assert_eq!(age_test(&[], 4, true, today), (true, None));
        assert!(age_test(&[d(2022, 10, 17)], 4, true, today).0, "exactly on the cutoff passes");
        assert!(!age_test(&[d(2022, 10, 16)], 4, true, today).0);
    }

    #[test]
    fn zip_entries_rewrite_and_lists() {
        let dir = std::env::temp_dir().join(format!("iqupl{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("UP.ZIP");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let old = zip::DateTime::from_date_and_time(1995, 3, 4, 1, 2, 3).unwrap();
            let opts = zip::write::SimpleFileOptions::default().last_modified_time(old);
            w.start_file("README.TXT", opts).unwrap();
            w.write_all(b"hello").unwrap();
            w.start_file("sub/BBSAD.TXT", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"call my board").unwrap();
            w.start_file("PROG.EXE", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"MZ").unwrap();
            w.finish().unwrap();
        }
        let e = zip_entries(&path, u64::MAX).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0], ("README.TXT".to_string(), Some(d(1995, 3, 4))));
        let list = dir.join("DELETE.LST");
        std::fs::write(&list, "; ads\nBBSAD.*\n*.NFO  # scene\n\n").unwrap();
        let patterns = read_list(&list);
        assert_eq!(patterns, ["BBSAD.*", "*.NFO"]);
        let dropped = zip_rewrite(&path, |n| patterns.iter().any(|p| wildcard(p, n)), Some(b"Visit the board!")).unwrap();
        assert_eq!(dropped, 1);
        let z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let names: Vec<String> = z.file_names().filter_map(Result::ok).map(|n| n.into_owned()).collect();
        assert_eq!(names, ["README.TXT", "PROG.EXE"]);
        assert_eq!(z.comment(), b"Visit the board!");
        // A damaged archive fails the integrity test.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[41] ^= 0xFF; // inside the first entry's data (30-byte header + 10-byte name)
        std::fs::write(&path, bytes).unwrap();
        assert!(zip_entries(&path, u64::MAX).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
