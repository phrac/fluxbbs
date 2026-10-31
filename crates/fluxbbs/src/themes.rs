//! Screen themes: Iniquity's text libraries (INIQLIB, `*T`). A theme is a
//! directory of screens (`themes/NAME/`) that replaces any of the files in
//! `text/`; whatever it doesn't have comes from `text/` as usual. The
//! original packed each library into a `.TFL` file and only the SysOp
//! could pick one for a user; here they're plain directories and callers
//! choose their own (`UT`). The first theme listed is the default.
//!
//! New: a theme can carry menus of its own too, in a `menus/` directory
//! inside it, ahead of the board's: a menu laid over a theme's picture
//! needs its lightbar drawn to fit.

use std::io;
use std::path::PathBuf;

use fluxbbs_term::Slot;
use fluxbbs_term::codes::{resize, squish};
use serde::{Deserialize, Serialize};

use crate::cfgedit::{Field, Form};
use crate::input::{DIGITS, ReadOpts};
use crate::session::Session;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    /// Its directory, under the themes directory.
    pub dir: String,
    pub desc: String,
    #[serde(default)]
    pub author: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Themes {
    #[serde(rename = "theme", default)]
    pub list: Vec<Theme>,
}

impl Themes {
    pub fn to_toml(&self) -> String {
        format!(
            "# Screen themes (text libraries). Each is a directory under themes/ whose\n\
             # files replace the same-named screens in text/. The first is the default;\n\
             # callers can pick another (UT).\n\n{}",
            toml::to_string_pretty(self).expect("themes serialize")
        )
    }

    /// The theme a user with this choice gets: theirs if it's listed,
    /// else the first.
    pub fn for_user(&self, choice: &str) -> Option<&Theme> {
        self.list.iter().find(|t| !choice.is_empty() && t.dir.eq_ignore_ascii_case(choice)).or(self.list.first())
    }
}

/// Screens in a theme directory.
fn count_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map(|rd| rd.filter_map(Result::ok).filter(|e| e.path().is_file()).count()).unwrap_or(0)
}

impl Session {
    /// The caller's theme, if any themes exist.
    pub fn current_theme(&self) -> Option<Theme> {
        self.state.themes().for_user(&self.user.theme).cloned()
    }

    /// New: the theme's menus directory, if it has one.
    pub fn theme_menus(&self) -> Option<PathBuf> {
        self.theme_dir().map(|d| d.join("menus")).filter(|d| d.is_dir())
    }

    /// Where the caller's theme's screens are.
    pub fn theme_dir(&self) -> Option<PathBuf> {
        let t = self.current_theme()?;
        Some(self.state.config().resolve(&self.state.config().paths.theme_dir).join(t.dir))
    }

    /// `UT`: pick a screen theme.
    pub async fn choose_theme(&mut self) -> io::Result<()> {
        let themes = self.state.themes();
        self.newline();
        if themes.list.is_empty() {
            return self.say_line("ThemeNone").await;
        }
        let current = self.current_theme().map(|t| t.dir).unwrap_or_default();
        self.set_slot(Slot::Info);
        self.write(&format!(" {} {} By", resize("Num", 4), resize("Theme", 36)));
        self.newline();
        self.rule().await?;
        for (i, t) in themes.list.iter().enumerate() {
            let mark = if t.dir == current { "*" } else { " " };
            self.set_slot(if t.dir == current { Slot::TextHi } else { Slot::Text });
            self.write(&format!("{mark}{} {} {}", resize(&(i + 1).to_string(), 4), resize(&t.desc, 36), squish(&t.author, 36)));
            self.newline();
        }
        self.rule().await?;
        let s = self.strings().fmt("ThemeAsk", &[&themes.list.len().to_string()]);
        self.show(&s).await?;
        let pick = self.read_string(ReadOpts::new(3).allowed(DIGITS)).await?;
        self.newline();
        let Some(t) = pick.parse::<usize>().ok().and_then(|n| themes.list.get(n.wrapping_sub(1))) else { return Ok(()) };
        self.user.theme = t.dir.clone();
        self.save_user();
        let s = self.strings().fmt("ThemeSet", &[&t.desc]);
        self.show_line(&s).await
    }

    /// `cfgTextLibEditor` (`*T`): list, add, remove, move and edit themes.
    pub async fn theme_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: theme edit", self.user.handle);
        let root = self.state.config().resolve(&self.state.config().paths.theme_dir);
        loop {
            let mut themes = (*self.state.themes()).clone();
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Directory   Description                Author               Files");
            self.newline();
            self.rule().await?;
            for (i, t) in themes.list.iter().enumerate() {
                let files = count_files(&root.join(&t.dir));
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&t.dir, 11),
                    resize(&t.desc, 26),
                    resize(&t.author, 20),
                    files
                ));
                self.newline();
            }
            if themes.list.is_empty() {
                self.write(" No themes: everyone sees text/. Insert one, then put screens in its directory.");
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            let n = themes.list.len() as u32;
            match cmd {
                'E' => {
                    self.newline();
                    if n > 0
                        && let Some(num) = self.cfg_ask_number("Begin edit with which theme?", 1, n).await?
                    {
                        self.theme_edit(num).await?;
                    }
                }
                'D' => {
                    self.newline();
                    if n == 0 {
                        continue;
                    }
                    let Some(which) = self.cfg_ask_number("Delete which theme?", 1, n).await? else { continue };
                    let gone = themes.list.remove(which as usize - 1);
                    self.state.set_themes(themes).map_err(io_err)?;
                    let dir = root.join(&gone.dir);
                    if dir.is_dir() {
                        self.show(&format!("|U1Also delete |U3{}|U1 and its {} screens? ", dir.display(), count_files(&dir))).await?;
                        if self.yes_no(false).await? {
                            std::fs::remove_dir_all(&dir)?;
                        }
                    }
                }
                _ => {
                    let fresh = |list: &[Theme]| {
                        let taken = |d: &str| list.iter().any(|t| t.dir.eq_ignore_ascii_case(d));
                        let dir = (1..).map(|i| if i == 1 { "new".to_string() } else { format!("new{i}") }).find(|d| !taken(d)).unwrap();
                        Theme { dir, desc: "New theme".into(), author: String::new() }
                    };
                    let mut list = themes.list.clone();
                    if self.list_change(&mut list, cmd, "theme", fresh).await? {
                        for t in &list {
                            std::fs::create_dir_all(root.join(&t.dir))?;
                        }
                        themes.list = list;
                        self.state.set_themes(themes).map_err(io_err)?;
                    }
                }
            }
        }
    }

    async fn theme_edit(&mut self, mut num: u32) -> io::Result<()> {
        let root = self.state.config().resolve(&self.state.config().paths.theme_dir);
        loop {
            let mut themes = (*self.state.themes()).clone();
            let n = themes.list.len() as u32;
            let idx = num as usize - 1;
            let mut t = themes.list[idx].clone();
            let mut f = Form::new(&format!("{} Theme Editor", self.state.config().bbs.name));
            f.label("Current theme", 12, format!("{num} of {n}{}", if num == 1 { " (default)" } else { "" }));
            f.item('A', "Description", 36, &t.desc, "What callers see when picking a theme.");
            f.item('B', "Author", 36, &t.author, "Who made it.");
            f.item('C', "Directory", 20, &t.dir, "Its directory under themes/ (renaming moves it).");
            f.label("Screens", 6, count_files(&root.join(&t.dir)).to_string());
            f.item('[', "Previous theme", 0, "", "");
            f.item(']', "Next theme", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            t.desc = v;
                        }
                    }
                    'B' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            t.author = v;
                        }
                    }
                    'C' => {
                        let field = Field { allowed: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-", ..Field::TEXT };
                        let Some(v) = self.cfg_text(&mut f, key, field).await? else { continue };
                        let clash = themes.list.iter().enumerate().any(|(i, o)| i != idx && o.dir.eq_ignore_ascii_case(&v));
                        let (old, new) = (root.join(&t.dir), root.join(&v));
                        // An unlisted directory (an uninstalled library) can be
                        // taken over by a theme that has no screens of its own.
                        let adopt = new.is_dir() && !v.eq_ignore_ascii_case(&t.dir) && count_files(&old) == 0;
                        if v.is_empty() || clash || new.exists() && !v.eq_ignore_ascii_case(&t.dir) && !adopt {
                            self.cfg_info(&mut f, "That directory is taken.");
                            self.cfg_set(&mut f, key, &t.dir);
                        } else if v != t.dir {
                            if adopt {
                                if old.is_dir() {
                                    std::fs::remove_dir(&old)?;
                                }
                            } else if old.is_dir() {
                                std::fs::rename(&old, &new)?;
                            }
                            // Users who picked it follow it.
                            for mut u in self.state.users.list().map_err(io_err)? {
                                if u.theme.eq_ignore_ascii_case(&t.dir) {
                                    let original = u.clone();
                                    u.theme = v.clone();
                                    self.save_other_user(&original, &u).map_err(io_err)?;
                                }
                            }
                            if self.user.theme.eq_ignore_ascii_case(&t.dir) {
                                self.user.theme = v.clone();
                            }
                            t.dir = v;
                            themes.list[idx] = t.clone();
                            self.state.set_themes(themes.clone()).map_err(io_err)?;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if themes.list[idx] != t {
                themes.list[idx] = t;
                self.state.set_themes(themes).map_err(io_err)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_get_their_theme_or_the_first() {
        let t = |d: &str| Theme { dir: d.into(), desc: d.into(), author: String::new() };
        let themes = Themes { list: vec![t("dark"), t("light")] };
        assert_eq!(themes.for_user("LIGHT").unwrap().dir, "light");
        assert_eq!(themes.for_user("").unwrap().dir, "dark");
        assert_eq!(themes.for_user("gone").unwrap().dir, "dark", "a removed theme falls back");
        assert!(Themes::default().for_user("dark").is_none());
    }
}
