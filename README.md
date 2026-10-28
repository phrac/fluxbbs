# flux/bbs

flux/bbs is a bulletin board system for today's internet. Callers reach
it over SSH or telnet, with SyncTERM or any terminal. It is written in
Rust, began as a port of the Iniquity BBS (1.00a26r5), and has grown a
lot since: a DOS machine for doors, FTN mail, a message center after
PTT, AI chat and an art gallery.

![The art gallery](docs/screenshots/gallery.png)

The whole board runs in one process. SQLite and TOML files hold its
data and settings. Iniquity's stock screens, menus and prompts are
kept, so an old Iniquity board imports and looks the same. The program,
crates, files and container are named `fluxbbs`, since a slash can't go
there.

## Quick start

You need `cargo` and `unzip`. Showing PDFs also needs Poppler
(`poppler-utils`).

    tools/setup_host.sh                      # first time: stock data, build
    cargo run --release --features full      # serves fluxbbs.toml
    ssh -p 2222 anyname@localhost            # UTF-8 terminal
    syncterm ssh://anyname@localhost:2222    # CP437 BBS terminal
    syncterm telnet://localhost:2323         # plain telnet

- Log in as `SysOp` / `SYSOP`, with system password `SYSTEM`. Change both.
- New callers pick "apply as new", or type `new` at the login prompt.
- `--wfc` adds the waiting-for-caller screen on the console.
- The database is `data/fluxbbs.db` and uploads live in `files/`.
  Neither is in git.
- Boards from before the rename keep `iniquity.toml` and
  `data/iniquity.db`, and still work.
- Doors, the games, animated menus, screen savers, AI chat, echomail,
  MeshCore, PDFs and Lua are optional components: a plain `cargo
  build` is the core board, `--features full` everything.
  [BUILDING.md](docs/BUILDING.md) lists them and how to pick.

## In a container

    docker compose -f deploy/docker-compose.yml up -d --build

The image holds the server, built with every optional component
(`FEATURES`; [BUILDING.md](docs/BUILDING.md)), and the stock files. Your board directory
(`BOARD`, default `~/code/disflux_bbs`) is mounted beside them. SSH is
on port 2222 and telnet on 23. Logs are in `docker logs fluxbbs`.
[SYSOP.md](docs/SYSOP.md#running-in-a-container) has the details.

## What it does

![The AI giving the keys to download a file](docs/screenshots/ai-howto.png)

### Connecting and security

- SSH and telnet; CP437 and UTF-8 terminals; 24-bit color
- Nodes, node messages, multinode chat, paging the SysOp
- A picture bot check: characters drawn in color, not text
- Per-address limits, escalating bans, password rules and sign-up
  limits ([details](docs/SYSOP.md#keeping-bots-out))

### Messages

- Areas, conferences, the full-screen editor, newscans, email and attachments
- A [message center](docs/FEATURES.md#message-center) after PTT
- [QWK](docs/FEATURES.md#qwk-offline-mail) offline mail
- [FTN echomail](docs/FEATURES.md#ftn-echomail) over binkp
- A [MeshCore](docs/FEATURES.md#meshcore) radio bridge: mesh channels as message areas

### Files

- Areas and [sub-areas](docs/FILES.md#sub-areas), the lister, batch downloads, Zmodem
- [Upload screening](docs/FILES.md#screening-uploads)
- The [art gallery](docs/FILES.md#art-gallery), with ANSI playback and animated GIFs
- The [textfiles.com archive](docs/FILES.md#the-textfilescom-archive) as file
  areas, with the site's tree and descriptions (`fluxbbs textfiles`)

### The rest of the board

- Display and menu engines: MCI codes, lightbars, ACS, stacked commands
- Logon, the matrix, new-user applications, access levels
- Oneliners, automessage, last callers, BBS list, statistics
- [Infoforms and new user voting](docs/FEATURES.md#infoforms-and-new-user-voting)
- The time bank and screen themes
- [Lua scripting](docs/SCRIPTING.md)
- [Doors](docs/DOORS.md), native and DOS (a built-in DOS machine)
- [AI chat](docs/FEATURES.md#ai-chat) that can search the web and knows its way around the board,
  and a [tarot and fortune door](docs/FEATURES.md#tarot-and-fortunes) it reads for
- A [casino](docs/FEATURES.md#casino): slots, video poker, blackjack, roulette and craps, a bar and a players club, paying minutes and file points
- [Points](docs/FEATURES.md#points) around the board: up votes and doors pay, callers tip each other, a lottery each night (`!P`)
- [Carrier Lost](docs/FEATURES.md#carrier-lost): a reflex war-dialing run on a pixel CRT; tune for a carrier, catch the handshake, push the trace meter, cash out to a shared leaderboard
- [Request boards](docs/FEATURES.md#request-boards) with points for fills, and
  [file links](docs/FEATURES.md#file-links) in messages
- A [JSON feed](docs/SYSOP.md#website-feed) for the board's website
- From Iniquity 2.0 and 2.20: the voting booth, newscan selections,
  message search, invisible logons, chat commands, door names in the
  status, web file lists ([details](docs/FEATURES.md#from-20-and-220))

### For the SysOp

- An editor for everything (`*C`, `*U`, `*F`, `*A`, `*#`...)
- The waiting-for-caller screen, the security log, scheduled events
- [Importing](docs/SYSOP.md#importing-an-existing-iniquity-install)
  users, messages, files, areas, menus, strings, screens and statistics
  from an existing Iniquity install

Everything in detail: [docs/FEATURES.md](docs/FEATURES.md).

## Documentation

| Doc | What's in it |
|---|---|
| [FEATURES.md](docs/FEATURES.md) | Every feature in detail: QWK, infoforms, AI chat, MeshCore, FTN echomail |
| [NEW.md](docs/NEW.md) | What's new over Iniquity, and what changed on purpose |
| [SYSOP.md](docs/SYSOP.md) | Configuration, importing, security, the website feed, themes |
| [FILES.md](docs/FILES.md) | Sub-areas, transfers, upload screening, the art gallery |
| [DOORS.md](docs/DOORS.md) | Native doors and the built-in DOS machine |
| [SCRIPTING.md](docs/SCRIPTING.md) | Lua scripts |
| [CODES.md](docs/CODES.md) | Display codes |
| [ARTWORK.md](docs/ARTWORK.md) | Screens to replace with your own art |
| [AGENTS.md](docs/AGENTS.md) | the guide for an AI agent calling in as the SysOp's assistant (setting one up is in SYSOP.md) |
| [BUILDING.md](docs/BUILDING.md) | The core board and its optional components, and how to build either |
| [DEVELOPMENT.md](docs/DEVELOPMENT.md) | Code layout and tests |

## Status

**Not done yet: netmail from callers.** Echomail, and netmail to the
board, work over binkp ([FTN](docs/FEATURES.md#ftn-echomail)). Callers
can't yet write netmail to an FTN address.

**Not planned:** Tetris (its source isn't in this release), modem
setup, MicroDOS, the local status bar, and the Oblivion/Renegade
converters.

## Credits

Iniquity BBS © 1995–97 Mike Fricker and the Iniquity Development Team.
flux/bbs began as a port of it and reimplements its behavior in Rust.
The stock screens, menus and strings are Iniquity's.
