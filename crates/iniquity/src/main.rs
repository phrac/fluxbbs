//! Iniquity BBS.
//!
//!     iniquity [iniquity.toml]                  run the BBS
//!     iniquity import <old-install> [dest]      convert a DOS install

mod acs;
mod commands;
mod config;
mod engine;
mod import;
mod input;
mod logon;
mod mci;
mod menu;
mod messages;
mod msgareas;
mod msgbase;
mod editor;
mod fileareas;
mod filebase;
mod files;
mod transfer;
mod nodes;
mod session;
mod ssh;
mod strings;
mod user;
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
use users::UserStore;

/// State shared by every session.
pub struct BbsState {
    pub config: Config,
    pub strings: Strings,
    pub users: UserStore,
    pub areas: MsgAreas,
    pub msgs: MsgBase,
    pub fareas: FileAreas,
    pub files: FileBase,
    pub nodes: Arc<Nodes>,
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
    let files = FileBase::open(&db)?;
    let nodes = Nodes::new(config.bbs.max_nodes);
    let state = Arc::new(BbsState { config, strings, users, areas, msgs, fareas, files, nodes });

    tokio::select! {
        r = ssh::serve(state) => r,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down");
            Ok(())
        }
    }
}
