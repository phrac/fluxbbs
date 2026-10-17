//! File transfers (the original's missing `Transfer` unit).
//!
//! Zmodem is built in (the `zmodem2` state machine, driven over the raw
//! connection). Other protocols are external programs from the config,
//! run with stdin/stdout connected to the caller, which is how the
//! original ran DSZ.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iniquity_term::Slot;
use iniquity_term::codes::resize;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use zmodem2::{Action, Event, FileInfo, Position};

use crate::config::Protocol;
use crate::extprog::shell_quote;
use crate::input::Key;
use crate::session::{Session, waves};

/// Give up after this long without hearing from the other end.
const STALL: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub enum Proto {
    Zmodem,
    External(Protocol),
}

impl Proto {
    pub fn name(&self) -> &str {
        match self {
            Proto::Zmodem => "Zmodem",
            Proto::External(p) => &p.desc,
        }
    }
}

fn zerr(e: zmodem2::Error) -> io::Error {
    io::Error::other(format!("zmodem: {e:?}"))
}

/// A safe local name for a file the caller sent: no directories, no
/// leading dots, printable ASCII only.
pub fn safe_name(raw: &[u8]) -> Option<String> {
    let raw = String::from_utf8_lossy(raw);
    let base = raw.rsplit(['/', '\\']).next()?.trim();
    let clean: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-+~!#$()".contains(c) { c } else { '_' })
        .collect::<String>()
        .trim_start_matches('.')
        .chars()
        .take(64)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

async fn write_wire<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> io::Result<()> {
    w.write_all(bytes).await?;
    w.flush().await
}

impl Session {
    /// The protocols this caller may use.
    fn protocols(&self, batch: bool) -> Vec<(char, Proto)> {
        let mut out = vec![('Z', Proto::Zmodem)];
        for p in &self.state.protocols().list {
            if self.acs(&p.acs) && (!batch || p.batch) && !p.key.eq_ignore_ascii_case(&'Z') {
                out.push((p.key.to_ascii_uppercase(), Proto::External(p.clone())));
            }
        }
        out
    }

    /// Ask which protocol to use (`xferSend`'s menu). None if aborted.
    pub async fn select_protocol(&mut self, batch: bool) -> io::Result<Option<Proto>> {
        let protos = self.protocols(batch);
        let mut rep = self.repeat_begin("PROTL", false).await?;
        if rep.is_none() {
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line("|U4 Key  Protocol                        Type").await?;
        }
        for (key, p) in &protos {
            let kind = match p {
                Proto::Zmodem => "Batch",
                Proto::External(e) if e.batch => "Batch",
                Proto::External(_) => "Single",
            };
            match rep.as_mut() {
                Some(r) => {
                    let w = waves([("KY", key.to_string()), ("PN", p.name().to_string()), ("PT", kind.to_string())]);
                    self.repeat_row(r, &w, false).await?;
                }
                None => {
                    let line = format!("|U1 |U3{}|U1 {} {kind}", resize(&key.to_string(), 4), resize(p.name(), 31));
                    self.show_line(&line).await?;
                }
            }
        }
        if let Some(r) = rep {
            self.repeat_end(r).await?;
        }
        self.say("XferSelectProtocol").await?;
        loop {
            match self.read_key().await? {
                Key::Enter | Key::Esc => {
                    self.newline();
                    return Ok(None);
                }
                k => {
                    if let Some(found) = k.upper().and_then(|c| protos.iter().find(|(key, _)| *key == c)) {
                        self.write(&found.0.to_string());
                        self.newline();
                        return Ok(Some(found.1.clone()));
                    }
                }
            }
        }
    }

    /// Hand the connection back to the terminal model after raw bytes.
    async fn after_transfer(&mut self) -> io::Result<()> {
        // Swallow stray protocol bytes (e.g. the receiver's "OO").
        let mut buf = [0u8; 512];
        while let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(500), self.io.read(&mut buf)).await {
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        self.input.clear();
        self.screen.resync();
        self.newline();
        Ok(())
    }

    /// Bytes the session had already read, for the protocol to see first.
    fn take_pending_input(&mut self) -> Vec<u8> {
        self.input.drain(..).collect()
    }

    /// Send files with a protocol. Returns which arrived.
    pub async fn send_files(&mut self, proto: &Proto, files: &[PathBuf]) -> io::Result<Vec<bool>> {
        let s = self.strings().fmt("s275", &[proto.name()]);
        self.show_line(&s).await?;
        self.flush().await?;
        self.node.update(|n| n.activity = "Downloading".into());
        let result = match proto {
            Proto::Zmodem => self.zmodem_send(files).await,
            Proto::External(p) => {
                let list = files.iter().map(|f| shell_quote(&f.to_string_lossy())).collect::<Vec<_>>().join(" ");
                let ok = self.run_external(&p.send.replace("%F", &list), Path::new(".")).await?;
                Ok(vec![ok; files.len()])
            }
        };
        self.after_transfer().await?;
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        result
    }

    /// Receive files into `dir`. Returns the files that arrived.
    pub async fn receive_files(&mut self, proto: &Proto, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let s = self.strings().fmt("s276", &[proto.name()]);
        self.show_line(&s).await?;
        self.flush().await?;
        self.node.update(|n| n.activity = "Uploading".into());
        std::fs::create_dir_all(dir)?;
        let result = match proto {
            Proto::Zmodem => self.zmodem_receive(dir).await,
            Proto::External(p) => {
                self.run_external(&p.receive, dir).await?;
                let mut got = Vec::new();
                for e in std::fs::read_dir(dir)? {
                    let p = e?.path();
                    if p.is_file() {
                        got.push(p);
                    }
                }
                Ok(got)
            }
        };
        self.after_transfer().await?;
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        result
    }

    async fn zmodem_send(&mut self, files: &[PathBuf]) -> io::Result<Vec<bool>> {
        let mut done = vec![false; files.len()];
        if files.is_empty() {
            return Ok(done);
        }
        let mut tx = zmodem2::Sender::new().map_err(zerr)?;
        // Reliable transport: don't wait for an ack every few packets.
        tx.set_streaming_window(usize::MAX);
        let mut cur = 0;
        let mut file: Option<tokio::fs::File> = None;
        let mut start_next = true;
        let mut finishing = false;
        let mut ended = false;
        let mut pending = self.take_pending_input();
        let mut idle = Duration::ZERO;
        let mut buf = vec![0u8; 8192];
        loop {
            if start_next && !finishing {
                if cur < files.len() {
                    let path = &files[cur];
                    let meta = tokio::fs::metadata(path).await?;
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs() as u32);
                    let size = u32::try_from(meta.len()).map_err(|_| io::Error::other("file too large for zmodem"))?;
                    let info = FileInfo::new(name.as_bytes(), Some(Position::new(size))).with_modified(mtime);
                    match tx.start_file(info) {
                        Ok(()) => {
                            file = Some(tokio::fs::File::open(path).await?);
                            start_next = false;
                        }
                        Err(zmodem2::Error::Backpressure) => {}
                        Err(e) => return Err(zerr(e)),
                    }
                } else {
                    finishing = true;
                }
            }
            if finishing && start_next {
                match tx.finish() {
                    Ok(()) => start_next = false,
                    Err(zmodem2::Error::Backpressure) => {}
                    Err(e) => return Err(zerr(e)),
                }
            }
            match tx.poll() {
                Action::WriteWire(bytes) => {
                    let n = bytes.len();
                    let bytes = bytes.to_vec();
                    write_wire(&mut self.io, &bytes).await?;
                    tx.wire_written(n);
                }
                Action::ReadFile { offset, max_len } => {
                    let f = file.as_mut().ok_or_else(|| io::Error::other("zmodem: no open file"))?;
                    f.seek(io::SeekFrom::Start(offset.get() as u64)).await?;
                    let want = max_len.min(buf.len());
                    let n = f.read(&mut buf[..want]).await?;
                    tx.submit_file(&buf[..n]).map_err(zerr)?;
                }
                Action::Event(Event::FileCompleted) => {
                    done[cur] = true;
                    cur += 1;
                    file = None;
                    start_next = true;
                }
                // The machine may still have bytes to send (the closing
                // "OO"); keep polling until it goes idle.
                Action::Event(Event::SessionCompleted | Event::Aborted) => ended = true,
                Action::Idle if ended => break,
                Action::Idle => {
                    if !pending.is_empty() {
                        let used = tx.submit_wire(&pending).map_err(zerr)?;
                        pending.drain(..used.min(pending.len()));
                        if used > 0 {
                            continue;
                        }
                    }
                    match tokio::time::timeout(POLL, self.io.read(&mut buf)).await {
                        Ok(Ok(0)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                        Ok(Ok(n)) => {
                            idle = Duration::ZERO;
                            pending.extend_from_slice(&buf[..n]);
                        }
                        Ok(Err(e)) => return Err(e),
                        Err(_) => {
                            idle += POLL;
                            if idle >= STALL {
                                tx.abort();
                            } else {
                                tx.timeout().map_err(zerr)?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(done)
    }

    async fn zmodem_receive(&mut self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let max = self.state.config.files.max_upload_bytes;
        let mut rx = zmodem2::Receiver::new().map_err(zerr)?;
        let mut got = Vec::new();
        let mut file: Option<(tokio::fs::File, PathBuf, u64)> = None;
        let mut ended = false;
        let mut pending = self.take_pending_input();
        let mut idle = Duration::ZERO;
        let mut buf = vec![0u8; 8192];
        loop {
            match rx.poll() {
                Action::WriteWire(bytes) => {
                    let n = bytes.len();
                    let bytes = bytes.to_vec();
                    write_wire(&mut self.io, &bytes).await?;
                    rx.wire_written(n);
                }
                Action::Event(Event::FileStarted(info)) => {
                    let name = safe_name(info.name).unwrap_or_else(|| "upload.bin".into());
                    let too_big = max > 0 && info.size.is_some_and(|s| s.get() as u64 > max);
                    let path = unique_path(dir, &name);
                    if too_big {
                        tracing::info!("{}: upload {name} too large", self.user.handle);
                        rx.abort().map_err(zerr)?;
                        continue;
                    }
                    file = Some((tokio::fs::File::create(&path).await?, path, 0));
                }
                Action::WriteFile(bytes) => {
                    let n = bytes.len();
                    let bytes = bytes.to_vec();
                    if let Some((f, _, written)) = file.as_mut() {
                        f.write_all(&bytes).await?;
                        *written += n as u64;
                        if max > 0 && *written > max {
                            rx.abort().map_err(zerr)?;
                            continue;
                        }
                    }
                    rx.file_written(n).map_err(zerr)?;
                }
                Action::Event(Event::FileCompleted) => {
                    if let Some((mut f, path, _)) = file.take() {
                        f.flush().await?;
                        got.push(path);
                    }
                }
                // Keep polling until idle: the reply to the sender's ZFIN
                // is still queued when the session reports completion.
                Action::Event(Event::SessionCompleted) => ended = true,
                Action::Event(Event::Aborted) => {
                    // A partial file isn't an upload.
                    if let Some((_, path, _)) = file.take() {
                        let _ = tokio::fs::remove_file(path).await;
                    }
                    ended = true;
                }
                Action::Idle if ended => break,
                Action::Idle => {
                    if !pending.is_empty() {
                        let used = rx.submit_wire(&pending).map_err(zerr)?;
                        pending.drain(..used.min(pending.len()));
                        if used > 0 {
                            continue;
                        }
                    }
                    match tokio::time::timeout(POLL, self.io.read(&mut buf)).await {
                        Ok(Ok(0)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                        Ok(Ok(n)) => {
                            idle = Duration::ZERO;
                            pending.extend_from_slice(&buf[..n]);
                        }
                        Ok(Err(e)) => return Err(e),
                        Err(_) => {
                            idle += POLL;
                            if idle >= STALL {
                                rx.abort().map_err(zerr)?;
                            } else {
                                rx.timeout().map_err(zerr)?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(got)
    }

    /// Run an external protocol program wired to the caller.
    async fn run_external(&mut self, cmd: &str, dir: &Path) -> io::Result<bool> {
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut cin = child.stdin.take().expect("piped stdin");
        let mut cout = child.stdout.take().expect("piped stdout");
        let pending = self.take_pending_input();
        cin.write_all(&pending).await?;
        let (mut rd, mut wr) = tokio::io::split(&mut self.io);
        let to_child = async {
            let _ = tokio::io::copy(&mut rd, &mut cin).await;
        };
        let from_child = async {
            let _ = tokio::io::copy(&mut cout, &mut wr).await;
            let _ = wr.flush().await;
            child.wait().await
        };
        let status = tokio::select! {
            s = from_child => s?,
            _ = to_child => {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        };
        Ok(status.success())
    }
}

/// `dir/name`, or `dir/name.1` etc. if that exists.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    (1..).map(|i| dir.join(format!("{name}.{i}"))).find(|p| !p.exists()).unwrap()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_names_are_sanitised() {
        assert_eq!(safe_name(b"../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_name(b"C:\\DOS\\IQ26.ZIP").as_deref(), Some("IQ26.ZIP"));
        assert_eq!(safe_name(b"..hidden").as_deref(), Some("hidden"));
        assert_eq!(safe_name(b"a b;c").as_deref(), Some("a_b_c"));
        assert_eq!(safe_name(b"../"), None);
    }
}
