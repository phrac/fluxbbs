"""LOGIN.ANS: the screen the login's name and password are asked on, made
only for a board whose art/local.py has LOGIN_ART (a piece of art in its
art/). The art keeps columns 1-78 (so nothing writes to the last ones and
wraps); in it, where the board asks for the name and the password (~LU,
~LW: the prompt there when it's asked, the answer typed on the row under
in the matrix's bright color), and a line for the rest (~LM: wrong
logins, other questions; not the bottom row, where a yes/no's newline
would scroll the screen). LOGIN_AT is where the prompts start, (column,
row): (44, 19) to start.

Run from text/: python3 -I ../art/login-gen.py
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ansgrid  # noqa: E402
import boardname  # noqa: E402

name = boardname.local('LOGIN_ART', None)
if not name or not boardname.art(name):
    sys.exit(0)
grid = ansgrid.parse(open(boardname.art(name), 'rb').read())
art = b''.join(ansgrid.line(grid, r, 0, 77, 1, r + 1) for r in range(24))

E = '\x1b['
FIELD = E + '0;' + boardname.matrix_colors()['bright'] + 'm'


def at(x, y):
    return E + f'{y};{x}H'


x, y = boardname.local('LOGIN_AT', (44, 19))
out = at(x, y) + FIELD + '~LU' + at(x, y + 2) + '~LW' + at(x, y + 4) + E + '0;37m' + '~LM' + E + '0m'
with open(os.path.join(os.getcwd(), 'LOGIN.ANS'), 'wb') as f:
    f.write(b'\x1b[0m\x1b[2J' + art + b'\x1b[0m' + out.encode('cp437'))
