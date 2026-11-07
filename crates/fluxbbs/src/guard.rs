//! Keeping bots out: per-address connection limits, failure counting and
//! temporary bans (with longer ones for repeat offenders), plus allow and
//! deny lists. The key-combination challenge itself is in `challenge.rs`.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::Security;

/// Why a connection was turned away.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    Denied,
    Banned(Duration),
    TooMany,
    TooFast,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Denied => write!(f, "on the deny list"),
            Refusal::Banned(left) if permanent(*left) => write!(f, "banned for good"),
            Refusal::Banned(left) => write!(f, "banned for {}m more", left.as_secs().div_ceil(60)),
            Refusal::TooMany => write!(f, "too many connections at once"),
            Refusal::TooFast => write!(f, "connecting too often"),
        }
    }
}

/// How long a ban for good lasts: a century, so it's kept and loaded
/// as any other ban is.
pub const PERMANENT: Duration = Duration::from_secs(100 * 365 * 24 * 3600);

/// Is a ban (or what's left of one) for good?
pub fn permanent(left: Duration) -> bool {
    left > Duration::from_secs(10 * 365 * 24 * 3600)
}

/// How long a failure counts toward a ban (new; before, only a good
/// login forgot them).
const FAILURE_WINDOW: Duration = Duration::from_secs(3600);

/// How long an address's ban history is kept after its last ban ends
/// (new): the table doesn't keep every address ever banned.
const STRIKES_KEPT: Duration = Duration::from_secs(30 * 24 * 3600);

/// New: connections from one address still in the SSH handshake or
/// telnet negotiation (not yet a session), and from everyone.
const PENDING_TOTAL: usize = 256;

/// Per address: twice the sessions it may have (at least 4); none with
/// `max_per_ip` 0 (unlimited), the total cap still holding.
fn pending_limit(cfg: &Security) -> u32 {
    if cfg.max_per_ip == 0 { u32::MAX } else { cfg.max_per_ip.max(2).saturating_mul(2) }
}

#[derive(Default)]
struct PerIp {
    active: u32,
    recent: VecDeque<Instant>,
    /// When each failure was, and the account it was against (none for
    /// the bot check or the system password).
    failures: VecDeque<(Instant, Option<u32>)>,
    /// Connections not yet a session ([`Guard::pending`]).
    pending: u32,
    banned_until: Option<Instant>,
    /// How many times it's been banned: each ban lasts twice the last.
    strikes: u32,
    /// Connections refused during the ban it's under.
    knocks: u32,
}

#[derive(Default)]
pub struct Guard {
    ips: Mutex<HashMap<IpAddr, PerIp>>,
    pending: AtomicUsize,
}

/// Does `ip` match an entry: an address, or a prefix like `10.0.` or
/// `192.168.1.0/24`?
pub fn listed(list: &[String], ip: IpAddr) -> bool {
    list.iter().any(|entry| {
        let entry = entry.trim();
        if let Some((net, bits)) = entry.split_once('/') {
            let (Ok(net), Ok(bits)) = (net.parse::<IpAddr>(), bits.parse::<u32>()) else { return false };
            return match (net, ip) {
                (IpAddr::V4(n), IpAddr::V4(i)) if bits <= 32 => {
                    let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
                    u32::from(n) & mask == u32::from(i) & mask
                }
                (IpAddr::V6(n), IpAddr::V6(i)) if bits <= 128 => {
                    let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
                    u128::from(n) & mask == u128::from(i) & mask
                }
                _ => false,
            };
        }
        let s = ip.to_string();
        s == entry || (entry.ends_with(['.', ':']) && s.starts_with(entry))
    })
}

impl Guard {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Bring back bans kept in the database: (address, time left if
    /// still banned, bans so far).
    pub fn load(&self, bans: impl IntoIterator<Item = (IpAddr, Option<Duration>, u32)>) {
        let now = Instant::now();
        let mut ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        for (ip, left, strikes) in bans {
            let e = ips.entry(ip).or_default();
            e.strikes = e.strikes.max(strikes);
            // An old ban ends now, for `tidy`'s clock.
            e.banned_until = Some(now + left.unwrap_or_default());
        }
    }

    /// How many bans an address has had.
    pub fn strikes(&self, ip: IpAddr) -> u32 {
        self.ips.lock().unwrap_or_else(|e| e.into_inner()).get(&ip).map_or(0, |e| e.strikes)
    }

    /// Addresses banned right now, with the time left.
    pub fn bans(&self) -> Vec<(IpAddr, Duration)> {
        let now = Instant::now();
        let mut out: Vec<_> = self
            .ips
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|(ip, e)| e.banned_until.filter(|t| *t > now).map(|t| (*ip, t - now)))
            .collect();
        out.sort_by_key(|(_, left)| *left);
        out
    }

    /// The SysOp's pardon: the ban, the failures and the history go.
    pub fn unban(&self, ip: IpAddr) -> bool {
        let mut ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        match ips.get_mut(&ip) {
            Some(e) if e.banned_until.is_some_and(|t| t > Instant::now()) || e.strikes > 0 => {
                e.banned_until = None;
                e.failures.clear();
                e.strikes = 0;
                true
            }
            _ => false,
        }
    }

    /// Is this address on the allow list (no limits, no challenge)?
    pub fn trusted(&self, cfg: &Security, ip: IpAddr) -> bool {
        listed(&cfg.allow, ip)
    }

    /// Only the ban and deny checks: for turning a connection away before
    /// doing any work for it.
    pub fn blocked(&self, cfg: &Security, ip: IpAddr) -> Option<Refusal> {
        if self.trusted(cfg, ip) {
            return None;
        }
        if listed(&cfg.deny, ip) {
            return Some(Refusal::Denied);
        }
        let now = Instant::now();
        let ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        let until = ips.get(&ip)?.banned_until?;
        (until > now).then(|| Refusal::Banned(until - now))
    }

    /// [`Self::blocked`], for a connection being turned away: one that
    /// keeps calling back while banned is a bot, and its `permanent_knocks`th
    /// try makes the ban for good (new). True with it when this one did.
    pub fn knock(&self, cfg: &Security, ip: IpAddr) -> Option<(Refusal, bool)> {
        let why = self.blocked(cfg, ip)?;
        let Refusal::Banned(left) = why else { return Some((why, false)) };
        if permanent(left) || cfg.permanent_knocks == 0 {
            return Some((why, false));
        }
        let mut ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        let Some(e) = ips.get_mut(&ip) else { return Some((why, false)) };
        e.knocks += 1;
        if e.knocks < cfg.permanent_knocks {
            return Some((why, false));
        }
        e.banned_until = Some(Instant::now() + PERMANENT);
        Some((Refusal::Banned(PERMANENT), true))
    }

    /// Let a connection in, or say why not. The returned ticket counts it
    /// as active until dropped.
    pub fn admit(self: &Arc<Self>, cfg: &Security, ip: IpAddr) -> Result<Ticket, Refusal> {
        if let Some(r) = self.blocked(cfg, ip) {
            return Err(r);
        }
        let trusted = self.trusted(cfg, ip);
        let now = Instant::now();
        let mut ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        let e = ips.entry(ip).or_default();
        if !trusted {
            while e.recent.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
                e.recent.pop_front();
            }
            if cfg.max_per_ip > 0 && e.active >= cfg.max_per_ip {
                return Err(Refusal::TooMany);
            }
            if cfg.max_per_minute > 0 && e.recent.len() as u32 >= cfg.max_per_minute {
                return Err(Refusal::TooFast);
            }
            e.recent.push_back(now);
        }
        e.active += 1;
        Ok(Ticket { guard: self.clone(), ip })
    }

    /// New: hold a place for a connection that isn't a session yet (the
    /// SSH handshake and auth, telnet's negotiation), so a flood of them
    /// can't use up the board's sockets: a few per address, and a few
    /// hundred in all. None: turn it away.
    pub fn pending(self: &Arc<Self>, cfg: &Security, ip: IpAddr) -> Option<Pending> {
        if self.pending.fetch_add(1, Ordering::SeqCst) >= PENDING_TOTAL && !self.trusted(cfg, ip) {
            self.pending.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        let mut ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        let e = ips.entry(ip).or_default();
        if e.pending >= pending_limit(cfg) && !self.trusted(cfg, ip) {
            self.pending.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        e.pending += 1;
        Some(Pending { guard: self.clone(), ip })
    }

    /// Count a failure (challenge or password) not against an account.
    /// Returns the ban length if this one earned a ban.
    pub fn failure(&self, cfg: &Security, ip: IpAddr) -> Option<Duration> {
        self.failure_for(cfg, ip, None)
    }

    /// [`Self::failure`], against `account` (a password or identity
    /// question for it): only that account's own good login forgets it.
    pub fn failure_for(&self, cfg: &Security, ip: IpAddr, account: Option<u32>) -> Option<Duration> {
        if self.trusted(cfg, ip) || cfg.fail_limit == 0 {
            return None;
        }
        let now = Instant::now();
        let mut ips = self.ips.lock().unwrap_or_else(|e| e.into_inner());
        let e = ips.entry(ip).or_default();
        while e.failures.front().is_some_and(|(t, _)| now.duration_since(*t) > FAILURE_WINDOW) {
            e.failures.pop_front();
        }
        e.failures.push_back((now, account));
        if (e.failures.len() as u32) < cfg.fail_limit {
            return None;
        }
        e.failures.clear();
        let minutes = (cfg.ban_minutes.max(1) as u64) << e.strikes.min(7);
        // New: the `permanent_after`th ban is for good.
        let len = if cfg.permanent_after > 0 && e.strikes + 1 >= cfg.permanent_after {
            PERMANENT
        } else {
            Duration::from_secs(minutes.min(24 * 60) * 60)
        };
        e.strikes += 1;
        e.knocks = 0;
        e.banned_until = Some(Instant::now() + len);
        Some(len)
    }

    /// A good login to `account` forgets the failures against it and
    /// those against no account (bot checks a flaky link hung up on, a
    /// mistyped system password), not the ban history. Guesses at other
    /// accounts still count: logging in to one's own account between
    /// guesses mustn't keep an address from being banned.
    pub fn success(&self, ip: IpAddr, account: u32) {
        if let Some(e) = self.ips.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&ip) {
            e.failures.retain(|(_, a)| a.is_some_and(|a| a != account));
        }
    }

    /// Forget addresses that are idle and not banned, so the table
    /// doesn't grow forever. Ban history goes a month after the last ban
    /// ended (it was kept for good).
    pub fn tidy(&self) {
        let now = Instant::now();
        self.ips.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, e| {
            e.active > 0
                || e.pending > 0
                || e.banned_until.is_some_and(|t| t > now)
                || e.recent.back().is_some_and(|t| now.duration_since(*t) < Duration::from_secs(3600))
                || e.failures.back().is_some_and(|(t, _)| now.duration_since(*t) < FAILURE_WINDOW)
                || (e.strikes > 0 && e.banned_until.is_some_and(|t| t + STRIKES_KEPT > now))
        });
    }
}

/// One admitted connection; frees its slot when dropped.
pub struct Ticket {
    guard: Arc<Guard>,
    ip: IpAddr,
}

impl Ticket {
    pub fn ip(&self) -> IpAddr {
        self.ip
    }
}

/// A connection not yet a session ([`Guard::pending`]); its place goes
/// when dropped.
pub struct Pending {
    guard: Arc<Guard>,
    ip: IpAddr,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.guard.pending.fetch_sub(1, Ordering::SeqCst);
        if let Some(e) = self.guard.ips.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&self.ip) {
            e.pending = e.pending.saturating_sub(1);
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Some(e) = self.guard.ips.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&self.ip) {
            e.active = e.active.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bans_load_list_and_lift() {
        let g = Guard::new();
        let ip: IpAddr = "10.1.1.1".parse().unwrap();
        g.load([(ip, Some(Duration::from_secs(600)), 2)]);
        assert_eq!(g.strikes(ip), 2);
        assert_eq!(g.bans().len(), 1);
        let cfg = Security { allow: Vec::new(), ..Security::default() };
        assert!(matches!(g.blocked(&cfg, ip), Some(Refusal::Banned(_))));
        assert!(g.unban(ip));
        assert!(g.blocked(&cfg, ip).is_none() && g.bans().is_empty() && g.strikes(ip) == 0);
        assert!(!g.unban(ip), "nothing left to lift");
    }

    fn cfg() -> Security {
        Security { max_per_ip: 2, max_per_minute: 3, fail_limit: 2, ban_minutes: 15, ..Security::default() }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn lists() {
        let l = ["10.0.0.5".to_string(), "192.168.1.0/24".into(), "172.16.".into(), "2001:db8::/32".into()];
        assert!(listed(&l, ip("10.0.0.5")));
        assert!(!listed(&l, ip("10.0.0.50")));
        assert!(listed(&l, ip("192.168.1.200")));
        assert!(!listed(&l, ip("192.168.2.1")));
        assert!(listed(&l, ip("172.16.9.9")));
        assert!(listed(&l, ip("2001:db8::1")));
    }

    #[test]
    fn concurrent_and_rate_limits() {
        let g = Guard::new();
        let c = cfg();
        let a = g.admit(&c, ip("1.2.3.4")).unwrap();
        let _b = g.admit(&c, ip("1.2.3.4")).unwrap();
        assert_eq!(g.admit(&c, ip("1.2.3.4")).err(), Some(Refusal::TooMany));
        drop(a);
        // A third in the minute is fine; a fourth is too fast.
        let _c = g.admit(&c, ip("1.2.3.4")).unwrap();
        drop(_c);
        assert_eq!(g.admit(&c, ip("1.2.3.4")).err(), Some(Refusal::TooFast));
        assert!(g.admit(&c, ip("5.6.7.8")).is_ok(), "other addresses aren't affected");
    }

    #[test]
    fn failures_ban_and_bans_grow() {
        let g = Guard::new();
        let c = cfg();
        let x = ip("9.9.9.9");
        assert_eq!(g.failure(&c, x), None);
        assert_eq!(g.failure(&c, x), Some(Duration::from_secs(15 * 60)));
        assert!(matches!(g.blocked(&c, x), Some(Refusal::Banned(_))));
        g.ips.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&x).unwrap().banned_until = None;
        g.failure(&c, x);
        assert_eq!(g.failure(&c, x), Some(Duration::from_secs(30 * 60)), "second ban is twice as long");
    }

    #[test]
    fn calling_back_while_banned_is_for_good() {
        let g = Guard::new();
        let c = Security { permanent_knocks: 3, ..cfg() };
        let x = ip("9.9.9.7");
        assert!(g.knock(&c, x).is_none(), "not banned: let in");
        g.failure(&c, x);
        g.failure(&c, x);
        assert_eq!(g.knock(&c, x).map(|(_, now)| now), Some(false));
        assert_eq!(g.knock(&c, x).map(|(_, now)| now), Some(false));
        let (why, now) = g.knock(&c, x).unwrap();
        assert!(now && why.to_string() == "banned for good");
        assert_eq!(g.knock(&c, x).map(|(_, now)| now), Some(false), "said once");
        let off = Security { permanent_knocks: 0, ..cfg() };
        let y = ip("9.9.9.6");
        g.failure(&off, y);
        g.failure(&off, y);
        assert!((0..20).all(|_| g.knock(&off, y).is_some_and(|(r, now)| !now && r.to_string().ends_with("m more"))));
    }

    #[test]
    fn the_fifth_ban_is_for_good() {
        let g = Guard::new();
        let c = Security { permanent_after: 5, ..cfg() };
        let x = ip("9.9.9.8");
        let mut lens = Vec::new();
        for _ in 0..5 {
            g.failure(&c, x);
            lens.push(g.failure(&c, x).unwrap());
            g.ips.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&x).unwrap().banned_until = None;
        }
        assert_eq!(lens[3], Duration::from_secs(120 * 60));
        assert!(lens[..4].iter().all(|l| !permanent(*l)) && lens[4] == PERMANENT);
        assert_eq!(Refusal::Banned(PERMANENT).to_string(), "banned for good");
        // Off: they go on doubling.
        let off = Security { permanent_after: 0, ..cfg() };
        g.failure(&off, x);
        assert_eq!(g.failure(&off, x), Some(Duration::from_secs(480 * 60)));
        // Loaded back after a restart, it still is.
        let h = Guard::new();
        h.load([(x, Some(PERMANENT - Duration::from_secs(5)), 5)]);
        assert_eq!(h.blocked(&c, x).map(|r| r.to_string()).as_deref(), Some("banned for good"));
    }

    #[test]
    fn a_good_login_forgets_only_its_own_failures() {
        let g = Guard::new();
        let c = Security { fail_limit: 3, ..cfg() };
        let x = ip("9.9.9.5");
        // Guesses at account 2, with logins to the attacker's own (7)
        // between: still banned on the third.
        assert_eq!(g.failure_for(&c, x, Some(2)), None);
        g.success(x, 7);
        assert_eq!(g.failure_for(&c, x, Some(2)), None);
        g.success(x, 7);
        assert!(g.failure_for(&c, x, Some(2)).is_some(), "banned");
        // A caller who mistypes their own password, then gets it right.
        let y = ip("9.9.9.4");
        g.failure_for(&c, y, Some(3));
        g.failure_for(&c, y, Some(3));
        g.success(y, 3);
        assert_eq!(g.failure_for(&c, y, Some(3)), None, "their own failures were forgotten");
        assert_eq!(g.failure(&c, y), None);
        assert!(g.failure(&c, y).is_some(), "a bot check's count until a good login");
        // A flaky link: bot checks hung up on, a mistyped system password,
        // then in; a good login forgives those, not guesses at others.
        let z = ip("9.9.9.3");
        g.failure(&c, z);
        g.failure_for(&c, z, Some(2));
        g.success(z, 3);
        assert_eq!(g.failure(&c, z), None);
        assert!(g.failure(&c, z).is_some(), "the guess at account 2 still counted");
    }

    #[test]
    fn pending_connections_are_limited_per_address() {
        let g = Guard::new();
        let c = cfg();
        let x = ip("9.9.9.3");
        let held: Vec<_> = (0..pending_limit(&c)).map(|_| g.pending(&c, x).unwrap()).collect();
        assert!(g.pending(&c, x).is_none(), "too many handshakes at once");
        assert!(g.pending(&c, ip("9.9.9.2")).is_some(), "other addresses aren't affected");
        drop(held);
        assert!(g.pending(&c, x).is_some(), "freed when they end");
        assert_eq!(g.pending.load(Ordering::SeqCst), 0);
        // No limit per address: only the total's.
        let open = Security { max_per_ip: 0, ..cfg() };
        let held: Vec<_> = (0..PENDING_TOTAL).map(|_| g.pending(&open, x).unwrap()).collect();
        assert!(g.pending(&open, ip("9.9.9.2")).is_none(), "the total still holds");
        drop(held);
    }

    #[test]
    fn tidy_forgets_old_ban_history() {
        let g = Guard::new();
        let (old, recent) = (ip("9.9.9.1"), ip("9.9.8.1"));
        g.load([(old, None, 2), (recent, None, 1)]);
        g.ips.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&old).unwrap().banned_until = Instant::now().checked_sub(STRIKES_KEPT + Duration::from_secs(1));
        g.tidy();
        assert_eq!((g.strikes(old), g.strikes(recent)), (0, 1));
    }

    #[test]
    fn allow_list_skips_everything() {
        let g = Guard::new();
        let c = Security { allow: vec!["127.0.0.1".into()], ..cfg() };
        let me = ip("127.0.0.1");
        let _t: Vec<_> = (0..5).map(|_| g.admit(&c, me).unwrap()).collect();
        assert_eq!(g.failure(&c, me), None);
        let d = Security { deny: vec!["6.6.6.6".into()], ..cfg() };
        assert_eq!(g.admit(&d, ip("6.6.6.6")).err(), Some(Refusal::Denied));
    }
}
