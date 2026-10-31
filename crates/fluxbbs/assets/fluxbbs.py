"""flux/bbs from Python: what a script (`-X name`, `%!name` in a string) uses
to talk to the caller and the board.

    import fluxbbs as bbs

    bbs.cls()
    bbs.showln("|15hello, |11" + bbs.user()["handle"])
    if bbs.yesno(False):
        name = bbs.input(max=30)

The board runs the script as its own process and answers each call over a
private socket (fd 3): the script never touches the caller's connection
itself. `print()` and `input()` go to the caller too. If the caller hangs
up, the script ends (SystemExit) at its next call. See docs/SCRIPTING.md.

This module comes with the board, which puts it where scripts find it, so
it always matches the board's version.
"""

import builtins
import io
import json
import os
import socket
import sys

__all__ = [
    "write", "writeln", "show", "showln", "say", "sayln", "newline", "cls", "clreol", "beep",
    "gotoxy", "up", "down", "left", "right", "color", "where", "showfile", "pause", "getkey",
    "input", "yesno", "menucmd", "mci", "expand", "acs", "inmenu", "user", "saveuser", "log",
    "get", "set", "nocolor", "pad", "node", "script", "version", "args", "Error",
]


class Error(Exception):
    """The board turned a call down (a bad user field, say)."""


#: This caller's node, the script's name, the board's version, and the
#: words after the script's name.
node = int(os.environ.get("FLUXBBS_NODE", "0") or 0)
script = os.environ.get("FLUXBBS_SCRIPT", "")
version = os.environ.get("FLUXBBS_VERSION", "")
args = sys.argv[1:]

_sock = None
_rd = None


def _line():
    global _sock, _rd
    if _sock is None:
        fd = int(os.environ.get("FLUXBBS_FD", "-1"))
        if fd < 0:
            raise RuntimeError("fluxbbs: not run by the board (no FLUXBBS_FD); run it with -X from a menu")
        _sock = socket.socket(fileno=fd)
        _rd = _sock.makefile("rb")
    return _sock, _rd


def _call(op, *a):
    sock, rd = _line()
    try:
        sock.sendall((json.dumps({"op": op, "a": list(a)}) + "\n").encode("utf-8"))
        reply = rd.readline()
    except (BrokenPipeError, ConnectionResetError, OSError):
        reply = b""
    if not reply:
        # The caller's gone (or the board stopped us): end quietly.
        raise SystemExit(0)
    r = json.loads(reply)
    if "e" in r:
        raise Error(r["e"])
    return r.get("r")


def _text(v):
    return "" if v is None else str(v)


# ---- the screen ---------------------------------------------------------------

def write(text=""):
    """Text as it is: no colour codes acted on."""
    _call("write", _text(text))


def writeln(text=""):
    _call("write", _text(text) + "\r\n")


def show(text=""):
    """Text with the board's codes: pipe colours (|15), MCI (%UN), %%FILE."""
    _call("show", _text(text))


def showln(text=""):
    show(text)
    newline()


def say(name):
    """A prompt string by its name (as *E lists them)."""
    _call("say", name)


def sayln(name):
    say(name)
    newline()


def newline(n=1):
    _call("newline", int(n))


def cls():
    _call("cls")


def clreol():
    _call("clreol")


def beep():
    _call("beep")


def gotoxy(x, y):
    _call("gotoxy", int(x), int(y))


def up(n=1):
    _call("move", "U", int(n))


def down(n=1):
    _call("move", "D", int(n))


def left(n=1):
    _call("move", "L", int(n))


def right(n=1):
    _call("move", "R", int(n))


def color(fg, bg=0):
    """DOS colours: fg 0-15, bg 0-7."""
    _call("color", int(fg) & 15, int(bg) & 7)


def where():
    """The cursor, (x, y) from 1."""
    x, y = _call("where")
    return x, y


def showfile(name):
    """A screen from the text library; False if there's no such screen."""
    return bool(_call("showfile", name))


def pause():
    """The board's "press any key"."""
    _call("pause")


# ---- the caller ---------------------------------------------------------------

def getkey():
    """One key: a character, or enter, esc, tab, backspace, up, down, left,
    right, home, end, pgup, pgdn, ins, del."""
    return _call("getkey")


def input(max=40, default="", password=False, upper=False, mixed=False, allowed="", required=False, complete=False):
    """A line typed by the caller. `complete`: Tab finishes a handle."""
    opts = {"max": int(max), "default": _text(default), "password": bool(password), "upper": bool(upper),
            "mixed": bool(mixed), "allowed": _text(allowed), "required": bool(required), "complete": bool(complete)}
    return _call("input", opts)


def yesno(default=True):
    return bool(_call("yesno", bool(default)))


# ---- the board ----------------------------------------------------------------

def menucmd(code, param=""):
    """Run a menu command (as a menu's `command` and `param`)."""
    _call("menucmd", code, _text(param))


def mci(code):
    """An MCI code's value (`UN` for %UN), or None."""
    return _call("mci", code)


def expand(text):
    """Text with its MCI codes filled in."""
    return _call("expand", _text(text))


def acs(expr):
    """Does the caller pass an ACS expression (s50, fX...)?"""
    return bool(_call("acs", expr))


def inmenu(keys):
    """Has the current menu a command on these keys?"""
    return bool(_call("inmenu", keys))


def user():
    """The caller's record, as a dict (handle, sl, level, flags, posts...)."""
    return _call("user")


def saveuser(fields):
    """Change fields of the caller's record (some aren't a script's to change:
    Error says which)."""
    _call("saveuser", dict(fields))


def log(text):
    """A line in the board's log, under the caller and this script."""
    _call("log", _text(text))


def get(key):
    """A value this script kept (with set), or None."""
    return _call("get", key)


def set(key, value):
    """Keep a value for this script, across calls and callers (None forgets it)."""
    _call("set", key, None if value is None else _text(value))


def nocolor(text):
    """Text with the board's codes taken out."""
    return _call("nocolor", _text(text))


def pad(text, n):
    """Text cut or padded to n columns, codes not counted."""
    return _call("pad", _text(text), int(n))


# ---- print() and input() go to the caller ------------------------------------

class _Caller(io.TextIOBase):
    def writable(self):
        return True

    def write(self, s):
        if s:
            _call("write", s.replace("\r\n", "\n").replace("\n", "\r\n"))
        return len(s)

    def flush(self):
        pass


def _builtin_input(prompt=""):
    if prompt:
        write(prompt)
    line = input(max=255)
    newline()
    return line


if os.environ.get("FLUXBBS_FD"):
    sys.stdout = _Caller()
    builtins.input = _builtin_input
