//! One caller's session: output, the display engine (`oStr` /
//! `sfShowFile`), and session state shared by logon, menus and commands.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::{Datelike, Timelike};
use iniquity_term::codes::{resize, squish};
use iniquity_term::{Charset, Color, Control, Screen, Slot, Source, Token, cp437, tokenize, tokenize_str, tokenize_user};
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
pub struct Repeat {
    prefix: String,
    mid: Vec<u8>,
    hil: Option<Vec<u8>>,
    lines: u16,
    /// Lines the BOT screen takes.
    pub bot_lines: u16,
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
    pub state: Arc<BbsState>,
    pub caller: Caller,
    pub node: NodeGuard,
    /// Keep node messages queued (full-screen editor, chat).
    pub hold_node_msgs: bool,
    pub user: User,
    pub(crate) io: Box<dyn Stream>,
    pub(crate) screen: Screen,
    pub(crate) input: VecDeque<u8>,
    pub(crate) last_cr: bool,
    /// Cursor position questions asked and answers seen, for `sync_output`.
    pub(crate) cursor_asks: u32,
    pub(crate) cursor_reports: u32,
    /// The start of a cursor report whose end hasn't arrived yet.
    pub(crate) report_tail: Vec<u8>,
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
    /// The message menu's figures, likewise.
    pub msg_stats: std::cell::RefCell<Option<(Instant, std::sync::Arc<crate::msgcenter::MsgFigures>)>>,
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
    pub(crate) ai_bar: String,
    /// The colour the AI chat's text is in, to resume after the bar.
    pub(crate) ai_ink: String,
    /// The context the AI conversation has used, for the bar (`AiContext`).
    pub(crate) ai_ctx: String,
    /// The chat's rows under the header, kept to redraw them (new).
    pub(crate) ai_win: Option<crate::aichat::AiWindow>,
}

/// Run a caller from connect to disconnect.
/// Display state carried from token to token: a pending `{NN`/`}NN`
/// limit and a `|**` return position.
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
    let result = s.call().await;
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
            let r = s.call().await;
            s.logoff();
            r
        }
        what => s.local_sysop(what).await,
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
        let mut screen = Screen::new(charset, rows);
        screen.set_truecolor(truecolor);
        let user = User { palette: state.config().colors.clone(), ..User::default() };
        Self {
            state,
            caller,
            node,
            hold_node_msgs: false,
            user,
            io,
            screen,
            input: VecDeque::new(),
            last_cr: false,
            cursor_asks: 0,
            cursor_reports: 0,
            report_tail: Vec::new(),
            utf8_tail: Vec::new(),
            sent: 0,
            saved_color: Color::DEFAULT,
            time_on: Instant::now(),
            logged_in: false,
            hidden_call: false,
            file_stats: Default::default(),
            msg_stats: Default::default(),
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
            script_depth: 0,
            email_tag: 0,
            opened_areas: Default::default(),
            read_tag: 0,
            use_tag: false,
            pages: 0,
            including: false,
            ai_bar: String::new(),
            ai_ink: String::new(),
            ai_ctx: String::new(),
            ai_win: None,
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

    /// `mTimeLeft`.
    pub fn minutes_left(&self) -> u32 {
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
        };
        acs::check(expr, &env)
    }

    /// Persist the user at the end of the call, charging time used.
    fn logoff(&mut self) {
        if !self.logged_in || self.user.id == 0 {
            return;
        }
        self.user.time_today = self.minutes_left();
        if let Err(e) = self.state.users.save(&self.user) {
            tracing::error!("saving user {}: {e}", self.user.handle);
        }
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
        let mut write = std::pin::pin!(async move {
            io.write_all(&out).await?;
            io.flush().await
        });
        let mut warned = false;
        loop {
            tokio::select! {
                r = &mut write => {
                    if warned {
                        tracing::warn!("{who}: output moving again after {:.0}s", started.elapsed().as_secs_f64());
                    }
                    return r;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)), if !warned => {
                    tracing::warn!("{who}: output stalled for 10s, {sent} bytes sent this call");
                    warned = true;
                }
            }
        }
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
        // `%!name args`: run a script instead (`iplExecute`).
        if let Some(cmd) = s.strip_prefix("%!") {
            return Box::pin(self.script_command(cmd)).await;
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
        for tok in tokenize(src, source) {
            self.display_token(tok, source, waves, &mut st).await?;
        }
        self.display_end(st);
        Ok(())
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
                Some(v) => {
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
        }
        Ok(())
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
            Control::ReadKey => {
                self.read_key().await?;
            }
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
        if self.user.pause {
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
        let Some(data) = self.read_text_file(name).await? else {
            return Ok(false);
        };
        self.display(&data, Source::File, Some(waves), false).await?;
        Ok(true)
    }

    async fn read_text_file(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        // Resolve first: the session can't be borrowed across the await.
        match self.find_text_file(name) {
            // Screens made with modern tools (UTF-8, LF) work too.
            Some(path) => Ok(Some(iniquity_term::cp437::dos_art(&tokio::fs::read(&path).await?).into_owned())),
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
        if !(has(self, "TOP") && has(self, "MID") && has(self, "BOT")) {
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
        Ok(Some(Repeat { prefix: prefix.into(), mid, hil, lines, bot_lines }))
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
        self.screen.set_color(iniquity_term::Color::DEFAULT);
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
                self.screen.set_color(iniquity_term::Color::DEFAULT);
                self.clear_screen();
                rep = self.repeat_begin(prefix, true).await?;
                let bot_lines = match &rep {
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
                self.screen.set_color(iniquity_term::Color::DEFAULT);
                match &rep {
                    Some(_) => {
                        self.show_file(&format!("{prefix}BOT")).await?;
                    }
                    None => self.rule().await?,
                }
                if self.screen.where_x() != 1 {
                    self.newline();
                }
                self.show_line(help).await?;
                if !ask.is_empty() {
                    self.say(ask).await?;
                }
                prompt = (self.screen.where_x(), self.screen.where_y());
                redraw = false;
            }
            self.screen.goto(prompt.0, prompt.1);
            self.screen.set_color(iniquity_term::Color::DEFAULT);
            self.write(&typed);
            self.screen.clear_eol();
            let key = self.read_key().await?;
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
                        if let Some(i) = find(&typed).and_then(|n| rows.iter().position(|r| r.n == n)) {
                            cur = i;
                        }
                        // Nothing longer to come: the bar's there, start afresh.
                        if !labels.iter().any(|(l, _)| l.to_string().len() > typed.len() && l.to_string().starts_with(typed.as_str())) {
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
                    self.screen.set_color(iniquity_term::Color::DEFAULT);
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
        let cfg = self.state.config();
        let set = |p: &PathBuf| (!p.as_os_str().is_empty()).then(|| cfg.resolve(p));
        let dirs: Vec<PathBuf> = self
            .theme_dir()
            .into_iter()
            .chain([cfg.resolve(&cfg.paths.text)])
            .chain(set(&cfg.paths.stock_text))
            .chain(set(&cfg.paths.stock_theme))
            .collect();
        if name.contains('.') {
            return dirs.iter().find_map(|d| find_ci(d, name));
        }
        dirs.iter()
            .find_map(|d| find_ansi(d, name))
            .or_else(|| dirs.iter().find_map(|d| find_ci(d, &format!("{name}.ASC"))))
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
