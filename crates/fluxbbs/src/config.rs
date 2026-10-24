//! `fluxbbs.toml`: the successor to Iniquity's `tCfgRec` / iniquity.dat and
//! LEVELS.DAT.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fluxbbs_term::Palette;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bbs: Bbs,
    pub ssh: Ssh,
    pub telnet: Telnet,
    pub web: Web,
    pub gallery: Gallery,
    pub security: Security,
    pub passwords: Passwords,
    pub paths: Paths,
    pub terminal: Terminal,
    pub acs: Acs,
    pub logon: Logon,
    pub newuser: NewUser,
    pub prompts: Prompts,
    pub messages: Messages,
    pub files: Files,
    pub nuv: Nuv,
    pub dos: Dos,
    pub qwk: Qwk,
    pub chat: Chat,
    pub wfc: Wfc,
    pub ai: Ai,
    pub meshcore: MeshCore,
    pub ftn: Ftn,
    pub bbslink: BbsLink,
    pub slots: Slots,
    /// External transfer protocols, in addition to built-in Zmodem. Only
    /// used until protocols.toml exists (the protocol editor writes it).
    #[serde(rename = "protocol", default, skip_serializing_if = "Vec::is_empty")]
    pub protocols: Vec<Protocol>,
    /// Default colour scheme for new users and before logon.
    pub colors: Palette,
    /// New: what each user flag (A-Z) is for, shown in the flag editor.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub flags: BTreeMap<String, String>,
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
    /// New: the zone the board keeps time in (an IANA name such as
    /// `America/Chicago`); blank for the host's clock.
    pub time_zone: String,
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

/// Keeping bots out (see guard.rs and challenge.rs).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Security {
    /// Ask every caller to press a random key combination first.
    pub challenge: bool,
    /// How many keys, how long they have, and how many goes.
    pub challenge_keys: u8,
    pub challenge_seconds: u16,
    pub challenge_tries: u8,
    /// New: the bot check as three big characters drawn in colour in a
    /// field of noise, to be typed (else the keys to press, written out).
    pub challenge_visual: bool,
    /// Connections from one address at once, and new ones per minute
    /// (0 = no limit).
    pub max_per_ip: u32,
    pub max_per_minute: u32,
    /// Failed challenges or passwords before a ban (0 = never ban).
    pub fail_limit: u32,
    /// First ban length; each later ban for the same address doubles it
    /// (up to a day).
    pub ban_minutes: u32,
    /// Idle limit before logging in (after, `bbs.inactivity_seconds`).
    pub prelogin_idle_seconds: u64,
    /// Addresses never limited or challenged, and ones always refused:
    /// `1.2.3.4`, `10.0.` (a prefix) or `192.168.1.0/24`.
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

impl Default for Security {
    fn default() -> Self {
        Self {
            challenge: true,
            challenge_keys: 3,
            challenge_seconds: 30,
            challenge_tries: 3,
            challenge_visual: true,
            max_per_ip: 2,
            max_per_minute: 6,
            fail_limit: 3,
            ban_minutes: 15,
            prelogin_idle_seconds: 60,
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }
}

/// Password rules (new; the original asked only for four characters).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Passwords {
    pub min_length: u8,
    /// Refuse common passwords, the user's handle, and one character
    /// repeated.
    pub refuse_common: bool,
    /// Make a caller whose password breaks the rules choose a new one when
    /// they log in.
    pub enforce_at_login: bool,
}

impl Default for Passwords {
    fn default() -> Self {
        Self { min_length: 8, refuse_common: true, enforce_at_login: true }
    }
}

/// The art gallery (`AG`; new): a folder of art and art packs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gallery {
    /// Folder of art; ZIP packs in it open as folders.
    pub path: PathBuf,
    /// Or a file area (its storage key): the gallery is then that area,
    /// its sub-areas as folders and its files from the file list.
    pub area: String,
    /// Who may browse it, and who may download from it.
    pub acs: String,
    pub download_acs: String,
    /// Starting play speed in bits per second (0 = all at once).
    pub speed: u32,
}

impl Default for Gallery {
    fn default() -> Self {
        Self { path: "gallery".into(), area: String::new(), acs: String::new(), download_acs: "s25".into(), speed: 38400 }
    }
}

/// The status feed for a website (new): who's online, last callers,
/// new files and totals as JSON, read-only, at `/status.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Web {
    pub enabled: bool,
    /// Keep it on 127.0.0.1 and let the web server pass
    /// `/api/status.json` through to it.
    pub listen: String,
    /// A site allowed to fetch it from another address (CORS), e.g.
    /// `https://disfluxbbs.com`; empty when it's served from the same site.
    pub allow_origin: String,
    /// Show handles (otherwise "a caller").
    pub show_handles: bool,
    /// List the SysOp among who's online.
    pub show_sysop: bool,
    pub last_callers: u8,
    pub new_files: u8,
    /// The latest public posts (new).
    pub new_posts: u8,
}

impl Default for Web {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "127.0.0.1:8099".into(),
            allow_origin: String::new(),
            show_handles: true,
            show_sysop: true,
            last_callers: 10,
            new_files: 10,
            new_posts: 5,
        }
    }
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
    /// Screens to fall back on when `text` doesn't have one, so a board's
    /// own art can live apart from the stock screens. Empty for none.
    pub stock_text: PathBuf,
    /// A text library for any screen neither `text` nor `stock_text` has
    /// (e.g. `themes/t10inq1`, for SYSOP and DOOR). Empty for none.
    pub stock_theme: PathBuf,
    pub menus: PathBuf,
    /// Menus to fall back on when `menus` doesn't have one (new), so a
    /// board can change a few stock menus without copying them all. Empty
    /// for none.
    pub stock_menus: PathBuf,
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
    /// Screen themes (text libraries): the list, and their directories.
    pub themes: PathBuf,
    pub theme_dir: PathBuf,
    /// Scratch space for uploads in progress.
    pub temp: PathBuf,
    /// Door drop files, a directory per node (`pathDoor`).
    pub doors: PathBuf,
    /// Lua scripts (`-X`, `%!`), which replace IPL modules (`pathIPLX`).
    pub scripts: PathBuf,
    /// Scheduled events (`*V`).
    pub events: PathBuf,
    /// The logon sequence (`*O`).
    pub logon: PathBuf,
    /// Files attached to messages (`pathAtch`).
    pub attach: PathBuf,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CharsetPref {
    Auto,
    Cp437,
    Utf8,
}

/// AI chat (`AI`): an OpenAI-style chat completions endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ai {
    pub enabled: bool,
    /// Up to and including `/v1`, e.g. `http://localhost:8080/v1`.
    pub base_url: String,
    pub model: String,
    /// The environment variable holding an API key, if the server wants
    /// one (kept out of this file).
    pub api_key_env: String,
    /// Who may use it: as stock, level 250 and up or flag I, which the
    /// SysOp grants in the user editor.
    pub acs: String,
    /// What the AI is called on screen and in its prompt.
    pub name: String,
    /// Added to the built-in system prompt.
    pub system_prompt: String,
    pub max_tokens: u32,
    /// `reasoning_effort` for models that think first (none, low...);
    /// empty for the server's default.
    pub reasoning_effort: String,
    /// Messages a caller may send a day (0 = no limit; SysOps have none).
    pub daily_limit: u32,
    /// New: tarot readings and fortunes a caller may have a day (0 = no
    /// limit; SysOps have none).
    pub tarot_daily: u32,
    pub fortune_daily: u32,
    /// Let the model search the web...
    pub web_search: bool,
    /// ...and read pages it finds (public addresses only).
    pub read_pages: bool,
    /// A SearXNG instance to search with...
    pub search_url: String,
    /// ...else the environment variable holding a Brave Search API key;
    /// with neither, DuckDuckGo (which turns away bots now and then).
    pub search_key_env: String,
    pub timeout_seconds: u64,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            model: String::new(),
            api_key_env: String::new(),
            acs: "s250|fI".into(),
            name: "ai".into(),
            system_prompt: String::new(),
            max_tokens: 1000,
            reasoning_effort: String::new(),
            daily_limit: 100,
            tarot_daily: 3,
            fortune_daily: 3,
            web_search: true,
            read_pages: true,
            search_url: String::new(),
            search_key_env: String::new(),
            timeout_seconds: 120,
        }
    }
}

/// The slot machine (new, `slots.rs`): pulls a day, how often a pull
/// wins, and what each line pays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Slots {
    /// 0: no limit.
    pub pulls_per_day: u32,
    /// The chance, in percent, that a pull wins something.
    pub win_percent: u32,
    /// Three sevens come up one pull in this many (0: never).
    pub jackpot_odds: u32,
    pub jackpot: Prize,
    pub bars: Prize,
    pub bells: Prize,
    pub plums: Prize,
    pub oranges: Prize,
    pub lemons: Prize,
    pub cherries: Prize,
    pub two_cherries: Prize,
    pub one_cherry: Prize,
}

/// What a win pays: minutes on today's time, and file points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Prize {
    pub minutes: u32,
    pub points: u32,
}

impl Default for Slots {
    fn default() -> Self {
        let p = |minutes, points| Prize { minutes, points };
        Self {
            pulls_per_day: 10,
            win_percent: 40,
            jackpot_odds: 200,
            jackpot: p(60, 100),
            bars: p(30, 40),
            bells: p(20, 25),
            plums: p(15, 15),
            oranges: p(10, 12),
            lemons: p(8, 10),
            cherries: p(10, 10),
            two_cherries: p(5, 5),
            one_cherry: p(2, 2),
        }
    }
}

/// BBSLink (new): door games on bbslink.net's server, played from here
/// (`bbslink.rs`, menu command `DB`). The three codes come from signing
/// the board up at bbslink.net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BbsLink {
    pub enabled: bool,
    /// The door server, for its web handshake (port 80) and telnet (23).
    pub host: String,
    pub system_code: String,
    pub auth_code: String,
    pub scheme_code: String,
}

impl Default for BbsLink {
    fn default() -> Self {
        Self { enabled: false, host: "games.bbslink.net".into(), system_code: String::new(), auth_code: String::new(), scheme_code: String::new() }
    }
}

/// MeshCore (new): a companion radio's channels bridged to message areas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MeshCore {
    pub enabled: bool,
    /// `host:port` of the companion radio: WiFi companion firmware listens
    /// on 5000; a USB radio can be put on the network with ser2net.
    pub address: String,
    /// Seconds between attempts while the radio can't be reached.
    pub reconnect_seconds: u64,
    /// Messages a long post may be split into (each fits in one packet).
    pub max_parts: u8,
    #[serde(rename = "channel", default)]
    pub channels: Vec<MeshChannel>,
}

/// One radio channel and the message area it's bridged to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshChannel {
    /// The channel's name on the radio ("Public").
    pub channel: String,
    /// The message area's storage key.
    pub area: String,
    /// Post what's said on the channel in the area.
    #[serde(default = "yes")]
    pub from_mesh: bool,
    /// Send what's posted in the area to the channel.
    #[serde(default = "yes")]
    pub to_mesh: bool,
}

fn yes() -> bool {
    true
}

impl Default for MeshCore {
    fn default() -> Self {
        Self { enabled: false, address: String::new(), reconnect_seconds: 30, max_parts: 2, channels: Vec::new() }
    }
}

/// FTN echomail (new): a node of a network such as fsxNet, polling its
/// hub over binkp (`ftntoss.rs`, `binkp.rs`). The address, hub and
/// passwords here are the first network's; `[[ftn.network]]` adds more,
/// each with its own hub, all through the one mailer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ftn {
    pub enabled: bool,
    /// The first network's name here (letters, digits, `-`, `_`); blank
    /// for its address's domain. Its outbound is `data/ftn/outbound/NAME`.
    pub name: String,
    /// This board's address, with the network's domain: `21:1/999@fsxnet`.
    pub address: String,
    /// The hub's address, and where its binkp mailer listens (`host:port`;
    /// 24554 if no port).
    pub hub: String,
    pub hub_host: String,
    /// The binkp session password, packet password and AreaFix password
    /// the hub gave (AreaFix's, if blank, is the session password).
    pub session_password: String,
    pub packet_password: String,
    pub areafix_password: String,
    /// The origin line's text; blank for the board's name.
    pub origin: String,
    /// Minutes between polls of the hub (0: only by hand, `fluxbbs ftn
    /// poll`).
    pub poll_minutes: u32,
    /// Where to answer the hub's binkp calls (`0.0.0.0:24554`), so it can
    /// send mail as it arrives ("crash"); blank: poll only ("hold").
    /// Takes effect at restart.
    pub listen: String,
    /// Echo areas: the network's tag and the message area it's tossed to.
    #[serde(rename = "area", default)]
    pub areas: Vec<FtnArea>,
    /// More networks than the first.
    #[serde(rename = "network", default, skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<FtnNetwork>,
}

/// One echo and the message area it lives in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FtnArea {
    /// The echo's tag on the network (`FSX_GEN`).
    pub tag: String,
    /// The message area's storage key.
    pub area: String,
    /// Which network it's on (its name); blank for the first.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub network: String,
}

/// A network this board is a node of: the first (from `[ftn]`), or one
/// from `[[ftn.network]]`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FtnNetwork {
    /// What it's called here (`agoranet`); blank for the address's domain.
    pub name: String,
    pub address: String,
    pub hub: String,
    pub hub_host: String,
    pub session_password: String,
    pub packet_password: String,
    pub areafix_password: String,
    /// The origin line's text; blank for `[ftn] origin`.
    pub origin: String,
}

impl FtnNetwork {
    /// AreaFix's password: its own, else the session password.
    pub fn areafix_pw(&self) -> &str {
        if self.areafix_password.is_empty() { &self.session_password } else { &self.areafix_password }
    }
}

/// A network's name from its address's domain (`fidonet.org` is
/// `fidonet-org`), or `ftn` / `netN` without one.
fn derived_name(address: &str, i: usize) -> String {
    match address.split_once('@') {
        Some((_, d)) if !d.trim().is_empty() => d.trim().to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' }).collect(),
        _ if i == 0 => "ftn".to_string(),
        _ => format!("net{i}"),
    }
}

impl Ftn {
    /// Every network in the file that has a hub, set up right or not:
    /// a caller claiming one that's left out is a mistake here, not theirs.
    pub fn all_hubs(&self) -> Vec<String> {
        self.checked().into_iter().map(|(n, _)| n.hub).filter(|h| !h.trim().is_empty()).collect()
    }

    /// The networks set up (with an address and a usable name and hub),
    /// the first first, each named.
    pub fn networks(&self) -> Vec<FtnNetwork> {
        self.checked().into_iter().filter_map(|(n, why)| why.is_none().then_some(n)).filter(|n| !n.address.trim().is_empty()).collect()
    }

    /// What's wrong with the FTN set-up, for the log: networks left out,
    /// and echoes on no network or on two.
    pub fn problems(&self) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }
        let checked = self.checked();
        let mut out: Vec<String> = checked.iter().filter_map(|(n, why)| why.as_ref().map(|w| format!("network {:?} left out: {w}", n.name))).collect();
        // Any network in the file, set up yet or not (one waiting for its
        // address is fine).
        let names: Vec<String> = checked.into_iter().map(|(n, _)| n.name).collect();
        let mut seen: std::collections::HashMap<String, String> = Default::default();
        for a in &self.areas {
            let on = self.network_of(a);
            if !names.iter().any(|n| n.eq_ignore_ascii_case(&on)) {
                out.push(format!("echo {} ({}) is on network {on:?}, which there's none of", a.tag, a.area));
            }
            if let Some(other) = seen.insert(a.area.to_ascii_uppercase(), on.clone())
                && !other.eq_ignore_ascii_case(&on)
            {
                out.push(format!("area {} is on two networks ({other}, {on}); its posts go out on only one", a.area));
            }
        }
        out
    }

    /// Every network in the file, named, with why it can't be used.
    fn checked(&self) -> Vec<(FtnNetwork, Option<String>)> {
        let first = FtnNetwork {
            name: self.name.clone(),
            address: self.address.clone(),
            hub: self.hub.clone(),
            hub_host: self.hub_host.clone(),
            session_password: self.session_password.clone(),
            packet_password: self.packet_password.clone(),
            areafix_password: self.areafix_password.clone(),
            origin: self.origin.clone(),
        };
        let mut out: Vec<(FtnNetwork, Option<String>)> = Vec::new();
        for (i, mut n) in std::iter::once(first).chain(self.networks.iter().cloned()).enumerate() {
            n.name = n.name.trim().to_ascii_lowercase();
            if n.name.is_empty() {
                n.name = derived_name(&n.address, i);
            }
            if n.origin.trim().is_empty() {
                n.origin = self.origin.clone();
            }
            // The name is a directory and a lock file's name.
            let why = if !n.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                Some("its name may only have letters, digits, - and _".to_string())
            } else if !n.address.trim().is_empty() && out.iter().any(|(o, why)| why.is_none() && !o.address.trim().is_empty() && o.name == n.name) {
                Some("another network has that name".to_string())
            } else if !n.address.trim().is_empty() && crate::ftn::Addr::parse(&n.address).is_none() {
                Some(format!("address {:?} isn't an FTN address", n.address))
            } else if !n.address.trim().is_empty() && crate::ftn::Addr::parse(&n.hub).is_none() {
                Some(format!("hub {:?} isn't an FTN address", n.hub))
            } else {
                None
            };
            out.push((n, why));
        }
        out
    }

    /// The first network's name (what an echo with none is on), set up
    /// or not.
    pub fn first_name(&self) -> String {
        let name = self.name.trim().to_ascii_lowercase();
        if !name.is_empty() {
            return name;
        }
        derived_name(&self.address, 0)
    }

    /// The network an echo is on.
    pub fn network_of(&self, a: &FtnArea) -> String {
        if a.network.trim().is_empty() { self.first_name() } else { a.network.trim().to_ascii_lowercase() }
    }

    /// The echoes on network `name`.
    pub fn areas_of(&self, name: &str) -> Vec<FtnArea> {
        self.areas.iter().filter(|a| self.network_of(a).eq_ignore_ascii_case(name)).cloned().collect()
    }
}

impl Default for Ftn {
    fn default() -> Self {
        Self {
            enabled: false,
            name: String::new(),
            address: String::new(),
            hub: String::new(),
            hub_host: String::new(),
            session_password: String::new(),
            packet_password: String::new(),
            areafix_password: String::new(),
            origin: String::new(),
            poll_minutes: 60,
            listen: String::new(),
            areas: Vec::new(),
            networks: Vec::new(),
        }
    }
}

/// The waiting-for-caller screen (`fluxbbs --wfc`): the original's
/// "Wait-For-Call Configuration".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Wfc {
    /// ESC at the screen shuts the BBS down (`ESCtoExit`).
    pub esc_to_exit: bool,
    /// Blank the screen after this many idle seconds (0 = never).
    pub blank_seconds: u32,
    /// Stats shown at first (`DefWFCstat`): 1 system information, 3 system
    /// totals, 4 today's statistics, 5 last few callers, 8 Iniquity
    /// information.
    pub default_stat: u8,
}

impl Default for Wfc {
    fn default() -> Self {
        Self { esc_to_exit: true, blank_seconds: 180, default_stat: 8 }
    }
}

/// Paging the SysOp (`CP`): the original's chat mode setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Chat {
    /// Hours a SysOp who's online can be paged (HH:MM; equal = always).
    pub start: String,
    pub end: String,
    /// Who may page outside those hours, and the password they need
    /// (blank = none).
    pub override_acs: String,
    pub override_password: String,
    /// Pages per call (0 = no limit), and how many five-second rounds a
    /// page lasts.
    pub max_page_times: u8,
    pub page_beeps: u8,
    /// Give the caller back the time a chat took.
    pub restore_time: bool,
    /// Offer to leave the SysOp email when a page goes unanswered.
    pub ask_email: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            start: "11:00".into(),
            end: "20:00".into(),
            override_acs: "s150".into(),
            override_password: String::new(),
            max_page_times: 3,
            page_beeps: 6,
            restore_time: true,
            ask_email: true,
        }
    }
}

/// QWK offline mail (`QD`, `QU`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Qwk {
    /// Packet name (the BBSID): packets are NAME.QWK, replies NAME.REP.
    pub packet: String,
    /// Screens from the text directory put in each packet.
    pub welcome: String,
    pub news: String,
    pub goodbye: String,
    /// Leave auto-signatures out of exported messages.
    pub strip_sigs: bool,
}

impl Default for Qwk {
    fn default() -> Self {
        Self {
            packet: "FLUXBBS".into(),
            welcome: "WELCOME.ANS".into(),
            news: "NEWS.ANS".into(),
            goodbye: "LOGOFF.ANS".into(),
            strip_sigs: true,
        }
    }
}

/// DOS doors: door commands naming a .BAT, .COM or .EXE run under DOSBox,
/// with COM1 wired to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dos {
    /// Empty: the built-in machine runs DOS doors. Otherwise a DOSBox
    /// command (through `sh -c`), %C being the config written for each
    /// door, for a door the built-in one can't run.
    pub emulator: String,
    /// Mounted as C:, on the PATH: the doors and their batch files.
    pub drive: PathBuf,
    /// DOS commands run before each door, e.g. a FOSSIL driver.
    pub setup: Vec<String>,
    /// Extra lines for the emulator's config (`[cpu]`, `cycles=max`).
    pub config: Vec<String>,
    /// Doors only one caller may be in at a time, by program or batch
    /// file (`TW2002.BAT`): single-player games, and unregistered copies
    /// that run on one node.
    pub one_at_a_time: Vec<String>,
}

impl Default for Dos {
    fn default() -> Self {
        Self {
            emulator: String::new(),
            drive: "doors/dos".into(),
            setup: Vec::new(),
            config: Vec::new(),
            one_at_a_time: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Terminal {
    pub charset: CharsetPref,
    /// TERM values that get raw CP437 when `charset = "auto"`.
    pub cp437_terms: Vec<String>,
    /// CP437 terminals that take 24-bit colour, which then carries bright
    /// backgrounds (others get the iCE blink bit, and blink if they don't
    /// honour the iCE switch).
    pub truecolor_terms: Vec<String>,
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
    /// Every this many calls, ask the caller their birth date / the last
    /// four digits of their phone number again (0 = never).
    pub calls_birth: u8,
    pub calls_phone: u8,
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
    /// Show the NEWCFG configuration screen after applying, so new users
    /// can check their entries.
    pub config: bool,
    pub ask_real_name: bool,
    pub ask_birth_date: bool,
    pub ask_phone: bool,
    pub ask_address: bool,
    /// City, country/state and zip code.
    pub ask_location: bool,
    pub ask_sex: bool,
    /// New accounts allowed from one address in a day, and on the whole
    /// board in a day (0 = no limit).
    pub per_address_per_day: u32,
    pub per_day: u32,
    /// Handles nobody may apply with (ignoring case).
    pub reserved_handles: Vec<String>,
    /// Delete accounts still at the new-user level (A) after this many
    /// days that never called back (0 = keep them).
    pub purge_after_days: u32,
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
    /// Status-screen lights: working, done, failed.
    pub light: char,
    pub light_ok: char,
    pub light_fail: char,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Messages {
    pub acs_post_email: String,
    pub acs_anonymous: String,
    pub acs_anon_automsg: String,
    /// May append their auto-signature.
    pub acs_autosig: String,
    /// May attach a file to a public message, or to email.
    pub acs_attach_public: String,
    pub acs_attach_email: String,
    /// May upload a prepared message instead of typing one; whether to
    /// offer that for replies and for email (new posts are always asked).
    pub acs_upload_message: String,
    pub ask_upload_reply: bool,
    pub ask_upload_email: bool,
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
    /// New: who may create a board of their own (blank: nobody).
    pub board_create_acs: String,
    /// New: the storage key of the area callers' boards go under (blank:
    /// the top level); they take its access.
    pub board_folder: String,
    /// New: boards each caller may run.
    pub boards_per_caller: u32,
    /// New: a caller's board with no posts this many days after it was
    /// made goes when `pack-messages` runs (0: never).
    pub board_empty_days: u32,
    /// New: points for filling a request on a request board (also file
    /// points, where the board uses them).
    pub fill_points: u32,
    /// New: minutes after posting in which an author may take a message
    /// back outright, while nobody has answered it (0: never).
    pub take_back_minutes: u32,
    /// New: file points an author gives up to burn a message for good
    /// (0, or a board without file points: free).
    pub burn_points: u32,
}

impl Default for Messages {
    fn default() -> Self {
        Self {
            acs_post_email: "s25".into(),
            acs_anonymous: "s50".into(),
            acs_anon_automsg: "s50".into(),
            acs_autosig: "s25".into(),
            acs_attach_public: "s50".into(),
            acs_attach_email: "s50".into(),
            acs_upload_message: "s25".into(),
            ask_upload_reply: false,
            ask_upload_email: true,
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
            board_create_acs: String::new(),
            board_folder: String::new(),
            boards_per_caller: 1,
            board_empty_days: 30,
            fill_points: 10,
            take_back_minutes: 15,
            burn_points: 25,
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
    /// Take descriptions from these files inside uploaded archives, or
    /// from the SAUCE of a single piece of art.
    pub import_descriptions: bool,
    pub description_files: Vec<String>,
    /// Most lines of an imported description to keep (2.0); 0 keeps them
    /// all (up to the lister's 20).
    pub max_import_lines: u16,
    pub no_description: String,
    /// Long descriptions continue on the next page of the file lister.
    pub wrap_descriptions: bool,
    /// Flagging a file moves the lister's bar down.
    pub advance_bar_on_flag: bool,
    /// Largest accepted upload, in bytes (0 = no limit).
    pub max_upload_bytes: u64,
    /// Bytes per second assumed when estimating transfer time.
    pub transfer_cps: u64,
    /// Offer to log the caller off when a download finishes, and how
    /// long the countdown is.
    pub ask_auto_logoff: bool,
    pub auto_logoff_seconds: u16,
    /// Upload tests: an outside command run on each upload (`%F`; blank
    /// = none) and the exit code that means it passed; refuse archives
    /// with files older than this many years (0 = don't), every file or
    /// just the newest; files in the data directory listing names to
    /// strip from uploaded ZIPs and holding the comment to put in them.
    pub test_command: String,
    pub test_success_code: i32,
    pub age_limit_years: u8,
    pub strict_age: bool,
    pub delete_list: String,
    pub archive_comment: String,
    /// File in the data directory added to every uploaded ZIP under its own
    /// name (an ad for the board); empty = off.
    pub bbs_ad: String,
    /// Upload types allowed, as suffixes (`zip`, `tar.gz`); empty = any.
    /// The contents have to match the name too.
    pub allowed_types: Vec<String>,
    /// Where the summary of what's inside goes in the description.
    pub scan_summary: SummaryAt,
    /// File names from inside an archive to list under the summary.
    pub scan_names: u8,
    /// Refuse archives that unpack to more than this many times their
    /// size (past a megabyte), or more than `max_unpacked_bytes` (0 = no
    /// limit for either).
    pub max_unpacked_ratio: u32,
    pub max_unpacked_bytes: u64,
    /// Pictures (GIF, PNG, JPEG): refuse uploads bigger than this many
    /// bytes, pixels a frame, or GIF frames (0 = no limit).
    pub max_image_bytes: u64,
    pub max_image_pixels: u64,
    pub max_gif_frames: u32,
    /// Refuse archives with names that point outside the folder they'd
    /// unpack into (`../`, `/etc/...`).
    pub refuse_unsafe_paths: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SummaryAt {
    Off,
    First,
    Last,
}

impl Files {
    pub fn image_limits(&self) -> crate::picture::Limits {
        crate::picture::Limits { max_bytes: self.max_image_bytes, max_pixels: self.max_image_pixels, max_frames: self.max_gif_frames as usize }
    }
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
            max_import_lines: 0,
            no_description: "No description".into(),
            wrap_descriptions: true,
            advance_bar_on_flag: true,
            max_upload_bytes: 100 * 1024 * 1024,
            transfer_cps: 500_000,
            ask_auto_logoff: true,
            auto_logoff_seconds: 10,
            test_command: String::new(),
            test_success_code: 0,
            age_limit_years: 0,
            strict_age: false,
            delete_list: String::new(),
            archive_comment: String::new(),
            bbs_ad: String::new(),
            allowed_types: Vec::new(),
            scan_summary: SummaryAt::Last,
            scan_names: 0,
            max_unpacked_ratio: 100,
            max_unpacked_bytes: 4 << 30,
            max_image_bytes: 10 << 20,
            max_image_pixels: 2048 * 2048,
            max_gif_frames: 300,
            refuse_unsafe_paths: true,
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
    /// Time bank: most minutes that can be kept, put in a day, taken out
    /// a day; and the same for download kilobytes. 0 closes that side.
    pub bank_time_max: u16,
    pub bank_time_deposit: u16,
    pub bank_time_withdraw: u16,
    pub bank_kb_max: u16,
    pub bank_kb_deposit: u16,
    pub bank_kb_withdraw: u16,
}

impl Default for Bbs {
    fn default() -> Self {
        Self {
            name: "flux/bbs".into(),
            location: String::new(),
            phone: String::new(),
            sysop_alias: "SysOp".into(),
            sysop_name: String::new(),
            max_nodes: 16,
            inactivity_seconds: 300,
            time_zone: String::new(),
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
            stock_text: PathBuf::new(),
            stock_menus: PathBuf::new(),
            stock_theme: PathBuf::new(),
            menus: "menus".into(),
            data: "data".into(),
            strings: "strings.toml".into(),
            msgareas: "msgareas.toml".into(),
            fileareas: "fileareas.toml".into(),
            protocols: "protocols.toml".into(),
            archivers: "archivers.toml".into(),
            levels: "levels.toml".into(),
            infoforms: "infoforms.toml".into(),
            themes: "themes.toml".into(),
            theme_dir: "themes".into(),
            temp: "data/temp".into(),
            doors: "data/doors".into(),
            scripts: "scripts".into(),
            events: "events.toml".into(),
            logon: "logon.toml".into(),
            attach: "data/attach".into(),
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
            truecolor_terms: vec!["syncterm".into()],
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
            calls_birth: 10,
            calls_phone: 5,
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
            config: true,
            ask_birth_date: true,
            ask_phone: true,
            ask_address: true,
            ask_location: true,
            ask_sex: true,
            per_address_per_day: 2,
            per_day: 10,
            reserved_handles: ["sysop", "cosysop", "admin", "administrator", "root", "guest", "new", "all", "staff", "moderator", "system", "iniquity"]
                .map(String::from)
                .to_vec(),
            purge_after_days: 0,
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
            light: '■',
            light_ok: '√',
            light_fail: 'x',
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
            bank_time_max: 60,
            bank_time_deposit: 30,
            bank_time_withdraw: 30,
            bank_kb_max: 1000,
            bank_kb_deposit: 500,
            bank_kb_withdraw: 500,
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
        ..Level::default()
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
        // Every command keeps board time from here on.
        crate::clock::set_board(&cfg.bbs.time_zone);
        Ok(cfg)
    }

    pub fn resolve(&self, p: &Path) -> PathBuf {
        self.root.join(p)
    }

    /// The database, `data/fluxbbs.db` (`iniquity.db` on boards from
    /// before the rename).
    pub fn db_path(&self) -> PathBuf {
        renamed(&self.resolve(&self.paths.data), "fluxbbs.db", "iniquity.db")
    }

    /// The WFC screen's log, `data/fluxbbs.log` (or `iniquity.log`).
    pub fn log_path(&self) -> PathBuf {
        renamed(&self.resolve(&self.paths.data), "fluxbbs.log", "iniquity.log")
    }

    /// Where menus are looked for: `menus`, then `stock_menus`.
    pub fn menu_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.resolve(&self.paths.menus)];
        if !self.paths.stock_menus.as_os_str().is_empty() {
            dirs.push(self.resolve(&self.paths.stock_menus));
        }
        dirs
    }

    pub fn level(&self, key: char) -> Option<&Level> {
        self.levels.get(&key.to_string()).filter(|l| !l.desc.is_empty())
    }
}

/// The configuration file when none is named: `fluxbbs.toml`, or
/// `iniquity.toml` on a board from before the rename.
pub fn default_path() -> PathBuf {
    renamed(Path::new("."), "fluxbbs.toml", "iniquity.toml").file_name().map_or_else(|| "fluxbbs.toml".into(), PathBuf::from)
}

/// `dir/new`, unless only the old name is there.
fn renamed(dir: &Path, new: &str, old: &str) -> PathBuf {
    let (n, o) = (dir.join(new), dir.join(old));
    if !n.exists() && o.exists() { o } else { n }
}


