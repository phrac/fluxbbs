//! STRINGS.DAT: the SysOp-editable prompt table, `String[255]` records.
//! Record 1 is string #1 (`strDetectEmu`); a26 defined 267, r5 has 329.

use crate::pascal::Reader;

pub const SIZE: usize = 256;

pub fn parse_file(buf: &[u8]) -> Vec<String> {
    buf.as_chunks::<SIZE>().0.iter().map(|c| Reader::new(c).string(255)).collect()
}

/// Names of strings 1..=267 from the a26 `CONSTANT.PAS` (`strXxx = N`),
/// without the `str` prefix. Later r5 strings have no source names.
pub const NAMES: &[&str] = &include!("string_names.in");
