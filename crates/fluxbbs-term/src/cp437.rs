//! Code page 437, the character set every Iniquity screen is drawn in.

/// CP437 byte -> Unicode, using the glyph forms for 0x01-0x1F and 0x7F
/// (what a DOS text-mode screen shows for those bytes).
pub const TO_UNICODE: [char; 256] = [
    '\u{0000}', '\u{263A}', '\u{263B}', '\u{2665}', '\u{2666}', '\u{2663}', '\u{2660}', '\u{2022}',
    '\u{25D8}', '\u{25CB}', '\u{25D9}', '\u{2642}', '\u{2640}', '\u{266A}', '\u{266B}', '\u{263C}',
    '\u{25BA}', '\u{25C4}', '\u{2195}', '\u{203C}', '\u{00B6}', '\u{00A7}', '\u{25AC}', '\u{21A8}',
    '\u{2191}', '\u{2193}', '\u{2192}', '\u{2190}', '\u{221F}', '\u{2194}', '\u{25B2}', '\u{25BC}',
    '\u{0020}', '\u{0021}', '\u{0022}', '\u{0023}', '\u{0024}', '\u{0025}', '\u{0026}', '\u{0027}',
    '\u{0028}', '\u{0029}', '\u{002A}', '\u{002B}', '\u{002C}', '\u{002D}', '\u{002E}', '\u{002F}',
    '\u{0030}', '\u{0031}', '\u{0032}', '\u{0033}', '\u{0034}', '\u{0035}', '\u{0036}', '\u{0037}',
    '\u{0038}', '\u{0039}', '\u{003A}', '\u{003B}', '\u{003C}', '\u{003D}', '\u{003E}', '\u{003F}',
    '\u{0040}', '\u{0041}', '\u{0042}', '\u{0043}', '\u{0044}', '\u{0045}', '\u{0046}', '\u{0047}',
    '\u{0048}', '\u{0049}', '\u{004A}', '\u{004B}', '\u{004C}', '\u{004D}', '\u{004E}', '\u{004F}',
    '\u{0050}', '\u{0051}', '\u{0052}', '\u{0053}', '\u{0054}', '\u{0055}', '\u{0056}', '\u{0057}',
    '\u{0058}', '\u{0059}', '\u{005A}', '\u{005B}', '\u{005C}', '\u{005D}', '\u{005E}', '\u{005F}',
    '\u{0060}', '\u{0061}', '\u{0062}', '\u{0063}', '\u{0064}', '\u{0065}', '\u{0066}', '\u{0067}',
    '\u{0068}', '\u{0069}', '\u{006A}', '\u{006B}', '\u{006C}', '\u{006D}', '\u{006E}', '\u{006F}',
    '\u{0070}', '\u{0071}', '\u{0072}', '\u{0073}', '\u{0074}', '\u{0075}', '\u{0076}', '\u{0077}',
    '\u{0078}', '\u{0079}', '\u{007A}', '\u{007B}', '\u{007C}', '\u{007D}', '\u{007E}', '\u{2302}',
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
    '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
    '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}',
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
    '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
    '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
    '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];

/// Unicode -> CP437 byte. ASCII maps to itself; anything without a CP437
/// form becomes `?`.
pub fn from_char(c: char) -> u8 {
    if c.is_ascii() {
        return c as u8;
    }
    TO_UNICODE
        .iter()
        .skip(0x80)
        .position(|&u| u == c)
        .map(|i| (i + 0x80) as u8)
        .or_else(|| TO_UNICODE[1..0x20].iter().position(|&u| u == c).map(|i| (i + 1) as u8))
        .unwrap_or(b'?')
}

/// Encode a Rust string as CP437 bytes (dropping `codes::lit` markers).
pub fn encode(s: &str) -> Vec<u8> {
    s.chars().filter(|&c| c != crate::codes::LIT_OPEN && c != crate::codes::LIT_CLOSE).map(from_char).collect()
}

/// Decode CP437 bytes to a Rust string (glyph forms for control bytes).
pub fn decode(b: &[u8]) -> String {
    b.iter().map(|&c| TO_UNICODE[c as usize]).collect()
}

/// Art made with modern tools, as a BBS screen: UTF-8 text becomes CP437
/// and bare LF line ends become CRLF. CP437 art (never valid UTF-8 once it
/// has block characters) and DOS line ends are left alone. Anything from
/// the DOS EOF byte on (SAUCE) is kept as it is.
pub fn dos_art(b: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let end = b.iter().position(|&c| c == 0x1a).unwrap_or(b.len());
    let (text, rest) = b.split_at(end);
    let utf8 = std::str::from_utf8(text).ok().filter(|t| !t.is_ascii());
    let bare_lf = !text.contains(&b'\r') && text.contains(&b'\n');
    if utf8.is_none() && !bare_lf {
        return std::borrow::Cow::Borrowed(b);
    }
    let text = utf8.map_or_else(|| text.to_vec(), encode);
    let mut out = Vec::with_capacity(text.len() + rest.len() + 64);
    for &c in &text {
        if c == b'\n' && bare_lf {
            out.push(b'\r');
        }
        out.push(c);
    }
    out.extend_from_slice(rest);
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_art_becomes_dos_art() {
        let art = "\x1b[38;2;1;2;3m\u{2584}\u{2588}\n\u{2580}\n".as_bytes();
        assert_eq!(&*dos_art(art), b"\x1b[38;2;1;2;3m\xdc\xdb\r\n\xdf\r\n");
        let dos = b"\xdc\xdc\r\n\x1aSAUCE00";
        assert!(matches!(dos_art(dos), std::borrow::Cow::Borrowed(_)), "CP437 art is left alone");
        assert_eq!(&*dos_art(b"plain\nlines\n\x1a\n"), b"plain\r\nlines\r\n\x1a\n", "SAUCE untouched");
    }

    #[test]
    fn round_trips_high_half() {
        for b in 0x80..=0xFFu8 {
            assert_eq!(from_char(TO_UNICODE[b as usize]), b);
        }
    }

    #[test]
    fn box_drawing() {
        assert_eq!(decode(&[0xC9, 0xCD, 0xBB]), "╔═╗");
        assert_eq!(encode("░▒▓█"), vec![0xB0, 0xB1, 0xB2, 0xDB]);
        assert_eq!(encode("日"), b"?");
    }
}
