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
use chrono::Local;

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

/// Toss everything in the inbound directory.
pub fn toss(state: &BbsState) -> anyhow::Result<Tossed> {
    let cfg = state.config().ftn.clone();
    let (me, _) = addresses(&cfg)?;
    let (inbound, _, bad) = dirs(state);
    std::fs::create_dir_all(&inbound)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&inbound)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let mut t = Tossed::default();
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Part-received files belong to the mailer.
        if name.ends_with(".part") || name.starts_with('.') {
            continue;
        }
        let data = std::fs::read(&path)?;
        let result = if data.starts_with(b"PK\x03\x04") {
            toss_bundle(state, &data, me, &cfg, &mut t)
        } else if name.to_ascii_lowercase().ends_with(".pkt") {
            toss_packet(state, &data, me, &cfg, &mut t)
        } else {
            // TICs and files of file echoes aren't handled; leave them.
            continue;
        };
        match result {
            Ok(()) => std::fs::remove_file(&path)?,
            Err(e) => {
                tracing::warn!("ftn: {name}: {e}");
                std::fs::create_dir_all(&bad)?;
                std::fs::rename(&path, bad.join(&name))?;
                t.bad += 1;
            }
        }
    }
    if t != Tossed::default() {
        tracing::info!("ftn: tossed {t:?}");
    }
    Ok(t)
}

fn toss_bundle(state: &BbsState, data: &[u8], me: Addr, cfg: &Ftn, t: &mut Tossed) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if !f.name().map_err(|e| e.to_string())?.to_ascii_lowercase().ends_with(".pkt") {
            continue;
        }
        let mut pkt = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut pkt).map_err(|e| e.to_string())?;
        toss_packet(state, &pkt, me, cfg, t)?;
    }
    Ok(())
}

fn toss_packet(state: &BbsState, data: &[u8], me: Addr, cfg: &Ftn, t: &mut Tossed) -> Result<(), String> {
    let pkt = Packet::parse(data)?;
    if (pkt.dest.zone != 0 && pkt.dest.zone != me.zone) || pkt.dest.net != me.net || pkt.dest.node != me.node {
        return Err(format!("addressed to {}, not {me}", pkt.dest));
    }
    if !cfg.packet_password.is_empty() && !pkt.password.eq_ignore_ascii_case(&cfg.packet_password) {
        return Err(format!("wrong packet password from {}", pkt.orig));
    }
    t.packets += 1;
    for m in &pkt.msgs {
        let parts = ftn::split_text(&m.text);
        let r = match &parts.area {
            Some(tag) => toss_echo(state, cfg, tag, m, &parts, t),
            None => toss_netmail(state, m, &parts, me, pkt.orig, t),
        };
        r.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Lines as a reader sees them: long ones word-wrapped to 79.
fn wrapped(body: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for l in body {
        if l.chars().count() <= 79 {
            out.push(l.clone());
        } else {
            out.extend(crate::multinode::wrap(l, 79));
        }
    }
    out
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
    let msgid = parts.kludge("MSGID").unwrap_or("").to_string();
    if !msgid.is_empty() && state.msgs.ftn_find(area, &msgid)?.is_some() {
        t.dupes += 1;
        return Ok(());
    }
    let parent = match parts.kludge("REPLY") {
        Some(r) => state.msgs.ftn_find(area, r)?,
        None => None,
    };
    let mut from = party(&m.from);
    from.note = parts.origin_addr().map(|a| a.to_string()).unwrap_or_default();
    let mut msg = Message {
        id: 0,
        area: area.to_string(),
        num: 0,
        from,
        to: party(&m.to),
        subject: m.subject.clone(),
        posted: ftn::parse_fts_date(&m.datetime).unwrap_or_else(|| Local::now().naive_local()),
        deleted: false,
        private: false,
        anonymous: false,
        replies: 0,
        sig_pos: 0,
        tag: 0,
        reply_to: parent.unwrap_or(0),
        lines: wrapped(&parts.body),
    };
    state.msgs.add(&mut msg)?;
    state.msgs.ftn_record(msg.id, &msgid, area, false)?;
    if let Some(p) = parent
        && let Some(mut pm) = state.msgs.by_id(p)?
    {
        pm.replies += 1;
        state.msgs.update(&pm)?;
    }
    let name = state.areas().areas.iter().find(|a| a.file.eq_ignore_ascii_case(area)).map(|a| a.name.clone()).unwrap_or_else(|| area.to_string());
    crate::mentions::record(state, &msg, &name, 0);
    t.echomail += 1;
    Ok(())
}

fn toss_netmail(state: &BbsState, m: &PackedMsg, parts: &Parts, me: Addr, pkt_orig: Addr, t: &mut Tossed) -> anyhow::Result<()> {
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
    if dest != me {
        tracing::info!("ftn: netmail for {dest}, not routed here");
        return Ok(());
    }
    let msgid = parts.kludge("MSGID").unwrap_or("").to_string();
    let email = state.areas().email.file.clone();
    if !msgid.is_empty() && state.msgs.ftn_find(&email, &msgid)?.is_some() {
        t.dupes += 1;
        return Ok(());
    }
    // To the user named, or the SysOp.
    let user = state.users.find(&m.to, false)?.or_else(|| state.users.get(1).ok().flatten());
    let to = match &user {
        Some(u) => Party { user_id: u.id, alias: u.handle.clone(), real_name: u.real_name.clone(), name: u.handle.clone(), note: String::new() },
        None => party(&m.to),
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
        posted: ftn::parse_fts_date(&m.datetime).unwrap_or_else(|| Local::now().naive_local()),
        deleted: false,
        private: true,
        anonymous: false,
        replies: 0,
        sig_pos: 0,
        tag: 0,
        reply_to: 0,
        lines: wrapped(&parts.body),
    };
    state.msgs.add(&mut msg)?;
    state.msgs.ftn_record(msg.id, &msgid, &email, false)?;
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

/// The local time zone, as TZUTC writes it (`-0500`, `0100`).
fn tzutc() -> String {
    let secs = Local::now().offset().local_minus_utc();
    let (sign, secs) = if secs < 0 { ("-", -secs) } else { ("", secs) };
    format!("{sign}{:02}{:02}", secs / 3600, secs % 3600 / 60)
}

fn origin_line(state: &BbsState, cfg: &Ftn, me: Addr) -> String {
    let text = if cfg.origin.trim().is_empty() { state.config().bbs.name.clone() } else { cfg.origin.trim().to_string() };
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
    let now = Local::now().naive_local();
    let mut msgs = Vec::new();
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
            let msgid = format!("{me} {:08x}", next_serial(state)?);
            let mut kludges = vec![("MSGID".to_string(), msgid.clone())];
            if m.reply_to != 0
                && let Some(r) = state.msgs.ftn_msgid(m.reply_to)?
                && !r.is_empty()
            {
                kludges.push(("REPLY".into(), r));
            }
            kludges.push(("PID".into(), format!("Iniquity/Rust {}", env!("CARGO_PKG_VERSION"))));
            kludges.push(("TZUTC".into(), tzutc()));
            kludges.push(("CHRS".into(), "CP437 2".into()));
            let mut body: Vec<String> = m.lines.iter().map(|l| crate::editor::strip_codes(l)).collect();
            if body.last().is_some_and(|l| !l.trim().is_empty()) {
                body.push(String::new());
            }
            body.push(tear_line());
            body.push(origin_line(state, &cfg, me));
            let mut seen = vec![me.net_node(), hub.net_node()];
            seen.sort_by_key(|s| s.split_once('/').map(|(n, f)| (n.parse::<u32>().unwrap_or(0), f.parse::<u32>().unwrap_or(0))));
            seen.dedup();
            let parts = Parts { area: Some(map.tag.clone()), kludges, body, seen_by: seen, path: vec![me.net_node()] };
            let from = if m.anonymous { "Anonymous".to_string() } else { m.from.name.clone() };
            msgs.push(PackedMsg {
                orig_node: me.node,
                dest_node: hub.node,
                orig_net: me.net,
                dest_net: hub.net,
                attr: ftn::ATTR_LOCAL,
                cost: 0,
                datetime: ftn::fts_date(m.posted),
                to: if m.to.name.is_empty() { "All".into() } else { m.to.name.clone() },
                from,
                subject: m.subject.clone(),
                text: ftn::join_text(&parts),
            });
            state.msgs.ftn_record(m.id, &msgid, &map.area, true)?;
        }
        if last > after {
            state.msgs.ftn_set(&key, last)?;
        }
    }
    let n = msgs.len();
    if n > 0 {
        write_packet(state, &Packet { orig: me, dest: hub, password: cfg.packet_password.clone(), created: now, msgs })?;
        tracing::info!("ftn: packed {n} message(s) for {hub}");
    }
    Ok(n)
}

/// Send a netmail (to AreaFix, say) with the next poll.
pub fn netmail(state: &BbsState, from: &str, to: &str, to_addr: Addr, subject: &str, body: &[String]) -> anyhow::Result<()> {
    let cfg = state.config().ftn.clone();
    let (me, hub) = addresses(&cfg)?;
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
    kludges.push(("TZUTC".into(), tzutc()));
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
        attr: ftn::ATTR_PRIVATE | ftn::ATTR_LOCAL,
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
        assert_eq!(t, Tossed { packets: 1, echomail: 2, netmail: 1, dupes: 1, unknown: 1, bad: 0 });
        assert!(std::fs::read_dir(&inbound).unwrap().next().is_none());
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
