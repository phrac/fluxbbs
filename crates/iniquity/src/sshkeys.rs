//! New: SSH public keys a caller keeps on their account, so their SSH
//! client logs them straight in (no bot check or login prompts). Keys are
//! `authorized_keys` lines; only the key itself counts, not the comment.
//! They work under the handle as the SSH user name, or another name the
//! caller picks (`ssh_user`), e.g. their own login so plain `ssh` works.

use std::io;

use iniquity_term::lit;
use russh::keys::{HashAlg, PublicKey};

use crate::input::{DIGITS, Key, ReadOpts};
use crate::messages::io_err;
use crate::session::Session;

/// Most keys a caller may keep.
pub const MAX_KEYS: usize = 8;
/// What an SSH user name may be made of.
const SSH_USER_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-";

/// A key from an `authorized_keys` line (`ssh-ed25519 AAAA... comment`).
pub fn parse(line: &str) -> Option<PublicKey> {
    PublicKey::from_openssh(line.trim()).ok()
}

/// Is `key` one of `stored`?
pub fn matches(stored: &[String], key: &PublicKey) -> bool {
    stored.iter().filter_map(|l| parse(l)).any(|k| k.key_data() == key.key_data())
}

/// `ed25519 SHA256:abc... comment`, for a list.
pub fn describe(line: &str) -> String {
    match parse(line) {
        Some(k) => {
            let alg = k.algorithm().as_str().trim_start_matches("ssh-").to_string();
            let comment = k.comment().as_str_lossy().trim();
            let fp = k.fingerprint(HashAlg::Sha256).to_string();
            if comment.is_empty() { format!("{alg} {fp}") } else { format!("{alg} {fp} {comment}") }
        }
        None => "(not a key)".into(),
    }
}

impl Session {
    /// `UK` (new), and K in user config: the caller's SSH keys, to list,
    /// add (pasted, as the `.pub` file's line) or delete. Saved at once.
    pub async fn ssh_keys_edit(&mut self) -> io::Result<()> {
        loop {
            self.screen.set_color(iniquity_term::Color::DEFAULT);
            self.clear_screen();
            self.say_line("SshKeysTitle").await?;
            let name = if self.user.ssh_user.is_empty() { self.user.handle.clone() } else { self.user.ssh_user.clone() };
            let how = self.strings().fmt("SshKeysHow", &[&name]);
            self.show_line(&how).await?;
            self.newline();
            if self.user.ssh_keys.is_empty() {
                self.say_line("SshKeysNone").await?;
            }
            for (i, k) in self.user.ssh_keys.clone().iter().enumerate() {
                let s = format!("|U2 {:>2}|U1. |U3{}", i + 1, lit(&describe(k)));
                self.show_line(&s).await?;
            }
            self.newline();
            self.say("SshKeysMenu").await?;
            let key = self.read_key().await?;
            self.newline();
            match key.upper() {
                Some('A') if self.user.ssh_keys.len() >= MAX_KEYS => {
                    self.say_line("SshKeysFull").await?;
                    self.prompt_key().await?;
                }
                Some('A') => {
                    self.say_line("SshKeysAsk").await?;
                    let line = self.read_paste().await?;
                    let reply = match parse(&line) {
                        _ if line.is_empty() => continue,
                        None => "SshKeysBad",
                        Some(k) if matches(&self.user.ssh_keys, &k) => "SshKeysDup",
                        Some(_) => {
                            self.user.ssh_keys.push(line.trim().to_string());
                            self.save_own_user().map_err(io_err)?;
                            tracing::info!("{}: added an SSH key", self.user.handle);
                            "SshKeysAdded"
                        }
                    };
                    self.say_line(reply).await?;
                    self.prompt_key().await?;
                }
                Some('U') => {
                    self.say("SshKeysAskUser").await?;
                    let name = self.read_string(ReadOpts::new(32).allowed(SSH_USER_CHARS).prefill(self.user.ssh_user.clone())).await?;
                    self.newline();
                    if let Some(why) = self.ssh_user_taken(&name)? {
                        self.say_line(why).await?;
                        self.prompt_key().await?;
                    } else {
                        self.user.ssh_user = name;
                        self.save_own_user().map_err(io_err)?;
                    }
                }
                Some('D') if !self.user.ssh_keys.is_empty() => {
                    let s = self.strings().fmt("SshKeysAskDelete", &[&self.user.ssh_keys.len().to_string()]);
                    self.show(&s).await?;
                    let n = self.read_string(ReadOpts::new(2).allowed(DIGITS)).await?;
                    self.newline();
                    if let Some(i) = n.parse::<usize>().ok().filter(|i| (1..=self.user.ssh_keys.len()).contains(i)) {
                        self.user.ssh_keys.remove(i - 1);
                        self.save_own_user().map_err(io_err)?;
                        tracing::info!("{}: deleted an SSH key", self.user.handle);
                    }
                }
                _ if matches!(key, Key::Esc | Key::Enter) || key.upper() == Some('Q') => return Ok(()),
                _ => {}
            }
            if self.hangup {
                return Ok(());
            }
        }
    }

    /// Why `name` can't be the caller's SSH user name, if it can't: someone
    /// else's handle, or their SSH user name.
    fn ssh_user_taken(&self, name: &str) -> io::Result<Option<&'static str>> {
        if name.is_empty() || name.eq_ignore_ascii_case(&self.user.handle) {
            return Ok(None);
        }
        let other = |u: Option<crate::user::User>| u.is_some_and(|u| u.id != self.user.id);
        if other(self.state.users.find(name, true).map_err(io_err)?) || other(self.state.users.by_ssh_user(name).map_err(io_err)?) {
            return Ok(Some("SshKeysUserTaken"));
        }
        Ok(None)
    }

    /// A pasted key: hundreds of characters, so the count is shown rather
    /// than the text.
    async fn read_paste(&mut self) -> io::Result<String> {
        let mut s = String::new();
        let x = self.screen.where_x();
        loop {
            match self.read_key().await? {
                Key::Enter if !s.is_empty() => break,
                Key::Enter | Key::Esc => {
                    s.clear();
                    break;
                }
                Key::Backspace => {
                    s.pop();
                }
                Key::Char(c @ 0x20..=0x7E) if s.len() < 4096 => s.push(c as char),
                _ => continue,
            }
            self.screen.pos_x(x);
            self.screen.clear_eol();
            let n = self.strings().fmt("SshKeysCount", &[&s.len().to_string()]);
            self.show(&n).await?;
        }
        self.newline();
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHNhpS0Cqe102WhHaezpZm/xsBKchG7LEE4gHJxt22gj phrac@rzn";

    #[test]
    fn keys_match_whatever_the_comment() {
        let k = parse(ED).unwrap();
        let other = ED.replace("phrac@rzn", "another comment");
        assert!(matches(&[other], &k));
        assert!(!matches(&["nonsense".into()], &k));
        assert!(parse("ssh-ed25519 notbase64").is_none());
        let d = describe(ED);
        assert!(d.starts_with("ed25519 SHA256:") && d.ends_with("phrac@rzn"), "{d}");
    }
}
