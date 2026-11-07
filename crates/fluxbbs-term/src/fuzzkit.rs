//! Test-only: a small mutation fuzzer for `#[ignore]`d `fuzz_*` tests,
//! with no nightly or cargo-fuzz. Inputs come from a seed corpus,
//! mutated by a deterministic PRNG (`FUZZ_SEED`), `FUZZ_ITERS` times; a
//! panic or a slow call (`FUZZ_SLOW_MS`, 200 by default) is recorded with
//! its input, and a call stuck for `FUZZ_HANG_S` (10) aborts the run
//! after printing it. `FUZZ_SECS` caps a target's time. The fluxbbs
//! crate includes this file by path.
#![allow(dead_code)]

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0 to n-1 (0 if n is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    /// True one time in `n`.
    pub fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }

    /// A number, often a boundary one.
    pub fn number(&mut self) -> u64 {
        const EDGES: [u64; 14] = [0, 1, 2, 7, 8, 80, 127, 128, 255, 256, 32767, 65535, 4294967295, u64::MAX];
        match self.below(4) {
            0 => *self.pick(&EDGES),
            1 => EDGES[self.below(EDGES.len())].wrapping_add(self.below(3) as u64).wrapping_sub(1),
            2 => self.below(1000) as u64,
            _ => self.next() >> self.below(64),
        }
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub fn iters(default: u64) -> u64 {
    env_u64("FUZZ_ITERS", default)
}

/// Bytes worth splicing in anywhere: control and CP437 high bytes,
/// escape and telnet starts, invalid UTF-8.
const BYTES: &[&[u8]] = &[
    b"\x1b", b"\x1b[", b"\x1b[0m", b"\x1b[1;31m", b"\x1b[38;5;", b"\x1b[48;2;", b"\x1b[2J", b"\x1b[K", b"\x1b[s", b"\x1b[u", b"\x1b[6n",
    b"\x1b[24;80R", b"\x1b[=3;16;8n", b"\x1b[?1;2c", b"\x1bP1$r", b"\x1b\\", b"\x1bO", b"\x1b[5~", b"\r", b"\n", b"\r\n", b"\r\0", b"\t",
    b"\x08", b"\x7f", b"\0", b"\xff", b"\xff\xff", b"\xff\xfa", b"\xff\xf0", b"\xff\xfb", b"\xc3", b"\xe2\x98", b"\xf0\x9f\x98\x80",
    b"\xc0\x80", b"\xed\xa0\x80", b"\xf4\x90\x80\x80", b"\xb0\xb1\xb2\xdb", b"\x1a", b";", b"|", b"%", b"~", b"^", b"\\", b"[", b"]",
    b"`", b"*", b"_", b"#", b"<", b">", b"&", b"&#", b"&#x", b"\"", b"'", b"/", b"\xe9", b"\xcd",
];

/// Mutate `base` with a few random edits; `dict` holds tokens that mean
/// something to the target.
pub fn mutate(rng: &mut Rng, base: &[u8], others: &[Vec<u8>], dict: &[&[u8]], max_len: usize) -> Vec<u8> {
    let mut v = base.to_vec();
    let edits = 1 + rng.below(8);
    for _ in 0..edits {
        let len = v.len();
        let at = rng.below(len + 1);
        match rng.below(14) {
            0 if len > 0 => {
                let i = rng.below(len);
                v[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                v[i] = rng.next() as u8;
            }
            2 => v.insert(at, rng.next() as u8),
            3 if len > 0 => {
                let n = 1 + rng.below((len - at.min(len - 1)).min(64));
                let a = at.min(len - 1);
                v.drain(a..(a + n).min(len));
            }
            4 | 5 => {
                let tok: &[u8] = if !dict.is_empty() && rng.one_in(2) { rng.pick(dict) } else { rng.pick(BYTES) };
                v.splice(at..at, tok.iter().copied());
            }
            6 if len > 0 => {
                // A chunk repeated, sometimes many times.
                let a = rng.below(len);
                let n = 1 + rng.below((len - a).min(32));
                let chunk = v[a..a + n].to_vec();
                let times = if rng.one_in(4) { 1 + rng.below(2000) } else { 1 + rng.below(8) };
                let mut add = Vec::with_capacity(chunk.len() * times);
                for _ in 0..times {
                    add.extend_from_slice(&chunk);
                }
                v.splice(at..at, add);
            }
            7 if !others.is_empty() => {
                let o = rng.pick(others);
                let a = rng.below(o.len() + 1);
                let b = a + rng.below(o.len() - a + 1);
                v.splice(at..at, o[a..b].iter().copied());
            }
            8 => {
                // A number: a digit run replaced, or one put in.
                let n = rng.number().to_string();
                let mut s = at;
                while s > 0 && v[s - 1].is_ascii_digit() {
                    s -= 1;
                }
                let mut e = at;
                while e < len && v[e].is_ascii_digit() {
                    e += 1;
                }
                v.splice(s..e, n.bytes());
            }
            9 => {
                // A long run of one byte.
                let b = if rng.one_in(2) { *rng.pick(&[b' ', b'a', b'\n', b'0', b';', 0x1b, 0xff, 0xdb, b'|', b'*', b'<']) } else { rng.next() as u8 };
                let n = if rng.one_in(8) { 1 + rng.below(20000) } else { 1 + rng.below(300) };
                v.splice(at..at, std::iter::repeat_n(b, n));
            }
            10 if len > 0 => v.truncate(at),
            11 if len > 1 => {
                let a = rng.below(len);
                let b = rng.below(len);
                v.swap(a, b);
            }
            12 => {
                let s = rng.number().to_string();
                v.splice(at..at, s.bytes());
            }
            _ => {
                let b = *rng.pick(&[0x00, 0x1b, 0x7f, 0x80, 0xc0, 0xfe, 0xff, b'\r', b'\n', b' ']);
                v.insert(at, b);
            }
        }
    }
    v.truncate(max_len);
    v
}

/// One fuzz target's settings.
pub struct Target<'a> {
    pub name: &'a str,
    pub corpus: Vec<Vec<u8>>,
    pub dict: Vec<&'a [u8]>,
    pub max_len: usize,
    pub iters: u64,
    /// A call slower than this is a finding (`FUZZ_SLOW_MS` overrides).
    pub slow_ms: u64,
}

impl<'a> Target<'a> {
    pub fn new(name: &'a str, corpus: &[&[u8]]) -> Self {
        Target { name, corpus: corpus.iter().map(|c| c.to_vec()).collect(), dict: Vec::new(), max_len: 16 * 1024, iters: iters(2000), slow_ms: 200 }
    }

    pub fn dict(mut self, d: &[&'a [u8]]) -> Self {
        self.dict = d.to_vec();
        self
    }

    pub fn max_len(mut self, n: usize) -> Self {
        self.max_len = n;
        self
    }

    /// For targets whose bounded work takes longer (decoding pictures).
    pub fn slow_ms(mut self, ms: u64) -> Self {
        self.slow_ms = ms;
        self
    }

    /// Run `f` on each input with its own PRNG (for the target's other
    /// choices); it panics on a broken invariant. A finding fails the
    /// test after the run, with each input printed.
    pub fn run(self, f: impl Fn(&[u8], &mut Rng)) {
        quiet_panics();
        let seed = env_u64("FUZZ_SEED", 1) ^ hash(self.name);
        let slow = Duration::from_millis(env_u64("FUZZ_SLOW_MS", self.slow_ms));
        let mut rng = Rng::new(seed);
        // Inputs that were interesting enough to keep mutating: the corpus
        // and, now and then, a mutant.
        let mut pool = self.corpus.clone();
        // What went wrong, the input, and the seed of its own PRNG.
        let mut findings: Vec<(String, Vec<u8>, u64)> = Vec::new();
        let watch = Watchdog::start(self.name);
        let once = |input: &[u8], seed: u64| -> Option<String> {
            watch.enter(input);
            let r = catch_unwind(AssertUnwindSafe(|| f(input, &mut Rng::new(seed))));
            watch.leave();
            r.err().map(|_| LAST_PANIC.with(|p| p.borrow_mut().take()).unwrap_or_default())
        };
        // `FUZZ_INPUT=file` (and `FUZZ_INPUT_SEED`): run that input alone.
        if let Ok(path) = std::env::var("FUZZ_INPUT") {
            let input = std::fs::read(&path).expect("FUZZ_INPUT");
            let r = once(&input, env_u64("FUZZ_INPUT_SEED", 0));
            assert!(r.is_none(), "{}: {}", self.name, r.unwrap_or_default());
            return;
        }
        let started = Instant::now();
        // `FUZZ_SECS`: stop after that long, whatever the iterations.
        let secs = env_u64("FUZZ_SECS", 0);
        let mut done = 0;
        for i in 0..self.iters {
            if secs > 0 && i % 64 == 0 && started.elapsed().as_secs() >= secs {
                break;
            }
            done += 1;
            let base = if pool.is_empty() || rng.one_in(16) { Vec::new() } else { pool[rng.below(pool.len())].clone() };
            let input = if i < self.corpus.len() as u64 { base } else { mutate(&mut rng, &base, &pool, &self.dict, self.max_len) };
            let sub = rng.next();
            let t = Instant::now();
            let r = once(&input, sub);
            let took = t.elapsed();
            match r {
                Some(msg) => {
                    if findings.len() < 20 && !findings.iter().any(|(m, ..)| *m == msg) {
                        findings.push((msg, input.clone(), sub));
                    }
                }
                None if took > slow && findings.len() < 20 => {
                    // Again, to be sure it wasn't the machine.
                    let t = Instant::now();
                    let _ = once(&input, sub);
                    if t.elapsed() > slow {
                        findings.push((format!("slow: {} ms for {} bytes", took.as_millis(), input.len()), input.clone(), sub));
                    }
                }
                None => {}
            }
            if pool.len() < 256 && rng.one_in(64) {
                pool.push(input);
            } else if pool.len() >= 256 && rng.one_in(64) {
                let k = self.corpus.len() + rng.below(pool.len() - self.corpus.len());
                pool[k] = input;
            }
        }
        eprintln!("fuzz {}: {done} iterations in {:.1}s, {} findings", self.name, started.elapsed().as_secs_f64(), findings.len());
        for (msg, input, seed) in &findings {
            eprintln!("--- {}: {msg}\n    input ({} bytes, FUZZ_INPUT_SEED={seed}): {}", self.name, input.len(), shown(input));
            // A panic, made smaller: pieces taken out while it still
            // panics the same way.
            if !msg.starts_with("slow:") {
                let at = msg.rsplit(" at ").next().unwrap_or_default().to_string();
                let small = minimize(input, |c| once(c, *seed).is_some_and(|m| m.ends_with(&at)));
                eprintln!("    smallest ({} bytes): {}", small.len(), shown(&small));
            }
        }
        assert!(findings.is_empty(), "{}: {} findings", self.name, findings.len());
    }
}

/// The smallest input found (removing pieces, halving their size) that
/// `fails` still holds for; a few thousand tries at most.
pub fn minimize(input: &[u8], fails: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut cur = input.to_vec();
    let mut n = cur.len().div_ceil(2).max(1);
    let mut tries = 0;
    while tries < 4000 {
        let mut i = 0;
        let mut cut = false;
        while i < cur.len() && tries < 4000 {
            let mut c = cur.clone();
            c.drain(i..(i + n).min(cur.len()));
            tries += 1;
            if fails(&c) {
                cur = c;
                cut = true;
            } else {
                i += n;
            }
        }
        if n == 1 && !cut {
            break;
        }
        if !cut {
            n = n.div_ceil(2);
        }
    }
    cur
}

/// An input as a Rust byte string, cut short if it's long.
pub fn shown(b: &[u8]) -> String {
    let cut = &b[..b.len().min(2000)];
    let mut s = String::from("b\"");
    for &c in cut {
        match c {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('"');
    if b.len() > cut.len() {
        s.push_str(&format!(" ... ({} more)", b.len() - cut.len()));
    }
    s
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

thread_local! {
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
    static FUZZING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Panics inside a fuzz run are kept for the report, not printed (other
/// tests' panics print as usual).
fn quiet_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if FUZZING.with(|f| f.get()) {
                let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
                let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
                LAST_PANIC.with(|p| *p.borrow_mut() = Some(format!("{msg} at {at}")));
            } else {
                prev(info);
            }
        }));
    });
    FUZZING.with(|f| f.set(true));
}

/// Aborts the run when one call has been going for `FUZZ_HANG_S`
/// seconds, printing the input it's stuck on.
struct Watchdog {
    current: Arc<Mutex<Vec<u8>>>,
    since: Arc<AtomicU64>,
    busy: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    epoch: Instant,
}

impl Watchdog {
    fn start(name: &str) -> Self {
        let w = Watchdog {
            current: Arc::default(),
            since: Arc::default(),
            busy: Arc::default(),
            done: Arc::default(),
            epoch: Instant::now(),
        };
        let (current, since, busy, done, epoch, name) = (w.current.clone(), w.since.clone(), w.busy.clone(), w.done.clone(), w.epoch, name.to_string());
        let limit = env_u64("FUZZ_HANG_S", 10);
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if busy.load(Ordering::Relaxed) && epoch.elapsed().as_millis() as u64 - since.load(Ordering::Relaxed) > limit * 1000 {
                    let input = current.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    // Straight to stderr: the test harness's capture would
                    // die with the process. The whole input goes to a file.
                    use std::io::Write;
                    let path = std::env::temp_dir().join(format!("fuzz-hang-{name}.bin"));
                    let _ = std::fs::write(&path, &input);
                    let _ = writeln!(std::io::stderr(), "--- {name}: HANG over {limit}s (input in {})\n    input ({} bytes): {}", path.display(), input.len(), shown(&input));
                    std::process::abort();
                }
            }
        });
        w
    }

    fn enter(&self, input: &[u8]) {
        let mut c = self.current.lock().unwrap_or_else(|e| e.into_inner());
        c.clear();
        c.extend_from_slice(input);
        self.since.store(self.epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
        self.busy.store(true, Ordering::Relaxed);
    }

    fn leave(&self) {
        self.busy.store(false, Ordering::Relaxed);
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        FUZZING.with(|f| f.set(false));
    }
}

/// Split `input` at random points, for parsers that keep state between
/// reads.
pub fn chunks<'b>(input: &'b [u8], rng: &mut Rng) -> Vec<&'b [u8]> {
    let mut out = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let n = match rng.below(4) {
            0 => 1,
            1 => 1 + rng.below(8),
            2 => 1 + rng.below(256),
            _ => rest.len(),
        }
        .min(rest.len());
        let (a, b) = rest.split_at(n);
        out.push(a);
        rest = b;
    }
    out
}
