"""A lightbar menu, after the stock IQEMAIL.IPS: pick an entry with the
arrow keys (or its number) and Enter; Q or Esc quits. The choice is
remembered for next time with bbs.set/bbs.get.

    -X lightbar
"""
import fluxbbs as bbs

items = ["the sysop", "the co-sysops", "everyone on the board", "nobody, really"]
title = "|09send a message to who?"

longest = max(len(s) for s in items)
top = 12 - len(items) // 2
left = 40 - longest // 2


def bar(i, on):
    bbs.gotoxy(left - 1, top + i)
    bbs.color(15, 1) if on else bbs.color(8, 0)
    bbs.write(" " + bbs.pad(items[i], longest) + " ")


bbs.color(7, 0)
bbs.cls()
bbs.gotoxy(left, top - 2)
bbs.show(title)
last = bbs.get("last")
cur = int(last) if last and last.isdigit() and int(last) < len(items) else 0
for i in range(len(items)):
    bar(i, i == cur)

while True:
    key = bbs.getkey()
    new = cur
    if key in ("up", "8"):
        new = (cur - 1) % len(items)
    elif key in ("down", "2"):
        new = (cur + 1) % len(items)
    elif key.isdigit() and 1 <= int(key) <= len(items):
        new = int(key) - 1
    elif key == "enter":
        break
    elif key == "esc" or key.lower() == "q":
        cur = None
        break
    if new != cur:
        bar(cur, False)
        cur = new
        bar(cur, True)

bbs.color(7, 0)
bbs.gotoxy(1, top + len(items) + 1)
if cur is not None:
    bbs.set("last", cur)
    bbs.showln(f"|07you picked |15{items[cur]}|07, {bbs.user()['handle']}.")
    bbs.log(f"picked {items[cur]}")
else:
    bbs.showln("|07never mind.")
