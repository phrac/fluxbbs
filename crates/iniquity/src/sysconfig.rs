//! `*C`: the system configuration editor (CONFIG1/CONFIG2). Each section
//! of iniquity.toml gets a screen; on leaving, only the settings that
//! changed are written back, so the file's comments and layout survive.
//! Most changes apply at once; SSH settings and paths need a restart.

use std::io;
use std::path::{Path, PathBuf};

use crate::cfgedit::{Field, Form, yes_no};
use crate::config::{CharsetPref, Config};
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
            set('F', "Telnet", 3, "Also accept plain (unencrypted) telnet? (restart)", Bool(|c| &mut c.telnet.enabled)),
            set('G', "Telnet listen", 40, "Address and port for telnet, e.g. 0.0.0.0:2323 (restart).", Text(|c| &mut c.telnet.listen, TEXT)),
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
            set('K', "Import DIZ", 3, "Take descriptions from files inside uploaded archives?", Bool(|c| &mut c.files.import_descriptions)),
            set('L', "Description files", 40, "Names to look for inside archives, in order.", List(|c| &mut c.files.description_files)),
            set('M', "No description", 40, "Shown for files without a description.", Text(|c| &mut c.files.no_description, TEXT.max(80))),
            set('N', "Wrap descriptions", 3, "Continue long descriptions on the lister's next page?", Bool(|c| &mut c.files.wrap_descriptions)),
            set('O', "Advance on flag", 3, "Move the lister's bar down after flagging a file?", Bool(|c| &mut c.files.advance_bar_on_flag)),
            set('P', "Max upload bytes", 12, "Largest upload accepted (0 = no limit).", Num(|c| c.files.max_upload_bytes, |c, v| c.files.max_upload_bytes = v, 0, u64::MAX / 2)),
            set('Q', "Transfer cps", 9, "Bytes per second assumed when estimating transfer time.", Num(|c| c.files.transfer_cps, |c, v| c.files.transfer_cps = v, 1, 1_000_000_000)),
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

impl Session {
    /// `cfgSystemConfig`: pick a section to edit.
    pub async fn system_config(&mut self) -> io::Result<()> {
        tracing::info!("{}: system config", self.user.handle);
        let original = (*self.state.config()).clone();
        let mut cfg = original.clone();
        let secs = sections();
        loop {
            let mut f = Form::new(&format!("{} System Configuration", cfg.bbs.name));
            for (i, (title, restart, _)) in secs.iter().enumerate() {
                let note = if *restart { "SSH and telnet settings take effect when the BBS restarts." } else { "" };
                f.item((b'A' + i as u8) as char, title, 0, "", note);
            }
            f.item((b'A' + secs.len() as u8) as char, "Default Colors", 0, "", "");
            self.cfg_draw(&f);
            let Some(key) = self.cfg_command(&mut f).await? else { break };
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
    fn config_path(&self) -> PathBuf {
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
        let path = dir.join("iniquity.toml");
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
