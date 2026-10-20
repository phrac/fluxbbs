//! The menu engine (`Menus.pas`): load and enter menus, display them,
//! read a command, and run every matching command in order.

use std::io;

use iniquity_term::Slot;
use iniquity_term::codes::squish;

use crate::input::{Key, ReadOpts};
use crate::menu::{FIRST_CMD, Menu, MenuType, Mode};
use crate::session::{Session, pos_code};

/// Most menus a gosub (`-/`) chain can nest.
const MAX_STACK: usize = 8;
/// Lightbar help line marker (`maxPos-1`) and exit position (`maxPos`).
const LIGHTBAR_HELP: [u8; 2] = *b"HP";
const LIGHTBAR_EXIT: u8 = 30;

#[derive(Default)]
pub struct MenuState {
    pub current: String,
    pub last: String,
    stack: Vec<String>,
    pub menu: Option<Menu>,
    /// Command keys to run next without asking (`NewMenuCmd`).
    pub next_cmd: String,
    /// Run `next_cmd` on the next pass (`ExecFirst`).
    pub exec_first: bool,
    do_help: bool,
    pub(crate) last_cmd_good: bool,
    /// The last command ended in a full-screen view the caller quit
    /// (lister, viewer, editor): no "press any key" before the menu. New;
    /// the original paused after every command.
    pub(crate) skip_pause: bool,
    /// Set by `-^ -/ -\` to stop running stacked commands (`Done`).
    pub(crate) done: bool,
    /// A menu change is pending (`NewMnuToLoad`).
    pub(crate) reload: bool,
    /// The running command's description (a door's name in the status).
    pub(crate) cmd_desc: String,
}

impl MenuState {
    fn commands(&self) -> &[crate::menu::Command] {
        self.menu.as_ref().map_or(&[], |m| &m.commands)
    }
}

impl Session {
    fn menu_dirs(&self) -> Vec<std::path::PathBuf> {
        self.state.config().menu_dirs()
    }

    /// `menuLoad`: the menu plus GLOBAL's commands if it uses them.
    fn menu_load(&self, name: &str) -> Option<Menu> {
        let dir = self.menu_dirs();
        let mut menu = Menu::load(&dir, name).map_err(|e| tracing::warn!("{e}")).ok()?;
        crate::logonseq::apply(&self.state.config(), &dir, name, &mut menu);
        if menu.global && !name.eq_ignore_ascii_case("GLOBAL")
            && let Ok(global) = Menu::load(&dir, "GLOBAL") {
                menu.commands.extend(global.commands);
            }
        Some(menu)
    }

    /// `menuRead`: load the current menu, falling back if it's missing.
    async fn menu_read(&mut self) -> io::Result<()> {
        if let Some(m) = self.menu_load(&self.menu.current) {
            self.menu.menu = Some(m);
            return Ok(());
        }
        let missing = self.menu.current.clone();
        self.show_line(&format!("|U0Error loading \"{missing}\"")).await?;
        self.show_line("|U1Attempting to load fallback menu ...").await?;
        let fallback = self
            .menu
            .menu
            .as_ref()
            .map(|m| m.fallback.clone())
            .filter(|f| !f.is_empty() && !f.eq_ignore_ascii_case(&missing))
            .unwrap_or_else(|| self.state.config().logon.start_menu.clone());
        self.menu.current = fallback;
        match self.menu_load(&self.menu.current) {
            Some(m) => self.menu.menu = Some(m),
            None => {
                self.show_line("|U0Error loading fallback menu.  Please inform SysOp").await?;
                self.hangup = true;
            }
        }
        Ok(())
    }

    /// `menuInstall`: enter the current menu, checking access/password.
    pub async fn menu_install(&mut self) -> io::Result<()> {
        self.menu_read().await?;
        let Some(menu) = self.menu.menu.clone() else { return Ok(()) };
        if !self.acs(&menu.acs) || !menu.password.is_empty() {
            let mut denied = true;
            if !menu.password.is_empty() {
                self.newline();
                self.say("MenuPassword").await?;
                let pw = self.read_string(ReadOpts::new(20).password()).await?;
                self.newline();
                denied = !pw.eq_ignore_ascii_case(&menu.password);
            }
            if denied {
                self.newline();
                self.say_line("MenuAccessDenied").await?;
                tracing::info!("{}: access to menu {} denied", self.user.handle, self.menu.current);
                self.menu.current = menu.fallback.clone();
                self.menu_read().await?;
            }
        }
        self.menu.do_help = false;
        self.check_expert();
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        Ok(())
    }

    /// A full-screen view the caller has just quit: there's nothing left
    /// to read, so the menu comes back without a pause.
    pub(crate) fn full_screen_done(&mut self) {
        self.menu.skip_pause = true;
    }

    /// After entering a menu, queue its FIRSTCMD commands.
    pub fn queue_first_cmd(&mut self) {
        let first = self.menu.commands().iter().any(|c| c.keys == FIRST_CMD && self.acs(&c.acs));
        if first && self.menu.next_cmd.is_empty() {
            self.menu.next_cmd = FIRST_CMD.into();
        }
    }

    fn check_expert(&mut self) {
        let Some(menu) = &self.menu.menu else { return };
        match menu.expert {
            Mode::Forced => self.menu.do_help = false,
            Mode::Disabled => self.menu.do_help = true,
            Mode::User => {
                if !self.menu.do_help {
                    self.menu.do_help = !self.user.expert;
                }
            }
        }
    }

    fn hot_keys(&self) -> bool {
        match self.menu.menu.as_ref().map(|m| m.hot_keys) {
            Some(Mode::Forced) => true,
            Some(Mode::Disabled) => false,
            _ => self.user.hot_keys,
        }
    }

    /// `menuExec`: one pass of the menu loop.
    pub async fn menu_exec(&mut self) -> io::Result<()> {
        let mut cmd = if self.menu.exec_first {
            self.menu.exec_first = false;
            std::mem::take(&mut self.menu.next_cmd)
        } else {
            self.menu_handle().await?
        };
        while !cmd.is_empty() && !self.hangup {
            self.menu.next_cmd.clear();
            self.menu_execute(&cmd).await?;
            cmd = std::mem::take(&mut self.menu.next_cmd);
        }
        Ok(())
    }

    /// `menuHandle`: show the menu if needed, prompt, and read a command.
    async fn menu_handle(&mut self) -> io::Result<String> {
        if self.logged_in && self.minutes_left() == 0 {
            self.newline();
            self.say_line("TimeExpired").await?;
            self.hangup = true;
            return Ok(String::new());
        }
        let Some(menu) = self.menu.menu.clone() else {
            self.hangup = true;
            return Ok(String::new());
        };
        self.nonstop = false;
        self.check_expert();
        if menu.pause_before && self.menu.do_help && self.menu.last_cmd_good && !self.menu.skip_pause {
            self.prompt_key().await?;
        }
        self.menu.last_cmd_good = false;
        self.menu.skip_pause = false;
        let lightbar = menu.menu_type == MenuType::Lightbar && self.text_file_exists(&menu.help_file);
        if self.menu.do_help && !lightbar {
            self.menu_show(&menu).await?;
        }
        if !lightbar && menu.show_prompt {
            self.newline();
            self.show(&menu.prompt).await?;
        }
        let cmd = if lightbar { self.menu_lightbar(&menu).await? } else { self.menu_get_cmd(&menu).await? };
        if cmd == "?" {
            self.menu.do_help = true;
            return Ok(String::new());
        }
        self.check_expert();
        Ok(cmd)
    }

    /// `menuShow`.
    async fn menu_show(&mut self, menu: &Menu) -> io::Result<()> {
        match menu.menu_type {
            MenuType::Normal | MenuType::Lightbar => {
                if !self.show_file(&menu.help_file).await? {
                    self.menu_generic(menu).await?;
                }
            }
            MenuType::Generic => self.menu_generic(menu).await?,
        }
        self.menu.do_help = false;
        Ok(())
    }

    /// `menuGeneric`: build a menu screen from the command list.
    async fn menu_generic(&mut self, menu: &Menu) -> io::Result<()> {
        let cols = menu.generic_columns.clamp(1, 7) as usize;
        let width = [79, 39, 25, 19, 16, 12, 11][cols - 1];
        let mut lines: Vec<String> = vec![String::new()];
        let mut on_line = 0;
        let mut max_right = 0;
        for c in &menu.commands {
            if c.hidden || c.desc.is_empty() || c.keys == FIRST_CMD || !self.acs(&c.acs) {
                continue;
            }
            on_line += 1;
            let mut item = format!("|U2[|U3{}|U2] |U1{}", c.keys, c.desc);
            if on_line != cols {
                item = self.pad_visible(&item, width);
            }
            lines.last_mut().unwrap().push_str(&item);
            if on_line == cols {
                max_right = max_right.max(self.visible_len(&iniquity_term::cp437::encode(lines.last().unwrap())));
                lines.push(String::new());
                on_line = 0;
            }
        }
        if menu.clear_before {
            self.clear_screen();
        }
        self.set_slot(Slot::Text);
        self.newline();
        if !menu.title.is_empty() {
            let title = if menu.center_title {
                let len = self.visible_len(&iniquity_term::cp437::encode(&menu.title));
                let max_right = max_right.max(width * cols);
                format!("{}{}", " ".repeat((max_right / 2).saturating_sub(len / 2)), menu.title)
            } else {
                menu.title.clone()
            };
            self.show_line(&title).await?;
            self.newline();
        }
        for line in lines.iter().filter(|l| !l.is_empty()) {
            self.show_line(line).await?;
        }
        Ok(())
    }

    /// Pad text containing codes to a visible width.
    pub fn pad_visible(&self, s: &str, width: usize) -> String {
        let len = self.visible_len(&iniquity_term::cp437::encode(s));
        format!("{s}{}", " ".repeat(width.saturating_sub(len)))
    }

    /// `menuGetCmd`: read a command. With hot keys, single-key commands
    /// run on the keypress; `/` starts a two-key command and `//` a typed
    /// one ended by Enter.
    async fn menu_get_cmd(&mut self, menu: &Menu) -> io::Result<String> {
        let sysop = self.is_sysop();
        let mut single = String::from("?");
        let mut slash = String::new();
        let mut has_slash = false;
        let mut has_long = false;
        // A `#` command takes a number typed at the prompt (`M#`, `F#`).
        let mut has_number = false;
        for c in &menu.commands {
            if !self.acs(&c.acs) {
                continue;
            }
            let k: Vec<char> = c.keys.chars().collect();
            match k.as_slice() {
                ['#'] => has_number = true,
                [one] => single.push(one.to_ascii_uppercase()),
                ['/', two] => {
                    has_slash = true;
                    slash.push(two.to_ascii_uppercase());
                }
                _ => has_long = true,
            }
        }

        let cmd = if !self.hot_keys() {
            let s = self.read_string(ReadOpts::new(60)).await?;
            s.to_uppercase()
        } else {
            let mut ss = String::new();
            loop {
                let key = self.read_key().await?;
                let before = ss.clone();
                let mut got = false;
                let c = match key {
                    Key::Char(c) => Some(if menu.upper_input { c.to_ascii_uppercase() } else { c } as char),
                    _ => None,
                };
                if ss.is_empty() {
                    if key == Key::Enter {
                        got = true;
                    } else if c == Some('/') && (has_slash || has_long || sysop) {
                        ss.push('/');
                    } else if let Some(c) = c.filter(|c| single.contains(c.to_ascii_uppercase())) {
                        ss.push(c);
                        got = true;
                    } else if let Some(c) = c.filter(|c| has_number && c.is_ascii_digit()) {
                        ss.push(c);
                    }
                } else if ss == "/" {
                    if key == Key::Backspace {
                        ss.clear();
                    } else if c == Some('/') && (has_long || sysop) {
                        ss.push('/');
                    } else if let Some(c) = c.filter(|c| has_slash && slash.contains(c.to_ascii_uppercase())) {
                        ss.push(c);
                        got = true;
                    }
                } else {
                    match key {
                        Key::Enter => got = true,
                        Key::Backspace => {
                            ss.pop();
                        }
                        Key::Char(0x18) => ss.truncate(2),
                        // A number for `#`: digits only.
                        Key::Char(c) if ss.starts_with(|f: char| f.is_ascii_digit()) => {
                            if c.is_ascii_digit() && ss.len() < 5 {
                                ss.push(c as char);
                            }
                        }
                        Key::Char(c @ 0x20..=0x7E) if ss.len() < 62 => ss.push(c as char),
                        _ => {}
                    }
                }
                if ss.len() > before.len() {
                    let last = ss.chars().last().unwrap();
                    self.write(&last.to_string());
                } else if ss.len() < before.len() {
                    self.screen.backspace((before.len() - ss.len()) as u16);
                }
                if got || self.hangup {
                    break;
                }
            }
            ss.strip_prefix("//").map(str::to_string).unwrap_or(ss).to_uppercase()
        };
        self.newline();
        // "cmd;more" type-ahead isn't supported; run the first part.
        Ok(cmd.split(';').next().unwrap_or("").to_string())
    }

    /// `menuDoLightBarMenu`: pick a command with the cursor keys over the
    /// `~01`.. positions marked in the menu's help file.
    async fn menu_lightbar(&mut self, menu: &Menu) -> io::Result<String> {
        if menu.clear_before {
            self.clear_screen();
        }
        self.show_file(&menu.help_file).await?;
        let items: Vec<usize> = menu
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| c.keys != FIRST_CMD && !c.hidden && self.acs(&c.acs))
            .map(|(i, _)| i)
            .take(24)
            .collect();
        if items.is_empty() {
            return Ok("?".into());
        }
        let has_help = self.positions.contains_key(&LIGHTBAR_HELP);
        let mut help_len = 0;
        for n in 1..=items.len() {
            self.lightbar_item(menu, &items, n, false, has_help, &mut help_len).await?;
        }
        let mut cur = 1;
        self.lightbar_item(menu, &items, cur, true, has_help, &mut help_len).await?;
        loop {
            let key = self.read_key().await?;
            let next = match key {
                Key::Up | Key::Left | Key::Char(b'8' | b'4') => Some(if cur == 1 { items.len() } else { cur - 1 }),
                Key::Down | Key::Right | Key::Char(b'6' | b'2' | b' ') => Some(if cur == items.len() { 1 } else { cur + 1 }),
                Key::Home | Key::PgUp | Key::Char(b'7' | b'9') => Some(1),
                Key::End | Key::PgDn | Key::Char(b'1' | b'3') => Some(items.len()),
                Key::Enter => break,
                _ => None,
            };
            if let Some(next) = next.filter(|&n| n != cur) {
                self.lightbar_item(menu, &items, cur, false, has_help, &mut help_len).await?;
                cur = next;
                self.lightbar_item(menu, &items, cur, true, has_help, &mut help_len).await?;
            }
        }
        // Leave the highlight colour behind, and don't scroll the art away.
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        if !self.goto_pos(pos_code(LIGHTBAR_EXIT)) {
            self.screen.goto(1, self.screen.rows());
        }
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        Ok(menu.commands[items[cur - 1]].keys.to_uppercase())
    }

    async fn lightbar_item(
        &mut self,
        menu: &Menu,
        items: &[usize],
        n: usize,
        hi: bool,
        has_help: bool,
        help_len: &mut usize,
    ) -> io::Result<()> {
        let c = &menu.commands[items[n - 1]];
        let ok = if hi { self.goto_pos(pos_code(n as u8)) } else { self.goto_pos2(n as u8) };
        if !ok {
            return Ok(());
        }
        let text = match (hi, c.on.is_empty(), c.off.is_empty()) {
            (true, false, _) => &c.on,
            (false, _, false) => &c.off,
            _ => &c.desc,
        };
        self.show(&text.clone()).await?;
        if hi && has_help {
            self.goto_pos(LIGHTBAR_HELP);
            self.write(&" ".repeat(*help_len));
            self.goto_pos(LIGHTBAR_HELP);
            let help = c.help.clone();
            self.show(&help).await?;
            *help_len = self.visible_len(&iniquity_term::cp437::encode(&help));
        }
        Ok(())
    }

    /// `menuExecute`: run every command bound to `keys` the user may use,
    /// in order, until one changes menu.
    async fn menu_execute(&mut self, keys: &str) -> io::Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let commands = self.menu.commands().to_vec();
        let mut exists = false;
        let mut visible = false;
        let mut allowed = false;
        self.menu.done = false;
        let number = keys.chars().all(|c| c.is_ascii_digit());
        for c in &commands {
            let by_number = number && c.keys == "#";
            if !c.keys.eq_ignore_ascii_case(keys) && !by_number {
                continue;
            }
            exists = true;
            visible |= !c.hidden;
            if !self.acs(&c.acs) {
                continue;
            }
            allowed = true;
            self.menu.cmd_desc = crate::editor::strip_codes(&c.desc);
            // The number typed is the `#` command's parameter.
            self.run_command(&c.command, if by_number { keys } else { &c.param }).await?;
            if self.menu.done || self.hangup {
                break;
            }
        }
        if !allowed {
            self.newline();
            if exists && visible {
                self.say_line("CmdAccessDenied").await?;
            } else {
                self.say_line("InvalidCommand").await?;
            }
        }
        Ok(())
    }

    /// `menuChange`: `-^` goto, `-/` gosub, `-\` return. The parameter is
    /// `MENU[;C][;KEYS]`: `C` clears the gosub stack, KEYS run on arrival.
    pub(crate) fn menu_change(&mut self, kind: char, param: &str) {
        let mut parts = param.split(';');
        let target = parts.next().unwrap_or("").trim().to_string();
        let mut rest: Vec<&str> = parts.collect();
        if rest.first().is_some_and(|s| s.eq_ignore_ascii_case("C")) {
            self.menu.stack.clear();
            rest.remove(0);
        }
        let then = rest.join(";").to_uppercase();
        match kind {
            '^' | '/' => {
                if target.is_empty() {
                    return;
                }
                if kind == '/' && self.menu.stack.len() < MAX_STACK {
                    self.menu.stack.push(self.menu.current.clone());
                }
                self.menu.last = std::mem::replace(&mut self.menu.current, target);
            }
            _ => {
                if let Some(prev) = self.menu.stack.pop() {
                    self.menu.last = std::mem::replace(&mut self.menu.current, prev);
                }
            }
        }
        self.menu.done = true;
        self.menu.reload = true;
        if !then.is_empty() {
            self.menu.next_cmd = then;
        }
    }

    /// `menuCmdHelp` (`-?`): list the commands with their help text.
    pub(crate) async fn menu_cmd_help(&mut self) -> io::Result<()> {
        let commands = self.menu.commands().to_vec();
        self.clear_screen();
        self.newline();
        self.set_slot(Slot::Info);
        self.show_line(" Key   Description").await?;
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(50));
        self.newline();
        self.set_slot(Slot::Text);
        let mut lines = 3;
        for c in commands.iter().filter(|c| !c.hidden && c.keys != FIRST_CMD) {
            if !self.acs(&c.acs) {
                continue;
            }
            let line = format!(" |U3{} |U1{}", iniquity_term::codes::resize(&c.keys, 5), squish(&c.help, 72));
            self.show_line(&line).await?;
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(50));
        self.newline();
        Ok(())
    }

    /// Page-length pausing for listings (`oUpPause`). Returns false if the
    /// user asked to stop.
    pub async fn more(&mut self, lines: &mut u16) -> io::Result<bool> {
        if self.nonstop || !self.user.pause || *lines < self.user.page_length.saturating_sub(1).max(5) {
            return Ok(true);
        }
        *lines = 0;
        let s = self.strings().get("Pause").to_string();
        let start = self.screen.where_x();
        let color = self.screen.color();
        self.show(&s).await?;
        let key = self.read_key().await?;
        let len = self.screen.where_x().saturating_sub(start);
        self.screen.backspace(len);
        self.screen.set_color(color);
        self.nonstop = key.upper() == Some('C');
        Ok(!matches!(key.upper(), Some('Q' | 'S' | 'N')) && key != Key::Esc)
    }
}
