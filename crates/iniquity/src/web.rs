//! A read-only status feed for the board's website (new): who's online,
//! the last callers, the newest files and some totals, as JSON at
//! `/status.json` (or `/api/status.json`). It answers GET and nothing
//! else, never shows what a caller isn't meant to, and is rebuilt at most
//! every few seconds however often it's asked for. `/files.txt` and
//! `/files.html` are the file list (2.0 made these with `iniquity fltxt`
//! and `flhtml`): the areas a new caller could open.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Datelike, Timelike};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::BbsState;
use crate::acs::{self, AcsEnv};
use crate::user::User;

const FRESH: Duration = Duration::from_secs(10);

#[derive(Serialize)]
struct Status {
    bbs: Board,
    online: Vec<Online>,
    last_callers: Vec<Caller>,
    new_files: Vec<NewFile>,
    new_posts: Vec<NewPost>,
    totals: Totals,
    today: Today,
    updated: String,
}

#[derive(Serialize)]
struct Board {
    name: String,
    sysop: String,
    nodes: u16,
    up_since: String,
}

#[derive(Serialize)]
struct Online {
    node: usize,
    handle: String,
    activity: String,
    via: &'static str,
}

#[derive(Serialize)]
struct Caller {
    handle: String,
    at: String,
    new_user: bool,
    via: String,
}

#[derive(Serialize)]
struct NewFile {
    name: String,
    area: String,
    size: u64,
    uploaded: String,
    uploader: String,
    description: String,
}

#[derive(Serialize)]
struct NewPost {
    area: String,
    subject: String,
    from: String,
    posted: String,
    /// Replies to it so far.
    replies: u32,
    /// Whether it answers another message.
    reply: bool,
}

#[derive(Serialize)]
struct Totals {
    users: u32,
    calls: u64,
    files: u32,
    messages: u32,
    uploads: u64,
    downloads: u64,
}

#[derive(Serialize)]
struct Today {
    calls: u64,
    new_users: u64,
    uploads: u64,
    downloads: u64,
}

/// What a node is doing, in words a stranger may see.
fn activity(raw: &str) -> String {
    // A door by name (2.0: "the ability for a door name to be in the status").
    if let Some(door) = raw.strip_prefix("Running ").filter(|d| *d != "a door") {
        return format!("in {door}");
    }
    if let Some(menu) = raw.strip_prefix("Menu: ") {
        return match menu.to_ascii_uppercase().as_str() {
            "MAIN" => "at the main menu",
            "FILE" | "SCANFILE" => "browsing files",
            "MESSAGE" | "SCANMSG" | "EMAIL" => "reading messages",
            "DOOR" => "looking at the doors",
            "MULTINOD" => "seeing who's on",
            "ONELINER" | "AUTOMSG" => "reading the oneliners",
            _ => "looking around",
        }
        .into();
    }
    match raw {
        "Downloading" => "downloading",
        "Uploading" => "uploading",
        "Running a door" => "playing a door game",
        "Multinode chat" => "in chat",
        "Voting" | "Adding voting question" => "in the voting booth",
        "Paging the SysOp" => "paging the sysop",
        r if r.starts_with("Chatting") => "chatting",
        r if r.starts_with("Writing") => "writing a message",
        _ => "online",
    }
    .into()
}

/// A description's first line with words in it (FILE_ID.DIZ files often
/// open with a line of art), trimmed.
pub(crate) fn blurb(desc: &[String]) -> String {
    let wordy = |l: &&String| l.chars().filter(char::is_ascii_alphabetic).count() >= 3;
    desc.iter().find(wordy).or(desc.first()).map(|l| l.trim().to_string()).unwrap_or_default()
}

/// Can a brand-new caller (access level A) get at this ACS?
/// The storage keys of the file areas a new caller could open: their
/// ACS and every parent's passed at the new-user level, no password.
/// (Also what the file figures show before anyone has logged on.)
pub(crate) fn public_fareas(state: &BbsState) -> Vec<String> {
    let fa = state.fareas();
    let mut ok_at: Vec<bool> = Vec::new();
    let mut out = Vec::new();
    for (n, depth) in fa.tree() {
        let a = &fa.areas[n as usize - 1];
        ok_at.truncate(depth);
        let ok = ok_at.last().copied().unwrap_or(true) && public(state, &a.acs) && a.password.is_empty();
        ok_at.push(ok);
        if ok {
            out.push(a.file.clone());
        }
    }
    out
}

fn public(state: &BbsState, expr: &str) -> bool {
    let mut u = User::default();
    if let Some(level) = state.level('A') {
        u.apply_level('A', &level);
    }
    let now = crate::clock::now();
    let env = AcsEnv {
        sl: u.sl,
        dsl: u.dsl,
        flags: u.flags.clone(),
        sex: u.sex,
        level: 'A',
        hour: now.hour(),
        weekday: now.weekday().num_days_from_sunday(),
        minute_of_day: now.hour() * 60 + now.minute(),
        ratio_ok: true,
        minutes_left: 60,
        ..AcsEnv::default()
    };
    acs::check(expr, &env)
}

fn build(state: &BbsState, started: &str) -> anyhow::Result<Status> {
    let cfg = state.config();
    let w = &cfg.web;
    let shown = |h: &str| if w.show_handles { h.to_string() } else { "a caller".to_string() };
    let online = state
        .nodes
        .online()
        .into_iter()
        .filter(|(_, n)| !n.user.is_empty() && !n.invisible && (w.show_sysop || !n.sysop))
        .map(|(node, n)| Online { node, handle: shown(&n.user), activity: activity(&n.activity), via: n.transport })
        .collect();
    let last_callers = state
        .sys
        .calls(Some(w.last_callers as u32), None)?
        .into_iter()
        .rev()
        .map(|c| Caller { handle: shown(&c.handle), at: c.at.format("%Y-%m-%dT%H:%M:%S").to_string(), new_user: c.new_user, via: c.transport })
        .collect();

    // Files only from areas (and parents) a new caller could open.
    let fa = state.fareas();
    let open = public_fareas(state);
    let mut files = Vec::new();
    let mut file_count = 0;
    for a in &fa.areas {
        file_count += state.files.count(&a.file)?;
        if open.iter().any(|k| k == &a.file) {
            files.extend(state.files.list(&a.file)?.into_iter().filter(|f| f.valid).map(|f| (a.name.clone(), f)));
        }
    }
    files.sort_by(|a, b| b.1.uploaded.cmp(&a.1.uploaded).then(b.1.num.cmp(&a.1.num)));
    let new_files = files
        .into_iter()
        .take(w.new_files as usize)
        .map(|(area, f)| NewFile {
            name: f.name,
            area,
            size: f.size,
            uploaded: f.uploaded.map(|d| d.to_string()).unwrap_or_default(),
            uploader: shown(&f.uploader),
            description: blurb(&f.description),
        })
        .collect();

    // Posts only from areas a new caller could read, never private ones.
    let mut messages = 0;
    let areas = state.areas();
    let mut open = Vec::new();
    for a in &areas.areas {
        messages += state.msgs.count(&a.file)?;
        if public(state, &a.acs) && a.password.is_empty() {
            open.push(a.file.clone());
        }
    }
    let posts = if w.new_posts > 0 { state.msgs.newest_public(&open, w.new_posts as usize)? } else { Vec::new() };
    let new_posts = posts
        .into_iter()
        .filter_map(|m| areas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&m.area)).map(|a| (a, m)))
        .map(|(a, m)| NewPost {
            area: a.name.clone(),
            subject: m.subject,
            // Handles, never real names (a real-name area's `name`).
            from: if m.anonymous { m.from.name } else if a.real_names && !m.from.alias.is_empty() { shown(&m.from.alias) } else { shown(&m.from.name) },
            posted: m.posted.format("%Y-%m-%dT%H:%M:%S").to_string(),
            replies: m.replies,
            reply: m.reply_to != 0,
        })
        .collect();
    let totals = state.sys.totals()?;
    let today = state.sys.day(crate::clock::now().date_naive())?;
    Ok(Status {
        bbs: Board { name: cfg.bbs.name.clone(), sysop: cfg.bbs.sysop_alias.clone(), nodes: cfg.bbs.max_nodes, up_since: started.to_string() },
        online,
        last_callers,
        new_files,
        new_posts,
        totals: Totals {
            users: state.users.count()?,
            calls: totals.calls,
            files: file_count,
            messages,
            uploads: totals.uploads,
            downloads: totals.downloads,
        },
        today: Today { calls: today.calls, new_users: today.new_users, uploads: today.uploads, downloads: today.downloads },
        updated: crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    })
}

struct Cache {
    at: Option<Instant>,
    body: String,
}

/// Serve the feed on `[web] listen` until the server stops.
/// The file list: each area a new caller could open, its checked files
/// with size, date and description.
fn file_list(state: &BbsState, html: bool) -> anyhow::Result<String> {
    let esc = |s: &str| if html { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") } else { s.to_string() };
    let cfg = state.config();
    let name = crate::editor::strip_codes(&cfg.bbs.name);
    let mut out = String::new();
    if html {
        out += &format!(
            "<!doctype html>\n<meta charset=\"utf-8\">\n<title>{} files</title>\n<style>body{{background:#000;color:#aaa;font:14px monospace}}h2{{color:#f5f}}b{{color:#fff}}</style>\n<h1>{}</h1>\n",
            esc(&name),
            esc(&name)
        );
    } else {
        out += &format!("{name}\n{}\n", "=".repeat(name.chars().count()));
    }
    let fa = state.fareas();
    let mut ok_at: Vec<bool> = Vec::new();
    for (n, depth) in fa.tree() {
        let a = &fa.areas[n as usize - 1];
        ok_at.truncate(depth);
        let ok = ok_at.last().copied().unwrap_or(true) && public(state, &a.acs) && a.password.is_empty();
        ok_at.push(ok);
        if !ok {
            continue;
        }
        let files: Vec<_> = state.files.list(&a.file)?.into_iter().filter(|f| f.valid).collect();
        let title = crate::editor::strip_codes(&a.name);
        if html {
            out += &format!("<h2>{}</h2>\n<pre>\n", esc(&title));
        } else {
            out += &format!("\n{title}\n{}\n", "-".repeat(title.chars().count()));
        }
        for f in &files {
            let date = f.uploaded.or(f.date).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
            let size = format!("{}k", f.size.div_ceil(1024));
            let plain = |l: &str| {
                let l = crate::editor::strip_colours(l);
                iniquity_term::cp437::decode(&crate::aichat::strip_escapes(&iniquity_term::cp437::encode(&l)))
            };
            let desc: Vec<String> = f.description.iter().map(|l| plain(l)).collect();
            let first = desc.first().cloned().unwrap_or_default();
            let fname = if html { format!("<b>{}</b>", esc(&f.name)) } else { f.name.clone() };
            let pad = " ".repeat(16usize.saturating_sub(f.name.chars().count()));
            out += &format!("{fname}{pad} {size:>8} {date:>10}  {}\n", esc(&first));
            for l in desc.iter().skip(1) {
                out += &format!("{:38}{}\n", "", esc(l));
            }
        }
        if html {
            out += "</pre>\n";
        }
    }
    Ok(out)
}

pub async fn serve(state: Arc<BbsState>) -> anyhow::Result<()> {
    let listen = state.config().web.listen.clone();
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("status feed on http://{listen}/status.json");
    let started = crate::clock::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let cache = Arc::new(Mutex::new(Cache { at: None, body: String::new() }));
    let lists = Arc::new([Mutex::new(Cache { at: None, body: String::new() }), Mutex::new(Cache { at: None, body: String::new() })]);
    // One rebuild at a time: requests that come meanwhile get its result.
    let building = Arc::new(tokio::sync::Mutex::new(()));
    // Requests served at once.
    let slots = Arc::new(tokio::sync::Semaphore::new(32));
    loop {
        let mut sock = match listener.accept().await {
            Ok((s, _)) => s,
            Err(e) => {
                tracing::warn!("status feed: accept: {e}");
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
        };
        let Ok(slot) = slots.clone().try_acquire_owned() else { continue };
        let (state, cache, started, lists, building) = (state.clone(), cache.clone(), started.clone(), lists.clone(), building.clone());
        tokio::spawn(async move {
            let _slot = slot;
            let mut buf = vec![0u8; 4096];
            let mut got = 0;
            // The request line and headers, within a few seconds.
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                while got < buf.len() && !buf[..got].windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf[got..]).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got += n,
                    }
                }
            });
            if read.await.is_err() {
                return;
            }
            let head = String::from_utf8_lossy(&buf[..got]);
            let mut words = head.split_whitespace();
            let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
            let path = path.split('?').next().unwrap_or("");
            let mut kind = "application/json";
            let (status, body) = if method != "GET" {
                ("405 Method Not Allowed", String::new())
            } else if path == "/files.txt" || path == "/files.html" {
                let html = path.ends_with("html");
                kind = if html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" };
                let slot = &lists[html as usize];
                let _b = building.lock().await;
                let fresh = slot.lock().unwrap().at.is_some_and(|t| t.elapsed() < FRESH * 6);
                if !fresh {
                    let st = state.clone();
                    match tokio::task::spawn_blocking(move || file_list(&st, html)).await {
                        Ok(Ok(body)) => *slot.lock().unwrap() = Cache { at: Some(Instant::now()), body },
                        Ok(Err(e)) => tracing::warn!("file list: {e:#}"),
                        Err(e) => tracing::warn!("file list: {e}"),
                    }
                }
                let body = slot.lock().unwrap().body.clone();
                if body.is_empty() { ("503 Service Unavailable", String::new()) } else { ("200 OK", body) }
            } else if path == "/status.json" || path == "/api/status.json" {
                let _b = building.lock().await;
                let fresh = cache.lock().unwrap().at.is_some_and(|t| t.elapsed() < FRESH);
                if !fresh {
                    let st = state.clone();
                    let s = started.clone();
                    match tokio::task::spawn_blocking(move || build(&st, &s)).await {
                        Ok(Ok(status)) => {
                            let mut c = cache.lock().unwrap();
                            c.body = serde_json::to_string(&status).unwrap_or_default();
                            c.at = Some(Instant::now());
                        }
                        Ok(Err(e)) => tracing::warn!("status feed: {e:#}"),
                        Err(e) => tracing::warn!("status feed: {e}"),
                    }
                }
                let body = cache.lock().unwrap().body.clone();
                if body.is_empty() { ("503 Service Unavailable", String::new()) } else { ("200 OK", body) }
            } else {
                ("404 Not Found", String::new())
            };
            let origin = state.config().web.allow_origin.clone();
            let cors = if origin.is_empty() { String::new() } else { format!("Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n") };
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(reply.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activities_stay_vague() {
        assert_eq!(activity("Menu: FILE"), "browsing files");
        assert_eq!(activity("Menu: SYSOP"), "looking around");
        assert_eq!(activity("Writing a message in Private Stuff"), "writing a message");
        assert_eq!(activity("Chatting with phrac"), "chatting");
        assert_eq!(activity("something new"), "online");
        assert_eq!(activity("Running LORD"), "in LORD");
        assert_eq!(activity("Running a door"), "playing a door game");
    }

    #[test]
    fn blurbs_skip_art_lines() {
        let d = ["    ▄█▀▀▄ ▄█▀▀ ▄█".to_string(), "  ACiD Productions, March 1995  ".to_string()];
        assert_eq!(blurb(&d), "ACiD Productions, March 1995");
        assert_eq!(blurb(&["░▒▓".to_string()]), "░▒▓");
        assert_eq!(blurb(&[]), "");
    }
}
