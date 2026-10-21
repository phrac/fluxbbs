//! The message centre (new): areas with what's new in them, threads, and
//! a full-screen reader (`MH`), and the logon check for new posts (`MW`).
//! The classic reader (`MR`, `MN`) and menus are still there.

use std::collections::{HashMap, HashSet};
use std::io;

use chrono::NaiveDateTime;
use iniquity_term::Slot;
use iniquity_term::codes::resize;

use crate::input::{DIGITS, Key, ReadOpts};
use crate::messages::{AreaRef, io_err};
use crate::msgbase::Message;
use crate::session::{BarPick, BarRow, BarSpec, Session, waves};

/// A conversation: a message and the replies under it.
pub struct Thread {
    /// (message number, depth) in reading order; the first is the root.
    pub msgs: Vec<(u32, usize)>,
    pub last: NaiveDateTime,
}

/// Group one area's messages (in number order) into threads, oldest
/// first. A reply whose parent isn't among them starts its own.
pub fn threads(msgs: &[Message]) -> Vec<Thread> {
    let at: HashMap<i64, usize> = msgs.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
    let mut kids: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for (i, m) in msgs.iter().enumerate() {
        // A parent is always older; anything else can't make a loop.
        match at.get(&m.reply_to) {
            Some(&p) if p < i => kids.entry(p).or_default().push(i),
            _ => roots.push(i),
        }
    }
    roots
        .into_iter()
        .map(|r| {
            let mut order = Vec::new();
            let mut stack = vec![(r, 0)];
            while let Some((i, depth)) = stack.pop() {
                order.push((i, depth));
                if let Some(k) = kids.get(&i) {
                    stack.extend(k.iter().rev().map(|&c| (c, depth + 1)));
                }
            }
            let last = order.iter().map(|&(i, _)| msgs[i].posted).max().unwrap_or_default();
            Thread { msgs: order.into_iter().map(|(i, d)| (msgs[i].num, d)).collect(), last }
        })
        .collect()
}

/// One area as a caller sees it: the messages they may read, and what
/// they've read.
struct AreaView {
    msgs: Vec<Message>,
    lastread: u32,
    seen: HashSet<i64>,
}

impl AreaView {
    fn is_new(&self, m: &Message) -> bool {
        m.num > self.lastread && !self.seen.contains(&m.id)
    }

    fn new_count(&self) -> usize {
        self.msgs.iter().filter(|m| self.is_new(m)).count()
    }

    /// The new messages, a thread at a time.
    fn new_in_thread_order(&self) -> Vec<u32> {
        let by_num: HashMap<u32, &Message> = self.msgs.iter().map(|m| (m.num, m)).collect();
        threads(&self.msgs).iter().flat_map(|t| t.msgs.iter().map(|&(n, _)| n)).filter(|n| by_num.get(n).is_some_and(|m| self.is_new(m))).collect()
    }
}

/// The message menu's figures: the message base's, what's new for the
/// caller, and area names by key.
#[derive(Default)]
pub struct MsgFigures {
    pub base: crate::msgbase::MsgStats,
    pub unread: usize,
    pub names: HashMap<String, String>,
}

impl MsgFigures {
    /// The random message shown: one with words in it, if any has.
    pub fn dig(&self) -> Option<&Message> {
        self.base.random.iter().find(|m| !crate::mci::blurb(m.body()).is_empty()).or(self.base.random.first())
    }

    pub fn area_name(&self, key: &str) -> String {
        self.names.get(&key.to_ascii_uppercase()).cloned().unwrap_or_default()
    }
}

/// How the reader ended.
#[derive(PartialEq)]
enum ReadEnd {
    /// Back to where it was started from (or past the last message).
    Back,
    /// Q: stop reading altogether (ends "read everything new").
    Quit,
}

impl Session {
    fn area_view(&self, a: &AreaRef) -> io::Result<AreaView> {
        let msgs = self.state.msgs.all(a.key()).map_err(io_err)?.into_iter().filter(|m| self.ok_to_read(a, m)).collect();
        Ok(AreaView {
            msgs,
            lastread: self.state.msgs.lastread(self.user.id, a.key()).map_err(io_err)?,
            seen: self.state.msgs.seen(self.user.id, a.key()).map_err(io_err)?,
        })
    }

    /// A message has been read: mark it, and move lastread past every
    /// message now read in a row after it.
    pub(crate) fn mark_read(&self, a: &AreaRef, m: &Message) -> io::Result<()> {
        let key = a.key();
        let last = self.state.msgs.lastread(self.user.id, key).map_err(io_err)?;
        if m.num <= last {
            return Ok(());
        }
        self.state.msgs.mark_seen(self.user.id, m.id).map_err(io_err)?;
        let seen = self.state.msgs.seen(self.user.id, key).map_err(io_err)?;
        let total = self.state.msgs.count(key).map_err(io_err)?;
        let mut n = last;
        while n < total {
            match self.state.msgs.get(key, n + 1).map_err(io_err)? {
                Some(x) if self.ok_to_read(a, &x) && !seen.contains(&x.id) => break,
                _ => n += 1,
            }
        }
        if n > last {
            self.state.msgs.set_lastread(self.user.id, key, n).map_err(io_err)?;
            self.state.msgs.forget_seen(self.user.id, key, n).map_err(io_err)?;
        }
        Ok(())
    }

    /// The areas the caller can read, as (number shown, area).
    pub(crate) fn readable_areas(&self) -> Vec<(u32, AreaRef)> {
        self.msg_area_labels().into_iter().filter_map(|(label, n)| self.msg_area(n).filter(|a| self.acs(&a.area.acs)).map(|a| (label, a))).collect()
    }

    /// [`Self::readable_areas`] less those taken out of newscans (`ME`).
    fn scanned_areas(&self) -> Vec<(u32, AreaRef)> {
        let off = self.scan_off('M');
        self.readable_areas().into_iter().filter(|(_, a)| !off.contains(a.key())).collect()
    }

    // ---- the message menu's figures (new) --------------------------------

    /// The message menu's figures, worked out again once they're a minute
    /// old; `shown` keeps the ones on screen however old.
    pub(crate) fn msg_stats(&self, shown: bool) -> std::sync::Arc<MsgFigures> {
        let cached = self.msg_stats.borrow().as_ref().filter(|(at, _)| shown || at.elapsed().as_secs() < 60).map(|(_, s)| s.clone());
        cached.unwrap_or_else(|| {
            let areas = self.readable_areas();
            let keys: Vec<String> = areas.iter().filter(|(_, a)| !a.is_email()).map(|(_, a)| a.key().to_string()).collect();
            let since = chrono::Local::now().naive_local() - chrono::Duration::days(30);
            let base = self.state.msgs.stats(&keys, since).unwrap_or_default();
            let unread = self.scanned_areas().iter().filter_map(|(_, a)| self.area_view(a).ok()).map(|v| v.new_count()).sum();
            let names = areas.iter().map(|(_, a)| (a.key().to_ascii_uppercase(), a.area.name.clone())).collect();
            let s = std::sync::Arc::new(MsgFigures { base, unread, names });
            *self.msg_stats.borrow_mut() = Some((std::time::Instant::now(), s.clone()));
            s
        })
    }

    /// `MX` (new), from the message menu's figures: `newest`, `hot`, `dig`
    /// or `mouth` (the top poster's latest) opens that message in its
    /// thread; `loud` the loudest area's threads; `unread` reads
    /// everything new.
    pub async fn msg_jump(&mut self, param: &str) -> io::Result<()> {
        let f = self.msg_stats(true);
        let what = param.trim().to_ascii_lowercase();
        if what == "unread" {
            if self.read_all_new().await? == 0 {
                self.newline();
                self.say_line("MsgNothingNew").await?;
            }
            return Ok(());
        }
        let areas: Vec<AreaRef> = self.readable_areas().into_iter().map(|(_, a)| a).collect();
        let find = |key: &str| areas.iter().find(|a| a.key().eq_ignore_ascii_case(key)).cloned();
        if what == "loud" {
            let Some(a) = f.base.loudest.as_ref().and_then(|(key, _)| find(key)) else { return Ok(()) };
            if self.change_area(a.num).await? {
                self.thread_list(&a).await?;
            }
            return Ok(());
        }
        let m = match what.as_str() {
            "newest" => f.base.newest.as_ref(),
            "hot" => f.base.hot.as_ref(),
            "dig" => f.dig(),
            "mouth" => f.base.mouth_latest.as_ref(),
            _ => None,
        };
        let Some((m, a)) = m.and_then(|m| find(&m.area).map(|a| (m, a))) else { return Ok(()) };
        let v = self.area_view(&a)?;
        let Some(t) = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|&(n, _)| n == m.num)) else { return Ok(()) };
        let seq: Vec<u32> = t.msgs.iter().map(|&(n, _)| n).collect();
        let at = seq.iter().position(|&n| n == m.num).unwrap_or(0);
        self.read_seq(&a, &seq, at).await?;
        Ok(())
    }

    // ---- the message centre ---------------------------------------------

    /// `MH`: the areas with how many new messages each has, the latest
    /// post, and keys to open one, read everything new, or post. MHUBTOP,
    /// MHUBMID, MHUBHIL and MHUBBOT dress it (`~AN` number, `~AT` name,
    /// `~NW` new, `~NM` messages, `~LP` and `~LD` the latest post's author
    /// and date, `~SP` sponsor).
    pub async fn msg_center(&mut self) -> io::Result<()> {
        loop {
            let mut rows = Vec::new();
            let mut labels = Vec::new();
            let mut total_new = 0;
            for (label, a) in self.readable_areas() {
                let v = self.area_view(&a)?;
                let new = v.new_count();
                total_new += new;
                let (who, when) = v.msgs.last().map(|m| (m.from.name.clone(), m.posted.format("%m/%d/%y").to_string())).unwrap_or_default();
                let new_s = if new == 0 { "-".to_string() } else { new.to_string() };
                let w = waves([
                    ("AN", label.to_string()),
                    ("AT", a.area.name.clone()),
                    ("NW", new_s.clone()),
                    ("NM", v.msgs.len().to_string()),
                    ("LP", who.clone()),
                    ("LD", when.clone()),
                    ("SP", a.area.sponsor.clone()),
                ]);
                let plain = format!(
                    " {} {} {} {}  {} {}",
                    resize(&label.to_string(), 4),
                    resize(&a.area.name, 30),
                    resize(&new_s, 5),
                    resize(&v.msgs.len().to_string(), 6),
                    resize(&who, 16),
                    when
                );
                rows.push(BarRow { n: a.num, waves: w, plain });
                labels.push((label, a.num));
            }
            let start = rows.iter().position(|r| r.n == self.user.cur_msg_area).unwrap_or(0);
            let help = self.strings().fmt("MsgHubKeys", &[&total_new.to_string()]);
            let spec = BarSpec {
                prefix: "MHUB",
                header: " #    Area                           New   Msgs    Latest post",
                rows: &rows,
                labels: &labels,
                start,
                help: &help,
                ask: "",
                can_go_up: false,
                keys: "NP",
            };
            self.node.update(|n| n.activity = "Reading messages".into());
            match self.area_bar(&spec).await? {
                BarPick::Area(n) => {
                    if self.change_area(n).await?
                        && let Some(a) = self.msg_area(n)
                    {
                        self.thread_list(&a).await?;
                    }
                }
                BarPick::Key('N', _) => {
                    self.read_all_new().await?;
                }
                BarPick::Key('P', n) => {
                    if self.change_area(n).await?
                        && let Some(a) = self.msg_area(n)
                    {
                        self.post(&a, "All", "", None).await?;
                    }
                }
                _ => return Ok(()),
            }
            if self.hangup {
                return Ok(());
            }
        }
    }

    /// An area's threads, the latest activity first. MTHRDTOP, MTHRDMID,
    /// MTHRDHIL and MTHRDBOT dress it (`~MN` the first message's number,
    /// `~MT` subject, `~SN` who started it, `~RP` replies, `~NW` new, `~DT`
    /// and `~TI` the latest post's date and time).
    async fn thread_list(&mut self, a: &AreaRef) -> io::Result<()> {
        let mut on_root: Option<u32> = None;
        loop {
            let v = self.area_view(a)?;
            let by_num: HashMap<u32, &Message> = v.msgs.iter().map(|m| (m.num, m)).collect();
            let mut ts = threads(&v.msgs);
            ts.sort_by(|x, y| y.last.cmp(&x.last));
            let mut rows = Vec::new();
            let mut labels = Vec::new();
            let mut first_new = None;
            for (i, t) in ts.iter().enumerate() {
                let root = by_num[&t.msgs[0].0];
                let new = t.msgs.iter().filter(|(n, _)| self_new(&v, &by_num, *n)).count();
                if new > 0 && first_new.is_none() {
                    first_new = Some(i);
                }
                let new_s = if new == 0 { String::new() } else { format!("{new} new") };
                let replies = (t.msgs.len() - 1).to_string();
                let w = waves([
                    ("MN", root.num.to_string()),
                    ("MT", root.subject.clone()),
                    ("SN", root.from.name.clone()),
                    ("RP", replies.clone()),
                    ("NW", new_s.clone()),
                    ("DT", t.last.format("%m/%d/%y").to_string()),
                    ("TI", t.last.format("%-I:%M%P").to_string()),
                ]);
                let plain = format!(
                    " {} {} {} {} {} {}",
                    resize(&root.num.to_string(), 5),
                    resize(&root.subject, 32),
                    resize(&root.from.name, 15),
                    resize(&replies, 3),
                    resize(&new_s, 7),
                    t.last.format("%m/%d/%y")
                );
                rows.push(BarRow { n: i as u32, waves: w, plain });
                labels.push((root.num, i as u32));
            }
            let start = on_root.and_then(|r| ts.iter().position(|t| t.msgs[0].0 == r)).or(first_new).unwrap_or(0);
            let help = self.strings().fmt("MsgThreadKeys", &[&a.area.name, &v.new_count().to_string()]);
            let spec = BarSpec {
                prefix: "MTHRD",
                header: " #     Subject                          Started by      Re  New     Latest",
                rows: &rows,
                labels: &labels,
                start,
                help: &help,
                ask: "",
                can_go_up: true,
                keys: "NP",
            };
            match self.area_bar(&spec).await? {
                BarPick::Area(i) => {
                    let t = &ts[i as usize];
                    on_root = Some(t.msgs[0].0);
                    let seq: Vec<u32> = t.msgs.iter().map(|&(n, _)| n).collect();
                    let from = seq.iter().position(|&n| self_new(&v, &by_num, n)).unwrap_or(0);
                    self.read_seq(a, &seq, from).await?;
                }
                BarPick::Key('N', _) => {
                    let seq = v.new_in_thread_order();
                    if seq.is_empty() {
                        self.say_line("MsgNothingNew").await?;
                        self.prompt_key().await?;
                    } else {
                        self.read_seq(a, &seq, 0).await?;
                    }
                }
                BarPick::Key('P', _) => {
                    self.post(a, "All", "", None).await?;
                }
                _ => return Ok(()),
            }
            if self.hangup {
                return Ok(());
            }
        }
    }

    /// Everything new in every area, a thread at a time. Returns how many
    /// messages there were.
    pub(crate) async fn read_all_new(&mut self) -> io::Result<usize> {
        let mut read = 0;
        for (_, a) in self.scanned_areas() {
            let seq = self.area_view(&a)?.new_in_thread_order();
            if seq.is_empty() {
                continue;
            }
            read += seq.len();
            if self.read_seq(&a, &seq, 0).await? == ReadEnd::Quit || self.hangup {
                break;
            }
        }
        if read == 0 {
            self.newline();
            self.say_line("MsgNothingNew").await?;
            self.prompt_key().await?;
        }
        Ok(read)
    }

    /// `MW` (at logon): say how many new posts there are, and offer to read
    /// them.
    pub async fn logon_new_check(&mut self) -> io::Result<()> {
        // New: posts the caller was @mentioned in come first.
        let mentioned = self.mentions_to_read(true)?.len();
        if mentioned > 0 {
            self.newline();
            let s = self.strings().fmt("MentionLogon", &[&mentioned.to_string()]);
            self.show(&s).await?;
            if self.yes_no(true).await? {
                self.read_mentions("").await?;
            }
            self.newline();
        }
        let (mut posts, mut areas) = (0, 0);
        for (_, a) in self.scanned_areas() {
            let n = self.area_view(&a)?.new_count();
            if n > 0 {
                posts += n;
                areas += 1;
            }
        }
        if posts == 0 {
            return Ok(());
        }
        self.newline();
        let s = self.strings().fmt("MsgLogonNew", &[&posts.to_string(), &areas.to_string()]);
        self.show(&s).await?;
        if self.yes_no(true).await? {
            self.read_all_new().await?;
        }
        self.newline();
        Ok(())
    }

    // ---- @mentions (new) ---------------------------------------------------

    /// The messages the caller was @mentioned in that they may read, with
    /// their areas; only unseen ones if `unseen`.
    fn mentions_to_read(&self, unseen: bool) -> io::Result<Vec<(AreaRef, Message)>> {
        let areas = self.readable_areas();
        let found = self.state.msgs.mentions(self.user.id, unseen).map_err(io_err)?;
        Ok(found
            .into_iter()
            .filter_map(|m| {
                let a = areas.iter().map(|(_, a)| a).find(|a| a.key().eq_ignore_ascii_case(&m.area))?.clone();
                self.ok_to_read(&a, &m).then_some((a, m))
            })
            .collect())
    }

    /// `M@` (new): read the posts you were @mentioned in that you haven't
    /// seen, newest first (`All`: the last hundred, seen or not); `t` in
    /// the reader shows the whole thread.
    pub async fn read_mentions(&mut self, param: &str) -> io::Result<()> {
        let all = param.trim().to_ascii_uppercase().starts_with('A');
        let found = self.mentions_to_read(!all)?;
        if found.is_empty() {
            self.newline();
            return self.say_line("MentionNone").await;
        }
        for (a, m) in found {
            self.state.msgs.mention_seen(self.user.id, m.id).map_err(io_err)?;
            if self.read_seq(&a, &[m.num], 0).await? == ReadEnd::Quit || self.hangup {
                break;
            }
        }
        Ok(())
    }

    // ---- searching (2.0's `MO`) -------------------------------------------

    /// `MO` (2.0): `All` or an area number; `F`rom, `T`o, `S`ubject, `A`ll
    /// text or `Y`our messages; the text; and `All` to search every
    /// message rather than just new ones. What's missing is asked for.
    /// Matches open in the reader, an area at a time.
    pub async fn msg_search(&mut self, param: &str) -> io::Result<()> {
        let mut p = param.split(';').map(str::trim);
        let (where_, kind, text, every) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
        self.newline();
        self.show_file("MSGSRCH").await?;
        let areas: Vec<(u32, AreaRef)> = if where_.to_ascii_uppercase().starts_with('A') {
            self.readable_areas()
        } else if let Ok(n) = where_.parse::<u32>() {
            self.readable_areas().into_iter().filter(|(l, _)| *l == n).collect()
        } else {
            self.say("MsgSearchAskAll").await?;
            if self.yes_no(false).await? { self.readable_areas() } else { self.readable_areas().into_iter().filter(|(_, a)| a.num == self.user.cur_msg_area).collect() }
        };
        let kind = match kind.chars().next().map(|c| c.to_ascii_uppercase()) {
            Some(c @ ('F' | 'T' | 'S' | 'A' | 'Y')) => c,
            _ => {
                self.say("MsgSearchAskType").await?;
                let k = self.read_string(ReadOpts::new(1).case(crate::input::Case::Upper).allowed("FTSAY")).await?;
                self.newline();
                match k.chars().next() {
                    Some(c) => c,
                    None => return Ok(()),
                }
            }
        };
        let text = if kind == 'Y' || !text.is_empty() {
            text.to_string()
        } else {
            self.say("MsgSearchAskText").await?;
            let t = self.read_string(ReadOpts::new(40)).await?;
            self.newline();
            if t.is_empty() {
                return Ok(());
            }
            t
        };
        let new_only = if every.is_empty() {
            self.say("MsgSearchAskNew").await?;
            self.yes_no(false).await?
        } else {
            !every.to_ascii_uppercase().starts_with('A')
        };
        let q = text.to_lowercase();
        let has = |s: &str| s.to_lowercase().contains(&q);
        let mut found = 0;
        for (_, a) in areas {
            let s = self.strings().fmt("MsgSearchArea", &[&a.area.name]);
            self.show_line(&s).await?;
            let v = self.area_view(&a)?;
            let seq: Vec<u32> = v
                .msgs
                .iter()
                .filter(|m| !new_only || v.is_new(m))
                .filter(|m| match kind {
                    'F' => has(&m.from.name),
                    'T' => has(&m.to.name),
                    'S' => has(&m.subject),
                    'Y' => self.is_me(&m.from) || self.is_me(&m.to),
                    _ => has(&m.subject) || has(&m.from.name) || has(&m.to.name) || m.body().iter().any(|l| has(l)),
                })
                .map(|m| m.num)
                .collect();
            if seq.is_empty() {
                continue;
            }
            found += seq.len();
            if self.read_seq(&a, &seq, 0).await? == ReadEnd::Quit || self.hangup {
                break;
            }
        }
        if found == 0 {
            self.say_line("MsgSearchNone").await?;
        }
        Ok(())
    }

    // ---- the reader -----------------------------------------------------

    /// Read the messages numbered `seq` in `a`, from `seq[at]`: the header
    /// (MSGHEAD) at the top, the text scrolling under it, and a line of
    /// keys at the bottom (`MsgReaderBar`).
    async fn read_seq(&mut self, a: &AreaRef, seq: &[u32], at: usize) -> io::Result<ReadEnd> {
        if seq.is_empty() {
            return Ok(ReadEnd::Back);
        }
        let mut i = at.min(seq.len() - 1);
        let mut forward = true;
        let mut cur: Option<(Message, Vec<String>, bool)> = None;
        let (mut scroll, mut body_top) = (0usize, 1u16);
        let mut redraw = true;
        loop {
            if self.hangup {
                return Ok(ReadEnd::Quit);
            }
            if cur.is_none() {
                match self.state.msgs.get(a.key(), seq[i]).map_err(io_err)?.filter(|m| self.ok_to_read(a, m)) {
                    Some(m) => {
                        self.mark_read(a, &m)?;
                        self.menu_msg = m.num;
                        let lines = self.reader_lines(a, &m);
                        let attached = self.state.msgs.attachment(m.id).map_err(io_err)?.is_some();
                        cur = Some((m, lines, attached));
                        scroll = 0;
                        redraw = true;
                    }
                    // Gone (deleted, or packed away) since the list was made.
                    None => {
                        let next = if forward { i + 1 } else { i.wrapping_sub(1) };
                        if next >= seq.len() {
                            return Ok(ReadEnd::Back);
                        }
                        i = next;
                        continue;
                    }
                }
            }
            let Some((m, lines, attached)) = cur.clone() else { continue };
            let rows = self.screen.rows();
            if redraw {
                let total = self.state.msgs.count(a.key()).map_err(io_err)?;
                self.msg_header(a, &m, m.num, total).await?;
                if self.screen.where_x() != 1 {
                    self.newline();
                }
                body_top = self.screen.where_y().min(rows.saturating_sub(3)).max(1);
                redraw = false;
            }
            let height = rows.saturating_sub(body_top).max(1) as usize;
            let last_scroll = lines.len().saturating_sub(height);
            for k in 0..height {
                self.screen.goto(1, body_top + k as u16);
                self.screen.set_color(iniquity_term::Color::DEFAULT);
                self.screen.clear_eol();
                if let Some(l) = lines.get(scroll + k) {
                    self.reader_line(l);
                }
            }
            self.screen.goto(1, rows);
            self.screen.set_color(iniquity_term::Color::DEFAULT);
            self.screen.clear_eol();
            let note = if scroll < last_scroll {
                self.strings().get("MsgReaderMore").to_string()
            } else if attached {
                self.strings().get("MsgReaderAttached").to_string()
            } else {
                String::new()
            };
            let bar = self.strings().fmt("MsgReaderBar", &[&format!("{}/{}", i + 1, seq.len()), &note]);
            self.show(&bar).await?;
            let key = self.read_key().await?;
            let next = |i: usize| (i + 1 < seq.len()).then_some(i + 1);
            match key {
                Key::Up => scroll = scroll.saturating_sub(1),
                Key::Down => scroll = (scroll + 1).min(last_scroll),
                Key::PgUp => scroll = scroll.saturating_sub(height),
                Key::Home => scroll = 0,
                Key::End => scroll = last_scroll,
                // Space, Enter and PgDn page through a message, then on.
                Key::PgDn | Key::Enter | Key::Char(b' ') if scroll < last_scroll => scroll = (scroll + height).min(last_scroll),
                Key::PgDn | Key::Enter | Key::Char(b' ') | Key::Right => match next(i) {
                    Some(n) => {
                        i = n;
                        forward = true;
                        cur = None;
                    }
                    None => return Ok(ReadEnd::Back),
                },
                Key::Left => {
                    if i > 0 {
                        i -= 1;
                        forward = false;
                        cur = None;
                    }
                }
                Key::Esc => return Ok(ReadEnd::Back),
                Key::Char(c) if c.is_ascii_digit() => {
                    // A message number: in this list, or read on its own.
                    self.screen.goto(1, rows);
                    self.screen.set_color(iniquity_term::Color::DEFAULT);
                    self.screen.clear_eol();
                    self.say("MsgReaderGoto").await?;
                    let s = self.read_string(ReadOpts::new(6).allowed(DIGITS).prefill((c as char).to_string())).await?;
                    if let Ok(n) = s.parse::<u32>() {
                        if let Some(p) = seq.iter().position(|&x| x == n) {
                            i = p;
                            cur = None;
                        } else if self.state.msgs.get(a.key(), n).map_err(io_err)?.is_some_and(|x| self.ok_to_read(a, &x)) {
                            if Box::pin(self.read_seq(a, &[n], 0)).await? == ReadEnd::Quit {
                                return Ok(ReadEnd::Quit);
                            }
                            cur = None;
                        }
                    }
                    redraw = true;
                }
                k => match k.upper() {
                    Some('N' | '+') => match next(i) {
                        Some(n) => {
                            i = n;
                            forward = true;
                            cur = None;
                        }
                        None => return Ok(ReadEnd::Back),
                    },
                    Some('-' | 'B') if i > 0 => {
                        i -= 1;
                        forward = false;
                        cur = None;
                    }
                    Some('L') => return Ok(ReadEnd::Back),
                    Some('Q') => return Ok(ReadEnd::Quit),
                    Some('R') => {
                        self.reader_prompt_area();
                        if self.post(a, "", "", Some(&m)).await?.is_some()
                            && let Some(mut p) = self.state.msgs.get(a.key(), m.num).map_err(io_err)?
                        {
                            p.replies += 1;
                            self.state.msgs.update(&p).map_err(io_err)?;
                        }
                        cur = None;
                        redraw = true;
                    }
                    Some('P') => {
                        self.reader_prompt_area();
                        self.post(a, "All", "", None).await?;
                        redraw = true;
                    }
                    Some('U') if m.reply_to != 0 => {
                        if let Some(p) = self.state.msgs.by_id(m.reply_to).map_err(io_err)?.filter(|p| p.area == m.area && self.ok_to_read(a, p)) {
                            if let Some(at) = seq.iter().position(|&x| x == p.num) {
                                i = at;
                                cur = None;
                            } else {
                                if Box::pin(self.read_seq(a, &[p.num], 0)).await? == ReadEnd::Quit {
                                    return Ok(ReadEnd::Quit);
                                }
                                cur = None;
                            }
                        }
                    }
                    Some('T') => {
                        // The whole conversation this message is in.
                        let v = self.area_view(a)?;
                        if let Some(t) = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|&(n, _)| n == m.num)) {
                            let tseq: Vec<u32> = t.msgs.iter().map(|&(n, _)| n).collect();
                            if tseq != seq {
                                let at = tseq.iter().position(|&n| n == m.num).unwrap_or(0);
                                if Box::pin(self.read_seq(a, &tseq, at)).await? == ReadEnd::Quit {
                                    return Ok(ReadEnd::Quit);
                                }
                                cur = None;
                            }
                        }
                    }
                    Some('D') if self.ok_to_delete(a, &m) => {
                        let mut x = m.clone();
                        x.deleted = !x.deleted;
                        self.state.msgs.update(&x).map_err(io_err)?;
                        tracing::info!("{}: message {} in {} {}", self.user.handle, x.num, a.area.name, if x.deleted { "deleted" } else { "undeleted" });
                        cur = None;
                    }
                    Some('E') if self.ok_to_edit(a, &m) => {
                        let mut x = m.clone();
                        self.reader_prompt_area();
                        self.edit_existing(a, &mut x).await?;
                        cur = None;
                    }
                    Some('G') if attached => {
                        self.reader_prompt_area();
                        self.offer_attachment(&m).await?;
                        redraw = true;
                    }
                    // New: finger the author.
                    Some('F') if !m.anonymous => {
                        if let Some(u) = self.state.users.get(m.from.user_id).map_err(io_err)?.filter(|u| !u.deleted) {
                            self.finger_user(&u).await?;
                            redraw = true;
                        }
                    }
                    Some('?') => {
                        self.reader_help().await?;
                        redraw = true;
                    }
                    _ => {}
                },
            }
        }
    }

    /// Before a prompt from the reader: a clear screen to write on.
    fn reader_prompt_area(&mut self) {
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
    }

    async fn reader_help(&mut self) -> io::Result<()> {
        self.reader_prompt_area();
        if self.show_file("MSGRHELP").await? {
            return self.prompt_key().await;
        }
        self.newline();
        for k in 1.. {
            let name = format!("MsgReaderHelp{k}");
            if self.strings().stock(&name).is_empty() {
                break;
            }
            let line = self.strings().get(&name).to_string();
            self.show_line(&line).await?;
        }
        self.newline();
        self.prompt_key().await
    }

    /// The text as the reader shows it: quotes and tear lines coloured,
    /// long plain lines wrapped to the screen.
    fn reader_lines(&self, a: &AreaRef, m: &Message) -> Vec<String> {
        let mut out = Vec::new();
        if m.anonymous && (self.is_sponsor(a) || self.is_cosysop()) {
            out.push(format!("|U5Anonymous message by: |U4{} |U5[{}]", m.from.alias, m.from.real_name));
        }
        for line in &m.lines {
            if line.contains('\x1b') {
                out.push(line.clone());
                continue;
            }
            let pieces = if line.chars().count() > 79 && !line.contains('|') { crate::multinode::wrap(line, 79) } else { vec![line.clone()] };
            for l in pieces {
                let quote = l.chars().take(3).any(|c| c == '>');
                out.push(if quote {
                    format!("|U1{l}|U3")
                } else if l.starts_with("---") && (l.len() == 3 || l.as_bytes()[3] == b' ') {
                    format!("|U2{l}")
                } else {
                    l
                });
            }
        }
        out
    }

    fn reader_line(&mut self, l: &str) {
        if l.contains('\x1b') {
            self.screen.write_bytes(&iniquity_term::cp437::encode(l));
        } else {
            self.set_slot(Slot::TextHi);
            self.write_with_controls(l);
        }
    }
}

/// Whether message `n` is new to the caller.
fn self_new(v: &AreaView, by_num: &HashMap<u32, &Message>, n: u32) -> bool {
    by_num.get(&n).is_some_and(|m| v.is_new(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msgbase::Party;

    fn msg(id: i64, num: u32, reply_to: i64, day: u32) -> Message {
        Message {
            id,
            area: "GEN".into(),
            num,
            from: Party::default(),
            to: Party::default(),
            subject: format!("m{num}"),
            posted: chrono::NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(12, 0, 0).unwrap(),
            deleted: false,
            private: false,
            anonymous: false,
            replies: 0,
            sig_pos: 0,
            tag: 0,
            reply_to,
            lines: Vec::new(),
        }
    }

    #[test]
    fn replies_nest_under_their_parents() {
        // 1 <- 2 <- 4, 1 <- 5; 3 alone; 6 answers a message that's gone.
        let msgs = vec![msg(10, 1, 0, 1), msg(11, 2, 10, 2), msg(12, 3, 0, 3), msg(13, 4, 11, 4), msg(14, 5, 10, 5), msg(15, 6, 99, 6)];
        let t = threads(&msgs);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].msgs, vec![(1, 0), (2, 1), (4, 2), (5, 1)]);
        assert_eq!(t[0].last, msgs[4].posted);
        assert_eq!(t[1].msgs, vec![(3, 0)]);
        assert_eq!(t[2].msgs, vec![(6, 0)]);
    }

    #[test]
    fn a_reply_can_only_follow_its_parent() {
        // A parent that comes later can't adopt it (no loops).
        let msgs = vec![msg(10, 1, 11, 1), msg(11, 2, 10, 2)];
        let t = threads(&msgs);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].msgs, vec![(1, 0), (2, 1)]);
    }

    #[test]
    fn new_messages_come_a_thread_at_a_time() {
        let msgs = vec![msg(10, 1, 0, 1), msg(11, 2, 0, 2), msg(12, 3, 10, 3), msg(13, 4, 11, 4)];
        let v = AreaView { msgs, lastread: 1, seen: HashSet::from([13]) };
        assert_eq!(v.new_count(), 2);
        assert_eq!(v.new_in_thread_order(), vec![3, 2]);
    }
}
