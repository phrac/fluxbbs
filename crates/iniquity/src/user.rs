//! A caller's account (`tUserRec`).

use std::collections::BTreeSet;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use chrono::NaiveDate;
use iniquity_term::Palette;

use crate::config::Level;

#[derive(Debug, Clone)]
pub struct User {
    /// 0 until saved.
    pub id: u32,
    pub handle: String,
    pub real_name: String,
    /// PHC string (argon2id).
    pub password_hash: String,
    pub phone: String,
    pub birth_date: Option<NaiveDate>,
    pub location: String,
    pub address: String,
    pub zip_code: String,
    pub note: String,
    pub sysop_note: String,
    pub sex: char,
    pub sl: u8,
    pub dsl: u8,
    pub level: char,
    /// Access flags A-Z (ACS `Fx`).
    pub flags: BTreeSet<char>,
    pub hot_keys: bool,
    pub expert: bool,
    pub pause: bool,
    pub quote: bool,
    pub yes_no_bars: bool,
    pub deleted: bool,
    pub palette: Palette,
    pub page_length: u16,
    pub start_menu: String,
    pub total_calls: u32,
    pub calls_today: u32,
    pub first_call: Option<NaiveDate>,
    pub last_call: Option<NaiveDate>,
    /// Minutes left today, and the daily allowance.
    pub time_today: u32,
    pub time_per_day: u32,
    pub posts: u32,
    pub email: u32,
    pub uploads: u32,
    pub downloads: u32,
    pub upload_kb: u64,
    pub download_kb: u64,
    pub file_points: u32,
    pub post_call: u32,
    pub limit_dl: u32,
    pub limit_dl_kb: u32,
    pub today_dl: u32,
    pub today_dl_kb: u32,
    pub uldl_ratio: u32,
    pub kb_ratio: u32,
    pub cur_msg_area: u32,
    pub cur_file_area: u32,
    pub conf_msg: u32,
    pub conf_file: u32,
    pub autosig: Vec<String>,
    pub vote_yes: u32,
    pub vote_no: u32,
    /// Files uploaded since this date are new (`fileScan`).
    pub file_scan: Option<NaiveDate>,
}

impl Default for User {
    /// `userPrelogonInit`: the placeholder before anyone logs in.
    fn default() -> Self {
        Self {
            id: 0,
            handle: "Unknown".into(),
            real_name: "Unknown".into(),
            password_hash: String::new(),
            phone: "(000)000-0000".into(),
            birth_date: None,
            location: "Somewhere, XX".into(),
            address: "Unknown".into(),
            zip_code: String::new(),
            note: "None".into(),
            sysop_note: "None".into(),
            sex: 'M',
            sl: 25,
            dsl: 25,
            level: 'A',
            flags: BTreeSet::new(),
            hot_keys: true,
            expert: false,
            pause: true,
            quote: true,
            yes_no_bars: true,
            deleted: false,
            palette: Palette::default(),
            page_length: 24,
            start_menu: String::new(),
            total_calls: 0,
            calls_today: 0,
            first_call: None,
            last_call: None,
            time_today: 15,
            time_per_day: 15,
            posts: 0,
            email: 0,
            uploads: 0,
            downloads: 0,
            upload_kb: 0,
            download_kb: 0,
            file_points: 0,
            post_call: 0,
            limit_dl: 0,
            limit_dl_kb: 0,
            today_dl: 0,
            today_dl_kb: 0,
            uldl_ratio: 0,
            kb_ratio: 0,
            cur_msg_area: 1,
            cur_file_area: 1,
            conf_msg: 1,
            conf_file: 1,
            autosig: Vec::new(),
            vote_yes: 0,
            vote_no: 0,
            file_scan: None,
        }
    }
}

/// Iniquity passwords are case-insensitive (always upper-cased on entry),
/// and imported ones are stored that way.
fn normalize(pw: &str) -> String {
    pw.to_uppercase()
}

impl User {
    pub fn set_password(&mut self, pw: &str) {
        self.password_hash = Argon2::default()
            .hash_password(normalize(pw).as_bytes())
            .expect("argon2 hashing")
            .to_string();
    }

    pub fn check_password(&self, pw: &str) -> bool {
        PasswordHash::new(&self.password_hash)
            .is_ok_and(|h| Argon2::default().verify_password(normalize(pw).as_bytes(), &h).is_ok())
    }

    /// `levelSet`: apply an access level's limits.
    pub fn apply_level(&mut self, key: char, level: &Level) {
        self.level = key;
        self.sl = level.sl;
        self.dsl = level.dsl;
        self.file_points = level.file_points as u32;
        self.post_call = level.post_call as u32;
        self.limit_dl = level.limit_dl as u32;
        self.limit_dl_kb = level.limit_dl_kb as u32;
        self.note = level.note.clone();
        self.time_per_day = level.time_limit as u32;
        self.time_today = level.time_limit as u32;
        self.uldl_ratio = level.uldl_ratio as u32;
        self.kb_ratio = level.kb_ratio as u32;
    }

    /// Age in whole years, if the birth date is known.
    pub fn age(&self, today: NaiveDate) -> Option<u32> {
        self.birth_date.and_then(|b| today.years_since(b))
    }

    /// Area code from a "(xxx)xxx-xxxx" phone number.
    pub fn area_code(&self) -> &str {
        self.phone.get(1..4).unwrap_or("")
    }
}

/// Dates are "MM/DD/YY" on screen, as in the original.
pub fn fmt_date(d: Option<NaiveDate>) -> String {
    d.map_or_else(|| "00/00/00".into(), |d| d.format("%m/%d/%y").to_string())
}

/// Parse "MM/DD/YY". Two-digit years from 30 on are 19xx.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let mut it = s.split('/').map(|p| p.trim().parse::<u32>().ok());
    let (m, d, y) = (it.next()??, it.next()??, it.next()??);
    let year = match y {
        0..=29 => 2000 + y,
        30..=99 => 1900 + y,
        _ => y,
    };
    NaiveDate::from_ymd_opt(year as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_case_insensitive() {
        let mut u = User::default();
        u.set_password("Secret");
        assert!(u.check_password("SECRET"));
        assert!(u.check_password("secret"));
        assert!(!u.check_password("other"));
    }

    #[test]
    fn dates() {
        let d = parse_date("01/02/80").unwrap();
        assert_eq!(fmt_date(Some(d)), "01/02/80");
        assert_eq!(parse_date("11/24/97").unwrap().to_string(), "1997-11-24");
        assert_eq!(parse_date("10/17/26").unwrap().to_string(), "2026-10-17");
        assert!(parse_date("13/40/99").is_none());
        assert!(parse_date("00/00/00").is_none());
    }
}
