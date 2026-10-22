//! BIOS services other than video: keyboard (INT 16h), time (INT 1Ah),
//! system services (INT 15h), the multiplex interrupt (INT 2Fh).

use chrono::{Datelike, Timelike};

use crate::cpu::{self, Cpu};
use crate::machine::{BDA_MIDNIGHT, BDA_TICKS, System};
use crate::video::BDA;
use crate::{After, Memory};

fn bcd(v: u32) -> u8 {
    ((v / 10) << 4 | (v % 10)) as u8
}

/// `INT 16h`: the keyboard. Reads that find nothing block (the call
/// re-runs when the host has fed keys).
pub fn int16(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) -> After {
    match cpu.ah() {
        0x00 | 0x10 => match sys.keys.pop_front() {
            Some(k) => {
                sys.active();
                cpu.set_ax(k);
            }
            None => sys.block(cpu, mem),
        },
        0x01 | 0x11 => {
            // ZF set means no key; the flags go back through the stack frame.
            let sp = cpu.sp() as u32;
            let fl_addr = crate::linear(cpu.ss(), sp + 4);
            let mut fl = mem.read16(fl_addr);
            match sys.keys.front() {
                Some(&k) => {
                    cpu.set_ax(k);
                    fl &= !(cpu::ZF as u16);
                }
                None => {
                    sys.polled();
                    fl |= cpu::ZF as u16;
                }
            }
            mem.write16(fl_addr, fl);
        }
        0x02 | 0x12 => cpu.set_ax(mem.read16(BDA + 0x17) & 0xFF),
        0x03 => {}
        0x05 => {
            sys.keys.push_back(cpu.cx());
            cpu.set_al(0);
        }
        0x09 => cpu.set_al(0x0F),
        _ => {}
    }
    After::Iret
}

/// `INT 1Ah`: ticks and the real-time clock.
pub fn int1a(cpu: &mut Cpu, mem: &mut Memory) {
    let now = crate::now();
    match cpu.ah() {
        0x00 => {
            let t = mem.read32(BDA_TICKS);
            cpu.set_r16(cpu::CX, (t >> 16) as u16);
            cpu.set_r16(cpu::DX, t as u16);
            cpu.set_al(mem.read8(BDA_MIDNIGHT));
            mem.write8(BDA_MIDNIGHT, 0);
        }
        0x01 => {
            let t = (cpu.cx() as u32) << 16 | cpu.dx() as u32;
            mem.write32(BDA_TICKS, t);
        }
        0x02 => {
            cpu.set_r8(5, bcd(now.hour())); // CH
            cpu.set_r8(1, bcd(now.minute())); // CL
            cpu.set_r8(6, bcd(now.second())); // DH
            cpu.set_r8(2, 0); // DL: no DST
            clear_cf(cpu, mem);
        }
        0x04 => {
            let y = now.year() as u32;
            cpu.set_r8(5, bcd(y / 100));
            cpu.set_r8(1, bcd(y % 100));
            cpu.set_r8(6, bcd(now.month()));
            cpu.set_r8(2, bcd(now.day()));
            clear_cf(cpu, mem);
        }
        _ => {}
    }
}

/// Clear CF in the interrupt return frame (results of BIOS/DOS calls).
pub fn clear_cf(cpu: &Cpu, mem: &mut Memory) {
    set_cf(cpu, mem, false);
}

pub fn set_cf(cpu: &Cpu, mem: &mut Memory, on: bool) {
    let sp = cpu.sp() as u32;
    let a = crate::linear(cpu.ss(), sp + 4);
    let fl = mem.read16(a);
    mem.write16(a, if on { fl | cpu::CF as u16 } else { fl & !(cpu::CF as u16) });
}

/// Set ZF in the return frame.
pub fn set_zf(cpu: &Cpu, mem: &mut Memory, on: bool) {
    let sp = cpu.sp() as u32;
    let a = crate::linear(cpu.ss(), sp + 4);
    let fl = mem.read16(a);
    mem.write16(a, if on { fl | cpu::ZF as u16 } else { fl & !(cpu::ZF as u16) });
}

/// `INT 15h`: a 286-era subset.
pub fn int15(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) {
    match cpu.ah() {
        0x24 => {
            // A20: 00 disable, 01 enable, 02 query, 03 support.
            match cpu.al() {
                0 => mem.a20 = false,
                1 => mem.a20 = true,
                2 => cpu.set_al(mem.a20 as u8),
                _ => cpu.set_r16(cpu::BX, 3),
            }
            cpu.set_ah(0);
            clear_cf(cpu, mem);
        }
        0x4F => {
            // Keyboard intercept: keep the key.
            set_cf(cpu, mem, true);
        }
        0x86 => {
            // Wait CX:DX microseconds: count it as polling, so the host
            // may idle, and move the clock on.
            sys.polled();
            clear_cf(cpu, mem);
        }
        0x88 => {
            cpu.set_ax(0);
            clear_cf(cpu, mem);
        }
        0xC0 | 0xC1 | 0xE8 | 0x41 | 0x53 => {
            cpu.set_ah(0x86);
            set_cf(cpu, mem, true);
        }
        _ => {
            cpu.set_ah(0x86);
            set_cf(cpu, mem, true);
        }
    }
}

/// `INT 2Fh`: say nothing is installed, except the DOS idle call.
pub fn int2f(cpu: &mut Cpu) {
    match cpu.ax() {
        0x1680 => cpu.set_al(0), // idle: supported
        0x1600 => cpu.set_al(0), // no Windows
        0x4300 => {}             // no XMS
        _ => {}
    }
}
