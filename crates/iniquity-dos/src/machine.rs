//! The machine: CPU, memory, the devices on the bus, and the run loop
//! that paces them. The host feeds it bytes from the caller and takes
//! bytes for the caller; what the door prints on its own screen is
//! mirrored to the caller until the door opens the serial port, as a
//! door run in local mode would want.
//!
//! Time is counted in instructions: a timer tick every
//! [`TICK_INSTRUCTIONS`], so delays inside the program come out the
//! same whatever the host's speed (and Turbo Pascal's `Delay`
//! calibration never overflows). The host sleeps to keep ticks at the
//! real 18.2 a second, and when the program is waiting for input the
//! run loop says so instead of spinning.

use std::collections::VecDeque;
use std::path::Path;

use crate::cpu::{self, CALLBACK_SEG, Cpu, IRET_STUB, Size, callback_offset};
use crate::dos::Dos;
use crate::serial::{COM1_BASE, COM1_IRQ, Uart};
use crate::video::{BDA, Screen};
use crate::{After, Bus, Memory, linear};

/// Instructions per timer tick (18.2 ticks a second): a fast 286.
pub const TICK_INSTRUCTIONS: u64 = 400_000;
/// BIOS tick count (dword) and the midnight flag.
pub const BDA_TICKS: u32 = BDA + 0x6C;
pub const BDA_MIDNIGHT: u32 = BDA + 0x70;
const BDA_KBD_HEAD: u32 = BDA + 0x1A;
const BDA_KBD_TAIL: u32 = BDA + 0x1C;
const BDA_KBD_START: u32 = BDA + 0x80;
const BDA_KBD_END: u32 = BDA + 0x82;

/// Why the run loop stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The budget ran out; call again.
    Running,
    /// Waiting for input (or halted): nothing happens until the host
    /// feeds input or advances time.
    Idle,
    /// The program ended with this code.
    Exited(u8),
    /// Something the emulator can't do; the program is dead.
    Fault(String),
}

/// Devices and the OS layer: everything the CPU talks to.
pub struct System {
    pub uart: Uart,
    pub dos: Dos,
    /// Bytes from the caller, for the serial port (or, before the door
    /// opens it, the keyboard).
    pub input: VecDeque<u8>,
    /// Bytes for the caller.
    pub output: Vec<u8>,
    pub carrier: bool,
    /// The door has used the serial port or FOSSIL: the caller is on the
    /// line, not the keyboard.
    pub remote: bool,
    /// The door lowered DTR: it wants the caller hung up.
    pub hangup: bool,
    pub screen: Screen,
    /// Pending hardware interrupts, one bit per IRQ.
    irq_pending: u8,
    pic_mask: u8,
    tick_pending: u32,
    /// Set by a handler that couldn't finish (no input yet): the
    /// interrupt will run again when there is.
    pub blocked: bool,
    /// Consecutive status polls with nothing new.
    polls: u32,
    pub exit: Option<u8>,
    pub fault: Option<String>,
    /// Keys waiting for the program's keyboard (scan code << 8 | ASCII).
    pub keys: VecDeque<u16>,
    /// A partial escape sequence from the caller, in console mode.
    esc: Vec<u8>,
    pit_latch: u16,
    pit_count: u16,
    /// Port 3DAh's retrace bit alternates so wait loops finish.
    retrace: bool,
    /// Log lines for the host (DOS errors, unknown calls).
    pub log: Vec<String>,
    /// Log every DOS call and error too, for finding what a door trips on.
    pub trace: bool,
}

impl System {
    pub fn new(dos: Dos) -> Self {
        Self {
            uart: Uart::new(),
            dos,
            input: VecDeque::new(),
            output: Vec::new(),
            carrier: true,
            remote: false,
            hangup: false,
            screen: Screen::new(),
            irq_pending: 0,
            pic_mask: 0xB8,
            tick_pending: 0,
            blocked: false,
            polls: 0,
            exit: None,
            fault: None,
            keys: VecDeque::new(),
            esc: Vec::new(),
            pit_latch: 0,
            pit_count: 0,
            retrace: false,
            log: Vec::new(),
            trace: false,
        }
    }

    /// Raise a hardware interrupt line.
    pub fn raise_irq(&mut self, irq: u8) {
        self.irq_pending |= 1 << irq;
    }

    /// A handler that needs input it hasn't got: make the interrupt
    /// re-run. The return frame on the stack points after the `INT`;
    /// move it back onto it.
    pub fn block(&mut self, cpu: &mut Cpu, mem: &mut Memory) {
        let sp = cpu.sp() as u32;
        let ip = mem.read16(linear(cpu.ss(), sp));
        mem.write16(linear(cpu.ss(), sp), ip.wrapping_sub(2));
        self.blocked = true;
    }

    /// Note a status poll that found nothing (so the run loop can idle).
    pub fn polled(&mut self) {
        self.polls += 1;
    }

    /// Something happened: not idle.
    pub fn active(&mut self) {
        self.polls = 0;
    }

    /// The caller typed, in console mode: turn bytes into BIOS keys.
    pub fn console_keys(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if !self.esc.is_empty() {
                self.esc.push(b);
                let key = match self.esc.as_slice() {
                    [0x1B, b'['] | [0x1B, b'O'] => continue,
                    [0x1B, b'[' | b'O', b'A'] => Some(0x4800),
                    [0x1B, b'[' | b'O', b'B'] => Some(0x5000),
                    [0x1B, b'[' | b'O', b'C'] => Some(0x4D00),
                    [0x1B, b'[' | b'O', b'D'] => Some(0x4B00),
                    [0x1B, b'[' | b'O', b'H'] => Some(0x4700),
                    [0x1B, b'[' | b'O', b'F'] => Some(0x4F00),
                    [0x1B, b'[', b'1'..=b'6'] => continue,
                    [0x1B, b'[', b'1', b'~'] => Some(0x4700),
                    [0x1B, b'[', b'2', b'~'] => Some(0x5200),
                    [0x1B, b'[', b'3', b'~'] => Some(0x5300),
                    [0x1B, b'[', b'4', b'~'] => Some(0x4F00),
                    [0x1B, b'[', b'5', b'~'] => Some(0x4900),
                    [0x1B, b'[', b'6', b'~'] => Some(0x5100),
                    _ => None,
                };
                self.esc.clear();
                if let Some(k) = key {
                    self.keys.push_back(k);
                }
                continue;
            }
            match b {
                0x1B => self.esc.push(b),
                b'\r' => self.keys.push_back(0x1C0D),
                b'\n' => {}
                0x7F | 0x08 => self.keys.push_back(0x0E08),
                b'\t' => self.keys.push_back(0x0F09),
                _ => self.keys.push_back(b as u16),
            }
        }
    }

    /// An escape alone, once nothing followed it.
    pub fn flush_escape(&mut self) {
        if self.esc == [0x1B] {
            self.esc.clear();
            self.keys.push_back(0x011B);
        }
    }

    /// Bytes from the caller.
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.active();
        if self.remote {
            self.input.extend(bytes);
            self.uart.data_arrived();
            if self.uart.irq() {
                self.raise_irq(COM1_IRQ);
            }
        } else {
            self.console_keys(bytes);
        }
    }

    /// Serial output for the caller, and the mirrored screen before the
    /// door goes remote.
    pub fn take_output(&mut self, mem: &Memory) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.output);
        if !self.remote {
            out.extend(self.screen.render(mem));
        }
        out
    }

    /// First use of the serial side: the caller is on the line from now.
    pub fn go_remote(&mut self) {
        if !self.remote {
            self.remote = true;
            // What was typed before goes to the port after all.
            let keys: Vec<u16> = self.keys.drain(..).collect();
            for k in keys {
                if k & 0xFF != 0 {
                    self.input.push_back(k as u8);
                }
            }
        }
    }

    fn pit_read(&mut self, instructions: u64) -> u8 {
        // A free-running 16-bit countdown at 1.19 MHz, from the
        // instruction count: about 2.5 instructions per PIT cycle at our
        // speed.
        let count = (0u64.wrapping_sub(instructions * 1_193_182 / (TICK_INSTRUCTIONS * 182 / 10))) as u16;
        let v = if self.pit_latch == 0 { count } else { self.pit_latch };
        self.pit_count = self.pit_count.wrapping_add(1);
        if self.pit_count & 1 == 1 { v as u8 } else { (v >> 8) as u8 }
    }
}

pub struct Machine {
    pub cpu: Cpu,
    pub mem: Memory,
    pub sys: System,
    next_tick: u64,
    /// Timer ticks delivered so far: the machine's clock.
    pub ticks: u64,
}

impl Machine {
    /// A machine with DOS set up: `c_root` is `C:\`, `d_root` (if any)
    /// `D:\`.
    pub fn new(c_root: &Path, d_root: Option<&Path>) -> Self {
        let mut mem = Memory::new();
        let mut cpu = Cpu::new();
        // Every vector points at a native handler.
        for n in 0..=255u32 {
            mem.write16(n * 4, callback_offset(n as u8));
            mem.write16(n * 4 + 2, CALLBACK_SEG);
        }
        // Where the FOSSIL signature and ID string live.
        crate::fossil::install(&mut mem);
        Screen::init(&mut mem);
        mem.write16(BDA + 0x10, 0x0021); // equipment: 80x25 colour, 1 serial port
        mem.write16(BDA + 0x13, 640);
        mem.write16(BDA, COM1_BASE);
        mem.write16(BDA_KBD_HEAD, 0x1E);
        mem.write16(BDA_KBD_TAIL, 0x1E);
        mem.write16(BDA_KBD_START, 0x1E);
        mem.write16(BDA_KBD_END, 0x3E);
        // The BIOS counts ticks from midnight, and programs read the time
        // of day from it (LORD's "arrived at").
        let secs = chrono::Timelike::num_seconds_from_midnight(&chrono::Local::now().time()) as u64;
        mem.write32(BDA_TICKS, (secs * 1_193_182 / 65_536) as u32);
        let dos = Dos::new(&mut mem, c_root, d_root);
        cpu.flags |= cpu::IF;
        Self { cpu, mem, sys: System::new(dos), next_tick: TICK_INSTRUCTIONS, ticks: 0 }
    }

    /// Load and start a program: `program` is a DOS path (or just a name
    /// found in the current directory), `args` the command tail.
    pub fn start(&mut self, program: &str, args: &str) -> Result<(), String> {
        self.sys.exit = None;
        self.sys.fault = None;
        self.sys.blocked = false;
        self.sys.hangup = false;
        self.sys.dos.exec(&mut self.cpu, &mut self.mem, program, args, None)
    }

    /// Hardware interrupts between instructions.
    fn deliver_irq(&mut self) -> bool {
        if !self.cpu.flag(cpu::IF) {
            return false;
        }
        let ready = self.sys.irq_pending & !self.sys.pic_mask;
        if ready == 0 {
            return false;
        }
        let irq = ready.trailing_zeros() as u8;
        if irq == 0 {
            self.sys.tick_pending = self.sys.tick_pending.saturating_sub(1);
            if self.sys.tick_pending == 0 {
                self.sys.irq_pending &= !1;
            }
        } else {
            self.sys.irq_pending &= !(1 << irq);
        }
        self.cpu.interrupt(&mut self.mem, 8 + irq);
        true
    }

    /// Timer ticks while the program waits for input.
    pub fn advance_ticks(&mut self, ticks: u32) {
        if ticks > 0 {
            self.ticks += ticks as u64;
            self.sys.tick_pending += ticks;
            self.sys.raise_irq(0);
        }
    }

    /// Run up to `budget` instructions.
    pub fn run(&mut self, budget: u64) -> Status {
        if let Some(code) = self.sys.exit {
            return Status::Exited(code);
        }
        if let Some(f) = &self.sys.fault {
            return Status::Fault(f.clone());
        }
        let end = self.cpu.instructions + budget;
        self.sys.flush_escape();
        while self.cpu.instructions < end {
            if self.sys.irq_pending != 0 {
                self.deliver_irq();
            }
            if self.cpu.halted {
                return Status::Idle;
            }
            if let Err(e) = self.cpu.step(&mut self.mem, &mut self.sys) {
                let msg = format!("{e:?}");
                self.sys.fault = Some(msg.clone());
                return Status::Fault(msg);
            }
            if let Some(code) = self.sys.exit {
                return Status::Exited(code);
            }
            if let Some(f) = &self.sys.fault {
                return Status::Fault(f.clone());
            }
            if self.sys.blocked {
                self.sys.blocked = false;
                return Status::Idle;
            }
            if self.cpu.instructions >= self.next_tick {
                self.next_tick += TICK_INSTRUCTIONS;
                self.advance_ticks(1);
            }
            if self.sys.polls > 20_000 {
                // Spinning on a status check: let the host wait instead.
                self.sys.polls = 0;
                return Status::Idle;
            }
        }
        Status::Running
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.sys.feed(bytes);
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        self.sys.take_output(&self.mem)
    }

    pub fn set_carrier(&mut self, on: bool) {
        self.sys.carrier = on;
    }
}

impl Bus for System {
    fn io_read(&mut self, port: u16, size: Size) -> u32 {
        let v: u32 = match port {
            0x20 => 0,
            0x21 => self.pic_mask as u32,
            0x40..=0x42 => {
                self.polled();
                self.pit_read(0) as u32
            }
            0x60 => self.keys.front().map_or(0, |k| (k >> 8) as u32),
            0x61 => 0x30,
            0x64 => 0x1C | if self.keys.is_empty() { 0 } else { 1 },
            0x3D4 => 0,
            0x3D5 => 0,
            0x3DA => {
                self.retrace = !self.retrace;
                self.polled();
                if self.retrace { 0x09 } else { 0x00 }
            }
            p if (COM1_BASE..COM1_BASE + 8).contains(&p) => {
                self.go_remote();
                let v = self.uart.read(p, &mut self.input, self.carrier);
                if p == COM1_BASE + 5 || p == COM1_BASE + 6 {
                    self.polled();
                } else {
                    self.active();
                }
                v as u32
            }
            _ => size.mask(),
        };
        v & size.mask()
    }

    fn io_write(&mut self, port: u16, _size: Size, value: u32) {
        let v = value as u8;
        match port {
            0x20 => {}
            0x21 => self.pic_mask = v,
            0x43 => {
                if v & 0x30 == 0 {
                    self.pit_latch = 1;
                } else {
                    self.pit_latch = 0;
                }
            }
            0x3D4 => self.pit_count = (self.pit_count & 0xFF00) | v as u16,
            p if (COM1_BASE..COM1_BASE + 8).contains(&p) => {
                self.go_remote();
                self.active();
                self.uart.write(p, v, &mut self.output);
                if self.uart.irq() {
                    self.raise_irq(COM1_IRQ);
                }
            }
            _ => {}
        }
    }

    fn native_interrupt(&mut self, n: u8, cpu: &mut Cpu, mem: &mut Memory) -> After {
        if self.trace && n != 0x21 && !(n == 0x14 && cpu.ah() == 3) && n != 0x2F && n != 0x28 && n != 0x08 && n != 0x1C {
            let before = format!("int {n:02X} ax={:04X} bx={:04X} cx={:04X} dx={:04X}", cpu.ax(), cpu.r16(cpu::BX), cpu.cx(), cpu.dx());
            let after = self.native_interrupt_inner(n, cpu, mem);
            self.log.push(format!("{before} -> ax={:04X} bx={:04X} cx={:04X} dx={:04X} es={:04X} cf={}", cpu.ax(), cpu.r16(cpu::BX), cpu.cx(), cpu.dx(), cpu.es(), mem.read16(linear(cpu.ss(), cpu.r16(cpu::SP) as u32 + 4)) & 1));
            return after;
        }
        self.native_interrupt_inner(n, cpu, mem)
    }
}

impl System {
    fn native_interrupt_inner(&mut self, n: u8, cpu: &mut Cpu, mem: &mut Memory) -> After {
        match n {
            0x08 => {
                // Timer: count the tick, then chain to INT 1Ch.
                let t = mem.read32(BDA_TICKS).wrapping_add(1);
                if t >= 0x1800B0 {
                    mem.write32(BDA_TICKS, 0);
                    mem.write8(BDA_MIDNIGHT, 1);
                } else {
                    mem.write32(BDA_TICKS, t);
                }
                cpu.ip = IRET_STUB;
                cpu.interrupt(mem, 0x1C);
                After::Redirected
            }
            0x0C => {
                // Serial IRQ with no handler of the program's own: nothing.
                After::Iret
            }
            0x10 => {
                Screen::int10(cpu, mem);
                After::Iret
            }
            0x11 => {
                cpu.set_ax(mem.read16(BDA + 0x10));
                After::Iret
            }
            0x12 => {
                cpu.set_ax(640);
                After::Iret
            }
            0x14 => crate::fossil::int14(self, cpu, mem),
            0x15 => {
                crate::bios::int15(self, cpu, mem);
                After::Iret
            }
            0x16 => crate::bios::int16(self, cpu, mem),
            0x1A => {
                crate::bios::int1a(cpu, mem);
                After::Iret
            }
            0x1C | 0x1B | 0x23 | 0x28 | 0x2A => After::Iret,
            0x20 => {
                cpu.do_iret(mem);
                if let Some(c) = self.dos.terminate(0, cpu, mem) {
                    self.exit = Some(c);
                }
                After::Redirected
            }
            0x21 => crate::dos::int21(self, cpu, mem),
            0x22 => {
                // The program returned through its termination address.
                cpu.do_iret(mem);
                if let Some(c) = self.dos.terminate(0, cpu, mem) {
                    self.exit = Some(c);
                }
                After::Redirected
            }
            0x24 => {
                // Critical error: fail the call.
                cpu.set_al(3);
                After::Iret
            }
            0x29 => {
                Screen::teletype(mem, cpu.al(), 0x07);
                After::Iret
            }
            0x2F => {
                crate::bios::int2f(cpu);
                After::Iret
            }
            0x33 => {
                // No mouse.
                if cpu.ax() == 0 {
                    cpu.set_ax(0);
                }
                After::Iret
            }
            0x00 => {
                self.fault = Some("divide error".into());
                After::Iret
            }
            0x06 => {
                self.fault = Some(format!("invalid opcode at {:04X}:{:04X}", cpu.cs(), cpu.ip));
                After::Iret
            }
            _ => After::Iret,
        }
    }
}
