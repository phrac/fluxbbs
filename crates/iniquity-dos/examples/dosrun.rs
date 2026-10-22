//! Run a DOS program or batch file in the machine, outside the BBS, for
//! trying doors: `dosrun C_ROOT D_ROOT 'START.BAT 1'`. Standard input is
//! the caller (raw bytes), standard output what the caller would see,
//! and the machine's log goes to standard error. `SCREEN=file` keeps the
//! local text screen written there, once a second; `DOS_TRACE=1` logs
//! every DOS call. `TRACE_RING=N` keeps the last N instructions' states
//! and writes and, if the program ends with an error, saves them to
//! trace.bin with the memory at the end in trace.mem, for `tools/`'s
//! replay against another emulator.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use iniquity_dos::batch::Batch;
use iniquity_dos::machine::{Machine, Status, TICK_INSTRUCTIONS};
use iniquity_dos::{Memory, VIDEO_BASE};

const TICK: Duration = Duration::from_micros(54_925);

fn screen_text(mem: &Memory) -> String {
    let mut s = String::new();
    for y in 0..25u32 {
        let line: String = (0..80u32).map(|x| mem.read8(VIDEO_BASE + (y * 80 + x) * 2)).map(|b| if (0x20..0x7f).contains(&b) { b as char } else if b == 0 { ' ' } else { '.' }).collect();
        s.push_str(line.trim_end());
        s.push('\n');
    }
    s
}

/// trace.bin: a count, then per record: native (1 byte), 8 registers,
/// 6 segment registers, IP, flags, a count of writes and the writes
/// (address, old, new); all little-endian.
fn save_trace(ring: &std::collections::VecDeque<iniquity_dos::cpu::Trace>, mem: &Memory) {
    use std::io::Write;
    let Ok(f) = std::fs::File::create("trace.bin") else { return };
    let mut out = std::io::BufWriter::with_capacity(1 << 20, f);
    let _ = out.write_all(&(ring.len() as u64).to_le_bytes());
    for t in ring {
        let _ = out.write_all(&[t.native as u8]);
        for r in t.regs {
            let _ = out.write_all(&r.to_le_bytes());
        }
        for s in t.sregs {
            let _ = out.write_all(&s.to_le_bytes());
        }
        let _ = out.write_all(&t.ip.to_le_bytes());
        let _ = out.write_all(&t.flags.to_le_bytes());
        let _ = out.write_all(&(t.writes.len() as u32).to_le_bytes());
        for (a, old, new) in &t.writes {
            let _ = out.write_all(&a.to_le_bytes());
            let _ = out.write_all(&[*old, *new]);
        }
    }
    let _ = out.flush();
    let _ = std::fs::write("trace.mem", (0..iniquity_dos::MEMORY_SIZE as u32).map(|a| mem.read8(a)).collect::<Vec<u8>>());
    eprintln!("{} instructions to trace.bin, memory to trace.mem", ring.len());
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, c, d, cmd] = &args[..] else { return Err("usage: dosrun C_ROOT D_ROOT COMMAND".into()) };
    let screen = std::env::var_os("SCREEN").map(PathBuf::from);
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        let mut stdin = std::io::stdin();
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut m = Machine::new(&PathBuf::from(c), Some(&PathBuf::from(d)));
    // `MEM_FILL=1`: free memory starts as a pseudo-random pattern, as on a
    // PC that has run other things, instead of zeros.
    if std::env::var_os("MEM_FILL").is_some() {
        let mut x: u32 = 0x1234_5678;
        for a in 0x1010..0xA0000u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            m.mem.write8(a, x as u8);
        }
    }
    m.sys.dos.set_drive(3);
    m.sys.trace = std::env::var_os("DOS_TRACE").is_some();
    let ring: usize = std::env::var("TRACE_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    m.cpu.keep_trace(&mut m.mem, ring);
    // `PROFILE_AT=secs`: that many seconds in, print where the CPU has
    // been (the ring's commonest addresses, with their bytes), for a
    // program that seems stuck.
    let profile_at: Option<f32> = std::env::var("PROFILE_AT").ok().and_then(|v| v.parse().ok());
    let mut profiled = false;
    let mut batch = Batch::new(&mut m, cmd)?;
    let started = Instant::now();
    let mut shown = Instant::now();
    let mut out = std::io::stdout();
    let mut send = |m: &mut Machine| {
        for l in m.sys.log.drain(..) {
            eprintln!("[{:.1}s] {l}", started.elapsed().as_secs_f32());
        }
        let o = m.take_output();
        let _ = out.write_all(&o);
        let _ = out.flush();
    };
    while let Some((prog, args)) = batch.next(&mut m)? {
        eprintln!("[{:.1}s] run {prog} {args}", started.elapsed().as_secs_f32());
        send(&mut m);
        m.start(&prog, &args)?;
        loop {
            while let Ok(b) = rx.try_recv() {
                m.feed(&b);
            }
            let st = m.run(TICK_INSTRUCTIONS / 8);
            send(&mut m);
            if let Some(at) = profile_at
                && !profiled
                && started.elapsed().as_secs_f32() >= at
                && let Some((ring, _)) = &m.cpu.ring
            {
                profiled = true;
                let mut counts: std::collections::HashMap<(u16, u16), usize> = std::collections::HashMap::new();
                for t in ring.iter().filter(|t| !t.native) {
                    *counts.entry((t.sregs[1], t.ip)).or_default() += 1;
                }
                let mut top: Vec<_> = counts.into_iter().collect();
                top.sort_by(|a, b| b.1.cmp(&a.1));
                eprintln!("profile at {at}s: {} instructions in the ring, status {st:?}, halted {}", ring.len(), m.cpu.halted);
                for ((cs, ip), n) in top.iter().take(12) {
                    let bytes: Vec<String> = (0..8).map(|i| format!("{:02x}", m.mem.read8(((*cs as u32) << 4) + *ip as u32 + i))).collect();
                    eprintln!("  {cs:04X}:{ip:04X} x{n}  {}", bytes.join(" "));
                }
            }
            if m.sys.hangup {
                eprintln!("door hung up");
                return Ok(());
            }
            if let Some(p) = &screen
                && shown.elapsed() > Duration::from_secs(1)
            {
                let _ = std::fs::write(p, screen_text(&m.mem));
                shown = Instant::now();
            }
            let wall = (started.elapsed().as_nanos() / TICK.as_nanos()) as u64;
            if m.ticks + 1 < wall {
                m.advance_ticks((wall - m.ticks).min(182) as u32);
            }
            match st {
                Status::Running => {
                    let due = started + TICK * m.ticks as u32;
                    let now = Instant::now();
                    if due > now {
                        std::thread::sleep(due - now);
                    }
                }
                Status::Idle => match rx.recv_timeout(TICK) {
                    Ok(b) => m.feed(&b),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => std::thread::sleep(TICK),
                },
                Status::Exited(c) => {
                    eprintln!("[{:.1}s] {prog} exited {c}", started.elapsed().as_secs_f32());
                    if c != 0 && let Some((ring, _)) = &m.cpu.ring {
                        save_trace(ring, &m.mem);
                    }
                    break;
                }
                Status::Fault(e) => {
                    if let Some(p) = &screen {
                        let _ = std::fs::write(p, screen_text(&m.mem));
                    }
                    return Err(format!("{prog}: {e} at {:04X}:{:04X}", m.cpu.cs(), m.cpu.ip));
                }
            }
        }
    }
    send(&mut m);
    if let Some(p) = &screen {
        let _ = std::fs::write(p, screen_text(&m.mem));
    }
    Ok(())
}
