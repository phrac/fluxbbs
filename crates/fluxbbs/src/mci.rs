//! `%XX` MCI values (`mciProcessMCICode`).

use chrono::{Datelike};

use crate::session::Session;
use crate::user::fmt_date;

/// Codes the original understood that have no data behind them yet
/// (message/file areas, conferences, history, system stats). They render
/// empty rather than as literal text, so stock screens don't show raw codes.
const PENDING: &[&[u8; 2]] = &[
    b"BR", b"LA", b"LD",
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

/// The second letters of the file menu's stats codes (new).
const FILE_STATS: &[u8] = b"BEGKLMNQRTUVWXYZ";

/// The second letters of the message menu's codes (new).
const MSG_STATS: &[u8] = b"BCDFGHJKLQRVXY";

/// How long ago, briefly ("3d ago").
pub(crate) fn ago(d: Option<chrono::NaiveDate>, today: chrono::NaiveDate) -> String {
    let Some(d) = d else { return String::new() };
    match (today - d).num_days() {
        ..=0 => "today".into(),
        n @ 1..=13 => format!("{n}d ago"),
        n @ 14..=60 => format!("{}w ago", n / 7),
        n @ 61..=729 => format!("{}mo ago", n / 30),
        n => format!("{}y ago", n / 365),
    }
}

/// The first line of a description with words in it (FILE_ID.DIZ files
/// often open with a banner), spaces squeezed.
pub(crate) fn blurb(desc: &[String]) -> String {
    desc.iter()
        .find(|l| {
            // Mostly plain words: letters and digits with ordinary punctuation.
            let plain = |w: &str| w.chars().filter(char::is_ascii_alphabetic).count() >= 2 && w.chars().all(|c| c.is_ascii_alphanumeric() || ".,'-!?:;()/&".contains(c));
            let (mut good, mut good_len, mut len) = (0, 0, 0);
            for w in l.split_whitespace() {
                len += w.len();
                if plain(w) {
                    good += 1;
                    good_len += w.len();
                }
            }
            good >= 2 && good_len * 10 >= len * 7
        })
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// The random file shown: one with something to say, if any has.
pub(crate) fn dig(s: &crate::filebase::FileStats) -> Option<&crate::filebase::FileRec> {
    s.random.iter().find(|f| !blurb(&f.description).is_empty()).or(s.random.first())
}

impl Session {
    /// The file stats, worked out again once they're a minute old; `shown`
    /// keeps the ones on screen however old (to act on what was shown).
    pub(crate) fn file_stats(&self, shown: bool) -> std::sync::Arc<crate::filebase::FileStats> {
        let cached = self.file_stats.borrow().as_ref().filter(|(at, _)| shown || at.elapsed().as_secs() < 60).map(|(_, s)| s.clone());
        cached.unwrap_or_else(|| {
            let fa = self.state.fareas();
            // Before anyone has logged on (PRELOGON, the matrix), the
            // figures are what a new caller would get.
            let areas: Vec<String> = if self.logged_in {
                self.visible_fareas().into_iter().filter_map(|(n, _)| fa.get(n).map(|a| a.file.clone())).collect()
            } else {
                crate::web::public_fareas(&self.state)
            };
            let s = std::sync::Arc::new(self.state.files.stats(&areas, self.user.file_scan).unwrap_or_default());
            *self.file_stats.borrow_mut() = Some((std::time::Instant::now(), s.clone()));
            s
        })
    }

    /// New: stats over the file areas the caller can see, for the file
    /// menu. `T` files, `E` areas, `Z` their size, `Y` new since the caller's scan
    /// date; the newest upload's name `N`, uploader `U`, area `W` and age
    /// `G`; the most downloaded `M` and its downloads `X`; the biggest `B`
    /// and its size `L`; the top uploader `K` and their files `Q`; and a
    /// random file `R` (one whose description has words in it, where it
    /// can) with that line `V`.
    fn file_stat(&self, c: u8) -> String {
        let s = self.file_stats(false);
        let name = |f: &Option<crate::filebase::FileRec>| f.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        let commas = |n: u32| crate::files::commas(n as u64);
        match c {
            b'T' => commas(s.files),
            b'E' => commas(s.areas),
            b'Z' => crate::upscan::human_size(s.bytes),
            b'Y' => commas(s.since),
            b'N' => name(&s.newest),
            b'U' => s.newest.as_ref().map(|f| f.uploader.clone()).unwrap_or_default(),
            b'W' => {
                let fa = self.state.fareas();
                s.newest.as_ref().and_then(|f| fa.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&f.area))).map(|a| a.name.clone()).unwrap_or_default()
            }
            b'G' => ago(s.newest.as_ref().and_then(|f| f.uploaded), crate::clock::now().date_naive()),
            b'M' => s.popular.as_ref().map_or_else(|| "nothing yet".into(), |f| f.name.clone()),
            b'X' => s.popular.as_ref().map_or(0, |f| f.downloads).to_string(),
            b'B' => name(&s.biggest),
            b'L' => s.biggest.as_ref().map(|f| crate::upscan::human_size(f.size)).unwrap_or_default(),
            b'K' => s.top_uploader.as_ref().map(|t| t.0.clone()).unwrap_or_default(),
            b'Q' => commas(s.top_uploader.as_ref().map_or(0, |t| t.1)),
            b'R' | b'V' => {
                match (c, dig(&s)) {
                    (b'R', Some(f)) => f.name.clone(),
                    (_, Some(f)) => blurb(&f.description),
                    _ => String::new(),
                }
            }
            _ => String::new(),
        }
    }

    /// New: the message menu's figures, over the public messages in the
    /// areas the caller can read. `G` messages, `Y` new for the caller;
    /// the newest post's subject `B`, author `F`, age `D` and area `K`;
    /// the most replied-to `H` and its replies `R`; the loudest area of
    /// the last 30 days `L` and its posts `Q`; the top poster `C` and their
    /// posts `X`; a message at random `V` with a line of it `J`.
    fn msg_stat(&self, c: u8) -> String {
        let f = self.msg_stats(false);
        let s = &f.base;
        let commas = |n: u32| crate::files::commas(n as u64);
        let author = |m: &crate::msgbase::Message| if m.anonymous { "anonymous".to_string() } else { m.from.name.clone() };
        match c {
            b'G' => commas(s.total),
            b'Y' => commas(f.unread as u32),
            b'B' => s.newest.as_ref().map(|m| m.subject.clone()).unwrap_or_default(),
            b'F' => s.newest.as_ref().map(author).unwrap_or_default(),
            b'D' => ago(s.newest.as_ref().map(|m| m.posted.date()), crate::clock::now().date_naive()),
            b'K' => s.newest.as_ref().map(|m| f.area_name(&m.area)).unwrap_or_default(),
            b'H' => s.hot.as_ref().map_or_else(|| "nothing yet".into(), |m| m.subject.clone()),
            b'R' => s.hot.as_ref().map_or(0, |m| m.replies).to_string(),
            b'L' => s.loudest.as_ref().map(|(k, _)| f.area_name(k)).unwrap_or_default(),
            b'Q' => commas(s.loudest.as_ref().map_or(0, |l| l.1)),
            b'C' => s.mouth.as_ref().map(|t| t.0.clone()).unwrap_or_default(),
            b'X' => commas(s.mouth.as_ref().map_or(0, |t| t.1)),
            b'V' => f.dig().map(|m| m.subject.clone()).unwrap_or_default(),
            b'J' => f.dig().map(|m| blurb(m.body())).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Value for an MCI code, or `None` if it isn't one (the caller then
    /// writes the text literally).
    pub fn mci_value(&self, code: [u8; 2]) -> Option<String> {
        let code = code.map(|c| c.to_ascii_uppercase());
        let cfg = &self.state.config().bbs;
        let u = &self.user;
        // The clock codes show the caller's own time; the day's figures are the board's day.
        let now = self.now();
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
            b"FA" => self.current_farea().map(|a| a.area.name).unwrap_or_default(),
            b"FD" | b"FS" => fmt_date(u.file_scan),
            [b'F', c] if FILE_STATS.contains(c) => self.file_stat(*c),
            b"FP" => u.file_points.to_string(),
            b"CF" => self.state.fareas().conf(u.conf_file).map(|c| c.desc.clone()).unwrap_or_default(),
            b"HK" => yes_no(u.hot_keys),
            b"ID" => u.id.to_string(),
            b"IN" => "flux/bbs".into(),
            b"IS" => self.input_string.clone(),
            b"IV" => env!("CARGO_PKG_VERSION").into(),
            b"LC" => fmt_date(u.last_call),
            b"LO" => u.location.clone(),
            // The current text library (theme).
            b"LA" => self.current_theme().map_or("n/a".into(), |t| t.author),
            b"LD" => self.current_theme().map_or("n/a".into(), |t| t.desc),
            [b'M', c] if MSG_STATS.contains(c) => self.msg_stat(*c),
            b"MI" => u.cur_msg_area.to_string(),
            b"MP" => self.menu.menu.as_ref().map(|m| m.prompt_name.clone()).unwrap_or_default(),
            b"MW" => self.mail_waiting().to_string(),
            b"MA" => self.current_area().map(|a| a.area.name).unwrap_or_default(),
            b"MN" => self.menu_msg.to_string(),
            b"MT" => self.current_area().map_or(0, |a| self.state.msgs.count(&a.area.file).unwrap_or(0)).to_string(),
            b"CM" => self.state.areas().conf(u.conf_msg).map(|c| c.desc.clone()).unwrap_or_default(),
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
            b"SX" => crate::user::gender_name(u.sex).into(),
            // 2.20: minutes in the time bank.
            b"TB" => u.bank_time.to_string(),
            b"TI" => now.format("%-I:%M%P").to_string(),
            b"TL" => self.minutes_left().to_string(),
            b"TM" => now.format("%H:%M").to_string(),
            b"TT" => u.time_per_day.to_string(),
            b"TU" => self.state.users.count().unwrap_or(0).to_string(),
            b"TC" | b"TD" | b"TE" | b"TP" | b"TZ" => {
                let t = self.state.sys.totals().unwrap_or_default();
                match &code {
                    b"TC" => crate::files::commas(t.calls),
                    b"TD" => crate::files::commas(t.downloads),
                    b"TE" => crate::files::commas(t.email),
                    b"TP" => crate::files::commas(t.posts),
                    _ => ratio(t.posts as u32, t.calls as u32),
                }
            }
            // DN (2.20): new users today.
            b"DC" | b"DD" | b"DN" | b"DP" | b"DU" => {
                let d = self.state.sys.day(crate::clock::now().date_naive()).unwrap_or_default();
                let v = match &code {
                    b"DC" => d.calls,
                    b"DD" => d.downloads,
                    b"DN" => d.new_users,
                    b"DP" => d.posts,
                    _ => d.uploads,
                };
                crate::files::commas(v)
            }
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
            b"VL" => format!("flux/bbs {}", env!("CARGO_PKG_VERSION")),
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
    crate::clock::now().weekday().num_days_from_sunday()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(initials("mike fricker"), "MF");
        assert_eq!(ratio(1, 4), "25%");
        assert_eq!(ratio(1, 0), "0%");
        let d = |m, d| chrono::NaiveDate::from_ymd_opt(2026, m, d);
        let today = d(10, 20).unwrap();
        assert_eq!(ago(d(10, 20), today), "today");
        assert_eq!(ago(d(10, 19), today), "1d ago");
        assert_eq!(ago(d(10, 15), today), "5d ago");
        assert_eq!(ago(d(9, 20), today), "4w ago");
        assert_eq!(ago(d(1, 1), today), "9mo ago");
        assert_eq!(ago(None, today), "");
        let d: Vec<String> = ["▒▒▒▒", "i n i q u", ".g\"\"^'°`\"^'+AXF+°", "-=[ AXF ]=-", "  The   best  pack ", "more"].map(String::from).into();
        assert_eq!(blurb(&d), "The best pack");
    }
}
