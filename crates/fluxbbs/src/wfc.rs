//! `wfcWaitForCall` (WAIT4CAL.PAS, ANSIWFCS.PAS): the waiting-for-caller
//! screen, on the console of a board started with `--wfc`.
//!
//! The original ran one per node and waited on the modem. Here the server
//! takes calls as usual and the screen watches all of it: the node field
//! counts callers on, the event field shows the next event, and the
//! "terminal" window that echoed the modem tails the log instead (the
//! whole log goes to `data/iniquity.log`). Its commands run in a local
//! session as the SysOp, as the original's did with `LocalIO`.
//!
//! WAIT4CAL.PAS is damaged (blocks repeated over others, like NODES.PAS),
//! and its coordinates are for an older screen than the one in
//! ANSIWFCS.PAS: the boxes there are the same, moved 26 columns. The
//! menu, keys and commands are as the original; the stats pages are
//! rebuilt from their titles and what's left of them, and the modem
//! commands, modem setup and MicroDOS are gone.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fluxbbs_term::codes::{resize, squish};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::BbsState;
use crate::input::{Case, ReadOpts};
use crate::session::{self, Caller, LocalJob, Session};

/// `INIWFCSCREEN`: 80x25 character/attribute pairs (TheDraw, 08/14/96).
const SCREEN: &[u8; 4000] = include_bytes!("../assets/wfc.bin");

/// `WFCCom`: six pages of eight commands (F1-F8).
const WFC_COM: [[&str; 8]; 6] = [
    ["local logon", "exit flux/bbs", "jump to shell", "---", "---", "---", "---", "---"],
    ["system config", "---", "string editor", "menu editor", "---", "msgarea editor", "---", "level editor"],
    ["protocol cfg", "user editor", "launch menu", "read email", "view mailbox", "enter email", "filearea edit", "edit textfile"],
    ["archiver setup", "file conf edit", "msg conf edit", "sort all files", "stat editor", "bbslist editor", "event editor", "textfile libs"],
    ["edit infoforms", "---", "---", "---", "---", "---", "---", "---"],
    ["system info", "---", "system totals", "today's stats", "last callers", "---", "---", "flux/bbs info"],
];

/// Stats titles (`Ttl`), by page number.
const STAT_TITLE: [&str; 8] =
    ["system information", "", "system statistics", "today's statistics", "last four callers", "", "", "flux/bbs information"];

// Where things are on the screen (1-based).
const MENU_X: u16 = 3;
const MENU_Y: u16 = 14;
const PAGE_X: u16 = 8;
const STAT_X: u16 = 31;
const STAT_Y: u16 = 15;
const TERM_Y: u16 = 20;
const TERM_W: usize = 47;

// Colours without the VGA palette effects (stock had them off).
const COL_LO: u8 = 0x02;
const COL_HI: u8 = 0x0A;

enum Act {
    Nothing,
    Exit,
    Shell,
    Local(LocalJob),
    Stat(u8),
}

fn action(page: usize, item: usize) -> Act {
    use LocalJob::{Command, EditText, LaunchMenu, Logon, Mailbox};
    let local = match (page, item) {
        (1, 1) => Logon,
        (1, 2) => return Act::Exit,
        (1, 3) => return Act::Shell,
        (2, 1) => Command("*C", ""),
        (2, 3) => Command("*E", ""),
        (2, 4) => Command("*#", ""),
        (2, 6) => Command("*A", ""),
        (2, 8) => Command("*L", ""),
        (3, 1) => Command("*X", ""),
        (3, 2) => Command("*U", ""),
        (3, 3) => LaunchMenu,
        (3, 4) => Command("*R", ""),
        (3, 5) => Mailbox,
        (3, 6) => Command("EE", ""),
        (3, 7) => Command("*F", ""),
        (3, 8) => EditText,
        (4, 1) => Command("*Z", ""),
        (4, 2) => Command("*!", ""),
        (4, 3) => Command("*@", ""),
        (4, 4) => Command("*S", "A"),
        (4, 5) => Command("*H", ""),
        (4, 6) => Command("*B", ""),
        (4, 7) => Command("*V", ""),
        (4, 8) => Command("*T", ""),
        (5, 1) => Command("*I", ""),
        (6, n @ (1 | 3 | 4 | 5 | 8)) => return Act::Stat(n as u8),
        _ => return Act::Nothing,
    };
    Act::Local(local)
}

// ---- the log ----------------------------------------------------------

/// Log lines for the terminal window, and the log file, in place of
/// stderr (which is the screen).
#[derive(Clone)]
pub struct LogTail(Arc<LogInner>);

struct LogInner {
    lines: Mutex<VecDeque<String>>,
    changed: AtomicU64,
    file: Mutex<Option<std::fs::File>>,
}

impl LogTail {
    pub fn new(file: &std::path::Path) -> Self {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(file).ok();
        LogTail(Arc::new(LogInner { lines: Mutex::new(VecDeque::new()), changed: AtomicU64::new(0), file: Mutex::new(file) }))
    }

    fn push(&self, text: &str) {
        let now = crate::clock::now();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Some(f) = self.0.file.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                let _ = writeln!(f, "{} {line}", now.format("%Y-%m-%d %H:%M:%S"));
            }
            // " INFO message": keep the level only when it says something.
            let line = line.trim_start();
            let line = match line.split_once(' ') {
                Some(("INFO", msg)) => msg.trim_start().to_string(),
                _ => line.to_string(),
            };
            let mut lines = self.0.lines.lock().unwrap_or_else(|e| e.into_inner());
            lines.push_back(format!("{} {line}", now.format("%H:%M")));
            while lines.len() > 3 {
                lines.pop_front();
            }
        }
        self.0.changed.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct LogWriter {
    tail: LogTail,
    buf: Vec<u8>,
}

impl io::Write for LogWriter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        self.tail.push(&String::from_utf8_lossy(&self.buf));
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogTail {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> LogWriter {
        LogWriter { tail: self.clone(), buf: Vec::new() }
    }
}

// ---- the console ------------------------------------------------------

fn is_tty() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

fn raw_mode() -> io::Result<libc::termios> {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &mut t) != 0 {
            return Err(io::Error::last_os_error());
        }
        let saved = t;
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(0, libc::TCSANOW, &t);
        Ok(saved)
    }
}

fn restore_mode(t: &libc::termios) {
    unsafe {
        libc::tcsetattr(0, libc::TCSANOW, t);
    }
}

fn term_size() -> (u16, u16) {
    unsafe {
        let mut w: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut w) == 0 && w.ws_col > 0 && w.ws_row > 0 {
            (w.ws_col, w.ws_row)
        } else {
            (80, 25)
        }
    }
}

/// Keyboard bytes from a thread, which stops reading while a shell has
/// the terminal.
fn spawn_reader(paused: Arc<AtomicBool>) -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            if paused.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut pfd, 1, 100) } <= 0 || paused.load(Ordering::Relaxed) {
                continue;
            }
            let n = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 || tx.send(buf[..n as usize].to_vec()).is_err() {
                return;
            }
        }
    });
    rx
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Quit,
    /// F1-F8, or 1-8.
    F(usize),
    Other,
}

fn parse_keys(b: &[u8]) -> Vec<Key> {
    const SEQ: [(&[u8], Key); 20] = [
        (b"\x1b[A", Key::Up),
        (b"\x1b[B", Key::Down),
        (b"\x1b[C", Key::Right),
        (b"\x1b[D", Key::Left),
        (b"\x1bOA", Key::Up),
        (b"\x1bOB", Key::Down),
        (b"\x1bOC", Key::Right),
        (b"\x1bOD", Key::Left),
        (b"\x1bOP", Key::F(1)),
        (b"\x1bOQ", Key::F(2)),
        (b"\x1bOR", Key::F(3)),
        (b"\x1bOS", Key::F(4)),
        (b"\x1b[11~", Key::F(1)),
        (b"\x1b[12~", Key::F(2)),
        (b"\x1b[13~", Key::F(3)),
        (b"\x1b[14~", Key::F(4)),
        (b"\x1b[15~", Key::F(5)),
        (b"\x1b[17~", Key::F(6)),
        (b"\x1b[18~", Key::F(7)),
        (b"\x1b[19~", Key::F(8)),
    ];
    let mut keys = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if let Some((s, k)) = SEQ.iter().find(|(s, _)| b[i..].starts_with(s)) {
            keys.push(*k);
            i += s.len();
            continue;
        }
        keys.push(match b[i] {
            b'\r' | b'\n' => Key::Enter,
            0x1b if i + 1 == b.len() => Key::Esc,
            3 => Key::Quit,
            c @ b'1'..=b'8' => Key::F((c - b'0') as usize),
            _ => Key::Other,
        });
        // An unknown escape sequence goes as one key.
        if b[i] == 0x1b && i + 1 < b.len() {
            i += 1;
            while i < b.len() && !(b[i].is_ascii_alphabetic() || b[i] == b'~') {
                i += 1;
            }
        }
        i += 1;
    }
    keys
}

/// Screen output: positioned text in DOS colours.
#[derive(Default)]
struct Out(String);

impl Out {
    fn color(&mut self, attr: u8) {
        const ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
        let fg = attr & 15;
        let fg = if fg >= 8 { 90 + ANSI[(fg - 8) as usize] } else { 30 + ANSI[fg as usize] };
        let _ = write!(self.0, "\x1b[0;{fg};{}m", 40 + ANSI[((attr >> 4) & 7) as usize]);
    }

    /// `fWrite`.
    fn at(&mut self, x: u16, y: u16, text: &str, attr: u8) {
        let _ = write!(self.0, "\x1b[{y};{x}H");
        self.color(attr);
        self.0.push_str(text);
    }

    fn flush(&mut self) {
        let mut out = io::stdout().lock();
        let _ = out.write_all(self.0.as_bytes());
        let _ = out.flush();
        self.0.clear();
    }
}

// ---- the screen -------------------------------------------------------

struct Wfc {
    state: Arc<BbsState>,
    log: LogTail,
    saved: libc::termios,
    paused: Arc<AtomicBool>,
    out: Out,
    page: usize,
    pos: usize,
    stat: u8,
    started: Instant,
    last_key: Instant,
    blanked: bool,
    log_seen: u64,
    last_fields: String,
    last_stats: Instant,
}

impl Drop for Wfc {
    fn drop(&mut self) {
        self.out.0.push_str("\x1b[0m\x1b[2J\x1b[H\x1b[?25h\x1b[?1049l");
        self.out.flush();
        restore_mode(&self.saved);
    }
}

/// Run the screen until the SysOp exits.
pub async fn run(state: Arc<BbsState>, log: LogTail) -> anyhow::Result<()> {
    let saved = raw_mode()?;
    let paused = Arc::new(AtomicBool::new(false));
    let mut keys = spawn_reader(paused.clone());
    let stat = state.config().wfc.default_stat;
    let mut w = Wfc {
        state,
        log,
        saved,
        paused,
        out: Out::default(),
        page: 1,
        pos: 1,
        stat: if matches!(stat, 1 | 3 | 4 | 5 | 8) { stat } else { 8 },
        started: Instant::now(),
        last_key: Instant::now(),
        blanked: false,
        log_seen: u64::MAX,
        last_fields: String::new(),
        last_stats: Instant::now(),
    };
    w.out.0.push_str("\x1b[?1049h");
    w.draw();
    tracing::info!("waiting for call");
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            got = keys.recv() => {
                let Some(bytes) = got else { return Ok(()) };
                w.last_key = Instant::now();
                if w.blanked {
                    w.blanked = false;
                    w.draw();
                    continue;
                }
                for key in parse_keys(&bytes) {
                    if !w.key(key, &mut keys).await? {
                        return Ok(());
                    }
                }
            }
            _ = tick.tick() => w.update(),
        }
    }
}

impl Wfc {
    /// The whole screen (`ansiWFCscreen` and the parts drawn on it).
    fn draw(&mut self) {
        self.out.0.push_str("\x1b[?25l\x1b[0m\x1b[2J");
        for y in 0..25u16 {
            let _ = write!(self.out.0, "\x1b[{};1H", y + 1);
            let mut last = None;
            for x in 0..80usize {
                let i = (y as usize * 80 + x) * 2;
                let (ch, attr) = (SCREEN[i], SCREEN[i + 1]);
                if last != Some(attr) {
                    self.out.color(attr);
                    last = Some(attr);
                }
                self.out.0.push(fluxbbs_term::cp437::TO_UNICODE[ch as usize]);
            }
        }
        self.draw_menu();
        self.draw_stats();
        self.last_fields.clear();
        self.log_seen = u64::MAX;
        self.update();
    }

    /// `wfcDrawSelBar` / `wfcDrawNorBar`: the first letter of each word
    /// picked out.
    fn draw_bar(&mut self, ci: usize, selected: bool) {
        let s = format!("{}{}", resize(&format!(" f{ci}"), 4), resize(WFC_COM[self.page - 1][ci - 1], 16));
        let chars: Vec<char> = s.chars().collect();
        for (x, &c) in chars.iter().enumerate() {
            let first = x > 1 && chars[x - 1] == ' ';
            let attr = match (selected, first) {
                (true, true) => 0x2F,
                (true, false) => 0x2A,
                (false, true) => 0x02,
                (false, false) => 0x08,
            };
            self.out.at(MENU_X + x as u16, MENU_Y + ci as u16, &c.to_string(), attr);
        }
    }

    fn draw_menu(&mut self) {
        self.out.at(PAGE_X, 23, "------", 0x08);
        let p = self.page as u16;
        if p > 1 {
            self.out.at(PAGE_X + p - 2, 23, "-", 0x07);
        }
        self.out.at(PAGE_X + p - 1, 23, "■", 0x0F);
        if p < 6 {
            self.out.at(PAGE_X + p, 23, "-", 0x07);
        }
        self.out.at(PAGE_X + 8, 23, &p.to_string(), 0x07);
        for ci in 1..=8 {
            self.draw_bar(ci, ci == self.pos);
        }
    }

    /// `wfcDrawStats`.
    fn draw_stats(&mut self) {
        let (sx, sy) = (STAT_X, STAT_Y);
        self.out.at(35, 23, &resize(STAT_TITLE[self.stat as usize - 1], 25), COL_HI);
        self.out.at(70, 23, &self.stat.to_string(), 0x07);
        for y in 0..4 {
            self.out.at(sx - 1, sy + y, &" ".repeat(49), 0x07);
        }
        let state = self.state.clone();
        let cfg = state.config();
        let pair = |out: &mut Out, x: u16, y: u16, label: &str, value: String| {
            out.at(x, sy + y, label, COL_LO);
            out.at(x + 12, sy + y, &value, COL_HI);
        };
        match self.stat {
            1 => {
                let (os, host) = uname();
                let up = self.started.elapsed().as_secs();
                let up = format!("{}d {:02}:{:02}", up / 86400, up / 3600 % 24, up / 60 % 60);
                let online = state.nodes.online().len();
                for (y, (label, value)) in [
                    ("Running under", os),
                    ("Host name", host),
                    ("Server up", up),
                    ("Nodes in use", format!("{online} of {}", cfg.bbs.max_nodes)),
                ]
                .into_iter()
                .enumerate()
                {
                    self.out.at(sx, sy + y as u16, label, COL_LO);
                    self.out.at(sx + 16, sy + y as u16, &squish(&value, 31), COL_HI);
                }
            }
            3 => {
                let t = state.sys.totals().unwrap_or_default();
                let users = state.users.count().unwrap_or(0);
                pair(&mut self.out, sx, 0, "Calls", t.calls.to_string());
                pair(&mut self.out, sx + 23, 0, "Users", users.to_string());
                pair(&mut self.out, sx, 1, "Posts", t.posts.to_string());
                pair(&mut self.out, sx + 23, 1, "Email", t.email.to_string());
                pair(&mut self.out, sx, 2, "Uploads", t.uploads.to_string());
                pair(&mut self.out, sx + 23, 2, "Downloads", t.downloads.to_string());
                pair(&mut self.out, sx, 3, "Upload kb", t.upload_kb.to_string());
                pair(&mut self.out, sx + 23, 3, "Download kb", t.download_kb.to_string());
            }
            4 => {
                let d = state.sys.day(crate::clock::now().date_naive()).unwrap_or_default();
                let cols = [
                    ("calls", d.calls),
                    ("posts", d.posts),
                    ("email", d.email),
                    ("uls", d.uploads),
                    ("dls", d.downloads),
                    ("new", d.new_users),
                ];
                for (i, (label, n)) in cols.into_iter().enumerate() {
                    self.out.at(sx + i as u16 * 8, sy, label, COL_LO);
                    self.out.at(sx + i as u16 * 8, sy + 1, &n.to_string(), COL_HI);
                }
                pair(&mut self.out, sx, 3, "Upload kb", d.upload_kb.to_string());
                pair(&mut self.out, sx + 23, 3, "Download kb", d.download_kb.to_string());
            }
            5 => {
                let calls = state.sys.calls(Some(4), None).unwrap_or_default();
                for w in 0..4u16 {
                    match calls.get(w as usize) {
                        Some(c) => {
                            self.out.at(sx, sy + w, &format!("{:>5}", c.call_num), COL_LO);
                            if c.new_user {
                                self.out.at(sx + 6, sy + w, "*", COL_LO);
                            }
                            self.out.at(sx + 7, sy + w, &squish(&c.handle, 23), COL_HI);
                            self.out.at(sx + 31, sy + w, &format!("{:>7}", c.at.format("%-I:%M%P").to_string()), COL_LO);
                            self.out.at(sx + 40, sy + w, &format!("{:>6}", squish(&c.transport, 6)), COL_LO);
                        }
                        None => self.out.at(sx, sy + w, "---", COL_LO),
                    }
                }
            }
            _ => {
                let centre = |s: &str| format!("{}{s}", " ".repeat(47usize.saturating_sub(s.chars().count()) / 2));
                let telnet = if cfg.telnet.enabled { format!(", telnet {}", cfg.telnet.listen) } else { String::new() };
                self.out.at(sx, sy, &centre(&format!("flux/bbs {}", env!("CARGO_PKG_VERSION"))), COL_HI);
                self.out.at(sx, sy + 1, &centre("after Iniquity 1.00a26r5,"), COL_LO);
                self.out.at(sx, sy + 2, &centre("(c)Copyright 1994-1997, Mike Fricker"), COL_LO);
                self.out.at(sx, sy + 3, &centre(&squish(&format!("ssh {}{telnet}", cfg.ssh.listen), 47)), COL_LO);
            }
        }
        self.last_stats = Instant::now();
    }

    /// `wfcUpdate`: the clock and fields, the log window, live stats, and
    /// the screen blanker.
    fn update(&mut self) {
        let blank = self.state.config().wfc.blank_seconds;
        if !self.blanked && blank > 0 && self.last_key.elapsed() >= Duration::from_secs(blank as u64) {
            self.blanked = true;
            self.out.0.push_str("\x1b[0m\x1b[2J");
            self.out.flush();
        }
        if self.blanked {
            return;
        }
        let now = crate::clock::now();
        let online = self.state.nodes.online().len();
        let fields = [
            format!("{online} of {}", self.state.config().bbs.max_nodes),
            next_event(&self.state, now.naive_local()),
            now.format("%-I:%M%P").to_string(),
            now.format("%m/%d/%y").to_string(),
        ];
        let joined = fields.join("|");
        if joined != self.last_fields {
            for ((x, w), v) in [(11, 10), (32, 10), (52, 8), (70, 9)].into_iter().zip(&fields) {
                self.out.at(x, 3, &resize(&squish(v, w), w), COL_LO);
            }
            self.last_fields = joined;
        }
        let seen = self.log.0.changed.load(Ordering::Relaxed);
        if seen != self.log_seen {
            self.log_seen = seen;
            let lines: Vec<String> = self.log.0.lines.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
            for y in 0..3 {
                let line = lines.get(y).map(String::as_str).unwrap_or("");
                self.out.at(STAT_X, TERM_Y + y as u16, &resize(&squish(line, TERM_W), TERM_W), 0x07);
            }
        }
        if self.stat != 8 && self.last_stats.elapsed() >= Duration::from_secs(5) {
            self.draw_stats();
        }
        self.out.flush();
    }

    /// One key at the menu (`wfcGetSel`); false to exit.
    async fn key(&mut self, key: Key, keys: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> anyhow::Result<bool> {
        let old = (self.page, self.pos);
        match key {
            Key::Up if self.pos == 1 && self.page > 1 => {
                self.page -= 1;
                self.pos = 8;
            }
            Key::Up if self.pos > 1 => self.pos -= 1,
            Key::Down if self.pos == 8 && self.page < 6 => {
                self.page += 1;
                self.pos = 1;
            }
            Key::Down if self.pos < 8 => self.pos += 1,
            Key::Left if self.page > 1 => self.page -= 1,
            Key::Right if self.page < 6 => self.page += 1,
            Key::F(n) => {
                self.pos = n;
                self.draw_menu();
                self.out.flush();
                return self.select(keys).await;
            }
            Key::Enter => return self.select(keys).await,
            Key::Esc if self.state.config().wfc.esc_to_exit => return Ok(false),
            Key::Quit => return Ok(false),
            _ => {}
        }
        if (self.page, self.pos) != old {
            self.draw_menu();
            self.out.flush();
        }
        Ok(true)
    }

    async fn select(&mut self, keys: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> anyhow::Result<bool> {
        match action(self.page, self.pos) {
            Act::Nothing => {}
            Act::Exit => return Ok(false),
            Act::Shell => self.shell().await?,
            Act::Stat(n) => {
                self.stat = n;
                self.draw_stats();
                self.out.flush();
            }
            Act::Local(what) => {
                self.local(what, keys).await?;
                self.draw();
            }
        }
        Ok(true)
    }

    /// `fJumpToDOS`: a shell, with the board still running.
    async fn shell(&mut self) -> anyhow::Result<()> {
        self.paused.store(true, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.out.0.push_str("\x1b[0m\x1b[2J\x1b[H\x1b[?25h\x1b[?1049l");
        self.out.0.push_str("Type 'exit' to return to flux/bbs.\r\n");
        self.out.flush();
        restore_mode(&self.saved);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let _ = tokio::task::spawn_blocking(move || std::process::Command::new(shell).status()).await;
        raw_mode()?;
        self.paused.store(false, Ordering::Relaxed);
        self.out.0.push_str("\x1b[?1049h");
        self.draw();
        Ok(())
    }

    /// A local session (`LocalIO`) on the console until it's done.
    async fn local(&mut self, what: LocalJob, keys: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> anyhow::Result<()> {
        self.out.0.push_str("\x1b[0m\x1b[2J\x1b[H\x1b[?25h");
        self.out.flush();
        let (mut ours, theirs) = tokio::io::duplex(1 << 16);
        let (_size_tx, size) = tokio::sync::watch::channel(term_size());
        let caller = Caller {
            remote: None,
            transport: "local",
            login_name: String::new(),
            key_user: None,
            term: std::env::var("TERM").unwrap_or_else(|_| "xterm".into()),
            size,
        };
        let task = tokio::spawn(session::run_local(self.state.clone(), Box::new(theirs), caller, what));
        let mut buf = vec![0u8; 8192];
        loop {
            tokio::select! {
                r = ours.read(&mut buf) => match r {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut out = io::stdout().lock();
                        let _ = out.write_all(&buf[..n]);
                        let _ = out.flush();
                    }
                },
                Some(b) = keys.recv() => {
                    let _ = ours.write_all(&b).await;
                }
            }
        }
        let _ = task.await;
        self.last_key = Instant::now();
        Ok(())
    }
}

/// The next active event: "HH:MM name".
fn next_event(state: &BbsState, now: chrono::NaiveDateTime) -> String {
    let events = state.events();
    let next = events
        .list
        .iter()
        .filter(|e| e.active)
        .filter_map(|e| {
            let t = crate::events::parse_time(&e.time)?;
            let at = now.date().and_time(t);
            Some((if at > now { at } else { at + chrono::Duration::days(1) }, e))
        })
        .min_by_key(|(at, _)| *at);
    match next {
        Some((at, e)) => format!("{} {}", at.format("%H:%M"), e.desc),
        None => "none".into(),
    }
}

/// The system and host name.
fn uname() -> (String, String) {
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut u) != 0 {
            return (std::env::consts::OS.into(), String::new());
        }
        let s = |f: &[libc::c_char]| std::ffi::CStr::from_ptr(f.as_ptr()).to_string_lossy().into_owned();
        (format!("{} {}", s(&u.sysname), s(&u.release)), s(&u.nodename))
    }
}

/// Check the console can show the screen.
pub fn check() -> anyhow::Result<()> {
    if !is_tty() {
        anyhow::bail!("--wfc needs a terminal");
    }
    Ok(())
}

impl Session {
    /// The SysOp at the console for a WFC command (`userReset`, then the
    /// SysOp's own record). It isn't a call: nothing is counted.
    pub(crate) async fn local_sysop(&mut self, what: LocalJob) -> io::Result<()> {
        let Some(u) = self.state.users.get(1).map_err(io::Error::other)?.filter(|u| !u.deleted) else {
            self.write("There's no SysOp account (user #1).");
            self.newline();
            return self.pause().await;
        };
        self.user = u;
        *self.user_base.borrow_mut() = Some(self.user.clone());
        self.logged_in = true;
        self.time_on = Instant::now();
        self.user.time_today = self.user.time_today.max(self.user.time_per_day);
        let confs = self.state.areas().confs.len().max(1) as u32;
        self.user.conf_msg = self.user.conf_msg.clamp(1, confs);
        self.user.conf_file = self.user.conf_file.clamp(1, self.state.fareas().confs.len().max(1) as u32);
        self.find_area_with_access();
        self.find_farea_with_access();
        let (handle, location) = (self.user.handle.clone(), self.user.location.clone());
        self.node.update(|n| {
            n.user = handle;
            n.location = location;
            n.sysop = true;
        });
        match what {
            LocalJob::Logon => {}
            LocalJob::Command(code, param) => {
                self.run_command(code, param).await?;
                // The sorter reports and returns.
                if code == "*S" {
                    self.newline();
                    self.pause().await?;
                }
            }
            LocalJob::LaunchMenu => {
                self.show("|U2-- |U1Enter menu name to launch|U2: |U3").await?;
                let name = self.read_string(ReadOpts::new(8).case(Case::Upper)).await?;
                self.newline();
                let dir = self.state.config().menu_dirs();
                if name.is_empty() || crate::menu::Menu::load(&dir, &name).is_err() {
                    return Ok(());
                }
                tracing::info!("launched menu {name}");
                self.menu.current = name;
                self.menu_install().await?;
                self.menu.next_cmd.clear();
                self.menu.exec_first = false;
                while !self.hangup {
                    self.menu_exec().await?;
                }
            }
            LocalJob::Mailbox => {
                self.show("|U2-- |U1View which user's email|U2: |U3").await?;
                let name = self.read_string(ReadOpts::new(36)).await?;
                self.newline();
                let Some(other) = self.state.users.find(&name, false).map_err(io::Error::other)?.filter(|_| !name.is_empty()) else {
                    return Ok(());
                };
                tracing::info!("checked {}'s mailbox", other.handle);
                // Their record, with its own base, for the while.
                let base = self.user_base.replace(Some(other.clone()));
                let sysop = std::mem::replace(&mut self.user, other);
                let r = self.read_email(false, false).await;
                if let Err(e) = self.save_own_user() {
                    tracing::error!("saving {}: {e}", self.user.handle);
                }
                self.user = sysop;
                *self.user_base.borrow_mut() = base;
                r?;
            }
            LocalJob::EditText => self.edit_textfile().await?,
        }
        self.flush().await
    }

    /// `maEditTextfile`: edit a text file (in the text directory) in the
    /// full-screen editor.
    async fn edit_textfile(&mut self) -> io::Result<()> {
        self.show("|U2-- |U1Enter filename to edit|U2: |U3").await?;
        let name = self.read_string(ReadOpts::new(36).case(Case::Upper)).await?;
        self.newline();
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Ok(());
        }
        let dir = self.state.config().resolve(&self.state.config().paths.text);
        let path = session::find_ci(&dir, &name).unwrap_or_else(|| dir.join(&name));
        let text = std::fs::read(&path).map(|b| fluxbbs_term::cp437::decode(&b)).unwrap_or_default();
        let lines: Vec<String> = text.trim_end_matches('\x1a').lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
        let ctx = crate::editor::EditCtx {
            subject: name.clone(),
            from: self.user.handle.clone(),
            to: String::new(),
            date: crate::clock::now().naive_local(),
            anonymous: false,
            can_anonymous: false,
            can_autosig: false,
            autosig: false,
            reply: None,
            auto_quote: false,
            text: lines,
        };
        if let Some(r) = self.edit_message(ctx).await? {
            let mut out = r.lines.join("\r\n");
            out.push_str("\r\n");
            std::fs::write(&path, fluxbbs_term::cp437::encode(&out))?;
            tracing::info!("edited {}", path.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_from_terminals() {
        assert_eq!(parse_keys(b"\x1b[A\x1bOB\x1b[C"), [Key::Up, Key::Down, Key::Right]);
        assert_eq!(parse_keys(b"\x1bOP\x1b[19~5\r"), [Key::F(1), Key::F(8), Key::F(5), Key::Enter]);
        assert_eq!(parse_keys(b"\x1b"), [Key::Esc]);
        assert_eq!(parse_keys(b"\x1b[24~x\x03"), [Key::Other, Key::Other, Key::Quit], "unknown sequences are one key");
    }

    #[test]
    fn every_item_has_a_label() {
        for page in 1..=6 {
            for item in 1..=8 {
                let used = !matches!(action(page, item), Act::Nothing);
                assert_eq!(used, WFC_COM[page - 1][item - 1] != "---", "page {page} item {item}");
            }
        }
    }
}
