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
use chrono::{Local, NaiveDateTime, TimeZone};

use crate::BbsState;
use crate::config::Ftn;
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

/// This node's and the hub's addresses.
pub fn addresses(cfg: &Ftn) -> anyhow::Result<(Addr, Addr)> {
    let me = Addr::parse(&cfg.address).ok_or_else(|| anyhow!("[ftn] address {:?} isn't an FTN address", cfg.address))?;
    let hub = Addr::parse(&cfg.hub).ok_or_else(|| anyhow!("[ftn] hub {:?} isn't an FTN address", cfg.hub))?;
    Ok((me, hub))
}

/// The inbound, outbound and bad directories.
pub fn dirs(state: &BbsState) -> (PathBuf, PathBuf, PathBuf) {
    let cfg = state.config();
    let base = cfg.resolve(&cfg.paths.data).join("ftn");
    (base.join("inbound"), base.join("outbound"), base.join("bad"))
}

/// Hold `data/ftn/NAME.lock` until the file is dropped: one toss or scan
/// (`toss`), one binkp session (`binkp`) at a time, across processes too
/// (the board, and `iniquity ftn` beside it). `wait` false: None if busy.
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

/// Toss everything in the inbound directory.
pub fn toss(state: &BbsState) -> anyhow::Result<Tossed> {
    let cfg = state.config().ftn.clone();
    let (me, hub) = addresses(&cfg)?;
    let (inbound, _, bad) = dirs(state);
    std::fs::create_dir_all(&inbound)?;
    let _lock = lock(state, "toss", true)?;
    // Oldest first (by name when the clock can't tell), so a thread comes
    // in in order.
    let mut files: Vec<PathBuf> = std::fs::read_dir(&inbound)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort_by_key(|p| (std::fs::metadata(p).and_then(|m| m.modified()).ok(), p.file_name().map(|n| n.to_os_string())));
    let mut t = Tossed::default();
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        // Part-received files belong to the mailer; TICs, file echoes'
        // files and nodelists aren't handled, and are left (unread).
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
        let result = if packet {
            toss_packet(state, &data, me, hub, &cfg, &mut t)
        } else if data.starts_with(b"PK\x03\x04") && bundled(&data) {
            toss_bundle(state, &data, me, hub, &cfg, &mut t)
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
            Err(Fault::Board(e)) => {
                if t != Tossed::default() {
                    tracing::info!("ftn: tossed {t:?}");
                }
                return Err(e.context(format!("ftn: tossing {name} (left in inbound)")));
            }
        }
    }
    if t != Tossed::default() {
        tracing::info!("ftn: tossed {t:?}");
    }
    Ok(t)
}

/// Is this ZIP a mail bundle (packets inside), not some other archive?
fn bundled(data: &[u8]) -> bool {
    zip::ZipArchive::new(std::io::Cursor::new(data)).is_ok_and(|z| z.file_names().any(|n| n.is_ok_and(|n| n.to_ascii_lowercase().ends_with(".pkt"))))
}

/// Every packet in a bundle, whatever happens to one of them.
fn toss_bundle(state: &BbsState, data: &[u8], me: Addr, hub: Addr, cfg: &Ftn, t: &mut Tossed) -> Result<(), Fault> {
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
        match toss_packet(state, &pkt, me, hub, cfg, t) {
            Ok(()) => {}
            Err(Fault::Bad(why)) => problems.push(format!("{name}: {why}")),
            Err(e) => return Err(e),
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(Fault::Bad(problems.join("; "))) }
}

fn toss_packet(state: &BbsState, data: &[u8], me: Addr, hub: Addr, cfg: &Ftn, t: &mut Tossed) -> Result<(), Fault> {
    let pkt = Packet::parse(data).map_err(Fault::Bad)?;
    if (pkt.dest.zone != 0 && pkt.dest.zone != me.zone) || pkt.dest.net != me.net || pkt.dest.node != me.node {
        return Err(Fault::Bad(format!("addressed to {}, not {me}", pkt.dest)));
    }
    // A leaf node's mail all comes through its hub.
    if (pkt.orig.zone != 0 && pkt.orig.zone != hub.zone) || pkt.orig.net != hub.net || pkt.orig.node != hub.node {
        return Err(Fault::Bad(format!("from {}, not the hub {hub}", pkt.orig)));
    }
    // The header holds 8 characters of it.
    let want: String = cfg.packet_password.chars().take(8).collect();
    if !want.is_empty() && !pkt.password.eq_ignore_ascii_case(&want) {
        return Err(Fault::Bad(format!("wrong packet password from {}", pkt.orig)));
    }
    t.packets += 1;
    let unknown = t.unknown;
    let mut problems = Vec::new();
    for m in &pkt.msgs {
        let parts = ftn::split_text(&m.text);
        match &parts.area {
            Some(tag) => toss_echo(state, cfg, tag, m, &parts, t)?,
            None => toss_netmail(state, m, &parts, me, pkt.orig, t, &mut problems)?,
        }
    }
    // Kept (in bad) until the echo is mapped to an area; then a retoss
    // brings in what's missing, the rest being dupes.
    if t.unknown > unknown {
        problems.push(format!("{} message(s) for echoes with no area here", t.unknown - unknown));
    }
    if problems.is_empty() { Ok(()) } else { Err(Fault::Bad(problems.join("; "))) }
}

/// When a message was written, in the board's time: FTN dates are the
/// writer's local time, and TZUTC says how far that is from UTC (fsxNet
/// is mostly New Zealand, half a day ahead).
fn posted(m: &PackedMsg, parts: &Parts) -> NaiveDateTime {
    let Some(t) = ftn::parse_fts_date(&m.datetime) else { return Local::now().naive_local() };
    let Some(offset) = parts.kludge("TZUTC").and_then(tzutc_minutes) else { return t };
    let utc = t - chrono::Duration::minutes(offset);
    chrono::Utc.from_utc_datetime(&utc).with_timezone(&Local).naive_local()
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

fn toss_echo(state: &BbsState, cfg: &Ftn, tag: &str, m: &PackedMsg, parts: &Parts, t: &mut Tossed) -> anyhow::Result<()> {
    let Some(map) = cfg.areas.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)) else {
        t.unknown += 1;
        return Ok(());
    };
    let area = map.area.as_str();
    if !state.areas().areas.iter().any(|a| a.file.eq_ignore_ascii_case(area)) {
        tracing::warn!("ftn: {tag} is mapped to message area {area}, which doesn't exist");
        t.unknown += 1;
        return Ok(());
    }
    let msgid = dupe_key(m, parts);
    if state.msgs.ftn_known(area, &msgid)? {
        t.dupes += 1;
        return Ok(());
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
        state.msgs.ftn_wait_reply(msg.id, area, r)?;
    }
    let name = state.areas().areas.iter().find(|a| a.file.eq_ignore_ascii_case(area)).map(|a| a.name.clone()).unwrap_or_else(|| area.to_string());
    crate::mentions::record(state, &msg, &name, 0);
    t.echomail += 1;
    Ok(())
}

/// What a message is known by for dupes: its MSGID, or for one without,
/// `~` and who, to whom, when and what (never sent on as a REPLY).
fn dupe_key(m: &PackedMsg, parts: &Parts) -> String {
    match parts.kludge("MSGID") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("~{}|{}|{}|{}", m.from, m.to, m.datetime, m.subject),
    }
}

fn toss_netmail(state: &BbsState, m: &PackedMsg, parts: &Parts, me: Addr, pkt_orig: Addr, t: &mut Tossed, problems: &mut Vec<String>) -> anyhow::Result<()> {
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
        problems.push(format!("netmail for {dest} from {orig}, not routed here"));
        return Ok(());
    }
    let msgid = dupe_key(m, parts);
    let email = state.areas().email.file.clone();
    if state.msgs.ftn_known(&email, &msgid)? {
        t.dupes += 1;
        return Ok(());
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
    Ok(())
}

/// A new MSGID serial: never the same twice, kept with the messages.
fn next_serial(state: &BbsState) -> anyhow::Result<u32> {
    let now = Local::now().timestamp();
    let last = state.msgs.ftn_get("serial")?.unwrap_or(0);
    let next = (last + 1).max(now);
    state.msgs.ftn_set("serial", next)?;
    Ok(next as u32)
}

/// The local time zone at `at`, as TZUTC writes it (`-0500`, `0100`).
fn tzutc(at: NaiveDateTime) -> String {
    use chrono::Offset;
    let secs = Local.offset_from_local_datetime(&at).earliest().map_or_else(|| Local::now().offset().fix(), |o| o.fix()).local_minus_utc();
    let (sign, secs) = if secs < 0 { ("-", -secs) } else { ("", secs) };
    format!("{sign}{:02}{:02}", secs / 3600, secs % 3600 / 60)
}

fn origin_line(state: &BbsState, cfg: &Ftn, me: Addr) -> String {
    let board = state.config();
    let text = crate::editor::strip_codes(if cfg.origin.trim().is_empty() { &board.bbs.name } else { cfg.origin.trim() });
    let tail = format!(" ({me})");
    let room = 79usize.saturating_sub(" * Origin: ".len() + tail.len());
    format!(" * Origin: {}{tail}", text.chars().take(room).collect::<String>())
}

fn tear_line() -> String {
    format!("--- Iniquity/Rust {}", env!("CARGO_PKG_VERSION"))
}

/// Pack what's been posted here in the echo areas since the last scan
/// into a packet for the hub. Returns how many messages.
pub fn scan(state: &BbsState) -> anyhow::Result<usize> {
    let cfg = state.config().ftn.clone();
    let (me, hub) = addresses(&cfg)?;
    let _lock = lock(state, "toss", true)?;
    let now = Local::now().naive_local();
    let mut msgs = Vec::new();
    // Recorded once the packet is written: lost mail is worse than a
    // resend.
    let mut sent: Vec<(i64, String, String)> = Vec::new();
    let mut marks: Vec<(String, i64)> = Vec::new();
    // An area mapped to two echoes goes out in the first only.
    let mut done = std::collections::HashSet::new();
    for map in &cfg.areas {
        let key = format!("sent:{}", map.area.to_ascii_uppercase());
        // First time: start from what's there now.
        let Some(after) = state.msgs.ftn_get(&key)? else {
            state.msgs.ftn_set(&key, state.msgs.max_id()?)?;
            continue;
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
            kludges.push(("PID".into(), format!("Iniquity/Rust {}", env!("CARGO_PKG_VERSION"))));
            kludges.push(("TZUTC".into(), tzutc(m.posted)));
            kludges.push(("CHRS".into(), "CP437 2".into()));
            let mut body: Vec<String> = m.lines.iter().map(|l| crate::editor::strip_colours(l)).collect();
            if body.last().is_some_and(|l| !l.trim().is_empty()) {
                body.push(String::new());
            }
            body.push(tear_line());
            body.push(origin_line(state, &cfg, me));
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
        if last > after {
            marks.push((key, last));
        }
    }
    let n = msgs.len();
    if n > 0 {
        write_packet(state, &Packet { orig: me, dest: hub, password: cfg.packet_password.clone(), created: now, msgs })?;
        tracing::info!("ftn: packed {n} message(s) for {hub}");
    }
    state.msgs.ftn_sent(&sent, &marks)?;
    Ok(n)
}

/// Send a netmail (to AreaFix, say) with the next poll.
pub fn netmail(state: &BbsState, from: &str, to: &str, to_addr: Addr, subject: &str, body: &[String]) -> anyhow::Result<()> {
    let cfg = state.config().ftn.clone();
    let (me, hub) = addresses(&cfg)?;
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
    kludges.push(("PID".into(), format!("Iniquity/Rust {}", env!("CARGO_PKG_VERSION"))));
    kludges.push(("TZUTC".into(), tzutc(Local::now().naive_local())));
    let mut lines = body.to_vec();
    lines.push(String::new());
    lines.push(tear_line());
    let parts = Parts { area: None, kludges, body: lines, seen_by: Vec::new(), path: Vec::new() };
    let now = Local::now().naive_local();
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
    write_packet(state, &Packet { orig: me, dest: hub, password: cfg.packet_password.clone(), created: now, msgs: vec![m] })?;
    tracing::info!("ftn: netmail to {to} at {to_addr} queued");
    Ok(())
}

/// Write a packet to the outbound directory, under a fresh name.
fn write_packet(state: &BbsState, pkt: &Packet) -> anyhow::Result<PathBuf> {
    let (_, outbound, _) = dirs(state);
    std::fs::create_dir_all(&outbound)?;
    let mut serial = Local::now().timestamp_millis() as u32;
    let path = loop {
        let p = outbound.join(format!("{serial:08x}.pkt"));
        if !p.exists() {
            break p;
        }
        serial = serial.wrapping_add(1);
    };
    // Written whole before it's named, so the mailer never sends half.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, pkt.to_bytes()).with_context(|| format!("writing {}", tmp.display()))?;
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
    use crate::config::FtnArea;

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
                areas: vec![FtnArea { tag: "FSX_GEN".into(), area: "GENERAL".into() }],
                ..Ftn::default()
            };
            *state.config.write().unwrap() = std::sync::Arc::new(cfg);
            let mut areas = (*state.areas()).clone();
            areas.areas.push(crate::msgareas::MsgArea::new("General", "GENERAL"));
            *state.areas.write().unwrap() = std::sync::Arc::new(areas);
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
        let pkt = |msgs: Vec<PackedMsg>| Packet { orig: Addr::parse("21:1/100").unwrap(), dest: Addr::parse("21:1/999").unwrap(), password: "PKTPW".into(), created: Local::now().naive_local(), msgs }.to_bytes();
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
        assert_eq!((t.packets, t.echomail, t.unknown, t.bad), (2, 2, 1, 1), "the good packet is tossed all the same");
        let msgs = state.msgs.all("GENERAL").unwrap();
        let (reply, parent) = (&msgs[0], &msgs[1]);
        assert_eq!((reply.reply_to, parent.replies), (parent.id, 1), "threaded once the parent came");
        // The same name kept in bad again doesn't take the first's place.
        std::fs::write(inbound.join("00000064.su0"), &bundle).unwrap();
        toss(&state).unwrap();
        assert!(bad.join("00000064.su0").exists() && bad.join("00000064-1.su0").exists());
        // A bundle-named file that isn't a ZIP goes to bad too.
        std::fs::write(inbound.join("00000064.mo1"), b"`\xeaARJ").unwrap();
        assert_eq!(toss(&state).unwrap().bad, 1);
        assert_eq!(tzutc_minutes("-0500"), Some(-300));
        assert_eq!(tzutc_minutes("1300"), Some(780));
        assert_eq!(tzutc_minutes("+0545"), Some(345));
        assert_eq!(tzutc_minutes("99"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replies_in_one_scan_and_resends_keep_msgids() {
        let (state, dir) = board();
        let (_, outbound, _) = dirs(&state);
        assert_eq!(scan(&state).unwrap(), 0);
        let post = |subject: &str, reply_to: i64| {
            let mut m = Message {
                id: 0,
                area: "GENERAL".into(),
                num: 0,
                from: party("phrac"),
                to: party("All"),
                subject: subject.into(),
                posted: Local::now().naive_local(),
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
    fn toss_and_scan() {
        let (state, dir) = board();
        let (inbound, outbound, _) = dirs(&state);
        std::fs::create_dir_all(&inbound).unwrap();
        let netmail_parts = Parts { kludges: vec![("INTL".into(), "21:1/999 21:1/100".into()), ("MSGID".into(), "21:1/100 00000009".into())], body: vec!["Linked FSX_GEN".into()], ..Default::default() };
        let areafix_reply = PackedMsg { orig_node: 100, dest_node: 999, orig_net: 1, dest_net: 1, datetime: "20 Oct 26  12:05:00".into(), to: "SysOp".into(), from: "AreaFix".into(), subject: "reply".into(), text: ftn::join_text(&netmail_parts), ..Default::default() };
        let pkt = Packet {
            orig: Addr::parse("21:1/100").unwrap(),
            dest: Addr::parse("21:1/999").unwrap(),
            password: "PKTPW".into(),
            created: Local::now().naive_local(),
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
        // FSX_OTHER has no area here: the packet is kept, in bad.
        assert_eq!(t, Tossed { packets: 1, echomail: 2, netmail: 1, dupes: 1, unknown: 1, bad: 1 });
        assert!(std::fs::read_dir(&inbound).unwrap().next().is_none());
        assert!(dirs(&state).2.join("0000abcd.pkt").exists());
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
            posted: Local::now().naive_local(),
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

        netmail(&state, "SysOp", "AreaFix", Addr::parse("21:1/100").unwrap(), "secret", &["+FSX_BOT".into()]).unwrap();
        let files = outbound_files(&outbound);
        assert_eq!(files.len(), 2);
        let nm = Packet::parse(&std::fs::read(&files[1]).unwrap()).unwrap();
        let p = ftn::split_text(&nm.msgs[0].text);
        assert_eq!((p.area.as_deref(), p.kludge("INTL")), (None, Some("21:1/100 21:1/999")));
        assert_eq!(nm.msgs[0].attr & ftn::ATTR_PRIVATE, ftn::ATTR_PRIVATE);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
