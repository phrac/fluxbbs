# Development

## Layout

- `crates/iniquity-term`: CP437, DOS colors, the ANSI cursor/color
  model and the display-code parser. No I/O.
- `crates/iniquity-legacy`: readers for the Turbo Pascal data files,
  checked byte-for-byte against a stock install.
- `crates/iniquity`: the server.
  - Connections: `ssh.rs`, `telnet.rs`, `guard.rs` (limits and bans),
    `challenge.rs` (bot check), `nodes.rs`, `session.rs` (output and the
    display engine), `input.rs`.
  - Board: `logon.rs`, `engine.rs` and `commands.rs` (menus), `acs.rs`,
    `mci.rs`, `messages.rs`, `editor.rs`, `msgbase.rs`, `files.rs`,
    `filebase.rs`, `transfer.rs`, `extras.rs`, `sysdata.rs`,
    `infoform.rs`, `multinode.rs`, `users.rs`.
  - SysOp: `cfgedit.rs` (the editors' screen, from CONFIG.PAS),
    `sysop.rs`, `useredit.rs`, `sysmisc.rs`, `menuedit.rs`,
    `sysconfig.rs`, `extprog.rs`, `themes.rs`, `wfc.rs` (the
    waiting-for-caller screen).
  - `import.rs`: the legacy importer.
- `menus/`, `text/`, `themes/`, `*.toml`: the stock r5 setup, imported.
- `crates/iniquity/assets/strings.toml`: the stock prompt strings, built
  in.
- [ARTWORK.md](ARTWORK.md): checklist of screens to replace with custom art.
- `tools/ansi2png.py`: renders an ANSI file to PNG for previews.
- `tools/menuscreen.py`: makes menu screens (and the user config forms,
  the lister frame and the file area list) in one style from the menus,
  with a shared `|SA` header.
- `tools/drive_ssh.py`, `tools/drive_telnet.py`: scripted callers for
  testing a running board (see `CLAUDE.md`).
- `CLAUDE.md`: notes for picking the work up (conventions, testing).
- `original/`: the original Turbo Pascal source and the 1.00a26r5 release,
  for reference (© the Iniquity authors); `tools/setup_host.sh` unpacks the
  stock install from it into `original/stock/`.

## Tests

    cargo test       # unit tests, plus the legacy readers once
                     # tools/setup_host.sh has unpacked original/stock
    cargo clippy --all-targets
