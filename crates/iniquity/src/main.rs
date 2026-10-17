//! Iniquity BBS.
//!
//!     iniquity [iniquity.toml]                  run the BBS
//!     iniquity import <old-install> [dest]      convert a DOS install

mod acs;
mod commands;
mod cfgedit;
mod config;
mod engine;
mod extprog;
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
mod session;
mod ssh;
mod sysop;
mod telnet;
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
    /// Infoform answers and new user votes.
    pub forms: infoform::FormStore,
    pub files: FileBase,
    pub sys: SysData,
    /// Serialises appends to shared text files (`?T`).
    pub text_lock: std::sync::Mutex<()>,
    pub nodes: Arc<Nodes>,
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
        forms,
        files,
        sys,
        text_lock: std::sync::Mutex::new(()),
        nodes,
    });

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
