//! New: the SysOp's own questions for new users (`*P`): asked after the
//! stock ones when someone applies, the answers kept with the infoform
//! answers (form `NEWUSER`) and shown in the user editor (`&`, `0`) and
//! to new user voters.

use std::io;

use fluxbbs_term::codes::resize;
use serde::{Deserialize, Serialize};

use crate::cfgedit::{Field, Form, yes_no};
use crate::input::{Case, DIGITS, ReadOpts};
use crate::session::Session;

/// The answers' form name in the store.
pub const FORM: &str = "NEWUSER";

pub const KINDS: [&str; 4] = ["text", "yes/no", "number", "choice"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// What's asked, codes and all.
    pub prompt: String,
    /// `text`, `yes/no`, `number` or `choice` (one of `choices`).
    #[serde(default = "text")]
    pub kind: String,
    /// The longest answer, for text and numbers.
    #[serde(default = "forty")]
    pub length: u16,
    /// Must it be answered?
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

fn text() -> String {
    "text".into()
}

fn forty() -> u16 {
    40
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Questions {
    #[serde(rename = "question", default)]
    pub list: Vec<Question>,
}

impl Questions {
    pub fn to_toml(&self) -> String {
        format!(
            "# Questions new users are asked when they apply, after the stock ones\n\
             # (*P edits them). kind: text, yes/no, number or choice.\n\n{}",
            toml::to_string_pretty(self).expect("questions serialize")
        )
    }
}

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

impl Session {
    /// Ask the questions, in order; the answers, one a question (blank
    /// where one wasn't required and not given).
    pub(crate) async fn ask_questions(&mut self) -> io::Result<Vec<String>> {
        let qs = self.state.questions().list.clone();
        let mut answers = Vec::with_capacity(qs.len());
        if qs.is_empty() {
            return Ok(answers);
        }
        self.newline();
        self.say_line("NuQuestions").await?;
        for q in &qs {
            self.newline();
            let len = q.length.clamp(1, 200) as usize;
            let answer = match q.kind.as_str() {
                "yes/no" => {
                    self.show(&format!("|U1{} ", q.prompt)).await?;
                    let yes = self.yes_no(true).await?;
                    if yes { "Yes" } else { "No" }.to_string()
                }
                "number" => {
                    self.show(&format!("|U1{} |U3", q.prompt)).await?;
                    let mut opts = ReadOpts::new(len).allowed(DIGITS);
                    if q.required {
                        opts = opts.required();
                    }
                    let s = self.read_string(opts).await?;
                    self.newline();
                    s
                }
                "choice" if !q.choices.is_empty() => {
                    self.show_line(&format!("|U1{}", q.prompt)).await?;
                    for (i, c) in q.choices.iter().enumerate() {
                        self.show_line(&format!("  |U3{}|U2) |U1{c}", i + 1)).await?;
                    }
                    loop {
                        self.say("NuQuestionWhich").await?;
                        let s = self.read_string(ReadOpts::new(2).allowed(DIGITS)).await?;
                        self.newline();
                        if s.is_empty() && !q.required {
                            break String::new();
                        }
                        if let Some(c) = s.parse::<usize>().ok().and_then(|n| q.choices.get(n.wrapping_sub(1))) {
                            break c.clone();
                        }
                    }
                }
                _ => {
                    self.show(&format!("|U1{} |U3", q.prompt)).await?;
                    let mut opts = ReadOpts::new(len).case(Case::Normal);
                    if q.required {
                        opts = opts.required();
                    }
                    let s = self.read_string(opts).await?;
                    self.newline();
                    s
                }
            };
            answers.push(answer);
        }
        Ok(answers)
    }

    /// A user's answers, question by question, then a key. False if they
    /// never answered any (or there are no questions to show them by).
    pub(crate) async fn show_answers(&mut self, user: u32) -> io::Result<bool> {
        let Some(answers) = self.state.forms.answers(FORM, user).map_err(io_err)? else { return Ok(false) };
        let qs = self.state.questions().list.clone();
        if qs.is_empty() {
            return Ok(false);
        }
        self.newline();
        self.say_line("NuAnswersHead").await?;
        for (q, a) in qs.iter().zip(answers.iter().chain(std::iter::repeat(&String::new()))) {
            // The answer as typed, its codes shown rather than run.
            let a = if a.is_empty() { "-".to_string() } else { fluxbbs_term::lit(a) };
            self.show_line(&format!(" |U1{}|U2: |U3{a}", q.prompt)).await?;
        }
        self.flush().await?;
        self.read_key().await?;
        Ok(true)
    }

    /// `*P`: list, insert, delete, move and edit the questions.
    pub async fn question_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: new user question edit", self.user.handle);
        loop {
            let mut qs = self.state.questions().list.clone();
            self.clear_screen();
            self.set_slot(fluxbbs_term::Slot::Text);
            self.write(" Num  Prompt                                    Type     Len  Required");
            self.newline();
            self.rule().await?;
            for (i, q) in qs.iter().enumerate() {
                self.set_slot(fluxbbs_term::Slot::Info);
                let prompt = crate::editor::strip_codes(&q.prompt);
                self.write(&format!(" {} {} {} {}  {}", resize(&(i + 1).to_string(), 4), resize(&prompt, 41), resize(&q.kind, 8), resize(&q.length.to_string(), 3), yes_no(q.required)));
                self.newline();
            }
            if qs.is_empty() {
                self.write(" No questions yet: new users get only the stock ones. Insert one.");
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            let n = qs.len() as u32;
            match cmd {
                'E' => {
                    self.newline();
                    if n > 0
                        && let Some(num) = self.cfg_ask_number("Begin edit with which question?", 1, n).await?
                    {
                        self.question_edit(num).await?;
                    }
                }
                _ => {
                    let fresh = |_: &[Question]| Question { prompt: "New question".into(), kind: text(), length: 40, required: false, choices: Vec::new() };
                    if self.list_change(&mut qs, cmd, "question", fresh).await? {
                        self.state.set_questions(Questions { list: qs }).map_err(io_err)?;
                    }
                }
            }
        }
    }

    async fn question_edit(&mut self, mut num: u32) -> io::Result<()> {
        loop {
            let mut list = self.state.questions().list.clone();
            let n = list.len() as u32;
            let idx = num as usize - 1;
            // Another node may have shortened the list meanwhile.
            let Some(mut q) = list.get(idx).cloned() else { return Ok(()) };
            let mut form = Form::new(&format!("{} New User Question Editor", self.state.config().bbs.name));
            form.label("Current question", 12, format!("{num} of {n}"));
            form.item('A', "Prompt", 60, &q.prompt, "What's asked (codes are fine).");
            form.item('B', "Kind", 8, &q.kind, "text, yes/no, number, or choice (one of the choices below).");
            form.item('C', "Length", 3, q.length.to_string(), "The longest answer, for text and numbers (1-200).");
            form.item('D', "Required", 3, yes_no(q.required), "Must it be answered?");
            form.item('E', "Choices", 60, q.choices.join(", "), "For a choice question: the choices, separated by commas.");
            form.item('[', "Previous question", 0, "", "");
            form.item(']', "Next question", 0, "", "");
            self.cfg_draw(&form);
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut form).await? else { break 0 };
                match key {
                    'A' => {
                        if let Some(v) = self.cfg_text(&mut form, key, Field::TEXT).await? {
                            q.prompt = v;
                        }
                    }
                    'B' => {
                        let cur = KINDS.iter().position(|k| *k == q.kind).unwrap_or(0);
                        q.kind = KINDS[self.cfg_option(&mut form, key, &KINDS, cur).await?].into();
                    }
                    'C' => {
                        if let Some(v) = self.cfg_number(&mut form, key, 1, 200).await? {
                            q.length = v as u16;
                        }
                    }
                    'D' => q.required = self.cfg_bool(&mut form, key, q.required).await?,
                    'E' => {
                        if let Some(v) = self.cfg_text(&mut form, key, Field::TEXT).await? {
                            q.choices = v.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
                            self.cfg_set(&mut form, key, q.choices.join(", "));
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if list[idx] != q {
                list[idx] = q;
                self.state.set_questions(Questions { list }).map_err(io_err)?;
            }
            if step == 0 {
                self.screen.goto(1, self.screen.rows());
                self.newline();
                return Ok(());
            }
            num = ((num as i64 - 1 + step).rem_euclid(n as i64) + 1) as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_round_trip_through_toml() {
        let qs = Questions {
            list: vec![
                Question { prompt: "Where did you hear of us?".into(), kind: text(), length: 40, required: true, choices: Vec::new() },
                Question { prompt: "Your scene".into(), kind: "choice".into(), length: 40, required: false, choices: vec!["art".into(), "hpavc".into()] },
            ],
        };
        let back: Questions = toml::from_str(&qs.to_toml()).unwrap();
        assert_eq!(back, qs);
        // The defaults: a prompt is enough.
        let short: Questions = toml::from_str("[[question]]\nprompt = \"Why?\"\n").unwrap();
        assert_eq!(short.list[0], Question { prompt: "Why?".into(), kind: text(), length: 40, required: false, choices: Vec::new() });
    }
}
