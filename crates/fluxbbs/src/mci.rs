//! `%XX` MCI values (`mciProcessMCICode`).

use chrono::{Datelike};

use crate::session::Session;
use crate::user::{User, fmt_date};

/// Codes the original understood that have no data behind them yet
/// (message/file areas, conferences, history, system stats). They render
/// empty rather than as literal text, so stock screens don't show raw codes.
const PENDING: &[&[u8; 2]] = &[
    b"BR", b"LA", b"LD",
];

/// 1st, 2nd, 3rd, 4th... 11th, 12th, 13th, 21st.
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Ten cells, filled from the left by how far up the field the rank is:
/// first of n fills them all, last leaves one.
fn rank_bar(rank: usize, n: usize) -> String {
    let n = n.max(1);
    let filled = ((n - rank.clamp(1, n) + 1) as u128 * 10).div_ceil(n as u128).clamp(1, 10) as usize;
    "█".repeat(filled) + &"░".repeat(10 - filled)
}

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

/// The second letters of the email codes (new).
const EMAIL_STATS: &[u8] = b"NRTFSDAXY";

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
    /// New: the caller's place among everyone (not deleted) by one
    /// counter: (rank from 1, how many). Ties share the better place.
    fn rank_among_users(&self, what: u8) -> (usize, usize) {
        let value = |u: &User| match what {
            b'C' => u.total_calls,
            b'P' => u.posts,
            b'U' => u.uploads,
            b'D' => u.downloads,
            _ => u.file_points,
        };
        let mine = value(&self.user);
        let others: Vec<u32> = self.state.users.list().unwrap_or_default().iter().filter(|u| !u.deleted && u.id != self.user.id).map(value).collect();
        (1 + others.iter().filter(|&&v| v > mine).count(), others.len() + 1)
    }

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

    /// New: the caller's email. In their box: `T` all, `N` unread, `R`
    /// read; the newest's sender `F`, subject `S`, age `D` and time `A`;
    /// whom they last wrote `X`, and when `Y`.
    fn email_stat(&self, c: u8) -> String {
        let area = self.state.areas().email.file.clone();
        let today = crate::clock::now().date_naive();
        let when = |m: &crate::msgbase::Message| m.posted.format("%Y-%m-%d %H:%M").to_string();
        if matches!(c, b'X' | b'Y') {
            let last = self.state.msgs.newest_from(&area, self.user.id).ok().flatten();
            return match (c, last) {
                (b'X', Some(m)) => m.to.name.clone(),
                (b'X', None) => "nobody yet".into(),
                (_, m) => ago(m.map(|m| m.posted.date()), today),
            };
        }
        let mail = self.mailbox().unwrap_or_default();
        let seen = self.state.msgs.seen(self.user.id, &area).unwrap_or_default();
        let read = mail.iter().filter(|m| seen.contains(&m.id)).count();
        let newest = mail.iter().max_by_key(|m| (m.posted, m.id));
        let from = |m: &crate::msgbase::Message| if m.anonymous { "anonymous".to_string() } else { m.from.name.clone() };
        match c {
            b'T' => mail.len().to_string(),
            b'N' => (mail.len() - read).to_string(),
            b'R' => read.to_string(),
            b'F' => newest.map_or_else(|| "nobody yet".into(), from),
            b'S' => newest.map_or_else(|| "no mail yet".into(), |m| m.subject.clone()),
            b'D' => ago(newest.map(|m| m.posted.date()), today),
            b'A' => newest.map(when).unwrap_or_default(),
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
            [b'E', c] if EMAIL_STATS.contains(c) => self.email_stat(*c),
            [b'G', c] if crate::doorstats::DOOR_STATS.contains(c) => self.door_stat(*c),
            b"MI" => u.cur_msg_area.to_string(),
            b"MP" => self.menu.menu.as_ref().map(|m| m.prompt_name.clone()).unwrap_or_default(),
            b"MW" => self.mail_waiting().to_string(),
            b"MA" => self.current_area().map(|a| a.area.name).unwrap_or_default(),
            b"MN" => self.menu_msg.to_string(),
            b"MT" => self.current_area().map_or(0, |a| self.state.msgs.count(&a.area.file).unwrap_or(0)).to_string(),
            // New: the caller's own posts in the area.
            b"MU" => self.current_area().map_or(0, |a| self.state.msgs.count_from(&a.area.file, u.id).unwrap_or(0)).to_string(),
            b"CM" => self.state.areas().conf(u.conf_msg).map(|c| c.desc.clone()).unwrap_or_default(),
            b"ND" => self.node.number().to_string(),
            b"PA" => yes_no(u.pause),
            b"PL" => u.page_length.to_string(),
            b"PN" => u.phone.clone(),
            // The original printed the password; never do that.
            b"PW" => String::new(),
            b"RN" => u.real_name.clone(),
            // New: a rumor at random, and the newest (`rumors.rs`).
            b"RM" => self.rumor_code(false),
            b"RL" => self.rumor_code(true),
            b"RT" => self.caller.term.clone(),
            b"SL" => u.sl.to_string(),
            b"SN" => cfg.sysop_alias.clone(),
            b"SO" => u.sysop_note.clone(),
            b"SR" => cfg.sysop_name.clone(),
            b"SX" => crate::user::gender_name(u.sex).into(),
            // 2.20: minutes in the time bank.
            b"TB" => u.bank_time.to_string(),
            // New: tarot readings and fortunes left today (∞: no limit).
            #[cfg(not(feature = "tarot"))]
            b"TR" | b"TF" => "0".into(),
            #[cfg(feature = "tarot")]
            b"TR" | b"TF" => {
                let ai = &self.state.config().ai;
                let (kind, limit) = if code == *b"TR" { ("tarot", ai.tarot_daily) } else { ("fortune", ai.fortune_daily) };
                self.tarot_left(kind, limit).map_or("∞".into(), |n| n.to_string())
            }
            // New: ANSI art gens: free ones left today (F), bonus ones banked (B).
            #[cfg(feature = "ansigen")]
            b"IF" | b"IB" => self.ansigen_code(code[1]),
            #[cfg(not(feature = "ansigen"))]
            b"IF" | b"IB" => "0".into(),
            b"TI" => now.format("%-I:%M%P").to_string(),
            b"TL" => self.minutes_left().to_string(),
            b"TM" => now.format("%H:%M").to_string(),
            b"TT" => u.time_per_day.to_string(),
            b"TU" => self.state.users.count().unwrap_or(0).to_string(),
            // New: how the caller compares to everyone, by calls (C),
            // posts (P), uploads (U), downloads (D) and file points (F):
            // R their rank ("3rd of 41"), O a ten-cell bar of it, K the
            // share they're in ("top 8%"). (%G is the door stats'.)
            [kind @ (b'R' | b'O' | b'K'), what @ (b'C' | b'P' | b'U' | b'D' | b'F')] => {
                let (rank, n) = self.rank_among_users(*what);
                match kind {
                    b'R' => format!("{} of {n}", ordinal(rank)),
                    b'O' => rank_bar(rank, n),
                    _ => format!("top {}%", (rank * 100).div_ceil(n.max(1)).max(1)),
                }
            }
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
        // Found by fuzzing (debug builds): the bar overflowed at the ends.
        let lit = |s: String| s.chars().filter(|&c| c == '█').count();
        assert_eq!((lit(rank_bar(1, usize::MAX)), lit(rank_bar(usize::MAX, usize::MAX)), lit(rank_bar(1, 1)), lit(rank_bar(3, 4))), (10, 1, 10, 5));
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

    /// New: the email codes count what's in the box, read or not, and
    /// name the newest and the caller's own last.
    #[test]
    fn email_codes() {
        use crate::msgbase::{Message, Party};
        let me = crate::user::User { id: 2, handle: "Ann".into(), ..crate::user::User::default() };
        let s = crate::session::Session::for_test(crate::tests::state(), me);
        let area = s.state.areas().email.file.clone();
        let at = |d| chrono::NaiveDate::from_ymd_opt(2026, 10, d).unwrap().and_hms_opt(9, 30, 0).unwrap();
        let mail = |from: u32, fname: &str, to: u32, tname: &str, subject: &str, d| Message {
            id: 0, area: area.clone(), num: 0,
            from: Party { user_id: from, alias: fname.into(), name: fname.into(), ..Party::default() },
            to: Party { user_id: to, alias: tname.into(), name: tname.into(), ..Party::default() },
            subject: subject.into(), posted: at(d), deleted: false, private: true, anonymous: false,
            replies: 0, sig_pos: 0, tag: 0, reply_to: 0, lines: vec!["hi".into()],
        };
        let code = |c: &str| s.mci_value(c.as_bytes().try_into().unwrap()).unwrap();
        assert_eq!((code("ET"), code("EN"), code("EF"), code("EX"), code("EA")), ("0".into(), "0".into(), "nobody yet".into(), "nobody yet".into(), String::new()));
        let mut first = mail(1, "Bob", 2, "Ann", "lunch", 3);
        s.state.msgs.add(&mut first).unwrap();
        s.state.msgs.add(&mut mail(3, "Cy", 2, "Ann", "files", 5)).unwrap();
        s.state.msgs.add(&mut mail(2, "Ann", 1, "Bob", "re: lunch", 4)).unwrap();
        s.state.msgs.mark_seen(2, first.id).unwrap();
        assert_eq!((code("ET"), code("EN"), code("ER")), ("2".into(), "1".into(), "1".into()));
        assert_eq!((code("EF"), code("ES"), code("EA")), ("Cy".into(), "files".into(), "2026-10-05 09:30".into()));
        assert_eq!(code("EX"), "Bob");
    }
}

#[cfg(test)]
pub(crate) mod fuzz {
    use super::*;
    use crate::fuzzkit::{Rng, Target};
    use crate::user::User;

    /// A date anywhere chrono has them, often near the ends.
    fn date(rng: &mut Rng) -> Option<chrono::NaiveDate> {
        match rng.below(5) {
            0 => None,
            1 => Some(chrono::NaiveDate::MIN),
            2 => Some(chrono::NaiveDate::MAX),
            _ => chrono::NaiveDate::from_num_days_from_ce_opt(rng.next() as i32 % 4_000_000),
        }
    }

    /// A caller with odd text (from `input`) and boundary numbers.
    pub(crate) fn user(rng: &mut Rng, input: &[u8]) -> User {
        let text = |rng: &mut Rng| {
            let a = rng.below(input.len() + 1);
            let b = a + rng.below(input.len() - a + 1).min(200);
            fluxbbs_term::cp437::decode(&input[a..b])
        };
        let mut u = User { handle: text(rng), real_name: text(rng), location: text(rng), phone: text(rng), note: text(rng), sysop_note: text(rng), address: text(rng), ..User::default() };
        u.autosig = (0..rng.below(8)).map(|_| text(rng)).collect();
        u.birth_date = date(rng);
        u.first_call = date(rng);
        u.last_call = date(rng);
        u.file_scan = date(rng);
        u.sex = rng.next() as u8 as char;
        u.level = rng.next() as u8 as char;
        u.sl = rng.next() as u8;
        u.dsl = rng.next() as u8;
        let mut n = || rng.number() as u32;
        (u.total_calls, u.calls_today, u.time_today, u.time_per_day, u.posts, u.email, u.uploads, u.downloads) = (n(), n(), n(), n(), n(), n(), n(), n());
        (u.file_points, u.post_call, u.limit_dl, u.limit_dl_kb, u.today_dl, u.today_dl_kb, u.uldl_ratio, u.kb_ratio) = (n(), n(), n(), n(), n(), n(), n(), n());
        (u.cur_msg_area, u.cur_file_area, u.conf_msg, u.conf_file, u.vote_yes, u.vote_no, u.bank_time, u.bank_kb) = (n(), n(), n(), n(), n(), n(), n(), n());
        u.page_length = rng.number() as u16;
        u.upload_kb = rng.number();
        u.download_kb = rng.number();
        u.flags = input.iter().take(26).map(|&b| (b'A' + b % 26) as char).collect();
        u
    }

    /// Every MCI code for callers with odd values, and the helpers on
    /// boundary numbers.
    #[test]
    #[ignore]
    fn fuzz_mci() {
        let mut s = Session::for_test(crate::tests::state(), User::default());
        let s = std::cell::RefCell::new(&mut s);
        Target::new("mci", &[b"Mike Fricker", b"\xdb\xb2|CS%UN\x1b[2J", b""]).max_len(1024).run(|input, rng| {
            let mut s = s.borrow_mut();
            s.user = user(rng, input);
            s.input_string = String::from_utf8_lossy(input).into_owned();
            for _ in 0..16 {
                let code = [*rng.pick(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ@0123456789"), *rng.pick(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")];
                let _ = s.mci_value(code);
            }
            let (a, b) = (rng.number() as usize, rng.number() as usize);
            let _ = (ordinal(a), rank_bar(a, b), ratio(a as u32, b as u32), initials(&s.user.handle));
            if let (Some(d), Some(t)) = (date(rng), date(rng)) {
                let _ = ago(Some(d), t);
            }
            let _ = blurb(&s.user.autosig);
        });
    }
}
