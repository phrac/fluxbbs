"""Make a board's screens and lightbar menus, all of them:

    python3 art/make.py [BOARD]

BOARD is the board's directory (its config names it in the logos; the
screens go to its text/ and the menus to its menus/); without one, this
repo's own text/ and menus/, the stock set that says FLUX/BBS. Run it
again after renaming the board (*C, I and K) or changing art/local.py.
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)

# (file the output goes to, or None for a generator that writes its own;
# the generator and its arguments).
SCREENS = [
    ('ABOUT.ANS', ['about-gen.py']),
    ('AICHAT.ANS', ['aichat-gen.py']),
    ('BOTCHECK.ANS', ['botcheck-gen.py']),
    (None, ['fseditor-gen.py']),
    ('GALLERY.ANS', ['gallery-gen.py']),
    ('LISTFILE.ANS', ['listgen.py', 'file']),
    (None, ['listsgen.py']),
    ('MATRIX.ANS', ['logon-gen.py']),
    ('../menus/MATRIX.toml', ['matrix-menu-gen.py']),
    ('MSGGUIDE.ANS', ['msgguide-gen.py']),
    ('MSGNEWS.ANS', ['msgguide-gen.py', 'news']),
    ('NODECHAT.ANS', ['nodechat-gen.py', 'screen']),
    ('POINTS.ANS', ['points-gen.py']),
    (None, ['prelogon-gen.py']),
    ('USERSTAT.ANS', ['stats-gen.py']),
    ('USERCFG.ANS', ['usercfg-gen.py']),
    (None, ['welcome-gen.py']),
]
MENUS = ['main', 'more', 'file', 'message', 'sysop', 'door', 'bbslink', 'tarot', 'email', 'automsg', 'autosig',
         'bbslist', 'multinod', 'qwkmail', 'voting', 'timebank']
MAIN_VARIANTS = 8


def main():
    board = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else REPO
    env = dict(os.environ, FLUX_BOARD=board)
    text = os.path.join(board, 'text')
    os.makedirs(text, exist_ok=True)
    os.makedirs(os.path.join(board, 'menus'), exist_ok=True)
    sys.path.insert(0, HERE)
    os.environ['FLUX_BOARD'] = board
    import boardname
    menus = MENUS + [m for m in boardname.local('MENUS', {}) if m not in MENUS]
    steps = list(SCREENS)
    steps += [(m.upper() + '.ANS', ['mainmenu-gen.py', m]) for m in menus]
    steps += [(f'MAIN.AN{v}', ['mainmenu-gen.py', 'main', str(v)]) for v in range(1, MAIN_VARIANTS + 1)]
    steps.append(('DOORSTAT.ANS', ['mainmenu-gen.py', 'doorstat']))
    steps.append(('STOCK.ANS', ['mainmenu-gen.py', 'stock']))
    # Screens the board keeps as they are (its art/local.py's KEEP): its
    # own hand-drawn STOCK.ANS, say.
    keep = set(boardname.local('KEEP', []))
    steps = [s for s in steps if s[0] not in keep]
    for out, (script, *args) in steps:
        cmd = [sys.executable, '-I', os.path.join(HERE, script), *args]
        r = subprocess.run(cmd, cwd=text, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if r.returncode != 0:
            sys.exit(f'{script} {" ".join(args)}: {r.stderr.decode(errors="replace").strip()}')
        if out:
            with open(os.path.join(text, out), 'wb') as f:
                f.write(r.stdout)
    print(f'{len(steps)} made for {boardname.NAME} in {board}')


if __name__ == '__main__':
    main()
