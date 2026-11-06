//! New: sixel pictures offered at logon (`UX`, the `sixel` logon step),
//! once a caller: the SIXEL screen says what they are and whether the
//! caller's terminal has said it shows them, offers a sample (SIXEL.PNG,
//! .JPG or .GIF beside the screen) and asks whether to turn them on.
//! SIXEL's markers: `~ST` the terminal's two lines, `~SQ` where the
//! questions go (three rows: two of question, one for the answer),
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
        if let Some(path) = sample
            && self.sixel_ask(art, &["SixelTry"]).await?
        {
            // Shown in sixels whatever their setting, which stays off.
            self.user.sixel = true;
            let caption = self.strings().get("SixelSample").to_string();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let p = path.clone();
            let r = self.show_picture_key(&name, Some(caption), path, String::new(), move || std::fs::read(&p)).await;
            self.user.sixel = false;
            r?;
            self.sixel_screen(can).await?;
        }
        let on = self.sixel_ask(art, &["SixelEnable", "SixelEnable2"]).await?;
        self.user.sixel = on;
        self.user.sixel_asked = true;
        self.save_user();
        tracing::info!("{}: sixel pictures {}", self.user.handle, if on { "on" } else { "left off" });
        let said = if on { "SixelOn" } else { "SixelOff" };
        if art && self.goto_pos(*b"SR") {
            self.say(said).await?;
            self.flush().await?;
            self.read_key().await?;
        } else {
            self.say_line(said).await?;
        }
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

    /// A question's lines at `~SQ` (cleared first), the yes/no under
    /// them; or on lines of their own.
    async fn sixel_ask(&mut self, art: bool, lines: &[&str]) -> io::Result<bool> {
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
        self.yes_no(false).await
    }
}
