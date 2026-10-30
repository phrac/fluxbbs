"""GALLERY.ANS: the art gallery's screen. GALLERY set in Crystal (red) over
a row of codes, then the lister's rows (art/GALLERY-lister.ans: the rule
with the area's name, the column heads, the template row the board
fills, and the bottom), with what's here written into the rule.

It also writes GALLERY.FX beside it: a glint and sparkle over the
letters, the codes decoding into the gallery's phrases (phrases.py), a
ripple along the rule, while the gallery waits for a key.

Run from text/: python3 -I ../art/gallery-gen.py > GALLERY.ANS
"""
import importlib.util
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from phrases import GALLERY

spec = importlib.util.spec_from_file_location('mainmenu_gen', os.path.join(HERE, 'mainmenu-gen.py'))
mg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mg)

E = '\x1b['
DG = E + '0;1;30m'
WH = E + '0;1;37m'
RD = E + '0;31m'
BR = E + '0;1;31m'


def goto(r, c):
    return E + f'{r};{c}H'


def codes_row(row):
    """Fifteen groups of four, dim, a letter lit here and there."""
    rnd = random.Random(1994)
    out = goto(row, 3)
    for g in range(15):
        for _ in range(4):
            ch = rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789')
            out += (BR if rnd.random() < 0.12 else DG) + ch
        if g < 14:
            out += ' '
    return out


def main():
    logo = mg.tdf_logo('CRYSTALX.TDF', 'Crystal Red', 'GALLERY', 0, {'spacing': 0})
    rows = logo.count(b'\n')
    head = b'\x1b[0m\x1b[2J\x1b[H' + logo
    codes = codes_row(rows + 1)
    lister = open(os.path.join(HERE, 'GALLERY-lister.ans'), 'rb').read()
    # What's here, over the right end of the rule (row 9 in the lister).
    ink = mg.Ink('31')
    # (No ~SZ for the size: ~S plus a character is the lister's scroll bar.)
    stats = goto(9, 52) + DG + '[ ' + ink.fade_text('files') + ' ' + WH + '~NA' + ' ' + BR + '·' + ' ' + ink.fade_text('folders') + ' ' + WH + '~NF' + ' ' + DG + ']'
    # Into the lister part before its column heads: later and it lands
    # on the prompt bar's line, whose %> measures what follows it.
    k = lister.find(b'\x1b[10;')
    lister = lister[:k] + (stats + E + '0m').encode('cp437') + lister[k:]
    sys.stdout.buffer.write(head + (codes + E + '0m').encode('cp437') + lister)
    fx = ['# GALLERY.ANS animated while the gallery waits (fluxbbs [[effect]]).',
          '# Made by art/gallery-gen.py; change that, not this.', '',
          '[[effect]]', 'kind = "random"', f'rows = [1, {rows}]', 'every = 10.0', 'delay = 2.0', '',
          '[[effect]]', 'kind = "dust"', f'rows = [1, {rows}]', 'every = 10.0', 'delay = 0.0', 'amount = 0.5', '',
          '[[effect]]', 'kind = "decode"', f'rows = [{rows + 1}, {rows + 1}]', 'cols = [3, 76]', 'every = 10.0', 'delay = 2.0', 'amount = 1.8',
          'messages = [' + ', '.join('"%s"' % m for m in GALLERY) + ']', '',
          '[[effect]]', 'kind = "ripple"', f'rows = [{rows + 2}, {rows + 2}]', 'every = 10.0', 'delay = 6.0']
    open(os.path.join(os.getcwd(), 'GALLERY.FX'), 'w').write('\n'.join(fx) + '\n')


if __name__ == '__main__':
    main()
