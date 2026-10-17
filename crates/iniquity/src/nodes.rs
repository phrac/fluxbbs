//! Node numbers and who's on them. Each session holds one for as long as
//! it's connected, replacing the original's NODE*.DAT files. Nodes can
//! also send each other messages and chat (NODES.PAS's node data file).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

/// Something sent to a node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeEvent {
    /// A message to this node (`XM`), or to everyone (`XB`).
    Message { from: usize, user: String, text: String, broadcast: bool },
    /// Multinode chat (`XC`): a line, someone arriving, someone leaving.
    Chat { from: usize, user: String, text: String },
    ChatJoin { user: String },
    ChatPart { user: String },
    /// Disconnect, after showing `text` (a forced event).
    Kick { text: String },
}

impl NodeEvent {
    fn is_chat(&self) -> bool {
        matches!(self, NodeEvent::Chat { .. } | NodeEvent::ChatJoin { .. } | NodeEvent::ChatPart { .. })
    }
}

/// How many events a node keeps before dropping the oldest.
const INBOX_MAX: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct NodeInfo {
    pub user: String,
    pub location: String,
    pub activity: String,
    pub transport: &'static str,
    /// In multinode chat (chat lines only go to these nodes).
    pub in_chat: bool,
}

pub struct Nodes {
    slots: Mutex<Vec<Option<NodeInfo>>>,
    inboxes: Mutex<Vec<VecDeque<NodeEvent>>>,
    wakers: Vec<Arc<Notify>>,
}

impl Nodes {
    pub fn new(count: u16) -> Arc<Self> {
        let n = count as usize;
        Arc::new(Self {
            slots: Mutex::new(vec![None; n]),
            inboxes: Mutex::new(vec![VecDeque::new(); n]),
            wakers: (0..n).map(|_| Arc::new(Notify::new())).collect(),
        })
    }

    /// Take the lowest free node, if any.
    pub fn acquire(self: &Arc<Self>, transport: &'static str) -> Option<NodeGuard> {
        let mut slots = self.slots.lock().unwrap();
        let i = slots.iter().position(Option::is_none)?;
        slots[i] = Some(NodeInfo { activity: "Logging in".into(), transport, ..NodeInfo::default() });
        Some(NodeGuard { nodes: self.clone(), index: i })
    }

    /// Deliver to node `to` (1-based). False if nobody's on it, or it's a
    /// chat event and they aren't in chat.
    pub fn send(&self, to: usize, ev: NodeEvent) -> bool {
        let wanted = {
            let slots = self.slots.lock().unwrap();
            match slots.get(to.wrapping_sub(1)) {
                Some(Some(info)) => !ev.is_chat() || info.in_chat,
                _ => false,
            }
        };
        if !wanted {
            return false;
        }
        let mut boxes = self.inboxes.lock().unwrap();
        let q = &mut boxes[to - 1];
        if q.len() >= INBOX_MAX {
            q.pop_front();
        }
        q.push_back(ev);
        drop(boxes);
        self.wakers[to - 1].notify_one();
        true
    }

    /// Deliver to every node but `from`. Returns how many got it.
    pub fn send_all(&self, from: usize, ev: NodeEvent) -> usize {
        let count = self.slots.lock().unwrap().len();
        (1..=count).filter(|&n| n != from && self.send(n, ev.clone())).count()
    }

    /// The node a user is on, by handle.
    pub fn find_user(&self, handle: &str) -> Option<usize> {
        self.online().into_iter().find(|(_, i)| i.user.eq_ignore_ascii_case(handle)).map(|(n, _)| n)
    }

    /// (node number, info) for every busy node.
    pub fn online(&self) -> Vec<(usize, NodeInfo)> {
        let slots = self.slots.lock().unwrap();
        slots.iter().enumerate().filter_map(|(i, s)| s.clone().map(|n| (i + 1, n))).collect()
    }
}

/// A held node; released on drop.
pub struct NodeGuard {
    nodes: Arc<Nodes>,
    index: usize,
}

impl NodeGuard {
    /// 1-based, as shown to callers.
    pub fn number(&self) -> usize {
        self.index + 1
    }

    pub fn update(&self, f: impl FnOnce(&mut NodeInfo)) {
        if let Some(info) = self.nodes.slots.lock().unwrap()[self.index].as_mut() {
            f(info);
        }
    }

    pub fn nodes(&self) -> &Arc<Nodes> {
        &self.nodes
    }

    /// Events waiting for this node, oldest first. With `chat` false,
    /// chat events are left where they are.
    pub fn take(&self, chat: bool) -> Vec<NodeEvent> {
        let mut boxes = self.nodes.inboxes.lock().unwrap();
        let q = &mut boxes[self.index];
        // Kicks wait for `take_kick`, except in chat, which handles them.
        let (take, keep): (Vec<_>, Vec<_>) =
            q.drain(..).partition(|e| chat || !(e.is_chat() || matches!(e, NodeEvent::Kick { .. })));
        q.extend(keep);
        take
    }

    /// A `Kick` waiting for this node, taken out of the inbox.
    pub fn take_kick(&self) -> Option<String> {
        let mut boxes = self.nodes.inboxes.lock().unwrap();
        let q = &mut boxes[self.index];
        let i = q.iter().position(|e| matches!(e, NodeEvent::Kick { .. }))?;
        match q.remove(i) {
            Some(NodeEvent::Kick { text }) => Some(text),
            _ => None,
        }
    }

    pub fn has_messages(&self) -> bool {
        self.nodes.inboxes.lock().unwrap()[self.index].iter().any(|e| matches!(e, NodeEvent::Message { .. }))
    }

    /// Wakes when something is sent to this node.
    pub fn waker(&self) -> Arc<Notify> {
        self.nodes.wakers[self.index].clone()
    }
}

impl Drop for NodeGuard {
    fn drop(&mut self) {
        self.nodes.slots.lock().unwrap()[self.index] = None;
        self.nodes.inboxes.lock().unwrap()[self.index].clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowest_free_and_release() {
        let n = Nodes::new(2);
        let a = n.acquire("ssh").unwrap();
        let b = n.acquire("ssh").unwrap();
        assert_eq!((a.number(), b.number()), (1, 2));
        assert!(n.acquire("ssh").is_none());
        a.update(|i| i.user = "Fiend".into());
        assert_eq!(n.online()[0].1.user, "Fiend");
        drop(a);
        assert_eq!(n.acquire("ssh").unwrap().number(), 1);
    }

    #[test]
    fn kicks_reach_everyone_and_wait_for_take_kick() {
        let n = Nodes::new(3);
        let a = n.acquire("ssh").unwrap();
        let b = n.acquire("ssh").unwrap();
        b.update(|i| i.in_chat = true);
        assert_eq!(n.send_all(0, NodeEvent::Kick { text: "bye".into() }), 2, "in chat or not");
        assert!(!a.has_messages(), "not an ordinary message");
        assert!(a.take(false).is_empty(), "left for take_kick");
        assert_eq!(a.take_kick().as_deref(), Some("bye"));
        assert_eq!(a.take_kick(), None);
        assert_eq!(b.take(true), [NodeEvent::Kick { text: "bye".into() }], "chat takes it");
    }

    #[test]
    fn messages_and_chat() {
        let n = Nodes::new(3);
        let a = n.acquire("ssh").unwrap();
        let b = n.acquire("ssh").unwrap();
        let msg = |t: &str| NodeEvent::Message { from: 1, user: "A".into(), text: t.into(), broadcast: false };
        assert!(n.send(2, msg("hi")));
        assert!(!n.send(3, msg("nobody")), "node 3 is free");
        // Chat only reaches nodes in chat.
        let line = NodeEvent::Chat { from: 1, user: "A".into(), text: "yo".into() };
        assert_eq!(n.send_all(1, line.clone()), 0);
        b.update(|i| i.in_chat = true);
        assert_eq!(n.send_all(1, line.clone()), 1);
        assert!(b.has_messages());
        assert_eq!(b.take(false), [msg("hi")], "chat stays for the chat screen");
        assert_eq!(b.take(true), [line]);
        assert!(a.take(true).is_empty());
        b.update(|i| i.user = "Fiend".into());
        assert_eq!(n.find_user("fiend"), Some(2));
    }
}
