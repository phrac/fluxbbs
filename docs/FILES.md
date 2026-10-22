# Files

File areas, transfers, upload screening and the art gallery.

## Sub-areas

New, not in the original: a file area can sit under another one, to any
depth, e.g. a "Zines" area with an area per zine title. In `*F`,
"Sub-area of" (`K`) picks the parent from a list of the areas (`/`
searches it; the area itself and its own sub-areas aren't offered, and
the first row is the top level), or in `fileareas.toml` set `parent` to
the parent's storage key:

    [[area]]
    name = "Phrack"
    file = "PHRACK"
    path = "files/zines/phrack"
    parent = "ZINES"

The change-area list (`FC`, `A` on the file menu) shows one level at a
time: an area with sub-areas is marked `/` (`FaGroupSuffix`) and counts
the files under it, and choosing it changes to it and opens it. New:
the list is a bar in a window between FAREATOP and FAREABOT (the bar
drawn with FAREAHIL): arrows, PgUp/PgDn and Home/End move it and scroll,
Enter or right picks, left or backspace goes back up a level, and Esc
stays put. Typing a number moves the bar to that area, so the original's
numbers still work (`FaBarKeys`, `FaBarInGroup` are the help lines).
`/` and some text searches: the bar goes to the next area with it in its
row, `/` again to the one after, Enter picks it and Esc ends the search
(every list like this has it, the message areas' too).
`#` to an area with sub-areas opens it too. `FA` lists every area
with sub-areas indented, and `[`/`]`, numbering and `*F` follow the
tree. A sub-area needs the access of every area above it, so a parent's
ACS hides a whole branch. A parent is still an area of its own, with a
directory and room for files. Everything else (newscan, search,
uploads, conferences) treats sub-areas like any other area.

## The textfiles.com archive

New: `iniquity textfiles` puts Jason Scott's textfiles.com collection
(some 58,000 files, 1.3 GB, in 43 sections and several hundred
directories under them) on the board as file areas, sorted as the site
sorts it.

    iniquity textfiles fetch ~/board/files/textfiles
    iniquity textfiles import ~/board/iniquity.toml

`fetch` reads the mirror's index pages (the catalogue: each directory's
heading, each file's size and the site's one-line description, or its
longer notes in the Top 100), takes each section's tarball from
archives.textfiles.com for the bulk of the files, and fetches one by one
from the mirror whatever the tarball lacks or has an older copy of
(the tarballs are from 2007; the mirror is newer). It takes an hour or
so and can be stopped and run again: pages, tarballs and the catalogue
(`catalog.json`) stay under `.textfiles/` in the directory, and only
what's missing is fetched. `--sections anarchy,bbs` does only those,
`--refresh` reads the pages again, `--mirror URL` and `--archives URL`
point it elsewhere (the defaults are `https://textfiles.meulie.net` and
`http://archives.textfiles.com`).

`import` needs the board stopped (it keeps `fileareas.toml` in memory):
it adds a `textfiles.com` area (storage key `TEXTFILES`) with a
sub-area per directory of the site (`TF_ANARCHY`,
`TF_ANARCHY_CARDING`...), named as the site names them (a section by
the directory page's name for it, a directory below by its heading),
and a record for every catalogued file that's on disk, in the site's
order, with the site's description, the file's own date, uploader
`textfiles.com` and no downloads. Access, download access and the
sponsor are copied from the board's first area (`--acs`,
`--download-acs` and `--sponsor` override them); uploads are the
SysOp's alone. Running it again adds only what's new: areas and files
already there are left as they are, so a later `fetch` and `import`
bring the mirror's additions. The whole tree is `[[area]]` entries in
`fileareas.toml` and rows in the database like any other: `*F` edits
them, and deleting the `textfiles.com` area in `*F` takes the branch
with it.

In the container (the board at `/srv/board`), `fetch` runs beside the
live board and `import` in its place:

    docker compose -f deploy/docker-compose.yml exec iniquity \
        iniquity textfiles fetch files/textfiles
    docker compose -f deploy/docker-compose.yml stop
    docker compose -f deploy/docker-compose.yml run --rm iniquity \
        iniquity textfiles import iniquity.toml
    docker compose -f deploy/docker-compose.yml up -d

The files section is sized for it: the area lists read counts rather
than records, searching every area (`FF`, `FT`) is one query that names
only the areas with a match, a newscan of all areas (`FN`) passes over
areas with nothing new without a word, and the viewer shows any file
that reads as text, whatever its extension (`.ana`, `.hum`, none at
all).

## File transfers

Zmodem is built in, so SyncTERM and other BBS terminals can upload and
download with nothing else installed. Other protocols are external
programs with stdin/stdout connected to the caller, as the original ran
DSZ (`protocols.toml`):

    [[protocol]]
    key = "Y"
    desc = "Ymodem (lrzsz)"
    batch = true
    send = "sb %F"      # %F: the file paths
    receive = "rb"      # runs in the upload directory

ZIP is read natively. Other formats go through `archivers.toml`: by
default `bsdtar` lists and extracts RAR, 7Z and LHA/LZH, `rar`/`7z` put
descriptions back, and ARJ needs 7-Zip. They're used for description
import, the archive viewer and the file editor.

### Screening uploads

New, in `[files]` or "Upload Tests" in `*C`:

- **`allowed_types`** (empty = any), e.g. `["zip", "tar.gz", "tgz",
  "txt"]`. A named upload of another type is refused before the
  transfer, a blind one when it arrives. The contents must match the
  name: a `.zip` has to be a ZIP, a `.tar.gz` gzip, a `.txt` text (no
  binary), so a renamed program doesn't get in.
- **Summary line** (`scan_summary = "last"`, `"first"` or `"off"`):
  what's inside is added to the description, e.g. `[ 14 files, 1.2 MB
  unpacked, newest 10/12/26 ]`, `[ text, 342 lines ]`, plus
  `[ password-protected ]` where it applies and, with `scan_names`, the
  first few names inside. ZIP and tar/gzip are read in the server;
  other archives are counted with `bsdtar`. The lines are `FaScan...`
  strings for `*E`. The scan never changes the upload (no FILE_ID.DIZ
  is added; `+` in `*Y` puts a description into a ZIP when you want
  one).
- **Zip bombs**: archives unpacking to more than `max_unpacked_ratio`
  (100) times their size, past a megabyte, or more than
  `max_unpacked_bytes` (4 GB) are refused. The size is read as far as
  the limit and no further.
- **`refuse_unsafe_paths`** (on): archives with names that point outside
  their folder (`../`, `/etc/...`, `C:`) are refused.
- **Virus scanning**: set `test_command = "clamscan --no-summary %F"`
  (ClamAV) and infected uploads fail the test.
- **BBS ad** (`bbs_ad`, off by default; "BBS ad file" in `*C`): a file in
  the data directory, e.g. `DISFLUX.NFO`, added to every uploaded ZIP
  under its own name, replacing an older copy. Like `archive_comment`
  and `delete_list`, this does change the upload (and its checksum).

Archives are read where they are, not unpacked (other than pulling a
FILE_ID.DIZ out of a non-ZIP archive for its description).

## Art gallery

![The art gallery](screenshots/gallery.png)

New, after Mystic's: `AG` (`R` on the main menu) browses `[gallery] path`
(default `gallery/` beside the config) like a file manager, or, with
`[gallery] area` set to a file area's storage key, that area: its
sub-areas become folders and its validated files are listed from the
file base (packs show their description), so uploads to the art areas
appear in the gallery by themselves. Sub-folders (any depth) and ZIP
packs open as folders (packs are read in place, not unpacked),
and only art is listed (`.ans`, `.ice`, `.asc`, `.txt`, `.nfo`, `.diz`,
`.cia`), with each piece's SAUCE title, artist and group. Enter views a
piece in the scrolling viewer; P plays it as it was sent in its day, at
the speed S cycles through (instant to 115200; `[gallery] speed` is where
it starts), which is how animated ANSIs animate; any key stops it. D
downloads the piece (pulled out of its pack) or a whole pack, for
`download_acs`; a file of a file area downloads the usual way instead,
with that area's access, points, limits and ratios. Like the viewer, playback passes only colour and cursor
codes through. Settings are under "Art Gallery" in `*C`.

It looks like the file lister: it shows `GALLERY.ANS` if there is one,
else the lister's own `LISTFILE.ANS`, with the same markers (`~TL`/`~BL`
the list rows, `~FN`/`~FS`/`~FD` the name, size and description columns,
`~PI` the info line, `~PM` the lightbar menu of open, play, speed, get
and back, `~C1`-`~C6` its colours, the `~S`/`~E` scroll bar, and `~FA`
for where you are). Without either it draws a plain screen. Browsing the
file areas (`FW`) and archives looks the same but always uses
`LISTFILE.ANS`, so the two can be drawn differently. New: both screens
can also show what's in the listing, `~NA` the files (packs count as
files), `~NF` the folders and `~SZ` the files' size; the lister (`FL`)
fills in `~NA` and `~SZ` too.

Pictures (`.gif`, `.png`, `.jpg`) are listed too and shown as ANSI:
half-blocks fitted to the screen, in 24-bit colour (SyncTERM and UTF-8
terminals) or else the nearest of xterm's 256. Animated GIFs play with
their own timing until a key, but never faster than the terminal shows
them: each frame asks where the cursor is (`ESC[6n`) and the next waits
for the answer, so a slow link doesn't pile up frames ahead of the
caller's keys. P replays, D downloads the file as it is.

PDFs (new): Enter shows the text, in reading order with a rule where
each page starts (OCR'd scans read fine, slips and all), and P shows
the pages as pictures, one at a time: arrows, space and pgup/pgdn turn
them, T goes back to the text, D downloads. A PDF with no text in it
opens on its pages. The text and page pictures are made once and kept
in the data directory's `cache/pdf`. This uses Poppler's `pdftotext`,
`pdftoppm` and `pdfinfo` (`poppler-utils`; the Docker image has them);
without them PDFs can still be downloaded.
Picture uploads are decoded first and refused past `[files]`
`max_image_bytes`, `max_image_pixels` or `max_gif_frames` (`*C`, Upload
Tests); an animated one gets a note that it plays in the gallery, and a
picture with no description gets one ("GIF image, 220x166, 22 frames").
Art made with modern tools (UTF-8 text, LF line ends) is shown as CP437
art.
