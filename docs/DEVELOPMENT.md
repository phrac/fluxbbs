# Development

How the code is laid out, how to test it, and the conventions to follow.
`CLAUDE.md` at the repo root has more notes for picking the work up.

## Crates

| Crate | What |
|---|---|
| `crates/fluxbbs` | the server |
| `crates/fluxbbs-term` | CP437, DOS colors, the ANSI cursor/color model and the display-code parser; no I/O |
| `crates/fluxbbs-dos` | the DOS machine DOS doors run in: a real-mode x86 core, BIOS, FOSSIL, DOS and batch files |
| `crates/iniquity-legacy` | readers for the Turbo Pascal data files, checked byte-for-byte against a stock install |

## The server's modules

All under `crates/fluxbbs/src/`.

| Area | Modules |
|---|---|
| Connections | `ssh.rs`, `sshkeys.rs`, `telnet.rs`, `guard.rs` (limits and bans), `challenge.rs` (bot check), `nodes.rs`, `session.rs` (output and the display engine), `input.rs` |
| Logon and menus | `logon.rs`, `logonseq.rs` (`*O`), `engine.rs`, `menu.rs` and `commands.rs` (menus), `acs.rs`, `mci.rs`, `password.rs` |
| Messages | `messages.rs`, `msgareas.rs`, `editor.rs`, `msgbase.rs`, `msgcenter.rs`, `boards.rs`, `pushes.rs`, `mentions.rs`, `attach.rs`, `scansel.rs`, `qwk.rs` |
| Files | `files.rs`, `fileareas.rs`, `filebase.rs`, `transfer.rs`, `upltest.rs`, `upscan.rs`, `archview.rs`, `pager.rs`, `pdf.rs`, `textfiles.rs` |
| Gallery | `gallery.rs`, `picture.rs` |
| Users | `user.rs`, `users.rs`, `profile.rs` |
| The rest of the board | `extras.rs`, `sysdata.rs`, `infoform.rs`, `voting.rs`, `timebank.rs`, `multinode.rs`, `chat.rs`, `doors.rs`, `scripts.rs`, `themes.rs`, `strings.rs`, `clock.rs`, `events.rs` |
| AI chat | `aichat.rs`, `aiboard.rs` |
| Networks | `ftn.rs`, `ftntoss.rs`, `binkp.rs` (FTN echomail), `meshcore.rs`, `web.rs` (the website feed) |
| SysOp | `cfgedit.rs` (the editors' screen, from CONFIG.PAS), `sysop.rs`, `useredit.rs`, `sysmisc.rs`, `menuedit.rs`, `sysconfig.rs`, `extprog.rs`, `wfc.rs` (the waiting-for-caller screen) |
| Setup | `main.rs`, `config.rs`, `import.rs` (the legacy importer) |

## Other files

| Path | What |
|---|---|
| `menus/`, `text/`, `themes/`, `*.toml` | the stock r5 setup, imported |
| `crates/fluxbbs/assets/strings.toml` | the stock prompt strings, built in |
| `original/` | the original Turbo Pascal source and the 1.00a26r5 release, for reference (© the Iniquity authors) |
| `original/stock/` | the stock install, unpacked from the release by `tools/setup_host.sh` |
| [ARTWORK.md](ARTWORK.md) | checklist of screens to replace with custom art |

## Tools

| Tool | What |
|---|---|
| `tools/setup_host.sh` | gets a fresh clone ready: unpacks the stock install, builds, makes a starting database |
| `tools/ansi2png.py` | renders an ANSI file to PNG, for previews |
| `tools/menuscreen.py` | makes menu screens (and the user config forms, the lister frame and the file area list) in one style from the menus, with a shared `\|SA` header |
| `tools/drive_ssh.py`, `tools/drive_telnet.py` | scripted callers for testing a running board |
| `tools/botsolve.py` | passes the picture bot check for the scripted callers |
| `tools/fake_meshcore.py` | a pretend MeshCore radio, for trying the bridge without one |

## Tests

    cargo test --features full   # unit tests, plus the legacy readers once
                                 # tools/setup_host.sh has unpacked original/stock
    cargo test                   # the core board alone
    cargo clippy --all-targets --features full
    cargo clippy --all-targets

Both clippy runs must be clean: the core board and the full one
([BUILDING.md](BUILDING.md) has the components, and a loop to check
each on its own). Don't run `cargo fmt`: the
code has never been rustfmt'd, and a format pass would rewrite every
file.

### Live tests

Run the server and drive it with the scripted callers in `tools/`. Each
step is `regex=>keys`: wait for the regex in the de-ANSI'd output, then
send the keys. `CLAUDE.md` lists every option and the things that trip
tests up (the bot check and per-address limits apply to localhost too).

    python3 tools/drive_ssh.py sysop syncterm 'in order=>@solve' ...
    python3 tools/drive_telnet.py 127.0.0.1:2323 'in order=>@solve' ...

Back up `data/` and the `*.toml` files around a live test, and restore
them after.

### The DOS machine

- `crates/fluxbbs-dos/tests/dos/*.asm` are test programs, built with
  nasm by `build.sh` beside them. The `.com` files are committed.
- The CPU core is checked against the SingleStepTests 8088, 80286 and
  80386 real-mode suites (`crates/fluxbbs-dos/tests/sst8088.rs`,
  `sst286.rs`, `sst386.rs`). They are `--ignored` tests; each file's
  header says where to point it and how to convert the data.
- `cargo build --release -p fluxbbs-dos --examples` builds
  `target/release/examples/dosrun C_ROOT D_ROOT CMD`, to try a door
  outside the BBS.
- `crates/fluxbbs-dos/tools/replay_trace.py` replays a recorded trace on
  QEMU's CPU and reports any disagreement.

## Conventions

- Port from the Turbo Pascal source in `original/source/`, not from
  memory. Read the unit before porting a feature. The files are CP437
  with CRLF: `iconv -f cp437 -t utf-8 FILE.PAS | tr -d '\r'`.
- Follow the original's behavior and names (`cfgEditFileArea`, strings
  by their `str*` names). Modernize where DOS-era design doesn't fit.
- Comments are sparse and name the original procedure in backticks.
- New prompt strings go at the end of
  `crates/fluxbbs/assets/strings.toml`, under a name, so SysOps can
  change them with `*E`.
- When a feature lands, update the README's status and add a line to
  [NEW.md](NEW.md) for anything the original didn't have.

### What's left to port

Netmail from callers (the original's `echomail.pas` and
`INIQMAIL.PAS`): echomail and netmail to the board work, but callers
can't yet write netmail to an FTN address.
