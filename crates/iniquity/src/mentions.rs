//! New: @mentions. A public post naming `@handle` tells that caller: at
//! once with a node message if they're on, and at their next logon (`MW`)
//! or with `M@`. Quoted lines don't count, nor naming yourself.

use crate::BbsState;
use crate::msgbase::Message;
use crate::nodes::NodeEvent;

/// The names written after `@` in `lines`, each with up to two more
/// words after it (handles can have spaces), longest first. Quoted lines
/// (`XX> ...`) are skipped.
pub fn candidates(lines: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for line in lines {
        let t = line.trim_start();
        if t.split_once('>').is_some_and(|(q, _)| q.len() <= 3 && q.chars().all(|c| c.is_alphanumeric())) {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if c != '@' || (i > 0 && chars[i - 1].is_alphanumeric()) {
                continue;
            }
            let rest: String = chars[i + 1..].iter().collect();
            let words: Vec<&str> = rest.split_whitespace().take(3).collect();
            if words.is_empty() || !words[0].starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            // Words until one ends in punctuation, which ends the name.
            let mut taken: Vec<String> = Vec::new();
            for w in words {
                let cleaned = w.trim_end_matches(|c: char| ".,!?:;)'\"".contains(c));
                if !cleaned.is_empty() {
                    taken.push(cleaned.to_string());
                }
                if cleaned != w {
                    break;
                }
            }
            let names: Vec<String> = (1..=taken.len()).rev().map(|n| taken[..n].join(" ")).collect();
            out.push(names);
        }
    }
    out
}

/// Record who `m` mentions, and tell those who are on. `area` is the
/// area's name; `from_node` the poster's node (0 for none).
pub fn record(state: &BbsState, m: &Message, area: &str, from_node: usize) {
    if m.private || m.deleted || m.id == 0 {
        return;
    }
    let mut told: Vec<u32> = Vec::new();
    for names in candidates(&m.lines) {
        let Some(u) = names.iter().find_map(|n| state.users.find(n, false).ok().flatten()) else { continue };
        if u.id == m.from.user_id || told.contains(&u.id) {
            continue;
        }
        told.push(u.id);
        if let Err(e) = state.msgs.add_mention(u.id, m.id) {
            tracing::warn!("mention of {}: {e}", u.handle);
            continue;
        }
        tracing::info!("{} mentioned {} in {area}", m.from.name, u.handle);
        if let Some(node) = state.nodes.find_user(&u.handle) {
            let text = state.strings().fmt("MentionNotice", &[area, &m.subject]);
            let from = if m.anonymous { "anonymous".to_string() } else { m.from.name.clone() };
            state.nodes.send(node, NodeEvent::Message { from: from_node, user: from, text, broadcast: false });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn finds_names() {
        let c = candidates(&lines(&["hey @phrac, look at this", "and @Dark Angel too", " PH> @quoted no", "mail me@example.com"]));
        assert_eq!(c[0], ["phrac"]);
        assert_eq!(c[1], ["Dark Angel too", "Dark Angel", "Dark"]);
        assert_eq!(c.len(), 2);
        // A comma ends the name.
        let c = candidates(&lines(&["@phrac, and others"]));
        assert_eq!(c[0], ["phrac"]);
    }
}
