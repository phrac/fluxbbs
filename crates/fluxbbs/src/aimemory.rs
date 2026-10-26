//! New: the board's short memory of a caller, for the AI chat (and the
//! menus' hints) to suit them next time. A session notes, coarsely, what
//! the caller did: the boards they read and posted in, the file areas they
//! took files from, the doors they played and what they asked the AI. At
//! logoff the AI folds that into a few lines about their interests: never
//! personal details, message text, times or habits, nothing about anyone
//! else. Callers see it, and can clear it, in their profile (`Q` in user
//! config). Callers with the bot flag (`B`) have none.

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
const MAX_CHARS: usize = 600;

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
    if !cfg.enabled || u.id == 0 || is_bot(&u) || seen.is_empty() {
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
        (what to point out, what they'd enjoy). Write at most five short lines of plain text about their interests on this board: \
        which boards and topics they read and post in, what kinds of files they take, which games they play, what they ask about. \
        Keep it coarse and friendly. Never include personal details, message contents, times, how often or how long they call, \
        or anything that would feel like being watched; nothing about other people. Keep what still holds from the old memory and \
        drop what's stale. Reply with the memory only.";
    let old = if u.ai_profile.trim().is_empty() { "(none yet)".to_string() } else { u.ai_profile.clone() };
    let prompt = format!("Caller: {}\n\nOld memory:\n{old}\n\nThis call:\n{}", u.handle, seen.describe());
    let text = complete(cfg, system, &prompt).await?;
    let text = crate::editor::strip_colours(&text);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).take(6).collect();
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
    let client = reqwest::Client::builder().timeout(Duration::from_secs(cfg.timeout_seconds.max(10))).build()?;
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let mut body = json!({
        "model": cfg.model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
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
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
    Ok(without_thinking(&text))
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
        let mut used = self.state.ai_usage.lock().unwrap();
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
    use super::*;

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
