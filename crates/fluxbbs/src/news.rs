//! New: the news (`NW`, the `news` component; `[news]`, off to start). The
//! board makes its own paper: an event (`news`) searches the web for each
//! section's news (SearXNG, `[ai] search_url`), the board's AI groups what
//! it found into stories, a second search finds more outlets for each, and
//! the AI writes each story from all of them: a headline, a summary where
//! every sentence names its sources (`[1][3]`), where the sources differ,
//! and a line on each source. A last pass has the AI check each sentence
//! against the sources it names, and drops those they don't support.
//! Stories are kept in the database, so callers read them at once; they
//! read a source's excerpt and follow its link, but the board never shows
//! a whole article. Callers vote on stories and leave one-line comments
//! (`x`), as on files. Stories never reach the message bases, FTN or QWK.
//! The original had nothing like it.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::BbsState;
use crate::config::NewsSection;
use crate::input::{Key, ReadOpts};
use crate::session::{BarPick, BarRow, BarSpec, Session, Waves};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS news_stories (
    id       INTEGER PRIMARY KEY,
    section  TEXT NOT NULL,
    rank     INTEGER NOT NULL,
    headline TEXT NOT NULL,
    summary  TEXT NOT NULL,
    differ   TEXT NOT NULL,
    made_at  TEXT NOT NULL,
    dek      TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS news_stories_at ON news_stories(made_at);
CREATE TABLE IF NOT EXISTS news_sources (
    story_id  INTEGER NOT NULL,
    n         INTEGER NOT NULL,
    outlet    TEXT NOT NULL,
    title     TEXT NOT NULL,
    url       TEXT NOT NULL,
    excerpt   TEXT NOT NULL,
    says      TEXT NOT NULL,
    published TEXT NOT NULL,
    PRIMARY KEY (story_id, n)
);
CREATE INDEX IF NOT EXISTS news_sources_url ON news_sources(url);
CREATE TABLE IF NOT EXISTS news_pushes (
    id       INTEGER PRIMARY KEY,
    story_id INTEGER NOT NULL,
    user_id  INTEGER NOT NULL,
    handle   TEXT NOT NULL,
    kind     INTEGER NOT NULL,
    text     TEXT NOT NULL,
    at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS news_pushes_story ON news_pushes(story_id);
CREATE TABLE IF NOT EXISTS news_reads (
    user_id  INTEGER NOT NULL,
    story_id INTEGER NOT NULL,
    at       TEXT NOT NULL,
    PRIMARY KEY (user_id, story_id)
);
";

/// Changes since the tables were first made (`sysdata::migrate`).
const STEPS: &[crate::sysdata::Step] = &[
    // New: a story's one-line overview, under its headline on the front page.
    crate::sysdata::Step::Column("news_stories", "dek", "TEXT NOT NULL DEFAULT ''"),
];

/// How many sources a story may have (and number keys reach).
const MOST_SOURCES: usize = 9;
/// A story's headline, at most.
const HEADLINE: usize = 72;
/// How much of each source's text the AI is given.
const SOURCE_CHARS: usize = 3000;
/// How much of a source's own words callers see.
const EXCERPT: usize = 300;
/// A story's overview line, at most.
const DEK: usize = 66;

#[derive(Debug, Clone, PartialEq)]
pub struct Story {
    pub id: i64,
    pub section: String,
    pub rank: u32,
    pub headline: String,
    pub summary: String,
    pub differ: String,
    pub made_at: NaiveDateTime,
    /// A line on what it's about, beyond the headline ("" on older ones).
    pub dek: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Source {
    pub n: u32,
    pub outlet: String,
    pub title: String,
    pub url: String,
    pub excerpt: String,
    pub says: String,
    pub published: String,
}

/// A vote (`1`, `-1`) or comment (`0`) on a story.
#[derive(Debug, Clone, PartialEq)]
pub struct Push {
    pub user_id: u32,
    pub handle: String,
    pub kind: i8,
    pub text: String,
    pub at: NaiveDateTime,
}

pub struct NewsStore {
    db: Mutex<Connection>,
    /// A refresh is under way (one at a time).
    running: std::sync::atomic::AtomicBool,
}

fn story_row(r: &rusqlite::Row) -> rusqlite::Result<Story> {
    Ok(Story {
        id: r.get("id")?,
        section: r.get("section")?,
        rank: r.get("rank")?,
        headline: r.get("headline")?,
        summary: r.get("summary")?,
        differ: r.get("differ")?,
        made_at: r.get("made_at")?,
        dek: r.get("dek")?,
    })
}

impl NewsStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut db = crate::sysdata::open_db(path)?;
        crate::sysdata::migrate(&mut db, "news", SCHEMA, STEPS)?;
        Ok(Self { db: Mutex::new(db), running: Default::default() })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let mut db = Connection::open_in_memory()?;
        crate::sysdata::migrate(&mut db, "news", SCHEMA, STEPS)?;
        Ok(Self { db: Mutex::new(db), running: Default::default() })
    }

    pub fn add_story(&self, s: &Story, sources: &[Source]) -> anyhow::Result<i64> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO news_stories (section, rank, headline, summary, differ, made_at, dek) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![s.section, s.rank, s.headline, s.summary, s.differ, s.made_at, s.dek],
        )?;
        let id = tx.last_insert_rowid();
        for src in sources {
            tx.execute(
                "INSERT INTO news_sources (story_id, n, outlet, title, url, excerpt, says, published) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, src.n, src.outlet, src.title, src.url, src.excerpt, src.says, src.published],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Stories made from `from` up to (not including) `to`.
    pub fn stories_between(&self, from: NaiveDateTime, to: NaiveDateTime) -> anyhow::Result<Vec<Story>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT * FROM news_stories WHERE made_at >= ?1 AND made_at < ?2 ORDER BY made_at DESC, rank")?;
        let rows = stmt.query_map(params![from, to], story_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// New: the newest edition's stories (made within EDITION_GAP of the
    /// newest, with no gap that long between).
    pub fn newest_edition(&self) -> anyhow::Result<Vec<Story>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT * FROM news_stories ORDER BY made_at DESC LIMIT 200")?;
        let all: Vec<Story> = stmt.query_map([], story_row)?.collect::<Result<_, _>>()?;
        let mut out: Vec<Story> = Vec::new();
        for s in all {
            if out.last().is_some_and(|l| l.made_at - s.made_at >= chrono::Duration::minutes(EDITION_GAP)) {
                break;
            }
            out.push(s);
        }
        Ok(out)
    }

    pub fn story(&self, id: i64) -> anyhow::Result<Option<Story>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT * FROM news_stories WHERE id = ?1", [id], story_row).optional()?)
    }

    pub fn sources(&self, story: i64) -> anyhow::Result<Vec<Source>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT * FROM news_sources WHERE story_id = ?1 ORDER BY n")?;
        let rows = stmt.query_map([story], |r| {
            Ok(Source {
                n: r.get("n")?,
                outlet: r.get("outlet")?,
                title: r.get("title")?,
                url: r.get("url")?,
                excerpt: r.get("excerpt")?,
                says: r.get("says")?,
                published: r.get("published")?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many sources each story has.
    pub fn source_counts(&self) -> anyhow::Result<HashMap<i64, u32>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT story_id, COUNT(*) FROM news_sources GROUP BY story_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Addresses the stories since `when` were made from.
    pub fn urls_since(&self, when: NaiveDateTime) -> anyhow::Result<HashSet<String>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT s.url FROM news_sources s JOIN news_stories t ON t.id = s.story_id WHERE t.made_at >= ?1")?;
        let rows = stmt.query_map([when], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The days there's news for, the latest first.
    pub fn days(&self) -> anyhow::Result<Vec<NaiveDate>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT DISTINCT substr(made_at, 1, 10) FROM news_stories ORDER BY 1 DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|d| d.ok()?.parse().ok()).collect())
    }

    /// Stories whose headline or summary has every word in `words`, the
    /// latest first.
    pub fn search(&self, words: &[String], limit: usize) -> anyhow::Result<Vec<Story>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT * FROM news_stories ORDER BY made_at DESC, rank")?;
        let rows = stmt.query_map([], story_row)?;
        let mut out = Vec::new();
        for s in rows {
            let s = s?;
            let text = format!("{} {}", s.headline, s.summary).to_lowercase();
            if words.iter().all(|w| text.contains(&w.to_lowercase())) {
                out.push(s);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    pub fn pushes(&self, story: i64) -> anyhow::Result<Vec<Push>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT user_id, handle, kind, text, at FROM news_pushes WHERE story_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([story], |r| Ok(Push { user_id: r.get(0)?, handle: r.get(1)?, kind: r.get(2)?, text: r.get(3)?, at: r.get(4)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// A vote or comment. A caller has one vote a story: a new one takes
    /// the place of the old.
    pub fn add_push(&self, story: i64, p: &Push) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        if p.kind != 0 {
            tx.execute("DELETE FROM news_pushes WHERE story_id = ?1 AND user_id = ?2 AND kind != 0", params![story, p.user_id])?;
        }
        tx.execute(
            "INSERT INTO news_pushes (story_id, user_id, handle, kind, text, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![story, p.user_id, p.handle, p.kind, p.text, p.at],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A caller's pushes taken off a story; how many.
    pub fn remove_pushes(&self, story: i64, user: u32) -> anyhow::Result<usize> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute("DELETE FROM news_pushes WHERE story_id = ?1 AND user_id = ?2", params![story, user])?)
    }

    pub fn last_push_at(&self, user: u32) -> anyhow::Result<Option<NaiveDateTime>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT MAX(at) FROM news_pushes WHERE user_id = ?1", [user], |r| r.get(0))?)
    }

    /// Each story's score (votes up less down) and comments.
    pub fn tallies(&self) -> anyhow::Result<HashMap<i64, (i32, u32)>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT story_id, SUM(kind), SUM(kind = 0) FROM news_pushes GROUP BY story_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// New: stories a caller has read.
    pub fn mark_read(&self, user: u32, ids: &[i64], at: NaiveDateTime) -> anyhow::Result<()> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        for id in ids {
            tx.execute("INSERT OR IGNORE INTO news_reads (user_id, story_id, at) VALUES (?1, ?2, ?3)", params![user, id, at])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The stories a caller has read.
    pub fn read_ids(&self, user: u32) -> anyhow::Result<HashSet<i64>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = db.prepare("SELECT story_id FROM news_reads WHERE user_id = ?1")?;
        let rows = stmt.query_map([user], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// When the latest story was made.
    pub fn latest(&self) -> anyhow::Result<Option<NaiveDateTime>> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row("SELECT MAX(made_at) FROM news_stories", [], |r| r.get(0))?)
    }

    /// Stories made before `when` gone, with their sources and pushes.
    pub fn prune(&self, when: NaiveDateTime) -> anyhow::Result<usize> {
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        tx.execute("DELETE FROM news_sources WHERE story_id IN (SELECT id FROM news_stories WHERE made_at < ?1)", [when])?;
        tx.execute("DELETE FROM news_pushes WHERE story_id IN (SELECT id FROM news_stories WHERE made_at < ?1)", [when])?;
        tx.execute("DELETE FROM news_reads WHERE story_id IN (SELECT id FROM news_stories WHERE made_at < ?1)", [when])?;
        let n = tx.execute("DELETE FROM news_stories WHERE made_at < ?1", [when])?;
        tx.commit()?;
        Ok(n)
    }
}

/// Ends a refresh's hold on the store.
struct Running<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

// ---- making the news ------------------------------------------------------

/// Something a search found.
#[derive(Debug, Clone, Default)]
struct Found {
    title: String,
    url: String,
    snippet: String,
    published: String,
}

/// Text the board shows in CP437: typographic quotes and dashes made
/// plain, spaces run together.
fn plain_text(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' => '"',
            '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            '\u{00a0}' | '\t' => ' ',
            c if c.is_control() && c != '\n' => ' ',
            c => c,
        })
        .collect();
    let s = s.replace('\u{2026}', "...");
    // Paragraphs kept; spaces within them run together.
    s.split("\n\n").map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// An outlet's name from its address: the well-known by name, the rest
/// by host.
fn outlet(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url).split(['/', '?', '#', ':']).next().unwrap_or_default().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    const KNOWN: &[(&str, &str)] = &[
        ("apnews.com", "AP News"),
        ("reuters.com", "Reuters"),
        ("bbc.com", "BBC"),
        ("bbc.co.uk", "BBC"),
        ("nytimes.com", "New York Times"),
        ("washingtonpost.com", "Washington Post"),
        ("wsj.com", "Wall Street Journal"),
        ("theguardian.com", "The Guardian"),
        ("npr.org", "NPR"),
        ("cnn.com", "CNN"),
        ("nbcnews.com", "NBC News"),
        ("cbsnews.com", "CBS News"),
        ("abcnews.go.com", "ABC News"),
        ("abcnews.com", "ABC News"),
        ("usnews.com", "U.S. News"),
        ("bloomberg.com", "Bloomberg"),
        ("ft.com", "Financial Times"),
        ("aljazeera.com", "Al Jazeera"),
        ("cbc.ca", "CBC"),
        ("arstechnica.com", "Ars Technica"),
        ("theverge.com", "The Verge"),
        ("wired.com", "Wired"),
        ("techcrunch.com", "TechCrunch"),
        ("theregister.com", "The Register"),
        ("engadget.com", "Engadget"),
        ("nature.com", "Nature"),
        ("science.org", "Science"),
        ("newscientist.com", "New Scientist"),
        ("space.com", "Space.com"),
        ("msn.com", "MSN"),
        ("yahoo.com", "Yahoo News"),
        ("politico.com", "Politico"),
        ("axios.com", "Axios"),
        ("hackaday.com", "Hackaday"),
    ];
    KNOWN.iter().find(|(h, _)| host == *h || host.ends_with(&format!(".{h}"))).map_or(host, |(_, n)| n.to_string())
}

/// News from SearXNG for `query`, from the last `range` (day, week,
/// month), in `language` (blank: any).
async fn search_news(client: &reqwest::Client, searx: &str, query: &str, range: &str, language: &str) -> anyhow::Result<Vec<Found>> {
    let mut url = format!("{}/search?q={}&format=json&categories=news&time_range={}", searx.trim_end_matches('/'), crate::aichat::url_encode(query), crate::aichat::url_encode(range));
    if !language.is_empty() {
        url += &format!("&language={}", crate::aichat::url_encode(language));
    }
    let mut v: Value = client.get(&url).send().await?.error_for_status()?.json().await?;
    // Bing (most of SearXNG's news that can be had by date) fails to parse
    // now and then: once more.
    if v["results"].as_array().is_none_or(Vec::is_empty) && !v["unresponsive_engines"].as_array().is_none_or(Vec::is_empty) {
        tokio::time::sleep(Duration::from_secs(2)).await;
        v = client.get(&url).send().await?.error_for_status()?.json().await?;
    }
    Ok(v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| Found {
                    title: plain_text(r["title"].as_str().unwrap_or("")),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: plain_text(r["content"].as_str().unwrap_or("")),
                    published: r["publishedDate"].as_str().unwrap_or("").to_string(),
                })
                .filter(|f| f.url.starts_with("http") && !f.title.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// Is a page's text only a wall (a bot check, a cookie or paywall
/// notice), not the article?
fn walled(text: &str) -> bool {
    let start: String = text.chars().take(600).collect::<String>().to_lowercase();
    const WALLS: &[&str] = &[
        "are you a robot",
        "not a robot",
        "captcha",
        "unusual activity",
        "enable javascript",
        "javascript is disabled",
        "access denied",
        "subscribe to continue",
        "verify you are human",
        "checking your browser",
        "just a moment",
    ];
    text.split_whitespace().count() < 60 || WALLS.iter().any(|w| start.contains(w))
}

/// The JSON object in an answer (models wrap it in words or fences).
fn json_in(answer: &str) -> Option<Value> {
    let (a, b) = (answer.find('{')?, answer.rfind('}')?);
    serde_json::from_str(answer.get(a..=b)?).ok()
}

/// Up to `n` characters of `s`, cut at a word, with `...` if cut.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out = String::new();
    for w in s.split_whitespace() {
        if out.chars().count() + w.chars().count() + 4 > n {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out + "..."
}

/// `s` without its citations (`[2]`).
fn uncited(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        let Some(j) = rest[i..].find(']') else { break };
        let inside = &rest[i + 1..i + j];
        out.push_str(&rest[..i]);
        if inside.is_empty() || !inside.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(&rest[i..=i + j]);
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ").replace(" .", ".").replace(" ,", ",")
}

/// A story's overview: its own, else its lede's first sentence.
fn dek_of(s: &Story) -> String {
    if !s.dek.is_empty() {
        return s.dek.clone();
    }
    let lede = s.summary.split("\n\n").next().unwrap_or("");
    clip(&uncited(sentences(lede).first().map_or(lede, String::as_str)), DEK)
}

/// A paragraph's sentences, each with the citations that follow it.
fn sentences(p: &str) -> Vec<String> {
    let chars: Vec<char> = p.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        cur.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') {
            // Citations after the stop belong to this sentence.
            let mut j = i + 1;
            loop {
                let mut k = j;
                while k < chars.len() && chars[k] == ' ' {
                    k += 1;
                }
                if k < chars.len() && chars[k] == '[' && let Some(end) = chars[k..].iter().position(|&c| c == ']') {
                    cur.extend(&chars[j..k + end + 1]);
                    j = k + end + 1;
                } else {
                    break;
                }
            }
            // A sentence ends where the next starts with a capital (or
            // the paragraph ends): not at "U.S. officials".
            let mut k = j;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k >= chars.len() || (k > j && (chars[k].is_uppercase() || chars[k] == '"' || chars[k].is_ascii_digit())) {
                out.push(cur.trim().to_string());
                cur.clear();
                i = k;
                continue;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The sources a sentence cites (`[2]`, `[1][3]`, `[1, 3]`).
fn cites(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.split('[').skip(1) {
        let Some(inside) = part.split(']').next() else { continue };
        for n in inside.split([',', ' ']).filter_map(|n| n.trim().parse().ok()) {
            out.push(n);
        }
    }
    out
}

/// Citations of sources that aren't there taken out of a sentence.
fn fix_cites(s: &str, have: usize) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        match rest[i..].find(']') {
            Some(j) => {
                let inside = &rest[i + 1..i + j];
                let ok: Vec<u32> = inside.split([',', ' ']).filter_map(|n| n.trim().parse().ok()).filter(|&n: &u32| n >= 1 && n as usize <= have).collect();
                let numeric = inside.split([',', ' ']).all(|n| n.trim().is_empty() || n.trim().parse::<u32>().is_ok());
                if !numeric {
                    out.push_str(&rest[i..=i + j]);
                } else {
                    for n in ok {
                        out.push_str(&format!("[{n}]"));
                    }
                }
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    // " ." left where a citation went.
    out.replace(" .", ".")
}

const CLUSTER: &str = "You're the news editor of a bulletin board system's daily paper. You're given a numbered list of news items \
found by a search, each with its outlet and headline. Group the items that report the same event into stories, and pick the most \
newsworthy stories for the section, the most important first. Skip ads, opinion columns, listicles, live blogs and items that aren't news. \
Skip any story the paper already has (listed). Answer with JSON only: \
{\"stories\": [{\"headline\": \"...\", \"search\": \"...\", \"items\": [1, 4]}]}. \
A headline is plain, under 60 characters, no closing period. search: 2 to 5 key words that would find other reports of the same story \
(names, places, the thing that happened; no symbols).";

const WRITE: &str = "You write for the news page of a bulletin board system. You're given numbered sources about one story: \
each its outlet, headline, and text (or only a snippet). Write in your own words, plainly, like a wire service: no opinions, no hype. \
Answer with JSON only: {\"headline\": \"...\", \"overview\": \"...\", \"summary\": \"...\", \"differ\": \"...\", \"sources\": [{\"n\": 1, \"says\": \"...\"}]}. \
headline: at most 60 characters, plain, no closing period. \
overview: one line of at most 60 characters that tells what the story is about beyond the headline, without repeating it; no citations. \
summary: 2 or 3 short paragraphs separated by a blank line, 80 to 180 words in all. Every sentence ends with the numbers of the sources it \
comes from, like [1] or [2][3]. Use only facts the cited sources state, and say so when something is unconfirmed. \
differ: if the sources disagree on facts or tell it differently, one or two sentences on how, with citations; otherwise \"\". \
Never invent a disagreement. \
sources: for each source, one sentence under 20 words on what it adds or its angle. \
If the sources aren't really about one story, answer {\"skip\": true}.";

const CHECK: &str = "You check a news summary against its sources. You're given numbered sources and numbered sentences; each sentence \
names the sources it comes from in brackets. For each sentence, decide whether the sources it names support it (a fair summary is fine; \
a fact they don't state is not). Answer with JSON only: {\"unsupported\": [sentence numbers]}.";

/// The AI's settings for the news: `[ai]` with the news's effort and
/// enough tokens for a story.
fn ai_cfg(state: &BbsState) -> crate::config::Ai {
    let cfg = state.config();
    let mut ai = cfg.ai.clone();
    if !cfg.news.ai_effort.trim().is_empty() {
        ai.reasoning_effort = cfg.news.ai_effort.trim().to_string();
    }
    ai.max_tokens = ai.max_tokens.max(cfg.news.max_tokens);
    ai.timeout_seconds = ai.timeout_seconds.max(180);
    ai
}

/// A story written, checked, and its sources: ready to keep.
struct Written {
    headline: String,
    dek: String,
    summary: String,
    differ: String,
    says: Vec<String>,
}

/// One story from its sources (and their text, where it could be read).
async fn write_story(ai: &crate::config::Ai, sources: &[(Found, Option<String>)]) -> anyhow::Result<Option<Written>> {
    let mut prompt = String::new();
    for (i, (f, text)) in sources.iter().enumerate() {
        let body = match text {
            Some(t) if t.chars().count() > f.snippet.chars().count() => t.chars().take(SOURCE_CHARS).collect::<String>(),
            _ => f.snippet.clone(),
        };
        prompt += &format!("[{}] {}: \"{}\"\n{}\n\n", i + 1, outlet(&f.url), f.title, body);
    }
    let answer = crate::aimemory::complete(ai, WRITE, &prompt).await?;
    let Some(v) = json_in(&answer) else { anyhow::bail!("the AI's story wasn't JSON") };
    if v["skip"].as_bool() == Some(true) {
        return Ok(None);
    }
    // (Cut at a word, if the AI ran long: a headline with "..." reads badly.)
    let headline = clip(plain_text(v["headline"].as_str().unwrap_or("")).trim_end_matches('.'), HEADLINE + 3).trim_end_matches("...").to_string();
    let summary = plain_text(v["summary"].as_str().unwrap_or(""));
    // (Cut at a word if it runs long, as the headline is.)
    let dek = clip(uncited(&plain_text(v["overview"].as_str().unwrap_or(""))).trim_end_matches('.'), DEK + 3).trim_end_matches("...").to_string();
    let differ = plain_text(v["differ"].as_str().unwrap_or(""));
    anyhow::ensure!(!headline.is_empty() && !summary.is_empty(), "the AI wrote no story");
    let mut says = vec![String::new(); sources.len()];
    for s in v["sources"].as_array().into_iter().flatten() {
        if let Some(n) = s["n"].as_u64().map(|n| n as usize).filter(|&n| (1..=sources.len()).contains(&n)) {
            says[n - 1] = clip(&plain_text(s["says"].as_str().unwrap_or("")), 140);
        }
    }
    // Every sentence, numbered, for the check: the summary's paragraphs,
    // then the differences.
    let have = sources.len();
    let paras: Vec<Vec<String>> = summary.split("\n\n").map(|p| sentences(&fix_cites(p, have))).collect();
    let differ: Vec<String> = sentences(&fix_cites(&differ, have));
    let all: Vec<&String> = paras.iter().flatten().chain(&differ).collect();
    let mut check = prompt.clone();
    check += "Sentences:\n";
    for (i, s) in all.iter().enumerate() {
        check += &format!("{}. {s}\n", i + 1);
    }
    let answer = crate::aimemory::complete(ai, CHECK, &check).await?;
    let Some(v) = json_in(&answer) else { anyhow::bail!("the AI's check wasn't JSON") };
    let mut bad: HashSet<usize> = v["unsupported"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as usize)).collect();
    // A sentence naming no source is unsupported too.
    for (i, s) in all.iter().enumerate() {
        if cites(s).is_empty() {
            bad.insert(i + 1);
        }
    }
    if bad.len() * 2 > all.len() {
        tracing::info!("news: {headline:?} dropped: {} of {} sentences unsupported", bad.len(), all.len());
        return Ok(None);
    }
    let mut k = 0;
    let mut keep = |list: &[String]| -> String {
        let kept: Vec<&str> = list
            .iter()
            .filter(|_| {
                k += 1;
                !bad.contains(&k)
            })
            .map(String::as_str)
            .collect();
        kept.join(" ")
    };
    let summary: Vec<String> = paras.iter().map(|p| keep(p)).filter(|p| !p.is_empty()).collect();
    let differ = keep(&differ);
    if !bad.is_empty() {
        tracing::info!("news: {headline:?}: {} unsupported sentence(s) dropped", bad.len());
    }
    Ok(Some(Written { headline, dek, summary: summary.join("\n\n"), differ, says }))
}

const SAME: &str = "You check search results for a news story. You're given the story's headline and numbered items found by a search. \
Answer with JSON only: {\"same\": [numbers]}, the items that report this same event (not an earlier or a different one, and not a general piece on the topic).";

/// Which of `found` (numbered from 1) report the story `headline`.
async fn same_story(ai: &crate::config::Ai, headline: &str, found: &[Found]) -> anyhow::Result<HashSet<usize>> {
    let mut prompt = format!("Story: {headline}\n\nItems:\n");
    for (i, f) in found.iter().enumerate() {
        prompt += &format!("{}. [{}] {} - {}\n", i + 1, outlet(&f.url), f.title, clip(&f.snippet, 200));
    }
    let answer = crate::aimemory::complete(ai, SAME, &prompt).await?;
    let Some(v) = json_in(&answer) else { anyhow::bail!("the AI's answer wasn't JSON") };
    Ok(v["same"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as usize)).collect())
}

/// Make one section's news: the stories it hasn't got yet. How many.
async fn refresh_section(state: &BbsState, sec: &NewsSection, client: &reqwest::Client, ai: &crate::config::Ai) -> anyhow::Result<usize> {
    let cfg = state.config();
    let searx = cfg.ai.search_url.clone();
    let now = crate::clock::now().naive_local();
    // Not a story again while the old one's kept (a month's range finds
    // the same pages for weeks).
    let used = state.news.urls_since(now - chrono::Duration::days(cfg.news.keep_days.clamp(3, 365) as i64))?;
    let range = if sec.range.is_empty() { "day" } else { sec.range.as_str() };
    let mut found: Vec<Found> = Vec::new();
    let mut seen_titles = HashSet::new();
    for q in &sec.queries {
        match search_news(client, &searx, q, range, &cfg.news.language).await {
            Ok(list) => {
                for f in list {
                    let t = f.title.to_lowercase();
                    if !used.contains(&f.url) && !found.iter().any(|g| g.url == f.url) && seen_titles.insert(t) {
                        found.push(f);
                    }
                }
            }
            Err(e) => tracing::warn!("news: searching {q:?}: {e:#}"),
        }
    }
    found.truncate(60);
    if found.is_empty() {
        tracing::info!("news: {}: nothing new found", sec.key);
        return Ok(0);
    }
    // What the paper has already, so the AI leaves it out.
    let had: Vec<String> = state.news.stories_between(now - chrono::Duration::hours(36), now + chrono::Duration::hours(1))?.into_iter().filter(|s| s.section == sec.key).map(|s| s.headline).collect();
    let about = if sec.about.is_empty() { sec.queries.join(", ") } else { sec.about.clone() };
    let mut prompt = format!("Section: {} ({about}). Only stories that belong in it; none is a fine answer.\n\nThe paper has already:\n", sec.name);
    if had.is_empty() {
        prompt += "(nothing yet)\n";
    }
    for h in &had {
        prompt += &format!("- {h}\n");
    }
    prompt += &format!("\nUp to {} stories, from these items:\n", cfg.news.stories);
    for (i, f) in found.iter().enumerate() {
        prompt += &format!("{}. [{}] {} - {}\n", i + 1, outlet(&f.url), f.title, clip(&f.snippet, 160));
    }
    let answer = crate::aimemory::complete(ai, CLUSTER, &prompt).await?;
    let Some(v) = json_in(&answer) else { anyhow::bail!("{}: the AI's stories weren't JSON", sec.key) };
    let picks: Vec<(String, String, Vec<usize>)> = v["stories"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let items = s["items"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as usize)).filter(|&n| (1..=found.len()).contains(&n)).collect();
            let headline = s["headline"].as_str().unwrap_or("").to_string();
            let search = s["search"].as_str().map(str::trim).filter(|q| !q.is_empty()).map_or_else(|| headline.clone(), str::to_string);
            (headline, search, items)
        })
        .filter(|(_, _, items): &(String, String, Vec<usize>)| !items.is_empty())
        .take(cfg.news.stories)
        .collect();
    let most = cfg.news.sources.clamp(1, MOST_SOURCES);
    let mut taken: HashSet<String> = HashSet::new();
    let mut made = 0;
    for (rank, (headline, search, items)) in picks.into_iter().enumerate() {
        let mut sources: Vec<Found> = Vec::new();
        let mut outlets = HashSet::new();
        let mut add = |f: Found, sources: &mut Vec<Found>| {
            if sources.len() < most && !used.contains(&f.url) && !taken.contains(&f.url) && outlets.insert(outlet(&f.url)) {
                taken.insert(f.url.clone());
                sources.push(f);
            }
        };
        for i in items {
            add(found[i - 1].clone(), &mut sources);
        }
        if sources.is_empty() {
            continue;
        }
        let picked = sources.len();
        // More outlets on the same story: in the section's range, then
        // (if that's few) the week's.
        for r in [range, "week"] {
            if sources.len() >= most || (r == "week" && (range != "day" || sources.len() >= sec.min_sources.max(2))) {
                continue;
            }
            match search_news(client, &searx, &search, r, &cfg.news.language).await {
                Ok(more) => {
                    for f in more {
                        add(f, &mut sources);
                    }
                }
                Err(e) => tracing::warn!("news: searching {search:?}: {e:#}"),
            }
        }
        // What the second search added, kept only if it's the same story.
        if sources.len() > picked {
            match same_story(ai, &headline, &sources[picked..]).await {
                Ok(keep) => {
                    let mut j = 0;
                    sources.retain(|_| {
                        j += 1;
                        j <= picked || keep.contains(&(j - picked))
                    });
                }
                Err(e) => {
                    tracing::warn!("news: checking {headline:?}'s sources: {e:#}");
                    sources.truncate(picked);
                }
            }
        }
        if sources.len() < sec.min_sources.max(1) {
            tracing::info!("news: {headline:?}: only {} source(s)", sources.len());
            continue;
        }
        let texts = futures_util::future::join_all(sources.iter().map(|f| async move { tokio::time::timeout(Duration::from_secs(25), crate::aichat::read_page(&f.url)).await.ok().and_then(Result::ok) })).await;
        let with: Vec<(Found, Option<String>)> = sources.into_iter().zip(texts.into_iter().map(|t| t.map(|t| plain_text(&t)).filter(|t| !walled(t)))).collect();
        let w = match write_story(ai, &with).await {
            Ok(Some(w)) => w,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("news: writing {headline:?}: {e:#}");
                continue;
            }
        };
        let srcs: Vec<Source> = with
            .iter()
            .enumerate()
            .map(|(i, (f, text))| {
                // The search's snippet, else the page's start.
                let words = if f.snippet.chars().count() >= 40 { f.snippet.clone() } else { text.clone().unwrap_or_default() };
                Source {
                    n: i as u32 + 1,
                    outlet: outlet(&f.url),
                    title: clip(&f.title, 120),
                    url: f.url.clone(),
                    excerpt: clip(&words, EXCERPT),
                    says: w.says[i].clone(),
                    published: f.published.clone(),
                }
            })
            .collect();
        let story = Story { id: 0, section: sec.key.clone(), rank: rank as u32, headline: w.headline, summary: w.summary, differ: w.differ, made_at: crate::clock::now().naive_local(), dek: w.dek };
        state.news.add_story(&story, &srcs)?;
        tracing::info!("news: {}: {:?} ({} sources)", sec.key, story.headline, srcs.len());
        made += 1;
    }
    Ok(made)
}

/// The `news` event (and `fluxbbs news`): each section's new stories
/// (or only `only`'s), and stories past `keep_days` gone. How many made.
pub async fn refresh(state: &BbsState, only: Option<&str>) -> anyhow::Result<usize> {
    let cfg = state.config();
    anyhow::ensure!(cfg.news.enabled, "the news is switched off ([news] enabled)");
    anyhow::ensure!(cfg.ai.enabled && !cfg.ai.base_url.is_empty(), "the news needs the board's AI ([ai])");
    anyhow::ensure!(!cfg.ai.search_url.is_empty(), "the news needs a SearXNG instance ([ai] search_url)");
    use std::sync::atomic::Ordering;
    if state.news.running.swap(true, Ordering::SeqCst) {
        anyhow::bail!("the news is being made already");
    }
    let _running = Running(&state.news.running);
    let ai = ai_cfg(state);
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).user_agent("Mozilla/5.0 (flux/bbs)").build()?;
    let mut made = 0;
    for sec in cfg.news.sections.iter().filter(|s| only.is_none_or(|o| s.key.eq_ignore_ascii_case(o))) {
        match refresh_section(state, sec, &client, &ai).await {
            Ok(n) => made += n,
            Err(e) => tracing::warn!("news: {}: {e:#}", sec.key),
        }
    }
    if cfg.news.keep_days > 0 {
        let gone = state.news.prune(crate::clock::now().naive_local() - chrono::Duration::days(cfg.news.keep_days as i64))?;
        if gone > 0 {
            tracing::info!("news: {gone} old stories gone");
        }
    }
    Ok(made)
}

/// The news made every `[news] every` minutes while the board runs: the
/// first time that long after the latest story (but not in the first
/// five minutes up, so a restart doesn't make it at once), then that
/// long after each run began.
pub async fn keeper(state: std::sync::Arc<BbsState>) {
    let up = std::time::Instant::now();
    let mut last: Option<std::time::Instant> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let cfg = state.config();
        if !cfg.news.enabled || cfg.news.every == 0 {
            continue;
        }
        let every = Duration::from_secs(cfg.news.every as u64 * 60);
        let due = match last {
            Some(t) => t.elapsed() >= every,
            None => {
                let latest = state.news.latest().ok().flatten();
                let now = crate::clock::now().naive_local();
                up.elapsed() >= Duration::from_secs(300) && latest.is_none_or(|l| (now - l).to_std().is_ok_and(|d| d >= every))
            }
        };
        if !due {
            continue;
        }
        last = Some(std::time::Instant::now());
        match refresh(&state, None).await {
            Ok(n) => tracing::info!("news: {n} new stories"),
            Err(e) => tracing::warn!("news: {e:#}"),
        }
    }
}

/// `fluxbbs news [--section KEY] [config]`: make the news now.
pub async fn from_args(args: &[String]) -> anyhow::Result<()> {
    let mut only = None;
    let mut config = crate::config::default_path();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--section" => only = Some(it.next().cloned().ok_or_else(|| anyhow::anyhow!("--section takes a section's key"))?),
            other => config = other.into(),
        }
    }
    let state = crate::load_state(&config)?;
    let n = refresh(&state, only.as_deref()).await?;
    println!("{n} new stories");
    Ok(())
}

// ---- reading it -----------------------------------------------------------

/// How the front page is sorted.
#[derive(Clone, Copy, PartialEq)]
enum Sort {
    /// By section, then the latest run's order.
    Front,
    Latest,
    Top,
}

/// What the front page shows.
#[derive(Clone)]
enum Showing {
    /// The last day's stories.
    Today,
    Day(NaiveDate),
    Found(String),
}

/// A front page's row: an edition's head, or a story (its place in the list).
struct Item {
    story: Option<usize>,
    waves: Waves,
}

/// What a caller did on the front page.
enum Pick {
    /// A story to read (its place in the list).
    Open(usize),
    /// One of the page's keys, with the story the bar was on.
    Key(char, usize),
    Back,
    Quit,
}

/// A gap between stories this long (minutes) starts another edition.
const EDITION_GAP: i64 = 45;

/// Word-wrapped `text` at `width`, each line led by `indent`.
fn wrap(text: &str, width: usize, indent: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
            out.push(format!("{indent}{cur}"));
            cur.clear();
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        out.push(format!("{indent}{cur}"));
    }
    out
}

/// `s` cut or padded to `n` characters.
fn fit(s: &str, n: usize) -> String {
    let s: String = s.chars().take(n).collect();
    format!("{s:<n$}")
}

// The story page's colors (ANSI: the pager reads escapes): newsprint,
// with the section's color for its kicker and yellow for the rules' heads.
const HEAD: &str = "\x1b[1;37m";
const DIM: &str = "\x1b[0;37m";
const SOFT: &str = "\x1b[1;30m";
const MARK: &str = "\x1b[0;36m";
const RULE: &str = "\x1b[1;33m";
const OFF: &str = "\x1b[0m";

/// Each section's color, in the front page's order: (pipe code, ANSI).
const SECTION_COLORS: &[(&str, &str)] = &[("|11", "\x1b[1;36m"), ("|10", "\x1b[1;32m"), ("|13", "\x1b[1;35m"), ("|14", "\x1b[1;33m"), ("|12", "\x1b[1;31m"), ("|09", "\x1b[1;34m")];

/// A line's citations (`[2]`) in their own color, then back to `base`.
fn cited(line: &str, base: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(i) = rest.find('[') {
        let Some(j) = rest[i..].find(']') else { break };
        let inside = &rest[i + 1..i + j];
        out.push_str(&rest[..i]);
        if !inside.is_empty() && inside.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(&format!("{MARK}[{inside}]{base}"));
        } else {
            out.push_str(&rest[i..=i + j]);
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    format!("{base}{out}")
}

/// A rule with a head on it: `-- SOURCES -----`.
fn headed_rule(title: &str) -> String {
    let n = 76usize.saturating_sub(title.chars().count() + 4);
    format!(" {SOFT}\u{2500}\u{2500} {RULE}{} {SOFT}{}", title.to_uppercase(), "\u{2500}".repeat(n))
}

impl Session {
    /// New: `%NH`, the front page's lead: the newest edition's first
    /// section, its best-ranked story. Empty without news.
    pub(crate) fn news_lead(&self) -> String {
        let cfg = self.state.config();
        if !crate::features::command_on(&cfg, "NW") {
            return String::new();
        }
        let order: Vec<&str> = cfg.news.sections.iter().map(|s| s.key.as_str()).collect();
        let pos = |k: &str| order.iter().position(|o| *o == k).unwrap_or(usize::MAX);
        let list = self.state.news.newest_edition().unwrap_or_default();
        list.into_iter().min_by_key(|s| (pos(&s.section), s.rank)).map(|s| s.headline).unwrap_or_default()
    }

    fn news_section_name(&self, key: &str) -> String {
        self.state.config().news.sections.iter().find(|s| s.key == key).map_or_else(|| key.to_string(), |s| s.name.clone())
    }

    /// A section's color (pipe code, ANSI).
    fn news_section_color(&self, key: &str) -> (&'static str, &'static str) {
        let i = self.state.config().news.sections.iter().position(|s| s.key == key).unwrap_or(SECTION_COLORS.len() - 1);
        SECTION_COLORS[i % SECTION_COLORS.len()]
    }

    /// The paper's name: `[news] name`, else the board's and "news".
    fn news_paper(&self) -> String {
        let cfg = self.state.config();
        if cfg.news.name.trim().is_empty() { format!("{} news", cfg.bbs.name) } else { cfg.news.name.trim().to_string() }
    }

    /// `NW`: the news, the front page first: the stories by edition
    /// (each run of the board's, the latest first), each with a line on
    /// what it's about, those the caller hasn't read marked.
    pub async fn news(&mut self) -> io::Result<()> {
        let cfg = self.state.config();
        if !cfg.news.enabled {
            self.newline();
            return self.say_line("NewsOff").await;
        }
        if !self.acs(&cfg.news.acs) {
            self.newline();
            return self.say_line("NewsNoAccess").await;
        }
        let mut showing = Showing::Today;
        let mut sort = Sort::Front;
        let mut at: Option<i64> = None;
        loop {
            let io = |e: anyhow::Error| io::Error::other(e);
            let now = crate::clock::now().naive_local();
            let mut list = match &showing {
                Showing::Today => {
                    // The last day's, or the latest day's if none are that new.
                    let since = match self.state.news.latest().map_err(io)? {
                        Some(l) if l < now - chrono::Duration::hours(24) => l - chrono::Duration::hours(24),
                        _ => now - chrono::Duration::hours(24),
                    };
                    self.state.news.stories_between(since, now + chrono::Duration::hours(1)).map_err(io)?
                }
                Showing::Day(d) => {
                    let from = d.and_hms_opt(0, 0, 0).unwrap_or_default();
                    self.state.news.stories_between(from, from + chrono::Duration::days(1)).map_err(io)?
                }
                Showing::Found(q) => {
                    let words: Vec<String> = q.split_whitespace().map(str::to_string).collect();
                    self.state.news.search(&words, 200).map_err(io)?
                }
            };
            if list.is_empty() {
                self.newline();
                if matches!(showing, Showing::Found(_)) {
                    self.say_line("NewsFoundNone").await?;
                    showing = Showing::Today;
                    continue;
                }
                return self.say_line("NewsNone").await;
            }
            let tallies = self.state.news.tallies().map_err(io)?;
            let counts = self.state.news.source_counts().map_err(io)?;
            let read = if self.user.id == 0 { HashSet::new() } else { self.state.news.read_ids(self.user.id).map_err(io)? };
            let order: Vec<String> = cfg.news.sections.iter().map(|s| s.key.clone()).collect();
            let sec_pos = |k: &str| order.iter().position(|o| o == k).unwrap_or(usize::MAX);
            // Editions: a run's stories are made minutes apart, and a gap
            // as long as EDITION_GAP starts another.
            list.sort_by_key(|s| std::cmp::Reverse(s.made_at));
            let mut edition: HashMap<i64, usize> = HashMap::new();
            for (i, s) in list.iter().enumerate() {
                let gap = i > 0 && list[i - 1].made_at - s.made_at >= chrono::Duration::minutes(EDITION_GAP);
                let ed = if i == 0 { 0 } else { edition[&list[i - 1].id] + gap as usize };
                edition.insert(s.id, ed);
            }
            let score = |s: &Story| tallies.get(&s.id).map_or(0, |t| t.0);
            match sort {
                // Each edition by section, a section in the run's order.
                Sort::Front => list.sort_by_key(|s| (edition[&s.id], sec_pos(&s.section), s.rank)),
                Sort::Latest => {}
                Sort::Top => list.sort_by_key(|s| std::cmp::Reverse(score(s))),
            }
            let unread = |s: &Story| self.user.id != 0 && !read.contains(&s.id);
            let rows: Vec<BarRow> = list
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (score, comments) = tallies.get(&s.id).copied().unwrap_or((0, 0));
                    let score = if score == 0 { String::new() } else { crate::pushes::score_label(Some(score)) };
                    let talk = if comments > 0 { format!("{comments}c") } else { String::new() };
                    let when = self.local(s.made_at).format("%H:%M");
                    let n = counts.get(&s.id).copied().unwrap_or(0);
                    let mark = if unread(s) { '*' } else { ' ' };
                    let plain = format!("{mark}{} {} {when} {n:>2}src {score:>4} {talk:>3}", fit(&self.news_section_name(&s.section), 8), fit(&s.headline, 47));
                    let votes = format!("{score} {talk}").trim().to_string();
                    let kicker = format!("{}{}", self.news_section_color(&s.section).0, self.news_section_name(&s.section).to_uppercase());
                    let (hl, ov, rd) = if unread(s) { ("|15", "|07", "|14\u{25a0}") } else { ("|07", "|08", " ") };
                    let waves = crate::session::waves([
                        ("RD", rd.to_string()),
                        ("SE", kicker),
                        ("HL", format!("{hl}{}", fluxbbs_term::lit(&clip(&s.headline, 54)))),
                        ("OV", format!("{ov}{}", fluxbbs_term::lit(&dek_of(s)))),
                        ("TI", when.to_string()),
                        ("NS", format!("{n} src")),
                        ("VT", votes),
                    ]);
                    BarRow { n: i as u32 + 1, waves, plain }
                })
                .collect();
            // The front page's items: each edition's head, then its stories
            // (sorted by votes, one head over them all).
            let str = self.strings();
            let today = self.local(now).date();
            let mut items: Vec<Item> = Vec::new();
            for (i, s) in list.iter().enumerate() {
                let ed = edition[&s.id];
                let new_head = match sort {
                    Sort::Top => i == 0,
                    _ => i == 0 || edition[&list[i - 1].id] != ed,
                };
                if new_head {
                    let mine: Vec<&Story> = list.iter().filter(|t| sort == Sort::Top || edition[&t.id] == ed).collect();
                    let label = if sort == Sort::Top {
                        str.get("NewsTopHead").to_string()
                    } else {
                        let t = self.local(mine.iter().map(|t| t.made_at).max().unwrap_or(s.made_at));
                        let when = if t.date() == today { t.format("%H:%M").to_string() } else { t.format("%a %-d %b, %H:%M").to_string() };
                        crate::strings::substitute(str.get(if ed == 0 { "NewsEditionLatest" } else { "NewsEdition" }), &[&when])
                    };
                    let n = mine.iter().filter(|t| unread(t)).count();
                    let un = if n > 0 { crate::strings::substitute(str.get("NewsEditionUnread"), &[&n.to_string()]) } else { String::new() };
                    let (label, un) = (label.to_uppercase(), un.to_uppercase());
                    let width = fluxbbs_term::codes::width(&label) + fluxbbs_term::codes::width(&un);
                    let rule = "\u{2500}".repeat(74usize.saturating_sub(width));
                    let waves = crate::session::waves([("ED", label), ("UN", un), ("RL", rule)]);
                    items.push(Item { story: None, waves });
                }
                items.push(Item { story: Some(i), waves: rows[i].waves.clone() });
            }
            // Where the bar starts: where the caller was, else the first
            // story not yet read.
            let start = at.and_then(|id| list.iter().position(|s| s.id == id)).or_else(|| list.iter().position(&unread)).unwrap_or(0);
            let what = match &showing {
                Showing::Today => self.local(list.iter().map(|s| s.made_at).max().unwrap_or(now)).format("%a %d %b %Y").to_string(),
                Showing::Day(d) => d.format("%a %d %b %Y").to_string(),
                Showing::Found(q) => str.fmt("NewsFoundWhat", &[q]),
            };
            let header = str.fmt("NewsHeader", &[&self.news_paper(), &what]);
            // The dateline: the issue (the days there's been news, to the
            // one shown), the day, the stories and the latest's time.
            let days = self.state.news.days().map_err(io)?;
            let shown = match &showing {
                Showing::Day(d) => Some(*d),
                _ => list.iter().map(|s| s.made_at.date()).max(),
            };
            let issue = shown.map_or(0, |d| days.iter().filter(|&&x| x <= d).count());
            let latest = list.iter().map(|s| s.made_at).max().unwrap_or(now);
            let day = match &showing {
                Showing::Found(_) => what.clone(),
                _ => self.local(latest).format("%A, %-d %B %Y").to_string().to_uppercase(),
            };
            let top = crate::session::waves([
                ("NO", issue.to_string()),
                ("DA", fluxbbs_term::lit(&day)),
                ("SC", list.len().to_string()),
                ("UP", self.local(latest).format("%H:%M").to_string()),
            ]);
            let help = str.get("NewsHelp").to_string();
            let start_item = items.iter().position(|it| it.story == Some(start)).unwrap_or(0);
            let keys = "FSDM<>";
            let pick = match self.news_front(&items, start_item, &top, &help, keys).await? {
                Some(p) => p,
                // Plain text, or a theme without the screens: the usual list.
                None => {
                    self.bar_top = top;
                    let spec = BarSpec { prefix: "NEWS", header: &header, rows: &rows, labels: &[], start, help: &help, ask: "", can_go_up: false, keys };
                    match self.area_bar(&spec).await? {
                        BarPick::Area(n) => Pick::Open(n as usize - 1),
                        BarPick::Key(c, n) => Pick::Key(c, (n as usize).saturating_sub(1)),
                        BarPick::Up | BarPick::Quit => Pick::Quit,
                        BarPick::Stay => Pick::Back,
                        BarPick::Refresh(_) => continue,
                    }
                }
            };
            match pick {
                Pick::Open(i) => {
                    if let Some(s) = list.get(i) {
                        let ids: Vec<i64> = list.iter().map(|s| s.id).collect();
                        at = Some(self.news_story(&ids, s.id).await?);
                    }
                }
                Pick::Key('F', _) => {
                    self.bar_line();
                    self.say("NewsFindAsk").await?;
                    let q = self.read_string(ReadOpts::new(40)).await?;
                    if !q.trim().is_empty() {
                        showing = Showing::Found(q.trim().to_string());
                        at = None;
                    }
                }
                Pick::Key('S', i) => {
                    sort = match sort {
                        Sort::Front => Sort::Latest,
                        Sort::Latest => Sort::Top,
                        Sort::Top => Sort::Front,
                    };
                    let name = match sort {
                        Sort::Front => "NewsSortFront",
                        Sort::Latest => "NewsSortLatest",
                        Sort::Top => "NewsSortTop",
                    };
                    self.note = Some(self.strings().get(name).to_string());
                    at = list.get(i).map(|s| s.id);
                }
                Pick::Key('D', _) => {
                    if let Some(d) = self.news_pick_day().await? {
                        showing = d;
                        at = None;
                    }
                }
                // Every story here read.
                Pick::Key('M', i) => {
                    if self.user.id != 0 {
                        let ids: Vec<i64> = list.iter().map(|s| s.id).collect();
                        self.state.news.mark_read(self.user.id, &ids, now).map_err(io)?;
                        self.note = Some(self.strings().get("NewsMarkedRead").to_string());
                    }
                    at = list.get(i).map(|s| s.id);
                }
                // To the next section's first story, or back to this one's
                // first (the one before's, from there).
                Pick::Key(c @ ('<' | '>'), i) => {
                    let i = i.min(list.len() - 1);
                    let firsts: Vec<usize> = (0..list.len()).filter(|&j| j == 0 || list[j].section != list[j - 1].section).collect();
                    let cur = firsts.iter().rposition(|&j| j <= i).unwrap_or(0);
                    let to = match c {
                        '>' => firsts[(cur + 1) % firsts.len()],
                        _ if i > firsts[cur] => firsts[cur],
                        _ => firsts[(cur + firsts.len() - 1) % firsts.len()],
                    };
                    at = Some(list[to].id);
                }
                Pick::Quit => return Ok(()),
                Pick::Back => {
                    // Esc from a search or a back issue goes back to today's.
                    if matches!(showing, Showing::Today) {
                        return Ok(());
                    }
                    showing = Showing::Today;
                    at = None;
                }
                Pick::Key(..) => {}
            }
        }
    }

    /// New: the front page, laid out by hand, as `area_bar`'s lists are but
    /// with rows of their own heights: `NEWSTOP` (`top` for its codes), the
    /// items in a window (a story in `NEWSMID`'s lines, under the bar in
    /// `NEWSHIL`'s; an edition's head in `NEWSHEAD`'s), and `NEWSBOT` with
    /// the help and the prompt. None in plain text or without the screens.
    async fn news_front(&mut self, items: &[Item], start: usize, top: &Waves, help: &str, keys: &str) -> io::Result<Option<Pick>> {
        if self.plain() || ["NEWSTOP", "NEWSMID", "NEWSHEAD", "NEWSBOT"].iter().any(|n| !self.text_file_exists(n)) {
            return Ok(None);
        }
        let trim = |mut b: Vec<u8>| {
            while b.last().is_some_and(|&c| c == b'\n' || c == b'\r') {
                b.pop();
            }
            b
        };
        let mid = trim(self.read_text_file("NEWSMID").await?.unwrap_or_default());
        let hil = match self.read_text_file("NEWSHIL").await? {
            Some(h) => trim(h),
            None => mid.clone(),
        };
        let head = trim(self.read_text_file("NEWSHEAD").await?.unwrap_or_default());
        let bot = trim(self.read_text_file("NEWSBOT").await?.unwrap_or_default());
        let lines = |b: &[u8]| b.iter().filter(|&&c| c == b'\n').count() + 1;
        let (story_h, head_h, bot_h) = (lines(&mid), lines(&head), lines(&bot));
        let bar = bot.windows(3).any(|w| w == b"~PC");
        let h = |i: usize| if items[i].story.is_some() { story_h } else { head_h };
        let stories: Vec<usize> = (0..items.len()).filter(|&i| items[i].story.is_some()).collect();
        let Some(&last) = stories.last() else { return Ok(Some(Pick::Back)) };
        let mut cur = if items.get(start).is_some_and(|it| it.story.is_some()) { start } else { stories[0] };
        let (mut first, mut top_y, mut height, mut prompt) = (0usize, 1u16, 3usize, (1u16, 1u16));
        let mut shown: Vec<(usize, u16)> = Vec::new();
        let mut drawn = None;
        let mut redraw = true;
        loop {
            if redraw {
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.clear_screen();
                self.show_file_with("NEWSTOP", top).await?;
                self.goto_pos(*b"TL");
                if self.screen.where_x() != 1 {
                    self.newline();
                }
                top_y = self.screen.where_y();
                height = (self.screen.rows() as usize + 1).saturating_sub(top_y as usize + bot_h).max(story_h + head_h);
                shown.clear();
            }
            // The window: from `first`, as many as fit, the bar's among
            // them (and its edition's head over it when it's the first).
            let fits = |from: usize| (from..=cur).map(h).sum::<usize>() <= height;
            if cur < first {
                first = cur;
            }
            while first < cur && !fits(first) {
                first += 1;
            }
            if first == cur && cur > 0 && items[cur - 1].story.is_none() && fits(cur - 1) {
                first = cur - 1;
            }
            let mut want = Vec::new();
            let mut used = 0;
            for i in first..items.len() {
                if used + h(i) > height {
                    break;
                }
                want.push((i, top_y + used as u16));
                used += h(i);
            }
            if want.first() != shown.first() {
                for &(i, y) in &want {
                    let src = match items[i].story {
                        None => &head,
                        Some(_) if i == cur => &hil,
                        Some(_) => &mid,
                    };
                    self.template_at(src, &items[i].waves, y, h(i) as u16).await?;
                }
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                for k in used..height {
                    self.screen.goto(1, top_y + k as u16);
                    self.screen.clear_eol();
                }
                shown = want;
            } else if let Some(prev) = drawn.filter(|&p| p != cur) {
                for (i, src) in [(prev, &mid), (cur, &hil)] {
                    if let Some(&(_, y)) = shown.iter().find(|&&(j, _)| j == i) {
                        self.template_at(src, &items[i].waves, y, h(i) as u16).await?;
                    }
                }
            }
            drawn = Some(cur);
            if redraw {
                self.screen.goto(1, top_y + height as u16);
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.show_file("NEWSBOT").await?;
                let noted = self.note.take();
                let help = noted.as_deref().unwrap_or(help);
                if bar && self.goto_pos(*b"HP") {
                    let room = 80usize.saturating_sub(self.screen.where_x() as usize);
                    self.show(&fluxbbs_term::codes::squish(help, room)).await?;
                    self.goto_pos(*b"PC");
                } else {
                    self.newline();
                    let help: String = crate::editor::strip_colours(help).chars().take(79).collect();
                    self.show(&help).await?;
                }
                prompt = (self.screen.where_x(), self.screen.where_y());
                redraw = false;
            }
            self.screen.goto(prompt.0, prompt.1);
            let key = self.read_key().await?;
            let at = stories.iter().position(|&i| i == cur).unwrap_or(0);
            // A window's worth of stories, for the page keys.
            let page = (height / story_h).max(1);
            cur = match key {
                Key::Up => stories[(at + stories.len() - 1) % stories.len()],
                Key::Down => stories[(at + 1) % stories.len()],
                Key::PgUp => stories[at.saturating_sub(page)],
                Key::PgDn => stories[(at + page).min(stories.len() - 1)],
                Key::Home => stories[0],
                Key::End => last,
                Key::Enter | Key::Right => return Ok(Some(Pick::Open(items[cur].story.unwrap_or(0)))),
                Key::Esc | Key::Left | Key::Backspace => return Ok(Some(Pick::Back)),
                k if k.upper().is_some_and(|c| keys.contains(c)) => return Ok(Some(Pick::Key(k.upper().unwrap_or(' '), items[cur].story.unwrap_or(0)))),
                k if k.upper() == Some('B') => return Ok(Some(Pick::Back)),
                k if k.upper() == Some('Q') => return Ok(Some(Pick::Quit)),
                _ => cur,
            };
        }
    }

    /// `d` on the front page: a day to read, from the days kept.
    async fn news_pick_day(&mut self) -> io::Result<Option<Showing>> {
        let days = self.state.news.days().map_err(io::Error::other)?;
        if days.len() < 2 {
            self.note = Some(self.strings().get("NewsOneDay").to_string());
            return Ok(None);
        }
        let rows: Vec<BarRow> = days
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let issue = days.len() - i;
                let waves = crate::session::waves([("DA", d.format("%A, %-d %B %Y").to_string()), ("SC", format!("no. {issue}"))]);
                BarRow { n: i as u32 + 1, waves, plain: format!(" {}  no. {issue}", d.format("%a %d %b %Y")) }
            })
            .collect();
        self.bar_top = crate::session::waves([("SC", days.len().to_string())]);
        let header = self.strings().get("NewsDaysHeader").to_string();
        let help = self.strings().get("NewsDaysHelp").to_string();
        let spec = BarSpec { prefix: "NEWSDAY", header: &header, rows: &rows, labels: &[], start: 0, help: &help, ask: "", can_go_up: true, keys: "" };
        Ok(match self.area_bar(&spec).await? {
            BarPick::Area(n) => days.get(n as usize - 1).map(|&d| if n == 1 { Showing::Today } else { Showing::Day(d) }),
            _ => None,
        })
    }

    /// A story as a page, set as a newspaper's: the paper and the day,
    /// the section's kicker, the headline, the byline, the lede and the
    /// rest, where the sources differ, the sources, and the readers'
    /// votes and comments.
    fn news_page(&self, s: &Story, sources: &[Source]) -> String {
        let mut out = Vec::new();
        let str = self.strings();
        // (The paper and the day are the title line over it.)
        out.push(format!(" {DIM}{}", "\u{2550}".repeat(78)));
        let (_, colour) = self.news_section_color(&s.section);
        out.push(format!(" {colour}{}", self.news_section_name(&s.section).to_uppercase()));
        for l in wrap(&s.headline.to_uppercase(), 76, " ") {
            out.push(format!("{HEAD}{l}"));
        }
        out.push(format!(" {SOFT}{}", "\u{2500}".repeat(40)));
        let ai = self.state.config().ai.name.clone();
        let by = str.fmt("NewsByline", &[&ai, &sources.len().to_string(), &self.local(s.made_at).format("%H:%M").to_string()]);
        out.push(format!(" {SOFT}{by}"));
        for (i, p) in s.summary.split("\n\n").enumerate() {
            out.push(String::new());
            // The lede bright, the rest in body type.
            let base = if i == 0 { HEAD } else { DIM };
            out.extend(wrap(p, 76, " ").into_iter().map(|l| cited(&l, base)));
        }
        if !s.differ.is_empty() {
            out.push(String::new());
            out.push(headed_rule(str.get("NewsDiffer")));
            out.extend(wrap(&s.differ, 76, " ").into_iter().map(|l| cited(&l, DIM)));
        }
        out.push(String::new());
        out.push(headed_rule(str.get("NewsSources")));
        for src in sources {
            let room = 70usize.saturating_sub(src.outlet.chars().count());
            out.push(format!(" {MARK}[{}] {HEAD}{} {SOFT}\u{00b7} {DIM}{}", src.n, src.outlet.to_uppercase(), clip(&src.title, room)));
            if !src.says.is_empty() {
                out.extend(wrap(&src.says, 72, "     ").into_iter().map(|l| format!("{SOFT}{l}")));
            }
        }
        let pushes = self.state.news.pushes(s.id).unwrap_or_default();
        if !pushes.is_empty() {
            let score: i32 = pushes.iter().map(|p| p.kind as i32).sum();
            out.push(String::new());
            let score = if pushes.iter().any(|p| p.kind != 0) { crate::pushes::score_label(Some(score)) } else { String::new() };
            out.push(headed_rule(str.fmt("NewsVotes", &[&score]).trim()));
            for p in &pushes {
                let (sign, colour) = match p.kind {
                    1 => ('+', "\x1b[1;32m"),
                    -1 => ('-', "\x1b[1;31m"),
                    _ => ('>', MARK),
                };
                out.push(format!(" {colour}{sign} {HEAD}{} {DIM}{} {SOFT}{}", fit(&p.handle, 12), fit(&p.text, crate::pushes::PUSH_WIDTH), self.local(p.at).format("%m/%d")));
            }
        }
        out.push(String::new());
        out.join(&format!("{OFF}\r\n")) + OFF
    }

    /// A source: its outlet, headline, a little of its own words and
    /// its address (a link, where the terminal takes them).
    fn news_source_page(&self, s: &Story, src: &Source) -> String {
        let str = self.strings();
        let mut out = vec![format!(" {DIM}{}", "\u{2550}".repeat(78))];
        out.push(format!(" {SOFT}{}", str.fmt("NewsSourceOf", &[&clip(&s.headline, 60)])));
        out.push(String::new());
        out.push(format!(" {MARK}[{}] {HEAD}{}", src.n, src.outlet.to_uppercase()));
        if !src.published.is_empty() {
            out.push(format!(" {SOFT}    {}", src.published.replace('T', " ").chars().take(16).collect::<String>()));
        }
        out.push(String::new());
        out.extend(wrap(&src.title, 72, "     ").into_iter().map(|l| format!("{HEAD}{l}")));
        if !src.excerpt.is_empty() {
            out.push(String::new());
            out.extend(wrap(&format!("\"{}\"", src.excerpt), 70, "     ").into_iter().map(|l| format!("{DIM}{l}")));
        }
        if !src.says.is_empty() {
            out.push(String::new());
            out.push(headed_rule(str.get("NewsSourceSays")));
            out.extend(wrap(&src.says, 72, "     ").into_iter().map(|l| format!("{DIM}{l}")));
        }
        out.push(String::new());
        out.push(headed_rule(str.get("NewsSourceLink")));
        // A long one from the edge, so it wraps whole (and stays a link).
        let lead = if src.url.chars().count() > 74 { " " } else { "     " };
        out.push(format!("{DIM}{lead}{}", src.url));
        out.join(&format!("{OFF}\r\n")) + OFF
    }

    /// One story, then the next or the one before (`ids`, the front
    /// page's order). The story the caller ended on.
    async fn news_story(&mut self, ids: &[i64], first: i64) -> io::Result<i64> {
        let mut i = ids.iter().position(|&id| id == first).unwrap_or(0);
        let mut extra: Vec<char> = vec!['N', '-', '>', '<', 'X', 'I', 'C'];
        extra.extend(('1'..='9').take(MOST_SOURCES));
        // Where the page was, and whether it's still on the screen (after
        // a vote, asked on its bottom line).
        let (mut top, mut over) = (0usize, false);
        loop {
            let Some(s) = self.state.news.story(ids[i]).map_err(io::Error::other)? else { return Ok(ids[i]) };
            let sources = self.state.news.sources(s.id).map_err(io::Error::other)?;
            if self.user.id != 0 {
                self.state.news.mark_read(self.user.id, &[s.id], crate::clock::now().naive_local()).map_err(io::Error::other)?;
            }
            let grid = crate::pager::render(self.news_page(&s, &sources).as_bytes(), true);
            let day = self.local(s.made_at).format("%A, %-d %B %Y").to_string();
            let title = format!("{} \u{00b7} {}", self.news_paper().to_uppercase(), day.to_uppercase());
            let paged = self.page_grid_titled("NewsViewerTitle", &title, &grid, "NewsStoryHelp", &extra, top, &['X', 'C'], over).await?;
            (top, over) = (0, false);
            match paged.key {
                Some('N' | '>') => i = (i + 1) % ids.len(),
                Some('-' | '<') => i = (i + ids.len() - 1) % ids.len(),
                Some(c @ ('X' | 'C')) => {
                    self.news_push(&s, c == 'C').await?;
                    (top, over) = (paged.top, true);
                }
                // `i` and a number: the sources, one by one.
                Some(c @ ('I' | '1'..='9')) => {
                    top = paged.top;
                    let mut k = if c == 'I' { 0 } else { c as usize - '1' as usize };
                    while let Some(src) = sources.get(k) {
                        let grid = crate::pager::render(self.news_source_page(&s, src).as_bytes(), true);
                        let title = format!("{} \u{00b7} SOURCE {} OF {}", self.news_paper().to_uppercase(), k + 1, sources.len());
                        match self.page_grid_titled("NewsViewerTitle", &title, &grid, "NewsSourceHelp", &['N', '-', '>', '<'], 0, &[], false).await?.key {
                            Some('N' | '>') => k = (k + 1) % sources.len(),
                            Some('-' | '<') => k = (k + sources.len() - 1) % sources.len(),
                            _ => break,
                        }
                    }
                }
                _ => return Ok(ids[i]),
            }
        }
    }

    /// `x` (or `c`, straight to a comment) on a story: a vote up or down,
    /// or a one-line comment, as on files; a co-SysOp can take a caller's
    /// off.
    async fn news_push(&mut self, s: &Story, comment: bool) -> io::Result<()> {
        if self.user.id == 0 {
            return Ok(());
        }
        let moderator = self.is_cosysop();
        let kind: i8 = if comment {
            0
        } else {
            self.bar_line();
            self.say(if moderator { "NewsPushAskSysop" } else { "NewsPushAsk" }).await?;
            match self.read_key().await? {
                Key::Char(b'+') => 1,
                Key::Char(b'-') => -1,
                Key::Char(b'>') => 0,
                k if moderator && k.upper() == Some('D') => {
                    let pushes = self.state.news.pushes(s.id).map_err(io::Error::other)?;
                    let voters: Vec<(u32, &str)> = pushes.iter().map(|p| (p.user_id, p.handle.as_str())).collect();
                    if let Some(uid) = self.ask_whose_pushes(&voters).await? {
                        let n = self.state.news.remove_pushes(s.id, uid).map_err(io::Error::other)?;
                        tracing::info!("{}: took {n} pushes by user {uid} off news story {}", self.user.handle, s.id);
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        };
        let now = crate::clock::now().naive_local();
        if let Some(last) = self.state.news.last_push_at(self.user.id).map_err(io::Error::other)?
            && (0..crate::pushes::PUSH_GAP).contains(&(now - last).num_seconds())
        {
            self.bar_note("PushTooFast").await?;
            return Ok(());
        }
        self.bar_line();
        self.say(match kind {
            1 => "PushTextUp",
            -1 => "NewsPushTextDown",
            _ => "NewsPushTextComment",
        })
        .await?;
        let text = self.read_string(ReadOpts::new(crate::pushes::PUSH_WIDTH)).await?;
        let text = crate::rumors::clean(&crate::editor::strip_colours(&text));
        if kind == 0 && text.is_empty() {
            return Ok(());
        }
        let p = Push { user_id: self.user.id, handle: self.user.handle.clone(), kind, text, at: now };
        self.state.news.add_push(s.id, &p).map_err(io::Error::other)?;
        tracing::info!("{}: pushed news story {} ({kind})", self.user.handle, s.id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_and_citations() {
        let p = "The Fed held rates on Tuesday [1][2]. Powell said a cut is likely. [3] U.S. stocks fell 1% [4]. Officials in the U.S. agreed.";
        let s = sentences(p);
        assert_eq!(s, vec!["The Fed held rates on Tuesday [1][2].", "Powell said a cut is likely. [3]", "U.S. stocks fell 1% [4].", "Officials in the U.S. agreed."]);
        assert_eq!(cites(&s[0]), vec![1, 2]);
        assert_eq!(cites("a [1, 3]."), vec![1, 3]);
        assert!(cites(&s[3]).is_empty());
        assert_eq!(fix_cites("It rose [2][7] .", 3), "It rose [2].");
        assert_eq!(fix_cites("A [Reuters] note [1, 9].", 3), "A [Reuters] note [1].");
        assert_eq!(uncited("The Fed held rates on Tuesday [1][2]. A [Reuters] note [3]."), "The Fed held rates on Tuesday. A [Reuters] note.");
        let s = Story { id: 0, section: "world".into(), rank: 0, headline: "Fed holds".into(), summary: p.into(), differ: String::new(), made_at: Default::default(), dek: String::new() };
        assert_eq!(dek_of(&s), "The Fed held rates on Tuesday.");
    }

    #[test]
    fn outlets_and_text() {
        assert_eq!(outlet("https://www.reuters.com/world/x"), "Reuters");
        assert_eq!(outlet("https://apnews.com/article/y"), "AP News");
        assert_eq!(outlet("https://news.bbc.co.uk/a"), "BBC");
        assert_eq!(outlet("https://www.example.org:8080/z"), "example.org");
        assert_eq!(plain_text("\u{201c}Hi\u{201d} \u{2014} it\u{2019}s  here\u{2026}\n\nTwo"), "\"Hi\" - it's here...\n\nTwo");
        assert_eq!(clip("one two three four", 12), "one two...");
        assert!(walled("Bloomberg - Are you a robot? We've detected unusual activity"));
        assert!(!walled(&"The Fed held rates on Tuesday and said more. ".repeat(10)));
        assert_eq!(json_in("Sure:\n```json\n{\"a\": [1]}\n```").unwrap()["a"][0], 1);
    }

    #[test]
    fn stories_votes_and_pruning() {
        let st = NewsStore::open_memory().unwrap();
        let at = |d: u32, h: u32| NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
        let story = |h: &str, when| Story { id: 0, section: "world".into(), rank: 0, headline: h.into(), summary: "It happened [1].".into(), differ: String::new(), made_at: when, dek: String::new() };
        let src = Source { n: 1, outlet: "AP News".into(), url: "https://apnews.com/a".into(), ..Default::default() };
        let a = st.add_story(&story("Old news", at(1, 9)), std::slice::from_ref(&src)).unwrap();
        let b = st.add_story(&story("Volcano erupts", at(10, 9)), &[Source { url: "https://apnews.com/b".into(), ..src.clone() }]).unwrap();
        assert_eq!(st.stories_between(at(10, 0), at(11, 0)).unwrap().len(), 1);
        assert_eq!(st.days().unwrap(), vec![at(10, 0).date(), at(1, 0).date()]);
        assert_eq!(st.search(&["VOLCANO".into()], 10).unwrap()[0].id, b);
        assert!(st.urls_since(at(5, 0)).unwrap().contains("https://apnews.com/b"));
        let p = |kind, text: &str| Push { user_id: 2, handle: "Ann".into(), kind, text: text.into(), at: at(10, 10) };
        st.add_push(b, &p(1, "")).unwrap();
        st.add_push(b, &p(-1, "")).unwrap();
        st.add_push(b, &p(0, "wild")).unwrap();
        // One vote a caller: the down took the up's place.
        assert_eq!(st.tallies().unwrap()[&b], (-1, 1));
        assert_eq!(st.prune(at(5, 0)).unwrap(), 1);
        assert!(st.story(a).unwrap().is_none() && st.sources(a).unwrap().is_empty());
        assert_eq!(st.remove_pushes(b, 2).unwrap(), 2);
        st.mark_read(2, &[b, b], at(10, 11)).unwrap();
        assert_eq!(st.read_ids(2).unwrap(), HashSet::from([b]));
        assert!(st.read_ids(3).unwrap().is_empty());
    }

    #[test]
    fn newest_edition_and_its_lead() {
        let st = NewsStore::open_memory().unwrap();
        let at = |h: u32, m: u32| NaiveDate::from_ymd_opt(2026, 11, 12).unwrap().and_hms_opt(h, m, 0).unwrap();
        let story = |h: &str, sec: &str, rank, when| Story { id: 0, section: sec.into(), rank, headline: h.into(), summary: String::new(), differ: String::new(), made_at: when, dek: String::new() };
        st.add_story(&story("Last run's lead", "world", 0, at(6, 0)), &[]).unwrap();
        st.add_story(&story("Tech two", "tech", 1, at(9, 0)), &[]).unwrap();
        st.add_story(&story("World one", "world", 1, at(9, 5)), &[]).unwrap();
        st.add_story(&story("World zero", "world", 0, at(9, 10)), &[]).unwrap();
        let ed = st.newest_edition().unwrap();
        assert_eq!(ed.len(), 3, "6:00 is another edition");
        let mut cfg = crate::config::Config::default();
        cfg.news.enabled = true;
        let state = crate::tests::state();
        state.news.add_story(&story("World zero", "world", 0, at(9, 10)), &[]).unwrap();
        state.news.add_story(&story("Tech one", "tech", 0, at(9, 0)), &[]).unwrap();
        state.set_config(cfg);
        let s = Session::for_test(state, crate::user::User { id: 2, ..Default::default() });
        // The first section in the run's order (world comes before tech).
        assert_eq!(s.mci_value(*b"NH").unwrap(), "World zero");
    }
}
