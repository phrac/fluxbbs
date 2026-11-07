//! Fuzz targets (`cargo test -p fluxbbs-term fuzz_ -- --ignored`; see
//! `fuzzkit`).

use crate::codes::{self, Source};
use crate::fuzzkit::{Target, chunks};
use crate::screen::{COLUMNS, Charset, Screen};
use crate::{Color, cp437};

const ANSI: &[&[u8]] = &[
    b"\x1b[0;1;34mHello\x1b[0m world\r\n",
    b"\x1b[2J\x1b[1;1H\x1b[40m\x1b[37m\xdb\xdb\xb2\xb1\xb0 \x1b[5;44m blink \x1b[25m\x1b[0m",
    b"\x1b[38;5;196mred\x1b[48;2;10;20;30m bg \x1b[0m\x1b[90mgrey\x1b[100m\x1b[0;102mx",
    b"\x1b[10;20H\x1b[s\x1b[3A\x1b[5B\x1b[7C\x1b[2D\x1b[u\x1b[K\x1b[0K\x1b[5G\x1b[9d\x1b[?25l\x1b[?33h",
    b"line one\r\nline two\ttab\x08\x08\x07\r\n\x1b[1;24r\x1b[24;1H\n\n\n",
    b"|09Blue|16|U3%UN @10&05{20}30|**|ZAraw|ZB~AB^12%>%[AMNESIA.TDF,0,80]Hi%]\x1a SAUCE",
];

const DICT: &[&[u8]] = &[b"\x1b[38;2;", b"\x1b[48;5;", b";2;", b";5;", b"m", b"H", b"A", b"B", b"C", b"D", b"J", b"K", b"r", b"s", b"u", b"G", b"d", b"f", b"|ZA", b"|ZB", b"|**", b"%[", b"%]", b"%>", b"|U", b"|CS", b"|PA", b"|SA", b"\x1a"];

fn check(s: &Screen) {
    assert!((1..=COLUMNS).contains(&s.where_x()), "x {} out of range", s.where_x());
    assert!((1..=s.rows()).contains(&s.where_y()), "y {} out of rows {}", s.where_y(), s.rows());
}

/// Bytes written in pieces, with the calls the board makes in between.
#[test]
#[ignore]
fn fuzz_screen() {
    Target::new("screen", ANSI).dict(DICT).run(|input, rng| {
        let charset = if rng.one_in(2) { Charset::Cp437 } else { Charset::Utf8 };
        let mut s = Screen::new(charset, rng.below(256) as u16);
        s.set_truecolor(rng.one_in(3));
        s.set_palette256(rng.one_in(3));
        s.set_plain(rng.one_in(6));
        let mut out = 0;
        for piece in chunks(input, rng) {
            s.write_bytes(piece);
            let n = rng.number() as u16;
            match rng.below(24) {
                // Screens are at most 200 rows (`telnet::clamp_size`).
                0 => s.set_rows(rng.below(256) as u16),
                1 => s.goto(n, rng.number() as u16),
                2 => s.move_up(n),
                3 => s.move_down(n),
                4 => s.move_left(n),
                5 => s.move_right(n),
                6 => s.pos_x(n),
                7 => s.pos_y(n),
                8 => s.backspace(n),
                9 => s.set_scroll_region(n, rng.number() as u16),
                10 => s.reset_scroll_region(),
                11 => s.save_cursor(),
                12 => s.restore_cursor(),
                13 => s.clear_eol(),
                14 => s.clear_screen(),
                15 => s.set_color(Color { fore: rng.next() as u8, back: rng.next() as u8, blink: rng.one_in(2) }),
                16 => s.write_str(&String::from_utf8_lossy(piece)),
                17 => s.write_text(&String::from_utf8_lossy(piece)),
                18 => s.set_charset(if rng.one_in(2) { Charset::Cp437 } else { Charset::Utf8 }),
                19 => s.resync(),
                20 => {
                    let _ = s.shown(n, rng.number() as u16);
                }
                21 => s.newline(),
                _ => {}
            }
            check(&s);
            if rng.one_in(4) {
                out += s.take_output().len();
            }
        }
        out += s.take_output().len();
        assert!(out <= 64 * input.len() + (1 << 20), "{out} bytes out for {} in", input.len());
    });
}

#[test]
#[ignore]
fn fuzz_codes() {
    Target::new("codes", ANSI).dict(DICT).run(|input, rng| {
        let _ = codes::tokenize(input, Source::File);
        let _ = codes::tokenize(input, Source::String);
        let _ = codes::tokenize_user(input);
        let _ = codes::visible_bytes(input);
        let s = String::from_utf8_lossy(input);
        let mut s = s.into_owned();
        if rng.one_in(3) {
            s = codes::lit(&s);
        }
        if rng.one_in(3) {
            let mut at = rng.below(s.len() + 1);
            while !s.is_char_boundary(at) {
                at -= 1;
            }
            s.insert(at, codes::LIT_OPEN);
        }
        let _ = codes::tokenize_str(&s, Source::File);
        let _ = codes::tokenize_str(&s, Source::String);
        let n = rng.below(200);
        let sq = codes::squish(&s, n);
        assert!(codes::width(&sq) <= n);
        let r = codes::resize(&s, n);
        assert_eq!(codes::width(&r), n, "resize to {n}");
        let d = codes::defuse(&s);
        // What's defused does nothing but colour.
        for source in [Source::String, Source::File] {
            for t in codes::tokenize(&cp437::encode(&d), source) {
                match t {
                    codes::Token::Text(_) => {}
                    codes::Token::Ctrl(c) => assert!(c.is_colour(), "{c:?} survived defuse"),
                    t => panic!("{t:?} survived defuse"),
                }
            }
        }
        let _ = codes::unlit(&s);
        let params: String = String::from_utf8_lossy(input).chars().filter(|c| c.is_ascii_digit() || *c == ';').take(200).collect();
        let _ = codes::sgr_filter(&params);
        let _ = codes::sgr_ok(&params);
        let dec = cp437::decode(input);
        assert_eq!(cp437::encode(&dec).len(), input.len());
        let _ = cp437::dos_art(input);
    });
}

#[test]
#[ignore]
fn fuzz_color() {
    Target::new("color", &[b"0;1;31;44", b"38;5;200;48;2;1;2;3", b"5;25;90;100"]).run(|input, _| {
        let params: Vec<u16> = input.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]) % 120).collect();
        let mut c = Color::DEFAULT;
        c.apply_sgr(&params);
        let mut d = Color { fore: input.first().copied().unwrap_or(0), back: input.get(1).copied().unwrap_or(0), blink: input.len() % 2 == 0 };
        let _ = c.transition(d);
        d.apply_sgr(&params);
        let _ = d.transition(c);
    });
}

const URLS: &[&[u8]] = &[
    b"see https://example.com/a?b=c&d=e#f. and (http://x.org/w/Foo_(bar))",
    b"mailto:sysop@example.com, telnet://bbs.example.com:23 ssh://me@host gopher://g.net/1/ ftp://f.net",
    b"|09http://a.com|04/b\x1b[1;31mhttps://c.com\x1b]8;;x\x1b\\\x9c\x07HTTPS://D.COM\xb0",
];

const URL_DICT: &[&[u8]] = &[b"http://", b"https://", b"mailto:", b"telnet://", b"ssh://", b"ftp://", b"gopher://", b"@", b"(", b")", b"[", b"]", b".", b"?", b"#", b"\x1b]8;;", b"\x1b\\", b"\x07", b"\x9c", b"\x1b["];

/// URLs found and linked: whatever the text, each link is clean (its URL
/// printable ASCII and closed), and what's on screen is as without links.
#[test]
#[ignore]
fn fuzz_links() {
    Target::new("links", URLS).dict(URL_DICT).run(|input, rng| {
        let mut last = 0;
        for (a, b) in crate::hyperlink::find(input) {
            assert!(last <= a && a < b && b <= input.len(), "{a}..{b} after {last}");
            last = b;
            let url = &input[a..b];
            assert!(url.len() <= crate::hyperlink::MAX_URL);
            assert!(url.iter().all(|&c| (0x21..=0x7E).contains(&c) && c != b'|'), "{url:?}");
            assert_eq!(crate::hyperlink::open(url), [b"\x1b]8;;", url, b"\x1b\\"].concat());
        }
        // Cut into rows: each piece of a URL is in it, with the whole URL.
        let width = 1 + rng.below(90);
        let rows: Vec<&[u8]> = input.split(|&c| c == b'\n').collect();
        for k in 0..rows.len() {
            for s in crate::hyperlink::in_rows(&rows, k, width) {
                assert!(s.start < s.end && s.end <= rows[k].len(), "{s:?} in row {k}");
                let piece = &rows[k][s.start..s.end];
                assert!(s.url.windows(piece.len()).any(|w| w == piece), "{s:?}");
                assert!(s.url.len() <= crate::hyperlink::MAX_URL && s.url.iter().all(|&c| (0x21..=0x7E).contains(&c)));
                assert_eq!(s.id.is_some(), s.url != piece);
            }
        }
        let charset = if rng.one_in(2) { Charset::Cp437 } else { Charset::Utf8 };
        let rows = 1 + rng.below(60) as u16;
        let (mut a, mut b) = (Screen::new(charset, rows), Screen::new(charset, rows));
        let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
        for piece in chunks(input, rng) {
            a.write_linked(piece, true);
            b.write_linked(piece, false);
            assert_eq!((a.where_x(), a.where_y(), a.text_lines()), (b.where_x(), b.where_y(), b.text_lines()));
            out_a.extend(a.take_output());
            out_b.extend(b.take_output());
        }
        for y in 1..=rows {
            for x in 1..=COLUMNS {
                assert_eq!(a.shown(x, y), b.shown(x, y), "cell {x},{y}");
            }
        }
        // Take the links out and it's what went without them (unless the
        // text held an `ESC ] 8` of its own, which is never a link here).
        if !input.windows(2).any(|w| w == b"\x1b]") {
            let mut stripped = Vec::new();
            let mut i = 0;
            while i < out_a.len() {
                if out_a[i..].starts_with(b"\x1b]8;;") {
                    let end = out_a[i..].windows(2).position(|w| w == b"\x1b\\").expect("an open link ends");
                    i += end + 2;
                    continue;
                }
                stripped.push(out_a[i]);
                i += 1;
            }
            assert_eq!(stripped, out_b);
        }
    });
}
