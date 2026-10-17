//! Readers for Iniquity 1.00a26 (r5) data files: the Turbo Pascal records
//! behind INIQUITY.DAT, USERS.DAT, LEVELS.DAT, STRINGS.DAT and *.MNU.
//!
//! Layouts follow the a26 source (`Global` types, preserved in
//! `testcol.pas`) and were checked byte-for-byte against a stock r5
//! install. The one r5 change is the menu command record, which grew
//! lightbar fields (see [`menu`]).

pub mod config;
pub mod filearea;
pub mod level;
pub mod menu;
pub mod misc;
pub mod msgarea;
pub mod pascal;
pub mod strings;
pub mod user;

pub use config::LegacyConfig;
pub use filearea::{LegacyFile, LegacyFileArea};
pub use level::LegacyLevel;
pub use menu::{LegacyCommand, LegacyMenu};
pub use misc::{LegacyBbs, LegacyCall, LegacyHistory, LegacyInfoform, LegacyStats, LegacyVote};
pub use msgarea::{LegacyConf, LegacyMsgArea, LegacyMsgHeader};
pub use user::LegacyUser;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{what}: expected {expected} bytes, got {got}")]
    Size { what: &'static str, expected: usize, got: usize },
}
