//! New: the board's short memory of a caller, for the AI chat (and the
//! menus' hints) to suit them next time. A session notes, coarsely, what
//! the caller did: the boards they read and posted in, the file areas they
//! took files from, the doors they played and what they asked the AI. At
//! logoff the AI folds that into a few lines about their interests: never
//! personal details, message text, times or habits, nothing about anyone
//! else. Nothing is kept unless the caller turns it on, in their profile
//! (`Q` in user config), where they see and clear it too. Callers with the
//! bot flag (`B`) have none.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
#[cfg(feature = "ai")]
use std::time::Duration;

#[cfg(feature = "ai")]
use serde_json::{Value, json};

use crate::BbsState;
#[cfg(feature = "ai")]
use crate::config::Ai;
use crate::user::User;

/// The flag that marks an account as a bot: no memory, no hints, the
/// board as it comes.
pub const BOT_FLAG: char = 'B';

/// The longest a memory gets.
#[cfg(feature = "ai")]
const MAX_CHARS: usize = 2000;

// Kept on every call; only the AI (`ai`) reads it.
#[cfg_attr(not(feature = "ai"), allow(dead_code))]
/// What a caller did this call, as much as the memory needs.
#[derive(Debug, Default, Clone)]
pub struct Seen {
    pub read: BTreeSet<String>,
    pub posted: BTreeSet<String>,
    /// File area name, and how many files from it.
    pub downloads: BTreeMap<String, u32>,
    pub doors: BTreeSet<String>,
    /// The first few things they asked the AI, cut short.
    pub asked: Vec<String>,
}

#[cfg_attr(not(feature = "ai"), allow(dead_code))]
impl Seen {
    pub fn is_empty(&self) -> bool {
        self.read.is_empty() && self.posted.is_empty() && self.downloads.is_empty() && self.doors.is_empty() && self.asked.is_empty()
    }

    pub fn asked(&mut self, text: &str) {
        if self.asked.len() < 5 {
            self.asked.push(text.chars().take(160).collect());
        }
    }

    fn describe(&self) -> String {
        let list = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
        let mut out = Vec::new();
        if !self.read.is_empty() {
            out.push(format!("- read in: {}", list(&self.read)));
        }
        if !self.posted.is_empty() {
            out.push(format!("- posted in: {}", list(&self.posted)));
        }
        if !self.downloads.is_empty() {
            let d: Vec<String> = self.downloads.iter().map(|(a, n)| format!("{a} ({n})")).collect();
            out.push(format!("- took files from: {}", d.join(", ")));
        }
        if !self.doors.is_empty() {
            out.push(format!("- played: {}", list(&self.doors)));
        }
        if !self.asked.is_empty() {
            out.push(format!("- asked the AI: {}", self.asked.join(" | ")));
        }
        out.join("\n")
    }
}

/// Is this account a bot (flag `B`)?
pub fn is_bot(u: &User) -> bool {
    u.flags.contains(&BOT_FLAG)
}

/// After a call: fold what `seen` says into `u`'s memory, in the
/// background (the caller has gone; nobody waits on it).
#[cfg(feature = "ai")]
pub fn remember(state: Arc<BbsState>, u: User, seen: Seen) {
    let cfg = state.config().ai.clone();
    if !cfg.enabled || u.id == 0 || !u.ai_memory || is_bot(&u) || seen.is_empty() {
        return;
    }
    tokio::spawn(async move {
        match updated(&cfg, &u, &seen).await {
            Ok(memory) => {
                let mut mine = u.clone();
                mine.ai_profile = memory;
                if let Err(e) = state.users.merge(&u, &mine) {
                    tracing::warn!("{}: saving what the board remembers: {e}", u.handle);
                }
            }
            Err(e) => tracing::info!("{}: the board's memory wasn't updated: {e}", u.handle),
        }
    });
}

#[cfg(feature = "ai")]
async fn updated(cfg: &Ai, u: &User, seen: &Seen) -> anyhow::Result<String> {
    let system = "You keep a short memory of a caller to a bulletin board system, so the board's AI can suit them next time \
        (what to point out, what they'd enjoy). Write plain text about their interests on this board, at most 2000 characters in at most 20 short lines: \
        which boards and topics they read and post in, what kinds of files they take, which games they play, what they ask about \
        and the things they enjoy talking about. \
        Keep it coarse and friendly. Never include personal details, message contents, times, how often or how long they call, \
        or anything that would feel like being watched; nothing about other people. Keep what still holds from the old memory and \
        drop what's stale. Reply with the memory only.";
    let old = if u.ai_profile.trim().is_empty() { "(none yet)".to_string() } else { u.ai_profile.clone() };
    let prompt = format!("Caller: {}\n\nOld memory:\n{old}\n\nThis call:\n{}", u.handle, seen.describe());
    let text = complete(cfg, system, &prompt).await?;
    let text = crate::editor::strip_colours(&text);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).take(20).collect();
    let mut memory = lines.join("\n");
    if memory.chars().count() > MAX_CHARS {
        memory = memory.chars().take(MAX_CHARS).collect();
    }
    if memory.is_empty() {
        anyhow::bail!("the model said nothing");
    }
    Ok(memory)
}

/// One answer from the model, not streamed, without its thinking.
#[cfg(feature = "ai")]
pub async fn complete(cfg: &Ai, system: &str, prompt: &str) -> anyhow::Result<String> {
    ask(cfg, system, json!(prompt)).await
}

/// New: one answer that may look things up on the way: the web search
/// and page reading the AI chat has (`[ai] web_search`, `read_pages`), a
/// few rounds of them at most, then the answer. Without web search it's
/// `complete`.
#[cfg(feature = "ai")]
pub async fn complete_with_web(cfg: &Ai, system: &str, prompt: &str) -> anyhow::Result<String> {
    const ROUNDS: usize = 3;
    if !cfg.web_search {
        return complete(cfg, system, prompt).await;
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(cfg.timeout_seconds.max(10))).build()?;
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let mut tools = vec![json!({"type": "function", "function": {
        "name": "web_search",
        "description": "Search the web. Returns the top results: title, address and a snippet.",
        "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": "What to search for"}}, "required": ["query"]}
    }})];
    if cfg.read_pages {
        tools.push(json!({"type": "function", "function": {
            "name": "read_page",
            "description": "Read a web page (from search results) as plain text, when the snippet isn't enough.",
            "parameters": {"type": "object", "properties": {"url": {"type": "string", "description": "The page's address"}}, "required": ["url"]}
        }}));
    }
    let system = format!("{system} You can search the web with the web_search tool when the question needs facts you don't have; say briefly where they came from.");
    let mut messages = vec![json!({"role": "system", "content": system}), json!({"role": "user", "content": prompt})];
    let key = (!cfg.api_key_env.is_empty()).then(|| std::env::var(&cfg.api_key_env).ok()).flatten();
    let brave = (!cfg.search_key_env.is_empty()).then(|| std::env::var(&cfg.search_key_env).ok()).flatten();
    for round in 0..=ROUNDS {
        let mut body = json!({"model": cfg.model, "messages": messages, "stream": false, "max_tokens": cfg.max_tokens});
        if !cfg.reasoning_effort.is_empty() {
            body["reasoning_effort"] = json!(cfg.reasoning_effort);
        }
        if round < ROUNDS {
            body["tools"] = Value::Array(tools.clone());
        } else {
            messages.push(json!({"role": "system", "content": "No more searches. Answer now from what you have."}));
            body["messages"] = Value::Array(messages.clone());
        }
        let mut req = client.post(&url).json(&body);
        if let Some(k) = &key {
            req = req.bearer_auth(k);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("{}", resp.status());
        }
        let v: Value = resp.json().await?;
        let msg = &v["choices"][0]["message"];
        let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() || round == ROUNDS {
            let text = without_thinking(msg["content"].as_str().unwrap_or(""));
            if v["choices"][0]["finish_reason"] == "length" {
                return Ok(whole_sentences(&text));
            }
            return Ok(text);
        }
        messages.push(json!({"role": "assistant", "content": msg["content"].as_str().unwrap_or(""), "tool_calls": calls}));
        for c in &calls {
            let name = c["function"]["name"].as_str().unwrap_or_default();
            let args = serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("{}")).unwrap_or_default();
            let result = match name {
                "web_search" => {
                    let q = args["query"].as_str().unwrap_or_default();
                    tracing::info!("ai: search {q:?}");
                    crate::aichat::web_search(&client, &cfg.search_url, brave.as_deref(), q)
                        .await
                        .unwrap_or_else(|e| format!("Search failed ({e}). Don't search again for this; answer from what you know, and say so."))
                }
                "read_page" if cfg.read_pages => {
                    let u = args["url"].as_str().unwrap_or_default();
                    tracing::info!("ai: read {u:?}");
                    crate::aichat::read_page(u).await.unwrap_or_else(|e| format!("Couldn't read it ({e})."))
                }
                _ => format!("No tool called {name}."),
            };
            messages.push(json!({"role": "tool", "tool_call_id": c["id"], "content": result}));
        }
    }
    anyhow::bail!("the AI never answered")
}

/// New: one answer about a picture (a PNG), shown to the model beside the
/// prompt. The model has to see images (vega's does).
#[cfg(feature = "ai")]
pub async fn complete_seeing(cfg: &Ai, system: &str, prompt: &str, png: &[u8]) -> anyhow::Result<String> {
    let url = format!("data:image/png;base64,{}", base64(png));
    ask(cfg, system, json!([{"type": "text", "text": prompt}, {"type": "image_url", "image_url": {"url": url}}])).await
}

#[cfg(feature = "ai")]
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            out.push(if k <= c.len() { A[(n >> (18 - 6 * k)) as usize & 63] as char } else { '=' });
        }
    }
    out
}

#[cfg(feature = "ai")]
async fn ask(cfg: &Ai, system: &str, content: Value) -> anyhow::Result<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(cfg.timeout_seconds.max(10))).build()?;
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let mut body = json!({
        "model": cfg.model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": content}],
        "stream": false,
        "max_tokens": cfg.max_tokens,
    });
    if !cfg.reasoning_effort.is_empty() {
        body["reasoning_effort"] = json!(cfg.reasoning_effort);
    }
    let mut req = client.post(&url).json(&body);
    if let Some(k) = (!cfg.api_key_env.is_empty()).then(|| std::env::var(&cfg.api_key_env).ok()).flatten() {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("{}", resp.status());
    }
    let v: Value = resp.json().await?;
    let text = without_thinking(v["choices"][0]["message"]["content"].as_str().unwrap_or(""));
    // Out of tokens mid-sentence: up to the last sentence it finished.
    if v["choices"][0]["finish_reason"] == "length" {
        tracing::warn!("ai: an answer ran out of tokens ({} of them)", cfg.max_tokens);
        return Ok(whole_sentences(&text));
    }
    Ok(text)
}

/// `s` up to the end of its last whole sentence (all of it if none ends).
#[cfg(feature = "ai")]
fn whole_sentences(s: &str) -> String {
    let s = s.trim_end();
    if s.ends_with(['.', '!', '?']) {
        return s.to_string();
    }
    // A stop followed by a space: not the point in 54.5.
    let end = s.char_indices().rfind(|&(i, c)| matches!(c, '.' | '!' | '?') && s[i + 1..].starts_with(char::is_whitespace));
    end.map_or(s, |(i, _)| &s[..=i]).to_string()
}

/// A reply less any `<think>...</think>` part.
#[cfg(feature = "ai")]
fn without_thinking(s: &str) -> String {
    let mut out = s.to_string();
    while let (Some(a), Some(b)) = (out.find("<think>"), out.find("</think>")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + "</think>".len(), "");
    }
    out.trim().to_string()
}

impl crate::session::Session {
    /// Count a message against the day's limit; false if it's used up.
    #[cfg_attr(not(feature = "ai"), allow(dead_code))]
    pub(crate) fn ai_count(&mut self, limit: u32) -> bool {
        if limit == 0 || self.is_sysop() {
            return true;
        }
        let key = (self.user.id, crate::clock::now().date_naive());
        let mut used = self.state.ai_usage.lock().unwrap_or_else(|e| e.into_inner());
        used.retain(|(_, day), _| *day == key.1);
        let n = used.entry(key).or_insert(0);
        if *n >= limit {
            return false;
        }
        *n += 1;
        true
    }
}

/// Without AI built in (`ai`), there's no memory to keep.
#[cfg(not(feature = "ai"))]
pub fn remember(_state: Arc<BbsState>, _u: User, _seen: Seen) {}

#[cfg(test)]
mod tests {
    #[cfg(feature = "ai")]
    #[test]
    fn base64_pads_as_it_should() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
        assert_eq!(super::base64(b""), "");
    }

    use super::*;

    #[cfg(feature = "ai")]
    #[test]
    fn cut_off_at_the_last_sentence() {
        assert_eq!(whole_sentences("Now in Firth it's 54.5 and overcast. Feels like 50 with a"), "Now in Firth it's 54.5 and overcast.");
        assert_eq!(whole_sentences("Grab a coat! Stay warm.\n"), "Grab a coat! Stay warm.");
        assert_eq!(whole_sentences("no stop at all"), "no stop at all");
    }

    #[test]
    fn a_call_described_and_thinking_dropped() {
        let mut s = Seen::default();
        assert!(s.is_empty());
        s.read.insert("hack/phreak".into());
        *s.downloads.entry("art scene".into()).or_default() += 2;
        s.asked("how do I download?");
        let d = s.describe();
        assert!(d.contains("read in: hack/phreak") && d.contains("art scene (2)") && d.contains("how do I download?"));
        #[cfg(feature = "ai")]
        assert_eq!(without_thinking("<think>hmm</think>Likes ANSI art."), "Likes ANSI art.");
    }
}
