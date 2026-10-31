//! New: MRC (Multi Relay Chat), live chat between BBSes. The board keeps
//! one connection to an MRC server (`[mrc]`), over TLS by default, and
//! every caller in chat (`XR`, `mrcchat.rs`) talks through it: packets
//! out are queued here, packets in are handed to every chatting session,
//! which keeps what's for it. Ported from uMRC (codefenix-dev/uMRC,
//! MIT), whose bridge does this job for its door, protocol 1.3.
//!
//! A packet is a line of seven fields and a closing `~`:
//! `fromUser~fromSite~fromRoom~toUser~msgExt~toRoom~body~`. `SERVER` as
//! `toUser` is a command; `NOTME` is everyone in the room but the
//! sender; `CLIENT` is every client. The board sends its name and
//! software first; the server says `HELLO`, and is told about the board
//! (`INFOWEB`... `IMALIVE`, `CAPABILITIES`); it says `ping` now and then,
//! and is answered `IMALIVE`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, anyhow};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc};

use crate::BbsState;

/// The protocol spoken.
pub const PROTOCOL: &str = "1.3";
/// The room CTCP requests go through.
pub const CTCP_ROOM: &str = "ctcp_echo_channel";
/// The most of a message one packet carries.
pub const BODY_MAX: usize = 140;
/// The longest line read from the server; past it, the rest of the line
/// is thrown away.
const LINE_MAX: usize = 4096;
/// Names the protocol keeps for itself.
const RESERVED: [&str; 4] = ["SERVER", "CLIENT", "NOTME", "ALL"];

/// One MRC packet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Packet {
    pub from_user: String,
    pub from_site: String,
    pub from_room: String,
    pub to_user: String,
    pub msg_ext: String,
    pub to_room: String,
    pub body: String,
}

/// A field as it can go in a packet: no `~`, no line breaks.
fn field(s: &str) -> String {
    s.chars().filter(|&c| c != '~' && c != '\r' && c != '\n').collect()
}

impl Packet {
    /// A line from the server; None if it hasn't the seven fields.
    pub fn parse(line: &str) -> Option<Packet> {
        let line = line.trim_end_matches(['\r', '\n']);
        let f: Vec<&str> = line.splitn(8, '~').collect();
        if f.len() < 7 {
            return None;
        }
        // Control characters the server sends become `_`, as uMRC does;
        // they'd drive the caller's terminal. So do CP437's glyphs for them
        // (`←` is ESC on the way out).
        let clean = |s: &str| s.chars().map(|c| if is_controlling(c) { '_' } else { c }).collect::<String>();
        Some(Packet {
            from_user: clean(f[0]),
            from_site: clean(f[1]),
            from_room: clean(f[2]),
            to_user: clean(f[3]),
            msg_ext: clean(f[4]),
            to_room: clean(f[5]),
            body: clean(f[6]),
        })
    }

    pub fn to_line(&self) -> String {
        format!(
            "{}~{}~{}~{}~{}~{}~{}~\n",
            field(&self.from_user),
            field(&self.from_site),
            field(&self.from_room),
            field(&self.to_user),
            field(&self.msg_ext),
            field(&self.to_room),
            field(&self.body)
        )
    }
}

/// Would this character drive a terminal: a control character, or one
/// CP437 sends as a control byte (its glyphs for them)?
pub fn is_controlling(c: char) -> bool {
    let b = fluxbbs_term::cp437::from_char(c);
    c.is_control() || (b < 0x20 && c != ' ') || b == 0x7F
}

/// A piece of MRC text: a character, or a pipe colour code (`|07`: the
/// number).
pub enum Piece {
    Char(char),
    Colour(u8),
}

/// MRC text as characters and colour codes.
pub fn pieces(s: &str) -> Vec<Piece> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        match (c[i], c.get(i + 1).and_then(|d| d.to_digit(10)), c.get(i + 2).and_then(|d| d.to_digit(10))) {
            ('|', Some(a), Some(b)) => {
                out.push(Piece::Colour((a * 10 + b) as u8));
                i += 3;
            }
            (ch, _, _) => {
                out.push(Piece::Char(ch));
                i += 1;
            }
        }
    }
    out
}

/// Pipe colour codes (`|07`) taken out.
pub fn strip_pipes(s: &str) -> String {
    pieces(s).into_iter().filter_map(|p| if let Piece::Char(c) = p { Some(c) } else { None }).collect()
}

/// A name as MRC takes it: no colours, no `~`, spaces as `_`, 30 at most.
pub fn wire_name(s: &str) -> String {
    let n: String = strip_pipes(s).chars().filter(|&c| c != '~' && !c.is_control()).map(|c| if c == ' ' { '_' } else { c }).take(30).collect();
    n.trim_matches('_').to_string()
}

/// A caller's chat name from their handle; None if it can't be one (it's
/// empty, or one of the names the protocol keeps).
pub fn nick_of(handle: &str) -> Option<String> {
    let n = wire_name(handle);
    (!n.is_empty() && !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&n))).then_some(n)
}

/// What the server says to the board as a whole, and the connection
/// coming and going, for every caller in chat.
#[derive(Debug, Clone)]
pub enum Event {
    Packet(Arc<Packet>),
    /// Connected (again): callers in chat say they're here and rejoin.
    Up,
    Down,
}

/// How MRC is, for the status bar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub up: bool,
    /// BBSes, rooms, users, and how busy (from the server's STATS).
    pub bbses: u32,
    pub rooms: u32,
    pub users: u32,
    pub activity: u32,
}

/// The board's end of MRC: what's going out, what's come in.
pub struct Hub {
    out: Mutex<Option<mpsc::UnboundedSender<String>>>,
    events: broadcast::Sender<Event>,
    status: Mutex<Status>,
}

impl Default for Hub {
    fn default() -> Self {
        Self { out: Mutex::new(None), events: broadcast::channel(1024).0, status: Mutex::new(Status::default()) }
    }
}

impl Hub {
    /// Send a packet; false if MRC isn't connected.
    pub fn send(&self, p: &Packet) -> bool {
        let out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        out.as_ref().is_some_and(|tx| tx.send(p.to_line()).is_ok())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_up(&self, tx: Option<mpsc::UnboundedSender<String>>) {
        let up = tx.is_some();
        *self.out.lock().unwrap_or_else(|e| e.into_inner()) = tx;
        self.status.lock().unwrap_or_else(|e| e.into_inner()).up = up;
        let _ = self.events.send(if up { Event::Up } else { Event::Down });
    }
}

/// The board's name on MRC, as sent at the start (colours kept) and in
/// packets (`fromSite`).
fn names(state: &BbsState) -> (String, String) {
    let cfg = state.config();
    let raw = if cfg.mrc.bbs_name.trim().is_empty() { cfg.bbs.name.clone() } else { cfg.mrc.bbs_name.trim().to_string() };
    (field(&raw), wire_name(&raw))
}

/// The site field of the board's packets.
pub fn site(state: &BbsState) -> String {
    names(state).1
}

/// Keep connected while `[mrc]` is on: again after a drop, waiting longer
/// each time (to five minutes).
pub async fn run(state: Arc<BbsState>) {
    let mut wait = 10;
    loop {
        if !state.config().mrc.enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let started = std::time::Instant::now();
        match connection(&state).await {
            Ok(()) => tracing::info!("mrc: the server closed the connection"),
            Err(e) => tracing::warn!("mrc: {e:#}"),
        }
        state.mrc.set_up(None);
        // A connection that lasted starts the waiting over.
        if started.elapsed() > Duration::from_secs(300) {
            wait = 10;
        }
        tokio::time::sleep(Duration::from_secs(wait)).await;
        wait = (wait * 2).min(300);
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

async fn connect(host: &str, port: u16, tls: bool) -> anyhow::Result<Box<dyn Stream>> {
    let tcp = tokio::time::timeout(Duration::from_secs(20), tokio::net::TcpStream::connect((host, port)))
        .await
        .map_err(|_| anyhow!("connecting to {host}:{port} timed out"))?
        .with_context(|| format!("connecting to {host}:{port}"))?;
    if !tls {
        return Ok(Box::new(tcp));
    }
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // ring, named: more than one provider is built in.
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| anyhow!("{host}: {e}"))?;
    let stream = tokio::time::timeout(Duration::from_secs(20), tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp))
        .await
        .map_err(|_| anyhow!("TLS with {host} timed out"))?
        .with_context(|| format!("TLS with {host}"))?;
    Ok(Box::new(stream))
}

/// A line from the server, `LINE_MAX` bytes at most (the rest of a longer
/// one is read and thrown away): UTF-8 if it is, else CP437, as MRC
/// mostly is. None at the end.
async fn read_line(rd: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> std::io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = (&mut *rd).take(LINE_MAX as u64).read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if !buf.ends_with(b"\n") && buf.len() >= LINE_MAX {
        // Too long: skip to its end, a bounded piece at a time.
        let mut skip = Vec::new();
        loop {
            skip.clear();
            let m = (&mut *rd).take(LINE_MAX as u64).read_until(b'\n', &mut skip).await?;
            if m == 0 || skip.ends_with(b"\n") {
                break;
            }
        }
        return Ok(Some(String::new()));
    }
    while buf.last().is_some_and(|&b| b == b'\n' || b == b'\r') {
        buf.pop();
    }
    Ok(Some(match String::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => fluxbbs_term::cp437::decode(e.as_bytes()),
    }))
}

/// Write a line as MRC takes it, CP437, in 30 seconds at most.
async fn send_line(wr: &mut (impl AsyncWrite + Unpin), line: &str) -> anyhow::Result<()> {
    let bytes = fluxbbs_term::cp437::encode(line);
    tokio::time::timeout(Duration::from_secs(30), async {
        wr.write_all(&bytes).await?;
        wr.flush().await
    })
    .await
    .map_err(|_| anyhow!("writing to the server timed out"))??;
    Ok(())
}

/// A command from the board itself (`CLIENT` to `SERVER`).
fn board_cmd(site: &str, from_room: &str, msg_ext: &str, body: String) -> Packet {
    Packet { from_user: "CLIENT".into(), from_site: site.into(), from_room: from_room.into(), to_user: "SERVER".into(), msg_ext: msg_ext.into(), to_room: String::new(), body }
}

async fn connection(state: &Arc<BbsState>) -> anyhow::Result<()> {
    let (host, port, tls) = {
        let c = &state.config().mrc;
        (c.host.trim().to_string(), c.port, c.tls)
    };
    let stream = connect(&host, port, tls).await?;
    let (rd, mut wr) = tokio::io::split(stream);
    let (name, site) = names(state);
    // `name~software/platform.arch/protocol.version`, with no line end,
    // as uMRC's bridge sends it.
    let hello = format!("{name}~fluxbbs/{}.{}/{PROTOCOL}.{}", platform(), std::env::consts::ARCH, env!("CARGO_PKG_VERSION"));
    send_line(&mut wr, &hello).await?;
    tracing::info!("mrc: connected to {host}:{port}{}", if tls { " (TLS)" } else { "" });
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mut rd = BufReader::new(rd);
    let pid = std::process::id().to_string();
    let mut announced = false;
    // The server pings every few minutes; ten without a word from it (what
    // the board sends doesn't count) is a dead connection.
    let mut heard = tokio::time::Instant::now();
    loop {
        let deadline = heard + Duration::from_secs(600);
        tokio::select! {
            line = read_line(&mut rd) => {
                let Some(line) = line? else { return Ok(()) };
                heard = tokio::time::Instant::now();
                let Some(p) = Packet::parse(&line) else { continue };
                if p.from_user != "SERVER" {
                    let _ = state.mrc.events.send(Event::Packet(Arc::new(p)));
                    continue;
                }
                let reply = |body: String, room: &str, ext: &str| board_cmd(&site, room, ext, body).to_line();
                let cfg = state.config().mrc.clone();
                match p.body.as_str() {
                    "HELLO" => {
                        for (k, v) in [("INFOWEB", &cfg.info_web), ("INFOTEL", &cfg.info_telnet), ("INFOSSH", &cfg.info_ssh), ("INFOSYS", &cfg.info_sysop), ("INFODSC", &cfg.info_desc)] {
                            send_line(&mut wr, &reply(format!("{k}:{}", field(v)), "", "")).await?;
                        }
                        send_line(&mut wr, &reply(format!("IMALIVE:{site}"), &pid, "")).await?;
                        let caps = format!("MCI{} CTCP GOODBYE USERROOM", if tls { " SSL" } else { "" });
                        send_line(&mut wr, &reply(format!("CAPABILITIES:{caps}"), "", "")).await?;
                        if !announced {
                            announced = true;
                            state.mrc.set_up(Some(tx.clone()));
                        }
                    }
                    b if b.eq_ignore_ascii_case("ping") => {
                        let now = chrono::Utc::now().timestamp_millis().to_string();
                        send_line(&mut wr, &reply(format!("IMALIVE:{site}"), &pid, &now)).await?;
                        send_line(&mut wr, &reply("STATS".into(), "", "")).await?;
                    }
                    "GOODBYE" => return Ok(()),
                    b => {
                        if let Some(stats) = b.strip_prefix("STATS:") {
                            let n: Vec<u32> = stats.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                            if n.len() >= 4 {
                                let mut s = state.mrc.status.lock().unwrap_or_else(|e| e.into_inner());
                                (s.bbses, s.rooms, s.users, s.activity) = (n[0], n[1], n[2], n[3]);
                            }
                        }
                        let _ = state.mrc.events.send(Event::Packet(Arc::new(p)));
                    }
                }
            }
            out = rx.recv() => {
                let Some(out) = out else { continue };
                send_line(&mut wr, &out).await?;
            }
            _ = tokio::time::sleep_until(deadline) => return Err(anyhow!("nothing from the server for ten minutes")),
        }
    }
}

fn platform() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "macOS",
        "freebsd" => "FreeBSD",
        "windows" => "Windows",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets() {
        let p = Packet::parse("phrac~disflux~lobby~~~lobby~|03<|11phrac|03> |07hello~\n").unwrap();
        assert_eq!((p.from_user.as_str(), p.from_site.as_str(), p.to_room.as_str(), p.body.as_str()), ("phrac", "disflux", "lobby", "|03<|11phrac|03> |07hello"));
        assert_eq!(p.to_line(), "phrac~disflux~lobby~~~lobby~|03<|11phrac|03> |07hello~\n");
        assert!(Packet::parse("not a packet").is_none());
        // Nothing sent can break a packet open.
        let q = Packet { from_user: "a~b".into(), body: "one\ntwo~three".into(), ..Default::default() };
        assert_eq!(q.to_line(), "ab~~~~~~onetwothree~\n");
        // Control characters from the server don't reach a terminal.
        assert_eq!(Packet::parse("SERVER~~~~~~a\x1b[2Jb~").unwrap().body, "a_[2Jb");
    }

    #[test]
    fn names() {
        assert_eq!(nick_of("Penny Flux").as_deref(), Some("Penny_Flux"));
        assert_eq!(nick_of("|12red~name").as_deref(), Some("redname"));
        assert!(nick_of("server").is_none() && nick_of("NOTME").is_none() && nick_of("  ").is_none());
        assert_eq!(wire_name("|09dis|15flux BBS"), "disflux_BBS");
        assert_eq!(strip_pipes("|07- |11phrac |03has arrived."), "- phrac has arrived.");
        assert_eq!(strip_pipes("a|b |7 c|"), "a|b |7 c|");
        // CP437's glyphs for control bytes are control bytes on the way out.
        assert!(is_controlling('\u{2190}') && is_controlling('\u{2022}') && is_controlling('\x1b'));
        assert!(!is_controlling('a') && !is_controlling(' ') && !is_controlling('\u{2588}') && !is_controlling('\u{e9}'));
        assert_eq!(Packet::parse("Bob~X~lobby~~~lobby~\u{2190}[2J~").unwrap().body, "_[2J");
    }

    #[tokio::test]
    async fn lines_from_the_server() {
        let mut data: Vec<u8> = b"plain~\r\n".to_vec();
        data.extend_from_slice(b"caf\x82 \xc9\xcd\xbb~\n");
        data.extend(std::iter::repeat_n(b'x', LINE_MAX * 3));
        data.extend_from_slice(b"\nafter~\n");
        let mut rd = BufReader::new(&data[..]);
        assert_eq!(read_line(&mut rd).await.unwrap().as_deref(), Some("plain~"));
        // Not UTF-8: CP437, and the connection carries on.
        assert_eq!(read_line(&mut rd).await.unwrap().as_deref(), Some("caf\u{e9} \u{2554}\u{2550}\u{2557}~"));
        // Far too long: dropped, and the next line comes as it should.
        assert_eq!(read_line(&mut rd).await.unwrap().as_deref(), Some(""));
        assert_eq!(read_line(&mut rd).await.unwrap().as_deref(), Some("after~"));
        assert_eq!(read_line(&mut rd).await.unwrap(), None);
    }
}
