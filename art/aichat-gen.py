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
ACCENT = '34'
RD = E + '0;' + ACCENT + 'm'
BR = E + '0;1;' + ACCENT + 'm'


def goto(r, c):
    return E + f'{r};{c}H'


def codes_row(row):
    rnd = random.Random(2049)
    out = goto(row, 3)
    for g in range(15):
        for _ in range(4):
            ch = rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789')
            out += (BR if rnd.random() < 0.12 else DG) + ch
        if g < 14:
            out += ' '
    return out


def main():
    cfg = mg.boardname.CONFIG
    name = cfg.get('ai', {}).get('name', 'ai') or 'ai'
    ink = mg.Ink(ACCENT)
    logo = mg.tdf_logo('BROTHOOD.TDF', 'Brotherhood', name.upper(), 0, {'spacing': 1})
    rows = logo.count(b'\n')
    out = b'\x1b[0m\x1b[2J\x1b[H' + logo + (codes_row(rows + 1) + E + '0m').encode('cp437')
    # The rule: the chat's name, and a word on the right.
    r = rows + 2
    left = DG + '[ ' + ink.fade_text('ai chat') + ' ' + DG + ']'
    right = DG + '[ ' + ink.fade_text('it forgets you when you leave') + ' ' + DG + ']'
    rule = ''.join((BR if i in (0, 77) else RD if min(i, 77 - i) < 4 else DG) + '─' for i in range(78))
    out += (goto(r, 2) + rule + goto(r, 4) + left + goto(r, 79 - 33) + right).encode('cp437')
    help_line = 'type a message and press enter. /help explains, /quit leaves.'
    out += (goto(r + 1, 2) + GR + help_line + E + '0m' + goto(r + 2, 1)).encode('cp437')
    sys.stdout.buffer.write(out)
    fx = ['# AICHAT.ANS animated while the chat waits (fluxbbs [[effect]]).',
          '# Made by art/aichat-gen.py; change that, not this.', '',
          '[[effect]]', 'kind = "random"', f'rows = [1, {rows}]', 'every = 10.0', 'delay = 2.0', '',
          '[[effect]]', 'kind = "decode"', f'rows = [{rows + 1}, {rows + 1}]', 'cols = [3, 76]', 'every = 10.0', 'delay = 2.0', 'amount = 1.8',
          'messages = [' + ', '.join('"%s"' % m for m in AI) + ']', '',
          '[[effect]]', 'kind = "ripple"', f'rows = [{r}, {r}]', 'every = 10.0', 'delay = 6.0', '',
          '[[effect]]', 'kind = "type"', f'rows = [{r + 1}, {r + 1}]', f'cols = [2, {2 + len(help_line)}]', 'every = 30.0', 'delay = 4.0', 'amount = 0.4', 'tint = "#5555ff"']
    open(os.path.join(os.getcwd(), 'AICHAT.FX'), 'w').write('\n'.join(fx) + '\n')


if __name__ == '__main__':
    main()
