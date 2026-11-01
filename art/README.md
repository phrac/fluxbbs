# The stock screens and menus, and how they're made

The board's stock look is disflux's, the flux/bbs world HQ: TheDraw-font
logos, lightbar menus in boxes, headers that decode hidden messages as a
glint crosses them. The scripts here make it, for any board: its own
name goes in the logos.

- `../text/` and `../menus/` are the stock set, made for FLUX/BBS over
  the classic set below.
- `../classic/` is Iniquity's own screens and menus, as the original
  shipped them. A board that wants them back points its config there:

      [paths]
      stock_text = "/srv/inq/iniquity/classic/text"
      stock_menus = "/srv/inq/iniquity/classic/menus"

  (In the container; from a checkout, `../iniquity/classic/text` and so
  on.)

## Your board's name in the logos

    python3 art/make.py /path/to/board

makes every screen and lightbar menu again for the board in that
directory: its `[bbs] name` (or `short_name`, or `super_short_name`, when
a font is too wide for the name or hasn't one of its letters) in the
logos, written into its `text/` and `menus/`, where they take the place
of the stock ones. Run it again after renaming the board (`*C`, I and K).
It needs Python 3.11 and nothing else; in the container, run it on the
host, against the board directory the container mounts.

Without a directory, it remakes the stock set here.

## Your board's own touches: `art/local.py`

A board can change what the stock set says without touching these
scripts: a file `art/local.py` in the board's directory, setting any of
these. disflux's is an example of all of them.

| Name | What it does |
|---|---|
| `MENUS` | a menu's title and boxes: `{'door': ('online games', [('popular doors', [('1', 'lord'), ...]), ...])}` |
| `PARAMS`, `COMMANDS`, `CHANGED`, `ADDED` | a menu option's parameter, command, every setting, or a new option: `{('door', '5'): {'command': 'D4', 'param': '...', 'desc': 'Usurper', 'help': 'Usurper'}}` |
| `HIDDEN_ADDED`, `DROP_REST`, `KEEP_HIDDEN`, `STOCK_FROM`, `GRIDS` | as in `mainmenu-gen.py` |
| `LABELS` | a single menu entry's new name: `{('main', 'j'): 'chat w/ Penny'}` |
| `PICTURES` | menus laid over a picture: the art (in the board's `art/`), its dark backdrop recolored, the menu's keys in boxes on the open side; see `build_picture` in `mainmenu-gen.py` |
| `AICHAT` | the AI chat screen's title, tagline, accent and logo colors, and with `face` part of a picture in its top left corner; see `aichat-gen.py` |
| `LOGOS` | a menu's logo: a piece of art in the board's `art/` (`{'main': ('MYLOGO.ans', '35')}`) or a TheDraw font (`('tdf', file, face, word, column, options)`) |
| `PRELOGON_LINES` | the lines under the prelogon's logo |
| `TELNET_SSH` | `host:port` the TELNET screen tells telnet callers to call back on |
| `ABOUT` | the about screen (h on the main menu) |
| `WELCOME_ART` | a piece of art in the board's `art/` to show first among the welcome screen's logos |
| `WELCOME_WHATS`, `WELCOME_ARCHIVE` | the welcome's line of what's here, and what the file archive is |
| `KEEP` | screens `make.py` leaves alone (a hand-drawn `STOCK.ANS`, say) |
| `THEMES` | screen themes the board makes: `{'penny': {'AICHAT': {...}, 'PICTURES': {...}}}`; `make.py` writes what each one's settings change to `themes/NAME/` and its menus to `themes/NAME/menus/`, and the rest comes from the board's own |

Art a script reads (`THREADS.ans`, `LOGON.ans`...) comes from the board's
`art/` first, so a board can draw its own: a `LOGON.ans` there is the
matrix screen's logo instead of its name in a font.

## The scripts

Each makes one screen or a few (its comment at the top says which), run
from a board's `text/`; `make.py` runs them all. `boardname.py` finds the
board (`FLUX_BOARD`) and its names, `phrases.py` holds the hidden
messages the headers decode, `fonts/` the TheDraw fonts. They're
disflux's own scripts, moved here; change them and run `make.py` for the
stock set (and for your board) again.

`ad-gen.py` isn't a board screen and `make.py` doesn't run it: it makes
the ads for flux/bbs and disflux (16 colors, CRLF, SAUCE), for ad
rotations, file echoes and other boards. They're kept in `ads/`, one
file per revision (`FLUXAD02.ANS`, `FLUXAD03.ANS`...): a changed ad is
the next number, never written over an old one.

The prompts that go with them (AI chat's `Ai*`, node chat's `MNC*`) are
in the stock `strings.toml`: `agent-gen.py strings` and `nodechat-gen.py
strings` print them, to paste in after changing their colors.
