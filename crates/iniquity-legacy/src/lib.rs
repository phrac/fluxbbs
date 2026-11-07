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
pub use misc::{LegacyBbs, LegacyCall, LegacyHistory, LegacyInfoform, LegacyStats, LegacyTextLib, LegacyVote};
pub use msgarea::{LegacyConf, LegacyMsgArea, LegacyMsgHeader};
pub use user::LegacyUser;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{what}: expected {expected} bytes, got {got}")]
    Size { what: &'static str, expected: usize, got: usize },
}

#[cfg(test)]
#[path = "../../fluxbbs/src/fuzzkit.rs"]
mod fuzzkit;

#[cfg(test)]
mod fuzz {
    use super::*;

    /// The stock install's data files, if it's here (`INIQUITY_STOCK`, or
    /// `original/stock`), else blank records of each size.
    fn corpus() -> Vec<Vec<u8>> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = match std::env::var_os("INIQUITY_STOCK") {
            Some(p) => root.join(p),
            None => root.join("original/stock"),
        };
        let mut out: Vec<Vec<u8>> = ["INIQUITY.DAT", "DATA/USERS.DAT", "DATA/LEVELS.DAT", "DATA/STRINGS.DAT", "DATA/FILEAREA.DAT", "DATA/MSGAREA.DAT", "DATA/EMAIL.DAT", "DATA/HISTORY.DAT", "DATA/STATS.DAT", "DATA/BBSLIST.DAT", "DATA/NUV.DAT", "DATA/MSGCONF.DAT", "MENU/MAIN.MNU", "LIBS/EX-IQ2.TFL"]
            .iter()
            .filter_map(|f| std::fs::read(dir.join(f)).ok())
            .map(|mut d| {
                // Big files are cut to a few records: mutations go further.
                d.truncate(24 << 10);
                d
            })
            .collect();
        for size in [config::SIZE, user::SIZE, level::SIZE * 26, menu::HEADER_SIZE + 2 * menu::COMMAND_SIZE, msgarea::HEADER_SIZE * 2, filearea::AREA_SIZE, filearea::FILE_SIZE * 3, misc::STATS_SIZE, 75 + 2 * 19 + 8] {
            let mut b = vec![0u8; size];
            for (i, c) in b.iter_mut().enumerate().step_by(37) {
                *c = (i % 40) as u8;
            }
            out.push(b);
        }
        out
    }

    #[test]
    #[ignore]
    fn fuzz_legacy_records() {
        fuzzkit::run("legacy", corpus(), &[b"\xff", b"\x00\x00\x00\x00", b"\xff\xff\xff\x7f"], 20_000, |d| {
            let _ = LegacyConfig::parse(d);
            let _ = LegacyUser::parse_file(d);
            let _ = LegacyLevel::parse_file(d);
            let _ = LegacyMenu::parse(d);
            let _ = strings::parse_file(d);
            let _ = LegacyFileArea::parse_file(d);
            if let Ok(files) = LegacyFile::parse_file(d) {
                for f in files.iter().take(50) {
                    let _ = f.description(d);
                }
            }
            let _ = LegacyMsgArea::parse_file(d);
            let _ = LegacyConf::parse_file(d);
            if let Ok(headers) = LegacyMsgHeader::parse_file(d) {
                for h in headers.iter().take(50) {
                    let _ = h.lines(d);
                }
            }
            let _ = LegacyHistory::parse_file(d);
            let _ = LegacyCall::parse_file(d);
            let _ = LegacyBbs::parse_file(d);
            let _ = LegacyInfoform::parse_file(d);
            let _ = LegacyStats::parse(d);
            let _ = LegacyTextLib::parse(d);
            let _ = misc::text_lib_names(d);
            let _ = misc::parse_votes(d);
            for user in [0, 1, 2, u16::MAX] {
                let _ = misc::infoform_answers(d, user);
            }
            if d.len() >= 4 {
                let _ = msgarea::unpack_dos_time(u32::from_le_bytes([d[0], d[1], d[2], d[3]]));
            }
        });
    }
}
