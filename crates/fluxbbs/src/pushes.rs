//! New: pushes, after PTT's 推/噓/→. Under a message, callers leave a
//! one-line reaction, up (`+`), down (`-`) or a comment (`>`), with `x`
//! in the message centre's reader; the classic reader shows them. A
//! message's score (ups less downs) shows in the thread list, which sorts
//! by latest, hot or top, and the Hot list gathers what's being pushed
//! across the board. Pushes stay here: FTN and QWK never carry them.

use std::collections::HashMap;
use std::io;

use chrono::NaiveDateTime;
use fluxbbs_term::codes::{lit, resize};

use crate::input::{Key, ReadOpts};
use crate::messages::{AreaRef, io_err};
use crate::msgbase::{Message, Push};
use crate::session::Session;

/// Characters in a push's text.
pub const PUSH_WIDTH: usize = 50;
/// A score this high shows as HOT.
const HOT_SCORE: i32 = 25;
/// How far back the hot sort looks.
pub const HOT_HOURS: i64 = 48;
/// Seconds between one caller's pushes.
pub(crate) const PUSH_GAP: i64 = 5;

/// A score as lists show it: `+4`, `0`, `HOT`, `X2`; blank for none.
pub fn score_label(score: Option<i32>) -> String {
    match score {
        None => String::new(),
        Some(n) if n >= HOT_SCORE => "HOT".into(),
        Some(n) if n > 0 => format!("+{n}"),
        Some(0) => "0".into(),
        Some(n) => format!("X{}", -n),
    }
}

/// Each message's heat: its pushes since `HOT_HOURS` ago, votes one
/// each way and comments a half, fading with age.
pub fn heats(recent: &[(i64, i8, NaiveDateTime)], now: NaiveDateTime) -> HashMap<i64, f64> {
    let mut h = HashMap::new();
    for &(id, kind, at) in recent {
        let age = (now - at).num_minutes() as f64 / 60.0;
        let fade = (1.0 - age / HOT_HOURS as f64).clamp(0.0, 1.0);
        let w = match kind {
            1 => 1.0,
            -1 => -1.0,
            _ => 0.5,
        };
        *h.entry(id).or_insert(0.0) += w * fade;
    }
    h
}

impl Session {
    pub(crate) fn may_push(&self, a: &AreaRef, m: &Message) -> bool {
        !a.is_email() && !m.private && self.acs(&a.area.post_acs) && !self.banned_from(a)
    }

    /// The lines under a message: a rule with its score, then its pushes.
    pub(crate) fn push_lines(&self, m: &Message) -> Vec<String> {
        let pushes = self.state.msgs.pushes(m.id).unwrap_or_default();
        if pushes.is_empty() {
            return Vec::new();
        }
        let score: i32 = pushes.iter().map(|p| p.kind as i32).sum();
        let s = self.strings();
        let mut out = vec![String::new(), s.fmt("PushHead", &[&score_label(Some(score))])];
        for p in &pushes {
            let name = match p.kind {
                1 => "PushUp",
                -1 => "PushDown",
                _ => "PushComment",
            };
            let when = self.local(p.at).format("%m/%d %H:%M").to_string();
            out.push(s.fmt(name, &[&lit(&resize(&p.handle, 12)), &lit(&resize(&p.text, PUSH_WIDTH)), &when]));
        }
        out
    }

    /// The reader's bottom line, cleared for a prompt.
    fn bar_line(&mut self) {
        let rows = self.screen.rows();
        self.screen.goto(1, rows);
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.screen.clear_eol();
    }

    /// A word on the bottom line, until a key.
    async fn bar_note(&mut self, name: &str) -> io::Result<bool> {
        let s = self.strings().get(name).to_string();
        self.bar_text(&s).await
    }

    async fn bar_text(&mut self, text: &str) -> io::Result<bool> {
        self.bar_line();
        self.show(text).await?;
        self.read_key().await?;
        Ok(false)
    }

    /// `x` in the reader: push `m` up or down, or comment on it; the
    /// area's sponsor can take a caller's pushes off. True if anything
    /// changed.
    /// `x` in the reader, or (`comment`) `r`, straight to a comment.
    pub(crate) async fn push_prompt(&mut self, a: &AreaRef, m: &Message, comment: bool) -> io::Result<bool> {
        if !self.may_push(a, m) {
            return self.bar_note("PushNotHere").await;
        }
        let moderator = self.is_sponsor(a) || self.is_cosysop();
        // Echomail: votes, but no comments, which the network never sees.
        let echo = !self.comments_only(a);
        let kind: i8 = if comment {
            0
        } else {
            self.bar_line();
            let ask = match (echo, moderator) {
                (true, true) => "PushAskEchoSponsor",
                (true, false) => "PushAskEcho",
                (false, true) => "PushAskSponsor",
                (false, false) => "PushAsk",
            };
            // New: `$` tips the author, where tips are on.
            let tips = self.tip_author(m).is_ok() && !self.is_me(&m.from);
            let s = self.strings();
            let ask = s.fmt(ask, &[if tips { s.get("PushTipKey") } else { "" }]);
            self.show(&ask).await?;
            match self.read_key().await? {
                Key::Char(b'+') => 1,
                Key::Char(b'-') => -1,
                Key::Char(b'>') => 0,
                Key::Char(b'$') if tips => return self.push_tip(m).await,
                k if moderator && k.upper() == Some('D') => return self.push_remove(m).await,
                _ => return Ok(false),
            }
        };
        if kind == 0 && echo {
            return self.bar_note("PushEchoComment").await;
        }
        if kind != 0 && self.is_me(&m.from) {
            return self.bar_note(if echo { "PushOwnEcho" } else { "PushOwn" }).await;
        }
        let now = crate::clock::now().naive_local();
        if let Some(last) = self.state.msgs.last_push_at(self.user.id).map_err(io_err)?
            && (now - last).num_seconds() < PUSH_GAP
        {
            return self.bar_note("PushTooFast").await;
        }
        self.bar_line();
        self.say(match kind {
            1 => "PushTextUp",
            -1 => "PushTextDown",
            _ => "PushTextComment",
        })
        .await?;
        let mut text = self.read_string(ReadOpts::new(PUSH_WIDTH)).await?;
        // New: `/l` picks a file to link to, then the words go round it.
        if kind == 0 && text.trim().eq_ignore_ascii_case("/l") {
            let Some(link) = self.pick_file_link().await? else { return Ok(true) };
            self.bar_line();
            self.say("PushTextComment").await?;
            let max = PUSH_WIDTH.max(link.chars().count() + 1);
            text = self.read_string(ReadOpts::new(max).prefill(format!("{link} "))).await?;
        }
        let text = crate::editor::strip_colours(&text).trim().to_string();
        if kind == 0 && text.is_empty() {
            return Ok(false);
        }
        if kind == 0 {
            self.seen.posted.insert(crate::editor::strip_colours(&a.area.name));
        }
        let p = Push { id: 0, msg_id: m.id, user_id: self.user.id, handle: self.user.handle.clone(), kind, text, at: now };
        self.state.msgs.add_push(&p).map_err(io_err)?;
        tracing::info!("{}: pushed message {} in {} ({kind})", self.user.handle, m.num, a.area.name);
        if kind == 1 {
            self.pay_upvote(m);
        }
        Ok(true)
    }

    /// `$` at the push prompt (new): tip the post's author some of our
    /// file points.
    async fn push_tip(&mut self, m: &Message) -> io::Result<bool> {
        let to = match self.tip_author(m) {
            Ok(u) => u,
            Err(why) => return self.bar_text(&why).await,
        };
        self.bar_line();
        self.sync_points();
        let s = self.strings().fmt("TipAskAmount", &[&to.handle, &self.user.file_points.to_string()]);
        self.show(&s).await?;
        let n: u32 = self.read_string(ReadOpts::new(5).allowed(crate::input::DIGITS)).await?.trim().parse().unwrap_or(0);
        if n == 0 {
            return Ok(false);
        }
        let said = self.tip(&to, n, Some(m))?;
        self.bar_text(&said).await
    }

    /// A sponsor's `d` at the push prompt: take one caller's pushes off.
    async fn push_remove(&mut self, m: &Message) -> io::Result<bool> {
        self.bar_line();
        self.say("PushRemoveWho").await?;
        let mut opts = ReadOpts::new(36);
        opts.complete_users = true;
        let who = self.read_string(opts).await?;
        if who.trim().is_empty() {
            return Ok(false);
        }
        let pushes = self.state.msgs.pushes(m.id).map_err(io_err)?;
        let Some(uid) = pushes.iter().find(|p| p.handle.eq_ignore_ascii_case(who.trim())).map(|p| p.user_id) else {
            return self.bar_note("PushRemoveNone").await;
        };
        let n = self.state.msgs.remove_pushes(m.id, uid).map_err(io_err)?;
        tracing::info!("{}: took {n} pushes by {} off message {}", self.user.handle, who.trim(), m.num);
        Ok(n > 0)
    }

    /// Finger's karma line: the score and the votes (`%S4`: how many posts
    /// they're on).
    pub(crate) fn karma_text(&self, user_id: u32) -> io::Result<String> {
        let (up, down, posts) = self.state.msgs.karma(user_id).map_err(io_err)?;
        let net = up as i64 - down as i64;
        let net = if net > 0 { format!("+{net}") } else { net.to_string() };
        Ok(self.strings().fmt("FingerKarmaText", &[&net, &up.to_string(), &down.to_string(), &posts.to_string()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_and_heat() {
        assert_eq!(score_label(None), "");
        assert_eq!(score_label(Some(0)), "0");
        assert_eq!(score_label(Some(4)), "+4");
        assert_eq!(score_label(Some(25)), "HOT");
        assert_eq!(score_label(Some(-2)), "X2");
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 23).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let h = |n: i64| now - chrono::Duration::hours(n);
        let heat = heats(&[(1, 1, h(0)), (1, 0, h(0)), (2, 1, h(24)), (3, -1, h(0)), (4, 1, h(60))], now);
        assert_eq!(heat[&1], 1.5);
        assert_eq!(heat[&2], 0.5);
        assert_eq!(heat[&3], -1.0);
        assert_eq!(heat[&4], 0.0);
    }
}
