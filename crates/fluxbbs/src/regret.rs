//! New: taking back what you posted, kindly but on the record. `d` on
//! your own post in a public area offers, in turn:
//!
//! - **take it back**: within `take_back_minutes` of posting, while nobody
//!   has commented, voted or replied, it goes as if never posted;
//! - **retract**: everyone else sees "[retracted by its author]" in its
//!   place; you and the moderators still see what it said, and you can
//!   undo it;
//! - **burn**: gone for good, for `burn_points` file points (free on a
//!   board without file points), after a second "are you sure".
//!
//! Nobody is told. A stub of a taken-back or burned post stays, deleted,
//! for moderators until a pack; echomail already sent stays out there.

use std::io;

use crate::messages::{AreaRef, io_err};
use crate::msgbase::Message;
use crate::session::Session;

impl Session {
    /// `d` on your own post. True if it changed.
    pub(crate) async fn regret(&mut self, a: &AreaRef, m: &Message) -> io::Result<bool> {
        let cfg = self.state.config().messages.clone();
        let now = crate::clock::now().naive_local();
        let retracted = self.state.msgs.retracted(m.id).map_err(io_err)?.is_some();
        self.newline();
        if !self.comments_only(a) {
            self.say_line("RegretEcho").await?;
        }
        let fresh = cfg.take_back_minutes > 0 && (now - m.posted).num_minutes() < cfg.take_back_minutes as i64;
        if fresh && !retracted && !m.deleted && self.state.msgs.responses(m.id).map_err(io_err)? == 0 {
            self.say("RegretTakeBack").await?;
            if self.yes_no(false).await? {
                return self.burn_post(a, m, "took back").await;
            }
            return Ok(false);
        }
        let points = if self.state.config().files.use_file_points { cfg.burn_points } else { 0 };
        let burn = if points > 0 { self.strings().fmt("RegretBurnCost", &[&points.to_string()]) } else { String::new() };
        let ask = self.strings().fmt(if retracted { "RegretAskRetracted" } else { "RegretAsk" }, &[&burn]);
        self.show(&ask).await?;
        let key = self.read_choice().await?;
        self.newline();
        match key.upper() {
            Some('R') if !retracted => {
                self.say("RegretSureRetract").await?;
                if !self.yes_no(false).await? {
                    return Ok(false);
                }
                let when = self.local(now).format("%m/%d/%y").to_string();
                let subject = self.strings().get("RetractedSubject").to_string();
                let body = self.strings().get("RetractedBody").replace("%S1", &when);
                self.state.msgs.retract(m, &subject, &body, now).map_err(io_err)?;
                tracing::info!("{}: retracted #{} in {}", self.user.handle, m.num, a.area.name);
                Ok(true)
            }
            Some('U') if retracted => {
                self.state.msgs.unretract(m.id).map_err(io_err)?;
                tracing::info!("{}: undid the retraction of #{} in {}", self.user.handle, m.num, a.area.name);
                Ok(true)
            }
            Some('B') => {
                if points > 0 && self.user.file_points < points {
                    let s = self.strings().fmt("RegretBurnPoor", &[&points.to_string(), &self.user.file_points.to_string()]);
                    self.show_line(&s).await?;
                    self.prompt_key().await?;
                    return Ok(false);
                }
                self.say("RegretSureBurn").await?;
                if !self.yes_no(false).await? {
                    return Ok(false);
                }
                if points > 0 {
                    self.user.file_points -= points;
                    self.save_own_user().map_err(io_err)?;
                }
                self.burn_post(a, m, "burned").await
            }
            _ => Ok(false),
        }
    }

    async fn burn_post(&mut self, a: &AreaRef, m: &Message, how: &str) -> io::Result<bool> {
        let when = self.local(crate::clock::now().naive_local()).format("%m/%d/%y").to_string();
        let subject = self.strings().get("BurnedSubject").to_string();
        let body = self.strings().get("BurnedBody").replace("%S1", &when);
        if let Some(name) = self.state.msgs.burn(m.id, &subject, &body).map_err(io_err)? {
            let path = crate::attach::attachment_path(&self.state, m.id, &name);
            if let Err(e) = std::fs::remove_file(&path)
                && e.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!("removing attachment {}: {e}", path.display());
            }
        }
        tracing::info!("{}: {how} #{} in {}", self.user.handle, m.num, a.area.name);
        self.say_line("RegretGone").await?;
        self.prompt_key().await?;
        Ok(true)
    }
}
