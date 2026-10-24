# Features

What flux/bbs does, area by area, and how to use and set up each part.
The [README](../README.md) has the short list, and [NEW.md](NEW.md)
says what's new over Iniquity.

**Contents**

- [In detail](#in-detail): connections, keeping bots out, logon, menus
  and screens, messages, files, profiles, doors, SysOp tools, importing
- [QWK offline mail](#qwk-offline-mail)
- [Infoforms and new user voting](#infoforms-and-new-user-voting)
- [AI chat](#ai-chat)
- [Message center](#message-center)
- [MeshCore](#meshcore)
- [FTN echomail](#ftn-echomail)
- [From 2.0 and 2.20](#from-20-and-220)
- [Not planned](#not-planned)

## In detail

### Connections

#### SSH

flux/bbs speaks SSH (through russh), including the older algorithms
that SyncTERM's cryptlib needs.

A caller can log in without the bot check or login prompts in two ways:

- With an SSH key kept on their account (`UK`, or `k` in user config).
- With the account's password as the SSH password (a terminal's stored
  login).

[SYSOP.md](SYSOP.md) has the details.

#### Telnet

Telnet supports binary mode, server echo, window size (NAWS) and
terminal type. Zmodem works over it.

#### Character sets and color

- CP437 for BBS terminals.
- UTF-8 for everything else, with DOS's wrap and clear behavior.
- aixterm bright colors and iCE backgrounds.
- 256-color and 24-bit color art passes through.

#### Nodes

Each caller is on a node. Nodes give you who's online, node messages,
broadcasts and multinode chat.

#### Paging the SysOp

`CP` (or `>C` from the matrix) pages the SysOp. The page goes to a node
where a SysOp is logged in, and they're asked, wherever they are,
whether to chat. The two then chat on a split screen (CHATMODE.ANS),
each keystroke sent as it's typed.

| Type | To |
|---|---|
| `/q` | end the chat |
| `/bye` | (SysOp) hang the caller up |
| `/cls` | clear the windows |
| `/?` | show CHATHELP |

`[chat]` in `fluxbbs.toml` sets the chat hours, the override ACS and
password, the pages per call and how long a page lasts. Outside the
hours, or with no SysOp online, the caller is offered email instead.

### Keeping bots out

#### The bot check

Every caller passes a bot check before anything else, on SSH and
telnet. Three big characters are drawn with background color in a
field of noise, and the caller types them. A person reads them; a
script reading the text sees only noise. The key-combination check is
still there as an option. See [SYSOP.md](SYSOP.md).

#### Limits and bans

- Connection limits per address.
- Bans for repeated failures, doubling each time, up to a day.
- Allow and deny lists.
- A short idle limit before login.

Bans and failures are kept in the database, so bans survive a restart.
At logon, SysOps are told how many failures and bans there have been
since their last call. `*G` lists them and lifts bans.

A SysOp locked out of their own board can run `fluxbbs unban
ADDRESS|all` from the shell.

### Logon

- The matrix menu, login and system password.
- The new-user application. Each personal question can be switched
  off, handles are kept as typed, and the NEWCFG screen lets the caller
  check their entries afterwards.
- Access levels and daily limits.
- A recheck every N calls: the caller is asked their birth date, or the
  last four digits of their phone number, again. Set it in `*C`, Access
  and Logon. These are the manual's "calls before check" options,
  rebuilt from the manual.

What happens after login is set with `*O`; see
[Logon sequence](#logon-sequence-o).

### Menus and screens

#### The display engine

| Code | Does |
|---|---|
| `\|XX` | colors and controls |
| `%XX` | MCI codes |
| `@`, `&` | positioning |
| `{`, `}` | limits and padding |
| `~XX` | fields |

The engine also shows repeat screens (TOP/MID/HIL/BOT) and picks random
and day-of-week variants of a screen. [CODES.md](CODES.md) lists the
codes.

#### The menu engine

- Hot keys, and `/` and `//` commands.
- `#`: an area number typed at the prompt, for `M#` and `F#`.
- FIRSTCMD, stacked commands, and gosub/return.
- Generic and lightbar menus.
- ACS expressions, passwords, fallbacks and command help.

#### Lightbar menus

- They take their commands' keys too: hot keys, `/X`, and digits for
  `#`, typed on the help line.
- The arrows go by where options sit on screen, so a grid of options
  works.
- They hold up to 29 options.

#### Screen themes

A theme is a directory of screens that replaces the stock ones (the
original's text libraries). Callers pick theirs with `UT`.

#### Scripting

Lua takes the place of IPL. `-X name` and `%!name` run
`scripts/name.lua`, which has a `bbs` API for output, input, the user
record, menu commands and stored values. See
[SCRIPTING.md](SCRIPTING.md).

### Messages

The classic message system has areas and conferences, the reader,
posting and replying, quoting, the full-screen editor, newscans and
mandatory messages, email and auto-signatures. The
[message center](#message-center) is the newer way to read.

#### Quoting

A quote in a reply opens with who wrote it, then each line with `> `
(a quote of a quote, `>>`):

```
phrac wrote:
> test message - use this area for general chat
```

The original put the author's initials in front (`P> `), which said too
little. `MsgQuoteHead` is the opening line; it's kept in the message,
so it's plain.

#### Editing your posts

Callers can edit what they've posted, as on reddit, but what they
posted stays. `e` in a reader on your own message opens the editor
empty ("what you posted stays as it was; this is added under it"); what
you write there is added at the end, under a dated line:

```
works for me, nice board

[edit 10/24/26 22:11]
also: try the doors menu
```

The signature stays at the very end. `MsgEditMark` is the dated line;
it's kept in the message, so it's plain text. Echomail already sent
doesn't change. A moderator (or co-SysOp) editing someone else's post
still gets the whole text, to clean it up.

#### Taking it back

Everyone posts something they regret. `d` on your own post (in either
reader, in any area but email) offers a way back, kindly but on the
record:

- **Take it back.** Within `take_back_minutes` of posting (15 to start),
  while nobody has commented, voted or replied, it goes as if you never
  posted it.
- **Retract it.** Everyone else sees "[retracted by its author
  10/24/26]" in its place, with the comments and votes still under it
  so the thread makes sense. You and the moderators still see what it
  said. `d` again undoes it.
- **Burn it.** Gone for good: its text, the comments and votes under
  it, any attachment, and what a retraction kept. It costs
  `burn_points` file points (25 to start), a small sacrifice you can
  earn back; on a board without file points it's free. It asks twice.

Nobody is told, and nothing says who took what back beyond the
placeholder. A taken-back or burned post leaves a deleted stub
("[burned by its author ...]") that moderators see until the area is
packed. On an echomail area, copies already sent to other boards stay
there, and it says so. Both settings are in `[messages]`, and in `*C`,
Caller Boards ("Take-back minutes", "Burn points"). Moderators deleting
someone else's post still use the usual delete.

#### Editor hints

As the full-screen editor opens, its prompt row shows one of the
board's mottos, at random, until the first key: "the internet never
forgets", "never forget", "every word goes on the record", or that `n`
on the main menu tells the SysOp what the board should do. They're the
strings `FsHint1`, `FsHint2`... (`*E` changes them; add more by number,
or blank one to drop it).

#### Attachments

A file can be attached to a message. This follows the manual.

- Who may attach is an ACS each for public messages and email, in `*C`.
- The file is uploaded after posting, or a SysOp names one on disk.
- It's kept under `data/attach`, by the message's row id.
- It's offered for download when the message is read.
- It's removed when packing drops the message.

#### Uploading a prepared message

A caller can upload a message instead of typing it. It lands in the
editor to look over. `*C` has an ACS and "ask" toggles for it.

#### Email tags

Tags split email up, for a separate support mailbox, say. This follows
the manual.

| Command | Does |
|---|---|
| `E/ n` | tag the email written this call with `n` |
| `E\ n` | pick tag `n` for reading |
| `E+` | show only email tagged with it |
| `E-` | show all email again |

### Files

File areas have conferences, the lightbar file lister, search and
newscan, and local upload. [FILES.md](FILES.md) has more.

#### Downloads

- Single or batch, with points, ratios and daily limits.
- Before a download the caller can ask to be logged off when it's done.
  A countdown follows the download. Set in `*C`, Transfers.

#### Uploads

- Blind or named.
- Descriptions come from FILE_ID.DIZ.
- A single piece of art, alone or alone in a ZIP, is described from its
  SAUCE: title, artist/group and date, and comments.

#### Upload tests

Uploads are tested on the FILETEST screen. The settings are in `*C`,
Upload Tests.

- ZIPs are read through, and a CRC failure refuses the file.
- ZIPs can be held to an age limit.
- An outside command can be run on every upload, in place of the DOS
  virus scanner: `clamscan --no-summary %F`, say.
- Files on the delete list (BBS ads) are stripped from ZIPs, and the
  archive comment is added.

#### Protocols and archivers

Zmodem is built in. Other protocols and archivers are external
programs (RAR, 7Z and LHA through bsdtar or 7-Zip).

#### Browsing files

`FW` (L on the file menu) works like the art gallery, over the file
areas. `FV` opens an archive the same way.

- Areas are folders, with their file counts and newest upload.
- Archives (ZIP, tar, tar.gz) open as folders in place.
- Names are shown whole, and sizes as "4.5 KB".
- Typed names keep their case.

| Key | Does |
|---|---|
| Enter | show a file: text and ANSI in the viewer, pictures as ANSI (animated GIFs play) |
| `D` | download, with the area's usual checks |
| back | go up a level |
| `Q` | leave the browser, from anywhere |

#### The file base on screens

Screens can show what's in the file base, with `%FT`...`%FV` (see
[CODES.md](CODES.md)):

- totals
- the newest upload
- the most downloaded and the biggest files
- the top uploader
- a file at random, with a line of its description

`FW newest` (or `hot`, `heavy`, `dig`, `kingpin`) opens the browser on
that file. That way a lightbar option on a menu can take the caller
straight to it.

### Profiles

Finger shows a caller's profile. Open it with `UF`, with `f` on a
message in the reader, or by giving a name after the user list.

It shows:

- location and note
- since when, and when last on (or on now, unless invisible)
- calls, posts, uploads and downloads
- their latest post and uploads
- their plan: up to six lines they write in their profile
- their ANSI header, if they've uploaded one
- their real name, only if they choose to show it

Phone, address and birth date stay private, and so does the real name
unless its owner turns it on.

#### Your profile

In user config, the profile item (`Q`; on a board's own USERCFG screen,
wherever it puts it) opens a short list:

| Key | Does |
|---|---|
| `p` | write your plan |
| `r` | show your real name on your profile, or not |
| `u` | upload an ANSI header: up to 8 lines; only colors and cursor-right moves are kept |
| `c` | clear the header |
| `m` | see what the board remembers about you (below), and clear it |
| `v` | see your profile as others do |

#### What the board remembers

After each call, the AI chat's model updates a short memory of the
caller's interests here: the boards they read and post in, the file
areas they take files from, the doors they play, and what they asked
the AI about. It runs in the background after they've gone. It's told
to keep it coarse and friendly: no personal details, message text,
times or habits, and nothing about other people. It's at most a few
lines.

The AI chat is given it, so it can suit itself to the caller next time.
The caller can read it and clear it from their profile (`m`). Nothing
is kept without `[ai] enabled`.

#### Caller notes

SysOps keep private notes on a caller: "regular, always asks for phrack
files", "new, seems fine, vouch for more access". Open them with `*N`
(name the caller, or be asked), or `:` in the user editor. Each note is
dated, with who wrote it; `-N` removes note N.

Notes are for SysOps only. A SysOp sees the latest few under a
caller's profile. The AI chat is given them too, to suit how it treats
the caller, and is told never to reveal, quote or hint at them.

#### Bots

Flag `B` marks an account as a bot (an agent that calls in, say). A
bot gets the board as it comes: no memory is kept, the AI chat isn't
given notes or memory, and menus show no hints. A bot can't start a
request on a [request board](#request-boards), and always signs its
posts (it isn't asked about posting anonymously). [AGENTS.md](AGENTS.md)
covers setting one up as the SysOp's assistant.

#### Menu hints

When a lightbar menu opens, its help line can show a hint about what's
waiting for the caller instead of the first option's help: unread
email, mentions, new posts in their boards, or new files since their
last scan (when that's under a thousand). It names the menu's key for
it when the menu has one, picks one at random when several apply, and
gives way to the usual help once they move the bar. A request of
theirs that someone [filled](#request-boards) comes first, and is
shown once.

A FINGER screen can dress the profile up with these markers:

| Marker | Is |
|---|---|
| `~UN` | handle |
| `~RN` | real name, if they show it (else blank) |
| `~LO` | location |
| `~UO` | note |
| `~FC` | first call |
| `~LS` | last seen |
| `~CA` | calls |
| `~PO` | posts |
| `~UL` | uploads |
| `~DL` | downloads |
| `~LP` | latest post |
| `~LU` | latest uploads |
| `~KA` | karma |
| `~P1`-`~P6` | the plan's lines |

### Around the board

#### Lists and other features

Oneliners, the automessage, last callers, history and statistics, the
BBS list, user configuration and the user listing.

#### The BBS list

Each entry has a telnet and an SSH address (`host:port`). Screens show
them with:

| Marker | Is |
|---|---|
| `~TN` | the telnet address |
| `~SS` | the SSH address |
| `~SH` | the SSH address shortened to `:port` on the telnet host, for lists |
| `~PN` | the telnet address (for older screens) |
| `~IN` | the description ("other info"): its first line |
| `~I2`-`~I9` | its later lines, in SHOWBBS |

The description can be several lines, up to 10. Adding a board, it
takes lines until a blank one, so a pasted description comes in whole;
in `*B` it's edited in the full-screen editor. Lists show its first
line. SHOWBBS shows the rest at `~I2`-`~I9`, or, on a screen without
them, on the rows after the screen's last field, lined up under the
first.

#### Infoforms and voting

Infoforms (questionnaires, mandatory or not) and new user voting; see
[Infoforms and new user voting](#infoforms-and-new-user-voting).

#### The time bank

| Command | Does |
|---|---|
| `!D` | deposit minutes |
| `!W` | withdraw minutes |
| `!E` | deposit download kilobytes |
| `!I` | withdraw download kilobytes |
| `!V` | show the account |

Each level has limits on what's kept and moved each day (`*L`). The
stock TIMEBANK menu's withdraw entry runs `!W`.

#### Offline mail

`QD` packs new messages for an offline reader, with email as
conference 0. `QU` posts the replies. The settings are `[qwk]` in
`fluxbbs.toml`. See [QWK offline mail](#qwk-offline-mail).

### Doors

Doors can be native programs, or DOS programs run in the built-in DOS
machine. See [DOORS.md](DOORS.md).

Every play is logged: who, which door, when and for how long. The doors
menu's screen can show what's in it with the `%G?` codes
([CODES](CODES.md#door-games-new)): the last door played and by whom,
the most played, who has spent the most time in doors, the longest
single play, who's in one now, the door gathering dust, and the
caller's own favorite, last and one they haven't tried.

| Command | Drop file |
|---|---|
| `D1` | DORINFO1.DEF |
| `D2` | DOOR.SYS |
| `D3` | CHAIN.TXT |
| `D4` | DOOR32.SYS |
| `DZ` | DOOR.SYS and DORINFO1.DEF |
| `D0` | none |
| `D7` | DOOR.SYS, read back afterwards (from 2.0) |

### SysOp tools

The `*` commands on the SysOp menu:

| Command | Does |
|---|---|
| `*C` | system configuration, written back to `fluxbbs.toml` keeping its comments; its sections in groups, with what's on and the board's state |
| `*E` | prompt strings |
| `*#` | menus |
| `*L` | access levels |
| `*U` | users, with colors, preferences, auto-signature and infoforms |
| `*F` | file areas |
| `*Y` | files: validate, rename, move, delete, extract or apply DIZ; `/` finds a file in any area, and `edit` in a file list opens it ([FILES](FILES.md#managing-files)) |
| `*S` | sort areas |
| `*>` | download any file |
| `*%` | local upload |
| `*K` | pack file areas |
| `*A` | message areas |
| `*P` | pack message areas |
| `*@`, `*!` | conferences |
| `*R` | all email |
| `*X` | protocols |
| `*Z` | archivers |
| `*B` | BBS list |
| `*H` | statistics |
| `*I` | infoforms |
| `*T` | themes |
| `*G` | security |
| `*V` | events |
| `*O` | logon sequence |

#### User flags (`*U`)

In the user editor's flags field (`=`), the info line says what the
flag under the cursor is for, and which menus, areas and settings test
it. `?` describes it. The descriptions are kept in `[flags]` in
`fluxbbs.toml`.

#### File areas (`*F`)

`*F` is a bar list of the file areas in tree order.

| Key | Does |
|---|---|
| `/` | search the list |
| Enter | edit the area under the bar |
| `I` | add an area before it, under the same parent, and edit it |
| `D` | delete it |
| `M` | move it |
| `S` | sort a level by name: the sub-areas of the area under the bar, or the areas beside it if it has none |

#### Adding and packing files

- `*%` adds files copied into an area's directory.
- `fluxbbs add-files` does the same for every area from the shell (for
  a mirror, say). A ZIP without a FILE_ID.DIZ is described by what's in
  it.
- `*K` packs file areas: it forgets files missing from their
  directories. `fluxbbs pack-files` does it from the shell, as
  FILEPACK.EXE did.
- `fluxbbs textfiles` puts the textfiles.com archive on the board as a
  tree of areas ([FILES](FILES.md#the-textfilescom-archive)).

#### Security (`*G`)

`*G` shows the current bans and recent failed logins and bot checks,
and lifts a ban.

#### Events (`*V`)

Events are jobs run once a day at a set time. One scheduler in the
server runs them all (no per-node events, and no modem off-hook).

An event's command goes to the shell, or is one of the built-in jobs:

| Job | Does |
|---|---|
| `pack-messages` | packs the message areas |
| `pack-files` | packs the file areas |
| `backup [DAYS] [DIR]` | backs up the database and settings, keeping DAYS days (new; [SYSOP](SYSOP.md#backups)) |
| `dos COMMAND` | runs a DOS program or batch file in the built-in machine, for a door's maintenance |

Options:

- `force` disconnects callers first, with the stock "event initiating
  NOW" string.
- `run_missed` catches up after downtime the same day.
- "Run it now" in the editor starts an event by hand.

#### Logon sequence (`*O`)

`*O` sets the steps after logging in, in the order you put them, each
on or off. The steps are mail, mandatory messages, new posts, last
callers, who's online, the automessage, your stats, oneliners, pauses,
and any screen of your own. The oneliners run in place, so they needn't
come last.

The sequence is kept in `logon.toml` with the board, and the start menu
(STARTUP) is built from it. Without the file, STARTUP runs as written.

Two special steps:

- **New users stop here** sends callers who aren't validated to
  NEWUSER. They see the steps above it too.
- **Fast logon** asks validated callers (`s50`) whether to go straight
  on to the main menu (STARTUP's fallback), skipping the steps after
  it. Put it first for a quick way in, or after the steps everyone must
  see.

#### The waiting-for-caller screen

`fluxbbs --wfc` shows the original's waiting-for-caller screen
(WAIT4CAL, ANSIWFCS) on the console while the server runs.

- F1-F8 (or 1-8) and the arrow keys pick from six pages of commands:
  local logon, a shell, the SysOp editors (run as user #1, the SysOp),
  launch a menu, read or view email, edit a text file, and stats pages
  (system, totals, today, last callers).
- The node and event fields show the callers on and the next event.
- The window that echoed the modem shows the log. All of the log goes
  to `data/fluxbbs.log`.
- ESC exits, unless `[wfc] esc_to_exit` is off. `*C` has the settings.

Modem commands, modem setup and MicroDOS are left out. WAIT4CAL.PAS is
damaged, so the stats pages are rebuilt from their titles and
fragments.

#### Shell commands

| Command | Does |
|---|---|
| `fluxbbs unban ADDRESS\|all` | lift bans |
| `fluxbbs add-files` | add files copied into every area's directory |
| `fluxbbs pack-files` | pack the file areas |
| `fluxbbs textfiles` | the textfiles.com archive as file areas |
| `fluxbbs ftn ...` | FTN jobs; see [FTN echomail](#ftn-echomail) |
| `fluxbbs --wfc` | the waiting-for-caller screen |

### Importing

flux/bbs can import an existing install:

- users (passwords re-hashed with argon2)
- message bases
- file records and descriptions
- areas and conferences
- levels, strings and menus
- text files
- statistics, history and callers
- the BBS list
- infoforms and their answers
- new user votes
- text libraries (unpacked into themes)

## QWK offline mail

### Downloading a packet

`QD` builds NAME.QWK (`[qwk] packet`) and sends it. It holds the new
messages in the caller's areas, from the current conference or every
conference, and their new email.

- Email goes in as conference 0, with PERSONAL.NDX.
- Years are four digits.
- "Include your own messages?" does what it says.
- Areas with a password are left out.
- Once the packet is down, the newscan pointers can move past what was
  packed: to the newest message packed, not to the time of the
  download.

### Uploading replies

`QU` takes NAME.REP back (a SysOp can give a local file instead). It
posts the replies where the caller may post.

- The same packet won't post twice.
- Long reply lines are wrapped, not cut at 79 columns.
- Replies to unknown users are refused in email. In public areas they
  are posted to the name.

## Infoforms and new user voting

### Infoforms

An infoform is a text file in `text/` with input points in it (see
`text/EXAMPLE.INF`).

| Code | Reads |
|---|---|
| `~SN` | text |
| `~SU` | text, upper case |
| `~SM` | text, mixed case |
| `~SC` | text, first letter up |
| `~SE` `~SW` `~SI` | text, in the r5 styles |
| `*` (bare) | plain text |
| `~BD` | a date |
| `~PN` | a phone number |
| `~NM` | digits |
| `~FN` | a filename |

| Code | Sets |
|---|---|
| `~NN` (two digits) | how long answers may be |
| `~**` | puts the length back to 254 |
| `~*=` | answers required |
| `~*-` | answers optional |

Mandatory forms are filled in at logon (`IM` on the stock STARTUP
menu). Callers see and redo theirs with `IN`.

### New user voting

With new user voting (`[nuv]`), users who pass the voter ACS vote on
the users who match the candidate ACS. They vote at logon or from the
main menu.

- Voters can read the candidate's NUV infoforms and leave comments.
- Enough yes votes give the candidate the validated level.
- Enough no votes delete them.

## Plain text

For bots, and callers who'd rather: no colors, cursor moves, art or
pauses, just lines. A caller's text mode (user config, `b`) is plain,
ANSI, or auto, which is plain for bots (flag [`B`](#bots)) and ANSI for
everyone else. It takes effect at once.

- **Logon.** A bot logging in with an SSH key gets plain text from the
  start: a line on mail, new posts, callers online and time left, then
  the menu the logon sequence would end on. The sequence is skipped.
- **Menus.** Listed when entered (key and description), then a prompt
  (`Main> `) that takes a line. `?` lists them again.
- **Prompts.** Yes/no takes `y` or `n` (Enter for the default); a
  prompt with a default shows it in brackets, and Enter takes it.
  Nothing waits for a key.
- **Lists.** The bar lists (file areas and the like) are numbered
  lines and a prompt.
- **Message boards.** Line commands: a board's number, then `l` its
  threads, `r #` read, `t #` the thread, `n` what's new, `p` post,
  `reply #`, `c # text` comment, `+ # text` / `- # text` vote, `q`
  back. [AGENTS](AGENTS.md#in-plain-text) has them all.
- **Writing.** A line at a time; `.` alone saves, `/abort` gives up.
- **Request boards.** Open requests first, filled ones marked; a
  comment can carry a file link (`c # file:AREA/NAME`, checked to be
  there, or `c # /l` to pick one).
- **Files.** Lists, searches and browsing give each file on a line
  with its link and description, 25 a page; file info adds the link.
  Uploads ask each question on its own line; protocol and other
  single-key choices take a line.
- **Not yet.** Marking a request filled, downloads from the lists, and
  the SysOp tools are as in ANSI, through the plain lists where they
  use them. Doors draw their own screens.

## AI chat

`AI` (J on the main menu) is a chat with a language model, for callers
the SysOp lets in.

### Using it

- Replies stream in as they're written, word-wrapped, without markdown
  or the model's thinking.
- The conversation is kept for as long as the caller stays; the AI is
  given its latest 40 messages each time, so a long one forgets its start.
- `/new` starts over; `/quit` or ESC leaves.

### Who may use it

`[ai] acs` is `s250|fI` as stock: SysOps, and anyone given flag I in
the user editor (`*U`). `daily_limit` caps a caller's messages a day
(not the SysOp's).

### Settings

All of these are under "AI Chat" in `*C`, and `[ai]` in
`fluxbbs.toml`.

| Setting | Is |
|---|---|
| `enabled` | turns it on |
| `acs` | who may use it |
| `base_url` | the server, up to `/v1`: any OpenAI-style chat completions server |
| `model` | the model |
| `api_key_env` | the environment variable holding a key, if the server needs one |
| `daily_limit` | a caller's messages a day |
| `web_search` | lets the model search the web |
| `search_url` | a SearXNG instance to search with |
| `search_key_env` | the environment variable holding a Brave Search API key |
| `read_pages` | lets the model read pages it found |
| `name`, `system_prompt`, `max_tokens`, `reasoning_effort`, `timeout_seconds` | what they say |

### Searching the web

With `web_search` on, the model may search the web, up to four times a
reply. It uses, in order of preference:

1. a SearXNG instance (`search_url`)
2. the Brave Search API (`search_key_env`)
3. DuckDuckGo's HTML page, which turns away bots now and then

With `read_pages` on too, it may read a page it found, as text, at most
8000 characters. It reads only from public addresses: never the board
itself, the LAN or the tailnet.

### Looking at the board

The model can always look at the board itself (`aiboard.rs`), as the
caller's own access allows:

- the menus, with their keys and where they lead
- the file and message areas
- the files in them, listed or searched

So it can say what's here and which keys to press.

![The AI finding files on the board](screenshots/ai-files.png)

![The AI giving the keys to download a file](screenshots/ai-howto.png)

### Stats and context

When the server reports usage (`stream_options.include_usage`):

- After each reply, a line (`AiStats`) says what it took: tokens
  written, how many of those were thinking, and tokens a second. Blank
  `AiStats` in `*E` to hide the line.
- The bottom bar shows how much of the model's context the conversation
  fills (`AiContext`). The context length comes from the server's
  `/models`.

### The screen

Under AICHAT.ANS, the conversation scrolls between the header and the
bar, which both stay put. The board keeps the lines it has shown and
draws the rows again one higher (terminal scroll regions left old text
showing through in SyncTERM).

A reply that fills the rows pauses (`AiMore`), for callers who pause.
The reply keeps coming in during a pause.

| Key | Does |
|---|---|
| any key | at a pause, go on |
| Esc | at a pause, let the rest run |
| Up/Down, PgUp/PgDn, Home | look back over the last 1000 lines (`AiBack`), at the prompt or a pause |
| End, or typing | come back |

## Tarot and fortunes

`AT` is a door for the board's AI: tarot readings and fortune cookies.
The board draws the cards; the AI only reads them. The stock door
menu's `T` opens the TAROT menu, whose options run `AT day`, `AT three`
and `AT fortune`; give TAROT.ANS lightbar markers and `type =
"lightbar"` and it's a lightbar like the rest (`%TR` and `%TF` show the
readings and fortunes left today). `AT` alone asks with a prompt
instead.

| Key | Does |
|---|---|
| `d` | the card of the day: one card, the same one all day for you, another tomorrow |
| `t` | three cards (past, present, future) for a question you type, or a general reading |
| `f` | a fortune cookie, on a slip with six lucky numbers under a crystal ball |
| `q` or Esc | back |

- The deck is the 78 cards of the Rider-Waite tarot; each card drawn
  is upright or reversed. Three cards are three different ones from a
  shuffled deck.
- The cards are drawn on screen with pictures, their place over them:
  each of the major arcana has its own (the Fool at the cliff's edge,
  the Tower struck...), in a double frame; the numbered cards show
  their pips, and the court cards a page, knight, queen or king with
  the suit's pip, in the suit's color. A reversed card is drawn upside
  down. The reading follows, paged.
- The pictures are CP437 text in the code (`tarotart.rs`), so they
  need no screens from the board, and use only the characters every
  terminal draws (no control bytes such as `○` or `♪`).
- The AI is told to be warm and a little mysterious but grounded: for
  fun and reflection, never foretelling death, illness or disaster,
  never giving medical, legal or financial advice.
- A caller has 3 readings (card of the day or three cards) and 3
  fortunes a day, as stock: `tarot_daily` and `fortune_daily` in `[ai]`
  (`*C`, AI chat, `P` and `Q`; 0 for no limit). The menu shows what's
  left; SysOps have no limit. The card of the day's reading is kept, so
  looking at it again later that day costs nothing.
- It takes the AI chat's settings and access (`[ai]`, its ACS), and
  each reading or fortune counts against the AI chat's daily limit too
  (`TarotLimit` says so).
- A parameter goes straight to one reading and back: `day`, `three` or
  `fortune` (say, `AT day` on a logon menu for a card of the day).
- A TAROTHDR screen heads each reading and fortune if the board has
  one; otherwise the `TarotTitle` line. Keep it to a line or two: the
  cards take 15 rows under it.

## Message center

`MH` (`r` on the message menu) and `MW Read` (`n` there) replace the
classic reader and newscan. The newscan prompt `SCANMSG` asks on the
way in. `MR` and `MN` are still there for menus that want them.

Callers see areas as "boards", laid out after Taiwan's PTT but in a
forum's words: boards, then threads, then the reader. At every level:

- arrows and Enter go in
- Esc (or left) goes back
- the bottom line shows that level's keys

A board can point its main menu's message key straight at `MH` (disflux
does), leaving the message menu behind `m`.

### The board list

Each board shows its number, name, what it's for (the area's `about`,
set in `*A`), how many callers are reading it now (PTT's popularity),
new posts, moderator (the sponsor) and the latest post's date.

| Key | Does |
|---|---|
| Enter | open the board, or folder |
| `n` | read everything new |
| `p` | post |
| `f` | search (`MO`) |
| `s` | put the busiest first: readers now, then posts this week (kept for the caller) |
| `h` | show MSGGUIDE, or the reader's help |
| `m` | go to the message menu |
| `a` | add the board or folder under the bar to "my boards", or take it off |
| `*` | go to "my boards" (from anywhere in the boards) |
| `!` | go to "Hot right now" (from anywhere in the boards) |
| `c` | make a board (callers who may; see [Callers' boards](#callers-boards)) |

The MHUB screens (TOP/MID/HIL/BOT) draw the list:

| Marker | Is |
|---|---|
| `~AN` | number |
| `~AT` | name |
| `~DE` | about |
| `~RD` | reading now |
| `~NW` | new posts |
| `~NM` | messages |
| `~LP` | latest post's author |
| `~LD` | latest post's date |
| `~SP` | moderator |

### Hot right now

While anything has been voted on in the last two days, "Hot right now"
heads the board list. It holds the threads being voted on in every
board you read, hottest first. `!` goes to it from anywhere in the
boards.

The MHOT screens (TOP/MID/HIL/BOT) draw it:

| Marker | Is |
|---|---|
| `~AT` | board |
| `~MK` | `*` if new |
| `~MT` | title |
| `~SN` | author |
| `~SC` | score |
| `~NW` | new |
| `~DS`, `~DT` | latest day and date |

### My boards

"My boards" is PTT's favorites. `a` in the board list adds the board
(or folder) under the bar, or takes it off.

- "my boards/" stays second in the top-level list.
- It's a folder of them, with their figures added up (strings
  `MyBoardsName` and `MyBoardsAbout`).
- `*` goes to it from anywhere in the boards.
- It's kept with the caller (`fav_boards`). A board they can no longer
  read drops out of it.

### Sub-boards

A message area can sit under another, to any depth, as file areas can
(PTT's board classes). Set `parent` to the other area's storage key
("Sub-area of" in `*A`).

- In the board list, a parent is a folder. Its name ends in `/`
  (`MsgGroupSuffix`), and its figures are those of everything under it.
- Enter opens a folder; Esc or left goes back up.
- A parent with posts of its own lists them first in its folder, as
  "name (own posts)".
- A sub-board needs the access of every area above it.

Elsewhere (the classic lists, scans, QWK) sub-boards are areas like any
other.

### Callers' boards

Callers can make boards of their own (PTT's personal boards). A caller
who passes `[messages] board_create_acs` gets `c` in the board list.
They give a name and a line on what it's for, and the board is made.

The creator is its moderator (its sponsor). They can delete and edit
posts there and take votes off. `o` in its thread list lets them:

- change its about line
- ban callers from posting and voting there (they can still read)
- lift bans
- set its tags
- pin or unpin the thread under the bar
- delete the board with its posts

A caller's board is an area in `msgareas.toml` like any other
(`created` and `banned` say whose it is), so the SysOp edits it with
`*A` as usual.

| Setting (`[messages]`) | Does |
|---|---|
| `board_create_acs` | who may make boards |
| `board_folder` | if set, new boards go under it |
| `boards_per_caller` | how many each caller may make |
| `board_empty_days` | `pack-messages` removes a caller's board that still has no posts this many days after it was made |

These are in `*C`, "Caller Boards".

### A board's threads

A board is a list of threads, a line each in PTT's order: number,
score, date, author, replies and title. A `*` before the title means
something in it is new. The latest activity comes first.

| Key | Does |
|---|---|
| Enter | read the thread, from its first new message |
| `n` | read what's new in the board |
| `p` | start a thread |
| `s` | sort by latest, hot (votes in the last 48 hours, newer counting more) or top (score); kept for the caller |
| `t` | show only threads with one of the board's tags (Esc shows them all again) |
| `v` | the thread as a tree ([below](#the-thread-tree)) |
| `f` | on a [request board](#request-boards): mark the request filled, or open again |
| `o` | the moderator's options |
| `h` | help |

The help line fills the bottom row; when there's no room, the keys at
its end (moderation, then tags) are left off, and `h` lists them all.

The MTHRD screens (TOP/MID/HIL/BOT) draw it:

| Marker | Is |
|---|---|
| `~MN` | number (`pin` for a pinned thread) |
| `~MK` | `*` if new |
| `~MT` | title (`[filled] ` in front on a request board, once filled) |
| `~FL` | on a request board, `open` or `filled` (`ReqOpen`, `ReqFilled`) |
| `~SN` | author |
| `~RP` | replies |
| `~SC` | score |
| `~NW` | new |
| `~DS` | day |
| `~DT` | date |
| `~TI` | time |

### The thread tree

`v` in the thread list, or in the reader, shows a thread as a tree:
a line a message, in reading order, each reply under the one it
answers and each message's comments under it, so you can see who
answered whom. (Since `r` is a comment, most threads are one post and
its comments.)

```
  1    +2   10/19  sysop                    new board is up
  2         10/24  ├─ansifan                works for me, nice board
  3         10/24  │ └─sysop                thanks! try the art gallery too
  4         10/24 *│   └─zinekid            gallery
  5         10/24 *└─zinekid                first time here, hello all
  6         10/24 *  └─sysop                welcome aboard, grab a door game
                10/24      └─ansifan            seconded
```

A comment's row has no number or score, and shows what it says.

The last column is a reply's subject where it changes, and its first
words where it's still "Re:" the same thing. From the reader, the bar
starts on the message you were reading; from the list, on the first
new one.

| Key | Does |
|---|---|
| Enter | read on from the message under the bar (on a comment, the message it's under) |
| `r` | comment on it (in email and echomail, reply) |
| a number | move to that message |
| Esc | back |
| `q` | out of the boards |

The MTREE screens (TOP/MID/HIL/BOT) draw it; without them it's a plain
list. Their markers:

| Marker | Is |
|---|---|
| `~MN` | number |
| `~MK` | `*` if new |
| `~TR` | the tree's lines in front of the message (`├─`, `└─`, `│ `) |
| `~SN` | author |
| `~TS` | the tree's lines and the author together (fits a fixed column) |
| `~RT` | who it answers |
| `~MT` | subject (`[filled] ` in front of a filled request) |
| `~FL` | the first message's `open` or `filled`, on a request board |
| `~BL` | its first words |
| `~TX` | the subject where it changes, else the first words |
| `~SC` | score |
| `~DS` | day (mm/dd) |
| `~DT`, `~TI` | date and time |

Only the innermost ten levels of a very deep thread are drawn.

### Request boards

A request board is a message area with "Requests" set in `*A` (item 6).
Anyone who may post there asks for something (a file, a door, a text) by
starting a thread. Anyone else answers it, usually with a comment that
[links to the file](#file-links) on the board. The asker says when it's
been filled, and the one who filled it gets [points](#points-for-fills).

- **Answering.** `r` in the reader leaves a comment under the request;
  `/l` as the comment picks a file to link to. A longer answer can be
  posted with a link from the editor's `/L`.
- **Marking.** The asker presses `f` on the thread in the list, or `m`
  in the reader. A lightbar lists the answers (comments and replies,
  with how many files each links to); the asker picks the one that
  filled it, or "none of these". In the reader, on someone's reply, it
  first asks "Did ...'s reply fill it?". The answer's links become the
  note; with none, it asks for one.
- **Who may mark.** Only the asker: whoever answers doesn't get to say
  it's done. Co-SysOps may too, for a request left behind. Pressing it
  again on a filled request asks whether to mark it open again.
- **The list.** Open requests come first (after pinned threads), then
  filled ones, which have `[filled]` in front of their title. `$` shows
  the [top fillers](#points-for-fills).
- **The reader** shows "Filled 10/24/26 by ansifan:
  file:DOORS/lord407p.zip" above the request.
- **Telling people.** If a co-SysOp marked it, the asker's next menu
  says so in its help line, once. The filler's next menu says "Your
  reply filled ...: +10 points", once.
- **Bots ask nothing.** A bot (flag [`B`](#bots)) can answer on a
  request board, but can't start a request (`ReqBotNoAsk`).

The marks are in the `filled` table, by the request's row id; packing
the request away takes its mark with it.

#### Points for fills

Topsite-style credit for filling requests, without the drama:

- The filler gets `fill_points` (`[messages]`, "Points per fill" in
  `*C`, Caller Boards; 10 to start, 0 for none). On a board that uses
  file points, they're added to the filler's file points too, to spend
  on downloads.
- Points are only ever given. Reopening a request doesn't take them
  back, and marking it filled again doesn't pay twice.
- Nobody earns points from their own request, or for marking one
  themselves. Bots earn none, and aren't ranked.
- `MF` (the stock message menu's `T`, or `$` on a request board) shows
  the top ten fillers this week, in the last 30 days and of all time,
  side by side, and your own total. A FILLERS screen heads it if the
  board has one.
- A caller's profile shows their fills (`~FI` on FINGER).

The ledger is the `fills` table. It outlasts the requests, so packing
doesn't change anyone's standing.

### Pinned threads

The moderator pins a thread from `o` (PTT's marked posts). Pinned
threads stay at the top of the list whatever the sort, with `pin`
(`PinLabel`) where the number goes (in `~MN` too).

### Tags

A board's `tags` are set in `*A` ("Post tags") or with its moderator's
`o` (PTT's categories). They're offered when a thread is started
there, and the one picked heads the title: `[question] How do I...`.
`t` in the thread list filters by them.

### The reader

The reader keeps the header (MSGHEAD) at the top and scrolls the text
under it. The keys are on the bottom line, or, if the board has a
MSGRBAR screen, in its prompt bar: `~HP` takes the keys and `~PC` the
cursor, as the lists' prompt bars do, and the screen's lines (up to
four) sit at the bottom. A reply starts with the
message it answers: "in reply to #1 by phrac: ..." (`MsgReaderReplyTo`,
its number, author and first words), so it reads on its own.

| Key | Does |
|---|---|
| Left, Right | previous, next message |
| Up, Down | scroll |
| `r` | comment ([below](#votes-and-comments)); in email and echomail, reply |
| `p` | post |
| `x` | vote or comment |
| `g` | the attached file and the files the message [links to](#file-links) |
| `t` | the whole thread; reading it already, its [tree](#the-thread-tree) |
| `v` | the [thread tree](#the-thread-tree) |
| `e` | [edit](#editing-your-posts): add to your own post (a moderator rewrites one) |
| `d` | your own post: [take it back, retract or burn it](#taking-it-back); as the moderator, delete |
| `u` | up to the message this one answers |
| `m` | on a [request board](#request-boards): mark the request filled, or open again |
| `f` | finger the author |
| `l` or Esc | back |
| `q` | stop |
| `?` or `h` | help |
| a number | go to that message |

### Votes and comments

Votes and comments are PTT's pushes. They're how you answer a post:
`r` in the reader goes straight to a comment, so there aren't two ways
to reply. (Email and echomail are the exceptions: there `r` writes a
real reply, since comments don't leave this board.) `x`, then one of
these, leaves a line of up to 50 characters under the message, with
your handle and the time:

| Key | Leaves |
|---|---|
| `+` | an up vote (words optional) |
| `-` | a down vote (words optional) |
| `>` | a comment (words required) |
| `d` | (the area's sponsor and co-SysOps) takes a caller's pushes off |

Rules:

- Each caller has one vote on a message. Voting again replaces it, line
  and all.
- You can't vote on your own message.
- Comments may repeat, one push every five seconds.
- Pushes are only allowed where you may post, and not on private mail.

The score is ups less downs. It shows as `+4`, as `X2` when negative,
and as `HOT` from 25.

- The classic reader shows pushes too.
- Pushes stay on this board: FTN and QWK don't carry them.
- Packing a message takes its pushes with it.
- Karma, on a caller's profile, is the votes on their public posts (not
  anonymous ones).

### File links

A message or comment can link to a file on the board. A link is written
`file:AREA/NAME`, with the file area's storage key and the file's name:
`file:DOORS/lord407p.zip`. It reads plainly in QWK readers and on FTN
too, and anyone can type one by hand.

- **Writing one.** `/L` in the full-screen editor, or `/l` as a
  comment, asks for a name (a part of one, or wildcards), lists the
  matching files in the areas you can see in a lightbar, and puts the
  link in for the one you pick.
- **Following one.** The reader's bottom line says `g files (n)` when a
  message or its comments link to files that are here. `g` lists them
  in a lightbar (with the attached file, if any); Enter shows one's
  details and offers it for download, with the usual checks.
- Links to files that aren't here, or are in areas you can't see,
  aren't listed.

The lightbars use MFILES screens (TOP/MID/HIL/BOT) if the board has
them: `~FN` name, `~SZ` size, `~AT` area, `~DE` description.

### Threads and what's read

A message keeps the row id of the one it answers (`reply_to`), so
threads survive packing. QWK replies join their thread by the header's
reference number, and packets carry it, so offline readers thread too.

Each area has a last-read pointer: everything up to it is read, and QWK
and the classic reader use it. Messages read beyond it are remembered
too, and the pointer moves up once everything before them is read.
Your own posts count as read.

### New posts at logon

`MW` (in STARTUP, after the mandatory scan) says how many new posts
there are and offers to read them, a thread at a time. `MW Read` reads
them without asking.

The website feed lists the newest public posts (`new_posts`).

### @mentions

`@handle` in a public post tells that caller. It works in posts typed
here, sent by QWK or from MeshCore, but not in quoted lines, and not
for yourself.

- If they're on, they get a node message at once ("mentioned you in
  General: subject", `MentionNotice`).
- At their next logon, `MW` first offers the posts that mention them.
- `M@` reads the unseen ones any time; `M@ All` reads the last hundred.

Handles with spaces work (`@Dark Angel`); punctuation ends a name. Only
callers who could read the area are told, and `@5` isn't user number 5.

### The message base on screens

Screens can show what's in the message base, with `%MG`...`%MJ` (see
[CODES.md](CODES.md)):

- the newest post
- the most replied to
- the loudest area of the month
- the top poster
- a message at random
- what's new for the caller

`MX` jumps there, so lightbar options on a menu can go straight to
them:

| Command | Opens |
|---|---|
| `MX newest`, `MX hot`, `MX dig`, `MX mouth` | that message, in its thread |
| `MX loud` | that area's threads |
| `MX unread` | everything new, to read |

## MeshCore

A [MeshCore](https://meshcore.co.uk) companion radio's channels can be
bridged to message areas (`meshcore.rs`).

- What's said on a channel is posted in its area, from the sender's
  name.
- What a caller posts there (in the editor or by QWK) goes out on the
  channel as `handle: text`, without the signature or quoted lines.
- A long post is split into at most `max_parts` messages of the radio's
  160 bytes.
- Private mail stays on the board.

### Setting it up

    [meshcore]
    enabled = true
    address = "192.168.1.50:5000"   # WiFi companion firmware; or ser2net

    [[meshcore.channel]]
    channel = "Public"              # the channel's name on the radio
    area = "MESH"                   # the message area's storage key
    # from_mesh / to_mesh: one way only, if wanted

The board speaks MeshCore's companion protocol over TCP: the WiFi
companion firmware's port 5000, or a USB radio put on the network with
ser2net. It reconnects when the link drops, and picks up changed
settings within a minute.

`*C`, MeshCore, has the switch, the address, the reconnect delay and
the parts. Channels are listed in the file.

`tools/fake_meshcore.py` is a pretend radio for trying it out.

## FTN echomail

The board can be a node of FidoNet-style networks such as fsxNet, with
its own mailer and tosser (the original handed packets to outside
programs). It polls its hub over binkp (`binkp.rs`, FTS-1026), sends
what's been posted here, and tosses what came (`ftntoss.rs`). The
session password goes as CRAM-MD5 when the hub offers it.

### Setting it up

`[ftn]` in `fluxbbs.toml`, or "FTN Echomail" in `*C`:

    [ftn]
    enabled = true
    address = "21:1/999@fsxnet"
    hub = "21:1/100"
    hub_host = "agency.bbs.nz:24556"
    session_password = "..."
    packet_password = "..."
    poll_minutes = 60

    [[ftn.area]]
    tag = "FSX_GEN"
    area = "FSXGEN"

| Setting | Is |
|---|---|
| `address` | the node's address, with its domain (`21:1/999@fsxnet`) |
| `hub`, `hub_host` | the hub's address, and its binkp host |
| `session_password`, `packet_password`, `areafix_password` | the passwords |
| `origin` | the origin line |
| `poll_minutes` | how often to poll |
| `listen` | where to answer the hub's calls |
| `name` | the first network's name (else its address's domain) |

Echoes are message areas: each `[[ftn.area]]` maps a network tag to an
area's storage key. Renaming an area's key in `*A` moves its mapping
with it.

| Command | Does |
|---|---|
| `fluxbbs ftn poll` | poll the hub now |
| `fluxbbs ftn toss` | toss what came in |
| `fluxbbs ftn scan` | scan for posts to send |
| `fluxbbs ftn areafix +FSX_GEN -FSX_X` | ask the hub's AreaFix to link and unlink echoes (or `%LIST`) |

### Mail coming in

Messages come in with their kludges read:

| Kludge | Used for |
|---|---|
| MSGID | duplicates (or who, when and subject for a message without one) |
| REPLY | threads, even when the reply comes first |
| CHRS | UTF-8, Latin-1, CP1252 or CP437, for the names and subject too |
| TZUTC | times shown in the board's zone |

- Long lines are wrapped.
- The sender's address (from the origin line) becomes their note.
- The tear and origin lines count as the signature, so quotes leave
  them out.
- Escape sequences from the network are cut down to colors and
  cursor-right (ANSI art's spacing). Nothing from elsewhere can clear a
  reader's screen or talk to their terminal.
- @mentions of local callers work.
- Dupes are remembered after their messages are packed away.

#### Which packets are taken

- ZIP bundles and plain packets are both read. A ZIP without packets (a
  nodelist, say) is left alone.
- Only packets from the hub, to this node, with the packet password are
  taken. Only the password's first 8 characters count, as that's all
  the header holds.
- If the board's side fails (the database busy), the packet stays in
  inbound for the next toss.

#### The bad directory

Anything that can't be tossed goes to `data/ftn/bad`, under a new name
if one's taken. Move it back to `inbound` to try again. So does:

- a packet with mail for an echo that isn't mapped to an area
- a packet with netmail for another address
- a bundle that isn't a ZIP (ARC, ARJ)

So nothing is lost while it's seen to. What was tossed from such a
packet comes in again as dupes. The other packets in a bundle are
tossed all the same.

### Mail going out

What callers post in echo areas goes out at the next poll. It carries
MSGID, REPLY (a reply to a network message threads there too), PID,
TZUTC and CHRS, a tear and origin line, SEEN-BY and PATH. Color codes
are taken out.

- An area starts from what's in it when it's first scanned, so old
  posts aren't sent.
- Posts that came from the network (or from MeshCore) aren't sent back.
- A post is marked sent only once its packet is written. One packed
  again after a failed write keeps its MSGID, so the network can tell a
  resend.
- A line of text starting `SEEN-BY:` goes out as `SEEN+BY:`, and
  control characters are left out, so nothing typed can pass for a
  control line. Coming in, only the SEEN-BY lines at the end of a
  message count.

### Netmail and AreaFix

Netmail for the node goes to the named caller's email (else the
SysOp's), from `name@address`. `fluxbbs ftn areafix` talks to the
hub's AreaFix (see the commands above).

### The hub calling in

The hub can call the board too, to deliver mail as it arrives ("crash"
rather than "hold"). Set `[ftn] listen` (`0.0.0.0:24554`, published by
the Docker compose file) and the board answers binkp.

- It offers CRAM-MD5 and lets in only its hub, with the session
  password.
- A wrong password counts as a failed login (security log, bans).
- Connections are held to the same per-address limits as callers.
- A hub calling with the wrong password is turned away and logged, but
  not banned, so a password changed at its end can't cut the board off.

### Sessions and limits

- One session runs at a time, and one toss or scan, across processes
  too. Lock files in `data/ftn` make `fluxbbs ftn` safe to run beside
  the running board. A call during a poll is told to try again.
- A hub that has part of a file from a broken session can ask for the
  rest (M_GET).
- A poll the hub answers as non-secure (it doesn't have the session
  password yet) sends nothing, so mail isn't left in its unsecure
  inbound. The log says why.
- Files over 256 MB are skipped, and names are made safe.
- A session quiet both ways for five minutes is ended.

### More than one network

`[ftn]`'s address, hub and passwords are the first network's. It's
named by `[ftn] name`, else by its address's domain (`fsxnet`). Each
`[[ftn.network]]` adds another network, with its own `name`,
`address`, `hub`, `hub_host`, passwords and (optionally) `origin`.

An echo says which network it's on with `network`; with none, it's on
the first.

    [[ftn.network]]
    name = "agoranet"
    address = "46:1/200@agoranet"
    hub = "46:1/100"
    hub_host = "binkp.pharcyde.org"
    session_password = "..."

    [[ftn.area]]
    tag = "AGN_GEN"
    area = "AGNGEN"
    network = "agoranet"

#### Polling

- Every network polls its own hub, one session each at a time, with
  its own outbound in `data/ftn/outbound/NAME`.
- A network with no `hub_host` isn't polled (its hub calls in).
- `fluxbbs ftn poll agoranet` polls one network.
- `fluxbbs ftn areafix agoranet +AGN_GEN` sends to a given network's
  AreaFix. Without a name, it goes to the network of the first tag's
  echo.
- The first network also sends what's left in `data/ftn/outbound` from
  before there were several.

#### Hubs calling in

- A hub calling in is shown all the board's addresses. It's taken as
  the network its own address is on, with that network's password, and
  gets only that network's mail.
- A system that's the hub of several networks gets each one whose
  password it gave.

#### Keeping networks apart

- What a hub sends goes to `data/ftn/inbound/NAME`. It's tossed only
  as mail of the network(s) its session proved it was, so one
  network's hub can't post into another's echoes.
- Packets put in `inbound` itself by hand (from `bad`, say) are taken
  as whichever network they're addressed on. A zoneless one that could
  be on two goes to bad.

#### Names and errors

- A name may only have letters, digits, `-` and `_`.
- A network with a bad or repeated name or address is left out.
- That, and echoes on no network (or on two), are logged at each poll,
  without stopping the other networks.

`*C` edits the first network; the others are edited in the file.

### Not yet

Callers writing netmail to an FTN address, and file echoes (TIC).

## From 2.0 and 2.20

Iniquity 2.0 (IDT, 1998) and jack phlash's 2.20A (2001) came after the
release flux/bbs ports. Their source isn't here. These features follow
2.0's manual, WHATSNEW.IQ2, menus and screens, and 2.20's string table
and notes.

### Voting booth

The stock VOTING menu is `V` on the main menu. Questions can let
callers add answers.

| Command | Does |
|---|---|
| `VV` | vote on what's new |
| `VA` | vote on one question |
| `VR` | results |
| `VN` | add a question |
| `VD` | delete a question |

| Screens | Markers |
|---|---|
| VQTOP/MID/BOT | `~QN` `~QD` |
| VATOP/MID/BOT | `~VQ`, `~AN` `~AD` |
| VRTOP/MID/BOT | `~VQ`, `~AP` `~AD` `~VN` `~VR` |

Without the screens, the booth shows plain lists with a bar per answer.

### Newscan selections

`ME` and `FE` (`E` on the message and file menus) turn areas in and out
of newscans with a bar list. The MSCAN and FSCAN screens draw it (`~AN`
`~AT` `~SC`).

These leave the other areas out: "all areas" scans, reading everything
new, the logon new-posts check, and QWK packets. `US` puts every area
back for everyone.

### Message search

`MO` (`B` on the message menu) searches:

- all areas or one
- from, to, subject, all text, or your own
- new or all messages

Matches open in the reader. 2.0's MSGSRCH screen is shown first, if
it's there.

### Multinode chat

`XC` (`C` on the multinode menu). On the way in it says who's in chat
and how it works (`MNChint`); `/help` lists the commands. Without a
NODECHAT screen the bottom line shows them (`MNCbar`); with one, the
screen should (its markers are in [ARTWORK](ARTWORK.md#chat)).

| Type | Does |
|---|---|
| `/who` | who's on, and what they're doing |
| `/me` | an action |
| `/msg user text` | a message to someone in chat |
| `/page user text` | a page to anyone on |
| `/help` (or `/?`) | the commands |
| Enter alone | who's in chat |
| `/quit` or Esc | leave |

### Invisible

`XI` after `>L` in the matrix is a hidden logon. It asks
`LogonInvisible` unless given `Y`. An invisible caller is out of the
callers list and other callers' who's-online, node messages and the
website feed.

Elsewhere, `XI` hides or shows the caller (`I` on the multinode menu).
SysOps still see everyone.

### Smaller things

- ACS `n#`: on node #.
- A door's name in the caller's status: the part after `;` in its
  command (`LORD.BAT;Legend of the Red Dragon`), or the menu item's
  description. The website feed says "in LORD".
- Tab completes a handle at "To:", and in `?I` with `/AC` (and
  `bbs.input{complete = true}`).
- `|ZA`...`|ZB` shows text as it is.
- 2.20's `%DN` (new users today) and `%TB` (time in the bank).
- 2.20's `|PO`, `|PE` and `|PD` are accepted; screens don't pause here.
- "Set newscan date" starts at the date the pointer is at.
- File lists for the web: `/files.txt` and `/files.html` (2.0's
  `fltxt` and `flhtml`), with only the areas a new caller could open.
- A cap on imported description lines (`*C`, Files: DIZ lines).
- `bbs.inmenu(keys)` for scripts (IPL's `isincurmenu`). The rest of
  2.0's new IPL functions have Lua equivalents (see
  [SCRIPTING.md](SCRIPTING.md)).
- `D7` runs a door with DOOR.SYS, read back afterwards (see
  [Doors](#doors)).

### Typed text and codes

2.0 fixed a bug where "'|XX' would kill multinode chat". Its equivalent
here is fixed too:

- What callers type (chat, oneliners, handles, file descriptions,
  message headers, BBS list entries...) keeps its color codes and
  nothing else.
- Typed into `?I` or `bbs.input`, other codes are wrapped in
  `|ZA`...`|ZB`, so they show but don't act where the text ends up (the
  stock oneliners append to a screen).
- Handles with codes in them are refused.

Caller numbers no longer repeat when two nodes log on at once (2.0's
"caller list numbering with multinode").

### Left out

- the modem and NetModem/32 items
- MicroDOS
- WFC changes
- 8+ COM ports
- phone formats
- the SETUP.SR, CALINFO.BBS and SFDOORS.DAT drop files (no door here
  needs them yet)
- the local-console keys alt-L and alt-V (alt-D, down a level, is `<`
  in the user editor)
- 2.20's paging of long screens

## Not planned

- Tetris (`TP`, `TS`): the game's source isn't in this release;
  `tetris.pas` is a misnamed terminal unit.
- Modem setup (`*M`).
- MicroDOS (`*D`, a shell on the host).
- The local console's status bar (STATBAR).
- The Oblivion and Renegade converters (OBV2INIQ, REN2INIQ).
