//! New: FTN nodelists (FTS-5000), to check a netmail's address before it's
//! sent. A network's nodelist is the newest file in the file area its
//! nodelist echo comes to (`nodelist` on the network: AgoraNet's
//! `agoranet.z03` in AGNNODE), as it came or in a ZIP. Each line is
//! `keyword,number,name,location,sysop,phone,speed,flags`: `Zone`,
//! `Region` and `Host` start a net (as its node 0), `Hub` and an empty
//! keyword are nodes in it, `Pvt` and `Hold` nodes that can be reached
//! by way of the hub, `Down` a node that can't.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::ftn::Addr;

/// A system in a nodelist.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// `Zone`, `Region`, `Host`, `Hub`, `Pvt`, `Hold`, `Down`, or blank.
    pub kind: String,
    pub name: String,
    pub location: String,
    pub sysop: String,
}

impl Entry {
    pub fn down(&self) -> bool {
        self.kind.eq_ignore_ascii_case("down")
    }
}

/// The systems in a nodelist, by zone, net and node.
#[derive(Debug, Default)]
pub struct Nodelist {
    pub entries: HashMap<(u16, u16, u16), Entry>,
}

impl Nodelist {
    /// Read one (CP437 text; `;` lines are comments).
    pub fn parse(text: &str) -> Nodelist {
        let mut entries = HashMap::new();
        let (mut zone, mut net) = (0u16, 0u16);
        for line in text.lines() {
            let line = line.trim_end_matches(['\r', '\x1a']);
            if line.starts_with(';') || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 5 {
                continue;
            }
            let Ok(n) = f[1].trim().parse::<u16>() else { continue };
            let kind = f[0].trim().to_string();
            let node = match kind.to_ascii_lowercase().as_str() {
                "zone" => {
                    (zone, net) = (n, n);
                    0
                }
                "region" | "host" => {
                    net = n;
                    0
                }
                _ => n,
            };
            if zone == 0 {
                continue;
            }
            let field = |i: usize| f.get(i).map(|s| s.replace('_', " ").trim().to_string()).unwrap_or_default();
            entries.insert((zone, net, node), Entry { kind, name: field(2), location: field(3), sysop: field(4) });
        }
        Nodelist { entries }
    }

    /// The system at `a`; a point is looked up as its boss node.
    pub fn find(&self, a: Addr) -> Option<&Entry> {
        self.entries.get(&(a.zone, a.net, a.node))
    }
}

/// The newest file in `dir`, read as a nodelist: itself, or the biggest
/// file in it if it's a ZIP. None if there's none to read.
pub fn read_dir(dir: &Path) -> Option<std::sync::Arc<Nodelist>> {
    let newest: PathBuf = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_file() && !e.file_name().to_string_lossy().starts_with('.'))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?
        .path();
    let meta = std::fs::metadata(&newest).ok()?;
    let key = (newest.clone(), meta.len(), meta.modified().ok()?);
    // Read once a version: a caller's every netmail would read it again.
    type Cached = Option<((PathBuf, u64, std::time::SystemTime), std::sync::Arc<Nodelist>)>;
    static CACHE: Mutex<Cached> = Mutex::new(None);
    if let Some((k, list)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && *k == key
    {
        return Some(list.clone());
    }
    let data = std::fs::read(&newest).ok()?;
    let text = if data.starts_with(b"PK\x03\x04") {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&data)).ok()?;
        let biggest = (0..z.len()).max_by_key(|&i| z.by_index(i).map(|f| f.size()).unwrap_or(0))?;
        let mut f = z.by_index(biggest).ok()?;
        let mut out = Vec::new();
        // A nodelist is a few MB; one that unpacks to far more isn't one.
        std::io::Read::read_to_end(&mut std::io::Read::take(&mut f, 64 << 20), &mut out).ok()?;
        out
    } else {
        data
    };
    // A line at a time: decoding would turn CR and LF into their glyphs.
    let lines: Vec<String> = text.split(|&b| b == b'\n').map(|l| fluxbbs_term::cp437::decode(l.strip_suffix(b"\r").unwrap_or(l))).collect();
    let list = std::sync::Arc::new(Nodelist::parse(&lines.join("\n")));
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, list.clone()));
    Some(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGN: &str = ";A Zone Nodelist for Friday, October 30, 2026 -- Day number 303 : 03233\r
;S\r
Zone,46,Agoranet_Coordinator,Pewaukee_WI,Nicholas_Boel,-Unpublished-,300,CM,INA:binkp.pharcyde.org,IBN\r
;\r
Host,1,Agoranet_Net_1,Pewaukee_WI,Nicholas_Boel,-Unpublished-,300,CM\r
Hub,100,The_Pharcyde_HUB,Pewaukee_WI,Nicholas_Boel,-Unpublished-,300,CM,INA:binkp.pharcyde.org,IBN\r
,115,disflux,Southeast_Nebraska,phrac,-Unpublished-,300,CM,INA:disfluxbbs.com,IBN\r
Down,116,Gone_BBS,Somewhere,Someone,-Unpublished-,300\r
Pvt,117,Quiet_BBS,Elsewhere,Another,-Unpublished-,300\r
Region,2,Region_Two,There,Them,-Unpublished-,300\r
,5,Far_BBS,Far,Far_Away,-Unpublished-,300\r
\x1a";

    #[test]
    fn reads_a_nodelist() {
        let n = Nodelist::parse(AGN);
        let at = |s: &str| n.find(Addr::parse(s).unwrap()).cloned();
        assert_eq!(at("46:1/100").unwrap().name, "The Pharcyde HUB");
        assert_eq!(at("46:1/115").unwrap().sysop, "phrac");
        assert_eq!(at("46:1/0").unwrap().kind, "Host");
        assert_eq!(at("46:46/0").unwrap().kind, "Zone");
        assert!(at("46:1/116").unwrap().down() && !at("46:1/117").unwrap().down());
        assert_eq!(at("46:2/5").unwrap().name, "Far BBS");
        // A point is its boss node's.
        assert_eq!(at("46:1/115.3").unwrap().name, "disflux");
        assert!(at("46:1/999").is_none() && at("21:1/100").is_none());
    }

    #[test]
    fn reads_the_newest_zipped() {
        let dir = std::env::temp_dir().join(format!("inq-nodelist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("agoranet.303", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zip, AGN.as_bytes()).unwrap();
        std::fs::write(dir.join("agoranet.z03"), zip.finish().unwrap().into_inner()).unwrap();
        let n = read_dir(&dir).unwrap();
        assert_eq!(n.find(Addr::parse("46:1/100").unwrap()).unwrap().location, "Pewaukee WI");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
