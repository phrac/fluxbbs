"""A piece of ANSI art as a grid of cells, to lay it out again: what each
cell shows and its colors (16, 256 or 24-bit), wrapping at 80 columns.
"""
import re

# Backgrounds that read as black.
BLACK = ('40', '48;5;0', '48;5;16')


def parse(art):
    """{(row, col): (byte, (fg, bg, bold, blink))}, rows and columns from
    0; fg and bg are SGR parameters ('32', '38;5;28'). The art ends at a
    SAUCE record's ^Z."""
    if b'\x1a' in art:
        art = art[:art.index(b'\x1a')]
    grid = {}
    row = col = 0
    fg, bg, bold, blink = '37', '40', 0, 0
    i = 0
    while i < len(art):
        m = re.match(rb'\x1b\[([0-9;]*)([A-Za-z])', art[i:])
        if m:
            ps, cmd = m.group(1), m.group(2)
            i += m.end()
            if cmd == b'C':
                col += int(ps or 1)
                while col >= 80:
                    col -= 80
                    row += 1
            elif cmd == b'm':
                vs = [int(x) for x in ps.split(b';') if x] or [0]
                k = 0
                while k < len(vs):
                    v = vs[k]
                    if v in (38, 48) and k + 2 < len(vs) and vs[k + 1] == 5:
                        n = 3
                    elif v in (38, 48) and k + 4 < len(vs) and vs[k + 1] == 2:
                        n = 5
                    else:
                        n = 1
                    if n > 1:
                        s = ';'.join(str(x) for x in vs[k:k + n])
                        if v == 38:
                            fg = s
                        else:
                            bg = s
                    elif v == 0:
                        fg, bg, bold, blink = '37', '40', 0, 0
                    elif v == 1:
                        bold = 1
                    elif v == 5:
                        blink = 1
                    elif 30 <= v <= 37:
                        fg = str(v)
                    elif 40 <= v <= 47:
                        bg = str(v)
                    elif v == 39:
                        fg = '37'
                    elif v == 49:
                        bg = '40'
                    k += n
            continue
        c = art[i]
        i += 1
        if c == 13:
            col = 0
        elif c == 10:
            row += 1
        else:
            grid[(row, col)] = (c, (fg, bg, bold, blink))
            col += 1
            if col >= 80:
                col = 0
                row += 1
    return grid


def blank(cell):
    return cell is None or cell[0] == 32 and cell[1][1] in BLACK


def sgr(a):
    fg, bg, bold, blink = a
    return '\x1b[0;' + ('1;' if bold else '') + ('5;' if blink else '') + f'{fg};{bg}m'


def line(grid, row, first, last, x, y):
    """Columns first..last (from 0) of a row of the grid, drawn at column
    x of screen row y (from 1), its blank end left off; b'' if blank."""
    cells = [grid.get((row, c)) for c in range(first, last + 1)]
    while cells and blank(cells[-1]):
        cells.pop()
    if not cells:
        return b''
    out = f'\x1b[{y};{x}H'.encode()
    cur = None
    for cell in cells:
        ch, a = cell if cell else (32, ('37', '40', 0, 0))
        if a != cur:
            out += sgr(a).encode()
            cur = a
        out += bytes([ch])
    return out
