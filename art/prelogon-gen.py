"""PRELOGON.ANS and its random variants (.AN1, .AN2...): the board's name in a
TheDraw font over a few lines about the board, scene style. Each variant
sets the logo from a TheDraw color font (.TDF) in art/fonts/, recolored
to the board's pink and cyan; the board shows one of them at random each
call. VARIANTS lists them: the file, the face in it (a file can hold
several), the columns between letters, and maps from the face's DOS
colors to the board's, foreground and background apart.

The lines sit in a single-line box colored like the logo, with white
corners.

It also writes PRELOGON.FX beside them (one for all, so every logo sits
in the same rows): glints over the logo, the box and the prompt in turn
while the screen waits for Enter.

Run from text/: python3 -I ../art/prelogon-gen.py
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import boardname  # noqa: E402

# (file, face, spacing, foreground map, background map); the first is
# PRELOGON.ANS, the rest .AN1, .AN2...
VARIANTS = [
    ('EXILANSA.TDF', 'Exiliian Sho', 0, {}, {}),      # Exilian Short Alt
    ('FEAR.TDF', 'Fear', 2, {4: 5, 6: 13, 14: 15}, {4: 5, 6: 5}),
    ('GRAVEX.TDF', 'Grave Purple', 2, {14: 13}, {}),
    ('DAMNATNX.TDF', 'Damnation Pu', 1, {}, {}),
    ('DAMNEDX.TDF', 'Damned Purpl', 1, {}, {}),
    ('GRNSHTX.TDF', 'GReeNSHiT P', 2, {}, {}),
    ('CRYSTALX.TDF', 'Crystal Purp', 1, {15: 13, 7: 5}, {7: 5}),
    ('AMNESIAX.TDF', 'Amnesia Purp', 1, {}, {}),
    ('FONT55X.TDF', 'Font 55 Ppl', 0, {5: 13}, {}),
]
LOGO_ROWS = 12                   # the tallest a logo may be (the box starts under them)
LOGO_ROW = 2

# The lines under it: (row, parts), a part being (color, text) with the
# board's own codes for the counts. Colors are DOS numbers.
PINK, CYAN, GRAY, DIM, WHITE = 13, 11, 7, 8, 15
LINES = [
    (16, [(PINK, boardname.TITLE), (DIM, ' :: '), (GRAY, 'a flux/bbs system')]),
    (18, [(CYAN, '%FT'), (GRAY, ' files in '), (CYAN, '%FE'), (GRAY, ' areas')]),
    (19, [(GRAY, 'message bases'), (DIM, ' .. '), (GRAY, 'ai chat'), (DIM, ' .. '), (GRAY, 'doors'), (DIM, ' .. '), (GRAY, 'ssh and telnet')]),
    (20, [(GRAY, 'sysop '), (PINK, boardname.SYSOP)]),
    (21, [(DIM, 'every call is logged. no lamers.')]),
    (22, [(GRAY, 'best with your terminal at full speed'), (DIM, ' .. '), (GRAY, 'no baud rate emulation')]),
    (24, [(PINK, '[ '), (WHITE, 'press enter to connect'), (PINK, ' ]')]),
]
# The board's own lines (its art/local.py's PRELOGON_LINES), if it has them.
LINES = boardname.local('PRELOGON_LINES', LINES)
# A single-line box round the lines, a column in from the sides, the
# logo's pink at the top fading to its cyan at the bottom, the corners
# white with a pink run off each: (top row, bottom row, left column,
# right column) and the color of each row of it.
BOX = (15, 23, 2, 79)
BOX_COLORS = [PINK, PINK, 5, 5, 3, 3, 3, CYAN, CYAN]
CORNER, ARM = WHITE, PINK
ANSI = [0, 4, 2, 6, 1, 5, 3, 7]      # SGR numbers for the DOS colors


def read_tdf(path):
    """The fonts in a .TDF file: name, type (0 outline, 1 block, 2 color),
    letter spacing and glyphs, a glyph being rows of (char, attr)."""
    d = open(path, 'rb').read()
    if d[:20] != b'\x13TheDraw FONTS file\x1a':
        sys.exit(path + ' is not a TheDraw font file')
    fonts, p = [], 20
    while p + 213 <= len(d) and d[p:p + 4] == b'\x55\xaa\x00\xff':
        name = d[p + 5:p + 5 + min(d[p + 4], 12)].decode('cp437', 'replace').rstrip('\x00 ')
        ftype, spacing = d[p + 21], d[p + 22]
        size = struct.unpack_from('<H', d, p + 23)[0]
        offs = struct.unpack_from('<94H', d, p + 25)
        data = d[p + 213:p + 213 + size]
        glyphs = {}
        for i, o in enumerate(offs):
            if o == 0xFFFF or o + 2 > len(data):
                continue
            w, q = data[o], o + 2
            rows, row = [], []
            while q < len(data) and data[q] != 0:
                if data[q] == 0x0D:
                    rows.append(row)
                    row = []
                    q += 1
                elif ftype == 2:
                    row.append((data[q], data[q + 1]))
                    q += 2
                else:
                    row.append((data[q], 7))
                    q += 1
            if row:
                rows.append(row)
            glyphs[chr(33 + i)] = (w, rows)
        fonts.append({'name': name, 'type': ftype, 'spacing': spacing, 'glyphs': glyphs})
        p += 213 + size
    return fonts


def set_word(font, word, spacing, fg_map, bg_map):
    """The word in the font, its colors mapped: rows of (char, fg, bg),
    None where the font draws nothing."""
    glyphs = [font['glyphs'][ch] if ch in font['glyphs'] else (2, []) for ch in word if ch == ' ' or ch in font['glyphs']]
    h = max(len(rows) for _, rows in glyphs)
    out = [[] for _ in range(h)]
    for gi, (w, rows) in enumerate(glyphs):
        for y in range(h):
            row = rows[y] if y < len(rows) else []
            cells = [(ch, fg_map.get(a & 15, a & 15), bg_map.get((a >> 4) & 7, (a >> 4) & 7)) if ch not in (0, 32) or a >> 4 & 7 else None for ch, a in row]
            cells += [None] * (w - len(cells))
            out[y].extend(cells[:w])
            if gi < len(glyphs) - 1:
                out[y].extend([None] * spacing)
    return out


def sgr(fg, bg=0):
    return '\x1b[0;%s3%d;4%dm' % ('1;' if fg > 7 else '', ANSI[fg & 7], ANSI[bg])


def main():
    for i, (file, face, spacing, fg_map, bg_map) in enumerate(VARIANTS):
        path = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'fonts', file)
        fonts = [f for f in read_tdf(path) if f['name'] == face]
        if not fonts:
            sys.exit('%s has no font "%s"' % (path, face))
        # The longest of the board's names that fits the screen.
        for name in boardname.NAMES:
            if any(c != ' ' and c not in fonts[0]['glyphs'] for c in name) and name != boardname.NAMES[-1]:
                continue
            word = set_word(fonts[0], name, spacing, fg_map, bg_map)
            if max(len(r) for r in word) <= 80:
                break
        if len(word) > LOGO_ROWS:
            sys.exit('%s sets the logo %d rows tall' % (face, len(word)))
        name = 'PRELOGON.ANS' if i == 0 else 'PRELOGON.AN%d' % i
        open(os.path.join(os.getcwd(), name), 'wb').write(screen(word))
    fx = ['# PRELOGON.ANS and its variants, animated while they wait for Enter',
          '# (fluxbbs [[effect]]). Made by art/prelogon-gen.py; change that, not this.', '',
          '[[effect]]', 'kind = "glint"', 'rows = [%d, %d]' % (LOGO_ROW, LOGO_ROW + LOGO_ROWS - 1),
          'cols = [1, 80]', 'every = 8.0', 'delay = 2.0', 'amount = 0.8',
          '', '# The box and its lines, then the prompt, after it.',
          '[[effect]]', 'kind = "glint"', 'rows = [%d, %d]' % (BOX[0], BOX[1]), 'cols = [%d, %d]' % (BOX[2], BOX[3]), 'every = 8.0', 'delay = 4.5', 'amount = 0.7',
          '', '[[effect]]', 'kind = "glint"', 'rows = [%d, %d]' % (LINES[-1][0], LINES[-1][0]), 'cols = [1, 80]', 'every = 8.0', 'delay = 6.5', 'amount = 0.9']
    open(os.path.join(os.getcwd(), 'PRELOGON.FX'), 'w').write('\n'.join(fx) + '\n')


def screen(word):
    """The screen: the logo centered at the top, the box and its lines."""
    width = max(len(r) for r in word)
    if width > 80:
        sys.exit('the logo is %d columns wide' % width)
    col0 = (80 - width) // 2
    # A short logo sits midway in the rows the tallest one takes.
    row0 = LOGO_ROW + (LOGO_ROWS - len(word)) // 2
    out = bytearray(b'\x1b[0m\x1b[2J')
    for y, row in enumerate(word):
        out += b'\x1b[%d;%dH' % (row0 + y, col0 + 1)
        cur = None
        for cell in row:
            if cell is None:
                out += b'\x1b[0m '
                cur = None
                continue
            ch, fg, bg = cell
            if (fg, bg) != cur:
                out += sgr(fg, bg).encode()
                cur = (fg, bg)
            out.append(ch)
    # The box, then the lines in it, centered as they'll read once the
    # counts are in.
    r0, r1, c0, c1 = BOX
    for row in range(r0, r1 + 1):
        color = BOX_COLORS[min(row - r0, len(BOX_COLORS) - 1)]
        if row in (r0, r1):
            left, right = (b'\xda', b'\xbf') if row == r0 else (b'\xc0', b'\xd9')
            run = b'\xc4' * 2
            out += b'\x1b[%d;%dH' % (row, c0) + sgr(CORNER).encode() + left + sgr(ARM).encode() + run
            out += sgr(color).encode() + b'\xc4' * (c1 - c0 - 5)
            out += sgr(ARM).encode() + run + sgr(CORNER).encode() + right
        else:
            side = ARM if row in (r0 + 1, r1 - 1) else color
            out += b'\x1b[%d;%dH' % (row, c0) + sgr(side).encode() + b'\xb3' + b'\x1b[%d;%dH' % (row, c1) + b'\xb3'
    for row, parts in LINES:
        text = ''.join(t for _, t in parts)
        for code, filled in (('%FT', '62,199'), ('%FE', '913'), ('%FZ', '16 GB')):
            text = text.replace(code, filled)
        out += b'\x1b[%d;%dH' % (row, (80 - len(text)) // 2 + 1)
        if len(text) > c1 - c0 - 3 and r0 < row < r1:
            sys.exit('too wide for the box: ' + text)
        for color, t in parts:
            out += (sgr(color) + t).encode('cp437')
    out += b'\x1b[0m|PK'
    return bytes(out)


if __name__ == '__main__':
    main()
