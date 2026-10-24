//! `msgareas.toml`: message conferences and areas (MSGCONF.DAT,
//! MSGAREA.DAT and EMAIL.DAT).

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsgArea {
    pub name: String,
    /// Storage key; unique.
    pub file: String,
    /// New: what the board is for, a line in the message center's list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    /// New: the storage key of the area this is a sub-area of (as file
    /// areas have); the message center shows a parent as a folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent: String,
    /// New: the day (YYYY-MM-DD) a caller made this board; blank for the
    /// SysOp's areas.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    /// New: handles its moderator has banned from posting and voting here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub banned: Vec<String>,
    /// New: their user numbers (bans from before these were kept go by
    /// handle alone).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub banned_ids: Vec<u32>,
    /// New: the tags a post here can carry, PTT's categories: a new thread
    /// picks one and its title starts `[tag]`; the thread list filters on them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sponsor: String,
    /// New: the sponsor's user number, so whoever takes a deleted
    /// sponsor's handle doesn't inherit the area; 0: matched by handle.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sponsor_id: u32,
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
    /// New: a request board: each thread asks for something (a file, a
    /// door, a fix), and is marked filled once it's been found.
    #[serde(default, skip_serializing_if = "is_false")]
    pub requests: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub qwk_name: String,
    /// `*P` keeps at most this many messages (0: no limit).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_msgs: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl MsgArea {
    /// `maReset`'s new-area defaults.
    pub fn new(name: &str, file: &str) -> Self {
        Self {
            name: name.into(),
            file: file.into(),
            about: String::new(),
            parent: String::new(),
            created: String::new(),
            banned: Vec::new(),
            banned_ids: Vec::new(),
            tags: Vec::new(),
            sponsor: "SysOp".into(),
            sponsor_id: 0,
            acs: "s25".into(),
            post_acs: "s50".into(),
            password: String::new(),
            unhidden: false,
            real_names: false,
            private: true,
            mandatory: false,
            anonymous: true,
            requests: false,
            qwk_name: String::new(),
            max_msgs: 0,
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

    /// Every area as (number, depth), each followed by its sub-areas, as
    /// [`crate::fileareas::FileAreas::tree`].
    pub fn tree(&self) -> Vec<(u32, usize)> {
        crate::fileareas::tree_of_keys(&self.areas.iter().map(|a| (a.file.as_str(), a.parent.as_str())).collect::<Vec<_>>())
    }

    /// `n` and the areas above it, nearest first.
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

    #[test]
    fn sub_areas_nest_and_survive_saving() {
        let a = MsgAreas {
            areas: vec![
                MsgArea::new("General", "GEN"),
                MsgArea { parent: "CALLERS".into(), ..MsgArea::new("Mopar", "MOPAR") },
                MsgArea::new("Caller boards", "CALLERS"),
                MsgArea { parent: "NOWHERE".into(), ..MsgArea::new("Lost", "LOST") },
            ],
            ..MsgAreas::default()
        };
        assert_eq!(a.tree(), vec![(1, 0), (3, 0), (2, 1), (4, 0)]);
        assert_eq!(a.lineage(2), vec![2, 3]);
        let b: MsgAreas = toml::from_str(&a.to_toml()).unwrap();
        assert_eq!(b.areas[1].parent, "CALLERS");
        assert!(!a.to_toml().contains("parent = \"\""));
    }
}
