//! Lua scripts, in place of IPL (`IPLX.PAS`, the Iniquity Programming
//! Language). `-X name args` runs `scripts/name.lua`, as `%!name args` in
//! any prompt string does; see docs/SCRIPTING.md.
//!
//! The script runs on a thread of its own, with plain (blocking) Lua: each
//! `bbs.*` call sends a request to the caller's session and waits for the
//! answer, so the session keeps sole charge of the connection. A script
//! that computes for [`WATCHDOG`] without calling `bbs` is stopped, as is
//! one whose caller hangs up.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use tokio::sync::{mpsc, oneshot};

use crate::input::{Case, Key, ReadOpts};
use crate::session::{Session, find_ci};
use crate::user::{User, fmt_date, parse_date};

/// How long a script may run without calling `bbs`.
const WATCHDOG: Duration = Duration::from_secs(10);
/// Scripts may run scripts (`%!` in a string they show), this deep.
const MAX_DEPTH: u8 = 4;
const MEMORY_LIMIT: usize = 64 << 20;

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
}

#[derive(Debug)]
enum Resp {
    Unit,
    Bool(bool),
    Str(String),
    MaybeStr(Option<String>),
    Pos(u16, u16),
    User(Vec<(&'static str, Field)>),
    /// The request was wrong (a bad field, say): a Lua error.
    Fail(String),
}

struct Call {
    req: Req,
    reply: oneshot::Sender<Resp>,
}

/// The script thread's line to the session.
#[derive(Clone)]
struct Bridge {
    tx: mpsc::Sender<Call>,
    abort: Arc<AtomicBool>,
}

fn stopped() -> mlua::Error {
    mlua::Error::RuntimeError("script stopped (the caller left, or it ran too long)".into())
}

impl Bridge {
    fn call(&self, req: Req) -> mlua::Result<Resp> {
        if self.abort.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        let (reply, rx) = oneshot::channel();
        self.tx.blocking_send(Call { req, reply }).map_err(|_| stopped())?;
        match rx.blocking_recv().map_err(|_| stopped())? {
            Resp::Fail(why) => Err(mlua::Error::RuntimeError(why)),
            r => Ok(r),
        }
    }
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
        Key::Char(c) => iniquity_term::cp437::decode(&[c]),
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

// ---- the Lua side ---------------------------------------------------------

/// Anything, as Lua's `tostring` would show it.
fn text(v: &Value) -> mlua::Result<String> {
    Ok(match v {
        Value::Nil => String::new(),
        Value::String(s) => s.to_string_lossy(),
        other => other.to_string()?,
    })
}

fn to_field(v: Value) -> mlua::Result<Field> {
    Ok(match v {
        Value::Boolean(b) => Field::Bool(b),
        Value::Integer(i) => Field::Num(i as f64),
        Value::Number(n) => Field::Num(n),
        Value::String(s) => Field::Str(s.to_string_lossy()),
        Value::Table(t) => Field::List(t.sequence_values::<Value>().map(|v| v.and_then(|v| text(&v))).collect::<mlua::Result<_>>()?),
        other => return Err(mlua::Error::RuntimeError(format!("can't store a {} in a user record", other.type_name()))),
    })
}

fn from_field(lua: &Lua, f: Field) -> mlua::Result<Value> {
    Ok(match f {
        Field::Str(s) => Value::String(lua.create_string(&s)?),
        Field::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => Value::Integer(n as i64),
        Field::Num(n) => Value::Number(n),
        Field::Bool(b) => Value::Boolean(b),
        Field::List(l) => Value::Table(lua.create_sequence_from(l)?),
    })
}

fn input_opts(v: Option<Table>) -> mlua::Result<InputOpts> {
    let mut o = InputOpts { max: 40, ..InputOpts::default() };
    let Some(t) = v else { return Ok(o) };
    if let Some(m) = t.get::<Option<usize>>("max")? {
        o.max = m.clamp(1, 255);
    }
    if let Some(d) = t.get::<Option<Value>>("default")? {
        o.default = text(&d)?;
    }
    o.password = t.get::<Option<bool>>("password")?.unwrap_or(false);
    o.required = t.get::<Option<bool>>("required")?.unwrap_or(false);
    o.complete = t.get::<Option<bool>>("complete")?.unwrap_or(false);
    if t.get::<Option<bool>>("upper")?.unwrap_or(false) {
        o.case = Case::Upper;
    }
    if t.get::<Option<bool>>("mixed")?.unwrap_or(false) {
        o.case = Case::Mixed;
    }
    if let Some(a) = t.get::<Option<String>>("allowed")? {
        o.allowed = a;
    }
    Ok(o)
}

/// Lua's standard library with what would disturb the BBS taken out:
/// `os.exit` would end it, and the console's stdin and stdout aren't the
/// caller's. `print` and `io.write` write to the caller instead.
const PRELUDE: &str = r##"
os.exit = nil
io.read, io.input, io.output, io.stdin, io.stdout, io.stderr, io.popen = nil, nil, nil, nil, nil, nil, nil
local lines = io.lines
io.lines = function(file, ...)
  if file == nil then error("io.lines needs a file name", 2) end
  return lines(file, ...)
end
io.write = function(...)
  for i = 1, select("#", ...) do bbs.write(tostring((select(i, ...)))) end
end
print = function(...)
  local t = {}
  for i = 1, select("#", ...) do t[#t + 1] = tostring((select(i, ...))) end
  bbs.writeln(table.concat(t, "\t"))
end
"##;

/// Build the `bbs` table.
fn install(lua: &Lua, b: &Bridge, name: &str, node: usize) -> mlua::Result<()> {
    let bbs = lua.create_table()?;
    macro_rules! f {
        ($name:literal, |$lua:ident, $b:ident, $args:tt : $ty:ty| $body:expr) => {{
            let $b = b.clone();
            bbs.set($name, lua.create_function(move |$lua, $args: $ty| {
                let _ = &$lua;
                $body
            })?)?;
        }};
    }
    f!("write", |_l, b, v: Value| b.call(Req::Write(text(&v)?)).map(|_| ()));
    f!("writeln", |_l, b, v: Option<Value>| b.call(Req::Write(format!("{}\r\n", text(&v.unwrap_or(Value::Nil))?))).map(|_| ()));
    f!("show", |_l, b, v: Value| b.call(Req::Show(text(&v)?)).map(|_| ()));
    f!("showln", |_l, b, v: Option<Value>| {
        b.call(Req::Show(text(&v.unwrap_or(Value::Nil))?))?;
        b.call(Req::Newline(1)).map(|_| ())
    });
    f!("say", |_l, b, v: String| b.call(Req::Say(v)).map(|_| ()));
    f!("sayln", |_l, b, v: String| {
        b.call(Req::Say(v))?;
        b.call(Req::Newline(1)).map(|_| ())
    });
    f!("newline", |_l, b, v: Option<u16>| b.call(Req::Newline(v.unwrap_or(1))).map(|_| ()));
    f!("cls", |_l, b, _a: ()| b.call(Req::Cls).map(|_| ()));
    f!("clreol", |_l, b, _a: ()| b.call(Req::ClrEol).map(|_| ()));
    f!("beep", |_l, b, _a: ()| b.call(Req::Beep).map(|_| ()));
    f!("gotoxy", |_l, b, (x, y): (u16, u16)| b.call(Req::Goto(x, y)).map(|_| ()));
    f!("up", |_l, b, v: Option<u16>| b.call(Req::Move('U', v.unwrap_or(1))).map(|_| ()));
    f!("down", |_l, b, v: Option<u16>| b.call(Req::Move('D', v.unwrap_or(1))).map(|_| ()));
    f!("left", |_l, b, v: Option<u16>| b.call(Req::Move('L', v.unwrap_or(1))).map(|_| ()));
    f!("right", |_l, b, v: Option<u16>| b.call(Req::Move('R', v.unwrap_or(1))).map(|_| ()));
    f!("color", |_l, b, (fg, bg): (u8, Option<u8>)| b.call(Req::Color(fg & 15, bg.unwrap_or(0) & 7)).map(|_| ()));
    f!("where", |_l, b, _a: ()| match b.call(Req::Where)? {
        Resp::Pos(x, y) => Ok((x, y)),
        _ => Ok((1, 1)),
    });
    f!("showfile", |_l, b, v: String| match b.call(Req::ShowFile(v))? {
        Resp::Bool(ok) => Ok(ok),
        _ => Ok(false),
    });
    f!("pause", |_l, b, _a: ()| b.call(Req::Pause).map(|_| ()));
    f!("getkey", |_l, b, _a: ()| match b.call(Req::GetKey)? {
        Resp::Str(k) => Ok(k),
        _ => Ok(String::new()),
    });
    f!("input", |_l, b, v: Option<Table>| match b.call(Req::Input(input_opts(v)?))? {
        Resp::Str(s) => Ok(s),
        _ => Ok(String::new()),
    });
    f!("yesno", |_l, b, v: Option<bool>| match b.call(Req::YesNo(v.unwrap_or(true)))? {
        Resp::Bool(y) => Ok(y),
        _ => Ok(false),
    });
    f!("menucmd", |_l, b, (code, param): (String, Option<Value>)| {
        let param = text(&param.unwrap_or(Value::Nil))?;
        b.call(Req::MenuCmd(code, param)).map(|_| ())
    });
    f!("mci", |_l, b, v: String| match b.call(Req::Mci(v))? {
        Resp::MaybeStr(s) => Ok(s),
        _ => Ok(None),
    });
    f!("expand", |_l, b, v: Value| match b.call(Req::Expand(text(&v)?))? {
        Resp::Str(s) => Ok(s),
        _ => Ok(String::new()),
    });
    f!("acs", |_l, b, v: String| match b.call(Req::Acs(v))? {
        Resp::Bool(ok) => Ok(ok),
        _ => Ok(false),
    });
    // 2.0's IPL `isincurmenu`: does the current menu have this key?
    f!("inmenu", |_l, b, v: String| match b.call(Req::InMenu(v))? {
        Resp::Bool(ok) => Ok(ok),
        _ => Ok(false),
    });
    f!("user", |lua, b, _a: ()| match b.call(Req::User)? {
        Resp::User(fields) => {
            let t = lua.create_table()?;
            for (k, v) in fields {
                t.set(k, from_field(lua, v)?)?;
            }
            Ok(t)
        }
        _ => lua.create_table(),
    });
    f!("saveuser", |_l, b, t: Table| {
        let mut fields = Vec::new();
        for pair in t.pairs::<String, Value>() {
            let (k, v) = pair?;
            fields.push((k, to_field(v)?));
        }
        b.call(Req::SaveUser(fields)).map(|_| ())
    });
    f!("log", |_l, b, v: Value| b.call(Req::Log(text(&v)?)).map(|_| ()));
    f!("get", |_l, b, k: String| match b.call(Req::Get(k))? {
        Resp::MaybeStr(s) => Ok(s),
        _ => Ok(None),
    });
    f!("set", |_l, b, (k, v): (String, Option<Value>)| {
        let v = match v {
            None | Some(Value::Nil) => None,
            Some(v) => Some(text(&v)?),
        };
        b.call(Req::Set(k, v)).map(|_| ())
    });
    // Local helpers: no trip to the session.
    bbs.set("nocolor", lua.create_function(|_, v: Value| Ok(crate::editor::strip_codes(&text(&v)?)))?)?;
    bbs.set("pad", lua.create_function(|_, (v, n): (Value, usize)| Ok(iniquity_term::codes::resize(&text(&v)?, n)))?)?;
    bbs.set("node", node)?;
    bbs.set("script", name)?;
    bbs.set("version", env!("CARGO_PKG_VERSION"))?;
    lua.globals().set("bbs", bbs)?;
    Ok(())
}

/// Run a script to the end, on the calling (blocking) thread.
fn run_lua(source: Vec<u8>, path: &Path, name: &str, args: Vec<String>, dir: &Path, b: Bridge, node: usize) -> mlua::Result<()> {
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    let abort = b.abort.clone();
    lua.set_global_hook(HookTriggers::new().every_nth_instruction(10_000), move |_, _| {
        if abort.load(Ordering::Relaxed) { Err(stopped()) } else { Ok(VmState::Continue) }
    })?;
    install(&lua, &b, name, node)?;
    lua.load(PRELUDE).set_name("=prelude").exec()?;
    let package: Table = lua.globals().get("package")?;
    let d = dir.to_string_lossy();
    package.set("path", format!("{d}/?.lua;{d}/?/init.lua"))?;
    package.set("cpath", "")?;
    lua.globals().set("arg", lua.create_sequence_from(args.clone())?)?;
    let chunk = lua.load(source).set_name(format!("@{}", path.file_name().map_or(name.into(), |f| f.to_string_lossy())));
    chunk.call::<()>(mlua::MultiValue::from_iter(args.into_iter().map(|a| lua.create_string(&a).map(Value::String)).collect::<mlua::Result<Vec<_>>>()?))
}

// ---- the session side -----------------------------------------------------

/// A script name: letters, digits, `_` and `-`, and no path.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) && !name.contains("..")
}

/// The script file for a name: `name.lua` in the scripts directory, in
/// any case.
pub fn find_script(dir: &Path, name: &str) -> Option<PathBuf> {
    if !valid_name(name) {
        return None;
    }
    let file = if name.to_ascii_lowercase().ends_with(".lua") { name.to_string() } else { format!("{name}.lua") };
    find_ci(dir, &file).filter(|p| p.is_file())
}

impl Session {
    /// `-X name args` (`iplExecute`).
    pub async fn script_command(&mut self, param: &str) -> io::Result<()> {
        let (name, args) = param.trim().split_once(' ').unwrap_or((param.trim(), ""));
        self.run_script(name, args).await
    }

    /// Run `scripts/name.lua` with whitespace-separated `args`.
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
        let source = std::fs::read(&path)?;
        let key = path.file_stem().map_or(name.to_string(), |s| s.to_string_lossy().to_ascii_lowercase());
        tracing::info!("{}: running script {key} {args}", self.user.handle);
        let (tx, mut rx) = mpsc::channel::<Call>(1);
        let abort = Arc::new(AtomicBool::new(false));
        let bridge = Bridge { tx, abort: abort.clone() };
        let args: Vec<String> = args.split_whitespace().map(String::from).collect();
        let (thread_key, node) = (key.clone(), self.node.number());
        let mut handle = tokio::task::spawn_blocking(move || {
            run_lua(source, &path, &thread_key, args, &dir, bridge, node).map_err(|e| e.to_string())
        });
        self.script_depth += 1;
        let mut failure: Option<io::Error> = None;
        let outcome = loop {
            tokio::select! {
                call = rx.recv(), if failure.is_none() => match call {
                    Some(c) => match self.script_call(&key, c.req).await {
                        Ok(r) => {
                            let _ = c.reply.send(r);
                        }
                        Err(e) => {
                            // The caller's gone: stop the script, then report.
                            abort.store(true, Ordering::Relaxed);
                            rx.close();
                            failure = Some(e);
                        }
                    },
                    // The script's done with us; its thread is finishing.
                    None => break tokio::time::timeout(WATCHDOG, &mut handle).await.ok(),
                },
                r = &mut handle => break Some(r),
                _ = tokio::time::sleep(WATCHDOG) => {
                    if abort.swap(true, Ordering::Relaxed) || failure.is_some() {
                        // Stopped, but stuck somewhere a hook can't reach.
                        break None;
                    }
                    tracing::warn!("script {key} ran {WATCHDOG:?} without calling bbs; stopping it");
                }
            }
        };
        self.script_depth -= 1;
        self.flush().await?;
        if let Some(e) = failure {
            return Err(e);
        }
        match outcome {
            Some(Ok(Ok(()))) => Ok(()),
            Some(Ok(Err(why))) => {
                tracing::warn!("script {key}: {why}");
                self.script_failed(&key, &why).await
            }
            Some(Err(e)) => {
                tracing::error!("script {key} panicked: {e}");
                self.script_failed(&key, "crashed").await
            }
            None => {
                tracing::error!("script {key} didn't stop; leaving its thread");
                self.script_failed(&key, "didn't stop").await
            }
        }
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
                self.write("\x07");
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
                self.screen.set_color(iniquity_term::Color::new(fg, bg));
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
                Resp::Str(if o.password { s } else { iniquity_term::defuse(&s) })
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_fields_round_trip() {
        let mut u = User { handle: "phrac".into(), sl: 50, ..User::default() };
        // Every field written back unchanged is accepted.
        for (k, v) in user_fields(&u.clone()) {
            set_user_field(&mut u, k, v).unwrap_or_else(|e| panic!("{k}: {e}"));
        }
        set_user_field(&mut u, "sl", Field::Num(100.0)).unwrap();
        set_user_field(&mut u, "level", Field::Str("c".into())).unwrap();
        set_user_field(&mut u, "flags", Field::Str("ab".into())).unwrap();
        set_user_field(&mut u, "birth_date", Field::Str("02/29/96".into())).unwrap();
        set_user_field(&mut u, "autosig", Field::List(vec!["-- phrac".into()])).unwrap();
        assert_eq!((u.sl, u.level, u.flags.iter().collect::<String>()), (100, 'C', "AB".into()));
        assert_eq!(u.autosig, ["-- phrac"]);
        assert!(set_user_field(&mut u, "handle", Field::Str("x".into())).unwrap_err().contains("can't be changed"));
        assert!(set_user_field(&mut u, "sl", Field::Num(300.0)).is_err());
        assert!(set_user_field(&mut u, "pause", Field::Str("yes".into())).is_err());
        assert!(set_user_field(&mut u, "nope", Field::Bool(true)).unwrap_err().contains("no user.nope"));
    }

    #[test]
    fn script_names() {
        assert!(valid_name("oneliner") && valid_name("my-mod_2") && valid_name("x.lua"));
        assert!(!valid_name("../etc/passwd") && !valid_name("a/b") && !valid_name("") && !valid_name("a b"));
    }

    /// Run a script against a fake session that records what it's asked.
    fn run(src: &str, answer: impl Fn(&Req) -> Resp + Send + 'static) -> (Result<(), String>, Vec<String>) {
        let (tx, mut rx) = mpsc::channel::<Call>(1);
        let abort = Arc::new(AtomicBool::new(false));
        let b = Bridge { tx, abort };
        let src = src.as_bytes().to_vec();
        let t = std::thread::spawn(move || {
            run_lua(src, Path::new("t.lua"), "t", vec!["one".into(), "two".into()], Path::new("."), b, 3).map_err(|e| e.to_string())
        });
        let mut log = Vec::new();
        while let Some(c) = rx.blocking_recv() {
            log.push(format!("{:?}", c.req));
            let _ = c.reply.send(answer(&c.req));
        }
        (t.join().unwrap(), log)
    }

    #[test]
    fn scripts_talk_to_the_session() {
        let (r, log) = run(
            r#"
            local a, b = ...
            bbs.write(a .. "+" .. arg[2])
            print("x", 1, true)
            io.write("y")
            bbs.showln("|15hi %UN")
            local name = bbs.input{max = 10, upper = true}
            if bbs.yesno(false) then bbs.set("last", name) end
            local u = bbs.user()
            u.sl = u.sl + 1
            bbs.saveuser(u)
            assert(bbs.node == 3 and bbs.script == "t")
            assert(bbs.pad("ab", 4) == "ab  " and bbs.nocolor("|04x") == "x")
            assert(os.exit == nil and io.read == nil)
            "#,
            |req| match req {
                Req::Input(_) => Resp::Str("FIEND".into()),
                Req::YesNo(_) => Resp::Bool(true),
                Req::User => Resp::User(user_fields(&User { sl: 10, ..User::default() })),
                _ => Resp::Unit,
            },
        );
        r.unwrap();
        assert_eq!(log[0], r#"Write("one+two")"#);
        assert_eq!(log[1], "Write(\"x\\t1\\ttrue\\r\\n\")");
        assert_eq!(log[2], r#"Write("y")"#);
        assert_eq!(&log[3..5], [r#"Show("|15hi %UN")"#, "Newline(1)"]);
        assert!(log[5].starts_with("Input(InputOpts { max: 10"), "{}", log[5]);
        assert_eq!(log[7], r#"Set("last", Some("FIEND"))"#);
        assert!(log[9].contains(r#"("sl", Num(11.0))"#), "{}", log[9]);
    }

    #[test]
    fn errors_and_runaways_stop() {
        let (r, _) = run("bbs.write(nil .. 1)", |_| Resp::Unit);
        assert!(r.unwrap_err().contains("attempt to concatenate"));
        let (r, _) = run("bbs.saveuser({handle = 'x'})", |_| Resp::Fail("user.handle can't be changed by a script".into()));
        assert!(r.unwrap_err().contains("can't be changed"));
        // A loop that never calls bbs is stopped once the session says so.
        let (tx, _rx) = mpsc::channel::<Call>(1);
        let abort = Arc::new(AtomicBool::new(false));
        let b = Bridge { tx, abort: abort.clone() };
        let t = std::thread::spawn(move || {
            run_lua(b"while true do end".to_vec(), Path::new("t.lua"), "t", vec![], Path::new("."), b, 1).map_err(|e| e.to_string())
        });
        std::thread::sleep(Duration::from_millis(100));
        abort.store(true, Ordering::Relaxed);
        assert!(t.join().unwrap().unwrap_err().contains("script stopped"));
    }
}
