//! The FOSSIL (`INT 14h`, revision 5) that doors talk to the caller
//! through, with the BIOS serial functions underneath. A door checking
//! for a FOSSIL looks for 1954h six bytes into the INT 14h handler, or
//! calls function 04h; both are here. The caller's bytes are the port's
//! input, what the door sends is the port's output; lowering DTR asks
//! the host to hang up.

use crate::cpu::{self, CALLBACK_SEG, Cpu, callback_offset};
use crate::machine::System;
use crate::video::Screen;
use crate::{After, Memory, linear};

const FOSSIL_SIG: u16 = 0x1954;
const MAX_FUNCTION: u8 = 0x1B;
const REVISION: u8 = 5;
const ID_OFFSET: u16 = 0x1E00;
const ID: &[u8] = b"flux/bbs FOSSIL (built in)\0";
const BUFFER_SIZE: u16 = 4096;

/// Put the signature at the handler and the driver name in the ROM.
pub fn install(mem: &mut Memory) {
    let entry = linear(CALLBACK_SEG, callback_offset(0x14) as u32);
    mem.write16(entry + 6, FOSSIL_SIG);
    mem.write8(entry + 8, MAX_FUNCTION);
    mem.write8(entry + 9, REVISION);
    mem.write(linear(CALLBACK_SEG, ID_OFFSET as u32), ID);
}

/// Line status in AH, modem status in AL (FOSSIL layout).
fn status(sys: &System) -> u16 {
    let mut ah: u8 = 0x60; // transmitter empty, holding register empty
    if !sys.input.is_empty() {
        ah |= 0x01;
    }
    let mut al: u8 = 0x08 | 0x10 | 0x20; // always 1, CTS, DSR
    if sys.carrier {
        al |= 0x80;
    }
    (ah as u16) << 8 | al as u16
}

pub fn int14(sys: &mut System, cpu: &mut Cpu, mem: &mut Memory) -> After {
    // Any FOSSIL or BIOS serial call means the door is talking to the
    // caller through the port.
    sys.go_remote();
    match cpu.ah() {
        // Initialise port (BIOS): parameters in AL; status back.
        0x00 => {
            sys.active();
            cpu.set_ax(status(sys));
        }
        // Send with wait.
        0x01 => {
            sys.active();
            sys.output.push(cpu.al());
            cpu.set_ax(status(sys));
        }
        // Receive with wait.
        0x02 => match sys.input.pop_front() {
            Some(b) => {
                sys.active();
                cpu.set_al(b);
                cpu.set_ah(0);
            }
            None => sys.block(cpu, mem),
        },
        // Status.
        0x03 => {
            sys.polled();
            cpu.set_ax(status(sys));
        }
        // Initialise driver.
        0x04 => {
            sys.active();
            cpu.set_ax(FOSSIL_SIG);
            cpu.set_r8(3, MAX_FUNCTION); // BL
            cpu.set_r8(7, REVISION); // BH
        }
        0x05 => {}
        // Raise/lower DTR.
        0x06 => {
            if cpu.al() == 0 {
                sys.hangup = true;
            }
        }
        // Timer tick information.
        0x07 => {
            cpu.set_ah(0x1C);
            cpu.set_al(18);
            cpu.set_r16(cpu::DX, 55);
        }
        0x08..=0x0A => {
            if cpu.ah() == 0x0A {
                sys.input.clear();
            }
        }
        // Transmit without wait.
        0x0B => {
            sys.active();
            sys.output.push(cpu.al());
            cpu.set_ax(1);
        }
        // Peek ahead.
        0x0C => match sys.input.front() {
            Some(&b) => cpu.set_ax(b as u16),
            None => {
                sys.polled();
                cpu.set_ax(0xFFFF);
            }
        },
        // Keyboard: read without wait / with wait (the local keyboard).
        0x0D => match sys.keys.pop_front() {
            Some(k) => cpu.set_ax(k),
            None => {
                sys.polled();
                cpu.set_ax(0xFFFF);
            }
        },
        0x0E => match sys.keys.pop_front() {
            Some(k) => cpu.set_ax(k),
            None => sys.block(cpu, mem),
        },
        // Flow control, break checks: nothing to do.
        0x0F => {}
        0x10 => cpu.set_ax(0),
        // Cursor and screen writes through the FOSSIL.
        0x11 => Screen::set_cursor(mem, cpu.r8(2) as u16, cpu.r8(6) as u16),
        0x12 => {
            let (x, y) = Screen::cursor(mem);
            cpu.set_r16(cpu::DX, x | y << 8);
        }
        0x13 | 0x15 => Screen::teletype(mem, cpu.al(), 0x07),
        0x14 => cpu.set_ax(0),
        0x16 => cpu.set_ax(0),
        0x17 => {}
        // Read block: up to CX bytes into ES:DI.
        0x18 => {
            let n = cpu.cx() as usize;
            let mut got = 0u16;
            let mut off = cpu.di() as u32;
            while (got as usize) < n {
                let Some(b) = sys.input.pop_front() else { break };
                mem.write8(linear(cpu.es(), off), b);
                off = off.wrapping_add(1);
                got += 1;
            }
            if got == 0 {
                sys.polled();
            } else {
                sys.active();
            }
            cpu.set_ax(got);
        }
        // Write block from ES:DI.
        0x19 => {
            let n = cpu.cx() as u32;
            for i in 0..n {
                let b = mem.read8(linear(cpu.es(), cpu.di() as u32 + i));
                sys.output.push(b);
            }
            sys.active();
            cpu.set_ax(n as u16);
        }
        0x1A => {}
        // Driver information block into ES:DI.
        0x1B => {
            let base = linear(cpu.es(), cpu.di() as u32);
            let n = cpu.cx().min(19) as u32;
            let mut info = Vec::new();
            info.extend(19u16.to_le_bytes());
            info.push(REVISION);
            info.push(1);
            info.extend(ID_OFFSET.to_le_bytes());
            info.extend(CALLBACK_SEG.to_le_bytes());
            info.extend(BUFFER_SIZE.to_le_bytes());
            info.extend((BUFFER_SIZE - sys.input.len().min(BUFFER_SIZE as usize) as u16).to_le_bytes());
            info.extend(BUFFER_SIZE.to_le_bytes());
            info.extend(BUFFER_SIZE.to_le_bytes());
            info.push(80);
            info.push(25);
            info.push(0xE3); // 38400 8N1
            for (i, b) in info.iter().take(n as usize).enumerate() {
                mem.write8(base + i as u32, *b);
            }
            cpu.set_ax(n as u16);
        }
        0x1C | 0x1D => cpu.set_ax(FOSSIL_SIG),
        _ => {}
    }
    After::Iret
}
