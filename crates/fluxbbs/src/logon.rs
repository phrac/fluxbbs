//! Logging on (`Logon.pas`, `Apply.pas`, parts of `Users.pas`) and the
//! top-level call flow from `Iniquity.pas`.

use std::io;
use std::time::Instant;


use crate::input::{Case, DIGITS, Key, ReadOpts};
use crate::menu::Menu;
use crate::password;
use crate::session::Session;
use crate::user::{User, parse_date};

/// Count a failed bot check or password against `addr` (in the security
/// log, and toward a ban). True if it earned a ban.
pub(crate) fn record_failure(state: &crate::BbsState, addr: std::net::SocketAddr, what: &str) -> bool {
    tracing::warn!("{addr}: {what}");
    let ip = addr.ip().to_string();
    let log = |what: &str| {
        if let Err(e) = state.sys.log_security(&ip, what) {
            tracing::error!("security log: {e}");
        }
    };
    log(what);
    match state.guard.failure(&state.config().security, addr.ip()) {
        Some(len) => {
            let minutes = len.as_secs() / 60;
            tracing::warn!("{ip}: banned for {minutes} minutes");
            log(&format!("banned for {minutes} minutes"));
            let until = crate::clock::now().naive_local() + chrono::Duration::from_std(len).unwrap_or_default();
            if let Err(e) = state.sys.set_ban(&ip, until, state.guard.strikes(addr.ip())) {
                tracing::error!("recording ban: {e}");
            }
            true
        }
        None => false,
    }
}

impl Session {
    /// A whole call: prelogon, logon (via the matrix menu or directly),
    /// then the menu loop until hang-up.
    pub async fn call(&mut self) -> io::Result<()> {
        let keyed = match self.caller.key_user {
            Some(id) => self.key_logon(id).await?,
            None => false,
        };
        if !keyed {
            if !self.bot_check().await? {
                self.record_failure("failed the bot check");
                return self.flush().await;
            }
            self.logon_init().await?;
            if self.state.config().logon.matrix {
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
        }
        if !self.logged_in || self.hangup {
            return self.flush().await;
        }

        self.say("InitBBS").await?;
        // When they were last on: the time, or (from before it was kept)
        // the start of that day.
        let previous_call = self.user.last_call_at.or_else(|| self.user.last_call.and_then(|d| d.and_hms_opt(0, 0, 0)));
        self.user_logon_init();
        tracing::info!("{} ({}) logged in", self.user.handle, self.user.real_name);
        // SysOps hear about failed logins and bans since they were last on.
        if self.is_sysop() {
            let since = previous_call.unwrap_or_default();
            if let Ok((n, bans)) = self.state.sys.security_since(since)
                && n > 0 {
                    self.newline();
                    let s = self.strings().fmt("SecurityNotice", &[&n.to_string(), &bans.to_string()]);
                    self.show_line(&s).await?;
                }
        }
        let sysop = self.is_sysop();
        self.node.update(|n| {
            n.user = self.user.handle.clone();
            n.location = self.user.location.clone();
            n.sysop = sysop;
        });

        let start = &self.user.start_menu;
        let dir = self.state.config().menu_dirs();
        self.menu.current = if !start.is_empty() && Menu::load(&dir, start).is_ok() {
            start.clone()
        } else {
            self.state.config().logon.start_menu.clone()
        };
        // New: plain text skips the logon sequence (the start menu's
        // FIRSTCMDs) for a line on what's waiting, and goes to where it
        // would end up.
        if self.plain() {
            let logon = self.state.config().logon.start_menu.clone();
            if self.menu.current.eq_ignore_ascii_case(&logon)
                && let Ok(m) = Menu::load(&dir, &logon)
                && !m.fallback.is_empty()
            {
                self.menu.current = m.fallback;
            }
            self.plain_logon().await?;
        }
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

    /// New: a caller whose SSH client logged in with a key on their
    /// account, or with their own password as the SSH password, comes
    /// straight in after PRELOGON, without the bot check or the login
    /// prompts (SysOps are still asked the system password). False if the
    /// account is gone, to go the usual way.
    async fn key_logon(&mut self, id: u32) -> io::Result<bool> {
        let Some(user) = self.state.users.get(id).map_err(io::Error::other)?.filter(|u| !u.deleted) else { return Ok(false) };
        // New: known from the start, so a bot gets plain text throughout.
        if crate::plain::wants_plain(&user.text_mode, crate::aimemory::is_bot(&user)) {
            self.screen.set_plain(true);
        } else {
            self.logon_init().await?;
        }
        let prelogon = std::mem::replace(&mut self.user, user);
        *self.user_base.borrow_mut() = Some(self.user.clone());
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        if !self.system_password_ok().await? {
            self.user = prelogon;
            self.record_failure("wrong system password");
            self.hangup = true;
            return Ok(true);
        }
        tracing::info!("{}: logged in over SSH, no prompts", self.user.handle);
        self.logged_in = true;
        if let Some(addr) = self.caller.remote {
            self.state.guard.success(addr.ip());
        }
        self.welcome_screens().await?;
        Ok(true)
    }

    /// The system password, where the caller's access asks for it.
    async fn system_password_ok(&mut self) -> io::Result<bool> {
        let cfg = self.state.config();
        let (sys_pw, acs) = (cfg.logon.system_password.clone(), cfg.logon.system_password_acs.clone());
        drop(cfg);
        if sys_pw.is_empty() || !self.acs(&acs) {
            return Ok(true);
        }
        self.login_prompt();
        self.say("LoginSystemPW").await?;
        let s = self.read_string(ReadOpts::new(20).password()).await?;
        self.login_answered();
        let ok = s.eq_ignore_ascii_case(&sys_pw);
        if !ok {
            tracing::info!("{}: wrong system password", self.user.handle);
        }
        Ok(ok)
    }

    /// `logonInit`: report the terminal and show PRELOGON.
    async fn logon_init(&mut self) -> io::Result<()> {
        self.say("DetectEmu").await?;
        self.delay(300).await?;
        self.say_line("DetectANSi").await?;
        // ConnectedAt ("Connected at %S1 baud") has no meaning without a modem.
        self.delay(700).await?;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
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
            if !self.state.config().logon.matrix {
                self.hangup = true;
            }
            return Ok(());
        }
        self.welcome_screens().await
    }

    /// WELCOME (and WELCOME.1-9), then the screens for the caller's
    /// security levels.
    async fn welcome_screens(&mut self) -> io::Result<()> {
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
        let cfg = self.state.config();
        let (tries, ask_apply) = (cfg.logon.tries, cfg.logon.ask_apply);
        let case = Case::from_alias_format(cfg.newuser.alias_format);
        drop(cfg);
        // After a lightbar menu the prompts share one line: the art's ~LP
        // marker, or the bottom row.
        let lightbar = self.menu.menu.as_ref().is_some_and(|m| m.menu_type == crate::menu::MenuType::Lightbar);
        self.login_line = self.positions.get(b"LP").map(|p| p.1).or_else(|| lightbar.then(|| self.screen.rows()));
        let result = self.login_loop(tries, ask_apply, case).await;
        self.login_line = None;
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        result
    }

    /// Start a login prompt: on the login line, cleared, if there is one,
    /// else on a new line.
    fn login_prompt(&mut self) {
        match self.login_line {
            Some(y) => {
                self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                self.screen.goto(1, y);
                self.screen.clear_eol();
            }
            None => self.newline(),
        }
    }

    /// After an answer: the login line stays put; otherwise a new line.
    fn login_answered(&mut self) {
        if self.login_line.is_none() {
            self.newline();
        }
    }

    /// A message during login, held a moment on the login line.
    async fn login_say(&mut self, key: &str) -> io::Result<()> {
        if self.login_line.is_none() {
            return self.say_line(key).await;
        }
        self.login_prompt();
        self.say(key).await?;
        self.delay(1200).await
    }

    async fn login_loop(&mut self, tries: u8, ask_apply: bool, case: Case) -> io::Result<bool> {
        let mut attempt = 0;
        loop {
            self.login_prompt();
            self.say("LoginName").await?;
            // Offer the SSH login name first if it's an account here.
            let mut opts = ReadOpts::new(36).case(case).required();
            if attempt == 0 && self.state.users.find(&self.caller.login_name, false).ok().flatten().is_some() {
                opts = opts.prefill(self.caller.login_name.clone());
            }
            let name = self.read_string(opts).await?;
            self.login_answered();
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
                    self.login_say("LoginUnknown").await?;
                    if ask_apply {
                        tracing::info!("unknown user {name:?}, offering application");
                        self.login_prompt();
                        self.say("LoginAskApply").await?;
                        ok = self.yes_no(false).await?;
                        self.login_answered();
                    }
                }
                None => {}
                Some(user) => {
                    self.login_prompt();
                    self.say("LoginPassword").await?;
                    let pw = self.read_string(ReadOpts::new(password::MAX_LEN).password()).await?;
                    self.login_answered();
                    ok = user.check_password(&pw);
                    if !ok {
                        tracing::info!("{}: wrong password", user.handle);
                        if self.record_failure("wrong password") {
                            self.hangup = true;
                        }
                    }
                    let prelogon = std::mem::replace(&mut self.user, user);
                    *self.user_base.borrow_mut() = Some(self.user.clone());
                    if ok {
                        self.apply_text_mode();
                    }
                    if ok {
                        ok = self.verify_identity().await?;
                    }
                    if ok {
                        ok = self.system_password_ok().await?;
                    }
                    if ok {
                        self.logged_in = true;
                        if let Some(addr) = self.caller.remote {
                            self.state.guard.success(addr.ip());
                        }
                        let rules = self.state.config().passwords.clone();
                        if rules.enforce_at_login && let Err(weak) = password::check(&rules, &pw, &self.user.handle) {
                            // Several lines of its own: off the art first.
                            if self.login_line.take().is_some() {
                                self.clear_screen();
                            }
                            tracing::info!("{}: password breaks the rules ({weak:?}), must change it", self.user.handle);
                            self.say_line("PwMustChange").await?;
                            self.say_weak(&weak).await?;
                            let new = self.ask_new_password(&self.user.handle.clone(), "PwNewPassword", "PwValidate", "PwNoMatch").await?;
                            self.user.set_password(&new);
                            self.save_own_user().map_err(io::Error::other)?;
                            self.say_line("PwChanged").await?;
                        }
                    } else {
                        self.user = prelogon;
                        self.login_say("LoginError").await?;
                        if self.login_line.is_none() {
                            self.delay(1000).await?;
                        }
                    }
                }
            }
            attempt += 1;
            if ok || self.hangup || (tries > 0 && attempt >= tries) {
                return Ok(ok && !self.hangup);
            }
        }
    }

    /// Every `calls_birth` calls ask the birth date, every `calls_phone`
    /// the last four phone digits (the login screen's "other
    /// information"; the code for it isn't in the surviving source).
    async fn verify_identity(&mut self) -> io::Result<bool> {
        let cfg = self.state.config();
        let (every_birth, every_phone) = (cfg.logon.calls_birth as u32, cfg.logon.calls_phone as u32);
        drop(cfg);
        let call = self.user.total_calls + 1;
        if every_birth > 0 && call.is_multiple_of(every_birth) && let Some(birth) = self.user.birth_date {
            self.login_say("LoginHelpBirth").await?;
            self.login_prompt();
            self.say("LoginBDate").await?;
            let s = self.read_string(ReadOpts::new(8).mask("mm/dd/yy")).await?;
            self.login_answered();
            if parse_date(&s) != Some(birth) {
                tracing::info!("{}: wrong birth date at logon", self.user.handle);
                self.record_failure("wrong birth date");
                return Ok(false);
            }
        }
        let digits: String = self.user.phone.chars().filter(char::is_ascii_digit).collect();
        if every_phone > 0 && call.is_multiple_of(every_phone) && digits.len() >= 7 && !digits.ends_with("0000") {
            self.login_say("LoginHelpPhone").await?;
            self.login_prompt();
            self.say("LoginPhone").await?;
            let s = self.read_string(ReadOpts::new(4).allowed(DIGITS)).await?;
            self.login_answered();
            if s != digits[digits.len() - 4..] {
                tracing::info!("{}: wrong phone number at logon", self.user.handle);
                self.record_failure("wrong phone number");
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Count a failed bot check or password against the caller's address.
    /// True if it earned a ban (the caller should be cut off).
    fn record_failure(&self, what: &str) -> bool {
        self.caller.remote.is_some_and(|addr| record_failure(&self.state, addr, what))
    }

    /// `userLogonInit`: count the call and reset daily limits.
    fn user_logon_init(&mut self) {
        let today = crate::clock::now().date_naive();
        let u = &mut self.user;
        let new_day = u.last_call != Some(today);
        if new_day || self.state.config().logon.time_per_call {
            u.time_today = u.time_per_day;
        }
        if new_day {
            u.today_dl = 0;
            u.today_dl_kb = 0;
            u.bank_time_in = 0;
            u.bank_time_out = 0;
            u.bank_kb_in = 0;
            u.bank_kb_out = 0;
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
        u.last_call_at = Some(crate::clock::now().naive_local());
        // confLoad / maFindAreaWithAccess
        let confs = self.state.areas().confs.len().max(1) as u32;
        u.conf_msg = u.conf_msg.clamp(1, confs);
        u.conf_file = u.conf_file.clamp(1, self.state.fareas().confs.len().max(1) as u32);
        self.find_area_with_access();
        self.find_farea_with_access();
        if self.new_user {
            self.bump(crate::sysdata::Stat::NewUser);
        }
        self.log_call();
        self.time_on = Instant::now();
        if let Err(e) = self.save_own_user() {
            tracing::error!("saving {}: {e}", self.user.handle);
        }
    }

    /// `applyToBBS`: the new-user application.
    pub(crate) async fn apply(&mut self) -> io::Result<()> {
        let cfg = self.state.config();
        let (nu_pw, verify, real_names) = (cfg.newuser.password.clone(), cfg.newuser.verify, cfg.logon.real_names);
        let nu = cfg.newuser.clone();
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
        if let Some(refused) = self.signup_refused()? {
            self.say_line(refused).await?;
            return Ok(());
        }

        let mut u = User { palette: self.state.config().colors.clone(), ..User::default() };
        // Details the application doesn't ask for are left blank, not
        // filled with placeholders that would show in user lists.
        if !nu.ask_phone {
            u.phone.clear();
        }
        if !nu.ask_address {
            u.address.clear();
        }
        if !nu.ask_location {
            u.location.clear();
        }
        if !real_names {
            loop {
                self.say("NuHandle").await?;
                let h = self.read_string(ReadOpts::new(36).case(case).required()).await?;
                self.newline();
                // And the names the board gives parties that aren't
                // callers: mail to them mustn't reach someone.
                let anonymous = crate::editor::strip_codes(self.strings().get("Anonymous"));
                let reserved = nu.reserved_handles.iter().chain([&anonymous, &"None".to_string(), &"Anonymous".to_string()]).any(|r| r.trim().eq_ignore_ascii_case(h.trim()));
                if reserved || h.eq_ignore_ascii_case("NEW") || h.eq_ignore_ascii_case("ALL") || h.starts_with(|c: char| c.is_ascii_digit()) || has_codes(&h) {
                    self.say_line("NuBadAlias").await?;
                } else if self.name_taken(&h)? {
                    self.say_line("NuDupUser").await?;
                } else {
                    u.handle = h;
                    break;
                }
            }
        }
        if real_names || nu.ask_real_name {
            loop {
                self.say("NuName").await?;
                let n = self.read_string(ReadOpts::new(36).case(Case::Mixed).required()).await?;
                self.newline();
                if n.starts_with(|c: char| c.is_ascii_digit()) || has_codes(&n) {
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
        }
        if real_names {
            u.handle = u.real_name.clone();
        } else if !nu.ask_real_name {
            u.real_name = u.handle.clone();
        }

        let today = crate::clock::now().date_naive();
        if nu.ask_birth_date {
            loop {
                self.say("NuBirthDate").await?;
                let s = self.read_string(ReadOpts::new(8).mask("mm/dd/yy").required()).await?;
                self.newline();
                u.birth_date = parse_date(&s);
                match u.age(today) {
                    Some(3..=90) => break,
                    _ => self.say_line("NuBadBirth").await?,
                }
            }
        }
        if let Some(age) = u.age(today) {
            let s = self.strings().fmt("NuYourAgeIs", &[&age.to_string()]);
            self.show_line(&s).await?;
        }

        if nu.ask_phone {
            self.say("NuPhoneNum").await?;
            let digits = self.read_string(ReadOpts::new(10).allowed(DIGITS)).await?;
            self.newline();
            u.phone = format_phone(&digits);
        }
        if nu.ask_address {
            self.say("NuAddress").await?;
            u.address = self.read_string(ReadOpts::new(36).case(Case::Mixed)).await?;
            self.newline();
        }
        if nu.ask_location {
            self.ask_location(&mut u).await?;
        }
        if nu.ask_sex {
            self.say("NuAskSex").await?;
            u.sex = loop {
                if let Some(c @ ('M' | 'F' | 'O' | 'P')) = self.read_key().await?.upper() { break c }
            };
            self.write(crate::user::gender_name(u.sex));
            self.newline();
        }
        self.new_user_prefs(&mut u).await?;
        let pw = self.ask_new_password(&u.handle.clone(), "NuPassword", "NuValidatePW", "NuPWnoMatch").await?;
        u.set_password(&pw);

        u.first_call = Some(today);
        u.last_call = None;
        u.start_menu = self.state.config().logon.start_menu.clone();
        if let Some(level) = self.state.level('A') {
            u.apply_level('A', &level);
        }
        self.say("NuSavingInfo").await?;
        self.newline();
        self.state.users.insert(&mut u).map_err(io::Error::other)?;
        tracing::info!("new user {} (#{}) applied", u.handle, u.id);
        let ip = self.caller.remote.map(|a| a.ip().to_string()).unwrap_or_default();
        if let Err(e) = self.state.sys.record_signup(&ip) {
            tracing::error!("recording sign-up: {e}");
        }
        self.user = u;
        *self.user_base.borrow_mut() = Some(self.user.clone());
        // `newConfig`: let them check what they entered.
        if nu.config {
            self.user_config_screen("NEWCFG").await?;
            self.save_user();
        }
        self.logged_in = true;
        self.new_user = true;
        Ok(())
    }

    /// City, country, state and zip code.
    async fn ask_location(&mut self, u: &mut User) -> io::Result<()> {
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
        Ok(())
    }

    /// Screen and menu preferences (the `ask_*` toggles in [newuser]).
    async fn new_user_prefs(&mut self, u: &mut User) -> io::Result<()> {
        let nu = &self.state.config().newuser;
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
        Ok(())
    }

    fn name_taken(&self, name: &str) -> io::Result<bool> {
        Ok(self.state.users.find(name, true).map_err(io::Error::other)?.is_some())
    }

    /// Too many sign-ups lately, from this address or overall: the string
    /// to show, if so. Addresses on the allow list aren't limited.
    fn signup_refused(&self) -> io::Result<Option<&'static str>> {
        let nu = self.state.config().newuser.clone();
        let ip = self.caller.remote.map(|a| a.ip());
        if ip.is_some_and(|ip| self.state.guard.trusted(&self.state.config().security, ip)) {
            return Ok(None);
        }
        let ip_str = ip.map(|ip| ip.to_string()).unwrap_or_default();
        let since = crate::clock::now().naive_local() - chrono::Duration::days(1);
        let (here, all) = self.state.sys.signups_since(&ip_str, since).map_err(io::Error::other)?;
        let refused = if nu.per_address_per_day > 0 && ip.is_some() && here >= nu.per_address_per_day {
            Some(("NuLimitAddress", "sign-up refused: daily limit for this address"))
        } else if nu.per_day > 0 && all >= nu.per_day {
            Some(("NuLimitBoard", "sign-up refused: daily limit for the board"))
        } else {
            None
        };
        Ok(refused.map(|(string, what)| {
            tracing::warn!("{ip_str}: {what}");
            if let Err(e) = self.state.sys.log_security(&ip_str, what) {
                tracing::error!("security log: {e}");
            }
            string
        }))
    }

    /// Why a password was refused.
    async fn say_weak(&mut self, weak: &password::Weak) -> io::Result<()> {
        let n = match weak {
            password::Weak::Short(n) => n.to_string(),
            _ => String::new(),
        };
        let s = self.strings().fmt(weak.string(), &[&n]);
        self.show_line(&s).await
    }

    /// Ask for a new password twice, held to the `[passwords]` rules.
    async fn ask_new_password(&mut self, handle: &str, ask: &str, again: &str, mismatch: &str) -> io::Result<String> {
        loop {
            self.say(ask).await?;
            let pw = self.read_string(ReadOpts::new(password::MAX_LEN).password()).await?;
            self.newline();
            let rules = self.state.config().passwords.clone();
            if let Err(weak) = password::check(&rules, &pw, handle) {
                self.say_weak(&weak).await?;
                continue;
            }
            self.say(again).await?;
            let check = self.read_string(ReadOpts::new(password::MAX_LEN).password()).await?;
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
        let old = self.read_string(ReadOpts::new(password::MAX_LEN).password()).await?;
        self.newline();
        if !self.user.check_password(&old) {
            self.say_line("PwWrong").await?;
            return Ok(());
        }
        let pw = self.ask_new_password(&self.user.handle.clone(), "PwNewPassword", "PwValidate", "PwNoMatch").await?;
        self.user.set_password(&pw);
        self.save_own_user().map_err(io::Error::other)?;
        self.say_line("PwChanged").await
    }

    /// Wait for any key (used by `-K`).
    pub(crate) async fn any_key(&mut self) -> io::Result<Key> {
        self.read_key().await
    }
}

/// Digits as "(xxx)xxx-xxxx".
pub(crate) fn format_phone(d: &str) -> String {
    if d.len() == 10 {
        format!("({}){}-{}", &d[..3], &d[3..6], &d[6..])
    } else {
        d.to_string()
    }
}

/// A name holding display codes (`|CS`, `%UN`, `@10`...), which would act
/// wherever it's shown. Not checked by the original.
pub fn has_codes(name: &str) -> bool {
    fluxbbs_term::tokenize(&fluxbbs_term::cp437::encode(name), fluxbbs_term::Source::String)
        .iter()
        .any(|t| !matches!(t, fluxbbs_term::Token::Text(_)))
}
