//! New: HTML in the file viewer, laid out the way a text browser dumps a
//! page: headings, emphasis, lists, quotes and rules drawn as `markup`
//! draws Markdown, `<pre>` kept as it is, tables in columns, pictures by
//! their alt text, and links numbered, their addresses listed at the end.
//! Scripts, styles and forms are left out. Lines wider than the screen
//! (`<pre>`, tables) are kept whole, for the viewer to scroll sideways.
//! Uploaded pages are anything but tidy, so the reading is forgiving:
//! unclosed `<p>`s, `<li>`s and cells end where the next one starts, and
//! a stray end tag is passed over.

use crate::markup::{push, span, width_of, wrap, Ink, Line};

/// Elements deeper than this are read as if they weren't there: an
/// upload can nest as deep as it likes.
const MAX_DEPTH: usize = 100;
/// Tables in tables deeper than this are read as plain blocks.
const MAX_TABLES: usize = 3;
/// Between table columns.
const GAP: usize = 2;
/// The widest a table column is made before its words wrap, unless the
/// screen has room for more.
const MAX_WORD: usize = 30;
/// The width a cell is measured at.
const UNWRAPPED: usize = 10_000;

enum Node {
    Text(String),
    El(El),
}

struct El {
    tag: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
}

impl El {
    fn new(tag: &str) -> El {
        El { tag: tag.to_string(), attrs: Vec::new(), kids: Vec::new() }
    }

    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn centered(&self) -> bool {
        self.attr("align").is_some_and(|a| a.eq_ignore_ascii_case("center"))
    }
}

const VOID: &[&str] = &["area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img", "input", "isindex", "link", "meta", "param", "source", "spacer", "track", "wbr"];
/// Their contents are text, not read for tags.
const RAW: &[&str] = &["script", "style", "title", "textarea", "xmp", "listing", "plaintext"];
/// Blocks, which end an open `<p>`.
const BLOCK: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "dd", "dir", "div", "dl", "dt", "fieldset", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "listing", "main", "menu", "nav",
    "ol", "p", "pre", "section", "table", "ul", "xmp",
];

/// Does `name` look like a web page?
pub fn is_html(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| matches!(e.to_ascii_lowercase().as_str(), "htm" | "html" | "shtml" | "xhtml"))
}

/// A page's bytes as text: UTF-8 if they are, else Windows-1252 (which
/// Latin-1 pages are too, near enough).
pub fn decode(b: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    String::from_utf8(b.to_vec()).unwrap_or_else(|_| b.iter().map(|&c| if (0x80..0xa0).contains(&c) { HIGH[c as usize - 0x80] } else { c as char }).collect())
}

/// Entities decoded: `&amp;`, `&#65;`, `&#x42;` and the common names.
/// An unknown one is left as it is. `&nbsp;` is a no-break space.
pub fn entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        let semi = rest[end..].starts_with(';');
        let decoded = match name {
            n if n.starts_with('#') => {
                let d = &n[1..];
                if let Some(h) = d.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { d.parse().ok() }.and_then(char::from_u32)
            }
            n => named(n),
        };
        match decoded {
            Some(c) if !name.is_empty() => {
                out.push(c);
                rest = &rest[end + semi as usize..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named(n: &str) -> Option<char> {
    Some(match n {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "bull" => '•',
        "middot" => '·',
        "deg" => '°',
        "plusmn" => '±',
        "frac12" => '½',
        "frac14" => '¼',
        "sup2" => '²',
        "micro" => 'µ',
        "para" => '¶',
        "sect" => '§',
        "cent" => '¢',
        "pound" => '£',
        "yen" => '¥',
        "euro" => '€',
        "iexcl" => '¡',
        "iquest" => '¿',
        "times" => '×',
        "divide" => '÷',
        "szlig" => 'ß',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "aelig" => 'æ',
        "AElig" => 'Æ',
        _ => {
            // Accented letters: `&eacute;`, `&Ouml;`...
            let mut cs = n.chars();
            let b = cs.next()?;
            let mark = cs.as_str();
            let marks: &[(&str, &str, &str)] = &[
                ("grave", "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
                ("acute", "aeiouyAEIOUY", "áéíóúýÁÉÍÓÚÝ"),
                ("circ", "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
                ("uml", "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
                ("tilde", "anoANO", "ãñõÃÑÕ"),
                ("ring", "aA", "åÅ"),
                ("cedil", "cC", "çÇ"),
                ("slash", "oO", "øØ"),
            ];
            let (_, from, to) = marks.iter().find(|(m, _, _)| *m == mark)?;
            to.chars().nth(from.chars().position(|c| c == b)?)?
        }
    })
}

/// Punctuation CP437 hasn't got, as what it has.
fn tidy(c: char, out: &mut String) {
    out.push_str(match c {
        '‘' | '’' | '‚' | '′' => "'",
        '“' | '”' | '„' | '″' => "\"",
        '–' | '—' | '‐' | '‑' | '−' => "-",
        '…' => "...",
        '•' => "∙",
        '©' => "(c)",
        '®' => "(R)",
        '™' => "(TM)",
        '€' => "EUR",
        '\u{200b}' | '\u{feff}' | '\u{ad}' => "",
        c => {
            out.push(c);
            return;
        }
    })
}

fn text_of(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in entities(s).chars() {
        tidy(c, &mut out);
    }
    out
}

// ---- reading -----------------------------------------------------------------

struct Parser {
    stack: Vec<El>,
    title: String,
}

impl Parser {
    fn top(&mut self) -> &mut El {
        self.stack.last_mut().expect("the root stays")
    }

    fn text(&mut self, t: &str) {
        if !t.is_empty() {
            let t = text_of(t);
            self.top().kids.push(Node::Text(t));
        }
    }

    /// End the innermost element.
    fn pop(&mut self) {
        if self.stack.len() > 1 {
            let e = self.stack.pop().expect("more than the root");
            self.top().kids.push(Node::El(e));
        }
    }

    /// End the nearest open one of `which`, and what's open inside it,
    /// unless one of `until` comes first.
    fn end(&mut self, which: &[&str], until: &[&str]) {
        for k in (1..self.stack.len()).rev() {
            let t = self.stack[k].tag.as_str();
            if which.contains(&t) {
                while self.stack.len() > k {
                    self.pop();
                }
                return;
            }
            if until.contains(&t) {
                return;
            }
        }
    }

    fn open(&mut self, el: El) {
        let tag = el.tag.as_str();
        if BLOCK.contains(&tag) {
            self.end(&["p"], &["td", "th", "li", "dd", "dt", "blockquote", "table", "button", "caption"]);
        }
        match tag {
            "li" => self.end(&["li"], &["ul", "ol", "menu", "dir", "table"]),
            "dt" | "dd" => self.end(&["dt", "dd"], &["dl", "table"]),
            "td" | "th" => self.end(&["td", "th"], &["tr", "table"]),
            // A row ends the cell before it, even one in no row.
            "tr" => {
                self.end(&["td", "th"], &["table"]);
                self.end(&["tr"], &["table"]);
            }
            "thead" | "tbody" | "tfoot" => {
                self.end(&["td", "th"], &["table"]);
                self.end(&["tr"], &["table"]);
                self.end(&["thead", "tbody", "tfoot"], &["table"]);
            }
            "option" => self.end(&["option"], &["select"]),
            _ => {}
        }
        if VOID.contains(&tag) || self.stack.len() > MAX_DEPTH {
            self.top().kids.push(Node::El(el));
        } else {
            self.stack.push(el);
        }
    }

    fn close(&mut self, tag: &str) {
        if let Some(k) = (1..self.stack.len()).rev().find(|&k| self.stack[k].tag == tag) {
            while self.stack.len() > k {
                self.pop();
            }
        }
    }
}

/// A tag's attributes, from what follows its name.
fn attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'/') {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if let Some(&q) = b.get(i).filter(|&&q| q == b'"' || q == b'\'') {
                let end = s[i + 1..].find(q as char).map_or(s.len(), |e| i + 1 + e);
                value = entities(&s[i + 1..end]);
                i = end + 1;
            } else {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = entities(&s[start..i]);
            }
        }
        out.push((name, value));
    }
    out
}

/// The page as a tree, and its title.
fn parse(src: &str) -> (El, String) {
    let low = src.to_ascii_lowercase();
    let mut p = Parser { stack: vec![El::new("")], title: String::new() };
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        let Some(lt) = rest.find('<') else {
            p.text(rest);
            break;
        };
        p.text(&rest[..lt]);
        i += lt;
        let rest = &src[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |e| e + 3);
            continue;
        }
        let next = rest[1..].chars().next();
        let tag_start = match next {
            Some('!' | '?') => {
                i += rest.find('>').map_or(rest.len(), |e| e + 1);
                continue;
            }
            Some('/') if rest[2..].starts_with(|c: char| c.is_ascii_alphabetic()) => 2,
            Some(c) if c.is_ascii_alphabetic() => 1,
            _ => {
                p.text("<");
                i += 1;
                continue;
            }
        };
        // The tag's end: the first `>` outside quotes.
        let mut quote = None;
        let end = rest.char_indices().skip(tag_start).find(|&(_, c)| {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None => return c == '>',
            }
            false
        });
        let Some((end, _)) = end else {
            p.text(rest);
            break;
        };
        let inner = &rest[tag_start..end];
        let name_len = inner.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(inner.len());
        let name = inner[..name_len].to_ascii_lowercase();
        i += end + 1;
        if tag_start == 2 {
            p.close(&name);
            continue;
        }
        if RAW.contains(&name.as_str()) {
            let close = format!("</{name}");
            let body_end = low[i..].find(&close).map_or(src.len(), |e| i + e);
            let body = &src[i..body_end];
            i = low[body_end..].find('>').map_or(src.len(), |e| body_end + e + 1);
            match name.as_str() {
                "title" if p.title.is_empty() => p.title = text_of(body).split_whitespace().collect::<Vec<_>>().join(" "),
                "xmp" | "listing" | "plaintext" => {
                    let mut pre = El::new("pre");
                    pre.kids.push(Node::Text(body.to_string()));
                    p.open(pre);
                    p.close("pre");
                }
                _ => {}
            }
            continue;
        }
        let mut el = El::new(&name);
        el.attrs = attrs(&inner[name_len..]);
        p.open(el);
    }
    while p.stack.len() > 1 {
        p.pop();
    }
    let root = p.stack.pop().expect("the root");
    (root, p.title)
}

// ---- laying out --------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
    dim: bool,
    heading: bool,
}

impl Style {
    fn ink(self) -> Ink {
        if self.link {
            Ink::Link
        } else if self.dim {
            Ink::Dim
        } else if self.code {
            Ink::Code
        } else if self.heading {
            Ink::Heading
        } else if self.bold {
            Ink::Bold
        } else if self.italic {
            Ink::Italic
        } else {
            Ink::Text
        }
    }
}

const DIM: Style = Style { bold: false, italic: false, code: false, link: false, dim: true, heading: false };

/// Lines as they're made: what's been laid out, and the words of the
/// paragraph so far.
struct Out {
    lines: Vec<Line>,
    words: Vec<(char, Ink, bool)>,
    width: usize,
    /// In `<pre>`: spaces and line ends as they are.
    pre: bool,
}

impl Out {
    fn new(width: usize, pre: bool) -> Out {
        Out { lines: Vec::new(), words: Vec::new(), width: width.max(1), pre }
    }

    fn text(&mut self, t: &str, st: Style) {
        let ink = st.ink();
        for c in t.chars() {
            if self.pre {
                if c != '\r' {
                    self.words.push((c, ink, false));
                }
                continue;
            }
            let c = if c.is_ascii_whitespace() { ' ' } else { c };
            if c == ' ' && self.words.last().is_none_or(|w| w.0 == ' ') {
                continue;
            }
            self.words.push((c, ink, false));
        }
    }

    /// The paragraph so far laid out; `blank`: an empty one is an empty
    /// line (`<br><br>`).
    fn flush(&mut self, blank: bool) {
        if self.pre {
            let mut line: Line = Vec::new();
            let mut col = 0;
            let mut words = std::mem::take(&mut self.words);
            // A line end straight after `<pre>` isn't part of it.
            if self.lines.is_empty() && words.first().is_some_and(|w| w.0 == '\n') {
                words.remove(0);
            }
            for (c, ink, _) in words {
                match c {
                    '\n' => {
                        self.lines.push(std::mem::take(&mut line));
                        col = 0;
                    }
                    '\t' => {
                        let n = 8 - col % 8;
                        push(&mut line, &" ".repeat(n), ink, false);
                        col += n;
                    }
                    c => {
                        push(&mut line, &c.to_string(), ink, false);
                        col += 1;
                    }
                }
            }
            if !line.is_empty() {
                self.lines.push(line);
            }
            return;
        }
        while self.words.last().is_some_and(|w| w.0 == ' ') {
            self.words.pop();
        }
        if !self.words.is_empty() {
            self.lines.extend(wrap(&self.words, self.width, &Vec::new(), &Vec::new()));
        } else if blank {
            self.lines.push(Vec::new());
        }
        self.words.clear();
    }

    /// An empty line, unless there's one already.
    fn gap(&mut self) {
        self.flush(false);
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(Vec::new());
        }
    }

    fn block(&mut self, lines: Vec<Line>) {
        self.flush(false);
        self.lines.extend(lines);
    }
}

struct Render {
    links: Vec<String>,
    tables: usize,
    lists: usize,
}

/// `lines` with `first` before the first and `next` before the rest.
fn indent(lines: Vec<Line>, first: Line, next: Line) -> Vec<Line> {
    let lines = if lines.is_empty() { vec![Vec::new()] } else { lines };
    lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let mut out = if i == 0 { first.clone() } else { next.clone() };
            if l.is_empty() && i > 0 {
                return Vec::new();
            }
            out.extend(l);
            out
        })
        .collect()
}

fn center(lines: Vec<Line>, width: usize) -> Vec<Line> {
    lines
        .into_iter()
        .map(|l| {
            let w = width_of(&l);
            if l.is_empty() || w >= width {
                return l;
            }
            let mut out = vec![span(" ".repeat((width - w) / 2), Ink::Text, false)];
            out.extend(l);
            out
        })
        .collect()
}

/// Without its trailing spaces.
fn trim_end(mut l: Line) -> Line {
    while let Some(last) = l.last_mut() {
        let t = last.text.trim_end_matches(' ');
        if !t.is_empty() {
            last.text = t.to_string();
            break;
        }
        l.pop();
    }
    l
}

fn text_line(l: &Line) -> String {
    l.iter().map(|s| s.text.as_str()).collect()
}

/// Empty lines one at a time, and none at the start or the end.
fn tidy_gaps(lines: Vec<Line>) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    for l in lines {
        if l.is_empty() && out.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        out.push(l);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

impl Render {
    fn link(&mut self, href: &str) -> usize {
        match self.links.iter().position(|l| l == href) {
            Some(n) => n + 1,
            None => {
                self.links.push(href.to_string());
                self.links.len()
            }
        }
    }

    fn kids(&mut self, e: &El, out: &mut Out, st: Style) {
        for k in &e.kids {
            match k {
                Node::Text(t) => out.text(t, st),
                Node::El(e) => self.node(e, out, st),
            }
        }
    }

    /// An element's contents laid out on their own, `width` wide.
    fn sub(&mut self, e: &El, width: usize, st: Style, pre: bool) -> Vec<Line> {
        let mut out = Out::new(width, pre);
        self.kids(e, &mut out, st);
        out.flush(false);
        out.lines
    }

    fn node(&mut self, e: &El, out: &mut Out, mut st: Style) {
        let width = out.width;
        match e.tag.as_str() {
            "script" | "style" | "head" | "select" | "textarea" | "object" | "applet" | "map" | "button" => {}
            "b" | "strong" | "u" | "ins" => {
                st.bold = true;
                self.kids(e, out, st);
            }
            "i" | "em" | "cite" | "var" | "dfn" => {
                st.italic = true;
                self.kids(e, out, st);
            }
            "code" | "tt" | "kbd" | "samp" => {
                st.code = true;
                self.kids(e, out, st);
            }
            "s" | "strike" | "del" => {
                st.dim = true;
                self.kids(e, out, st);
            }
            "q" => {
                out.text("\"", st);
                self.kids(e, out, st);
                out.text("\"", st);
            }
            "a" => {
                let href = e.attr("href").map(str::trim).filter(|h| !h.is_empty() && !h.starts_with('#') && !h.to_ascii_lowercase().starts_with("javascript:"));
                match href {
                    Some(h) => {
                        let n = self.link(h);
                        self.kids(e, out, Style { link: true, ..st });
                        out.text(&format!("[{n}]"), DIM);
                    }
                    None => self.kids(e, out, st),
                }
            }
            "img" => {
                let alt = e.attr("alt").map(str::trim);
                let what = match alt {
                    Some("") => return,
                    Some(a) => a.to_string(),
                    None => match e.attr("src").and_then(|s| s.rsplit('/').next()).filter(|s| !s.is_empty()) {
                        Some(f) => format!("img: {f}"),
                        None => "img".to_string(),
                    },
                };
                out.text(&format!("[{what}] "), DIM);
            }
            "input" => {
                let kind = e.attr("type").unwrap_or("text").to_ascii_lowercase();
                let label = match kind.as_str() {
                    "submit" | "button" | "reset" => e.attr("value").unwrap_or(&kind),
                    "image" => e.attr("alt").unwrap_or("submit"),
                    _ => return,
                };
                out.text(&format!("[{label}]"), DIM);
            }
            "br" => out.flush(true),
            "hr" => out.block(vec![vec![span("─".repeat(width.min(79)), Ink::Dim, false)]]),
            "frame" | "iframe" => {
                let Some(src) = e.attr("src").filter(|s| !s.is_empty()) else { return };
                let n = self.link(src);
                let name = e.attr("name").or(e.attr("title")).unwrap_or(src);
                out.flush(false);
                out.text(&format!("[frame: {name}]"), Style { link: true, ..st });
                out.text(&format!("[{n}]"), DIM);
                out.flush(false);
            }
            "p" => {
                let lines = self.sub(e, width, st, false);
                out.gap();
                out.block(if e.centered() { center(lines, width) } else { lines });
                out.gap();
            }
            "center" => {
                let lines = self.sub(e, width, st, false);
                out.block(center(lines, width));
            }
            h @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                let mut lines = self.sub(e, width, Style { heading: true, ..st }, false);
                if lines.is_empty() {
                    return;
                }
                if h == "h1" || h == "h2" {
                    let w = lines.iter().map(width_of).max().unwrap_or(0).clamp(3, width);
                    lines.push(vec![span((if h == "h1" { "═" } else { "─" }).repeat(w), Ink::Dim, false)]);
                }
                out.gap();
                out.block(if e.centered() { center(lines, width) } else { lines });
                out.gap();
            }
            "pre" => {
                let lines = self.sub(e, width, st, true);
                out.gap();
                out.block(lines);
                out.gap();
            }
            "blockquote" => {
                let lines = self.sub(e, width.saturating_sub(3), st, false);
                let bar = vec![span(" │ ", Ink::Dim, false)];
                out.gap();
                out.block(tidy_gaps(lines).into_iter().map(|l| {
                    let mut b = bar.clone();
                    b.extend(l);
                    b
                }).collect());
                out.gap();
            }
            "ul" | "ol" | "menu" | "dir" => {
                let lines = self.list(e, width, st);
                if self.lists == 0 {
                    out.gap();
                }
                out.block(lines);
                if self.lists == 0 {
                    out.gap();
                }
            }
            "li" => {
                // One outside a list.
                let lines = self.sub(e, width.saturating_sub(4), st, false);
                out.block(indent(tidy_gaps(lines), vec![span("  ∙ ", Ink::Bullet, false)], vec![span("    ", Ink::Text, false)]));
            }
            "dl" => {
                out.gap();
                for k in &e.kids {
                    match k {
                        Node::El(d) if d.tag == "dt" => {
                            let lines = self.sub(d, width, Style { bold: true, ..st }, false);
                            out.block(lines);
                        }
                        Node::El(d) if d.tag == "dd" => {
                            let lines = self.sub(d, width.saturating_sub(4), st, false);
                            let pad = vec![span("    ", Ink::Text, false)];
                            out.block(indent(tidy_gaps(lines), pad.clone(), pad));
                        }
                        Node::Text(t) => out.text(t, st),
                        Node::El(d) => self.node(d, out, st),
                    }
                }
                out.gap();
            }
            "dt" | "dd" => {
                let lines = self.sub(e, width, st, false);
                out.block(lines);
            }
            "table" => {
                let lines = self.table(e, width, st);
                out.gap();
                out.block(lines);
                out.gap();
            }
            "address" => {
                let lines = self.sub(e, width, Style { italic: true, ..st }, false);
                out.block(lines);
            }
            "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside" | "form" | "fieldset" | "figure" | "figcaption" | "tr" | "td" | "th" | "caption" | "tbody" | "thead" | "tfoot"
            | "noscript" | "noframes" | "frameset" | "body" | "html" | "legend" => {
                let lines = self.sub(e, width, st, false);
                out.block(if e.centered() { center(lines, width) } else { lines });
            }
            _ => self.kids(e, out, st),
        }
    }

    fn list(&mut self, e: &El, width: usize, st: Style) -> Vec<Line> {
        self.lists += 1;
        let ordered = e.tag == "ol";
        let mut n: i64 = e.attr("start").and_then(|s| s.trim().parse().ok()).unwrap_or(1);
        let items = e.kids.iter().filter(|k| matches!(k, Node::El(li) if li.tag == "li")).count() as i64;
        let pad = if ordered { format!("{}", (n + items).max(1)).len() + 4 } else { 4 };
        let mut out = Out::new(width, false);
        for k in &e.kids {
            match k {
                Node::El(li) if li.tag == "li" => {
                    if let Some(v) = li.attr("value").and_then(|v| v.trim().parse().ok()) {
                        n = v;
                    }
                    let lines = tidy_gaps(self.sub(li, width.saturating_sub(pad), st, false));
                    let mark = if ordered { format!("  {:>w$}. ", n, w = pad - 4) } else { "  ∙ ".to_string() };
                    out.block(indent(lines, vec![span(mark, Ink::Bullet, false)], vec![span(" ".repeat(pad), Ink::Text, false)]));
                    n += 1;
                }
                Node::Text(t) if t.trim().is_empty() => {}
                // A list straight in a list, or words outside an item:
                // in as far as an item's.
                Node::El(other) => {
                    let mut tmp = Out::new(width.saturating_sub(pad), false);
                    self.node(other, &mut tmp, st);
                    tmp.flush(false);
                    let p = vec![span(" ".repeat(pad), Ink::Text, false)];
                    out.block(indent(tidy_gaps(tmp.lines), p.clone(), p));
                }
                Node::Text(t) => {
                    let p = vec![span(" ".repeat(pad), Ink::Text, false)];
                    let mut tmp = Out::new(width.saturating_sub(pad), false);
                    tmp.text(t, st);
                    tmp.flush(false);
                    out.block(indent(tmp.lines, p.clone(), p));
                }
            }
        }
        out.flush(false);
        self.lists -= 1;
        out.lines
    }

    fn table(&mut self, e: &El, width: usize, st: Style) -> Vec<Line> {
        fn collect<'a>(e: &'a El, rows: &mut Vec<Vec<&'a El>>, caption: &mut Option<&'a El>) {
            for k in &e.kids {
                let Node::El(k) = k else { continue };
                match k.tag.as_str() {
                    "tr" => rows.push(
                        k.kids.iter().filter_map(|c| match c {
                            Node::El(c) if c.tag == "td" || c.tag == "th" => Some(c),
                            _ => None,
                        }).collect(),
                    ),
                    "thead" | "tbody" | "tfoot" => collect(k, rows, caption),
                    "caption" => *caption = Some(k),
                    "td" | "th" => match rows.last_mut() {
                        Some(r) => r.push(k),
                        None => rows.push(vec![k]),
                    },
                    _ => {}
                }
            }
        }
        let (mut grid, mut caption) = (Vec::new(), None);
        collect(e, &mut grid, &mut caption);
        let span_of = |c: &El, ncols: usize| c.attr("colspan").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, ncols.max(1));
        let ncols = grid.iter().map(|r| r.iter().map(|c| span_of(c, 64)).sum::<usize>()).max().unwrap_or(0).min(64);
        let style_of = |c: &El| if c.tag == "th" { Style { bold: true, ..st } } else { st };
        let mut out = Out::new(width, false);
        if let Some(c) = caption {
            let lines = self.sub(c, width, Style { italic: true, ..st }, false);
            out.block(center(lines, width));
        }
        // A table of one column, or tables too deep in tables: the cells
        // one after another.
        if ncols <= 1 || self.tables >= MAX_TABLES {
            for row in &grid {
                for c in row {
                    let lines = self.sub(c, width, style_of(c), false);
                    out.block(if c.centered() { center(lines, width) } else { lines });
                }
            }
            out.flush(false);
            return out.lines;
        }
        self.tables += 1;
        // Each column as wide as its widest cell, or (when they won't all
        // fit) a fair share, but never narrower than its longest word.
        let (mut natural, mut least) = (vec![0usize; ncols], vec![0usize; ncols]);
        for row in &grid {
            let mut col = 0;
            for c in row {
                let s = span_of(c, ncols);
                if col >= ncols {
                    break;
                }
                if s == 1 {
                    let lines = self.sub(c, UNWRAPPED, style_of(c), false);
                    let w = lines.iter().map(width_of).max().unwrap_or(0);
                    let word = lines.iter().flat_map(|l| text_line(l).split(' ').map(|w| w.chars().count()).collect::<Vec<_>>()).max().unwrap_or(0);
                    natural[col] = natural[col].max(w);
                    least[col] = least[col].max(word.min(MAX_WORD));
                }
                col += s;
            }
        }
        let room = width.saturating_sub(GAP * (ncols - 1));
        let mut widths = natural.clone();
        if natural.iter().sum::<usize>() > room {
            let mut order: Vec<usize> = (0..ncols).collect();
            order.sort_by_key(|&i| natural[i]);
            let mut left = room;
            for (k, &i) in order.iter().enumerate() {
                let fair = left / (ncols - k);
                widths[i] = natural[i].min(fair.max(least[i]).max(1));
                left = left.saturating_sub(widths[i]);
            }
        }
        let spanned = |widths: &[usize], col: usize, s: usize| widths[col..(col + s).min(ncols)].iter().sum::<usize>() + GAP * (s - 1);
        // The cells laid out, and each column as wide as what's in it
        // came out (`<pre>` doesn't wrap).
        let mut laid: Vec<Vec<(usize, Vec<Line>)>> = Vec::new();
        for row in &grid {
            let mut cells = Vec::new();
            let mut col = 0;
            for c in row {
                if col >= ncols {
                    break;
                }
                let s = span_of(c, ncols).min(ncols - col);
                let w = spanned(&widths, col, s).max(1);
                let mut lines = tidy_gaps(self.sub(c, w, style_of(c), false));
                if c.centered() {
                    lines = center(lines, w);
                }
                if s == 1 {
                    widths[col] = widths[col].max(lines.iter().map(width_of).max().unwrap_or(0));
                }
                cells.push((col, lines));
                col += s;
            }
            laid.push(cells);
        }
        self.tables -= 1;
        for cells in laid {
            let height = cells.iter().map(|(_, l)| l.len()).max().unwrap_or(0);
            if height == 0 {
                continue;
            }
            // A row that wraps is set apart from the ones around it.
            if height > 1 {
                out.lines.push(Vec::new());
            }
            for y in 0..height {
                let mut line: Line = Vec::new();
                let mut at = 0;
                for (col, lines) in &cells {
                    let x = widths[..*col].iter().sum::<usize>() + GAP * col;
                    if x > at {
                        push(&mut line, &" ".repeat(x - at), Ink::Text, false);
                        at = x;
                    }
                    if let Some(l) = lines.get(y) {
                        for sp in l {
                            push(&mut line, &sp.text, sp.ink, sp.shade);
                        }
                        at += width_of(l);
                    }
                }
                // A spacer row (`&nbsp;`) is an empty line.
                let line = trim_end(line);
                out.lines.push(if text_line(&line).trim_matches([' ', '\u{a0}']).is_empty() { Vec::new() } else { line });
            }
            if height > 1 {
                out.lines.push(Vec::new());
            }
        }
        out.flush(false);
        // Every row as wide as the table, so that centering it moves
        // the table as one.
        let w = out.lines.iter().map(width_of).max().unwrap_or(0);
        for l in out.lines.iter_mut().filter(|l| !l.is_empty()) {
            let n = w - width_of(l);
            push(l, &" ".repeat(n), Ink::Text, false);
        }
        tidy_gaps(out.lines)
    }
}

/// A page's lines, `width` wide (but for what can't be wrapped).
pub fn render(src: &str, width: usize) -> Vec<Line> {
    let (root, title) = parse(src);
    let mut r = Render { links: Vec::new(), tables: 0, lists: 0 };
    let mut out = Out::new(width, false);
    r.kids(&root, &mut out, Style::default());
    out.flush(false);
    let body = tidy_gaps(out.lines);
    let mut lines: Vec<Line> = Vec::new();
    // The title, unless the page starts by saying it.
    if !title.is_empty() && !body.iter().take(5).any(|l| text_line(l).trim() == title) {
        let t: Vec<(char, Ink, bool)> = title.chars().map(|c| (c, Ink::Heading, false)).collect();
        lines = wrap(&t, width, &Vec::new(), &Vec::new());
        let w = lines.iter().map(width_of).max().unwrap_or(0).clamp(3, width);
        lines.push(vec![span("═".repeat(w), Ink::Dim, false)]);
        lines.push(Vec::new());
    }
    lines.extend(body);
    if !r.links.is_empty() {
        lines.push(Vec::new());
        lines.push(vec![span("Links", Ink::Heading, false)]);
        lines.push(vec![span("─────", Ink::Dim, false)]);
        let w = r.links.len().to_string().len();
        for (i, l) in r.links.iter().enumerate() {
            lines.push(vec![span(format!(" {:>w$}. ", i + 1), Ink::Dim, false), span(l.clone(), Ink::Link, false)]);
        }
    }
    tidy_gaps(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(src: &str, width: usize) -> Vec<String> {
        render(src, width).iter().map(text_line).collect()
    }

    #[test]
    fn blocks_inline_and_links() {
        let page = "<html><head><title>The Page</title><style>p{x}</style></head><body>\
            <h1>Hello</h1><p>Some <b>bold</b> and <a href=\"x.html\">a link</a>.<p>Next<br>line\
            <ul><li>one<li>two<ul><li>deep</ul></ul><!-- gone --><script>alert(1)</script>\
            <img src=\"pics/cat.gif\"><img src=a alt=\"\"><img alt=Logo></body></html>";
        let t = text(page, 40);
        assert_eq!(t, [
            "The Page", "════════", "", "Hello", "═════", "", "Some bold and a link[1].", "", "Next", "line", "", "  ∙ one", "  ∙ two", "      ∙ deep", "", "[img: cat.gif] [Logo]", "", "Links", "─────", " 1. x.html",
        ]);
        let lines = render(page, 40);
        assert!(lines[6].iter().any(|s| s.text == "bold" && s.ink == Ink::Bold));
        assert!(lines[6].iter().any(|s| s.text == "a link" && s.ink == Ink::Link));
    }

    #[test]
    fn pre_keeps_its_spacing_and_width() {
        let wide = "x".repeat(120);
        let t = text(&format!("<pre>\na  b\n\tc &amp; d\n{wide}</pre>after"), 80);
        assert_eq!(t, ["a  b", "        c & d", wide.as_str(), "", "after"]);
    }

    #[test]
    fn tables_in_columns() {
        let t = text("<table><tr><th>Code<th>Country<tr><td>GT-1<td>Guatemala<tr><td colspan=2>both</table>", 80);
        assert_eq!(t, ["Code  Country  ", "GT-1  Guatemala", "both           "]);
        // A heading row in no row, and centered: the columns stay lined up.
        let t = text("<center><table><td>Name<td>What</tr>\n<tr><td>a<td>b</table></center>", 20);
        assert_eq!(t, ["     Name  What", "     a     b   "]);
        // Too wide for the screen: wrapped in the columns.
        let long = "word ".repeat(30);
        let t = text(&format!("<table><tr><td>left<td>{long}</table>"), 40);
        assert!(t.iter().all(|l| l.chars().count() <= 40), "{t:?}");
        assert!(t[0].starts_with("left  word"));
        // A layout table of one column is just its contents.
        assert_eq!(text("<table><tr><td><p>a</p><p>b</p></table>", 80), ["a", "", "b"]);
    }

    #[test]
    fn untidy_pages_and_entities() {
        assert_eq!(text("<p>one<p>two</i></b><li>x", 80), ["one", "", "two", "", "  ∙ x"]);
        assert_eq!(text("a &lt;b&gt; &eacute;&Ouml; &rsquo;q&rsquo; &#233; &bogus; 1 < 2", 80), ["a <b> éÖ 'q' é &bogus; 1 < 2"]);
        assert_eq!(decode(b"caf\xe9 \x93hi\x94"), "café “hi”");
        let deep = "<div>".repeat(5000) + "deep" + &"</div>".repeat(5000);
        assert_eq!(text(&deep, 80), ["deep"]);
        assert!(is_html("INDEX.HTM") && is_html("a.html") && !is_html("a.txt"));
    }

    /// `HTML_IN=a.html:b.html` (or `HTML_LIST=` a file of them) `cargo test --features full -- --ignored html::tests::dump --nocapture`
    #[test]
    #[ignore]
    fn dump() {
        let list = match std::env::var("HTML_LIST") {
            Ok(file) => std::fs::read_to_string(file).unwrap(),
            Err(_) => std::env::var("HTML_IN").unwrap(),
        };
        for path in list.split([':', '\n']).filter(|p| !p.is_empty()) {
            let t = std::time::Instant::now();
            let lines = render(&decode(&std::fs::read(path).unwrap()), 80);
            let wide = lines.iter().map(width_of).max().unwrap_or(0);
            println!("=== {path}: {} lines, {wide} wide, {:?}", lines.len(), t.elapsed());
            if std::env::var("HTML_QUIET").is_err() {
                for l in &lines {
                    println!("{}", text_line(l));
                }
            }
        }
    }
}
