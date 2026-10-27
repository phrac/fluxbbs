//! `*#`: the menu editor (CONFIG4). Menus are files in the menus
//! directory and are read each time one is entered, so a saved change
//! takes effect the next time anyone goes to that menu.

use std::io;
use std::path::{Path, PathBuf};

use fluxbbs_term::Slot;
use fluxbbs_term::codes::{resize, squish};

use crate::cfgedit::{Field, Form, yes_no};
use crate::menu::{Command, Menu, MenuType, Mode};
use crate::session::{Session, find_ci};

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

const TYPES: [(MenuType, &str); 3] = [(MenuType::Normal, "Normal"), (MenuType::Generic, "Generic"), (MenuType::Lightbar, "LightBar")];
const MODES: [(Mode, &str); 3] = [(Mode::User, "Normal"), (Mode::Forced, "Forced"), (Mode::Disabled, "Disabled")];

fn type_label(t: MenuType) -> &'static str {
    TYPES.iter().find(|x| x.0 == t).map_or("Normal", |x| x.1)
}

fn mode_label(m: Mode) -> &'static str {
    MODES.iter().find(|x| x.0 == m).map_or("Normal", |x| x.1)
}

/// `cfgAddMenu`'s new menu: a quit command and the stock settings.
pub fn new_menu(name: &str, bbs: &str) -> Menu {
    let mut prompt_name = name.to_ascii_lowercase();
    if let Some(c) = prompt_name.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    Menu {
        menu_type: MenuType::Normal,
        title: format!("New {bbs} Menu"),
        prompt_name,
        help_file: name.to_string(),
        prompt: "Command: ".into(),
        acs: String::new(),
        password: String::new(),
        fallback: "MAIN".into(),
        expert: Mode::User,
        hot_keys: Mode::User,
        generic_columns: 3,
        clear_before: true,
        center_title: true,
        show_prompt: true,
        pause_before: true,
        global: true,
        upper_input: true,
        effects: Vec::new(),
        commands: vec![Command {
            keys: "Q".into(),
            command: "-\\".into(),
            param: "MAIN".into(),
            desc: "Quit".into(),
            help: "Quit current menu".into(),
            ..Command::default()
        }],
    }
}

/// Menu names in the directories, sorted, each once.
pub(crate) fn menu_names(dirs: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = dirs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|rd| {
            rd.filter_map(Result::ok).filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let (stem, ext) = name.rsplit_once('.')?;
                ext.eq_ignore_ascii_case("toml").then(|| stem.to_ascii_uppercase())
            })
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Where a menu lives: the existing file if any, else NAME.toml.
fn menu_path(dir: &Path, name: &str) -> PathBuf {
    let file = format!("{}.toml", name.to_ascii_uppercase());
    find_ci(dir, &file).unwrap_or_else(|| dir.join(file))
}

fn save_menu(dir: &Path, name: &str, m: &Menu) -> anyhow::Result<()> {
    let path = menu_path(dir, name);
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, m.to_toml())?;
    crate::keep_mode(&path, &tmp);
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

impl Session {
    /// `cfgMenuEditor`: pick a menu to add, delete or edit.
    pub async fn menu_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: menu editor", self.user.handle);
        let dir = self.state.config().resolve(&self.state.config().paths.menus);
        loop {
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(&format!("   {} ", self.state.config().bbs.name));
            self.set_slot(Slot::TextHi);
            self.write("Menu Editing System");
            self.newline();
            self.rule().await?;
            self.set_slot(Slot::Info);
            // Saves go to `menus`; a stock menu saved there overrides it.
            let names = menu_names(&self.state.config().menu_dirs());
            if names.is_empty() {
                self.write(" -- No menu files found.");
                self.newline();
            }
            for row in names.chunks(7) {
                for n in row {
                    self.write(&resize(n, 11));
                }
                self.newline();
            }
            self.rule().await?;
            let cmd = self.cfg_list_prompt(&[("A", "dd menu"), ("D", "elete menu"), ("E", "dit menu"), ("Esc", " Quit")]).await?;
            let Some(cmd) = cmd else { return Ok(()) };
            let what = match cmd {
                'A' => "new menu",
                'D' => "menu to delete",
                _ => "menu to edit",
            };
            self.write_with_controls(&format!("|U1Enter {what}'s name|U2: |U3"));
            let name = self.read_string(crate::input::ReadOpts::new(8).case(crate::input::Case::Upper).allowed("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-")).await?;
            self.newline();
            if name.is_empty() {
                continue;
            }
            let exists = names.contains(&name);
            match cmd {
                'A' => {
                    if exists {
                        self.show(&format!("|U0{name} already exists.|U1  Overwrite it? ")).await?;
                        if !self.yes_no(false).await? {
                            continue;
                        }
                    }
                    let m = new_menu(&name, &self.state.config().bbs.name);
                    save_menu(&dir, &name, &m).map_err(io_err)?;
                    tracing::info!("{}: created menu {name}", self.user.handle);
                    self.menu_edit(&dir, &name).await?;
                }
                'D' => {
                    if !exists {
                        continue;
                    }
                    let warn = if ["MAIN", "GLOBAL", "MATRIX", "STARTUP"].contains(&name.as_str()) { " |U0(the BBS relies on it)|U1" } else { "" };
                    self.show(&format!("|U3{name}{warn}|U1 - Delete this menu? ")).await?;
                    // Only the board's own copy goes; a stock one comes back.
                    let path = menu_path(&dir, &name);
                    if self.yes_no(false).await? && path.exists() {
                        std::fs::remove_file(path)?;
                        tracing::info!("{}: deleted menu {name}", self.user.handle);
                    }
                }
                _ => {
                    if exists {
                        self.menu_edit(&dir, &name).await?;
                    }
                }
            }
        }
    }

    /// `cfgEditMenu`: a menu's settings, and its commands with `*`.
    async fn menu_edit(&mut self, dir: &Path, name: &str) -> io::Result<()> {
        let original = Menu::load(&self.state.config().menu_dirs(), name).map_err(io_err)?;
        let mut m = original.clone();
        type Flag = (char, &'static str, &'static str, fn(&mut Menu) -> &mut bool);
        let flags: [Flag; 6] = [
            ('1', "Clear before", "Clear the screen before displaying this menu?", |m| &mut m.clear_before),
            ('2', "Center title", "Center the menu name when displaying a generic menu?", |m| &mut m.center_title),
            ('3', "Show prompt", "Display the menu prompt for this menu?", |m| &mut m.show_prompt),
            ('4', "Pause before", "Pause before displaying this menu?", |m| &mut m.pause_before),
            ('5', "Global use", "Include GLOBAL's commands in this menu?", |m| &mut m.global),
            ('6', "Upcase input", "Should menu input be converted to uppercase?", |m| &mut m.upper_input),
        ];
        let types: Vec<&str> = TYPES.iter().map(|t| t.1).collect();
        let modes: Vec<&str> = MODES.iter().map(|t| t.1).collect();
        loop {
            let mut f = Form::new(&format!("{} Menu Editor [{name}]", self.state.config().bbs.name));
            f.value_col(25);
            f.item('A', "Menu type", 8, type_label(m.menu_type), "Menu interface type.");
            f.item('B', "Menu title", 54, &m.title, "Title of this menu (displayed in generic menus).");
            f.item('C', "Prompt name", 54, &m.prompt_name, "Name of menu that will be displayed in prompt (with %MP code).");
            f.item('D', "Help filename", 8, &m.help_file, "Text file displayed as the menu.");
            f.item('E', "Menu prompt", 53, &m.prompt, "Prompt displayed for this menu (%%NAME shows a text file).");
            f.item('F', "Access", 20, &m.acs, "Access condition required to enter this menu.");
            f.item('G', "Password", 20, &m.password, "Password required to access this menu.");
            f.item('H', "Fallback menu", 8, &m.fallback, "Menu to drop to if this one can't be entered.");
            f.item('I', "Expert status", 8, mode_label(m.expert), "Menu expert mode status.");
            f.item('J', "Generic columns", 1, m.generic_columns.to_string(), "Number of command columns (when displaying the generic menu).");
            f.item('K', "Hotkey status", 8, mode_label(m.hot_keys), "Menu hotkey status.");
            f.gap(1);
            f.item('*', &format!("Edit menu commands ({})", m.commands.len()), 0, "", "");
            // The flags sit in a second column beside B to G.
            for (i, (key, label, info, get)) in flags.iter().enumerate() {
                f.item_xy(46, 2 + i as u16, 66, *key, label, 3, yes_no(*get(&mut m)), info);
            }
            self.cfg_draw(&f);
            let again = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break false };
                match key {
                    'A' => {
                        let cur = TYPES.iter().position(|t| t.0 == m.menu_type).unwrap_or(0);
                        m.menu_type = TYPES[self.cfg_option(&mut f, key, &types, cur).await?].0;
                    }
                    'I' | 'K' => {
                        let target = if key == 'I' { &mut m.expert } else { &mut m.hot_keys };
                        let cur = MODES.iter().position(|x| x.0 == *target).unwrap_or(0);
                        let pick = self.cfg_option(&mut f, key, &modes, cur).await?;
                        *if key == 'I' { &mut m.expert } else { &mut m.hot_keys } = MODES[pick].0;
                    }
                    'J' => {
                        if let Some(v) = self.cfg_number(&mut f, key, 1, 9).await? {
                            m.generic_columns = v as u8;
                        }
                    }
                    'B' | 'C' | 'E' => {
                        let max = if key == 'E' { 200 } else { 70 };
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(max)).await? {
                            *match key {
                                'B' => &mut m.title,
                                'C' => &mut m.prompt_name,
                                _ => &mut m.prompt,
                            } = v;
                        }
                    }
                    'D' | 'H' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::KEY.max(8)).await? {
                            *if key == 'D' { &mut m.help_file } else { &mut m.fallback } = v;
                        }
                    }
                    'F' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::LOWER).await? {
                            m.acs = v;
                        }
                    }
                    'G' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::UPPER).await? {
                            m.password = v;
                        }
                    }
                    '*' => {
                        self.menu_commands(name, &mut m.commands).await?;
                        break true;
                    }
                    k => {
                        if let Some((_, _, _, get)) = flags.iter().find(|fl| fl.0 == k) {
                            let cur = *get(&mut m);
                            *get(&mut m) = self.cfg_bool(&mut f, k, cur).await?;
                        }
                    }
                }
            };
            if again {
                continue;
            }
            self.screen.goto(1, self.screen.rows());
            self.newline();
            if m.to_toml() != original.to_toml() {
                self.show(&format!("|U1Save |U3{name}|U1? ")).await?;
                if self.yes_no(true).await? {
                    save_menu(dir, name, &m).map_err(io_err)?;
                    tracing::info!("{}: saved menu {name}", self.user.handle);
                }
            }
            return Ok(());
        }
    }

    /// `cfgEditMenuCommands`: the command list.
    async fn menu_commands(&mut self, name: &str, cmds: &mut Vec<Command>) -> io::Result<()> {
        loop {
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(&format!(" Num  Keys      Cmd  Param           Description        [{name}]"));
            self.newline();
            self.rule().await?;
            let mut lines = 3;
            for (i, c) in cmds.iter().enumerate() {
                self.set_slot(if c.hidden { Slot::InfoLo } else { Slot::Info });
                self.write(&format!(
                    " {} {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&c.keys, 9),
                    resize(&c.command, 4),
                    resize(&c.param, 15),
                    squish(&c.desc, 38)
                ));
                self.newline();
                lines += 1;
                if !self.more(&mut lines).await? {
                    break;
                }
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Done")]).await? else {
                return Ok(());
            };
            if cmd == 'E' {
                self.newline();
                let n = cmds.len() as u32;
                if n > 0
                    && let Some(num) = self.cfg_ask_number("Begin edit with which command?", 1, n).await?
                {
                    self.menu_command_edit(cmds, num).await?;
                }
                continue;
            }
            // `cfgNewCommand`.
            let fresh = |_: &[Command]| Command { keys: "X".into(), desc: "New menu command".into(), help: "None".into(), ..Command::default() };
            self.list_change(cmds, cmd, "command", fresh).await?;
        }
    }

    /// `cfgEditMenuCommand`.
    async fn menu_command_edit(&mut self, cmds: &mut [Command], mut num: u32) -> io::Result<()> {
        type Text = (char, &'static str, usize, usize, &'static str, fn(&mut Command) -> &mut String);
        let texts: [Text; 8] = [
            ('A', "Description", 35, 60, "Description shown in generic menus and help.", |c| &mut c.desc),
            ('B', "Command help", 54, 120, "Help text shown with /? (command help).", |c| &mut c.help),
            ('C', "Menu keys", 14, 40, "Key(s) that run it: X, /X, //WORD, or FIRSTCMD to run on entry.", |c| &mut c.keys),
            ('D', "Access", 20, 40, "Access condition required to use this command.", |c| &mut c.acs),
            ('E', "Menu command", 2, 2, "Two-character command code (e.g. -/ to go to a menu).", |c| &mut c.command),
            ('F', "Parameters", 54, 200, "The command's parameters.", |c| &mut c.param),
            ('H', "Lightbar on", 54, 200, "Lightbar text when highlighted (|B1 codes etc).", |c| &mut c.on),
            ('I', "Lightbar off", 54, 200, "Lightbar text when not highlighted.", |c| &mut c.off),
        ];
        let n = cmds.len() as u32;
        loop {
            let idx = num as usize - 1;
            let c = &mut cmds[idx];
            let mut f = Form::new(&format!("{} Menu Command Editor", self.state.config().bbs.name));
            f.value_col(25);
            f.label("Current command", 12, format!("{num} of {n}"));
            for (key, label, len, _, info, get) in &texts[..6] {
                f.item(*key, label, *len, get(c).clone(), info);
            }
            f.item('G', "Hidden", 3, yes_no(c.hidden), "Leave this command out of generic menus and help?");
            for (key, label, len, _, info, get) in &texts[6..] {
                f.item(*key, label, *len, get(c).clone(), info);
            }
            f.item('[', "Previous", 0, "", "");
            f.item(']', "Next", 0, "", "");
            self.cfg_draw(&f);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                let c = &mut cmds[idx];
                match key {
                    'G' => c.hidden = self.cfg_bool(&mut f, key, c.hidden).await?,
                    '[' => break -1,
                    ']' => break 1,
                    k => {
                        let Some((_, _, _, max, _, get)) = texts.iter().find(|t| t.0 == k) else { continue };
                        let field = if k == 'E' { Field::UPPER.max(2) } else { Field::TEXT.max(*max) };
                        if let Some(v) = self.cfg_text(&mut f, k, field).await? {
                            *get(c) = v;
                        }
                    }
                }
            };
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
    fn new_menus_round_trip() {
        let m = new_menu("FILES2", "Iniquity");
        assert_eq!(m.prompt_name, "Files2");
        let back: Menu = toml::from_str(&m.to_toml()).unwrap();
        assert_eq!(back.to_toml(), m.to_toml());
        assert_eq!(back.commands[0].command, "-\\");
    }

    #[test]
    fn lists_menu_files() {
        let dir = std::env::temp_dir().join(format!("iqmenus{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["main.toml", "FILE.toml", "notes.txt"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        assert_eq!(menu_names(std::slice::from_ref(&dir)), ["FILE", "MAIN"]);
        assert_eq!(menu_path(&dir, "MAIN"), dir.join("main.toml"), "keeps the existing file's case");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
