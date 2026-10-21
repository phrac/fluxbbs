//! New: FidoNet technology (FTN), for echomail networks such as fsxNet.
//! Addresses (`21:1/100.0@fsxnet`), Type 2+ packets (FTS-0001 with
//! FSC-0039/0048's header) and the text of a packed message: the `AREA:`
//! line, `^A` kludges (MSGID, REPLY, PID, TZUTC, CHRS, INTL, FMPT, TOPT),
//! the body with its tear and origin lines, then SEEN-BY and `^APATH`
//! (FTS-0004, FTS-0009, FTS-4000). The tosser is `ftntoss.rs`, the
//! mailer `binkp.rs`.

use std::fmt;

use chrono::{Datelike, NaiveDateTime, Timelike};

/// An FTN address: zone:net/node.point.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Addr {
    pub zone: u16,
    pub net: u16,
    pub node: u16,
    pub point: u16,
}

impl Addr {
    /// `21:1/100`, `21:1/100.5`, with or without `@domain`.
    pub fn parse(s: &str) -> Option<Addr> {
        let s = s.trim().split('@').next()?;
        let (zone, rest) = s.split_once(':')?;
        let (net, rest) = rest.split_once('/')?;
        let (node, point) = match rest.split_once('.') {
            Some((n, p)) => (n, p),
            None => (rest, "0"),
        };
        Some(Addr { zone: zone.parse().ok()?, net: net.parse().ok()?, node: node.parse().ok()?, point: point.parse().ok()? })
    }

    /// `net/node`, as SEEN-BY and PATH write it.
    pub fn net_node(&self) -> String {
        format!("{}/{}", self.net, self.node)
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}/{}", self.zone, self.net, self.node)?;
        if self.point != 0 {
            write!(f, ".{}", self.point)?;
        }
        Ok(())
    }
}

/// Message attributes (FTS-0001) used here.
pub const ATTR_PRIVATE: u16 = 0x0001;
pub const ATTR_LOCAL: u16 = 0x0100;

/// One message in a packet, as it travels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackedMsg {
    pub orig_node: u16,
    pub dest_node: u16,
    pub orig_net: u16,
    pub dest_net: u16,
    pub attr: u16,
    pub cost: u16,
    /// `01 Jan 26  21:30:00`.
    pub datetime: String,
    pub to: String,
    pub from: String,
    pub subject: String,
    /// Lines ended by CR, in the packet's character set.
    pub text: Vec<u8>,
}

/// A packet: who it's from and to, its password, and its messages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Packet {
    pub orig: Addr,
    pub dest: Addr,
    pub password: String,
    pub created: NaiveDateTime,
    pub msgs: Vec<PackedMsg>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

/// A NUL-ended string starting at `*i`, at most `max` bytes, moving `*i`
/// past it.
fn cstr(b: &[u8], i: &mut usize, max: usize) -> Result<Vec<u8>, String> {
    let start = *i;
    let end = b[start..].iter().position(|&c| c == 0).map(|p| start + p).ok_or("unterminated string")?;
    if end - start > max {
        return Err(format!("string longer than {max}"));
    }
    *i = end + 1;
    Ok(b[start..end].to_vec())
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// FTS-0001's date: `01 Jan 26  21:30:00`.
pub fn fts_date(t: NaiveDateTime) -> String {
    format!("{:02} {} {:02}  {:02}:{:02}:{:02}", t.day(), MONTHS[t.month0() as usize], t.year() % 100, t.hour(), t.minute(), t.second())
}

/// Read an FTS-0001 date (and the SEAdog form, `Mon  1 Jan 26 21:30`).
pub fn parse_fts_date(s: &str) -> Option<NaiveDateTime> {
    let w: Vec<&str> = s.split_whitespace().collect();
    let (d, m, y, t) = match w.as_slice() {
        [d, m, y, t] => (*d, *m, *y, *t),
        [_, d, m, y, t] => (*d, *m, *y, *t),
        _ => return None,
    };
    let month = MONTHS.iter().position(|x| x.eq_ignore_ascii_case(m))? as u32 + 1;
    let yy: i32 = y.parse().ok()?;
    let year = if yy < 80 { 2000 + yy } else if yy < 100 { 1900 + yy } else { yy };
    let mut hms = t.split(':').map(|p| p.parse::<u32>().ok());
    let (h, mi, sec) = (hms.next()??, hms.next()??, hms.next().flatten().unwrap_or(0));
    chrono::NaiveDate::from_ymd_opt(year, month, d.parse().ok()?)?.and_hms_opt(h, mi, sec)
}

impl Packet {
    /// Read a packet (Type 2+, or plain Type 2 without the zones).
    pub fn parse(b: &[u8]) -> Result<Packet, String> {
        if b.len() < 58 {
            return Err("too short for a packet header".into());
        }
        if u16_at(b, 18) != 2 {
            return Err(format!("not a type 2 packet ({})", u16_at(b, 18)));
        }
        let cap = u16_at(b, 44);
        let cap_check = u16::from_be_bytes([b[40], b[41]]);
        let plus = cap == cap_check && cap & 1 == 1;
        let mut orig = Addr { zone: u16_at(b, 34), net: u16_at(b, 20), node: u16_at(b, 0), point: 0 };
        let mut dest = Addr { zone: u16_at(b, 36), net: u16_at(b, 22), node: u16_at(b, 2), point: 0 };
        if plus {
            orig.zone = u16_at(b, 46);
            dest.zone = u16_at(b, 48);
            orig.point = u16_at(b, 50);
            dest.point = u16_at(b, 52);
            // A point's packet has net 0xFFFF and the real net in auxNet.
            if orig.point != 0 && orig.net == 0xFFFF {
                orig.net = u16_at(b, 38);
            }
        }
        let password = String::from_utf8_lossy(&b[26..34]).trim_end_matches('\0').to_string();
        let created = chrono::NaiveDate::from_ymd_opt(u16_at(b, 4) as i32, u16_at(b, 6) as u32 + 1, u16_at(b, 8) as u32)
            .and_then(|d| d.and_hms_opt(u16_at(b, 10) as u32, u16_at(b, 12) as u32, u16_at(b, 14) as u32))
            .unwrap_or_default();
        let mut msgs = Vec::new();
        let mut i = 58;
        loop {
            if i + 2 > b.len() {
                break; // some packers leave off the final 0000
            }
            let kind = u16_at(b, i);
            if kind == 0 {
                break;
            }
            if kind != 2 {
                return Err(format!("bad message type {kind} at {i}"));
            }
            if i + 34 > b.len() {
                return Err("message header cut short".into());
            }
            let h = |o: usize| u16_at(b, i + o);
            let mut m = PackedMsg { orig_node: h(2), dest_node: h(4), orig_net: h(6), dest_net: h(8), attr: h(10), cost: h(12), ..Default::default() };
            i += 14;
            // The date is a fixed 20 bytes, NUL-padded.
            let end = (i + 20).min(b.len());
            m.datetime = String::from_utf8_lossy(&b[i..end]).trim_end_matches('\0').to_string();
            i = end;
            m.to = iniquity_term::cp437::decode(&cstr(b, &mut i, 36)?);
            m.from = iniquity_term::cp437::decode(&cstr(b, &mut i, 36)?);
            m.subject = iniquity_term::cp437::decode(&cstr(b, &mut i, 72)?);
            m.text = cstr(b, &mut i, usize::MAX)?;
            msgs.push(m);
        }
        Ok(Packet { orig, dest, password, created, msgs })
    }

    /// Write it as Type 2+.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = vec![0u8; 58];
        let put = |b: &mut Vec<u8>, i: usize, v: u16| b[i..i + 2].copy_from_slice(&v.to_le_bytes());
        let t = self.created;
        put(&mut b, 0, self.orig.node);
        put(&mut b, 2, self.dest.node);
        put(&mut b, 4, t.year() as u16);
        put(&mut b, 6, t.month0() as u16);
        put(&mut b, 8, t.day() as u16);
        put(&mut b, 10, t.hour() as u16);
        put(&mut b, 12, t.minute() as u16);
        put(&mut b, 14, t.second() as u16);
        put(&mut b, 18, 2);
        // A point writes net 0xFFFF and its net in auxNet.
        put(&mut b, 20, if self.orig.point != 0 { 0xFFFF } else { self.orig.net });
        put(&mut b, 22, self.dest.net);
        b[24] = 0xFE; // product code: none assigned
        let pw = self.password.as_bytes();
        b[26..26 + pw.len().min(8)].copy_from_slice(&pw[..pw.len().min(8)]);
        put(&mut b, 34, self.orig.zone);
        put(&mut b, 36, self.dest.zone);
        put(&mut b, 38, if self.orig.point != 0 { self.orig.net } else { 0 });
        b[40..42].copy_from_slice(&1u16.to_be_bytes());
        put(&mut b, 44, 1);
        put(&mut b, 46, self.orig.zone);
        put(&mut b, 48, self.dest.zone);
        put(&mut b, 50, self.orig.point);
        put(&mut b, 52, self.dest.point);
        for m in &self.msgs {
            for v in [2, m.orig_node, m.dest_node, m.orig_net, m.dest_net, m.attr, m.cost] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            let mut date = m.datetime.as_bytes().to_vec();
            date.resize(20, 0);
            date[19] = 0;
            b.extend_from_slice(&date);
            for (s, max) in [(&m.to, 35), (&m.from, 35), (&m.subject, 71)] {
                let mut e = iniquity_term::cp437::encode(s);
                e.truncate(max);
                b.extend_from_slice(&e);
                b.push(0);
            }
            b.extend(m.text.iter().filter(|&&c| c != 0));
            b.push(0);
        }
        b.extend_from_slice(&[0, 0]);
        b
    }
}

/// A message's text taken apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parts {
    /// The echo tag (`AREA:`), if it's echomail.
    pub area: Option<String>,
    /// `^A` kludges, as (name, value): ("MSGID", "21:1/100 1234abcd").
    pub kludges: Vec<(String, String)>,
    /// What a reader sees, tear and origin lines included.
    pub body: Vec<String>,
    pub seen_by: Vec<String>,
    pub path: Vec<String>,
}

impl Parts {
    pub fn kludge(&self, name: &str) -> Option<&str> {
        self.kludges.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The address in an origin line, `(21:1/100)` at its end.
    pub fn origin_addr(&self) -> Option<Addr> {
        let line = self.body.iter().rev().find(|l| l.starts_with(" * Origin:"))?;
        let open = line.rfind('(')?;
        Addr::parse(line[open + 1..].trim_end().trim_end_matches(')'))
    }
}

/// Take a message's text apart. `CHRS: UTF-8` text is read as UTF-8,
/// anything else as CP437.
pub fn split_text(text: &[u8]) -> Parts {
    let utf8 = text.windows(10).any(|w| w.eq_ignore_ascii_case(b"CHRS: UTF-")) || text.windows(10).any(|w| w.eq_ignore_ascii_case(b"CHARSET: U"));
    let mut p = Parts::default();
    let lines: Vec<&[u8]> = text.split(|&c| c == b'\r').collect();
    for (n, raw) in lines.iter().enumerate() {
        // A LF after the CR, and soft CRs (0x8D) in CP437 text, aren't text.
        let raw: Vec<u8> = raw.iter().copied().filter(|&c| c != b'\n' && (utf8 || c != 0x8D)).collect();
        // The kludge mark is looked for before decoding: CP437 makes 0x01 a
        // face.
        let (kludge, raw) = match raw.split_first() {
            Some((1, rest)) => (true, rest.to_vec()),
            _ => (false, raw),
        };
        let line = if utf8 { String::from_utf8_lossy(&raw).into_owned() } else { iniquity_term::cp437::decode(&raw) };
        if n == 0 && let Some(tag) = line.strip_prefix("AREA:") {
            p.area = Some(tag.trim().to_string());
            continue;
        }
        if kludge {
            // The name runs to a colon or a space, whichever is first (INTL,
            // FMPT and TOPT have no colon, and INTL's addresses have them).
            let k = line.as_str();
            let cut = k.find([':', ' ']).unwrap_or(k.len());
            let (name, value) = (k[..cut].trim(), k[cut..].trim_start_matches(':').trim());
            if name.eq_ignore_ascii_case("PATH") {
                p.path.extend(value.split_whitespace().map(String::from));
            } else {
                p.kludges.push((name.to_string(), value.to_string()));
            }
            continue;
        }
        if let Some(s) = line.strip_prefix("SEEN-BY:") {
            p.seen_by.extend(s.split_whitespace().map(String::from));
            continue;
        }
        p.body.push(line);
    }
    while p.body.last().is_some_and(|l| l.trim().is_empty()) {
        p.body.pop();
    }
    p
}

/// Put a message's text together: `AREA:` (echomail), kludges, body,
/// SEEN-BY (wrapped) and PATH, CR-ended, in CP437.
pub fn join_text(p: &Parts) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let line = |out: &mut Vec<u8>, kludge: bool, s: &str| {
        if kludge {
            out.push(1);
        }
        out.extend(iniquity_term::cp437::encode(s));
        out.push(b'\r');
    };
    if let Some(a) = &p.area {
        line(&mut out, false, &format!("AREA:{a}"));
    }
    for (k, v) in &p.kludges {
        // INTL, FMPT and TOPT have no colon.
        let sep = if matches!(k.as_str(), "INTL" | "FMPT" | "TOPT") { " " } else { ": " };
        line(&mut out, true, &format!("{k}{sep}{v}"));
    }
    for l in &p.body {
        line(&mut out, false, l);
    }
    for (name, list, kludge) in [("SEEN-BY:", &p.seen_by, false), ("PATH:", &p.path, true)] {
        let mut acc = String::new();
        for item in list {
            if !acc.is_empty() && acc.len() + item.len() > 70 {
                line(&mut out, kludge, &format!("{name} {acc}"));
                acc.clear();
            }
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(item);
        }
        if !acc.is_empty() {
            line(&mut out, kludge, &format!("{name} {acc}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        let a = Addr::parse("21:1/100@fsxnet").unwrap();
        assert_eq!((a.zone, a.net, a.node, a.point), (21, 1, 100, 0));
        assert_eq!(a.to_string(), "21:1/100");
        assert_eq!(Addr::parse("21:1/100.5").unwrap().to_string(), "21:1/100.5");
        assert_eq!(a.net_node(), "1/100");
        assert!(Addr::parse("1/100").is_none());
    }

    #[test]
    fn dates() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap().and_hms_opt(21, 3, 9).unwrap();
        assert_eq!(fts_date(t), "05 Jan 26  21:03:09");
        assert_eq!(parse_fts_date("05 Jan 26  21:03:09"), Some(t));
        assert_eq!(parse_fts_date("Mon  5 Jan 26 21:03"), Some(t.with_second(0).unwrap()));
        assert_eq!(parse_fts_date("05 Jan 99  21:03:09").unwrap().year(), 1999);
    }

    #[test]
    fn packets_round_trip() {
        let p = Packet {
            orig: Addr::parse("21:1/999").unwrap(),
            dest: Addr::parse("21:1/100").unwrap(),
            password: "SECRET".into(),
            created: chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(1, 2, 3).unwrap(),
            msgs: vec![PackedMsg {
                orig_node: 999,
                dest_node: 100,
                orig_net: 1,
                dest_net: 1,
                datetime: "21 Oct 26  01:02:03".into(),
                to: "All".into(),
                from: "phrac".into(),
                subject: "hello".into(),
                text: b"AREA:FSX_GEN\r\x01MSGID: 21:1/999 0000abcd\rhi there\r".to_vec(),
                ..Default::default()
            }],
        };
        let b = p.to_bytes();
        assert_eq!(&b[b.len() - 2..], &[0, 0]);
        assert_eq!(Packet::parse(&b).unwrap(), p);
        // A point: net 0xFFFF in the header, the real net from auxNet.
        let mut q = p.clone();
        q.orig.point = 7;
        assert_eq!(Packet::parse(&q.to_bytes()).unwrap().orig, q.orig);
    }

    #[test]
    fn text_apart_and_together() {
        let raw = b"AREA:FSX_GEN\r\x01MSGID: 21:1/100 12345678\r\x01REPLY: 21:1/5 0000beef\r\x01INTL 21:1/2 21:1/3\rHello\r\r--- tool\r * Origin: a board (21:1/100)\rSEEN-BY: 1/100 1/999\r\x01PATH: 1/100\r";
        let p = split_text(raw);
        assert_eq!(p.area.as_deref(), Some("FSX_GEN"));
        assert_eq!(p.kludge("msgid"), Some("21:1/100 12345678"));
        assert_eq!(p.kludge("INTL"), Some("21:1/2 21:1/3"));
        assert_eq!(p.body, ["Hello", "", "--- tool", " * Origin: a board (21:1/100)"]);
        assert_eq!(p.seen_by, ["1/100", "1/999"]);
        assert_eq!(p.path, ["1/100"]);
        assert_eq!(p.origin_addr(), Addr::parse("21:1/100"));
        assert_eq!(split_text(&join_text(&p)), p);
        // CP437 by default, UTF-8 when it says so.
        assert_eq!(split_text(b"caf\x82\r").body, ["caf\u{e9}"]);
        assert_eq!(split_text("\x01CHRS: UTF-8 4\rcaf\u{e9}\r".as_bytes()).body, ["caf\u{e9}"]);
    }
}
