#!/usr/bin/env python3
"""The plain ASCII ad for flux/bbs and disflux (7-bit, 79 columns at most,
so it travels on any echo and shows on any terminal): the names in
figlet's "standard" letters over the spec sheets.

    python3 art/ad-ascii.py art/ads/FLUXADnn.ASC

Like the ANSI ads, each revision is a new numbered file; old ones stay.
"""
import sys

# figlet "standard", five rows a letter.
FONT = {
    'd': ["     _ ", "  __| |", " / _` |", "| (_| |", " \\__,_|"],
    'i': [" _ ", "(_)", "| |", "| |", "|_|"],
    's': ["     ", " ___ ", "/ __|", "\\__ \\", "|___/"],
    'f': ["  __ ", " / _|", "| |_ ", "|  _|", "|_|  "],
    'l': [" _ ", "| |", "| |", "| |", "|_|"],
    'u': ["       ", " _   _ ", "| | | |", "| |_| |", " \\__,_|"],
    'x': ["      ", "__  __", "\\ \\/ /", " >  < ", "/_/\\_\\"],
    'b': [" _     ", "| |__  ", "| '_ \\ ", "| |_) |", "|_.__/ "],
    '/': ["    __", "   / /", "  / / ", " / /  ", "/_/   "],
}


def word(w):
    return [''.join(FONT[c][r] for c in w).rstrip() for r in range(5)]


def centered(lines, width=79):
    w = max(len(l) for l in lines)
    pad = (width - w) // 2
    return [(' ' * pad + l).rstrip() for l in lines]


def rule(label=''):
    if not label:
        return '-' * 79
    s = f'[ {label} ]'
    left = (79 - len(s)) // 2
    return '-' * left + s + '-' * (79 - left - len(s))


def sheet(rows):
    out = []
    for k, v in rows:
        out.append(f'  {k:>8} : {v}' if k else (f'  {"":>8}   {v}' if v else ''))
    return out


lines = ['']
lines += centered(word('flux/bbs'))
lines += ['', centered(['the bbs software. rust. mit.'])[0], '']
lines += sheet([
    ('version', 'v1.4 .. linux x86_64/arm64 .. docker'),
    ('connect', 'ssh .. telnet .. cp437/utf-8 .. 24-bit color'),
    ('msgs', 'threads .. votes .. @mentions .. caller-run boards'),
    ('', 'google-style search .. qwk .. full-screen editor'),
    ('nets', 'ftn/binkp mailer + tosser .. echomail .. netmail'),
    ('', 'tic file echoes .. mrc chat'),
    ('files', 'typeahead search .. votes/comments .. ansi/gif/pdf'),
    ('doors', 'door.sys .. door32.sys .. bbslink'),
    ('', 'dos doors native, no dosbox'),
    ('extras', 'reading room .. casino .. tarot .. weather .. ai'),
    ('imports', 'iniquity 1.00a26r5'),
    ('', ''),
    ('get it', 'github.com/phrac/fluxbbs'),
    ('docker', 'ghcr.io/phrac/fluxbbs'),
])
lines += ['', rule('world hq'), '']
lines += centered(word('disflux'))
lines += ['', centered(['a private h/p/a/v/c system'])[0], '']
lines += sheet([
    ('ssh', 'disfluxbbs.com port 2222'),
    ('telnet', 'disfluxbbs.com port 23'),
    ('nodes', '8'),
    ('sysop', 'phrac .. co: subcarrier'),
    ('files', '68k+ .. textfiles.com .. 16colo.rs .. phrack'),
    ('doors', 'lord .. tw2002 .. bre .. usurper .. oo2'),
    ('', 'food fight .. ghostwire (ours)'),
    ('nets', 'agoranet 46:1/115 .. sysopnet 23:100/14 .. mrc'),
])
lines += ['', rule(), '']

text = '\r\n'.join(lines) + '\r\n'
assert all(len(l) <= 79 for l in lines), [l for l in lines if len(l) > 79]
assert text.isascii()
if len(sys.argv) > 1:
    open(sys.argv[1], 'w', newline='').write(text)
else:
    sys.stdout.write(text.replace('\r\n', '\n'))
