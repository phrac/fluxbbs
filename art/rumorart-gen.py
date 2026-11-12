"""RUMORART.ANS: the rumors over a picture, if the board's art/local.py
has a RUMOR_ART; the stock set has none (the rumors list as a list).

    RUMOR_ART = {'art': 'valley-girl.ans', 'backdrop': '46', 'from': 36, 'shift': 8,
                 'font': ('TOWNSMLX.TDF', 'Town Small P', 'rumors')}

The picture's backdrop (cells on that background, blank or drawn in its
own color) is blacked out left of the figure, row by row: the figure is
the first other cell at or past column `from` (from 0), drawn `shift`
columns further right for more room (its right edge cut). The rumors go
in the black, between `~TL` and `~BR`, with the prompts at `~PR`; the
board wraps them to fit (rumors.rs). With a 'font' (file, face, word;
the file from the board's fonts/ or art/fonts/), the word heads the
space in it, else a title line ('title'). Run from text/ (make.py does).
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ansgrid  # noqa: E402
import boardname  # noqa: E402

E = '\x1b['
DG = E + '0;1;30m'


def fade(t, accent):
    """White through the bright accent to the dark (the menus' titles)."""
    stops = [(255, 255, 255), (255, 85, 255), (170, 0, 170)] if accent == '35' else [(255, 255, 255), (85, 255, 255), (0, 170, 170)]
    out = ''
    for i, ch in enumerate(t):
        x = i / max(1, len(t) - 1) * 2
        a, b = (stops[0], stops[1]) if x <= 1 else (stops[1], stops[2])
        f = x if x <= 1 else x - 1
        r, g, bl = (round(a[j] + (b[j] - a[j]) * f) for j in range(3))
        out += E + f'38;2;{r};{g};{bl}m' + ch if ch != ' ' else ' '
    return out


def mainmenu():
    """mainmenu-gen.py, for its TheDraw fonts."""
    import importlib.util
    sp = importlib.util.spec_from_file_location('mm', os.path.join(HERE, 'mainmenu-gen.py'))
    mm = importlib.util.module_from_spec(sp)
    sp.loader.exec_module(mm)
    return mm


def build(spec):
    grid = ansgrid.parse(open(boardname.art(spec['art']), 'rb').read())
    back, start = spec.get('backdrop', '46'), spec.get('from', 36)
    accent = spec.get('accent', '35')
    shift = spec.get('shift', 0)
    rows = 24

    def backdrop(cell):
        return cell is None or cell[1][1] == back and (cell[0] == 32 or cell[1][0] == back.replace('4', '3', 1))
    # Where the figure starts on each row; the space is left of the
    # narrowest, a column short of it.
    edge = []
    for r in range(rows):
        edge.append(next((c for c in range(start, 80) if not backdrop(grid.get((r, c)))), 80))
    right = min(edge) + shift - 1
    out = E + '0m' + E + '2J' + E + 'H'
    for r in range(rows):
        out += E + f'{r + 1};1H' + E + '0m' + ' ' * (edge[r] + shift)
        out += ansgrid.line(grid, r, edge[r], 78 - shift, edge[r] + shift + 1, r + 1).decode('cp437')
    # The heading (a word in a font, or a title line) and the rules
    # round the space.
    w = right - 1
    top = 2
    if spec.get('font'):
        file, face, word = spec['font']
        own = os.path.join(boardname.BOARD, 'fonts', file)
        logo = mainmenu().tdf_logo(own if os.path.isfile(own) else file, face, word, 2).decode('cp437')
        rows_ = logo.split('\r\n')[:-1]
        for i, row in enumerate(rows_):
            out += E + f'{i + 1};1H' + E + '0m' + row + E + '0m'
        out += E + f'{len(rows_) + 1};2H' + DG + '─' * w
        top = len(rows_) + 2
    else:
        title = spec.get('title', 'rumor has it')
        out += E + '1;2H' + DG + '─[ ' + fade(title, accent) + DG + ' ]' + '─' * max(0, w - len(title) - 5)
    out += E + f'{top};2H' + E + '0m' + '~TL'
    out += E + f'{rows - 3};{right}H' + E + '0m' + '~BR'
    out += E + f'{rows - 2};2H' + DG + '─' * w
    out += E + f'{rows - 1};2H' + E + '0m' + '~PR'
    return out.encode('cp437')


def main():
    spec = boardname.local('RUMOR_ART', None)
    path = os.path.join(os.getcwd(), 'RUMORART.ANS')
    if not spec:
        return
    with open(path, 'wb') as f:
        f.write(build(spec))


if __name__ == '__main__':
    main()
