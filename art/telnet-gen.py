"""TELNET.ANS: what a caller who came in over telnet sees after PRELOGON,
before the login. The board's name in the prelogon's own TheDraw font,
then a box saying telnet is unencrypted and how to call back over SSH,
and the question the board waits on (Y goes on, N hangs up).

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


def main():
    file, face, spacing, fg_map, bg_map = pre.VARIANTS[0]
    path = os.path.join(HERE, 'fonts', file)
    fonts = [f for f in pre.read_tdf(path) if f['name'] == face]
    if not fonts:
        sys.exit('%s has no font "%s"' % (path, face))
    for name in boardname.NAMES:
        if any(c != ' ' and c not in fonts[0]['glyphs'] for c in name) and name != boardname.NAMES[-1]:
            continue
        word = pre.set_word(fonts[0], name, spacing, fg_map, bg_map)
        if max(len(r) for r in word) <= 80:
            break
    pre.LINES = lines()
    # The box in the warning's colors: red at the top to yellow.
    pre.BOX_COLORS = [RED, RED, 4, 4, 6, 6, 6, YELLOW, YELLOW]
    pre.ARM = RED
    out = pre.screen(word)
    # The board asks the question itself: no "press a key" at the end, the
    # cursor left after the prompt.
    out = out.removesuffix(b'|PK')
    sys.stdout.buffer.write(out)


if __name__ == '__main__':
    main()
