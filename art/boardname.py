"""The board the screens are made for, and its names for the logos.

FLUX_BOARD names the board's directory: its config (fluxbbs.toml, or
iniquity.toml on boards from before the rename) gives the names, and the
generators write into its text/ and menus/ (run them from its text/, or
use art/make.py, which does). Unset, it's this repo, whose stock screens
say FLUX/BBS.

The names are [bbs] name, short_name and super_short_name (two
characters at most), upper-cased. A logo set in a font too wide for the
name falls back to the short name, then the super short one; set them in
*C, Main BBS Configuration (I and K), then make the screens again.

A board's own touches (its doors, the lines under its logo, its about
screen) go in its art/local.py: see local(), and art/README.md.
"""
import importlib.util
import os
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
BOARD = os.path.abspath(os.environ.get('FLUX_BOARD') or REPO)
STOCK = BOARD == REPO
# The stock menus the board's lightbar menus are made from.
CLASSIC_MENUS = os.path.join(REPO, 'classic', 'menus')


def config():
    for name in ('fluxbbs.toml', 'iniquity.toml'):
        try:
            with open(os.path.join(BOARD, name), 'rb') as f:
                return tomllib.load(f)
        except OSError:
            continue
        except tomllib.TOMLDecodeError:
            return {}
    return {}


CONFIG = {} if STOCK else config()


def names():
    if STOCK:
        return ('FLUX/BBS', 'FLUXBBS', 'FB')
    bbs = CONFIG.get('bbs', {})
    return ((bbs.get('name') or 'flux/bbs').strip().upper(), (bbs.get('short_name') or '').strip().upper(),
            (bbs.get('super_short_name') or '').strip().upper()[:2])


def _local():
    path = os.path.join(BOARD, 'art', 'local.py')
    if STOCK or not os.path.isfile(path):
        return None
    spec = importlib.util.spec_from_file_location('board_local', path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


LOCAL = _local()


def art(name):
    """A piece of art the screens are made from: the board's own (in its
    art/) if it has one by that name, else the stock one, else None."""
    for d in ([] if STOCK else [os.path.join(BOARD, 'art')]) + [HERE]:
        p = os.path.join(d, name)
        if os.path.isfile(p):
            return p
    return None


def local(name, default):
    """What the board's art/local.py sets `name` to, else `default`."""
    return getattr(LOCAL, name, default) if LOCAL else default


NAME, SHORT, SUPER = names()
# What to try in turn, longest first (a logo also moves on from a name its
# font has no letter for: the stock set's slash).
NAMES = [n for i, n in enumerate([NAME, SHORT, SUPER]) if n and n not in [NAME, SHORT, SUPER][:i]]
if STOCK:
    NAMES = ['FLUX/BBS', 'FLUX BBS', 'FLUXBBS', 'FLUX', 'FB']
# The board's name and the SysOp's handle as the screens' text gives
# them: the stock set has the codes the board fills in (%BN, %SN).
TITLE = '%BN' if STOCK else ((CONFIG.get('bbs', {}).get('name') or 'flux/bbs').strip().lower())
SYSOP = '%SN' if STOCK else ((CONFIG.get('bbs', {}).get('sysop_alias') or 'sysop').strip())
