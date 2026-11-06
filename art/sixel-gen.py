"""SIXEL.ANS: the logon step that offers sixel pictures (`sixel`, UX),
made only for a board whose art/local.py has SIXEL_ART (a piece of art
in its art/ that leaves a column of black on the right). The art keeps
columns 1-79; the words go in the black from SIXEL_AT (column, row; (50,
2) to start), 30 columns wide: what sixels are, SIXEL_LINES (the board's
own, about its art), and where the board writes the terminal's lines
(~ST), asks (~SQ, three rows) and answers (~SR, four rows). SIXEL_SAMPLE, a
picture in the board's art/, is copied beside it as SIXEL.<ext>, the
sample the step offers.

Run from text/: python3 -I ../art/sixel-gen.py
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ansgrid  # noqa: E402
import boardname  # noqa: E402

name = boardname.local('SIXEL_ART', None)
if not name or not boardname.art(name):
    sys.exit(0)
grid = ansgrid.parse(open(boardname.art(name), 'rb').read())
art = b''.join(ansgrid.line(grid, r, 0, 78, 1, r + 1) for r in range(24))

E = '\x1b['
TITLE, RULE, BODY, NOTE = E + '0;1;36m', E + '0;36m', E + '0;37m', E + '0;1;30m'


def at(x, y):
    return E + f'{y};{x}H'


board = boardname.names()[0].lower()
if len(board) > 16:
    board = 'this board'
body = [f'{board} now draws pictures', 'in sixel graphics: real', 'pixels, not blocks. for the',
        'gallery, file previews and', 'PDF pages.']
x, y = boardname.local('SIXEL_AT', (50, 2))
out = at(x, y) + TITLE + 'sixel graphics' + at(x, y + 1) + RULE + '─' * 29
for i, s in enumerate(body):
    out += at(x, y + 3 + i) + BODY + s
out += at(x, y + 9) + BODY + '~ST'
for i, s in enumerate(boardname.local('SIXEL_LINES', [])[:3]):
    out += at(x, y + 12 + i) + NOTE + s
out += at(x, y + 16) + BODY + '~SQ' + at(x, y + 19) + BODY + '~SR' + E + '0m'
with open(os.path.join(os.getcwd(), 'SIXEL.ANS'), 'wb') as f:
    f.write(b'\x1b[0m\x1b[2J' + art + b'\x1b[0m' + out.encode('cp437'))

sample = boardname.local('SIXEL_SAMPLE', None)
if sample and boardname.art(sample):
    ext = os.path.splitext(sample)[1].upper()
    for old in ('SIXEL.PNG', 'SIXEL.JPG', 'SIXEL.GIF'):
        if old != 'SIXEL' + ext and os.path.exists(old):
            os.remove(old)
    shutil.copyfile(boardname.art(sample), 'SIXEL' + ext)
