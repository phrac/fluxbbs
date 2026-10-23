# Display codes

Codes typed as plain text into screens (`text/*.ANS`), prompt strings
(`*E`) and menu text. The board acts on them and doesn't show them, so
anything after a code on the same line moves left by the code's width
(3 characters for most). Letters can be upper or lower case.

## Colors: `|XX`

| Code | Color |
|---|---|
| `|00`-`|15` | foreground: 00 black, 01 blue, 02 green, 03 cyan, 04 red, 05 magenta, 06 brown, 07 gray, 08 dark gray, 09 light blue, 10 light green, 11 light cyan, 12 light red, 13 light magenta, 14 yellow, 15 white |
| `|16`-`|23` | background 0-7 (black, blue, green, cyan, red, magenta, brown, gray) |
| `|B0`-`|B7` | background, the same 0-7 |
| `|U0`-`|U9` | the caller's color scheme (`*U` palette): 0 error, 1 text, 2 dim text, 3 bright text, 4 info, 5 dim info, 6 bright info, 7 item, 8 selected item, 9 border. Lets a screen follow each caller's colors. |
| `|AS` / `|AL` | save / restore the current color |

## Screen and cursor: `|XX`

| Code | Does |
|---|---|
| `|CS` | clear the screen |
| `|CZ` | clear to the end of the line |
| `|HC` | cursor to the top left |
| `|CU` `|CD` `|CL` `|CR` | cursor up / down / left / right one |
| `|CT` `|CB` | cursor to the top / bottom (25 rows) |
| `|CH` `|CE` | cursor to the start / end of the line |
| `|SC` / `|LC` | save / restore the cursor position |
| `|SA` | draw the STOCK screen here (a header shared by many screens), then carry on (new) |
| `|LF` | new line |
| `|**` | after the whole string, come back to this spot |
| `@NN` | move to column NN (two digits: `@40`) |
| `&NN` | move to row NN |
| `|ZA` ... `|ZB` | everything between is shown as it is, codes and all: for ASCII art full of `|`, `%` and `@` (2.0) |
| `|PO` `|PE` `|PD` | 2.20's pausing codes; screens don't pause here, so they do nothing (but aren't shown) |

What a caller typed (oneliners, chat, handles, file descriptions, message headers and so on) keeps its color codes but nothing else: no moves, clears, pauses, positions or `%XX` values.

## Waiting: `|XX`

| Code | Does |
|---|---|
| `|PA` | "press any key" prompt (the `HitAKey` string), then wait |
| `|PK` | wait for a key, silently (put your own "press enter" text before it) |
| `|PS` | the "paused" prompt, if the caller has pausing on |
| `|DM` `|DH` `|DE` `|DS` | wait 0.1 / 0.5 / 0.75 / 1 second |

## Values: `%XX`

Replaced by a value. An unknown `%XX` is shown as it is in prompts, and dropped in screens (as the original did) when both characters are letters.

| Caller | |
|---|---|
| `%UN` handle | `%RN` real name | `%LO` location | `%UO` user note |
| `%SL` security level | `%DS` download level | `%UA` access level | `%ID` user number |
| `%UC` total calls | `%CT` calls today | `%FC` first call | `%LC` last call |
| `%UP` posts | `%UE` email sent | `%UZ` post/call ratio | `%MW` mail waiting |
| `%UL` uploads | `%UK` upload kb | `%DL` downloads | `%DK` download kb |
| `%FP` file points | `%TL` minutes left | `%TT` minutes per day | `%UI` initials |
| `%TB` minutes in the time bank (2.20) | | | |
| `%BD` birth date | `%SX` gender | `%PN` phone | `%AD` address |
| `%HK` `%XM` `%PA` `%AQ` `%YN` | hot keys / expert / pausing / autoquote / yes-no bars (Yes/No) | `%PL` page length | `%EM` emulation |
| `%LD` `%LA` | theme name / author | `%RT` terminal type | |

| Board | |
|---|---|
| `%BN` board name | `%BL` location | `%BP` phone | `%SN` SysOp handle | `%SR` SysOp real name |
| `%TU` users | `%TC` calls | `%TP` posts | `%TE` email | `%TD` downloads |
| `%DC` calls today | `%DP` posts today | `%DD` downloads today | `%DU` uploads today | `%TZ` post/call ratio |
| `%DN` new users today (2.20) | | | | |
| `%ND` this node | `%IN` "Iniquity" | `%IV` version | `%VL` "Iniquity x (Rust)" | |
| `%DT` date (10/18/26) | `%DF` long date | `%TI` time (10:30pm) | `%TM` time (22:30) | (all in the caller's time zone) |

| Where the caller is | |
|---|---|
| `%MA` `%MI` `%MT` | message area name / number / message count |
| `%FA` `%FI` | file area name / number |
| `%CM` `%CF` | message / file conference |
| `%MN` | message number being read |
| `%FS` | new-files scan date |
| `%MP` | the menu's prompt name |
| `%IS` | the last thing typed at a menu input |

| The file base (new; over the areas the caller can see, worked out once a minute) | |
|---|---|
| `%FT` `%FE` `%FZ` `%FY` | files / file areas / their total size / new since the caller's scan date (before logon, what a new caller would see) |
| `%FN` `%FU` `%FW` `%FG` | the newest upload / who uploaded it / its area / how long ago ("today", "3d ago") |
| `%FM` `%FX` | the most downloaded file / its downloads |
| `%FB` `%FL` | the biggest file / its size |
| `%FK` `%FQ` | the top uploader / how many files |
| `%FR` `%FV` | a file at random / the first line of its description with words in it |

| The message base (new; public messages in the areas the caller can read, worked out once a minute) | |
|---|---|
| `%MG` `%MY` | messages / new for the caller (in their newscan areas) |
| `%MB` `%MF` `%MD` `%MK` | the newest post's subject / author / how long ago / area |
| `%MH` `%MR` | the most replied-to post / its replies |
| `%ML` `%MQ` | the loudest area of the last 30 days / its posts |
| `%MC` `%MX` | the top poster (not counting anonymous posts) / their posts |
| `%MV` `%MJ` | a message at random / a line of it with words in it |

## Fitting values: `{NN` and `}NN`

Put before a `%XX` code. `{20%UN` cuts the handle to 20 characters;
`}20%UN` pads or cuts it to exactly 20, so columns line up.

## Strings only

A prompt string (not a screen) can start with:

| Prefix | Does |
|---|---|
| `%%NAME` | show the screen `text/NAME` instead of text |
| `%!script args` | run a Lua script (see `SCRIPTING.md`) |
| `%|` / `%>` | center / right-align the line |

## Screen markers: `~XX` and `^NN`

Some screens mark where the board draws things, rather than showing a
value: the file lister and gallery (`~TL`, `~FN`...), the upload test
(`~L0`-`~L9`, `~PI`), lightbar menus (`~01^01`...), the user
configuration screen. Each is described with its screen in
`ARTWORK.md` and the README; copying the stock file and drawing around
its markers is the easy way.
