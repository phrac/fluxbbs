# Files

File areas, transfers, upload screening and the art gallery.

- [Sub-areas](#sub-areas)
- [The area list](#the-area-list)
- [Managing files](#managing-files)
- [The textfiles.com archive](#the-textfilescom-archive)
- [File transfers](#file-transfers)
- [Screening uploads](#screening-uploads)
- [Art gallery](#art-gallery)

## Sub-areas

A file area can sit under another one, to any depth. For example, a
"Zines" area with an area per zine title under it.

### Making one

In `*F`, "Sub-area of" (`K`) picks the parent from a list of the areas:

- `/` searches the list.
- The first row is the top level.
- The area itself and its own sub-areas aren't offered.

Or, in `fileareas.toml`, set `parent` to the parent's storage key:

```toml
[[area]]
name = "Phrack"
file = "PHRACK"
path = "files/zines/phrack"
parent = "ZINES"
```

### How sub-areas behave

- A sub-area needs the access of every area above it. So a parent's ACS
  hides a whole branch.
- A parent is still an area of its own, with a directory and room for
  files.
- Everything else (newscan, search, uploads, conferences) treats
  sub-areas like any other area.
- `FA` lists every area, with sub-areas indented.
- `[`/`]`, numbering and `*F` follow the tree.
- The file browser (`FW`) makes each area it opens the current one,
  so `q` leaves you in the last area you were in (not one with a
  password; `FA` asks for that).

### Sorting and moving in `*F`

- `S` sorts a level by name: the sub-areas of the area under the bar,
  or the areas beside it if it has none.
- `M` still moves one area at a time.

## The area list

The change-area list (`FC`, `A` on the file menu) shows one level at a
time.

- An area with sub-areas is marked `/` (`FaGroupSuffix`) and counts the
  files under it.
- Choosing it changes to it and opens it. `#` to such an area opens it
  too.

The list is a bar (new) in a window between FAREATOP and FAREABOT. The
bar is drawn with FAREAHIL. The help lines are the strings `FaBarKeys`
and `FaBarInGroup`.

| Key | Does |
|---|---|
| arrows, PgUp/PgDn, Home/End | move the bar and scroll |
| Enter or right | pick the area |
| left or backspace | go back up a level |
| Esc | stay put |
| a number | move the bar to that area, so the original's numbers still work |
| `/` and some text | search (below) |

### Searching the list

1. Type `/` and some text. The bar goes to the next area with that text
   in its row.
2. `/` again goes to the one after.
3. Enter picks it; Esc ends the search.

Every list like this has it, the message areas' too.

## Managing files

A SysOp finds files the way callers do, and works on them where they
are.

1. Find the file: browse (`L`), or search every area (`F`): words in
   its name or description, all of them, in any case; `"two words"`
   together, `*.zip` a filespec, `-word` leaves out. What's found opens
   in the browser, best first (a word in the name counts most), with
   the areas whose name or line about them has the words above the
   files; the info line says which area a file is in, and `f` searches
   again from anywhere in the browser. Areas with a password aren't
   searched. (`FF` and `FT`, the original's searches by filespec and by
   one piece of text, are still there for menus that use them.)

   The search opens on a page of its own: the board's name big in a
   search engine's four colors (`[files] search_font`, a TheDraw font,
   and `search_face`; `search_logo` for another word), a box, and two
   buttons. What the words find drops down under the box as they're
   typed, two matching areas and then the best files; Up and Down pick
   one and Enter opens it, with the bar on the file. A file found by its
   description has a second line saying why: a piece of the description
   round the words, the words lit. Enter in the box
   searches; Tab goes to the buttons, and I'm Feeling Lucky opens the
   best file straight away. Esc goes back. A search that finds nothing
   stays on the page: "no results for ..., did you mean" over searches
   that do find something, picked the same way. A word no file has is
   put right from the words the file base has (the closest by a few
   letters, the commoner first: `nodlist` offers `nodelist`), and words
   that are there but never together are offered a word fewer. "don't
   be evil" sits under it all. A caller without ANSI gets the one-line
   prompt.
2. Put the bar on it and pick `edit`. The file editor (`*Y`) opens on
   that file.
3. Edit the record, rename (`A`), validate (`H`), move (`=`), or
   delete (`*`). Esc goes back to the list, which shows the change:
   a moved or deleted file leaves it.

In the editor itself:

- `[` and `]` step through the area's files.
- `/` takes a number in this area, or a name (wildcards, or any part
  of one) to find in every area. If more than one file matches, pick
  one from a list, which starts on the first match in this area.

A co-SysOp's `edit` in a list edits the description only, as in the
original.

### Descriptions

- `-` in the file editor reads a description from the file: an
  archive's FILE_ID.DIZ (`[files] description_files`), else the first
  `.nfo` in a ZIP, else a piece of art's SAUCE.
- With the AI on, a blind upload (a file sent without naming it first)
  isn't asked for a description: the AI writes one.
- With the AI on (`[ai]`), a file uploaded with no description, or only
  its own name (or a meaningless hash some tool left as its SAUCE
  title, just a picture's size, or the board's own list of what a ZIP
  holds), is described by the AI in the background: a few words from
  its name, size and area, its SAUCE, the start of its text, a PDF's
  words, or a ZIP's contents and the start of the text in it that says
  most (a FILE_ID.DIZ, a read-me, an `.nfo`, then a `.doc` or `.txt`),
  and, for a model that sees, a look at it: a picture as it is, a piece
  of ANSI art drawn as one (`.ans`, `.ice`, `.asc`, `.diz`, `.nfo`), a
  PDF's first page, the first piece of art or picture in a ZIP. The board's own lines (`[ text, 59 lines ]`, a picture's
  size) stay under it. Files put in an area by hand get the same when
  they're added: by the `pack-files` event in the background, and by
  `fluxbbs add-files`/`pack-files` there and then, with progress
  (`--no-describe` skips it; `^C` stops it, and `fluxbbs describe`
  finishes later).
- An upload the AI couldn't describe when it came (it was down, say)
  is caught by a `describe` event: each night it describes the last
  week's uploads still without a description, 50 at most (`describe
  N` for another number).
- `K` in the file editor asks the AI for this file's description; `L`
  for every file in the area that has none worth the name (any key
  stops it).
- `fluxbbs describe [--dry-run | --try] [--limit N] [--area KEY] [config]`
  does the files already on the board: `--dry-run` lists them, `--try`
  shows what the AI would say without saving, `--area` does one area.
  In the container, run it with
  `docker exec fluxbbs fluxbbs describe ... /srv/board/fluxbbs.toml`.
- Each file area can have a line about what's in it (`L`, About, in
  `*F`), shown beside it in the file browser in place of its newest
  file's date. `fluxbbs describe --areas` has the AI write one for each
  area without: it's told where the area sits, its sub-areas and a
  spread of its files' names and descriptions, and does the deepest
  areas first, so an area above goes by what's under it (`--area KEY`
  does that area and everything under it). The lines are kept in the
  database, not `fileareas.toml`, so they're written while the board
  runs and show at once.
- In the file browser (`L`), resting the bar on a file for a moment pops
  up its whole description; any key takes it away.

### Deleting in bulk

New. Two ways, both asking before anything goes:

- **Files:** a SysOp's download batch holds up to 1,000 files. Tag
  files into it as usual, or in the batch editor (`FB`) press `w` to
  add every file in the current area matching a wildcard (`*.exe`) or
  numbers (`3-17,20`). The batch lists what's in it; `k` deletes every
  file in it from its area, and, if asked, from disk.
- **Areas:** in `*F`, `d` on an area with sub-areas offers just that
  area (its sub-areas move up a level) or `a`, the whole section with
  everything under it. Typing numbers instead (`12-40`, `3,5,9`)
  deletes those areas. It then asks whether to delete their file
  records, and then the files on disk too. A folder another area still
  uses keeps its files, and folders left empty are removed.

## The textfiles.com archive

`fluxbbs textfiles` puts Jason Scott's textfiles.com collection on the
board as file areas, sorted as the site sorts it. That's some 58,000
files, 1.3 GB, in 43 sections and several hundred directories under
them.

It's two steps:

```sh
fluxbbs textfiles fetch ~/board/files/textfiles
fluxbbs textfiles import ~/board/fluxbbs.toml
```

### Step 1: fetch

`fetch DIR` brings the archive down into `DIR` (default
`files/textfiles`). It:

1. reads the mirror's index pages, for the catalog: each directory's
   heading, each file's size, and the site's one-line description (or
   its longer notes, in the Top 100);
2. takes each section's tarball from archives.textfiles.com, for the
   bulk of the files;
3. fetches one by one from the mirror whatever the tarball lacks or has
   an older copy of. The tarballs are from 2007; the mirror is newer.

It takes an hour or so. It can be stopped and run again: pages,
tarballs and the catalog (`catalog.json`) stay under `.textfiles/` in
the directory, and only what's missing is fetched.

| Option | Does |
|---|---|
| `--sections anarchy,bbs` | does only those sections |
| `--refresh` | reads the pages again |
| `--mirror URL` | the mirror (default `https://textfiles.meulie.net`) |
| `--archives URL` | the tarballs (default `http://archives.textfiles.com`) |

### Step 2: import

`import CONFIG [DIR]` needs the board stopped, since the running board
keeps `fileareas.toml` in memory.

It adds the site's sections as file areas of the board's own, after the
areas it already has (Anarchy, Hack, Phreak, Magazines..., as a board of
the day sorted its files).

- Each section's directories become sub-areas under it (`TF_ANARCHY`,
  `TF_ANARCHY_CARDING`...).
- Areas are named as the site names them: a section by the directory
  page's name for it, a directory below by its heading.
- Every cataloged file that's on disk gets a record, in the site's
  order, with the site's description, the file's own date, uploader
  `textfiles.com` and no downloads.
- Access, download access and the sponsor are copied from the board's
  first area.
- Uploads to the new areas are the SysOp's alone.

| Option | Does |
|---|---|
| `--under KEY` | puts the sections under an existing area instead |
| `--put SECTION=KEY,...` | folds particular sections into areas of the board's own (below) |
| `--acs ACS` | access for the new areas |
| `--download-acs ACS` | download access |
| `--sponsor WHO` | the sponsor |

### Folding sections into your own areas

With `--put`, a section's files are copied into that area's directory
and records, and its directories become sub-areas of it.

- A name the area has already is left out, and said.
- If an earlier import gave that section an area of its own, that
  area's records are dropped.

For example:

- `--put bbs=BBS,hacking=HACK,phreak=PHREAK,magazines=ZINES` makes the
  site's BBS, hacking, phreaking and e-zine files part of a board's own
  areas of those names.
- `--put art=ARTSCENE` puts the artwork where the [gallery](#art-gallery)
  shows it. Its `.art`, `.pic`, `.pix` and `.vt` files count as art
  there.

### Running it again

Running it again adds only what's new. Areas and files already there
are left as they are, so a later `fetch` and `import` bring the
mirror's additions.

The areas are `[[area]]` entries in `fileareas.toml` and rows in the
database, like any other. `*F` edits, moves and deletes them; to drop
a whole section, `d` on it and `a` takes everything under it, records
and files too if asked ([Deleting in bulk](#deleting-in-bulk)). A
section deleted this way comes back with the next `textfiles import` of
a fetch that still has it.

### In the container

With the board at `/srv/board`, `fetch` runs beside the live board, and
`import` in its place:

```sh
docker compose -f deploy/docker-compose.yml exec fluxbbs \
    fluxbbs textfiles fetch files/textfiles
docker compose -f deploy/docker-compose.yml stop
docker compose -f deploy/docker-compose.yml run --rm fluxbbs \
    fluxbbs textfiles import fluxbbs.toml
docker compose -f deploy/docker-compose.yml up -d
```

### Built for its size

- The area lists read counts rather than records.
- Searching every area (`F`, `FF`, `FT`) is one query. `FF` and `FT`
  name only the areas with a match; `F` lists the best 500.
- A newscan of all areas (`FN`) passes over areas with nothing new
  without a word.
- The viewer shows any file that reads as text, whatever its extension
  (`.ana`, `.hum`, none at all). Arrows, PgUp/PgDn, space and Home/End
  move; `b`, `q` or Esc go back.

## File transfers

### Web links

On a board that gives them out (`[web] download_url`), downloading a
file from a file area offers `W` (Web link) among the protocols. The
board shows a link instead of sending anything:

    https://disfluxbbs.com/dl/cff5ae35ec1e7d880fa058c1d9fd18c3/agoranet.z03

Copied into a browser, it fetches that file over HTTPS. Each link:

- is for one file, and the caller who asked (a batch gets one each);
- lasts `link_minutes` (30) and works `link_uses` times (3), then says
  it has expired; a made-up or old token gets the same;
- counts as that caller's download, the file's count, points, today's
  totals, once, when it's first fetched whole; asking for it is held
  to the same access, points and limits as any download.

Links are kept in memory, so a restart ends them early. Only files of
file areas get them: pieces out of archives and QWK packets don't.

### Protocols

Zmodem is built in, so SyncTERM and other BBS terminals can upload and
download with nothing else installed.

Other protocols are external programs, set in `protocols.toml`. Their
stdin and stdout are connected to the caller, as the original ran DSZ:

```toml
[[protocol]]
key = "Y"
desc = "Ymodem (lrzsz)"
batch = true
send = "sb %F"      # %F: the file paths
receive = "rb"      # runs in the upload directory
```

### Archivers

ZIP is read natively. Other formats go through `archivers.toml`. By
default:

| Format | Uses |
|---|---|
| RAR, 7Z, LHA/LZH | `bsdtar` lists and extracts |
| RAR, 7Z (putting descriptions back) | `rar` / `7z` |
| ARJ | needs 7-Zip |

They're used for description import, the archive viewer and the file
editor.

### Screening uploads

These settings are in `[files]`, or "Upload Tests" in `*C`.

| Setting | Default | Does |
|---|---|---|
| `allowed_types` | empty (any) | upload types allowed, e.g. `["zip", "tar.gz", "tgz", "txt"]` |
| `scan_summary` | `"last"` | where the summary of what's inside goes: `"last"`, `"first"` or `"off"` |
| `scan_names` | 0 | how many names from inside an archive to list with the summary |
| `max_unpacked_ratio` | 100 | refuse archives unpacking to more than this many times their size, past a megabyte |
| `max_unpacked_bytes` | 4 GB | refuse archives unpacking to more than this |
| `refuse_unsafe_paths` | on | refuse archives with names pointing outside their folder |
| `test_command` | none | an outside test run on each upload (`%F`), e.g. a virus scanner; `test_success_code` (0) is the exit code that passes |
| `bbs_ad` | off | a file added to every uploaded ZIP ("BBS ad file" in `*C`) |
| `max_image_bytes`, `max_image_pixels`, `max_gif_frames` | | picture limits (see [Picture uploads](#picture-uploads)) |

Archives are read where they are, not unpacked. The one exception:
a FILE_ID.DIZ is pulled out of a non-ZIP archive for its description.

#### Allowed types

- A named upload of another type is refused before the transfer; a
  blind one when it arrives.
- The contents must match the name: a `.zip` has to be a ZIP, a
  `.tar.gz` gzip, a `.txt` text (no binary). So a renamed program
  doesn't get in.

#### Summary line

What's inside is added to the description, for example:

- `[ 14 files, 1.2 MB unpacked, newest 10/12/26 ]`
- `[ text, 342 lines ]`
- `[ password-protected ]`, where it applies;
- with `scan_names`, the first few names inside.

ZIP and tar/gzip are read in the server; other archives are counted
with `bsdtar`. The lines are `FaScan...` strings, for `*E`.

The scan never changes the upload: no FILE_ID.DIZ is added. When you
want a description in a ZIP, `+` in `*Y` puts one there.

#### Zip bombs

Archives are refused if they unpack to:

- more than `max_unpacked_ratio` (100) times their size, past a
  megabyte; or
- more than `max_unpacked_bytes` (4 GB).

0 turns a limit off. The size is read as far as the limit and no
further.

#### Unsafe paths

With `refuse_unsafe_paths` (on), archives with names that point outside
their folder (`../`, `/etc/...`, `C:`) are refused.

#### Virus scanning

Set an outside scanner, such as ClamAV, and infected uploads fail the
test:

```toml
[files]
test_command = "clamscan --no-summary %F"
```

#### BBS ad

`bbs_ad` names a file in the data directory, e.g. `DISFLUX.NFO`. It is
added to every uploaded ZIP under its own name, replacing an older
copy.

Like `archive_comment` and `delete_list`, this does change the upload
(and its checksum).

## Art gallery

![The art gallery](screenshots/gallery.png)

`AG` (`R` on the main menu) browses art like a file manager, after
Mystic's gallery. Its settings are under "Art Gallery" in `*C`.

### What it shows

It shows one of two things:

- **A folder**: `[gallery] path`, by default `gallery/` beside the
  config. Sub-folders (any depth) and ZIP packs open as folders. Packs
  are read in place, not unpacked.
- **A file area**: set `[gallery] area` to the area's storage key. Its
  sub-areas become folders, and its validated files are listed from the
  file base (packs show their description). So uploads to the art areas
  appear in the gallery by themselves.

Only art is listed, with each piece's SAUCE title, artist and group:
`.ans`, `.ice`, `.asc`, `.txt`, `.nfo`, `.diz`, `.cia`, `.art`, `.pic`,
`.pix` and `.vt`. [Pictures](#pictures) and [PDFs](#pdfs) are listed
too.

| Setting | Default | Is |
|---|---|---|
| `path` | `gallery` | the folder of art |
| `area` | empty | or a file area's storage key |
| `acs` | empty | who may browse it |
| `download_acs` | `s25` | who may download from it |
| `speed` | 38400 | the play speed it starts at |

### Keys

| Key | Does |
|---|---|
| Enter | views a piece in the scrolling viewer |
| P | plays it as it was sent in its day, which is how animated ANSIs animate; any key stops it |
| S | cycles the play speed: instant, 2400, 9600, 14400, 28800, 38400, 57600, 115200 |
| R | reorders the list: by name, newest first, oldest first, by type, by size (biggest first); folders stay at the top, and the order lasts the visit |
| I | the file's information, as the file lister shows it (files in an area; not folders, or pieces inside a pack) |
| E | edits it, for a co-SysOp and up: a SysOp gets the file editor (`*Y`) on that file, a co-SysOp its description |
| D | downloads the piece (pulled out of its pack) or a whole pack, for `download_acs` |

A file of a file area downloads the usual way instead, with that
area's access, points, limits and ratios.

Like the viewer, playback passes only color and cursor codes through.
Art made with modern tools (UTF-8 text, LF line ends) is shown as CP437
art.

### The screen

It looks like the file lister. It shows `GALLERY.ANS` if there is one,
else the lister's own `LISTFILE.ANS`. Without either it draws a plain
screen.

| Marker | Is |
|---|---|
| `~TL` / `~BL` | the list rows |
| `~FN`, `~FS`, `~FD` | the name, size and description columns |
| `~PI` | the info line |
| `~PM` | the lightbar menu of open, play, speed, reorder, get, info, edit and back |
| `~C1`-`~C6` | its colors |
| `~S` / `~E` | the scroll bar |
| `~FA` | where you are |
| `~NA` | the files in the listing (packs count as files) |
| `~NF` | the folders |
| `~SZ` | the files' size |

Browsing the file areas (`FW`) and archives looks the same, but always
uses `LISTFILE.ANS`, so the two can be drawn differently. The lister
(`FL`) fills in `~NA` and `~SZ` too.

### Pictures

`.gif`, `.png`, `.jpg` and `.webp` (its first frame) are shown as ANSI: half-blocks fitted to the
screen.

- Colors are 24-bit on SyncTERM and UTF-8 terminals, else the nearest
  of xterm's 256.
- Animated GIFs play with their own timing until a key.
- They never play faster than the terminal shows them. Each frame asks
  where the cursor is (`ESC[6n`) and the next waits for the answer. So
  a slow link doesn't pile up frames ahead of the caller's keys.
- P replays; D downloads the file as it is.

### PDFs

| Key | Does |
|---|---|
| Enter | shows the text, in reading order, with a rule where each page starts |
| P | shows the pages as pictures, one at a time |
| arrows, space, PgUp/PgDn | turn the pages |
| T | goes back to the text |
| D | downloads |

- OCR'd scans read fine, slips and all.
- A PDF with no text in it opens on its pages.
- The text and page pictures are made once and kept in the data
  directory's `cache/pdf`.
- This uses Poppler's `pdftotext`, `pdftoppm` and `pdfinfo`
  (`poppler-utils`; the Docker image has them). Without them, PDFs can
  still be downloaded.

### Picture uploads

Picture uploads are decoded first. They're refused past these `[files]`
limits (`*C`, Upload Tests):

| Setting | Default |
|---|---|
| `max_image_bytes` | 10 MB |
| `max_image_pixels` | 2048 x 2048 a frame |
| `max_gif_frames` | 300 |

0 means no limit.

- An animated one gets a note that it plays in the gallery.
- A picture with no description gets one, e.g. "GIF image, 220x166, 22
  frames".
