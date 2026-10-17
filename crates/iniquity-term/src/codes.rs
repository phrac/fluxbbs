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
                b'|' => {
                    if let Some(ctl) = Control::parse([a, b]) {
                        emit!(Token::Ctrl(ctl), 3)
                    }
                }
                b'%' if a.is_ascii_alphanumeric() || a == b'@' => emit!(Token::Mci([a, b]), 3),
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

/// `strSquish`: cut to `len` characters.
pub fn squish(s: &str, len: usize) -> String {
    s.chars().take(len).collect()
}

/// `Resize`: cut or space-pad to exactly `len` characters.
pub fn resize(s: &str, len: usize) -> String {
    let mut r = squish(s, len);
    let n = r.chars().count();
    r.extend(std::iter::repeat_n(' ', len - n));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use Token::*;

    #[test]
    fn controls() {
        assert_eq!(Control::parse(*b"07"), Some(Control::Fore(7)));
        assert_eq!(Control::parse(*b"15"), Some(Control::Fore(15)));
        assert_eq!(Control::parse(*b"17"), Some(Control::Back(1)));
        assert_eq!(Control::parse(*b"24"), None);
        assert_eq!(Control::parse(*b"cs"), Some(Control::ClearScreen));
        assert_eq!(Control::parse(*b"U4"), Some(Control::Theme(4)));
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
    }

    #[test]
    fn squish_and_resize() {
        assert_eq!(squish("Iniquity", 3), "Ini");
        assert_eq!(resize("ab", 4), "ab  ");
        assert_eq!(resize("abcdef", 4), "abcd");
    }
}
