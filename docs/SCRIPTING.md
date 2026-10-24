# Scripting

flux/bbs runs scripts in [Lua 5.4](https://www.lua.org/manual/5.4/),
built into the board. Lua takes the place of Iniquity's IPL (the
Iniquity Programming Language, compiled `.IPS` → `.IPX` modules).

- A script is a `.lua` file in `scripts/` (`[paths] scripts` in
  `fluxbbs.toml`).
- Old `.IPS` modules don't run. Porting one is mostly mechanical: see
  [IPL to Lua](#ipl-to-lua) below.
- Two stock scripts show the way: `scripts/oneliner.lua` (the stock
  ONELINER.IPS) and `scripts/lightbar.lua` (a lightbar pick list after
  the stock IQEMAIL.IPS).

## Running a script

| From | How | Example |
|---|---|---|
| A menu | command `-X`, param `name args...` | `-X oneliner` runs `scripts/oneliner.lua` |
| A prompt string, or a `-L`/`-E` line | `%!name args...`, as in the original | the string runs the script instead of being shown |

**Names** are letters, digits, `_`, `-` and `.`; case doesn't matter.
A trailing `.lua` may be given or left off.

**Arguments** are split at spaces. The script gets them as `...` and as
the table `arg`:

```lua
local who, count = ...   -- or arg[1], arg[2]
```

## Limits and errors

| What | Then |
|---|---|
| The script fails | the caller sees `ScriptError` (with the reason, if they're a SysOp), and it's logged |
| It runs 10 seconds without calling `bbs` (a loop gone wrong) | it's stopped |
| The caller hangs up | it's stopped |
| Memory | 64 MB at most |
| Scripts running scripts (`%!` inside a `bbs.show`, say) | 4 deep at most |

## The `bbs` table

Brackets mark optional arguments; `→` is what a call returns.

### Output

| Call | Does |
|---|---|
| `bbs.write(s)` | write text as it is, no codes |
| `bbs.writeln([s])` | the same, then a new line |
| `bbs.show(s)` | text with `\|XX` color and `%XX` MCI codes, as prompt strings are shown |
| `bbs.showln([s])` | the same, then a new line |
| `bbs.say(name)` | a string from the string table (`*E`), by name |
| `bbs.sayln(name)` | the same, then a new line |
| `bbs.showfile(name)` → bool | show a screen from the text directory (theme first); false if there's none |
| `bbs.newline([n])` | `n` new lines (default 1) |
| `bbs.cls()` | clear the screen |
| `bbs.clreol()` | clear the rest of the line |
| `bbs.beep()` | beep |
| `bbs.gotoxy(x, y)` | move the cursor (1-based) |
| `bbs.up([n])` `bbs.down([n])` `bbs.left([n])` `bbs.right([n])` | move the cursor `n` places (default 1) |
| `bbs.color(fore, [back])` | set the color: 0-15 on 0-7 |
| `bbs.where()` → x, y | where the cursor is |
| `bbs.pause()` | "press any key" |

`print(...)` and `io.write(...)` write to the caller too.

### Input

| Call | Does |
|---|---|
| `bbs.getkey()` → string | one key (below) |
| `bbs.input([opts])` → string | a line (options below) |
| `bbs.yesno([default])` → bool | yes or no, the board's way (bars or `[Y/n]`); the default is yes |

`bbs.getkey()` returns the character typed, or one of these names:
`"enter"`, `"backspace"`, `"tab"`, `"esc"`, `"up"`, `"down"`,
`"left"`, `"right"`, `"home"`, `"end"`, `"pgup"`, `"pgdn"`, `"ins"`,
`"del"`.

**`bbs.input` options** (a table, e.g. `bbs.input{max = 10, upper = true}`):

| Option | Does |
|---|---|
| `max` | longest answer (default 40; 1-255) |
| `default` | text already filled in |
| `upper` | upper case only |
| `mixed` | mixed case |
| `password` | hide what's typed |
| `required` | an answer must be given |
| `allowed` | the characters that may be typed |
| `complete` | Tab completes a handle |

As with `?I`, codes typed into the answer (other than colors) come back
wrapped in `|ZA`...`|ZB`, so they show but don't act wherever the text
is shown later. Passwords come back as typed.

### The board

| Call | Does |
|---|---|
| `bbs.menucmd(code, [param])` | run any menu command (`"MN"`, `"?T"`, `"-/"`...), as a menu would |
| `bbs.mci(code)` → string or nil | an MCI code's value: `bbs.mci("UN")` is the caller's handle |
| `bbs.expand(s)` → string | `s` with its `%XX` codes filled in |
| `bbs.acs(expr)` → bool | does the caller pass an ACS expression (`"s50&fA"`)? |
| `bbs.inmenu(keys)` → bool | does the current menu have a command on these keys (2.0's `isincurmenu`)? |
| `bbs.user()` → table | the caller's record (see [The user record](#the-user-record)) |
| `bbs.saveuser(t)` | write changed fields back and save |
| `bbs.log(s)` | a line in the BBS log |
| `bbs.nocolor(s)` | `s` without its `\|XX` codes |
| `bbs.pad(s, n)` | `s` padded or cut to `n` columns |

### Stored values

| Call | Does |
|---|---|
| `bbs.get(key)` → string or nil | a value this script stored |
| `bbs.set(key, value)` | store a value (as text) for this script; `nil` removes it |

Values live in the board's database, kept apart for each script. So a
script can keep scores or settings between calls without files.

### Fields

| Field | Is |
|---|---|
| `bbs.node` | the node number |
| `bbs.script` | this script's name |
| `bbs.version` | the BBS version |

## The user record

`bbs.user()` returns a table of the caller's record. Passwords aren't in
it at all.

### Fields a script can change

`bbs.saveuser(t)` writes these back. A bad value is an error (e.g.
`sl = 300`, `sex = "X"`). Writing back a field unchanged is always fine.

| Field | Is |
|---|---|
| `real_name`, `location`, `address`, `zip_code`, `phone`, `note`, `sysop_note` | text |
| `start_menu`, `theme` | text |
| `sex` | `"M"`, `"F"`, `"O"` (other) or `"P"` (not given) |
| `birth_date` | `MM/DD/YY`, or blank |
| `sl`, `dsl` | 0-255 |
| `level` | a letter A-Z |
| `flags` | letters A-Z |
| `hot_keys`, `expert`, `pause`, `quote`, `yes_no_bars` | true or false |
| `page_length` | 1-200 |
| `time_today` | minutes, 0-65535 |
| `file_points` | a number |
| `cur_msg_area`, `cur_file_area`, `conf_msg`, `conf_file` | numbers |
| `autosig` | a list of lines (up to 6 kept) |
| `bank_time`, `bank_kb` | the time bank: minutes, kilobytes |

### Fields a script can only read

Changing one of these is an error.

| Field | Is |
|---|---|
| `id` | user number |
| `handle` | handle |
| `total_calls`, `calls_today` | calls |
| `first_call`, `last_call` | dates |
| `time_per_day` | minutes a day |
| `posts`, `email` | posts and email sent |
| `uploads`, `downloads`, `upload_kb`, `download_kb` | transfers |

### Example

```lua
local u = bbs.user()
if u.sl < 50 and bbs.yesno(false) then
  u.sl = 50
  u.flags = u.flags .. "V"
  bbs.saveuser(u)
end
```

## Lua's own library

Scripts have Lua's standard library:

- `string`, `table`, `math`, `utf8`, `coroutine`;
- `os`, and `io` for files;
- `require` for modules in `scripts/`: `require "util"` loads
  `scripts/util.lua`.

Left out:

- `os.exit` (it would end the BBS);
- anything that uses the console's stdin or stdout: `io.read`,
  `io.lines()` without a file, `io.stdin`..., `io.popen`.

Scripts are the SysOp's own, so file access isn't restricted otherwise.

## IPL to Lua

What each IPL call became.

### Output and input

| IPL | Lua |
|---|---|
| `out`, `outln` | `bbs.write`, `bbs.writeln` |
| `cout`, `coutln`, `sout`, `soutln`, `xout` | `bbs.show`, `bbs.showln` |
| `strout[n]`, `stroutln[n]` | `bbs.say(name)`, `bbs.sayln(name)` (strings go by name now) |
| `clrscr`, `clreol`, `beep` | `bbs.cls()`, `bbs.clreol()`, `bbs.beep()` |
| `dnln[n]`, `fline` | `bbs.newline(n)` |
| `gotoxy` | `bbs.gotoxy` |
| `posup`, `posdown`, `posleft`, `posright` | `bbs.up`, `bbs.down`, `bbs.left`, `bbs.right` |
| `setcolor[f,b]`, `setfore`, `setback` | `bbs.color(f, b)` |
| `wherex`, `wherey` | `bbs.where()` |
| `showtext`, `showfile` | `bbs.showfile` |
| `inkey` | `bbs.getkey()` |
| `instr[...]`, `instrf[...]` | `bbs.input{...}` |
| `inyesno[d]` | `bbs.yesno(d)` |

### Strings and numbers

| IPL | Lua |
|---|---|
| `strup`, `strlow` | `s:upper()`, `s:lower()` |
| `strpos` | `s:find(x, 1, true)` |
| `strcopy` | `s:sub(i, j)` |
| `strreplace` | `s:gsub(...)` |
| `strrepeat` | `s:rep(n)` |
| `strnocol` | `bbs.nocolor` |
| `strsize` | `bbs.pad` |
| `strint` | `tostring` |
| `strreal` | `string.format("%.2f", n)` |

### The board and the language

| IPL | Lua |
|---|---|
| `iplpar[n]`, `iplnumpar` | `arg[n]`, `#arg` (or `...`) |
| `userget` / `uhandle`... / `userput` + `usersave` | `local u = bbs.user()` / `u.handle`... / `bbs.saveuser(u)` |
| `menucmd[c,p]` | `bbs.menucmd(c, p)` |
| `inputstring = x` | `bbs.menucmd("?=", x)` |
| `logwrite[s]` | `bbs.log(s)` |
| `@ str x` | `local x` |
| `proc` | `local function` |
| `if`, `while`, `repeat`, `for`, `goto` | Lua's own |

### 2.0's additions

| IPL | Lua |
|---|---|
| `strmci` | `bbs.expand` |
| `strnocolor` | `bbs.nocolor` |
| `ascok` | `bbs.acs` |
| `isincurmenu` | `bbs.inmenu` |
| `random` | `math.random(0, n)` |
| `delay` | `bbs.menucmd("-D", seconds)`, or `\|DM`...`\|DS` in a `bbs.show` |
| `asciichr` | `string.char` |
| math (`sin`, `sqrt`, `logx`...) | Lua's `math` |

### Not carried over

- `userload`: scripts work on the caller.
- The internal switches (`localio`, `screenoff`, `tempsysop`...).
- IPL modules that replaced built-in parts of the board (`APPLY.IPX`
  for the new-user application).
