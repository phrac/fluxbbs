//! New: a small fuzzer for the `fuzz_*` tests, which are `#[ignore]`d:
//!
//!     FUZZ_ITERS=200000 nice -n 19 cargo test --release --features full fuzz_ -- --ignored --test-threads=4
//!
//! Each mutates a corpus of good inputs (bit flips, inserts, deletes,
//! splices, cut-offs, numbers and length fields set to edge values, long
//! runs, bad UTF-8, CP437 high bytes) and calls the code under test with
//! each, catching panics and timing it. `FUZZ_SEED` picks the run,
//! `FUZZ_ITERS` how many inputs (else the test's own small count),
//! `FUZZ_SECS` a time limit, `FUZZ_SLOW_MS` what counts as slow (200 ms,
//! 2 s in a debug build), `FUZZ_MAX_ALLOC` the biggest single allocation
//! taken as sane (256 MB), `FUZZ_OUT` where failing inputs are saved (the
//! temp directory's `fluxbbs-fuzz`). A failure is cut down to a small
//! input that still fails, saved, and fails the test.
//! (iniquity-legacy uses this file too, by path.)

#![allow(dead_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static PANIC_AT: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static BIGGEST: Cell<usize> = const { Cell::new(0) };
}

/// Notes the biggest single allocation made while an input runs.
struct Counting;

fn note(size: usize) {
    let _ = BIGGEST.try_with(|b| {
        if size > b.get() {
            b.set(size);
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        note(l.size());
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        note(size);
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// xorshift64*.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// 0..n (0 if n is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    /// One time in `n`.
    pub fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

pub fn seed() -> u64 {
    env_u64("FUZZ_SEED").unwrap_or(0x5EED_F1C5)
}

const BYTES: [u8; 22] = [0, 1, 0x7f, 0x80, 0xff, 0x1a, b'\r', b'\n', 0xe3, 0x1b, b'%', b':', b'/', b'@', b'.', b' ', b'\t', b',', b'-', 0x8d, 0xe1, 0xe2];
const WORDS: [u64; 14] = [0, 1, 2, 0x7f, 0x80, 0xff, 0x100, 0x7fff, 0x8000, 0xffff, 0x1_0000, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff];
const NUMBERS: [&str; 12] = ["0", "1", "-1", "00", "65535", "65536", "2147483648", "4294967295", "4294967296", "18446744073709551615", "18446744073709551616", "99999999999999999999999"];

/// One input from the corpus, changed a few ways.
pub fn mutate(rng: &mut Rng, base: &[u8], corpus: &[Vec<u8>], dict: &[&[u8]], max_len: usize) -> Vec<u8> {
    let mut d = base.to_vec();
    let most = if rng.one_in(4) { 16 } else { 4 };
    let rounds = 1 + rng.below(most);
    for _ in 0..rounds {
        let len = d.len();
        let at = rng.below(len + 1);
        match rng.below(15) {
            0 if len > 0 => {
                let i = rng.below(len);
                d[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                d[i] = rng.next() as u8;
            }
            2 if len > 0 => {
                let i = rng.below(len);
                d[i] = *rng.pick(&BYTES);
            }
            3 if len > 0 => {
                // A number field at an edge, either byte order, 2 or 4 bytes.
                let v = *rng.pick(&WORDS);
                let v = if rng.one_in(3) { v.wrapping_sub(1) } else { v };
                let w = if rng.one_in(2) { 2 } else { 4 };
                let b: Vec<u8> = if rng.one_in(2) { v.to_le_bytes()[..w].to_vec() } else { v.to_be_bytes()[8 - w..].to_vec() };
                let i = rng.below(len);
                for (k, x) in b.into_iter().enumerate() {
                    if let Some(c) = d.get_mut(i + k) {
                        *c = x;
                    }
                }
            }
            4 => {
                let n = 1 + rng.below(16);
                let b = rng.bytes(n);
                d.splice(at..at, b);
            }
            5 if len > 0 => {
                let i = rng.below(len);
                let most = if rng.one_in(8) { len } else { 32 };
                let n = 1 + rng.below((len - i).min(most));
                d.drain(i..i + n);
            }
            6 if len > 0 => {
                // A piece of it again, maybe many times: long lines, many records.
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(256));
                let times = if rng.one_in(8) { 1 + rng.below(2000) } else { 1 + rng.below(4) };
                let piece = d[i..i + n].repeat(times);
                d.splice(at..at, piece);
            }
            7 if len > 0 => d.truncate(rng.below(len)),
            8 if !corpus.is_empty() => {
                let other = rng.pick(corpus);
                let i = rng.below(other.len() + 1);
                let j = i + rng.below(other.len() - i + 1);
                let cut = rng.below(len + 1);
                if rng.one_in(2) {
                    d.truncate(cut);
                    d.extend_from_slice(&other[i..]);
                } else {
                    d.splice(at..at, other[i..j].iter().copied());
                }
            }
            9 if !dict.is_empty() => {
                let w = rng.pick(dict).to_vec();
                d.splice(at..at, w);
            }
            10 => {
                // A decimal number where one is (or anywhere).
                let n = NUMBERS[rng.below(NUMBERS.len())].as_bytes().to_vec();
                let mut i = at;
                while i < len && !d[i].is_ascii_digit() {
                    i += 1;
                }
                let mut j = i;
                while j < len && d[j].is_ascii_digit() {
                    j += 1;
                }
                if i < len { d.splice(i..j, n) } else { d.splice(at..at, n) };
            }
            11 if rng.one_in(4) => {
                let b = *rng.pick(&[b'a', b' ', 0, 0xff, 0xe3, b'\r', b'*', b'(']);
                let n = 256 + rng.below(64 << 10);
                d.splice(at..at, std::iter::repeat_n(b, n));
            }
            12 => {
                let bad: &[u8] = rng.pick(&[&b"\xc0\x80"[..], b"\xff\xfe", b"\xe2\x82", b"\xf0\x9f\x92", b"\xed\xa0\x80", b"\xb0\xb1\xb2\xdb", b"\x80\x9f\x8d", "\u{1f600}".as_bytes(), b"\x1b[2J", b"\x1b[999C"]);
                d.splice(at..at, bad.iter().copied());
            }
            13 if len > 1 => {
                // Two pieces swapped about.
                let i = rng.below(len);
                let j = rng.below(len);
                d.swap(i, j);
            }
            _ => {
                let n = rng.below(4);
                d.splice(at..at, std::iter::repeat_n(*rng.pick(&BYTES), n));
            }
        }
        if d.len() > max_len {
            d.truncate(max_len);
        }
    }
    d
}

fn install_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if ACTIVE.with(Cell::get) {
                let at = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
                let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
                PANIC_AT.with(|p| *p.borrow_mut() = Some((at, msg)));
            } else {
                prev(info);
            }
        }));
    });
}

/// What one input did.
enum Outcome {
    Ok,
    Panic(String, String),
}

/// Run `f` on `data`: how it ended, how long it took, its biggest allocation.
fn call<F: Fn(&[u8])>(f: &F, data: &[u8]) -> (Outcome, Duration, usize) {
    BIGGEST.with(|b| b.set(0));
    PANIC_AT.with(|p| *p.borrow_mut() = None);
    ACTIVE.with(|a| a.set(true));
    let start = Instant::now();
    let r = std::panic::catch_unwind(AssertUnwindSafe(|| f(data)));
    let took = start.elapsed();
    ACTIVE.with(|a| a.set(false));
    let big = BIGGEST.with(Cell::get);
    let out = match r {
        Ok(()) => Outcome::Ok,
        Err(_) => {
            let (at, msg) = PANIC_AT.with(|p| p.borrow_mut().take()).unwrap_or_default();
            Outcome::Panic(at, msg)
        }
    };
    (out, took, big)
}

/// Cut `data` down while `still(input)` holds.
fn minimize(data: &[u8], limit: usize, still: &dyn Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut best = data.to_vec();
    let mut chunk = best.len() / 2;
    let mut tries = 0;
    while chunk >= 1 && tries < limit {
        let mut i = 0;
        let mut cut = false;
        while i < best.len() && tries < limit {
            let mut c = best.clone();
            c.drain(i..(i + chunk).min(c.len()));
            tries += 1;
            if still(&c) {
                best = c;
                cut = true;
            } else {
                i += chunk;
            }
        }
        if !cut {
            chunk /= 2;
        }
    }
    best
}

fn shown(d: &[u8]) -> String {
    let s: String = d.iter().take(200).map(|&b| if (0x20..0x7f).contains(&b) && b != b'\\' { (b as char).to_string() } else { format!("\\x{b:02x}") }).collect();
    if d.len() > 200 { format!("{s}... ({} bytes)", d.len()) } else { s }
}

fn save(name: &str, what: &str, n: usize, data: &[u8]) -> std::path::PathBuf {
    let dir = std::env::var_os("FUZZ_OUT").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("fluxbbs-fuzz"));
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join(format!("{name}-{what}-{n}.bin"));
    let _ = std::fs::write(&p, data);
    p
}

/// Fuzz `f` with mutations of `corpus` (and `dict`'s words put in):
/// `default_iters` inputs unless FUZZ_ITERS says otherwise. Fails if any
/// input panics, is slow, or allocates absurdly.
pub fn run<F: Fn(&[u8])>(name: &str, corpus: Vec<Vec<u8>>, dict: &[&[u8]], default_iters: u64, f: F) {
    run_with(name, corpus, dict, default_iters, 1 << 20, f)
}

/// As [`run`], failing only on panics: for code allowed to be big and
/// slow (a picture as big as the SysOp allows).
pub fn run_panics_only<F: Fn(&[u8])>(name: &str, corpus: Vec<Vec<u8>>, dict: &[&[u8]], default_iters: u64, f: F) {
    run_inner(name, corpus, dict, default_iters, 1 << 20, false, f)
}

pub fn run_with<F: Fn(&[u8])>(name: &str, corpus: Vec<Vec<u8>>, dict: &[&[u8]], default_iters: u64, max_len: usize, f: F) {
    run_inner(name, corpus, dict, default_iters, max_len, true, f)
}

fn run_inner<F: Fn(&[u8])>(name: &str, corpus: Vec<Vec<u8>>, dict: &[&[u8]], default_iters: u64, max_len: usize, bounded: bool, f: F) {
    install_hook();
    assert!(!corpus.is_empty(), "{name}: empty corpus");
    let iters = env_u64("FUZZ_ITERS").unwrap_or(default_iters);
    let secs = env_u64("FUZZ_SECS").map(Duration::from_secs);
    let slow = if bounded { Duration::from_millis(env_u64("FUZZ_SLOW_MS").unwrap_or(if cfg!(debug_assertions) { 2000 } else { 200 })) } else { Duration::MAX };
    let max_alloc = if bounded { env_u64("FUZZ_MAX_ALLOC").unwrap_or(256 << 20) as usize } else { usize::MAX };
    // Each test its own stream, whatever order they run in.
    let mut rng = Rng::new(seed() ^ name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)));
    let started = Instant::now();
    // Unique panics by place: the smallest input for each.
    let mut panics: Vec<(String, String, Vec<u8>, usize)> = Vec::new();
    let mut slowest: Option<(Duration, Vec<u8>)> = None;
    let mut biggest: Option<(usize, Vec<u8>)> = None;
    let mut last = corpus[0].clone();
    let mut done = 0u64;
    for i in 0..iters {
        if secs.is_some_and(|s| started.elapsed() >= s) {
            break;
        }
        done = i + 1;
        let data = if i < corpus.len() as u64 {
            corpus[i as usize].clone()
        } else if rng.one_in(3) {
            // Stacked on the last one: further from good input.
            mutate(&mut rng, &last, &corpus, dict, max_len)
        } else {
            let base = rng.pick(&corpus).clone();
            mutate(&mut rng, &base, &corpus, dict, max_len)
        };
        let (out, took, big) = call(&f, &data);
        if let Outcome::Panic(at, msg) = out {
            match panics.iter_mut().find(|p| p.0 == at) {
                Some(p) => {
                    p.3 += 1;
                    if data.len() < p.2.len() {
                        p.2 = data.clone();
                    }
                }
                None => panics.push((at, msg, data.clone(), 1)),
            }
        }
        if took > slow && slowest.as_ref().is_none_or(|s| took > s.0) {
            slowest = Some((took, data.clone()));
        }
        if big > max_alloc && biggest.as_ref().is_none_or(|b| big > b.0) {
            biggest = Some((big, data.clone()));
        }
        last = if data.len() > max_len / 2 { rng.pick(&corpus).clone() } else { data };
    }
    let mut report = Vec::new();
    for (n, (at, msg, data, count)) in panics.iter().enumerate() {
        let small = minimize(data, 4000, &|d| matches!(call(&f, d).0, Outcome::Panic(ref a, _) if a == at));
        let p = save(name, "panic", n, &small);
        report.push(format!("panic at {at} ({count}x): {msg}\n  input ({} bytes, {}): {}", small.len(), p.display(), shown(&small)));
    }
    if let Some((took, data)) = &slowest {
        let small = minimize(data, 100, &|d| call(&f, d).1 > slow);
        let p = save(name, "slow", 0, &small);
        report.push(format!("slow: {took:?}\n  input ({} bytes, {}): {}", small.len(), p.display(), shown(&small)));
    }
    if let Some((big, data)) = &biggest {
        let small = minimize(data, 400, &|d| call(&f, d).2 > max_alloc);
        let p = save(name, "alloc", 0, &small);
        report.push(format!("allocated {big} bytes at once\n  input ({} bytes, {}): {}", small.len(), p.display(), shown(&small)));
    }
    eprintln!("{name}: {done} inputs in {:.1?} (seed {:#x})", started.elapsed(), seed());
    assert!(report.is_empty(), "{name}:\n{}", report.join("\n"));
}
