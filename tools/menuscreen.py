#!/usr/bin/env python3
"""Make menu screens in the style of disflux's MAIN: `|SA` (the STOCK
header), a rule with the menu's name, then its commands as `(k) label`
in columns, in 24-bit colour.

    tools/menuscreen.py MENUS_DIR OUT_DIR NAME...

USERCFG and NEWCFG (the user configuration forms) are made the same way,
with the stock screens' field markers (`~XY`: field X, hot key Y), and so
is LISTFILE, the frame of the file lister, the gallery and the archive
browser (its markers are described in the README's gallery section).

Commands only the SysOp can use (s250+) are left off, except on SYSOP,
and so are the ones not ported. Columns: as many as the labels allow, up
to six, within the rows below the header.
"""
import os, sys, tomllib

# The rows a list may take: 25, less STOCK (11), the rule and the prompt.
MAX_ROWS = 11
NOT_PORTED = {'*D', '*M', 'TP', 'TS'}
TITLES = {'TIMEBANK': 'time bank', 'DOOR': 'online games', 'MULTINOD': 'multinode'}

RULE_DARK, RULE_MID = (90, 20, 30), (233, 98, 12)
TITLE = (255, 200, 60)
BRACKET, KEY = (110, 40, 60), (0, 230, 230)
LABEL_TOP, LABEL_BOTTOM = (255, 150, 0), (235, 60, 25)


def fg(c):
    return b'\x1b[38;2;%d;%d;%dm' % c


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def sysop_only(acs):
    for part in acs.replace('&', ' ').replace('|', ' ').split():
        if part[:1] == 's' and part[1:].isdigit() and int(part[1:]) >= 250:
            return True
    return False


def order(key):
    k = key.lstrip('/')
    return (0 if k[:1].isalnum() else 1, k.lower(), key.startswith('/'))


def rule(name):
    title = b' %s ' % name.encode()
    left = (78 - len(title)) // 2
    right = 78 - len(title) - left
    out = bytearray(b'\x1b[0m ')
    for i in range(left):
        out += fg(lerp(RULE_DARK, RULE_MID, i / max(left - 1, 1))) + b'\xc4'
    out += fg(TITLE) + title
    for i in range(right):
        out += fg(lerp(RULE_MID, RULE_DARK, i / max(right - 1, 1))) + b'\xc4'
    return bytes(out + b'\x1b[0m\r\n')


def key(k, label, colour, width=0):
    return fg(BRACKET) + b'(' + fg(KEY) + k.encode() + fg(BRACKET) + b') ' + colour + label.ljust(width).encode()


# Field letter, hot key and label, as on the stock forms; the board fills
# in the value at the marker (plain bright white there, a DOS colour).
LEFT = [('D', 'l', 'location'), ('E', 'a', 'address'), ('F', 'u', 'usernote'), ('A', 'p', 'password'),
        ('B', 'n', 'phone num'), ('C', 'b', 'birthdate'), ('O', 'z', 'zip code')]
RIGHT = [('G', 's', 'screen len'), ('M', 'e', 'emulation'), ('L', 'q', 'autoquote'), ('H', 'y', 'yes/no bars'),
         ('I', 'x', 'expert mode'), ('J', 'h', 'hot keys'), ('K', 'w', 'pausing')]
VALUE = b'\x1b[0;1;37m'


def form(name):
    new = name == 'NEWCFG'
    left = [f for f in LEFT if new or f[0] not in 'AC']
    out = bytearray(b'\x1b[0m\x1b[2J|SA') + rule('new user config' if new else 'user config')
    for r in range(max(len(left), len(RIGHT))):
        colour = fg(lerp(LABEL_TOP, LABEL_BOTTOM, r / (len(RIGHT) - 1)))
        out += b'\x1b[0m '
        if r < len(left):
            f, k, label = left[r]
            out += key(k, label, colour, 12) + VALUE + b'~' + f.encode() + k.upper().encode()
        if r < len(RIGHT):
            f, k, label = RIGHT[r]
            out += b'@51' + key(k, label, colour, 12) + VALUE + b'~' + f.encode() + k.upper().encode()
        out += b'\x1b[0m\r\n'
    colour = fg(LABEL_BOTTOM)
    out += b'\x1b[0m ' + key('t', 'theme', colour, 12) + VALUE + b'~NT' + b'@57' + colour + b'by ' + VALUE + b'~*A\x1b[0m\r\n\r\n'
    out += b' ' + key('enter', 'save', fg(LABEL_TOP), 6) + key('esc', 'abort', fg(LABEL_TOP), 9)
    out += fg(BRACKET) + b'command: ' + VALUE + b'~**\x1b[0m'
    return bytes(out)


def listfile():
    """The lister's frame: rule, area, column heads, rows 4-21, then the
    scroll bar, info line and menu bar. The live parts are drawn in DOS
    colours, from the ~C1-~C6 markers: normal, dim, hot key, bar, dim bar,
    hot key on the bar."""
    out = bytearray(b'\x1b[0m\x1b[2J') + rule('file listing')
    out += b' \x1b[0;1;33m~FA\x1b[0m\r\n'
    head = fg(BRACKET)
    out += b'\x1b[0m' + b'@03' + head + b'name' + b'@28' + head + b'size' + b'@37' + head + b'description\x1b[0m\r\n'
    out += b'\x1b[0m@01~TL' + b'\x1b[21;1H~BL'
    out += b'\x1b[0m\x1b[4;3H~FN\x1b[4;28H~FS\x1b[4;37H~FD'
    # Scroll bar along row 22: the empty part, then the full part.
    out += b'\x1b[22;2H\x1b[0;31m~S\xc4\x1b[22;79H\x1b[0;33m~E\xcd'
    out += b'\x1b[23;1H\x1b[0;33m~PI\x1b[24;1H~PM'
    out += b'\x1b[25;1H\x1b[0;33m~C1\x1b[0;1;30m~C2\x1b[0;1;36m~C3\x1b[0;1;37;41m~C4\x1b[0;37;41m~C5\x1b[0;1;33;41m~C6\x1b[0m'
    return bytes(out)


def bar():
    """A gradient rule with no title."""
    out = bytearray(b'\x1b[0m ')
    for i in range(78):
        out += fg(lerp(RULE_DARK, RULE_MID, 1 - abs(i - 39) / 39)) + b'\xc4'
    return bytes(out + b'\x1b[0m\r\n')


def farea(part):
    """The file area list (`FA`/`FC`): a top with the column heads, a row
    (MID, and HIL for the current area) and a bottom. Values: ~AN number,
    ~AT name, ~NF files, ~NW newest upload, ~SP sponsor."""
    if part == 'TOP':
        head = fg(BRACKET)
        return (b'\x1b[0m\x1b[2J|SA' + rule('file areas') + b'\x1b[0m@03' + head + b'#' + b'@08' + head + b'area'
                + b'@46' + head + b'files' + b'@54' + head + b'newest' + b'@66' + head + b'sponsor\x1b[0m\r\n')
    if part == 'BOT':
        return bar()
    hi = part == 'HIL'
    name = fg(TITLE) if hi else fg(LABEL_TOP)
    other = fg((255, 230, 180)) if hi else fg(LABEL_BOTTOM)
    mark = fg(TITLE) + b'\x10' if hi else b' '
    return (b'\x1b[255D\x1b[0m ' + mark + b'@03' + fg(KEY) + b'~AN@08' + name + b'{37~AT@46' + other + b'~NF@54'
            + b'~NW@66' + other + b'{14~SP\x1b[0m\r\n')


def aichat():
    """The AI chat's header: the conversation scrolls below it."""
    return b'\x1b[0m\x1b[2J|SA' + rule('ai chat')


def screen(menu, name):
    cmds = [c for c in menu.get('command', [])
            if c.get('desc') and not c.get('hidden') and c.get('keys') != 'FIRSTCMD'
            and c.get('command') not in NOT_PORTED
            and (name == 'SYSOP' or not sysop_only(c.get('acs', '')))]
    seen, items = set(), []
    for c in sorted(cmds, key=lambda c: order(c['keys'])):
        k = c['keys'].lower()
        if k in seen:
            continue
        seen.add(k)
        label = c['desc'].lower()
        if label in ('quit menu', 'quit sysop menu'):
            label = 'quit'
        items.append((k, label))
    # As many columns as fit the longest label, within MAX_ROWS.
    keyw = max(len(k) for k, _ in items)
    for cols in (6, 5, 4, 3, 2, 1):
        width = 78 // cols
        rows = -(-len(items) // cols)
        if rows <= MAX_ROWS and max(len(l) for _, l in items) <= width - keyw - 4:
            break
    rows = -(-len(items) // cols)
    title = TITLES.get(name, menu.get('title', name).lower().removesuffix(' menu') + ' menu')
    out = bytearray(b'\x1b[0m\x1b[2J|SA') + rule(title)
    for r in range(rows):
        colour = fg(lerp(LABEL_TOP, LABEL_BOTTOM, r / max(rows - 1, 1)))
        out += b' '
        row = [items[c * rows + r] for c in range(cols) if c * rows + r < len(items)]
        for n, (k, label) in enumerate(row):
            cell = fg(BRACKET) + b'(' + fg(KEY) + k.encode() + fg(BRACKET) + b') ' + colour
            pad = width - len(k) - 4
            out += cell + (label.encode() if n == len(row) - 1 else label.ljust(pad).encode())
        out += b'\x1b[0m\r\n'
    return bytes(out)


if __name__ == '__main__':
    menus, outdir, names = sys.argv[1], sys.argv[2], sys.argv[3:]
    for name in names:
        if name in ('USERCFG', 'NEWCFG'):
            data = form(name)
        elif name == 'LISTFILE':
            data = listfile()
        elif name == 'AICHAT':
            data = aichat()
        elif name.startswith('FAREA'):
            data = farea(name[5:])
        else:
            data = screen(tomllib.load(open(os.path.join(menus, name + '.toml'), 'rb')), name)
        open(os.path.join(outdir, name + '.ANS'), 'wb').write(data)
        print(f'{name}.ANS')
