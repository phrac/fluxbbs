//! New: code and Markdown made readable on a terminal. Source files get
//! syntax colors; Markdown gets its headings, emphasis, lists, quotes and
//! rules drawn, and its code (inline, and fenced blocks, colored when they
//! name their language) on a shaded background. The file viewer, the
//! message reader (fenced blocks in posts) and AI chat (its replies,
//! streamed) all use it. Colors are fixed DOS ones; the shade is a 24-bit
//! dark gray where the terminal takes it (`Session::shade_sgr`).

/// What a piece of text is, which decides its color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    Text,
    Keyword,
    Type,
    Str,
    Num,
    Comment,
    Pre,
    Heading,
    Bold,
    Italic,
    Bullet,
    Dim,
    Link,
    Code,
}

impl Ink {
    /// The DOS color (0-15).
    pub fn color(self) -> u8 {
        match self {
            Ink::Text => 7,
            Ink::Keyword | Ink::Bold | Ink::Code => 15,
            Ink::Type | Ink::Italic => 11,
            Ink::Str => 10,
            Ink::Num => 13,
            Ink::Comment => 3,
            Ink::Pre => 12,
            Ink::Heading | Ink::Bullet => 14,
            Ink::Dim => 8,
            Ink::Link => 9,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub ink: Ink,
    pub shade: bool,
}

pub type Line = Vec<Span>;

pub(crate) fn span(text: impl Into<String>, ink: Ink, shade: bool) -> Span {
    Span { text: text.into(), ink, shade }
}

/// Add text to a line, joining it to the last span if it looks the same.
pub(crate) fn push(line: &mut Line, text: &str, ink: Ink, shade: bool) {
    if text.is_empty() {
        return;
    }
    match line.last_mut() {
        Some(s) if s.ink == ink && s.shade == shade => s.text.push_str(text),
        _ => line.push(span(text, ink, shade)),
    }
}

pub fn width_of(line: &Line) -> usize {
    line.iter().map(|s| s.text.chars().count()).sum()
}

// ---- themes ------------------------------------------------------------------

/// A theme's 24-bit colors.
pub struct Colors {
    pub bg: [u8; 3],
    pub text: [u8; 3],
    pub keyword: [u8; 3],
    pub kind: [u8; 3],
    pub string: [u8; 3],
    pub number: [u8; 3],
    pub comment: [u8; 3],
    pub pre: [u8; 3],
    pub heading: [u8; 3],
    pub bold: [u8; 3],
    pub italic: [u8; 3],
    pub bullet: [u8; 3],
    pub dim: [u8; 3],
    pub link: [u8; 3],
}

/// A color theme for code (its colors and background) or Markdown (its
/// headings, emphasis, bullets and links). `classic` is the 16 DOS colors,
/// which terminals without 24-bit color always get.
pub struct Theme {
    pub name: &'static str,
    pub colors: Option<Colors>,
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// The themes; the second (Nord) is the default.
pub const THEMES: [Theme; 5] = [
    Theme { name: "classic", colors: None },
    Theme {
        name: "nord",
        colors: Some(Colors {
            bg: hex(0x2E3440),
            text: hex(0xD8DEE9),
            keyword: hex(0x81A1C1),
            kind: hex(0x8FBCBB),
            string: hex(0xA3BE8C),
            number: hex(0xB48EAD),
            comment: hex(0x616E88),
            pre: hex(0x5E81AC),
            heading: hex(0x88C0D0),
            bold: hex(0xECEFF4),
            italic: hex(0x8FBCBB),
            bullet: hex(0xEBCB8B),
            dim: hex(0x4C566A),
            link: hex(0x81A1C1),
        }),
    },
    Theme {
        name: "dracula",
        colors: Some(Colors {
            bg: hex(0x282A36),
            text: hex(0xF8F8F2),
            keyword: hex(0xFF79C6),
            kind: hex(0x8BE9FD),
            string: hex(0xF1FA8C),
            number: hex(0xBD93F9),
            comment: hex(0x6272A4),
            pre: hex(0x50FA7B),
            heading: hex(0xBD93F9),
            bold: hex(0xFFB86C),
            italic: hex(0xF1FA8C),
            bullet: hex(0xFF79C6),
            dim: hex(0x6272A4),
            link: hex(0x8BE9FD),
        }),
    },
    Theme {
        name: "one dark",
        colors: Some(Colors {
            bg: hex(0x282C34),
            text: hex(0xABB2BF),
            keyword: hex(0xC678DD),
            kind: hex(0xE5C07B),
            string: hex(0x98C379),
            number: hex(0xD19A66),
            comment: hex(0x7F848E),
            pre: hex(0xE06C75),
            heading: hex(0xE06C75),
            bold: hex(0xD19A66),
            italic: hex(0xC678DD),
            bullet: hex(0xE5C07B),
            dim: hex(0x5C6370),
            link: hex(0x61AFEF),
        }),
    },
    Theme {
        name: "monokai",
        colors: Some(Colors {
            bg: hex(0x272822),
            text: hex(0xF8F8F2),
            keyword: hex(0xF92672),
            kind: hex(0x66D9EF),
            string: hex(0xE6DB74),
            number: hex(0xAE81FF),
            comment: hex(0x75715E),
            pre: hex(0xA6E22E),
            heading: hex(0xA6E22E),
            bold: hex(0xFD971F),
            italic: hex(0xE6DB74),
            bullet: hex(0xF92672),
            dim: hex(0x75715E),
            link: hex(0x66D9EF),
        }),
    },
];

/// The theme named `name`, or Nord.
pub fn theme(name: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.name.eq_ignore_ascii_case(name)).unwrap_or(&THEMES[1])
}

impl Theme {
    fn rgb(&self, ink: Ink) -> Option<[u8; 3]> {
        let c = self.colors.as_ref()?;
        Some(match ink {
            Ink::Text | Ink::Code => c.text,
            Ink::Keyword => c.keyword,
            Ink::Type => c.kind,
            Ink::Str => c.string,
            Ink::Num => c.number,
            Ink::Comment => c.comment,
            Ink::Pre => c.pre,
            Ink::Heading => c.heading,
            Ink::Bold => c.bold,
            Ink::Italic => c.italic,
            Ink::Bullet => c.bullet,
            Ink::Dim => c.dim,
            Ink::Link => c.link,
        })
    }
}

/// How code and Markdown are colored for one caller: their themes, the
/// code background (an SGR, empty for none) and whether 24-bit colors go
/// out (else the classic 16).
#[derive(Clone)]
pub struct Look {
    pub code: &'static Theme,
    pub md: &'static Theme,
    pub shade: String,
    pub rgb: bool,
}

impl Look {
    /// A span's 24-bit color, if it has one here: code (shaded) in the
    /// code theme's, the rest in the Markdown theme's.
    pub fn rgb(&self, ink: Ink, shaded: bool) -> Option<[u8; 3]> {
        if !self.rgb {
            return None;
        }
        if shaded { self.code.rgb(ink) } else { self.md.rgb(ink) }
    }

    /// A span's foreground as SGR parameters.
    pub fn fore(&self, ink: Ink, shaded: bool) -> String {
        match self.rgb(ink, shaded) {
            Some([r, g, b]) => format!("38;2;{r};{g};{b}"),
            None => {
                let fg = ink.color();
                format!("{}3{}", if fg >= 8 { "1;" } else { "" }, [0, 4, 2, 6, 1, 5, 3, 7][(fg & 7) as usize])
            }
        }
    }
}

/// Code's backgrounds a caller can pick: name, 24-bit SGR, 16-color SGR
/// (`100` is dark gray, shown with iCE colors). The first, the code
/// theme's own, is the default.
pub const CODE_BACKS: [(&str, &str, &str); 7] = [
    ("theme", "", "100"),
    ("gray", "48;2;58;58;58", "100"),
    ("dim", "48;2;36;36;40", "100"),
    ("blue", "48;2;20;40;84", "44"),
    ("green", "48;2;18;54;30", "42"),
    ("purple", "48;2;58;28;74", "45"),
    ("none", "", ""),
];

/// The background named `name` (the default for anything else).
pub fn code_back(name: &str) -> (&'static str, &'static str, &'static str) {
    CODE_BACKS.iter().copied().find(|b| b.0.eq_ignore_ascii_case(name)).unwrap_or(CODE_BACKS[0])
}

/// Columns a code block stands in from each side.
const MARGIN: usize = 2;

// ---- code ------------------------------------------------------------------

/// A language the highlighter knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    C,
    Rust,
    Python,
    Js,
    Go,
    Java,
    Shell,
    Pascal,
    Asm,
    Lua,
    Basic,
}

struct Syntax {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: &'static [(&'static str, &'static str)],
    quotes: &'static [char],
    /// A line starting with this is a preprocessor line (`#include`).
    pre: Option<char>,
    ignore_case: bool,
}

const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for", "goto", "if", "inline",
    "register", "restrict", "return", "sizeof", "static", "struct", "switch", "typedef", "union", "volatile", "while",
    "class", "namespace", "template", "typename", "public", "private", "protected", "virtual", "new", "delete", "this",
    "try", "catch", "throw", "using", "operator", "nullptr", "true", "false", "NULL", "constexpr",
];
const C_TYPES: &[&str] = &[
    "void", "char", "short", "int", "long", "float", "double", "signed", "unsigned", "bool", "size_t", "ssize_t", "FILE",
    "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t", "int64_t", "string", "auto",
];
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static",
    "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
];
const RUST_TYPES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "bool", "char",
    "str", "String", "Vec", "Option", "Result", "Box", "Some", "None", "Ok", "Err",
];
const PY_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
    "return", "try", "while", "with", "yield", "True", "False", "None", "self",
];
const PY_TYPES: &[&str] = &["int", "str", "float", "list", "dict", "set", "tuple", "bool", "bytes", "object", "print", "len", "range"];
const JS_KEYWORDS: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "export",
    "extends", "finally", "for", "function", "if", "import", "in", "instanceof", "let", "new", "of", "return", "switch",
    "this", "throw", "try", "typeof", "var", "void", "while", "yield", "true", "false", "null", "undefined", "from",
    "interface", "type", "implements",
];
const JS_TYPES: &[&str] = &["number", "string", "boolean", "any", "Array", "Object", "Promise", "Map", "Set", "console"];
const GO_KEYWORDS: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto",
    "if", "import", "interface", "map", "package", "range", "return", "select", "struct", "switch", "type", "var", "true",
    "false", "nil",
];
const GO_TYPES: &[&str] = &["int", "int64", "int32", "uint", "uint8", "uint32", "uint64", "byte", "rune", "string", "bool", "float64", "error"];
const JAVA_KEYWORDS: &[&str] = &[
    "abstract", "break", "case", "catch", "class", "continue", "default", "do", "else", "enum", "extends", "final",
    "finally", "for", "if", "implements", "import", "instanceof", "interface", "new", "package", "private", "protected",
    "public", "return", "static", "super", "switch", "synchronized", "this", "throw", "throws", "try", "while", "true",
    "false", "null", "var", "namespace", "using",
];
const JAVA_TYPES: &[&str] = &["void", "int", "long", "short", "byte", "char", "boolean", "float", "double", "String", "Object", "string"];
const SH_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "return",
    "exit", "export", "local", "echo", "cd", "set", "unset", "shift", "source",
];
const PAS_KEYWORDS: &[&str] = &[
    "and", "array", "asm", "begin", "case", "const", "constructor", "destructor", "div", "do", "downto", "else", "end",
    "file", "for", "function", "goto", "if", "implementation", "in", "inline", "interface", "label", "mod", "nil", "not",
    "object", "of", "or", "packed", "procedure", "program", "record", "repeat", "set", "shl", "shr", "string", "then",
    "to", "type", "unit", "until", "uses", "var", "while", "with", "xor", "true", "false", "exit",
];
const PAS_TYPES: &[&str] = &["integer", "word", "byte", "longint", "shortint", "char", "boolean", "real", "pointer", "text"];
const ASM_KEYWORDS: &[&str] = &[
    "mov", "push", "pop", "call", "ret", "jmp", "je", "jne", "jz", "jnz", "jc", "jnc", "cmp", "add", "sub", "inc", "dec",
    "int", "and", "or", "xor", "not", "shl", "shr", "mul", "div", "lea", "loop", "test", "nop", "db", "dw", "dd", "org",
    "section", "segment", "global", "extern", "bits", "equ", "times", "rep", "movsb", "stosb", "lodsb",
];
const ASM_TYPES: &[&str] = &["ax", "bx", "cx", "dx", "si", "di", "bp", "sp", "al", "ah", "bl", "bh", "cl", "ch", "dl", "dh", "cs", "ds", "es", "ss", "eax", "ebx", "ecx", "edx", "esi", "edi"];
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil", "not",
    "or", "repeat", "return", "then", "true", "until", "while",
];
const BASIC_KEYWORDS: &[&str] = &[
    "print", "input", "if", "then", "else", "end", "for", "to", "step", "next", "goto", "gosub", "return", "dim", "let",
    "while", "wend", "rem", "data", "read", "sub", "function", "do", "loop", "and", "or", "not", "cls", "locate", "color",
];

fn syntax(l: Lang) -> Syntax {
    let c_like = |keywords, types| Syntax { keywords, types, line_comment: &["//"], block_comment: &[("/*", "*/")], quotes: &['"', '\''], pre: None, ignore_case: false };
    match l {
        Lang::C => Syntax { pre: Some('#'), ..c_like(C_KEYWORDS, C_TYPES) },
        Lang::Rust => Syntax { quotes: &['"'], ..c_like(RUST_KEYWORDS, RUST_TYPES) },
        Lang::Js => Syntax { quotes: &['"', '\'', '`'], ..c_like(JS_KEYWORDS, JS_TYPES) },
        Lang::Go => Syntax { quotes: &['"', '\'', '`'], ..c_like(GO_KEYWORDS, GO_TYPES) },
        Lang::Java => c_like(JAVA_KEYWORDS, JAVA_TYPES),
        Lang::Python => Syntax { keywords: PY_KEYWORDS, types: PY_TYPES, line_comment: &["#"], block_comment: &[], quotes: &['"', '\''], pre: None, ignore_case: false },
        Lang::Shell => Syntax { keywords: SH_KEYWORDS, types: &[], line_comment: &["#"], block_comment: &[], quotes: &['"', '\''], pre: None, ignore_case: false },
        Lang::Pascal => Syntax {
            keywords: PAS_KEYWORDS,
            types: PAS_TYPES,
            line_comment: &["//"],
            block_comment: &[("{", "}"), ("(*", "*)")],
            quotes: &['\''],
            pre: None,
            ignore_case: true,
        },
        Lang::Asm => Syntax { keywords: ASM_KEYWORDS, types: ASM_TYPES, line_comment: &[";"], block_comment: &[], quotes: &['"', '\''], pre: Some('%'), ignore_case: true },
        Lang::Lua => Syntax { keywords: LUA_KEYWORDS, types: &[], line_comment: &["--"], block_comment: &[("--[[", "]]")], quotes: &['"', '\''], pre: None, ignore_case: false },
        Lang::Basic => Syntax { keywords: BASIC_KEYWORDS, types: &[], line_comment: &["'"], block_comment: &[], quotes: &['"'], pre: None, ignore_case: true },
    }
}

/// The language of a file, by its extension.
pub fn lang_for_name(name: &str) -> Option<Lang> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    lang_for_tag(&ext)
}

/// The language a fenced block names (```c), or an extension.
pub fn lang_for_tag(tag: &str) -> Option<Lang> {
    Some(match tag.trim().to_ascii_lowercase().as_str() {
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "c++" | "cs" | "csharp" => Lang::C,
        "rs" | "rust" => Lang::Rust,
        "py" | "python" | "python3" => Lang::Python,
        "js" | "javascript" | "ts" | "typescript" | "jsx" | "tsx" | "json" | "mjs" => Lang::Js,
        "go" | "golang" => Lang::Go,
        "java" | "kt" | "kotlin" | "scala" => Lang::Java,
        "sh" | "bash" | "zsh" | "shell" | "console" | "ksh" => Lang::Shell,
        "pas" | "pascal" | "pp" | "dpr" | "inc" => Lang::Pascal,
        "asm" | "s" | "nasm" | "x86" => Lang::Asm,
        "lua" => Lang::Lua,
        "bas" | "basic" | "qb" => Lang::Basic,
        _ => return None,
    })
}

pub fn is_markdown(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mkd"))
}

/// What carries from one line of code to the next: an open block comment.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct CodeState {
    in_comment: Option<&'static str>,
}

fn starts(chars: &[char], i: usize, s: &str) -> bool {
    let mut k = i;
    for c in s.chars() {
        if chars.get(k) != Some(&c) {
            return false;
        }
        k += 1;
    }
    true
}

/// One line of code in colors (tabs as 4 spaces).
pub fn highlight(lang: Lang, line: &str, st: &mut CodeState, shade: bool) -> Line {
    let sx = syntax(lang);
    let chars: Vec<char> = line.replace('\t', "    ").chars().collect();
    let mut out: Line = Vec::new();
    let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = st.in_comment {
            let mut j = i;
            while j < chars.len() && !starts(&chars, j, end) {
                j += 1;
            }
            if j < chars.len() {
                j += end.chars().count();
                st.in_comment = None;
            }
            push(&mut out, &text(i, j), Ink::Comment, shade);
            i = j;
            continue;
        }
        let c = chars[i];
        let at_start = chars[..i].iter().all(|c| c.is_whitespace());
        if at_start && sx.pre == Some(c) {
            push(&mut out, &text(i, chars.len()), Ink::Pre, shade);
            break;
        }
        if sx.line_comment.iter().any(|lc| starts(&chars, i, lc)) && !sx.block_comment.iter().any(|(b, _)| starts(&chars, i, b)) {
            push(&mut out, &text(i, chars.len()), Ink::Comment, shade);
            break;
        }
        if let Some((b, e)) = sx.block_comment.iter().find(|(b, _)| starts(&chars, i, b)) {
            st.in_comment = Some(e);
            push(&mut out, b, Ink::Comment, shade);
            i += b.chars().count();
            continue;
        }
        if sx.quotes.contains(&c) {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                if chars[j] == '\\' && lang != Lang::Pascal {
                    j += 1;
                }
                j += 1;
            }
            let j = (j + 1).min(chars.len());
            push(&mut out, &text(i, j), Ink::Str, shade);
            i = j;
            continue;
        }
        let word_char = |c: char| c.is_alphanumeric() || c == '_';
        if c.is_ascii_digit() && (i == 0 || !word_char(chars[i - 1])) {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_') {
                j += 1;
            }
            push(&mut out, &text(i, j), Ink::Num, shade);
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' || (lang == Lang::Shell && c == '$') {
            let mut j = i + 1;
            while j < chars.len() && word_char(chars[j]) {
                j += 1;
            }
            let w = text(i, j);
            let is = |list: &[&str]| if sx.ignore_case { list.iter().any(|k| k.eq_ignore_ascii_case(&w)) } else { list.contains(&w.as_str()) };
            let ink = if is(sx.keywords) {
                Ink::Keyword
            } else if is(sx.types) {
                Ink::Type
            } else {
                Ink::Text
            };
            // A Basic REM is a comment to the end of the line.
            if lang == Lang::Basic && w.eq_ignore_ascii_case("rem") {
                push(&mut out, &text(i, chars.len()), Ink::Comment, shade);
                break;
            }
            push(&mut out, &w, ink, shade);
            i = j;
            continue;
        }
        push(&mut out, &c.to_string(), Ink::Text, shade);
        i += 1;
    }
    out
}

/// A whole source file as one code block, `width` wide: a shaded line
/// above and below, each line in from the margin (long ones cut).
pub fn render_code(lang: Lang, text: &str, width: usize) -> Vec<Line> {
    let mut st = CodeState::default();
    let mut out = vec![block_line(Vec::new(), width)];
    out.extend(text.lines().map(|l| code_line(Some(lang), l.trim_end_matches('\r'), &mut st, width)));
    out.push(block_line(Vec::new(), width));
    out
}

/// Cut or pad a line to `width` (a code block's shade goes edge to edge).
fn fit(mut line: Line, width: usize, shade: bool) -> Line {
    let mut used = 0;
    let mut out: Line = Vec::new();
    for s in line.drain(..) {
        let room = width.saturating_sub(used);
        let t: String = s.text.chars().take(room).collect();
        used += t.chars().count();
        push(&mut out, &t, s.ink, s.shade);
    }
    if used < width {
        push(&mut out, &" ".repeat(width - used), Ink::Text, shade);
    }
    out
}

/// A fenced code line: in from the margin, colored if the language is
/// known, shaded to the margin on the right.
fn code_line(lang: Option<Lang>, raw: &str, st: &mut CodeState, width: usize) -> Line {
    let mut l = vec![span(" ", Ink::Text, true)];
    match lang {
        Some(lang) => l.extend(highlight(lang, raw, st, true)),
        None => push(&mut l, &raw.replace('\t', "    "), Ink::Text, true),
    }
    block_line(l, width)
}

/// A line of a code block: the margin, then `l` shaded to its width.
fn block_line(l: Line, width: usize) -> Line {
    let mut out = vec![span(" ".repeat(MARGIN), Ink::Text, false)];
    out.extend(fit(l, width.saturating_sub(2 * MARGIN), true));
    out
}

// ---- Markdown ----------------------------------------------------------------

/// What a line of Markdown is, from how it starts.
#[derive(Clone, Debug, PartialEq)]
enum Block {
    Para,
    Heading(usize),
    Bullet,
    Numbered(String),
    Quote,
    Rule,
    Fence(String),
}

/// A line's kind, and where its text starts (in chars).
fn block_of(line: &str) -> (Block, usize) {
    let lead = line.chars().take_while(|c| *c == ' ').count();
    let rest: String = line.chars().skip(lead).collect();
    let t = rest.trim_end();
    if let Some(tag) = t.strip_prefix("```").or_else(|| t.strip_prefix("~~~")) {
        return (Block::Fence(tag.trim().to_string()), line.chars().count());
    }
    if t.len() >= 3 && (t.chars().all(|c| c == '-' || c == ' ') || t.chars().all(|c| c == '*' || c == ' ') || t.chars().all(|c| c == '_' || c == ' ')) && t.chars().filter(|c| !c.is_whitespace()).count() >= 3 {
        return (Block::Rule, line.chars().count());
    }
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && rest.chars().nth(hashes) == Some(' ') {
        return (Block::Heading(hashes), lead + hashes + 1);
    }
    let mut cs = rest.chars();
    if let (Some(b @ ('-' | '*' | '+')), Some(' ')) = (cs.next(), cs.next()) {
        let _ = b;
        return (Block::Bullet, lead + 2);
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) && matches!(rest.chars().nth(digits), Some('.' | ')')) && rest.chars().nth(digits + 1) == Some(' ') {
        return (Block::Numbered(rest.chars().take(digits + 1).collect()), lead + digits + 2);
    }
    if let Some(after) = rest.strip_prefix('>') {
        return (Block::Quote, lead + 1 + usize::from(after.starts_with(' ')));
    }
    (Block::Para, 0)
}

/// Inline Markdown: `code`, **bold**, *italic*, [links](url).
#[derive(Clone, Default)]
struct Inline {
    bold: bool,
    italic: bool,
    code: bool,
    /// `*`s seen and not yet decided on, and the character before them.
    stars: usize,
    before: Option<char>,
}

impl Inline {
    fn ink(&self, base: Ink) -> (Ink, bool) {
        if self.code {
            (Ink::Code, true)
        } else if self.bold {
            (Ink::Bold, false)
        } else if self.italic {
            (Ink::Italic, false)
        } else {
            (base, false)
        }
    }

    /// Decide on pending stars, given the character after them and
    /// whether the same stars come again later on the line (`closes`;
    /// true when that can't be known, streaming). Emphasis opens only at
    /// a word's start (`2*3`, `*.ZIP` stay as they are) and closes what
    /// is open. Returns the stars left over, which are just stars.
    fn settle(&mut self, next: Option<char>, closes: impl Fn(usize) -> bool) -> usize {
        let opens = |me: &Self, n: usize| {
            me.before.is_none_or(|p| !p.is_alphanumeric()) && next.is_some_and(|c| c.is_alphanumeric() || "\"'([".contains(c)) && closes(n)
        };
        let mut n = std::mem::take(&mut self.stars);
        while n >= 2 && (self.bold || opens(self, 2)) {
            n -= 2;
            self.bold = !self.bold;
        }
        if n == 1 && (self.italic || opens(self, 1)) {
            n = 0;
            self.italic = !self.italic;
        }
        n
    }
}

/// Whether `delim` stars come again in `rest` (closing what they'd open).
fn star_run_ahead(rest: &[char], delim: usize) -> bool {
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == '*' {
            let run = rest[i..].iter().take_while(|c| **c == '*').count();
            if run >= delim && i > 0 && !rest[i - 1].is_whitespace() {
                return true;
            }
            i += run;
        } else {
            i += 1;
        }
    }
    false
}

/// Styled characters of one line's text.
fn inline_chars(text: &str, base: Ink, st: &mut Inline) -> Vec<(char, Ink, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let literal = |st: &Inline, out: &mut Vec<(char, Ink, bool)>, n: usize| {
        let (ink, sh) = st.ink(base);
        out.extend(std::iter::repeat_n(('*', ink, sh), n));
    };
    while i < chars.len() {
        let c = chars[i];
        if st.stars > 0 && c != '*' {
            let n = st.settle(Some(c), |d| star_run_ahead(&chars[i..], d));
            literal(st, &mut out, n);
        }
        // A backtick opens code only if another closes it on the line.
        if c == '`' && (st.code || chars[i + 1..].contains(&'`')) {
            st.code = !st.code;
            st.before = Some(c);
            i += 1;
            continue;
        }
        if st.code {
            out.push((c, Ink::Code, true));
            i += 1;
            continue;
        }
        if c == '*' {
            if st.stars == 0 {
                st.before = i.checked_sub(1).map(|k| chars[k]);
            }
            st.stars += 1;
            i += 1;
            continue;
        }
        // [text](url): the text, then the address dimmed.
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            let img = c == '!';
            let open = i + usize::from(img);
            if let Some(close) = (open + 1..chars.len()).find(|&k| chars[k] == ']')
                && chars.get(close + 1) == Some(&'(')
                && let Some(end) = (close + 2..chars.len()).find(|&k| chars[k] == ')')
            {
                let label: String = chars[open + 1..close].iter().collect();
                let url: String = chars[close + 2..end].iter().collect();
                let shown = if img { format!("[image: {label}]") } else { label };
                out.extend(shown.chars().map(|c| (c, Ink::Link, false)));
                if !url.is_empty() && !img {
                    out.extend(format!(" <{url}>").chars().map(|c| (c, Ink::Dim, false)));
                }
                i = end + 1;
                continue;
            }
        }
        let (ink, sh) = st.ink(base);
        out.push((c, ink, sh));
        i += 1;
    }
    let n = st.settle(None, |_| false);
    literal(st, &mut out, n);
    out
}

/// Words of styled characters wrapped into lines `width` wide; the first
/// line starts with `first`, later ones with `next`.
pub(crate) fn wrap(chars: &[(char, Ink, bool)], width: usize, first: &Line, next: &Line) -> Vec<Line> {
    struct W<'a> {
        lines: Vec<Line>,
        cur: Line,
        col: usize,
        /// Where the current line's text starts (after its prefix).
        start: usize,
        width: usize,
        next: &'a Line,
    }
    impl W<'_> {
        fn break_line(&mut self) {
            self.lines.push(std::mem::replace(&mut self.cur, self.next.clone()));
            self.col = width_of(self.next);
            self.start = self.col;
        }
        fn word(&mut self, word: &mut Vec<(char, Ink, bool)>) {
            if word.is_empty() {
                return;
            }
            if self.col > self.start && self.col + word.len() > self.width {
                self.break_line();
            }
            for &(c, ink, sh) in word.iter() {
                if self.col >= self.width {
                    self.break_line();
                }
                push(&mut self.cur, &c.to_string(), ink, sh);
                self.col += 1;
            }
            word.clear();
        }
    }
    let start = width_of(first);
    let mut w = W { lines: Vec::new(), cur: first.clone(), col: start, start, width, next };
    let mut word: Vec<(char, Ink, bool)> = Vec::new();
    for &(c, ink, sh) in chars {
        if c == ' ' && !sh {
            w.word(&mut word);
            if w.col > w.start && w.col < width {
                push(&mut w.cur, " ", ink, sh);
                w.col += 1;
            }
        } else {
            word.push((c, ink, sh));
        }
    }
    w.word(&mut word);
    let mut lines = w.lines;
    lines.push(w.cur);
    for l in lines.iter_mut() {
        // No trailing plain spaces (shaded ones are kept: inline code).
        while let Some(last) = l.last_mut() {
            if last.shade || !last.text.ends_with(' ') {
                break;
            }
            let t = last.text.trim_end().to_string();
            if t.is_empty() {
                l.pop();
            } else {
                last.text = t;
                break;
            }
        }
    }
    lines
}

/// A Markdown file's lines, `width` wide.
pub fn render_markdown(text: &str, width: usize) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut fence: Option<Option<Lang>> = None;
    let mut code = CodeState::default();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        if let Some(lang) = fence {
            if matches!(block_of(raw).0, Block::Fence(_)) {
                fence = None;
                out.push(block_line(Vec::new(), width));
            } else {
                out.push(code_line(lang, raw, &mut code, width));
            }
            continue;
        }
        out.extend(markdown_line(raw, width, &mut fence, &mut code));
    }
    out
}

/// One line of Markdown outside a code block (it may open one).
fn markdown_line(raw: &str, width: usize, fence: &mut Option<Option<Lang>>, code: &mut CodeState) -> Vec<Line> {
    let (block, at) = block_of(raw);
    let text: String = raw.chars().skip(at).collect();
    let mut st = Inline::default();
    match block {
        Block::Fence(tag) => {
            *fence = Some(lang_for_tag(&tag));
            *code = CodeState::default();
            let label = if tag.is_empty() { String::new() } else { format!(" {tag}") };
            vec![block_line(vec![span(label, Ink::Comment, true)], width)]
        }
        Block::Rule => vec![vec![span("─".repeat(width.min(79)), Ink::Dim, false)]],
        Block::Heading(n) => {
            let chars = inline_chars(&text, Ink::Heading, &mut st);
            let mut lines = wrap(&chars, width, &Vec::new(), &Vec::new());
            if n <= 2 {
                let w = lines.iter().map(width_of).max().unwrap_or(0).max(3);
                lines.push(vec![span((if n == 1 { "═" } else { "─" }).repeat(w), Ink::Dim, false)]);
            }
            lines
        }
        Block::Bullet => {
            let chars = inline_chars(&text, Ink::Text, &mut st);
            wrap(&chars, width, &vec![span("  ∙ ", Ink::Bullet, false)], &vec![span("    ", Ink::Text, false)])
        }
        Block::Numbered(n) => {
            let chars = inline_chars(&text, Ink::Text, &mut st);
            let pad = n.chars().count() + 3;
            wrap(&chars, width, &vec![span(format!("  {n} "), Ink::Bullet, false)], &vec![span(" ".repeat(pad), Ink::Text, false)])
        }
        Block::Quote => {
            let chars = inline_chars(&text, Ink::Text, &mut st);
            let bar = vec![span(" │ ", Ink::Dim, false)];
            wrap(&chars, width, &bar, &bar)
        }
        Block::Para => {
            if raw.trim().is_empty() {
                return vec![Vec::new()];
            }
            let chars = inline_chars(raw, Ink::Text, &mut st);
            wrap(&chars, width, &Vec::new(), &Vec::new())
        }
    }
}

// ---- streamed Markdown (AI chat) ---------------------------------------------

#[cfg(feature = "ai")]
/// What a stream gives to write: text in a style, or a new line.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text(String, Ink, bool),
    Newline,
}

#[cfg(feature = "ai")]
/// Markdown arriving a piece at a time, written as it comes: a line's
/// kind is decided from its first characters, words go out as they end,
/// and a code block goes a line at a time.
pub struct MdStream {
    width: usize,
    col: usize,
    /// The text so far of a line whose kind isn't known yet.
    head: Option<String>,
    block: Block,
    inline: Inline,
    word: Vec<(char, Ink, bool)>,
    /// In a code block (with its language), and the line so far.
    fence: Option<Option<Lang>>,
    code: CodeState,
    code_line: String,
    /// Where continuation lines start, and what they start with.
    indent: Line,
}

#[cfg(feature = "ai")]
impl MdStream {
    /// `col`: where the first line starts (after the speaker's name).
    pub fn new(width: usize, col: usize) -> Self {
        Self {
            width,
            col,
            head: Some(String::new()),
            block: Block::Para,
            inline: Inline::default(),
            word: Vec::new(),
            fence: None,
            code: CodeState::default(),
            code_line: String::new(),
            indent: Vec::new(),
        }
    }

    pub fn feed(&mut self, text: &str) -> Vec<Piece> {
        let mut out = Vec::new();
        for c in text.chars() {
            self.char(c, &mut out);
        }
        out
    }

    /// The rest, at the end of the reply.
    pub fn finish(&mut self) -> Vec<Piece> {
        let mut out = Vec::new();
        if let Some(h) = self.head.take() {
            self.start_line(&h, true, &mut out);
        }
        if self.fence.is_some() && !self.code_line.is_empty() {
            let line = std::mem::take(&mut self.code_line);
            // The closing fence, without a newline after it.
            if matches!(block_of(&line).0, Block::Fence(_)) {
                self.fence = None;
                self.emit_line(block_line(Vec::new(), self.width), &mut out);
                return out;
            }
            let l = code_line(self.fence.unwrap_or(None), &line, &mut self.code, self.width);
            self.emit_line(l, &mut out);
        } else {
            self.flush_word(&mut out);
        }
        out
    }

    fn char(&mut self, c: char, out: &mut Vec<Piece>) {
        if let Some(lang) = self.fence {
            if c == '\n' {
                let line = std::mem::take(&mut self.code_line);
                if matches!(block_of(&line).0, Block::Fence(_)) {
                    self.fence = None;
                    self.head = Some(String::new());
                    // A shaded line to close it, as at the top.
                    self.emit_line(block_line(Vec::new(), self.width), out);
                } else {
                    let l = code_line(lang, &line, &mut self.code, self.width);
                    self.emit_line(l, out);
                }
            } else {
                self.code_line.push(c);
            }
            return;
        }
        if let Some(head) = &mut self.head {
            if c == '\n' {
                let h = self.head.take().unwrap_or_default();
                self.start_line(&h, true, out);
                if self.fence.is_none() {
                    self.end_line(out);
                }
                return;
            }
            head.push(c);
            // The kind shows once the first word's done (or it's long).
            let decided = head.trim_start().contains(' ') || head.chars().count() > 8;
            let fence_start = head.trim_start().starts_with("```") || head.trim_start().starts_with("~~~");
            let rule_start = head.trim_start().chars().all(|c| matches!(c, '-' | '*' | '_' | ' '));
            if decided && !fence_start && !rule_start {
                let h = self.head.take().unwrap_or_default();
                self.start_line(&h, false, out);
            }
            return;
        }
        if c == '\n' {
            self.end_line(out);
            return;
        }
        self.text_char(c, out);
    }

    /// A line's start, now its kind can be told; `whole`: it's the whole
    /// line.
    fn start_line(&mut self, head: &str, whole: bool, out: &mut Vec<Piece>) {
        let (block, at) = block_of(head);
        let (block, at) = match block {
            // Not known to be a rule or fence until the line's done.
            Block::Rule | Block::Fence(_) if !whole => (Block::Para, 0),
            b => (b, at),
        };
        self.inline = Inline::default();
        match &block {
            Block::Fence(tag) => {
                self.flush_word(out);
                if self.col > 0 {
                    self.newline(out);
                }
                self.fence = Some(lang_for_tag(tag));
                self.code = CodeState::default();
                let label = if tag.is_empty() { String::new() } else { format!(" {tag}") };
                self.emit_line(block_line(vec![span(label, Ink::Comment, true)], self.width), out);
                self.block = Block::Para;
                return;
            }
            Block::Rule => {
                let w = self.width.saturating_sub(self.col);
                out.push(Piece::Text("─".repeat(w), Ink::Dim, false));
                self.col += w;
                self.block = Block::Para;
                return;
            }
            Block::Bullet => self.prefix(span("  ∙ ", Ink::Bullet, false), span("    ", Ink::Text, false), out),
            Block::Numbered(n) => {
                let pad = n.chars().count() + 3;
                self.prefix(span(format!("  {n} "), Ink::Bullet, false), span(" ".repeat(pad), Ink::Text, false), out);
            }
            Block::Quote => self.prefix(span(" │ ", Ink::Dim, false), span(" │ ", Ink::Dim, false), out),
            _ => self.indent = Vec::new(),
        }
        self.block = block;
        for c in head.chars().skip(at) {
            self.text_char(c, out);
        }
    }

    fn prefix(&mut self, first: Span, next: Span, out: &mut Vec<Piece>) {
        self.col += first.text.chars().count();
        out.push(Piece::Text(first.text, first.ink, first.shade));
        self.indent = vec![next];
    }

    fn base(&self) -> Ink {
        if matches!(self.block, Block::Heading(_)) { Ink::Heading } else { Ink::Text }
    }

    fn text_char(&mut self, c: char, out: &mut Vec<Piece>) {
        let base = self.base();
        let st = &mut self.inline;
        // Stars waiting: decided now (what's ahead can't be seen, so a
        // closing pair is assumed); those left over are just stars.
        if st.stars > 0 && c != '*' {
            let n = st.settle(Some(c), |_| true);
            let (ink, sh) = st.ink(base);
            self.word.extend(std::iter::repeat_n(('*', ink, sh), n));
        }
        let st = &mut self.inline;
        if c == '`' {
            st.code = !st.code;
            st.before = Some(c);
            return;
        }
        if !st.code && c == '*' {
            if st.stars == 0 {
                st.before = self.word.last().map(|w| w.0).or(Some(' '));
            }
            st.stars += 1;
            return;
        }
        let (ink, sh) = self.inline.ink(base);
        if c == ' ' && !sh {
            self.flush_word(out);
            if self.col > width_of(&self.indent) && self.col < self.width {
                out.push(Piece::Text(" ".into(), ink, sh));
                self.col += 1;
            }
        } else {
            self.word.push((c, ink, sh));
        }
    }

    fn flush_word(&mut self, out: &mut Vec<Piece>) {
        if self.word.is_empty() {
            return;
        }
        let indent = width_of(&self.indent);
        if self.col > indent && self.col + self.word.len() > self.width {
            self.newline(out);
        }
        let word = std::mem::take(&mut self.word);
        let mut line: Line = Vec::new();
        for (c, ink, sh) in word {
            if self.col >= self.width {
                self.write(&mut line, out);
                self.newline(out);
            }
            push(&mut line, &c.to_string(), ink, sh);
            self.col += 1;
        }
        self.write(&mut line, out);
    }

    fn write(&mut self, line: &mut Line, out: &mut Vec<Piece>) {
        for s in line.drain(..) {
            out.push(Piece::Text(s.text, s.ink, s.shade));
        }
    }

    /// A new line, continuing the line's indent (a list item's, a quote's).
    fn newline(&mut self, out: &mut Vec<Piece>) {
        out.push(Piece::Newline);
        self.col = 0;
        for s in self.indent.clone() {
            self.col += s.text.chars().count();
            out.push(Piece::Text(s.text, s.ink, s.shade));
        }
    }

    /// A whole line out (a code line), starting on a fresh line.
    fn emit_line(&mut self, line: Line, out: &mut Vec<Piece>) {
        if self.col > 0 {
            out.push(Piece::Newline);
        }
        for s in line {
            out.push(Piece::Text(s.text, s.ink, s.shade));
        }
        out.push(Piece::Newline);
        self.col = 0;
    }

    fn end_line(&mut self, out: &mut Vec<Piece>) {
        let n = self.inline.settle(None, |_| false);
        let (ink, sh) = self.inline.ink(self.base());
        self.word.extend(std::iter::repeat_n(('*', ink, sh), n));
        self.flush_word(out);
        if self.col > 0 || self.head.is_none() {
            out.push(Piece::Newline);
        }
        self.col = 0;
        self.indent = Vec::new();
        self.block = Block::Para;
        self.head = Some(String::new());
    }
}

/// A message's lines with its fenced code blocks drawn (shaded, colored
/// when they name a language); everything else as it was. None if it has
/// no fenced block.
pub fn message_code_blocks(lines: &[String], width: usize) -> Option<Vec<Result<String, Line>>> {
    if !lines.iter().any(|l| matches!(block_of(l).0, Block::Fence(_))) {
        return None;
    }
    let mut out = Vec::new();
    let mut fence: Option<Option<Lang>> = None;
    let mut code = CodeState::default();
    for l in lines {
        let is_fence = matches!(block_of(l).0, Block::Fence(_));
        match fence {
            Some(_) if is_fence => {
                fence = None;
                out.push(Err(block_line(Vec::new(), width)));
            }
            Some(lang) => out.push(Err(code_line(lang, l, &mut code, width))),
            None if is_fence => {
                let Block::Fence(tag) = block_of(l).0 else { continue };
                fence = Some(lang_for_tag(&tag));
                code = CodeState::default();
                let label = if tag.is_empty() { String::new() } else { format!(" {tag}") };
                out.push(Err(block_line(vec![span(label, Ink::Comment, true)], width)));
            }
            None => out.push(Ok(l.clone())),
        }
    }
    Some(out)
}

/// A line as ANSI in a caller's `look`; `text` for plain text outside
/// code (the caller's own color, as SGR; empty: the Markdown theme's).
pub fn to_ansi(line: &Line, look: &Look, text: &str) -> String {
    let shade = look.shade.as_str();
    let mut s = String::new();
    for sp in line {
        let fore = look.fore(sp.ink, sp.shade);
        let code = match (sp.ink, sp.shade) {
            (Ink::Text, false) if !text.is_empty() => format!("\x1b[0;{text}m"),
            (_, true) if !shade.is_empty() => format!("\x1b[0;{fore};{shade}m"),
            (Ink::Text, true) if !text.is_empty() => format!("\x1b[0;{text}m"),
            _ => format!("\x1b[0;{fore}m"),
        };
        s.push_str(&code);
        s.push_str(&sp.text);
    }
    s.push_str("\x1b[0m");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inks(l: &Line) -> Vec<(String, Ink)> {
        l.iter().map(|s| (s.text.clone(), s.ink)).collect()
    }

    #[test]
    fn c_is_colored() {
        let mut st = CodeState::default();
        let l = highlight(Lang::C, "int main(void) { return 0; } // hi", &mut st, false);
        let i = inks(&l);
        assert_eq!(i[0], ("int".into(), Ink::Type));
        assert!(i.contains(&("return".into(), Ink::Keyword)));
        assert!(i.contains(&("0".into(), Ink::Num)));
        assert_eq!(i.last().unwrap(), &("// hi".into(), Ink::Comment));
        assert_eq!(inks(&highlight(Lang::C, "#include <stdio.h>", &mut st, false)), vec![("#include <stdio.h>".into(), Ink::Pre)]);
        // A block comment carries over lines.
        highlight(Lang::C, "x = 1; /* start", &mut st, false);
        assert_eq!(inks(&highlight(Lang::C, "still */ y", &mut st, false))[0], ("still */".into(), Ink::Comment));
        let s = highlight(Lang::C, r#"puts("a \" b");"#, &mut st, false);
        assert!(inks(&s).contains(&(r#""a \" b""#.into(), Ink::Str)));
    }

    #[test]
    fn pascal_ignores_case_and_has_brace_comments() {
        let mut st = CodeState::default();
        let i = inks(&highlight(Lang::Pascal, "BEGIN WriteLn('hi'); { note } END;", &mut st, false));
        assert_eq!(i[0], ("BEGIN".into(), Ink::Keyword));
        assert!(i.contains(&("'hi'".into(), Ink::Str)));
        assert!(i.contains(&("{ note }".into(), Ink::Comment)));
        assert_eq!(lang_for_name("CONFIG.PAS"), Some(Lang::Pascal));
        assert_eq!(lang_for_name("a.c"), Some(Lang::C));
        assert_eq!(lang_for_name("notes.txt"), None);
    }

    #[test]
    fn markdown_blocks_and_inline() {
        let md = "# Title\n\nSome **bold** and *it* and `code`.\n- one\n> said\n```c\nint x;\n```\n---\n[site](http://x.y)";
        let lines = render_markdown(md, 40);
        let text: Vec<String> = lines.iter().map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>()).collect();
        assert_eq!(text[0], "Title");
        assert!(text[1].starts_with("═════"));
        assert_eq!(text[3].trim_end(), "Some bold and it and code.");
        assert!(lines[3].iter().any(|s| s.text == "bold" && s.ink == Ink::Bold));
        assert!(lines[3].iter().any(|s| s.text == "code" && s.shade));
        assert_eq!(text[4], "  ∙ one");
        assert_eq!(text[5], " │ said");
        assert_eq!(text[6].trim_end(), "   c", "two in, then the shade");
        assert!(lines[7].iter().any(|s| s.text == "int" && s.ink == Ink::Type && s.shade));
        assert_eq!(text[7].chars().count(), 38, "code is shaded to the margin");
        assert!(!lines[7][0].shade && lines[7][1].shade);
        assert!(text[9].starts_with("───"));
        assert_eq!(text[10], "site <http://x.y>");
        // Stars that aren't emphasis stay, and style nothing.
        let flat = |s: &str| render_markdown(s, 60)[0].iter().map(|s| s.text.as_str()).collect::<String>();
        for s in ["2 * 3 = 6", "2*3 = 6", "*.ZIP files", "*nix rocks", "a ** b c", "end*", "a `tick"] {
            assert_eq!(flat(s), s);
            assert!(render_markdown(s, 60)[0].iter().all(|sp| matches!(sp.ink, Ink::Text) && !sp.shade), "{s}");
        }
        assert!(render_markdown("an *it* and **bo**", 60)[0].iter().any(|s| s.text == "it" && s.ink == Ink::Italic));
    }

    #[cfg(feature = "ai")]
    #[test]
    fn long_lines_wrap_with_their_indent() {
        let lines = render_markdown(&format!("- {}", "word ".repeat(20)), 30);
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|l| width_of(l) <= 30));
        assert!(lines[1].iter().map(|s| s.text.as_str()).collect::<String>().starts_with("    word"));
    }

    #[cfg(feature = "ai")]
    fn streamed(chunks: &[&str]) -> String {
        let mut m = MdStream::new(40, 0);
        let mut out = Vec::new();
        for c in chunks {
            out.extend(m.feed(c));
        }
        out.extend(m.finish());
        out.iter()
            .map(|p| match p {
                Piece::Text(t, _, _) => t.clone(),
                Piece::Newline => "\n".into(),
            })
            .collect()
    }

    #[cfg(feature = "ai")]
    #[test]
    fn a_stream_comes_out_as_the_whole_would() {
        let s = streamed(&["## He", "ading\nSome **bo", "ld** te", "xt.\n- it", "em one\n```", "c\nint x", ";\n```\nAfter."]);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "Heading");
        assert_eq!(lines[1], "Some bold text.");
        assert_eq!(lines[2], "  ∙ item one");
        assert_eq!(lines[3].trim_end(), "   c");
        assert_eq!(lines[4].trim_end(), "   int x;");
        assert_eq!(lines[4].chars().count(), 38);
        assert_eq!(lines.last().unwrap().trim(), "After.");
        // A reply that ends on its closing fence, no newline after it.
        let s = streamed(&["Here:\n```py\nprint(1)\n```"]);
        assert!(!s.contains("```"), "{s:?}");
        assert_eq!(streamed(&["2*3 and *.ZIP"]).trim_end(), "2*3 and *.ZIP");
        let long = streamed(&[&"word ".repeat(30)]);
        assert!(long.lines().all(|l| l.chars().count() <= 40), "{long:?}");
    }

    #[test]
    fn messages_keep_all_but_their_code() {
        let lines: Vec<String> = ["hi", "```rust", "fn x() {}", "```", "bye"].map(String::from).into();
        let out = message_code_blocks(&lines, 20).unwrap();
        assert_eq!(out[0], Ok("hi".to_string()));
        assert!(matches!(&out[2], Err(l) if l.iter().any(|s| s.text == "fn" && s.ink == Ink::Keyword)));
        assert_eq!(out[4], Ok("bye".to_string()));
        assert!(message_code_blocks(&["plain".to_string()], 20).is_none());
        assert_eq!(code_back("blue").1, "48;2;20;40;84");
        assert_eq!(code_back("").0, "theme");
        let classic = Look { code: theme("classic"), md: theme("classic"), shade: "48;2;1;2;3".into(), rgb: true };
        let a = to_ansi(&vec![span("x", Ink::Keyword, true)], &classic, "");
        assert_eq!(a, "\x1b[0;1;37;48;2;1;2;3mx\x1b[0m");
        let nord = Look { code: theme(""), md: theme("nord"), shade: "48;2;46;52;64".into(), rgb: true };
        assert_eq!(theme("").name, "nord");
        assert_eq!(to_ansi(&vec![span("x", Ink::Keyword, true)], &nord, ""), "\x1b[0;38;2;129;161;193;48;2;46;52;64mx\x1b[0m");
        // Without 24-bit color, the classic 16 whatever the theme.
        let old = Look { rgb: false, ..nord };
        assert!(to_ansi(&vec![span("x", Ink::Keyword, true)], &old, "").starts_with("\x1b[0;1;37;"));
    }
}
