"""Dusty's Disk Cart: a LORD in-game module, native to flux/bbs.

A peddler with a cart of unlabeled floppies parks by the road out of
town. A player buys up to three a day (the price grows with their level)
and boots each to see what's on it: a shareware game (forest fights), a
forgotten bank file (gold), a gem collection, a love poem (charm), a
strategy guide (experience), a blank disk, "Abort, Retry, Fail?" or a
virus, with its own effect on the screen and a cost: Cascade's letters
fall, Ping-Pong's ball bounces, Stoned and Michelangelo say hello.

Players may also leave a floppy on the cart once a day, with a line on
it; a later buyer may get it, with the note. The cart keeps them (the
newest 30) in DUSTY.DAT in LORD's folder, with who bought what today.

Install (the board's [dos] native names it DUSTY, or a "*" folder holds
it):

    [dos.native]
    DUSTY = "lord/dusty.py"

then `python3 dusty.py install /path/to/LORD` adds it to LORD's
3RDPARTY.DAT, with the DUSTY.IGM LORD looks for (`uninstall` takes them
off again).
"""
import json
import os
import random
import sys
import time

NAME = "DUSTY"
SHOWN = "`0D`2usty's `0D`2isk `0C`2art"
STATE = "DUSTY.DAT"
PER_DAY = 3
KEEP_LEFT = 30
# A floppy someone left: the chance a bought one is it.
LEFT_CHANCE = 4

SAYINGS = [
    "Every disk's a surprise. That's the business model.",
    "Labels? Labels are for people who know what's on 'em.",
    "Got these off a sysop who moved to the city. Said he didn't need 'em. Odd fellow.",
    "Don't boot anything you find in the Dark Cloak Tavern. Trust me.",
    "Five and a quarter, three and a half, I don't judge.",
    "A fella once found the King's own tax ledger on one. Bought my cart with the reward.",
    "If it asks you to Abort, Retry or Fail, always Retry. Usually.",
    "Keep 'em away from the magnets in the Healer's hut.",
    "No refunds. No returns. No warranty, expressed or implied.",
]


def today():
    return time.strftime("%Y-%m-%d")


def load():
    try:
        s = json.load(open(STATE))
    except (OSError, ValueError):
        s = {}
    s.setdefault("players", {})
    s.setdefault("left", [])
    return s


def save(s):
    tmp = STATE + ".new"
    with open(tmp, "w") as f:
        json.dump(s, f)
    os.replace(tmp, STATE)


def mine(s, g):
    """Today's record for this player (a new game reuses accounts: it's
    theirs only with their name)."""
    p = s["players"].get(str(g.account))
    if not p or p.get("name") != g.player.name or p.get("day") != today():
        p = {"name": g.player.name, "day": today(), "bought": 0, "left": False}
        s["players"][str(g.account)] = p
    return p


def price(g):
    return 25 + 25 * max(1, g.player.level)


# ---- what's on a disk -----------------------------------------------------

def shareware(g):
    n = random.randint(1, 2)
    g.say("  `2It's a shareware game!  You lose an afternoon to it, and come")
    g.say(f"  `2away itching for a real fight: `0+{n} forest fight{'s' if n > 1 else ''}`2.")
    g.player.fights_left += n


def bankfile(g):
    gold = price(g) * random.randint(2, 4)
    g.say("  `2BANKPASS.TXT: someone's account at Ye Old Bank, with a balance")
    g.say(f"  `2nobody's touched in years.  You make a withdrawal: `${gold:,} gold`2.")
    g.player.gold += gold


def gems(g):
    g.say("  `2GEMS.ZIP: a catalogue of the realm's gems, and stuck to the back")
    g.say("  `2of the sleeve with tape, the real thing: `$1 gem`2.")
    g.player.gems += 1


def poem(g):
    g.say("  `2LOVEPOEM.TXT.  It's terrible.  You learn it by heart anyway,")
    g.say("  `2and Violet will never know where you got it: `0+1 charm`2.")
    g.player.charm += 1


def guide(g):
    exp = max(1, g.player.level) * 30
    g.say("  `2DRAGON.DOC: a strategy guide, mostly about the Red Dragon's")
    g.say(f"  `2bad breath.  Still, you learn a thing or two: `0+{exp:,} experience`2.")
    g.player.exp += exp


def blank(g):
    g.say("  `7Non-System disk or disk error")
    g.say("  `7Replace and strike any key when ready")
    g.getkey()
    g.say("  `2It's blank.  Dusty shrugs.  `0\"Every disk's a surprise.\"")


def abort_retry(g):
    g.say("  `7Sector not found reading drive A")
    while True:
        k = g.key("  `7Abort, Retry, Fail? ", "ARF")
        if k == "R" and random.random() < 0.5:
            g.say()
            g.say("  `2The drive grinds... and reads it!")
            random.choice(GOOD)(g)
            return
        if k == "R":
            g.say("  `7Sector not found reading drive A")
            continue
        g.say("  `2You give up on it.  The disk makes a fine coaster.")
        return


def cascade(g):
    """Cascade (1701): the screen's letters fall to the bottom."""
    lines = ["C:\\>dir a:", "", " Volume in drive A has no label", " Directory of A:\\", "",
             "README   TXT      1701 04-01-88", "SETUP    EXE     12288 04-01-88", "        2 file(s)     13989 bytes"]
    if g.ansi:
        g.write("\x1b[2J")
        for i, l in enumerate(lines):
            g.write(f"\x1b[{3 + i};3H\x1b[0;37m{l}")
        cells = [(3 + i, 3 + j, ch) for i, l in enumerate(lines) for j, ch in enumerate(l) if ch != " "]
        random.shuffle(cells)
        bottom = 20
        # Each column's pile: where the next letter to fall there lands.
        floor = {}
        for r, c, ch in sorted(cells[:45], key=lambda t: -t[0]):
            land = floor.get(c, bottom)
            if land <= r:
                continue
            for y in range(r, land):
                g.write(f"\x1b[{y};{c}H \x1b[{y + 1};{c}H{ch}")
                g.pause(0.004)
            floor[c] = land - 1
        g.write(f"\x1b[{bottom + 2};1H")
    g.say("  `4CASCADE!  `2The letters fell right off your screen.  Picking them")
    hurt = min(max(1, g.player.hit_max // 10), max(0, g.player.hit_points - 1))
    g.player.hit_points -= hurt
    g.say(f"  `2up one by one is exhausting: `4-{hurt} hit points`2.")
    return "Cascade"


def pingpong(g):
    """Ping-Pong: a ball bounces round the screen."""
    if g.ansi:
        g.write("\x1b[2J")
        x, y, dx, dy = 10, 5, 1, 1
        for _ in range(90):
            g.write(f"\x1b[{y};{x}H\x1b[1;37mo")
            g.pause(0.03)
            g.write(f"\x1b[{y};{x}H ")
            if not 2 <= x + dx <= 78:
                dx = -dx
            if not 2 <= y + dy <= 20:
                dy = -dy
            x, y = x + dx, y + dy
        g.write("\x1b[22;1H")
    g.say("  `4PING-PONG!  `2You watch the little ball bounce for an hour.")
    if g.player.fights_left > 0:
        g.player.fights_left -= 1
        g.say("  `2That's an hour you didn't spend in the forest: `4-1 forest fight`2.")
    else:
        g.say("  `2Luckily you had nothing better to do.")
    return "Ping-Pong"


def stoned(g):
    g.say()
    g.say("  `%Your PC is now Stoned!")
    g.pause(1.0)
    g.say()
    if g.player.charm > 0:
        g.player.charm -= 1
        g.say("  `2You giggle about it for the rest of the day.  People stare:")
        g.say("  `4-1 charm`2.")
    else:
        g.say("  `2You giggle about it.  Nobody was looking anyway.")
    return "Stoned"


def michelangelo(g):
    lost = g.player.gold // 4
    g.say()
    g.say("  `4Michelangelo!  `2It wipes the disk, and then, somehow, your coin")
    g.player.gold -= lost
    g.say(f"  `2purse: `4-{lost:,} gold`2.  Dusty is suddenly very busy.")
    return "Michelangelo"


GOOD = [shareware, bankfile, gems, poem, guide]
OUTCOMES = [(shareware, 15), (bankfile, 15), (gems, 10), (poem, 8), (guide, 12), (blank, 15), (abort_retry, 10),
            (cascade, 4), (pingpong, 4), (stoned, 4), (michelangelo, 3)]


def boot(g, s):
    p = g.player
    cost = price(g)
    if mine(s, g)["bought"] >= PER_DAY:
        g.say()
        g.say(f"  `0\"That's {PER_DAY} for today, friend.  Leave some for the others.\"")
        g.say()
        g.more()
        return
    if p.gold < cost:
        g.say()
        g.say(f"  `0\"That's {cost:,} gold, friend.  Cash only.  Bank's back in town.\"")
        g.say()
        g.more()
        return
    p.gold -= cost
    mine(s, g)["bought"] += 1
    # Saved now: a caller who hangs up has still bought it.
    save(s)
    g.save()
    g.title("Dusty's Disk Cart")
    g.say(f"  `2You hand over `${cost:,} gold`2 (`${p.gold:,}`2 left) and pick a floppy")
    g.say("  `2from the pile.")
    note = None
    others = [d for d in s["left"] if d.get("account") != g.account or d.get("name") != p.name]
    if others and g.chance(LEFT_CHANCE):
        note = random.choice(others)
        s["left"].remove(note)
        save(s)
        g.say("  `2This one has a label, in someone's handwriting.")
    g.say()
    g.write("  `3A:\\>")
    g.pause(0.5)
    for ch in "boot":
        g.write("`3" + ch)
        g.pause(0.12)
    g.say()
    g.write("  `3Reading drive A")
    for _ in range(4):
        g.pause(0.4)
        g.write("`3.")
    g.say()
    g.say()
    if note:
        g.say(f"  `2README.TXT, from `0{note['name']}`2:")
        g.say(f"     `%\"{note['text']}\"")
        g.say()
    fns, weights = zip(*OUTCOMES)
    before = p.gold
    virus = random.choices(fns, weights)[0](g)
    if p.gold != before:
        g.say(f"  `2You have `${p.gold:,} gold`2 on hand now.")
    if virus:
        g.news(f"`0{p.name} `2booted a floppy from `0Dusty's cart `2and caught `4{virus}`2!")
    g.say()
    g.more()


def leave(g, s):
    me = mine(s, g)
    if me["left"]:
        g.say()
        g.say("  `0\"You already left one today.  The cart's only so big.\"")
        g.say()
        g.more()
        return
    g.say()
    g.say("  `2You find a blank floppy and a pen.  What do you write on it?")
    text = g.clean(g.ask("  `2> ", size=60).strip())
    if not text:
        g.say("  `2You think better of it.")
        g.pause(0.6)
        return
    me["left"] = True
    s["left"].append({"account": g.account, "name": g.player.name, "text": text, "day": today()})
    s["left"] = s["left"][-KEEP_LEFT:]
    save(s)
    g.say()
    g.say("  `2You slip it into the pile when Dusty isn't looking.  Someone will")
    g.say("  `2boot it one day.")
    g.say()
    g.more()


def main():
    import lordigm

    with lordigm.Igm() as g:
        while True:
            s = load()
            me = mine(s, g)
            left_today = PER_DAY - me["bought"]
            g.title("Dusty's Disk Cart")
            g.say("  `2By the road out of town, a cart piled high with floppies: no")
            g.say("  `2labels, no sleeves, no promises.  `0Dusty`2 tips his hat.")
            g.say(f"  `0\"{price(g):,} gold apiece.  What's on 'em?  That's the fun part.\"")
            g.say()
            g.say(f"  `2(`0B`2)uy and boot a floppy   `2({left_today} left for you today)" if left_today else "  `2(`0B`2)uy and boot a floppy   `2(none left for you today)")
            g.say("  `2(`0L`2)eave a floppy for someone")
            g.say("  `2(`0T`2)alk to Dusty")
            g.say("  `2(`0R`2)eturn to town")
            g.say()
            g.say(f"  `2You have `${g.player.gold:,} gold`2 on hand.")
            k = g.key("  `2Your choice? [`0R`2] : ", "BLTR", "R")
            if k == "B":
                boot(g, s)
            elif k == "L":
                leave(g, s)
            elif k == "T":
                g.say()
                g.say("  `0\"" + random.choice(SAYINGS) + "\"")
                g.say()
                g.more()
            else:
                g.say()
                g.say("  `2You leave Dusty to his floppies.")
                g.pause(0.6)
                return


def install(folder, remove=False):
    """Add the two lines to (or take them off) LORD's 3RDPARTY.DAT, with
    DUSTY.IGM beside it: LORD only sends players to a program it can
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
            f.write(b"Dusty's Disk Cart: a flux/bbs native module ([dos] native).\r\n")
    open(path, "wb").write(("\r\n".join(out) + "\r\n").encode("cp437"))
    print(("took Dusty's Disk Cart off " if remove else "added Dusty's Disk Cart to ") + path)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] in ("install", "uninstall"):
        install(sys.argv[2] if len(sys.argv) > 2 else ".", sys.argv[1] == "uninstall")
    else:
        main()
