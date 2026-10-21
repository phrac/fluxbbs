//! New: @mentions. A public post naming `@handle` tells that caller: at
//! once with a node message if they're on, and at their next logon (`MW`)
//! or with `M@`. Quoted lines don't count, nor naming yourself.

use chrono::{Datelike, Local, Timelike};

use crate::BbsState;
use crate::acs::{self, AcsEnv};
use crate::msgareas::MsgArea;
use crate::msgbase::Message;
use crate::nodes::NodeEvent;
use crate::user::User;

/// Could `u`, not necessarily on, read `area`? Its ACS, in their current
/// conference or any other they may join; never a passworded area.
pub fn may_read(state: &BbsState, u: &User, area: &MsgArea) -> bool {
    if !area.password.is_empty() {
        return false;
    }
    let now = Local::now();
    let level_req = state.level(u.level).map_or(0, |l| l.post_call as u32);
    let env = AcsEnv {
        age: u.age(now.date_naive()),
        conf_msg: u.conf_msg,
        conf_file: u.conf_file,
        dsl: u.dsl,
        sl: u.sl,
        flags: u.flags.clone(),
        sex: u.sex,
        hour: now.hour(),
        area_code: u.area_code().to_string(),
        posts: u.posts,
        minutes_left: u.time_today,
        user_id: u.id,
        level: u.level,
        weekday: now.weekday().num_days_from_sunday(),
        minute_of_day: now.hour() * 60 + now.minute(),
        ratio_ok: (u.posts * 100).checked_div(u.total_calls).unwrap_or(0) >= level_req,
        ..AcsEnv::default()
    };
    if acs::check(&area.acs, &env) {
        return true;
    }
    state.areas().confs.iter().enumerate().any(|(i, c)| {
        let env = AcsEnv { conf_msg: i as u32 + 1, ..env.clone() };
        acs::check(&c.acs, &env) && acs::check(&area.acs, &env)
    })
}

/// The names written after `@` in `lines`, each with up to two more
/// words after it (handles can have spaces), longest first. Quoted lines
/// (`XX> ...`) are skipped.
pub fn candidates(lines: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for line in lines {
        let t = line.trim_start();
        // As `quote_lines` writes them: initials (`??` for anonymous).
        if t.split_once('>').is_some_and(|(q, _)| q.chars().count() <= 6 && !q.contains(' ')) {
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
    let Some(where_) = state.areas().areas.iter().find(|a| a.file.eq_ignore_ascii_case(&m.area)).cloned() else { return };
    let mut told: Vec<u32> = Vec::new();
    for names in candidates(&m.lines) {
        // "@5" isn't user number 5.
        let Some(u) = names.iter().filter(|n| !n.chars().all(|c| c.is_ascii_digit())).find_map(|n| state.users.find(n, false).ok().flatten()) else { continue };
        if u.id == m.from.user_id || told.contains(&u.id) {
            continue;
        }
        // Not a word to someone who can't read it.
        if !may_read(state, &u, &where_) {
            continue;
        }
        told.push(u.id);
        // Told already (the post was edited since): not again.
        match state.msgs.add_mention(u.id, m.id) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!("mention of {}: {e}", u.handle);
                continue;
            }
        }
        tracing::info!("{} mentioned {} in {area}", m.from.name, u.handle);
        if let Some(node) = state.nodes.find_user(&u.handle) {
            let text = state.strings().fmt("MentionNotice", &[area, &m.subject]);
            let from = if m.anonymous { "anonymous".to_string() } else { m.from.name.clone() };
            // An anonymous writer's node would give them away.
            let from_node = if m.anonymous { 0 } else { from_node };
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
