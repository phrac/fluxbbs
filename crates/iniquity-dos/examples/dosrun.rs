//! Run a DOS program or batch file in the machine, outside the BBS, for
//! trying doors: `dosrun C_ROOT D_ROOT 'START.BAT 1'`. Standard input is
//! the caller (raw bytes), standard output what the caller would see,
//! and the machine's log goes to standard error. `SCREEN=file` keeps the
//! local text screen written there, once a second; `DOS_TRACE=1` logs
//! every DOS call.

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
    m.sys.dos.set_drive(3);
    m.sys.trace = std::env::var_os("DOS_TRACE").is_some();
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
