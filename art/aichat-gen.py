"""AICHAT.ANS: the AI chat's header. The AI's name ([ai] name in the
board's TOML) set in Brotherhood (four rows), a row of codes under it, the rule with the
chat's name and a word, and the help line; the conversation runs below.

It also writes AICHAT.FX beside it: a glint over the name, the codes
decoding into the chat's phrases (phrases.py), a ripple along the rule,
the help line typing itself out now and then, all while the chat waits
for the caller.

Run from text/: python3 -I ../art/aichat-gen.py > AICHAT.ANS
"""
import importlib.util
import os
import random
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from phrases import AI

spec = importlib.util.spec_from_file_location('mainmenu_gen', os.path.join(HERE, 'mainmenu-gen.py'))
mg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mg)

E = '\x1b['
DG = E + '0;1;30m'
WH = E + '0;1;37m'
GR = E + '0;37m'
# A board can name the chat and its word, and pick the color, in its
# art/local.py (AICHAT = {'title': ..., 'tagline': ..., 'accent': '35',
# 'recolor': {9: 13}}, the logo's DOS colors swapped for others). With
# 'face': {'art': 'X.ans', 'cols': [19, 46], 'rows': [1, 12]} that part of
# the board's art/X.ans goes in the top left corner, the name and a box of
# stats ('left' and 'right': [(label, codes)]) beside it.
AICHAT = mg.boardname.local('AICHAT', {})
ACCENT = AICHAT.get('accent', '34')
# The help line's typing, in the accent's bright color.
TINT = {'31': '#ff5555', '32': '#55ff55', '33': '#ffff55', '34': '#5555ff', '35': '#ff55ff', '36': '#55ffff'}.get(ACCENT, '#5555ff')
RD = E + '0;' + ACCENT + 'm'
BR = E + '0;1;' + ACCENT + 'm'


def goto(r, c):
    return E + f'{r};{c}H'


def codes_row(row, col=3, groups=15):
    rnd = random.Random(2049)
    out = goto(row, col)
    for g in range(groups):
        for _ in range(4):
            ch = rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789')
            out += (BR if rnd.random() < 0.12 else DG) + ch
        if g < groups - 1:
            out += ' '
    return out


def cells(path):
    """A piece of 24-bit (or plain) ANSI art as rows of (sgr, glyph): the
    colors each cell is drawn in, as escape parameters."""
    import re
    d = open(path, 'rb').read().split(b'\x1a')[0]
    out = []
    for line in d.split(b'\r\n'):
        fg, bg, row = '0;37', '40', []
        for m in re.finditer(rb'\x1b\[([0-9;]*)m|([^\x1b])', line, re.S):
            if m.group(1) is None:
                row.append(((fg, bg), m.group(2)))
                continue
            p = m.group(1).decode().split(';')
            while p:
                if p[:2] == ['38', '2']:
                    fg, p = ';'.join(['38', '2'] + p[2:5]), p[5:]
                elif p[:2] == ['48', '2']:
                    bg, p = ';'.join(['48', '2'] + p[2:5]), p[5:]
                elif p[0] in ('', '0'):
                    fg, bg, p = '0;37', '40', p[1:]
                else:
                    p = p[1:]
        out.append(row)
    return out


def dim(sgr, f):
    """A 24-bit color darkened to f of itself (others as they are)."""
    p = sgr.split(';')
    if p[1:2] != ['2']:
        return sgr
    return ';'.join(p[:2] + [str(round(int(v) * f)) for v in p[2:5]])


def face(spec):
    """The face (AICHAT['face']: art, cols, rows) cut out of a piece and
    put in the top left corner, its right edge fading into the dark."""
    grid = cells(mg.boardname.art(spec['art']))
    (c0, c1), (r0, r1) = spec['cols'], spec['rows']
    w = c1 - c0 + 1
    out = ''
    for y in range(r0, r1 + 1):
        row = grid[y - 1] if y - 1 < len(grid) else []
        out += goto(y - r0 + 1, 1)
        cur = None
        for x in range(c0, c1 + 1):
            (fg, bg), ch = row[x - 1] if x - 1 < len(row) else (('0;37', '40'), b' ')
            # The last four columns, and the bottom row, fade out.
            f = min(1.0, (w - (x - c0)) / 5) * (0.6 if y == r1 else 1.0)
            fg, bg = dim(fg, f), dim(bg, f)
            if (fg, bg) != cur:
                out += E + '0;' + fg + ';' + bg + 'm'
                cur = (fg, bg)
            out += ch.decode('cp437')
    return out + E + '0m', r1 - r0 + 1, w


def stat_box(row, col, w, ink, title, left, right):
    """A box in the stock style with two columns of (label, value codes)."""
    out = goto(row, col) + RD + '┌' + BR + '─' + DG + '[ ' + ink.fade_text(title) + ' ' + DG + ']'
    out += ink.hline(w - 2 - (len(title) + 5)) + RD + '┐'
    half = (w - 4) // 2
    for i in range(max(len(left), len(right))):
        out += goto(row + 1 + i, col) + DG + '│' + E + '0m' + ' ' * (w - 2) + DG + '│'
        for j, items in enumerate((left, right)):
            if i < len(items):
                label, value = items[i]
                out += goto(row + 1 + i, col + 2 + j * half) + ink.fade_text(label) + ' ' + DG + '.' * (9 - len(label))
                out += ' ' + WH + '{' + f'{half - 12:02}' + value
    n = max(len(left), len(right))
    return out + goto(row + 1 + n, col) + RD + '└' + ink.hline(w - 2) + RD + '┘', n + 2


def main_face(name, ink, title, tagline):
    """The face layout: her face in the corner, the name, a row of codes
    and the stats beside it, the rule under them; the chat below."""
    pic, h, w = face(AICHAT['face'])
    x = w + 3
    logo = mg.tdf_logo('BROTHOOD.TDF', 'Brotherhood', name.upper(), x + 2, {'spacing': 1, 'recolor': AICHAT.get('recolor', {})})
    rows = logo.count(b'\n')
    # The logo's rows are padded from the left edge: it goes first, the face over it.
    out = b'\x1b[0m\x1b[2J\x1b[H' + logo + pic.encode('cp437')
    groups = (80 - x) // 5
    s = codes_row(rows + 1, x, groups)
    box, bh = stat_box(rows + 2, x, 80 - x, ink, 'tonight', AICHAT.get('left', []), AICHAT.get('right', []))
    s += box
    help_line = 'type a message, enter sends. /help, /quit.'
    s += goto(rows + 2 + bh, x + 1) + GR + help_line
    r = max(h, rows + 2 + bh) + 1
    left = DG + '[ ' + ink.fade_text(title) + ' ' + DG + ']'
    right = DG + '[ ' + ink.fade_text(tagline) + ' ' + DG + ']'
    rule = ''.join((BR if i in (0, 77) else RD if min(i, 77 - i) < 4 else DG) + '─' for i in range(78))
    s += goto(r, 2) + rule + goto(r, 4) + left + goto(r, 79 - len(tagline) - 4) + right + E + '0m' + goto(r + 1, 1)
    out += s.encode('cp437')
    fx = ['[[effect]]', 'kind = "random"', f'rows = [1, {rows}]', f'cols = [{x}, 79]', 'every = 10.0', 'delay = 2.0', '',
          '[[effect]]', 'kind = "decode"', f'rows = [{rows + 1}, {rows + 1}]', f'cols = [{x}, {x + groups * 5 - 2}]', 'every = 10.0', 'delay = 2.0', 'amount = 1.8',
          'messages = [' + ', '.join('"%s"' % m for m in AI) + ']', '',
          '[[effect]]', 'kind = "ripple"', f'rows = [{r}, {r}]', 'every = 10.0', 'delay = 6.0', '',
          '[[effect]]', 'kind = "type"', f'rows = [{rows + 2 + bh}, {rows + 2 + bh}]', f'cols = [{x + 1}, {x + 1 + len(help_line)}]', 'every = 30.0', 'delay = 4.0', 'amount = 0.4', f'tint = "{TINT}"']
    return out, fx


def main():
    cfg = mg.boardname.CONFIG
    name = cfg.get('ai', {}).get('name', 'ai') or 'ai'
    ink = mg.Ink(ACCENT)
    title = AICHAT.get('title', 'ai chat')
    tagline = AICHAT.get('tagline', 'it forgets you when you leave')
    if AICHAT.get('face'):
        out, fx = main_face(name, ink, title, tagline)
        sys.stdout.buffer.write(out)
        open(os.path.join(os.getcwd(), 'AICHAT.FX'), 'w').write('\n'.join(
            ['# AICHAT.ANS animated while the chat waits (fluxbbs [[effect]]).',
             '# Made by art/aichat-gen.py; change that, not this.', ''] + fx) + '\n')
        return
    logo = mg.tdf_logo('BROTHOOD.TDF', 'Brotherhood', name.upper(), 0, {'spacing': 1, 'recolor': AICHAT.get('recolor', {})})
    rows = logo.count(b'\n')
    out = b'\x1b[0m\x1b[2J\x1b[H' + logo + (codes_row(rows + 1) + E + '0m').encode('cp437')
    # The rule: the chat's name, and a word on the right.
    r = rows + 2
    left = DG + '[ ' + ink.fade_text(title) + ' ' + DG + ']'
    right = DG + '[ ' + ink.fade_text(tagline) + ' ' + DG + ']'
    rule = ''.join((BR if i in (0, 77) else RD if min(i, 77 - i) < 4 else DG) + '─' for i in range(78))
    out += (goto(r, 2) + rule + goto(r, 4) + left + goto(r, 79 - len(tagline) - 4) + right).encode('cp437')
    help_line = 'type a message and press enter. /help explains, /quit leaves.'
    out += (goto(r + 1, 2) + GR + help_line + E + '0m' + goto(r + 2, 1)).encode('cp437')
    sys.stdout.buffer.write(out)
    fx = ['# AICHAT.ANS animated while the chat waits (fluxbbs [[effect]]).',
          '# Made by art/aichat-gen.py; change that, not this.', '',
          '[[effect]]', 'kind = "random"', f'rows = [1, {rows}]', 'every = 10.0', 'delay = 2.0', '',
          '[[effect]]', 'kind = "decode"', f'rows = [{rows + 1}, {rows + 1}]', 'cols = [3, 76]', 'every = 10.0', 'delay = 2.0', 'amount = 1.8',
          'messages = [' + ', '.join('"%s"' % m for m in AI) + ']', '',
          '[[effect]]', 'kind = "ripple"', f'rows = [{r}, {r}]', 'every = 10.0', 'delay = 6.0', '',
          '[[effect]]', 'kind = "type"', f'rows = [{r + 1}, {r + 1}]', f'cols = [2, {2 + len(help_line)}]', 'every = 30.0', 'delay = 4.0', 'amount = 0.4', f'tint = "{TINT}"']
    open(os.path.join(os.getcwd(), 'AICHAT.FX'), 'w').write('\n'.join(fx) + '\n')


if __name__ == '__main__':
    main()
