//! New: URLs in text, for clickable links (OSC 8,
//! `ESC ] 8 ; ; URL ESC \ text ESC ] 8 ; ; ESC \`). Only what terminals
//! open safely is a link: web, FTP, gopher, telnet, SSH and mail addresses,
//! printable ASCII only (no escape, bell or C1 byte can reach the
//! terminal inside the sequence), and no longer than [`MAX_URL`].

/// The longest URL that's made a link (longer ones show as text).
pub const MAX_URL: usize = 2048;

/// The schemes that are links, lowercase; all but `mailto:` need `//`.
const SCHEMES: &[&[u8]] = &[b"https://", b"http://", b"ftps://", b"ftp://", b"gopher://", b"telnet://", b"ssh://", b"mailto:"];

/// Ends a link.
pub const CLOSE: &[u8] = b"\x1b]8;;\x1b\\";

/// Starts a link to `url` (one [`find`] gave).
pub fn open(url: &[u8]) -> Vec<u8> {
    open_id(url, None)
}

/// [`open`], with an `id` that makes the pieces of a URL cut over rows
/// one link.
pub fn open_id(url: &[u8], id: Option<u64>) -> Vec<u8> {
    let mut v = b"\x1b]8;".to_vec();
    if let Some(id) = id {
        v.extend_from_slice(format!("id=fl{id:x}").as_bytes());
    }
    v.push(b';');
    v.extend(url.iter().filter(|&&c| url_byte(c)));
    v.extend_from_slice(b"\x1b\\");
    v
}

/// A URL in one row: its columns there, the whole URL, and its id if
/// it's cut over rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub url: Vec<u8>,
    pub id: Option<u64>,
}

impl Span {
    pub fn open(&self) -> Vec<u8> {
        open_id(&self.url, self.id)
    }
}

/// Rows looked through either side for the rest of a cut URL.
const ROWS_AROUND: usize = 32;

/// The URLs in `rows[k]` (each row's visible text), a URL that runs to
/// the end of a full row (`width` long) going on at the start of the next,
/// as a long one cut to fit the screen does.
pub fn in_rows(rows: &[&[u8]], k: usize, width: usize) -> Vec<Span> {
    let Some(row) = rows.get(k) else { return Vec::new() };
    // A row that goes on into the next.
    let joins = |j: usize| rows[j].len() == width && rows[j].last().is_some_and(|&c| url_byte(c)) && rows.get(j + 1).and_then(|r| r.first()).is_some_and(|&c| url_byte(c));
    let mut first = k;
    while first > 0 && k - first < ROWS_AROUND && joins(first - 1) {
        first -= 1;
    }
    let mut last = k;
    while last + 1 < rows.len() && last - k < ROWS_AROUND && joins(last) {
        last += 1;
    }
    if first == last {
        return find(row).into_iter().map(|(a, b)| Span { start: a, end: b, url: row[a..b].to_vec(), id: None }).collect();
    }
    let joined: Vec<u8> = rows[first..=last].concat();
    let at: usize = rows[first..k].iter().map(|r| r.len()).sum();
    let (lo, hi) = (at, at + row.len());
    find(&joined)
        .into_iter()
        .filter(|&(a, b)| a < hi && b > lo)
        .map(|(a, b)| {
            let cut = a < lo || b > hi;
            // The same for each piece: the URL and where it starts.
            let id = cut.then(|| fnv(&joined[a..b]) ^ (first as u64 * width as u64 + a as u64));
            Span { start: a.max(lo) - lo, end: b.min(hi) - lo, url: joined[a..b].to_vec(), id }
        })
        .collect()
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &c| (h ^ c as u64).wrapping_mul(0x100_0000_01b3))
}

/// A byte a URL may hold: printable ASCII but the space and what text
/// puts round a URL (`"<>` and the like) or a pipe code starts with.
fn url_byte(c: u8) -> bool {
    (0x21..=0x7E).contains(&c) && !matches!(c, b'"' | b'\'' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'}' | b'|')
}

/// The URLs in `b` (CP437 or UTF-8 bytes), as byte ranges.
pub fn find(b: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        // Quickly past anything that can't start one.
        if !matches!(b[i] | 0x20, b'h' | b'f' | b'g' | b't' | b's' | b'm') || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let Some(scheme) = SCHEMES.iter().find(|s| b.len() - i >= s.len() && b[i..i + s.len()].eq_ignore_ascii_case(s)) else {
            i += 1;
            continue;
        };
        let start = i + scheme.len();
        let mut end = start;
        while end < b.len() && url_byte(b[end]) {
            end += 1;
        }
        // Too long to be one (and not looked through again).
        if end - i > MAX_URL {
            i = end;
            continue;
        }
        // Not the punctuation after it, nor a closing bracket it didn't open.
        let count = |c: u8| b[start..end].iter().filter(|&&x| x == c).count();
        let mut brackets = [count(b'('), count(b')'), count(b'['), count(b']')];
        while let Some(&last) = b[start..end].last() {
            let unopened = (last == b')' && brackets[0] < brackets[1]) || (last == b']' && brackets[2] < brackets[3]);
            if !matches!(last, b'.' | b',' | b';' | b':' | b'!' | b'?' | b'*') && !unopened {
                break;
            }
            match last {
                b')' => brackets[1] -= 1,
                b']' => brackets[3] -= 1,
                _ => {}
            }
            end -= 1;
        }
        let rest = &b[start..end];
        let ok = if *scheme == b"mailto:" {
            rest.iter().position(|&c| c == b'@').is_some_and(|at| at > 0 && rest.get(at + 1).is_some_and(u8::is_ascii_alphanumeric))
        } else {
            rest.first().is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'[')
        };
        if ok && end - i <= MAX_URL {
            out.push((i, end));
        }
        i = end.max(i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(s: &str) -> Vec<&str> {
        find(s.as_bytes()).into_iter().map(|(a, b)| &s[a..b]).collect()
    }

    #[test]
    fn finds_urls() {
        assert_eq!(urls("see https://example.com/a?b=c&d=e#f."), ["https://example.com/a?b=c&d=e#f"]);
        assert_eq!(urls("(http://x.org/wiki/Foo_(bar)) and HTTP://Y.COM, ok"), ["http://x.org/wiki/Foo_(bar)", "HTTP://Y.COM"]);
        assert_eq!(urls("[gopher://g.net/1/] telnet://bbs.example.com:23 ssh://me@host"), ["gopher://g.net/1/", "telnet://bbs.example.com:23", "ssh://me@host"]);
        assert_eq!(urls("mail mailto:sysop@example.com! or mailto:nobody"), ["mailto:sysop@example.com"]);
        assert_eq!(urls("ftp://files.example.com/pub/ ftps://f.example.com"), ["ftp://files.example.com/pub/", "ftps://f.example.com"]);
        assert_eq!(urls("<https://a.com/x>\"https://b.com\"'https://c.com'"), ["https://a.com/x", "https://b.com", "https://c.com"]);
    }

    #[test]
    fn leaves_the_rest() {
        // Not schemes it opens, nor half a word, nor a scheme alone.
        assert!(urls("file:FILES/README.TXT javascript://x data:text/html,x").is_empty());
        assert!(urls("xhttp://example.com http:// https://. https://-x").is_empty());
        assert!(urls("http:/example.com mailto:@x").is_empty());
        // No control bytes, escapes, C1 or CP437's high half in one.
        assert_eq!(urls("http://a.com\x1b]8;;x\x07"), ["http://a.com"]);
        assert_eq!(find(b"http://a.com\x9cb"), [(0, 12)]);
        assert_eq!(urls("http://a.com|04red"), ["http://a.com"]);
        // Too long for a link.
        let long = format!("https://a.com/{}", "x".repeat(MAX_URL));
        assert!(urls(&long).is_empty());
    }

    #[test]
    fn cut_urls_stay_whole() {
        let url = format!("https://example.com/{}", "a".repeat(20));
        let (one, two) = url.split_at(20);
        let r0 = format!("{} {one}", "x".repeat(9));
        let rows: Vec<&[u8]> = vec![r0.as_bytes(), two.as_bytes(), b"then http://b.com"];
        let s0 = in_rows(&rows, 0, 30);
        let s1 = in_rows(&rows, 1, 30);
        assert_eq!((s0.len(), s1.len()), (1, 1));
        assert_eq!((s0[0].start, s0[0].end, s1[0].start, s1[0].end), (10, 30, 0, two.len()));
        assert!(s0[0].url == url.as_bytes() && s1[0].url == url.as_bytes());
        assert!(s0[0].id.is_some() && s0[0].id == s1[0].id);
        // A row that isn't full doesn't go on; one URL alone has no id.
        assert_eq!(in_rows(&rows, 2, 30), [Span { start: 5, end: 17, url: b"http://b.com".to_vec(), id: None }]);
        let rows: Vec<&[u8]> = vec![b"see http://a.com", b"next"];
        assert_eq!(in_rows(&rows, 0, 30)[0].url, b"http://a.com");
        assert!(in_rows(&rows, 1, 30).is_empty() && in_rows(&rows, 5, 30).is_empty());
    }

    #[test]
    fn open_is_clean() {
        assert_eq!(open_id(b"http://a.com", Some(255)), b"\x1b]8;id=flff;http://a.com\x1b\\");
        assert_eq!(open(b"https://a.com/?q=1;2"), b"\x1b]8;;https://a.com/?q=1;2\x1b\\");
        assert_eq!(open(b"https://a\x1b\x07\x9c.com"), b"\x1b]8;;https://a.com\x1b\\");
    }
}
