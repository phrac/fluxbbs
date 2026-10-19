# Doors

A door is a menu command `D0`–`D4` or `DZ` whose parameter is a shell
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
(`crates/iniquity-dos`): a real-mode x86 with 640 KB, the BIOS video,
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

Not in the machine: protected mode (DPMI doors), EMS/XMS, the FPU,
sound. For a door that needs one, `[dos] emulator` names a DOSBox
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
