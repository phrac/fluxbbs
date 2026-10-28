//! New: the TheDraw font viewer (`*J`): every face in the fonts folder
//! (`[paths] fonts`), one at a time, setting the board's name, its short
//! name and its super short name, as the logo generators would. What's
//! wider than the screen is cut off, not wrapped. Outline faces (whose
//! glyphs are codes for a style, not characters) are left out.

use std::io;
use std::path::PathBuf;

use crate::input::ReadOpts;
use crate::pager::{Cell, cell};
use crate::session::Session;
use crate::tdf::Font;

/// The fonts folder's `.TDF` files, by name.
fn font_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("tdf"))).collect())
        .unwrap_or_default();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_uppercase()));
    files
}

/// A face's samples: each text set in it, under a dim label, cut at the
/// screen's edge.
fn samples(font: &Font, texts: &[(String, String)]) -> Vec<Vec<Cell>> {
    let mut grid: Vec<Vec<Cell>> = Vec::new();
    for (label, text) in texts {
        grid.push(label.bytes().map(|b| cell(b, 8, 0)).collect());
        let (cells, w, h) = font.set(text);
        if w == 0 {
            grid.push(b"(none of these letters in this face)".iter().map(|&b| cell(b, 8, 0)).collect());
        } else {
            let mut rows = vec![vec![cell(b' ', 7, 0); (w as usize).min(80)]; h as usize];
            for c in cells.iter().filter(|c| (c.x as usize) < 80) {
                rows[c.y as usize][c.x as usize] = cell(c.ch, c.fg, c.bg);
            }
            grid.extend(rows);
        }
        grid.push(Vec::new());
    }
    grid
}

impl Session {
    /// `*J`: page through the fonts, a face at a time: `n` and `p` the
    /// next and previous face, `f` finds one by its file or face name.
    pub async fn font_viewer(&mut self) -> io::Result<()> {
        let cfg = self.state.config();
        let files = font_files(&cfg.resolve(&cfg.paths.fonts));
        if files.is_empty() {
            let s = self.strings().fmt("FontViewNone", &[&cfg.paths.fonts.display().to_string()]);
            return self.show_line(&s).await;
        }
        // The board's names, as the logos set them.
        let texts: Vec<(String, String)> = [("name", cfg.bbs.name.clone()), ("short name", cfg.bbs.short_name.clone()), ("super short name", cfg.bbs.super_short())]
            .into_iter()
            .map(|(l, t)| (l.to_string(), t.trim().to_ascii_uppercase()))
            .filter(|(_, t)| !t.is_empty())
            .collect();
        let (mut file, mut face) = (0usize, 0usize);
        let mut faces: Vec<Font> = Vec::new();
        let mut loaded = usize::MAX;
        // Which way to step past a file with no faces we show.
        let mut step: isize = 1;
        loop {
            if loaded != file {
                faces = crate::tdf::read(&files[file]).into_iter().filter(|f| f.kind != 0).collect();
                loaded = file;
            }
            if faces.is_empty() {
                file = (file as isize + step).rem_euclid(files.len() as isize) as usize;
                face = if step < 0 { usize::MAX } else { 0 };
                continue;
            }
            face = face.min(faces.len() - 1);
            let name = files[file].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let title = format!("{name} · {} ({}/{}) · file {}/{}", faces[face].name, face + 1, faces.len(), file + 1, files.len());
            let grid = samples(&faces[face], &texts);
            match self.page_grid_keys(&title, &grid, "FontViewHelp", &['N', 'P', 'F']).await? {
                Some('N') => {
                    step = 1;
                    if face + 1 < faces.len() {
                        face += 1;
                    } else {
                        file = (file + 1) % files.len();
                        face = 0;
                    }
                }
                Some('P') => {
                    step = -1;
                    if face > 0 {
                        face -= 1;
                    } else {
                        file = (file + files.len() - 1) % files.len();
                        face = usize::MAX;
                    }
                }
                Some('F') => {
                    self.screen.goto(1, self.screen.rows());
                    self.screen.clear_eol();
                    self.say("FontViewFind").await?;
                    let want = self.read_string(ReadOpts::new(30)).await?.trim().to_ascii_lowercase();
                    if want.is_empty() {
                        continue;
                    }
                    // From the next file on, by file name, then by face name.
                    let order = (1..=files.len()).map(|k| (file + k) % files.len());
                    let by_file = order.clone().find(|&i| files[i].file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains(&want)));
                    let by_face = || {
                        order.clone().find_map(|i| {
                            crate::tdf::read(&files[i]).iter().filter(|f| f.kind != 0).position(|f| f.name.to_ascii_lowercase().contains(&want)).map(|j| (i, j))
                        })
                    };
                    if let Some((i, j)) = by_file.map(|i| (i, 0)).or_else(by_face) {
                        (file, face, step) = (i, j, 1);
                    }
                }
                _ => return Ok(()),
            }
        }
    }
}
