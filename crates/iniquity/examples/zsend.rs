//! A Zmodem sender for live tests, standing in for `sz` where lrzsz
//! isn't installed: `zsend FILE...` sends over stdin/stdout with the same
//! zmodem2 machine the BBS uses. With the test drivers:
//! `'(?i)begin your transfer=>!target/release/examples/zsend f.zip'`.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use zmodem2::{Action, Event, FileInfo, Position};

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let (wire_tx, wire_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut stdin = std::io::stdin();
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || wire_tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut out = std::io::stdout();
    let mut tx = zmodem2::Sender::new().expect("sender");
    tx.set_streaming_window(usize::MAX);
    let (mut cur, mut start_next, mut finishing, mut ended, mut idle) = (0, true, false, false, 0);
    let mut file: Option<std::fs::File> = None;
    let mut pending = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        if start_next && !finishing {
            if let Some(path) = files.get(cur) {
                let size = std::fs::metadata(path).expect("file").len() as u32;
                let name = Path::new(path).file_name().unwrap().to_string_lossy().into_owned();
                match tx.start_file(FileInfo::new(name.as_bytes(), Some(Position::new(size)))) {
                    Ok(()) => {
                        file = Some(std::fs::File::open(path).expect("open"));
                        start_next = false;
                    }
                    Err(zmodem2::Error::Backpressure) => {}
                    Err(e) => panic!("{e:?}"),
                }
            } else {
                finishing = true;
            }
        }
        if finishing && start_next {
            match tx.finish() {
                Ok(()) => start_next = false,
                Err(zmodem2::Error::Backpressure) => {}
                Err(e) => panic!("{e:?}"),
            }
        }
        match tx.poll() {
            Action::WriteWire(bytes) => {
                let n = bytes.len();
                out.write_all(bytes).unwrap();
                out.flush().unwrap();
                tx.wire_written(n);
            }
            Action::ReadFile { offset, max_len } => {
                let f = file.as_mut().expect("open file");
                f.seek(SeekFrom::Start(offset.get() as u64)).unwrap();
                let n = f.read(&mut buf[..max_len.min(8192)]).unwrap();
                tx.submit_file(&buf[..n]).unwrap();
            }
            Action::Event(Event::FileCompleted) => {
                cur += 1;
                file = None;
                start_next = true;
            }
            Action::Event(Event::SessionCompleted | Event::Aborted) => ended = true,
            Action::Idle if ended => break,
            Action::Idle => {
                if !pending.is_empty() {
                    let used = tx.submit_wire(&pending).unwrap();
                    pending.drain(..used.min(pending.len()));
                    if used > 0 {
                        continue;
                    }
                }
                match wire_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(b) => {
                        idle = 0;
                        pending.extend(b);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) if idle < 12 => {
                        idle += 1;
                        tx.timeout().unwrap();
                    }
                    Err(_) => std::process::exit(1),
                }
            }
            _ => {}
        }
    }
}
