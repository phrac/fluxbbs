//! The time bank (`!D`, `!W`, `!E`, `!I`, `!V`): callers put minutes
//! they don't need today, or download kilobytes, aside for another day.
//! The commands, the limits (per access level) and the strings are from
//! the Iniquity manual and string table (s306-s318, s324); the code for
//! them isn't in this release's source, so this follows those.
//!
//! Depositing time takes it off today's time left; withdrawing adds it.
//! Kilobytes work on the daily download allowance the same way, so they
//! only mean something when `[files] use_daily_kb_limit` is on and the
//! user has a daily kb limit.

use std::io;

use crate::config::Level;
use crate::input::{DIGITS, ReadOpts};
use crate::session::Session;
use crate::user::User;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Minutes,
    Kb,
}

/// Why a transaction can't happen: a string from the table and its
/// arguments.
#[derive(Debug, PartialEq)]
pub struct Refused {
    pub string: &'static str,
    pub args: Vec<String>,
}

fn refused(string: &'static str, amount: u32, limit: u64) -> Refused {
    Refused { string, args: vec![amount.to_string(), limit.to_string()] }
}

/// Put `amount` in the bank. `available` is what the caller has today
/// (minutes left, or kilobytes still downloadable).
pub fn deposit(u: &mut User, l: &Level, unit: Unit, amount: u32, available: u64) -> Result<(), Refused> {
    let (bank, done, max, per_day) = match unit {
        Unit::Minutes => (u.bank_time, u.bank_time_in, l.bank_time_max, l.bank_time_deposit),
        Unit::Kb => (u.bank_kb, u.bank_kb_in, l.bank_kb_max, l.bank_kb_deposit),
    };
    let (s_left, s_max, s_day) = match unit {
        Unit::Minutes => ("s307", "s306", "s308"),
        Unit::Kb => ("s313", "s314", "s315"),
    };
    if amount as u64 > available {
        return Err(refused(s_left, amount, available));
    }
    if bank as u64 + amount as u64 > max as u64 {
        return Err(refused(s_max, amount, max as u64));
    }
    if done as u64 + amount as u64 > per_day as u64 {
        return Err(refused(s_day, amount, per_day as u64));
    }
    match unit {
        Unit::Minutes => {
            u.bank_time += amount;
            u.bank_time_in += amount;
            u.time_today = u.time_today.saturating_sub(amount);
        }
        Unit::Kb => {
            u.bank_kb += amount;
            u.bank_kb_in += amount;
        }
    }
    Ok(())
}

/// Take `amount` out of the bank.
pub fn withdraw(u: &mut User, l: &Level, unit: Unit, amount: u32) -> Result<(), Refused> {
    let (bank, done, per_day) = match unit {
        Unit::Minutes => (u.bank_time, u.bank_time_out, l.bank_time_withdraw),
        Unit::Kb => (u.bank_kb, u.bank_kb_out, l.bank_kb_withdraw),
    };
    let (s_bank, s_day) = match unit {
        Unit::Minutes => ("s310", "s309"),
        Unit::Kb => ("s317", "s316"),
    };
    if amount > bank {
        return Err(refused(s_bank, amount, bank as u64));
    }
    if done as u64 + amount as u64 > per_day as u64 {
        return Err(refused(s_day, amount, per_day as u64));
    }
    match unit {
        Unit::Minutes => {
            u.bank_time -= amount;
            u.bank_time_out += amount;
            u.time_today += amount;
        }
        Unit::Kb => {
            u.bank_kb -= amount;
            u.bank_kb_out += amount;
        }
    }
    Ok(())
}

impl Session {
    fn bank_level(&self) -> Level {
        self.state.level(self.user.level).unwrap_or_default()
    }

    /// Kilobytes banking only makes sense with a daily kb limit.
    fn kb_banking(&self) -> bool {
        self.state.config().files.use_daily_kb_limit && self.user.limit_dl_kb > 0
    }

    /// `!D`, `!W`, `!E`, `!I`.
    pub async fn bank_transaction(&mut self, unit: Unit, put_in: bool) -> io::Result<()> {
        self.newline();
        if unit == Unit::Kb && !self.kb_banking() {
            return self.say_line("BankNoKbLimit").await;
        }
        let ask = match (unit, put_in) {
            (Unit::Minutes, true) => "BankAskDepositTime",
            (Unit::Minutes, false) => "BankAskWithdrawTime",
            (Unit::Kb, true) => "s311",
            (Unit::Kb, false) => "s312",
        };
        self.say(ask).await?;
        self.write(" ");
        let s = self.read_string(ReadOpts::new(5).allowed(DIGITS)).await?;
        self.newline();
        let amount: u32 = match s.trim().parse() {
            Ok(n) if n > 0 => n,
            _ => return Ok(()),
        };
        // Time a caller has without limit isn't theirs to bank.
        if put_in && unit == Unit::Minutes && self.time_unlimited() {
            return self.say_line("BankUnlimited").await;
        }
        let level = self.bank_level();
        let available = match unit {
            Unit::Minutes => self.minutes_left() as u64,
            Unit::Kb => self.user.dl_kb_allowance().saturating_sub(self.user.today_dl_kb as u64),
        };
        let result = if put_in {
            deposit(&mut self.user, &level, unit, amount, available)
        } else {
            withdraw(&mut self.user, &level, unit, amount)
        };
        match result {
            Ok(()) => {
                self.save_user();
                let what = if unit == Unit::Minutes { "min" } else { "kb" };
                tracing::info!(
                    "{}: time bank {} {amount} {what} (bank now {}m/{}k)",
                    self.user.handle,
                    if put_in { "deposit" } else { "withdrawal" },
                    self.user.bank_time,
                    self.user.bank_kb
                );
                self.say_line("s324").await
            }
            Err(r) => {
                let args: Vec<&str> = r.args.iter().map(String::as_str).collect();
                let s = self.strings().fmt(r.string, &args);
                self.show_line(&s).await
            }
        }
    }

    /// `!V`: the caller's account.
    pub async fn bank_stats(&mut self) -> io::Result<()> {
        let l = self.bank_level();
        let u = self.user.clone();
        self.newline();
        let row = |what: &str, bank: u32, max: u16, inn: u32, in_max: u16, out: u32, out_max: u16, unit: &str| {
            format!(
                "|U1{what} |U2: |U3{bank}{unit} |U2of |U3{max}{unit}  |U1in today |U3{inn}|U2/|U3{in_max}  |U1out today |U3{out}|U2/|U3{out_max}"
            )
        };
        self.show_line("|U4Time bank").await?;
        let t = row("Time ", u.bank_time, l.bank_time_max, u.bank_time_in, l.bank_time_deposit, u.bank_time_out, l.bank_time_withdraw, "m");
        self.show_line(&t).await?;
        if self.kb_banking() {
            let k = row("Kbytes", u.bank_kb, l.bank_kb_max, u.bank_kb_in, l.bank_kb_deposit, u.bank_kb_out, l.bank_kb_withdraw, "k");
            self.show_line(&k).await?;
        }
        let left = format!("|U1Time left today |U2: |U3{}m", self.minutes_left());
        self.show_line(&left).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validated_callers_have_no_time_limit() {
        let state = crate::tests::state();
        let mut cfg = (*state.config()).clone();
        cfg.bbs.unlimited_time_acs = "s50".into();
        *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
        let mut u = crate::user::User { handle: "phrac".into(), sl: 50, time_today: 3, ..Default::default() };
        state.users.insert(&mut u).unwrap();
        let mut s = Session::for_test(state.clone(), u);
        s.logged_in = true;
        assert!(s.time_unlimited());
        assert_eq!(s.minutes_left(), Session::UNLIMITED_MINUTES);
        // Below the line, the level's time as ever.
        s.user.sl = 49;
        assert!(!s.time_unlimited());
        assert_eq!(s.minutes_left(), 3);
    }

    fn level() -> Level {
        Level { bank_time_max: 60, bank_time_deposit: 30, bank_time_withdraw: 20, bank_kb_max: 1000, bank_kb_deposit: 500, bank_kb_withdraw: 400, ..Level::default() }
    }

    #[test]
    fn time_in_and_out() {
        let l = level();
        let mut u = User { time_today: 50, ..User::default() };
        deposit(&mut u, &l, Unit::Minutes, 25, 45).unwrap();
        assert_eq!((u.bank_time, u.bank_time_in, u.time_today), (25, 25, 25));
        // Only 30 a day go in.
        assert_eq!(deposit(&mut u, &l, Unit::Minutes, 10, 20).unwrap_err().string, "s308");
        // Can't put in more than is left today.
        assert_eq!(deposit(&mut u, &l, Unit::Minutes, 5, 4).unwrap_err(), Refused { string: "s307", args: vec!["5".into(), "4".into()] });
        withdraw(&mut u, &l, Unit::Minutes, 20).unwrap();
        assert_eq!((u.bank_time, u.bank_time_out, u.time_today), (5, 20, 45));
        assert_eq!(withdraw(&mut u, &l, Unit::Minutes, 1).unwrap_err().string, "s309", "20 a day out");
        u.bank_time_out = 0;
        assert_eq!(withdraw(&mut u, &l, Unit::Minutes, 6).unwrap_err().string, "s310", "only 5 in the bank");
    }

    #[test]
    fn bank_has_a_ceiling() {
        let l = level();
        let mut u = User { bank_time: 50, time_today: 100, ..User::default() };
        assert_eq!(deposit(&mut u, &l, Unit::Minutes, 15, 100).unwrap_err().string, "s306");
        let closed = Level { bank_time_max: 0, ..level() };
        assert_eq!(deposit(&mut User::default(), &closed, Unit::Minutes, 1, 100).unwrap_err().string, "s306");
    }

    #[test]
    fn kilobytes_move_the_allowance() {
        let l = level();
        let mut u = User { limit_dl_kb: 600, ..User::default() };
        deposit(&mut u, &l, Unit::Kb, 200, 600).unwrap();
        assert_eq!((u.bank_kb, u.dl_kb_allowance()), (200, 400));
        assert_eq!(deposit(&mut u, &l, Unit::Kb, 400, 400).unwrap_err().string, "s315", "500 a day in");
        withdraw(&mut u, &l, Unit::Kb, 200).unwrap();
        assert_eq!((u.bank_kb, u.dl_kb_allowance()), (0, 600));
        assert_eq!(withdraw(&mut u, &l, Unit::Kb, 1).unwrap_err().string, "s317");
    }
}
