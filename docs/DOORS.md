# Doors

A door is a menu command `D0`–`D4`, `D7` or `DZ` whose parameter is a shell
command. The letter picks the drop file, written to `data/doors/nodeN/`
(`[paths] doors`), which is also where the door runs:

    [[command]]
    keys = "1"
    command = "D2"
    param = "/opt/doors/mygame/start.sh %PN %ND"

The door gets a pseudo-terminal (80 columns, the caller's rows, their
`TERM`) wired to the caller, so curses programs work. Its output is
taken as CP437 and translated for UTF-8 callers. If the caller hangs up,
or their time runs out, the door's process group gets SIGHUP, then
SIGKILL.

The parameter can use `%PN` (the node's drop directory), `%PD`, `%PT`,
`%PM` (data, text and menu directories), `%BR`/`%BP` (baud, always
38400), `%CP` (COM port, 1) and any MCI code (`%UN`, `%ND`, `%TL`...).
Every value goes in shell-quoted, so don't put quotes around codes.

**DOS doors.** A command whose program ends in `.BAT`, `.COM` or `.EXE`
(the stock menus' `LORD.BAT`, say) runs in the built-in DOS machine
(`crates/fluxbbs-dos`): a real-mode x86 with 640 KB, the BIOS video,
keyboard and timer services, a FOSSIL (revision 5) and an 8250 COM1
wired to the caller, and enough of DOS 5 for door programs (files,
directories, memory, exec, batch files). Drop a door into a directory
under the DOS drive (`doors/dos`, which is C: and on the PATH), point a
menu command at its batch file, and it runs: no DOSBox and no FOSSIL
driver to install. The node's drop directory is D:, the current drive,
so `LORD.BAT` finds DOOR.SYS where the original left it. What a door
prints on its own screen before it opens the port is mirrored to the
caller, as a door run in local mode would want. A door that lowers DTR
hangs the caller up, as it would have; the machine's clock keeps 18.2
ticks a second whatever the host's speed; a caller going away drops
carrier, and a door that doesn't notice within a few seconds is
stopped. In DOS commands `%PN` is `D:\`, and values lose the characters
DOS would act on (`<>|"%`).

LORD, for instance: unzip it (and the LORD.ZIP inside) into
`doors/dos/LORD`, run LORDCFG once (as a door, or with `dosrun`) to
make LORD.DAT (leave through "Exit the program" and save), and give
each node a `NODEn.DAT` (4.07 ships NODE0.DAT to copy; 4.06 doesn't,
but takes the same file) with `BBSTYPE DOORSYS`, `COMPORT 1`, `FOSSIL`
and `BBSDROP D:\`. Then `doors/dos/LORD.BAT` is `@ECHO OFF`, `C:`,
`CD \LORD`, `CALL START.BAT %1`, and the menu's `DZ` parameter
`LORD.BAT %ND`. Use **4.06**: in 4.07, examining the etchings in the
Dark Cloak Tavern ends the game with "Range check error" at
2A1D:0A9A. That's 4.07's own bug (it does the same under DOSBox's DOS
and CPU with the BNU FOSSIL, and 4.06 shows the ranking fine); the
player is back at the menu with their character intact, nothing more.

BRE (Barren Realms Elite 0.988, `brev988.exe`, an ARJ self-extractor
that runs in the machine): extract it into `doors/dos/BRE`, run
`BREDATA -Y` there (what UNPACK.BAT does), and skip INSTALL.EXE: write
SETUP.SR for its SRDOOR by hand, `D:\DOOR.SYS` then the DOOR.SYS line
numbers INSTALL would have put (name 10, last name 0, IBM characters
and ANSI 20, page length 21, baud 2, COM port 1, time left 19, real
name 0), and RESOURCE.DAT as `Fossil Y`. Then `BRE RESET` once with
`dosrun` (Esc saves the settings), and `doors/dos/BRE.BAT` is `@ECHO
OFF`, `C:`, `CD \BRE`, `SRDOOR`, `BRE`, run by `DZ`. Reset it in the
board's time zone (`TZ=America/Chicago dosrun ...` when `[bbs]
time_zone` is set): BRE refuses to start ("Computer Clock has been
tampered with") if the clock is earlier than the last time it saw. One
player at a time; others are told the game is in use (INUSE.FLG, which
BRE removes itself when the caller hangs up).

TradeWars 2002 (3.09, `2002V309.ZIP`; its TW2002, TEDIT, BIGBANG and
EXTERN need the coprocessor): unzip it into `doors/dos/TW2002` and do
what its INSTALL.BAT would, with the TWSUPP.EXE and TWGOLD.EXE inside
(both self-extracting zips) unpacked into `ANSI` and `GOLD` and
TWPGM.EXE's programs into the game directory. Run BIGBANG (an
unregistered copy builds the default 1,000-sector universe; `Z` and
Enter starts it), then TEDIT's `O` for node 1: drop file path `D:\`,
type `7` (DOOR.SYS), I/O type FOSSIL. Unregistered, it runs one node
only, so every caller is node 1: `doors/dos/TW2002.BAT` is `@ECHO OFF`,
`C:`, `CD \TW2002`, `TW2002 TWNODE=1 SHARE`, run by `DZ`, and
`TW2002.BAT` goes in `[dos] one_at_a_time` (two callers on one node's
game would trample each other's files; the next caller is told to try
later). Registered (TEDIT's `G`: `Y` the name it's registered to, `Z`
the code, `X` the SysOp; menu picks there are single keys, so don't
follow them with Enter), it takes a caller on every node: give each of
the board's nodes the same settings in `O`, make the batch file's line
`TW2002 TWNODE=%1 SHARE`, the menu's parameter `TW2002.BAT %ND`, and
take it out of `one_at_a_time`. Its nightly maintenance
is an event: `command = "dos TWMAINT.BAT"`, a batch file running
`EXTERN SHARE` in `\TW2002`. Run BIGBANG and TEDIT in the board's time
zone, as with BRE. Prompts that look like hot keys mostly want Enter.

Operation: Overkill II (1.20, `oo120.zip` from breakintochat.com's
Nulf collection): unzip it, and the MAPFILES.ZIP and MAPTEXT.ZIP inside,
into `doors/dos/OOII`. Run OOSETUP once from `\OOII` (a batch file with
`C:`, `CD \OOII`, `OOSETUP`, under `dosrun`) to write OOCONFIG.DAT, a
text file: BBS type `GAP` (DOOR.SYS), path to BBS data `D:\`, comm
support `FOSSIL`. Leave OOSETUP with Esc, `Y` to exit and `Y` to save:
that's when it makes the game's data files. `doors/dos/OOII.BAT` is
`@ECHO OFF`, `C:`, `CD \OOII`, `OOINFO 2 D:\`, `MAINTOO`, `OOII`, run by
`DZ` and listed in `[dos] one_at_a_time`: it plays one at a time
whatever its "Multi-Node?" says (that only has a second player told to
come back later), and OOINFO writes each caller to the one BBSINFO.OO,
which two starting together would share. Unregistered, it takes 10 players; registration was free, but
its site is gone. A character whose caller hangs up before its creation
is finished is dead until midnight; a hangup in play just camps.

Usurper (0.19, `USURP019.ZIP` from breakintochat.com's Dangarden
collection; it says "Registered Copy" by itself): unzip it into
`doors/dos/USURPER`, make a `DOCS` directory there (it wants one; the
manuals go in it), and write `USURP.CTL` (DDplus's control file) with
`BBSTYPE DOORSYS`, `FOSSIL`, `STATUS OFF` and the SysOp's and board's
names. Run EDITOR (from `\USURPER`) and choose Reset Game, then
`USURPER /L` once, which makes the rest of its files and stops at
"[Press a key]". `doors/dos/USURPER.BAT` is `@ECHO OFF`, `C:`, `CD
\USURPER`, `USURPER /PD:\`, run by `DZ`. Several callers can play at
once, each on their own node (DOOR.SYS says which): they see each other
online and share the game's files through SHARE (below).
Before DDplus reads its control file it polls "port 0" (I/O ports 5 and
6, the DMA controller) for CTS and carrier, so its first messages go
nowhere: the machine's DMA ports read 0, as a real one's unused
channels do.

Multi-node doors work as they did on a network or under a multitasker
with SHARE loaded: every node's machine runs in the board's process, and
they share one table of open files and locks. Opening with a deny mode
(`3Dh` with bits 4-6 set) fails against the opens it conflicts with:
access denied (05h), or a sharing violation (20h) for an open in
compatibility mode (DOS's INT 24h case), as in Ralf Brown's table. A
create (`3Ch`, or `6Ch` replacing) can't empty a file another open
holds with a deny mode, and such a file can't be deleted or renamed
(compatibility opens don't stop either: single-node doors erase and
rename files they have open).
`5Ch` locks and unlocks byte ranges, and a read or write into another
process's locked range fails with a lock violation (21h, at once: the
retry count of `440Bh` is taken and ignored). Closing a file lets go of
the locks made through it; a program ending or a caller hanging up lets
go of everything they held. A caller whose link stops taking output for
a minute loses carrier, as at a hangup, so the door saves and goes and
a dead connection can't hold a game's locks. `INT 2Fh` 1000h says SHARE is installed.
Compatibility-mode opens (no deny bits) deny nothing, as under DOSBox,
since single-node doors open that way and still expect to share files
with their own maintenance programs. Temporary files (`5Ah`) get the
first free name.

`[dos] one_at_a_time` lists doors (by program or batch file, as in the
menu) that only one caller may be in at once: single-player games, and
unregistered copies that run on one node.

Not in the machine: protected mode (DPMI doors), EMS/XMS, sound. The
coprocessor is a 387 whose registers are 64-bit doubles rather than
80-bit extended reals: plenty for doors, not bit-exact. For a door that needs one, `[dos] emulator` names a DOSBox
command (`dosbox-staging -noprimaryconf -nolocalconf -conf %C`), with
`setup` for DOS commands run before each door (a FOSSIL driver such as
BNU or X00, kept on the DOS drive) and `config` for extra dosbox.conf
lines; COM1 is then a null modem back to the BBS, DOSBox's output goes
to `dosbox.log` in the drop directory, and since DOSBox doesn't drop
carrier when its socket closes, a caller hanging up stops DOSBox (TERM,
then KILL) rather than the door noticing.

Changes from the original: each node gets its own drop directory; the
DOOR.SYS password line is empty; CHAIN.TXT has real times and transfer
totals, and points its directories at the drop directory; DOOR32.SYS
(`D4`) is new.

From 2.0: `D7` writes DOOR.SYS and reads it back when the door ends,
taking the time left (up to the day's allowance) and the upload,
download and today's-kbytes counts (only upwards); never the security
level. While a door runs, other callers see its name: what follows a
`;` in the parameter (`LORD.BAT;Legend of the Red Dragon`), else the
menu item's description, else the program's name.
