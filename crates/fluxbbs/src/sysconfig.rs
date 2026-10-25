//! `*C`: the system configuration editor (CONFIG1/CONFIG2). Each section
//! of fluxbbs.toml gets a screen; on leaving, only the settings that
//! changed are written back, so the file's comments and layout survive.
//! Most changes apply at once; SSH settings and paths need a restart.

use std::io;
use std::path::{Path, PathBuf};

use crate::cfgedit::{Field, Form, yes_no};
use crate::config::{CharsetPref, Config, SummaryAt};
use crate::session::Session;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// How a setting is edited.
enum Kind {
    Text(fn(&mut Config) -> &mut String, Field),
    Bool(fn(&mut Config) -> &mut bool),
    /// Getter, setter, min, max.
    Num(fn(&Config) -> u64, fn(&mut Config, u64), u64, u64),
    /// A comma-separated list.
    List(fn(&mut Config) -> &mut Vec<String>),
    Path(fn(&mut Config) -> &mut PathBuf),
    Char(fn(&mut Config) -> &mut char),
    /// A level letter, A-Z.
    Letter(fn(&mut Config) -> &mut char),
    /// Getter, setter, labels.
    Choice(fn(&Config) -> usize, fn(&mut Config, usize), &'static [&'static str]),
    /// Minutes and file points.
    Prize(fn(&mut Config) -> &mut crate::config::Prize),
}

fn prize_shown(p: &crate::config::Prize) -> String {
    format!("{} min, {} points", p.minutes, p.points)
}

/// "30 40", "30 min, 40 points": the first two numbers.
fn prize_typed(s: &str) -> Option<crate::config::Prize> {
    let mut n = s.split(|c: char| !c.is_ascii_digit()).filter(|w| !w.is_empty()).map(|w| w.parse::<u32>().ok().map(|v| v.min(9999)));
    Some(crate::config::Prize { minutes: n.next()??, points: n.next().flatten().unwrap_or(0) })
}

struct Setting {
    key: char,
    label: &'static str,
    len: usize,
    info: &'static str,
    kind: Kind,
}

const fn set(key: char, label: &'static str, len: usize, info: &'static str, kind: Kind) -> Setting {
    Setting { key, label, len, info, kind }
}

const TEXT: Field = Field::TEXT;
const ACS: Field = Field::LOWER;

const CHARSETS: [&str; 3] = ["Auto (by TERM)", "CP437 always", "UTF-8 always"];
const SUMMARY_AT: [&str; 3] = ["Off", "First", "Last"];
const ALIAS_FORMATS: [&str; 8] =
    ["As typed", "First letter up", "UPPER CASE", "lower case", "Each Word Up", "WeiRD (MiKe)", "wARPED (mIkE)", "CooL (FiEND)"];

fn shown(kind: &Kind, c: &mut Config) -> String {
    match kind {
        Kind::Text(get, _) => get(c).clone(),
        Kind::Bool(get) => yes_no(*get(c)).into(),
        Kind::Num(get, ..) => get(c).to_string(),
        Kind::List(get) => get(c).join(", "),
        Kind::Path(get) => get(c).display().to_string(),
        Kind::Char(get) | Kind::Letter(get) => get(c).to_string(),
        Kind::Choice(get, _, labels) => labels.get(get(c)).copied().unwrap_or("?").into(),
        Kind::Prize(get) => prize_shown(get(c)),
    }
}

/// The sections: title, whether they need a restart, and their settings.
fn sections() -> Vec<(&'static str, bool, Vec<Setting>)> {
    use Kind::*;
    vec![
        ("Main BBS Configuration", false, vec![
            set('A', "BBS name", 40, "Name of this BBS.", Text(|c| &mut c.bbs.name, TEXT)),
            set('B', "Location", 40, "Where the BBS is.", Text(|c| &mut c.bbs.location, TEXT)),
            set('C', "Address", 40, "How to reach it (host, or a phone number).", Text(|c| &mut c.bbs.phone, TEXT)),
            set('D', "SysOp alias", 36, "The SysOp's handle.", Text(|c| &mut c.bbs.sysop_alias, TEXT)),
            set('E', "SysOp name", 36, "The SysOp's real name.", Text(|c| &mut c.bbs.sysop_name, TEXT)),
            set('F', "Max nodes", 3, "Callers online at once (takes effect on restart).", Num(|c| c.bbs.max_nodes as u64, |c, v| c.bbs.max_nodes = v as u16, 1, 250)),
            set('G', "Idle timeout", 6, "Disconnect after this many idle seconds (0 = never).", Num(|c| c.bbs.inactivity_seconds, |c, v| c.bbs.inactivity_seconds = v, 0, 86_400)),
            set('H', "Time zone", 32, "Zone the board keeps time in (America/Chicago, EST, UTC-6); blank for the host's clock.", Text(|c| &mut c.bbs.time_zone, TEXT)),
        ]),
        ("SSH, Telnet and Terminal", true, vec![
            set('A', "Listen on", 40, "Address and port for SSH, e.g. 0.0.0.0:2222 (restart).", Text(|c| &mut c.ssh.listen, TEXT)),
            set('B', "Host key dir", 40, "Where the SSH host keys live (restart).", Path(|c| &mut c.ssh.host_key_dir)),
            set('C', "Old algorithms", 3, "Also offer older SSH algorithms (SyncTERM needs them; restart).", Bool(|c| &mut c.ssh.legacy_algorithms)),
            set('D', "Character set", 16, "CP437 for BBS terminals, UTF-8 for the rest.", Choice(
                |c| c.terminal.charset as usize,
                |c, v| c.terminal.charset = [CharsetPref::Auto, CharsetPref::Cp437, CharsetPref::Utf8][v],
                &CHARSETS,
            )),
            set('E', "CP437 TERMs", 50, "TERM values that get CP437 when the character set is Auto.", List(|c| &mut c.terminal.cp437_terms)),
            set('F', "24-bit TERMs", 50, "CP437 terminals sent bright backgrounds as 24-bit color, not the iCE blink bit.", List(|c| &mut c.terminal.truecolor_terms)),
            set('G', "Telnet", 3, "Also accept plain (unencrypted) telnet? (restart)", Bool(|c| &mut c.telnet.enabled)),
            set('H', "Telnet listen", 40, "Address and port for telnet, e.g. 0.0.0.0:2323 (restart).", Text(|c| &mut c.telnet.listen, TEXT)),
            set('I', "Screen saver", 5, "Seconds idle on a menu before the screen saver (0 = none); for callers whose menus animate.", Num(|c| c.terminal.screensaver_seconds, |c, v| c.terminal.screensaver_seconds = v, 0, 86_400)),
            set('J', "256-color TERMs", 50, "CP437 terminals with 256 colors but not 24-bit: sent the nearest of the 256.", List(|c| &mut c.terminal.palette256_terms)),
        ]),
        ("Access and Logon", false, vec![
            set('A', "SysOp ACS", 20, "ACS for SysOp access.", Text(|c| &mut c.acs.sysop, ACS)),
            set('B', "Co-SysOp ACS", 20, "ACS for co-SysOp access.", Text(|c| &mut c.acs.cosysop, ACS)),
            set('C', "Matrix menu", 3, "Show the MATRIX menu before logging in?", Bool(|c| &mut c.logon.matrix)),
            set('D', "Offer to apply", 3, "Offer the new-user application to unknown names?", Bool(|c| &mut c.logon.ask_apply)),
            set('E', "Login tries", 2, "Password attempts before hanging up.", Num(|c| c.logon.tries as u64, |c, v| c.logon.tries = v as u8, 1, 20)),
            set('F', "System password", 20, "Extra password asked of the users below (blank = none).", Text(|c| &mut c.logon.system_password, Field::UPPER)),
            set('G', "System PW ACS", 20, "Who is asked the system password.", Text(|c| &mut c.logon.system_password_acs, ACS)),
            set('H', "Start menu", 8, "Menu callers start in after logging in.", Text(|c| &mut c.logon.start_menu, Field::KEY.max(8))),
            set('I', "Real names", 3, "Are handles real names?", Bool(|c| &mut c.logon.real_names)),
            set('J', "Time per call", 3, "Reset time limits every call instead of every day?", Bool(|c| &mut c.logon.time_per_call)),
            set('K', "Last callers", 3, "How many calls the last-callers list shows.", Num(|c| c.logon.last_callers as u64, |c, v| c.logon.last_callers = v as u32, 1, 100)),
            set('L', "Birth date check", 3, "Ask the birth date again every this many calls (0 = never).", Num(|c| c.logon.calls_birth as u64, |c, v| c.logon.calls_birth = v as u8, 0, 255)),
            set('M', "Phone check", 3, "Ask the last four phone digits every this many calls (0 = never).", Num(|c| c.logon.calls_phone as u64, |c, v| c.logon.calls_phone = v as u8, 0, 255)),
        ]),
        ("New User Configuration", false, vec![
            set('A', "Apply password", 20, "Password needed to apply (blank = none).", Text(|c| &mut c.newuser.password, Field::UPPER)),
            set('B', "Verify first", 3, "Ask \"proceed with application?\" first?", Bool(|c| &mut c.newuser.verify)),
            set('C', "Handle format", 16, "How new handles are capitalised.", Choice(
                |c| c.newuser.alias_format.saturating_sub(1) as usize,
                |c, v| c.newuser.alias_format = v as u8 + 1,
                &ALIAS_FORMATS,
            )),
            set('D', "Page length", 3, "Default screen length.", Num(|c| c.newuser.page_length as u64, |c, v| c.newuser.page_length = v as u16, 10, 200)),
            set('E', "Ask page length", 3, "Ask new users for their screen length?", Bool(|c| &mut c.newuser.ask_page_length)),
            set('F', "Hot keys", 3, "Hot keys on by default?", Bool(|c| &mut c.newuser.hot_keys)),
            set('G', "Ask hot keys", 3, "Ask new users about hot keys?", Bool(|c| &mut c.newuser.ask_hot_keys)),
            set('H', "Expert mode", 3, "Expert mode on by default?", Bool(|c| &mut c.newuser.expert)),
            set('I', "Ask expert", 3, "Ask new users about expert mode?", Bool(|c| &mut c.newuser.ask_expert)),
            set('J', "Pausing", 3, "Screen pausing on by default?", Bool(|c| &mut c.newuser.pause)),
            set('K', "Ask pausing", 3, "Ask new users about screen pausing?", Bool(|c| &mut c.newuser.ask_pause)),
            set('L', "Autoquote", 3, "Autoquote on by default?", Bool(|c| &mut c.newuser.quote)),
            set('M', "Ask autoquote", 3, "Ask new users about autoquoting?", Bool(|c| &mut c.newuser.ask_quote)),
            set('N', "Yes/no bars", 3, "Yes/no bars on by default?", Bool(|c| &mut c.newuser.yes_no_bars)),
            set('O', "Ask yes/no bars", 3, "Ask new users about yes/no bars?", Bool(|c| &mut c.newuser.ask_yes_no_bars)),
            set('P', "Config screen", 3, "Show the NEWCFG screen after applying, to check the entries?", Bool(|c| &mut c.newuser.config)),
        ]),
        ("Prompts", false, vec![
            set('A', "Yes prompt", 50, "Shown for a yes/no question that defaults to yes.", Text(|c| &mut c.prompts.yes, TEXT.max(200))),
            set('B', "No prompt", 50, "Shown for a yes/no question that defaults to no.", Text(|c| &mut c.prompts.no, TEXT.max(200))),
            set('C', "Yes word", 30, "Shown after answering yes.", Text(|c| &mut c.prompts.yes_word, TEXT.max(100))),
            set('D', "No word", 30, "Shown after answering no.", Text(|c| &mut c.prompts.no_word, TEXT.max(100))),
            set('E', "Yes bar", 50, "The yes/no bar with yes selected.", Text(|c| &mut c.prompts.yes_bar, TEXT.max(200))),
            set('F', "No bar", 50, "The yes/no bar with no selected.", Text(|c| &mut c.prompts.no_bar, TEXT.max(200))),
            set('G', "Password echo", 1, "Character shown for each typed password letter.", Char(|c| &mut c.prompts.password_echo)),
            set('H', "Erase pauses", 3, "Erase \"pause\" prompts after a key?", Bool(|c| &mut c.prompts.remove_pause)),
        ]),
        ("Security", false, vec![
            set('A', "Bot check", 3, "Make every caller press a random key combination first?", Bool(|c| &mut c.security.challenge)),
            set('B', "Keys to press", 1, "How many keys the bot check asks for.", Num(|c| c.security.challenge_keys as u64, |c, v| c.security.challenge_keys = v as u8, 1, 8)),
            set('C', "Seconds allowed", 3, "Time to finish the bot check.", Num(|c| c.security.challenge_seconds as u64, |c, v| c.security.challenge_seconds = v as u16, 5, 300)),
            set('D', "Tries", 2, "Goes at the bot check before hanging up.", Num(|c| c.security.challenge_tries as u64, |c, v| c.security.challenge_tries = v as u8, 1, 10)),
            set('E', "Per address", 3, "Connections from one address at once (0 = no limit).", Num(|c| c.security.max_per_ip as u64, |c, v| c.security.max_per_ip = v as u32, 0, 100)),
            set('F', "Per minute", 3, "New connections from one address per minute (0 = no limit).", Num(|c| c.security.max_per_minute as u64, |c, v| c.security.max_per_minute = v as u32, 0, 1000)),
            set('G', "Failures to ban", 3, "Failed bot checks or passwords before a ban (0 = never).", Num(|c| c.security.fail_limit as u64, |c, v| c.security.fail_limit = v as u32, 0, 100)),
            set('H', "Ban minutes", 5, "First ban's length; each later one doubles, up to a day.", Num(|c| c.security.ban_minutes as u64, |c, v| c.security.ban_minutes = v as u32, 1, 1440)),
            set('I', "Pre-login idle", 4, "Seconds a caller may sit idle before logging in.", Num(|c| c.security.prelogin_idle_seconds, |c, v| c.security.prelogin_idle_seconds = v, 10, 3600)),
            set('J', "Allow list", 50, "Never limited or checked: addresses, prefixes (10.0.) or ranges (/24).", List(|c| &mut c.security.allow)),
            set('K', "Deny list", 50, "Always refused: addresses, prefixes or ranges.", List(|c| &mut c.security.deny)),
            set('L', "Picture check", 3, "Bot check as three big characters drawn in color (BOTCHECK screen)? Else keys written out.", Bool(|c| &mut c.security.challenge_visual)),
        ]),
        ("Art Gallery", false, vec![
            set('A', "Gallery folder", 40, "Folder of art (relative to the config); ZIP packs in it open as folders.", Path(|c| &mut c.gallery.path)),
            set('B', "Or file area", 8, "Storage key of a file area to show instead of the folder (its sub-areas become folders); blank = the folder.", Text(|c| &mut c.gallery.area, Field::KEY)),
            set('C', "Browse ACS", 20, "Who may browse the gallery.", Text(|c| &mut c.gallery.acs, ACS)),
            set('D', "Download ACS", 20, "Who may download from it (files of a file area use that area's rules instead).", Text(|c| &mut c.gallery.download_acs, ACS)),
            set('E', "Play speed", 6, "Starting speed (bps) for playing art; 0 = all at once. Callers change it with S.", Num(|c| c.gallery.speed as u64, |c, v| c.gallery.speed = v as u32, 0, 1_000_000)),
        ]),
        ("Website Feed", true, vec![
            set('A', "Enabled", 3, "Serve who's online, last callers, new files and totals as JSON for a website? (restart)", Bool(|c| &mut c.web.enabled)),
            set('B', "Listen", 30, "Address for the feed; keep 127.0.0.1 and have the web server pass /api/status.json to it (restart).", Text(|c| &mut c.web.listen, TEXT)),
            set('C', "Allowed site", 40, "A site on another address allowed to fetch it (e.g. https://disfluxbbs.com); blank if proxied.", Text(|c| &mut c.web.allow_origin, TEXT.max(100))),
            set('D', "Show handles", 3, "Show callers' handles (else \"a caller\")?", Bool(|c| &mut c.web.show_handles)),
            set('E', "Show SysOp", 3, "List the SysOp among who's online?", Bool(|c| &mut c.web.show_sysop)),
            set('F', "Last callers", 2, "How many recent callers to list.", Num(|c| c.web.last_callers as u64, |c, v| c.web.last_callers = v as u8, 0, 50)),
            set('G', "New files", 2, "How many new files to list.", Num(|c| c.web.new_files as u64, |c, v| c.web.new_files = v as u8, 0, 50)),
        ]),
        ("Passwords and Sign-ups", false, vec![
            set('A', "Min. password length", 3, "Shortest password allowed.", Num(|c| c.passwords.min_length as u64, |c, v| c.passwords.min_length = v as u8, 1, 64)),
            set('B', "Refuse weak ones", 3, "Refuse common passwords, the user's handle, one character repeated?", Bool(|c| &mut c.passwords.refuse_common)),
            set('C', "Enforce at login", 3, "Make callers with a password that breaks the rules change it at login?", Bool(|c| &mut c.passwords.enforce_at_login)),
            set('D', "Sign-ups per address", 3, "New accounts from one address per day (0 = no limit).", Num(|c| c.newuser.per_address_per_day as u64, |c, v| c.newuser.per_address_per_day = v as u32, 0, 1000)),
            set('E', "Sign-ups per day", 4, "New accounts on the whole board per day (0 = no limit).", Num(|c| c.newuser.per_day as u64, |c, v| c.newuser.per_day = v as u32, 0, 10000)),
            set('F', "Reserved handles", 50, "Handles nobody may apply with.", List(|c| &mut c.newuser.reserved_handles)),
            set('G', "Purge after days", 4, "Delete level-A accounts that never called back after this many days (0 = never).", Num(|c| c.newuser.purge_after_days as u64, |c, v| c.newuser.purge_after_days = v as u32, 0, 3650)),
        ]),
        ("New User Questions", false, vec![
            set('A', "Real name", 3, "Ask for a real name? (If not, the handle is used.)", Bool(|c| &mut c.newuser.ask_real_name)),
            set('B', "Birth date", 3, "Ask for a date of birth? (ACS age checks need it.)", Bool(|c| &mut c.newuser.ask_birth_date)),
            set('C', "Phone number", 3, "Ask for a voice phone number?", Bool(|c| &mut c.newuser.ask_phone)),
            set('D', "Street address", 3, "Ask for a street address?", Bool(|c| &mut c.newuser.ask_address)),
            set('E', "Location", 3, "Ask for city, country/state and zip code?", Bool(|c| &mut c.newuser.ask_location)),
            set('F', "Gender", 3, "Ask for gender?", Bool(|c| &mut c.newuser.ask_sex)),
        ]),
        ("New User Voting", false, vec![
            set('A', "Use NUV", 3, "Do existing users vote new users in or out?", Bool(|c| &mut c.nuv.enabled)),
            set('B', "Yes votes", 3, "Yes votes that validate a new user.", Num(|c| c.nuv.votes_yes as u64, |c, v| c.nuv.votes_yes = v as u8, 1, 255)),
            set('C', "No votes", 3, "No votes that delete a new user.", Num(|c| c.nuv.votes_no as u64, |c, v| c.nuv.votes_no = v as u8, 1, 255)),
            set('D', "Candidate ACS", 20, "Who gets voted on (stock: !s50, everyone below SL 50).", Text(|c| &mut c.nuv.candidate_acs, ACS)),
            set('E', "Voter ACS", 20, "Who may vote.", Text(|c| &mut c.nuv.voter_acs, ACS)),
            set('F', "Show initials", 3, "Show voters' initials beside their comments?", Bool(|c| &mut c.nuv.initials)),
            set('G', "Validated level", 1, "Access level given to users voted in.", Letter(|c| &mut c.nuv.level)),
        ]),
        ("Message System", false, vec![
            set('A', "Email ACS", 20, "ACS to send email.", Text(|c| &mut c.messages.acs_post_email, ACS)),
            set('B', "Anonymous ACS", 20, "ACS to post anonymously.", Text(|c| &mut c.messages.acs_anonymous, ACS)),
            set('C', "Anon automsg ACS", 20, "ACS for an anonymous automessage.", Text(|c| &mut c.messages.acs_anon_automsg, ACS)),
            set('D', "Autosig ACS", 20, "ACS to use auto-signatures.", Text(|c| &mut c.messages.acs_autosig, ACS)),
            set('E', "Quit mandatory", 3, "May callers quit reading mandatory messages?", Bool(|c| &mut c.messages.abort_mandatory_ok)),
            set('F', "Ask private", 3, "Ask whether a post is private?", Bool(|c| &mut c.messages.ask_private)),
            set('G', "Ask private reply", 3, "Ask whether a reply is private?", Bool(|c| &mut c.messages.ask_private_reply)),
            set('H', "Post after read", 3, "Offer to post after reading an area's last message?", Bool(|c| &mut c.messages.ask_post_after_read)),
            set('I', "Delete replied", 3, "Offer to delete email after replying?", Bool(|c| &mut c.messages.ask_delete_replied)),
            set('J', "Delete all read", 3, "Offer to delete all read email on leaving the mail reader?", Bool(|c| &mut c.messages.ask_delete_all_read)),
            set('K', "Ask autoquote", 3, "Ask whether to quote when replying?", Bool(|c| &mut c.messages.ask_auto_quote)),
            set('L', "Default quote lines", 3, "Suggest line numbers when quoting by hand?", Bool(|c| &mut c.messages.default_quote_lines)),
            set('M', "Max quote lines", 3, "Most lines quoted automatically.", Num(|c| c.messages.max_quote_lines as u64, |c, v| c.messages.max_quote_lines = v as u8, 1, 99)),
            set('N', "Mandatory all confs", 3, "Should the mandatory scan ignore conferences?", Bool(|c| &mut c.messages.mandatory_all_confs)),
            set('O', "Compress numbers", 3, "Number listed areas 1, 2, 3... counting only visible ones?", Bool(|c| &mut c.messages.compress_area_numbers)),
            set('P', "ANSI quote text", 40, "Shown instead of quoted lines that contain ANSI.", Text(|c| &mut c.messages.ansi_quote, TEXT.max(100))),
        ]),
        ("Attachments and Uploaded Messages", false, vec![
            set('A', "Public attach ACS", 20, "ACS to attach a file to a public message.", Text(|c| &mut c.messages.acs_attach_public, ACS)),
            set('B', "Email attach ACS", 20, "ACS to attach a file to email.", Text(|c| &mut c.messages.acs_attach_email, ACS)),
            set('C', "Upload msg ACS", 20, "ACS to upload a prepared message instead of typing it.", Text(|c| &mut c.messages.acs_upload_message, ACS)),
            set('D', "Ask upload reply", 3, "Offer to upload a prepared message when replying?", Bool(|c| &mut c.messages.ask_upload_reply)),
            set('E', "Ask upload email", 3, "Offer to upload a prepared message for email?", Bool(|c| &mut c.messages.ask_upload_email)),
        ]),
        ("Caller Boards", false, vec![
            set('A', "Create ACS", 20, "Who may create a board of their own and moderate it (blank = nobody).", Text(|c| &mut c.messages.board_create_acs, ACS)),
            set('B', "Folder", 8, "Storage key of the message area callers' boards go under (blank = top level).", Text(|c| &mut c.messages.board_folder, TEXT.max(8))),
            set('C', "Boards per caller", 3, "Boards each caller may run.", Num(|c| c.messages.boards_per_caller as u64, |c, v| c.messages.boards_per_caller = v as u32, 1, 99)),
            set('D', "Empty after days", 3, "Remove a caller's board with no posts this many days after it was made (0 = never).", Num(|c| c.messages.board_empty_days as u64, |c, v| c.messages.board_empty_days = v as u32, 0, 999)),
            set('F', "Take-back minutes", 3, "Minutes in which an author may take a post back, while nobody has answered it (0 = never).", Num(|c| c.messages.take_back_minutes as u64, |c, v| c.messages.take_back_minutes = v as u32, 0, 1440)),
            set('G', "Burn points", 5, "File points an author gives up to delete a post for good (0 = free).", Num(|c| c.messages.burn_points as u64, |c, v| c.messages.burn_points = v as u32, 0, 99999)),
            set('E', "Points per fill", 5, "Points for filling a request on a request board (and file points, if used; 0 = none).", Num(|c| c.messages.fill_points as u64, |c, v| c.messages.fill_points = v as u32, 0, 99999)),
        ]),
        ("Transfers", false, vec![
            set('A', "Max upload bytes", 12, "Largest upload accepted (0 = no limit).", Num(|c| c.files.max_upload_bytes, |c, v| c.files.max_upload_bytes = v, 0, u64::MAX / 2)),
            set('B', "Transfer cps", 9, "Bytes per second assumed when estimating transfer time.", Num(|c| c.files.transfer_cps, |c, v| c.files.transfer_cps = v, 1, 1_000_000_000)),
            set('C', "Ask auto-logoff", 3, "Offer to log off when a download finishes?", Bool(|c| &mut c.files.ask_auto_logoff)),
            set('D', "Auto-logoff secs", 3, "Countdown before the auto-logoff hangs up.", Num(|c| c.files.auto_logoff_seconds as u64, |c, v| c.files.auto_logoff_seconds = v as u16, 1, 600)),
        ]),
        ("Upload Tests", false, vec![
            set('A', "Test command", 50, "Run on each upload, %F the file (e.g. clamscan --no-summary %F); blank = none.", Text(|c| &mut c.files.test_command, TEXT.max(200))),
            set('B', "Pass exit code", 3, "Exit code from the test command that means the file passed.", Num(|c| c.files.test_success_code.max(0) as u64, |c, v| c.files.test_success_code = v as i32, 0, 255)),
            set('C', "Age limit (years)", 3, "Refuse ZIPs with files older than this many years (0 = don't).", Num(|c| c.files.age_limit_years as u64, |c, v| c.files.age_limit_years = v as u8, 0, 99)),
            set('D', "Strict age test", 3, "Every file must be newer than the limit (else just the newest)?", Bool(|c| &mut c.files.strict_age)),
            set('E', "Delete list", 12, "File in the data directory listing names (wildcards) to strip from uploaded ZIPs.", Text(|c| &mut c.files.delete_list, TEXT.max(40))),
            set('F', "Archive comment", 12, "File in the data directory whose text becomes each uploaded ZIP's comment.", Text(|c| &mut c.files.archive_comment, TEXT.max(40))),
            set('G', "Allowed types", 50, "Upload types allowed (zip, tar.gz, txt...), checked against the contents; empty = any.", List(|c| &mut c.files.allowed_types)),
            set('H', "Summary line", 5, "Add what's inside (files, size, newest date) to the description: first, last or not.", Choice(
                |c| c.files.scan_summary as usize,
                |c, v| c.files.scan_summary = [SummaryAt::Off, SummaryAt::First, SummaryAt::Last][v],
                &SUMMARY_AT,
            )),
            set('I', "Names listed", 2, "File names from inside an archive to list under the summary.", Num(|c| c.files.scan_names as u64, |c, v| c.files.scan_names = v as u8, 0, 18)),
            set('J', "Max unpack ratio", 6, "Refuse archives unpacking to more than this many times their size (0 = any).", Num(|c| c.files.max_unpacked_ratio as u64, |c, v| c.files.max_unpacked_ratio = v as u32, 0, 1_000_000)),
            set('K', "Max unpacked bytes", 14, "Refuse archives unpacking to more than this (0 = any).", Num(|c| c.files.max_unpacked_bytes, |c, v| c.files.max_unpacked_bytes = v, 0, u64::MAX / 2)),
            set('L', "Refuse unsafe paths", 3, "Refuse archives with names like ../ or /etc/ that point outside their folder?", Bool(|c| &mut c.files.refuse_unsafe_paths)),
            set('M', "BBS ad file", 12, "File in the data directory added to every uploaded ZIP (an ad for the board); blank = off.", Text(|c| &mut c.files.bbs_ad, TEXT.max(40))),
            set('N', "Max picture bytes", 10, "Refuse GIF/PNG/JPEG uploads bigger than this (0 = any).", Num(|c| c.files.max_image_bytes, |c, v| c.files.max_image_bytes = v, 0, u64::MAX / 2)),
            set('O', "Max picture pixels", 10, "Refuse pictures with more pixels than this, width times height (0 = any).", Num(|c| c.files.max_image_pixels, |c, v| c.files.max_image_pixels = v, 0, u64::MAX / 2)),
            set('P', "Max GIF frames", 5, "Refuse animated GIFs with more frames than this (0 = any).", Num(|c| c.files.max_gif_frames as u64, |c, v| c.files.max_gif_frames = v as u32, 0, 100_000)),
        ]),
        ("Chat Mode", false, vec![
            set('A', "Override password", 20, "Password to page outside chat hours (blank = none).", Text(|c| &mut c.chat.override_password, Field::UPPER)),
            set('B', "Max page times", 3, "Pages allowed per call (0 = no limit).", Num(|c| c.chat.max_page_times as u64, |c, v| c.chat.max_page_times = v as u8, 0, 255)),
            set('C', "Page beeps", 3, "How many five-second rounds a page lasts.", Num(|c| c.chat.page_beeps as u64, |c, v| c.chat.page_beeps = v as u8, 1, 255)),
            set('D', "Restore chat time", 3, "Give callers back the time a chat took?", Bool(|c| &mut c.chat.restore_time)),
            set('E', "Available from", 5, "Start of chat hours (HH:MM); same as the end means always.", Text(|c| &mut c.chat.start, Field { allowed: "0123456789:", ..TEXT.max(5) })),
            set('F', "Available until", 5, "End of chat hours (HH:MM).", Text(|c| &mut c.chat.end, Field { allowed: "0123456789:", ..TEXT.max(5) })),
            set('G', "Override ACS", 20, "Who may page outside chat hours.", Text(|c| &mut c.chat.override_acs, ACS)),
            set('H', "Ask leave email", 3, "Offer to leave the SysOp email when a page goes unanswered?", Bool(|c| &mut c.chat.ask_email)),
        ]),
        ("Wait-For-Call", false, vec![
            set('A', "Allow ESC to exit", 3, "Does ESC at the WFC screen (--wfc) shut the BBS down?", Bool(|c| &mut c.wfc.esc_to_exit)),
            set('B', "Blank after", 5, "Blank the WFC screen after this many idle seconds (0 = never).", Num(|c| c.wfc.blank_seconds as u64, |c, v| c.wfc.blank_seconds = v as u32, 0, 65_535)),
            set('C', "Default WFC stat", 22, "Stats the WFC screen starts with.", Choice(
                |c| [1, 3, 4, 5, 8].iter().position(|&n| n == c.wfc.default_stat).unwrap_or(4),
                |c, v| c.wfc.default_stat = [1, 3, 4, 5, 8][v],
                &["System information", "System totals", "Today's statistics", "Last few callers", "flux/bbs information"],
            )),
        ]),
        ("AI Chat", false, vec![
            set('A', "Enabled", 3, "Offer AI chat (the AI menu command)?", Bool(|c| &mut c.ai.enabled)),
            set('B', "Server URL", 50, "OpenAI-style API, up to /v1 (e.g. http://localhost:8080/v1).", Text(|c| &mut c.ai.base_url, TEXT.max(200))),
            set('C', "Model", 30, "The model to ask.", Text(|c| &mut c.ai.model, TEXT.max(100))),
            set('D', "API key variable", 30, "Environment variable holding an API key, if the server needs one.", Text(|c| &mut c.ai.api_key_env, TEXT.max(60))),
            set('E', "Access", 20, "Who may use it; stock is s250|fI (SysOps, or users given flag I).", Text(|c| &mut c.ai.acs, ACS)),
            set('F', "AI's name", 20, "What the AI is called on screen and to itself.", Text(|c| &mut c.ai.name, TEXT.max(30))),
            set('G', "Extra prompt", 50, "Added to the built-in system prompt (its personality, house rules).", Text(|c| &mut c.ai.system_prompt, TEXT.max(1000))),
            set('H', "Reply length", 6, "Most tokens in one reply.", Num(|c| c.ai.max_tokens as u64, |c, v| c.ai.max_tokens = v as u32, 50, 100_000)),
            set('I', "Daily limit", 5, "Messages a caller may send a day (0 = no limit; SysOps have none).", Num(|c| c.ai.daily_limit as u64, |c, v| c.ai.daily_limit = v as u32, 0, 100_000)),
            set('J', "Web search", 3, "Let the AI search the web?", Bool(|c| &mut c.ai.web_search)),
            set('K', "SearXNG URL", 50, "A SearXNG instance to search with; blank for DuckDuckGo.", Text(|c| &mut c.ai.search_url, TEXT.max(200))),
            set('L', "Brave key variable", 30, "Environment variable holding a Brave Search API key (if no SearXNG).", Text(|c| &mut c.ai.search_key_env, TEXT.max(60))),
            set('M', "Timeout (secs)", 4, "How long to wait for the server.", Num(|c| c.ai.timeout_seconds, |c, v| c.ai.timeout_seconds = v, 10, 3600)),
            set('N', "Reasoning effort", 10, "For models that think first: none, low, medium...; blank for the server's default.", Text(|c| &mut c.ai.reasoning_effort, TEXT.max(20))),
            set('O', "Read pages", 3, "Let the AI read web pages it finds (public addresses only)?", Bool(|c| &mut c.ai.read_pages)),
            set('P', "Tarot a day", 3, "Tarot readings a caller may have a day (0 = no limit; SysOps have none).", Num(|c| c.ai.tarot_daily as u64, |c, v| c.ai.tarot_daily = v as u32, 0, 999)),
            set('Q', "Fortunes a day", 3, "Fortunes a caller may have a day (0 = no limit; SysOps have none).", Num(|c| c.ai.fortune_daily as u64, |c, v| c.ai.fortune_daily = v as u32, 0, 999)),
        ]),
        ("MeshCore", false, vec![
            set('A', "Enabled", 3, "Bridge a MeshCore companion radio's channels to message areas? Channels: [[meshcore.channel]] in fluxbbs.toml.", Bool(|c| &mut c.meshcore.enabled)),
            set('B', "Radio address", 40, "host:port of the radio (WiFi companion firmware: port 5000; a USB radio via ser2net).", Text(|c| &mut c.meshcore.address, TEXT.max(100))),
            set('C', "Reconnect (secs)", 4, "How long to wait between tries while the radio can't be reached.", Num(|c| c.meshcore.reconnect_seconds, |c, v| c.meshcore.reconnect_seconds = v, 5, 3600)),
            set('D', "Parts per post", 2, "Messages a long post may be split into on the mesh (each about 140 characters).", Num(|c| c.meshcore.max_parts as u64, |c, v| c.meshcore.max_parts = v as u8, 1, 10)),
        ]),
        ("FTN Echomail", false, vec![
            set('A', "Enabled", 3, "Be a node of an FTN network (fsxNet...), polling the hub over binkp? Echoes: [[ftn.area]] in fluxbbs.toml.", Bool(|c| &mut c.ftn.enabled)),
            set('B', "Our address", 30, "This board's address with the network's domain, e.g. 21:1/999@fsxnet.", Text(|c| &mut c.ftn.address, TEXT.max(40))),
            set('C', "Hub address", 30, "The hub's FTN address, e.g. 21:1/100.", Text(|c| &mut c.ftn.hub, TEXT.max(40))),
            set('D', "Hub host", 40, "Where the hub's binkp mailer listens: host or host:port (24554 if none).", Text(|c| &mut c.ftn.hub_host, TEXT.max(100))),
            set('E', "Session password", 20, "The binkp session password the hub gave (sent as CRAM-MD5 when the hub offers it).", Text(|c| &mut c.ftn.session_password, TEXT.max(40))),
            set('F', "Packet password", 8, "The packet password (up to 8 characters); blank if none.", Text(|c| &mut c.ftn.packet_password, TEXT.max(8))),
            set('G', "AreaFix password", 20, "AreaFix's password; blank to use the session password.", Text(|c| &mut c.ftn.areafix_password, TEXT.max(40))),
            set('H', "Origin line", 50, "Text of the origin line on messages sent; blank for the board's name.", Text(|c| &mut c.ftn.origin, TEXT.max(60))),
            set('I', "Poll minutes", 4, "Minutes between polls of the hub (0: only by hand, fluxbbs ftn poll).", Num(|c| c.ftn.poll_minutes as u64, |c, v| c.ftn.poll_minutes = v as u32, 0, 1440)),
            set('J', "Answer on", 22, "Where to answer the hub's binkp calls (0.0.0.0:24554), so it can send mail as it comes (crash); blank: poll only (hold). Restart to apply.", Text(|c| &mut c.ftn.listen, TEXT.max(60))),
        ]),
        ("File System", false, vec![
            set('A', "Use file points", 3, "Charge file points for downloads?", Bool(|c| &mut c.files.use_file_points)),
            set('B', "Kb per point", 5, "One file point per this many kb.", Num(|c| c.files.kb_per_point as u64, |c, v| c.files.kb_per_point = v as u16, 1, 65_535)),
            set('C', "Upload points %", 5, "Percent of an upload's points given to the uploader.", Num(|c| c.files.upload_points_percent as u64, |c, v| c.files.upload_points_percent = v as u16, 0, 65_535)),
            set('D', "Daily file limit", 3, "Enforce users' daily download limits?", Bool(|c| &mut c.files.use_daily_limit)),
            set('E', "Daily kb limit", 3, "Enforce users' daily kb limits?", Bool(|c| &mut c.files.use_daily_kb_limit)),
            set('F', "UL/DL ratio", 3, "Enforce file ratios?", Bool(|c| &mut c.files.use_ratio)),
            set('G', "UL/DL kb ratio", 3, "Enforce kb ratios?", Bool(|c| &mut c.files.use_kb_ratio)),
            set('H', "Auto-validate", 3, "May new uploads be downloaded before a SysOp checks them?", Bool(|c| &mut c.files.auto_validate)),
            set('I', "Blind uploads", 3, "Accept files sent without naming them first?", Bool(|c| &mut c.files.allow_blind_upload)),
            set('J', "Dupe check", 16, "Check upload names against existing files.", Choice(
                |c| (c.files.upload_dupe_check.clamp(1, 3) - 1) as usize,
                |c, v| c.files.upload_dupe_check = v as u8 + 1,
                &["Never", "This area", "Every area"],
            )),
            set('K', "Import DIZ", 3, "Take descriptions from files inside uploaded archives, or from a single art file's SAUCE?", Bool(|c| &mut c.files.import_descriptions)),
            set('L', "Description files", 40, "Names to look for inside archives, in order.", List(|c| &mut c.files.description_files)),
            set('M', "No description", 40, "Shown for files without a description.", Text(|c| &mut c.files.no_description, TEXT.max(80))),
            set('N', "Wrap descriptions", 3, "Continue long descriptions on the lister's next page?", Bool(|c| &mut c.files.wrap_descriptions)),
            set('O', "Advance on flag", 3, "Move the lister's bar down after flagging a file?", Bool(|c| &mut c.files.advance_bar_on_flag)),
            set('P', "DIZ lines", 2, "Most lines of an imported description to keep (0: all).", Num(|c| c.files.max_import_lines as u64, |c, v| c.files.max_import_lines = v as u16, 0, 20)),
        ]),
        ("BBSLink", false, vec![
            set('A', "Enabled", 3, "Offer BBSLink's door games (menu command DB with a door code; DB menu lists them all)?", Bool(|c| &mut c.bbslink.enabled)),
            set('B', "Server", 40, "BBSLink's door server (games.bbslink.net).", Text(|c| &mut c.bbslink.host, TEXT.max(100))),
            set('C', "System code", 20, "This board's system code, from signing up at bbslink.net.", Text(|c| &mut c.bbslink.system_code, TEXT.max(40))),
            set('D', "Auth code", 20, "This board's authorization code from BBSLink. Keep it secret.", Text(|c| &mut c.bbslink.auth_code, TEXT.max(40))),
            set('E', "Scheme code", 20, "This board's scheme code from BBSLink.", Text(|c| &mut c.bbslink.scheme_code, TEXT.max(40))),
        ]),
        ("Slot Machine", false, vec![
            set('A', "Pulls a day", 4, "Pulls a caller gets a day (0 = no limit).", Num(|c| c.slots.pulls_per_day as u64, |c, v| c.slots.pulls_per_day = v as u32, 0, 9999)),
            set('B', "Win chance %", 3, "How loose: the chance, in percent, that a pull wins something.", Num(|c| c.slots.win_percent as u64, |c, v| c.slots.win_percent = v as u32, 0, 100)),
            set('C', "Jackpot 1 in", 6, "Three sevens come up once in this many pulls (0 = never).", Num(|c| c.slots.jackpot_odds as u64, |c, v| c.slots.jackpot_odds = v as u32, 0, 1_000_000)),
            set('D', "7 7 7", 20, "The jackpot: minutes on today's time, then file points (e.g. 60 100).", Prize(|c| &mut c.slots.jackpot)),
            set('E', "BAR BAR BAR", 20, "Three bars pay: minutes, then file points.", Prize(|c| &mut c.slots.bars)),
            set('F', "Three bells", 20, "Three bells pay: minutes, then file points.", Prize(|c| &mut c.slots.bells)),
            set('G', "Three plums", 20, "Three plums pay: minutes, then file points.", Prize(|c| &mut c.slots.plums)),
            set('H', "Three oranges", 20, "Three oranges pay: minutes, then file points.", Prize(|c| &mut c.slots.oranges)),
            set('I', "Three lemons", 20, "Three lemons pay: minutes, then file points.", Prize(|c| &mut c.slots.lemons)),
            set('J', "Three cherries", 20, "Three cherries pay: minutes, then file points.", Prize(|c| &mut c.slots.cherries)),
            set('K', "Two cherries", 20, "Cherries on the first two reels pay: minutes, then file points.", Prize(|c| &mut c.slots.two_cherries)),
            set('L', "One cherry", 20, "A cherry on the first reel pays: minutes, then file points.", Prize(|c| &mut c.slots.one_cherry)),
            set('M', "Free logon pulls", 3, "A free pull on each call (the logon sequence's slots step), at most this many a day.", Num(|c| c.slots.free_per_day as u64, |c, v| c.slots.free_per_day = v as u32, 0, 999)),
        ]),
        ("Casino, Temple and Carrier Lost", false, vec![
            set('A', "Chips a day", 6, "The free stack each caller gets a day for blackjack and craps.", Num(|c| c.casino.daily_chips as u64, |c, v| c.casino.daily_chips = v as u32, 1, 1_000_000)),
            set('B', "Smallest bet", 6, "The table minimum, in chips.", Num(|c| c.casino.min_bet as u64, |c, v| c.casino.min_bet = v as u32, 1, 1_000_000)),
            set('C', "Biggest bet", 6, "The table maximum, in chips.", Num(|c| c.casino.max_bet as u64, |c, v| c.casino.max_bet = v as u32, 1, 1_000_000)),
            set('D', "Chips a point", 6, "Leaving a table up pays a file point per this many chips won (0 = none).", Num(|c| c.casino.chips_per_point as u64, |c, v| c.casino.chips_per_point = v as u32, 0, 1_000_000)),
            set('E', "Chips a minute", 6, "...and a minute on today's time per this many (0 = none).", Num(|c| c.casino.chips_per_minute as u64, |c, v| c.casino.chips_per_minute = v as u32, 0, 1_000_000)),
            set('F', "Decks", 2, "Decks in the blackjack shoe.", Num(|c| c.casino.decks as u64, |c, v| c.casino.decks = v as u32, 1, 8)),
            set('G', "Hit soft 17", 3, "Does the blackjack dealer hit a soft 17?", Bool(|c| &mut c.casino.hit_soft_17)),
            set('H', "Craps odds", 2, "The most odds a craps player may lay behind the line, times the line bet.", Num(|c| c.casino.max_odds as u64, |c, v| c.casino.max_odds = v as u32, 0, 100)),
            set('I', "Buffet after", 6, "Players club: chips played in a day before the buffet's comped.", Num(|c| c.casino.comp_buffet as u64, |c, v| c.casino.comp_buffet = v as u32, 0, 1_000_000)),
            set('J', "Buffet minutes", 4, "Minutes on today's time the buffet gives.", Num(|c| c.casino.buffet_minutes as u64, |c, v| c.casino.buffet_minutes = v as u32, 0, 9999)),
            set('K', "Suite after", 6, "Chips played in a day before a suite's comped.", Num(|c| c.casino.comp_suite as u64, |c, v| c.casino.comp_suite = v as u32, 0, 1_000_000)),
            set('L', "Suite chips", 6, "Chips the suite gives (on today's stack).", Num(|c| c.casino.suite_chips as u64, |c, v| c.casino.suite_chips = v as u32, 0, 1_000_000)),
            set('M', "Show after", 6, "Chips played in a day before show tickets are comped.", Num(|c| c.casino.comp_show as u64, |c, v| c.casino.comp_show = v as u32, 0, 1_000_000)),
            set('N', "Show points", 6, "File points the show tickets give.", Num(|c| c.casino.show_points as u64, |c, v| c.casino.show_points = v as u32, 0, 1_000_000)),
            set('O', "Temple turns", 4, "The temple (AD): turns a caller has a day for walking, searching, resting and trying things (0 = no limit).", Num(|c| c.temple.turns_per_day as u64, |c, v| c.temple.turns_per_day = v as u32, 0, 9999)),
            set('P', "Temple depth pay", 6, "File points for reaching a new depth, times the depth (0 = none).", Num(|c| c.temple.points_per_depth as u64, |c, v| c.temple.points_per_depth = v as u32, 0, 1_000_000)),
            set('Q', "Temple gold a point", 6, "The torch-bearer trades a file point for this much gold (0 = no trading).", Num(|c| c.temple.gold_per_point as u64, |c, v| c.temple.gold_per_point = v as u32, 0, 1_000_000)),
            set('R', "Carrier calls a day", 4, "Carrier Lost (AP): scans and dials a caller has a day (0 = no limit). [carrier] points_per_tier and gear_cost are in the TOML.", Num(|c| c.carrier.actions_per_day as u64, |c, v| c.carrier.actions_per_day = v as u32, 0, 9999)),
        ]),
    ]
}

/// Write the settings that differ between `old` and `new` into the
/// TOML file at `path`, keeping everything else as it is.
pub fn save_changes(path: &Path, old: &Config, new: &Config) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let fresh: toml_edit::DocumentMut = toml::to_string(new)?.parse()?;
    let (old_v, new_v) = (toml::Value::try_from(old)?, toml::Value::try_from(new)?);
    let empty = toml::Table::new();
    let (ot, nt) = (old_v.as_table().unwrap_or(&empty), new_v.as_table().unwrap_or(&empty));
    let keys: std::collections::BTreeSet<&String> = ot.keys().chain(nt.keys()).collect();
    for k in keys {
        match (ot.get(k), nt.get(k)) {
            (a, b) if a == b => {}
            (_, None) => {
                doc.remove(k);
            }
            (Some(toml::Value::Table(a)), Some(toml::Value::Table(b))) if doc.get(k).is_some_and(|i| i.is_table_like()) => {
                let inner: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
                for kk in inner {
                    if a.get(kk) == b.get(kk) {
                        continue;
                    }
                    let table = doc[k.as_str()].as_table_like_mut().expect("checked");
                    match fresh.get(k).and_then(|t| t.get(kk)) {
                        // A plain value replaced in place keeps its comments.
                        Some(toml_edit::Item::Value(v)) if table.get(kk).is_some_and(|i| i.is_value()) => {
                            let slot = table.get_mut(kk).and_then(|i| i.as_value_mut()).expect("checked");
                            let decor = slot.decor().clone();
                            *slot = v.clone();
                            *slot.decor_mut() = decor;
                        }
                        Some(item) => {
                            table.insert(kk, item.clone());
                        }
                        None => {
                            table.remove(kk);
                        }
                    }
                }
            }
            _ => {
                if let Some(item) = fresh.get(k) {
                    doc[k.as_str()] = item.clone();
                }
            }
        }
    }
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, doc.to_string())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// New: the sections on the `*C` screen, in four groups: each one's key
/// (its place in [`sections`]; the last, the default colors), a short
/// name and what's in it.
type Group = (&'static str, &'static [(char, &'static str, &'static str)]);

const GROUPS: &[Group] = &[
    ("the board", &[
        ('A', "Main BBS Settings", "Name, location, address, SysOp, nodes, idle timeout, time zone"),
        ('B', "SSH, Telnet, Terminal", "Where SSH and telnet listen, character sets (takes effect at restart)"),
        ('E', "Prompts", "Yes/no prompts and bars, password echo, pauses"),
        ('H', "Website Feed", "The status feed for a website: who's on, last callers, new files"),
        ('R', "Wait-For-Call", "The local wait-for-call screen"),
        ('Z', "Default Colors", "The color scheme new callers start with"),
    ]),
    ("callers", &[
        ('C', "Access and Logon", "SysOp access, the matrix, logon tries, system password, time per call"),
        ('F', "Security", "Bot check, connection limits, bans, allow and deny lists"),
        ('I', "Passwords, Sign-ups", "Password rules, sign-up limits, reserved handles, purging"),
        ('D', "New User Config", "New callers' settings, and which of them they're asked"),
        ('J', "New User Questions", "What new callers are asked: real name, birth date, phone..."),
        ('K', "New User Voting", "Callers voting new users in"),
    ]),
    ("messages & files", &[
        ('L', "Message System", "Message access, private replies, quoting, deleting"),
        ('M', "Attachments, Uploads", "Attaching files to messages, uploading messages"),
        ('N', "Caller Boards", "Boards callers run, request fills and points"),
        ('V', "File System", "File points, ratios, limits, validation, descriptions"),
        ('O', "Transfers", "Upload size, transfer speed, logging off after"),
        ('P', "Upload Tests", "Testing and unpacking uploads, the BBS ad, pictures"),
        ('G', "Art Gallery", "The gallery's folder or file area, access, play speed"),
    ]),
    ("talk & networks", &[
        ('Q', "Chat Mode", "Paging the SysOp: hours, beeps, overrides"),
        ('S', "AI Chat", "The AI's server, model, limits, web search, tarot and fortunes"),
        ('T', "MeshCore", "The MeshCore radio bridge"),
        ('U', "FTN Echomail", "Echomail networks: addresses, hub, passwords, polling"),
        ('W', "BBSLink", "Door games on bbslink.net's server: on or off, the board's codes"),
        ('X', "Slot Machine", "Pulls a day, how loose it is, what each line pays"),
        ('Y', "Casino, Temple, CL", "The tables; the temple; Carrier Lost (AP)"),
    ]),
];

/// Columns of the groups, and the row each group's heading is on.
const GROUP_AT: [(u16, u16); 4] = [(2, 4), (28, 4), (54, 4), (2, 13)];

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Cut from the left, for paths: "...end".
fn tail(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len <= n { s.to_string() } else { format!("...{}", s.chars().skip(len + 3 - n).collect::<String>()) }
}

impl Session {
    /// The board right now, beside the groups.
    fn config_status(&mut self, cfg: &Config) {
        let st = self.state.clone();
        let now = crate::clock::now().naive_local();
        let up = (now - st.started).num_minutes().max(0) as u64;
        let today = st.sys.day(now.date()).unwrap_or_default();
        let ftn = match (cfg.ftn.enabled, cfg.ftn.address.trim()) {
            (false, _) => "off".to_string(),
            (true, "") => "on, no address yet".into(),
            (true, a) if cfg.ftn.networks.is_empty() => a.to_string(),
            (true, a) => format!("{a} +{} more", cfg.ftn.networks.len()),
        };
        let zone = if cfg.bbs.time_zone.trim().is_empty() { "host clock" } else { cfg.bbs.time_zone.trim() };
        let rows = [
            ("board", format!("{} · flux/bbs {}", cfg.bbs.name, env!("CARGO_PKG_VERSION"))),
            ("up", format!("since {}, {}", st.started.format("%m/%d %H:%M"), crate::doorstats::span(up))),
            ("online", format!("{} of {} nodes · {} users · {} calls today", st.nodes.online().len(), cfg.bbs.max_nodes, st.users.count().unwrap_or(0), today.calls)),
            ("clock", format!("{} {zone}", now.format("%H:%M"))),
            ("ai", if cfg.ai.enabled { cfg.ai.model.clone() } else { "off".into() }),
            ("ftn", ftn),
            ("meshcore", if cfg.meshcore.enabled { cfg.meshcore.address.clone() } else { "off".into() }),
            ("config", tail(&self.config_path().display().to_string(), 40)),
        ];
        let (x, y) = (28, GROUP_AT[3].1);
        self.screen.goto(x, y);
        self.write_with_controls(&format!("|U5─[|U4 right now |U5]{}", "─".repeat(80 - x as usize - 14)));
        for (i, (label, value)) in rows.iter().enumerate() {
            self.screen.goto(x + 2, y + 1 + i as u16);
            let value: String = value.chars().take(40).collect();
            self.write_with_controls(&format!("|U5{label:<9}|U3"));
            self.write(&value);
        }
    }

    /// `cfgSystemConfig`: pick a section to edit. New: the sections in
    /// groups, what each holds on the bottom bar, and the board's state.
    pub async fn system_config(&mut self) -> io::Result<()> {
        tracing::info!("{}: system config", self.user.handle);
        let original = (*self.state.config()).clone();
        let mut cfg = original.clone();
        let secs = sections();
        let mut at = 'A';
        loop {
            let mut f = Form::new(&format!("{} System Configuration", cfg.bbs.name));
            f.hints();
            for ((_, items), (x, y)) in GROUPS.iter().zip(GROUP_AT) {
                for (i, (key, label, info)) in items.iter().enumerate() {
                    // What's on, beside the sections that switch something on.
                    let state = match key {
                        'H' => on_off(cfg.web.enabled),
                        'S' => on_off(cfg.ai.enabled),
                        'T' => on_off(cfg.meshcore.enabled),
                        'U' => on_off(cfg.ftn.enabled),
                        'W' => on_off(cfg.bbslink.enabled),
                        _ => "",
                    };
                    // Rows count from the form's first (screen row 4).
                    let row = y + 1 + i as u16 - 3;
                    f.item_xy(x, row, x + 21, *key, label, if state.is_empty() { 0 } else { 3 }, state, info);
                }
            }
            // The bottom bar goes under the status lines.
            f.gap(18);
            f.select(at);
            self.cfg_draw(&f);
            for ((title, _), (x, y)) in GROUPS.iter().zip(GROUP_AT) {
                self.screen.goto(x, y);
                self.write_with_controls(&format!("|U5─[|U4 {title} |U5]{}", "─".repeat(20 - title.len())));
            }
            self.config_status(&cfg);
            let Some(key) = self.cfg_command(&mut f).await? else { break };
            at = key;
            let i = (key as u8 - b'A') as usize;
            match secs.get(i) {
                Some((title, _, settings)) => self.config_section(title, settings, &mut cfg).await?,
                None => self.palette_editor(&mut cfg.colors).await?,
            }
        }
        self.screen.goto(1, self.screen.rows());
        self.newline();
        if toml::to_string(&cfg).ok() != toml::to_string(&original).ok() {
            self.show("|U1Save system configuration? ").await?;
            if self.yes_no(true).await? {
                let path = self.config_path();
                save_changes(&path, &original, &cfg).map_err(io_err)?;
                self.state.set_config(cfg);
                tracing::info!("{}: saved system configuration", self.user.handle);
            }
        }
        Ok(())
    }

    /// The file the configuration was loaded from.
    pub(crate) fn config_path(&self) -> PathBuf {
        self.state.config_path.clone()
    }

    /// One section's screen.
    async fn config_section(&mut self, title: &str, settings: &[Setting], cfg: &mut Config) -> io::Result<()> {
        let mut f = Form::new(title);
        f.value_col(24);
        for s in settings {
            f.item(s.key, s.label, s.len, shown(&s.kind, cfg), s.info);
        }
        self.cfg_draw(&f);
        while let Some(key) = self.cfg_command(&mut f).await? {
            let Some(s) = settings.iter().find(|s| s.key == key) else { continue };
            match &s.kind {
                Kind::Text(get, field) => {
                    if let Some(v) = self.cfg_text(&mut f, key, *field).await? {
                        *get(cfg) = v;
                    }
                }
                Kind::Bool(get) => {
                    let cur = *get(cfg);
                    *get(cfg) = self.cfg_bool(&mut f, key, cur).await?;
                }
                Kind::Num(get, put, min, max) => {
                    if let Some(v) = self.cfg_number(&mut f, key, *min, *max).await? {
                        put(cfg, v);
                    }
                    self.cfg_set(&mut f, key, get(cfg).to_string());
                }
                Kind::List(get) => {
                    if let Some(v) = self.cfg_text(&mut f, key, TEXT.max(200)).await? {
                        *get(cfg) = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                    }
                    self.cfg_set(&mut f, key, get(cfg).join(", "));
                }
                Kind::Path(get) => {
                    if let Some(v) = self.cfg_text(&mut f, key, TEXT.max(200)).await?.filter(|v| !v.is_empty()) {
                        *get(cfg) = v.into();
                    }
                    self.cfg_set(&mut f, key, get(cfg).display().to_string());
                }
                Kind::Char(get) => {
                    if let Some(c) = self.cfg_text(&mut f, key, TEXT.max(1)).await?.and_then(|v| v.chars().next()) {
                        *get(cfg) = c;
                    }
                    self.cfg_set(&mut f, key, get(cfg).to_string());
                }
                Kind::Letter(get) => {
                    let field = Field { allowed: "ABCDEFGHIJKLMNOPQRSTUVWXYZ", ..Field::UPPER.max(1) };
                    if let Some(c) = self.cfg_text(&mut f, key, field).await?.and_then(|v| v.chars().next()) {
                        *get(cfg) = c;
                    }
                    self.cfg_set(&mut f, key, get(cfg).to_string());
                }
                Kind::Choice(get, put, labels) => {
                    let pick = self.cfg_option(&mut f, key, labels, get(cfg)).await?;
                    put(cfg, pick);
                }
                Kind::Prize(get) => {
                    // Edited as two numbers, shown in words.
                    let p = *get(cfg);
                    f.set(key, format!("{} {}", p.minutes, p.points));
                    if let Some(p) = self.cfg_text(&mut f, key, TEXT.max(20)).await?.as_deref().and_then(prize_typed) {
                        *get(cfg) = p;
                    }
                    self.cfg_set(&mut f, key, prize_shown(get(cfg)));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_keeps_comments_and_untouched_settings() {
        let dir = std::env::temp_dir().join(format!("iqcfg{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fluxbbs.toml");
        let text = "# my board\n\n[bbs]\nname = \"Old Name\" # the name\nmax_nodes = 4\n\n[files]\n# points!\nuse_file_points = false\n";
        std::fs::write(&path, text).unwrap();
        let old = Config::load(&path).unwrap();
        let mut new = old.clone();
        new.bbs.name = "New Name".into();
        new.files.use_file_points = true;
        save_changes(&path, &old, &new).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.starts_with("# my board"), "{out}");
        assert!(out.contains("name = \"New Name\" # the name"), "{out}");
        assert!(out.contains("max_nodes = 4"), "{out}");
        assert!(out.contains("# points!\nuse_file_points = true"), "{out}");
        assert_eq!(Config::load(&path).unwrap().bbs.name, "New Name");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_section_is_in_a_group() {
        let mut keys: Vec<char> = GROUPS.iter().flat_map(|(_, items)| items.iter().map(|i| i.0)).collect();
        keys.sort();
        let want: Vec<char> = (0..=sections().len() as u8).map(|i| (b'A' + i) as char).collect();
        assert_eq!(keys, want);
        assert!(GROUPS.iter().all(|(t, items)| t.len() <= 20 && items.iter().all(|i| i.1.len() <= 21 && i.2.len() <= 72)));
    }

    #[test]
    fn prizes_are_typed_as_two_numbers() {
        assert_eq!(prize_typed("30 40"), Some(crate::config::Prize { minutes: 30, points: 40 }));
        assert_eq!(prize_typed(&prize_shown(&crate::config::Prize { minutes: 5, points: 7 })), Some(crate::config::Prize { minutes: 5, points: 7 }));
        assert_eq!(prize_typed("12"), Some(crate::config::Prize { minutes: 12, points: 0 }));
        assert_eq!(prize_typed("none"), None);
    }

    #[test]
    fn section_keys_are_unique() {
        for (title, _, settings) in sections() {
            let mut keys: Vec<char> = settings.iter().map(|s| s.key).collect();
            let n = keys.len();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), n, "{title}");
            assert!(n <= 18, "{title} has too many rows for the screen");
        }
    }
}
