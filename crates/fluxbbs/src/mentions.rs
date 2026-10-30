//! New: @mentions. A public post naming `@handle` tells that caller: at
//! once with a node message if they're on, and at their next logon (`MW`)
//! or with `M@`. Quoted lines don't count, nor naming yourself.

use chrono::{Datelike, Timelike};

use crate::BbsState;
use crate::acs::{self, AcsEnv};
use crate::msgareas::MsgArea;
use crate::msgbase::Message;
use crate::nodes::NodeEvent;
use crate::user::User;

/// Group names a co-SysOp (`[acs] cosysop`) may write after `@`: everyone
/// who can read the area, the co-SysOps among them, or those on now.
pub const GROUPS: [&str; 3] = ["all", "mods", "here"];

/// `u`'s ACS environment as if they were on now (they may not be).
fn env_for(state: &BbsState, u: &User) -> AcsEnv {
    let now = crate::clock::now();
    let level_req = state.level(u.level).map_or(0, |l| l.post_call as u32);
    AcsEnv {
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
        calls: u.total_calls,
        ..AcsEnv::default()
    }
}

/// Could `u`, not necessarily on, read `area`? Its ACS, in their current
/// conference or any other they may join; never a passworded area.
pub fn may_read(state: &BbsState, u: &User, area: &MsgArea) -> bool {
    if !area.password.is_empty() {
        return false;
    }
    let env = env_for(state, u);
    if acs::check(&area.acs, &env) {
        return true;
    }
    state.areas().confs.iter().enumerate().any(|(i, c)| {
        let env = AcsEnv { conf_msg: i as u32 + 1, ..env.clone() };
        acs::check(&c.acs, &env) && acs::check(&area.acs, &env)
    })
}

/// May `u` mention a group (`@all`...)? Co-SysOps and up.
pub fn may_group(state: &BbsState, u: &User) -> bool {
    acs::check(&state.config().acs.cosysop, &env_for(state, u))
}

/// Who a group mention in `area` reaches (not the poster), or none for
/// a name that isn't a group.
fn group_members(state: &BbsState, group: &str, area: &MsgArea, poster: u32) -> Vec<User> {
    let readers = |us: Vec<User>| -> Vec<User> { us.into_iter().filter(|u| !u.deleted && u.id != poster && may_read(state, u, area)).collect() };
    match group.to_ascii_lowercase().as_str() {
        "all" => readers(state.users.list().unwrap_or_default()),
        "mods" => readers(state.users.list().unwrap_or_default()).into_iter().filter(|u| may_group(state, u)).collect(),
        "here" => readers(state.nodes.online().into_iter().filter_map(|(_, n)| state.users.find(&n.user, false).ok().flatten()).collect()),
        _ => Vec::new(),
    }
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
    let poster = state.users.get(m.from.user_id).ok().flatten();
    let groups_ok = poster.as_ref().is_some_and(|p| may_group(state, p));
    let mut told: Vec<u32> = Vec::new();
    for names in candidates(&m.lines) {
        // A group (the one-word name is the last), for those allowed one.
        let word = names.last().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        if groups_ok && GROUPS.contains(&word.as_str()) {
            for u in group_members(state, &word, &where_, m.from.user_id) {
                tell(state, m, area, from_node, &u, &mut told);
            }
            continue;
        }
        // "@5" isn't user number 5.
        let Some(u) = names.iter().filter(|n| !n.chars().all(|c| c.is_ascii_digit())).find_map(|n| state.users.find(n, false).ok().flatten()) else { continue };
        if u.id == m.from.user_id {
            continue;
        }
        // Not a word to someone who can't read it.
        if !may_read(state, &u, &where_) {
            continue;
        }
        tell(state, m, area, from_node, &u, &mut told);
    }
}

/// Note that `m` mentions `u` and say so if they're on, once a post.
fn tell(state: &BbsState, m: &Message, area: &str, from_node: usize, u: &User, told: &mut Vec<u32>) {
    if told.contains(&u.id) {
        return;
    }
    told.push(u.id);
    // Told already (the post was edited since): not again.
    match state.msgs.add_mention(u.id, m.id) {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            tracing::warn!("mention of {}: {e}", u.handle);
            return;
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
