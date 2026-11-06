//! New: sixel pictures offered at logon (`UX`, the `sixel` logon step),
//! once a caller: the SIXEL screen says what they are and whether the
//! caller's terminal has said it shows them, offers a sample (SIXEL.PNG,
//! .JPG or .GIF beside the screen) and asks whether to turn them on.
//! SIXEL's markers: `~ST` the terminal's two lines, `~SQ` where the
//! questions go (three rows: two of question, one for the choices),
//! `~SR` the answer's line. Without the screen it asks in plain lines.

use std::io;

use crate::session::Session;

const SAMPLES: [&str; 3] = ["SIXEL.PNG", "SIXEL.JPG", "SIXEL.GIF"];

impl Session {
    pub async fn sixel_offer(&mut self) -> io::Result<()> {
        if self.plain() || self.user.sixel || self.user.sixel_asked {
            return Ok(());
        }
        self.ask_terminal().await?;
        let can = self.sixel_seen();
        let sample = SAMPLES.iter().find_map(|n| self.find_text_file(n));
        let art = self.sixel_screen(can).await?;
        let mut seen = false;
        if sample.is_some() && self.sixel_ask(art, &["SixelTry"], None).await? == Some(true) {
            self.sixel_sample(sample.clone(), can).await?;
            seen = true;
        }
        // Yes, no, or (with a sample) see it again first.
        let on = loop {
            let again = sample.is_some().then_some(seen);
            match self.sixel_ask(art, &["SixelEnable", "SixelEnable2"], again).await? {
                Some(on) => break on,
                None => {
                    self.sixel_sample(sample.clone(), can).await?;
                    seen = true;
                }
            }
        };
        self.user.sixel = on;
        self.user.sixel_asked = true;
        self.save_user();
        tracing::info!("{}: sixel pictures {}", self.user.handle, if on { "on" } else { "left off" });
        let said = [if on { "SixelOn" } else { "SixelOff" }, "SixelChange", "SixelChange2", "SixelKey"];
        match self.positions.get(b"SR").copied().filter(|_| art) {
            Some((x, y, _)) => {
                for (i, s) in said.into_iter().enumerate() {
                    self.screen.goto(x, y + i as u16);
                    self.say(s).await?;
                }
                self.flush().await?;
                self.read_key().await?;
                // The next step (fast logon's question) writes where the
                // cursor is: not over this screen.
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.clear_screen();
            }
            None => {
                for s in &said[..3] {
                    self.say_line(s).await?;
                }
            }
        }
        Ok(())
    }

    /// The sample in sixels whatever their setting (which stays off),
    /// then the screen again.
    async fn sixel_sample(&mut self, sample: Option<std::path::PathBuf>, can: bool) -> io::Result<()> {
        let Some(path) = sample else { return Ok(()) };
        let was = std::mem::replace(&mut self.user.sixel, true);
        let caption = self.strings().get("SixelSample").to_string();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let p = path.clone();
        let r = self.show_picture_key(&name, Some(caption), path, String::new(), move || std::fs::read(&p)).await;
        self.user.sixel = was;
        r?;
        self.sixel_screen(can).await?;
        Ok(())
    }

    /// The SIXEL screen with the terminal's lines; false (and a plain
    /// line instead) without one.
    async fn sixel_screen(&mut self, can: bool) -> io::Result<bool> {
        let (a, b) = if can { ("SixelCan", "SixelCan2") } else { ("SixelCant", "SixelCant2") };
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        if self.show_file("SIXEL").await? && self.positions.contains_key(b"SQ") {
            if let Some(&(x, y, _)) = self.positions.get(b"ST") {
                for (i, s) in [a, b].into_iter().enumerate() {
                    self.screen.goto(x, y + i as u16);
                    self.say(s).await?;
                }
            }
            return Ok(true);
        }
        let name = self.state.config().bbs.name.clone();
        let intro = self.strings().fmt("SixelPlain", &[&name]);
        self.show_line(&intro).await?;
        self.say_line(a).await?;
        self.say_line(b).await?;
        Ok(false)
    }

    /// A question's lines at `~SQ` (cleared first), the choices under
    /// them; or on lines of their own. `again`, if given, adds a third
    /// choice, to see the sample (again, if it's been `true`ly seen):
    /// None.
    async fn sixel_ask(&mut self, art: bool, lines: &[&str], again: Option<bool>) -> io::Result<Option<bool>> {
        let at = self.positions.get(b"SQ").copied().filter(|_| art);
        match at {
            Some((x, y, c)) => {
                for row in 0..3 {
                    self.screen.goto(x, y + row);
                    self.screen.set_color(c);
                    self.write(&" ".repeat(80usize.saturating_sub(x as usize)));
                }
                for (i, s) in lines.iter().enumerate() {
                    self.screen.goto(x, y + i as u16);
                    self.say(s).await?;
                }
                self.screen.goto(x, y + lines.len() as u16);
            }
            None => {
                for s in lines {
                    self.say(s).await?;
                    self.write(" ");
                }
            }
        }
        let words = self.strings().get("SixelChoices").to_string();
        let words: Vec<&str> = words.split('|').collect();
        let mut labels = vec![words.first().copied().unwrap_or("Yes"), words.get(1).copied().unwrap_or("No")];
        match again {
            Some(true) => labels.push(words.get(2).copied().unwrap_or("show me again")),
            Some(false) => labels.push(words.get(3).copied().unwrap_or("show me")),
            None => {}
        }
        let picked = self.choose_bar(&labels, 1).await?;
        if at.is_none() {
            self.newline();
        }
        Ok(match picked {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        })
    }

    /// New: a row of choices as a lightbar (the yes/no bar's colors):
    /// arrows, 4/6 or space move, a choice's first letter or Enter picks.
    async fn choose_bar(&mut self, labels: &[&str], start: usize) -> io::Result<usize> {
        let x = self.screen.where_x();
        let mut cur = start.min(labels.len() - 1);
        loop {
            self.screen.pos_x(x);
            self.show(&bar_text(labels, cur)).await?;
            self.flush().await?;
            let key = self.read_key().await?;
            let c = key.upper();
            match key {
                crate::input::Key::Enter => return Ok(cur),
                crate::input::Key::Left => cur = (cur + labels.len() - 1) % labels.len(),
                crate::input::Key::Right => cur = (cur + 1) % labels.len(),
                _ => match c {
                    Some('4') => cur = (cur + labels.len() - 1) % labels.len(),
                    Some('6' | ' ') => cur = (cur + 1) % labels.len(),
                    Some(k) => {
                        if let Some(i) = labels.iter().position(|l| l.chars().next().map(|f| f.to_ascii_uppercase()) == Some(k)) {
                            cur = i;
                            self.screen.pos_x(x);
                            self.show(&bar_text(labels, cur)).await?;
                            return Ok(cur);
                        }
                    }
                    None => {}
                },
            }
        }
    }
}

fn bar_text(labels: &[&str], cur: usize) -> String {
    let bar: String = labels.iter().enumerate().map(|(i, l)| format!("{} {l} ", if i == cur { "|U8" } else { "|U7" })).collect();
    format!("{bar}|U1")
}
