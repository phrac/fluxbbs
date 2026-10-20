//! The voting booth (2.0): `VV` vote on everything new, `VA` vote on one
//! question, `VR` see results, `VN` add a question, `VD` delete one.
//! 2.0's source isn't available; this follows its manual, VOTING.MNU,
//! its screens and wave codes, and 2.20's strings. Screens:
//! VQTOP/VQMID/VQBOT list the questions (`~QN` number, `~QD` question),
//! VATOP/VAMID/VABOT the answers (`~VQ` the question; `~AN` number, `~AD`
//! answer), VRTOP/VRMID/VRBOT the results (`~VQ`; `~AP` percent, `~AD`,
//! and 2.20's `~VN` number and `~VR` votes).

use std::io;

use iniquity_term::Slot;
use iniquity_term::codes::resize;

use crate::input::{DIGITS, ReadOpts};
use crate::messages::io_err;
use crate::session::{Session, waves};
use crate::sysdata::Question;

/// 2.0 capped the booth; so do we, so the list stays a list.
const MAX_QUESTIONS: usize = 50;
const MAX_ANSWERS: usize = 20;

impl Session {
    fn questions(&self) -> io::Result<Vec<Question>> {
        self.state.sys.questions().map_err(io_err)
    }

    fn voting_status(&self, what: &str) {
        let s = self.strings().get(what).to_string();
        self.node.update(|n| n.activity = s);
    }

    fn voting_done(&self) {
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
    }

    /// `VV`: every question the caller hasn't answered, in turn.
    pub async fn vote_all(&mut self) -> io::Result<()> {
        self.voting_status("NodeVoting");
        let qs = self.questions()?;
        let mut asked = false;
        for q in qs {
            if self.state.sys.my_vote(self.user.id, q.id).map_err(io_err)?.is_some() {
                continue;
            }
            asked = true;
            if !self.vote_on(&q).await? || self.hangup {
                break;
            }
        }
        if !asked {
            self.newline();
            self.say_line("VoteNoneNew").await?;
        }
        self.voting_done();
        Ok(())
    }

    /// `VA`: pick a question and vote on it.
    pub async fn vote_one(&mut self) -> io::Result<()> {
        self.voting_status("NodeVoting");
        if let Some(q) = self.pick_question("VoteAskQuestion").await? {
            if self.state.sys.my_vote(self.user.id, q.id).map_err(io_err)?.is_some() {
                self.say("VoteChangeVote").await?;
                if self.yes_no(false).await? {
                    self.vote_on(&q).await?;
                }
            } else {
                self.vote_on(&q).await?;
            }
        }
        self.voting_done();
        Ok(())
    }

    /// `VR`: pick a question and see how it's going.
    pub async fn vote_results(&mut self) -> io::Result<()> {
        if let Some(q) = self.pick_question("VoteAskQuestion").await? {
            self.show_results(&q).await?;
        }
        Ok(())
    }

    /// `VN`: add a question, its answers, and whether callers may add
    /// their own.
    pub async fn vote_add(&mut self) -> io::Result<()> {
        if self.questions()?.len() >= MAX_QUESTIONS {
            self.newline();
            return self.say_line("VoteTooMany").await;
        }
        self.voting_status("NodeVoteAdd");
        self.newline();
        self.say("VoteAskText").await?;
        let text = self.read_string(ReadOpts::new(70)).await?;
        self.newline();
        if text.is_empty() {
            self.voting_done();
            return Ok(());
        }
        self.say_line("VoteAskAnswers").await?;
        let mut answers = Vec::new();
        while answers.len() < MAX_ANSWERS {
            let s = format!("|U2[|U3{:>2}|U2] |U3", answers.len() + 1);
            self.show(&s).await?;
            let a = self.read_string(ReadOpts::new(60)).await?;
            self.newline();
            if a.is_empty() {
                break;
            }
            answers.push(a);
        }
        self.say("VoteAskUserAdd").await?;
        let user_add = self.yes_no(answers.len() < 2).await?;
        self.say("VoteAskSave").await?;
        if (!answers.is_empty() || user_add) && self.yes_no(true).await? {
            self.state.sys.add_question(&text, &self.user.handle, user_add, &answers).map_err(io_err)?;
            tracing::info!("{}: added voting question {text:?}", self.user.handle);
        }
        self.voting_done();
        Ok(())
    }

    /// `VD`: delete a question and its votes.
    pub async fn vote_delete(&mut self) -> io::Result<()> {
        let qs = self.questions()?;
        if qs.is_empty() {
            self.newline();
            return self.say_line("VoteNone").await;
        }
        self.list_questions(&qs).await?;
        let s = self.strings().fmt("VoteAskDelete", &[&qs.len().to_string()]);
        self.show(&s).await?;
        let n = self.read_string(ReadOpts::new(3).allowed(DIGITS)).await?;
        self.newline();
        if let Some(q) = n.parse::<usize>().ok().and_then(|n| qs.get(n.wrapping_sub(1))) {
            self.state.sys.delete_question(q.id).map_err(io_err)?;
            tracing::info!("{}: deleted voting question {:?}", self.user.handle, q.text);
        }
        Ok(())
    }

    async fn list_questions(&mut self, qs: &[Question]) -> io::Result<()> {
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        let mut rep = self.repeat_begin("VQ", false).await?;
        if rep.is_none() {
            self.newline();
            self.set_slot(Slot::Info);
            self.show_line(" Num  Question").await?;
            self.rule().await?;
        }
        let mut lines = 4;
        for (i, q) in qs.iter().enumerate() {
            let more = match rep.as_mut() {
                Some(r) => self.repeat_row(r, &waves([("QN", (i + 1).to_string()), ("QD", q.text.clone())]), false).await?,
                None => {
                    let mine = self.state.sys.my_vote(self.user.id, q.id).map_err(io_err)?.is_some();
                    self.set_slot(if mine { Slot::Text } else { Slot::TextHi });
                    self.write(&format!(" {} {}", resize(&(i + 1).to_string(), 4), q.text));
                    self.newline();
                    lines += 1;
                    self.more(&mut lines).await?
                }
            };
            if !more {
                break;
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await,
            None => self.rule().await,
        }
    }

    /// List the questions and ask for one by number.
    async fn pick_question(&mut self, ask: &str) -> io::Result<Option<Question>> {
        let qs = self.questions()?;
        if qs.is_empty() {
            self.newline();
            self.say_line("VoteNone").await?;
            return Ok(None);
        }
        self.list_questions(&qs).await?;
        self.say(ask).await?;
        let n = self.read_string(ReadOpts::new(3).allowed(DIGITS)).await?;
        self.newline();
        Ok(n.parse::<usize>().ok().and_then(|n| qs.into_iter().nth(n.wrapping_sub(1))))
    }

    /// Show a question's answers and take the caller's vote (or a new
    /// answer, where callers may add them). False if they quit.
    async fn vote_on(&mut self, q: &Question) -> io::Result<bool> {
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        let mut rep = self.repeat_begin_with("VA", false, &waves([("VQ", q.text.clone())])).await?;
        if rep.is_none() {
            self.newline();
            self.set_slot(Slot::Info);
            self.write(&format!(" {}", q.text));
            self.newline();
            self.rule().await?;
        }
        let mut lines = 4;
        for (i, a) in q.answers.iter().enumerate() {
            let w = waves([("AN", (i + 1).to_string()), ("AD", a.text.clone())]);
            let more = match rep.as_mut() {
                Some(r) => self.repeat_row(r, &w, false).await?,
                None => {
                    self.set_slot(Slot::Text);
                    self.write(&format!(" {} {}", resize(&(i + 1).to_string(), 4), a.text));
                    self.newline();
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
        let can_add = q.user_add && q.answers.len() < MAX_ANSWERS;
        if can_add {
            self.say_line("VotePressAdd").await?;
        }
        self.say("VoteAskAnswer").await?;
        let pick = self.read_string(ReadOpts::new(3).allowed(if can_add { "0123456789Aa" } else { DIGITS })).await?;
        self.newline();
        let answer = if can_add && pick.eq_ignore_ascii_case("A") {
            self.say("VoteAskNewAnswer").await?;
            let text = self.read_string(ReadOpts::new(60)).await?;
            self.newline();
            if text.is_empty() {
                return Ok(true);
            }
            Some(self.state.sys.add_answer(q.id, &text).map_err(io_err)?)
        } else {
            pick.parse::<usize>().ok().and_then(|n| q.answers.get(n.wrapping_sub(1))).map(|a| a.id)
        };
        let Some(answer) = answer else { return Ok(!pick.is_empty()) };
        self.state.sys.vote(self.user.id, q.id, answer).map_err(io_err)?;
        self.say("VoteAskResults").await?;
        if self.yes_no(false).await? {
            let q = self.questions()?.into_iter().find(|x| x.id == q.id);
            if let Some(q) = q {
                self.show_results(&q).await?;
            }
        }
        Ok(true)
    }

    async fn show_results(&mut self, q: &Question) -> io::Result<()> {
        let total: u32 = q.answers.iter().map(|a| a.votes).sum();
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        let mut rep = self.repeat_begin_with("VR", false, &waves([("VQ", q.text.clone())])).await?;
        if rep.is_none() {
            self.newline();
            self.set_slot(Slot::Info);
            self.write(&format!(" {}", q.text));
            self.newline();
            self.rule().await?;
        }
        let mut lines = 4;
        for (i, a) in q.answers.iter().enumerate() {
            let pct = if total == 0 { 0 } else { a.votes * 100 / total };
            let w = waves([("AP", format!("{pct}%")), ("AD", a.text.clone()), ("VN", (i + 1).to_string()), ("VR", a.votes.to_string())]);
            let more = match rep.as_mut() {
                Some(r) => self.repeat_row(r, &w, false).await?,
                None => {
                    self.set_slot(Slot::Text);
                    let bar = "■".repeat((pct as usize).div_ceil(5));
                    self.write(&format!(" {} {} {} {}", resize(&format!("{pct}%"), 4), resize(&a.votes.to_string(), 4), resize(&a.text, 40), bar));
                    self.newline();
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
        self.prompt_key().await
    }
}
