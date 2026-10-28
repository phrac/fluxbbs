//! New: backups of the board, as an event job (`backup [DAYS] [DIR]`) or
//! by hand (`fluxbbs backup [DAYS] [DIR] [config]`). One dated
//! `fluxbbs-YYYY-MM-DD-HHMMSS.tar.gz` holds the database (a snapshot taken
//! with `VACUUM INTO`, so the board keeps running), the settings, screens,
//! menus, door games' data and SSH host keys: everything in the board's
//! directory but the file areas' folders and the folders holding them
//! (their files are too big to copy nightly), the temporary and cache
//! folders, the TheDraw fonts, earlier backups and `.bak` copies. Backups older than DAYS days (7 if not given) are removed.

use std::path::{Path, PathBuf};

use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use anyhow::Context;

use crate::BbsState;

/// Where backups go when the event doesn't say.
pub const DIR: &str = "backups";
pub const KEEP_DAYS: u64 = 7;

/// `backup [DAYS] [DIR]`, from an event's or the command line's words.
pub fn parse(args: &[&str]) -> anyhow::Result<(u64, Option<PathBuf>)> {
    let mut days = KEEP_DAYS;
    let mut dir = None;
    for a in args {
        match a.parse::<u64>() {
            Ok(n) if n > 0 => days = n,
            Ok(_) => anyhow::bail!("backup: keep at least 1 day"),
            Err(_) => dir = Some(PathBuf::from(a)),
        }
    }
    Ok((days, dir))
}

/// Make a backup; returns its path.
pub fn run(state: &BbsState, days: u64, dir: Option<&Path>) -> anyhow::Result<PathBuf> {
    let cfg = state.config();
    // Absolute, so the board's files and the database line up however
    // the config was named.
    let root = cfg.resolve(Path::new(".")).canonicalize().context("the board's directory")?;
    let dir = cfg.resolve(dir.unwrap_or(Path::new(DIR)));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    // They hold the host keys and every account: the owner's alone.
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    // To the second, and the process: two runs never share a file.
    let stamp = crate::clock::now().format("%Y-%m-%d-%H%M%S");
    let out = dir.join(format!("fluxbbs-{stamp}.tar.gz"));
    let db = cfg.db_path().canonicalize().context("the database")?;
    let snapshot = dir.join(format!(".snapshot-{stamp}-{}.db", std::process::id()));
    let part = dir.join(format!(".fluxbbs-{stamp}-{}.part", std::process::id()));
    let made = (|| {
        let conn = rusqlite::Connection::open(&db)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.execute("VACUUM INTO ?1", [snapshot.to_string_lossy()])?;
        drop(conn);
        std::fs::set_permissions(&snapshot, std::fs::Permissions::from_mode(0o600))?;
        // Left out: the file areas' folders and those holding them, temp
        // and cache, backups. Never a folder the board's own things are
        // in (data, screens, menus, doors), whatever area sits inside it.
        let own: Vec<PathBuf> = [&cfg.paths.data, &cfg.paths.text, &cfg.paths.menus, &cfg.paths.doors, &cfg.paths.events]
            .iter()
            .filter_map(|p| cfg.resolve(p).canonicalize().ok())
            .collect();
        let mut skip: Vec<PathBuf> = Vec::new();
        for a in &state.fareas().areas {
            let Ok(p) = cfg.resolve(&a.path).canonicalize() else { continue };
            skip.extend(p.ancestors().take_while(|d| *d != root && d.starts_with(&root)).filter(|d| !own.iter().any(|o| o.starts_with(d))).map(Path::to_path_buf));
        }
        // New: and the TheDraw fonts, a collection fetched again easily.
        let more = [cfg.resolve(&cfg.paths.temp), cfg.resolve(&cfg.paths.data).join("cache"), cfg.resolve(&cfg.paths.fonts), dir.clone()];
        skip.extend(more.iter().filter_map(|p| p.canonicalize().ok()));
        let file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&part)?;
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        tar.follow_symlinks(false);
        let in_db = db.strip_prefix(&root).context("the database isn't in the board's directory")?.to_path_buf();
        tar.append_path_with_name(&snapshot, &in_db)?;
        let db_name = db.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut files = 1;
        add_dir(&mut tar, &root, Path::new(""), &skip, &db_name, &mut files)?;
        tar.into_inner()?.finish()?;
        std::fs::rename(&part, &out)?;
        anyhow::Ok(files)
    })();
    let _ = std::fs::remove_file(&snapshot);
    let _ = std::fs::remove_file(&part);
    let files = made.with_context(|| format!("backing up to {}", out.display()))?;
    let size = std::fs::metadata(&out).map_or(0, |m| m.len());
    tracing::info!("backup: {} ({files} files, {})", out.display(), crate::upscan::human_size(size));
    prune(&dir, days);
    Ok(out)
}

/// The board directory's files, in tar under their relative paths.
fn add_dir<W: std::io::Write>(tar: &mut tar::Builder<W>, root: &Path, rel: &Path, skip: &[PathBuf], db_name: &str, files: &mut u32) -> anyhow::Result<()> {
    let here = root.join(rel);
    let mut entries: Vec<_> = std::fs::read_dir(&here).with_context(|| here.display().to_string())?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let rel = rel.join(&name);
        // The live database (the snapshot stands in for it), its journals
        // and copies, and other `.bak` copies.
        if (!db_name.is_empty() && name.starts_with(db_name)) || name.contains(".bak") || name.ends_with(".part") {
            continue;
        }
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            if path.canonicalize().is_ok_and(|p| skip.contains(&p)) {
                continue;
            }
            add_dir(tar, root, &rel, skip, db_name, files)?;
        } else if kind.is_file() || kind.is_symlink() {
            // A file that went away meanwhile is no reason to fail.
            match tar.append_path_with_name(&path, &rel) {
                Ok(()) => *files += 1,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err).with_context(|| path.display().to_string()),
            }
        }
    }
    Ok(())
}

/// Remove backups older than `days`, and what a run that died left
/// behind (a day old).
fn prune(dir: &Path, days: u64) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    let max = std::time::Duration::from_secs(days * 86_400);
    let day = std::time::Duration::from_secs(86_400);
    for e in list.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let backup = name.starts_with("fluxbbs-") && name.ends_with(".tar.gz");
        let debris = name.starts_with(".snapshot-") || (name.starts_with(".fluxbbs-") && name.ends_with(".part"));
        if !backup && !debris {
            continue;
        }
        let limit = if backup { max } else { day };
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
        if old {
            match std::fs::remove_file(e.path()) {
                Ok(()) => tracing::info!("backup: removed {name}"),
                Err(err) => tracing::warn!("backup: removing {name}: {err}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args() {
        assert_eq!(parse(&[]).unwrap(), (7, None));
        assert_eq!(parse(&["14"]).unwrap(), (14, None));
        assert_eq!(parse(&["30", "/mnt/nas/bbs"]).unwrap(), (30, Some(PathBuf::from("/mnt/nas/bbs"))));
        assert!(parse(&["0"]).is_err());
    }
}
