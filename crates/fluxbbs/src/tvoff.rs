//! New: the picture switching off as an old set's did, for a logoff
//! (`HI`): whatever is on screen flickers, squashes to a bright line,
//! the line shrinks to a dot, and the dot fades.

use std::io;
use std::time::Duration;

use fluxbbs_term::Shown;

use crate::session::Session;

const COLS: usize = 80;
const WHITE: [u8; 3] = [235, 240, 255];

fn scale(c: [u8; 3], k: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * k).round().clamp(0.0, 255.0) as u8)
}

fn toward(c: [u8; 3], to: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (c[i] as f32 + (to[i] as f32 - c[i] as f32) * k).round() as u8)
}

/// A whole frame, a row at a time: colors only where they change, and
/// rows that are all black cleared rather than drawn.
fn paint(rows: &[Vec<Shown>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pen: Option<([u8; 3], [u8; 3])> = None;
    for (y, row) in rows.iter().enumerate() {
        out.extend(format!("\x1b[{};1H", y + 1).as_bytes());
        if row.iter().all(|c| c.bg == [0; 3] && (c.ch == b' ' || c.fg == [0; 3])) {
            if pen != Some(([0; 3], [0; 3])) {
                out.extend(b"\x1b[0;38;2;0;0;0;48;2;0;0;0m");
                pen = Some(([0; 3], [0; 3]));
            }
            out.extend(b"\x1b[2K");
            continue;
        }
        for (x, c) in row.iter().enumerate() {
            // The last column of the last row would scroll the screen.
            if y + 1 == rows.len() && x + 1 == COLS {
                break;
            }
            if pen != Some((c.fg, c.bg)) {
                out.extend(format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", c.fg[0], c.fg[1], c.fg[2], c.bg[0], c.bg[1], c.bg[2]).as_bytes());
                pen = Some((c.fg, c.bg));
            }
            out.push(c.ch);
        }
    }
    out
}

const BLACK: Shown = Shown { ch: b' ', fg: [0; 3], bg: [0; 3] };

impl Session {
    /// The screen switching off, before a logoff hangs up. Skipped for
    /// plain-text callers and those who turned animation off.
    pub(crate) async fn tv_off(&mut self) -> io::Result<()> {
        if self.plain() || self.user.animate == "off" {
            return Ok(());
        }
        let rows = self.screen.rows().clamp(2, 60) as usize;
        let screen: Vec<Vec<Shown>> =
            (1..=rows as u16).map(|y| (1..=COLS as u16).map(|x| self.screen.shown(x, y).unwrap_or(BLACK)).collect()).collect();
        self.screen.write_bytes(b"\x1b[?25l");

        // The flicker: the picture surging and dipping, a bright bar
        // rolling down it, for as long as the board says
        // (`logoff_flicker_ms`, 3.5 s to start).
        let flicker = Duration::from_millis(self.state.config().terminal.logoff_flicker_ms);
        let start = std::time::Instant::now();
        let mut i = 0usize;
        while start.elapsed() < flicker {
            let swing = [1.45f32, 0.4, 1.25, 0.65, 1.1, 0.85, 1.35, 0.5][i % 8];
            let k = swing * (0.85 + 0.3 * crate::pixels::hash(i as i32, 7));
            let bar = (i * 3) % rows;
            let frame: Vec<Vec<Shown>> = screen
                .iter()
                .enumerate()
                .map(|(y, row)| {
                    let k = if y.abs_diff(bar) <= 1 { k + 0.5 } else { k };
                    row.iter().map(|c| Shown { ch: c.ch, fg: scale(c.fg, k), bg: scale(c.bg, k) }).collect()
                })
                .collect();
            self.tv_frame(&paint(&frame), 70).await?;
            i += 1;
        }

        // Squashed into a band about the middle, going white as it goes.
        let mid = rows as f32 / 2.0;
        for h in [0.6f32, 0.36, 0.2, 0.1, 0.05] {
            let half = (mid * h).max(0.5);
            let white = 1.0 - h;
            let frame: Vec<Vec<Shown>> = (0..rows)
                .map(|y| {
                    let off = y as f32 + 0.5 - mid;
                    if off.abs() > half {
                        return vec![BLACK; COLS];
                    }
                    let src = ((mid + off / h).floor() as usize).min(rows - 1);
                    screen[src]
                        .iter()
                        .map(|c| Shown { ch: c.ch, fg: toward(c.fg, WHITE, white), bg: toward(scale(c.bg, 1.3), WHITE, white * 0.8) })
                        .collect()
                })
                .collect();
            self.tv_frame(&paint(&frame), 45).await?;
        }

        // The line, shrinking to a dot that lingers and fades.
        let y = rows / 2;
        let mut blank = vec![vec![BLACK; COLS]; rows];
        for w in [40usize, 30, 20, 12, 6, 3, 1] {
            blank[y] = (0..COLS).map(|x| if x.abs_diff(COLS / 2) < w { Shown { ch: 0xDF, fg: WHITE, bg: [0; 3] } } else { BLACK }).collect();
            self.tv_frame(&paint(&blank), 40).await?;
        }
        for (ch, k, ms) in [(0xFEu8, 1.0f32, 220u64), (0xF9, 0.75, 200), (0xFA, 0.45, 200), (0xFA, 0.2, 160)] {
            blank[y] = vec![BLACK; COLS];
            blank[y][COLS / 2] = Shown { ch, fg: scale(WHITE, k), bg: [0; 3] };
            self.tv_frame(&paint(&blank), ms).await?;
        }
        self.screen.write_bytes(b"\x1b[0m\x1b[2J\x1b[H\x1b[?25h");
        // A last word on the dark screen (`TvOffLater`, blank for none).
        let later = self.strings().get("TvOffLater").to_string();
        if !later.trim().is_empty() {
            let width = crate::editor::strip_codes(&later).chars().count() as u16;
            self.screen.goto((80u16.saturating_sub(width)) / 2 + 1, (rows as u16 / 2).max(1));
            self.show(&later).await?;
            self.flush().await?;
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        self.flush().await
    }

    async fn tv_frame(&mut self, frame: &[u8], ms: u64) -> io::Result<()> {
        self.screen.write_bytes(frame);
        // One frame on its way at a time, on a slow line.
        self.sync_output(Duration::from_secs(2), 1).await?;
        self.flush().await?;
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Ok(())
    }
}
