"""A piece of 24-bit half-block ANSI art with its dark backdrop swapped for a
color: the picture is read as pixels (two to a character cell), the dark
pixels joined to the edges are filled, and the specks left floating in it
(the dim lights of a dark room) go with them. The figure's own shadows
aren't joined to the edges, so they stay.

    from backdrop import backdrop
    ansi = backdrop('art/X.ans', (255, 85, 255))
"""
import re


def pixels(path):
    """The art's pixels, rows of (r, g, b), two rows to a line of text."""
    d = open(path, 'rb').read().split(b'\x1a')[0]
    rows = []
    for line in d.split(b'\r\n')[:25]:
        fg = bg = (0, 0, 0)
        top, bot = [], []
        for m in re.finditer(rb'\x1b\[([0-9;]*)m|([^\x1b])', line, re.S):
            if m.group(1) is not None:
                p = [int(v) for v in m.group(1).split(b';') if v] or [0]
                while p:
                    if p[:2] == [38, 2]:
                        fg, p = tuple(p[2:5]), p[5:]
                    elif p[:2] == [48, 2]:
                        bg, p = tuple(p[2:5]), p[5:]
                    elif p[0] == 0:
                        fg, bg, p = (170, 170, 170), (0, 0, 0), p[1:]
                    else:
                        p = p[1:]
                continue
            ch = m.group(2)[0]
            t, b = {0xDC: (bg, fg), 0xDF: (fg, bg), 0xDB: (fg, fg)}.get(ch, (bg, bg))
            top.append(t)
            bot.append(b)
        rows += [top, bot]
    return rows


def _flood(px, start, take):
    """The pixels reached from `start` through ones `take` accepts."""
    h, w = len(px), len(px[0])
    seen, stack = set(), list(start)
    while stack:
        y, x = stack.pop()
        if (y, x) in seen or not (0 <= y < h and 0 <= x < w) or not take(y, x):
            continue
        seen.add((y, x))
        stack += [(y + 1, x), (y - 1, x), (y, x + 1), (y, x - 1)]
    return seen


def backdrop(path, color, threshold=30, speck=25, width=80, lines=24, fade=None):
    """The art as ANSI (absolute rows, so it can sit under anything), its
    backdrop in `color`, widened to `width` columns with it; `lines` rows
    of it (the 25th is a menu's prompt bar, and drawing its last cell would
    scroll some terminals). With `fade` (two columns), the color is only
    left of the first, fading into the art's own backdrop by the second."""
    px = pixels(path)
    orig = [list(r) + [(0, 0, 0)] * (width - len(r)) for r in px]
    px = [r + [(0, 0, 0)] * (width - len(r)) for r in px]
    h, w = len(px), width
    edges = [(y, x) for y in range(h) for x in (0, w - 1)] + [(y, x) for x in range(w) for y in (0, h - 1)]
    for y, x in _flood(px, edges, lambda y, x: max(px[y][x]) < threshold):
        px[y][x] = color
    # Specks: what's left that isn't the color, in pieces too small to be
    # part of the picture.
    done = set()
    for y in range(h):
        for x in range(w):
            if (y, x) in done or px[y][x] == color:
                continue
            piece = _flood(px, [(y, x)], lambda yy, xx: px[yy][xx] != color)
            done |= piece
            if len(piece) < speck:
                for yy, xx in piece:
                    px[yy][xx] = color
    if fade:
        a, b = fade[0] - 1, fade[1] - 1
        for y in range(h):
            for x in range(a, w):
                if px[y][x] == color:
                    f = max(0.0, (b - x) / (b - a))
                    px[y][x] = tuple(round(orig[y][x][i] * (1 - f) + color[i] * f) for i in range(3))
    out = ''
    for r in range(0, min(h - 1, 2 * lines), 2):
        out += f'\x1b[{r // 2 + 1};1H'
        cur = None
        for x in range(w):
            key = (px[r][x], px[r + 1][x])
            if key != cur:
                out += '\x1b[48;2;%d;%d;%dm\x1b[38;2;%d;%d;%dm' % (key[0] + key[1])
                cur = key
            out += '▄'
    return out + '\x1b[0m'
