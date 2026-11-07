//! New: feedback from the matrix (`>F`), after ENiGMA½'s pre-auth
//! feedback (MIT). Someone who hasn't logged in leaves the SysOp a note:
//! any name they like, a way to reach them if they want, a subject and
//! the text, in the usual editor. It's email to the SysOp from a
//! visitor, tied to no account: the name shows with "(visitor)", the
//! text starts with where it came from, and it can't be answered (there's
//! no one to send to). The bot check comes before the matrix, and the
//! per-address limits and bans hold as for any call; on top of them,
//! only so many a day from an address and from everyone.

use std::io;

use crate::editor::EditCtx;
use crate::input::ReadOpts;
use crate::msgbase::{Message, Party};
use crate::nodes::NodeEvent;
use crate::rumors::clean;
use crate::session::Session;

/// The note a visitor's party carries, which marks it as one.
pub const VISITOR: &str = "(visitor)";
/// Feedback a day from one address, and from everyone.
pub const PER_ADDRESS: u32 = 2;
pub const PER_DAY: u32 = 20;
/// How long the name, the way to reach them and the subject may be, and
/// the most lines the text keeps.
pub const NAME_LEN: usize = 30;
pub const REACH_LEN: usize = 60;
pub const SUBJECT_LEN: usize = 40;
pub const MAX_LINES: usize = 100;

/// Is this party a visitor from the matrix (no account to answer)?
pub fn is_visitor(p: &Party) -> bool {
    p.user_id == 0 && p.note == VISITOR
}

/// The party a visitor's note is from: the name typed, marked.
pub fn visitor(name: &str) -> Party {
    let shown = format!("{name} {VISITOR}");
    Party { user_id: 0, alias: shown.clone(), real_name: shown.clone(), name: shown, note: VISITOR.into() }
}

/// The lines over the visitor's text: who they say they are, how to
/// reach them, and where it came from.
pub fn header(name: &str, reach: &str, from: &str, when: &str) -> Vec<String> {
    let mut h = vec![
        "* Left at the login matrix by a visitor, not logged in: the name is".to_string(),
        "* whatever they typed, and it can't be answered here.".to_string(),
        format!("  Name     : {name}"),
    ];
    if !reach.is_empty() {
        h.push(format!("  Reach at : {reach}"));
    }
    h.push(format!("  Came from: {from}"));
    h.push(format!("  When     : {when}"));
    h.push(String::new());
    h
}

impl Session {
    /// `>F`: a note to the SysOp (the parameter: who, by name or user
    /// number; user 1 if blank) from someone not logged in. Logged in,
    /// it's plain email to them (`EE`).
    pub async fn matrix_feedback(&mut self, param: &str) -> io::Result<()> {
        let to_name = if param.trim().is_empty() { "1" } else { param.trim() };
        if self.logged_in {
            return self.enter_email(to_name, "").await;
        }
        self.newline();
        let Some(to) = self.state.users.find(to_name, false).map_err(io::Error::other)? else {
            tracing::warn!("matrix feedback: no user {to_name:?} to send it to");
            return self.note_say("EmailUserUnknown").await;
        };
        let ip = self.caller.remote.map(|a| a.ip().to_string()).unwrap_or_default();
        let day = crate::clock::now().naive_local() - chrono::Duration::days(1);
        let (mine, all) = self.state.sys.feedback_since(&ip, day).map_err(io::Error::other)?;
        if mine >= PER_ADDRESS || all >= PER_DAY {
            tracing::info!("{ip}: matrix feedback refused (limit)");
            return self.note_say("VisitorFbLimit").await;
        }
        self.say_line("VisitorFbIntro").await?;
        self.say("VisitorFbName").await?;
        let name = clean(&crate::editor::strip_colours(&self.read_string(ReadOpts::new(NAME_LEN)).await?));
        self.newline();
        if name.is_empty() {
            return Ok(());
        }
        self.say("VisitorFbReach").await?;
        let reach = clean(&crate::editor::strip_colours(&self.read_string(ReadOpts::new(REACH_LEN)).await?));
        self.newline();
        self.say("MsgAskTitle").await?;
        let subject = clean(&self.read_string(ReadOpts::new(SUBJECT_LEN).prefill("Feedback")).await?);
        self.newline();
        if subject.is_empty() {
            return self.note_say("MsgNoTitle").await;
        }
        let from = visitor(&name);
        let now = crate::clock::now().naive_local();
        let ctx = EditCtx {
            subject,
            from: from.name.clone(),
            to: to.handle.clone(),
            date: now,
            anonymous: false,
            can_anonymous: false,
            can_autosig: false,
            autosig: false,
            reply: None,
            auto_quote: false,
            text: Vec::new(),
        };
        self.node.update(|n| n.activity = "Leaving feedback".into());
        let done = self.edit_message(ctx).await?;
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        let Some(done) = done else { return Ok(()) };
        let mut body = done.lines;
        body.truncate(MAX_LINES);
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        if body.iter().all(|l| l.trim().is_empty()) {
            return Ok(());
        }
        // Counted only now, once there's something to send; checked
        // again, as another node may have taken the last place.
        if !self.state.sys.take_feedback(&ip, PER_ADDRESS, PER_DAY).map_err(io::Error::other)? {
            return self.note_say("VisitorFbLimit").await;
        }
        let came = format!("{} by {}, node {}", if ip.is_empty() { "?" } else { &ip }, self.caller.transport, self.node.number());
        let mut lines = header(&name, &reach, &came, &now.format("%Y-%m-%d %H:%M").to_string());
        lines.extend(body);
        let subject: String = clean(&done.subject).chars().take(SUBJECT_LEN).collect();
        let a = self.email_area();
        let mut m = Message {
            id: 0,
            area: a.key().to_string(),
            num: 0,
            from,
            to: Party { user_id: to.id, alias: to.handle.clone(), real_name: to.real_name.clone(), name: to.handle.clone(), note: to.note.clone() },
            subject: if subject.is_empty() { "Feedback".into() } else { subject },
            posted: now,
            deleted: false,
            private: true,
            anonymous: false,
            replies: 0,
            sig_pos: 0,
            tag: 0,
            reply_to: 0,
            lines,
        };
        self.state.msgs.add(&mut m).map_err(io::Error::other)?;
        self.bump(crate::sysdata::Stat::Email);
        tracing::info!("{ip}: matrix feedback to {} from {name:?}: {:?}", to.handle, m.subject);
        // Said as mail is: in their box at logon, and now if they're on.
        if let Some(node) = self.state.nodes.find_user(&to.handle) {
            let text = self.strings().fmt("VisitorFbNotice", &[&name, &m.subject]);
            self.state.nodes.send(node, NodeEvent::Notice { text });
        }
        let s = self.strings().fmt("EmailSent", &[&to.handle]);
        self.note(s).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visitors_are_marked_and_tied_to_no_one() {
        let p = visitor("SysOp");
        assert_eq!((p.user_id, p.name.as_str()), (0, "SysOp (visitor)"));
        assert!(is_visitor(&p));
        // Netmail and echomail senders are user 0 too, but not visitors.
        assert!(!is_visitor(&Party { user_id: 0, alias: "x".into(), real_name: "x".into(), name: "Bob".into(), note: "46:1/100".into() }));
        assert!(!is_visitor(&Party { user_id: 3, note: VISITOR.into(), ..p }));
    }

    #[test]
    fn the_header_says_where_it_came_from() {
        let h = header("Bob", "", "1.2.3.4 by telnet, node 2", "2026-11-07 14:30");
        assert!(h.iter().any(|l| l.contains("1.2.3.4 by telnet, node 2")));
        assert!(!h.iter().any(|l| l.contains("Reach at")));
        assert!(header("Bob", "bob@example.com", "", "").iter().any(|l| l.ends_with("bob@example.com")));
        assert_eq!(h.last().map(String::as_str), Some(""));
    }
}
