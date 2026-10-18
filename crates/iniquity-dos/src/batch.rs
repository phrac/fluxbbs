//! Batch files: enough of COMMAND.COM to run the `.BAT` wrappers doors
//! come with (`@echo off`, `cd`, drive changes, `%1`, `if exist`,
//! `if errorlevel`, `goto`, `call`, `echo`). The interpreter runs the
//! built-in commands itself and hands programs back to the host, which
//! runs each in the machine before asking for the next.

use crate::machine::Machine;
use crate::video::Screen;

struct Frame {
    lines: Vec<String>,
    pc: usize,
    args: Vec<String>,
}

/// A command line being worked through: a program with its arguments,
/// or a batch file (and the ones it calls).
pub struct Batch {
    frames: Vec<Frame>,
    echo: bool,
}

/// Split a command line into the program and the rest.
fn split_command(line: &str) -> (String, String) {
    let line = line.trim();
    match line.find(char::is_whitespace) {
        Some(i) => (line[..i].to_string(), line[i..].trim().to_string()),
        None => (line.to_string(), String::new()),
    }
}

impl Batch {
    /// `cmd` as typed on the door's command line: `LORD.BAT 1`, or a
    /// program with arguments. The batch file is read through DOS, so it
    /// is found like any program.
    pub fn new(m: &mut Machine, cmd: &str) -> Result<Self, String> {
        let mut b = Self { frames: Vec::new(), echo: true };
        b.push(m, cmd)?;
        Ok(b)
    }

    /// Enter `cmd`: a batch file becomes a frame; a program a one-line one.
    fn push(&mut self, m: &mut Machine, cmd: &str) -> Result<(), String> {
        let (prog, rest) = split_command(cmd);
        let args: Vec<String> = rest.split_whitespace().map(String::from).collect();
        if prog.to_ascii_uppercase().ends_with(".BAT") {
            // Found like a program: the current directory, then C:\.
            let (host, _) = m.sys.dos.find_program(&prog).ok_or_else(|| format!("{prog}: not found"))?;
            let text = std::fs::read(&host).map_err(|e| format!("{}: {e}", host.display()))?;
            let text = String::from_utf8_lossy(&text).into_owned();
            let lines = text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
            self.frames.push(Frame { lines, pc: 0, args });
        } else if !prog.is_empty() {
            self.frames.push(Frame { lines: vec![cmd.trim().to_string()], pc: 0, args: Vec::new() });
        }
        Ok(())
    }

    /// `%1`..`%9`, `%0` and `%%`.
    fn expand(line: &str, args: &[String], name: &str) -> String {
        let mut out = String::new();
        let mut it = line.chars().peekable();
        while let Some(c) = it.next() {
            if c == '%' {
                match it.peek().copied() {
                    Some('%') => {
                        it.next();
                        out.push('%');
                    }
                    Some(d) if d.is_ascii_digit() => {
                        it.next();
                        let n = d as usize - '0' as usize;
                        if n == 0 {
                            out.push_str(name);
                        } else if let Some(a) = args.get(n - 1) {
                            out.push_str(a);
                        }
                    }
                    _ => out.push('%'),
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn goto(frame: &mut Frame, label: &str) {
        let want = label.trim().trim_start_matches(':').to_ascii_uppercase();
        frame.pc = frame
            .lines
            .iter()
            .position(|l| {
                let l = l.trim();
                l.starts_with(':') && l[1..].split_whitespace().next().is_some_and(|x| x.eq_ignore_ascii_case(&want))
            })
            .map_or(frame.lines.len(), |i| i + 1);
    }

    /// Run built-in commands until a program needs running (returned as
    /// `program`, `args`) or the batch is finished.
    pub fn next(&mut self, m: &mut Machine) -> Result<Option<(String, String)>, String> {
        loop {
            let Some(frame) = self.frames.last_mut() else { return Ok(None) };
            let Some(raw) = frame.lines.get(frame.pc).cloned() else {
                self.frames.pop();
                continue;
            };
            frame.pc += 1;
            let line = Self::expand(&raw, &frame.args, "");
            let mut line = line.trim().to_string();
            if line.starts_with('@') {
                line = line[1..].trim().to_string();
            } else if self.echo && !line.is_empty() && !line.starts_with(':') {
                Self::say(m, &format!("\r\nC:\\>{line}\r\n"));
            }
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(run) = self.command(m, &line)? {
                return Ok(Some(run));
            }
        }
    }

    /// One line: `Some` means the host runs it as a program.
    fn command(&mut self, m: &mut Machine, line: &str) -> Result<Option<(String, String)>, String> {
        let (word, rest) = split_command(line);
        let up = word.to_ascii_uppercase();
        // `cd\lord` and `echo.` run the word into its argument.
        let (up, rest) = if let Some(r) = up.strip_prefix("CD").filter(|r| r.starts_with('\\') || r.starts_with('.')) {
            ("CD".to_string(), format!("{r}{}", if rest.is_empty() { "" } else { " " }) + &rest)
        } else if up.starts_with("ECHO.") {
            ("ECHO".to_string(), word[5..].to_string() + if rest.is_empty() { "" } else { " " } + &rest)
        } else {
            (up, rest)
        };
        match up.as_str() {
            "REM" | "PAUSE" | "SET" | "PATH" | "PROMPT" | "BREAK" | "VERIFY" | "CTTY" | "MODE" => {}
            "ECHO" => match rest.to_ascii_uppercase().as_str() {
                "OFF" => self.echo = false,
                "ON" => self.echo = true,
                "" => Self::say(m, if self.echo { "ECHO is on\r\n" } else { "ECHO is off\r\n" }),
                _ => Self::say(m, &format!("{rest}\r\n")),
            },
            "CLS" => Screen::clear(&mut m.mem),
            "CD" | "CHDIR" => {
                if !rest.is_empty() && !m.sys.dos.change_dir(&rest) {
                    Self::say(m, "Invalid directory\r\n");
                }
            }
            "GOTO" => {
                if let Some(f) = self.frames.last_mut() {
                    Self::goto(f, &rest);
                }
            }
            "EXIT" => self.frames.clear(),
            "CALL" => self.push(m, &rest)?,
            "IF" => {
                let (negate, cond) = match rest.split_once(char::is_whitespace) {
                    Some((n, c)) if n.eq_ignore_ascii_case("not") => (true, c.trim().to_string()),
                    _ => (false, rest.clone()),
                };
                let (test, then) = split_command(&cond);
                let (truth, then) = match test.to_ascii_uppercase().as_str() {
                    "EXIST" => {
                        let (file, then) = split_command(&then);
                        (m.sys.dos.exists(&file), then)
                    }
                    "ERRORLEVEL" => {
                        let (n, then) = split_command(&then);
                        (m.sys.dos.errorlevel() >= n.parse().unwrap_or(0), then)
                    }
                    _ => match cond.split_once("==") {
                        Some((a, b)) => {
                            let (b, then) = split_command(b);
                            (a.trim() == b, then)
                        }
                        None => (false, String::new()),
                    },
                };
                if truth != negate && !then.is_empty() {
                    return self.command(m, &then);
                }
            }
            _ if up.len() == 2 && up.ends_with(':') => {
                let d = (up.as_bytes()[0] - b'A') as usize;
                if !m.sys.dos.set_drive(d) {
                    Self::say(m, "Invalid drive specification\r\n");
                }
            }
            _ => {
                if m.sys.dos.find_program(&word).is_none() {
                    Self::say(m, "Bad command or file name\r\n");
                    return Ok(None);
                }
                if up.ends_with(".BAT") {
                    // Running a batch file without CALL replaces this one.
                    self.frames.pop();
                    self.push(m, line)?;
                    return Ok(None);
                }
                return Ok(Some((word, rest)));
            }
        }
        Ok(None)
    }

    fn say(m: &mut Machine, s: &str) {
        for b in s.bytes() {
            Screen::teletype(&mut m.mem, b, 0x07);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn machine(files: &[(&str, &str)]) -> (Machine, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("iqbat{}-{}", std::process::id(), files.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("LORD")).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        std::fs::write(dir.join("LORD/LORD.EXE"), b"MZ").unwrap();
        std::fs::write(dir.join("LORD/START.COM"), b"\xC3").unwrap();
        (Machine::new(&dir, None), dir)
    }

    #[test]
    fn runs_the_builtins_and_hands_back_programs() {
        let bat = "@echo off\r\ncd \\lord\r\nif exist lord.exe goto run\r\necho missing\r\nexit\r\n:run\r\nlord /N%1\r\ncd\\\r\nif errorlevel 1 start\r\n";
        let (mut m, dir) = machine(&[("LORD.BAT", bat)]);
        let mut b = Batch::new(&mut m, "lord.bat 3").unwrap();
        assert_eq!(b.next(&mut m).unwrap(), Some(("lord".into(), "/N3".into())));
        assert_eq!(m.sys.dos.cwd_dos(2), "LORD");
        assert_eq!(b.next(&mut m).unwrap(), None, "errorlevel is 0 so START isn't run");
        assert_eq!(m.sys.dos.cwd_dos(2), "");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn call_and_plain_programs() {
        let (mut m, dir) = machine(&[("A.BAT", "call b.bat x\r\nlord\\start.com\r\n"), ("B.BAT", "echo in %1\r\nlord\\lord.exe %1\r\n")]);
        let mut b = Batch::new(&mut m, "A.BAT").unwrap();
        assert_eq!(b.next(&mut m).unwrap(), Some(("lord\\lord.exe".into(), "x".into())));
        assert_eq!(b.next(&mut m).unwrap(), Some(("lord\\start.com".into(), "".into())));
        assert_eq!(b.next(&mut m).unwrap(), None);
        let mut b = Batch::new(&mut m, "LORD\\LORD.EXE /N1 /P1").unwrap();
        assert_eq!(b.next(&mut m).unwrap(), Some(("LORD\\LORD.EXE".into(), "/N1 /P1".into())));
        assert_eq!(b.next(&mut m).unwrap(), None);
        assert!(Batch::new(&mut m, "NOPE.BAT").is_err());
        let _ = Path::new(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
