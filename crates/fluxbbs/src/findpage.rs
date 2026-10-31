//! New: the file search page (`f` in the file browser, `FW search`): the
//! board's name big in a search engine's four colors over a search box,
//! the areas and files the words find dropping down under it as they're
//! typed, and two buttons, Search and I'm Feeling Lucky (straight to the
//! best file).

use std::collections::HashMap;
use std::io;

use fluxbbs_term::Color;

use crate::filebase::Find;
use crate::input::Key;
use crate::session::Session;

/// What the caller chose.
pub enum Choice {
    /// Search for these words.
    Words(String),
    /// Open this file area.
    Area(u32),
    /// Open the area with the bar on this file.
    File(u32, String),
}

/// The logo's colors, letter by letter: blue, red, yellow, blue, green, red.
const LOGO: [u8; 6] = [9, 12, 14, 9, 10, 12];
/// The box: from this column, this wide.
const BOX_X: u16 = 11;
const BOX_W: u16 = 60;
/// Suggestions shown at most.
const MAX_SUGGEST: usize = 6;
/// The longest search.
const MAX_LEN: usize = 70;
/// Matches a suggestion looks through at most.
const SCAN: usize = 300;

/// One line of the drop-down.
struct Suggestion {
    label: String,
    /// Where it is (an area's parent, a file's area), dim at the right.
    place: String,
    to: Choice,
}

/// Where focus is: the box (with a suggestion picked or not), or a button.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Box(Option<usize>),
    Search,
    Lucky,
}

/// The areas a caller may search: theirs without a password.
struct Areas {
    keys: Vec<String>,
    num_of: HashMap<String, u32>,
    /// Name and line about it, to match words against.
    text: Vec<(u32, String, String)>,
}

impl Session {
    /// The search page: what to look for, or None (Esc, or nothing typed).
    pub(crate) async fn find_page(&mut self) -> io::Result<Option<Choice>> {
        let areas = self.searchable_areas()?;
        let rows = self.screen.rows().max(20);
        // The logo's last row, the box's top row.
        let top = rows / 2 - 1;
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        self.find_page_frame(top, rows);
        let mut text = String::new();
        let mut focus = Focus::Box(None);
        let mut shown: Vec<Suggestion> = Vec::new();
        let mut drawn = 0usize;
        let mut stale = false;
        loop {
            // Typing fast is read in one go; the drop-down catches up once
            // the keys stop.
            if stale && !self.key_waiting().await? {
                shown = self.suggest(&areas, &text);
                if let Focus::Box(Some(i)) = focus
                    && i >= shown.len()
                {
                    focus = Focus::Box(None);
                }
                stale = false;
            }
            // Still stale: keys are waiting, read them before drawing.
            if !stale {
                drawn = self.find_page_draw(top, &text, &shown, focus, drawn);
                self.flush().await?;
            }
            let key = self.read_key().await?;
            let before = focus;
            match key {
                Key::Esc => {
                    self.find_page_leave();
                    return Ok(None);
                }
                Key::Tab => {
                    focus = match focus {
                        Focus::Box(_) => Focus::Search,
                        Focus::Search => Focus::Lucky,
                        Focus::Lucky => Focus::Box(None),
                    }
                }
                Key::Down => {
                    focus = match focus {
                        Focus::Box(None) if !shown.is_empty() => Focus::Box(Some(0)),
                        Focus::Box(Some(i)) if i + 1 < shown.len() => Focus::Box(Some(i + 1)),
                        Focus::Box(None) => Focus::Search,
                        f => f,
                    }
                }
                Key::Up => {
                    focus = match focus {
                        Focus::Box(Some(0)) => Focus::Box(None),
                        Focus::Box(Some(i)) => Focus::Box(Some(i - 1)),
                        Focus::Search | Focus::Lucky => Focus::Box(None),
                        f => f,
                    }
                }
                Key::Left | Key::Right if matches!(focus, Focus::Search | Focus::Lucky) => {
                    focus = if focus == Focus::Search { Focus::Lucky } else { Focus::Search };
                }
                Key::Enter => {
                    let words = text.trim().to_string();
                    let choice = match focus {
                        Focus::Box(Some(i)) if i < shown.len() => Some(shown.swap_remove(i).to),
                        Focus::Lucky => self.lucky(&areas, &words),
                        _ => Some(Choice::Words(words.clone())).filter(|_| !Find::parse(&words).is_empty()),
                    };
                    match choice {
                        Some(c) => {
                            self.find_page_leave();
                            return Ok(Some(c));
                        }
                        // Nothing to go on: stay.
                        None => focus = Focus::Box(None),
                    }
                }
                Key::Backspace => {
                    text.pop();
                    focus = Focus::Box(None);
                    stale = true;
                }
                // ^U, ^Y: clear the box.
                Key::Char(0x15 | 0x19) => {
                    text.clear();
                    focus = Focus::Box(None);
                    stale = true;
                }
                Key::Char(c) if (0x20..0x7F).contains(&c) && text.len() < MAX_LEN => {
                    text.push(c as char);
                    focus = Focus::Box(None);
                    stale = true;
                }
                _ => {}
            }
            // On a button, the drop-down closes so the buttons show; back
            // in the box, it opens again.
            if matches!(focus, Focus::Search | Focus::Lucky) {
                shown.clear();
            } else if matches!(before, Focus::Search | Focus::Lucky) {
                stale = true;
            }
        }
    }

    fn searchable_areas(&mut self) -> io::Result<Areas> {
        let fa = self.state.fareas();
        let nums: Vec<u32> = self.visible_fareas().into_iter().map(|(n, _)| n).filter(|&n| fa.get(n).is_some_and(|a| a.password.is_empty())).collect();
        let abouts = self.state.files.area_abouts().map_err(io::Error::other)?;
        let keys = nums.iter().filter_map(|&n| fa.get(n)).map(|a| a.file.clone()).collect();
        let num_of = nums.iter().filter_map(|&n| fa.get(n).map(|a| (a.file.to_lowercase(), n))).collect();
        let text = nums
            .iter()
            .filter_map(|&n| {
                let a = fa.get(n)?;
                let name = crate::editor::strip_colours(&a.name);
                let about = abouts.get(&a.file.to_lowercase()).cloned().unwrap_or_default();
                Some((n, name.clone(), format!("{name}\n{about}")))
            })
            .collect();
        Ok(Areas { keys, num_of, text })
    }

    /// The areas whose name or line has the words (two at most), then the
    /// best files.
    fn suggest(&self, areas: &Areas, text: &str) -> Vec<Suggestion> {
        let find = Find::parse(text);
        if find.is_empty() {
            return Vec::new();
        }
        let fa = self.state.fareas();
        let area_name = |n: u32| fa.get(n).map(|a| crate::editor::strip_colours(&a.name)).unwrap_or_default();
        let mut out: Vec<Suggestion> = areas
            .text
            .iter()
            .filter(|(_, _, t)| find.matches_text(t))
            .take(2)
            .map(|(n, name, _)| Suggestion { label: name.clone(), place: fa.parent_of(*n).map(area_name).map_or_else(|| "area".into(), |p| format!("area in {p}")), to: Choice::Area(*n) })
            .collect();
        let want = MAX_SUGGEST - out.len();
        if let Ok((files, _)) = self.state.files.find_words_quick(&areas.keys, &find, want, SCAN) {
            for f in files {
                let Some(&n) = areas.num_of.get(&f.area.to_lowercase()) else { continue };
                out.push(Suggestion { label: f.name.clone(), place: area_name(n), to: Choice::File(n, f.name) });
            }
        }
        out
    }

    /// I'm Feeling Lucky: the best file the words find.
    fn lucky(&self, areas: &Areas, words: &str) -> Option<Choice> {
        let find = Find::parse(words);
        let (files, _) = self.state.files.find_words(&areas.keys, &find, 1).ok()?;
        let f = files.into_iter().next()?;
        let n = *areas.num_of.get(&f.area.to_lowercase())?;
        Some(Choice::File(n, f.name))
    }

    /// What doesn't change: the logo, the count of files, the help line.
    fn find_page_frame(&mut self, top: u16, rows: u16) {
        let cfg = self.state.config();
        let fs = &cfg.files;
        let word = if fs.search_logo.trim().is_empty() { cfg.bbs.name.clone() } else { fs.search_logo.trim().to_string() };
        let font = fs.search_font.trim();
        let set = if font.is_empty() {
            None
        } else {
            let fonts = crate::tdf::read(&cfg.resolve(std::path::Path::new(font)));
            let face = fs.search_face.trim();
            fonts.iter().find(|f| face.is_empty() || f.name.eq_ignore_ascii_case(face)).map(|f| f.set_letters(&word)).filter(|(c, w, h)| !c.is_empty() && *w <= 80 && *h < top)
        };
        match set {
            Some((cells, w, h)) => {
                let x0 = (80 - w) / 2 + 1;
                let y0 = top - h;
                for (letter, c) in cells {
                    let ink = LOGO[letter % LOGO.len()];
                    // The face's shading kept: its dim cells dim, its
                    // backgrounds the dim of the letter's color.
                    let fore = if c.fg >= 8 || c.fg == 7 { ink } else { ink & 7 };
                    let back = if c.bg == 0 { 0 } else { ink & 7 };
                    self.screen.goto(x0 + c.x, y0 + c.y);
                    self.screen.set_color(Color::new(fore, back));
                    self.screen.write_bytes(&[c.ch]);
                }
            }
            None => {
                let letters: Vec<char> = word.chars().collect();
                let x0 = (80u16.saturating_sub(letters.len() as u16 * 2) / 2).max(1);
                let mut i = 0;
                for (k, ch) in letters.iter().enumerate() {
                    if *ch == ' ' {
                        continue;
                    }
                    self.screen.goto(x0 + k as u16 * 2, top - 2);
                    self.screen.set_color(Color::new(LOGO[i % LOGO.len()], 0));
                    self.screen.write_text(&ch.to_string());
                    i += 1;
                }
            }
        }
        // Top right, as a search page shows the account: how many files,
        // and the caller's initial.
        let total = self.file_stats(true).files;
        let count = self.strings().fmt("FaFindPageFiles", &[&crate::files::commas(total as u64)]);
        let count = crate::editor::strip_colours(&count);
        let initial = self.user.handle.chars().next().unwrap_or('?').to_uppercase().to_string();
        self.screen.goto(80 - 4 - count.chars().count() as u16, 1);
        self.screen.set_color(Color::new(7, 0));
        self.screen.write_text(&count);
        self.screen.goto(77, 1);
        self.screen.set_color(Color::new(15, 5));
        self.screen.write_text(&format!(" {initial} "));
        let help = crate::editor::strip_colours(self.strings().get("FaFindPageHelp"));
        let help: String = help.chars().take(79).collect();
        self.screen.goto((80 - help.chars().count() as u16) / 2 + 1, rows);
        self.screen.set_color(Color::new(8, 0));
        self.screen.write_text(&help);
    }

    /// The box, the drop-down or the buttons under it, and the cursor.
    /// `drawn`: how many suggestion lines were up before; returns how many
    /// are now.
    fn find_page_draw(&mut self, top: u16, text: &str, shown: &[Suggestion], focus: Focus, drawn: usize) -> usize {
        let edge = Color::new(if matches!(focus, Focus::Box(_)) { 7 } else { 8 }, 0);
        let inner = (BOX_W - 2) as usize;
        let line = "\u{2500}".repeat(inner);
        let (y_box, y_text) = (top + 1, top + 2);
        self.screen.write_bytes(b"\x1b[?25l");
        self.screen.set_color(edge);
        self.screen.goto(BOX_X, y_box);
        self.screen.write_str(&format!("\u{250c}{line}\u{2510}"));
        // The text: its end, if it's longer than the box.
        let room = inner - 4;
        let tail: String = { let n = text.chars().count(); text.chars().skip(n.saturating_sub(room)).collect() };
        self.screen.goto(BOX_X, y_text);
        self.screen.write_str("\u{2502}");
        self.screen.set_color(Color::new(8, 0));
        self.screen.write_str(" \u{00bb} ");
        if text.is_empty() {
            let name = self.state.config().bbs.name.clone();
            let hint = crate::editor::strip_colours(&self.strings().fmt("FaFindPageHint", &[&name]));
            let hint: String = hint.chars().take(room).collect();
            self.screen.write_text(&format!("{hint:<room$}"));
        } else {
            self.screen.set_color(Color::new(15, 0));
            self.screen.write_text(&format!("{tail:<room$}"));
        }
        self.screen.write_str(" ");
        self.screen.set_color(edge);
        self.screen.write_str("\u{2502}");
        // The drop-down, closed with the box's bottom edge; or that edge
        // and the buttons.
        let mut y = y_text + 1;
        if !shown.is_empty() {
            self.screen.goto(BOX_X, y);
            self.screen.write_str(&format!("\u{251c}{line}\u{2524}"));
            y += 1;
            for (i, s) in shown.iter().enumerate() {
                let on = focus == Focus::Box(Some(i));
                let place_w = 22.min(s.place.chars().count());
                let label_w = inner - 4 - place_w - 1;
                let label: String = s.label.chars().take(label_w).collect();
                let place: String = s.place.chars().take(place_w).collect();
                self.screen.goto(BOX_X, y);
                self.screen.set_color(edge);
                self.screen.write_str("\u{2502}");
                let (fg, bg) = if on { (15, 1) } else { (7, 0) };
                self.screen.set_color(Color::new(8, bg));
                let icon = if matches!(s.to, Choice::Area(_)) { "\u{25ba}" } else { "\u{00bb}" };
                self.screen.write_str(&format!(" {icon} "));
                self.screen.set_color(Color::new(fg, bg));
                self.screen.write_text(&format!("{label:<label_w$}"));
                self.screen.set_color(Color::new(if on { 7 } else { 8 }, bg));
                self.screen.write_text(&format!(" {place:>place_w$} "));
                self.screen.set_color(edge);
                self.screen.write_str("\u{2502}");
                y += 1;
            }
        }
        self.screen.goto(BOX_X, y);
        self.screen.set_color(edge);
        self.screen.write_str(&format!("\u{2514}{line}\u{2518}"));
        y += 1;
        // Lines the drop-down had and hasn't now.
        let old_bottom = y_text + if drawn > 0 { drawn as u16 + 2 } else { 1 };
        while y <= old_bottom.max(y_text + 3) {
            self.screen.goto(BOX_X, y);
            self.screen.set_color(Color::DEFAULT);
            self.screen.write_str(&" ".repeat(BOX_W as usize));
            y += 1;
        }
        if shown.is_empty() {
            let name = self.state.config().bbs.name.clone();
            let search = crate::editor::strip_colours(&self.strings().fmt("FaFindPageSearch", &[&name]));
            let lucky = crate::editor::strip_colours(self.strings().get("FaFindPageLucky"));
            let (a, b) = (format!("  {search}  "), format!("  {lucky}  "));
            let w = (a.chars().count() + 3 + b.chars().count()) as u16;
            let x = (80u16.saturating_sub(w)) / 2 + 1;
            let button = |s: &mut Self, at: u16, label: &str, on: bool| {
                s.screen.goto(at, y_text + 3);
                s.screen.set_color(if on { Color::new(15, 1) } else { Color::new(0, 7) });
                s.screen.write_text(label);
            };
            button(self, x, &a, focus == Focus::Search);
            button(self, x + a.chars().count() as u16 + 3, &b, focus == Focus::Lucky);
        }
        self.screen.set_color(Color::DEFAULT);
        // The cursor in the box while typing there.
        if focus == Focus::Box(None) {
            self.screen.goto(BOX_X + 4 + tail.chars().count() as u16, y_text);
            self.screen.write_bytes(b"\x1b[?25h");
        }
        shown.len()
    }

    fn find_page_leave(&mut self) {
        self.screen.write_bytes(b"\x1b[?25h");
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
    }
}
