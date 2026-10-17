//! `%XX` MCI values (`mciProcessMCICode`).

use chrono::{Datelike, Local};

use crate::session::Session;
use crate::user::fmt_date;

/// Codes the original understood that have no data behind them yet
/// (message/file areas, conferences, history, system stats). They render
/// empty rather than as literal text, so stock screens don't show raw codes.
const PENDING: &[&[u8; 2]] = &[
    b"BR", b"CF", b"DC", b"DD", b"DP", b"DU", b"FA", b"FD", b"FP", b"FS", b"LA", b"LD", b"TC", b"TD",
    b"TE", b"TP", b"TZ",
];

fn yes_no(b: bool) -> String {
    if b { "Yes" } else { "No" }.into()
}

fn ratio(a: u32, b: u32) -> String {
    if b == 0 { "0%".into() } else { format!("{}%", a as u64 * 100 / b as u64) }
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).collect::<String>().to_uppercase()
}

impl Session {
    /// Value for an MCI code, or `None` if it isn't one (the caller then
    /// writes the text literally).
    pub fn mci_value(&self, code: [u8; 2]) -> Option<String> {
        let code = code.map(|c| c.to_ascii_uppercase());
        let cfg = &self.state.config.bbs;
        let u = &self.user;
        let now = Local::now();
        Some(match &code {
            [b'A', n @ b'1'..=b'6'] => u.autosig.get((n - b'1') as usize).cloned().unwrap_or_default(),
            b"AC" => u.area_code().into(),
            b"AD" => u.address.clone(),
            b"AL" => u.autosig.len().to_string(),
            b"AQ" => yes_no(u.quote),
            b"BD" => fmt_date(u.birth_date),
            b"BL" => cfg.location.clone(),
            b"BN" => cfg.name.clone(),
            b"BP" => cfg.phone.clone(),
            b"CT" => u.calls_today.to_string(),
            b"DF" => now.format("%A, %B %-d, %Y").to_string(),
            b"DK" => format!("{}k", u.download_kb),
            b"DL" => u.downloads.to_string(),
            b"DS" => u.dsl.to_string(),
            b"DT" => now.format("%m/%d/%y").to_string(),
            b"EM" => self.emulation_name().into(),
            b"FC" => fmt_date(u.first_call),
            b"FI" => u.cur_file_area.to_string(),
            b"HK" => yes_no(u.hot_keys),
            b"ID" => u.id.to_string(),
            b"IN" => "Iniquity".into(),
            b"IS" => self.input_string.clone(),
            b"IV" => env!("CARGO_PKG_VERSION").into(),
            b"LC" => fmt_date(u.last_call),
            b"LO" => u.location.clone(),
            b"MI" => u.cur_msg_area.to_string(),
            b"MP" => self.menu.menu.as_ref().map(|m| m.prompt_name.clone()).unwrap_or_default(),
            b"MW" => self.mail_waiting().to_string(),
            b"MA" => self.current_area().map(|a| a.area.name).unwrap_or_default(),
            b"MN" => self.menu_msg.to_string(),
            b"MT" => self.current_area().map_or(0, |a| self.state.msgs.count(&a.area.file).unwrap_or(0)).to_string(),
            b"CM" => self.state.areas.conf(u.conf_msg).map(|c| c.desc.clone()).unwrap_or_default(),
            b"ND" => self.node.number().to_string(),
            b"PA" => yes_no(u.pause),
            b"PL" => u.page_length.to_string(),
            b"PN" => u.phone.clone(),
            // The original printed the password; never do that.
            b"PW" => String::new(),
            b"RN" => u.real_name.clone(),
            b"RT" => self.caller.term.clone(),
            b"SL" => u.sl.to_string(),
            b"SN" => cfg.sysop_alias.clone(),
            b"SO" => u.sysop_note.clone(),
            b"SR" => cfg.sysop_name.clone(),
            b"SX" => if u.sex == 'F' { "Female" } else { "Male" }.into(),
            b"TI" => now.format("%-I:%M%P").to_string(),
            b"TL" => self.minutes_left().to_string(),
            b"TM" => now.format("%H:%M").to_string(),
            b"TT" => u.time_per_day.to_string(),
            b"TU" => self.state.users.count().unwrap_or(0).to_string(),
            b"UA" => u.level.to_string(),
            b"UC" => u.total_calls.to_string(),
            b"UE" => u.email.to_string(),
            b"UI" => initials(&u.handle),
            b"UK" => format!("{}k", u.upload_kb),
            b"UL" => u.uploads.to_string(),
            b"UN" => u.handle.clone(),
            b"UO" => u.note.clone(),
            b"UP" => u.posts.to_string(),
            b"UZ" => ratio(u.posts, u.total_calls),
            b"VL" => format!("Iniquity {} (Rust)", env!("CARGO_PKG_VERSION")),
            b"VN" => u.vote_no.to_string(),
            b"VY" => u.vote_yes.to_string(),
            b"XM" => yes_no(u.expert),
            b"YN" => yes_no(u.yes_no_bars),
            [b'@', c @ b'A'..=b'Z'] => yes_no(u.flags.contains(&(*c as char))),
            c if PENDING.contains(&c) => String::new(),
            _ => return None,
        })
    }
}

/// Weekday number, Sunday = 0 (Turbo Pascal `GetDate`).
pub fn day_of_week() -> u32 {
    Local::now().weekday().num_days_from_sunday()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(initials("mike fricker"), "MF");
        assert_eq!(ratio(1, 4), "25%");
        assert_eq!(ratio(1, 0), "0%");
    }
}
