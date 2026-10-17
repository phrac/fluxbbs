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
    fn votes() {
        let buf = b"\x02\x00\x05\x00JD\x02hi\x03\x00\x05\x00XY\x00";
        let v = parse_votes(buf);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], LegacyVote { voter: 2, candidate: 5, initials: "JD".into(), comment: "hi".into() });
        assert_eq!(v[1].comment, "");
    }
}
