//! Node numbers and who's on them. Each session holds one for as long as
//! it's connected, replacing the original's NODE*.DAT files.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct NodeInfo {
    pub user: String,
    pub location: String,
    pub activity: String,
    pub transport: &'static str,
}

pub struct Nodes {
    slots: Mutex<Vec<Option<NodeInfo>>>,
}

impl Nodes {
    pub fn new(count: u16) -> Arc<Self> {
        Arc::new(Self { slots: Mutex::new(vec![None; count as usize]) })
    }

    /// Take the lowest free node, if any.
    pub fn acquire(self: &Arc<Self>, transport: &'static str) -> Option<NodeGuard> {
        let mut slots = self.slots.lock().unwrap();
        let i = slots.iter().position(Option::is_none)?;
        slots[i] = Some(NodeInfo { activity: "Logging in".into(), transport, ..NodeInfo::default() });
        Some(NodeGuard { nodes: self.clone(), index: i })
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
}

impl Drop for NodeGuard {
    fn drop(&mut self) {
        self.nodes.slots.lock().unwrap()[self.index] = None;
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
}
