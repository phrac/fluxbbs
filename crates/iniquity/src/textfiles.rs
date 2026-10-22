//! `iniquity textfiles`: the textfiles.com archive as file areas (new).
//!
//!     iniquity textfiles fetch [DIR] [--mirror URL] [--archives URL]
//!                              [--sections a,b,...] [--refresh]
//!     iniquity textfiles import [CONFIG] [DIR] [--acs ACS]
//!                               [--download-acs ACS] [--sponsor WHO]
//!                               [--under KEY] [--put SECTION=KEY,...]
//!
//! `fetch` reads the mirror's index pages (the catalogue: every directory
//! with its heading, every file with its size and description), takes
//! each section's tarball from archives.textfiles.com for the bulk of
//! the files, and fetches from the mirror whatever the tarball lacks or
//! has an older copy of. Pages, tarballs and the catalogue are kept
//! under `DIR/.textfiles`, so a run that was stopped carries on where it
//! was. `import` adds the site's sections as file areas of the board's
//! own (Anarchy, Hack, Phreak..., as a board of the day had them) with
//! the directories under each as sub-areas, and a record per catalogued
//! file that's on disk, leaving alone what's already there; it needs the
//! board stopped, as the running board keeps `fileareas.toml` in memory.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::fileareas::{FileArea, FileAreas, SortBy};
use crate::filebase::{FileBase, FileRec};

const MIRROR: &str = "https://textfiles.meulie.net";
const ARCHIVES: &str = "http://archives.textfiles.com";
/// Sections the directory page doesn't link but the site has.
const EXTRA_SECTIONS: &[&str] = &["history"];
/// Where pages, tarballs and the catalogue are kept, under the files.
const CACHE: &str = ".textfiles";
const USER_AGENT: &str = "IniquityBBS-textfiles/0.1";
/// The storage key of the catalogue's root, were it an area (it isn't:
/// the sections are the areas); theirs are `TF_<PATH>`.
pub const ROOT_KEY: &str = "TEXTFILES";
/// The lister's description column is 52 wide with the stock layout.
const DESC_WIDTH: usize = 50;
const MAX_DESC_LINES: usize = 20;
/// The area lists show 30 columns of a name.
const MAX_NAME: usize = 30;

/// A directory of the site, as catalogued from its index page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Dir {
    /// Path on the site (`anarchy/CARDING`); empty for the root.
    pub path: String,
    /// The link's text in the directory above (`Anarchy`, `CARDING`).
    pub label: String,
    /// The page's heading (`Anarchy: Credit Card Fraud`), if it has one.
    #[serde(default)]
    pub title: String,
    /// What the directory above says of it.
    #[serde(default)]
    pub blurb: String,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub dirs: Vec<Dir>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub name: String,
    /// As the index says; None for a link with no size cell.
    pub size: Option<u64>,
    #[serde(default)]
    pub desc: Vec<String>,
}

impl Dir {
    /// The directory's own name: the last part of the path.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }

    /// This directory and everything under it, depth first.
    pub fn walk(&self) -> Vec<&Dir> {
        let mut out = vec![self];
        for d in &self.dirs {
            out.extend(d.walk());
        }
        out
    }

    /// Files under this directory (and below) by site path.
    pub fn all_files(&self) -> Vec<(String, &File)> {
        self.walk().into_iter().flat_map(|d| d.files.iter().map(move |f| (join(&d.path, &f.name), f))).collect()
    }

    /// The area's storage key: `TEXTFILES` for the root, else `TF_` and
    /// the path with anything but letters and digits as `_`.
    pub fn key(&self) -> String {
        if self.path.is_empty() {
            return ROOT_KEY.into();
        }
        let p: String = self.path.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
        format!("TF_{p}")
    }

    /// The area's name. A section goes by the directory page's name for
    /// it (`Anarchy`, `Sex and Sexuality`), unless that's just a number
    /// (`100`), when its heading does. A directory lower down goes by its
    /// heading less the part up to the first colon (`Anarchy: Credit Card
    /// Fraud` is under Anarchy already), else what its parent says of
    /// it, else its name.
    pub fn area_name(&self) -> String {
        let depth = self.path.matches('/').count() + !self.path.is_empty() as usize;
        let pick = match depth {
            0 => "textfiles.com".to_string(),
            1 if !self.label.trim().is_empty() && !self.label.trim().chars().all(|c| c.is_ascii_digit()) => self.label.trim().to_string(),
            _ => {
                let t = self.title.trim();
                let t = t.split_once(": ").map_or(t, |(_, rest)| rest).trim();
                [t, self.blurb.trim(), self.label.trim(), self.name()].into_iter().find(|s| !s.is_empty()).unwrap_or("?").to_string()
            }
        };
        let mut out: String = pick.chars().take(MAX_NAME).collect();
        if pick.chars().count() > MAX_NAME
            && let Some(sp) = out.rfind(' ').filter(|&i| i > MAX_NAME / 2) {
                out.truncate(sp);
            }
        out
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }
}

// ---- index pages ------------------------------------------------------

/// A link on an index page: a file (with its size) or a directory.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub label: String,
    pub size: Option<u64>,
    pub desc: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Page {
    pub title: String,
    pub entries: Vec<Entry>,
}

/// The HTML's `name="value"` (or `name=value`) attribute.
fn attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        if low[..at].ends_with(|c: char| !c.is_whitespace()) {
            continue;
        }
        let rest = tag[at + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let v = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(v.to_string());
    }
    None
}

/// A link to something in the same directory: not a URL, not a path
/// (a directory listing's `SUB/` counts).
fn relative(href: &str) -> bool {
    let href = href.strip_suffix('/').unwrap_or(href);
    !href.is_empty()
        && !href.contains("://")
        && !href.contains('/')
        && !href.starts_with(['#', '?', '.'])
        && !href.to_ascii_lowercase().starts_with("mailto:")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len()
            && let Ok(h) = std::str::from_utf8(&b[i + 1..i + 3])
            && let Ok(v) = u8::from_str_radix(h, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| latin1(e.as_bytes()))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'!' | b'$' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// Page bytes as text: UTF-8 if they are, else Latin-1.
pub fn decode(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap_or_else(|_| latin1(b))
}

fn entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        let semi = rest[end..].starts_with(';');
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n if n.starts_with('#') => {
                let d = &n[1..];
                if let Some(h) = d.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { d.parse().ok() }.and_then(char::from_u32)
            }
            _ => None,
        };
        match decoded {
            Some(c) if !name.is_empty() => {
                out.push(c);
                rest = &rest[end + semi as usize..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// HTML as lines of text: tags dropped (`<br>` and `<p>` end a line),
/// entities decoded, blank space squeezed, empty lines left out.
pub fn text_lines(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        text.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find('>') else { break };
        let tag = rest[1..end].trim_start_matches('/').to_ascii_lowercase();
        if tag.starts_with("br") || tag.starts_with("p ") || tag == "p" || tag.starts_with("tr") || tag.starts_with("li") {
            text.push('\n');
        }
        rest = &rest[end + 1..];
    }
    if !rest.starts_with('<') {
        text.push_str(rest);
    }
    entities(&text)
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Earliest of the markers in `low` from `from`, or its end.
fn next_of(low: &str, from: usize, marks: &[&str]) -> usize {
    marks.iter().filter_map(|m| low[from..].find(m)).min().map_or(low.len(), |i| from + i)
}

/// The links on a textfiles.com index page. A row with a cell that's a
/// number is a file of that size; the other cells with text in them are
/// its description. A link in a row without a size is a directory
/// (`<B>` links in the table of sub-directories; the plain links of the
/// e-text page). The heading is the `<H1>`.
pub fn parse_index(html: &str) -> Page {
    let low = html.to_ascii_lowercase();
    if low.contains("<h1>index of ") {
        return parse_listing(html, &low);
    }
    let mut page = Page::default();
    if let Some(h) = low.find("<h1")
        && let Some(close) = low[h..].find("</h1>") {
            let open = low[h..h + close].find('>').map_or(h + close, |i| h + i + 1);
            page.title = text_lines(&html[open..h + close]).join(" ");
        }
    let mut seen = HashSet::new();
    let mut pos = 0;
    while let Some(i) = low[pos..].find("<a ") {
        let a = pos + i;
        let Some(tag_end) = low[a..].find('>') else { break };
        let tag = &html[a..a + tag_end];
        pos = a + tag_end + 1;
        let Some(href) = attr(tag, "href") else { continue };
        let Some(close) = low[pos..].find("</a>") else { break };
        let label = text_lines(&html[pos..pos + close]).join(" ");
        let after = pos + close + 4;
        if !relative(&href) {
            continue;
        }
        let name = percent_decode(&href);
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        // The rest of the row: up to the next row, table or block, or
        // the next link.
        let end = next_of(&low, after, &["<tr", "</table", "</blockquote", "<a ", "<hr"]);
        let row = &html[after..end];
        let row_low = &low[after..end];
        let mut parts = Vec::new();
        let mut from = 0;
        loop {
            let to = row_low[from..].find("<td").map_or(row.len(), |i| from + i);
            parts.push(text_lines(&row[from..to]));
            if to >= row.len() {
                break;
            }
            from = row_low[to..].find('>').map_or(row.len(), |i| to + i + 1);
        }
        let mut size = None;
        let mut desc = Vec::new();
        for p in parts {
            let joined = p.join(" ");
            let digits = joined.replace(',', "");
            if size.is_none() && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                size = digits.parse().ok();
            } else {
                desc.extend(p);
            }
        }
        page.entries.push(Entry { name, label, size, desc });
        pos = end.max(pos);
    }
    page
}

/// A directory the site has no page for comes as the web server's own
/// listing: `<a href="name">name</a>  date  time  size` a line, `-` for
/// a sub-directory (whose link ends in `/`). No heading, no
/// descriptions.
fn parse_listing(html: &str, low: &str) -> Page {
    let mut page = Page::default();
    let mut seen = HashSet::new();
    let mut pos = 0;
    while let Some(i) = low[pos..].find("<a ") {
        let a = pos + i;
        let Some(tag_end) = low[a..].find('>') else { break };
        pos = a + tag_end + 1;
        let Some(href) = attr(&html[a..a + tag_end], "href") else { continue };
        let Some(close) = low[pos..].find("</a>") else { break };
        let after = pos + close + 4;
        if !relative(&href) {
            continue;
        }
        let is_dir = href.ends_with('/');
        let name = percent_decode(href.trim_end_matches('/'));
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        let end = next_of(low, after, &["<", "\n"]);
        let size = if is_dir { None } else { html[after..end].split_whitespace().last().and_then(|t| t.replace(',', "").parse().ok()) };
        page.entries.push(Entry { name: name.clone(), label: name, size, desc: Vec::new() });
        pos = end;
    }
    page
}

// ---- fetching -----------------------------------------------------------

pub struct Fetcher {
    client: reqwest::Client,
    mirror: String,
    archives: String,
    /// Where the files go (each section a directory under it).
    dir: PathBuf,
    refresh: bool,
}

/// A 404: what was asked for isn't there.
struct NotFound;

impl Fetcher {
    pub fn new(dir: &Path, mirror: &str, archives: &str, refresh: bool) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().user_agent(USER_AGENT).connect_timeout(Duration::from_secs(30)).build()?;
        Ok(Self { client, mirror: mirror.trim_end_matches('/').into(), archives: archives.trim_end_matches('/').into(), dir: dir.into(), refresh })
    }

    fn cache(&self) -> PathBuf {
        self.dir.join(CACHE)
    }

    /// GET, trying again on transport errors and server trouble.
    async fn get(&self, url: &str) -> anyhow::Result<Result<reqwest::Response, NotFound>> {
        let mut wait = 2;
        for attempt in 1..=4 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            match self.client.get(url).timeout(Duration::from_secs(600)).send().await {
                // The mirror sends what it hasn't got to another site's
                // "404" page, with a 200: landing elsewhere is not found.
                Ok(r) if r.status().is_success() && r.url().host_str() != reqwest::Url::parse(url).ok().as_ref().and_then(|u| u.host_str()) => return Ok(Err(NotFound)),
                Ok(r) if r.status().is_success() => return Ok(Ok(r)),
                Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => return Ok(Err(NotFound)),
                Ok(r) if attempt == 4 => bail!("{url}: HTTP {}", r.status()),
                Ok(r) => eprintln!("{url}: HTTP {}, trying again in {wait}s", r.status()),
                Err(e) if attempt == 4 => return Err(e).with_context(|| url.to_string()),
                Err(e) => eprintln!("{url}: {e}, trying again in {wait}s"),
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait *= 3;
        }
        unreachable!()
    }

    /// A directory's index page, from the cache if it's there. None if
    /// the site says there's no such directory (or it isn't HTML).
    async fn page(&self, path: &str) -> anyhow::Result<Option<String>> {
        let cached = self.cache().join("pages").join(if path.is_empty() { "directory.html".to_string() } else { format!("{path}/index.html") });
        if !self.refresh
            && let Ok(b) = std::fs::read(&cached) {
                return Ok(Some(decode(&b)));
            }
        let url = if path.is_empty() { format!("{}/directory.html", self.mirror) } else { format!("{}/{}/", self.mirror, encode_path(path)) };
        let Ok(resp) = self.get(&url).await? else { return Ok(None) };
        let html = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_none_or(|t| t.contains("html"));
        if !html {
            return Ok(None);
        }
        let b = resp.bytes().await.with_context(|| url.clone())?;
        if let Some(p) = cached.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&cached, &b).with_context(|| cached.display().to_string())?;
        Ok(Some(decode(&b)))
    }

    /// Catalogue `path` and everything under it. None if it isn't a
    /// directory after all.
    async fn crawl(&self, path: &str, label: &str, blurb: &str, only: &[String]) -> anyhow::Result<Option<Dir>> {
        let Some(html) = self.page(path).await? else { return Ok(None) };
        let page = parse_index(&html);
        let mut dir = Dir { path: path.into(), label: label.into(), title: page.title, blurb: blurb.into(), ..Default::default() };
        let root = path.is_empty();
        let mut entries = page.entries;
        if root {
            // The directory page links its own pages too; sections have
            // no dot in their names.
            entries.retain(|e| !e.name.contains('.') && e.size.is_none());
            for extra in EXTRA_SECTIONS {
                if !entries.iter().any(|e| e.name.eq_ignore_ascii_case(extra)) {
                    let mut label = extra.to_string();
                    label[..1].make_ascii_uppercase();
                    entries.push(Entry { name: extra.to_string(), label, size: None, desc: Vec::new() });
                }
            }
            if !only.is_empty() {
                entries.retain(|e| only.iter().any(|o| o.eq_ignore_ascii_case(&e.name)));
            }
        }
        for e in entries {
            let sub = join(path, &e.name);
            if e.size.is_none()
                && let Some(d) = Box::pin(self.crawl(&sub, &e.label, &e.desc.join(" "), &[])).await? {
                    dir.dirs.push(d);
                    continue;
                }
            if root {
                continue;
            }
            dir.files.push(File { name: e.name, size: e.size, desc: e.desc });
        }
        println!("{}: {} files, {} directories", if root { "(root)" } else { path }, dir.files.len(), dir.dirs.len());
        Ok(Some(dir))
    }

    /// Download `url` to `to` (through a `.part` file). Ok(false) if it
    /// isn't there.
    async fn download(&self, url: &str, to: &Path) -> anyhow::Result<bool> {
        let Ok(mut resp) = self.get(url).await? else { return Ok(false) };
        let modified = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
            .map(std::time::SystemTime::from);
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        let part = to.with_extension(format!("{}part", to.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()));
        {
            let mut f = std::fs::File::create(&part).with_context(|| part.display().to_string())?;
            while let Some(chunk) = resp.chunk().await.with_context(|| url.to_string())? {
                f.write_all(&chunk)?;
            }
            if let Some(m) = modified {
                let _ = f.set_modified(m);
            }
        }
        std::fs::rename(&part, to)?;
        Ok(true)
    }

    /// A section's archive from archives.textfiles.com: the tarball, or
    /// the ZIP where there's only that. None if there's neither.
    async fn archive(&self, section: &str) -> anyhow::Result<Option<PathBuf>> {
        for ext in ["tar.gz", "zip"] {
            let to = self.cache().join(format!("{section}.{ext}"));
            if to.is_file() {
                return Ok(Some(to));
            }
            let url = format!("{}/{}.{ext}", self.archives, encode_path(section));
            print!("{section}: downloading {url}... ");
            std::io::stdout().flush()?;
            if self.download(&url, &to).await? {
                println!("{}", crate::files::short_size(std::fs::metadata(&to)?.len()));
                return Ok(Some(to));
            }
            println!("not there");
        }
        Ok(None)
    }

    /// Does the file on disk match the catalogue?
    fn have(&self, path: &str, size: Option<u64>) -> bool {
        match std::fs::metadata(self.dir.join(path)) {
            Ok(m) => m.is_file() && size.is_none_or(|s| s == m.len()),
            Err(_) => false,
        }
    }

    /// Put a section's files on disk: from its archive, then one by one
    /// from the mirror for whatever that lacks. Returns (from the
    /// archive, from the mirror, still missing).
    async fn fetch_section(&self, section: &Dir) -> anyhow::Result<(usize, usize, usize)> {
        let wanted: HashMap<String, Option<u64>> = section.all_files().into_iter().map(|(p, f)| (p, f.size)).collect();
        let missing = |me: &Self| wanted.iter().filter(|(p, s)| !me.have(p, **s)).count();
        let before = missing(self);
        let mut unpacked = 0;
        if before > 0
            && let Some(arc) = self.archive(section.name()).await? {
                unpacked = unpack(&arc, section.name(), &wanted, &self.dir, |p, s| !self.have(p, s))?;
                println!("{}: {unpacked} file(s) from {}", section.name(), arc.file_name().unwrap_or_default().to_string_lossy());
            }
        let mut fetched = 0;
        let mut still: Vec<&String> = wanted.iter().filter(|(p, s)| !self.have(p, **s)).map(|(p, _)| p).collect();
        still.sort();
        for p in still {
            let url = format!("{}/{}", self.mirror, encode_path(p));
            match self.download(&url, &self.dir.join(p)).await {
                Ok(true) => fetched += 1,
                Ok(false) => println!("{p}: not on the mirror"),
                Err(e) => println!("{p}: {e:#}"),
            }
        }
        let after = wanted.iter().filter(|(p, _)| !self.dir.join(p).is_file()).count();
        println!("{}: {fetched} file(s) from the mirror, {after} missing", section.name());
        Ok((unpacked, fetched, after))
    }

    /// The whole job: catalogue, then files, section by section.
    pub async fn run(&self, only: &[String]) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.cache())?;
        let Some(root) = self.crawl("", "", "", only).await? else { bail!("{}/directory.html: not found", self.mirror) };
        let (files, dirs) = (root.all_files().len(), root.walk().len() - 1);
        println!("catalogued {files} files in {dirs} directories");
        let cat = self.cache().join("catalog.json");
        std::fs::write(&cat, serde_json::to_vec_pretty(&root)?).with_context(|| cat.display().to_string())?;
        let mut totals = (0, 0, 0);
        for sec in &root.dirs {
            let (a, b, c) = self.fetch_section(sec).await?;
            totals = (totals.0 + a, totals.1 + b, totals.2 + c);
        }
        println!("done: {} file(s) unpacked, {} fetched, {} missing; catalogue in {}", totals.0, totals.1, totals.2, cat.display());
        Ok(())
    }
}

fn encode_path(p: &str) -> String {
    p.split('/').map(percent_encode).collect::<Vec<_>>().join("/")
}

/// A path in an archive as a site path under `section`.
fn site_path(entry: &Path, section: &str) -> Option<String> {
    let parts: Vec<String> = entry.components().filter_map(|c| match c {
        std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
        _ => None,
    }).collect();
    if parts.is_empty() {
        return None;
    }
    let p = parts.join("/");
    Some(if parts[0].eq_ignore_ascii_case(section) { p } else { format!("{section}/{p}") })
}

fn write_file(to: &Path, data: &[u8], modified: Option<std::time::SystemTime>) -> anyhow::Result<()> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut f = std::fs::File::create(to).with_context(|| to.display().to_string())?;
    f.write_all(data)?;
    if let Some(m) = modified {
        let _ = f.set_modified(m);
    }
    Ok(())
}

/// Unpack the catalogued files (`wanted`, site path to size) that `need`
/// says are missing from `dest`. Dotfiles, the index pages and whatever
/// else the archive holds stay packed.
fn unpack(arc: &Path, section: &str, wanted: &HashMap<String, Option<u64>>, dest: &Path, need: impl Fn(&str, Option<u64>) -> bool) -> anyhow::Result<usize> {
    let file = std::fs::File::open(arc).with_context(|| arc.display().to_string())?;
    let mut n = 0;
    if arc.extension().is_some_and(|e| e == "zip") {
        let mut z = zip::ZipArchive::new(file)?;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if !e.is_file() {
                continue;
            }
            let Some(p) = e.enclosed_name().and_then(|p| site_path(&p, section)) else { continue };
            let Some(&size) = wanted.get(&p) else { continue };
            if !need(&p, size) {
                continue;
            }
            let mut data = Vec::new();
            e.read_to_end(&mut data)?;
            let modified = e.last_modified().and_then(|d| {
                let date = NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32)?;
                let secs = date.and_hms_opt(d.hour() as u32, d.minute() as u32, d.second() as u32)?.and_utc().timestamp();
                Some(std::time::UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64))
            });
            write_file(&dest.join(&p), &data, modified)?;
            n += 1;
        }
        return Ok(n);
    }
    let reader: Box<dyn Read> = if arc.extension().is_some_and(|e| e == "tar") { Box::new(file) } else { Box::new(flate2::read::MultiGzDecoder::new(file)) };
    let mut tar = tar::Archive::new(reader);
    for e in tar.entries()? {
        let mut e = e?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let Some(p) = e.path().ok().and_then(|p| site_path(&p, section)) else { continue };
        let Some(&size) = wanted.get(&p) else { continue };
        if !need(&p, size) {
            continue;
        }
        let mut data = Vec::new();
        e.read_to_end(&mut data)?;
        let modified = e.header().mtime().ok().map(|t| std::time::UNIX_EPOCH + Duration::from_secs(t));
        write_file(&dest.join(&p), &data, modified)?;
        n += 1;
    }
    Ok(n)
}

// ---- importing ----------------------------------------------------------

/// The areas and records for a fetched archive. The sections are
/// top-level areas (or sub-areas of `under`, an existing area's key;
/// `put` folds particular sections into areas of the board's own: the
/// section's files are copied into that area and its directories
/// become sub-areas of it, say the artwork into the board's art area
/// so the gallery shows it); `acs`,
/// `download_acs` and `sponsor` default to the board's first area's
/// (uploads are the SysOp's alone: the archive is read, not added to).
pub fn import(config_path: &Path, dir: Option<&Path>, acs: Option<&str>, download_acs: Option<&str>, sponsor: Option<&str>, under: Option<&str>, put: &[(String, String)]) -> anyhow::Result<()> {
    let config = Config::load(config_path)?;
    let dir = dir.map(Path::to_path_buf).unwrap_or_else(|| config.resolve(Path::new("files/textfiles")));
    let cat = dir.join(CACHE).join("catalog.json");
    let root: Dir = serde_json::from_slice(&std::fs::read(&cat).with_context(|| format!("{}: run `iniquity textfiles fetch` first", cat.display()))?)?;
    let fareas_path = config.resolve(&config.paths.fileareas);
    let mut fareas = FileAreas::load(&fareas_path)?;
    let db = config.resolve(&config.paths.data).join("iniquity.db");
    let files = FileBase::open(&db).with_context(|| format!("file database {}", db.display()))?;
    let kb_pt = config.files.kb_per_point.max(1) as u64;
    let tpl = fareas.areas.first().cloned();
    let acs = acs.map(str::to_string).or_else(|| tpl.as_ref().map(|a| a.acs.clone())).unwrap_or_else(|| "s25".into());
    let download_acs = download_acs.map(str::to_string).or_else(|| tpl.as_ref().map(|a| a.download_acs.clone())).unwrap_or_else(|| "s50".into());
    let sponsor = sponsor.map(str::to_string).or_else(|| tpl.as_ref().map(|a| a.sponsor.clone())).unwrap_or_else(|| "SysOp".into());
    let upload_acs = config.acs.sysop.clone();
    // Area paths are relative to the config file where they can be.
    let root_dir = std::path::absolute(config_path)?.parent().map(Path::to_path_buf).unwrap_or_default();
    let abs = std::path::absolute(&dir)?;
    let rel = match abs.strip_prefix(&root_dir) {
        Ok(r) => r.to_path_buf(),
        Err(_) => abs.clone(),
    };

    let under = under.unwrap_or("").to_string();
    for key in std::iter::once(&under).chain(put.iter().map(|(_, k)| k)).filter(|k| !k.is_empty()) {
        if !fareas.areas.iter().any(|a| a.file.eq_ignore_ascii_case(key)) {
            bail!("no area with the storage key {key} to put sections under");
        }
    }
    for (section, _) in put {
        if !root.dirs.iter().any(|d| d.path.eq_ignore_ascii_case(section)) {
            bail!("no section {section} in the catalogue");
        }
    }
    let (mut new_areas, mut new_files, mut missing, mut known) = (0, 0, 0, 0);
    let mut parent_key = HashMap::new();
    let (mut taken, mut folded) = (0, 0);
    for d in root.walk().into_iter().filter(|d| !d.path.is_empty()) {
        let key = d.key();
        // A section folded into an area of the board's own: its files go
        // into that area, its directories under it.
        if let Some((_, into)) = put.iter().find(|(s, _)| s.eq_ignore_ascii_case(&d.path)) {
            let area = fareas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(into)).cloned().with_context(|| format!("no area {into}"))?;
            parent_key.insert(d.path.clone(), area.file.clone());
            // Left by an earlier import that gave the section an area of
            // its own.
            if !fareas.areas.iter().any(|a| a.file.eq_ignore_ascii_case(&key)) {
                let gone = files.delete_area(&key)?;
                if gone > 0 {
                    println!("{}: {gone} record(s) of the former {key} area dropped", d.path);
                }
            }
            let dest = config.resolve(&area.path);
            std::fs::create_dir_all(&dest).with_context(|| dest.display().to_string())?;
            let recs: HashMap<String, String> = files.list(&area.file)?.into_iter().map(|f| (f.name.to_lowercase(), f.uploader)).collect();
            let mut adding = Vec::new();
            for f in &d.files {
                let lower = f.name.to_lowercase();
                if recs.get(&lower).is_some_and(|u| u.eq_ignore_ascii_case("textfiles.com")) {
                    known += 1;
                    continue;
                }
                let on_disk = dir.join(&d.path).join(&f.name);
                let Some(meta) = std::fs::metadata(&on_disk).ok().filter(|m| m.is_file()) else {
                    missing += 1;
                    continue;
                };
                let to = dest.join(&f.name);
                if recs.contains_key(&lower) || to.exists() {
                    // The board has a file of that name already: left out.
                    taken += 1;
                    continue;
                }
                std::fs::copy(&on_disk, &to).with_context(|| to.display().to_string())?;
                if let Ok(m) = meta.modified() {
                    let _ = std::fs::File::options().write(true).open(&to).and_then(|f| f.set_modified(m));
                }
                adding.push(record(&area.file, &f.name, &meta, kb_pt, &f.desc));
            }
            files.add_all(&mut adding)?;
            if !adding.is_empty() && !area.sort.is_none() {
                files.sort_area(&area.file, area.sort, area.sort_descending)?;
            }
            folded += adding.len();
            new_files += adding.len();
            continue;
        }
        let path = rel.join(&d.path);
        let parent = match d.path.rsplit_once('/') {
            Some((above, _)) => parent_key.get(above).cloned().unwrap_or_else(|| under.clone()),
            None => under.clone(),
        };
        parent_key.insert(d.path.clone(), key.clone());
        match fareas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&key)) {
            Some(a) if a.path != path => bail!("area {key} exists already, at {} (this one is {})", a.path.display(), path.display()),
            Some(_) => {}
            None => {
                let at = parent_pos(&fareas, &parent);
                fareas.areas.insert(
                    at,
                    FileArea {
                        name: d.area_name(),
                        file: key.clone(),
                        path,
                        sponsor: sponsor.clone(),
                        acs: acs.clone(),
                        upload_acs: upload_acs.clone(),
                        download_acs: download_acs.clone(),
                        password: String::new(),
                        sort: SortBy::Name,
                        sort_descending: false,
                        parent,
                    },
                );
                new_areas += 1;
            }
        }
        let have: HashSet<String> = files.list(&key)?.into_iter().map(|f| f.name.to_lowercase()).collect();
        let mut recs = Vec::new();
        for f in &d.files {
            if have.contains(&f.name.to_lowercase()) {
                known += 1;
                continue;
            }
            let on_disk = dir.join(&d.path).join(&f.name);
            let Some(meta) = std::fs::metadata(&on_disk).ok().filter(|m| m.is_file()) else {
                missing += 1;
                continue;
            };
            recs.push(record(&key, &f.name, &meta, kb_pt, &f.desc));
        }
        files.add_all(&mut recs)?;
        new_files += recs.len();
    }
    if !put.is_empty() {
        println!("{folded} file(s) folded into the board's own areas, {taken} left out (a file of that name there already)");
    }
    crate::write_atomic(&fareas_path, &fareas.to_toml())?;
    println!(
        "{new_areas} area(s) added ({} in all), {new_files} file(s) added, {known} already there, {missing} not on disk; areas in {}",
        fareas.areas.len(),
        fareas_path.display()
    );
    Ok(())
}

/// A record for a file on disk: dated by the file, the site's description.
fn record(area: &str, name: &str, meta: &std::fs::Metadata, kb_pt: u64, desc: &[String]) -> FileRec {
    let date = meta.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).date_naive());
    FileRec {
        area: area.to_string(),
        num: 0,
        name: name.to_string(),
        size: meta.len(),
        date,
        uploaded: date,
        uploader: "textfiles.com".into(),
        downloads: 0,
        points: (meta.len() / (kb_pt * 1024)) as u32,
        valid: true,
        description: description(desc),
    }
}

/// Where a new area goes: after the last area under its parent (or the
/// parent itself), so the file reads in tree order; the end for a
/// top-level area.
fn parent_pos(fareas: &FileAreas, parent: &str) -> usize {
    if parent.is_empty() {
        return fareas.areas.len();
    }
    let Some(p) = fareas.areas.iter().position(|a| a.file.eq_ignore_ascii_case(parent)) else { return fareas.areas.len() };
    let under = fareas.subtree(p as u32 + 1);
    under.iter().map(|&n| n as usize).max().unwrap_or(p + 1).max(p + 1)
}

/// A catalogue description as record lines: wrapped to the lister's
/// column, and not too many of them.
pub fn description(desc: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for l in desc {
        out.extend(crate::multinode::wrap(l, DESC_WIDTH));
        if out.len() >= MAX_DESC_LINES {
            out.truncate(MAX_DESC_LINES);
            break;
        }
    }
    out
}

// ---- the command --------------------------------------------------------

pub async fn from_args(args: &[String]) -> anyhow::Result<()> {
    let usage = "usage: iniquity textfiles fetch [DIR] [--mirror URL] [--archives URL] [--sections a,b] [--refresh]\n       iniquity textfiles import [CONFIG] [DIR] [--acs ACS] [--download-acs ACS] [--sponsor WHO] [--under KEY] [--put SECTION=KEY,...]";
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut positional = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if let Some(f) = a.strip_prefix("--") {
            let v = if f == "refresh" { "1".to_string() } else { it.next().cloned().with_context(|| format!("--{f} needs a value\n{usage}"))? };
            flags.insert(f.to_string(), v);
        } else {
            positional.push(a.clone());
        }
    }
    match args.first().map(String::as_str) {
        Some("fetch") => {
            let dir = positional.first().map_or_else(|| PathBuf::from("files/textfiles"), PathBuf::from);
            let only: Vec<String> = flags.get("sections").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
            let f = Fetcher::new(&dir, flags.get("mirror").map_or(MIRROR, String::as_str), flags.get("archives").map_or(ARCHIVES, String::as_str), flags.contains_key("refresh"))?;
            f.run(&only).await
        }
        Some("import") => {
            let (config, dir) = match positional.as_slice() {
                [] => (PathBuf::from("iniquity.toml"), None),
                [one] if one.ends_with(".toml") => (PathBuf::from(one), None),
                [one] => (PathBuf::from("iniquity.toml"), Some(PathBuf::from(one))),
                [c, d, ..] => (PathBuf::from(c), Some(PathBuf::from(d))),
            };
            let put: Vec<(String, String)> = flags
                .get("put")
                .map(|v| v.split(',').filter(|p| !p.trim().is_empty()).map(|p| p.split_once('=').map(|(s, k)| (s.trim().to_string(), k.trim().to_string())).with_context(|| format!("--put wants SECTION=KEY, not {p}"))).collect::<Result<_, _>>())
                .transpose()?
                .unwrap_or_default();
            import(&config, dir.as_deref(), flags.get("acs").map(String::as_str), flags.get("download-acs").map(String::as_str), flags.get("sponsor").map(String::as_str), flags.get("under").map(String::as_str), &put)
        }
        _ => bail!("{usage}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANARCHY: &str = r#"<HTML><TITLE>T E X T F I L E S</TITLE>
<H1>Anarchy and Explosives and General Mayhem</H1>
<P>I wrote an <A HREF="http://www.textfiles.com/thoughts/anarchy.html">essay</A> about it.
<TABLE WIDTH=100%>
<TR VALIGN=TOP><TD VALIGN=TOP><B><A HREF="CARDING">CARDING</A></B><TAB TO=T><TD WIDTH=20></TD><TD><B>On Committing Credit Card Fraud</B></TD></TR>
<TR VALIGN=TOP><TD VALIGN=TOP><B><A HREF="FDR">FDR</A></B><TAB TO=T><TD WIDTH=20></TD><TD><B>The FDR Anarchy File Collection</B></TD></TR>
<TR><TD>&nbsp;</TD></TR>
<TR VALIGN=TOP><TD ALIGN=TOP><A HREF="001.txt">001.txt</A>  <tab to=T><TD> 1151<BR><TD> How to Make a Dry Ice Bomb, by Vortex
<TR VALIGN=TOP><TD ALIGN=TOP><A HREF="ac.ana">ac.ana</A>  <tab to=T><TD> 3725<BR><TD> Anarchist's &amp; Ordering Catalog
<TR VALIGN=TOP><TD ALIGN=TOP><A HREF="914bbs.txt">914bbs.txt</A>  <tab to=T><TD> 3968<BR><TD> <B>914 Area Code BBS List (January 15, 1984)</B><BR><I>A snapshot of the typical BBSes.</I><BR>&nbsp
<TR VALIGN=TOP><TD ALIGN=TOP><A HREF="odd%20name.txt">odd name.txt</A>  <tab to=T><TD> 12,345<BR><TD>
</TABLE>
<P>There are 4 files for a total of 2,975,303 bytes.<P>
either <A HREF="http://archives.textfiles.com/anarchy.tar.gz"><B>anarchy.tar.gz</B></A> (13073725 bytes)
"#;

    #[test]
    fn index_page() {
        let p = parse_index(ANARCHY);
        assert_eq!(p.title, "Anarchy and Explosives and General Mayhem");
        let names: Vec<(&str, Option<u64>)> = p.entries.iter().map(|e| (e.name.as_str(), e.size)).collect();
        assert_eq!(names, [("CARDING", None), ("FDR", None), ("001.txt", Some(1151)), ("ac.ana", Some(3725)), ("914bbs.txt", Some(3968)), ("odd name.txt", Some(12345))]);
        assert_eq!(p.entries[0].desc, ["On Committing Credit Card Fraud"]);
        assert_eq!(p.entries[2].desc, ["How to Make a Dry Ice Bomb, by Vortex"]);
        assert_eq!(p.entries[3].desc, ["Anarchist's & Ordering Catalog"]);
        assert_eq!(p.entries[4].desc, ["914 Area Code BBS List (January 15, 1984)", "A snapshot of the typical BBSes."]);
        assert!(p.entries[5].desc.is_empty());
    }

    #[test]
    fn etext_style_links() {
        let html = r#"<BLOCKQUOTE>
<FONT SIZE=+1><B><A STYLE="text-decoration:none" HREF="AUTHORS">AUTHORS</A></B></FONT><BR>
Check here first. Collections of transcribed books. <I>Burroughs, Doyle, Poe...</I>
</BLOCKQUOTE>
<BLOCKQUOTE>
<FONT SIZE=+1><B><A HREF="FICTION">FICTION</A></B></FONT><BR>
Stories and novels.
</BLOCKQUOTE>"#;
        let p = parse_index(html);
        assert_eq!(p.title, "");
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].name, "AUTHORS");
        assert_eq!(p.entries[0].desc, ["Check here first. Collections of transcribed books. Burroughs, Doyle, Poe..."]);
        assert_eq!(p.entries[1].desc, ["Stories and novels."]);
    }

    #[test]
    fn server_listing() {
        let html = r#"<html><head><title>Index of /messages/TBFILES/</title></head><body><h1>Index of /messages/TBFILES/</h1><hr><pre><a href="../">../</a>
<a href="SUB/">SUB/</a>                                               11-Apr-2016 12:41                   -
<a href="1317tags.txt">1317tags.txt</a>                                       11-Apr-2016 12:41               46371
<a href="16cds.bbs">16cds.bbs</a>                                          11-Apr-2016 12:41                1165
</pre><hr></body></html>"#;
        let p = parse_index(html);
        assert_eq!(p.title, "");
        let got: Vec<(&str, Option<u64>)> = p.entries.iter().map(|e| (e.name.as_str(), e.size)).collect();
        assert_eq!(got, [("SUB", None), ("1317tags.txt", Some(46371)), ("16cds.bbs", Some(1165))]);
        assert!(p.entries.iter().all(|e| e.desc.is_empty()));
    }

    #[test]
    fn directory_page_sections() {
        let html = r#"<A HREF="/">home</A> <A HREF="disclaimer.html">Disclaimer</A>
<TR><TD>
<B><A HREF="anarchy">Anarchy</A></B><BR>
<I>Files that <B>YOU SHOULD NOT FOLLOW</B></I><BR>
<TD>
<B><A HREF="sex">Sex and Sexuality</A></B><BR>
<I>Files about trying to Make More of You</I><BR>
</TR>"#;
        let p = parse_index(html);
        let e: Vec<&Entry> = p.entries.iter().filter(|e| !e.name.contains('.')).collect();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].name.as_str(), e[0].label.as_str()), ("anarchy", "Anarchy"));
        assert_eq!(e[0].desc, ["Files that YOU SHOULD NOT FOLLOW"]);
        assert_eq!(e[1].label, "Sex and Sexuality");
    }

    #[test]
    fn area_names_and_keys() {
        let sec = Dir { path: "anarchy".into(), label: "Anarchy".into(), title: "Anarchy and Explosives and General Mayhem".into(), ..Default::default() };
        assert_eq!(sec.area_name(), "Anarchy");
        assert_eq!(sec.key(), "TF_ANARCHY");
        let top = Dir { path: "100".into(), label: "100".into(), title: "Jason Scott's Top 100 Textfiles".into(), ..Default::default() };
        assert_eq!(top.area_name(), "Jason Scott's Top 100");
        let sub = Dir { path: "anarchy/CARDING".into(), label: "CARDING".into(), title: "Anarchy: Credit Card Fraud".into(), blurb: "On Committing Credit Card Fraud".into(), ..Default::default() };
        assert_eq!(sub.area_name(), "Credit Card Fraud");
        assert_eq!(sub.key(), "TF_ANARCHY_CARDING");
        let blurbed = Dir { path: "history/EVILEXIDY".into(), label: "EVILEXIDY".into(), blurb: "Mirror of Apple 2 Memory Page by Evil Exidy (August, 2005)".into(), ..Default::default() };
        assert_eq!(blurbed.area_name(), "Mirror of Apple 2 Memory Page");
        let bare = Dir { path: "etext/AUTHORS/POE".into(), label: "POE".into(), ..Default::default() };
        assert_eq!(bare.area_name(), "POE");
        assert_eq!(bare.key(), "TF_ETEXT_AUTHORS_POE");
        assert_eq!(Dir::default().key(), ROOT_KEY);
        assert_eq!(Dir::default().area_name(), "textfiles.com");
    }

    #[test]
    fn text_and_entities() {
        assert_eq!(text_lines("a &lt;b&gt; &amp; &#65;&#x42; &quot;c&quot; &nbsp;d &unknown; e"), ["a <b> & AB \"c\" d &unknown; e"]);
        assert_eq!(text_lines("one<BR>two<p>three  <i>four</i>"), ["one", "two", "three four"]);
        assert_eq!(percent_decode("odd%20name%2etxt"), "odd name.txt");
        assert_eq!(percent_encode("odd name#1.txt"), "odd%20name%231.txt");
        assert_eq!(attr(r#"<A STYLE="x" HREF="AUTHORS">"#, "href").as_deref(), Some("AUTHORS"));
        assert_eq!(attr("<a href=plain.txt>", "href").as_deref(), Some("plain.txt"));
        assert_eq!(attr("<a xhref=\"no\">", "href"), None);
    }

    #[test]
    fn archive_paths() {
        assert_eq!(site_path(Path::new("anarchy/CARDING/ac1.txt"), "anarchy").as_deref(), Some("anarchy/CARDING/ac1.txt"));
        assert_eq!(site_path(Path::new("./anarchy/001.txt"), "anarchy").as_deref(), Some("anarchy/001.txt"));
        assert_eq!(site_path(Path::new("AUTHORS/POE/raven.txt"), "etext").as_deref(), Some("etext/AUTHORS/POE/raven.txt"));
        assert_eq!(site_path(Path::new("/"), "etext"), None);
    }

    #[test]
    fn descriptions_wrap() {
        let long = "A".repeat(30) + " " + &"B".repeat(30);
        assert_eq!(description(&[long, "short".into()]), ["A".repeat(30), "B".repeat(30), "short".into()]);
        let many: Vec<String> = (0..30).map(|i| i.to_string()).collect();
        assert_eq!(description(&many).len(), MAX_DESC_LINES);
    }

    #[test]
    fn unpacks_only_what_is_catalogued() {
        let tmp = std::env::temp_dir().join(format!("iqtf{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let arc = tmp.join("anarchy.tar.gz");
        {
            let f = std::fs::File::create(&arc).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut t = tar::Builder::new(gz);
            for (name, body) in [("anarchy/001.txt", "dry ice"), ("anarchy/.descs", "hidden"), ("anarchy/CARDING/ac1.txt", "cards"), ("anarchy/gone.txt", "removed")] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_mtime(1_000_000_000);
                h.set_cksum();
                t.append_data(&mut h, name, body.as_bytes()).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap();
        }
        let wanted: HashMap<String, Option<u64>> = [("anarchy/001.txt".to_string(), Some(7)), ("anarchy/CARDING/ac1.txt".to_string(), None), ("anarchy/new.txt".to_string(), Some(3))].into_iter().collect();
        let dest = tmp.join("files");
        let n = unpack(&arc, "anarchy", &wanted, &dest, |_, _| true).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read_to_string(dest.join("anarchy/001.txt")).unwrap(), "dry ice");
        assert_eq!(std::fs::read_to_string(dest.join("anarchy/CARDING/ac1.txt")).unwrap(), "cards");
        assert!(!dest.join("anarchy/gone.txt").exists());
        assert!(!dest.join("anarchy/.descs").exists());
        let m = std::fs::metadata(dest.join("anarchy/001.txt")).unwrap().modified().unwrap();
        assert_eq!(m, std::time::UNIX_EPOCH + Duration::from_secs(1_000_000_000));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn imports_areas_and_records() {
        let tmp = std::env::temp_dir().join(format!("iqtfi{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("files/textfiles/anarchy/CARDING")).unwrap();
        std::fs::create_dir_all(tmp.join("files/textfiles/.textfiles")).unwrap();
        std::fs::create_dir_all(tmp.join("data")).unwrap();
        std::fs::write(tmp.join("iniquity.toml"), "").unwrap();
        std::fs::write(tmp.join("files/textfiles/anarchy/001.txt"), "dry ice bomb").unwrap();
        std::fs::write(tmp.join("files/textfiles/anarchy/CARDING/ac1.txt"), "cards").unwrap();
        let root = Dir {
            dirs: vec![Dir {
                path: "anarchy".into(),
                label: "Anarchy".into(),
                files: vec![File { name: "001.txt".into(), size: Some(12), desc: vec!["Dry ice".into()] }, File { name: "lost.txt".into(), size: None, desc: vec![] }],
                dirs: vec![Dir { path: "anarchy/CARDING".into(), label: "CARDING".into(), title: "Anarchy: Credit Card Fraud".into(), files: vec![File { name: "ac1.txt".into(), size: Some(5), desc: vec![] }], ..Default::default() }],
                ..Default::default()
            }],
            ..Default::default()
        };
        std::fs::write(tmp.join("files/textfiles/.textfiles/catalog.json"), serde_json::to_vec(&root).unwrap()).unwrap();
        assert!(import(&tmp.join("iniquity.toml"), None, None, None, None, Some("NOSUCH"), &[]).is_err());
        assert!(import(&tmp.join("iniquity.toml"), None, None, None, None, None, &[("anarchy".into(), "NOSUCH".into())]).is_err());
        assert!(import(&tmp.join("iniquity.toml"), None, None, None, None, None, &[("nosuch".into(), "NEWAREA".into())]).is_err());
        import(&tmp.join("iniquity.toml"), None, Some("s10"), None, None, None, &[]).unwrap();
        let fa = FileAreas::load(&tmp.join("fileareas.toml")).unwrap();
        let keys: Vec<(&str, &str, &str)> = fa.areas.iter().map(|a| (a.file.as_str(), a.name.as_str(), a.parent.as_str())).collect();
        // The sections are areas of the board's own, in the site's order.
        assert_eq!(keys[1..], [("TF_ANARCHY", "Anarchy", ""), ("TF_ANARCHY_CARDING", "Credit Card Fraud", "TF_ANARCHY")]);
        assert_eq!(fa.areas[1].path, Path::new("files/textfiles/anarchy"));
        assert_eq!(fa.areas[1].acs, "s10");
        assert_eq!(fa.areas[1].upload_acs, "s255");
        let fb = FileBase::open(&tmp.join("data/iniquity.db")).unwrap();
        let got = fb.list("TF_ANARCHY").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].name.as_str(), got[0].size, got[0].uploader.as_str()), ("001.txt", 12, "textfiles.com"));
        assert_eq!(got[0].description, ["Dry ice"]);
        assert_eq!(fb.count("TF_ANARCHY_CARDING").unwrap(), 1);
        // Again: nothing doubled.
        import(&tmp.join("iniquity.toml"), None, None, None, None, None, &[]).unwrap();
        assert_eq!(FileAreas::load(&tmp.join("fileareas.toml")).unwrap().areas.len(), 3);
        // A section folded into an area of the board's own: its files
        // join that area, its directories go under it, and the records
        // of the area it had before go.
        std::fs::remove_file(tmp.join("fileareas.toml")).unwrap();
        std::fs::create_dir_all(tmp.join("files/newarea")).unwrap();
        std::fs::write(tmp.join("files/newarea/lost.txt"), "the board's own").unwrap();
        let mut own = FileRec { area: "NEWAREA".into(), num: 0, name: "lost.txt".into(), size: 15, date: None, uploader: "phrac".into(), uploaded: None, downloads: 0, points: 0, valid: true, description: vec![] };
        fb.add(&mut own).unwrap();
        import(&tmp.join("iniquity.toml"), None, None, None, None, None, &[("Anarchy".into(), "newarea".into())]).unwrap();
        let fa = FileAreas::load(&tmp.join("fileareas.toml")).unwrap();
        let parents: Vec<(&str, &str)> = fa.areas.iter().map(|a| (a.file.as_str(), a.parent.as_str())).collect();
        assert_eq!(parents, [("NEWAREA", ""), ("TF_ANARCHY_CARDING", "NEWAREA")]);
        assert_eq!(fb.count("TF_ANARCHY").unwrap(), 0);
        let got: Vec<(String, String)> = fb.list("NEWAREA").unwrap().into_iter().map(|f| (f.name, f.uploader)).collect();
        assert_eq!(got, [("lost.txt".to_string(), "phrac".to_string()), ("001.txt".to_string(), "textfiles.com".to_string())]);
        assert_eq!(std::fs::read_to_string(tmp.join("files/newarea/001.txt")).unwrap(), "dry ice bomb");
        // Again: nothing doubled, the board's own file untouched.
        import(&tmp.join("iniquity.toml"), None, None, None, None, None, &[("anarchy".into(), "NEWAREA".into())]).unwrap();
        assert_eq!(fb.count("NEWAREA").unwrap(), 2);
        assert_eq!(std::fs::read_to_string(tmp.join("files/newarea/lost.txt")).unwrap(), "the board's own");
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
