//! New: the MRC chat screen (`XR`), laid out as irssi lays out IRC: a
//! topic bar over the scrolling window (each line timed, MRC's pipe
//! colours shown), a status bar (the time, your name, the room and how
//! many are in it, MRC's size, what you've missed while scrolled back),
//! and the line you type on. `/join`, `/msg`, `/r`, `/me`, `/topic`,
//! `/names`, `/list`, `/clear`, `/help`, `/quit`; any other `/command`
//! goes to the MRC server. PgUp/PgDn scroll, Up/Down recall, Tab
//! completes a name, Esc leaves. What's shown for whom follows uMRC's
//! client (see `mrc.rs`).

use std::io;
use std::time::{Duration, Instant};

use fluxbbs_term::Color;
use tokio::sync::broadcast::error::TryRecvError;

use crate::input::Key;
use crate::mrc::{self, CTCP_ROOM, Event, Packet};
use crate::session::Session;

/// Lines kept to scroll back through.
const SCROLLBACK: usize = 1000;
/// The most typed at once (sent as packets of 140).
const INPUT_MAX: usize = 420;
const COLS: usize = 80;
/// The bars' colours.
const BAR: Color = Color::new(15, 1);
const BAR_DIM: Color = Color::new(11, 1);

/// The commands, for the list that drops up as `/` is typed, the hint
/// once one's chosen, and `/help`: name, what follows it, what it does.
/// The MRC server's own are here too; any other goes to it as typed.
const COMMANDS: &[(&str, &str, &str)] = &[
    ("join", "#room", "go to another room (made if no one's there)"),
    ("part", "", "back to #lobby"),
    ("msg", "name text", "a private message, to anyone on MRC"),
    ("r", "text", "answer whoever last messaged you"),
    ("me", "action", "an action: * you waves"),
    ("topic", "text", "set the room's topic"),
    ("names", "", "who's in the room"),
    ("list", "", "the rooms on MRC"),
    ("whoon", "", "everyone on MRC, every room"),
    ("chatters", "", "who's chatting"),
    ("info", "", "the boards on MRC"),
    ("motd", "", "the server's message of the day"),
    ("register", "password [email]", "claim your name on MRC"),
    ("identify", "password", "prove it's you, once registered"),
    ("server", "command", "a command to the MRC server: /server help"),
    ("clear", "", "empty your window"),
    ("help", "", "these commands"),
    ("quit", "", "leave chat (Esc does too)"),
];
/// Other names the commands answer to.
const ALIASES: &[(&str, &str)] = &[("j", "join"), ("m", "msg"), ("t", "msg"), ("query", "msg"), ("reply", "r"), ("who", "names"), ("users", "names"), ("rooms", "list"), ("cls", "clear"), ("?", "help"), ("q", "quit"), ("exit", "quit"), ("bye", "quit")];
/// Commands listed at once.
const SUGGEST_MAX: usize = 8;

/// The command a word names (an alias too).
fn command(word: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let w = word.to_ascii_lowercase();
    let name = ALIASES.iter().find(|(a, _)| *a == w).map_or(w.as_str(), |(_, n)| n);
    COMMANDS.iter().find(|c| c.0 == name)
}

/// While a command's name is being typed (`/jo`): those it could be.
fn suggestions(input: &str) -> Vec<&'static (&'static str, &'static str, &'static str)> {
    match input.strip_prefix('/') {
        Some(w) if !w.contains(' ') => {
            let w = w.to_ascii_lowercase();
            COMMANDS.iter().filter(|c| c.0.starts_with(&w)).take(SUGGEST_MAX).collect()
        }
        _ => Vec::new(),
    }
}

/// Once it's named (`/msg `): what it takes and does.
fn hint(input: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let (w, _) = input.strip_prefix('/')?.split_once(' ')?;
    command(w)
}

/// A line on screen: characters and their colours.
type Row = Vec<(char, Color)>;

/// MRC text with pipe colours (`|00`-`|15` foreground, `|16`-`|23`
/// background) as coloured characters, from `start`.
fn cells(text: &str, start: Color) -> Row {
    let mut color = start;
    let mut out = Vec::new();
    for piece in mrc::pieces(text) {
        match piece {
            mrc::Piece::Colour(n @ 0..=15) => color.fore = n,
            mrc::Piece::Colour(n @ 16..=23) => color.back = n - 16,
            mrc::Piece::Colour(_) => {}
            // Nothing that would drive the caller's terminal.
            mrc::Piece::Char(c) if mrc::is_controlling(c) => {}
            mrc::Piece::Char(c) => out.push((c, color)),
        }
    }
    out
}

/// A row cut into screen lines `width` wide, at spaces where it can;
/// lines after the first start `indent` in.
fn wrap(row: Row, width: usize, indent: usize) -> Vec<Row> {
    let mut out = Vec::new();
    let mut rest = row;
    let mut first = true;
    loop {
        let room = if first { width } else { width - indent };
        if rest.len() <= room {
            out.push(pad_indent(rest, if first { 0 } else { indent }));
            return out;
        }
        let cut = (1..room).rev().find(|&i| rest[i].0 == ' ').map_or(room, |i| i + 1);
        let tail = rest.split_off(cut);
        out.push(pad_indent(rest, if first { 0 } else { indent }));
        rest = tail;
        first = false;
    }
}

fn pad_indent(row: Row, indent: usize) -> Row {
    let mut r: Row = std::iter::repeat_n((' ', Color::DEFAULT), indent).collect();
    r.extend(row);
    r
}

/// A message cut into packets of 140 at most, at spaces where it can.
fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest: Vec<char> = text.trim().chars().collect();
    while !rest.is_empty() {
        if rest.len() <= mrc::BODY_MAX {
            out.push(rest.iter().collect());
            break;
        }
        let cut = (1..mrc::BODY_MAX).rev().find(|&i| rest[i] == ' ').unwrap_or(mrc::BODY_MAX);
        let tail = rest.split_off(cut);
        out.push(rest.iter().collect::<String>().trim_end().to_string());
        rest = tail.into_iter().skip_while(|&c| c == ' ').collect();
    }
    out
}

/// Is this server line a notice of someone coming or going (so the room's
/// list is asked for again)? Never one of its replies (`USERLIST:...`),
/// whatever names are in it.
fn someone_came_or_went(body: &str) -> bool {
    let reply = body.split_once(':').is_some_and(|(c, _)| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_uppercase()));
    !reply && ["Joining", "Leaving", "Timeout", "Rename", "Linked", "Unlink"].iter().any(|w| body.contains(w))
}

/// The chat as one caller sees it.
struct Chat {
    nick: String,
    site: String,
    room: String,
    topic: String,
    users: Vec<String>,
    lines: Vec<Row>,
    /// Lines up from the bottom being looked at (0: the newest).
    back: usize,
    /// Lines come in while scrolled back, and mentions or messages among them.
    missed: usize,
    mentions: usize,
    last_dm: String,
    input: String,
    history: Vec<String>,
    recall: Option<usize>,
    dirty: bool,
    /// When the room's list was last asked for.
    asked: Option<Instant>,
    /// Which of the commands dropping up is picked.
    pick: usize,
}

impl Chat {
    fn packet(&self, to_user: &str, to_room: &str, body: &str) -> Packet {
        Packet {
            from_user: self.nick.clone(),
            from_site: self.site.clone(),
            from_room: self.room.clone(),
            to_user: to_user.into(),
            msg_ext: String::new(),
            to_room: to_room.into(),
            body: body.into(),
        }
    }

    /// A command to the server: `CMD`, or `CMD arg` (`CMD:arg` if it ends
    /// in a colon).
    fn cmd(&self, cmd: &str, arg: &str) -> Packet {
        let body = if arg.is_empty() { cmd.to_string() } else if cmd.ends_with(':') { format!("{cmd}{arg}") } else { format!("{cmd} {arg}") };
        self.packet("SERVER", "", &body)
    }

    /// A line in the window, timed.
    fn add(&mut self, text: &str, mention: bool) {
        let now = crate::clock::now().format("%H:%M").to_string();
        let time = Color::new(if mention { 14 } else { 8 }, 0);
        let mut row: Row = now.chars().map(|c| (c, time)).collect();
        row.push((' ', Color::DEFAULT));
        row.extend(cells(text, Color::new(7, 0)));
        let rows = wrap(row, COLS, 6);
        if self.back > 0 {
            self.back += rows.len();
            self.missed += 1;
        }
        if mention {
            self.mentions += 1;
        }
        self.lines.extend(rows);
        if self.lines.len() > SCROLLBACK {
            let over = self.lines.len() - SCROLLBACK;
            self.lines.drain(..over);
            self.back = self.back.min(self.lines.len());
        }
        self.dirty = true;
    }

    /// One of the board's own notices (irssi's `-!-`).
    fn notice(&mut self, text: &str) {
        self.add(&format!("|11-|15!|11- |07{text}"), false);
    }

    fn mentions_me(&self, body: &str) -> bool {
        mrc::strip_pipes(body).to_lowercase().contains(&self.nick.to_lowercase())
    }
}

impl Session {
    /// `XR`: MRC chat.
    pub async fn mrc_chat(&mut self) -> io::Result<()> {
        let acs = self.state.config().mrc.acs.clone();
        if crate::aimemory::is_bot(&self.user) || !self.acs(&acs) {
            return self.say_line("MrcNoAccess").await;
        }
        let Some(nick) = mrc::nick_of(&self.user.handle) else {
            return self.say_line("MrcBadName").await;
        };
        if !self.state.mrc.status().up {
            return self.say_line("MrcDown").await;
        }
        let room = mrc::wire_name(&self.state.config().mrc.room).to_lowercase();
        let mut chat = Chat {
            nick,
            site: mrc::site(&self.state),
            room: if room.is_empty() { "lobby".into() } else { room },
            topic: String::new(),
            users: Vec::new(),
            lines: Vec::new(),
            back: 0,
            missed: 0,
            mentions: 0,
            last_dm: String::new(),
            input: String::new(),
            history: Vec::new(),
            recall: None,
            dirty: true,
            asked: None,
            pick: 0,
        };
        let mut events = self.state.mrc.subscribe();
        chat.notice("|07Type and Enter to talk to the room. |15/|07 shows the commands, |15/help|07 lists them, |15Esc|07 leaves.");
        self.mrc_enter(&chat);
        tracing::info!("{}: MRC chat as {}", self.user.handle, chat.nick);
        // Node messages come into the window, not over it.
        let held = std::mem::replace(&mut self.hold_node_msgs, true);
        let result = self.mrc_loop(&mut chat, &mut events).await;
        self.hold_node_msgs = held;
        // However it ends (a hangup too), MRC hears we've gone.
        let hub = &self.state.mrc;
        hub.send(&chat.packet("NOTME", "", &format!("|07- |12{} |04has left chat.", chat.nick)));
        hub.send(&chat.cmd("LOGOFF", ""));
        self.screen.write_bytes(b"\x1b[?25h");
        self.screen.reset_scroll_region();
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        self.full_screen_done();
        result
    }

    /// Say we're here, as MRC clients do on arriving.
    fn mrc_enter(&self, chat: &Chat) {
        let hub = &self.state.mrc;
        hub.send(&chat.cmd("IAMHERE", ""));
        hub.send(&chat.cmd("MOTD", ""));
        hub.send(&chat.cmd("TERMSIZE:", &format!("{COLS}x{}", self.screen.rows())));
        hub.send(&chat.packet("NOTME", "", &format!("|07- |11{} |03has arrived.", chat.nick)));
        hub.send(&chat.cmd("NEWROOM::", &chat.room));
        hub.send(&chat.cmd("USERLIST", ""));
    }

    async fn mrc_loop(&mut self, chat: &mut Chat, events: &mut tokio::sync::broadcast::Receiver<Event>) -> io::Result<()> {
        self.node.update(|n| n.activity = "MRC chat".into());
        self.screen.set_color(Color::DEFAULT);
        self.clear_screen();
        let mut here = Instant::now();
        let mut active = Instant::now();
        let mut drawn_at = String::new();
        loop {
            // What's come in.
            loop {
                match events.try_recv() {
                    Ok(Event::Packet(p)) => self.mrc_packet(chat, &p),
                    Ok(Event::Up) => {
                        chat.notice("|07Back on MRC.");
                        self.mrc_enter(chat);
                    }
                    Ok(Event::Down) => chat.notice("|12Lost the MRC server|07; reconnecting..."),
                    Err(TryRecvError::Lagged(n)) => chat.notice(&format!("|08({n} lines missed)")),
                    Err(_) => break,
                }
            }
            // A forced event (shutdown, the SysOp): go.
            if let Some(text) = self.node.take_kick() {
                chat.notice(&format!("|12{}", crate::editor::strip_colours(&text)));
                self.mrc_draw(chat);
                let _ = self.flush().await;
                return Err(crate::session::kicked());
            }
            // Node messages, in the window.
            for ev in self.node.take(false) {
                match ev {
                    crate::nodes::NodeEvent::Message { from, user, text, broadcast } => {
                        let name = if broadcast { "NodeBCreceived" } else { "NodeMsgReceived" };
                        let head = crate::editor::strip_colours(&self.strings().fmt(name, &[&user, &from.to_string()]));
                        chat.notice(&format!("|14{} |15{}", head.trim(), crate::editor::strip_colours(&text)));
                    }
                    crate::nodes::NodeEvent::Notice { text } => chat.notice(&crate::editor::strip_colours(&text)),
                    _ => {}
                }
            }
            // Idle as long as the board allows: gone, as at any prompt.
            let idle = self.idle_limit();
            if idle > 0 && active.elapsed() > Duration::from_secs(idle) {
                let msg = crate::editor::strip_colours(self.strings().get("TimeInactive"));
                chat.notice(&format!("|12{msg}"));
                self.mrc_draw(chat);
                let _ = self.flush().await;
                return Err(io::Error::new(io::ErrorKind::TimedOut, "inactivity timeout"));
            }
            // Still here every minute; away after ten idle.
            if here.elapsed() >= Duration::from_secs(59) {
                let away = active.elapsed() > Duration::from_secs(600);
                self.state.mrc.send(&chat.cmd("IAMHERE:", if away { "AWAY" } else { "ACTIVE" }));
                here = Instant::now();
            }
            let minute = crate::clock::now().format("%H:%M").to_string();
            if chat.dirty || minute != drawn_at {
                self.mrc_draw(chat);
                drawn_at = minute;
                chat.dirty = false;
                self.flush().await?;
            }
            if !self.key_waiting().await? {
                continue;
            }
            let key = self.read_key().await?;
            active = Instant::now();
            if !self.mrc_key(chat, key)? {
                return Ok(());
            }
        }
    }

    /// A key typed; false to leave.
    fn mrc_key(&mut self, chat: &mut Chat, key: Key) -> io::Result<bool> {
        let page = self.screen.rows().saturating_sub(4) as usize;
        // A command being named: the list takes the arrows, Tab and Enter.
        let sugg = suggestions(&chat.input);
        if !sugg.is_empty() {
            let pick = chat.pick.min(sugg.len() - 1);
            let fill = |c: &(&str, &str, &str)| if c.1.is_empty() { format!("/{}", c.0) } else { format!("/{} ", c.0) };
            match key {
                Key::Up => {
                    chat.pick = pick.checked_sub(1).unwrap_or(sugg.len() - 1);
                    chat.dirty = true;
                    return Ok(true);
                }
                Key::Down => {
                    chat.pick = (pick + 1) % sugg.len();
                    chat.dirty = true;
                    return Ok(true);
                }
                Key::Tab => {
                    chat.input = fill(sugg[pick]);
                    chat.pick = 0;
                    chat.dirty = true;
                    return Ok(true);
                }
                // Enter on a command that's whole and needs nothing more runs
                // it; otherwise it's filled in, to go on typing.
                Key::Enter => {
                    let typed = chat.input[1..].to_ascii_lowercase();
                    let whole = command(&typed).is_some_and(|c| c.1.is_empty() && (c.0 == typed || ALIASES.iter().any(|(a, _)| *a == typed)));
                    if !whole {
                        chat.input = fill(sugg[pick]);
                        chat.pick = 0;
                        chat.dirty = true;
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }
        match key {
            Key::Esc => return Ok(false),
            Key::Enter => {
                let line = std::mem::take(&mut chat.input);
                chat.recall = None;
                if !line.trim().is_empty() {
                    chat.history.push(line.clone());
                    if chat.history.len() > 50 {
                        chat.history.remove(0);
                    }
                    chat.back = 0;
                    chat.missed = 0;
                    chat.mentions = 0;
                    if !self.mrc_say(chat, line.trim()) {
                        return Ok(false);
                    }
                }
            }
            Key::Backspace => {
                chat.input.pop();
                chat.pick = 0;
            }
            // ^U: clear the line.
            Key::Char(0x15) => chat.input.clear(),
            Key::Up | Key::Down if !chat.history.is_empty() => {
                let n = chat.history.len();
                chat.recall = match (key, chat.recall) {
                    (Key::Up, None) => Some(n - 1),
                    (Key::Up, Some(i)) => Some(i.saturating_sub(1)),
                    (Key::Down, Some(i)) if i + 1 < n => Some(i + 1),
                    _ => None,
                };
                chat.input = chat.recall.map(|i| chat.history[i].clone()).unwrap_or_default();
            }
            Key::PgUp => chat.back = (chat.back + page).min(chat.lines.len().saturating_sub(page)),
            Key::PgDn => {
                chat.back = chat.back.saturating_sub(page);
                if chat.back == 0 {
                    chat.missed = 0;
                    chat.mentions = 0;
                }
            }
            Key::Tab => {
                // The last word, as a name in the room.
                let start = chat.input.rfind(' ').map_or(0, |i| i + 1);
                let word = chat.input[start..].to_lowercase();
                if !word.is_empty()
                    && let Some(u) = chat.users.iter().find(|u| u.to_lowercase().starts_with(&word))
                {
                    let sep = if start == 0 { ": " } else { " " };
                    chat.input = format!("{}{u}{sep}", &chat.input[..start]);
                }
            }
            Key::Char(c) if (0x20..0x7F).contains(&c) && chat.input.chars().count() < INPUT_MAX && c != b'~' => {
                chat.input.push(c as char);
                chat.pick = 0;
            }
            Key::Char(c @ 0x80..=0xFE) if chat.input.chars().count() < INPUT_MAX => chat.input.push(fluxbbs_term::cp437::TO_UNICODE[c as usize]),
            _ => {}
        }
        chat.dirty = true;
        Ok(true)
    }

    /// What was typed: a command, or something to say to the room; false
    /// to leave.
    fn mrc_say(&mut self, chat: &mut Chat, line: &str) -> bool {
        let state = self.state.clone();
        let hub = &state.mrc;
        let Some(cmdline) = line.strip_prefix('/') else {
            for part in chunks(line) {
                hub.send(&chat.packet("", &chat.room, &format!("|08<|15{}|08> |07{part}", chat.nick)));
            }
            return true;
        };
        let (cmd, args) = cmdline.split_once(' ').map_or((cmdline, ""), |(c, a)| (c, a.trim()));
        match cmd.to_ascii_lowercase().as_str() {
            "quit" | "q" | "exit" | "bye" => return false,
            "join" | "j" => {
                let room = mrc::wire_name(args.trim_start_matches('#')).to_lowercase();
                if room.is_empty() {
                    chat.notice("|14Join which room?");
                } else {
                    hub.send(&chat.cmd(&format!("NEWROOM:{}:", chat.room), &room));
                    chat.room = room;
                    chat.topic.clear();
                    chat.users.clear();
                    hub.send(&chat.cmd("USERLIST", ""));
                }
            }
            "part" => {
                hub.send(&chat.cmd(&format!("NEWROOM:{}:", chat.room), "lobby"));
                chat.room = "lobby".into();
                hub.send(&chat.cmd("USERLIST", ""));
            }
            "msg" | "m" | "t" | "query" => match args.split_once(' ') {
                Some((to, text)) if !text.trim().is_empty() => self.mrc_dm(chat, to, text),
                _ => chat.notice("|14/msg name what to say"),
            },
            "r" | "reply" => {
                if chat.last_dm.is_empty() {
                    chat.notice("|14No one has messaged you yet.");
                } else if !args.is_empty() {
                    let to = chat.last_dm.clone();
                    self.mrc_dm(chat, &to, args);
                }
            }
            "me" if !args.is_empty() => {
                for part in chunks(args) {
                    hub.send(&chat.packet("", &chat.room, &format!("|15* |13{} {part}", chat.nick)));
                }
            }
            "topic" if !args.is_empty() => {
                hub.send(&chat.cmd(&format!("NEWTOPIC:{}:", chat.room), &mrc::strip_pipes(args)));
            }
            "names" | "who" | "users" => {
                hub.send(&chat.cmd("USERLIST", ""));
                let list = chat.users.join(" ");
                chat.notice(&format!("|07In |15#{}|07: |11{}", chat.room, if list.is_empty() { "(asking...)".into() } else { list }));
            }
            "list" | "rooms" => {
                hub.send(&chat.cmd("LIST", ""));
            }
            "clear" | "cls" => {
                chat.lines.clear();
                chat.back = 0;
            }
            "help" | "?" => {
                chat.notice("|15Commands|07 (type |15/|07 to pick one as you go):");
                for (name, args, what) in COMMANDS {
                    let left = if args.is_empty() { format!("/{name}") } else { format!("/{name} {args}") };
                    chat.notice(&format!("  |15{left:<24}|07{what}"));
                }
                chat.notice("|07To answer someone in the room, just type; |15Tab|07 finishes a name. |15PgUp/PgDn|07 scroll, |15Up/Down|07 your last lines.");
            }
            "server" if !args.is_empty() => {
                let (c, a) = args.split_once(' ').unwrap_or((args, ""));
                hub.send(&chat.cmd(&c.to_ascii_uppercase(), a));
            }
            other => {
                hub.send(&chat.cmd(&other.to_ascii_uppercase(), args));
                if matches!(other, "whoon" | "chatters") {
                    hub.send(&chat.cmd("USERLIST", ""));
                }
            }
        }
        true
    }

    fn mrc_dm(&mut self, chat: &mut Chat, to: &str, text: &str) {
        let to = mrc::wire_name(to);
        for part in chunks(text) {
            self.state.mrc.send(&chat.packet(&to, "", &format!("|15* |08(|15{}|08/|14DirectMsg|08) |07{part}", chat.nick)));
        }
        chat.add(&format!("|15* |08(|14DirectMsg|08->|15{to}|08) |07{text}"), false);
    }

    /// A packet from MRC: shown, or acted on, if it's for this caller.
    fn mrc_packet(&mut self, chat: &mut Chat, p: &Packet) {
        let (nick, room) = (chat.nick.clone(), chat.room.clone());
        let me = |s: &str| s.eq_ignore_ascii_case(&nick);
        // Whose it is at all (uMRC's bridge).
        let for_me = me(&p.to_user)
            || p.to_user.eq_ignore_ascii_case("CLIENT")
            || (p.to_user.eq_ignore_ascii_case("NOTME") && !me(&p.from_user))
            || p.to_user.eq_ignore_ascii_case("ALL")
            || p.to_user.is_empty();
        if !for_me {
            return;
        }
        let in_room = |r: &str| r.is_empty() || r.eq_ignore_ascii_case(&room);
        if p.from_user == "SERVER" && in_room(&p.to_room) {
            self.mrc_server(chat, p);
            // Someone came or went: the list again. Only for the server's
            // notices, never its replies (a name with "Linked" in it would
            // ask for the list forever).
            if someone_came_or_went(&p.body) {
                self.mrc_userlist(chat);
            }
            if p.to_user == "NOTME" && in_room(&p.from_room) {
                chat.notice(&p.body);
            }
        } else if p.to_room == CTCP_ROOM && p.from_room == CTCP_ROOM {
            self.mrc_ctcp(chat, p);
        } else if p.to_user == "NOTME" {
            if in_room(&p.from_room) && in_room(&p.to_room) {
                chat.add(&p.body, false);
                self.mrc_userlist(chat);
            }
        } else if (in_room(&p.to_room) && (p.to_user.is_empty() || me(&p.to_user))) || (p.to_room.is_empty() && p.to_user.is_empty()) {
            if !p.to_user.is_empty() && !p.from_user.is_empty() {
                // A message to us alone.
                chat.last_dm = p.from_user.clone();
                chat.add(&p.body, true);
            } else {
                let mention = !me(&p.from_user) && chat.mentions_me(&p.body);
                chat.add(&p.body, mention);
            }
        }
    }

    /// Ask for the room's list, once in two seconds at most.
    fn mrc_userlist(&self, chat: &mut Chat) {
        if chat.asked.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            chat.asked = Some(Instant::now());
            self.state.mrc.send(&chat.cmd("USERLIST", ""));
        }
    }

    /// What the server says (`CMD:args`).
    fn mrc_server(&mut self, chat: &mut Chat, p: &Packet) {
        let (cmd, arg) = p.body.split_once(':').unwrap_or((p.body.as_str(), ""));
        match cmd {
            "USERLIST" => {
                let mut users: Vec<String> = arg.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                users.sort_by_key(|u| u.to_lowercase());
                chat.users = users;
                chat.dirty = true;
            }
            "ROOMTOPIC" => {
                if let Some((_, topic)) = arg.split_once(':') {
                    chat.topic = topic.to_string();
                    chat.dirty = true;
                }
            }
            "USERROOM" => {
                let room = mrc::wire_name(arg);
                if !room.is_empty() {
                    chat.room = room;
                }
                chat.dirty = true;
            }
            "USERNICK" => {
                let Some(nick) = mrc::nick_of(arg) else { return };
                chat.nick = nick;
                chat.notice(&format!("|07The server has named you |15{arg}|07."));
                self.state.mrc.send(&chat.cmd("NICKCHANGED:", arg));
            }
            "TERMINATE" => {
                chat.notice(&format!("|12{arg}"));
                chat.input.clear();
                // Leaving is the caller's: the line says why.
            }
            "BANNER" => chat.add(&format!("|14* |13<|15#|13> |15{arg}"), false),
            "NOTIFY" => chat.add(&format!("|14* |14<|15!|14> |15{arg}"), false),
            "STATS" | "LATENCY" => chat.dirty = true,
            _ if (p.to_user.is_empty() || p.to_user.eq_ignore_ascii_case(&chat.nick)) && p.to_user != "NOTME" => chat.notice(&p.body),
            _ => {}
        }
    }

    /// CTCP: answered for this client (VERSION, TIME, PING, CLIENTINFO);
    /// answers to ours shown.
    fn mrc_ctcp(&mut self, chat: &mut Chat, p: &Packet) {
        let to_me = p.to_user.is_empty() || p.to_user.eq_ignore_ascii_case(&chat.nick);
        if let Some(rest) = p.body.strip_prefix("[CTCP] ")
            && to_me
        {
            // "[CTCP] sender target CMD args"
            let mut w = rest.splitn(3, ' ');
            let (_, _, what) = (w.next(), w.next(), w.next().unwrap_or(""));
            let up = what.to_ascii_uppercase();
            let reply = if up.starts_with("VERSION") {
                format!("VERSION flux/bbs {} ({}) MRC {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, mrc::PROTOCOL)
            } else if up.starts_with("TIME") {
                format!("TIME {}", crate::clock::now().format("%a %b %d %H:%M:%S %Y"))
            } else if up.starts_with("PING") {
                format!("PING {}", what.get(5..).unwrap_or(""))
            } else if up.starts_with("CLIENTINFO") {
                "CLIENTINFO VERSION TIME PING CLIENTINFO".to_string()
            } else {
                format!("Unsupported CTCP command: '{what}'")
            };
            let body = format!("[CTCP-REPLY] {} {reply}", chat.nick);
            let mut out = chat.packet(&p.from_user, CTCP_ROOM, &body);
            out.from_room = CTCP_ROOM.into();
            self.state.mrc.send(&out);
        } else if let Some(rest) = p.body.strip_prefix("[CTCP-REPLY] ")
            && p.to_user.eq_ignore_ascii_case(&chat.nick)
        {
            let text = rest.split_once(' ').map_or("", |(_, t)| t);
            chat.add(&format!("|15* |14[CTCP-REPLY] |10{} |15{text}", p.from_user), false);
        }
    }

    /// The whole screen: topic bar, window, status bar, input line.
    fn mrc_draw(&mut self, chat: &Chat) {
        let rows = self.screen.rows().max(10) as usize;
        let window = rows - 3;
        self.screen.write_bytes(b"\x1b[?25l");
        // Topic bar.
        let topic = format!(" #{}{}", chat.room, if chat.topic.is_empty() { String::new() } else { format!(": {}", mrc::strip_pipes(&chat.topic)) });
        self.mrc_bar(1, &[(topic, BAR)]);
        // The window: the lines ending `back` up from the newest.
        let end = chat.lines.len().saturating_sub(chat.back);
        let start = end.saturating_sub(window);
        let shown = &chat.lines[start..end];
        for i in 0..window {
            let y = (2 + i) as u16;
            self.screen.goto(1, y);
            // Top-aligned under the bar until the window's full.
            let row = shown.get(i);
            let mut used = 0;
            if let Some(row) = row {
                for &(ch, color) in row.iter().take(COLS) {
                    self.screen.set_color(color);
                    // `write_text`: never a control byte, whatever came in.
                    self.screen.write_text(&ch.to_string());
                    used += 1;
                }
            }
            self.screen.set_color(Color::DEFAULT);
            if used < COLS {
                self.screen.write_str(&" ".repeat(COLS - used));
            }
        }
        // The commands dropping up as one's named, or the hint for it.
        let sugg = suggestions(&chat.input);
        let rows_list: Vec<(String, String, String, bool)> = if !sugg.is_empty() {
            let pick = chat.pick.min(sugg.len() - 1);
            sugg.iter().enumerate().map(|(i, c)| (format!("/{}", c.0), c.1.to_string(), c.2.to_string(), i == pick)).collect()
        } else if let Some(c) = hint(&chat.input) {
            vec![(format!("/{}", c.0), c.1.to_string(), c.2.to_string(), false)]
        } else {
            Vec::new()
        };
        for (i, (name, args, what, on)) in rows_list.iter().enumerate() {
            let y = (rows - 1 - rows_list.len() + i) as u16;
            let bg = if *on { 1 } else { 0 };
            self.screen.goto(1, y);
            self.screen.set_color(Color::new(8, bg));
            self.screen.write_str(if *on { " \u{25ba} " } else { "   " });
            self.screen.set_color(Color::new(15, bg));
            self.screen.write_text(&format!("{name:<11}"));
            self.screen.set_color(Color::new(11, bg));
            self.screen.write_text(&format!("{args:<18}"));
            self.screen.set_color(Color::new(if *on { 15 } else { 7 }, bg));
            let room = COLS - 3 - 11 - 18;
            let what: String = what.chars().take(room).collect();
            self.screen.write_text(&format!("{what:<room$}"));
        }
        // Status bar.
        let st = self.state.mrc.status();
        let now = crate::clock::now().format("%H:%M").to_string();
        let mut parts = vec![
            ("[".to_string(), BAR_DIM),
            (now, BAR),
            ("] [".into(), BAR_DIM),
            (chat.nick.clone(), BAR),
            ("] [".into(), BAR_DIM),
            (format!("#{}({})", chat.room, chat.users.len()), BAR),
            ("] [".into(), BAR_DIM),
            (if st.up { format!("MRC {} users/{} bbses", st.users, st.bbses) } else { "MRC down".into() }, BAR),
            ("]".into(), BAR_DIM),
        ];
        if chat.back > 0 {
            parts.push((" [".into(), BAR_DIM));
            parts.push((format!("Act: {}{}", chat.missed, if chat.mentions > 0 { format!(", {} for you", chat.mentions) } else { String::new() }), Color::new(14, 1)));
            parts.push(("] -more-".into(), BAR_DIM));
        }
        self.mrc_bar((rows - 1) as u16, &parts);
        // Input line: its end, if it's longer than there's room for.
        let prompt = format!("[#{}] ", chat.room);
        let room = COLS.saturating_sub(prompt.chars().count() + 1).max(10);
        let shown: String = { let n = chat.input.chars().count(); chat.input.chars().skip(n.saturating_sub(room)).collect() };
        self.screen.goto(1, rows as u16);
        self.screen.set_color(Color::new(3, 0));
        self.screen.write_str(&prompt);
        self.screen.set_color(Color::new(15, 0));
        self.screen.write_text(&format!("{shown:<room$}"));
        self.screen.goto((prompt.chars().count() + shown.chars().count() + 1) as u16, rows as u16);
        self.screen.write_bytes(b"\x1b[?25h");
    }

    /// A bar the width of the screen, in its colours.
    fn mrc_bar(&mut self, y: u16, parts: &[(String, Color)]) {
        self.screen.goto(1, y);
        let mut used = 0;
        for (text, color) in parts {
            let text: String = text.chars().take(COLS - used).collect();
            used += text.chars().count();
            self.screen.set_color(*color);
            self.screen.write_text(&text);
        }
        self.screen.set_color(BAR);
        self.screen.write_str(&" ".repeat(COLS - used));
        self.screen.set_color(Color::DEFAULT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_wrapping_and_chunks() {
        let r = cells("|03<|11phrac|03> |07hi|16x", Color::new(7, 0));
        let text: String = r.iter().map(|c| c.0).collect();
        assert_eq!(text, "<phrac> hix");
        assert_eq!((r[0].1.fore, r[1].1.fore, r[8].1.fore), (3, 11, 7));
        assert_eq!(r[10].1.back, 0);
        let long: Row = "word ".repeat(40).chars().map(|c| (c, Color::DEFAULT)).collect();
        let w = wrap(long, 80, 6);
        assert!(w.len() == 3 && w.iter().all(|r| r.len() <= 80) && w[1][..6].iter().all(|c| c.0 == ' '));
        let c = chunks(&"abc ".repeat(80));
        assert!(c.len() == 3 && c.iter().all(|p| p.chars().count() <= mrc::BODY_MAX && !p.ends_with(' ')));
        assert_eq!(chunks("  hello  "), ["hello"]);
        // Commands drop up as they're named, then hint at what they take.
        assert_eq!(suggestions("/").len(), SUGGEST_MAX);
        assert_eq!(suggestions("/jo").iter().map(|c| c.0).collect::<Vec<_>>(), ["join"]);
        assert_eq!(suggestions("/m").iter().map(|c| c.0).collect::<Vec<_>>(), ["msg", "me", "motd"]);
        assert!(suggestions("/msg Zed").is_empty() && suggestions("hello").is_empty());
        assert_eq!(hint("/msg Zed hi").map(|c| c.1), Some("name text"));
        assert_eq!(hint("/j #hacking").map(|c| c.0), Some("join"));
        assert!(hint("/nonsense x").is_none() && hint("just talking").is_none());
        // The server's notices bring the list again; its replies never do.
        assert!(someone_came_or_went("|07- |11Bob |03is Joining |15#lobby"));
        assert!(!someone_came_or_went("USERLIST:LinkedLarry,Timeout_Tim,phrac"));
        assert!(!someone_came_or_went("ROOMTOPIC:lobby:Unlinked again"));
        // A remote user's ESC in CP437's glyph for it is dropped.
        let r = cells("|07hi \u{2190}[2J\u{2022} \u{e9}\u{2554}", Color::new(7, 0));
        assert_eq!(r.iter().map(|c| c.0).collect::<String>(), "hi [2J \u{e9}\u{2554}");
    }
}
