//! `*U`: the user editor (CONFIG8), with its colour and preference
//! sub-screens, and `*L`: the access level editor (CONFIG6). Changes are
//! saved when the SysOp moves to another user, and on leaving if they say
//! so.

use std::io;

use chrono::NaiveDate;
use iniquity_term::{Palette, Slot};

use crate::cfgedit::{Field, Form, color_name, flags_text, yes_no};
use crate::config::Level;
use crate::input::Case;
use crate::session::Session;
use crate::user::{User, fmt_date, parse_date};

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

type Get<T> = fn(&User) -> T;
type Set<T> = fn(&mut User, T);

/// How a plain field is edited.
enum Kind {
    Text(Get<String>, Set<String>, Field),
    /// Getter, setter, min, max, and a suffix shown after the number.
    Num(Get<u64>, Set<u64>, u64, u64, &'static str),
    Date(Get<Option<NaiveDate>>, Set<Option<NaiveDate>>),
}

/// One field on the editor screen: key, label, key column, row, value
/// column, width, help text.
struct Spec {
    key: char,
    label: &'static str,
    x: u16,
    y: u16,
    vx: u16,
    len: usize,
    info: &'static str,
    kind: Kind,
}

// Columns from the original: two wide columns (a), three narrow (b).
const A1: (u16, u16) = (1, 17);
const A2: (u16, u16) = (40, 57);
const B1: (u16, u16) = (1, 17);
const B2: (u16, u16) = (26, 43);
const B3: (u16, u16) = (52, 69);

fn spec(key: char, label: &'static str, col: (u16, u16), y: u16, len: usize, info: &'static str, kind: Kind) -> Spec {
    Spec { key, label, x: col.0, y, vx: col.1, len, info, kind }
}

fn num(get: Get<u64>, set: Set<u64>, max: u64) -> Kind {
    Kind::Num(get, set, 0, max, "")
}

/// The fields that need nothing special.
fn plain_fields() -> Vec<Spec> {
    use Kind::*;
    let text = Field::TEXT;
    vec![
        spec('C', "Real name", A1, 3, 22, "User's real first and last name.", Text(|u| u.real_name.clone(), |u, v| u.real_name = v, text.max(36))),
        spec('D', "Phone number", A2, 3, 13, "User's voice phone number.", Text(|u| u.phone.clone(), |u, v| u.phone = v, text.max(13))),
        spec('F', "Address", A1, 4, 22, "User's street address.", Text(|u| u.address.clone(), |u, v| u.address = v, text.max(36))),
        spec('E', "Location", A2, 4, 22, "User's location [format: city, state].", Text(|u| u.location.clone(), |u, v| u.location = v, text.max(40))),
        spec('H', "SysOp note", A1, 5, 22, "SysOp note/comment for this user.", Text(|u| u.sysop_note.clone(), |u, v| u.sysop_note = v, text.max(40))),
        spec('G', "Usernote", A2, 5, 22, "Usernote for this user.", Text(|u| u.note.clone(), |u, v| u.note = v, text.max(40))),
        spec('#', "Zip code", B3, 6, 10, "This user's zip/postal code.", Text(|u| u.zip_code.clone(), |u, v| u.zip_code = v, Field::UPPER.max(10))),
        spec('I', "Birthdate", B1, 7, 8, "User's date of birth [format: mm/dd/yy].", Date(|u| u.birth_date, |u, v| u.birth_date = v)),
        spec('M', "SL", B2, 8, 3, "User's security level (SL).", num(|u| u.sl as u64, |u, v| u.sl = v as u8, 255)),
        spec('N', "DSL", B3, 8, 3, "User's download (file) security level.", num(|u| u.dsl as u64, |u, v| u.dsl = v as u8, 255)),
        spec('O', "Msg area", B1, 9, 5, "User's current message area number.", Kind::Num(|u| u.cur_msg_area as u64, |u, v| u.cur_msg_area = v as u32, 1, 65535, "")),
        spec('P', "File area", B2, 9, 5, "User's current file area number.", Kind::Num(|u| u.cur_file_area as u64, |u, v| u.cur_file_area = v as u32, 1, 65535, "")),
        spec('Q', "File pts", B3, 9, 7, "Number of file points user has.", num(|u| u.file_points as u64, |u, v| u.file_points = v as u32, 9_999_999)),
        spec('R', "Calls", B1, 10, 7, "User's total number of calls to the system.", num(|u| u.total_calls as u64, |u, v| u.total_calls = v as u32, 9_999_999)),
        spec('S', "Last call", B2, 10, 8, "Date of user's last call to the system.", Date(|u| u.last_call, |u, v| u.last_call = v)),
        spec('T', "First call", B3, 10, 8, "Date of this user's first call to the system.", Date(|u| u.first_call, |u, v| u.first_call = v)),
        spec('U', "Start menu", B1, 11, 8, "Starting menu for this user (blank for the default).", Text(|u| u.start_menu.clone(), |u, v| u.start_menu = v, Field::KEY.max(8))),
        spec('V', "Page length", B2, 11, 3, "User's terminal screen size in rows.", Kind::Num(|u| u.page_length as u64, |u, v| u.page_length = v as u16, 4, 200, "")),
        spec('X', "Time today", B1, 12, 5, "User's time remaining today (minutes).", num(|u| u.time_today as u64, |u, v| u.time_today = v as u32, 99_999)),
        spec('Y', "Time p/day", B2, 12, 5, "User's daily online time (minutes).", num(|u| u.time_per_day as u64, |u, v| u.time_per_day = v as u32, 99_999)),
        spec('Z', "Calls today", B3, 12, 5, "Number of times user has called today.", num(|u| u.calls_today as u64, |u, v| u.calls_today = v as u32, 99_999)),
        spec('1', "Posts", B1, 13, 7, "User's total number of public messages posted.", num(|u| u.posts as u64, |u, v| u.posts = v as u32, 9_999_999)),
        spec('2', "Email", B2, 13, 7, "User's total number of private messages sent.", num(|u| u.email as u64, |u, v| u.email = v as u32, 9_999_999)),
        spec('3', "Post/call %", B3, 13, 6, "Percent of calls for each post user can make to keep post:call ratio ok.", Kind::Num(|u| u.post_call as u64, |u, v| u.post_call = v as u32, 0, 99_999, "%")),
        spec('4', "Uploads", B1, 14, 7, "User's total number of files uploaded.", num(|u| u.uploads as u64, |u, v| u.uploads = v as u32, 9_999_999)),
        spec('5', "Downloads", B2, 14, 7, "User's total number of files downloaded.", num(|u| u.downloads as u64, |u, v| u.downloads = v as u32, 9_999_999)),
        spec('6', "Daily DL", B3, 14, 6, "Maximum number of files user may download in one day (0: no limit).", num(|u| u.limit_dl as u64, |u, v| u.limit_dl = v as u32, 99_999)),
        spec('7', "UL Kb", B1, 15, 9, "User's total number of kilobytes uploaded.", Kind::Num(|u| u.upload_kb, |u, v| u.upload_kb = v, 0, 99_999_999, "k")),
        spec('8', "DL Kb", B2, 15, 9, "User's total number of kilobytes downloaded.", Kind::Num(|u| u.download_kb, |u, v| u.download_kb = v, 0, 99_999_999, "k")),
        spec('9', "Daily Kb", B3, 15, 9, "Maximum number of kilobytes user may download in one day (0: no limit).", Kind::Num(|u| u.limit_dl_kb as u64, |u, v| u.limit_dl_kb = v as u32, 0, 9_999_999, "k")),
        spec('0', "UL/DL ratio", B1, 16, 5, "Number of files user can download per each upload (0: no ratio).", num(|u| u.uldl_ratio as u64, |u, v| u.uldl_ratio = v as u32, 99_999)),
        spec('!', "U/D Kb ratio", B2, 16, 6, "Number of kilobytes user can download per each kilobyte uploaded (0: no ratio).", Kind::Num(|u| u.kb_ratio as u64, |u, v| u.kb_ratio = v as u32, 0, 99_999, "k")),
        spec('%', "\"Yes\" votes", B1, 17, 3, "Number of \"yes\" votes this user currently has (new user voting).", num(|u| u.vote_yes as u64, |u, v| u.vote_yes = v as u32, 255)),
        spec('$', "\"No\" votes", B2, 17, 3, "Number of \"no\" votes this user currently has (new user voting).", num(|u| u.vote_no as u64, |u, v| u.vote_no = v as u32, 255)),
    ]
}

fn shown(kind: &Kind, u: &User) -> String {
    match kind {
        Kind::Text(get, ..) => get(u),
        Kind::Num(get, _, _, _, suffix) => format!("{}{suffix}", get(u)),
        Kind::Date(get, _) => fmt_date(get(u)),
    }
}

fn handle_shown(u: &User) -> String {
    if u.deleted { "[Deleted]".into() } else { u.handle.clone() }
}

const GENDERS: [&str; 2] = ["Male", "Female"];

/// The colour slots in the order the original listed them.
const COLOR_ITEMS: [(char, &str, Slot); 11] = [
    ('A', "Normal text", Slot::Text),
    ('B', "Dark text", Slot::TextLo),
    ('C', "Bright text", Slot::TextHi),
    ('D', "Normal info", Slot::Info),
    ('E', "Dark info", Slot::InfoLo),
    ('F', "Bright info", Slot::InfoHi),
    ('G', "Menu item", Slot::Item),
    ('H', "Selected item", Slot::ItemSel),
    ('I', "Line/border", Slot::Border),
    ('J', "Error/warning", Slot::Error),
    ('K', "Input color", Slot::Edit),
];

/// `levelUpgrade` / `levelLower`: the next defined level up (or down).
/// Unlike the original, this never wraps: lowering level A used to jump
/// to the highest level.
pub fn next_level(levels: impl Fn(char) -> bool, cur: char, up: bool) -> Option<char> {
    let cur = cur as u8;
    if up {
        (cur + 1..=b'Z').map(char::from).find(|&c| levels(c))
    } else {
        (b'A'..cur).rev().map(char::from).find(|&c| levels(c))
    }
}

/// A level's number fields: key, label, width, suffix, help, accessor.
type LevelNum = (char, &'static str, usize, &'static str, &'static str, fn(&mut Level) -> &mut u16);

const LEVEL_NUMS: [LevelNum; 7] = [
    ('E', "Time limit", 5, "", "Daily time limit (minutes) for users at this access level.", |l| &mut l.time_limit),
    ('F', "Post/call ratio", 6, "%", "Default post/call ratio for users with this access.", |l| &mut l.post_call),
    ('G', "File points", 5, "", "File points for users at this access level.", |l| &mut l.file_points),
    ('H', "Download limit", 5, "", "Allowed number of downloads per day at this level (0: no limit).", |l| &mut l.limit_dl),
    ('I', "Download kb limit", 7, "kb", "Allowed number of downloaded kilobytes per day at this level (0: no limit).", |l| &mut l.limit_dl_kb),
    ('J', "UL/DL ratio", 5, "", "Allowed number of downloads for each file uploaded (0: no ratio).", |l| &mut l.uldl_ratio),
    ('K', "UL/DL kb ratio", 5, "", "Allowed number of downloadable kilobytes for every kb uploaded (0: no ratio).", |l| &mut l.kb_ratio),
];

impl Session {
    /// `cfgLevelEditor`: levels A-Z; `[` and `]` step through them.
    pub async fn level_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: access level edit", self.user.handle);
        let original = (*self.state.levels()).clone();
        let mut levels = original.clone();
        let mut c = 'A';
        loop {
            let mut l = levels.0.get(&c.to_string()).cloned().unwrap_or_default();
            let mut f = Form::new(&format!("{} Access Level Editor", self.state.config().bbs.name));
            f.label("Current level", 1, c.to_string());
            f.item('A', "Description", 40, &l.desc, "Text description of this access level (blank: not in use).");
            f.item('B', "Security level", 3, l.sl.to_string(), "Security level (SL) for this access level.");
            f.item('C', "File security level", 3, l.dsl.to_string(), "File (download) security level (DSL) for this access level.");
            f.item('D', "User note", 40, &l.note, "Default user note given to users validated at this level.");
            for (key, label, len, suffix, info, get) in &LEVEL_NUMS {
                f.item(*key, label, *len, format!("{}{suffix}", get(&mut l)), info);
            }
            f.item('=', "Update users", 0, "", "Give every user at this level the limits above.");
            f.item('[', "Previous level", 0, "", "");
            f.item(']', "Next level", 0, "", "");
            self.cfg_draw(&f);
            let step: i32 = loop {
                let Some(key) = self.cfg_command(&mut f).await? else { break 0 };
                match key {
                    'A' | 'D' => {
                        if let Some(v) = self.cfg_text(&mut f, key, Field::TEXT).await? {
                            *if key == 'A' { &mut l.desc } else { &mut l.note } = v;
                        }
                    }
                    'B' | 'C' => {
                        if let Some(v) = self.cfg_number(&mut f, key, 0, 255).await? {
                            *if key == 'B' { &mut l.sl } else { &mut l.dsl } = v as u8;
                        }
                    }
                    '=' => {
                        levels.0.insert(c.to_string(), l.clone());
                        if l.desc.is_empty() {
                            self.cfg_info(&mut f, "Give the level a description first.");
                        } else if self.cfg_ask(&mut f, &format!("Update all level {c} users?"), false).await? {
                            let n = self.update_level_users(c, &l)?;
                            self.cfg_info(&mut f, &format!("{n} user(s) updated."));
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    k => {
                        let Some((_, _, _, suffix, _, get)) = LEVEL_NUMS.iter().find(|n| n.0 == k) else { continue };
                        f.set(k, get(&mut l).to_string());
                        let min = if k == 'E' { 1 } else { 0 };
                        if let Some(v) = self.cfg_number(&mut f, k, min, u16::MAX as u64).await? {
                            *get(&mut l) = v as u16;
                        }
                        self.cfg_set(&mut f, k, format!("{}{suffix}", get(&mut l)));
                    }
                }
            };
            if l != Level::default() || levels.0.contains_key(&c.to_string()) {
                levels.0.insert(c.to_string(), l);
            }
            if step == 0 {
                break;
            }
            c = (b'A' + ((c as u8 - b'A') as i32 + step).rem_euclid(26) as u8) as char;
        }
        self.screen.goto(1, self.screen.rows());
        self.newline();
        if levels != original {
            self.show("|U1Save access levels? ").await?;
            if self.yes_no(true).await? {
                self.state.set_levels(levels).map_err(io_err)?;
            }
        }
        Ok(())
    }

    /// `=` in the level editor: `levelSet` on every user at a level.
    fn update_level_users(&mut self, key: char, level: &Level) -> io::Result<u32> {
        self.save_user();
        let mut n = 0;
        for mut u in self.state.users.list().map_err(io_err)? {
            if u.level == key {
                u.apply_level(key, level);
                self.state.users.save(&u).map_err(io_err)?;
                if u.id == self.user.id {
                    self.user = u;
                }
                n += 1;
            }
        }
        tracing::info!("{}: applied level {key} to {n} users", self.user.handle);
        Ok(n)
    }

    /// `cfgUserEditor`: `param` may name a user to start with.
    pub async fn user_editor(&mut self, param: &str) -> io::Result<()> {
        // Our own record is live in the session: save it so the editor
        // starts from what's current.
        self.save_user();
        let first = if param.trim().is_empty() {
            self.user.id
        } else {
            self.state.users.find(param, true).map_err(io_err)?.map_or(self.user.id, |u| u.id)
        };
        tracing::info!("{}: user edit", self.user.handle);
        let mut id = first;
        let mut note = String::new();
        let fields = plain_fields();
        'users: loop {
            let ids: Vec<u32> = self.state.users.list().map_err(io_err)?.iter().map(|u| u.id).collect();
            if ids.is_empty() {
                return Ok(());
            }
            let pos = ids.iter().position(|&i| i == id).unwrap_or(0);
            id = ids[pos];
            let Some(mut u) = self.state.users.get(id).map_err(io_err)? else { return Ok(()) };
            let original = u.clone();
            let online = self.state.nodes.online().into_iter().find(|(n, info)| *n != self.node.number() && info.user.eq_ignore_ascii_case(&u.handle));
            if let Some((n, _)) = online {
                note = format!("{} is online on node {n}; their session may overwrite changes.", u.handle);
            }
            loop {
                let mut f = Form::new(&format!("{} User Editor", self.state.config().bbs.name));
                f.item_xy(B1.0, 1, B1.1, '/', "Current #", 14, format!("{} of {}", pos + 1, ids.len()), "");
                f.item_xy(40, 1, 1, '[', "Previous", 0, "", "");
                f.item_xy(59, 1, 1, ']', "Next", 0, "", "");
                f.item_xy(A1.0, 2, A1.1, 'A', "Handle", 22, handle_shown(&u), "User's handle or alias.");
                let pw = if u.password_hash.is_empty() { "(none)" } else { "(set)" };
                f.item_xy(A2.0, 2, A2.1, 'B', "Password", 22, pw, "Type a new password to change it (blank keeps the old one).");
                f.item_xy(A1.0, 6, A1.1, '=', "User flags", 28, flags_text(&u.flags), "User's flags.");
                f.item_xy(B2.0, 7, B2.1, 'J', "Gender", 6, GENDERS[(u.sex == 'F') as usize], "User's sex.");
                f.item_xy(B1.0, 8, B1.1, 'L', "Level ID", 1, u.level.to_string(), "User's access level (A-Z).");
                for s in &fields {
                    f.item_xy(s.x, s.y, s.vx, s.key, s.label, s.len, shown(&s.kind, &u), s.info);
                }
                f.item_xy(1, 18, 1, '-', "Edit colors", 0, "", "");
                f.item_xy(20, 18, 1, '+', "Autosignature", 0, "", "");
                f.item_xy(40, 18, 1, '?', "Edit user prefs", 0, "", "");
                f.item_xy(60, 18, 1, '>', "Upgrade access", 0, "", "");
                f.item_xy(1, 19, 1, '&', "View infoforms", 0, "", "");
                f.item_xy(20, 19, 1, '~', "Clear infoforms", 0, "", "");
                f.item_xy(40, 19, 1, '*', if u.deleted { "Undelete user" } else { "Delete user" }, 0, "", "");
                f.item_xy(60, 19, 1, '<', "Lower access", 0, "", "");
                self.cfg_draw(&f);
                if !note.is_empty() {
                    let n = std::mem::take(&mut note);
                    self.cfg_info(&mut f, &n);
                }
                loop {
                    let Some(key) = self.cfg_command(&mut f).await? else {
                        if u != original && self.cfg_ask(&mut f, "Save user?", true).await? {
                            self.store_user(&u)?;
                        }
                        self.screen.goto(1, self.screen.rows());
                        self.newline();
                        return Ok(());
                    };
                    match key {
                        '[' | ']' | '/' => {
                            let target = match key {
                                '[' => Some(ids[(pos + ids.len() - 1) % ids.len()]),
                                ']' => Some(ids[(pos + 1) % ids.len()]),
                                _ => {
                                    let want = self.cfg_read_info(&mut f, "User to edit (name/number): ", 36).await?;
                                    if want.is_empty() {
                                        continue;
                                    }
                                    let found = self.state.users.find(&want, true).map_err(io_err)?;
                                    if found.is_none() {
                                        self.cfg_info(&mut f, &format!("No user {want}."));
                                    }
                                    found.map(|x| x.id)
                                }
                            };
                            let Some(target) = target else { continue };
                            if u != original {
                                self.store_user(&u)?;
                            }
                            id = target;
                            continue 'users;
                        }
                        'A' => {
                            if u.deleted {
                                self.cfg_info(&mut f, "Undelete the user to change their handle.");
                                continue;
                            }
                            let Some(v) = self.cfg_text(&mut f, key, Field::TEXT.max(36)).await? else { continue };
                            let taken = self.state.users.find(&v, true).map_err(io_err)?.is_some_and(|o| o.id != u.id);
                            if v.is_empty() || taken {
                                if taken {
                                    self.cfg_info(&mut f, "Another user already has that name.");
                                }
                                self.cfg_set(&mut f, key, &u.handle);
                            } else {
                                u.handle = v;
                            }
                        }
                        'B' => {
                            f.set(key, "");
                            match self.cfg_text(&mut f, key, Field::UPPER.max(20)).await? {
                                Some(v) if !v.is_empty() => {
                                    u.set_password(&v);
                                    self.cfg_set(&mut f, key, "(changed)");
                                }
                                _ => self.cfg_set(&mut f, key, pw),
                            }
                        }
                        '=' => {
                            let mut flags = u.flags.clone();
                            self.cfg_flags(&mut f, key, &mut flags).await?;
                            u.flags = flags;
                        }
                        'J' => {
                            let pick = self.cfg_option(&mut f, key, &GENDERS, (u.sex == 'F') as usize).await?;
                            u.sex = if pick == 1 { 'F' } else { 'M' };
                        }
                        'L' => {
                            let field = Field { case: Case::Upper, allowed: "ABCDEFGHIJKLMNOPQRSTUVWXYZ", max: 1 };
                            let Some(v) = self.cfg_text(&mut f, key, field).await? else { continue };
                            let Some(c) = v.chars().next() else {
                                self.cfg_set(&mut f, key, u.level.to_string());
                                continue;
                            };
                            u.level = c;
                            let defined = self.state.level(c);
                            if let Some(level) = defined
                                && self.cfg_ask(&mut f, &format!("Apply level {c}'s limits ({})?", level.desc), true).await?
                            {
                                u.apply_level(c, &level);
                                break;
                            }
                        }
                        '>' | '<' => {
                            let levels = self.state.levels();
                            match next_level(|c| levels.get(c).is_some(), u.level, key == '>') {
                                Some(c) => {
                                    let level = levels.get(c).cloned().expect("level exists");
                                    u.apply_level(c, &level);
                                    note = format!("Now level {c}: {}.", level.desc);
                                }
                                None => note = "No other access levels are defined.".into(),
                            }
                            break;
                        }
                        '*' => {
                            u.deleted = !u.deleted;
                            break;
                        }
                        '-' => {
                            self.palette_editor(&mut u.palette).await?;
                            break;
                        }
                        '&' | '~' => {
                            let forms = self.state.infoforms().list.clone();
                            if forms.is_empty() {
                                self.cfg_info(&mut f, "There are no infoforms.");
                                continue;
                            }
                            let verb = if key == '&' { "view" } else { "clear" };
                            let q = format!("Infoform to {verb} [1-{}, = for all]: ", forms.len());
                            let pick = self.cfg_read_info(&mut f, &q, 3).await?;
                            let which: Vec<_> = if pick == "=" {
                                forms.iter().collect()
                            } else {
                                pick.parse::<usize>().ok().and_then(|n| forms.get(n.wrapping_sub(1))).into_iter().collect()
                            };
                            if which.is_empty() {
                                continue;
                            }
                            if key == '~' {
                                for form in &which {
                                    self.state.forms.clear(Some(&form.file), u.id).map_err(io_err)?;
                                }
                                self.cfg_info(&mut f, &format!("Cleared {} infoform(s).", which.len()));
                                continue;
                            }
                            let mut shown = false;
                            for form in which.into_iter().cloned().collect::<Vec<_>>() {
                                shown |= self.show_form(&form, u.id).await?;
                            }
                            if shown {
                                break;
                            }
                            self.cfg_info(&mut f, "They haven't filled that in.");
                        }
                        '?' => {
                            self.prefs_editor(&mut u).await?;
                            break;
                        }
                        '+' => {
                            let (handle, sig) = (u.handle.clone(), u.autosig.clone());
                            if let Some(sig) = self.sig_editor(&handle, sig).await? {
                                u.autosig = sig;
                            }
                            break;
                        }
                        k => {
                            let Some(s) = fields.iter().find(|s| s.key == k) else { continue };
                            match &s.kind {
                                Kind::Text(get, set, field) => match self.cfg_text(&mut f, k, *field).await? {
                                    Some(v) => set(&mut u, v),
                                    None => self.cfg_set(&mut f, k, get(&u)),
                                },
                                Kind::Num(get, set, min, max, suffix) => {
                                    f.set(k, get(&u).to_string());
                                    if let Some(n) = self.cfg_number(&mut f, k, *min, *max).await? {
                                        set(&mut u, n);
                                    }
                                    self.cfg_set(&mut f, k, format!("{}{suffix}", get(&u)));
                                }
                                Kind::Date(get, set) => {
                                    if let Some(v) = self.cfg_text(&mut f, k, Field::TEXT.max(8)).await? {
                                        match parse_date(&v) {
                                            Some(d) => set(&mut u, Some(d)),
                                            None if v.is_empty() => set(&mut u, None),
                                            None => {}
                                        }
                                    }
                                    self.cfg_set(&mut f, k, fmt_date(get(&u)));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Save an edited user. Our own record also lives in the session, so
    /// keep that in step.
    fn store_user(&mut self, u: &User) -> io::Result<()> {
        self.state.users.save(u).map_err(io_err)?;
        if u.id == self.user.id {
            self.user = u.clone();
        }
        tracing::info!("{}: saved user #{} ({})", self.user.handle, u.id, u.handle);
        Ok(())
    }

    /// `cfgUserColors`.
    pub(crate) async fn palette_editor(&mut self, pal: &mut Palette) -> io::Result<()> {
        loop {
            let mut f = Form::new("User Color Configuration");
            for (key, label, slot) in COLOR_ITEMS {
                f.item(key, label, 40, color_name(pal.get(slot)), "");
            }
            f.gap(1);
            f.item('1', "Set to defaults", 0, "", "");
            self.cfg_draw(&f);
            for (key, _, slot) in COLOR_ITEMS {
                self.cfg_paint(&f, key, pal.get(slot));
            }
            loop {
                let Some(key) = self.cfg_command(&mut f).await? else { return Ok(()) };
                if key == '1' {
                    *pal = Palette::default();
                    break;
                }
                let Some(&(_, _, slot)) = COLOR_ITEMS.iter().find(|(k, ..)| *k == key) else { continue };
                pal.0[slot as usize] = self.cfg_color(&mut f, key, pal.get(slot)).await?;
            }
        }
    }

    /// `cfgUserPrefs`.
    async fn prefs_editor(&mut self, u: &mut User) -> io::Result<()> {
        type Pref = (char, &'static str, &'static str, fn(&mut User) -> &mut bool);
        let prefs: [Pref; 5] = [
            ('1', "Yes/No bars", "Does user have Yes/No selection bars activated?", |u| &mut u.yes_no_bars),
            ('2', "Expert mode", "Does user have \"expert\" mode enabled?", |u| &mut u.expert),
            ('3', "Hot keys", "Does user have \"hot key\" menu entry mode activated?", |u| &mut u.hot_keys),
            ('4', "Screen pausing", "Does user have screen pausing enabled?", |u| &mut u.pause),
            ('5', "Autoquote", "Does user want message autoquoting?", |u| &mut u.quote),
        ];
        let mut f = Form::new("User Preference Editor");
        f.value_col(22);
        for (key, label, info, get) in prefs {
            f.item(key, label, 3, yes_no(*get(u)), info);
        }
        self.cfg_draw(&f);
        while let Some(key) = self.cfg_command(&mut f).await? {
            if let Some((_, _, _, get)) = prefs.iter().find(|p| p.0 == key) {
                let cur = *get(u);
                *get(u) = self.cfg_bool(&mut f, key, cur).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_step_over_gaps() {
        let defined = |c: char| "ACE".contains(c);
        assert_eq!(next_level(defined, 'A', true), Some('C'));
        assert_eq!(next_level(defined, 'C', true), Some('E'));
        assert_eq!(next_level(defined, 'E', true), None, "the top level stays put");
        assert_eq!(next_level(defined, 'A', false), None, "the bottom level stays put");
        assert_eq!(next_level(defined, 'E', false), Some('C'));
        assert_eq!(next_level(defined, 'Z', true), None);
    }

    #[test]
    fn fields_have_unique_keys_and_fit() {
        let mut keys: Vec<char> = plain_fields().iter().map(|s| s.key).collect();
        keys.extend(['/', '[', ']', 'A', 'B', '=', 'J', 'L', '-', '+', '?', '>', '*', '<', '&', '~']);
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate hotkey");
        for s in plain_fields() {
            assert!(s.vx as usize + s.len <= 80, "{} runs off the screen", s.label);
        }
    }
}
