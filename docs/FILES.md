# Files

File areas, transfers, upload screening and the art gallery.

## Sub-areas

New, not in the original: a file area can sit under another one, to any
depth, e.g. a "Zines" area with an area per zine title. In `*F`, set
"Sub-area of" (`K`) to the parent's storage key, or in `fileareas.toml`:

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
`#` to an area with sub-areas opens it too. `FA` lists every area
with sub-areas indented, and `[`/`]`, numbering and `*F` follow the
tree. A sub-area needs the access of every area above it, so a parent's
ACS hides a whole branch. A parent is still an area of its own, with a
directory and room for files. Everything else (newscan, search,
uploads, conferences) treats sub-areas like any other area.

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
Picture uploads are decoded first and refused past `[files]`
`max_image_bytes`, `max_image_pixels` or `max_gif_frames` (`*C`, Upload
Tests); an animated one gets a note that it plays in the gallery, and a
picture with no description gets one ("GIF image, 220x166, 22 frames").
Art made with modern tools (UTF-8 text, LF line ends) is shown as CP437
art.
