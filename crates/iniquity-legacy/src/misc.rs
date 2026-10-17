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
