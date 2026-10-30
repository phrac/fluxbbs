//! The smaller features: menu-scripted input (`?I`, `?T`, which the stock
//! oneliners use), last callers, BBS history, the automessage, the BBS
//! list, user configuration and auto-signatures.

use std::io;

use fluxbbs_term::{Charset, Slot, lit};
use fluxbbs_term::codes::{resize, squish};

use crate::editor::EditCtx;
use crate::files::commas;
use crate::input::{Case, DIGITS, Key, ReadOpts};
use crate::session::{Session, Waves, waves};
use crate::sysdata::{AutoMsg, Bbs, Call, Stat};
use crate::user::fmt_date;

const AUTOMSG_LINES: usize = 10;
/// `maxSigLines`, and the `String[80]` each line was stored in.
const SIG_LINES: usize = 6;
const SIG_WIDTH: usize = 80;

/// Fit text into an auto-signature: at most six lines of 80 characters,
/// without trailing blank lines.
pub fn clamp_sig(lines: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut sig: Vec<String> = lines
        .into_iter()
        .take(SIG_LINES)
        .map(|l| l.trim_end().chars().take(SIG_WIDTH).collect())
        .collect();
    while sig.last().is_some_and(|l| l.is_empty()) {
        sig.pop();
    }
    sig
}

/// An uploaded signature file: CP437 text, possibly ANSI, ending at ^Z.
fn parse_sig_file(data: &[u8]) -> Vec<String> {
    let data = data.split(|&b| b == 0x1A).next().unwrap_or_default();
    let lines = data
        .split(|&b| b == b'\n')
        // ESC kept as itself (CP437 makes it `←`), as in QWK replies.
        .map(|l| fluxbbs_term::cp437::decode(l.strip_suffix(b"\r").unwrap_or(l)).replace('\u{2190}', "\x1b"));
    let mut sig = clamp_sig(lines);
    while sig.first().is_some_and(|l| l.is_empty()) {
        sig.remove(0);
    }
    sig
}

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// Lines a BBS list entry's description may have.
pub(crate) const BBS_INFO_LINES: usize = 10;

/// The SSH address for a list: just `:port` when it's on the telnet
/// address's host.
fn ssh_short(b: &Bbs) -> String {
    let host = |a: &str| a.rsplit_once(':').map_or(a, |(h, _)| h).to_ascii_lowercase();
    match b.ssh.rsplit_once(':') {
        Some((h, port)) if !h.is_empty() && h.eq_ignore_ascii_case(&host(&b.telnet)) => format!(":{port}"),
        _ => b.ssh.clone(),
    }
}

impl Session {
    /// Count something toward today's history and the totals.
    pub fn bump(&self, stat: Stat) {
        if let Err(e) = self.state.sys.bump(crate::clock::now().date_naive(), stat) {
            tracing::error!("recording stats: {e}");
        }
    }

    /// `callAdd`: count and log this call.
    pub fn log_call(&self) {
        let call = Call {
            call_num: 0,
            user_id: self.user.id,
            handle: self.user.handle.clone(),
            location: self.user.location.clone(),
            at: crate::clock::now().naive_local(),
            new_user: self.new_user,
            area_code: self.user.area_code().to_string(),
            transport: self.caller.transport.to_string(),
        };
        if self.hidden_call {
            self.bump(crate::sysdata::Stat::Call);
            return;
        }
        if let Err(e) = self.state.sys.log_call(call) {
            tracing::error!("logging call: {e}");
        }
    }

    // ---- ?I / ?T -----------------------------------------------------------

    /// `?I` (`iGetString`): `format;chars;options;length;default`. Format
    /// is the case (N/C/U/L/M/V/W/I), chars the allowed set (A alpha,
    /// N/I numeric, else anything), `*` defaults. Of the options, 2.0's
    /// `/AC` (tab completes a handle) is honoured.
    pub(crate) async fn menu_input(&mut self, semis: &[&str]) -> io::Result<()> {
        let get = |i: usize| semis.get(i).copied().unwrap_or("");
        let case = match get(0).chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('C') => Case::Capital,
            Some('U') => Case::Upper,
            Some('L') => Case::Lower,
            Some('M') => Case::Mixed,
            Some('V') => Case::Weird,
            Some('W') => Case::Warped,
            Some('I') => Case::Cool,
            _ => Case::Normal,
        };
        let allowed = match get(1).chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('A') => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz ",
            Some('N' | 'I') => DIGITS,
            _ => "",
        };
        let len = get(3).trim().parse::<usize>().ok().filter(|&n| n > 0).unwrap_or(255);
        let default = match get(4) {
            "*" => self.input_string.clone(),
            d => d.to_string(),
        };
        let flags = get(2).to_ascii_uppercase();
        let mut opts = ReadOpts::new(len).case(case).allowed(allowed).prefill(default);
        if flags.contains("/AC") {
            opts = opts.complete_users();
        }
        let s = self.read_string(opts).await?;
        self.newline();
        // What's typed may end up in a screen (`?T`): its codes mustn't act
        // there. Passwords are left as typed.
        self.input_string = if flags.contains("/PW") { s } else { fluxbbs_term::defuse(&s) };
        Ok(())
    }

    /// `?T` (`fAddToText`): `file;keep` appends the input string to a text
    /// file, keeping its last `keep` lines.
    pub(crate) fn menu_append(&mut self, semis: &[&str]) -> io::Result<()> {
        let name = semis.first().copied().unwrap_or("").trim();
        let keep: usize = semis.get(1).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Ok(());
        }
        let cfg = self.state.config();
        let dir = cfg.resolve(&cfg.paths.text);
        let path = crate::session::find_ci(&dir, name).unwrap_or_else(|| dir.join(name));
        // A stock file the board hasn't written yet starts from the stock copy.
        let from = if path.exists() || cfg.paths.stock_text.as_os_str().is_empty() {
            path.clone()
        } else {
            crate::session::find_ci(&cfg.resolve(&cfg.paths.stock_text), name).unwrap_or_else(|| path.clone())
        };
        let _guard = self.state.text_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut lines: Vec<Vec<u8>> = match std::fs::read(&from) {
            Ok(b) => {
                let b = b.split(|&c| c == 0x1a).next().unwrap_or_default().to_vec();
                b.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec()).collect()
            }
            Err(_) => Vec::new(),
        };
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.push(fluxbbs_term::cp437::encode(&self.input_string));
        if keep > 0 && lines.len() > keep {
            lines.drain(..lines.len() - keep);
        }
        let mut out: Vec<u8> = lines.join(&b"\r\n"[..]);
        out.extend_from_slice(b"\r\n");
        std::fs::write(&path, out)
    }

    // ---- last callers and history ------------------------------------------

    /// `callLastCallersShow` (`LC`): `All` lists today's callers.
    pub async fn last_callers(&mut self, param: &str) -> io::Result<()> {
        let all = param.trim().to_ascii_uppercase().starts_with('A');
        let calls = if all {
            self.state.sys.calls(None, Some(crate::clock::now().date_naive()))
        } else {
            self.state.sys.calls(Some(self.state.config().logon.last_callers), None)
        }
        .map_err(io_err)?;
        // New: the caller's own call, the newest of theirs, is drawn
        // highlighted (LASTCHIL) and then, animating, types itself in
        // with a glint across it: that last call is this one.
        let me = self.user.id;
        let newest = if self.hidden_call { None } else { calls.iter().filter(|c| c.user_id == me).map(|c| c.call_num).max() };
        let mut mine: Option<u16> = None;
        let mut rep = self.repeat_begin("LASTC", true).await?;
        if rep.is_none() {
            self.clear_screen();
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line("|U4 Num   Username                 Location              New  Via     Time").await?;
            self.rule().await?;
        }
        let mut lines = 4;
        for c in &calls {
            let at = self.local(c.at);
            let time = at.format("%-I:%M%P").to_string();
            let new = if c.new_user { "Yes" } else { "No" };
            let own = c.user_id == me && Some(c.call_num) == newest;
            if own {
                mine = Some(self.screen.where_y());
            }
            let more = match rep.as_mut() {
                Some(r) => {
                    let w = waves([
                        ("CN", c.call_num.to_string()),
                        ("UN", c.handle.clone()),
                        ("NM", c.user_id.to_string()),
                        ("LO", c.location.clone()),
                        ("BR", c.transport.clone()),
                        ("DT", at.format("%m/%d/%y").to_string()),
                        ("TI", time),
                        ("NU", new.into()),
                        ("AC", c.area_code.clone()),
                    ]);
                    self.repeat_row(r, &w, own).await?
                }
                None => {
                    let line = format!(
                        "{} {} {} {} {} {} {}",
                        if own { "|U3" } else { "|U1" },
                        resize(&c.call_num.to_string(), 5),
                        lit(&resize(&c.handle, 24)),
                        lit(&resize(&c.location, 21)),
                        resize(new, 4),
                        resize(&c.transport, 7),
                        time
                    );
                    self.show_line(&line).await?;
                    lines += 1;
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => self.rule().await?,
        }
        // The list may have scrolled since that row was drawn: find it
        // again by what it shows, and skip the effect if it's gone.
        let mine = mine.and_then(|_| {
            let (num, handle) = calls.iter().find(|c| c.user_id == me && Some(c.call_num) == newest).map(|c| (c.call_num.to_string(), fluxbbs_term::cp437::encode(&c.handle)))?;
            (1..=self.screen.rows()).find(|&y| {
                let text: Vec<u8> = (1..=80).map(|x| self.screen.shown(x, y).map_or(b' ', |s| s.ch)).collect();
                let has = |needle: &[u8]| !needle.is_empty() && text.windows(needle.len()).any(|w| w == needle);
                has(num.as_bytes()) && has(&handle)
            })
        });
        if let Some(y) = mine {
            use crate::ambient::Effect;
            let row = |kind: &str, delay: f32, amount: f32, tint: &str| Effect {
                kind: kind.into(),
                rows: [y, y],
                cols: [1, 80],
                every: 60.0,
                delay,
                amount,
                tint: tint.into(),
                matches: String::new(),
                messages: Vec::new(),
            };
            self.play_effects(&[row("type", 0.0, 0.6, "#ffffff"), row("glint", 1.6, 0.9, "")], 3.4).await?;
        }
        Ok(())
    }

    /// `hisView` (`LH`): one line per day.
    pub async fn show_history(&mut self) -> io::Result<()> {
        let days = self.state.sys.history().map_err(io_err)?;
        self.newline();
        self.set_slot(Slot::Info);
        self.show_line(" Date         Calls  New   Posts   Email   # ULs   # DLs   UL kb   DL kb").await?;
        self.rule().await?;
        let mut lines = 3;
        for (d, c) in &days {
            let row = format!(
                "|U1 {} {} {} {} {} {} {} {} {}",
                resize(&d.format("%m/%d/%y").to_string(), 12),
                resize(&commas(c.calls), 6),
                resize(&commas(c.new_users), 5),
                resize(&commas(c.posts), 7),
                resize(&commas(c.email), 7),
                resize(&commas(c.uploads), 7),
                resize(&commas(c.downloads), 7),
                resize(&commas(c.upload_kb), 7),
                commas(c.download_kb)
            );
            self.show_line(&row).await?;
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        self.rule().await
    }

    // ---- the automessage ---------------------------------------------------

    /// `AS`: show the automessage.
    pub async fn show_automsg(&mut self) -> io::Result<()> {
        let Some(m) = self.state.sys.automsg().map_err(io_err)? else {
            self.newline();
            return self.say_line("NoAutoMsg").await;
        };
        let author = if m.anonymous { self.strings().get("Anonymous").to_string() } else { m.author.clone() };
        let w = waves([("MT", m.title.clone()), ("SN", author.clone())]);
        if self.show_file_with("AUTOTOP", &w).await? {
            self.goto_pos(*b"TL");
        } else {
            self.newline();
            self.show_line(&format!("|U4Automessage|U5: |U6{}", lit(&m.title))).await?;
            self.rule().await?;
        }
        for l in &m.lines {
            self.set_slot(Slot::Text);
            self.write_with_controls(l);
            self.newline();
        }
        if !self.show_file_with("AUTOBOT", &w).await? {
            self.rule().await?;
            self.show_line(&format!("|U4Posted by|U5: |U6{}", fluxbbs_term::lit(&author))).await?;
        }
        Ok(())
    }

    /// `AW`: replace the automessage.
    pub async fn write_automsg(&mut self) -> io::Result<()> {
        self.newline();
        self.say("MsgAskTitle").await?;
        let title = self.read_string(ReadOpts::new(40)).await?;
        self.newline();
        if title.is_empty() {
            return Ok(());
        }
        let mut anonymous = false;
        if self.acs(&self.state.config().messages.acs_anon_automsg.clone()) {
            self.say("MsgAskAnonymous").await?;
            anonymous = self.yes_no(false).await?;
        }
        let ctx = EditCtx {
            subject: title.clone(),
            from: self.user.handle.clone(),
            to: "All".into(),
            date: crate::clock::now().naive_local(),
            anonymous,
            can_anonymous: false,
            can_autosig: false,
            autosig: false,
            reply: None,
            auto_quote: false,
            text: Vec::new(),
        };
        let Some(done) = self.edit_message(ctx).await? else { return Ok(()) };
        if done.lines.is_empty() {
            return Ok(());
        }
        let m = AutoMsg {
            user_id: self.user.id,
            author: self.user.handle.clone(),
            anonymous,
            title: done.subject,
            posted: crate::clock::now().naive_local(),
            lines: done.lines.into_iter().take(AUTOMSG_LINES).collect(),
        };
        self.state.sys.set_automsg(&m).map_err(io_err)?;
        tracing::info!("{}: changed the automessage", self.user.handle);
        Ok(())
    }

    /// `AR`: email the automessage's author.
    pub async fn reply_automsg(&mut self) -> io::Result<()> {
        let Some(m) = self.state.sys.automsg().map_err(io_err)? else {
            return self.say_line("NoAutoMsg").await;
        };
        if m.anonymous {
            return self.say_line("AutoMsgAnon").await;
        }
        let subject = format!("Re: {}", m.title);
        self.enter_email(&m.user_id.to_string(), &subject).await
    }

    // ---- auto-signatures (the AUTOSIG unit) ---------------------------------

    /// `SS`: show the caller's auto-signature.
    pub async fn show_autosig(&mut self) -> io::Result<()> {
        self.newline();
        if self.user.autosig.is_empty() {
            return self.say_line("AutoSigNone").await;
        }
        self.say("AutoSigShow").await?;
        for line in self.user.autosig.clone() {
            self.show_line(&fluxbbs_term::lit(&line)).await?;
        }
        self.screen.set_color(self.user.palette.get(Slot::Text));
        Ok(())
    }

    /// `SD`: delete the auto-signature.
    pub async fn delete_autosig(&mut self) -> io::Result<()> {
        self.newline();
        if self.user.autosig.is_empty() {
            return self.say_line("AutoSigNone").await;
        }
        self.say("AutoSigAskDelete").await?;
        if !self.yes_no(false).await? {
            return Ok(());
        }
        self.newline();
        self.user.autosig.clear();
        self.save_user();
        self.say_line("AutoSigDeleted").await
    }

    /// `SE`: write or change the auto-signature in the full-screen editor.
    pub async fn edit_autosig(&mut self) -> io::Result<()> {
        let (handle, sig) = (self.user.handle.clone(), self.user.autosig.clone());
        let Some(sig) = self.sig_editor(&handle, sig).await? else { return Ok(()) };
        self.set_autosig(sig).await
    }

    /// `asEditAutoSig`: a signature in the full-screen editor. None if
    /// aborted; otherwise clamped to six lines.
    pub async fn sig_editor(&mut self, handle: &str, sig: Vec<String>) -> io::Result<Option<Vec<String>>> {
        let ctx = EditCtx {
            subject: "AutoSignature".into(),
            from: handle.to_string(),
            to: handle.to_string(),
            date: crate::clock::now().naive_local(),
            anonymous: false,
            can_anonymous: false,
            can_autosig: false,
            autosig: false,
            reply: None,
            auto_quote: false,
            text: sig,
        };
        Ok(self.edit_message(ctx).await?.map(|done| clamp_sig(done.lines)))
    }

    /// `SU`: upload a prepared signature (ASCII or ANSI).
    pub async fn upload_autosig(&mut self) -> io::Result<()> {
        self.newline();
        self.say("AutoSigAskUpload").await?;
        if !self.yes_no(true).await? {
            return Ok(());
        }
        self.newline();
        let Some(proto) = self.select_protocol(false).await? else { return Ok(()) };
        let temp = self.state.config().resolve(&self.state.config().paths.temp).join(format!("node{}", self.node.number()));
        let _ = std::fs::remove_dir_all(&temp);
        let got = self.receive_files(&proto, &temp).await?;
        // Only the start of the first file matters; six lines can't be long.
        let data = match got.first() {
            Some(p) => std::fs::read(p).map(|mut d| {
                d.truncate(16 * 1024);
                d
            })?,
            None => Vec::new(),
        };
        let _ = std::fs::remove_dir_all(&temp);
        let sig = parse_sig_file(&data);
        if sig.is_empty() {
            return self.say_line("AutoSigNone").await;
        }
        self.set_autosig(sig).await
    }

    async fn set_autosig(&mut self, sig: Vec<String>) -> io::Result<()> {
        self.newline();
        if sig.is_empty() {
            self.user.autosig.clear();
            self.save_user();
            return self.say_line("AutoSigDeleted").await;
        }
        self.user.autosig = sig;
        self.save_user();
        tracing::info!("{}: changed their auto-signature", self.user.handle);
        self.say_line("AutoSigChanged").await?;
        self.show_autosig().await
    }

    // ---- the BBS list --------------------------------------------------------

    /// `full`: the whole description (several lines, for SHOWBBS); a
    /// list's row gets its first line.
    fn bbs_waves(b: &Bbs, n: usize, full: bool) -> Waves {
        let mut lines = b.info.lines();
        let info = lines.next().unwrap_or("").to_string();
        let mut w = waves([
            ("BN", b.name.clone()),
            // TN/SS (new) telnet and SSH; PN, the original's phone
            // number, is the telnet address on older screens.
            ("TN", b.telnet.clone()),
            ("PN", b.telnet.clone()),
            ("SS", b.ssh.clone()),
            ("SH", ssh_short(b)),
            ("SN", b.sysop.clone()),
            ("SF", b.software.clone()),
            ("ST", b.storage.clone()),
            ("IN", info),
            ("NM", n.to_string()),
            ("OT", b.who_added.clone()),
        ]);
        // New: the description's later lines, `~I2`..`~I9`.
        if full {
            for (k, l) in lines.take(BBS_INFO_LINES - 1).enumerate() {
                w.insert([b'I', b'2' + k as u8], l.to_string());
            }
        }
        w
    }

    /// `bbsListShowAll` (`BL`). Returns how many there are.
    pub async fn bbs_list(&mut self) -> io::Result<usize> {
        let list = self.state.sys.bbs_list().map_err(io_err)?;
        if list.is_empty() {
            self.newline();
            self.say_line("BBSnoSystems").await?;
            return Ok(0);
        }
        let mut rep = self.repeat_begin("BBS", false).await?;
        if rep.is_none() {
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line(" Num  BBS Name           Telnet                       SSH          Software").await?;
            self.rule().await?;
        }
        let mut lines = 3;
        for (i, b) in list.iter().enumerate() {
            let more = match rep.as_mut() {
                Some(r) => self.repeat_row(r, &Self::bbs_waves(b, i + 1, false), false).await?,
                None => {
                    let line = format!(
                        "|U1 {} {} {} {} {}",
                        resize(&(i + 1).to_string(), 4),
                        lit(&resize(&b.name, 18)),
                        lit(&resize(&b.telnet, 28)),
                        lit(&resize(&ssh_short(b), 12)),
                        lit(&squish(&b.software, 10))
                    );
                    self.show_line(&line).await?;
                    lines += 1;
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => self.rule().await?,
        }
        Ok(list.len())
    }

    /// Pick an entry: the parameter's number, or list and ask.
    async fn pick_bbs(&mut self, param: &str, prompt: &str) -> io::Result<Option<(usize, Bbs)>> {
        let list = self.state.sys.bbs_list().map_err(io_err)?;
        let n = match param.split(';').next().unwrap_or("").trim().parse::<usize>() {
            Ok(n) if (1..=list.len()).contains(&n) => n,
            _ => {
                let count = self.bbs_list().await?;
                if count == 0 {
                    return Ok(None);
                }
                let s = self.strings().fmt(prompt, &[&count.to_string()]);
                self.show(&s).await?;
                let s = self.read_string(ReadOpts::new(3).allowed(DIGITS)).await?;
                self.newline();
                match s.parse::<usize>() {
                    Ok(n) if (1..=list.len()).contains(&n) => n,
                    _ => return Ok(None),
                }
            }
        };
        Ok(Some((n, list[n - 1].clone())))
    }

    /// `bbsListShowOne` (`BS`).
    pub async fn bbs_show(&mut self, param: &str) -> io::Result<()> {
        let Some((n, b)) = self.pick_bbs(param, "BBSshowOne").await? else { return Ok(()) };
        let w = Self::bbs_waves(&b, n, true);
        if self.show_file_with("SHOWBBS", &w).await? {
            // A screen with no `~I2`: the rest of a long description goes
            // under it, lined up with its first line.
            let rest: Vec<&str> = b.info.lines().skip(1).collect();
            if !rest.is_empty() && !self.positions.contains_key(b"I2") {
                let (x, _, color) = self.positions.get(b"IN").copied().unwrap_or((1, 1, fluxbbs_term::Color::DEFAULT));
                // On the rows after the screen's last field.
                let y = self.positions.values().map(|p| p.1).max().unwrap_or(1) + 1;
                for (k, l) in rest.iter().enumerate() {
                    self.screen.goto(x, y + k as u16);
                    self.screen.set_color(color);
                    self.write(l);
                }
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.screen.goto(1, y + rest.len() as u16);
            }
        } else {
            self.newline();
            for (k, v) in [
                ("List Number", n.to_string()),
                ("System Name", b.name.clone()),
                ("Telnet", b.telnet.clone()),
                ("SSH", b.ssh.clone()),
                ("SysOp", b.sysop.clone()),
                ("Software", b.software.clone()),
                ("Storage", b.storage.clone()),
                ("Other Info", b.info.clone()),
            ] {
                // A description of several lines, each under the first.
                for (i, line) in v.lines().chain(v.is_empty().then_some("")).enumerate() {
                    let label = if i == 0 { format!("|U1{}|U2:", resize(k, 17)) } else { " ".repeat(18) };
                    self.show_line(&format!("{label} |U3{}", lit(line))).await?;
                }
            }
        }
        Ok(())
    }

    /// `bbsListAddtoList` (`BA`), using ADDBBS's positions if present.
    pub async fn bbs_add(&mut self) -> io::Result<()> {
        let screen = self.show_file("ADDBBS").await?;
        if !screen {
            self.newline();
        }
        let mut b = Bbs { who_added: self.user.handle.clone(), ..Bbs::default() };
        // ADDBBS marks each field (`~TN`/`~SS`; older screens have `~PN`
        // and `~BR`, the phone number and baud rate, in those places).
        let fields: [(&[u8; 2], &[u8; 2], &str, usize); 7] = [
            (b"BN", b"BN", "BBSaskName", 40),
            (b"TN", b"PN", "BBSaskTelnet", 40),
            (b"SS", b"BR", "BBSaskSSH", 40),
            (b"SN", b"SN", "BBSaskSysOp", 36),
            (b"SF", b"SF", "BBSaskSoftware", 12),
            (b"ST", b"ST", "BBSaskStorage", 20),
            (b"IN", b"IN", "BBSaskInfo", 75),
        ];
        let mut values: Vec<String> = Vec::new();
        for (i, (code, old, ask, len)) in fields.into_iter().enumerate() {
            let placed = self.goto_pos(*code) || self.goto_pos(*old);
            if !placed {
                self.say(ask).await?;
            }
            let at = (self.screen.where_x(), self.screen.where_y());
            let mut v = self.read_string(ReadOpts::new(len)).await?.trim().to_string();
            if !screen {
                self.newline();
            }
            // New: the description takes more lines (a pasted one comes in
            // whole), to a blank line, under the first.
            if *code == *b"IN" && !v.is_empty() {
                for k in 1..BBS_INFO_LINES {
                    if placed {
                        self.screen.goto(at.0, at.1 + k as u16);
                    } else {
                        self.say("BBSaskInfoMore").await?;
                    }
                    let more = self.read_string(ReadOpts::new(len)).await?.trim_end().to_string();
                    if !screen {
                        self.newline();
                    }
                    if more.trim().is_empty() {
                        break;
                    }
                    v.push('\n');
                    v.push_str(&more);
                }
            }
            if i == 0 && v.is_empty() {
                return Ok(());
            }
            if i == 1 {
                let list = self.state.sys.bbs_list().map_err(io_err)?;
                let name = values[0].clone();
                if list.iter().any(|x| x.name.eq_ignore_ascii_case(&name) || (!v.is_empty() && x.telnet.eq_ignore_ascii_case(&v))) {
                    if !self.goto_pos(*b"OT") {
                        self.newline();
                    }
                    self.say_line("BBSexists").await?;
                    return Ok(());
                }
            }
            values.push(v);
        }
        [b.name, b.telnet, b.ssh, b.sysop] = [values[0].clone(), values[1].clone(), values[2].clone(), values[3].clone()];
        [b.software, b.storage] = [values[4].clone(), values[5].clone()];
        b.info = if values[6].is_empty() { "None".into() } else { values[6].clone() };
        if !self.goto_pos(*b"OT") {
            self.newline();
        }
        self.say("BBSaskSaveBBS").await?;
        if self.yes_no(true).await? {
            self.say_line("BBSsavingEntry").await?;
            self.state.sys.add_bbs(&b).map_err(io_err)?;
            tracing::info!("{}: added {} to the BBS list", self.user.handle, b.name);
        }
        self.newline();
        Ok(())
    }

    /// `bbsListDeleteOne` (`BD`): `number;Ask`.
    pub async fn bbs_delete(&mut self, param: &str) -> io::Result<()> {
        let ask = param.to_ascii_uppercase().contains('A');
        let Some((_, b)) = self.pick_bbs(param, "BBSdeleteOne").await? else { return Ok(()) };
        if ask {
            self.say("BBSverifyKill").await?;
            if !self.yes_no(false).await? {
                return Ok(());
            }
        }
        self.state.sys.delete_bbs(b.id).map_err(io_err)?;
        tracing::info!("{}: removed {} from the BBS list", self.user.handle, b.name);
        Ok(())
    }

    /// `bbsListClear` (`BC`): `Ask` confirms first.
    pub async fn bbs_clear(&mut self, param: &str) -> io::Result<()> {
        if param.trim().to_ascii_uppercase().starts_with('A') {
            self.say("BBSclearAll").await?;
            if !self.yes_no(false).await? {
                return Ok(());
            }
        }
        self.state.sys.clear_bbs().map_err(io_err)
    }

    // ---- user configuration ----------------------------------------------------

    /// `userConfiguration` (`UC`). USERCFG.ANS marks each field with
    /// `~XY`: X is the field (A password .. O zip code; new: P SSH keys,
    /// Q the profile, R time zone, S text mode, T-V code and Markdown
    /// colors, W animations, X the screen saver, Y the email address, Z
    /// the editor's keys), Y the key that edits
    /// it. `~**` is where the command key is read.
    pub async fn user_config(&mut self) -> io::Result<()> {
        self.user_config_screen("USERCFG").await
    }

    /// The configuration screen with a given layout file (NEWCFG for new
    /// users checking their application).
    pub async fn user_config_screen(&mut self, screen: &str) -> io::Result<()> {
        self.full_screen_done();
        // Enter saves; Escape puts everything back (a password change,
        // confirmed on its own, stays).
        let (original, charset) = (self.user.clone(), self.screen.charset());
        // A SysOp's edit meanwhile waits until the screen's done, so Esc
        // puts back only what the caller changed here.
        let held = std::mem::replace(&mut self.hold_account_changes, true);
        let r = self.user_config_screen_in(screen, original, charset).await;
        self.hold_account_changes = held;
        r
    }

    async fn user_config_screen_in(&mut self, screen: &str, original: crate::user::User, charset: fluxbbs_term::Charset) -> io::Result<()> {
        loop {
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.clear_screen();
            if !self.show_file(screen).await? {
                return self.user_config_plain().await;
            }
            // Field letter -> (hotkey, position).
            let mut fields: Vec<(char, char, (u16, u16, fluxbbs_term::Color))> = Vec::new();
            for (code, pos) in self.positions.clone() {
                let (f, k) = (code[0] as char, code[1] as char);
                if f.is_ascii_uppercase() && k.is_ascii_alphanumeric() {
                    fields.push((f, k.to_ascii_uppercase(), pos));
                }
            }
            for (f, _, pos) in &fields {
                let v = self.user_field(*f);
                self.screen.goto(pos.0, pos.1);
                self.screen.set_color(pos.2);
                self.write_with_controls(&v);
            }
            if self.goto_pos(*b"*A") {
                let author = self.current_theme().map(|t| t.author).filter(|a| !a.is_empty()).unwrap_or_else(|| "n/a".into());
                self.write_with_controls(&author);
            }
            if !self.goto_pos(*b"**") {
                self.screen.goto(1, self.screen.rows());
            }
            let key = self.read_key().await?;
            if matches!(key, Key::Enter | Key::Esc) {
                if key == Key::Esc {
                    // The password, SSH keys and plan are saved as they change.
                    let pw = std::mem::take(&mut self.user.password_hash);
                    let keys = std::mem::take(&mut self.user.ssh_keys);
                    let plan = std::mem::take(&mut self.user.plan);
                    self.user = original;
                    self.apply_text_mode();
                    self.user.password_hash = pw;
                    self.user.ssh_keys = keys;
                    self.user.plan = plan;
                    self.screen.set_charset(charset);
                }
                self.save_user();
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            let Some(c) = key.upper() else { continue };
            let Some(&(f, _, pos)) = fields.iter().find(|(_, k, _)| *k == c) else { continue };
            self.screen.goto(pos.0, pos.1);
            self.screen.set_color(pos.2);
            self.edit_user_field(f).await?;
        }
    }

    fn user_field(&self, f: char) -> String {
        let u = &self.user;
        let yn = |b: bool| if b { "Yes" } else { "No " }.to_string();
        match f {
            'A' => "-hidden-".into(),
            'B' => u.phone.clone(),
            'C' => fmt_date(u.birth_date),
            'D' => squish(&u.location, 30),
            'E' => squish(&u.address, 30),
            'F' => squish(&u.note, 30),
            'G' => format!("{:<3}", u.page_length),
            'H' => yn(u.yes_no_bars),
            'I' => yn(u.expert),
            'J' => yn(u.hot_keys),
            'K' => yn(u.pause),
            'L' => yn(u.quote),
            'M' => self.emulation_name().into(),
            'N' => self.current_theme().map(|t| t.desc).unwrap_or_else(|| "n/a".into()),
            'O' => u.zip_code.clone(),
            // New: the profile (plan, header), for finger.
            'Q' => match (u.plan.len(), u.profile_header.is_empty()) {
                (0, true) => "none".into(),
                (n, true) => format!("plan, {n} line{}", if n == 1 { "" } else { "s" }),
                (0, false) => "header".into(),
                (n, false) => format!("plan ({n}), header"),
            },
            // New: SSH keys on file.
            'P' => match u.ssh_keys.len() {
                0 => "none".into(),
                1 => "1 key".into(),
                n => format!("{n} keys"),
            },
            // New: the time zone they see times in.
            'R' => crate::clock::Zone::parse(&u.time_zone).map_or_else(|| "board's".into(), |z| z.short()),
            // New: code's background, and the themes for code and Markdown.
            'T' => crate::markup::code_back(&u.code_bg).0.into(),
            'U' => crate::markup::theme(&u.code_theme).name.into(),
            'V' => crate::markup::theme(&u.md_theme).name.into(),
            // New: menus animating, or still.
            'W' => if u.animate == "off" { "off" } else { "on" }.into(),
            // New: their screen saver, or the board's.
            // New: their email address, and whether it's confirmed.
            'Y' if !self.email_on() => "off".into(),
            'Y' if u.mail_address.is_empty() => "none".into(),
            'Y' if u.mail_verified.is_empty() => format!("{} (unconfirmed)", squish(&u.mail_address, 14)),
            'Y' => squish(&u.mail_address, 28),
            // New: vim keys in the full-screen editor, or its own.
            'Z' => if u.editor_keys == "vim" { "vim" } else { "standard" }.into(),
            'X' => match crate::sysconfig::SAVERS.iter().find(|s| s.0 == u.saver) {
                Some(s) => s.0.into(),
                None => "board's".into(),
            },
            // New: plain text, ANSI, or as a bot or not (flag B).
            'S' => {
                let on = crate::plain::wants_plain(&u.text_mode, crate::aimemory::is_bot(u));
                match (u.text_mode.as_str(), on) {
                    ("", true) => "auto (plain)".into(),
                    ("", false) => "auto (ANSI)".into(),
                    (_, true) => "plain".into(),
                    _ => "ANSI".into(),
                }
            }
            _ => String::new(),
        }
    }

    async fn edit_user_field(&mut self, f: char) -> io::Result<()> {
        let text = |len| ReadOpts::new(len);
        match f {
            'A' => {
                self.change_password().await?;
                self.prompt_key().await?;
            }
            'B' => self.user.phone = self.read_string(text(13).prefill(self.user.phone.clone())).await?,
            'D' => self.user.location = self.read_string(text(40).case(Case::Mixed).prefill(self.user.location.clone())).await?,
            'E' => self.user.address = self.read_string(text(36).case(Case::Mixed).prefill(self.user.address.clone())).await?,
            'F' => self.user.note = self.read_string(text(40).prefill(self.user.note.clone())).await?,
            'G' => {
                let s = self.read_string(text(3).allowed(DIGITS).prefill(self.user.page_length.to_string())).await?;
                self.user.page_length = s.parse::<u16>().unwrap_or(self.user.page_length).clamp(5, 200);
            }
            'P' => self.ssh_keys_edit().await?,
            'Q' => self.profile_edit().await?,
            'R' => self.time_zone_edit().await?,
            // The next theme (blank is Nord).
            'U' | 'V' => {
                let cur = if f == 'U' { &self.user.code_theme } else { &self.user.md_theme };
                let themes = &crate::markup::THEMES;
                let at = themes.iter().position(|t| t.name == crate::markup::theme(cur).name).unwrap_or(1);
                let next = themes[(at + 1) % themes.len()].name;
                let v = if next == "nord" { String::new() } else { next.to_string() };
                if f == 'U' { self.user.code_theme = v } else { self.user.md_theme = v }
            }
            // The next background for code (blank is the first, the theme's).
            'T' => {
                let backs = crate::markup::CODE_BACKS;
                let cur = backs.iter().position(|b| b.0 == crate::markup::code_back(&self.user.code_bg).0).unwrap_or(0);
                let next = (cur + 1) % backs.len();
                self.user.code_bg = if next == 0 { String::new() } else { backs[next].0.into() };
            }
            // Plain text or ANSI, the other of what it is now; auto
            // (plain for bots) when that's what auto would give. It takes
            // effect at once.
            'S' => {
                let bot = crate::aimemory::is_bot(&self.user);
                let plain = !crate::plain::wants_plain(&self.user.text_mode, bot);
                self.user.text_mode = if plain == bot { "" } else if plain { "plain" } else { "ansi" }.into();
                self.apply_text_mode();
            }
            'W' => self.user.animate = if self.user.animate == "off" { String::new() } else { "off".into() },
            'Z' => self.user.editor_keys = if self.user.editor_keys == "vim" { String::new() } else { "vim".into() },
            // Change or confirm it (`UV`).
            #[cfg(feature = "email")]
            'Y' if self.email_on() => self.email_check("").await?,
            // The board's, then each saver in turn, then the board's again.
            'X' => {
                let names: Vec<&str> = std::iter::once("").chain(crate::sysconfig::SAVERS.iter().map(|s| s.0)).collect();
                let at = names.iter().position(|n| *n == self.user.saver).unwrap_or(0);
                self.user.saver = names[(at + 1) % names.len()].into();
            }
            'H' => self.user.yes_no_bars = !self.user.yes_no_bars,
            'I' => self.user.expert = !self.user.expert,
            'J' => self.user.hot_keys = !self.user.hot_keys,
            'K' => self.user.pause = !self.user.pause,
            'L' => self.user.quote = !self.user.quote,
            // Was ANSI/RIP/Avatar; today the choice that matters is the
            // character set, when detection guessed wrong.
            'M' => {
                let next = match self.screen.charset() {
                    Charset::Cp437 => Charset::Utf8,
                    Charset::Utf8 => Charset::Cp437,
                };
                self.screen.set_charset(next);
            }
            // The next theme (text library), as `fLibraryInstall` cycled.
            'N' => {
                let themes = self.state.themes();
                if !themes.list.is_empty() {
                    let cur = self.current_theme().and_then(|c| themes.list.iter().position(|t| t.dir == c.dir)).unwrap_or(0);
                    self.user.theme = themes.list[(cur + 1) % themes.list.len()].dir.clone();
                }
            }
            'O' => self.user.zip_code = self.read_string(text(10).case(Case::Upper).prefill(self.user.zip_code.clone())).await?,
            _ => {}
        }
        Ok(())
    }

    /// New: the caller's time zone, picked from a list or typed (a name,
    /// an abbreviation or an offset); blank for the board's.
    async fn time_zone_edit(&mut self) -> io::Result<()> {
        use crate::clock::{CHOICES, Zone};
        self.newline();
        let board = crate::clock::board();
        let head = self.strings().fmt("TimeZoneHead", &[&board.short(), &board.now().format("%-I:%M%P").to_string()]);
        self.show_line(&head).await?;
        // Two columns: number, place, and the time there now.
        let rows = CHOICES.len().div_ceil(2);
        for r in 0..rows {
            let mut line = String::new();
            for k in [r, r + rows] {
                if let Some((label, name)) = CHOICES.get(k) {
                    let now = Zone::parse(name).map(|z| z.now().format("%-I:%M%P").to_string()).unwrap_or_default();
                    line += &format!(" |U3{:>2}|U2) |U1{} |U6{} ", k + 1, resize(label, 22), resize(&now, 7));
                }
            }
            self.show_line(&line).await?;
        }
        self.say("TimeZoneAsk").await?;
        let typed = self.read_string(ReadOpts::new(32).case(Case::Mixed)).await?;
        let typed = typed.trim();
        if typed.is_empty() {
            self.user.time_zone.clear();
            return Ok(());
        }
        let picked = typed.parse::<usize>().ok().and_then(|n| CHOICES.get(n.checked_sub(1)?)).map(|(_, name)| name.to_string());
        match picked.or_else(|| Zone::parse(typed).map(|z| z.name())) {
            Some(name) => self.user.time_zone = name,
            None => {
                self.say_line("TimeZoneBad").await?;
                self.prompt_key().await?;
            }
        }
        Ok(())
    }

    /// Does the board ask for email addresses (built in and switched on)?
    fn email_on(&self) -> bool {
        cfg!(feature = "email") && self.state.config().email.enabled
    }

    /// User configuration without USERCFG.ANS.
    async fn user_config_plain(&mut self) -> io::Result<()> {
        let mut items = [
            ('A', 'P', "Password"),
            ('D', 'L', "Location"),
            ('E', 'A', "Address"),
            ('F', 'U', "User note"),
            ('G', 'S', "Screen length"),
            ('H', 'Y', "Yes/no bars"),
            ('I', 'X', "Expert mode"),
            ('J', 'H', "Hot keys"),
            ('K', 'W', "Screen pausing"),
            ('L', 'Q', "Message autoquote"),
            ('M', 'E', "Emulation"),
            ('N', 'T', "Theme"),
            ('R', 'Z', "Time zone"),
            ('S', 'B', "Text mode"),
            ('T', 'G', "Code background"),
            ('U', 'R', "Code theme"),
            ('V', 'M', "Markdown theme"),
            ('W', 'V', "Animations"),
            ('X', 'D', "Screen saver"),
            ('Z', 'I', "Editor keys"),
        ]
        .to_vec();
        // New: the email address, where the board asks for one.
        if self.email_on() {
            items.push(('Y', 'O', "Email address"));
        }
        loop {
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line("|U4 User configuration").await?;
            self.rule().await?;
            for &(f, k, name) in &items {
                let v = self.user_field(f);
                self.show_line(&format!(" |U3{k}|U2) |U1{} |U6{v}", resize(name, 18))).await?;
            }
            self.rule().await?;
            self.show("|U1Change which|U2? |U5[|U6enter|U5=|U4done|U5] |U3").await?;
            // Plain text answers with a line.
            let key = if self.plain() {
                let s = self.read_string(ReadOpts::new(2)).await?;
                s.chars().next().map_or(Key::Enter, |c| Key::Char(c as u8))
            } else {
                self.read_key().await?
            };
            self.newline();
            let Some(c) = key.upper().filter(|_| !matches!(key, Key::Enter | Key::Esc)) else { return Ok(()) };
            if let Some(&(f, _, name)) = items.iter().find(|(_, k, _)| *k == c) {
                if !"AHIJKLMNRSTUVWXYZ".contains(f) {
                    self.show(&format!("|U1{name}|U2: |U3")).await?;
                }
                self.edit_user_field(f).await?;
                self.newline();
                self.save_own_user().map_err(io_err)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_short_on_the_same_host() {
        let b = |t: &str, s: &str| Bbs { telnet: t.into(), ssh: s.into(), ..Bbs::default() };
        assert_eq!(ssh_short(&b("bbs.example.net:23", "BBS.example.net:2222")), ":2222");
        assert_eq!(ssh_short(&b("bbs.example.net", "bbs.example.net:22")), ":22");
        assert_eq!(ssh_short(&b("bbs.example.net:23", "other.net:22")), "other.net:22");
        assert_eq!(ssh_short(&b("", "")), "");
    }

    #[test]
    fn sig_is_clamped() {
        let long = "x".repeat(100);
        let lines = ["a", &long, "", "b", "c", "d", "e", "f"].map(String::from);
        let sig = clamp_sig(lines);
        assert_eq!(sig.len(), 6);
        assert_eq!(sig[1].len(), 80);
        assert_eq!(clamp_sig(["hi  ", "", " "].map(String::from)), vec!["hi"]);
    }

    #[test]
    fn sig_file_parsing() {
        let data = b"\r\n\x1b[1;36m-- \xfe me\r\nline two\n\x1aSAUCE00junk";
        assert_eq!(parse_sig_file(data), vec!["\x1b[1;36m-- ■ me", "line two"]);
        assert!(parse_sig_file(b"\r\n\r\n").is_empty());
    }
}
