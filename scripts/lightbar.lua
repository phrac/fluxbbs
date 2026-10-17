-- A lightbar menu, after the stock IQEMAIL.IPS: pick an entry with the
-- arrow keys (or its number) and Enter; Q or Esc quits. The choice is
-- remembered for next time with bbs.set/bbs.get.
--
--   -X lightbar

local items = {
  "the sysop",
  "the co-sysops",
  "everyone on the board",
  "nobody, really",
}
local title = "|09send a message to who?"

local longest = 0
for _, s in ipairs(items) do longest = math.max(longest, #s) end
local top = 12 - #items // 2
local left = 40 - longest // 2

local function bar(i, on)
  bbs.gotoxy(left - 1, top + i - 1)
  if on then bbs.color(15, 1) else bbs.color(8, 0) end
  bbs.write(" " .. bbs.pad(items[i], longest) .. " ")
end

bbs.color(7, 0)
bbs.cls()
bbs.gotoxy(left, top - 2)
bbs.show(title)
local cur = tonumber(bbs.get("last")) or 1
for i = 1, #items do bar(i, i == cur) end

while true do
  local key = bbs.getkey()
  local new = cur
  if key == "up" or key == "8" then
    new = cur > 1 and cur - 1 or #items
  elseif key == "down" or key == "2" then
    new = cur < #items and cur + 1 or 1
  elseif tonumber(key) and items[tonumber(key)] then
    new = tonumber(key)
  elseif key == "enter" then
    break
  elseif key == "esc" or key:lower() == "q" then
    cur = nil
    break
  end
  if new ~= cur then
    bar(cur, false)
    cur = new
    bar(cur, true)
  end
end

bbs.color(7, 0)
bbs.gotoxy(1, top + #items + 1)
if cur then
  bbs.set("last", cur)
  bbs.showln("|07you picked |15" .. items[cur] .. "|07, " .. bbs.user().handle .. ".")
  bbs.log("picked " .. items[cur])
else
  bbs.showln("|07never mind.")
end
