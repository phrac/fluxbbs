//! Logging on (`Logon.pas`, `Apply.pas`, parts of `Users.pas`) and the
//! top-level call flow from `Iniquity.pas`.

use std::io;
use std::time::Instant;

use chrono::Local;

use crate::input::{Case, DIGITS, Key, ReadOpts};
use crate::menu::Menu;
use crate::session::Session;
use crate::user::{User, parse_date};

impl Session {
    /// A whole call: prelogon, logon (via the matrix menu or directly),
    /// then the menu loop until hang-up.
    pub async fn call(&mut self) -> io::Result<()> {
        self.logon_init().await?;
        if self.state.config.logon.matrix {
            self.menu.current = "MATRIX".into();
            self.menu_install().await?;
            self.menu.next_cmd.clear();
            self.menu.exec_first = false;
            while !self.hangup && !self.logged_in {
                self.menu_exec().await?;
            }
        } else {
            self.logon_system().await?;
        }
        if !self.logged_in || self.hangup {
            return self.flush().await;
        }

        self.say("InitBBS").await?;
        self.user_logon_init();
        tracing::info!("{} ({}) logged in", self.user.handle, self.user.real_name);
        self.node.update(|n| {
            n.user = self.user.handle.clone();
            n.location = self.user.location.clone();
        });

        let start = &self.user.start_menu;
        let dir = self.state.config.resolve(&self.state.config.paths.menus);
        self.menu.current = if !start.is_empty() && Menu::load(&dir, start).is_ok() {
            start.clone()
        } else {
            self.state.config.logon.start_menu.clone()
        };
        self.menu_install().await?;
        self.menu.next_cmd.clear();
        self.queue_first_cmd();
        self.menu.exec_first = !self.menu.next_cmd.is_empty();
        self.newline();
        while !self.hangup {
            self.menu_exec().await?;
        }
        self.flush().await
    }

    /// `logonInit`: report the terminal and show PRELOGON.
    async fn logon_init(&mut self) -> io::Result<()> {
        self.say("DetectEmu").await?;
        self.delay(300).await?;
        self.say_line("DetectANSi").await?;
        // ConnectedAt ("Connected at %S1 baud") has no meaning without a modem.
        self.delay(700).await?;
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        self.show_file("PRELOGON").await?;
        Ok(())
    }

    /// `logonSystem` (`>L`): log in, or apply as a new user, then show the
    /// welcome screens.
    pub async fn logon_system(&mut self) -> io::Result<()> {
        if self.get_login().await? && !self.logged_in {
            self.apply().await?;
        }
        if !self.logged_in {
            if !self.state.config.logon.matrix {
                self.hangup = true;
            }
            return Ok(());
        }
        self.show_file("WELCOME").await?;
        for n in 1..=9 {
            if !self.show_file(&format!("WELCOME.{n}")).await? {
                break;
            }
        }
        let (sl, dsl) = (self.user.sl, self.user.dsl);
        self.show_file(&format!("SL{sl}")).await?;
        self.show_file(&format!("DSL{dsl}")).await?;
        Ok(())
    }

    /// `logonGetLogin`: ask for a name and password. Returns true on a
    /// good login, or when the caller chose to apply (`logged_in` false).
    async fn get_login(&mut self) -> io::Result<bool> {
        let cfg = &self.state.config;
        let (tries, ask_apply) = (cfg.logon.tries, cfg.logon.ask_apply);
        let case = Case::from_alias_format(cfg.newuser.alias_format);
        let mut attempt = 0;
        loop {
            self.newline();
            self.say("LoginName").await?;
            // Offer the SSH login name first if it's an account here.
            let mut opts = ReadOpts::new(36).case(case).required();
            if attempt == 0 && self.state.users.find(&self.caller.login_name, false).ok().flatten().is_some() {
                opts = opts.prefill(self.caller.login_name.clone());
            }
            let name = self.read_string(opts).await?;
            self.newline();
            let mut ok = false;
            let found = if ask_apply && name.eq_ignore_ascii_case("NEW") {
                tracing::info!("caller asked to apply as a new user");
                ok = true;
                None
            } else {
                self.state.users.find(&name, false).map_err(io::Error::other)?
            };
            match found {
                None if !ok => {
                    self.say_line("LoginUnknown").await?;
                    if ask_apply {
                        tracing::info!("unknown user {name:?}, offering application");
                        self.say("LoginAskApply").await?;
                        ok = self.yes_no(false).await?;
                    }
                }
                None => {}
                Some(user) => {
                    self.say("LoginPassword").await?;
                    let pw = self.read_string(ReadOpts::new(20).password()).await?;
                    self.newline();
                    ok = user.check_password(&pw);
                    if !ok {
                        tracing::info!("{}: wrong password", user.handle);
                    }
                    let prelogon = std::mem::replace(&mut self.user, user);
                    let sys_pw = self.state.config.logon.system_password.clone();
                    if ok && !sys_pw.is_empty() && self.acs(&self.state.config.logon.system_password_acs.clone()) {
                        self.say("LoginSystemPW").await?;
                        let s = self.read_string(ReadOpts::new(20).password()).await?;
                        self.newline();
                        ok = s.eq_ignore_ascii_case(&sys_pw);
                        if !ok {
                            tracing::info!("{}: wrong system password", self.user.handle);
                        }
                    }
                    if ok {
                        self.logged_in = true;
                    } else {
                        self.user = prelogon;
                        self.say_line("LoginError").await?;
                        self.delay(1000).await?;
                    }
                }
            }
            attempt += 1;
            if ok || self.hangup || (tries > 0 && attempt >= tries) {
                return Ok(ok && !self.hangup);
            }
        }
    }

    /// `userLogonInit`: count the call and reset daily limits.
    fn user_logon_init(&mut self) {
        let today = Local::now().date_naive();
        let u = &mut self.user;
        let new_day = u.last_call != Some(today);
        if new_day || self.state.config.logon.time_per_call {
            u.time_today = u.time_per_day;
        }
        if new_day {
            u.today_dl = 0;
            u.today_dl_kb = 0;
            u.calls_today = 1;
        } else {
            u.calls_today += 1;
        }
        u.total_calls += 1;
        // New files are those since the last call.
        if u.last_call.is_some() {
            u.file_scan = u.last_call;
        }
        u.first_call.get_or_insert(today);
        u.last_call = Some(today);
        // confLoad / maFindAreaWithAccess
        let confs = self.state.areas.confs.len().max(1) as u32;
        u.conf_msg = u.conf_msg.clamp(1, confs);
        u.conf_file = u.conf_file.clamp(1, self.state.fareas().confs.len().max(1) as u32);
        self.find_area_with_access();
        self.find_farea_with_access();
        self.bump(crate::sysdata::Stat::Call);
        if self.new_user {
            self.bump(crate::sysdata::Stat::NewUser);
        }
        self.log_call();
        self.time_on = Instant::now();
        if let Err(e) = self.state.users.save(&self.user) {
            tracing::error!("saving {}: {e}", self.user.handle);
        }
    }

    /// `applyToBBS`: the new-user application.
    pub(crate) async fn apply(&mut self) -> io::Result<()> {
        let cfg = &self.state.config;
        let (nu_pw, verify, real_names) = (cfg.newuser.password.clone(), cfg.newuser.verify, cfg.logon.real_names);
        let case = Case::from_alias_format(cfg.newuser.alias_format);
        self.show_file("APPLY").await?;
        if verify {
            self.say("NuAskApply").await?;
            if !self.yes_no(true).await? {
                return Ok(());
            }
        }
        tracing::info!("new user application started");
        self.newline();
        if !nu_pw.is_empty() {
            self.say("NuEnterNewUserPW").await?;
            let s = self.read_string(ReadOpts::new(20).password()).await?;
            self.newline();
            if !s.eq_ignore_ascii_case(&nu_pw) {
                self.say_line("NuBadNewUserPW").await?;
                return Ok(());
            }
        }

        let mut u = User { palette: self.state.config.colors.clone(), ..User::default() };
        if !real_names {
            loop {
                self.say("NuHandle").await?;
                let h = self.read_string(ReadOpts::new(36).case(case).required()).await?;
                self.newline();
                if h.eq_ignore_ascii_case("NEW") || h.eq_ignore_ascii_case("ALL") || h.starts_with(|c: char| c.is_ascii_digit()) {
                    self.say_line("NuBadAlias").await?;
                } else if self.name_taken(&h)? {
                    self.say_line("NuDupUser").await?;
                } else {
                    u.handle = h;
                    break;
                }
            }
        }
        loop {
            self.say("NuName").await?;
            let n = self.read_string(ReadOpts::new(36).case(Case::Mixed).required()).await?;
            self.newline();
            if n.starts_with(|c: char| c.is_ascii_digit()) {
                self.say_line("NuBadName").await?;
            } else if !n.contains(' ') {
                self.say_line("NuFullName").await?;
            } else if self.name_taken(&n)? {
                self.say_line("NuDupUser").await?;
            } else {
                u.real_name = n;
                break;
            }
        }
        if real_names {
            u.handle = u.real_name.clone();
        }

        let today = Local::now().date_naive();
        loop {
            self.say("NuBirthDate").await?;
            let s = self.read_string(ReadOpts::new(8).allowed("0123456789/").required()).await?;
            self.newline();
            u.birth_date = parse_date(&s);
            match u.age(today) {
                Some(3..=90) => break,
                _ => self.say_line("NuBadBirth").await?,
            }
        }
        let age = u.age(today).unwrap_or(0).to_string();
        let s = self.strings().fmt("NuYourAgeIs", &[&age]);
        self.show_line(&s).await?;

        self.say("NuPhoneNum").await?;
        let digits = self.read_string(ReadOpts::new(10).allowed(DIGITS)).await?;
        self.newline();
        u.phone = format_phone(&digits);
        self.say("NuAddress").await?;
        u.address = self.read_string(ReadOpts::new(36).case(Case::Mixed)).await?;
        self.newline();
        self.say("NuLocation").await?;
        let city = self.read_string(ReadOpts::new(36).case(Case::Mixed)).await?;
        self.newline();

        self.say("NuAskCountry").await?;
        let country = loop {
            if let Some(c @ ('U' | 'C' | 'O')) = self.read_key().await?.upper() { break c }
        };
        self.write(match country {
            'U' => "USA",
            'C' => "Canada",
            _ => "Other",
        });
        self.newline();
        let region = if country == 'O' {
            self.say("NuEnterCountry").await?;
            self.read_string(ReadOpts::new(20).case(Case::Mixed)).await?
        } else {
            self.say("NuAskState").await?;
            self.read_string(ReadOpts::new(2).case(Case::Upper).allowed("ABCDEFGHIJKLMNOPQRSTUVWXYZ").required()).await?
        };
        self.newline();
        u.location = if region.is_empty() { city } else { format!("{city}, {region}") };
        let format = match country {
            'U' => "#####-####",
            'C' => "l#l-l#l",
            _ => "n/a",
        };
        let s = self.strings().fmt("NuEnterZipCode", &[format]);
        self.show(&s).await?;
        u.zip_code = self.read_string(ReadOpts::new(10).case(Case::Upper)).await?;
        self.newline();

        self.say("NuAskSex").await?;
        u.sex = loop {
            if let Some(c @ ('M' | 'F')) = self.read_key().await?.upper() { break c }
        };
        self.write(if u.sex == 'M' { "Male" } else { "Female" });
        self.newline();

        let nu = &self.state.config.newuser;
        let (page_len, ask_page) = (nu.page_length, nu.ask_page_length);
        let toggles = [
            ("NuHotKey", nu.ask_hot_keys, nu.hot_keys),
            ("NuExpertMode", nu.ask_expert, nu.expert),
            ("NuPausing", nu.ask_pause, nu.pause),
            ("NuAutoQuote", nu.ask_quote, nu.quote),
            ("NuYesNoBar", nu.ask_yes_no_bars, nu.yes_no_bars),
        ];
        u.page_length = page_len;
        if ask_page {
            self.say("NuAskPageLen").await?;
            let s = self.read_string(ReadOpts::new(3).allowed(DIGITS).prefill(page_len.to_string())).await?;
            self.newline();
            u.page_length = s.parse::<u16>().unwrap_or(page_len).clamp(5, 200);
        }
        let mut answers = [false; 5];
        for (i, (name, ask, default)) in toggles.into_iter().enumerate() {
            answers[i] = default;
            if ask {
                self.say(name).await?;
                answers[i] = self.yes_no(default).await?;
            }
        }
        [u.hot_keys, u.expert, u.pause, u.quote, u.yes_no_bars] = answers;

        let pw = self.ask_new_password("NuPassword", "NuSmallPassword", "NuValidatePW", "NuPWnoMatch").await?;
        u.set_password(&pw);

        u.first_call = Some(today);
        u.last_call = None;
        u.start_menu = self.state.config.logon.start_menu.clone();
        if let Some(level) = self.state.config.level('A').cloned() {
            u.apply_level('A', &level);
        }
        self.say("NuSavingInfo").await?;
        self.newline();
        self.state.users.insert(&mut u).map_err(io::Error::other)?;
        tracing::info!("new user {} (#{}) applied", u.handle, u.id);
        self.user = u;
        self.logged_in = true;
        self.new_user = true;
        Ok(())
    }

    fn name_taken(&self, name: &str) -> io::Result<bool> {
        Ok(self.state.users.find(name, true).map_err(io::Error::other)?.is_some())
    }

    /// Ask for a new password (at least 4 characters) twice.
    async fn ask_new_password(&mut self, ask: &str, short: &str, again: &str, mismatch: &str) -> io::Result<String> {
        loop {
            self.say(ask).await?;
            let pw = self.read_string(ReadOpts::new(20).password()).await?;
            self.newline();
            if pw.chars().count() < 4 {
                self.say_line(short).await?;
                continue;
            }
            self.say(again).await?;
            let check = self.read_string(ReadOpts::new(20).password()).await?;
            self.newline();
            if check == pw {
                return Ok(pw);
            }
            self.say_line(mismatch).await?;
        }
    }

    /// `userPassword` (`UP`).
    pub(crate) async fn change_password(&mut self) -> io::Result<()> {
        self.newline();
        self.say("PwAskOld").await?;
        let old = self.read_string(ReadOpts::new(20).password()).await?;
        self.newline();
        if !self.user.check_password(&old) {
            self.say_line("PwWrong").await?;
            return Ok(());
        }
        let pw = self.ask_new_password("PwNewPassword", "PwTooSmall", "PwValidate", "PwNoMatch").await?;
        self.user.set_password(&pw);
        self.state.users.save(&self.user).map_err(io::Error::other)?;
        self.say_line("PwChanged").await
    }

    /// Wait for any key (used by `-K`).
    pub(crate) async fn any_key(&mut self) -> io::Result<Key> {
        self.read_key().await
    }
}

/// Digits as "(xxx)xxx-xxxx".
fn format_phone(d: &str) -> String {
    if d.len() == 10 {
        format!("({}){}-{}", &d[..3], &d[3..6], &d[6..])
    } else {
        d.to_string()
    }
}
