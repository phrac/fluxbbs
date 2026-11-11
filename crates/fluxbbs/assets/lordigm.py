"""lordigm: LORD in-game modules in Python, for flux/bbs.

A module is a script the board runs in place of a DOS program when LORD
(Legend of the Red Dragon) sends a player to it from Other Places: the
board's `[dos] native` names it, and LORD's 3RDPARTY.DAT lists that name
like any IGM. The caller is on standard input and output, the current
folder is LORD's, and the node is the first argument, as for a DOS IGM.

    import lordigm
    with lordigm.Igm() as g:
        g.clear()
        g.say("`2Hello, `0" + g.player.name + "`2!")
        if g.key("`2Want `$100`2 gold? [`0Y`2/`0N`2] ", "YN") == "Y":
            g.player.gold += 100
        g.more()

On leaving, the player's record is saved, and their time left goes back
to LORD (in INFO.<node>). A caller who hangs up ends the module
(`Hangup`); the record is still saved.

The record is LORD's own (PLAYER.DAT, 236 bytes a player: LORDSTRC.PAS,
the same in 3.x and 4.x), and the other files are LORD's: the daily news
(LOGNOW.TXT) and mail (MAIL<n>.DAT). Text takes LORD's colour codes: `1 to
`9, `0, `! `@ `# `$ `% `^, `r0 to `r7 for backgrounds, `c to clear.
Keep dark grey (`8) for what may go unseen: some terminals draw it black.
"""

import os
import random
import re
import struct
import sys
import time

__all__ = ["Igm", "Player", "Hangup", "players"]

RECORD = 236

# (name, kind): s<n> a Pascal string[n], h integer, H word, l longint,
# b shortint, B boolean.
FIELDS = [
    ("name", "s20"), ("real_name", "s50"), ("hit_points", "h"), ("bad", "h"), ("rate", "h"),
    ("hit_max", "h"), ("weapon_num", "h"), ("weapon", "s20"), ("seen_master", "h"),
    ("fights_left", "h"), ("human_left", "h"), ("gold", "l"), ("bank", "l"), ("defense", "h"),
    ("strength", "h"), ("charm", "h"), ("seen_dragon", "h"), ("seen_violet", "h"), ("level", "h"),
    ("time", "H"), ("armour", "s20"), ("armour_num", "h"), ("dead", "b"), ("inn", "b"), ("gems", "h"),
    ("exp", "l"), ("sex", "b"), ("seen_bard", "b"), ("last_alive_time", "h"), ("lays", "h"),
    ("why", "h"), ("on_now", "B"), ("m_time", "h"), ("time_on", "s5"), ("klass", "b"), ("horse", "h"),
    ("love", "s25"), ("married", "h"), ("kids", "h"), ("king", "h"), ("skill_w", "b"), ("skill_m", "b"),
    ("skill_t", "b"), ("uses_w", "b"), ("uses_m", "b"), ("uses_t", "b"), ("inn_random", "B"),
    ("married_to", "h"), ("v1", "l"), ("kills", "h"), ("v3", "h"), ("v4", "B"), ("flirted", "b"),
    ("new_stat1", "b"), ("new_stat2", "b"), ("new_stat3", "b"),
]
_NUM = {"h": "<h", "H": "<H", "l": "<i", "b": "<b", "B": "<B"}


class Player:
    """One player's record. Fields by their LORDSTRC.PAS names (`klass`
    for class, `horse` for extra, `kills` for v2, `flirted` for v5)."""

    def __init__(self, account, data):
        self.account = account
        self._raw = bytes(data)
        at = 0
        for name, kind in FIELDS:
            if kind[0] == "s":
                n = int(kind[1:])
                ln = min(data[at], n)
                setattr(self, name, data[at + 1:at + 1 + ln].decode("cp437"))
                at += n + 1
            else:
                fmt = _NUM[kind]
                setattr(self, name, struct.unpack_from(fmt, data, at)[0])
                at += struct.calcsize(fmt)

    def pack(self):
        # Over the record as read: what's past a string's end stays.
        out = bytearray(self._raw)
        at = 0
        for name, kind in FIELDS:
            v = getattr(self, name)
            if kind[0] == "s":
                n = int(kind[1:])
                b = str(v).encode("cp437", "replace")[:n]
                out[at] = len(b)
                out[at + 1:at + 1 + len(b)] = b
                at += n + 1
            else:
                fmt = _NUM[kind]
                lo, hi = {"h": (-32768, 32767), "H": (0, 65535), "l": (-2**31, 2**31 - 1), "b": (-128, 127), "B": (0, 255)}[kind]
                struct.pack_into(fmt, out, at, max(lo, min(hi, int(v))))
                at += struct.calcsize(fmt)
        assert at == RECORD
        return bytes(out)

    @property
    def female(self):
        return self.sex == 5

    def __repr__(self):
        return f"<Player {self.account} {self.name!r} level {self.level}>"


def players(folder="."):
    """Every player in PLAYER.DAT."""
    try:
        data = open(os.path.join(folder, _find(folder, "PLAYER.DAT")), "rb").read()
    except OSError:
        return []
    return [Player(i, data[i * RECORD:(i + 1) * RECORD]) for i in range(len(data) // RECORD)]


def _find(folder, name):
    """A file's real name in `folder`, whatever its case (DOS files)."""
    for f in os.listdir(folder):
        if f.upper() == name.upper():
            return f
    return name


class Hangup(Exception):
    """The caller has gone."""


_FG = {"1": "0;34", "2": "0;32", "3": "0;36", "4": "0;31", "5": "0;35", "6": "0;33", "7": "0;37", "8": "1;30",
       "9": "1;34", "0": "1;32", "!": "1;36", "@": "1;31", "#": "1;35", "$": "1;33", "%": "1;37", "^": "0;30"}
_BG = {"0": "40", "1": "44", "2": "42", "3": "46", "4": "41", "5": "45", "6": "43", "7": "47"}


class Igm:
    """The module's session: the player (`player`), what LORD said
    (`info`), the caller's keys and the screen."""

    def __init__(self, node=None, idle=300):
        self.node = int(node if node is not None else (sys.argv[1] if len(sys.argv) > 1 else os.environ.get("FLUXBBS_NODE", "1")))
        self.idle = idle
        self.started = time.time()
        self._pending = b""
        self._info_name = _find(".", f"INFO.{self.node}")
        lines = open(self._info_name, "rb").read().decode("cp437").replace("\r", "").split("\n")
        self._info = lines
        get = lambda i: lines[i].strip() if i < len(lines) else ""
        self.account = int(get(0) or 0)
        self.ansi = (int(get(1) or 3) >= 3)
        self.time_left = int(get(4) or 30)
        self.info = {"graphics": get(1), "rip": get(2), "fairy": get(3).upper().endswith("YES"), "handle": get(5), "first": get(6), "last": get(7)}
        self._players = _find(".", "PLAYER.DAT")
        self.player = self._load()

    # ---- the record ---------------------------------------------------------

    def _load(self):
        with open(self._players, "rb") as f:
            f.seek(self.account * RECORD)
            data = f.read(RECORD)
        if len(data) != RECORD:
            raise SystemExit(f"no player {self.account} in PLAYER.DAT")
        return Player(self.account, data)

    def save(self):
        """The player's record back in PLAYER.DAT (only theirs)."""
        with open(self._players, "r+b") as f:
            f.seek(self.account * RECORD)
            f.write(self.player.pack())

    def _done(self):
        self.save()
        # The time used comes off what LORD gives them back.
        used = int((time.time() - self.started) // 60)
        lines = list(self._info)
        while len(lines) < 5:
            lines.append("")
        lines[4] = str(max(0, self.time_left - used))
        with open(self._info_name, "wb") as f:
            f.write("\r\n".join(lines).encode("cp437", "replace"))

    def __enter__(self):
        return self

    def __exit__(self, kind, err, tb):
        self._done()
        # A hangup is a normal end.
        return kind is Hangup

    # ---- the screen ---------------------------------------------------------

    def colour(self, text):
        """LORD's colour codes as ANSI (or gone, for a caller without)."""
        out, i = [], 0
        while i < len(text):
            c = text[i]
            if c == "`" and i + 1 < len(text):
                n = text[i + 1]
                if n == "`":
                    out.append("`")
                elif n == "c":
                    out.append("\x1b[2J\x1b[H\r\n\r\n" if self.ansi else "\r\n\r\n")
                elif n == "r" and i + 2 < len(text) and text[i + 2] in _BG:
                    if self.ansi:
                        out.append(f"\x1b[{_BG[text[i + 2]]}m")
                    i += 1
                elif n in _FG:
                    if self.ansi:
                        out.append(f"\x1b[{_FG[n]}m")
                else:
                    out.append(c + n)
                i += 2
                continue
            out.append(c)
            i += 1
        return "".join(out)

    def write(self, text):
        data = self.colour(text).replace("\r\n", "\n").replace("\n", "\r\n").encode("cp437", "replace")
        try:
            os.write(1, data)
        except OSError:
            raise Hangup()

    def say(self, text=""):
        """`text` and a new line."""
        self.write(text + "\n")

    def clear(self):
        self.write("`c")

    def line(self):
        """LORD's rule under a title."""
        self.say("`2" + "-=" * 27 + "-")

    def title(self, text):
        """A screen's title, as LORD sets them."""
        self.clear()
        self.say("  `0" + text)
        self.line()

    def pause(self, secs):
        time.sleep(secs)

    # ---- the caller's keys ----------------------------------------------------

    def _byte(self):
        if self._pending:
            b, self._pending = self._pending[:1], self._pending[1:]
            return b
        import select
        r, _, _ = select.select([0], [], [], self.idle)
        if not r:
            raise Hangup()
        b = os.read(0, 64)
        if not b:
            raise Hangup()
        self._pending = b[1:]
        return b[:1]

    def getkey(self):
        """A key: a character, or "UP", "DOWN", "LEFT", "RIGHT", "ENTER", "ESC"."""
        b = self._byte()
        if b in (b"\r", b"\n"):
            # CR LF, CR NUL: one Enter.
            if self._pending[:1] in (b"\n", b"\0"):
                self._pending = self._pending[1:]
            return "ENTER"
        if b == b"\x1b":
            if self._pending[:1] == b"[" and len(self._pending) > 1:
                code, self._pending = self._pending[1:2], self._pending[2:]
                return {b"A": "UP", b"B": "DOWN", b"C": "RIGHT", b"D": "LEFT"}.get(code, "ESC")
            return "ESC"
        if b in (b"\x08", b"\x7f"):
            return "BACKSPACE"
        return b.decode("cp437")

    def key(self, prompt="", allowed=None, default=None):
        """A key from `allowed` (upper case), Enter giving `default`;
        it's shown after the prompt, as LORD does."""
        if prompt:
            self.write(prompt)
        while True:
            k = self.getkey()
            if k == "ENTER" and default:
                k = default
            if len(k) == 1:
                k = k.upper()
                if allowed is None or k in allowed:
                    self.say("`0" + k)
                    return k

    def ask(self, prompt="", size=20, digits=False):
        """A line typed, up to `size` characters."""
        if prompt:
            self.write(prompt)
        self.write("`%")
        s = ""
        while True:
            k = self.getkey()
            if k == "ENTER":
                self.say()
                return s
            if k == "BACKSPACE" and s:
                s = s[:-1]
                self.write("\b \b")
            elif len(k) == 1 and k >= " " and len(s) < size and (not digits or k.isdigit()):
                s += k
                self.write(k.replace("`", "``"))

    def more(self):
        """LORD's <MORE>, until a key."""
        self.write("`2<`0MORE`2>")
        self.getkey()
        self.write("\r" + " " * 8 + "\r")

    # ---- the realm ------------------------------------------------------------

    def news(self, *lines):
        """Lines for today's Daily Happenings, with LORD's rule after."""
        name = _find(".", "LOGNOW.TXT")
        text = "".join("  " + l + "\r\n" for l in lines) + "                                `2-`0=`2-`0=`2-`0=`2-\r\n"
        with open(name, "ab") as f:
            f.write(text.encode("cp437", "replace"))

    def mail(self, account, *lines):
        """Mail to another player (MAIL<account>.DAT), as LORD's own."""
        name = _find(".", f"MAIL{account}.DAT")
        with open(name, "ab") as f:
            f.write("".join(l + "\r\n" for l in lines).encode("cp437", "replace"))

    def chance(self, n):
        """True one time in `n`."""
        return random.randrange(n) == 0

    def clean(self, text):
        """`text` with LORD's bad words (BADWORDS.DAT, `word|instead`)
        replaced, as LORD does what players type."""
        try:
            lines = open(_find(".", "BADWORDS.DAT"), "rb").read().decode("cp437").replace("\r", "").split("\n")
        except OSError:
            return text
        for l in lines:
            if not l.strip() or l.startswith(";") or "|" not in l:
                continue
            bad, good = l.split("|", 1)
            if bad:
                text = re.sub(re.escape(bad), lambda _m: good, text, flags=re.IGNORECASE)
        return text
