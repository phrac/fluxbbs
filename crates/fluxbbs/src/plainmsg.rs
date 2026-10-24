//! New: the message boards in plain text (`plain.rs`): line commands in
//! place of the message center's bars and the reader's keys.

use std::collections::HashMap;
use std::io;

use crate::editor::strip_colours;
use crate::input::ReadOpts;
use crate::messages::AreaRef;
use crate::msgbase::{Message, Push};
use crate::msgcenter::{AreaView, threads};
use crate::pushes::{PUSH_GAP, PUSH_WIDTH, score_label};
use crate::session::Session;

/// Threads a page of `l`.
const PAGE: usize = 20;
/// Messages one `n` shows.
const NEW_MAX: usize = 20;

const BOARDS_HELP: &[&str] = &[
    "  <number>       open that board",
    "  l              list the boards again",
    "  n              read what's new on every board (20 at a time)",
    "  q              back to the menu",
];

const BOARD_HELP: &[&str] = &[
    "  l [page]       the threads, latest first (20 a page)",
    "  r <#>          read message #",
    "  t <#>          the thread message # is in",
    "  n              read what's new here (20 at a time)",
    "  p              start a thread",
    "  reply <#>      reply to message # (where replies are posts)",
    "  c <#> <text>   comment on message #",
    "  + <#> <text>   vote message # up, with a word",
    "  - <#> <text>   vote message # down, with a word",
    "  q              back to the boards",
];

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

impl Session {
    /// A line of text, its color codes dropped.
    fn plain_line(&mut self, s: &str) {
        self.write(&strip_colours(s));
        self.newline();
    }

    /// Ask with a prompt; the line typed, split into a lower-case command
    /// and the rest.
    async fn plain_ask(&mut self, prompt: &str) -> io::Result<(String, String)> {
        self.write(&format!("{prompt}> "));
        let s = self.read_string(ReadOpts::new(PUSH_WIDTH + 20)).await?;
        self.newline();
        let s = s.trim();
        let (cmd, rest) = s.split_once(' ').unwrap_or((s, ""));
        Ok((cmd.to_ascii_lowercase(), rest.trim().to_string()))
    }

    /// The message boards: a list, then commands a line at a time.
    pub(crate) async fn plain_boards(&mut self) -> io::Result<()> {
        let mut list = true;
        loop {
            let areas = self.readable_areas();
            if list {
                self.plain_board_list(&areas)?;
                list = false;
            }
            let (cmd, _) = self.plain_ask("boards").await?;
            if self.hangup {
                return Ok(());
            }
            match cmd.as_str() {
                "q" => return Ok(()),
                "l" => list = true,
                "?" | "h" => {
                    for l in BOARDS_HELP {
                        self.plain_line(l);
                    }
                }
                "n" => self.plain_read_new(None).await?,
                "" => {}
                n => match n.parse::<u32>().ok().and_then(|n| areas.iter().find(|(label, _)| *label == n)) {
                    Some((_, a)) => {
                        let a = a.clone();
                        if self.change_area(a.num).await? && self.plain_board(&a).await? {
                            return Ok(());
                        }
                    }
                    None => self.plain_line("No such board. l lists them, ? the commands."),
                },
            }
        }
    }

    fn plain_board_list(&mut self, areas: &[(u32, AreaRef)]) -> io::Result<()> {
        let mut total_new = 0;
        let mut rows = Vec::new();
        for (label, a) in areas {
            let v = self.area_view(a)?;
            let new = v.new_count();
            total_new += new;
            // Sub-boards indented under their folders.
            let depth = self.state.areas().lineage(a.num).len().saturating_sub(1);
            let name = format!("{}{}", "  ".repeat(depth), strip_colours(&a.area.name));
            let latest = v.msgs.last().map(|m| self.local(m.posted).format("%m/%d").to_string()).unwrap_or_default();
            rows.push(format!("{label:>4}  {name:<28} {:>5} posts {new:>4} new  {latest}", v.msgs.len()));
        }
        self.newline();
        self.plain_line(&format!("Message boards: {total_new} new. A number opens one; ? for the commands."));
        for r in rows {
            self.plain_line(&r);
        }
        Ok(())
    }

    /// One board. True if the caller quit the boards altogether.
    async fn plain_board(&mut self, a: &AreaRef) -> io::Result<bool> {
        let was = self.reading(a.key());
        let r = self.plain_board_in(a).await;
        self.reading(&was);
        r
    }

    async fn plain_board_in(&mut self, a: &AreaRef) -> io::Result<bool> {
        let name = strip_colours(&a.area.name);
        let prompt = name.to_lowercase();
        {
            let v = self.area_view(a)?;
            let about = if a.area.about.is_empty() { String::new() } else { format!(": {}", strip_colours(&a.area.about)) };
            self.newline();
            self.plain_line(&format!("{name}{about}. {} threads, {} new. ? for the commands.", threads(&v.msgs).len(), v.new_count()));
        }
        self.plain_threads(a, 1)?;
        loop {
            let (cmd, rest) = self.plain_ask(&prompt).await?;
            if self.hangup {
                return Ok(true);
            }
            let mut words = rest.splitn(2, ' ');
            let num = words.next().and_then(|w| w.trim_start_matches('#').parse::<u32>().ok());
            let text = words.next().unwrap_or("").trim().to_string();
            match cmd.as_str() {
                "q" => return Ok(false),
                "?" | "h" => {
                    for l in BOARD_HELP {
                        self.plain_line(l);
                    }
                }
                "l" => self.plain_threads(a, num.unwrap_or(1).max(1) as usize)?,
                "n" => self.plain_read_new(Some(a)).await?,
                "p" => {
                    if let Some(n) = self.post(a, "All", "", None).await? {
                        self.plain_line(&format!("Posted as #{n}."));
                    }
                }
                "r" | "t" | "reply" | "c" | "+" | "-" => {
                    let Some(m) = num.and_then(|n| self.plain_msg(a, n)) else {
                        self.plain_line("Which message? Give its number, e.g. r 12.");
                        continue;
                    };
                    match cmd.as_str() {
                        "r" => self.plain_show(a, &m)?,
                        "t" => self.plain_thread(a, &m)?,
                        "reply" if self.comments_only(a) => self.plain_line("Replies here are comments: c <#> <text>."),
                        "reply" => {
                            if let Some(n) = self.post(a, "", "", Some(&m)).await? {
                                self.plain_line(&format!("Posted as #{n}."));
                            }
                        }
                        "c" => self.plain_push(a, &m, 0, text).await?,
                        "+" => self.plain_push(a, &m, 1, text).await?,
                        _ => self.plain_push(a, &m, -1, text).await?,
                    }
                }
                "" => {}
                _ => self.plain_line("What? ? for the commands."),
            }
        }
    }

    /// Message `num` of an area, if the caller may read it.
    fn plain_msg(&self, a: &AreaRef, num: u32) -> Option<Message> {
        let m = self.state.msgs.get(a.key(), num).ok().flatten()?;
        (!m.deleted && self.ok_to_read(a, &m)).then_some(m)
    }

    /// A page of an area's threads, the latest activity first.
    fn plain_threads(&mut self, a: &AreaRef, page: usize) -> io::Result<()> {
        let v = self.area_view(a)?;
        let mut ts = threads(&v.msgs);
        ts.sort_by_key(|t| std::cmp::Reverse(t.last));
        let by_id: HashMap<i64, &Message> = v.msgs.iter().map(|m| (m.id, m)).collect();
        let scores = self.state.msgs.scores(a.key()).map_err(io_err)?;
        let comments = self.state.msgs.comment_counts(a.key()).map_err(io_err)?;
        let pages = ts.len().div_ceil(PAGE).max(1);
        let page = page.min(pages);
        if ts.is_empty() {
            self.plain_line("No threads yet. p starts one.");
            return Ok(());
        }
        self.plain_line(&format!("Threads, page {page} of {pages} (* has new):"));
        self.plain_line("     #  score  day    by            re  title");
        let mut lines = Vec::new();
        for t in ts.iter().skip((page - 1) * PAGE).take(PAGE) {
            let Some(root) = t.msgs.first().and_then(|(id, _)| by_id.get(id)) else { continue };
            let re = t.msgs.len() - 1 + t.msgs.iter().map(|(id, _)| comments.get(id).copied().unwrap_or(0) as usize).sum::<usize>();
            let new = t.msgs.iter().any(|(id, _)| by_id.get(id).is_some_and(|m| v.is_new(m)));
            lines.push(format!(
                "{}{:>5}  {:>5}  {}  {:<12} {:>3}  {}",
                if new { "*" } else { " " },
                root.num,
                score_label(scores.get(&root.id).copied()),
                self.local(t.last).format("%m/%d"),
                fluxbbs_term::codes::resize(&self.plain_author(root), 12),
                re,
                strip_colours(&root.subject)
            ));
        }
        for l in lines {
            self.plain_line(&l);
        }
        if page < pages {
            self.plain_line(&format!("More: l {}", page + 1));
        }
        Ok(())
    }

    fn plain_author(&self, m: &Message) -> String {
        if m.anonymous { strip_colours(self.strings().get("Anonymous")) } else { strip_colours(&m.from.name) }
    }

    /// A message: a header, the text and its comments; then it's read.
    fn plain_show(&mut self, a: &AreaRef, m: &Message) -> io::Result<()> {
        let when = self.local(m.posted).format("%Y-%m-%d %H:%M").to_string();
        self.newline();
        self.plain_line(&format!("--- #{} in {}: {}", m.num, strip_colours(&a.area.name), strip_colours(&m.subject)));
        self.plain_line(&format!("From {} to {}, {when}", self.plain_author(m), strip_colours(&m.to.name)));
        self.newline();
        for l in self.reader_lines(a, m) {
            self.reader_line(&l);
            self.newline();
        }
        self.plain_line(&format!("--- end of #{}", m.num));
        self.mark_read(a, m)
    }

    /// The thread a message is in, in reading order.
    fn plain_thread(&mut self, a: &AreaRef, m: &Message) -> io::Result<()> {
        let v: AreaView = self.area_view(a)?;
        let Some(t) = threads(&v.msgs).into_iter().find(|t| t.msgs.iter().any(|(id, _)| *id == m.id)) else { return Ok(()) };
        let by_id: HashMap<i64, &Message> = v.msgs.iter().map(|m| (m.id, m)).collect();
        let comments = self.state.msgs.comment_counts(a.key()).map_err(io_err)?;
        let mut lines = Vec::new();
        for (id, depth) in &t.msgs {
            let Some(x) = by_id.get(id) else { continue };
            let c = comments.get(id).copied().unwrap_or(0);
            let c = if c == 0 { String::new() } else { format!(" ({c} comments)") };
            lines.push(format!(
                "{}{:>5}  {}{}  {}  {}{c}",
                if v.is_new(x) { "*" } else { " " },
                x.num,
                "  ".repeat(*depth),
                self.local(x.posted).format("%m/%d %H:%M"),
                self.plain_author(x),
                strip_colours(&x.subject)
            ));
        }
        self.plain_line("The thread (* is new):");
        for l in lines {
            self.plain_line(&l);
        }
        Ok(())
    }

    /// What's new, a thread at a time: in one area, or all the caller
    /// scans. At most `NEW_MAX` a time.
    async fn plain_read_new(&mut self, only: Option<&AreaRef>) -> io::Result<()> {
        let areas: Vec<AreaRef> = match only {
            Some(a) => vec![a.clone()],
            None => self.scanned_areas().into_iter().map(|(_, a)| a).collect(),
        };
        let mut shown = 0;
        let mut left = 0;
        for a in &areas {
            let v = self.area_view(a)?;
            let ids = v.new_in_thread_order();
            let by_id: HashMap<i64, Message> = v.msgs.into_iter().map(|m| (m.id, m)).collect();
            for id in ids {
                if shown >= NEW_MAX {
                    left += 1;
                    continue;
                }
                if let Some(m) = by_id.get(&id) {
                    self.plain_show(a, m)?;
                    shown += 1;
                }
            }
        }
        match (shown, left) {
            (0, _) => self.plain_line("Nothing new."),
            (_, 0) => self.plain_line("That's everything new."),
            (_, n) => self.plain_line(&format!("{n} more new: n again.")),
        }
        Ok(())
    }

    /// A comment (0) or a vote up (1) or down (-1) under a message.
    async fn plain_push(&mut self, a: &AreaRef, m: &Message, kind: i8, text: String) -> io::Result<()> {
        if !self.may_push(a, m) {
            return self.say_line("PushNotHere").await;
        }
        if kind != 0 && self.is_me(&m.from) {
            return self.say_line("PushOwn").await;
        }
        let now = crate::clock::now().naive_local();
        if let Some(last) = self.state.msgs.last_push_at(self.user.id).map_err(io_err)?
            && (now - last).num_seconds() < PUSH_GAP
        {
            return self.say_line("PushTooFast").await;
        }
        let mut text = strip_colours(&text).trim().to_string();
        if kind == 0 && text.is_empty() {
            self.write("Comment: ");
            text = strip_colours(&self.read_string(ReadOpts::new(PUSH_WIDTH)).await?).trim().to_string();
            self.newline();
            if text.is_empty() {
                return Ok(());
            }
        }
        if text.chars().count() > PUSH_WIDTH {
            text = text.chars().take(PUSH_WIDTH).collect();
            self.plain_line(&format!("Cut to {PUSH_WIDTH} characters."));
        }
        if kind == 0 {
            self.seen.posted.insert(strip_colours(&a.area.name));
        }
        let p = Push { id: 0, msg_id: m.id, user_id: self.user.id, handle: self.user.handle.clone(), kind, text, at: now };
        self.state.msgs.add_push(&p).map_err(io_err)?;
        tracing::info!("{}: pushed message {} in {} ({kind}), plain text", self.user.handle, m.num, a.area.name);
        self.plain_line(match kind {
            0 => "Commented.",
            1 => "Voted up.",
            _ => "Voted down.",
        });
        Ok(())
    }
}
