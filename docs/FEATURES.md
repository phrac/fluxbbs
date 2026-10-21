# Features

Everything the port does, in detail. The [README](../README.md) has the short list.

## In detail

**Connections**
- SSH (russh), including the older algorithms SyncTERM's cryptlib needs.
  New: log in with an SSH key kept on the account (`UK`, `k` in user
  config), or the account's password as the SSH password (a terminal's
  stored login), with no bot check or login prompts (SYSOP.md).
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
- Bot check before anything else, on SSH and telnet (new): three big
  characters drawn with background colour in a field of noise, to be
  typed; a person reads them, a script reading the text sees noise
  (SYSOP.md). The key-combination check is still there as an option.
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
  passwords, fallbacks, command help. Lightbar menus (new) also take
  their commands' keys (hot keys, `/X`, and digits for `#`, typed on the
  help line), the arrows go by where options are on screen so a grid of
  them works, and they hold up to 29 options (the original took 24).
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
  (blind or named, with FILE_ID.DIZ import, or for a single piece of
  art, alone or alone in a ZIP, a description from its SAUCE: title,
  artist/group and date, comments), search and newscan, local upload.
  Browsing (`FW`, L on the file menu; new) works like the art gallery
  over the file areas: areas are folders (with their file counts and
  newest upload), archives (ZIP, tar, tar.gz) open as folders in place,
  and Enter shows a file: text and ANSI in the viewer, pictures as ANSI
  (animated GIFs play); D downloads, with the area's usual checks. `FV`
  opens an archive the same way. Names are shown whole, sizes as
  "4.5 KB", and typed names keep their case. Q leaves the browser from
  anywhere; back goes up a level. Screens can show what's in the file
  base (new, `%FT`...`%FV` in `CODES.md`): totals, the newest upload, the
  most downloaded and biggest files, the top uploader, and a file at
  random with a line of its description. `FW newest` (`hot`, `heavy`,
  `dig`, `kingpin`) opens the browser on that file, so a lightbar
  option on a menu can take the caller straight to it. Zmodem built in; other protocols and archivers
  (RAR, 7Z, LHA via bsdtar/7-Zip) are external programs. Before a
  download the caller can ask to be logged off when it's done (a
  countdown afterwards; `*C`, Transfers).
  Uploads are tested on the FILETEST screen (`*C`, Upload Tests): ZIPs
  are read through (a CRC failure refuses the file) and can be held to
  an age limit; an outside command can be run on every upload in place
  of the DOS virus scanner (`clamscan --no-summary %F`, say); files on
  the delete list (BBS ads) are stripped from ZIPs and the archive
  comment added.
- Profiles (new): finger (`UF`, `f` on a message in the reader, or a
  name after the user list) shows a caller's location and note, since
  when and when last on (or on now, unless invisible), calls, posts,
  uploads and downloads, their latest post and uploads, and their plan:
  up to six lines they write in user config (`p`). Real name, phone,
  address and birth date stay private. A FINGER screen can dress it
  (`~UN` `~LO` `~UO` `~FC` `~LS` `~CA` `~PO` `~UL` `~DL` `~LP` `~LU`,
  `~P1`-`~P6`).
- Oneliners, the automessage, last callers, history and statistics, the
  BBS list, user configuration, user listing. BBS list entries have a
  telnet and an SSH address (`host:port`) in place of the original's
  phone number and baud rate (screens: `~TN` telnet, `~SS` SSH, `~SH`
  SSH shortened to `:port` on the telnet host, for lists; older screens'
  `~PN` shows the telnet address).
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
  [SCRIPTING.md](SCRIPTING.md).
- QWK offline mail: `QD` packs new messages (email as conference 0)
  for an offline reader, `QU` posts the replies (`[qwk]` in
  `iniquity.toml`). See [QWK offline mail](#qwk-offline-mail).
- Doors, native and DOS (in the built-in DOS machine): `D1` DORINFO1.DEF, `D2` DOOR.SYS,
  `D3` CHAIN.TXT, `D4` DOOR32.SYS, `DZ` DOOR.SYS and DORINFO1.DEF, `D0`
  none, `D7` DOOR.SYS read back afterwards (2.0). See [DOORS.md](DOORS.md).

**SysOp tools** (the `*` commands on the SysOp menu)
- `*C` system configuration, written back to `iniquity.toml` keeping its
  comments; `*E` prompt strings; `*#` menus; `*L` access levels.
- `*U` users (with colours, preferences, auto-signature, infoforms).
- `*F` file areas (new: a bar list in tree order with `/` to search it;
  Enter edits the area under the bar, `I` adds one before it, under the
  same parent, and edits it, `D` deletes it and `M` moves it), `*Y` files (validate, move, extract/apply DIZ), `*S`
  sort areas, `*>` download any file, `*%` local upload (add files
  copied into the area's directory; `iniquity add-files` does every
  area from the shell, for a mirror, say: a ZIP without a FILE_ID.DIZ
  is described by what's in it), `*K` pack file areas (forget files
  missing from their directories; also `iniquity pack-files` from the
  shell, as FILEPACK.EXE was).
- `*A` message areas, `*P` pack them, `*@`/`*!` conferences, `*R` all
  email.
- `*X` protocols, `*Z` archivers, `*B` BBS list, `*H` statistics, `*I`
  infoforms, `*T` themes.
- The waiting-for-caller screen (WAIT4CAL, ANSIWFCS), with `--wfc`:
  the original's screen on the console while the server runs. F1-F8
  (or 1-8) and the arrow keys pick from six pages of commands: local
  logon, a shell, the SysOp editors (run as user #1, the SysOp), launch
  a menu, read or view email, edit a text file, and stats pages (system,
  totals, today, last callers). The node and event fields show callers
  on and the next event; the window that echoed the modem shows the log
  (all of it goes to `data/iniquity.log`). ESC exits unless `[wfc]
  esc_to_exit` is off; `*C` has the settings. Modem commands, modem setup
  and MicroDOS are left out. WAIT4CAL.PAS is damaged, so the stats pages
  are rebuilt from their titles and fragments.
- `*G` security: current bans and recent failed logins and bot checks;
  lift a ban.
- `*V` events: jobs run once a day at a set time, by one scheduler in
  the server (no per-node events or modem off-hook any more). A command
  goes to the shell, or is one of the built-in jobs `pack-messages` and
  `pack-files`; `force` disconnects callers first (with the stock
  "event initiating NOW" string), `run_missed` catches up after
  downtime the same day. "Run it now" in the editor starts one by hand.
- `*O` logon sequence (new): the steps after logging in (mail,
  mandatory messages, new posts, last callers, who's online, the
  automessage, your stats, oneliners, pauses, any screen of your own),
  in the order you put them, each on or off. It's kept in `logon.toml`
  with the board and the start menu (STARTUP) is built from it; without
  the file STARTUP runs as written. The oneliners run in place, so they
  needn't come last. "New users stop here" sends callers who aren't
  validated to NEWUSER: steps above it are seen by them too.

**Importing** an existing install: users (passwords re-hashed with
argon2), message bases, file records and descriptions, areas,
conferences, levels, strings, menus, text files, statistics, history,
callers, BBS list, infoforms and their answers, new user votes, text
libraries (unpacked into themes).

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

## AI chat

New: `AI` (J on the main menu) is a chat with a language model, for
callers the SysOp lets in. `[ai] acs` is `s250|fI` as stock: SysOps,
and anyone given flag I in the user editor (`*U`). It talks to any
OpenAI-style chat completions server (`base_url` up to `/v1`, `model`,
and `api_key_env` naming an environment variable with a key, if the
server needs one), streams replies as they're written, word-wrapped and
without markdown or the model's thinking, and keeps the conversation for
as long as the caller stays (`/new` starts over, `/quit` or ESC leaves).
With `web_search` on, the model may search the web, up to four times a
reply: through a SearXNG instance (`search_url`), else the Brave Search
API (`search_key_env`), else DuckDuckGo's HTML page, which turns away
bots now and then. With `read_pages` on too, it may read a page it
found (as text, at most 8000 characters), but only from public
addresses: never the board itself, the LAN or the tailnet. It can always
look at the board itself (`aiboard.rs`): the menus with their keys and
where they lead, the file and message areas, and the files in them
(listed or searched), all as the caller's own access allows, so it can
say what's here and which keys to press. `daily_limit` caps a caller's messages a day (not
the SysOp's); `name`, `system_prompt`, `max_tokens`, `reasoning_effort`
and `timeout_seconds` do what they say. All of it is under "AI Chat" in
`*C`.

After each reply a line says what it took (`AiStats`: tokens written,
of those the thinking, and tokens a second), and the bottom bar shows
how much of the model's context the conversation fills (`AiContext`,
the length from the server's `/models`), when the server reports usage
(`stream_options.include_usage`). Blank `AiStats` in `*E` to hide the
line.

Under AICHAT.ANS the conversation scrolls between the header and the
bar, which both stay put: the board keeps the lines it has shown and
draws the rows again one higher (terminal scroll regions left old text
showing through in SyncTERM). A reply that fills the rows pauses
(`AiMore`, for callers who pause): any key goes on, Esc lets the rest
run. The reply keeps coming in during a pause. Up/Down, PgUp/PgDn and
Home look back over the last 1000 lines (`AiBack`), at the prompt or a
pause; End, or typing, comes back.

![The AI finding files on the board](screenshots/ai-files.png)

![The AI giving the keys to download a file](screenshots/ai-howto.png)

## Message centre

New: `MH` (`r` on the message menu) and `MW Read` (`n` there, and the
newscan prompt `SCANMSG` asks on the way in) replace the classic reader
and newscan (`MR`, `MN`, still there for menus that want them).

- The areas, with how many messages are new in each and the latest
  post. Enter opens one, `n` reads everything new, `p` posts.
- An area is a list of threads, the latest activity first, with their
  replies and what's new. Enter reads a thread from its first new
  message, `n` reads what's new in the area, `p` starts a thread.
- The reader keeps the header (MSGHEAD) at the top and scrolls the text
  under it, with the keys on the bottom line: left and right for the
  previous and next message, up and down to scroll, `r` reply, `p` post,
  `t` the whole thread, `u` up to the message this one answers, `l` or
  Esc back, `q` stop, `?` help, a number to go to that message.
- Threads: a message keeps the row id of the one it answers
  (`reply_to`), so threads survive packing. QWK replies join their
  thread by the header's reference number, and packets carry it, so
  offline readers thread too.
- What's read: besides each area's last-read pointer (everything up to
  it is read; QWK and the classic reader use it), messages read beyond
  it are remembered, and the pointer moves up once everything before
  them is read too. Your own posts are read.
- `MW` (in STARTUP, after the mandatory scan) says how many new posts
  there are and offers to read them, a thread at a time; `MW Read` reads
  them without asking.
- The website feed lists the newest public posts (`new_posts`).
- @mentions (new): `@handle` in a public post (typed, by QWK or from
  MeshCore; not in quoted lines, and not yourself) tells that caller, at
  once with a node message if they're on ("mentioned you in General:
  subject", `MentionNotice`), and at their next logon, where `MW` first
  offers the posts that mention them. `M@` reads the unseen ones any
  time (`M@ All`: the last hundred). Handles with spaces work
  (`@Dark Angel`); punctuation ends a name. Only callers who could read
  the area are told, and `@5` isn't user number 5.
- Screens can show what's in the message base (`%MG`...`%MJ` in
  `CODES.md`): the newest post, the most replied to, the loudest area
  of the month, the top poster, a message at random, and what's new for
  the caller. `MX newest` (`hot`, `dig`, `mouth`) opens that message in
  its thread, `MX loud` that area's threads, `MX unread` reads
  everything new, so lightbar options on a menu can go straight there.

## MeshCore

New: a [MeshCore](https://meshcore.co.uk) companion radio's channels
bridged to message areas (`meshcore.rs`). What's said on a channel is
posted in its area, from the sender's name; what a caller posts there
(in the editor or by QWK) goes out on the channel as `handle: text`,
without the signature or quoted lines, split into at most `max_parts`
messages of the radio's 160 bytes. Private mail stays on the board.

    [meshcore]
    enabled = true
    address = "192.168.1.50:5000"   # WiFi companion firmware; or ser2net

    [[meshcore.channel]]
    channel = "Public"              # the channel's name on the radio
    area = "MESH"                   # the message area's storage key
    # from_mesh / to_mesh: one way only, if wanted

The board speaks MeshCore's companion protocol over TCP (the WiFi
companion firmware's port 5000; a USB radio can be put on the network
with ser2net), reconnects when the link drops, and picks up changed
settings within a minute. `*C`, MeshCore, has the switch, the address,
the reconnect delay and the parts; channels are listed in the file.
`tools/fake_meshcore.py` is a pretend radio for trying it out.

## FTN echomail

New: the board can be a node of a FidoNet-style network such as fsxNet,
with its own mailer and tosser (the original handed packets to outside
programs). It polls its hub over binkp (`binkp.rs`, FTS-1026; the
session password goes as CRAM-MD5 when the hub offers it), sends what's
been posted here and tosses what came (`ftntoss.rs`).

- Echoes are message areas: `[[ftn.area]]` maps a network tag to an
  area's storage key. Messages come in with their kludges read (MSGID
  for duplicates, or who/when/subject for one without; REPLY for
  threads, even when the reply comes first; CHRS for UTF-8, Latin-1,
  CP1252 or CP437, the names and subject too; TZUTC, so times are the
  board's), long lines wrapped, and
  the sender's address (from the origin line) as their note; the tear
  and origin lines count as the signature, so quotes leave them out.
  Escape sequences from the network are cut down to colours and
  cursor-right (ANSI art's spacing): nothing from elsewhere can clear a
  reader's screen or talk to their terminal. @mentions of local callers
  work. ZIP bundles and plain packets are both read (a ZIP without
  packets, a nodelist say, is left alone); only packets from the hub,
  to this node, with the packet password (its first 8 characters, all
  the header holds) are taken. Anything that can't be tossed goes to
  `data/ftn/bad` (under a new name if one's taken), from where it can be
  moved back to `inbound` to try again; so does a packet with mail for
  an echo that isn't mapped to an area, or netmail for another address,
  so nothing is lost while it's seen to (what was tossed from it comes
  in again as dupes). The other packets in a bundle are tossed all the
  same, and a bundle that isn't a ZIP (ARC, ARJ) goes to bad too. If
  the board's side fails (the database busy), the packet stays in
  inbound for the next toss. Dupes are remembered after their
  messages are packed away. Renaming an area's key in `*M` moves its
  mapping with it.
- What callers post in those areas goes out at the next poll with
  MSGID, REPLY (a reply to a network message threads there too), PID,
  TZUTC and CHRS, a tear and origin line, SEEN-BY and PATH; colour codes
  are taken out. An area starts from what's in it when it's first
  scanned, so old posts aren't sent; posts that came from the network
  (or from MeshCore) aren't sent back. A post is marked sent only once
  its packet is written, and one packed again after a failed write
  keeps its MSGID, so the network can tell a resend. A line of text
  starting `SEEN-BY:` goes out as `SEEN+BY:`, and control characters
  are left out, so nothing typed can pass for a control line; coming
  in, only the SEEN-BY lines at the end of a message count.
- Netmail for the node goes to the named caller's email (else the
  SysOp's), from `name@address`. `iniquity ftn areafix +FSX_GEN -FSX_X`
  (or `%LIST`) asks the hub's AreaFix to link and unlink echoes.
- The hub can call the board too, to deliver mail as it arrives
  ("crash" rather than "hold"): with `[ftn] listen` set (`0.0.0.0:24554`,
  published by the Docker compose file) the board answers binkp, offers
  CRAM-MD5, and lets in only its hub with the session password; a wrong
  one counts as a failed login (security log, bans), and connections
  are held to the same per-address limits as callers. One session runs
  at a time, and one toss or scan, across processes too (lock files in
  `data/ftn`, so `iniquity ftn` beside the running board is safe): a
  call during a poll is told to try again. A hub that has part of a
  file from a broken session can ask for the rest (M_GET). A poll the
  hub answers as non-secure (it doesn't have the session password yet)
  sends nothing, so mail isn't left in its unsecure inbound; the log
  says why. Files over 256 MB are skipped, names are made safe, and a
  session quiet both ways for five minutes is ended. A hub calling with
  the wrong password is turned away and logged, but not banned, so a
  password changed at its end can't cut the board off.
- `[ftn]` (or "FTN Echomail" in `*C`): the node's address with its
  domain (`21:1/999@fsxnet`), the hub's address and binkp host, the
  session, packet and AreaFix passwords, the origin line, how often
  to poll, and where to answer. `iniquity ftn poll`, `toss` and `scan` run the jobs by hand.

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

Not yet: callers writing netmail to an FTN address, and file echoes
(TIC).

## From 2.0 and 2.20

Iniquity 2.0 (IDT, 1998) and jack phlash's 2.20A (2001) came after the
release this ports. Their source isn't here; these follow 2.0's manual,
WHATSNEW.IQ2, menus, screens and 2.20's string table and notes.

- Voting booth (`VV` vote on what's new, `VA` one question, `VR`
  results, `VN` add a question, `VD` delete one), the stock VOTING menu
  (`V` on the main menu). Questions can let callers add answers.
  Screens: VQTOP/MID/BOT (`~QN` `~QD`), VATOP/MID/BOT (`~VQ`, `~AN`
  `~AD`), VRTOP/MID/BOT (`~VQ`, `~AP` `~AD` `~VN` `~VR`); without them,
  plain lists with a bar per answer.
- Newscan selections: `ME`/`FE` (`E` on the message and file menus)
  turn areas in and out of newscans with a bar list (MSCAN/FSCAN
  screens, `~AN` `~AT` `~SC`); "all areas" scans, reading everything new,
  the logon new-posts check and QWK packets leave the others out. `US`
  puts every area back for everyone.
- Message search (`MO`, `B` on the message menu): all areas or one;
  from, to, subject, all text or your own; new or all messages. Matches
  open in the reader. 2.0's MSGSRCH screen is shown first if there.
- Multinode chat: `/who`, `/me`, `/msg user text` (someone in chat),
  `/page user text` (anyone on), `/help`, and Enter alone for who's in
  chat.
- Invisible (`XI`): after `>L` in the matrix, a hidden logon (asks
  `LogonInvisible` unless given `Y`): out of the callers list and other
  callers' who's-online, node messages and the website feed. Elsewhere
  it hides or shows the caller (`I` on the multinode menu). SysOps
  still see everyone.
- ACS `n#`: on node #.
- A door's name in the caller's status: the part after `;` in its
  command (`LORD.BAT;Legend of the Red Dragon`), or the menu item's
  description; the website feed says "in LORD".
- Tab completes a handle at "To:" and in `?I` with `/AC` (and
  `bbs.input{complete = true}`).
- `|ZA`...`|ZB` shows text as it is. 2.20's `%DN` (new users today),
  `%TB` (time in the bank), `|PO`/`|PE`/`|PD` (accepted; screens don't
  pause here). "Set newscan date" starts at the date the pointer is at.
- File lists for the web: `/files.txt` and `/files.html` (2.0's
  `iniquity fltxt`/`flhtml`), only areas a new caller could open.
- A cap on imported description lines (`*C`, Files: DIZ lines).
- `bbs.inmenu(keys)` for scripts (IPL's `isincurmenu`); the rest of
  2.0's new IPL functions have Lua equivalents (see SCRIPTING.md).
- 2.0's bug "'|XX' would kill multinode chat" has its equivalent here,
  fixed: what callers type (chat, oneliners, handles, file
  descriptions, message headers, BBS list entries...) keeps its colour
  codes and nothing else. Typed into `?I` or `bbs.input`, other codes
  are wrapped in `|ZA`...`|ZB` so they show but don't act where the
  text ends up (the stock oneliners append to a screen); handles with
  codes in them are refused. Caller numbers no longer repeat when two
  nodes log on at once ("caller list numbering with multinode").

Left out: the modem and NetModem/32 items, MicroDOS, WFC changes, 8+
COM ports, phone formats, SETUP.SR/CALINFO.BBS/SFDOORS.DAT drop files
(no door here needs them yet), the local-console keys alt-L and alt-V
(alt-D, down a level, is `<` in the user editor), and 2.20's paging of
long screens.

## Not planned

Tetris (`TP`, `TS`; the game's source isn't in this
release, `tetris.pas` being a misnamed terminal unit), modem setup
(`*M`), MicroDOS (`*D`, a shell on the host),
the local console's status bar (STATBAR), and the Oblivion/Renegade
converters (OBV2INIQ, REN2INIQ).
