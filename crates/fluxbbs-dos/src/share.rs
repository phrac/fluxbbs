//! SHARE: what open files let each other do, and byte-range locks
//! (`INT 21h` 5Ch), across every machine in the process, as SHARE.EXE
//! kept them across a network's or a multitasker's sessions. A board's
//! nodes each run a door in a machine of their own, so a multi-node door
//! sees the others' opens and locks here. Compatibility-mode opens (no
//! deny bits) deny nothing, as under DOSBox: most single-node doors open
//! that way and expect to share.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const ERR_SHARING: u16 = 0x20;
pub const ERR_LOCK: u16 = 0x21;

#[derive(Default)]
struct Table {
    next: u64,
    opens: HashMap<PathBuf, Vec<Open>>,
    locks: HashMap<PathBuf, Vec<Lock>>,
}

struct Open {
    id: u64,
    read: bool,
    write: bool,
    deny: u8,
}

struct Lock {
    /// The process that holds it: (machine, PSP).
    owner: (u64, u16),
    start: u64,
    end: u64,
}

static TABLE: Mutex<Option<Table>> = Mutex::new(None);

fn table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    let mut t = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    f(t.get_or_insert_with(Table::default))
}

/// Does deny mode `deny` (1 all, 2 write, 3 read; others none) shut out
/// an open that reads or writes?
fn denies(deny: u8, read: bool, write: bool) -> bool {
    match deny {
        1 => read || write,
        2 => write,
        3 => read,
        _ => false,
    }
}

/// One open of a file (a DOS system file table entry): shared by
/// duplicated handles, and gone, with its place in the table, when the
/// last of them closes.
pub struct Shared {
    id: u64,
    path: PathBuf,
}

/// The key a file goes by: its real path, so two drives or names for one
/// file meet.
fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl Shared {
    /// Open `path` with DOS access code `access` (0 read, 1 write, 2
    /// both, 4 read) and sharing mode `share` (bits 4-6 of the open
    /// mode), or fail with a sharing violation.
    pub fn open(path: &Path, access: u8, share: u8) -> Result<Arc<Shared>, u16> {
        let path = key(path);
        let (read, write) = (access != 1, access == 1 || access == 2);
        table(|t| {
            let opens = t.opens.entry(path.clone()).or_default();
            if opens.iter().any(|o| denies(o.deny, read, write) || denies(share, o.read, o.write)) {
                return Err(ERR_SHARING);
            }
            t.next += 1;
            let id = t.next;
            opens.push(Open { id, read, write, deny: share });
            Ok(Arc::new(Shared { id, path }))
        })
    }

    /// `5Ch` AL=0: lock `len` bytes from `start` for `owner`. Locks may
    /// not overlap, whoever holds them.
    pub fn lock(&self, owner: (u64, u16), start: u64, len: u64) -> Result<(), u16> {
        let end = start.saturating_add(len);
        table(|t| {
            let locks = t.locks.entry(self.path.clone()).or_default();
            if len == 0 || locks.iter().any(|l| l.start < end && start < l.end) {
                return Err(ERR_LOCK);
            }
            locks.push(Lock { owner, start, end });
            Ok(())
        })
    }

    /// `5Ch` AL=1: undo exactly one of `owner`'s locks.
    pub fn unlock(&self, owner: (u64, u16), start: u64, len: u64) -> Result<(), u16> {
        let end = start.saturating_add(len);
        table(|t| {
            let locks = t.locks.entry(self.path.clone()).or_default();
            let at = locks.iter().position(|l| l.owner == owner && l.start == start && l.end == end).ok_or(ERR_LOCK)?;
            locks.remove(at);
            Ok(())
        })
    }

    /// May `owner` read or write `len` bytes from `start` (none of them
    /// locked by another process)?
    pub fn check(&self, owner: (u64, u16), start: u64, len: u64) -> Result<(), u16> {
        let end = start.saturating_add(len.max(1));
        table(|t| match t.locks.get(&self.path) {
            Some(locks) if locks.iter().any(|l| l.owner != owner && l.start < end && start < l.end) => Err(ERR_LOCK),
            _ => Ok(()),
        })
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        table(|t| {
            if let Some(opens) = t.opens.get_mut(&self.path) {
                opens.retain(|o| o.id != self.id);
                if opens.is_empty() {
                    t.opens.remove(&self.path);
                }
            }
        });
    }
}

/// A process ended (or its machine went): its locks go.
pub fn release(owner: (u64, u16)) {
    table(|t| {
        for locks in t.locks.values_mut() {
            locks.retain(|l| l.owner != owner);
        }
        t.locks.retain(|_, l| !l.is_empty());
    });
}

/// A machine went: every lock its processes held goes too.
pub fn release_machine(machine: u64) {
    table(|t| {
        for locks in t.locks.values_mut() {
            locks.retain(|l| l.owner.0 != machine);
        }
        t.locks.retain(|_, l| !l.is_empty());
    });
}

/// A number for each machine, to tell their processes apart.
pub fn machine_id() -> u64 {
    table(|t| {
        t.next += 1;
        t.next
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("inq-share-{}-{name}", std::process::id()));
        std::fs::write(&p, b"0123456789").unwrap();
        p
    }

    #[test]
    fn deny_modes_shut_out_what_they_say() {
        let p = file("deny");
        let a = Shared::open(&p, 2, 2).unwrap(); // read/write, deny write
        assert!(Shared::open(&p, 0, 4).is_ok()); // read, deny none: fine
        assert_eq!(Shared::open(&p, 1, 4).err(), Some(ERR_SHARING)); // write: denied
        assert_eq!(Shared::open(&p, 0, 3).err(), Some(ERR_SHARING)); // would deny a's reading
        drop(a);
        let b = Shared::open(&p, 1, 0).unwrap(); // compatibility: denies nothing
        let c = Shared::open(&p, 2, 0).unwrap();
        assert_eq!(Shared::open(&p, 0, 1).err(), Some(ERR_SHARING)); // deny all, with others open
        drop((b, c));
        let d = Shared::open(&p, 0, 1).unwrap();
        assert_eq!(Shared::open(&p, 0, 4).err(), Some(ERR_SHARING));
        drop(d);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn locks_keep_other_processes_out() {
        let p = file("lock");
        let (me, them) = ((machine_id(), 0x100), (machine_id(), 0x100));
        let a = Shared::open(&p, 2, 4).unwrap();
        let b = Shared::open(&p, 2, 4).unwrap();
        a.lock(me, 2, 4).unwrap(); // bytes 2-5
        assert_eq!(b.lock(them, 5, 2), Err(ERR_LOCK));
        assert_eq!(a.lock(me, 4, 1), Err(ERR_LOCK)); // no overlaps, even its own
        b.lock(them, 6, 2).unwrap();
        assert_eq!(b.check(them, 0, 3), Err(ERR_LOCK));
        assert!(a.check(me, 0, 6).is_ok());
        assert!(b.check(them, 0, 2).is_ok());
        assert_eq!(b.unlock(them, 2, 4), Err(ERR_LOCK)); // not theirs
        assert_eq!(a.unlock(me, 2, 3), Err(ERR_LOCK)); // not the same range
        a.unlock(me, 2, 4).unwrap();
        assert!(b.check(them, 0, 6).is_ok());
        release(them);
        assert!(a.check(me, 6, 2).is_ok());
        std::fs::remove_file(p).unwrap();
    }
}
