//! `fileareas.toml`: file conferences and areas (FILECONF.DAT and
//! FILEAREA.DAT).

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::msgareas::Conf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAreas {
    #[serde(rename = "conference", default)]
    pub confs: Vec<Conf>,
    #[serde(rename = "area", default)]
    pub areas: Vec<FileArea>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileArea {
    pub name: String,
    /// Storage key; unique.
    pub file: String,
    /// Directory holding the files, relative to the config file.
    pub path: PathBuf,
    #[serde(default)]
    pub sponsor: String,
    #[serde(default)]
    pub acs: String,
    #[serde(default)]
    pub upload_acs: String,
    #[serde(default)]
    pub download_acs: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// How `*S` orders the area.
    #[serde(default, skip_serializing_if = "SortBy::is_none")]
    pub sort: SortBy,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sort_descending: bool,
    /// The storage key of the area this one is a sub-area of; empty for a
    /// top-level area. (New: the original's areas were flat.)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent: String,
}

/// `SortType`: the key a file area is sorted by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    #[default]
    None,
    Name,
    Extension,
    Size,
    Date,
    Uploaded,
    Uploader,
    Downloads,
}

impl SortBy {
    pub const ALL: [SortBy; 8] = [
        SortBy::None,
        SortBy::Name,
        SortBy::Extension,
        SortBy::Size,
        SortBy::Date,
        SortBy::Uploaded,
        SortBy::Uploader,
        SortBy::Downloads,
    ];

    pub fn is_none(&self) -> bool {
        *self == SortBy::None
    }

    /// The legacy `SortType` byte.
    pub fn from_legacy(n: u8) -> Self {
        Self::ALL.get(n as usize).copied().unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            SortBy::None => "None (unsorted)",
            SortBy::Name => "Filename",
            SortBy::Extension => "Extension",
            SortBy::Size => "Size",
            SortBy::Date => "Date",
            SortBy::Uploaded => "Upload date",
            SortBy::Uploader => "Uploader",
            SortBy::Downloads => "Downloads",
        }
    }
}

impl Default for FileAreas {
    fn default() -> Self {
        Self {
            confs: vec![Conf { key: 'X', desc: "New File Conference".into(), acs: String::new() }],
            areas: vec![FileArea {
                name: "New Iniquity file area".into(),
                file: "NEWAREA".into(),
                path: "files/newarea".into(),
                sponsor: "SysOp".into(),
                acs: "s25".into(),
                upload_acs: "s50".into(),
                download_acs: "s50".into(),
                password: String::new(),
                sort: SortBy::None,
                sort_descending: false,
                parent: String::new(),
            }],
        }
    }
}

impl FileAreas {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# File conferences and areas. Area numbers are positions in this file,\n\
             # from 1. Put an area in conference N with `acs = \"xN ...\"`. Paths\n\
             # are relative to this file.\n\n{}",
            toml::to_string_pretty(self).expect("areas serialize")
        )
    }

    /// Area by 1-based number.
    pub fn get(&self, n: u32) -> Option<&FileArea> {
        self.areas.get((n as usize).checked_sub(1)?)
    }

    pub fn conf(&self, n: u32) -> Option<&Conf> {
        self.confs.get((n as usize).checked_sub(1)?)
    }

    /// The area `n` is a sub-area of, if its parent exists.
    pub fn parent_of(&self, n: u32) -> Option<u32> {
        let a = self.get(n)?;
        if a.parent.is_empty() {
            return None;
        }
        let p = self.areas.iter().position(|o| o.file.eq_ignore_ascii_case(&a.parent))? as u32 + 1;
        (p != n).then_some(p)
    }

    /// Every area as (number, depth) in tree order: each area followed by
    /// its sub-areas, siblings in file order. An area whose parent is
    /// missing, or that is caught in a loop of parents, is top-level.
    pub fn tree(&self) -> Vec<(u32, usize)> {
        let total = self.areas.len() as u32;
        let parents: Vec<Option<u32>> = (1..=total).map(|n| self.parent_of(n)).collect();
        // An area whose parents lead back to itself becomes a root, which
        // breaks the loop for anything hanging below it.
        let roots_at = |n: u32| {
            let mut seen = vec![n];
            let mut cur = parents[n as usize - 1];
            while let Some(p) = cur {
                if seen.contains(&p) {
                    return p == n;
                }
                seen.push(p);
                cur = parents[p as usize - 1];
            }
            false
        };
        let parent: Vec<Option<u32>> = (1..=total).map(|n| if roots_at(n) { None } else { parents[n as usize - 1] }).collect();
        let mut out = Vec::with_capacity(self.areas.len());
        fn walk(parent: &[Option<u32>], of: Option<u32>, depth: usize, out: &mut Vec<(u32, usize)>) {
            for (i, p) in parent.iter().enumerate() {
                if *p == of {
                    let n = i as u32 + 1;
                    out.push((n, depth));
                    walk(parent, Some(n), depth + 1, out);
                }
            }
        }
        walk(&parent, None, 0, &mut out);
        out
    }

    /// The sub-areas directly under `n` (or the top-level areas for
    /// `None`), in tree order.
    pub fn children(&self, of: Option<u32>) -> Vec<u32> {
        let tree = self.tree();
        match of {
            None => tree.iter().filter(|(_, d)| *d == 0).map(|(n, _)| *n).collect(),
            Some(p) => {
                let Some(i) = tree.iter().position(|(n, _)| *n == p) else { return Vec::new() };
                let depth = tree[i].1;
                tree[i + 1..].iter().take_while(|(_, d)| *d > depth).filter(|(_, d)| *d == depth + 1).map(|(n, _)| *n).collect()
            }
        }
    }

    /// `n` and the areas above it, nearest first, as `tree` sees them.
    pub fn lineage(&self, n: u32) -> Vec<u32> {
        let tree = self.tree();
        let Some(mut i) = tree.iter().position(|(m, _)| *m == n) else { return Vec::new() };
        let mut out = vec![n];
        let mut depth = tree[i].1;
        while depth > 0 {
            i -= 1;
            if tree[i].1 < depth {
                depth = tree[i].1;
                out.push(tree[i].0);
            }
        }
        out
    }

    /// `n` and everything under it.
    pub fn subtree(&self, n: u32) -> Vec<u32> {
        let tree = self.tree();
        let Some(i) = tree.iter().position(|(m, _)| *m == n) else { return Vec::new() };
        let depth = tree[i].1;
        std::iter::once(n).chain(tree[i + 1..].iter().take_while(|(_, d)| *d > depth).map(|(m, _)| *m)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn areas(spec: &[(&str, &str)]) -> FileAreas {
        let mut fa = FileAreas::default();
        let proto = fa.areas[0].clone();
        fa.areas = spec.iter().map(|(file, parent)| FileArea { file: file.to_string(), parent: parent.to_string(), ..proto.clone() }).collect();
        fa
    }

    #[test]
    fn sub_areas_form_a_tree() {
        // 1 UPLOADS, 2 ISSUE1 (under ZINES), 3 ZINES, 4 PHRACK (under ZINES), 5 ISSUE2 (under PHRACK)
        let fa = areas(&[("UPLOADS", ""), ("ISSUE1", "zines"), ("ZINES", ""), ("PHRACK", "ZINES"), ("ISSUE2", "PHRACK")]);
        assert_eq!(fa.tree(), vec![(1, 0), (3, 0), (2, 1), (4, 1), (5, 2)]);
        assert_eq!(fa.children(None), vec![1, 3]);
        assert_eq!(fa.children(Some(3)), vec![2, 4]);
        assert_eq!(fa.children(Some(4)), vec![5]);
        assert_eq!(fa.lineage(5), vec![5, 4, 3]);
        assert_eq!(fa.subtree(3), vec![3, 2, 4, 5]);
        assert_eq!(fa.parent_of(2), Some(3));
    }

    #[test]
    fn broken_parents_are_top_level() {
        let fa = areas(&[("A", "B"), ("B", "A"), ("C", "GONE"), ("D", "D"), ("E", "A")]);
        let tree = fa.tree();
        assert_eq!(tree.len(), 5, "every area appears once: {tree:?}");
        assert!(tree.contains(&(3, 0)) && tree.contains(&(4, 0)));
        assert!(tree.contains(&(5, 1)), "E still hangs under A");
    }
}
