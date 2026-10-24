# Doors

A door is an outside program a caller plays from a menu. flux/bbs runs
two kinds:

- **Native doors**: any Linux program or script.
- **DOS doors**: `.BAT`, `.COM` and `.EXE` programs, run in a DOS
  machine built into the board. No DOSBox or FOSSIL driver to install.

Both are set up the same way: a menu command and a shell command.

## Adding a door to a menu

A door is a menu command `D0`–`D4`, `D7` or `DZ`. Its parameter is the
command to run:

```toml
[[command]]
keys = "1"
command = "D2"
param = "/opt/doors/mygame/start.sh %PN %ND"
```

### Drop files

The command's letter picks the drop file. It is written to the node's
own drop directory, `data/doors/nodeN/` (`[paths] doors`), and the door
runs in that directory.

| Command | Drop file |
|---|---|
| `D0` | none |
| `D1` | DORINFO1.DEF |
| `D2` | DOOR.SYS |
| `D3` | CHAIN.TXT |
| `D4` | DOOR32.SYS |
| `D7` | DOOR.SYS, read back when the door ends (see below) |
| `DZ` | DOOR.SYS and DORINFO1.DEF |

### Codes in the parameter

| Code | Is |
|---|---|
| `%PN` | the node's drop directory (`D:\` in DOS commands) |
| `%PD` | the data directory |
| `%PT` | the text directory |
| `%PM` | the menu directory |
| `%BR`, `%BP` | the baud rate, always 38400 |
| `%CP` | the COM port, 1 |
| any MCI code | `%UN`, `%ND`, `%TL`... |

Every value goes in shell-quoted, so don't put quotes around codes.

### The door's name in the status

While a door runs, other callers see its name. It is taken from, in
order:

1. what follows a `;` in the parameter (`LORD.BAT;Legend of the Red
   Dragon`);
2. the menu item's description;
3. the program's name.

### `D7`: DOOR.SYS read back

`D7` (from 2.0) writes DOOR.SYS and reads it back when the door ends.
It takes:

- the time left, up to the day's allowance;
- the upload, download and today's-kbytes counts, only upwards.

It never takes the security level.

### How the drop files differ from the original's

- Each node gets its own drop directory.
- DOOR.SYS's password line is empty.
- CHAIN.TXT has real times and transfer totals, and points its
  directories at the drop directory.
- DOOR32.SYS (`D4`) is new.

## Native doors

A native door gets a pseudo-terminal wired to the caller: 80 columns,
the caller's rows and their `TERM`. Curses programs work.

- Its output is taken as CP437 and translated for UTF-8 callers.
- If the caller hangs up, or their time runs out, the door's process
  group gets SIGHUP, then SIGKILL.

## DOS doors

A command whose program ends in `.BAT`, `.COM` or `.EXE` (the stock
menus' `LORD.BAT`, say) runs in the built-in DOS machine
(`crates/fluxbbs-dos`).

### Installing one

1. Put the door in a directory under the DOS drive, `doors/dos`
   (`[dos] drive`). That drive is C: and is on the PATH.
2. Write a batch file for it in `doors/dos`.
3. Point a menu command at the batch file.

The node's drop directory is D:, the current drive. So a door finds
DOOR.SYS on D:, where the original left it.

In DOS commands, `%PN` is `D:\`. Values lose the characters DOS would
act on (`<>|"%`).

### What the machine has

- A real-mode x86 with 640 KB.
- A coprocessor: a 387 whose registers are 64-bit doubles rather than
  80-bit extended reals. Plenty for doors, but not bit-exact.
- BIOS video, keyboard and timer services.
- A FOSSIL (revision 5) and an 8250 COM1, both wired to the caller.
- Enough of DOS 5 for door programs: files, directories, memory, exec
  and batch files.
- SHARE across the nodes (see [Multi-node doors and
  SHARE](#multi-node-doors-and-share)).

Not in the machine: protected mode (DPMI doors), EMS/XMS, sound. For a
door that needs them, see [Using DOSBox instead](#using-dosbox-instead).

### How it behaves

- What a door prints on its own screen before it opens the port is
  mirrored to the caller, as a door run in local mode would want.
- The clock keeps 18.2 ticks a second, whatever the host's speed.
- The DMA ports read 0, as a real machine's unused channels do.

### Carrier and hangups

- A door that lowers DTR hangs the caller up, as it would have.
- A caller going away drops carrier. A door that doesn't notice within
  a few seconds is stopped.
- A caller whose link stops taking output for a minute loses carrier,
  as at a hangup. The door saves and goes, and a dead connection can't
  hold a game's locks.

### Trying a door with `dosrun`

`dosrun` runs a program in the machine outside the board. The game
setups below use it for setup programs. Build it from the repo:

```sh
cargo build --release -p fluxbbs-dos --examples
target/release/examples/dosrun C_ROOT D_ROOT CMD
```

Standard input is the caller, standard output what they'd see, and the
machine's log goes to standard error.

### Doors one caller at a time

`[dos] one_at_a_time` lists doors that only one caller may be in at
once. Name them by program or batch file, as in the menu
(`TW2002.BAT`). The next caller is told to try later.

Use it for:

- single-player games;
- unregistered copies that run on one node.

### Multi-node doors and SHARE

Multi-node doors work as they did on a network, or under a multitasker
with SHARE loaded. Every node's machine runs in the board's process,
and they share one table of open files and locks. `INT 2Fh` 1000h says
SHARE is installed.

#### Opening files

- An open with a deny mode (`3Dh` with bits 4-6 set) fails against the
  opens it conflicts with, as in Ralf Brown's table.
- Compatibility-mode opens (no deny bits) deny nothing, as under
  DOSBox. Single-node doors open that way and still expect to share
  files with their own maintenance programs.

#### Creating, deleting and renaming

- A create (`3Ch`, or `6Ch` replacing) can't empty a file that another
  open holds with a deny mode.
- Such a file can't be deleted or renamed either.
- Compatibility opens stop neither: single-node doors erase and rename
  files they have open.
- Temporary files (`5Ah`) get the first free name.

#### Record locks

- `5Ch` locks and unlocks byte ranges.
- A read or write into another process's locked range fails at once.
  The retry count of `440Bh` is taken and ignored.

#### Letting go

- Closing a file lets go of the locks made through it.
- A program ending, or a caller hanging up, lets go of everything they
  held.

#### Error codes

| Code | Name | When |
|---|---|---|
| 05h | access denied | an open conflicts with another open's deny mode |
| 20h | sharing violation | the same, for an open in compatibility mode (DOS's INT 24h case) |
| 21h | lock violation | a read or write into another process's locked range |

### Using DOSBox instead

For a door the built-in machine can't run, `[dos] emulator` names a
DOSBox command:

```toml
[dos]
emulator = "dosbox-staging -noprimaryconf -nolocalconf -conf %C"
```

| Setting | Is |
|---|---|
| `emulator` | the DOSBox command; `%C` is the config written for each door. Empty (the default) uses the built-in machine. |
| `setup` | DOS commands run before each door, such as a FOSSIL driver (BNU or X00, kept on the DOS drive) |
| `config` | extra dosbox.conf lines |

Under DOSBox:

- COM1 is a null modem back to the board.
- DOSBox's output goes to `dosbox.log` in the drop directory.
- DOSBox doesn't drop carrier when its socket closes. So a caller
  hanging up stops DOSBox (TERM, then KILL), rather than the door
  noticing.

## Setting up games

The batch files go in `doors/dos`; the menu entries go in a menu such
as `DOOR.toml` (their `keys` are only examples). Where two games go in
`[dos] one_at_a_time`, list both: `["TW2002.BAT", "OOII.BAT"]`.

Run setup programs in the board's time zone where noted
(`TZ=America/Chicago dosrun ...` when `[bbs] time_zone` is set).

### Legend of the Red Dragon (LORD)

Use **4.06** (see the notes).

1. Unzip LORD, and the LORD.ZIP inside it, into `doors/dos/LORD`.
2. Run LORDCFG once, as a door or with `dosrun`, to make LORD.DAT.
   Leave through "Exit the program" and save.
3. Give each node a `NODEn.DAT` with `BBSTYPE DOORSYS`, `COMPORT 1`,
   `FOSSIL` and `BBSDROP D:\`. 4.07 ships NODE0.DAT to copy; 4.06
   doesn't, but takes the same file.
4. Write `doors/dos/LORD.BAT`:

   ```bat
   @ECHO OFF
   C:
   CD \LORD
   CALL START.BAT %1
   ```

5. Add the menu entry:

   ```toml
   [[command]]
   keys = "1"
   command = "DZ"
   param = "LORD.BAT %ND"
   ```

Notes:

- Multi-node: each node needs its own `NODEn.DAT` (step 3).
- In 4.07, examining the etchings in the Dark Cloak Tavern ends the
  game with "Range check error" at 2A1D:0A9A. That's 4.07's own bug: it
  does the same under DOSBox's DOS and CPU with the BNU FOSSIL, and 4.06
  shows the ranking fine. The player is back at the menu with their
  character intact, nothing more.

### Barren Realms Elite (BRE)

Version 0.988, `brev988.exe`: an ARJ self-extractor that runs in the
machine.

1. Extract it into `doors/dos/BRE`.
2. Run `BREDATA -Y` there (what UNPACK.BAT does).
3. Skip INSTALL.EXE. Write SETUP.SR for its SRDOOR by hand:
   `D:\DOOR.SYS`, then the DOOR.SYS line numbers INSTALL would have put:

   | Field | Line |
   |---|---|
   | name | 10 |
   | last name | 0 |
   | IBM characters and ANSI | 20 |
   | page length | 21 |
   | baud | 2 |
   | COM port | 1 |
   | time left | 19 |
   | real name | 0 |

4. Write RESOURCE.DAT as `Fossil Y`.
5. Run `BRE RESET` once with `dosrun`, in the board's time zone. Esc
   saves the settings.
6. Write `doors/dos/BRE.BAT`:

   ```bat
   @ECHO OFF
   C:
   CD \BRE
   SRDOOR
   BRE
   ```

7. Add the menu entry:

   ```toml
   [[command]]
   keys = "2"
   command = "DZ"
   param = "BRE.BAT"
   ```

Notes:

- One player at a time. Others are told the game is in use
  (INUSE.FLG, which BRE removes itself when the caller hangs up).
- BRE refuses to start ("Computer Clock has been tampered with") if the
  clock is earlier than the last time it saw. That's why the reset runs
  in the board's time zone.

### TradeWars 2002

Version 3.09, `2002V309.ZIP`. Its TW2002, TEDIT, BIGBANG and EXTERN
need the coprocessor.

1. Unzip it into `doors/dos/TW2002`.
2. Do what its INSTALL.BAT would:
   - unpack TWSUPP.EXE into `ANSI` and TWGOLD.EXE into `GOLD` (both are
     self-extracting zips);
   - unpack TWPGM.EXE's programs into the game directory.
3. Run BIGBANG, in the board's time zone. An unregistered copy builds
   the default 1,000-sector universe; `Z` and Enter starts it.
4. Run TEDIT, also in the board's time zone. In `O`, for node 1, set:
   - drop file path `D:\`;
   - type `7` (DOOR.SYS);
   - I/O type FOSSIL.
5. Write `doors/dos/TW2002.BAT`:

   ```bat
   @ECHO OFF
   C:
   CD \TW2002
   TW2002 TWNODE=1 SHARE
   ```

6. Add the menu entry:

   ```toml
   [[command]]
   keys = "3"
   command = "DZ"
   param = "TW2002.BAT"
   ```

7. Unregistered, it runs one node only, so every caller is node 1. Put
   it in `[dos] one_at_a_time` (two callers on one node's game would
   trample each other's files):

   ```toml
   [dos]
   one_at_a_time = ["TW2002.BAT"]
   ```

8. Add its nightly maintenance as an event (`*V`). `TWMAINT.BAT` runs
   `EXTERN SHARE` in `\TW2002`:

   ```toml
   [[event]]
   desc = "TW2002 maintenance"
   time = "00:05"          # any time
   command = "dos TWMAINT.BAT"
   ```

   ```bat
   @ECHO OFF
   C:
   CD \TW2002
   EXTERN SHARE
   ```

#### Registered, on every node

Register it in TEDIT's `G`: `Y` is the name it's registered to, `Z` the
code, `X` the SysOp. Then it takes a caller on every node:

1. In TEDIT's `O`, give each of the board's nodes the same settings as
   node 1.
2. Make the batch file's line `TW2002 TWNODE=%1 SHARE`.
3. Make the menu's parameter `TW2002.BAT %ND`.
4. Take it out of `one_at_a_time`.

Notes:

- TEDIT's menu picks are single keys: don't follow them with Enter.
- Elsewhere, prompts that look like hot keys mostly want Enter.

### Operation: Overkill II

Version 1.20, `oo120.zip`, from breakintochat.com's Nulf collection.

1. Unzip it, and the MAPFILES.ZIP and MAPTEXT.ZIP inside, into
   `doors/dos/OOII`.
2. Run OOSETUP once from `\OOII`, with a batch file under `dosrun`:

   ```bat
   C:
   CD \OOII
   OOSETUP
   ```

   It writes OOCONFIG.DAT, a text file. Set:
   - BBS type `GAP` (DOOR.SYS);
   - path to BBS data `D:\`;
   - comm support `FOSSIL`.
3. Leave OOSETUP with Esc, `Y` to exit and `Y` to save. That's when it
   makes the game's data files.
4. Write `doors/dos/OOII.BAT`:

   ```bat
   @ECHO OFF
   C:
   CD \OOII
   OOINFO 2 D:\
   MAINTOO
   OOII
   ```

5. Add the menu entry:

   ```toml
   [[command]]
   keys = "4"
   command = "DZ"
   param = "OOII.BAT"
   ```

6. List it in `[dos] one_at_a_time`:

   ```toml
   [dos]
   one_at_a_time = ["OOII.BAT"]
   ```

Notes:

- One at a time, whatever its "Multi-Node?" setting says. That setting
  only has a second player told to come back later.
- OOINFO writes each caller to the one BBSINFO.OO, which two callers
  starting together would share.
- Unregistered, it takes 10 players. Registration was free, but its
  site is gone.
- A character whose caller hangs up before its creation is finished is
  dead until midnight. A hangup in play just camps.

### Usurper

Version 0.19, `USURP019.ZIP`, from breakintochat.com's Dangarden
collection. It says "Registered Copy" by itself.

1. Unzip it into `doors/dos/USURPER`.
2. Make a `DOCS` directory there. It wants one; the manuals go in it.
3. Write `USURP.CTL` (DDplus's control file) with `BBSTYPE DOORSYS`,
   `FOSSIL`, `STATUS OFF` and the SysOp's and board's names.
4. Run EDITOR (from `\USURPER`) and choose Reset Game.
5. Run `USURPER /L` once. It makes the rest of its files and stops at
   "[Press a key]".
6. Write `doors/dos/USURPER.BAT`:

   ```bat
   @ECHO OFF
   C:
   CD \USURPER
   USURPER /PD:\
   ```

7. Add the menu entry:

   ```toml
   [[command]]
   keys = "5"
   command = "DZ"
   param = "USURPER.BAT"
   ```

Notes:

- Several callers can play at once, each on their own node (DOOR.SYS
  says which). They see each other online and share the game's files
  through [SHARE](#multi-node-doors-and-share).
- Before DDplus reads its control file, it polls "port 0" (I/O ports 5
  and 6, the DMA controller) for CTS and carrier. So its first messages
  go nowhere. The machine's DMA ports read 0, as a real one's unused
  channels do.
