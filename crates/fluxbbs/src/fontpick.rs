//! New: the screen saver's font picker (`*C`, SSH, Telnet and Terminal,
//! Saver font / Saver face). Every face in every TheDraw font file
//! (`.TDF`) under the board, in a list, with the saver's text set in the
//! one under the bar as a preview, so a SysOp can see a font before
//! choosing it rather than typing file and face names blind.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::input::Key;
use crate::session::Session;

/// Directories under the board never searched for fonts: the file base
/// and the board's own data are big and hold none.
const SKIP: [&str; 8] = ["files", "data", "backups", "doors", "temp", "text", ".git", "node_modules"];
const DEPTH: usize = 3;

/// A face to offer: its file (relative to the board) and its name.
#[derive(Clone, Debug, PartialEq)]
struct Face {
    file: PathBuf,
    name: String,
}

/// The `.TDF` files under `root` (not under `SKIP`, at most `DEPTH`
/// down), by path.
fn tdf_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    if depth < DEPTH && !SKIP.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                        walk(&p, depth + 1, out);
                    }
                }
                Ok(t) if t.is_file() && name.to_ascii_lowercase().ends_with(".tdf") => out.push(p),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out.sort();
    out
}

/// Every face in the board's font files, file by file.
fn faces(root: &Path) -> Vec<Face> {
    let mut out = Vec::new();
    for path in tdf_files(root) {
        let file = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        for f in crate::tdf::read(&path) {
            out.push(Face { file: file.clone(), name: f.name });
        }
    }
    out
}

/// What the saver spells: the message if there is one, else the name;
/// set in `font`, falling back to the short name when it's too wide for
/// 80 columns. Returns the set word and the text it shows.
fn preview_word(cfg: &Config, font: &crate::tdf::Font) -> ((Vec<crate::tdf::Cell>, u16, u16), String) {
    let msg = cfg.terminal.screensaver_message.trim();
    let text = if msg.is_empty() { cfg.bbs.name.clone() } else { msg.to_string() };
    let set = font.set(&text);
    let short = cfg.bbs.short_name.trim();
    if set.1 > 80 && !short.is_empty() {
        (font.set(short), short.to_string())
    } else {
        (set, text)
    }
}

/// The SGR for a DOS foreground (0-15) and background (0-7).
fn dos_sgr(fg: u8, bg: u8) -> String {
    const ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
    format!("\x1b[0;{}3{};4{}m", if fg > 7 { "1;" } else { "" }, ANSI[(fg & 7) as usize], ANSI[(bg & 7) as usize])
}

impl Session {
    /// Pick the saver's font and face from a list with a preview. Sets
    /// both in `cfg`; returns whether anything changed. Plain-text
    /// callers type them instead (the caller handles that).
    pub(crate) async fn saver_font_pick(&mut self, cfg: &mut Config) -> io::Result<bool> {
        let root = cfg.root.clone();
        let list = faces(&root);
        // The first row is "no font": the name in plain letters.
        let cur_file = cfg.terminal.screensaver_font.trim().to_string();
        let cur_face = cfg.terminal.screensaver_face.trim().to_string();
        let mut sel = if cur_file.is_empty() {
            0
        } else {
            list.iter()
                .position(|f| {
                    f.file == Path::new(&cur_file) && (cur_face.is_empty() || f.name.eq_ignore_ascii_case(&cur_face))
                })
                .map_or(0, |i| i + 1)
        };
        let rows = self.screen.rows().max(24);
        let list_rows: usize = 8;
        let mut top = 0usize;
        let total = list.len() + 1;
        // Faces read once each as the bar reaches them.
        let mut cache: std::collections::HashMap<usize, Option<crate::tdf::Font>> = std::collections::HashMap::new();
        loop {
            if sel < top {
                top = sel;
            } else if sel >= top + list_rows {
                top = sel + 1 - list_rows;
            }
            self.screen.set_color(fluxbbs_term::Color::DEFAULT);
            self.clear_screen();
            self.screen.write_bytes(b"\x1b[?25l");
            let mut out = String::new();
            out.push_str("\x1b[1;1H\x1b[0;1;37m Screen saver font\x1b[0;37m  -  arrows pick, enter sets it, esc leaves it as it was");
            for (r, i) in (top..(top + list_rows).min(total)).enumerate() {
                let label = match i {
                    0 => "(none)                 the name in plain letters".to_string(),
                    _ => {
                        let f = &list[i - 1];
                        format!("{:<22} {}", f.file.display().to_string(), f.name)
                    }
                };
                let label: String = label.chars().take(76).collect();
                let color = if i == sel { "\x1b[0;30;47m" } else { "\x1b[0;37m" };
                out.push_str(&format!("\x1b[{};2H{color} {label:<76} \x1b[0m", 2 + r));
            }
            if total > list_rows {
                out.push_str(&format!("\x1b[{};2H\x1b[0;1;30m {}-{} of {}", 2 + list_rows, top + 1, (top + list_rows).min(total), total));
            }
            self.screen.write_bytes(fluxbbs_term::cp437::encode(&out).as_slice());
            // The preview, under a rule.
            let rule_row = 3 + list_rows as u16;
            let prev_top = rule_row + 1;
            let prev_rows = rows.saturating_sub(prev_top + 1);
            let mut note = String::new();
            if sel == 0 {
                let msg = cfg.terminal.screensaver_message.trim();
                let text = if msg.is_empty() { cfg.bbs.name.clone() } else { msg.to_string() };
                let spaced = text.to_lowercase().split("").collect::<Vec<_>>().join(" ");
                let spaced = spaced.trim().to_string();
                let col = (80usize.saturating_sub(spaced.chars().count()) / 2 + 1) as u16;
                let mid = prev_top + prev_rows / 2;
                self.screen.write_bytes(format!("\x1b[{mid};{col}H\x1b[0;1;35m").as_bytes());
                self.screen.write_bytes(&fluxbbs_term::cp437::encode(&spaced));
                note.push_str("plain letters");
            } else {
                let i = sel - 1;
                let font = cache.entry(i).or_insert_with(|| {
                    let f = &list[i];
                    crate::tdf::read(&root.join(&f.file)).into_iter().find(|x| x.name == f.name)
                });
                match font {
                    Some(font) => {
                        let ((cells, w, h), shows) = preview_word(cfg, font);
                        let x0 = 80u16.saturating_sub(w) / 2 + 1;
                        let y0 = prev_top + prev_rows.saturating_sub(h) / 2;
                        let mut bytes = Vec::new();
                        let mut last = None;
                        for c in &cells {
                            let (x, y) = (x0 + c.x, y0 + c.y);
                            if x > 80 || y >= prev_top + prev_rows {
                                continue;
                            }
                            bytes.extend(format!("\x1b[{y};{x}H").as_bytes());
                            if last != Some((c.fg, c.bg)) {
                                bytes.extend(dos_sgr(c.fg, c.bg).as_bytes());
                                last = Some((c.fg, c.bg));
                            }
                            bytes.push(c.ch);
                        }
                        bytes.extend(b"\x1b[0m");
                        self.screen.write_bytes(&bytes);
                        note = format!("{w}x{h}, shows \"{shows}\"");
                        if w > 80 {
                            note.push_str(" - too wide, cut off (set a short name)");
                        }
                    }
                    None => note.push_str("can't read that face"),
                }
            }
            let rule = format!("\x1b[{rule_row};1H\x1b[0;1;30m{}\x1b[{rule_row};3H\x1b[0;37m preview: \x1b[0;1;37m{note} ", "\u{2500}".repeat(80));
            self.screen.write_bytes(&fluxbbs_term::cp437::encode(&rule));
            self.screen.write_bytes(format!("\x1b[{rows};1H\x1b[0m").as_bytes());
            self.flush().await?;
            let key = self.read_key().await?;
            match key {
                Key::Up => sel = sel.saturating_sub(1),
                Key::Down => sel = (sel + 1).min(total - 1),
                Key::PgUp => sel = sel.saturating_sub(list_rows),
                Key::PgDn => sel = (sel + list_rows).min(total - 1),
                Key::Home => sel = 0,
                Key::End => sel = total - 1,
                Key::Enter => {
                    let (file, face) = match sel {
                        0 => (String::new(), String::new()),
                        _ => {
                            let f = &list[sel - 1];
                            (f.file.display().to_string(), f.name.clone())
                        }
                    };
                    let changed = file != cfg.terminal.screensaver_font || face != cfg.terminal.screensaver_face;
                    cfg.terminal.screensaver_font = file;
                    cfg.terminal.screensaver_face = face;
                    self.finish_pick();
                    return Ok(changed);
                }
                Key::Esc => {
                    self.finish_pick();
                    return Ok(false);
                }
                k if k.upper() == Some('Q') => {
                    self.finish_pick();
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    fn finish_pick(&mut self) {
        self.screen.write_bytes(b"\x1b[0m\x1b[?25h");
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_fonts_but_not_in_the_file_base() {
        let root = std::env::temp_dir().join(format!("fluxbbs-fontpick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["art/fonts", "files/art", "fonts"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["art/fonts/ONE.TDF", "files/art/HIDDEN.TDF", "fonts/two.tdf", "art/fonts/notes.txt"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let got: Vec<String> = tdf_files(&root).iter().map(|p| p.strip_prefix(&root).unwrap().display().to_string()).collect();
        assert_eq!(got, ["art/fonts/ONE.TDF", "fonts/two.tdf"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
