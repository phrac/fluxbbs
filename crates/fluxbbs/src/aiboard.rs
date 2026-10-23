//! The AI chat's view of the board itself (new): its menus and their
//! keys, the file and message areas, and the files in them, as tools the
//! model can call, so it can answer "what's here?" and "how do I...?"
//! with real keys. Only what the caller's own access lets them see.

use serde_json::{Value, json};

use crate::menu::Menu;
use crate::session::Session;

/// Most files one call lists or finds.
const MAX_FILES: usize = 60;

pub(crate) fn tool_specs() -> Vec<Value> {
    let tool = |name: &str, desc: &str, props: Value, required: &[&str]| {
        json!({"type": "function", "function": {"name": name, "description": desc,
            "parameters": {"type": "object", "properties": props, "required": required}}})
    };
    vec![
        tool("board_menus", "This BBS's menus the caller can use: each menu's keys, what they do, and the menu a key leads to. \
              Use it to tell the caller exactly which keys to press.", json!({}), &[]),
        tool("file_areas", "This BBS's file areas the caller can see, with how many files each holds.", json!({}), &[]),
        tool("list_files", "The files in one of this BBS's file areas: name, size, date, downloads and description.",
             json!({"area": {"type": "string", "description": "The area's name, as file_areas gives it"}}), &["area"]),
        tool("search_files", "Search this BBS's files (names and descriptions) in every area the caller can see.",
             json!({"query": {"type": "string", "description": "Words to look for"}}), &["query"]),
        tool("message_areas", "This BBS's message areas the caller can read, with how many messages each holds.", json!({}), &[]),
    ]
}

impl Session {
    /// Run a board tool; None if `name` isn't one.
    pub(crate) fn ai_board_tool(&self, name: &str, args: &Value) -> Option<String> {
        let arg = |k: &str| args[k].as_str().unwrap_or_default().trim().to_string();
        Some(match name {
            "board_menus" => self.ai_menus(),
            "file_areas" => self.ai_file_areas(),
            "list_files" => self.ai_list_files(&arg("area")),
            "search_files" => self.ai_search_files(&arg("query")),
            "message_areas" => self.ai_message_areas(),
            _ => return None,
        })
    }

    fn ai_menus(&self) -> String {
        let cfg = self.state.config();
        let dir = cfg.menu_dirs();
        let names = crate::menuedit::menu_names(&dir);
        let global = Menu::load(&dir, "GLOBAL").ok();
        let mut out = format!("The caller starts at {} after logging on. Keys are typed at a menu's prompt.\n", cfg.logon.start_menu);
        for name in names.iter().filter(|n| !matches!(n.as_str(), "GLOBAL" | "MATRIX" | "NEWUSER" | "STARTUP")) {
            let Ok(menu) = Menu::load(&dir, name) else { continue };
            if !self.acs(&menu.acs) {
                continue;
            }
            let mut cmds: Vec<String> = Vec::new();
            for c in menu.commands.iter().chain(global.iter().filter(|_| menu.global).flat_map(|g| g.commands.iter())) {
                if c.hidden || c.keys.eq_ignore_ascii_case("FIRSTCMD") || c.desc.is_empty() || !self.acs(&c.acs) {
                    continue;
                }
                let goes = match c.command.as_str() {
                    "-^" | "-/" | "-\\" if !c.param.is_empty() => format!(" (opens {})", c.param.to_ascii_uppercase()),
                    _ => String::new(),
                };
                let line = format!("{} = {}{goes}", c.keys, c.desc);
                if !cmds.contains(&line) {
                    cmds.push(line);
                }
            }
            if cmds.is_empty() {
                continue;
            }
            let title = if menu.title.is_empty() { String::new() } else { format!(" ({})", menu.title) };
            out.push_str(&format!("{name}{title}: {}\n", cmds.join("; ")));
        }
        out
    }

    /// The file areas the caller can see, in board order.
    fn ai_visible_file_areas(&self) -> Vec<crate::fileareas::FileArea> {
        self.state.fareas().areas.iter().filter(|a| self.acs(&a.acs)).cloned().collect()
    }

    fn ai_file_areas(&self) -> String {
        let areas = self.ai_visible_file_areas();
        if areas.is_empty() {
            return "There are no file areas this caller can see.".into();
        }
        let mut out = String::from("File areas (sub-areas show their parent):\n");
        for a in &areas {
            let n = self.state.files.count(&a.file).unwrap_or(0);
            let parent = areas.iter().find(|p| !a.parent.is_empty() && p.file.eq_ignore_ascii_case(&a.parent)).map(|p| format!(" [in {}]", p.name)).unwrap_or_default();
            let locked = if a.password.is_empty() { "" } else { " (password)" };
            out.push_str(&format!("{}{parent}: {n} files{locked}\n", a.name));
        }
        out
    }

    fn ai_list_files(&self, area: &str) -> String {
        let areas = self.ai_visible_file_areas();
        let wanted = area.to_ascii_lowercase();
        let Some(a) = areas.iter().find(|a| a.name.eq_ignore_ascii_case(area)).or_else(|| areas.iter().find(|a| a.name.to_ascii_lowercase().contains(&wanted))) else {
            return format!("No file area called {area:?}; file_areas lists them.");
        };
        if !a.password.is_empty() {
            return format!("{} needs a password; its files aren't listed.", a.name);
        }
        let files = self.state.files.list(&a.file).unwrap_or_default();
        if files.is_empty() {
            return format!("{} is empty.", a.name);
        }
        let mut out = format!("{} ({} files):\n", a.name, files.len());
        for f in files.iter().take(MAX_FILES) {
            out.push_str(&file_line(f));
        }
        if files.len() > MAX_FILES {
            out.push_str(&format!("...and {} more.\n", files.len() - MAX_FILES));
        }
        out
    }

    fn ai_search_files(&self, query: &str) -> String {
        let words: Vec<String> = query.split_whitespace().map(str::to_ascii_lowercase).collect();
        if words.is_empty() {
            return "Search for what?".into();
        }
        let mut out = String::new();
        let mut found = 0;
        for a in self.ai_visible_file_areas().iter().filter(|a| a.password.is_empty()) {
            for f in self.state.files.list(&a.file).unwrap_or_default() {
                let text = format!("{} {}", f.name, f.description.join(" ")).to_ascii_lowercase();
                if words.iter().all(|w| text.contains(w.as_str())) {
                    found += 1;
                    if found <= MAX_FILES {
                        out.push_str(&format!("[{}] {}", a.name, file_line(&f)));
                    }
                }
            }
        }
        match found {
            0 => format!("No files match {query:?}."),
            n if n > MAX_FILES => format!("{out}...and {} more.\n", n - MAX_FILES),
            _ => out,
        }
    }

    fn ai_message_areas(&self) -> String {
        let areas = self.state.areas();
        let mut out = String::from("Message areas:\n");
        for a in areas.areas.iter().filter(|a| self.acs(&a.acs)) {
            let n = self.state.msgs.count(&a.file).unwrap_or(0);
            out.push_str(&format!("{}: {n} messages\n", a.name));
        }
        out.push_str("Private mail is in the email menu.\n");
        out
    }
}

fn file_line(f: &crate::filebase::FileRec) -> String {
    let date = f.date.map(|d| d.format(" %Y-%m-%d").to_string()).unwrap_or_default();
    format!("{} ({} KB{date}, {} downloads): {}\n", f.name, f.size.div_ceil(1024), f.downloads, crate::web::blurb(&f.description))
}
