//! New: the FTN tosser and scanner. Tossing reads what the hub sent
//! (packets, and ZIP bundles of them, in `data/ftn/inbound`): echomail
//! goes to the message area its tag is mapped to (`[[ftn.area]]`), less
//! duplicates (by MSGID), with replies threaded by REPLY; netmail for this
//! node goes to the named user's email (else the SysOp's). Scanning packs
//! what's been posted here in those areas since the last scan into a
//! packet for the hub in `data/ftn/outbound`. An area starts from what's
//! there when it's first scanned, so old posts aren't sent.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use chrono::NaiveDateTime;

use crate::BbsState;
use crate::config::{FtnArea, FtnNetwork};
use crate::ftn::{self, Addr, PackedMsg, Packet, Parts};
use crate::msgbase::{Message, Party};

/// What a toss did.
#[derive(Debug, Default, PartialEq)]
pub struct Tossed {
    pub packets: u32,
    pub echomail: u32,
    pub netmail: u32,
    pub dupes: u32,
    /// Echomail for areas not mapped here.
    pub unknown: u32,
    /// Files that couldn't be read, moved to `bad`.
    pub bad: u32,
    /// Files from file echoes put in their areas (`ftntic.rs`).
    pub files: u32,
}

/// Why a packet wasn't tossed whole.
enum Fault {
    /// Something about the packet: it goes to `bad`, to be tossed again
    /// once seen to (what was tossed from it comes in again as dupes).
    Bad(String),
    /// The board's side (the database busy, a disk error): the packet
    /// stays in inbound for the next toss.
    Board(anyhow::Error),
}

impl From<anyhow::Error> for Fault {
    fn from(e: anyhow::Error) -> Self {
        Fault::Board(e)
    }
}

/// A name in `dir` not yet taken: `name`, else a number before its
/// extension (`stem-1.pkt`), so the extension still says what it is.
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let mut dest = dir.join(name);
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(match name.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}-{n}.{ext}"),
            None => format!("{name}-{n}"),
        });
        n += 1;
    }
    dest
}

/// A mail bundle's name: `.mo0` to `.su9` (`.moa`... too), by the day.
fn bundle_name(lower: &str) -> bool {
    lower.rsplit_once('.').is_some_and(|(_, e)| {
        e.len() == 3 && ["mo", "tu", "we", "th", "fr", "sa", "su"].contains(&&e[..2]) && e.as_bytes()[2].is_ascii_alphanumeric()
    })
}

/// This node's and the hub's addresses on a network.
pub fn addresses(net: &FtnNetwork) -> anyhow::Result<(Addr, Addr)> {
    let me = Addr::parse(&net.address).ok_or_else(|| anyhow!("ftn {}: address {:?} isn't an FTN address", net.name, net.address))?;
    let hub = Addr::parse(&net.hub).ok_or_else(|| anyhow!("ftn {}: hub {:?} isn't an FTN address", net.name, net.hub))?;
    Ok((me, hub))
}

/// The networks set up, with their addresses; an error if there are none.
/// (`Ftn::networks` leaves out any that are set up wrong.)
fn networks(state: &BbsState) -> anyhow::Result<Vec<(FtnNetwork, Addr, Addr)>> {
    let nets: Vec<_> = state.config().ftn.networks().into_iter().filter_map(|n| addresses(&n).ok().map(|(me, hub)| (n, me, hub))).collect();
    if nets.is_empty() {
        anyhow::bail!("ftn: no network set up ([ftn] address, or [[ftn.network]])");
    }
    Ok(nets)
}

/// The network called `name` (any case).
pub fn network(state: &BbsState, name: &str) -> anyhow::Result<FtnNetwork> {
    let nets = state.config().ftn.networks();
    nets.iter().find(|n| n.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| {
        let names: Vec<&str> = nets.iter().map(|n| n.name.as_str()).collect();
        anyhow!("ftn: no network {name:?} (there's {})", if names.is_empty() { "none".to_string() } else { names.join(", ") })
    })
}

/// Where packets for a network's hub wait.
pub fn outbound_dir(state: &BbsState, net: &str) -> PathBuf {
    dirs(state).1.join(net.to_ascii_lowercase())
}

/// Every directory with packets for a network's hub: its own, and for
/// the first network `outbound` itself (packets queued before there were
/// several networks).
pub fn outbound_dirs(state: &BbsState, net: &str) -> Vec<PathBuf> {
    let mut v = vec![outbound_dir(state, net)];
    if state.config().ftn.first_name().eq_ignore_ascii_case(net) {
        v.push(dirs(state).1);
    }
    v
}

/// Where a session with a network's hub puts what it receives: tossed
/// only as mail of the network(s) that session proved it was.
pub fn inbound_dir(state: &BbsState, net: &str) -> PathBuf {
    dirs(state).0.join(net.to_ascii_lowercase())
}

/// The inbound, outbound and bad directories.
pub fn dirs(state: &BbsState) -> (PathBuf, PathBuf, PathBuf) {
    let cfg = state.config();
    let base = cfg.resolve(&cfg.paths.data).join("ftn");
    (base.join("inbound"), base.join("outbound"), base.join("bad"))
}

/// Hold `data/ftn/NAME.lock` until the file is dropped: one toss or scan
/// (`toss`), one binkp session per network (`binkp-NAME`) at a time,
/// across processes too (the board, and `fluxbbs ftn` beside it). `wait`
/// false: None if busy.
pub fn lock(state: &BbsState, name: &str, wait: bool) -> anyhow::Result<Option<std::fs::File>> {
    let (inbound, _, _) = dirs(state);
    let dir = inbound.parent().unwrap_or(&inbound);
    std::fs::create_dir_all(dir)?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("{name}.lock")))?;
    if wait {
        f.lock()?;
    } else if f.try_lock().is_err() {
        return Ok(None);
    }
    Ok(Some(f))
}

/// Toss everything inbound: each network's own inbound as mail of the
/// networks its hub's session proved it was (those sharing its session
/// password), and `inbound` itself (put there by hand, as from `bad`) as
/// whichever network a packet is addressed on.
pub fn toss(state: &BbsState) -> anyhow::Result<Tossed> {
    let nets = networks(state)?;
    let (inbound, _, _) = dirs(state);
    std::fs::create_dir_all(&inbound)?;
    let _lock = lock(state, "toss", true)?;
    let mut t = Tossed::default();
    let result = (|| {
        for (net, _, _) in &nets {
            let allowed: Vec<_> = nets
                .iter()
                .filter(|(n, _, _)| n.name == net.name || (!net.session_password.is_empty() && n.session_password == net.session_password))
                .cloned()
                .collect();
            tic_toss(state, &inbound_dir(state, &net.name), &allowed, &mut t);
            toss_dir(state, &inbound_dir(state, &net.name), &allowed, &mut t)?;
        }
        // A network renamed (or left out) leaves its inbound behind: that
        // and `inbound` itself go by the packets' addresses, as by hand.
        for d in stray_dirs(state, &inbound) {
            tracing::warn!("ftn: {} belongs to no network set up; tossing it by its packets' addresses", d.display());
            tic_toss(state, &d, &nets, &mut t);
            toss_dir(state, &d, &nets, &mut t)?;
        }
        tic_toss(state, &inbound, &nets, &mut t);
        toss_dir(state, &inbound, &nets, &mut t)?;
        retoss_unknown(state, &nets, &mut t)
    })();
    if t != Tossed::default() {
        tracing::info!("ftn: tossed {t:?}");
    }
    result.map(|()| t)
}

/// File echoes' TICs in `dir` (`ftntic.rs`): what goes wrong there is
/// logged, and never holds the mail up.
fn tic_toss(state: &BbsState, dir: &Path, nets: &[(FtnNetwork, Addr, Addr)], t: &mut Tossed) {
    if let Err(e) = crate::ftntic::toss_dir(state, dir, nets, t) {
        tracing::warn!("ftn: file echoes in {}: {e:#}", dir.display());
    }
}

/// Directories under `dir` that aren't a network's (a network renamed,
/// say).
pub fn stray_dirs(state: &BbsState, dir: &Path) -> Vec<PathBuf> {
    let names: Vec<String> = state.config().ftn.networks().into_iter().map(|n| n.name).collect();
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| !names.iter().any(|m| m.eq_ignore_ascii_case(&n.to_string_lossy()))))
        .collect();
    v.sort();
    v
}

/// Toss the files in `dir` as mail of `nets`.
fn toss_dir(state: &BbsState, dir: &Path, nets: &[(FtnNetwork, Addr, Addr)], t: &mut Tossed) -> anyhow::Result<()> {
    let (_, _, bad) = dirs(state);
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
    // Oldest first (by name when the clock can't tell), so a thread comes
    // in in order.
    // Files waiting for their TICs' echoes to be mapped aren't mail, even
    // a ZIP with a packet in it.
    let held = crate::ftntic::held(dir);
    let mut files: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && !held.contains(&p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()))
        .collect();
    files.sort_by_key(|p| (std::fs::metadata(p).and_then(|m| m.modified()).ok(), p.file_name().map(|n| n.to_os_string())));
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        // Part-received files belong to the mailer; TICs and their files
        // are `ftntic`'s, and anything else is left (unread).
        let packet = lower.ends_with(".pkt");
        if name.starts_with('.') || !(packet || bundle_name(&lower) || lower.ends_with(".zip")) {
            continue;
        }
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("ftn: {name}: {e}");
                continue;
            }
        };
        // Empty: a bundle some mailers send with nothing in it, or one cut
        // off. Nothing to keep.
        if data.is_empty() {
            tracing::info!("ftn: {name} is empty; removed");
            std::fs::remove_file(&path)?;
            continue;
        }
        let result = if packet {
            toss_packet(state, &data, nets, t)
        } else if data.starts_with(b"PK\x03\x04") && bundled(&data) {
            toss_bundle(state, &data, nets, t)
        } else if bundle_name(&lower) {
            Err(Fault::Bad("not a mail bundle that can be read here (ZIP only)".into()))
        } else {
            continue; // a ZIP of something else
        };
        match result {
            Ok(()) => std::fs::remove_file(&path)?,
            Err(Fault::Bad(why)) => {
                tracing::warn!("ftn: {name}: {why}; kept in bad");
                std::fs::create_dir_all(&bad)?;
                std::fs::rename(&path, free_name(&bad, &name))?;
                t.bad += 1;
            }
            Err(Fault::Board(e)) => return Err(e.context(format!("ftn: tossing {name} (left in inbound)"))),
        }
    }
    Ok(())
}

/// Is this ZIP a mail bundle (packets inside), not some other archive?
fn bundled(data: &[u8]) -> bool {
    zip::ZipArchive::new(std::io::Cursor::new(data)).is_ok_and(|z| z.file_names().any(|n| n.is_ok_and(|n| n.to_ascii_lowercase().ends_with(".pkt"))))
}

/// Every packet in a bundle, whatever happens to one of them.
fn toss_bundle(state: &BbsState, data: &[u8], nets: &[(FtnNetwork, Addr, Addr)], t: &mut Tossed) -> Result<(), Fault> {
    let bad = |e: &dyn std::fmt::Display| Fault::Bad(e.to_string());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| bad(&e))?;
    let mut problems = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| bad(&e))?;
        let name = f.name().map_err(|e| bad(&e))?.to_string();
        if !name.to_ascii_lowercase().ends_with(".pkt") {
            continue;
        }
        // A bundle is a few MB; one that unpacks to far more is refused.
        let mut pkt = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(&mut f, 64 << 20), &mut pkt).map_err(|e| bad(&e))?;
        match toss_packet(state, &pkt, nets, t) {
            Ok(()) => {}
            Err(Fault::Bad(why)) => problems.push(format!("{name}: {why}")),
            Err(e) => return Err(e),
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(Fault::Bad(problems.join("; "))) }
}

/// Does packet address `a` (zone 0: unknown) name `b`?
fn same_node(a: Addr, b: Addr) -> bool {
    (a.zone == 0 || a.zone == b.zone) && a.net == b.net && a.node == b.node
}

fn toss_packet(state: &BbsState, data: &[u8], nets: &[(FtnNetwork, Addr, Addr)], t: &mut Tossed) -> Result<(), Fault> {
    let pkt = Packet::parse(data).map_err(Fault::Bad)?;
    // The network it's on: addressed to us there, and (a leaf node's mail
    // all comes through its hub) from that network's hub.
    let on: Vec<_> = nets.iter().filter(|(_, me, hub)| same_node(pkt.dest, *me) && same_node(pkt.orig, *hub)).collect();
    // A packet without zones, between net/node numbers we have on two
    // networks, can't be told apart.
    if on.len() > 1 {
        return Err(Fault::Bad(format!("{} to {} could be on {} networks (no zone in it)", pkt.orig, pkt.dest, on.len())));
    }
    let Some((net, me, _)) = on.first().copied() else {
        return Err(Fault::Bad(match nets.iter().find(|(_, me, _)| same_node(pkt.dest, *me)) {
            Some((_, _, hub)) => format!("from {}, not the hub {hub}", pkt.orig),
            None => format!("addressed to {}, not to us on a network it may send for", pkt.dest),
        }));
    };
    let me = *me;
    let areas = state.config().ftn.areas_of(&net.name);
    // The header holds 8 characters of it.
    let want: String = net.packet_password.chars().take(8).collect();
    if !want.is_empty() && !pkt.password.eq_ignore_ascii_case(&want) {
        return Err(Fault::Bad(format!("wrong packet password from {}", pkt.orig)));
    }
    t.packets += 1;
    // What can't be tossed is kept apart, so the rest of the packet isn't
    // held up: echoes with no area here (until one's mapped), and netmail
    // for others (for the SysOp, in bad).
    let mut unknown: std::collections::BTreeMap<String, Vec<PackedMsg>> = Default::default();
    let mut misrouted = Vec::new();
    let mut problems = Vec::new();
    for m in &pkt.msgs {
        let parts = ftn::split_text(&m.text);
        match &parts.area {
            Some(tag) => {
                if !toss_echo(state, &areas, tag, m, &parts, t)? {
                    unknown.entry(tag.to_ascii_uppercase()).or_default().push(m.clone());
                }
            }
            None => {
                if let Some(why) = toss_netmail(state, m, &parts, me, pkt.orig, t)? {
                    problems.push(why);
                    misrouted.push(m.clone());
                }
            }
        }
    }
    let (inbound, _, bad) = dirs(state);
    for (tag, msgs) in unknown {
        let n = msgs.len();
        let dir = inbound.parent().unwrap_or(&inbound).join("unknown");
        // The network and echo in the name: kept until that echo has an
        // area on that network.
        // (Network names are letters, digits, `-` and `_` already.)
        let name = format!("{}~{}-{:08x}.pkt", net.name, safe_tag(&tag), crate::clock::now().timestamp_millis() as u32);
        save_packet(&dir, &name, &Packet { msgs, ..pkt.clone() }).map_err(Fault::Board)?;
        tracing::warn!("ftn: {n} message(s) for {tag}, which has no area here; kept in ftn/unknown until it's mapped");
    }
    if !misrouted.is_empty() {
        let name = format!("netmail-{:08x}.pkt", crate::clock::now().timestamp_millis() as u32);
        save_packet(&bad, &name, &Packet { msgs: misrouted, ..pkt.clone() }).map_err(Fault::Board)?;
        tracing::warn!("ftn: {}; kept in bad", problems.join("; "));
        t.bad += 1;
    }
    Ok(())
}

/// An echo tag as part of a file name.
fn safe_tag(tag: &str) -> String {
    let s: String = tag.chars().filter(|c| c.is_ascii_alphanumeric() || "_.".contains(*c)).take(40).collect();
    if s.is_empty() { "echo".into() } else { s }
}

/// Write `pkt` as `dir/name` (under a free name), whole before it's named.
fn save_packet(dir: &Path, name: &str, pkt: &Packet) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let dest = free_name(dir, name);
    let tmp = dest.with_extension("tmp");
    write_private(&tmp, &pkt.to_bytes())?;
    std::fs::rename(&tmp, &dest)?;
    Ok(())
}

/// Write a file only its owner can read: packets carry the packet
/// password, and an AreaFix request the AreaFix one.
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(data)
}

/// Toss again what was kept for echoes with no area, once they have one.
fn retoss_unknown(state: &BbsState, nets: &[(FtnNetwork, Addr, Addr)], t: &mut Tossed) -> anyhow::Result<()> {
    let (inbound, _, _) = dirs(state);
    let dir = inbound.parent().unwrap_or(&inbound).join("unknown");
    // Listed first: what's kept again as it's tossed isn't tossed twice.
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(()) };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    files.sort();
    let ftn = state.config().ftn.clone();
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some((net, rest)) = name.split_once('~') else { continue };
        let Some((tag, _)) = rest.rsplit_once('-') else { continue };
        let mapped = ftn.areas_of(net).iter().any(|a| safe_tag(&a.tag.to_ascii_uppercase()) == tag);
        if !name.ends_with(".pkt") || !mapped {
            continue;
        }
        let data = std::fs::read(&path)?;
        match toss_packet(state, &data, nets, t) {
            Ok(()) => std::fs::remove_file(&path)?,
            Err(Fault::Bad(why)) => tracing::warn!("ftn: unknown/{name}: {why}"),
            Err(Fault::Board(e)) => return Err(e.context(format!("ftn: tossing unknown/{name}"))),
        }
    }
    Ok(())
}

/// When a message was written, in the board's time: FTN dates are the
/// writer's local time, and TZUTC says how far that is from UTC (fsxNet
/// is mostly New Zealand, half a day ahead).
fn posted(m: &PackedMsg, parts: &Parts) -> NaiveDateTime {
    let Some(t) = ftn::parse_fts_date(&m.datetime) else { return crate::clock::now().naive_local() };
    let Some(offset) = parts.kludge("TZUTC").and_then(tzutc_minutes) else { return t };
    match t.checked_sub_signed(chrono::Duration::minutes(offset)) {
        Some(utc) => crate::clock::from_utc(utc),
        None => t,
    }
}

/// `-0500`, `0100`, `+1300` as minutes east of UTC.
fn tzutc_minutes(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, d) = match s.strip_prefix('-') {
        Some(d) => (-1, d),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    if d.len() != 4 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m): (i64, i64) = (d[..2].parse().ok()?, d[2..].parse().ok()?);
    (h <= 14 && m < 60).then_some(sign * (h * 60 + m))
}

/// Lines as a reader sees them: long ones word-wrapped to 79 (not ANSI
/// ones, whose escapes would be cut).
fn wrapped(body: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for l in body {
        if l.chars().count() <= 79 || l.contains('\x1b') {
            out.push(l.clone());
        } else {
            out.extend(crate::multinode::wrap(l, 79));
        }
    }
    out
}

/// Where the tear line is, as `sig_pos` counts (1-based, 0: none): the
/// tear and origin lines then count as the signature, left out of quotes.
fn tear_pos(lines: &[String]) -> u32 {
    lines.iter().rposition(|l| l == "---" || l.starts_with("--- ")).map_or(0, |i| i as u32 + 1)
}

fn party(name: &str) -> Party {
    Party { name: name.to_string(), alias: name.to_string(), real_name: name.to_string(), ..Party::default() }
}

/// Toss an echomail message; false if its echo has no area here.
fn toss_echo(state: &BbsState, areas: &[FtnArea], tag: &str, m: &PackedMsg, parts: &Parts, t: &mut Tossed) -> anyhow::Result<bool> {
    let Some(map) = areas.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)) else {
        t.unknown += 1;
        return Ok(false);
    };
    let area = map.area.as_str();
    if !state.areas().areas.iter().any(|a| a.file.eq_ignore_ascii_case(area)) {
        tracing::warn!("ftn: {tag} is mapped to message area {area}, which doesn't exist");
        t.unknown += 1;
        return Ok(false);
    }
    let msgid = dupe_key(m, parts);
    if state.msgs.ftn_known(area, &msgid)? {
        t.dupes += 1;
        return Ok(true);
    }
    let reply = parts.kludge("REPLY").filter(|r| !r.is_empty());
    let parent = match reply {
        Some(r) => state.msgs.ftn_find(area, r)?,
        None => None,
    };
    // No alias: a writer elsewhere with a local caller's name mustn't
    // pass for them (`is_me` matches aliases).
    let mut from = Party { alias: String::new(), ..party(&m.from) };
    from.note = parts.origin_addr().map(|a| a.to_string()).unwrap_or_default();
    let mut msg = Message {
        id: 0,
        area: area.to_string(),
        num: 0,
        from,
        to: party(&m.to),
        subject: m.subject.clone(),
        posted: posted(m, parts),
        deleted: false,
        private: false,
        anonymous: false,
        replies: 0,
        sig_pos: 0,
        tag: 0,
        reply_to: parent.unwrap_or(0),
        lines: wrapped(&parts.body),
    };
    msg.sig_pos = tear_pos(&msg.lines);
    state.msgs.add_ftn(&mut msg, Some(&msgid))?;
    // Threads whatever came before it (a reply can arrive first), and
    // waits for its own parent if that hasn't come yet.
    state.msgs.ftn_link_replies(area, &msgid, msg.id)?;
    if let (None, Some(r)) = (parent, reply) {
        // Not here yet (or never: some software's REPLY matches no MSGID).
        tracing::info!("ftn: {tag}: {} from {} answers {r}, not here (yet)", msg.num, msg.from.note);
        state.msgs.ftn_wait_reply(msg.id, area, r)?;
    }
    // No @mentions from elsewhere: anyone on the network could ping a
    // caller here.
    t.echomail += 1;
    Ok(true)
}

/// What a message is known by for dupes: its MSGID, or for one without,
/// `~` and who, to whom, when and what (never sent on as a REPLY).
fn dupe_key(m: &PackedMsg, parts: &Parts) -> String {
    match parts.kludge("MSGID") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("~{}|{}|{}|{}", m.from, m.to, m.datetime, m.subject),
    }
}

/// Toss a netmail; Some(why) if it isn't for us (it's kept for the
/// SysOp).
fn toss_netmail(state: &BbsState, m: &PackedMsg, parts: &Parts, me: Addr, pkt_orig: Addr, t: &mut Tossed) -> anyhow::Result<Option<String>> {
    // Who it's for and from: INTL (zones), FMPT/TOPT (points), else the
    // message header.
    let (mut orig, mut dest) = (Addr { zone: pkt_orig.zone, net: m.orig_net, node: m.orig_node, point: 0 }, Addr { zone: me.zone, net: m.dest_net, node: m.dest_node, point: 0 });
    if let Some(intl) = parts.kludge("INTL") {
        let mut w = intl.split_whitespace();
        if let (Some(d), Some(o)) = (w.next().and_then(Addr::parse), w.next().and_then(Addr::parse)) {
            (dest, orig) = (d, o);
        }
    }
    dest.point = parts.kludge("TOPT").and_then(|p| p.parse().ok()).unwrap_or(0);
    orig.point = parts.kludge("FMPT").and_then(|p| p.parse().ok()).unwrap_or(0);
    // Nothing is routed on from here: kept, in bad, for the SysOp.
    if dest != me {
        return Ok(Some(format!("netmail for {dest} from {orig}, not routed here")));
    }
    let msgid = dupe_key(m, parts);
    let email = state.areas().email.file.clone();
    if state.msgs.ftn_known(&email, &msgid)? {
        t.dupes += 1;
        return Ok(None);
    }
    // To the user named (not by number), or the SysOp, still showing
    // whom it was for.
    let named = if m.to.trim().chars().all(|c| c.is_ascii_digit()) { None } else { state.users.find(&m.to, false)? };
    let to = match (&named, state.users.get(1).ok().flatten()) {
        (Some(u), _) => Party { user_id: u.id, alias: u.handle.clone(), real_name: u.real_name.clone(), name: u.handle.clone(), note: String::new() },
        (None, Some(s)) => Party { user_id: s.id, alias: s.handle.clone(), real_name: s.real_name.clone(), name: m.to.clone(), note: String::new() },
        (None, None) => party(&m.to),
    };
    let mut from = party(&format!("{}@{orig}", m.from));
    from.note = orig.to_string();
    let mut msg = Message {
        id: 0,
        area: email.clone(),
        num: 0,
        from,
        to,
        subject: m.subject.clone(),
        posted: posted(m, parts),
        deleted: false,
        private: true,
        anonymous: false,
        replies: 0,
        sig_pos: 0,
        tag: 0,
        reply_to: 0,
        lines: wrapped(&parts.body),
    };
    msg.sig_pos = tear_pos(&msg.lines);
    state.msgs.add_ftn(&mut msg, Some(&msgid))?;
    tracing::info!("ftn: netmail from {}@{orig} to {}", m.from, msg.to.name);
    t.netmail += 1;
    Ok(None)
}

/// A new MSGID serial: never the same twice, kept with the messages.
fn next_serial(state: &BbsState) -> anyhow::Result<u32> {
    let now = crate::clock::now().timestamp();
    let last = state.msgs.ftn_get("serial")?.unwrap_or(0);
    let next = (last + 1).max(now);
    state.msgs.ftn_set("serial", next)?;
    Ok(next as u32)
}

/// The local time zone at `at`, as TZUTC writes it (`-0500`, `0100`).
fn tzutc(at: NaiveDateTime) -> String {
    let secs = crate::clock::board().offset_local(at).local_minus_utc();
    let (sign, secs) = if secs < 0 { ("-", -secs) } else { ("", secs) };
    format!("{sign}{:02}{:02}", secs / 3600, secs % 3600 / 60)
}

fn origin_line(state: &BbsState, net: &FtnNetwork, me: Addr) -> String {
    let board = state.config();
    let text = crate::editor::strip_codes(if net.origin.trim().is_empty() { &board.bbs.name } else { net.origin.trim() });
    let tail = format!(" ({me})");
    let room = 79usize.saturating_sub(" * Origin: ".len() + tail.len());
    format!(" * Origin: {}{tail}", text.chars().take(room).collect::<String>())
}

/// A line a caller wrote that would pass for a tear or origin line (or
/// SEEN-BY, which the packet writer handles), changed as other tossers
/// do so readers don't take it for the real one.
fn defused(l: &str) -> String {
    if l == "---" || l.starts_with("--- ") {
        format!("-+-{}", &l[3..])
    } else if let Some(rest) = l.strip_prefix(" * Origin:") {
        format!(" + Origin:{rest}")
    } else {
        l.to_string()
    }
}

fn tear_line() -> String {
    format!("--- flux/bbs {}", env!("CARGO_PKG_VERSION"))
}

/// Pack what's been posted here in the echo areas since the last scan
/// into a packet for each network's hub. Returns how many messages.
pub fn scan(state: &BbsState) -> anyhow::Result<usize> {
    let nets = networks(state)?;
    let _lock = lock(state, "toss", true)?;
    let mut n = 0;
    for (net, me, hub) in &nets {
        n += scan_net(state, net, *me, *hub)?;
    }
    Ok(n)
}

fn scan_net(state: &BbsState, net: &FtnNetwork, me: Addr, hub: Addr) -> anyhow::Result<usize> {
    let areas = state.config().ftn.areas_of(&net.name);
    let now = crate::clock::now().naive_local();
    let mut msgs = Vec::new();
    // Recorded once the packet is written: lost mail is worse than a
    // resend.
    let mut sent: Vec<(i64, String, String)> = Vec::new();
    let mut marks: Vec<(String, i64)> = Vec::new();
    // An area mapped to two echoes goes out in the first only.
    let mut done = std::collections::HashSet::new();
    for map in &areas {
        let key = format!("sent:{}", map.area.to_ascii_uppercase());
        // First time: start after the area's history, but send what was
        // posted in the last day (while the echo was being set up).
        let after = match state.msgs.ftn_get(&key)? {
            Some(a) => a,
            None => {
                let a = state.msgs.ftn_start(&map.area, now - chrono::Duration::days(1))?;
                state.msgs.ftn_set(&key, a)?;
                a
            }
        };
        let mut last = after;
        for m in state.msgs.ftn_unsent(&map.area, after)? {
            last = last.max(m.id);
            if !done.insert(m.id) {
                continue;
            }
            // Packed before but never written out: the same MSGID again,
            // so the hub can tell a resend. Pending, a reply packed after
            // it here finds it for REPLY.
            let msgid = match state.msgs.ftn_msgid(m.id)? {
                Some(id) => id,
                None => format!("{me} {:08x}", next_serial(state)?),
            };
            state.msgs.ftn_pending(m.id, &msgid, &map.area)?;
            let mut kludges = vec![("MSGID".to_string(), msgid.clone())];
            if m.reply_to != 0
                && let Some(r) = state.msgs.ftn_msgid(m.reply_to)?
                && !r.is_empty()
                && !r.starts_with('~')
            {
                kludges.push(("REPLY".into(), r));
            }
            kludges.push(("PID".into(), format!("fluxbbs {}", env!("CARGO_PKG_VERSION"))));
            kludges.push(("TZUTC".into(), tzutc(m.posted)));
            kludges.push(("CHRS".into(), "CP437 2".into()));
            let mut body: Vec<String> = m.lines.iter().map(|l| defused(&crate::editor::strip_colours(l))).collect();
            if body.last().is_some_and(|l| !l.trim().is_empty()) {
                body.push(String::new());
            }
            body.push(tear_line());
            body.push(origin_line(state, net, me));
            let mut seen = vec![me.net_node(), hub.net_node()];
            seen.sort_by_key(|s| s.split_once('/').map(|(n, f)| (n.parse::<u32>().unwrap_or(0), f.parse::<u32>().unwrap_or(0))));
            seen.dedup();
            let parts = Parts { area: Some(map.tag.clone()), kludges, body, seen_by: seen, path: vec![me.net_node()] };
            // Colour codes are this board's; they don't travel.
            let from = if m.anonymous { "Anonymous".to_string() } else { crate::editor::strip_colours(&m.from.name) };
            msgs.push(PackedMsg {
                orig_node: me.node,
                dest_node: hub.node,
                orig_net: me.net,
                dest_net: hub.net,
                // Local is cleared when packed (FTS-0001).
                attr: 0,
                cost: 0,
                datetime: ftn::fts_date(m.posted),
                to: if m.to.name.is_empty() { "All".into() } else { crate::editor::strip_colours(&m.to.name) },
                from,
                subject: crate::editor::strip_colours(&m.subject),
                text: ftn::join_text(&parts),
            });
            sent.push((m.id, msgid, map.area.clone()));
        }
        // Not past a post that's deleted or private for now (a week at
        // most): restored, it still goes.
        if let Some(held) = state.msgs.ftn_held(&map.area, after, now - chrono::Duration::days(7))? {
            last = last.min(held - 1);
        }
        if last > after {
            marks.push((key, last));
        }
    }
    state.msgs.ftn_forget_waits(now - chrono::Duration::days(30))?;
    let n = msgs.len();
    if n > 0 {
        write_packet(state, &net.name, &Packet { orig: me, dest: hub, password: net.packet_password.clone(), created: now, msgs })?;
        tracing::info!("ftn: packed {n} message(s) for {hub} ({})", net.name);
    }
    state.msgs.ftn_sent(&sent, &marks)?;
    Ok(n)
}

/// Send a netmail (to AreaFix, say) on network `net` with its next poll.
pub fn netmail(state: &BbsState, net: &FtnNetwork, from: &str, to: &str, to_addr: Addr, subject: &str, body: &[String]) -> anyhow::Result<()> {
    let (me, hub) = addresses(net)?;
    let _lock = lock(state, "toss", true)?;
    let msgid = format!("{me} {:08x}", next_serial(state)?);
    let mut kludges = vec![("INTL".to_string(), format!("{} {}", Addr { point: 0, ..to_addr }, Addr { point: 0, ..me }))];
    if me.point != 0 {
        kludges.push(("FMPT".into(), me.point.to_string()));
    }
    if to_addr.point != 0 {
        kludges.push(("TOPT".into(), to_addr.point.to_string()));
    }
    kludges.push(("MSGID".into(), msgid));
    kludges.push(("PID".into(), format!("fluxbbs {}", env!("CARGO_PKG_VERSION"))));
    kludges.push(("TZUTC".into(), tzutc(crate::clock::now().naive_local())));
    let mut lines = body.to_vec();
    lines.push(String::new());
    lines.push(tear_line());
    let parts = Parts { area: None, kludges, body: lines, seen_by: Vec::new(), path: Vec::new() };
    let now = crate::clock::now().naive_local();
    let m = PackedMsg {
        orig_node: me.node,
        dest_node: to_addr.node,
        orig_net: me.net,
        dest_net: to_addr.net,
        attr: ftn::ATTR_PRIVATE,
        cost: 0,
        datetime: ftn::fts_date(now),
        to: to.to_string(),
        from: from.to_string(),
        subject: subject.to_string(),
        text: ftn::join_text(&parts),
    };
    // Netmail goes by way of the hub.
    write_packet(state, &net.name, &Packet { orig: me, dest: hub, password: net.packet_password.clone(), created: now, msgs: vec![m] })?;
    tracing::info!("ftn: netmail to {to} at {to_addr} queued");
    Ok(())
}

/// Write a packet to a network's outbound directory, under a fresh name.
fn write_packet(state: &BbsState, net: &str, pkt: &Packet) -> anyhow::Result<PathBuf> {
    let outbound = outbound_dir(state, net);
    std::fs::create_dir_all(&outbound)?;
    // A name no network's outbound has: a hub of several networks gets
    // them all in one session, where files go by name.
    let base = dirs(state).1;
    let mut all = vec![base.clone()];
    all.extend(std::fs::read_dir(&base).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    let mut serial = crate::clock::now().timestamp_millis() as u32;
    let path = loop {
        let name = format!("{serial:08x}.pkt");
        if !all.iter().any(|d| d.join(&name).exists()) {
            break outbound.join(name);
        }
        serial = serial.wrapping_add(1);
    };
    // Written whole before it's named, so the mailer never sends half.
    let tmp = path.with_extension("tmp");
    write_private(&tmp, &pkt.to_bytes()).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Packets waiting to go, oldest first.
pub fn outbound_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pkt"))).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Ftn, FtnArea};

    fn board() -> (std::sync::Arc<BbsState>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("inq-ftn-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let state = crate::tests::state();
        {
            let mut cfg = (*state.config()).clone();
            cfg.root = dir.clone();
            cfg.paths.data = "data".into();
            cfg.ftn = Ftn {
                enabled: true,
                address: "21:1/999@fsxnet".into(),
                hub: "21:1/100".into(),
                packet_password: "PKTPW".into(),
                areas: vec![FtnArea { tag: "FSX_GEN".into(), area: "GENERAL".into(), network: String::new() }],
                ..Ftn::default()
            };
            *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
            let mut areas = (*state.areas()).clone();
            areas.areas.push(crate::msgareas::MsgArea::new("General", "GENERAL"));
            *state.areas.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(areas);
        }
        let mut sysop = crate::user::User { handle: "SysOp".into(), real_name: "System Operator".into(), ..Default::default() };
        state.users.insert(&mut sysop).unwrap();
        (state, dir)
    }

    fn echo(from: &str, subject: &str, msgid: &str, reply: Option<&str>, body: &str, tag: &str) -> PackedMsg {
        let mut kludges = vec![("MSGID".to_string(), msgid.to_string())];
        if let Some(r) = reply {
            kludges.push(("REPLY".into(), r.into()));
        }
        let parts = Parts { area: Some(tag.into()), kludges, body: vec![body.into(), "--- x".into(), " * Origin: far away (21:1/5)".into()], seen_by: vec!["1/5".into(), "1/100".into()], path: vec!["1/5".into()] };
        PackedMsg { orig_node: 5, dest_node: 999, orig_net: 1, dest_net: 1, datetime: "20 Oct 26  12:00:00".into(), to: "All".into(), from: from.into(), subject: subject.into(), text: ftn::join_text(&parts), ..Default::default() }
    }

    #[test]
    fn bundles_bad_files_and_late_parents() {
        let (state, dir) = board();
        let (inbound, _, bad) = dirs(&state);
        std::fs::create_dir_all(&inbound).unwrap();
        let pkt = |msgs: Vec<PackedMsg>| Packet { orig: Addr::parse("21:1/100").unwrap(), dest: Addr::parse("21:1/999").unwrap(), password: "PKTPW".into(), created: crate::clock::now().naive_local(), msgs }.to_bytes();
        // A bundle: an unmapped echo's packet first, then a good one with a
        // reply that comes before its parent.
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("a.pkt", opts).unwrap();
        std::io::Write::write_all(&mut z, &pkt(vec![echo("Eve", "x", "21:1/5 00000010", None, "x", "FSX_BOT")])).unwrap();
        z.start_file("b.pkt", opts).unwrap();
        std::io::Write::write_all(&mut z, &pkt(vec![echo("Bob", "Re: hi", "21:1/5 00000012", Some("21:1/5 00000011"), "reply", "FSX_GEN"), echo("Al", "hi", "21:1/5 00000011", None, "parent", "FSX_GEN")])).unwrap();
        let bundle = z.finish().unwrap().into_inner();
        std::fs::write(inbound.join("00000064.su0"), &bundle).unwrap();
        let t = toss(&state).unwrap();
        assert_eq!((t.packets, t.echomail, t.unknown, t.bad), (2, 2, 1, 0), "the good packet is tossed all the same");
        let msgs = state.msgs.all("GENERAL").unwrap();
        let (reply, parent) = (&msgs[0], &msgs[1]);
        assert_eq!((reply.reply_to, parent.replies), (parent.id, 1), "threaded once the parent came");
        // The unmapped echo's message is kept apart, and comes in once the
        // echo has an area.
        let unknown = inbound.parent().unwrap().join("unknown");
        let kept: Vec<_> = std::fs::read_dir(&unknown).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(kept.len() == 1 && kept[0].starts_with("fsxnet~FSX_BOT-"), "{kept:?}");
        assert_eq!(toss(&state).unwrap().echomail, 0, "not retossed until mapped");
        {
            let mut cfg = (*state.config()).clone();
            cfg.ftn.areas.push(FtnArea { tag: "FSX_BOT".into(), area: "GENERAL".into(), network: String::new() });
            *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
        }
        assert_eq!(toss(&state).unwrap().echomail, 1);
        assert_eq!(std::fs::read_dir(&unknown).unwrap().count(), 0);
        // Files that can't be read here go to bad, and one of a name
        // there already doesn't take its place.
        std::fs::write(inbound.join("00000064.mo1"), b"`\xeaARJ").unwrap();
        assert_eq!(toss(&state).unwrap().bad, 1);
        std::fs::write(inbound.join("00000064.mo1"), b"`\xeaARJ").unwrap();
        toss(&state).unwrap();
        assert!(bad.join("00000064.mo1").exists() && bad.join("00000064-1.mo1").exists());
        // An empty one is just removed.
        std::fs::write(inbound.join("00000065.tu0"), b"").unwrap();
        assert_eq!(toss(&state).unwrap(), Tossed::default());
        assert!(!inbound.join("00000065.tu0").exists());
        // A date no one could mean (and a TZUTC to push it over) is now.
        let mut odd = echo("Mal", "far", "21:1/5 00000099", None, "x", "FSX_GEN");
        odd.datetime = "31 Dec 262142 23:59".into();
        let mut parts = ftn::split_text(&odd.text);
        parts.kludges.push(("TZUTC".into(), "-1400".into()));
        odd.text = ftn::join_text(&parts);
        std::fs::write(inbound.join("00000066.pkt"), pkt(vec![odd])).unwrap();
        assert_eq!(toss(&state).unwrap().echomail, 1, "tossed, not a panic");
        let far = state.msgs.all("GENERAL").unwrap().into_iter().find(|m| m.subject == "far").unwrap();
        assert!((far.posted - crate::clock::now().naive_local()).num_minutes().abs() < 5);
        // Netmail for someone else is kept for the SysOp; the echomail in
        // the same packet is tossed.
        let parts = Parts { kludges: vec![("INTL".into(), "21:3/4 21:1/5".into()), ("MSGID".into(), "21:1/5 0000aaaa".into())], body: vec!["for 3/4".into()], ..Default::default() };
        let stray = PackedMsg { orig_node: 5, dest_node: 4, orig_net: 1, dest_net: 3, datetime: "20 Oct 26  12:00:00".into(), to: "Someone".into(), from: "Al".into(), subject: "not you".into(), text: ftn::join_text(&parts), ..Default::default() };
        std::fs::write(inbound.join("00000067.pkt"), pkt(vec![stray, echo("Al", "also", "21:1/5 0000aaab", None, "x", "FSX_GEN")])).unwrap();
        let t = toss(&state).unwrap();
        assert_eq!((t.echomail, t.bad), (1, 1));
        assert!(std::fs::read_dir(&bad).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("netmail-")));
        assert_eq!(tzutc_minutes("-0500"), Some(-300));
        assert_eq!(tzutc_minutes("1300"), Some(780));
        assert_eq!(tzutc_minutes("+0545"), Some(345));
        assert_eq!(tzutc_minutes("99"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replies_in_one_scan_and_resends_keep_msgids() {
        let (state, dir) = board();
        let outbound = outbound_dir(&state, "fsxnet");
        assert_eq!(scan(&state).unwrap(), 0);
        let post = |subject: &str, reply_to: i64| {
            let mut m = Message {
                id: 0,
                area: "GENERAL".into(),
                num: 0,
                from: party("phrac"),
                to: party("All"),
                subject: subject.into(),
                posted: crate::clock::now().naive_local(),
                deleted: false,
                private: false,
                anonymous: false,
                replies: 0,
                sig_pos: 0,
                tag: 0,
                reply_to,
                lines: vec!["text".into()],
            };
            state.msgs.add(&mut m).unwrap();
            m.id
        };
        let first = post("first", 0);
        post("Re: first", first);
        // A MSGID given in a scan whose packet was never written.
        state.msgs.ftn_pending(first, "21:1/999 0badc0de", "GENERAL").unwrap();
        assert_eq!(scan(&state).unwrap(), 2);
        let out = Packet::parse(&std::fs::read(&outbound_files(&outbound)[0]).unwrap()).unwrap();
        let (a, b) = (ftn::split_text(&out.msgs[0].text), ftn::split_text(&out.msgs[1].text));
        assert_eq!(a.kludge("MSGID"), Some("21:1/999 0badc0de"));
        assert_eq!(b.kludge("REPLY"), Some("21:1/999 0badc0de"));
        assert_eq!(scan(&state).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_scans_held_posts_and_typed_origin_lines() {
        let (state, dir) = board();
        let outbound = outbound_dir(&state, "fsxnet");
        let post = |subject: &str, days_ago: i64, lines: Vec<&str>| {
            let mut m = Message {
                id: 0,
                area: "GENERAL".into(),
                num: 0,
                from: party("phrac"),
                to: party("All"),
                subject: subject.into(),
                posted: crate::clock::now().naive_local() - chrono::Duration::days(days_ago),
                deleted: false,
                private: false,
                anonymous: false,
                replies: 0,
                sig_pos: 0,
                tag: 0,
                reply_to: 0,
                lines: lines.into_iter().map(String::from).collect(),
            };
            state.msgs.add(&mut m).unwrap();
            m
        };
        // History isn't sent on the first scan; a hello written while the
        // echo was being set up is.
        post("old", 30, vec!["old"]);
        post("hello", 0, vec!["hi all", "--- not my tear", " * Origin: not mine (1:2/3)"]);
        assert_eq!(scan(&state).unwrap(), 1);
        let files = outbound_files(&outbound);
        let out = Packet::parse(&std::fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(out.msgs[0].subject, "hello");
        let body = ftn::split_text(&out.msgs[0].text).body;
        assert!(body.contains(&"-+- not my tear".to_string()) && body.contains(&" + Origin: not mine (1:2/3)".to_string()), "{body:?}");
        std::fs::remove_file(&files[0]).unwrap();
        // Deleted when scanned, restored after: it still goes.
        let gone = post("second thoughts", 0, vec!["x"]);
        state.msgs.set_deleted(gone.id, true, "SysOp").unwrap();
        post("after it", 0, vec!["y"]);
        assert_eq!(scan(&state).unwrap(), 1);
        state.msgs.set_deleted(gone.id, false, "SysOp").unwrap();
        assert_eq!(scan(&state).unwrap(), 1);
        assert_eq!(scan(&state).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn toss_and_scan() {
        let (state, dir) = board();
        let (inbound, _, _) = dirs(&state);
        let outbound = outbound_dir(&state, "fsxnet");
        std::fs::create_dir_all(&inbound).unwrap();
        let netmail_parts = Parts { kludges: vec![("INTL".into(), "21:1/999 21:1/100".into()), ("MSGID".into(), "21:1/100 00000009".into())], body: vec!["Linked FSX_GEN".into()], ..Default::default() };
        let areafix_reply = PackedMsg { orig_node: 100, dest_node: 999, orig_net: 1, dest_net: 1, datetime: "20 Oct 26  12:05:00".into(), to: "SysOp".into(), from: "AreaFix".into(), subject: "reply".into(), text: ftn::join_text(&netmail_parts), ..Default::default() };
        let pkt = Packet {
            orig: Addr::parse("21:1/100").unwrap(),
            dest: Addr::parse("21:1/999").unwrap(),
            password: "PKTPW".into(),
            created: crate::clock::now().naive_local(),
            msgs: vec![
                echo("Alice", "hello", "21:1/5 00000001", None, "first post", "FSX_GEN"),
                echo("Bob", "Re: hello", "21:1/5 00000002", Some("21:1/5 00000001"), "a reply that goes on and on ".repeat(5).trim(), "FSX_GEN"),
                echo("Alice", "hello", "21:1/5 00000001", None, "first post", "FSX_GEN"),
                echo("Carol", "elsewhere", "21:1/5 00000003", None, "x", "FSX_OTHER"),
                areafix_reply,
            ],
        };
        std::fs::write(inbound.join("0000abcd.pkt"), pkt.to_bytes()).unwrap();
        let t = toss(&state).unwrap();
        // FSX_OTHER has no area here: its message is kept apart, the rest
        // tossed.
        assert_eq!(t, Tossed { packets: 1, echomail: 2, netmail: 1, dupes: 1, unknown: 1, bad: 0, files: 0 });
        assert!(std::fs::read_dir(&inbound).unwrap().next().is_none());
        assert!(std::fs::read_dir(inbound.parent().unwrap().join("unknown")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("fsxnet~FSX_OTHER-")));
        let msgs = state.msgs.all("GENERAL").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].reply_to, msgs[0].id);
        assert_eq!(msgs[0].replies, 1);
        assert_eq!(msgs[0].from.note, "21:1/5");
        assert!(msgs[1].lines.iter().all(|l| l.chars().count() <= 79));
        let mail = state.msgs.all("EMAIL").unwrap();
        assert_eq!((mail[0].to.name.as_str(), mail[0].from.name.as_str()), ("SysOp", "AreaFix@21:1/100"));

        // The first scan only marks where the area starts.
        assert_eq!(scan(&state).unwrap(), 0);
        let mut local = Message {
            id: 0,
            area: "GENERAL".into(),
            num: 0,
            from: party("phrac"),
            to: party("Bob"),
            subject: "Re: hello".into(),
            posted: crate::clock::now().naive_local(),
            deleted: false,
            private: false,
            anonymous: false,
            replies: 0,
            sig_pos: 0,
            tag: 0,
            reply_to: msgs[1].id,
            lines: vec!["|15nice|07 one".into()],
        };
        state.msgs.add(&mut local).unwrap();
        assert_eq!(scan(&state).unwrap(), 1);
        assert_eq!(scan(&state).unwrap(), 0, "sent once");
        let files = outbound_files(&outbound);
        let out = Packet::parse(&std::fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!((out.orig.to_string(), out.dest.to_string(), out.password.as_str()), ("21:1/999".into(), "21:1/100".into(), "PKTPW"));
        let p = ftn::split_text(&out.msgs[0].text);
        assert_eq!(p.area.as_deref(), Some("FSX_GEN"));
        assert!(p.kludge("MSGID").unwrap().starts_with("21:1/999 "));
        assert_eq!(p.kludge("REPLY"), Some("21:1/5 00000002"));
        assert_eq!(p.body[0], "nice one");
        assert!(p.body.iter().any(|l| l.starts_with(" * Origin: ") && l.ends_with("(21:1/999)")));
        assert_eq!(p.seen_by, ["1/100", "1/999"]);
        assert_eq!(p.path, ["1/999"]);

        // A message without a MSGID is still a dupe the second time; a ZIP
        // that isn't a mail bundle (a nodelist) is left alone.
        let mut bare = echo("Dave", "no id", "", None, "plain", "FSX_GEN");
        bare.text = ftn::join_text(&Parts { kludges: Vec::new(), ..ftn::split_text(&bare.text) });
        let pkt2 = Packet { msgs: vec![bare.clone(), bare], ..pkt.clone() };
        std::fs::write(inbound.join("0000abce.pkt"), pkt2.to_bytes()).unwrap();
        let mut nodelist = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        nodelist.start_file("FSXNET.293", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut nodelist, b";A nodelist").unwrap();
        std::fs::write(inbound.join("FSXNET.Z93"), nodelist.finish().unwrap().into_inner()).unwrap();
        let t = toss(&state).unwrap();
        assert_eq!((t.echomail, t.dupes), (1, 1));
        assert!(inbound.join("FSXNET.Z93").exists());

        netmail(&state, &network(&state, "fsxnet").unwrap(), "SysOp", "AreaFix", Addr::parse("21:1/100").unwrap(), "secret", &["+FSX_BOT".into()]).unwrap();
        let files = outbound_files(&outbound);
        assert_eq!(files.len(), 2);
        let nm = Packet::parse(&std::fs::read(&files[1]).unwrap()).unwrap();
        let p = ftn::split_text(&nm.msgs[0].text);
        assert_eq!((p.area.as_deref(), p.kludge("INTL")), (None, Some("21:1/100 21:1/999")));
        assert_eq!(nm.msgs[0].attr & ftn::ATTR_PRIVATE, ftn::ATTR_PRIVATE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_networks_each_their_own() {
        let (state, dir) = board();
        {
            let mut cfg = (*state.config()).clone();
            cfg.ftn.networks.push(crate::config::FtnNetwork {
                name: "agoranet".into(),
                address: "46:1/200@agoranet".into(),
                hub: "46:1/100".into(),
                ..Default::default()
            });
            cfg.ftn.areas.push(FtnArea { tag: "AGN_GEN".into(), area: "AGNGEN".into(), network: "agoranet".into() });
            *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
            let mut areas = (*state.areas()).clone();
            areas.areas.push(crate::msgareas::MsgArea::new("Agora general", "AGNGEN"));
            *state.areas.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(areas);
        }
        let names: Vec<String> = state.config().ftn.networks().into_iter().map(|n| n.name).collect();
        assert_eq!(names, ["fsxnet", "agoranet"]);
        let (inbound, _, _) = dirs(&state);
        std::fs::create_dir_all(&inbound).unwrap();
        let from_agora = |msgs: Vec<PackedMsg>| Packet { orig: Addr::parse("46:1/100").unwrap(), dest: Addr::parse("46:1/200").unwrap(), password: String::new(), created: crate::clock::now().naive_local(), msgs };
        let mut m = echo("Acc", "hi", "46:1/100 00000001", None, "agora", "AGN_GEN");
        (m.orig_net, m.dest_net, m.dest_node) = (1, 1, 200);
        // FSX_GEN isn't an echo of this network's, whatever its tag says.
        let wrong = echo("Eve", "x", "46:1/100 00000002", None, "x", "FSX_GEN");
        std::fs::write(inbound.join("00000001.pkt"), from_agora(vec![m, wrong]).to_bytes()).unwrap();
        let t = toss(&state).unwrap();
        assert_eq!((t.echomail, t.unknown), (1, 1));
        assert_eq!(state.msgs.all("AGNGEN").unwrap().len(), 1);
        assert!(state.msgs.all("GENERAL").unwrap().is_empty());
        // What agoranet's hub sent in its own session can't pass for
        // fsxnet's mail, whatever the packet says.
        let agn_in = inbound_dir(&state, "agoranet");
        std::fs::create_dir_all(&agn_in).unwrap();
        let forged = Packet { orig: Addr::parse("21:1/100").unwrap(), dest: Addr::parse("21:1/999").unwrap(), password: "PKTPW".into(), created: crate::clock::now().naive_local(), msgs: vec![echo("Mallory", "x", "21:1/100 00000003", None, "x", "FSX_GEN")] };
        std::fs::write(agn_in.join("00000003.pkt"), forged.to_bytes()).unwrap();
        assert_eq!(toss(&state).unwrap().bad, 1);
        assert!(state.msgs.all("GENERAL").unwrap().is_empty());
        // A packet from a stranger on a network we're on goes to bad.
        let stranger = Packet { orig: Addr::parse("46:1/5").unwrap(), ..from_agora(Vec::new()) };
        std::fs::write(inbound.join("00000002.pkt"), stranger.to_bytes()).unwrap();
        assert_eq!(toss(&state).unwrap().bad, 1);

        // Posts go out each to their own network's hub, from our address
        // there.
        assert_eq!(scan(&state).unwrap(), 0);
        for area in ["GENERAL", "AGNGEN"] {
            let mut m = Message {
                id: 0,
                area: area.into(),
                num: 0,
                from: party("phrac"),
                to: party("All"),
                subject: format!("in {area}"),
                posted: crate::clock::now().naive_local(),
                deleted: false,
                private: false,
                anonymous: false,
                replies: 0,
                sig_pos: 0,
                tag: 0,
                reply_to: 0,
                lines: vec!["hello".into()],
            };
            state.msgs.add(&mut m).unwrap();
        }
        assert_eq!(scan(&state).unwrap(), 2);
        for (net, me, hub, tag) in [("fsxnet", "21:1/999", "21:1/100", "FSX_GEN"), ("agoranet", "46:1/200", "46:1/100", "AGN_GEN")] {
            let files = outbound_files(&outbound_dir(&state, net));
            assert_eq!(files.len(), 1, "{net}");
            let p = Packet::parse(&std::fs::read(&files[0]).unwrap()).unwrap();
            assert_eq!((p.orig.to_string(), p.dest.to_string()), (me.to_string(), hub.to_string()));
            let parts = ftn::split_text(&p.msgs[0].text);
            assert_eq!(parts.area.as_deref(), Some(tag));
            assert!(parts.body.iter().any(|l| l.contains(&format!("({me})"))), "the origin line has our address there");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_networks_are_left_out_and_said_so() {
        let n = |name: &str, addr: &str| crate::config::FtnNetwork { name: name.into(), address: addr.into(), hub: "46:1/100".into(), ..Default::default() };
        let ftn = Ftn {
            enabled: true,
            name: "FSX".into(),
            address: "21:1/999@fsxnet".into(),
            hub: "21:1/100".into(),
            networks: vec![n("../inbound", "46:1/1@agoranet"), n("fsx", "46:1/2"), n("agoranet", "46:1/200"), n("", "")],
            areas: vec![
                FtnArea { tag: "AGN_GEN".into(), area: "AGNGEN".into(), network: "agoranet".into() },
                FtnArea { tag: "X_GEN".into(), area: "AGNGEN".into(), network: "nope".into() },
            ],
            ..Ftn::default()
        };
        let names: Vec<String> = ftn.networks().into_iter().map(|n| n.name).collect();
        assert_eq!(names, ["fsx", "agoranet"], "a path for a name and a second fsx are left out; one with no address is off");
        let p = ftn.problems().join("\n");
        assert!(p.contains("\"../inbound\" left out") && p.contains("\"fsx\" left out"), "{p}");
        assert!(p.contains("X_GEN") && p.contains("two networks"), "{p}");
    }
}
