# Iniquity (Rust)

A port of the Iniquity BBS (Mike Fricker and the Iniquity Development
Team, 1995-97, Turbo Pascal) to Rust, with SSH in place of modems and
FOSSIL drivers. It runs the stock 1.00a26r5 screens, menus and strings.

    cargo run                                # serves iniquity.toml on :2222
    ssh -p 2222 anyname@localhost            # UTF-8 terminal
    syncterm ssh://anyname@localhost:2222    # raw CP437

The stock SysOp account is `SysOp` / `SYSOP`, with system password
`SYSTEM` (asked of SL 250+). Type `new` at the login prompt, or pick
"apply as new" on the matrix, to create an account.

## Importing an existing Iniquity install

    iniquity import /path/to/old/INIQUITY [dest-dir] [--force]

| from                  | to                          |
|-----------------------|-----------------------------|
| INIQUITY.DAT          | iniquity.toml               |
| DATA\LEVELS.DAT       | iniquity.toml `[levels.X]`  |
| DATA\STRINGS.DAT      | strings.toml                |
| DATA\USERS.DAT        | data/iniquity.db (SQLite)   |
| DATA\MSGAREA.DAT, EMAIL.DAT, MSGCONF.DAT | msgareas.toml |
| MSGS\*.HDR, *.DAT     | data/iniquity.db            |
| MENU\*.MNU            | menus/*.toml                |
| TEXT\*                | text/                       |

Passwords are re-hashed with argon2 on import. They stay case-insensitive,
as in the original.

## Layout

- `crates/iniquity-term`: CP437, DOS colours, the ANSI cursor/colour model,
  and the `|XX` / `%XX` / `@NN` display-code parser. No I/O.
- `crates/iniquity-legacy`: readers for the Turbo Pascal data files,
  checked byte-for-byte against a stock install (`INIQUITY_STOCK=dir
  cargo test -p iniquity-legacy`).
- `crates/iniquity`: the server.
  - `ssh.rs` transport, `session.rs` output and display engine,
    `input.rs` keys/line input/yes-no
  - `logon.rs` matrix, login, new-user application
  - `messages.rs` areas, reader, posting, scans, email; `editor.rs` the
    full-screen editor; `msgbase.rs` message storage
  - `engine.rs` menu engine, `commands.rs` menu commands, `acs.rs` access
    conditions, `mci.rs` MCI codes
  - `users.rs` user database, `import.rs` legacy import
- `menus/`, `text/`, `iniquity.toml`: the stock r5 setup, imported.
- `crates/iniquity/assets/strings.toml`: the stock prompt strings, built in;
  a `strings.toml` beside the config overrides any of them.

## Terminal handling

Callers whose TERM is a BBS terminal (`syncterm`, `ansi-bbs`, ...) get raw
CP437. Everyone else gets UTF-8, with DOS behaviour emulated: hard wrap at
column 80 and `ESC[2J` homing the cursor. Override with `[terminal] charset`.

## Status

Working: SSH (including SyncTERM), nodes, display codes and text files,
repeat screens (TOP/MID/HIL/BOT), the matrix and lightbar menus, login,
new-user application, access levels, the menu engine (hot keys, `/` and
`//` commands, FIRSTCMD, stacked commands, gosub/return, generic menus,
command help), message areas and conferences, the message reader, the
full-screen editor (word wrap, quoting, cut/paste, auto-signatures),
newscans, the mandatory scan, and email.

Commands: `?= ?+ ?/ ?% ?B ?E ?F ?L ?N ?Y ?S ?U`,
`-^ -/ -\ -= -? -C -D -E -F -K -L -P -R -S -W`, the `|` cursor commands,
`>A >H >L`, `HC HI HM`, `MA MC MJ MN MM MP MR MS MZ M[ M]`, `EE ER EC *R`,
`UL UP XL`.

Other commands from the original say "not available yet". Next: file
areas and transfers, then the automessage, oneliners (`?I`/`?T`), last
callers and user configuration.
