//! File areas.
//!
//! * FILEAREA.DAT: `tFileAreaRec` (316 bytes) per area.
//! * FILECONF.DAT: `tConfRec` (53 bytes), as for messages.
//! * `<area>.DIR`: `tFileRec` (122 bytes) per file.
//! * FILEDESC.DAT: description lines (`String[50]`, 51 bytes), shared by
//!   all areas; a file's `desc_ptr` is the record number of its first.

use crate::pascal::{Reader, check};

pub const AREA_SIZE: usize = 316;
pub const FILE_SIZE: usize = 122;
pub const DESC_SIZE: usize = 51;

#[derive(Debug, Clone)]
pub struct LegacyFileArea {
    pub name: String,
    pub filename: String,
    /// DOS path of the files.
    pub path: String,
    pub sponsor: String,
    pub acs: String,
    pub acs_upload: String,
    pub acs_download: String,
    pub password: String,
    pub files: u16,
}

impl LegacyFileArea {
    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tFileAreaRec", buf, AREA_SIZE)?;
        let mut r = Reader::new(buf);
        Ok(Self {
            name: r.string(40),
            filename: r.string(8),
            path: r.string(40),
            sponsor: r.string(36),
            acs: r.string(20),
            acs_upload: r.string(20),
            acs_download: r.string(20),
            password: r.string(20),
            files: r.u16(),
        })
    }

    pub fn parse_file(buf: &[u8]) -> Result<Vec<Self>, crate::Error> {
        buf.as_chunks::<AREA_SIZE>().0.iter().map(|c| Self::parse(c)).collect()
    }
}

#[derive(Debug, Clone)]
pub struct LegacyFile {
    pub filename: String,
    pub size: i32,
    /// "MM/DD/YY"
    pub date: String,
    pub downloads: u16,
    pub file_points: u16,
    pub uploader: String,
    pub ul_date: String,
    pub desc_ptr: i32,
    pub desc_lines: u8,
    pub valid: bool,
}

impl LegacyFile {
    pub fn parse(buf: &[u8]) -> Result<Self, crate::Error> {
        check("tFileRec", buf, FILE_SIZE)?;
        let mut r = Reader::new(buf);
        Ok(Self {
            filename: r.string(12),
            size: r.i32(),
            date: r.string(8),
            downloads: r.u16(),
            file_points: r.u16(),
            uploader: r.string(36),
            ul_date: r.string(8),
            desc_ptr: r.i32(),
            desc_lines: r.u8(),
            valid: r.bool(),
        })
    }

    pub fn parse_file(buf: &[u8]) -> Result<Vec<Self>, crate::Error> {
        buf.as_chunks::<FILE_SIZE>().0.iter().map(|c| Self::parse(c)).collect()
    }

    /// This file's description from FILEDESC.DAT.
    pub fn description(&self, descs: &[u8]) -> Vec<String> {
        (0..self.desc_lines as usize)
            .filter_map(|i| {
                let at = (self.desc_ptr.max(0) as usize + i) * DESC_SIZE;
                descs.get(at..at + DESC_SIZE).map(|rec| Reader::new(rec).string(50))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pstr(s: &str, max: usize) -> Vec<u8> {
        let mut v = vec![s.len() as u8];
        v.extend_from_slice(s.as_bytes());
        v.resize(max + 1, 0);
        v
    }

    #[test]
    fn file_and_description() {
        let mut rec = pstr("FOO.ZIP", 12);
        rec.extend(1234i32.to_le_bytes());
        rec.extend(pstr("10/21/97", 8));
        rec.extend(7u16.to_le_bytes());
        rec.extend(3u16.to_le_bytes());
        rec.extend(pstr("Fiend", 36));
        rec.extend(pstr("11/01/97", 8));
        rec.extend(1i32.to_le_bytes());
        rec.push(2);
        rec.push(1);
        rec.resize(FILE_SIZE, 0);
        let f = LegacyFile::parse(&rec).unwrap();
        assert_eq!((f.filename.as_str(), f.size, f.downloads, f.valid), ("FOO.ZIP", 1234, 7, true));
        let descs = [pstr("other", 50), pstr("line one", 50), pstr("line two", 50)].concat();
        assert_eq!(f.description(&descs), vec!["line one", "line two"]);
    }
}
