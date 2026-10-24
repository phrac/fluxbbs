# Files

File areas, transfers, upload screening and the art gallery.

- [Sub-areas](#sub-areas)
- [The area list](#the-area-list)
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
database, like any other. `*F` edits, moves and deletes them; a section
takes its sub-areas with it.

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
- Searching every area (`FF`, `FT`) is one query, and names only the
  areas with a match.
- A newscan of all areas (`FN`) passes over areas with nothing new
  without a word.
- The viewer shows any file that reads as text, whatever its extension
  (`.ana`, `.hum`, none at all).

## File transfers

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
| `~PM` | the lightbar menu of open, play, speed, get and back |
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

`.gif`, `.png` and `.jpg` are shown as ANSI: half-blocks fitted to the
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
