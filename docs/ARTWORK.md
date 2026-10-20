# Custom artwork checklist

Screens live in `text/` as `NAME.ANS`. Replace a file and the BBS uses it on
the next display, with no restart needed. Tick each one off as it's done.
To keep your art out of the repo, run the board from its own directory
with `stock_text` pointing at the repo's screens (README, "Running a board
outside the repo") and put your screens in the board's `text/`.

Lightbar menus (MATRIX, GOODBYE) draw their options themselves: mark each
option's place with `~01^01`, `~02^02`... in command order (`~NN` is where
it's drawn highlighted, `^NN` where it's drawn plain), and optionally
`~HP` for the help line. The arrows move by where the options are on
screen (left and right along a row, up and down to the next row), and a
command's key still runs it. The options' own colours (`on` / `off`) and
help text are in the menu (`menus/MATRIX.toml`); to restyle them, put a
copy in the board's `menus/` (with `stock_menus` set, SYSOP.md) and
change that.

A screen can have up to nine random variants, `NAME.AN1` to `NAME.AN9`:
each time it's shown one of them or `NAME.ANS` is picked, e.g. a main
menu with a different line on it each time.

## What a caller sees, in order

1. **PRELOGON**: shown right after connecting, once the "detecting
   emulation" lines have printed. It's the first impression and has the
   whole 80×25 screen. End it with a pause (stock: "press enter to
   continue").
2. **MATRIX**: the pre-login menu (logon / apply / feedback / page).
3. **WELCOME**: right after login.
4. The logon sequence: oneliners, last callers, automessage, your stats.
5. **MAIN**: the main menu.

## First impression (every caller sees these)

- [x] `PRELOGON`: splash screen before login
- [x] `MATRIX`: pre-login menu
- [x] `WELCOME`: after login
- [x] `MAIN`: main menu (graffiti logo with built-in command list)
- [ ] `MENUPMT1`: the command prompt under most menus (`MENUPMT2` and
      `MENUPMT3` on a few). A one-liner, but always on screen
- [ ] `LOGOFF`: logging off
- [ ] `GOODBYE`: goodbye menu

## File areas (the core of this board)

- [x] `FILE`: file menu
- [x] `LISTFILE`: file lister frame (header and command bar)
- [x] `FAREATOP` / `FAREAMID` / `FAREAHIL` / `FAREABOT`: file area list
- [ ] `FCONFTOP` / `FCONFMID` / `FCONFBOT`: file conference list
- [ ] `FILEINFO`: single-file details
- [ ] `PROTLTOP` / `PROTLMID` / `PROTLBOT`: transfer protocol picker
- [ ] `LISTHELP`: help inside the lister
- [x] `GALLERY`: the art gallery (optional; without it the gallery uses
      `LISTFILE`, whose markers it shares, so copy that and relabel the
      columns). Browsing the file areas (`L`) and archives always use
      `LISTFILE`. Both can show `~NA` (files), `~NF` (folders) and `~SZ`
      (size) for the listing, e.g. in a box beside a logo

## Messaging

- [x] `MESSAGE`: message menu
- [x] `EMAIL`: email menu
- [ ] `SCANMSG`: newscan menu
- [ ] `MSGHEAD`: header above each message being read
- [ ] `MAILHEAD`: header above each email being read
- [x] `MAREATOP` / `MAREAMID` / `MAREAHIL` / `MAREABOT`: message area list
- [ ] `MCONFTOP` / `MCONFMID` / `MCONFBOT`: message conference list
- [x] `MSGLTOP` / `MSGLMID` / `MSGLHIL` / `MSGLBOT`: message listing (a
  page is as many rows as fit under `MSGLTOP`, so a tall header is fine)
- [x] `MHUBTOP` / `MHUBMID` / `MHUBHIL` / `MHUBBOT`: the message centre's
  area list (`~AN` number, `~AT` area, `~NW` new, `~NM` messages, `~LP` /
  `~LD` the latest post's author and date, `~SP` sponsor)
- [x] `MTHRDTOP` / `MTHRDMID` / `MTHRDHIL` / `MTHRDBOT`: an area's threads
  (`~MN` number, `~MT` subject, `~SN` started by, `~RP` replies, `~NW`
  new, `~DT` / `~TI` latest post)
- [ ] `MSGRHELP`: the message reader's help (otherwise `MsgReaderHelp1`..)
- [ ] `MAILTOP` / `MAILMID` / `MAILBOT`: mailbox listing
- [ ] `FSEDITOR`: full-screen editor frame
- [ ] `READHELP` / `FSHELP` / `EMAILHLP`: help screens

## Community extras

- [ ] `ONELNTOP` / `ONELNMID` / `ONELNBOT`: oneliners (every logon)
- [ ] `LASTCTOP` / `LASTCMID` / `LASTCBOT`: last callers (every logon)
- [ ] `AUTOTOP` / `AUTOBOT`: the automessage
- [x] `AUTOMSG`: automessage menu
- [ ] `USERSTAT`: your stats
- [ ] `USERCFG`: user configuration screen
- [x] `AUTOSIG`: auto-signature menu
- [x] `BBSLIST`: BBS list menu
- [ ] `BBSTOP` / `BBSMID` / `BBSBOT`: BBS list (`~NM` `~BN` `~TN` `~SH`
      `~SF`; the stock ones are relabelled for telnet/SSH)
- [ ] `SHOWBBS`: one BBS's details (`~TN` telnet, `~SS` SSH)
- [ ] `ADDBBS`: adding a BBS (`~TN`, `~SS` where each is typed)
- [ ] `NODETOP` / `NODEMID` / `NODEBOT`: who's online
- [x] `AICHAT`: the AI chat's header; it stays at the top while the
  conversation scrolls between it and the bar on the bottom row (keep it
  to about 12 rows). Its colours are the `Ai*` strings (`*E`)

## New users

- [ ] `APPLY`: start of the new-user application
- [ ] `NEWUSER`

## Not needed yet (features not ported)

`QWKMAIL` `QWKDL` `QWKUL` `DOOR` `TIMEBANK` `TBNK` `TETRIS` `HIGH*` `NUV`
`INFO*` `CHATMODE` `NODECHAT` `USERL*` `ARCH*` `MICRODOS`. The SysOp editors
draw their own screens and need no art.

## Notes for the artist

- **Codes:** colours, pauses, clear-screen and values like the caller's
  handle can be typed into a screen as text; `CODES.md` lists them all.

- **Size:** 80×25. On menu screens the prompt takes the bottom row or
  two, so keep the art to about 23 rows (MAIN is 18 rows of logo plus 5
  of commands). Stay within 79 columns, or end lines exactly at 80.
- **Repeat screens** (`*TOP` / `*MID` / `*HIL` / `*BOT`) are list
  templates. `TOP` and `BOT` frame the list. `MID` is drawn once per row
  and `HIL` is the highlighted row. They contain codes such as `~AT`
  (area title), `~NF` (file count) and `%UN` (user name) that get filled
  in with live data. Keep those codes, and the columns they sit in. It's
  easiest to copy the stock file and restyle around them.
- **Other screens with codes:** `MSGHEAD` (new: `~RT`, "#12 by phrac"
  for a reply), `MAILHEAD`, `FILEINFO`,
  `USERSTAT`, `USERCFG`, `SHOWBBS`, `ADDBBS` and `FSEDITOR` contain
  positioning or data codes too, so the same rule applies.
- **Colour:** 24-bit colour works (like MAIN) in SyncTERM and modern
  terminals. Classic 16-colour art with iCE colours is the safest choice
  for old clients.
- **Clear screen:** stock menu screens start with a clear-screen code
  (`ESC[2J`). Keep one at the start of full-screen art, otherwise it draws
  over whatever was there.
- **Preview:** `python3 tools/ansi2png.py text/NAME.ANS out.png` renders a
  file roughly as a terminal would.
