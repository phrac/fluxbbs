//! New: the file base kept in step with files moved by hand. A file
//! moved or renamed outside the board leaves a record whose file is gone
//! and a file with no record; packing would forget the one and adding
//! would give the other a new, blank record. Here they are paired up
//! first and the record pointed at the file, so its description,
//! downloads, uploader and dates stay. A pair is a record and a file of
//! the same size whose names agree: the same (in any case), or one is
//! the other with a prefix taken off or put on
//! (`Constitution_Strategy.txt` moved into `constitution/` as
//! `Strategy.txt`). A record that could be more than one file, or a file
//! more than one record, is left as it is and named, for the SysOp.
//! `fluxbbs pack-files` and `add-files` (`main.rs`), the `pack-files`
//! event and the in-board pack (`*K`) all work this way.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::filebase::{FileBase, FileRec};

/// A file area: its key, its name (to show) and its directory.
pub struct Area {
    pub file: String,
    pub name: String,
    pub dir: PathBuf,
}

/// The board's file areas, with their directories.
pub fn areas(cfg: &crate::config::Config, list: &[crate::fileareas::FileArea]) -> Vec<Area> {
    list.iter().map(|a| Area { file: a.file.clone(), name: crate::editor::strip_colours(&a.name), dir: cfg.resolve(&a.path) }).collect()
}

/// A new record for a file found in an area: as `*%` adds one, uploaded
/// by the SysOp.
pub fn new_record(cfg: &crate::config::Config, area: &str, path: &Path) -> anyhow::Result<FileRec> {
    let kb_pt = cfg.files.kb_per_point.max(1) as u64;
    Ok(crate::files::local_record(area, path, &cfg.bbs.sysop_alias, &cfg.files.description_files, kb_pt, cfg.files.image_limits())?)
}

/// What to do. `only`: the areas whose missing records are looked at
/// (all of them when None); files with no record are looked for in every
/// area either way.
#[derive(Default)]
pub struct Opts<'a> {
    pub dry_run: bool,
    /// Forget records of files that are gone and pair with nothing.
    pub prune: bool,
    /// Give files with no record, that pair with nothing, a new one.
    pub add: bool,
    pub only: Option<&'a HashSet<String>>,
}

/// What was done (or, dry, would be). Areas by their keys.
#[derive(Default, Debug)]
pub struct Report {
    /// A record pointed at its file: from area, old name, to area, new name.
    pub moved: Vec<(String, String, String, String)>,
    /// Records of files that are gone: area, name.
    pub removed: Vec<(String, String)>,
    /// Files given a new record: area, name.
    pub added: Vec<(String, String)>,
    /// Pairs that couldn't be told apart, left alone.
    pub unsure: Vec<String>,
}

impl Report {
    /// One line for each thing done, with the areas' names.
    pub fn lines(&self, areas: &[Area], dry_run: bool) -> Vec<String> {
        let names: HashMap<&str, &str> = areas.iter().map(|a| (a.file.as_str(), a.name.as_str())).collect();
        let name = |k: &str| names.get(k).map_or_else(|| k.to_string(), |n| n.to_string());
        let (moved, removed, added) = if dry_run { ("would move", "would remove", "would add") } else { ("moved", "removed", "added") };
        let mut out = Vec::new();
        for (fa, fname, ta, tname) in &self.moved {
            if fa == ta {
                out.push(format!("{}: {fname} {} to {tname} (renamed)", name(fa), if dry_run { "would be renamed" } else { "renamed" }));
            } else {
                out.push(format!("{}: {fname} {moved} to {}{}", name(fa), name(ta), if fname == tname { String::new() } else { format!(" as {tname}") }));
            }
        }
        out.extend(self.unsure.iter().map(|u| format!("left alone: {u}")));
        out.extend(self.removed.iter().map(|(a, n)| format!("{}: {n} is missing; record {removed}", name(a))));
        out.extend(self.added.iter().map(|(a, n)| format!("{}: {n} {added}", name(a))));
        out
    }

    pub fn summary(&self, dry_run: bool) -> String {
        format!(
            "{}{} record(s) moved, {} removed, {} file(s) added, {} left alone",
            if dry_run { "dry run: " } else { "" },
            self.moved.len(),
            self.removed.len(),
            self.added.len(),
            self.unsure.len()
        )
    }
}

/// Do the names `old` (a record's) and `new` (a file's) agree?
fn names_agree(old: &str, new: &str) -> bool {
    let (o, n) = (old.to_lowercase(), new.to_lowercase());
    o == n || tail_of(&o, &n) || tail_of(&n, &o)
}

/// Is `short` the end of `long`, after a separator?
fn tail_of(long: &str, short: &str) -> bool {
    long.len() > short.len() && long.ends_with(short) && long[..long.len() - short.len()].ends_with(['_', '-', '.', ' '])
}

/// A file with no record: its area (an index into the areas), name and size.
struct Loose {
    area: usize,
    name: String,
    size: u64,
}

/// Pair, then (as `opts` says) prune and add. `make` makes a new file's
/// record (`files::local_record`).
pub fn sync(files: &FileBase, areas: &[Area], opts: &Opts, mut make: impl FnMut(&str, &Path) -> anyhow::Result<FileRec>) -> anyhow::Result<Report> {
    let mut missing: Vec<FileRec> = Vec::new();
    let mut loose: Vec<Loose> = Vec::new();
    for (i, a) in areas.iter().enumerate() {
        let recs = files.list(&a.file)?;
        let named: HashSet<&str> = recs.iter().map(|f| f.name.as_str()).collect();
        if let Ok(dir) = std::fs::read_dir(&a.dir) {
            for e in dir.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_file() && !name.starts_with('.') && !named.contains(name.as_str()) {
                    loose.push(Loose { area: i, name, size: meta.len() });
                }
            }
        }
        if opts.only.is_none_or(|o| o.contains(&a.file)) {
            missing.extend(recs.into_iter().filter(|f| !a.dir.join(&f.name).is_file()));
        }
    }
    loose.sort_by(|x, y| (x.area, &x.name).cmp(&(y.area, &y.name)));
    // Each record's possible files, and how many records want each file.
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (j, l) in loose.iter().enumerate() {
        by_size.entry(l.size).or_default().push(j);
    }
    let cands: Vec<Vec<usize>> = missing
        .iter()
        .map(|m| by_size.get(&m.size).map_or_else(Vec::new, |js| js.iter().copied().filter(|&j| names_agree(&m.name, &loose[j].name)).collect()))
        .collect();
    let mut wanted = vec![0usize; loose.len()];
    for c in &cands {
        for &j in c {
            wanted[j] += 1;
        }
    }
    let mut report = Report::default();
    let label = |l: &Loose| format!("{}/{}", areas[l.area].name, l.name);
    for (m, c) in missing.iter().zip(&cands) {
        match c.as_slice() {
            [] if opts.prune => report.removed.push((m.area.clone(), m.name.clone())),
            [] => {}
            [j] if wanted[*j] == 1 => report.moved.push((m.area.clone(), m.name.clone(), areas[loose[*j].area].file.clone(), loose[*j].name.clone())),
            // More than one file, or a file another record wants too.
            _ => report.unsure.push(format!("the record {} could be {}", m.name, c.iter().map(|&j| label(&loose[j])).collect::<Vec<_>>().join(", "))),
        }
    }
    if opts.add {
        report.added = loose.iter().enumerate().filter(|(j, _)| wanted[*j] == 0).map(|(_, l)| (areas[l.area].file.clone(), l.name.clone())).collect();
    }
    if opts.dry_run {
        return Ok(report);
    }
    // Moves first, by name: each one renumbers the area it leaves.
    for (fa, fname, ta, tname) in &report.moved {
        let Some(f) = files.find(fa, fname)? else { continue };
        let mut f = if fa == ta {
            f
        } else {
            let n = files.move_to(fa, f.num, ta)?;
            files.get(ta, n)?.ok_or_else(|| anyhow::anyhow!("{ta}: record {n} vanished"))?
        };
        f.name.clone_from(tname);
        files.update(&f)?;
    }
    for (a, name) in &report.removed {
        if let Some(f) = files.find(a, name)? {
            files.remove(a, f.num)?;
        }
    }
    let mut new: HashMap<&str, Vec<FileRec>> = HashMap::new();
    for (a, name) in &report.added {
        let dir = &areas.iter().find(|x| &x.file == a).expect("an area listed above").dir;
        new.entry(a.as_str()).or_default().push(make(a, &dir.join(name))?);
    }
    for recs in new.values_mut() {
        files.add_all(recs)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(area: &str, name: &str, size: u64, desc: &str) -> FileRec {
        FileRec {
            area: area.into(),
            num: 0,
            name: name.into(),
            size,
            date: None,
            uploader: "pi".into(),
            uploaded: None,
            downloads: 7,
            points: 0,
            valid: true,
            description: vec![desc.into()],
        }
    }

    fn names(fb: &FileBase, area: &str) -> Vec<String> {
        fb.list(area).unwrap().iter().map(|f| format!("{}:{}:{}", f.num, f.name, f.description.join(" "))).collect()
    }

    #[test]
    fn names_agree_with_a_prefix_on_or_off() {
        assert!(names_agree("Constitution_Strategy.txt", "Strategy.txt"));
        assert!(names_agree("strategy.txt", "Constitution_Strategy.txt"));
        assert!(names_agree("FOO.ZIP", "foo.zip"));
        assert!(!names_agree("data.txt", "a.txt"));
        assert!(!names_agree("one.txt", "two.txt"));
    }

    #[test]
    fn moved_and_renamed_files_keep_their_records() {
        let fb = FileBase::open_memory().unwrap();
        let root = std::env::temp_dir().join(format!("filesync{}", std::process::id()));
        let (top, sub) = (root.join("tez"), root.join("tez/constitution"));
        std::fs::create_dir_all(&sub).unwrap();
        let areas = vec![
            Area { file: "TEZ".into(), name: "Tezcat".into(), dir: top.clone() },
            Area { file: "TEZ_CON".into(), name: "Constitution".into(), dir: sub.clone() },
        ];
        // Before: three files in TEZ, described.
        for (n, size, d) in [("Constitution_Strategy.txt", 5, "Rothbard"), ("keep.txt", 3, "stays"), ("gone.txt", 4, "deleted"), ("twin.txt", 2, "which?")] {
            fb.add(&mut rec("TEZ", n, size, d)).unwrap();
        }
        // After: one moved and renamed, one deleted, one stayed, a new
        // file, and a record with two files that could be it.
        std::fs::write(sub.join("Strategy.txt"), b"12345").unwrap();
        std::fs::write(top.join("keep.txt"), b"123").unwrap();
        std::fs::write(top.join("new.txt"), b"1").unwrap();
        std::fs::write(top.join("x_twin.txt"), b"12").unwrap();
        std::fs::write(sub.join("twin.txt"), b"12").unwrap();
        let make = |a: &str, p: &Path| Ok(rec(a, &p.file_name().unwrap().to_string_lossy(), 1, "fresh"));

        let dry = Opts { dry_run: true, prune: true, add: true, only: None };
        let r = sync(&fb, &areas, &dry, make).unwrap();
        assert_eq!(r.summary(true), "dry run: 1 record(s) moved, 1 removed, 1 file(s) added, 1 left alone");
        assert_eq!(names(&fb, "TEZ").len(), 4, "a dry run changes nothing");

        let r = sync(&fb, &areas, &Opts { dry_run: false, ..dry }, make).unwrap();
        assert_eq!(r.moved, [("TEZ".into(), "Constitution_Strategy.txt".into(), "TEZ_CON".into(), "Strategy.txt".into())]);
        assert_eq!(names(&fb, "TEZ_CON"), ["1:Strategy.txt:Rothbard"]);
        assert_eq!(fb.find("TEZ_CON", "Strategy.txt").unwrap().unwrap().downloads, 7);
        assert_eq!(names(&fb, "TEZ"), ["1:keep.txt:stays", "2:twin.txt:which?", "3:new.txt:fresh"]);
        assert!(r.lines(&areas, false).iter().any(|l| l.starts_with("left alone: the record twin.txt could be ")));
        assert!(r.lines(&areas, false).contains(&"Tezcat: Constitution_Strategy.txt moved to Constitution as Strategy.txt".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
