# Iniquity BBS, in Rust

A port of **Iniquity** (Mike Fricker and the Iniquity Development Team,
1995–97, Turbo Pascal, version 1.00a26r5) to Rust, for running a BBS on
today's internet. Modems and FOSSIL drivers are replaced by SSH and
telnet, the `.DAT` files by SQLite and TOML, and everything runs in one
async process. The stock screens, menus, prompt strings and behaviour
are kept, so an existing Iniquity board can be imported and look the
same.

## Quick start

    cargo run --release                      # serves iniquity.toml
    ssh -p 2222 anyname@localhost            # UTF-8 terminal
    syncterm ssh://anyname@localhost:2222    # CP437 BBS terminal
    syncterm telnet://localhost:2323         # plain telnet (unencrypted)

Every caller first passes a short bot check (press the keys shown). The
stock SysOp account is `SysOp` / `SYSOP`, with system password `SYSTEM`
(asked of SL 250+). New callers pick "apply as new" on the matrix menu
or type `new` at the login prompt.

SSH host keys (Ed25519, ECDSA and RSA) are made in `data/` on first run.
The database is `data/iniquity.db`; uploaded files live under `files/`.
Neither is in git.

## What's done

**Connections**
- SSH (russh), including the older algorithms SyncTERM's cryptlib needs.
- Telnet: binary, server echo, window size (NAWS) and terminal type;
  Zmodem works over it.
- CP437 for BBS terminals, UTF-8 (with DOS wrap and clear behaviour)
  for everything else; aixterm bright colours and iCE backgrounds; 256
  and 24-bit colour art passes through.
- Nodes, with who's online, node messages, broadcasts and multinode chat.

**Keeping bots out**
- Key-combination bot check before anything else, on SSH and telnet.
- Per-address connection limits, bans for repeated failures (doubling,
  up to a day), allow and deny lists, a short idle limit before login.

**The board**
- Display engine: `|XX` colours and controls, `%XX` MCI codes, `@`/`&`
  positioning, `{`/`}` limits and padding, `~XX` fields, repeat screens
  (TOP/MID/HIL/BOT), random and day-of-week screen variants.
- Menu engine: hot keys, `/` and `//` commands, FIRSTCMD, stacked
  commands, gosub/return, generic and lightbar menus, ACS expressions,
  passwords, fallbacks, command help.
- Logon: the matrix menu, login, system password, new-user application
  (each personal question can be switched off; handles kept as typed),
  access levels and daily limits.
- Messages: areas and conferences, the reader, posting and replying,
  quoting, the full-screen editor, newscans and mandatory messages,
  email, auto-signatures.
- Files: areas and conferences, the lightbar file lister, downloads
  (single and batch, with points, ratios and daily limits), uploads
  (blind or named, with FILE_ID.DIZ import), search and newscan, archive
  viewing, local upload. Zmodem built in; other protocols and archivers
  (RAR, 7Z, LHA via bsdtar/7-Zip) are external programs.
- Oneliners, the automessage, last callers, history and statistics, the
  BBS list, user configuration, user listing.
- Infoforms (questionnaires, mandatory or not) and new user voting.
- Screen themes (the original's text libraries): directories of screens
  that replace the stock ones; callers pick theirs with `UT`.

**SysOp tools** (the `*` commands on the SysOp menu)
- `*C` system configuration, written back to `iniquity.toml` keeping its
  comments; `*E` prompt strings; `*#` menus; `*L` access levels.
- `*U` users (with colours, preferences, auto-signature, infoforms).
- `*F` file areas, `*Y` files (validate, move, extract/apply DIZ), `*S`
  sort areas, `*>` download any file, `*%` local upload.
- `*A` message areas, `*P` pack them, `*@`/`*!` conferences, `*R` all
  email.
- `*X` protocols, `*Z` archivers, `*B` BBS list, `*H` statistics, `*I`
  infoforms, `*T` themes.

**Importing** an existing install: users (passwords re-hashed with
argon2), message bases, file records and descriptions, areas,
conferences, levels, strings, menus, text files, statistics, history,
callers, BBS list, infoforms and their answers, new user votes, text
libraries (unpacked into themes).

## What's left to port

| Feature | Original | Commands | Notes |
|---|---|---|---|
| Doors | DOORS.PAS | `D0`–`D3`, `DZ` | Drop files (DOOR.SYS, DORINFO1.DEF, CHAIN.TXT), running the door with the caller's connection as its stdin/stdout (or a virtual FOSSIL for DOS doors under DOSBox/dosemu). **Next.** |
| IPL scripting | IPLC.PAS, IPLI.PAS, IPLX.PAS | `-X` | Iniquity's own scripting language: compiler and interpreter. Port it so `.IPS` mods run, or offer Rhai/Lua instead. |
| QWK offline mail | QWKMAIL.PAS | `QD`, `QU` | Packet download and REP upload. |
| Paging the SysOp / SysOp chat | CHATMODE.PAS | `CP`, `>C` | The original split-screen chat assumed a local console; here it would page a SysOp who is logged in on another node, then chat. |
| Time bank | (MENUCMD) | `!D`, `!W`, `!E`, `!I`, `!V` | Deposit and withdraw minutes and kb. |
| Tetris | tetris.pas | `TP`, `TS` | The built-in game and its high scores. |
| Message attachments | ATTACH.PAS | | Files attached to messages and email. |
| Jump to area by number | MSGAREA/FILEAREA | `M#`, `F#` | |
| Message tags | EMAIL.PAS | `E+`, `E-`, `E/`, `E\` | Tagging messages and email while reading. |
| Events | EVENTS.PAS | `*V` | Scheduled jobs (packing, maintenance). |
| Echomail / netmail | echomail.pas, INIQMAIL.PAS | | FidoNet-style networking; would need a mailer/tosser bridge today. |
| File area packing | FILEPACK.PAS | | Removing records of missing files. |

Smaller gaps:
- Bans are kept in memory and cleared by a restart.
- Wrong-password and bot-check failures aren't shown to the SysOp
  anywhere but the log.

**Not planned:** modem setup (`*M`), MicroDOS (`*D`, a shell on the host),
the local console and wait-for-call screen (ANSIWFCS, STATBAR,
WAIT4CAL), and the Oblivion/Renegade converters (OBV2INIQ, REN2INIQ). A
web or terminal SysOp dashboard could replace the local console later.

## Configuration

| File | What | Edited with |
|---|---|---|
| `iniquity.toml` | system settings, `[security]`, `[nuv]`, `[telnet]` | `*C` |
| `levels.toml` | access levels A–Z | `*L` |
| `msgareas.toml` | message conferences and areas | `*A`, `*@` |
| `fileareas.toml` | file conferences and areas | `*F`, `*!` |
| `protocols.toml` | external transfer protocols | `*X` |
| `archivers.toml` | archivers for formats other than ZIP | `*Z` |
| `infoforms.toml` | infoforms | `*I` |
| `themes.toml`, `themes/` | screen themes | `*T` |
| `strings.toml` | prompt strings that differ from stock | `*E` |
| `menus/*.toml` | menus | `*#` |
| `text/` | screens (ANSI/ASCII) | your art tools; see `docs/ARTWORK.md` |

Files that don't exist yet use built-in defaults; the editors create
them. Most changes apply at once; SSH, telnet and path settings need a
restart.

## Importing an existing Iniquity install

    iniquity import /path/to/old/INIQUITY [dest-dir] [--force]

| from | to |
|---|---|
| INIQUITY.DAT | iniquity.toml |
| DATA\LEVELS.DAT | levels.toml |
| DATA\STRINGS.DAT | strings.toml |
| DATA\USERS.DAT | data/iniquity.db |
| DATA\MSGAREA.DAT, EMAIL.DAT, MSGCONF.DAT | msgareas.toml |
| MSGS\*.HDR, *.DAT | data/iniquity.db |
| DATA\FILEAREA.DAT, FILECONF.DAT | fileareas.toml |
| DATA\*.DIR, FILEDESC.DAT | data/iniquity.db |
| DATA\STATS, HISTORY, CALLERS, BBSLIST.DAT | data/iniquity.db |
| DATA\INFOFORM.DAT, INFOFORM.nnn, NUV.DAT | infoforms.toml, data/iniquity.db |
| DATA\TEXTLIB.DAT, *.TFL | themes.toml, themes/NAME/ |
| MENU\*.MNU | menus/*.toml |
| TEXT\* | text/ |

Passwords stay case-insensitive, as in the original. File records are
imported, but the files themselves must be copied into each area's new
directory (the importer says where).

## Keeping bots out

`[security]` in `iniquity.toml`, or the Security section of `*C`:

- **Bot check**: every caller first presses a random combination of keys
  shown on screen (3 keys, 30 seconds, 3 goes by default). The keys are
  drawn out of order with cursor moves, so a script reading the byte
  stream doesn't see them in sequence. Its prompts are strings
  (`BotCheck...`), editable with `*E`.
- **Limits per address**: 2 connections at once and 6 new ones a minute.
- **Bans**: 3 failed bot checks or passwords ban the address for 15
  minutes; each later ban doubles, up to a day. Banned addresses are
  dropped before the telnet negotiation or SSH login.
- **Idle callers** get 60 seconds to log in.
- `allow` and `deny` take addresses, prefixes (`10.0.`) or ranges
  (`192.168.1.0/24`). Put your own address in `allow` so a mistyped
  password can't lock you out.

New users start at access level A until voted in or validated with `*U`.

## File transfers

Zmodem is built in, so SyncTERM and other BBS terminals can upload and
download with nothing else installed. Other protocols are external
programs with stdin/stdout connected to the caller, as the original ran
DSZ (`protocols.toml`):

    [[protocol]]
    key = "Y"
    desc = "Ymodem (lrzsz)"
    batch = true
    send = "sb %F"      # %F: the file paths
    receive = "rb"      # runs in the upload directory

ZIP is read natively. Other formats go through `archivers.toml`: by
default `bsdtar` lists and extracts RAR, 7Z and LHA/LZH, `rar`/`7z` put
descriptions back, and ARJ needs 7-Zip. They're used for description
import, the archive viewer and the file editor.

## Infoforms and new user voting

An infoform is a text file in `text/` with input points in it (see
`text/EXAMPLE.INF`): `~SN` reads text (`~SU` upper case, `~SM` mixed,
`~SC` first letter up, and the r5 `~SE ~SW ~SI` styles), `~BD` a date,
`~PN` a phone number, `~NM` digits, `~FN` a filename, and a bare `*` is
plain text too. `~NN` (two digits) sets how long answers may be, `~**`
puts it back to 254, and `~*=` / `~*-` make them required or optional.
Mandatory forms are filled in at logon (`IM` on the stock STARTUP menu);
callers see and redo theirs with `IN`.

With new user voting (`[nuv]`), users who pass the voter ACS vote on the
users who match the candidate ACS, at logon or from the main menu.
Voters can read the candidate's NUV infoforms and leave comments; enough
yes votes give the candidate the validated level, enough no votes delete
them.

## Screen themes

Iniquity's text libraries let a board offer more than one look. Here a
theme is a directory under `themes/` holding any screens that should
differ from `text/` (`MAIN.ANS`, `PRELOGON.ANS`, the list screens...);
everything else still comes from `text/`. `themes.toml` lists them with
a description and author (`*T` edits it); the first is the default.
Callers choose with the `UT` command (put it on a menu, e.g.
`keys = "/T"`, `command = "UT"` in `menus/GLOBAL.toml`), and the SysOp
can set a user's theme in `*U`. `%LD` and `%LA` show the current theme's
description and author. Imported `.TFL` libraries are unpacked into
theme directories.

## Terminal handling

Callers whose TERM is a BBS terminal (`syncterm`, `ansi-bbs`, ...) get
raw CP437; so do telnet clients that don't report a type. Everyone else
gets UTF-8, with DOS behaviour emulated: hard wrap at column 80 and
`ESC[2J` homing the cursor. Override with `[terminal] charset`.

## Layout

- `crates/iniquity-term`: CP437, DOS colours, the ANSI cursor/colour
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
    `sysconfig.rs`, `extprog.rs`.
  - `import.rs`: the legacy importer.
- `menus/`, `text/`, `*.toml`: the stock r5 setup, imported.
- `crates/iniquity/assets/strings.toml`: the stock prompt strings, built
  in.
- `docs/ARTWORK.md`: checklist of screens to replace with custom art.
- `tools/ansi2png.py`: renders an ANSI file to PNG for previews.
- `tools/drive_ssh.py`, `tools/drive_telnet.py`: scripted callers for
  testing a running board (see `CLAUDE.md`).
- `CLAUDE.md`: notes for picking the work up (conventions, testing).

## Development

    cargo test                                        # unit tests
    INIQUITY_STOCK=/path/to/stock/install cargo test  # plus the legacy readers
    cargo clippy --all-targets

## Credits

Iniquity BBS © 1995–97 Mike Fricker and the Iniquity Development Team.
This port reimplements its behaviour in Rust; the stock screens, menus
and strings are the original's.
