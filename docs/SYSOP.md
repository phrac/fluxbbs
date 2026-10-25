# Running a board

Setting up, configuring and securing a flux/bbs board.

## Setting up

### Logging in the first time

| What | Value |
|---|---|
| SysOp account | `SysOp` / `SYSOP` |
| System password | `SYSTEM`, asked of SL 250 and up |
| New callers | pick "apply as new" on the matrix menu, or type `new` at the login prompt |

Change the stock passwords before going public. Every caller first
passes a short bot check: type the three characters shown.

### What's on disk

| Path | What |
|---|---|
| `data/` | SSH host keys (Ed25519, ECDSA and RSA), made on first run |
| `data/fluxbbs.db` | the database |
| `files/` | uploaded files |
| `backups/` | [backups](#backups), if you make them |

Neither the database nor the uploads are in git. Boards from before the
rename keep `iniquity.toml` and `data/iniquity.db`, and still work.

## The command line

| Command | What it does |
|---|---|
| `fluxbbs [CONFIG]` | runs the board; CONFIG defaults to `fluxbbs.toml` (or `iniquity.toml`) in the current directory |
| `fluxbbs --wfc [CONFIG]` | the same, with the waiting-for-caller screen on the console |
| `fluxbbs import OLD [DEST] [--force]` | imports an Iniquity install ([below](#importing-an-existing-iniquity-install)) |
| `fluxbbs unban ADDRESS\|all [CONFIG]` | lifts a ban, for a SysOp locked out of their own board; takes effect when the server (re)starts, or at once if it isn't running |
| `fluxbbs add-files [CONFIG]` | adds files put into area directories by hand (a mirror, say), with their descriptions, as uploaded by the SysOp |
| `fluxbbs pack-files [CONFIG]` | forgets files that are gone from their area's directory |
| `fluxbbs textfiles fetch DIR`, `import CONFIG` | the textfiles.com archive as file areas ([FILES.md](FILES.md#the-textfilescom-archive)) |
| `fluxbbs ftn poll [NET]\|toss\|scan\|areafix ...` | FTN mail jobs by hand ([FEATURES.md](FEATURES.md#ftn-echomail)) |
| `fluxbbs backup [DAYS] [DIR] [CONFIG]` | a [backup](#backups) now, with the board running or not |

## Backups

A backup is one file, `fluxbbs-YYYY-MM-DD-HHMM.tar.gz`, in `backups/`
(or DIR). It holds everything in the board's directory a restore needs:

- the database, snapshotted (SQLite's `VACUUM INTO`), so the board
  needn't stop;
- the settings (`*.toml`), the board's own screens, menus and themes;
- door games' data and the SSH host keys (so callers' clients don't
  warn of a changed key).

Left out: the file areas' folders and the folders holding them (uploads
are too big to copy nightly; copy `files/` elsewhere yourself), the
temporary and cache folders, other backups, and `.bak` copies. Backups older than DAYS days (7 if not
given) are removed.

Make one each night with an event (`*V`):

```toml
[[event]]
desc = "Nightly backup"
time = "03:30"
run_missed = true
command = "backup 7"
```

`backup 30 /mnt/nas/bbs` keeps a month's on another disk. A backup on
the board's own disk guards against mistakes and a damaged database,
not against losing the disk: copy `backups/` somewhere else too.

To restore: stop the board, unpack the backup over the board's
directory (or into an empty one, with `files/` copied in), and start it:

    tar xzf backups/fluxbbs-2026-10-24-0330.tar.gz -C /path/to/board

## Running in a container

`deploy/docker-compose.yml` runs one board:

    docker compose -f deploy/docker-compose.yml up -d --build

The image (from `Dockerfile`) holds the server and the stock screens,
menus, themes and scripts at `/srv/inq/iniquity`. The board directory
is mounted at `/srv/board`: its `fluxbbs.toml`, `data/`, `files/` and
own screens. So a board whose config points at `../inq/iniquity/...`
works the same inside the container as beside a checkout.

| Setting | What | Default |
|---|---|---|
| `BOARD` | the board directory | `~/code/disflux_bbs`, for a checkout at `~/code/inq/iniquity` |
| `BBS_UID`, `BBS_GID` | who the server runs as; must own the board directory | 1000 |

| Host port | What |
|---|---|
| 2222 | SSH |
| 23 | telnet |
| 24554 | binkp (FTN mail), if `[ftn] listen` is set |
| 127.0.0.1:8099 | the [website feed](#website-feed), for a web server on the host to pass through |

Logs go to `docker logs fluxbbs`.

## Configuration

| File | What | Edited with |
|---|---|---|
| `fluxbbs.toml` | system settings, `[security]`, `[nuv]`, `[telnet]`, `[chat]`, `[qwk]`, `[dos]` | `*C` |
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
them. Most changes apply at once. SSH, telnet and path settings need a
restart.

### Running a board outside the repo

Paths in `fluxbbs.toml` are relative to that file. So a board can live
in its own directory, apart from the code, and start with
`fluxbbs /path/to/board/fluxbbs.toml`.

Copy `fluxbbs.toml`, `msgareas.toml` and `fileareas.toml` there. Then
point the shared parts back at the repo (here, a board directory beside
the checkout):

    [paths]
    text = "text"                          # the board's own screens
    stock_text = "../iniquity/text"        # stock screens, for the rest
    menus = "menus"                        # the board's own menus
    stock_menus = "../iniquity/menus"      # stock menus, for the rest
    scripts = "../iniquity/scripts"

- A screen in the board's `text/` replaces the stock one of the same
  name. Anything it doesn't have comes from `stock_text`.
- Menus work the same way. `*#` saves a changed stock menu into the
  board's `menus/`, so deleting it there brings the stock one back.
- `data/`, `files/` and whatever the editors create stay with the
  board, and the repo stays stock.

## Importing an existing Iniquity install

    fluxbbs import /path/to/old/INIQUITY [dest-dir] [--force]

| From | To |
|---|---|
| INIQUITY.DAT | fluxbbs.toml |
| DATA\LEVELS.DAT | levels.toml |
| DATA\STRINGS.DAT | strings.toml |
| DATA\USERS.DAT | data/fluxbbs.db |
| DATA\MSGAREA.DAT, EMAIL.DAT, MSGCONF.DAT | msgareas.toml |
| MSGS\*.HDR, *.DAT | data/fluxbbs.db |
| DATA\FILEAREA.DAT, FILECONF.DAT | fileareas.toml |
| DATA\*.DIR, FILEDESC.DAT | data/fluxbbs.db |
| DATA\STATS, HISTORY, CALLERS, BBSLIST.DAT | data/fluxbbs.db |
| DATA\INFOFORM.DAT, INFOFORM.nnn, NUV.DAT | infoforms.toml, data/fluxbbs.db |
| DATA\TEXTLIB.DAT, *.TFL | themes.toml, themes/NAME/ |
| MENU\*.MNU | menus/*.toml |
| TEXT\* | text/ |

- The destination must not hold a board already. `--force` replaces
  `fluxbbs.toml` and `data/fluxbbs.db`; a pre-rename `iniquity.toml` or
  `data/iniquity.db` there is never replaced, so import somewhere else.
- Imported passwords stay case-insensitive until changed (see
  [Passwords and sign-ups](#passwords-and-sign-ups)).
- File records are imported, but you copy the files themselves into
  each area's new directory. The importer says where.

## Keeping bots out

These settings are `[security]` in `fluxbbs.toml`, or the Security
section of `*C`.

### The bot check

Every caller first types three big characters drawn on screen. A
person reads them easily; a script reading the text, even through a
terminal emulator, sees only noise.

- The characters are drawn with background color, in a field of noise
  made of the same symbols inside and out.
- The colors change each time. The characters shift and are speckled.
- The field is drawn in shuffled pieces.
- An invisible decoy in the old "press these keys" form comes first.
- An answer in under 0.6 seconds is wrong.

| Setting | What | Default |
|---|---|---|
| `challenge` | ask every caller to pass the check | on |
| `challenge_seconds` | time allowed | 30 |
| `challenge_tries` | goes allowed | 3 |
| `challenge_visual` | "Picture check"; off goes back to pressing keys written out, drawn out of order with cursor moves | on |
| `challenge_keys` | how many keys, for the key-press check | 3 |

A `BOTCHECK` screen can frame the check: `~BC` marks the field's top
left (40x9) and `~BP` the answer. Without one there's a plain frame.
The prompts are strings (`BotCheck...`), editable with `*E`.

### Limits and bans

| Setting | What | Default |
|---|---|---|
| `max_per_ip` | connections from one address at once | 2 |
| `max_per_minute` | new connections from one address a minute | 6 |
| `fail_limit` | failed bot checks or passwords before a ban | 3 |
| `ban_minutes` | first ban; each later ban doubles, up to a day | 15 |
| `prelogin_idle_seconds` | time an idle caller gets to log in | 60 |
| `allow` | addresses never limited or challenged | none |
| `deny` | addresses always refused | none |

`allow` and `deny` take addresses, prefixes (`10.0.`) or ranges
(`192.168.1.0/24`). Banned addresses are dropped before the telnet
negotiation or SSH login. Put your own address in `allow`, so a mistyped password can't
lock you out.

New users start at access level A until voted in or validated with `*U`.

### Passwords and sign-ups

These are `[passwords]` and `[newuser]`, or "Passwords and Sign-ups" in
`*C`.

**Passwords** are case-sensitive and up to 64 characters (Iniquity
upper-cased them and stopped at 20). Ones set before this, imported
ones included, were stored upper-cased. They still work in any case
until changed.

| Setting | What | Default |
|---|---|---|
| `min_length` | shortest password allowed | 8 |
| `refuse_common` | refuse common passwords, the handle, and one character repeated | on |
| `enforce_at_login` | a caller whose password breaks the rules (the stock `SYSOP`, say) must choose a new one before going on | on |
| `per_address_per_day` | new accounts from one address in 24 hours | 2 |
| `per_day` | new accounts on the whole board in 24 hours | 10 |
| `reserved_handles` | handles nobody may apply with | sysop, admin, root, guest... |
| `purge_after_days` | mark never-validated accounts deleted after this many days (0 = off) | 0 |

- `refuse_common` checks the list of most-used passwords
  (`assets/common-passwords.txt`), which also catches `password1!` and
  the like. It also refuses passwords containing the handle (of three
  characters or more), or one character repeated.
- The rules apply when applying and when changing a password. The SysOp
  setting one in `*U` isn't held to them.
- Addresses on the `allow` list aren't held to the sign-up limits.
  Refusals go in the security log.
- A handle can't start with a digit.
- The purge takes accounts still at level A that never called back. It
  checks every 10 minutes.

### SSH keys and stored logins

An SSH caller can come straight in after PRELOGON, with no bot check or
login prompts. Their SSH user name must be an account (the handle, or
another SSH user name they set), and their client must bring one of:

- **A key** kept on the account. The caller adds it with `k` in user
  config, or `UK` on a menu: paste the line from the `.pub` file. Up to
  eight keys; the comment at the end doesn't matter.
- **The account's own password** as the SSH password. This is a
  terminal's stored login, e.g. the Username and Password in a SyncTERM
  directory entry. A wrong one counts as a failed login (security log,
  bans).

SysOps are still asked the system password.

Without either, the client falls back to the usual way in, so nobody is
locked out: OpenSSH asks nothing, and the board's own login follows.
Names that aren't accounts, as bots use, go to the login as before.

**The other SSH user name** (`U` on the key screen) lets keys and stored
logins work under, say, the caller's own login. Then plain `ssh
bbs.example.com -p 2222` works. It can't be someone else's handle or
SSH user name.

The USERCFG screen shows the key count at a `~P` field (`~PK`, key `k`).

## An AI assistant

An agent is an AI that calls the board as a caller does, over SSH
with a terminal; one can help run the board (fill requests, upload,
post news, report bugs). Its own guide is [AGENTS.md](AGENTS.md).
To set one up:

1. **Make the account.** Apply as new, or add it in `*U`. Give it a
   handle that says what it is (`hermes`, say).
2. **Give it flag `B`** (`*U`, flags). This marks it as a bot. Bots
   get the board as it comes, with none of the tailoring callers get:
   - no AI memory is kept of it
   - the AI chat isn't told about it
   - its menus show no hints
   - it can't start a request ([request boards](FEATURES.md#request-boards))
3. **Pick its level.** Give it a level that can read and post where it
   works and upload files. Keep it below co-SysOp (`s250` by default):
   - The system password is asked of co-SysOps, even with a key, and an
     agent shouldn't hold it.
   - An agent doesn't need the SysOp menus.
   - If your levels have upload/download ratios, exempt its level, or
     its downloads will run out.
4. **Make it the moderator of the boards it looks after.** In `*A`,
   set "Area sponsor" on those areas to its handle. A board's moderator
   may pin threads and edit or delete posts there. It doesn't make it
   a moderator anywhere else, and it doesn't let it mark requests
   filled: only the asker does that.
5. **Make a request board** if you haven't one. Add an area in `*A`
   (named "requests", say), set item 6 "Requests" to Yes, and make the
   agent its sponsor. Being a bot, it earns no fill points and isn't on
   the top fillers.
6. **Make a news board** for its posts (named "ai news", say). To let
   only you and the agent start threads there, set the post ACS to
   `s255|U<n>`, where `<n>` is the agent's user number (`U` matches a
   user number in ACS).
7. **Add its SSH key.** Log in as the agent once and add the public
   key with `k` in user config (or `UK` on a menu), or ask the agent to
   do it. With the key, it comes straight in with no bot check or login
   prompts ([SSH keys](#ssh-keys-and-stored-logins)).
8. **Tell it where things are:** the request board, the news board,
   which file areas it may upload to, and how often it may post.

The board's limits apply to agents too:

- two connections at once from one address
- six new connections a minute
- a ban after three failed logins

One session at a time is plenty. If the agent runs on the same host as
the board, it shares that host's address with anything else there.

## Website feed

`[web]` serves a read-only JSON status for a board's website, at
`/status.json` and `/api/status.json`. It's off by default; "Website
Feed" in `*C` turns it on.

The feed holds:

- **Who's online**: handle, a vague activity such as "browsing files",
  and ssh or telnet.
- **The last callers.**
- **The newest files**, from areas a brand-new caller could open, with
  the first description line that has words in it.
- **The newest posts** (`new_posts`), from message areas a brand-new
  caller could read, never private ones: area, subject, author,
  replies, and whether it's a reply.
- **Totals and today's numbers.**

It answers GET only and is rebuilt at most every 10 seconds. The same
listener serves the web file lists `/files.txt` and `/files.html`
([FEATURES.md](FEATURES.md#from-20-and-220)).

| Setting | What | Default |
|---|---|---|
| `enabled` | serve the feed | off |
| `listen` | where it listens; keep it private | `127.0.0.1:8099` |
| `allow_origin` | a site on another address allowed to fetch it | empty |
| `show_handles` | show handles (otherwise "a caller") | on |
| `show_sysop` | list the SysOp among who's online | on |
| `last_callers`, `new_files`, `new_posts` | how many of each | 10, 10, 5 |

Have the web server pass the path through, e.g. Caddy's
`reverse_proxy`. A page shows the feed with a few lines of script
fetching that URL. Insert the values as text, not HTML, since handles
and descriptions are typed by callers.

## Screen themes

A theme lets a board offer more than one look. It is a directory under
`themes/` holding any screens that should differ from `text/`
(`MAIN.ANS`, `PRELOGON.ANS`, the list screens...). Everything else
still comes from `text/`.

- `themes.toml` lists the themes with a description and author; `*T`
  edits it. The first is the default.
- Callers choose with the `UT` command. Put it on a menu, e.g.
  `keys = "/T"`, `command = "UT"` in `menus/GLOBAL.toml`.
- The SysOp can set a user's theme in `*U`.
- `%LD` and `%LA` show the current theme's description and author.
- Imported Iniquity text libraries (`.TFL`) are unpacked into theme
  directories.
- Screens sharing a header can start with `|SA`, which draws
  `STOCK.ANS` in place, so the header lives in one file.

### The stock libraries

The Iniquity release shipped two libraries it never installed (no
TEXTLIB.DAT). They're here, unpacked:

| Directory | Name | What |
|---|---|---|
| `themes/t10inq1/` | "Menuset #1" | ANSI by tosh10!acid, 85 screens, including the only stock SYSOP and DOOR screens |
| `themes/ex-iq2/` | "True Blue" | 11 screens |

Like the stock board, the repo doesn't list them in `themes.toml`, so
callers see `text/`. To use one:

- **As a theme**: add it with `*T` (insert, then set its directory to
  `t10inq1`).
- **As a fallback**: set `[paths] stock_theme` to its directory. Then
  any screen that neither the board's `text/` nor the stock screens
  have (SYSOP, DOOR) comes from the library.

## Terminal handling

| Caller | Gets |
|---|---|
| TERM is a BBS terminal (`syncterm`, `ansi-bbs`, ...) | raw CP437 |
| telnet client that doesn't report a type | raw CP437 |
| everyone else | UTF-8, with DOS behavior emulated: hard wrap at column 80, and `ESC[2J` homing the cursor |

The BBS terminals are listed in `[terminal] cp437_terms`. Override the
choice with `[terminal] charset`.

Color goes out three ways. `truecolor_terms` (SyncTERM) get 24-bit
color as it is. `palette256_terms` take xterm's 256 colors but not
24-bit: by default `ansi-256color`, which NetRunner reports. Every
24-bit color is sent as the nearest of the 256, bright backgrounds as
its bright colors, and each color code names both foreground and
background (NetRunner keeps an old 256-color background through a
reset otherwise). Both get the animated screens and the pictures.
Any other CP437 terminal is asked what it can show when it connects,
from its answers rather than its TERM (IcyTerm sends a plain `ANSI`).
It is asked for its device attributes (`ESC[c`); one that answers is
set to a 24-bit color and asked what it has (DECRQSS): the color coming
back means 24-bit, a `38;5` the 256. One answering as CTerm does
(`ESC[=...c`: SyncTERM, IcyTerm) that didn't say is asked for its
features (`ESC[<c`), as Synchronet does, and an extended palette there
means 24-bit. The log shows the answers ("the terminal answers ...").
A terminal that doesn't answer, or says nothing of color (Qodem,
`ansi-bbs`), gets 16 colors: 256 and 24-bit color as the nearest DOS
color, the iCE blink bit for bright backgrounds, and no animation.
