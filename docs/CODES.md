# Display codes

A reference for the codes you type as plain text into screens
(`text/*.ANS`), prompt strings (`*E`) and menu text. The board acts on
a code and doesn't show it.

- Letters can be upper or lower case.
- A code takes no room on screen, so text after it on the same line
  moves left by the code's width (3 characters for most).
- For which codes are new over Iniquity, see [NEW.md](NEW.md).

## At a glance

| Looks like | Is | Section |
|---|---|---|
| `\|07`, `\|B1`, `\|U3` | a color | [Colors](#colors-xx) |
| `\|CS`, `\|CU`, `@40`, `&10` | a screen or cursor action | [Screen and cursor](#screen-and-cursor) |
| `\|PA`, `\|DS` | a wait | [Waiting](#waiting-xx) |
| `\|ZA` ... `\|ZB` | text shown as it is | [Literal text](#literal-text-za--zb) |
| `%UN` | a value | [Values](#values-xx) |
| `{20`, `}20` | a width for the next value | [Fitting values](#fitting-values-nn-and-nn) |
| `%>` | right-align the rest of the line (screens) | [Right-aligning](#right-aligning-in-screens-) |
| `%%NAME`, `%!script` | a prompt string's prefix | [Prompt strings only](#prompt-strings-only) |
| `~XX`, `^NN` | a screen marker | [Screen markers](#screen-markers-xx-and-nn) |

## Colors: `|XX`

| Code | Color |
|---|---|
| `\|00`-`\|15` | foreground (table below) |
| `\|16`-`\|23` | background 0-7 |
| `\|B0`-`\|B7` | background 0-7, the same |
| `\|U0`-`\|U9` | the caller's color scheme (below) |
| `\|AS` | save the current color |
| `\|AL` | restore the saved color |

**Foreground** (`|00`-`|15`):

| | | | |
|---|---|---|---|
| 00 black | 04 red | 08 dark gray | 12 light red |
| 01 blue | 05 magenta | 09 light blue | 13 light magenta |
| 02 green | 06 brown | 10 light green | 14 yellow |
| 03 cyan | 07 gray | 11 light cyan | 15 white |

**Background** (`|16`-`|23`, or `|B0`-`|B7`): 0 black, 1 blue, 2
green, 3 cyan, 4 red, 5 magenta, 6 brown, 7 gray.

**Color scheme** (`|U0`-`|U9`): the caller's own palette, set in `*U`.
A screen that uses these follows each caller's colors.

| Code | Slot | Code | Slot |
|---|---|---|---|
| `\|U0` | error | `\|U5` | dim info |
| `\|U1` | text | `\|U6` | bright info |
| `\|U2` | dim text | `\|U7` | item |
| `\|U3` | bright text | `\|U8` | selected item |
| `\|U4` | info | `\|U9` | border |

## Screen and cursor

| Code | Does |
|---|---|
| `\|CS` | clear the screen |
| `\|CZ` | clear to the end of the line |
| `\|HC` | cursor to the top left |
| `\|CU` `\|CD` `\|CL` `\|CR` | cursor up / down / left / right one |
| `\|CT` `\|CB` | cursor to the top / bottom (25 rows) |
| `\|CH` `\|CE` | cursor to the start / end of the line |
| `\|SC` | save the cursor position |
| `\|LC` | restore the saved cursor position |
| `\|LF` | new line |
| `\|**` | after the whole string, come back to this spot |
| `\|SA` | draw the STOCK screen here, then carry on (new) |
| `@NN` | move to column NN (two digits: `@40`) |
| `&NN` | move to row NN (two digits: `&10`) |

`|SA` is for a header shared by many screens: it lives once, in
`STOCK.ANS`. It isn't drawn inside STOCK itself.

## Waiting: `|XX`

| Code | Does |
|---|---|
| `\|PA` | "press any key" prompt (the `HitAKey` string), then wait |
| `\|PK` | wait for a key, silently (put your own "press enter" text before it) |
| `\|PS` | the "paused" prompt, if the caller has pausing on |
| `\|DM` | wait 0.1 second |
| `\|DH` | wait 0.5 second |
| `\|DE` | wait 0.75 second |
| `\|DS` | wait 1 second |
| `\|PO` `\|PE` `\|PD` | 2.20's pausing codes: do nothing here (screens don't pause), but aren't shown |

## Literal text: `|ZA` ... `|ZB`

Everything between `|ZA` and `|ZB` is shown as it is, codes and all
(from 2.0). Use it for ASCII art full of `|`, `%` and `@`.

## Values: `%XX`

A value code is replaced by its value.

**Unknown codes:** in a prompt string, an unknown `%XX` is shown as it
is (so "50%" reads right). In a screen, it's dropped when both
characters are letters, as the original did.

### The caller

| Code | Value |
|---|---|
| `%UN` | handle |
| `%RN` | real name |
| `%LO` | location |
| `%UO` | user note |
| `%SO` | SysOp's note on the caller |
| `%UI` | initials |
| `%ID` | user number |
| `%SL` | security level |
| `%DS` | download level |
| `%UA` | access level |
| `%@A`-`%@Z` | has flag A-Z (Yes/No) |
| `%UC` | total calls |
| `%CT` | calls today |
| `%FC` | first call |
| `%LC` | last call |
| `%UP` | posts |
| `%UE` | email sent |
| `%UZ` | post/call ratio |
| `%MW` | mail waiting |
| `%UL` | uploads |
| `%UK` | upload kb |
| `%DL` | downloads |
| `%DK` | download kb |
| `%FP` | file points |
| `%TL` | minutes left |
| `%TT` | minutes per day |
| `%TB` | minutes in the time bank (2.20) |
| `%BD` | birth date |
| `%SX` | gender |
| `%PN` | phone |
| `%AC` | area code (from the phone number) |
| `%AD` | address |
| `%PW` | always blank (the original showed the password) |
| `%A1`-`%A6` | auto-signature lines 1-6 |
| `%AL` | how many auto-signature lines |
| `%VY` `%VN` | new user voting: yes / no votes on the caller |

**Settings** (Yes/No unless noted):

| Code | Value |
|---|---|
| `%HK` | hot keys |
| `%XM` | expert mode |
| `%PA` | pausing |
| `%AQ` | autoquote |
| `%YN` | yes/no bars |
| `%PL` | page length (a number) |
| `%EM` | emulation |
| `%RT` | terminal type |
| `%LD` | theme name |
| `%LA` | theme author |

### The board

| Code | Value |
|---|---|
| `%BN` | board name |
| `%BL` | board location |
| `%BP` | board phone |
| `%SN` | SysOp handle |
| `%SR` | SysOp real name |
| `%ND` | this node |
| `%IN` | "flux/bbs" |
| `%IV` | version |
| `%VL` | "flux/bbs" and the version |

**Totals:**

| Code | Value | Code | Value |
|---|---|---|---|
| `%TU` | users | `%DC` | calls today |
| `%TC` | calls | `%DP` | posts today |
| `%TP` | posts | `%DD` | downloads today |
| `%TE` | email | `%DU` | uploads today |
| `%TD` | downloads | `%DN` | new users today (2.20) |
| `%TZ` | post/call ratio | | |

**Date and time**, all in the caller's time zone:

| Code | Value |
|---|---|
| `%DT` | date (10/18/26) |
| `%DF` | long date (Saturday, October 24, 2026) |
| `%TI` | time (10:30pm) |
| `%TM` | time (22:30) |

### Where the caller is

| Code | Value |
|---|---|
| `%MA` | message area name |
| `%MI` | message area number |
| `%MT` | message count in the area |
| `%MN` | message number being read |
| `%CM` | message conference |
| `%FA` | file area name |
| `%FI` | file area number |
| `%CF` | file conference |
| `%FS` | new-files scan date (`%FD` too) |
| `%MP` | the menu's prompt name |
| `%IS` | the last thing typed at a menu input |

### The file base (new)

Over the areas the caller can see, worked out once a minute. Before
logon, the figures are what a new caller would see.

| Code | Value |
|---|---|
| `%FT` | files |
| `%FE` | file areas |
| `%FZ` | their total size |
| `%FY` | new since the caller's scan date |
| `%FN` | the newest upload |
| `%FU` | who uploaded it |
| `%FW` | its area |
| `%FG` | how long ago ("today", "3d ago") |
| `%FM` | the most downloaded file |
| `%FX` | its downloads |
| `%FB` | the biggest file |
| `%FL` | its size |
| `%FK` | the top uploader |
| `%FQ` | how many files they uploaded |
| `%FR` | a file at random |
| `%FV` | the first line of its description with words in it |

### The message base (new)

Public messages in the areas the caller can read, worked out once a
minute.

| Code | Value |
|---|---|
| `%MG` | messages |
| `%MY` | new for the caller (in their newscan areas) |
| `%MB` | the newest post's subject |
| `%MF` | its author |
| `%MD` | how long ago |
| `%MK` | its area |
| `%MH` | the most replied-to post |
| `%MR` | its replies |
| `%ML` | the loudest area of the last 30 days |
| `%MQ` | its posts |
| `%MC` | the top poster (not counting anonymous posts) |
| `%MX` | their posts |
| `%MV` | a message at random |
| `%MJ` | a line of it with words in it |

## Fitting values: `{NN` and `}NN`

Put one before a `%XX` code (or a screen's `~XX` value) to fit it to a
width. Color codes in the value take no room.

| Code | Does | Example |
|---|---|---|
| `{NN` | cut to at most NN characters | `{20%UN` |
| `}NN` | pad or cut to exactly NN, so columns line up | `}20%UN` |

The width stays in force for later values in the same screen or string,
until the next `{NN` or `}NN`.

## Right-aligning in screens: `%>`

New: in a screen, `%>` puts the rest of its line against the right edge
(ending at column 79), values and all. Use it for a prompt bar's stats,
say.

In a prompt string, `%>` works only at the start (see below).

## Prompt strings only

A prompt string (not a screen) can start with one of these:

| Prefix | Does |
|---|---|
| `%%NAME` | show the screen `text/NAME` instead of the text |
| `%!script args` | run a Lua script (see [SCRIPTING.md](SCRIPTING.md)) |
| `%\|` | center the line |
| `%>` | right-align the line |

## Screen markers: `~XX` and `^NN`

Markers work in screens only. Rather than showing a value, some mark
where the board draws things:

- the file lister and gallery (`~TL`, `~FN`...);
- the upload test (`~L0`-`~L9`, `~PI`);
- lightbar menus (`~01^01`...) and prompt bars (`~HP`, `~PC`);
- list screens (TOP/MID/HIL/BOT) and the user configuration screen.

Each is listed with its screen in [ARTWORK.md](ARTWORK.md). The easy
way is to copy the stock file and draw around its markers.

## What callers type

Text a caller typed (oneliners, chat, handles, file descriptions,
message headers and so on) keeps its color codes and nothing else. No
moves, clears, pauses, positions or `%XX` values work in it.
