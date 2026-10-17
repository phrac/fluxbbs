# Working on this repo

A Rust port of the Iniquity BBS (Turbo Pascal, 1.00a26r5). `README.md` is
the source of truth for what's done and **what's left to port** (the table
under "What's left to port"); keep it current when features land.

## Where things are

- The original Pascal source and a stock install are **not** in this repo.
  On the first host they were at `~/code/inq/source` (the `.PAS` files) and
  `~/code/inq/inq26r5.zip` (the stock r5 install, unpacked for tests). Ask
  for them if they aren't on this host; port from the source, not memory.
- Some original units are missing or corrupt in that source: `autosig.pas`
  is a TheDraw screen, `faDownloadAny` and `maPackArea` aren't there, and
  `NODES.PAS` has a block repeated. Those features were reconstructed from
  strings, menus, records and callers; say so when doing the same.
- Turbo Pascal files are CP437 with CRLF: read them with
  `iconv -f cp437 -t utf-8 FILE.PAS | tr -d '\r'`.

## How the user likes to work

- Commit only when asked. They usually say "commit and continue"; commit,
  push (`origin` is the private `github.com/phrac/iniquity`), carry on.
- Follow the original's behaviour and names (`cfgEditFileArea`, strings by
  their `str*` names), but modernise where DOS-era design doesn't fit (TOML
  and SQLite instead of `.DAT`, directories instead of packed `.TFL`, SSH
  and telnet instead of modems). Explain such choices briefly.
- Fix real hazards in stock behaviour rather than copying them (e.g. the
  stock NEWUSER menu auto-validated everyone outside area code 902; level
  "lower" wrapped to the top level). Mention when you deviate.
- Don't make ANSI art for them; they supply their own (see
  `docs/ARTWORK.md`). Their main menu is a 24-bit colour graffiti logo with
  a generated command list.
- This board: handles kept as typed (`phrac`, not `Phrac`), new users asked
  only for handle and password, bot protection on SSH and telnet.

## Conventions

- `cargo clippy --all-targets` must be clean. **Don't run `cargo fmt`**: the
  code has never been rustfmt'd, and a format pass would rewrite every file.
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
- New prompt strings go at the end of `crates/iniquity/assets/strings.toml`
  under a name, so SysOps can change them with `*E`.

## Testing

    cargo test
    INIQUITY_STOCK=/path/to/stock/install cargo test   # legacy readers too
    cargo clippy --all-targets

Test features **live** as well: run the server (`cargo build`, then
`./target/debug/iniquity` from the repo root; logs to stderr) and drive it
with the scripted callers in `tools/`:

    python3 tools/drive_ssh.py sysop syncterm 'in order=>@solve' \
        'press enter=>\r' 'logon to bbs=>\r' 'Login: sysop=>\r' \
        '(?i)password=>SYSOP\r' 'system\s*:=>SYSTEM\r' ...
    python3 tools/drive_telnet.py 127.0.0.1:2323 'in order=>@solve' ...

Each step is `regex=>keys`: wait for the regex in the de-ANSI'd output, then
send. `opt:` makes a step optional, `long:` waits 45s, `@solve` passes the
bot check, `!cmd` runs a program on the connection (e.g. `rz`/`sz` for
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
  `data/iniquity.db` and the `*.toml` files (git restores tracked ones;
  editors create `levels.toml`, `strings.toml`, `themes.toml` etc., which
  should be removed again if a test made them).
- In zsh, an unmatched glob aborts the whole command; and `pkill -f` with a
  pattern that also appears in your own command line kills your shell.

## Next

Doors (`DOORS.PAS`): drop files (DOOR.SYS, DORINFO1.DEF, CHAIN.TXT) and
running a door with the caller's connection as stdin/stdout, or a virtual
FOSSIL for DOS doors under DOSBox/dosemu. Then the rest of the README's
"What's left to port" table.
