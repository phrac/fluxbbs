//! Conformance against the SingleStepTests 8088 suite (real-hardware
//! traces, 10,000 per opcode). Point `SST8088_DIR` at a directory of its
//! unpacked `v2/*.json` files and `metadata.json`, then
//! `cargo test -p iniquity-dos --test sst8088 -- --ignored --nocapture`.

use std::collections::HashMap;

use iniquity_dos::cpu::{self, Cpu, Size};
use iniquity_dos::{After, Bus, Memory};
use serde::Deserialize;

#[derive(Deserialize)]
struct Regs(HashMap<String, u32>);

#[derive(Deserialize)]
struct State {
    regs: Regs,
    ram: Vec<(u32, u8)>,
}

#[derive(Deserialize)]
struct Test {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    end: State,
}

#[derive(Deserialize)]
struct Metadata {
    opcodes: HashMap<String, serde_json::Value>,
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

fn set_regs(cpu: &mut Cpu, r: &Regs) {
    for (k, v) in &r.0 {
        let v = *v;
        match k.as_str() {
            "ax" => cpu.set_r16(cpu::AX, v as u16),
            "bx" => cpu.set_r16(cpu::BX, v as u16),
            "cx" => cpu.set_r16(cpu::CX, v as u16),
            "dx" => cpu.set_r16(cpu::DX, v as u16),
            "sp" => cpu.set_r16(cpu::SP, v as u16),
            "bp" => cpu.set_r16(cpu::BP, v as u16),
            "si" => cpu.set_r16(cpu::SI, v as u16),
            "di" => cpu.set_r16(cpu::DI, v as u16),
            "cs" => cpu.sregs[cpu::CS] = v as u16,
            "ds" => cpu.sregs[cpu::DS] = v as u16,
            "es" => cpu.sregs[cpu::ES] = v as u16,
            "ss" => cpu.sregs[cpu::SS] = v as u16,
            "ip" => cpu.ip = v as u16,
            "flags" => cpu.flags = v,
            _ => {}
        }
    }
}

fn get_reg(cpu: &Cpu, k: &str) -> u32 {
    match k {
        "ax" => cpu.r16(cpu::AX) as u32,
        "bx" => cpu.r16(cpu::BX) as u32,
        "cx" => cpu.r16(cpu::CX) as u32,
        "dx" => cpu.r16(cpu::DX) as u32,
        "sp" => cpu.r16(cpu::SP) as u32,
        "bp" => cpu.r16(cpu::BP) as u32,
        "si" => cpu.r16(cpu::SI) as u32,
        "di" => cpu.r16(cpu::DI) as u32,
        "cs" => cpu.sregs[cpu::CS] as u32,
        "ds" => cpu.sregs[cpu::DS] as u32,
        "es" => cpu.sregs[cpu::ES] as u32,
        "ss" => cpu.sregs[cpu::SS] as u32,
        "ip" => cpu.ip as u32,
        "flags" => cpu.flags & 0xFFFF,
        _ => 0,
    }
}

/// The flags mask for a test name's opcode (and /reg form), from the metadata.
fn flags_mask(meta: &Metadata, op: &str, reg: Option<u8>) -> u32 {
    let entry = &meta.opcodes[op];
    let pick = |v: &serde_json::Value| v.get("flags-mask").and_then(|m| m.as_u64()).map(|m| m as u32);
    if let Some(r) = reg
        && let Some(sub) = entry.get("reg").and_then(|r2| r2.get(r.to_string()))
    {
        return pick(sub).unwrap_or(0xFFFF);
    }
    pick(entry).unwrap_or(0xFFFF)
}

#[test]
#[ignore]
fn sst8088() {
    let Ok(dir) = std::env::var("SST8088_DIR") else {
        eprintln!("SST8088_DIR not set; skipping");
        return;
    };
    let meta: Metadata = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/metadata.json")).unwrap()).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json") && p.file_stem().is_some_and(|s| s.len() == 2 || s.len() == 4)).collect();
    files.sort();
    let (mut total, mut failed) = (0usize, 0usize);
    let mut bad_ops: Vec<String> = Vec::new();
    for path in files {
        let op = path.file_stem().unwrap().to_string_lossy().to_uppercase()[..2].to_string();
        let tests: Vec<Test> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut fails_here = 0;
        let mut shown = 0;
        // One memory image per file, cleared where a test touched it.
        let mut mem = Memory::new();
        for t in &tests {
            total += 1;
            let mut cpu = Cpu::new();
            cpu.model_8086 = true;
            cpu.shift_mask = false;
            set_regs(&mut cpu, &t.initial.regs);
            for (a, b) in &t.initial.ram {
                mem.write8(*a, *b);
            }
            let mut bus = NoBus;
            // The group opcodes' /reg decides the flags mask.
            let reg = {
                let bytes: Vec<u8> = (0..4).map(|i| mem.read8((((cpu.sregs[cpu::CS] as u32) << 4) + cpu.ip as u32 + i) & 0xFFFFF)).collect();
                let mut i = 0;
                while i < 3 && matches!(bytes[i], 0x26 | 0x2E | 0x36 | 0x3E | 0xF0 | 0xF2 | 0xF3) {
                    i += 1;
                }
                matches!(bytes[i], 0x80..=0x83 | 0xC0 | 0xC1 | 0xD0..=0xD3 | 0xF6 | 0xF7 | 0xFE | 0xFF | 0x8F | 0xC6 | 0xC7).then(|| (bytes[i + 1] >> 3) & 7)
            };
            let mask = flags_mask(&meta, &op, reg);
            // Run to the end of this instruction (string ops iterate).
            let steps = cpu.step(&mut mem, &mut bus);
            let mut ok = steps.is_ok();
            let mut why = String::new();
            if ok {
                for (k, v) in &t.end.regs.0 {
                    let got = get_reg(&cpu, k);
                    let (got, want) = if k == "flags" { (got & mask, *v & mask) } else { (got, *v) };
                    if got != want {
                        ok = false;
                        why += &format!(" {k}: got {got:#x} want {want:#x}");
                    }
                }
                for (a, b) in &t.end.ram {
                    if mem.read8(*a) != *b {
                        ok = false;
                        why += &format!(" [{a:#x}]: got {:#x} want {:#x}", mem.read8(*a), b);
                    }
                }
            } else {
                why = format!(" {:?}", steps);
            }
            if !ok {
                failed += 1;
                fails_here += 1;
                if shown < 3 {
                    eprintln!("FAIL {op} {:?}:{why}", t.name);
                    shown += 1;
                }
            }
            for (a, _) in t.initial.ram.iter().chain(t.end.ram.iter()) {
                mem.write8(*a, 0);
            }
        }
        if fails_here > 0 {
            bad_ops.push(format!("{op}:{fails_here}/{}", tests.len()));
        }
    }
    eprintln!("{} tests, {} failed; opcodes with failures: {}", total, failed, bad_ops.join(" "));
    assert_eq!(failed, 0);
}
