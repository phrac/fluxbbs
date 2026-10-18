# Iniquity BBS, in Rust

A port of **Iniquity** (Mike Fricker and the Iniquity Development Team,
1995–97, Turbo Pascal, version 1.00a26r5) to Rust, for running a BBS on
today's internet. Modems and FOSSIL drivers are replaced by SSH and
telnet, the `.DAT` files by SQLite and TOML, and everything runs in one
async process. The stock screens, menus, prompt strings and behaviour
are kept, so an existing Iniquity board can be imported and look the
same.

## Quick start

    tools/setup_host.sh                      # first time: stock data, build
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
- Paging the SysOp (`CP`, `>C` from the matrix): the page goes to a node
  where a SysOp is logged in, who is asked, wherever they are, whether
  to chat; the two then chat on the split screen (CHATMODE.ANS), each
  keystroke sent as it's typed. `/q` ends it, `/bye` from the SysOp
  hangs the caller up, `/cls` clears the windows, `/?` shows CHATHELP. Chat hours, the
  override ACS and password, pages per call and how long a page lasts
  are `[chat]` in `iniquity.toml`; outside the hours, or with no SysOp
  online, the caller is offered email instead.

**Keeping bots out**
- Key-combination bot check before anything else, on SSH and telnet.
- Per-address connection limits, bans for repeated failures (doubling,
  up to a day), allow and deny lists, a short idle limit before login.
- Bans and failures are kept in the database: bans survive a restart,
  SysOps are told at logon how many failures and bans there have been
  since their last call, `*G` lists them and lifts bans, and
  `iniquity unban ADDRESS|all` does so from the shell for a SysOp
  locked out of their own board.

**The board**
- Display engine: `|XX` colours and controls, `%XX` MCI codes, `@`/`&`
  positioning, `{`/`}` limits and padding, `~XX` fields, repeat screens
  (TOP/MID/HIL/BOT), random and day-of-week screen variants.
- Menu engine: hot keys, `/` and `//` commands, `#` (an area number
  typed at the prompt, for `M#`/`F#`), FIRSTCMD, stacked
  commands, gosub/return, generic and lightbar menus, ACS expressions,
  passwords, fallbacks, command help.
- Logon: the matrix menu, login, system password, new-user application
  (each personal question can be switched off; handles kept as typed;
  the NEWCFG screen to check the entries afterwards), access levels and
  daily limits. Every N calls the caller is asked their birth date or
  the last four digits of their phone number again (`*C`, Access and
  Logon; the manual's "calls before check" options, whose code isn't in
  the surviving source).
- Messages: areas and conferences, the reader, posting and replying,
  quoting, the full-screen editor, newscans and mandatory messages,
  email, auto-signatures. A file can be attached to a message (ACS per
  public/email in `*C`; nothing in this release's source called
  ATTACH.PAS, so this follows the manual): uploaded after posting, or
  named on disk by a SysOp, kept under `data/attach` by the message's
  row id, offered for download when the message is read, and removed
  when packing drops the message. A prepared message can be uploaded
  instead of typed (ACS and "ask" toggles in `*C`); it lands in the
  editor to look over. Email tags: `E/ n` tags the email written
  this call, `E\ n` with `E+` shows only email tagged `n` (`E-` shows
  it all), e.g. for a separate support mailbox; this release kept the
  tag on messages but never used it, so this follows the manual.
- Files: areas and conferences, the lightbar file lister, downloads
  (single and batch, with points, ratios and daily limits), uploads
  (blind or named, with FILE_ID.DIZ import), search and newscan, archive
  viewing, local upload. Zmodem built in; other protocols and archivers
  (RAR, 7Z, LHA via bsdtar/7-Zip) are external programs. Before a
  download the caller can ask to be logged off when it's done (a
  countdown afterwards; `*C`, Transfers).
  Uploads are tested on the FILETEST screen (`*C`, Upload Tests): ZIPs
  are read through (a CRC failure refuses the file) and can be held to
  an age limit; an outside command can be run on every upload in place
  of the DOS virus scanner (`clamscan --no-summary %F`, say); files on
  the delete list (BBS ads) are stripped from ZIPs and the archive
  comment added.
- Oneliners, the automessage, last callers, history and statistics, the
  BBS list, user configuration, user listing.
- Infoforms (questionnaires, mandatory or not) and new user voting.
- The time bank (`!D`/`!W` minutes, `!E`/`!I` download kilobytes, `!V`
  the account), with per-level limits on what's kept and moved each
  day (`*L`). The stock TIMEBANK menu's withdraw entry had no command;
  it's `!W` now.
- Screen themes (the original's text libraries): directories of screens
  that replace the stock ones; callers pick theirs with `UT`.
- Scripting in Lua, in place of IPL: `-X name` and `%!name` run
  `scripts/name.lua`, with a `bbs` API for output, input, the user
  record, menu commands and stored values. See
  [docs/SCRIPTING.md](docs/SCRIPTING.md).
- QWK offline mail: `QD` packs new messages (email as conference 0)
  for an offline reader, `QU` posts the replies (`[qwk]` in
  `iniquity.toml`). See [QWK](#qwk-offline-mail).
- Doors, native and DOS (under DOSBox): `D1` DORINFO1.DEF, `D2` DOOR.SYS,
  `D3` CHAIN.TXT, `D4` DOOR32.SYS, `DZ` DOOR.SYS and DORINFO1.DEF, `D0`
  none. See [Doors](#doors).

**SysOp tools** (the `*` commands on the SysOp menu)
- `*C` system configuration, written back to `iniquity.toml` keeping its
  comments; `*E` prompt strings; `*#` menus; `*L` access levels.
- `*U` users (with colours, preferences, auto-signature, infoforms).
- `*F` file areas, `*Y` files (validate, move, extract/apply DIZ), `*S`
  sort areas, `*>` download any file, `*%` local upload, `*K` pack file
  areas (forget files missing from their directories; also
  `iniquity pack-files` from the shell, as FILEPACK.EXE was).
- `*A` message areas, `*P` pack them, `*@`/`*!` conferences, `*R` all
  email.
- `*X` protocols, `*Z` archivers, `*B` BBS list, `*H` statistics, `*I`
  infoforms, `*T` themes.
- `*G` security: current bans and recent failed logins and bot checks;
  lift a ban.
- `*V` events: jobs run once a day at a set time, by one scheduler in
  the server (no per-node events or modem off-hook any more). A command
  goes to the shell, or is one of the built-in jobs `pack-messages` and
  `pack-files`; `force` disconnects callers first (with the stock
  "event initiating NOW" string), `run_missed` catches up after
  downtime the same day. "Run it now" in the editor starts one by hand.

**Importing** an existing install: users (passwords re-hashed with
argon2), message bases, file records and descriptions, areas,
conferences, levels, strings, menus, text files, statistics, history,
callers, BBS list, infoforms and their answers, new user votes, text
libraries (unpacked into themes).

## What's left to port

| Feature | Original | Commands | Notes |
|---|---|---|---|
| Echomail / netmail | echomail.pas, INIQMAIL.PAS | | FidoNet-style networking; would need a mailer/tosser bridge today. |

**Not planned:** Tetris (`TP`, `TS`; the game's source isn't in this
release, `tetris.pas` being a misnamed terminal unit), modem setup
(`*M`), MicroDOS (`*D`, a shell on the host),
the local console and wait-for-call screen (ANSIWFCS, STATBAR,
WAIT4CAL), and the Oblivion/Renegade converters (OBV2INIQ, REN2INIQ). A
web or terminal SysOp dashboard could replace the local console later.

## Configuration

| File | What | Edited with |
|---|---|---|
| `iniquity.toml` | system settings, `[security]`, `[nuv]`, `[telnet]`, `[chat]`, `[qwk]`, `[dos]` | `*C` |
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
| `scripts/` | Lua scripts (`-X`, `%!`) | any editor; see `docs/SCRIPTING.md` |
| `events.toml` | scheduled events | `*V` |

Files that don't exist yet use built-in defaults; the editors create
them. Most changes apply at once; SSH, telnet and path settings need a
restart.

### Running a board outside the repo

Paths in `iniquity.toml` are relative to that file, so a board can live
in its own directory, apart from the code, and be started with
`iniquity /path/to/board/iniquity.toml`. Copy `iniquity.toml`,
`msgareas.toml` and `fileareas.toml` there, and point the shared parts back
at the repo:

    [paths]
    text = "text"                          # the board's own screens
    stock_text = "../iniquity/text"        # stock screens, for the rest
    menus = "../iniquity/menus"
    scripts = "../iniquity/scripts"

A screen in the board's `text/` replaces the stock one of the same name;
anything it doesn't have comes from `stock_text`. `data/`, `files/` and
whatever the editors create then stay with the board, and the repo stays
stock. (Copy `menus/` too once you want to change menus with `*#`.)

## Doors

A door is a menu command `D0`–`D4` or `DZ` whose parameter is a shell
command. The letter picks the drop file, written to `data/doors/nodeN/`
(`[paths] doors`), which is also where the door runs:

    [[command]]
    keys = "1"
    command = "D2"
    param = "/opt/doors/mygame/start.sh %PN %ND"

The door gets a pseudo-terminal (80 columns, the caller's rows, their
`TERM`) wired to the caller, so curses programs work. Its output is
taken as CP437 and translated for UTF-8 callers. If the caller hangs up,
or their time runs out, the door's process group gets SIGHUP, then
SIGKILL.

The parameter can use `%PN` (the node's drop directory), `%PD`, `%PT`,
`%PM` (data, text and menu directories), `%BR`/`%BP` (baud, always
38400), `%CP` (COM port, 1) and any MCI code (`%UN`, `%ND`, `%TL`...).
Every value goes in shell-quoted, so don't put quotes around codes.

**DOS doors.** A command whose program ends in `.BAT`, `.COM` or `.EXE`
(the stock menus' `LORD.BAT`, say) runs under DOSBox (`[dos]` in
`iniquity.toml`; dosbox-staging by default). Its COM1 is a null modem
back to the BBS, so doors that drive the UART themselves just work; for
FOSSIL doors put a FOSSIL driver (BNU, X00) on the DOS drive and in
`setup`. The DOS drive, `doors/dos`, is C: and on the PATH, and holds the
doors and their batch files, as `pathDoor` did; the node's drop
directory is D:, the current drive, so `LORD.BAT` finds DOOR.SYS where
the original left it. In DOS commands `%PN` is `D:\`, and values lose
the characters DOS would act on (`<>|"%`). DOSBox's own output goes to
`dosbox.log` in the drop directory.

DOSBox doesn't drop carrier when its socket closes, so when a caller
hangs up or runs out of time in a DOS door, DOSBox is stopped (TERM,
then KILL after 3 seconds) rather than the door noticing: a turn in
progress may not be saved.

Changes from the original: each node gets its own drop directory; the
DOOR.SYS password line is empty; CHAIN.TXT has real times and transfer
totals, and points its directories at the drop directory; DOOR32.SYS
(`D4`) is new.

## QWK offline mail

`QD` builds NAME.QWK (`[qwk] packet`) from the new messages in the
caller's areas (the current conference's, or every conference's) and
their new email, and sends it; once it's down, the newscan pointers can
move past what was packed. `QU` takes NAME.REP back (a SysOp can give a
local file instead) and posts the replies where the caller may post;
the same packet won't post twice.

Changes from the original: email goes in as conference 0, with
PERSONAL.NDX; "include your own messages?" does what it says (the
original's test was the wrong way round); four-digit years; areas with
a password are left out; pointers move to the newest message packed
rather than to the time of the download; long reply lines are wrapped
rather than cut at 79 columns; and replies to unknown users are refused
in email (posted to the name in public areas, as before).

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

### Passwords and sign-ups

New, in `[passwords]` and `[newuser]`, or "Passwords and Sign-ups" in
`*C`:

- **Passwords** are case-sensitive and up to 64 characters (Iniquity
  upper-cased them and stopped at 20). Ones set before this, imported
  ones included, were stored upper-cased and still work in any case
  until changed.
- **Rules**: at least `min_length` characters (8), and with
  `refuse_common` not on the list of most-used passwords
  (`assets/common-passwords.txt`, also catching `password1!` and the
  like), not containing the handle, and not one character repeated.
  They apply when applying and changing a password; the SysOp setting
  one in `*U` isn't held to them.
- **`enforce_at_login`**: a caller whose password breaks the rules
  (the stock `SYSOP`, say) has to choose a new one before going on.
- **Sign-up limits**: `per_address_per_day` (2) new accounts from one
  address and `per_day` (10) on the whole board, over the last 24 hours;
  addresses on the `allow` list aren't limited. Refusals go in the
  security log.
- **`reserved_handles`**: handles nobody may apply with (sysop, admin,
  root, guest...). As in the original, a handle can't start with a
  digit either.
- **`purge_after_days`** (0 = off): accounts still at level A that never
  called back are marked deleted after that many days, checked every 10
  minutes.

## Art gallery

New, after Mystic's: `AG` (`R` on the main menu) browses `[gallery] path`
(default `gallery/` beside the config) like a file manager, or, with
`[gallery] area` set to a file area's storage key, that area: its
sub-areas become folders and its validated files are listed from the
file base (packs show their description), so uploads to the art areas
appear in the gallery by themselves. Sub-folders (any depth) and ZIP
packs open as folders (packs are read in place, not unpacked),
and only art is listed (`.ans`, `.ice`, `.asc`, `.txt`, `.nfo`, `.diz`,
`.cia`), with each piece's SAUCE title, artist and group. Enter views a
piece in the scrolling viewer; P plays it as it was sent in its day, at
the speed S cycles through (instant to 115200; `[gallery] speed` is where
it starts), which is how animated ANSIs animate; any key stops it. D
downloads the piece (pulled out of its pack) or a whole pack, for
`download_acs`; a file of a file area downloads the usual way instead,
with that area's access, points, limits and ratios. Like the viewer, playback passes only colour and cursor
codes through. Settings are under "Art Gallery" in `*C`.

It looks like the file lister: it shows `GALLERY.ANS` if there is one,
else the lister's own `LISTFILE.ANS`, with the same markers (`~TL`/`~BL`
the list rows, `~FN`/`~FS`/`~FD` the name, size and description columns,
`~PI` the info line, `~PM` the lightbar menu of open, play, speed, get
and back, `~C1`-`~C6` its colours, the `~S`/`~E` scroll bar, and `~FA`
for where you are). Without either it draws a plain screen.

## Website feed

New: `[web]` (off by default; "Website Feed" in `*C`) serves a read-only
JSON status at `/status.json` and `/api/status.json` for a board's
website: who's online (handle, a vague activity such as "browsing
files", ssh/telnet), the last callers, the newest files (from areas a
brand-new caller could open; the first description line with words in
it), totals and today's numbers. It answers GET only, is rebuilt at most
every 10 seconds, and can hide handles (`show_handles`) or the SysOp
(`show_sysop`). Keep `listen` private and have the web server pass the
path through (e.g. Caddy's `reverse_proxy`); `allow_origin` is for a
site on another address. A page shows it with a few lines of script
fetching that URL; insert the values as text, not HTML, since handles
and descriptions are typed by callers.

## Sub-areas

New, not in the original: a file area can sit under another one, to any
depth, e.g. a "Zines" area with an area per zine title. In `*F`, set
"Sub-area of" (`K`) to the parent's storage key, or in `fileareas.toml`:

    [[area]]
    name = "Phrack"
    file = "PHRACK"
    path = "files/zines/phrack"
    parent = "ZINES"

The change-area list (`FC`, `A` on the file menu) shows one level at a
time: an area with sub-areas is marked `/` (`FaGroupSuffix`) and counts
the files under it, and choosing it changes to it and opens it, where
Enter stays put. `#` to such an area opens it too. `FA` lists every area
with sub-areas indented, and `[`/`]`, numbering and `*F` follow the
tree. A sub-area needs the access of every area above it, so a parent's
ACS hides a whole branch. A parent is still an area of its own, with a
directory and room for files. Everything else (newscan, search,
uploads, conferences) treats sub-areas like any other area.

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

### Screening uploads

New, in `[files]` or "Upload Tests" in `*C`:

- **`allowed_types`** (empty = any), e.g. `["zip", "tar.gz", "tgz",
  "txt"]`. A named upload of another type is refused before the
  transfer, a blind one when it arrives. The contents must match the
  name: a `.zip` has to be a ZIP, a `.tar.gz` gzip, a `.txt` text (no
  binary), so a renamed program doesn't get in.
- **Summary line** (`scan_summary = "last"`, `"first"` or `"off"`):
  what's inside is added to the description, e.g. `[ 14 files, 1.2 MB
  unpacked, newest 10/12/26 ]`, `[ text, 342 lines ]`, plus
  `[ password-protected ]` where it applies and, with `scan_names`, the
  first few names inside. ZIP and tar/gzip are read in the server;
  other archives are counted with `bsdtar`. The lines are `FaScan...`
  strings for `*E`. The scan never changes the upload (no FILE_ID.DIZ
  is added; `+` in `*Y` puts a description into a ZIP when you want
  one).
- **Zip bombs**: archives unpacking to more than `max_unpacked_ratio`
  (100) times their size, past a megabyte, or more than
  `max_unpacked_bytes` (4 GB) are refused. The size is read as far as
  the limit and no further.
- **`refuse_unsafe_paths`** (on): archives with names that point outside
  their folder (`../`, `/etc/...`, `C:`) are refused.
- **Virus scanning**: set `test_command = "clamscan --no-summary %F"`
  (ClamAV) and infected uploads fail the test.
- **BBS ad** (`bbs_ad`, off by default; "BBS ad file" in `*C`): a file in
  the data directory, e.g. `DISFLUX.NFO`, added to every uploaded ZIP
  under its own name, replacing an older copy. Like `archive_comment`
  and `delete_list`, this does change the upload (and its checksum).

Archives are read where they are, not unpacked (other than pulling a
FILE_ID.DIZ out of a non-ZIP archive for its description).

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
- `original/`: the original Turbo Pascal source and the 1.00a26r5 release,
  for reference (© the Iniquity authors); `tools/setup_host.sh` unpacks the
  stock install from it into `original/stock/`.

## Development

    cargo test       # unit tests, plus the legacy readers once
                     # tools/setup_host.sh has unpacked original/stock
    cargo clippy --all-targets

## Credits

Iniquity BBS © 1995–97 Mike Fricker and the Iniquity Development Team.
This port reimplements its behaviour in Rust; the stock screens, menus
and strings are the original's.
