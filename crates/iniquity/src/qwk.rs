//! QWK offline mail (`QwkMail.pas`): `QD` packs new messages for an
//! offline reader, `QU` posts the replies it sends back.
//!
//! A packet (NAME.QWK) is a ZIP of CONTROL.DAT (the board and its
//! conferences), DOOR.ID, MESSAGES.DAT (128-byte blocks: a header block,
//! then the text, lines ended by 0xE3), an NNN.NDX index per conference,
//! and the welcome, news and goodbye screens. Conferences are message
//! area numbers. Replies come back as NAME.REP holding NAME.MSG, in the
//! same block format.
//!
//! Changes from the original: email is included as conference 0 (the
//! original left mail out), with PERSONAL.NDX for messages to the caller;
//! "include your own messages?" does what it asks (the original's test was
//! inverted); dates have four-digit years; areas with a password are left
//! out; newscan pointers move to the newest message packed, not to "now",
//! so nothing posted while the packet downloads is skipped; and long reply
//! lines are wrapped, not cut at 79.

use std::io::{self, Read, Write};
use std::path::Path;

use chrono::{Local, NaiveDateTime};
use iniquity_term::codes::resize;
use iniquity_term::cp437;

use crate::messages::AreaRef;
use crate::msgbase::{Message, Party};
use crate::session::Session;

/// QWK's line end in message text.
const EOL: u8 = 0xE3;
const BLOCK: usize = 128;
/// Message lines are kept to this width.
const WIDTH: usize = 79;

/// `qwkLong2BasicReal`: a number in Microsoft Binary Format, as the
/// index files hold record numbers.
pub fn msbin(n: u32) -> [u8; 4] {
    if n == 0 {
        return [0; 4];
    }
    let (mut v, mut exp) = (n as u64, 152u32);
    while v >= 0x100_0000 {
        v >>= 1;
        exp += 1;
    }
    while v < 0x80_0000 {
        v <<= 1;
        exp -= 1;
    }
    ((v as u32 & 0x7F_FFFF) | (exp << 24)).to_le_bytes()
}

/// `qwkBasicReal2Long` (readers use it; here only to test `msbin`).
#[cfg(test)]
fn from_msbin(b: [u8; 4]) -> u32 {
    let v = u32::from_le_bytes(b);
    let exp = (v >> 24) as i32 - 152;
    if v >> 24 == 0 {
        return 0;
    }
    let m = (v & 0x7F_FFFF) | 0x80_0000;
    if exp < 0 { m >> -exp } else { m << exp }
}

/// Put `s` into a space-filled field.
fn field(buf: &mut [u8], at: usize, len: usize, s: &str) {
    let b = cp437::encode(s);
    let n = b.len().min(len);
    buf[at..at + n].copy_from_slice(&b[..n]);
}

fn text(buf: &[u8]) -> String {
    cp437::decode(buf).trim_matches(|c| c == ' ' || c == '\0').to_string()
}

/// One message's header block (`tQwkMsgRec`).
pub struct Header<'a> {
    pub private: bool,
    pub num: u32,
    pub posted: NaiveDateTime,
    pub to: &'a str,
    pub from: &'a str,
    pub subject: &'a str,
    /// Blocks including this one.
    pub blocks: usize,
    pub conf: u16,
    /// Position in the packet, from 1.
    pub seq: u16,
}

impl Header<'_> {
    pub fn encode(&self) -> [u8; BLOCK] {
        let mut b = [b' '; BLOCK];
        b[0] = if self.private { b'*' } else { b'-' };
        field(&mut b, 1, 7, &self.num.to_string());
        field(&mut b, 8, 8, &self.posted.format("%m-%d-%y").to_string());
        field(&mut b, 16, 5, &self.posted.format("%H:%M").to_string());
        field(&mut b, 21, 25, &self.to.to_uppercase());
        field(&mut b, 46, 25, &self.from.to_uppercase());
        field(&mut b, 71, 25, self.subject);
        field(&mut b, 108, 8, "0");
        field(&mut b, 116, 6, &self.blocks.to_string());
        b[122] = 0xE1; // active
        b[123..125].copy_from_slice(&self.conf.to_le_bytes());
        b[125..127].copy_from_slice(&self.seq.to_le_bytes());
        b
    }
}

/// `qdWriteMsg`: message text as QWK blocks. A 0xE3 in the text would end
/// the line, so it becomes 0xD2, as the original did.
pub fn body_blocks(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for l in lines {
        out.extend(cp437::encode(l).into_iter().map(|c| if c == EOL { 0xD2 } else { c }));
        out.push(EOL);
    }
    let pad = (BLOCK - out.len() % BLOCK) % BLOCK;
    out.resize(out.len() + pad, b' ');
    out
}

/// What goes in CONTROL.DAT.
pub struct Control<'a> {
    pub bbs_name: &'a str,
    pub location: &'a str,
    pub phone: &'a str,
    pub sysop: &'a str,
    pub packet: &'a str,
    pub when: NaiveDateTime,
    pub user: &'a str,
    pub messages: usize,
    pub confs: &'a [(u16, String)],
    pub welcome: &'a str,
    pub news: &'a str,
    pub goodbye: &'a str,
}

/// CRLF lines in CP437.
fn dos_lines(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for l in lines {
        out.extend(cp437::encode(l));
        out.extend(b"\r\n");
    }
    out
}

impl Control<'_> {
    pub fn encode(&self) -> Vec<u8> {
        let mut l = vec![
            self.bbs_name.to_string(),
            self.location.to_string(),
            self.phone.replace('(', "").replace(')', "-"),
            format!("{}, SysOp", self.sysop),
            format!("0241,{}", self.packet),
            self.when.format("%m-%d-%Y,%H:%M:%S").to_string(),
            self.user.to_uppercase(),
            String::new(),
            "0".into(),
            self.messages.to_string(),
            (self.confs.len().max(1) - 1).to_string(),
        ];
        for (n, name) in self.confs {
            l.push(n.to_string());
            l.push(name.clone());
        }
        l.extend([self.welcome.to_string(), self.news.to_string(), self.goodbye.to_string()]);
        dos_lines(&l)
    }
}

/// DOOR.ID: who made the packet. No ADD/DROP: conferences aren't
/// selected from the reader.
pub fn door_id() -> Vec<u8> {
    dos_lines(&[
        "DOOR = iNiQwk".into(),
        format!("VERSION = {}", env!("CARGO_PKG_VERSION")),
        "SYSTEM = Iniquity".into(),
        "CONTROLNAME = INIQWK".into(),
    ])
}

/// A reply from a REP packet.
#[derive(Debug, PartialEq)]
pub struct Reply {
    pub conf: u32,
    pub to: String,
    pub subject: String,
    pub private: bool,
    pub lines: Vec<String>,
}

/// Read the replies in a REP's NAME.MSG. The first block is the BBSID.
/// Readers put the conference in the message number field, falling back
/// to the binary conference number.
pub fn parse_rep(data: &[u8]) -> Vec<Reply> {
    let mut out = Vec::new();
    let mut at = BLOCK;
    while at + BLOCK <= data.len() {
        let h = &data[at..at + BLOCK];
        let blocks: usize = match text(&h[116..122]).parse() {
            Ok(n) if n >= 1 => n,
            _ => break,
        };
        let conf = match text(&h[1..8]).parse::<u32>() {
            Ok(n) => n,
            Err(_) => u16::from_le_bytes([h[123], h[124]]) as u32,
        };
        let body = &data[(at + BLOCK).min(data.len())..(at + blocks * BLOCK).min(data.len())];
        let mut lines: Vec<String> = body
            .split(|&c| c == EOL)
            .map(|l| cp437::decode(l).trim_end_matches([' ', '\0', '\r', '\n']).to_string())
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        out.push(Reply {
            conf,
            to: text(&h[21..46]),
            subject: text(&h[71..96]),
            private: matches!(h[0], b'*' | b'+'),
            lines,
        });
        at += blocks * BLOCK;
    }
    out
}

/// Lines longer than the message width, wrapped at spaces.
fn fit(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for l in lines {
        if l.chars().count() <= WIDTH {
            out.push(l);
        } else {
            out.extend(crate::multinode::wrap(&l, WIDTH));
        }
    }
    out
}

/// A stable fingerprint of a REP's replies (FNV-1a), to spot a packet
/// uploaded twice.
pub fn fingerprint(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

fn zip_files(path: &Path, files: &[(String, Vec<u8>)]) -> anyhow::Result<()> {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        z.start_file(name.as_str(), opts)?;
        z.write_all(data)?;
    }
    z.finish()?;
    Ok(())
}

/// The reply data in a REP: its first *.MSG.
fn rep_messages(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let name = z
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.to_ascii_uppercase().ends_with(".MSG"))
        .map(|n| n.into_owned())
        .ok_or_else(|| anyhow::anyhow!("no .MSG file in the packet"))?;
    let mut data = Vec::new();
    z.by_name(&name)?.read_to_end(&mut data)?;
    Ok(data)
}

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// An area in a packet.
struct Packed {
    conf: u16,
    area: AreaRef,
    /// Newest message looked at: where the newscan pointer moves to.
    newest: u32,
}

impl Session {
    /// Can a QWK packet include this area? `all`: in any conference, not
    /// just the current one (`mandMsg`).
    fn qwk_area_ok(&self, a: &AreaRef, all: bool) -> bool {
        if !a.area.password.is_empty() {
            return false;
        }
        if self.acs(&a.area.acs) {
            return true;
        }
        all && (1..=self.state.areas().confs.len() as u32).any(|c| {
            let mut u = self.user.clone();
            u.conf_msg = c;
            self.acs_for(&u, &a.area.acs)
        })
    }

    /// `sfLight`/`sfOkLight` on a status screen, or a line of text without one.
    async fn qwk_step(&mut self, screen: bool, code: &[u8; 2], what: &str) -> io::Result<()> {
        if screen {
            let c = self.state.config().prompts.light;
            if self.goto_pos(*code) {
                self.write(&c.to_string());
            }
        } else {
            self.show_line(&format!("|U2-- |U1{what}|U2.")).await?;
        }
        self.flush().await
    }

    fn qwk_done(&mut self, screen: bool, code: &[u8; 2], ok: bool) {
        let p = &self.state.config().prompts;
        let c = if ok { p.light_ok } else { p.light_fail };
        if screen && self.goto_pos(*code) {
            self.write(&c.to_string());
        }
    }

    /// Write a value at a status screen's `~XX` field.
    fn qwk_field(&mut self, screen: bool, code: &[u8; 2], value: &str, width: usize) {
        if screen && self.goto_pos(*code) {
            self.write_with_controls(&resize(value, width));
        }
    }

    fn qwk_temp(&self) -> std::path::PathBuf {
        let cfg = self.state.config();
        cfg.resolve(&cfg.paths.temp).join(format!("qwk{}", self.node.number()))
    }

    /// `qwkDownload` (`QD`).
    pub async fn qwk_download(&mut self) -> io::Result<()> {
        let total_areas = self.state.areas().areas.len() as u32;
        if total_areas == 0 {
            return Ok(());
        }
        self.newline();
        self.say("QwkDLaskCurConf").await?;
        let all = !self.yes_no(true).await?;
        self.say("QwkDLownMsgs").await?;
        let own = self.yes_no(false).await?;
        self.newline();
        tracing::info!("{}: downloaded offline mail [QWK] packet", self.user.handle);

        let cfg = self.state.config();
        let q = cfg.qwk.clone();
        let screen = self.show_file("QWKDL").await?;
        let end = (self.screen.where_x(), self.screen.where_y());

        self.qwk_step(screen, b"LC", "Creating control file").await?;
        let mut packed = vec![Packed { conf: 0, area: self.email_area(), newest: 0 }];
        for n in 1..=total_areas {
            if let Some(a) = self.msg_area(n)
                && self.qwk_area_ok(&a, all) {
                    packed.push(Packed { conf: n as u16, area: a, newest: 0 });
                }
        }
        let confs: Vec<(u16, String)> =
            packed.iter().map(|p| (p.conf, crate::editor::strip_codes(if p.conf == 0 { "E-mail" } else { &p.area.area.name }))).collect();
        self.qwk_done(screen, b"LC", true);
        self.qwk_step(screen, b"LD", "Creating door identification file").await?;
        self.qwk_done(screen, b"LD", true);

        self.qwk_step(screen, b"LE", "Exporting QWK, please wait").await?;
        let mut dat = vec![b' '; BLOCK];
        dat[..b"Produced by Qmail...Copyright (c) 1987 by Sparkware.  All Rights Reserved".len()]
            .copy_from_slice(b"Produced by Qmail...Copyright (c) 1987 by Sparkware.  All Rights Reserved");
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut personal = Vec::new();
        let mut seq: u16 = 0;
        for p in packed.iter_mut() {
            let key = p.area.area.file.clone();
            let last = self.state.msgs.lastread(self.user.id, &key).map_err(io_err)?;
            let msgs: Vec<Message> = if p.conf == 0 {
                self.state.msgs.mail_for(&key, &self.user.handle, &self.user.real_name, false).map_err(io_err)?
                    .into_iter().filter(|m| m.num > last).collect()
            } else {
                self.state.msgs.range(&key, last + 1, u32::MAX).map_err(io_err)?
            };
            p.newest = msgs.last().map_or(last, |m| m.num);
            let wanted: Vec<&Message> = msgs
                .iter()
                .filter(|m| self.ok_to_read(&p.area, m) && (own || !self.is_me(&m.from)))
                .collect();
            if p.conf != 0 {
                self.qwk_field(screen, b"MA", &p.area.area.name, 30);
                self.qwk_field(screen, b"EA", "No", 3);
                let count = self.state.msgs.count(&key).map_err(io_err)?;
                self.qwk_field(screen, b"MS", &count.to_string(), 5);
                self.qwk_field(screen, b"NM", &wanted.len().to_string(), 5);
                self.flush().await?;
            }
            if wanted.is_empty() {
                continue;
            }
            let mut ndx = Vec::new();
            for m in wanted {
                seq = seq.wrapping_add(1);
                let record = (dat.len() / BLOCK + 1) as u32;
                let mut entry = msbin(record).to_vec();
                entry.push(p.conf as u8);
                ndx.extend(&entry);
                if self.is_me(&m.to) {
                    personal.extend(&entry);
                }
                let lines = if q.strip_sigs { m.body() } else { &m.lines[..] };
                let body = body_blocks(lines);
                let h = Header {
                    private: m.private,
                    num: m.num,
                    posted: m.posted,
                    to: &m.to.name,
                    from: &m.from.name,
                    subject: &m.subject,
                    blocks: 1 + body.len() / BLOCK,
                    conf: p.conf,
                    seq,
                };
                dat.extend(h.encode());
                dat.extend(body);
            }
            files.push((format!("{:03}.NDX", p.conf), ndx));
            self.qwk_field(screen, b"TS", &format!("{}k", dat.len() / 1024), 6);
            self.qwk_field(screen, b"TN", &seq.to_string(), 5);
            self.flush().await?;
        }
        self.qwk_done(screen, b"LE", true);
        let total = seq as usize;
        let when = Local::now().naive_local();
        let control = Control {
            bbs_name: &crate::editor::strip_codes(&cfg.bbs.name),
            location: &cfg.bbs.location,
            phone: &cfg.bbs.phone,
            sysop: if cfg.bbs.sysop_name.is_empty() { &cfg.bbs.sysop_alias } else { &cfg.bbs.sysop_name },
            packet: &q.packet,
            when,
            user: &self.user.handle,
            messages: total,
            confs: &confs,
            welcome: &q.welcome,
            news: &q.news,
            goodbye: &q.goodbye,
        };
        files.push(("CONTROL.DAT".into(), control.encode()));
        files.push(("DOOR.ID".into(), door_id()));
        files.push(("MESSAGES.DAT".into(), dat));
        if !personal.is_empty() {
            files.push(("PERSONAL.NDX".into(), personal));
        }

        self.qwk_step(screen, b"LI", "Including text/information files").await?;
        for name in [&q.welcome, &q.news, &q.goodbye] {
            if let Some(path) = self.find_text_file(name)
                && let Ok(data) = std::fs::read(&path) {
                    files.push((name.to_ascii_uppercase(), data));
                }
        }
        self.qwk_done(screen, b"LI", true);

        if screen {
            self.screen.goto(end.0, end.1);
        }
        if total == 0 {
            self.newline();
            return self.say_line("QwkDLnoNewMsgs").await;
        }
        self.qwk_step(screen, b"LZ", "Compressing QWK mail packet").await?;
        let temp = self.qwk_temp();
        let _ = std::fs::remove_dir_all(&temp);
        let path = temp.join(format!("{}.QWK", q.packet.to_ascii_uppercase()));
        let zipped = std::fs::create_dir_all(&temp).map_err(anyhow::Error::from).and_then(|_| zip_files(&path, &files));
        self.qwk_done(screen, b"LZ", zipped.is_ok());
        if screen {
            self.screen.goto(end.0, end.1);
        }
        self.newline();
        if let Err(e) = zipped {
            tracing::error!("QWK download error: {e}");
            let _ = std::fs::remove_dir_all(&temp);
            return self.say_line("QwkDLerror").await;
        }
        let result = self.qwk_send(&path, &packed).await;
        let _ = std::fs::remove_dir_all(&temp);
        result
    }

    /// Offer the packet, and move the newscan pointers once it's down.
    async fn qwk_send(&mut self, path: &Path, packed: &[Packed]) -> io::Result<()> {
        let size = std::fs::metadata(path)?.len();
        let s = self.strings().fmt("QwkDLaskDownload", &[&crate::files::commas(size)]);
        self.show(&s).await?;
        if !self.yes_no(true).await? {
            return Ok(());
        }
        let Some(proto) = self.select_protocol(false).await? else { return Ok(()) };
        let ok = self.send_files(&proto, &[path.to_path_buf()]).await?;
        if ok.first() != Some(&true) {
            tracing::info!("{}: QWK download failed", self.user.handle);
            return self.say_line("QwkDLerror").await;
        }
        self.say("QwkDLaskUpdatePtrs").await?;
        if self.yes_no(true).await? {
            self.say("QwkDLupdatingPtrs").await?;
            for p in packed {
                self.state.msgs.set_lastread(self.user.id, &p.area.area.file, p.newest).map_err(io_err)?;
            }
            self.newline();
        }
        Ok(())
    }

    /// `qwkUpload` (`QU`).
    pub async fn qwk_upload(&mut self) -> io::Result<()> {
        if self.state.areas().areas.is_empty() {
            return Ok(());
        }
        self.newline();
        let packet = self.state.config().qwk.packet.to_ascii_uppercase();
        let temp = self.qwk_temp();
        let _ = std::fs::remove_dir_all(&temp);
        let mut local = String::new();
        if self.is_sysop() {
            self.say("QwkULaskFile").await?;
            local = self.read_string(crate::input::ReadOpts::new(65)).await?;
            self.newline();
        }
        let rep = if local.trim().is_empty() {
            let Some(proto) = self.select_protocol(false).await? else { return Ok(()) };
            let got = self.receive_files(&proto, &temp).await?;
            match got.into_iter().next() {
                Some(p) => p,
                None => return Ok(()),
            }
        } else {
            std::path::PathBuf::from(local.trim())
        };
        tracing::info!("{}: uploaded offline mail [REP] packet {}", self.user.handle, rep.display());
        let result = self.qwk_import(&rep, &packet).await;
        let _ = std::fs::remove_dir_all(&temp);
        result
    }

    async fn qwk_import(&mut self, rep: &Path, packet: &str) -> io::Result<()> {
        let screen = self.show_file("QWKUL").await?;
        let end = (self.screen.where_x(), self.screen.where_y());
        let fail = |s: &mut Self, why: String| {
            tracing::warn!("REP upload error ({packet}): {why}");
            if screen {
                s.screen.goto(end.0, end.1);
            }
            s.newline();
        };
        self.qwk_step(screen, b"LU", "Unarchiving REP packet").await?;
        let data = match rep_messages(rep) {
            Ok(d) => d,
            Err(e) => {
                self.qwk_done(screen, b"LU", false);
                fail(self, e.to_string());
                return self.say_line("QwkULerror").await;
            }
        };
        self.qwk_done(screen, b"LU", true);
        let print = fingerprint(&data);
        if print == self.user.last_rep {
            fail(self, "already uploaded".into());
            return self.say_line("QwkULalready").await;
        }
        let replies = parse_rep(&data);
        if replies.is_empty() {
            fail(self, "no replies in the packet".into());
            return self.say_line("QwkULerror").await;
        }

        self.qwk_step(screen, b"LP", "Importing replies, please wait").await?;
        let (mut posted, mut refused) = (0u32, 0u32);
        for (i, r) in replies.into_iter().enumerate() {
            let area = if r.conf == 0 { Some(self.email_area()) } else { self.msg_area(r.conf) };
            self.qwk_field(screen, b"MA", &area.as_ref().map_or("?".into(), |a| a.area.name.clone()), 30);
            self.qwk_field(screen, b"TM", &(i + 1).to_string(), 5);
            self.qwk_field(screen, b"SN", &self.user.handle.clone(), 30);
            self.qwk_field(screen, b"RN", &r.to, 30);
            self.qwk_field(screen, b"MT", &r.subject, 40);
            self.flush().await?;
            match self.qwk_post(area, r) {
                Ok(true) => posted += 1,
                Ok(false) => refused += 1,
                Err(e) => {
                    tracing::error!("posting a QWK reply: {e}");
                    refused += 1;
                }
            }
        }
        self.qwk_done(screen, b"LP", true);
        self.qwk_step(screen, b"LS", "Updating message statistics").await?;
        self.user.last_rep = print;
        self.save_user();
        self.qwk_done(screen, b"LS", true);
        if screen {
            self.screen.goto(end.0, end.1);
        }
        self.newline();
        tracing::info!("{}: {posted} QWK replies posted, {refused} refused", self.user.handle);
        let s = self.strings().fmt("QwkULdone", &[&posted.to_string(), &refused.to_string()]);
        self.show_line(&s).await
    }

    /// Post one reply, if the caller may post there. Ok(false): refused.
    fn qwk_post(&mut self, area: Option<AreaRef>, r: Reply) -> anyhow::Result<bool> {
        let Some(a) = area else { return Ok(false) };
        let email = a.num == 0;
        let allowed = if email {
            self.acs(&self.state.config().messages.acs_post_email)
        } else {
            a.area.password.is_empty() && self.acs(&a.area.acs) && self.acs(&a.area.post_acs)
        };
        if !allowed || r.lines.is_empty() {
            return Ok(false);
        }
        let to = match self.state.users.find(&r.to, false)? {
            Some(u) => Party { user_id: u.id, alias: u.handle.clone(), real_name: u.real_name.clone(), name: u.handle, note: u.note },
            None if email => return Ok(false),
            None => {
                let name = if r.to.is_empty() { "All".to_string() } else { title_case(&r.to) };
                Party { user_id: 0, alias: "None".into(), real_name: "None".into(), name, note: "None".into() }
            }
        };
        let u = &self.user;
        let from = Party {
            user_id: u.id,
            alias: u.handle.clone(),
            real_name: u.real_name.clone(),
            name: if a.area.real_names { u.real_name.clone() } else { u.handle.clone() },
            note: u.note.clone(),
        };
        let private = email || (r.private && a.area.private);
        let lines = fit(r.lines);
        // The original took the last line starting "--" as the signature.
        let sig_pos = lines.iter().rposition(|l| l.chars().take(4).collect::<String>().contains("--")).map_or(0, |i| i as u32 + 1);
        let mut m = Message {
            area: a.area.file.clone(),
            num: 0,
            from,
            to,
            subject: if r.subject.is_empty() { "(no subject)".into() } else { r.subject },
            posted: Local::now().naive_local(),
            deleted: false,
            private,
            anonymous: false,
            replies: 0,
            sig_pos,
            lines,
        };
        self.state.msgs.add(&mut m)?;
        if private {
            self.user.email += 1;
            self.bump(crate::sysdata::Stat::Email);
        } else {
            self.user.posts += 1;
            self.bump(crate::sysdata::Stat::Post);
        }
        Ok(true)
    }
}

/// `strMixed`: "JOHN DOE" -> "John Doe" (QWK names are upper case).
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn msbin_round_trips() {
        for n in [1u32, 2, 3, 127, 128, 1000, 65_535, 8_388_607, 8_388_608, 20_000_000] {
            assert_eq!(from_msbin(msbin(n)), n, "{n}");
        }
        // Known values: 1.0 and 2.0 in MBF.
        assert_eq!(msbin(1), [0, 0, 0, 0x81]);
        assert_eq!(msbin(2), [0, 0, 0, 0x82]);
        assert_eq!(msbin(0), [0; 4]);
    }

    #[test]
    fn header_fields() {
        let when = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(9, 5, 0).unwrap();
        let h = Header { private: true, num: 42, posted: when, to: "phrac", from: "Fiend", subject: "Hello there", blocks: 3, conf: 7, seq: 2 };
        let b = h.encode();
        assert_eq!(b[0], b'*');
        assert_eq!(&b[1..8], b"42     ");
        assert_eq!(&b[8..21], b"10-17-2609:05");
        assert_eq!(text(&b[21..46]), "PHRAC");
        assert_eq!(text(&b[46..71]), "FIEND");
        assert_eq!(text(&b[71..96]), "Hello there");
        assert_eq!(text(&b[116..122]), "3");
        assert_eq!(b[122], 0xE1);
        assert_eq!(u16::from_le_bytes([b[123], b[124]]), 7);
    }

    #[test]
    fn bodies_are_blocks() {
        let b = body_blocks(&["one".into(), "π".into()]);
        assert_eq!(b.len(), BLOCK);
        assert_eq!(&b[..6], &[b'o', b'n', b'e', EOL, 0xD2, EOL]);
        assert!(body_blocks(&[]).is_empty());
        assert_eq!(body_blocks(&["x".repeat(200)]).len(), 2 * BLOCK);
    }

    #[test]
    fn control_file() {
        let when = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(21, 30, 5).unwrap();
        let confs = [(0, "E-mail".to_string()), (1, "General".to_string())];
        let c = Control {
            bbs_name: "Iniquity",
            location: "Halifax, NS",
            phone: "(902)555-1212",
            sysop: "SysOp",
            packet: "INIQUITY",
            when,
            user: "phrac",
            messages: 5,
            confs: &confs,
            welcome: "WELCOME.ANS",
            news: "NEWS.ANS",
            goodbye: "LOGOFF.ANS",
        };
        let s = String::from_utf8(c.encode()).unwrap();
        let l: Vec<&str> = s.split("\r\n").collect();
        assert_eq!(&l[..11], ["Iniquity", "Halifax, NS", "902-555-1212", "SysOp, SysOp", "0241,INIQUITY", "10-17-2026,21:30:05", "PHRAC", "", "0", "5", "1"]);
        assert_eq!(&l[11..18], ["0", "E-mail", "1", "General", "WELCOME.ANS", "NEWS.ANS", "LOGOFF.ANS"]);
    }

    /// A REP as a reader would write it.
    fn rep(replies: &[(u32, &str, &str, bool, &[&str])]) -> Vec<u8> {
        let mut d = vec![b' '; BLOCK];
        d[..8].copy_from_slice(b"INIQUITY");
        for (conf, to, subj, private, lines) in replies {
            let body = body_blocks(&lines.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            let mut h = [b' '; BLOCK];
            h[0] = if *private { b'*' } else { b' ' };
            field(&mut h, 1, 7, &conf.to_string());
            field(&mut h, 21, 25, to);
            field(&mut h, 71, 25, subj);
            field(&mut h, 116, 6, &(1 + body.len() / BLOCK).to_string());
            h[123..125].copy_from_slice(&(*conf as u16).to_le_bytes());
            d.extend(h);
            d.extend(body);
        }
        d
    }

    #[test]
    fn reads_replies() {
        let data = rep(&[(3, "ALL", "Hi", false, &["line one", "", "line three"]), (0, "FIEND", "Psst", true, &["secret"])]);
        let r = parse_rep(&data);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], Reply { conf: 3, to: "ALL".into(), subject: "Hi".into(), private: false, lines: vec!["line one".into(), "".into(), "line three".into()] });
        assert_eq!((r[1].conf, r[1].private, r[1].lines.clone()), (0, true, vec!["secret".to_string()]));
        // No number in the text field: the binary conference.
        let mut blank = data.clone();
        blank[BLOCK + 1..BLOCK + 8].fill(b' ');
        assert_eq!(parse_rep(&blank)[0].conf, 3);
        // A truncated packet stops cleanly.
        assert_eq!(parse_rep(&data[..BLOCK + 10]).len(), 0);
    }

    #[test]
    fn wraps_long_lines_and_names() {
        let long = "word ".repeat(30);
        let out = fit(vec!["short".into(), long.trim().into()]);
        assert_eq!(out[0], "short");
        assert!(out.len() > 2 && out.iter().all(|l| l.chars().count() <= WIDTH));
        assert_eq!(title_case("JOHN DOE"), "John Doe");
        assert_ne!(fingerprint(b"a"), fingerprint(b"b"));
    }
}
