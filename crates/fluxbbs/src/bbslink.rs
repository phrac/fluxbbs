//! New: BBSLink (bbslink.net), door games on another server played from
//! here (menu command `DB code`). The board asks the server's web side
//! for a token (`token.php`), proves who it is with the token hashed with
//! its codes (`auth.php`, which answers `complete`), then connects the
//! caller to the server's telnet port, where the game is waiting. The
//! three requests go to one address (the server ties them together by
//! where they come from), one caller's handshake at a time.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use md5::{Digest, Md5};
use rand::RngExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::config::BbsLink;
use crate::doors::DoorInput;
use crate::session::Session;

/// The whole handshake, web and telnet.
const HANDSHAKE_SECS: u64 = 20;
/// Longest web answer read.
const ANSWER_MAX: usize = 16 * 1024;

/// One handshake at a time: two callers' tokens mustn't cross.
static HANDSHAKE: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A door code as BBSLink takes it: letters, digits, `_` and `-`.
pub fn valid_code(code: &str) -> bool {
    (1..=32).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What's missing before BBSLink can be used, if anything.
pub fn not_ready(cfg: &BbsLink) -> Option<&'static str> {
    if !cfg.enabled {
        Some("BBSLink isn't turned on")
    } else if cfg.host.trim().is_empty() || cfg.system_code.trim().is_empty() || cfg.auth_code.trim().is_empty() || cfg.scheme_code.trim().is_empty() {
        Some("BBSLink's codes aren't set up")
    } else {
        None
    }
}

/// The handshake: a token, then the board's credentials for this caller
/// and door; then the telnet connection, to the same address.
async fn connect(cfg: &BbsLink, user: u32, rows: u16, door: &str) -> anyhow::Result<TcpStream> {
    let host = cfg.host.trim().to_string();
    let _one = HANDSHAKE.lock().await;
    let ip: IpAddr = tokio::net::lookup_host((host.as_str(), 80))
        .await?
        .map(|a| a.ip())
        .min_by_key(|ip| ip.is_ipv6())
        .ok_or_else(|| anyhow::anyhow!("{host} has no address"))?;
    // BBSLink reads its headers by their exact names (`X-Auth`, not
    // `x-auth`, which it answers with "Unauthorised (Error 7)").
    let client = reqwest::Client::builder()
        .http1_title_case_headers()
        .resolve(&host, SocketAddr::new(ip, 80))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()?;
    let key: String = {
        let mut rng = rand::rng();
        (0..6).map(|_| b"abcdefghijklmnopqrstuvwxyz0123456789"[rng.random_range(0..36)] as char).collect()
    };
    // Read no more than ANSWER_MAX of an answer, whatever comes.
    let answer = |mut r: reqwest::Response| async move {
        let mut body = Vec::new();
        while body.len() < ANSWER_MAX
            && let Some(chunk) = r.chunk().await?
        {
            body.extend_from_slice(&chunk);
        }
        body.truncate(ANSWER_MAX);
        anyhow::Ok(String::from_utf8_lossy(&body).trim().to_string())
    };
    let token = answer(client.get(format!("http://{host}/token.php?key={key}")).send().await?).await?;
    // It goes back as a header: nothing but printable ASCII.
    anyhow::ensure!(!token.is_empty() && token.len() <= 128 && token.bytes().all(|b| b.is_ascii_graphic()), "no token from {host}");
    let auth = client
        .get(format!("http://{host}/auth.php?key={key}"))
        .header("X-User", user.to_string())
        .header("X-System", cfg.system_code.trim())
        .header("X-Auth", md5_hex(&format!("{}{token}", cfg.auth_code.trim())))
        .header("X-Code", md5_hex(&format!("{}{token}", cfg.scheme_code.trim())))
        .header("X-Rows", rows.to_string())
        .header("X-Key", &key)
        .header("X-Door", door)
        .header("X-Token", &token)
        .header("X-Type", "fluxbbs")
        .header("X-Version", env!("CARGO_PKG_VERSION"))
        .send()
        .await?;
    let said = answer(auth).await?;
    anyhow::ensure!(said == "complete", "BBSLink said: {}", said.chars().take(100).collect::<String>());
    let stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((ip, 23))).await??;
    stream.set_nodelay(true)?;
    Ok(stream)
}

// Telnet (RFC 854) commands and the options handled.
const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const BINARY: u8 = 0;
const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

/// The telnet side of the connection: commands taken out of what the
/// server sends and answered, the rest passed on.
pub struct Telnet {
    state: State,
    sb: Vec<u8>,
    /// Options on, each side's (RFC 1143): the server's (it said WILL and
    /// we DO) and ours (we said WILL and it DOes). A request that would
    /// change nothing isn't answered, so the two can't loop.
    theirs: [bool; 256],
    ours: [bool; 256],
    /// The last byte passed on was a CR (a NUL after it is padding).
    after_cr: bool,
    rows: u16,
}

impl Default for Telnet {
    fn default() -> Self {
        Self { state: State::Data, sb: Vec::new(), theirs: [false; 256], ours: [false; 256], after_cr: false, rows: 24 }
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
enum State {
    #[default]
    Data,
    Iac,
    Verb(u8),
    Sub,
    SubIac,
}

impl Telnet {
    pub fn new(rows: u16) -> Self {
        Self { rows, ..Self::default() }
    }

    /// What to send first: an 8-bit session both ways, as BBSLink's own
    /// clients ask for (`telnet -8`). Taken as agreed; a refusal turns
    /// it off again.
    pub fn start(&mut self) -> Vec<u8> {
        self.theirs[BINARY as usize] = true;
        self.ours[BINARY as usize] = true;
        vec![IAC, DO, BINARY, IAC, WILL, BINARY]
    }

    /// We send binary: a CR is a CR (else CR NUL).
    pub fn binary(&self) -> bool {
        self.ours[BINARY as usize]
    }

    /// What the server sent: (for the caller, answers for the server).
    pub fn server_sent(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut out, mut reply) = (Vec::new(), Vec::new());
        for &b in bytes {
            match self.state {
                State::Data if b == IAC => self.state = State::Iac,
                State::Data => {
                    // A NUL after a CR is padding, outside binary.
                    if !(self.after_cr && b == 0 && !self.theirs[BINARY as usize]) {
                        out.push(b);
                    }
                    self.after_cr = b == b'\r';
                }
                State::Iac => {
                    self.state = State::Data;
                    match b {
                        IAC => out.push(IAC),
                        WILL | WONT | DO | DONT => self.state = State::Verb(b),
                        SB => {
                            self.sb.clear();
                            self.state = State::Sub;
                        }
                        _ => {}
                    }
                }
                State::Verb(v) => {
                    self.state = State::Data;
                    self.answer(v, b, &mut reply);
                }
                State::Sub if b == IAC => self.state = State::SubIac,
                State::Sub => {
                    if self.sb.len() < 64 {
                        self.sb.push(b);
                    }
                }
                State::SubIac if b == SE => {
                    self.state = State::Data;
                    // TTYPE SEND: say ANSI.
                    if self.sb.first() == Some(&TTYPE) && self.sb.get(1) == Some(&1) {
                        reply.extend([IAC, SB, TTYPE, 0]);
                        reply.extend(b"ANSI");
                        reply.extend([IAC, SE]);
                    }
                }
                State::SubIac => {
                    if b == IAC && self.sb.len() < 64 {
                        self.sb.push(IAC);
                    }
                    self.state = State::Sub;
                }
            }
        }
        (out, reply)
    }

    fn answer(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        let i = opt as usize;
        match verb {
            // The server offers: take what we use, once; refuse the rest.
            WILL if matches!(opt, BINARY | ECHO | SGA) => {
                if !self.theirs[i] {
                    self.theirs[i] = true;
                    reply.extend([IAC, DO, opt]);
                }
            }
            WILL => reply.extend([IAC, DONT, opt]),
            WONT if self.theirs[i] => {
                self.theirs[i] = false;
                reply.extend([IAC, DONT, opt]);
            }
            // The server asks: do what we can, once; refuse the rest.
            DO if matches!(opt, BINARY | SGA | TTYPE | NAWS) => {
                if !self.ours[i] {
                    self.ours[i] = true;
                    reply.extend([IAC, WILL, opt]);
                    if opt == NAWS {
                        reply.extend(self.naws());
                    }
                }
            }
            DO => reply.extend([IAC, WONT, opt]),
            DONT if self.ours[i] => {
                self.ours[i] = false;
                reply.extend([IAC, WONT, opt]);
            }
            _ => {}
        }
    }

    /// The window size, 80 by the caller's rows (an IAC in it doubled).
    fn naws(&self) -> Vec<u8> {
        let mut out = vec![IAC, SB, NAWS];
        for b in [0, 80].into_iter().chain(self.rows.max(1).to_be_bytes()) {
            out.push(b);
            if b == IAC {
                out.push(IAC);
            }
        }
        out.extend([IAC, SE]);
        out
    }

    /// What the caller typed, for the server: IAC doubled, and outside
    /// binary mode a CR as CR NUL.
    pub fn to_server(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &b in bytes {
            match b {
                IAC => out.extend([IAC, IAC]),
                b'\r' if !self.binary() => out.extend([b'\r', 0]),
                _ => out.push(b),
            }
        }
        out
    }
}

/// How a BBSLink game ended.
enum End {
    Closed,
    TimeUp,
    Hangup,
}

impl Session {
    /// `DB code` (new): a BBSLink door.
    pub async fn bbslink_door(&mut self, code: &str) -> io::Result<()> {
        let code = code.trim();
        let cfg = self.state.config().bbslink.clone();
        if let Some(why) = not_ready(&cfg) {
            tracing::warn!("bbslink: {why}");
            return self.note_say("BbsLinkOff").await;
        }
        if !valid_code(code) {
            tracing::warn!("bbslink: bad door code {code:?}");
            return self.note_say("BbsLinkOff").await;
        }
        let name = if self.menu.cmd_desc.trim().is_empty() { format!("BBSLink {code}") } else { self.menu.cmd_desc.trim().to_string() };
        self.newline();
        self.say_line("BbsLinkConnecting").await?;
        self.flush().await?;
        let rows = self.screen.rows();
        let stream = match tokio::time::timeout(Duration::from_secs(HANDSHAKE_SECS), connect(&cfg, self.user.id, rows, code)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return self.bbslink_failed(&format!("{e:#}")).await,
            Err(_) => return self.bbslink_failed("no answer in time").await,
        };
        tracing::info!("{}: BBSLink {code}", self.user.handle);
        self.seen.doors.insert(name.clone());
        self.save_user();
        self.node.update(|n| n.activity = format!("Running {name}"));
        let started = crate::clock::now().naive_local();
        let end = self.bbslink_run(stream, rows).await;
        self.input.clear();
        self.screen.resync();
        self.full_screen_done();
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        self.record_play(&name, started);
        match end? {
            End::Hangup => Err(io::ErrorKind::UnexpectedEof.into()),
            End::TimeUp => {
                tracing::info!("{}: out of time in BBSLink", self.user.handle);
                Ok(())
            }
            End::Closed => Ok(()),
        }
    }

    async fn bbslink_failed(&mut self, why: &str) -> io::Result<()> {
        tracing::warn!("{}: BBSLink: {why}", self.user.handle);
        let s = self.strings().fmt("BbsLinkFailed", &[&fluxbbs_term::lit(why)]);
        self.note(s).await
    }

    /// The caller and the game, until one of them goes or time's up.
    async fn bbslink_run(&mut self, mut stream: TcpStream, rows: u16) -> io::Result<End> {
        let mut telnet = Telnet::new(rows);
        // A server that stops reading mustn't hold the caller (or their
        // time limit) up: a write that can't go in a while ends it.
        async fn send(stream: &mut TcpStream, bytes: &[u8]) -> io::Result<()> {
            match tokio::time::timeout(Duration::from_secs(30), stream.write_all(bytes)).await {
                Ok(r) => r,
                Err(_) => Err(io::ErrorKind::TimedOut.into()),
            }
        }
        send(&mut stream, &telnet.start()).await?;
        let mut input = DoorInput::new(self.screen.charset() == fluxbbs_term::Charset::Utf8);
        let pending = self.take_pending_input();
        if !pending.is_empty() {
            send(&mut stream, &telnet.to_server(&input.feed(&pending))).await?;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        let mut sbuf = [0u8; 4096];
        let mut cbuf = [0u8; 1024];
        loop {
            tokio::select! {
                r = stream.read(&mut sbuf) => {
                    let n: usize = r.unwrap_or_default();
                    if n == 0 {
                        return Ok(End::Closed);
                    }
                    let (out, reply) = telnet.server_sent(&sbuf[..n]);
                    if !reply.is_empty() && send(&mut stream, &reply).await.is_err() {
                        return Ok(End::Closed);
                    }
                    self.screen.write_bytes(&out);
                    self.flush().await?;
                }
                r = self.io.read(&mut cbuf) => {
                    match r {
                        Ok(n) if n > 0 => {
                            if send(&mut stream, &telnet.to_server(&input.feed(&cbuf[..n]))).await.is_err() {
                                return Ok(End::Closed);
                            }
                        }
                        _ => return Ok(End::Hangup),
                    }
                }
                _ = tick.tick() => {
                    if self.minutes_left() == 0 {
                        return Ok(End::TimeUp);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_codes() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert!(valid_code("lord") && valid_code("tw2002") && valid_code("menu"));
        assert!(!valid_code("") && !valid_code("lord;rm") && !valid_code("a b"));
        let mut c = BbsLink { enabled: true, ..BbsLink::default() };
        assert!(not_ready(&c).is_some());
        c.system_code = "s".into();
        c.auth_code = "a".into();
        c.scheme_code = "c".into();
        assert!(not_ready(&c).is_none());
    }

    #[test]
    fn telnet_is_answered_and_stripped() {
        let mut t = Telnet::new(24);
        let (out, reply) = t.server_sent(&[b'h', IAC, WILL, ECHO, b'i', IAC, DO, NAWS, IAC, DO, 99, b'\r', 0, IAC, IAC]);
        assert_eq!(out, [b'h', b'i', b'\r', IAC]);
        assert_eq!(reply, [IAC, DO, ECHO, IAC, WILL, NAWS, IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE, IAC, WONT, 99]);
        // Asked again: not answered again; turned off and on, answered.
        assert!(t.server_sent(&[IAC, WILL, ECHO]).1.is_empty());
        assert_eq!(t.server_sent(&[IAC, WONT, ECHO, IAC, WILL, ECHO]).1, [IAC, DONT, ECHO, IAC, DO, ECHO]);
        let (_, reply) = t.server_sent(&[IAC, SB, TTYPE, 1, IAC, SE]);
        assert_eq!(reply, [IAC, SB, TTYPE, 0, b'A', b'N', b'S', b'I', IAC, SE]);
        assert_eq!(t.to_server(b"a\r\xff"), [b'a', b'\r', 0, IAC, IAC]);
        // Asked for binary at the start; the server agreeing isn't
        // answered again, and a CR then goes as it is.
        assert_eq!(t.start(), [IAC, DO, BINARY, IAC, WILL, BINARY]);
        assert!(t.server_sent(&[IAC, WILL, BINARY, IAC, DO, BINARY]).1.is_empty());
        assert_eq!(t.to_server(b"\r"), [b'\r']);
        // It won't take ours: CR NUL again, and the refusal acknowledged.
        assert_eq!(t.server_sent(&[IAC, DONT, BINARY]).1, [IAC, WONT, BINARY]);
        assert_eq!(t.to_server(b"\r"), [b'\r', 0]);
        // A window size byte of 255 goes doubled.
        let big = Telnet::new(255);
        assert_eq!(big.naws(), [IAC, SB, NAWS, 0, 80, 0, IAC, IAC, IAC, SE]);
    }

    /// The real handshake, with codes from the environment (BBSLINK_SYSTEM,
    /// BBSLINK_AUTH, BBSLINK_SCHEME): `cargo test live_handshake --
    /// --ignored`. Signs in as user 1 and hangs up at once.
    #[tokio::test]
    #[ignore]
    async fn live_handshake() {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let cfg = BbsLink { enabled: true, system_code: env("BBSLINK_SYSTEM"), auth_code: env("BBSLINK_AUTH"), scheme_code: env("BBSLINK_SCHEME"), ..BbsLink::default() };
        connect(&cfg, 1, 25, "menu").await.expect("BBSLink let us in");
    }
}
