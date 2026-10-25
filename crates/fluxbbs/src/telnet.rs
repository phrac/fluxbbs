//! Telnet (RFC 854), alongside SSH. Many BBS directories and older
//! clients still connect this way. The connection is negotiated into
//! binary, character-at-a-time mode with the server echoing; window size
//! (NAWS) and terminal type are asked for, and the session then runs over
//! a stream that strips telnet commands from input and escapes 0xFF in
//! output. It's unencrypted, so SSH stays the better choice.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::Context as _;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::BbsState;
use crate::session::{self, Caller};

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

/// TTYPE subnegotiation: IS and SEND.
const IS: u8 = 0;
const SEND: u8 = 1;

/// Options we offer to do ourselves, and ones we ask the client to do.
const OURS: [u8; 3] = [ECHO, SGA, BINARY];
const THEIRS: [u8; 4] = [SGA, BINARY, NAWS, TTYPE];

/// What the client told us between the data.
#[derive(Debug, PartialEq)]
enum Event {
    /// Window size (columns, rows).
    Size(u16, u16),
    TermType(String),
    /// WILL / WONT / DO / DONT and the option.
    Option(u8, u8),
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
enum State {
    #[default]
    Data,
    Iac,
    Verb(u8),
    Sub,
    SubIac,
}

/// Splits a telnet byte stream into data and events.
#[derive(Default)]
struct Parser {
    state: State,
    sub: Vec<u8>,
}

impl Parser {
    fn feed(&mut self, input: &[u8], data: &mut Vec<u8>, events: &mut Vec<Event>) {
        for &b in input {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Iac,
                (State::Data, b) => {
                    data.push(b);
                    State::Data
                }
                (State::Iac, IAC) => {
                    data.push(IAC);
                    State::Data
                }
                (State::Iac, WILL | WONT | DO | DONT) => State::Verb(b),
                (State::Iac, SB) => {
                    self.sub.clear();
                    State::Sub
                }
                // NOP, GA, AYT and the like: nothing to do.
                (State::Iac, _) => State::Data,
                (State::Verb(v), opt) => {
                    events.push(Event::Option(v, opt));
                    State::Data
                }
                (State::Sub, IAC) => State::SubIac,
                (State::Sub, b) => {
                    if self.sub.len() < 256 {
                        self.sub.push(b);
                    }
                    State::Sub
                }
                (State::SubIac, IAC) => {
                    self.sub.push(IAC);
                    State::Sub
                }
                (State::SubIac, SE) => {
                    if let Some(e) = subnegotiation(&self.sub) {
                        events.push(e);
                    }
                    State::Data
                }
                (State::SubIac, _) => State::Data,
            };
        }
    }
}

fn subnegotiation(sub: &[u8]) -> Option<Event> {
    match sub {
        [NAWS, w1, w2, h1, h2] => Some(Event::Size(u16::from_be_bytes([*w1, *w2]), u16::from_be_bytes([*h1, *h2]))),
        // Some clients end the name with a NUL (NetRunner sends
        // "ANSI-256COLOR\0"): control characters go with the spaces.
        [TTYPE, IS, name @ ..] => Some(Event::TermType(String::from_utf8_lossy(name).trim_matches(|c: char| c.is_whitespace() || c.is_control()).to_string())),
        _ => None,
    }
}

/// What to say back to an option request, if anything.
fn reply(verb: u8, opt: u8) -> Vec<u8> {
    match verb {
        // They'll do something: ask for their terminal type, or refuse
        // what we don't want.
        WILL if opt == TTYPE => vec![IAC, SB, TTYPE, SEND, IAC, SE],
        WILL if THEIRS.contains(&opt) => vec![],
        WILL => vec![IAC, DONT, opt],
        // They want us to: we already offered these.
        DO if OURS.contains(&opt) => vec![],
        DO => vec![IAC, WONT, opt],
        _ => vec![],
    }
}

/// Escape data for the wire: 0xFF is doubled.
fn escape(data: &[u8], out: &mut Vec<u8>) {
    for &b in data {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
}

/// The connection as the session sees it: plain bytes both ways.
pub struct TelnetStream {
    inner: TcpStream,
    parser: Parser,
    /// Decoded input not yet read.
    input: Vec<u8>,
    /// Bytes waiting to go out (escaped data and our replies).
    output: Vec<u8>,
    size: watch::Sender<(u16, u16)>,
    term: Option<String>,
}

impl TelnetStream {
    fn new(inner: TcpStream, size: watch::Sender<(u16, u16)>) -> Self {
        Self { inner, parser: Parser::default(), input: Vec::new(), output: Vec::new(), size, term: None }
    }

    /// Take in raw bytes from the client.
    fn absorb(&mut self, raw: &[u8]) {
        let mut events = Vec::new();
        self.parser.feed(raw, &mut self.input, &mut events);
        for e in events {
            match e {
                Event::Size(w, h) if w > 0 && h > 0 => {
                    let _ = self.size.send((w, h));
                }
                Event::Size(..) => {}
                Event::TermType(t) => self.term = Some(t),
                Event::Option(verb, opt) => self.output.extend(reply(verb, opt)),
            }
        }
    }

    /// Offer our options and wait a moment for the client's answers
    /// (terminal type above all).
    async fn negotiate(&mut self, wait: Duration) -> std::io::Result<()> {
        let mut hello = Vec::new();
        for opt in OURS {
            hello.extend([IAC, WILL, opt]);
        }
        for opt in THEIRS {
            hello.extend([IAC, DO, opt]);
        }
        self.inner.write_all(&hello).await?;
        let deadline = tokio::time::Instant::now() + wait;
        let mut buf = [0u8; 512];
        while self.term.is_none() {
            let n = match tokio::time::timeout_at(deadline, self.inner.read(&mut buf)).await {
                Ok(r) => r?,
                Err(_) => break,
            };
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            self.absorb(&buf[..n]);
            if !self.output.is_empty() {
                let out = std::mem::take(&mut self.output);
                self.inner.write_all(&out).await?;
            }
        }
        Ok(())
    }

    /// Push out what's queued; Ready once it's all gone.
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while !self.output.is_empty() {
            match Pin::new(&mut self.inner).poll_write(cx, &self.output) {
                Poll::Ready(Ok(0)) => return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into())),
                Poll::Ready(Ok(n)) => {
                    self.output.drain(..n);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for TelnetStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = &mut *self;
        loop {
            if !this.input.is_empty() {
                let n = this.input.len().min(buf.remaining());
                buf.put_slice(&this.input[..n]);
                this.input.drain(..n);
                return Poll::Ready(Ok(()));
            }
            let mut raw = [0u8; 1024];
            let mut rb = ReadBuf::new(&mut raw);
            match Pin::new(&mut this.inner).poll_read(cx, &mut rb) {
                Poll::Ready(Ok(())) => {
                    if rb.filled().is_empty() {
                        return Poll::Ready(Ok(())); // EOF
                    }
                    let got = rb.filled().to_vec();
                    this.absorb(&got);
                    // Answers to option requests go out promptly.
                    if !this.output.is_empty() {
                        let _ = this.poll_drain(cx);
                    }
                }
                other => return other,
            }
        }
    }
}

impl AsyncWrite for TelnetStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
        // Keep the queue bounded: wait for room before taking more.
        if self.output.len() > 64 * 1024 && self.poll_drain(cx)?.is_pending() {
            return Poll::Pending;
        }
        escape(data, &mut self.output);
        let _ = self.poll_drain(cx)?;
        Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.poll_drain(cx)?.is_pending() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.poll_drain(cx)?.is_pending() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Accept telnet connections until the server stops.
pub async fn serve(state: Arc<BbsState>) -> anyhow::Result<()> {
    let listen = state.config().telnet.listen.clone();
    let listener = TcpListener::bind(&listen).await.with_context(|| format!("telnet on {listen}"))?;
    tracing::info!("telnet listening on {listen}");
    loop {
        let (sock, peer) = listener.accept().await?;
        // Banned and denied addresses are dropped before any negotiation.
        if let Some(why) = state.guard.blocked(&state.config().security, peer.ip()) {
            tracing::info!("{peer}: telnet refused: {why}");
            drop(sock);
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(state, sock, peer).await {
                tracing::info!("{peer}: telnet: {e}");
            }
        });
    }
}

async fn connection(state: Arc<BbsState>, sock: TcpStream, peer: SocketAddr) -> std::io::Result<()> {
    sock.set_nodelay(true)?;
    let (tx, rx) = watch::channel((80, 24));
    let mut t = TelnetStream::new(sock, tx);
    t.negotiate(Duration::from_millis(1500)).await?;
    // A client that doesn't say is almost certainly a BBS terminal.
    let term = t.term.clone().unwrap_or_else(|| "ansi-bbs".into());
    let caller = Caller { remote: Some(peer), transport: "telnet", login_name: String::new(), key_user: None, term, size: rx };
    session::run(state, Box::new(t), caller).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[u8]) -> (Vec<u8>, Vec<Event>) {
        let (mut data, mut events) = (Vec::new(), Vec::new());
        Parser::default().feed(input, &mut data, &mut events);
        (data, events)
    }

    #[test]
    fn data_and_escapes() {
        let (data, events) = parse(b"hi\xff\xff!\xff\xf1x");
        assert_eq!(data, b"hi\xff!x", "IAC IAC is a 0xFF byte; IAC NOP is nothing");
        assert!(events.is_empty());
    }

    #[test]
    fn options_and_subnegotiation() {
        let (data, events) = parse(&[
            b'a', IAC, WILL, NAWS, IAC, SB, NAWS, 0, 132, 0, 50, IAC, SE, IAC, SB, TTYPE, IS, b'S', b'Y', b'N', b'C', b'T',
            b'E', b'R', b'M', IAC, SE, b'b',
        ]);
        assert_eq!(data, b"ab");
        assert_eq!(events, [Event::Option(WILL, NAWS), Event::Size(132, 50), Event::TermType("SYNCTERM".into())]);
        // NetRunner ends its name with a NUL.
        let mut netrunner = vec![IAC, SB, TTYPE, IS];
        netrunner.extend(b"ANSI-256COLOR\0");
        netrunner.extend([IAC, SE]);
        assert_eq!(parse(&netrunner).1, [Event::TermType("ANSI-256COLOR".into())]);
    }

    #[test]
    fn split_across_reads() {
        let mut p = Parser::default();
        let (mut data, mut events) = (Vec::new(), Vec::new());
        for chunk in [&[IAC][..], &[SB, NAWS, 0], &[80, 0, 25, IAC], &[SE, b'z']] {
            p.feed(chunk, &mut data, &mut events);
        }
        assert_eq!(events, [Event::Size(80, 25)]);
        assert_eq!(data, b"z");
    }

    #[test]
    fn replies_and_escaping() {
        assert_eq!(reply(WILL, TTYPE), [IAC, SB, TTYPE, SEND, IAC, SE]);
        assert!(reply(WILL, NAWS).is_empty());
        assert_eq!(reply(WILL, 39), [IAC, DONT, 39], "unwanted options are refused");
        assert!(reply(DO, ECHO).is_empty());
        assert_eq!(reply(DO, 5), [IAC, WONT, 5]);
        let mut out = Vec::new();
        escape(b"a\xffb", &mut out);
        assert_eq!(out, b"a\xff\xffb");
    }
}
