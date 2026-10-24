# New over the original

What flux/bbs has that Iniquity 1.00a26r5 didn't, and where it behaves
differently on purpose. Features from the later 2.0 and 2.20 releases
are listed apart, at the end; features of 1.00a26r5 whose code didn't
survive (the time bank, multinode chat, email tags, attachments...) and
were rebuilt from its manual and strings aren't new, so they aren't
here. Keep this current: when a feature lands that the original didn't
have, add it here (and the details in the doc it belongs to).

It goes by its own name: flux/bbs (`fluxbbs` for the program, its
files and container), crediting Iniquity; it says so in `%IN`/`%VL`,
FTN tear lines and binkp's `VER`. Boards set up before the rename keep
`iniquity.toml` and `data/iniquity.db`.

## Connections and storage

- SSH (with the older algorithms SyncTERM needs) and telnet (binary,
  NAWS, terminal type, Zmodem over it) in place of modems.
- SSH key login and stored logins: a key kept on the account, or the
  account's password as the SSH password (SyncTERM's stored login),
  logs the caller straight in (no bot check or login prompts), under the
  handle or another SSH user name they choose.
  ([SYSOP](SYSOP.md#ssh-keys-and-stored-logins))
- CP437 or UTF-8 terminals; aixterm bright colors, iCE backgrounds, and
  256 and 24-bit color art passed through.
- Output paced to the terminal for fast streams (animated GIFs), and a
  log line when a link stalls.
- TOML files and SQLite in place of the `.DAT` files. Settings, strings,
  areas, levels, protocols, archivers, infoforms and themes can be
  edited while the board runs; `*C` writes back keeping comments.
- An importer for an existing DOS install: users (passwords re-hashed),
  messages, files and descriptions, areas, conferences, levels, strings,
  menus, screens, stats, history, callers, BBS list, infoforms, votes,
  text libraries as themes. ([SYSOP](SYSOP.md))
- Shell commands: `fluxbbs import`, `unban`, `add-files`, `pack-files`,
  `textfiles`, `--wfc`. A Docker image and compose file.
- A board's own `menus/` overrides the stock ones file by file.

## Keeping bots out

- The picture bot check: three big characters drawn with background
  color in noise of the same symbols inside and out, so a script
  reading the text sees nothing; colors change, characters shift and
  are speckled, the field is drawn in shuffled pieces, a decoy comes
  first, too-quick answers fail. A BOTCHECK screen frames it. The
  key-press check is the other option. ([SYSOP](SYSOP.md))
- Per-address connection limits and new connections a minute, bans for
  repeated failures (doubling, up to a day, kept across restarts),
  allow and deny lists (addresses, prefixes, ranges), a pre-login idle
  limit, a security log; SysOps hear at logon about failures since
  their last call.
- Passwords: case-sensitive, up to 64 characters, argon2-hashed; rules
  (length, common passwords, the handle, repeats), enforced at login if
  wanted. Sign-up limits per address and per day, reserved handles, and
  never-validated accounts purged after a set time.
  ([SYSOP](SYSOP.md#passwords-and-sign-ups))

## Menus and screens

- Lightbar menus also take their commands' keys (hot keys, `/X`, digits
  for `#` typed on the help line), the arrows go by where options sit
  on screen so grids work, and they hold 29 options (24 before).
- Board stats as MCI codes, for menus: the file base and the message
  base (see the table below).
- `|SA` draws the shared STOCK header in any screen.
- New screens and markers (table below), and a lister/gallery header
  with counts.

## Files

- Sub-areas, to any depth, with `*F` as a bar list in tree order (`/`
  searches, insert, delete, move, `S` sorts a level by name).
  ([FILES](FILES.md#sub-areas))
- The file browser (`FW`, L on the file menu): areas as folders, ZIP,
  tar and tar.gz opened in place, text and ANSI in a viewer, pictures as
  ANSI (half-blocks, 24-bit), animated GIFs played; `FW newest`, `hot`,
  `heavy`, `dig`, `kingpin` opens it on that file.
- The art gallery (`AG`), after Mystic's: a folder or a file area of
  art, played at a chosen speed, SAUCE shown. ([FILES](FILES.md#art-gallery))
- PDFs in the browser: the text to read, and the pages as pictures
  (Poppler). ([FILES](FILES.md#art-gallery))
- Upload screening: allowed types, contents matching the name, a summary
  of what's inside added to the description, zip bombs and unsafe paths
  refused, an outside virus scanner, an ad file added to ZIPs.
  ([FILES](FILES.md#screening-uploads))
- Descriptions from SAUCE for single pieces of art; RAR, 7Z and LHA
  through bsdtar or 7-Zip.
- Log off after a download, with a countdown.
- Web file lists (`/files.txt`, `/files.html`) are from 2.0, below.
- The textfiles.com archive as file areas: `fluxbbs textfiles fetch`
  catalogs the mirror and brings the files down, `import` adds the
  site's sections as file areas with its directories as sub-areas and
  its descriptions on every file. ([FILES](FILES.md#the-textfilescom-archive))
- Built for a board with hundreds of areas and tens of thousands of
  files: the area list reads counts, not records; `FF`/`FT` search is
  one query and names only the areas with a match (the original named
  each as it read it); `FN` over all areas passes silently over areas
  with nothing new; the viewer (`V` in the lister, `FV`) shows any file
  that reads as text, whatever it's called. `%FE` counts the file areas;
  before logon the file figures are a new caller's. The top uploader
  (`%FK`) is always a caller, never an archive's name.

## Messages

- The message center (`MH`): areas with what's new and the latest post,
  threads (kept across packing, and through QWK), a reader with the
  header fixed and the text scrolling, and read tracking beyond the
  last-read pointer. `MW` says how many posts are new at logon and
  offers to read them. ([FEATURES](FEATURES.md#message-center))
- `MX newest`, `hot`, `dig`, `mouth`, `loud`, `unread`: straight to a
  message, an area or what's new, for the menu's stats.
- Message boards after PTT: the message center shows areas as boards
  with what each is for and how many are reading it now, threads a line
  each with their score up front, the same keys at every level; `x` in
  the reader leaves a one-line vote up, down or comment under a message;
  threads sort by latest, hot or top, boards by number or busiest; a Hot
  list across the board; karma on profiles.
- Sub-boards: message areas under message areas, shown as folders; and
  boards callers make and moderate (ban from posting, about line,
  delete), removed when left empty.
  ([FEATURES](FEATURES.md#message-center))
- Pinned threads, post tags with a filter, and "my boards" (favorites)
  in the message center, after PTT. ([FEATURES](FEATURES.md#message-center))
- A caller's session saves only what it changed in their record, so a
  SysOp's edits (level, flags, daily time) stay while they're online;
  the session reads the edit at once and tells them what changed, and
  more daily time is more time today too.
- @mentions: `@handle` in a public post tells that caller, by node
  message if they're on and at their next logon; `M@` reads them.
  ([FEATURES](FEATURES.md#message-center))
- FTN echomail (fsxNet and the like) built in: a binkp mailer that
  polls and answers (CRAM-MD5), a tosser and scanner with threading and
  dupe checking, netmail to email, and AreaFix; on several networks at
  once, each with its own hub. ([FEATURES](FEATURES.md#ftn-echomail))
- MeshCore radio channels bridged to message areas.
  ([FEATURES](FEATURES.md#meshcore))
- QWK: email as conference 0, threads in and out, four-digit years.
  ([FEATURES](FEATURES.md#qwk-offline-mail))

## Around the board

- AI chat (`AI`): any OpenAI-style server, web search and page reading,
  and the board's own menus, areas and files as the caller may see them.
  ([FEATURES](FEATURES.md#ai-chat))
- Lua scripting in place of IPL, with a `bbs` API. ([SCRIPTING](SCRIPTING.md))
- Doors in a built-in DOS machine (real-mode x86 with a 387, BIOS,
  FOSSIL, DOS, batch files); DOOR32.SYS (`D4`) and no drop file (`D0`).
  SHARE across the nodes (sharing modes, record locks), so multi-node
  doors take several callers at once; doors one caller at a time
  (`[dos] one_at_a_time`), and events that
  run a DOS program (`dos COMMAND`, a door's nightly maintenance).
  ([DOORS](DOORS.md))
- Screen themes picked by callers (`UT`).
- Time zones: the board keeps time in its own zone (`[bbs] time_zone`,
  not the host's clock, which in a container is UTC; doors see it too),
  and callers pick a zone of their own in user config, so posted times,
  last callers and the `%TI` clock are shown in their time.
- Profiles: finger (`UF`, `f` in the reader, after the user list) with
  a plan written in user config. ([FEATURES](FEATURES.md#in-detail))
- `*O` logon sequence: the steps after logging in, ordered and switched
  on and off, with "new users stop here" and a fast logon step that
  offers to skip the rest.
- `*V` events run by the server's own scheduler, with built-in pack jobs.
- `*G` security: bans and recent failures, and lifting a ban.
- A JSON feed of who's on, last callers, new files and posts, and
  totals, for a website. ([SYSOP](SYSOP.md#website-feed))
- The BBS list keeps telnet and SSH addresses, not phone numbers.

## Reference

New commands (2.0's are in FEATURES):

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

New display codes ([CODES](CODES.md)):

| Code | Is |
|---|---|
| `\|SA` | the STOCK header |
| `%FT` `%FZ` `%FY` `%FN` `%FU` `%FW` `%FG` `%FM` `%FX` `%FB` `%FL` `%FK` `%FQ` `%FR` `%FV` | the file base: totals, newest, most downloaded, biggest, top uploader, at random |
| `%MG` `%MY` `%MB` `%MF` `%MD` `%MK` `%MH` `%MR` `%ML` `%MQ` `%MC` `%MX` `%MV` `%MJ` | the message base: totals, new for you, newest, most replied to, loudest area, top poster, at random |

New screens ([ARTWORK](ARTWORK.md)):

| Screen | For |
|---|---|
| `BOTCHECK` (`~BC`, `~BP`) | the picture bot check |
| `STOCK` | the header `\|SA` draws |
| `GALLERY` | the art gallery (else `LISTFILE`) |
| `AICHAT` | AI chat |
| `MHUB` TOP/MID/HIL/BOT | the message center's areas |
| `MTHRD` TOP/MID/HIL/BOT | an area's threads (`~SC` score, `~MK` new, `~DS` day) |
| `MHOT` TOP/MID/HIL/BOT | the Hot list |
| `MSGRHELP` | the reader's help |
| `CFGFAREA` TOP/MID/HIL/BOT | `*F`'s area list |
| `FINGER` | a caller's profile (`~KA` karma) |

New markers in older screens: `~RT` in MSGHEAD (what a message
answers), `~NA` `~NF` `~SZ` in LISTFILE and GALLERY (files, folders,
size), `~TN` `~SS` `~SH` in the BBS list screens (telnet, SSH, SSH
short), `~P?` (SSH keys), `~Q?` (plan) and `~R?` (time zone) in USERCFG.

New settings sections in `fluxbbs.toml`: `[ssh]`, `[telnet]`, `[web]`,
`[gallery]`, `[security]`, `[passwords]`, `[dos]`, `[ai]`,
`[meshcore]`, `[ftn]`; and new keys elsewhere, chiefly upload screening in
`[files]` and sign-up limits in `[newuser]`. New prompt strings are at
the end of `assets/strings.toml` (about 140: bot check, passwords,
upload checks, gallery, viewer, AI, the message center, themes...).

## Changed on purpose

Where the original's behavior was a hazard or no longer fits:

- Passwords are hashed, never shown (`%PW` is blank), case-sensitive,
  and held to rules.
- What callers type keeps its color codes and nothing else, so text
  can't run codes where it ends up.
- The stock NEWUSER menu no longer validates everyone outside area code
  902; lowering a level no longer wraps to the top one.
- QWK's "include your own messages?" works the right way round;
  pointers move to the newest message packed; long reply lines wrap.
- The stock TIMEBANK menu's withdraw entry runs `!W` (it had no command).
- Caller numbers don't repeat when two nodes log on at once.
- Events are one scheduler in the server, not per node; no modem
  off-hook.
- Messages are matched to callers by user number, not name: a handle of
  "None" (every non-user party's alias) or one that matches a name from
  an echo no longer gives someone else's mail or the right to edit and
  delete; mail to a user is theirs alone. Replies to anonymous posts
  stay addressed "Anonymous".
- Passworded message areas stay out of scans, searches, mentions and
  the menu's figures until their password is given in the call (the
  original's newscan skipped the question).
- QWK: "all conferences" only packs conferences the caller may join; a
  reply marked private in an area without private mail is refused, not
  posted in public; a reply goes to the writer of the message it
  answers, whatever its 25-character To says. Messages already read in the
  message center are left out of packets and of `MN`/`MM` scans.
- Characters that a CP437 terminal takes as control bytes (`←` is ESC)
  are left out when a message is shown, and lines with ANSI keep only
  colors and cursor-right, so posts (from the network, the radio or an
  uploaded file) can't drive readers' terminals. The same goes for any
  text the board shows without codes (file and archive names, SAUCE,
  values in prompts): no control characters reach the terminal, and
  values that end up in screen files (the oneliners) can't run codes.
- A reply to a private message is private unless the writer says
  otherwise; deleting a message changes only that, never undoing an
  edit made meanwhile on another node.
- Reading follows messages by row, not number, so a pack on another
  node while someone reads doesn't skip messages or move their pointer
  wrong.
- An area's sponsor is matched by handle, or by real name only for the
  first caller with it (real names aren't unique).
- An @mention in an anonymous post doesn't say which node it came from.
- QWK replies must be for this board (the packet's ID), at most 500 a
  packet and 1000 lines each; ones killed in the reader aren't posted;
  a packet uploaded again after a dropped call carries on where it
  stopped; a reply to an area packed from another
  conference is taken; deleted messages aren't packed; a signature is
  a line starting `--`. Subjects and names from packets, the radio and
  the network lose control characters.
- MeshCore text loses control characters, and a sender using a
  caller's name here shows as `name@mesh`.
- The website feed shows handles, not real names, from real-name areas.
- Typing: CP437 characters go in prompts and the editor as the
  original allowed (only ASCII did before), and a UTF-8 terminal's keys
  are decoded to them; an escape sequence the board doesn't know is
  ignored rather than taken as Esc. Fixed-width fields don't count
  color codes.
- "Anonymous" and "None" can't be taken as handles (mail to those
  parties would reach whoever had them); "delete all read mail" deletes
  only the letters read just now; packing all areas packs email too.
  Editing a post tells callers it now mentions (not those told before).
- A message the editor can't hold whole (over 200 lines, wider than
  79 columns, or with ANSI codes, as echomail can be) isn't opened for
  editing, rather than cut short.
- Reply counts leave out private replies to public messages (they
  gave away that someone had written privately) and deleted ones.
- The message center's lists (threads, new, search, mentions) follow
  messages by row too, so a pack while one is open doesn't show others.
- QWK replies go to the area each conference was when the packet was
  made, even if areas have been added or moved since.
- Private messages aren't offered in areas linked to an echo (echomail
  is public; the scanner never sent them).
- Echomail sent as CP850 or CP866 (`CHRS`) is read as such, not as
  CP437; an echomail writer with a local caller's name isn't taken for
  them.
- `M#` / `F#` with no number open the area list to pick from.
- Deleting an area keeps its echomail dupe history, so relinking the
  echo doesn't take old messages in again.
- The message menu's figures read an index, not every message.

## From 2.0 and 2.20

The voting booth, newscan selections, message search, multinode chat
commands, invisible logons, door names in the status, `D7`, Tab
completion, `|ZA`...`|ZB`, `%DN`, `%TB`, `|PO`/`|PE`/`|PD`, web file
lists and more: see [FEATURES](FEATURES.md#from-20-and-220).
