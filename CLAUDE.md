# Working on this repo

flux/bbs (`fluxbbs` where a slash can't go: the program, crates, files,
container), a Rust BBS that began as a port of the Iniquity BBS (Turbo
Pascal, 1.00a26r5); credit Iniquity, but it's no longer called that.
The checkout folder is still `inq/iniquity`, and the image keeps the
stock files at `/srv/inq/iniquity`, which boards' configs point at.
`README.md` is
the source of truth for what's done and what isn't (its "Status"
section); keep it current when features land.
`docs/NEW.md` tracks what's new over the original (and what changed on
purpose): add a line there for every feature the original didn't have.

## Setting up a new host

    git clone git@github.com:phrac/fluxbbs.git && cd fluxbbs
    tools/setup_host.sh

That unpacks the stock install into `original/stock/` (git-ignored), builds,
and, if there's no `data/fluxbbs.db` (or older `iniquity.db`), creates one from the stock install
(SysOp / `SYSOP`, system password `SYSTEM`) without touching the committed
config. SSH host keys are made in `data/` on first run. Showing PDFs
needs Poppler (`poppler-utils`; in the Docker image, not on rzn itself,
so test PDFs in a container). Needs `cargo` and
`unzip`; live tests also want `python3` and `ssh`, and Zmodem tests `sz`/`rz`
(lrzsz) on the PATH. The board's real database and uploaded files are not in
git: copy `data/` and `files/` across if the board should move with you.

## Where things are

- `original/source/`: the Turbo Pascal source (`.PAS`, CP437). Port from
  it, not from memory: read the unit before porting a feature.
- `original/inqa26r5.zip`: the 1.00a26r5 release; `original/stock/` is the
  stock install unpacked from it (by `tools/setup_host.sh`), used by the
  legacy tests (`cargo test` uses it when present) and as an
  import source.
- Some original units are missing or corrupt in that source: `autosig.pas`
  is a TheDraw screen, `faDownloadAny` and `maPackArea` aren't there, and
  `NODES.PAS` has a block repeated. Those features were reconstructed from
  strings, menus, records and callers; say so when doing the same.
- Turbo Pascal files are CP437 with CRLF: read them with
  `iconv -f cp437 -t utf-8 FILE.PAS | tr -d '\r'`.
- `crates/fluxbbs-dos`: the DOS machine DOS doors run in (real-mode x86
  core, BIOS, FOSSIL, DOS, batch files). `tests/dos/*.asm` are test
  programs built with nasm (`tests/dos/build.sh`; the `.com` files are
  committed). The CPU core is checked against the SingleStepTests 8088
  suite: unpack its `v2/*.json.gz` and `metadata.json` somewhere and run
  `SST8088_DIR=dir cargo test -p fluxbbs-dos --release --test sst8088
  -- --ignored --nocapture`.
  The 286 and 386 real-mode suites (SingleStepTests/80286, /80386) run the
  same way through `tests/sst286.rs` and `tests/sst386.rs` (their headers
  say how to convert the MOO files; convert one file at a time, as /tmp is
  RAM on rzn). The CPU is a 386 by default; `model_286` gives the 286's
  differences for its suite. At last run: 4 of 1.48M (a 286 IDIV quirk)
  and 1,814 of 1.76M (segment-limit edge cases) fail.
  To try a door outside the BBS, `cargo build --release -p fluxbbs-dos
  --examples` gives `target/release/examples/dosrun C_ROOT D_ROOT CMD`:
  stdin is the caller, stdout what they'd see, the machine's log on
  stderr, and `SCREEN=file` keeps the local text screen; `DOS_TRACE=1`
  logs every DOS/BIOS call, `TRACE_RING=N` records the last N
  instructions for `crates/fluxbbs-dos/tools/replay_trace.py`, which
  replays each one on QEMU's CPU (pip `unicorn`) and reports any
  disagreement (`trace_chunks.py` splits a long trace for parallel
  runs); `DOS_DATE` and `MEM_FILL` fake the date and dirty memory. When
  a door misbehaves, that replay plus a run of the same files under
  DOSBox (BNU as the FOSSIL, its modem emulation for carrier; DOSBox
  0.74-3 needs an INT 21h shim for LORD 4.07's open mode 44h) tells
  ours from the door's own bugs. LORD 4.06 plays through this way and
  as a door; see docs/DOORS.md on 4.07.
- `Dockerfile` and `deploy/docker-compose.yml` run a board in a
  container: the image holds the server and the stock `text/`, `themes/`,
  `menus/`, `scripts/` at `/srv/inq/iniquity`, and the board directory is
  mounted at `/srv/board`. The live board is `disfluxbbs.com` on `rzn`
  (SSH 2222, telnet 23), from `~/code/disflux_bbs` there with the repo at
  `~/code/inq/iniquity`; the test board is the same layout on `ryzen`.

## How the user likes to work

- Commit only when asked. They usually say "commit and continue"; commit,
  push (`origin` is the private `github.com/phrac/fluxbbs`), carry on.
- Follow the original's behavior and names (`cfgEditFileArea`, strings by
  their `str*` names), but modernize where DOS-era design doesn't fit (TOML
  and SQLite instead of `.DAT`, directories instead of packed `.TFL`, SSH
  and telnet instead of modems). Explain such choices briefly.
- Fix real hazards in stock behavior rather than copying them (e.g. the
  stock NEWUSER menu auto-validated everyone outside area code 902; level
  "lower" wrapped to the top level). Mention when you deviate.
- Don't make ANSI art for them; they supply their own (see
  `docs/ARTWORK.md`). Adding a menu's command list or lightbar markers
  (`~01^01`...) under their art is fine.
- Keep the repo stock: their board (disflux) lives in
  `~/code/disflux_bbs`, outside the repo, with its own `iniquity.toml`,
  `data/`, `files/` and `text/` (their screens; `art/` holds the
  originals), using the repo's `text/` as `stock_text` and its menus as
  `stock_menus` (the board's own `menus/` has MATRIX.toml and the
  lightbar menus `art/mainmenu-gen.py` writes beside each menu screen). Many of
  its screens are generated from their art by `art/*-gen.py` (menus,
  lists including last callers, USERSTAT, USERCFG, BOTCHECK, PRELOGON, MATRIX,
  WELCOME, AICHAT, NODECHAT, FSEDITOR and FSHELP (the message editor, in
  the main menu's pink);
  each says how to run it):
  change the script and rerun it rather than editing the `.ANS`.
  Board settings go there, not in the repo's `fluxbbs.toml`: handles
  kept as typed (`alias_format = 1`), new users asked only for handle and
  password. Run it with `fluxbbs ~/code/disflux_bbs/iniquity.toml` (disflux
  predates the rename and keeps `iniquity.toml` and `data/iniquity.db`). The
  repo's own `data/` is just a dev copy.

## Conventions

- `cargo clippy --all-targets` must be clean, both for the core board and
  with `--features full`. **Don't run `cargo fmt`**: the
  code has never been rustfmt'd, and a format pass would rewrite every file.
- Doors, the games, effects, screen savers, AI, FTN, MeshCore, PDFs, Lua
  and the textfiles importer are optional components (Cargo features;
  docs/BUILDING.md). A plain build is the core; disflux, the flagship,
  runs `full` (the compose file and `tools/setup_host.sh` build it).
  Keep new optional work behind a feature, with a stand-in where the
  core calls into it, and check the core still builds on its own.
- Comments are sparse and name the original procedure in backticks
  (`/// \`nodeChat\` (\`XC\`): ...`).
- Live-editable settings follow one pattern: `BbsState` holds
  `RwLock<Arc<T>>`, a getter clones the `Arc`, and `set_*` writes the TOML
  file atomically (`write_atomic`) and swaps it in. Config, strings, areas,
  levels, protocols, archivers, infoforms and themes all work this way.
- SysOp editors use `cfgedit.rs` (`Form`, `cfg_command`, `cfg_text`...),
  ported from CONFIG.PAS; list editors share `list_change` (insert, delete,
  move).
- `Session` isn't `Sync`: never hold `&self` across an `.await` in a method
  that a spawned future uses (take `&mut self`, or copy what you need).
- Locks are taken with `.lock().unwrap_or_else(|e| e.into_inner())`
  (likewise `read`/`write`), never `.unwrap()`: one node's panic while
  holding a lock mustn't take every other node down with it.
- Schema changes go at the end of the store's `STEPS`
  (`sysdata::migrate`, a `Step::Column` or `Step::Sql`); never edit or
  reorder existing steps. `schema_versions` counts each store's steps,
  since every store shares one database file.
- New prompt strings go at the end of `crates/fluxbbs/assets/strings.toml`
  under a name, so SysOps can change them with `*E`.

## Testing

    cargo test --features full   # includes the legacy readers once original/stock exists
    cargo test                   # the core board alone
    cargo clippy --all-targets --features full
    cargo clippy --all-targets

Test features **live** as well: run the server (`cargo build --features
full`, then `./target/debug/fluxbbs` from the repo root; logs to stderr)
and drive it
with the scripted callers in `tools/`:

    python3 tools/drive_ssh.py sysop syncterm 'in order=>@solve' \
        'press enter=>\r' 'logon to bbs=>\r' 'Login: sysop=>\r' \
        '(?i)password=>SYSOP\r' 'system\s*:=>SYSTEM\r' ...
    python3 tools/drive_telnet.py 127.0.0.1:2323 'in order=>@solve' ...

`PORT=3222` points `drive_ssh.py` at a test server while a live board
holds 2222; `SSH_KEY=file` logs in with that key (otherwise it offers
none, not even `~/.ssh`'s), and `SSH_PASSWORD=1` uses password auth
(answer ssh's `password:` prompt in a step). For uploads without lrzsz, `cargo build --release --examples`
gives `target/release/examples/zsend FILE...`, a Zmodem sender on the
same library (`'(?i)initiate=>!/abs/path/zsend a.zip'`, with `XFER_DIR`
as its working directory). Remember `cargo build --release --features
full` for the server itself: `--examples` alone leaves a stale binary,
and a plain build leaves out the optional components.

Each step is `regex=>keys`: wait for the regex in the de-ANSI'd output, then
send. `CPR=1` answers cursor position requests as a terminal does and
`READ_BPS=n` reads the output at n bytes a second (a slow link); either
adds timestamps to the steps. `opt:` makes a step optional, `long:`
waits 45s, `@solve` passes the bot check (the picture one through
`tools/botsolve.py`, which reads the font from `challenge.rs`), `!cmd` runs a program on the connection (e.g. `rz`/`sz` for
Zmodem), and keys understand `\r`, `\e`, `{UP}`/`{DOWN}`/`{LEFT}`/`{RIGHT}`,
`\!` for a literal `!`. `RAW_OUT=file` saves the raw output;
`tools/ansi2png.py` renders it to check layouts.

Things that trip tests up:
- The bot check and per-address limits apply to localhost too: at most 6
  connections a minute and 2 at once, and 3 failures ban for 15 minutes
  (restart the server to clear). Add `127.0.0.1` to `[security] allow`
  locally if that gets in the way, but don't commit it.
- Menus with hot keys act on the key alone; an extra `\r` answers the next
  prompt. Lightbar menus treat `4`/`6` as arrow keys.
- Many screens end in a "press any key" pause; add `opt:any key=>\r` steps.
- **Back up and restore the board's data around live tests**:
  `data/fluxbbs.db` (disflux: `iniquity.db`) and the `*.toml` files (git restores tracked ones;
  editors create `levels.toml`, `strings.toml`, `themes.toml` etc., which
  should be removed again if a test made them).
- In zsh, an unmatched glob aborts the whole command; and `pkill -f` with a
  pattern that also appears in your own command line kills your shell.

## Status, 18 Oct 2026: the board is live, dev moves to rzn

`disfluxbbs.com` runs on `rzn` in Docker (`~/code/inq/iniquity` is the
checkout, `~/code/disflux_bbs` the board, SSH 2222, telnet 23; update with
`git pull && docker compose -f deploy/docker-compose.yml up -d --build`,
logs with `docker logs -f fluxbbs`). A deploy restarts the board and
drops callers: check `curl -s 127.0.0.1:8099/status.json` first and wait
if anyone is on. To hand-edit the board's TOML (`strings.toml`,
`fileareas.toml`...), stop the container first (`docker compose ...
stop`, edit, `up -d`): the running board keeps them in memory and an edit
in `*E`/`*F`/`*C` writes its copy back over yours. Screens in its `text/`
are read on each display and need neither. The board directory was copied from
ryzen (every file hash-checked); the ryzen board still runs and is now a
separate copy, so don't develop or post there. `cargo` on rzn is in
`~/.cargo/bin`; `original/inqa26r5.zip` is in git, so `tools/setup_host.sh`
works there (once the stock install is unpacked; see below).

Fixed 18 Oct: AI chat (the image now has CA certificates, and a client
that can't be built says `AiUnavailable` instead of dropping the caller;
vega and reflux resolve from the container through Tailscale's search
domain, so `[ai]` is fine as it is); `[dos] emulator = ""` on the live
board; the `[ NOW ]` feed (`[web] listen = "0.0.0.0:8099"` in the board,
published as `127.0.0.1:8099` by the compose file, where rzn's host Caddy
already proxies `/api/status.json`). rzn has no `unzip`: unpack
`original/inqa26r5.zip` and the `newinst.exe` inside it with Python's
`zipfile` before running `tools/setup_host.sh`.

Freezes (keys seem ignored), from SyncTERM over SSH on a phone link:
- GIFs: output piled up ahead of the terminal (~450 KB queued on rzn).
  Fixed: each frame asks where the cursor is and the next waits for the
  answer (`sync_output` in `input.rs`, one frame in flight; reports are
  stripped from the input). Use it for anything else that streams fast.
- Logon: still open. Once, the server was at the main menu with nothing
  queued, SyncTERM had acked everything and was still sending keys, but
  showed a logon screen; the main menu had gone in two 16 KB SSH packets.
  Capping packets at 4 KB made everything slow and was reverted. No
  window adjusts ever come from SyncTERM. `flush` now logs "output
  stalled" after 10 s; `RUST_LOG=info,russh=debug docker compose ... up
  -d` shows the packets. Ask the SyncTERM version; try telnet.

Backups (24 Oct 2026): a nightly event (`backup 7`, 03:30) writes
`~/code/disflux_bbs/backups/fluxbbs-*.tar.gz` (database snapshot,
settings, screens, door data, host keys; not `files/`), keeping 7 days.
`fluxbbs backup` makes one by hand; restoring is in docs/SYSOP.md.
Nothing copies them off rzn yet.

## textfiles.com, 22 Oct 2026

`fluxbbs textfiles fetch DIR` / `import CONFIG` (`textfiles.rs`, docs in
`docs/FILES.md`) put the whole archive on a board as areas of its own (the sections
top-level, their directories as sub-areas; no group): 57,909 files in
649 directories from the meulie.net mirror's pages plus the 2007 tarballs
on archives.textfiles.com. A full fetch takes about an hour; the mirror
answers missing paths with a redirect to a 404 site (HTTP 200), which
the fetcher treats as not found. On the live board since 22 Oct (942
areas, 67,395 files, imported with `--put
art=ARTSCENE,hacking=HACK,phreak=PHREAK,bbs=BBS,magazines=ZINES`, so
those sections' files are in the board's own areas and their
directories sub-areas of them; `fileareas.toml.bak-2026-10-22-textfiles` and
`data/iniquity.db.bak-2026-10-22-textfiles` are from just before). To
take the mirror's additions later: fetch in the container beside the
running board, stop it for the import.

## Next

Feature parity with the manual is done bar callers' outgoing netmail
and the "not planned" items in the README. FTN echomail (fsxNet) is
built (`ftn.rs`, `ftntoss.rs`, `binkp.rs`) and tested against binkd in a
container: a `debian:bookworm-slim` image with `binkd` installed, a
config with `address 21:1/100@fsxnet`, `node 21:1/999@fsxnet - SESSPW`
and `domain fsxnet /hub/outbound 21`, packets for the node dropped in its
outbound as `000103e7.out`, and the board polled with `fluxbbs ftn
poll`. For the hub calling in, run binkd with `--network host` (rzn's
firewall drops Docker-to-host connections), give its `node` line
`127.0.0.1:<port>`, set `[ftn] listen` to that port, and drop the packet
as `000103e7.cut` (crash): binkd calls within seconds. The live board isn't a node yet: the SysOp applies to fsxNet for
an address and passwords. Next:
real doors (LORD, TW2002, Usurper, BRE) in the DOS machine, fixing what
they trip over. The MeshCore bridge (`meshcore.rs`, channels to message
areas, ported from the firmware's examples/companion_radio/MyMesh.cpp)
is built and tested against `tools/fake_meshcore.py` only: there's no
radio yet, and rzn has no USB radio (a WiFi companion over the tailnet,
or ser2net, is the plan). New work after that is improvements rather than ports: a
web or terminal SysOp dashboard in place of the local console, for
instance.
