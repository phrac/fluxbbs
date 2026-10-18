//! The SysOp editors' screen (CONFIG.PAS): a titled form of hotkeyed
//! items with a lightbar, an info line under them, and fields edited in
//! place. Each editor builds a [`Form`], draws it, and loops on
//! [`Session::cfg_command`].

use std::collections::BTreeSet;
use std::io;

use iniquity_term::{Color, Slot};
use iniquity_term::codes::{resize, squish};

use crate::input::{Case, Key};
use crate::session::Session;

/// `cfgTop`: the first item row.
const TOP: u16 = 4;
/// `cfgCol`: where values start.
const VALUE_COL: u16 = 30;

pub struct Item {
    /// None for a label-only line (`--Current area`).
    key: Option<char>,
    label: String,
    x: u16,
    y: u16,
    /// Value column and width; width 0 means a command with no value.
    vx: u16,
    len: usize,
    value: String,
    info: String,
}

pub struct Form {
    title: String,
    items: Vec<Item>,
    /// Index of the highlighted item.
    bar: usize,
    /// Rows used by sequential items (`cfgBot`).
    rows: u16,
    /// Width of the text on the info line, to blank it.
    info_len: usize,
    /// Value column for [`Form::item`] (`cfgCol`).
    col: u16,
}

/// What a text field accepts.
#[derive(Clone, Copy)]
pub struct Field {
    pub case: Case,
    /// Allowed characters; empty = any printable.
    pub allowed: &'static str,
    /// Longest value; 0 = the item's width.
    pub max: usize,
}

impl Field {
    pub const TEXT: Field = Field { case: Case::Normal, allowed: "", max: 0 };
    pub const UPPER: Field = Field { case: Case::Upper, allowed: "", max: 0 };
    pub const LOWER: Field = Field { case: Case::Lower, allowed: "", max: 0 };
    pub const NUMBER: Field = Field { case: Case::Normal, allowed: "0123456789", max: 0 };
    /// A storage key: letters, digits, `_` and `-`.
    pub const KEY: Field =
        Field { case: Case::Upper, allowed: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-", max: 0 };

    pub fn max(mut self, n: usize) -> Self {
        self.max = n;
        self
    }
}

impl Form {
    pub fn new(title: &str) -> Self {
        Self { title: title.to_string(), items: Vec::new(), bar: 0, rows: 0, info_len: 0, col: VALUE_COL }
    }

    /// Column where later [`Form::item`] values start.
    pub fn value_col(&mut self, col: u16) {
        self.col = col;
    }

    /// Leave `n` rows for the editor to draw in itself.
    pub fn gap(&mut self, n: u16) {
        self.rows += n;
    }

    /// The row the next [`Form::item`] goes on.
    pub fn next_row(&self) -> u16 {
        TOP + self.rows
    }

    /// An item beside the previous one on the same row.
    pub fn item_beside(&mut self, x: u16, key: char, label: &str, len: usize, value: impl Into<String>, info: &str) {
        let y = TOP + self.rows.saturating_sub(1);
        let vx = x + 5 + label.chars().count() as u16;
        self.push(Some(key), label, x, y, vx, len, value.into(), info);
    }

    /// `cfgItem`: the next item down the left column.
    pub fn item(&mut self, key: char, label: &str, len: usize, value: impl Into<String>, info: &str) {
        let y = TOP + self.rows;
        self.rows += 1;
        self.push(Some(key), label, 1, y, self.col, len, value.into(), info);
    }

    /// An item anywhere (`cfgItemXY`): key box at `x`, value at `vx`, on
    /// row `y` counting from 1.
    #[allow(clippy::too_many_arguments)]
    pub fn item_xy(&mut self, x: u16, y: u16, vx: u16, key: char, label: &str, len: usize, value: impl Into<String>, info: &str) {
        self.push(Some(key), label, x, TOP + y - 1, vx, len, value.into(), info);
    }

    /// A line that shows a value but can't be picked.
    pub fn label(&mut self, label: &str, len: usize, value: impl Into<String>) {
        let y = TOP + self.rows;
        self.rows += 1;
        self.push(None, label, 1, y, self.col, len, value.into(), "");
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, key: Option<char>, label: &str, x: u16, y: u16, vx: u16, len: usize, value: String, info: &str) {
        self.items.push(Item { key, label: label.to_string(), x, y, vx, len, value, info: info.to_string() });
        if self.items[self.bar].key.is_none() {
            self.bar = self.items.len() - 1;
        }
    }

    /// Change an item's value (redrawn by [`Session::cfg_set`]).
    pub fn set(&mut self, key: char, value: impl Into<String>) {
        if let Some(i) = self.find(key) {
            self.items[i].value = value.into();
        }
    }

    fn find(&self, key: char) -> Option<usize> {
        self.items.iter().position(|i| i.key == Some(key))
    }

    /// Row below the last item: the bottom bar and info line.
    fn bar_row(&self) -> u16 {
        let last = self.items.iter().map(|i| i.y).max().unwrap_or(TOP - 1);
        last.max(TOP + self.rows.saturating_sub(1)) + 2
    }

    /// `cfgKeyUp` and friends: the nearest pickable item that way.
    fn nearest(&self, dx: i32, dy: i32) -> Option<usize> {
        let cur = &self.items[self.bar];
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.key.is_some())
            .filter_map(|(n, i)| {
                let (ix, iy, cx, cy) = (i.x as i32, i.y as i32, cur.x as i32, cur.y as i32);
                let d = if dy != 0 { (iy - cy) * dy } else { (ix - cx) * dx };
                let same_line = if dy != 0 { ix == cx } else { iy == cy };
                (same_line && d > 0).then_some((d, n))
            })
            .min()
            .map(|(_, n)| n)
    }
}

impl Session {
    /// `cfgInit` + items + `cfgBar` + `cfgDrawAllItems`.
    pub fn cfg_draw(&mut self, f: &Form) {
        self.full_screen_done();
        self.set_slot(Slot::Text);
        self.clear_screen();
        self.write_with_controls("|U5▐▄");
        self.screen.move_right(75);
        self.write_with_controls("▄▌");
        let title = format!("- {} -", f.title);
        self.screen.goto(40u16.saturating_sub(title.chars().count() as u16 / 2).max(1), 1);
        self.write_with_controls(&format!("|U4{title}"));
        self.screen.goto(1, 2);
        self.write_with_controls(
            "|U5 ▀▓▄▄▄▄▄▄▓■[|U4command:       |U5]■▄▄▄▄▄▄▄▄▄▄■▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄■[|U4esc|U5/|U4quit|U5]■▓▄▄▄▄▄▄▓▀",
        );
        for i in 0..f.items.len() {
            self.cfg_draw_key(f, i);
            let it = &f.items[i];
            let label = it.label.clone();
            self.set_slot(Slot::Info);
            self.write(&format!(" {label}"));
            self.cfg_draw_value(f, i);
        }
        self.screen.goto(1, f.bar_row());
        self.write_with_controls("|U5▀■|U4[");
        self.screen.move_right(73);
        self.write_with_controls("]|U5■▀");
    }

    /// `cfgBarOn` / `cfgBarOff`, or `---` for a label line.
    fn cfg_draw_key(&mut self, f: &Form, i: usize) {
        let it = &f.items[i];
        self.screen.goto(it.x, it.y);
        match it.key {
            None => {
                self.set_slot(Slot::InfoLo);
                self.write("---");
            }
            Some(k) if i == f.bar => {
                self.set_slot(Slot::ItemSel);
                self.write(&format!(" {k} "));
            }
            Some(k) => {
                self.set_slot(Slot::InfoLo);
                self.write("[");
                self.set_slot(Slot::InfoHi);
                self.write(&k.to_string());
                self.set_slot(Slot::InfoLo);
                self.write("]");
            }
        }
    }

    fn cfg_draw_value(&mut self, f: &Form, i: usize) {
        let it = &f.items[i];
        if it.len == 0 {
            return;
        }
        let shown = resize(&squish(&it.value, it.len), it.len);
        self.screen.goto(it.vx, it.y);
        self.set_slot(Slot::Text);
        self.write_with_controls(&shown);
    }

    /// Redraw an item's value in a colour of its own (colour pickers).
    pub fn cfg_paint(&mut self, f: &Form, key: char, color: Color) {
        let Some(i) = f.find(key) else { return };
        let it = &f.items[i];
        self.screen.goto(it.vx, it.y);
        self.screen.set_color(color);
        self.write(&resize(&it.value, it.len));
    }

    /// `cfgSetItem`: store and redraw a value.
    pub fn cfg_set(&mut self, f: &mut Form, key: char, value: impl Into<String>) {
        f.set(key, value);
        if let Some(i) = f.find(key) {
            self.cfg_draw_value(f, i);
        }
    }

    /// `cfgInfo`: text on the bottom bar (empty clears it).
    pub fn cfg_info(&mut self, f: &mut Form, text: &str) {
        let len = text.chars().count();
        self.screen.goto(4, f.bar_row());
        self.set_slot(Slot::InfoHi);
        self.write(&format!("{text:<width$}", width = f.info_len.max(len).min(72)));
        f.info_len = len;
    }

    fn cfg_move_bar(&mut self, f: &mut Form, to: usize) {
        if to == f.bar {
            return;
        }
        let old = std::mem::replace(&mut f.bar, to);
        self.cfg_draw_key(f, old);
        self.cfg_draw_key(f, to);
    }

    /// `cfgPromptCommand`: wait for an item's key (or Enter on the bar).
    /// None when the SysOp quits with Esc (or Q, if no item uses it).
    pub async fn cfg_command(&mut self, f: &mut Form) -> io::Result<Option<char>> {
        let has_q = f.find('Q').is_some();
        loop {
            self.screen.goto(22, 2);
            self.set_slot(Slot::InfoHi);
            self.write(" ");
            self.screen.move_left(1);
            let key = self.read_key().await?;
            let dir = match key {
                Key::Up => Some((0, -1)),
                Key::Down => Some((0, 1)),
                Key::Left => Some((-1, 0)),
                Key::Right => Some((1, 0)),
                _ => None,
            };
            if let Some((dx, dy)) = dir {
                if let Some(n) = f.nearest(dx, dy) {
                    self.cfg_move_bar(f, n);
                }
                continue;
            }
            let pick = match key {
                Key::Esc => None,
                Key::Enter => f.items[f.bar].key,
                k => match k.upper() {
                    Some('Q') if !has_q => None,
                    Some(c) if f.find(c).is_some() => Some(c),
                    _ => continue,
                },
            };
            self.screen.goto(22, 2);
            self.set_slot(Slot::InfoHi);
            match pick {
                None => {
                    self.write("Quit");
                    return Ok(None);
                }
                Some(c) => {
                    self.write(&c.to_string());
                    if let Some(n) = f.find(c) {
                        self.cfg_move_bar(f, n);
                    }
                    return Ok(Some(c));
                }
            }
        }
    }

    /// `cfgReadInfo`: edit an item's value in place. The form keeps what
    /// was entered; None (and the old value redrawn) if aborted with Esc.
    pub async fn cfg_text(&mut self, f: &mut Form, key: char, field: Field) -> io::Result<Option<String>> {
        let Some(i) = f.find(key) else { return Ok(None) };
        let info = f.items[i].info.clone();
        if !info.is_empty() {
            self.cfg_info(f, &info);
        }
        let (x, y, len) = (f.items[i].vx, f.items[i].y, f.items[i].len);
        let max = if field.max == 0 { len } else { field.max };
        let value = f.items[i].value.clone();
        let result = self.edit_line(x, y, len.max(1), &value, Field { max, ..field }).await?;
        if !info.is_empty() {
            self.cfg_info(f, "");
        }
        if let Some(v) = &result {
            f.items[i].value = v.clone();
        }
        self.cfg_draw_value(f, i);
        Ok(result)
    }

    /// Edit a line of text in place, `width` columns wide (the text
    /// scrolls if it's longer). Enter keeps it, Esc gives None.
    pub async fn edit_line(&mut self, x: u16, y: u16, width: usize, initial: &str, field: Field) -> io::Result<Option<String>> {
        let max = if field.max == 0 { width } else { field.max };
        let mut s: String = initial.chars().take(max).collect();
        loop {
            // Show the tail when the text is wider than the field.
            let skip = s.chars().count().saturating_sub(width - 1);
            let visible: String = s.chars().skip(skip).collect();
            self.screen.goto(x, y);
            self.set_slot(Slot::TextHi);
            self.write(&format!("{visible:<width$}"));
            self.screen.goto(x + visible.chars().count() as u16, y);
            match self.read_key().await? {
                Key::Enter => return Ok(Some(s.trim_end().to_string())),
                Key::Esc => return Ok(None),
                Key::Backspace => {
                    s.pop();
                }
                Key::Char(0x15 | 0x18 | 0x19) => s.clear(),
                Key::Char(c @ 0x20..=0x7E) if s.chars().count() < max => {
                    let c = field.case.apply(&s, c as char);
                    if (field.allowed.is_empty() || field.allowed.contains(c)) && !(c == ' ' && s.is_empty()) {
                        s.push(c);
                    }
                }
                Key::Char(c @ 0x80..=0xFF) if s.chars().count() < max && field.allowed.is_empty() => {
                    s.push(iniquity_term::cp437::TO_UNICODE[c as usize]);
                }
                _ => {}
            }
        }
    }

    /// A number field, clamped to `min..=max`. None if aborted.
    pub async fn cfg_number(&mut self, f: &mut Form, key: char, min: u64, max: u64) -> io::Result<Option<u64>> {
        let width = max.to_string().len();
        let Some(s) = self.cfg_text(f, key, Field::NUMBER.max(width)).await? else { return Ok(None) };
        let n = s.parse::<u64>().unwrap_or(min).clamp(min, max);
        self.cfg_set(f, key, n.to_string());
        Ok(Some(n))
    }

    /// `cfgReadOption`: cycle through choices with space or the arrows.
    pub async fn cfg_option(&mut self, f: &mut Form, key: char, choices: &[&str], cur: usize) -> io::Result<usize> {
        let Some(i) = f.find(key) else { return Ok(cur) };
        let info = format!("[space/arrows: select; enter: done] {}", f.items[i].info);
        self.cfg_info(f, &info);
        let (x, y, len) = (f.items[i].vx, f.items[i].y, f.items[i].len);
        let mut pick = cur.min(choices.len().saturating_sub(1));
        let done = loop {
            self.screen.goto(x, y);
            self.set_slot(Slot::TextHi);
            self.write(&resize(choices[pick], len));
            match self.read_key().await? {
                Key::Enter => break pick,
                Key::Esc => break cur,
                Key::Char(b' ') | Key::Right | Key::Up => pick = (pick + 1) % choices.len(),
                Key::Left | Key::Down => pick = (pick + choices.len() - 1) % choices.len(),
                _ => {}
            }
        };
        self.cfg_info(f, "");
        self.cfg_set(f, key, choices[done]);
        Ok(done)
    }

    /// `cfgReadBoolean`: a Yes/No bar over the value.
    pub async fn cfg_bool(&mut self, f: &mut Form, key: char, cur: bool) -> io::Result<bool> {
        let Some(i) = f.find(key) else { return Ok(cur) };
        let info = f.items[i].info.clone();
        if !info.is_empty() {
            self.cfg_info(f, &info);
        }
        let (x, y) = (f.items[i].vx.saturating_sub(1).max(1), f.items[i].y);
        let mut yes = cur;
        let answer = loop {
            self.screen.goto(x, y);
            self.write_with_controls(if yes { "|U5[|U6Yes|U5]|U4 No " } else { "|U4 Yes |U5[|U6No|U5]" });
            let k = self.read_key().await?;
            match (k, k.upper()) {
                (Key::Enter, _) => break yes,
                (Key::Esc, _) => break cur,
                (_, Some('Y')) => break true,
                (_, Some('N')) => break false,
                (Key::Left | Key::Right, _) | (_, Some(' ')) => yes = !yes,
                _ => {}
            }
        };
        self.screen.goto(x, y);
        self.write("          ");
        self.cfg_info(f, "");
        self.cfg_set(f, key, yes_no(answer));
        Ok(answer)
    }

    /// A yes/no question on the info line.
    pub async fn cfg_ask(&mut self, f: &mut Form, question: &str, default: bool) -> io::Result<bool> {
        self.cfg_info(f, question);
        let mut yes = default;
        let x = 4 + question.chars().count() as u16 + 1;
        let row = f.bar_row();
        let answer = loop {
            self.screen.goto(x, row);
            self.write_with_controls(if yes { "|U5[|U6Yes|U5]|U4 No " } else { "|U4 Yes |U5[|U6No|U5]" });
            let k = self.read_key().await?;
            match (k, k.upper()) {
                (Key::Enter, _) => break yes,
                (Key::Esc, _) => break false,
                (_, Some('Y')) => break true,
                (_, Some('N')) => break false,
                (Key::Left | Key::Right, _) | (_, Some(' ')) => yes = !yes,
                _ => {}
            }
        };
        f.info_len = f.info_len.max(question.chars().count() + 10);
        self.cfg_info(f, "");
        Ok(answer)
    }

    /// Ask for text on the info line (`cfgInfo` then `iReadString`).
    pub async fn cfg_read_info(&mut self, f: &mut Form, question: &str, max: usize) -> io::Result<String> {
        self.cfg_info(f, question);
        self.screen.goto(4 + question.chars().count() as u16, f.bar_row());
        self.set_slot(Slot::TextHi);
        let s = self.read_string(crate::input::ReadOpts::new(max)).await?;
        f.info_len = f.info_len.max(question.chars().count() + max);
        self.cfg_info(f, "");
        Ok(s)
    }

    /// `cfgReadColor`: up/down change the foreground, left/right the
    /// background. Returns the colour picked (the old one on Esc).
    pub async fn cfg_color(&mut self, f: &mut Form, key: char, cur: Color) -> io::Result<Color> {
        let Some(i) = f.find(key) else { return Ok(cur) };
        self.cfg_info(f, "[up/down: foreground; left/right: background; enter: done; esc: abort]");
        let (x, y, len) = (f.items[i].vx, f.items[i].y, f.items[i].len);
        let mut c = cur;
        let done = loop {
            self.screen.goto(x, y);
            self.screen.set_color(c);
            self.write(&resize(&color_name(c), len));
            match self.read_key().await? {
                Key::Enter => break c,
                Key::Esc => break cur,
                Key::Up => c.fore = (c.fore + 1) % 16,
                Key::Down => c.fore = (c.fore + 15) % 16,
                Key::Right => c.back = (c.back + 1) % 8,
                Key::Left => c.back = (c.back + 7) % 8,
                _ => {}
            }
        };
        self.cfg_info(f, "");
        self.screen.goto(x, y);
        self.screen.set_color(done);
        self.write(&resize(&color_name(done), len));
        f.set(key, color_name(done));
        Ok(done)
    }

    /// `cfgReadFlags`: toggle flags A-Z with their letters, or move with
    /// left/right and toggle with space.
    pub async fn cfg_flags(&mut self, f: &mut Form, key: char, flags: &mut BTreeSet<char>) -> io::Result<()> {
        let Some(i) = f.find(key) else { return Ok(()) };
        let info = format!("[left/right: select; space/a-z: toggle; enter: done] {}", f.items[i].info);
        self.cfg_info(f, &info);
        let (x, y) = (f.items[i].vx, f.items[i].y);
        let mut cur = 0u8;
        loop {
            self.screen.goto(x, y);
            self.set_slot(Slot::TextHi);
            self.write(&flags_text(flags));
            self.screen.goto(x + 1 + cur as u16, y);
            let k = self.read_key().await?;
            match (k, k.upper()) {
                (Key::Enter | Key::Esc, _) => break,
                (Key::Left, _) => cur = (cur + 25) % 26,
                (Key::Right, _) => cur = (cur + 1) % 26,
                (_, Some(c @ 'A'..='Z')) => {
                    cur = c as u8 - b'A';
                    toggle(flags, c);
                }
                (_, Some(' ')) => toggle(flags, (b'A' + cur) as char),
                _ => {}
            }
        }
        self.cfg_info(f, "");
        self.cfg_set(f, key, flags_text(flags));
        Ok(())
    }

    /// `cfgPrompt`: a command line under a list, e.g.
    /// `-- [I]nsert [D]elete [Esc]Quit ∙ Command: `. Returns the key picked
    /// (upper-cased), or None for Esc. Keys with an empty label work but
    /// aren't shown (`[1]-5 Edit` covers 2 to 5).
    pub async fn cfg_list_prompt(&mut self, cmds: &[(&str, &str)]) -> io::Result<Option<char>> {
        let mut s = String::from("|U2-- ");
        for (k, rest) in cmds.iter().filter(|(_, rest)| !rest.is_empty()) {
            s.push_str(&format!("|U2[|U3{k}|U2]|U1{rest} "));
        }
        s.push_str("|U2∙ |U4Command|U5: |U6");
        self.write_with_controls(&s);
        let keys: Vec<char> = cmds.iter().filter(|(k, _)| k.len() == 1).filter_map(|(k, _)| k.chars().next()).collect();
        loop {
            let k = self.read_key().await?;
            if k == Key::Esc {
                self.write("Quit");
                self.newline();
                return Ok(None);
            }
            if let Some(c) = k.upper().filter(|c| keys.contains(c)) {
                self.write(&c.to_string());
                self.newline();
                return Ok(Some(c));
            }
        }
    }

    /// Ask for a number in `min..=max` on its own line. None if blank or
    /// out of range.
    pub async fn cfg_ask_number(&mut self, question: &str, min: u32, max: u32) -> io::Result<Option<u32>> {
        self.write_with_controls(&format!("|U1{question} |U5[|U6{min}|U5-|U6{max}|U5]: |U3"));
        let s = self.read_string(crate::input::ReadOpts::new(max.to_string().len()).allowed(crate::input::DIGITS)).await?;
        self.newline();
        Ok(s.parse::<u32>().ok().filter(|n| (min..=max).contains(n)))
    }
}

fn toggle(flags: &mut BTreeSet<char>, c: char) {
    if !flags.remove(&c) {
        flags.insert(c);
    }
}

/// `cfgFlags`: `[AbcD...]`, upper-case for the flags that are set.
pub fn flags_text(flags: &BTreeSet<char>) -> String {
    let set: String = ('A'..='Z').map(|c| if flags.contains(&c) { c } else { c.to_ascii_lowercase() }).collect();
    format!("[{set}]")
}

/// `strColor`: "Light cyan on blue".
pub fn color_name(c: Color) -> String {
    const NAMES: [&str; 16] = [
        "Black", "Blue", "Green", "Cyan", "Red", "Magenta", "Brown", "Light gray",
        "Dark gray", "Light blue", "Light green", "Light cyan", "Light red", "Light magenta", "Yellow", "White",
    ];
    let blink = if c.blink { ", blinking" } else { "" };
    format!("{} on {}{blink}", NAMES[c.fore as usize & 15], NAMES[c.back as usize & 7].to_ascii_lowercase())
}

pub fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_flags() {
        assert_eq!(color_name(Color::new(11, 1)), "Light cyan on blue");
        let flags: BTreeSet<char> = ['A', 'C'].into();
        assert_eq!(flags_text(&flags), "[AbCdefghijklmnopqrstuvwxyz]");
    }

    #[test]
    fn bar_skips_labels_and_moves_by_column() {
        let mut f = Form::new("Test");
        f.label("Current area", 3, "1");
        f.item('A', "Name", 10, "x", "");
        f.item('B', "Path", 10, "y", "");
        f.item_beside(40, 'C', "Other", 5, "z", "");
        assert_eq!(f.bar, 1, "starts on the first pickable item");
        assert_eq!(f.nearest(0, 1), Some(2));
        assert_eq!(f.nearest(0, -1), None, "the label can't be picked");
        assert_eq!(f.nearest(1, 0), None, "C sits beside B, not A");
        f.bar = 2;
        assert_eq!(f.nearest(1, 0), Some(3));
        f.bar = 3;
        assert_eq!(f.nearest(-1, 0), Some(2));
        assert_eq!(f.bar_row(), TOP + 2 + 2);
    }
}
