# Scripting

Iniquity's IPL (the Iniquity Programming Language, compiled `.IPS` →
`.IPX` modules) is replaced by [Lua 5.4](https://www.lua.org/manual/5.4/),
built into the BBS. A script is a `.lua` file in `scripts/`
(`[paths] scripts` in `iniquity.toml`). Old `.IPS` modules don't run;
the table at the end shows what each IPL call became, so porting one is
mostly mechanical (see `scripts/oneliner.lua`, the stock ONELINER.IPS).

## Running a script

- From a menu: command `-X`, param `name args...`. `-X oneliner` runs
  `scripts/oneliner.lua`.
- From any prompt string or `-L`/`-E` line: `%!name args...`, as in the
  original. The string runs the script instead of being shown.

Names are letters, digits, `_`, `-` and `.`; case doesn't matter. The
arguments are split at spaces and given to the script as `...` and as
the table `arg`.

```lua
local who, count = ...   -- or arg[1], arg[2]
```

A script that fails shows the caller `ScriptError` (with the reason, if
the caller is a SysOp) and logs it. A script that runs for 10 seconds
without calling `bbs` (a loop gone wrong) is stopped, as is one whose
caller hangs up. Scripts get 64 MB of memory.

## The `bbs` table

### Output

| Call | Does |
|---|---|
| `bbs.write(s)` / `bbs.writeln([s])` | Text as it is, no codes |
| `bbs.show(s)` / `bbs.showln([s])` | Text with `\|XX` colour and `%XX` MCI codes, as prompt strings are shown |
| `bbs.say(name)` / `bbs.sayln(name)` | A string from the string table (`*E`), by name |
| `bbs.showfile(name)` → bool | Show a screen from the text directory (theme first); false if there's none |
| `bbs.newline([n])` | `n` new lines (1) |
| `bbs.cls()`, `bbs.clreol()`, `bbs.beep()` | Clear the screen, the rest of the line; beep |
| `bbs.gotoxy(x, y)` | Move the cursor (1-based) |
| `bbs.up([n])`, `bbs.down([n])`, `bbs.left([n])`, `bbs.right([n])` | Move the cursor |
| `bbs.color(fore, [back])` | Set the colour: 0-15 on 0-7 |
| `bbs.where()` → x, y | Where the cursor is |
| `bbs.pause()` | "Press any key" |

`print(...)` and `io.write(...)` write to the caller too.

### Input

| Call | Does |
|---|---|
| `bbs.getkey()` → string | One key: the character, or `"enter"`, `"backspace"`, `"tab"`, `"esc"`, `"up"`, `"down"`, `"left"`, `"right"`, `"home"`, `"end"`, `"pgup"`, `"pgdn"`, `"ins"`, `"del"` |
| `bbs.input([opts])` → string | A line. `opts`: `max` (40), `default`, `upper`, `mixed`, `password`, `required`, `allowed` (the characters that may be typed) |
| `bbs.yesno([default])` → bool | Yes or no, the board's way (bars or `[Y/n]`) |

### The board

| Call | Does |
|---|---|
| `bbs.menucmd(code, [param])` | Run any menu command (`"MN"`, `"?T"`, `"-/"`...), as a menu would |
| `bbs.mci(code)` → string or nil | An MCI code's value: `bbs.mci("UN")` is the caller's handle |
| `bbs.expand(s)` → string | `s` with its `%XX` codes filled in |
| `bbs.acs(expr)` → bool | Does the caller pass an ACS expression (`"s50&fA"`)? |
| `bbs.user()` → table | The caller's record (below) |
| `bbs.saveuser(t)` | Write changed fields back and save |
| `bbs.log(s)` | A line in the BBS log |
| `bbs.get(key)` → string or nil | A value this script stored |
| `bbs.set(key, value)` | Store a value (as text) for this script; `nil` removes it |
| `bbs.nocolor(s)`, `bbs.pad(s, n)` | Strip `\|XX` codes; pad or cut to `n` columns |
| `bbs.node`, `bbs.script`, `bbs.version` | Node number, this script's name, the BBS version |

`bbs.get`/`bbs.set` keep values in the board's database, separately for
each script, so a script can keep scores or settings between calls
without files.

### The user record

`bbs.user()` has `id`, `handle`, `real_name`, `location`, `address`,
`zip_code`, `phone`, `note`, `sysop_note`, `sex`, `birth_date`
(MM/DD/YY), `sl`, `dsl`, `level`, `flags` (letters), `hot_keys`,
`expert`, `pause`, `quote`, `yes_no_bars`, `page_length`, `start_menu`,
`total_calls`, `calls_today`, `first_call`, `last_call`, `time_today`,
`time_per_day`, `posts`, `email`, `uploads`, `downloads`, `upload_kb`,
`download_kb`, `file_points`, `cur_msg_area`, `cur_file_area`,
`conf_msg`, `conf_file`, `autosig` (a list of lines) and `theme`.

`bbs.saveuser` changes any of them except `id`, `handle` and the call,
post and transfer counters; a bad value (`sl = 300`, `sex = "X"`) is an
error. Passwords aren't there at all.

```lua
local u = bbs.user()
if u.sl < 50 and bbs.yesno(false) then
  u.sl = 50
  u.flags = u.flags .. "V"
  bbs.saveuser(u)
end
```

## Lua's own library

Scripts have Lua's standard library: `string`, `table`, `math`, `utf8`,
`coroutine`, `os` and `io` for files, and `require` for modules in
`scripts/` (`require "util"` loads `scripts/util.lua`). Left out:
`os.exit` (it would end the BBS), and anything that uses the console's
stdin or stdout (`io.read`, `io.lines()` without a file, `io.stdin`...,
`io.popen`). Scripts are the SysOp's own, so file access isn't
restricted otherwise.

## IPL to Lua

| IPL | Lua |
|---|---|
| `out`, `outln` | `bbs.write`, `bbs.writeln` |
| `cout`, `coutln`, `sout`, `soutln`, `xout` | `bbs.show`, `bbs.showln` |
| `strout[n]`, `stroutln[n]` | `bbs.say(name)`, `bbs.sayln(name)` (strings go by name now) |
| `clrscr`, `clreol`, `beep`, `dnln[n]`, `fline` | `bbs.cls()`, `bbs.clreol()`, `bbs.beep()`, `bbs.newline(n)` |
| `gotoxy`, `posup`/`posdown`/`posleft`/`posright` | `bbs.gotoxy`, `bbs.up`/`down`/`left`/`right` |
| `setcolor[f,b]`, `setfore`, `setback` | `bbs.color(f, b)` |
| `wherex`, `wherey` | `bbs.where()` |
| `showtext`, `showfile` | `bbs.showfile` |
| `inkey` | `bbs.getkey()` |
| `instr[...]`, `instrf[...]` | `bbs.input{...}` |
| `inyesno[d]` | `bbs.yesno(d)` |
| `strup`, `strlow`, `strpos`, `strcopy`, `strreplace`, `strrepeat` | `s:upper()`, `s:lower()`, `s:find(x, 1, true)`, `s:sub(i, j)`, `s:gsub(...)`, `s:rep(n)` |
| `strnocol`, `strsize` | `bbs.nocolor`, `bbs.pad` |
| `strint`, `strreal` | `tostring`, `string.format("%.2f", n)` |
| `iplpar[n]`, `iplnumpar` | `arg[n]`, `#arg` (or `...`) |
| `userget` / `uhandle`... / `userput` + `usersave` | `local u = bbs.user()` / `u.handle`... / `bbs.saveuser(u)` |
| `menucmd[c,p]` | `bbs.menucmd(c, p)` |
| `inputstring = x` | `bbs.menucmd("?=", x)` |
| `logwrite[s]` | `bbs.log(s)` |
| `@ str x`, `proc`, `if`/`while`/`repeat`/`for`, `goto` | `local x`, `local function`, Lua's own |

Not carried over: `userload` (scripts work on the caller), the internal
switches (`localio`, `screenoff`, `tempsysop`...), and IPL modules that
replaced built-in parts of the board (`APPLY.IPX` for the new-user
application).
