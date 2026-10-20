"""Read the picture bot check like a person would, for the scripted callers.

The server draws three big characters with background colour in a field
of noise (challenge.rs). This replays the output on an 80x25 screen,
takes the commonest non-black background as the characters, and matches
each of the three slots against the font (read from challenge.rs, so
there's one copy) at each row offset, keeping the best fit; flipped
cells just cost a point or two.
"""
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, '..', 'crates', 'iniquity', 'src', 'challenge.rs')
W, H = 40, 9


def font():
    text = open(SRC, encoding='utf-8').read()
    out = {}
    for c, rows in re.findall(r"\('(.)', \[((?:\"[ #]{5}\", ?){6}\"[ #]{5}\")\]\)", text):
        out[c] = re.findall(r'"([ #]{5})"', rows)
    return out


def screen(raw):
    """(char, background) for each cell after replaying `raw`."""
    grid = [[(' ', 0)] * 80 for _ in range(25)]
    x = y = 0
    bg = 0
    i = 0
    while i < len(raw):
        b = raw[i]
        if b == 0x1b and raw[i + 1:i + 2] == b'[':
            m = re.match(rb'\x1b\[([0-9;?]*)([A-Za-z])', raw[i:])
            if not m:
                i += 1
                continue
            args = [int(a) if a.isdigit() else 0 for a in m.group(1).split(b';')] if m.group(1) else []
            n = (args[0] if args and args[0] else 1)
            cmd = m.group(2)
            if cmd == b'H':
                y = (args[0] if args and args[0] else 1) - 1
                x = (args[1] if len(args) > 1 and args[1] else 1) - 1
            elif cmd == b'A': y -= n
            elif cmd == b'B': y += n
            elif cmd == b'C': x += n
            elif cmd == b'D': x -= n
            elif cmd == b'G': x = n - 1
            elif cmd == b'J' and args[:1] == [2]:
                grid = [[(' ', 0)] * 80 for _ in range(25)]
                x = y = 0
            elif cmd == b'm':
                k = 0
                args = args or [0]
                while k < len(args):
                    a = args[k]
                    if a == 0: bg = 0
                    elif 40 <= a <= 47: bg = a - 40
                    elif a in (38, 48): k += 4 if args[k + 1:k + 2] == [2] else 2
                    k += 1
            i += m.end()
            x, y = max(0, min(79, x)), max(0, min(24, y))
            continue
        if b == 13: x = 0
        elif b == 10: y = min(24, y + 1)
        elif b >= 32:
            if 0 <= y < 25 and 0 <= x < 80:
                grid[y][x] = (bytes([b]).decode('cp437'), bg)
            x = min(79, x + 1)
        i += 1
    return grid


def solve(raw):
    """The three characters, or '' if there's no picture to read."""
    grid = screen(raw)
    counts = {}
    for row in grid:
        for _, bg in row:
            if bg: counts[bg] = counts.get(bg, 0) + 1
    if not counts:
        return ''
    back = max(counts, key=counts.get)
    lit = [[bg == back for _, bg in row] for row in grid]
    # Speckles are mostly alone; the characters' leftmost column is x0+2.
    def solid(r, c):
        return lit[r][c] and any(lit[rr][cc] for rr, cc in ((r - 1, c), (r + 1, c), (r, c - 1), (r, c + 1))
                                 if 0 <= rr < 25 and 0 <= cc < 80)
    cells = [(r, c) for r in range(25) for c in range(80) if solid(r, c)]
    if not cells:
        return ''
    minx, miny = min(c for _, c in cells), min(r for r, _ in cells)
    f = font()
    best = (-1, '')
    for x0 in range(minx - 3, minx - 1):
        for y0 in range(miny - 2, miny + 1):
            got, total = '', 0
            for slot in range(3):
                scores = []
                for ch, rows in f.items():
                    for top in range(3):
                        s = 0
                        for r in range(7):
                            for k in range(5):
                                want = rows[r][k] == '#'
                                for dx in range(2):
                                    yy, xx = y0 + top + r, x0 + 2 + slot * 13 + k * 2 + dx
                                    have = 0 <= yy < 25 and 0 <= xx < 80 and lit[yy][xx]
                                    s += want == have
                        scores.append((s, ch))
                s, ch = max(scores)
                got += ch
                total += s
            best = max(best, (total, got))
    return best[1]
