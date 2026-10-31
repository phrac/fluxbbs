"""TELNET.ANS: what a caller who came in over telnet sees after PRELOGON,
before the login. WARNING in the prelogon's own TheDraw font, in red,
then a box saying telnet is unencrypted and how to call back over SSH,
and the question the board waits on (Y goes on, N hangs up).

It also writes TELNET.FX beside it: the word flashing while the board
waits (blink in a screen file means iCE's bright backgrounds here, so
the flashing is the board's).

The SSH address in it is the board's art/local.py's TELNET_SSH if it has
one, else [mrc] info_ssh, else the host in [web] download_url with the
port from [ssh] listen.

Run from text/: python3 -I ../art/telnet-gen.py
"""
import importlib.util
import os
import sys
from urllib.parse import urlparse

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import boardname  # noqa: E402

spec = importlib.util.spec_from_file_location('prelogon', os.path.join(HERE, 'prelogon-gen.py'))
pre = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pre)

PINK, CYAN, GRAY, DIM, WHITE, YELLOW, RED = 13, 11, 7, 8, 15, 14, 12


def ssh_address():
    """(host, port) for SSH: the host may be blank."""
    cfg = boardname.CONFIG
    given = boardname.local('TELNET_SSH', '') or cfg.get('mrc', {}).get('info_ssh', '')
    if given:
        host, _, port = given.rpartition(':') if ':' in given else (given, '', '22')
        return host, port
    listen = cfg.get('ssh', {}).get('listen', '0.0.0.0:2222')
    port = listen.rpartition(':')[2] or '2222'
    host = urlparse(cfg.get('web', {}).get('download_url', '')).hostname or ''
    return host, port


def lines():
    host, port = ssh_address()
    cmd = f'ssh -p {port} {host}' if port != '22' else f'ssh {host}'
    call = [(GRAY, 'call back over '), (WHITE, 'SSH'), (GRAY, ' instead'), (DIM, ' .. ')]
    call += [(CYAN, cmd)] if host else [(GRAY, 'port '), (CYAN, port)]
    return [
        (16, [(RED, '!! '), (YELLOW, "you're connected over TELNET"), (RED, ' !!')]),
        (18, [(GRAY, 'telnet is plain text: your password and all you type')]),
        (19, [(GRAY, 'cross the net unencrypted, for anyone on the way to read.')]),
        (21, call),
        (22, [(GRAY, 'syncterm: connection type '), (WHITE, 'SSH'), (GRAY, ', port '), (WHITE, port), (DIM, ' .. '), (GRAY, 'use a current build')]),
        (24, [(PINK, '[ '), (WHITE, 'Y'), (GRAY, ' go on over telnet'), (DIM, ' .. '), (WHITE, 'N'), (GRAY, ' hang up'), (PINK, ' ]')]),
    ]


def warning(font, spacing):
    """WARNING in the font, each letter's blank edge columns trimmed so
    it fits 80 columns; every cell red (bright where the font is bright)."""
    letters = []
    for ch in 'WARNING':
        rows = pre.set_word(font, ch, 0, {}, {})
        used = [i for i in range(max(len(r) for r in rows)) if any(i < len(r) and r[i] for r in rows)]
        letters.append([r[used[0]:used[-1] + 1] + [None] * (used[-1] + 1 - len(r)) for r in rows])
    h = max(len(l) for l in letters)
    word = [[] for _ in range(h)]
    for n, l in enumerate(letters):
        w = max(len(r) for r in l)
        for y in range(h):
            row = l[y] if y < len(l) else []
            word[y].extend(row + [None] * (w - len(row)))
            if n < len(letters) - 1:
                word[y].extend([None] * spacing)
    red = lambda c: None if c is None else (c[0], (RED if c[1] > 7 else 4) if c[1] else 0, 4 if c[2] else 0)
    return [[red(c) for c in r] for r in word]


def main():
    file, face, spacing, _, _ = pre.VARIANTS[0]
    path = os.path.join(HERE, 'fonts', file)
    fonts = [f for f in pre.read_tdf(path) if f['name'] == face]
    if not fonts:
        sys.exit('%s has no font "%s"' % (path, face))
    word = warning(fonts[0], spacing)
    if max(len(r) for r in word) > 80:
        sys.exit('WARNING is %d columns wide' % max(len(r) for r in word))
    pre.LINES = lines()
    # The box in the warning's colors: red at the top to yellow.
    pre.BOX_COLORS = [RED, RED, 4, 4, 6, 6, 6, YELLOW, YELLOW]
    pre.ARM = RED
    out = pre.screen(word)
    # The board asks the question itself: no "press a key" at the end, the
    # cursor left after the prompt.
    out = out.removesuffix(b'|PK')
    sys.stdout.buffer.write(out)
    fx = ['# TELNET.ANS: WARNING flashing while the board waits for Y or N',
          '# (fluxbbs [[effect]]). Made by art/telnet-gen.py; change that, not this.', '',
          '[[effect]]', 'kind = "flash"', 'rows = [%d, %d]' % (pre.LOGO_ROW, pre.LOGO_ROW + pre.LOGO_ROWS - 1),
          'cols = [1, 80]', 'every = 1.0', 'delay = 0.0', 'amount = 0.6']
    open(os.path.join(os.getcwd(), 'TELNET.FX'), 'w').write('\n'.join(fx) + '\n')


if __name__ == '__main__':
    main()
