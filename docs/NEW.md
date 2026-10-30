# New over the original

What flux/bbs has that Iniquity 1.00a26r5 didn't, and where it behaves
differently on purpose. Each line links to the doc with the details.

- **Keep this current.** When a feature lands that the original didn't
  have, add a line here (and the details in the doc it belongs to).
- **Rebuilt features aren't listed.** Features of 1.00a26r5 whose code
  didn't survive (the time bank, multinode chat, email tags,
  attachments...) were rebuilt from its manual and strings, so they
  aren't new.
- **Later releases are apart.** Features from Iniquity 2.0 and 2.20 are
  at the end, in [From 2.0 and 2.20](#from-20-and-220).

## The name

- It goes by its own name, flux/bbs (`fluxbbs` for the program, its
  files and container), crediting Iniquity.
- It says so in `%IN`/`%VL`, FTN tear lines and binkp's `VER`.
- Boards set up before the rename keep `iniquity.toml` and
  `data/iniquity.db`.

## Connections and terminals

- SSH (with the older algorithms SyncTERM needs) and telnet in place of
  modems. ([FEATURES](FEATURES.md#in-detail))
- Telnet: binary, NAWS, terminal type, and Zmodem over it.
- SSH key login and stored logins: a key kept on the account (up to
  eight), or the account's password as the SSH password (SyncTERM's
  stored login). ([SYSOP](SYSOP.md#ssh-keys-and-stored-logins))
- Such a caller comes straight in, with no bot check or login prompts,
  under the handle or another SSH user name they choose.
- CP437 or UTF-8 terminals. UTF-8 callers get DOS behavior emulated
  (wrap at column 80, `ESC[2J` homing); `[terminal] charset` overrides.
  ([SYSOP](SYSOP.md#terminal-handling))
- aixterm bright colors, iCE backgrounds, and 256 and 24-bit color art
  passed through to terminals that take them; NetRunner
  (`ansi-256color`) gets the nearest of the 256, so the pictures and
  animations show there too, and 16-color terminals (Qodem) the nearest
  DOS color. Other ANSI terminals are asked at connect what they can
  show (device attributes, DECRQSS, CTerm's `ESC[<c`), so IcyTerm gets
  24-bit color though it sends a plain `ANSI`. ([SYSOP](SYSOP.md#terminal-handling))
- Output paced to the terminal for fast streams (animated GIFs), and a
  log line when a link stalls.

## Storage and setup

- TOML files and SQLite in place of the `.DAT` files.
  ([SYSOP](SYSOP.md#configuration))
- Settings, strings, areas, levels, protocols, archivers, infoforms and
  themes can be edited while the board runs.
- `*C` writes `fluxbbs.toml` back keeping its comments.
- An importer for an existing DOS install: users (passwords re-hashed),
  messages, files and descriptions, areas, conferences, levels,
  strings, menus, screens, stats, history, callers, BBS list,
  infoforms, votes, text libraries as themes.
  ([SYSOP](SYSOP.md#importing-an-existing-iniquity-install))
- A board can live in its own directory, apart from the code, taking
  stock screens and menus from `stock_text` and `stock_menus`.
  ([SYSOP](SYSOP.md#running-a-board-outside-the-repo))
- A board's own `menus/` overrides the stock ones file by file; `*#`
  saves a changed stock menu there.
- Shell commands: `fluxbbs import`, `unban`, `add-files`,
  `pack-files`, `textfiles`, `ftn`, `--wfc`.
- A Docker image and compose file.
  ([SYSOP](SYSOP.md#running-in-a-container))

## Keeping bots out

All in [SYSOP](SYSOP.md#keeping-bots-out).

- The picture bot check: three big characters drawn with background
  color, in noise made of the same symbols inside and out. A script
  reading the text sees nothing.
- It fights scripts further: colors change, characters shift and are
  speckled, the field is drawn in shuffled pieces, a decoy comes first,
  and too-quick answers fail.
- A BOTCHECK screen frames it. The key-press check is the other option.
- Per-address limits: connections at once, and new connections a
  minute.
- Bans for repeated failures: doubling, up to a day, kept across
  restarts. `fluxbbs unban` lifts them from the shell.
- Allow and deny lists (addresses, prefixes, ranges).
- A pre-login idle limit.
- A security log. SysOps hear at logon about failures since their last
  call.

## Passwords and accounts

- Passwords: case-sensitive, up to 64 characters, argon2-hashed.
  ([SYSOP](SYSOP.md#passwords-and-sign-ups))
- Password rules (length, common passwords, the handle, repeats),
  enforced at login if wanted.
- Sign-up limits per address and per day.
- Reserved handles.
- Never-validated accounts purged after a set time.
- A caller's session saves only what it changed in their record. A
  SysOp's edits (level, flags, daily time) stay while they're online.
- The session reads such an edit at once and tells the caller what
  changed; more daily time is more time today too.
- More ways over a logo than the glint (`wave`, `drips`, `sparkle`,
  `breathe`), and `random` draws one of them each time a screen comes
  up. ([ARTWORK](ARTWORK.md#animation-new))
- The art gallery's list and the AI chat animate while they wait, from
  `GALLERY.FX` and `AICHAT.FX` beside their screens; and the chat
  pulses dots (`AiThinking`) on the reply's line until the first words
  come.
- `glint_seconds` (`*C`, SSH, Telnet and Terminal; 12 to start) paces
  every menu's and screen's effects: they're timed for a glint every
  10 seconds, and all their timings scale to this, so the hidden
  messages keep decoding as the glint passes.
- The screen saver's rain falls in the color of the screen it covers,
  Backspace on any lightbar menu starts it at once, and the board's
  name (or a message of the SysOp's, `screensaver_message`) assembles
  out of it, set in a TheDraw font if one is named
  (`screensaver_font`, `screensaver_face`; `*C`, SSH, Telnet and
  Terminal). `*C` picks the font and face from a list of every face in
  the board's `.TDF` files, with the saver's text previewed in the one
  under the bar. Too wide for the screen, it falls back to the board's
  short name. TheDraw fonts are read by `tdf.rs`.
  ([ARTWORK](ARTWORK.md#animation-new))
- A short name for the board (`[bbs] short_name`), for logo fonts too
  wide for the full name, and a super short one (two characters) for
  where even that won't fit. ([SYSOP](SYSOP.md#the-boards-short-name))
- `*C` shows the description of the setting under the bar as you move
  it, over two lines when it's long, and keeps it up while you edit.
- `*C` reaches every setting in the config: the paths (`1`, in two
  columns), QWK mail (`2`), DOS doors and the weather (`3`), Carrier
  Lost's payouts, the status lights, the feed's new posts, and the
  lists (FTN networks and echoes, MeshCore channels) as list editors.
- The weather (`WX`; the `weather` component), after Lone Wolf
  Software's AI-WX door but our own: the sky over the caller's place in
  animated half-block pixels, the temperature large, the next 24 hours
  and the week, from Open-Meteo; `f` has the AI read it like a TV
  forecaster, a weatherman at a map with radar and fronts, his words in
  the lower third (thinking off, `[weather] ai_effort`, so it isn't cut
  short). `h` is the last 30 days against the 1991-2020 normal
  and the day's records since 1940, `d` the U.S. Drought Monitor for the
  caller's county (and the rain against the normal anywhere), `n` the
  country on a map colored by the temperature, the cities' readings on
  it. A caller's place is kept; `WX <place>` shows a fixed one.
  A profile location that's only a state or a country isn't used: it
  asks the town.
  ([BUILDING](BUILDING.md))
- Email addresses confirmed by a code (the `email` component, off until
  `[email] enabled`): a logon step asks for an address, mails a
  six-digit code over SMTP and takes it back; a confirmed address can
  raise the caller's level. ([SYSOP](SYSOP.md#email-addresses))
- Optional components: doors, the games, effects, screen savers, AI,
  echomail, MeshCore, PDFs, Lua and the textfiles importer are Cargo
  features; a plain build is the core board, `--features full` all of
  it. A part left out answers as one switched off and drops out of the
  menus; its settings still load. ([BUILDING](BUILDING.md))
- More screen savers: a warp starfield and the demoscene fire beside the
  digital rain, the board's default in `*C` (`screensaver_style`;
  random if it isn't set) and each caller's own pick in their
  configuration (`X`). ([SYSOP](SYSOP.md#screen-savers))
- An ACS for callers who never time out for being idle
  (`[bbs] idle_exempt_acs`, e.g. a user flag).
  ([SYSOP](SYSOP.md#callers-who-never-time-out))
- The last callers list draws the caller's own call (the newest of
  theirs) highlighted (`LASTCHIL`), and, animating, types it in and
  runs a glint across it: that last call is this one.
- `%RC` `%RP` `%RU` `%RD` `%RF` rank the caller among everyone by
  calls, posts, uploads, downloads and file points ("3rd of 41"),
  `%O`+letter draws a ten-cell bar of it and `%K`+letter says the
  share ("top 8%"), for a stats screen that compares.
  ([CODES](CODES.md))
- `*C`, Features: whole parts of the board switch off, each game on
  its own (tarot, slots, casino, Carrier Lost, gallery, message
  center, time bank, voting, BBS list, infoforms, QWK, automessage,
  scripts). Off, a part refuses its commands and drops out of every
  menu as it loads. ([SYSOP](SYSOP.md#turning-parts-of-the-board-off))
- `*P`: the SysOp's own questions for new users (text, yes/no, a
  number or a choice; required or not), asked after the stock ones
  when someone applies. The answers are kept with the infoform
  answers, shown in `*U` (`&`, then `0`) and to new user voters before
  the vote. ([SYSOP](SYSOP.md#configuration))
- `*U`'s flags field: the info line says what the flag under the cursor
  is for and which menus, areas and settings test it; `?` describes it
  (`[flags]` in `fluxbbs.toml`). ([FEATURES](FEATURES.md#in-detail))

## Menus and screens

- Menus can animate while they wait (`[[effect]]` in the menu): a glint
  across the art, a pulse along the box lines, sent as only the cells
  that change, and after a minute idle a screen saver of digital rain.
  Callers can turn it all off in their config. Any screen can animate
  at its `|PK` with effects in a `NAME.FX` beside it (firelight,
  glowing eyes, dust in the light, text typing itself out, a hidden
  message that scrambles and settles into the next).
- Logging off (`/G`, or the goodbye menu's logoff, which now thanks the
  caller the same way instead of clearing the screen) switches the screen off
  as an old TV did: what's showing flickers (`logoff_flicker_ms`, 3.5 s
  to start; `*C`, SSH, Telnet and Terminal), squashes to a bright
  line, shrinks to a dot that fades, a last word shows on the dark
  (`TvOffLater`, `*E`), then the line drops. Not for plain-text callers
  or those who turned animation off.
  ([ARTWORK](ARTWORK.md#animation-new))
- Lightbar menus also take their commands' keys: hot keys, `/X`, and
  digits for `#` typed on the help line. ([ARTWORK](ARTWORK.md#lightbar-menus))
- Lightbar arrows go by where options sit on screen, so grids work.
- Lightbar menus hold 29 options (24 before).
- `~PC` marks where a lightbar's cursor waits (a prompt bar's slot).
- Lists with a moving bar (area lists, `*F`...), where the original
  asked for a number; numbers still work, and `/` searches.
  ([FILES](FILES.md#sub-areas))
- A list's BOT screen can be a prompt bar (`~HP` keys, `~PC` slot).
  ([ARTWORK](ARTWORK.md#prompt-bar-bot-screens))
- A message listing page is as many rows as fit under MSGLTOP.
- Board stats as MCI codes, for menus: the file base and the message
  base. ([CODES](CODES.md#the-file-base-new))
- `|SA` draws the shared STOCK header in any screen.
- `%>` right-aligns the rest of a line in a screen.
- New screens and markers (see [Reference](#reference)), and a
  lister/gallery header with counts.

## Files

- Sub-areas, to any depth. ([FILES](FILES.md#sub-areas))
- `*F` is a bar list in tree order: `/` searches, insert, delete, move,
  and `S` sorts a level by name.
- Deleting in bulk: `*F` deletes a whole section, or areas by number
  (`12-40`), with their records and files if asked; a SysOp's batch
  (`FB`) takes up to 1,000 files, adds by wildcard or numbers (`w`) and
  deletes them all (`k`). ([FILES](FILES.md#deleting-in-bulk))
- The file browser (`FW`, L on the file menu): areas as folders; ZIP,
  tar and tar.gz opened in place; text and ANSI in a viewer. The area
  being browsed becomes the current one, so leaving stays there.
- In the browser, pictures show as ANSI (half-blocks, 24-bit) and
  animated GIFs play.
- `FW newest`, `hot`, `heavy`, `dig`, `kingpin` open the browser on
  that file.
- The art gallery (`AG`), after Mystic's: a folder or a file area of
  art, played at a chosen speed, SAUCE shown.
  ([FILES](FILES.md#art-gallery))
- PDFs in the browser: the text to read, and the pages as pictures
  (Poppler). ([FILES](FILES.md#art-gallery))
- Upload screening: allowed types, contents matching the name, and a
  summary of what's inside added to the description.
  ([FILES](FILES.md#screening-uploads))
- Zip bombs and unsafe paths refused.
- An outside virus scanner (`test_command`, e.g. ClamAV) in place of
  the DOS one.
- An ad file added to uploaded ZIPs.
- Descriptions from SAUCE for single pieces of art.
- RAR, 7Z and LHA through bsdtar or 7-Zip; ZIP read natively.
- `fluxbbs add-files` adds files copied into every area from the shell;
  a ZIP without a FILE_ID.DIZ is described by what's in it.
- Log off after a download, with a countdown.
- The textfiles.com archive as file areas: `fluxbbs textfiles fetch`
  catalogs the mirror and brings the files down.
  ([FILES](FILES.md#the-textfilescom-archive))
- `fluxbbs textfiles import` adds the site's sections as file areas,
  its directories as sub-areas, and its descriptions on every file.
- Web file lists (`/files.txt`, `/files.html`) are from 2.0, below.

### Built for a big file base

For a board with hundreds of areas and tens of thousands of files:

- The area list reads counts, not records.
- `FF`/`FT` search is one query and names only the areas with a match
  (the original named each as it read it).
- `FN` over all areas passes silently over areas with nothing new.
- The viewer (`V` in the lister, `FV`) shows any file that reads as
  text, whatever it's called; `b` goes back from it, as from the lists.
- `%FE` counts the file areas.
- Before logon, the file figures are a new caller's.
- The top uploader (`%FK`) is always a caller, never an archive's name.

## Messages

### The message center

All in [FEATURES](FEATURES.md#message-center).

- `MH`: areas with what's new and the latest post; threads; a reader
  with the header fixed and the text scrolling.
- Threads are kept through QWK.
- Messages are never packed away (`*P`, and areas' message limits, are
  gone): deleting a post marks it, and moderators can undelete it.
  Deleted email goes for good, at once. Packing was for DOS-era disks.
- Read tracking beyond the last-read pointer.
- `MW` says how many posts are new at logon and offers to read them.
- `MX newest`, `hot`, `dig`, `mouth`, `loud`, `unread`: straight to a
  message, an area or what's new, for the menu's stats.

### Boards after PTT

- The message center shows areas as boards: what each is for, and how
  many are reading it now.
- Threads a line each, with their score up front; the same keys at
  every level.
- `x` in the reader leaves a one-line vote (up or down) or comment
  under a message.
- Threads sort by latest, hot or top; boards by number or busiest.
- A Hot list across the board.
- Karma on profiles.
- Sub-boards: message areas under message areas, shown as folders.
- New callers start in an area named by key (`[messages] new_user_area`),
  wherever it's moved to; the original started everyone in area 1.
- Boards callers make and moderate (ban from posting, about line,
  delete), removed when left empty.
- Pinned threads, post tags with a filter, and "my boards" (favorites).
- A thread as a tree (`v`): who answered whom.
  ([FEATURES](FEATURES.md#the-thread-tree))
- Request boards (`*A` item 6): anyone asks, anyone answers, and only
  the asker marks it filled (`f`, `m`), picking the answer that did it.
  Bots can't make requests. ([FEATURES](FEATURES.md#request-boards))
- Points for fills, topsite style without the drama: never taken back,
  paid once a request, file points too where used; the top fillers
  (`MF`, `$`). ([FEATURES](FEATURES.md#points-for-fills))
- File links in messages and comments (`file:AREA/NAME`): `/L` in the
  editor or `/l` in a comment picks one from a lightbar; `g` in the
  reader lists them to download. ([FEATURES](FEATURES.md#file-links))
- Quotes say who wrote them ("phrac wrote:") and mark lines with `> `,
  not the author's initials; a line too wide with them goes on at the
  start of the paragraph's next line, rewrapped, rather than losing its
  end. ([FEATURES](FEATURES.md#quoting))
- The reader can have the board's prompt bar (MSGRBAR).
- The thread tree (`v`) shows each message's comments under it.
- Multinode chat says how it works on the way in, and shows its
  commands at the bottom without a NODECHAT screen.
- A SysOp's `edit` in a file list (and in search results) opens the
  whole file editor on that file; `*Y`'s `/` finds a file in any area.
  ([FILES](FILES.md#managing-files))
- A thread's reply count takes in its comments; `%MU` is the caller's
  posts in the area.
- A post left without a title says nothing was posted, rather than
  going quietly back to the list; Esc gets out of the reader's
  `Message #` prompt.
- Editing your own post adds to it, reddit style: the original stays,
  the edit goes under a dated line. ([FEATURES](FEATURES.md#editing-your-posts))
- Taking back a post you regret (`d` on your own): take it back while
  it's fresh and unanswered, retract it (hidden, kept, undoable), or
  burn it for good for a few file points.
  ([FEATURES](FEATURES.md#taking-it-back))
- The editor opens with one of the board's mottos ("the internet never
  forgets"). ([FEATURES](FEATURES.md#editor-hints))
- A reply in the reader starts with what it answers ("in reply to #1
  by ..."); `t` in a thread already open shows its tree.
- One way to answer a post: `r` in the reader is a comment under it,
  like `x` then `>` (email and echomail still get a real reply).
  ([FEATURES](FEATURES.md#votes-and-comments))
- A tarot and fortune door read by the AI (`AT`): card of the day,
  three cards for a question, fortune cookies; cards drawn with
  pictures, 3 readings and 3 fortunes a day; its own menu (TAROT) and
  `%TR`/`%TF` for what's left.
  ([FEATURES](FEATURES.md#tarot-and-fortunes))
- A slot machine (`AL`): three big reels drawn in 24-bit half-block
  pixels, lit as cylinders, blurred as they spin, stopping one by one
  with a jolt; wins glow and rain coins. Pulls a day, how loose it is
  and what each line pays (minutes and file points) are in `*C`.
  ([FEATURES](FEATURES.md#slot-machine))
- A casino (`AC`): a lobby under a neon sign, with the slot machine,
  video poker, blackjack, roulette and craps, a bar (drinks poured on the house, a
  smoke, the bartender's gossip) and a players club whose comps (the
  buffet, a suite, show tickets) come with play. Pixel-art cards slide out of the shoe and chips
  move to whoever won them; red dice tumble and bounce off the back
  wall. The tables play for a free stack of chips a day, and winnings
  pay out in file points and minutes. A free slot pull on the first
  call of the day is a logon sequence step.
  ([FEATURES](FEATURES.md#casino))
- Points around the board (`!P`): up votes pay a post's author, doors
  pay by the minute, callers tip each other (`$` in the reader), and a
  lottery is drawn each night from the day's tickets: a ball machine
  in half-block pixels, the pot in lights, and last night's draw
  replayed for those who held tickets (a logon step too). All of it is
  kept in a ledger, with caps a day in `*C`, Points.
  ([FEATURES](FEATURES.md#points))
- Esc leaves an infoform without saving; the original had no way out.
- A handle typed for a tip shows the callers who match as you type, in
  a box to pick from (plain text: Tab completes).
- Message lists show the year with the day (mm/dd/yy).
- ANSI uploaded as a message (an ad) can be posted as it came, and its
  colours and spacing go out on echomail.
- Catching up: `z` in the message center marks a board, a folder or
  every board read (plain text: `z <#>`, `z all`, `z` in a board).
- Carrier Lost (`AP`): a reflex war-dialing run on the pixel stage.
  Tune a dial until the needle sits on a live carrier and lock on, then
  catch a three-signal handshake in its sync band for a haul of bytes.
  Every dial raises a trace meter; cash out to fence the bytes for file
  points and a spot on a shared leaderboard, or let the trace hit 100
  and lose the run till tomorrow. Banked bytes buy quieter gear that
  slows the trace. No typed commands and no AI; a short typed version
  for plain terminals. (Replaces the earlier AI-told dial-up adventure.)
  ([FEATURES](FEATURES.md#carrier-lost))

### Mentions

- `@handle` in a public post tells that caller: by node message if
  they're on, and at their next logon. ([FEATURES](FEATURES.md#message-center))
- `M@` reads them.
- The editor completes `@` to a real handle (Tab takes the dimmed
  suggestion); co-SysOps can write `@all`, `@mods` or `@here`.

### QWK

All in [FEATURES](FEATURES.md#qwk-offline-mail).

- Email goes in as conference 0.
- Threads in and out.
- Four-digit years.

## Networks

- FTN echomail (fsxNet and the like) built in, where the original
  handed packets to outside programs. ([FEATURES](FEATURES.md#ftn-echomail))
- A binkp mailer that polls and answers (CRAM-MD5).
- A tosser and scanner, with threading and dupe checking.
- Echomail replies from software that writes no REPLY kludge (Renegade,
  Oblivion/2...) are shown threaded by subject (jwz's subject step),
  under the latest post on the topic from whoever they're to, else
  under its first; nothing is stored, so "in reply to" stays for what a
  message says itself. Replies that arrive before their parents
  (rescans) still thread, and wait for them 30 days from arriving.
- Netmail to the board goes to callers' email.
- AreaFix requests from the shell (`fluxbbs ftn areafix`).
- Several networks at once, each with its own hub.
- MeshCore radio channels bridged to message areas.
  ([FEATURES](FEATURES.md#meshcore))

## Doors

All in [DOORS](DOORS.md).

- Doors in a built-in DOS machine: real-mode x86 with a 387, BIOS,
  FOSSIL, DOS and batch files. No DOSBox or FOSSIL driver to install.
- SHARE across the nodes (sharing modes, record locks), so multi-node
  doors take several callers at once.
- Doors one caller at a time (`[dos] one_at_a_time`).
- Events that run a DOS program (`dos COMMAND`), such as a door's
  nightly maintenance.
- DOOR32.SYS (`D4`) and no drop file (`D0`).
- Each node has its own drop directory.
- CHAIN.TXT has real times and transfer totals, and points its
  directories at the drop directory.
- Native doors: any Linux program, on a pseudo-terminal (curses
  programs work).
- `[dos] emulator` runs a door under DOSBox instead, for one the
  machine can't run.
- `dosrun` tries a door outside the board.
- `D7` ignores a DOOR.SYS the door didn't change, so a door that never
  writes it (or a hangup) doesn't refund the time spent in it.
- Leaving a door logs the bytes typed, passed on and shown, and when
  the last key came, so a door that seems to ignore keys shows whether
  they reached it.
- Code and Markdown in the viewer: source code in colors, Markdown laid
  out, code on a shaded background; the same for code blocks in
  messages and AI chat replies. Each caller picks a code theme and a
  Markdown theme (Nord, Dracula, One Dark, Monokai, classic) and the
  code background. ([FEATURES](FEATURES.md#code-and-markdown))
- BBSLink: games on bbslink.net's server, with players from other
  boards (`DB code`, the stock BBSLINK menu). ([DOORS](DOORS.md#bbslink))
- A log of every door played (who, which, when, how long), for the
  doors menu's figures: the `%G?` codes. ([CODES](CODES.md#door-games-new))

## Around the board

- Plain text, for bots (flag B) and anyone who sets it in user config:
  no ANSI, art or pauses; menus listed and answered a line at a time,
  the message boards as line commands, messages written a line at a
  time, files listed with their links, uploads a question a line.
  Message lines keep their indentation and take Tab, so code posted
  this way keeps its shape. ([FEATURES](FEATURES.md#plain-text))
- Profiles: an uploaded ANSI header and an opt-in real name.
  ([FEATURES](FEATURES.md#profiles))
- A short AI-kept memory of each caller's interests, updated after each
  call, which they can see and clear; SysOps' private caller notes
  (`*N`); flag `B` for bots, which get neither. The AI chat suits
  itself to both. ([FEATURES](FEATURES.md#what-the-board-remembers))
- Menu hints: what's waiting for the caller, in the help line when a
  menu opens. ([FEATURES](FEATURES.md#menu-hints))
- A guide for AI agents that call in as the SysOp's assistant: filling
  requests, uploading, doors, news posts, bug reports.
  ([AGENTS](AGENTS.md))
- BBS list descriptions of several lines (up to 10; a pasted one comes in
  whole), shown in SHOWBBS at `~I2`-`~I9` or under the screen.
  ([FEATURES](FEATURES.md#the-bbs-list))
- AI chat (`AI`): any OpenAI-style server, web search and page reading.
  ([FEATURES](FEATURES.md#ai-chat))
- The AI sees the board's own menus, areas and files, as the caller may
  see them.
- Lua scripting in place of IPL, with a `bbs` API.
  ([SCRIPTING](SCRIPTING.md))
- Screen themes picked by callers (`UT`).
  ([SYSOP](SYSOP.md#screen-themes))
- Time zones: the board keeps time in its own zone (`[bbs] time_zone`),
  not the host's clock (which in a container is UTC). Doors see it too.
- Callers pick a zone of their own in user config, so posted times,
  last callers and the `%TI` clock are shown in their time.
- Profiles: finger (`UF`, `f` in the reader, after the user list), with
  a plan written in user config. ([FEATURES](FEATURES.md#in-detail))
- The BBS list keeps telnet and SSH addresses, not phone numbers.
- A JSON feed of who's on, last callers, new files and posts, and
  totals, for a website. ([SYSOP](SYSOP.md#website-feed))

## SysOp tools

- `*O` logon sequence: the steps after logging in, ordered and switched
  on and off. ([FEATURES](FEATURES.md#in-detail))
- It has "new users stop here", and a fast logon step that offers to
  skip the rest.
- `*V` events run by the server's own scheduler, with built-in pack
  jobs.
- Backups: the database (snapshotted while the board runs), settings,
  screens, door data and host keys in one file, nightly as an event
  (`backup`) or by hand (`fluxbbs backup`), old ones removed.
  ([SYSOP](SYSOP.md#backups))
- `*G` security: bans and recent failures, and lifting a ban.
- `*W` screen viewer: every screen with its random variants and weekday
  overrides, and any one shown as a caller sees it, effects and all.
  Art taller than the screen scrolls. ([ARTWORK](ARTWORK.md#trying-a-screen))
- `*-` the trash: deleted posts, newest first, with who deleted them and
  the days left before they're purged (`[messages] keep_deleted_days`,
  30 to start), to restore, read or purge now; the board empties it
  daily by itself. ([FEATURES](FEATURES.md#the-trash))
- `*J` font viewer: every TheDraw font in `fonts/` (`[paths] fonts`), a
  face at a time, with the board's name, short name and super short
  name set in it, cut off at the screen's edge; `n`/`p` step, `f` finds
  one by file or face name. Outline faces are left out.
- `*F` as a bar list in tree order (see [Files](#files)).
- `*U`'s flags field explains each flag (see
  [Passwords and accounts](#passwords-and-accounts)).
- `*C` on one screen: its sections in four groups, what each holds on
  the bottom bar, which features are on, and the board right now
  (uptime, nodes, calls today, AI, FTN, MeshCore).

## Reference

### New commands

2.0's are in [FEATURES](FEATURES.md#from-20-and-220).

| Command | Does |
|---|---|
| `AG` | art gallery |
| `AI` | AI chat |
| `AT` (+ `day` `three` `fortune`) | tarot and fortunes |
| `AC` (+ `slots` `poker` `blackjack` `roulette` `craps` `bar` `club`) | the casino |
| `AL` (+ `free`) | the slot machine; `free`, the day's free pull |
| `!P` (+ `lottery` `tip`) | your points, the lottery, tips |
| `AP` | Carrier Lost (the dial-up game) |
| `DB` (+ door code) | BBSLink doors |
| `FW` (+ `newest` `hot` `heavy` `dig` `kingpin`) | file browser |
| `MH` | message center |
| `MW` (+ `Read`) | new posts at logon |
| `MX` (+ `newest` `hot` `dig` `mouth` `loud` `unread`) | jump to a message from the stats |
| `M@` (+ `All`) | posts that mention you |
| `UT` | pick a screen theme |
| `UK` | your SSH keys |
| `UF` (+ handle) | finger someone |
| `D0`, `D4` | door with no drop file, with DOOR32.SYS |
| `*G` | security log and bans |
| `*O` | logon sequence |
| `*W` | screen viewer: any screen or variant, as callers see it |
| `*J` | font viewer: the board's names in every TheDraw font |
| `*-` | the trash: deleted posts to restore or purge |

### New display codes

Details in [CODES](CODES.md).

| Code | Is |
|---|---|
| `\|SA` | the STOCK header |
| `%>` | right-align the rest of the line (screens) |
| `%FT` `%FE` `%FZ` `%FY` `%FN` `%FU` `%FW` `%FG` `%FM` `%FX` `%FB` `%FL` `%FK` `%FQ` `%FR` `%FV` | the file base: totals, newest, most downloaded, biggest, top uploader, at random |
| `%MG` `%MY` `%MB` `%MF` `%MD` `%MK` `%MH` `%MR` `%ML` `%MQ` `%MC` `%MX` `%MV` `%MJ` | the message base: totals, new for you, newest, most replied to, loudest area, top poster, at random |
| `%GL` `%GW` `%GA` `%GP` `%GC` `%GK` `%GQ` `%GH` `%GN` `%GX` `%GJ` `%GZ` `%GO` `%GD` `%GG` `%GE` `%GF` `%GY` `%GU` `%GV` | door games: last played, most played, most time, longest play, who's in one now, gathering dust, untried, the caller's own |

### New screens

Details in [ARTWORK](ARTWORK.md).

| Screen | For |
|---|---|
| `BOTCHECK` (`~BC`, `~BP`) | the picture bot check |
| `STOCK` | the header `\|SA` draws |
| `GALLERY` | the art gallery (else `LISTFILE`) |
| `AICHAT` | AI chat |
| `MHUB` TOP/MID/HIL/BOT | the message center's areas |
| `MTHRD` TOP/MID/HIL/BOT | an area's threads (`~SC` score, `~MK` new, `~DS` day, `~FL` filled) |
| `MTREE` TOP/MID/HIL/BOT | a thread as a tree (`~TR`, `~TS`, `~RT`, `~TX`) |
| `MHOT` TOP/MID/HIL/BOT | the Hot list |
| `MSGRHELP` | the reader's help |
| `CFGFAREA` TOP/MID/HIL/BOT | `*F`'s area list |
| `FINGER` | a caller's profile (`~KA` karma) |

### New markers in older screens

| Screen | Markers |
|---|---|
| lightbar menus | `~PC` (where the cursor waits) |
| list BOT screens | `~HP`, `~PC` (a prompt bar) |
| MSGHEAD | `~RT` (what a message answers) |
| LISTFILE, GALLERY | `~NA` `~NF` `~SZ` (files, folders, size) |
| BBS list screens | `~TN` `~SS` `~SH` (telnet, SSH, SSH short) |
| USERCFG | `~P?` (SSH keys), `~Q?` (plan), `~R?` (time zone) |

### New settings

- New sections in `fluxbbs.toml`: `[ssh]`, `[telnet]`, `[web]`,
  `[gallery]`, `[security]`, `[passwords]`, `[terminal]`, `[dos]`,
  `[ai]`, `[meshcore]`, `[ftn]`, `[flags]`.
- New keys elsewhere, chiefly upload screening in `[files]`, sign-up
  limits in `[newuser]`, and callers' boards in `[messages]`.
- New prompt strings are at the end of `assets/strings.toml`: about
  260 (bot check, passwords, upload checks, gallery, viewer, AI, the
  message center, themes...).

## Changed on purpose

Where the original's behavior was a hazard or no longer fits.

### Security and privacy

- Passwords are hashed, never shown (`%PW` is blank), case-sensitive,
  and held to rules. Old ones, stored upper-cased, work in any case
  until changed.
- What callers type keeps its color codes and nothing else, so text
  can't run codes where it ends up.
- The stock NEWUSER menu no longer validates everyone outside area code
  902.
- Lowering a level no longer wraps to the top one.
- "Anonymous" and "None" can't be taken as handles: mail to those
  parties would reach whoever had them.
- An area's sponsor is matched by handle, or by real name only for the
  first caller with it (real names aren't unique).
- The website feed shows handles, not real names, from real-name areas.
- DOOR.SYS's password line is empty.

### Whose message is whose

- Messages are matched to callers by user number, not name.
- So a handle of "None" (every non-user party's alias), or one that
  matches a name from an echo, no longer gives someone else's mail or
  the right to edit and delete.
- Mail to a user is theirs alone.
- Replies to anonymous posts stay addressed "Anonymous".
- An @mention in an anonymous post doesn't say which node it came from.

### Private and passworded

- Passworded message areas stay out of scans, searches, mentions and
  the menu's figures until their password is given in the call (the
  original's newscan skipped the question).
- A reply to a private message is private unless the writer says
  otherwise.
- Reply counts leave out private replies to public messages (they gave
  away that someone had written privately) and deleted ones.
- Private messages aren't offered in areas linked to an echo (echomail
  is public; the scanner never sent them).

### Reading, editing and packing

- Reading follows messages by row, not number. A pack on another node
  while someone reads doesn't skip messages or move their pointer
  wrong.
- The message center's lists (threads, new, search, mentions) follow
  messages by row too, so a pack while one is open doesn't show others.
- Deleting a message changes only that, never undoing an edit made
  meanwhile on another node.
- A message the editor can't hold whole (over 200 lines, wider than 79
  columns, or with ANSI codes, as echomail can be) isn't opened for
  editing, rather than cut short.
- Editing a post tells callers it now mentions (not those told before).
- "Delete all read mail" deletes only the letters read just now.
- Packing all areas packs email too.
- `M#` / `F#` with no number open the area list to pick from.
- The message menu's figures read an index, not every message.

### Display and typing

- Characters a CP437 terminal takes as control bytes (`←` is ESC) are
  left out when a message is shown.
- Lines with ANSI keep only colors and cursor-right, so posts (from the
  network, the radio or an uploaded file) can't drive readers'
  terminals.
- The same goes for any text the board shows without codes (file and
  archive names, SAUCE, values in prompts): no control characters reach
  the terminal.
- Values that end up in screen files (the oneliners) can't run codes.
- CP437 characters go in prompts and the editor as the original allowed
  (only ASCII did before); a UTF-8 terminal's keys are decoded to them.
- An escape sequence the board doesn't know is ignored rather than
  taken as Esc.
- Fixed-width fields don't count color codes.

### QWK

- "Include your own messages?" works the right way round.
- Pointers move to the newest message packed.
- Long reply lines wrap rather than being cut at 79 columns.
- Areas with a password are left out of packets.
- "All conferences" only packs conferences the caller may join.
- Messages already read in the message center are left out of packets
  and of `MN`/`MM` scans.
- Deleted messages aren't packed.
- QWK replies must be for this board (the packet's ID): at most 500 a
  packet and 1000 lines each.
- Replies killed in the reader aren't posted.
- A packet uploaded again after a dropped call carries on where it
  stopped.
- A reply marked private in an area without private mail is refused,
  not posted in public.
- A reply goes to the writer of the message it answers, whatever its
  25-character To says.
- Replies to unknown users are refused in email (posted to the name in
  public areas, as before).
- A reply to an area packed from another conference is taken.
- Replies go to the area each conference was when the packet was made,
  even if areas have been added or moved since.
- A signature is a line starting `--`.
- Subjects and names from packets lose control characters.

### Networks

- Echomail sent as CP850 or CP866 (`CHRS`) is read as such, not as
  CP437.
- An echomail writer with a local caller's name isn't taken for them.
- Deleting an area keeps its echomail dupe history, so relinking the
  echo doesn't take old messages in again.
- Subjects and names from the radio and the network lose control
  characters.
- MeshCore text loses control characters, and a sender using a
  caller's name here shows as `name@mesh`.

### The board

- The stock TIMEBANK menu's withdraw entry runs `!W` (it had no
  command).
- Caller numbers don't repeat when two nodes log on at once.
- Events are one scheduler in the server, not per node.
- No modem off-hook.
- A menu's "press any key" (`pause_before`) comes only after a command
  that showed something to read (two lines or more): not after changing
  menus, nor coming back from a list, a door or anything full-screen.
  A lightbar menu pauses after such output even in expert mode (it's
  drawn again over it, so the output would vanish).
- One-line notes ("Added to my boards", "Nothing new", a door that
  wouldn't start) go on the help line of the list or menu the caller
  returns to, until their next key, not behind a "press any key".
- The stock screens, themes and menus say flux/bbs where they said
  Iniquity, but for the original's copyright line (PRELOGON.AN2) and
  the credit on the wait-for-call screen.

## From 2.0 and 2.20

Iniquity 2.0 and 2.20 came after the release this ports. Their
features here, all in [FEATURES](FEATURES.md#from-20-and-220):

- The voting booth.
- Newscan selections.
- Message search.
- Multinode chat commands.
- Invisible logons.
- Door names in the status.
- `D7` (DOOR.SYS read back).
- Tab completion of handles.
- `|ZA`...`|ZB`, `%DN`, `%TB`, `|PO`/`|PE`/`|PD`.
- Web file lists.
- And more.
