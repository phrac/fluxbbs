-- Oneliners: the stock ONELINER.IPS (Mike Fricker, 03/10/96) in Lua.
-- Run it from a menu with command -X, param "oneliner".

bbs.color(7, 0)
bbs.cls()
bbs.showfile("onelntop")
if not bbs.showfile("onelnmid") then
  bbs.writeln("no oneliners currently exist.")
end
bbs.showfile("onelnbot")

bbs.show("|12would you like to add something? ")
if not bbs.yesno(false) then return end

local line = bbs.input{max = 76}
if line == "" then
  bbs.writeln("[aborted]")
  return
end
bbs.newline(2)
bbs.write("adding '" .. line .. "' ...")
-- ?= sets the menu input string; ?T adds it to a text file, keeping the
-- last 12 lines.
bbs.menucmd("?=", line)
bbs.menucmd("?T", "onelnmid.ans;12")
bbs.writeln(" done.")
