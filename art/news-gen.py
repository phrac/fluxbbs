"""The news's screens (NW): a newspaper's front page and its back issues.

    python3 -I news-gen.py      (from text/: writes NEWSTOP/MID/HIL/BOT and
                                 NEWSDAYTOP/MID/HIL/BOT)

The masthead is the paper's name ([news] name, else the board's name and
"news") set in Members, in newsprint silver, with a tagline (art/local.py's
NEWS_TAGLINE), a double rule, and the dateline the board fills in: the
issue (~NO), the day (~DA), the stories (~SC) and when it was updated
(~UP). Each story takes two rows: whether it's unread (~RD), its section
(~SE, colored by the board), the headline (~HL), its sources (~NS) and
votes (~VT), then a line on what it's about (~OV), indented under the
headline; ~TI is its time, left off here as the edition's head has it.
The stories come in editions (each of the board's runs), each under a
head (NEWSHEAD): its name (~ED), how many are unread (~UN) and the rule
to the edge (~RL)."""
import importlib.util, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import boardname

spec = importlib.util.spec_from_file_location('mainmenu_gen', os.path.join(HERE, 'mainmenu-gen.py'))
mg = importlib.util.module_from_spec(spec)
sys.argv = sys.argv[:1]
spec.loader.exec_module(mg)

E = '\x1b['
DG = E + '0;1;30m'
GR = E + '0;37m'
WH = E + '0;1;37m'
YL = E + '0;1;33m'
BN = E + '0;33m'


def at(r, c):
    return E + f'{r};{c}H'


def visible(b):
    return len(re.sub(rb'\x1b\[[0-9;]*[A-Za-z]', b'', b).decode('cp437'))


def paper_name():
    name = (boardname.CONFIG.get('news', {}).get('name') or '').strip()
    return name or f'{boardname.NAME} news'


def masthead():
    """The paper's name in Members, newsprint silver, centered; too wide
    and it's the board's shorter names and "news", then just "news"."""
    names = [paper_name()] + [f'{n} news' for n in boardname.NAMES[1:] if n] + ['news']
    opts = {'spacing': 1, 'recolor': {10: 15, 2: 7}, 'fade': ((255, 255, 255), (120, 120, 120))}
    for word in names:
        word = word.upper()
        try:
            art = mg.tdf_logo('MEMBERS.TDF', 'Members', word, 0, opts)
        except (KeyError, ValueError, StopIteration):
            continue
        if all(visible(l) <= 80 for l in art.split(b'\r\n')):
            return art
    return mg.tdf_logo('MEMBERS.TDF', 'Members', 'NEWS', 0, opts)


def double_rule(r):
    # Heavy in the middle, fading to the edges, as a printed rule.
    n = 78
    cs = [WH if 12 <= i < n - 12 else GR if 4 <= i < n - 4 else DG for i in range(n)]
    return at(r, 2) + ''.join(c + '═' for c in cs)


def thin_rule(r):
    return at(r, 2) + DG + '─' * 78


def prompt_bar(name):
    """The keys (~HP) and the prompt bar with the cursor's slot (~PC), as
    the boards' lists have; the bar is the last row."""
    HIW = E + '0;1;37;43m'
    out = ' ' + YL + '»' + BN + '»' + DG + '» ' + WH + '~HP' + E + '0m\r\n'
    s = ' ' + DG + '░' + BN + '▒' + YL + '▓' + HIW + ' ' + name + ' ' + YL + '▓' + BN + '▒' + DG + '░ ' + BN + '(' + YL + ' ~PC      ' + BN + ') '
    used = 1 + 3 + len(name) + 2 + 3 + 1 + 1 + 1 + 6 + 1 + 1
    stats = DG + '─[ ' + WH + '%TL' + GR + ' min left' + DG + ' ]─[ ' + GR + 'node ' + WH + '%ND' + DG + ' ]─[ ' + WH + '%TM' + DG + ' ]'
    return out + s + DG + '─' * (79 - 36 - used) + '%>' + stats + E + '0m'


def tagline():
    return boardname.local('NEWS_TAGLINE', 'all the news that fits in eighty columns')


def head(title_row, rule=True):
    """The masthead, the tagline, the double rule, the title row and
    (with `rule`) a thin rule under it."""
    art = masthead()
    rows = art.count(b'\r\n')
    t = tagline()
    out = b'\x1b[0m\x1b[2J\x1b[H' + art
    r = rows + 1
    out += (at(r, max(1, (80 - len(t) - 4) // 2)) + DG + '~ ' + GR + t + DG + ' ~').encode('cp437')
    out += (double_rule(r + 1)).encode('cp437')
    out += title_row(r + 2).encode('cp437')
    out += ((thin_rule(r + 3) + at(r + 4, 1)) if rule else at(r + 3, 1)).encode('cp437') + (E + '0m').encode('cp437')
    return out


def front_dateline(r):
    left = at(r, 2) + YL + 'NO. ' + WH + '{05~NO'
    mid = at(r, 22) + WH + '{36~DA'
    right = at(r, 58) + WH + '~SC' + GR + ' stories' + DG + ' · ' + GR + 'at ' + WH + '~UP'
    return left + mid + right


def back_dateline(r):
    return at(r, 2) + YL + 'BACK ISSUES' + at(r, 58) + WH + '~SC' + GR + ' days kept'


def screens():
    out = {}
    # The front page's editions under heads of their own, not a rule.
    out['NEWSTOP'] = head(front_dateline, rule=False)
    # (The time is the edition's, on its head.)
    cols = [(2, '~RD', 1, ''), (4, '~SE', 8, ''), (13, '~HL', 54, GR), (68, '~NS', 5, BN), (74, '~VT', 6, YL)]
    hcols = [(2, '~RD', 1, ''), (4, '~SE', 8, ''), (13, '~HL', 54, WH), (68, '~NS', 5, YL), (74, '~VT', 6, YL)]
    row = lambda cs: ''.join(f'@{c:02d}' + st + f'{{{w:02d}' + f for c, f, w, st in cs)
    over = '\r\n' + E + '0m' + '@13' + DG + '{66~OV'
    out['NEWSMID'] = (E + '255D' + E + '0m' + row(cols) + over + E + '0m\r\n').encode('cp437')
    out['NEWSHIL'] = (E + '255D' + E + '0m' + YL + '\x10' + row(hcols) + over + E + '0m\r\n').encode('cp437')
    out['NEWSHEAD'] = (E + '255D' + E + '0m' + '@02' + DG + '\u2500\u2500 ' + YL + '~ED' + BN + '~UN ' + DG + '~RL' + E + '0m\r\n').encode('cp437')
    out['NEWSBOT'] = (E + '255D' + E + '0m ' + DG + '─' * 78 + E + '0m\r\n' + prompt_bar('the news')).encode('cp437')
    out['NEWSDAYTOP'] = head(back_dateline)
    out['NEWSDAYMID'] = (E + '255D' + E + '0m' + '@04' + GR + '{30~DA' + '@36' + DG + '{20~SC' + E + '0m\r\n').encode('cp437')
    out['NEWSDAYHIL'] = (E + '255D' + E + '0m' + YL + '\x10' + '@04' + WH + '{30~DA' + '@36' + GR + '{20~SC' + E + '0m\r\n').encode('cp437')
    out['NEWSDAYBOT'] = (E + '255D' + E + '0m ' + DG + '─' * 78 + E + '0m\r\n' + prompt_bar('back issues')).encode('cp437')
    return out


for name, data in screens().items():
    open(name + '.ANS', 'wb').write(data)
