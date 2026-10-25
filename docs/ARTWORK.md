# Artwork

A guide for an artist replacing the board's screens: what screens there
are, what markers each needs, and how to try them out. The display
codes themselves are in [CODES.md](CODES.md).

## The basics

- Screens live in `text/` as `NAME.ANS`.
- Replace a file and the board uses it on its next display. No restart.
- Copy the stock file and draw around its codes and markers. That's the
  easiest way to keep everything working.
- Tick each screen off in the [checklist](#checklist) as it's done.

### Keeping your art out of the repo

Run the board from its own directory, with `stock_text` pointing at the
repo's screens (see [SYSOP.md](SYSOP.md#running-a-board-outside-the-repo)).
Put your screens in the board's `text/`. Anything it doesn't have comes
from the stock screens.

### Where a screen is found

The board looks in these places, in order, and takes the first match:

1. the caller's theme (`themes/NAME/`), if they have one;
2. the board's `text/`;
3. `[paths] stock_text`;
4. `[paths] stock_theme`.

In each place it looks for `NAME.ANS` (and its variants, below), then
`NAME.ASC`. Screens made with modern tools (UTF-8, LF line ends) work
too.

### Variants

| File | Shown |
|---|---|
| `NAME.AN1` to `NAME.AN9` | at random: each time, one of them or `NAME.ANS` is picked (a main menu with a different line each time, say) |
| `NAME.A0S` to `NAME.A6S` | on that weekday (0 is Sunday), in place of `NAME.ANS` |

Variants need a `NAME.ANS` beside them.

## Size, color and clearing

- **Size:** 80×25. Stay within 79 columns, or end lines exactly at 80.
- **Menus:** the prompt takes the bottom row or two, so keep menu art to
  about 23 rows. (MAIN is 18 rows of logo plus 5 of commands.)
- **Clear screen:** stock menu screens start with a clear-screen code
  (`ESC[2J`). Keep one at the start of full-screen art, or it draws over
  whatever was there.
- **Color:** 24-bit color works (like MAIN) in SyncTERM and modern
  terminals. Classic 16-color art with iCE colors is the safest choice
  for old clients.
- **Codes:** colors, pauses, clear-screen and values such as the
  caller's handle can be typed into a screen as text. See
  [CODES.md](CODES.md).

## Lightbar menus

Menus such as MATRIX and GOODBYE draw their options themselves, on top
of your art. A menu is a lightbar when it's set as one and its screen
exists.

### Markers

| Marker | Marks |
|---|---|
| `~01`, `~02`... | where each option is drawn highlighted, in command order |
| `^01`, `^02`... | where each option is drawn plain (without one, the `~NN` spot) |
| `~HP` | the help line: the highlighted option's help (optional) |
| `~PC` | where the cursor waits, such as a prompt bar's slot (optional, new) |
| `~30` | where the cursor goes after a pick (else the bottom row) |

- Write them as pairs: `~01^01`, `~02^02`...
- A lightbar holds up to 29 options.
- Options the caller can't use (by access) and hidden ones are skipped,
  so the next option takes their number.
- A marker takes the color in effect where it sits.

### What the options look like

The options' text and colors are in the menu (`menus/MATRIX.toml`):
`on` is the highlighted text, `off` the plain text (each falls back to
the description), and `help` goes at `~HP`. To restyle them, put a copy
of the menu in the board's `menus/` (with `stock_menus` set; see
[SYSOP.md](SYSOP.md)) and change that.

### Keys

- The arrows move by where the options are on screen: left and right
  along a row, up and down to the next row. So a grid works.
- A command's own key still runs it (hot keys, `/X`, digits for `#`).
- Space steps to the next option; Home/PgUp the first, End/PgDn the
  last; Enter picks.
- Without a `#` command, the number pad's digits act as arrows (`8`
  `2` `4` `6`), first (`7` `9`) and last (`1` `3`).

### Animation (new)

A lightbar menu can animate its screen while it waits for a key. List
the effects in the menu, each over a band of screen rows:

```toml
[[effect]]
kind = "glint"     # a band of light sweeps across the art
rows = [1, 9]
every = 10.0       # seconds from one sweep to the next
delay = 2.0        # seconds before the first

[[effect]]
kind = "ripple"    # a pulse runs along the box-drawing lines
rows = [10, 21]
every = 10.0
delay = 6.0
```

- **The effects work on what's on screen**, cell by cell. That means
  any art, codes and all, and whichever random variant was drawn.
- **Only the cells that change are sent**, a few kilobytes a second.
- **A glint lights the art in its rows**: everything but blank space.
  **A ripple lights only the box-drawing lines** (`─│┌┐└┘├┤┬┴┼` and the
  double ones). Neither touches the menu's text or the lightbar.
- **When it runs:** only on screens that clear first, for callers with
  24-bit or 256 colors (SyncTERM, IcyTerm, NetRunner, or UTF-8) who
  haven't turned animations off in their config (`v` on disflux's,
  field `W`). Never in plain text.
- **The screen saver.** After `screensaver_seconds` idle on a menu (60
  to start; `*C`, SSH, Telnet and Terminal; 0 for none), digital rain
  in pinks and magentas falls over the screen. The board's name shows
  through across the middle as drops pass over it. Enter or Esc
  (`ScreenSaverExit` says so at the bottom) brings the menu back. Like
  the effects, it runs on any lightbar menu that clears the screen,
  only for callers whose menus animate.
- **Any screen can animate too**, while it waits at a `|PK`. Put its
  effects in a file beside it with the same name and `.FX` (say
  `PRELOGON.FX`), as `[[effect]]` entries like these.
- **More kinds, for scenes:**
  - `flicker`: firelight; cells brighten and dim, warmer as they
    brighten (`amount` 0.3 to start).
  - `pulse`: cells of one exact color (`matches = "#600404"`) glow
    toward another (`tint`) and back, every `every` seconds. Use it for
    eyes, gems or windows, drawn in a color nothing else on the screen
    uses.
  - `dust`: motes drift down through the area, catching the light.
  - `type`: the text in the area types itself out again every `every`
    seconds, starting `delay` seconds into each round. `amount` sets
    the speed: 1 is a hundred characters a second, 0.1 ten (12 to
    start). With `tint`, a block cursor in that color runs ahead of
    the text, and blinks at the end until the next `type` begins. Give
    each line its own `type` with the same `every` and delays one
    after another, and a screen can dial in, print a banner and wait
    at a login prompt.
  - `blink`: cells of one exact color (`matches`) go out and come back
    on, each on its own, a change every `every` seconds, on `amount` of
    the time (half to start). Modem lights (`every = 0.09`), windows
    across a city (`every = 3.5`, `amount = 0.8`), a failing neon
    letter.
  - `roll`: a bright bar rolls down the area once every `every`
    seconds, as on an old tube.
  - `decode`: the letters in the area scramble, matrix style, and
    settle into the next of `messages = ["...", ...]`, every `every`
    seconds, `delay` in; the scramble lasts `amount` seconds (2 to
    start). Each round picks fresh cells for the message's letters,
    in reading order, and fills the rest with noise: the letters in
    `tint` (or the brightest color the area was drawn in), the noise
    in the darkest. A message as long as the area (a word down a
    margin) keeps each cell's own color. Give it the glint's `every`
    and `delay` and the message changes as the glint passes.
- **Effects can share cells.** Each applies to what the one before it
  made, in the order they're listed: put a `roll` before the `type`s
  on the same tube.
- **`cols = [first, last]`** limits any effect to those columns, as
  `rows` limits it to those rows.
- **It stops for the board's own messages.** A node message, a page or
  the inactivity timeout takes over as usual, and the timeout still
  counts from the last key.

## List screens: TOP, MID, HIL, BOT

Lists are drawn from a set of screens that share a name:

| Screen | Is |
|---|---|
| `NAMETOP` | the frame above the list |
| `NAMEMID` | one row, drawn once per item |
| `NAMEHIL` | the highlighted row, in lists with a moving bar |
| `NAMEBOT` | the frame below the list |

- TOP, MID and BOT must all be there; otherwise the board draws a plain
  list.
- Without HIL, the bar row is drawn with MID.
- `~TL` in TOP, if there, is where the rows start.
- MID and HIL hold markers such as `~AT` (area title) or `~NF` (file
  count), filled in with live data, and can use `%XX` values too. Keep
  the markers and the columns they sit in.

### Prompt-bar BOT screens

A list's BOT screen can be a prompt bar: put `~PC` in it.

| Marker | Marks |
|---|---|
| `~HP` | where the list's keys (the help line) go, kept to that row |
| `~PC` | the slot where what's typed shows (its last 6 characters) |

The BOT screen is then the screen's last rows, and holds the help and
prompt itself (e.g. `MHUBBOT`).

## What a caller sees, in order

1. **BOTCHECK**: the bot check, before anything else.
2. **PRELOGON**: right after connecting, once the "detecting emulation"
   lines have printed. It's the first impression and has the whole
   80×25 screen. End it with a pause (stock: "press enter to
   continue").
3. **MATRIX**: the pre-login menu (logon / apply / feedback / page).
4. **WELCOME**: right after login.
5. The logon sequence: oneliners, last callers, automessage, your stats.
6. **MAIN**: the main menu.

## Checklist

### First impression (every caller sees these)

- [ ] `BOTCHECK`: the bot check (markers [below](#botcheck))
- [x] `PRELOGON`: splash screen before login
- [x] `MATRIX`: pre-login menu (lightbar)
- [x] `WELCOME`: after login
- [x] `MAIN`: main menu (graffiti logo with built-in command list)
- [ ] `MENUPMT1`: the command prompt under most menus (`MENUPMT2` and
      `MENUPMT3` on a few). A one-liner, but always on screen
- [ ] `LOGOFF`: logging off
- [ ] `GOODBYE`: goodbye menu (lightbar)

### File areas

- [x] `FILE`: file menu
- [x] `LISTFILE`: file lister frame (header and command bar; markers
      [below](#listfile-and-gallery))
- [x] `GALLERY`: the art gallery (optional; see [below](#listfile-and-gallery))
- [x] `FAREATOP` / `FAREAMID` / `FAREAHIL` / `FAREABOT`: file area list
- [ ] `FCONFTOP` / `FCONFMID` / `FCONFBOT`: file conference list
- [ ] `FILEINFO`: single-file details
- [ ] `FILETEST`: the upload test (`~L0`-`~L9` lights, `~PI`; `~FN`
      name, `~FS` size)
- [ ] `PROTLTOP` / `PROTLMID` / `PROTLBOT`: transfer protocol picker
- [ ] `LISTHELP`: help inside the lister
- [ ] `FSCANTOP` / `FSCANMID` / `FSCANHIL` / `FSCANBOT`: file newscan
      selection (optional; `~AN` `~AT` `~SC`)

### Messages

- [x] `MESSAGE`: message menu
- [x] `EMAIL`: email menu
- [ ] `SCANMSG`: newscan menu
- [ ] `MSGHEAD`: header above each message being read (`~RT`: what the
      message answers, "#12 by phrac")
- [ ] `MAILHEAD`: header above each email being read
- [x] `MAREATOP` / `MAREAMID` / `MAREAHIL` / `MAREABOT`: message area list
- [ ] `MCONFTOP` / `MCONFMID` / `MCONFBOT`: message conference list
- [x] `MSGLTOP` / `MSGLMID` / `MSGLHIL` / `MSGLBOT`: message listing (a
      page is as many rows as fit under `MSGLTOP`, so a tall header is
      fine)
- [x] `MHUBTOP` / `MHUBMID` / `MHUBHIL` / `MHUBBOT`: the message
      center's board list ([markers](#message-center))
- [x] `MTHRDTOP` / `MTHRDMID` / `MTHRDHIL` / `MTHRDBOT`: a board's
      threads ([markers](#message-center))
- [ ] `MTREETOP` / `MTREEMID` / `MTREEHIL` / `MTREEBOT`: a thread as a
      tree ([markers](#message-center))
- [ ] `MFILESTOP` / `MFILESMID` / `MFILESHIL` / `MFILESBOT`: a message's
      linked files, and the file picker for links (`~FN` name, `~SZ`
      size, `~AT` area, `~DE` description)
- [ ] `MFILLTOP` / `MFILLMID` / `MFILLHIL` / `MFILLBOT`: the answers to
      a request, to pick the one that filled it (`~SN` who, `~DS` day,
      `~TX` what)
- [ ] `FILLERS`: heads the top fillers (`MF`)
- [ ] `MSGRBAR`: the message reader's bottom, a prompt bar (`~HP` the
      keys, `~PC` the cursor; its last line is the screen's last row)
- [ ] `TAROT`: the tarot menu (card of the day, three cards, a fortune);
      `%TR` and `%TF` are the readings and fortunes left today
- [ ] `BBSLINK`: the BBSLink games menu
- [ ] `TAROTHDR`: heads each reading and fortune (the cards and the
      reading take 15 rows under it, so keep it to a line or two)
- [ ] `MHOTTOP` / `MHOTMID` / `MHOTHIL` / `MHOTBOT`: the Hot list
      ([markers](#message-center))
- [ ] `MSGRHELP`: the message reader's help (otherwise the
      `MsgReaderHelp1`... strings)
- [ ] `MSGGUIDE`: the message center's guide (`h` in the boards;
      otherwise the reader's help)
- [ ] `MSGSRCH`: shown before a message search (optional)
- [ ] `MSCANTOP` / `MSCANMID` / `MSCANHIL` / `MSCANBOT`: message
      newscan selection (optional; `~AN` `~AT` `~SC`)
- [ ] `MAILTOP` / `MAILMID` / `MAILBOT`: mailbox listing
- [ ] `FSEDITOR`: full-screen editor frame
- [ ] `READHELP` / `FSHELP` / `EMAILHLP`: help screens
- [ ] `QWKMAIL`: QWK menu
- [ ] `QWKDL` / `QWKUL`: QWK download and upload

### Community

- [ ] `ONELNTOP` / `ONELNMID` / `ONELNBOT`: oneliners (every logon)
- [ ] `LASTCTOP` / `LASTCMID` / `LASTCBOT`: last callers (every logon)
- [ ] `AUTOTOP` / `AUTOBOT`: the automessage
- [x] `AUTOMSG`: automessage menu
- [ ] `USERSTAT`: your stats
- [ ] `USERCFG`: user configuration screen ([markers](#usercfg))
- [ ] `FINGER`: a caller's profile ([markers](#finger))
- [x] `AUTOSIG`: auto-signature menu
- [x] `BBSLIST`: BBS list menu
- [ ] `BBSTOP` / `BBSMID` / `BBSBOT`: BBS list ([markers](#bbs-list))
- [ ] `SHOWBBS`: one BBS's details ([markers](#bbs-list))
- [ ] `ADDBBS`: adding a BBS ([markers](#bbs-list))
- [ ] `NODECHAT`: multinode chat ([markers](#chat))
- [ ] `CHATMODE`: SysOp chat, split screen ([markers](#chat))
- [ ] `CHATHELP`: help in SysOp chat (`/?`)
- [x] `AICHAT`: the AI chat's header ([notes](#aichat))
- [ ] `VQTOP` / `VQMID` / `VQBOT`, `VATOP`..., `VRTOP`...: the voting
      booth (optional; [markers](#voting-booth))
- [ ] `DOOR`: doors menu
- [ ] `TIMEBANK`: time bank menu

### New users

- [ ] `APPLY`: start of the new-user application
- [ ] `NEWUSER`
- [ ] `NEWCFG`: checking a new user's entries afterwards
- [ ] `NUV`: new user voting
- [ ] `INFOTOP` / `INFOMID` / `INFOBOT`: infoforms list

### Shared

- [ ] `STOCK`: a header shared by many screens, drawn where a screen
      has `|SA`

The SysOp editors draw their own screens and need no art. `*F`'s area
list can use `CFGFAREATOP` / `MID` / `HIL` / `BOT` (optional); its rows
have `~AN` (number), `~AT` (name, indented under its parent), `~FK`
(storage key), `~NF` (files) and `~AC` (access: list / upload /
download).

### Not used

These stock screens are for features that aren't ported or are drawn
another way, so they're never shown: `TBNK`, `TETRIS`, `HIGH*`,
`USERL*` and `NODE*` (the user list `UL` and who's online `XL` are
drawn as text), `ARCH*`, `MICRODOS`, `HELP*`, `INIQHELP`.

## Markers by screen

### BOTCHECK

| Marker | Marks |
|---|---|
| `~BC` | the top left of the 40×9 field the characters are drawn in |
| `~BP` | where the answer is typed (the line above it takes "try again") |

### LISTFILE and GALLERY

The art gallery shows `GALLERY` if there is one, else `LISTFILE`; they
share markers, so copy LISTFILE and relabel the columns. Browsing the
file areas (`FW`, `L`) and archives always uses `LISTFILE`, so the two
can be drawn differently.

| Marker | Marks |
|---|---|
| `~TL` / `~BL` | the first and last list rows |
| `~FI` `~FN` `~FS` `~FP` `~FD` | the number, name, size, points and description columns |
| `~PI` | the info line |
| `~PM` | the lightbar command menu (in the gallery: open, play, speed, get, back) |
| `~C1`-`~C6` | its colors (each takes the color in effect where it sits) |
| `~S?` / `~E?` | the scroll bar's start and end; `?` is the character for its empty / full part |
| `~FA` | where you are |
| `~NA` `~NF` `~SZ` | the listing's files (packs count as files), folders and size (new) |

`~NA`, `~NF` and `~SZ` let a box beside a logo say what's in the
listing. The lister (`FL`) fills in `~NA` and `~SZ` too.

### Message center

| Screens | Markers |
|---|---|
| `MHUB` (the boards) | `~AN` number, `~AT` board, `~DE` what it's for, `~RD` reading now, `~NW` new, `~NM` messages, `~LP` / `~LD` the latest post's author and date, `~SP` sponsor (moderator) |
| `MTHRD` (a board's threads) | `~MN` number, `~MK` `*` if new, `~MT` subject (`[filled] ` in front of a filled request), `~FL` `open`/`filled` on a request board, `~SN` started by, `~RP` replies and comments, `~SC` score from votes, `~NW` new, `~DS` day (mm/dd), `~DT` / `~TI` latest post |
| `MTREE` (a thread's tree) | `~MN` number, `~MK` `*` if new, `~TR` the tree's lines, `~SN` author, `~TS` lines and author together, `~RT` who it answers, `~MT` subject, `~FL` as above, `~BL` first words, `~TX` the subject where it changes, else the first words, `~SC` score, `~DS` day, `~DT` / `~TI` date and time |
| `MHOT` (the Hot list) | `~AT` board, `~MK` `*` if new, `~MT` subject, `~SN` started by, `~SC` score, `~NW` new, `~DS` / `~DT` latest day and date |

### USERCFG

| Marker | Shows | Stock marker |
|---|---|---|
| `~P?` | how many SSH keys are on file | `~PK` (key `k`) |
| `~Q?` | the plan's lines | `~QP` |
| `~R?` | the time zone | `~RZ` |
| `~S?` | text mode: plain text or ANSI (`auto` follows flag B) | `~SB` |
| `~T?` | code's background (theme, gray, dim, blue, green, purple, none) | `~TG` |
| `~U?` / `~V?` | the code and Markdown color themes (nord, dracula, one dark, monokai, classic) | none |
| `~W?` | menus animating: on or off (new; plain-text key `v`) | none |

The second character is the field's key.

### FINGER

| Marker | Shows |
|---|---|
| `~UN` | handle |
| `~RN` | real name, if they show it |
| `~FI` | requests filled and points, if any |
| `~LO` | location |
| `~UO` | note |
| `~FC` | first call |
| `~LS` | last seen |
| `~CA` | calls |
| `~PO` | posts |
| `~UL` / `~DL` | uploads / downloads |
| `~LP` | latest post |
| `~LU` | latest uploads |
| `~KA` | karma |
| `~P1`-`~P6` | the plan |

### BBS list

The stock BBS list screens are relabeled for telnet and SSH.

| Screens | Markers |
|---|---|
| `BBSTOP` / `BBSMID` / `BBSBOT` | `~NM` `~BN` `~TN` (telnet) `~SH` (SSH, shortened to `:port` on the telnet host) `~SF` |
| `SHOWBBS` | `~TN` telnet, `~SS` SSH, `~IN` the description's first line and `~I2`-`~I9` its later lines (without them, those go under the screen) |
| `ADDBBS` | `~TN`, `~SS`: where each is typed |

Older screens' `~PN` shows the telnet address.

### Message editor

`FSEDITOR`, the full-screen editor's frame. Each value is written at
its marker, in the marker's color.

| Marker | Is |
|---|---|
| `~RN` / `~SN` | to / from |
| `~MT` | the title |
| `~DT` / `~TI` | date / time (24-hour) |
| `~AN` / `~AS` | anonymous (`Yes`/`No`) / autosig (`On`/`Off`) |
| `~TL` | the text's first row; its color is quoted lines' |
| `~BL` | the text's last row; its color is the typed text's |
| `~SL` | the row for prompts and hints (else the first text row) |
| `~CM` | where the command key is typed (else a prompt asks) |
| `~LN` / `~CN` / `~IN` | line, column, insert mode (`On`/`Off`) |

### Chat

| Screen | Markers |
|---|---|
| `NODECHAT` | `~TL` / `~BL` the conversation's top and bottom rows, `~SI` / `~EI` the input line's start and end; values `~ND` node, `~UN` handle |
| `CHATMODE` | `~ST` / `~SB` the SysOp's window, `~UT` / `~UB` the caller's (top left and bottom right); value `~ND` node |

Without the markers, the board draws a plain screen.

### Voting booth

From 2.0. Without these screens, the booth shows plain lists with a bar
per answer.

| Screens | Markers |
|---|---|
| `VQTOP` / `VQMID` / `VQBOT` (questions) | `~QN` `~QD` |
| `VATOP` / `VAMID` / `VABOT` (answering) | `~VQ`, `~AN` `~AD` |
| `VRTOP` / `VRMID` / `VRBOT` (results) | `~VQ`, `~AP` `~AD` `~VN` `~VR` |

### AICHAT

The AI chat's header. It stays at the top while the conversation
scrolls between it and the bar on the bottom row, so keep it to about
12 rows. Its colors are the `Ai*` strings (`*E`).

### Other screens with codes

`MAILHEAD`, `FILEINFO`, `USERSTAT`, `FSEDITOR`, `SHOWBBS` and `ADDBBS`
contain positioning or data codes too. Keep them, as with list screens.

## Trying a screen

- **Preview:** `python3 tools/ansi2png.py text/NAME.ANS out.png` renders
  a file roughly as a terminal would.
- **Live:** save it and call the board; it's used on the next display.
