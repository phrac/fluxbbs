//! Conformance against the SingleStepTests 80386 real-mode suite (traces
//! from a 386EX). Convert its `v1_ex_real_mode/*.MOO.gz` to JSON with the
//! MOO repository's `python/moo2json.py` (the `cycles` can be left out),
//! put `80386.csv` and `revocation_list.txt` beside them, point
//! `SST386_DIR` there, and run
//! `cargo test -p iniquity-dos --release --test sst386 -- --ignored --nocapture`.
//! `SST386_ONLY=66F7` runs files whose names start with that.

use std::collections::{HashMap, HashSet};

use iniquity_dos::cpu::{self, Cpu, Size};
use iniquity_dos::{After, Bus, Memory};
use serde::Deserialize;

#[derive(Deserialize)]
struct State {
    regs: HashMap<String, u32>,
    ram: Vec<(u32, u8)>,
}

#[derive(Deserialize)]
struct Exception {
    number: u8,
    flag_address: u32,
}

#[derive(Deserialize)]
struct Test {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    end: State,
    exception: Option<Exception>,
    hash: String,
}

struct NoBus;
impl Bus for NoBus {
    fn io_read(&mut self, _: u16, size: Size) -> u32 {
        size.mask()
    }
    fn io_write(&mut self, _: u16, _: Size, _: u32) {}
    fn native_interrupt(&mut self, _: u8, _: &mut Cpu, _: &mut Memory) -> After {
        After::Iret
    }
}

const REGS: [(&str, usize); 8] = [("eax", cpu::AX), ("ebx", cpu::BX), ("ecx", cpu::CX), ("edx", cpu::DX), ("esp", cpu::SP), ("ebp", cpu::BP), ("esi", cpu::SI), ("edi", cpu::DI)];
const SREGS: [(&str, usize); 6] = [("cs", cpu::CS), ("ds", cpu::DS), ("es", cpu::ES), ("ss", cpu::SS), ("fs", cpu::FS), ("gs", cpu::GS)];

fn set_reg(cpu: &mut Cpu, k: &str, v: u32) {
    if let Some(&(_, r)) = REGS.iter().find(|(n, _)| *n == k) {
        cpu.regs[r] = v;
    } else if let Some(&(_, s)) = SREGS.iter().find(|(n, _)| *n == k) {
        cpu.sregs[s] = v as u16;
    } else if k == "eip" {
        cpu.ip = v as u16;
    } else if k == "eflags" {
        cpu.flags = v & 0x3_7FD7 | 2;
    }
}

fn get_reg(cpu: &Cpu, k: &str) -> u32 {
    if let Some(&(_, r)) = REGS.iter().find(|(n, _)| *n == k) {
        cpu.regs[r]
    } else if let Some(&(_, s)) = SREGS.iter().find(|(n, _)| *n == k) {
        cpu.sregs[s] as u32
    } else if k == "eip" {
        cpu.ip as u32
    } else {
        cpu.flags & 0xFFFF
    }
}

/// Undefined-flags masks from `80386.csv`, by opcode (`D3`, `0FAF`) and,
/// for group opcodes, `D3.4`.
fn flag_masks(csv: &str) -> HashMap<String, u32> {
    let mut lines = csv.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").split(',').collect();
    let col = |n: &str| head.iter().position(|h| *h == n).unwrap();
    let (op, g, ex, um) = (col("op"), col("g"), col("ex"), col("f_umask"));
    let mut m = HashMap::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() <= um {
            continue;
        }
        let key = if f[g].is_empty() { f[op].to_string() } else { format!("{}.{}", f[op], f[ex]) };
        if let Some(v) = f[um].strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()) {
            m.insert(key, v);
        }
    }
    m
}

/// The mask for a file: its name without the 66/67 prefixes.
fn flags_mask(masks: &HashMap<String, u32>, file: &str) -> u32 {
    let mut f = file;
    while f.len() > 2 && (f.starts_with("66") || f.starts_with("67")) {
        f = &f[2..];
    }
    masks.get(f).or_else(|| masks.get(f.split('.').next().unwrap_or(f))).copied().unwrap_or(0xFFFF)
}

#[test]
#[ignore]
fn sst386() {
    let Ok(dir) = std::env::var("SST386_DIR") else {
        eprintln!("SST386_DIR not set; skipping");
        return;
    };
    let only = std::env::var("SST386_ONLY").unwrap_or_default();
    let masks = flag_masks(&std::fs::read_to_string(format!("{dir}/80386.csv")).unwrap());
    let revoked: HashSet<String> = std::fs::read_to_string(format!("{dir}/revocation_list.txt")).unwrap_or_default().lines().map(|l| l.trim().to_string()).collect();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.file_stem().is_some_and(|s| s.to_string_lossy().starts_with(&only)))
        .collect();
    files.sort();
    let (mut total, mut failed) = (0usize, 0usize);
    let mut bad: Vec<String> = Vec::new();
    for path in files {
        let file = path.file_stem().unwrap().to_string_lossy().to_uppercase();
        let tests: Vec<Test> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mask = flags_mask(&masks, &file);
        let (mut fails_here, mut shown) = (0, 0);
        let mut mem = Memory::new();
        mem.a20 = true;
        for t in tests.iter().filter(|t| !revoked.contains(&t.hash)) {
            total += 1;
            let mut cpu = Cpu::new();
            for (k, v) in &t.initial.regs {
                set_reg(&mut cpu, k, *v);
            }
            for (a, b) in &t.initial.ram {
                mem.write8(*a, *b);
            }
            let mut bus = NoBus;
            // The instruction, then the HLT that ends every test.
            let mut err = None;
            for _ in 0..10_000 {
                if let Err(e) = cpu.step(&mut mem, &mut bus) {
                    err = Some(format!("{e:?}"));
                    break;
                }
                if cpu.halted {
                    break;
                }
            }
            let mut why = String::new();
            if let Some(e) = err {
                why = format!(" {e}");
            } else if !cpu.halted {
                why = " never halted".into();
            } else {
                let want = |k: &str| t.end.regs.get(k).or(t.initial.regs.get(k)).copied().unwrap_or(0);
                for k in REGS.iter().map(|r| r.0).chain(SREGS.iter().map(|r| r.0)).chain(["eip", "eflags"]) {
                    let (mut got, mut w) = (get_reg(&cpu, k), want(k));
                    if k == "eflags" {
                        got &= mask;
                        w &= mask;
                    }
                    if got != w {
                        why += &format!(" {k}: got {got:#x} want {w:#x}");
                    }
                }
                let flag_at = t.exception.as_ref().map(|e| e.flag_address);
                for (a, b) in &t.end.ram {
                    let (mut got, mut w) = (mem.read8(*a), *b);
                    if let Some(f) = flag_at
                        && (*a == f || *a == f + 1)
                    {
                        let m = (mask >> (8 * (*a - f))) as u8;
                        got &= m;
                        w &= m;
                    }
                    if got != w {
                        why += &format!(" [{a:#x}]: got {got:#04x} want {w:#04x}");
                    }
                }
            }
            if !why.is_empty() {
                failed += 1;
                fails_here += 1;
                if shown < 3 {
                    let exc = t.exception.as_ref().map(|e| format!(" (int {})", e.number)).unwrap_or_default();
                    eprintln!("FAIL {file} {:?}{exc}:{why}", t.name);
                    shown += 1;
                }
            }
            for (a, _) in t.initial.ram.iter().chain(t.end.ram.iter()) {
                mem.write8(*a, 0);
            }
        }
        if fails_here > 0 {
            bad.push(format!("{file}:{fails_here}/{}", tests.len()));
        }
    }
    eprintln!("{total} tests, {failed} failed; files with failures: {}", bad.join(" "));
    assert_eq!(failed, 0);
}
