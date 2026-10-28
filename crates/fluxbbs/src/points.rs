//! New: file points around the board (`[points]`). Upvotes pay a post's
//! author (once per voter and post, however often the vote changes, up
//! to a cap a day); doors pay by the minute; callers tip each other, a
//! post's author from the reader (`$`, plain text's `tip`) or anyone
//! from `!P`; and a lottery is drawn each night from the tickets sold
//! that day, the house adding to the pot. Every movement goes in the
//! `points` ledger, which keeps the caps and shows each caller theirs.
//! Bots earn, give and play none of it, and all of it needs `[files]
//! use_file_points`. The lottery's pictures are in `lottery.rs`.

use std::io;
use std::ops::Range;

use chrono::{NaiveDate, NaiveDateTime};
use rand::RngExt;

use crate::BbsState;
use crate::aimemory::is_bot;
use crate::input::{DIGITS, ReadOpts};
use crate::msgbase::Message;
use crate::nodes::NodeEvent;
use crate::session::Session;
use crate::sysdata::PointsEntry;
use crate::user::User;

/// Lines of the ledger `!P` shows.
const RECENT: usize = 8;

fn now() -> NaiveDateTime {
    crate::clock::now().naive_local()
}

/// When today began, for the daily caps.
fn today() -> NaiveDateTime {
    now().date().and_hms_opt(0, 0, 0).unwrap_or_default()
}

/// A lottery's day, as the ledger keeps it.
pub(crate) fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// A daily cap, 0 for none.
fn cap(n: u32) -> u32 {
    if n == 0 { u32::MAX } else { n }
}

fn entry(kind: &str, from: (u32, &str), to: (u32, &str), amount: u32, on: &str, note: &str) -> PointsEntry {
    PointsEntry {
        at: now(),
        kind: kind.into(),
        from_id: from.0,
        from: from.1.into(),
        to_id: to.0,
        to: to.1.into(),
        amount,
        on: on.into(),
        note: note.into(),
    }
}

/// Add to someone's file points, onto their record as it is now: a
/// session they have open adds what it changed to that when it saves.
pub(crate) fn credit(state: &BbsState, user_id: u32, amount: u32) -> anyhow::Result<()> {
    let Some(u) = state.users.get(user_id)? else { return Ok(()) };
    let mut paid = u.clone();
    paid.file_points = paid.file_points.saturating_add(amount);
    state.users.merge_with(&u, &paid, false)?;
    Ok(())
}

/// A node message to `handle`, wherever they're on.
fn tell(state: &BbsState, handle: &str, from: usize, user: &str, text: &str) {
    for (n, info) in state.nodes.online() {
        if info.user.eq_ignore_ascii_case(handle) {
            state.nodes.send(n, NodeEvent::Message { from, user: user.into(), text: text.into(), broadcast: false });
        }
    }
}

// ---- the lottery --------------------------------------------------------------

/// A day's tickets, numbered from 1 in the order bought: each purchase's
/// buyer and numbers.
fn numbered(bought: &[(u32, String, u32)]) -> Vec<(u32, String, Range<u32>)> {
    let mut at = 1;
    bought
        .iter()
        .map(|(id, handle, n)| {
            let r = at..at + n;
            at += n;
            (*id, handle.clone(), r)
        })
        .collect()
}

/// `user`'s ticket numbers on `day`.
pub(crate) fn ticket_numbers(state: &BbsState, day: &str, user: u32) -> Vec<u32> {
    let bought = state.sys.lottery_entries(day).unwrap_or_default();
    numbered(&bought).into_iter().filter(|t| t.0 == user).flat_map(|t| t.2).collect()
}

/// A night's draw, as the ledger has it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Draw {
    pub day: String,
    /// The winning ticket, and how many were in it.
    pub number: u32,
    pub tickets: u32,
    pub winner_id: u32,
    pub winner: String,
    pub pot: u32,
}

impl Draw {
    fn from_entry(e: &PointsEntry) -> Draw {
        // The note is "number/tickets".
        let (n, t) = e.note.split_once('/').unwrap_or(("0", &e.note));
        Draw { day: e.on.clone(), number: n.parse().unwrap_or(0), tickets: t.parse().unwrap_or(0), winner_id: e.to_id, winner: e.to.clone(), pot: e.amount }
    }

    /// Its day as "10/27".
    pub(crate) fn date(&self) -> String {
        NaiveDate::parse_from_str(&self.day, "%Y-%m-%d").map_or(self.day.clone(), |d| d.format("%m/%d").to_string())
    }
}

pub(crate) fn last_draw(state: &BbsState) -> Option<Draw> {
    state.sys.lottery_last().ok().flatten().filter(|e| e.to_id != 0).map(|e| Draw::from_entry(&e))
}

/// Tonight's lottery as a caller sees it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tonight {
    /// What the tickets cost so far, and the house's part once any sold.
    pub pot: u32,
    pub sold: u32,
    /// The caller's ticket numbers, how many more they may buy today, and
    /// at what price.
    pub mine: Vec<u32>,
    pub room: u32,
    pub price: u32,
}

/// Draw every past day's lottery that hasn't been (the event scheduler,
/// each tick: just after midnight, or when the board's back up).
pub fn draw_due(state: &BbsState) {
    let days = match state.sys.lottery_undrawn(&day_key(now().date())) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("lottery: {e}");
            return;
        }
    };
    for day in days {
        if let Err(e) = draw(state, &day) {
            tracing::error!("lottery {day}: {e}");
        }
    }
}

/// One day's draw: a ticket at random wins the pot and the house's part.
/// Tickets whose buyer has gone can't win.
fn draw(state: &BbsState, day: &str) -> anyhow::Result<()> {
    let tickets = numbered(&state.sys.lottery_entries(day)?);
    let total = tickets.last().map_or(0, |t| t.2.end - 1);
    let mut live = Vec::new();
    for (id, _, r) in &tickets {
        if state.users.get(*id)?.is_some_and(|u| !u.deleted) {
            live.extend(r.clone());
        }
    }
    let won = (!live.is_empty()).then(|| live[rand::rng().random_range(0..live.len())]);
    let holder = won.and_then(|n| tickets.iter().find(|t| t.2.contains(&n)));
    let pot = state.sys.lottery_pot(day)? + state.config().points.house;
    // Nobody left to win: drawn all the same.
    let (to, handle, pot) = holder.map_or((0, "", 0), |h| (h.0, h.1.as_str(), pot));
    let e = entry("lottery", (0, ""), (to, handle), pot, day, &format!("{}/{total}", won.unwrap_or(0)));
    if !state.sys.lottery_record(&e)? || to == 0 {
        return Ok(());
    }
    credit(state, to, pot)?;
    tracing::info!("lottery {day}: {handle} won {pot} points with ticket {} of {total}", won.unwrap_or(0));
    let s = state.strings().fmt("LotteryWonNote", &[&pot.to_string(), &won.unwrap_or(0).to_string()]);
    tell(state, handle, 0, "the lottery", &s);
    Ok(())
}

impl Session {
    /// Points mean something here, for this caller.
    pub(crate) fn points_on(&self) -> bool {
        self.state.config().files.use_file_points && self.user.id != 0 && !is_bot(&self.user)
    }

    /// Our points as the database has them (upvotes, tips and wins land
    /// there while we're on), keeping what this session changed since.
    pub(crate) fn sync_points(&mut self) {
        let Ok(Some(db)) = self.state.users.get(self.user.id) else { return };
        let mut base = self.user_base.borrow_mut();
        let Some(b) = base.as_mut().filter(|b| b.id == self.user.id) else { return };
        let mine = self.user.file_points as i64 - b.file_points as i64;
        self.user.file_points = (db.file_points as i64 + mine).max(0) as u32;
        b.file_points = db.file_points;
    }

    /// An upvote on `m`: its author's points (`upvote`), once per voter
    /// and post, up to `upvote_daily` a day. Not for bots' votes or
    /// posts, nor for posts from elsewhere on a network.
    pub(crate) fn pay_upvote(&self, m: &Message) {
        let cfg = self.state.config().points.clone();
        if !self.points_on() || cfg.upvote == 0 || m.from.user_id == 0 || m.from.user_id == self.user.id {
            return;
        }
        let Ok(Some(author)) = self.state.users.get(m.from.user_id) else { return };
        if author.deleted || is_bot(&author) {
            return;
        }
        let subject = crate::editor::strip_colours(&m.subject);
        let e = entry("upvote", (self.user.id, &self.user.handle), (author.id, &author.handle), cfg.upvote, &m.id.to_string(), &subject);
        match self.state.sys.add_points_capped(&e, cap(cfg.upvote_daily), today(), true) {
            Ok(0) => {}
            Ok(n) => match credit(&self.state, author.id, n) {
                Ok(()) => tracing::info!("{}: upvote paid {} {n} point(s) for message {}", self.user.handle, author.handle, m.num),
                Err(e) => tracing::error!("upvote points to {}: {e}", author.handle),
            },
            Err(e) => tracing::error!("upvote points: {e}"),
        }
    }

    /// After a door: a point per `door_minutes` played, up to
    /// `door_daily` a day.
    #[cfg_attr(not(feature = "doors"), allow(dead_code))]
    pub(crate) fn door_points(&mut self, door: &str, minutes: u32) {
        let cfg = self.state.config().points.clone();
        if !self.points_on() || cfg.door_minutes == 0 || minutes < cfg.door_minutes {
            return;
        }
        let e = entry("door", (0, ""), (self.user.id, &self.user.handle), minutes / cfg.door_minutes, door, &minutes.to_string());
        match self.state.sys.add_points_capped(&e, cap(cfg.door_daily), today(), false) {
            Ok(0) => {}
            Ok(n) => {
                self.user.file_points = self.user.file_points.saturating_add(n);
                self.save_user();
                tracing::info!("{}: {n} point(s) for {minutes} min of {door}", self.user.handle);
            }
            Err(e) => tracing::error!("door points: {e}"),
        }
    }

    // ---- tips -------------------------------------------------------------------

    /// Why we can't give `amount` points to `to` today, said in full; None
    /// if we can.
    fn tip_refused(&mut self, to: &User, amount: u32) -> Option<String> {
        let s = self.strings();
        let limit = self.state.config().points.tip_daily;
        if limit == 0 {
            return Some(s.get("TipOff").to_string());
        }
        if to.id == self.user.id {
            return Some(s.get("TipSelf").to_string());
        }
        if is_bot(to) || to.deleted {
            return Some(s.get("TipBot").to_string());
        }
        self.sync_points();
        if amount > self.user.file_points {
            return Some(s.fmt("PointsShort", &[&self.user.file_points.to_string(), &amount.to_string()]));
        }
        let given = self.state.sys.points_since("tip", self.user.id, true, today()).unwrap_or(0);
        let left = limit.saturating_sub(given);
        (amount > left).then(|| s.fmt("TipLimit", &[&left.to_string()]))
    }

    /// Give `amount` points to `to`, for message `on` (its id, or blank)
    /// with `note` (its subject): what to say after.
    fn tip_pay(&mut self, to: &User, amount: u32, on: &str, note: &str) -> io::Result<String> {
        self.user.file_points -= amount;
        self.save_own_user().map_err(io::Error::other)?;
        let e = entry("tip", (self.user.id, &self.user.handle), (to.id, &to.handle), amount, on, note);
        self.state.sys.add_points(&e).map_err(io::Error::other)?;
        credit(&self.state, to.id, amount).map_err(io::Error::other)?;
        tracing::info!("{}: tipped {} {amount} point(s)", self.user.handle, to.handle);
        let s = self.strings();
        let why = if note.is_empty() { String::new() } else { s.fmt("TipNoteFor", &[note]) };
        tell(&self.state, &to.handle, self.node.number(), &self.user.handle, &s.fmt("TipNote", &[&amount.to_string(), &why]));
        Ok(s.fmt("TipDone", &[&amount.to_string(), &to.handle]))
    }

    /// The tip for a post: its author, or why there's none to give. The
    /// reader asks how much on its bottom line, plain text on a line.
    pub(crate) fn tip_author(&self, m: &Message) -> Result<User, String> {
        let s = self.strings();
        if !self.points_on() || self.state.config().points.tip_daily == 0 {
            return Err(s.get("TipOff").to_string());
        }
        match self.state.users.get(m.from.user_id) {
            Ok(Some(u)) if m.from.user_id != 0 => Ok(u),
            _ => Err(s.get("TipNobody").to_string()),
        }
    }

    /// Tip `to` `amount` for `m` (or nothing in particular): what to say.
    pub(crate) fn tip(&mut self, to: &User, amount: u32, m: Option<&Message>) -> io::Result<String> {
        if let Some(why) = self.tip_refused(to, amount) {
            return Ok(why);
        }
        let (on, note) = m.map_or((String::new(), String::new()), |m| (m.id.to_string(), crate::editor::strip_colours(&m.subject)));
        self.tip_pay(to, amount, &on, &note)
    }

    /// `t` in `!P`: tip anyone, by handle.
    async fn tip_someone(&mut self) -> io::Result<()> {
        if self.state.config().points.tip_daily == 0 {
            return self.say_line("TipOff").await;
        }
        self.say("TipWho").await?;
        let mut opts = ReadOpts::new(36);
        opts.complete_users = true;
        let who = self.read_string(opts).await?;
        self.newline();
        if who.trim().is_empty() {
            return Ok(());
        }
        let Some(to) = self.state.users.find(&who, false).map_err(io::Error::other)? else {
            return self.say_line("TipNoSuch").await;
        };
        let Some(amount) = self.ask_tip(&to).await? else { return Ok(()) };
        let s = self.tip(&to, amount, None)?;
        self.show_line(&s).await
    }

    /// How much to tip `to`, on a line; None for nothing.
    pub(crate) async fn ask_tip(&mut self, to: &User) -> io::Result<Option<u32>> {
        self.sync_points();
        let s = self.strings().fmt("TipAskAmount", &[&to.handle, &self.user.file_points.to_string()]);
        self.show(&s).await?;
        let n: u32 = self.read_string(ReadOpts::new(5).allowed(DIGITS)).await?.trim().parse().unwrap_or(0);
        self.newline();
        Ok((n > 0).then_some(n))
    }

    // ---- the lottery, for a caller ------------------------------------------------

    pub(crate) fn tonight(&mut self) -> Tonight {
        self.sync_points();
        let cfg = self.state.config().points.clone();
        let day = day_key(now().date());
        let sold: u32 = self.state.sys.lottery_entries(&day).unwrap_or_default().iter().map(|e| e.2).sum();
        let mine = ticket_numbers(&self.state, &day, self.user.id);
        let pot = self.state.sys.lottery_pot(&day).unwrap_or(0) + if sold > 0 { cfg.house } else { 0 };
        let room = cap(cfg.tickets_daily).saturating_sub(mine.len() as u32).min(self.user.file_points / cfg.ticket.max(1));
        Tonight { pot, sold, mine, room, price: cfg.ticket }
    }

    /// Buy `n` tickets in tonight's draw, or say why not.
    pub(crate) fn buy_tickets_now(&mut self, n: u32) -> io::Result<Result<(), String>> {
        let cfg = self.state.config().points.clone();
        let s = self.strings();
        if cfg.ticket == 0 {
            return Ok(Err(s.get("LotteryOff").to_string()));
        }
        let t = self.tonight();
        if cap(cfg.tickets_daily) <= t.mine.len() as u32 {
            return Ok(Err(s.get("LotteryLimit").to_string()));
        }
        let cost = n * cfg.ticket;
        if n > t.room {
            return Ok(Err(s.fmt("PointsShort", &[&self.user.file_points.to_string(), &cost.to_string()])));
        }
        self.user.file_points -= cost;
        self.save_own_user().map_err(io::Error::other)?;
        let e = entry("ticket", (self.user.id, &self.user.handle), (0, ""), cost, &day_key(now().date()), &n.to_string());
        self.state.sys.add_points(&e).map_err(io::Error::other)?;
        tracing::info!("{}: {n} lottery ticket(s) for {cost} points", self.user.handle);
        Ok(Ok(()))
    }

    fn seen_key(&self) -> String {
        format!("{}:seen", self.user.id)
    }

    /// The last draw, with our tickets in it, if we held any and haven't
    /// seen it yet.
    pub(crate) fn unseen_draw(&self) -> Option<(Draw, Vec<u32>)> {
        let d = last_draw(&self.state)?;
        let seen = self.state.sys.script_get("lottery", &self.seen_key()).ok().flatten().unwrap_or_default();
        if seen >= d.day {
            return None;
        }
        let mine = ticket_numbers(&self.state, &d.day, self.user.id);
        (!mine.is_empty()).then_some((d, mine))
    }

    pub(crate) fn saw_draw(&self, d: &Draw) {
        if let Err(e) = self.state.sys.script_set("lottery", &self.seen_key(), Some(&d.day)) {
            tracing::warn!("lottery seen: {e}");
        }
    }

    /// The draw in a line, for plain text (and boards without the casino).
    pub(crate) fn draw_line(&self, d: &Draw, mine: &[u32]) -> String {
        let s = self.strings();
        let mut line = s.fmt("LotteryDrawLine", &[&d.date(), &d.number.to_string(), &d.tickets.to_string(), &d.winner, &d.pot.to_string()]);
        if mine.contains(&d.number) {
            line.push(' ');
            line.push_str(s.get("LotteryThatsYou"));
        }
        line
    }

    /// The pictures, where they can be shown.
    #[cfg(feature = "casino")]
    fn lottery_pictures(&self) -> bool {
        !self.plain() && self.screen.rows() >= 24
    }

    /// `!P draw` (the logon step `lottery`): last night's draw, for those
    /// who held tickets in it, once.
    async fn lottery_logon(&mut self) -> io::Result<()> {
        if !self.points_on() {
            return Ok(());
        }
        let Some((d, mine)) = self.unseen_draw() else { return Ok(()) };
        self.saw_draw(&d);
        #[cfg(feature = "casino")]
        if self.lottery_pictures() {
            return self.lottery_replay(&d, &mine).await;
        }
        self.newline();
        let s = self.draw_line(&d, &mine);
        self.show_line(&s).await
    }

    /// `b` in `!P`: the lottery, in pictures where it can be.
    async fn lottery(&mut self) -> io::Result<()> {
        if self.state.config().points.ticket == 0 {
            return self.say_line("LotteryOff").await;
        }
        #[cfg(feature = "casino")]
        if self.lottery_pictures() {
            return self.lottery_stage().await;
        }
        if let Some((d, mine)) = self.unseen_draw() {
            self.saw_draw(&d);
            let s = self.draw_line(&d, &mine);
            self.show_line(&s).await?;
        }
        self.buy_tickets().await
    }

    /// Tickets, on a line.
    async fn buy_tickets(&mut self) -> io::Result<()> {
        let t = self.tonight();
        if t.room == 0 {
            if let Err(why) = self.buy_tickets_now(1)? {
                self.show_line(&why).await?;
            }
            return Ok(());
        }
        let s = self.strings().fmt("LotteryHowMany", &[&t.price.to_string(), &t.room.to_string()]);
        self.show(&s).await?;
        let n: u32 = self.read_string(ReadOpts::new(4).allowed(DIGITS)).await?.trim().parse().unwrap_or(0);
        self.newline();
        if n == 0 {
            return Ok(());
        }
        match self.buy_tickets_now(n.min(t.room))? {
            Ok(()) => {
                let held = self.tonight().mine;
                let nums: Vec<String> = held.iter().map(|n| format!("#{n}")).collect();
                let s = self.strings().fmt("LotteryBought", &[&held.len().to_string(), &nums.join(" ")]);
                self.show_line(&s).await
            }
            Err(why) => self.show_line(&why).await,
        }
    }

    // ---- !P ------------------------------------------------------------------------

    /// `!P` (new): your points, what moved them lately, tonight's
    /// lottery; buy tickets (`b`, or `!P lottery`) or tip someone (`t`,
    /// `!P tip`). `!P draw` is the logon step's.
    pub async fn points_screen(&mut self, param: &str) -> io::Result<()> {
        let p = param.trim().to_ascii_lowercase();
        if p == "draw" {
            return self.lottery_logon().await;
        }
        self.newline();
        if !self.state.config().files.use_file_points {
            return self.say_line("PointsOff").await;
        }
        if !self.points_on() {
            return self.say_line("PointsBot").await;
        }
        let mut first = p.chars().next();
        loop {
            let key = match first.take() {
                Some(c) => c,
                None => {
                    self.points_summary().await?;
                    self.say("PointsMenu").await?;
                    let k = self.read_choice().await?;
                    self.newline();
                    match k.upper() {
                        Some(c) => c.to_ascii_lowercase(),
                        None => return Ok(()),
                    }
                }
            };
            match key {
                'b' | 'l' => self.lottery().await?,
                't' => self.tip_someone().await?,
                _ => return Ok(()),
            }
            if self.hangup {
                return Ok(());
            }
        }
    }

    async fn points_summary(&mut self) -> io::Result<()> {
        // New: the board's POINTS screen heads it, if it has one.
        if !self.plain() && self.find_text_file("POINTS").is_some() {
            self.clear_screen();
            self.show_file("POINTS").await?;
        }
        let t = self.tonight();
        let s = self.strings();
        let mut lines = vec![s.fmt("PointsBalance", &[&self.user.file_points.to_string()])];
        if t.price > 0 {
            lines.push(s.fmt("PointsLottery", &[&t.pot.to_string(), &t.sold.to_string(), &t.mine.len().to_string()]));
        }
        if let Some(d) = last_draw(&self.state) {
            lines.push(s.fmt("PointsLastDraw", &[&d.date(), &d.winner, &d.pot.to_string()]));
        }
        let recent = self.state.sys.points_recent(self.user.id, RECENT).unwrap_or_default();
        lines.push(String::new());
        if recent.is_empty() {
            lines.push(s.get("PointsNone").to_string());
        } else {
            lines.push(s.get("PointsRecent").to_string());
            for e in &recent {
                lines.push(self.ledger_line(e));
            }
        }
        lines.push(String::new());
        for l in lines {
            self.show_line(&l).await?;
        }
        Ok(())
    }

    /// One line of the ledger, as the caller sees it.
    fn ledger_line(&self, e: &PointsEntry) -> String {
        let s = self.strings();
        let mine = e.to_id == self.user.id;
        let what = match e.kind.as_str() {
            "upvote" => s.fmt("PointsUpvote", &[&e.from, &e.note]),
            "door" => s.fmt("PointsDoor", &[&e.on, &e.note]),
            "tip" if mine => s.fmt("PointsTipFrom", &[&e.from]),
            "tip" => s.fmt("PointsTipTo", &[&e.to]),
            "ticket" => s.fmt("PointsTickets", &[&e.note]),
            "lottery" => {
                let d = Draw::from_entry(e);
                s.fmt("PointsWon", &[&d.number.to_string(), &d.tickets.to_string()])
            }
            k => k.to_string(),
        };
        let amount = format!("{}{}", if mine { "+" } else { "-" }, e.amount);
        let when = self.local(e.at).format("%m/%d %H:%M").to_string();
        s.fmt("PointsRow", &[&when, &format!("{amount:>5}"), &fluxbbs_term::codes::lit(&what)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_days_and_numbers() {
        assert_eq!(cap(0), u32::MAX);
        assert_eq!(cap(20), 20);
        assert_eq!(day_key(NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()), "2026-10-27");
        let n = numbered(&[(1, "a".into(), 2), (2, "b".into(), 3), (1, "a".into(), 1)]);
        assert_eq!(n.iter().map(|t| t.2.clone()).collect::<Vec<_>>(), [1..3, 3..6, 6..7]);
        let e = PointsEntry {
            at: now(),
            kind: "lottery".into(),
            from_id: 0,
            from: String::new(),
            to_id: 2,
            to: "b".into(),
            amount: 90,
            on: "2026-10-26".into(),
            note: "4/6".into(),
        };
        let d = Draw::from_entry(&e);
        assert_eq!((d.number, d.tickets, d.date().as_str()), (4, 6, "10/26"));
    }
}
