//! AI chat (`AI`; new): a conversation with a language model, for callers
//! the SysOp lets in (`[ai] acs`: level 250 and up, or flag I, as stock).
//! It talks to any OpenAI-style chat completions endpoint (`[ai] base_url`
//! and `model`), streams the reply as it's written, and can search the web
//! when the model asks to. Each call's conversation is forgotten when the
//! caller leaves it.

use std::io;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::input::Key;
use crate::session::Session;

/// Searches one reply may make before it must answer.
const MAX_TOOL_ROUNDS: usize = 4;
/// What a caller may type in one message.
const MAX_INPUT: usize = 500;
/// The conversation kept for the model: the system prompt and this many
/// of the latest messages.
const MAX_HISTORY: usize = 40;
/// The width replies are wrapped to.
const WIDTH: usize = 79;
/// Chat lines kept to look back over.
const SCROLLBACK: usize = 1000;

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// Decode `%XX` and `+` in a URL query value.
fn url_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
            (b'+', _, _) => out.push(b' '),
            (c, _, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Text from a bit of HTML: tags dropped, the common entities decoded.
fn html_text(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#x27;", "'").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
}

/// Results from DuckDuckGo's HTML page: (title, url, snippet).
fn parse_duckduckgo(html: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for block in html.split("class=\"result__a\"").skip(1) {
        let href = block.split("href=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_default();
        // Links go through DuckDuckGo's redirect; the target is `uddg`.
        let url = href.split("uddg=").nth(1).map(|u| url_decode(u.split('&').next().unwrap_or(u))).unwrap_or_else(|| href.to_string());
        let title = block.split_once('>').and_then(|(_, r)| r.split("</a>").next()).map(html_text).unwrap_or_default();
        let snippet = block
            .split("class=\"result__snippet\"")
            .nth(1)
            .and_then(|r| r.split_once('>'))
            .and_then(|(_, r)| r.split("</a>").next())
            .map(html_text)
            .unwrap_or_default();
        if !url.is_empty() && !title.is_empty() {
            out.push((title.trim().to_string(), url, snippet.trim().to_string()));
        }
        if out.len() >= 5 {
            break;
        }
    }
    out
}

/// Search the web: SearXNG's JSON at `searx` if set, else the Brave
/// Search API if `brave_key` is, else DuckDuckGo's HTML page (which works
/// until it decides the board is a bot).
async fn web_search(client: &reqwest::Client, searx: &str, brave_key: Option<&str>, query: &str) -> Result<String, String> {
    let results: Vec<(String, String, String)> = if let (true, Some(key)) = (searx.is_empty(), brave_key) {
        let url = format!("https://api.search.brave.com/res/v1/web/search?count=5&q={}", url_encode(query));
        let v: Value = client.get(url).header("X-Subscription-Token", key).header("Accept", "application/json").send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
        v["web"]["results"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(5)
                    .map(|r| (r["title"].as_str().unwrap_or("").to_string(), r["url"].as_str().unwrap_or("").to_string(), html_text(r["description"].as_str().unwrap_or(""))))
                    .collect()
            })
            .unwrap_or_default()
    } else if searx.is_empty() {
        let url = format!("https://html.duckduckgo.com/html/?q={}", url_encode(query));
        let html = client.get(url).header("User-Agent", "Mozilla/5.0 (flux/bbs)").send().await.map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        let found = parse_duckduckgo(&html);
        if found.is_empty() && !html.contains("No results") {
            return Err("the search engine is refusing requests right now".into());
        }
        found
    } else {
        let url = format!("{}/search?q={}&format=json", searx.trim_end_matches('/'), url_encode(query));
        let v: Value = client.get(url).send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
        v["results"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(5)
                    .map(|r| (r["title"].as_str().unwrap_or("").to_string(), r["url"].as_str().unwrap_or("").to_string(), r["content"].as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    if results.is_empty() {
        return Ok("No results.".into());
    }
    Ok(results.iter().enumerate().map(|(i, (t, u, s))| format!("{}. {t}\n{u}\n{s}", i + 1)).collect::<Vec<_>>().join("\n\n"))
}

/// The colour codes (`|XX` or ANSI) a string ends with, after its last
/// visible character: the colour text that follows it is in.
fn trailing_codes(s: &str) -> String {
    let b = s.as_bytes();
    let mut end_visible = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && b.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                i += 1;
            }
            i += 1;
        } else if b[i] == b'|' && i + 2 < b.len() && b[i + 1].is_ascii_alphanumeric() {
            i += 3;
        } else {
            i += 1;
            end_visible = i;
        }
    }
    s.get(end_visible..).unwrap_or_default().to_string()
}

/// Text without its ANSI escape sequences (which take no room).
pub(crate) fn strip_escapes(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && b.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                i += 1;
            }
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

/// An address shown short: no scheme, at most 60 characters.
fn squish_url(url: &str) -> String {
    let u = url.trim_start_matches("https://").trim_start_matches("http://");
    if u.chars().count() > 60 { format!("{}...", u.chars().take(57).collect::<String>()) } else { u.to_string() }
}

/// May the board fetch from this address? Not from itself, the LAN or
/// the tailnet: callers mustn't reach inside through the AI.
fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast() || a == 0
                || (a == 100 && (64..128).contains(&b)))
        }
        std::net::IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            !(v6.is_loopback() || v6.is_unspecified() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80)
                && v6.to_ipv4_mapped().is_none_or(|v4| public_ip(std::net::IpAddr::V4(v4)))
        }
    }
}

/// A web page as plain text, for the model: scripts and styles dropped,
/// tags stripped, blank space squeezed, cut to 8000 characters.
async fn read_page(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("only http and https".into());
    }
    let host = parsed.host_str().ok_or("no host")?.to_string();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<_> = tokio::net::lookup_host((host.as_str(), port)).await.map_err(|e| e.to_string())?.collect();
    if addrs.is_empty() || !addrs.iter().all(|a| public_ip(a.ip())) {
        return Err("that address isn't on the public internet".into());
    }
    let resp = client
        .get(parsed)
        .header("User-Agent", "Mozilla/5.0 (flux/bbs)")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(resp.status().to_string());
    }
    // At most 2MB of it.
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        if body.len() > 2 << 20 {
            break;
        }
    }
    let html = String::from_utf8_lossy(&body);
    let mut text = String::new();
    let mut rest: &str = &html;
    // Drop script and style blocks before stripping tags.
    while let Some(i) = rest.find('<') {
        text.push_str(&rest[..i]);
        rest = &rest[i..];
        let lower: String = rest.chars().take(8).collect::<String>().to_ascii_lowercase();
        let close = if lower.starts_with("<script") { Some("</script>") } else if lower.starts_with("<style") { Some("</style>") } else { None };
        match close {
            Some(end) => match rest.to_ascii_lowercase().find(end) {
                Some(j) => rest = &rest[j + end.len()..],
                None => rest = "",
            },
            None => match rest.find('>') {
                Some(j) => {
                    text.push(' ');
                    rest = &rest[j + 1..];
                }
                None => rest = "",
            },
        }
    }
    text.push_str(rest);
    let text = html_text(&text);
    let squeezed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(squeezed.chars().take(8000).collect())
}

/// Writes streamed text word-wrapped, without the markdown models like
/// to add, and without any `<think>` section or tool call written as text.
#[derive(Default)]
struct Wrap {
    col: usize,
    word: String,
    /// Inside a hidden section, until this closing tag.
    hiding: Option<&'static str>,
    pending: String,
}

impl Wrap {
    /// Text to show from `chunk`, with `\n` for line breaks.
    fn feed(&mut self, chunk: &str) -> String {
        const HIDDEN: [(&str, &str); 2] = [("<think>", "</think>"), ("<tool_call>", "</tool_call>")];
        self.pending.push_str(chunk);
        let mut text = String::new();
        loop {
            if let Some(close) = self.hiding {
                match self.pending.find(close) {
                    Some(i) => {
                        self.pending.drain(..i + close.len());
                        self.hiding = None;
                    }
                    None => {
                        // Keep what could be the start of the closing tag.
                        let keep = (0..=self.pending.len()).rev().find(|&c| self.pending.is_char_boundary(c) && self.pending.len() - c >= close.len().min(self.pending.len())).unwrap_or(0);
                        self.pending.drain(..keep);
                        return text;
                    }
                }
            }
            let next = HIDDEN.iter().filter_map(|(open, close)| self.pending.find(open).map(|i| (i, *open, *close))).min_by_key(|x| x.0);
            match next {
                Some((i, open, close)) => {
                    text.push_str(&self.pending[..i]);
                    self.pending.drain(..i + open.len());
                    self.hiding = Some(close);
                }
                None => {
                    // Hold back what could be the start of a tag.
                    let hold = self.pending.rfind('<').filter(|&i| HIDDEN.iter().any(|(open, _)| open.starts_with(&self.pending[i..]))).unwrap_or(self.pending.len());
                    text.push_str(&self.pending[..hold]);
                    self.pending.drain(..hold);
                    return text;
                }
            }
        }
    }

    /// Lay out `text`: returns what to print, `\n` meaning a new line.
    fn lay_out(&mut self, text: &str) -> String {
        let mut out = String::new();
        for c in text.chars() {
            match c {
                '*' | '`' | '#' if self.col == 0 || c != '#' => {}
                '\n' => {
                    self.flush(&mut out);
                    out.push('\n');
                    self.col = 0;
                }
                c if c.is_whitespace() => {
                    self.flush(&mut out);
                    if self.col > 0 && self.col < WIDTH {
                        out.push(' ');
                        self.col += 1;
                    }
                }
                c => self.word.push(c),
            }
        }
        out
    }

    fn flush(&mut self, out: &mut String) {
        if self.word.is_empty() {
            return;
        }
        let len = self.word.chars().count();
        if self.col > 0 && self.col + len > WIDTH {
            // Drop the space that ended the last line.
            if out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
            self.col = 0;
        }
        out.push_str(&self.word);
        self.col += len;
        self.word.clear();
    }

    fn finish(&mut self) -> String {
        let mut out = String::new();
        self.flush(&mut out);
        out
    }
}

/// A tool call being put together from streamed pieces.
/// What a reply cost, from the server's `usage` and `timings` (new).
#[derive(Default)]
struct Usage {
    /// The last request's prompt: the conversation so far.
    prompt: u64,
    /// Written over every round of the reply, thinking included.
    completion: u64,
    reasoning: u64,
    per_second: f64,
}

impl Usage {
    /// Tokens a second from the clock, when the server doesn't say.
    fn finish(mut self, started: std::time::Instant) -> Self {
        let secs = started.elapsed().as_secs_f64();
        if self.per_second == 0.0 && secs > 0.0 {
            self.per_second = self.completion as f64 / secs;
        }
        self
    }
}

/// 1234 as "1.2k", 200000 as "200k".
fn kilo(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..10_000 => format!("{:.1}k", n as f64 / 1000.0),
        _ => format!("{}k", n / 1000),
    }
}

/// The model's context length, if the server says (`max_model_len`,
/// `context_length` or `context_window` in `/models`).
async fn context_len(client: &reqwest::Client, cfg: &crate::config::Ai) -> Option<u64> {
    let url = format!("{}/models", cfg.base_url.trim_end_matches('/'));
    let mut req = client.get(&url).timeout(Duration::from_secs(5));
    if let Some(k) = (!cfg.api_key_env.is_empty()).then(|| std::env::var(&cfg.api_key_env).ok()).flatten() {
        req = req.bearer_auth(k);
    }
    let v: Value = req.send().await.ok()?.json().await.ok()?;
    let m = v["data"].as_array()?.iter().find(|m| m["id"].as_str() == Some(cfg.model.as_str()))?;
    ["max_model_len", "context_length", "context_window"].iter().find_map(|k| m[*k].as_u64())
}

/// A piece of a chat line as it was shown: a string with codes, or text
/// written as it is.
#[derive(Clone)]
pub(crate) enum AiSeg {
    Codes(String),
    Text(String),
}

/// The rows between the header and the bar (new). Rather than have the
/// terminal scroll them (SyncTERM's scroll regions leave old text showing
/// through), each line is kept as it was shown, and when the last row is
/// full the rows are drawn again one higher.
pub(crate) struct AiWindow {
    /// The first row.
    top: u16,
    /// The finished lines, the latest last.
    lines: Vec<Vec<AiSeg>>,
    /// The line being written.
    cur: Vec<AiSeg>,
    /// A reply is being shown, pausing when it fills the rows (if the
    /// caller pauses), and the lines it's shown since the last pause.
    pausing: bool,
    since: usize,
}

fn looks_back(k: Key) -> bool {
    matches!(k, Key::Up | Key::Down | Key::PgUp | Key::PgDn | Key::Home | Key::End)
}

#[derive(Default, Clone)]
struct Call {
    id: String,
    name: String,
    args: String,
}

impl Session {
    /// `AI`: chat with the board's AI.
    pub async fn ai_chat(&mut self) -> io::Result<()> {
        let cfg = self.state.config().ai.clone();
        if !cfg.enabled || cfg.base_url.is_empty() || cfg.model.is_empty() {
            self.newline();
            return self.say_line("AiOff").await;
        }
        if !self.acs(&cfg.acs) {
            self.newline();
            return self.say_line("AiNoAccess").await;
        }
        // No client (say, no CA certificates on the host): say so and go
        // back to the menu rather than dropping the caller.
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(cfg.timeout_seconds.max(10))).build() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("{}: ai chat: {e}", self.user.handle);
                self.newline();
                return self.say_line("AiUnavailable").await;
            }
        };
        self.full_screen_done();
        let r = self.ai_session(&cfg, &client).await;
        self.ai_win = None;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        r
    }

    async fn ai_session(&mut self, cfg: &crate::config::Ai, client: &reqwest::Client) -> io::Result<()> {
        let cfg = cfg.clone();
        tracing::info!("{}: ai chat", self.user.handle);
        self.node.update(|n| n.activity = "Chatting with the AI".into());
        let system = self.ai_system_prompt(&cfg);
        let mut history: Vec<Value> = Vec::new();
        let max_ctx = context_len(client, &cfg).await;
        self.ai_ctx.clear();
        self.ai_header(&cfg).await?;
        loop {
            self.ai_nl().await?;
            let you = self.strings().fmt("AiYou", &[&self.user.handle]);
            self.ai_show(&you).await?;
            self.ai_ink = trailing_codes(&you);
            let Some(line) = self.ai_read_line().await? else { break };
            let line = line.trim().to_string();
            match line.to_ascii_lowercase().as_str() {
                "" => continue,
                "/quit" | "/q" | "/exit" | "q" => break,
                "/new" | "/clear" => {
                    history.clear();
                    self.ai_ctx.clear();
                    self.ai_header(&cfg).await?;
                    continue;
                }
                "/help" | "/?" | "?" => {
                    self.ai_nl().await?;
                    for key in ["AiHelp1", "AiHelp2", "AiHelp3", "AiHelp4"] {
                        { let s = self.strings().get(key).to_string(); self.ai_line(&s).await?; }
                    }
                    continue;
                }
                _ => {}
            }
            if !self.ai_count(cfg.daily_limit) {
                self.ai_nl().await?;
                let s = self.strings().fmt("AiLimit", &[&cfg.daily_limit.to_string()]);
                self.ai_line(&s).await?;
                continue;
            }
            history.push(json!({"role": "user", "content": line}));
            if history.len() > MAX_HISTORY {
                history.drain(..history.len() - MAX_HISTORY);
            }
            let replied = self.ai_reply(client, &cfg, &system, &mut history).await;
            self.ai_stop_pausing();
            if let Ok(u) = &replied
                && u.prompt > 0
            {
                self.ai_stats(u, max_ctx).await?;
            }
            self.ai_bar(&cfg).await?;
            if let Err(e) = replied {
                if e.kind() == io::ErrorKind::Other {
                    self.ai_nl().await?;
                    let s = self.strings().fmt("AiError", &[&e.to_string()]);
                    self.ai_line(&s).await?;
                    tracing::warn!("{}: ai chat: {e}", self.user.handle);
                } else {
                    return Err(e);
                }
            }
        }
        self.ai_nl().await?;
        Ok(())
    }

    fn ai_system_prompt(&self, cfg: &crate::config::Ai) -> String {
        let bbs = &self.state.config().bbs;
        let search = if cfg.web_search { " You can search the web with the web_search tool when you need current or specific information; say briefly where facts came from." } else { "" };
        let search = format!(" You can look at this board itself with the board tools (its menus and keys, file areas, files, message areas): use them for questions about the board, and when saying how to do something here, give the exact keys from board_menus, menu by menu.{search}");
        format!(
            "You are {name}, the AI on {board}, a bulletin board system run by {sysop}. You're talking with {user} over a text terminal 80 columns wide. \
             Write plain text only: no markdown, no emoji, no tables. Keep replies short unless asked for more.{search} \
             Today is {today}.{extra}",
            name = cfg.name,
            board = bbs.name,
            sysop = bbs.sysop_alias,
            user = self.user.handle,
            today = crate::clock::now().format("%A, %B %-d, %Y"),
            extra = if cfg.system_prompt.is_empty() { String::new() } else { format!(" {}", cfg.system_prompt) },
        )
    }

    /// A fresh screen: AICHAT.ANS if the board has one (its header), else
    /// a plain one. The conversation scrolls between the header and the
    /// help bar on the bottom line, so both stay put.
    async fn ai_header(&mut self, cfg: &crate::config::Ai) -> io::Result<()> {
        self.ai_win = None;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        if !self.show_file("AICHAT").await? {
            let s = self.strings().fmt("AiHeader", &[&cfg.name, &cfg.model]);
            self.ai_line(&s).await?;
            self.rule().await?;
            { let s = self.strings().get("AiHelp").to_string(); self.ai_line(&s).await?; }
        }
        if self.screen.where_x() > 1 {
            self.ai_nl().await?;
        }
        // A header too tall to leave a few rows scrolls with the chat.
        let (top, rows) = (self.screen.where_y(), self.screen.rows());
        if top + 4 < rows {
            self.ai_win = Some(AiWindow { top, lines: Vec::new(), cur: Vec::new(), pausing: false, since: 0 });
        }
        self.ai_bar(cfg).await
    }

    /// The help bar on the bottom line, with the messages left today.
    async fn ai_bar(&mut self, cfg: &crate::config::Ai) -> io::Result<()> {
        let (x, y) = (self.screen.where_x(), self.screen.where_y());
        let left = if cfg.daily_limit == 0 || self.is_sysop() {
            self.strings().get("AiUnlimited").to_string()
        } else {
            let used = self.state.ai_usage.lock().unwrap().get(&(self.user.id, crate::clock::now().date_naive())).copied().unwrap_or(0);
            self.strings().fmt("AiLeft", &[&cfg.daily_limit.saturating_sub(used).to_string()])
        };
        self.screen.goto(1, self.screen.rows());
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
        let bar = self.strings().fmt("AiBar", &[&left, &self.ai_ctx.clone()]);
        self.ai_bar = bar.clone();
        self.show(&bar).await?;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.goto(x, y);
        self.flush().await
    }

    /// Count a message against the day's limit; false if it's used up.
    fn ai_count(&mut self, limit: u32) -> bool {
        if limit == 0 || self.is_sysop() {
            return true;
        }
        let key = (self.user.id, crate::clock::now().date_naive());
        let mut used = self.state.ai_usage.lock().unwrap();
        used.retain(|(_, day), _| *day == key.1);
        let n = used.entry(key).or_insert(0);
        if *n >= limit {
            return false;
        }
        *n += 1;
        true
    }

    /// A line of up to MAX_INPUT characters, wrapping onto the lines below
    /// as it goes. None for ESC.
    async fn ai_read_line(&mut self) -> io::Result<Option<String>> {
        let mut s = String::new();
        // Where each character went, to back up over the wrap.
        let mut at: Vec<(u16, u16)> = Vec::new();
        loop {
            let mut key = self.read_key().await?;
            if looks_back(key) {
                let Some(k) = self.ai_look_back(key).await? else { continue };
                key = k;
            }
            match key {
                Key::Enter => {
                    self.ai_nl().await?;
                    return Ok(Some(s));
                }
                Key::Esc => return Ok(None),
                Key::Backspace => {
                    if let Some((x, y)) = at.pop() {
                        s.pop();
                        self.ai_unrecord();
                        self.screen.goto(x, y);
                        self.write(" ");
                        self.screen.goto(x, y);
                    }
                }
                Key::Char(c @ 0x20..=0x7E) if s.chars().count() < MAX_INPUT => {
                    if self.screen.where_x() >= WIDTH as u16 {
                        // On the last row the chat scrolls: what's been
                        // typed moves up with it.
                        if self.screen.where_y() >= self.ai_last_row() {
                            at.iter_mut().for_each(|p| p.1 = p.1.saturating_sub(1).max(1));
                        }
                        self.ai_nl().await?;
                    }
                    at.push((self.screen.where_x(), self.screen.where_y()));
                    s.push(c as char);
                    self.ai_text(&(c as char).to_string());
                }
                _ => {}
            }
        }
    }

    /// After a reply: what it took (`AiStats`, unless blanked), and the
    /// context used so far for the bar (`AiContext`).
    async fn ai_stats(&mut self, u: &Usage, max_ctx: Option<u64>) -> io::Result<()> {
        let s = self.strings().fmt(
            "AiStats",
            &[&crate::files::commas(u.completion), &crate::files::commas(u.reasoning), &format!("{:.0}", u.per_second)],
        );
        if !s.is_empty() {
            self.ai_line(&s).await?;
        }
        let used = u.prompt + u.completion;
        let (max, pct) = match max_ctx {
            Some(m) if m > 0 => (kilo(m), format!("{}", used * 100 / m)),
            _ => ("?".into(), "?".into()),
        };
        self.ai_ctx = self.strings().fmt("AiContext", &[&kilo(used), &max, &pct]);
        Ok(())
    }

    /// Ask the model, showing its reply as it comes and running any
    /// searches it wants on the way.
    async fn ai_reply(&mut self, client: &reqwest::Client, cfg: &crate::config::Ai, system: &str, history: &mut Vec<Value>) -> io::Result<Usage> {
        let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
        let key = (!cfg.api_key_env.is_empty()).then(|| std::env::var(&cfg.api_key_env).ok()).flatten();
        let mut tools = crate::aiboard::tool_specs();
        if cfg.web_search {
            tools.push(json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web. Returns the top results: title, address and a snippet.",
                "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": "What to search for"}}, "required": ["query"]}
            }
            }));
        }
        if cfg.web_search && cfg.read_pages {
            tools.push(json!({
                "type": "function",
                "function": {
                    "name": "read_page",
                    "description": "Read a web page (from search results) as plain text, when the snippet isn't enough.",
                    "parameters": {"type": "object", "properties": {"url": {"type": "string", "description": "The page's address (http or https)"}}, "required": ["url"]}
                }
            }));
        }
        let tools = Value::Array(tools);
        self.ai_nl().await?;
        let name = self.strings().fmt("AiName", &[&cfg.name]);
        let mut usage = Usage::default();
        let started = std::time::Instant::now();
        let pause = self.user.pause;
        if let Some(w) = &mut self.ai_win {
            (w.pausing, w.since) = (pause, 0);
        }
        for round in 0..=MAX_TOOL_ROUNDS {
            let mut messages = vec![json!({"role": "system", "content": system})];
            messages.extend(history.iter().cloned());
            if round == MAX_TOOL_ROUNDS {
                messages.push(json!({"role": "system", "content": "No more searches. Answer now from what you have."}));
            }
            let mut body = json!({
                "model": cfg.model, "messages": messages, "stream": true, "max_tokens": cfg.max_tokens,
                "stream_options": {"include_usage": true},
            });
            if !cfg.reasoning_effort.is_empty() {
                body["reasoning_effort"] = json!(cfg.reasoning_effort);
            }
            if round < MAX_TOOL_ROUNDS {
                body["tools"] = tools.clone();
            }
            let mut req = client.post(&url).json(&body);
            if let Some(k) = &key {
                req = req.bearer_auth(k);
            }
            let resp = req.send().await.map_err(io_err)?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(io_err(format!("{status}: {}", text.chars().take(200).collect::<String>())));
            }
            // Read the reply in its own task, so it keeps coming in while
            // the caller is at a pause.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let mut stream = resp.bytes_stream();
            tokio::spawn(async move {
                while let Some(chunk) = stream.next().await {
                    if tx.send(chunk).is_err() {
                        break;
                    }
                }
            });
            let mut buf = Vec::new();
            let mut content = String::new();
            let mut calls: Vec<Call> = Vec::new();
            // Lines start after the name, as wide as it shows.
            let indent = self.visible_len(&strip_escapes(&fluxbbs_term::cp437::encode(&name)));
            let mut wrap = Wrap { col: indent, ..Wrap::default() };
            let mut named = false;
            'stream: while let Some(chunk) = rx.recv().await {
                buf.extend_from_slice(&chunk.map_err(io_err)?);
                // Server-sent events: one `data: {...}` per line.
                while let Some(i) = buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=i).collect();
                    let line = String::from_utf8_lossy(&line);
                    let Some(data) = line.trim().strip_prefix("data:") else { continue };
                    let data = data.trim();
                    if data == "[DONE]" {
                        break 'stream;
                    }
                    let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                    // The last chunk, asked for with include_usage.
                    if let Some(u) = v["usage"].as_object() {
                        usage.prompt = u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
                        usage.completion += u.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0);
                        usage.reasoning += v["usage"]["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0);
                    }
                    if let Some(ps) = v["timings"]["predicted_per_second"].as_f64() {
                        usage.per_second = ps;
                    }
                    let delta = &v["choices"][0]["delta"];
                    if let Some(t) = delta["content"].as_str() {
                        content.push_str(t);
                        let shown = wrap.feed(t);
                        let out = wrap.lay_out(&shown);
                        // The name goes up with the first words, not before
                        // a search.
                        if !out.trim().is_empty() && !named {
                            self.ai_show(&name).await?;
                            self.ai_ink = trailing_codes(&name);
                            named = true;
                        }
                        self.ai_write(&out).await?;
                        self.flush().await?;
                    }
                    for tc in delta["tool_calls"].as_array().into_iter().flatten() {
                        let i = tc["index"].as_u64().unwrap_or(0) as usize;
                        if calls.len() <= i {
                            calls.resize(i + 1, Call::default());
                        }
                        let c = &mut calls[i];
                        if let Some(id) = tc["id"].as_str() {
                            c.id = id.to_string();
                        }
                        if let Some(n) = tc["function"]["name"].as_str() {
                            c.name.push_str(n);
                        }
                        if let Some(a) = tc["function"]["arguments"].as_str() {
                            c.args.push_str(a);
                        }
                    }
                }
            }
            let rest = wrap.finish();
            // A reply too short to wrap hasn't put the name up yet.
            if !rest.trim().is_empty() && !named {
                self.ai_show(&name).await?;
                self.ai_ink = trailing_codes(&name);
                named = true;
            }
            self.ai_write(&rest).await?;
            if calls.is_empty() {
                history.push(json!({"role": "assistant", "content": content}));
                self.ai_nl().await?;
                self.ai_stop_pausing();
                return Ok(usage.finish(started));
            }
            // The model wants to search: do it, and ask again.
            let tool_calls: Vec<Value> = calls
                .iter()
                .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.args}}))
                .collect();
            history.push(json!({"role": "assistant", "content": content, "tool_calls": tool_calls}));
            for c in &calls {
                let args = serde_json::from_str::<Value>(&c.args).unwrap_or_default();
                let query = args["query"].as_str().unwrap_or_default().to_string();
                if named {
                    self.ai_nl().await?;
                    named = false;
                }
                if let Some(result) = self.ai_board_tool(&c.name, &args) {
                    let what = match args["area"].as_str().or(args["query"].as_str()) {
                        Some(a) => format!("{} {a}", c.name.replace('_', " ")),
                        None => c.name.replace('_', " "),
                    };
                    let s = self.strings().fmt("AiLooking", &[&what]);
                    self.ai_line(&s).await?;
                    tracing::info!("{}: ai {what}", self.user.handle);
                    history.push(json!({"role": "tool", "tool_call_id": c.id, "content": result}));
                    continue;
                }
                if c.name == "read_page" && cfg.read_pages {
                    let url = args["url"].as_str().unwrap_or_default().to_string();
                    let s = self.strings().fmt("AiReading", &[&squish_url(&url)]);
                    self.ai_line(&s).await?;
                    tracing::info!("{}: ai read {url:?}", self.user.handle);
                    let result = read_page(client, &url).await.unwrap_or_else(|e| format!("Couldn't read it ({e})."));
                    history.push(json!({"role": "tool", "tool_call_id": c.id, "content": result}));
                    continue;
                }
                let s = self.strings().fmt("AiSearching", &[&query]);
                self.ai_line(&s).await?;
                tracing::info!("{}: ai search {query:?}", self.user.handle);
                let result = if c.name == "web_search" && !query.is_empty() {
                    let brave = (!cfg.search_key_env.is_empty()).then(|| std::env::var(&cfg.search_key_env).ok()).flatten();
                    web_search(client, &cfg.search_url, brave.as_deref(), &query)
                        .await
                        .unwrap_or_else(|e| format!("Search failed ({e}). Don't search again for this; answer from what you know, and say so."))
                } else {
                    format!("No tool called {}.", c.name)
                };
                history.push(json!({"role": "tool", "tool_call_id": c.id, "content": result}));
            }
        }
        self.ai_nl().await?;
        self.ai_stop_pausing();
        Ok(usage.finish(started))
    }

    /// A new line in the chat. Under a header (`ai_win`), at the last
    /// row the rows are drawn again one higher. Without one (a header too
    /// tall for it) the whole screen scrolls below the last row: the bar
    /// is cleared first, so it doesn't scroll up with the text, and drawn
    /// again on the bottom line.
    async fn ai_nl(&mut self) -> io::Result<()> {
        let rows = self.screen.rows();
        let ink = self.ai_ink.clone();
        if let Some(mut win) = self.ai_win.take() {
            win.lines.push(std::mem::take(&mut win.cur));
            if win.lines.len() > SCROLLBACK {
                win.lines.drain(..win.lines.len() - SCROLLBACK);
            }
            let (top, last) = (win.top, rows - 1);
            win.since += 1;
            let full = win.pausing && win.since + 1 >= (last - top + 1) as usize;
            self.ai_win = Some(win);
            if self.screen.where_y() < last {
                self.newline();
            } else {
                self.ai_paint(0).await?;
                self.screen.goto(1, last);
            }
            // The text's own colour again (it may be 24-bit, which the
            // screen model only knows roughly).
            if !ink.is_empty() {
                self.show(&ink).await?;
                if let Some(win) = &mut self.ai_win {
                    win.cur.push(AiSeg::Codes(ink));
                }
            }
            if full {
                self.ai_more().await?;
            }
            return Ok(());
        }
        if self.screen.where_y() < self.ai_last_row() {
            self.newline();
            return Ok(());
        }
        let color = self.screen.color();
        self.screen.goto(1, rows);
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
        self.newline();
        let bar = self.ai_bar.clone();
        self.show(&bar).await?;
        self.screen.goto(1, rows - 1);
        if ink.is_empty() {
            self.screen.set_color(color);
        } else {
            self.show(&ink).await?;
        }
        Ok(())
    }

    /// Draw the rows from the kept lines, `back` lines up from the latest
    /// (the line being written is the latest).
    async fn ai_paint(&mut self, back: usize) -> io::Result<()> {
        let Some(win) = self.ai_win.take() else { return Ok(()) };
        let height = (self.screen.rows() - win.top) as usize;
        let total = win.lines.len() + 1;
        let end = total.saturating_sub(back);
        let start = end.saturating_sub(height);
        for row in 0..height {
            self.screen.goto(1, win.top + row as u16);
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            let i = start + row;
            let line = if i >= end { None } else { win.lines.get(i).or(Some(&win.cur)) };
            for seg in line.into_iter().flatten() {
                match seg {
                    AiSeg::Codes(c) => self.show(c).await?,
                    AiSeg::Text(t) => self.write(t),
                }
            }
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.screen.clear_eol();
        }
        self.ai_win = Some(win);
        Ok(())
    }

    /// Something in place of the bar on the bottom row, the cursor left
    /// where it was.
    async fn ai_bar_says(&mut self, s: &str) -> io::Result<()> {
        let (x, y) = (self.screen.where_x(), self.screen.where_y());
        self.screen.goto(1, self.screen.rows());
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
        self.show(s).await?;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.goto(x, y);
        Ok(())
    }

    fn ai_stop_pausing(&mut self) {
        if let Some(w) = &mut self.ai_win {
            w.pausing = false;
        }
    }

    /// A reply has filled the rows: wait (`AiMore`) for a key, which may
    /// look back first; esc lets the rest of the reply run on.
    async fn ai_more(&mut self) -> io::Result<()> {
        let s = self.strings().get("AiMore").to_string();
        self.ai_bar_says(&s).await?;
        let mut key = self.read_key().await?;
        while looks_back(key) {
            match self.ai_look_back(key).await? {
                Some(k) => key = k,
                None => {
                    self.ai_bar_says(&s).await?;
                    key = self.read_key().await?;
                }
            }
        }
        if let Some(w) = &mut self.ai_win {
            w.since = 0;
            if key == Key::Esc {
                w.pausing = false;
            }
        }
        let bar = self.ai_bar.clone();
        self.ai_bar_says(&bar).await
    }

    /// Look back over the chat (new): up and down a line, page up and
    /// down, home to the start; end, or coming back down, returns to the
    /// latest (None). Any other key returns too, and is handed back.
    async fn ai_look_back(&mut self, mut key: Key) -> io::Result<Option<Key>> {
        let Some(win) = &self.ai_win else { return Ok(Some(key)) };
        let height = (self.screen.rows() - win.top) as usize;
        let most = (win.lines.len() + 1).saturating_sub(height);
        if most == 0 {
            return Ok(None);
        }
        let (x, y) = (self.screen.where_x(), self.screen.where_y());
        let ink = self.ai_ink.clone();
        let mut back = 0usize;
        let handed = loop {
            back = match key {
                Key::Up => back + 1,
                Key::Down => back.saturating_sub(1),
                Key::PgUp => back + height - 1,
                Key::PgDn => back.saturating_sub(height - 1),
                Key::Home => most,
                Key::End => 0,
                other => break Some(other),
            }
            .min(most);
            if back == 0 {
                break None;
            }
            self.ai_paint(back).await?;
            let s = self.strings().fmt("AiBack", &[&back.to_string()]);
            self.ai_bar_says(&s).await?;
            key = self.read_key().await?;
        };
        self.ai_paint(0).await?;
        let bar = self.ai_bar.clone();
        self.ai_bar_says(&bar).await?;
        self.screen.goto(x, y);
        if !ink.is_empty() {
            self.show(&ink).await?;
        }
        Ok(handed)
    }

    /// The chat's last row, the one above the bar.
    fn ai_last_row(&self) -> u16 {
        self.screen.rows() - 1
    }

    /// Show a string with codes in the chat, kept for redrawing.
    async fn ai_show(&mut self, s: &str) -> io::Result<()> {
        self.show(s).await?;
        if let Some(win) = &mut self.ai_win {
            win.cur.push(AiSeg::Codes(s.to_string()));
        }
        Ok(())
    }

    /// Write text in the chat as it is, kept for redrawing.
    fn ai_text(&mut self, t: &str) {
        self.write(t);
        if let Some(win) = &mut self.ai_win {
            match win.cur.last_mut() {
                Some(AiSeg::Text(s)) => s.push_str(t),
                _ => win.cur.push(AiSeg::Text(t.to_string())),
            }
        }
    }

    /// Forget the last character typed (backspace), from this line or,
    /// if it wrapped, the one before.
    fn ai_unrecord(&mut self) {
        let Some(win) = &mut self.ai_win else { return };
        let pop = |line: &mut Vec<AiSeg>| {
            line.iter_mut().rev().find_map(|seg| match seg {
                AiSeg::Text(s) => s.pop(),
                AiSeg::Codes(_) => None,
            })
        };
        if pop(&mut win.cur).is_none()
            && let Some(prev) = win.lines.last_mut()
        {
            pop(prev);
        }
    }

    /// A line of text in the chat.
    async fn ai_line(&mut self, s: &str) -> io::Result<()> {
        self.ai_show(s).await?;
        self.ai_nl().await
    }

    async fn ai_write(&mut self, text: &str) -> io::Result<()> {
        // CP437 has no typographic dashes, quotes or ellipsis.
        let text = text
            .replace(['\u{2014}', '\u{2013}', '\u{2212}'], "-")
            .replace(['\u{2018}', '\u{2019}'], "'")
            .replace(['\u{201c}', '\u{201d}'], "\"")
            .replace('\u{2026}', "...")
            .replace('\u{a0}', " ");
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.ai_nl().await?;
            }
            self.ai_text(line);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_wrap_and_lose_markdown_and_thinking() {
        let mut w = Wrap::default();
        let mut out = String::new();
        for chunk in ["<thi", "nk>hmm, let me", " think</th", "ink>**Hello** there, `world`.", "<tool_call>web_search</tool_call>", " ", &"word ".repeat(20)] {
            let t = w.feed(chunk);
            out.push_str(&w.lay_out(&t));
        }
        out.push_str(&w.finish());
        assert!(out.starts_with("Hello there, world."), "{out:?}");
        assert!(!out.contains("think") && !out.contains('*') && !out.contains("web_search"));
        assert!(out.split('\n').all(|l| l.chars().count() <= WIDTH));
    }

    #[test]
    fn colour_after_the_name() {
        assert_eq!(trailing_codes("|U5(|U6ai|U5) |U1"), "|U1");
        assert_eq!(trailing_codes("(ai) \x1b[38;2;1;2;3m"), "\x1b[38;2;1;2;3m");
        assert_eq!(trailing_codes("plain"), "");
    }

    #[test]
    fn pages_only_from_the_public_internet() {
        let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap();
        assert!(public_ip(ip("93.184.216.34")));
        for private in ["127.0.0.1", "10.1.2.3", "192.168.1.7", "172.16.0.1", "100.111.65.25", "169.254.1.1", "::1", "fd7a:115c::1", "::ffff:192.168.1.1"] {
            assert!(!public_ip(ip(private)), "{private}");
        }
    }

    #[test]
    fn duckduckgo_results() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1&amp;rut=x">An <b>example</b> &amp; more</a>
            <a class="result__snippet" href="x">Snippet <b>text</b></a>"#;
        let r = parse_duckduckgo(html);
        assert_eq!(r, vec![("An example & more".to_string(), "https://example.com/a?b=1".to_string(), "Snippet text".to_string())]);
        assert_eq!(url_encode("a b&c"), "a+b%26c");
    }
}
