//! Newscan selections: `ME` and `FE` (2.0) pick which message and file
//! areas newscans look in, and `US` puts every area back for everyone.
//! 2.0's source isn't available; this follows its manual and WHATSNEW.

use std::collections::HashSet;
use std::io;

use iniquity_term::codes::{resize, squish};

use crate::messages::io_err;
use crate::session::{BarPick, BarRow, BarSpec, Session, waves};

impl Session {
    /// Areas (by file name) the caller has taken out of newscans.
    pub(crate) fn scan_off(&self, kind: char) -> HashSet<String> {
        self.state.sys.scan_off(self.user.id, kind).unwrap_or_default()
    }

    /// `ME` (`msg`) / `FE`: a bar of the caller's areas; enter turns one
    /// in or out of newscans, A puts them all in, N takes them all out.
    /// MSCANTOP/MID/HIL/BOT or FSCAN... dress it (`~AN` number, `~AT`
    /// name, `~SC` the mark).
    pub async fn scan_select(&mut self, msg: bool) -> io::Result<()> {
        let kind = if msg { 'M' } else { 'F' };
        let areas: Vec<(u32, u32, String, String)> = if msg {
            self.msg_area_labels().into_iter().filter_map(|(l, n)| self.msg_area(n).filter(|a| self.acs(&a.area.acs)).map(|a| (l, n, a.area.name, a.area.file))).collect()
        } else {
            self.file_area_labels().into_iter().filter_map(|(l, n)| self.farea(n).map(|a| (l, n, a.area.name, a.area.file))).collect()
        };
        if areas.is_empty() {
            return Ok(());
        }
        let labels: Vec<(u32, u32)> = areas.iter().map(|a| (a.0, a.1)).collect();
        let help = self.strings().get("ScanBarKeys").to_string();
        let mut start = 0;
        loop {
            let off = self.scan_off(kind);
            let rows: Vec<BarRow> = areas
                .iter()
                .map(|(label, n, name, file)| {
                    let mark = if off.contains(file) { " " } else { "■" };
                    let waves = waves([("AN", label.to_string()), ("AT", name.clone()), ("SC", mark.to_string())]);
                    let plain = format!(" {} {}  {}", resize(&label.to_string(), 6), mark, squish(name, 60));
                    BarRow { n: *n, waves, plain }
                })
                .collect();
            let spec = BarSpec {
                prefix: if msg { "MSCAN" } else { "FSCAN" },
                header: " Num    Scan  Area Title",
                rows: &rows,
                labels: &labels,
                start,
                help: &help,
                ask: "",
                can_go_up: false,
                keys: "AN",
            };
            match self.area_bar(&spec).await? {
                BarPick::Area(n) => {
                    if let Some((_, _, _, file)) = areas.iter().find(|a| a.1 == n) {
                        self.state.sys.set_scan(self.user.id, kind, file, off.contains(file)).map_err(io_err)?;
                    }
                    start = areas.iter().position(|a| a.1 == n).unwrap_or(0);
                }
                BarPick::Key(c, n) => {
                    for (_, _, _, file) in &areas {
                        self.state.sys.set_scan(self.user.id, kind, file, c == 'A').map_err(io_err)?;
                    }
                    start = areas.iter().position(|a| a.1 == n).unwrap_or(0);
                }
                BarPick::Up | BarPick::Stay | BarPick::Quit => return Ok(()),
            }
        }
    }

    /// `US`: every message and file area back in every user's newscans.
    pub async fn scan_all_on(&mut self) -> io::Result<()> {
        let n = self.state.sys.scan_all_on().map_err(io_err)?;
        tracing::info!("{}: put every area back in everyone's newscans ({n} cleared)", self.user.handle);
        self.newline();
        self.say_line("ScanAllOn").await
    }
}
