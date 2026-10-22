//! Infoforms (INFOFORM.PAS) and new user voting (NUV.PAS).
//!
//! An infoform is a text file with input points in it: `~SN` (and the
//! other `~S?` cases), `~BD` (a date), `~PN` (a phone number), `~NM`
//! (digits), `~FN` (a filename), or a bare `*`. `~NN` sets the length of
//! the next answers, `~**` puts it back to 254, `~*=` makes answers
//! required and `~*-` optional again. Filling one in reads an answer at
//! each point; showing it puts the answers back in the same places.
//!
//! The forms are listed in `infoforms.toml`; answers and votes are kept in
//! the database, by the form's file name and the user's number.

use std::io;
use std::path::Path;
use std::sync::Mutex;

use chrono::{NaiveDateTime};
use iniquity_term::codes::resize;
use iniquity_term::{Slot, Source, Token, tokenize};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::input::{Case, DIGITS, ReadOpts};
use crate::session::{DisplayState, Session, waves};
use crate::user::{User, parse_date};

fn io_err(e: anyhow::Error) -> io::Error {
    io::Error::other(e)
}

/// `tInfoformRec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Infoform {
    pub desc: String,
    /// The form's text file, in the text directory (`NEWUSER.INF`).
    pub file: String,
    /// Everyone it applies to fills it in at logon (`IM`).
    #[serde(default)]
    pub mandatory: bool,
    /// Shown to voters during new user voting.
    #[serde(default)]
    pub nuv: bool,
    #[serde(default)]
    pub acs: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Infoforms {
    #[serde(rename = "form", default)]
    pub list: Vec<Infoform>,
}

impl Infoforms {
    pub fn to_toml(&self) -> String {
        format!(
            "# Infoforms: questionnaires in the text directory (see EXAMPLE.INF).\n\
             # Mandatory ones are filled in at logon; NUV ones are shown to voters.\n\n{}",
            toml::to_string_pretty(self).expect("infoforms serialize")
        )
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS infoform_answers (
    form    TEXT NOT NULL COLLATE NOCASE,
    user_id INTEGER NOT NULL,
    answers TEXT NOT NULL,
    at      TEXT NOT NULL,
    PRIMARY KEY (form, user_id)
);
CREATE TABLE IF NOT EXISTS nuv_votes (
    voter_id     INTEGER NOT NULL,
    candidate_id INTEGER NOT NULL,
    initials     TEXT NOT NULL,
    comment      TEXT NOT NULL,
    at           TEXT NOT NULL,
    PRIMARY KEY (voter_id, candidate_id)
);
";

/// Infoform answers and NUV votes.
pub struct FormStore {
    db: Mutex<Connection>,
}

impl FormStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = crate::sysdata::open_db(path)?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn answers(&self, form: &str, user: u32) -> anyhow::Result<Option<Vec<String>>> {
        let db = self.db.lock().unwrap();
        let text: Option<String> = db
            .query_row("SELECT answers FROM infoform_answers WHERE form = ?1 AND user_id = ?2", params![form, user], |r| r.get(0))
            .optional()?;
        Ok(text.map(|t| split_lines(&t)))
    }

    pub fn set_answers(&self, form: &str, user: u32, answers: &[String], at: NaiveDateTime) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO infoform_answers (form, user_id, answers, at) VALUES (?1, ?2, ?3, ?4)",
            params![form, user, join_lines(answers), at],
        )?;
        Ok(())
    }

    /// `ifResetForm` / `ifResetUser` (form None: all of a user's forms).
    pub fn clear(&self, form: Option<&str>, user: u32) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        let n = match form {
            Some(f) => db.execute("DELETE FROM infoform_answers WHERE form = ?1 AND user_id = ?2", params![f, user])?,
            None => db.execute("DELETE FROM infoform_answers WHERE user_id = ?1", [user])?,
        };
        Ok(n as u32)
    }

    /// Everyone's answers to a form (when it's removed).
    pub fn clear_form(&self, form: &str) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        Ok(db.execute("DELETE FROM infoform_answers WHERE form = ?1", [form])? as u32)
    }

    pub fn has_voted(&self, voter: u32, candidate: u32) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row("SELECT 1 FROM nuv_votes WHERE voter_id = ?1 AND candidate_id = ?2", params![voter, candidate], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn add_vote(&self, voter: u32, candidate: u32, initials: &str, comment: &str, at: NaiveDateTime) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO nuv_votes (voter_id, candidate_id, initials, comment, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![voter, candidate, initials, comment, at],
        )?;
        Ok(())
    }

    /// The comments left on a candidate, oldest first: (initials, comment).
    pub fn comments(&self, candidate: u32) -> anyhow::Result<Vec<(String, String)>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT initials, comment FROM nuv_votes WHERE candidate_id = ?1 AND comment <> '' ORDER BY at, rowid")?;
        let rows = stmt.query_map([candidate], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// `nuvReset`: forget votes by or about a user.
    pub fn clear_votes(&self, user: u32) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM nuv_votes WHERE voter_id = ?1 OR candidate_id = ?1", [user])?;
        Ok(())
    }
}

/// Answers are stored one per line; a line break inside one can't happen
/// (input is a single line), so plain newlines are enough.
fn join_lines(answers: &[String]) -> String {
    answers.join("\n")
}

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(String::from).collect()
}

/// `mInitials`: the first letter, then the letter after the first space
/// (or dash), else the second letter. Case is kept.
fn initials(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let Some(&first) = chars.first() else { return String::new() };
    let split = chars.iter().position(|&c| c == ' ').or_else(|| chars.iter().position(|&c| c == '-'));
    let second = match split {
        Some(z) => chars.get(z + 1).filter(|c| !matches!(c, ' ' | '-')).or(chars.get(1)),
        None => chars.get(1),
    };
    second.map_or(first.to_string(), |s| format!("{first}{s}"))
}

/// What an input point is.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Input {
    Text(Case),
    Date,
    Phone,
    Number,
    Filename,
}

fn input_kind(code: [u8; 2]) -> Option<Input> {
    Some(match &code {
        b"SN" => Input::Text(Case::Normal),
        b"SM" => Input::Text(Case::Mixed),
        b"SU" => Input::Text(Case::Upper),
        b"SL" => Input::Text(Case::Lower),
        b"SC" => Input::Text(Case::Capital),
        b"SE" => Input::Text(Case::Weird),
        b"SW" => Input::Text(Case::Warped),
        b"SI" => Input::Text(Case::Cool),
        b"BD" => Input::Date,
        b"PN" => Input::Phone,
        b"NM" => Input::Number,
        b"FN" => Input::Filename,
        _ => return None,
    })
}

/// "Mandatory/NUV", "Normal"...
fn form_kind(f: &Infoform) -> String {
    match (f.mandatory, f.nuv) {
        (true, true) => "Mandatory/NUV".into(),
        (true, false) => "Mandatory".into(),
        (false, true) => "NUV".into(),
        _ => "Normal".into(),
    }
}

impl Session {
    /// Run an infoform's text file: read answers (`answers` None) or show
    /// the given ones in place. Returns the answers read, or None if the
    /// file is missing (or there was nothing to answer).
    async fn run_form(&mut self, file: &str, answers: Option<&[String]>) -> io::Result<Option<Vec<String>>> {
        let Some(path) = self.find_text_file(file) else { return Ok(None) };
        let data = tokio::fs::read(&path).await?;
        self.positions.clear();
        self.positions2.clear();
        let mut st = DisplayState::default();
        let (mut limit, mut required) = (254usize, false);
        let mut got = Vec::new();
        let mut next = 0;
        for tok in tokenize(&data, Source::File) {
            match tok {
                Token::Text(text) => {
                    // A bare `*` is an input point too.
                    for (i, part) in text.split(|&b| b == b'*').enumerate() {
                        if i > 0 {
                            self.form_point(Input::Text(Case::Normal), limit, required, answers, &mut next, &mut got).await?;
                        }
                        self.screen.write_bytes(part);
                    }
                }
                Token::Ext(code) => match &code {
                    [a, b] if a.is_ascii_digit() && b.is_ascii_digit() => limit = ((a - b'0') * 10 + (b - b'0')).max(1) as usize,
                    b"*-" => required = false,
                    b"*=" => required = true,
                    b"**" => limit = 254,
                    _ => match input_kind(code) {
                        Some(kind) => self.form_point(kind, limit, required, answers, &mut next, &mut got).await?,
                        None => self.display_token(Token::Ext(code), Source::File, None, &mut st).await?,
                    },
                },
                t => self.display_token(t, Source::File, None, &mut st).await?,
            }
        }
        self.display_end(st);
        self.set_slot(Slot::Text);
        Ok(if answers.is_none() && got.is_empty() { None } else { Some(got) })
    }

    /// One input point: read an answer, or show the next stored one.
    async fn form_point(
        &mut self,
        kind: Input,
        limit: usize,
        required: bool,
        answers: Option<&[String]>,
        next: &mut usize,
        got: &mut Vec<String>,
    ) -> io::Result<()> {
        if let Some(a) = answers {
            if let Some(s) = a.get(*next) {
                self.write_with_controls(s);
            }
            *next += 1;
            return Ok(());
        }
        let opts = |o: ReadOpts| if required { o.required() } else { o };
        let s = match kind {
            Input::Text(case) => self.read_string(opts(ReadOpts::new(limit).case(case))).await?,
            Input::Number => self.read_string(opts(ReadOpts::new(limit).allowed(DIGITS))).await?,
            Input::Filename => {
                let o = ReadOpts::new(limit.min(64)).case(Case::Upper).allowed("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-");
                self.read_string(opts(o)).await?
            }
            Input::Phone => {
                let digits = self.read_string(opts(ReadOpts::new(15).allowed(DIGITS))).await?;
                crate::logon::format_phone(&digits)
            }
            Input::Date => {
                // Keep asking until it's a real date (or blank, if allowed).
                let x = self.screen.where_x();
                loop {
                    let s = self.read_string(opts(ReadOpts::new(8).allowed("0123456789/"))).await?;
                    if s.is_empty() || parse_date(&s).is_some() {
                        break s;
                    }
                    self.screen.backspace(s.chars().count() as u16);
                    self.screen.pos_x(x);
                }
            }
        };
        got.push(s);
        Ok(())
    }

    /// The forms this user may fill in, with their numbers (from 1).
    fn forms_for(&self, u: &User) -> Vec<(usize, Infoform)> {
        self.state.infoforms().list.iter().cloned().enumerate().map(|(i, f)| (i + 1, f)).filter(|(_, f)| self.acs_for(u, &f.acs)).collect()
    }

    /// Fill in a form and store the answers. False if nothing was saved.
    async fn fill_form(&mut self, f: &Infoform) -> io::Result<bool> {
        let Some(answers) = self.run_form(&f.file, None).await? else { return Ok(false) };
        self.newline();
        self.state.forms.set_answers(&f.file, self.user.id, &answers, crate::clock::now().naive_local()).map_err(io_err)?;
        tracing::info!("{}: filled in infoform {}", self.user.handle, f.desc);
        Ok(true)
    }

    /// `ifShowForm`: show a user's answers to a form, then wait for a
    /// key. False if they haven't filled it in.
    pub async fn show_form(&mut self, f: &Infoform, user: u32) -> io::Result<bool> {
        let Some(answers) = self.state.forms.answers(&f.file, user).map_err(io_err)? else { return Ok(false) };
        if self.run_form(&f.file, Some(&answers)).await?.is_none() {
            return Ok(false);
        }
        self.flush().await?;
        self.read_key().await?;
        Ok(true)
    }

    /// `ifInfoforms` (`IN`, and `IM` with `mandatory_only`): fill in any
    /// mandatory forms not done yet, then list the forms and offer one.
    pub async fn infoforms(&mut self, mandatory_only: bool) -> io::Result<()> {
        let user = self.user.clone();
        let forms = self.forms_for(&user);
        for (_, f) in forms.iter().filter(|(_, f)| f.mandatory) {
            if self.state.forms.answers(&f.file, user.id).map_err(io_err)?.is_none() {
                self.fill_form(f).await?;
            }
        }
        if mandatory_only || forms.is_empty() {
            return Ok(());
        }
        let mut rep = self.repeat_begin("INFO", false).await?;
        if rep.is_none() {
            self.clear_screen();
            self.newline();
            self.set_slot(Slot::Info);
            self.write(&format!(" {} {} {} Complete", resize("Num", 4), resize("Description", 20), resize("Type", 13)));
            self.newline();
            self.rule().await?;
        }
        for (n, f) in &forms {
            let done = if self.state.forms.answers(&f.file, user.id).map_err(io_err)?.is_some() { "Yes" } else { "No" };
            match rep.as_mut() {
                Some(r) => {
                    let w = waves([("NM", n.to_string()), ("DS", f.desc.clone()), ("ST", form_kind(f)), ("CP", done.into())]);
                    self.repeat_row(r, &w, false).await?;
                }
                None => {
                    self.set_slot(Slot::Text);
                    self.write(&format!(" {} {} {} {done}", resize(&n.to_string(), 4), resize(&f.desc, 20), resize(&form_kind(f), 13)));
                    self.newline();
                }
            }
        }
        match rep {
            Some(r) => self.repeat_end(r).await?,
            None => self.rule().await?,
        }
        let total = self.state.infoforms().list.len();
        let s = self.strings().fmt("InfoAskWhich", &[&total.to_string()]);
        self.show(&s).await?;
        let pick = self.read_string(ReadOpts::new(3).allowed(DIGITS)).await?;
        self.newline();
        let Some((_, f)) = pick.parse::<usize>().ok().and_then(|n| forms.iter().find(|(m, _)| *m == n)).cloned() else {
            return Ok(());
        };
        if self.state.forms.answers(&f.file, user.id).map_err(io_err)?.is_some() {
            self.say("InfoAskReplace").await?;
            if !self.yes_no(false).await? {
                return Ok(());
            }
        }
        self.fill_form(&f).await?;
        Ok(())
    }

    /// `nuvVote` (`NV`; `show` when the param has an S): vote on each new
    /// user this caller hasn't voted on yet.
    pub async fn nuv_vote(&mut self, show: bool) -> io::Result<()> {
        let nuv = self.state.config().nuv.clone();
        if !self.acs(&nuv.voter_acs) {
            return Ok(());
        }
        if !nuv.enabled {
            if show {
                self.newline();
                self.say_line("NuvIsDisabled").await?;
                self.newline();
            }
            return Ok(());
        }
        self.newline();
        self.say("NuvScanning").await?;
        self.newline();
        let me = self.user.id;
        let mut any = false;
        for mut u in self.state.users.list().map_err(io_err)? {
            if u.deleted || u.id == me || !self.acs_for(&u, &nuv.candidate_acs) {
                continue;
            }
            if self.state.forms.has_voted(me, u.id).map_err(io_err)? {
                continue;
            }
            any = true;
            let comments: Vec<String> = self
                .state
                .forms
                .comments(u.id)
                .map_err(io_err)?
                .into_iter()
                .take(12)
                .map(|(ini, c)| if nuv.initials { format!("{ini} - {c}") } else { c })
                .collect();
            let (choice, done) = loop {
                self.newline();
                let mut w = waves([
                    ("UN", u.handle.clone()),
                    ("YV", u.vote_yes.to_string()),
                    ("NV", u.vote_no.to_string()),
                    ("YN", nuv.votes_yes.to_string()),
                    ("NN", nuv.votes_no.to_string()),
                ]);
                for i in 0..12 {
                    let key = format!("{:02}", i + 1).into_bytes();
                    w.insert([key[0], key[1]], comments.get(i).cloned().unwrap_or_default());
                }
                if !self.show_file_with("NUV", &w).await? {
                    self.newline();
                    let s = format!(
                        "|U1New user|U2: |U3{}|U2, |U1yes votes|U2: |U3{}|U2, |U1no votes|U2: |U3{}",
                        u.handle, u.vote_yes, u.vote_no
                    );
                    self.show_line(&s).await?;
                    self.newline();
                    if !comments.is_empty() {
                        self.show_line("|U1Comments|U2:").await?;
                        for c in &comments {
                            self.show_line(&format!(" |U3{c}")).await?;
                        }
                        self.newline();
                    }
                }
                let s = self.strings().fmt("NuvVote", &[&u.handle]);
                self.show(&s).await?;
                let c = loop {
                    if let Some(c @ ('Y' | 'N' | 'A' | 'I')) = self.read_key().await?.upper() {
                        break c;
                    }
                };
                match c {
                    'I' => {
                        self.write("Info");
                        self.newline();
                        let mut shown = false;
                        for f in self.state.infoforms().list.iter().filter(|f| f.nuv).cloned().collect::<Vec<_>>() {
                            shown |= self.show_form(&f, u.id).await?;
                        }
                        if !shown {
                            self.say_line("NuvNoInfoforms").await?;
                            self.newline();
                            self.prompt_key().await?;
                        }
                    }
                    'Y' => {
                        self.write("Yes");
                        self.newline();
                        u.vote_yes += 1;
                        let done = u.vote_yes >= nuv.votes_yes as u32;
                        if done {
                            let s = self.strings().fmt("NuvValidated", &[&u.handle]);
                            self.show_line(&s).await?;
                            if let Some(level) = self.state.level(nuv.level) {
                                u.apply_level(nuv.level, &level);
                            }
                            tracing::info!("{} voted in by NUV", u.handle);
                        }
                        self.state.users.save(&u).map_err(io_err)?;
                        break ('Y', done);
                    }
                    'N' => {
                        self.write("No");
                        self.newline();
                        u.vote_no += 1;
                        let done = u.vote_no >= nuv.votes_no as u32;
                        if done {
                            let s = self.strings().fmt("NuvDeleted", &[&u.handle]);
                            self.show_line(&s).await?;
                            u.deleted = true;
                            tracing::info!("{} voted out by NUV", u.handle);
                        }
                        self.state.users.save(&u).map_err(io_err)?;
                        break ('N', done);
                    }
                    _ => {
                        self.write("Abstain");
                        self.newline();
                        break ('A', false);
                    }
                }
            };
            let mut comment = String::new();
            if choice != 'A' && !done && comments.len() < 12 {
                self.say("NuvAskComment").await?;
                if self.yes_no(false).await? {
                    self.say("NuvComment").await?;
                    comment = self.read_string(ReadOpts::new(80)).await?;
                    self.newline();
                }
            }
            let ini = initials(&self.user.handle);
            self.state.forms.add_vote(me, u.id, &ini, &comment, crate::clock::now().naive_local()).map_err(io_err)?;
        }
        if !any {
            self.newline();
            self.say_line("NuvNone").await?;
        }
        self.newline();
        Ok(())
    }
}

impl Session {
    /// `cfgInfoformEditor` (`*I`): list, insert, delete, move and edit
    /// the forms.
    pub async fn infoform_editor(&mut self) -> io::Result<()> {
        tracing::info!("{}: infoform edit", self.user.handle);
        loop {
            let mut forms = self.state.infoforms().list.clone();
            self.clear_screen();
            self.set_slot(Slot::Text);
            self.write(" Num  Description               File          Type           ACS");
            self.newline();
            self.rule().await?;
            for (i, f) in forms.iter().enumerate() {
                let missing = if self.find_text_file(&f.file).is_none() { " (missing)" } else { "" };
                self.set_slot(Slot::Info);
                self.write(&format!(
                    " {} {} {} {} {}",
                    resize(&(i + 1).to_string(), 4),
                    resize(&f.desc, 25),
                    resize(&format!("{}{missing}", f.file), 13),
                    resize(&form_kind(f), 14),
                    f.acs
                ));
                self.newline();
            }
            if forms.is_empty() {
                self.write(" No infoforms yet. Insert one; text/EXAMPLE.INF shows the codes.");
                self.newline();
            }
            self.rule().await?;
            let Some(cmd) = self.cfg_list_prompt(&[("I", "nsert"), ("D", "elete"), ("E", "dit"), ("M", "ove"), ("Esc", " Quit")]).await? else {
                return Ok(());
            };
            let n = forms.len() as u32;
            match cmd {
                'E' => {
                    self.newline();
                    if n > 0
                        && let Some(num) = self.cfg_ask_number("Begin edit with which infoform?", 1, n).await?
                    {
                        self.infoform_edit(num).await?;
                    }
                }
                'D' => {
                    self.newline();
                    if n == 0 {
                        continue;
                    }
                    let Some(which) = self.cfg_ask_number("Delete which infoform?", 1, n).await? else { continue };
                    let gone = forms.remove(which as usize - 1);
                    self.state.set_infoforms(Infoforms { list: forms }).map_err(io_err)?;
                    self.show(&format!("|U1Also delete everyone's answers to |U3{}|U1? ", gone.desc)).await?;
                    if self.yes_no(false).await? {
                        self.state.forms.clear_form(&gone.file).map_err(io_err)?;
                    }
                }
                _ => {
                    let fresh = |_: &[Infoform]| Infoform {
                        desc: "New infoform".into(),
                        file: "EXAMPLE.INF".into(),
                        mandatory: false,
                        nuv: false,
                        acs: String::new(),
                    };
                    if self.list_change(&mut forms, cmd, "infoform", fresh).await? {
                        self.state.set_infoforms(Infoforms { list: forms }).map_err(io_err)?;
                    }
                }
            }
        }
    }

    async fn infoform_edit(&mut self, mut num: u32) -> io::Result<()> {
        use crate::cfgedit::{Field, Form, yes_no};
        loop {
            let mut list = self.state.infoforms().list.clone();
            let n = list.len() as u32;
            let idx = num as usize - 1;
            let mut f = list[idx].clone();
            let mut form = Form::new(&format!("{} Infoform Editor", self.state.config().bbs.name));
            form.label("Current infoform", 12, format!("{num} of {n}"));
            form.item('A', "Description", 40, &f.desc, "What this infoform is (shown in the infoform list).");
            form.item('B', "Text file", 13, &f.file, "The form, in the text directory, with its extension (NEWUSER.INF).");
            form.item('C', "Mandatory", 3, yes_no(f.mandatory), "Must users it applies to fill it in at logon?");
            form.item('D', "New user voting", 3, yes_no(f.nuv), "Show it to voters during new user voting?");
            form.item('E', "Access ACS", 20, &f.acs, "Who this infoform applies to.");
            form.item('[', "Previous infoform", 0, "", "");
            form.item(']', "Next infoform", 0, "", "");
            self.cfg_draw(&form);
            if self.find_text_file(&f.file).is_none() {
                self.cfg_info(&mut form, &format!("{} isn't in the text directory yet.", f.file));
            }
            let step: i64 = loop {
                let Some(key) = self.cfg_command(&mut form).await? else { break 0 };
                match key {
                    'A' => {
                        if let Some(v) = self.cfg_text(&mut form, key, Field::TEXT).await? {
                            f.desc = v;
                        }
                    }
                    'B' => {
                        let field = Field { allowed: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-", ..Field::UPPER };
                        match self.cfg_text(&mut form, key, field).await? {
                            // Answers are kept by file, so a different
                            // file starts with none.
                            Some(v) if !v.is_empty() => {
                                f.file = v;
                                if self.find_text_file(&f.file).is_none() {
                                    self.cfg_info(&mut form, &format!("{} isn't in the text directory yet.", f.file));
                                }
                            }
                            _ => self.cfg_set(&mut form, key, &f.file),
                        }
                    }
                    'C' => f.mandatory = self.cfg_bool(&mut form, key, f.mandatory).await?,
                    'D' => f.nuv = self.cfg_bool(&mut form, key, f.nuv).await?,
                    'E' => {
                        if let Some(v) = self.cfg_text(&mut form, key, Field::LOWER).await? {
                            f.acs = v;
                        }
                    }
                    '[' => break -1,
                    ']' => break 1,
                    _ => {}
                }
            };
            if list[idx] != f {
                list[idx] = f;
                self.state.set_infoforms(Infoforms { list }).map_err(io_err)?;
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
    fn answers_and_votes_store() {
        let s = FormStore::open_memory().unwrap();
        let now = crate::clock::now().naive_local();
        assert_eq!(s.answers("NEW.INF", 3).unwrap(), None);
        s.set_answers("NEW.INF", 3, &["Bob".into(), String::new(), "x".into()], now).unwrap();
        assert_eq!(s.answers("new.inf", 3).unwrap().unwrap(), ["Bob", "", "x"]);
        s.add_vote(1, 3, "SO", "seems fine", now).unwrap();
        s.add_vote(2, 3, "AB", "", now).unwrap();
        assert!(s.has_voted(1, 3).unwrap() && !s.has_voted(3, 1).unwrap());
        assert_eq!(s.comments(3).unwrap(), [("SO".to_string(), "seems fine".to_string())]);
        s.clear_votes(3).unwrap();
        assert!(!s.has_voted(1, 3).unwrap());
        assert_eq!(s.clear(None, 3).unwrap(), 1);
    }

    #[test]
    fn input_codes_and_initials() {
        assert_eq!(input_kind(*b"SC"), Some(Input::Text(Case::Capital)));
        assert_eq!(input_kind(*b"BD"), Some(Input::Date));
        assert_eq!(input_kind(*b"XY"), None);
        assert_eq!(initials("System Operator"), "SO");
        assert_eq!(initials("SysOp"), "Sy");
        assert_eq!(initials("dark-angel"), "da");
        assert_eq!(initials("x"), "x");
    }
}
