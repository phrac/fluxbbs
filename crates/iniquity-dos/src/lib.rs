//! A real-mode x86 and DOS emulator, enough to run DOS door programs
//! (LORD, TradeWars 2002, BRE, Usurper...) inside the BBS without an
//! outside emulator. Real mode only: 8086 through the 386's real-mode
//! extensions, no protected mode or DPMI.
//!
//! [`cpu`] is the processor; [`Memory`] the 1 MB (plus HMA) address
//! space; a [`Bus`] supplies I/O ports and takes the interrupts the
//! machine handles natively (DOS, BIOS, FOSSIL).

pub mod batch;
pub mod bios;

/// The clock DOS and the BIOS answer with: a function giving a UTC time
/// as the local time programs should see. The host's zone unless the
/// embedding board sets its own with [`set_clock`].
static CLOCK: std::sync::RwLock<fn(chrono::NaiveDateTime) -> chrono::NaiveDateTime> = std::sync::RwLock::new(host_local);

fn host_local(utc: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    chrono::TimeZone::from_utc_datetime(&chrono::Local, &utc).naive_local()
}

/// Keep time with `f` from now on (a UTC time to the local time to show).
pub fn set_clock(f: fn(chrono::NaiveDateTime) -> chrono::NaiveDateTime) {
    *CLOCK.write().unwrap_or_else(|e| e.into_inner()) = f;
}

/// A UTC time as the machine's local time.
pub fn local(utc: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    (CLOCK.read().unwrap_or_else(|e| e.into_inner()))(utc)
}

/// Now, as the machine's local time.
pub fn now() -> chrono::NaiveDateTime {
    local(chrono::Utc::now().naive_utc())
}
pub mod cpu;
pub mod dos;
pub mod fossil;
pub mod machine;
pub mod serial;
pub mod share;
pub mod video;

/// Linear address space: 1 MB plus the 64 KB the A20 line reaches.
pub struct Memory {
    bytes: Vec<u8>,
    /// Something was written to the text-mode video buffer.
    pub video_dirty: bool,
    /// The A20 line: off, addresses wrap at 1 MB as on an 8088; on, the
    /// 64 KB above it (the HMA) can be reached.
    pub a20: bool,
    /// For `Cpu::ring`: every byte written since the last instruction
    /// began, as (address, old value, new value).
    pub writes: Option<Vec<(u32, u8, u8)>>,
}

pub const MEMORY_SIZE: usize = 0x110000;
pub const VIDEO_BASE: u32 = 0xB8000;
pub const VIDEO_END: u32 = 0xC0000;

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Self { bytes: vec![0; MEMORY_SIZE], video_dirty: false, a20: false, writes: None }
    }

    #[inline]
    fn at(&self, addr: u32) -> usize {
        if self.a20 { (addr as usize) % MEMORY_SIZE } else { (addr & 0xFFFFF) as usize }
    }

    #[inline]
    pub fn read8(&self, addr: u32) -> u8 {
        self.bytes[self.at(addr)]
    }

    #[inline]
    pub fn read16(&self, addr: u32) -> u16 {
        self.read8(addr) as u16 | (self.read8(addr.wrapping_add(1)) as u16) << 8
    }

    #[inline]
    pub fn read32(&self, addr: u32) -> u32 {
        self.read16(addr) as u32 | (self.read16(addr.wrapping_add(2)) as u32) << 16
    }

    #[inline]
    pub fn write8(&mut self, addr: u32, v: u8) {
        let i = self.at(addr);
        if let Some(w) = &mut self.writes {
            w.push((i as u32, self.bytes[i], v));
        }
        self.bytes[i] = v;
        if (VIDEO_BASE..VIDEO_END).contains(&(i as u32)) {
            self.video_dirty = true;
        }
    }

    #[inline]
    pub fn write16(&mut self, addr: u32, v: u16) {
        self.write8(addr, v as u8);
        self.write8(addr.wrapping_add(1), (v >> 8) as u8);
    }

    #[inline]
    pub fn write32(&mut self, addr: u32, v: u32) {
        self.write16(addr, v as u16);
        self.write16(addr.wrapping_add(2), (v >> 16) as u16);
    }

    pub fn read(&self, addr: u32, len: usize) -> &[u8] {
        let a = self.at(addr);
        &self.bytes[a..(a + len).min(MEMORY_SIZE)]
    }

    pub fn write(&mut self, addr: u32, data: &[u8]) {
        for (i, &b) in data.iter().enumerate() {
            self.write8(addr.wrapping_add(i as u32), b);
        }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

/// `segment:offset` as a linear address.
#[inline]
pub fn linear(seg: u16, off: u32) -> u32 {
    ((seg as u32) << 4).wrapping_add(off & 0xFFFF)
}

/// What a native interrupt handler wants done afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Return to the interrupted code (`IRET`).
    Iret,
    /// The handler set CS:IP itself (chained to another interrupt, say).
    Redirected,
}

/// The world outside the CPU: I/O ports and native interrupt handlers.
pub trait Bus {
    fn io_read(&mut self, port: u16, size: cpu::Size) -> u32;
    fn io_write(&mut self, port: u16, size: cpu::Size, value: u32);
    /// Interrupt `n` reached a native handler (its vector points into
    /// the callback ROM).
    fn native_interrupt(&mut self, n: u8, cpu: &mut cpu::Cpu, mem: &mut Memory) -> After;
}

/// A bus with no devices: ports read as 0xFF, interrupts just return.
pub struct NullBus;

impl Bus for NullBus {
    fn io_read(&mut self, _port: u16, size: cpu::Size) -> u32 {
        size.mask()
    }
    fn io_write(&mut self, _port: u16, _size: cpu::Size, _value: u32) {}
    fn native_interrupt(&mut self, _n: u8, _cpu: &mut cpu::Cpu, _mem: &mut Memory) -> After {
        After::Iret
    }
}
