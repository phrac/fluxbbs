//! `iniquity.toml`: the successor to `tCfgRec` / iniquity.dat and
//! LEVELS.DAT.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use iniquity_term::Palette;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bbs: Bbs,
    pub ssh: Ssh,
    pub telnet: Telnet,
    pub paths: Paths,
    pub terminal: Terminal,
    pub acs: Acs,
    pub logon: Logon,
    pub newuser: NewUser,
    pub prompts: Prompts,
    pub messages: Messages,
    pub files: Files,
    pub nuv: Nuv,
    /// External transfer protocols, in addition to built-in Zmodem. Only
    /// used until protocols.toml exists (the protocol editor writes it).
    #[serde(rename = "protocol", default, skip_serializing_if = "Vec::is_empty")]
    pub protocols: Vec<Protocol>,
    /// Default colour scheme for new users and before logon.
    pub colors: Palette,
    /// Access levels 'A'..'Z'. Only used until levels.toml exists (the
    /// level editor writes it).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub levels: BTreeMap<String, Level>,
    /// Directory the config file lives in; relative paths resolve from here.
    #[serde(skip)]
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bbs {
    pub name: String,
    pub location: String,
    pub phone: String,
    pub sysop_alias: String,
    pub sysop_name: String,
    pub max_nodes: u16,
    /// Disconnect after this many idle seconds (0 = never).
    pub inactivity_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ssh {
    pub listen: String,
    /// Where the host keys live (Ed25519, ECDSA and RSA; made on first run).
    pub host_key_dir: PathBuf,
    /// Also offer older algorithms (ECDH P-256, DH group 14, AES-CBC,
    /// HMAC-SHA1) after the modern ones. Needed by SyncTERM.
    pub legacy_algorithms: bool,
}

/// Plain telnet, for clients and directories that don't do SSH.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Telnet {
    pub enabled: bool,
    pub listen: String,
}

impl Default for Telnet {
    fn default() -> Self {
        Self { enabled: true, listen: "0.0.0.0:2323".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    pub text: PathBuf,
    pub menus: PathBuf,
    pub data: PathBuf,
    /// Prompt string overrides; built-in stock strings fill any gaps.
    pub strings: PathBuf,
    /// Message conferences and areas.
    pub msgareas: PathBuf,
    /// File conferences and areas.
    pub fileareas: PathBuf,
    /// External transfer protocols.
    pub protocols: PathBuf,
    /// Archivers for formats other than ZIP.
    pub archivers: PathBuf,
    /// Access levels.
    pub levels: PathBuf,
    /// Infoform definitions.
    pub infoforms: PathBuf,
    /// Scratch space for uploads in progress.
    pub temp: PathBuf,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CharsetPref {
    Auto,
    Cp437,
    Utf8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Terminal {
    pub charset: CharsetPref,
    /// TERM values that get raw CP437 when `charset = "auto"`.
    pub cp437_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Acs {
    pub sysop: String,
    pub cosysop: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Logon {
    /// Show the MATRIX menu before logging in.
    pub matrix: bool,
    /// Offer the new-user application to unknown names.
    pub ask_apply: bool,
    pub tries: u8,
    /// Extra password asked of users matching `system_password_acs`.
    pub system_password: String,
    pub system_password_acs: String,
    pub start_menu: String,
    /// Handles are real names.
    pub real_names: bool,
    /// Time limits reset every call instead of every day.
    pub time_per_call: bool,
    /// How many calls the last-callers list shows.
    pub last_callers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewUser {
    /// Password needed to apply (empty = none).
    pub password: String,
    /// Ask "proceed with application?" first.
    pub verify: bool,
    /// Handle capitalisation, 1-8 (`tInFlag`, 1 = as typed).
    pub alias_format: u8,
    pub page_length: u16,
    pub ask_page_length: bool,
    pub hot_keys: bool,
    pub ask_hot_keys: bool,
    pub expert: bool,
    pub ask_expert: bool,
    pub pause: bool,
    pub ask_pause: bool,
    pub quote: bool,
    pub ask_quote: bool,
    pub yes_no_bars: bool,
    pub ask_yes_no_bars: bool,
    /// Which personal details the application asks for. Without a real
    /// name, the handle is used.
    pub ask_real_name: bool,
    pub ask_birth_date: bool,
    pub ask_phone: bool,
    pub ask_address: bool,
    /// City, country/state and zip code.
    pub ask_location: bool,
    pub ask_sex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Prompts {
    pub yes: String,
    pub no: String,
    pub yes_word: String,
    pub no_word: String,
    pub yes_bar: String,
    pub no_bar: String,
    pub password_echo: char,
    /// Erase "pause" prompts after a key.
    pub remove_pause: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Messages {
    pub acs_post_email: String,
    pub acs_anonymous: String,
    pub acs_anon_automsg: String,
    /// May append their auto-signature.
    pub acs_autosig: String,
    /// Callers may quit reading mandatory messages.
    pub abort_mandatory_ok: bool,
    pub ask_private: bool,
    pub ask_private_reply: bool,
    /// Offer to post after reading the last message in an area.
    pub ask_post_after_read: bool,
    /// Offer to delete email after replying to it.
    pub ask_delete_replied: bool,
    /// Offer to delete all read email when leaving the mail reader.
    pub ask_delete_all_read: bool,
    pub ask_auto_quote: bool,
    /// Suggest line numbers when quoting by hand.
    pub default_quote_lines: bool,
    pub max_quote_lines: u8,
    /// The mandatory scan ignores conferences.
    pub mandatory_all_confs: bool,
    /// Number listed areas 1, 2, 3... counting only visible ones.
    pub compress_area_numbers: bool,
    /// Shown instead of quoted lines that contain ANSI.
    pub ansi_quote: String,
}

impl Default for Messages {
    fn default() -> Self {
        Self {
            acs_post_email: "s25".into(),
            acs_anonymous: "s50".into(),
            acs_anon_automsg: "s50".into(),
            acs_autosig: "s25".into(),
            abort_mandatory_ok: false,
            ask_private: true,
            ask_private_reply: true,
            ask_post_after_read: true,
            ask_delete_replied: true,
            ask_delete_all_read: true,
            ask_auto_quote: true,
            default_quote_lines: true,
            max_quote_lines: 11,
            mandatory_all_confs: true,
            compress_area_numbers: true,
            ansi_quote: "(ansi codes detected; line not displayed)".into(),
        }
    }
}

/// New user voting (NUV): existing users vote new ones in or out.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Nuv {
    pub enabled: bool,
    /// Yes votes that validate a new user.
    pub votes_yes: u8,
    /// No votes that delete one.
    pub votes_no: u8,
    /// Who gets voted on.
    pub candidate_acs: String,
    /// Who may vote.
    pub voter_acs: String,
    /// Show voters' initials beside their comments.
    pub initials: bool,
    /// Access level given when voted in.
    pub level: char,
}

impl Default for Nuv {
    fn default() -> Self {
        Self {
            enabled: true,
            votes_yes: 5,
            votes_no: 3,
            candidate_acs: "!s50".into(),
            voter_acs: "s50".into(),
            initials: true,
            level: 'B',
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Files {
    pub use_file_points: bool,
    /// One file point per this many kb.
    pub kb_per_point: u16,
    /// Percent of an upload's points given to the uploader.
    pub upload_points_percent: u16,
    pub use_daily_limit: bool,
    pub use_daily_kb_limit: bool,
    pub use_ratio: bool,
    pub use_kb_ratio: bool,
    /// New uploads may be downloaded before a SysOp checks them.
    pub auto_validate: bool,
    /// Accept files sent without naming them first.
    pub allow_blind_upload: bool,
    /// Check names before upload: 1 never, 2 this area, 3+ every area.
    pub upload_dupe_check: u8,
    /// Take descriptions from these files inside uploaded archives.
    pub import_descriptions: bool,
    pub description_files: Vec<String>,
    pub no_description: String,
    /// Long descriptions continue on the next page of the file lister.
    pub wrap_descriptions: bool,
    /// Flagging a file moves the lister's bar down.
    pub advance_bar_on_flag: bool,
    /// Largest accepted upload, in bytes (0 = no limit).
    pub max_upload_bytes: u64,
    /// Bytes per second assumed when estimating transfer time.
    pub transfer_cps: u64,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            use_file_points: false,
            kb_per_point: 15,
            upload_points_percent: 175,
            use_daily_limit: true,
            use_daily_kb_limit: true,
            use_ratio: false,
            use_kb_ratio: false,
            auto_validate: true,
            allow_blind_upload: true,
            upload_dupe_check: 4,
            import_descriptions: true,
            description_files: vec!["FILE_ID.DIZ".into(), "DESC.SDI".into()],
            no_description: "No description".into(),
            wrap_descriptions: true,
            advance_bar_on_flag: true,
            max_upload_bytes: 100 * 1024 * 1024,
            transfer_cps: 500_000,
        }
    }
}

/// An external transfer program, run with its stdin/stdout connected to
/// the caller (like lrzsz's `sz`/`rz`). `%F` in `send` is replaced by the
/// file paths; `receive` runs in the upload directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    pub key: char,
    pub desc: String,
    #[serde(default)]
    pub acs: String,
    /// Can send several files at once.
    #[serde(default)]
    pub batch: bool,
    pub send: String,
    pub receive: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Level {
    pub desc: String,
    pub sl: u8,
    pub dsl: u8,
    /// Minutes per day (or per call).
    pub time_limit: u16,
    pub file_points: u16,
    pub post_call: u16,
    pub limit_dl: u16,
    pub limit_dl_kb: u16,
    pub note: String,
    pub uldl_ratio: u16,
    pub kb_ratio: u16,
}

impl Default for Bbs {
    fn default() -> Self {
        Self {
            name: "Iniquity".into(),
            location: String::new(),
            phone: String::new(),
            sysop_alias: "SysOp".into(),
            sysop_name: String::new(),
            max_nodes: 16,
            inactivity_seconds: 300,
        }
    }
}

impl Default for Ssh {
    fn default() -> Self {
        Self { listen: "0.0.0.0:2222".into(), host_key_dir: "data".into(), legacy_algorithms: true }
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            text: "text".into(),
            menus: "menus".into(),
            data: "data".into(),
            strings: "strings.toml".into(),
            msgareas: "msgareas.toml".into(),
            fileareas: "fileareas.toml".into(),
            protocols: "protocols.toml".into(),
            archivers: "archivers.toml".into(),
            levels: "levels.toml".into(),
            infoforms: "infoforms.toml".into(),
            temp: "data/temp".into(),
        }
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            charset: CharsetPref::Auto,
            cp437_terms: ["syncterm", "ansi-bbs", "ansi", "pc-ansi", "pcansi", "scoansi", "netrunner"]
                .map(String::from)
                .to_vec(),
        }
    }
}

impl Default for Acs {
    fn default() -> Self {
        Self { sysop: "s255".into(), cosysop: "s250".into() }
    }
}

impl Default for Logon {
    fn default() -> Self {
        Self {
            matrix: true,
            ask_apply: true,
            tries: 5,
            system_password: String::new(),
            system_password_acs: "s250".into(),
            start_menu: "STARTUP".into(),
            real_names: false,
            time_per_call: false,
            last_callers: 10,
        }
    }
}

impl Default for NewUser {
    fn default() -> Self {
        Self {
            password: String::new(),
            verify: true,
            alias_format: 1,
            page_length: 24,
            ask_page_length: true,
            hot_keys: true,
            ask_hot_keys: true,
            expert: false,
            ask_expert: true,
            pause: true,
            ask_pause: true,
            quote: true,
            ask_quote: true,
            yes_no_bars: true,
            ask_yes_no_bars: true,
            ask_real_name: true,
            ask_birth_date: true,
            ask_phone: true,
            ask_address: true,
            ask_location: true,
            ask_sex: true,
        }
    }
}

impl Default for Prompts {
    fn default() -> Self {
        Self {
            yes: "|U5[|U6Y|U5/|U4n|U5] |U6".into(),
            no: "|U5[|U4y|U5/|U6N|U5] |U6".into(),
            yes_word: "Yes".into(),
            no_word: "No".into(),
            yes_bar: "|U8 Yes |U7 No ".into(),
            no_bar: "|U7 Yes |U8 No |U1".into(),
            password_echo: '■',
            remove_pause: true,
        }
    }
}

impl Default for Level {
    fn default() -> Self {
        Self {
            desc: String::new(),
            sl: 25,
            dsl: 25,
            time_limit: 25,
            file_points: 0,
            post_call: 0,
            limit_dl: 0,
            limit_dl_kb: 0,
            note: String::new(),
            uldl_ratio: 0,
            kb_ratio: 0,
        }
    }
}

/// The original's built-in levels (`levelLoad` with no LEVELS.DAT).
pub fn default_levels() -> BTreeMap<String, Level> {
    let l = |desc: &str, sl, time_limit, file_points, post_call, limit_dl, limit_dl_kb, note: &str, uldl_ratio, kb_ratio| Level {
        desc: desc.into(),
        sl,
        dsl: sl,
        time_limit,
        file_points,
        post_call,
        limit_dl,
        limit_dl_kb,
        note: note.into(),
        uldl_ratio,
        kb_ratio,
    };
    BTreeMap::from([
        ("A".into(), l("New user access level", 25, 25, 0, 20, 5, 100, "New user access", 1, 10)),
        ("B".into(), l("Normal user access level", 50, 60, 50, 20, 10, 600, "Normal access", 5, 15)),
        ("C".into(), l("Enhanced user access level", 100, 80, 150, 10, 15, 1500, "Enhanced access", 10, 30)),
        ("D".into(), l("Co-SysOp access level", 250, 120, 200, 0, 20, 5000, "Co-SysOp access", 30, 100)),
        ("E".into(), l("SysOp access level", 255, 64000, 64000, 0, 64000, 64000, "SysOp access", 64000, 64000)),
    ])
}

impl Config {
    /// Load `path`, or use defaults if it doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut cfg: Config = match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!("{} not found, using defaults", path.display());
                Config::default()
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if cfg.levels.is_empty() {
            cfg.levels = default_levels();
        }
        cfg.root = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(cfg)
    }

    pub fn resolve(&self, p: &Path) -> PathBuf {
        self.root.join(p)
    }

    pub fn level(&self, key: char) -> Option<&Level> {
        self.levels.get(&key.to_string()).filter(|l| !l.desc.is_empty())
    }
}
