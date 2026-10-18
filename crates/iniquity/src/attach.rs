//! Files attached to messages (`ATTACH.PAS`). The unit survives but
//! nothing in this release's source calls it; the manual's strings and
//! the config's attach path and ACS settings say what was meant: a
//! message may carry one file, offered for download when it's read.
//!
//! Files are kept under `[paths] attach` as `ID_NAME`, by the message's
//! row id, so packing an area (which renumbers messages) doesn't lose
//! them; packing also removes files whose message is gone.

use std::io;
use std::path::PathBuf;

use chrono::Local;
use iniquity_term::Slot;

use crate::BbsState;
use crate::files::commas;
use crate::input::ReadOpts;
use crate::messages::AreaRef;
use crate::msgbase::{Attachment, Message};
use crate::session::Session;
use crate::transfer::safe_name;

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// Where a message's file lives.
pub fn attachment_path(state: &BbsState, msg_id: i64, name: &str) -> PathBuf {
    let cfg = state.config();
    cfg.resolve(&cfg.paths.attach).join(format!("{msg_id}_{name}"))
}

/// Delete the files of attachments whose messages have gone (after an
/// area is packed or deleted).
pub fn remove_orphans(state: &BbsState) -> anyhow::Result<usize> {
    let gone = state.msgs.orphan_attachments()?;
    for (id, name) in &gone {
        let path = attachment_path(state, *id, name);
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != io::ErrorKind::NotFound {
                tracing::warn!("removing attachment {}: {e}", path.display());
            }
    }
    Ok(gone.len())
}

impl Session {
    /// May the caller attach a file to a message in this area?
    pub fn may_attach(&self, a: &AreaRef) -> bool {
        let m = &self.state.config().messages;
        let acs = if a.num == 0 { m.acs_attach_email.clone() } else { m.acs_attach_public.clone() };
        self.acs(&acs)
    }

    /// `atAttachFile`: upload a file (a SysOp may name one on disk
    /// instead) and attach it to `m`, replacing any it has.
    pub async fn attach_file(&mut self, m: &Message) -> io::Result<()> {
        self.newline();
        if let Some(old) = self.state.msgs.attachment(m.id).map_err(io_err)? {
            self.say("AtchAlreadyInc").await?;
            if !self.yes_no(false).await? {
                return Ok(());
            }
            let _ = std::fs::remove_file(attachment_path(&self.state, m.id, &old.name));
            self.state.msgs.remove_attachment(m.id).map_err(io_err)?;
        }
        let mut local = String::new();
        if self.is_sysop() {
            self.show("|U5-- |U4Enter filename to attach to message |U2[|U3Enter|U2/|U1Upload|U2]|U5: |U6").await?;
            local = self.read_string(ReadOpts::new(70)).await?;
            self.newline();
        }
        // An upload is moved into place; a file the SysOp named is copied.
        let uploaded = local.trim().is_empty();
        let source: PathBuf = if uploaded {
            let Some(proto) = self.select_protocol(false).await? else { return Ok(()) };
            let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("atch{}", self.node.number()));
            let _ = std::fs::remove_dir_all(&temp);
            let got = self.receive_files(&proto, &temp).await?;
            let Some(first) = got.into_iter().next() else { return Ok(()) };
            first
        } else {
            let p = PathBuf::from(local.trim());
            if !p.is_file() {
                self.say_line("FaFileNotThere").await?;
                return Ok(());
            }
            p
        };
        let Some(name) = source.file_name().and_then(|n| safe_name(n.to_string_lossy().as_bytes())) else { return Ok(()) };
        let dest = attachment_path(&self.state, m.id, &name);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if !uploaded || std::fs::rename(&source, &dest).is_err() {
            std::fs::copy(&source, &dest)?;
            if uploaded {
                let _ = std::fs::remove_file(&source);
            }
        }
        let size = std::fs::metadata(&dest)?.len();
        self.say("AtchAskDesc").await?;
        let desc = self.read_string(ReadOpts::new(70)).await?;
        self.newline();
        let a = Attachment { name: name.clone(), desc, size, uploaded: Local::now().date_naive() };
        self.state.msgs.set_attachment(m.id, &a).map_err(io_err)?;
        tracing::info!("{}: attached {name} ({size} bytes) to message {} in {}", self.user.handle, m.num, m.area);
        let s = self.strings().fmt("AtchFileAttached", &[&name]);
        self.show_line(&s).await
    }

    /// `atAskDownload`: after a message is shown, offer its file.
    pub async fn offer_attachment(&mut self, m: &Message) -> io::Result<()> {
        let Some(a) = self.state.msgs.attachment(m.id).map_err(io_err)? else { return Ok(()) };
        if self.screen.where_x() != 1 {
            self.newline();
        }
        self.say_line("AtchIsAttached").await?;
        let s = self.strings().fmt("AtchShowDesc", &[&a.desc]);
        self.show_line(&s).await?;
        let s = self.strings().fmt("AtchShowInfo", &[&a.name, &commas(a.size)]);
        self.show_line(&s).await?;
        self.say("AtchAskDownload").await?;
        if !self.yes_no(false).await? {
            return Ok(());
        }
        let path = attachment_path(&self.state, m.id, &a.name);
        if !path.is_file() {
            self.say_line("FaFileNotThere").await?;
            return Ok(());
        }
        let Some(proto) = self.select_protocol(false).await? else { return Ok(()) };
        // Sent under its own name, not the id-prefixed one it's stored as.
        let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("atch{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp)?;
        let named = temp.join(&a.name);
        if std::fs::hard_link(&path, &named).is_err() {
            std::fs::copy(&path, &named)?;
        }
        let ok = self.send_files(&proto, &[named]).await;
        let _ = std::fs::remove_dir_all(&temp);
        let ok = ok?;
        self.set_slot(Slot::Text);
        self.say_line(if ok.first() == Some(&true) { "FaXferSuccess" } else { "FaXferFailed" }).await
    }
}
