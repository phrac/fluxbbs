//! New: vim keys for the message editor, for callers who pick them in
//! user config (`editor_keys`). It starts in normal mode, as vim does,
//! and has vim's modes (insert, replace, visual, visual line), counts,
//! operators (`d c y > < g~ gu gU`) with motions and text objects,
//! registers (named, numbered, `0`, `-`, `_`), marks, undo and redo,
//! `.`, macros (`q`, `@`), searches and an ex line with ranges.
//!
//! Left out: block visual, folds, windows, sentences, options (`:set`),
//! and patterns' groups and alternation. Lines are at most `WIDTH`
//! long: an edit that makes one longer wraps it, as typing does. A shift
//! is 4 columns, the editor's Tab.

use std::collections::{HashMap, VecDeque};

use crate::editor::{Draft, MAX_LINES, Redraw, WIDTH};
use crate::input::Key;

const SHIFT: usize = 4;

// Keys as characters: the special keys get private-use ones, so a
// macro or `.` can hold any of them.
const ESC: char = '\x1b';
const CR: char = '\r';
const BS: char = '\x08';
const TAB: char = '\t';
const UP: char = '\u{e000}';
const DOWN: char = '\u{e001}';
const LEFT: char = '\u{e002}';
const RIGHT: char = '\u{e003}';
const HOME: char = '\u{e004}';
const END: char = '\u{e005}';
const PGUP: char = '\u{e006}';
const PGDN: char = '\u{e007}';
const INS: char = '\u{e008}';
const DEL: char = '\u{e009}';

fn key_char(k: Key) -> char {
    match k {
        Key::Char(b) if b < 0x80 => b as char,
        Key::Char(b) => fluxbbs_term::cp437::TO_UNICODE[b as usize],
        Key::Enter => CR,
        Key::Backspace => BS,
        Key::Tab => TAB,
        Key::Esc => ESC,
        Key::Up => UP,
        Key::Down => DOWN,
        Key::Left => LEFT,
        Key::Right => RIGHT,
        Key::Home => HOME,
        Key::End => END,
        Key::PgUp => PGUP,
        Key::PgDn => PGDN,
        Key::Ins => INS,
        Key::Del => DEL,
    }
}

/// A key that's text (not a control key or a special one).
fn is_text(c: char) -> bool {
    c >= ' ' && c != '\x7f' && !('\u{e000}'..='\u{e0ff}').contains(&c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Replace,
    Visual,
    VisualLine,
}

impl Mode {
    pub fn visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine)
    }
}

/// The editor's view, for `H`, `M`, `L` and the page keys.
#[derive(Debug, Clone, Copy)]
pub struct Scr {
    pub first: usize,
    pub rows: usize,
}

/// What a key (or a prompt's line) did.
#[derive(Debug, PartialEq)]
pub enum VimDo {
    Edit(Redraw),
    /// Read a line in the prompt row after this character (`:`, `/`,
    /// `?`), starting with this text, and hand it to [`Vim::prompted`].
    Prompt(char, String),
    Save,
    Abort,
    /// `:q`: ask before the message is thrown away.
    AskAbort,
    /// `:` and Enter alone: the editor's own commands (title, quote...).
    Menu,
    Help,
    /// Ctrl-E and Ctrl-Y: the view moves this many lines.
    Scroll(isize),
    /// A string to show in the prompt row, and its `%S` values.
    Say(&'static str, Vec<String>),
    /// Ctrl-L: the editor redraws.
    Pass,
}

type Pos = (usize, usize);

/// Visual mode's selection: its first and last positions, and whether
/// it's whole lines.
pub type Sel = (Pos, Pos, bool);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Excl,
    Incl,
    Line,
}

#[derive(Debug, Clone, Default)]
struct Reg {
    text: Vec<Vec<char>>,
    lines: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Delete,
    Change,
    Yank,
    Right,
    Left,
    Toggle,
    Lower,
    Upper,
}

#[derive(Debug, Clone, PartialEq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    Col0,
    First,
    End,
    LastNonBlank,
    FirstDown,
    NextLine,
    PrevLine,
    Word(bool),
    Back(bool),
    WordEnd(bool),
    BackEnd(bool),
    Find(char, char),
    Repeat(bool),
    Line,
    Top,
    Column,
    Match,
    ParaNext,
    ParaPrev,
    Search(bool),
    Pattern(String, bool),
    Next(bool),
    Star(bool),
    ScreenTop,
    ScreenMid,
    ScreenEnd,
    Mark(char, bool),
}

#[derive(Debug, Clone, PartialEq)]
enum Target {
    Motion(Motion),
    Object(bool, char),
    Lines,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum At {
    Before,
    After,
    LineStart,
    LineEnd,
    Below,
    Above,
    Last,
    Col0,
}

#[derive(Debug, Clone, PartialEq)]
enum Act {
    Move(Motion),
    Operate(Op, Target),
    Put(bool),
    Replace(char),
    Insert(At),
    ReplaceMode,
    Join(bool),
    Tilde,
    Undo,
    Redo,
    Dot,
    Visual(Mode),
    Reselect,
    Mark(char),
    Ex,
    Save,
    Abort,
    Record(Option<char>),
    Play(char),
    Add(i64),
    Scroll(isize),
    Half(bool),
    Page(bool),
    Status,
    Refresh,
    // Visual mode's own.
    Swap,
    VisOp(Op, bool),
    VisReplace(char),
    VisPut,
    VisJoin(bool),
    VisObject(bool, char),
}

#[derive(Debug, Clone, PartialEq)]
struct Cmd {
    count: Option<usize>,
    reg: Option<char>,
    act: Act,
    /// Its keys without the count and register, for `.`.
    core: Vec<char>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stop {
    More,
    Bad,
}

type P<T> = Result<T, Stop>;

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    skips: Vec<(usize, usize)>,
}

impl Parser<'_> {
    fn peek(&self) -> P<char> {
        self.s.get(self.i).copied().ok_or(Stop::More)
    }

    fn next(&mut self) -> P<char> {
        let c = self.peek()?;
        self.i += 1;
        Ok(c)
    }

    fn count(&mut self) -> Option<usize> {
        let start = self.i;
        let mut n: Option<usize> = None;
        while let Some(d) = self.s.get(self.i).and_then(|c| c.to_digit(10)) {
            if d == 0 && n.is_none() {
                break;
            }
            n = Some((n.unwrap_or(0) * 10 + d as usize).min(9999));
            self.i += 1;
        }
        if self.i > start {
            self.skips.push((start, self.i));
        }
        n
    }

    /// A key that can't be Esc (which cancels).
    fn arg(&mut self) -> P<char> {
        match self.next()? {
            ESC => Err(Stop::Bad),
            c => Ok(c),
        }
    }
}

fn times(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some((a.unwrap_or(1) * b.unwrap_or(1)).min(9999)),
    }
}

fn reg_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '"' | '-' | '_')
}

/// A motion's keys, the first already read (`c`).
fn parse_motion(p: &mut Parser, c: char) -> P<Motion> {
    use Motion::*;
    Ok(match c {
        'h' | LEFT | BS => Left,
        'l' | RIGHT | ' ' => Right,
        'j' | DOWN | '\x0e' | '\n' => Down,
        'k' | UP | '\x10' => Up,
        '0' | HOME => Col0,
        '^' => First,
        '$' | END => End,
        '_' => FirstDown,
        '+' | CR => NextLine,
        '-' => PrevLine,
        'w' | 'W' => Word(c == 'W'),
        'b' | 'B' => Back(c == 'B'),
        'e' | 'E' => WordEnd(c == 'E'),
        'f' | 'F' | 't' | 'T' => Find(c, p.arg()?),
        ';' | ',' => Repeat(c == ','),
        'G' => Line,
        '|' => Column,
        '%' => Match,
        '}' => ParaNext,
        '{' => ParaPrev,
        '/' | '?' => Search(c == '/'),
        'n' | 'N' => Next(c == 'N'),
        '*' | '#' => Star(c == '*'),
        'H' => ScreenTop,
        'M' => ScreenMid,
        'L' => ScreenEnd,
        '\'' | '`' => Mark(p.arg()?, c == '`'),
        'g' => match p.next()? {
            'g' => Top,
            'e' => BackEnd(false),
            'E' => BackEnd(true),
            '_' => LastNonBlank,
            _ => return Err(Stop::Bad),
        },
        _ => return Err(Stop::Bad),
    })
}

/// An operator's key (and `g` before it), if `c` is one.
fn parse_op(p: &mut Parser, c: char) -> P<Option<(Op, char)>> {
    Ok(match c {
        'd' => Some((Op::Delete, 'd')),
        'c' => Some((Op::Change, 'c')),
        'y' => Some((Op::Yank, 'y')),
        '>' => Some((Op::Right, '>')),
        '<' => Some((Op::Left, '<')),
        'g' => match p.peek()? {
            '~' => Some((Op::Toggle, '~')),
            'u' => Some((Op::Lower, 'u')),
            'U' => Some((Op::Upper, 'U')),
            _ => None,
        }
        .inspect(|_| p.i += 1),
        _ => None,
    })
}

/// What follows an operator: itself again (the lines), a text object,
/// or a motion.
fn parse_target(p: &mut Parser, op: (Op, char), gop: bool, count: &mut Option<usize>) -> P<Target> {
    let c2 = p.count();
    *count = times(*count, c2);
    let t = p.arg()?;
    if t == op.1 {
        return Ok(Target::Lines);
    }
    if gop && t == 'g' && p.peek()? == op.1 {
        p.i += 1;
        return Ok(Target::Lines);
    }
    if t == 'i' || t == 'a' {
        return Ok(Target::Object(t == 'i', p.arg()?));
    }
    Ok(Target::Motion(parse_motion(p, t)?))
}

fn parse(s: &[char], mode: Mode, recording: bool) -> P<Cmd> {
    let mut p = Parser { s, i: 0, skips: Vec::new() };
    let c1 = p.count();
    let mut reg = None;
    if p.peek()? == '"' {
        let start = p.i;
        p.i += 1;
        let r = p.arg()?;
        if !reg_name(r) {
            return Err(Stop::Bad);
        }
        reg = Some(r);
        p.skips.push((start, p.i));
    }
    let c2 = p.count();
    let mut count = times(c1, c2);
    let c = p.next()?;
    if c == ESC {
        return Err(Stop::Bad);
    }
    let act = if mode.visual() { parse_visual(&mut p, c)? } else { parse_normal(&mut p, c, &mut count, recording)? };
    let core = (0..p.i).filter(|&i| !p.skips.iter().any(|&(a, b)| (a..b).contains(&i))).map(|i| s[i]).collect();
    Ok(Cmd { count, reg, act, core })
}

fn parse_normal(p: &mut Parser, c: char, count: &mut Option<usize>, recording: bool) -> P<Act> {
    use Act::*;
    if let Some(op) = parse_op(p, c)? {
        let gop = c == 'g';
        return Ok(Operate(op.0, parse_target(p, op, gop, count)?));
    }
    let motion = |m| Ok(Operate(Op::Delete, Target::Motion(m)));
    Ok(match c {
        'x' | DEL => return motion(Motion::Right),
        'X' => return motion(Motion::Left),
        's' => Operate(Op::Change, Target::Motion(Motion::Right)),
        'S' => Operate(Op::Change, Target::Lines),
        'D' => Operate(Op::Delete, Target::Motion(Motion::End)),
        'C' => Operate(Op::Change, Target::Motion(Motion::End)),
        'Y' => Operate(Op::Yank, Target::Lines),
        'p' | 'P' => Put(c == 'P'),
        'r' => Replace(p.arg()?),
        'R' => ReplaceMode,
        'J' => Join(true),
        '~' => Tilde,
        'u' => Undo,
        '\x12' => Redo,
        '.' => Dot,
        'v' => Visual(Mode::Visual),
        'V' => Visual(Mode::VisualLine),
        'm' => Mark(p.arg()?),
        ':' => Ex,
        'Z' => match p.next()? {
            'Z' => Save,
            'Q' => Abort,
            _ => return Err(Stop::Bad),
        },
        'q' if recording => Record(None),
        'q' => Record(Some(p.arg()?)),
        '@' => Play(p.arg()?),
        '\x01' => Add(1),
        '\x18' => Add(-1),
        '\x05' => Scroll(1),
        '\x19' => Scroll(-1),
        '\x04' => Half(true),
        '\x15' => Half(false),
        '\x06' | PGDN => Page(true),
        '\x02' | PGUP => Page(false),
        '\x07' => Status,
        '\x0c' => Refresh,
        'i' | INS => Insert(At::Before),
        'a' => Insert(At::After),
        'I' => Insert(At::LineStart),
        'A' => Insert(At::LineEnd),
        'o' => Insert(At::Below),
        'O' => Insert(At::Above),
        'g' => match p.peek()? {
            'J' => {
                p.i += 1;
                Join(false)
            }
            'i' => {
                p.i += 1;
                Insert(At::Last)
            }
            'I' => {
                p.i += 1;
                Insert(At::Col0)
            }
            'v' => {
                p.i += 1;
                Reselect
            }
            'p' | 'P' => {
                let before = p.next()? == 'P';
                Put(before)
            }
            _ => Move(parse_motion(p, c)?),
        },
        c => Move(parse_motion(p, c)?),
    })
}

fn parse_visual(p: &mut Parser, c: char) -> P<Act> {
    use Act::*;
    Ok(match c {
        'o' | 'O' => Swap,
        'v' => Visual(Mode::Visual),
        'V' => Visual(Mode::VisualLine),
        '\x03' => Visual(Mode::Normal),
        'd' | 'x' | DEL => VisOp(Op::Delete, false),
        'X' | 'D' => VisOp(Op::Delete, true),
        'c' | 's' => VisOp(Op::Change, false),
        'C' | 'S' | 'R' => VisOp(Op::Change, true),
        'y' => VisOp(Op::Yank, false),
        'Y' => VisOp(Op::Yank, true),
        '>' => VisOp(Op::Right, true),
        '<' => VisOp(Op::Left, true),
        '~' => VisOp(Op::Toggle, false),
        'u' => VisOp(Op::Lower, false),
        'U' => VisOp(Op::Upper, false),
        'J' => VisJoin(true),
        'r' => VisReplace(p.arg()?),
        'p' | 'P' => VisPut,
        ':' => Ex,
        'i' | 'a' => VisObject(c == 'i', p.arg()?),
        '\x0c' => Refresh,
        'g' => match p.peek()? {
            'J' => {
                p.i += 1;
                VisJoin(false)
            }
            '~' | 'u' | 'U' => {
                let k = p.next()?;
                VisOp(if k == '~' { Op::Toggle } else if k == 'u' { Op::Lower } else { Op::Upper }, false)
            }
            'v' => {
                p.i += 1;
                Reselect
            }
            _ => Move(parse_motion(p, c)?),
        },
        c => Move(parse_motion(p, c)?),
    })
}

// ---- the text --------------------------------------------------------

fn nlines(d: &Draft) -> usize {
    d.lines.len()
}

fn len(d: &Draft, r: usize) -> usize {
    d.lines[r].len()
}

fn ch(d: &Draft, (r, c): Pos) -> Option<char> {
    d.lines.get(r).and_then(|l| l.get(c)).copied()
}

/// `^`: the first character that isn't a space (the last, on a line of
/// spaces).
fn fnb(d: &Draft, r: usize) -> usize {
    d.lines[r].iter().position(|&c| c != ' ').unwrap_or(len(d, r).saturating_sub(1))
}

fn blank(d: &Draft, r: usize) -> bool {
    d.lines[r].iter().all(|&c| c == ' ')
}

/// The position after, character by character; an empty line is one
/// position.
fn next_pos(d: &Draft, (r, c): Pos) -> Option<Pos> {
    if c + 1 < len(d, r) {
        Some((r, c + 1))
    } else if r + 1 < nlines(d) {
        Some((r + 1, 0))
    } else {
        None
    }
}

fn prev_pos(d: &Draft, (r, c): Pos) -> Option<Pos> {
    if c > 0 {
        Some((r, (c - 1).min(len(d, r).saturating_sub(1))))
    } else if r > 0 {
        Some((r - 1, len(d, r - 1).saturating_sub(1)))
    } else {
        None
    }
}

/// A character's kind for word motions: 0 space, 1 word, 2 punctuation
/// (with `big`, anything not a space is a word).
fn class(c: Option<char>, big: bool) -> u8 {
    match c {
        None => 3,
        Some(' ') => 0,
        Some(_) if big => 1,
        Some(c) if c.is_alphanumeric() || c == '_' => 1,
        Some(_) => 2,
    }
}

/// `w`: the next word's start; an empty line is a word.
fn word_fwd(d: &Draft, p: Pos, big: bool) -> Pos {
    let (mut r, mut c) = p;
    let k = class(ch(d, p), big);
    if k == 1 || k == 2 {
        while c < len(d, r) && class(ch(d, (r, c)), big) == k {
            c += 1;
        }
    }
    loop {
        while c < len(d, r) && ch(d, (r, c)) == Some(' ') {
            c += 1;
        }
        if c < len(d, r) {
            return (r, c);
        }
        if r + 1 >= nlines(d) {
            return (r, len(d, r));
        }
        r += 1;
        c = 0;
        if len(d, r) == 0 {
            return (r, 0);
        }
    }
}

/// `b`: this word's start, or the one before.
fn word_back(d: &Draft, p: Pos, big: bool) -> Pos {
    let Some(mut q) = prev_pos(d, p) else { return (0, 0) };
    loop {
        // An empty line is a word.
        if len(d, q.0) == 0 {
            return q;
        }
        if ch(d, q) != Some(' ') {
            break;
        }
        match prev_pos(d, q) {
            Some(x) => q = x,
            None => return (0, 0),
        }
    }
    let k = class(ch(d, q), big);
    while q.1 > 0 && class(ch(d, (q.0, q.1 - 1)), big) == k {
        q.1 -= 1;
    }
    q
}

/// `e`: the end of this word or the next.
fn word_end(d: &Draft, p: Pos, big: bool) -> Pos {
    let Some(mut q) = next_pos(d, p) else { return p };
    while class(ch(d, q), big) == 0 || len(d, q.0) == 0 {
        match next_pos(d, q) {
            Some(x) => q = x,
            None => return q,
        }
    }
    let k = class(ch(d, q), big);
    while q.1 + 1 < len(d, q.0) && class(ch(d, (q.0, q.1 + 1)), big) == k {
        q.1 += 1;
    }
    q
}

/// `ge`: the end of the word before.
fn back_end(d: &Draft, p: Pos, big: bool) -> Pos {
    let k = class(ch(d, p), big);
    let mut q = p;
    if k == 1 || k == 2 {
        while q.1 > 0 && class(ch(d, (q.0, q.1 - 1)), big) == k {
            q.1 -= 1;
        }
    }
    let Some(mut q) = prev_pos(d, q) else { return (0, 0) };
    while ch(d, q) == Some(' ') {
        match prev_pos(d, q) {
            Some(x) => q = x,
            None => return (0, 0),
        }
    }
    q
}

/// Wrap any line from `row` to `last` longer than the editor allows,
/// as typing would; returns the last row touched.
fn fit(d: &mut Draft, row: usize, last: usize) -> usize {
    let mut r = row;
    let mut last = last;
    while r <= last && r < nlines(d) {
        if len(d, r) > WIDTH {
            let line = &d.lines[r];
            let sp = line[..=WIDTH].iter().rposition(|&c| c == ' ').filter(|&i| i > 0);
            let (keep, rest) = match sp {
                Some(i) => (line[..i].to_vec(), line[i + 1..].to_vec()),
                None => (line[..WIDTH].to_vec(), line[WIDTH..].to_vec()),
            };
            d.lines[r] = keep;
            d.lines.insert(r + 1, rest);
            last += 1;
        }
        r += 1;
    }
    last.min(nlines(d) - 1)
}

/// The text from `a` to `b` (`b` past the end, for characters).
fn get(d: &Draft, a: Pos, b: Pos, lines: bool) -> Reg {
    if lines {
        return Reg { text: d.lines[a.0..=b.0].to_vec(), lines: true };
    }
    let text = if a.0 == b.0 {
        vec![d.lines[a.0][a.1..b.1].to_vec()]
    } else {
        let mut t = vec![d.lines[a.0][a.1..].to_vec()];
        t.extend(d.lines[a.0 + 1..b.0].iter().cloned());
        t.push(d.lines[b.0][..b.1].to_vec());
        t
    };
    Reg { text, lines: false }
}

fn remove(d: &mut Draft, a: Pos, b: Pos, lines: bool) {
    if lines {
        d.lines.drain(a.0..=b.0);
        if d.lines.is_empty() {
            d.lines.push(Vec::new());
        }
        return;
    }
    let tail = d.lines[b.0][b.1..].to_vec();
    d.lines[a.0].truncate(a.1);
    d.lines[a.0].extend(tail);
    d.lines.drain(a.0 + 1..=b.0);
}

/// Text into a line at `p`; returns where it ends (after it).
fn insert_text(d: &mut Draft, p: Pos, text: &[Vec<char>]) -> Pos {
    let (r, c) = (p.0, p.1.min(len(d, p.0)));
    if text.len() <= 1 {
        let t = text.first().cloned().unwrap_or_default();
        let n = t.len();
        d.lines[r].splice(c..c, t);
        return (r, c + n);
    }
    let tail = d.lines[r].split_off(c);
    d.lines[r].extend(text[0].iter().copied());
    let mut at = r;
    for l in &text[1..] {
        at += 1;
        d.lines.insert(at, l.clone());
    }
    let end = (at, len(d, at));
    d.lines[at].extend(tail);
    end
}

fn shift(d: &mut Draft, r: usize, right: bool, times: usize) {
    for _ in 0..times {
        let l = &mut d.lines[r];
        if l.is_empty() {
            return;
        }
        if right {
            if l.len() + SHIFT > WIDTH {
                return;
            }
            l.splice(0..0, std::iter::repeat_n(' ', SHIFT));
        } else {
            let n = l.iter().take(SHIFT).take_while(|&&c| c == ' ').count();
            l.drain(..n);
        }
    }
}

fn recase(c: char, op: Op) -> char {
    // Only to one character, and one CP437 has (not ß to SS, µ to Μ).
    let f = |s: String| {
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(x), None) if x.is_ascii() || fluxbbs_term::cp437::TO_UNICODE.contains(&x) => x,
            _ => c,
        }
    };
    match op {
        Op::Lower => f(c.to_lowercase().collect()),
        Op::Upper => f(c.to_uppercase().collect()),
        _ if c.is_uppercase() => f(c.to_lowercase().collect()),
        _ => f(c.to_uppercase().collect()),
    }
}

fn span(r: Redraw) -> Option<(usize, usize)> {
    match r {
        Redraw::Cursor => None,
        Redraw::Line(i) | Redraw::Tail(i, _) => Some((i, i)),
        Redraw::Rows(a, b) => Some((a, b)),
        Redraw::From(i) => Some((i, usize::MAX)),
    }
}

/// Both redraws in one.
pub fn merge(a: Redraw, b: Redraw) -> Redraw {
    match (span(a), span(b)) {
        (None, _) => b,
        (_, None) => a,
        (Some((a0, a1)), Some((b0, b1))) => {
            let (lo, hi) = (a0.min(b0), a1.max(b1));
            if hi == usize::MAX {
                Redraw::From(lo)
            } else if lo == hi {
                Redraw::Line(lo)
            } else {
                Redraw::Rows(lo, hi)
            }
        }
    }
}

// ---- patterns ---------------------------------------------------------

/// A vim pattern, the parts vim's `magic` gives without groups:
/// `. * [] ^ $ \+ \= \? \{n,m} \< \> \s \S \d \D \w \W \a \A \l \u \c`.
#[derive(Debug, Clone)]
struct Pat {
    pieces: Vec<(Atom, usize, usize)>,
    bol: bool,
    eol: bool,
    icase: bool,
    /// New: the steps matching may still take (`Vim::work`); with none
    /// left, nothing matches.
    left: std::cell::Cell<u64>,
}

/// New: the most pieces a pattern may have.
const MAX_PIECES: usize = 100;

#[derive(Debug, Clone)]
enum Atom {
    Ch(char),
    Any,
    Set(Vec<(char, char)>, bool),
    Class(char),
    WordStart,
    WordEnd,
}

impl Pat {
    fn new(s: &str) -> Option<Pat> {
        let c: Vec<char> = s.chars().collect();
        let mut pieces: Vec<(Atom, usize, usize)> = Vec::new();
        let (mut bol, mut eol, mut icase) = (false, false, false);
        let mut i = 0;
        while i < c.len() {
            let x = c[i];
            i += 1;
            let atom = match x {
                '^' if i == 1 => {
                    bol = true;
                    continue;
                }
                '$' if i == c.len() => {
                    eol = true;
                    continue;
                }
                '.' => Atom::Any,
                '*' if !pieces.is_empty() => {
                    let last = pieces.last_mut()?;
                    (last.1, last.2) = (0, usize::MAX);
                    continue;
                }
                '[' => {
                    let mut set = Vec::new();
                    let neg = c.get(i) == Some(&'^');
                    if neg {
                        i += 1;
                    }
                    let mut first = true;
                    loop {
                        let &a = c.get(i)?;
                        if a == ']' && !first {
                            i += 1;
                            break;
                        }
                        first = false;
                        let a = if a == '\\' && i + 1 < c.len() {
                            i += 1;
                            c[i]
                        } else {
                            a
                        };
                        if c.get(i + 1) == Some(&'-') && c.get(i + 2).is_some_and(|&b| b != ']') {
                            set.push((a, c[i + 2]));
                            i += 3;
                        } else {
                            set.push((a, a));
                            i += 1;
                        }
                    }
                    Atom::Set(set, neg)
                }
                '\\' => {
                    let &y = c.get(i)?;
                    i += 1;
                    match y {
                        '+' | '=' | '?' => {
                            let last = pieces.last_mut()?;
                            if y == '+' {
                                (last.1, last.2) = (1, usize::MAX);
                            } else {
                                (last.1, last.2) = (0, 1);
                            }
                            continue;
                        }
                        '{' => {
                            let end = c[i..].iter().position(|&z| z == '}')? + i;
                            let inner: String = c[i..end].iter().collect::<String>().trim_end_matches('\\').to_string();
                            i = end + 1;
                            let (lo, hi) = match inner.split_once(',') {
                                Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(usize::MAX)),
                                None => {
                                    let n = inner.parse().unwrap_or(0);
                                    (n, n)
                                }
                            };
                            let last = pieces.last_mut()?;
                            (last.1, last.2) = (lo, hi);
                            continue;
                        }
                        'c' => {
                            icase = true;
                            continue;
                        }
                        'C' => continue,
                        '<' => Atom::WordStart,
                        '>' => Atom::WordEnd,
                        's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'a' | 'A' | 'l' | 'u' => Atom::Class(y),
                        't' => Atom::Ch('\t'),
                        y => Atom::Ch(y),
                    }
                }
                x => Atom::Ch(x),
            };
            pieces.push((atom, 1, 1));
            if pieces.len() > MAX_PIECES {
                return None;
            }
        }
        Some(Pat { pieces, bol, eol, icase, left: std::cell::Cell::new(u64::MAX) })
    }

    fn one(&self, a: &Atom, line: &[char], at: usize) -> bool {
        let Some(&c) = line.get(at) else { return false };
        let same = |x: char, y: char| x == y || (self.icase && x.to_lowercase().eq(y.to_lowercase()));
        match a {
            Atom::Ch(x) => same(*x, c),
            Atom::Any => true,
            Atom::Set(set, neg) => set.iter().any(|&(lo, hi)| (lo..=hi).contains(&c) || (self.icase && (lo..=hi).contains(&c.to_ascii_lowercase()))) != *neg,
            Atom::Class(k) => match k {
                's' => c == ' ' || c == '\t',
                'S' => c != ' ' && c != '\t',
                'd' => c.is_ascii_digit(),
                'D' => !c.is_ascii_digit(),
                'w' => c.is_ascii_alphanumeric() || c == '_',
                'W' => !(c.is_ascii_alphanumeric() || c == '_'),
                'a' => c.is_alphabetic(),
                'A' => !c.is_alphabetic(),
                'l' => c.is_lowercase(),
                _ => c.is_uppercase(),
            },
            Atom::WordStart | Atom::WordEnd => false,
        }
    }

    /// Where piece `i` on matches from `pos`. Without groups that
    /// depends only on `i` and `pos`, so `memo` keeps each answer and
    /// `.*.*.*x` can't take exponential time.
    fn at(&self, i: usize, line: &[char], pos: usize, memo: &mut Vec<Option<Option<usize>>>) -> Option<usize> {
        let k = i * (line.len() + 1) + pos;
        if let Some(&Some(done)) = memo.get(k) {
            return done;
        }
        let out = self.at_inner(i, line, pos, memo);
        if let Some(m) = memo.get_mut(k) {
            *m = Some(out);
        }
        out
    }

    fn at_inner(&self, i: usize, line: &[char], pos: usize, memo: &mut Vec<Option<Option<usize>>>) -> Option<usize> {
        let Some((atom, lo, hi)) = self.pieces.get(i) else {
            return (!self.eol || pos == line.len()).then_some(pos);
        };
        let word = |p: usize| line.get(p).is_some_and(|&c| c.is_alphanumeric() || c == '_');
        match atom {
            Atom::WordStart => {
                return if word(pos) && (pos == 0 || !word(pos - 1)) { self.at(i + 1, line, pos, memo) } else { None };
            }
            Atom::WordEnd => {
                return if pos > 0 && word(pos - 1) && !word(pos) { self.at(i + 1, line, pos, memo) } else { None };
            }
            _ => {}
        }
        let mut n = 0;
        while n < *hi && self.one(atom, line, pos + n) {
            n += 1;
        }
        let cost = 1 + 2 * n as u64;
        if self.left.get() < cost {
            self.left.set(0);
            return None;
        }
        self.left.set(self.left.get() - cost);
        if n < *lo {
            return None;
        }
        loop {
            if let Some(end) = self.at(i + 1, line, pos + n, memo) {
                return Some(end);
            }
            if n == *lo {
                return None;
            }
            n -= 1;
        }
    }

    fn memo(&self, line: &[char]) -> Vec<Option<Option<usize>>> {
        vec![None; (self.pieces.len() + 1) * (line.len() + 1)]
    }

    /// The first match in `line` starting at or after `from`.
    #[cfg(test)]
    fn find(&self, line: &[char], from: usize) -> Option<(usize, usize)> {
        self.find_in(line, from, &mut self.memo(line))
    }

    /// The first match at or after `from`, with a memo kept for `line`
    /// (it holds whatever `from` is).
    fn find_in(&self, line: &[char], from: usize, memo: &mut Vec<Option<Option<usize>>>) -> Option<(usize, usize)> {
        let last = if self.bol { 0 } else { line.len() };
        (from..=last).find_map(|s| self.at(0, line, s, memo).map(|e| (s, e)))
    }

    /// Where every match in `line` starts.
    fn starts(&self, line: &[char]) -> Vec<usize> {
        let last = if self.bol { 0 } else { line.len() };
        let mut memo = self.memo(line);
        (0..=last).filter(|&s| self.at(0, line, s, &mut memo).is_some()).collect()
    }

    /// Out of steps: what it found may not be all there is.
    fn spent(&self) -> bool {
        self.left.get() == 0
    }
}

// ---- the machine --------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Dot {
    keys: Vec<char>,
    count: Option<usize>,
    reg: Option<char>,
}

#[derive(Debug, Default)]
struct Ins {
    count: usize,
    keys: Vec<char>,
    /// `o` or `O` (true below): a count opens that many lines.
    open: Option<bool>,
    /// Replace mode: what each typed character covered.
    replaced: Vec<Option<char>>,
    ctrl_r: bool,
    /// Ctrl-O: one command in normal mode, then back to this mode.
    one_shot: Option<Mode>,
    /// Its command has been given.
    shot_ran: bool,
    /// Replace mode: where each typed character went, and what it covered.
    replaced_at: Vec<Pos>,
}

type Snap = (Vec<Vec<char>>, usize, usize);

#[derive(Default)]
pub struct Vim {
    pub mode: Mode,
    pending: Vec<char>,
    regs: HashMap<char, Reg>,
    /// The column `j` and `k` keep to.
    want: Option<usize>,
    find: Option<(char, char)>,
    search: Option<(String, bool)>,
    marks: HashMap<char, Pos>,
    jump: Option<Pos>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    dot: Option<Dot>,
    /// The change being recorded for `.`, until its insert ends.
    rec: Option<Dot>,
    ins: Ins,
    anchor: Pos,
    last_visual: Option<(Pos, Pos, Mode)>,
    /// Keys to run: typed, or from a macro or `.` (true).
    queue: VecDeque<(char, bool)>,
    macro_rec: Option<(char, Vec<char>)>,
    last_macro: Option<char>,
    last_sub: Option<(String, String, bool)>,
    /// A command waiting for the prompt's line (`d/word`).
    waiting: Option<Cmd>,
    /// The last command failed (a motion that couldn't move...): a macro
    /// or `.` stops there, as in vim.
    failed: bool,
    /// New: the work (characters copied, pattern steps) the typed key's
    /// commands, macros and `.` may still do.
    work: u64,
    /// New: `"` is this register (after an append to it), not a copy.
    unnamed: Option<char>,
}

/// The most keys a macro or `.` may run (or queue) for one key typed:
/// `qq@qq` loops in vim too, until Ctrl-C.
const REPLAY_MAX: usize = 50_000;
/// New: the work one typed key may do ([`Vim::work`]): a macro run
/// thousands of times over a full message stops here.
const WORK_MAX: u64 = 50_000_000;
/// New: the most a register holds, a whole message.
const REG_MAX: usize = MAX_LINES * (WIDTH + 1);

fn reg_size(r: &Reg) -> usize {
    r.text.iter().map(|l| l.len() + 1).sum()
}

impl Vim {
    pub fn new() -> Self {
        Vim { work: WORK_MAX, ..Vim::default() }
    }

    /// `recording @a`, while a macro's being recorded.
    pub fn recording(&self) -> Option<char> {
        self.macro_rec.as_ref().map(|m| m.0)
    }

    /// Visual mode's selection: its ends (first, last) and whether it's
    /// whole lines.
    pub fn selection(&self, d: &Draft) -> Option<Sel> {
        if !self.mode.visual() {
            return None;
        }
        let (a, b) = order(self.anchor, (d.row, d.col));
        Some((a, b, self.mode == Mode::VisualLine))
    }

    pub fn key(&mut self, d: &mut Draft, k: Key, s: Scr) -> VimDo {
        self.queue.push_back((key_char(k), false));
        self.run(d, s)
    }

    /// Text typed into insert mode (a mention completed by Tab).
    pub fn feed(&mut self, d: &mut Draft, text: &str, s: Scr) -> VimDo {
        self.queue.extend(text.chars().map(|c| (c, false)));
        self.run(d, s)
    }

    fn run(&mut self, d: &mut Draft, s: Scr) -> VimDo {
        let mut out = Redraw::Cursor;
        let mut budget = REPLAY_MAX;
        self.work = WORK_MAX;
        while let Some((c, replayed)) = self.queue.pop_front() {
            if replayed {
                if budget == 0 || self.work == 0 || std::mem::take(&mut self.failed) {
                    self.queue.retain(|q| !q.1);
                    self.pending.clear();
                    continue;
                }
                budget -= 1;
            } else {
                self.failed = false;
                if let Some((_, keys)) = &mut self.macro_rec {
                    keys.push(c);
                }
            }
            let done = self.step(d, c, s);
            if !matches!(done, VimDo::Prompt(..)) {
                self.settle(d);
            }
            match done {
                VimDo::Edit(r) => out = merge(out, r),
                // From a macro or `.`: the line is in the keys.
                VimDo::Prompt(kind, _) if self.queue.front().is_some_and(|q| q.1) => {
                    let mut line = String::new();
                    let mut done = false;
                    while let Some((c, _)) = self.queue.pop_front() {
                        if c == CR {
                            done = true;
                            break;
                        }
                        if c == ESC {
                            break;
                        }
                        line.push(c);
                    }
                    let r = self.prompted_line(d, kind, done.then_some(line), s, false);
                    self.settle(d);
                    match r {
                        VimDo::Edit(r) => out = merge(out, r),
                        other => {
                            self.queue.clear();
                            return other;
                        }
                    }
                }
                other => {
                    if !matches!(other, VimDo::Prompt(..)) {
                        self.queue.retain(|q| !q.1);
                    }
                    return other;
                }
            }
        }
        self.failed = false;
        VimDo::Edit(out)
    }

    /// After a command: back to insert mode after Ctrl-O's one (once any
    /// keys it queued have run), and the cursor (and visual mode's other
    /// end) kept on the text.
    fn settle(&mut self, d: &mut Draft) {
        if let Some(m) = self.ins.one_shot
            && self.ins.shot_ran
            && self.mode == Mode::Normal
            && self.pending.is_empty()
            && self.waiting.is_none()
            && !self.queue.front().is_some_and(|q| q.1)
        {
            self.ins.one_shot = None;
            self.mode = m;
            d.insert = m == Mode::Insert;
        }
        d.row = d.row.min(nlines(d) - 1);
        let l = len(d, d.row);
        d.col = if matches!(self.mode, Mode::Insert | Mode::Replace) { d.col.min(l) } else { d.col.min(l.saturating_sub(1)) };
        let r = self.anchor.0.min(nlines(d) - 1);
        self.anchor = (r, self.anchor.1.min(len(d, r).saturating_sub(1)));
    }

    /// Keys for a macro or `.` to run, `n` times, ahead of what's typed;
    /// false if that's more than [`REPLAY_MAX`].
    fn replay(&mut self, keys: &[char], n: usize) -> bool {
        let queued = self.queue.iter().filter(|q| q.1).count();
        if keys.len().saturating_mul(n).saturating_add(queued) > REPLAY_MAX {
            self.failed = true;
            return false;
        }
        for _ in 0..n {
            for &c in keys.iter().rev() {
                self.queue.push_front((c, true));
            }
        }
        true
    }

    fn step(&mut self, d: &mut Draft, c: char, s: Scr) -> VimDo {
        if matches!(self.mode, Mode::Insert | Mode::Replace) {
            return self.insert_key(d, c, s);
        }
        if c == ESC && self.pending.is_empty() {
            self.ins.shot_ran = true;
            if self.mode.visual() {
                return self.leave_visual(d);
            }
            return VimDo::Edit(Redraw::Cursor);
        }
        self.pending.push(c);
        let cmd = match parse(&self.pending, self.mode, self.macro_rec.is_some()) {
            Err(Stop::More) => return VimDo::Edit(Redraw::Cursor),
            Err(Stop::Bad) => {
                self.pending.clear();
                return VimDo::Edit(Redraw::Cursor);
            }
            Ok(cmd) => cmd,
        };
        self.pending.clear();
        self.ins.shot_ran = true;
        let out = self.exec(d, cmd, s);
        self.full(d, out)
    }

    /// An edit that left more lines than a message may have is undone.
    fn full(&mut self, d: &mut Draft, out: VimDo) -> VimDo {
        if nlines(d) <= MAX_LINES {
            return out;
        }
        match self.undo.pop() {
            Some(snap) => Self::restore(d, snap),
            None => d.lines.truncate(MAX_LINES),
        }
        if matches!(self.mode, Mode::Insert | Mode::Replace) {
            self.mode = Mode::Normal;
            self.ins = Ins::default();
        }
        self.rec = None;
        self.failed = true;
        VimDo::Say("FsVimFull", vec![])
    }

    // ---- motions --------------------------------------------------------

    fn set_jump(&mut self, d: &Draft) {
        self.jump = Some((d.row, d.col));
    }

    /// Where a motion goes from the cursor, and what kind it is; None if
    /// it can't. `op`: for an operator (`l` may go past the end).
    fn motion(&mut self, d: &Draft, m: &Motion, count: Option<usize>, s: Scr, op: bool) -> Option<(Pos, Kind)> {
        use Motion::*;
        let n = count.unwrap_or(1);
        let (r, c) = (d.row, d.col.min(len(d, d.row)));
        let last = nlines(d) - 1;
        let col_for = |row: usize, want: usize| want.min(len(d, row).saturating_sub(1));
        Some(match m {
            Left => {
                if c == 0 {
                    return None;
                }
                ((r, c.saturating_sub(n)), Kind::Excl)
            }
            Right => {
                let l = len(d, r);
                if l == 0 || (!op && c + 1 >= l) {
                    return None;
                }
                ((r, (c + n).min(if op { l } else { l - 1 })), Kind::Excl)
            }
            Up | Down => {
                let row = if *m == Up { r.checked_sub(n.min(r)).filter(|&x| x != r)? } else { (r + n).min(last) };
                if row == r {
                    return None;
                }
                let w = self.want.unwrap_or(c);
                self.want = Some(w);
                ((row, col_for(row, w)), Kind::Line)
            }
            Col0 => ((r, 0), Kind::Excl),
            First => ((r, fnb(d, r)), Kind::Excl),
            End => {
                let row = (r + n - 1).min(last);
                self.want = Some(usize::MAX);
                ((row, len(d, row).saturating_sub(1)), Kind::Incl)
            }
            LastNonBlank => {
                let row = (r + n - 1).min(last);
                let col = d.lines[row].iter().rposition(|&c| c != ' ').unwrap_or(0);
                ((row, col), Kind::Incl)
            }
            FirstDown => {
                let row = (r + n - 1).min(last);
                ((row, fnb(d, row)), Kind::Line)
            }
            NextLine => {
                let row = r + n;
                if row > last {
                    return None;
                }
                ((row, fnb(d, row)), Kind::Line)
            }
            PrevLine => {
                let row = r.checked_sub(n)?;
                ((row, fnb(d, row)), Kind::Line)
            }
            Word(big) => {
                let mut p = (r, c);
                for _ in 0..n {
                    p = word_fwd(d, p, *big);
                }
                (p, Kind::Excl)
            }
            Back(big) => {
                let mut p = (r, c);
                for _ in 0..n {
                    p = word_back(d, p, *big);
                }
                (p, Kind::Excl)
            }
            WordEnd(big) => {
                let mut p = (r, c);
                for _ in 0..n {
                    p = word_end(d, p, *big);
                }
                (p, Kind::Incl)
            }
            BackEnd(big) => {
                let mut p = (r, c);
                for _ in 0..n {
                    p = back_end(d, p, *big);
                }
                (p, Kind::Incl)
            }
            Find(kind, x) => {
                self.find = Some((*kind, *x));
                return self.find_char(d, *kind, *x, n, false);
            }
            Repeat(rev) => {
                let (kind, x) = self.find?;
                let kind = if *rev {
                    match kind {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        _ => 't',
                    }
                } else {
                    kind
                };
                return self.find_char(d, kind, x, n, true);
            }
            Line | Top => {
                self.set_jump(d);
                let row = match count {
                    Some(n) => (n - 1).min(last),
                    None if *m == Line => last,
                    None => 0,
                };
                ((row, fnb(d, row)), Kind::Line)
            }
            Column => ((r, (n - 1).min(len(d, r))), Kind::Excl),
            Match => {
                self.set_jump(d);
                if let Some(pct) = count {
                    let row = ((pct.min(100) * nlines(d)).div_ceil(100)).saturating_sub(1);
                    return Some(((row, fnb(d, row)), Kind::Line));
                }
                (self.match_pair(d)?, Kind::Incl)
            }
            ParaNext | ParaPrev => {
                self.set_jump(d);
                let mut row = r;
                for _ in 0..n {
                    if *m == ParaNext {
                        while row < last && blank(d, row) {
                            row += 1;
                        }
                        while row < last && !blank(d, row) {
                            row += 1;
                        }
                    } else {
                        while row > 0 && blank(d, row) {
                            row -= 1;
                        }
                        while row > 0 && !blank(d, row) {
                            row -= 1;
                        }
                    }
                }
                if *m == ParaNext && !blank(d, row) {
                    ((row, len(d, row)), Kind::Excl)
                } else {
                    ((row, 0), Kind::Excl)
                }
            }
            Search(_) => return None,
            Pattern(p, fwd) => {
                self.search = Some((p.clone(), *fwd));
                return self.search_from(d, (r, c), n, false);
            }
            Next(rev) => return self.search_from(d, (r, c), n, *rev),
            Star(fwd) => {
                let line = &d.lines[r];
                let start = (c..line.len()).find(|&i| class(Some(line[i]), false) == 1)?;
                let mut a = start;
                while a > 0 && class(Some(line[a - 1]), false) == 1 {
                    a -= 1;
                }
                let mut b = start;
                while b < line.len() && class(Some(line[b]), false) == 1 {
                    b += 1;
                }
                let word: String = line[a..b].iter().collect();
                self.search = Some((format!("\\<{word}\\>"), *fwd));
                return self.search_from(d, (r, a), n, false);
            }
            ScreenTop | ScreenMid | ScreenEnd => {
                self.set_jump(d);
                // The view may be past the end after a macro's deletions.
                let first = s.first.min(last);
                let bottom = (first + s.rows.max(1)).min(nlines(d)) - 1;
                let row = match m {
                    ScreenTop => (first + n - 1).min(bottom),
                    ScreenEnd => bottom.saturating_sub(n - 1).max(first),
                    _ => first + (bottom - first) / 2,
                };
                ((row, fnb(d, row)), Kind::Line)
            }
            Mark(x, exact) => {
                let p = match x {
                    '\'' | '`' => self.jump?,
                    '<' => self.last_visual.map(|v| v.0)?,
                    '>' => self.last_visual.map(|v| v.1)?,
                    x => *self.marks.get(x)?,
                };
                let row = p.0.min(last);
                self.set_jump(d);
                if *exact { ((row, p.1.min(len(d, row))), Kind::Excl) } else { ((row, fnb(d, row)), Kind::Line) }
            }
        })
    }

    fn find_char(&self, d: &Draft, kind: char, x: char, n: usize, again: bool) -> Option<(Pos, Kind)> {
        let line = &d.lines[d.row];
        let c = d.col;
        let till = kind == 't' || kind == 'T';
        // `;` after `t` doesn't stick on the character it stopped before.
        let skip = usize::from(again && till);
        if kind == 'f' || kind == 't' {
            let mut at = c + skip;
            for _ in 0..n {
                at = (at + 1..line.len()).find(|&i| line[i] == x)?;
            }
            Some(((d.row, if till { at - 1 } else { at }), Kind::Incl))
        } else {
            let mut at = c.saturating_sub(skip);
            for _ in 0..n {
                at = (0..at).rev().find(|&i| line[i] == x)?;
            }
            Some(((d.row, if till { at + 1 } else { at }), Kind::Excl))
        }
    }

    /// `%`: the bracket on this line at or after the cursor, and its
    /// other half.
    fn match_pair(&self, d: &Draft) -> Option<Pos> {
        let line = &d.lines[d.row];
        let at = (d.col..line.len()).find(|&i| "()[]{}".contains(line[i]))?;
        let open = line[at];
        let (close, fwd) = match open {
            '(' => (')', true),
            '[' => (']', true),
            '{' => ('}', true),
            ')' => ('(', false),
            ']' => ('[', false),
            _ => ('{', false),
        };
        let mut depth = 0;
        let mut p = (d.row, at);
        loop {
            p = if fwd { next_pos(d, p)? } else { prev_pos(d, p)? };
            match ch(d, p) {
                Some(x) if x == open => depth += 1,
                Some(x) if x == close => {
                    if depth == 0 {
                        return Some(p);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }

    fn search_from(&mut self, d: &Draft, from: Pos, n: usize, rev: bool) -> Option<(Pos, Kind)> {
        let (pat, fwd) = self.search.clone()?;
        let fwd = fwd != rev;
        let p = Pat::new(&pat)?;
        p.left.set(self.work);
        let at = find_pat(d, &p, from, fwd, n);
        self.work = p.left.get();
        if p.spent() {
            self.failed = true;
            return None;
        }
        self.set_jump(d);
        Some((at?, Kind::Excl))
    }

    // ---- text objects ---------------------------------------------------

    fn object(&self, d: &Draft, inner: bool, x: char, count: usize) -> Option<(Pos, Pos, Kind)> {
        let r = d.row;
        let line = &d.lines[r];
        match x {
            'w' | 'W' => {
                if line.is_empty() {
                    return None;
                }
                let big = x == 'W';
                let k = |i: usize| class(Some(line[i]), big);
                let c = d.col.min(line.len() - 1);
                let run = |i: usize| {
                    let (mut a, mut b) = (i, i);
                    while a > 0 && k(a - 1) == k(i) {
                        a -= 1;
                    }
                    while b + 1 < line.len() && k(b + 1) == k(i) {
                        b += 1;
                    }
                    (a, b)
                };
                let (a, mut b) = run(c);
                let mut a = a;
                for _ in 1..count {
                    if b + 1 >= line.len() {
                        break;
                    }
                    b = run(b + 1).1;
                }
                if !inner {
                    if k(c) == 0 {
                        if b + 1 < line.len() {
                            b = run(b + 1).1;
                        }
                    } else if b + 1 < line.len() && k(b + 1) == 0 {
                        b = run(b + 1).1;
                    } else if a > 0 && k(a - 1) == 0 {
                        a = run(a - 1).0;
                    }
                }
                Some(((r, a), (r, b), Kind::Incl))
            }
            '"' | '\'' | '`' => {
                let qs: Vec<usize> = (0..line.len()).filter(|&i| line[i] == x && (i == 0 || line[i - 1] != '\\')).collect();
                let c = d.col;
                let pair = qs.chunks(2).filter(|p| p.len() == 2).find(|p| c <= p[1]).map(|p| (p[0], p[1]))?;
                let (o, e) = pair;
                if inner {
                    return Some(((r, o + 1), (r, e), Kind::Excl));
                }
                let mut b = e;
                let mut a = o;
                if line.get(b + 1) == Some(&' ') {
                    while line.get(b + 1) == Some(&' ') {
                        b += 1;
                    }
                } else {
                    while a > 0 && line[a - 1] == ' ' {
                        a -= 1;
                    }
                }
                Some(((r, a), (r, b), Kind::Incl))
            }
            '(' | ')' | 'b' | '[' | ']' | '{' | '}' | 'B' | '<' | '>' => {
                let (open, close) = match x {
                    '(' | ')' | 'b' => ('(', ')'),
                    '[' | ']' => ('[', ']'),
                    '{' | '}' | 'B' => ('{', '}'),
                    _ => ('<', '>'),
                };
                let mut o = (r, d.col.min(line.len().saturating_sub(1)));
                for k in 0..count {
                    if k > 0 {
                        o = prev_pos(d, o)?;
                    }
                    let mut depth = 0;
                    loop {
                        match ch(d, o) {
                            Some(y) if y == open && depth == 0 => break,
                            Some(y) if y == open => depth -= 1,
                            Some(y) if y == close && o != (r, d.col) => depth += 1,
                            _ => {}
                        }
                        o = prev_pos(d, o)?;
                    }
                }
                let mut e = o;
                let mut depth = 0;
                loop {
                    e = next_pos(d, e)?;
                    match ch(d, e) {
                        Some(y) if y == open => depth += 1,
                        Some(y) if y == close => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                }
                if !inner {
                    return Some((o, e, Kind::Incl));
                }
                // On lines of their own, what's between is whole lines.
                if o.1 + 1 == len(d, o.0) && e.0 > o.0 + 1 && d.lines[e.0][..e.1].iter().all(|&c| c == ' ') {
                    return Some(((o.0 + 1, 0), (e.0 - 1, 0), Kind::Line));
                }
                Some(((o.0, o.1 + 1), e, Kind::Excl))
            }
            'p' => {
                let last = nlines(d) - 1;
                let run = |row: usize| {
                    let b = blank(d, row);
                    let (mut a, mut z) = (row, row);
                    while a > 0 && blank(d, a - 1) == b {
                        a -= 1;
                    }
                    while z < last && blank(d, z + 1) == b {
                        z += 1;
                    }
                    (a, z)
                };
                let (a, mut z) = run(r);
                let mut a = a;
                for _ in 1..count {
                    if z >= last {
                        break;
                    }
                    z = run(z + 1).1;
                }
                if !inner {
                    if z < last {
                        z = run(z + 1).1;
                    } else if a > 0 {
                        a = run(a - 1).0;
                    }
                }
                Some(((a, 0), (z, 0), Kind::Line))
            }
            _ => None,
        }
    }

    // ---- registers, undo ------------------------------------------------

    /// `n` more of the typed key's work done; false (stopping a macro or
    /// `.`) once there's none left.
    fn spend(&mut self, n: usize) -> bool {
        match self.work.checked_sub(n as u64) {
            Some(left) => {
                self.work = left;
                true
            }
            None => {
                self.work = 0;
                self.failed = true;
                false
            }
        }
    }

    /// A register as it's kept (`"` may be another one).
    fn reg(&self, n: char) -> Option<&Reg> {
        match (n, self.unnamed) {
            ('"', Some(r)) => self.regs.get(&r),
            _ => self.regs.get(&n),
        }
    }

    /// A register's text, without the keys a macro may hold that aren't
    /// text (Esc, the arrows...).
    fn get_reg(&mut self, name: Option<char>) -> Option<Reg> {
        let n = name.unwrap_or('"').to_ascii_lowercase();
        let mut r = self.reg(n).cloned()?;
        self.spend(reg_size(&r));
        for l in &mut r.text {
            l.retain(|&c| is_text(c));
        }
        Some(r)
    }

    fn set_reg(&mut self, name: Option<char>, reg: Reg, yank: bool) {
        self.spend(reg_size(&reg));
        match name {
            Some('_') => return,
            // New: an append can't make a register more than a message
            // holds (it fails, stopping a macro), and `"` is then that
            // register itself, not a copy of it.
            Some(c) if c.is_ascii_uppercase() => {
                let c = c.to_ascii_lowercase();
                if self.regs.get(&c).map_or(0, reg_size) + reg_size(&reg) > REG_MAX {
                    self.failed = true;
                    return;
                }
                let e = self.regs.entry(c).or_default();
                if e.lines || reg.lines {
                    e.lines = true;
                    e.text.extend(reg.text);
                } else {
                    let mut t = reg.text.into_iter();
                    match (e.text.last_mut(), t.next()) {
                        (Some(l), Some(first)) => {
                            l.extend(first);
                            e.text.extend(t);
                        }
                        (_, first) => e.text = first.into_iter().chain(t).collect(),
                    }
                }
                self.regs.remove(&'"');
                self.unnamed = Some(c);
                return;
            }
            Some(c) if c != '"' => {
                self.regs.insert(c, reg.clone());
            }
            _ if yank => {
                self.regs.insert('0', reg.clone());
            }
            _ if reg.lines || reg.text.len() > 1 => {
                for i in (1..9).rev() {
                    if let Some(r) = self.regs.remove(&char::from_digit(i, 10).unwrap_or('1')) {
                        self.regs.insert(char::from_digit(i + 1, 10).unwrap_or('9'), r);
                    }
                }
                self.regs.insert('1', reg.clone());
            }
            _ => {
                self.regs.insert('-', reg.clone());
            }
        }
        self.unnamed = None;
        self.regs.insert('"', reg);
    }

    fn snapshot(&mut self, d: &Draft) {
        self.spend(d.lines.iter().map(|l| l.len() + 1).sum());
        self.push_undo((d.lines.clone(), d.row, d.col));
    }

    /// Undo keeps the last 200 changes.
    fn push_undo(&mut self, snap: Snap) {
        if self.undo.len() >= 200 {
            self.undo.drain(..self.undo.len() - 199);
        }
        self.undo.push(snap);
        self.redo.clear();
    }

    fn restore(d: &mut Draft, (lines, row, col): Snap) {
        d.lines = lines;
        d.row = row.min(d.lines.len() - 1);
        d.col = col;
    }

    // ---- commands -------------------------------------------------------

    fn exec(&mut self, d: &mut Draft, cmd: Cmd, s: Scr) -> VimDo {
        let n = cmd.count.unwrap_or(1);
        // Only `j` and `k` keep to the column they began in.
        if !matches!(cmd.act, Act::Move(Motion::Up | Motion::Down)) {
            self.want = None;
        }
        let change = matches!(
            &cmd.act,
            Act::Operate(op, _) | Act::VisOp(op, _) if *op != Op::Yank
        ) || matches!(&cmd.act, Act::Put(_) | Act::Replace(_) | Act::Insert(_) | Act::ReplaceMode | Act::Join(_) | Act::Tilde | Act::Add(_) | Act::VisReplace(_) | Act::VisPut | Act::VisJoin(_));
        if change {
            self.rec = Some(match self.selection(d) {
                // `.` after visual mode: as many lines (and characters on
                // the last) from the cursor, as vim does.
                Some((a, b, lines)) => {
                    let mut keys = vec![if lines { 'V' } else { 'v' }];
                    if b.0 > a.0 {
                        keys.extend(format!("{}j", b.0 - a.0).chars());
                        if !lines {
                            keys.push('0');
                        }
                    }
                    let right = if b.0 > a.0 { b.1 } else { b.1 - a.1 };
                    if !lines && right > 0 {
                        keys.extend(format!("{right}l").chars());
                    }
                    if let Some(c) = cmd.count {
                        keys.extend(c.to_string().chars());
                    }
                    keys.extend(cmd.core.iter().copied());
                    Dot { keys, count: None, reg: cmd.reg }
                }
                None => Dot { keys: cmd.core.clone(), count: cmd.count, reg: cmd.reg },
            });
        }
        let out = self.exec_inner(d, &cmd, n, s);
        // `$` keeps `j` and `k` at lines' ends only as a motion of its own.
        if !matches!(cmd.act, Act::Move(_)) {
            self.want = None;
        }
        if change
            && !matches!(self.mode, Mode::Insert | Mode::Replace)
            && !matches!(out, VimDo::Prompt(..))
            && let Some(r) = self.rec.take()
        {
            self.dot = Some(r);
        }
        out
    }

    fn exec_inner(&mut self, d: &mut Draft, cmd: &Cmd, n: usize, s: Scr) -> VimDo {
        use VimDo::Edit;
        let row = d.row;
        match &cmd.act {
            Act::Move(Motion::Search(fwd)) => {
                self.waiting = Some(cmd.clone());
                VimDo::Prompt(if *fwd { '/' } else { '?' }, String::new())
            }
            Act::Move(m) => {
                match self.motion(d, m, cmd.count, s, false) {
                    Some(((r, c), _)) => {
                        d.row = r.min(nlines(d) - 1);
                        d.col = c;
                    }
                    None => self.failed = true,
                }
                Edit(if self.mode.visual() { Redraw::Rows(row.min(d.row), row.max(d.row)) } else { Redraw::Cursor })
            }
            Act::Operate(op, Target::Motion(Motion::Search(fwd))) => {
                let _ = op;
                self.waiting = Some(cmd.clone());
                VimDo::Prompt(if *fwd { '/' } else { '?' }, String::new())
            }
            Act::Operate(op, target) => self.operate(d, *op, target, cmd, s),
            Act::Put(before) => self.put(d, cmd.reg, *before, n),
            Act::Replace(x) => {
                let l = len(d, row);
                if d.col + n > l || !(is_text(*x) || *x == CR) {
                    self.failed = true;
                    return Edit(Redraw::Cursor);
                }
                self.snapshot(d);
                if *x == CR {
                    d.lines[row].drain(d.col..d.col + n);
                    let tail = d.lines[row].split_off(d.col);
                    d.lines.insert(row + 1, tail);
                    d.row += 1;
                    d.col = 0;
                    return Edit(Redraw::From(row));
                }
                for i in d.col..d.col + n {
                    d.lines[row][i] = *x;
                }
                d.col += n - 1;
                Edit(Redraw::Line(row))
            }
            Act::Insert(at) => {
                self.snapshot(d);
                Edit(self.start_insert(d, *at, n))
            }
            Act::ReplaceMode => {
                self.snapshot(d);
                let r = self.start_insert(d, At::Before, n);
                self.mode = Mode::Replace;
                d.insert = false;
                Edit(r)
            }
            Act::Join(spaces) => {
                let last = (row + n.max(2) - 1).min(nlines(d) - 1);
                if last == row {
                    return Edit(Redraw::Cursor);
                }
                self.snapshot(d);
                Edit(self.join(d, row, last, *spaces))
            }
            Act::Tilde => {
                let l = len(d, row);
                if l == 0 {
                    return Edit(Redraw::Cursor);
                }
                self.snapshot(d);
                let end = (d.col + n).min(l);
                for i in d.col..end {
                    d.lines[row][i] = recase(d.lines[row][i], Op::Toggle);
                }
                d.col = end.min(l - 1);
                Edit(Redraw::Line(row))
            }
            Act::Undo | Act::Redo => {
                let (from, to) = if cmd.act == Act::Undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
                for _ in 0..n {
                    let Some(snap) = from.pop() else { break };
                    to.push((d.lines.clone(), d.row, d.col));
                    Self::restore(d, snap);
                }
                Edit(Redraw::From(0))
            }
            Act::Dot => {
                let Some(dot) = self.dot.clone() else { return Edit(Redraw::Cursor) };
                let mut keys: Vec<char> = Vec::new();
                if let Some(r) = cmd.reg.or(dot.reg) {
                    keys.extend(['"', r]);
                }
                if let Some(c) = cmd.count.or(dot.count) {
                    keys.extend(c.to_string().chars());
                }
                keys.extend(dot.keys.iter().copied());
                self.replay(&keys, 1);
                Edit(Redraw::Cursor)
            }
            Act::Visual(m) => {
                if self.mode == *m || *m == Mode::Normal {
                    return self.leave_visual(d);
                }
                if !self.mode.visual() {
                    self.anchor = (d.row, d.col);
                }
                self.mode = *m;
                let (a, b) = order(self.anchor, (d.row, d.col));
                Edit(Redraw::Rows(a.0, b.0))
            }
            Act::Reselect => {
                let Some((a, b, m)) = self.last_visual else { return Edit(Redraw::Cursor) };
                let last = nlines(d) - 1;
                let (ar, br) = (a.0.min(last), b.0.min(last));
                self.anchor = (ar, a.1.min(len(d, ar).saturating_sub(1)));
                d.row = br;
                d.col = b.1.min(len(d, br).saturating_sub(1));
                self.mode = m;
                Edit(Redraw::From(0))
            }
            Act::Mark(x) => {
                if x.is_ascii_lowercase() {
                    self.marks.insert(*x, (d.row, d.col));
                }
                Edit(Redraw::Cursor)
            }
            Act::Ex => {
                if self.mode.visual() {
                    let _ = self.leave_visual(d);
                    return VimDo::Prompt(':', "'<,'>".into());
                }
                let pre = cmd.count.map(|c| if c == 1 { ".".to_string() } else { format!(".,.+{}", c - 1) }).unwrap_or_default();
                VimDo::Prompt(':', pre)
            }
            Act::Save => VimDo::Save,
            Act::Abort => VimDo::Abort,
            Act::Record(Some(r)) => {
                if r.is_ascii_alphanumeric() {
                    self.macro_rec = Some((r.to_ascii_lowercase(), Vec::new()));
                }
                Edit(Redraw::Cursor)
            }
            Act::Record(None) => {
                if let Some((r, mut keys)) = self.macro_rec.take() {
                    keys.pop();
                    if self.unnamed == Some(r) {
                        // `"` keeps what it was.
                        self.unnamed = None;
                        if let Some(old) = self.regs.get(&r).cloned() {
                            self.regs.insert('"', old);
                        }
                    }
                    self.regs.insert(r, Reg { text: vec![keys], lines: false });
                }
                Edit(Redraw::Cursor)
            }
            Act::Play(r) => {
                let r = if *r == '@' { self.last_macro } else { Some(r.to_ascii_lowercase()) };
                let Some(r) = r else { return Edit(Redraw::Cursor) };
                self.last_macro = Some(r);
                let Some(reg) = self.reg(r).cloned() else { return Edit(Redraw::Cursor) };
                let mut keys: Vec<char> = Vec::new();
                for (i, l) in reg.text.iter().enumerate() {
                    if i > 0 {
                        keys.push(CR);
                    }
                    keys.extend(l.iter().copied());
                }
                if reg.lines {
                    keys.push(CR);
                }
                self.replay(&keys, n);
                Edit(Redraw::Cursor)
            }
            Act::Add(sign) => self.add(d, *sign * n as i64),
            Act::Scroll(k) => VimDo::Scroll(*k * n as isize),
            Act::Half(down) | Act::Page(down) => {
                let by = if matches!(cmd.act, Act::Half(_)) { (s.rows / 2).max(1) } else { s.rows.saturating_sub(2).max(1) } * n;
                let last = nlines(d) - 1;
                d.row = if *down { (row + by).min(last) } else { row.saturating_sub(by) };
                d.col = fnb(d, d.row);
                Edit(Redraw::Cursor)
            }
            Act::Status => {
                let pct = (d.row + 1) * 100 / nlines(d);
                VimDo::Say("FsVimStatus", vec![(d.row + 1).to_string(), nlines(d).to_string(), pct.to_string()])
            }
            Act::Refresh => VimDo::Pass,
            Act::Swap => {
                let here = (d.row, d.col);
                (d.row, d.col) = self.anchor;
                self.anchor = here;
                Edit(Redraw::Cursor)
            }
            Act::VisOp(op, lines) => {
                let (a, b, l) = self.selection(d).unwrap_or(((row, d.col), (row, d.col), false));
                let _ = self.leave_visual(d);
                let kind = if l || *lines { Kind::Line } else { Kind::Incl };
                let times = if matches!(op, Op::Right | Op::Left) { n } else { 1 };
                self.apply(d, *op, (a, b, kind), cmd.reg, times)
            }
            Act::VisReplace(x) => {
                let (a, b, l) = self.selection(d).unwrap_or(((row, d.col), (row, d.col), false));
                let _ = self.leave_visual(d);
                if !is_text(*x) {
                    return Edit(Redraw::Rows(a.0, b.0));
                }
                self.snapshot(d);
                for r in a.0..=b.0 {
                    let (from, to) = if l { (0, len(d, r)) } else { (if r == a.0 { a.1 } else { 0 }, if r == b.0 { (b.1 + 1).min(len(d, r)) } else { len(d, r) }) };
                    for i in from..to {
                        d.lines[r][i] = *x;
                    }
                }
                (d.row, d.col) = a;
                Edit(Redraw::Rows(a.0, b.0))
            }
            Act::VisPut => {
                let (a, b, l) = self.selection(d).unwrap_or(((row, d.col), (row, d.col), false));
                let _ = self.leave_visual(d);
                let Some(reg) = self.get_reg(cmd.reg) else { return Edit(Redraw::Cursor) };
                self.snapshot(d);
                let taken = if l { get(d, a, b, true) } else { get(d, a, (b.0, (b.1 + 1).min(len(d, b.0))), false) };
                if l {
                    remove(d, a, b, true);
                    let at = a.0.min(nlines(d));
                    let lines = reg.text.clone();
                    let all_gone = nlines(d) == 1 && d.lines[0].is_empty() && at == 0;
                    for (i, t) in lines.iter().enumerate() {
                        d.lines.insert(at + i, t.clone());
                    }
                    if all_gone {
                        d.lines.pop();
                    }
                    d.row = at;
                    d.col = fnb(d, at);
                } else {
                    remove(d, a, (b.0, (b.1 + 1).min(len(d, b.0))), false);
                    (d.row, d.col) = a;
                    if reg.lines {
                        let at = a.1.min(len(d, a.0));
                        let tail = d.lines[a.0].split_off(at);
                        let mut at = a.0 + 1;
                        for t in &reg.text {
                            d.lines.insert(at, t.clone());
                            at += 1;
                        }
                        d.lines.insert(at, tail);
                        d.row = a.0 + 1;
                        d.col = 0;
                    } else {
                        let end = insert_text(d, a, &reg.text);
                        d.col = if reg.text.len() == 1 { end.1.saturating_sub(1) } else { a.1 };
                    }
                }
                self.set_reg(None, taken, false);
                fit(d, a.0.min(nlines(d) - 1), nlines(d) - 1);
                d.row = d.row.min(nlines(d) - 1);
                Edit(Redraw::From(a.0.min(d.row)))
            }
            Act::VisJoin(spaces) => {
                let (a, b, _) = self.selection(d).unwrap_or(((row, d.col), (row, d.col), false));
                let _ = self.leave_visual(d);
                let last = b.0.max(a.0 + 1).min(nlines(d) - 1);
                if last == a.0 {
                    return Edit(Redraw::From(a.0));
                }
                self.snapshot(d);
                Edit(merge(self.join(d, a.0, last, *spaces), Redraw::From(a.0)))
            }
            Act::VisObject(inner, x) => {
                let Some((a, b, kind)) = self.object(d, *inner, *x, n) else { return Edit(Redraw::Cursor) };
                let b = if kind == Kind::Excl { prev_pos(d, b).filter(|p| *p >= a).unwrap_or(a) } else { b };
                self.anchor = a;
                (d.row, d.col) = b;
                if kind == Kind::Line {
                    self.mode = Mode::VisualLine;
                }
                Edit(Redraw::Rows(a.0.min(row), b.0.max(row)))
            }
        }
    }

    fn leave_visual(&mut self, d: &Draft) -> VimDo {
        let sel = self.selection(d);
        if let Some((a, b, _)) = sel {
            self.last_visual = Some((a, b, self.mode));
        }
        self.mode = Mode::Normal;
        VimDo::Edit(sel.map_or(Redraw::Cursor, |(a, b, _)| Redraw::Rows(a.0, b.0)))
    }

    fn start_insert(&mut self, d: &mut Draft, at: At, n: usize) -> Redraw {
        let r = d.row;
        let l = len(d, r);
        let mut change = Redraw::Cursor;
        match at {
            At::Before => d.col = d.col.min(l),
            At::After => d.col = (d.col + 1).min(l),
            At::LineStart => d.col = d.lines[r].iter().position(|&c| c != ' ').unwrap_or(l),
            At::Col0 => d.col = 0,
            At::LineEnd => d.col = l,
            At::Last => {
                if let Some(&(row, col)) = self.marks.get(&'^') {
                    d.row = row.min(nlines(d) - 1);
                    d.col = col.min(len(d, d.row));
                }
            }
            At::Below | At::Above => {
                // On a full message the line is taken back (`full`).
                let row = if at == At::Below { r + 1 } else { r };
                d.lines.insert(row, Vec::new());
                d.row = row;
                d.col = 0;
                change = Redraw::From(row.min(r));
            }
        }
        self.mode = Mode::Insert;
        d.insert = true;
        self.want = None;
        self.ins = Ins { count: n, open: matches!(at, At::Below | At::Above).then_some(at == At::Below), ..Ins::default() };
        change
    }

    fn insert_key(&mut self, d: &mut Draft, c: char, s: Scr) -> VimDo {
        if c == ESC || c == '\x03' {
            return VimDo::Edit(self.leave_insert(d, s));
        }
        self.ins.keys.push(c);
        VimDo::Edit(self.typed(d, c))
    }

    /// One key in insert or replace mode.
    fn typed(&mut self, d: &mut Draft, c: char) -> Redraw {
        let row = d.row;
        if std::mem::take(&mut self.ins.ctrl_r) {
            let Some(reg) = self.get_reg(Some(c)) else { return Redraw::Cursor };
            for (i, l) in reg.text.iter().enumerate() {
                if i > 0 {
                    d.enter();
                }
                for &x in l {
                    d.type_char(x);
                }
            }
            if reg.lines {
                d.enter();
            }
            return Redraw::From(row);
        }
        // Replace mode's Backspace restores only what was typed over just
        // before the cursor: another key moving it forgets the rest.
        if self.mode == Mode::Replace && c != BS && !is_text(c) {
            self.ins.replaced.clear();
            self.ins.replaced_at.clear();
        }
        match c {
            CR | '\n' => d.enter(),
            BS => {
                if self.mode == Mode::Replace {
                    let here = d.col.checked_sub(1).map(|c| (row, c));
                    if here.is_none() || self.ins.replaced_at.last().copied() != here {
                        self.ins.replaced.clear();
                        self.ins.replaced_at.clear();
                        d.col = d.col.saturating_sub(1);
                        return Redraw::Cursor;
                    }
                    self.ins.replaced_at.pop();
                    let Some(was) = self.ins.replaced.pop() else { return Redraw::Cursor };
                    d.col -= 1;
                    match was {
                        Some(x) => d.lines[row][d.col] = x,
                        None => {
                            d.lines[row].remove(d.col);
                        }
                    }
                    return Redraw::Tail(row, d.col);
                }
                d.backspace()
            }
            DEL => d.delete(),
            TAB => d.tab(),
            '\x17' => {
                // Ctrl-W: the word before the cursor.
                if d.col == 0 {
                    return d.backspace();
                }
                let line = &d.lines[row];
                let mut a = d.col.min(line.len());
                while a > 0 && line[a - 1] == ' ' {
                    a -= 1;
                }
                if a > 0 {
                    let k = class(Some(line[a - 1]), false);
                    while a > 0 && line[a - 1] != ' ' && class(Some(line[a - 1]), false) == k {
                        a -= 1;
                    }
                }
                d.lines[row].drain(a..d.col);
                d.col = a;
                Redraw::Tail(row, a)
            }
            '\x15' => {
                // Ctrl-U: all before the cursor.
                if d.col == 0 {
                    return d.backspace();
                }
                d.lines[row].drain(..d.col);
                d.col = 0;
                Redraw::Line(row)
            }
            '\x14' | '\x04' => {
                // Ctrl-T, Ctrl-D: shift the line.
                let before = len(d, row);
                shift(d, row, c == '\x14', 1);
                d.col = (d.col + len(d, row)).saturating_sub(before);
                Redraw::Line(row)
            }
            '\x12' => {
                self.ins.ctrl_r = true;
                Redraw::Cursor
            }
            '\x0f' => {
                // Ctrl-O: one normal-mode command.
                self.ins.one_shot = Some(self.mode);
                self.ins.shot_ran = false;
                // A count doesn't type again what came before (`3ia^O...`).
                self.ins.keys.clear();
                self.ins.count = 1;
                self.ins.replaced.clear();
                self.ins.replaced_at.clear();
                self.mode = Mode::Normal;
                Redraw::Cursor
            }
            '\x19' | '\x05' => {
                // Ctrl-Y, Ctrl-E: the character above or below.
                let other = if c == '\x19' { row.checked_sub(1) } else { Some(row + 1) };
                match other.and_then(|o| ch(d, (o, d.col))) {
                    Some(x) => d.type_char(x),
                    None => Redraw::Cursor,
                }
            }
            UP => {
                d.up();
                Redraw::Cursor
            }
            DOWN => {
                d.down();
                Redraw::Cursor
            }
            LEFT => {
                d.left();
                Redraw::Cursor
            }
            RIGHT => {
                d.right();
                Redraw::Cursor
            }
            HOME => {
                d.home();
                Redraw::Cursor
            }
            END => {
                d.end();
                Redraw::Cursor
            }
            PGUP | PGDN => {
                d.page(if c == PGUP { -10 } else { 10 });
                Redraw::Cursor
            }
            c if is_text(c) => {
                if self.mode == Mode::Replace {
                    let (r0, c0) = (d.row, d.col);
                    let was = ch(d, (r0, c0));
                    d.insert = false;
                    let out = d.type_char(c);
                    // A wrap moves what was typed: that can't be taken back.
                    if (d.row, d.col) == (r0, c0 + 1) {
                        self.ins.replaced.push(was);
                        self.ins.replaced_at.push((r0, c0));
                    } else {
                        self.ins.replaced.clear();
                        self.ins.replaced_at.clear();
                    }
                    return out;
                }
                d.type_char(c)
            }
            _ => Redraw::Cursor,
        }
    }

    fn leave_insert(&mut self, d: &mut Draft, _s: Scr) -> Redraw {
        let row = d.row;
        let keys = std::mem::take(&mut self.ins.keys);
        let mut change = Redraw::Cursor;
        // A count types it again (`3ihi`, `2o...`).
        for _ in 1..self.ins.count.min(500) {
            if let Some(below) = self.ins.open {
                if nlines(d) >= MAX_LINES {
                    break;
                }
                let at = if below { d.row + 1 } else { d.row };
                d.lines.insert(at, Vec::new());
                d.row = at;
                d.col = 0;
            }
            for &k in &keys {
                change = merge(change, self.typed(d, k));
            }
        }
        if self.ins.count > 1 {
            change = Redraw::From(row.min(d.row));
        }
        self.mode = Mode::Normal;
        d.insert = true;
        self.marks.insert('^', (d.row, d.col));
        d.col = d.col.saturating_sub(1).min(len(d, d.row).saturating_sub(1));
        if let Some(mut r) = self.rec.take() {
            r.keys.extend(keys);
            r.keys.push(ESC);
            self.dot = Some(r);
        }
        self.ins = Ins::default();
        change
    }

    /// An operator over a motion, a text object or lines.
    fn operate(&mut self, d: &mut Draft, op: Op, target: &Target, cmd: &Cmd, s: Scr) -> VimDo {
        let here = (d.row, d.col.min(len(d, d.row)));
        let n = cmd.count.unwrap_or(1);
        let (a, b, kind) = match target {
            Target::Lines => {
                let last = (here.0 + n - 1).min(nlines(d) - 1);
                (here, (last, 0), Kind::Line)
            }
            Target::Object(inner, x) => match self.object(d, *inner, *x, n) {
                Some(o) => o,
                None => return self.no_target(d, op),
            },
            Target::Motion(m) => {
                // `cw` on a word is `ce`: the word, not the space after it.
                let m = match m {
                    Motion::Word(big) if op == Op::Change && class(ch(d, here), *big) != 0 && len(d, here.0) > 0 => Motion::WordEnd(*big),
                    m => m.clone(),
                };
                let cw = matches!(m, Motion::WordEnd(_)) && matches!(target, Target::Motion(Motion::Word(_)));
                let (mut to, mut kind) = if cw {
                    // As `ce`, but from a word's last letter it's that letter.
                    let big = matches!(m, Motion::WordEnd(true));
                    let mut p = here;
                    let k = class(ch(d, p), big);
                    while p.1 + 1 < len(d, p.0) && class(ch(d, (p.0, p.1 + 1)), big) == k {
                        p.1 += 1;
                    }
                    for _ in 1..n {
                        p = word_end(d, p, big);
                    }
                    (p, Kind::Incl)
                } else {
                    match self.motion(d, &m, cmd.count, s, true) {
                        Some(x) => x,
                        None => return self.no_target(d, op),
                    }
                };
                // `dw` on a line's last word stops at the line's end.
                if matches!(m, Motion::Word(_)) && to.0 > here.0 {
                    let row = to.0 - 1;
                    if to.1 <= fnb(d, to.0) || len(d, to.0) == 0 {
                        to = (row.max(here.0), len(d, row.max(here.0)));
                    }
                }
                // An exclusive motion ending at a line's start ends at the
                // end of the line before (and is whole lines if it began
                // at or before the first character), either way round.
                let (from, mut to) = order(here, to);
                if kind == Kind::Excl && to.0 > from.0 && to.1 == 0 {
                    let row = to.0 - 1;
                    to = (row, len(d, row));
                    if from.1 <= fnb(d, from.0) {
                        kind = Kind::Line;
                    }
                }
                (from, to, kind)
            }
        };
        self.apply(d, op, (a, b, kind), cmd.reg, 1)
    }

    /// An operator with nothing to work on: `c` on an empty line still
    /// begins inserting (`s`, `cl`, `ciw`), as in vim; else it failed.
    fn no_target(&mut self, d: &mut Draft, op: Op) -> VimDo {
        if op == Op::Change && len(d, d.row) == 0 {
            self.snapshot(d);
            self.mode = Mode::Insert;
            d.insert = true;
            d.col = 0;
            self.ins = Ins { count: 1, ..Ins::default() };
            return VimDo::Edit(Redraw::Cursor);
        }
        self.failed = true;
        VimDo::Edit(Redraw::Cursor)
    }

    fn apply(&mut self, d: &mut Draft, op: Op, (a, b, kind): (Pos, Pos, Kind), reg: Option<char>, times: usize) -> VimDo {
        let (a, b) = order(a, b);
        let lines = kind == Kind::Line;
        let b = match kind {
            Kind::Incl => (b.0, (b.1 + 1).min(len(d, b.0))),
            Kind::Excl => (b.0, b.1.min(len(d, b.0))),
            Kind::Line => b,
        };
        let a = (a.0, a.1.min(len(d, a.0)));
        let first = a.0;
        match op {
            Op::Yank => {
                let text = get(d, a, b, lines);
                self.set_reg(reg, text, true);
                if lines {
                    d.row = a.0;
                } else {
                    (d.row, d.col) = a;
                }
                VimDo::Edit(Redraw::Cursor)
            }
            Op::Delete | Op::Change => {
                if !lines && a == b && op == Op::Delete {
                    return VimDo::Edit(Redraw::Cursor);
                }
                self.snapshot(d);
                let text = get(d, a, b, lines);
                self.set_reg(reg, text, false);
                if op == Op::Change && lines {
                    d.lines.drain(a.0..=b.0);
                    d.lines.insert(a.0, Vec::new());
                    d.row = a.0;
                    d.col = 0;
                } else {
                    remove(d, a, b, lines);
                    d.row = a.0.min(nlines(d) - 1);
                    d.col = if lines { fnb(d, d.row) } else { a.1 };
                    fit(d, d.row, d.row);
                }
                if op == Op::Change {
                    self.mode = Mode::Insert;
                    d.insert = true;
                    self.ins = Ins { count: 1, ..Ins::default() };
                }
                VimDo::Edit(Redraw::From(first.min(d.row)))
            }
            Op::Right | Op::Left => {
                self.snapshot(d);
                for r in a.0..=b.0.min(nlines(d) - 1) {
                    shift(d, r, op == Op::Right, times);
                }
                d.row = a.0;
                d.col = fnb(d, a.0);
                VimDo::Edit(Redraw::Rows(a.0, b.0))
            }
            Op::Toggle | Op::Lower | Op::Upper => {
                self.snapshot(d);
                for r in a.0..=b.0.min(nlines(d) - 1) {
                    let (from, to) = if lines { (0, len(d, r)) } else { (if r == a.0 { a.1 } else { 0 }, if r == b.0 { b.1 } else { len(d, r) }) };
                    for i in from..to.min(len(d, r)) {
                        d.lines[r][i] = recase(d.lines[r][i], op);
                    }
                }
                if lines {
                    d.row = a.0;
                } else {
                    (d.row, d.col) = a;
                }
                VimDo::Edit(Redraw::Rows(a.0, b.0))
            }
        }
    }

    fn put(&mut self, d: &mut Draft, name: Option<char>, before: bool, n: usize) -> VimDo {
        let Some(reg) = self.get_reg(name) else { return VimDo::Edit(Redraw::Cursor) };
        if reg.text.is_empty() {
            return VimDo::Edit(Redraw::Cursor);
        }
        let row = d.row;
        if reg.lines {
            let room = MAX_LINES.saturating_sub(nlines(d));
            if room == 0 {
                return VimDo::Edit(Redraw::Cursor);
            }
            self.snapshot(d);
            let at = if before { row } else { row + 1 };
            let all: Vec<Vec<char>> = (0..n).flat_map(|_| reg.text.iter().cloned()).take(room).collect();
            d.lines.splice(at..at, all);
            d.row = at;
            d.col = fnb(d, at);
            return VimDo::Edit(Redraw::From(row.min(at)));
        }
        // No more copies than a message could hold.
        let size: usize = reg.text.iter().map(|l| l.len() + 1).sum();
        let n = n.min((MAX_LINES * (WIDTH + 1) / size.max(1)).max(1));
        self.snapshot(d);
        let l = len(d, row);
        let mut at = (row, if before || l == 0 { d.col.min(l) } else { (d.col + 1).min(l) });
        let start = at;
        for _ in 0..n {
            at = insert_text(d, at, &reg.text);
        }
        let last = fit(d, row, at.0);
        if reg.text.len() == 1 {
            let end = at;
            if end.0 < nlines(d) && end.1 <= len(d, end.0) {
                (d.row, d.col) = (end.0, end.1.saturating_sub(1));
            }
        } else {
            (d.row, d.col) = start;
        }
        d.row = d.row.min(last).min(nlines(d) - 1);
        VimDo::Edit(Redraw::From(row))
    }

    fn join(&mut self, d: &mut Draft, row: usize, last: usize, spaces: bool) -> Redraw {
        for _ in row..last {
            if row + 1 >= nlines(d) {
                break;
            }
            let next: Vec<char> = if spaces { d.lines[row + 1].iter().copied().skip_while(|&c| c == ' ').collect() } else { d.lines[row + 1].clone() };
            let l = if spaces { d.lines[row].iter().rposition(|&c| c != ' ').map_or(0, |i| i + 1) } else { len(d, row) };
            let sep = usize::from(spaces && l > 0 && !next.is_empty() && next[0] != ')');
            if l + sep + next.len() > WIDTH {
                break;
            }
            d.lines[row].truncate(l);
            d.lines.remove(row + 1);
            if sep == 1 {
                d.lines[row].push(' ');
            }
            d.lines[row].extend(next);
            d.col = l;
        }
        d.row = row;
        Redraw::From(row)
    }

    /// Ctrl-A and Ctrl-X: the number at or after the cursor.
    fn add(&mut self, d: &mut Draft, by: i64) -> VimDo {
        let row = d.row;
        let line = &d.lines[row];
        let mut at = d.col.min(line.len());
        while at > 0 && line.get(at).is_some_and(|c| c.is_ascii_digit()) && line[at - 1].is_ascii_digit() {
            at -= 1;
        }
        let Some(start) = (at..line.len()).find(|&i| line[i].is_ascii_digit()) else { return VimDo::Edit(Redraw::Cursor) };
        let mut end = start;
        while end < line.len() && line[end].is_ascii_digit() {
            end += 1;
        }
        let neg = start > 0 && line[start - 1] == '-';
        let from = if neg { start - 1 } else { start };
        let digits: String = line[start..end].iter().collect();
        let Ok(v) = digits.parse::<i64>() else { return VimDo::Edit(Redraw::Cursor) };
        let v = if neg { -v } else { v }.saturating_add(by);
        let text: Vec<char> = v.to_string().chars().collect();
        if line.len() - (end - from) + text.len() > WIDTH {
            return VimDo::Edit(Redraw::Cursor);
        }
        self.snapshot(d);
        let n = text.len();
        d.lines[row].splice(from..end, text);
        d.col = from + n - 1;
        VimDo::Edit(Redraw::Line(row))
    }

    // ---- the prompt's line ----------------------------------------------

    /// The line typed after `:`, `/` or `?` (None if Esc).
    pub fn prompted(&mut self, d: &mut Draft, kind: char, text: Option<String>, s: Scr) -> VimDo {
        self.failed = false;
        self.work = WORK_MAX;
        let out = self.prompted_line(d, kind, text, s, true);
        self.settle(d);
        out
    }

    /// The prompt's line, `typed` or from a macro or `.` (not recorded
    /// again).
    fn prompted_line(&mut self, d: &mut Draft, kind: char, text: Option<String>, s: Scr, typed: bool) -> VimDo {
        if typed && let Some((_, keys)) = &mut self.macro_rec {
            match &text {
                Some(t) => keys.extend(t.chars().chain([CR])),
                None => keys.push(ESC),
            }
        }
        if kind == ':' {
            let out = match text {
                Some(t) => self.ex(d, &t, s),
                None => VimDo::Edit(Redraw::Cursor),
            };
            return self.full(d, out);
        }
        let Some(cmd) = self.waiting.take() else { return VimDo::Edit(Redraw::Cursor) };
        let Some(t) = text else {
            self.rec = None;
            return VimDo::Edit(Redraw::Cursor);
        };
        let pat = if t.is_empty() {
            match &self.search {
                Some((p, _)) => p.clone(),
                None => return VimDo::Edit(Redraw::Cursor),
            }
        } else {
            t.clone()
        };
        let mut core = cmd.core.clone();
        core.extend(t.chars());
        core.push(CR);
        let fwd = kind == '/';
        let m = Motion::Pattern(pat.clone(), fwd);
        let act = match cmd.act {
            Act::Operate(op, _) => Act::Operate(op, Target::Motion(m)),
            _ => Act::Move(m),
        };
        self.failed = false;
        let out = self.exec(d, Cmd { act, core, ..cmd }, s);
        if self.failed {
            return VimDo::Say("FsVimNoMatch", vec![pat]);
        }
        out
    }

    /// The ex line: a range (`.`, `$`, `%`, `N`, `'a`, `'<,'>`, with `+N`
    /// and `-N`) and a command.
    pub fn ex(&mut self, d: &mut Draft, line: &str, s: Scr) -> VimDo {
        let t: Vec<char> = line.trim().trim_start_matches(':').trim_start().chars().collect();
        if t.is_empty() {
            return VimDo::Menu;
        }
        let last = nlines(d) - 1;
        let mut i = 0;
        let mut range: Option<(usize, usize)> = None;
        if t.first() == Some(&'%') {
            range = Some((0, last));
            i = 1;
        } else if let Some(a) = self.address(d, &t, &mut i) {
            let a = a.min(last);
            let mut b = a;
            if t.get(i) == Some(&',') || t.get(i) == Some(&';') {
                i += 1;
                b = self.address(d, &t, &mut i).unwrap_or(a).min(last);
            }
            range = Some((a.min(b), a.max(b)));
        }
        while t.get(i) == Some(&' ') {
            i += 1;
        }
        let rest: String = t[i..].iter().collect();
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let name = if name.is_empty() { rest.chars().take_while(|&c| c == '>' || c == '<' || c == '&').collect() } else { name };
        let after = &rest[name.len()..];
        let bang = after.starts_with('!');
        let arg = after.trim_start_matches('!').trim();
        let (a, b) = range.unwrap_or((d.row, d.row));
        let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name.as_str());
        if name.is_empty() && arg.is_empty() && !bang {
            if range.is_some() {
                self.set_jump(d);
                d.row = b;
                d.col = fnb(d, b);
            }
            return VimDo::Edit(Redraw::Cursor);
        }
        if is("write", 1) || is("wq", 2) || is("xit", 1) || is("exit", 3) || is("update", 2) || name == "wa" || name == "wqa" || name == "xa" {
            return VimDo::Save;
        }
        if is("quit", 1) || name == "qa" || name == "qall" || name == "cq" {
            return if bang || name == "cq" { VimDo::Abort } else { VimDo::AskAbort };
        }
        if is("delete", 1) || is("yank", 1) {
            let reg = arg.chars().next().filter(|c| reg_name(*c));
            let text = get(d, (a, 0), (b, 0), true);
            if name.starts_with('y') {
                self.set_reg(reg, text, true);
                return VimDo::Edit(Redraw::Cursor);
            }
            self.snapshot(d);
            self.set_reg(reg, text, false);
            remove(d, (a, 0), (b, 0), true);
            d.row = a.min(nlines(d) - 1);
            d.col = fnb(d, d.row);
            return VimDo::Edit(Redraw::From(a.min(d.row)));
        }
        if name.starts_with('>') || name.starts_with('<') {
            self.snapshot(d);
            for r in a..=b {
                shift(d, r, name.starts_with('>'), name.len());
            }
            d.row = b;
            d.col = fnb(d, b);
            return VimDo::Edit(Redraw::Rows(a, b));
        }
        if is("join", 1) {
            let b = if range.is_some_and(|r| r.0 != r.1) { b } else { (a + 1).min(last) };
            if b == a {
                return VimDo::Edit(Redraw::Cursor);
            }
            self.snapshot(d);
            return VimDo::Edit(self.join(d, a, b, !bang));
        }
        if is("move", 1) || name == "t" || is("copy", 2) {
            let mut j = 0;
            let to: Vec<char> = arg.chars().collect();
            let Some(dest) = (if to.first() == Some(&'0') { Some(usize::MAX) } else { self.address(d, &to, &mut j) }) else {
                return VimDo::Say("FsVimUnknown", vec![line.trim().to_string()]);
            };
            let block: Vec<Vec<char>> = d.lines[a..=b].to_vec();
            let count = block.len();
            if !name.starts_with('m') && nlines(d) + count > MAX_LINES {
                return VimDo::Edit(Redraw::Cursor);
            }
            if name.starts_with('m') && dest != usize::MAX && (a..=b).contains(&dest) {
                return VimDo::Edit(Redraw::Cursor);
            }
            self.snapshot(d);
            let mut at = if dest == usize::MAX { 0 } else { dest.min(last) + 1 };
            if name.starts_with('m') {
                d.lines.drain(a..=b);
                if at > b {
                    at -= count;
                }
            }
            for (k, l) in block.into_iter().enumerate() {
                d.lines.insert(at + k, l);
            }
            d.row = at + count - 1;
            d.col = fnb(d, d.row);
            return VimDo::Edit(Redraw::From(0));
        }
        if is("substitute", 1) || name == "&" || name == "&&" {
            let (pat, rep, flags) = if name.starts_with('&') {
                match &self.last_sub {
                    Some((p, r, g)) => (p.clone(), r.clone(), if *g || name == "&&" { "g".to_string() } else { String::new() }),
                    None => return VimDo::Edit(Redraw::Cursor),
                }
            } else {
                match split_sub(arg) {
                    Some(x) => x,
                    None => match &self.last_sub {
                        Some((p, r, _)) => (p.clone(), r.clone(), arg.to_string()),
                        None => return VimDo::Edit(Redraw::Cursor),
                    },
                }
            };
            let pat = if pat.is_empty() { self.search.as_ref().map(|s| s.0.clone()).unwrap_or_default() } else { pat };
            if pat.is_empty() {
                return VimDo::Edit(Redraw::Cursor);
            }
            let global = flags.contains('g');
            let icase = flags.contains('i');
            self.last_sub = Some((pat.clone(), rep.clone(), global));
            self.search = Some((pat.clone(), true));
            let Some(mut p) = Pat::new(&pat) else { return VimDo::Say("FsVimNoMatch", vec![pat]) };
            p.icase |= icase;
            self.spend(d.lines.iter().map(|l| l.len() + 1).sum());
            p.left.set(self.work);
            let before = d.lines.clone();
            let (mut hits, mut last_row) = (0, a);
            let mut r = a;
            let mut end = b;
            // New: a line's memo serves all its matches, and a change
            // that couldn't fit in a message, or matching out of steps,
            // stops it (the message as it was).
            while r <= end && r < nlines(d) && nlines(d) <= MAX_LINES {
                let line = d.lines[r].clone();
                let mut memo = p.memo(&line);
                let mut out: Vec<char> = Vec::new();
                let mut at = 0;
                let mut changed = false;
                while at <= line.len() {
                    let Some((s0, e0)) = p.find_in(&line, at, &mut memo) else { break };
                    out.extend_from_slice(&line[at..s0]);
                    out.extend(expand(&rep, &line[s0..e0]));
                    if out.len() > REG_MAX || p.spent() {
                        break;
                    }
                    changed = true;
                    hits += 1;
                    if e0 == s0 {
                        if let Some(&c) = line.get(s0) {
                            out.push(c);
                        }
                        at = s0 + 1;
                    } else {
                        at = e0;
                    }
                    if !global {
                        break;
                    }
                }
                if out.len() > REG_MAX || p.spent() {
                    self.work = p.left.get();
                    self.failed = true;
                    d.lines = before;
                    return if p.spent() { VimDo::Say("FsVimNoMatch", vec![pat]) } else { VimDo::Say("FsVimFull", vec![]) };
                }
                if changed {
                    if at < line.len() {
                        out.extend_from_slice(&line[at..]);
                    }
                    d.lines[r] = out;
                    let was = nlines(d);
                    fit(d, r, r);
                    let more = nlines(d) - was;
                    end += more;
                    last_row = r;
                    r += more;
                }
                r += 1;
            }
            self.work = p.left.get();
            if hits == 0 {
                return VimDo::Say("FsVimNoMatch", vec![pat]);
            }
            self.push_undo((before, d.row, d.col));
            d.row = last_row.min(nlines(d) - 1);
            d.col = fnb(d, d.row);
            return VimDo::Edit(Redraw::From(a.min(d.row)));
        }
        if is("undo", 1) || is("redo", 3) {
            let act = if name.starts_with('u') { Act::Undo } else { Act::Redo };
            return self.exec_inner(d, &Cmd { count: None, reg: None, act, core: Vec::new() }, 1, s);
        }
        if is("nohlsearch", 3) {
            return VimDo::Edit(Redraw::Cursor);
        }
        if is("help", 1) {
            return VimDo::Help;
        }
        if is("set", 2) {
            return VimDo::Say("FsVimNoSet", vec![]);
        }
        VimDo::Say("FsVimUnknown", vec![line.trim().to_string()])
    }

    /// An ex address at `t[*i]`, as a row.
    fn address(&self, d: &Draft, t: &[char], i: &mut usize) -> Option<usize> {
        let last = nlines(d) as isize - 1;
        let mut base: Option<isize> = None;
        match t.get(*i) {
            Some('.') => {
                base = Some(d.row as isize);
                *i += 1;
            }
            Some('$') => {
                base = Some(last);
                *i += 1;
            }
            Some('\'') => {
                let m = *t.get(*i + 1)?;
                let p = match m {
                    '<' => self.last_visual?.0,
                    '>' => self.last_visual?.1,
                    m => *self.marks.get(&m)?,
                };
                base = Some(p.0 as isize);
                *i += 2;
            }
            Some(c) if c.is_ascii_digit() => {
                let mut n = 0isize;
                while let Some(x) = t.get(*i).and_then(|c| c.to_digit(10)) {
                    n = n.saturating_mul(10).saturating_add(x as isize);
                    *i += 1;
                }
                base = Some(n - 1);
            }
            _ => {}
        }
        while let Some(&sign) = t.get(*i).filter(|c| **c == '+' || **c == '-') {
            *i += 1;
            let mut n = 0isize;
            let mut any = false;
            while let Some(x) = t.get(*i).and_then(|c| c.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(x as isize);
                *i += 1;
                any = true;
            }
            let n = if any { n } else { 1 };
            let b = base.unwrap_or(d.row as isize);
            base = Some(if sign == '+' { b.saturating_add(n) } else { b.saturating_sub(n) });
        }
        base.map(|b| b.clamp(0, last) as usize)
    }
}

fn order(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The `n`th match of a pattern from `at` (not at it), wrapping round.
/// New: once round the whole message, the count goes round the matches
/// found on the way instead of searching again.
fn find_pat(d: &Draft, p: &Pat, at: Pos, fwd: bool, n: usize) -> Option<Pos> {
    let rows = nlines(d);
    let mut seen: Vec<Pos> = Vec::new();
    for k in 0..=rows {
        let r = if fwd { (at.0 + k) % rows } else { (at.0 + rows * 2 - k) % rows };
        let starts = p.starts(&d.lines[r]);
        if p.spent() {
            return None;
        }
        // The cursor's line: after it at first, and before it (backward:
        // at or after it) once round.
        let keep = |s: &usize| match (k == 0, k == rows, fwd) {
            (true, _, true) => *s > at.1,
            (true, _, false) => *s < at.1,
            (_, true, true) => *s <= at.1,
            (_, true, false) => *s >= at.1,
            _ => true,
        };
        let here: Vec<usize> = if fwd { starts.into_iter().filter(keep).collect() } else { starts.into_iter().rev().filter(keep).collect() };
        for s in here {
            seen.push((r, s));
            if seen.len() == n {
                return Some((r, s));
            }
        }
    }
    (!seen.is_empty()).then(|| seen[(n.max(1) - 1) % seen.len()])
}

/// `:s/pat/rep/flags`, the delimiter being the first character.
fn split_sub(arg: &str) -> Option<(String, String, String)> {
    let mut it = arg.chars();
    let delim = it.next().filter(|c| !c.is_alphanumeric() && *c != ' ' && *c != '"' && *c != '|')?;
    let mut parts = vec![String::new()];
    let mut esc = false;
    for c in it {
        if esc {
            if c != delim {
                parts.last_mut()?.push('\\');
            }
            parts.last_mut()?.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == delim && parts.len() < 3 {
            parts.push(String::new());
        } else {
            parts.last_mut()?.push(c);
        }
    }
    if esc {
        parts.last_mut()?.push('\\');
    }
    while parts.len() < 3 {
        parts.push(String::new());
    }
    Some((parts[0].clone(), parts[1].clone(), parts[2].clone()))
}

/// A substitution's replacement: `&` is what matched, `\&` an `&`.
fn expand(rep: &str, matched: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut it = rep.chars();
    while let Some(c) = it.next() {
        match c {
            '&' => out.extend_from_slice(matched),
            '\\' => match it.next() {
                Some('0') => out.extend_from_slice(matched),
                Some('t') => out.push(' '),
                Some(x) => out.push(x),
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Scr = Scr { first: 0, rows: 18 };

    fn draft(text: &[&str]) -> Draft {
        Draft::new(&text.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    /// Keys typed; `<esc>`, `<cr>`, `<bs>`, `<up>` and `<C-x>` for control keys.
    fn keys(v: &mut Vim, d: &mut Draft, k: &str) -> VimDo {
        let mut out = VimDo::Edit(Redraw::Cursor);
        let mut rest = k;
        while !rest.is_empty() {
            let (key, n) = if let Some(r) = rest.strip_prefix("<esc>") {
                (Key::Esc, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<cr>") {
                (Key::Enter, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<bs>") {
                (Key::Backspace, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<up>") {
                (Key::Up, rest.len() - r.len())
            } else if rest.starts_with("<C-") && rest.len() > 4 {
                (Key::Char(rest.as_bytes()[3] & 0x1f), 5)
            } else {
                (Key::Char(rest.as_bytes()[0]), 1)
            };
            rest = &rest[n..];
            out = v.key(d, key, S);
        }
        out
    }

    fn text(d: &Draft) -> Vec<String> {
        d.lines.iter().map(|l| l.iter().collect()).collect()
    }

    #[test]
    fn it_starts_in_normal_mode_and_esc_keeps_it_there() {
        let mut v = Vim::new();
        let mut d = draft(&["hello world"]);
        assert_eq!(v.mode, Mode::Normal);
        keys(&mut v, &mut d, "<esc><esc>");
        assert_eq!(v.mode, Mode::Normal, "esc in normal mode stays in normal mode");
        v.key(&mut d, Key::Down, S);
        v.key(&mut d, Key::Right, S);
        assert_eq!(v.mode, Mode::Normal, "the arrows don't leave it");
        assert_eq!(d.col, 1);
        keys(&mut v, &mut d, "x");
        assert_eq!(text(&d), ["hllo world"]);
    }

    #[test]
    fn insert_and_back() {
        let mut v = Vim::new();
        let mut d = draft(&["abc"]);
        keys(&mut v, &mut d, "A def<esc>");
        assert_eq!(text(&d), ["abc def"]);
        assert_eq!((v.mode, d.col), (Mode::Normal, 6));
        keys(&mut v, &mut d, "0ix<esc>");
        assert_eq!(text(&d), ["xabc def"]);
        assert_eq!(d.col, 0);
        keys(&mut v, &mut d, "oline two<esc>Oabove<esc>");
        assert_eq!(text(&d), ["xabc def", "above", "line two"]);
        keys(&mut v, &mut d, "gg3ia<esc>");
        assert_eq!(text(&d)[0], "aaaxabc def");
        keys(&mut v, &mut d, "u");
        assert_eq!(text(&d)[0], "xabc def");
        // `/` at a line's start is a slash, not the editor's menu.
        keys(&mut v, &mut d, "Go/path<esc>");
        assert_eq!(text(&d)[3], "/path");
    }

    #[test]
    fn motions() {
        let mut v = Vim::new();
        let mut d = draft(&["one two-three  four", "", "  five six", "seven"]);
        keys(&mut v, &mut d, "w");
        assert_eq!(d.col, 4);
        keys(&mut v, &mut d, "w");
        assert_eq!(d.col, 7, "punctuation is a word of its own");
        keys(&mut v, &mut d, "W");
        assert_eq!(d.col, 15);
        keys(&mut v, &mut d, "w");
        assert_eq!((d.row, d.col), (1, 0), "an empty line is a word");
        keys(&mut v, &mut d, "w");
        assert_eq!((d.row, d.col), (2, 2));
        keys(&mut v, &mut d, "e");
        assert_eq!(d.col, 5);
        keys(&mut v, &mut d, "b");
        assert_eq!(d.col, 2);
        keys(&mut v, &mut d, "ge");
        assert_eq!((d.row, d.col), (1, 0));
        keys(&mut v, &mut d, "gg$");
        assert_eq!(d.col, 18);
        keys(&mut v, &mut d, "jj");
        assert_eq!((d.row, d.col), (2, 9), "$ sticks to the end going down");
        keys(&mut v, &mut d, "G");
        assert_eq!(d.row, 3);
        keys(&mut v, &mut d, "1G0fe");
        assert_eq!(d.col, 2);
        keys(&mut v, &mut d, ";");
        assert_eq!(d.col, 11);
        keys(&mut v, &mut d, ",");
        assert_eq!(d.col, 2);
        keys(&mut v, &mut d, "0tf");
        assert_eq!(d.col, 14);
        keys(&mut v, &mut d, "3|");
        assert_eq!(d.col, 2);
        keys(&mut v, &mut d, "}");
        assert_eq!(d.row, 1);
        keys(&mut v, &mut d, "}");
        assert_eq!((d.row, d.col), (3, 4), "the last paragraph's end");
        keys(&mut v, &mut d, "{");
        assert_eq!(d.row, 1);
    }

    #[test]
    fn operators_with_motions() {
        let mut v = Vim::new();
        let mut d = draft(&["the quick  brown fox", "jumps"]);
        keys(&mut v, &mut d, "wdw");
        assert_eq!(text(&d)[0], "the brown fox", "dw takes the spaces");
        keys(&mut v, &mut d, "cwred<esc>");
        assert_eq!(text(&d)[0], "the red fox", "cw keeps the space");
        keys(&mut v, &mut d, "$dw");
        assert_eq!(text(&d), ["the red fo", "jumps"], "dw on a line's last word stops at its end");
        keys(&mut v, &mut d, "0d2w");
        assert_eq!(text(&d)[0], "fo");
        keys(&mut v, &mut d, "u0dtd");
        assert_eq!(text(&d)[0], "d fo");
        keys(&mut v, &mut d, "uu0d$");
        assert_eq!(text(&d)[0], "");
        let mut d = draft(&["a (b c) d", "x"]);
        keys(&mut v, &mut d, "fbdi(");
        assert_eq!(text(&d)[0], "a () d");
        keys(&mut v, &mut d, "da(");
        assert_eq!(text(&d)[0], "a  d");
        let mut d = draft(&["say \"hi there\" now"]);
        keys(&mut v, &mut d, "fhci\"yo<esc>");
        assert_eq!(text(&d)[0], "say \"yo\" now");
        keys(&mut v, &mut d, "da\"");
        assert_eq!(text(&d)[0], "say now");
        keys(&mut v, &mut d, "gUiw");
        assert_eq!(text(&d)[0], "say NOW");
        keys(&mut v, &mut d, "g~~");
        assert_eq!(text(&d)[0], "SAY now");
        keys(&mut v, &mut d, ">>");
        assert_eq!(text(&d)[0], "    SAY now");
        keys(&mut v, &mut d, "<<");
        assert_eq!(text(&d)[0], "SAY now");
    }

    #[test]
    fn lines_paragraphs_and_registers() {
        let mut v = Vim::new();
        let mut d = draft(&["Bob wrote:", "", "> one", "> two", "", "my reply", "more"]);
        keys(&mut v, &mut d, "jjdap");
        assert_eq!(text(&d), ["Bob wrote:", "", "my reply", "more"], "dap takes the quote and the blank after it");
        keys(&mut v, &mut d, "Gp");
        assert_eq!(text(&d), ["Bob wrote:", "", "my reply", "more", "> one", "> two", ""]);
        keys(&mut v, &mut d, "gg\"ayyj\"Ayy\"aP");
        assert_eq!(text(&d)[1..3], ["Bob wrote:".to_string(), String::new()]);
        let mut d = draft(&["one", "two", "three"]);
        keys(&mut v, &mut d, "jd}");
        assert_eq!(text(&d), ["one", ""], "d}} to the end takes the text, as vim does");
        keys(&mut v, &mut d, "\"1p");
        assert_eq!(text(&d), ["one", "two", "three"]);
        let mut d = draft(&["abc def"]);
        keys(&mut v, &mut d, "wyiw0P");
        assert_eq!(text(&d), ["defabc def"]);
        keys(&mut v, &mut d, "$p");
        assert_eq!(text(&d), ["defabc defdef"]);
        keys(&mut v, &mut d, "0\"_dw\"0P");
        assert_eq!(text(&d), ["defdefdef"], "the blackhole leaves the yank alone");
    }

    #[test]
    fn small_commands() {
        let mut v = Vim::new();
        let mut d = draft(&["abcdef", "  ghi"]);
        keys(&mut v, &mut d, "3x");
        assert_eq!(text(&d)[0], "def");
        keys(&mut v, &mut d, "rX");
        assert_eq!(text(&d)[0], "Xef");
        keys(&mut v, &mut d, "J");
        assert_eq!(text(&d), ["Xef ghi"]);
        assert_eq!(d.col, 3);
        keys(&mut v, &mut d, "0~~");
        assert_eq!(text(&d)[0], "xEf ghi");
        keys(&mut v, &mut d, "$X");
        assert_eq!(text(&d)[0], "xEf gi");
        keys(&mut v, &mut d, "0sz<esc>");
        assert_eq!(text(&d)[0], "zEf gi");
        keys(&mut v, &mut d, "wD");
        assert_eq!(text(&d)[0], "zEf ");
        keys(&mut v, &mut d, "ccnew<esc>");
        assert_eq!(text(&d)[0], "new");
        keys(&mut v, &mut d, "0Rold<esc>");
        assert_eq!(text(&d)[0], "old");
        keys(&mut v, &mut d, "0Rab<bs><bs><esc>");
        assert_eq!(text(&d)[0], "old", "backspace in replace mode puts back what was there");
        let mut d = draft(&["item 9 of 10"]);
        keys(&mut v, &mut d, "<C-a>");
        assert_eq!(text(&d)[0], "item 10 of 10");
        keys(&mut v, &mut d, "5<C-x>");
        assert_eq!(text(&d)[0], "item 5 of 10");
    }

    #[test]
    fn dot_and_undo_redo() {
        let mut v = Vim::new();
        let mut d = draft(&["a b c d e"]);
        keys(&mut v, &mut d, "dw..");
        assert_eq!(text(&d)[0], "d e");
        keys(&mut v, &mut d, "u");
        assert_eq!(text(&d)[0], "c d e");
        keys(&mut v, &mut d, "<C-r>");
        assert_eq!(text(&d)[0], "d e");
        let mut d = draft(&["one", "two", "three"]);
        keys(&mut v, &mut d, "A!<esc>j.j.");
        assert_eq!(text(&d), ["one!", "two!", "three!"]);
        let mut d = draft(&["x x x x"]);
        keys(&mut v, &mut d, "cwy<esc>w.w2.");
        assert_eq!(text(&d)[0], "y y y", "2. changes two words");
    }

    #[test]
    fn visual_mode() {
        let mut v = Vim::new();
        let mut d = draft(&["one two three", "four", "five"]);
        keys(&mut v, &mut d, "wve");
        assert_eq!(v.mode, Mode::Visual);
        assert_eq!(v.selection(&d), Some(((0, 4), (0, 6), false)));
        keys(&mut v, &mut d, "d");
        assert_eq!(text(&d)[0], "one  three");
        assert_eq!(v.mode, Mode::Normal);
        keys(&mut v, &mut d, "Vjd");
        assert_eq!(text(&d), ["five"]);
        keys(&mut v, &mut d, "P");
        assert_eq!(text(&d), ["one  three", "four", "five"]);
        keys(&mut v, &mut d, "vjy");
        keys(&mut v, &mut d, "VU");
        assert_eq!(text(&d)[0], "ONE  THREE");
        keys(&mut v, &mut d, "viwy");
        assert_eq!(v.get_reg(None).map(|r| r.text), Some(vec!["ONE".chars().collect()]));
        keys(&mut v, &mut d, "Vj>");
        assert_eq!(text(&d)[..2], ["    ONE  THREE".to_string(), "    four".to_string()]);
        keys(&mut v, &mut d, "v<esc>");
        assert_eq!(v.mode, Mode::Normal);
    }

    #[test]
    fn search_and_marks() {
        let mut v = Vim::new();
        let mut d = draft(&["alpha beta", "gamma beta", "delta"]);
        assert_eq!(keys(&mut v, &mut d, "/"), VimDo::Prompt('/', String::new()));
        v.prompted(&mut d, '/', Some("beta".into()), S);
        assert_eq!((d.row, d.col), (0, 6));
        keys(&mut v, &mut d, "n");
        assert_eq!((d.row, d.col), (1, 6));
        keys(&mut v, &mut d, "n");
        assert_eq!((d.row, d.col), (0, 6), "it wraps round");
        keys(&mut v, &mut d, "N");
        assert_eq!(d.row, 1);
        keys(&mut v, &mut d, "gg*");
        assert_eq!(d.row, 0, "alpha is only here");
        keys(&mut v, &mut d, "jmajj'a");
        assert_eq!(d.row, 1);
        keys(&mut v, &mut d, "gg");
        assert_eq!(keys(&mut v, &mut d, "d/"), VimDo::Prompt('/', String::new()));
        v.prompted(&mut d, '/', Some("ga".into()), S);
        assert_eq!(text(&d), ["gamma beta", "delta"], "d/ up to a match on a later line");
        assert!(matches!(v.prompted(&mut d, '/', Some("zzz".into()), S), VimDo::Edit(_)), "nothing waiting");
        keys(&mut v, &mut d, "/");
        assert!(matches!(v.prompted(&mut d, '/', Some("zzz".into()), S), VimDo::Say("FsVimNoMatch", _)));
    }

    #[test]
    fn ex_commands() {
        let mut v = Vim::new();
        let mut d = draft(&["the cat sat", "on the mat", "the end"]);
        assert_eq!(v.ex(&mut d, "wq", S), VimDo::Save);
        assert_eq!(v.ex(&mut d, "x", S), VimDo::Save);
        assert_eq!(v.ex(&mut d, "q", S), VimDo::AskAbort);
        assert_eq!(v.ex(&mut d, "q!", S), VimDo::Abort);
        assert_eq!(v.ex(&mut d, "", S), VimDo::Menu);
        v.ex(&mut d, "s/the/a/", S);
        assert_eq!(text(&d)[0], "a cat sat");
        v.ex(&mut d, "%s/at/og/g", S);
        assert_eq!(text(&d), ["a cog sog", "on the mog", "the end"]);
        v.ex(&mut d, "%s/\\<the\\>/THE/", S);
        assert_eq!(text(&d)[1..], ["on THE mog".to_string(), "THE end".to_string()]);
        v.ex(&mut d, "2,3s/^/> /", S);
        assert_eq!(text(&d)[1..], ["> on THE mog".to_string(), "> THE end".to_string()]);
        v.ex(&mut d, "2", S);
        assert_eq!(d.row, 1);
        v.ex(&mut d, "$d", S);
        assert_eq!(text(&d).len(), 2);
        v.ex(&mut d, "1m$", S);
        assert_eq!(text(&d), ["> on THE mog", "a cog sog"]);
        v.ex(&mut d, "u", S);
        assert_eq!(text(&d), ["a cog sog", "> on THE mog"]);
        assert!(matches!(v.ex(&mut d, "frobnicate", S), VimDo::Say("FsVimUnknown", _)));
        let mut d = draft(&["one", "two", "three"]);
        keys(&mut v, &mut d, "Vj");
        assert_eq!(keys(&mut v, &mut d, ":"), VimDo::Prompt(':', "'<,'>".into()));
        v.prompted(&mut d, ':', Some("'<,'>s/o/0/g".into()), S);
        assert_eq!(text(&d), ["0ne", "tw0", "three"]);
        assert_eq!(keys(&mut v, &mut d, "ZZ"), VimDo::Save);
    }

    #[test]
    fn macros() {
        let mut v = Vim::new();
        let mut d = draft(&["a", "b", "c", "d"]);
        keys(&mut v, &mut d, "qqI- <esc>jq");
        assert_eq!(v.recording(), None);
        keys(&mut v, &mut d, "2@q");
        assert_eq!(text(&d), ["- a", "- b", "- c", "d"]);
        keys(&mut v, &mut d, "@@");
        assert_eq!(text(&d)[3], "- d");
    }

    #[test]
    fn long_lines_wrap() {
        let mut v = Vim::new();
        let word = "word ".repeat(15);
        let mut d = draft(&[word.trim_end(), "tail"]);
        keys(&mut v, &mut d, "yyp");
        keys(&mut v, &mut d, "ggJ");
        assert_eq!(text(&d).len(), 3, "J that would pass the edge doesn't join");
        keys(&mut v, &mut d, "gg0y$$p");
        assert!(d.lines.iter().all(|l| l.len() <= WIDTH));
    }

    #[test]
    fn edges_as_vim_has_them() {
        let mut v = Vim::new();
        // dd on the last line goes up a line; counts; the last line left.
        let mut d = draft(&["a", "b", "c"]);
        keys(&mut v, &mut d, "Gdd");
        assert_eq!((text(&d), d.row), (vec!["a".to_string(), "b".to_string()], 1));
        keys(&mut v, &mut d, "gg5dd");
        assert_eq!(text(&d), [""]);
        keys(&mut v, &mut d, "p");
        assert_eq!(text(&d), ["", "a", "b"]);
        // x at the end steps back; 5x past the end takes the rest.
        let mut d = draft(&["abc"]);
        keys(&mut v, &mut d, "$x");
        assert_eq!((text(&d)[0].as_str(), d.col), ("ab", 1));
        keys(&mut v, &mut d, "05x");
        assert_eq!(text(&d)[0], "");
        // d0, d^, de, cb, dG, dgg, 3J, gJ.
        let mut d = draft(&["  one two", "three", "four", "five"]);
        keys(&mut v, &mut d, "$d^");
        assert_eq!(text(&d)[0], "  o");
        keys(&mut v, &mut d, "ud0");
        assert_eq!(text(&d)[0], "o");
        keys(&mut v, &mut d, "u^de");
        assert_eq!(text(&d)[0], "   two");
        keys(&mut v, &mut d, "u$cbX<esc>");
        assert_eq!(text(&d)[0], "  one Xo");
        keys(&mut v, &mut d, "jdG");
        assert_eq!(text(&d), ["  one Xo"]);
        let mut d = draft(&["a", "  b", "c", "d"]);
        keys(&mut v, &mut d, "3J");
        assert_eq!(text(&d), ["a b c", "d"]);
        keys(&mut v, &mut d, "gJ");
        assert_eq!(text(&d), ["a b cd"]);
        let mut d = draft(&["a", "b", "c"]);
        keys(&mut v, &mut d, "jdgg");
        assert_eq!(text(&d), ["c"]);
        // yw leaves the cursor; P puts before and ends on the last put.
        let mut d = draft(&["ab cd"]);
        keys(&mut v, &mut d, "ywP");
        assert_eq!((text(&d)[0].as_str(), d.col), ("ab ab cd", 2));
        // . after x, after A, after o with a count.
        let mut d = draft(&["abcd"]);
        keys(&mut v, &mut d, "x..");
        assert_eq!(text(&d)[0], "d");
        let mut d = draft(&["x"]);
        keys(&mut v, &mut d, "2oy<esc>");
        assert_eq!(text(&d), ["x", "y", "y"]);
        // Esc at a line's start stays there; arrows in insert move.
        let mut d = draft(&["ab"]);
        keys(&mut v, &mut d, "0i<esc>");
        assert_eq!(d.col, 0);
        keys(&mut v, &mut d, "Ac<up><esc>");
        assert_eq!(text(&d)[0], "abc");
        // % across lines, and ci{ over lines of their own.
        let mut d = draft(&["if (a) {", "    b;", "    c;", "}"]);
        keys(&mut v, &mut d, "$%");
        assert_eq!((d.row, d.col), (3, 0));
        keys(&mut v, &mut d, "kdi{");
        assert_eq!(text(&d), ["if (a) {", "}"]);
        keys(&mut v, &mut d, "ugg0f(ci(x<esc>");
        assert_eq!(text(&d)[0], "if (x) {");
        // r with a count past the end does nothing; ~ at the end stays.
        let mut d = draft(&["ab"]);
        keys(&mut v, &mut d, "5rx");
        assert_eq!(text(&d)[0], "ab");
        keys(&mut v, &mut d, "$~");
        assert_eq!((text(&d)[0].as_str(), d.col), ("aB", 1));
        // A pending command is dropped by Esc; a lone d then waits again.
        let mut d = draft(&["one", "two"]);
        keys(&mut v, &mut d, "d<esc>j");
        assert_eq!((text(&d).len(), d.row), (2, 1));
        // Esc O typed fast opens a line above (not a key code).
        keys(&mut v, &mut d, "ix<esc>Oz<esc>");
        assert_eq!(text(&d), ["one", "z", "xtwo"]);
    }

    #[test]
    fn patterns() {
        let find = |p: &str, s: &str| Pat::new(p).and_then(|p| p.find(&s.chars().collect::<Vec<_>>(), 0));
        assert_eq!(find("b.d", "abcd"), Some((1, 4)));
        assert_eq!(find("a*b", "xaaab"), Some((1, 5)));
        assert_eq!(find("^x", "ax"), None);
        assert_eq!(find("x$", "axb"), None);
        assert_eq!(find("[0-9]\\+", "ab123c"), Some((2, 5)));
        assert_eq!(find("\\cHELLO", "say hello"), Some((4, 9)));
        assert_eq!(find("\\<in\\>", "within in"), Some((7, 9)));
        assert_eq!(find("colou\\=r", "color"), Some((0, 5)));
        assert_eq!(find("\\d\\{2}", "a1b22"), Some((3, 5)));
    }

    /// Keys as `keys` has them, a prompt's line read up to `<cr>`.
    fn typed(v: &mut Vim, d: &mut Draft, k: &str, s: Scr) -> VimDo {
        let mut rest = k;
        let mut out = VimDo::Edit(Redraw::Cursor);
        while !rest.is_empty() {
            let (key, n) = if let Some(r) = rest.strip_prefix("<esc>") {
                (Key::Esc, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<cr>") {
                (Key::Enter, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<bs>") {
                (Key::Backspace, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<up>") {
                (Key::Up, rest.len() - r.len())
            } else if rest.starts_with("<C-") && rest.len() > 4 {
                (Key::Char(rest.as_bytes()[3] & 0x1f), 5)
            } else {
                (Key::Char(rest.as_bytes()[0]), 1)
            };
            rest = &rest[n..];
            out = v.key(d, key, s);
            while let VimDo::Prompt(kind, pre) = &out {
                let (line, after) = rest.split_once("<cr>").unwrap_or((rest, ""));
                let kind = *kind;
                let line = format!("{pre}{line}");
                rest = after;
                out = v.prompted(d, kind, Some(line), s);
            }
        }
        out
    }

    fn lines(n: usize, f: impl Fn(usize) -> String) -> Draft {
        Draft::new(&(0..n).map(f).collect::<Vec<_>>())
    }

    #[test]
    fn runaway_macros_and_dot_stop() {
        let mut v = Vim::new();
        let mut d = draft(&["a", "b"]);
        typed(&mut v, &mut d, "qq@qq@q", S);
        assert!(v.queue.is_empty());
        // Vim's own recursive macro: it stops where `j` can't go on.
        let mut v = Vim::new();
        let mut d = draft(&["one", "two", "three", "four"]);
        typed(&mut v, &mut d, "qaI> <esc>j@aq@a", S);
        assert_eq!(text(&d), ["> one", "> two", "> three", "> four"]);
        let mut v = Vim::new();
        let mut d = draft(&["x"]);
        typed(&mut v, &mut d, "qa@a@aq@a", S);
        typed(&mut v, &mut d, "i<C-o>.<esc>.", S);
        assert!(v.queue.is_empty());
        // A count can't queue a whole message's keys thousands of times.
        let mut d = lines(200, |_| "y".repeat(78));
        typed(&mut v, &mut d, "ggyG9999@0", S);
        assert!(v.queue.len() <= REPLAY_MAX);
    }

    #[test]
    fn patterns_cant_take_forever() {
        let mut v = Vim::new();
        let mut d = lines(200, |_| "a".repeat(79));
        let t = std::time::Instant::now();
        let out = typed(&mut v, &mut d, "/.*.*.*.*.*.*Q<cr>", S);
        assert_eq!(out, VimDo::Say("FsVimNoMatch", vec![".*.*.*.*.*.*Q".into()]));
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    }

    #[test]
    fn undo_and_puts_stay_bounded() {
        let mut v = Vim::new();
        let mut d = draft(&["a"]);
        typed(&mut v, &mut d, "qq:s/a/a/<cr>q999@q", S);
        assert!(v.undo.len() <= 200);
        let mut d = lines(200, |i| format!("line {i}"));
        let t = std::time::Instant::now();
        typed(&mut v, &mut d, "ggvG$y9999p", S);
        assert!(nlines(&d) <= MAX_LINES);
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    }

    #[test]
    fn the_cursor_stays_on_the_text() {
        // `cb` that a re-wrap shortens.
        let mut v = Vim::new();
        let mut d = draft(&[&format!("ab {}", "x".repeat(40)), &"y".repeat(78)]);
        typed(&mut v, &mut d, "jcbz<esc>", S);
        // `:j` refused after trailing spaces, then `F,`.
        let mut v = Vim::new();
        let mut d = draft(&["I think we could meet on Thursday", &"z".repeat(74)]);
        typed(&mut v, &mut d, "A   <esc>:j<cr>F,", S);
        assert_eq!(text(&d)[0], "I think we could meet on Thursday   ");
        // Ctrl-O's yank up, then typing.
        let mut v = Vim::new();
        let mut d = draft(&["hi", "a much longer line"]);
        typed(&mut v, &mut d, "jA<C-o>ykx<esc>", S);
        assert_eq!(text(&d)[0], "hix");
        // `gv` over a line that got shorter.
        let mut v = Vim::new();
        let mut d = draft(&["abcdefgh", "xyz"]);
        typed(&mut v, &mut d, "05lvjygg0Dgvp", S);
        // `L` from a view past the end after a macro's `dG`.
        let mut v = Vim::new();
        let mut d = lines(100, |i| if i == 0 { "ggdGL".into() } else { "x".into() });
        typed(&mut v, &mut d, "\"ayy@a", Scr { first: 50, rows: 18 });
        assert_eq!(text(&d), [""]);
        typed(&mut v, &mut d, ":99999999999999999999<cr>", S);
    }

    #[test]
    fn a_full_message_takes_no_more_lines() {
        let mut v = Vim::new();
        let mut d = lines(200, |i| format!("line {i}"));
        typed(&mut v, &mut d, "G0r<cr>", S);
        assert_eq!((nlines(&d), text(&d)[199].as_str()), (200, "line 199"));
        typed(&mut v, &mut d, "ggr<cr>", S);
        assert_eq!((nlines(&d), text(&d)[199].as_str()), (200, "line 199"));
        assert_eq!(typed(&mut v, &mut d, "Gonew<esc>", S), VimDo::Edit(Redraw::Cursor));
        assert_eq!((nlines(&d), v.mode), (200, Mode::Normal));
        assert!(!text(&d)[199].contains("new"));
        // Enter in insert mode: what's after the cursor must fit on the
        // next line.
        let mut v = Vim::new();
        let mut d = lines(200, |_| "q".repeat(60));
        typed(&mut v, &mut d, "ggwi<cr><esc>", S);
        assert!(d.lines.iter().all(|l| l.len() <= WIDTH));
        assert_eq!(text(&d)[0].len(), 60);
    }

    #[test]
    fn appends_and_counts_stay_bounded() {
        // `"A` in a macro run thousands of times: the register stops at a
        // message's worth, and quickly.
        let mut v = Vim::new();
        let mut d = lines(200, |i| format!("{i} {}", "r".repeat(70)));
        let t = std::time::Instant::now();
        typed(&mut v, &mut d, "ggqq\"AyGq9999@q", S);
        assert!(reg_size(&v.regs[&'a']) <= REG_MAX);
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
        // `"` is what was appended to, with no copy made.
        let mut v = Vim::new();
        let mut d = draft(&["one", "two"]);
        typed(&mut v, &mut d, "\"ayyj\"Ayyp", S);
        assert_eq!(text(&d), ["one", "two", "one", "two"]);
        typed(&mut v, &mut d, "qaqP", S);
        assert_eq!(text(&d), ["one", "two", "one", "two", "one", "two"]);
        // A count goes round the matches rather than the message again.
        let mut v = Vim::new();
        let mut d = draft(&["x a", "a", "y a"]);
        typed(&mut v, &mut d, "/a<cr>", S);
        assert_eq!((d.row, d.col), (0, 2));
        typed(&mut v, &mut d, "3n", S);
        assert_eq!((d.row, d.col), (0, 2));
        typed(&mut v, &mut d, "4n", S);
        assert_eq!((d.row, d.col), (1, 0));
        typed(&mut v, &mut d, "9998n", S);
        assert_eq!((d.row, d.col), (0, 2));
        typed(&mut v, &mut d, "2N", S);
        assert_eq!((d.row, d.col), (1, 0));
        let mut d = lines(200, |_| "ab ".repeat(26));
        let t = std::time::Instant::now();
        typed(&mut v, &mut d, "/a*b<cr>qq9999n9999Nq9999@q", S);
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
        // A pattern as slow as they come runs out of steps, not time.
        let mut d = lines(200, |_| "a".repeat(79));
        let slow = ".*".repeat(50);
        let t = std::time::Instant::now();
        typed(&mut v, &mut d, &format!("/{slow}Q<cr>qqnq9999@q"), S);
        assert!(t.elapsed().as_secs() < 10, "{:?}", t.elapsed());
        assert!(Pat::new(&".".repeat(MAX_PIECES + 1)).is_none());
        // `:s` with empty matches and a long replacement can't build more
        // than a message.
        let mut d = lines(200, |_| "a".repeat(79));
        let t = std::time::Instant::now();
        let out = typed(&mut v, &mut d, &format!(":%s/x*/{}/g<cr>", "&".repeat(60) + "yyyy"), S);
        assert_eq!(out, VimDo::Say("FsVimFull", vec![]));
        assert_eq!(text(&d)[0], "a".repeat(79));
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    }

    #[test]
    fn ctrl_o_runs_its_command_first() {
        let mut v = Vim::new();
        let mut d = draft(&["a b c d"]);
        typed(&mut v, &mut d, "dw0i<C-o>.<esc>", S);
        assert_eq!(text(&d), ["c d"]);
        let mut v = Vim::new();
        let mut d = draft(&["abcd"]);
        typed(&mut v, &mut d, "R<C-o>lx", S);
        assert_eq!((v.mode, text(&d)[0].as_str()), (Mode::Replace, "axcd"));
        let mut v = Vim::new();
        let mut d = draft(&["abcd"]);
        typed(&mut v, &mut d, "i<C-o>v<esc>", S);
        assert_eq!(v.mode, Mode::Insert);
    }

    #[test]
    fn only_text_goes_into_the_message() {
        let mut v = Vim::new();
        let mut d = draft(&["abc"]);
        v.key(&mut d, Key::Char(b'r'), S);
        v.key(&mut d, Key::Up, S);
        typed(&mut v, &mut d, "r<C-a>", S);
        assert_eq!(text(&d), ["abc"]);
        typed(&mut v, &mut d, "qaA!<esc>q\"ap", S);
        assert_eq!(text(&d), ["abc!A!"]);
    }

    #[test]
    fn review_edges() {
        // Replace mode's Backspace after the cursor moved.
        let mut v = Vim::new();
        let mut d = draft(&["hello world", "xyz"]);
        typed(&mut v, &mut d, "jRab<up><bs><bs><esc>", S);
        assert_eq!(text(&d), ["hello world", "abz"]);
        // `s` on an empty line inserts.
        let mut v = Vim::new();
        let mut d = draft(&["abc", "", "def"]);
        typed(&mut v, &mut d, "jshello<esc>", S);
        assert_eq!(text(&d), ["abc", "hello", "def"]);
        typed(&mut v, &mut d, "jo<esc>ciwnew<esc>", S);
        assert_eq!(text(&d), ["abc", "hello", "def", "new"]);
        // `.` after a visual change repeats it.
        let mut v = Vim::new();
        let mut d = draft(&["aaa bbb ccc ddd eee"]);
        typed(&mut v, &mut d, "xwvlld.", S);
        assert_eq!(text(&d), ["aa c ddd eee"]);
        let mut d = draft(&["a", "b", "c", "d"]);
        typed(&mut v, &mut d, "Vj>jj.", S);
        assert_eq!(text(&d), ["    a", "    b", "    c", "    d"]);
        // A backward `db` from a line's start keeps the line break.
        let mut d = draft(&["one two", "three"]);
        typed(&mut v, &mut d, "jdb", S);
        assert_eq!(text(&d), ["one ", "three"]);
        // `D` then `j` goes straight down, not to the line's end.
        let mut d = draft(&["abcdef", "abcdef"]);
        typed(&mut v, &mut d, "3lDj.", S);
        assert_eq!(text(&d), ["abc", "ab"]);
        // `#` from inside a word finds the one before.
        let mut d = draft(&["foo bar foo"]);
        typed(&mut v, &mut d, "$h#", S);
        assert_eq!(d.col, 0);
        // Case changes keep to one character the board has.
        assert_eq!(recase('ß', Op::Upper), 'ß');
    }
}
