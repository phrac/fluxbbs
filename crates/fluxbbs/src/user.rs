//! A caller's account (`tUserRec`).

use std::collections::BTreeSet;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use chrono::{NaiveDate, NaiveDateTime};
use fluxbbs_term::Palette;

use crate::config::Level;

#[derive(Debug, Clone, PartialEq)]
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
    /// Chosen screen theme (text library); empty for the default.
    pub theme: String,
    /// Fingerprint of the last REP packet's replies (`lastQwkDate`), so
    /// the same packet isn't posted twice.
    pub last_rep: String,
    /// Time bank: minutes and kilobytes saved, and what went in and out
    /// today (reset each day, as the download counters are).
    pub bank_time: u32,
    pub bank_kb: u32,
    pub bank_time_in: u32,
    pub bank_time_out: u32,
    pub bank_kb_in: u32,
    pub bank_kb_out: u32,
    /// New: OpenSSH public keys (`authorized_keys` lines) that log this
    /// user straight in over SSH.
    pub ssh_keys: Vec<String>,
    /// New: another SSH user name the keys work under (blank: the handle).
    pub ssh_user: String,
    /// New: a few lines about themselves, shown by finger (`UF`).
    pub plan: Vec<String>,
    /// New: their profile's ANSI header (uploaded), a few lines.
    pub profile_header: Vec<String>,
    /// New: their real name is shown on their profile.
    pub show_real_name: bool,
    /// New: what the board remembers of their interests, for the AI chat
    /// (`aimemory.rs`); theirs to see and clear.
    pub ai_profile: String,
    /// New: whether they've asked the board to keep that memory; off
    /// until they turn it on (`Q` in user config).
    pub ai_memory: bool,
    /// New: `plain` for plain text (no ANSI), `ansi`, or blank: plain for
    /// bots (flag B).
    pub text_mode: String,
    /// New: code's background (`markup::CODE_BACKS`); blank for gray.
    pub code_bg: String,
    /// New: color themes (`markup::THEMES`) for code and for Markdown;
    /// blank for Nord.
    pub code_theme: String,
    pub md_theme: String,
    /// New: `off` stops menus animating; blank, they do.
    pub animate: String,
    /// New: their screen saver (`rain`, `stars`, `fire`, `random`);
    /// blank for the board's.
    pub saver: String,
    /// New: `vim` for vim keys in the full-screen editor; blank for its
    /// own.
    pub editor_keys: String,
    /// New: their email address (`emailver`), shown to them and SysOps
    /// only, and the day it was confirmed (blank: not yet).
    pub mail_address: String,
    pub mail_verified: String,
    /// New: the time zone times are shown to them in (an IANA name or
    /// an offset, as `clock::Zone::parse` reads it); blank for the board's.
    pub time_zone: String,
    /// New: how the message centre orders threads (`hot`, `top`; blank
    /// for latest first).
    pub thread_sort: String,
    /// New: how the message center orders areas (`popular`; blank for by
    /// number).
    pub area_sort: String,
    /// New: the caller's favorite message areas ("my boards"), storage
    /// keys separated by spaces.
    pub fav_boards: String,
    /// New: when the last call began (board time); `last_call` is its day.
    pub last_call_at: Option<NaiveDateTime>,
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
            sex: 'P',
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
            theme: String::new(),
            last_rep: String::new(),
            bank_time: 0,
            bank_kb: 0,
            bank_time_in: 0,
            bank_time_out: 0,
            bank_kb_in: 0,
            bank_kb_out: 0,
            ssh_keys: Vec::new(),
            ssh_user: String::new(),
            plan: Vec::new(),
            profile_header: Vec::new(),
            show_real_name: false,
            ai_profile: String::new(),
            ai_memory: false,
            text_mode: String::new(),
            code_bg: String::new(),
            code_theme: String::new(),
            md_theme: String::new(),
            animate: String::new(),
            saver: String::new(),
            editor_keys: String::new(),
            mail_address: String::new(),
            mail_verified: String::new(),
            time_zone: String::new(),
            thread_sort: String::new(),
            area_sort: String::new(),
            fav_boards: String::new(),
            last_call_at: None,
        }
    }
}

/// Genders: the letter stored (and tested by the `g` ACS) and how it's
/// shown. Iniquity had only M and F; P is "prefer not to say", also what
/// users the board never asked get.
pub const GENDERS: [(char, &str); 4] = [('M', "Male"), ('F', "Female"), ('O', "Other"), ('P', "Not given")];

pub fn gender_name(c: char) -> &'static str {
    GENDERS.iter().find(|g| g.0 == c.to_ascii_uppercase()).map_or("Not given", |g| g.1)
}

/// New: take the fields an editor changed (`original` to `edited`), and
/// only those, so an online caller's own changes survive a SysOp's edit.
/// Counters (time, posts, file points, the bank...) take the change
/// rather than the value, so two at once both count.
macro_rules! merge_fields {
    ($me:expr, $original:expr, $edited:expr, [$($f:ident),* $(,)?], [$($c:ident),* $(,)?]) => {{
        // Every field, so a new one can't be forgotten here.
        let User { $($f,)* $($c,)* } = $edited;
        $(
            if *$f != $original.$f {
                $me.$f = $f.clone();
            }
        )*
        $(
            if *$c != $original.$c {
                $me.$c = add_change($me.$c, $original.$c, *$c);
            }
        )*
    }};
}

/// `now` moved on by what took `from` to `to`, kept in range (0 up to
/// the largest of the three, if it would go past what the type holds).
fn add_change<T: Copy + Ord + Into<i128> + TryFrom<i128>>(now: T, from: T, to: T) -> T {
    let v: i128 = now.into() + to.into() - from.into();
    T::try_from(v.max(0)).unwrap_or_else(|_| now.max(to).max(from))
}

impl User {
    pub fn merge_edit(&mut self, original: &User, edited: &User) {
        merge_fields!(
            self,
            original,
            edited,
            [
                id,
                handle,
                real_name,
                password_hash,
                phone,
                birth_date,
                location,
                address,
                zip_code,
                note,
                sysop_note,
                sex,
                sl,
                dsl,
                level,
                flags,
                hot_keys,
                expert,
                pause,
                quote,
                yes_no_bars,
                deleted,
                palette,
                page_length,
                start_menu,
                first_call,
                last_call,
                time_per_day,
                post_call,
                limit_dl,
                limit_dl_kb,
                uldl_ratio,
                kb_ratio,
                cur_msg_area,
                cur_file_area,
                conf_msg,
                conf_file,
                autosig,
                file_scan,
                theme,
                last_rep,
                ssh_keys,
                ssh_user,
                plan,
                time_zone,
                thread_sort,
                area_sort,
                fav_boards,
                last_call_at,
                profile_header,
                show_real_name,
                ai_profile,
                ai_memory,
                text_mode,
                code_bg,
                code_theme,
                md_theme,
                animate,
                saver,
                editor_keys,
                mail_address,
                mail_verified,
            ],
            [
                total_calls,
                calls_today,
                time_today,
                posts,
                email,
                uploads,
                downloads,
                upload_kb,
                download_kb,
                file_points,
                today_dl,
                today_dl_kb,
                vote_yes,
                vote_no,
                bank_time,
                bank_kb,
                bank_time_in,
                bank_time_out,
                bank_kb_in,
                bank_kb_out,
            ]
        );
    }

    /// [`Self::merge_edit`], counters too taken as values (what a SysOp
    /// typed, a level's limits).
    pub fn merge_values(&mut self, original: &User, edited: &User) {
        merge_fields!(
            self,
            original,
            edited,
            [
                id,
                handle,
                real_name,
                password_hash,
                phone,
                birth_date,
                location,
                address,
                zip_code,
                note,
                sysop_note,
                sex,
                sl,
                dsl,
                level,
                flags,
                hot_keys,
                expert,
                pause,
                quote,
                yes_no_bars,
                deleted,
                palette,
                page_length,
                start_menu,
                first_call,
                last_call,
                time_per_day,
                post_call,
                limit_dl,
                limit_dl_kb,
                uldl_ratio,
                kb_ratio,
                cur_msg_area,
                cur_file_area,
                conf_msg,
                conf_file,
                autosig,
                file_scan,
                theme,
                last_rep,
                ssh_keys,
                ssh_user,
                plan,
                time_zone,
                thread_sort,
                area_sort,
                fav_boards,
                last_call_at,
                profile_header,
                show_real_name,
                ai_profile,
                ai_memory,
                text_mode,
                code_bg,
                code_theme,
                md_theme,
                animate,
                saver,
                editor_keys,
                mail_address,
                mail_verified,
                            total_calls,
                calls_today,
                time_today,
                posts,
                email,
                uploads,
                downloads,
                upload_kb,
                download_kb,
                file_points,
                today_dl,
                today_dl_kb,
                vote_yes,
                vote_no,
                bank_time,
                bank_kb,
                bank_time_in,
                bank_time_out,
                bank_kb_in,
                bank_kb_out,
                        ],
            []
        );
    }

    /// Passwords are case-sensitive (new: Iniquity upper-cased them).
    pub fn set_password(&mut self, pw: &str) {
        self.password_hash = Argon2::default().hash_password(pw.as_bytes()).expect("argon2 hashing").to_string();
    }

    /// Passwords set before they were case-sensitive (imported ones, and
    /// any set by earlier versions) were stored upper-cased, so the
    /// upper-cased guess is tried too. That only lets a password in when
    /// it was all capitals to begin with.
    pub fn check_password(&self, pw: &str) -> bool {
        let Ok(h) = PasswordHash::new(&self.password_hash) else { return false };
        let ok = |p: &str| Argon2::default().verify_password(p.as_bytes(), &h).is_ok();
        ok(pw) || (pw != pw.to_uppercase() && ok(&pw.to_uppercase()))
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

    /// Kilobytes the user may download today: the daily limit, less what
    /// they banked today, plus what they took out of the bank.
    pub fn dl_kb_allowance(&self) -> u64 {
        (self.limit_dl_kb as u64 + self.bank_kb_out as u64).saturating_sub(self.bank_kb_in as u64)
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
    fn an_edit_keeps_the_callers_own_changes() {
        let original = User { time_per_day: 60, posts: 3, ..User::default() };
        let mut edited = original.clone();
        edited.time_per_day = 120;
        // Meanwhile, online, they posted.
        let mut live = original.clone();
        live.posts = 4;
        live.merge_edit(&original, &edited);
        assert_eq!((live.time_per_day, live.posts), (120, 4));
        // Counters add: 30 minutes given while 5 were used is 25 more.
        let base = User { time_today: 60, ..User::default() };
        let mut now = User { time_today: 90, ..User::default() };
        now.merge_edit(&base, &User { time_today: 55, ..User::default() });
        assert_eq!(now.time_today, 85);
        let mut low = User { time_today: 3, ..User::default() };
        low.merge_edit(&base, &User { time_today: 0, ..User::default() });
        assert_eq!(low.time_today, 0, "never below nothing");
        // A value a SysOp typed stays what was typed.
        let mut db = User { file_points: 150, ..User::default() };
        db.merge_values(&User { file_points: 100, ..User::default() }, &User { file_points: 0, ..User::default() });
        assert_eq!(db.file_points, 0);
        assert_eq!(add_change(u32::MAX - 1, 0u32, 5), u32::MAX - 1, "past the top: the largest, not a wrap");
    }

    #[test]
    fn passwords_are_case_sensitive() {
        let mut u = User::default();
        u.set_password("Secret");
        assert!(u.check_password("Secret"));
        assert!(!u.check_password("SECRET"));
        assert!(!u.check_password("secret"));
        // An old, upper-cased one still takes any case.
        u.set_password("SECRET");
        assert!(u.check_password("secret"));
        assert!(u.check_password("SECRET"));
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
