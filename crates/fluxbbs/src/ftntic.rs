//! New: file echoes. The hub sends each file with a TIC (FSC-0087), a few
//! lines naming its echo, its name, a description, its size and CRC, who
//! sent it and a password. A TIC from the network's hub, to us (or to no
//! one in particular), with the right password and a CRC, whose file has
//! come whole, puts that file in the file area its echo is mapped to
//! (`[[ftn.filearea]]`), with the TIC's description and the sender's
//! address as the uploader; `Replaces` takes out the files it names (the
//! last nodelist). A file of the same name is replaced. What's checked,
//! and how, follows HTick's `processTic` (the hubs' usual tosser): a
//! name with a path in it is refused, a `Replaces` mask starting with
//! `*` is ignored, and a file the mailer renamed is found by its size and
//! CRC. Nothing is passed on: the board is a leaf node and has no
//! downlinks, and it hatches nothing of its own.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::BbsState;
use crate::config::FtnNetwork;
use crate::filebase::FileRec;
use crate::ftn::Addr;
use crate::ftntoss::Tossed;

/// What a TIC says.
#[derive(Debug, Default, PartialEq)]
pub struct Tic {
    pub area: String,
    pub file: String,
    /// The name before it was cut to 8.3 (`Fullname`, or `Lfile`), if any.
    pub lfile: String,
    pub replaces: Vec<String>,
    pub desc: Vec<String>,
    pub ldesc: Vec<String>,
    pub size: Option<u64>,
    pub crc: Option<u32>,
    pub from: Option<Addr>,
    pub to: Option<Addr>,
    /// A `To` line there was, but not an address.
    pub bad_to: bool,
    pub origin: Option<Addr>,
    pub pw: String,
}

/// How old a TIC whose file never came may get before it goes to `bad`.
const STALE_DAYS: u64 = 3;
/// Description lines kept.
const MAX_DESC: usize = 20;

/// An address as a TIC may write it (FSC-0087): `zone:net/node`, with
/// `.point`, `@domain`, or `domain#` in front.
fn tic_addr(s: &str) -> Option<Addr> {
    let s = s.split_whitespace().next()?;
    let s = s.rsplit_once('#').map_or(s, |(_, a)| a);
    Addr::parse(s).filter(|a| a.zone != 0 && a.net != 0)
}

/// Read a TIC: UTF-8, else CP437.
pub fn parse(data: &[u8]) -> Result<Tic, String> {
    let text = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => fluxbbs_term::cp437::decode(data),
    };
    let mut t = Tic::default();
    for line in text.lines() {
        let line = line.trim_end_matches(['\r', '\x1a']).trim_start();
        // HTick skips these as comments.
        if line.starts_with([';', '#']) {
            continue;
        }
        let (key, val) = line.split_once([' ', '\t']).unwrap_or((line, ""));
        let val = crate::editor::printable(val.trim()).into_owned();
        match key.to_ascii_lowercase().as_str() {
            "area" => t.area = val.split_whitespace().next().unwrap_or("").to_ascii_uppercase(),
            "file" => t.file = val,
            "fullname" | "lfile" => t.lfile = val,
            "replaces" if !val.is_empty() => t.replaces.push(val),
            "desc" => t.desc.push(val),
            "ldesc" => t.ldesc.push(val),
            "size" => t.size = val.parse().ok(),
            "crc" => t.crc = u32::from_str_radix(val.trim_start_matches("0x").trim_start_matches("0X"), 16).ok(),
            // `From address [password]`: the address.
            "from" => t.from = tic_addr(&val),
            // `To address`, or HTick's `To Name, address`.
            "to" => {
                t.to = tic_addr(&val).or_else(|| val.split_once(',').and_then(|(_, a)| tic_addr(a)));
                t.bad_to = t.to.is_none() && !val.is_empty();
            }
            "origin" => t.origin = tic_addr(&val),
            "pw" => t.pw = val,
            _ => {}
        }
    }
    if t.area.is_empty() || t.file.is_empty() {
        return Err("no Area or File line".into());
    }
    Ok(t)
}

/// The CRC-32 a TIC gives for a file.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(data);
    c.sum()
}

/// Has the name a path in it (HTick refuses those)?
fn has_path(name: &str) -> bool {
    name.contains(['/', '\\', ':'])
}

/// A name from a TIC as a file in an area: printable, no path, not hidden.
fn usable_name(name: &str) -> Option<String> {
    let n = name.trim();
    (!n.is_empty() && n.len() <= 200 && !has_path(n) && !n.starts_with('.') && n.chars().all(|c| !c.is_control())).then(|| n.to_string())
}

/// A name as our binkp receiver writes it (`binkp::safe_name`, less its
/// rarer cases): letters, digits, `.`, `_` and `-` only, in lower case.
fn received_form(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect::<String>().to_ascii_lowercase()
}

/// A `Replaces` mask (`*`, `?`), case-insensitive, as HTick's `patimat`.
fn mask_matches(mask: &str, name: &str) -> bool {
    fn m(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => m(&p[1..], n) || (!n.is_empty() && m(p, &n[1..])),
            (Some('?'), Some(_)) => m(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b) && m(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let (p, n): (Vec<char>, Vec<char>) = (mask.chars().collect(), name.chars().collect());
    m(&p, &n)
}

fn lower_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The TICs in `dir`, oldest first.
fn tics(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && lower_name(p).ends_with(".tic")).collect();
    v.sort_by_key(|p| (std::fs::metadata(p).and_then(|m| m.modified()).ok(), p.file_name().map(|n| n.to_os_string())));
    v
}

fn read_tic(p: &Path) -> Option<Tic> {
    std::fs::read(p).ok().and_then(|d| parse(&d).ok())
}

/// Files in `dir` with the TIC's file's name: that name as our receiver
/// writes it, or a second one of it (`stem-N.ext`, `ftntoss::free_name`).
fn by_name(dir: &Path, name: &str) -> Vec<PathBuf> {
    let want = received_form(name);
    if want.is_empty() {
        return Vec::new();
    }
    let (stem, ext) = want.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((want.clone(), String::new()));
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let n = lower_name(p);
            n == want || n.strip_prefix(&stem).and_then(|r| r.strip_prefix('-')).and_then(|r| r.strip_suffix(ext.as_str())).is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
        })
        .collect();
    v.sort();
    v
}

/// The TIC's file in `dir`: one of its name with its size and CRC, else
/// (FSC-0087: forwarders find files a mailer renamed) any file there with
/// them. None without a CRC to go by.
fn find_file(dir: &Path, tic: &Tic) -> std::io::Result<Option<PathBuf>> {
    let Some(crc) = tic.crc else { return Ok(None) };
    let named = by_name(dir, &tic.file);
    let others = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| {
        let n = lower_name(p);
        p.is_file() && !n.ends_with(".tic") && !n.ends_with(".part") && !n.starts_with('.') && !named.contains(p)
    });
    for p in named.clone().into_iter().chain(others) {
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        if tic.size.is_some_and(|s| s != meta.len()) {
            continue;
        }
        if crc32(&std::fs::read(&p)?) == crc {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

/// The files in `dir` that TICs there name or match: the mail tosser
/// leaves them alone, and moving one TIC to `bad` leaves another's file.
fn claimed(dir: &Path, except: Option<&Path>) -> HashSet<String> {
    let mut out = HashSet::new();
    for p in tics(dir) {
        if Some(p.as_path()) == except {
            continue;
        }
        let Some(t) = read_tic(&p) else { continue };
        for c in by_name(dir, &t.file) {
            out.insert(lower_name(&c));
        }
        if let Ok(Some(f)) = find_file(dir, &t) {
            out.insert(lower_name(&f));
        }
    }
    out
}

/// The files the TICs in `dir` are for (see [`claimed`]).
pub fn held(dir: &Path) -> HashSet<String> {
    claimed(dir, None)
}

/// What became of one TIC.
enum Outcome {
    Done,
    /// Its file hasn't come (yet), or its echo isn't mapped: left.
    Wait(String),
    /// Wrong: the TIC and its file (if it came) go to `bad`.
    Bad(String),
}

/// Toss the TICs in `dir` for `nets` (a network's own inbound: those its
/// hub's session proved it was).
pub fn toss_dir(state: &BbsState, dir: &Path, nets: &[(FtnNetwork, Addr, Addr)], t: &mut Tossed) -> anyhow::Result<()> {
    let (_, _, bad) = crate::ftntoss::dirs(state);
    for path in tics(dir) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let outcome = match std::fs::read(&path) {
            Ok(d) => match parse(&d) {
                Ok(tic) => toss_one(state, dir, &tic, nets)?,
                Err(why) => Outcome::Bad(why),
            },
            Err(e) => {
                tracing::warn!("ftn: {name}: {e}");
                continue;
            }
        };
        match outcome {
            Outcome::Done => {
                std::fs::remove_file(&path)?;
                t.files += 1;
            }
            Outcome::Wait(why) => {
                let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).unwrap_or_default();
                if age.as_secs() > STALE_DAYS * 86_400 {
                    tracing::warn!("ftn: {name}: {why}, for {STALE_DAYS} days; kept in bad");
                    to_bad(dir, &path, &bad)?;
                    t.bad += 1;
                } else {
                    tracing::info!("ftn: {name}: {why}; left for the next toss");
                }
            }
            Outcome::Bad(why) => {
                tracing::warn!("ftn: {name}: {why}; kept in bad");
                to_bad(dir, &path, &bad)?;
                t.bad += 1;
            }
        }
    }
    Ok(())
}

/// Move a TIC, and its file if that came, to `bad`: the file it matches,
/// else those of its name, but none another TIC there is waiting for.
fn to_bad(dir: &Path, tic: &Path, bad: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(bad)?;
    let others = claimed(dir, Some(tic));
    let files: Vec<PathBuf> = match read_tic(tic) {
        Some(t) => match find_file(dir, &t)? {
            Some(f) => vec![f],
            None => by_name(dir, &t.file),
        },
        None => Vec::new(),
    };
    for f in std::iter::once(tic.to_path_buf()).chain(files.into_iter().filter(|f| !others.contains(&lower_name(f)))) {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::rename(&f, crate::ftntoss::free_name(bad, &name))?;
    }
    Ok(())
}

fn toss_one(state: &BbsState, dir: &Path, tic: &Tic, nets: &[(FtnNetwork, Addr, Addr)]) -> anyhow::Result<Outcome> {
    // A leaf node gets its files from its hub alone, addressed to us (`To`
    // is optional in FSC-0087).
    let Some(from) = tic.from else { return Ok(Outcome::Bad("no From address".into())) };
    if tic.bad_to {
        return Ok(Outcome::Bad("its To line isn't an address".into()));
    }
    let Some((net, _, _)) = nets.iter().find(|(_, me, hub)| from == *hub && tic.to.is_none_or(|to| to == *me)) else {
        return Ok(Outcome::Bad(match tic.to {
            Some(to) if !nets.iter().any(|(_, me, _)| to == *me) => format!("addressed to {to}, not to us"),
            _ => format!("from {from}, not a hub that may send here"),
        }));
    };
    if !tic.pw.eq_ignore_ascii_case(net.tic_pw()) {
        return Ok(Outcome::Bad(format!("wrong TIC password from {from}")));
    }
    if tic.crc.is_none() {
        return Ok(Outcome::Bad("no CRC (FSC-0087 requires one)".into()));
    }
    if has_path(&tic.file) || tic.replaces.iter().any(|r| has_path(r)) {
        return Ok(Outcome::Bad("a path in its File or Replaces".into()));
    }
    let ftn = state.config().ftn.clone();
    let Some(map) = ftn.file_areas_of(&net.name).into_iter().find(|a| a.tag.eq_ignore_ascii_case(&tic.area)) else {
        return Ok(Outcome::Wait(format!("file echo {} has no file area here ([[ftn.filearea]])", tic.area)));
    };
    let fareas = state.fareas();
    let Some(area) = fareas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&map.area)) else {
        return Ok(Outcome::Wait(format!("file echo {}'s area {} isn't a file area", tic.area, map.area)));
    };
    let Some(src) = find_file(dir, tic)? else {
        // Its name there but not its CRC: a file that came wrong, or one
        // still to come under a name taken.
        return Ok(Outcome::Wait(format!("{} with its size and CRC hasn't come", tic.file)));
    };
    let Some(dest_name) = usable_name(&tic.lfile).or_else(|| usable_name(&tic.file)) else {
        return Ok(Outcome::Bad(format!("{:?} can't be a file name here", tic.file)));
    };
    let cfg = state.config();
    let area_dir = cfg.resolve(&area.path);
    std::fs::create_dir_all(&area_dir)?;
    // `Replaces` (a mask starting with `*` is ignored, as HTick does), and
    // a file of the same name.
    let masks: Vec<&str> = tic.replaces.iter().map(|r| r.trim()).filter(|r| !r.starts_with('*')).collect();
    let gone: Vec<String> = state
        .files
        .list(&area.file)?
        .into_iter()
        .map(|r| r.name)
        .filter(|n| n.eq_ignore_ascii_case(&dest_name) || masks.iter().any(|m| mask_matches(m, n)))
        .collect();
    for g in &gone {
        if let Some(rec) = state.files.find(&area.file, g)? {
            state.files.remove(&area.file, rec.num)?;
        }
        let _ = std::fs::remove_file(area_dir.join(g));
    }
    let dest = area_dir.join(&dest_name);
    if std::fs::rename(&src, &dest).is_err() {
        // Another file system.
        std::fs::copy(&src, &dest)?;
        std::fs::remove_file(&src)?;
    }
    let sender = tic.origin.unwrap_or(from);
    let mut rec: FileRec = crate::filesync::new_record(&cfg, &area.file, &dest)?;
    rec.uploader = sender.to_string();
    let desc = description(tic);
    if !desc.is_empty() {
        rec.description = desc;
    }
    state.files.add(&mut rec)?;
    if !area.sort.is_none() {
        state.files.sort_area(&area.file, area.sort, area.sort_descending)?;
    }
    let replaced = gone.iter().filter(|g| !g.eq_ignore_ascii_case(&dest_name)).cloned().collect::<Vec<_>>();
    tracing::info!(
        "ftn: {} from {sender} ({}) in {}{}",
        dest_name,
        tic.area,
        area.file,
        if replaced.is_empty() { String::new() } else { format!(", replacing {}", replaced.join(", ")) }
    );
    Ok(Outcome::Done)
}

/// The long description if there is one, else the short; wrapped to the
/// file list's width.
fn description(tic: &Tic) -> Vec<String> {
    let lines = if tic.ldesc.iter().any(|l| !l.trim().is_empty()) { &tic.ldesc } else { &tic.desc };
    let mut out = Vec::new();
    for l in lines {
        let mut cur = String::new();
        for w in l.split_whitespace() {
            if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > crate::files::DESC_WIDTH {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out.truncate(MAX_DESC);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_tic() {
        // As HTick's writeTic writes one.
        let t = parse(b"Created by HTick, written by Gabriel Plutzar\r\nFile AGORANET.Z05\r\nArea AGN_NODE\r\nAreadesc AgoraNet nodelist\r\nDesc AgoraNet nodelist\r\nLDesc day 305\r\nReplaces AGORANET.Z*\r\nFrom 46:1/100\r\nTo 46:1/115\r\nOrigin 46:1/100\r\nSize 1234\r\nDate 1793375139\r\nCrc 1A2B3C4D\r\nPath 46:1/100 1793375139 Sat Oct 31 17:00:00 2026 UTC htick/lnx 1.9.0\r\nSeenby 46:1/115\r\nPw SECRET\r\n").unwrap();
        assert_eq!((t.area.as_str(), t.file.as_str()), ("AGN_NODE", "AGORANET.Z05"));
        assert_eq!(t.replaces, vec!["AGORANET.Z*"]);
        assert_eq!((t.crc, t.size), (Some(0x1A2B3C4D), Some(1234)));
        assert_eq!((t.from, t.to), (Addr::parse("46:1/100"), Addr::parse("46:1/115")));
        assert_eq!(t.pw, "SECRET");
        assert!(parse(b"Desc nothing else\r\n").is_err());
        // FSC-0087's address forms, HTick's `To Name, address`, a password
        // after From, and comments.
        let t = parse(b"; a comment\r\nArea X\r\nFile Y\r\nFrom agoranet#46:1/100.0 PASS\r\nTo phrac, 46:1/115@agoranet\r\nOrigin 46:1/100.0@agoranet\r\n").unwrap();
        assert_eq!((t.from, t.to, t.origin), (Addr::parse("46:1/100"), Addr::parse("46:1/115"), Addr::parse("46:1/100")));
        let t = parse(b"Area X\r\nFile Y\r\nTo nobody\r\n").unwrap();
        assert!(t.bad_to && t.to.is_none());
    }

    #[test]
    fn names_masks_and_descriptions() {
        assert!(has_path("../etc/passwd") && has_path("C:GAME.ZIP") && has_path("a\\b"));
        assert_eq!(usable_name("My File.zip").as_deref(), Some("My File.zip"));
        assert_eq!(usable_name(".hidden"), None);
        assert_eq!(received_form("My File+v2.ZIP"), "myfilev2.zip");
        assert!(mask_matches("AGORANET.Z*", "agoranet.z05"));
        assert!(mask_matches("NODE?.ZIP", "node1.zip"));
        // No extension is no extension: not the board's DOS-style match.
        assert!(!mask_matches("README", "README.TXT"));
        let t = Tic { desc: vec!["short".into()], ldesc: vec!["a long one".into(), "x ".repeat(60)], ..Default::default() };
        let d = description(&t);
        assert_eq!(d[0], "a long one");
        assert!(d.len() == 3 && d.iter().all(|l| l.len() <= crate::files::DESC_WIDTH));
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    fn board() -> (std::sync::Arc<BbsState>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("inq-tic-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let state = crate::tests::state();
        let mut cfg = (*state.config()).clone();
        cfg.root = dir.clone();
        cfg.paths.data = "data".into();
        cfg.ftn = crate::config::Ftn {
            enabled: true,
            name: "agoranet".into(),
            address: "46:1/115@agoranet".into(),
            hub: "46:1/100".into(),
            filefix_password: "TICPW".into(),
            file_areas: vec![crate::config::FtnArea { tag: "AGN_NODE".into(), area: "AGNNODE".into(), network: String::new() }],
            ..Default::default()
        };
        *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
        let mut fa = (*state.fareas()).clone();
        fa.areas.push(crate::fileareas::FileArea {
            name: "AgoraNet nodelist".into(),
            file: "AGNNODE".into(),
            path: "files/agnnode".into(),
            sponsor: String::new(),
            acs: String::new(),
            upload_acs: String::new(),
            download_acs: String::new(),
            password: String::new(),
            sort: Default::default(),
            sort_descending: false,
            parent: String::new(),
        });
        *state.fareas.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(fa);
        (state, dir)
    }

    /// A TIC as HTick sends a link one.
    fn tic_text(area: &str, name: &str, body: &[u8], extra: &str, pw: &str) -> String {
        format!(
            "Created by HTick, written by Gabriel Plutzar\r\nFile {name}\r\nArea {area}\r\nDesc The AgoraNet nodelist\r\n{extra}From 46:1/100\r\nTo 46:1/115\r\nOrigin 46:1/100\r\nSize {}\r\nCrc {:08X}\r\nPath 46:1/100 1793375139\r\nSeenby 46:1/115\r\nPw {pw}\r\n",
            body.len(),
            crc32(body)
        )
    }

    fn send(inbound: &Path, tic: &str, area: &str, name: &str, body: &[u8], extra: &str, pw: &str) {
        std::fs::create_dir_all(inbound).unwrap();
        std::fs::write(inbound.join(name), body).unwrap();
        std::fs::write(inbound.join(tic), tic_text(area, name, body, extra, pw)).unwrap();
    }

    #[test]
    fn files_go_to_their_areas() {
        let (state, dir) = board();
        let inbound = crate::ftntoss::inbound_dir(&state, "agoranet");
        let area = dir.join("files/agnnode");
        let (_, _, bad) = crate::ftntoss::dirs(&state);
        send(&inbound, "a1.tic", "AGN_NODE", "AGORANET.Z04", b"list four", "", "ticpw");
        let t = crate::ftntoss::toss(&state).unwrap();
        assert_eq!(t.files, 1);
        assert!(area.join("AGORANET.Z04").exists());
        let recs = state.files.list("AGNNODE").unwrap();
        assert_eq!((recs[0].uploader.as_str(), recs[0].description.clone()), ("46:1/100", vec!["The AgoraNet nodelist".to_string()]));
        assert_eq!(std::fs::read_dir(&inbound).unwrap().count(), 0);

        // The next replaces it; a mask starting with * is ignored.
        send(&inbound, "a2.tic", "AGN_NODE", "AGORANET.Z05", b"list five", "Replaces AGORANET.Z*\r\nReplaces *\r\n", "TICPW");
        crate::ftntoss::toss(&state).unwrap();
        assert!(!area.join("AGORANET.Z04").exists() && area.join("AGORANET.Z05").exists());
        let names: Vec<String> = state.files.list("AGNNODE").unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["AGORANET.Z05"]);

        // No To is fine; a file the mailer renamed is found by its CRC.
        std::fs::write(inbound.join("renamed.zip"), b"spaced name").unwrap();
        let text = tic_text("AGN_NODE", "My Nodes.zip", b"spaced name", "", "TICPW").replace("To 46:1/115\r\n", "");
        std::fs::write(inbound.join("a3.tic"), text).unwrap();
        assert_eq!(crate::ftntoss::toss(&state).unwrap().files, 1);
        assert!(area.join("My Nodes.zip").exists());

        // A wrong password, a path in the name, no CRC, the hub's point,
        // to another node: bad, with their files. An echo with no area,
        // and a file that hasn't come whole, wait.
        send(&inbound, "b1.tic", "AGN_NODE", "EVIL.ZIP", b"evil", "", "guess");
        std::fs::write(inbound.join("b2.tic"), tic_text("AGN_NODE", "../x.zip", b"x", "", "TICPW")).unwrap();
        std::fs::write(inbound.join("b3.tic"), tic_text("AGN_NODE", "NOCRC.ZIP", b"n", "", "TICPW").replace("Crc ", "Xcrc ")).unwrap();
        std::fs::write(inbound.join("b4.tic"), tic_text("AGN_NODE", "P.ZIP", b"p", "", "TICPW").replace("From 46:1/100\r\n", "From 46:1/100.5\r\n")).unwrap();
        std::fs::write(inbound.join("b5.tic"), tic_text("AGN_NODE", "O.ZIP", b"o", "", "TICPW").replace("To 46:1/115", "To 46:1/116")).unwrap();
        send(&inbound, "w1.tic", "AGN_GAME", "GAME.ZIP", b"game", "", "TICPW");
        std::fs::write(inbound.join("w2.tic"), tic_text("AGN_NODE", "AGORANET.Z06", b"list six", "", "TICPW")).unwrap();
        std::fs::write(inbound.join("AGORANET.Z06"), b"cut off").unwrap();
        let t = crate::ftntoss::toss(&state).unwrap();
        assert_eq!((t.files, t.bad), (0, 5));
        for f in ["b1.tic", "EVIL.ZIP", "b2.tic", "b3.tic", "b4.tic", "b5.tic"] {
            assert!(bad.join(f).exists(), "{f} in bad");
        }
        assert!(inbound.join("w1.tic").exists() && inbound.join("GAME.ZIP").exists());
        assert!(inbound.join("w2.tic").exists() && inbound.join("AGORANET.Z06").exists());
        assert!(!area.join("EVIL.ZIP").exists());

        // The whole file arrives (the mailer names it -1): it's tossed.
        std::fs::write(inbound.join("AGORANET-1.Z06"), b"list six").unwrap();
        assert_eq!(crate::ftntoss::toss(&state).unwrap().files, 1);
        assert!(area.join("AGORANET.Z06").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
