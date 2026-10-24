//! The message centre (new): areas with what's new in them, threads, and
//! a full-screen reader (`MH`), and the logon check for new posts (`MW`).
//! The classic reader (`MR`, `MN`) and menus are still there.

use std::collections::{HashMap, HashSet};
use std::io;

use chrono::NaiveDateTime;
use fluxbbs_term::Slot;
use fluxbbs_term::codes::resize;

use crate::input::{DIGITS, Key, ReadOpts};
use crate::messages::{AreaRef, io_err};
use crate::msgbase::Message;
use crate::pushes::{HOT_HOURS, heats, score_label};
use crate::session::{BarPick, BarRow, BarSpec, Session, waves};

/// A conversation: a message and the replies under it.
pub struct Thread {
    /// (message row id, depth) in reading order; the first is the root.
    /// Ids, not numbers: a pack renumbers while a list is open.
    pub msgs: Vec<(i64, usize)>,
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
            Thread { msgs: order.into_iter().map(|(i, d)| (msgs[i].id, d)).collect(), last }
        })
        .collect()
}

/// The tree's lines in front of each of a thread's messages, from their
/// depths in reading order: `├─` for a reply with more after it, `└─` for
/// the last, `│ ` carrying a line down past a deeper branch. Only the
/// innermost `TREE_LEVELS` levels are drawn.
pub fn branches(depths: &[usize]) -> Vec<String> {
    const TREE_LEVELS: usize = 10;
    // Does level k still have a reply to come?
    let mut open: Vec<bool> = Vec::new();
    let mut out = Vec::with_capacity(depths.len());
    for (i, &d) in depths.iter().enumerate() {
        let last = depths[i + 1..].iter().find(|&&x| x <= d).is_none_or(|&x| x < d);
        let mut s = String::new();
        for k in (d.saturating_sub(TREE_LEVELS - 1)).max(1)..d {
            s += if open.get(k).copied().unwrap_or(false) { "│ " } else { "  " };
        }
        if d > 0 {
            s += if last { "└─" } else { "├─" };
        }
        open.resize(d + 1, false);
        open[d] = !last;
        out.push(s);
    }
    out
}

/// One area as a caller sees it: the messages they may read, and what
/// they've read.
pub(crate) struct AreaView {
    pub(crate) msgs: Vec<Message>,
    lastread: u32,
    seen: HashSet<i64>,
}

impl AreaView {
    pub(crate) fn is_new(&self, m: &Message) -> bool {
        m.num > self.lastread && !self.seen.contains(&m.id)
    }

    pub(crate) fn new_count(&self) -> usize {
        self.msgs.iter().filter(|m| self.is_new(m)).count()
    }

    /// The new messages, a thread at a time.
    pub(crate) fn new_in_thread_order(&self) -> Vec<i64> {
        let by_id = self.by_id();
        threads(&self.msgs).iter().flat_map(|t| t.msgs.iter().map(|&(n, _)| n)).filter(|&n| self_new(self, &by_id, n)).collect()
    }

    fn by_id(&self) -> HashMap<i64, &Message> {
        self.msgs.iter().map(|m| (m.id, m)).collect()
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

/// The message center's row for the Hot list.
const HOT_ROW: u32 = u32::MAX;
/// And for "my boards", the caller's favorites.
const FAV_ROW: u32 = u32::MAX - 1;

/// A board's figures in the message center's list; a folder's add up
/// everything under it.
#[derive(Default)]
struct Board {
    new: usize,
    msgs: usize,
    /// Posts this week.
    week: usize,
    /// Callers reading it now.
    now: usize,
    latest: Option<NaiveDateTime>,
    /// Who wrote the latest post.
    by: String,
}

impl Board {
    fn add(&mut self, o: &Board) {
        self.new += o.new;
        self.msgs += o.msgs;
        self.week += o.week;
        self.now += o.now;
        if o.latest > self.latest {
            self.latest = o.latest;
            self.by = o.by.clone();
        }
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
    pub(crate) fn area_view(&self, a: &AreaRef) -> io::Result<AreaView> {
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
        // Its number now (a pack renumbers), if it's still there.
        let Some(num) = self.state.msgs.num_of(m.id).map_err(io_err)? else { return Ok(()) };
        if num <= last {
            return Ok(());
        }
        self.state.msgs.mark_seen(self.user.id, m.id).map_err(io_err)?;
        self.state.msgs.mention_seen(self.user.id, m.id).map_err(io_err)?;
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
    /// Areas the caller may read; a passworded one only once they've
    /// given its password this call (the original's scans skipped the
    /// question altogether).
    pub(crate) fn readable_areas(&self) -> Vec<(u32, AreaRef)> {
        self.msg_area_labels()
            .into_iter()
            .filter_map(|(label, n)| {
                self.msg_area(n)
                    .filter(|a| self.area_open(a))
                    .map(|a| (label, a))
            })
            .collect()
    }

    /// [`Self::readable_areas`] less those taken out of newscans (`ME`).
    pub(crate) fn scanned_areas(&self) -> Vec<(u32, AreaRef)> {
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
            let since = crate::clock::now().naive_local() - chrono::Duration::days(30);
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
        let Some(t) = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|&(n, _)| n == m.id)) else { return Ok(()) };
        let seq: Vec<i64> = t.msgs.iter().map(|&(n, _)| n).collect();
        let at = seq.iter().position(|&n| n == m.id).unwrap_or(0);
        self.read_seq(&a, &seq, at).await?;
        Ok(())
    }

    // ---- the message centre ---------------------------------------------

    /// `MH`: the areas ("boards" to callers) with how many new messages
    /// each has, and keys to open one, read everything new, post, and (new)
    /// put the busiest first, search, help, or go to the message menu.
    /// MHUBTOP, MHUBMID, MHUBHIL and MHUBBOT dress it (`~AN` number, `~AT`
    /// name, `~DE` what it's for, `~RD` reading it now, `~NW` new, `~NM`
    /// messages, `~LP` and `~LD` the latest post's author and date, `~SP`
    /// sponsor, the SysOp's handle for "SysOp").
    pub async fn msg_center(&mut self) -> io::Result<()> {
        if self.plain() {
            return self.plain_boards().await;
        }
        // New: the folders opened on the way down (areas with sub-areas),
        // and the one just come back up from, for the bar.
        let mut path: Vec<u32> = Vec::new();
        let mut came_from: Option<u32> = None;
        loop {
            let level = path.last().copied();
            let readers = self.readers_now();
            let week = crate::clock::now().naive_local() - chrono::Duration::days(7);
            let readable = self.readable_areas();
            let labels: Vec<(u32, u32)> = readable.iter().map(|(l, a)| (*l, a.num)).collect();
            let tree: Vec<(u32, usize)> = self.state.areas().tree().into_iter().filter(|(n, _)| readable.iter().any(|(_, a)| a.num == *n)).collect();
            let mut stats: HashMap<u32, Board> = HashMap::new();
            for (_, a) in &readable {
                let v = self.area_view(a)?;
                let last = v.msgs.last();
                stats.insert(
                    a.num,
                    Board {
                        new: v.new_count(),
                        msgs: v.msgs.len(),
                        week: v.msgs.iter().filter(|m| m.posted >= week).count(),
                        now: readers.get(&a.key().to_ascii_uppercase()).copied().unwrap_or(0),
                        latest: last.map(|m| m.posted),
                        by: last.map(|m| m.from.name.clone()).unwrap_or_default(),
                    },
                );
            }
            let total_new: usize = stats.values().map(|b| b.new).sum();
            let label_of = |n: u32| labels.iter().find(|l| l.1 == n).map_or(0, |l| l.0);
            let suffix = self.strings().get("MsgGroupSuffix").to_string();
            let mut rows = Vec::new();
            // A folder's own posts, if it has any, come first in it.
            let mut shown: Vec<(u32, bool)> = Vec::new();
            if let Some(f) = level
                && stats.get(&f).is_some_and(|b| b.msgs > 0)
            {
                shown.push((f, false));
            }
            let has_kids = |n: u32| crate::fileareas::subtree_of(&tree, n).len() > 1;
            // New: the caller's favorites, the boards they can still read.
            let favs: Vec<u32> = self
                .user
                .fav_boards
                .split_whitespace()
                .filter_map(|k| readable.iter().find(|(_, a)| a.key().eq_ignore_ascii_case(k)).map(|(_, a)| a.num))
                .collect();
            let level_kids = if level == Some(FAV_ROW) { favs.clone() } else { crate::fileareas::children_of(&tree, level) };
            for c in level_kids {
                shown.push((c, has_kids(c)));
            }
            // (readers now, posts this week) for the busiest order.
            let mut pop: HashMap<u32, (usize, usize)> = HashMap::new();
            for (n, folder) in shown {
                let Some(a) = self.msg_area(n) else { continue };
                // A folder adds up everything under it.
                let under = if folder { crate::fileareas::subtree_of(&tree, n) } else { vec![n] };
                let mut b = Board::default();
                for m in &under {
                    if let Some(x) = stats.get(m) {
                        b.add(x);
                    }
                }
                pop.insert(n, (b.now, b.week));
                let name = if folder {
                    format!("{}{suffix}", a.area.name)
                } else if Some(n) == level {
                    self.strings().fmt("MsgFolderOwn", &[&a.area.name])
                } else {
                    a.area.name.clone()
                };
                let now_s = if b.now == 0 { String::new() } else { b.now.to_string() };
                let new_s = if b.new == 0 { "-".to_string() } else { b.new.to_string() };
                let when = b.latest.map(|t| self.local(t).format("%m/%d/%y").to_string()).unwrap_or_default();
                let mod_ = if a.area.sponsor.eq_ignore_ascii_case("sysop") { self.state.config().bbs.sysop_alias.clone() } else { a.area.sponsor.clone() };
                let label = label_of(n);
                let w = waves([
                    ("AN", label.to_string()),
                    ("AT", name.clone()),
                    ("DE", a.area.about.clone()),
                    ("RD", now_s.clone()),
                    ("NW", new_s.clone()),
                    ("NM", b.msgs.to_string()),
                    ("LP", b.by.clone()),
                    ("LD", when.clone()),
                    ("SP", mod_.clone()),
                ]);
                let plain = format!(
                    " {} {} {} {} {} {} {}",
                    resize(&label.to_string(), 4),
                    resize(&name, 16),
                    resize(&a.area.about, 24),
                    resize(&now_s, 3),
                    resize(&new_s, 4),
                    resize(&mod_, 12),
                    when
                );
                rows.push(BarRow { n, waves: w, plain });
            }
            // New: busiest first (`s`): readers now, then posts this week.
            if self.user.area_sort == "popular" {
                rows.sort_by_key(|r| std::cmp::Reverse(pop.get(&r.n).copied().unwrap_or_default()));
            }
            // New: "my boards" at the top, a folder of the favorites.
            if level.is_none() {
                let mut b = Board::default();
                for f in &favs {
                    for m in crate::fileareas::subtree_of(&tree, *f) {
                        if let Some(x) = stats.get(&m) {
                            b.add(x);
                        }
                    }
                }
                let name = format!("{}{suffix}", self.strings().get("MyBoardsName"));
                let about = self.strings().get("MyBoardsAbout").to_string();
                let now_s = if b.now == 0 { String::new() } else { b.now.to_string() };
                let new_s = if b.new == 0 { "-".to_string() } else { b.new.to_string() };
                let when = b.latest.map(|t| self.local(t).format("%m/%d/%y").to_string()).unwrap_or_default();
                let w = waves([("AN", "*".into()), ("AT", name.clone()), ("DE", about.clone()), ("RD", now_s.clone()), ("NW", new_s.clone()), ("NM", b.msgs.to_string()), ("LP", b.by.clone()), ("LD", when.clone()), ("SP", String::new())]);
                let plain = format!(" {} {} {} {} {} {} {}", resize("*", 4), resize(&name, 16), resize(&about, 24), resize(&now_s, 3), resize(&new_s, 4), resize("", 12), when);
                rows.insert(0, BarRow { n: FAV_ROW, waves: w, plain });
            }
            // New: the Hot list first, while anything's being voted on.
            let since = crate::clock::now().naive_local() - chrono::Duration::hours(HOT_HOURS);
            if level.is_none() && !self.state.msgs.recent_pushes(since).map_err(io_err)?.is_empty() {
                let name = self.strings().get("HotName").to_string();
                let about = self.strings().get("HotAbout").to_string();
                let w = waves([("AN", "!".into()), ("AT", name.clone()), ("DE", about.clone()), ("RD", String::new()), ("NW", String::new()), ("NM", String::new()), ("LP", String::new()), ("LD", String::new()), ("SP", String::new())]);
                rows.insert(0, BarRow { n: HOT_ROW, waves: w, plain: format!(" {} {} {}", resize("!", 4), resize(&name, 16), about) });
            }
            let want = came_from.take().unwrap_or(self.user.cur_msg_area);
            let start = rows
                .iter()
                .position(|r| r.n == want)
                .or_else(|| rows.iter().position(|r| self.state.areas().lineage(self.user.cur_msg_area).contains(&r.n)))
                .unwrap_or(0);
            let order = self.strings().get(if self.user.area_sort == "popular" { "AreaSortPopular" } else { "AreaSortNumber" }).to_string();
            let mut help = self.strings().fmt("MsgHubKeys", &[&total_new.to_string(), &order]);
            let in_name = if level == Some(FAV_ROW) { Some(self.strings().get("MyBoardsName").to_string()) } else { level.and_then(|f| self.msg_area(f)).map(|a| a.area.name) };
            if let Some(name) = in_name {
                help = self.strings().fmt("MsgHubIn", &[&fluxbbs_term::codes::squish(&name, 14)]) + &help;
            }
            let create = self.may_create_board();
            if create {
                help += self.strings().get("MsgHubCreate");
            }
            let spec = BarSpec {
                prefix: "MHUB",
                header: " #    Board            About                    Now New  Mod          Latest",
                rows: &rows,
                labels: &labels,
                start,
                help: &help,
                ask: "",
                can_go_up: level.is_some(),
                keys: if create { "NPSFHMCA*!" } else { "NPSFHMA*!" },
            };
            self.node.update(|n| n.activity = "Reading messages".into());
            let is_folder = |n: u32| n == FAV_ROW || (Some(n) != level && has_kids(n));
            match self.area_bar(&spec).await? {
                BarPick::Area(HOT_ROW) => {
                    if self.hot_list().await? {
                        return Ok(());
                    }
                }
                BarPick::Area(FAV_ROW) if favs.is_empty() => {
                    self.reader_prompt_area();
                    self.say_line("MyBoardsEmpty").await?;
                    self.prompt_key().await?;
                    came_from = Some(FAV_ROW);
                }
                BarPick::Area(n) if is_folder(n) => path.push(n),
                BarPick::Area(n) => {
                    if self.change_area(n).await?
                        && let Some(a) = self.msg_area(n)
                        && self.thread_list(&a).await?
                    {
                        return Ok(());
                    }
                }
                // `*` (my boards) and `!` (hot) from anywhere in the boards.
                BarPick::Key('*', _) if level != Some(FAV_ROW) => {
                    if favs.is_empty() {
                        self.reader_prompt_area();
                        self.say_line("MyBoardsEmpty").await?;
                        self.prompt_key().await?;
                    } else {
                        path = vec![FAV_ROW];
                    }
                }
                BarPick::Key('!', _) => {
                    if self.hot_list().await? {
                        return Ok(());
                    }
                }
                BarPick::Key('N', _) => {
                    self.read_all_new().await?;
                }
                BarPick::Key('P', n) if n != HOT_ROW && !is_folder(n) => {
                    if self.change_area(n).await?
                        && let Some(a) = self.msg_area(n)
                    {
                        self.post(&a, "All", "", None).await?;
                    }
                }
                BarPick::Key('S', _) => {
                    self.user.area_sort = if self.user.area_sort == "popular" { String::new() } else { "popular".into() };
                    self.save_own_user().map_err(io_err)?;
                }
                BarPick::Key('F', _) => self.msg_search("").await?,
                // New: add the board under the bar to "my boards", or take
                // it off.
                BarPick::Key('A', n) if n != HOT_ROW && n != FAV_ROW => {
                    if let Some(a) = self.msg_area(n) {
                        let key = a.key().to_string();
                        let mut keys: Vec<String> = self.user.fav_boards.split_whitespace().map(String::from).collect();
                        let had = keys.len();
                        keys.retain(|k| !k.eq_ignore_ascii_case(&key));
                        let added = keys.len() == had;
                        if added {
                            keys.push(key);
                        }
                        self.user.fav_boards = keys.join(" ");
                        self.save_own_user().map_err(io_err)?;
                        came_from = Some(n);
                        self.reader_prompt_area();
                        let s = self.strings().fmt(if added { "FavAdded" } else { "FavRemoved" }, &[&fluxbbs_term::lit(&a.area.name)]);
                        self.show_line(&s).await?;
                        self.prompt_key().await?;
                    }
                }
                BarPick::Key('H', _) => self.msg_help().await?,
                // New: a board of their own, opened once it's made.
                BarPick::Key('C', _) => {
                    if let Some(n) = self.create_board().await?
                        && self.change_area(n).await?
                        && let Some(a) = self.msg_area(n)
                    {
                        came_from = Some(n);
                        if self.thread_list(&a).await? {
                            return Ok(());
                        }
                    }
                }
                // The classic message menu (newscan settings and the rest).
                BarPick::Key('M', _) => {
                    self.menu_change('^', "MESSAGE");
                    return Ok(());
                }
                BarPick::Key(..) => {}
                BarPick::Quit => return Ok(()),
                // Back up out of a folder, or out of the boards.
                _ => match path.pop() {
                    Some(f) => came_from = Some(f),
                    None => return Ok(()),
                },
            }
            if self.hangup {
                return Ok(());
            }
        }
    }

    /// New: how many callers are reading each message area (by key, upper
    /// case) right now, PTT's popular boards; invisible ones only for SysOps.
    fn readers_now(&self) -> HashMap<String, usize> {
        let sysop = self.is_sysop();
        let mut n = HashMap::new();
        for (_, i) in self.state.nodes.online() {
            if !i.msg_area.is_empty() && (!i.invisible || sysop) {
                *n.entry(i.msg_area.to_ascii_uppercase()).or_insert(0) += 1;
            }
        }
        n
    }

    /// An area's threads, the latest activity first (or, new, the hottest
    /// or best pushed: `s`), a line each in PTT's order; new, threads its
    /// moderator pinned come first, and `t` shows only those with one of
    /// its tags. MTHRDTOP, MTHRDMID, MTHRDHIL and MTHRDBOT dress it (`~MN`
    /// the first message's number, `PinLabel` if pinned,
    /// `~MK` `*` if any are new, `~MT` subject, `~SN` who started it, `~RP`
    /// replies, `~SC` its score, `~NW` new, `~DS` the latest post's day
    /// (mm/dd), `~DT` and `~TI` its date and time).
    /// True if the caller quit the boards altogether (`q`).
    async fn thread_list(&mut self, a: &AreaRef) -> io::Result<bool> {
        let was = self.reading(a.key());
        let r = self.thread_list_in(a).await;
        self.reading(&was);
        r
    }

    /// Say which area this node is reading (empty: none); returns the
    /// one it was.
    pub(crate) fn reading(&self, key: &str) -> String {
        let mut was = String::new();
        self.node.update(|n| was = std::mem::replace(&mut n.msg_area, key.to_string()));
        was
    }

    async fn thread_list_in(&mut self, a: &AreaRef) -> io::Result<bool> {
        let mut on_root: Option<i64> = None;
        let mut a = a.clone();
        let mut tag: Option<String> = None;
        loop {
            // The moderator may have changed its tags or about line.
            if let Some(x) = self.msg_area(a.num).filter(|x| x.key() == a.key()) {
                a = x;
            }
            let a = &a;
            let v = self.area_view(a)?;
            let by_id = v.by_id();
            let scores = self.state.msgs.scores(a.key()).map_err(io_err)?;
            let comments = self.state.msgs.comment_counts(a.key()).map_err(io_err)?;
            let mut ts = threads(&v.msgs);
            ts.sort_by(|x, y| y.last.cmp(&x.last));
            match self.user.thread_sort.as_str() {
                "hot" => {
                    let heat = self.heat_now()?;
                    let of = |t: &Thread| t.msgs.iter().filter_map(|(n, _)| heat.get(n)).sum::<f64>();
                    ts.sort_by(|x, y| of(y).total_cmp(&of(x)));
                }
                "top" => ts.sort_by_key(|t| std::cmp::Reverse(scores.get(&t.msgs[0].0).copied().unwrap_or(0))),
                _ => {}
            }
            let pinned = self.state.msgs.pinned(a.key()).map_err(io_err)?;
            // A request board's open requests come before its filled ones.
            let filled = if a.area.requests { self.state.msgs.filled(a.key()).map_err(io_err)? } else { HashMap::new() };
            ts.sort_by_key(|t| (!pinned.contains(&t.msgs[0].0), filled.contains_key(&t.msgs[0].0)));
            if let Some(want) = &tag {
                ts.retain(|t| tag_of(&by_id[&t.msgs[0].0].subject).is_some_and(|x| x.eq_ignore_ascii_case(want)));
            }
            let pin_label = self.strings().get("PinLabel").to_string();
            let mut rows = Vec::new();
            let mut labels = Vec::new();
            let mut first_new = None;
            for (i, t) in ts.iter().enumerate() {
                let root = by_id[&t.msgs[0].0];
                let new = t.msgs.iter().filter(|(n, _)| self_new(&v, &by_id, *n)).count();
                if new > 0 && first_new.is_none() {
                    first_new = Some(i);
                }
                let new_s = if new == 0 { String::new() } else { format!("{new} new") };
                // Replies and comments alike (`r` comments, mostly).
                let replies = (t.msgs.len() - 1 + t.msgs.iter().map(|(n, _)| comments.get(n).copied().unwrap_or(0) as usize).sum::<usize>()).to_string();
                let score = score_label(scores.get(&root.id).copied());
                let mark = if new > 0 { "*" } else { " " };
                let day = self.local(t.last).format("%m/%d").to_string();
                let num = if pinned.contains(&root.id) { pin_label.clone() } else { root.num.to_string() };
                let (state, title) = self.request_state(a, root, &filled);
                let w = waves([
                    ("MN", num.clone()),
                    ("MK", mark.into()),
                    ("MT", title.clone()),
                    ("FL", state),
                    ("SN", root.from.name.clone()),
                    ("RP", replies.clone()),
                    ("SC", score.clone()),
                    ("NW", new_s.clone()),
                    ("DS", day.clone()),
                    ("DT", self.local(t.last).format("%m/%d/%y").to_string()),
                    ("TI", self.local(t.last).format("%H:%M").to_string()),
                ]);
                let plain = post_line(mark, &num, &score, &day, &root.from.name, &replies, &title);
                rows.push(BarRow { n: i as u32, waves: w, plain });
                labels.push((root.num, i as u32));
            }
            let start = on_root.and_then(|r| ts.iter().position(|t| t.msgs[0].0 == r)).or(first_new).unwrap_or(0);
            let sort = self.strings().get(match self.user.thread_sort.as_str() {
                "hot" => "ThreadSortHot",
                "top" => "ThreadSortTop",
                _ => "ThreadSortLatest",
            }).to_string();
            let moderator = self.is_sponsor(a) || self.is_cosysop();
            let tagged = !a.area.tags.is_empty();
            // What there's no room for falls off the end: least needed last.
            let mut more = String::new();
            if a.area.requests {
                more += self.strings().get("ReqFillKey");
            }
            if tagged {
                more += self.strings().get("ThreadTagKey");
            }
            if moderator {
                more += self.strings().get("MsgThreadModerate");
            }
            let new_count = v.new_count().to_string();
            // Room for the board's name (or the tag shown) on one line.
            let rest = crate::editor::strip_colours(&(self.strings().fmt("MsgThreadKeys", &["", &new_count, &sort]) + &more)).chars().count();
            let room = 74usize.saturating_sub(rest).clamp(8, 16);
            let name = match &tag {
                Some(t) => format!("[{t}]"),
                None => a.area.name.clone(),
            };
            let help = self.strings().fmt("MsgThreadKeys", &[&fluxbbs_term::lit(&fluxbbs_term::codes::squish(&name, room)), &new_count, &sort]) + &more;
            let header = post_line(" ", "#", "+/-", "Date", "Author", "Re", "Title");
            let mut keys = String::from("NPSHV");
            if moderator {
                keys.push('O');
            }
            if tagged {
                keys.push('T');
            }
            if a.area.requests {
                keys.push_str("F$");
            }
            let spec = BarSpec {
                prefix: "MTHRD",
                header: &header,
                rows: &rows,
                labels: &labels,
                start,
                help: &help,
                ask: "",
                can_go_up: true,
                keys: &keys,
            };
            match self.area_bar(&spec).await? {
                BarPick::Area(i) => {
                    let t = &ts[i as usize];
                    on_root = Some(t.msgs[0].0);
                    let seq: Vec<i64> = t.msgs.iter().map(|&(n, _)| n).collect();
                    let from = seq.iter().position(|&n| self_new(&v, &by_id, n)).unwrap_or(0);
                    if self.read_seq(a, &seq, from).await? == ReadEnd::Quit {
                        return Ok(true);
                    }
                }
                BarPick::Key('N', _) => {
                    let seq = v.new_in_thread_order();
                    if seq.is_empty() {
                        self.say_line("MsgNothingNew").await?;
                        self.prompt_key().await?;
                    } else if self.read_seq(a, &seq, 0).await? == ReadEnd::Quit {
                        return Ok(true);
                    }
                }
                BarPick::Key('P', _) => {
                    self.post(a, "All", "", None).await?;
                }
                BarPick::Key('V', n) => {
                    if let Some(t) = ts.get(n as usize) {
                        on_root = Some(t.msgs[0].0);
                        if self.thread_tree(a, t.msgs[0].0, None).await? {
                            return Ok(true);
                        }
                    }
                }
                BarPick::Key('H', _) => self.msg_help().await?,
                BarPick::Key('$', n) => {
                    on_root = ts.get(n as usize).map(|t| t.msgs[0].0);
                    self.top_fillers().await?;
                }
                BarPick::Key('F', n) => {
                    if let Some(root) = ts.get(n as usize).map(|t| by_id[&t.msgs[0].0]) {
                        on_root = Some(root.id);
                        let root = root.clone();
                        self.reader_prompt_area();
                        self.fill_request(a, &root, None).await?;
                    }
                }
                BarPick::Key('O', n) => {
                    let root = ts.get(n as usize).map(|t| t.msgs[0].0);
                    on_root = root;
                    if self.moderate_board(a, root.and_then(|r| by_id.get(&r).map(|m| (*m).clone()))).await? {
                        return Ok(false);
                    }
                }
                BarPick::Key('T', n) => {
                    on_root = ts.get(n as usize).map(|t| t.msgs[0].0);
                    self.reader_prompt_area();
                    tag = self.pick_tag(&a.area.tags, "TagPickShow", "TagAll").await?;
                }
                // Esc clears the tag before it leaves.
                BarPick::Stay | BarPick::Up if tag.is_some() => tag = None,
                BarPick::Key('S', n) => {
                    on_root = ts.get(n as usize).map(|t| t.msgs[0].0);
                    self.user.thread_sort = match self.user.thread_sort.as_str() {
                        "" => "hot",
                        "hot" => "top",
                        _ => "",
                    }
                    .into();
                    self.save_own_user().map_err(io_err)?;
                }
                BarPick::Quit => return Ok(true),
                _ => return Ok(false),
            }
            if self.hangup {
                return Ok(false);
            }
        }
    }

    /// New: a thread as a tree, who answered whom, a line a message in
    /// reading order, each with its comments under it (a comment's row
    /// leaves `~MN`, `~MT` and `~SC` empty; `~TX` is what it says). MTREETOP, MTREEMID, MTREEHIL and MTREEBOT dress it
    /// (`~MN` number, `~MK` `*` if new, `~TR` the branches, `~SN` author,
    /// `~TS` both, `~RT` who it answers, `~MT` subject, `~BL` its first
    /// words, `~TX` the subject where it changes and the first words
    /// otherwise, `~SC` score, `~DS` day (mm/dd), `~DT` and `~TI` date and
    /// time). Enter reads on from the bar; `r` replies. The bar starts on
    /// `on`, or the first new message. True if the caller quit (`q`).
    async fn thread_tree(&mut self, a: &AreaRef, root: i64, on: Option<i64>) -> io::Result<bool> {
        // Its screens name the area (`%MA`): this one, from a scan of all.
        let was = std::mem::replace(&mut self.user.cur_msg_area, a.num);
        let r = self.thread_tree_in(a, root, on).await;
        if self.user.cur_msg_area == a.num {
            self.user.cur_msg_area = was;
        }
        r
    }

    async fn thread_tree_in(&mut self, a: &AreaRef, root: i64, mut on: Option<i64>) -> io::Result<bool> {
        loop {
            let v = self.area_view(a)?;
            let by_id = v.by_id();
            let Some(t) = threads(&v.msgs).into_iter().find(|t| t.msgs[0].0 == root) else { return Ok(false) };
            let scores = self.state.msgs.scores(a.key()).map_err(io_err)?;
            let filled = if a.area.requests { self.state.msgs.filled(a.key()).map_err(io_err)? } else { HashMap::new() };
            let seq: Vec<i64> = t.msgs.iter().map(|&(n, _)| n).collect();
            // New: each message's comments, as branches under it (`r` is a
            // comment now, so they're most of a thread). A row is the
            // message it reads (its place in `seq`) and the comment, if
            // it's one.
            let mut entries: Vec<(usize, Option<crate::msgbase::Push>)> = Vec::new();
            let mut depths = Vec::new();
            for (i, &(id, d)) in t.msgs.iter().enumerate() {
                entries.push((i, None));
                depths.push(d);
                for p in self.state.msgs.pushes(id).map_err(io_err)?.into_iter().filter(|p| p.kind == 0) {
                    entries.push((i, Some(p)));
                    depths.push(d + 1);
                }
            }
            let lines = branches(&depths);
            let first = by_id[&root];
            let topic = |s: &str| s.trim_start_matches("Re:").trim_start_matches("RE:").trim().to_lowercase();
            let mut rows = Vec::new();
            let mut labels = Vec::new();
            for (row, ((i, comment), branch)) in entries.iter().zip(&lines).enumerate() {
                let m = by_id[&seq[*i]];
                if let Some(p) = comment {
                    let text = crate::editor::strip_colours(&p.text);
                    let day = self.local(p.at).format("%m/%d").to_string();
                    let who = format!("{branch}{}", p.handle);
                    let w = waves([
                        ("MN", String::new()),
                        ("MK", " ".into()),
                        ("TR", branch.clone()),
                        ("SN", p.handle.clone()),
                        ("TS", who.clone()),
                        ("RT", m.from.name.clone()),
                        ("MT", String::new()),
                        ("FL", String::new()),
                        ("BL", text.clone()),
                        ("TX", text.clone()),
                        ("SC", String::new()),
                        ("DS", day.clone()),
                        ("DT", self.local(p.at).format("%m/%d/%y").to_string()),
                        ("TI", self.local(p.at).format("%H:%M").to_string()),
                    ]);
                    rows.push(BarRow { n: row as u32, waves: w, plain: tree_line(" ", "", "", &day, &who, &text) });
                    continue;
                }
                let new = v.is_new(m);
                let mark = if new { "*" } else { " " };
                let words = crate::mci::blurb(m.body());
                let (state, subject) = if *i == 0 { self.request_state(a, m, &filled) } else { (String::new(), m.subject.clone()) };
                let text = if *i == 0 || topic(&m.subject) != topic(&first.subject) || words.is_empty() { subject.clone() } else { words.clone() };
                let to = match by_id.get(&m.reply_to) {
                    Some(p) => p.from.name.clone(),
                    None => m.to.name.clone(),
                };
                let score = score_label(scores.get(&m.id).copied());
                let day = self.local(m.posted).format("%m/%d").to_string();
                let who = format!("{branch}{}", m.from.name);
                let w = waves([
                    ("MN", m.num.to_string()),
                    ("MK", mark.into()),
                    ("TR", branch.clone()),
                    ("SN", m.from.name.clone()),
                    ("TS", who.clone()),
                    ("RT", to),
                    ("MT", subject),
                    ("FL", state),
                    ("BL", words),
                    ("TX", text.clone()),
                    ("SC", score.clone()),
                    ("DS", day.clone()),
                    ("DT", self.local(m.posted).format("%m/%d/%y").to_string()),
                    ("TI", self.local(m.posted).format("%H:%M").to_string()),
                ]);
                rows.push(BarRow { n: row as u32, waves: w, plain: tree_line(mark, &m.num.to_string(), &score, &day, &who, &text) });
                labels.push((m.num, row as u32));
            }
            // The rows of messages (not comments), as places in `seq`.
            let row_of = |i: usize| entries.iter().position(|(j, c)| *j == i && c.is_none()).unwrap_or(0);
            let start = on.and_then(|o| seq.iter().position(|&n| n == o)).or_else(|| seq.iter().position(|&n| self_new(&v, &by_id, n))).map_or(0, row_of);
            let help = self.strings().fmt("MsgTreeKeys", &[&fluxbbs_term::lit(&fluxbbs_term::codes::squish(&first.subject, 30))]);
            let header = tree_line(" ", "#", "+/-", "Date", "Who", "What");
            let spec = BarSpec { prefix: "MTREE", header: &header, rows: &rows, labels: &labels, start, help: &help, ask: "", can_go_up: true, keys: "R" };
            match self.area_bar(&spec).await? {
                // A comment reads (or answers) the message it's under.
                BarPick::Area(row) => {
                    let i = entries.get(row as usize).map_or(0, |e| e.0);
                    on = seq.get(i).copied();
                    if self.read_seq(a, &seq, i).await? == ReadEnd::Quit {
                        return Ok(true);
                    }
                }
                BarPick::Key('R', row) => {
                    let i = entries.get(row as usize).map_or(0, |e| e.0);
                    if let Some(m) = seq.get(i).and_then(|id| by_id.get(id)) {
                        on = Some(m.id);
                        let m = (*m).clone();
                        if self.comments_only(a) {
                            self.push_prompt(a, &m, true).await?;
                        } else {
                            self.post(a, "", "", Some(&m)).await?;
                        }
                    }
                }
                BarPick::Quit => return Ok(true),
                _ => return Ok(false),
            }
            if self.hangup {
                return Ok(false);
            }
        }
    }

    /// Each message's heat from the pushes of the last `HOT_HOURS`.
    fn heat_now(&self) -> io::Result<HashMap<i64, f64>> {
        let now = crate::clock::now().naive_local();
        let recent = self.state.msgs.recent_pushes(now - chrono::Duration::hours(HOT_HOURS)).map_err(io_err)?;
        Ok(heats(&recent, now))
    }

    /// The Hot list (new): threads being pushed lately in every area the
    /// caller reads, hottest first. MHOTTOP, MHOTMID, MHOTHIL and MHOTBOT
    /// dress it (`~AT` area, `~MK` `*` if any are new, `~MT` subject, `~SN`
    /// who started it, `~SC` score, `~NW` new, `~DS` and `~DT` the latest
    /// post's day and date).
    async fn hot_list(&mut self) -> io::Result<bool> {
        let mut on: Option<i64> = None;
        loop {
            let heat = self.heat_now()?;
            // (heat, area, thread's row ids, where to start, row)
            let mut hot: Vec<(f64, AreaRef, Vec<i64>, usize, BarRow)> = Vec::new();
            for (_, a) in self.readable_areas().into_iter().filter(|(_, a)| !a.is_email()) {
                let v = self.area_view(&a)?;
                if !v.msgs.iter().any(|m| heat.contains_key(&m.id)) {
                    continue;
                }
                let by_id = v.by_id();
                let scores = self.state.msgs.scores(a.key()).map_err(io_err)?;
                for t in threads(&v.msgs) {
                    let h: f64 = t.msgs.iter().filter_map(|(n, _)| heat.get(n)).sum();
                    if h <= 0.0 {
                        continue;
                    }
                    let root = by_id[&t.msgs[0].0];
                    let new = t.msgs.iter().filter(|(n, _)| self_new(&v, &by_id, *n)).count();
                    let new_s = if new == 0 { String::new() } else { format!("{new} new") };
                    let score = score_label(scores.get(&root.id).copied());
                    let mark = if new > 0 { "*" } else { " " };
                    let day = self.local(t.last).format("%m/%d").to_string();
                    let w = waves([
                        ("AT", a.area.name.clone()),
                        ("MK", mark.into()),
                        ("MT", root.subject.clone()),
                        ("SN", root.from.name.clone()),
                        ("SC", score.clone()),
                        ("NW", new_s.clone()),
                        ("DS", day.clone()),
                        ("DT", self.local(t.last).format("%m/%d/%y").to_string()),
                    ]);
                    let plain = hot_line(mark, &a.area.name, &score, &day, &root.from.name, &root.subject);
                    let seq: Vec<i64> = t.msgs.iter().map(|&(n, _)| n).collect();
                    let from = seq.iter().position(|&n| self_new(&v, &by_id, n)).unwrap_or(0);
                    hot.push((h, a.clone(), seq, from, BarRow { n: 0, waves: w, plain }));
                }
            }
            if hot.is_empty() {
                self.newline();
                self.say_line("HotNone").await?;
                self.prompt_key().await?;
                return Ok(false);
            }
            hot.sort_by(|x, y| y.0.total_cmp(&x.0));
            let mut bar = Vec::new();
            let mut picks = Vec::new();
            for (i, (_, a, seq, from, mut row)) in hot.into_iter().enumerate() {
                row.n = i as u32;
                bar.push(row);
                picks.push((a, seq, from));
            }
            let start = on.and_then(|id| picks.iter().position(|p| p.1[0] == id)).unwrap_or(0);
            let help = self.strings().get("HotKeys").to_string();
            let header = hot_line(" ", "Board", "+/-", "Date", "Author", "Title");
            let spec = BarSpec {
                prefix: "MHOT",
                header: &header,
                rows: &bar,
                labels: &[],
                start,
                help: &help,
                ask: "",
                can_go_up: true,
                keys: "H",
            };
            match self.area_bar(&spec).await? {
                BarPick::Area(i) => {
                    let Some((a, seq, from)) = picks.get(i as usize).cloned() else { continue };
                    on = Some(seq[0]);
                    if self.read_seq(&a, &seq, from).await? == ReadEnd::Quit {
                        return Ok(true);
                    }
                }
                BarPick::Key('H', _) => self.msg_help().await?,
                BarPick::Quit => return Ok(true),
                _ => return Ok(false),
            }
            if self.hangup {
                return Ok(false);
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
            if self.read_seq(&a, &[m.id], 0).await? == ReadEnd::Quit || self.hangup {
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
            let seq: Vec<i64> = v
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
                .map(|m| m.id)
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

    /// Read the messages with row ids `seq` in `a`, from `seq[at]`: the header
    /// (MSGHEAD) at the top, the text scrolling under it, and a line of
    /// keys at the bottom (`MsgReaderBar`).
    async fn read_seq(&mut self, a: &AreaRef, seq: &[i64], at: usize) -> io::Result<ReadEnd> {
        if !a.is_email() {
            self.seen.read.insert(crate::editor::strip_colours(&a.area.name));
        }
        let was = self.reading(a.key());
        let r = self.read_seq_in(a, seq, at).await;
        self.reading(&was);
        r
    }

    async fn read_seq_in(&mut self, a: &AreaRef, seq: &[i64], at: usize) -> io::Result<ReadEnd> {
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
                match self.state.msgs.by_id(seq[i]).map_err(io_err)?.filter(|m| m.area.eq_ignore_ascii_case(a.key()) && self.ok_to_read(a, m)) {
                    Some(m) => {
                        self.mark_read(a, &m)?;
                        if a.area.requests && m.from.user_id == self.user.id && self.user.id != 0 {
                            self.state.msgs.filled_told(m.id).map_err(io_err)?;
                        }
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
            let bar_lines = self.reader_bar_lines();
            let height = (rows + 1).saturating_sub(body_top + bar_lines).max(1) as usize;
            let last_scroll = lines.len().saturating_sub(height);
            for k in 0..height {
                self.screen.goto(1, body_top + k as u16);
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.screen.clear_eol();
                if let Some(l) = lines.get(scroll + k) {
                    self.reader_line(l);
                }
            }
            for y in rows + 1 - bar_lines..=rows {
                self.screen.goto(1, y);
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.screen.clear_eol();
            }
            self.screen.goto(1, rows + 1 - bar_lines);
            let note = if scroll < last_scroll {
                self.strings().get("MsgReaderMore").to_string()
            } else if attached {
                self.strings().get("MsgReaderAttached").to_string()
            } else if !crate::links::file_links(&self.link_lines(&m)).is_empty() {
                self.strings().fmt("MsgReaderFiles", &[&crate::links::file_links(&self.link_lines(&m)).len().to_string()])
            } else {
                String::new()
            };
            let bar = self.strings().fmt("MsgReaderBar", &[&format!("{}/{}", i + 1, seq.len()), &note]);
            // New: the board's prompt bar (MSGRBAR: `~HP` the keys, `~PC`
            // the cursor), as the lists have; else the keys alone.
            if bar_lines > 1 || self.text_file_exists("MSGRBAR") {
                self.show_file("MSGRBAR").await?;
                if self.goto_pos(*b"HP") {
                    let room = 80usize.saturating_sub(self.screen.where_x() as usize);
                    self.show(&fluxbbs_term::codes::squish(&bar, room)).await?;
                }
                self.goto_pos(*b"PC");
            } else {
                self.show(&bar).await?;
            }
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
                    self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                    self.screen.clear_eol();
                    self.say("MsgReaderGoto").await?;
                    let s = self.read_string(ReadOpts::new(6).allowed(DIGITS).prefill((c as char).to_string()).esc_cancels()).await?;
                    if let Some(x) = s.parse::<u32>().ok().and_then(|n| self.state.msgs.get(a.key(), n).ok().flatten()).filter(|x| self.ok_to_read(a, x)) {
                        if let Some(p) = seq.iter().position(|&id| id == x.id) {
                            i = p;
                            cur = None;
                        } else {
                            if Box::pin(self.read_seq(a, &[x.id], 0)).await? == ReadEnd::Quit {
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
                    // A reply is a comment under the message (one system,
                    // not two), but for email and echomail, where comments
                    // can't go.
                    Some('R') if self.comments_only(a) => {
                        if self.push_prompt(a, &m, true).await? {
                            let lines = self.reader_lines(a, &m);
                            scroll = lines.len().saturating_sub(height);
                            cur = Some((m, lines, attached));
                        }
                        // `/l`'s file picker took the screen.
                        redraw = true;
                    }
                    Some('R') => {
                        self.reader_prompt_area();
                        self.post(a, "", "", Some(&m)).await?;
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
                            if let Some(at) = seq.iter().position(|&x| x == p.id) {
                                i = at;
                                cur = None;
                            } else {
                                if Box::pin(self.read_seq(a, &[p.id], 0)).await? == ReadEnd::Quit {
                                    return Ok(ReadEnd::Quit);
                                }
                                cur = None;
                            }
                        }
                    }
                    Some('T') => {
                        // The whole conversation this message is in; already
                        // reading it, its tree.
                        let v = self.area_view(a)?;
                        if let Some(t) = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|&(n, _)| n == m.id)) {
                            let tseq: Vec<i64> = t.msgs.iter().map(|&(n, _)| n).collect();
                            let r = if tseq != seq {
                                let at = tseq.iter().position(|&n| n == m.id).unwrap_or(0);
                                Box::pin(self.read_seq(a, &tseq, at)).await? == ReadEnd::Quit
                            } else {
                                Box::pin(self.thread_tree(a, tseq[0], Some(m.id))).await?
                            };
                            if r {
                                return Ok(ReadEnd::Quit);
                            }
                            cur = None;
                            redraw = true;
                        }
                    }
                    // New: mark the request this thread makes filled (or open).
                    Some('M') if a.area.requests => {
                        let v = self.area_view(a)?;
                        let root = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|&(n, _)| n == m.id)).map(|t| t.msgs[0].0);
                        if let Some(r) = root.and_then(|r| v.msgs.iter().find(|x| x.id == r).cloned()) {
                            self.reader_prompt_area();
                            self.fill_request(a, &r, Some(&m)).await?;
                        }
                        cur = None;
                        redraw = true;
                    }
                    // New: the conversation as a tree.
                    Some('V') => {
                        let v = self.area_view(a)?;
                        let root = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|&(n, _)| n == m.id)).map(|t| t.msgs[0].0);
                        if let Some(r) = root
                            && Box::pin(self.thread_tree(a, r, Some(m.id))).await?
                        {
                            return Ok(ReadEnd::Quit);
                        }
                        cur = None;
                        redraw = true;
                    }
                    // New: your own post: take it back, retract or burn it.
                    Some('D') if !a.is_email() && self.wrote(&m.from) => {
                        self.reader_prompt_area();
                        self.regret(a, &m).await?;
                        cur = None;
                        redraw = true;
                    }
                    Some('D') if self.ok_to_delete(a, &m) => {
                        let mut x = m.clone();
                        x.deleted = !x.deleted;
                        self.state.msgs.set_deleted(x.id, x.deleted).map_err(io_err)?;
                        tracing::info!("{}: message {} in {} {}", self.user.handle, x.num, a.area.name, if x.deleted { "deleted" } else { "undeleted" });
                        cur = None;
                    }
                    Some('E') if self.ok_to_edit(a, &m) => {
                        let mut x = m.clone();
                        self.reader_prompt_area();
                        self.edit_existing(a, &mut x).await?;
                        cur = None;
                    }
                    // The attachment, and (new) the files the message links to.
                    Some('G') if attached || !crate::links::file_links(&self.link_lines(&m)).is_empty() => {
                        self.reader_prompt_area();
                        self.message_files(&m, attached).await?;
                        redraw = true;
                    }
                    // New: finger the author.
                    Some('F') if !m.anonymous => {
                        if let Some(u) = self.state.users.get(m.from.user_id).map_err(io_err)?.filter(|u| !u.deleted) {
                            self.finger_user(&u).await?;
                            redraw = true;
                        }
                    }
                    // New: push it (`pushes.rs`), and see the new line.
                    Some('X') => {
                        if self.push_prompt(a, &m, false).await? {
                            let lines = self.reader_lines(a, &m);
                            scroll = lines.len().saturating_sub(height);
                            cur = Some((m, lines, attached));
                        }
                        redraw = true;
                    }
                    Some('?' | 'H') => {
                        self.reader_help().await?;
                        redraw = true;
                    }
                    _ => {}
                },
            }
        }
    }

    /// New: pick one of an area's tags from a numbered list; None for
    /// Enter (`none` names what that means: "all" or "none").
    pub(crate) async fn pick_tag(&mut self, tags: &[String], head: &str, none: &str) -> io::Result<Option<String>> {
        self.newline();
        self.say_line(head).await?;
        for (i, tag) in tags.iter().enumerate() {
            let s = self.strings().fmt("TagItem", &[&(i + 1).to_string(), &fluxbbs_term::lit(tag)]);
            self.show_line(&s).await?;
        }
        let none = self.strings().get(none).to_string();
        let s = self.strings().fmt("TagPickAsk", &[&none]);
        self.show(&s).await?;
        let n = self.read_string(ReadOpts::new(2).allowed(DIGITS)).await?;
        self.newline();
        Ok(n.parse::<usize>().ok().and_then(|n| tags.get(n.wrapping_sub(1))).cloned())
    }

    /// Before a prompt from the reader: a clear screen to write on.
    /// New: replies here are comments under the message, except in email
    /// and echomail (comments stay on this board; a reply has to travel).
    pub(crate) fn comments_only(&self, a: &AreaRef) -> bool {
        !a.is_email() && !self.state.config().ftn.areas.iter().any(|f| f.area.eq_ignore_ascii_case(a.key()))
    }

    /// A request board's thread: its state for `~FL` (`ReqOpen` or
    /// `ReqFilled`) and its title, `ReqFilledTag` in front once filled.
    /// Elsewhere, nothing and the subject.
    pub(crate) fn request_state(&self, a: &AreaRef, root: &Message, filled: &HashMap<i64, crate::msgbase::Filled>) -> (String, String) {
        if !a.area.requests {
            return (String::new(), root.subject.clone());
        }
        if filled.contains_key(&root.id) {
            (self.strings().get("ReqFilled").to_string(), format!("{}{}", self.strings().get("ReqFilledTag"), root.subject))
        } else {
            (self.strings().get("ReqOpen").to_string(), root.subject.clone())
        }
    }

    /// New: mark a request filled, or open again. Only its asker (and
    /// co-SysOps, for one left behind) may: whoever answers it doesn't get
    /// to say it's done. They pick the answer that filled it (a comment, or
    /// in echomail a reply), whose author is credited; `on`, the message
    /// being read, is offered first when it's someone's reply. The note is
    /// the answer's file links, else asked for.
    async fn fill_request(&mut self, a: &AreaRef, root: &Message, on: Option<&Message>) -> io::Result<()> {
        if !(self.is_me(&root.from) || self.is_cosysop()) {
            self.say_line("ReqNotYours").await?;
            return self.prompt_key().await;
        }
        let s = self.strings().fmt("ReqHead", &[&fluxbbs_term::lit(&fluxbbs_term::codes::squish(&root.subject, 60))]);
        self.show_line(&s).await?;
        let now = crate::clock::now().naive_local();
        if self.state.msgs.filled_one(root.id).map_err(io_err)?.is_some() {
            self.say("ReqAskReopen").await?;
            if self.yes_no(false).await? {
                self.state.msgs.set_filled(root.id, None, now).map_err(io_err)?;
                tracing::info!("{}: request {} in {} open again", self.user.handle, root.num, a.area.name);
            }
            return Ok(());
        }
        // The answers: replies and comments in the thread, less the
        // asker's own.
        let v = self.area_view(a)?;
        let by_id = v.by_id();
        let thread: Vec<Message> = threads(&v.msgs)
            .into_iter()
            .find(|t| t.msgs[0].0 == root.id)
            .map(|t| t.msgs.iter().filter_map(|(id, _)| by_id.get(id).map(|m| (*m).clone())).collect())
            .unwrap_or_default();
        let asker = |id: u32, name: &str| (root.from.user_id != 0 && id == root.from.user_id) || name.eq_ignore_ascii_case(&root.from.name);
        let mut answers: Vec<Answer> = Vec::new();
        for m in &thread {
            if m.id != root.id && !asker(m.from.user_id, &m.from.name) {
                answers.push(Answer { who: m.from.name.clone(), who_id: if m.anonymous { 0 } else { m.from.user_id }, at: m.posted, text: m.lines.clone(), msg: m.id, push: 0 });
            }
            for p in self.state.msgs.pushes(m.id).map_err(io_err)?.into_iter().filter(|p| p.kind == 0 && !asker(p.user_id, &p.handle)) {
                answers.push(Answer { who: p.handle.clone(), who_id: p.user_id, at: p.at, text: vec![p.text.clone()], msg: m.id, push: p.id });
            }
        }
        answers.sort_by_key(|x| x.at);
        let mut pick = None;
        if let Some(i) = on.and_then(|m| answers.iter().position(|x| x.msg == m.id && x.push == 0)) {
            let s = self.strings().fmt("ReqAskThisReply", &[&fluxbbs_term::lit(&answers[i].who)]);
            self.show(&s).await?;
            if self.yes_no(true).await? {
                pick = Some(i);
            }
            self.newline();
        }
        if pick.is_none() && !answers.is_empty() {
            let none = self.strings().get("ReqNoneOfThese").to_string();
            let mut rows = vec![BarRow { n: 0, waves: waves([("SN", none.clone())]), plain: format!("  {none}") }];
            for (i, x) in answers.iter().enumerate() {
                let files = crate::links::file_links(&x.text).len();
                let words = if x.push != 0 { x.text.join(" ") } else { crate::mci::blurb(&x.text) };
                let what = if files > 0 { format!("[{files} file{}] {words}", if files == 1 { "" } else { "s" }) } else { words };
                let day = self.local(x.at).format("%m/%d").to_string();
                let w = waves([("SN", x.who.clone()), ("DS", day.clone()), ("TX", what.clone())]);
                rows.push(BarRow { n: i as u32 + 1, waves: w, plain: format!("  {} {} {}", resize(&x.who, 16), resize(&day, 5), resize(&what, 54)) });
            }
            let header = format!("  {} {} {}", resize("Who", 16), resize("Date", 5), "What");
            let help = self.strings().fmt("ReqPickKeys", &[&fluxbbs_term::lit(&fluxbbs_term::codes::squish(&root.subject, 30))]);
            let spec = BarSpec { prefix: "MFILL", header: &header, rows: &rows, labels: &[], start: 1, help: &help, ask: "", can_go_up: true, keys: "" };
            match self.area_bar(&spec).await? {
                BarPick::Area(0) => {}
                BarPick::Area(i) => pick = Some(i as usize - 1),
                _ => return Ok(()),
            }
            self.reader_prompt_area();
            self.show_line(&s).await?;
        }
        let answer = pick.and_then(|i| answers.get(i));
        let links: Vec<String> = answer.map(|x| crate::links::file_links(&x.text).iter().map(|(k, n)| crate::links::link_text(k, n)).collect()).unwrap_or_default();
        let note = if links.is_empty() {
            self.say("ReqAskNote").await?;
            let n = self.read_string(ReadOpts::new(60)).await?;
            self.newline();
            n.trim().to_string()
        } else {
            links.join(" ")
        };
        if self.hangup {
            return Ok(());
        }
        let handle = self.user.handle.clone();
        let (filler_id, filler, reply_id, push_id) = match answer {
            Some(x) if x.who_id != 0 => (x.who_id, x.who.clone(), x.msg, x.push),
            Some(x) => (0, String::new(), x.msg, x.push),
            None => (0, String::new(), 0, 0),
        };
        let mark = crate::msgbase::FillMark { by_id: self.user.id, by: &handle, filler_id, filler: &filler, reply_id, push_id, note: &note };
        self.state.msgs.set_filled(root.id, Some(mark), now).map_err(io_err)?;
        tracing::info!("{}: request {} in {} filled{}", self.user.handle, root.num, a.area.name, if filler.is_empty() { String::new() } else { format!(" by {filler}") });
        self.pay_fill(a, root, filler_id, now)
    }

    /// New: the filler's points (`fill_points`, and as many file points
    /// where the board uses them). Not to bots, nor to whoever marked it.
    fn pay_fill(&self, a: &AreaRef, root: &Message, filler_id: u32, now: NaiveDateTime) -> io::Result<()> {
        let pts = self.state.config().messages.fill_points;
        if filler_id == 0 || pts == 0 || filler_id == self.user.id || filler_id == root.from.user_id {
            return Ok(());
        }
        let Some(u) = self.state.users.get(filler_id).map_err(io_err)?.filter(|u| !u.deleted && !crate::aimemory::is_bot(u)) else { return Ok(()) };
        if !self.state.msgs.award_fill(root.id, (u.id, &u.handle), a.key(), &root.subject, pts, now).map_err(io_err)? {
            return Ok(());
        }
        if self.state.config().files.use_file_points {
            let mut paid = u.clone();
            paid.file_points = paid.file_points.saturating_add(pts);
            self.save_other_user(&u, &paid).map_err(io_err)?;
        }
        tracing::info!("{}: {pts} points to {} for request {} in {}", self.user.handle, u.handle, root.num, a.area.name);
        Ok(())
    }

    /// `MF` (new): the top fillers of requests, this week, this month and
    /// ever, side by side. FILLERS heads it if the board has one.
    pub async fn top_fillers(&mut self) -> io::Result<()> {
        self.reader_prompt_area();
        if !self.show_file("FILLERS").await? {
            self.say_line("FillTitle").await?;
        }
        if self.screen.where_x() != 1 {
            self.newline();
        }
        self.newline();
        let now = crate::clock::now().naive_local();
        let mut cols = Vec::new();
        for since in [Some(now - chrono::Duration::days(7)), Some(now - chrono::Duration::days(30)), None] {
            cols.push(self.state.msgs.fill_board(since, 10).map_err(io_err)?);
        }
        let heads = ["FillWeek", "FillMonth", "FillEver"].map(|s| self.strings().get(s).to_string());
        self.set_slot(fluxbbs_term::Slot::Info);
        self.write(&heads.iter().map(|h| format!("  {}", resize(h, 24))).collect::<String>());
        self.newline();
        self.set_slot(fluxbbs_term::Slot::TextLo);
        let head = format!("{:>2} {} {:>4} {:>5}", "#", resize("who", 10), "fill", "pts");
        self.write(&format!("  {}", [head.as_str(); 3].join("  ")));
        self.newline();
        for i in 0..10 {
            for col in &cols {
                self.write("  ");
                match col.get(i) {
                    Some((id, handle, fills, pts)) => {
                        self.set_slot(if *id == self.user.id { fluxbbs_term::Slot::InfoHi } else { fluxbbs_term::Slot::TextHi });
                        self.write(&format!("{:>2} {} {:>4} {:>5}", i + 1, resize(handle, 10), fills, pts));
                    }
                    None => self.write(&" ".repeat(24)),
                }
            }
            self.newline();
        }
        let (fills, pts) = self.state.msgs.fills_of(self.user.id).map_err(io_err)?;
        self.newline();
        let s = self.strings().fmt("FillYours", &[&fills.to_string(), &pts.to_string()]);
        self.show_line(&s).await?;
        self.prompt_key().await
    }

    /// Rows the reader's bottom takes: MSGRBAR's, else one.
    fn reader_bar_lines(&self) -> u16 {
        let Some(d) = self.find_text_file("MSGRBAR").and_then(|p| std::fs::read(p).ok()) else { return 1 };
        let breaks = d.iter().filter(|&&b| b == b'\n').count() as u16;
        (breaks + u16::from(!d.ends_with(b"\n"))).clamp(1, 4)
    }

    fn reader_prompt_area(&mut self) {
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
    }

    /// `h` in the message center: MSGGUIDE, or the reader's help.
    async fn msg_help(&mut self) -> io::Result<()> {
        self.reader_prompt_area();
        // It's shown from a menu too, which pauses after it: a screen
        // without its own pause (`|PA`) gets one here.
        let paused = self.find_text_file("MSGGUIDE").and_then(|p| std::fs::read(p).ok()).is_some_and(|d| d.windows(3).any(|w| w.eq_ignore_ascii_case(b"|PA")));
        if self.show_file("MSGGUIDE").await? {
            if !paused {
                self.prompt_key().await?;
            }
            return Ok(());
        }
        self.reader_help().await
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
    pub(crate) fn reader_lines(&self, a: &AreaRef, m: &Message) -> Vec<String> {
        let mut out = Vec::new();
        if m.anonymous && (self.is_sponsor(a) || self.is_cosysop()) {
            out.push(format!("|U5Anonymous message by: |U4{} |U5[{}]", m.from.alias, m.from.real_name));
        }
        // New: what a reply answers, so it reads on its own.
        if m.reply_to != 0
            && let Ok(Some(p)) = self.state.msgs.by_id(m.reply_to)
            && p.area.eq_ignore_ascii_case(a.key())
            && self.ok_to_read(a, &p)
        {
            let who = if p.anonymous { self.strings().get("Anonymous").to_string() } else { p.from.name.clone() };
            let words = fluxbbs_term::codes::squish(&crate::mci::blurb(p.body()), 50);
            out.push(self.strings().fmt("MsgReaderReplyTo", &[&p.num.to_string(), &fluxbbs_term::lit(&who), &fluxbbs_term::lit(&words)]));
            out.push(String::new());
        }
        // A retraction hides it from others; its author and moderators
        // still see what it said.
        if (self.wrote(&m.from) || self.is_sponsor(a) || self.is_cosysop())
            && let Ok(Some((subject, lines, _))) = self.state.msgs.retracted(m.id)
        {
            out.extend(m.lines.iter().cloned());
            out.push(self.strings().get("RetractedYouSee").to_string());
            out.push(fluxbbs_term::lit(&subject));
            out.push(String::new());
            out.extend(lines);
            out.extend(self.push_lines(m));
            return out;
        }
        if a.area.requests
            && let Ok(Some(f)) = self.state.msgs.filled_one(m.id)
        {
            let day = self.local(f.at).format("%m/%d/%y").to_string();
            let note = if f.note.is_empty() { String::new() } else { self.strings().fmt("ReqFilledNote", &[&fluxbbs_term::lit(&f.note)]) };
            let by = if f.filler.is_empty() { &f.handle } else { &f.filler };
            out.push(self.strings().fmt("ReqFilledLine", &[&day, &fluxbbs_term::lit(by), &note]));
            out.push(String::new());
        }
        // New: fenced code blocks shaded, and colored when they name their
        // language (not in plain text, which keeps the fences).
        let lines: Vec<Result<String, crate::markup::Line>> = match (!self.plain()).then(|| crate::markup::message_code_blocks(&m.lines, 79)).flatten() {
            Some(l) => l,
            None => m.lines.iter().cloned().map(Ok).collect(),
        };
        for line in &lines {
            let line = match line {
                Ok(l) => l,
                Err(code) => {
                    out.push(crate::markup::to_ansi(code, self.shade_sgr(), ""));
                    continue;
                }
            };
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
        out.extend(self.push_lines(m));
        out
    }

    pub(crate) fn reader_line(&mut self, l: &str) {
        if l.contains('\x1b') {
            self.screen.write_bytes(&crate::editor::ansi_bytes(l));
        } else {
            self.set_slot(Slot::TextHi);
            self.write_with_controls(&crate::editor::printable(l));
        }
    }
}

/// A line of the post list, PTT's order: number, score, date, author,
/// replies, then the title with `*` before it if anything's new.
fn post_line(mark: &str, num: &str, score: &str, day: &str, author: &str, replies: &str, title: &str) -> String {
    format!(
        "  {} {} {} {} {} {mark}{}",
        resize(num, 4),
        resize(score, 4),
        resize(day, 5),
        resize(author, 12),
        resize(replies, 3),
        resize(title, 41)
    )
}

/// An answer to a request: a reply, or a comment (`push`) under a
/// message of its thread.
struct Answer {
    who: String,
    /// 0: anonymous, or from elsewhere.
    who_id: u32,
    at: NaiveDateTime,
    text: Vec<String>,
    msg: i64,
    push: i64,
}

/// A line of a thread's tree.
fn tree_line(mark: &str, num: &str, score: &str, day: &str, who: &str, text: &str) -> String {
    format!("  {} {} {} {mark}{} {}", resize(num, 4), resize(score, 4), resize(day, 5), resize(who, 24), resize(text, 35))
}

/// A line of the Hot list: the post list's, with the board in front.
fn hot_line(mark: &str, board: &str, score: &str, day: &str, author: &str, title: &str) -> String {
    format!("  {} {} {} {} {mark}{}", resize(board, 13), resize(score, 4), resize(day, 5), resize(author, 12), resize(title, 38))
}

/// New: an area's tags as typed, commas between, without blanks or
/// repeats.
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',').map(|t| t.trim().trim_matches(|c| c == '[' || c == ']').trim()) {
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

/// The tag a title starts with (`[question] ...`), if any.
pub fn tag_of(subject: &str) -> Option<&str> {
    let rest = subject.trim_start().strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].trim()).filter(|t| !t.is_empty())
}

/// Whether message (row id) `n` is new to the caller.
fn self_new(v: &AreaView, by_id: &HashMap<i64, &Message>, n: i64) -> bool {
    by_id.get(&n).is_some_and(|m| v.is_new(m))
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
        assert_eq!(t[0].msgs, vec![(10, 0), (11, 1), (13, 2), (14, 1)]);
        assert_eq!(t[0].last, msgs[4].posted);
        assert_eq!(t[1].msgs, vec![(12, 0)]);
        assert_eq!(t[2].msgs, vec![(15, 0)]);
    }

    #[test]
    fn a_reply_can_only_follow_its_parent() {
        // A parent that comes later can't adopt it (no loops).
        let msgs = vec![msg(10, 1, 11, 1), msg(11, 2, 10, 2)];
        let t = threads(&msgs);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].msgs, vec![(10, 0), (11, 1)]);
    }

    #[test]
    fn tags_are_read_from_titles_and_settings() {
        assert_eq!(tag_of("[question] how do I"), Some("question"));
        assert_eq!(tag_of(" [ news ]x"), Some("news"));
        assert_eq!(tag_of("Re: [question] how"), None);
        assert_eq!(tag_of("[] empty"), None);
        assert_eq!(tag_of("no tag"), None);
        assert_eq!(parse_tags(" question, [news],, Question ,for sale"), ["question", "news", "for sale"]);
    }

    #[test]
    fn pins_belong_to_their_area() {
        let mb = crate::msgbase::MsgBase::open_memory().unwrap();
        let mut a = msg(0, 0, 0, 1);
        mb.add(&mut a).unwrap();
        let mut b = Message { area: "OTHER".into(), ..msg(0, 0, 0, 2) };
        mb.add(&mut b).unwrap();
        mb.set_pinned(a.id, true).unwrap();
        mb.set_pinned(b.id, true).unwrap();
        assert_eq!(mb.pinned("GEN").unwrap(), HashSet::from([a.id]));
        mb.set_pinned(a.id, false).unwrap();
        assert!(mb.pinned("GEN").unwrap().is_empty());
        mb.delete_area("OTHER").unwrap();
        assert!(mb.pinned("OTHER").unwrap().is_empty());
    }

    #[test]
    fn a_thread_drawn_as_a_tree() {
        // 1, replies 2 (with 3 under it, and 4 under 3) and 5.
        let b = branches(&[0, 1, 2, 3, 1, 2]);
        assert_eq!(b, vec!["", "├─", "│ └─", "│   └─", "└─", "  └─"]);
        assert_eq!(branches(&[0, 1, 1]), vec!["", "├─", "└─"]);
        assert_eq!(branches(&[0, 1, 2, 2]), vec!["", "└─", "  ├─", "  └─"]);
    }

    #[test]
    fn new_messages_come_a_thread_at_a_time() {
        let msgs = vec![msg(10, 1, 0, 1), msg(11, 2, 0, 2), msg(12, 3, 10, 3), msg(13, 4, 11, 4)];
        let v = AreaView { msgs, lastread: 1, seen: HashSet::from([13]) };
        assert_eq!(v.new_count(), 2);
        assert_eq!(v.new_in_thread_order(), vec![12, 11]);
    }
}
