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
}
