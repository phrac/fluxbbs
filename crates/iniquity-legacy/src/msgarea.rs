//! Message areas and message bases.
//!
//! * MSGAREA.DAT / EMAIL.DAT: `tMsgAreaRec` (299 bytes) per area.
//! * MSGCONF.DAT: `tConfRec` (53 bytes) per conference.
//! * `<area>.HDR`: `tMsgHeaderRec` (440 bytes) per message; message N is
//!   record N, deleted ones included until the area is packed.
//! * `<area>.DAT`: message lines, each a length byte then the text;
//!   a header's `pos` is the 1-based offset of its first line.

use chrono::NaiveDateTime;

use crate::pascal::{Reader, check};

pub const AREA_SIZE: usize = 299;
pub const CONF_SIZE: usize = 53;
pub const HEADER_SIZE: usize = 440;

/// `tMsgAreaFlag` members.
pub mod flag {
    pub const UNHIDDEN: u32 = 0;
    pub const REAL_NAME: u32 = 1;
    pub const PRIVATE: u32 = 2;
    pub const MANDATORY: u32 = 3;
    pub const ANONYMOUS: u32 = 4;
}

/// `tMsgStatusFlag` members.
pub mod status {
    pub const DELETED: u32 = 0;
    pub const SENT: u32 = 1;
    pub const ANONYMOUS: u32 = 2;
    pub const ECHOMAIL: u32 = 3;
    pub const PRIVATE: u32 = 4;
    pub const FORWARDED: u32 = 5;
}

#[derive(Debug, Clone)]
pub struct LegacyMsgArea {
    pub name: String,
    pub filename: String,
    pub sponsor: String,
    pub acs: String,
    pub post_acs: String,
    pub max_msgs: u16,
    pub msgs: u16,
    pub password: String,
    pub flags: Vec<u32>,
    /// 0 email, 1 normal, other values echomail/netmail.
    pub area_type: u8,
    pub qwk_name: String,
}

impl LegacyMsgArea {
    pub fn has(&self, f: u32) -> bool {
        self.flags.contains(&f)
    }

    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tMsgAreaRec", buf, AREA_SIZE)?;
        let mut r = Reader::new(buf);
        let name = r.string(40);
        let filename = r.string(8);
        r.skip(41); // MsgPath (unused by a26)
        let sponsor = r.string(36);
        let acs = r.string(20);
        let post_acs = r.string(20);
        let max_msgs = r.u16();
        let msgs = r.u16();
        let password = r.string(20);
        let flags = r.set_bits(1);
        let area_type = r.u8();
        r.skip(2); // Origin, Address
        let qwk_name = r.string(16);
        Ok(Self { name, filename, sponsor, acs, post_acs, max_msgs, msgs, password, flags, area_type, qwk_name })
    }

    pub fn parse_file(buf: &[u8]) -> Result<Vec<Self>, crate::Error> {
        buf.as_chunks::<AREA_SIZE>().0.iter().map(|c| Self::parse(c)).collect()
    }
}

#[derive(Debug, Clone)]
pub struct LegacyConf {
    pub desc: String,
    pub acs: String,
    pub key: char,
}

impl LegacyConf {
    pub fn parse_file(buf: &[u8]) -> Vec<Self> {
        buf.as_chunks::<CONF_SIZE>()
            .0
            .iter()
            .map(|c| {
                let mut r = Reader::new(c);
                LegacyConf { desc: r.string(30), acs: r.string(20), key: r.char() }
            })
            .collect()
    }
}

/// `tMsgInfoRec`: one end of a message.
#[derive(Debug, Clone, Default)]
pub struct LegacyMsgInfo {
    pub user_num: u16,
    pub alias: String,
    pub real_name: String,
    /// The name shown on the message.
    pub name: String,
    pub user_note: String,
}

#[derive(Debug, Clone)]
pub struct LegacyMsgHeader {
    pub from: LegacyMsgInfo,
    pub to: LegacyMsgInfo,
    /// 1-based offset of the first line in the .DAT file.
    pub pos: u32,
    /// Number of lines.
    pub size: u16,
    pub date: Option<NaiveDateTime>,
    pub status: Vec<u32>,
    pub replies: u16,
    pub subject: String,
    /// Line where the auto-signature starts (0 = none).
    pub sig_pos: u16,
}

fn info(r: &mut Reader) -> LegacyMsgInfo {
    let i = LegacyMsgInfo {
        user_num: r.u16(),
        alias: r.string(36),
        real_name: r.string(36),
        name: r.string(36),
        user_note: r.string(40),
    };
    r.skip(8); // tNetAddressRec
    i
}

/// A DOS packed date/time (`PackTime`): date in the high word, time low.
pub fn unpack_dos_time(v: u32) -> Option<NaiveDateTime> {
    let (date, time) = ((v >> 16) as u16, v as u16);
    let d = chrono::NaiveDate::from_ymd_opt(1980 + (date >> 9) as i32, ((date >> 5) & 15) as u32, (date & 31) as u32)?;
    d.and_hms_opt((time >> 11) as u32, ((time >> 5) & 63) as u32, ((time & 31) * 2) as u32)
}

impl LegacyMsgHeader {
    pub fn has(&self, s: u32) -> bool {
        self.status.contains(&s)
    }

    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tMsgHeaderRec", buf, HEADER_SIZE)?;
        let mut r = Reader::new(buf);
        let from = info(&mut r);
        let to = info(&mut r);
        let pos = r.u32();
        let size = r.u16();
        let date = unpack_dos_time(r.u32());
        let status = r.set_bits(1);
        let replies = r.u16();
        let subject = r.string(40);
        r.skip(2); // NetFlag
        let sig_pos = r.u16();
        Ok(Self { from, to, pos, size, date, status, replies, subject, sig_pos })
    }

    pub fn parse_file(buf: &[u8]) -> Result<Vec<Self>, crate::Error> {
        buf.as_chunks::<HEADER_SIZE>().0.iter().map(|c| Self::parse(c)).collect()
    }

    /// This message's lines from the area's .DAT file.
    pub fn lines(&self, dat: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = (self.pos as usize).saturating_sub(1);
        for _ in 0..self.size {
            let Some(&len) = dat.get(i) else { break };
            let end = (i + 1 + len as usize).min(dat.len());
            out.push(iniquity_term::cp437::decode(&dat[i + 1..end]));
            i = end;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_time() {
        // 1997-11-24 13:45:30
        let v = (((1997 - 1980) << 9 | 11 << 5 | 24) << 16) | (13 << 11 | 45 << 5 | 15);
        assert_eq!(unpack_dos_time(v).unwrap().to_string(), "1997-11-24 13:45:30");
        assert!(unpack_dos_time(0).is_none());
    }

    #[test]
    fn message_lines() {
        let dat = b"\x05hello\x00\x05world";
        let h = LegacyMsgHeader {
            from: Default::default(),
            to: Default::default(),
            pos: 1,
            size: 3,
            date: None,
            status: vec![],
            replies: 0,
            subject: String::new(),
            sig_pos: 0,
        };
        assert_eq!(h.lines(dat), vec!["hello", "", "world"]);
    }
}
