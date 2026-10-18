//! A scrolling viewer for uploaded text and ANSI files (new; the original
//! could only list archives). The file is drawn into an 80-column grid
//! the way a terminal would draw it, cursor moves and all, so ANSI art
//! that moves the cursor comes out right; the grid is then shown a
//! screenful at a time, scrolled with the cursor keys. Only colours and
//! cursor moves are read from the file: nothing in it reaches the
//! caller's terminal as a code of its own.

use std::io;

use iniquity_term::cp437;

use crate::input::Key;
use crate::session::Session;

const WIDTH: usize = 80;
/// Rows a file may draw on, so a stray cursor move can't run away.
const MAX_ROWS: usize = 10_000;

/// A cell's colour: DOS foreground (0-15), background (0-7), and whether
/// the background is bright (iCE, which `ESC[5m` means in DOS art).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Attr {
    fore: u8,
    back: u8,
    bright_back: bool,
}

const PLAIN: Attr = Attr { fore: 7, back: 0, bright_back: false };

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    /// A CP437 character.
    pub ch: u8,
    pub attr: Attr,
}

const BLANK: Cell = Cell { ch: b' ', attr: PLAIN };

/// ANSI colour digit for a DOS colour (and back: the map is its own
/// inverse).
fn ansi(dos: u8) -> u8 {
    [0, 4, 2, 6, 1, 5, 3, 7][(dos & 7) as usize]
}

/// Draw a file into rows of cells. `utf8`: the text is UTF-8, not CP437.
pub fn render(text: &[u8], utf8: bool) -> Vec<Vec<Cell>> {
    let chars: Vec<u8> = if utf8 { cp437::encode(&String::from_utf8_lossy(text)) } else { text.to_vec() };
    let mut grid: Vec<Vec<Cell>> = vec![Vec::new()];
    let (mut x, mut y, mut saved) = (0usize, 0usize, (0usize, 0usize));
    let mut attr = PLAIN;
    // Bright background from the DOS blink bit, which a background colour
    // change keeps, or from aixterm's 100-107, which it ends.
    let (mut blink, mut aix_bright) = (false, false);
    let mut i = 0;
    let row = |grid: &mut Vec<Vec<Cell>>, y: usize| {
        while grid.len() <= y {
            grid.push(Vec::new());
        }
    };
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            b'\r' => x = 0,
            b'\n' => {
                x = 0;
                y = (y + 1).min(MAX_ROWS - 1);
            }
            b'\t' => x = ((x / 8 + 1) * 8).min(WIDTH - 1),
            0x1b if chars.get(i) == Some(&b'[') => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && (0x20..=0x3f).contains(&chars[j]) {
                    j += 1;
                }
                let Some(&fin) = chars.get(j) else { break };
                i = j + 1;
                let raw = &chars[start..j];
                if raw.iter().any(|c| !c.is_ascii_digit() && *c != b';') {
                    continue;
                }
                let params: Vec<usize> = std::str::from_utf8(raw).unwrap_or("").split(';').map(|p| p.parse().unwrap_or(0)).collect();
                let n = params.first().copied().unwrap_or(0).max(1);
                match fin {
                    b'A' => y = y.saturating_sub(n),
                    b'B' => y = (y + n).min(MAX_ROWS - 1),
                    b'C' => x = (x + n).min(WIDTH - 1),
                    b'D' => x = x.saturating_sub(n),
                    b'H' | b'f' => {
                        y = (params.first().copied().unwrap_or(1).max(1) - 1).min(MAX_ROWS - 1);
                        x = (params.get(1).copied().unwrap_or(1).max(1) - 1).min(WIDTH - 1);
                    }
                    b'J' if params.first() == Some(&2) => {
                        grid = vec![Vec::new()];
                        (x, y) = (0, 0);
                    }
                    b'K' => {
                        row(&mut grid, y);
                        grid[y].truncate(x);
                    }
                    b's' => saved = (x, y),
                    b'u' => (x, y) = saved,
                    b'm' => {
                        for &p in if raw.is_empty() { &[0][..] } else { &params[..] } {
                            match p {
                                0 => {
                                    attr = PLAIN;
                                    (blink, aix_bright) = (false, false);
                                }
                                1 => attr.fore |= 8,
                                22 => attr.fore &= 7,
                                5 | 6 => blink = true,
                                25 => blink = false,
                                30..=37 => attr.fore = (attr.fore & 8) | ansi(p as u8 - 30),
                                90..=97 => attr.fore = 8 | ansi(p as u8 - 90),
                                39 => attr.fore = (attr.fore & 8) | 7,
                                40..=47 => {
                                    attr.back = ansi(p as u8 - 40);
                                    aix_bright = false;
                                }
                                100..=107 => {
                                    attr.back = ansi(p as u8 - 100);
                                    aix_bright = true;
                                }
                                49 => attr.back = 0,
                                _ => {}
                            }
                        }
                        attr.bright_back = blink || aix_bright;
                    }
                    _ => {}
                }
            }
            c if c < 0x20 && c != 0 => {}
            c => {
                if x >= WIDTH {
                    x = 0;
                    y = (y + 1).min(MAX_ROWS - 1);
                }
                row(&mut grid, y);
                let line = &mut grid[y];
                while line.len() <= x {
                    line.push(BLANK);
                }
                // NUL is a blank cell in DOS art.
                line[x] = Cell { ch: if c == 0 { b' ' } else { c }, attr };
                x += 1;
            }
        }
    }
    while grid.len() > 1 && grid.last().is_some_and(|r| r.iter().all(|c| c.ch == b' ' && c.attr.back == 0 && !c.attr.bright_back)) {
        grid.pop();
    }
    grid
}

/// One row as bytes for `Screen::write_bytes`: CP437 characters and
/// plain colour codes, bright backgrounds as `10x` so the screen turns
/// them into whatever the caller's terminal shows without blinking.
fn row_bytes(cells: &[Cell]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cur: Option<Attr> = None;
    for c in cells {
        if cur != Some(c.attr) {
            let a = c.attr;
            let back = if a.bright_back { 100 } else { 40 } + ansi(a.back) as u16;
            out.extend_from_slice(format!("\x1b[0;{}3{};{back}m", if a.fore >= 8 { "1;" } else { "" }, ansi(a.fore)).as_bytes());
            cur = Some(a);
        }
        out.push(c.ch);
    }
    out.extend_from_slice(b"\x1b[0m");
    out
}

impl Session {
    /// Show `grid` under a title line, with a status line at the bottom,
    /// until the caller quits.
    pub(crate) async fn page_grid(&mut self, title: &str, grid: &[Vec<Cell>]) -> io::Result<()> {
        self.page_grid_keys(title, grid, "ViewerHelp", &[]).await.map(|_| ())
    }

    /// The same, with `extra` keys that also end it (returned, upper
    /// case) and `help` naming the status line's string.
    pub(crate) async fn page_grid_keys(&mut self, title: &str, grid: &[Vec<Cell>], help: &str, extra: &[char]) -> io::Result<Option<char>> {
        let rows = self.screen.rows().max(5) as usize;
        let height = rows - 2;
        let total = grid.len();
        let last_top = total.saturating_sub(height);
        let mut top = 0usize;
        let mut drawn: Option<usize> = None;
        loop {
            if drawn != Some(top) {
                self.clear_screen();
                let range = format!("{}-{} of {}", top + 1, (top + height).min(total), total);
                // One line: a long name and SAUCE title mustn't wrap.
                let title: String = title.chars().take(38).collect();
                let s = self.strings().fmt("ViewerTitle", &[&title, &range]);
                self.show(&s).await?;
                for (k, line) in grid.iter().skip(top).take(height).enumerate() {
                    self.screen.goto(1, 2 + k as u16);
                    self.screen.write_bytes(&row_bytes(line));
                }
                self.screen.goto(1, rows as u16);
                self.screen.set_color(iniquity_term::Color::DEFAULT);
                let help = self.strings().get(help).to_string();
                self.show(&help).await?;
                drawn = Some(top);
            }
            let key = self.read_key().await?;
            if let Some(c) = key.upper().filter(|c| extra.contains(c)) {
                self.screen.set_color(iniquity_term::Color::DEFAULT);
                self.clear_screen();
                return Ok(Some(c));
            }
            top = match key {
                Key::Up => top.saturating_sub(1),
                Key::Down | Key::Enter => (top + 1).min(last_top),
                Key::PgUp => top.saturating_sub(height),
                Key::PgDn => (top + height).min(last_top),
                Key::Home => 0,
                Key::End => last_top,
                Key::Esc => break,
                k => match k.upper() {
                    Some(' ' | 'N') => (top + height).min(last_top),
                    Some('B' | 'P') => top.saturating_sub(height),
                    Some('Q' | 'X') => break,
                    _ => top,
                },
            };
        }
        self.screen.set_color(iniquity_term::Color::DEFAULT);
        self.clear_screen();
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(grid: &[Vec<Cell>]) -> Vec<String> {
        grid.iter().map(|r| r.iter().map(|c| c.ch as char).collect::<String>().trim_end().to_string()).collect()
    }

    #[test]
    fn draws_like_a_terminal() {
        // Plain lines, a cursor move back up, a column jump, a NUL cell.
        let g = render(b"one\r\ntwo\r\nthree\x1b[2A\x1b[10Cup\x1b[2B\r\n\x00x\r\n", false);
        assert_eq!(text(&g).len(), 4);
        assert_eq!(text(&g)[0], "one            up");
        assert_eq!(text(&g)[2], "three");
        assert_eq!(text(&g)[3], " x");
    }

    #[test]
    fn colours_and_wrapping() {
        let g = render(b"\x1b[1;33;44mA\x1b[5;42mB\x1b[0m", false);
        assert_eq!(g[0][0].attr, Attr { fore: 14, back: 1, bright_back: false });
        assert_eq!(g[0][1].attr, Attr { fore: 14, back: 2, bright_back: true });
        let long = vec![b'x'; 85];
        assert_eq!(render(&long, false)[1].len(), 5);
        // A title-setting sequence draws nothing and isn't passed on.
        let g = render(b"\x1b]0;evil\x07ok", false);
        assert!(text(&g)[0].ends_with("ok"));
        assert!(!row_bytes(&g[0]).contains(&0x07));
    }

    #[test]
    fn utf8_text_maps_to_cp437() {
        let g = render("caf\u{e9} \u{2591}".as_bytes(), true);
        assert_eq!(g[0][3].ch, 0x82);
        assert_eq!(g[0][5].ch, 0xb0);
    }
}
