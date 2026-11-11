"""Willow Pond: a LORD in-game module, native to flux/bbs.

A quiet pond past the forest, where Gert the fishmonger buys whatever
comes out of it. A player gets five casts a day. After a cast they watch
the bobber: hook too soon and the fish is scared off, too late and it's
gone. Big ones fight: the line goes taut (reel in) or the fish runs
(give it slack), and the wrong key, or none, loses it. Gert pays by the
pound; the pond keeps the biggest of each kind ever caught, and a new
record pike or catfish makes the Daily Happenings. Somewhere in the deep
end lives Old Whiskers, a catfish older than the Red Dragon, worth a gem
to Gert.

The pond's records and who cast how often today are in POND.DAT in
LORD's folder.

Install (the board's [dos] native names it POND, or a "*" folder holds
it):

    [dos.native]
    POND = "lord/pond.py"

then `python3 pond.py install /path/to/LORD` adds it to LORD's
3RDPARTY.DAT, with the POND.IGM LORD looks for (`uninstall` takes them
off again).
"""
import json
import os
import random
import select
import sys
import time

NAME = "POND"
SHOWN = "`0G`2o `0F`2ishing at `0W`2illow `0P`2ond"
STATE = "POND.DAT"
CASTS = 5
# Seconds to hook a fish once the bobber dips, and to answer a run or a
# pull: generous, for a caller on a slow line.
HOOK = 2.0
FIGHT = 2.5

# name, (lightest, heaviest) in pounds, gold a pound, how often, how
# many runs and pulls it fights.
FISH = [
    ("minnow", (0.1, 0.4), 4, 18, 0),
    ("perch", (0.5, 2.5), 10, 22, 0),
    ("trout", (1.0, 7.0), 14, 20, 1),
    ("bass", (2.0, 10.0), 12, 16, 2),
    ("pike", (4.0, 22.0), 11, 10, 3),
    ("catfish", (6.0, 40.0), 9, 7, 4),
]
OLD_WHISKERS = ("Old Whiskers", (60.0, 95.0), 0, 0, 5)
WHISKERS_CHANCE = 120
JUNK = ["an old boot", "a rusty helmet", "a soggy love letter to Violet", "a tangle of weeds",
        "a broken fishing rod (not yours)", "a very surprised frog"]

SAYINGS = [
    "Fish don't care how many dragons you've killed.",
    "Quiet, now. The trout can hear you thinking.",
    "Big pike in the reeds. Bit the King's toe once.",
    "Old Whiskers was here before the town. He'll be here after.",
    "Pay by the pound, I do. Bones and all.",
    "Never fish with a hangover. Ask Barak.",
    "Somebody pulled a sword out of here once. Kept fishing, though.",
]


def today():
    return time.strftime("%Y-%m-%d")


def load():
    try:
        s = json.load(open(STATE))
    except (OSError, ValueError):
        s = {}
    s.setdefault("players", {})
    s.setdefault("records", {})
    return s


def save(s):
    tmp = STATE + ".new"
    with open(tmp, "w") as f:
        json.dump(s, f)
    os.replace(tmp, STATE)


def mine(s, g):
    p = s["players"].get(str(g.account))
    if not p or p.get("name") != g.player.name or p.get("day") != today():
        p = {"name": g.player.name, "day": today(), "casts": 0}
        s["players"][str(g.account)] = p
    return p


def poll(g, secs):
    """A key within `secs` seconds, or None."""
    if not g._pending:
        r, _, _ = select.select([0], [], [], secs)
        if not r:
            return None
    return g.getkey()


def drain(g):
    """Keys typed ahead are dropped: no hooking a fish before it bites."""
    g._pending = b""
    while select.select([0], [], [], 0)[0]:
        if not os.read(0, 256):
            break


def wait_bite(g):
    """The bobber, until it dips. False if the player struck too soon."""
    g.write("  `2")
    for _ in range(random.randint(3, 9)):
        g.write("`3~ ")
        if poll(g, 0.7) is not None:
            g.say()
            g.say("  `2You yank the line... at nothing.  Whatever was down there is")
            g.say("  `2long gone now.")
            return False
    g.say()
    g.say("  `%*BLOOP*  `0The bobber dips!  `2Strike! (any key)")
    return True


def fight(g, rounds):
    """The fish's runs and pulls: the right key each time, quick."""
    for i in range(rounds):
        drain(g)
        if random.random() < 0.5:
            want, msg = "R", "`0The line goes taut!  `2(`0R`2)eel it in!"
        else:
            want, msg = "S", "`0It runs!  `2Give it (`0S`2)lack!"
        g.say("  " + msg)
        k = poll(g, FIGHT)
        if k is None:
            g.say("  `4Too slow.")
            return False
        if k.upper() != want:
            g.say("  `4" + ("The line snaps!" if want == "S" else "It throws the hook!"))
            return False
        g.say("  `2" + random.choice(["Good.", "Steady...", "Nice.", "Hold on..."]))
    return True


def catch(g):
    """What's on the line: a kind, its weight, or junk."""
    if random.randrange(WHISKERS_CHANCE) == 0:
        kind = OLD_WHISKERS
    elif random.randrange(12) == 0:
        return None, 0.0
    else:
        kind = random.choices(FISH, [f[3] for f in FISH])[0]
    lo, hi = kind[1]
    # Most are small; a few are big.
    w = lo + (hi - lo) * random.random() ** 2.2
    return kind, round(w, 1)


def cast(g, s):
    p = g.player
    me = mine(s, g)
    if me["casts"] >= CASTS:
        g.say()
        g.say(f"  `0\"That's {CASTS} casts, love.  The fish need their rest too.\"")
        g.say()
        g.more()
        return
    me["casts"] += 1
    save(s)
    g.title("Willow Pond")
    g.say("  `2You bait the hook and cast out over the still water.")
    g.say()
    drain(g)
    if not wait_bite(g):
        g.say()
        g.more()
        return
    drain(g)
    if poll(g, HOOK) is None:
        g.say("  `2You blink, and the bobber's floating again.  Too late.")
        g.say()
        g.more()
        return
    kind, weight = catch(g)
    if kind is None:
        g.say(f"  `2You haul it in: `0{random.choice(JUNK)}`2.  Gert laughs at you.")
        g.say()
        g.more()
        return
    name, _, per_lb, _, rounds = kind
    if rounds:
        g.say("  `2It's a fighter!")
        if not fight(g, rounds):
            g.say(f"  `2It got away.  From the size of the swirl, a big `0{name}`2.")
            g.say()
            g.more()
            return
    g.say()
    g.say(f"  `2You land a `0{weight} lb {name}`2!")
    if kind is OLD_WHISKERS:
        g.say("  `%OLD WHISKERS!  `2The pond goes very quiet.  Gert drops her knife.")
        g.say("  `0\"Never thought I'd see him out of the water.  That's worth a")
        g.say("  `0gem, that is.\"  `2She gives you `$1 gem`2, and lets him go again.")
        p.gems += 1
        g.news(f"`0{p.name} `2caught `%Old Whiskers`2, the catfish of Willow Pond! `0({weight} lb)")
    else:
        gold = max(1, int(weight * per_lb * (1 + max(1, p.level) / 4)))
        p.gold += gold
        g.say(f"  `2Gert weighs it and pays you `${gold:,} gold`2.")
    rec = s["records"].get(name)
    if not rec or weight > rec["weight"]:
        s["records"][name] = {"weight": weight, "by": p.name, "day": today()}
        save(s)
        if rec:
            g.say(f"  `%A pond record!  `2The old one was {rec['weight']} lb, by `0{rec['by']}`2.")
            # (Only the big ones are news.)
            if kind is not OLD_WHISKERS and rounds >= 3:
                g.news(f"`0{p.name} `2pulled a record `0{weight} lb {name} `2out of Willow Pond!")
        else:
            g.say(f"  `%The first {name} in the pond's book!")
    g.say()
    g.more()


def records(g, s):
    g.title("Willow Pond: The Book")
    g.say("  `2Gert keeps a book of the biggest of each.")
    g.say()
    kinds = [f[0] for f in FISH] + [OLD_WHISKERS[0]]
    for k in kinds:
        r = s["records"].get(k)
        if r:
            g.say(f"  `0{k:<14}`${r['weight']:>6.1f} lb  `2{r['by']}, {r['day']}")
        elif k != OLD_WHISKERS[0]:
            g.say(f"  `0{k:<14}`2nobody yet")
    g.say()
    g.more()


def main():
    import lordigm

    with lordigm.Igm() as g:
        while True:
            s = load()
            left = CASTS - mine(s, g)["casts"]
            g.title("Willow Pond")
            g.say("  `2Past the forest, a still pond under the willows.  A rowboat")
            g.say("  `2rots by the bank, and `0Gert`2 the fishmonger sits on a crate with")
            g.say("  `2her scales and a very sharp knife.")
            g.say()
            g.say(f"  `2(`0C`2)ast a line   `2({left} cast{'s' if left != 1 else ''} left today)")
            g.say("  `2(`0B`2)ook of records")
            g.say("  `2(`0T`2)alk to Gert")
            g.say("  `2(`0R`2)eturn to town")
            g.say()
            k = g.key("  `2Your choice? [`0R`2] : ", "CBTR", "R")
            if k == "C":
                cast(g, s)
            elif k == "B":
                records(g, s)
            elif k == "T":
                g.say()
                g.say("  `0\"" + random.choice(SAYINGS) + "\"")
                g.say()
                g.more()
            else:
                g.say()
                g.say("  `2You leave the pond to the dragonflies.")
                g.pause(0.6)
                return


def install(folder, remove=False):
    """Add the two lines to (or take them off) LORD's 3RDPARTY.DAT, with
    POND.IGM beside it: LORD only sends players to a program it can
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
            f.write(b"Willow Pond: a flux/bbs native module ([dos] native).\r\n")
    open(path, "wb").write(("\r\n".join(out) + "\r\n").encode("cp437"))
    print(("took Willow Pond off " if remove else "added Willow Pond to ") + path)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] in ("install", "uninstall"):
        install(sys.argv[2] if len(sys.argv) > 2 else ".", sys.argv[1] == "uninstall")
    else:
        main()
