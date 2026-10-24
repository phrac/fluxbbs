//! New: a binkp/1.0 mailer (FTS-1026), polling the FTN hub: say who we
//! are, give the session password (CRAM-MD5, FTS-1027, if the hub offers
//! it), send the packets in `data/ftn/outbound` and take what the hub has
//! into `data/ftn/inbound`, each file acknowledged (M_GOT) before it's
//! forgotten. Then the tosser runs (`ftntoss.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::BbsState;
use crate::config::FtnNetwork;
use crate::ftn::Addr;

const M_NUL: u8 = 0;
const M_ADR: u8 = 1;
const M_PWD: u8 = 2;
const M_FILE: u8 = 3;
const M_OK: u8 = 4;
const M_EOB: u8 = 5;
const M_GOT: u8 = 6;
const M_ERR: u8 = 7;
const M_BSY: u8 = 8;
const M_GET: u8 = 9;
const M_SKIP: u8 = 10;

/// Data frames are at most this long (the header leaves 15 bits).
const BLOCK: usize = 16 * 1024;
/// Nothing read or written this long and the session is given up.
const IDLE: Duration = Duration::from_secs(300);
/// Files bigger than this are skipped (left at the other end).
const MAX_FILE: u64 = 256 << 20;

/// What a poll moved.
#[derive(Debug, Default, PartialEq)]
pub struct Polled {
    pub sent: Vec<String>,
    pub received: Vec<String>,
}

enum Frame {
    Cmd(u8, String),
    Data(Vec<u8>),
}

fn cmd(id: u8, args: &str) -> Vec<u8> {
    let len = 1 + args.len();
    let mut f = vec![0x80 | ((len >> 8) as u8 & 0x7F), len as u8, id];
    f.extend_from_slice(args.as_bytes());
    f
}

fn data(chunk: &[u8]) -> Vec<u8> {
    let mut f = vec![(chunk.len() >> 8) as u8 & 0x7F, chunk.len() as u8];
    f.extend_from_slice(chunk);
    f
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Fill `buf`. Only quiet both ways for `IDLE` gives up: while one of our
/// files goes out the other side may say nothing (`wrote`: when we last
/// wrote, in `now_ms`).
async fn fill(rd: &mut (impl AsyncReadExt + Unpin), buf: &mut [u8], wrote: &AtomicU64) -> anyhow::Result<()> {
    let mut got = 0;
    let mut last = now_ms();
    while got < buf.len() {
        match tokio::time::timeout(Duration::from_secs(5), rd.read(&mut buf[got..])).await {
            Ok(Ok(0)) => bail!("binkp: connection closed"),
            Ok(Ok(n)) => {
                got += n;
                last = now_ms();
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {
                if now_ms().saturating_sub(last.max(wrote.load(Ordering::Relaxed))) > IDLE.as_millis() as u64 {
                    bail!("binkp: the other side went quiet");
                }
            }
        }
    }
    Ok(())
}

async fn read_frame(rd: &mut (impl AsyncReadExt + Unpin), wrote: &AtomicU64) -> anyhow::Result<Frame> {
    let mut h = [0u8; 2];
    let body = loop {
        fill(rd, &mut h, wrote).await?;
        let len = (((h[0] & 0x7F) as usize) << 8) | h[1] as usize;
        let mut body = vec![0u8; len];
        fill(rd, &mut body, wrote).await?;
        // An empty command frame is dropped (FTS-1026: some old mailers
        // end with one).
        if !(h[0] & 0x80 != 0 && body.is_empty()) {
            break body;
        }
    };
    if h[0] & 0x80 == 0 {
        return Ok(Frame::Data(body));
    }
    let Some((&id, args)) = body.split_first() else { bail!("binkp: empty command frame") };
    let args = String::from_utf8_lossy(args).trim_end_matches('\0').to_string();
    Ok(Frame::Cmd(id, args))
}

/// `CRAM-MD5-<digest>`: HMAC-MD5 of the hub's challenge, keyed with the
/// password (FTS-1027).
pub fn cram_md5(password: &str, challenge_hex: &str) -> Option<String> {
    use hmac::{Hmac, Mac};
    let hex = challenge_hex.as_bytes();
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let challenge: Vec<u8> = hex.chunks(2).map(|c| std::str::from_utf8(c).ok().and_then(|c| u8::from_str_radix(c, 16).ok())).collect::<Option<_>>()?;
    let mut mac = Hmac::<md5::Md5>::new_from_slice(password.as_bytes()).ok()?;
    mac.update(&challenge);
    Some(format!("CRAM-MD5-{}", mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// The challenge in an OPT's CRAM offer, if MD5 is among its methods:
/// `CRAM-MD5-<hex>`, or a list, `CRAM-SHA1/MD5-<hex>` (FTS-1027).
fn cram_offer(opt: &str) -> Option<&str> {
    let (methods, challenge) = opt.strip_prefix("CRAM-")?.rsplit_once('-')?;
    methods.split('/').any(|m| m.eq_ignore_ascii_case("MD5")).then_some(challenge)
}

/// A received file's name, made safe to write: binkp's `\xHH` escapes
/// undone, then at most 64 characters (keeping the extension), never one
/// ending `.part`.
fn safe_name(name: &str) -> String {
    let b = name.as_bytes();
    let mut raw = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1..i + 4).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| h.strip_prefix('x')).and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'\\', Some(c)) => {
                raw.push(c);
                i += 4;
            }
            (c, _) => {
                raw.push(c);
                i += 1;
            }
        }
    }
    let mut n: String = raw.iter().map(|&c| c as char).filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
    if n.is_empty() || n.starts_with('.') {
        n.insert(0, 'x');
    }
    if n.len() > 64 {
        let ext = n.rsplit_once('.').map(|(_, e)| e).filter(|e| e.len() <= 8).unwrap_or("").to_string();
        n.truncate(63 - ext.len());
        n.push('.');
        n.push_str(&ext);
    }
    if n.ends_with(".part") {
        n.push('_');
    }
    n
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// A connection's two halves: frames to send go through `send` (one
/// writer, so they go out whole and in order while we read).
struct Link {
    rd: tokio::net::tcp::OwnedReadHalf,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    writer: tokio::task::JoinHandle<std::io::Result<()>>,
    /// When a frame last went out (`now_ms`).
    wrote: Arc<AtomicU64>,
}

impl Link {
    fn new(stream: TcpStream) -> anyhow::Result<Link> {
        stream.set_nodelay(true)?;
        let (rd, mut wr) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let wrote = Arc::new(AtomicU64::new(now_ms()));
        let w = wrote.clone();
        let writer = tokio::spawn(async move {
            while let Some(f) = rx.recv().await {
                wr.write_all(&f).await?;
                w.store(now_ms(), Ordering::Relaxed);
            }
            wr.shutdown().await
        });
        Ok(Link { rd, tx, writer, wrote })
    }

    async fn read(&mut self) -> anyhow::Result<Frame> {
        read_frame(&mut self.rd, &self.wrote).await
    }

    fn send(&self, f: Vec<u8>) -> anyhow::Result<()> {
        self.tx.send(f).map_err(|_| anyhow!("binkp: connection closed"))
    }

    /// Send what's queued, then close; a minute at most, if the other
    /// side has stopped reading.
    async fn close(mut self) {
        drop(self.tx);
        if tokio::time::timeout(Duration::from_secs(60), &mut self.writer).await.is_err() {
            self.writer.abort();
        }
    }

    /// Close without sending what's left (the session failed).
    fn abort(self) {
        self.writer.abort();
    }

    /// Close cleanly after a good session, at once after a failed one.
    async fn end<T>(self, r: anyhow::Result<T>) -> anyhow::Result<T> {
        if r.is_ok() { self.close().await } else { self.abort() }
        r
    }

    /// Who we are: M_NUL lines (and `extra`, e.g. a CRAM challenge), then
    /// M_ADR with our address(es) `me`.
    fn greet(&self, state: &BbsState, me: &str, extra: Option<String>) -> anyhow::Result<()> {
        let cfg = state.config();
        for nul in [
            format!("SYS {}", cfg.bbs.name),
            format!("ZYZ {}", cfg.bbs.sysop_alias),
            format!("LOC {}", cfg.bbs.location),
            "NDL 115200,TCP,BINKP".to_string(),
            format!("TIME {}", crate::clock::now().to_rfc2822()),
            format!("VER fluxbbs/{} binkp/1.0", env!("CARGO_PKG_VERSION")),
        ]
        .into_iter()
        .chain(extra)
        {
            self.send(cmd(M_NUL, &nul))?;
        }
        self.send(cmd(M_ADR, me))
    }
}

/// Is `s` (an M_ADR's addresses) the hub?
fn is_hub(s: &str, hub: Addr) -> bool {
    s.split_whitespace().filter_map(Addr::parse).any(|a| a.zone == hub.zone && a.net == hub.net && a.node == hub.node)
}

/// `host`, `host:port`, `v6` or `[v6]:port`, with binkp's port if none.
fn with_port(host: &str) -> String {
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]:24554")
    } else if host.starts_with('[') && !host.contains("]:") {
        format!("{host}:24554")
    } else if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:24554")
    }
}

/// A network's session lock: one session with its hub at a time, a poll
/// or a call in (two at once would send the same packets).
fn session_lock(state: &BbsState, net: &str, wait: bool) -> anyhow::Result<Option<std::fs::File>> {
    crate::ftntoss::lock(state, &format!("binkp-{}", net.to_ascii_lowercase()), wait)
}

/// Poll a network's hub once. Returns what was sent and received.
pub async fn poll(state: Arc<BbsState>, net: FtnNetwork) -> anyhow::Result<Polled> {
    let (_, hub) = crate::ftntoss::addresses(&net)?;
    let host = with_port(net.hub_host.trim());
    // A session with this hub under way (it called): this poll's not
    // needed, and waiting would hold up the other networks' polls.
    let Some(_busy) = session_lock(&state, &net.name, false)? else {
        tracing::info!("binkp: a session with {hub} ({}) is under way; not polling", net.name);
        return Ok(Polled::default());
    };
    let stream = tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(&host)).await.with_context(|| format!("binkp: connecting to {host}"))??;
    let mut link = Link::new(stream)?;
    link.greet(&state, net.address.trim(), None)?;

    // The hub says who it is (and may offer CRAM-MD5); then our password.
    let mut challenge = None;
    loop {
        match link.read().await? {
            Frame::Cmd(M_NUL, s) => {
                if let Some(c) = s.strip_prefix("OPT ").and_then(|o| o.split_whitespace().find_map(cram_offer)) {
                    challenge = Some(c.to_string());
                }
                tracing::debug!("binkp: {s}");
            }
            Frame::Cmd(M_ADR, s) => {
                if !is_hub(&s, hub) {
                    bail!("binkp: {host} is {s}, not {hub}");
                }
                break;
            }
            Frame::Cmd(M_ERR | M_BSY, s) => bail!("binkp: hub said {s}"),
            _ => {}
        }
    }
    let pw = &net.session_password;
    let answer = match &challenge {
        Some(c) if !pw.is_empty() => cram_md5(pw, c).ok_or_else(|| anyhow!("binkp: bad CRAM challenge"))?,
        _ if pw.is_empty() => "-".to_string(),
        _ => pw.clone(),
    };
    link.send(cmd(M_PWD, &answer))?;
    loop {
        match link.read().await? {
            Frame::Cmd(M_OK, s) => {
                // The hub doesn't know our password: our mail would land
                // in its unsecure inbound and likely never be tossed, so
                // none is sent.
                if !pw.is_empty() && s.to_ascii_lowercase().contains("non-secure") {
                    let _ = link.send(cmd(M_ERR, "we have a session password for you"));
                    link.close().await;
                    bail!("binkp: {hub} took us as non-secure ({s}); is the session password set up there?");
                }
                break;
            }
            Frame::Cmd(M_ERR | M_BSY, s) => bail!("binkp: hub refused us: {s}"),
            _ => {}
        }
    }
    let polled = exchange(&mut link, &crate::ftntoss::outbound_dirs(&state, &net.name), &crate::ftntoss::inbound_dir(&state, &net.name)).await;
    let polled = link.end(polled).await?;
    tracing::info!("binkp: poll of {hub} ({}): sent {:?}, received {:?}", net.name, polled.sent, polled.received);
    Ok(polled)
}

/// A session the hub opens (new): it calls us when mail arrives for us
/// ("crash"). Only the hub, with the session password (CRAM-MD5 offered,
/// or plain), gets in; then files go both ways as in a poll, and what came
/// is tossed. `peer` is who connected.
async fn answer(state: Arc<BbsState>, stream: TcpStream, peer: std::net::SocketAddr) -> anyhow::Result<()> {
    // Every network's hub may call: we're all our addresses, and the
    // caller's say which network(s) it's on (one system can be the hub of
    // several).
    let nets: Vec<(FtnNetwork, Addr)> =
        state.config().ftn.networks().into_iter().filter_map(|n| crate::ftntoss::addresses(&n).ok().map(|(_, hub)| (n, hub))).collect();
    let mine: Vec<String> = nets.iter().map(|(n, _)| n.address.trim().to_string()).collect();
    let mut link = Link::new(stream)?;
    let challenge: String = (0..16).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    link.greet(&state, &mine.join(" "), Some(format!("OPT CRAM-MD5-{challenge}")))?;
    let mut claimed: Vec<(FtnNetwork, Addr)> = Vec::new();
    let mut claims_any_hub = false;
    // A minute to say who they are.
    let hello = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            match link.read().await? {
                Frame::Cmd(M_NUL, s) => tracing::debug!("binkp in: {s}"),
                Frame::Cmd(M_ADR, s) => {
                    claimed = nets.iter().filter(|(_, hub)| is_hub(&s, *hub)).cloned().collect();
                    claims_any_hub = state.config().ftn.all_hubs().iter().filter_map(|h| Addr::parse(h)).any(|h| is_hub(&s, h));
                }
                Frame::Cmd(M_PWD, p) => break Ok(p),
                Frame::Cmd(M_ERR | M_BSY, s) => break Err(anyhow!("binkp: {peer} said {s}")),
                _ => {}
            }
        }
    })
    .await;
    let pwd = match hello {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            link.abort();
            return Err(e);
        }
        Err(_) => {
            link.abort();
            bail!("binkp: {peer} never gave a password");
        }
    };
    let remote_is_hub = !claimed.is_empty();
    // The digest may have options after it (FTS-1027).
    let given = pwd.split_whitespace().next().unwrap_or("");
    // Signed in as each network whose hub it claims and whose password it
    // gave: never as one whose password it didn't.
    let authed: Vec<(FtnNetwork, Addr)> = claimed
        .into_iter()
        .filter(|(n, _)| {
            let pw = &n.session_password;
            !pw.is_empty() && (cram_md5(pw, &challenge).is_some_and(|c| c.eq_ignore_ascii_case(given)) || given == pw)
        })
        .collect();
    if authed.is_empty() {
        let why = if remote_is_hub { "Incorrect password" } else { "Unknown address" };
        link.send(cmd(M_ERR, why))?;
        link.close().await;
        if remote_is_hub {
            // Most likely the hub's copy of the password changed: banning
            // it (it calls back every few seconds) would cut the board off
            // from every service. Guessing is held back by the connection
            // limits, and CRAM never shows the password.
            tracing::warn!("binkp: {peer} gave the hub's address with the wrong session password");
        } else if claims_any_hub {
            // The hub of a network that's set up wrong here: not their fault.
            for p in state.config().ftn.problems() {
                tracing::warn!("ftn: {p}");
            }
            tracing::warn!("binkp: {peer} is the hub of a network that isn't set up right (above); turned away");
        } else {
            crate::logon::record_failure(&state, peer, "binkp: unknown address");
        }
        return Ok(());
    }
    // A poll of one of them is already under way: try again later.
    let mut busy = Vec::new();
    for (n, _) in &authed {
        match session_lock(&state, &n.name, false)? {
            Some(l) => busy.push(l),
            None => {
                link.send(cmd(M_BSY, "busy, try again later"))?;
                link.close().await;
                return Ok(());
            }
        }
    }
    link.send(cmd(M_OK, "secure"))?;
    let outbound: Vec<PathBuf> = authed.iter().flat_map(|(n, _)| crate::ftntoss::outbound_dirs(&state, &n.name)).collect();
    // What it sends is tossed as mail of these networks only (they share
    // the password it gave: the first one's inbound stands for them all).
    let inbound = crate::ftntoss::inbound_dir(&state, &authed[0].0.name);
    let polled = exchange(&mut link, &outbound, &inbound).await;
    let polled = link.end(polled).await;
    drop(busy);
    // What did arrive is tossed even if the session ended badly.
    let s = state.clone();
    let tossed = tokio::task::spawn_blocking(move || crate::ftntoss::toss(&s)).await?;
    let polled = polled?;
    tossed?;
    let names: Vec<&str> = authed.iter().map(|(n, _)| n.name.as_str()).collect();
    tracing::info!("binkp: {} ({}) called: sent {:?}, received {:?}", authed[0].1, names.join(", "), polled.sent, polled.received);
    Ok(())
}

/// Listen for the hub's calls on `[ftn] listen`, if set.
pub async fn listen(state: Arc<BbsState>) {
    let addr = state.config().ftn.listen.trim().to_string();
    if addr.is_empty() {
        return;
    }
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => return tracing::error!("binkp: can't listen on {addr}: {e}"),
    };
    tracing::info!("binkp listening on {addr}");
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(c) => c,
            // Out of file handles, say: wait rather than spin.
            Err(e) => {
                tracing::warn!("binkp: accept: {e}");
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
        };
        let cfg = state.config();
        if !cfg.ftn.enabled {
            continue;
        }
        let ticket = match state.guard.admit(&cfg.security, peer.ip()) {
            Ok(t) => t,
            Err(why) => {
                tracing::info!("{peer}: binkp refused: {why}");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            let _ticket = ticket;
            if let Err(e) = answer(state, stream, peer).await {
                tracing::info!("{peer}: {e:#}");
            }
        });
    }
}

/// Files both ways once the session is up: ours, then end-of-batch; GOTs
/// for theirs go out as they finish. Done when both sides are, and every
/// file we sent is acknowledged.
/// `outbound`: where the hub's packets wait; `inbound`: where what it
/// sends goes, which no other session uses (it's the network's, and its
/// lock is held).
async fn exchange(link: &mut Link, outbound: &[PathBuf], inbound: &Path) -> anyhow::Result<Polled> {
    let inbound = inbound.to_path_buf();
    std::fs::create_dir_all(&inbound)?;
    // Halves of files from sessions that broke off (none in use: see
    // above).
    for e in std::fs::read_dir(&inbound)?.flatten() {
        if e.file_name().to_string_lossy().ends_with(".part") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    // M_GETs answered per file: a peer can't have a file sent over and over.
    let mut resent: std::collections::HashMap<String, u32> = Default::default();
    let mut waiting: Vec<(String, PathBuf)> = Vec::new();
    for p in outbound.iter().flat_map(|d| crate::ftntoss::outbound_files(d)) {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // GOT, SKIP and GET name a file: two of one name (two networks'
        // outbounds) go in separate sessions.
        if waiting.iter().any(|(n, _)| n == &name) {
            continue;
        }
        send_file(link, &name, &p, 0)?;
        waiting.push((name, p));
    }
    link.send(cmd(M_EOB, ""))?;

    let mut polled = Polled::default();
    let mut their_eob = false;
    // The file coming in: its name, size, time, what's arrived, and where.
    let mut incoming: Option<(String, u64, String, u64, std::fs::File, PathBuf)> = None;
    while !(their_eob && waiting.is_empty() && incoming.is_none()) {
        match link.read().await? {
            Frame::Cmd(M_FILE, s) => {
                let w: Vec<&str> = s.split_whitespace().collect();
                let [name, given, time, rest @ ..] = w.as_slice() else { bail!("binkp: bad M_FILE {s}") };
                incoming = None;
                // A size that won't parse (too big for us) is skipped.
                let size: u64 = given.parse().unwrap_or(u64::MAX);
                if size > MAX_FILE {
                    tracing::warn!("binkp: {name} is {given} bytes; skipped");
                    link.send(cmd(M_SKIP, &format!("{name} {given} {time}")))?;
                    continue;
                }
                // We keep no halves to resume: from the start, please
                // (also the answer to -1, "where from?").
                if rest.first().is_some_and(|o| *o != "0") {
                    link.send(cmd(M_GET, &format!("{name} {size} {time} 0")))?;
                    continue;
                }
                let part = inbound.join(format!("{}.part", safe_name(name)));
                let f = std::fs::File::create(&part)?;
                incoming = Some((name.to_string(), size, time.to_string(), 0, f, part));
                if size == 0 {
                    finish(&mut incoming, &inbound, link, &mut polled)?;
                }
            }
            Frame::Data(d) => {
                if let Some(f) = incoming.as_mut() {
                    // Nothing past the size it gave.
                    let take = (f.1 - f.3).min(d.len() as u64) as usize;
                    std::io::Write::write_all(&mut f.4, &d[..take])?;
                    f.3 += take as u64;
                    if f.3 >= f.1 {
                        finish(&mut incoming, &inbound, link, &mut polled)?;
                    }
                }
            }
            Frame::Cmd(M_GOT, s) => {
                let name = s.split_whitespace().next().unwrap_or("");
                if let Some(i) = waiting.iter().position(|(n, _)| n == name) {
                    let (n, p) = waiting.remove(i);
                    std::fs::remove_file(&p)?;
                    polled.sent.push(n);
                }
            }
            // Left for the next session.
            Frame::Cmd(M_SKIP, s) => {
                let name = s.split_whitespace().next().unwrap_or("");
                waiting.retain(|(n, _)| n != name);
            }
            Frame::Cmd(M_EOB, _) => their_eob = true,
            Frame::Cmd(M_ERR | M_BSY, s) => bail!("binkp: other side said {s}"),
            // The other side has part of it (from a session that broke
            // off) and wants the rest: from the offset it gives. What we
            // sent before this is thrown away there.
            Frame::Cmd(M_GET, s) => {
                let w: Vec<&str> = s.split_whitespace().collect();
                if let [name, _, _, offset, ..] = w.as_slice()
                    && let Some((n, p)) = waiting.iter().find(|(n, _)| n == name)
                {
                    let times = resent.entry(n.clone()).or_default();
                    *times += 1;
                    if *times > 3 {
                        bail!("binkp: {name} asked for too many times");
                    }
                    send_file(link, n, p, offset.parse().unwrap_or(0))?;
                }
            }
            Frame::Cmd(_, _) => {}
        }
    }
    Ok(polled)
}

/// Send a file from `offset` on.
fn send_file(link: &Link, name: &str, p: &Path, offset: u64) -> anyhow::Result<()> {
    let body = std::fs::read(p)?;
    let offset = (offset as usize).min(body.len());
    link.send(cmd(M_FILE, &format!("{name} {} {} {offset}", body.len(), mtime(p))))?;
    for chunk in body[offset..].chunks(BLOCK) {
        link.send(data(chunk))?;
    }
    Ok(())
}

/// A received file is all here: name it and acknowledge it. A name that's
/// taken gets a number before its extension, so `.pkt` stays `.pkt`.
fn finish(incoming: &mut Option<(String, u64, String, u64, std::fs::File, PathBuf)>, inbound: &Path, link: &Link, polled: &mut Polled) -> anyhow::Result<()> {
    let Some((name, size, time, _, f, part)) = incoming.take() else { return Ok(()) };
    f.sync_all()?;
    drop(f);
    let dest = crate::ftntoss::free_name(inbound, &safe_name(&name));
    std::fs::rename(&part, &dest)?;
    link.send(cmd(M_GOT, &format!("{name} {size} {time}")))?;
    polled.received.push(name);
    Ok(())
}

/// Scan, poll and toss now and then, as `[ftn] poll_minutes` says.
pub async fn run(state: Arc<BbsState>) {
    loop {
        let (enabled, minutes) = {
            let c = state.config();
            (c.ftn.enabled, c.ftn.poll_minutes)
        };
        if enabled && minutes > 0 {
            if let Err(e) = cycle(state.clone(), None).await {
                tracing::warn!("ftn: {e:#}");
            }
            tokio::time::sleep(Duration::from_secs(minutes as u64 * 60)).await;
        } else {
            // Settings can change; look again in a minute.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

/// Pack what's new, poll each network's hub (or just `only`), toss what
/// came.
pub async fn cycle(state: Arc<BbsState>, only: Option<&str>) -> anyhow::Result<()> {
    for p in state.config().ftn.problems() {
        tracing::warn!("ftn: {p}");
    }
    let (_, outbound, _) = crate::ftntoss::dirs(&state);
    for d in crate::ftntoss::stray_dirs(&state, &outbound) {
        if !crate::ftntoss::outbound_files(&d).is_empty() {
            tracing::warn!("ftn: packets in {} are for no network set up (renamed?); move them to the network's outbound", d.display());
        }
    }
    // Nothing packing (the database busy, say) doesn't stop mail coming.
    let mut failed = Vec::new();
    let s = state.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || crate::ftntoss::scan(&s)).await? {
        failed.push(format!("scan: {e:#}"));
    }
    let nets = match only {
        Some(name) => vec![crate::ftntoss::network(&state, name)?],
        // A hub with nowhere to call only calls us.
        None => state.config().ftn.networks().into_iter().filter(|n| !n.hub_host.trim().is_empty()).collect(),
    };
    // One hub down doesn't hold up the others.
    for net in nets {
        let name = net.name.clone();
        if let Err(e) = poll(state.clone(), net).await {
            failed.push(format!("{name}: {e:#}"));
        }
    }
    // Toss whatever arrived, even if a session broke off.
    let s = state.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || crate::ftntoss::toss(&s)).await? {
        failed.push(format!("toss: {e:#}"));
    }
    if failed.is_empty() { Ok(()) } else { Err(anyhow!(failed.join("; "))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_command_frames_are_dropped() {
        let mut bytes: &[u8] = &[0x80, 0x00, 0x80, 0x01, M_EOB];
        assert!(matches!(read_frame(&mut bytes, &AtomicU64::new(now_ms())).await.unwrap(), Frame::Cmd(M_EOB, _)));
    }

    #[test]
    fn frames_and_cram() {
        assert_eq!(cmd(M_OK, "secure"), [0x80, 7, 4, b's', b'e', b'c', b'u', b'r', b'e']);
        assert_eq!(data(&[1, 2, 3]), [0, 3, 1, 2, 3]);
        // RFC 2104 test vector 2: key "Jefe", data "what do ya want for nothing?".
        let challenge: String = b"what do ya want for nothing?".iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(cram_md5("Jefe", &challenge).unwrap(), "CRAM-MD5-750c783e6ab0b503eaa86e310a5db738");
        assert_eq!(safe_name("../../etc/passwd"), "x....etcpasswd");
        assert_eq!(safe_name("0000abcd.su0"), "0000abcd.su0");
        assert_eq!(safe_name("evil.part"), "evil.part_");
        let long = safe_name(&format!("{}.pkt", "a".repeat(300)));
        assert!(long.len() <= 64 && long.ends_with(".pkt"));
        // A garbled challenge is refused, never a panic.
        assert_eq!(cram_md5("pw", "a\u{fffd}b"), None);
        assert_eq!(cram_md5("pw", "abc"), None);
        assert_eq!(cram_md5("pw", "zz"), None);
        assert_eq!(cram_offer("CRAM-SHA1/MD5-abcd"), Some("abcd"));
        assert_eq!(cram_offer("CRAM-MD5-abcd"), Some("abcd"));
        assert_eq!(cram_offer("CRAM-SHA1-abcd"), None);
        assert_eq!(safe_name("odd\\x41name.pkt"), "oddAname.pkt");
        assert_eq!(with_port("hub.example"), "hub.example:24554");
        assert_eq!(with_port("hub.example:24555"), "hub.example:24555");
        assert_eq!(with_port("2001:db8::1"), "[2001:db8::1]:24554");
        assert_eq!(with_port("[2001:db8::1]:24555"), "[2001:db8::1]:24555");
    }

    /// The other side asks for the rest of a file (M_GET, as after a
    /// session that broke off): it's sent again from that offset.
    #[tokio::test]
    async fn resumes_from_m_get() {
        let dir = std::env::temp_dir().join(format!("inq-binkp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = crate::tests::state();
        {
            let mut cfg = (*state.config()).clone();
            cfg.root = dir.clone();
            cfg.paths.data = "data".into();
            *state.config.write().unwrap() = Arc::new(cfg);
        }
        let (_, outbound, _) = crate::ftntoss::dirs(&state);
        std::fs::create_dir_all(&outbound).unwrap();
        let body: Vec<u8> = (0..40_000u32).map(|i| i as u8).collect();
        std::fs::write(outbound.join("0000abcd.pkt"), &body).unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let want = |f: Frame| match f {
                Frame::Cmd(M_FILE, a) => a,
                _ => panic!("expected M_FILE"),
            };
            let first = want(read_frame(&mut s, &AtomicU64::new(now_ms())).await.unwrap());
            let w: Vec<String> = first.split_whitespace().map(String::from).collect();
            s.write_all(&cmd(M_GET, &format!("{} {} {} 30000", w[0], w[1], w[2]))).await.unwrap();
            // Skip to the M_FILE that answers, then take its data.
            let again = loop {
                if let Frame::Cmd(M_FILE, a) = read_frame(&mut s, &AtomicU64::new(now_ms())).await.unwrap() {
                    break a;
                }
            };
            let mut got = Vec::new();
            while got.len() < 10_000 {
                if let Frame::Data(d) = read_frame(&mut s, &AtomicU64::new(now_ms())).await.unwrap() {
                    got.extend(d);
                }
            }
            s.write_all(&cmd(M_GOT, &format!("{} {} {}", w[0], w[1], w[2]))).await.unwrap();
            s.write_all(&cmd(M_EOB, "")).await.unwrap();
            (again, got)
        });
        let mut link = Link::new(TcpStream::connect(addr).await.unwrap()).unwrap();
        let polled = exchange(&mut link, std::slice::from_ref(&outbound), &crate::ftntoss::inbound_dir(&state, "test")).await.unwrap();
        link.close().await;
        let (again, got) = peer.await.unwrap();
        assert!(again.starts_with("0000abcd.pkt 40000 ") && again.ends_with(" 30000"), "{again}");
        assert_eq!(got, body[30000..]);
        assert_eq!(polled.sent, ["0000abcd.pkt"]);
        assert!(!outbound.join("0000abcd.pkt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A hub calling in is matched to its network by its address: it gets
    /// that network's password checked and only that network's mail.
    #[tokio::test]
    async fn a_call_in_is_on_the_callers_network() {
        let dir = std::env::temp_dir().join(format!("inq-binkp-nets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = crate::tests::state();
        {
            let mut cfg = (*state.config()).clone();
            cfg.root = dir.clone();
            cfg.paths.data = "data".into();
            cfg.ftn.enabled = true;
            cfg.ftn.address = "21:1/999@fsxnet".into();
            cfg.ftn.hub = "21:1/100".into();
            cfg.ftn.session_password = "FSXPW".into();
            cfg.ftn.networks.push(FtnNetwork { name: "agoranet".into(), address: "46:1/200@agoranet".into(), hub: "46:1/100".into(), session_password: "AGNPW".into(), ..Default::default() });
            *state.config.write().unwrap() = Arc::new(cfg);
        }
        for (net, file) in [("fsxnet", "00000001.pkt"), ("agoranet", "00000002.pkt")] {
            let out = crate::ftntoss::outbound_dir(&state, net);
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join(file), b"packet").unwrap();
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let st = state.clone();
        let board = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            answer(st, stream, peer).await.unwrap();
        });
        let mut s = TcpStream::connect(addr).await.unwrap();
        let clock = AtomicU64::new(now_ms());
        let ours = loop {
            if let Frame::Cmd(M_ADR, a) = read_frame(&mut s, &clock).await.unwrap() {
                break a;
            }
        };
        assert_eq!(ours, "21:1/999@fsxnet 46:1/200@agoranet");
        s.write_all(&cmd(M_ADR, "46:1/100@agoranet")).await.unwrap();
        s.write_all(&cmd(M_PWD, "AGNPW")).await.unwrap();
        let mut files = Vec::new();
        loop {
            match read_frame(&mut s, &clock).await.unwrap() {
                Frame::Cmd(M_FILE, a) => {
                    let w: Vec<String> = a.split_whitespace().map(String::from).collect();
                    s.write_all(&cmd(M_GOT, &format!("{} {} {}", w[0], w[1], w[2]))).await.unwrap();
                    files.push(w[0].clone());
                }
                Frame::Cmd(M_EOB, _) => break,
                Frame::Cmd(M_ERR, e) => panic!("refused: {e}"),
                _ => {}
            }
        }
        s.write_all(&cmd(M_EOB, "")).await.unwrap();
        board.await.unwrap();
        assert_eq!(files, ["00000002.pkt"], "only agoranet's mail");
        assert!(crate::ftntoss::outbound_dir(&state, "fsxnet").join("00000001.pkt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
