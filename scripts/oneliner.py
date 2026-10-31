"""Oneliners: the stock ONELINER.IPS (Mike Fricker, 03/10/96) in Python.
Run it from a menu with command -X, param "oneliner"."""
import fluxbbs as bbs

bbs.color(7, 0)
bbs.cls()
bbs.showfile("onelntop")
if not bbs.showfile("onelnmid"):
    print("no oneliners currently exist.")
bbs.showfile("onelnbot")

bbs.show("|12would you like to add something? ")
if bbs.yesno(False):
    line = bbs.input(max=76)
    if not line:
        print("[aborted]")
    else:
        bbs.newline(2)
        bbs.write(f"adding '{line}' ...")
        # ?= sets the menu input string; ?T adds it to a text file,
        # keeping the last 12 lines.
        bbs.menucmd("?=", line)
        bbs.menucmd("?T", "onelnmid.ans;12")
        print(" done.")
