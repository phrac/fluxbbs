# Scripting

flux/bbs scripts are [Python](https://docs.python.org/3/) programs.
Python takes the place of Iniquity's IPL (the Iniquity Programming
Language, compiled `.IPS` → `.IPX` modules): rather than ship a language
of its own, the board gives Python a module, `fluxbbs`, for talking to
the caller and the board.

```python
import fluxbbs as bbs

bbs.cls()
bbs.showln("|15hello, |11" + bbs.user()["handle"])
if bbs.yesno(False):
    name = bbs.input(max=30)
```

- A script is a `.py` file in `scripts/` (`[paths] scripts` in
  `fluxbbs.toml`).
- Old `.IPS` modules don't run. Porting one is mostly mechanical: see
  [IPL to Python](#ipl-to-python) below.
- Two stock scripts show the way: `scripts/oneliner.py` (the stock
  ONELINER.IPS) and `scripts/lightbar.py` (a lightbar pick list after
  the stock IQEMAIL.IPS).
- The board needs Python 3 on the host (`python3`, or `[paths] python`
  in `fluxbbs.toml`, `*C` Paths `X`); the Docker image has it. Scripts
  are the `python` feature, which `full` includes (docs/BUILDING.md).

## How it works

Each script runs as a process of its own. The board starts
`python3` with the script, and the two talk over a private socket: each
`bbs.*` call is a line of JSON to the board, which does it and answers.
The script never touches the caller's connection itself, so a script
that crashes, hangs or leaks memory can't take the node (or the board)
down with it.

The `fluxbbs` module comes with the board, which writes it where scripts
find it (`data/cache/python/`) each time it starts, so it always matches
the board's version. To use an editor's completion, point it there.

`print()` and `input()` go to the caller too: `print()` writes as
`bbs.write` does, and `input("Name: ")` asks as `bbs.input(max=255)`.
What a script writes to stderr goes to the BBS log when it fails;
a SysOp sees the last line of it.

## Running a script

| From | How | Example |
|---|---|---|
| A menu | command `-X`, param `name args...` | `-X oneliner` runs `scripts/oneliner.py` |
| A prompt string, or a `-L`/`-E` line | `%!name args...`, as in the original | the string runs the script instead of being shown |

**Names** are letters, digits, `_`, `-` and `.`; case doesn't matter.
A trailing `.py` may be given or left off.

**Arguments** are split at spaces. The script gets them as `bbs.args`
(and `sys.argv[1:]`, as any Python program would):

```python
who, count = bbs.args[:2]
```

**Modules**: a script can `import` other `.py` files beside it in
`scripts/`, and anything installed for that Python (`pip`, a venv's
`python` in `[paths] python`).

## Limits and errors

| What | Then |
|---|---|
| The script fails (an exception) | the caller sees `ScriptError` (with the reason, if they're a SysOp), and it's logged |
| It runs 60 seconds without calling `bbs` (a loop gone wrong, a slow site) | it's stopped |
| The caller hangs up | it's stopped: its next `bbs` call ends it quietly (`SystemExit`), and the board kills it |
| Scripts running scripts (`%!` inside a `bbs.show`, say) | 4 deep at most |

A call the board turns down (a bad user field, say) raises
`bbs.Error`, which a script may catch.

Waiting on the caller doesn't count: `bbs.getkey()` may wait as long as
the board's own idle limit allows.

## The `fluxbbs` module

Brackets mark optional arguments; `→` is what a call returns. Keyword
arguments are Python's own: `bbs.input(max=10, upper=True)`.

### Output

| Call | Does |
|---|---|
| `bbs.write(s)` | write text as it is, no codes |
| `bbs.writeln([s])` | the same, then a new line |
| `bbs.show(s)` | text with `\|XX` color and `%XX` MCI codes, as prompt strings are shown |
| `bbs.showln([s])` | the same, then a new line |
| `bbs.say(name)` | a string from the string table (`*E`), by name |
| `bbs.sayln(name)` | the same, then a new line |
| `bbs.showfile(name)` → bool | show a screen from the text directory (theme first); `False` if there's none |
| `bbs.newline([n])` | `n` new lines (default 1) |
| `bbs.cls()` | clear the screen |
| `bbs.clreol()` | clear the rest of the line |
| `bbs.beep()` | beep |
| `bbs.gotoxy(x, y)` | move the cursor (1-based) |
| `bbs.up([n])` `bbs.down([n])` `bbs.left([n])` `bbs.right([n])` | move the cursor `n` places (default 1) |
| `bbs.color(fore, [back])` | set the color: 0-15 on 0-7 |
| `bbs.where()` → (x, y) | where the cursor is |
| `bbs.pause()` | "press any key" |

### Input

| Call | Does |
|---|---|
| `bbs.getkey()` → str | one key (below) |
| `bbs.input([options])` → str | a line (options below) |
| `bbs.yesno([default])` → bool | yes or no, the board's way (bars or `[Y/n]`); the default is `True` |

`bbs.getkey()` returns the character typed, or one of these names:
`"enter"`, `"backspace"`, `"tab"`, `"esc"`, `"up"`, `"down"`,
`"left"`, `"right"`, `"home"`, `"end"`, `"pgup"`, `"pgdn"`, `"ins"`,
`"del"`.

**`bbs.input` options** (keywords, e.g. `bbs.input(max=10, upper=True)`):

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
| `bbs.mci(code)` → str or None | an MCI code's value: `bbs.mci("UN")` is the caller's handle |
| `bbs.expand(s)` → str | `s` with its `%XX` codes filled in |
| `bbs.acs(expr)` → bool | does the caller pass an ACS expression (`"s50&fA"`)? |
| `bbs.inmenu(keys)` → bool | does the current menu have a command on these keys (2.0's `isincurmenu`)? |
| `bbs.user()` → dict | the caller's record (see [The user record](#the-user-record)) |
| `bbs.saveuser(t)` | write changed fields back and save |
| `bbs.log(s)` | a line in the BBS log |
| `bbs.nocolor(s)` → str | `s` without its `\|XX` codes |
| `bbs.pad(s, n)` → str | `s` padded or cut to `n` columns |

### Stored values

| Call | Does |
|---|---|
| `bbs.get(key)` → str or None | a value this script stored |
| `bbs.set(key, value)` | store a value (as text) for this script; `None` removes it |

Values live in the board's database, kept apart for each script. So a
script can keep scores or settings between calls without files.

### Values

| Field | Is |
|---|---|
| `bbs.node` | the node number |
| `bbs.script` | this script's name |
| `bbs.version` | the BBS version |
| `bbs.args` | the words after the script's name |
| `bbs.Error` | what a call the board turned down raises |

## The user record

`bbs.user()` returns a dict of the caller's record. Passwords aren't in
it at all.

### Fields a script can change

`bbs.saveuser(t)` writes these back. A bad value raises `bbs.Error` (e.g.
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
| `hot_keys`, `expert`, `pause`, `quote`, `yes_no_bars` | `True` or `False` |
| `page_length` | 1-200 |
| `time_today` | minutes, 0-65535 |
| `file_points` | a number |
| `cur_msg_area`, `cur_file_area`, `conf_msg`, `conf_file` | numbers |
| `autosig` | a list of lines (up to 6 kept) |
| `bank_time`, `bank_kb` | the time bank: minutes, kilobytes |

### Fields a script can only read

Changing one of these raises `bbs.Error`.

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

```python
u = bbs.user()
if u["sl"] < 50 and bbs.yesno(False):
    u["sl"] = 50
    u["flags"] += "V"
    bbs.saveuser(u)
```

Only the fields given are written, so `bbs.saveuser({"note": "hi"})`
works too.

## Python's own library

Scripts have all of Python: files, `sqlite3`, `json`, `urllib`,
`random`, `datetime`... Scripts are the SysOp's own, so nothing is
restricted. Keep in mind:

- stdin isn't the caller: use `bbs.getkey()` / `bbs.input()` (or the
  `input()` the module puts in place of Python's).
- `sys.exit()` ends the script, not the board; the caller goes back to
  the menu.
- Anything slow (a web request) should finish inside the 60 seconds, or
  show something to the caller now and then.

## IPL to Python

What each IPL call became.

### Output and input

| IPL | Python |
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
| `instr[...]`, `instrf[...]` | `bbs.input(...)` |
| `inyesno[d]` | `bbs.yesno(d)` |

### Strings and numbers

| IPL | Python |
|---|---|
| `strup`, `strlow` | `s.upper()`, `s.lower()` |
| `strpos` | `s.find(x) + 1` |
| `strcopy` | `s[i:j]` |
| `strreplace` | `s.replace(a, b)` |
| `strrepeat` | `s * n` |
| `strnocol` | `bbs.nocolor` |
| `strsize` | `bbs.pad` |
| `strint` | `str(n)` |
| `strreal` | `f"{n:.2f}"` |

### The board and the language

| IPL | Python |
|---|---|
| `iplpar[n]`, `iplnumpar` | `bbs.args[n-1]`, `len(bbs.args)` |
| `userget` / `uhandle`... / `userput` + `usersave` | `u = bbs.user()` / `u["handle"]`... / `bbs.saveuser(u)` |
| `menucmd[c,p]` | `bbs.menucmd(c, p)` |
| `inputstring = x` | `bbs.menucmd("?=", x)` |
| `logwrite[s]` | `bbs.log(s)` |
| `@ str x` | `x = ""` |
| `proc` | `def` |
| `if`, `while`, `repeat`, `for`, `goto` | Python's own (`goto` becomes a loop or a function) |

### 2.0's additions

| IPL | Python |
|---|---|
| `strmci` | `bbs.expand` |
| `strnocolor` | `bbs.nocolor` |
| `ascok` | `bbs.acs` |
| `isincurmenu` | `bbs.inmenu` |
| `random` | `random.randint(0, n)` |
| `delay` | `bbs.menucmd("-D", seconds)`, or `\|DM`...`\|DS` in a `bbs.show` |
| `asciichr` | `chr` |
| math (`sin`, `sqrt`, `logx`...) | Python's `math` |

### Not carried over

- `userload`: scripts work on the caller.
- The internal switches (`localio`, `screenoff`, `tempsysop`...).
- IPL modules that replaced built-in parts of the board (`APPLY.IPX`
  for the new-user application).
