//! External programs: transfer protocols (`protocols.toml`, PROTOCOL.DAT)
//! and archivers (`archivers.toml`, ARCHIVER.DAT). ZIP and Zmodem are
//! built in; these add the rest.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::Protocol;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocols {
    #[serde(rename = "protocol", default)]
    pub list: Vec<Protocol>,
}

impl Protocols {
    pub fn to_toml(&self) -> String {
        format!(
            "# External transfer protocols (Zmodem is built in, on key Z). The\n\
             # program's stdin/stdout are the caller's connection. In `send`, %F is\n\
             # the files to send; `receive` runs in the directory uploads go to.\n\n{}",
            toml::to_string_pretty(self).expect("protocols serialize")
        )
    }
}

/// `tArchiverRec`: how to handle one kind of archive with outside tools.
/// Commands run through `sh -c` in a scratch directory; `%A` is the
/// archive and `%F` the file names inside it (both quoted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archiver {
    /// File extension, without the dot (`RAR`).
    pub ext: String,
    #[serde(default = "yes")]
    pub active: bool,
    /// Leading bytes that identify the format, in hex (`52617221`).
    /// Empty: go by the extension alone.
    #[serde(default)]
    pub signature: String,
    /// Prints the archive's contents (the archive viewer).
    #[serde(default)]
    pub list: String,
    /// Extracts %F from %A into the current directory.
    #[serde(default)]
    pub extract: String,
    /// Adds %F (in the current directory) to %A.
    #[serde(default)]
    pub add: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archivers {
    #[serde(rename = "archiver", default)]
    pub list: Vec<Archiver>,
}

impl Default for Archivers {
    /// bsdtar (libarchive) reads RAR, 7Z and LHA; adding a description
    /// back needs rar or 7z. ARJ needs 7-Zip. Missing tools just mean no
    /// description or listing for those files.
    fn default() -> Self {
        let a = |ext: &str, sig: &str, list: &str, extract: &str, add: &str| Archiver {
            ext: ext.into(),
            active: true,
            signature: sig.into(),
            list: list.into(),
            extract: extract.into(),
            add: add.into(),
        };
        Self {
            list: vec![
                a("RAR", "526172211A07", "bsdtar -tvf %A", "bsdtar -xf %A %F", "rar a -ep -inul %A %F"),
                a("7Z", "377ABCAF271C", "bsdtar -tvf %A", "bsdtar -xf %A %F", "7z a %A %F"),
                a("LZH", "", "bsdtar -tvf %A", "bsdtar -xf %A %F", ""),
                a("LHA", "", "bsdtar -tvf %A", "bsdtar -xf %A %F", ""),
                a("ARJ", "60EA", "7z l %A", "7z e -y %A %F", ""),
            ],
        }
    }
}

impl Archivers {
    pub fn to_toml(&self) -> String {
        format!(
            "# Archivers for formats other than ZIP (built in). Commands run through\n\
             # `sh -c` in a scratch directory: %A is the archive, %F the files inside\n\
             # it. `list` shows the contents, `extract` pulls out FILE_ID.DIZ and\n\
             # the like, `add` puts a description back.\n\n{}",
            toml::to_string_pretty(self).expect("archivers serialize")
        )
    }

    /// The archiver for a file: by signature where one is given, else by
    /// extension.
    pub fn for_file(&self, path: &Path) -> Option<&Archiver> {
        let mut head = [0u8; 16];
        let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_uppercase()).unwrap_or_default();
        let by_sig = self.list.iter().filter(|a| a.active && !a.signature.is_empty()).find(|a| {
            hex(&a.signature).is_some_and(|sig| !sig.is_empty() && head[..n].starts_with(&sig))
        });
        by_sig.or_else(|| self.list.iter().find(|a| a.active && a.ext.eq_ignore_ascii_case(&ext)))
    }
}

fn hex(s: &str) -> Option<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// Load a list file, or the default if it doesn't exist yet.
pub fn load<T: Default + serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Fill in `%A` and `%F` (quoted).
pub fn expand(cmd: &str, archive: &Path, files: &[&str]) -> String {
    let list = files.iter().map(|f| shell_quote(f)).collect::<Vec<_>>().join(" ");
    cmd.replace("%A", &shell_quote(&archive.to_string_lossy())).replace("%F", &list)
}

/// Run an archiver command in `dir`, giving up after a minute. Returns
/// whether it succeeded, and what it printed.
pub async fn run(cmd: &str, dir: &Path) -> std::io::Result<(bool, Vec<u8>)> {
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    match tokio::time::timeout(Duration::from_secs(60), child.wait_with_output()).await {
        Ok(out) => {
            let out = out?;
            Ok((out.status.success(), out.stdout))
        }
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "archiver took too long")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_signature_then_extension() {
        let dir = std::env::temp_dir().join(format!("iqarch{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rar = dir.join("misnamed.dat");
        std::fs::write(&rar, b"Rar!\x1a\x07\x00rest").unwrap();
        let lzh = dir.join("X.LZH");
        std::fs::write(&lzh, b"..-lh5-").unwrap();
        let a = Archivers::default();
        assert_eq!(a.for_file(&rar).unwrap().ext, "RAR");
        assert_eq!(a.for_file(&lzh).unwrap().ext, "LZH");
        assert!(a.for_file(&dir.join("none.txt")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commands_are_quoted() {
        let cmd = expand("7z e -y %A %F", Path::new("/f/it's.7z"), &["FILE_ID.DIZ", "DESC.SDI"]);
        assert_eq!(cmd, r"7z e -y '/f/it'\''s.7z' 'FILE_ID.DIZ' 'DESC.SDI'");
    }

    #[test]
    fn default_round_trips() {
        let a = Archivers::default();
        assert_eq!(toml::from_str::<Archivers>(&a.to_toml()).unwrap(), a);
    }
}
