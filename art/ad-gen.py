"""The ads for flux/bbs and disflux. Every ad sells both: the software
(what it is, where to get it) and the board (how to call it). 16 colors,
no cursor moves, lines under 80 columns and CRLF ends, with a SAUCE
record, so they show the same in any ANSI viewer, a board's ad rotation
or a message.

    python3 -I art/ad-gen.py > art/ads/FLUXADnn.ANS        # the spec sheet
    python3 -I art/ad-gen.py fun > art/ads/FLUXADnn.ANS    # games and gear (5)
    python3 -I art/ad-gen.py games > art/ads/FLUXADnn.ANS  # the same, plainer (6)
    python3 -I art/ad-gen.py spec7 > art/ads/FLUXADnn.ANS  # 03's look, games added (7)
    python3 -I art/ad-gen.py echo8 > art/ads/FLUXADnn.ANS  # 07 for echomail: short lines (8)

art/ads/ keeps every one: a changed ad is a new file, the next number,
never written over an old one (they've gone out on the networks as they
were). The same goes for the copies in disflux's flux/bbs file area.
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
        ('nodes', '8 .. ssh [disfluxbbs.com:2222] .. telnet [disfluxbbs.com]'),
        ('files', '67,976 .. all of textfiles.com, 16colo.rs packs,'),
        ('', 'phrack, 2600'),
        ('doors', 'lord, tw2002, bre, usurper, overkill ii, food fight'),
        ('nets', 'agoranet 46:1/115 .. mrc'),
        ('where', 'southeast nebraska'),
    ],
}


def specs(rows, accent, width=9):
    """label  facts: the label dim and right-aligned, [words] in white."""
    out = []
    for label, text in rows:
        out.append('  ' + c(accent) + f'{label:>{width}}' + c(DIM) + ('  ' if not label else ' :') + ' '
                   + c(GRAY) + re.sub(r'\[([^\]]*)\]', lambda m: c(WHITE) + m.group(1) + c(GRAY), text))
    return out


def fluxbbs_ad():
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
    return out, 'flux/bbs + disflux'


# The second ad: the games and gear built into flux/bbs, then disflux,
# where they all run.
FUN = [
    ('the casino', PINK, MAG, 12, [
        ('slots', 'three reels in half-block pixels, 24-bit color, a jackpot'),
        ('tables', 'video poker, blackjack, roulette, craps'),
        ('the bar', 'drinks .. and the players club'),
        ('chips', 'a free stack every day .. winnings paid in [file points]'),
        ('', 'and [minutes]'),
    ]),
    ('doors', CYAN, TEAL, 12, [
        ('carrier lost', 'war-dial a dead exchange in 1994. lock a carrier,'),
        ('', 'catch the handshake, cash out before the trace hits'),
        ('', '100. one high score board for everyone'),
        ('tarot', 'card of the day, three-card readings, fortune cookies'),
        ('dos doors', 'lord, tw2002, bre, overkill ii run inside, no dosbox'),
        ('ghostwire', 'cyberpunk, multiplayer, persistent: fixers, netruns,'),
        ('', 'bosses, other runners robbing you. [disflux only, alpha]'),
    ]),
    ('new school', PINK, MAG, 12, [
        ('ai chat', '[free]. knows the board, searches the web'),
        ('points', 'up votes and doors pay .. tip anyone .. nightly lottery'),
        ('boards', 'hot threads, votes, comments, @mentions, run your own'),
        ('mrc', 'live chat with other boards'),
        ('file search', 'google-style: matches drop down as you type, "did you'),
        ('', 'mean", the hits lit up'),
        ('art gallery', 'the scene\'s packs opened in place, ansi played at modem'),
        ('', 'speed, gifs, pictures and pdfs right in your terminal'),
        ('savers', 'digital rain, a warp starfield, demoscene fire'),
    ]),
]


def fun_ad():
    out = ['']
    out += logo('NEUROSIX.TDF', 'Neurosis 1 P', 'flux/bbs')
    out += ['', center((GRAY, 'all of this is built in')), '']
    for title, accent, mid, width, rows in FUN:
        out += [rule(title, accent, mid), '']
        out += specs(rows, accent, width)
        out += ['']
    out += [center((DIM, 'flux/bbs '), (WHITE, 'v1.1'), (DIM, ' .. rust .. mit .. linux + docker .. '), (WHITE, 'github.com/phrac/fluxbbs')), '']
    out += [rule('play it on', CYAN, TEAL), '']
    out += logo('ROYSAC2C.TDF', 'RoySAC2Mgn', 'disflux')
    out += ['', center((GRAY, 'a private h/p/a/v/c system'), (DIM, ' .. '), (GRAY, '8 nodes'), (DIM, ' .. '), (GRAY, '68,000+ files')), '',
            center((DIM, 'ssh '), (WHITE, 'disfluxbbs.com'), (DIM, ':'), (WHITE, '2222'), (DIM, '  ..  telnet '), (WHITE, 'disfluxbbs.com'),
                   (DIM, '  ..  sysop '), (PINK, 'phrac')),
            '', rule('', CYAN, TEAL)]
    return out, 'flux/bbs + disflux: games and gear'


# Revision 6: games and gear again, flux/bbs in HardWired, the copy
# cut to lists.
GAMES = [
    ('casino', PINK, MAG, 9, [
        ('tables', 'slots .. video poker .. blackjack .. roulette .. craps'),
        ('also', 'the bar .. players club'),
        ('pays', 'file points + time .. free chips daily'),
    ]),
    ('doors', CYAN, TEAL, 9, [
        ('ours', 'carrier lost (war-dialer) .. tarot .. ghostwire (alpha)'),
        ('dos', 'lord .. tw2002 .. bre .. overkill ii .. food fight'),
        ('', 'run inside the bbs. no dosbox'),
        ('native', 'usurper'),
        ('online', 'bbslink'),
    ]),
    ('more', PINK, MAG, 9, [
        ('ai', 'free ai chat'),
        ('msgs', 'votes + comments on posts .. @mentions .. caller subs'),
        ('files', 'search w/ typeahead .. art gallery .. ansi/gif/pdf view'),
        ('nets', 'ftn (agoranet here) .. mrc .. qwk'),
        ('misc', 'points, tips, lottery .. screen savers'),
    ]),
]


def games_ad():
    out = ['']
    out += logo('HWPLATED.TDF', 'HardWired Pl', 'flux/bbs')
    out += ['', center((WHITE, 'v1.1'), (DIM, ' .. '), (GRAY, 'rust'), (DIM, ' .. '), (GRAY, 'mit'), (DIM, ' .. '), (GRAY, 'linux, arm, docker'),
                       (DIM, ' .. '), (WHITE, 'github.com/phrac/fluxbbs')), '']
    for title, accent, mid, width, rows in GAMES:
        out += [rule(title, accent, mid), '']
        out += specs(rows, accent, width)
        out += ['']
    out += [rule('running on', CYAN, TEAL), '']
    out += logo('ROYSAC2C.TDF', 'RoySAC2Mgn', 'disflux')
    out += ['', center((GRAY, 'h/p/a/v/c'), (DIM, ' .. '), (GRAY, '8 nodes'), (DIM, ' .. '), (GRAY, '68k files'), (DIM, ' .. '),
                       (GRAY, 'sysop '), (PINK, 'phrac')),
            center((DIM, 'ssh '), (WHITE, 'disfluxbbs.com'), (DIM, ':'), (WHITE, '2222'), (DIM, '  ..  telnet '), (WHITE, 'disfluxbbs.com')),
            '', rule('', CYAN, TEAL)]
    return out, 'flux/bbs + disflux'


# Revision 7: 03's look (it's the best), its spec sheets grown to the
# games and the new school things.
SOFTWARE7 = [
    ('version', '[v1.1] .. rust .. mit license'),
    ('runs on', 'linux x86_64 + arm64, static .. or docker'),
    ('connect', 'ssh, telnet .. cp437, utf-8, 24-bit color'),
    ('msgs', 'threaded bases w/ voting, @mentions, caller subs, qwk,'),
    ('', 'vim keys in the fse'),
    ('nets', 'ftn over binkp: echomail, netmail, tic .. mrc'),
    ('files', 'search w/ typeahead .. art gallery .. ansi/gif/pdf viewer'),
    ('doors', 'door.sys, door32.sys, bbslink .. dos doors run inside,'),
    ('', 'no dosbox'),
    ('games', '[casino]: slots, video poker, blackjack, roulette, craps'),
    ('', '[carrier lost] .. [tarot]'),
    ('extras', 'free ai chat .. points, tips, lottery .. screen savers'),
    ('scripts', 'python'),
    ('imports', 'iniquity 1.00a26r5 boards, as they are'),
]

BOARD7 = dict(BOARD, specs=[
    ('sysop', '[phrac] .. co-sysop subcarrier'),
    ('nodes', '8 .. ssh [disfluxbbs.com:2222] .. telnet [disfluxbbs.com]'),
    ('files', '68,000+ .. all of textfiles.com, 16colo.rs packs,'),
    ('', 'phrack, 2600'),
    ('doors', 'lord, tw2002, bre, usurper, overkill ii, food fight,'),
    ('', '[ghostwire] (ours, alpha)'),
    ('ai', 'penny, free'),
    ('nets', 'agoranet 46:1/115 .. mrc'),
    ('where', 'southeast nebraska'),
])


def spec7_ad():
    out = ['']
    out += logo('ROYSAC2C.TDF', 'RoySAC2Ppl', 'flux/bbs')
    out += ['']
    out += specs(SOFTWARE7, PINK)
    out += ['', center((DIM, 'github.com/'), (WHITE, 'phrac/fluxbbs'), (DIM, '  ..  ghcr.io/'), (WHITE, 'phrac/fluxbbs')), '']
    out += [rule('world hq', CYAN, TEAL), '']
    out += logo(*BOARD7['font'], BOARD7['name'])
    out += ['', center(*BOARD7['tag']), '']
    out += specs(BOARD7['specs'], CYAN)
    out += ['', rule('', CYAN, TEAL)]
    return out, 'flux/bbs + disflux'


# For echomail: a message reader takes a line at a time, and some drop
# the colors from lines much over 80 characters (Pharcyde's did, 1 Nov
# 2026, with 07's 589-byte logo rows). So a line is laid out as cells
# and written again with as few codes as it takes: a color only when it
# changes, the shortest code that gets there, and cursor-forward over
# runs of blanks. ECHO_MAX is the most a line may take, codes and all.
ECHO_MAX = 79


def cells(line):
    """An ANSI line (SGR codes only) as cells of (char, fg, bg)."""
    out, fg, bg, bold = [], 7, 0, False
    for m in re.finditer(r'\x1b\[([0-9;]*)m|(.)', line, re.S):
        if m.group(2) is not None:
            out.append((m.group(2), fg | (8 if bold else 0), bg))
            continue
        for p in (m.group(1) or '0').split(';'):
            n = int(p or 0)
            if n == 0:
                fg, bg, bold = 7, 0, False
            elif n == 1:
                bold = True
            elif 30 <= n <= 37:
                fg = pre.ANSI.index(n - 30)
            elif 40 <= n <= 47:
                bg = pre.ANSI.index(n - 40)
    return out


def compact(row):
    """Cells as short ANSI: a blank (a space on black) takes no color, runs
    of blanks are a cursor move, and each change takes the least code."""
    while row and row[-1][0] == ' ' and row[-1][2] == 0:
        row = row[:-1]
    out, fg, bg, i = '', 7, 0, 0
    while i < len(row):
        ch, f, b = row[i]
        if ch == ' ' and b == 0:
            j = i
            while j < len(row) and row[j][0] == ' ' and row[j][2] == 0:
                j += 1
            n = j - i
            out += ' ' * n if n < 4 else f'\x1b[{n}C'
            i = j
            continue
        if (f, b) != (fg, bg):
            parts = []
            reset = (fg > 7 and f < 8) or (bg != 0 and b == 0)
            if reset:
                parts.append('0')
                fg, bg = 7, 0
            if f > 7 and fg < 8:
                parts.append('1')
            if (f & 7) != (fg & 7):
                parts.append('3%d' % pre.ANSI[f & 7])
            if b != bg:
                parts.append('4%d' % pre.ANSI[b])
            out += '\x1b[' + ';'.join(parts) + 'm'
            fg, bg = f, b
        out += ch
        i += 1
    return out + ('\x1b[0m' if (fg, bg) != (7, 0) else '')


def for_echomail(lines):
    """The lines written short, and how long the longest is."""
    out = [compact(cells(l)) for l in lines]
    return out, max(len(l) for l in out)


def echo_logo(file, face, word, grad, spacing=None):
    """A logo that fits echomail's lines: each row in one color (grad, top
    to bottom), the font's gray shading drawn as \u2591 in that color, so
    a row needs one color code and keeps its shape."""
    font = next(f for f in pre.read_tdf(os.path.join(HERE, 'fonts', file)) if f['name'] == face)
    rows = [r for r in pre.set_word(font, word, font['spacing'] if spacing is None else spacing, {}, {}) if any(r)]
    lead = (80 - max(len(r) for r in rows)) // 2
    out = []
    for i, r in enumerate(rows):
        col = grad[min(i * len(grad) // len(rows), len(grad) - 1)]
        cl = [(' ', 7, 0)] * lead
        for cell in r:
            if cell is None:
                cl.append((' ', 7, 0))
                continue
            ch, fg, bg = cell
            if ch in (0, 32):
                cl.append(((LIGHT if bg == 7 else FULL), col, 0) if bg else (' ', 7, 0))
                continue
            g = bytes([ch]).decode('cp437')
            cl.append((LIGHT if fg in (7, 8) and g in FULL + DARK + MEDIUM else g, col, 0))
        out.append(compact(cl))
    return out


FULL, DARK, MEDIUM, LIGHT = '\u2588', '\u2593', '\u2592', '\u2591'


def echo_specs(rows, accent, width=9):
    """Spec rows for echomail: the text rewrapped so no line, codes and
    all, is over ECHO_MAX."""
    out = []
    for label, text in rows:
        words, lab = text.split(' '), label
        while words:
            n = len(words)
            while n > 1 and len(compact(cells(specs([(lab, ' '.join(words[:n]))], accent, width)[0]))) > ECHO_MAX:
                n -= 1
            out.append(compact(cells(specs([(lab, ' '.join(words[:n]))], accent, width)[0])))
            words, lab = words[n:], ''
    return out


def echo_rule(title='', accent=PINK, mid=MAG):
    """A shorter rule for echomail: 40 columns, centered, two colors."""
    if not title:
        return compact(cells(' ' * 20 + c(mid) + '─' * 40))
    side = (40 - len(title) - 4) // 2
    line = ' ' * 20 + c(mid) + '─' * side + '[ ' + c(WHITE) + title + c(mid) + ' ]' + '─' * (40 - side - len(title) - 4)
    return compact(cells(line))


# 07's spec rows split by hand for echomail's short lines.
SOFTWARE8 = [
    ('version', '[v1.1] .. rust .. mit license'),
    ('runs on', 'linux x86_64 + arm64, static .. docker'),
    ('connect', 'ssh, telnet .. cp437, utf-8, 24-bit color'),
    ('msgs', 'threaded bases w/ voting, @mentions,'),
    ('', 'caller subs, qwk, vim keys in the fse'),
    ('nets', 'ftn over binkp: echomail, netmail, tic'),
    ('', 'mrc chat'),
    ('files', 'search w/ typeahead .. art gallery'),
    ('', 'ansi, gif and pdf viewer'),
    ('doors', 'door.sys, door32.sys, bbslink'),
    ('', 'dos doors run inside. no dosbox'),
    ('games', '[casino]: slots, video poker, blackjack,'),
    ('', 'roulette, craps .. [carrier lost] .. [tarot]'),
    ('extras', 'free ai chat .. points, tips, lottery'),
    ('', 'screen savers'),
    ('scripts', 'python'),
    ('imports', 'iniquity 1.00a26r5 boards, as they are'),
]

BOARD8 = [
    ('sysop', '[phrac] .. co-sysop subcarrier'),
    ('nodes', '8 .. ssh [disfluxbbs.com:2222]'),
    ('', 'telnet [disfluxbbs.com]'),
    ('files', '68,000+ .. all of textfiles.com,'),
    ('', '16colo.rs packs, phrack, 2600'),
    ('doors', 'lord, tw2002, bre, usurper, overkill ii,'),
    ('', 'food fight, [ghostwire] (ours, alpha)'),
    ('ai', 'penny, free'),
    ('nets', 'agoranet 46:1/115 .. mrc'),
    ('where', 'southeast nebraska'),
]


def echo8_ad():
    """Revision 8: 07 for echomail, every line at most ECHO_MAX."""
    out = ['']
    out += echo_logo('ROYSAC2C.TDF', 'RoySAC2Ppl', 'flux/bbs', [PINK, PINK, PINK, MAG, MAG, MAG], spacing=0)
    out += ['']
    out += echo_specs(SOFTWARE8, PINK)
    out += ['', compact(cells(center((DIM, 'github.com/'), (WHITE, 'phrac/fluxbbs')))),
            compact(cells(center((DIM, 'ghcr.io/'), (WHITE, 'phrac/fluxbbs')))), '']
    out += [echo_rule('world hq', CYAN, TEAL), '']
    out += echo_logo('AMNESIAX.TDF', 'Amnesia Cyan', 'disflux', [WHITE, CYAN, CYAN, CYAN, TEAL, TEAL, TEAL])
    out += ['', compact(cells(center(*BOARD7['tag']))), '']
    out += echo_specs(BOARD8, CYAN)
    out += ['', echo_rule('', CYAN, TEAL)]
    return out, 'flux/bbs + disflux'


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else 'fluxbbs'
    out, title = {'fluxbbs': fluxbbs_ad, 'fun': fun_ad, 'games': games_ad, 'spec7': spec7_ad, 'echo8': echo8_ad}[which]()
    for line in out:
        w = len(re.sub(r'\x1b\[([0-9]*)C', lambda m: ' ' * int(m.group(1) or 1), re.sub(r'\x1b\[[0-9;]*m', '', line)))
        if w >= 80:
            sys.exit(f'{w} columns: {line!r}')
        if which.startswith('echo') and len(line) > ECHO_MAX:
            sys.exit(f'{len(line)} bytes, over {ECHO_MAX} for echomail: {line!r}')
    data = ('\r\n'.join(out) + E + '0m\r\n').encode('cp437')
    sys.stdout.buffer.write(data + sauce(len(data), len(out), title))


def sauce(size, lines, title):
    """A SAUCE record: what the ad is, who made it, and that it's 80 columns."""
    def f(s, n):
        return s.encode('cp437')[:n].ljust(n, b' ')
    import datetime
    rec = (b'SAUCE00' + f(title, 35) + f('phrac', 20) + f('disflux', 20)
           + datetime.date.today().strftime('%Y%m%d').encode() + size.to_bytes(4, 'little')
           + bytes([1, 1]) + (80).to_bytes(2, 'little') + lines.to_bytes(2, 'little') + bytes(4)
           + bytes([0, 0]) + f('IBM VGA', 22).replace(b' ', b'\0'))
    assert len(rec) == 128, len(rec)
    return b'\x1a' + rec


if __name__ == '__main__':
    main()
