//! `fileareas.toml`: file conferences and areas (FILECONF.DAT and
//! FILEAREA.DAT).

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::msgareas::Conf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileAreas {
    #[serde(rename = "conference", default)]
    pub confs: Vec<Conf>,
    #[serde(rename = "area", default)]
    pub areas: Vec<FileArea>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
