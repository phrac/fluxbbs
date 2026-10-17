//! Menu commands (`MenuCmd.pas`). Each command is two characters plus a
//! parameter; see section 5.4 of the Iniquity manual.

use std::io;

use iniquity_term::Slot;
use iniquity_term::codes::resize;

use crate::input::Key;
use crate::session::Session;

/// Commands the original had that aren't ported yet. They report that,
/// rather than "invalid command", so stock menus degrade honestly.
const NOT_YET: &[&str] = &[
    "?I", "?T", "AR", "AS", "AW", "SD", "SE", "SS", "SU", "BA", "BC", "BD", "BL", "BS", "D0", "D1",
    "D2", "D3", "DZ", "E+", "E-", "E/", "E\\", "F[", "F]", "F+", "FA", "FC", "FD", "FF", "FI", "FJ",
    "FL", "FN", "FS", "FT", "FU", "FV", "FZ", "F#", "IM", "IN", "LC", "LH", "M#", "NV", "QD", "QU",
    "TP", "TS", "UC", "XC", "XM", "XB", "XS", "CP", ">C", "!D", "!W", "!E", "!I", "!V", "-X",
];

impl Session {
    /// `menuCommand`: run one command.
    pub async fn run_command(&mut self, code: &str, param: &str) -> io::Result<()> {
        let code = code.to_uppercase();
        let semis: Vec<&str> = param.split(';').collect();
        let semi = |i: usize| semis.get(i).copied().unwrap_or("");
        let num = |s: &str| s.trim().parse::<u16>().unwrap_or(0);
        let mut known = true;
        self.menu.reload = false;

        match code.as_str() {
            // -- input string and OK flag
            "?=" => self.input_string = param.into(),
            "?+" => self.input_string.push_str(param),
            "?/" => self.input_string.insert_str(0, param),
            "?%" => {
                let n = num(param) as usize;
                let mut s = String::new();
                for tok in iniquity_term::tokenize(&iniquity_term::cp437::encode(&self.input_string), iniquity_term::Source::String) {
                    match tok {
                        iniquity_term::Token::Mci(c) => {
                            let v = self.mci_value(c).unwrap_or_else(|| format!("%{}", String::from_utf8_lossy(&c)));
                            s.push_str(&if n > 0 { iniquity_term::codes::squish(&v, n) } else { v });
                        }
                        iniquity_term::Token::Text(b) => s.push_str(&iniquity_term::cp437::decode(&b)),
                        _ => {}
                    }
                }
                self.input_string = s;
            }
            "?E" => self.ok_flag = self.text_file_exists(param),
            "?F" => self.ok_flag = param.trim_start().starts_with(['Y', 'y', 'T', 't']),
            "?L" => self.input_string = self.minutes_left().to_string(),
            "?N" | "?Y" => {
                if !param.is_empty() {
                    self.show(param).await?;
                }
                self.ok_flag = self.yes_no(code == "?Y").await?;
            }
            "?S" => self.ok_flag = param == self.input_string,
            "?U" => self.ok_flag = param.eq_ignore_ascii_case(&self.input_string),
            "?X" => self.input_string = "0".into(),
            "?B" => self.x_prompt(&semis).await?,

            // -- menus and display
            "-^" | "-/" | "-\\" => self.menu_change(code.chars().nth(1).unwrap(), param),
            "-<" | "->" | "-Z" => {} // remote I/O toggles and buffer drain: nothing to do
            "-=" => {
                let key = param.trim().chars().next().unwrap_or('A').to_ascii_uppercase();
                if let Some(level) = self.state.config.level(key).cloned() {
                    self.user.apply_level(key, &level);
                    self.save_user();
                }
            }
            "-?" => self.menu_cmd_help().await?,
            "-C" => self.clear_screen(),
            "-D" => self.delay(num(param) as u64 * 1000).await?,
            "-E" => self.show(param).await?,
            "-F" => {
                self.show_file(param).await?;
            }
            "-K" => {
                self.show(param).await?;
                self.any_key().await?;
            }
            "-L" => self.show_line(param).await?,
            "-P" => self.prompt_key().await?,
            "-R" => {
                for _ in 0..num(param).max(1) {
                    self.newline();
                }
            }
            "-S" => {
                let mut on = true;
                for c in param.to_uppercase().chars() {
                    match c {
                        '+' => on = true,
                        '-' => on = false,
                        'A'..='Z' if on => {
                            self.user.flags.insert(c);
                        }
                        'A'..='Z' => {
                            self.user.flags.remove(&c);
                        }
                        _ => {}
                    }
                }
            }
            "-W" => tracing::info!("{}: {param}", self.user.handle),

            // -- cursor
            "|B" => self.screen.move_down(25),
            "|C" => self.screen.goto(1, 1),
            "|D" => self.screen.move_down(num(semi(0)).max(1)),
            "|E" => self.screen.move_right(80),
            "|H" => self.screen.move_left(80),
            "|L" => self.screen.move_left(num(semi(0)).max(1)),
            "|M" => self.screen.goto(num(semi(0)), num(semi(1))),
            "|R" => self.screen.move_right(num(semi(0)).max(1)),
            "|U" => self.screen.move_up(num(semi(0)).max(1)),
            "|T" => self.screen.move_up(25),
            "|Z" => self.screen.clear_eol(),

            // -- matrix logon
            ">A" => self.apply().await?,
            ">H" => self.hangup = true,
            ">L" => self.logon_system().await?,

            // -- logoff
            "HC" => {
                self.show(param).await?;
                if self.yes_no(true).await? {
                    self.clear_screen();
                    self.show_file("LOGOFF").await?;
                    self.hangup = true;
                }
            }
            "HI" => {
                if !param.is_empty() {
                    self.show_line(param).await?;
                }
                self.hangup = true;
            }
            "HM" => {
                self.show(param).await?;
                self.show_file("LOGOFF").await?;
                self.hangup = true;
            }

            // -- messages
            "MA" => self.list_areas(false).await?,
            "MC" => {
                let ok = match param.trim().parse::<u32>() {
                    Ok(n) => self.change_area(n).await?,
                    Err(_) => false,
                };
                if !ok {
                    self.list_areas(true).await?;
                }
            }
            "M[" => self.step_area(false).await?,
            "M]" => self.step_area(true).await?,
            "MJ" => self.select_conf(param).await?,
            "MZ" => self.list_confs().await?,
            "MN" => self.newscan_cmd(param).await?,
            "MM" => self.mandatory_scan(param).await?,
            "MP" => {
                if let Some(a) = self.current_area() {
                    self.post(&a, semi(0), semi(1), None).await?;
                }
            }
            "MR" => {
                if let Some(a) = self.current_area() {
                    let mandatory = param.to_ascii_uppercase().contains('M');
                    self.read_messages(&a, 0, mandatory, true).await?;
                }
            }
            "MS" => self.set_pointer_date().await?,

            // -- email
            "EE" => self.enter_email(semi(0), semi(1)).await?,
            "ER" => self.read_email(false, false).await?,
            "EC" => self.read_email(false, true).await?,
            "*R" if self.is_sysop() => self.read_email(true, false).await?,

            // -- users and nodes
            "UL" => self.user_list().await?,
            "UP" => self.change_password().await?,
            "XL" => self.node_list().await?,

            _ if NOT_YET.contains(&code.as_str()) || code.starts_with('*') => {
                self.newline();
                self.show_line(&format!("|U4That feature isn't available yet |U5(|U6{code}|U5)|U4.")).await?;
                tracing::debug!("unported command {code} {param:?}");
            }
            _ => known = false,
        }

        self.menu.last_cmd_good = known;
        if !known {
            self.newline();
            self.set_slot(Slot::Error);
            let s = self.strings().fmt("InvalidMnuCmd", &[&code]);
            self.show_line(&s).await?;
        }
        if self.menu.reload && !self.hangup {
            self.menu.reload = false;
            self.menu_install().await?;
            self.menu.last_cmd_good = false;
            self.queue_first_cmd();
        }
        Ok(())
    }

    /// `?B` (`iXprompt`): a horizontal lightbar of choices. The parameter
    /// is `choice;choice;...;default`; the pick goes in the input string.
    async fn x_prompt(&mut self, semis: &[&str]) -> io::Result<()> {
        self.input_string.clear();
        let Some((def, opts)) = semis.split_last() else { return Ok(()) };
        let def: usize = def.trim().parse().unwrap_or(0);
        if opts.is_empty() || def == 0 || def > opts.len() {
            return Ok(());
        }
        let mut xs = Vec::new();
        self.set_slot(Slot::Item);
        for o in opts {
            xs.push(self.screen.where_x());
            self.show(&format!(" {o} ")).await?;
        }
        let mut cur = def - 1;
        let draw = |s: &mut Self, i: usize, hi: bool| {
            s.screen.pos_x(xs[i]);
            s.set_slot(if hi { Slot::ItemSel } else { Slot::Item });
            s.write_with_controls(&format!(" {} ", opts[i]));
        };
        draw(self, cur, true);
        loop {
            let key = self.read_key().await?;
            let next = match key {
                Key::Right | Key::Char(b'6' | b' ') => (cur + 1) % opts.len(),
                Key::Left | Key::Char(b'4') => (cur + opts.len() - 1) % opts.len(),
                Key::Enter => break,
                _ => cur,
            };
            if next != cur {
                draw(self, cur, false);
                cur = next;
                draw(self, cur, true);
            }
        }
        self.set_slot(Slot::Info);
        self.newline();
        self.input_string = opts[cur].to_string();
        Ok(())
    }

    fn save_user(&self) {
        if self.user.id != 0
            && let Err(e) = self.state.users.save(&self.user) {
                tracing::error!("saving {}: {e}", self.user.handle);
            }
    }

    /// `userShowList` (`UL`), text version.
    async fn user_list(&mut self) -> io::Result<()> {
        let users = self.state.users.list().map_err(io::Error::other)?;
        self.clear_screen();
        self.set_slot(Slot::Info);
        self.show_line("|U4 Num   Username                 Location              Calls  Level").await?;
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(79));
        self.newline();
        let mut lines = 3;
        for u in users.iter().filter(|u| !u.deleted) {
            let line = format!(
                "|U1 {} {} {} {} {}",
                resize(&u.id.to_string(), 5),
                resize(&u.handle, 24),
                resize(&u.location, 21),
                resize(&u.total_calls.to_string(), 6),
                u.level
            );
            self.show_line(&line).await?;
            lines += 1;
            if !self.more(&mut lines).await? {
                break;
            }
        }
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(79));
        self.newline();
        Ok(())
    }

    /// `nodeList` (`XL`), text version.
    async fn node_list(&mut self) -> io::Result<()> {
        self.newline();
        self.set_slot(Slot::Info);
        self.show_line("|U4 Node  User                     Activity").await?;
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(60));
        self.newline();
        for (n, info) in self.state.nodes.online() {
            let user = if info.user.is_empty() { "(logging in)".to_string() } else { info.user };
            let line = format!("|U1 {} |U3{}|U1 {}", resize(&n.to_string(), 5), resize(&user, 24), info.activity);
            self.show_line(&line).await?;
        }
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(60));
        self.newline();
        Ok(())
    }
}

