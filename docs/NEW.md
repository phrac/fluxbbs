# New over the original

What this port has that Iniquity 1.00a26r5 didn't, and where it behaves
differently on purpose. Features ported from the later 2.0 and 2.20
releases are listed apart, at the end. Keep this current: when a
feature lands that the original didn't have, add a line here (and the
details in the doc it belongs to).

## Connections and storage

- SSH and telnet in place of modems; CP437 or UTF-8 terminals; 256 and
  24-bit colour art passes through. ([FEATURES](FEATURES.md#in-detail))
- TOML files and SQLite in place of the `.DAT` files; settings, strings,
  areas, levels and the rest can be edited while the board runs, and
  `*C` writes back keeping comments. ([SYSOP](SYSOP.md))
- An importer for an existing DOS install (users re-hashed with argon2,
  messages, files, menus, strings, screens...). ([SYSOP](SYSOP.md))
- A Docker image and compose file for running a board. (README)
- Shell commands: `iniquity unban`, `add-files`, `pack-files`, `import`.

## Keeping bots out

- The picture bot check: three big characters drawn with background
  colour in noise, so a script reading the text sees nothing; decoy,
  shuffled drawing, too-quick answers refused; a BOTCHECK screen frames
  it. The key-press check is the fallback. ([SYSOP](SYSOP.md))
- Per-address connection limits, doubling bans kept in the database,
  allow/deny lists, a pre-login idle limit; SysOps hear at logon about
  failures since their last call; `*G` lists and lifts bans.

## Menus and screens

- Lightbar menus take their commands' keys too (hot keys, `/X`, digits
  for `#`), the arrows move by where options sit on screen (so grids
  work), and they hold 29 options. ([ARTWORK](ARTWORK.md))
- `|SA` draws a shared STOCK header in any screen. ([CODES](CODES.md))
- Board stats as MCI codes: the file base (`%FT`...`%FV`) and the
  message base (`%MG`...`%MJ`): newest, most downloaded or replied to,
  biggest, loudest area, top poster, something at random, what's new
  for you. ([CODES](CODES.md))
- Output paced to the terminal for fast streams (animated GIFs).

## Files

- Sub-areas, to any depth. ([FILES](FILES.md#sub-areas))
- The file browser (`FW`): areas as folders, archives (ZIP, tar,
  tar.gz) opened in place, text/ANSI/pictures viewed, animated GIFs
  played. `FW newest`/`hot`/`heavy`/`dig`/`kingpin` opens it on that
  file. ([FEATURES](FEATURES.md#in-detail))
- The art gallery (`AG`), after Mystic's. ([FILES](FILES.md#art-gallery))
- Upload screening: ZIPs read through, age limits, an outside scanner
  command, ads stripped. Descriptions from SAUCE for single pieces of
  art. External archivers (RAR, 7Z, LHA) through bsdtar/7-Zip.
  ([FILES](FILES.md))
- Log off after a download, with a countdown.
- `*F` as a bar list in tree order, with search.

## Messages

- The message centre (`MH`): areas with what's new, threads (kept across
  packing, and through QWK), a reader with the header fixed and the
  text scrolling, and read tracking beyond the last-read pointer.
  ([FEATURES](FEATURES.md#message-centre))
- `MX newest`/`hot`/`dig`/`mouth`/`loud`/`unread`: straight to a
  message, an area or what's new, for menu stats.
- MeshCore radio channels bridged to message areas.
  ([FEATURES](FEATURES.md#meshcore))
- QWK: email as conference 0, threads, four-digit years.
  ([FEATURES](FEATURES.md#qwk-offline-mail))

## Around the board

- AI chat (`AI`), with web search and knowledge of the board.
  ([FEATURES](FEATURES.md#ai-chat))
- Lua scripting in place of IPL. ([SCRIPTING](SCRIPTING.md))
- Doors in a built-in DOS machine (real-mode x86, BIOS, FOSSIL, DOS).
  ([DOORS](DOORS.md))
- `*O` logon sequence editor; `*V` events run by the server's own
  scheduler.
- A JSON feed of who's on, last callers, new files and posts, and
  totals, for a website. ([SYSOP](SYSOP.md))
- The BBS list keeps telnet and SSH addresses, not phone numbers.

## Changed on purpose

Where the original's behaviour was a hazard or no longer fits:

- Passwords are hashed (argon2), never shown (`%PW` is blank), and held
  to rules.
- What callers type keeps its colour codes and nothing else, so text
  can't run codes where it ends up.
- The stock NEWUSER menu no longer validates everyone outside area code
  902; lowering a level no longer wraps to the top one.
- QWK's "include your own messages?" works the right way round.
- The stock TIMEBANK menu's withdraw entry runs `!W` (it had no command).
- Caller numbers don't repeat when two nodes log on at once.

## From 2.0 and 2.20

The voting booth, newscan selections, message search, multinode chat
commands, invisible logons, door names in the status, Tab completion,
`|ZA`...`|ZB`, web file lists and more: see
[FEATURES](FEATURES.md#from-20-and-220).
