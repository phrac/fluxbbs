//! New: links from messages to files on the board. A link is written
//! `file:AREA/NAME` (the file area's storage key and the file's name), so
//! it reads plainly anywhere the message goes (QWK, FTN). The editor's
//! `/L` picks a file from a lightbar and writes the link; `g` in the
//! reader lists a message's linked files (and its attachment) in a
//! lightbar to download.

use std::io;

use fluxbbs_term::codes::resize;

use crate::filebase::FileRec;
use crate::files::FAreaRef;
use crate::msgbase::Message;
use crate::session::{BarPick, BarRow, BarSpec, Session, waves};

/// Most matches the picker lists.
const PICK_MAX: usize = 300;

/// The links in a message's text, in order, each once: (area key, name).
pub fn file_links(lines: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in lines {
        for word in line.split_whitespace() {
            let Some(at) = word.to_ascii_lowercase().find("file:") else { continue };
            let rest = &word[at + 5..];
            let Some((key, name)) = rest.split_once('/') else { continue };
            // Punctuation after a link isn't part of it.
            let name = name.trim_end_matches(|c: char| ".,;:!?)]}>\"'".contains(c));
            if key.is_empty() || name.is_empty() || name.contains('/') {
                continue;
            }
            if !out.iter().any(|(k, n)| k.eq_ignore_ascii_case(key) && n.eq_ignore_ascii_case(name)) {
                out.push((key.to_string(), name.to_string()));
            }
        }
    }
    out
}

pub fn link_text(area_key: &str, name: &str) -> String {
    format!("file:{area_key}/{name}")
}

fn file_line(name: &str, size: &str, area: &str, desc: &str) -> String {
    format!("  {} {} {} {}", resize(name, 18), resize(size, 11), resize(area, 18), resize(desc, 27))
}

impl Session {
    /// A file area by storage key, if the caller may see into it (its ACS,
    /// and no password unless it's the area they're in).
    pub(crate) fn linked_farea(&self, key: &str) -> Option<FAreaRef> {
        let areas = self.state.fareas();
        let n = areas.areas.iter().position(|a| a.file.eq_ignore_ascii_case(key))? as u32 + 1;
        let a = self.farea(n)?;
        (self.acs(&a.area.acs) && (a.area.password.is_empty() || n == self.user.cur_file_area)).then_some(a)
    }

    /// A message's text and the comments under it, where links are.
    pub(crate) fn link_lines(&self, m: &Message) -> Vec<String> {
        let mut lines = m.lines.clone();
        lines.extend(self.state.msgs.pushes(m.id).unwrap_or_default().into_iter().filter(|p| p.kind == 0).map(|p| p.text));
        lines
    }

    /// The files a message (or a comment under it) links to that are here
    /// and the caller may see.
    pub(crate) fn linked_files(&self, m: &Message) -> Vec<(FAreaRef, FileRec)> {
        file_links(&self.link_lines(m))
            .into_iter()
            .filter_map(|(key, name)| {
                let a = self.linked_farea(&key)?;
                let f = self.state.files.find(&a.area.file, &name).ok().flatten()?;
                Some((a, f))
            })
            .collect()
    }

    /// `g` in the reader: the message's attachment and linked files in a
    /// lightbar (MFILES TOP/MID/HIL/BOT, else plain); Enter shows one and
    /// offers it for download.
    pub(crate) async fn message_files(&mut self, m: &Message, attached: bool) -> io::Result<()> {
        let files = self.linked_files(m);
        if files.is_empty() {
            if attached {
                return self.offer_attachment(m).await;
            }
            self.newline();
            self.say_line("MsgNoFiles").await?;
            return self.prompt_key().await;
        }
        let mut start = 0;
        loop {
            let mut rows = Vec::new();
            if attached {
                let label = self.strings().get("MsgFilesAttached").to_string();
                rows.push(BarRow { n: 0, waves: waves([("FN", label.clone())]), plain: format!("  {label}") });
            }
            for (i, (a, f)) in files.iter().enumerate() {
                let desc = f.description.first().cloned().unwrap_or_default();
                let w = waves([
                    ("FN", f.name.clone()),
                    ("SZ", crate::files::commas(f.size)),
                    ("AT", a.area.name.clone()),
                    ("DE", desc.clone()),
                ]);
                rows.push(BarRow { n: i as u32 + 1, waves: w, plain: file_line(&f.name, &crate::files::commas(f.size), &a.area.name, &desc) });
            }
            let header = file_line("File", "Size", "Area", "Description");
            let help = self.strings().get("MsgFilesKeys").to_string();
            let spec = BarSpec { prefix: "MFILES", header: &header, rows: &rows, labels: &[], start, help: &help, ask: "", can_go_up: true, keys: "" };
            match self.area_bar(&spec).await? {
                BarPick::Area(0) if attached => self.offer_attachment(m).await?,
                BarPick::Area(n) => {
                    start = rows.iter().position(|r| r.n == n).unwrap_or(0);
                    if let Some((a, f)) = files.get(n as usize - 1).cloned() {
                        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
                        self.clear_screen();
                        self.file_info(&f).await?;
                        self.download_record(&a, f).await?;
                        self.prompt_key().await?;
                    }
                }
                _ => return Ok(()),
            }
            if self.hangup {
                return Ok(());
            }
        }
    }

    /// The editor's `/L`: find files by name (wildcards, or any part of
    /// one) in the areas the caller can see, pick one from a lightbar, and
    /// give its link.
    pub(crate) async fn pick_file_link(&mut self) -> io::Result<Option<String>> {
        self.screen.set_color(fluxbbs_term::Color::DEFAULT);
        self.clear_screen();
        self.say("LinkAskFile").await?;
        let what = self.read_string(crate::input::ReadOpts::new(40)).await?;
        self.newline();
        let what = what.trim();
        if what.is_empty() {
            return Ok(None);
        }
        let spec = if what.contains(['*', '?']) { what.to_string() } else { format!("*{what}*") };
        let areas: Vec<FAreaRef> = (1..=self.state.fareas().areas.len() as u32)
            .filter_map(|n| self.farea(n))
            .filter(|a| self.linked_farea(&a.area.file).is_some())
            .collect();
        let keys: Vec<String> = areas.iter().map(|a| a.area.file.clone()).collect();
        let mut found = self.state.files.search(&keys, Some(&spec), None).map_err(|e| io::Error::other(e.to_string()))?;
        if found.is_empty() {
            self.say_line("LinkNone").await?;
            self.prompt_key().await?;
            return Ok(None);
        }
        found.truncate(PICK_MAX);
        let name_of = |key: &str| areas.iter().find(|a| a.area.file.eq_ignore_ascii_case(key)).map(|a| a.area.name.clone()).unwrap_or_default();
        let rows: Vec<BarRow> = found
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let area = name_of(&f.area);
                let desc = f.description.first().cloned().unwrap_or_default();
                let w = waves([("FN", f.name.clone()), ("SZ", crate::files::commas(f.size)), ("AT", area.clone()), ("DE", desc.clone())]);
                BarRow { n: i as u32, waves: w, plain: file_line(&f.name, &crate::files::commas(f.size), &area, &desc) }
            })
            .collect();
        let header = file_line("File", "Size", "Area", "Description");
        let help = self.strings().get("LinkKeys").to_string();
        let spec = BarSpec { prefix: "MFILES", header: &header, rows: &rows, labels: &[], start: 0, help: &help, ask: "", can_go_up: true, keys: "" };
        Ok(match self.area_bar(&spec).await? {
            BarPick::Area(i) => found.get(i as usize).map(|f| link_text(&f.area, &f.name)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_found_in_text() {
        let lines = vec![
            "got it: file:DOORS/lord407p.zip.".to_string(),
            "also (file:ART/acid-94.zip) and FILE:doors/LORD407P.ZIP again".to_string(),
            "not a link: file: or file:nokey or https://x/y".to_string(),
        ];
        assert_eq!(
            file_links(&lines),
            vec![("DOORS".to_string(), "lord407p.zip".to_string()), ("ART".to_string(), "acid-94.zip".to_string())]
        );
        assert_eq!(link_text("DOORS", "a.zip"), "file:DOORS/a.zip");
    }
}
