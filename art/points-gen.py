import importlib.util, os, sys
# Points & lottery (!P, k on the main menu): POINTS in a TheDraw font
# faded to gold, then two panels the board fills in (~ codes, see
# points.rs): your points and lately, tonight's lottery and the last
# draw. The board prints its keys on the row under them. Run from text/:
#     python3 -I ../art/points-gen.py > POINTS.ANS
HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('mm', os.path.join(HERE, 'mainmenu-gen.py'))
mm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mm)

E = '\x1b['
DG = E + '0;1;30m'; GR = E + '0;37m'; WH = E + '0;1;37m'
# Gold, from pale to deep.
GOLD = [(255, 240, 160), (245, 200, 80), (215, 150, 30), (170, 100, 10)]
FONT = ('CRYSTALX.TDF', 'Crystal Red', {'fade': ((255, 240, 160), (200, 120, 0))})


def rgb(c):
    return E + '38;2;%d;%d;%dm' % c


def mix(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def fade_text(t):
    out = ''
    for i, ch in enumerate(t):
        x = i / max(1, len(t) - 1)
        out += (rgb(mix(GOLD[0], GOLD[2], x)) + ch) if ch != ' ' else ' '
    return out


def fade(n, rev):
    cs = [DG if i < n - 6 else rgb(GOLD[3]) if i < n - 1 else rgb(GOLD[1]) for i in range(n)]
    return ''.join(c + '─' for c in (cs[::-1] if rev else cs))


def at(r, c):
    return E + f'{r};{c}H'


W = 38  # each panel
LEFT, RIGHT = 2, 42


def box_top(title, w):
    rest = w - 10 - len(title)
    return rgb(GOLD[1]) + '┌' + DG + '─[ ' + fade_text(title) + DG + ' ]' + '─' * rest + rgb(GOLD[3]) + '───' + rgb(GOLD[1]) + '┐'


def box_bottom(w):
    return rgb(GOLD[1]) + '└' + rgb(GOLD[3]) + '──' + DG + '─' * (w - 6) + rgb(GOLD[3]) + '──' + rgb(GOLD[1]) + '┘'


def label(t):
    return GR + f'{t:<11}'


out = '\x1b[0m\x1b[2J\x1b[H' + mm.tdf_logo(*FONT[:2], 'points', 0, FONT[2]).decode('cp437') + '\x1b[0m'
top = 8
title = 'points & lottery'
side = (78 - len(title) - 6) // 2
rest = 78 - len(title) - 6 - side
out += at(top, 1) + ' ' + fade(side, False) + DG + ' [ ' + fade_text(title) + ' ' + DG + ']' + ' ' + fade(rest, True)

rows = 8
left = [
    label('balance') + WH + '{12~PB' + GR + ' points',
    label('today') + rgb(GOLD[1]) + '+' + '{10~PT',
    DG + '── ' + GR + 'lately' + DG + ' ' + '─' * 23,
] + [DG + '}05~D%d ' % i + WH + '}05~A%d ' % i + GR + '{22~W%d' % i for i in range(1, 6)]
right = [
    label('pot') + rgb(GOLD[0]) + '{12~LP' + GR + ' points',
    label('tickets') + WH + '{06~LS' + GR + ' sold',
    label('yours') + WH + '{04~LM ' + DG + '{18~LN',
    label('drawn') + GR + 'at midnight',
    '',
    label('last draw') + WH + '{12~LD',
    label('winner') + WH + '{14~LW ' + DG + '{06~LT',
    label('won') + rgb(GOLD[0]) + '{10~LA' + GR + ' points',
]
out += at(top + 1, LEFT) + box_top('your points', W) + at(top + 1, RIGHT) + box_top("tonight's lottery", W)
for i in range(rows):
    r = top + 2 + i
    for col, items in ((LEFT, left), (RIGHT, right)):
        out += at(r, col) + DG + '│' + at(r, col + 2) + (items[i] if i < len(items) else '') + at(r, col + W - 1) + DG + '│'
out += at(top + 2 + rows, LEFT) + box_bottom(W) + at(top + 2 + rows, RIGHT) + box_bottom(W)
hint = 'up votes, doors and tips pay points. b plays the lottery'
out += at(top + 4 + rows, (80 - len(hint) - 4) // 2 + 1) + rgb(GOLD[2]) + '- ' + DG + hint + rgb(GOLD[2]) + ' -'
# The board's keys go on the row under it.
out += '\x1b[0m' + at(top + 6 + rows, 2)
sys.stdout.buffer.write(out.encode('cp437'))
