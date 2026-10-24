//! Doors (`DOORS.PAS`): write a drop file, then run an outside program
//! with the caller's connection as its terminal.
//!
//! The original changed to `pathDoor`, wrote the drop file there and
//! shelled out with the FOSSIL released. Here each node gets its own drop
//! directory (so two callers in doors don't share one DOOR.SYS) and the
//! door runs on a pseudo-terminal, so ordinary Unix programs, curses
//! ones included, work as doors. Output is CP437, as door output always
//! was, and goes through the same translation as the board's own screens.
//!
//! The drop file writers in the source are partly corrupt (the start of
//! DORINFO1.DEF, the middle of DOOR.SYS and the end of CHAIN.TXT are lost
//! under a repeated block); the missing lines follow the published
//! formats, which the surviving lines match.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self as sync_mpsc, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use chrono::{NaiveDate, Timelike};
use fluxbbs_dos::batch::Batch;
use fluxbbs_dos::machine::{Machine, Status, TICK_INSTRUCTIONS};
use fluxbbs_term::{Charset, cp437};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use crate::config::Dos;
use crate::editor::strip_codes;
use crate::extprog::shell_quote;
use crate::session::Session;
use crate::user::fmt_date;

/// What the drop files say about the line. There's no modem: doors see a
/// remote caller on COM1 at a fixed rate.
const BAUD: &str = "38400";
const COM: &str = "1";

/// Every drop file we write, cleared before each door so a door never
/// reads one left from an earlier call.
const DROP_FILES: &[&str] = &["DORINFO1.DEF", "DOOR.SYS", "CHAIN.TXT", "DOOR32.SYS"];

/// Everything a drop file needs, taken from the session.
#[derive(Debug, Clone, Default)]
pub struct DropInfo {
    pub bbs_name: String,
    pub sysop_name: String,
    pub node: usize,
    pub user_id: u32,
    pub handle: String,
    pub real_name: String,
    pub location: String,
    pub phone: String,
    pub age: u32,
    pub sex: char,
    pub sl: u8,
    pub sysop: bool,
    pub cosysop: bool,
    pub ansi: bool,
    pub expert: bool,
    pub page_length: u16,
    pub total_calls: u32,
    pub last_call: Option<NaiveDate>,
    pub seconds_left: u32,
    /// Seconds after midnight the call started, and seconds used so far.
    pub logon_secs: u32,
    pub used_secs: u32,
    pub uploads: u32,
    pub downloads: u32,
    pub upload_kb: u64,
    pub download_kb: u64,
    pub today_dl_kb: u32,
    pub limit_dl_kb: u32,
    /// The node's drop directory, with a trailing slash.
    pub dir: String,
}

impl DropInfo {
    fn minutes_left(&self) -> u32 {
        self.seconds_left / 60
    }
}

/// First word, and the rest.
fn split_name(s: &str) -> (&str, &str) {
    s.split_once(' ').unwrap_or((s, ""))
}

fn bit(b: bool) -> &'static str {
    if b { "1" } else { "0" }
}

/// CP437 lines with CRLF, as DOS doors expect.
fn lines(v: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for l in v {
        out.extend(cp437::encode(l));
        out.extend(b"\r\n");
    }
    out
}

/// `doorCreateDorInfo`: DORINFO1.DEF (RBBS/QuickBBS).
pub fn dorinfo(d: &DropInfo) -> Vec<u8> {
    let (sf, sl) = split_name(&d.sysop_name);
    let (uf, ul) = split_name(&d.handle);
    let mins = d.minutes_left().min(999);
    lines(&[
        d.bbs_name.clone(),
        sf.into(),
        sl.into(),
        format!("COM{COM}"),
        format!("{BAUD} BAUD,N,8,1"),
        "0".into(),
        uf.into(),
        ul.into(),
        d.location.clone(),
        bit(d.ansi).into(),
        d.sl.to_string(),
        mins.to_string(),
        "0".into(),
    ])
}

/// "(902)555-1212" -> "902 555-1212", as the original wrote it.
fn door_phone(p: &str) -> String {
    match (p.get(1..4), p.get(5..)) {
        (Some(area), Some(rest)) if p.starts_with('(') => format!("{area} {rest}"),
        _ => p.into(),
    }
}

/// `doorCreateDoorSys`: DOOR.SYS (GAP, 31 lines). The password line is
/// left empty: passwords are only kept hashed, and a door has no business
/// with one anyway.
pub fn door_sys(d: &DropInfo) -> Vec<u8> {
    let yn = |b: bool| if b { "Y" } else { "N" }.to_string();
    let phone = door_phone(&d.phone);
    lines(&[
        format!("COM{COM}:"),
        BAUD.into(),
        "8".into(),
        d.node.to_string(),
        BAUD.into(),
        "Y".into(),
        "N".into(),
        "Y".into(),
        "Y".into(),
        d.handle.clone(),
        d.location.clone(),
        phone.clone(),
        phone,
        String::new(),
        d.sl.to_string(),
        d.total_calls.to_string(),
        fmt_date(d.last_call),
        d.seconds_left.to_string(),
        d.minutes_left().to_string(),
        if d.ansi { "GR" } else { "NG" }.into(),
        d.page_length.to_string(),
        yn(d.expert),
        "1234567".into(),
        "7".into(),
        "01/01/99".into(),
        d.user_id.to_string(),
        "Z".into(),
        d.uploads.to_string(),
        d.downloads.to_string(),
        d.today_dl_kb.to_string(),
        d.limit_dl_kb.to_string(),
    ])
}

/// `doorCreateChainTxt`: CHAIN.TXT (WWIV). The original had placeholders
/// for the times and transfer totals; these are the real values. The
/// gfiles and data directories are the drop directory, not the board's
/// own data.
pub fn chain_txt(d: &DropInfo) -> Vec<u8> {
    lines(&[
        d.user_id.to_string(),
        d.handle.clone(),
        d.real_name.clone(),
        String::new(),
        d.age.to_string(),
        d.sex.to_string(),
        "0.00".into(),
        fmt_date(d.last_call),
        "80".into(),
        d.page_length.to_string(),
        d.sl.to_string(),
        bit(d.sysop).into(),
        bit(d.cosysop).into(),
        bit(d.ansi).into(),
        "1".into(),
        d.seconds_left.to_string(),
        d.dir.clone(),
        d.dir.clone(),
        "XX.XX".into(),
        BAUD.into(),
        COM.into(),
        d.bbs_name.clone(),
        d.sysop_name.clone(),
        d.logon_secs.to_string(),
        d.used_secs.to_string(),
        d.upload_kb.to_string(),
        d.uploads.to_string(),
        d.download_kb.to_string(),
        d.downloads.to_string(),
        "8N1".into(),
        BAUD.into(),
        "0".into(),
    ])
}

/// DOOR32.SYS (not in the original; what newer doors read). Comm type 0:
/// the door talks to its terminal, there's no socket to hand it.
pub fn door32_sys(d: &DropInfo) -> Vec<u8> {
    lines(&[
        "0".into(),
        "0".into(),
        BAUD.into(),
        "flux/bbs".into(),
        d.user_id.to_string(),
        d.real_name.clone(),
        d.handle.clone(),
        d.sl.to_string(),
        d.minutes_left().to_string(),
        bit(d.ansi).into(),
        d.node.to_string(),
    ])
}

type Writer = fn(&DropInfo) -> Vec<u8>;

/// The drop files for a `Dx` command.
fn drop_files(kind: char) -> &'static [(&'static str, Writer)] {
    match kind.to_ascii_uppercase() {
        '1' => &[("DORINFO1.DEF", dorinfo)],
        '2' | '7' => &[("DOOR.SYS", door_sys)],
        '3' => &[("CHAIN.TXT", chain_txt)],
        '4' => &[("DOOR32.SYS", door32_sys)],
        'Z' => &[("DOOR.SYS", door_sys), ("DORINFO1.DEF", dorinfo)],
        _ => &[],
    }
}

/// Caller input on its way to a door: one Enter per `\r` whatever the
/// client sends, and UTF-8 turned back into CP437 for UTF-8 callers.
pub(crate) struct DoorInput {
    utf8: bool,
    last_cr: bool,
    carry: Vec<u8>,
}

impl DoorInput {
    pub(crate) fn new(utf8: bool) -> Self {
        Self { utf8, last_cr: false, carry: Vec::new() }
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        for &b in bytes {
            let after_cr = std::mem::replace(&mut self.last_cr, b == b'\r');
            if !(after_cr && (b == b'\n' || b == 0)) {
                self.carry.push(b);
            }
        }
        if !self.utf8 {
            return std::mem::take(&mut self.carry);
        }
        let mut out = Vec::new();
        loop {
            match std::str::from_utf8(&self.carry) {
                Ok(s) => {
                    out.extend(cp437::encode(s));
                    self.carry.clear();
                    return out;
                }
                Err(e) => {
                    let good = e.valid_up_to();
                    out.extend(cp437::encode(std::str::from_utf8(&self.carry[..good]).expect("valid prefix")));
                    match e.error_len() {
                        Some(bad) => {
                            out.extend_from_slice(&self.carry[good..good + bad]);
                            self.carry.drain(..good + bad);
                        }
                        // An incomplete character: wait for the rest.
                        None => {
                            self.carry.drain(..good);
                            return out;
                        }
                    }
                }
            }
        }
    }
}

/// The controlling side of a pseudo-terminal.
struct Pty(AsyncFd<OwnedFd>);

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r) }
}

impl Pty {
    /// A new pty: the controller, and the terminal end for the door.
    fn open(cols: u16, rows: u16) -> io::Result<(Self, OwnedFd)> {
        let (mut m, mut s) = (0, 0);
        let mut ws = winsize(cols, rows);
        cvt(unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::addr_of_mut!(ws)) })?;
        let (m, s) = unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) };
        for fd in [&m, &s] {
            cvt(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) })?;
        }
        let fl = cvt(unsafe { libc::fcntl(m.as_raw_fd(), libc::F_GETFL) })?;
        cvt(unsafe { libc::fcntl(m.as_raw_fd(), libc::F_SETFL, fl | libc::O_NONBLOCK) })?;
        // SAFETY: we own `m` and nothing else closes or replaces it.
        Ok((Self(unsafe { AsyncFd::register(m)? }), s))
    }

    fn resize(&self, cols: u16, rows: u16) {
        let ws = winsize(cols, rows);
        unsafe { libc::ioctl(self.0.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
    }

    /// Read what the door wrote; 0 once nothing has the terminal open.
    async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.0.readable().await?;
            let r = guard.try_io(|fd| {
                let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
            });
            match r {
                // Linux reports a closed terminal end as EIO.
                Ok(Err(e)) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
                Ok(r) => return r,
                Err(_would_block) => continue,
            }
        }
    }

    async fn write_all(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            let mut guard = self.0.writable().await?;
            let r = guard.try_io(|fd| {
                let n = unsafe { libc::write(fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
                if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
            });
            match r {
                Ok(Ok(n)) => buf = &buf[n..],
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
        Ok(())
    }
}

/// Signal the door's whole process group.
fn signal_group(pid: Option<u32>, sig: libc::c_int) {
    if let Some(pid) = pid {
        unsafe { libc::kill(-(pid as libc::pid_t), sig) };
    }
}

/// Is `cmd` a DOS door: does the program it names end in .BAT, .COM or
/// .EXE? The stock menus' doors (`LORD.BAT`) are.
pub fn is_dos(cmd: &str) -> bool {
    let prog = cmd.split_whitespace().next().unwrap_or("");
    let ext = prog.rsplit_once('.').map(|(_, e)| e.to_ascii_uppercase());
    !prog.contains('/') && matches!(ext.as_deref(), Some("BAT" | "COM" | "EXE"))
}

/// A value for a DOS command line: printable ASCII without the characters
/// COMMAND.COM treats specially.
fn dos_safe(s: &str) -> String {
    s.chars().filter(|c| (' '..='~').contains(c) && !"<>|\"%".contains(*c)).collect()
}

/// Is a bare program name (no drive or directory) on the DOS drive?
fn dos_program_found(drive: &Path, cmd: &str) -> bool {
    let prog = cmd.split_whitespace().next().unwrap_or("");
    if prog.contains([':', '\\']) {
        return true;
    }
    std::fs::read_dir(drive)
        .map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(prog)))
        .unwrap_or(false)
}

/// The DOSBox config for one DOS door: COM1 is a null modem to `port`
/// with no control-line protocol (raw bytes, carrier always up), the DOS
/// drive is C: and the node's drop directory D:, the current drive, as
/// `pathDoor` was the original's current directory.
pub fn dosbox_conf(dos: &Dos, port: u16, drive: &Path, node_dir: &Path, cmd: &str) -> String {
    let mut s = String::new();
    for l in &dos.config {
        s.push_str(l);
        s.push('\n');
    }
    s.push_str(&format!("\n[serial]\nserial1=nullmodem server:127.0.0.1 port:{port} transparent:1\n\n[autoexec]\n"));
    s.push_str("@echo off\n");
    s.push_str(&format!("mount c \"{}\"\n", drive.display()));
    s.push_str(&format!("mount d \"{}\"\n", node_dir.display()));
    // Z: stays on the PATH: CHOICE and the other DOSBox tools live there.
    s.push_str("d:\npath z:\\;c:\\\n");
    for l in &dos.setup {
        s.push_str(l);
        s.push('\n');
    }
    let prog = cmd.split_whitespace().next().unwrap_or("");
    let call = if prog.to_ascii_uppercase().ends_with(".BAT") { "call " } else { "" };
    s.push_str(&format!("{call}{cmd}\n"));
    // DOSBox drops what the UART hasn't sent yet when it quits: give the
    // door's last screen time to go out.
    s.push_str("choice /c:y /t:y,2 > nul\nexit\n");
    s
}

/// The door's end of the connection.
enum Line {
    Pty(Pty),
    /// DOSBox's COM1.
    Com(tokio::net::TcpStream),
    /// The built-in DOS machine, on its thread.
    Dos(Builtin),
}

/// Our side of a door in the built-in machine: bytes to it, bytes from
/// it. Dropping the sender tells the machine the caller has gone.
struct Builtin {
    to_door: sync_mpsc::Sender<Vec<u8>>,
    from_door: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Line {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Line::Pty(p) => p.read(buf).await,
            Line::Com(s) => s.read(buf).await,
            Line::Dos(b) => {
                if b.pending.is_empty() {
                    match b.from_door.recv().await {
                        Some(v) => b.pending = v,
                        None => return Ok(0),
                    }
                }
                let n = buf.len().min(b.pending.len());
                buf[..n].copy_from_slice(&b.pending[..n]);
                b.pending.drain(..n);
                Ok(n)
            }
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            Line::Pty(p) => p.write_all(buf).await,
            Line::Com(s) => s.write_all(buf).await,
            Line::Dos(b) => {
                // The machine gone is seen on the read side.
                let _ = b.to_door.send(buf.to_vec());
                Ok(())
            }
        }
    }

    fn resize(&self, cols: u16, rows: u16) {
        if let Line::Pty(p) = self {
            p.resize(cols, rows);
        }
    }

    /// What tells the door the caller has gone. DOSBox doesn't drop
    /// carrier when its socket closes, so DOS doors are stopped instead.
    fn hangup_signal(&self) -> libc::c_int {
        match self {
            Line::Pty(_) | Line::Dos(_) => libc::SIGHUP,
            Line::Com(_) => libc::SIGTERM,
        }
    }
}

/// The door itself: a process of ours, or the machine thread.
enum Proc {
    Child(Child),
    Machine {
        task: tokio::task::JoinHandle<Result<MachineEnd, String>>,
        /// Its result, once in: a join handle isn't polled twice.
        done: Option<io::Result<ExitStatus>>,
        /// The door dropped DTR: the caller is to be hung up.
        hangup: Arc<AtomicBool>,
    },
}

impl Proc {
    fn id(&self) -> Option<u32> {
        match self {
            Proc::Child(c) => c.id(),
            Proc::Machine { .. } => None,
        }
    }

    /// Wait for the door to end; a machine door's exit code becomes a
    /// status like a process's.
    async fn wait(&mut self) -> io::Result<ExitStatus> {
        match self {
            Proc::Child(c) => c.wait().await,
            Proc::Machine { task, done, hangup } => {
                if done.is_none() {
                    let r = match task.await {
                        Ok(Ok(MachineEnd::Exited(code))) => Ok(ExitStatus::from_raw((code as i32) << 8)),
                        Ok(Ok(MachineEnd::Hangup)) => {
                            hangup.store(true, Ordering::Relaxed);
                            Ok(ExitStatus::from_raw(0))
                        }
                        Ok(Err(e)) => Err(io::Error::other(e)),
                        Err(e) => Err(io::Error::other(e)),
                    };
                    *done = Some(r);
                }
                match done {
                    Some(Ok(st)) => Ok(*st),
                    Some(Err(e)) => Err(io::Error::new(e.kind(), e.to_string())),
                    None => unreachable!(),
                }
            }
        }
    }
}

/// How a door in the built-in machine ended.
enum MachineEnd {
    Exited(u8),
    Hangup,
}

/// Run `cmd` (a program or batch file) in the built-in DOS machine with
/// `drive` as C: and the drop directory `dir` as D:, until it ends or
/// the caller goes. The machine's clock follows real time: it sleeps
/// when it gets ahead of 18.2 ticks a second and takes ticks in a lump
/// when it has been waiting for input.
fn run_machine(drive: PathBuf, dir: PathBuf, cmd: String, from_caller: sync_mpsc::Receiver<Vec<u8>>, to_caller: tokio::sync::mpsc::Sender<Vec<u8>>) -> Result<MachineEnd, String> {
    const TICK: Duration = Duration::from_micros(54_925);
    /// How long a door gets to notice carrier has dropped.
    const GRACE: Duration = Duration::from_secs(5);
    let mut m = Machine::new(&drive, Some(&dir));
    m.sys.dos.set_drive(3);
    let mut batch = Batch::new(&mut m, &cmd)?;
    let started = Instant::now();
    let mut lost: Option<Instant> = None;
    let mut code = 0;
    // Output for the caller. One whose side stopped taking it (a dead link
    // that never closed) mustn't hold the door, and its locks on files
    // other nodes share, for as long as TCP takes to give up: after a
    // minute carrier drops, as for a hangup, and the door has its grace to
    // save and go. What it writes after that goes nowhere.
    let send = |m: &mut Machine, lost: &mut Option<Instant>| {
        for l in m.sys.log.drain(..) {
            tracing::debug!("dos: {l}");
        }
        let out = m.take_output();
        if out.is_empty() || lost.is_some() {
            return;
        }
        let give_up = Instant::now() + Duration::from_secs(60);
        let mut out = out;
        loop {
            match to_caller.try_send(out) {
                Ok(()) => return,
                Err(tokio::sync::mpsc::error::TrySendError::Full(o)) if Instant::now() < give_up => {
                    out = o;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    if matches!(e, tokio::sync::mpsc::error::TrySendError::Full(_)) {
                        tracing::warn!("dos: the caller took no output for a minute; dropping carrier");
                    }
                    *lost = Some(Instant::now());
                    m.set_carrier(false);
                    return;
                }
            }
        }
    };
    while let Some((prog, args)) = batch.next(&mut m)? {
        send(&mut m, &mut lost);
        m.start(&prog, &args)?;
        loop {
            loop {
                match from_caller.try_recv() {
                    Ok(b) => m.feed(&b),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if lost.is_none() {
                            lost = Some(Instant::now());
                            m.set_carrier(false);
                        }
                        break;
                    }
                }
            }
            if lost.is_some_and(|t| t.elapsed() > GRACE) {
                return Err("the caller hung up and the door didn't notice".into());
            }
            let st = m.run(TICK_INSTRUCTIONS / 8);
            send(&mut m, &mut lost);
            if m.sys.hangup {
                return Ok(MachineEnd::Hangup);
            }
            // Keep the machine's clock on real time.
            let wall = (started.elapsed().as_nanos() / TICK.as_nanos()) as u64;
            if m.ticks + 1 < wall {
                m.advance_ticks((wall - m.ticks).min(182) as u32);
            }
            match st {
                Status::Running => {
                    let due = started + TICK * m.ticks as u32;
                    let now = Instant::now();
                    if due > now {
                        std::thread::sleep(due - now);
                    }
                }
                Status::Idle => match from_caller.recv_timeout(TICK) {
                    Ok(b) => m.feed(&b),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => std::thread::sleep(TICK),
                },
                Status::Exited(c) => {
                    code = c;
                    break;
                }
                Status::Fault(e) => return Err(format!("{prog}: {e}")),
            }
        }
    }
    send(&mut m, &mut lost);
    Ok(MachineEnd::Exited(code))
}

/// An event's `dos COMMAND`: run a DOS program or batch file in the
/// built-in machine with no caller (C: the DOS drive, D: `event` under
/// the doors directory), for a door's maintenance (TW2002's EXTERN). What
/// it prints goes to the log; after an hour it's stopped.
pub async fn run_dos_event(state: &crate::BbsState, cmd: &str) -> anyhow::Result<()> {
    let cfg = state.config();
    let drive = cfg.resolve(&cfg.dos.drive);
    let dir = cfg.resolve(&cfg.paths.doors).join("event");
    std::fs::create_dir_all(&dir)?;
    let dir = dir.canonicalize()?;
    anyhow::ensure!(dos_program_found(&drive, cmd), "no {cmd} in {}", drive.display());
    let (line, from_caller) = sync_mpsc::channel::<Vec<u8>>();
    let (to_caller, mut out) = tokio::sync::mpsc::channel(64);
    let cmd = cmd.to_string();
    let task = tokio::task::spawn_blocking(move || run_machine(drive, dir, cmd, from_caller, to_caller));
    let mut text = Vec::new();
    let read = async {
        while let Some(b) = out.recv().await {
            text.extend(b);
        }
    };
    let timed_out = tokio::time::timeout(Duration::from_secs(3600), read).await.is_err();
    // No caller: dropping the line drops carrier, which ends a program
    // that ran too long.
    drop(line);
    let end = task.await?;
    for l in plain_text(&text).lines().map(str::trim_end).filter(|l| !l.is_empty()) {
        tracing::info!("event: | {l}");
    }
    anyhow::ensure!(!timed_out, "DOS command still running after an hour; stopped");
    match end {
        Ok(MachineEnd::Exited(0)) => Ok(()),
        Ok(MachineEnd::Exited(c)) => anyhow::bail!("exited with {c}"),
        Ok(MachineEnd::Hangup) => anyhow::bail!("hung up"),
        Err(e) => anyhow::bail!(e),
    }
}

/// A DOS program's output as lines of text: escape sequences go, and a
/// cursor move starts a new line.
fn plain_text(b: &[u8]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1B && b.get(i + 1) == Some(&b'[') {
            let mut j = i + 2;
            while j < b.len() && !b[j].is_ascii_alphabetic() {
                j += 1;
            }
            if matches!(b.get(j), Some(b'H' | b'f')) {
                out.push(b'\n');
            }
            i = j + 1;
        } else {
            if b[i] != b'\r' {
                out.push(b[i]);
            }
            i += 1;
        }
    }
    // Line by line: CP437 has glyphs for the control characters.
    out.split(|&c| c == b'\n').map(fluxbbs_term::cp437::decode).collect::<Vec<_>>().join("\n")
}

/// Start a DOS door in the built-in machine.
fn spawn_builtin(cmd: &str, dir: &Path, drive: &Path) -> (Line, Proc) {
    let (to_door, from_caller) = sync_mpsc::channel();
    let (to_caller, from_door) = tokio::sync::mpsc::channel(64);
    let (drive, dir, cmd) = (drive.to_path_buf(), dir.to_path_buf(), cmd.to_string());
    let task = tokio::task::spawn_blocking(move || run_machine(drive, dir, cmd, from_caller, to_caller));
    (Line::Dos(Builtin { to_door, from_door, pending: Vec::new() }), Proc::Machine { task, done: None, hangup: Arc::new(AtomicBool::new(false)) })
}

/// Start a DOS door under DOSBox, with COM1 connected back to us.
/// Its window is hidden and what it prints goes to `dosbox.log` in the
/// drop directory.
async fn spawn_dos(dos: &Dos, cmd: &str, dir: &Path, drive: &Path) -> io::Result<(Line, Child)> {
    let drive = drive.canonicalize()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let conf = dir.join("dosbox.conf");
    std::fs::write(&conf, dosbox_conf(dos, port, &drive, dir, cmd))?;
    let log = std::fs::File::create(dir.join("dosbox.log"))?;
    let emulator = dos.emulator.replace("%C", &shell_quote(&conf.to_string_lossy()));
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&emulator)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .process_group(0)
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id();
    let failed = |why: String| {
        signal_group(pid, libc::SIGKILL);
        io::Error::other(format!("{why} (see {})", dir.join("dosbox.log").display()))
    };
    tokio::select! {
        r = listener.accept() => {
            let (sock, _) = r?;
            sock.set_nodelay(true)?;
            Ok((Line::Com(sock), child))
        }
        st = child.wait() => Err(failed(format!("`{emulator}` exited with {}", st?))),
        _ = tokio::time::sleep(Duration::from_secs(20)) => Err(failed(format!("`{emulator}` didn't open COM1"))),
    }
}

/// How a door run ended.
enum DoorEnd {
    Exited(ExitStatus),
    TimeUp,
    Hangup,
}

/// Door screens are 80 columns.
const COLS: u16 = 80;

impl Session {
    /// `doorExecute` (`D0`-`D4`, `DZ`): write the drop file `kind` asks
    /// for and run `cmd`, natively or, for a DOS program, under DOSBox.
    pub async fn door_execute(&mut self, cmd: &str, kind: char) -> io::Result<()> {
        // `X:\DOOR.EXT;DESCRIPTION` (2.0): the description is what other
        // callers see the caller doing (a path after `;` is a shell command
        // going on); without one, the menu item's, or the program's name.
        let (cmd, name) = match cmd.rsplit_once(';') {
            Some((c, d)) if !d.trim().is_empty() && !d.contains(['/', '\\']) => (c, d.trim().to_string()),
            _ if !self.menu.cmd_desc.trim().is_empty() => (cmd, self.menu.cmd_desc.trim().to_string()),
            _ => (cmd, door_name(cmd)),
        };
        if cmd.trim().is_empty() {
            return Ok(());
        }
        tracing::info!("{}: online door game execution: {cmd}", self.user.handle);
        self.seen.doors.insert(name.clone());
        let cfg = self.state.config();
        let dos = is_dos(cmd);
        let drive = cfg.resolve(&cfg.dos.drive);
        if dos && !dos_program_found(&drive, cmd) {
            tracing::warn!("DOS door not found in {}: {cmd}", drive.display());
            return self.note_say("DoorFailed").await;
        }
        let _claim = if cfg.dos.one_at_a_time.iter().any(|d| door_name(d).eq_ignore_ascii_case(&door_name(cmd))) {
            match DoorClaim::take(&self.state, door_name(cmd).to_ascii_uppercase()) {
                Some(c) => Some(c),
                None => {
                    return self.note_say("DoorInUse").await;
                }
            }
        } else {
            None
        };
        let dir = cfg.resolve(&cfg.paths.doors).join(format!("node{}", self.node.number()));
        let prepared = (|| {
            std::fs::create_dir_all(&dir)?;
            let dir = dir.canonicalize()?;
            for name in DROP_FILES {
                let _ = std::fs::remove_file(dir.join(name));
            }
            let shown = if dos { "D:\\".to_string() } else { format!("{}/", dir.display()) };
            let info = self.drop_info(shown);
            for (name, write) in drop_files(kind) {
                std::fs::write(dir.join(name), write(&info))?;
            }
            io::Result::Ok(dir)
        })();
        let dir = match prepared {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("door drop directory {}: {e}", dir.display());
                return self.note_say("DoorFailed").await;
            }
        };
        self.save_user();
        let cmd = self.door_command(cmd, &dir, dos);
        self.node.update(|n| n.activity = format!("Running {name}"));
        self.flush().await?;
        let started = crate::clock::now().naive_local();
        let spawned = if dos && cfg.dos.emulator.is_empty() {
            Ok(spawn_builtin(&cmd, &dir, &drive))
        } else if dos {
            spawn_dos(&cfg.dos, &cmd, &dir, &drive).await.map(|(l, c)| (l, Proc::Child(c)))
        } else {
            self.spawn_native(&cmd, &dir).map(|(l, c)| (l, Proc::Child(c)))
        };
        let result = match spawned {
            Ok((line, proc)) => self.run_door(line, proc).await,
            Err(e) => Err(e),
        };
        // A program the shell couldn't find wasn't played.
        if let Ok(end) = &result
            && !matches!(end, DoorEnd::Exited(st) if st.code() == Some(127))
        {
            self.record_play(&name, started);
        }
        self.input.clear();
        self.screen.resync();
        // New: the game had the screen; nothing of it to keep, so the
        // menu comes back without a pause.
        self.full_screen_done();
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        if kind == '7' {
            self.door_readback(&dir);
        }
        match result {
            // The shell couldn't find it.
            Ok(DoorEnd::Exited(st)) if st.code() == Some(127) => {
                tracing::warn!("door not found: {cmd}");
                self.note_say("DoorFailed").await?;
            }
            Ok(DoorEnd::Exited(st)) if !st.success() => tracing::info!("door exited with {st}: {cmd}"),
            Ok(DoorEnd::TimeUp) => tracing::info!("{}: out of time in a door", self.user.handle),
            Ok(DoorEnd::Hangup) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(_) => {}
            Err(e) => {
                tracing::error!("running door {cmd}: {e}");
                self.note_say("DoorFailed").await?;
            }
        }
        Ok(())
    }

    /// `D7` (2.0): what the door changed in DOOR.SYS comes back: time
    /// left (up to the day's allowance) and uploads, downloads and today's
    /// download kbytes (only upwards). Not the security level: a door has
    /// no business raising that.
    fn door_readback(&mut self, dir: &Path) {
        let Ok(data) = std::fs::read(dir.join("DOOR.SYS")) else { return };
        let text = fluxbbs_term::cp437::decode(&data);
        let rb = readback(&text);
        let u = &mut self.user;
        if let Some(m) = rb.minutes_left {
            let used = (self.time_on.elapsed().as_secs() / 60) as u32;
            u.time_today = (m + used).min(u.time_per_day.max(u.time_today));
        }
        if let Some(n) = rb.uploads {
            u.uploads = u.uploads.max(n);
        }
        if let Some(n) = rb.downloads {
            u.downloads = u.downloads.max(n);
        }
        if let Some(n) = rb.today_dl_kb {
            u.today_dl_kb = u.today_dl_kb.max(n);
        }
        self.save_user();
    }

    /// `dir` is the drop directory as the door sees it.
    fn drop_info(&self, dir: String) -> DropInfo {
        let cfg = self.state.config();
        let u = &self.user;
        let now = crate::clock::now();
        let used = self.time_on.elapsed().as_secs() as u32;
        let start = now - chrono::Duration::seconds(used as i64);
        let sysop_name = if cfg.bbs.sysop_name.is_empty() { &cfg.bbs.sysop_alias } else { &cfg.bbs.sysop_name };
        DropInfo {
            bbs_name: strip_codes(&cfg.bbs.name),
            sysop_name: sysop_name.clone(),
            node: self.node.number(),
            user_id: u.id,
            handle: u.handle.clone(),
            real_name: u.real_name.clone(),
            location: u.location.clone(),
            phone: u.phone.clone(),
            age: u.age(now.date_naive()).unwrap_or(0),
            sex: u.sex,
            sl: u.sl,
            sysop: self.is_sysop(),
            cosysop: self.acs(&cfg.acs.cosysop),
            ansi: true,
            expert: u.expert,
            page_length: u.page_length,
            total_calls: u.total_calls,
            last_call: u.last_call,
            seconds_left: (u.time_today * 60).saturating_sub(used),
            logon_secs: start.num_seconds_from_midnight(),
            used_secs: used,
            uploads: u.uploads,
            downloads: u.downloads,
            upload_kb: u.upload_kb,
            download_kb: u.download_kb,
            today_dl_kb: u.today_dl_kb,
            limit_dl_kb: u.limit_dl_kb,
            dir,
        }
    }

    /// The door command with its codes filled in: the original's `%PD`
    /// (data), `%PT` (text), `%PM` (menus), `%BR`/`%BP` (baud) and `%CP`
    /// (COM port), `%PN` for the node's drop directory, then any MCI code
    /// (`%UN`, `%ND`, `%TL`...). For a native door each value goes in
    /// shell-quoted, so a handle can't run commands; for a DOS door it
    /// loses the characters DOS would act on, and `%PN` is `D:\`.
    fn door_command(&self, cmd: &str, dir: &Path, dos: bool) -> String {
        let cfg = self.state.config();
        let path = |p: &Path| cfg.resolve(p).to_string_lossy().into_owned();
        let chars: Vec<char> = cmd.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '%' && i + 2 < chars.len() && chars[i + 1].is_ascii() && chars[i + 2].is_ascii() {
                let code = [chars[i + 1].to_ascii_uppercase() as u8, chars[i + 2].to_ascii_uppercase() as u8];
                let value = match &code {
                    b"PD" => Some(path(&cfg.paths.data)),
                    b"PT" => Some(path(&cfg.paths.text)),
                    b"PM" => Some(path(&cfg.paths.menus)),
                    b"PN" if dos => Some("D:\\".into()),
                    b"PN" => Some(dir.to_string_lossy().into_owned()),
                    b"BR" | b"BP" => Some(BAUD.into()),
                    b"CP" => Some(COM.into()),
                    _ => self.mci_value(code),
                };
                if let Some(v) = value {
                    out.push_str(&if dos { dos_safe(&v) } else { shell_quote(&v) });
                    i += 3;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    /// Start a native door on a pty: 80 columns, the caller's rows and
    /// `TERM`.
    fn spawn_native(&self, cmd: &str, dir: &Path) -> io::Result<(Line, Child)> {
        let rows = self.screen.rows();
        let (pty, tty) = Pty::open(COLS, rows)?;
        let term = if self.caller.term.is_empty() { "ansi".to_string() } else { self.caller.term.clone() };
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .stdin(Stdio::from(tty.try_clone()?))
            .stdout(Stdio::from(tty.try_clone()?))
            .stderr(Stdio::from(tty))
            .env("TERM", term)
            .env("COLUMNS", COLS.to_string())
            .env("LINES", rows.to_string())
            .kill_on_drop(true);
        // A session of its own, with the pty as its controlling terminal,
        // so the door gets SIGHUP when the caller drops.
        unsafe {
            command.pre_exec(|| {
                cvt(libc::setsid())?;
                cvt(libc::ioctl(0, libc::TIOCSCTTY as _, 0))?;
                Ok(())
            });
        }
        let child = command.spawn()?;
        // `command` holds our copies of the terminal end; dropping it on
        // return lets us see the door close it.
        Ok((Line::Pty(pty), child))
    }

    /// Pass bytes between the caller and a door until it exits, the caller
    /// hangs up, or their time runs out.
    async fn run_door(&mut self, mut line: Line, mut child: Proc) -> io::Result<DoorEnd> {
        let pid = child.id();
        let mut input = DoorInput::new(self.screen.charset() == Charset::Utf8);
        let pending = self.take_pending_input();
        line.write_all(&input.feed(&pending)).await?;
        let mut size = self.caller.size.clone();
        let mut size_open = true;
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        let mut obuf = [0u8; 4096];
        let mut ibuf = [0u8; 1024];
        let end = loop {
            tokio::select! {
                r = line.read(&mut obuf) => {
                    let n = r?;
                    if n == 0 {
                        break DoorEnd::Exited(child.wait().await?);
                    }
                    self.screen.write_bytes(&obuf[..n]);
                    self.flush().await?;
                }
                r = self.io.read(&mut ibuf) => {
                    match r {
                        Ok(n) if n > 0 => line.write_all(&input.feed(&ibuf[..n])).await?,
                        _ => break DoorEnd::Hangup,
                    }
                }
                st = child.wait() => {
                    let st = st?;
                    // Whatever it wrote last, unless something it left
                    // behind still holds the terminal.
                    while let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(200), line.read(&mut obuf)).await {
                        if n == 0 {
                            break;
                        }
                        self.screen.write_bytes(&obuf[..n]);
                    }
                    self.flush().await?;
                    break DoorEnd::Exited(st);
                }
                _ = tick.tick() => {
                    if self.minutes_left() == 0 {
                        break DoorEnd::TimeUp;
                    }
                }
                r = size.changed(), if size_open => {
                    if r.is_err() {
                        size_open = false;
                    } else {
                        let rows = size.borrow_and_update().1.max(1);
                        self.screen.set_rows(rows);
                        line.resize(COLS, rows);
                    }
                }
            }
        };
        let signal = line.hangup_signal();
        drop(line);
        match &child {
            Proc::Child(_) => {
                if matches!(end, DoorEnd::Exited(_)) {
                    // Like dropping carrier on anything the door left running.
                    signal_group(pid, libc::SIGHUP);
                } else {
                    signal_group(pid, signal);
                    if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
                        signal_group(pid, libc::SIGKILL);
                        let _ = child.wait().await;
                    }
                }
            }
            Proc::Machine { .. } => {
                // The machine sees carrier drop and ends on its own.
                if !matches!(end, DoorEnd::Exited(_)) {
                    let _ = tokio::time::timeout(Duration::from_secs(8), child.wait()).await;
                }
                if let Proc::Machine { hangup, .. } = &child
                    && hangup.load(Ordering::Relaxed)
                {
                    return Ok(DoorEnd::Hangup);
                }
            }
        }
        Ok(end)
    }
}

/// What a door left in DOOR.SYS for `D7` (GAP lines 19, 28, 29, 30).
#[derive(Debug, Default, PartialEq)]
struct Readback {
    minutes_left: Option<u32>,
    uploads: Option<u32>,
    downloads: Option<u32>,
    today_dl_kb: Option<u32>,
}

fn readback(text: &str) -> Readback {
    let lines: Vec<&str> = text.lines().collect();
    let num = |i: usize| lines.get(i).and_then(|l| l.trim().parse::<u32>().ok());
    Readback { minutes_left: num(18), uploads: num(27), downloads: num(28), today_dl_kb: num(29) }
}

/// A caller's place in a `[dos] one_at_a_time` door, given up when the
/// door ends.
struct DoorClaim {
    state: std::sync::Arc<crate::BbsState>,
    name: String,
}

impl DoorClaim {
    fn take(state: &std::sync::Arc<crate::BbsState>, name: String) -> Option<Self> {
        let mut used = state.doors_in_use.lock().unwrap_or_else(|e| e.into_inner());
        used.insert(name.clone()).then(|| Self { state: state.clone(), name })
    }
}

impl Drop for DoorClaim {
    fn drop(&mut self) {
        self.state.doors_in_use.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.name);
    }
}

/// A door's program name, for the node status: `LORD.BAT 1` -> `LORD`.
fn door_name(cmd: &str) -> String {
    let prog = cmd.split_whitespace().next().unwrap_or("");
    let file = prog.rsplit(['/', '\\']).next().unwrap_or(prog);
    let stem = file.split('.').next().unwrap_or(file);
    if stem.is_empty() { "a door".into() } else { stem.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DropInfo {
        DropInfo {
            bbs_name: "Iniquity".into(),
            sysop_name: "Joe Sysop".into(),
            node: 2,
            user_id: 7,
            handle: "phrac".into(),
            real_name: "Some One".into(),
            location: "Halifax, NS".into(),
            phone: "(902)555-1212".into(),
            age: 30,
            sex: 'M',
            sl: 50,
            ansi: true,
            page_length: 24,
            total_calls: 12,
            last_call: NaiveDate::from_ymd_opt(2026, 10, 16),
            seconds_left: 3599,
            uploads: 3,
            downloads: 4,
            dir: "/bbs/doors/node2/".into(),
            ..DropInfo::default()
        }
    }

    fn text(b: Vec<u8>) -> Vec<String> {
        let s = String::from_utf8(b).unwrap();
        assert!(s.ends_with("\r\n"));
        s.trim_end_matches("\r\n").split("\r\n").map(String::from).collect()
    }

    #[test]
    fn dorinfo_lines() {
        let l = text(dorinfo(&sample()));
        assert_eq!(l.len(), 13);
        assert_eq!(&l[..5], ["Iniquity", "Joe", "Sysop", "COM1", "38400 BAUD,N,8,1"]);
        assert_eq!(&l[6..], ["phrac", "", "Halifax, NS", "1", "50", "59", "0"]);
    }

    #[test]
    fn door_sys_lines() {
        let l = text(door_sys(&sample()));
        assert_eq!(l.len(), 31);
        assert_eq!(l[0], "COM1:");
        assert_eq!(l[3], "2");
        assert_eq!(l[9], "phrac");
        assert_eq!(l[11], "902 555-1212");
        assert_eq!(l[13], "", "no password");
        assert_eq!(&l[14..20], ["50", "12", "10/16/26", "3599", "59", "GR"]);
        assert_eq!(l[25], "7");
        assert_eq!(&l[27..29], ["3", "4"]);
    }

    #[test]
    fn chain_and_door32_lines() {
        let l = text(chain_txt(&sample()));
        assert_eq!(&l[..3], ["7", "phrac", "Some One"]);
        assert_eq!(l[15], "3599");
        assert_eq!(l[16], "/bbs/doors/node2/");
        assert_eq!(l[29], "8N1");
        let l = text(door32_sys(&sample()));
        assert_eq!(l.len(), 11);
        assert_eq!(&l[4..], ["7", "Some One", "phrac", "50", "59", "1", "2"]);
    }

    #[test]
    fn dz_writes_both() {
        let names: Vec<_> = drop_files('z').iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["DOOR.SYS", "DORINFO1.DEF"]);
        assert!(drop_files('0').is_empty());
    }

    #[test]
    fn input_for_doors() {
        let mut i = DoorInput::new(true);
        assert_eq!(i.feed(b"a\r\nb\r"), b"a\rb\r");
        assert_eq!(i.feed(b"\0c"), b"c", "NUL after CR across reads");
        // A box character split across two reads.
        assert_eq!(i.feed(&"░".as_bytes()[..1]), b"");
        assert_eq!(i.feed(&"░".as_bytes()[1..]), [0xB0]);
        assert_eq!(DoorInput::new(false).feed(&[0xB0, 0x1B]), [0xB0, 0x1B]);
    }

    #[test]
    fn dos_doors_by_extension() {
        assert!(is_dos("LORD.BAT"));
        assert!(is_dos("c:\\bre\\bre.exe /n1"));
        assert!(is_dos("tw2002.com %ND"));
        assert!(!is_dos("/opt/doors/start.sh"));
        assert_eq!(door_name("LORD.BAT 1"), "LORD");
        let d = String::from_utf8(door_sys(&DropInfo { seconds_left: 1800, uploads: 3, downloads: 9, today_dl_kb: 120, ..DropInfo::default() })).unwrap();
        assert_eq!(readback(&d), Readback { minutes_left: Some(30), uploads: Some(3), downloads: Some(9), today_dl_kb: Some(120) });
        assert_eq!(door_name("/opt/doors/tw2002/run.sh"), "run");
        assert_eq!(door_name("C:\\DOORS\\BRE.EXE"), "BRE");
        assert!(!is_dos("./game.exe"));
        assert!(!is_dos("sleep 600"));
        assert_eq!(dos_safe("Dark Lord|del *.*>nul %path%"), "Dark Lorddel *.*nul path");
    }

    #[test]
    fn dosbox_config() {
        let dos = Dos { setup: vec!["c:\\bnu\\bnu.com".into()], config: vec!["[cpu]".into(), "cycles=max".into()], ..Dos::default() };
        let c = dosbox_conf(&dos, 4321, Path::new("/bbs/doors/dos"), Path::new("/bbs/data/doors/node1"), "LORD.BAT 1");
        assert!(c.starts_with("[cpu]\ncycles=max\n"));
        assert!(c.contains("serial1=nullmodem server:127.0.0.1 port:4321 transparent:1"));
        let auto = c.split("[autoexec]\n").nth(1).unwrap();
        assert_eq!(
            auto.lines().collect::<Vec<_>>(),
            [
                "@echo off",
                "mount c \"/bbs/doors/dos\"",
                "mount d \"/bbs/data/doors/node1\"",
                "d:",
                "path z:\\;c:\\",
                "c:\\bnu\\bnu.com",
                "call LORD.BAT 1",
                "choice /c:y /t:y,2 > nul",
                "exit",
            ]
        );
    }

    #[test]
    fn finds_dos_programs_any_case() {
        let dir = std::env::temp_dir().join(format!("iqdos{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lord.bat"), "").unwrap();
        assert!(dos_program_found(&dir, "LORD.BAT 1"));
        assert!(!dos_program_found(&dir, "BRE.BAT"));
        assert!(dos_program_found(&dir, "C:\\BRE\\BRE.EXE"), "paths are DOS's business");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
