//! DOS: `INT 21h`, the program loader, memory blocks and the file
//! system. Drives map onto host directories (`C:\` is the doors
//! directory, `D:\` the node's drop directory); names are matched
//! without regard to case, and what the host holds under other names
//! stays out of sight. Enough of DOS 5 for door programs: no FCBs, no
//! network redirector; SHARE's sharing modes and locks are [`share`]'s.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDateTime, Timelike};

use crate::bios::{clear_cf, set_cf};
use crate::cpu::{self, Cpu};
use crate::machine::System;
use crate::share::{self, Shared};
use crate::video::Screen;
use crate::{After, Memory, linear};

/// Our DOS data segment: the InDOS flag and the "list of lists".
pub const DOS_SEG: u16 = 0x0070;
const INDOS: u32 = (DOS_SEG as u32) << 4 | 0x0001;
const SYSVARS: u16 = 0x0010;
/// The first memory control block; everything above it to 640 KB is
/// for programs.
const FIRST_MCB: u16 = 0x0100;
const MEMORY_TOP: u16 = 0xA000;
const MAX_HANDLES: usize = 40;

const ERR_FILE_NOT_FOUND: u16 = 2;
const ERR_PATH_NOT_FOUND: u16 = 3;
const ERR_TOO_MANY_FILES: u16 = 4;
const ERR_ACCESS_DENIED: u16 = 5;
const ERR_BAD_HANDLE: u16 = 6;
const ERR_NO_MEMORY: u16 = 8;
const ERR_BAD_DRIVE: u16 = 15;
const ERR_NO_MORE_FILES: u16 = 18;

enum Handle {
    /// `share` is the open's place in SHARE's table, which duplicates
    /// share.
    File { f: File, path: PathBuf, owner: u16, share: std::sync::Arc<Shared> },
    Stdin,
    Stdout,
    Aux,
    Nul,
}

struct Entry {
    name: String,
    attr: u8,
    size: u64,
    time: NaiveDateTime,
}

struct Search {
    entries: Vec<Entry>,
    next: usize,
}

struct SavedProcess {
    regs: [u32; 8],
    sregs: [u16; 6],
    ip: u16,
    flags: u32,
    psp: u16,
    dta: (u16, u16),
}

pub struct Dos {
    /// This machine, to SHARE: its processes are (machine, PSP).
    machine: u64,
    roots: Vec<Option<PathBuf>>,
    cwd: Vec<String>,
    cur_drive: usize,
    files: Vec<Option<Handle>>,
    dta: (u16, u16),
    pub psp: u16,
    searches: HashMap<u32, Search>,
    procs: Vec<SavedProcess>,
    strategy: u16,
    last_error: u16,
    verify: bool,
    break_flag: bool,
    /// The last program's exit code (`4Dh`).
    child_code: u8,
}

fn dos_time(t: NaiveDateTime) -> (u16, u16) {
    let time = (t.hour() as u16) << 11 | (t.minute() as u16) << 5 | ((t.second() as u16) / 2);
    let date = ((t.year() - 1980).clamp(0, 127) as u16) << 9 | (t.month() as u16) << 5 | t.day() as u16;
    (time, date)
}

fn host_time(path: &Path) -> NaiveDateTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| crate::local(chrono::DateTime::<chrono::Utc>::from(t).naive_utc()))
        .unwrap_or_else(|_| crate::now())
}

/// Does a host file name fit DOS (8.3, upper-case, legal characters)?
fn dos_name(name: &str) -> Option<String> {
    let up = name.to_ascii_uppercase();
    let (base, ext) = up.rsplit_once('.').map(|(b, e)| (b, Some(e))).unwrap_or((&up, None));
    let ok = |s: &str, max: usize| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'()-@^_`{}~".contains(&b))
    };
    match ext {
        Some(e) => (ok(base, 8) && (e.is_empty() || ok(e, 3))).then_some(up),
        None => ok(base, 8).then_some(up),
    }
}

/// DOS wildcard matching on 8.3 names: `*` runs to the end of the part,
/// `?` any character (or none at the end of a part).
pub fn wild_match(pattern: &str, name: &str) -> bool {
    fn part(p: &str, n: &str) -> bool {
        let (p, n): (Vec<char>, Vec<char>) = (p.chars().collect(), n.chars().collect());
        let mut i = 0;
        for (j, pc) in p.iter().enumerate() {
            match pc {
                '*' => return true,
                '?' => {
                    if i < n.len() {
                        i += 1;
                    }
                }
                c => {
                    if i >= n.len() || n[i] != *c {
                        return false;
                    }
                    i += 1;
                }
            }
            let _ = j;
        }
        i == n.len()
    }
    let (pb, pe) = pattern.split_once('.').unwrap_or((pattern, ""));
    let (nb, ne) = name.split_once('.').unwrap_or((name, ""));
    part(pb, nb) && part(pe, ne)
}

/// `name` padded into an 11-byte FCB field? No: the DTA wants ASCIIZ.
fn read_asciiz(mem: &Memory, seg: u16, off: u16) -> String {
    let mut s = Vec::new();
    for i in 0..256u32 {
        let b = mem.read8(linear(seg, off as u32 + i));
        if b == 0 {
            break;
        }
        s.push(b);
    }
    String::from_utf8_lossy(&s).into_owned()
}

impl Drop for Dos {
    fn drop(&mut self) {
        share::release_machine(self.machine);
    }
}

impl Dos {
    pub fn new(mem: &mut Memory, c_root: &Path, d_root: Option<&Path>) -> Self {
        let mut roots = vec![None; 26];
        roots[2] = Some(c_root.to_path_buf());
        roots[3] = d_root.map(Path::to_path_buf);
        // One free block from the first MCB to the top of memory.
        let size = MEMORY_TOP - FIRST_MCB - 1;
        Self::write_mcb(mem, FIRST_MCB, b'Z', 0, size, b"");
        mem.write16(((DOS_SEG as u32) << 4) + SYSVARS as u32 - 2, FIRST_MCB);
        mem.write8(INDOS, 0);
        let mut files: Vec<Option<Handle>> = (0..MAX_HANDLES).map(|_| None).collect();
        files[0] = Some(Handle::Stdin);
        files[1] = Some(Handle::Stdout);
        files[2] = Some(Handle::Stdout);
        files[3] = Some(Handle::Aux);
        files[4] = Some(Handle::Nul);
        Self {
            machine: share::machine_id(),
            roots,
            cwd: vec![String::new(); 26],
            cur_drive: 2,
            files,
            dta: (0, 0x80),
            psp: 0,
            searches: HashMap::new(),
            procs: Vec::new(),
            strategy: 0,
            last_error: 0,
            verify: false,
            break_flag: false,
            child_code: 0,
        }
    }

    // ---- memory control blocks ---------------------------------------------

    fn write_mcb(mem: &mut Memory, seg: u16, kind: u8, owner: u16, size: u16, name: &[u8]) {
        let a = (seg as u32) << 4;
        mem.write8(a, kind);
        mem.write16(a + 1, owner);
        mem.write16(a + 3, size);
        for i in 0..8u32 {
            mem.write8(a + 8 + i, name.get(i as usize).copied().unwrap_or(0));
        }
    }

    fn mcb(mem: &Memory, seg: u16) -> (u8, u16, u16) {
        let a = (seg as u32) << 4;
        (mem.read8(a), mem.read16(a + 1), mem.read16(a + 3))
    }

    /// Walk the chain: (segment, kind, owner, size).
    fn blocks(mem: &Memory) -> Vec<(u16, u8, u16, u16)> {
        let mut out = Vec::new();
        let mut seg = FIRST_MCB;
        loop {
            let (kind, owner, size) = Self::mcb(mem, seg);
            out.push((seg, kind, owner, size));
            if kind != b'M' || out.len() > 1024 {
                break;
            }
            seg = seg + 1 + size;
        }
        out
    }

    pub fn largest_free(mem: &Memory) -> u16 {
        Self::blocks(mem).iter().filter(|b| b.2 == 0).map(|b| b.3).max().unwrap_or(0)
    }

    /// Allocate `paras` paragraphs for `owner`; the data segment.
    pub fn alloc(&mut self, mem: &mut Memory, paras: u16, owner: u16, name: &[u8]) -> Option<u16> {
        let blocks = Self::blocks(mem);
        let pick = match self.strategy & 0x3F {
            1 => blocks.iter().filter(|b| b.2 == 0 && b.3 >= paras).min_by_key(|b| b.3),
            2 => blocks.iter().rfind(|b| b.2 == 0 && b.3 >= paras),
            _ => blocks.iter().find(|b| b.2 == 0 && b.3 >= paras),
        }?;
        let (seg, kind, _, size) = *pick;
        if size > paras {
            // Split: the remainder stays free after ours.
            let rest = seg + 1 + paras;
            Self::write_mcb(mem, rest, kind, 0, size - paras - 1, b"");
            Self::write_mcb(mem, seg, b'M', owner, paras, name);
        } else {
            Self::write_mcb(mem, seg, kind, owner, size, name);
        }
        Some(seg + 1)
    }

    fn coalesce(mem: &mut Memory) {
        let mut seg = FIRST_MCB;
        loop {
            let (kind, owner, size) = Self::mcb(mem, seg);
            if kind != b'M' {
                break;
            }
            let next = seg + 1 + size;
            let (nkind, nowner, nsize) = Self::mcb(mem, next);
            if owner == 0 && nowner == 0 {
                Self::write_mcb(mem, seg, nkind, 0, size + 1 + nsize, b"");
                continue;
            }
            seg = next;
        }
    }

    pub fn free(&mut self, mem: &mut Memory, data_seg: u16) -> bool {
        let seg = data_seg.wrapping_sub(1);
        if !Self::blocks(mem).iter().any(|b| b.0 == seg) {
            return false;
        }
        let (kind, _, size) = Self::mcb(mem, seg);
        Self::write_mcb(mem, seg, kind, 0, size, b"");
        Self::coalesce(mem);
        true
    }

    /// Resize a block; on failure, the most it could be.
    pub fn resize(&mut self, mem: &mut Memory, data_seg: u16, paras: u16) -> Result<(), u16> {
        let seg = data_seg.wrapping_sub(1);
        let blocks = Self::blocks(mem);
        let Some(i) = blocks.iter().position(|b| b.0 == seg) else { return Err(0) };
        let (_, kind, owner, size) = blocks[i];
        // Room: this block plus any free block right after it.
        let mut avail = size;
        let mut last_kind = kind;
        if kind == b'M' {
            let (nk, no, ns) = Self::mcb(mem, seg + 1 + size);
            if no == 0 {
                avail += 1 + ns;
                last_kind = nk;
            }
        }
        if paras > avail {
            return Err(avail);
        }
        if paras == avail {
            Self::write_mcb(mem, seg, last_kind, owner, paras, b"");
        } else {
            Self::write_mcb(mem, seg, b'M', owner, paras, b"");
            Self::write_mcb(mem, seg + 1 + paras, last_kind, 0, avail - paras - 1, b"");
        }
        Self::coalesce(mem);
        Ok(())
    }

    fn free_owned(&mut self, mem: &mut Memory, owner: u16) {
        for (seg, kind, o, size) in Self::blocks(mem) {
            if o == owner {
                Self::write_mcb(mem, seg, kind, 0, size, b"");
            }
        }
        Self::coalesce(mem);
    }

    // ---- paths -----------------------------------------------------------------

    /// A DOS path resolved on the host: the host path, the drive, and the
    /// normalised DOS directory path. `create`: the last part needn't
    /// exist. A device name gives the device instead.
    fn resolve(&self, path: &str, create: bool) -> Result<Resolved, u16> {
        let up = path.trim().to_ascii_uppercase().replace('/', "\\");
        let base = up.rsplit('\\').next().unwrap_or("");
        let dev = base.split_once('.').map_or(base, |(b, _)| b);
        match dev {
            "CON" => return Ok(Resolved::Device(Handle::Stdin)),
            "AUX" | "COM1" | "COM2" => return Ok(Resolved::Device(Handle::Aux)),
            "NUL" | "PRN" | "LPT1" | "LPT2" => return Ok(Resolved::Device(Handle::Nul)),
            _ => {}
        }
        let (drive, rest) = match up.as_bytes() {
            [d, b':', ..] if d.is_ascii_alphabetic() => ((d - b'A') as usize, &up[2..]),
            _ => (self.cur_drive, up.as_str()),
        };
        let root = self.roots.get(drive).cloned().flatten().ok_or(ERR_BAD_DRIVE)?;
        let mut parts: Vec<String> = if rest.starts_with('\\') { Vec::new() } else { self.cwd[drive].split('\\').filter(|s| !s.is_empty()).map(String::from).collect() };
        for p in rest.split('\\').filter(|s| !s.is_empty()) {
            match p {
                "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(p.to_string()),
            }
        }
        let mut host = root;
        let n = parts.len();
        for (i, p) in parts.iter().enumerate() {
            let last = i + 1 == n;
            match find_ci(&host, p) {
                Some(name) => host.push(name),
                None if last && create => host.push(p),
                None => return Err(if last { ERR_FILE_NOT_FOUND } else { ERR_PATH_NOT_FOUND }),
            }
        }
        Ok(Resolved::Path { host, drive, dos: parts.join("\\") })
    }

    pub fn cwd_dos(&self, drive: usize) -> String {
        self.cwd[drive].clone()
    }

    /// Make `d` (0 = A:) the current drive, if it exists.
    pub fn set_drive(&mut self, d: usize) -> bool {
        if d < 26 && self.roots[d].is_some() {
            self.cur_drive = d;
            true
        } else {
            false
        }
    }

    /// `cd`: change the current directory of the path's drive.
    pub fn change_dir(&mut self, path: &str) -> bool {
        match self.resolve(path, false) {
            Ok(Resolved::Path { host, drive, dos }) if host.is_dir() => {
                self.cwd[drive] = dos;
                true
            }
            _ => false,
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        matches!(self.resolve(path, false), Ok(Resolved::Path { host, .. }) if host.exists())
    }

    /// The whole of a file, by DOS name (for batch files).
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        match self.resolve(path, false) {
            Ok(Resolved::Path { host, .. }) => std::fs::read(host).ok(),
            _ => None,
        }
    }

    /// The last program's exit code.
    pub fn errorlevel(&self) -> u8 {
        self.child_code
    }

    // ---- handles ---------------------------------------------------------------

    fn open_handle(&mut self, h: Handle) -> Result<u16, u16> {
        let slot = self.files.iter().position(Option::is_none).ok_or(ERR_TOO_MANY_FILES)?;
        self.files[slot] = Some(h);
        Ok(slot as u16)
    }

    fn file_mut(&mut self, h: u16) -> Result<&mut Handle, u16> {
        self.files.get_mut(h as usize).and_then(Option::as_mut).ok_or(ERR_BAD_HANDLE)
    }

    /// Where SHARE files this process's locks.
    fn lock_owner(&self) -> (u64, u16) {
        (self.machine, self.psp)
    }

    fn close_owned(&mut self, owner: u16) {
        for slot in self.files.iter_mut().skip(5) {
            if matches!(slot, Some(Handle::File { owner: o, .. }) if *o == owner) {
                *slot = None;
            }
        }
    }

    // ---- programs --------------------------------------------------------------

    /// Find a program by DOS name: as given, then with .COM/.EXE/.BAT, in
    /// the current directory then `C:\`.
    pub fn find_program(&self, name: &str) -> Option<(PathBuf, String)> {
        let name = name.trim();
        let candidates: Vec<String> = if name.contains('.') {
            vec![name.to_string()]
        } else {
            vec![format!("{name}.COM"), format!("{name}.EXE"), format!("{name}.BAT")]
        };
        let dirs: Vec<String> = if name.contains('\\') || name.contains(':') { vec![String::new()] } else { vec![String::new(), "C:\\".into()] };
        for dir in dirs {
            for c in &candidates {
                if let Ok(Resolved::Path { host, drive, dos }) = self.resolve(&format!("{dir}{c}"), false)
                    && host.is_file()
                {
                    return Some((host, format!("{}:\\{dos}", (b'A' + drive as u8) as char)));
                }
            }
        }
        None
    }

    /// Load a program and make it the running process. From `INT 21h/4Bh`
    /// (`from_int21`), the caller's return frame has been popped and its
    /// state is saved first.
    pub fn exec(&mut self, cpu: &mut Cpu, mem: &mut Memory, program: &str, args: &str, env_seg: Option<u16>) -> Result<(), String> {
        let (host, dos_path) = self.find_program(program).ok_or_else(|| format!("{program}: not found"))?;
        if dos_path.to_ascii_uppercase().ends_with(".BAT") {
            return Err(format!("{program}: batch files aren't run from inside a program"));
        }
        let image = std::fs::read(&host).map_err(|e| format!("{}: {e}", host.display()))?;
        let parent = self.psp;
        if parent != 0 {
            self.procs.push(SavedProcess { regs: cpu.regs, sregs: cpu.sregs, ip: cpu.ip, flags: cpu.flags, psp: parent, dta: self.dta });
        }
        // The environment: the parent's, or ours.
        let env_seg = match env_seg {
            Some(s) if s != 0 => s,
            _ => {
                let text = if parent != 0 {
                    let pe = mem.read16(((parent as u32) << 4) + 0x2C);
                    read_env(mem, pe)
                } else {
                    b"PATH=C:\\\0COMSPEC=C:\\COMMAND.COM\0PROMPT=$P$G\0".to_vec()
                };
                let mut block = text.clone();
                block.extend([0, 1, 0]);
                block.extend(dos_path.as_bytes());
                block.push(0);
                let paras = (block.len() as u16).div_ceil(16);
                let seg = self.alloc(mem, paras, 0, b"").ok_or("no memory for the environment")?;
                mem.write((seg as u32) << 4, &block);
                seg
            }
        };
        let is_exe = image.len() > 0x1C && &image[..2] == b"MZ" || &image[..2] == b"ZM";
        let (load_size, min_alloc, max_alloc) = if is_exe {
            let last_page = u16::from_le_bytes([image[2], image[3]]) as u32;
            let pages = u16::from_le_bytes([image[4], image[5]]) as u32;
            let header_paras = u16::from_le_bytes([image[8], image[9]]) as u32;
            let total = if last_page == 0 { pages * 512 } else { (pages - 1) * 512 + last_page };
            let size = total.saturating_sub(header_paras * 16);
            (size, u16::from_le_bytes([image[0xA], image[0xB]]) as u32, u16::from_le_bytes([image[0xC], image[0xD]]) as u32)
        } else {
            (image.len() as u32, 0, 0xFFFF)
        };
        let need = 0x10 + load_size.div_ceil(16) + min_alloc;
        let want = (0x10 + load_size.div_ceil(16) + max_alloc).min(0xFFFF);
        let largest = Self::largest_free(mem) as u32;
        if largest < need || need > 0xFFFF {
            self.procs.pop();
            return Err("not enough memory".into());
        }
        let paras = want.min(largest) as u16;
        let psp = self.alloc(mem, paras, 0, b"").ok_or("no memory")?;
        // The block belongs to the new process.
        let seg = psp - 1;
        let (kind, _, size) = Self::mcb(mem, seg);
        Self::write_mcb(mem, seg, kind, psp, size, dos_path.rsplit('\\').next().unwrap_or("").as_bytes());
        let (ek, _, es) = Self::mcb(mem, env_seg - 1);
        if ek == b'M' || ek == b'Z' {
            Self::write_mcb(mem, env_seg - 1, ek, psp, es, b"");
        }
        self.build_psp(mem, psp, parent, env_seg, args, psp + size);
        let load_seg = psp + 0x10;
        let base = (load_seg as u32) << 4;
        if is_exe {
            let header_paras = u16::from_le_bytes([image[8], image[9]]) as usize;
            let body = &image[header_paras * 16..];
            mem.write(base, &body[..body.len().min(load_size as usize)]);
            let reloc_count = u16::from_le_bytes([image[6], image[7]]) as usize;
            let reloc_off = u16::from_le_bytes([image[0x18], image[0x19]]) as usize;
            for i in 0..reloc_count {
                let r = reloc_off + i * 4;
                if r + 4 > image.len() {
                    break;
                }
                let off = u16::from_le_bytes([image[r], image[r + 1]]) as u32;
                let sg = u16::from_le_bytes([image[r + 2], image[r + 3]]);
                let a = linear(load_seg.wrapping_add(sg), off);
                let v = mem.read16(a).wrapping_add(load_seg);
                mem.write16(a, v);
            }
            let init_ss = u16::from_le_bytes([image[0xE], image[0xF]]);
            let init_sp = u16::from_le_bytes([image[0x10], image[0x11]]);
            let init_ip = u16::from_le_bytes([image[0x14], image[0x15]]);
            let init_cs = u16::from_le_bytes([image[0x16], image[0x17]]);
            cpu.regs = [0; 8];
            cpu.sregs[cpu::CS] = load_seg.wrapping_add(init_cs);
            cpu.ip = init_ip;
            cpu.sregs[cpu::SS] = load_seg.wrapping_add(init_ss);
            cpu.set_r16(cpu::SP, init_sp);
        } else {
            mem.write(base, &image);
            cpu.regs = [0; 8];
            cpu.sregs[cpu::CS] = psp;
            cpu.sregs[cpu::SS] = psp;
            cpu.set_r16(cpu::SP, 0xFFFE);
            mem.write16(linear(psp, 0xFFFE), 0);
            cpu.ip = 0x100;
        }
        cpu.sregs[cpu::DS] = psp;
        cpu.sregs[cpu::ES] = psp;
        cpu.flags = (cpu.flags & !(cpu::TF | cpu::DF)) | cpu::IF | 0x0002;
        cpu.halted = false;
        self.psp = psp;
        self.dta = (psp, 0x80);
        Ok(())
    }

    fn build_psp(&self, mem: &mut Memory, psp: u16, parent: u16, env_seg: u16, args: &str, top: u16) {
        let a = (psp as u32) << 4;
        for i in 0..256u32 {
            mem.write8(a + i, 0);
        }
        mem.write16(a, 0x20CD);
        mem.write16(a + 2, top);
        // INT 22h/23h/24h vectors as they stand.
        mem.write32(a + 0x0A, mem.read32(0x22 * 4));
        mem.write32(a + 0x0E, mem.read32(0x23 * 4));
        mem.write32(a + 0x12, mem.read32(0x24 * 4));
        mem.write16(a + 0x16, if parent == 0 { psp } else { parent });
        for i in 0..20u32 {
            mem.write8(a + 0x18 + i, if i < 5 { i as u8 } else { 0xFF });
        }
        mem.write16(a + 0x2C, env_seg);
        mem.write16(a + 0x32, 20);
        mem.write32(a + 0x34, (psp as u32) << 16 | 0x18);
        mem.write8(a + 0x50, 0xCD);
        mem.write8(a + 0x51, 0x21);
        mem.write8(a + 0x52, 0xCB);
        let tail: Vec<u8> = args.bytes().take(126).collect();
        mem.write8(a + 0x80, tail.len() as u8 + if tail.is_empty() { 0 } else { 1 });
        let mut t = Vec::new();
        if !tail.is_empty() {
            t.push(b' ');
            t.extend(&tail);
        }
        t.push(0x0D);
        mem.write(a + 0x81, &t);
    }

    /// A process ended: tidy after it and go back to its parent, or
    /// report the machine's exit code if it was the last.
    pub fn terminate(&mut self, code: u8, cpu: &mut Cpu, mem: &mut Memory) -> Option<u8> {
        let psp = self.psp;
        self.close_owned(psp);
        share::release((self.machine, psp));
        self.free_owned(mem, psp);
        self.child_code = code;
        match self.procs.pop() {
            Some(p) => {
                cpu.regs = p.regs;
                cpu.sregs = p.sregs;
                cpu.ip = p.ip;
                cpu.flags = p.flags & !cpu::CF;
                cpu.set_ax(code as u16);
                self.psp = p.psp;
                self.dta = p.dta;
                None
            }
            None => {
                self.psp = 0;
                Some(code)
            }
        }
    }
}

enum Resolved {
    Path { host: PathBuf, drive: usize, dos: String },
    Device(Handle),
}

fn read_env(mem: &Memory, seg: u16) -> Vec<u8> {
    let a = (seg as u32) << 4;
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let b = mem.read8(a + i);
        if b == 0 && (i == 0 || mem.read8(a + i - 1) == 0) {
            break;
        }
        out.push(b);
        i += 1;
        if i > 4096 {
            break;
        }
    }
    out
}

/// A directory entry matching `name` without regard to case.
fn find_ci(dir: &Path, name: &str) -> Option<String> {
    // The name as the host spells it (its file system may not care
    // about case either, but the DOS side should see one spelling).
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|f| f.eq_ignore_ascii_case(name))
        .or_else(|| dir.join(name).exists().then(|| name.to_string()))
}

// ---- INT 21h -----------------------------------------------------------------

fn fail(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory, code: u16) {
    if sys.trace {
        sys.log.push(format!("  -> error {code}"));
    }
    sys.dos.last_error = code;
    cpu.set_ax(code);
    set_cf(cpu, mem, true);
}

fn ok(cpu: &Cpu, mem: &mut Memory) {
    clear_cf(cpu, mem);
}

pub fn int21(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) -> After {
    let ah = cpu.ah();
    if sys.trace && !matches!(ah, 0x0B | 0x2C | 0x2A | 0x06) {
        let name = if matches!(ah, 0x39..=0x3D | 0x41 | 0x43 | 0x4B | 0x4E | 0x56 | 0x5A | 0x5B | 0x6C) {
            read_asciiz(mem, cpu.ds(), if ah == 0x6C { cpu.r16(cpu::SI) } else { cpu.dx() })
        } else {
            String::new()
        };
        sys.log.push(format!("21h {:04X} bx={:04X} cx={:04X} dx={:04X} {name}", cpu.ax(), cpu.r16(cpu::BX), cpu.cx(), cpu.dx()));
    }
    ok(cpu, mem);
    match ah {
        0x00 => return exit(sys, 0, cpu, mem),
        // Character input (01 echoes; 07/08 don't).
        0x01 | 0x07 | 0x08 => match sys.keys.pop_front() {
            Some(k) => {
                sys.active();
                let c = k as u8;
                cpu.set_al(c);
                if ah == 0x01 {
                    Screen::teletype(mem, c, 0x07);
                }
            }
            None => sys.block(cpu, mem),
        },
        0x02 => Screen::teletype(mem, cpu.r8(2), 0x07),
        0x03 => match sys.input.pop_front() {
            Some(b) => {
                sys.go_remote();
                cpu.set_al(b);
            }
            None => sys.block(cpu, mem),
        },
        0x04 => {
            sys.go_remote();
            sys.output.push(cpu.r8(2));
        }
        0x05 => {}
        0x06 => {
            let dl = cpu.r8(2);
            if dl == 0xFF {
                match sys.keys.pop_front() {
                    Some(k) => {
                        sys.active();
                        cpu.set_al(k as u8);
                        crate::bios::set_zf(cpu, mem, false);
                    }
                    None => {
                        sys.polled();
                        cpu.set_al(0);
                        crate::bios::set_zf(cpu, mem, true);
                    }
                }
            } else {
                Screen::teletype(mem, dl, 0x07);
            }
        }
        0x09 => {
            for off in (cpu.dx() as u32..).take(4096) {
                let b = mem.read8(linear(cpu.ds(), off));
                if b == b'$' {
                    break;
                }
                Screen::teletype(mem, b, 0x07);
            }
        }
        0x0A => {
            // Buffered input: DS:DX -> max, count, text.
            let a = linear(cpu.ds(), cpu.dx() as u32);
            let max = mem.read8(a) as usize;
            let mut line: Vec<u8> = Vec::new();
            let mut done = false;
            while let Some(k) = sys.keys.pop_front() {
                let c = k as u8;
                match c {
                    0x0D => {
                        done = true;
                        break;
                    }
                    0x08 => {
                        if line.pop().is_some() {
                            Screen::teletype(mem, 0x08, 7);
                            Screen::teletype(mem, b' ', 7);
                            Screen::teletype(mem, 0x08, 7);
                        }
                    }
                    0 => {}
                    _ if line.len() + 1 < max => {
                        line.push(c);
                        Screen::teletype(mem, c, 7);
                    }
                    _ => {}
                }
            }
            if !done {
                // Keep what's typed so far and wait for more.
                for &c in line.iter().rev() {
                    sys.keys.push_front(c as u16);
                }
                for _ in 0..line.len() {
                    Screen::teletype(mem, 0x08, 7);
                }
                sys.block(cpu, mem);
            } else {
                sys.active();
                mem.write8(a + 1, line.len() as u8);
                mem.write(a + 2, &line);
                mem.write8(a + 2 + line.len() as u32, 0x0D);
                Screen::teletype(mem, 0x0D, 7);
            }
        }
        0x0B => {
            if sys.keys.is_empty() {
                sys.polled();
                cpu.set_al(0);
            } else {
                cpu.set_al(0xFF);
            }
        }
        0x0C => {
            sys.keys.clear();
            let f = cpu.al();
            if matches!(f, 0x01 | 0x06 | 0x07 | 0x08 | 0x0A) {
                cpu.set_ah(f);
                return int21(sys, cpu, mem);
            }
            cpu.set_al(0);
        }
        0x0D => {}
        0x0E => {
            let d = cpu.r8(2) as usize;
            if d < 26 && sys.dos.roots[d].is_some() {
                sys.dos.cur_drive = d;
            }
            cpu.set_al(26);
        }
        0x19 => cpu.set_al(sys.dos.cur_drive as u8),
        0x1A => sys.dos.dta = (cpu.ds(), cpu.dx()),
        0x25 => {
            let n = cpu.al() as u32;
            mem.write16(n * 4, cpu.dx());
            mem.write16(n * 4 + 2, cpu.ds());
        }
        0x2A => {
            let now = crate::now();
            cpu.set_r16(cpu::CX, now.year() as u16);
            cpu.set_r8(6, now.month() as u8);
            cpu.set_r8(2, now.day() as u8);
            cpu.set_al(now.weekday().num_days_from_sunday() as u8);
        }
        0x2B | 0x2D => cpu.set_al(0),
        0x2C => {
            let now = crate::now();
            cpu.set_r8(5, now.hour() as u8);
            cpu.set_r8(1, now.minute() as u8);
            cpu.set_r8(6, now.second() as u8);
            cpu.set_r8(2, (now.nanosecond() / 10_000_000) as u8);
        }
        0x2E => sys.dos.verify = cpu.al() != 0,
        0x2F => {
            cpu.sregs[cpu::ES] = sys.dos.dta.0;
            cpu.set_r16(cpu::BX, sys.dos.dta.1);
        }
        0x30 => {
            cpu.set_ax(0x0005);
            cpu.set_r16(cpu::BX, 0);
            cpu.set_r16(cpu::CX, 0);
        }
        0x33 => match cpu.al() {
            0 => cpu.set_r8(2, sys.dos.break_flag as u8),
            1 => sys.dos.break_flag = cpu.r8(2) != 0,
            5 => cpu.set_r8(2, 3),
            6 => {
                cpu.set_r16(cpu::BX, 0x0005);
                cpu.set_r16(cpu::DX, 0);
            }
            _ => {}
        },
        0x34 => {
            cpu.sregs[cpu::ES] = DOS_SEG;
            cpu.set_r16(cpu::BX, 0x0001);
        }
        0x35 => {
            let n = cpu.al() as u32;
            cpu.sregs[cpu::ES] = mem.read16(n * 4 + 2);
            cpu.set_r16(cpu::BX, mem.read16(n * 4));
        }
        0x36 => {
            let d = cpu.r8(2) as usize;
            let d = if d == 0 { sys.dos.cur_drive } else { d - 1 };
            if d < 26 && sys.dos.roots[d].is_some() {
                cpu.set_ax(8);
                cpu.set_r16(cpu::BX, 25_600);
                cpu.set_r16(cpu::CX, 512);
                cpu.set_r16(cpu::DX, 65_000);
            } else {
                cpu.set_ax(0xFFFF);
            }
        }
        0x37 => {
            if cpu.al() == 0 {
                cpu.set_r8(2, b'/');
            }
            cpu.set_al(0);
        }
        0x38 => {
            // Country information (US) into DS:DX.
            let a = linear(cpu.ds(), cpu.dx() as u32);
            let info: [u8; 34] = [
                1, 0, b'$', 0, 0, 0, 0, b',', 0, b'.', 0, b'-', 0, b':', 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, b',', 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];
            mem.write(a, &info);
            cpu.set_r16(cpu::BX, 1);
        }
        0x39 | 0x3A | 0x3B | 0x41 | 0x43 | 0x56 | 0x5B | 0x3C | 0x3D | 0x4E | 0x6C => files_by_name(sys, cpu, mem, ah),
        0x3E => match sys.dos.files.get_mut(cpu.bx() as usize) {
            Some(slot) if slot.is_some() => {
                if cpu.bx() > 4 {
                    *slot = None;
                }
            }
            _ => fail(sys, cpu, mem, ERR_BAD_HANDLE),
        },
        0x3F => read(sys, cpu, mem),
        0x40 => write(sys, cpu, mem),
        0x42 => {
            let pos = (cpu.cx() as u64) << 16 | cpu.dx() as u64;
            match sys.dos.file_mut(cpu.bx()) {
                Ok(Handle::File { f, .. }) => {
                    let whence = match cpu.al() {
                        0 => SeekFrom::Start(pos),
                        1 => SeekFrom::Current(pos as u32 as i32 as i64),
                        _ => SeekFrom::End(pos as u32 as i32 as i64),
                    };
                    match f.seek(whence) {
                        Ok(p) => {
                            cpu.set_ax(p as u16);
                            cpu.set_r16(cpu::DX, (p >> 16) as u16);
                        }
                        Err(_) => fail(sys, cpu, mem, 25),
                    }
                }
                Ok(_) => {
                    cpu.set_ax(0);
                    cpu.set_r16(cpu::DX, 0);
                }
                Err(e) => fail(sys, cpu, mem, e),
            }
        }
        0x44 => ioctl(sys, cpu, mem),
        0x45 => match sys.dos.files.get(cpu.bx() as usize) {
            Some(Some(h)) => {
                let dup = match h {
                    Handle::File { f, path, owner, share } => f.try_clone().ok().map(|f| Handle::File { f, path: path.clone(), owner: *owner, share: share.clone() }),
                    Handle::Stdin => Some(Handle::Stdin),
                    Handle::Stdout => Some(Handle::Stdout),
                    Handle::Aux => Some(Handle::Aux),
                    Handle::Nul => Some(Handle::Nul),
                };
                match dup.ok_or(ERR_ACCESS_DENIED).and_then(|d| sys.dos.open_handle(d)) {
                    Ok(h) => cpu.set_ax(h),
                    Err(e) => fail(sys, cpu, mem, e),
                }
            }
            _ => fail(sys, cpu, mem, ERR_BAD_HANDLE),
        },
        0x46 => {
            // Force-duplicate BX onto CX: good enough to say yes for the
            // standard handles, which is what redirection code does.
            if cpu.bx() as usize >= MAX_HANDLES || cpu.cx() as usize >= MAX_HANDLES {
                fail(sys, cpu, mem, ERR_BAD_HANDLE);
            }
        }
        0x47 => {
            let d = cpu.r8(2) as usize;
            let d = if d == 0 { sys.dos.cur_drive } else { d - 1 };
            if d < 26 && sys.dos.roots[d].is_some() {
                let cwd = sys.dos.cwd_dos(d);
                let a = linear(cpu.ds(), cpu.si() as u32);
                mem.write(a, cwd.as_bytes());
                mem.write8(a + cwd.len() as u32, 0);
            } else {
                fail(sys, cpu, mem, ERR_BAD_DRIVE);
            }
        }
        0x48 => {
            let paras = cpu.bx();
            let owner = sys.dos.psp;
            match sys.dos.alloc(mem, paras, owner, b"") {
                Some(seg) => cpu.set_ax(seg),
                None => {
                    cpu.set_r16(cpu::BX, Dos::largest_free(mem));
                    fail(sys, cpu, mem, ERR_NO_MEMORY);
                }
            }
        }
        0x49 => {
            if !sys.dos.free(mem, cpu.es()) {
                fail(sys, cpu, mem, 9);
            }
        }
        0x4A => {
            if let Err(max) = sys.dos.resize(mem, cpu.es(), cpu.bx()) {
                cpu.set_r16(cpu::BX, max);
                fail(sys, cpu, mem, ERR_NO_MEMORY);
            }
        }
        0x4B => {
            let name = read_asciiz(mem, cpu.ds(), cpu.dx());
            let pb = linear(cpu.es(), cpu.bx() as u32);
            let env = mem.read16(pb);
            let tail_off = mem.read16(pb + 2);
            let tail_seg = mem.read16(pb + 4);
            let n = mem.read8(linear(tail_seg, tail_off as u32)) as u32;
            let tail: Vec<u8> = (0..n).map(|i| mem.read8(linear(tail_seg, tail_off as u32 + 1 + i))).collect();
            let tail = String::from_utf8_lossy(&tail).trim().to_string();
            if cpu.al() != 0 {
                fail(sys, cpu, mem, 1);
            } else {
                // Return to the caller first, so its state is what the
                // child comes back to.
                cpu.do_iret(mem);
                match sys.dos.exec(cpu, mem, &name, &tail, Some(env)) {
                    Ok(()) => {}
                    Err(e) => {
                        sys.log.push(format!("exec {name}: {e}"));
                        cpu.set_ax(ERR_FILE_NOT_FOUND);
                        cpu.set_flag(cpu::CF, true);
                    }
                }
                return After::Redirected;
            }
        }
        0x4C => return exit(sys, cpu.al(), cpu, mem),
        0x4D => cpu.set_ax(sys.dos.child_code as u16),
        0x4F => find_next(sys, cpu, mem),
        0x50 => sys.dos.psp = cpu.bx(),
        0x51 | 0x62 => cpu.set_r16(cpu::BX, sys.dos.psp),
        0x52 => {
            cpu.sregs[cpu::ES] = DOS_SEG;
            cpu.set_r16(cpu::BX, SYSVARS);
        }
        0x54 => cpu.set_al(sys.dos.verify as u8),
        0x57 => match sys.dos.file_mut(cpu.bx()) {
            Ok(Handle::File { path, .. }) => {
                if cpu.al() == 0 {
                    let (t, d) = dos_time(host_time(path));
                    cpu.set_r16(cpu::CX, t);
                    cpu.set_r16(cpu::DX, d);
                }
            }
            Ok(_) => {
                cpu.set_r16(cpu::CX, 0);
                cpu.set_r16(cpu::DX, 0);
            }
            Err(e) => fail(sys, cpu, mem, e),
        },
        0x58 => match cpu.al() {
            0 => cpu.set_ax(sys.dos.strategy),
            1 => sys.dos.strategy = cpu.bx(),
            2 => cpu.set_al(0),
            _ => {}
        },
        0x59 => {
            cpu.set_ax(sys.dos.last_error);
            cpu.set_r8(7, 1); // BH class
            cpu.set_r8(3, 3); // BL action
            cpu.set_r8(5, 1); // CH locus
        }
        0x5A => {
            // Temporary file in the directory at DS:DX (with a trailing \):
            // the first free name, since other nodes make them too.
            let dir = read_asciiz(mem, cpu.ds(), cpu.dx());
            let seed = (sys.dos.machine as u32).wrapping_mul(7919) % 100_000;
            let mut made = None;
            for i in 0..1000u32 {
                let name = format!("{dir}TMP{:05}.$$$", (seed + i) % 100_000);
                let Ok(Resolved::Path { host, .. }) = sys.dos.resolve(&name, true) else { break };
                match File::options().read(true).write(true).create_new(true).open(&host) {
                    Ok(f) => {
                        made = Some((name, host, f));
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(_) => break,
                }
            }
            match made.map(|(name, host, f)| (Shared::open(&host, 2, 0, false), name, host, f)) {
                None => fail(sys, cpu, mem, ERR_PATH_NOT_FOUND),
                Some((Ok(share), name, host, f)) => {
                    let full = name.as_bytes();
                    let a = linear(cpu.ds(), cpu.dx() as u32);
                    mem.write(a, full);
                    mem.write8(a + full.len() as u32, 0);
                    let owner = sys.dos.psp;
                    match sys.dos.open_handle(Handle::File { f, path: host, owner, share }) {
                        Ok(h) => cpu.set_ax(h),
                        Err(e) => fail(sys, cpu, mem, e),
                    }
                }
                Some((Err(e), ..)) => fail(sys, cpu, mem, e),
            }
        }
        // Lock or unlock a range (SHARE's): CX:DX from, SI:DI long.
        0x5C => {
            let start = (cpu.cx() as u64) << 16 | cpu.dx() as u64;
            let len = (cpu.si() as u64) << 16 | cpu.r16(cpu::DI) as u64;
            let owner = sys.dos.lock_owner();
            let r = match (sys.dos.file_mut(cpu.bx()), cpu.al()) {
                (Ok(Handle::File { share, .. }), 0) => share.lock(owner, start, len),
                (Ok(Handle::File { share, .. }), 1) => share.unlock(owner, start, len),
                (Ok(_), 0 | 1) => Ok(()),
                (Ok(_), _) => Err(1),
                (Err(e), _) => Err(e),
            };
            if let Err(e) = r {
                fail(sys, cpu, mem, e);
            }
        }
        0x5D => {
            if cpu.al() == 0x0A {
                // Set extended error: ignore.
            }
        }
        0x60 => {
            // Canonicalise DS:SI into ES:DI.
            let name = read_asciiz(mem, cpu.ds(), cpu.si());
            match sys.dos.resolve(&name, true) {
                Ok(Resolved::Path { drive, dos, .. }) => {
                    let full = format!("{}:\\{dos}", (b'A' + drive as u8) as char);
                    let a = linear(cpu.es(), cpu.di() as u32);
                    mem.write(a, full.as_bytes());
                    mem.write8(a + full.len() as u32, 0);
                }
                Ok(Resolved::Device(_)) => {
                    let a = linear(cpu.es(), cpu.di() as u32);
                    mem.write(a, name.as_bytes());
                    mem.write8(a + name.len() as u32, 0);
                }
                Err(e) => fail(sys, cpu, mem, e),
            }
        }
        0x63 => cpu.set_al(0xFF),
        0x65 | 0x66 => {
            // Code page and case tables: not here.
            fail(sys, cpu, mem, 1);
        }
        0x68 => {
            if let Ok(Handle::File { f, .. }) = sys.dos.file_mut(cpu.bx()) {
                let _ = f.flush();
            }
        }
        0x6A | 0x6B => {}
        _ => {
            sys.log.push(format!("INT 21h function {ah:02X}h not implemented"));
            fail(sys, cpu, mem, 1);
        }
    }
    After::Iret
}

fn exit(sys: &mut System, code: u8, cpu: &mut Cpu, mem: &mut Memory) -> After {
    // Pop the INT frame first; a parent resumes from its own saved state.
    cpu.do_iret(mem);
    if let Some(c) = sys.dos.terminate(code, cpu, mem) {
        sys.exit = Some(c);
    }
    After::Redirected
}

fn read(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) {
    let n = cpu.cx() as usize;
    let a = linear(cpu.ds(), cpu.dx() as u32);
    let h = cpu.bx();
    let owner = sys.dos.lock_owner();
    match sys.dos.files.get_mut(h as usize) {
        Some(Some(Handle::File { f, share, .. })) => {
            if let Err(e) = f.stream_position().map_or(Ok(()), |pos| share.check(owner, pos, n as u64)) {
                return fail(sys, cpu, mem, e);
            }
            // DOS returns fewer bytes only at the end of the file.
            let mut buf = vec![0u8; n];
            let mut got = 0;
            let r = loop {
                match f.read(&mut buf[got..]) {
                    Ok(0) => break Ok(got),
                    Ok(k) => {
                        got += k;
                        if got == n {
                            break Ok(got);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => break Err(e),
                }
            };
            match r {
                Ok(got) => {
                    mem.write(a, &buf[..got]);
                    cpu.set_ax(got as u16);
                }
                Err(_) => fail(sys, cpu, mem, ERR_ACCESS_DENIED),
            }
        }
        Some(Some(Handle::Stdin)) => {
            // A line from the keyboard, like function 0Ah with echo.
            let mut line: Vec<u8> = Vec::new();
            let mut done = false;
            while let Some(k) = sys.keys.pop_front() {
                let c = k as u8;
                if c == 0x0D {
                    done = true;
                    break;
                }
                if c == 0x08 {
                    line.pop();
                    continue;
                }
                if c != 0 {
                    line.push(c);
                }
            }
            if !done {
                for &c in line.iter().rev() {
                    sys.keys.push_front(c as u16);
                }
                sys.block(cpu, mem);
                return;
            }
            sys.active();
            for &c in &line {
                Screen::teletype(mem, c, 7);
            }
            Screen::teletype(mem, 0x0D, 7);
            Screen::teletype(mem, 0x0A, 7);
            line.extend([0x0D, 0x0A]);
            let take = line.len().min(n);
            mem.write(a, &line[..take]);
            cpu.set_ax(take as u16);
        }
        Some(Some(Handle::Aux)) => {
            sys.go_remote();
            let mut got = 0;
            while got < n {
                let Some(b) = sys.input.pop_front() else { break };
                mem.write8(a + got as u32, b);
                got += 1;
            }
            if got == 0 && n > 0 {
                sys.block(cpu, mem);
                return;
            }
            cpu.set_ax(got as u16);
        }
        Some(Some(_)) => cpu.set_ax(0),
        _ => fail(sys, cpu, mem, ERR_BAD_HANDLE),
    }
}

fn write(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) {
    let n = cpu.cx() as usize;
    let a = linear(cpu.ds(), cpu.dx() as u32);
    let h = cpu.bx();
    let data: Vec<u8> = (0..n as u32).map(|i| mem.read8(a + i)).collect();
    let owner = sys.dos.lock_owner();
    match sys.dos.files.get_mut(h as usize) {
        Some(Some(Handle::File { f, share, .. })) => {
            if let Err(e) = f.stream_position().map_or(Ok(()), |pos| share.check(owner, pos, n as u64)) {
                return fail(sys, cpu, mem, e);
            }
            if n == 0 {
                // Truncate at the current position.
                if let Ok(pos) = f.stream_position() {
                    let _ = f.set_len(pos);
                }
                cpu.set_ax(0);
            } else {
                match f.write_all(&data) {
                    Ok(()) => cpu.set_ax(n as u16),
                    Err(_) => fail(sys, cpu, mem, ERR_ACCESS_DENIED),
                }
            }
        }
        Some(Some(Handle::Stdout)) => {
            for &b in &data {
                Screen::teletype(mem, b, 0x07);
            }
            cpu.set_ax(n as u16);
        }
        Some(Some(Handle::Aux)) => {
            sys.go_remote();
            sys.active();
            sys.output.extend(&data);
            cpu.set_ax(n as u16);
        }
        Some(Some(Handle::Nul)) => cpu.set_ax(n as u16),
        Some(Some(Handle::Stdin)) => cpu.set_ax(0),
        _ => fail(sys, cpu, mem, ERR_BAD_HANDLE),
    }
}

fn ioctl(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) {
    let h = cpu.bx();
    match cpu.al() {
        0x00 => match sys.dos.files.get(h as usize) {
            Some(Some(Handle::File { .. })) => cpu.set_r16(cpu::DX, 0x0002), // drive C, file
            Some(Some(Handle::Stdin)) => cpu.set_r16(cpu::DX, 0x80D3),
            Some(Some(Handle::Stdout)) => cpu.set_r16(cpu::DX, 0x80D3),
            Some(Some(Handle::Aux)) => cpu.set_r16(cpu::DX, 0x80C0),
            Some(Some(Handle::Nul)) => cpu.set_r16(cpu::DX, 0x80C4),
            _ => fail(sys, cpu, mem, ERR_BAD_HANDLE),
        },
        0x01 => {}
        0x06 => match sys.dos.files.get(h as usize) {
            Some(Some(Handle::Stdin)) => cpu.set_al(if sys.keys.is_empty() { 0 } else { 0xFF }),
            Some(Some(Handle::Aux)) => cpu.set_al(if sys.input.is_empty() { 0 } else { 0xFF }),
            Some(Some(_)) => cpu.set_al(0xFF),
            _ => fail(sys, cpu, mem, ERR_BAD_HANDLE),
        },
        0x07 => cpu.set_al(0xFF),
        0x08 => cpu.set_ax(1), // not removable
        0x09 => cpu.set_r16(cpu::DX, 0), // local drive
        0x0A => cpu.set_r16(cpu::DX, 0), // local handle
        0x0B => {}                       // SHARE's retry count: refused at once
        0x0E | 0x0F => cpu.set_al(0),
        _ => fail(sys, cpu, mem, 1),
    }
}

/// The functions that take a path at DS:DX (6Ch: DS:SI, DX its action).
fn files_by_name(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory, ah: u8) {
    let name = read_asciiz(mem, cpu.ds(), if ah == 0x6C { cpu.r16(cpu::SI) } else { cpu.dx() });
    let creating = matches!(ah, 0x39 | 0x3C | 0x5B) || (ah == 0x6C && cpu.dx() != 0 && cpu.r16(cpu::DX) & 0x10 != 0);
    let resolved = match sys.dos.resolve(&name, creating || ah == 0x6C || ah == 0x4E) {
        Ok(r) => r,
        Err(e) => return fail(sys, cpu, mem, e),
    };
    let (host, drive, dos) = match resolved {
        Resolved::Device(h) => {
            // Devices open as themselves.
            if matches!(ah, 0x3C | 0x3D | 0x6C) {
                match sys.dos.open_handle(h) {
                    Ok(n) => cpu.set_ax(n),
                    Err(e) => fail(sys, cpu, mem, e),
                }
            } else {
                fail(sys, cpu, mem, ERR_ACCESS_DENIED);
            }
            return;
        }
        Resolved::Path { host, drive, dos } => (host, drive, dos),
    };
    let owner = sys.dos.psp;
    // Opened with SHARE's leave (`share`, taken before the file is
    // touched, so a refused create doesn't truncate it).
    let open = |sys: &mut System, cpu: &mut Cpu, mem: &mut Memory, f: std::io::Result<File>, host: PathBuf, share: std::sync::Arc<Shared>| match f {
        Ok(f) => match sys.dos.open_handle(Handle::File { f, path: host, owner, share }) {
            Ok(h) => cpu.set_ax(h),
            Err(e) => fail(sys, cpu, mem, e),
        },
        Err(e) => fail(sys, cpu, mem, if e.kind() == std::io::ErrorKind::NotFound { ERR_FILE_NOT_FOUND } else { ERR_ACCESS_DENIED }),
    };
    match ah {
        0x39 => {
            if std::fs::create_dir(&host).is_err() {
                fail(sys, cpu, mem, ERR_ACCESS_DENIED);
            }
        }
        0x3A => {
            if std::fs::remove_dir(&host).is_err() {
                fail(sys, cpu, mem, ERR_ACCESS_DENIED);
            }
        }
        0x3B => {
            if host.is_dir() {
                sys.dos.cwd[drive] = dos;
            } else {
                fail(sys, cpu, mem, ERR_PATH_NOT_FOUND);
            }
        }
        0x3C => {
            let share = match Shared::open(&host, 2, 0, host.exists()) {
                Ok(s) => s,
                Err(e) => return fail(sys, cpu, mem, e),
            };
            let f = File::options().read(true).write(true).create(true).truncate(true).open(&host);
            open(sys, cpu, mem, f, host, share);
        }
        0x5B => {
            let f = File::options().read(true).write(true).create_new(true).open(&host);
            match f {
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => fail(sys, cpu, mem, 80),
                f => match Shared::open(&host, 2, 0, false) {
                    Ok(share) => open(sys, cpu, mem, f, host, share),
                    Err(e) => fail(sys, cpu, mem, e),
                },
            }
        }
        0x3D | 0x6C => {
            if !host.is_file() && ah == 0x3D {
                return fail(sys, cpu, mem, if host.is_dir() { ERR_ACCESS_DENIED } else { ERR_FILE_NOT_FOUND });
            }
            // Access codes: 0 read, 1 write, 2 read/write, 4 read without
            // touching the file's dates (LORD 4.07 opens with 44h, as DOSBox
            // notes too); the rest are invalid (error 0Ch).
            let full = if ah == 0x3D { cpu.al() } else { cpu.bx() as u8 };
            let mode = full & 7;
            if !matches!(mode, 0 | 1 | 2 | 4) {
                return fail(sys, cpu, mem, 0x0C);
            }
            // 6Ch's action: 1 opened, 2 created, 3 replaced (emptied).
            let exists = host.is_file();
            let action = if ah != 0x6C {
                1
            } else if !exists {
                2
            } else if cpu.dx() & 0x0F == 2 {
                3
            } else {
                1
            };
            let share = match Shared::open(&host, mode, (full >> 4) & 7, action == 3) {
                Ok(s) => s,
                Err(e) => return fail(sys, cpu, mem, e),
            };
            let mut o = File::options();
            o.read(true);
            if mode == 1 || mode == 2 {
                o.write(true);
            }
            if ah == 0x6C {
                let action = cpu.dx();
                let exists = host.is_file();
                if exists && action & 0x0F == 0 {
                    return fail(sys, cpu, mem, 80);
                }
                if !exists {
                    if action & 0xF0 == 0 {
                        return fail(sys, cpu, mem, ERR_FILE_NOT_FOUND);
                    }
                    o.write(true).create(true);
                } else if action & 0x0F == 2 {
                    o.write(true).truncate(true);
                }
            }
            let f = o.open(&host).or_else(|_| File::open(&host));
            if ah == 0x6C {
                cpu.set_r16(cpu::CX, action);
            }
            open(sys, cpu, mem, f, host, share);
        }
        0x41 => {
            // SHARE: not while it's open, here or on another node.
            if crate::share::in_use(&host) {
                return fail(sys, cpu, mem, ERR_ACCESS_DENIED);
            }
            if std::fs::remove_file(&host).is_err() {
                fail(sys, cpu, mem, ERR_FILE_NOT_FOUND);
            }
        }
        0x43 => match cpu.al() {
            0 => {
                let md = std::fs::metadata(&host);
                match md {
                    Ok(m) => {
                        let mut attr = if m.is_dir() { 0x10 } else { 0x20 };
                        if m.permissions().readonly() {
                            attr |= 1;
                        }
                        cpu.set_r16(cpu::CX, attr);
                    }
                    Err(_) => fail(sys, cpu, mem, ERR_FILE_NOT_FOUND),
                }
            }
            _ => {
                if !host.exists() {
                    fail(sys, cpu, mem, ERR_FILE_NOT_FOUND);
                }
            }
        },
        0x56 => {
            let new = read_asciiz(mem, cpu.es(), cpu.di());
            match sys.dos.resolve(&new, true) {
                Ok(Resolved::Path { host: to, .. }) => {
                    if to.exists() || crate::share::in_use(&host) || std::fs::rename(&host, &to).is_err() {
                        fail(sys, cpu, mem, ERR_ACCESS_DENIED);
                    }
                }
                _ => fail(sys, cpu, mem, ERR_PATH_NOT_FOUND),
            }
        }
        0x4E => {
            let attr = cpu.cx() as u8;
            let pattern = dos.rsplit('\\').next().unwrap_or("").to_string();
            let dir = host.parent().map(Path::to_path_buf).unwrap_or(host.clone());
            let mut entries = Vec::new();
            if attr & 0x08 != 0 && attr & 0x37 == 0 {
                // Volume label only: there isn't one.
            } else if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let Some(name) = dos_name(&e.file_name().to_string_lossy()) else { continue };
                    if !wild_match(&pattern, &name) {
                        continue;
                    }
                    let Ok(m) = e.metadata() else { continue };
                    let is_dir = m.is_dir();
                    if is_dir && attr & 0x10 == 0 {
                        continue;
                    }
                    let mut a = if is_dir { 0x10 } else { 0x20 };
                    if m.permissions().readonly() {
                        a |= 0x01;
                    }
                    entries.push(Entry { name, attr: a, size: if is_dir { 0 } else { m.len() }, time: host_time(&e.path()) });
                }
                entries.sort_by(|a, b| a.name.cmp(&b.name));
            }
            let key = linear(sys.dos.dta.0, sys.dos.dta.1 as u32);
            sys.dos.searches.insert(key, Search { entries, next: 0 });
            find_next(sys, cpu, mem);
        }
        _ => {}
    }
}

fn find_next(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) {
    let key = linear(sys.dos.dta.0, sys.dos.dta.1 as u32);
    let Some(s) = sys.dos.searches.get_mut(&key) else { return fail(sys, cpu, mem, ERR_NO_MORE_FILES) };
    let Some(e) = s.entries.get(s.next) else {
        sys.dos.searches.remove(&key);
        return fail(sys, cpu, mem, ERR_NO_MORE_FILES);
    };
    s.next += 1;
    let (t, d) = dos_time(e.time);
    mem.write8(key + 0x15, e.attr);
    mem.write16(key + 0x16, t);
    mem.write16(key + 0x18, d);
    mem.write32(key + 0x1A, e.size as u32);
    let mut name = e.name.as_bytes().to_vec();
    name.truncate(12);
    name.push(0);
    mem.write(key + 0x1E, &name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_and_names() {
        assert!(wild_match("*.*", "LORD.EXE"));
        assert!(wild_match("*.EXE", "LORD.EXE") && !wild_match("*.EXE", "LORD.COM"));
        assert!(wild_match("L???.*", "LORD.EXE") && !wild_match("L??.*", "LORD.EXE"));
        assert!(wild_match("PLAYER?.DAT", "PLAYER1.DAT") && wild_match("PLAYER?.DAT", "PLAYER.DAT"));
        assert!(wild_match("*", "README") && !wild_match("*", "README.TXT"));
        assert_eq!(dos_name("lord.exe").as_deref(), Some("LORD.EXE"));
        assert_eq!(dos_name("a long name.txt"), None);
    }

    #[test]
    fn memory_blocks() {
        let mut mem = Memory::new();
        let mut dos = Dos::new(&mut mem, Path::new("."), None);
        let total = Dos::largest_free(&mem);
        let a = dos.alloc(&mut mem, 0x100, 0x1234, b"A").unwrap();
        let b = dos.alloc(&mut mem, 0x200, 0x1234, b"B").unwrap();
        assert_eq!(b, a + 0x101);
        assert_eq!(Dos::largest_free(&mem), total - 0x302);
        assert!(dos.free(&mut mem, a));
        assert_eq!(Dos::largest_free(&mem), total - 0x302, "the hole before B is smaller");
        assert_eq!(dos.resize(&mut mem, b, 0x1000), Ok(()));
        assert_eq!(dos.resize(&mut mem, b, 0xFFFF).unwrap_err(), total - 0x101);
        dos.free_owned(&mut mem, 0x1234);
        assert_eq!(Dos::largest_free(&mem), total);
    }

    #[test]
    fn paths_resolve_on_the_host() {
        let dir = std::env::temp_dir().join(format!("iqdos{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lord")).unwrap();
        std::fs::write(dir.join("lord/LORD.EXE"), b"MZ").unwrap();
        let mut mem = Memory::new();
        let mut dos = Dos::new(&mut mem, &dir, None);
        match dos.resolve("c:\\LORD\\lord.exe", false).ok().unwrap() {
            Resolved::Path { host, dos, .. } => {
                assert_eq!(host, dir.join("lord/LORD.EXE"));
                assert_eq!(dos, "LORD\\LORD.EXE");
            }
            _ => panic!(),
        }
        dos.cwd[2] = "LORD".into();
        assert!(matches!(dos.resolve("..\\lord\\NEW.DAT", true).ok().unwrap(), Resolved::Path { host, .. } if host == dir.join("lord/NEW.DAT")));
        assert_eq!(dos.resolve("NOPE\\X.DAT", false).err(), Some(ERR_PATH_NOT_FOUND));
        assert!(matches!(dos.resolve("NUL", false).ok().unwrap(), Resolved::Device(Handle::Nul)));
        assert!(dos.find_program("lord").is_some(), "found by PATH with an extension added");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
