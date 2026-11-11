"""The Hermit's Terminal: a LORD in-game module, native to flux/bbs.

Deep in the forest an old hermit keeps a terminal that still dials out.
Once a day a player may dial his board and try to crack its password:
four digits, none twice, in seven tries, the terminal blinking after each
(a bright `*` for a right digit in the right place, a dim `+` for a right
digit in the wrong one). Cracking it pays gems, gold and experience and
makes the Daily Happenings; failing costs a headache's worth of hit
points, never the last one.

Install (the board's [dos] native names it TERMINAL):

    [dos.native]
    TERMINAL = "lord/terminal.py"

then `python3 terminal.py install /path/to/LORD` adds it to LORD's
3RDPARTY.DAT, with the TERMINAL.IGM LORD looks for (`uninstall` takes
them off again).
"""
import json
import os
import random
import sys
import time

NAME = "TERMINAL"
SHOWN = "`0V`2isit `0T`2he `0H`2ermit's `0T`2erminal"
TRIES = 7
STATE = "TERMINAL.DAT"

SAYINGS = [
    "In my day we waited all night for one picture to come down the wire.",
    "Never tell your password in the bar. Seth Able listens to everything.",
    "The Red Dragon? Bah. Try a sysop with a full hard disk.",
    "I lost a whole game of TradeWars to a thunderstorm once. Never again.",
    "Some say Violet has a modem now. Some say a lot of things.",
    "A busy signal is the realm telling you to go fight something.",
    "My board has one node. One! And it's mine.",
    "Lightning took my first modem. I keep the second one in a bucket of sand.",
    "Want to know a secret? Every password I ever made had no digit twice.",
    "Kids today with their fairies. In my day you got a sysop and you liked it.",
]


def today():
    return time.strftime("%Y-%m-%d")


def load_state():
    try:
        return json.load(open(STATE))
    except (OSError, ValueError):
        return {}


def save_state(state):
    tmp = STATE + ".new"
    with open(tmp, "w") as f:
        json.dump(state, f)
    os.replace(tmp, STATE)


def called_today(g):
    s = load_state().get(str(g.account))
    # A new game reuses accounts: it's theirs only with their name.
    return bool(s) and s.get("name") == g.player.name and s.get("day") == today()


def mark_called(g):
    state = load_state()
    state[str(g.account)] = {"name": g.player.name, "day": today()}
    save_state(state)


def blinks(secret, guess):
    right = sum(a == b for a, b in zip(secret, guess))
    near = sum(1 for c in guess if c in secret) - right
    return right, near


def dial(g):
    if called_today(g):
        g.say()
        g.say("  `2The hermit doesn't look up.  `0\"One call a day.  The line's long")
        g.say("  `0distance, and I'm not made of gold.\"")
        g.say()
        g.more()
        return
    mark_called(g)
    g.title("The Hermit's Terminal")
    g.say("  `2The hermit shuffles aside.  The glass face hums and glows `$amber`2.")
    g.say()
    g.write("  `8ATDT 555-0199")
    for _ in range(3):
        g.pause(0.6)
        g.write("`8.")
    g.say()
    g.pause(0.5)
    g.say("  `8ring...")
    g.pause(0.8)
    g.say("  `8ring...")
    g.pause(0.8)
    g.say("  `0CONNECT 2400")
    g.pause(0.6)
    g.say()
    g.say("  `$THE HERMIT'S HOLE  `8- node 1 of 1 - sysop: the hermit -")
    g.say("  `2Password: `0four digits`2, no digit used twice.  After each try the")
    g.say("  `2screen blinks: `%*`2 for a right digit in its place, `7+`2 for a right")
    g.say("  `2digit in the wrong place.  `0" + str(TRIES) + "`2 tries before the line drops.")
    g.say()
    secret = "".join(random.sample("0123456789", 4))
    tries = 0
    while tries < TRIES:
        guess = g.ask(f"  `2Try `0{tries + 1}`2 of `0{TRIES}`2: ", size=4, digits=True)
        if len(guess) != 4 or len(set(guess)) != 4:
            g.say("  `4  INVALID: four digits, none twice.")
            continue
        tries += 1
        right, near = blinks(secret, guess)
        if right == 4:
            win(g, TRIES - tries)
            return
        g.say("        `8[ " + "`%* " * right + "`7+ " * near + "`8. " * (4 - right - near) + "`8]")
    g.say()
    g.say("  `4NO CARRIER")
    g.pause(0.8)
    lose(g, secret)


def win(g, spare):
    p = g.player
    gems = 1 + spare // 3
    gold = max(1, p.level) * 250 * (spare + 1)
    exp = max(1, p.level) * 40
    g.say()
    g.say("  `0ACCESS GRANTED.")
    g.pause(0.6)
    g.say("  `2You find a file called `0TREASURE.ZIP`2 and download it at a")
    g.say("  `2blistering `0240 characters a second`2.  Inside:")
    g.say()
    g.say(f"     `${gems} gem{'s' if gems > 1 else ''}`2,  `${gold:,} gold`2  and  `${exp:,} experience`2.")
    p.gems += gems
    p.gold += gold
    p.exp += exp
    g.say()
    g.say("  `2The hermit squints at you.  `0\"Nobody's cracked that in years.\"")
    g.news(f"`0{p.name} `2cracked the `0Hermit's terminal`2 and got away with `${gems} gem{'s' if gems > 1 else ''}`2!")
    g.say()
    g.more()


def lose(g, secret):
    p = g.player
    hurt = min(max(1, p.hit_max // 10), max(0, p.hit_points - 1))
    p.hit_points -= hurt
    g.say()
    g.say(f"  `2The password was `0{secret}`2.  The hermit cackles.")
    g.say("  `0\"Line's busy tomorrow too, if ye like.\"")
    if hurt:
        g.say(f"  `2Staring at the glass gives you a headache: `4-{hurt} hit points`2.")
    g.say()
    g.more()


def main():
    # (Here: install, run by hand, needs only this file.)
    import lordigm

    with lordigm.Igm() as g:
        while True:
            g.title("The Hermit's Cave")
            g.say("  `2Deep in the forest a cave glows `$amber`2.  Inside, an old hermit")
            g.say("  `2hunches over a humming box with a glass face, cables running off")
            g.say("  `2into the rock.  `0\"A terminal,\"`2 he says.  `0\"Still dials out.\"")
            g.say()
            g.say("  `2(`0D`2)ial the hermit's board")
            g.say("  `2(`0T`2)alk to the hermit")
            g.say("  `2(`0L`2)eave the cave")
            g.say()
            k = g.key("  `2Your choice? [`0L`2] : ", "DTL", "L")
            if k == "D":
                dial(g)
            elif k == "T":
                g.say()
                g.say("  `0\"" + random.choice(SAYINGS) + "\"")
                g.say()
                g.more()
            else:
                g.say()
                g.say("  `2You leave the hermit to his humming.")
                g.pause(0.6)
                return


def install(folder, remove=False):
    """Add the two lines to (or take them off) LORD's 3RDPARTY.DAT, with
    TERMINAL.IGM beside it: LORD only sends players to a program it can
    find, and the board runs this in its place."""
    find = lambda n: next((f for f in os.listdir(folder) if f.upper() == n), n)
    path = os.path.join(folder, find("3RDPARTY.DAT"))
    stub = os.path.join(folder, find(NAME + ".IGM"))
    lines = open(path, "rb").read().decode("cp437").replace("\r", "").split("\n") if os.path.exists(path) else []
    out, i = [], 0
    while i < len(lines):
        first = lines[i].strip().upper().split(" ")[0]
        if first.rsplit("\\", 1)[-1].split(".")[0] == NAME:
            i += 2
            continue
        out.append(lines[i])
        i += 1
    while out and not out[-1].strip():
        out.pop()
    if remove:
        if os.path.exists(stub):
            os.remove(stub)
    else:
        out += [f"{NAME}.IGM *", SHOWN]
        with open(stub, "wb") as f:
            f.write(b"The Hermit's Terminal: a flux/bbs native module ([dos] native).\r\n")
    open(path, "wb").write(("\r\n".join(out) + "\r\n").encode("cp437"))
    print(("took The Hermit's Terminal off " if remove else "added The Hermit's Terminal to ") + path)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] in ("install", "uninstall"):
        install(sys.argv[2] if len(sys.argv) > 2 else ".", sys.argv[1] == "uninstall")
    else:
        main()
