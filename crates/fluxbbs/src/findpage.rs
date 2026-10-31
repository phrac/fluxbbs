//! New: the file search page (`f` in the file browser, `FW search`): the
//! board's name big in a search engine's four colors over a search box,
//! the areas and files the words find dropping down under it as they're
//! typed, and two buttons, Search and I'm Feeling Lucky (straight to the
//! best file). A search that finds nothing stays on the page and offers
//! what the caller may have meant: misspelled words put right from the
//! words the file base has, or the search less a word.

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
/// A description's piece under a file in the drop-down: the box's inside
/// less the indent and a space.
const SNIPPET_W: usize = (BOX_W as usize - 2) - 4;
/// Matches a suggestion looks through at most.
const SCAN: usize = 300;

/// One line of the drop-down.
struct Suggestion {
    label: String,
    /// Where it is (an area's parent, a file's area), dim at the right.
    place: String,
    to: Choice,
    /// For a file found by its description: a piece of it round the words,
    /// as (text, is it one of the words); empty when its name says why.
    snippet: Vec<(String, bool)>,
}

impl Suggestion {
    /// Lines it takes in the drop-down.
    fn lines(&self) -> usize {
        1 + usize::from(!self.snippet.is_empty())
    }
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
    /// The words the file base has, and in how many files: read the first
    /// time a search finds nothing.
    vocab: Option<HashMap<String, u32>>,
}

impl Session {
    /// The search page: what to look for, or None (Esc, or nothing typed).
    pub(crate) async fn find_page(&mut self) -> io::Result<Option<Choice>> {
        let mut areas = self.searchable_areas()?;
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
        // "no results for ...", over what it may have meant.
        let mut notice: Option<String> = None;
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
                drawn = self.find_page_draw(top, &text, &shown, focus, drawn, notice.as_deref());
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
                    let empty = Find::parse(&words).is_empty();
                    let choice = match focus {
                        Focus::Box(Some(i)) if i < shown.len() => Some(shown.swap_remove(i).to),
                        _ if empty => None,
                        Focus::Lucky => self.lucky(&areas, &words),
                        _ => Some(Choice::Words(words.clone())).filter(|_| self.results(&areas, &Find::parse(&words)) > 0),
                    };
                    match choice {
                        Some(c) => {
                            self.find_page_leave();
                            return Ok(Some(c));
                        }
                        // Nothing to go on: stay.
                        None if empty => focus = Focus::Box(None),
                        // Nothing found: stay, with what they may have meant.
                        None => {
                            let name = self.state.config().bbs.name.clone();
                            shown = self.did_you_mean(&mut areas, &words);
                            let s = self.strings();
                            notice = Some(plain(&if shown.is_empty() {
                                s.fmt("FaFindPageNone", &[&words, &name])
                            } else {
                                s.fmt("FaFindPageMeant", &[&words, &name])
                            }));
                            focus = Focus::Box(None);
                            continue;
                        }
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
            if stale {
                notice = None;
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
                let name = plain(&a.name);
                let about = abouts.get(&a.file.to_lowercase()).cloned().unwrap_or_default();
                Some((n, name.clone(), format!("{name}\n{about}")))
            })
            .collect();
        Ok(Areas { keys, num_of, text, vocab: None })
    }

    /// The areas whose name or line has the words (two at most), then the
    /// best files.
    fn suggest(&self, areas: &Areas, text: &str) -> Vec<Suggestion> {
        let find = Find::parse(text);
        if find.is_empty() {
            return Vec::new();
        }
        let fa = self.state.fareas();
        let area_name = |n: u32| fa.get(n).map(|a| plain(&a.name)).unwrap_or_default();
        let mut out: Vec<Suggestion> = areas
            .text
            .iter()
            .filter(|(_, _, t)| find.matches_text(t))
            .take(2)
            .map(|(n, name, _)| Suggestion {
                label: name.clone(),
                place: fa.parent_of(*n).map(area_name).map_or_else(|| "area".into(), |p| format!("area in {p}")),
                to: Choice::Area(*n),
                snippet: Vec::new(),
            })
            .collect();
        let want = MAX_SUGGEST - out.len();
        // A file with a piece of its description takes two lines; as many
        // as fit in the drop-down's.
        let mut lines = out.len();
        if let Ok((files, _)) = self.state.files.find_words_quick(&areas.keys, &find, want, SCAN) {
            for f in files {
                let Some(&n) = areas.num_of.get(&f.area.to_lowercase()) else { continue };
                let snip = snippet(&f.description.join(" "), &f.name, &find.want, SNIPPET_W);
                let s = Suggestion { label: f.name.clone(), place: area_name(n), to: Choice::File(n, f.name), snippet: snip };
                if lines + s.lines() > MAX_SUGGEST {
                    break;
                }
                lines += s.lines();
                out.push(s);
            }
        }
        out
    }

    /// How many areas and files the words find.
    fn results(&self, areas: &Areas, find: &Find) -> usize {
        let in_areas = areas.text.iter().filter(|(_, _, t)| find.matches_text(t)).count();
        in_areas + self.state.files.count_words(&areas.keys, find).unwrap_or(0)
    }

    /// What a search that found nothing may have meant, each finding
    /// something: its misspelled words (in no file at all) put right, the
    /// closest of the file base's words first; then the search less a word.
    fn did_you_mean(&self, areas: &mut Areas, words: &str) -> Vec<Suggestion> {
        let find = Find::parse(words);
        if areas.vocab.is_none() {
            areas.vocab = Some(self.state.files.vocabulary(&areas.keys).unwrap_or_default());
        }
        let areas = &*areas;
        let vocab = areas.vocab.as_ref().expect("read above");
        let one = |w: &str| Find { want: vec![w.to_string()], not: Vec::new() };
        // Each word's best spellings: itself if it's found, else the
        // closest words there are.
        let spellings: Vec<Vec<String>> = find
            .want
            .iter()
            .map(|w| {
                if w.contains(['*', '?', ' ']) || self.results(areas, &one(w)) > 0 {
                    vec![w.clone()]
                } else {
                    closest(vocab, w, 3)
                }
            })
            .collect();
        if spellings.iter().any(|s| s.is_empty()) && find.want.len() == 1 {
            return Vec::new();
        }
        let mut tries: Vec<Vec<String>> = Vec::new();
        let best: Vec<String> = spellings.iter().map(|s| s.first().cloned().unwrap_or_default()).collect();
        if spellings.iter().all(|s| !s.is_empty()) {
            tries.push(best.clone());
            // The other spellings of each word put right.
            for (i, s) in spellings.iter().enumerate() {
                for alt in s.iter().skip(1) {
                    let mut t = best.clone();
                    t[i] = alt.clone();
                    tries.push(t);
                }
            }
        }
        // Less a word.
        if find.want.len() > 1 {
            for i in 0..best.len() {
                let t: Vec<String> = best.iter().enumerate().filter(|&(j, w)| j != i && !w.is_empty()).map(|(_, w)| w.clone()).collect();
                if !t.is_empty() {
                    tries.push(t);
                }
            }
        }
        let mut out: Vec<Suggestion> = Vec::new();
        for t in tries {
            let f = Find { want: t, not: find.not.clone() };
            let q = query_text(&f);
            if q.eq_ignore_ascii_case(words) || out.iter().any(|s| s.label.eq_ignore_ascii_case(&q)) {
                continue;
            }
            let n = self.results(areas, &f);
            if n > 0 {
                let place = plain(&self.strings().fmt("FaFindPageFound", &[&crate::files::commas(n as u64)]));
                out.push(Suggestion { label: q.clone(), place, to: Choice::Words(q), snippet: Vec::new() });
            }
            if out.len() == MAX_SUGGEST {
                break;
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
        let count = plain(&count);
        let initial = self.user.handle.chars().next().unwrap_or('?').to_uppercase().to_string();
        self.screen.goto(80 - 4 - count.chars().count() as u16, 1);
        self.screen.set_color(Color::new(7, 0));
        self.screen.write_text(&count);
        self.screen.goto(77, 1);
        self.screen.set_color(Color::new(15, 5));
        self.screen.write_text(&format!(" {initial} "));
        // Under it all, as a search engine once had it.
        let motto = plain(self.strings().get("FaFindPageMotto"));
        let motto: String = motto.chars().take(79).collect();
        if !motto.trim().is_empty() {
            self.screen.goto((80 - motto.chars().count() as u16) / 2 + 1, rows - 2);
            self.screen.set_color(Color::new(8, 0));
            self.screen.write_text(&motto);
        }
        let help = plain(self.strings().get("FaFindPageHelp"));
        let help: String = help.chars().take(79).collect();
        self.screen.goto((80 - help.chars().count() as u16) / 2 + 1, rows);
        self.screen.set_color(Color::new(8, 0));
        self.screen.write_text(&help);
    }

    /// The box, the drop-down or the buttons under it, and the cursor.
    /// `drawn`: how many drop-down lines were up before; returns how many
    /// are now.
    fn find_page_draw(&mut self, top: u16, text: &str, shown: &[Suggestion], focus: Focus, drawn: usize, notice: Option<&str>) -> usize {
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
            let hint = plain(&self.strings().fmt("FaFindPageHint", &[&name]));
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
            match notice {
                // The notice on the line over what it offers.
                Some(n) => {
                    let n: String = n.chars().take(inner - 4).collect();
                    // "├─ " and the notice, then dashes to the edge.
                    let rest = "\u{2500}".repeat(inner - 2 - n.chars().count());
                    self.screen.write_str("\u{251c}\u{2500} ");
                    self.screen.set_color(Color::new(12, 0));
                    self.screen.write_text(&n);
                    self.screen.set_color(edge);
                    self.screen.write_str(&format!("{rest}\u{2524}"));
                }
                None => self.screen.write_str(&format!("\u{251c}{line}\u{2524}")),
            }
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
                let icon = match s.to {
                    Choice::Area(_) => "\u{25ba}",
                    Choice::Words(_) => "?",
                    Choice::File(..) => "\u{00bb}",
                };
                self.screen.write_str(&format!(" {icon} "));
                self.screen.set_color(Color::new(fg, bg));
                self.screen.write_text(&format!("{label:<label_w$}"));
                self.screen.set_color(Color::new(if on { 7 } else { 8 }, bg));
                self.screen.write_text(&format!(" {place:>place_w$} "));
                self.screen.set_color(edge);
                self.screen.write_str("\u{2502}");
                y += 1;
                // Why it was found: its words lit in a piece of its description.
                if !s.snippet.is_empty() {
                    self.screen.goto(BOX_X, y);
                    self.screen.set_color(edge);
                    self.screen.write_str("\u{2502}");
                    self.screen.set_color(Color::new(8, bg));
                    self.screen.write_str("   ");
                    let mut used = 0;
                    for (part, hit) in &s.snippet {
                        self.screen.set_color(Color::new(if *hit { 15 } else if on { 7 } else { 8 }, bg));
                        self.screen.write_text(part);
                        used += part.chars().count();
                    }
                    self.screen.set_color(Color::new(8, bg));
                    self.screen.write_str(&" ".repeat(inner - 3 - used));
                    self.screen.set_color(edge);
                    self.screen.write_str("\u{2502}");
                    y += 1;
                }
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
            // Nothing to offer: the notice under the box, over the buttons.
            if let Some(n) = notice {
                let n: String = n.chars().take(78).collect();
                self.screen.goto((80 - n.chars().count() as u16) / 2 + 1, y_text + 2);
                self.screen.set_color(Color::new(12, 0));
                self.screen.write_text(&n);
            }
            let name = self.state.config().bbs.name.clone();
            let search = plain(&self.strings().fmt("FaFindPageSearch", &[&name]));
            let lucky = plain(self.strings().get("FaFindPageLucky"));
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
        shown.iter().map(Suggestion::lines).sum()
    }

    fn find_page_leave(&mut self) {
        self.screen.write_bytes(b"\x1b[?25h");
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
    }
}

/// Text to measure and show as it is: no color codes, nor the marks
/// `Strings::fmt` puts round what it fills in (they take no room on
/// screen, so counting them left lines short).
fn plain(s: &str) -> String {
    fluxbbs_term::codes::unlit(&crate::editor::strip_colours(s))
}

/// A piece of `desc`, `width` long at most, round the first of `words`
/// it has that `name` doesn't (the reason a file was found), with every
/// one of the words in it marked; empty if the name has them all.
fn snippet(desc: &str, name: &str, words: &[String], width: usize) -> Vec<(String, bool)> {
    let text: Vec<char> = desc.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    // Lower case a character at a time, so places in it are places in text.
    let lower: Vec<char> = text.iter().map(|c| c.to_ascii_lowercase()).collect();
    let name = name.to_lowercase();
    let words: Vec<Vec<char>> = words.iter().filter(|w| !w.contains(['*', '?']) && !w.is_empty()).map(|w| w.chars().map(|c| c.to_ascii_lowercase()).collect()).collect();
    let find = |w: &[char], from: usize| (from..lower.len().saturating_sub(w.len() - 1)).find(|&i| lower[i..i + w.len()] == *w);
    let Some(first) = words.iter().filter(|w| !name.contains(&w.iter().collect::<String>())).filter_map(|w| find(w, 0)).min() else { return Vec::new() };
    // All of it if it fits; else from a little before the word, at a
    // word's start, and no later than leaves the line full.
    let mut start = if text.len() <= width { 0 } else { first.saturating_sub(width / 4).min(text.len() - (width - 2)) };
    if start > 0 {
        start = (start..first).find(|&i| text[i - 1] == ' ').unwrap_or(first);
    }
    let lead = if start > 0 { 2 } else { 0 };
    let room = width.saturating_sub(lead);
    let end = (start + room).min(text.len());
    let cut = end < text.len();
    let end = if cut { end - 2 } else { end };
    let mut lit = vec![false; text.len()];
    for w in &words {
        let mut at = 0;
        while let Some(i) = find(w, at) {
            lit[i..i + w.len()].iter_mut().for_each(|l| *l = true);
            at = i + 1;
        }
    }
    let mut out: Vec<(String, bool)> = Vec::new();
    if lead > 0 {
        out.push(("..".into(), false));
    }
    for i in start..end {
        match out.last_mut() {
            Some((s, l)) if *l == lit[i] => s.push(text[i]),
            _ => out.push((text[i].to_string(), lit[i])),
        }
    }
    if cut {
        out.push(("..".into(), false));
    }
    out
}

/// A search as words again: a phrase in quotes, left-out words with `-`.
fn query_text(f: &Find) -> String {
    let quote = |w: &String| if w.contains(' ') { format!("\"{w}\"") } else { w.clone() };
    f.want.iter().map(quote).chain(f.not.iter().map(|w| format!("-{}", quote(w)))).collect::<Vec<_>>().join(" ")
}

/// The `n` words in `vocab` closest to `word` (by edit distance, a few
/// edits at most for its length; the commoner first when as close).
fn closest(vocab: &HashMap<String, u32>, word: &str, n: usize) -> Vec<String> {
    let len = word.chars().count();
    let most = match len {
        0..=2 => return Vec::new(),
        3..=4 => 1,
        5..=8 => 2,
        _ => 3,
    };
    let w: Vec<char> = word.chars().collect();
    let mut found: Vec<(usize, std::cmp::Reverse<u32>, &String)> = vocab
        .iter()
        .filter(|(v, _)| v.chars().count().abs_diff(len) <= most)
        .filter_map(|(v, &c)| edits(&w, v, most).map(|d| (d, std::cmp::Reverse(c), v)))
        .collect();
    found.sort();
    found.into_iter().take(n).map(|(_, _, v)| v.clone()).collect()
}

/// Edits (insert, delete, change) from `a` to `b`, if `most` or fewer.
fn edits(a: &[char], b: &str, most: usize) -> Option<usize> {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        // Every way on is already too many.
        if cur.iter().min().is_some_and(|&m| m > most) {
            return None;
        }
        prev = cur;
    }
    let d = prev[b.len()];
    (d <= most).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_words_and_queries() {
        let vocab: HashMap<String, u32> = [("nodelist", 40), ("modelist", 2), ("hacking", 90), ("packing", 5), ("error", 30)].into_iter().map(|(w, c)| (w.to_string(), c)).collect();
        assert_eq!(closest(&vocab, "nodlist", 3), ["nodelist", "modelist"]);
        assert_eq!(closest(&vocab, "hackin", 3), ["hacking", "packing"]);
        assert_eq!(closest(&vocab, "errror", 3), ["error"]);
        assert!(closest(&vocab, "zz", 3).is_empty() && closest(&vocab, "qqqqq", 3).is_empty());
        assert_eq!(edits(&"kitten".chars().collect::<Vec<_>>(), "sitting", 3), Some(3));
        assert_eq!(edits(&"kitten".chars().collect::<Vec<_>>(), "sitting", 2), None);
        assert_eq!(query_text(&Find::parse(r#"atm "two words" -demo"#)), r#"atm "two words" -demo"#);
    }

    #[test]
    fn a_piece_of_the_description_says_why() {
        let words = vec!["atm".to_string(), "error".to_string()];
        let s = snippet("ATM diagnostic error codes explained", "nia-08.phk.txt", &words, 54);
        let shown: String = s.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(shown, "ATM diagnostic error codes explained");
        let s = snippet("Article explaining ATM error codes", "nia-08.phk", &words, 54);
        assert_eq!(s.iter().map(|(t, _)| t.as_str()).collect::<String>(), "Article explaining ATM error codes");
        let lit: Vec<&str> = s.iter().filter(|(_, l)| *l).map(|(t, _)| t.as_str()).collect();
        assert_eq!(lit, ["ATM", "error"]);
        // The name says why: nothing.
        assert!(snippet("A file of errors", "atm-error.txt", &words, 54).is_empty());
        // Far into a long one: cut round it, marked as cut.
        let long = format!("{} the ATM network {}", "word ".repeat(40), "more ".repeat(40));
        let s = snippet(&long, "x.txt", &["atm".to_string()], 30);
        let shown: String = s.iter().map(|(t, _)| t.as_str()).collect();
        assert!(shown.starts_with("..") && shown.ends_with("..") && shown.contains("ATM") && shown.chars().count() <= 30, "{shown:?}");
    }
}
