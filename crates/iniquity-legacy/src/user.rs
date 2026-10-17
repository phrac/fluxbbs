//! USERS.DAT: an array of `tUserRec` (1294 bytes). Record N is user N.

use iniquity_term::Color;

use crate::pascal::{Reader, check};

pub const SIZE: usize = 1294;

/// `tUserACflag` members, in declaration order.
pub mod ac {
    pub const ANSI: u32 = 0;
    pub const AVATAR: u32 = 1;
    pub const RIP: u32 = 2;
    pub const YES_NO_BAR: u32 = 3;
    pub const DELETED: u32 = 4;
    pub const EXPERT: u32 = 5;
    pub const HOT_KEY: u32 = 6;
    pub const PAUSE: u32 = 7;
    pub const QUOTE: u32 = 8;
}

#[derive(Debug, Clone)]
pub struct LegacyUser {
    pub number: i16,
    pub user_name: String,
    pub real_name: String,
    /// Stored in plain text, upper-case.
    pub password: String,
    pub phone: String,
    /// "MM/DD/YY"
    pub birth_date: String,
    pub location: String,
    pub address: String,
    pub user_note: String,
    pub sex: char,
    pub sl: u8,
    pub dsl: u8,
    pub baud_rate: i32,
    pub total_calls: u16,
    pub cur_msg_area: u16,
    pub cur_file_area: u16,
    pub ac_flags: Vec<u32>,
    pub colors: [Color; 11],
    pub last_call: String,
    pub page_length: u16,
    pub email_waiting: u16,
    pub level: char,
    pub time_today: u16,
    pub time_per_day: u16,
    pub autosig: Vec<String>,
    pub conf_msg: u8,
    pub conf_file: u8,
    pub first_call: String,
    pub start_menu: String,
    pub file_scan: String,
    pub sysop_note: String,
    pub posts: u16,
    pub email: u16,
    pub calls_today: u16,
    /// Access flags 'A'..='Z'.
    pub flags: Vec<char>,
    pub file_points: u16,
    pub post_call: u16,
    pub limit_dl: u16,
    pub limit_dl_kb: u16,
    pub today_dl: u16,
    pub today_dl_kb: u16,
    pub uldl_ratio: u16,
    pub kb_ratio: u16,
    pub zip_code: String,
    /// Text library chosen (1-based into TEXTLIB.DAT; 0 or out of range
    /// means the first).
    pub text_lib: u8,
    pub vote_yes: u8,
    pub vote_no: u8,
    pub uploads: i32,
    pub downloads: i32,
    pub upload_kb: i32,
    pub download_kb: i32,
}

impl LegacyUser {
    pub fn has(&self, flag: u32) -> bool {
        self.ac_flags.contains(&flag)
    }

    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tUserRec", buf, SIZE)?;
        let mut r = Reader::new(buf);
        let number = r.i16();
        let user_name = r.string(36);
        let real_name = r.string(36);
        let password = r.string(20);
        let phone = r.string(13);
        let birth_date = r.string(8);
        let location = r.string(40);
        let address = r.string(36);
        let user_note = r.string(40);
        let sex = r.char();
        let sl = r.u8();
        let dsl = r.u8();
        let baud_rate = r.i32();
        let total_calls = r.u16();
        let cur_msg_area = r.u16();
        let cur_file_area = r.u16();
        let ac_flags = r.set_bits(2);
        let colors = std::array::from_fn(|_| r.color());
        let last_call = r.string(8);
        let page_length = r.u16();
        let email_waiting = r.u16();
        let level = r.char();
        let time_today = r.u16();
        let time_per_day = r.u16();
        let autosig_lines = r.u8() as usize;
        let mut autosig: Vec<String> = (0..6).map(|_| r.string(80)).collect();
        autosig.truncate(autosig_lines.min(6));
        let conf_msg = r.u8();
        let conf_file = r.u8();
        let first_call = r.string(8);
        let start_menu = r.string(8);
        let file_scan = r.string(8);
        let sysop_note = r.string(40);
        let posts = r.u16();
        let email = r.u16();
        r.skip(8); // oldUploads, oldDownloads, oldUploadKb, oldDownloadKb
        let calls_today = r.u16();
        // set of 'A'..'Z' occupies bytes 8..=11 of the full 32-byte set,
        // so bit 0 of the first stored byte is ordinal 64 ('@').
        let flags = r.set_bits(4).into_iter().filter_map(|b| char::from_u32(64 + b)).filter(char::is_ascii_uppercase).collect();
        let file_points = r.u16();
        let post_call = r.u16();
        let limit_dl = r.u16();
        let limit_dl_kb = r.u16();
        let today_dl = r.u16();
        let today_dl_kb = r.u16();
        r.skip(4); // lastQwkDate
        let uldl_ratio = r.u16();
        let kb_ratio = r.u16();
        let text_lib = r.u8();
        let zip_code = r.string(10);
        let vote_yes = r.u8();
        let vote_no = r.u8();
        let uploads = r.i32();
        let downloads = r.i32();
        let upload_kb = r.i32();
        let download_kb = r.i32();
        debug_assert_eq!(r.pos(), SIZE - 364);
        Ok(Self {
            number, user_name, real_name, password, phone, birth_date, location, address,
            user_note, sex, sl, dsl, baud_rate, total_calls, cur_msg_area, cur_file_area,
            ac_flags, colors, last_call, page_length, email_waiting, level, time_today,
            time_per_day, autosig, conf_msg, conf_file, first_call, start_menu, file_scan,
            sysop_note, posts, email, calls_today, flags, file_points, post_call, limit_dl,
            limit_dl_kb, today_dl, today_dl_kb, uldl_ratio, kb_ratio, zip_code, text_lib, vote_yes,
            vote_no, uploads, downloads, upload_kb, download_kb,
        })
    }

    /// Every record in a USERS.DAT file.
    pub fn parse_file(buf: &[u8]) -> Result<Vec<Self>, crate::Error> {
        buf.as_chunks::<SIZE>().0.iter().map(|c| Self::parse(c)).collect()
    }
}
