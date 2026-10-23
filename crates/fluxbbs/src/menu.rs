//! Menu files (`menus/NAME.toml`), converted from *.MNU by the importer.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuType {
    /// Display the help file (or a generated menu if it's missing).
    #[default]
    Normal,
    /// Always generate the menu from the command list.
    Generic,
    /// Highlight bar over positions marked in the help file.
    Lightbar,
}

/// How a menu decides expert mode and hot keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Follow the user's setting.
    #[default]
    User,
    Forced,
    Disabled,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn three() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Menu {
    #[serde(rename = "type", default, skip_serializing_if = "is_default")]
    pub menu_type: MenuType,
    /// Title for generated menus.
    #[serde(default)]
    pub title: String,
    /// Shown by `%MP`.
    #[serde(default)]
    pub prompt_name: String,
    /// Text file displayed as the menu.
    #[serde(default)]
    pub help_file: String,
    /// Prompt string, or `%%FILE` to display a text file.
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub acs: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// Menu to use if this one can't be loaded or entered.
    #[serde(default)]
    pub fallback: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub expert: Mode,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hot_keys: Mode,
    #[serde(default = "three")]
    pub generic_columns: u8,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub clear_before: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub center_title: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub show_prompt: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub pause_before: bool,
    /// Append GLOBAL's commands.
    #[serde(default, skip_serializing_if = "is_default")]
    pub global: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub upper_input: bool,
    #[serde(default, rename = "command")]
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Key(s) that run it: `X`, `/X`, `//WORD`, or `FIRSTCMD` (run on entry).
    /// Several commands may share keys; they run in order ("stacking").
    pub keys: String,
    /// Two-character command code (see `commands.rs`).
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub param: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub acs: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hidden: bool,
    /// Lightbar text when highlighted / not highlighted (r5).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub off: String,
}

pub const FIRST_CMD: &str = "FIRSTCMD";

impl Menu {
    /// The menu from the first of `dirs` that has it.
    pub fn load(dirs: &[PathBuf], name: &str) -> anyhow::Result<Menu> {
        let file = format!("{}.toml", name.to_ascii_uppercase());
        let path = dirs.iter().find_map(|d| crate::session::find_ci(d, &file)).ok_or_else(|| anyhow::anyhow!("menu {name} not found"))?;
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("menu serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let m: Menu = toml::from_str(
            r#"
            title = "Main"
            [[command]]
            keys = "G"
            command = "-/"
            param = "GOODBYE"
            "#,
        )
        .unwrap();
        assert_eq!(m.menu_type, MenuType::Normal);
        assert!(m.clear_before && m.show_prompt && !m.global);
        assert_eq!(m.generic_columns, 3);
        let again: Menu = toml::from_str(&m.to_toml()).unwrap();
        assert_eq!(again.commands[0].param, "GOODBYE");
    }
}
