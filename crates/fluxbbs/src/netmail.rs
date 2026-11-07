//! New: callers' netmail. Email to `name@zone:net/node` (`Nick Boel@46:1/100`)
//! goes out as netmail on the network that zone is on, by way of its hub,
//! and answering netmail goes back to whoever sent it, with REPLY. Before
//! it's written, the address is looked up in the network's nodelist (the
//! newest file in its nodelist area, `nodelist.rs`): one that isn't
//! there, or is down, is refused, as is a network with no nodelist here
//! to check. `[messages] acs_netmail` says who may; bots never.

use std::io;

use crate::config::FtnNetwork;
use crate::ftn::Addr;
use crate::msgbase::{Message, Party};
use crate::nodelist::Entry;
use crate::session::Session;

/// `name@address` (the address with or without `@domain`): the name and
/// address, if it's one.
pub fn parse(s: &str) -> Option<(String, Addr)> {
    let (name, addr) = s.split_once('@')?;
    let name = name.trim();
    let a = Addr::parse(addr.trim())?;
    (!name.is_empty() && a.zone != 0 && a.net != 0).then(|| (name.to_string(), a))
}

/// Why a netmail can't go.
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// The board isn't a node of any network (or FTN is off).
    Off,
    /// No network here has that zone.
    NoNetwork(u16),
    /// The network's nodelist isn't here to check against.
    NoNodelist(String),
    /// Not in the nodelist.
    Unknown(Addr, String),
    /// In it, but down.
    Down(Addr, String),
    /// One of this board's own addresses.
    Ours,
    /// New: no network here has that domain.
    NoDomain(String),
    /// New: on more than one network here (they share its zone), and
    /// nothing said which: `@domain` does.
    Ambiguous(Addr, Vec<String>),
}

/// The domain an address names (`46:1/100@agoranet`), if it names one.
pub fn domain_of(addr: &str) -> Option<String> {
    let (_, d) = addr.trim().split_once('@')?;
    let d = d.trim().to_ascii_lowercase();
    (!d.is_empty()).then_some(d)
}

/// `name@address[@domain]`'s domain.
pub fn parse_domain(s: &str) -> Option<String> {
    domain_of(s.split_once('@')?.1)
}

/// Fidonet's zones (1-6) are one network: a board on it is reached by
/// way of the same hub whichever of them it's in.
fn fidonet_zone(z: u16) -> bool {
    (1..=6).contains(&z)
}

/// Who a netmail party is, by name and address: the address kept in its
/// note (the tosser's sender's origin, or where it was sent), the name
/// what's before it, so an `@` in the name can't move it.
pub fn sender(p: &Party) -> Option<(String, Addr, Option<String>)> {
    match Addr::parse(p.note.trim()) {
        Some(addr) => {
            let name = p.name.strip_suffix(&format!("@{}", p.note.trim())).unwrap_or(&p.name);
            let name = name.rsplit_once('@').filter(|(_, a)| Addr::parse(a).is_some()).map_or(name, |(n, _)| n);
            (!name.trim().is_empty()).then(|| (name.trim().to_string(), addr, domain_of(&p.note)))
        }
        None => parse(&p.name).map(|(n, a)| (n, a, parse_domain(&p.name))),
    }
}

/// The network `addr` is on, from here, and its nodelist entry.
pub fn route(state: &crate::BbsState, addr: Addr, domain: Option<&str>) -> Result<(FtnNetwork, Entry), Refused> {
    let cfg = state.config();
    if !cfg.ftn.enabled {
        return Err(Refused::Off);
    }
    let nets: Vec<(FtnNetwork, Addr)> = cfg.ftn.networks().into_iter().filter_map(|n| Addr::parse(&n.address).map(|me| (n, me))).collect();
    if nets.is_empty() {
        return Err(Refused::Off);
    }
    if nets.iter().any(|(_, me)| *me == addr) {
        return Err(Refused::Ours);
    }
    // The networks it could be on: the one its domain names; else those
    // with its zone (Fidonet's zones are one network).
    let on: Vec<&(FtnNetwork, Addr)> = match domain {
        Some(d) => {
            let on: Vec<_> = nets.iter().filter(|(n, _)| n.domain() == d || n.name.eq_ignore_ascii_case(d)).collect();
            if on.is_empty() {
                return Err(Refused::NoDomain(d.to_string()));
            }
            on
        }
        None => {
            let mut on: Vec<_> = nets.iter().filter(|(_, me)| me.zone == addr.zone).collect();
            if on.is_empty() {
                on = nets.iter().filter(|(_, me)| fidonet_zone(me.zone) && fidonet_zone(addr.zone)).collect();
            }
            on
        }
    };
    if on.is_empty() {
        return Err(Refused::NoNetwork(addr.zone));
    }
    // Each one's word on it, from its nodelist.
    let fa = state.fareas();
    let looked: Vec<(&FtnNetwork, Result<Entry, Refused>)> = on
        .iter()
        .map(|(net, _)| {
            let list = fa
                .areas
                .iter()
                .find(|a| !net.nodelist.trim().is_empty() && a.file.eq_ignore_ascii_case(net.nodelist.trim()))
                .and_then(|a| crate::nodelist::read_dir(&cfg.resolve(&a.path)));
            let r = match list {
                None => Err(Refused::NoNodelist(net.name.clone())),
                Some(list) => match list.find(addr) {
                    None => Err(Refused::Unknown(addr, net.name.clone())),
                    Some(e) if e.down() => Err(Refused::Down(addr, net.name.clone())),
                    Some(e) => Ok(e.clone()),
                },
            };
            (net, r)
        })
        .collect();
    // One network: its answer. More (a shared zone, no domain given): the
    // one that has the node; refused if more than one does.
    let found: Vec<&(&FtnNetwork, Result<Entry, Refused>)> = looked.iter().filter(|(_, r)| r.is_ok()).collect();
    match found.as_slice() {
        [(net, Ok(e))] => Ok(((*net).clone(), e.clone())),
        // None has it: the first's reason (one network: its own).
        [] => Err(looked.into_iter().find_map(|(_, r)| r.err()).unwrap_or(Refused::NoNetwork(addr.zone))),
        many => Err(Refused::Ambiguous(addr, many.iter().map(|(n, _)| n.name.clone()).collect())),
    }
}

impl Session {
    /// May this caller write netmail? Said why not if not.
    async fn netmail_allowed(&mut self) -> io::Result<bool> {
        let acs = self.state.config().messages.acs_netmail.clone();
        if crate::aimemory::is_bot(&self.user) || !self.acs(&acs) {
            self.say_line("NetmailNoAccess").await?;
            return Ok(false);
        }
        Ok(true)
    }

    /// Netmail to `to` (`name@address`): who it goes to, shown and checked,
    /// as the email's recipient; None (having said why) if it can't go.
    pub(crate) async fn netmail_party(&mut self, to: &str) -> io::Result<Option<Party>> {
        let Some((name, addr)) = parse(to) else { return Ok(None) };
        self.netmail_party_at(&name, addr, parse_domain(to).as_deref()).await
    }

    /// [`Self::netmail_party`] to `name` at `addr` (a name with an `@` in
    /// it, from a reply to netmail, is taken as it is).
    pub(crate) async fn netmail_party_at(&mut self, name: &str, addr: Addr, domain: Option<&str>) -> io::Result<Option<Party>> {
        let name = name.to_string();
        if !self.netmail_allowed().await? {
            return Ok(None);
        }
        let s = self.strings();
        match route(&self.state, addr, domain) {
            Ok((net, e)) => {
                let line = s.fmt("NetmailTo", &[&name, &addr.to_string(), &e.name, &e.location, &net.name]);
                self.show_line(&line).await?;
                // Nobody here: "None", as for any party that isn't a
                // caller, so a caller with that handle or real name doesn't
                // get the copy kept in email (`mail_for`, `is_me`).
                // The note names its network too, for sending it by that.
                Ok(Some(Party { user_id: 0, alias: "None".into(), real_name: "None".into(), name: format!("{name}@{addr}"), note: format!("{addr}@{}", net.domain()) }))
            }
            Err(why) => {
                let line = match why {
                    Refused::Off => s.get("NetmailOff").to_string(),
                    Refused::NoNetwork(z) => s.fmt("NetmailNoNetwork", &[&z.to_string()]),
                    Refused::NoNodelist(n) => s.fmt("NetmailNoNodelist", &[&n]),
                    Refused::Unknown(a, n) => s.fmt("NetmailUnknown", &[&a.to_string(), &n]),
                    Refused::Down(a, n) => s.fmt("NetmailDown", &[&a.to_string(), &n]),
                    Refused::Ours => s.get("NetmailOurs").to_string(),
                    Refused::NoDomain(d) => s.fmt("NetmailNoDomain", &[&d]),
                    Refused::Ambiguous(a, nets) => s.fmt("NetmailAmbiguous", &[&a.to_string(), &nets.join(", ")]),
                };
                self.show_line(&line).await?;
                Ok(None)
            }
        }
    }

    /// Write `m` (just saved in email) out as netmail to its recipient,
    /// answering `reply`'s MSGID if it came as netmail. Said if it can't.
    /// True if it went.
    pub(crate) async fn send_netmail(&mut self, m: &Message, reply: Option<&Message>) -> io::Result<bool> {
        // Off the session's thread: writing it waits for a toss or scan
        // that's running (`ftntoss::lock`), which a big one makes long.
        let (state, msg, reply_id, handle) = (self.state.clone(), m.clone(), reply.map(|r| r.id), self.user.handle.clone());
        let sent = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let m = &msg;
            let (name, addr, domain) = sender(&m.to).ok_or_else(|| anyhow::anyhow!("{:?} isn't a netmail address", m.to.name))?;
            let (net, _) = route(&state, addr, domain.as_deref()).map_err(|why| anyhow::anyhow!("{why:?}"))?;
            let answers = match reply_id {
                Some(r) => state.msgs.ftn_msgid(r)?,
                None => None,
            };
            // All of it, signature too, as echomail goes.
            let out = crate::ftntoss::Outgoing { from: &m.from.name, to: &name, to_addr: addr, subject: &m.subject, body: &m.lines, reply: answers.as_deref() };
            let msgid = crate::ftntoss::netmail(&state, &net, &out)?;
            // Its MSGID kept, for a reply to it to find (it's gone either way).
            if let Err(e) = state.msgs.ftn_record(m.id, &msgid, &m.area, true) {
                tracing::error!("netmail {msgid}: recording it: {e:#}");
            }
            tracing::info!("{handle}: netmail to {name} at {addr} ({}): {:?}", net.name, m.subject);
            Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));
        if let Err(e) = &sent {
            tracing::error!("{}: netmail to {} not sent: {e:#}", self.user.handle, m.to.name);
            self.say_line("NetmailFailed").await?;
        }
        Ok(sent.is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two networks with the same zone, each with its nodelist.
    #[test]
    fn a_shared_zone_goes_by_nodelist_or_domain() {
        let state = board(true);
        let dir = state.config().root.clone();
        std::fs::create_dir_all(dir.join("files/other")).unwrap();
        std::fs::write(
            dir.join("files/other/othernet.303"),
            "Zone,46,Othernet,Here,Someone,-Unpublished-,300,CM\r\nHost,1,Net_1,Here,Someone,-Unpublished-,300\r\nHub,100,Other_HUB,Here,Someone,-Unpublished-,300\r\n,200,Only_Here,Here,Someone,-Unpublished-,300\r\n",
        )
        .unwrap();
        let mut cfg = (*state.config()).clone();
        cfg.ftn.networks.push(crate::config::FtnNetwork { name: "othernet".into(), address: "46:2/5@othernet".into(), hub: "46:1/100".into(), nodelist: "OTHNODE".into(), ..Default::default() });
        *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
        let mut fa = (*state.fareas()).clone();
        let mut other = fa.areas.last().unwrap().clone();
        (other.file, other.path) = ("OTHNODE".into(), "files/other".into());
        fa.areas.push(other);
        *state.fareas.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(fa);
        let at = |a: &str, d: Option<&str>| route(&state, Addr::parse(a).unwrap(), d);
        // Only one nodelist has it: that network.
        assert_eq!(at("46:1/200", None).unwrap().0.name, "othernet");
        assert_eq!(at("46:1/115", None).unwrap_err(), Refused::Ours);
        // Both have 46:1/100: which, unsaid, is refused; @domain says.
        assert!(matches!(at("46:1/100", None), Err(Refused::Ambiguous(_, n)) if n.len() == 2));
        assert_eq!(at("46:1/100", Some("othernet")).unwrap().0.name, "othernet");
        assert_eq!(at("46:1/100", Some("agoranet")).unwrap().0.name, "agoranet");
        assert_eq!(parse_domain("Nick@46:1/100@AgoraNet").as_deref(), Some("agoranet"));
    }

    #[test]
    fn senders_with_an_at_in_their_name() {
        let p = |name: &str, note: &str| Party { user_id: 0, alias: "None".into(), real_name: "None".into(), name: name.into(), note: note.into() };
        assert_eq!(sender(&p("D@rk Angel@46:1/999", "46:1/999@agoranet")), Some(("D@rk Angel".into(), Addr::parse("46:1/999").unwrap(), Some("agoranet".into()))));
        assert_eq!(sender(&p("joe@example.com@1:1/31", "1:1/31")).map(|s| (s.0, s.2)), Some(("joe@example.com".into(), None)));
        // A name that looks like another address goes to where it came from.
        assert_eq!(sender(&p("Nick Boel@46:1/100@46:1/999", "46:1/999")).map(|s| s.1), Addr::parse("46:1/999"));
        assert_eq!(sender(&p("Nick Boel@46:1/100@agoranet", "")), Some(("Nick Boel".into(), Addr::parse("46:1/100").unwrap(), Some("agoranet".into()))));
    }

    #[test]
    fn addresses() {
        assert_eq!(parse("Nick Boel@46:1/100"), Some(("Nick Boel".to_string(), Addr::parse("46:1/100").unwrap())));
        assert_eq!(parse("Nick Boel @ 46:1/100@agoranet").map(|p| p.1), Addr::parse("46:1/100"));
        assert_eq!(parse("sysop@21:1/100.2").map(|p| p.1.point), Some(2));
        assert!(parse("phrac").is_none() && parse("@46:1/100").is_none() && parse("someone@example.com").is_none());
        assert!(fidonet_zone(1) && fidonet_zone(2) && !fidonet_zone(46) && !fidonet_zone(21));
    }

    /// A board on AgoraNet (46:1/115) with its nodelist in AGNNODE.
    fn board(with_list: bool) -> std::sync::Arc<crate::BbsState> {
        let dir = std::env::temp_dir().join(format!("inq-netmail-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("files/node")).unwrap();
        if with_list {
            std::fs::write(
                dir.join("files/node/agoranet.303"),
                "Zone,46,Agoranet,Pewaukee_WI,Nicholas_Boel,-Unpublished-,300,CM\r\nHost,1,Net_1,Pewaukee_WI,Nicholas_Boel,-Unpublished-,300\r\nHub,100,The_Pharcyde_HUB,Pewaukee_WI,Nicholas_Boel,-Unpublished-,300\r\n,115,disflux,Nebraska,phrac,-Unpublished-,300\r\nDown,116,Gone,Nowhere,Someone,-Unpublished-,300\r\n",
            )
            .unwrap();
        }
        let state = crate::tests::state();
        let mut cfg = (*state.config()).clone();
        cfg.root = dir.clone();
        cfg.paths.data = "data".into();
        cfg.ftn = crate::config::Ftn { enabled: true, name: "agoranet".into(), address: "46:1/115@agoranet".into(), hub: "46:1/100".into(), nodelist: "AGNNODE".into(), ..Default::default() };
        *state.config.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(cfg);
        let mut fa = (*state.fareas()).clone();
        fa.areas.push(crate::fileareas::FileArea {
            name: "nodelist".into(),
            file: "AGNNODE".into(),
            path: "files/node".into(),
            sponsor: String::new(),
            acs: String::new(),
            upload_acs: String::new(),
            download_acs: String::new(),
            password: String::new(),
            sort: Default::default(),
            sort_descending: false,
            parent: String::new(),
        });
        *state.fareas.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(fa);
        state
    }

    #[test]
    fn routes_by_zone_and_nodelist() {
        let state = board(true);
        let at = |s: &str| route(&state, Addr::parse(s).unwrap(), None);
        let (net, e) = at("46:1/100").unwrap();
        assert_eq!((net.name.as_str(), e.name.as_str(), e.location.as_str()), ("agoranet", "The Pharcyde HUB", "Pewaukee WI"));
        assert_eq!(at("46:1/999").unwrap_err(), Refused::Unknown(Addr::parse("46:1/999").unwrap(), "agoranet".into()));
        assert!(matches!(at("46:1/116"), Err(Refused::Down(..))));
        assert_eq!(at("46:1/115").unwrap_err(), Refused::Ours);
        assert_eq!(at("21:1/100").unwrap_err(), Refused::NoNetwork(21));
        assert_eq!(route(&state, Addr::parse("46:1/100").unwrap(), Some("fsxnet")).unwrap_err(), Refused::NoDomain("fsxnet".into()));
        assert_eq!(route(&state, Addr::parse("46:1/100").unwrap(), Some("agoranet")).unwrap().0.name, "agoranet");
        assert_eq!(route(&board(false), Addr::parse("46:1/100").unwrap(), None).unwrap_err(), Refused::NoNodelist("agoranet".into()));
    }
}

#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::fuzzkit;

    #[test]
    #[ignore]
    fn fuzz_netmail_addresses() {
        let corpus: Vec<Vec<u8>> = ["Nick Boel@46:1/100", "Nick Boel@46:1/100@agoranet", "a@b@21:1/5.3@fsxnet", "x@1:2/3", "21:1/100@fsxnet", "@@@"].iter().map(|s| s.as_bytes().to_vec()).collect();
        fuzzkit::run("netmail_addr", corpus, &[b"@", b":", b"/", b".", b" "], 100_000, |d| {
            let s = String::from_utf8_lossy(d);
            if let Some((name, a)) = parse(&s) {
                assert!(!name.is_empty() && a.zone != 0 && a.net != 0);
            }
            let _ = parse_domain(&s);
            let _ = domain_of(&s);
            // Split anywhere into a name and a note.
            let cut = d.first().map_or(0, |&b| b as usize % (s.len() + 1));
            let cut = (0..=cut).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
            let p = Party { name: s[..cut].to_string(), note: s[cut..].to_string(), ..Party::default() };
            if let Some((name, _, _)) = sender(&p) {
                assert!(!name.trim().is_empty());
            }
            let _ = sender(&Party { name: s.to_string(), ..Party::default() });
        });
    }
}
