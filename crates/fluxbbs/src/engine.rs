//! The menu engine (`Menus.pas`): load and enter menus, display them,
//! read a command, and run every matching command in order.

use std::io;

use fluxbbs_term::Slot;
use fluxbbs_term::codes::squish;

use crate::input::{Key, ReadOpts};
use crate::menu::{FIRST_CMD, Menu, MenuType, Mode};
use crate::session::{Session, pos_code};

/// Most menus a gosub (`-/`) chain can nest.
const MAX_STACK: usize = 8;
/// Lightbar help line marker (`maxPos-1`) and exit position (`maxPos`).
const LIGHTBAR_HELP: [u8; 2] = *b"HP";
/// New: where the cursor waits on a lightbar menu (a prompt bar's slot).
const LIGHTBAR_CURSOR: [u8; 2] = *b"PC";
const LIGHTBAR_EXIT: u8 = 30;
/// Options on a lightbar, short of the exit position (the original took 24).
const MAX_BAR: usize = 29;

/// New: where an arrow goes on a lightbar, by where the options are
/// (`places`, 0-based; `None` where an option has no marker): left and
/// right along the row, up and down to the nearest option below or above
/// (rows count more than columns). Where there's nothing that way, it
/// steps through them in order.
/// How many columns a lightbar option's text takes: its characters
/// less its escape sequences and pipe codes.
fn ansi_width(s: &str) -> usize {
    let mut plain = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        plain.push(c);
    }
    crate::editor::strip_codes(&plain).chars().count()
}

fn bar_move(places: &[Option<(i32, i32)>], cur: usize, key: Key) -> usize {
    let n = places.len();
    let (next, prev) = ((cur + 1) % n, (cur + n - 1) % n);
    let Some((cx, cy)) = places[cur] else {
        return if matches!(key, Key::Up | Key::Left) { prev } else { next };
    };
    let others = places.iter().enumerate().filter_map(|(i, p)| p.map(|(x, y)| (i, x, y)));
    let found = match key {
        Key::Right => others.filter(|&(_, x, y)| y == cy && x > cx).min_by_key(|&(_, x, _)| x),
        Key::Left => others.filter(|&(_, x, y)| y == cy && x < cx).max_by_key(|&(_, x, _)| x),
        // Down and up weigh sideways distance too, so the option below
        // wins over one a row nearer but across the screen.
        Key::Down => others.filter(|&(_, _, y)| y > cy).min_by_key(|&(_, x, y)| (y - cy) * 4 + (x - cx).abs() / 2),
        _ => others.filter(|&(_, _, y)| y < cy).min_by_key(|&(_, x, y)| (cy - y) * 4 + (x - cx).abs() / 2),
    };
    match found {
        Some((i, _, _)) => i,
        None if matches!(key, Key::Up | Key::Left) => prev,
        None => next,
    }
}

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
    /// New: the menu last listed in plain text.
    pub(crate) plain_listed: String,
    /// New: the screen's line count when the last command began.
    lines_mark: u64,
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
        // New: the entries of parts that are switched off ([features])
        // go dark, as does the way into a menu with nothing left on (a
        // game's own menu).
        let cfg = self.state.config();
        for c in &mut menu.commands {
            c.dark = !crate::features::command_on(&cfg, &c.command) || ((c.command == "-/" || c.command == "-^") && Self::menu_dark(&cfg, &dir, &c.param));
        }
        Some(menu)
    }

    /// Does a menu have real commands (not screens, pauses or menu
    /// changes) and all of them switched off?
    fn menu_dark(cfg: &crate::config::Config, dir: &[std::path::PathBuf], name: &str) -> bool {
        let Ok(m) = Menu::load(dir, name.trim()) else { return false };
        let real: Vec<&crate::menu::Command> = m.commands.iter().filter(|c| !c.command.starts_with('-')).collect();
        !real.is_empty() && real.iter().all(|c| !crate::features::command_on(cfg, &c.command))
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

    /// New: a line to tell the caller, shown on the help line of the list
    /// or menu they go back to (until their next key) rather than with a
    /// "press any key" stop; plain text has it at once.
    pub(crate) async fn note(&mut self, s: String) -> io::Result<()> {
        if self.plain() {
            return self.show_line(&s).await;
        }
        self.note = Some(s);
        self.full_screen_done();
        Ok(())
    }

    /// `note` with a prompt string by name.
    pub(crate) async fn note_say(&mut self, name: &str) -> io::Result<()> {
        let s = self.strings().get(name).to_string();
        self.note(s).await
    }

    /// After entering a menu, queue its FIRSTCMD commands.
    pub fn queue_first_cmd(&mut self) {
        let first = self.menu.commands().iter().any(|c| c.keys == FIRST_CMD && !c.dark && self.acs(&c.acs));
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
        self.menu.lines_mark = self.screen.mark_lines();
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
        let lightbar = menu.menu_type == MenuType::Lightbar && self.text_file_exists(&menu.help_file);
        // New: a lightbar menu is drawn again even in expert mode, over
        // what the last command showed, so that waits for a key first;
        // and only after a command that showed something worth reading
        // (a line or more: not a menu change, nor a list or view the
        // caller has just left).
        let showed = self.screen.text_lines() > self.menu.lines_mark;
        if menu.pause_before && (self.menu.do_help || lightbar) && self.menu.last_cmd_good && !self.menu.skip_pause && showed {
            self.prompt_key().await?;
        }
        self.menu.last_cmd_good = false;
        self.menu.skip_pause = false;
        // New: plain text lists a menu when it's entered or asked for.
        if self.plain() {
            let list = self.menu.plain_listed != self.menu.current;
            if list {
                self.menu.plain_listed = self.menu.current.clone();
            }
            let cmd = self.plain_menu(&menu, list).await?;
            if cmd == "?" {
                self.menu.plain_listed.clear();
                return Ok(String::new());
            }
            return Ok(cmd);
        }
        if self.menu.do_help && !lightbar {
            self.menu_show(&menu).await?;
        }
        if !lightbar && let Some(n) = self.note.take() {
            self.newline();
            self.show_line(&n).await?;
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
            if c.hidden || c.dark || c.desc.is_empty() || c.keys == FIRST_CMD || !self.acs(&c.acs) {
                continue;
            }
            on_line += 1;
            let mut item = format!("|U2[|U3{}|U2] |U1{}", c.keys, c.desc);
            if on_line != cols {
                item = self.pad_visible(&item, width);
            }
            lines.last_mut().unwrap().push_str(&item);
            if on_line == cols {
                max_right = max_right.max(self.visible_len(&fluxbbs_term::cp437::encode(lines.last().unwrap())));
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
                let len = self.visible_len(&fluxbbs_term::cp437::encode(&menu.title));
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
        let len = self.visible_len(&fluxbbs_term::cp437::encode(s));
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
    /// `~01`.. positions marked in the menu's help file. New: a command's
    /// own keys still run it (hot keys, `/X`, a number for `#`), and the
    /// arrows go by where the options are on screen, so a grid of them
    /// works as well as a row.
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
            .take(MAX_BAR)
            .collect();
        if items.is_empty() {
            return Ok("?".into());
        }
        let has_help = self.positions.contains_key(&LIGHTBAR_HELP);
        let mut help_len = 0;
        for n in 1..=items.len() {
            self.lightbar_item(menu, &items, n, false, has_help, &mut help_len).await?;
        }
        // A dark slot (its part switched off) keeps its place but has no
        // position to move to, so the bar passes it by.
        let lit = |n: usize| !menu.commands[items[n - 1]].dark;
        let places: Vec<Option<(i32, i32)>> = (1..=items.len())
            .map(|n| if lit(n) { self.positions.get(&pos_code(n as u8)).map(|p| (p.0 as i32, p.1 as i32)) } else { None })
            .collect();
        let first_lit = (1..=items.len()).find(|&n| lit(n)).unwrap_or(1);
        let last_lit = (1..=items.len()).rev().find(|&n| lit(n)).unwrap_or(1);
        let after = |cur: usize| {
            let mut n = cur;
            for _ in 0..items.len() {
                n = if n == items.len() { 1 } else { n + 1 };
                if lit(n) {
                    break;
                }
            }
            n
        };
        let keys: Vec<String> = menu.commands.iter().filter(|c| c.keys != FIRST_CMD && !c.dark && self.acs(&c.acs)).map(|c| c.keys.to_uppercase()).collect();
        let allowed = |k: &str| keys.iter().any(|c| c == k);
        // With a `#` command, digits are a number rather than arrows.
        let has_number = allowed("#");
        let mut cur = first_lit;
        self.lightbar_item(menu, &items, cur, true, has_help, &mut help_len).await?;
        // New: a note (`Session::note`), else a hint for this caller, in
        // the help line till they move.
        // No help line: a note goes on the bottom row instead.
        if !has_help && let Some(n) = self.note.take() {
            self.screen.goto(1, self.screen.rows());
            self.show(&n).await?;
        }
        let hint = if has_help { self.note.take().or_else(|| self.caller_hint(menu)) } else { None };
        if has_help && let Some(hint) = hint {
            self.goto_pos(LIGHTBAR_HELP);
            self.write(&" ".repeat(help_len));
            self.goto_pos(LIGHTBAR_HELP);
            let room = 80usize.saturating_sub(self.screen.where_x() as usize);
            let hint = squish(&hint, room);
            help_len = self.visible_len(&fluxbbs_term::cp437::encode(&hint)).min(room);
            self.show(&hint).await?;
            self.goto_pos(LIGHTBAR_CURSOR);
        }
        // New: the screen animates while it waits, if the menu says how.
        let mut ambient = if !menu.effects.is_empty() && menu.clear_before && self.animating() {
            crate::ambient::Ambient::new(&self.screen, &menu.effects)
        } else {
            None
        };
        let picked = loop {
            // After the screen saver, the menu's drawn again.
            let saver = menu.clear_before && self.animating();
            let Some(key) = self.read_key_ambient(&mut ambient, saver).await? else { break "?".to_string() };
            // New: Backspace brings the screen saver on at once (where
            // it can run); the menu's drawn again after.
            if key == Key::Backspace && saver && self.screensaver_now().await? {
                break "?".to_string();
            }
            if let Some(c) = key.upper().filter(|c| c.is_ascii_graphic()) {
                let k = c.to_string();
                if allowed(&k) {
                    break k;
                }
                if c == '/' {
                    if let Some(two) = self.read_key().await?.upper() {
                        let k = format!("/{two}");
                        if allowed(&k) {
                            break k;
                        }
                    }
                    continue;
                }
                if has_number && c.is_ascii_digit() {
                    break self.lightbar_number(&k, has_help, help_len).await?;
                }
            }
            let next = match key {
                Key::Up | Key::Down | Key::Left | Key::Right => Some(bar_move(&places, cur - 1, key) + 1),
                Key::Char(b'8') if !has_number => Some(bar_move(&places, cur - 1, Key::Up) + 1),
                Key::Char(b'2') if !has_number => Some(bar_move(&places, cur - 1, Key::Down) + 1),
                Key::Char(b'4') if !has_number => Some(bar_move(&places, cur - 1, Key::Left) + 1),
                Key::Char(b'6') if !has_number => Some(bar_move(&places, cur - 1, Key::Right) + 1),
                Key::Char(b' ') => Some(after(cur)),
                Key::Home | Key::PgUp => Some(first_lit),
                Key::Char(b'7' | b'9') if !has_number => Some(first_lit),
                Key::End | Key::PgDn => Some(last_lit),
                Key::Char(b'1' | b'3') if !has_number => Some(last_lit),
                Key::Enter => {
                    let k = menu.commands[items[cur - 1]].keys.to_uppercase();
                    if k == "#" {
                        break self.lightbar_number("", has_help, help_len).await?;
                    }
                    break k;
                }
                _ => None,
            };
            if let Some(next) = next.filter(|&n| n != cur) {
                self.lightbar_item(menu, &items, cur, false, has_help, &mut help_len).await?;
                cur = next;
                self.lightbar_item(menu, &items, cur, true, has_help, &mut help_len).await?;
            }
        };
        // Leave the highlight colour behind, and don't scroll the art away.
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        if !self.goto_pos(pos_code(LIGHTBAR_EXIT)) {
            self.screen.goto(1, self.screen.rows());
        }
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        Ok(picked)
    }

    /// A number for a lightbar menu's `#` command, typed on the help line
    /// (or the bottom row), starting with `first`.
    async fn lightbar_number(&mut self, first: &str, has_help: bool, help_len: usize) -> io::Result<String> {
        if has_help {
            self.goto_pos(LIGHTBAR_HELP);
            self.write(&" ".repeat(help_len));
            self.goto_pos(LIGHTBAR_HELP);
        } else {
            self.screen.goto(1, self.screen.rows());
            self.screen.clear_eol();
        }
        self.say("LightbarNumber").await?;
        self.read_string(ReadOpts::new(5).allowed(crate::input::DIGITS).prefill(first)).await
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
        // A dark slot: blank, as wide as the option would be.
        let text = if c.dark { " ".repeat(ansi_width(text).max(1)) } else { text.clone() };
        self.show(&text).await?;
        if hi && has_help {
            self.goto_pos(LIGHTBAR_HELP);
            self.write(&" ".repeat(*help_len));
            self.goto_pos(LIGHTBAR_HELP);
            let help = c.help.clone();
            // How far it went, as written (values cut by `{NN` and all).
            let (x, y) = (self.screen.where_x(), self.screen.where_y());
            self.show(&help).await?;
            *help_len = if self.screen.where_y() == y { self.screen.where_x().saturating_sub(x) as usize } else { self.visible_len(&fluxbbs_term::cp437::encode(&help)) };
        }
        if hi {
            self.goto_pos(LIGHTBAR_CURSOR);
        }
        Ok(())
    }

    /// New: a hint for the caller from what's waiting for them here
    /// (email, mentions, new posts, new files), with the key this menu
    /// has for it; one at random of those that apply. None for a bot, or
    /// with nothing to say.
    fn caller_hint(&self, menu: &Menu) -> Option<String> {
        if !self.logged_in || self.user.id == 0 || crate::aimemory::is_bot(&self.user) {
            return None;
        }
        let key = |codes: &[&str], to_menu: &str| {
            menu.commands
                .iter()
                .filter(|c| !c.hidden && c.keys != FIRST_CMD && self.acs(&c.acs))
                .find(|c| codes.contains(&c.command.as_str()) || (c.command.starts_with('-') && c.param.eq_ignore_ascii_case(to_menu)))
                .map(|c| self.strings().fmt("HintKey", &[&c.keys.to_ascii_lowercase()]))
                .unwrap_or_default()
        };
        let s = |n: usize| if n == 1 { "" } else { "s" };
        // Only what this menu can act on: a hint whose key isn't here (new
        // files, on the doors menu) would just cover the selected option's
        // help, so it waits for a menu that has one.
        let board = key(&["MH"], "MESSAGE");
        // A request of theirs filled comes first, and once.
        if !board.is_empty()
            && let Some((id, _, subject)) = self.state.msgs.filled_untold(self.user.id).ok().and_then(|v| v.into_iter().next())
        {
            let _ = self.state.msgs.filled_told(id);
            let subject = fluxbbs_term::codes::squish(&subject, 30);
            return Some(self.strings().fmt("HintFilled", &[&fluxbbs_term::lit(&subject), &board]));
        }
        // And one that filled someone's request.
        if !board.is_empty()
            && let Some((id, subject, pts)) = self.state.msgs.fills_untold(self.user.id).ok().and_then(|v| v.into_iter().next())
        {
            let _ = self.state.msgs.fill_told(id, self.user.id);
            let subject = fluxbbs_term::codes::squish(&subject, 30);
            return Some(self.strings().fmt("HintFill", &[&fluxbbs_term::lit(&subject), &pts.to_string()]));
        }
        let mut hints = Vec::new();
        let mail = self.mail_waiting() as usize;
        let k = key(&["ER"], "EMAIL");
        if mail > 0 && !k.is_empty() {
            hints.push(self.strings().fmt("HintMail", &[&mail.to_string(), s(mail), &k]));
        }
        let mentions = self.state.msgs.mentions(self.user.id, true).map(|m| m.len()).unwrap_or(0);
        let k = key(&["M@", "MH"], "MESSAGE");
        if mentions > 0 && !k.is_empty() {
            hints.push(self.strings().fmt("HintMentions", &[&mentions.to_string(), s(mentions), &k]));
        }
        let posts = self.msg_stats(false).unread;
        let k = key(&["MH", "MN"], "MESSAGE");
        if posts > 0 && !k.is_empty() {
            hints.push(self.strings().fmt("HintPosts", &[&posts.to_string(), s(posts), &k]));
        }
        // Only a count worth reading: not every file there is, for a caller
        // who's never scanned, nor a whole archive brought in at once.
        let files = self.file_stats(false).since as usize;
        let k = key(&["FN"], "FILE");
        if self.user.file_scan.is_some() && (1..1000).contains(&files) && !k.is_empty() {
            hints.push(self.strings().fmt("HintFiles", &[&files.to_string(), s(files), &k]));
        }
        if hints.is_empty() {
            return None;
        }
        Some(hints.swap_remove(rand::random::<u32>() as usize % hints.len()))
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
            if (!c.keys.eq_ignore_ascii_case(keys) && !by_number) || c.dark {
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
        for c in commands.iter().filter(|c| !c.hidden && !c.dark && c.keys != FIRST_CMD) {
            if !self.acs(&c.acs) {
                continue;
            }
            let line = format!(" |U3{} |U1{}", fluxbbs_term::codes::resize(&c.keys, 5), squish(&c.help, 72));
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
        if self.nonstop || !self.user.pause || self.plain() || *lines < self.user.page_length.saturating_sub(1).max(5) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_option_is_as_wide_as_its_letters() {
        assert_eq!(ansi_width(" \x1b[0;1;30m(\x1b[0;1;31mt\x1b[0;1;30m) \x1b[38;2;255;85;85mt\x1b[38;2;250;80;80marot  \x1b[0m"), 12);
        assert_eq!(ansi_width("|B1Tarot"), 5);
    }

    #[test]
    fn bar_moves_by_place() {
        // Two columns of two, then a wide one below.
        let p = [Some((2, 10)), Some((2, 11)), Some((40, 10)), Some((40, 11)), Some((30, 14))];
        assert_eq!(bar_move(&p, 0, Key::Right), 2);
        assert_eq!(bar_move(&p, 2, Key::Left), 0);
        assert_eq!(bar_move(&p, 0, Key::Down), 1);
        assert_eq!(bar_move(&p, 3, Key::Down), 4);
        assert_eq!(bar_move(&p, 4, Key::Up), 3);
        // Nothing that way: in order, wrapping.
        assert_eq!(bar_move(&p, 2, Key::Right), 3);
        assert_eq!(bar_move(&p, 4, Key::Down), 0);
        assert_eq!(bar_move(&p, 0, Key::Up), 4);
        // Down from the foot of one box: the cell straight below, not the
        // option a row lower in a box across the screen.
        let q = [Some((3, 13)), Some((55, 14)), Some((3, 18))];
        assert_eq!(bar_move(&q, 0, Key::Down), 2);
        // A row (MATRIX): up and down step along it.
        let row = [Some((1, 20)), Some((15, 20)), Some((30, 20))];
        assert_eq!(bar_move(&row, 1, Key::Down), 2);
        assert_eq!(bar_move(&row, 1, Key::Up), 0);
        assert_eq!(bar_move(&[None, None], 0, Key::Down), 1);
    }
}
