//! Node numbers and who's on them. Each session holds one for as long as
//! it's connected, replacing the original's NODE*.DAT files. Nodes can
//! also send each other messages and chat (NODES.PAS's node data file).

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures_util::task::AtomicWaker;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
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
    /// `/me` and `/msg` in chat (2.0).
    ChatAction { user: String, text: String },
    ChatPrivate { from: usize, user: String, text: String },
    /// Disconnect, after showing `text` (a forced event).
    Kick { text: String },
    /// Someone paging the SysOp on this node (`CP`), their answer, a
    /// keystroke in a SysOp chat, and its end (`hangup`: the SysOp said
    /// `/bye`).
    Page { from: usize, user: String, reason: String },
    PageAnswer { from: usize, accept: bool },
    ChatKey { from: usize, key: u8 },
    ChatEnd { from: usize, hangup: bool },
    /// New: someone else (the SysOp, a vote) changed this node's caller's
    /// record, from `original` to `edited`; the session reads it again.
    AccountChanged { original: Box<crate::user::User>, edited: Box<crate::user::User> },
    /// New: a line from the board itself (a post on a board they follow).
    Notice { text: String },
}

impl NodeEvent {
    fn is_chat(&self) -> bool {
        matches!(
            self,
            NodeEvent::Chat { .. } | NodeEvent::ChatJoin { .. } | NodeEvent::ChatPart { .. } | NodeEvent::ChatAction { .. } | NodeEvent::ChatPrivate { .. }
        )
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
    /// A SysOp is logged in here (pages come to this node).
    pub sysop: bool,
    /// Hidden from other callers' lists (`XI`, 2.0); SysOps still see it.
    pub invisible: bool,
    /// New: the message area being read (its key), for the message
    /// center's readers-now count; empty if none.
    pub msg_area: String,
    /// New: the account logged in here (its user id; 0 before login),
    /// claimed by [`NodeGuard::claim`] so it's on one node at a time.
    pub account: u32,
    /// New: an account taken over from this node by a login elsewhere
    /// (its id): this session's saves of it are dropped from then on.
    pub lost: u32,
    /// New: this node's line, for cutting it (a takeover).
    pub cut: Cut,
}

/// New: a node's line, cut from elsewhere (an account taken over): its
/// reads and writes fail from then on, so a session blocked writing to a
/// dead link ends too, not just one waiting for a key.
#[derive(Clone, Default)]
pub struct Cut(Arc<CutInner>);

#[derive(Default)]
struct CutInner {
    done: AtomicBool,
    read: AtomicWaker,
    write: AtomicWaker,
    notify: Notify,
}

impl std::fmt::Debug for Cut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cut({})", self.is_cut())
    }
}

impl Cut {
    pub fn cut(&self) {
        self.0.done.store(true, Ordering::SeqCst);
        self.0.read.wake();
        self.0.write.wake();
        self.0.notify.notify_waiters();
    }

    pub fn is_cut(&self) -> bool {
        self.0.done.load(Ordering::SeqCst)
    }

    /// Returns once the line is cut.
    pub async fn wait(&self) {
        loop {
            let n = self.0.notify.notified();
            let mut n = std::pin::pin!(n);
            n.as_mut().enable();
            if self.is_cut() {
                return;
            }
            n.await;
        }
    }

    fn check(&self, waker: &AtomicWaker, cx: &Context<'_>) -> Option<io::Error> {
        if !self.is_cut() {
            waker.register(cx.waker());
        }
        self.is_cut().then(|| io::Error::new(io::ErrorKind::ConnectionReset, "line cut: account logged in elsewhere"))
    }
}

/// A connection that a [`Cut`] can end.
pub struct CutStream<S> {
    inner: S,
    cut: Cut,
}

impl<S> CutStream<S> {
    pub fn new(inner: S, cut: Cut) -> Self {
        Self { inner, cut }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for CutStream<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if let Some(e) = self.cut.check(&self.cut.0.read, cx) {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for CutStream<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if let Some(e) = self.cut.check(&self.cut.0.write, cx) {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(e) = self.cut.check(&self.cut.0.write, cx) {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
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
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let i = slots.iter().position(Option::is_none)?;
        slots[i] = Some(NodeInfo { activity: "Logging in".into(), transport, ..NodeInfo::default() });
        Some(NodeGuard { nodes: self.clone(), index: i })
    }

    /// Deliver to node `to` (1-based). False if nobody's on it, or it's a
    /// chat event and they aren't in chat.
    pub fn send(&self, to: usize, ev: NodeEvent) -> bool {
        let wanted = {
            let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
            match slots.get(to.wrapping_sub(1)) {
                Some(Some(info)) => !ev.is_chat() || info.in_chat,
                _ => false,
            }
        };
        if !wanted {
            return false;
        }
        let mut boxes = self.inboxes.lock().unwrap_or_else(|e| e.into_inner());
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
        let count = self.slots.lock().unwrap_or_else(|e| e.into_inner()).len();
        (1..=count).filter(|&n| n != from && self.send(n, ev.clone())).count()
    }

    /// The node a user is on, by handle.
    pub fn find_user(&self, handle: &str) -> Option<usize> {
        self.online().into_iter().find(|(_, i)| i.user.eq_ignore_ascii_case(handle)).map(|(n, _)| n)
    }

    /// (node number, info) for every busy node.
    /// New: wake every node waiting for a key, to look again at what's on
    /// its screen (a post has landed).
    pub fn wake_all(&self) {
        for w in &self.wakers {
            w.notify_one();
        }
    }

    /// New: the node account `id` is claimed on, if any.
    pub fn account_node(&self, id: u32) -> Option<usize> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots.iter().position(|s| id != 0 && s.as_ref().is_some_and(|n| n.account == id)).map(|i| i + 1)
    }

    /// New: end the call on node `n` if account `id` is still claimed
    /// there: a `Kick` showing `text`, and with `cut` its line cut as
    /// well. False if it isn't there now.
    pub fn end_account(&self, n: usize, id: u32, text: &str, cut: bool) -> bool {
        let line = {
            let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
            match slots.get(n.wrapping_sub(1)) {
                Some(Some(info)) if id != 0 && info.account == id => info.cut.clone(),
                _ => return false,
            }
        };
        if cut {
            line.cut();
        } else {
            self.send(n, NodeEvent::Kick { text: text.into() });
        }
        true
    }

    pub fn online(&self) -> Vec<(usize, NodeInfo)> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
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
        if let Some(info) = self.nodes.slots.lock().unwrap_or_else(|e| e.into_inner())[self.index].as_mut() {
            f(info);
        }
    }

    pub fn nodes(&self) -> &Arc<Nodes> {
        &self.nodes
    }

    /// New: claim account `id` for this node, unless another node has it
    /// (then its number). Checked and taken under one lock, so two logins
    /// at once can't both have it; the node's release (drop) frees it.
    pub fn claim(&self, id: u32) -> Result<(), usize> {
        let mut slots = self.nodes.slots.lock().unwrap_or_else(|e| e.into_inner());
        if id != 0
            && let Some(other) = slots.iter().enumerate().position(|(i, s)| i != self.index && s.as_ref().is_some_and(|n| n.account == id))
        {
            return Err(other + 1);
        }
        if let Some(info) = slots[self.index].as_mut() {
            info.account = id;
        }
        Ok(())
    }

    /// New: claim account `id` for this node even if another node has it
    /// (a takeover whose old session didn't end in time): that node loses
    /// it, and its saves of the account are dropped from then on. Returns
    /// the nodes it was taken from.
    pub fn take_over(&self, id: u32) -> Vec<usize> {
        let mut slots = self.nodes.slots.lock().unwrap_or_else(|e| e.into_inner());
        let mut from = Vec::new();
        for (i, s) in slots.iter_mut().enumerate() {
            if let Some(info) = s.as_mut().filter(|n| i != self.index && id != 0 && n.account == id) {
                info.account = 0;
                info.lost = id;
                from.push(i + 1);
            }
        }
        if let Some(info) = slots[self.index].as_mut() {
            info.account = id;
            if info.lost == id {
                info.lost = 0;
            }
        }
        from
    }

    /// New: has another node taken account `id` over from this one?
    pub fn lost(&self, id: u32) -> bool {
        id != 0 && self.nodes.slots.lock().unwrap_or_else(|e| e.into_inner())[self.index].as_ref().is_some_and(|n| n.lost == id)
    }

    /// New: this node's line, for [`CutStream`].
    pub fn line(&self) -> Cut {
        self.nodes.slots.lock().unwrap_or_else(|e| e.into_inner())[self.index].as_ref().map(|n| n.cut.clone()).unwrap_or_default()
    }

    /// Events waiting for this node, oldest first. With `chat` false,
    /// chat events are left where they are.
    pub fn take(&self, chat: bool) -> Vec<NodeEvent> {
        let mut boxes = self.nodes.inboxes.lock().unwrap_or_else(|e| e.into_inner());
        let q = &mut boxes[self.index];
        // Kicks wait for `take_kick` (multinode chat handles them itself);
        // pages and SysOp chat events wait for `take_page` / `take_if`.
        let (take, keep): (Vec<_>, Vec<_>) = q
            .drain(..)
            .partition(|e| matches!(e, NodeEvent::Message { .. } | NodeEvent::Notice { .. }) || (chat && (e.is_chat() || matches!(e, NodeEvent::Kick { .. }))));
        q.extend(keep);
        take
    }

    /// A `Kick` waiting for this node, taken out of the inbox.
    pub fn take_kick(&self) -> Option<String> {
        let mut boxes = self.nodes.inboxes.lock().unwrap_or_else(|e| e.into_inner());
        let q = &mut boxes[self.index];
        let i = q.iter().position(|e| matches!(e, NodeEvent::Kick { .. }))?;
        match q.remove(i) {
            Some(NodeEvent::Kick { text }) => Some(text),
            _ => None,
        }
    }

    /// The first event `pred` picks, taken out of the inbox.
    pub fn take_if(&self, pred: impl Fn(&NodeEvent) -> bool) -> Option<NodeEvent> {
        let mut boxes = self.nodes.inboxes.lock().unwrap_or_else(|e| e.into_inner());
        let q = &mut boxes[self.index];
        let i = q.iter().position(pred)?;
        q.remove(i)
    }

    /// A page waiting for this node. Chat events from before it (an
    /// earlier chat's leftovers) are dropped.
    pub fn take_page(&self) -> Option<(usize, String, String)> {
        let mut boxes = self.nodes.inboxes.lock().unwrap_or_else(|e| e.into_inner());
        let q = &mut boxes[self.index];
        let i = q.iter().position(|e| matches!(e, NodeEvent::Page { .. }))?;
        let stale: Vec<usize> = (0..i)
            .filter(|&j| matches!(q[j], NodeEvent::ChatKey { .. } | NodeEvent::ChatEnd { .. } | NodeEvent::PageAnswer { .. }))
            .collect();
        for j in stale.into_iter().rev() {
            q.remove(j);
        }
        let i = q.iter().position(|e| matches!(e, NodeEvent::Page { .. }))?;
        match q.remove(i) {
            Some(NodeEvent::Page { from, user, reason }) => Some((from, user, reason)),
            _ => None,
        }
    }

    pub fn has_messages(&self) -> bool {
        self.nodes.inboxes.lock().unwrap_or_else(|e| e.into_inner())[self.index].iter().any(|e| matches!(e, NodeEvent::Message { .. } | NodeEvent::Notice { .. }))
    }

    /// Wakes when something is sent to this node.
    pub fn waker(&self) -> Arc<Notify> {
        self.nodes.wakers[self.index].clone()
    }
}

impl Drop for NodeGuard {
    fn drop(&mut self) {
        self.nodes.slots.lock().unwrap_or_else(|e| e.into_inner())[self.index] = None;
        self.nodes.inboxes.lock().unwrap_or_else(|e| e.into_inner())[self.index].clear();
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
    fn pages_and_chat_keys_wait_for_their_takers() {
        let n = Nodes::new(2);
        let a = n.acquire("ssh").unwrap();
        let _b = n.acquire("ssh").unwrap();
        n.send(1, NodeEvent::ChatEnd { from: 2, hangup: false });
        n.send(1, NodeEvent::Page { from: 2, user: "phrac".into(), reason: "hi".into() });
        n.send(1, NodeEvent::ChatKey { from: 2, key: b'x' });
        assert!(a.take(false).is_empty() && !a.has_messages());
        assert_eq!(a.take_page(), Some((2, "phrac".into(), "hi".into())), "the stale ChatEnd before it went");
        assert_eq!(a.take_if(|e| matches!(e, NodeEvent::ChatEnd { .. })), None);
        assert_eq!(a.take_if(|e| matches!(e, NodeEvent::ChatKey { .. })), Some(NodeEvent::ChatKey { from: 2, key: b'x' }));
        assert_eq!(a.take_page(), None);
    }

    #[test]
    fn an_account_is_on_one_node_at_a_time() {
        let n = Nodes::new(3);
        let a = n.acquire("ssh").unwrap();
        let b = n.acquire("telnet").unwrap();
        assert_eq!(a.claim(7), Ok(()));
        assert_eq!(a.claim(7), Ok(()), "its own node may claim it again");
        assert_eq!(b.claim(7), Err(1), "on node 1 already");
        assert_eq!(b.claim(8), Ok(()), "another account is fine");
        assert_eq!(n.acquire("ssh").unwrap().claim(0), Ok(()), "nobody yet never collides");
        drop(a);
        assert_eq!(b.claim(7), Ok(()), "free once node 1 hangs up");
    }

    #[test]
    fn an_account_can_be_taken_over() {
        let n = Nodes::new(3);
        let old = n.acquire("ssh").unwrap();
        let new = n.acquire("telnet").unwrap();
        old.claim(7).unwrap();
        assert_eq!(n.account_node(7), Some(1));
        assert!(!n.end_account(1, 8, "bye", false), "not that account's node");
        assert!(n.end_account(1, 7, "bye", false));
        assert_eq!(old.take_kick().as_deref(), Some("bye"));
        let line = old.line();
        assert!(!line.is_cut());
        assert!(n.end_account(1, 7, "", true));
        assert!(line.is_cut());
        assert_eq!(new.claim(7), Err(1), "still claimed until it lets go");
        assert_eq!(new.take_over(7), vec![1]);
        assert!(old.lost(7) && !new.lost(7));
        assert_eq!(n.account_node(7), Some(2));
        assert_eq!(old.claim(7), Err(2), "the old node can't take it back");
        assert!(!n.end_account(1, 7, "bye", false), "nothing to end there now");
        drop(old);
        assert_eq!(new.claim(7), Ok(()));
        assert!(!n.acquire("ssh").unwrap().line().is_cut(), "a fresh node has a fresh line");
    }

    #[tokio::test]
    async fn a_cut_line_fails_reads_and_writes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (a, _b) = tokio::io::duplex(16);
        let cut = Cut::default();
        let mut s = CutStream::new(a, cut.clone());
        s.write_all(b"0123456789abcdef").await.unwrap();
        // The pipe is full: this write waits until the cut ends it.
        let c = cut.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            c.cut();
        });
        let e = s.write_all(b"more").await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionReset);
        let mut buf = [0u8; 4];
        assert!(s.read(&mut buf).await.is_err());
        tokio::time::timeout(std::time::Duration::from_secs(1), cut.wait()).await.expect("wait returns once cut");
    }

    #[test]
    fn a_hangup_or_panic_frees_the_account() {
        let n = Nodes::new(2);
        let a = n.acquire("ssh").unwrap();
        a.claim(5).unwrap();
        let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _a = a;
            panic!("session panicked");
        }));
        assert!(held.is_err());
        assert_eq!(n.acquire("ssh").unwrap().claim(5), Ok(()));
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
