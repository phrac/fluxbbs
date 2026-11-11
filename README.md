# flux/bbs

A bulletin board system for people who still own a copy of SyncTERM.

You call it over SSH or telnet and get 80 columns of ANSI art, lightbar
menus, hot keys, door games and a SysOp you can page. Underneath, it's a
modern program: one Rust binary, SQLite, threads you can vote on,
echomail over binkp, a DOS machine built in for the old doors, and
pictures drawn in real pixels if your terminal can do sixel graphics.

It started as a port of the Iniquity BBS (1.00a26r5, 1995-97) and grew
from there. Iniquity's own screens still ship with it as the "classic"
set. The default look is disflux's, with your board's name put into the
logos for you.

![The main menu](docs/screenshots/main.png)

## Call disflux

[disflux](https://disfluxbbs.com) is the board flux/bbs is built on, and
where everything shows up first. It's also an AgoraNet (46:1/115) and
SysopNet (23:100/14) node.

    ssh -p 2222 yourhandle@disfluxbbs.com
    telnet disfluxbbs.com

Any handle works for a first call. Every screenshot here comes from a
copy of disflux, drawn the way SyncTERM draws it.

## Logging in

The prelogon, the "matrix" and the login prompt are each a piece of art,
with the board's text set into the black parts of it. A board brings its
own art and `art/make.py` builds the screens around it.

| | | |
|---|---|---|
| ![Prelogon](docs/screenshots/prelogon.png) | ![The matrix](docs/screenshots/matrix.png) | ![Login](docs/screenshots/login.png) |

Telnet callers are told, before they type a password, that telnet is
plain text and how to come back over SSH instead. New callers apply from
the matrix: the screen says what the board asks for, how they get in
(an emailed code, or a yes vote from the members), the rules, and how
passwords are kept (argon2; nobody can read them, the SysOp included).

| | |
|---|---|
| ![The telnet warning](docs/screenshots/telnet.png) | ![Applying](docs/screenshots/apply.png) |

## Sixel graphics

New in 1.9. Terminals that speak sixel (SyncTERM, IcyTerm, foot,
WezTerm, Windows Terminal, xterm with sixels on) can show pictures at
their real resolution, 640×384 on SyncTERM, instead of colored
half-blocks. Callers switch it on
in user config, and the board offers it once at logon with a sample so
they can see the difference first.

![The same piece in ANSI and in sixels](docs/screenshots/sixels.png)

*Left: catwoman.ans, as ANSI. Right: the same piece as a sixel picture.*

![The sixel offer at logon](docs/screenshots/sixel-offer.png)

PDFs get the most out of it. With sixels on, a PDF opens on its pages,
and Z zooms a page to the full screen width so a 1994 Boardwatch is
readable in a terminal.

| | |
|---|---|
| ![A Boardwatch page](docs/screenshots/pdf-page.png) | ![The same page, zoomed](docs/screenshots/pdf-zoom.png) |

## Messages

The message center borrows from PTT, the Taiwanese BBS: boards sorted by
what's active, boards that callers create and moderate themselves, and
replies shown as a tree. Posts take votes and one-line comments, and the
lists update while you're looking at them. Echomail from AgoraNet and
SysopNet lives in its own folders.

| | |
|---|---|
| ![Message boards](docs/screenshots/boards.png) | ![A thread as a tree](docs/screenshots/threads.png) |
| ![A post with comments](docs/screenshots/post.png) | ![The editor in vim mode](docs/screenshots/editor.png) |

The full-screen editor has a vim mode, and it's actually vim: normal
mode, operators and motions, visual mode, registers, macros and ex
ranges. Everyone else gets the usual editor.

Underneath, Iniquity's message menu is still there, with conferences,
newscan dates and text search, and the boxes on it say what's newest,
hottest, loudest and unread right now. QWK offline mail takes the whole
lot away in a packet for a reader, and brings the replies back.

| | |
|---|---|
| ![The message menu](docs/screenshots/message-menu.png) | ![QWK offline mail](docs/screenshots/qwk.png) |

## Files

disflux carries about 70,000 files: the whole textfiles.com archive,
scene art packs, zines and a lot of magazine scans. The search page looks
through all of it as you type. The browser opens ZIPs and tarballs in
place, and the art gallery plays ANSI back at modem speed if you want
the 1995 experience.

| | |
|---|---|
| ![File search](docs/screenshots/search.png) | ![The file browser](docs/screenshots/browse.png) |
| ![The art gallery](docs/screenshots/gallery.png) | ![Points and the lottery](docs/screenshots/points.png) |

Files get votes and comments too. Uploads from new callers can be held
until the SysOp has looked at them. Points come from upvotes, filled
requests and the games, and there's a lottery every night.

The gallery has a search page of its own (art by name, group and the
SAUCE inside the pieces, with an I'm Feeling Lucky), and the file
menu keeps a running board of what's newest, hottest, heaviest, the
deepest dig into the archive, and who's uploaded the most.

| | |
|---|---|
| ![Searching the gallery](docs/screenshots/gallery-search.png) | ![The file menu](docs/screenshots/file-menu.png) |

## Chat

MRC is chat shared between BBSes, laid out like irssi. Penny is disflux's
AI hostess. She runs on the board's own hardware, knows its file areas
and menus, and can look things up on the web when she needs to.

| | |
|---|---|
| ![MRC chat](docs/screenshots/mrc.png) | ![Chat with Penny](docs/screenshots/ai.png) |

## The news and the weather

Every few hours the board writes its own paper: its AI reads each
section's news (world, tech, science, retro) from several outlets and
writes each story from all of them, every sentence naming the sources it
comes from and checked against them. The front page comes in editions,
with a line on what each story is about and the ones you haven't read
marked; callers vote on stories and leave one-line comments.

The weather is the sky over your town in half-block pixels, animated,
with the week ahead, the last month against normal, the drought monitor
and the whole country on a map.

| | |
|---|---|
| ![The news](docs/screenshots/news.png) | ![A story](docs/screenshots/news-story.png) |
| ![The weather](docs/screenshots/weather.png) | ![The national map](docs/screenshots/weather-nation.png) |
| ![The last 30 days](docs/screenshots/weather-history.png) | ![The drought monitor](docs/screenshots/weather-drought.png) |

`h` is the last month as a chart, the warmest and coldest days and the
rain, and what happened on this day in years past; `d` is the U.S.
Drought Monitor for your county, this week and the six months before it.

## Doors

LORD, BRE, TradeWars 2002, Overkill II, Usurper and Food Fight run as
doors. The DOS ones run in an x86 machine built into the board (a 386 in
real mode, with BIOS, FOSSIL, DOS, SHARE and batch files), so there's no
DOSBox or FOSSIL driver to set up, and several callers can play the
multi-node ones at once. The doors menu shows who's playing what, and its
scoreboard keeps the hottest door, the longest session and the one nobody's
touched.

LORD takes in-game modules: the DOS ones of the '90s (Barak's House is on
disflux) and new ones written in Python, which the board runs on the host
in place of a DOS program ([phrac/lord-igms](https://github.com/phrac/lord-igms):
a hermit's dial-up terminal, a fishing pond, a drunk knife-grinder and
more, with a jar of fairies they share).

| | |
|---|---|
| ![The doors menu](docs/screenshots/doors.png) | ![LORD in the built-in DOS machine](docs/screenshots/lord.png) |
| ![Barren Realms Elite](docs/screenshots/bre.png) | ![TradeWars 2002](docs/screenshots/tradewars.png) |
| ![Food Fight](docs/screenshots/foodfight.png) | ![The doors scoreboard](docs/screenshots/scoreboard.png) |
| ![LORD's Other Places, with modules in Python](docs/screenshots/lord-places.png) | ![The Hermit's Terminal, a module in Python](docs/screenshots/lord-hermit.png) |

## The casino

The casino is drawn in half-block pixels, in disflux's underground look
(neon, skulls and floppy disks; a board can have the classic red carpet
and green felt instead). The house gives each caller a free stack of
chips a day, and the cashier at the players club cashes the winnings in
for file points and minutes, so nobody loses anything real. Video poker is Jacks or Better with the pay table on the
machine; blackjack pays 3 to 2 and the dealer stands on all 17s (and
there's a secret card count, if you know where to look); roulette has the
whole layout; craps has the pass line, don't pass, the field and come
bets. The bar pours drinks on the house, and the players club turns what
you've played into the buffet, a suite upstairs and show tickets.

| | |
|---|---|
| ![The casino](docs/screenshots/casino.png) | ![The slot machine](docs/screenshots/slots.png) |
| ![Video poker](docs/screenshots/poker.png) | ![Blackjack](docs/screenshots/blackjack.png) |
| ![Roulette](docs/screenshots/roulette.png) | ![Craps](docs/screenshots/craps.png) |
| ![The bar](docs/screenshots/bar.png) | ![The players club](docs/screenshots/players-club.png) |

## More games

Carrier Lost is a war-dialing game: tune the dial across a dead exchange,
lock onto carriers, catch the handshake signals for a bigger haul, and
cash out before the trace hits 100. Callers make ANSI art from a prompt,
drawn by an image model trained on the scene's art on the SysOp's own
GPU, filed in a gallery of its own where they vote on each other's.
Tarot deals the card of the day, a three-card spread for a question, or a
fortune cookie: the board draws the cards, and the AI reads them. The reading room puts a new shelf of
textfiles out by the fire every day, with a note from Penny. Achievements
come along the way, for posts, uploads, calls and days in a row.

| | |
|---|---|
| ![Carrier Lost](docs/screenshots/carrier.png) | ![A gen: ANSI art from a prompt](docs/screenshots/ansigen.png) |
| ![Tarot: the card of the day](docs/screenshots/tarot-day.png) | ![Tarot: past, present and future](docs/screenshots/tarot-three.png) |
| ![The reading room](docs/screenshots/library.png) | ![Achievements](docs/screenshots/achievements.png) |

Leave it alone at a menu for a while and the screen saver comes on.

![The screen saver](docs/screenshots/saver.png)

## You

User config is one screen: where you are, a profile, SSH keys, the theme,
the code and markdown colors, vim keys in the editor, sixel pictures and
clickable links, your time zone, the screen saver. Your stats put you
against everyone else on the board. The time bank keeps the minutes you
didn't use today for a day you need them, and the about screen says what
the board is.

| | |
|---|---|
| ![User config](docs/screenshots/config.png) | ![Your stats](docs/screenshots/stats.png) |
| ![The time bank](docs/screenshots/timebank.png) | ![About flux/bbs](docs/screenshots/about.png) |

## For the SysOp

Everything is set up from inside the board, as on Iniquity: the SysOp
menu, the system configuration (live: changes take effect without a
restart), the logon sequence step by step, access levels, the message and
file areas (the textfiles.com archive is 942 of them), every prompt the
board says (by name, so a theme can change any of them), and a viewer for
the TheDraw fonts the menus' logos are set in. Settings are TOML files
underneath, and the editors write them back.

| | |
|---|---|
| ![The SysOp menu](docs/screenshots/sysop.png) | ![System configuration](docs/screenshots/sysop-config.png) |
| ![The logon sequence](docs/screenshots/logon-sequence.png) | ![Access levels](docs/screenshots/access-levels.png) |
| ![Message areas](docs/screenshots/message-areas.png) | ![File areas](docs/screenshots/file-areas.png) |
| ![The string editor](docs/screenshots/strings.png) | ![The TheDraw font viewer](docs/screenshots/fonts.png) |

## What else is in there

- SSH (keys too) and telnet. Telnet callers are warned that it isn't
  encrypted and told how to come back over SSH.
- A picture bot check, per-address limits, bans that escalate, password
  rules and email verification.
- FTN echomail, netmail and file echoes (TIC) over binkp, with AreaFix
  and FileFix, several networks at once.
- QWK offline mail, request boards, @mentions, a voting booth,
  oneliners, rumors, infoforms, new-user voting and profiles, and
  feedback to the SysOp from callers not yet logged in.
- Achievements callers earn (posts, uploads, calls, days in a row,
  doors...), and top lists for the week, the month and all time.
- Screen themes callers can pick, animated screens, and plain text for
  screen readers and scripts.
- URLs in messages, file descriptions and the text viewer are clickable
  (OSC 8) on terminals that take links.
- Python scripting (`import fluxbbs`) in place of Iniquity's IPL.
- A MeshCore radio bridge that turns mesh channels into message areas.
- An editor for every setting, the waiting-for-caller screen, scheduled
  events, nightly backups and a JSON feed for the board's website.
- An AI assistant can call in as a co-SysOp ([AGENTS.md](docs/AGENTS.md)).
- It imports an existing Iniquity board: users, messages, files, menus,
  strings and screens.

[FEATURES.md](docs/FEATURES.md) has all of it in detail, and
[NEW.md](docs/NEW.md) lists what's changed since Iniquity.

## Running your own

Grab a [release](https://github.com/phrac/fluxbbs/releases) for Linux
(x86_64 or ARM64, so a Raspberry Pi works). It's a static binary with
nothing to install: unpack it and run `./fluxbbs`. There's also a Docker
image, `ghcr.io/phrac/fluxbbs`.

From source you need `cargo`. Showing PDFs needs Poppler
(`poppler-utils`) and Python scripts need `python3`.

    tools/setup_host.sh                      # first time: build
    cargo run --release --features full      # serves fluxbbs.toml
    ssh -p 2222 anyname@localhost            # any UTF-8 terminal
    syncterm ssh://anyname@localhost:2222    # a proper BBS terminal
    syncterm telnet://localhost:2323         # plain telnet

A new board creates its SysOp on first start. Log in as `SysOp` with the
password `SYSOP` (it makes you pick a new one), and the system password
is `SYSTEM` until you change it in `*C`. New callers choose "apply" or
type `new` at the login.

The data lives in `data/fluxbbs.db` and uploads in `files/`. Doors, the
games, AI chat, echomail, MRC, MeshCore, PDFs and scripting are optional
components: a plain `cargo build` gives you the core board and
`--features full` gives you everything. [BUILDING.md](docs/BUILDING.md)
explains how to choose.

To run it in a container:

    docker compose -f deploy/docker-compose.yml up -d --build

The image has every component built in and the stock files at
`/srv/inq/iniquity`. Your board's directory is mounted beside them. SSH
is on 2222 and telnet on 23. [SYSOP.md](docs/SYSOP.md#running-in-a-container)
has the rest.

## Documentation

| Doc | What's in it |
|---|---|
| [FEATURES.md](docs/FEATURES.md) | Every feature in detail |
| [NEW.md](docs/NEW.md) | What's new since Iniquity, and what changed on purpose |
| [SYSOP.md](docs/SYSOP.md) | Configuration, backups, importing, security, themes |
| [FILES.md](docs/FILES.md) | File areas, transfers, the gallery, pictures, PDFs, textfiles.com |
| [DOORS.md](docs/DOORS.md) | Drop files, native doors, BBSLink and the DOS machine |
| [SCRIPTING.md](docs/SCRIPTING.md) | Python scripts |
| [CODES.md](docs/CODES.md) | Display codes |
| [ARTWORK.md](docs/ARTWORK.md) | Swapping in your own art |
| [art/README.md](art/README.md) | How the stock screens are generated |
| [AGENTS.md](docs/AGENTS.md) | Guide for an AI agent calling in as the SysOp's assistant |
| [BUILDING.md](docs/BUILDING.md) | The core board and the optional components |
| [DEVELOPMENT.md](docs/DEVELOPMENT.md) | Code layout and tests |

## Status

Everything in the Iniquity 1.00a26r5 manual is done, along with
everything above. The DoorParty door network is the one thing still on
the list. A few things aren't planned: Tetris (its source isn't in that
release), modem setup, MicroDOS, the local status bar, and the
Oblivion/Renegade converters.

## License

MIT. See [LICENSE](LICENSE).

## Credits

Iniquity BBS is © 1995–97 Mike Fricker and the Iniquity Development
Team. flux/bbs began as a port of it and reimplements its behavior in
Rust. Iniquity's own release and source aren't in this repository. The
stock prompts are Iniquity's, as are the classic screens and menus in
`classic/`. The default screens and menus are disflux's.
