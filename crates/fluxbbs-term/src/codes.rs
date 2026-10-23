//! Iniquity's inline display codes.
//!
//! | prefix | meaning                                   | source            |
//! |--------|-------------------------------------------|-------------------|
//! | `\|XX` | control code: colour, cursor, pause...    | `mciProcessControlCode` |
//! | `%XX`  | MCI code: substitute a value (user name...) | `mciProcessMCICode` |
//! | `@NN`  | move to column NN                         | `posHeader`       |
//! | `&NN`  | move to row NN                            | `rowHeader`       |
//! | `{NN`  | truncate the next MCI values to NN chars  | `limHeader`       |
//! | `}NN`  | pad/truncate the next MCI values to NN    | `padHeader`       |
//! | `\|**` | after the string, return to this spot     | `oStr`            |
//! | `~XX`  | screen-specific position marker (files)   | `extHeader`       |
//! | `^NN`  | secondary position marker (files)         | `ex2Header`       |

/// A parsed `|XX` control code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Fore(u8),
    Back(u8),
    SaveColor,
    LoadColor,
    Up(u8),
    Down(u8),
    Left(u8),
    Right(u8),
    ClearScreen,
    ClearEol,
    Delay(u16),
    Home,
    SaveCursor,
    RestoreCursor,
    Newline,
    PromptKey,
    ReadKey,
    Pause,
    /// User colour scheme slot `|U0`..`|U9`.
    Theme(u8),
    /// `|SA`: the STOCK screen (a shared header) drawn here. New.
    StockArt,
    /// 2.20's `|PO`, `|PE`, `|PD` (pausing off for a screen, on, off).
    /// Screens don't pause here, so they do nothing, but don't show.
    PauseCtl,
}

impl Control {
    pub fn parse(code: [u8; 2]) -> Option<Control> {
        use Control::*;
        let [a, b] = code.map(|c| c.to_ascii_uppercase());
        let digit = |c: u8| c.is_ascii_digit().then(|| c - b'0');
        Some(match (a, b) {
            (b'0'..=b'2', _) => {
                let n = (a - b'0') * 10 + digit(b)?;
                match n {
                    0..=15 => Fore(n),
                    16..=23 => Back(n - 16),
                    _ => return None,
                }
            }
            (b'A', b'S') => SaveColor,
            (b'A', b'L') => LoadColor,
            (b'B', b'0'..=b'7') => Back(b - b'0'),
            (b'C', b'B') => Down(25),
            (b'C', b'D') => Down(1),
            (b'C', b'E') => Right(80),
            (b'C', b'H') => Left(80),
            (b'C', b'L') => Left(1),
            (b'C', b'R') => Right(1),
            (b'C', b'S') => ClearScreen,
            (b'C', b'T') => Up(25),
            (b'C', b'U') => Up(1),
            (b'C', b'Z') => ClearEol,
            (b'D', b'E') => Delay(750),
            (b'D', b'H') => Delay(500),
            (b'D', b'M') => Delay(100),
            (b'D', b'S') => Delay(1000),
            (b'H', b'C') => Home,
            (b'L', b'C') => RestoreCursor,
            (b'L', b'F') => Newline,
            (b'P', b'A') => PromptKey,
            (b'P', b'K') => ReadKey,
            (b'P', b'S') => Pause,
            (b'P', b'O' | b'E' | b'D') => PauseCtl,
            (b'S', b'A') => StockArt,
            (b'S', b'C') => SaveCursor,
            (b'U', b'0'..=b'9') => Theme(b - b'0'),
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// Literal CP437 bytes, including any ANSI escape sequences.
    Text(Vec<u8>),
    Ctrl(Control),
    /// `%XX`, as written. Unknown codes should be written back literally.
    Mci([u8; 2]),
    PosX(u8),
    PosY(u8),
    Limit(u8),
    Pad(u8),
    SavePos,
    /// `~XX` (file mode only).
    Ext([u8; 2]),
    /// `^NN` (file mode only).
    Ext2(u8),
    /// New, `%>` in a file: the rest of the line ends at the right edge.
    AlignRight,
}

/// Whether the text is a prompt string or a display file; files also get
/// `~XX`/`^NN` markers and stop at the DOS EOF byte (where SAUCE begins).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    String,
    File,
}

fn two_digits(a: u8, b: u8) -> Option<u8> {
    (a.is_ascii_digit() && b.is_ascii_digit()).then(|| (a - b'0') * 10 + (b - b'0'))
}

/// Split CP437 text into literal runs and codes. ANSI escape sequences are
/// kept intact inside `Text`, so their bytes are never read as codes.
pub fn tokenize(src: &[u8], source: Source) -> Vec<Token> {
    let mut out = Vec::new();
    let mut text = Vec::new();
    let mut i = 0;
    let file = source == Source::File;

    macro_rules! emit {
        ($tok:expr, $len:expr) => {{
            if !text.is_empty() {
                out.push(Token::Text(std::mem::take(&mut text)));
            }
            out.push($tok);
            i += $len;
            continue;
        }};
    }

    while i < src.len() {
        let c = src[i];
        if file && c == 0x1A {
            break;
        }
        if c == 0x1B && src.get(i + 1) == Some(&b'[') {
            let end = src[i + 2..]
                .iter()
                .position(|b| (0x40..=0x7E).contains(b))
                .map_or(src.len(), |p| i + 2 + p + 1);
            text.extend_from_slice(&src[i..end]);
            i = end;
            continue;
        }
        if let (Some(&a), Some(&b)) = (src.get(i + 1), src.get(i + 2)) {
            let pair = [a.to_ascii_uppercase(), b.to_ascii_uppercase()];
            match c {
                b'|' if a == b'*' && b == b'*' => emit!(Token::SavePos, 3),
                // 2.0's `|ZA`: everything up to `|ZB` is literal (2.20: "this
                // should fix the display of garbled ascii art").
                b'|' if pair == *b"ZA" => {
                    let rest = &src[i + 3..];
                    let end = rest.windows(3).position(|w| w[0] == b'|' && w[1].eq_ignore_ascii_case(&b'Z') && w[2].eq_ignore_ascii_case(&b'B'));
                    let body = &rest[..end.unwrap_or(rest.len())];
                    let body = if file { body.split(|&b| b == 0x1A).next().unwrap_or_default() } else { body };
                    text.extend_from_slice(body);
                    if file && body.len() < end.unwrap_or(rest.len()) {
                        break;
                    }
                    i += 3 + end.map_or(rest.len(), |e| e + 3);
                    continue;
                }
                b'|' => {
                    if let Some(ctl) = Control::parse([a, b]) {
                        emit!(Token::Ctrl(ctl), 3)
                    }
                }
                b'%' if a.is_ascii_alphanumeric() || a == b'@' => emit!(Token::Mci([a, b]), 3),
                b'%' if file && a == b'>' => emit!(Token::AlignRight, 2),
                b'@' | b'&' | b'{' | b'}' => {
                    if let Some(n) = two_digits(a, b) {
                        let tok = match c {
                            b'@' => Token::PosX(n),
                            b'&' => Token::PosY(n),
                            b'{' => Token::Limit(n),
                            _ => Token::Pad(n),
                        };
                        emit!(tok, 3)
                    }
                }
                b'~' if file && (a.is_ascii_alphanumeric() || a == b'*') => {
                    emit!(Token::Ext(pair), 3)
                }
                b'^' if file => {
                    if let Some(n) = two_digits(a, b) {
                        emit!(Token::Ext2(n), 3)
                    }
                }
                _ => {}
            }
        }
        text.push(c);
        i += 1;
    }
    if !text.is_empty() {
        out.push(Token::Text(text));
    }
    out
}

impl Control {
    /// Colour codes, the only ones honoured in what callers typed.
    pub fn is_colour(self) -> bool {
        matches!(self, Control::Fore(_) | Control::Back(_) | Control::Theme(_) | Control::SaveColor | Control::LoadColor)
    }
}

/// Marks the start and end of caller-supplied text inside a prompt
/// (see [`lit`]); dropped by `cp437::encode`.
pub const LIT_OPEN: char = '\u{E000}';
pub const LIT_CLOSE: char = '\u{E001}';

/// Wrap text a caller typed so that, inside a prompt, only its colour
/// codes work: no cursor moves, clears, pauses, positions or MCI codes,
/// which could wreck other callers' screens.
pub fn lit(s: &str) -> String {
    // Marks inside (text that has them, or `lit` text again) would end
    // the protection early.
    format!("{LIT_OPEN}{}{LIT_CLOSE}", unlit(s))
}

/// Text a caller typed, made safe to keep in a screen file (the stock
/// oneliners append to ONELNMID): colour codes stay, and every other code
/// is wrapped in `|ZA`...`|ZB`, so it shows as typed and does nothing.
pub fn defuse(s: &str) -> String {
    // Without `lit`'s marks first: `encode` drops them, and a code split
    // by one would come back together.
    let c: Vec<char> = unlit(s).chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let (a, b) = (c.get(i + 1).copied(), c.get(i + 2).copied());
        let two = |x: Option<char>| x.is_some_and(|x| x.is_ascii_digit());
        let code = match (c[i], a, b) {
            ('|', Some(a), Some(b)) if a.is_ascii() && b.is_ascii() => {
                let pair = [a as u8, b as u8];
                let zb = pair.eq_ignore_ascii_case(b"ZB");
                !zb && (Control::parse(pair).is_some_and(|k| !k.is_colour()) || pair == *b"**" || pair.eq_ignore_ascii_case(b"ZA"))
            }
            ('%', Some(a), Some(_)) => a.is_ascii_alphanumeric() || a == '@',
            ('@' | '&' | '{' | '}' | '^', _, _) => two(a) && two(b),
            ('~', Some(a), Some(_)) => a.is_ascii_alphanumeric() || a == '*',
            _ => false,
        };
        if code {
            out.push_str("|ZA");
            out.extend(&c[i..i + 3]);
            out.push_str("|ZB");
            i += 3;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Drop [`lit`] markers (for text that's measured or stored, not shown).
pub fn unlit(s: &str) -> String {
    s.chars().filter(|&c| c != LIT_OPEN && c != LIT_CLOSE).collect()
}

/// [`tokenize`] for text a caller typed: colour codes (pipe codes, and
/// ANSI's `ESC[...m`, which a SysOp's string put in another's can hold),
/// and everything else literal. Other escape characters are dropped.
pub fn tokenize_user(src: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut text = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c == 0x1B && src.get(i + 1) == Some(&b'[') {
            let params = src[i + 2..].iter().take_while(|b| b.is_ascii_digit() || **b == b';').count();
            if src.get(i + 2 + params) == Some(&b'm') {
                text.extend_from_slice(&src[i..i + 3 + params]);
                i += 3 + params;
                continue;
            }
        }
        // `defuse`'s `|ZA`/`|ZB` (everything here is literal anyway).
        if c == b'|' && src.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case(&b'Z')) && src.get(i + 2).is_some_and(|b| b.eq_ignore_ascii_case(&b'A') || b.eq_ignore_ascii_case(&b'B')) {
            i += 3;
            continue;
        }
        if c == b'|'
            && let (Some(&a), Some(&b)) = (src.get(i + 1), src.get(i + 2))
            && let Some(ctl) = Control::parse([a, b]).filter(|c| c.is_colour())
        {
            if !text.is_empty() {
                out.push(Token::Text(std::mem::take(&mut text)));
            }
            out.push(Token::Ctrl(ctl));
            i += 3;
            continue;
        }
        // No control bytes at all (CP437's glyphs for them, `←` and `♪`,
        // arrive here as ESC and CR).
        if c >= 0x20 && c != 0x7F {
            text.push(c);
        }
        i += 1;
    }
    if !text.is_empty() {
        out.push(Token::Text(text));
    }
    out
}

/// [`tokenize`] a prompt that may hold [`lit`] sections.
pub fn tokenize_str(s: &str, source: Source) -> Vec<Token> {
    if !s.contains(LIT_OPEN) {
        return tokenize(&crate::cp437::encode(s), source);
    }
    let mut out = Vec::new();
    for (i, part) in s.split(LIT_OPEN).enumerate() {
        let (user, rest) = if i == 0 { ("", part) } else { part.split_once(LIT_CLOSE).unwrap_or((part, "")) };
        out.extend(tokenize_user(&crate::cp437::encode(user)));
        out.extend(tokenize(&crate::cp437::encode(rest), source));
    }
    out
}

/// `strSquish`: cut to `len` characters on screen.
pub fn squish(s: &str, len: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut shown = 0;
    let mut i = 0;
    while i < c.len() {
        // Colour codes and `lit`'s marks take no room on screen: kept
        // whole, not counted.
        let code = colour_at(&c, i);
        if code > 0 || c[i] == LIT_OPEN || c[i] == LIT_CLOSE {
            let n = code.max(1);
            out.extend(&c[i..i + n]);
            i += n;
            continue;
        }
        if shown == len {
            break;
        }
        out.push(c[i]);
        shown += 1;
        i += 1;
    }
    out
}

/// How many characters CP437 text takes on screen: ANSI escape sequences
/// take none.
pub fn visible_bytes(b: &[u8]) -> usize {
    let (mut n, mut i) = (0, 0);
    while i < b.len() {
        if b[i] == 0x1B && b.get(i + 1) == Some(&b'[') {
            i += 2 + b[i + 2..].iter().position(|c| (0x40..=0x7E).contains(c)).map_or(b.len(), |p| p + 1);
            continue;
        }
        n += 1;
        i += 1;
    }
    n
}

/// The length of a `|XX` colour code at `c[i]`, or 0.
fn colour_at(c: &[char], i: usize) -> usize {
    match (c.get(i), c.get(i + 1), c.get(i + 2)) {
        (Some('|'), Some(&a), Some(&b)) if a.is_ascii() && b.is_ascii() && Control::parse([a as u8, b as u8]).is_some_and(|k| k.is_colour()) => 3,
        _ => 0,
    }
}

/// Characters `s` takes on screen: colour codes and `lit`'s marks don't
/// count.
pub fn width(s: &str) -> usize {
    let c: Vec<char> = s.chars().collect();
    let mut n = 0;
    let mut i = 0;
    while i < c.len() {
        let code = colour_at(&c, i);
        if code > 0 {
            i += code;
            continue;
        }
        if c[i] != LIT_OPEN && c[i] != LIT_CLOSE {
            n += 1;
        }
        i += 1;
    }
    n
}

/// `Resize`: cut or space-pad to exactly `len` characters on screen.
pub fn resize(s: &str, len: usize) -> String {
    let mut r = squish(s, len);
    let n = width(&r);
    r.extend(std::iter::repeat_n(' ', len.saturating_sub(n)));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use Token::*;

    #[test]
    fn widths_skip_colour_codes() {
        assert_eq!(squish("|12Ano|04nymous", 5), "|12Ano|04ny");
        assert_eq!(resize("|12Hi", 4), "|12Hi  ");
        assert_eq!(width("|12Hi|CS"), 5);
    }

    #[test]
    fn caller_text_stays_text() {
        // No way out of `lit` from inside it.
        let t = tokenize_str(&format!("|U1{}", lit("Bob\u{E001}|CS|PA%UN")), Source::String);
        assert!(!t.iter().any(|x| matches!(x, Ctrl(Control::ClearScreen) | Ctrl(Control::PromptKey))));
        // Control bytes go, whatever the charset made of them.
        let t = tokenize_user(b"**\x18B00\r\n\x07ok");
        assert_eq!(t, vec![Text(b"**B00ok".to_vec())]);
    }

    #[test]
    fn controls() {
        assert_eq!(Control::parse(*b"07"), Some(Control::Fore(7)));
        assert_eq!(Control::parse(*b"15"), Some(Control::Fore(15)));
        assert_eq!(Control::parse(*b"17"), Some(Control::Back(1)));
        assert_eq!(Control::parse(*b"24"), None);
        assert_eq!(Control::parse(*b"cs"), Some(Control::ClearScreen));
        assert_eq!(Control::parse(*b"U4"), Some(Control::Theme(4)));
        assert_eq!(Control::parse(*b"SA"), Some(Control::StockArt));
        assert_eq!(Control::parse(*b"ZZ"), None);
    }

    #[test]
    fn mixed_string() {
        let t = tokenize(b"|15Hi %UN|07!", Source::String);
        assert_eq!(
            t,
            vec![
                Ctrl(Control::Fore(15)),
                Text(b"Hi ".to_vec()),
                Mci(*b"UN"),
                Ctrl(Control::Fore(7)),
                Text(b"!".to_vec()),
            ]
        );
    }

    #[test]
    fn non_codes_stay_literal() {
        assert_eq!(tokenize(b"a|zz 50% @x", Source::String), vec![Text(b"a|zz 50% @x".to_vec())]);
    }

    #[test]
    fn za_to_zb_is_literal() {
        assert_eq!(
            tokenize(b"|15a|ZA|CS %UN @10|zb|07b", Source::String),
            vec![Ctrl(Control::Fore(15)), Text(b"a|CS %UN @10".to_vec()), Ctrl(Control::Fore(7)), Text(b"b".to_vec())]
        );
        assert_eq!(tokenize(b"|ZA~XX|CS", Source::File), vec![Text(b"~XX|CS".to_vec())]);
        assert_eq!(tokenize(b"|ZAart\x1aSAUCE", Source::File), vec![Text(b"art".to_vec())]);
        assert_eq!(Control::parse(*b"PO"), Some(Control::PauseCtl));
    }

    #[test]
    fn defused_text_shows_as_typed() {
        let typed = "|12red |CS|PS@10 %UN ~XX ^03 &05 100% e@mail |ZB|ZA";
        let d = defuse(typed);
        let shown: Vec<Token> = tokenize(d.as_bytes(), Source::File);
        let text: Vec<u8> = shown.iter().flat_map(|t| if let Text(b) = t { b.clone() } else { Vec::new() }).collect();
        assert_eq!(shown[0], Ctrl(Control::Fore(12)));
        assert!(shown[1..].iter().all(|t| matches!(t, Text(_))), "{shown:?}");
        assert_eq!(String::from_utf8(text).unwrap(), &typed[3..]);
        assert_eq!(defuse("hello"), "hello");
        assert_eq!(tokenize_user(defuse("|04a|CS").as_bytes()), vec![Ctrl(Control::Fore(4)), Text(b"a|CS".to_vec())]);
    }

    #[test]
    fn caller_text_keeps_only_colours() {
        assert_eq!(
            tokenize_user(b"|04hi|CS|PS @10%UN\x1b[2J"),
            vec![Ctrl(Control::Fore(4)), Text(b"hi|CS|PS @10%UN[2J".to_vec())]
        );
        // ANSI colour, as a SysOp's string set inside another holds it, works.
        assert_eq!(
            tokenize_user(b"\x1b[38;2;170;170;170mno limit\x1b[0m\x1b[H"),
            vec![Text(b"\x1b[38;2;170;170;170mno limit\x1b[0m[H".to_vec())]
        );
        let t = tokenize_str(&format!("|U1From: {} |CS", lit("|CS|15x")), Source::String);
        assert_eq!(
            t,
            vec![Ctrl(Control::Theme(1)), Text(b"From: ".to_vec()), Text(b"|CS".to_vec()), Ctrl(Control::Fore(15)), Text(b"x".to_vec()), Text(b" ".to_vec()), Ctrl(Control::ClearScreen)]
        );
        assert_eq!(crate::cp437::encode(&lit("a")), b"a");
    }

    #[test]
    fn positions_and_limits() {
        assert_eq!(
            tokenize(b"@10&05{08}12|**", Source::String),
            vec![PosX(10), PosY(5), Limit(8), Pad(12), SavePos]
        );
    }

    #[test]
    fn ansi_sequences_are_opaque() {
        // "@" is the final byte of ICH; "1;31" must not be split.
        let t = tokenize(b"\x1b[12@34\x1b[1;31mX", Source::String);
        assert_eq!(t, vec![Text(b"\x1b[12@34\x1b[1;31mX".to_vec())]);
    }

    #[test]
    fn file_mode_extras_and_sauce() {
        let t = tokenize(b"~LN^03ok\x1aSAUCE00", Source::File);
        assert_eq!(t, vec![Ext(*b"LN"), Ext2(3), Text(b"ok".to_vec())]);
        assert_eq!(tokenize(b"~LN", Source::String), vec![Text(b"~LN".to_vec())]);
        assert_eq!(tokenize(b"a%>b", Source::File), vec![Text(b"a".to_vec()), AlignRight, Text(b"b".to_vec())]);
        assert_eq!(tokenize(b"50%>", Source::String), vec![Text(b"50%>".to_vec())]);
        assert_eq!(visible_bytes(b"\x1b[1;35mab\x1b[0mc"), 3);
    }

    #[test]
    fn squish_and_resize() {
        assert_eq!(squish("Iniquity", 3), "Ini");
        assert_eq!(resize("ab", 4), "ab  ");
        assert_eq!(resize("abcdef", 4), "abcd");
    }
}
