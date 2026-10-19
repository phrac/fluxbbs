//! MeshCore (new): a companion radio's channels bridged to message areas.
//! What's said on a channel is posted in its area, and what callers post
//! there goes out on the channel.
//!
//! The radio speaks MeshCore's companion protocol (examples/companion_radio
//! in the MeshCore firmware) over a byte stream: TCP to WiFi companion
//! firmware (port 5000), or a USB radio put on the network with ser2net.
//! Frames to the radio are `<`, a 16-bit little-endian length and the
//! payload; from it, `>` and the same. Replies carry no request id, so
//! commands go one at a time; codes from 0x80 are pushed at any time.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Local;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::BbsState;
use crate::config::{MeshChannel, MeshCore};
use crate::msgbase::{Message, Party};

const CMD_APP_START: u8 = 1;
const CMD_SEND_CHANNEL_TXT_MSG: u8 = 3;
const CMD_SET_DEVICE_TIME: u8 = 6;
const CMD_SYNC_NEXT_MESSAGE: u8 = 10;
const CMD_DEVICE_QUERY: u8 = 22;
const CMD_GET_CHANNEL: u8 = 31;

const RESP_OK: u8 = 0;
const RESP_SELF_INFO: u8 = 5;
const RESP_CHANNEL_MSG_RECV: u8 = 8;
const RESP_NO_MORE_MESSAGES: u8 = 10;
const RESP_DEVICE_INFO: u8 = 13;
const RESP_CHANNEL_MSG_RECV_V3: u8 = 17;
const RESP_CHANNEL_INFO: u8 = 18;

const PUSH_MSG_WAITING: u8 = 0x83;
const TXT_TYPE_PLAIN: u8 = 0;

/// The protocol version asked for (3+ gives SNR with channel messages).
const APP_VERSION: u8 = 3;
/// The firmware's MAX_TEXT_LEN: a channel message, "<sender>: " and all.
const MAX_TEXT: usize = 160;
/// The firmware's MAX_FRAME_SIZE (plus room for transport codes).
const MAX_FRAME: usize = 180;
const REPLY_WAIT: Duration = Duration::from_secs(10);
/// Between the parts of a long post, and between posts: airtime is shared.
const SEND_GAP: Duration = Duration::from_secs(3);

/// A post on its way to the mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub area: String,
    pub from: String,
    pub text: String,
}

/// Where callers' posts are handed to the bridge (nothing happens while
/// it isn't running).
#[derive(Default)]
pub struct Outbox(Mutex<Option<mpsc::UnboundedSender<Outgoing>>>);

impl Outbox {
    /// A public post a caller just made: send it on, if its area is
    /// bridged. The signature and quoted lines stay behind.
    pub fn post(&self, m: &Message) {
        let Some(tx) = self.0.lock().unwrap().clone() else { return };
        let body_end = if m.sig_pos > 0 { (m.sig_pos as usize - 1).min(m.lines.len()) } else { m.lines.len() };
        let text = m.lines[..body_end]
            .iter()
            .filter(|l| !l.chars().take(4).any(|c| c == '>'))
            .flat_map(|l| l.split_whitespace())
            .collect::<Vec<_>>()
            .join(" ");
        let text = if text.is_empty() { m.subject.clone() } else { text };
        let _ = tx.send(Outgoing { area: m.area.clone(), from: m.from.name.clone(), text });
    }
}

/// What the radio says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioInfo {
    pub firmware: u8,
    pub max_channels: u8,
    /// The node name, which the radio puts before everything it sends.
    pub name: String,
}

/// A message heard on a channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub slot: u8,
    pub sender: String,
    pub text: String,
}

/// The companion protocol over a byte stream.
pub struct Radio<S> {
    io: S,
    buf: Vec<u8>,
    /// A message-waiting push came in while waiting for a reply.
    waiting: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Radio<S> {
    pub fn new(io: S) -> Self {
        Self { io, buf: Vec::new(), waiting: false }
    }

    async fn send(&mut self, payload: &[u8]) -> io::Result<()> {
        let mut f = Vec::with_capacity(payload.len() + 3);
        f.push(b'<');
        f.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        f.extend_from_slice(payload);
        self.io.write_all(&f).await?;
        self.io.flush().await
    }

    /// The next frame from the radio. Cancel-safe: what's read is kept.
    pub async fn recv(&mut self) -> io::Result<Vec<u8>> {
        loop {
            if let Some(f) = take_frame(&mut self.buf) {
                return Ok(f);
            }
            let mut tmp = [0u8; 512];
            let n = self.io.read(&mut tmp).await?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
    }

    /// Send a command and wait for its reply, noting pushes on the way.
    async fn command(&mut self, payload: &[u8]) -> io::Result<Vec<u8>> {
        self.send(payload).await?;
        let reply = async {
            loop {
                let f = self.recv().await?;
                match f.first() {
                    Some(&PUSH_MSG_WAITING) => self.waiting = true,
                    Some(&c) if c >= 0x80 => {}
                    Some(_) => return Ok(f),
                    None => {}
                }
            }
        };
        tokio::time::timeout(REPLY_WAIT, reply).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the radio didn't answer"))?
    }

    /// Say hello: the protocol version, the app's name, the time.
    pub async fn start(&mut self) -> io::Result<RadioInfo> {
        let r = self.command(&[CMD_DEVICE_QUERY, APP_VERSION]).await?;
        if r.first() != Some(&RESP_DEVICE_INFO) || r.len() < 2 {
            return Err(io::Error::other(format!("not a MeshCore companion radio (reply {:?})", r.first())));
        }
        let (firmware, max_channels) = (r[1], r.get(3).copied().filter(|&n| n > 0).unwrap_or(8));
        let mut start = vec![CMD_APP_START, 0, 0, 0, 0, 0, 0, 0];
        start.extend_from_slice(b"iniquity");
        let r = self.command(&start).await?;
        if r.first() != Some(&RESP_SELF_INFO) {
            return Err(io::Error::other("the radio didn't start the session"));
        }
        // Type, power, max power, 32-byte key, lat, lon, four flags,
        // frequency, bandwidth, SF and CR come before the name.
        let name = r.get(58..).map(|n| String::from_utf8_lossy(n).trim_end_matches('\0').to_string()).unwrap_or_default();
        // Refused if the radio's clock is already later; either is fine.
        let mut t = vec![CMD_SET_DEVICE_TIME];
        t.extend_from_slice(&now().to_le_bytes());
        self.command(&t).await?;
        Ok(RadioInfo { firmware, max_channels, name })
    }

    /// The radio's channels, by slot.
    pub async fn channels(&mut self, max: u8) -> io::Result<Vec<(u8, String)>> {
        let mut out = Vec::new();
        for slot in 0..max {
            let r = self.command(&[CMD_GET_CHANNEL, slot]).await?;
            if r.first() == Some(&RESP_CHANNEL_INFO) && r.len() >= 34 {
                let name = &r[2..34];
                let name = String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(32)]).to_string();
                if !name.is_empty() {
                    out.push((slot, name));
                }
            }
        }
        Ok(out)
    }

    /// Everything the radio has queued: the channel messages.
    pub async fn drain(&mut self) -> io::Result<Vec<Heard>> {
        self.waiting = false;
        let mut out = Vec::new();
        loop {
            let r = self.command(&[CMD_SYNC_NEXT_MESSAGE]).await?;
            match r.first() {
                Some(&RESP_NO_MORE_MESSAGES) | None => break,
                _ => out.extend(parse_channel_msg(&r)),
            }
        }
        Ok(out)
    }

    /// Send text on a channel (the radio adds "<its name>: ").
    pub async fn send_channel(&mut self, slot: u8, text: &str) -> io::Result<()> {
        let mut c = vec![CMD_SEND_CHANNEL_TXT_MSG, TXT_TYPE_PLAIN, slot];
        c.extend_from_slice(&now().to_le_bytes());
        c.extend_from_slice(text.as_bytes());
        let r = self.command(&c).await?;
        if r.first() == Some(&RESP_OK) {
            Ok(())
        } else {
            Err(io::Error::other(format!("the radio refused the message (error {})", r.get(1).copied().unwrap_or(0))))
        }
    }
}

fn now() -> u32 {
    Local::now().timestamp() as u32
}

/// A whole `>` frame off the front of `buf`, skipping anything before it.
fn take_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        let start = buf.iter().position(|&b| b == b'>')?;
        buf.drain(..start);
        if buf.len() < 3 {
            return None;
        }
        let len = u16::from_le_bytes([buf[1], buf[2]]) as usize;
        if len == 0 || len > MAX_FRAME {
            // Not a frame header after all.
            buf.drain(..1);
            continue;
        }
        if buf.len() < 3 + len {
            return None;
        }
        let f = buf[3..3 + len].to_vec();
        buf.drain(..3 + len);
        return Some(f);
    }
}

/// A channel message reply (`RESP_CODE_CHANNEL_MSG_RECV`, or `_V3` with
/// SNR first). Its text is "<sender>: <message>".
fn parse_channel_msg(r: &[u8]) -> Option<Heard> {
    let (slot, kind, text) = match *r.first()? {
        RESP_CHANNEL_MSG_RECV_V3 if r.len() >= 11 => (r[4], r[6], &r[11..]),
        RESP_CHANNEL_MSG_RECV if r.len() >= 8 => (r[1], r[3], &r[8..]),
        _ => return None,
    };
    if kind != TXT_TYPE_PLAIN {
        return None;
    }
    let text = String::from_utf8_lossy(text).trim_end_matches('\0').to_string();
    let (sender, text) = match text.split_once(": ") {
        Some((s, t)) if !s.is_empty() && s.len() <= 32 => (s.to_string(), t.to_string()),
        _ => ("someone".to_string(), text),
    };
    Some(Heard { slot, sender, text: text.trim().to_string() })
}

/// A post as channel messages: "<from>: text", each within `budget`
/// bytes, at most `max` of them, the last cut short with ".." if need be.
fn parts(from: &str, text: &str, budget: usize, max: usize) -> Vec<String> {
    let prefix = format!("{from}: ");
    let room = budget.saturating_sub(prefix.len());
    if room < 8 || max == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() && out.len() < max {
        let last = out.len() + 1 == max;
        if rest.len() <= room {
            out.push(format!("{prefix}{rest}"));
            break;
        }
        // Leave room for ".." on the last part.
        let limit = if last { room - 2 } else { room };
        let mut cut = limit;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        // Break at a space if there's one in the back half.
        if let Some(sp) = rest[..cut].rfind(' ').filter(|&sp| sp > cut / 2) {
            cut = sp;
        }
        let piece = rest[..cut].trim_end();
        out.push(if last { format!("{prefix}{piece}..") } else { format!("{prefix}{piece}") });
        rest = rest[cut..].trim_start();
    }
    out
}

/// The bridge: keep a session with the radio, reconnecting as needed.
pub async fn run(state: Arc<BbsState>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    *state.mesh.0.lock().unwrap() = Some(tx);
    let mut down = false;
    loop {
        let cfg = state.config().meshcore.clone();
        if !cfg.enabled || cfg.address.is_empty() {
            // Posts made while it's off aren't kept for later.
            while rx.try_recv().is_ok() {}
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        match tokio::time::timeout(REPLY_WAIT, tokio::net::TcpStream::connect(&cfg.address)).await {
            Ok(Ok(stream)) => {
                let _ = stream.set_nodelay(true);
                tracing::info!("meshcore: connected to {}", cfg.address);
                down = false;
                match session(&state, &cfg, Radio::new(stream), &mut rx).await {
                    Ok(()) => tracing::info!("meshcore: settings changed; reconnecting"),
                    Err(e) => tracing::warn!("meshcore: {e}"),
                }
                continue;
            }
            Ok(Err(e)) if !down => tracing::warn!("meshcore: can't reach {}: {e}", cfg.address),
            Err(_) if !down => tracing::warn!("meshcore: can't reach {}: timed out", cfg.address),
            _ => {}
        }
        down = true;
        tokio::time::sleep(Duration::from_secs(cfg.reconnect_seconds.max(5))).await;
    }
}

/// One connection: start, find the channels, then pass messages both
/// ways until the link drops or the settings change.
async fn session<S: AsyncRead + AsyncWrite + Unpin>(state: &BbsState, cfg: &MeshCore, mut radio: Radio<S>, rx: &mut mpsc::UnboundedReceiver<Outgoing>) -> io::Result<()> {
    let info = radio.start().await?;
    tracing::info!("meshcore: radio {:?}, firmware {}", info.name, info.firmware);
    let slots = radio.channels(info.max_channels).await?;
    let mut map: Vec<(u8, MeshChannel)> = Vec::new();
    for ch in &cfg.channels {
        match slots.iter().find(|(_, name)| name.eq_ignore_ascii_case(&ch.channel)) {
            Some((slot, _)) => map.push((*slot, ch.clone())),
            None => tracing::warn!("meshcore: the radio has no channel {:?}", ch.channel),
        }
    }
    let budget = MAX_TEXT.saturating_sub(info.name.len() + 2);
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        if radio.waiting {
            for h in radio.drain().await? {
                import(state, &map, h);
            }
        }
        tokio::select! {
            f = radio.recv() => {
                if f?.first() == Some(&PUSH_MSG_WAITING) {
                    radio.waiting = true;
                }
            }
            Some(out) = rx.recv() => {
                for (slot, ch) in map.iter().filter(|(_, ch)| ch.to_mesh && ch.area.eq_ignore_ascii_case(&out.area)) {
                    for p in parts(&out.from, &out.text, budget, cfg.max_parts.max(1) as usize) {
                        radio.send_channel(*slot, &p).await?;
                        tracing::info!("meshcore: {} -> {}", out.from, ch.channel);
                        tokio::time::sleep(SEND_GAP).await;
                    }
                }
            }
            _ = tick.tick() => {
                if state.config().meshcore != *cfg {
                    return Ok(());
                }
                // Now and then, in case a push went astray.
                radio.waiting = true;
            }
        }
    }
}

/// Post a channel message in its area.
fn import(state: &BbsState, map: &[(u8, MeshChannel)], h: Heard) {
    let Some((_, ch)) = map.iter().find(|(slot, ch)| *slot == h.slot && ch.from_mesh) else { return };
    let areas = state.areas();
    let Some(area) = areas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&ch.area)) else {
        tracing::warn!("meshcore: no message area {:?} for {:?}", ch.area, ch.channel);
        return;
    };
    let from = Party { user_id: 0, alias: h.sender.clone(), real_name: h.sender.clone(), name: h.sender.clone(), note: "MeshCore".into() };
    let to = Party { user_id: 0, alias: "All".into(), real_name: "All".into(), name: "All".into(), note: String::new() };
    let subject: String = h.text.chars().take(40).collect();
    let mut m = Message {
        id: 0,
        area: area.file.clone(),
        num: 0,
        from,
        to,
        subject,
        posted: Local::now().naive_local(),
        deleted: false,
        private: false,
        anonymous: false,
        replies: 0,
        sig_pos: 0,
        tag: 0,
        lines: wrap(&h.text, 79),
    };
    match state.msgs.add(&mut m) {
        Ok(()) => tracing::info!("meshcore: {} on {} -> {} #{}", h.sender, ch.channel, area.name, m.num),
        Err(e) => tracing::error!("meshcore: posting in {}: {e}", area.name),
    }
}

/// Word-wrap to `width` columns.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for w in text.split_whitespace() {
        let line = lines.last_mut().expect("a line");
        if !line.is_empty() && line.chars().count() + 1 + w.chars().count() > width {
            lines.push(w.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(w);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut f = vec![b'>'];
        f.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn frames_come_apart() {
        let mut buf = b"junk".to_vec();
        buf.extend(frame(&[1, 2, 3]));
        buf.extend(frame(&[PUSH_MSG_WAITING]));
        buf.extend(&frame(&[9, 9])[..3]);
        assert_eq!(take_frame(&mut buf), Some(vec![1, 2, 3]));
        assert_eq!(take_frame(&mut buf), Some(vec![PUSH_MSG_WAITING]));
        assert_eq!(take_frame(&mut buf), None, "a partial frame waits");
        buf.extend([9, 9]);
        assert_eq!(take_frame(&mut buf), Some(vec![9, 9]));
    }

    #[test]
    fn channel_messages_parse() {
        let mut v3 = vec![RESP_CHANNEL_MSG_RECV_V3, 40, 0, 0, 1, 2, TXT_TYPE_PLAIN, 1, 2, 3, 4];
        v3.extend(b"Alice: hi there");
        assert_eq!(parse_channel_msg(&v3), Some(Heard { slot: 1, sender: "Alice".into(), text: "hi there".into() }));
        let mut old = vec![RESP_CHANNEL_MSG_RECV, 0, 2, TXT_TYPE_PLAIN, 1, 2, 3, 4];
        old.extend(b"no sender here");
        assert_eq!(parse_channel_msg(&old).unwrap().sender, "someone");
        assert_eq!(parse_channel_msg(&[RESP_OK]), None);
    }

    #[test]
    fn posts_fit_the_packet() {
        assert_eq!(parts("phrac", "hello mesh", 150, 2), ["phrac: hello mesh"]);
        let long = "word ".repeat(80);
        let p = parts("phrac", &long, 150, 2);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|s| s.len() <= 150 && s.starts_with("phrac: ")));
        assert!(p[1].ends_with(".."));
        // Multi-byte text is cut on a character boundary.
        let p = parts("x", &"é".repeat(200), 50, 1);
        assert!(p[0].len() <= 50 && p[0].ends_with(".."));
    }

    /// A scripted companion radio on the other end of a pipe.
    async fn fake_radio(mut io: tokio::io::DuplexStream, queued: Vec<Vec<u8>>, sent: Arc<Mutex<Vec<(u8, String)>>>) {
        let mut queued = std::collections::VecDeque::from(queued);
        let mut buf = Vec::new();
        loop {
            let mut tmp = [0u8; 256];
            let Ok(n) = io.read(&mut tmp).await else { return };
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n]);
            while buf.len() >= 3 && buf[0] == b'<' {
                let len = u16::from_le_bytes([buf[1], buf[2]]) as usize;
                if buf.len() < 3 + len {
                    break;
                }
                let cmd: Vec<u8> = buf.drain(..3 + len).skip(3).collect();
                let reply = match cmd[0] {
                    CMD_DEVICE_QUERY => vec![RESP_DEVICE_INFO, 14, 50, 4],
                    CMD_APP_START => {
                        let mut r = vec![RESP_SELF_INFO];
                        r.resize(58, 0);
                        r.extend(b"disflux");
                        r
                    }
                    CMD_GET_CHANNEL => {
                        let names = ["Public", "disflux", "", ""];
                        let name = names[cmd[1] as usize];
                        if name.is_empty() {
                            vec![1, 2]
                        } else {
                            let mut r = vec![RESP_CHANNEL_INFO, cmd[1]];
                            let mut n = name.as_bytes().to_vec();
                            n.resize(32, 0);
                            r.extend(n);
                            r.extend([0u8; 16]);
                            r
                        }
                    }
                    CMD_SYNC_NEXT_MESSAGE => queued.pop_front().unwrap_or(vec![RESP_NO_MORE_MESSAGES]),
                    CMD_SEND_CHANNEL_TXT_MSG => {
                        sent.lock().unwrap().push((cmd[2], String::from_utf8_lossy(&cmd[7..]).to_string()));
                        vec![RESP_OK]
                    }
                    _ => vec![RESP_OK],
                };
                // A push arrives ahead of a reply now and then.
                if cmd[0] == CMD_GET_CHANNEL && cmd[1] == 0 {
                    io.write_all(&frame(&[PUSH_MSG_WAITING])).await.unwrap();
                }
                io.write_all(&frame(&reply)).await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn a_session_with_a_radio() {
        let (ours, theirs) = tokio::io::duplex(4096);
        let mut heard = vec![RESP_CHANNEL_MSG_RECV_V3, 20, 0, 0, 1, 3, TXT_TYPE_PLAIN, 0, 0, 0, 0];
        heard.extend(b"Bob: anyone on?");
        let sent = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn(fake_radio(theirs, vec![heard], sent.clone()));
        let mut radio = Radio::new(ours);
        let info = radio.start().await.unwrap();
        assert_eq!(info, RadioInfo { firmware: 14, max_channels: 4, name: "disflux".into() });
        assert_eq!(radio.channels(info.max_channels).await.unwrap(), [(0, "Public".to_string()), (1, "disflux".to_string())]);
        assert!(radio.waiting, "the push during the channel list was noted");
        assert_eq!(radio.drain().await.unwrap(), [Heard { slot: 1, sender: "Bob".into(), text: "anyone on?".into() }]);
        radio.send_channel(1, "phrac: hello").await.unwrap();
        assert_eq!(*sent.lock().unwrap(), [(1, "phrac: hello".to_string())]);
    }
}
