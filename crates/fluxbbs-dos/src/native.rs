//! New: native programs. A batch file may name a program the host runs
//! in place of a DOS one: a LORD in-game module's `DO1.BAT` names the
//! module, and the board runs a Python script instead (its `[dos]
//! native`). The script gets the caller on its standard input and output
//! (raw bytes, as a DOS door sees them through the FOSSIL), starts in the
//! host folder of the machine's current directory, and its exit code is
//! the errorlevel the batch file sees next. The original had nothing like
//! it; DOS doors couldn't run anything but DOS programs.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a native program gets to notice the caller has gone (its
/// input ends) before it's stopped.
const GRACE: Duration = Duration::from_secs(5);
/// How much of what it said on standard error is kept, for the log.
const STDERR_KEEP: usize = 2000;

/// What the caller's side gave, asked for with a wait.
pub enum Recv {
    Data(Vec<u8>),
    /// Nothing in the time.
    Idle,
    /// The caller has gone.
    Gone,
}

/// How a native program ended.
pub struct Ended {
    pub code: u8,
    /// The caller went while it ran.
    pub hangup: bool,
    /// The end of what it said on standard error.
    pub stderr: String,
}

/// Run `program` (a `.py` file through `python`, else the file itself)
/// with `args` split on spaces, in `cwd`, with `env` added. `recv` waits
/// up to its time for the caller; `send` takes output for the caller.
pub fn run(program: &Path, args: &str, cwd: &Path, python: &str, env: &[(String, String)], mut recv: impl FnMut(Duration) -> Recv, mut send: impl FnMut(Vec<u8>)) -> Result<Ended, String> {
    let py = program.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
    let mut cmd = if py {
        let mut c = Command::new(python);
        // Its own folder on the path, and PYTHONPATH (the board's helpers),
        // but not the user's site packages.
        c.arg("-s").arg(program);
        c
    } else {
        Command::new(program)
    };
    cmd.args(args.split_whitespace())
        .current_dir(cwd)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", program.display()))?;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 || out_tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let errs = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 1024];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
            if all.len() > STDERR_KEEP * 2 {
                all.drain(..all.len() - STDERR_KEEP);
            }
        }
        let s = String::from_utf8_lossy(&all).into_owned();
        let skip = s.chars().count().saturating_sub(STDERR_KEEP);
        s.chars().skip(skip).collect::<String>()
    });
    let mut lost: Option<Instant> = None;
    let status = loop {
        while let Ok(b) = out_rx.try_recv() {
            send(b);
        }
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if lost.is_some_and(|t| t.elapsed() > GRACE) {
            let _ = child.kill();
            break child.wait().map_err(|e| e.to_string())?;
        }
        match recv(Duration::from_millis(20)) {
            Recv::Data(b) => {
                // One that stopped reading is ending anyway.
                if let Some(i) = stdin.as_mut()
                    && (i.write_all(&b).is_err() || i.flush().is_err())
                {
                    stdin = None;
                }
            }
            Recv::Idle => {}
            Recv::Gone => {
                if lost.is_none() {
                    // Its input ends: it sees the caller has gone.
                    lost = Some(Instant::now());
                    stdin = None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    let _ = reader.join();
    while let Ok(b) = out_rx.try_recv() {
        send(b);
    }
    let stderr = errs.join().unwrap_or_default();
    let code = status.code().map_or(255, |c| c.clamp(0, 255) as u8);
    Ok(Ended { code, hangup: lost.is_some(), stderr })
}

/// The name a batch file's program is known by for `[dos] native`: its
/// file name without the folder or extension, in capitals
/// (`C:\IGM\TERMINAL.EXE` is `TERMINAL`).
pub fn program_name(prog: &str) -> String {
    let base = prog.rsplit(['\\', '/', ':']).next().unwrap_or(prog);
    base.split('.').next().unwrap_or(base).to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(program_name("C:\\IGM\\terminal.exe"), "TERMINAL");
        assert_eq!(program_name("TERMINAL"), "TERMINAL");
        assert_eq!(program_name("c:terminal.bat"), "TERMINAL");
    }

    #[test]
    fn runs_with_the_caller() {
        let dir = std::env::temp_dir().join(format!("native-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("echo.sh");
        std::fs::write(&script, "#!/bin/sh\nread x\necho \"got $x in $(basename \"$PWD\")\"\nexit 7\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut fed = false;
        let mut out = Vec::new();
        let end = run(&script, "", &dir, "python3", &[], |_| if fed { Recv::Idle } else { fed = true; Recv::Data(b"hi\n".to_vec()) }, |b| out.extend(b)).unwrap();
        assert_eq!(end.code, 7);
        assert!(!end.hangup);
        assert!(String::from_utf8_lossy(&out).starts_with("got hi in native-test-"));
        // The caller goes: its input ends.
        let end = run(&script, "", &dir, "python3", &[], |_| Recv::Gone, |_| {}).unwrap();
        assert!(end.hangup);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
