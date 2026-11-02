//! New: the reading room (`AB`): a library by a fire, its shelf a set
//! of the board's textfiles that changes every day, on a theme (`[library]`).
//! The theme picks books from across the areas by words in their names
//! and descriptions, so a day's shelf is a curated mix rather than one
//! directory; dated themes take their days (ghost stories at Halloween),
//! the rest go round. Every caller sees the same shelf that day. Books
//! open in the viewer at the caller's bookmark; reading to the end marks
//! one finished. The room is drawn in pixels on the casino's stage.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::config::{Library, LibraryTheme};
use crate::input::Key;
use crate::pixels::{Canvas, H, Ink, Stage, W, add, hash, ink, mix, painted, rgb, scale, smooth};
use crate::session::Session;
use crate::BbsState;

const MSG_ROW: u16 = 23;
const STATUS_ROW: u16 = 24;
/// Where the reading room keeps its shelf and the callers' cards.
const STORE: &str = "library";

/// The built-in themes: name, what the mantel says, the words, the days.
const THEMES: &[(&str, &str, &[&str], &[&str])] = &[
    ("Hackers", "Logins, lockouts, and the people who couldn't leave a system alone.",
        &["hack", "phrack", "unix", "vax", "password", "intrusion", "computer underground", "legion of doom", "system security", "intruder", "login"], &["11-02"]),
    ("Phone phreaks", "Before the net there was the phone network, and the kids who knew it better than Ma Bell did.",
        &["phreak", "blue box", "bell system", "telco", "payphone", "pay phone", "telephone", "2600", "operator", "switching", "trunk"], &[]),
    ("BBS days", "Modems, sysops and the boards that came before this one.",
        &["bbs", "sysop", "fidonet", "modem", "baud", "door game", "bulletin board", "callers"], &["02-16"]),
    ("Ghost stories", "Pull the chair closer to the fire.",
        &["ghost", "haunt", "horror", "vampire", "zombie", "halloween", "werewolf", "poltergeist", "scary", "spooky", "ghost story"], &["10-25", "10-28", "10-30", "10-31"]),
    ("Little green men", "Lights in the sky, men in black, and what crashed at Roswell.",
        &["ufo", "alien", "abduct", "roswell", "flying saucer", "extraterrestrial", "area 51", "close encounter"], &["07-02", "07-08"]),
    ("The space race", "Rockets, the moon, and the long way out.",
        &["nasa", "space shuttle", "spaceflight", "moon", "mars", "apollo", "astronaut", "rocket", "orbit", "planet"], &["07-20"]),
    ("They don't want you to know", "Cover-ups, grassy knolls and files marked secret.",
        &["conspiracy", "cia", "fbi", "kennedy", "jfk", "cover-up", "coverup", "illuminati", "assassination", "secret government"], &[]),
    ("Laughs", "The jokes that went round every board in town.",
        &["humor", "humour", "joke", "funny", "parody", "satire", "comedy", "laugh", "pun"], &["04-01"]),
    ("Tall tales", "Stories somebody sat up all night typing.",
        &["story", "stories", "fiction", "tale", "fable", "novel", "chapter"], &[]),
    ("Science fiction", "Robots, starships and futures that never quite came.",
        &["science fiction", "sci-fi", "scifi", "robot", "star trek", "starship", "time travel", "cyberpunk", "android", "galaxy"], &[]),
    ("Old machines", "Apple IIs, Commodores, Ataris and the beige boxes that started it.",
        &["apple ii", "apple //", "commodore", "atari", "ibm pc", "amiga", "trs-80", "c64", "6502", "z80", "8-bit", "microcomputer"], &["01-24", "08-12"]),
    ("The kitchen", "Recipes passed hand to hand, modem to modem.",
        &["recipe", "cook", "beer", "coffee", "bread", "chili", "wine", "cake", "cookies", "barbecue"], &["11-26"]),
    ("Noise", "Bands, lyrics, and arguments about both.",
        &["music", "band", "lyrics", "rock", "punk", "guitar", "album", "heavy metal", "song", "concert"], &[]),
    ("Game night", "Dungeons, walkthroughs and hints for the parts you were stuck on.",
        &["game", "d&d", "dungeon", "adventure", "role playing", "roleplaying", "rpg", "arcade", "infocom", "walkthrough", "zork"], &[]),
    ("Weird science", "Tesla coils, free energy and theories the journals passed on.",
        &["science", "physics", "experiment", "invention", "tesla", "free energy", "theory", "keelynet", "inventor"], &["03-14"]),
    ("After midnight", "Tarot, astrology, and things that go bump.",
        &["occult", "tarot", "magick", "ritual", "astrology", "psychic", "paranormal", "mystic", "spiritual", "esoteric"], &[]),
    ("On the air", "Hams, scanners and the voices between the stations.",
        &["radio", "ham", "scanner", "shortwave", "pirate radio", "antenna", "frequency", "amateur radio", "cb"], &[]),
    ("Law and order", "Busts, raids, trials, and what the Secret Service took.",
        &["law", "police", "arrest", "court", "crackdown", "sun devil", "sundevil", "secret service", "trial", "legal", "rights"], &[]),
    ("Zine night", "Issue after issue, from the zines that ran on every board.",
        &["magazine", "issue", "newsletter", "zine", "journal", "editorial"], &[]),
    ("The net", "Usenet, gophers and the first days of the internet.",
        &["internet", "usenet", "email", "e-mail", "arpanet", "newsgroup", "gopher", "world wide web", "network"], &["10-29"]),
    ("The holidays", "Snow at the window, and a fire in the grate.",
        &["christmas", "santa", "holiday", "new year", "snow", "winter", "xmas"], &["12-20", "12-23", "12-24", "12-25"]),
    ("Back then", "History, as people on the boards told it.",
        &["history", "war", "century", "ancient", "president", "revolution", "historical"], &[]),
    ("Soapbox", "Rants, manifestos and people with something to say.",
        &["rant", "manifesto", "opinion", "freedom", "censorship", "privacy", "free speech"], &[]),
];

/// What never goes on the shelf: how-tos for hurting people, and files
/// that aren't for reading (release notes, cracks, lists).
const SKIP: &[&str] = &[
    "bomb", "explosive", "napalm", "detonat", "incendiar", "thermite", "nitroglycerin", "gunpowder", "molotov", "nuclear device",
    "poison", "suicide", "kill", "weapon", "firearm", "silencer", "meth", "synthesi",
    "unprotect", "pinout", "softdocs", "bbs:", "bbs list", "collection of", "cracked by", "trainer", "nfo",
];

/// Extensions of files that aren't for reading in the viewer.
const NOT_BOOKS: &[&str] = &["ans", "nfo", "diz", "exe", "com", "zip", "gz", "tgz", "tar", "arj", "lzh", "gif", "jpg", "jpeg", "png", "pdf", "bin", "dat", "xm", "mod", "s3m", "htm", "html"];

/// A book on the shelf.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub area: String,
    pub name: String,
    pub title: String,
}

impl Book {
    fn key(&self) -> String {
        format!("{}/{}", self.area, self.name)
    }
}

/// The day's shelf.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shelf {
    /// The day it's for (`YYYY-MM-DD`).
    pub day: String,
    pub theme: String,
    pub note: String,
    pub books: Vec<Book>,
    /// New: the AI's note on the mantel about the day's shelf, once it's
    /// written (in the background, when the shelf is chosen).
    #[serde(default)]
    pub penny: String,
}

/// A file that could be a book.
pub struct Candidate {
    pub area: String,
    /// The area and those above it, their keys: variety is kept by both.
    pub top: String,
    pub name: String,
    pub title: String,
    /// Its name and description, lower case; the areas' names.
    pub text: String,
    pub areas_text: String,
    pub path: PathBuf,
}

/// The themes in use: the board's own, then the built-in ones.
fn themes(cfg: &Library) -> Vec<LibraryTheme> {
    let mut out: Vec<LibraryTheme> = cfg.themes.iter().filter(|t| !t.name.trim().is_empty() && !t.words.is_empty()).cloned().collect();
    if cfg.builtin_themes || out.is_empty() {
        out.extend(THEMES.iter().map(|(name, note, words, dates)| LibraryTheme {
            name: name.to_string(),
            note: note.to_string(),
            words: words.iter().map(|w| w.to_string()).collect(),
            dates: dates.iter().map(|d| d.to_string()).collect(),
        }));
    }
    out
}

/// Does `dates` (`MM-DD` or `MM-DD..MM-DD`) take in `day`?
fn on_date(dates: &[String], day: NaiveDate) -> bool {
    let md = |s: &str| {
        let (m, d) = s.trim().split_once('-')?;
        Some((m.parse::<u32>().ok()?, d.parse::<u32>().ok()?))
    };
    let today = (day.month(), day.day());
    dates.iter().any(|d| match d.split_once("..") {
        Some((a, b)) => match (md(a), md(b)) {
            (Some(a), Some(b)) if a <= b => a <= today && today <= b,
            // Across the new year.
            (Some(a), Some(b)) => today >= a || today <= b,
            _ => false,
        },
        None => md(d) == Some(today),
    })
}

/// The order to try the themes in on `day`: one dated for today first,
/// then the rest going round a day at a time.
fn theme_order(themes: &[LibraryTheme], day: NaiveDate) -> Vec<usize> {
    let mut dated: Vec<usize> = (0..themes.len()).filter(|&i| on_date(&themes[i].dates, day)).collect();
    // A theme with its day within the week either side waits for it.
    let near = |i: usize| (1..=7u64).any(|k| on_date(&themes[i].dates, day + chrono::Days::new(k)) || on_date(&themes[i].dates, day - chrono::Days::new(k)));
    let mut round: Vec<usize> = (0..themes.len()).collect();
    // A steady shuffle, so neighbours in the list aren't neighbours in time.
    round.sort_by(|&a, &b| hash(a as i32, 7919).total_cmp(&hash(b as i32, 7919)));
    let n = round.len().max(1);
    round.rotate_left(day.num_days_from_ce().rem_euclid(n as i32) as usize);
    // One that waits gives its day to one from half the round away, so
    // the stand-in isn't tomorrow's theme too.
    if dated.is_empty() && near(round[0]) {
        let swap = (0..n).map(|k| (n / 2 + k) % n).find(|&k| !near(round[k]));
        if let Some(k) = swap {
            round.swap(0, k);
        }
    }
    let (far, close): (Vec<usize>, Vec<usize>) = round.into_iter().filter(|i| !dated.contains(i)).partition(|&i| !near(i));
    dated.extend(far);
    dated.extend(close);
    dated
}

/// Is `word` in `text` at the start of a word?
fn has_word(text: &str, word: &str) -> bool {
    let word = word.trim();
    if word.is_empty() {
        return false;
    }
    text.match_indices(word).any(|(i, _)| i == 0 || !text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric()))
}

/// How well a file fits a theme: its name and description count double
/// its areas' names.
fn score(c: &Candidate, words: &[String]) -> u32 {
    words
        .iter()
        .map(|w| w.to_lowercase())
        .map(|w| if has_word(&c.text, &w) { 2 } else if has_word(&c.areas_text, &w) { 1 } else { 0 })
        .sum()
}

/// Is the file text that reads as prose: no control codes or ANSI, and
/// mostly letters (not art, tables or a list)?
fn reads_well(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut head = vec![0u8; 16 * 1024];
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let Ok(n) = f.read(&mut head) else { return false };
    let head = &head[..n];
    if !crate::upscan::looks_like_text(head) || head.iter().filter(|&&b| b == 0x1b).count() > 4 {
        return false;
    }
    let solid: Vec<u8> = head.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    let letters = solid.iter().filter(|b| b.is_ascii_alphabetic()).count();
    solid.len() > 200 && letters * 10 >= solid.len() * 6
}

/// A file's name without its extensions (`nha-002.phk.txt`: `nha-002`),
/// to tell copies apart.
fn stem(name: &str) -> String {
    name.to_lowercase().split('.').next().unwrap_or_default().to_string()
}

/// The books for `theme` from `cands`: the best fits first, at most a
/// few from any one area so it's a mix, steady for `day`. `ok` says if
/// a file reads well (looked at only for those that might be picked).
fn pick_books(cands: &[Candidate], theme: &LibraryTheme, day: NaiveDate, most: usize, ok: &mut dyn FnMut(usize) -> bool) -> Vec<usize> {
    let seed = day.num_days_from_ce();
    let mut fits: Vec<(u32, f32, usize)> = cands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let s = score(c, &theme.words);
            (s >= 2).then(|| (s.min(6), hash(i as i32, seed), i))
        })
        .collect();
    fits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
    let mut out = Vec::new();
    // First a mix (three from an area, a third of the shelf from one
    // section), then whatever's left if that's too few.
    for (per_area, per_top) in [(3usize, (most / 3).max(3)), (most, most)] {
        let mut by_area: HashMap<&str, usize> = HashMap::new();
        let mut by_top: HashMap<&str, usize> = HashMap::new();
        for &i in &out {
            let c: &Candidate = &cands[i];
            *by_area.entry(&c.area).or_default() += 1;
            *by_top.entry(&c.top).or_default() += 1;
        }
        let mut seen_titles: HashSet<String> = out.iter().map(|&i: &usize| cands[i].title.to_lowercase()).collect();
        let mut seen_stems: HashSet<String> = out.iter().map(|&i: &usize| stem(&cands[i].name)).collect();
        for &(_, _, i) in &fits {
            if out.len() >= most {
                break;
            }
            let c = &cands[i];
            if out.contains(&i) || by_area.get(c.area.as_str()).copied().unwrap_or(0) >= per_area || by_top.get(c.top.as_str()).copied().unwrap_or(0) >= per_top {
                continue;
            }
            // One copy of a file kept in two places.
            if (!c.title.is_empty() && seen_titles.contains(&c.title.to_lowercase())) || seen_stems.contains(&stem(&c.name)) {
                continue;
            }
            if !ok(i) {
                continue;
            }
            seen_titles.insert(c.title.to_lowercase());
            seen_stems.insert(stem(&c.name));
            *by_area.entry(&c.area).or_default() += 1;
            *by_top.entry(&c.top).or_default() += 1;
            out.push(i);
        }
        if out.len() >= most {
            break;
        }
    }
    // Shelved in a steady jumble, not best first.
    out.sort_by(|&a, &b| hash(a as i32, seed + 1).total_cmp(&hash(b as i32, seed + 1)));
    out
}

/// Every file in the reading room's areas that could be a book.
fn candidates(state: &BbsState) -> anyhow::Result<Vec<Candidate>> {
    let cfg = state.config();
    let lib = &cfg.library;
    let fa = state.fareas();
    let tree = fa.tree();
    let wanted: Vec<u32> = if lib.areas.iter().all(|a| a.trim().is_empty()) {
        tree.iter().map(|&(n, _)| n).collect()
    } else {
        let mut v = Vec::new();
        for key in &lib.areas {
            if let Some(i) = fa.areas.iter().position(|a| a.file.eq_ignore_ascii_case(key.trim())) {
                for n in fa.subtree(i as u32 + 1) {
                    if !v.contains(&n) {
                        v.push(n);
                    }
                }
            }
        }
        v
    };
    let mut keys = Vec::new();
    let mut about: HashMap<String, (String, String, PathBuf)> = HashMap::new();
    for n in wanted {
        let Some(a) = fa.get(n) else { continue };
        // A passworded area isn't for browsing into from here.
        if !a.password.is_empty() {
            continue;
        }
        let line = fa.lineage(n);
        let top = line.last().and_then(|&t| fa.get(t)).map_or_else(|| a.file.clone(), |t| t.file.clone());
        let names: Vec<String> = line.iter().filter_map(|&m| fa.get(m)).map(|m| crate::editor::strip_colours(&m.name).to_lowercase()).collect();
        about.insert(a.file.clone(), (top, names.join(" "), cfg.resolve(&a.path)));
        keys.push(a.file.clone());
    }
    let skip: Vec<String> = SKIP.iter().map(|s| s.to_string()).chain(lib.skip.iter().map(|s| s.to_lowercase())).collect();
    let (min, max) = (lib.min_kb as u64 * 1024, lib.max_kb.max(lib.min_kb) as u64 * 1024);
    let mut out = Vec::new();
    for f in state.files.search(&keys, None, None)? {
        if !f.valid || f.size < min || f.size > max {
            continue;
        }
        let lower = f.name.to_lowercase();
        let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
        if NOT_BOOKS.contains(&ext) || matches!(lower.as_str(), "index" | "index.txt" | "00index.txt" | "files.bbs" | "descript.ion") {
            continue;
        }
        let title = crate::web::blurb(&f.description);
        let desc = f.description.join(" ").to_lowercase();
        let text = format!("{lower} {desc}");
        if skip.iter().any(|w| has_word(&text, w)) {
            continue;
        }
        // Who wrote it isn't what it's about ("by The Silver Ghost").
        let text = match text.find(" by ") {
            Some(i) => text[..i].to_string(),
            None => text,
        };
        let Some((top, areas_text, dir)) = about.get(&f.area) else { continue };
        out.push(Candidate { area: f.area.clone(), top: top.clone(), path: dir.join(&f.name), name: f.name, title, text, areas_text: areas_text.clone() });
    }
    Ok(out)
}

/// Choose the shelf for `day`: the first theme in the day's order that
/// finds enough books, else the one that found most.
pub fn choose(state: &BbsState, day: NaiveDate) -> anyhow::Result<Shelf> {
    let cfg = state.config();
    let lib = &cfg.library;
    let cands = candidates(state)?;
    let themes = themes(lib);
    let most = lib.books.clamp(1, 33) as usize;
    let fewest = (lib.min_books as usize).clamp(1, most);
    let mut best: Option<(usize, Vec<usize>)> = None;
    let mut checked: HashMap<usize, bool> = HashMap::new();
    for t in theme_order(&themes, day) {
        let mut ok = |i: usize| *checked.entry(i).or_insert_with(|| reads_well(&cands[i].path));
        let books = pick_books(&cands, &themes[t], day, most, &mut ok);
        let enough = books.len() >= fewest;
        if best.as_ref().is_none_or(|(_, b)| books.len() > b.len()) {
            best = Some((t, books));
        }
        if enough {
            break;
        }
    }
    let (t, books) = best.unwrap_or((0, Vec::new()));
    let theme = themes.get(t).cloned().unwrap_or_default();
    Ok(Shelf {
        day: day.to_string(),
        theme: theme.name,
        note: theme.note,
        books: books.into_iter().map(|i| Book { area: cands[i].area.clone(), name: cands[i].name.clone(), title: cands[i].title.clone() }).collect(),
        penny: String::new(),
    })
}

/// Today's shelf: kept once chosen, so it stays the same all day.
pub async fn today(state: Arc<BbsState>, again: bool) -> anyhow::Result<Shelf> {
    let day = crate::clock::now().date_naive();
    if !again
        && let Some(s) = state.sys.script_get(STORE, "shelf")?.and_then(|v| serde_json::from_str::<Shelf>(&v).ok())
        && s.day == day.to_string()
    {
        return Ok(s);
    }
    let st = state.clone();
    let shelf = tokio::task::spawn_blocking(move || choose(&st, day)).await??;
    state.sys.script_set(STORE, "shelf", Some(&serde_json::to_string(&shelf)?))?;
    tracing::info!("library: {}'s shelf is {:?}, {} books", day, shelf.theme, shelf.books.len());
    #[cfg(feature = "ai")]
    if state.config().ai.enabled && !shelf.books.is_empty() {
        let (st, sh) = (state.clone(), shelf.clone());
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(120), penny_note(&st, &sh)).await {
                Ok(Ok(note)) => {
                    // Onto the shelf as kept, if it's still the day's.
                    let kept = st.sys.script_get(STORE, "shelf").ok().flatten().and_then(|v| serde_json::from_str::<Shelf>(&v).ok());
                    if let Some(mut k) = kept.filter(|k| k.day == sh.day && k.theme == sh.theme) {
                        tracing::info!("library: the AI's note: {note:?}");
                        k.penny = note;
                        if let Ok(v) = serde_json::to_string(&k) {
                            let _ = st.sys.script_set(STORE, "shelf", Some(&v));
                        }
                    }
                }
                Ok(Err(e)) => tracing::warn!("library: no note from the AI: {e:#}"),
                Err(_) => tracing::warn!("library: no note from the AI: it took too long"),
            }
        });
    }
    Ok(shelf)
}

/// The AI's note for the mantel: a few lines about the day's theme and
/// shelf, in its own voice (`[ai] name` and the extra prompt).
#[cfg(feature = "ai")]
async fn penny_note(state: &BbsState, shelf: &Shelf) -> anyhow::Result<String> {
    let cfg = state.config();
    let name = if cfg.ai.name.trim().is_empty() { "the librarian".to_string() } else { cfg.ai.name.trim().to_string() };
    let system = format!(
        "You are {name}, who looks after {board}, a bulletin board system. {extra}\n\nTonight you're leaving a handwritten note on the \
         mantel of the board's reading room, a library by a fire whose shelf is a set of old textfiles on one theme. Write the note: \
         two or three short sentences, under 260 characters, in your own voice, inviting callers to pull up a chair and read. \
         You may name one or two of the titles. Plain text only: no quotes around it, no emoji, no markdown, no signature.",
        board = cfg.bbs.name,
        extra = cfg.ai.system_prompt.trim(),
    );
    let titles: Vec<&str> = shelf.books.iter().map(|b| if b.title.is_empty() { b.name.as_str() } else { b.title.as_str() }).take(10).collect();
    let prompt = format!("Tonight's theme: {}. ({})\nSome of the books on the shelf:\n- {}", shelf.theme, shelf.note, titles.join("\n- "));
    let answer = crate::aimemory::complete(&cfg.ai, &system, &prompt).await?;
    let note = answer.split_whitespace().collect::<Vec<_>>().join(" ");
    let note = note.trim_matches(['"', '\'', '`', ' ']);
    anyhow::ensure!(!note.is_empty(), "the AI said nothing");
    // Kept to the card (eight lines), cut at a word.
    let mut out = String::new();
    for w in note.split(' ') {
        if out.chars().count() + w.chars().count() + 1 > 340 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    Ok(out)
}

/// How often a book has been opened: by whom (each caller once), and by
/// how many today.
#[derive(Clone, Default, Serialize, Deserialize)]
struct ReadCount {
    area: String,
    name: String,
    title: String,
    readers: Vec<u32>,
    day: String,
    today: Vec<u32>,
}

/// One node at a time changes the counts.
static COUNTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_counts(state: &BbsState) -> Vec<ReadCount> {
    state.sys.script_get(STORE, "reads").ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// Count `user` as a reader of `b`.
fn count_read(state: &BbsState, b: &Book, user: u32) {
    let _held = COUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let today = crate::clock::now().date_naive().to_string();
    let mut all = read_counts(state);
    let i = match all.iter().position(|r| r.area == b.area && r.name == b.name) {
        Some(i) => i,
        None => {
            all.push(ReadCount { area: b.area.clone(), name: b.name.clone(), title: b.title.clone(), ..Default::default() });
            all.len() - 1
        }
    };
    let r = &mut all[i];
    if r.day != today {
        r.day = today;
        r.today.clear();
    }
    if !r.readers.contains(&user) {
        r.readers.push(user);
    }
    if !r.today.contains(&user) {
        r.today.push(user);
    }
    // The most read are kept when there are too many.
    if all.len() > 3000 {
        all.sort_by_key(|r| std::cmp::Reverse(r.readers.len()));
        all.truncate(2500);
    }
    if let Ok(v) = serde_json::to_string(&all)
        && let Err(e) = state.sys.script_set(STORE, "reads", Some(&v))
    {
        tracing::error!("library: counting a read: {e}");
    }
}

/// The most read books, most readers first (then most today).
fn most_read(state: &BbsState, visible: &HashSet<String>, n: usize) -> Vec<(Book, usize, usize)> {
    let today = crate::clock::now().date_naive().to_string();
    let mut all: Vec<(Book, usize, usize)> = read_counts(state)
        .into_iter()
        .filter(|r| visible.contains(&r.area.to_ascii_uppercase()))
        .map(|r| {
            let t = if r.day == today { r.today.len() } else { 0 };
            (Book { area: r.area, name: r.name, title: r.title }, r.readers.len(), t)
        })
        .collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    all.truncate(n);
    all
}

/// A card over the room: the note on the mantel, or the most read.
enum Overlay {
    Note(Vec<String>),
    Most(Vec<(Book, usize, usize)>, usize),
}

/// Draw the card: parchment, its lines, the one picked marked.
fn draw_overlay(cv: &mut Canvas, o: &Overlay, who: &str) {
    const COL: usize = 14;
    const WIDTH: usize = 52;
    let paper = rgb(236, 222, 186);
    let ink_c = rgb(60, 40, 25);
    let (head, lines, foot, sel): (String, Vec<String>, &str, Option<usize>) = match o {
        Overlay::Note(lines) => (format!("a note from {}", who.to_lowercase()), lines.clone(), "any key puts it back", None),
        Overlay::Most(list, sel) => {
            let lines = list
                .iter()
                .map(|(b, n, t)| {
                    let title = if b.title.is_empty() { b.name.clone() } else { b.title.clone() };
                    let today = if *t > 0 { format!("{t} today") } else { String::new() };
                    let count = format!("{n:>3}  {today:<8}");
                    let room = WIDTH - 4 - count.chars().count() - 2;
                    format!("{count}  {}", title.chars().take(room).collect::<String>())
                })
                .collect();
            ("most read in the reading room".to_string(), lines, "enter reads  esc puts it back", Some(*sel))
        }
    };
    let top = 3usize;
    let rows = lines.len() + 4;
    for r in 0..rows {
        cv.label(COL, top + r, &" ".repeat(WIDTH), ink_c, Some(paper));
    }
    cv.label(COL + 2, top, &head, rgb(140, 30, 30), Some(paper));
    for (i, l) in lines.iter().enumerate() {
        let picked = sel == Some(i);
        let text = format!("{:<w$}", l, w = WIDTH - 4);
        cv.label(COL + 2, top + 2 + i, &text, if picked { paper } else { ink_c }, Some(if picked { rgb(120, 70, 36) } else { paper }));
    }
    cv.label(COL + 2, top + rows - 1, foot, rgb(140, 120, 90), Some(paper));
}

/// `fluxbbs library [--day YYYY-MM-DD] [--days N] [CONFIG]`: the shelves
/// those days would have, printed and not kept (the board's own is
/// chosen on the day's first visit).
pub fn from_args(args: &[String]) -> anyhow::Result<()> {
    let mut day = crate::clock::now().date_naive();
    let mut days = 1u32;
    let mut config = crate::config::default_path();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--day" => day = it.next().and_then(|d| d.parse().ok()).ok_or_else(|| anyhow::anyhow!("--day takes YYYY-MM-DD"))?,
            "--days" => days = it.next().and_then(|n| n.parse().ok()).ok_or_else(|| anyhow::anyhow!("--days takes a number"))?,
            other => config = other.into(),
        }
    }
    let state = crate::load_state(&config)?;
    for k in 0..days {
        let d = day + chrono::Days::new(k as u64);
        let shelf = choose(&state, d)?;
        println!("{d}: {} ({} books) - {}", shelf.theme, shelf.books.len(), shelf.note);
        if days == 1 {
            for b in &shelf.books {
                println!("  {:<24} {:<14} {}", b.area, b.name, b.title);
            }
        }
    }
    Ok(())
}

// ---- the room ------------------------------------------------------------------

/// Where things stand in the room.
const FIRE: (f32, f32) = (21.0, 27.0);
/// The globe: its middle and radius.
const GLOBE: (f32, f32, f32) = (52.0, 36.0, 4.0);
const SHELF_X: i32 = 44;
const SHELF_SLOTS: usize = 11;
const BOARDS: [i32; 3] = [9, 19, 28];

/// Value noise, smooth between the points of a grid.
fn noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * sx;
    let b = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * sx;
    a + (b - a) * sy
}

/// Inside the fireplace's opening (an arch).
fn in_firebox(x: i32, y: i32) -> bool {
    if !(13..=29).contains(&x) || !(14..=30).contains(&y) {
        return false;
    }
    let (u, v) = ((x as f32 - 21.0) / 8.5, (y as f32 - 19.0) / 5.0);
    y >= 19 || u * u + v * v <= 1.0
}

/// The room without what moves or changes.
fn room() -> &'static Canvas {
    static ROOM: OnceLock<Canvas> = OnceLock::new();
    ROOM.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let c = if y < 21 {
                    // Green wallpaper with a faint diamond in it.
                    let base = mix(rgb(30, 52, 40), rgb(16, 30, 24), y as f32 / 21.0);
                    let (u, v) = ((x.rem_euclid(8)) - 4, (y.rem_euclid(8)) - 4);
                    if u.abs() + v.abs() == 3 { add(base, rgb(60, 70, 40), 0.07) } else { base }
                } else if y < 31 {
                    // Wood panelling under a rail.
                    let base = mix(rgb(78, 44, 24), rgb(52, 28, 14), (y - 21) as f32 / 10.0);
                    if y == 21 {
                        rgb(120, 78, 42)
                    } else if y == 22 || x.rem_euclid(13) == 0 || y == 30 {
                        scale(base, 0.7)
                    } else {
                        scale(base, 0.92 + 0.08 * hash(x / 13, 3))
                    }
                } else {
                    // Floorboards, darker towards the back.
                    let row = (y - 31) / 2;
                    let seam = (y - 31) % 2 == 1 || (x + (hash(row, 11) * 20.0) as i32).rem_euclid(19) == 0;
                    let base = mix(rgb(62, 36, 20), rgb(100, 60, 32), (y - 31) as f32 / 13.0);
                    let base = scale(base, 0.9 + 0.12 * hash(row, (x + (hash(row, 5) * 20.0) as i32) / 19));
                    if seam { scale(base, 0.72) } else { base }
                };
                cv.set(x, y, c);
            }
        }
        // The rug in front of the hearth: red, a gold border.
        for y in 33..44i32 {
            for x in 2..42i32 {
                let (u, v) = ((x as f32 - 22.0) / 19.0, (y as f32 - 38.5) / 4.8);
                let d = u * u + v * v;
                if d <= 1.0 {
                    let c = if d > 0.78 {
                        rgb(150, 110, 45)
                    } else if d > 0.68 {
                        rgb(90, 18, 24)
                    } else if (x + y * 2).rem_euclid(6) == 0 && d < 0.5 {
                        rgb(140, 95, 50)
                    } else {
                        mix(rgb(128, 26, 32), rgb(96, 16, 24), d)
                    };
                    cv.set(x, y, c);
                }
            }
        }
        // The chimney breast: stone, in courses.
        for y in 6..31i32 {
            for x in 5..38i32 {
                let course = y / 3;
                let off = if course % 2 == 0 { 0 } else { 3 };
                let mortar = y % 3 == 0 || (x + off).rem_euclid(6) == 0;
                let stone = scale(rgb(112, 100, 90), 0.75 + 0.3 * hash((x + off) / 6, course));
                cv.set(x, y, if mortar { rgb(58, 52, 48) } else { stone });
            }
        }
        // The mantel, over it all.
        for y in 6..9 {
            for x in 3..40 {
                cv.set(x, y, if y == 6 { rgb(130, 82, 44) } else { mix(rgb(92, 54, 28), rgb(60, 34, 18), (y - 6) as f32 / 2.0) });
            }
        }
        // The opening, sooty at the back.
        for y in 14..31 {
            for x in 13..30 {
                if in_firebox(x, y) {
                    let back = mix(rgb(30, 20, 16), rgb(10, 7, 6), (y - 14) as f32 / 16.0);
                    cv.set(x, y, back);
                }
            }
        }
        // The hearth stone.
        for x in 8..35 {
            cv.set(x, 31, rgb(130, 120, 110));
            cv.set(x, 32, rgb(96, 88, 80));
        }
        // The bookcase: a dark frame, three shelves.
        for y in 0..31 {
            for x in 42..80 {
                let frame = !(SHELF_X..78).contains(&x) || y == 0 || y == 30;
                cv.set(x, y, if frame { mix(rgb(70, 40, 20), rgb(50, 28, 14), x as f32 / 80.0) } else { rgb(26, 15, 9) });
            }
        }
        for &b in &BOARDS {
            for x in SHELF_X..78 {
                cv.set(x, b, rgb(104, 64, 34));
                cv.set(x, b + 1, rgb(60, 36, 20));
            }
        }
        // The armchair, turned to the fire: a wingback in oxblood leather,
        // lit from the fire's side.
        let leather = |x: i32, k: f32| scale(rgb(128, 36, 28), (0.55 + 0.6 * (1.0 - (x - 30) as f32 / 12.0)) * k);
        for y in 21..37 {
            for x in 30..42 {
                let back = (37..41).contains(&x) && y >= 21 && !(y == 21 && (x == 37 || x == 40));
                let wing = (35..37).contains(&x) && (23..31).contains(&y);
                let seat = (31..41).contains(&x) && (32..36).contains(&y);
                let arm = (30..34).contains(&x) && (29..33).contains(&y);
                let c = if arm {
                    leather(x, if y == 29 { 1.25 } else { 0.95 })
                } else if seat {
                    leather(x, if y == 32 { 1.15 } else { 0.85 })
                } else if wing {
                    leather(x, 0.8)
                } else if back {
                    // Buttoned.
                    leather(x, if (x + y) % 3 == 0 && y < 31 { 0.7 } else { 0.95 })
                } else {
                    continue;
                };
                cv.set(x, y, c);
            }
        }
        for x in 30..42 {
            cv.set(x, 36, rgb(70, 24, 18));
        }
        for y in 37..40 {
            cv.set(31, y, rgb(46, 26, 14));
            cv.set(40, y, rgb(46, 26, 14));
        }
        // The lamp table in the corner.
        for x in 66..77 {
            cv.set(x, 36, rgb(110, 68, 36));
        }
        for y in 37..43 {
            cv.set(67, y, rgb(70, 42, 22));
            cv.set(75, y, rgb(70, 42, 22));
        }
        for y in 33..36 {
            cv.set(71, y, rgb(170, 140, 70));
        }
        for y in 30..33 {
            let half = 2 + (y - 30);
            for x in 71 - half..=71 + half {
                cv.set(x, y, mix(rgb(240, 200, 120), rgb(200, 150, 80), (x - 71).abs() as f32 / 4.0));
            }
        }
        // A stack of books on the table.
        cv.rect(73, 34, 3, 1, rgb(40, 60, 100));
        cv.rect(72, 35, 4, 1, rgb(120, 30, 30));
        // The globe's stand: a wooden post and foot.
        cv.rect(GLOBE.0 as i32 - 1, 41, 2, 1, rgb(80, 48, 24));
        cv.rect(GLOBE.0 as i32 - 3, 42, 6, 1, rgb(96, 58, 30));
        cv
    })
}

/// A pixel of fire: how hot, to its color.
fn flame(heat: f32) -> Option<[f32; 3]> {
    if heat < 0.22 {
        return None;
    }
    Some(if heat < 0.42 {
        mix(rgb(90, 14, 4), rgb(200, 46, 10), (heat - 0.22) / 0.2)
    } else if heat < 0.66 {
        mix(rgb(200, 46, 10), rgb(255, 128, 24), (heat - 0.42) / 0.24)
    } else if heat < 0.86 {
        mix(rgb(255, 128, 24), rgb(255, 205, 70), (heat - 0.66) / 0.2)
    } else {
        mix(rgb(255, 205, 70), rgb(255, 245, 200), (heat - 0.86) / 0.14)
    })
}

/// Each slot on the shelves: (x, board) for the books in order, and the
/// rest are old volumes that are only for looks.
fn slots(n: usize) -> Vec<(i32, usize)> {
    let per = n.div_ceil(3).clamp(1, SHELF_SLOTS);
    let mut out = Vec::new();
    for (shelf, _) in BOARDS.iter().enumerate() {
        let here = per.min(n.saturating_sub(shelf * per));
        let first = (SHELF_SLOTS - per) / 2;
        for k in 0..here {
            out.push((SHELF_X + 1 + (first + k) as i32 * 3, shelf));
        }
    }
    out
}

/// A spine: its color and height, steady for the book.
fn spine(name: &str) -> ([f32; 3], i32) {
    const COLORS: [(u8, u8, u8); 9] =
        [(124, 26, 32), (32, 74, 48), (34, 48, 100), (112, 82, 38), (74, 32, 74), (146, 114, 62), (44, 42, 46), (98, 52, 26), (30, 86, 92)];
    let h = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let (r, g, b) = COLORS[h as usize % COLORS.len()];
    (rgb(r, g, b), 6 + (h >> 8) as i32 % 3)
}

/// How a book on the shelf stands: picked, finished, bookmarked.
#[derive(Clone, Copy, Default)]
struct Look {
    picked: bool,
    finished: bool,
    marked: bool,
}

fn draw_spine(cv: &mut Canvas, x: i32, board: i32, name: &str, look: Look, old: bool) {
    let (c, h) = spine(name);
    let c = if old { scale(c, 0.45) } else if look.picked { add(scale(c, 1.3), rgb(60, 50, 30), 0.6) } else { c };
    let lift = if look.picked { 1 } else { 0 };
    // Not up into the shelf above (or the frame).
    let top = (board - h - lift).max(board - 8);
    for y in top..board - lift {
        for dx in 0..2 {
            let mut p = if dx == 1 { scale(c, 0.78) } else { c };
            if (y == top + 1 || y == board - lift - 2) && !old {
                p = mix(p, rgb(210, 170, 80), 0.55);
            }
            cv.set(x + dx, y, p);
        }
    }
    if look.finished {
        cv.set(x, top, rgb(255, 215, 90));
    }
    if look.marked {
        // The ribbon hangs out under it.
        cv.set(x + 1, board, rgb(200, 30, 40));
    }
    if look.picked {
        for y in top..board {
            cv.blend(x - 1, y, rgb(255, 200, 120), 0.35);
            cv.blend(x + 2, y, rgb(255, 200, 120), 0.35);
        }
    }
}

/// The light the fire throws, at each pixel: steady, so a frame only
/// scales it.
fn glow_map() -> &'static Vec<f32> {
    static GLOW: OnceLock<Vec<f32>> = OnceLock::new();
    GLOW.get_or_init(|| {
        let mut out = vec![0.0; W * H];
        for y in 0..H {
            for x in 0..W {
                let (dx, dy) = ((x as f32 - FIRE.0) / 1.0, (y as f32 - FIRE.1) * 1.6);
                let d = (dx * dx + dy * dy).sqrt();
                let k = (1.0 - d / 46.0).max(0.0);
                out[y * W + x] = if in_firebox(x as i32, y as i32) { 0.0 } else { k * k };
            }
        }
        out
    })
}

fn draw_room(cv: &mut Canvas, t: f32, shelf: &Shelf, pick: usize, looks: &[Look], theme: &str) {
    cv.px.clone_from(&room().px);
    cv.clear_labels();
    // The fire's light, in a few steps so the room isn't redrawn whole
    // every frame.
    let flick = (0.8 + 0.3 * noise(t * 1.7, 3.0)).clamp(0.7, 1.1);
    let flick = (flick * 6.0).round() / 6.0;
    let glow = glow_map();
    for (i, p) in cv.px.iter_mut().enumerate() {
        let k = glow[i] * flick;
        if k > 0.01 {
            *p = add(*p, rgb(255, 120, 50), 0.32 * k);
        }
    }
    // The lamp's pool of light.
    for y in 26..44 {
        for x in 58..80 {
            let (dx, dy) = ((x as f32 - 71.0) / 9.0, (y as f32 - 31.0) / 7.0);
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1.0 {
                cv.blend(x, y, rgb(255, 220, 150), 0.18 * (1.0 - d));
            }
        }
    }
    // Flames: hot at the bottom and middle, licking upward.
    for y in 14..31 {
        for x in 13..30 {
            if !in_firebox(x, y) {
                continue;
            }
            let v = (30 - y) as f32 / 13.0;
            let u = (x as f32 - 21.0) / 8.0;
            let n = 0.6 * noise(x as f32 * 0.42, y as f32 * 0.42 + t * 5.0) + 0.4 * noise(x as f32 * 0.9 + 7.0, y as f32 * 0.8 + t * 8.5);
            let heat = (1.0 - v * 1.05) * (1.0 - u * u * 0.85) + (n - 0.5) * (0.5 + 0.7 * v);
            if let Some(c) = flame(heat) {
                cv.set(x, y, c);
            }
        }
    }
    // Sparks going up.
    for i in 0..4 {
        let life = (t * 0.9 + i as f32 * 0.27).fract();
        let born = (t * 0.9 + i as f32 * 0.27).floor() as i32;
        let x = 15 + (hash(i, born) * 12.0) as i32 + (life * 3.0 * (hash(born, i) - 0.5)) as i32;
        let y = 24 - (life * 10.0) as i32;
        if in_firebox(x, y) && life < 0.8 {
            cv.set(x, y, mix(rgb(255, 230, 120), rgb(200, 60, 20), life));
        }
    }
    // The logs, in front of the flames, glowing along their edges.
    for x in 14..29 {
        let ember = 0.5 + 0.5 * noise(x as f32 * 0.6, t * 2.0);
        cv.set(x, 29, mix(rgb(70, 40, 22), rgb(220, 70, 20), ember * 0.6));
        cv.set(x, 30, rgb(52, 30, 16));
    }
    for x in 16..27 {
        cv.set(x, 28, mix(rgb(80, 48, 26), rgb(255, 110, 30), 0.25 + 0.35 * noise(x as f32 * 0.8, t * 2.6)));
    }
    // Candles on the mantel.
    for (k, cx) in [8, 35].into_iter().enumerate() {
        cv.rect(cx, 3, 1, 3, rgb(230, 220, 190));
        let f = noise(t * 6.0, k as f32 * 9.0);
        cv.set(cx, 2, mix(rgb(255, 170, 60), rgb(255, 240, 180), f));
        if f > 0.55 {
            cv.set(cx, 1, rgb(255, 150, 50));
        }
        for dy in -1..=1 {
            for dx in -2..=2 {
                cv.blend(cx + dx, 2 + dy, rgb(255, 200, 120), 0.12);
            }
        }
    }
    // A note pinned up beside the clock.
    cv.rect(26, 3, 3, 3, rgb(236, 222, 186));
    cv.set(27, 3, rgb(200, 30, 40));
    // The clock in the middle of the mantel.
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (0, 0)] {
        cv.set(21 + dx, 4 + dy, if (dx, dy) == (0, 0) { rgb(40, 30, 20) } else { rgb(200, 160, 70) });
    }
    // The cat asleep on the rug, an orange tabby curled up, its tail
    // twitching now and then.
    for y in 36..42 {
        for x in 15..27 {
            let (u, v) = ((x as f32 - 21.5) / 5.0, (y as f32 - 39.2) / 2.1);
            if u * u + v * v <= 1.0 {
                let stripe = (x + y / 2) % 3 == 0;
                let c = mix(rgb(222, 140, 62), rgb(150, 80, 32), smooth(-0.6, 1.0, v));
                cv.set(x, y, if stripe { scale(c, 0.78) } else { c });
            }
        }
    }
    // Its head, tucked in at the left: ears up, eyes shut.
    for (x, y) in [(14, 37), (15, 37), (16, 37), (14, 38), (15, 38), (16, 38), (17, 38), (15, 39), (16, 39)] {
        cv.set(x, y, rgb(232, 152, 72));
    }
    cv.set(14, 36, rgb(196, 112, 50));
    cv.set(16, 36, rgb(196, 112, 50));
    cv.set(14, 38, rgb(80, 44, 22));
    cv.set(16, 38, rgb(80, 44, 22));
    // Its tail round the front, the tip lifting.
    let twitch = (t * 0.3).fract() < 0.1;
    for x in 18..27 {
        cv.set(x, 41, rgb(186, 106, 46));
    }
    cv.set(17, 41, if twitch { rgb(120, 20, 30) } else { rgb(205, 125, 55) });
    if twitch {
        cv.set(17, 40, rgb(205, 125, 55));
    }
    // The globe, turning slowly: sea and land, lit from the fire's side.
    let (gx, gy, r) = GLOBE;
    for y in (gy - r) as i32..=(gy + r) as i32 {
        for x in (gx - r) as i32 - 1..=(gx + r) as i32 + 1 {
            let (u, v) = ((x as f32 + 0.5 - gx) / r, (y as f32 + 0.5 - gy) / r);
            let d = u * u + v * v;
            if d > 1.0 {
                continue;
            }
            let lon = u.asin() + t * 0.25;
            let land = noise(lon * 2.2 + 10.0, v * 2.5 + 4.0) > 0.55;
            let c = if land { rgb(70, 120, 60) } else { rgb(40, 70, 130) };
            let lit = 0.6 + 0.4 * (-u * 0.7 - v * 0.3 + 0.4).clamp(0.0, 1.0);
            cv.set(x, y, scale(c, lit * (1.0 - 0.25 * d)));
        }
    }
    // Its brass meridian, round the right side.
    for k in 0..24 {
        let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 23.0;
        cv.set((gx + (r + 1.0) * a.cos()).round() as i32, (gy + (r + 1.0) * a.sin()).round() as i32, rgb(190, 150, 70));
    }
    // The books, today's among the old volumes.
    let places = slots(shelf.books.len());
    for (shelf_no, &board) in BOARDS.iter().enumerate() {
        for s in 0..SHELF_SLOTS {
            let x = SHELF_X + 1 + s as i32 * 3;
            if !places.iter().any(|&(px, b)| px == x && b == shelf_no) {
                draw_spine(cv, x, board, &format!("old{shelf_no}{s}"), Look::default(), true);
            }
        }
    }
    for (i, (b, &(x, shelf_no))) in shelf.books.iter().zip(places.iter()).enumerate() {
        let look = Look { picked: i == pick, ..looks.get(i).copied().unwrap_or_default() };
        draw_spine(cv, x, BOARDS[shelf_no], &b.name, look, false);
    }
    // The room's name over the bookcase, the day's theme on the mantel.
    let title = " the reading room ";
    cv.label(61 - title.len() / 2, 0, title, rgb(230, 190, 110), Some(rgb(50, 28, 14)));
    let plaque = format!(" {theme} ");
    let plaque: String = plaque.chars().take(34).collect();
    cv.label(21usize.saturating_sub(plaque.chars().count() / 2), 3, &plaque, rgb(40, 24, 10), Some(rgb(186, 146, 70)));
}

// ---- the caller's card ---------------------------------------------------------

/// What a caller has read: books finished, and bookmarks (the line each
/// unfinished one was left at).
#[derive(Default, Serialize, Deserialize)]
struct Card {
    finished: Vec<String>,
    marks: Vec<(String, usize)>,
}

impl Session {
    fn card_key(&self) -> String {
        format!("{}:card", self.user.id)
    }

    fn card(&self) -> Card {
        self.state.sys.script_get(STORE, &self.card_key()).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
    }

    fn card_save(&self, c: &mut Card) {
        // Kept to a size: the oldest go first.
        let n = c.finished.len();
        c.finished.drain(..n.saturating_sub(2000));
        let n = c.marks.len();
        c.marks.drain(..n.saturating_sub(200));
        if let Ok(v) = serde_json::to_string(c)
            && let Err(e) = self.state.sys.script_set(STORE, &self.card_key(), Some(&v))
        {
            tracing::error!("library: saving {}'s card: {e}", self.user.handle);
        }
    }

    /// `AB`: the reading room. A SysOp's `AB new` chooses today's shelf
    /// again.
    pub async fn library(&mut self, param: &str) -> io::Result<()> {
        let acs = self.state.config().library.acs.clone();
        if !self.acs(&acs) {
            return self.say_line("LibraryNoAccess").await;
        }
        let again = self.is_sysop() && param.trim().eq_ignore_ascii_case("new");
        if again || self.plain() {
            self.newline();
            self.say_line("LibraryChoosing").await?;
            self.flush().await?;
        }
        let shelf = match today(self.state.clone(), again).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("library: choosing the shelf: {e:#}");
                return self.say_line("LibraryEmpty").await;
            }
        };
        // Only the books the caller may open.
        let fa = self.state.fareas();
        let visible: HashSet<String> = self.visible_fareas().iter().filter_map(|&(n, _)| fa.get(n).map(|a| a.file.to_ascii_uppercase())).collect();
        drop(fa);
        let shelf = Shelf { books: shelf.books.into_iter().filter(|b| visible.contains(&b.area.to_ascii_uppercase())).collect(), ..shelf };
        if shelf.books.is_empty() {
            return self.say_line("LibraryEmpty").await;
        }
        tracing::info!("{}: library ({})", self.user.handle, shelf.theme);
        if self.plain() || self.screen.rows() < 24 {
            return self.library_plain(&shelf).await;
        }
        let mut pick = 0usize;
        let who = { let n = self.state.config().ai.name.trim().to_string(); if n.is_empty() { "the librarian".to_string() } else { n } };
        // Her note's up when the room opens, if she's written one.
        let note_lines = |shelf: &Shelf| -> Vec<String> {
            let text = if shelf.penny.is_empty() { format!("Tonight: {}. {}", shelf.theme, shelf.note) } else { shelf.penny.clone() };
            crate::multinode::wrap(&text, 46).into_iter().take(8).collect()
        };
        let mut overlay = (!shelf.penny.is_empty()).then(|| Overlay::Note(note_lines(&shelf)));
        let mut message: Option<Vec<(char, Ink)>> =
            shelf.penny.is_empty().then(|| painted(&format!("tonight: {}. {}", shelf.theme.to_lowercase(), shelf.note), [235, 200, 140]));
        let mut shelf = shelf;
        loop {
            self.node.update(|n| n.activity = "In the reading room".into());
            // The note may have come since.
            if shelf.penny.is_empty()
                && let Ok(s) = today(self.state.clone(), false).await
                && s.day == shelf.day
            {
                shelf.penny = s.penny;
            }
            let card = self.card();
            let counts = read_counts(&self.state);
            let today_counts: HashMap<String, usize> = counts
                .iter()
                .filter(|r| r.day == shelf.day)
                .map(|r| (format!("{}/{}", r.area, r.name), r.today.len()))
                .collect();
            let looks: Vec<Look> = shelf
                .books
                .iter()
                .map(|b| {
                    let k = b.key();
                    Look { picked: false, finished: card.finished.contains(&k), marked: card.marks.iter().any(|m| m.0 == k) }
                })
                .collect();
            let mut st = self.stage_open();
            self.library_status(&st, &shelf, &card);
            // What the message line is about: a word stays up until the
            // bar moves, then it's the book under the bar.
            let mut line_for = None;
            if let Some(s) = message.take() {
                self.stage_line(MSG_ROW, &s, st.truecolor);
                line_for = Some(pick);
            }
            let since = Instant::now();
            let go: Option<Book> = loop {
                if line_for != Some(pick) {
                    let n = today_counts.get(&shelf.books[pick].key()).copied().unwrap_or(0);
                    let line = self.book_line(&shelf, pick, &looks, n);
                    self.stage_line(MSG_ROW, &line, st.truecolor);
                    line_for = Some(pick);
                }
                let t = st.t();
                draw_room(&mut st.cv, t, &shelf, pick, &looks, &shelf.theme);
                if let Some(o) = &overlay {
                    draw_overlay(&mut st.cv, o, &who);
                }
                self.stage_show(&mut st, true).await?;
                self.flush().await?;
                if !self.key_waiting().await? {
                    tokio::time::sleep(self.idle_pause(90)).await;
                    if since.elapsed() > Duration::from_secs(900) {
                        break None;
                    }
                    continue;
                }
                let key = self.read_key().await?;
                match &mut overlay {
                    Some(Overlay::Note(_)) => {
                        overlay = None;
                        continue;
                    }
                    Some(Overlay::Most(list, sel)) => {
                        let n = list.len();
                        match key {
                            Key::Up | Key::Left => *sel = (*sel + n - 1) % n,
                            Key::Down | Key::Right | Key::Tab => *sel = (*sel + 1) % n,
                            Key::Enter | Key::Char(b' ') => break Some(list[*sel].0.clone()),
                            _ => overlay = None,
                        }
                        continue;
                    }
                    None => {}
                }
                let n = shelf.books.len();
                let per = n.div_ceil(3).max(1);
                match key {
                    Key::Left => pick = (pick + n - 1) % n,
                    Key::Right | Key::Tab => pick = (pick + 1) % n,
                    // Up and down go a shelf at a time, round from the top.
                    Key::Up => pick = if pick >= per { pick - per } else { (pick + per * 2).min(n - 1) },
                    Key::Down => pick = if pick + per < n { pick + per } else { pick % per },
                    Key::Home => pick = 0,
                    Key::End => pick = n - 1,
                    Key::Enter | Key::Char(b' ') => break Some(shelf.books[pick].clone()),
                    Key::Esc => break None,
                    k => match k.upper() {
                        Some('Q') => break None,
                        Some('N') => overlay = Some(Overlay::Note(note_lines(&shelf))),
                        Some('M') => {
                            let list = most_read(&self.state, &visible, 12);
                            if list.is_empty() {
                                let s = crate::editor::strip_colours(self.strings().get("LibraryNoneRead"));
                                self.stage_line(MSG_ROW, &painted(&s, [235, 200, 140]), st.truecolor);
                                line_for = Some(pick);
                            } else {
                                overlay = Some(Overlay::Most(list, 0));
                            }
                        }
                        _ => {}
                    },
                }
            };
            self.stage_close();
            let Some(b) = go else { break };
            message = self.read_book(&b, &shelf).await?;
            overlay = None;
        }
        self.node.update(|n| n.activity = format!("Menu: {}", self.menu.current));
        Ok(())
    }

    /// The line about the book under the bar: its title, and the file.
    fn book_line(&self, shelf: &Shelf, pick: usize, looks: &[Look], readers: usize) -> Vec<(char, Ink)> {
        let b = &shelf.books[pick];
        let mut line = painted(&format!("{}/{}  ", pick + 1, shelf.books.len()), [150, 120, 90]);
        let title = if b.title.is_empty() { b.name.clone() } else { b.title.clone() };
        let read = if readers > 0 { format!("  {readers} read today") } else { String::new() };
        let room = 74usize.saturating_sub(line.len() + b.name.len() + read.len());
        line.extend(painted(&title.chars().take(room).collect::<String>(), [245, 225, 185]));
        line.extend(painted(&format!("  {}", b.name), [140, 120, 100]));
        line.extend(painted(&read, [230, 170, 90]));
        match looks.get(pick) {
            Some(l) if l.finished => line.extend(painted(" *", [255, 215, 90])),
            Some(l) if l.marked => line.extend(painted(" >", [220, 60, 60])),
            _ => {}
        }
        line
    }

    fn library_status(&mut self, st: &Stage, shelf: &Shelf, card: &Card) {
        let read = shelf.books.iter().filter(|b| card.finished.contains(&b.key())).count();
        let (dim, lit) = ([170, 140, 110], [255, 215, 120]);
        // Pieces that go whole or not at all.
        let mut parts: Vec<Vec<(String, Ink)>> = vec![
            vec![("read ".into(), dim), (format!("{read}/{}", shelf.books.len()), lit), (" today".into(), dim)],
            vec![("  ".into(), dim), (card.finished.len().to_string(), lit), (" in all".into(), dim)],
        ];
        // Others in the room.
        let me = self.node.number();
        let others: Vec<String> = self
            .state
            .nodes
            .online()
            .into_iter()
            .filter(|(n, i)| *n != me && !i.invisible && !i.user.is_empty() && (i.activity == "In the reading room" || i.activity.starts_with("Reading ")))
            .map(|(_, i)| i.user)
            .take(3)
            .collect();
        if !others.is_empty() {
            // Before the count of all, which gives way first.
            parts.insert(1, vec![("  here: ".into(), dim), (others.join(", "), [230, 200, 150])]);
        }
        let keys = self.strings().get("LibraryKeys").to_string();
        let len = self.visible_len(&fluxbbs_term::cp437::encode(&keys));
        // What's on the left stops short of the keys.
        let mut room = 80usize.saturating_sub(len + 3);
        let mut out = format!("\x1b[{STATUS_ROW};1H\x1b[0m\x1b[K ").into_bytes();
        for piece in parts {
            let n: usize = piece.iter().map(|(t, _)| t.chars().count()).sum();
            if n > room {
                continue;
            }
            room -= n;
            for (text, c) in piece {
                ink(&mut out, c, st.truecolor);
                out.extend(fluxbbs_term::cp437::encode(&text));
            }
        }
        out.extend(b"\x1b[0m");
        self.screen.write_bytes(&out);
        self.screen.goto(80u16.saturating_sub(len as u16).max(1), STATUS_ROW);
        self.write_with_controls(&keys);
        self.screen.write_bytes(b"\x1b[0m");
    }

    /// Open a book at the caller's bookmark; reading to the end finishes
    /// it, leaving it sooner marks the place. Returns what to say after.
    async fn read_book(&mut self, b: &Book, shelf: &Shelf) -> io::Result<Option<Vec<(char, Ink)>>> {
        let fa = self.state.fareas();
        let Some(area) = fa.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&b.area)).cloned() else { return Ok(None) };
        drop(fa);
        let path = self.farea_dir(&area).join(&b.name);
        if !path.is_file() {
            return Ok(Some(painted(&format!("{} has gone missing from the shelf.", b.name), [200, 150, 140])));
        }
        let grid = self.text_grid(&path, &b.name)?;
        let key = b.key();
        let mut card = self.card();
        let start = card.marks.iter().find(|m| m.0 == key).map_or(0, |m| m.1);
        self.node.update(|n| n.activity = format!("Reading {} in the reading room", b.name));
        tracing::info!("{}: library: reading {}", self.user.handle, key);
        count_read(&self.state, b, self.user.id);
        let title = if b.title.is_empty() { b.name.clone() } else { format!("{} ({})", b.title, b.name) };
        let paged = self.page_grid_from(&title, &grid, "ViewerHelp", &[], start).await?;
        card.marks.retain(|m| m.0 != key);
        let said = if !paged.seen_end && paged.top == 0 {
            // Put back unread: nothing to say.
            self.card_save(&mut card);
            return Ok(None);
        } else if paged.seen_end {
            if !card.finished.contains(&key) {
                card.finished.push(key);
            }
            let read = shelf.books.iter().filter(|b| card.finished.contains(&b.key())).count();
            self.strings().fmt("LibraryFinished", &[&b.name, &read.to_string(), &shelf.books.len().to_string()])
        } else {
            if paged.top > 0 {
                card.marks.push((key, paged.top));
            }
            self.strings().fmt("LibraryMarked", &[&b.name, &(paged.top + 1).to_string()])
        };
        self.card_save(&mut card);
        Ok(Some(painted(&crate::editor::strip_colours(&said), [235, 205, 150])))
    }

    async fn library_plain(&mut self, shelf: &Shelf) -> io::Result<()> {
        loop {
            self.newline();
            self.write(&format!("The reading room. Tonight: {}. {}", shelf.theme, shelf.note));
            self.newline();
            if !shelf.penny.is_empty() {
                self.write(&format!("A note on the mantel: {}", shelf.penny));
                self.newline();
            }
            let card = self.card();
            for (i, b) in shelf.books.iter().enumerate() {
                let done = if card.finished.contains(&b.key()) { "*" } else { " " };
                let title = if b.title.is_empty() { b.name.clone() } else { b.title.clone() };
                let line = format!("{done}{:>2}. {}", i + 1, title);
                self.write(&line.chars().take(79).collect::<String>());
                self.newline();
            }
            self.say("LibraryPlainPrompt").await?;
            let s = self.read_string(crate::input::ReadOpts::new(3)).await?;
            self.newline();
            match s.trim().parse::<usize>() {
                Ok(n) if (1..=shelf.books.len()).contains(&n) => {
                    if let Some(said) = self.read_book(&shelf.books[n - 1], shelf).await? {
                        let line: String = said.iter().map(|c| c.0).collect();
                        self.write(&line);
                        self.newline();
                    }
                }
                _ => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(area: &str, top: &str, name: &str, desc: &str) -> Candidate {
        Candidate {
            area: area.into(),
            top: top.into(),
            name: name.into(),
            title: desc.into(),
            text: format!("{} {}", name.to_lowercase(), desc.to_lowercase()),
            areas_text: area.to_lowercase(),
            path: PathBuf::new(),
        }
    }

    #[test]
    fn dates_take_their_days() {
        let d = |m, dd| NaiveDate::from_ymd_opt(2026, m, dd).unwrap();
        assert!(on_date(&["10-24..10-31".into()], d(10, 31)));
        assert!(!on_date(&["10-24..10-31".into()], d(11, 1)));
        assert!(on_date(&["12-28..01-03".into()], d(1, 2)), "across the new year");
        assert!(on_date(&["11-02".into()], d(11, 2)));
        let t = themes(&Library::default());
        assert_eq!(t[theme_order(&t, d(10, 30))[0]].name, "Ghost stories");
        assert_eq!(t[theme_order(&t, d(11, 2))[0]].name, "Hackers");
        // Undated days go round: the next day, another theme.
        assert_ne!(theme_order(&t, d(6, 10))[0], theme_order(&t, d(6, 11))[0]);
    }

    #[test]
    fn words_match_at_the_start_of_words() {
        assert!(has_word("the phreaking manual", "phreak"));
        assert!(!has_word("ultraphreak", "phreak"));
        assert!(has_word("ufo: roswell", "roswell"));
        assert!(!has_word("graham", "ham"));
    }

    #[test]
    fn a_shelf_is_a_mix() {
        let mut cands = Vec::new();
        for i in 0..20 {
            cands.push(cand("TF_PHREAK_BOXES", "TF_PHREAK", &format!("box{i}.txt"), &format!("Blue box plans part {i}")));
        }
        for i in 0..5 {
            cands.push(cand("TF_LAW", "TF_LAW", &format!("bust{i}.txt"), &format!("Phone phreak busted, {i}")));
        }
        cands.push(cand("TF_FOOD", "TF_FOOD", "chili.txt", "Texas chili"));
        let theme = themes(&Library::default()).into_iter().find(|t| t.name == "Phone phreaks").unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let picked = pick_books(&cands, &theme, day, 9, &mut |_| true);
        assert_eq!(picked.len(), 9);
        assert!(picked.iter().all(|&i| cands[i].name != "chili.txt"));
        assert!(picked.iter().any(|&i| cands[i].area == "TF_LAW"), "not all from one directory");
        assert_eq!(picked, pick_books(&cands, &theme, day, 9, &mut |_| true), "the same all day");
        // A file that doesn't read well stays off.
        let picked = pick_books(&cands, &theme, day, 30, &mut |i| cands[i].name != "bust0.txt");
        assert!(picked.iter().all(|&i| cands[i].name != "bust0.txt"));
    }

    /// `ROOM_OUT=room.ans cargo test --features full -- --ignored library::tests::room --nocapture`
    /// draws the room (a frame at `ROOM_T` seconds) for `tools/ansi2png.py`.
    #[test]
    #[ignore]
    fn room() {
        let Ok(out) = std::env::var("ROOM_OUT") else { return };
        let t: f32 = std::env::var("ROOM_T").ok().and_then(|t| t.parse().ok()).unwrap_or(1.0);
        let books = (0..24).map(|i| Book { area: "A".into(), name: format!("book{i}.txt"), title: format!("Book {i}") }).collect();
        let shelf = Shelf { day: String::new(), theme: "Phone phreaks".into(), note: String::new(), books, penny: String::new() };
        let mut looks = vec![Look::default(); 24];
        looks[3].finished = true;
        looks[9].marked = true;
        let mut cv = Canvas::new(W, H);
        draw_room(&mut cv, t, &shelf, 5, &looks, &shelf.theme);
        match std::env::var("ROOM_CARD").as_deref() {
            Ok("note") => {
                let text = "Pull up a chair, the kettle's on. Tonight it's the phone network and the kids who knew it better than Ma Bell. Start with The Black Box and mind the operator.";
                draw_overlay(&mut cv, &Overlay::Note(crate::multinode::wrap(text, 46)), "Penny");
            }
            Ok("most") => {
                let list = (0..8).map(|i| (Book { area: "A".into(), name: format!("b{i}"), title: format!("A much-read textfile number {i}") }, 12 - i, i % 3)).collect();
                draw_overlay(&mut cv, &Overlay::Most(list, 2), "Penny");
            }
            _ => {}
        }
        let mut enc = crate::pixels::Encoder::new(true, W, H);
        let mut bytes = b"\x1b[2J".to_vec();
        bytes.extend(enc.frame(&cv, 1));
        bytes.extend(b"\x1b[0m\x1b[24;1H");
        std::fs::write(out, bytes).unwrap();
    }

    #[test]
    fn the_shelves_hold_the_books() {
        for n in [1, 15, 24, 30, 33] {
            let s = slots(n);
            assert_eq!(s.len(), n);
            assert!(s.iter().all(|&(x, _)| x >= SHELF_X && x + 2 <= 78));
            let mut seen = HashSet::new();
            assert!(s.iter().all(|p| seen.insert(*p)));
        }
    }
}
