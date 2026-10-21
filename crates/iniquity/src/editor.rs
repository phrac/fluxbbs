//! The full-screen message editor (`FsEditor.pas`).
//!
//! [`Draft`] is the text model (typing, word wrap, joins, cut and paste);
//! [`Session::edit_message`] draws it inside the FSEDITOR screen and runs
//! the editor's commands (ESC, or `/` at the start of a line).

use std::io;

use chrono::NaiveDateTime;
use iniquity_term::{Color, Slot};

use crate::input::{Key, ReadOpts};
use crate::msgbase::Message;
use crate::session::{Session, Waves};

pub const MAX_LINES: usize = 200;
/// Longest line; the 80th column stays free so lines never wrap.
pub const WIDTH: usize = 79;
const TAB: usize = 4;

/// What a change needs redrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Only the cursor moved.
    Cursor,
    /// This line changed.
    Line(usize),
    /// This line changed from this column on.
    Tail(usize, usize),
    /// This line and everything below changed.
    From(usize),
}

#[derive(Debug, Clone)]
pub struct Draft {
    lines: Vec<Vec<char>>,
    pub row: usize,
    /// 0-based; may equal the line length (after the last character).
    pub col: usize,
    pub insert: bool,
    cut: Option<Vec<char>>,
}

impl Draft {
    pub fn new(text: &[String]) -> Self {
        let mut lines: Vec<Vec<char>> = text.iter().map(|l| l.chars().take(WIDTH).collect()).collect();
        lines.truncate(MAX_LINES);
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        Self { lines, row: 0, col: 0, insert: true, cut: None }
    }

    pub fn line(&self, i: usize) -> Option<String> {
        self.lines.get(i).map(|l| l.iter().collect())
    }

    fn cur(&mut self) -> &mut Vec<char> {
        &mut self.lines[self.row]
    }

    fn trim_cur(&mut self) {
        while self.lines[self.row].last() == Some(&' ') {
            self.lines[self.row].pop();
        }
    }

    fn clamp_col(&mut self) {
        self.col = self.col.min(self.lines[self.row].len());
    }

    /// The finished text: trailing spaces and blank lines removed.
    pub fn text(&self) -> Vec<String> {
        let mut out: Vec<String> = self.lines.iter().map(|l| l.iter().collect::<String>().trim_end().to_string()).collect();
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out
    }

    /// Insert lines at the cursor (quoting), leaving the cursor after them.
    pub fn insert_lines(&mut self, new: &[String]) {
        for l in new {
            if self.lines.len() >= MAX_LINES {
                break;
            }
            self.lines.insert(self.row, l.chars().take(WIDTH).collect());
            self.row += 1;
        }
        if self.row >= self.lines.len() {
            self.lines.push(Vec::new());
        }
        self.col = 0;
    }

    pub fn type_char(&mut self, c: char) -> Redraw {
        let row = self.row;
        let len = self.lines[row].len();
        if len >= WIDTH && (self.insert || self.col >= len) {
            if self.col < len || self.row + 1 >= MAX_LINES {
                return Redraw::Cursor; // full line, typing mid-line: ignore
            }
            // Word wrap: carry the last word down.
            self.cur().push(c);
            let line = &self.lines[row];
            let (keep, carry) = match line.iter().rposition(|&ch| ch == ' ') {
                Some(sp) if sp > 0 => (line[..sp].to_vec(), line[sp + 1..].to_vec()),
                _ => (line[..line.len() - 1].to_vec(), vec![c]),
            };
            self.lines[row] = keep;
            self.trim_cur();
            if row + 1 < self.lines.len() && self.lines[row + 1].is_empty() {
                self.lines[row + 1] = carry;
            } else {
                self.lines.insert(row + 1, carry);
                self.lines.truncate(MAX_LINES);
            }
            self.row += 1;
            self.col = self.lines[self.row].len();
            return Redraw::From(row);
        }
        let col = self.col;
        let insert = self.insert;
        let line = self.cur();
        if insert {
            line.insert(col, c);
        } else {
            while line.len() < col {
                line.push(' ');
            }
            if col < line.len() {
                line[col] = c;
            } else {
                line.push(c);
            }
        }
        self.col += 1;
        Redraw::Tail(row, col)
    }

    pub fn enter(&mut self) -> Redraw {
        if self.lines.len() >= MAX_LINES && self.row + 1 >= self.lines.len() {
            return Redraw::Cursor;
        }
        let row = self.row;
        let at = self.col.min(self.lines[row].len());
        let tail = self.lines[row].split_off(at);
        self.trim_cur();
        if self.lines.len() < MAX_LINES {
            self.lines.insert(row + 1, tail);
        } else {
            self.lines[row + 1].splice(0..0, tail);
        }
        self.row += 1;
        self.col = 0;
        Redraw::From(row)
    }

    pub fn backspace(&mut self) -> Redraw {
        if self.col > 0 {
            self.col -= 1;
            let col = self.col;
            self.cur().remove(col);
            return Redraw::Tail(self.row, col);
        }
        if self.row == 0 || self.lines[self.row - 1].len() + self.lines[self.row].len() > WIDTH {
            return Redraw::Cursor;
        }
        let line = self.lines.remove(self.row);
        self.row -= 1;
        self.col = self.lines[self.row].len();
        self.cur().extend(line);
        Redraw::From(self.row)
    }

    pub fn delete(&mut self) -> Redraw {
        let row = self.row;
        let len = self.lines[row].len();
        if self.col < len {
            let col = self.col;
            self.cur().remove(col);
            return Redraw::Tail(row, col);
        }
        if row + 1 >= self.lines.len() || len + self.lines[row + 1].len() > WIDTH {
            return Redraw::Cursor;
        }
        let next = self.lines.remove(row + 1);
        self.cur().extend(next);
        Redraw::From(row)
    }

    /// Ctrl-Y: delete the line. Ctrl-K: cut it for Ctrl-U.
    pub fn delete_line(&mut self, keep: bool) -> Redraw {
        let row = self.row;
        if keep {
            self.cut = Some(self.lines[row].clone());
        }
        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(row);
            if self.row >= self.lines.len() {
                self.row = self.lines.len() - 1;
            }
        }
        self.col = 0;
        Redraw::From(row)
    }

    pub fn paste(&mut self) -> Redraw {
        match self.cut.clone() {
            Some(l) if self.lines.len() < MAX_LINES => {
                self.lines.insert(self.row, l);
                self.col = 0;
                Redraw::From(self.row)
            }
            _ => Redraw::Cursor,
        }
    }

    /// Ctrl-B: split the line at the cursor without moving.
    pub fn break_line(&mut self) -> Redraw {
        if self.col >= self.lines[self.row].len() || self.lines.len() >= MAX_LINES {
            return Redraw::Cursor;
        }
        let row = self.row;
        let tail = self.lines[row].split_off(self.col);
        self.lines.insert(row + 1, tail);
        Redraw::From(row)
    }

    pub fn tab(&mut self) -> Redraw {
        if self.lines[self.row].len() + TAB >= WIDTH {
            return Redraw::Cursor;
        }
        let col = self.col.min(self.lines[self.row].len());
        self.cur().splice(col..col, std::iter::repeat_n(' ', TAB));
        self.col = col + TAB;
        Redraw::Line(self.row)
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.trim_cur();
            self.row -= 1;
            self.col = self.lines[self.row].len();
        }
    }

    pub fn right(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.col += 1;
        } else if self.row + 1 < MAX_LINES {
            self.trim_cur();
            self.row += 1;
            if self.row == self.lines.len() {
                self.lines.push(Vec::new());
            }
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row > 0 {
            self.trim_cur();
            self.row -= 1;
            self.clamp_col();
        }
    }

    pub fn down(&mut self) {
        if self.row + 1 < MAX_LINES {
            self.trim_cur();
            self.row += 1;
            if self.row == self.lines.len() {
                self.lines.push(Vec::new());
            }
            self.clamp_col();
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.lines[self.row].len();
    }

    pub fn page(&mut self, rows: isize) {
        self.trim_cur();
        self.row = (self.row as isize + rows).clamp(0, self.lines.len() as isize - 1) as usize;
        self.clamp_col();
    }
}

/// What's being written, for the editor's header and commands.
pub struct EditCtx<'a> {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: NaiveDateTime,
    pub anonymous: bool,
    /// The caller may toggle anonymous (`N`).
    pub can_anonymous: bool,
    /// The caller may toggle the auto-signature (`U`).
    pub can_autosig: bool,
    pub autosig: bool,
    /// The message being replied to, for quoting.
    pub reply: Option<&'a Message>,
    pub auto_quote: bool,
    /// Text to start with (editing an existing message).
    pub text: Vec<String>,
}

pub struct EditResult {
    pub lines: Vec<String>,
    pub subject: String,
    pub anonymous: bool,
    pub autosig: bool,
}

/// Screen layout from FSEDITOR.ANS (or the built-in header).
struct View {
    top: u16,
    rows: u16,
    /// First draft line on screen.
    first: usize,
    text: Color,
    quote: Color,
    /// Row used for command prompts.
    info: u16,
    has_header: bool,
}

fn is_quote(line: &str) -> bool {
    line.chars().take(3).any(|c| c == '>')
}

/// Initials for quote prefixes (`mInitials`).
fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).collect::<String>().to_uppercase()
}

/// The lines `fsQuotePos` would quote: the end of the message (before the
/// signature), up to `max` lines, skipping leading blanks and old quotes.
pub fn quote_range(m: &Message, max: usize) -> Option<(usize, usize)> {
    let body = m.body();
    let usable = |l: &String| !l.is_empty() && !l.contains('\x1b');
    let last = body.iter().rposition(usable)?;
    let mut first = last.saturating_sub(max.saturating_sub(1));
    while first < last && (!usable(&body[first]) || (body[first].contains('>') && body[first].len() <= 4)) {
        first += 1;
    }
    Some((first, last))
}

pub fn quote_lines(m: &Message, first: usize, last: usize) -> Vec<String> {
    let who = if m.anonymous { "??".to_string() } else { initials(&m.from.name) };
    m.body()[first..=last.min(m.body().len() - 1)]
        .iter()
        .map(|l| {
            let plain = strip_colours(l);
            let q = if plain.chars().take(3).any(|c| c == '>') { plain } else { format!("{who}> {plain}") };
            q.chars().take(WIDTH).collect()
        })
        .collect()
}

/// `NoColor`: remove `|XX` colour codes.
pub fn strip_codes(s: &str) -> String {
    let bytes = iniquity_term::cp437::encode(s);
    iniquity_term::tokenize(&bytes, iniquity_term::Source::String)
        .into_iter()
        .map(|t| match t {
            iniquity_term::Token::Text(b) => iniquity_term::cp437::decode(&b),
            iniquity_term::Token::Mci(c) => format!("%{}", String::from_utf8_lossy(&c)),
            _ => String::new(),
        })
        .collect()
}

/// Text a caller wrote, plain: its `|XX` colour codes and escape
/// sequences go, and everything else (`@12:30`, `{10}`, `%`) stays as
/// typed, which `strip_codes` (for screens' own text) doesn't keep.
pub fn strip_colours(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '|'
            && i + 2 < c.len()
            && c[i + 1].is_ascii()
            && c[i + 2].is_ascii()
            && iniquity_term::Control::parse([c[i + 1] as u8, c[i + 2] as u8]).is_some_and(|k| k.is_colour())
        {
            i += 3;
            continue;
        }
        if c[i] == '\x1b' {
            i += 1;
            if c.get(i) == Some(&'[') {
                i += 1;
                while i < c.len() && ('\x20'..'\x40').contains(&c[i]) {
                    i += 1;
                }
                i += 1;
            }
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// A message line made safe to print: characters that CP437 sends as
/// control bytes (`←` is ESC, `♪` CR, `•` BEL...) would act on the
/// reader's terminal rather than show, so they go (a bullet becomes `∙`).
pub fn printable(s: &str) -> std::borrow::Cow<'_, str> {
    let control = |c: char| c < ' ' || iniquity_term::cp437::TO_UNICODE[1..0x20].contains(&c);
    if !s.chars().any(control) {
        return std::borrow::Cow::Borrowed(s);
    }
    s.chars().filter_map(|c| if c == '\u{2022}' { Some('\u{2219}') } else if control(c) { None } else { Some(c) }).collect()
}

/// A message line with ANSI codes, as bytes for the terminal: colours
/// (`ESC[..m`) and cursor-right (`ESC[nC`, art's spacing) kept; any other
/// escape sequence, and control characters, left out. However the line
/// got in (radio, an uploaded file), it can't drive the reader's terminal.
/// A line as the editor shows it: one column a character, so the cursor
/// stays put. `write_text` drops control characters (an uploaded message's
/// escapes, say), and CP437's glyphs for them; ESC shows as `«`, the rest
/// as `?`.
fn on_screen(l: &str) -> std::borrow::Cow<'_, str> {
    let hidden = |c: char| c != '\u{2022}' && iniquity_term::cp437::encode(c.encode_utf8(&mut [0; 4])).first().is_some_and(|&b| b < 0x20 || b == 0x7f);
    if !l.chars().any(hidden) {
        return l.into();
    }
    l.chars().map(|c| if c == '\x1b' { '«' } else if hidden(c) { '?' } else { c }).collect::<String>().into()
}

pub fn ansi_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            text.push(c);
            continue;
        }
        out.extend(iniquity_term::cp437::encode(&printable(&text)));
        text.clear();
        if it.peek() != Some(&'[') {
            continue;
        }
        it.next();
        let mut params = String::new();
        let mut end = None;
        for d in it.by_ref() {
            if ('\x20'..'\x40').contains(&d) {
                params.push(d);
            } else {
                end = Some(d);
                break;
            }
        }
        if let Some(e @ ('m' | 'C')) = end
            && params.chars().all(|d| d.is_ascii_digit() || d == ';')
        {
            out.extend(format!("\x1b[{params}{e}").bytes());
        }
    }
    out.extend(iniquity_term::cp437::encode(&printable(&text)));
    out
}

impl Session {
    /// `fsEdit`: edit a message full-screen. None if aborted. Node
    /// messages wait until it's done, so they don't land in the text.
    pub async fn edit_message(&mut self, ctx: EditCtx<'_>) -> io::Result<Option<EditResult>> {
        let held = std::mem::replace(&mut self.hold_node_msgs, true);
        let result = self.edit_message_inner(ctx).await;
        self.hold_node_msgs = held;
        result
    }

    async fn edit_message_inner(&mut self, mut ctx: EditCtx<'_>) -> io::Result<Option<EditResult>> {
        let mut d = Draft::new(&ctx.text);
        if ctx.auto_quote
            && let Some(r) = ctx.reply {
                let max = self.state.config().messages.max_quote_lines as usize;
                if let Some((a, b)) = quote_range(r, max) {
                    d.insert_lines(&quote_lines(r, a, b));
                }
            }
        let mut v = self.fs_draw(&ctx, &d).await?;
        let saved = loop {
            let key = self.read_key().await?;
            let change = match key {
                Key::Left => {
                    d.left();
                    Redraw::Cursor
                }
                Key::Right => {
                    d.right();
                    Redraw::Cursor
                }
                Key::Up => {
                    d.up();
                    Redraw::Cursor
                }
                Key::Down => {
                    d.down();
                    Redraw::Cursor
                }
                Key::Home => {
                    d.home();
                    Redraw::Cursor
                }
                Key::End => {
                    d.end();
                    Redraw::Cursor
                }
                Key::PgUp => {
                    d.page(-(v.rows as isize - 4).max(1));
                    Redraw::Cursor
                }
                Key::PgDn => {
                    d.page((v.rows as isize - 4).max(1));
                    Redraw::Cursor
                }
                Key::Ins | Key::Char(0x16) => {
                    d.insert = !d.insert;
                    self.fs_flag(*b"IN", if d.insert { "On " } else { "Off" });
                    Redraw::Cursor
                }
                Key::Del | Key::Char(0x04) => d.delete(),
                Key::Backspace => d.backspace(),
                Key::Tab => d.tab(),
                Key::Enter => d.enter(),
                Key::Char(0x02) => d.break_line(),
                Key::Char(0x0B) => d.delete_line(true),
                Key::Char(0x19) => d.delete_line(false),
                Key::Char(0x15) => d.paste(),
                Key::Char(0x0C) => {
                    v = self.fs_draw(&ctx, &d).await?;
                    Redraw::Cursor
                }
                Key::Char(0x18) => break true, // Ctrl-X: save
                Key::Esc => match self.fs_command(&mut ctx, &mut d, &mut v).await? {
                    Some(save) => break save,
                    None => Redraw::Cursor,
                },
                Key::Char(b'/') if d.col == 0 => match self.fs_command(&mut ctx, &mut d, &mut v).await? {
                    Some(save) => break save,
                    None => Redraw::Cursor,
                },
                Key::Char(c) if crate::input::text_char(c).is_some() => d.type_char(crate::input::text_char(c).unwrap_or(' ')),
                Key::Char(c @ 0x80..=0xFF) => d.type_char(iniquity_term::cp437::TO_UNICODE[c as usize]),
                _ => Redraw::Cursor,
            };
            self.fs_render(&d, &mut v, change);
        };
        if !self.goto_pos(*b"30") {
            self.screen.goto(1, v.top + v.rows);
        }
        self.screen.set_color(self.user.palette.get(Slot::Text));
        self.newline();
        if !saved {
            return Ok(None);
        }
        Ok(Some(EditResult { lines: d.text(), subject: ctx.subject, anonymous: ctx.anonymous, autosig: ctx.autosig }))
    }

    /// `fsDrawScreen`: the header, then the visible text.
    async fn fs_draw(&mut self, ctx: &EditCtx<'_>, d: &Draft) -> io::Result<View> {
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        let header = self.show_file_with("FSEDITOR", &Waves::new()).await?;
        let rows = self.screen.rows();
        let mut v = View {
            top: 4,
            rows: rows.saturating_sub(6).max(5),
            first: 0,
            text: self.user.palette.get(Slot::TextHi),
            quote: self.user.palette.get(Slot::Text),
            info: 4,
            has_header: header,
        };
        let date = ctx.date.format("%m/%d/%y").to_string();
        let time = ctx.date.format("%-I:%M%P").to_string();
        if header {
            for (code, val) in [(*b"MT", &ctx.subject), (*b"SN", &ctx.from), (*b"RN", &ctx.to), (*b"DT", &date), (*b"TI", &time)] {
                if self.goto_pos(code) {
                    self.write_with_controls(&val.chars().take(40).collect::<String>());
                }
            }
            if let Some(&(_, y, c)) = self.positions.get(b"TL") {
                v.top = y;
                v.quote = c;
            }
            if let Some(&(_, y, c)) = self.positions.get(b"BL") {
                v.rows = y.saturating_sub(v.top) + 1;
                v.text = c;
            }
            v.info = self.positions.get(b"SL").map_or(v.top, |p| p.1);
            self.fs_flag(*b"AN", if ctx.anonymous { "Yes" } else { "No " });
            self.fs_flag(*b"AS", if ctx.autosig { "On " } else { "Off" });
            self.fs_flag(*b"IN", if d.insert { "On " } else { "Off" });
        } else {
            let s = format!(
                "|U9- |U6Iniquity FullScreen Message Editor |U9-------- |U4Date|U5: |U1{date:<10}|U4Time|U5: |U1{time}"
            );
            self.show_line(&s).await?;
            let s = format!(
                "|U9- |U4Anon|U5: |U1{:<4}|U4To|U5: |U1{:<26}|U4Subject|U5: |U1{}",
                if ctx.anonymous { "Yes" } else { "No" },
                iniquity_term::lit(&ctx.to.chars().take(25).collect::<String>()),
                iniquity_term::lit(&ctx.subject.chars().take(27).collect::<String>())
            );
            self.show_line(&s).await?;
            self.set_slot(Slot::Border);
            self.write(&"─".repeat(79));
            self.screen.goto(1, v.top + v.rows);
            self.write(&"─".repeat(79));
        }
        self.fs_scroll(d, &mut v);
        let first = v.first;
        self.fs_render(d, &mut v, Redraw::From(first));
        Ok(v)
    }

    fn fs_flag(&mut self, code: [u8; 2], val: &str) {
        let back = self.screen.where_x();
        let backy = self.screen.where_y();
        let color = self.screen.color();
        if self.goto_pos(code) {
            self.write(val);
            self.screen.goto(back, backy);
            self.screen.set_color(color);
        }
    }

    /// Keep the cursor's line on screen; true if the view moved.
    fn fs_scroll(&self, d: &Draft, v: &mut View) -> bool {
        let rows = v.rows as usize;
        let old = v.first;
        if d.row < v.first {
            v.first = d.row.saturating_sub(rows / 3);
        } else if d.row >= v.first + rows {
            v.first = d.row + rows / 3 + 1 - rows;
        }
        old != v.first
    }

    fn fs_line(&mut self, d: &Draft, v: &View, i: usize) {
        let y = v.top + (i - v.first) as u16;
        self.screen.goto(1, y);
        if let Some(l) = d.line(i) {
            self.screen.set_color(if is_quote(&l) { v.quote } else { v.text });
            self.write(&on_screen(&l));
        }
        self.screen.clear_eol();
    }

    /// Redraw what changed and put the cursor back.
    fn fs_render(&mut self, d: &Draft, v: &mut View, change: Redraw) {
        let change = if self.fs_scroll(d, v) { Redraw::From(v.first) } else { change };
        let last = v.first + v.rows as usize;
        match change {
            Redraw::Cursor => {}
            Redraw::Line(i) if (v.first..last).contains(&i) => self.fs_line(d, v, i),
            Redraw::Tail(i, col) if (v.first..last).contains(&i) => {
                let line = d.line(i).unwrap_or_default();
                self.screen.goto(col as u16 + 1, v.top + (i - v.first) as u16);
                self.screen.set_color(if is_quote(&line) { v.quote } else { v.text });
                self.write(&on_screen(&line.chars().skip(col).collect::<String>()));
                self.screen.clear_eol();
            }
            Redraw::Line(_) | Redraw::Tail(..) => {}
            Redraw::From(i) => {
                for i in i.max(v.first)..last {
                    self.fs_line(d, v, i);
                }
            }
        }
        self.fs_flag(*b"LN", &format!("{:03}", d.row + 1));
        self.fs_flag(*b"CN", &format!("{:02}", d.col + 1));
        let line = d.line(d.row).unwrap_or_default();
        self.screen.goto(d.col as u16 + 1, v.top + (d.row - v.first) as u16);
        self.screen.set_color(if is_quote(&line) { v.quote } else { v.text });
    }

    /// The command prompt (`fsGetCommand`). Some(true) save, Some(false)
    /// abort, None keep editing.
    async fn fs_command(&mut self, ctx: &mut EditCtx<'_>, d: &mut Draft, v: &mut View) -> io::Result<Option<bool>> {
        let has_cm = self.positions.contains_key(b"CM");
        self.fs_info(if has_cm { "FsCommandHelp" } else { "FsAskCommand" }, v).await?;
        if has_cm {
            self.goto_pos(*b"CM");
        }
        let quote_key = if ctx.reply.is_some() { 'Q' } else { '\x1b' };
        let cmd = loop {
            let key = self.read_key().await?;
            let c = match key {
                Key::Enter | Key::Esc => 'C',
                k => k.upper().unwrap_or('\0'),
            };
            if "CNSAT?U/".contains(c) || c == quote_key {
                break c;
            }
        };
        self.write(&cmd.to_string());
        let mut result = None;
        match cmd {
            'S' => {
                self.fs_info("FsSavingMsg", v).await?;
                return Ok(Some(true));
            }
            'A' => {
                self.fs_info("FsAskAbort", v).await?;
                if self.yes_no(false).await? {
                    return Ok(Some(false));
                }
                *v = self.fs_draw(ctx, d).await?;
            }
            '?' => {
                if self.show_file("FSHELP").await? {
                    self.prompt_key().await?;
                }
                *v = self.fs_draw(ctx, d).await?;
            }
            'T' => {
                self.fs_info("MsgAlterTitle", v).await?;
                if self.goto_pos(*b"MT") || !v.has_header {
                    if !v.has_header {
                        self.screen.goto(1, v.info);
                    }
                    let s = self.read_string(ReadOpts::new(40).prefill(ctx.subject.clone())).await?;
                    ctx.subject = if s.is_empty() { "None".into() } else { s };
                }
                *v = self.fs_draw(ctx, d).await?;
            }
            'N' if ctx.can_anonymous && !ctx.autosig => {
                ctx.anonymous = !ctx.anonymous;
                self.fs_flag(*b"AN", if ctx.anonymous { "Yes" } else { "No " });
            }
            'U' if ctx.can_autosig && !ctx.anonymous => {
                ctx.autosig = !ctx.autosig;
                self.fs_flag(*b"AS", if ctx.autosig { "On " } else { "Off" });
            }
            'Q' => {
                if let Some(r) = ctx.reply {
                    self.fs_quote(r, d).await?;
                }
                *v = self.fs_draw(ctx, d).await?;
            }
            '/' => result = Some(d.type_char('/')),
            _ => {}
        }
        self.fs_info_clear(d, v);
        if self.positions.contains_key(b"CM") {
            self.fs_flag(*b"CM", " ");
        }
        self.fs_render(d, v, result.unwrap_or(Redraw::Cursor));
        Ok(None)
    }

    async fn fs_info(&mut self, name: &str, v: &View) -> io::Result<()> {
        self.screen.goto(1, v.info);
        self.set_slot(Slot::Text);
        self.screen.clear_eol();
        let s = self.strings().get(name).to_string();
        self.show(&s).await
    }

    fn fs_info_clear(&mut self, d: &Draft, v: &View) {
        if (v.top..v.top + v.rows).contains(&v.info) {
            self.fs_line(d, v, v.first + (v.info - v.top) as usize);
        } else {
            self.screen.goto(1, v.info);
            self.screen.clear_eol();
        }
    }

    /// `gcQuoteMessage`: list the message being replied to and quote a
    /// range of its lines.
    async fn fs_quote(&mut self, r: &Message, d: &mut Draft) -> io::Result<()> {
        let body = r.body().to_vec();
        if body.is_empty() {
            return Ok(());
        }
        let ansi = self.state.config().messages.ansi_quote.clone();
        self.clear_screen();
        let mut lines = 0;
        for (i, l) in body.iter().enumerate() {
            let text = if l.contains('\x1b') { ansi.clone() } else { strip_colours(l).chars().take(74).collect() };
            self.show_line(&format!("|U4{:<3}|U5: |U1{}", i + 1, iniquity_term::lit(&printable(&text)))).await?;
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        self.newline();
        let defaults = self.state.config().messages.default_quote_lines && !self.user.quote;
        let suggest = quote_range(r, self.state.config().messages.max_quote_lines as usize);
        let n = body.len().to_string();
        let s = self.strings().fmt("MsgQuoteFirst", &[&n]);
        self.show(&s).await?;
        let pre = suggest.filter(|_| defaults).map(|(a, _)| (a + 1).to_string()).unwrap_or_default();
        let first: usize = self.read_string(ReadOpts::new(3).allowed(crate::input::DIGITS).prefill(pre)).await?.parse().unwrap_or(0);
        self.newline();
        if first < 1 || first > body.len() {
            return Ok(());
        }
        let s = self.strings().fmt("MsgQuoteSecond", &[&first.to_string(), &n]);
        self.show(&s).await?;
        let pre = suggest.filter(|&(_, b)| defaults && b + 1 >= first).map(|(_, b)| (b + 1).to_string()).unwrap_or_default();
        let s = self.read_string(ReadOpts::new(3).allowed(crate::input::DIGITS).prefill(pre)).await?;
        let last: usize = if s.is_empty() { first } else { s.parse().unwrap_or(0) };
        if last >= first && last <= body.len() {
            d.insert_lines(&quote_lines(r, first - 1, last - 1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(d: &mut Draft, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                d.enter();
            } else {
                d.type_char(c);
            }
        }
    }

    #[test]
    fn editor_lines_keep_their_width() {
        assert_eq!(on_screen("plain • text"), "plain • text");
        assert_eq!(on_screen("\x1b[31mred\u{263a}"), "«[31mred?");
    }

    #[test]
    fn ansi_lines_keep_colour_only() {
        let b = ansi_bytes("\x1b[1;31mred\x1b[0m\x1b[5C!\x1b]0;title\x07\x1b[2J\x1b[6n\u{2190}[H.");
        // What's left of a dropped sequence is plain text.
        assert_eq!(b, b"\x1b[1;31mred\x1b[0m\x1b[5C!]0;title[H.".to_vec());
    }

    #[test]
    fn typing_and_enter() {
        let mut d = Draft::new(&[]);
        typed(&mut d, "hello\nworld");
        assert_eq!(d.text(), vec!["hello", "world"]);
        assert_eq!((d.row, d.col), (1, 5));
    }

    #[test]
    fn word_wrap_carries_last_word() {
        let mut d = Draft::new(&[]);
        let long = "word ".repeat(15) + "abcd"; // 79 chars
        assert_eq!(long.len(), 79);
        typed(&mut d, &long);
        assert_eq!(d.row, 0);
        typed(&mut d, "e");
        assert_eq!(d.line(0).unwrap(), "word ".repeat(15).trim_end());
        assert_eq!(d.line(1).unwrap(), "abcde");
        assert_eq!((d.row, d.col), (1, 5));
    }

    #[test]
    fn wrap_without_spaces_breaks_mid_word() {
        let mut d = Draft::new(&[]);
        typed(&mut d, &"x".repeat(80));
        assert_eq!(d.line(0).unwrap().len(), 79);
        assert_eq!(d.line(1).unwrap(), "x");
    }

    #[test]
    fn enter_splits_and_backspace_joins() {
        let mut d = Draft::new(&["helloworld".into()]);
        d.col = 5;
        d.enter();
        assert_eq!(d.text(), vec!["hello", "world"]);
        d.backspace();
        assert_eq!(d.text(), vec!["helloworld"]);
        assert_eq!((d.row, d.col), (0, 5));
    }

    #[test]
    fn delete_at_end_joins_next() {
        let mut d = Draft::new(&["ab".into(), "cd".into()]);
        d.end();
        d.delete();
        assert_eq!(d.text(), vec!["abcd"]);
    }

    #[test]
    fn overwrite_mode() {
        let mut d = Draft::new(&["abc".into()]);
        d.insert = false;
        d.type_char('X');
        assert_eq!(d.text(), vec!["Xbc"]);
    }

    #[test]
    fn cut_and_paste() {
        let mut d = Draft::new(&["one".into(), "two".into()]);
        d.delete_line(true);
        assert_eq!(d.text(), vec!["two"]);
        d.down();
        d.paste();
        assert_eq!(d.text(), vec!["two", "one"]);
    }

    #[test]
    fn text_trims_trailing_blank_lines() {
        let mut d = Draft::new(&[]);
        typed(&mut d, "hi  \n\n\n");
        assert_eq!(d.text(), vec!["hi"]);
    }

    #[test]
    fn quoting() {
        let m = Message {
            area: "A".into(),
            num: 1,
            from: crate::msgbase::Party { name: "Mike Fricker".into(), ..Default::default() },
            to: Default::default(),
            subject: String::new(),
            posted: NaiveDateTime::default(),
            deleted: false,
            private: false,
            anonymous: false,
            replies: 0,
            sig_pos: 5,
            tag: 0,
            reply_to: 0,
            id: 0,
            lines: vec!["".into(), "first".into(), "|15second".into(), "".into(), "sig".into()],
        };
        assert_eq!(quote_range(&m, 11), Some((1, 2)));
        assert_eq!(quote_lines(&m, 1, 2), vec!["MF> first", "MF> second"]);
        assert_eq!(quote_range(&m, 1), Some((2, 2)));
    }
}
