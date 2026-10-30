"""WELCOME.ANS and its random variants (.AN1, .AN2...): the screen after
the password. The board's name across the top (its own art first, if
its art/local.py names a piece in WELCOME_ART, then the same TheDraw
fonts the prelogon rotates, recolored to the board's pink),
a row of codes under it, the "you made it" rule, the board/today/you
boxes and what's here. The board shows one variant at random each call.

VARIANTS lists the logos: ('art', file) for a piece of art in art/
(its drawn rows placed in the logo band), or (file, face, spacing,
color map) for a TheDraw font in art/fonts/.

It also writes WELCOME.FX beside them (one for all, so every logo sits in
the same rows): a glint over the logo, the codes decoding into the
welcome's phrases (phrases.py), the "identity confirmed" line typing
itself and a glint over the boxes, all while the screen waits at the
pause.

Run from text/: python3 -I ../art/welcome-gen.py
"""
import importlib.util
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from phrases import WELCOME

spec = importlib.util.spec_from_file_location('mainmenu_gen', os.path.join(HERE, 'mainmenu-gen.py'))
mg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mg)

# The first is WELCOME.ANS, the rest .AN1, .AN2... A font's map sends
# its DOS colors (foreground and background alike) to the board's.
VARIANTS = [
    ('AMNESIAX.TDF', 'Amnesia Purp', 1, {}),
    ('CRYSTALX.TDF', 'Crystal Purp', 1, {15: 13, 7: 5}),
    ('DAMNEDX.TDF', 'Damned Purpl', 1, {}),
    ('DAMNATNX.TDF', 'Damnation Pu', 1, {}),
    ('FONT55X.TDF', 'Font 55 Ppl', 0, {5: 13}),
    ('GRNSHTX.TDF', 'GReeNSHiT P', 2, {}),
]
# The board's own piece of art first, if its art/local.py names one.
if mg.boardname.local('WELCOME_ART', None):
    VARIANTS.insert(0, ('art', mg.boardname.local('WELCOME_ART', None)))
LOGO_ROWS = 10                   # rows 1-10; a shorter logo sits midway
CODES_ROW = LOGO_ROWS + 1

E = '\x1b['
DG = E + '0;1;30m'; WH = E + '0;1;37m'; GR = E + '0;37m'
accent = '35'
RD = E + '0;' + accent + 'm'; BR = E + '0;1;' + accent + 'm'
STOPS = [(255, 255, 255), (255, 85, 255), (170, 0, 170)]


def goto(r, c):
    return E + f'{r};{c}H'


def fade_text(t):
    out = ''
    for i, ch in enumerate(t):
        x = i / max(1, len(t) - 1) * 2
        a, b = (STOPS[0], STOPS[1]) if x <= 1 else (STOPS[1], STOPS[2])
        f = x if x <= 1 else x - 1
        r, g, bl = (round(a[j] + (b[j] - a[j]) * f) for j in range(3))
        out += E + f'38;2;{r};{g};{bl}m' + ch if ch != ' ' else ' '
    return out


# (box title, [(label, mci, width)]); values pad to width with }NN.
BOXES = [('the board', [('users', 'TU', 7), ('calls', 'TC', 7), ('posts', 'TP', 7), ('email', 'TE', 7), ('downloads', 'TD', 7)]),
         ('today', [('date', 'DT', 8), ('calls', 'DC', 8), ('posts', 'DP', 8), ('uploads', 'DU', 8), ('downloads', 'DD', 8)]),
         ('you', [('calls', 'UC', 8), ('last on', 'LC', 8), ('posts', 'UP', 8), ('mail', 'MW', 8), ('mins left', 'TL', 8)])]
W = 25
LEFT = ' ' * ((79 - (W * 3 + 2)) // 2)


def hline(n):
    def c(i):
        d = min(i, n - 1 - i)
        return BR if d == 0 else RD if d < 3 else DG
    return ''.join(c(i) + '─' for i in range(n))


def top(t):
    head = BR + '─' + DG + '[ ' + fade_text(t) + ' ' + DG + ']'
    return RD + '┌' + head + hline(W - 2 - (len(t) + 5)) + RD + '┐'


def bot():
    return RD + '└' + hline(W - 2) + RD + '┘'


def row(label, code, n):
    dots = W - 2 - (1 + len(label) + 1 + n + 1)
    return DG + '│ ' + fade_text(label) + ' ' + DG + '.' * (dots - 1) + ' ' + WH + '}%02d%%%s' % (n, code) + ' ' + DG + '│'


def center(items):
    sep = ' ' + BR + '·' + ' '
    plain = ' · '.join(items)
    return ' ' * ((79 - len(plain)) // 2) + sep.join(fade_text(i) for i in items)


def say(t, c=None):  # a line centered, faded or in one color
    return ' ' * ((79 - len(t)) // 2) + (c + t if c else fade_text(t))


def codes_row():
    """Fifteen groups of four, a few lit: noise for the decode effect."""
    rnd = random.Random(1995)
    out = goto(CODES_ROW, 3)
    for g in range(15):
        for _ in range(4):
            ch = rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789')
            out += (BR if rnd.random() < 0.12 else DG) + ch
        if g < 14:
            out += ' '
    return out


def logo_rows(variant):
    """The logo's drawn rows, as bytes without line ends."""
    if variant[0] == 'art':
        d = open(mg.boardname.art(variant[1]), 'rb').read()
        d = d[:d.index(b'\x1a')] if b'\x1a' in d else d
        rows = d.replace(b'\x1b[2J', b'').split(b'\r\n')
        while rows and not rows[0].strip():
            rows.pop(0)
        while rows and not rows[-1].strip():
            rows.pop()
        return rows
    file, face, spacing, recolor = variant
    logo = mg.tdf_logo(file, face, mg.boardname.NAME, 0, {'spacing': spacing, 'recolor': recolor})
    return [r.rstrip(b'\r') for r in logo.split(b'\n') if r.strip()]


def screen(variant):
    rows = logo_rows(variant)
    if len(rows) > LOGO_ROWS:
        sys.exit('%s is %d rows tall' % (variant[1], len(rows)))
    row0 = 1 + (LOGO_ROWS - len(rows)) // 2
    out = b'\x1b[0m\x1b[2J'
    for i, r in enumerate(rows):
        out += goto(row0 + i, 1).encode() + r + b'\x1b[0m'
    out += (codes_row() + E + '0m').encode('cp437')
    # The rule, then "you made it" over its left end (the handle's length
    # varies, so it's written over the dashes rather than measured in).
    made = ('\r' + E + '3C' + DG + '[ ' + fade_text('you made it, ') + WH + '{20%UN' + fade_text(". you weren't lying.")
            + ' ' + DG + ']')
    plain_rule = ' ' + ''.join((BR if i in (0, 77) else RD if min(i, 77 - i) < 6 else DG) + '─' for i in range(78))
    lines = [plain_rule + made]
    lines.append(say('identity confirmed. the trace has been called off. for now.', GR))
    cols = [[top(b[0])] + [row(*r) for r in b[1]] + [bot()] for b in BOXES]
    for i in range(len(cols[0])):
        lines.append(LEFT + ' '.join(c[i] for c in cols))
    lines.append(say("you're in. don't make us regret it."))
    # What's here, and how big the files are: a board's art/local.py can
    # name its own (WELCOME_WHATS) and say what its archive is
    # (WELCOME_ARCHIVE, after the size).
    lines.append(center(mg.boardname.local('WELCOME_WHATS', ['messages', 'email', 'files', 'art gallery', 'doors', 'ai chat'])))
    # Bragging rights: }NN pads a code's value to NN columns, so the plain
    # text centers exactly.
    archive = mg.boardname.local('WELCOME_ARCHIVE', None)
    brag = [('}06%FT', WH), (' files in ', GR), ('}03%FE', WH)]
    brag += [(' areas, ', GR), ('{06%FZ', WH), (archive, GR)] if archive else [(' areas, ', GR), ('{06%FZ', WH), ('.', GR)]
    lines.append(' ' * ((79 - len(''.join(t for t, c in brag))) // 2) + ''.join(c + t for t, c in brag))
    lines.append(say('best with your terminal at full speed. no baud rate emulation.', DG))
    out += (goto(CODES_ROW + 1, 1) + '\x1b[0m\r\n'.join(lines) + '\x1b[0m\r\n').encode('cp437') + b'|PA'
    return out


def main():
    for i, v in enumerate(VARIANTS):
        name = 'WELCOME.ANS' if i == 0 else 'WELCOME.AN%d' % i
        open(os.path.join(os.getcwd(), name), 'wb').write(screen(v))
    rule, ident, box0, box1 = CODES_ROW + 1, CODES_ROW + 2, CODES_ROW + 3, CODES_ROW + 9
    fx = ['# WELCOME.ANS and its variants, animated at the pause (fluxbbs [[effect]]).',
          '# Made by art/welcome-gen.py; change that, not this.', '',
          '[[effect]]', 'kind = "random"', 'rows = [1, %d]' % LOGO_ROWS, 'every = 10.0', 'delay = 1.5', '',
          '[[effect]]', 'kind = "decode"', 'rows = [%d, %d]' % (CODES_ROW, CODES_ROW), 'cols = [3, 76]', 'every = 10.0', 'delay = 3.0', 'amount = 1.8',
          'messages = [' + ', '.join('"%s"' % m for m in WELCOME) + ']', '',
          '[[effect]]', 'kind = "ripple"', 'rows = [%d, %d]' % (rule, rule), 'every = 10.0', 'delay = 6.0', '',
          '[[effect]]', 'kind = "type"', 'rows = [%d, %d]' % (ident, ident), 'cols = [1, 80]', 'every = 30.0', 'delay = 0.3', 'amount = 0.5', 'tint = "#ff55ff"', '',
          '[[effect]]', 'kind = "glint"', 'rows = [%d, %d]' % (box0, box1), 'cols = [1, 80]', 'every = 10.0', 'delay = 7.5', 'amount = 0.8']
    open(os.path.join(os.getcwd(), 'WELCOME.FX'), 'w').write('\n'.join(fx) + '\n')


if __name__ == '__main__':
    main()
