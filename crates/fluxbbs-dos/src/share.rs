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

pub const ERR_ACCESS: u16 = 0x05;
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
    /// The open it was made through: closing that lets go of it.
    open: u64,
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
/// file meet; one not made yet goes by its directory's real path, as it
/// will once it's there.
fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| match (path.parent().and_then(|d| std::fs::canonicalize(d).ok()), path.file_name()) {
        (Some(d), Some(n)) => d.join(n),
        _ => path.to_path_buf(),
    })
}

/// Is `path` held open with a deny mode anywhere (so it can't be deleted
/// or renamed)? Compatibility opens deny nothing, here as elsewhere:
/// single-node doors erase and rename files they have open.
pub fn in_use(path: &Path) -> bool {
    let path = key(path);
    table(|t| t.opens.get(&path).is_some_and(|o| o.iter().any(|o| o.deny != 0)))
}

impl Shared {
    /// Open `path` with DOS access code `access` (0 read, 1 write, 2
    /// both, 4 read) and sharing mode `share` (bits 4-6 of the open
    /// mode), or fail: access denied when two deny modes clash, a sharing
    /// violation when compatibility mode is in it (as DOS has it, its
    /// INT 24h failed). `truncate`: the open empties the file (a create),
    /// which no other open with a deny mode allows.
    pub fn open(path: &Path, access: u8, share: u8, truncate: bool) -> Result<Arc<Shared>, u16> {
        let path = key(path);
        let (read, write) = (access != 1, access == 1 || access == 2 || truncate);
        table(|t| {
            let opens = t.opens.entry(path.clone()).or_default();
            // RBIL's table: an open in compatibility mode that clashes goes
            // through INT 24h (here 20h); one with a deny mode is refused
            // (05h), whatever is open already.
            if opens.iter().any(|o| denies(o.deny, read, write) || denies(share, o.read, o.write) || (truncate && o.deny != 0)) {
                return Err(if share == 0 { ERR_SHARING } else { ERR_ACCESS });
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
            locks.push(Lock { owner, open: self.id, start, end });
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
            // Closing a file lets go of the locks made through it.
            if let Some(locks) = t.locks.get_mut(&self.path) {
                locks.retain(|l| l.open != self.id);
                if locks.is_empty() {
                    t.locks.remove(&self.path);
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
        let a = Shared::open(&p, 2, 2, false).unwrap(); // read/write, deny write
        assert!(Shared::open(&p, 0, 4, false).is_ok()); // read, deny none: fine
        assert_eq!(Shared::open(&p, 1, 4, false).err(), Some(ERR_ACCESS)); // write: denied (two deny modes: 05h)
        assert_eq!(Shared::open(&p, 0, 3, false).err(), Some(ERR_ACCESS)); // would deny a's reading
        drop(a);
        let b = Shared::open(&p, 1, 0, false).unwrap(); // compatibility: denies nothing
        let c = Shared::open(&p, 2, 0, false).unwrap();
        assert_eq!(Shared::open(&p, 0, 1, false).err(), Some(ERR_ACCESS)); // deny all, with others open: a deny mode, 05h
        drop((b, c));
        let d = Shared::open(&p, 0, 1, false).unwrap();
        assert_eq!(Shared::open(&p, 0, 4, false).err(), Some(ERR_ACCESS));
        assert_eq!(Shared::open(&p, 0, 0, false).err(), Some(ERR_SHARING), "compatibility mode in it: 20h");
        drop(d);
        // A create can't empty what another node holds with a deny mode,
        // even deny-none; compatibility opens don't stop it.
        let e = Shared::open(&p, 2, 4, false).unwrap();
        assert_eq!(Shared::open(&p, 2, 0, true).err(), Some(ERR_SHARING));
        drop(e);
        let f = Shared::open(&p, 2, 0, false).unwrap();
        assert!(!in_use(&p), "compatibility opens don't stop a delete");
        assert!(Shared::open(&p, 2, 0, true).is_ok());
        let g = Shared::open(&p, 0, 4, false).unwrap();
        assert!(in_use(&p), "a deny-mode open does");
        drop((f, g));
        assert!(!in_use(&p));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn closing_a_file_lets_go_of_its_locks() {
        let p = file("close");
        let me = (machine_id(), 0x100);
        let a = Shared::open(&p, 2, 4, false).unwrap();
        a.lock(me, 0, 4).unwrap();
        let other = (machine_id(), 0x100);
        let b = Shared::open(&p, 2, 4, false).unwrap();
        assert_eq!(b.check(other, 0, 1), Err(ERR_LOCK));
        drop(a);
        assert!(b.check(other, 0, 1).is_ok(), "the lock went with the close");
        // Locking it again (reopened) works: it isn't still there.
        let a2 = Shared::open(&p, 2, 4, false).unwrap();
        a2.lock(me, 0, 4).unwrap();
        drop((a2, b));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn locks_keep_other_processes_out() {
        let p = file("lock");
        let (me, them) = ((machine_id(), 0x100), (machine_id(), 0x100));
        let a = Shared::open(&p, 2, 4, false).unwrap();
        let b = Shared::open(&p, 2, 4, false).unwrap();
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
