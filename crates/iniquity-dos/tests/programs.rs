//! Real programs (assembled from `tests/dos/*.asm`) run in a Machine.

use std::path::{Path, PathBuf};

use iniquity_dos::machine::{Machine, Status};

fn programs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/dos")
}

/// Run until the program exits or needs input, collecting output.
fn run(m: &mut Machine, out: &mut Vec<u8>) -> Status {
    loop {
        let st = m.run(1_000_000);
        out.extend(m.take_output());
        if st != Status::Running {
            return st;
        }
    }
}

fn text(out: &[u8]) -> String {
    // Strip the ANSI the screen mirror adds.
    let mut s = String::new();
    let mut esc = false;
    for &b in out {
        if b == 0x1B {
            esc = true;
        } else if esc {
            if b.is_ascii_alphabetic() {
                esc = false;
            }
        } else {
            s.push(b as char);
        }
    }
    s
}

#[test]
fn hello_prints_and_exits() {
    let mut m = Machine::new(&programs(), None);
    m.start("hello", "").unwrap();
    let mut out = Vec::new();
    assert_eq!(run(&mut m, &mut out), Status::Exited(7));
    assert!(text(&out).contains("Hello, DOS!"), "{:?}", text(&out));
}

#[test]
fn fossil_door_echoes_the_caller() {
    let mut m = Machine::new(&programs(), None);
    m.start("ECHO.COM", "").unwrap();
    let mut out = Vec::new();
    assert_eq!(run(&mut m, &mut out), Status::Idle, "waiting for the caller");
    assert!(m.sys.remote, "the FOSSIL was opened");
    assert_eq!(out, b"READY\r\n");
    m.feed(b"hi there");
    out.clear();
    assert_eq!(run(&mut m, &mut out), Status::Idle);
    assert_eq!(out, b"HI THERE");
    m.feed(b"q");
    assert_eq!(run(&mut m, &mut out), Status::Exited(0));
    assert!(m.sys.hangup, "DTR was lowered");
}

#[test]
fn files_on_drive_c() {
    let dir = std::env::temp_dir().join(format!("iqdos-files{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(programs().join("files.com"), dir.join("FILES.COM")).unwrap();
    let mut m = Machine::new(&dir, None);
    m.start("files", "").unwrap();
    let mut out = Vec::new();
    let st = run(&mut m, &mut out);
    assert_eq!(st, Status::Exited(0), "log: {:?}", m.sys.log);
    let shown = text(&out);
    assert!(shown.contains("Hi") && shown.contains("ok"), "{shown:?}");
    assert!(!dir.join("out.txt").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ticks_advance_and_keys_arrive() {
    let mut m = Machine::new(&programs(), None);
    m.start("ticks", "").unwrap();
    let mut out = Vec::new();
    // Two ticks' worth of instructions pass inside the budget.
    assert_eq!(run(&mut m, &mut out), Status::Idle, "waiting for a key");
    assert!(!m.sys.remote, "a local program: the caller is the keyboard");
    m.feed(b"Z");
    assert_eq!(run(&mut m, &mut out), Status::Exited(0));
    assert!(text(&out).ends_with('Z'), "{:?}", text(&out));
}

#[test]
fn exe_loads_with_relocations() {
    // A tiny EXE built by hand: one segment, `mov ax, seg; mov ds, ax;
    // mov ah, 9; mov dx, msg; int 21h; mov ax, 4C03h; int 21h`, with the
    // `seg` immediate relocated.
    let code: Vec<u8> = vec![
        0xB8, 0x00, 0x00, // mov ax, 0000 (relocated: +load segment)
        0x8E, 0xD8, // mov ds, ax
        0xB4, 0x09, // mov ah, 9
        0xBA, 0x0F, 0x00, // mov dx, msg
        0xCD, 0x21, // int 21h
        0xB8, 0x03, 0x4C, // mov ax, 4C03h (at 0x0C)
        0xCD, 0x21, // int 21h  -- wait, msg must be after; fix offsets below
    ];
    let mut body = code;
    // Put the message after the code: adjust mov dx.
    let msg_off = body.len() as u16;
    body[8] = msg_off as u8;
    body[9] = (msg_off >> 8) as u8;
    body.extend(b"EXE ok$");
    let mut header = vec![0u8; 32];
    header[0] = b'M';
    header[1] = b'Z';
    let total = 32 + body.len();
    header[2..4].copy_from_slice(&((total % 512) as u16).to_le_bytes());
    header[4..6].copy_from_slice(&(total.div_ceil(512) as u16).to_le_bytes());
    header[6..8].copy_from_slice(&1u16.to_le_bytes()); // one relocation
    header[8..10].copy_from_slice(&2u16.to_le_bytes()); // header paragraphs
    header[0x0A..0x0C].copy_from_slice(&0x10u16.to_le_bytes()); // min alloc
    header[0x0C..0x0E].copy_from_slice(&0xFFFFu16.to_le_bytes());
    header[0x0E..0x10].copy_from_slice(&0u16.to_le_bytes()); // SS
    header[0x10..0x12].copy_from_slice(&0x200u16.to_le_bytes()); // SP
    header[0x14..0x16].copy_from_slice(&0u16.to_le_bytes()); // IP
    header[0x16..0x18].copy_from_slice(&0u16.to_le_bytes()); // CS
    header[0x18..0x1A].copy_from_slice(&0x1Cu16.to_le_bytes()); // reloc table
    header[0x1C..0x1E].copy_from_slice(&1u16.to_le_bytes()); // reloc: offset 1
    header[0x1E..0x20].copy_from_slice(&0u16.to_le_bytes()); // segment 0
    let dir = std::env::temp_dir().join(format!("iqdos-exe{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("T.EXE"), [header, body].concat()).unwrap();
    let mut m = Machine::new(&dir, None);
    m.start("C:\\T.EXE", "").unwrap();
    let mut out = Vec::new();
    assert_eq!(run(&mut m, &mut out), Status::Exited(3), "log: {:?}", m.sys.log);
    assert!(text(&out).contains("EXE ok"), "{:?}", text(&out));
    std::fs::remove_dir_all(&dir).unwrap();
}
