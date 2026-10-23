#!/usr/bin/env python3
"""Check a `dosrun` trace against QEMU's CPU (through Unicorn).

    replay_trace.py trace.bin trace.mem [max_reports]
    replay_trace.py trace.bin --chunk K [max_reports]   (after trace_chunks.py)

Every recorded instruction is run on its own in Unicorn from the same
starting state, and its result compared with what our CPU did (the next
record's state and the bytes written). Records from native interrupt
handlers, INT/HLT/IN instructions (the other emulator can't see our
devices) and hardware-interrupt frames are passed over. Differences are
reported with the instruction's bytes, grouped by opcode, so what's shown
is only how two emulators disagree on one instruction. Needs the
`unicorn` package.
"""
import collections
import json
import mmap
import os
import struct
import sys

from unicorn import UC_ARCH_X86, UC_HOOK_MEM_WRITE, UC_MODE_16, Uc, UcError
from unicorn.x86_const import (UC_X86_REG_CS, UC_X86_REG_DS, UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                               UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EFLAGS, UC_X86_REG_EIP,
                               UC_X86_REG_ES, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FS, UC_X86_REG_GS,
                               UC_X86_REG_SS)

REGS = [UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EBX, UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI]
SREGS = [UC_X86_REG_ES, UC_X86_REG_CS, UC_X86_REG_SS, UC_X86_REG_DS, UC_X86_REG_FS, UC_X86_REG_GS]
RNAMES = 'eax ecx edx ebx esp ebp esi edi'.split()
SNAMES = 'es cs ss ds fs gs'.split()
PREFIXES = {0x26, 0x2E, 0x36, 0x3E, 0x64, 0x65, 0x66, 0x67, 0xF0, 0xF2, 0xF3}
AF = 0x10
HEAD = struct.Struct('<B8I6HHII')


def load(path, start=8, count=None):
    f = open(path, 'rb')
    data = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
    n, = struct.unpack_from('<Q', data, 0)
    if count is not None:
        n = count
    offs = []
    p = start
    for _ in range(n):
        if p >= len(data):
            break
        offs.append(p)
        nw = struct.unpack_from('<I', data, p + HEAD.size - 4)[0]
        p += HEAD.size + 6 * nw
    return data, offs


def record(data, off):
    f = HEAD.unpack_from(data, off)
    native, regs, sregs, ip, flags, nw = f[0], f[1:9], f[9:15], f[15], f[16], f[17]
    w = [struct.unpack_from('<IBB', data, off + HEAD.size + 6 * i) for i in range(nw)]
    return native, regs, sregs, ip, flags, w


def main():
    max_reports = int(sys.argv[3]) if len(sys.argv) > 3 else 40
    first = 0
    if sys.argv[2] == '--chunk':
        k = int(sys.argv[3])
        max_reports = int(sys.argv[4]) if len(sys.argv) > 4 else 40
        d = os.path.dirname(os.path.abspath(sys.argv[1]))
        info = json.load(open(os.path.join(d, f'chunk_{k}.json')))
        first = info['first']
        # One record past the chunk, to check its last instruction against.
        data, offs = load(sys.argv[1], info['offset'], info['count'] + 1)
        mem = bytearray(open(os.path.join(d, f'chunk_{k}.mem'), 'rb').read())
    else:
        data, offs = load(sys.argv[1])
        mem = bytearray(open(sys.argv[2], 'rb').read())
        # The memory as it was when the window began: undo every write.
        for off in reversed(offs):
            for a, old, _ in reversed(record(data, off)[5]):
                mem[a] = old
    uc = Uc(UC_ARCH_X86, UC_MODE_16)
    uc.mem_map(0, 0x110000)
    uc.mem_write(0, bytes(mem))
    uc_writes = []
    uc.hook_add(UC_HOOK_MEM_WRITE, lambda u, acc, addr, size, val, ud: uc_writes.append((addr, size, val)))
    reports = collections.Counter()
    shown = 0
    skipped = collections.Counter()
    checked = 0
    for k in range(len(offs) - 1):
        native, regs, sregs, ip, flags, writes = record(data, offs[k])
        lin = (sregs[1] << 4) + ip
        # The opcode, past any prefixes.
        i = lin
        rep = False
        while mem[i] in PREFIXES:
            rep |= mem[i] in (0xF2, 0xF3)
            i += 1
        op = mem[i]
        op2 = mem[i + 1]
        skip = None
        if native:
            skip = 'native'
        elif op in (0xCC, 0xCD, 0xCE, 0xF4):
            skip = 'int/hlt'
        elif op in (0xE4, 0xE5, 0xEC, 0xED, 0x6C, 0x6D):
            skip = 'in'
        elif 0xD8 <= op <= 0xDF:
            skip = 'fpu'
        if skip:
            skipped[skip] += 1
        else:
            for r, v in zip(REGS, regs):
                uc.reg_write(r, v)
            for r, v in zip(SREGS, sregs):
                uc.reg_write(r, v)
            uc.reg_write(UC_X86_REG_EFLAGS, flags)
            uc_writes.clear()
            err = None
            try:
                # A REP string instruction is one record; Unicorn runs it an
                # iteration at a time.
                for _ in range(70000):
                    uc.emu_start(lin, 0x7FFFFFFF, count=1)
                    if not rep or uc.reg_read(UC_X86_REG_EIP) != ip:
                        break
                    lin = (uc.reg_read(UC_X86_REG_CS) << 4) + uc.reg_read(UC_X86_REG_EIP)
            except UcError as e:
                err = str(e)
            got_regs = [uc.reg_read(r) for r in REGS]
            got_sregs = [uc.reg_read(r) for r in SREGS]
            got_ip = uc.reg_read(UC_X86_REG_EIP) & 0xFFFF
            got_flags = uc.reg_read(UC_X86_REG_EFLAGS)
            n_native, n_regs, n_sregs, n_ip, n_flags, _ = record(data, offs[k + 1])
            why = []
            if err:
                why.append(err)
            # A hardware interrupt taken after this instruction: the next
            # record is the handler's start, with our result on its stack.
            irq = False
            if (n_sregs[1], n_ip) != (got_sregs[1], got_ip) and (got_regs[4] - n_regs[4]) & 0xFFFF == 6:
                sp = (n_sregs[2] << 4) + (n_regs[4] & 0xFFFF)
                # The frame our machine pushed is in this record's writes.
                pushed = {a: new for a, _, new in writes}
                def word(a):
                    return pushed.get(a, mem[a]) | pushed.get(a + 1, mem[a + 1]) << 8
                irq = word(sp) == got_ip and word(sp + 2) == got_sregs[1] and word(sp + 4) == (got_flags & 0xFFFF)
            if not irq:
                for name, g, w in zip(RNAMES, got_regs, n_regs):
                    if g != w:
                        why.append(f'{name} {g:#x} vs ours {w:#x}')
                for name, g, w in zip(SNAMES, got_sregs, n_sregs):
                    if g != w:
                        why.append(f'{name} {g:#x} vs ours {w:#x}')
                if got_ip != n_ip:
                    why.append(f'ip {got_ip:#x} vs ours {n_ip:#x}')
                if (got_flags ^ n_flags) & 0xFFFF & ~AF:
                    why.append(f'flags {got_flags & 0xffff:#06x} vs ours {n_flags & 0xffff:#06x}')
                theirs = {}
                for a, size, val in uc_writes:
                    for j in range(size):
                        theirs[a + j] = (val >> (8 * j)) & 0xFF
                ours = {a: new for a, _, new in writes}
                # Our record also holds what the machine wrote between
                # instructions (an interrupt frame); only count bytes the
                # other emulator wrote differently or we didn't write.
                for a, v in theirs.items():
                    if ours.get(a) != v:
                        why.append(f'[{a:#x}] {v:#04x} vs ours {ours.get(a)}')
            # Put the other emulator's memory back and move both on by our record.
            for a, size, _ in uc_writes:
                uc.mem_write(a, bytes(mem[a:a + size]))
            checked += 1
            if why:
                code = ' '.join(f'{mem[j]:02x}' for j in range(i, min(i + 7, len(mem))))
                key = f'{op:02x}' + (f' {op2 >> 3 & 7}' if op in (0x80, 0x81, 0x83, 0xC0, 0xC1, 0xD0, 0xD1, 0xD2, 0xD3, 0xF6, 0xF7, 0xFE, 0xFF) else '') + (f' {op2:02x}' if op == 0x0F else '')
                reports[key] += 1
                if shown < max_reports:
                    shown += 1
                    print(f'#{first + k} {sregs[1]:04x}:{ip:04x} [{code}] {"; ".join(why)}', flush=True)
        for a, _, new in writes:
            mem[a] = new
        if writes:
            for a, _, new in writes:
                uc.mem_write(a, bytes([new]))
    print(f'{checked} instructions checked, {sum(reports.values())} differ; skipped {dict(skipped)}')
    for key, n in reports.most_common():
        print(f'  {key}: {n}')


if __name__ == '__main__':
    main()
