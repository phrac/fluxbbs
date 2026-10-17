//! Small data files: STATS.DAT (`tStatRec`), HISTORY.DAT (`tHistoryRec`),
//! CALLERS.DAT (`tCallRec`) and BBSLIST.DAT (`tBBSlistRec`).

use crate::pascal::{Reader, check};

pub const STATS_SIZE: usize = 1055;
pub const HISTORY_SIZE: usize = 25;
pub const CALL_SIZE: usize = 109;
pub const BBS_SIZE: usize = 243;

#[derive(Debug, Clone)]
pub struct LegacyStats {
    pub first_day: String,
    pub calls: u16,
    pub posts: u16,
    pub email: u16,
    pub uploads: i32,
    pub downloads: i32,
    pub upload_kb: i32,
    pub download_kb: i32,
}

impl LegacyStats {
    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tStatRec", buf, STATS_SIZE)?;
        let mut r = Reader::new(buf);
        Ok(Self {
            first_day: r.string(8),
            calls: r.u16(),
            posts: r.u16(),
            email: r.u16(),
            uploads: r.i32(),
            downloads: r.i32(),
            upload_kb: r.i32(),
            download_kb: r.i32(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LegacyHistory {
    pub date: String,
    pub calls: u16,
    pub new_users: u16,
    pub posts: u16,
    pub email: u16,
    pub uploads: u16,
    pub downloads: u16,
    pub upload_kb: u16,
    pub download_kb: u16,
}

impl LegacyHistory {
    pub fn parse_file(buf: &[u8]) -> Vec<Self> {
        buf.as_chunks::<HISTORY_SIZE>()
            .0
            .iter()
            .map(|c| {
                let mut r = Reader::new(c);
                Self {
                    date: r.string(8),
                    calls: r.u16(),
                    new_users: r.u16(),
                    posts: r.u16(),
                    email: r.u16(),
                    uploads: r.u16(),
                    downloads: r.u16(),
                    upload_kb: r.u16(),
                    download_kb: r.u16(),
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct LegacyCall {
    pub call_num: i32,
    pub user_name: String,
    pub user_num: u16,
    pub location: String,
    pub baud: i32,
    pub date: String,
    pub time: String,
    pub new_user: bool,
    pub area_code: String,
}

impl LegacyCall {
    pub fn parse_file(buf: &[u8]) -> Vec<Self> {
        buf.as_chunks::<CALL_SIZE>()
            .0
            .iter()
            .map(|c| {
                let mut r = Reader::new(c);
                Self {
                    call_num: r.i32(),
                    user_name: r.string(36),
                    user_num: r.u16(),
                    location: r.string(40),
                    baud: r.i32(),
                    date: r.string(8),
                    time: r.string(7),
                    new_user: r.bool(),
                    area_code: r.string(3),
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct LegacyBbs {
    pub name: String,
    pub sysop: String,
    pub phone: String,
    pub baud: i32,
    pub software: String,
    pub storage: String,
    pub info: String,
    pub who_added: String,
}

impl LegacyBbs {
    pub fn parse_file(buf: &[u8]) -> Vec<Self> {
        buf.as_chunks::<BBS_SIZE>()
            .0
            .iter()
            .map(|c| {
                let mut r = Reader::new(c);
                Self {
                    name: r.string(40),
                    sysop: r.string(36),
                    phone: r.string(13),
                    baud: r.i32(),
                    software: r.string(12),
                    storage: r.string(20),
                    info: r.string(75),
                    who_added: r.string(36),
                }
            })
            .collect()
    }
}

/// INFOFORM.DAT: `tInfoformRec` (78 bytes) per form.
pub const INFOFORM_SIZE: usize = 78;

#[derive(Debug, Clone)]
pub struct LegacyInfoform {
    pub desc: String,
    /// The form's text file (with its extension).
    pub filename: String,
    pub mandatory: bool,
    pub nuv: bool,
    pub acs: String,
}

impl LegacyInfoform {
    pub fn parse_file(buf: &[u8]) -> Vec<Self> {
        buf.as_chunks::<INFOFORM_SIZE>()
            .0
            .iter()
            .map(|c| {
                let mut r = Reader::new(c);
                Self { desc: r.string(40), filename: r.string(13), mandatory: r.bool(), nuv: r.bool(), acs: r.string(20) }
            })
            .collect()
    }
}

/// A user's answers in an `INFOnnn` file: a 6-byte index per user number
/// (`tInfoIdxRec`: position, count) at the start, the answers as Pascal
/// strings at that position. None if they haven't filled it in.
pub fn infoform_answers(buf: &[u8], user: u16) -> Option<Vec<String>> {
    let at = (user as usize).checked_sub(1)? * 6;
    let idx = buf.get(at..at + 6)?;
    let pos = u32::from_le_bytes(idx[..4].try_into().ok()?) as usize;
    let count = u16::from_le_bytes(idx[4..].try_into().ok()?);
    if count == 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut p = pos;
    for _ in 0..count {
        let len = *buf.get(p)? as usize;
        out.push(iniquity_term::cp437::decode(buf.get(p + 1..p + 1 + len)?));
        p += 1 + len;
    }
    Some(out)
}

/// A NUV.DAT record: who voted on whom, their initials and comment.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyVote {
    pub voter: u16,
    pub candidate: u16,
    pub initials: String,
    pub comment: String,
}

pub fn parse_votes(buf: &[u8]) -> Vec<LegacyVote> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 7 <= buf.len() {
        let voter = u16::from_le_bytes([buf[p], buf[p + 1]]);
        let candidate = u16::from_le_bytes([buf[p + 2], buf[p + 3]]);
        let initials = iniquity_term::cp437::decode(&buf[p + 4..p + 6]);
        let len = buf[p + 6] as usize;
        let Some(c) = buf.get(p + 7..p + 7 + len) else { break };
        out.push(LegacyVote { voter, candidate, initials, comment: iniquity_term::cp437::decode(c) });
        p += 7 + len;
    }
    out
}

/// A text library (`.TFL`, made by INIQLIB): screens packed into one file
/// so a library could replace the text directory on DOS.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyTextLib {
    pub desc: String,
    pub author: String,
    /// (filename, contents)
    pub files: Vec<(String, Vec<u8>)>,
}

impl LegacyTextLib {
    /// Header: count (byte), description and author (`String[36]` each),
    /// then per file 13 bytes of name (length byte and 12 characters),
    /// position (u32) and size (u16).
    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        crate::pascal::check("tTextLib", buf.get(..75).unwrap_or(buf), 75)?;
        let mut r = Reader::new(buf);
        let n = r.u8() as usize;
        let desc = r.string(36);
        let author = r.string(36);
        let index = buf.get(75..75 + n * 19).ok_or(crate::Error::Size { what: "TFL index", expected: 75 + n * 19, got: buf.len() })?;
        let mut files = Vec::new();
        for e in index.chunks(19) {
            let len = (e[0] as usize).min(12);
            let name = iniquity_term::cp437::decode(&e[1..1 + len]);
            let pos = u32::from_le_bytes([e[13], e[14], e[15], e[16]]) as usize;
            let size = u16::from_le_bytes([e[17], e[18]]) as usize;
            let data = buf.get(pos..pos + size).ok_or(crate::Error::Size { what: "TFL file", expected: pos + size, got: buf.len() })?;
            files.push((name, data.to_vec()));
        }
        Ok(Self { desc, author, files })
    }
}

/// TEXTLIB.DAT: the installed libraries' names (`String[8]`), in order.
pub fn text_lib_names(buf: &[u8]) -> Vec<String> {
    buf.as_chunks::<9>().0.iter().map(|c| iniquity_term::cp437::decode(&c[1..1 + (c[0] as usize).min(8)])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infoform_answers_by_user() {
        // Index for users 1 and 2, then user 2's two answers at 12.
        let mut buf = vec![0u8; 12];
        buf[6..10].copy_from_slice(&12u32.to_le_bytes());
        buf[10..12].copy_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(b"\x03Bob\x00");
        assert_eq!(infoform_answers(&buf, 1), None);
        assert_eq!(infoform_answers(&buf, 2), Some(vec!["Bob".into(), String::new()]));
        assert_eq!(infoform_answers(&buf, 9), None);
    }

    #[test]
    fn text_library() {
        let pstr = |s: &str, max: usize| {
            let mut v = vec![s.len() as u8];
            v.extend(s.bytes());
            v.resize(max + 1, 0);
            v
        };
        let mut buf = vec![2u8];
        buf.extend(pstr("Dark theme", 36));
        buf.extend(pstr("phrac", 36));
        let data_at = 75 + 2 * 19;
        for (name, pos, size) in [("MAIN.ANS", data_at, 3), ("LOGOFF.ANS", data_at + 3, 2)] {
            buf.extend(&pstr(name, 12)[..13]);
            buf.extend((pos as u32).to_le_bytes());
            buf.extend((size as u16).to_le_bytes());
        }
        buf.extend(b"abcde");
        let lib = LegacyTextLib::parse(&buf).unwrap();
        assert_eq!((lib.desc.as_str(), lib.author.as_str()), ("Dark theme", "phrac"));
        assert_eq!(lib.files, [("MAIN.ANS".to_string(), b"abc".to_vec()), ("LOGOFF.ANS".to_string(), b"de".to_vec())]);
        let names = [pstr("DARK", 8), pstr("LIGHT", 8)].concat();
        assert_eq!(text_lib_names(&names), ["DARK", "LIGHT"]);
    }

    #[test]
    fn votes() {
        let buf = b"\x02\x00\x05\x00JD\x02hi\x03\x00\x05\x00XY\x00";
        let v = parse_votes(buf);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], LegacyVote { voter: 2, candidate: 5, initials: "JD".into(), comment: "hi".into() });
        assert_eq!(v[1].comment, "");
    }
}
