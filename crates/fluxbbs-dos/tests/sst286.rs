//! Conformance against the SingleStepTests 80286 real-mode suite (traces
//! from a Harris 80C286). Convert its `v1_real_mode/*.MOO.gz` to JSON with
//! the suite's `tools/moo2json.py` (the `cycles` can be left out), put
//! `metadata.json` and `revocation_list.txt` beside them, point
//! `SST286_DIR` there, and run
//! `cargo test -p fluxbbs-dos --release --test sst286 -- --ignored --nocapture`.
//! `SST286_ONLY=F6.7` runs files whose names start with that.

use std::collections::{HashMap, HashSet};

use fluxbbs_dos::cpu::{self, Cpu, Size};
use fluxbbs_dos::{After, Bus, Memory};
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

const REGS: [(&str, usize); 8] = [("ax", cpu::AX), ("bx", cpu::BX), ("cx", cpu::CX), ("dx", cpu::DX), ("sp", cpu::SP), ("bp", cpu::BP), ("si", cpu::SI), ("di", cpu::DI)];
const SREGS: [(&str, usize); 4] = [("cs", cpu::CS), ("ds", cpu::DS), ("es", cpu::ES), ("ss", cpu::SS)];

fn set_reg(cpu: &mut Cpu, k: &str, v: u32) {
    if let Some(&(_, r)) = REGS.iter().find(|(n, _)| *n == k) {
        cpu.set_r16(r, v as u16);
    } else if let Some(&(_, s)) = SREGS.iter().find(|(n, _)| *n == k) {
        cpu.sregs[s] = v as u16;
    } else if k == "ip" {
        cpu.ip = v as u16;
    } else if k == "flags" {
        cpu.flags = v;
    }
}

fn get_reg(cpu: &Cpu, k: &str) -> u32 {
    if let Some(&(_, r)) = REGS.iter().find(|(n, _)| *n == k) {
        cpu.r16(r) as u32
    } else if let Some(&(_, s)) = SREGS.iter().find(|(n, _)| *n == k) {
        cpu.sregs[s] as u32
    } else if k == "ip" {
        cpu.ip as u32
    } else {
        cpu.flags & 0xFFFF
    }
}

/// The undefined-flags mask for a file (`F6.7`: opcode F6, /7).
fn flags_mask(meta: &serde_json::Value, file: &str) -> u32 {
    let mut parts = file.split('.');
    let op = parts.next().unwrap_or("");
    let entry = &meta["opcodes"][op];
    let entry = match parts.next() {
        Some(r) if entry.get("reg").is_some() => &entry["reg"][r],
        _ => entry,
    };
    entry.get("flags-mask").and_then(|m| m.as_u64()).map_or(0xFFFF, |m| m as u32)
}

#[test]
#[ignore]
fn sst286() {
    let Ok(dir) = std::env::var("SST286_DIR") else {
        eprintln!("SST286_DIR not set; skipping");
        return;
    };
    let only = std::env::var("SST286_ONLY").unwrap_or_default();
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/metadata.json")).unwrap()).unwrap();
    let revoked: HashSet<String> = std::fs::read_to_string(format!("{dir}/revocation_list.txt")).unwrap_or_default().lines().map(|l| l.trim().to_string()).collect();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.file_stem().is_some_and(|s| s != "metadata" && s.to_string_lossy().starts_with(&only)))
        .collect();
    files.sort();
    let (mut total, mut failed) = (0usize, 0usize);
    let mut bad: Vec<String> = Vec::new();
    for path in files {
        let file = path.file_stem().unwrap().to_string_lossy().to_uppercase();
        let tests: Vec<Test> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mask = flags_mask(&meta, &file);
        let (mut fails_here, mut shown) = (0, 0);
        let mut mem = Memory::new();
        mem.a20 = true;
        for t in tests.iter().filter(|t| !revoked.contains(&t.hash)) {
            total += 1;
            let mut cpu = Cpu::new();
            cpu.model_286 = true;
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
                for k in REGS.iter().map(|r| r.0).chain(SREGS.iter().map(|r| r.0)).chain(["ip", "flags"]) {
                    let (mut got, mut w) = (get_reg(&cpu, k), want(k));
                    if k == "flags" {
                        got &= mask;
                        w &= mask;
                    }
                    if got != w {
                        why += &format!(" {k}: got {got:#06x} want {w:#06x}");
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
