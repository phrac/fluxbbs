"""FLUXBBS.ANS: the ad for flux/bbs, the software, and disflux, its home
board. flux/bbs in RoySAC2 over its spec sheet and where to get it;
then disflux in the menus' Amnesia over the board's. 16 colors, no cursor moves,
lines under 80 columns and CRLF ends, with a SAUCE record, so it shows
the same in any ANSI viewer, a board's ad rotation or a message.

    python3 -I art/ad-gen.py > art/FLUXBBS.ANS

The board's half is disflux's: change BOARD below for another.
"""
import importlib.util
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('prelogon', os.path.join(HERE, 'prelogon-gen.py'))
pre = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pre)

E = '\x1b['
PINK, MAG, CYAN, TEAL, GRAY, DIM, WHITE = 13, 5, 11, 3, 7, 8, 15


def c(fg):
    return E + ('0;1;3%dm' if fg > 7 else '0;3%dm') % pre.ANSI[fg & 7]


def logo(file, face, word):
    """The word in a TheDraw font, centered, as lines of ANSI."""
    font = next(f for f in pre.read_tdf(os.path.join(HERE, 'fonts', file)) if f['name'] == face)
    rows = pre.set_word(font, word, font['spacing'], {}, {})
    rows = [r for r in rows if any(r)]
    width = max(len(r) for r in rows)
    lead = ' ' * ((80 - width) // 2)
    out = []
    for r in rows:
        line, cur = lead, None
        for cell in r:
            if cell is None:
                if cur is not None:
                    line += E + '0m'
                    cur = None
                line += ' '
                continue
            ch, fg, bg = cell
            if (fg, bg) != cur:
                line += pre.sgr(fg, bg)
                cur = (fg, bg)
            line += bytes([ch]).decode('cp437')
        out.append(line.rstrip() + E + '0m')
    return out


def center(*parts):
    """Parts of (color, text), centered."""
    text = ''.join(t for _, t in parts)
    return ' ' * ((80 - len(text)) // 2) + ''.join(c(col) + t for col, t in parts)


def rule(title='', accent=PINK, mid=MAG):
    """A line across, bright at its ends, a [ title ] in the middle."""
    if not title:
        return ' ' + c(accent) + '──' + c(mid) + '────' + c(DIM) + '─' * 66 + c(mid) + '────' + c(accent) + '──'
    side = (78 - len(title) - 6) // 2
    rest = 78 - len(title) - 6 - side
    return (' ' + c(DIM) + '─' * (side - 4) + c(mid) + '───' + c(accent) + '─' + c(DIM) + ' [ ' + c(WHITE) + title
            + c(DIM) + ' ] ' + c(accent) + '─' + c(mid) + '───' + c(DIM) + '─' * (rest - 4))


W = 25


def fade(n, accent, mid):
    def col(i):
        d = min(i, n - 1 - i)
        return accent if d == 0 else mid if d < 3 else DIM
    return ''.join(c(col(i)) + '─' for i in range(n))


def box(title, items, accent, mid):
    """The menus' boxes: corners in the accent, edges fading to gray."""
    rows = [c(mid) + '┌' + c(accent) + '─' + c(DIM) + '[ ' + c(WHITE) + title + c(DIM) + ' ]' + fade(W - 2 - (len(title) + 5), accent, mid) + c(mid) + '┐']
    for it in items:
        rows.append(c(DIM) + '│' + c(accent) + ' » ' + c(GRAY) + f'{it:<{W - 5}}' + c(DIM) + '│')
    rows.append(c(mid) + '└' + fade(W - 2, accent, mid) + c(mid) + '┘')
    return rows


def side_by_side(boxes):
    h = max(len(b) for b in boxes)
    return [' ' + ' '.join(b[i] if i < len(b) else ' ' * W for b in boxes) for i in range(h)]


def say(text, lead=3):
    """Gray text, [bracketed] words in white."""
    return ' ' * lead + c(GRAY) + re.sub(r'\[([^\]]*)\]', lambda m: c(WHITE) + m.group(1) + c(GRAY), text)


# Spec sheets, as BBS ads have them: a label, then the facts.
SOFTWARE = [
    ('version', '[v1.0] .. rust .. mit license'),
    ('runs on', 'linux x86_64 + arm64, static .. or docker'),
    ('connect', 'ssh, telnet .. cp437, utf-8, 24-bit color'),
    ('msgs', 'threaded bases w/ voting, qwk, vim keys in the fse'),
    ('nets', 'ftn over binkp: echomail, netmail, tic .. mrc'),
    ('doors', 'door.sys, door32.sys, bbslink .. dos doors run inside,'),
    ('', 'no dosbox'),
    ('files', 'zmodem, https links, ansi/gif/pdf viewer'),
    ('scripts', 'python'),
    ('imports', 'iniquity 1.00a26r5 boards, as they are'),
]

# The board's half: disflux.
BOARD = {
    'name': 'disflux',
    'font': ('AMNESIAX.TDF', 'Amnesia Cyan'),
    'tag': [(GRAY, 'a private h/p/a/v/c system')],
    'specs': [
        ('sysop', '[phrac] .. co-sysop subcarrier'),
        ('nodes', '32 .. ssh [disfluxbbs.com:2222] .. telnet [disfluxbbs.com]'),
        ('files', '67,976 .. all of textfiles.com, 16colo.rs packs,'),
        ('', 'phrack, 2600'),
        ('doors', 'lord, tw2002, bre, usurper, overkill ii, food fight'),
        ('nets', 'agoranet 46:1/115 .. mrc'),
        ('where', 'southeast nebraska'),
    ],
}


def specs(rows, accent):
    """label  facts: the label dim and right-aligned, [words] in white."""
    out = []
    for label, text in rows:
        out.append('  ' + c(accent) + f'{label:>9}' + c(DIM) + ('  ' if not label else ' :') + ' '
                   + c(GRAY) + re.sub(r'\[([^\]]*)\]', lambda m: c(WHITE) + m.group(1) + c(GRAY), text))
    return out


def main():
    out = ['']
    out += logo('ROYSAC2C.TDF', 'RoySAC2Ppl', 'flux/bbs')
    out += ['']
    out += specs(SOFTWARE, PINK)
    out += ['', center((DIM, 'github.com/'), (WHITE, 'phrac/fluxbbs'), (DIM, '  ..  ghcr.io/'), (WHITE, 'phrac/fluxbbs')), '']
    out += [rule('world hq', CYAN, TEAL), '']
    out += logo(*BOARD['font'], BOARD['name'])
    out += ['', center(*BOARD['tag']), '']
    out += specs(BOARD['specs'], CYAN)
    out += ['', rule('', CYAN, TEAL)]
    for line in out:
        w = len(re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', line))
        if w >= 80:
            sys.exit(f'{w} columns: {line!r}')
    data = ('\r\n'.join(out) + E + '0m\r\n').encode('cp437')
    sys.stdout.buffer.write(data + sauce(len(data), len(out)))


def sauce(size, lines):
    """A SAUCE record: what the ad is, who made it, and that it's 80 columns."""
    def f(s, n):
        return s.encode('cp437')[:n].ljust(n, b' ')
    import datetime
    rec = (b'SAUCE00' + f('flux/bbs + disflux', 35) + f('phrac', 20) + f('disflux', 20)
           + datetime.date.today().strftime('%Y%m%d').encode() + size.to_bytes(4, 'little')
           + bytes([1, 1]) + (80).to_bytes(2, 'little') + lines.to_bytes(2, 'little') + bytes(4)
           + bytes([0, 0]) + f('IBM VGA', 22).replace(b' ', b'\0'))
    assert len(rec) == 128, len(rec)
    return b'\x1a' + rec


if __name__ == '__main__':
    main()
