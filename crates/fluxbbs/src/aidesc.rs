//! New: descriptions for files that came without one. A file whose
//! description is empty, or only its own name (a piece of art whose SAUCE
//! title is its file name, say), is shown to the board's AI with what the
//! board can tell about it: its name, size and area, its SAUCE, the start
//! of its text, what a zip holds (and the start of its read-me or other
//! text), and a picture, for a model that sees: an image as it is, a
//! piece of ANSI art drawn (`ansishot`), a PDF's first page, the art or
//! a picture in a zip. The few words it answers become the
//! description. Uploads are done in the background as they come in, one at
//! a time; `fluxbbs describe` does the files already there.

use std::path::Path;
use std::sync::Arc;

use crate::BbsState;
use crate::filebase::FileRec;

/// The most the AI's words may run to; the file lists show about this much.
#[cfg_attr(not(feature = "ai"), allow(dead_code))]
pub const MAX_CHARS: usize = 56;

/// A file's name squeezed to its letters and digits, lowercased.
fn squeeze(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect()
}

/// The board's own lines in a description: its summary (`[ text, 59
/// lines ]`), a picture's (`JPEG image, 1024x1536`) and a SAUCE date
/// standing alone.
fn is_board_line(l: &str) -> bool {
    let l = l.trim();
    is_kept_line(l) || (l.len() == 10 && l.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() }))
}

/// The board's lines that stay under the AI's words: its summary and a
/// picture's size.
fn is_kept_line(l: &str) -> bool {
    let l = l.trim();
    let picture = l.split_once(" image, ").is_some_and(|(fmt, rest)| {
        !fmt.is_empty() && !fmt.contains(' ') && rest.split(',').next().is_some_and(|wh| wh.split_once('x').is_some_and(|(w, h)| w.parse::<u32>().is_ok() && h.parse::<u32>().is_ok()))
    });
    picture || (l.starts_with('[') && l.ends_with(']'))
}

/// A title some tool left that means nothing: one long run of letters and
/// digits, mixed case, no spaces (`QFHHKng1ReWgb4xV8vvDYNSaBhThkOZh`).
fn is_hash(s: &str) -> bool {
    let s = s.trim();
    s.len() >= 16
        && s.chars().all(|c| c.is_ascii_alphanumeric())
        && s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars().any(|c| c.is_ascii_lowercase())
        && s.chars().any(|c| c.is_ascii_digit())
}

/// Whether a record has no description worth the name: once the board's
/// own lines are set aside, nothing, or only the file's name (with or
/// without its extension), or a meaningless hash.
pub fn is_bare(f: &FileRec) -> bool {
    let words: Vec<&str> = f.description.iter().map(String::as_str).filter(|l| !is_board_line(l)).collect();
    // Joined with spaces, several lines are never a hash.
    let text = words.join(" ");
    says_nothing(&text, &f.name) || is_listing(&text)
}

/// The board's own stand-in for a ZIP without a FILE_ID.DIZ
/// (`files::zip_contents`): "2 files: HYPERP.DOC HYPERP.EXE".
fn is_listing(text: &str) -> bool {
    let mut words = text.split_whitespace();
    let (Some(n), Some(what)) = (words.next().and_then(|n| n.parse::<usize>().ok()), words.next()) else { return false };
    matches!(what, "file:" | "files:") && words.count() <= n
}

/// Whether a line of text (a description, a SAUCE title) says nothing
/// about the file `name`: empty, a hash, or only its name.
pub fn says_nothing(text: &str, name: &str) -> bool {
    let d = squeeze(text);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    d.is_empty() || is_hash(text) || d == squeeze(name) || d == squeeze(stem)
}

/// The AI's words as the description, over the board's own summary line.
#[cfg_attr(not(feature = "ai"), allow(dead_code))]
pub fn with_summary(f: &FileRec, words: String) -> Vec<String> {
    let summary = f.description.iter().filter(|l| is_kept_line(l)).cloned();
    std::iter::once(words).chain(summary).collect()
}

/// What the board can tell the AI about a file.
#[cfg_attr(not(feature = "ai"), allow(dead_code))]
pub fn evidence(path: &Path, f: &FileRec, area: &str) -> String {
    let mut out = vec![format!("File name: {}", f.name), format!("Size: {} bytes", f.size), format!("File area: {area}")];
    let data = std::fs::read(path).unwrap_or_default();
    if let Some(s) = crate::gallery::parse_sauce(&data) {
        let mut parts = Vec::new();
        for (k, v) in [("title", &s.title), ("author", &s.author), ("group", &s.group), ("date", &s.date)] {
            if !v.is_empty() {
                parts.push(format!("{k} {v}"));
            }
        }
        if !parts.is_empty() {
            out.push(format!("SAUCE record: {}", parts.join(", ")));
        }
    }
    if crate::files::is_zip(path) {
        let names = crate::files::zip_contents(path);
        if !names.is_empty() {
            out.push(format!("Zip contents: {}", names.join(" ")));
        }
        // New: and the start of the file in it that best says what it holds.
        if let Some((name, data)) = zip_entry(path, text_rank, 16 * 1024)
            && let Some(words) = words_of(&data)
        {
            out.push(format!("The start of {name}, inside it: {words}"));
        }
    } else if is_pdf(&f.name) {
        // Its words, as the PDF viewer reads them (Poppler); the raw file is
        // compressed streams.
        #[cfg(feature = "pdf")]
        if let Ok(t) = crate::pdf::text(path) {
            let words = t.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(format!("The start of its text: {}", words.chars().take(1500).collect::<String>()));
        }
    } else if crate::picture::is_image(&f.name) {
        out.push("It is a picture: see it attached.".into());
    } else if let Some(words) = words_of(&data[..data.len().min(16 * 1024)]) {
        out.push(format!("The start of its text: {words}"));
    }
    out.join("\n")
}

/// The words in some text, up to 1,500 characters: escapes and drawing
/// characters dropped, so a piece of ANSI art gives up only the text
/// written in it. None if there are hardly any.
fn words_of(data: &[u8]) -> Option<String> {
    let text = fluxbbs_term::cp437::decode(crate::upscan::text_part(data));
    let text = strip_escapes(&text);
    let words: String = text
        .chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (words.chars().filter(char::is_ascii_alphabetic).count() >= 12).then(|| words.chars().take(1500).collect())
}

/// The file in a zip that `rank` likes best (lowest first; None passes
/// one over), and up to `cap` bytes of it.
fn zip_entry(path: &Path, rank: fn(&str) -> Option<u8>, cap: u64) -> Option<(String, Vec<u8>)> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let (_, i, name) = (0..z.len())
        .filter_map(|i| {
            let name = z.name_for_index(i)?.ok()?.to_string();
            let base = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_string();
            Some((rank(&base)?, i, base))
        })
        .min_by_key(|(r, i, _)| (*r, *i))?;
    let mut data = Vec::new();
    z.by_index(i).ok()?.take(cap).read_to_end(&mut data).ok()?;
    Some((name, data))
}

/// Which file in a zip to read for what it holds: its description, a
/// read-me, then documents and other text.
fn text_rank(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    let ext = n.rsplit_once('.').map_or("", |(_, e)| e);
    Some(match () {
        _ if n == "file_id.diz" || n == "desc.sdi" => 0,
        _ if n.starts_with("readme") || n.starts_with("read.me") || n.starts_with("read_me") || ext == "1st" => 1,
        _ if matches!(ext, "nfo" | "diz") => 2,
        _ if matches!(ext, "doc" | "txt" | "me" | "man" | "hlp" | "asc") => 3,
        _ => return None,
    })
}

/// Which file in a zip to show the AI: a piece of art, else a picture,
/// else an NFO (often drawn).
#[cfg(feature = "ai")]
fn picture_rank(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    let ext = n.rsplit_once('.').map_or("", |(_, e)| e);
    Some(match ext {
        "ans" | "ice" => 0,
        _ if crate::picture::is_image(&n) => 1,
        "nfo" => 2,
        _ => return None,
    })
}

#[cfg_attr(not(feature = "ai"), allow(dead_code))]
fn is_pdf(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pdf")
}

#[cfg_attr(not(feature = "ai"), allow(dead_code))]
fn strip_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if ('@'..='~').contains(&d) {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The AI's answer as a description: one line, no quotes or closing stop,
/// cut at a word to `MAX_CHARS` (with a little give).
#[cfg_attr(not(feature = "ai"), allow(dead_code))]
pub fn tidy(answer: &str) -> String {
    let line = answer.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let line = line.trim_start_matches(['-', '*', ' ']).trim_matches(['"', '\'', '`', ' ']);
    let line = line.strip_prefix("Description:").unwrap_or(line).trim().trim_end_matches('.');
    if line.chars().count() <= MAX_CHARS + 8 {
        return line.to_string();
    }
    let mut cut = String::new();
    for w in line.split_whitespace() {
        if cut.chars().count() + 1 + w.chars().count() > MAX_CHARS {
            break;
        }
        if !cut.is_empty() {
            cut.push(' ');
        }
        cut.push_str(w);
    }
    cut
}

#[cfg(feature = "ai")]
const SYSTEM: &str = "You write the descriptions in a bulletin board system's file listings. You're told what's known about one file, \
and sometimes shown it (a picture, a piece of ANSI art drawn as a picture, or a document's first page); \
answer with only its description: 3 to 8 words, under 50 characters, saying what the file is: for a picture or ANSI art, what it shows \
(say it's ANSI art if it is); for a document, what it's about; for a program or archive, what it does or holds. \
No quotes, no closing period, no file name unless it means something. If little is known, describe the kind of file plainly.";

/// What the AI can look at for a file, as a PNG: a picture as it is, a
/// piece of ANSI art drawn, a PDF's first page; scaled to a size a model
/// takes in comfortably.
#[cfg(feature = "ai")]
fn picture(path: &Path, f: &FileRec, temp: &Path) -> Option<Vec<u8>> {
    let lower = f.name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let img = if crate::picture::is_image(&f.name) {
        image::load_from_memory(&std::fs::read(path).ok()?).ok()?
    } else if matches!(ext, "ans" | "ice" | "asc" | "diz" | "nfo") {
        let png = crate::ansishot::png(&std::fs::read(path).ok()?)?;
        image::load_from_memory(&png).ok()?
    } else if ext == "zip" {
        // New: a piece of art or a picture from inside it.
        let (name, data) = zip_entry(path, picture_rank, 4 * 1024 * 1024)?;
        if crate::picture::is_image(&name) {
            image::load_from_memory(&data).ok()?
        } else {
            image::load_from_memory(&crate::ansishot::png(&data)?).ok()?
        }
    } else if ext == "pdf" {
        #[cfg(feature = "pdf")]
        {
            image::load_from_memory(&crate::pdf::page_png(path, 1, temp).ok()?).ok()?
        }
        #[cfg(not(feature = "pdf"))]
        return None;
    } else {
        return None;
    };
    let _ = temp;
    let img = if img.width() > 1024 || img.height() > 1536 { img.thumbnail(1024, 1536) } else { img };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Ask the AI for one file's description, or say why not.
#[cfg(feature = "ai")]
pub async fn describe(state: &BbsState, f: &FileRec) -> anyhow::Result<String> {
    let cfg = state.config();
    anyhow::ensure!(cfg.ai.enabled, "the AI is switched off");
    let areas = state.fareas();
    let area = areas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&f.area)).ok_or_else(|| anyhow::anyhow!("no area {}", f.area))?;
    let path = cfg.resolve(&area.path).join(&f.name);
    let facts = evidence(&path, f, &area.name);
    let temp = cfg.resolve(&cfg.paths.temp).join("aidesc");
    let answer = match picture(&path, f, &temp) {
        Some(png) => crate::aimemory::complete_seeing(&cfg.ai, SYSTEM, &facts, &png).await?,
        None => crate::aimemory::complete(&cfg.ai, SYSTEM, &facts).await?,
    };
    let d = tidy(&answer);
    anyhow::ensure!(!d.is_empty(), "the AI said nothing");
    Ok(d)
}

/// `describe` as a boxed future that owns what it needs: the file
/// editor's future is deep already, and the web client's would overflow
/// the compiler's checks inside it.
#[cfg(feature = "ai")]
pub fn describe_owned(state: Arc<BbsState>, f: FileRec) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move { describe(&state, &f).await })
}

/// One at a time, so a batch of uploads doesn't queue up on the AI.
#[cfg(feature = "ai")]
static ONE_AT_A_TIME: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// After an upload: if the file's description is bare, have the AI write
/// one, in the background.
pub fn spawn(state: Arc<BbsState>, area: String, num: u32) {
    #[cfg(feature = "ai")]
    tokio::spawn(async move {
        let _turn = ONE_AT_A_TIME.acquire().await;
        let Ok(Some(mut f)) = state.files.get(&area, num) else { return };
        if !is_bare(&f) {
            return;
        }
        match describe(&state, &f).await {
            Ok(d) => {
                tracing::info!("ai: described {} as {d:?}", f.name);
                f.description = with_summary(&f, d);
                if let Err(e) = state.files.update(&f) {
                    tracing::warn!("ai: couldn't save {}'s description: {e:#}", f.name);
                }
            }
            Err(e) => tracing::warn!("ai: couldn't describe {}: {e:#}", f.name),
        }
    });
    #[cfg(not(feature = "ai"))]
    let _ = (state, area, num);
}

/// `fluxbbs describe [--dry-run | --try] [--limit N] [--area AREA] [config]`: describe
/// the files already on the board whose descriptions are bare. `--dry-run`
/// lists them; `--try` asks the AI and shows what it says, saving nothing.
pub async fn from_args(args: &[String]) -> anyhow::Result<()> {
    let (mut dry, mut try_only) = (false, false);
    let mut limit = usize::MAX;
    let mut only: Option<String> = None;
    let mut config = crate::config::default_path();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" => dry = true,
            "--try" => try_only = true,
            "--limit" => limit = it.next().and_then(|n| n.parse().ok()).ok_or_else(|| anyhow::anyhow!("--limit takes a number"))?,
            "--area" => only = Some(it.next().cloned().ok_or_else(|| anyhow::anyhow!("--area takes an area's file name"))?),
            other => config = other.into(),
        }
    }
    let state = crate::load_state(&config)?;
    let mut bare = Vec::new();
    for area in state.fareas().areas.iter().filter(|a| only.as_ref().is_none_or(|o| a.file.eq_ignore_ascii_case(o))) {
        bare.extend(state.files.list(&area.file)?.into_iter().filter(is_bare));
    }
    println!("{} files with bare descriptions", bare.len());
    for f in bare.into_iter().take(limit) {
        if dry {
            println!("{}/{}: {:?}", f.area, f.name, f.description.join(" "));
            continue;
        }
        #[cfg(feature = "ai")]
        match describe(&state, &f).await {
            Ok(d) if try_only => println!("{}/{}: {d}", f.area, f.name),
            Ok(d) => {
                println!("{}/{}: {d}", f.area, f.name);
                let mut f = f;
                f.description = with_summary(&f, d);
                state.files.update(&f)?;
            }
            Err(e) => println!("{}/{}: not described ({e:#})", f.area, f.name),
        }
        #[cfg(not(feature = "ai"))]
        {
            let _ = try_only;
            anyhow::bail!("the AI isn't built into this board (build with --features ai; docs/BUILDING.md)");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, desc: &[&str]) -> FileRec {
        FileRec {
            area: "A".into(),
            num: 1,
            name: name.into(),
            size: 0,
            date: None,
            uploader: String::new(),
            uploaded: None,
            downloads: 0,
            points: 0,
            valid: true,
            description: desc.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn bare_is_none_or_the_name() {
        assert!(is_bare(&rec("pennyflux.ans", &[])));
        assert!(is_bare(&rec("pennyflux.ans", &["pennyflux"])));
        assert!(is_bare(&rec("fluxgirl_4.ans", &["FLUXGIRL4.ANS"])));
        assert!(!is_bare(&rec("pennyflux.ans", &["Penny Flux, the queen of disflux"])));
        // A picture's size is the board's, not a description.
        assert!(is_bare(&rec("x.jpg", &["JPEG image, 1024x1536"])));
        assert!(is_bare(&rec("x.gif", &["GIF image, 320x200, 12 frames"])));
        assert_eq!(with_summary(&rec("x.jpg", &["JPEG image, 1024x1536"]), "A cyborg".into()), vec!["A cyborg".to_string(), "JPEG image, 1024x1536".to_string()]);
        // The board's summary and a SAUCE date don't count; a hash title is nothing.
        assert!(is_bare(&rec("fluxgirl2.ans", &["fluxgirl2", "2026-10-30", "[ text, 59 lines ]"])));
        assert!(is_bare(&rec("fluxgirl.ans", &["QFHHKng1ReWgb4xV8vvDYNSaBhThkOZh", "2026-10-30", "[ text, 59 lines ]"])));
        assert!(!is_bare(&rec("a.ans", &["skull by pi", "2026-10-30", "[ text, 59 lines ]"])));
        let f = rec("fluxgirl2.ans", &["fluxgirl2", "2026-10-30", "[ text, 59 lines ]"]);
        assert_eq!(with_summary(&f, "Penny".into()), vec!["Penny".to_string(), "[ text, 59 lines ]".to_string()]);
    }

    #[test]
    fn the_text_in_a_zip_that_says_most() {
        assert!(text_rank("FILE_ID.DIZ") < text_rank("README.TXT"));
        assert!(text_rank("READ.ME") < text_rank("HYPERP.DOC"));
        assert_eq!(text_rank("HYPERP.EXE"), None);
        let dir = std::env::temp_dir().join(format!("aidesc-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hyperp.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, body) in [("HYPERP.EXE", "MZ binary"), ("HYPERP.DOC", "HyperPad is a hypertext notepad for DOS by someone")] {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        let facts = evidence(&path, &rec("hyperp.zip", &[]), "Utilities");
        assert!(facts.contains("The start of HYPERP.DOC, inside it: HyperPad is a hypertext notepad"), "{facts}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the AI would be told about files:
    /// `AIDESC_IN=a.zip:b.txt cargo test --features full -- --ignored aidesc::tests::facts --nocapture`
    #[test]
    #[ignore]
    fn facts() {
        for p in std::env::var("AIDESC_IN").unwrap().split(':') {
            let path = Path::new(p);
            let name = path.file_name().unwrap().to_string_lossy();
            println!("=== {p}\n{}", evidence(path, &rec(&name, &[]), "test"));
        }
    }

    #[test]
    fn sauce_titles_that_are_only_the_name_say_nothing() {
        assert!(says_nothing("penny_green", "penny_green.ans"));
        assert!(says_nothing("QFHHKng1ReWgb4xV8vvDYNSaBhThkOZh", "fluxgirl.ans"));
        assert!(says_nothing("", "fluxgirl.ans"));
        assert!(!says_nothing("Neon Nights", "fluxgirl.ans"));
    }

    #[test]
    fn the_boards_own_zip_listing_is_bare() {
        assert!(is_bare(&rec("hyperp.zip", &["2 files: HYPERP.DOC HYPERP.EXE"])));
        assert!(is_bare(&rec("big.zip", &["139 files: A.ANS B.ANS", "C.ANS"])));
        assert!(!is_bare(&rec("x.zip", &["2 files: a door game and its docs"])));
        assert!(!is_bare(&rec("x.zip", &["Hyper Pascal compiler"])));
    }

    #[test]
    fn answers_are_tidied_to_a_line() {
        assert_eq!(tidy("\"ANSI art of a redhead with a chrome arm.\"\n"), "ANSI art of a redhead with a chrome arm");
        assert_eq!(tidy("Description: TradeWars 2002 door game"), "TradeWars 2002 door game");
        let long = "word ".repeat(30);
        assert!(tidy(&long).chars().count() <= MAX_CHARS);
    }
}
