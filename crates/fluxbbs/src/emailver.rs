//! New: callers' email addresses, confirmed by a code (the `email`
//! component; `[email]`, off to start). A caller gives an address, the
//! board mails them a six-digit code over SMTP, and they type it back;
//! a confirmed address can raise their level (`verified_level`), which
//! is how a board gates posting and the like on it. It runs as a logon
//! step (`*O`, `email`), from a menu (`UV`) and from the caller's
//! configuration. Addresses are seen only by the caller and SysOps.
//!
//! Codes are kept hashed, good for `code_minutes`, five wrong tries and
//! they're void; at most one goes out every two minutes and five a day
//! to a caller. An address confirmed on one account can't be confirmed
//! on another.

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use chrono::NaiveDateTime;
use rusqlite::{Connection, OptionalExtension, params};

use crate::config::Email as Cfg;
use crate::input::ReadOpts;
use crate::session::Session;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS email_codes (
    user    INTEGER PRIMARY KEY,
    address TEXT NOT NULL,
    hash    TEXT NOT NULL,
    expires TEXT NOT NULL,
    tries   INTEGER NOT NULL DEFAULT 0,
    sent    TEXT NOT NULL,
    sends   INTEGER NOT NULL DEFAULT 1,
    day     TEXT NOT NULL
);
";

/// Wrong codes before one is void.
const TRIES: u32 = 5;
/// Codes a caller may have sent in a day.
const SENDS_A_DAY: u32 = 5;
/// Seconds between codes.
const RESEND_AFTER: i64 = 120;
const STAMP: &str = "%Y-%m-%d %H:%M:%S";

/// Whether a code went out.
#[derive(Debug, PartialEq)]
pub enum Issue {
    Sent,
    /// Too soon after the last: seconds to wait.
    TooSoon(i64),
    TooMany,
}

/// What a typed code turned out to be.
#[derive(Debug, PartialEq)]
pub enum Check {
    Right,
    /// Wrong, with this many tries left.
    Wrong(u32),
    Expired,
    /// No code out for that address.
    NoCode,
    /// Too many wrong tries: it's void.
    Void,
}

pub struct CodeStore {
    db: Mutex<Connection>,
}

impl CodeStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = crate::sysdata::open_db(path)?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// May a code go out to `user` now? If so, record `code` (hashed) for
    /// `address`, good for `minutes`.
    pub fn issue(&self, user: u32, address: &str, code: &str, now: NaiveDateTime, minutes: u32) -> anyhow::Result<Issue> {
        let db = self.db.lock().unwrap();
        let today = now.date().to_string();
        let last: Option<(String, u32, String)> =
            db.query_row("SELECT sent, sends, day FROM email_codes WHERE user = ?1", params![user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let mut sends = 1;
        if let Some((sent, n, day)) = last {
            if day == today {
                if n >= SENDS_A_DAY {
                    return Ok(Issue::TooMany);
                }
                sends = n + 1;
            }
            if let Ok(sent) = NaiveDateTime::parse_from_str(&sent, STAMP) {
                let since = (now - sent).num_seconds();
                if since < RESEND_AFTER {
                    return Ok(Issue::TooSoon(RESEND_AFTER - since));
                }
            }
        }
        let hash = Argon2::default().hash_password(code.as_bytes()).map_err(|e| anyhow::anyhow!("hashing a code: {e}"))?.to_string();
        let expires = (now + chrono::Duration::minutes(minutes.max(1) as i64)).format(STAMP).to_string();
        db.execute(
            "INSERT INTO email_codes (user, address, hash, expires, tries, sent, sends, day) VALUES (?1,?2,?3,?4,0,?5,?6,?7)
             ON CONFLICT(user) DO UPDATE SET address=?2, hash=?3, expires=?4, tries=0, sent=?5, sends=?6, day=?7",
            params![user, address, hash, expires, now.format(STAMP).to_string(), sends, today],
        )?;
        Ok(Issue::Sent)
    }

    /// Is `code` the one out to `user` for `address`? Right clears it.
    pub fn check(&self, user: u32, address: &str, code: &str, now: NaiveDateTime) -> anyhow::Result<Check> {
        let db = self.db.lock().unwrap();
        let row: Option<(String, String, String, u32)> = db
            .query_row("SELECT address, hash, expires, tries FROM email_codes WHERE user = ?1", params![user], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let Some((to, hash, expires, tries)) = row else { return Ok(Check::NoCode) };
        if !to.eq_ignore_ascii_case(address) || hash.is_empty() {
            return Ok(Check::NoCode);
        }
        if tries >= TRIES {
            return Ok(Check::Void);
        }
        if NaiveDateTime::parse_from_str(&expires, STAMP).is_ok_and(|e| now > e) {
            return Ok(Check::Expired);
        }
        let right = PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(code.trim().as_bytes(), &h).is_ok());
        if right {
            db.execute("UPDATE email_codes SET hash = '' WHERE user = ?1", params![user])?;
            return Ok(Check::Right);
        }
        db.execute("UPDATE email_codes SET tries = tries + 1 WHERE user = ?1", params![user])?;
        let left = TRIES - (tries + 1);
        Ok(if left == 0 { Check::Void } else { Check::Wrong(left) })
    }

    /// Is a code out to `user` for `address` that's still good?
    pub fn live(&self, user: u32, address: &str, now: NaiveDateTime) -> bool {
        let db = self.db.lock().unwrap();
        let row: Option<(String, String, String, u32)> = db
            .query_row("SELECT address, hash, expires, tries FROM email_codes WHERE user = ?1", params![user], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .ok();
        row.is_some_and(|(to, hash, expires, tries)| {
            to.eq_ignore_ascii_case(address)
                && !hash.is_empty()
                && tries < TRIES
                && NaiveDateTime::parse_from_str(&expires, STAMP).is_ok_and(|e| now <= e)
        })
    }
}

/// Does `s` look like an email address a code could go to?
pub fn valid_address(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 3 || s.len() > 100 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((local, domain)) = s.split_once('@') else { return false };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && domain.split('.').all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && s.parse::<lettre::Address>().is_ok()
}

/// Six digits.
fn new_code() -> String {
    use rand::RngExt;
    format!("{:06}", rand::rng().random_range(0..1_000_000u32))
}

/// Mail `body` to `to` through the board's SMTP server.
async fn send(cfg: &Cfg, to: &str, subject: String, body: String) -> anyhow::Result<()> {
    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
    if cfg.smtp_host.trim().is_empty() || cfg.from.trim().is_empty() {
        anyhow::bail!("[email] smtp_host and from must be set");
    }
    let msg = Message::builder()
        .from(cfg.from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)?;
    let host = cfg.smtp_host.trim();
    let mut smtp = match cfg.smtp_security.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
        "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
    }
    .port(cfg.smtp_port)
    .timeout(Some(Duration::from_secs(20)));
    if !cfg.smtp_user.is_empty() {
        let pw = std::env::var(&cfg.smtp_password_env).unwrap_or_default();
        smtp = smtp.credentials(Credentials::new(cfg.smtp_user.clone(), pw));
    }
    smtp.build().send(msg).await?;
    Ok(())
}

impl Session {
    /// `UV`: the caller's email address and its code. As a logon step
    /// (`UV logon`) it asks for an address if there's none (insisting
    /// with `required`) and for the code if it isn't confirmed, and is
    /// quiet when it is; from a menu or their configuration, it shows
    /// the address and offers to change or confirm it.
    pub async fn email_check(&mut self, param: &str) -> io::Result<()> {
        let cfg = self.state.config().email.clone();
        if !cfg.enabled || self.user.id == 0 {
            return Ok(());
        }
        let logon = param.trim().eq_ignore_ascii_case("logon");
        if logon && !self.user.mail_address.is_empty() && !self.user.mail_verified.is_empty() {
            return Ok(());
        }
        if !logon {
            if self.user.mail_address.is_empty() {
                self.say_line("EmStatusNone").await?;
            } else {
                let status = if self.user.mail_verified.is_empty() {
                    self.strings().get("EmUnconfirmed").to_string()
                } else {
                    self.strings().fmt("EmConfirmedOn", &[&self.user.mail_verified])
                };
                let s = self.strings().fmt("EmStatus", &[&self.user.mail_address, &status]);
                self.show_line(&s).await?;
                self.say("EmMenu").await?;
                let key = self.read_key().await?;
                self.newline();
                match key.upper() {
                    Some('C') => self.user.mail_address.clear(),
                    Some('V') if self.user.mail_verified.is_empty() => {}
                    _ => return Ok(()),
                }
            }
        }
        // An address, if there's none.
        if self.user.mail_address.is_empty() && !self.email_ask_address(&cfg, logon).await? {
            return Ok(());
        }
        if !self.user.mail_verified.is_empty() {
            return Ok(());
        }
        // A code, unless one's still out for this address.
        let now = crate::clock::now().naive_local();
        if !self.state.email.live(self.user.id, &self.user.mail_address, now) && !self.email_send(&cfg).await? {
            return Ok(());
        }
        loop {
            self.say("EmCodePrompt").await?;
            let typed = self.read_string(ReadOpts::new(6)).await?;
            self.newline();
            let typed = typed.trim().to_string();
            if typed.is_empty() || self.escaped {
                self.escaped = false;
                return self.say_line("EmLater").await;
            }
            match typed.to_ascii_uppercase().as_str() {
                "R" => {
                    if !self.email_send(&cfg).await? {
                        return Ok(());
                    }
                    continue;
                }
                "C" => {
                    self.user.mail_address.clear();
                    if !self.email_ask_address(&cfg, logon).await? || !self.email_send(&cfg).await? {
                        return Ok(());
                    }
                    continue;
                }
                _ => {}
            }
            let now = crate::clock::now().naive_local();
            match self.state.email.check(self.user.id, &self.user.mail_address, &typed, now).map_err(io::Error::other)? {
                Check::Right => return self.email_confirmed(&cfg).await,
                Check::Wrong(left) => {
                    let s = self.strings().fmt("EmWrong", &[&left.to_string()]);
                    self.show_line(&s).await?;
                }
                Check::Expired | Check::NoCode => {
                    self.say_line("EmExpired").await?;
                    if !self.email_send(&cfg).await? {
                        return Ok(());
                    }
                }
                Check::Void => return self.say_line("EmVoid").await,
            }
        }
    }

    /// Ask for an address and keep it (unconfirmed). False if they gave
    /// none and may go without.
    async fn email_ask_address(&mut self, cfg: &Cfg, logon: bool) -> io::Result<bool> {
        loop {
            self.say(if cfg.required { "EmAsk" } else { "EmAskOptional" }).await?;
            let a = self.read_string(ReadOpts::new(60)).await?;
            self.newline();
            let a = a.trim().to_string();
            if a.is_empty() {
                self.escaped = false;
                if cfg.required && logon {
                    self.say_line("EmRequired").await?;
                    continue;
                }
                return Ok(false);
            }
            if !valid_address(&a) {
                self.say_line("EmBadAddress").await?;
                continue;
            }
            if self.state.users.mail_taken(&a, self.user.id).map_err(io::Error::other)? {
                self.say_line("EmInUse").await?;
                continue;
            }
            self.user.mail_address = a;
            self.user.mail_verified.clear();
            self.save_user();
            return Ok(true);
        }
    }

    /// Mail a fresh code; false (having said why) if it didn't go.
    async fn email_send(&mut self, cfg: &Cfg) -> io::Result<bool> {
        let code = new_code();
        let now = crate::clock::now().naive_local();
        let to = self.user.mail_address.clone();
        match self.state.email.issue(self.user.id, &to, &code, now, cfg.code_minutes).map_err(io::Error::other)? {
            Issue::Sent => {}
            Issue::TooSoon(secs) => {
                let s = self.strings().fmt("EmWait", &[&secs.to_string()]);
                self.show_line(&s).await?;
                return Ok(false);
            }
            Issue::TooMany => {
                self.say_line("EmTooMany").await?;
                return Ok(false);
            }
        }
        let board = self.state.config().bbs.name.clone();
        let mins = cfg.code_minutes.max(1).to_string();
        // Mail isn't a screen: no color codes, and none of the markers
        // that keep a filled-in value from being read as codes.
        let plain = |s: String| fluxbbs_term::codes::unlit(&crate::editor::strip_colours(&s));
        let subject = plain(self.strings().fmt("EmailSubject", &[&board]));
        let body = plain(self.strings().fmt("EmailBody", &[&board, &self.user.handle, &code, &mins]));
        match send(cfg, &to, subject, body).await {
            Ok(()) => {
                tracing::info!("{}: emailed a code", self.user.handle);
                let s = self.strings().fmt("EmSent", &[&to, &mins]);
                self.show_line(&s).await?;
                Ok(true)
            }
            Err(e) => {
                tracing::warn!("{}: emailing a code: {e:#}", self.user.handle);
                self.say_line("EmSendFailed").await?;
                Ok(false)
            }
        }
    }

    /// The address is theirs: note the day, raise their level if the
    /// board says to (never lower it), and save.
    async fn email_confirmed(&mut self, cfg: &Cfg) -> io::Result<()> {
        self.user.mail_verified = crate::clock::now().date_naive().to_string();
        let s = self.strings().fmt("EmVerified", &[&self.user.mail_address]);
        self.show_line(&s).await?;
        if let Some(key) = cfg.verified_level.trim().chars().next().map(|c| c.to_ascii_uppercase())
            && let Some(level) = self.state.level(key)
            && level.sl > self.user.sl
        {
            self.user.apply_level(key, &level);
            self.say_line("EmLevelUp").await?;
        }
        self.save_user();
        tracing::info!("{}: email address confirmed", self.user.handle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, STAMP).unwrap()
    }

    #[test]
    fn addresses_are_checked() {
        for good in ["a@b.co", "phrac@disfluxbbs.com", "first.last+bbs@mail.example.org"] {
            assert!(valid_address(good), "{good}");
        }
        for bad in ["", "nope", "a@b", "@b.co", "a@@b.co", "a b@c.co", "a@b..co", "a@-.co\n"] {
            assert!(!valid_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_code_confirms_once_and_then_is_gone() {
        let s = CodeStore::open_memory().unwrap();
        let t = at("2026-10-26 12:00:00");
        assert_eq!(s.issue(7, "a@b.co", "123456", t, 30).unwrap(), Issue::Sent);
        assert!(s.live(7, "a@b.co", t));
        assert_eq!(s.check(7, "x@y.co", "123456", t).unwrap(), Check::NoCode, "not for another address");
        assert_eq!(s.check(7, "a@b.co", "000000", t).unwrap(), Check::Wrong(4));
        assert_eq!(s.check(7, "A@B.CO", "123456", t).unwrap(), Check::Right, "the address in any case");
        assert_eq!(s.check(7, "a@b.co", "123456", t).unwrap(), Check::NoCode, "used up");
        assert!(!s.live(7, "a@b.co", t));
    }

    #[test]
    fn codes_run_out_and_wrong_tries_void_them() {
        let s = CodeStore::open_memory().unwrap();
        let t = at("2026-10-26 12:00:00");
        s.issue(1, "a@b.co", "111111", t, 30).unwrap();
        assert_eq!(s.check(1, "a@b.co", "111111", at("2026-10-26 12:31:00")).unwrap(), Check::Expired);
        s.issue(2, "c@d.co", "222222", t, 30).unwrap();
        for left in (1..TRIES).rev() {
            assert_eq!(s.check(2, "c@d.co", "999999", t).unwrap(), Check::Wrong(left));
        }
        assert_eq!(s.check(2, "c@d.co", "999999", t).unwrap(), Check::Void);
        assert_eq!(s.check(2, "c@d.co", "222222", t).unwrap(), Check::Void, "even the right one, once void");
    }

    #[test]
    fn sending_is_rationed() {
        let s = CodeStore::open_memory().unwrap();
        let mut t = at("2026-10-26 09:00:00");
        assert_eq!(s.issue(3, "a@b.co", "1", t, 30).unwrap(), Issue::Sent);
        assert_eq!(s.issue(3, "a@b.co", "2", t + chrono::Duration::seconds(30), 30).unwrap(), Issue::TooSoon(90));
        for _ in 1..SENDS_A_DAY {
            t += chrono::Duration::minutes(5);
            assert_eq!(s.issue(3, "a@b.co", "3", t, 30).unwrap(), Issue::Sent);
        }
        t += chrono::Duration::minutes(5);
        assert_eq!(s.issue(3, "a@b.co", "4", t, 30).unwrap(), Issue::TooMany);
        assert_eq!(s.issue(3, "a@b.co", "5", at("2026-10-27 09:00:00"), 30).unwrap(), Issue::Sent, "a new day");
    }
}
