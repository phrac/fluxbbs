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
}

/// Fidonet's zones (1-6) are one network: a board on it is reached by
/// way of the same hub whichever of them it's in.
fn fidonet_zone(z: u16) -> bool {
    (1..=6).contains(&z)
}

/// The network `addr` is on, from here, and its nodelist entry.
pub fn route(state: &crate::BbsState, addr: Addr) -> Result<(FtnNetwork, Entry), Refused> {
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
    let Some((net, _)) = nets.iter().find(|(_, me)| me.zone == addr.zone).or_else(|| nets.iter().find(|(_, me)| fidonet_zone(me.zone) && fidonet_zone(addr.zone))) else {
        return Err(Refused::NoNetwork(addr.zone));
    };
    let fa = state.fareas();
    let list = fa
        .areas
        .iter()
        .find(|a| !net.nodelist.trim().is_empty() && a.file.eq_ignore_ascii_case(net.nodelist.trim()))
        .and_then(|a| crate::nodelist::read_dir(&cfg.resolve(&a.path)));
    let Some(list) = list else { return Err(Refused::NoNodelist(net.name.clone())) };
    match list.find(addr) {
        None => Err(Refused::Unknown(addr, net.name.clone())),
        Some(e) if e.down() => Err(Refused::Down(addr, net.name.clone())),
        Some(e) => Ok((net.clone(), e.clone())),
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
        if !self.netmail_allowed().await? {
            return Ok(None);
        }
        let s = self.strings();
        match route(&self.state, addr) {
            Ok((net, e)) => {
                let line = s.fmt("NetmailTo", &[&name, &addr.to_string(), &e.name, &e.location, &net.name]);
                self.show_line(&line).await?;
                Ok(Some(Party { user_id: 0, alias: name.clone(), real_name: name.clone(), name: format!("{name}@{addr}"), note: addr.to_string() }))
            }
            Err(why) => {
                let line = match why {
                    Refused::Off => s.get("NetmailOff").to_string(),
                    Refused::NoNetwork(z) => s.fmt("NetmailNoNetwork", &[&z.to_string()]),
                    Refused::NoNodelist(n) => s.fmt("NetmailNoNodelist", &[&n]),
                    Refused::Unknown(a, n) => s.fmt("NetmailUnknown", &[&a.to_string(), &n]),
                    Refused::Down(a, n) => s.fmt("NetmailDown", &[&a.to_string(), &n]),
                    Refused::Ours => s.get("NetmailOurs").to_string(),
                };
                self.show_line(&line).await?;
                Ok(None)
            }
        }
    }

    /// Write `m` (just saved in email) out as netmail to its recipient,
    /// answering `reply`'s MSGID if it came as netmail. Said if it can't.
    pub(crate) async fn send_netmail(&mut self, m: &Message, reply: Option<&Message>) -> io::Result<()> {
        let sent = (|| -> anyhow::Result<()> {
            let (name, addr) = parse(&m.to.name).ok_or_else(|| anyhow::anyhow!("{:?} isn't a netmail address", m.to.name))?;
            let (net, _) = route(&self.state, addr).map_err(|why| anyhow::anyhow!("{why:?}"))?;
            let answers = match reply {
                Some(r) => self.state.msgs.ftn_msgid(r.id)?,
                None => None,
            };
            // All of it, signature too, as echomail goes.
            let out = crate::ftntoss::Outgoing { from: &m.from.name, to: &name, to_addr: addr, subject: &m.subject, body: &m.lines, reply: answers.as_deref() };
            let msgid = crate::ftntoss::netmail(&self.state, &net, &out)?;
            // Its MSGID kept, for a reply to it to find.
            self.state.msgs.ftn_record(m.id, &msgid, &m.area, true)?;
            tracing::info!("{}: netmail to {name} at {addr} ({}): {:?}", self.user.handle, net.name, m.subject);
            Ok(())
        })();
        if let Err(e) = sent {
            tracing::error!("{}: netmail to {} not sent: {e:#}", self.user.handle, m.to.name);
            self.say_line("NetmailFailed").await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let at = |s: &str| route(&state, Addr::parse(s).unwrap());
        let (net, e) = at("46:1/100").unwrap();
        assert_eq!((net.name.as_str(), e.name.as_str(), e.location.as_str()), ("agoranet", "The Pharcyde HUB", "Pewaukee WI"));
        assert_eq!(at("46:1/999").unwrap_err(), Refused::Unknown(Addr::parse("46:1/999").unwrap(), "agoranet".into()));
        assert!(matches!(at("46:1/116"), Err(Refused::Down(..))));
        assert_eq!(at("46:1/115").unwrap_err(), Refused::Ours);
        assert_eq!(at("21:1/100").unwrap_err(), Refused::NoNetwork(21));
        assert_eq!(route(&board(false), Addr::parse("46:1/100").unwrap()).unwrap_err(), Refused::NoNodelist("agoranet".into()));
    }
}
