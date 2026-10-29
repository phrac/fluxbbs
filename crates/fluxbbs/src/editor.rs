//! The full-screen message editor (`FsEditor.pas`).
//!
//! [`Draft`] is the text model (typing, word wrap, joins, cut and paste);
//! [`Session::edit_message`] draws it inside the FSEDITOR screen and runs
//! the editor's commands (ESC, or `/` at the start of a line).

use std::io;

use chrono::NaiveDateTime;
use fluxbbs_term::{Color, Slot};

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

/// What's typed after the `@` the cursor is in (none if the cursor isn't
/// in one, or nothing's typed yet).
fn mention_prefix(d: &Draft) -> Option<String> {
    let line: Vec<char> = d.line(d.row)?.chars().collect();
    let col = d.col.min(line.len());
    let at = (0..col).rev().find(|&i| line[i] == '@')?;
    if at > 0 && line[at - 1].is_alphanumeric() {
        return None;
    }
    let typed: String = line[at + 1..col].iter().collect();
    (!typed.is_empty() && !typed.ends_with("  ")).then_some(typed)
}

/// How many characters are typed after the `@`, and the names they
/// begin (there's more of each to come).
fn mention_matches<'a>(names: &'a [String], d: &Draft) -> (usize, Vec<&'a String>) {
    let Some(typed) = mention_prefix(d) else { return (0, Vec::new()) };
    let (lower, n) = (typed.to_lowercase(), typed.chars().count());
    (n, names.iter().filter(|h| h.to_lowercase().starts_with(&lower) && h.chars().count() > n).collect())
}

fn is_quote(line: &str) -> bool {
    line.chars().take(3).any(|c| c == '>')
}

/// Initials for quote prefixes (`mInitials`).
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

/// Quoted lines under `head` (who wrote them, `MsgQuoteHead`). New: a
/// plain `> ` in front, a quote of a quote `>>`, where the original put
/// the author's initials (`MF> `), which said too little.
pub fn quote_lines(m: &Message, first: usize, last: usize, head: &str) -> Vec<String> {
    let quoted = m.body()[first..=last.min(m.body().len() - 1)].iter().map(|l| {
        let plain = strip_colours(l);
        if plain.trim_start().starts_with('>') { format!(">{}", plain.trim_start()) } else { format!("> {plain}") }
    });
    (!head.is_empty()).then(|| head.to_string()).into_iter().chain(quote_flow(quoted)).collect()
}

/// Quoted lines too wide for the editor: the words that don't fit start
/// the paragraph's next line (the same `>`s, not indented), rewrapped
/// from there, rather than losing the line's end or standing alone.
fn quote_flow(lines: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut carry: Option<(String, String)> = None;
    for mut q in lines {
        let (lead, text) = quote_split(&q);
        if let Some((was, words)) = carry.take() {
            if was == lead && !text.is_empty() && !text.starts_with(char::is_whitespace) {
                q = format!("{lead}{words} {text}");
            } else {
                out.extend(quote_wrap(&format!("{was}{words}")));
            }
        }
        let (fits, over) = quote_cut(&q, lead.len());
        out.push(fits);
        carry = over.map(|w| (lead, w));
    }
    if let Some((was, words)) = carry {
        out.extend(quote_wrap(&format!("{was}{words}")));
    }
    out
}

/// A quoted line's `>`s (with the space after them), and its text.
fn quote_split(q: &str) -> (String, String) {
    let n = q.chars().take_while(|&c| c == '>').count();
    let text = &q[n..];
    (format!("{} ", &q[..n]), text.strip_prefix(' ').unwrap_or(text).to_string())
}

/// What of `q` fits the width, cut at a space where there is one, and the
/// words left over.
fn quote_cut(q: &str, lead: usize) -> (String, Option<String>) {
    let chars: Vec<char> = q.chars().collect();
    if chars.len() <= WIDTH {
        return (q.to_string(), None);
    }
    if lead >= WIDTH / 2 {
        return (chars[..WIDTH].iter().collect(), None);
    }
    let cut = chars[lead + 1..=WIDTH].iter().rposition(|&c| c == ' ').map_or(WIDTH, |i| i + lead + 1);
    let rest: String = chars[cut..].iter().collect::<String>().trim().to_string();
    (chars[..cut].iter().collect::<String>().trim_end().to_string(), (!rest.is_empty()).then_some(rest))
}

/// One quoted line over as many as it takes.
fn quote_wrap(q: &str) -> Vec<String> {
    let lead = quote_split(q).0;
    let mut out = Vec::new();
    let mut q = q.to_string();
    loop {
        let (fits, over) = quote_cut(&q, lead.len());
        out.push(fits);
        match over {
            Some(w) => q = format!("{lead}{w}"),
            None => return out,
        }
    }
}

/// `NoColor`: remove `|XX` colour codes.
pub fn strip_codes(s: &str) -> String {
    let bytes = fluxbbs_term::cp437::encode(s);
    fluxbbs_term::tokenize(&bytes, fluxbbs_term::Source::String)
        .into_iter()
        .map(|t| match t {
            fluxbbs_term::Token::Text(b) => fluxbbs_term::cp437::decode(&b),
            fluxbbs_term::Token::Mci(c) => format!("%{}", String::from_utf8_lossy(&c)),
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
            && fluxbbs_term::Control::parse([c[i + 1] as u8, c[i + 2] as u8]).is_some_and(|k| k.is_colour())
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
    let control = |c: char| c < ' ' || fluxbbs_term::cp437::TO_UNICODE[1..0x20].contains(&c);
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
    let hidden = |c: char| c != '\u{2022}' && fluxbbs_term::cp437::encode(c.encode_utf8(&mut [0; 4])).first().is_some_and(|&b| b < 0x20 || b == 0x7f);
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
        out.extend(fluxbbs_term::cp437::encode(&printable(&text)));
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
        // Colours the screen keeps track of, and a cursor-right of no more
        // than a line (ANSI art's spacing).
        match end {
            Some('m') if params.chars().all(|d| d.is_ascii_digit() || d == ';') => {
                if let Some(f) = fluxbbs_term::codes::sgr_filter(&params) {
                    out.extend(format!("\x1b[{f}m").bytes());
                }
            }
            Some('C') if params.chars().all(|d| d.is_ascii_digit()) => {
                let n = params.parse::<u32>().unwrap_or(1).clamp(1, 79);
                out.extend(format!("\x1b[{n}C").bytes());
            }
            _ => {}
        }
    }
    out.extend(fluxbbs_term::cp437::encode(&printable(&text)));
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
        if self.plain() {
            return self.plain_edit(&ctx).await;
        }
        let mut d = Draft::new(&ctx.text);
        if ctx.auto_quote
            && let Some(r) = ctx.reply {
                let max = self.state.config().messages.max_quote_lines as usize;
                if let Some((a, b)) = quote_range(r, max) {
                    let head = self.quote_head(r);
                    d.insert_lines(&quote_lines(r, a, b, &head));
                }
            }
        let mut v = self.fs_draw(&ctx, &d).await?;
        let mut hinted = self.fs_hint(&d, &mut v).await?;
        // New: `@` completes to a real handle (or a group a co-SysOp may
        // name); the best match shows dimmed after the cursor, Tab takes it.
        let names = self.mention_names();
        let mut ghost: Option<usize> = None;
        let saved = loop {
            let key = self.read_key().await?;
            if std::mem::take(&mut hinted) {
                self.fs_info_clear(&d, &v);
                self.fs_render(&d, &mut v, Redraw::Cursor);
            }
            if let Some(row) = ghost.take()
                && (v.first..v.first + v.rows as usize).contains(&row)
            {
                self.fs_line(&d, &v, row);
            }
            let (typed, matches) = mention_matches(&names, &d);
            let change = match key {
                Key::Tab if !matches.is_empty() => {
                    let row = d.row;
                    for c in matches[0].chars().skip(typed) {
                        d.type_char(c);
                    }
                    Redraw::From(row)
                }
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
                Key::Char(c @ 0x80..=0xFF) => d.type_char(fluxbbs_term::cp437::TO_UNICODE[c as usize]),
                _ => Redraw::Cursor,
            };
            self.fs_render(&d, &mut v, change);
            let (typed, matches) = mention_matches(&names, &d);
            if let Some(first) = matches.first() {
                let rest: String = first.chars().skip(typed).take(WIDTH.saturating_sub(d.col)).collect();
                let (x, y, color) = (self.screen.where_x(), self.screen.where_y(), self.screen.color());
                self.screen.set_color(v.quote);
                self.write(&on_screen(&rest));
                ghost = Some(d.row);
                let list = matches.iter().take(6).map(|h| format!("@{}", fluxbbs_term::lit(h))).collect::<Vec<_>>().join(" |U2· |U3");
                self.screen.goto(1, v.info);
                self.set_slot(Slot::Text);
                self.screen.clear_eol();
                let s = self.strings().fmt("FsMention", &[&list]);
                self.show(&s).await?;
                hinted = true;
                self.screen.goto(x, y);
                self.screen.set_color(color);
            }
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

    /// What `@` may complete to: every other caller's handle, and the
    /// groups (`@all`...) if this caller may name them.
    fn mention_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.state.users.handles().unwrap_or_default().into_iter().filter(|h| !h.eq_ignore_ascii_case(&self.user.handle)).collect();
        if crate::mentions::may_group(&self.state, &self.user) {
            names.extend(crate::mentions::GROUPS.iter().map(|g| g.to_string()));
        }
        names
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
        let posted = self.local(ctx.date);
        let date = posted.format("%m/%d/%y").to_string();
        let time = posted.format("%H:%M").to_string();
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
                "|U9- |U6flux/bbs FullScreen Message Editor |U9-------- |U4Date|U5: |U1{date:<10}|U4Time|U5: |U1{time}"
            );
            self.show_line(&s).await?;
            let s = format!(
                "|U9- |U4Anon|U5: |U1{:<4}|U4To|U5: |U1{:<26}|U4Subject|U5: |U1{}",
                if ctx.anonymous { "Yes" } else { "No" },
                fluxbbs_term::lit(&ctx.to.chars().take(25).collect::<String>()),
                fluxbbs_term::lit(&ctx.subject.chars().take(27).collect::<String>())
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
            if "CNSAT?U/L".contains(c) || c == quote_key {
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
            // New: a link to a file on the board, picked from a lightbar.
            'L' => {
                if let Some(link) = self.pick_file_link().await? {
                    for c in link.chars() {
                        d.type_char(c);
                    }
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

    /// New: a line in the prompt row as the editor opens, until a key:
    /// the caller's `editor_note`, or one of the `FsHint` strings (the
    /// board's mottos). False if there was none.
    async fn fs_hint(&mut self, d: &Draft, v: &mut View) -> io::Result<bool> {
        let line = match self.editor_note.take() {
            Some(n) => n,
            None => {
                let hints: Vec<String> = (1..)
                    .map(|k| format!("FsHint{k}"))
                    .take_while(|k| !self.strings().stock(k).is_empty())
                    .map(|k| self.strings().get(&k).to_string())
                    .filter(|s| !s.trim().is_empty())
                    .collect();
                if hints.is_empty() {
                    return Ok(false);
                }
                hints[rand::random::<u32>() as usize % hints.len()].clone()
            }
        };
        self.screen.goto(1, v.info);
        self.set_slot(Slot::Text);
        self.screen.clear_eol();
        self.show(&line).await?;
        self.fs_render(d, v, Redraw::Cursor);
        Ok(true)
    }

    /// The line over a quote: who wrote it.
    fn quote_head(&self, r: &Message) -> String {
        let who = if r.anonymous { self.strings().get("Anonymous").to_string() } else { r.from.name.clone() };
        self.strings().get("MsgQuoteHead").replace("%S1", &who)
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
            self.show_line(&format!("|U4{:<3}|U5: |U1{}", i + 1, fluxbbs_term::lit(&printable(&text)))).await?;
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
            let head = self.quote_head(r);
            d.insert_lines(&quote_lines(r, first - 1, last - 1, &head));
        }
        Ok(())
    }
}

/// Text without its ANSI escape sequences (which take no room).
pub(crate) fn strip_escapes(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && b.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                i += 1;
            }
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
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
        assert_eq!(ansi_bytes("\x1b[65535Cx\x1b[8my"), b"\x1b[79Cxy".to_vec(), "a line at most; no hidden text");
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
        let mut m = Message {
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
        assert_eq!(quote_lines(&m, 1, 2, "Mad Fiend wrote:"), vec!["Mad Fiend wrote:", "> first", "> second"]);
        m.lines[1] = "> older".into();
        assert_eq!(quote_lines(&m, 1, 1, ""), vec![">> older"]);
        assert_eq!(quote_range(&m, 1), Some((2, 2)));
        // Too wide with the `> ` in front: what doesn't fit starts the
        // next line of the paragraph, not a line of its own.
        let mut m2 = m.clone();
        m2.sig_pos = 0;
        m2.lines = vec![
            "But - putting together (getting Claude to put together) my own BBS software is".into(),
            "possibly the biggest piece of string I have ever encountered. Where do I stop".into(),
            "with it? Short.".into(),
            "".into(),
            "Next paragraph.".into(),
        ];
        assert_eq!(
            quote_lines(&m2, 0, 4, ""),
            vec![
                "> But - putting together (getting Claude to put together) my own BBS software",
                "> is possibly the biggest piece of string I have ever encountered. Where do I",
                "> stop with it? Short.",
                "> ",
                "> Next paragraph.",
            ]
        );
        // The last line of a paragraph, or code, isn't run into.
        m2.lines = vec![format!("{} tail", "word ".repeat(15)), "    indented".into()];
        assert_eq!(quote_lines(&m2, 0, 1, "").len(), 3);
        m.lines[1] = format!("> {}", "x".repeat(100));
        let q = quote_lines(&m, 1, 1, "");
        assert_eq!(q, vec![format!(">> {}", "x".repeat(WIDTH - 3)), format!(">> {}", "x".repeat(100 - (WIDTH - 3)))]);
        m.lines[1] = format!("{}trailing{}", "a ".repeat(36), " ".repeat(20));
        assert!(quote_lines(&m, 1, 1, "").iter().all(|l| l.chars().count() <= WIDTH && l.trim() != ">"));
    }

    #[test]
    fn at_completes_to_a_handle() {
        let names: Vec<String> = ["Dark Angel", "phrac", "phreak", "all"].iter().map(|s| s.to_string()).collect();
        let mut d = Draft::new(&["hey @ph".to_string()]);
        d.end();
        let (n, m) = mention_matches(&names, &d);
        assert_eq!((n, m), (2, vec![&names[1], &names[2]]));
        // Case doesn't matter; a name with a space completes too.
        let mut d = Draft::new(&["@dark a".to_string()]);
        d.end();
        assert_eq!(mention_matches(&names, &d).1, vec![&names[0]]);
        // Nothing typed yet, a complete name, an address, or not at the cursor.
        for line in ["@", "@phrac", "me@phrac", "@phrac is here"] {
            let mut d = Draft::new(&[line.to_string()]);
            d.end();
            assert!(mention_matches(&names, &d).1.is_empty(), "{line}");
        }
        let mut d = Draft::new(&["@ph later".to_string()]);
        d.end();
        assert!(mention_matches(&names, &d).1.is_empty(), "the cursor has moved on");
    }
}
