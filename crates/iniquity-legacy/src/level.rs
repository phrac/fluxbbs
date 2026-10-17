//! LEVELS.DAT: `tLevels = array['A'..'Z'] of tLevelRec` (26 x 294 bytes).
//! A level with an empty description is unused.

use crate::pascal::{Reader, check};

pub const SIZE: usize = 294;

#[derive(Debug, Clone)]
pub struct LegacyLevel {
    pub key: char,
    pub desc: String,
    pub sl: u8,
    pub dsl: u8,
    pub time_limit: u16,
    pub file_points: u16,
    pub post_call: u16,
    pub limit_dl: u16,
    pub limit_dl_kb: u16,
    pub user_note: String,
    pub uldl_ratio: u16,
    pub kb_ratio: u16,
}

impl LegacyLevel {
    /// The defined levels in a LEVELS.DAT file.
    pub fn parse_file(buf: &[u8]) -> Result<Vec<Self>, crate::Error> {
        check("tLevels", buf, SIZE * 26)?;
        let mut out = Vec::new();
        for (i, rec) in buf.as_chunks::<SIZE>().0.iter().take(26).enumerate() {
            let mut r = Reader::new(rec);
            let level = LegacyLevel {
                key: (b'A' + i as u8) as char,
                desc: r.string(40),
                sl: r.u8(),
                dsl: r.u8(),
                time_limit: r.u16(),
                file_points: r.u16(),
                post_call: r.u16(),
                limit_dl: r.u16(),
                limit_dl_kb: r.u16(),
                user_note: r.string(40),
                uldl_ratio: r.u16(),
                kb_ratio: r.u16(),
            };
            if !level.desc.is_empty() {
                out.push(level);
            }
        }
        Ok(out)
    }
}
