//! Keeping bots out: per-address connection limits, failure counting and
//! temporary bans (with longer ones for repeat offenders), plus allow and
//! deny lists. The key-combination challenge itself is in `challenge.rs`.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
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
            Refusal::Banned(left) => write!(f, "banned for {}m more", left.as_secs().div_ceil(60)),
            Refusal::TooMany => write!(f, "too many connections at once"),
            Refusal::TooFast => write!(f, "connecting too often"),
        }
    }
}

#[derive(Default)]
struct PerIp {
    active: u32,
    recent: VecDeque<Instant>,
    failures: u32,
    banned_until: Option<Instant>,
    /// How many times it's been banned: each ban lasts twice the last.
    strikes: u32,
}

#[derive(Default)]
pub struct Guard {
    ips: Mutex<HashMap<IpAddr, PerIp>>,
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
        let ips = self.ips.lock().unwrap();
        let until = ips.get(&ip)?.banned_until?;
        (until > now).then(|| Refusal::Banned(until - now))
    }

    /// Let a connection in, or say why not. The returned ticket counts it
    /// as active until dropped.
    pub fn admit(self: &Arc<Self>, cfg: &Security, ip: IpAddr) -> Result<Ticket, Refusal> {
        if let Some(r) = self.blocked(cfg, ip) {
            return Err(r);
        }
        let trusted = self.trusted(cfg, ip);
        let now = Instant::now();
        let mut ips = self.ips.lock().unwrap();
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

    /// Count a failure (challenge or password). Returns the ban length
    /// if this one earned a ban.
    pub fn failure(&self, cfg: &Security, ip: IpAddr) -> Option<Duration> {
        if self.trusted(cfg, ip) || cfg.fail_limit == 0 {
            return None;
        }
        let mut ips = self.ips.lock().unwrap();
        let e = ips.entry(ip).or_default();
        e.failures += 1;
        if e.failures < cfg.fail_limit {
            return None;
        }
        e.failures = 0;
        let minutes = (cfg.ban_minutes.max(1) as u64) << e.strikes.min(7);
        let len = Duration::from_secs(minutes.min(24 * 60) * 60);
        e.strikes += 1;
        e.banned_until = Some(Instant::now() + len);
        Some(len)
    }

    /// A good login clears the failure count (not the ban history).
    pub fn success(&self, ip: IpAddr) {
        if let Some(e) = self.ips.lock().unwrap().get_mut(&ip) {
            e.failures = 0;
        }
    }

    /// Forget addresses that are idle and not banned, so the table
    /// doesn't grow forever.
    pub fn tidy(&self) {
        let now = Instant::now();
        self.ips.lock().unwrap().retain(|_, e| {
            e.active > 0
                || e.banned_until.is_some_and(|t| t > now)
                || e.recent.back().is_some_and(|t| now.duration_since(*t) < Duration::from_secs(3600))
                || e.strikes > 0
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

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Some(e) = self.guard.ips.lock().unwrap().get_mut(&self.ip) {
            e.active = e.active.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        g.ips.lock().unwrap().get_mut(&x).unwrap().banned_until = None;
        g.failure(&c, x);
        assert_eq!(g.failure(&c, x), Some(Duration::from_secs(30 * 60)), "second ban is twice as long");
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
