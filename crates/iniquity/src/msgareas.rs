//! `msgareas.toml`: message conferences and areas (MSGCONF.DAT,
//! MSGAREA.DAT and EMAIL.DAT).

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsgAreas {
    #[serde(rename = "conference", default)]
    pub confs: Vec<Conf>,
    /// The private mail area.
    pub email: MsgArea,
    #[serde(rename = "area", default)]
    pub areas: Vec<MsgArea>,
}

/// A conference groups areas; areas join one through ACS `Cn` (n = the
/// conference's position, from 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conf {
    pub key: char,
    pub desc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub acs: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsgArea {
    pub name: String,
    /// Storage key; unique.
    pub file: String,
    #[serde(default)]
    pub sponsor: String,
    #[serde(default)]
    pub acs: String,
    #[serde(default)]
    pub post_acs: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// Listed even to callers without access.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unhidden: bool,
    /// Messages are signed with real names.
    #[serde(default, skip_serializing_if = "is_false")]
    pub real_names: bool,
    /// Private messages allowed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
    /// Read during the mandatory scan.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mandatory: bool,
    /// Anonymous messages allowed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub anonymous: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub qwk_name: String,
}

impl MsgArea {
    /// `maReset`'s new-area defaults.
    pub fn new(name: &str, file: &str) -> Self {
        Self {
            name: name.into(),
            file: file.into(),
            sponsor: "SysOp".into(),
            acs: "s25".into(),
            post_acs: "s50".into(),
            password: String::new(),
            unhidden: false,
            real_names: false,
            private: true,
            mandatory: false,
            anonymous: true,
            qwk_name: String::new(),
        }
    }
}

impl Default for MsgAreas {
    fn default() -> Self {
        Self {
            confs: vec![Conf { key: 'X', desc: "New Message Conference".into(), acs: String::new() }],
            email: MsgArea {
                acs: String::new(),
                post_acs: String::new(),
                private: true,
                anonymous: false,
                ..MsgArea::new("Private Mail Message Area", "EMAIL")
            },
            areas: vec![MsgArea::new("New Iniquity Message Area", "NEWAREA")],
        }
    }
}

impl MsgAreas {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# Message conferences and areas. Area numbers are positions in this\n\
             # file, from 1. Put an area in conference N with `acs = \"cN ...\"`.\n\n{}",
            toml::to_string_pretty(self).expect("areas serialize")
        )
    }

    /// Area by 1-based number.
    pub fn get(&self, n: u32) -> Option<&MsgArea> {
        self.areas.get((n as usize).checked_sub(1)?)
    }

    pub fn conf(&self, n: u32) -> Option<&Conf> {
        self.confs.get((n as usize).checked_sub(1)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let a = MsgAreas::default();
        let b: MsgAreas = toml::from_str(&a.to_toml()).unwrap();
        assert_eq!(b.areas[0].file, "NEWAREA");
        assert_eq!(b.email.file, "EMAIL");
        assert_eq!(b.conf(1).unwrap().key, 'X');
        assert!(b.get(0).is_none() && b.get(2).is_none());
    }
}
