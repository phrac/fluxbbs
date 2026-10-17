//! Iniquity BBS.
//!
//!     iniquity [iniquity.toml]                  run the BBS
//!     iniquity import <old-install> [dest]      convert a DOS install

mod acs;
mod commands;
mod cfgedit;
mod challenge;
mod config;
mod doors;
mod engine;
mod events;
mod extprog;
mod guard;
mod import;
mod infoform;
mod input;
mod logon;
mod mci;
mod menu;
mod menuedit;
mod messages;
mod msgareas;
mod multinode;
mod msgbase;
mod editor;
mod extras;
mod fileareas;
mod filebase;
mod files;
mod transfer;
mod nodes;
mod qwk;
mod scripts;
mod session;
mod ssh;
mod sysop;
mod telnet;
mod themes;
mod timebank;
mod strings;
mod sysconfig;
mod sysdata;
mod sysmisc;
mod user;
mod useredit;
mod users;

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
    themes: std::sync::RwLock<Arc<themes::Themes>>,
    events: std::sync::RwLock<Arc<events::Events>>,
    /// Infoform answers and new user votes.
    pub forms: infoform::FormStore,
    pub files: FileBase,
    pub sys: SysData,
    /// Serialises appends to shared text files (`?T`).
    pub text_lock: std::sync::Mutex<()>,
    pub nodes: Arc<Nodes>,
    /// Connection limits, failure counts and bans.
    pub guard: Arc<guard::Guard>,
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
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
    }

    /// The message areas as they are now.
    pub fn areas(&self) -> Arc<MsgAreas> {
        self.areas.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Save new message areas to msgareas.toml and start using them.
    pub fn set_areas(&self, new: MsgAreas) -> anyhow::Result<()> {
        let path = self.config().resolve(&self.config().paths.msgareas);
        write_atomic(&path, &new.to_toml())?;
        *self.areas.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        Ok(())
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
fn write_atomic(path: &std::path::Path, text: &str) -> anyhow::Result<()> {
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// `iniquity pack-files [config]`: what FILEPACK.EXE was, run outside
/// the BBS: forget files that are gone from every file area's directory.
fn pack_files(path: PathBuf) -> anyhow::Result<()> {
    let config = Config::load(&path)?;
    let db = config.resolve(&config.paths.data).join("iniquity.db");
    let files = FileBase::open(&db).with_context(|| format!("file database {}", db.display()))?;
    let fareas = FileAreas::load(&config.resolve(&config.paths.fileareas))?;
    let mut total = 0;
    for a in &fareas.areas {
        let gone = files.prune_missing(&a.file, &config.resolve(&a.path))?;
        for name in &gone {
            println!("{}: {name} is missing; record removed", a.name);
        }
        total += gone.len();
    }
    println!("{total} record(s) removed from {} file area(s)", fareas.areas.len());
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,russh=warn".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "import") {
        return import::from_args(&args[1..]);
    }
    if args.first().is_some_and(|a| a == "pack-files") {
        return pack_files(args.get(1).map_or("iniquity.toml".into(), PathBuf::from));
    }

    let path = args.first().map(PathBuf::from).unwrap_or_else(|| "iniquity.toml".into());
    let config = Config::load(&path)?;
    let strings = Strings::load(&config.resolve(&config.paths.strings))?;
    let db = config.resolve(&config.paths.data).join("iniquity.db");
    let users = UserStore::open(&db).with_context(|| format!("user database {}", db.display()))?;
    tracing::info!("{} users in {}", users.count()?, db.display());
    let areas = MsgAreas::load(&config.resolve(&config.paths.msgareas))?;
    let msgs = MsgBase::open(&db)?;
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
    let forms = infoform::FormStore::open(&db)?;
    let themes: themes::Themes = extprog::load(&config.resolve(&config.paths.themes))?.unwrap_or_default();
    let events: events::Events = extprog::load(&config.resolve(&config.paths.events))?.unwrap_or_default();
    let files = FileBase::open(&db)?;
    let sys = SysData::open(&db)?;
    let nodes = Nodes::new(config.bbs.max_nodes);
    let state = Arc::new(BbsState {
        config: std::sync::RwLock::new(Arc::new(config)),
        config_path: path.clone(),
        strings: std::sync::RwLock::new(Arc::new(strings)),
        users,
        areas: std::sync::RwLock::new(Arc::new(areas)),
        msgs,
        fareas: std::sync::RwLock::new(Arc::new(fareas)),
        protocols: std::sync::RwLock::new(Arc::new(protocols)),
        archivers: std::sync::RwLock::new(Arc::new(archivers)),
        levels: std::sync::RwLock::new(Arc::new(levels)),
        infoforms: std::sync::RwLock::new(Arc::new(infoforms)),
        themes: std::sync::RwLock::new(Arc::new(themes)),
        events: std::sync::RwLock::new(Arc::new(events)),
        forms,
        files,
        sys,
        text_lock: std::sync::Mutex::new(()),
        nodes,
        guard: guard::Guard::new(),
    });
    tokio::spawn(events::scheduler(state.clone()));

    // Forget idle addresses now and then.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                tick.tick().await;
                state.guard.tidy();
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
    tokio::select! {
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
            config_path: PathBuf::from("iniquity.toml"),
            users: UserStore::open_memory().unwrap(),
            areas: std::sync::RwLock::new(Arc::new(MsgAreas::default())),
            msgs: MsgBase::open_memory().unwrap(),
            fareas: std::sync::RwLock::new(Arc::new(FileAreas::default())),
            protocols: std::sync::RwLock::new(Arc::new(extprog::Protocols::default())),
            archivers: std::sync::RwLock::new(Arc::new(extprog::Archivers::default())),
            levels: std::sync::RwLock::new(Arc::new(extprog::Levels(config::default_levels()))),
            infoforms: std::sync::RwLock::new(Arc::new(infoform::Infoforms::default())),
            themes: std::sync::RwLock::new(Arc::new(themes::Themes::default())),
            events: std::sync::RwLock::new(Arc::new(events::Events::default())),
            forms: infoform::FormStore::open_memory().unwrap(),
            files: FileBase::open_memory().unwrap(),
            sys: SysData::open_memory().unwrap(),
            text_lock: std::sync::Mutex::new(()),
            nodes: Nodes::new(config.bbs.max_nodes),
            guard: guard::Guard::new(),
            config: std::sync::RwLock::new(Arc::new(config)),
        })
    }
}
