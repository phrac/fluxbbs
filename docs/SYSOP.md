# Running a board

Configuration, security and the SysOp's side of things.

## Setting up

Every caller first passes a short bot check (press the keys shown). The
stock SysOp account is `SysOp` / `SYSOP`, with system password `SYSTEM`
(asked of SL 250+). New callers pick "apply as new" on the matrix menu
or type `new` at the login prompt.

SSH host keys (Ed25519, ECDSA and RSA) are made in `data/` on first run.
The database is `data/iniquity.db`; uploaded files live under `files/`.
Neither is in git.

## Running in a container

`Dockerfile` builds an image with the server and the stock screens,
menus, themes and scripts at `/srv/inq/iniquity`; a board directory
(its `iniquity.toml`, `data/`, `files/`, own screens) is mounted at
`/srv/board`, so a board whose config points at `../inq/iniquity/...`
works the same inside. `deploy/docker-compose.yml` runs one board,
SSH on the host's 2222 and telnet on 23:

    docker compose -f deploy/docker-compose.yml up -d --build

`BOARD` is the board directory (default `~/code/disflux_bbs` for a
checkout at `~/code/inq/iniquity`), `BBS_UID`/`BBS_GID` who the server
runs as (default 1000, which must own the board directory). Logs go to
`docker logs iniquity`.

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
| `text/` | screens (ANSI/ASCII) | your art tools; see [ARTWORK.md](ARTWORK.md) |
| `scripts/` | Lua scripts (`-X`, `%!`) | any editor; see [SCRIPTING.md](SCRIPTING.md) |
| `events.toml` | scheduled events | `*V` |
| `logon.toml` | the logon sequence | `*O` |

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
    menus = "menus"                        # the board's own menus
    stock_menus = "../iniquity/menus"      # stock menus, for the rest
    scripts = "../iniquity/scripts"

A screen in the board's `text/` replaces the stock one of the same name;
anything it doesn't have comes from `stock_text`. Menus work the same way
(new): one in the board's `menus/` replaces the stock one, and `*#` saves
a changed stock menu there, so deleting it brings the stock one back.
`data/`, `files/` and whatever the editors create then stay with the
board, and the repo stays stock.

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

## Website feed

New: `[web]` (off by default; "Website Feed" in `*C`) serves a read-only
JSON status at `/status.json` and `/api/status.json` for a board's
website: who's online (handle, a vague activity such as "browsing
files", ssh/telnet), the last callers, the newest files (from areas a
brand-new caller could open; the first description line with words in
it), the newest posts (`new_posts`, from message areas a brand-new
caller could read, never private ones: area, subject, author, replies
and whether it's a reply), totals and today's numbers. It answers GET only, is rebuilt at most
every 10 seconds, and can hide handles (`show_handles`) or the SysOp
(`show_sysop`). Keep `listen` private and have the web server pass the
path through (e.g. Caddy's `reverse_proxy`); `allow_origin` is for a
site on another address. A page shows it with a few lines of script
fetching that URL; insert the values as text, not HTML, since handles
and descriptions are typed by callers.

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

The release shipped two libraries it never installed (no TEXTLIB.DAT),
and they're here, unpacked, as `themes/t10inq1/` ("Menuset #1", ANSI by
tosh10!acid, 85 screens including the only stock SYSOP and DOOR
screens) and `themes/ex-iq2/` ("True Blue", 11). Like the stock board,
the repo doesn't list them in `themes.toml`, so callers see `text/`; add
one with `*T` (insert, then set its directory to `t10inq1`). Or set
`[paths] stock_theme` to its directory: then any screen that neither the
board's `text/` nor the stock screens have (SYSOP, DOOR) comes from the
library. Screens sharing a header can start
with `|SA`, which draws `STOCK.ANS` in place, so the header lives in one
file.

## Terminal handling

Callers whose TERM is a BBS terminal (`syncterm`, `ansi-bbs`, ...) get
raw CP437; so do telnet clients that don't report a type. Everyone else
gets UTF-8, with DOS behaviour emulated: hard wrap at column 80 and
`ESC[2J` homing the cursor. Override with `[terminal] charset`.
