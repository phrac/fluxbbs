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
  passed through.
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
- `*U`'s flags field: the info line says what the flag under the cursor
  is for and which menus, areas and settings test it; `?` describes it
  (`[flags]` in `fluxbbs.toml`). ([FEATURES](FEATURES.md#in-detail))

## Menus and screens

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
- The file browser (`FW`, L on the file menu): areas as folders; ZIP,
  tar and tar.gz opened in place; text and ANSI in a viewer.
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
  text, whatever it's called.
- `%FE` counts the file areas.
- Before logon, the file figures are a new caller's.
- The top uploader (`%FK`) is always a caller, never an archive's name.

## Messages

### The message center

All in [FEATURES](FEATURES.md#message-center).

- `MH`: areas with what's new and the latest post; threads; a reader
  with the header fixed and the text scrolling.
- Threads are kept across packing, and through QWK.
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
- Boards callers make and moderate (ban from posting, about line,
  delete), removed when left empty.
- Pinned threads, post tags with a filter, and "my boards" (favorites).
- A thread as a tree (`v`): who answered whom.
  ([FEATURES](FEATURES.md#the-thread-tree))
- Request boards (`*A` item 6): each thread asks for something, and is
  marked filled (`f`, `m`) with a note on where to find it; the asker
  hears at their next menu. Bots can't make requests.
  ([FEATURES](FEATURES.md#request-boards))

### Mentions

- `@handle` in a public post tells that caller: by node message if
  they're on, and at their next logon. ([FEATURES](FEATURES.md#message-center))
- `M@` reads them.

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

## Around the board

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
- `*G` security: bans and recent failures, and lifting a ban.
- `*F` as a bar list in tree order (see [Files](#files)).
- `*U`'s flags field explains each flag (see
  [Passwords and accounts](#passwords-and-accounts)).

## Reference

### New commands

2.0's are in [FEATURES](FEATURES.md#from-20-and-220).

| Command | Does |
|---|---|
| `AG` | art gallery |
| `AI` | AI chat |
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

### New display codes

Details in [CODES](CODES.md).

| Code | Is |
|---|---|
| `\|SA` | the STOCK header |
| `%>` | right-align the rest of the line (screens) |
| `%FT` `%FE` `%FZ` `%FY` `%FN` `%FU` `%FW` `%FG` `%FM` `%FX` `%FB` `%FL` `%FK` `%FQ` `%FR` `%FV` | the file base: totals, newest, most downloaded, biggest, top uploader, at random |
| `%MG` `%MY` `%MB` `%MF` `%MD` `%MK` `%MH` `%MR` `%ML` `%MQ` `%MC` `%MX` `%MV` `%MJ` | the message base: totals, new for you, newest, most replied to, loudest area, top poster, at random |

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
