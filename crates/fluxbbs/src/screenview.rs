//! New: the screen viewer (`*W`). Every screen the board can show, with
//! its random variants and weekday overrides and where each comes from,
//! and any one of them shown as a caller sees it, effects and all. For a
//! SysOp checking art without logging on again and again for the variant
//! they want.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use fluxbbs_term::codes::resize;
use fluxbbs_term::{Control, Slot, Source, Token, tokenize};

use crate::input::ReadOpts;
use crate::session::{Session, Waves};

const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// What a screen file's extension means: `.ANS` or `.ASC` (the screen),
/// `.AN1`-`.AN9` (a random variant) or `.A0S`-`.A6S` (a weekday).
#[derive(Debug, PartialEq)]
enum Kind {
    Screen,
    Random(u8),
    Weekday(u8),
}

fn kind(ext: &str) -> Option<Kind> {
    let e = ext.to_ascii_uppercase();
    let b = e.as_bytes();
    match b {
        b"ANS" | b"ASC" => Some(Kind::Screen),
        [b'A', b'N', d @ b'1'..=b'9'] => Some(Kind::Random(d - b'0')),
        [b'A', d @ b'0'..=b'6', b'S'] => Some(Kind::Weekday(d - b'0')),
        _ => None,
    }
}

/// The screens in the directories, by name: the random variants and
/// weekdays each has, and the index of the directory it's in. The first
/// directory with a screen's files is the one the board shows it from,
/// as `find_text_file` looks.
fn scan(dirs: &[PathBuf]) -> BTreeMap<String, (Vec<Kind>, usize)> {
    let mut out: BTreeMap<String, (Vec<Kind>, usize)> = BTreeMap::new();
    for (i, dir) in dirs.iter().enumerate() {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for name in rd.filter_map(Result::ok).filter_map(|e| e.file_name().into_string().ok()) {
            let Some((stem, ext)) = name.split_once('.') else { continue };
            let Some(k) = kind(ext) else { continue };
            if stem.is_empty() || ext.contains('.') {
                continue;
            }
            let e = out.entry(stem.to_ascii_uppercase()).or_insert_with(|| (Vec::new(), i));
            if e.1 == i && !e.0.contains(&k) {
                e.0.push(k);
            }
        }
    }
    out
}

/// A screen's variants in a few characters: `+3` random ones, the
/// weekdays it changes on.
fn tag(kinds: &[Kind]) -> String {
    let n = kinds.iter().filter(|k| matches!(k, Kind::Random(_))).count();
    let mut days: Vec<u8> = kinds.iter().filter_map(|k| if let Kind::Weekday(d) = k { Some(*d) } else { None }).collect();
    days.sort_unstable();
    let mut s = String::new();
    if n > 0 {
        s.push_str(&format!("+{n}"));
    }
    for d in days {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(DAYS[d as usize]);
    }
    s
}

impl Session {
    /// `*W`: list the screens, then show the ones asked for until Enter
    /// alone. A bare name shows what a caller would get (a random
    /// variant, today's weekday); `NAME.AN2` shows that file.
    pub async fn screen_viewer(&mut self) -> io::Result<()> {
        if self.plain() {
            return self.show_line("|U4Screens need ANSI|U5.").await;
        }
        self.screen_list().await?;
        loop {
            self.newline();
            self.show("|U1Show which |U5[|U4name|U5, |U4name.AN2|U5, |U4?|U5=|U4list|U5, |U4enter|U5=|U4done|U5]: |U3").await?;
            let name = self.read_string(ReadOpts::new(12)).await?;
            self.newline();
            let name = name.trim().to_string();
            if name.is_empty() {
                return Ok(());
            }
            if name == "?" {
                self.screen_list().await?;
                continue;
            }
            let Some(path) = self.find_text_file(&name) else {
                self.show_line("|U4No such screen|U5.").await?;
                continue;
            };
            let data = tokio::fs::read(&path).await?;
            let tokens = tokenize(&data, Source::File);
            let waits = tokens.iter().any(|t| matches!(t, Token::Ctrl(Control::Pause | Control::PromptKey | Control::ReadKey)));
            // New: art taller than the caller's screen (an ad, say) scrolls
            // in the pager; screens with the board's codes show as they are.
            let art = tokens.iter().all(|t| matches!(t, Token::Text(_)));
            let shown = &data[..data.iter().position(|&b| b == 0x1a).unwrap_or(data.len())];
            let grid = if art { crate::pager::trimmed(crate::pager::render(shown, false)) } else { Vec::new() };
            if art && grid.len() >= self.screen.rows() as usize {
                let title = path.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_string();
                self.page_grid(&title, &grid).await?;
            } else {
                self.show_path(&path, &Waves::new()).await?;
                if !waits {
                    self.prompt_key().await?;
                }
            }
            self.clear_screen();
            let file = path.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_string();
            let dirs = self.text_dirs();
            let from = match dirs.iter().position(|d| path.parent() == Some(d.as_path())) {
                Some(0) if self.theme_dir().is_some() => "their theme",
                Some(i) if i < self.own_dirs(&dirs) => "the board's own",
                _ => "stock",
            };
            self.show_line(&format!("|U1That was |U3{file}|U1, {from}")).await?;
        }
    }

    /// How many of the screen directories are the board's own (the rest
    /// are stock).
    fn own_dirs(&self, dirs: &[PathBuf]) -> usize {
        let cfg = self.state.config();
        let stock = usize::from(!cfg.paths.stock_text.as_os_str().is_empty()) + usize::from(!cfg.paths.stock_theme.as_os_str().is_empty());
        dirs.len() - stock
    }

    /// The screens by name, three to a row, the board's own lit and the
    /// stock ones dim, with each one's variants.
    async fn screen_list(&mut self) -> io::Result<()> {
        let dirs = self.text_dirs();
        let names = scan(&dirs);
        let own = self.own_dirs(&dirs);
        self.clear_screen();
        self.set_slot(Slot::Info);
        self.show_line("|U4 Screens  |U3the board's own|U1, stock, |U3name |U2+n|U1 random variants, |U2day|U1 weekday ones").await?;
        self.rule().await?;
        let mut lines = 2u16;
        let mut col = 0;
        for (name, (kinds, i)) in &names {
            let lit = *i < own;
            let cell = format!("{}{} |U2{}", if lit { "|U3" } else { "|U1" }, resize(name, 8), resize(&tag(kinds), 11));
            self.show(&cell).await?;
            col += 1;
            if col == 3 {
                col = 0;
                self.newline();
                lines += 1;
                if !self.more(&mut lines).await? {
                    return Ok(());
                }
            } else {
                self.show(" |U1").await?;
            }
        }
        if col != 0 {
            self.newline();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_screens_with_their_variants() {
        let dir = std::env::temp_dir().join(format!("fluxbbs-screenview-{}", std::process::id()));
        let (own, stock) = (dir.join("text"), dir.join("stock"));
        std::fs::create_dir_all(&own).unwrap();
        std::fs::create_dir_all(&stock).unwrap();
        for f in ["WELCOME.ANS", "WELCOME.AN1", "WELCOME.AN2", "welcome.a3s", "WELCOME.FX", "MAIN.ANS", "notes.txt"] {
            std::fs::write(own.join(f), b"x").unwrap();
        }
        for f in ["MAIN.ANS", "MAIN.AN1", "LOGOFF.ASC", "LOGOFF.A0S"] {
            std::fs::write(stock.join(f), b"x").unwrap();
        }
        let names = scan(&[own, stock]);
        assert_eq!(names.keys().cloned().collect::<Vec<_>>(), ["LOGOFF", "MAIN", "WELCOME"]);
        assert_eq!(names["WELCOME"].1, 0);
        assert_eq!(tag(&names["WELCOME"].0), "+2 wed");
        assert_eq!((names["MAIN"].1, tag(&names["MAIN"].0).as_str()), (0, ""), "the board's MAIN hides stock's, variants and all");
        assert_eq!((names["LOGOFF"].1, tag(&names["LOGOFF"].0).as_str()), (1, "sun"));
        assert_eq!(kind("FX"), None);
        assert_eq!(kind("an9"), Some(Kind::Random(9)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
