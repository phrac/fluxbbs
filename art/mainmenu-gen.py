"""A board's menu screens and lightbar menus, made from the classic menus.

Run from text/:

    python3 -I ../art/mainmenu-gen.py NAME [N] > NAME.ANS

NAME is one of MENUS below, doorstat (the doors menu's scoreboard
screen, DOORSTAT.ANS and its .FX) or stock (STOCK.ANS, the header |SA
draws). For main, N (0-8) picks the variant: MAIN.ANS
is 0, MAIN.AN1..AN8 are 1..8, and the board shows one at random. It also
writes ../menus/NAME.toml (the lightbar menu, from the stock one), and
for tarot TAROTHDR.ANS. Change this script and run it again; don't edit
what it writes.
"""
import json
import os
import random
import re
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import boardname  # noqa: E402
MENUDIR = boardname.CLASSIC_MENUS

# ---- what's on each menu ---------------------------------------------------

# name: (title, [(box title, [(key, label)])])
MENUS = {
 'main': ('main menu', [('talk', [('m', 'message boards'), ('e', 'email'), ('o', 'oneliners'), ('a', 'automsg'), ('j', 'ai chat'), ('w', "who's on"), ('x', 'nodes'), ('p', 'page sysop'), ('n', 'feedback')]),
       ('files & fun', [('f', 'files'), ('r', 'art gallery'), ('d', 'doors'), ('k', 'points & lottery'), ('b', 'bbs list'), ('@', 'weather'), ('v', 'voting booth'), ('i', 'infoforms'), ('h', 'about flux/bbs')]),
       ('you & the board', [('c', 'config'), ('s', 'autosig'), ('+', 'password'), ('y', 'your stats'), ('l', 'last callers'), ('u', 'users'), ('z', 'history'), ('>', 'more...'), ('g', 'goodbye')])]),
 # The main menu's overflow: what's used least. Their keys still work
 # from the main menu too (hidden there).
 'more': ('more', [('more', [('q', 'qwk mail'), ('t', 'time bank'), ('%', 'new user voting'), ('<', 'back')])]),
 'file': ('file menu', [('areas', [('a', 'change area'), ('j', 'join conference'), ('[', 'previous area'), (']', 'next area'), ('#', 'jump to area #'), ('s', 'set newscan date'), ('e', 'scan select')]),
       ('find', [('l', 'browse files'), ('n', 'newscan'), ('f', 'filespec search'), ('t', 'text search'), ('i', 'file information'), ('v', 'view archive')]),
       ('transfer', [('d', 'download'), ('u', 'upload'), ('+', 'flag for batch'), ('b', 'batch edit'), ('q', 'quit')])]),
 'message': ('message menu', [('read & post', [('r', 'read messages'), ('n', 'new messages'), ('p', 'post message'), ('b', 'text search'), ('h', 'how it works')]),
       ('areas', [('a', 'change area'), ('l', 'list areas'), ('j', 'join conference'), ('#', 'jump to area #')]),
       ('more', [('[', 'previous area'), (']', 'next area'), ('s', 'set newscan date'), ('e', 'scan select'), ('q', 'quit')])]),
 'sysop': ('sysop menu', [('users', [('u', 'user editor'), ('l', 'access levels'), ('i', 'infoform editor'), ('p', 'new user questions'), ('g', 'security log'), ('r', 'read email'), ('h', 'stat editor'), ('b', 'bbs list editor'), ('n', 'caller notes'), ('q', 'quit')]),
       ('areas & files', [('a', 'msg area editor'), ('@', 'msg conferences'), ('-', 'message trash'), ('f', 'file area editor'), ('!', 'file conferences'), ('s', 'file area sort'), ('y', 'file editor'), ('k', 'pack file areas'), ('%', 'local upload'), ('>', 'download any file')]),
       ('system', [('c', 'system config'), ('o', 'logon sequence'), ('#', 'menu editor'), ('e', 'string editor'), ('t', 'text libraries'), ('w', 'screen viewer'), ('j', 'tdf font viewer'), ('v', 'event editor'), ('x', 'protocol editor'), ('z', 'archiver config'), ('d', 'flux/bbs microdos'), ('m', 'modem setup')])]),
 'door': ('online games', [('popular doors', [('1', 'lord'), ('2', 'bre'), ('3', 'usurper'), ('4', 'tradewars 2002'), ('l', 'bbslink games')]),
          ('built in', [('t', 'tarot & fortunes'), ('c', 'casino'), ('v', 'carrier lost')]),
          ('more doors', [('5', 'yankees & rednecks'), ('6', 'bbs hacker'), ('7', 'lore'), ('8', 'steel & ice'), ('9', 'mechwars')])]),
 'bbslink': ('bbslink games', [('bbslink.net', [('m', 'every bbslink game'), ('1', 'lord'), ('2', 'lord ii'), ('3', 'planets: teos'), ('4', 'overkill ii'), ('5', 'tradewars 2002'), ('6', 'barren realms elite'), ('q', 'back to doors')])]),
 'tarot': ('tarot & fortunes', [('the cards', [('d', 'card of the day'), ('t', 'three cards'), ('f', 'a fortune'), ('q', 'quit')])]),
 'email': ('email menu', [('email', [('r', 'read your email'), ('e', 'enter email'), ('q', 'quit')])]),
 'automsg': ('automessage menu', [('automessage', [('s', 'show automessage'), ('w', 'write automessage'), ('r', 'reply to its author'), ('q', 'quit')])]),
 'autosig': ('autosignature menu', [('autosig', [('v', 'show your autosig'), ('e', 'edit autosignature'), ('u', 'upload autosig'), ('d', 'delete autosig'), ('q', 'quit')])]),
 'bbslist': ('bbs list menu', [('bbs list', [('l', 'display bbs list'), ('s', 'show a bbs'), ('a', 'add a bbs to the list'), ('q', 'quit')])]),
 'multinod': ('multinode', [('nodes', [('w', "who's online"), ('m', 'send a message'), ('c', 'multinode chat'), ('q', 'quit')])]),
 'qwkmail': ('offline mail menu', [('packets', [('d', 'download qwk packet'), ('u', 'upload rep packet'), ('q', 'quit')]),
       ('setup', [('j', 'join msg conference'), ('s', 'set newscan date')])]),
 'voting': ('voting booth', [('vote', [('v', 'vote on new questions'), ('o', 'answer a question'), ('r', 'see results'), ('a', 'add a question'), ('q', 'quit')])]),
 'timebank': ('time bank', [('time', [('d', 'deposit time'), ('w', 'withdraw time'), ('s', 'stats'), ('q', 'quit')]),
       ('kilobytes', [('/d', 'deposit kb'), ('/w', 'withdraw kb')])]),
}

# Menus with their own logo, and each menu's accent color (ANSI 31 red,
# 32 green, 35 magenta, 36 cyan); the rest get the STOCK header (|SA).
# A logo can also be a word set in a TheDraw font from art/fonts/:
# ('tdf', file, face, word, column (0 centers it)[, options]): options are
# 'spacing' (columns between letters; the face's own if left out) and
# 'recolor' ({from: to} DOS colors, e.g. a silver face in cyan) or 'fade'
# ((top rgb, bottom rgb): the face's white and gray become that 24-bit fade
# down the rows, the gray darker). A space in the word is a two-column gap.
LOGOS = {'main': (('tdf', 'AMNESIAX.TDF', 'Amnesia Purp', boardname.NAME, 0), '35'), 'more': (('tdf', 'AMNESIAX.TDF', 'Amnesia Purp', boardname.NAME, 0), '35'), 'file': (('tdf', 'GRNSHT.TDF', 'GReeNSHiT!', boardname.NAME, 0), '32'), 'message': (('tdf', 'METALIX2.TDF', 'Metalix2', boardname.NAME, 0, {'spacing': 2, 'fade': ((200, 200, 200), (85, 255, 255))}), '36'),
         'door': (('tdf', 'DAMNEDX.TDF', 'Damned Red', boardname.NAME, 0, {'spacing': 0}), '31'),
         'tarot': (None, '35')}

# Where this board's menus differ from stock: parameters and commands
# the stock menu lacks, options changed outright, options added, and the
# menus whose other stock options go rather than stay as hidden keys.
PARAMS = {}
COMMANDS = {}
CHANGED = {
    # m goes straight to the boards (MH), not through SCANMSG.
    ('main', 'm'): {'command': 'MH', 'param': '', 'desc': 'Message boards', 'help': 'The message boards: read, post, vote and comment'},
    # c is the casino, not the classic menu's KOMBAT.BAT.
    ('door', 'c'): {'command': 'AC', 'param': '', 'desc': 'Casino', 'help': 'The casino: slots, blackjack, roulette, craps, the bar and the players club'},
}
ADDED = {('message', 'h'): {'keys': 'H', 'command': '-F', 'param': 'MSGGUIDE', 'desc': 'How it works', 'help': 'The keys for reading, posting, votes and comments'},
         # The about screen (art/about-gen.py).
         ('main', 'h'): {'keys': 'H', 'command': '-F', 'param': 'ABOUT', 'desc': 'About flux/bbs', 'help': 'What flux/bbs is and what it does'},
         # Points around the board (!P): the ledger, tips, the lottery.
         ('main', 'k'): {'keys': 'K', 'command': '!P', 'param': '', 'desc': 'Points & lottery', 'help': 'Your file points, tips, and the nightly lottery'},
         # The overflow menu, and the way back from it.
         ('main', '>'): {'keys': '>', 'command': '-/', 'param': 'MORE', 'desc': 'More', 'help': 'Offline mail (qwk), new user voting, and what else is used least'},
         ('more', '<'): {'keys': '<', 'command': '-\\', 'param': 'MAIN', 'desc': 'Back', 'help': 'Back to the main menu'},
         # The weather, from the doors menu.
         ('main', '@'): {'keys': '@', 'command': 'WX', 'param': '', 'desc': 'Weather', 'help': 'The weather where you are: the sky now, the next day and the week'},
}
# Options a menu gains off the bar (their keys work; the screen says so):
# the doors menu's s, its full scoreboard.
HIDDEN_ADDED = {'door': [{'keys': 'S', 'command': '-F', 'param': 'DOORSTAT', 'desc': 'Scoreboard', 'help': 'Who played what, when and for how long', 'hidden': True}]}
DROP_REST = {'door', 'more'}
# Menus with no stock menu of their own: whose options they take, and
# their own title, prompt and screen.
STOCK_FROM = {'more': ('main', {'title': 'More', 'prompt_name': 'More', 'help_file': 'MORE'})}
# Keys those menus keep, hidden, though they're not in a box (the doors
# menu's q, so its box fits above the help line).
KEEP_HIDDEN = {('door', 'q')}

# The file and message menus' grids under the boxes, three cells a row:
# (label, keys, command, parameter, what's in it, help line). Each
# cell's text uses its own {NN limits: a limit lasts until the next one.
GRIDS = {
 'file': [('newest', '!NEW', 'FW', 'newest', '{12%FN » {08%FG', '|10newest upload|02: |15{14%FN |07by |15{10%FU |07in |15{14%FW'),
          ('hot', '!HOT', 'FW', 'hot', '{12%FM » {05%FX dl', '|10most downloaded|02: |15{14%FM|07, |15%FX |07downloads'),
          ('heavy', '!BIG', 'FW', 'heavy', '{12%FB » {09%FL', '|10biggest file|02: |15{14%FB|07, |15%FL'),
          ('dig', '!DIG', 'FW', 'dig', '{12%FR » {08%FV', '|10dig this up|02: |07{44%FV'),
          ('kingpin', '!KING', 'FW', 'kingpin', '{12%FK » {06%FQ', '|10top uploader|02: |15{12%FK|07, |15%FQ |07files. their latest:'),
          ('vault', '!VAULT', 'FN', '', '{06%FT » {06%FZ » !{04%FY new', '|10scan for the |15%FY |10new files since your last scan')],
 'message': [('newest', '!NEW', 'MX', 'newest', '{12%MB » {08%MD', '|11newest post|03: |15{20%MB |07by |15{10%MF |07in |15{12%MK'),
             ('hot', '!HOT', 'MX', 'hot', '{13%MH » {03%MR re', '|11most replies|03: |15{24%MH|07, |15%MR |07replies'),
             ('loudest', '!LOUD', 'MX', 'loud', '{13%ML » {05%MQ', '|11loudest area this month|03: |15{20%ML|07, |15%MQ |07posts'),
             ('dig', '!DIG', 'MX', 'dig', '{12%MV » {08%MJ', '|11dig this up|03: |07{44%MJ'),
             ('big mouth', '!MOUTH', 'MX', 'mouth', '{12%MC » {06%MX', '|11big mouth|03: |15{12%MC|07, |15%MX |07posts. their latest:'),
             ('unread', '!UNREAD', 'MX', 'unread', '!{05%MY new » {06%MG all', '|11read the |15%MY |11new messages, a thread at a time')],
}

# The board's own (its art/local.py): its doors, its menus' boxes, its
# grids and logos, added to these or put in their place.
for _d, _n in ((MENUS, 'MENUS'), (LOGOS, 'LOGOS'), (PARAMS, 'PARAMS'), (COMMANDS, 'COMMANDS'), (CHANGED, 'CHANGED'),
               (ADDED, 'ADDED'), (HIDDEN_ADDED, 'HIDDEN_ADDED'), (STOCK_FROM, 'STOCK_FROM'), (GRIDS, 'GRIDS')):
    _d.update(boardname.local(_n, {}))
# Its new names for single entries (LABELS): {(menu, key): label}.
for (_m, _k), _label in boardname.local('LABELS', {}).items():
    _title, _boxes = MENUS[_m]
    MENUS[_m] = (_title, [(_bt, [(i[0], _label, *i[2:]) if i[0] == _k else i for i in _items]) for _bt, _items in _boxes])
DROP_REST |= boardname.local('DROP_REST', set())
# Menus a board lays over a picture of its own (see build_picture): {name:
# {'art', 'color', 'columns': [(box title, [keys])], 'font', ...}}.
PICTURES = boardname.local('PICTURES', {})
KEEP_HIDDEN |= boardname.local('KEEP_HIDDEN', set())

# The main menu's variants: a whisper under the boxes, and a phrase
# hidden in the row of noise over the logo.
WHISPERS = ['the sysop can see what you type. say hi.',
            'nothing here is indexed. keep it that way.',
            'every keystroke echoes somewhere.',
            'you are never the only one on this line.',
            'you left a trail getting in. we cleaned it up.',
            'the logs are kept forever. probably.',
            "don't trust the last caller.",
            "if the screen flickers, don't worry about it.",
            'this call is not being recorded. (it is.)']
HIDDEN = ['NOBODY LEAVES', 'STAY QUIET', 'TRUST NO ONE', 'WE SEE YOU', 'KEEP DIGGING',
          'LOOK CLOSER', 'NO WAY OUT', 'ALMOST THERE', 'READ BETWEEN']
# What the headers decode into as the glint passes: the lists in
# phrases.py, one an area (about a hundred each).
sys.path.insert(0, HERE)
from phrases import MAIN as MESSAGES, WORDS, BOARDS as BOARD_WORDS, FILES as FILE_WORDS, DOORS as DOOR_WORDS

# Animation while a menu waits (fluxbbs [[effect]]): (kind, over what,
# seconds between, seconds before the first, and any more settings).
# The main menu: the noise row and the word down the side decode into
# their next messages as a glint crosses the logo, then a pulse runs
# along the rule and the boxes' lines.
EFFECTS = {'main': [('decode', 'noise', 10.0, 2.0, {'amount': 1.8, 'messages': MESSAGES}),
                    ('decode', 'inside', 10.0, 2.0, {'amount': 1.8, 'messages': WORDS}),
                    ('random', 'logo', 10.0, 2.0, {}), ('ripple', 'boxes', 10.0, 6.0, {})],
           'file': [('decode', 'side_left', 10.0, 2.0, {'amount': 1.8, 'messages': FILE_WORDS}),
                    ('decode', 'side_right', 10.0, 2.0, {'amount': 1.8, 'messages': FILE_WORDS[::-1]}),
                    ('random', 'logo', 10.0, 2.0, {}), ('ripple', 'boxes', 10.0, 6.0, {})],
           'door': [('decode', 'side_left', 10.0, 2.0, {'amount': 1.8, 'messages': DOOR_WORDS}),
                    ('decode', 'side_right', 10.0, 2.0, {'amount': 1.8, 'messages': DOOR_WORDS[::-1]}),
                    ('random', 'logo', 10.0, 2.0, {}), ('ripple', 'boxes', 10.0, 6.0, {})],
           'message': [('decode', 'codes_left', 10.0, 2.0, {'amount': 1.8, 'messages': BOARD_WORDS}),
                       ('decode', 'codes_right', 10.0, 2.0, {'amount': 1.8, 'messages': BOARD_WORDS[::-1]}),
                       ('random', 'logo', 10.0, 2.0, {}), ('ripple', 'boxes', 10.0, 6.0, {})]}

# ---- colors ----------------------------------------------------------------

E = '\x1b['
DG = E + '0;1;30m'
WH = E + '0;1;37m'
GR = E + '0;37m'
# Each accent's bright and dark colors, for 24-bit fades.
RGB = {'31': ((255, 85, 85), (170, 0, 0)), '32': ((85, 255, 85), (0, 170, 0)), '34': ((85, 85, 255), (0, 0, 170)), '35': ((255, 85, 255), (170, 0, 170)), '36': ((85, 255, 255), (0, 170, 170))}


def goto(r, c):
    return E + f'{r};{c}H'


class Ink:
    """A menu's accent: RD and BR its normal and bright codes, HI and HIW
    the lightbar's highlight, and the rules and fades drawn in it."""

    def __init__(self, accent):
        self.accent = accent
        self.RD = E + '0;' + accent + 'm'
        self.BR = E + '0;1;' + accent + 'm'
        bg = '4' + accent[1]
        self.HI = E + '0;30;' + bg + 'm'
        self.HIW = E + '0;1;37;' + bg + 'm'

    def fade_text(self, t):
        """Text fading from white through the bright accent to the dark."""
        stops = [(255, 255, 255)] + list(RGB[self.accent])
        out = ''
        for i, ch in enumerate(t):
            x = i / max(1, len(t) - 1) * 2
            a, b = (stops[0], stops[1]) if x <= 1 else (stops[1], stops[2])
            f = x if x <= 1 else x - 1
            r, g, bl = (round(a[j] + (b[j] - a[j]) * f) for j in range(3))
            out += E + f'38;2;{r};{g};{bl}m' + ch if ch != ' ' else ' '
        return out

    def hline(self, n):
        """A rule, bright at its ends, the accent near them, dark between."""
        def c(i):
            d = min(i, n - 1 - i)
            return self.BR if d == 0 else self.RD if d < 3 else DG
        return ''.join(c(i) + '─' for i in range(n))

    def fade(self, n, rev):
        """A rule fading in from dark to bright (or out, `rev`)."""
        cs = [DG if i < n - 6 else self.RD if i < n - 1 else self.BR for i in range(n)]
        if rev:
            cs = cs[::-1]
        return ''.join(c + '─' for c in cs)

    def box_top(self, t, w):
        head = self.BR + '─' + DG + '[' + ' ' + self.fade_text(t) + ' ' + DG + ']'
        return self.RD + '┌' + head + self.hline(w - 2 - (1 + len(t) + 4)) + self.RD + '┐'

    def box_divider(self, t, w):
        head = self.BR + '─' + DG + '[' + ' ' + self.fade_text(t) + ' ' + DG + ']'
        return self.RD + '├' + head + self.hline(w - 2 - (1 + len(t) + 4)) + self.RD + '┤'

    def box_bottom(self, w):
        return self.RD + '└' + self.hline(w - 2) + self.RD + '┘'


def cells_line(row):
    """A row of (character, color) cells as ANSI, trailing spaces cut."""
    s = ''
    cur = None
    last = max([i for i, (ch, _) in enumerate(row) if ch != ' '], default=-1)
    for ch, col in row[:last + 1]:
        if ch == ' ':
            s += ' '
            continue
        if col != cur:
            s += col
            cur = col
        s += ch
    return s

# ---- the screen's parts ----------------------------------------------------


def read_tdf(path):
    """The fonts in a TheDraw .TDF file: name, type, spacing and glyphs
    (rows of (char, attr))."""
    import struct
    d = open(path, 'rb').read()
    fonts, p = [], 20
    while p + 213 <= len(d) and d[p:p + 4] == b'\x55\xaa\x00\xff':
        name = d[p + 5:p + 5 + min(d[p + 4], 12)].decode('cp437', 'replace').rstrip('\x00 ')
        ftype, spacing = d[p + 21], d[p + 22]
        size = struct.unpack_from('<H', d, p + 23)[0]
        offs = struct.unpack_from('<94H', d, p + 25)
        data = d[p + 213:p + 213 + size]
        glyphs = {}
        for i, o in enumerate(offs):
            if o == 0xFFFF or o + 2 > len(data):
                continue
            w, q, rows, row = data[o], o + 2, [], []
            while q < len(data) and data[q] != 0:
                if data[q] == 0x0D:
                    rows.append(row); row = []; q += 1
                elif ftype == 2:
                    row.append((data[q], data[q + 1])); q += 2
                else:
                    row.append((data[q], 7)); q += 1
            if row:
                rows.append(row)
            glyphs[chr(33 + i)] = (w, rows)
        fonts.append({'name': name, 'type': ftype, 'spacing': spacing, 'glyphs': glyphs})
        p += 213 + size
    return fonts


def tdf_logo(file, face, word, col, opts=None):
    """A word set in a TheDraw font, as rows of ANSI ending in newlines
    (so the rows count as a screen's), starting at the column."""
    opts = opts or {}
    font = next(f for f in read_tdf(os.path.join(HERE, 'fonts', file)) if f['name'] == face)
    spacing = opts.get('spacing', font['spacing'])
    recolor = opts.get('recolor', {})
    # A letter the font hasn't (FLUX/BBS's slash): the board's next name,
    # if it has one, else the word without it.
    names = boardname.NAMES
    if any(ch != ' ' and ch not in font['glyphs'] for ch in word):
        if word in names and names.index(word) + 1 < len(names):
            return tdf_logo(file, face, names[names.index(word) + 1], col, opts)
        word = ''.join(ch for ch in word if ch == ' ' or ch in font['glyphs'])
    glyphs = [font['glyphs'][ch] if ch != ' ' else (2, []) for ch in word]
    h = max(len(rows) for _, rows in glyphs)
    ansi = [0, 4, 2, 6, 1, 5, 3, 7]
    # The word as a grid of cells first, to find what actually shows.
    grid = []
    for y in range(h):
        cells = []
        for gi, (w, rows) in enumerate(glyphs):
            row = rows[y] if y < len(rows) else []
            cells += (list(row) + [(32, 0)] * (w - len(row)))[:w] + [(32, 0)] * (spacing if gi < len(glyphs) - 1 else 0)
        cells = [(ch, recolor.get(a & 15, a & 15) | (recolor.get((a >> 4) & 7, (a >> 4) & 7) << 4)) for ch, a in cells]
        grid.append(cells)
    fade = opts.get('fade')
    def drawn(c):
        return c[0] not in (0, 32) or (c[1] >> 4) & 7
    grid = [r for r in grid if any(drawn(c) for c in r)]          # no blank rows over or under it
    left = min(next(i for i, c in enumerate(r) if drawn(c)) for r in grid if any(drawn(c) for c in r))
    right = max(max(i for i, c in enumerate(r) if drawn(c)) for r in grid)
    # Too wide for the screen: the board's next shorter name, if it has one.
    names = boardname.NAMES
    if right - left + 1 > 80 - (col - 1 if col else 0) and word in names and names.index(word) + 1 < len(names):
        return tdf_logo(file, face, names[names.index(word) + 1], col, opts)
    col = col if col else (80 - (right - left + 1)) // 2 + 1  # centered on what shows
    out = b''
    for y, cells in enumerate(grid):
        line = bytearray(b' ' * (col - 1))
        cur = None
        for ch, a in cells[left:right + 1]:
            fg, bg = a & 15, (a >> 4) & 7
            if fade:
                t = y / max(1, len(grid) - 1)
                c = tuple(round(fade[0][i] + (fade[1][i] - fade[0][i]) * t) for i in range(3))
                tone = {15: c, 7: tuple(round(v * 0.62) for v in c)}
                fgc = tone.get(fg, (0, 0, 0) if fg == 0 else tuple(RGB.get(str(ansi[fg & 7] + 30), ((170, 170, 170), (85, 85, 85)))[0]))
                bgc = tuple(round(v * 0.4) for v in c) if bg == 7 else (0, 0, 0)
                sgr = ('\x1b[38;2;%d;%d;%d;48;2;%d;%d;%dm' % (fgc + bgc)).encode()
                key = (fgc, bgc)
            else:
                sgr = ('\x1b[0;%s3%d;4%dm' % ('1;' if fg > 7 else '', ansi[fg & 7], ansi[bg])).encode()
                key = (fg, bg)
            if key != cur:
                line += sgr
                cur = key
            line.append(ch if ch else 32)
        out += bytes(line) + b'\x1b[0m\r\n'
    return out


def logo_head(name, logo):
    """What goes over the menu: its logo, or the STOCK header."""
    if name == 'tarot':
        return tarot_logo()
    if not logo:
        return b'\x1b[0m\x1b[2J|SA'
    if isinstance(logo, tuple) and logo[0] == 'tdf':
        # Home after the clear: a terminal's ESC[2J may leave the cursor.
        # The main menu's logo sits under its row of noise, as a drawn
        # one does: two rows down.
        top = b'\r\n\r\n' if name in ('main', 'more') else b''
        return b'\x1b[0m\x1b[2J\x1b[H' + top + tdf_logo(*logo[1:]) + b'\x1b[0m'
    d = open(boardname.art(logo), 'rb').read()
    art = d[:d.index(b'\x1a')]
    # The file menu needs every row: no blank ones over its logo.
    while name == 'file':
        line, nl, rest = art.partition(b'\n')
        if not nl or re.sub(rb'\x1b\[[0-9;]*[A-Za-z]|\r', b'', line):
            break
        art = rest
    return b'\x1b[0m' + art + b'\x1b[0m\r\n'


def tarot_logo():
    """TAROT in block letters, lit from above, with a shadow, a crescent
    moon and stars (7 rows)."""
    ink = Ink('35')
    font = {'T': ['██████', '  ██  ', '  ██  ', '  ██  ', '  ██  '],
            'A': [' ████ ', '██  ██', '██████', '██  ██', '██  ██'],
            'R': ['█████ ', '██  ██', '█████ ', '██ ██ ', '██  ██'],
            'O': [' ████ ', '██  ██', '██  ██', '██  ██', ' ████ ']}
    g = [[(' ', '')] * 80 for _ in range(7)]
    shades = [E + '0;1;37m', E + '0;1;35m', E + '0;1;35m', E + '0;35m', E + '0;35m']
    pix = {(r + 1, 21 + i * 8 + c) for i, ch in enumerate('TAROT') for r, row in enumerate(font[ch]) for c, p in enumerate(row) if p != ' '}
    for r, c in pix:
        g[r][c] = ('█', shades[r - 1])
    for r, c in pix:
        if r + 1 < 7 and (r + 1, c + 1) not in pix:
            g[r + 1][c + 1] = ('░', DG)
    for r, row in enumerate(['  ▄██▀', '▐██▀  ', '██▌   ', '▐██▄  ', '  ▀██▄']):
        for c, p in enumerate(row):
            if p != ' ':
                g[r + 1][7 + c] = (p, E + ('0;1;37m' if r in (0, 4) else '0;37m'))
    stars = [(0, 3, '·', DG), (0, 15, '*', ink.RD), (0, 40, '·', DG), (0, 66, '+', ink.BR), (1, 62, '·', GR), (2, 70, '*', WH), (3, 64, '·', DG),
             (4, 75, '·', ink.RD), (5, 61, '+', DG), (5, 3, '·', DG), (6, 16, '·', ink.RD), (3, 2, '*', DG), (6, 72, '*', ink.RD), (1, 77, '·', DG)]
    for r, c, ch, col in stars:
        if g[r][c][0] == ' ':
            g[r][c] = (ch, col)
    return ('\x1b[0m\x1b[2J' + '\x1b[0m\r\n'.join(cells_line(r) for r in g) + '\x1b[0m\r\n').encode('cp437')


def main_head_extras(variant, ink):
    """Over the main menu's logo: INSIDE down its left, and a row of noise
    with a phrase lit in it, one phrase a variant."""
    side = ''.join(goto(3 + i, 4) + (ink.BR if i % 2 == 0 else ink.RD) + ch for i, ch in enumerate('INSIDE'))
    rnd = random.Random(1994 + variant)
    phrase = HIDDEN[variant].replace(' ', '')
    cells = [(g, i) for g in range(15) for i in range(4)]
    lit = {cells[p]: phrase[k] for k, p in enumerate(sorted(rnd.sample(range(len(cells)), len(phrase))))}
    noise = ''
    for g in range(15):
        for i in range(4):
            noise += (ink.BR + lit[(g, i)]) if (g, i) in lit else (DG + rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789'))
        if g < 14:
            noise += ' '
    return side + goto(2, 4) + noise


def message_head_extras(ink):
    """Either side of the message menu's logo: columns of code groups in
    its cyan, a letter lit here and there (the board decodes phrases
    into them)."""
    rnd = random.Random(2026)
    out = ''
    for col in (3, 69):
        for r in range(1, 10):
            out += goto(r, col)
            for g in range(2):
                for i in range(4):
                    ch = rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789')
                    out += (ink.BR if rnd.random() < 0.12 else ink.RD) + ch
                if g == 0:
                    out += ' '
    return out


def file_head_extras(ink, rows, words=FILE_WORDS):
    """Down each margin of the file menu's logo (and the doors menus'): a
    word, letters lit and dim by turns (the board decodes others into
    them)."""
    out = ''
    for col, word in ((2, words[0]), (79, words[1])):
        for i, ch in enumerate(word[:rows]):
            out += goto(1 + i, col) + (ink.BR if i % 2 == 0 else ink.RD) + ch
    return out


def item_text(ink, w, k, label, hi):
    """An option as the lightbar draws it, highlighted or not."""
    pad = ' ' * (w - 2 - (4 + len(k) + len(label)))
    if hi:
        return ink.HI + ' (' + ink.HIW + k + ink.HI + ') ' + ink.HIW + label + pad + E + '0m'
    return ' ' + DG + '(' + ink.BR + k + DG + ') ' + ink.fade_text(label) + pad + E + '0m'


def grid_rows(name, ink, left, first_marker):
    """The file and message menus' grid: its rows on screen, and its cells
    as menu commands."""
    grid = GRIDS[name]
    cw = [24, 24, 25]
    xs = [3, 28, 53]

    def top(ts, first, last):
        s = ink.RD + first
        for i, (t, w) in enumerate(zip(ts, cw)):
            s += ink.BR + '─' + DG + '[ ' + ink.fade_text(t) + ' ' + DG + ']' + ink.hline(w - len(t) - 5)
            s += ink.RD + (last if i == 2 else ('┬' if first == '┌' else '┼'))
        return s

    def markers(row):
        s = ''
        for i in range(3):
            m = first_marker + row * 3 + i + 1
            s += DG + '│' + f'~{m:02d}^{m:02d}' + ' ' * cw[i]
        return left + s + DG + '│'

    rows = [left + top([g[0] for g in grid[:3]], '┌', '┐'), markers(0),
            left + top([g[0] for g in grid[3:]], '├', '┤'), markers(1),
            left + ink.RD + '└' + DG + '─' * cw[0] + ink.RD + '┴' + DG + '─' * cw[1] + ink.RD + '┴' + DG + '─' * cw[2] + ink.RD + '┘']

    # A cell: its background painted first, then what's in it, so short
    # values still fill it. The first value is bright, the rest plain; a
    # ! before a value lights it in the accent.
    def cell(i, hi):
        body = grid[i][4]
        x, w = xs[i % 3], cw[i % 3]
        txt = ''
        for j, part in enumerate(body.split(' » ')):
            if j:
                txt += (ink.HI + ' » ') if hi else (' ' + DG + '» ')
            c = ink.HIW if hi else (WH if j == 0 else GR)
            if part.startswith('!'):
                part = part[1:]
                c = ink.HIW if hi else ink.BR
                # The word after the value goes back to plain.
                v, _, word = part.partition(' ')
                part = v + (((ink.HIW if hi else GR) + ' ' + word) if word else '')
            txt += c + part
        return (ink.HI if hi else E + '0m') + ' ' * w + f'@{x:02d}' + ' ' + txt + E + '0m'

    cmds = []
    for i, (label, keys, command, param, _, help_) in enumerate(grid):
        c = {'keys': keys, 'command': command, 'desc': label, 'help': help_, 'on': cell(i, True), 'off': cell(i, False)}
        if param:
            c['param'] = param
        cmds.append(c)
    return rows, cmds


def prow(ink, label, body, lw=9):
    """A scoreboard row: the label in the accent, then values (cut with
    {NN, then plain words), ` · ` between them."""
    txt = ''
    for j, part in enumerate(body.split(' · ')):
        if j:
            txt += DG + ' · '
        v, _, word = part.partition(' ')
        txt += (WH if j == 0 else GR) + v + (GR + ' ' + word if word else '')
    return ink.BR + f'{label:<{lw}}' + ' ' + txt + E + '0m'


def door_strip(ink, left, w):
    """Under the doors menu's boxes, the scoreboard in two rows; s shows
    the rest. Each row's right border is drawn at its column, whatever
    the values."""
    right = len(left) + w
    def seg(label, value, tail=''):
        return ink.BR + label + ' ' + WH + value + (GR + tail if tail else '')
    dot = DG + ' · '
    rows = [seg('on now', '{16%GO') + dot + seg('last', '{12%GL', ' {10%GW {08%GA'),
            seg('hottest', '{12%GP') + dot + seg('untried', '{14%GE') + dot + seg('you', '{04%GY', ' plays')]
    hint = 's the whole scoreboard · q back to main'
    out = [left + ink.box_top('lately', w)]
    out += [left + DG + '│ ' + r + E + '0m' + f'@{right:02d}' + DG + '│' for r in rows]
    out.append(left + ink.RD + '└' + ink.hline(w - 2 - len(hint) - 5) + DG + '[ ' + GR + hint + DG + ' ]' + ink.BR + '─' + ink.RD + '┘')
    return out


def door_scoreboard(ink, left, w):
    """DOORSTAT (the doors menu's s): who played what, when and how long
    (%G? codes), then the caller's own, across the screen."""
    right = len(left) + w
    rows = [('on now', '{50%GO'),
            ('last', '{20%GL · {16%GW · {10%GA'),
            ('hottest', '{24%GP · %GC plays'),
            ('marathon', '{20%GX · {16%GJ · {08%GZ'),
            ('addict', '{20%GK · {10%GQ in doors'),
            ('time sunk', '{10%GH all told · %GN plays today'),
            ('dusty', '{24%GD · {14%GG'),
            ('untried', '{40%GE')]
    yours = [('plays', '%GY · favorite {24%GF'),
             ('last', '{24%GU · {14%GV')]
    out = [left + ink.box_top('who played what', w)]
    out += [left + DG + '│ ' + prow(ink, label, body) + f'@{right:02d}' + DG + '│' for label, body in rows]
    out.append(left + ink.box_divider('you', w))
    out += [left + DG + '│ ' + prow(ink, label, body) + f'@{right:02d}' + DG + '│' for label, body in yours]
    out.append(left + ink.box_bottom(w))
    return out


def bbslink_note(out, ink):
    """Beside the BBSLink menu's box: what it is."""
    note = [WH + 'interbbs door games',
            GR + 'on bbslink.net\'s server: your',
            GR + 'character and scores are shared',
            GR + 'with players on other boards.',
            '',
            DG + 'm ' + GR + 'shows every game they run;',
            GR + 'the numbers go straight in.',
            '',
            ink.RD + '- ' + DG + 'q comes back here when you\'re done.' + ink.RD + ' -']
    for i, t in enumerate(note):
        out[1 + i] += '   ' + t


def tarot_extras(out, ink):
    """Under the tarot menu's box: what's left today and a whisper; beside
    it three cards face down (the middle one raised), drawn in place."""
    out.append('')
    out.append('      ' + DG + 'left today' + ink.RD + ': ' + WH + '%TR' + GR + ' readings' + DG + ' · ' + WH + '%TF' + GR + ' fortunes')
    out.append('      ' + ink.RD + '- ' + DG + 'the cards remember every caller.' + ink.RD + ' -')
    back = ['╔═══════╗', '║░▒▓▓▓▒░║', '║▒ ▄▀▄ ▒║', '║▓ ▌*▐ ▓║', '║▒ ▀▄▀ ▒║', '║░▒▓▓▓▒░║', '╚═══════╝']
    shade = {'░': DG, '▒': ink.RD, '▓': ink.BR, '*': E + '0;1;33m'}
    cards = ''
    for x, y in ((48, 10), (58, 9), (68, 10)):
        for i, row in enumerate(back):
            cards += goto(y + i, x) + cells_line([(ch, ink.RD if ch in '╔═╗║╚╝' else shade.get(ch, WH)) for ch in row])
    out.append(cards)


def prompt_bar(ink, title):
    """The screen's last row, the old Iniquity way: where you are, the
    cursor's slot, then time left, node and the time against the right
    edge (%> right-aligns the rest). The run of rules is long enough for
    the narrowest stats; wider ones cover its end. {99 first: a screen's
    {NN width stays in force, and a box's {04 would cut the clock."""
    s = ' ' + DG + '░' + ink.RD + '▒' + ink.BR + '▓' + ink.HIW + ' ' + title + ' ' + ink.BR + '▓' + ink.RD + '▒' + DG + '░ ' + ink.RD + '(' + ink.BR + ' ~PC  ' + ink.RD + ') '
    used = 1 + 3 + len(title) + 2 + 3 + 1 + 1 + 1 + 2 + 1 + 1
    stats = ('{99' + DG + '─[ ' + WH + '%TL' + GR + ' min left' + DG + ' ]─[ ' + GR + 'node ' + WH + '%ND' + DG + ' ]─[ ' + WH + '%TM' + DG + ' ]')
    return s + DG + '─' * (79 - 36 - used) + '%>' + stats

# ---- the menu file ---------------------------------------------------------


def toml_value(v):
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return '[' + ', '.join(toml_value(x) for x in v) + ']'
    return 'true' if v is True else 'false' if v is False else str(v)


def effect_lines(effects):
    """`[[effect]]` tables, as a menu file or a screen's .FX has them."""
    t = []
    for kind, (r0, r1, c0, c1), every, delay, more in effects:
        t += ['', '[[effect]]', f'kind = "{kind}"', f'rows = [{r0}, {r1}]']
        if (c0, c1) != (1, 80):
            t.append(f'cols = [{c0}, {c1}]')
        t += [f'every = {every}', f'delay = {delay}']
        t += [f'{k} = {toml_value(v)}' for k, v in more.items()]
    return t


def menu_toml(name, title, ink, w, order, labels, extra, effects):
    """The board's menu, from the stock one: commands in marker order (a
    key on screen that's a global command, like main's g, comes from
    GLOBAL), then any extra options, then the rest hidden (their keys
    still work)."""
    base, own = STOCK_FROM.get(name, (name, {}))
    stock = tomllib.load(open(os.path.join(MENUDIR, base.upper() + '.toml'), 'rb'))
    stock.update(own)
    glob = tomllib.load(open(os.path.join(MENUDIR, 'GLOBAL.toml'), 'rb'))['command']
    rest = [dict(c) for c in stock['command']]
    cmds = [c for c in rest if c['keys'] == 'FIRSTCMD']
    rest = [c for c in rest if c['keys'] != 'FIRSTCMD']
    for k in order:
        same = [c for c in rest if c['keys'].lower() == k] or [dict(c) for c in glob if c['keys'].lower() == k] or ([dict(ADDED[(name, k)])] if (name, k) in ADDED else [])
        if not same:
            sys.exit(f'{name}: no command for {k}')
        rest = [c for c in rest if c['keys'].lower() != k]
        first, stacked = same[0], same[1:]
        first['on'] = item_text(ink, w, k, labels[k], True)
        first['off'] = item_text(ink, w, k, labels[k], False)
        first.pop('hidden', None)
        if (name, k) in PARAMS:
            first['param'] = PARAMS[(name, k)]
        if (name, k) in COMMANDS:
            first['command'] = COMMANDS[(name, k)]
        if (name, k) in CHANGED:
            first.update(CHANGED[(name, k)])
            if not first['param']:
                first.pop('param')
        cmds.append(first)
        # Commands stacked on the same key run with it, off the bar.
        for c in stacked:
            c['hidden'] = True
            cmds.append(c)
    cmds += extra
    cmds += [dict(c) for c in HIDDEN_ADDED.get(name, [])]
    for c in rest:
        if name not in DROP_REST or (name, c['keys'].lower()) in KEEP_HIDDEN:
            c['hidden'] = True
            cmds.append(c)

    def q(v):
        if isinstance(v, str):
            return json.dumps(v, ensure_ascii=False)
        return 'true' if v is True else 'false' if v is False else str(v)
    t = [f"# The board's {title}: a lightbar with hot keys over {name.upper()}.ANS. Made by",
         f'# art/mainmenu-gen.py {name} (from the stock menu); change that, not this.']
    m = {k: v for k, v in stock.items() if k != 'command'}
    m['type'] = 'lightbar'
    m.pop('prompt', None)
    for k, v in m.items():
        t.append(f'{k} = {q(v)}')
    t += effect_lines(effects)
    for c in cmds:
        t.append('')
        t.append('[[command]]')
        for k in ('keys', 'command', 'param', 'acs', 'desc', 'help', 'hidden', 'on', 'off'):
            if k in c and c[k] not in ('', None):
                t.append(f'{k} = {q(c[k])}')
    return '\n'.join(t) + '\n'

# ---- putting it together ---------------------------------------------------


def build(name, variant):
    """The menu's screen and its menu file."""
    title, boxes = MENUS[name]
    logo, accent = LOGOS.get(name, (None, '31'))
    ink = Ink(accent)
    w = min(38, (77 - (len(boxes) - 1)) // len(boxes))
    left = ' ' * ((79 - (w * len(boxes) + len(boxes) - 1)) // 2)
    # The tarot menu's box sits left, with the cards face down beside it;
    # the doors menu's narrower, with the scoreboard beside it.
    if name == 'tarot':
        left = ' ' * 5
    if name == 'bbslink':
        w = 26
        left = ' ' * 2
    h = max(len(b[1]) for b in boxes)
    # The doors menu's boxes have room for more doors.
    if name == 'door':
        h = max(h, 7)
    head = logo_head(name, logo)

    out = []
    # The rule: where you are, on the file and message menus; else the
    # title. The area name is cut to 24, so the rule ends by column 79.
    where = {'file': ('area', '{24%FA'), 'message': ('area', '{24%MA')}.get(name)
    if where:
        out.append(' ' + ink.fade(8, False) + DG + ' [ ' + ink.fade_text(title) + ' ' + DG + '] ' + ink.fade(29 - len(title), True) + DG + ' [ ' + ink.fade_text(where[0]) + DG + ': ' + WH + where[1] + DG + ' ]')
    else:
        side = (78 - len(title) - 6) // 2
        rest = 78 - len(title) - 6 - side
        out.append(' ' + ink.fade(side, False) + DG + ' [ ' + ink.fade_text(title) + ' ' + DG + ']' + ' ' + ink.fade(rest, True))

    # Every menu is a lightbar with hot keys (the engine takes both): the
    # boxes hold markers, and the menu draws the options from its on/off
    # text. The markers take no room, so the spaces after them make up
    # each row; they're numbered down each box, then across.
    order = []
    columns = []
    for box_title, items in boxes:
        col = [ink.box_top(box_title, w)]
        for i in range(h):
            if i >= len(items):
                col.append(DG + '│' + ' ' * (w - 2) + DG + '│')
                continue
            k = items[i][0]
            order.append(k)
            col.append(DG + '│' + '@@' + k + '@@' + ' ' * (w - 2) + DG + '│')
        col.append(ink.box_bottom(w))
        columns.append(col)
    num = {k: i + 1 for i, k in enumerate(order)}
    for i in range(h + 2):
        r = left + ' '.join(c[i] for c in columns)
        for k in order:
            r = r.replace('@@' + k + '@@', f'~{num[k]:02d}^{num[k]:02d}')
        out.append(r)
    labels = {k: label for b in boxes for (k, label) in b[1]}

    extra = []
    if name in GRIDS:
        rows, extra = grid_rows(name, ink, left, len(order))
        out += rows
    if name == 'door':
        out += door_strip(ink, left, len(boxes) * (w + 1) - 1)
    if name == 'bbslink':
        bbslink_note(out, ink)
    if name == 'main':
        head += (main_head_extras(variant, ink) + E + '0m' + goto(head.count(b'\n') + 1, 1)).encode('cp437')
        whisper = WHISPERS[variant]
        out.append(' ' * ((79 - len(whisper) - 4) // 2) + ink.RD + '- ' + DG + whisper + ink.RD + ' -')
    if name == 'message':
        head += (message_head_extras(ink) + E + '0m' + goto(head.count(b'\n') + 1, 1)).encode('cp437')
    if name == 'file':
        head += (file_head_extras(ink, head.count(b'\n')) + E + '0m' + goto(head.count(b'\n') + 1, 1)).encode('cp437')
    if name == 'door':
        head += (file_head_extras(ink, head.count(b'\n'), DOOR_WORDS) + E + '0m' + goto(head.count(b'\n') + 1, 1)).encode('cp437')
    if name == 'tarot':
        tarot_extras(out, ink)
    # The selected option's help, then the prompt bar, on the caller's last
    # two rows (255 is past any bottom; terminals stop there).
    out.append(E + '255;1H' + E + 'A' + ' ' + ink.BR + '»' + ink.RD + '»' + DG + '» ' + WH + '~HP')
    out.append(E + '255;1H' + prompt_bar(ink, title))

    # The effects' rows: the logo's, then the rule's and the boxes'.
    logo_rows = head.count(b'\n')
    spans = {'logo': (1, logo_rows, 1, 80), 'boxes': (logo_rows + 1, logo_rows + h + 3, 1, 80), 'noise': (2, 2, 4, 77), 'inside': (3, 8, 4, 4),
             'codes_left': (1, 9, 3, 11), 'codes_right': (1, 9, 69, 77), 'side_left': (1, logo_rows, 2, 2), 'side_right': (1, logo_rows, 79, 79)}
    effects = [(kind, spans[over], every, delay, more) for kind, over, every, delay, more in EFFECTS.get(name, [])]

    screen = head + ('\x1b[0m\r\n'.join(out) + '\x1b[0m').encode('cp437')
    return screen, menu_toml(name, title, ink, w, order, labels, extra, effects), ink


def build_picture(name, spec):
    """A menu laid over a picture (a board's PICTURES[name]): the art, its
    backdrop recolored (backdrop.py), a panel with the title over the menu's
    boxes in columns on the open side, an optional box of values under
    them, then the help line and the prompt bar. Everything is placed by
    row and column, so the picture shows wherever nothing is drawn."""
    from backdrop import backdrop
    title, boxes = MENUS[name]
    ink = Ink(spec.get('accent', '35'))
    left, w = spec.get('left', 2), spec.get('box_width', 20)
    names = {k: label for b in boxes for (k, label) in b[1]}
    labels = {k: spec.get('labels', {}).get(k, v) for k, v in names.items()}
    out = E + '0m' + E + '2J' + backdrop(boardname.art(spec['art']), tuple(spec['color']), spec.get('threshold', 30), spec.get('speck', 25), fade=spec.get('fade'))
    # The title panel: the menu's name in a font (the board draws it, from
    # its fonts/), and a word under it.
    pw = len(spec['columns']) * (w + 1) - 1
    out += goto(1, left) + ink.box_top(spec.get('title', title), pw)
    for y in range(2, 7):
        out += goto(y, left) + DG + '│' + E + '0m' + ' ' * (pw - 2) + DG + '│'
    out += goto(7, left) + ink.box_bottom(pw)
    out += goto(2, left + 2) + E + '0m' + '%[' + spec['font'] + ']' + spec.get('word', title) + '%]'
    # The boxes: markers numbered down each column, then across.
    order, row0 = [], 8
    for c, (box_title, keys) in enumerate(spec['columns']):
        x = left + c * (w + 1)
        out += goto(row0, x) + ink.box_top(box_title, w)
        for i, k in enumerate(keys):
            order.append(k)
            out += goto(row0 + 1 + i, x) + DG + '│' + E + '0m' + f'~{len(order):02d}^{len(order):02d}' + ' ' * (w - 2) + DG + '│'
        out += goto(row0 + 1 + len(keys), x) + ink.box_bottom(w)
    # A box of values under the columns, if there's room for one.
    if spec.get('values'):
        vt, rows = spec['values']
        vw = spec.get('values_width', w)
        y0 = row0 + max(len(k) for _, k in spec['columns']) + 3
        out += goto(y0, left) + ink.box_top(vt, vw)
        for i, (label, codes) in enumerate(rows):
            out += goto(y0 + 1 + i, left) + DG + '│' + E + '0m' + ' ' * (vw - 2) + DG + '│'
            out += goto(y0 + 1 + i, left + 2) + ink.fade_text(label) + ' ' + WH + '{' + f'{vw - 5 - len(label):02d}' + codes
        out += goto(y0 + 1 + len(rows), left) + ink.box_bottom(vw)
    out += E + '255;1H' + E + 'A' + E + '0m' + ' ' + ink.BR + '»' + ink.RD + '»' + DG + '» ' + WH + '~HP'
    out += E + '255;1H' + E + '0m' + prompt_bar(ink, title) + E + '0m'
    return out.encode('cp437'), menu_toml(name, title, ink, w, order, labels, [], []), ink


def doorstat():
    """DOORSTAT.ANS (the doors menu's s): the doors menu's logo and rule,
    the whole scoreboard, a pause; and DOORSTAT.FX, its margins decoding
    as the menu's do."""
    logo, accent = LOGOS['door']
    ink = Ink(accent)
    head = logo_head('door', logo)
    rows = head.count(b'\n')
    head += (file_head_extras(ink, rows, DOOR_WORDS) + E + '0m' + goto(rows + 1, 1)).encode('cp437')
    title = 'scoreboard'
    side = (78 - len(title) - 6) // 2
    out = [' ' + ink.fade(side, False) + DG + ' [ ' + ink.fade_text(title) + ' ' + DG + ']' + ' ' + ink.fade(78 - len(title) - 6 - side, True)]
    out += door_scoreboard(ink, ' ', 77)
    out.append(E + '0m|PA')
    fx = [(kind, (1, rows, c, c), every, delay, more) for (kind, over, every, delay, more), c in zip(EFFECTS['door'][:2], (2, 79))]
    fx.append(('random', (1, rows, 1, 80), 10.0, 2.0, {}))
    screen = head + ('\x1b[0m\r\n'.join(out)).encode('cp437')
    note = ['# DOORSTAT.ANS animated while it waits (fluxbbs [[effect]]).', '# Made by art/mainmenu-gen.py doorstat; change that, not this.']
    return screen, '\n'.join(note + effect_lines(fx)) + '\n'


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else 'main'
    variant = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    # STOCK.ANS, the header |SA draws on the screens without a logo of
    # their own: the board's name in red.
    if name == 'stock':
        sys.stdout.buffer.write(b'\x1b[0m' + tdf_logo('DAMNEDX.TDF', 'Damned Red', boardname.NAME, 0, {'spacing': 0}) + b'\x1b[0m')
        return
    if name == 'doorstat':
        screen, fx = doorstat()
        open(os.path.join(os.getcwd(), 'DOORSTAT.FX'), 'w').write(fx)
        sys.stdout.buffer.write(screen)
        return
    if name in PICTURES:
        screen, toml, ink = build_picture(name, PICTURES[name])
    else:
        screen, toml, ink = build(name, variant)
    open(os.path.join(os.getcwd(), '..', 'menus', name.upper() + '.toml'), 'w').write(toml)
    sys.stdout.buffer.write(screen)
    # The tarot readings' header (one line): the menu's rule, with stars
    # at its ends.
    if name == 'tarot':
        t = MENUS['tarot'][0]
        side = (70 - len(t) - 6) // 2
        rest = 70 - len(t) - 6 - side
        hdr = ' ' + ink.BR + '*' + DG + ' · ' + ink.fade(side, False) + DG + ' [ ' + ink.fade_text(t) + ' ' + DG + ']' + ' ' + ink.fade(rest, True) + DG + ' · ' + ink.BR + '*'
        open(os.path.join(os.getcwd(), 'TAROTHDR.ANS'), 'wb').write(('\x1b[0m' + hdr + '\x1b[0m\r\n').encode('cp437'))


if __name__ == '__main__':
    main()
