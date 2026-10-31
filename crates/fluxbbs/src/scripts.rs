//! Python scripts, in place of IPL (`IPLX.PAS`, the Iniquity Programming
//! Language). `-X name args` runs `scripts/name.py`, as `%!name args` in
//! any prompt string does; see docs/SCRIPTING.md.
//!
//! New: the script is a process of its own (`python3`), and the
//! `fluxbbs` module it imports (assets/fluxbbs.py, which comes with the
//! board) talks to the caller's session over a private socket, a JSON
//! request and its answer a line each: the session keeps sole charge of
//! the connection, and a script that crashes or hangs takes only itself
//! down. One that computes for [`WATCHDOG`] without calling the board is
//! stopped, as is one whose caller hangs up.

use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::input::{Case, Key, ReadOpts};
use crate::session::{Session, find_ci};
use crate::user::{User, fmt_date, parse_date};

/// How long a script may run without calling the board.
const WATCHDOG: Duration = Duration::from_secs(60);
/// Scripts may run scripts (`%!` in a string they show), this deep.
const MAX_DEPTH: u8 = 4;
/// The longest request line a script may send.
const LINE_MAX: u64 = 1 << 20;
/// The `fluxbbs` module, as scripts import it.
const MODULE: &str = include_str!("../assets/fluxbbs.py");
/// How a script is started: the module first (so `print` and `input` are
/// the caller's even without importing it), the script's folder on the
/// path, then the script as `__main__`.
const LAUNCH: &str = "import sys, os, runpy, fluxbbs\nsys.argv = sys.argv[1:]\nsys.path.insert(0, os.path.dirname(os.path.abspath(sys.argv[0])))\nrunpy.run_path(sys.argv[0], run_name='__main__')\n";

/// A value in a user record, as scripts see it.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Str(String),
    Num(f64),
    Bool(bool),
    List(Vec<String>),
}

/// `bbs.input` options.
#[derive(Debug, Clone, Default)]
pub struct InputOpts {
    max: usize,
    default: String,
    password: bool,
    case: Case,
    allowed: String,
    required: bool,
    /// Tab completes a handle (2.0's `/AC`).
    complete: bool,
}

/// What a script asks of the session.
#[derive(Debug)]
enum Req {
    Write(String),
    Show(String),
    Say(String),
    Newline(u16),
    Cls,
    ClrEol,
    Beep,
    Goto(u16, u16),
    Move(char, u16),
    Color(u8, u8),
    Where,
    ShowFile(String),
    Pause,
    GetKey,
    Input(InputOpts),
    YesNo(bool),
    MenuCmd(String, String),
    Mci(String),
    Expand(String),
    Acs(String),
    InMenu(String),
    User,
    SaveUser(Vec<(String, Field)>),
    Log(String),
    Get(String),
    Set(String, Option<String>),
    /// Done here, no screen: the board's codes out; cut or padded.
    NoColor(String),
    Pad(String, usize),
}

#[derive(Debug)]
enum Resp {
    Unit,
    Bool(bool),
    Str(String),
    MaybeStr(Option<String>),
    Pos(u16, u16),
    User(Vec<(&'static str, Field)>),
    /// The request was wrong (a bad field, say): an error in the script.
    Fail(String),
}

// ---- user records ---------------------------------------------------------

fn s(v: &str) -> Field {
    Field::Str(v.to_string())
}

fn n(v: impl Into<f64>) -> Field {
    Field::Num(v.into())
}

/// The user record as scripts see it (`userget`).
pub fn user_fields(u: &User) -> Vec<(&'static str, Field)> {
    vec![
        ("id", n(u.id)),
        ("handle", s(&u.handle)),
        ("real_name", s(&u.real_name)),
        ("location", s(&u.location)),
        ("address", s(&u.address)),
        ("zip_code", s(&u.zip_code)),
        ("phone", s(&u.phone)),
        ("note", s(&u.note)),
        ("sysop_note", s(&u.sysop_note)),
        ("sex", s(&u.sex.to_string())),
        ("birth_date", Field::Str(u.birth_date.map(|d| fmt_date(Some(d))).unwrap_or_default())),
        ("sl", n(u.sl)),
        ("dsl", n(u.dsl)),
        ("level", s(&u.level.to_string())),
        ("flags", Field::Str(u.flags.iter().collect())),
        ("hot_keys", Field::Bool(u.hot_keys)),
        ("expert", Field::Bool(u.expert)),
        ("pause", Field::Bool(u.pause)),
        ("quote", Field::Bool(u.quote)),
        ("yes_no_bars", Field::Bool(u.yes_no_bars)),
        ("page_length", n(u.page_length)),
        ("start_menu", s(&u.start_menu)),
        ("total_calls", n(u.total_calls)),
        ("calls_today", n(u.calls_today)),
        ("first_call", s(&fmt_date(u.first_call))),
        ("last_call", s(&fmt_date(u.last_call))),
        ("time_today", n(u.time_today)),
        ("time_per_day", n(u.time_per_day)),
        ("posts", n(u.posts)),
        ("email", n(u.email)),
        ("uploads", n(u.uploads)),
        ("downloads", n(u.downloads)),
        ("upload_kb", n(u.upload_kb as f64)),
        ("download_kb", n(u.download_kb as f64)),
        ("file_points", n(u.file_points)),
        ("cur_msg_area", n(u.cur_msg_area)),
        ("cur_file_area", n(u.cur_file_area)),
        ("conf_msg", n(u.conf_msg)),
        ("conf_file", n(u.conf_file)),
        ("autosig", Field::List(u.autosig.clone())),
        ("theme", s(&u.theme)),
        ("bank_time", n(u.bank_time)),
        ("bank_kb", n(u.bank_kb)),
    ]
}

fn want_str(name: &str, v: Field) -> Result<String, String> {
    match v {
        Field::Str(s) => Ok(s),
        Field::Num(n) => Ok(n.to_string()),
        _ => Err(format!("user.{name} must be a string")),
    }
}

fn want_num(name: &str, v: Field, max: f64) -> Result<f64, String> {
    match v {
        Field::Num(n) if n >= 0.0 && n <= max && n.fract() == 0.0 => Ok(n),
        Field::Str(s) => s.trim().parse::<f64>().ok().filter(|n| *n >= 0.0 && *n <= max).ok_or(format!("user.{name} must be 0-{max}")),
        _ => Err(format!("user.{name} must be a whole number, 0-{max}")),
    }
}

fn want_bool(name: &str, v: Field) -> Result<bool, String> {
    match v {
        Field::Bool(b) => Ok(b),
        _ => Err(format!("user.{name} must be true or false")),
    }
}

/// `userput`: change one field. The record number, handle, password and
/// the call and transfer counters aren't scripts' to change.
pub fn set_user_field(u: &mut User, name: &str, v: Field) -> Result<(), String> {
    let changed = |old: Field, new: &Field| old == *new;
    // Writing back a field unchanged is fine, whatever it is.
    if let Some((_, old)) = user_fields(u).into_iter().find(|(k, _)| *k == name)
        && changed(old, &v) {
            return Ok(());
        }
    match name {
        "real_name" => u.real_name = want_str(name, v)?,
        "location" => u.location = want_str(name, v)?,
        "address" => u.address = want_str(name, v)?,
        "zip_code" => u.zip_code = want_str(name, v)?,
        "phone" => u.phone = want_str(name, v)?,
        "note" => u.note = want_str(name, v)?,
        "sysop_note" => u.sysop_note = want_str(name, v)?,
        "start_menu" => u.start_menu = want_str(name, v)?,
        "theme" => u.theme = want_str(name, v)?,
        "sex" => {
            let s = want_str(name, v)?.to_ascii_uppercase();
            u.sex = match s.as_str() {
                "M" | "F" | "O" | "P" => s.chars().next().unwrap_or('P'),
                _ => return Err("user.sex must be \"M\", \"F\", \"O\" (other) or \"P\" (not given)".into()),
            };
        }
        "birth_date" => {
            let s = want_str(name, v)?;
            u.birth_date = if s.trim().is_empty() {
                None
            } else {
                Some(parse_date(&s).ok_or("user.birth_date must be MM/DD/YY")?)
            };
        }
        "level" => {
            let s = want_str(name, v)?.to_ascii_uppercase();
            match s.chars().collect::<Vec<_>>()[..] {
                [c @ 'A'..='Z'] => u.level = c,
                _ => return Err("user.level must be a letter A-Z".into()),
            }
        }
        "flags" => {
            let s = want_str(name, v)?.to_ascii_uppercase();
            if !s.chars().all(|c| c.is_ascii_uppercase()) {
                return Err("user.flags must be letters A-Z".into());
            }
            u.flags = s.chars().collect();
        }
        "sl" => u.sl = want_num(name, v, 255.0)? as u8,
        "dsl" => u.dsl = want_num(name, v, 255.0)? as u8,
        "page_length" => u.page_length = want_num(name, v, 200.0)?.max(1.0) as u16,
        "time_today" => u.time_today = want_num(name, v, 65_535.0)? as u32,
        "file_points" => u.file_points = want_num(name, v, 1e9)? as u32,
        "bank_time" => u.bank_time = want_num(name, v, 1e9)? as u32,
        "bank_kb" => u.bank_kb = want_num(name, v, 1e9)? as u32,
        "cur_msg_area" => u.cur_msg_area = want_num(name, v, 1e9)? as u32,
        "cur_file_area" => u.cur_file_area = want_num(name, v, 1e9)? as u32,
        "conf_msg" => u.conf_msg = want_num(name, v, 1e9)? as u32,
        "conf_file" => u.conf_file = want_num(name, v, 1e9)? as u32,
        "hot_keys" => u.hot_keys = want_bool(name, v)?,
        "expert" => u.expert = want_bool(name, v)?,
        "pause" => u.pause = want_bool(name, v)?,
        "quote" => u.quote = want_bool(name, v)?,
        "yes_no_bars" => u.yes_no_bars = want_bool(name, v)?,
        "autosig" => match v {
            Field::List(l) => u.autosig = l.into_iter().take(6).collect(),
            _ => return Err("user.autosig must be a list of lines".into()),
        },
        _ if user_fields(u).iter().any(|(k, _)| *k == name) => return Err(format!("user.{name} can't be changed by a script")),
        _ => return Err(format!("there's no user.{name}")),
    }
    Ok(())
}

fn key_name(k: Key) -> String {
    match k {
        Key::Char(c) => fluxbbs_term::cp437::decode(&[c]),
        Key::Enter => "enter".into(),
        Key::Backspace => "backspace".into(),
        Key::Tab => "tab".into(),
        Key::Esc => "esc".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PgUp => "pgup".into(),
        Key::PgDn => "pgdn".into(),
        Key::Ins => "ins".into(),
        Key::Del => "del".into(),
    }
}


// ---- the wire ----------------------------------------------------------------

fn arg_str(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

fn arg_num(a: &[Value], i: usize, default: u64) -> u64 {
    a.get(i).and_then(Value::as_u64).unwrap_or(default)
}

fn to_field(v: &Value) -> Result<Field, String> {
    Ok(match v {
        Value::Bool(b) => Field::Bool(*b),
        Value::Number(n) => Field::Num(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Field::Str(s.clone()),
        Value::Array(l) => Field::List(l.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)).collect()),
        Value::Null => Field::Str(String::new()),
        Value::Object(_) => return Err("can't store a dict in a user record".into()),
    })
}

fn from_field(f: Field) -> Value {
    match f {
        Field::Str(s) => Value::String(s),
        Field::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => json!(n as i64),
        Field::Num(n) => json!(n),
        Field::Bool(b) => Value::Bool(b),
        Field::List(l) => json!(l),
    }
}

fn input_opts(v: Option<&Value>) -> InputOpts {
    let mut o = InputOpts { max: 40, ..InputOpts::default() };
    let Some(t) = v.and_then(Value::as_object) else { return o };
    let flag = |k: &str| t.get(k).and_then(Value::as_bool).unwrap_or(false);
    if let Some(m) = t.get("max").and_then(Value::as_u64) {
        o.max = (m as usize).clamp(1, 255);
    }
    o.default = t.get("default").and_then(Value::as_str).unwrap_or("").to_string();
    o.password = flag("password");
    o.required = flag("required");
    o.complete = flag("complete");
    if flag("upper") {
        o.case = Case::Upper;
    }
    if flag("mixed") {
        o.case = Case::Mixed;
    }
    o.allowed = t.get("allowed").and_then(Value::as_str).unwrap_or("").to_string();
    o
}

/// A request line from a script, as what it asks.
fn parse_req(line: &str) -> Result<Req, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("not a request: {e}"))?;
    let op = v.get("op").and_then(Value::as_str).ok_or("a request needs an op")?;
    let empty = Vec::new();
    let a = v.get("a").and_then(Value::as_array).unwrap_or(&empty);
    let n16 = |i, d| arg_num(a, i, d).min(u16::MAX as u64) as u16;
    Ok(match op {
        "write" => Req::Write(arg_str(a, 0)),
        "show" => Req::Show(arg_str(a, 0)),
        "say" => Req::Say(arg_str(a, 0)),
        "newline" => Req::Newline(n16(0, 1)),
        "cls" => Req::Cls,
        "clreol" => Req::ClrEol,
        "beep" => Req::Beep,
        "gotoxy" => Req::Goto(n16(0, 1), n16(1, 1)),
        "move" => Req::Move(arg_str(a, 0).chars().next().unwrap_or('R'), n16(1, 1)),
        "color" => Req::Color((arg_num(a, 0, 7) & 15) as u8, (arg_num(a, 1, 0) & 7) as u8),
        "where" => Req::Where,
        "showfile" => Req::ShowFile(arg_str(a, 0)),
        "pause" => Req::Pause,
        "getkey" => Req::GetKey,
        "input" => Req::Input(input_opts(a.first())),
        "yesno" => Req::YesNo(a.first().and_then(Value::as_bool).unwrap_or(true)),
        "menucmd" => Req::MenuCmd(arg_str(a, 0), arg_str(a, 1)),
        "mci" => Req::Mci(arg_str(a, 0)),
        "expand" => Req::Expand(arg_str(a, 0)),
        "acs" => Req::Acs(arg_str(a, 0)),
        "inmenu" => Req::InMenu(arg_str(a, 0)),
        "user" => Req::User,
        "saveuser" => {
            let t = a.first().and_then(Value::as_object).ok_or("saveuser takes a dict")?;
            Req::SaveUser(t.iter().map(|(k, v)| to_field(v).map(|f| (k.clone(), f))).collect::<Result<_, _>>()?)
        }
        "log" => Req::Log(arg_str(a, 0)),
        "get" => Req::Get(arg_str(a, 0)),
        "set" => Req::Set(arg_str(a, 0), a.get(1).filter(|v| !v.is_null()).map(|_| arg_str(a, 1))),
        "nocolor" => Req::NoColor(arg_str(a, 0)),
        "pad" => Req::Pad(arg_str(a, 0), arg_num(a, 1, 0) as usize),
        other => return Err(format!("there's no fluxbbs.{other}")),
    })
}

/// An answer, as the script reads it.
fn reply_line(r: Resp) -> String {
    let v = match r {
        Resp::Unit => json!({ "r": null }),
        Resp::Bool(b) => json!({ "r": b }),
        Resp::Str(s) => json!({ "r": s }),
        Resp::MaybeStr(s) => json!({ "r": s }),
        Resp::Pos(x, y) => json!({ "r": [x, y] }),
        Resp::User(fields) => json!({ "r": fields.into_iter().map(|(k, f)| (k.to_string(), from_field(f))).collect::<serde_json::Map<_, _>>() }),
        Resp::Fail(why) => json!({ "e": why }),
    };
    format!("{v}\n")
}

/// A line from the script, `LINE_MAX` at most; None at its end.
async fn read_line(rd: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = (&mut *rd).take(LINE_MAX).read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if !buf.ends_with(b"\n") && n as u64 >= LINE_MAX {
        return Err(io::Error::other("a request far too long"));
    }
    Ok(Some(String::from_utf8_lossy(&buf).trim_end().to_string()))
}

// ---- the session side -----------------------------------------------------

/// A script name: letters, digits, `_` and `-`, and no path.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) && !name.contains("..")
}

/// The script file for a name: `name.py` in the scripts directory, in
/// any case.
pub fn find_script(dir: &Path, name: &str) -> Option<PathBuf> {
    if !valid_name(name) {
        return None;
    }
    let file = if name.to_ascii_lowercase().ends_with(".py") { name.to_string() } else { format!("{name}.py") };
    find_ci(dir, &file).filter(|p| p.is_file())
}

/// Where the `fluxbbs` module goes for scripts to import: written there
/// when it's missing or not this board's.
fn module_dir(cfg: &crate::config::Config) -> io::Result<PathBuf> {
    let dir = cfg.resolve(&cfg.paths.data).join("cache").join("python");
    let file = dir.join("fluxbbs.py");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(MODULE) {
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("fluxbbs.py.new");
        std::fs::write(&tmp, MODULE)?;
        std::fs::rename(&tmp, &file)?;
    }
    // Whole: the script runs in its own folder.
    dir.canonicalize()
}

/// What a script's process said on stderr, kept to its last part.
async fn collect(mut from: impl tokio::io::AsyncRead + Unpin, keep: usize) -> String {
    let mut all = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = from.read(&mut buf).await {
        if n == 0 {
            break;
        }
        all.extend_from_slice(&buf[..n]);
        if all.len() > keep * 2 {
            all.drain(..all.len() - keep);
        }
    }
    let s = String::from_utf8_lossy(&all).to_string();
    s.chars().rev().take(keep).collect::<Vec<_>>().into_iter().rev().collect()
}

impl Session {
    /// `-X name args` (`iplExecute`).
    pub async fn script_command(&mut self, param: &str) -> io::Result<()> {
        let (name, args) = param.trim().split_once(' ').unwrap_or((param.trim(), ""));
        self.run_script(name, args).await
    }

    /// Run `scripts/name.py` with whitespace-separated `args`.
    pub async fn run_script(&mut self, name: &str, args: &str) -> io::Result<()> {
        let cfg = self.state.config();
        let dir = cfg.resolve(&cfg.paths.scripts);
        let Some(path) = find_script(&dir, name) else {
            tracing::warn!("script not found: {name} (in {})", dir.display());
            return self.script_failed(name, "not found").await;
        };
        if self.script_depth >= MAX_DEPTH {
            return self.script_failed(name, "scripts nested too deep").await;
        }
        let key = path.file_stem().map_or(name.to_string(), |s| s.to_string_lossy().to_ascii_lowercase());
        tracing::info!("{}: running script {key} {args}", self.user.handle);
        // Whole paths: the script runs in its own folder.
        let (path, dir) = (path.canonicalize()?, dir.canonicalize()?);
        let modules = module_dir(&cfg)?;
        let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
        ours.set_nonblocking(true)?;
        let fd = theirs.as_raw_fd();
        let python = if cfg.paths.python.trim().is_empty() { "python3".to_string() } else { cfg.paths.python.trim().to_string() };
        let mut cmd = tokio::process::Command::new(&python);
        cmd.arg("-u")
            .arg("-c")
            .arg(LAUNCH)
            .arg(&path)
            .args(args.split_whitespace())
            .current_dir(&dir)
            .env("PYTHONPATH", &modules)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("FLUXBBS_FD", "3")
            .env("FLUXBBS_NODE", self.node.number().to_string())
            .env("FLUXBBS_SCRIPT", &key)
            .env("FLUXBBS_VERSION", env!("CARGO_PKG_VERSION"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        // The script's end of the socket, as its fd 3.
        unsafe {
            cmd.pre_exec(move || {
                if fd == 3 {
                    if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                } else if libc::dup2(fd, 3) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("script {key}: can't start {python}: {e}");
                return self.script_failed(&key, &format!("can't start {python}: {e}")).await;
            }
        };
        drop(theirs);
        // What it prints past the module (a program it runs) goes to the log;
        // what it says on stderr is kept, for when it fails.
        let out = child.stdout.take().map(|o| {
            let key = key.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(o).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    tracing::info!("script {key}: {l}");
                }
            })
        });
        let err = child.stderr.take().map(|e| tokio::spawn(collect(e, 2000)));
        let sock = tokio::net::UnixStream::from_std(ours)?;
        let (rd, mut wr) = sock.into_split();
        let mut rd = BufReader::new(rd);
        self.script_depth += 1;
        let mut failure: Option<io::Error> = None;
        let mut why: Option<String> = None;
        loop {
            let line = match tokio::time::timeout(WATCHDOG, read_line(&mut rd)).await {
                Ok(Ok(Some(l))) => l,
                // The script's done (or gone): its exit says how.
                Ok(Ok(None)) => break,
                Ok(Err(e)) => {
                    why = Some(e.to_string());
                    break;
                }
                Err(_) => {
                    tracing::warn!("script {key} ran {WATCHDOG:?} without calling the board; stopping it");
                    why = Some(format!("ran {}s without calling the board", WATCHDOG.as_secs()));
                    break;
                }
            };
            let reply = match parse_req(&line) {
                Ok(req) => match self.script_call(&key, req).await {
                    Ok(r) => r,
                    Err(e) => {
                        // The caller's gone: stop the script, then report.
                        failure = Some(e);
                        break;
                    }
                },
                Err(e) => Resp::Fail(e),
            };
            if wr.write_all(reply_line(reply).as_bytes()).await.is_err() {
                break;
            }
        }
        drop(wr);
        drop(rd);
        self.script_depth -= 1;
        // Done with it: it has a moment to finish, then it's ended.
        if failure.is_some() || why.is_some() {
            let _ = child.start_kill();
        }
        let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(st)) => Some(st),
            _ => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                None
            }
        };
        if let Some(o) = out {
            let _ = o.await;
        }
        let stderr = match err {
            Some(e) => e.await.unwrap_or_default(),
            None => String::new(),
        };
        self.flush().await?;
        if let Some(e) = failure {
            return Err(e);
        }
        if why.is_none() && status.is_some_and(|s| s.success()) {
            return Ok(());
        }
        // The traceback's last line says what went wrong.
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        let why = why.unwrap_or_else(|| if last.is_empty() { format!("exited with {}", status.map_or("no status".into(), |s| s.to_string())) } else { last });
        if !stderr.trim().is_empty() {
            tracing::warn!("script {key} failed:\n{}", stderr.trim_end());
        } else {
            tracing::warn!("script {key}: {why}");
        }
        self.script_failed(&key, &why).await
    }

    /// Tell the caller a script went wrong; the SysOp sees why.
    async fn script_failed(&mut self, name: &str, why: &str) -> io::Result<()> {
        self.newline();
        let why = if self.is_sysop() { why.to_string() } else { String::new() };
        let s = self.strings().fmt("ScriptError", &[name, &why]);
        self.show_line(&s).await
    }

    /// Carry out one request from a script.
    async fn script_call(&mut self, key: &str, req: Req) -> io::Result<Resp> {
        Ok(match req {
            Req::Write(s) => {
                self.write(&s);
                self.flush().await?;
                Resp::Unit
            }
            Req::Show(s) => {
                // Boxed: what's shown may run a script itself (`%!`).
                Box::pin(self.show(&s)).await?;
                self.flush().await?;
                Resp::Unit
            }
            Req::Say(name) => {
                let s = self.strings().get(&name).to_string();
                Box::pin(self.show(&s)).await?;
                self.flush().await?;
                Resp::Unit
            }
            Req::Newline(n) => {
                for _ in 0..n.min(100) {
                    self.newline();
                }
                Resp::Unit
            }
            Req::Cls => {
                self.clear_screen();
                Resp::Unit
            }
            Req::ClrEol => {
                self.screen.clear_eol();
                Resp::Unit
            }
            Req::Beep => {
                self.screen.write_bytes(b"\x07");
                Resp::Unit
            }
            Req::Goto(x, y) => {
                self.screen.goto(x.max(1), y.max(1));
                Resp::Unit
            }
            Req::Move(dir, n) => {
                match dir {
                    'U' => self.screen.move_up(n),
                    'D' => self.screen.move_down(n),
                    'L' => self.screen.move_left(n),
                    _ => self.screen.move_right(n),
                }
                Resp::Unit
            }
            Req::Color(fg, bg) => {
                self.screen.set_color(fluxbbs_term::Color::new(fg, bg));
                Resp::Unit
            }
            Req::Where => Resp::Pos(self.screen.where_x(), self.screen.where_y()),
            Req::ShowFile(name) => Resp::Bool(Box::pin(self.show_file(&name)).await?),
            Req::Pause => {
                self.prompt_key().await?;
                Resp::Unit
            }
            Req::GetKey => {
                self.flush().await?;
                Resp::Str(key_name(self.read_key().await?))
            }
            Req::Input(o) => {
                let mut opts = ReadOpts::new(o.max).case(o.case).prefill(o.default).allowed(o.allowed);
                if o.password {
                    opts = opts.password();
                }
                if o.required {
                    opts = opts.required();
                }
                if o.complete {
                    opts = opts.complete_users();
                }
                let s = self.read_string(opts).await?;
                // Like `?I`: a caller's codes can't act wherever it's shown.
                Resp::Str(if o.password { s } else { fluxbbs_term::defuse(&s) })
            }
            Req::YesNo(default) => Resp::Bool(self.yes_no(default).await?),
            Req::MenuCmd(code, param) => {
                Box::pin(self.run_command(&code, &param)).await?;
                self.flush().await?;
                Resp::Unit
            }
            Req::Mci(code) => match code.as_bytes() {
                [a, b] => Resp::MaybeStr(self.mci_value([*a, *b])),
                _ => Resp::MaybeStr(None),
            },
            Req::Expand(s) => Resp::Str(self.expand_mci(&s, 0)),
            Req::Acs(expr) => Resp::Bool(self.acs(&expr)),
            Req::InMenu(keys) => Resp::Bool(self.menu.menu.as_ref().is_some_and(|m| m.commands.iter().any(|c| c.keys.eq_ignore_ascii_case(&keys)))),
            Req::User => Resp::User(user_fields(&self.user)),
            Req::SaveUser(fields) => {
                let mut u = self.user.clone();
                for (k, v) in fields {
                    if let Err(why) = set_user_field(&mut u, &k, v) {
                        return Ok(Resp::Fail(why));
                    }
                }
                self.user = u;
                self.save_user();
                Resp::Unit
            }
            Req::Log(s) => {
                tracing::info!("{} [{key}]: {s}", self.user.handle);
                Resp::Unit
            }
            Req::Get(k) => match self.state.sys.script_get(key, &k) {
                Ok(v) => Resp::MaybeStr(v),
                Err(e) => Resp::Fail(e.to_string()),
            },
            Req::Set(k, v) => match self.state.sys.script_set(key, &k, v.as_deref()) {
                Ok(()) => Resp::Unit,
                Err(e) => Resp::Fail(e.to_string()),
            },
            Req::NoColor(s) => Resp::Str(crate::editor::strip_codes(&s)),
            Req::Pad(s, n) => Resp::Str(fluxbbs_term::codes::resize(&s, n.min(255))),
        })
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_fields_round_trip() {
        let mut u = User { handle: "phrac".into(), sl: 50, flags: ['A', 'X'].into_iter().collect(), ..Default::default() };
        for (k, v) in user_fields(&u.clone()) {
            set_user_field(&mut u, k, v).unwrap();
        }
        set_user_field(&mut u, "sl", Field::Num(100.0)).unwrap();
        assert_eq!(u.sl, 100);
        assert!(set_user_field(&mut u, "handle", Field::Str("x".into())).is_err());
        assert!(set_user_field(&mut u, "nonsense", Field::Num(1.0)).is_err());
        assert!(set_user_field(&mut u, "sl", Field::Num(300.0)).is_err());
    }

    #[test]
    fn script_names() {
        let dir = std::env::temp_dir().join(format!("inq-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Hello.py"), "").unwrap();
        assert!(find_script(&dir, "hello").is_some() && find_script(&dir, "hello.py").is_some());
        assert!(find_script(&dir, "../hello").is_none() && find_script(&dir, "").is_none() && find_script(&dir, "nope").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn requests_and_answers() {
        assert!(matches!(parse_req(r#"{"op":"write","a":["hi"]}"#), Ok(Req::Write(s)) if s == "hi"));
        assert!(matches!(parse_req(r#"{"op":"gotoxy","a":[3,4]}"#), Ok(Req::Goto(3, 4))));
        assert!(matches!(parse_req(r#"{"op":"input","a":[{"max":500,"password":true}]}"#), Ok(Req::Input(o)) if o.max == 255 && o.password));
        assert!(matches!(parse_req(r#"{"op":"set","a":["k",null]}"#), Ok(Req::Set(k, None)) if k == "k"));
        assert!(matches!(parse_req(r#"{"op":"saveuser","a":[{"sl":60,"autosig":["a","b"]}]}"#), Ok(Req::SaveUser(f)) if f.len() == 2));
        assert!(parse_req(r#"{"op":"rm_rf","a":[]}"#).is_err() && parse_req("not json").is_err());
        assert_eq!(reply_line(Resp::Pos(2, 3)), "{\"r\":[2,3]}\n");
        assert_eq!(reply_line(Resp::MaybeStr(None)), "{\"r\":null}\n");
        assert_eq!(reply_line(Resp::Fail("no".into())), "{\"e\":\"no\"}\n");
        let u = User { handle: "phrac".into(), sl: 50, ..Default::default() };
        let v: Value = serde_json::from_str(&reply_line(Resp::User(user_fields(&u)))).unwrap();
        assert_eq!((v["r"]["handle"].as_str(), v["r"]["sl"].as_i64()), (Some("phrac"), Some(50)));
    }

    /// A script run for real (if this host has python3): what it asks of
    /// the board, answered here as a session would.
    #[tokio::test]
    async fn a_python_script_talks_to_the_board() {
        if std::process::Command::new("python3").arg("-c", ).arg("pass").status().map(|s| !s.success()).unwrap_or(true) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("inq-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fluxbbs.py"), MODULE).unwrap();
        let script = dir.join("t.py");
        std::fs::write(&script, "import fluxbbs as bbs\nprint('hi', bbs.user()['handle'])\nif bbs.yesno(False):\n    bbs.set('last', bbs.input(max=10))\ntry:\n    bbs.saveuser({'handle': 'x'})\nexcept bbs.Error as e:\n    bbs.log(str(e))\n").unwrap();
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
        let fd = theirs.as_raw_fd();
        let mut cmd = tokio::process::Command::new("python3");
        cmd.arg("-c").arg(LAUNCH).arg(&script).env("PYTHONPATH", &dir).env("FLUXBBS_FD", "3");
        unsafe {
            cmd.pre_exec(move || {
                if fd == 3 { libc::fcntl(3, libc::F_SETFD, 0); } else { libc::dup2(fd, 3); }
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        drop(theirs);
        ours.set_nonblocking(true).unwrap();
        let (rd, mut wr) = tokio::net::UnixStream::from_std(ours).unwrap().into_split();
        let mut rd = BufReader::new(rd);
        let mut seen = Vec::new();
        while let Some(line) = read_line(&mut rd).await.unwrap() {
            let req = parse_req(&line).unwrap();
            seen.push(format!("{req:?}"));
            let r = match req {
                Req::User => Resp::User(user_fields(&User { handle: "phrac".into(), ..Default::default() })),
                Req::YesNo(_) => Resp::Bool(true),
                Req::Input(_) => Resp::Str("typed".into()),
                Req::SaveUser(f) => {
                    let mut u = User::default();
                    match f.into_iter().try_for_each(|(k, v)| set_user_field(&mut u, &k, v)) {
                        Ok(()) => Resp::Unit,
                        Err(e) => Resp::Fail(e),
                    }
                }
                _ => Resp::Unit,
            };
            wr.write_all(reply_line(r).as_bytes()).await.unwrap();
        }
        assert!(child.wait().await.unwrap().success());
        let all = seen.join("|");
        assert!(all.contains("Write(\"hi phrac\\r\\n\")") || all.contains("Write(\"hi\")"), "{all}");
        assert!(all.contains("Set(\"last\", Some(\"typed\"))"), "{all}");
        assert!(all.contains("Log(\"user.handle can't be changed by a script\")"), "{all}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
