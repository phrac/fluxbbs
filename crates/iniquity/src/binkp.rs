//! New: a binkp/1.0 mailer (FTS-1026), polling the FTN hub: say who we
//! are, give the session password (CRAM-MD5, FTS-1027, if the hub offers
//! it), send the packets in `data/ftn/outbound` and take what the hub has
//! into `data/ftn/inbound`, each file acknowledged (M_GOT) before it's
//! forgotten. Then the tosser runs (`ftntoss.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::BbsState;
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
/// Quiet this long and the session is given up.
const IDLE: Duration = Duration::from_secs(120);

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

async fn read_frame(rd: &mut (impl AsyncReadExt + Unpin)) -> anyhow::Result<Frame> {
    let mut h = [0u8; 2];
    tokio::time::timeout(IDLE, rd.read_exact(&mut h)).await.context("binkp: hub went quiet")??;
    let len = (((h[0] & 0x7F) as usize) << 8) | h[1] as usize;
    let mut body = vec![0u8; len];
    tokio::time::timeout(IDLE, rd.read_exact(&mut body)).await.context("binkp: hub went quiet")??;
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
    let challenge: Vec<u8> = (0..challenge_hex.len() / 2).map(|i| u8::from_str_radix(&challenge_hex[i * 2..i * 2 + 2], 16).ok()).collect::<Option<_>>()?;
    let mut mac = Hmac::<md5::Md5>::new_from_slice(password.as_bytes()).ok()?;
    mac.update(&challenge);
    Some(format!("CRAM-MD5-{}", mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// A received file's name, made safe to write.
fn safe_name(name: &str) -> String {
    let n: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
    if n.is_empty() || n.starts_with('.') { format!("x{n}") } else { n }
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// One binkp session at a time, a poll or a call in: two at once would
/// send the same packets.
static SESSION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A connection's two halves: frames to send go through `send` (one
/// writer, so they go out whole and in order while we read).
struct Link {
    rd: tokio::net::tcp::OwnedReadHalf,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    writer: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Link {
    fn new(stream: TcpStream) -> anyhow::Result<Link> {
        stream.set_nodelay(true)?;
        let (rd, mut wr) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let writer = tokio::spawn(async move {
            while let Some(f) = rx.recv().await {
                wr.write_all(&f).await?;
            }
            wr.shutdown().await
        });
        Ok(Link { rd, tx, writer })
    }

    fn send(&self, f: Vec<u8>) -> anyhow::Result<()> {
        self.tx.send(f).map_err(|_| anyhow!("binkp: connection closed"))
    }

    /// Send what's queued, then close.
    async fn close(self) {
        drop(self.tx);
        let _ = self.writer.await;
    }

    /// Who we are: M_NUL lines (and `extra`, e.g. a CRAM challenge), then
    /// M_ADR.
    fn greet(&self, state: &BbsState, extra: Option<String>) -> anyhow::Result<()> {
        let cfg = state.config();
        let me = cfg.ftn.address.trim().to_string();
        for nul in [
            format!("SYS {}", cfg.bbs.name),
            format!("ZYZ {}", cfg.bbs.sysop_alias),
            format!("LOC {}", cfg.bbs.location),
            "NDL 115200,TCP,BINKP".to_string(),
            format!("TIME {}", chrono::Local::now().to_rfc2822()),
            format!("VER Iniquity/{} binkp/1.0", env!("CARGO_PKG_VERSION")),
        ]
        .into_iter()
        .chain(extra)
        {
            self.send(cmd(M_NUL, &nul))?;
        }
        self.send(cmd(M_ADR, &me))
    }
}

/// Is `s` (an M_ADR's addresses) the hub?
fn is_hub(s: &str, hub: Addr) -> bool {
    s.split_whitespace().filter_map(Addr::parse).any(|a| a.zone == hub.zone && a.net == hub.net && a.node == hub.node)
}

/// Poll the hub once. Returns what was sent and received.
pub async fn poll(state: Arc<BbsState>) -> anyhow::Result<Polled> {
    let ftn = state.config().ftn.clone();
    let (_, hub) = crate::ftntoss::addresses(&ftn)?;
    let host = if ftn.hub_host.contains(':') { ftn.hub_host.clone() } else { format!("{}:24554", ftn.hub_host) };
    let _busy = SESSION.lock().await;
    let stream = tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(&host)).await.with_context(|| format!("binkp: connecting to {host}"))??;
    let mut link = Link::new(stream)?;
    link.greet(&state, None)?;

    // The hub says who it is (and may offer CRAM-MD5); then our password.
    let mut challenge = None;
    loop {
        match read_frame(&mut link.rd).await? {
            Frame::Cmd(M_NUL, s) => {
                if let Some(opt) = s.strip_prefix("OPT ") {
                    challenge = opt.split_whitespace().find_map(|o| o.strip_prefix("CRAM-MD5-")).map(String::from);
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
    let pw = &ftn.session_password;
    let answer = match &challenge {
        Some(c) if !pw.is_empty() => cram_md5(pw, c).ok_or_else(|| anyhow!("binkp: bad CRAM challenge"))?,
        _ if pw.is_empty() => "-".to_string(),
        _ => pw.clone(),
    };
    link.send(cmd(M_PWD, &answer))?;
    loop {
        match read_frame(&mut link.rd).await? {
            Frame::Cmd(M_OK, _) => break,
            Frame::Cmd(M_ERR | M_BSY, s) => bail!("binkp: hub refused us: {s}"),
            _ => {}
        }
    }
    let polled = exchange(&state, &mut link).await;
    link.close().await;
    let polled = polled?;
    tracing::info!("binkp: poll of {hub}: sent {:?}, received {:?}", polled.sent, polled.received);
    Ok(polled)
}

/// A session the hub opens (new): it calls us when mail arrives for us
/// ("crash"). Only the hub, with the session password (CRAM-MD5 offered,
/// or plain), gets in; then files go both ways as in a poll, and what came
/// is tossed. `peer` is who connected.
async fn answer(state: Arc<BbsState>, stream: TcpStream, peer: std::net::SocketAddr) -> anyhow::Result<()> {
    let ftn = state.config().ftn.clone();
    let (_, hub) = crate::ftntoss::addresses(&ftn)?;
    let mut link = Link::new(stream)?;
    let challenge: String = (0..16).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    link.greet(&state, Some(format!("OPT CRAM-MD5-{challenge}")))?;
    let mut remote_is_hub = false;
    let pwd = loop {
        match read_frame(&mut link.rd).await? {
            Frame::Cmd(M_NUL, s) => tracing::debug!("binkp in: {s}"),
            Frame::Cmd(M_ADR, s) => remote_is_hub = is_hub(&s, hub),
            Frame::Cmd(M_PWD, p) => break p,
            Frame::Cmd(M_ERR, s) => bail!("binkp: {peer} said {s}"),
            _ => {}
        }
    };
    let pw = &ftn.session_password;
    let good = remote_is_hub && !pw.is_empty() && (cram_md5(pw, &challenge).is_some_and(|c| c.eq_ignore_ascii_case(&pwd)) || pwd == *pw);
    if !good {
        let why = if remote_is_hub { "Incorrect password" } else { "Unknown address" };
        link.send(cmd(M_ERR, why))?;
        link.close().await;
        crate::logon::record_failure(&state, peer, &format!("binkp: {}", why.to_ascii_lowercase()));
        return Ok(());
    }
    // A poll is already under way: try again later.
    let Ok(_busy) = SESSION.try_lock() else {
        link.send(cmd(M_BSY, "busy, try again later"))?;
        link.close().await;
        return Ok(());
    };
    link.send(cmd(M_OK, "secure"))?;
    let polled = exchange(&state, &mut link).await;
    link.close().await;
    let polled = polled?;
    tracing::info!("binkp: {hub} called: sent {:?}, received {:?}", polled.sent, polled.received);
    if !polled.received.is_empty() {
        let s = state.clone();
        tokio::task::spawn_blocking(move || crate::ftntoss::toss(&s)).await??;
    }
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
        let Ok((stream, peer)) = listener.accept().await else { continue };
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
async fn exchange(state: &BbsState, link: &mut Link) -> anyhow::Result<Polled> {
    let (inbound, outbound, _) = crate::ftntoss::dirs(state);
    std::fs::create_dir_all(&inbound)?;
    std::fs::create_dir_all(&outbound)?;
    let mut waiting: Vec<(String, PathBuf)> = Vec::new();
    for p in crate::ftntoss::outbound_files(&outbound) {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let body = std::fs::read(&p)?;
        link.send(cmd(M_FILE, &format!("{name} {} {} 0", body.len(), mtime(&p))))?;
        for chunk in body.chunks(BLOCK) {
            link.send(data(chunk))?;
        }
        waiting.push((name, p));
    }
    link.send(cmd(M_EOB, ""))?;

    let mut polled = Polled::default();
    let mut their_eob = false;
    // The file coming in: its name, size, time, what's arrived, and where.
    let mut incoming: Option<(String, u64, String, u64, std::fs::File, PathBuf)> = None;
    let send = |f: Vec<u8>| link.tx.send(f).map_err(|_| anyhow!("binkp: connection closed"));
    while !(their_eob && waiting.is_empty() && incoming.is_none()) {
        match read_frame(&mut link.rd).await? {
            Frame::Cmd(M_FILE, s) => {
                let w: Vec<&str> = s.split_whitespace().collect();
                let [name, size, time, ..] = w.as_slice() else { bail!("binkp: bad M_FILE {s}") };
                let size: u64 = size.parse().context("binkp: M_FILE size")?;
                let part = inbound.join(format!("{}.part", safe_name(name)));
                let f = std::fs::File::create(&part)?;
                incoming = Some((name.to_string(), size, time.to_string(), 0, f, part));
                if size == 0 {
                    finish(&mut incoming, &inbound, &send, &mut polled)?;
                }
            }
            Frame::Data(d) => {
                if let Some(f) = incoming.as_mut() {
                    std::io::Write::write_all(&mut f.4, &d)?;
                    f.3 += d.len() as u64;
                    if f.3 >= f.1 {
                        finish(&mut incoming, &inbound, &send, &mut polled)?;
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
            Frame::Cmd(M_GET, s) => tracing::info!("binkp: asked again for {s} (not supported; it'll come next session)"),
            Frame::Cmd(_, _) => {}
        }
    }
    Ok(polled)
}

/// A received file is all here: name it and acknowledge it.
fn finish(
    incoming: &mut Option<(String, u64, String, u64, std::fs::File, PathBuf)>,
    inbound: &Path,
    send: &impl Fn(Vec<u8>) -> anyhow::Result<()>,
    polled: &mut Polled,
) -> anyhow::Result<()> {
    let Some((name, size, time, _, f, part)) = incoming.take() else { return Ok(()) };
    f.sync_all()?;
    drop(f);
    let mut dest = inbound.join(safe_name(&name));
    let mut n = 1;
    while dest.exists() {
        dest = inbound.join(format!("{}.{n}", safe_name(&name)));
        n += 1;
    }
    std::fs::rename(&part, &dest)?;
    send(cmd(M_GOT, &format!("{name} {size} {time}")))?;
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
            if let Err(e) = cycle(state.clone()).await {
                tracing::warn!("ftn: {e:#}");
            }
            tokio::time::sleep(Duration::from_secs(minutes as u64 * 60)).await;
        } else {
            // Settings can change; look again in a minute.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

/// Pack what's new, poll the hub, toss what came.
pub async fn cycle(state: Arc<BbsState>) -> anyhow::Result<()> {
    let s = state.clone();
    tokio::task::spawn_blocking(move || crate::ftntoss::scan(&s)).await??;
    let polled = poll(state.clone()).await;
    // Toss whatever arrived, even if the session broke off.
    let s = state.clone();
    tokio::task::spawn_blocking(move || crate::ftntoss::toss(&s)).await??;
    polled.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_cram() {
        assert_eq!(cmd(M_OK, "secure"), [0x80, 7, 4, b's', b'e', b'c', b'u', b'r', b'e']);
        assert_eq!(data(&[1, 2, 3]), [0, 3, 1, 2, 3]);
        // RFC 2104 test vector 2: key "Jefe", data "what do ya want for nothing?".
        let challenge: String = b"what do ya want for nothing?".iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(cram_md5("Jefe", &challenge).unwrap(), "CRAM-MD5-750c783e6ab0b503eaa86e310a5db738");
        assert_eq!(safe_name("../../etc/passwd"), "x....etcpasswd");
        assert_eq!(safe_name("0000abcd.su0"), "0000abcd.su0");
    }
}
