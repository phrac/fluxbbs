#!/usr/bin/env python3
"""The plain-text ads for disflux alone (no word of the software), 79
columns at most and no color: the board's name over its spec sheet, set
in TheDraw's Chaos 2 (CP437 blocks, "high ASCII") or in one of two figlet
faces (7-bit, for echoes that strip the 8th bit).

    python3 art/ad-board.py chaos2   art/ads/FLUXAD14.ASC
    python3 art/ad-board.py graffiti art/ads/FLUXAD15.ASC
    python3 art/ad-board.py larry3d  art/ads/FLUXAD16.ASC

Like the ANSI ads, each revision is a new numbered file; old ones stay.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
W = 79

# figlet's "graffiti" and "larry3d", as pyfiglet sets "disflux".
FIGLET = {
    'graffiti': [
        '    .___.__          _____.__',
        '  __| _/|__| _______/ ____\\  |  __ _____  ___',
        ' / __ | |  |/  ___/\\   __\\|  | |  |  \\  \\/  /',
        '/ /_/ | |  |\\___ \\  |  |  |  |_|  |  />    <',
        '\\____ | |__/____  > |__|  |____/____//__/\\_ \\',
        '     \\/         \\/                         \\/',
    ],
    'larry3d': [
        '  __                 ___  ___',
        " /\\ \\  __          /'___\\/\\_ \\",
        ' \\_\\ \\/\\_\\    ____/\\ \\__/\\//\\ \\    __  __  __  _',
        " /'_` \\/\\ \\  /',__\\ \\ ,__\\ \\ \\ \\  /\\ \\/\\ \\/\\ \\/'\\",
        '/\\ \\L\\ \\ \\ \\/\\__, `\\ \\ \\_/  \\_\\ \\_\\ \\ \\_\\ \\/>  </',
        '\\ \\___,_\\ \\_\\/\\____/\\ \\_\\   /\\____\\\\ \\____//\\_/\\_\\',
        ' \\/__,_ /\\/_/\\/___/  \\/_/   \\/____/ \\/___/ \\//\\/_/',
    ],
}


def tdf_logo(file, face, word):
    """`word` in a TheDraw font, its characters only (colors dropped)."""
    spec = importlib.util.spec_from_file_location('prelogon', os.path.join(HERE, 'prelogon-gen.py'))
    pre = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(pre)
    font = [f for f in pre.read_tdf(os.path.join(HERE, 'fonts', file)) if f['name'] == face][0]
    rows = pre.set_word(font, word, font['spacing'], {}, {})
    return [''.join(' ' if c is None or c[0] < 32 else bytes([c[0]]).decode('cp437') for c in r).rstrip() for r in rows]


def centered(lines):
    pad = (W - max(len(l) for l in lines)) // 2
    return [(' ' * pad + l).rstrip() for l in lines]


def main():
    kind, out = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else None)
    if kind == 'chaos2':
        logo, dash = tdf_logo('CHAOS2X.TDF', 'Chaos 2 Blue', 'disflux'), '\u2500'
    elif kind in FIGLET:
        logo, dash = FIGLET[kind], '-'
    else:
        sys.exit('chaos2, graffiti or larry3d')

    def rule(label=''):
        s = f'[ {label} ]' if label else ''
        left = (W - len(s)) // 2
        return dash * left + s + dash * (W - left - len(s))

    def sheet(rows):
        return [f'  {k:>9} : {v}' if k else (f'  {"":>9}   {v}' if v else '') for k, v in rows]

    lines = [''] + centered(logo)
    lines += ['', centered(['a private h/p/a/v/c system'])[0], '']
    lines += [rule('dial in'), '']
    lines += sheet([
        ('ssh', 'disfluxbbs.com port 2222'),
        ('telnet', 'disfluxbbs.com port 23'),
        ('nodes', '8, open 24/7'),
        ('sysop', 'phrac .. co-sysop subcarrier'),
    ])
    lines += ['', rule('the goods'), '']
    lines += sheet([
        ('files', '68,000+ .. all of textfiles.com .. 16colo.rs packs'),
        ('', 'phrack .. h/p/a/v/c archives .. ansi, gif and pdf viewers'),
        ('doors', 'lord .. tw2002 .. bre .. usurper .. oo2 .. food fight'),
        ('', 'ghostwire (written here)'),
        ('msgs', 'agoranet 46:1/115 .. sysopnet 23:100/14'),
        ('', 'mrc chat .. caller-run boards .. full-screen editor'),
        ('extras', 'reading room .. casino .. tarot .. weather'),
        ('', 'penny flux, the hostess, answers back'),
    ])
    lines += ['', rule('no web. no app. just call.'), '']

    assert all(len(l) <= W for l in lines), [l for l in lines if len(l) > W]
    text = '\r\n'.join(lines) + '\r\n'
    if kind in FIGLET:
        assert text.isascii()
    if out:
        open(out, 'wb').write(text.encode('cp437'))
    else:
        sys.stdout.write(text.replace('\r\n', '\n'))


if __name__ == '__main__':
    main()
