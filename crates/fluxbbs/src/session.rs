//! One caller's session: output, the display engine (`oStr` /
//! `sfShowFile`), and session state shared by logon, menus and commands.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::{Datelike, Timelike};
use fluxbbs_term::codes::{resize, squish};
use fluxbbs_term::{Charset, Color, Control, Screen, Slot, Source, Token, cp437, tokenize, tokenize_str, tokenize_user};
use rand::RngExt;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;

use crate::acs::{self, AcsEnv};
use crate::config::CharsetPref;
use crate::engine::MenuState;
use crate::input::Key;
use crate::nodes::NodeGuard;
use crate::strings::Strings;
use crate::user::User;
use crate::{BbsState, mci};

/// Values for a screen's `~XX` codes ("wave codes"). Codes without a
/// value mark positions instead.
pub type Waves = HashMap<[u8; 2], String>;

/// Build [`Waves`] from `("XX", value)` pairs.
pub fn waves<const N: usize>(pairs: [(&str, String); N]) -> Waves {
    pairs.into_iter().map(|(k, v)| (k.as_bytes().try_into().expect("two-letter code"), v)).collect()
}

/// One row of a list with a moving bar (`area_bar`): its area number,
/// `~XX` values and plain-text line.
pub struct BarRow {
    pub n: u32,
    pub waves: Waves,
    pub plain: String,
}

/// What a caller did in `area_bar`.
pub enum BarPick {
    Area(u32),
    Up,
    Stay,
    /// `q`: out of the lists altogether, back to the menu.
    Quit,
    /// One of the spec's `keys`, with the row the bar was on (its `n`).
    Key(char, u32),
    /// New: a post has landed (`bar_news`); draw the list again, the bar
    /// on this row.
    Refresh(u32),
}

/// A list for `area_bar`.
#[derive(Clone, Copy)]
pub struct BarSpec<'a> {
    /// The repeat screens' prefix: `{prefix}TOP`, MID, HIL, BOT.
    pub prefix: &'a str,
    /// The column heading when those screens don't exist.
    pub header: &'a str,
    pub rows: &'a [BarRow],
    /// (number shown, row `n`) for typing a number.
    pub labels: &'a [(u32, u32)],
    pub start: usize,
    pub help: &'a str,
    /// The prompt's string name; empty for none.
    pub ask: &'a str,
    pub can_go_up: bool,
    /// Letters (upper case) that end the list as `BarPick::Key`.
    pub keys: &'a str,
}

/// A TOP/MID/HIL/BOT repeat screen in progress (Appendix C).
/// How much of what's typed shows in a prompt bar's slot.
const BAR_SLOT: usize = 6;

pub struct Repeat {
    prefix: String,
    mid: Vec<u8>,
    hil: Option<Vec<u8>>,
    lines: u16,
    /// Lines the BOT screen takes.
    pub bot_lines: u16,
    /// New: BOT is a prompt bar, with `~HP` for the help line and `~PC`
    /// for the cursor, and is the screen's last row.
    pub bar: bool,
}

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// What the transport knows about the connection.
pub struct Caller {
    pub remote: Option<SocketAddr>,
    pub transport: &'static str,
    /// SSH user name.
    pub login_name: String,
    /// New: the user whose SSH key (or own password) the client logged in
    /// with.
    pub key_user: Option<u32>,
    pub term: String,
    /// (columns, rows), updated on window changes.
    pub size: watch::Receiver<(u16, u16)>,
}

pub struct Session {
    /// New: the order the art gallery and file browser list a place in, kept
    /// for the visit (R in the gallery cycles it).
    pub(crate) gallery_order: crate::gallery::Order,
    pub state: Arc<BbsState>,
    pub caller: Caller,
    pub node: NodeGuard,
    /// Keep node messages queued (full-screen editor, chat).
    pub hold_node_msgs: bool,
    /// New: a SysOp's edits to this caller wait (their settings screen is
    /// open, and Esc there puts its own copy back).
    pub(crate) hold_account_changes: bool,
    pub user: User,
    /// The caller's record as this session last loaded or saved it, to
    /// tell its own changes from others' (`save_own_user`).
    pub(crate) user_base: std::cell::RefCell<Option<User>>,
    /// Set while asking whether the caller's time is unlimited: the ACS
    /// that decides it sees their own minutes, not a whole day.
    checking_time: std::cell::Cell<bool>,
    pub(crate) io: Box<dyn Stream>,
    pub(crate) screen: Screen,
    pub(crate) input: VecDeque<u8>,
    pub(crate) last_cr: bool,
    /// New: the next idle wait counts from here (a menu animating
    /// meanwhile mustn't put the inactivity timeout off).
    pub(crate) idle_since: Option<tokio::time::Instant>,
    /// When the caller last pressed a key: animations calm down after a while.
    pub(crate) last_key: std::time::Instant,
    /// New: the effects beside the screen being shown (`NAME.FX`), for
    /// its `|PK` wait.
    pub(crate) screen_fx: Option<Vec<crate::ambient::Effect>>,
    /// Cursor position questions asked and answers seen, for `sync_output`.
    pub(crate) cursor_asks: u32,
    pub(crate) cursor_reports: u32,
    /// The start of a cursor report whose end hasn't arrived yet.
    pub(crate) report_tail: Vec<u8>,
    /// New: the posts count a key wait gives up at (`read_key_or_post`).
    pub(crate) refresh_on: Option<u64>,
    /// New: the posts count the next `area_bar` was drawn at, so it
    /// redraws when one lands; None for a list that stays as it is.
    pub(crate) bar_news: Option<u64>,
    /// New: while a lightbar menu with a help line (`~HP`) waits, how much
    /// of that line its text takes, so a notice can go there instead.
    pub(crate) menu_help: Option<usize>,
    /// New: the terminal's answers to `ESC[c` and `ESC[<c` (without
    /// `ESC[` and `c`) and to DECRQSS (without `ESC P` and `ESC \`).
    pub(crate) device_attrs: Vec<String>,
    /// A UTF-8 character split between reads.
    pub(crate) utf8_tail: Vec<u8>,
    /// Bytes sent this call, for the stalled-output warning.
    sent: u64,
    saved_color: Color,
    /// When this call's time accounting started (logon).
    pub time_on: Instant,
    pub logged_in: bool,
    /// Set to end the call (`HangUp`).
    pub hangup: bool,
    pub menu: MenuState,
    /// The menu command "input string" and OK flag (`?` commands, ACS `*`).
    pub input_string: String,
    pub ok_flag: bool,
    /// Message number being read (`%MN`).
    pub menu_msg: u32,
    /// Files flagged for download: (area, file number).
    pub batch: Vec<(u32, u32)>,
    /// The caller applied as a new user on this call.
    pub new_user: bool,
    /// `XI` in the matrix: this call stays out of the callers list.
    pub hidden_call: bool,
    /// The `%F?` file stats, worked out at most once a minute.
    pub file_stats: std::cell::RefCell<Option<(Instant, std::sync::Arc<crate::filebase::FileStats>)>>,
    /// The message menu's figures, likewise, and while nothing's been
    /// posted or read since (`MsgBase::changes`).
    pub msg_stats: std::cell::RefCell<Option<(Instant, u64, std::sync::Arc<crate::msgcenter::MsgFigures>)>>,
    /// The doors menu's figures, likewise.
    pub door_stats: std::cell::RefCell<Option<(Instant, std::sync::Arc<crate::sysdata::DoorStats>)>>,
    /// `~XX` / `^NN` markers from the last displayed file: (x, y, colour).
    pub positions: HashMap<[u8; 2], (u16, u16, Color)>,
    pub positions2: HashMap<u8, (u16, u16, Color)>,
    /// "Continuous" was chosen at a pause: no more pauses until the next
    /// menu prompt (the original's `PausePos := 0`).
    pub nonstop: bool,
    /// FILETEST is on screen: upload messages go in its status field.
    pub test_screen: bool,
    /// The row login prompts share, each replacing the last, when they
    /// follow a lightbar menu (MATRIX) rather than scrolling it away.
    pub login_line: Option<u16>,
    /// Scripts running inside scripts (`%!` in a string a script shows).
    #[cfg(feature = "lua")]
    pub(crate) script_depth: u8,
    /// Email tags for this call: what new email is tagged with (`E/`),
    /// and, when `use_tag` is on (`E+`), the only tag read (`E\`).
    pub email_tag: u16,
    /// Passworded message areas the caller has given the password for this
    /// call (by storage key): only those show in scans and searches.
    pub opened_areas: std::collections::HashSet<String>,
    pub read_tag: u16,
    pub use_tag: bool,
    /// Times the SysOp has been paged this call (`numPaged`).
    pub pages: u32,
    /// Drawing the STOCK screen for `|SA`.
    including: bool,
    /// The AI chat's help bar, redrawn as the chat scrolls.
    #[cfg(feature = "ai")]
    pub(crate) ai_bar: String,
    /// The colour the AI chat's text is in, to resume after the bar.
    #[cfg(feature = "ai")]
    pub(crate) ai_ink: String,
    /// New: the style AI chat's text is in now (`markup.rs`).
    #[cfg(feature = "ai")]
    pub(crate) ai_style: Option<(crate::markup::Ink, bool)>,
    /// New: a one-line note for the help line of the next list or menu
    /// (`Session::note`).
    pub(crate) note: Option<String>,
    /// New: the last `read_string` with `esc_cancels` ended with Esc (its
    /// empty answer isn't an Enter).
    pub(crate) escaped: bool,
    /// The context the AI conversation has used, for the bar (`AiContext`).
    #[cfg(feature = "ai")]
    pub(crate) ai_ctx: String,
    /// The chat's rows under the header, kept to redraw them (new).
    #[cfg(feature = "ai")]
    pub(crate) ai_win: Option<crate::aichat::AiWindow>,
    /// New: the chat screen's effects (AICHAT.FX), run while it waits.
    #[cfg(feature = "ai")]
    pub(crate) ai_amb: Option<crate::ambient::Ambient>,
    /// New: the line the editor opens with in place of a hint (once).
    pub(crate) editor_note: Option<String>,
    /// New: what this call did, for the board's memory of the caller.
    pub(crate) seen: crate::aimemory::Seen,
}

/// Run a caller from connect to disconnect.
/// Display state carried from token to token: the `{NN`/`}NN` limit in
/// force (as the original's `mciLimit`, it holds for every value after it
/// in the string or screen) and a `|**` return position.
#[derive(Default)]
pub(crate) struct DisplayState {
    limit: Option<(usize, bool)>,
    return_to: Option<(u16, u16, Color)>,
}

pub async fn run(state: Arc<BbsState>, mut io: Box<dyn Stream>, caller: Caller) {
    let who = caller.remote.map_or("?".into(), |a| a.to_string());
    // Connection limits and bans, before a node is taken.
    let _ticket = match caller.remote {
        Some(addr) => match state.guard.admit(&state.config().security, addr.ip()) {
            Ok(t) => Some(t),
            Err(why) => {
                tracing::info!("{who}: refused via {}: {why}", caller.transport);
                let _ = io.write_all(b"\r\nToo many connections from your address; try again later.\r\n").await;
                let _ = io.flush().await;
                return;
            }
        },
        None => None,
    };
    let Some(node) = state.nodes.acquire(caller.transport) else {
        tracing::info!("{who}: all nodes busy");
        let _ = io.write_all(b"\r\nAll nodes are busy, try again later.\r\n").await;
        let _ = io.flush().await;
        return;
    };
    tracing::info!(
        "{who}: node {} via {} (TERM={:?}, {:?})",
        node.number(),
        caller.transport,
        caller.term,
        *caller.size.borrow()
    );
    let mut s = Session::new(state, io, caller, node);
    if let Err(e) = s.probe_terminal().await {
        tracing::info!("{who}: disconnected ({e})");
        return;
    }
    // Boxed, as are each command, the thread list and the reader: a
    // session's futures nest each other's state, and unboxed the whole
    // call came to 186 KB, which overflowed a debug build's worker stack.
    let result = Box::pin(s.call()).await;
    s.logoff();
    match result {
        Ok(()) => tracing::info!("{who}: {} logged off", s.user.handle),
        Err(e) if is_hangup(&e) => tracing::info!("{who}: {} disconnected ({e})", s.user.handle),
        Err(e) => tracing::warn!("{who}: session error: {e}"),
    }
    let _ = s.flush().await;
}

/// What a session started from the WFC screen does.
pub enum LocalJob {
    /// `local logon`: a call like any other, at the console.
    Logon,
    /// A SysOp command, run as the SysOp, e.g. `*C`.
    Command(&'static str, &'static str),
    /// `wfcLaunchMenu`: the SysOp, at a menu asked for.
    LaunchMenu,
    /// `wfcViewMailbox`: read another user's email.
    Mailbox,
    /// `wfcEditTextfile`: a file in the text directory, in the editor.
    EditText,
}

/// A session at the console (`LocalIO`), for the WFC screen.
pub async fn run_local(state: Arc<BbsState>, mut io: Box<dyn Stream>, caller: Caller, what: LocalJob) {
    let Some(node) = state.nodes.acquire(caller.transport) else {
        let _ = io.write_all(b"\r\nAll nodes are busy.\r\n").await;
        let _ = io.flush().await;
        return;
    };
    let mut s = Session::new(state, io, caller, node);
    let result = match what {
        LocalJob::Logon => {
            let r = Box::pin(s.call()).await;
            s.logoff();
            r
        }
        what => Box::pin(s.local_sysop(what)).await,
    };
    match result {
        Ok(()) => {}
        Err(e) if is_hangup(&e) => {}
        Err(e) => tracing::warn!("local: session error: {e}"),
    }
    let _ = s.flush().await;
}

/// The error that ends a call for a forced event.
pub fn kicked() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionReset, "disconnected for an event")
}

fn is_hangup(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::TimedOut
    )
}

impl Session {
    /// New: ask a 16-colour ANSI terminal what it can show, from its
    /// answers rather than its TERM (IcyTerm sends `ANSI`). Each question
    /// is followed by a cursor report, so we know when it has answered.
    /// One that answers `ESC[c` at all is set to a 24-bit colour and
    /// asked what it has (DECRQSS, `DCS $ q m ST`): the colour coming back
    /// means 24-bit, `38;5` the 256. One answering as CTerm does
    /// (`ESC[=...c`: SyncTERM, IcyTerm) that didn't say is asked for its
    /// features (`ESC[<c`), as Synchronet does: an extended palette (6)
    /// means CTerm's colours, 24-bit included.
    async fn probe_terminal(&mut self) -> io::Result<()> {
        if self.screen.charset() != Charset::Cp437 || self.screen.truecolor() || self.screen.palette256() {
            return Ok(());
        }
        let wait = std::time::Duration::from_secs(2);
        self.screen.write_bytes(b"\x1b[c");
        self.sync_output(wait, 0).await?;
        if self.device_attrs.is_empty() {
            return Ok(());
        }
        // Raw: the screen would turn the colour into a DOS one. The line
        // is wiped after, in case a terminal printed the question.
        self.screen.write_raw(b"\x1b[0;38;2;1;2;3m\x1bP$qm\x1b\\\x1b[0m\r\x1b[K");
        self.sync_output(wait, 0).await?;
        let said = |s: &Self, any: &[&str]| s.device_attrs.iter().any(|a| a.contains("$r") && any.iter().any(|x| a.contains(x)));
        let (mut truecolor, palette256) = (said(self, &["38;2;1;2;3", "38:2::1:2:3", "38:2:1:2:3"]), said(self, &["38;5;", "38:5:"]));
        if !truecolor && !palette256 && self.device_attrs.iter().any(|a| a.starts_with('=')) {
            self.screen.write_bytes(b"\x1b[<c");
            self.sync_output(wait, 0).await?;
            let features = self.device_attrs.iter().find_map(|a| a.strip_prefix("<0;")).unwrap_or("");
            truecolor = features.split(';').any(|f| f == "6");
        }
        let shows = if truecolor { "24-bit color" } else if palette256 { "256 colors" } else { "16 colors" };
        tracing::info!("node {}: the terminal answers {:?}: {shows}", self.node.number(), self.device_attrs);
        self.screen.set_truecolor(truecolor);
        self.screen.set_palette256(palette256 && !truecolor);
        Ok(())
    }

    /// A session for tests: `user` logged in on a node, talking to nobody.
    #[cfg(test)]
    pub(crate) fn for_test(state: Arc<BbsState>, user: User) -> Self {
        let (io, _) = tokio::io::duplex(1 << 16);
        let (_, size) = watch::channel((80, 24));
        let caller = Caller { remote: None, transport: "test", login_name: String::new(), key_user: None, term: "ansi".into(), size };
        let node = state.nodes.acquire("test").expect("a free node");
        let mut s = Session::new(state, Box::new(io), caller, node);
        s.user = user.clone();
        *s.user_base.borrow_mut() = Some(user);
        s
    }

    fn new(state: Arc<BbsState>, io: Box<dyn Stream>, caller: Caller, node: NodeGuard) -> Self {
        let charset = match state.config().terminal.charset {
            CharsetPref::Cp437 => Charset::Cp437,
            CharsetPref::Utf8 => Charset::Utf8,
            CharsetPref::Auto => {
                let term = caller.term.to_ascii_lowercase();
                if state.config().terminal.cp437_terms.iter().any(|t| t.eq_ignore_ascii_case(&term)) {
                    Charset::Cp437
                } else {
                    Charset::Utf8
                }
            }
        };
        let rows = caller.size.borrow().1;
        let truecolor = state.config().terminal.truecolor_terms.iter().any(|t| t.eq_ignore_ascii_case(&caller.term));
        let palette256 = state.config().terminal.palette256_terms.iter().any(|t| t.eq_ignore_ascii_case(&caller.term));
        let mut screen = Screen::new(charset, rows);
        screen.set_truecolor(truecolor);
        screen.set_palette256(palette256 && charset == Charset::Cp437);
        let user = User { palette: state.config().colors.clone(), ..User::default() };
        Self {
            gallery_order: crate::gallery::Order::default(),
            state,
            caller,
            node,
            hold_node_msgs: false,
            hold_account_changes: false,
            user,
            user_base: Default::default(),
            checking_time: Default::default(),
            io,
            screen,
            input: VecDeque::new(),
            last_cr: false,
            idle_since: None,
            last_key: std::time::Instant::now(),
            screen_fx: None,
            cursor_asks: 0,
            cursor_reports: 0,
            report_tail: Vec::new(),
            refresh_on: None,
            bar_news: None,
            menu_help: None,
            device_attrs: Vec::new(),
            utf8_tail: Vec::new(),
            sent: 0,
            saved_color: Color::DEFAULT,
            time_on: Instant::now(),
            logged_in: false,
            hidden_call: false,
            file_stats: Default::default(),
            msg_stats: Default::default(),
            door_stats: Default::default(),
            hangup: false,
            menu: MenuState::default(),
            input_string: String::new(),
            ok_flag: false,
            menu_msg: 0,
            batch: Vec::new(),
            new_user: false,
            positions: HashMap::new(),
            positions2: HashMap::new(),
            nonstop: false,
            test_screen: false,
            login_line: None,
            #[cfg(feature = "lua")]
            script_depth: 0,
            email_tag: 0,
            opened_areas: Default::default(),
            read_tag: 0,
            use_tag: false,
            pages: 0,
            including: false,
            #[cfg(feature = "ai")]
            ai_bar: String::new(),
            #[cfg(feature = "ai")]
            ai_ink: String::new(),
            #[cfg(feature = "ai")]
            ai_style: None,
            note: None,
            escaped: false,
            #[cfg(feature = "ai")]
            ai_ctx: String::new(),
            #[cfg(feature = "ai")]
            ai_win: None,
            #[cfg(feature = "ai")]
            ai_amb: None,
            editor_note: None,
            seen: Default::default(),
        }
    }

    pub fn strings(&self) -> Arc<Strings> {
        self.state.strings()
    }

    pub fn emulation_name(&self) -> &'static str {
        match self.screen.charset() {
            Charset::Cp437 => "ANSI",
            Charset::Utf8 => "ANSI/UTF-8",
        }
    }

    pub fn minutes_used(&self) -> u32 {
        (self.time_on.elapsed().as_secs() / 60) as u32
    }

    /// Minutes in a day: what a caller with no time limit always has left.
    pub const UNLIMITED_MINUTES: u32 = 1440;

    /// New: has this caller no time limit (`[bbs] unlimited_time_acs`)?
    pub fn time_unlimited(&self) -> bool {
        let acs = self.state.config().bbs.unlimited_time_acs.clone();
        if acs.trim().is_empty() || !self.logged_in || self.checking_time.get() {
            return false;
        }
        self.checking_time.set(true);
        let yes = self.acs(&acs);
        self.checking_time.set(false);
        yes
    }

    /// `mTimeLeft`; a whole day for a caller with no limit.
    pub fn minutes_left(&self) -> u32 {
        if self.time_unlimited() {
            return Self::UNLIMITED_MINUTES;
        }
        self.user.time_today.saturating_sub(self.minutes_used())
    }

    pub fn is_sysop(&self) -> bool {
        self.acs(&self.state.config().acs.sysop.clone())
    }

    /// `acsOk`: does the current user pass an ACS expression?
    pub fn acs(&self, expr: &str) -> bool {
        self.acs_for(&self.user, expr)
    }

    /// New: the zone this caller sees times in (theirs, or the board's).
    pub fn zone(&self) -> crate::clock::Zone {
        crate::clock::Zone::parse(&self.user.time_zone).unwrap_or_else(crate::clock::board)
    }

    /// A board time as this caller sees it.
    pub fn local(&self, at: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
        crate::clock::convert(at, &self.zone())
    }

    /// Now, as this caller sees it.
    pub fn now(&self) -> chrono::DateTime<chrono::FixedOffset> {
        self.zone().now()
    }

    /// `mUserAcs`: does another user pass an ACS expression?
    pub fn acs_for(&self, u: &User, expr: &str) -> bool {
        let now = crate::clock::now();
        let level_req = self.state.level(u.level).map_or(0, |l| l.post_call as u32);
        let ratio = (u.posts * 100).checked_div(u.total_calls).unwrap_or(0);
        let env = AcsEnv {
            local: false,
            ok_flag: self.ok_flag,
            age: u.age(now.date_naive()),
            conf_msg: u.conf_msg,
            conf_file: u.conf_file,
            dsl: u.dsl,
            sl: u.sl,
            flags: u.flags.clone(),
            sex: u.sex,
            hour: now.hour(),
            area_code: u.area_code().to_string(),
            posts: u.posts,
            minutes_left: if u.id == self.user.id { self.minutes_left() } else { u.time_today },
            user_id: u.id,
            level: u.level,
            weekday: now.weekday().num_days_from_sunday(),
            minute_of_day: now.hour() * 60 + now.minute(),
            ratio_ok: ratio >= level_req,
            node: self.node.number() as u32,
            calls: u.total_calls,
        };
        acs::check(expr, &env)
    }

    /// Persist the user at the end of the call, charging time used.
    fn logoff(&mut self) {
        if !self.logged_in || self.user.id == 0 {
            return;
        }
        // An edit that hasn't reached us yet (no key since) still counts.
        let _ = self.absorb_account_changes();
        // A caller with no limit keeps their level's time as it was: the
        // day's minutes aren't theirs to carry.
        if !self.time_unlimited() {
            self.user.time_today = self.minutes_left();
        }
        if let Err(e) = self.save_own_user() {
            tracing::error!("saving user {}: {e}", self.user.handle);
        }
        // Folded into what the board remembers of them, after they've gone.
        crate::aimemory::remember(self.state.clone(), self.user.clone(), std::mem::take(&mut self.seen));
    }

    // ---- output -------------------------------------------------------

    pub async fn flush(&mut self) -> io::Result<()> {
        let out = self.screen.take_output();
        if out.is_empty() {
            return Ok(());
        }
        self.sent += out.len() as u64;
        // Say so if the caller's side stops taking output (a full SSH
        // window, a stuck terminal): the session waits here, keys unread.
        let (who, sent) = (&self.user.handle, self.sent);
        let started = std::time::Instant::now();
        let io = &mut self.io;
        let mut warned = false;
        // In pieces, so a slow link that's moving is told from one that's
        // gone: five minutes with no piece through and it's taken as gone
        // (hung up), rather than holding the node, and anything it has
        // locked, until TCP gives up.
        const STALL: std::time::Duration = std::time::Duration::from_secs(300);
        for piece in out.chunks(16 * 1024).map(Some).chain(std::iter::once(None)) {
            let mut write = std::pin::pin!(async {
                match piece {
                    Some(p) => io.write_all(p).await,
                    None => io.flush().await,
                }
            });
            let deadline = tokio::time::Instant::now() + STALL;
            loop {
                tokio::select! {
                    r = &mut write => {
                        r?;
                        break;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(10)), if !warned => {
                        tracing::warn!("{who}: output stalled for 10s, {sent} bytes sent this call");
                        warned = true;
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        tracing::warn!("{who}: no output went for 5 minutes; hanging up");
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "output stalled"));
                    }
                }
            }
        }
        if warned {
            tracing::warn!("{who}: output moving again after {:.0}s", started.elapsed().as_secs_f64());
        }
        Ok(())
    }

    pub fn newline(&mut self) {
        self.screen.newline();
    }

    pub fn set_slot(&mut self, slot: Slot) {
        self.screen.set_color(self.user.palette.get(slot));
    }

    pub fn clear_screen(&mut self) {
        self.screen.clear_screen();
    }

    /// Write text without interpreting any codes (`oWrite`), and without
    /// control characters.
    pub fn write(&mut self, s: &str) {
        self.screen.write_text(s);
    }

    /// Display a prompt string with codes (`oStr`). Also handles the
    /// leading `%|` (centre) and `%>` (right-justify) prefixes, `%%FILE`
    /// (display a text file instead) and `%!SCRIPT args` (run a script).
    pub async fn show(&mut self, s: &str) -> io::Result<()> {
        if let Some(file) = s.strip_prefix("%%") {
            self.show_file(file.trim()).await?;
            return Ok(());
        }
        // `%!name args`: run a script instead (`iplExecute`); without
        // scripts built in (`lua`), nothing.
        if let Some(cmd) = s.strip_prefix("%!") {
            #[cfg(feature = "lua")]
            return Box::pin(self.script_command(cmd)).await;
            #[cfg(not(feature = "lua"))]
            {
                let _ = cmd;
                return Ok(());
            }
        }
        let (align, s) = match s.get(..2) {
            Some("%|") => (Some(true), &s[2..]),
            Some("%>") => (Some(false), &s[2..]),
            _ => (None, s),
        };
        // Caller-typed parts (`lit`) keep only their colour codes.
        let toks = tokenize_str(s, Source::String);
        if let Some(center) = align {
            let len = self.tokens_len(&toks) as u16;
            let col = if center { (80u16.saturating_sub(len) / 2).clamp(1, 39) } else { 80u16.saturating_sub(len).max(1) };
            self.screen.pos_x(col);
        }
        let mut st = DisplayState::default();
        for tok in toks {
            self.display_token(tok, Source::String, None, &mut st).await?;
        }
        self.display_end(st);
        Ok(())
    }

    /// `oStrLn`.
    pub async fn show_line(&mut self, s: &str) -> io::Result<()> {
        self.show(s).await?;
        self.newline();
        Ok(())
    }

    /// `oString` / `oStringLn`: show a string from the table.
    pub async fn say(&mut self, name: &str) -> io::Result<()> {
        let s = self.strings().get(name).to_string();
        self.show(&s).await
    }

    pub async fn say_line(&mut self, name: &str) -> io::Result<()> {
        let s = self.strings().get(name).to_string();
        self.show_line(&s).await
    }

    /// Visible width of text with display codes in it.
    pub fn visible_len(&self, bytes: &[u8]) -> usize {
        self.tokens_len(&tokenize(bytes, Source::String))
    }

    fn tokens_len(&self, toks: &[Token]) -> usize {
        toks.iter()
            .map(|t| match t {
                Token::Text(b) => b.len(),
                Token::Mci(c) => self.mci_value(*c).map_or(3, |v| v.chars().count()),
                _ => 0,
            })
            .sum()
    }

    /// The display engine shared by prompts and text files. `waves`
    /// supplies values for `~XX` codes; `keep_positions` keeps the markers
    /// of an earlier file (repeat rows).
    async fn display(&mut self, src: &[u8], source: Source, waves: Option<&Waves>, keep_positions: bool) -> io::Result<()> {
        if source == Source::File && !keep_positions {
            self.positions.clear();
            self.positions2.clear();
        }
        let mut st = DisplayState::default();
        let toks = tokenize(src, source);
        for (i, tok) in toks.iter().enumerate() {
            if *tok == Token::AlignRight {
                let w = self.rest_of_line_len(&toks[i + 1..], waves, st.limit);
                self.screen.pos_x(80u16.saturating_sub(w as u16).max(1));
                continue;
            }
            self.display_token(tok.clone(), source, waves, &mut st).await?;
        }
        self.display_end(st);
        Ok(())
    }

    /// How wide what's left of a line of a screen will be on screen.
    fn rest_of_line_len(&self, toks: &[Token], waves: Option<&Waves>, limit: Option<(usize, bool)>) -> usize {
        let (mut n, mut limit) = (0, limit);
        // As `display_token` draws them: a `{NN`/`}NN` holds for every
        // value after it, and colour codes in a value take no room.
        let fit = |v: String, limit: &mut Option<(usize, bool)>| {
            let w = fluxbbs_term::codes::width(&v);
            match *limit {
                Some((n, true)) => n,
                Some((n, false)) => w.min(n),
                None => w,
            }
        };
        for t in toks {
            match t {
                Token::Text(b) => {
                    let line = b.split(|&c| c == b'\r' || c == b'\n').next().unwrap_or_default();
                    n += fluxbbs_term::codes::visible_bytes(line);
                    if line.len() < b.len() {
                        break;
                    }
                }
                Token::Mci(c) => n += fit(self.mci_value(*c).unwrap_or_default(), &mut limit),
                Token::Ext(c) => n += waves.and_then(|w| w.get(c)).map_or(0, |v| fit(v.clone(), &mut limit)),
                Token::Limit(w) => limit = Some((*w as usize, false)),
                Token::Pad(w) => limit = Some((*w as usize, true)),
                _ => {}
            }
        }
        n
    }

    /// One token of [`Session::display`].
    pub(crate) async fn display_token(&mut self, tok: Token, source: Source, waves: Option<&Waves>, st: &mut DisplayState) -> io::Result<()> {
        match tok {
            Token::Text(b) => self.screen.write_bytes(&b),
            Token::Ctrl(c) => self.control(c).await?,
            Token::Mci(code) => match self.mci_value(code) {
                Some(mut v) => {
                    if let Some((n, pad)) = st.limit {
                        v = if pad { resize(&v, n) } else { squish(&v, n) };
                    }
                    // In files the value may carry |XX colour codes
                    // (oStrCtr); in prompts it's written as-is.
                    if source == Source::File {
                        self.write_with_controls(&v);
                    } else {
                        self.write(&v);
                    }
                }
                // `mciProcessMCICode` swallows any %XX in a screen, known or
                // not (Menuset #1 has a stray %SC); prompts keep it, so
                // "50%" and the like still read right.
                None if source == Source::File && code.iter().all(u8::is_ascii_alphabetic) => {}
                None => {
                    self.screen.write_bytes(b"%");
                    self.screen.write_bytes(&code);
                }
            },
            Token::PosX(n) => self.screen.pos_x(n as u16),
            Token::PosY(n) => self.screen.pos_y(n as u16),
            Token::Limit(n) => st.limit = Some((n as usize, false)),
            Token::Pad(n) => st.limit = Some((n as usize, true)),
            Token::SavePos => st.return_to = Some(self.here()),
            Token::Ext(code) => match waves.and_then(|w| w.get(&code)) {
                // sfWrite: limit/pad apply, |XX codes in the value work.
                // Where it went is kept too (new), for what's drawn after.
                Some(v) => {
                    let here = self.here();
                    self.positions.insert(code, here);
                    let v = match st.limit {
                        Some((n, true)) => resize(v, n),
                        Some((n, false)) => squish(v, n),
                        None => v.clone(),
                    };
                    self.write_with_controls(&v);
                }
                None => {
                    let here = self.here();
                    self.positions.insert(code, here);
                }
            },
            Token::Ext2(n) => {
                let here = self.here();
                self.positions2.insert(n, here);
            }
            // Only a whole screen (`display`) can measure the line.
            Token::AlignRight => {}
            Token::Font { spec, text } => self.draw_font(&spec, &text, waves),
        }
        Ok(())
    }

    /// New: `%[FONT.TDF,face,width,spacing]text%]`, the text (its codes filled in)
    /// set in a TheDraw font from `[paths] fonts` at the cursor, rows going
    /// down; the cursor ends under it, where it started. `face` is a face's
    /// number in the file (1 is the first) or its name, `width` the most
    /// columns it may take (to the right edge if left out), and `spacing`,
    /// if given, the columns between letters in place of the face's own. A text
    /// too wide for that, or a font that isn't there, is written plainly.
    /// Block and outline faces take the current color; color faces their own.
    fn draw_font(&mut self, spec: &[u8], text: &[u8], waves: Option<&Waves>) {
        let spec = String::from_utf8_lossy(spec);
        let mut parts = spec.split(',').map(str::trim);
        let file = parts.next().unwrap_or_default();
        let face = parts.next().unwrap_or("1");
        let (x0, y0, pen) = self.here();
        let width = parts.next().and_then(|w| w.parse::<u16>().ok()).unwrap_or(80u16.saturating_sub(x0) + 1);
        let spacing = parts.next().and_then(|s| s.parse::<u8>().ok());
        let mut word = String::new();
        for t in tokenize(text, Source::String) {
            let v = match t {
                Token::Text(b) => fluxbbs_term::cp437::decode(&b),
                Token::Mci(c) => self.mci_value(c).unwrap_or_default(),
                Token::Ext(c) => waves.and_then(|w| w.get(&c)).cloned().unwrap_or_default(),
                _ => continue,
            };
            // A value's own colour codes don't belong in a font.
            for t in tokenize_str(&v, Source::String) {
                if let Token::Text(b) = t {
                    word.push_str(&fluxbbs_term::cp437::decode(&b));
                }
            }
        }
        let cfg = self.state.config();
        let mut fonts = if file.is_empty() || file.contains("..") || file.contains('/') {
            Vec::new()
        } else {
            crate::tdf::read(&cfg.resolve(&cfg.paths.fonts).join(file))
        };
        let font = match face.parse::<usize>() {
            Ok(n) => fonts.get_mut(n.saturating_sub(1)),
            Err(_) => fonts.iter_mut().find(|f| f.name.eq_ignore_ascii_case(face)),
        };
        let font = font.map(|f| {
            if let Some(s) = spacing {
                f.spacing = s;
            }
            &*f
        });
        let set = font.map(|f| (f.kind, f.set(&word))).filter(|(_, (_, w, _))| *w > 0 && *w <= width);
        let Some((kind, (cells, _, h))) = set else {
            self.write(&squish(&word, width as usize));
            return;
        };
        for c in cells {
            self.screen.goto(x0 + c.x, y0 + c.y);
            self.screen.set_color(if kind == 2 { Color::new(c.fg, c.bg) } else { pen });
            self.screen.write_bytes(&[c.ch]);
        }
        self.screen.goto(x0, y0 + h);
        self.screen.set_color(pen);
    }

    /// Finish a display: go back to a `|**` saved position.
    pub(crate) fn display_end(&mut self, st: DisplayState) {
        if let Some((x, y, c)) = st.return_to {
            self.screen.goto(x, y);
            self.screen.set_color(c);
        }
    }

    fn here(&self) -> (u16, u16, Color) {
        (self.screen.where_x(), self.screen.where_y(), self.screen.color())
    }

    /// `sfGotoPos`: move to a `~XX` marker from the last file.
    pub fn goto_pos(&mut self, code: [u8; 2]) -> bool {
        match self.positions.get(&code).copied() {
            Some((x, y, c)) => {
                self.screen.goto(x, y);
                self.screen.set_color(c);
                true
            }
            None => false,
        }
    }

    /// `sfGotoPos2`: move to a `^NN` marker, falling back to `~NN`.
    pub fn goto_pos2(&mut self, n: u8) -> bool {
        match self.positions2.get(&n).copied() {
            Some((x, y, c)) => {
                self.screen.goto(x, y);
                self.screen.set_color(c);
                true
            }
            None => self.goto_pos(pos_code(n)),
        }
    }

    /// `oStrCtr`: text with `|XX` colour codes only. It shows what callers
    /// typed, where cursor moves and clears would wreck other screens (2.0
    /// fixed "'|XX' would kill multinode chat").
    pub(crate) fn write_with_controls(&mut self, s: &str) {
        for tok in tokenize_user(&cp437::encode(s)) {
            match tok {
                Token::Ctrl(c) => {
                    self.control_now(c);
                }
                Token::Text(b) => self.screen.write_bytes(&b),
                _ => {}
            }
        }
    }

    /// Apply a control code that needs no input. Returns false for the
    /// interactive ones (pauses, delays).
    fn control_now(&mut self, c: Control) -> bool {
        let cur = self.screen.color();
        match c {
            Control::Fore(n) => self.screen.set_color(Color { fore: n, ..cur }),
            Control::Back(n) => self.screen.set_color(Color { back: n, ..cur }),
            Control::Theme(n) => {
                if let Some(col) = self.user.palette.index(n) {
                    self.screen.set_color(col);
                }
            }
            Control::SaveColor => self.saved_color = cur,
            Control::LoadColor => self.screen.set_color(self.saved_color),
            Control::Up(n) => self.screen.move_up(n as u16),
            Control::Down(n) => self.screen.move_down(n as u16),
            Control::Left(n) => self.screen.move_left(n as u16),
            Control::Right(n) => self.screen.move_right(n as u16),
            Control::ClearScreen => self.screen.clear_screen(),
            Control::ClearEol => self.screen.clear_eol(),
            Control::Home => self.screen.goto(1, 1),
            Control::SaveCursor => self.screen.save_cursor(),
            Control::RestoreCursor => self.screen.restore_cursor(),
            Control::Newline => self.screen.newline(),
            Control::PauseCtl => {}
            Control::Delay(_) | Control::PromptKey | Control::ReadKey | Control::Pause | Control::StockArt => return false,
        }
        true
    }

    async fn control(&mut self, c: Control) -> io::Result<()> {
        if self.control_now(c) {
            return Ok(());
        }
        match c {
            Control::Delay(ms) => self.delay(ms as u64).await?,
            Control::PromptKey => self.prompt_key().await?,
            // New: a screen with effects beside it animates while it waits.
            Control::ReadKey if !self.plain() => match self.screen_fx.take() {
                Some(fx) => {
                    self.flush().await?;
                    let mut amb = crate::ambient::Ambient::new(&self.screen, &self.paced(&fx));
                    self.read_key_ambient(&mut amb, false).await?;
                    self.screen_fx = Some(fx);
                }
                None => {
                    self.read_key().await?;
                }
            },
            Control::ReadKey => {}
            Control::Pause => self.pause().await?,
            Control::StockArt => self.include_stock().await?,
            _ => {}
        }
        Ok(())
    }

    pub async fn delay(&mut self, ms: u64) -> io::Result<()> {
        self.flush().await?;
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        Ok(())
    }

    /// `oPause`: the "more" prompt, honouring the user's pause setting.
    pub async fn pause(&mut self) -> io::Result<()> {
        if self.user.pause && !self.plain() {
            let s = self.strings().get("Pause").to_string();
            self.prompt_line(&s).await?;
        }
        Ok(())
    }

    /// `oPromptKey`: "press any key", always.
    pub async fn prompt_key(&mut self) -> io::Result<()> {
        let s = self.strings().get("HitAKey").to_string();
        self.prompt_line(&s).await
    }

    /// Show a one-line prompt, wait for a key, then erase the prompt if
    /// the SysOp wants that (`RemovePause`).
    async fn prompt_line(&mut self, prompt: &str) -> io::Result<()> {
        // New: plain text never waits for a key.
        if self.plain() {
            return Ok(());
        }
        let color = self.screen.color();
        let start = self.screen.where_x();
        // Boxed: prompts are displayed by the same engine that calls us.
        Box::pin(self.show(prompt)).await?;
        self.read_key().await?;
        if self.state.config().prompts.remove_pause {
            let len = self.screen.where_x().saturating_sub(start);
            self.screen.backspace(len);
        } else {
            self.newline();
        }
        self.screen.set_color(color);
        Ok(())
    }

    // ---- text files ---------------------------------------------------

    /// `sfShowTextFile`: display a file from the text directory. Returns
    /// false if no such file exists.
    pub async fn show_file(&mut self, name: &str) -> io::Result<bool> {
        self.show_file_with(name, &Waves::new()).await
    }

    /// `|SA`: the STOCK screen, in place, keeping the markers of the screen
    /// it's in. Not inside itself.
    async fn include_stock(&mut self) -> io::Result<()> {
        if self.including {
            return Ok(());
        }
        let Some(data) = self.read_text_file("STOCK").await? else { return Ok(()) };
        self.including = true;
        let r = Box::pin(self.display(&data, Source::File, None, true)).await;
        self.including = false;
        r
    }

    /// Display a screen whose `~XX` codes take values from `waves`.
    pub async fn show_file_with(&mut self, name: &str, waves: &Waves) -> io::Result<bool> {
        // New: plain text shows no screens; callers fall back to text.
        if self.plain() {
            return Ok(false);
        }
        let Some(path) = self.find_text_file(name) else {
            return Ok(false);
        };
        self.show_path(&path, waves).await?;
        Ok(true)
    }

    /// A screen file as found, with its effects (`NAME.FX`) at its pauses.
    pub(crate) async fn show_path(&mut self, path: &Path, waves: &Waves) -> io::Result<()> {
        // Screens made with modern tools (UTF-8, LF) work too.
        let data = fluxbbs_term::cp437::dos_art(&tokio::fs::read(path).await?).into_owned();
        let fx = crate::ambient::sidecar(path).filter(|_| self.animating());
        let outer = std::mem::replace(&mut self.screen_fx, fx);
        let r = self.display(&data, Source::File, Some(waves), false).await;
        self.screen_fx = outer;
        r
    }

    async fn read_text_file(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        // Resolve first: the session can't be borrowed across the await.
        match self.find_text_file(name) {
            // Screens made with modern tools (UTF-8, LF) work too.
            Some(path) => Ok(Some(fluxbbs_term::cp437::dos_art(&tokio::fs::read(&path).await?).into_owned())),
            None => Ok(None),
        }
    }

    /// Start a repeat screen: show `{prefix}TOP` and load MID (and HIL if
    /// `highlight`). None if the screens don't exist; callers then fall
    /// back to plain text.
    pub async fn repeat_begin(&mut self, prefix: &str, highlight: bool) -> io::Result<Option<Repeat>> {
        self.repeat_begin_with(prefix, highlight, &Waves::new()).await
    }

    /// [`Self::repeat_begin`] with values for TOP's `~XX` codes.
    pub async fn repeat_begin_with(&mut self, prefix: &str, highlight: bool, top_waves: &Waves) -> io::Result<Option<Repeat>> {
        let has = |s: &Self, part: &str| s.text_file_exists(&format!("{prefix}{part}"));
        // Plain text: the built-in layout (art's columns are cursor moves).
        if self.plain() || !(has(self, "TOP") && has(self, "MID") && has(self, "BOT")) {
            return Ok(None);
        }
        let mid = self.read_text_file(&format!("{prefix}MID")).await?.unwrap_or_default();
        let hil = if highlight { self.read_text_file(&format!("{prefix}HIL")).await? } else { None };
        let top = self.read_text_file(&format!("{prefix}TOP")).await?.unwrap_or_default();
        let bot = self.read_text_file(&format!("{prefix}BOT")).await?.unwrap_or_default();
        let bot_lines = (bot.iter().filter(|&&b| b == b'\n').count() as u16).max(1);
        self.show_file_with(&format!("{prefix}TOP"), top_waves).await?;
        self.goto_pos(*b"TL");
        // Count the screen from the top if the file cleared it; otherwise
        // (it carries on below whatever was there) just its own lines, as
        // `oUpPause(ansiRows)` did.
        let clears = top.windows(4).any(|w| w == b"\x1b[2J");
        let lines = if clears { self.screen.where_y() } else { top.iter().filter(|&&b| b == b'\n').count() as u16 + 1 };
        let bar = bot.windows(3).any(|w| w == b"~PC");
        Ok(Some(Repeat { prefix: prefix.into(), mid, hil, lines, bot_lines, bar }))
    }

    /// Show one row. Returns false if the caller quit at a pause.
    pub async fn repeat_row(&mut self, r: &mut Repeat, waves: &Waves, highlight: bool) -> io::Result<bool> {
        let src = match (&r.hil, highlight) {
            (Some(hil), true) => hil.clone(),
            _ => r.mid.clone(),
        };
        self.display(&src, Source::File, Some(waves), true).await?;
        if self.screen.where_x() != 1 {
            self.newline();
        }
        r.lines += 1;
        self.more(&mut r.lines).await
    }

    /// One row drawn at line `y`, for a list with a moving bar: no pause.
    pub async fn repeat_line(&mut self, r: &Repeat, waves: &Waves, highlight: bool, y: u16) -> io::Result<()> {
        let src = match (&r.hil, highlight) {
            (Some(hil), true) => hil.clone(),
            _ => r.mid.clone(),
        };
        self.screen.goto(1, y);
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
        self.display(&src, Source::File, Some(waves), true).await
    }

    /// A list with a moving bar (new; the original asked for a number):
    /// `rows` in a window between `{prefix}TOP` and `{prefix}BOT`, the bar
    /// drawn with `{prefix}HIL`, starting on `rows[start]`. Arrows,
    /// PgUp/PgDn and Home/End move it, scrolling the window; Enter or
    /// right picks; Esc, left or backspace go up when `can_go_up` (Esc
    /// otherwise stays); `q` quits. Typing a number (a label) moves the bar to it, as typed, and
    /// a digit no label goes on with starts a new number. `header` is
    /// for boards without the screens.
    pub async fn area_bar(&mut self, spec: &BarSpec<'_>) -> io::Result<BarPick> {
        let news = self.bar_news.take();
        if self.plain() {
            return self.area_bar_plain(spec).await;
        }
        let BarSpec { prefix, header, rows, labels, start, help, ask, can_go_up, keys } = *spec;
        let mut cur = start.min(rows.len().saturating_sub(1));
        let (mut first, mut top, mut height) = (0usize, 1u16, 1usize);
        let mut prompt = (1u16, 1u16);
        let mut typed = String::new();
        let mut rep = None;
        let mut redraw = true;
        let find = |typed: &str| typed.parse::<u32>().ok().and_then(|l| labels.iter().find(|&&(label, _)| label == l)).map(|r| r.1);
        // New: `/` and some text moves the bar to the next row with it in.
        let mut search: Option<String> = None;
        let matching = |q: &str, from: usize| {
            let q = q.to_lowercase();
            (0..rows.len()).map(|i| (from + i) % rows.len()).find(|&i| rows[i].plain.to_lowercase().contains(&q))
        };
        loop {
            if redraw {
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.clear_screen();
                rep = self.repeat_begin(prefix, true).await?;
                // A prompt bar BOT holds the help and prompt itself.
                let bar = rep.as_ref().is_some_and(|r| r.bar);
                let bot_lines = match &rep {
                    Some(r) if r.bar => r.bot_lines - 1,
                    Some(r) => r.bot_lines,
                    None => {
                        self.newline();
                        self.set_slot(Slot::Info);
                        self.show_line(header).await?;
                        self.rule().await?;
                        1
                    }
                };
                if self.screen.where_x() != 1 {
                    self.newline();
                }
                top = self.screen.where_y();
                // Room for BOT, the help line and the prompt below.
                height = (self.screen.rows() + 1).saturating_sub(top + bot_lines + 2).max(3) as usize;
                first = first.min(cur).max((cur + 1).saturating_sub(height));
                self.bar_window(rep.as_ref(), rows, first, cur, top, height).await?;
                self.screen.goto(1, top + height as u16);
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                match &rep {
                    Some(_) => {
                        self.show_file(&format!("{prefix}BOT")).await?;
                    }
                    None => self.rule().await?,
                }
                // A note waiting takes the help line's place, this once.
                let noted = self.note.take();
                let help = noted.as_deref().unwrap_or(help);
                if bar {
                    self.goto_pos(*b"HP");
                    // Kept to its row: the bar is under it.
                    let room = 80usize.saturating_sub(self.screen.where_x() as usize);
                    self.show(&fluxbbs_term::codes::squish(help, room)).await?;
                    self.goto_pos(*b"PC");
                } else {
                    if self.screen.where_x() != 1 {
                        self.newline();
                    }
                    // Kept to one line: a help line that wrapped would
                    // scroll the screen under the list, and every redraw
                    // after would land a row off.
                    let fits = self.visible_len(&cp437::encode(help)) <= 79;
                    let help = if fits { help.to_string() } else { crate::editor::strip_colours(help).chars().take(79).collect() };
                    self.show_line(&help).await?;
                    if !ask.is_empty() {
                        self.say(ask).await?;
                    }
                }
                prompt = (self.screen.where_x(), self.screen.where_y());
                redraw = false;
            }
            self.screen.goto(prompt.0, prompt.1);
            if rep.as_ref().is_some_and(|r| r.bar) {
                // The bar's slot: the end of what's typed, in its place.
                let shown: String = typed.chars().rev().take(BAR_SLOT).collect::<Vec<_>>().into_iter().rev().collect();
                self.write(&format!("{shown:<BAR_SLOT$}"));
                self.screen.goto(prompt.0 + shown.chars().count() as u16, prompt.1);
            } else {
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.write(&typed);
                self.screen.clear_eol();
            }
            let key = match news {
                Some(seen) => match self.read_key_or_post(seen).await? {
                    Some(k) => k,
                    None => return Ok(BarPick::Refresh(rows.get(cur).map_or(0, |r| r.n))),
                },
                None => self.read_key().await?,
            };
            let prev = cur;
            let last = rows.len().saturating_sub(1);
            // While searching, text goes to the search and `/` again finds
            // the next; esc ends it where the bar is, enter picks that row.
            let mut searched = false;
            if let Some(q) = search.as_mut() {
                searched = true;
                match key {
                    Key::Esc => search = None,
                    Key::Backspace => {
                        if q.pop().is_none() {
                            search = None;
                        }
                    }
                    Key::Char(b'/') if !q.is_empty() => {
                        if let Some(i) = matching(q, cur + 1) {
                            cur = i;
                        }
                    }
                    Key::Char(c @ 0x20..=0x7E) if q.len() < 30 => {
                        q.push(c as char);
                        if let Some(i) = matching(q, cur) {
                            cur = i;
                        }
                    }
                    Key::Char(_) => {}
                    _ => {
                        search = None;
                        searched = false;
                    }
                }
                typed = search.as_ref().map_or(String::new(), |q| format!("/{q}"));
            }
            if !searched {
                match key {
                    Key::Char(b'/') if !rows.is_empty() => {
                        search = Some(String::new());
                        typed = "/".into();
                    }
                    Key::Up => cur = cur.saturating_sub(1),
                    Key::Down => cur = (cur + 1).min(last),
                    Key::PgUp => cur = cur.saturating_sub(height),
                    Key::PgDn => cur = (cur + height).min(last),
                    Key::Home => cur = 0,
                    Key::End => cur = last,
                    // A digit carries on the number while some label
                    // starts that way, else starts a new one.
                    Key::Char(c) if c.is_ascii_digit() => {
                        let more = format!("{typed}{}", c as char);
                        typed = if labels.iter().any(|(l, _)| l.to_string().starts_with(&more)) { more } else { (c as char).to_string() };
                        let here = find(&typed).and_then(|n| rows.iter().position(|r| r.n == n));
                        if let Some(i) = here {
                            cur = i;
                        }
                        // Nothing longer to come and the bar's on it: start
                        // afresh. (One elsewhere, in another folder, stays
                        // typed for enter to open.)
                        let longer = labels.iter().any(|(l, _)| l.to_string().len() > typed.len() && l.to_string().starts_with(typed.as_str()));
                        // Or a number nothing has: it goes rather than sit there.
                        if !longer && (here.is_some() || find(&typed).is_none()) {
                            typed.clear();
                        }
                    }
                    Key::Backspace if !typed.is_empty() => {
                        typed.pop();
                    }
                    Key::Left | Key::Backspace if can_go_up => return Ok(BarPick::Up),
                    Key::Enter | Key::Right => {
                        self.newline();
                        if !typed.is_empty() {
                            return Ok(find(&typed).map_or(BarPick::Stay, BarPick::Area));
                        }
                        return Ok(rows.get(cur).map_or(BarPick::Stay, |r| BarPick::Area(r.n)));
                    }
                    Key::Esc => {
                        self.newline();
                        return Ok(if can_go_up { BarPick::Up } else { BarPick::Stay });
                    }
                    k if k.upper().is_some_and(|c| keys.contains(c)) => {
                        self.newline();
                        let c = k.upper().unwrap_or(' ');
                        return Ok(BarPick::Key(c, rows.get(cur).map_or(0, |r| r.n)));
                    }
                    k if k.upper() == Some('Q') => {
                        self.newline();
                        return Ok(BarPick::Quit);
                    }
                    _ => {}
                }
                if !matches!(key, Key::Char(_) | Key::Backspace) {
                    typed.clear();
                }
            }
            if cur == prev {
                continue;
            }
            if cur < first || cur >= first + height {
                first = if cur < first { cur } else { cur + 1 - height };
                self.bar_window(rep.as_ref(), rows, first, cur, top, height).await?;
            } else {
                self.bar_row(rep.as_ref(), &rows[prev], top + (prev - first) as u16, false).await?;
                self.bar_row(rep.as_ref(), &rows[cur], top + (cur - first) as u16, true).await?;
            }
        }
    }

    async fn bar_window(&mut self, rep: Option<&Repeat>, rows: &[BarRow], first: usize, cur: usize, top: u16, height: usize) -> io::Result<()> {
        for i in 0..height {
            let y = top + i as u16;
            match rows.get(first + i) {
                Some(row) => self.bar_row(rep, row, y, first + i == cur).await?,
                None => {
                    self.screen.goto(1, y);
                    self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                    self.screen.clear_eol();
                }
            }
        }
        Ok(())
    }

    async fn bar_row(&mut self, rep: Option<&Repeat>, row: &BarRow, y: u16, hi: bool) -> io::Result<()> {
        match rep {
            Some(r) => self.repeat_line(r, &row.waves, hi, y).await,
            None => {
                self.screen.goto(1, y);
                self.set_slot(if hi { Slot::TextHi } else { Slot::Text });
                self.write(&row.plain);
                self.screen.clear_eol();
                Ok(())
            }
        }
    }

    pub async fn repeat_end(&mut self, r: Repeat) -> io::Result<()> {
        self.show_file(&format!("{}BOT", r.prefix)).await?;
        Ok(())
    }

    pub fn text_file_exists(&self, name: &str) -> bool {
        self.find_text_file(name).is_some()
    }

    /// `sfGetTextFile`: with no extension, prefer `.ANS` then `.ASC`. An ANSI
    /// screen may have random variants (`.AN1`-`.AN9`) and a weekday
    /// override (`.A0S`-`.A6S`, Sunday = 0). The caller's theme (text
    /// library) is looked in first, then the text directory, then the
    /// stock screens (`stock_text`), then a library for any screen stock
    /// lacks (`stock_theme`).
    pub(crate) fn find_text_file(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        let dirs = self.text_dirs();
        if name.contains('.') {
            return dirs.iter().find_map(|d| find_ci(d, name));
        }
        dirs.iter()
            .find_map(|d| find_ansi(d, name))
            .or_else(|| dirs.iter().find_map(|d| find_ci(d, &format!("{name}.ASC"))))
    }
}

impl Session {
    /// Where screens are looked for, in order: the caller's theme, the
    /// board's text directory, the stock screens, the stock theme.
    pub(crate) fn text_dirs(&self) -> Vec<PathBuf> {
        let cfg = self.state.config();
        let set = |p: &PathBuf| (!p.as_os_str().is_empty()).then(|| cfg.resolve(p));
        self.theme_dir()
            .into_iter()
            .chain([cfg.resolve(&cfg.paths.text)])
            .chain(set(&cfg.paths.stock_text))
            .chain(set(&cfg.paths.stock_theme))
            .collect()
    }
}

/// `NAME.ANS` in `dir`, or one of its random or weekday variants.
fn find_ansi(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let mut pick = find_ci(dir, &format!("{name}.ANS"))?;
    let variants: Vec<PathBuf> = (1..=9).filter_map(|i| find_ci(dir, &format!("{name}.AN{i}"))).collect();
    if !variants.is_empty() {
        let i = rand::rng().random_range(0..=variants.len());
        if i > 0 {
            pick = variants[i - 1].clone();
        }
    }
    let fname = pick.file_name()?.to_str()?.to_string();
    let mut chars: Vec<char> = fname.chars().collect();
    let n = chars.len();
    chars[n - 2] = char::from_digit(mci::day_of_week(), 10)?;
    let dow: String = chars.into_iter().collect();
    Some(find_ci(dir, &dow).unwrap_or(pick))
}

/// `~NN` marker code for a number.
pub fn pos_code(n: u8) -> [u8; 2] {
    [b'0' + n / 10 % 10, b'0' + n % 10]
}

/// Case-insensitive lookup, since DOS files come with any casing. Gives
/// the name as it is on disk, even where the filesystem itself ignores
/// case (macOS), so rewriting the file keeps its name.
pub fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let names: Vec<String> =
        std::fs::read_dir(dir).ok()?.filter_map(Result::ok).filter_map(|e| e.file_name().into_string().ok()).collect();
    names
        .iter()
        .find(|f| *f == name)
        .or_else(|| names.iter().find(|f| f.eq_ignore_ascii_case(name)))
        .map(|f| dir.join(f))
}
