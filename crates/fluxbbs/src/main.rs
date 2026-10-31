//! flux/bbs: a BBS based on Iniquity (Mike Fricker and the Iniquity
//! Development Team, 1995-97).
//!
//!     fluxbbs [--wfc] [fluxbbs.toml]           run the BBS (--wfc: with the
//!                                               waiting-for-caller screen)
//!     fluxbbs import <old-install> [dest]      convert a DOS install
//!     fluxbbs add-files [fluxbbs.toml]         add files put in areas by hand

mod acs;
mod ambient;
mod clock;
#[cfg(feature = "ai")]
mod aiboard;
#[cfg(feature = "ai")]
mod aichat;
mod aimemory;
mod aidesc;
#[cfg(feature = "ai")]
mod ansishot;
mod backup;
#[cfg(feature = "doors")]
mod bbslink;
#[cfg(feature = "tarot")]
mod tarot;
#[cfg(feature = "tarot")]
mod tarotart;
mod fontview;
mod findpage;
mod tdf;
#[cfg(feature = "casino")]
mod blackjack;
#[cfg(feature = "carrierlost")]
mod carrierart;
#[cfg(feature = "carrierlost")]
mod carrierlost;
#[cfg(feature = "casino")]
mod casino;
#[cfg(feature = "casino")]
mod casinobar;
#[cfg(feature = "casino")]
mod craps;
#[cfg(feature = "casino")]
mod lottery;
#[cfg(feature = "casino")]
mod poker;
#[cfg(any(feature = "casino", feature = "carrierlost", feature = "effects", feature = "weather"))]
mod pixels;
#[cfg(feature = "casino")]
mod roulette;
#[cfg(feature = "casino")]
mod slotart;
#[cfg(feature = "casino")]
mod slots;
mod links;
mod regret;
mod archview;
mod attach;
#[cfg(feature = "ftn")]
mod binkp;
mod boards;
mod commands;
mod cfgedit;
mod challenge;
mod chat;
mod config;
#[cfg(feature = "doors")]
mod doors;
mod doorstats;
mod engine;
mod events;
mod extprog;
mod gallery;
mod guard;
mod import;
mod infoform;
mod input;
mod logon;
mod logonseq;
mod markup;
mod mci;
mod mentions;
mod menu;
mod menuedit;
#[cfg(feature = "meshcore")]
mod meshcore;
#[cfg(feature = "mrc")]
mod mrc;
#[cfg(feature = "mrc")]
mod mrcchat;
mod messages;
mod msgareas;
mod multinode;
mod msgbase;
mod msgcenter;
mod editor;
#[cfg(feature = "email")]
mod emailver;
mod extras;
mod features;
#[cfg(feature = "screensavers")]
mod fontpick;
#[cfg(feature = "screensavers")]
mod savers;
mod fileareas;
#[cfg(feature = "ftn")]
mod ftn;
#[cfg(feature = "ftn")]
mod ftntic;
#[cfg(feature = "ftn")]
mod netmail;
#[cfg(feature = "ftn")]
mod nodelist;
#[cfg(feature = "ftn")]
mod ftntoss;
mod filebase;
mod filesync;
mod files;
mod transfer;
mod trash;
#[cfg(feature = "effects")]
mod tvoff;
mod upltest;
mod upscan;
#[cfg(feature = "weather")]
mod weather;
mod web;
mod newposts;
mod nodes;
mod html;
mod pager;
#[cfg(feature = "pdf")]
mod pdf;
mod plain;
mod plainmsg;
mod profile;
mod points;
mod pushes;
mod picture;
mod password;
mod questions;
mod qwk;
mod scansel;
#[cfg(feature = "lua")]
mod scripts;
mod session;
mod ssh;
mod sshkeys;
mod sysop;
mod telnet;
#[cfg(feature = "textfiles")]
mod textfiles;
mod themes;
mod timebank;
mod strings;
mod sysconfig;
mod sysdata;
mod sysmisc;
mod screenview;
mod user;
mod useredit;
mod users;
mod voting;
mod wfc;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use config::Config;
use fileareas::FileAreas;
use filebase::FileBase;
use msgareas::MsgAreas;
use msgbase::MsgBase;
use nodes::Nodes;
use strings::Strings;
use sysdata::SysData;
use users::UserStore;

/// Posts on their way to the MeshCore bridge: the bridge's outbox, or
/// without it built in (`meshcore`) one that lets them go.
#[cfg(feature = "meshcore")]
pub type MeshOutbox = meshcore::Outbox;

#[cfg(not(feature = "meshcore"))]
#[derive(Default)]
pub struct MeshOutbox;

#[cfg(not(feature = "meshcore"))]
impl MeshOutbox {
    pub fn post(&self, _m: &msgbase::Message) {}
}


/// State shared by every session.
pub struct BbsState {
    /// Swapped whole by the system configuration editor.
    config: std::sync::RwLock<Arc<Config>>,
    /// Where the configuration came from (the editor writes it back).
    pub config_path: PathBuf,
    /// Swapped whole by the string editor.
    strings: std::sync::RwLock<Arc<Strings>>,
    pub users: UserStore,
    /// Swapped whole by the SysOp's message area and conference editors.
    areas: std::sync::RwLock<Arc<MsgAreas>>,
    pub msgs: MsgBase,
    /// Swapped whole by the SysOp's file area editor.
    fareas: std::sync::RwLock<Arc<FileAreas>>,
    protocols: std::sync::RwLock<Arc<extprog::Protocols>>,
    archivers: std::sync::RwLock<Arc<extprog::Archivers>>,
    levels: std::sync::RwLock<Arc<extprog::Levels>>,
    infoforms: std::sync::RwLock<Arc<infoform::Infoforms>>,
    questions: std::sync::RwLock<Arc<questions::Questions>>,
    themes: std::sync::RwLock<Arc<themes::Themes>>,
    events: std::sync::RwLock<Arc<events::Events>>,
    /// Infoform answers and new user votes.
    pub forms: infoform::FormStore,
    /// Carrier Lost's world (`AP`).
    #[cfg(feature = "carrierlost")]
    pub carrier: carrierlost::CarrierStore,
    pub files: FileBase,
    pub sys: SysData,
    /// Serialises appends to shared text files (`?T`).
    pub text_lock: std::sync::Mutex<()>,
    pub nodes: Arc<Nodes>,
    /// Connection limits, failure counts and bans.
    pub guard: Arc<guard::Guard>,
    /// AI chat messages today: (user, day) -> count.
    pub ai_usage: std::sync::Mutex<std::collections::HashMap<(u32, chrono::NaiveDate), u32>>,
    /// Posts on their way to the MeshCore bridge.
    pub mesh: MeshOutbox,
    /// New: download links given out (`web.rs`).
    pub links: web::Links,
    /// New: the board's MRC connection (`mrc.rs`).
    #[cfg(feature = "mrc")]
    pub mrc: mrc::Hub,
    /// Codes out to confirm callers' email addresses (`UV`).
    #[cfg(feature = "email")]
    pub email: emailver::CodeStore,
    /// Places callers picked for the weather, and recent forecasts.
    #[cfg(feature = "weather")]
    pub weather: weather::WeatherStore,
    /// The `[dos] one_at_a_time` doors someone is in.
    pub doors_in_use: std::sync::Mutex<std::collections::HashSet<String>>,
    /// When the board started (its clock).
    pub started: chrono::NaiveDateTime,
}

impl BbsState {
    pub fn strings(&self) -> Arc<Strings> {
        self.strings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Save the strings that differ from stock to strings.toml and start
    /// using them.
    pub fn set_strings(&self, new: Strings) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.strings), &strings::to_toml(&new.overrides()))?;
        *self.strings.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    /// The configuration as it is now.
    pub fn config(&self) -> Arc<Config> {
        self.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Start using a new configuration (already saved by the caller).
    pub fn set_config(&self, new: Config) {
        clock::set_board(&new.bbs.time_zone);
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
    }

    /// The message areas as they are now.
    pub fn areas(&self) -> Arc<MsgAreas> {
        self.areas.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Save new message areas to msgareas.toml and start using them.
    pub fn set_areas(&self, new: MsgAreas) -> anyhow::Result<()> {
        let path = self.config().resolve(&self.config().paths.msgareas);
        // Held across the write, so it and `update_areas` can't cross.
        let mut held = self.areas.write().unwrap_or_else(|e| e.into_inner());
        write_atomic(&path, &new.to_toml())?;
        *held = Arc::new(new);
        Ok(())
    }

    /// Change the message areas as they are now, in one go (no other
    /// change can come between reading and writing them): what callers
    /// do to their boards while others do the same.
    pub fn update_areas<T>(&self, f: impl FnOnce(&mut MsgAreas) -> T) -> anyhow::Result<T> {
        let path = self.config().resolve(&self.config().paths.msgareas);
        let mut held = self.areas.write().unwrap_or_else(|e| e.into_inner());
        let mut new = (**held).clone();
        let r = f(&mut new);
        write_atomic(&path, &new.to_toml())?;
        *held = Arc::new(new);
        Ok(r)
    }

    pub fn events(&self) -> Arc<events::Events> {
        self.events.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_events(&self, new: events::Events) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.events), &new.to_toml())?;
        *self.events.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    pub fn protocols(&self) -> Arc<extprog::Protocols> {
        self.protocols.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_protocols(&self, new: extprog::Protocols) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.protocols), &new.to_toml())?;
        *self.protocols.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    pub fn archivers(&self) -> Arc<extprog::Archivers> {
        self.archivers.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_archivers(&self, new: extprog::Archivers) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.archivers), &new.to_toml())?;
        *self.archivers.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    pub fn levels(&self) -> Arc<extprog::Levels> {
        self.levels.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// An access level in use (it has a description).
    pub fn level(&self, key: char) -> Option<config::Level> {
        self.levels().get(key).cloned()
    }

    pub fn set_levels(&self, new: extprog::Levels) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.levels), &new.to_toml())?;
        *self.levels.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    pub fn themes(&self) -> Arc<themes::Themes> {
        self.themes.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_themes(&self, new: themes::Themes) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.themes), &new.to_toml())?;
        *self.themes.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    pub fn infoforms(&self) -> Arc<infoform::Infoforms> {
        self.infoforms.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_infoforms(&self, new: infoform::Infoforms) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.infoforms), &new.to_toml())?;
        *self.infoforms.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    pub fn questions(&self) -> Arc<questions::Questions> {
        self.questions.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_questions(&self, new: questions::Questions) -> anyhow::Result<()> {
        write_atomic(&self.config().resolve(&self.config().paths.questions), &new.to_toml())?;
        *self.questions.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }

    /// The file areas as they are now.
    pub fn fareas(&self) -> Arc<FileAreas> {
        self.fareas.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Save new file areas to fileareas.toml and start using them.
    pub fn set_fareas(&self, new: FileAreas) -> anyhow::Result<()> {
        for a in &new.areas {
            let dir = self.config().resolve(&a.path);
            std::fs::create_dir_all(&dir).with_context(|| format!("file area directory {}", dir.display()))?;
        }
        let path = self.config().resolve(&self.config().paths.fileareas);
        write_atomic(&path, &new.to_toml())?;
        *self.fareas.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
    }
}

/// Write a file through a temporary, so a crash can't leave half of it.
pub(crate) fn write_atomic(path: &std::path::Path, text: &str) -> anyhow::Result<()> {
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    keep_mode(path, &tmp);
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Give `new` the permissions `old` has, if it exists: a file the SysOp
/// made private (it holds passwords) stays private when rewritten.
pub(crate) fn keep_mode(old: &std::path::Path, new: &std::path::Path) {
    if let Ok(m) = std::fs::metadata(old) {
        let _ = std::fs::set_permissions(new, m.permissions());
    }
}

/// `[newuser] purge_after_days`: delete accounts still at the new-user
/// level (A) that never called back after their first day, once they're
/// that many days old. New; the original left them for the SysOp.
fn purge_unvalidated(state: &BbsState) -> anyhow::Result<()> {
    let days = state.config().newuser.purge_after_days;
    if days == 0 {
        return Ok(());
    }
    let cutoff = crate::clock::now().date_naive() - chrono::Duration::days(days as i64);
    for mut u in state.users.list()? {
        let stale = u.first_call.is_some_and(|d| d <= cutoff);
        if !u.deleted && u.level == 'A' && u.total_calls <= 1 && stale && !state.nodes.online().iter().any(|(_, n)| n.user.eq_ignore_ascii_case(&u.handle)) {
            let original = u.clone();
            u.deleted = true;
            state.users.merge(&original, &u)?;
            tracing::info!("purged {} (#{}): never validated or called back in {days} days", u.handle, u.id);
        }
    }
    Ok(())
}

/// `fluxbbs unban ADDRESS|all [config]`: lift a ban from outside the
/// board (for a SysOp locked out of it). Takes effect when the server
/// (re)starts, or at once if it isn't running.
fn unban(who: Option<&str>, path: PathBuf) -> anyhow::Result<()> {
    let Some(who) = who else { anyhow::bail!("usage: fluxbbs unban <address>|all [config]") };
    let config = Config::load(&path)?;
    let db = config.db_path();
    let sys = SysData::open(&db).with_context(|| format!("database {}", db.display()))?;
    let ips: Vec<String> = sys.bans()?.into_iter().map(|(ip, ..)| ip).filter(|ip| who == "all" || ip == who).collect();
    let n = sys.clear_ban(who)?;
    for ip in &ips {
        sys.log_security(ip, "ban lifted from the command line")?;
    }
    println!("{n} ban(s) lifted; restart the server for it to notice");
    Ok(())
}

/// `fluxbbs pack-files [--dry-run] [config]`: what FILEPACK.EXE was, run
/// outside the BBS: forget files that are gone from every file area's
/// directory. New: files moved or renamed by hand keep their records
/// (`filesync`), and files with none get one, as `add-files` does.
/// `fluxbbs add-files [--dry-run] [config]`: `*%` (`faLocalUpload`) for
/// every area at once, from the shell: files put into an area's directory
/// by hand (a mirror, say) that the file base doesn't know yet are added,
/// with their descriptions, as uploaded by the SysOp; moved ones keep
/// their records, and nothing is forgotten.
/// New: files added whose descriptions are bare get one from the AI then
/// and there, unless `--no-describe`.
async fn sync_files(args: &[String], prune: bool) -> anyhow::Result<()> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let path = args.iter().find(|a| !a.starts_with("--")).map_or_else(config::default_path, PathBuf::from);
    let config = Config::load(&path)?;
    let db = config.db_path();
    let files = FileBase::open(&db).with_context(|| format!("file database {}", db.display()))?;
    let fareas = FileAreas::load(&config.resolve(&config.paths.fileareas))?;
    let areas = filesync::areas(&config, &fareas.areas);
    let opts = filesync::Opts { dry_run, prune, add: true, only: None };
    let report = filesync::sync(&files, &areas, &opts, |a, p| filesync::new_record(&config, a, p))?;
    for line in report.lines(&areas, dry_run) {
        println!("{line}");
    }
    println!("{} ({} file areas)", report.summary(dry_run), areas.len());
    #[cfg(feature = "ai")]
    if !dry_run && !report.added.is_empty() && !args.iter().any(|a| a == "--no-describe") {
        drop(files);
        aidesc::describe_added(&path, &report.added).await?;
    }
    Ok(())
}

/// For `fluxbbs ftn ...`: `data/ftn/ftn.log`, opened to add to.
fn ftn_log(args: &[String]) -> Option<std::fs::File> {
    if args.first().is_none_or(|a| a != "ftn") {
        return None;
    }
    let path = args.last().filter(|a| a.ends_with(".toml")).map(PathBuf::from).unwrap_or_else(config::default_path);
    let cfg = Config::load(&path).ok()?;
    let dir = cfg.resolve(&cfg.paths.data).join("ftn");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new().create(true).append(true).open(dir.join("ftn.log")).ok()
}

/// `fluxbbs ftn poll [NET]|toss|scan|areafix|filefix [NET] +TAG -TAG...
/// [config]`: the FTN jobs by hand (new); `poll` polls every network's
/// hub, or NET's; `areafix` asks a hub's AreaFix to link (+) or unlink (-)
/// echoes, `areafix NET %LIST` for what it carries, and `filefix` the
/// same of its FileFix for file echoes. Without NET, the request goes to
/// the network the first tag's echo is on (else the first network).
#[cfg(feature = "ftn")]
async fn ftn_command(args: &[String]) -> anyhow::Result<()> {
    let (path, rest): (PathBuf, &[String]) = match args.split_last() {
        Some((last, rest)) if last.ends_with(".toml") => (last.into(), rest),
        _ => (config::default_path(), args),
    };
    let state = load_state(&path)?;
    match rest.first().map(String::as_str) {
        Some("poll") => binkp::cycle(state, rest.get(1).map(String::as_str)).await,
        Some("toss") => {
            println!("{:?}", ftntoss::toss(&state)?);
            Ok(())
        }
        Some("scan") => {
            println!("{} message(s) packed", ftntoss::scan(&state)?);
            Ok(())
        }
        Some(robot @ ("areafix" | "filefix")) if rest.len() > 1 => {
            let files = robot == "filefix";
            let ftn = state.config().ftn.clone();
            let mapped = if files { &ftn.file_areas } else { &ftn.areas };
            // A network's name, else a tag (bare, + or -) or %command.
            let named = !rest[1].starts_with(['+', '-', '%']) && ftn.networks().iter().any(|n| n.name.eq_ignore_ascii_case(&rest[1]));
            let net = if named {
                ftntoss::network(&state, &rest[1])?
            } else {
                let tag = rest[1].trim_start_matches(['+', '-']);
                let on = mapped.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)).map(|a| ftn.network_of(a));
                let nets = ftn.networks();
                match on {
                    Some(n) => ftntoss::network(&state, &n)?,
                    // An echo not mapped yet: which hub can't be guessed.
                    None if nets.len() > 1 => anyhow::bail!("{tag} isn't in [[ftn.{}]]: say which network (fluxbbs ftn {robot} NET +{tag})", if files { "filearea" } else { "area" }),
                    None => nets.into_iter().next().ok_or_else(|| anyhow::anyhow!("ftn: no network set up"))?,
                }
            };
            let lines = &rest[if named { 2 } else { 1 }..];
            if lines.is_empty() {
                anyhow::bail!("say what to ask it: fluxbbs ftn {robot} {} +TAG", net.name);
            }
            let (_, hub) = ftntoss::addresses(&net)?;
            let sysop = state.config().bbs.sysop_alias.clone();
            let (to, pw) = if files { ("FileFix", net.filefix_pw()) } else { ("AreaFix", net.areafix_pw()) };
            ftntoss::netmail(&state, &net, &ftntoss::Outgoing { from: &sysop, to, to_addr: hub, subject: pw, body: lines, reply: None })?;
            println!("queued for {to} at {hub} ({}); it goes with the next poll (fluxbbs ftn poll {})", net.name, net.name);
            Ok(())
        }
        _ => anyhow::bail!("usage: fluxbbs ftn poll [NET]|toss|scan|areafix|filefix [NET] +TAG -TAG... [config]"),
    }
}

/// Load a board: its config, strings, databases and areas.
fn load_state(path: &std::path::Path) -> anyhow::Result<Arc<BbsState>> {
    let config = Config::load(path)?;
    let strings = Strings::load(&config.resolve(&config.paths.strings))?;
    let db = config.db_path();
    let users = UserStore::open(&db).with_context(|| format!("user database {}", db.display()))?;
    tracing::info!("{} users in {}", users.count()?, db.display());
    let areas = MsgAreas::load(&config.resolve(&config.paths.msgareas))?;
    let msgs = MsgBase::open(&db)?;
    // Deleted email goes for good now; what earlier versions kept goes here.
    let purged = msgs.purge_deleted(&areas.email.file)?;
    let fareas = FileAreas::load(&config.resolve(&config.paths.fileareas))?;
    for a in &fareas.areas {
        let dir = config.resolve(&a.path);
        std::fs::create_dir_all(&dir).with_context(|| format!("file area directory {}", dir.display()))?;
    }
    let protocols = extprog::load(&config.resolve(&config.paths.protocols))?
        .unwrap_or_else(|| extprog::Protocols { list: config.protocols.clone() });
    let archivers: extprog::Archivers = extprog::load(&config.resolve(&config.paths.archivers))?.unwrap_or_default();
    let levels = extprog::load(&config.resolve(&config.paths.levels))?
        .unwrap_or_else(|| extprog::Levels(config.levels.clone()));
    let infoforms: infoform::Infoforms = extprog::load(&config.resolve(&config.paths.infoforms))?.unwrap_or_default();
    let questions: questions::Questions = extprog::load(&config.resolve(&config.paths.questions))?.unwrap_or_default();
    let forms = infoform::FormStore::open(&db)?;
    #[cfg(feature = "carrierlost")]
    let carrier = carrierlost::CarrierStore::open(&db)?;
    #[cfg(feature = "email")]
    let email = emailver::CodeStore::open(&db)?;
    #[cfg(feature = "weather")]
    let weather = weather::WeatherStore::open(&db)?;
    let themes: themes::Themes = extprog::load(&config.resolve(&config.paths.themes))?.unwrap_or_default();
    let events: events::Events = extprog::load(&config.resolve(&config.paths.events))?.unwrap_or_default();
    let files = FileBase::open(&db)?;
    let sys = SysData::open(&db)?;
    let nodes = Nodes::new(config.bbs.max_nodes);
    let state = Arc::new(BbsState {
        config: std::sync::RwLock::new(Arc::new(config)),
        config_path: path.to_path_buf(),
        strings: std::sync::RwLock::new(Arc::new(strings)),
        users,
        areas: std::sync::RwLock::new(Arc::new(areas)),
        msgs,
        fareas: std::sync::RwLock::new(Arc::new(fareas)),
        protocols: std::sync::RwLock::new(Arc::new(protocols)),
        archivers: std::sync::RwLock::new(Arc::new(archivers)),
        levels: std::sync::RwLock::new(Arc::new(levels)),
        infoforms: std::sync::RwLock::new(Arc::new(infoforms)),
        questions: std::sync::RwLock::new(Arc::new(questions)),
        themes: std::sync::RwLock::new(Arc::new(themes)),
        events: std::sync::RwLock::new(Arc::new(events)),
        forms,
        #[cfg(feature = "carrierlost")]
        carrier,
        #[cfg(feature = "email")]
        email,
        #[cfg(feature = "weather")]
        weather,
        files,
        sys,
        text_lock: std::sync::Mutex::new(()),
        nodes,
        guard: guard::Guard::new(),
        ai_usage: Default::default(),
        mesh: Default::default(),
        links: Default::default(),
        #[cfg(feature = "mrc")]
        mrc: Default::default(),
        doors_in_use: Default::default(),
        started: clock::now().naive_local(),
    });
    if purged > 0 {
        attach::remove_orphans(&state)?;
        tracing::info!("{purged} deleted letters removed");
    }
    Ok(state)
}

// Each caller's session runs on a worker thread, and its futures nest
// deep (menu, command, message center, reader, editor): a debug build's
// frames for them outgrew tokio's 2 MB stack. The room is reserved, not
// used, until it's needed.
fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build()?.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let filter = || tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,russh=warn".into());
    // The WFC screen has the terminal, so the log goes to its window and
    // to data/fluxbbs.log.
    let wfc_log = if let Some(i) = args.iter().position(|a| a == "--wfc") {
        args.remove(i);
        wfc::check()?;
        let config = Config::load(&args.first().map(PathBuf::from).unwrap_or_else(config::default_path))?;
        let data = config.resolve(&config.paths.data);
        std::fs::create_dir_all(&data)?;
        let log = wfc::LogTail::new(&config.log_path());
        tracing_subscriber::fmt().with_env_filter(filter()).with_writer(log.clone()).with_ansi(false).without_time().with_target(false).init();
        Some(log)
    } else if let Some(log) = ftn_log(&args) {
        // `fluxbbs ftn` beside the board (docker exec) logs where the
        // board's log can't see it: data/ftn/ftn.log keeps it too.
        use tracing_subscriber::fmt::writer::MakeWriterExt;
        tracing_subscriber::fmt().with_env_filter(filter()).with_ansi(false).with_writer(std::io::stderr.and(std::sync::Mutex::new(log))).init();
        None
    } else {
        tracing_subscriber::fmt().with_env_filter(filter()).init();
        None
    };

    if args.first().is_some_and(|a| a == "import") {
        return import::from_args(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "unban") {
        return unban(args.get(1).map(String::as_str), args.get(2).map_or(config::default_path(), PathBuf::from));
    }
    if args.first().is_some_and(|a| a == "pack-files") {
        return sync_files(&args[1..], true).await;
    }
    if args.first().is_some_and(|a| a == "add-files") {
        return sync_files(&args[1..], false).await;
    }
    if args.first().is_some_and(|a| a == "ftn") {
        #[cfg(feature = "ftn")]
        return ftn_command(&args[1..]).await;
        #[cfg(not(feature = "ftn"))]
        anyhow::bail!("echomail isn't built into this board (build with --features ftn; docs/BUILDING.md)");
    }
    // New: `fluxbbs describe [--dry-run] [--limit N] [config]`.
    if args.first().is_some_and(|a| a == "describe") {
        return aidesc::from_args(&args[1..]).await;
    }
    // New: `fluxbbs backup [DAYS] [DIR] [config]`.
    if args.first().is_some_and(|a| a == "backup") {
        let (path, rest): (PathBuf, &[String]) = match args[1..].split_last() {
            Some((last, rest)) if last.ends_with(".toml") => (last.into(), rest),
            _ => (config::default_path(), &args[1..]),
        };
        let (days, dir) = backup::parse(&rest.iter().map(String::as_str).collect::<Vec<_>>())?;
        println!("{}", backup::run(&*load_state(&path)?, days, dir.as_deref())?.display());
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "textfiles") {
        #[cfg(feature = "textfiles")]
        return textfiles::from_args(&args[1..]).await;
        #[cfg(not(feature = "textfiles"))]
        anyhow::bail!("the textfiles importer isn't built into this board (build with --features textfiles; docs/BUILDING.md)");
    }

    let path = args.first().map(PathBuf::from).unwrap_or_else(config::default_path);
    let state = load_state(&path)?;
    // Bans survive restarts.
    match state.sys.bans() {
        Ok(bans) => {
            let now = crate::clock::now().naive_local();
            state.guard.load(bans.into_iter().filter_map(|(ip, until, strikes)| {
                let left = (until - now).to_std().ok();
                Some((ip.parse().ok()?, left, strikes))
            }));
        }
        Err(e) => tracing::error!("loading bans: {e}"),
    }
    // New: callers' boards from before moderators were kept by number get
    // theirs (by exact handle), so a new caller taking a deleted
    // moderator's handle can't inherit a board.
    let unnumbered: Vec<(String, u32)> = state
        .areas()
        .areas
        .iter()
        .filter(|a| !a.created.is_empty() && a.sponsor_id == 0)
        .filter_map(|a| state.users.find_handle(&a.sponsor).ok().flatten().map(|u| (a.file.clone(), u.id)))
        .collect();
    if !unnumbered.is_empty() {
        let r = state.update_areas(|areas| {
            for (file, id) in &unnumbered {
                if let Some(a) = areas.areas.iter_mut().find(|a| a.file.eq_ignore_ascii_case(file) && a.sponsor_id == 0) {
                    a.sponsor_id = *id;
                }
            }
        });
        if let Err(e) = r {
            tracing::error!("numbering boards' moderators: {e}");
        }
    }
    for p in state.config().ftn.problems() {
        tracing::warn!("ftn: {p}");
    }
    tokio::spawn(events::scheduler(state.clone()));
    tokio::spawn(trash::keeper(state.clone()));
    tokio::spawn(newposts::watch(state.clone()));
    #[cfg(feature = "meshcore")]
    tokio::spawn(meshcore::run(state.clone()));
    #[cfg(feature = "mrc")]
    tokio::spawn(mrc::run(state.clone()));
    #[cfg(feature = "ftn")]
    tokio::spawn(binkp::run(state.clone()));
    #[cfg(feature = "ftn")]
    tokio::spawn(binkp::listen(state.clone()));

    // Forget idle addresses now and then, and clear out abandoned
    // applications.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                tick.tick().await;
                state.guard.tidy();
                if let Err(e) = purge_unvalidated(&state) {
                    tracing::error!("purging new users: {e:#}");
                }
            }
        });
    }

    let telnet = {
        let state = state.clone();
        async move {
            if state.config().telnet.enabled {
                telnet::serve(state).await
            } else {
                std::future::pending().await
            }
        }
    };
    if state.config().web.enabled {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = web::serve(state).await {
                tracing::error!("status feed: {e:#}");
            }
        });
    }
    let wfc = {
        let state = state.clone();
        async move {
            match wfc_log {
                Some(log) => wfc::run(state, log).await,
                None => std::future::pending().await,
            }
        }
    };
    tokio::select! {
        r = wfc => {
            tracing::info!("shutting down");
            r
        }
        r = ssh::serve(state) => r,
        r = telnet => r,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down");
            Ok(())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A board with stock settings and empty in-memory databases.
    pub fn state() -> Arc<BbsState> {
        let config = Config::default();
        Arc::new(BbsState {
            strings: std::sync::RwLock::new(Arc::new(Strings::load(std::path::Path::new("/nonexistent/strings.toml")).unwrap())),
            config_path: config::default_path(),
            users: UserStore::open_memory().unwrap(),
            areas: std::sync::RwLock::new(Arc::new(MsgAreas::default())),
            msgs: MsgBase::open_memory().unwrap(),
            fareas: std::sync::RwLock::new(Arc::new(FileAreas::default())),
            protocols: std::sync::RwLock::new(Arc::new(extprog::Protocols::default())),
            archivers: std::sync::RwLock::new(Arc::new(extprog::Archivers::default())),
            levels: std::sync::RwLock::new(Arc::new(extprog::Levels(config::default_levels()))),
            infoforms: std::sync::RwLock::new(Arc::new(infoform::Infoforms::default())),
            questions: std::sync::RwLock::new(Arc::new(questions::Questions::default())),
            themes: std::sync::RwLock::new(Arc::new(themes::Themes::default())),
            events: std::sync::RwLock::new(Arc::new(events::Events::default())),
            forms: infoform::FormStore::open_memory().unwrap(),
            #[cfg(feature = "carrierlost")]
            carrier: carrierlost::CarrierStore::open_memory().unwrap(),
            #[cfg(feature = "email")]
            email: emailver::CodeStore::open_memory().unwrap(),
            #[cfg(feature = "weather")]
            weather: weather::WeatherStore::open_memory().unwrap(),
            files: FileBase::open_memory().unwrap(),
            sys: SysData::open_memory().unwrap(),
            text_lock: std::sync::Mutex::new(()),
            nodes: Nodes::new(config.bbs.max_nodes),
            guard: guard::Guard::new(),
            ai_usage: Default::default(),
            mesh: Default::default(),
            links: Default::default(),
            #[cfg(feature = "mrc")]
            mrc: Default::default(),
            doors_in_use: Default::default(),
            started: clock::now().naive_local(),
            config: std::sync::RwLock::new(Arc::new(config)),
        })
    }
}
