//! The processor: an interpreter for real-mode x86, the 8086 base set
//! through the 186 additions and what 386 code uses in real mode
//! (32-bit registers and operands, the 0F instructions). Protected
//! mode, paging and the FPU aren't here; FPU instructions are skipped,
//! which suits Turbo Pascal doors (software reals) but not everything.

use crate::{After, Bus, Memory, linear};

/// Operand size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    B,
    W,
    D,
}

impl Size {
    #[inline]
    pub fn mask(self) -> u32 {
        match self {
            Size::B => 0xFF,
            Size::W => 0xFFFF,
            Size::D => 0xFFFF_FFFF,
        }
    }
    #[inline]
    pub fn bits(self) -> u32 {
        match self {
            Size::B => 8,
            Size::W => 16,
            Size::D => 32,
        }
    }
    #[inline]
    pub fn sign(self) -> u32 {
        1 << (self.bits() - 1)
    }
    #[inline]
    pub fn bytes(self) -> u32 {
        self.bits() / 8
    }
    #[inline]
    fn wd(opsize32: bool) -> Size {
        if opsize32 { Size::D } else { Size::W }
    }
}

pub const AX: usize = 0;
pub const CX: usize = 1;
pub const DX: usize = 2;
pub const BX: usize = 3;
pub const SP: usize = 4;
pub const BP: usize = 5;
pub const SI: usize = 6;
pub const DI: usize = 7;

pub const ES: usize = 0;
pub const CS: usize = 1;
pub const SS: usize = 2;
pub const DS: usize = 3;
pub const FS: usize = 4;
pub const GS: usize = 5;

pub const CF: u32 = 1 << 0;
pub const PF: u32 = 1 << 2;
pub const AF: u32 = 1 << 4;
pub const ZF: u32 = 1 << 6;
pub const SF: u32 = 1 << 7;
pub const TF: u32 = 1 << 8;
pub const IF: u32 = 1 << 9;
pub const DF: u32 = 1 << 10;
pub const OF: u32 = 1 << 11;
/// Bits that are always set in a real-mode FLAGS image.
const FLAGS_FIXED: u32 = 0x0002;
const FLAGS_WRITABLE: u32 = CF | PF | AF | ZF | SF | TF | IF | DF | OF;

/// Where native interrupt handlers live: an interrupt vector pointing
/// at `CALLBACK_SEG:(n*4)` means the bus handles interrupt `n`.
pub const CALLBACK_SEG: u16 = 0xF000;
/// A bare `IRET` for handlers that chain to another interrupt.
pub const IRET_STUB: u16 = 0x0FF0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuError {
    /// An opcode this emulator doesn't do, at the given CS:IP.
    Unsupported { cs: u16, ip: u16, opcode: u8, second: Option<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rep {
    None,
    Z,
    Nz,
}

#[derive(Debug, Clone, Copy)]
struct Prefix {
    seg: Option<usize>,
    rep: Rep,
    op32: bool,
    ad32: bool,
}

#[derive(Debug, Clone, Copy)]
enum Operand {
    Reg(u8),
    /// A memory operand: segment value and offset (bytes of a word or
    /// dword wrap inside the 64 KB segment, as on the real chip).
    Mem(u16, u32),
}

#[derive(Debug, Clone)]
pub struct Cpu {
    /// EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI.
    pub regs: [u32; 8],
    /// ES, CS, SS, DS, FS, GS.
    pub sregs: [u16; 6],
    pub ip: u16,
    pub flags: u32,
    pub halted: bool,
    pub instructions: u64,
    /// Mask shift counts to 5 bits (186 and later; the 8086 doesn't).
    pub shift_mask: bool,
    /// Behave as an 8086/8088 where the later chips differ (for the
    /// conformance tests): unmasked shifts, PUSH SP after the decrement,
    /// 60-6F and C0/C1/C8/C9 as aliases, POP CS, divide faults pointing
    /// past the instruction.
    pub model_8086: bool,
    /// The last instruction waited for input (an `IN` or `HLT`), so the
    /// machine may idle instead of spinning.
    pub idle_hint: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

/// Even parity of the low byte.
#[inline]
fn parity(v: u32) -> bool {
    (v as u8).count_ones().is_multiple_of(2)
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            regs: [0; 8],
            sregs: [0; 6],
            ip: 0,
            flags: FLAGS_FIXED,
            halted: false,
            instructions: 0,
            shift_mask: true,
            model_8086: false,
            idle_hint: false,
        }
    }

    // ---- registers ----------------------------------------------------------

    #[inline]
    pub fn r16(&self, r: usize) -> u16 {
        self.regs[r] as u16
    }
    #[inline]
    pub fn set_r16(&mut self, r: usize, v: u16) {
        self.regs[r] = (self.regs[r] & 0xFFFF_0000) | v as u32;
    }
    #[inline]
    pub fn r8(&self, r: usize) -> u8 {
        if r < 4 { self.regs[r] as u8 } else { (self.regs[r - 4] >> 8) as u8 }
    }
    #[inline]
    pub fn set_r8(&mut self, r: usize, v: u8) {
        if r < 4 {
            self.regs[r] = (self.regs[r] & 0xFFFF_FF00) | v as u32;
        } else {
            self.regs[r - 4] = (self.regs[r - 4] & 0xFFFF_00FF) | (v as u32) << 8;
        }
    }
    pub fn al(&self) -> u8 {
        self.r8(0)
    }
    pub fn ah(&self) -> u8 {
        self.r8(4)
    }
    pub fn ax(&self) -> u16 {
        self.r16(AX)
    }
    pub fn set_al(&mut self, v: u8) {
        self.set_r8(0, v)
    }
    pub fn set_ah(&mut self, v: u8) {
        self.set_r8(4, v)
    }
    pub fn set_ax(&mut self, v: u16) {
        self.set_r16(AX, v)
    }
    pub fn bx(&self) -> u16 {
        self.r16(BX)
    }
    pub fn cx(&self) -> u16 {
        self.r16(CX)
    }
    pub fn dx(&self) -> u16 {
        self.r16(DX)
    }
    pub fn si(&self) -> u16 {
        self.r16(SI)
    }
    pub fn di(&self) -> u16 {
        self.r16(DI)
    }
    pub fn bp(&self) -> u16 {
        self.r16(BP)
    }
    pub fn sp(&self) -> u16 {
        self.r16(SP)
    }
    pub fn cs(&self) -> u16 {
        self.sregs[CS]
    }
    pub fn ds(&self) -> u16 {
        self.sregs[DS]
    }
    pub fn es(&self) -> u16 {
        self.sregs[ES]
    }
    pub fn ss(&self) -> u16 {
        self.sregs[SS]
    }

    #[inline]
    fn reg(&self, r: u8, size: Size) -> u32 {
        match size {
            Size::B => self.r8(r as usize) as u32,
            Size::W => self.r16(r as usize) as u32,
            Size::D => self.regs[r as usize],
        }
    }
    #[inline]
    fn set_reg(&mut self, r: u8, size: Size, v: u32) {
        match size {
            Size::B => self.set_r8(r as usize, v as u8),
            Size::W => self.set_r16(r as usize, v as u16),
            Size::D => self.regs[r as usize] = v,
        }
    }

    #[inline]
    pub fn flag(&self, f: u32) -> bool {
        self.flags & f != 0
    }
    #[inline]
    pub fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }

    /// Set CS:IP (a far jump).
    pub fn jump_far(&mut self, seg: u16, off: u16) {
        self.sregs[CS] = seg;
        self.ip = off;
    }

    // ---- memory access ------------------------------------------------------

    /// Read `size` at seg:off, each byte's offset wrapping at 64 KB.
    #[inline]
    fn rd(&self, mem: &Memory, seg: u16, off: u32, size: Size) -> u32 {
        let mut v = 0u32;
        for i in 0..size.bytes() {
            v |= (mem.read8(linear(seg, off.wrapping_add(i))) as u32) << (8 * i);
        }
        v
    }
    #[inline]
    fn wr(&self, mem: &mut Memory, seg: u16, off: u32, size: Size, v: u32) {
        for i in 0..size.bytes() {
            mem.write8(linear(seg, off.wrapping_add(i)), (v >> (8 * i)) as u8);
        }
    }

    #[inline]
    fn fetch8(&mut self, mem: &Memory) -> u8 {
        let b = mem.read8(linear(self.sregs[CS], self.ip as u32));
        self.ip = self.ip.wrapping_add(1);
        b
    }
    #[inline]
    fn fetch16(&mut self, mem: &Memory) -> u16 {
        let lo = self.fetch8(mem) as u16;
        lo | (self.fetch8(mem) as u16) << 8
    }
    #[inline]
    fn fetch32(&mut self, mem: &Memory) -> u32 {
        let lo = self.fetch16(mem) as u32;
        lo | (self.fetch16(mem) as u32) << 16
    }
    #[inline]
    fn fetch(&mut self, mem: &Memory, size: Size) -> u32 {
        match size {
            Size::B => self.fetch8(mem) as u32,
            Size::W => self.fetch16(mem) as u32,
            Size::D => self.fetch32(mem),
        }
    }
    /// A sign-extended immediate of `size`, as a 32-bit value.
    #[inline]
    fn fetch_signed(&mut self, mem: &Memory, size: Size) -> u32 {
        match size {
            Size::B => self.fetch8(mem) as i8 as i32 as u32,
            Size::W => self.fetch16(mem) as i16 as i32 as u32,
            Size::D => self.fetch32(mem),
        }
    }

    // ---- stack --------------------------------------------------------------

    pub fn push(&mut self, mem: &mut Memory, size: Size, v: u32) {
        let sp = self.r16(SP).wrapping_sub(size.bytes() as u16);
        self.set_r16(SP, sp);
        self.wr(mem, self.sregs[SS], sp as u32, size, v);
    }

    pub fn pop(&mut self, mem: &Memory, size: Size) -> u32 {
        let sp = self.r16(SP);
        let v = self.rd(mem, self.sregs[SS], sp as u32, size);
        self.set_r16(SP, sp.wrapping_add(size.bytes() as u16));
        v
    }

    pub fn push16(&mut self, mem: &mut Memory, v: u16) {
        self.push(mem, Size::W, v as u32)
    }
    pub fn pop16(&mut self, mem: &Memory) -> u16 {
        self.pop(mem, Size::W) as u16
    }

    /// Take interrupt `n`: push FLAGS, CS, IP, clear IF and TF, vector.
    pub fn interrupt(&mut self, mem: &mut Memory, n: u8) {
        self.push16(mem, self.flags as u16);
        self.push16(mem, self.sregs[CS]);
        self.push16(mem, self.ip);
        self.flags &= !(IF | TF);
        let v = n as u32 * 4;
        self.ip = mem.read16(v);
        self.sregs[CS] = mem.read16(v + 2);
        self.halted = false;
    }

    fn iret(&mut self, mem: &Memory, size: Size) {
        let ip = self.pop(mem, size);
        let cs = self.pop(mem, size);
        let fl = self.pop(mem, size);
        self.ip = ip as u16;
        self.sregs[CS] = cs as u16;
        self.load_flags(fl, size);
    }

    fn load_flags(&mut self, v: u32, size: Size) {
        let keep = if size == Size::W { 0xFFFF } else { 0x003F_FFFF };
        self.flags = ((self.flags & !(FLAGS_WRITABLE & keep)) | (v & FLAGS_WRITABLE & keep) | FLAGS_FIXED) & 0x003F_FFFF;
        self.flags &= !(1 << 14 | 1 << 17); // no NT, no VM in real mode
        if self.model_8086 {
            // The 8086's top four flag bits always read as 1.
            self.flags |= 0xF000;
        }
    }

    // ---- ModRM --------------------------------------------------------------

    fn modrm(&mut self, mem: &Memory, p: &Prefix) -> (u8, u8, Operand) {
        let b = self.fetch8(mem);
        let md = b >> 6;
        let reg = (b >> 3) & 7;
        let rm = b & 7;
        if md == 3 {
            return (md, reg, Operand::Reg(rm));
        }
        let (mut off, mut seg): (u32, usize);
        if p.ad32 {
            seg = DS;
            if rm == 4 {
                let sib = self.fetch8(mem);
                let scale = sib >> 6;
                let index = (sib >> 3) & 7;
                let base = sib & 7;
                off = if index == 4 { 0 } else { self.regs[index as usize] << scale };
                if base == 5 && md == 0 {
                    off = off.wrapping_add(self.fetch32(mem));
                } else {
                    off = off.wrapping_add(self.regs[base as usize]);
                    if base == 4 || base == 5 {
                        seg = SS;
                    }
                }
            } else if rm == 5 && md == 0 {
                off = self.fetch32(mem);
            } else {
                off = self.regs[rm as usize];
                if rm == 5 {
                    seg = SS;
                }
            }
            match md {
                1 => off = off.wrapping_add(self.fetch8(mem) as i8 as i32 as u32),
                2 => off = off.wrapping_add(self.fetch32(mem)),
                _ => {}
            }
        } else {
            let (base, s) = match rm {
                0 => (self.r16(BX).wrapping_add(self.r16(SI)), DS),
                1 => (self.r16(BX).wrapping_add(self.r16(DI)), DS),
                2 => (self.r16(BP).wrapping_add(self.r16(SI)), SS),
                3 => (self.r16(BP).wrapping_add(self.r16(DI)), SS),
                4 => (self.r16(SI), DS),
                5 => (self.r16(DI), DS),
                6 => {
                    if md == 0 {
                        (0, DS)
                    } else {
                        (self.r16(BP), SS)
                    }
                }
                _ => (self.r16(BX), DS),
            };
            seg = s;
            let mut o = base;
            match md {
                0 if rm == 6 => o = self.fetch16(mem),
                1 => o = o.wrapping_add(self.fetch8(mem) as i8 as i16 as u16),
                2 => o = o.wrapping_add(self.fetch16(mem)),
                _ => {}
            }
            off = o as u32;
        }
        let seg = p.seg.unwrap_or(seg);
        let off = if p.ad32 { off } else { off & 0xFFFF };
        (md, reg, Operand::Mem(self.sregs[seg], off))
    }

    #[inline]
    fn read_op(&self, mem: &Memory, op: Operand, size: Size) -> u32 {
        match op {
            Operand::Reg(r) => self.reg(r, size),
            Operand::Mem(s, o) => self.rd(mem, s, o, size),
        }
    }
    #[inline]
    fn write_op(&mut self, mem: &mut Memory, op: Operand, size: Size, v: u32) {
        match op {
            Operand::Reg(r) => self.set_reg(r, size, v),
            Operand::Mem(s, o) => self.wr(mem, s, o, size, v),
        }
    }

    // ---- arithmetic and flags -----------------------------------------------

    fn szp(&mut self, res: u32, size: Size) {
        let res = res & size.mask();
        self.set_flag(ZF, res == 0);
        self.set_flag(SF, res & size.sign() != 0);
        self.set_flag(PF, parity(res));
    }

    fn add(&mut self, a: u32, b: u32, carry: bool, size: Size) -> u32 {
        let m = size.mask();
        let (a, b) = (a & m, b & m);
        let r = (a as u64 + b as u64 + carry as u64) & 0xFFFF_FFFF_FFFF;
        let res = r as u32 & m;
        self.set_flag(CF, r >> size.bits() != 0);
        self.set_flag(OF, (a ^ res) & (b ^ res) & size.sign() != 0);
        self.set_flag(AF, (a ^ b ^ res) & 0x10 != 0);
        self.szp(res, size);
        res
    }

    fn sub(&mut self, a: u32, b: u32, borrow: bool, size: Size) -> u32 {
        let m = size.mask();
        let (a, b) = (a & m, b & m);
        let res = a.wrapping_sub(b).wrapping_sub(borrow as u32) & m;
        self.set_flag(CF, (a as u64) < b as u64 + borrow as u64);
        self.set_flag(OF, (a ^ b) & (a ^ res) & size.sign() != 0);
        self.set_flag(AF, (a ^ b ^ res) & 0x10 != 0);
        self.szp(res, size);
        res
    }

    fn logic(&mut self, res: u32, size: Size) -> u32 {
        let res = res & size.mask();
        self.set_flag(CF, false);
        self.set_flag(OF, false);
        self.set_flag(AF, false);
        self.szp(res, size);
        res
    }

    /// The eight ALU operations by their /reg or opcode-column number.
    fn alu(&mut self, op: u8, a: u32, b: u32, size: Size) -> Option<u32> {
        let c = self.flag(CF);
        Some(match op {
            0 => self.add(a, b, false, size),
            1 => self.logic(a | b, size),
            2 => self.add(a, b, c, size),
            3 => self.sub(a, b, c, size),
            4 => self.logic(a & b, size),
            5 => self.sub(a, b, false, size),
            6 => self.logic(a ^ b, size),
            _ => {
                self.sub(a, b, false, size);
                return None;
            }
        })
    }

    fn inc(&mut self, a: u32, size: Size) -> u32 {
        let c = self.flag(CF);
        let r = self.add(a, 1, false, size);
        self.set_flag(CF, c);
        r
    }

    fn dec(&mut self, a: u32, size: Size) -> u32 {
        let c = self.flag(CF);
        let r = self.sub(a, 1, false, size);
        self.set_flag(CF, c);
        r
    }

    fn shift(&mut self, op: u8, v: u32, count: u32, size: Size) -> u32 {
        let bits = size.bits();
        let m = size.mask();
        let v = v & m;
        let count = if self.shift_mask { count & 0x1F } else { count & 0xFF };
        if count == 0 {
            return v;
        }
        let msb = |x: u32| x & size.sign() != 0;
        match op {
            // ROL
            0 => {
                let n = count % bits;
                let res = if n == 0 { v } else { ((v << n) | (v >> (bits - n))) & m };
                self.set_flag(CF, res & 1 != 0);
                if count == 1 {
                    self.set_flag(OF, msb(res) ^ self.flag(CF));
                }
                res
            }
            // ROR
            1 => {
                let n = count % bits;
                let res = if n == 0 { v } else { ((v >> n) | (v << (bits - n))) & m };
                self.set_flag(CF, msb(res));
                if count == 1 {
                    self.set_flag(OF, msb(res) ^ (res & (size.sign() >> 1) != 0));
                }
                res
            }
            // RCL
            2 => {
                let n = count % (bits + 1);
                let mut res = v;
                let mut cf = self.flag(CF);
                for _ in 0..n {
                    let new_cf = msb(res);
                    res = ((res << 1) | cf as u32) & m;
                    cf = new_cf;
                }
                self.set_flag(CF, cf);
                if count == 1 {
                    self.set_flag(OF, msb(res) ^ cf);
                }
                res
            }
            // RCR
            3 => {
                let n = count % (bits + 1);
                let mut res = v;
                let mut cf = self.flag(CF);
                if count == 1 {
                    self.set_flag(OF, msb(v) ^ cf);
                }
                for _ in 0..n {
                    let new_cf = res & 1 != 0;
                    res = (res >> 1) | (cf as u32) << (bits - 1);
                    cf = new_cf;
                }
                self.set_flag(CF, cf);
                res
            }
            // SHL / SAL
            4 | 6 => {
                let res = if count >= bits { 0 } else { (v << count) & m };
                let cf = if count > bits { false } else { (v >> (bits - count)) & 1 != 0 };
                self.set_flag(CF, cf);
                if count == 1 {
                    self.set_flag(OF, msb(res) ^ cf);
                }
                self.set_flag(AF, false);
                self.szp(res, size);
                res
            }
            // SHR
            5 => {
                let res = if count >= bits { 0 } else { v >> count };
                let cf = if count > bits { false } else { (v >> (count - 1)) & 1 != 0 };
                self.set_flag(CF, cf);
                if count == 1 {
                    self.set_flag(OF, msb(v));
                }
                self.set_flag(AF, false);
                self.szp(res, size);
                res
            }
            // SAR
            _ => {
                let sv = match size {
                    Size::B => v as u8 as i8 as i32,
                    Size::W => v as u16 as i16 as i32,
                    Size::D => v as i32,
                };
                let n = count.min(bits);
                let res = (if n >= bits { sv >> (bits - 1) } else { sv >> n }) as u32 & m;
                let cf = if n >= bits { sv < 0 } else { (sv >> (n - 1)) & 1 != 0 };
                self.set_flag(CF, cf);
                if count == 1 {
                    self.set_flag(OF, false);
                }
                self.set_flag(AF, false);
                self.szp(res, size);
                res
            }
        }
    }

    // ---- conditions ---------------------------------------------------------

    fn cond(&self, cc: u8) -> bool {
        let f = self.flags;
        let r = match cc >> 1 {
            0 => f & OF != 0,
            1 => f & CF != 0,
            2 => f & ZF != 0,
            3 => f & (CF | ZF) != 0,
            4 => f & SF != 0,
            5 => f & PF != 0,
            6 => (f & SF != 0) ^ (f & OF != 0),
            _ => (f & ZF != 0) || ((f & SF != 0) ^ (f & OF != 0)),
        };
        r ^ (cc & 1 != 0)
    }

    // ---- the run loop -------------------------------------------------------

    /// Run one instruction.
    pub fn step(&mut self, mem: &mut Memory, bus: &mut dyn Bus) -> Result<(), CpuError> {
        self.idle_hint = false;
        // Native interrupt handlers: the vector pointed into the callback ROM.
        if self.sregs[CS] == CALLBACK_SEG {
            if self.ip < 0x400 && self.ip.is_multiple_of(4) {
                let n = (self.ip / 4) as u8;
                self.instructions += 1;
                match bus.native_interrupt(n, self, mem) {
                    After::Iret => self.iret(mem, Size::W),
                    After::Redirected => {}
                }
                return Ok(());
            }
            if self.ip == IRET_STUB {
                self.instructions += 1;
                self.iret(mem, Size::W);
                return Ok(());
            }
        }
        if self.halted {
            self.idle_hint = true;
            return Ok(());
        }
        self.instructions += 1;
        let start_ip = self.ip;
        let mut p = Prefix { seg: None, rep: Rep::None, op32: false, ad32: false };
        let op = loop {
            let b = self.fetch8(mem);
            match b {
                0x26 => p.seg = Some(ES),
                0x2E => p.seg = Some(CS),
                0x36 => p.seg = Some(SS),
                0x3E => p.seg = Some(DS),
                // 386 prefixes; jump aliases on an 8086.
                0x64 if !self.model_8086 => p.seg = Some(FS),
                0x65 if !self.model_8086 => p.seg = Some(GS),
                0x66 if !self.model_8086 => p.op32 = true,
                0x67 if !self.model_8086 => p.ad32 = true,
                0xF0 => {}
                0xF2 => p.rep = Rep::Nz,
                0xF3 => p.rep = Rep::Z,
                _ => break b,
            }
        };
        self.exec(op, &p, start_ip, mem, bus)
    }

    fn unsupported(&self, op: u8, second: Option<u8>, start_ip: u16) -> CpuError {
        CpuError::Unsupported { cs: self.sregs[CS], ip: start_ip, opcode: op, second }
    }

    fn exec(&mut self, op: u8, p: &Prefix, start_ip: u16, mem: &mut Memory, bus: &mut dyn Bus) -> Result<(), CpuError> {
        let wd = Size::wd(p.op32);
        let op = if self.model_8086 {
            match op {
                0x60..=0x6F => op + 0x10,
                0xC0 | 0xC1 => op + 2,
                0xC8 | 0xC9 => op + 2,
                _ => op,
            }
        } else {
            op
        };
        if self.model_8086 && op == 0x0F {
            self.sregs[CS] = self.pop(mem, Size::W) as u16;
            return Ok(());
        }
        match op {
            // ---- ALU, the regular rows
            0x00..=0x3F if op & 7 < 6 => {
                let alu_op = op >> 3;
                match op & 7 {
                    0 | 1 => {
                        let size = if op & 1 == 0 { Size::B } else { wd };
                        let (_, reg, rm) = self.modrm(mem, p);
                        let a = self.read_op(mem, rm, size);
                        let b = self.reg(reg, size);
                        if let Some(r) = self.alu(alu_op, a, b, size) {
                            self.write_op(mem, rm, size, r);
                        }
                    }
                    2 | 3 => {
                        let size = if op & 1 == 0 { Size::B } else { wd };
                        let (_, reg, rm) = self.modrm(mem, p);
                        let a = self.reg(reg, size);
                        let b = self.read_op(mem, rm, size);
                        if let Some(r) = self.alu(alu_op, a, b, size) {
                            self.set_reg(reg, size, r);
                        }
                    }
                    4 => {
                        let b = self.fetch8(mem) as u32;
                        let a = self.al() as u32;
                        if let Some(r) = self.alu(alu_op, a, b, Size::B) {
                            self.set_al(r as u8);
                        }
                    }
                    _ => {
                        let b = self.fetch(mem, wd);
                        let a = self.reg(0, wd);
                        if let Some(r) = self.alu(alu_op, a, b, wd) {
                            self.set_reg(0, wd, r);
                        }
                    }
                }
            }
            // ---- segment pushes/pops and BCD in the ALU rows' columns 6 and 7
            0x06 => self.push(mem, wd, self.sregs[ES] as u32),
            0x07 => self.sregs[ES] = self.pop(mem, wd) as u16,
            0x0E => self.push(mem, wd, self.sregs[CS] as u32),
            0x0F => return self.exec_0f(p, start_ip, mem, bus),
            0x16 => self.push(mem, wd, self.sregs[SS] as u32),
            0x17 => self.sregs[SS] = self.pop(mem, wd) as u16,
            0x1E => self.push(mem, wd, self.sregs[DS] as u32),
            0x1F => self.sregs[DS] = self.pop(mem, wd) as u16,
            0x27 => self.daa(),
            0x2F => self.das(),
            0x37 => self.aaa(),
            0x3F => self.aas(),
            // ---- INC/DEC/PUSH/POP reg
            0x40..=0x47 => {
                let r = op & 7;
                let v = self.inc(self.reg(r, wd), wd);
                self.set_reg(r, wd, v);
            }
            0x48..=0x4F => {
                let r = op & 7;
                let v = self.dec(self.reg(r, wd), wd);
                self.set_reg(r, wd, v);
            }
            0x50..=0x57 => {
                let r = op & 7;
                // PUSH SP pushes the value before the decrement (186+); the
                // 8086 pushed the decremented one.
                let v = if self.model_8086 && r == 4 { self.reg(r, wd).wrapping_sub(2) } else { self.reg(r, wd) };
                self.push(mem, wd, v);
            }
            0x58..=0x5F => {
                let r = op & 7;
                let v = self.pop(mem, wd);
                self.set_reg(r, wd, v);
            }
            // ---- 186: PUSHA, POPA, BOUND, PUSH imm, IMUL imm, INS, OUTS
            0x60 => {
                let sp = self.reg(SP as u8, wd);
                for r in 0..8u8 {
                    let v = if r == 4 { sp } else { self.reg(r, wd) };
                    self.push(mem, wd, v);
                }
            }
            0x61 => {
                for r in (0..8u8).rev() {
                    let v = self.pop(mem, wd);
                    if r != 4 {
                        self.set_reg(r, wd, v);
                    }
                }
            }
            0x62 => {
                let (_, reg, rm) = self.modrm(mem, p);
                if let Operand::Mem(s, o) = rm {
                    let idx = self.reg(reg, wd) as i32;
                    let (lo, hi) = if wd == Size::W {
                        (self.rd(mem, s, o, Size::W) as i16 as i32, self.rd(mem, s, o + 2, Size::W) as i16 as i32)
                    } else {
                        (self.rd(mem, s, o, Size::D) as i32, self.rd(mem, s, o + 4, Size::D) as i32)
                    };
                    let idx = if wd == Size::W { idx as i16 as i32 } else { idx };
                    if idx < lo || idx > hi {
                        self.ip = start_ip;
                        self.interrupt(mem, 5);
                    }
                }
            }
            0x68 => {
                let v = self.fetch(mem, wd);
                self.push(mem, wd, v);
            }
            0x69 | 0x6B => {
                let (_, reg, rm) = self.modrm(mem, p);
                let a = self.read_op(mem, rm, wd);
                let b = if op == 0x69 { self.fetch(mem, wd) } else { self.fetch_signed(mem, Size::B) };
                let r = self.imul2(a, b, wd);
                self.set_reg(reg, wd, r);
            }
            0x6A => {
                let v = self.fetch_signed(mem, Size::B);
                self.push(mem, wd, v);
            }
            0x6C..=0x6F => self.string_op(op, p, mem, bus),
            // ---- Jcc short
            0x70..=0x7F => {
                let d = self.fetch8(mem) as i8 as i16 as u16;
                if self.cond(op & 0xF) {
                    self.ip = self.ip.wrapping_add(d);
                }
            }
            // ---- ALU with immediate
            0x80..=0x83 => {
                let size = if op & 1 == 0 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let a = self.read_op(mem, rm, size);
                let b = match op {
                    0x81 => self.fetch(mem, wd),
                    0x83 => self.fetch_signed(mem, Size::B),
                    _ => self.fetch8(mem) as u32,
                };
                if let Some(r) = self.alu(reg, a, b, size) {
                    self.write_op(mem, rm, size, r);
                }
            }
            0x84 | 0x85 => {
                let size = if op == 0x84 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let a = self.read_op(mem, rm, size);
                let b = self.reg(reg, size);
                self.logic(a & b, size);
            }
            0x86 | 0x87 => {
                let size = if op == 0x86 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let a = self.read_op(mem, rm, size);
                let b = self.reg(reg, size);
                self.write_op(mem, rm, size, b);
                self.set_reg(reg, size, a);
            }
            // ---- MOV
            0x88 | 0x89 => {
                let size = if op == 0x88 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.reg(reg, size);
                self.write_op(mem, rm, size, v);
            }
            0x8A | 0x8B => {
                let size = if op == 0x8A { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.read_op(mem, rm, size);
                self.set_reg(reg, size, v);
            }
            0x8C => {
                let (_, reg, rm) = self.modrm(mem, p);
                let s = if self.model_8086 { (reg & 3) as usize } else { (reg & 7) as usize % 6 };
                let v = self.sregs[s] as u32;
                // To a register the full operand size is written; to memory a word.
                match rm {
                    Operand::Reg(_) => self.write_op(mem, rm, wd, v),
                    Operand::Mem(..) => self.write_op(mem, rm, Size::W, v),
                }
            }
            0x8D => {
                let (_, reg, rm) = self.modrm(mem, p);
                if let Operand::Mem(_, off) = rm {
                    self.set_reg(reg, wd, off & wd.mask());
                }
            }
            0x8E => {
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.read_op(mem, rm, Size::W) as u16;
                let s = if self.model_8086 { (reg & 3) as usize } else { (reg & 7) as usize };
                if s < 6 && (s != CS || self.model_8086) {
                    self.sregs[s] = v;
                }
            }
            0x8F => {
                let (_, _, rm) = self.modrm(mem, p);
                let v = self.pop(mem, wd);
                self.write_op(mem, rm, wd, v);
            }
            0x90 => {}
            0x91..=0x97 => {
                let r = op & 7;
                let a = self.reg(0, wd);
                let b = self.reg(r, wd);
                self.set_reg(0, wd, b);
                self.set_reg(r, wd, a);
            }
            0x98 => {
                if p.op32 {
                    self.regs[AX] = self.r16(AX) as i16 as i32 as u32;
                } else {
                    self.set_r16(AX, self.al() as i8 as i16 as u16);
                }
            }
            0x99 => {
                if p.op32 {
                    self.regs[DX] = if self.regs[AX] & 0x8000_0000 != 0 { 0xFFFF_FFFF } else { 0 };
                } else {
                    self.set_r16(DX, if self.r16(AX) & 0x8000 != 0 { 0xFFFF } else { 0 });
                }
            }
            0x9A => {
                let off = self.fetch(mem, wd);
                let seg = self.fetch16(mem);
                self.push(mem, wd, self.sregs[CS] as u32);
                self.push(mem, wd, self.ip as u32);
                self.jump_far(seg, off as u16);
            }
            0x9B => {}
            0x9C => self.push(mem, wd, self.flags & 0x00FC_FFFF),
            0x9D => {
                let v = self.pop(mem, wd);
                self.load_flags(v, wd);
            }
            0x9E => {
                let ah = self.ah() as u32;
                self.flags = (self.flags & !(SF | ZF | AF | PF | CF)) | (ah & (SF | ZF | AF | PF | CF)) | FLAGS_FIXED;
            }
            0x9F => self.set_ah((self.flags as u8 & 0xD5) | 0x02),
            // ---- MOV with moffs
            0xA0..=0xA3 => {
                let size = if op & 1 == 0 { Size::B } else { wd };
                let off = if p.ad32 { self.fetch32(mem) } else { self.fetch16(mem) as u32 };
                let seg = self.sregs[p.seg.unwrap_or(DS)];
                if op < 0xA2 {
                    let v = self.rd(mem, seg, off, size);
                    self.set_reg(0, size, v);
                } else {
                    let v = self.reg(0, size);
                    self.wr(mem, seg, off, size, v);
                }
            }
            0xA4..=0xA7 | 0xAA..=0xAF => self.string_op(op, p, mem, bus),
            0xA8 => {
                let b = self.fetch8(mem) as u32;
                self.logic(self.al() as u32 & b, Size::B);
            }
            0xA9 => {
                let b = self.fetch(mem, wd);
                self.logic(self.reg(0, wd) & b, wd);
            }
            0xB0..=0xB7 => {
                let v = self.fetch8(mem);
                self.set_r8((op & 7) as usize, v);
            }
            0xB8..=0xBF => {
                let v = self.fetch(mem, wd);
                self.set_reg(op & 7, wd, v);
            }
            // ---- shifts and rotates
            0xC0 | 0xC1 | 0xD0..=0xD3 => {
                let size = if op & 1 == 0 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let count = match op {
                    0xC0 | 0xC1 => self.fetch8(mem) as u32,
                    0xD0 | 0xD1 => 1,
                    _ => self.r8(1) as u32,
                };
                let v = self.read_op(mem, rm, size);
                let r = self.shift(reg, v, count, size);
                self.write_op(mem, rm, size, r);
            }
            0xC2 | 0xC3 => {
                let n = if op == 0xC2 { self.fetch16(mem) } else { 0 };
                self.ip = self.pop(mem, wd) as u16;
                self.set_r16(SP, self.r16(SP).wrapping_add(n));
            }
            0xC4 | 0xC5 => {
                let (_, reg, rm) = self.modrm(mem, p);
                if let Operand::Mem(s, o) = rm {
                    let off = self.rd(mem, s, o, wd);
                    let seg = self.rd(mem, s, o + wd.bytes(), Size::W) as u16;
                    self.set_reg(reg, wd, off);
                    self.sregs[if op == 0xC4 { ES } else { DS }] = seg;
                }
            }
            0xC6 | 0xC7 => {
                let size = if op == 0xC6 { Size::B } else { wd };
                let (_, _, rm) = self.modrm(mem, p);
                let v = self.fetch(mem, size);
                self.write_op(mem, rm, size, v);
            }
            0xC8 => {
                let frame = self.fetch16(mem);
                let level = (self.fetch8(mem) & 0x1F) as u32;
                let bp = self.reg(BP as u8, wd);
                self.push(mem, wd, bp);
                let new_bp = self.reg(SP as u8, wd);
                if level > 0 {
                    let mut src = bp;
                    for _ in 1..level {
                        src = src.wrapping_sub(wd.bytes());
                        let v = self.rd(mem, self.sregs[SS], src, wd);
                        self.push(mem, wd, v);
                    }
                    self.push(mem, wd, new_bp);
                }
                self.set_reg(BP as u8, wd, new_bp);
                let sp = self.r16(SP).wrapping_sub(frame);
                self.set_r16(SP, sp);
            }
            0xC9 => {
                self.set_r16(SP, self.r16(BP));
                let v = self.pop(mem, wd);
                self.set_reg(BP as u8, wd, v);
            }
            0xCA | 0xCB => {
                let n = if op == 0xCA { self.fetch16(mem) } else { 0 };
                let ip = self.pop(mem, wd) as u16;
                let cs = self.pop(mem, wd) as u16;
                self.jump_far(cs, ip);
                self.set_r16(SP, self.r16(SP).wrapping_add(n));
            }
            0xCC => self.interrupt(mem, 3),
            0xCD => {
                let n = self.fetch8(mem);
                self.interrupt(mem, n);
            }
            0xCE => {
                if self.flag(OF) {
                    self.interrupt(mem, 4);
                }
            }
            0xCF => self.iret(mem, wd),
            0xD4 => {
                let base = self.fetch8(mem) as u32;
                let al = self.al() as u32;
                match al.checked_div(base) {
                    Some(q) => {
                        self.set_ah(q as u8);
                        self.set_al((al % base) as u8);
                        self.szp(al % base, Size::B);
                    }
                    None => {
                        // Flags as for a zero result, then the divide fault.
                        self.szp(0, Size::B);
                        self.set_flag(CF, false);
                        self.set_flag(OF, false);
                        self.set_flag(AF, false);
                        if !self.model_8086 {
                            self.ip = start_ip;
                        }
                        self.interrupt(mem, 0);
                    }
                }
            }
            0xD5 => {
                let base = self.fetch8(mem) as u32;
                let v = (self.al() as u32 + self.ah() as u32 * base) & 0xFF;
                self.set_al(v as u8);
                self.set_ah(0);
                self.szp(v, Size::B);
            }
            0xD6 => self.set_al(if self.flag(CF) { 0xFF } else { 0 }),
            0xD7 => {
                let seg = self.sregs[p.seg.unwrap_or(DS)];
                let v = self.rd(mem, seg, self.r16(BX).wrapping_add(self.al() as u16) as u32, Size::B);
                self.set_al(v as u8);
            }
            // ---- FPU: skipped
            0xD8..=0xDF => {
                self.modrm(mem, p);
            }
            // ---- loops and jumps
            0xE0..=0xE2 => {
                let d = self.fetch8(mem) as i8 as i16 as u16;
                let (n, cond) = if p.ad32 {
                    self.regs[CX] = self.regs[CX].wrapping_sub(1);
                    (self.regs[CX], true)
                } else {
                    let c = self.r16(CX).wrapping_sub(1);
                    self.set_r16(CX, c);
                    (c as u32, true)
                };
                let go = n != 0
                    && cond
                    && match op {
                        0xE0 => !self.flag(ZF),
                        0xE1 => self.flag(ZF),
                        _ => true,
                    };
                if go {
                    self.ip = self.ip.wrapping_add(d);
                }
            }
            0xE3 => {
                let d = self.fetch8(mem) as i8 as i16 as u16;
                let zero = if p.ad32 { self.regs[CX] == 0 } else { self.r16(CX) == 0 };
                if zero {
                    self.ip = self.ip.wrapping_add(d);
                }
            }
            0xE4 | 0xE5 => {
                let port = self.fetch8(mem) as u16;
                let size = if op == 0xE4 { Size::B } else { wd };
                let v = bus.io_read(port, size);
                self.set_reg(0, size, v);
                self.idle_hint = true;
            }
            0xE6 | 0xE7 => {
                let port = self.fetch8(mem) as u16;
                let size = if op == 0xE6 { Size::B } else { wd };
                bus.io_write(port, size, self.reg(0, size));
            }
            0xE8 => {
                let d = self.fetch(mem, wd);
                self.push(mem, wd, self.ip as u32);
                self.ip = self.ip.wrapping_add(d as u16);
            }
            0xE9 => {
                let d = self.fetch(mem, wd);
                self.ip = self.ip.wrapping_add(d as u16);
            }
            0xEA => {
                let off = self.fetch(mem, wd) as u16;
                let seg = self.fetch16(mem);
                self.jump_far(seg, off);
            }
            0xEB => {
                let d = self.fetch8(mem) as i8 as i16 as u16;
                self.ip = self.ip.wrapping_add(d);
            }
            0xEC | 0xED => {
                let size = if op == 0xEC { Size::B } else { wd };
                let v = bus.io_read(self.r16(DX), size);
                self.set_reg(0, size, v);
                self.idle_hint = true;
            }
            0xEE | 0xEF => {
                let size = if op == 0xEE { Size::B } else { wd };
                bus.io_write(self.r16(DX), size, self.reg(0, size));
            }
            0xF1 => {}
            0xF4 => {
                self.halted = true;
                self.idle_hint = true;
            }
            0xF5 => self.flags ^= CF,
            0xF6 | 0xF7 => {
                let size = if op == 0xF6 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.read_op(mem, rm, size);
                match reg {
                    0 | 1 => {
                        let imm = self.fetch(mem, size);
                        self.logic(v & imm, size);
                    }
                    2 => self.write_op(mem, rm, size, !v & size.mask()),
                    3 => {
                        let r = self.sub(0, v, false, size);
                        self.set_flag(CF, v & size.mask() != 0);
                        self.write_op(mem, rm, size, r);
                    }
                    4 => self.mul(v, size),
                    5 => self.imul1(v, size),
                    6 => {
                        if !self.div(v, size) {
                            if !self.model_8086 {
                                self.ip = start_ip;
                            }
                            self.interrupt(mem, 0);
                        }
                    }
                    _ => {
                        if !self.idiv(v, size) {
                            if !self.model_8086 {
                                self.ip = start_ip;
                            }
                            self.interrupt(mem, 0);
                        }
                    }
                }
            }
            0xF8 => self.set_flag(CF, false),
            0xF9 => self.set_flag(CF, true),
            0xFA => self.set_flag(IF, false),
            0xFB => self.set_flag(IF, true),
            0xFC => self.set_flag(DF, false),
            0xFD => self.set_flag(DF, true),
            0xFE => {
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.read_op(mem, rm, Size::B);
                let r = if reg & 1 == 0 { self.inc(v, Size::B) } else { self.dec(v, Size::B) };
                self.write_op(mem, rm, Size::B, r);
            }
            0xFF => {
                let (_, reg, rm) = self.modrm(mem, p);
                match reg {
                    0 => {
                        let v = self.read_op(mem, rm, wd);
                        let r = self.inc(v, wd);
                        self.write_op(mem, rm, wd, r);
                    }
                    1 => {
                        let v = self.read_op(mem, rm, wd);
                        let r = self.dec(v, wd);
                        self.write_op(mem, rm, wd, r);
                    }
                    2 => {
                        let target = self.read_op(mem, rm, wd) as u16;
                        self.push(mem, wd, self.ip as u32);
                        self.ip = target;
                    }
                    3 => {
                        if let Operand::Mem(s, o) = rm {
                            let off = self.rd(mem, s, o, wd) as u16;
                            let seg = self.rd(mem, s, o + wd.bytes(), Size::W) as u16;
                            self.push(mem, wd, self.sregs[CS] as u32);
                            self.push(mem, wd, self.ip as u32);
                            self.jump_far(seg, off);
                        }
                    }
                    4 => self.ip = self.read_op(mem, rm, wd) as u16,
                    5 => {
                        if let Operand::Mem(s, o) = rm {
                            let off = self.rd(mem, s, o, wd) as u16;
                            let seg = self.rd(mem, s, o + wd.bytes(), Size::W) as u16;
                            self.jump_far(seg, off);
                        }
                    }
                    6 => {
                        let v = self.read_op(mem, rm, wd);
                        self.push(mem, wd, v);
                    }
                    _ => return Err(self.unsupported(op, None, start_ip)),
                }
            }
            _ => return Err(self.unsupported(op, None, start_ip)),
        }
        Ok(())
    }

    fn exec_0f(&mut self, p: &Prefix, start_ip: u16, mem: &mut Memory, bus: &mut dyn Bus) -> Result<(), CpuError> {
        let wd = Size::wd(p.op32);
        let op = self.fetch8(mem);
        match op {
            0x80..=0x8F => {
                let d = self.fetch(mem, wd) as u16;
                if self.cond(op & 0xF) {
                    self.ip = self.ip.wrapping_add(d);
                }
            }
            0x90..=0x9F => {
                let (_, _, rm) = self.modrm(mem, p);
                let v = self.cond(op & 0xF) as u32;
                self.write_op(mem, rm, Size::B, v);
            }
            0xA0 => self.push(mem, wd, self.sregs[FS] as u32),
            0xA1 => self.sregs[FS] = self.pop(mem, wd) as u16,
            0xA8 => self.push(mem, wd, self.sregs[GS] as u32),
            0xA9 => self.sregs[GS] = self.pop(mem, wd) as u16,
            0xA2 => {
                // CPUID: say "no", like a 386.
                self.regs[AX] = 0;
                self.regs[BX] = 0;
                self.regs[CX] = 0;
                self.regs[DX] = 0;
            }
            0xA3 | 0xAB | 0xB3 | 0xBB => self.bit_op(op, p, mem, None),
            0xBA => {
                let (_, reg, rm) = self.modrm(mem, p);
                let n = self.fetch8(mem) as u32;
                if reg < 4 {
                    return Err(self.unsupported(0x0F, Some(op), start_ip));
                }
                self.bit_apply(reg - 4, rm, n, wd, mem);
            }
            0xA4 | 0xA5 | 0xAC | 0xAD => {
                let (_, reg, rm) = self.modrm(mem, p);
                let count = if op & 1 == 0 { self.fetch8(mem) as u32 } else { self.r8(1) as u32 } & 0x1F;
                let dst = self.read_op(mem, rm, wd);
                let src = self.reg(reg, wd);
                if count == 0 {
                    return Ok(());
                }
                let bits = wd.bits();
                let count = count % bits;
                let r = if count == 0 {
                    dst
                } else if op < 0xA8 {
                    // SHLD
                    let r = ((dst << count) | (src >> (bits - count))) & wd.mask();
                    self.set_flag(CF, (dst >> (bits - count)) & 1 != 0);
                    self.set_flag(OF, (dst ^ r) & wd.sign() != 0);
                    r
                } else {
                    let r = ((dst >> count) | (src << (bits - count))) & wd.mask();
                    self.set_flag(CF, (dst >> (count - 1)) & 1 != 0);
                    self.set_flag(OF, (dst ^ r) & wd.sign() != 0);
                    r
                };
                self.szp(r, wd);
                self.write_op(mem, rm, wd, r);
            }
            0xAF => {
                let (_, reg, rm) = self.modrm(mem, p);
                let a = self.reg(reg, wd);
                let b = self.read_op(mem, rm, wd);
                let r = self.imul2(a, b, wd);
                self.set_reg(reg, wd, r);
            }
            0xB0 | 0xB1 => {
                let size = if op == 0xB0 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let dst = self.read_op(mem, rm, size);
                let acc = self.reg(0, size);
                self.sub(acc, dst, false, size);
                if acc & size.mask() == dst & size.mask() {
                    let v = self.reg(reg, size);
                    self.write_op(mem, rm, size, v);
                } else {
                    self.set_reg(0, size, dst);
                }
            }
            0xB2 | 0xB4 | 0xB5 => {
                let (_, reg, rm) = self.modrm(mem, p);
                if let Operand::Mem(s, o) = rm {
                    let off = self.rd(mem, s, o, wd);
                    let seg = self.rd(mem, s, o + wd.bytes(), Size::W) as u16;
                    self.set_reg(reg, wd, off);
                    self.sregs[match op {
                        0xB2 => SS,
                        0xB4 => FS,
                        _ => GS,
                    }] = seg;
                }
            }
            0xB6 | 0xB7 | 0xBE | 0xBF => {
                let src = if op & 1 == 0 { Size::B } else { Size::W };
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.read_op(mem, rm, src);
                let v = if op >= 0xBE {
                    if src == Size::B { v as u8 as i8 as i32 as u32 } else { v as u16 as i16 as i32 as u32 }
                } else {
                    v
                };
                self.set_reg(reg, wd, v & wd.mask());
            }
            0xBC | 0xBD => {
                let (_, reg, rm) = self.modrm(mem, p);
                let v = self.read_op(mem, rm, wd) & wd.mask();
                self.set_flag(ZF, v == 0);
                if v != 0 {
                    let r = if op == 0xBC { v.trailing_zeros() } else { 31 - v.leading_zeros() };
                    self.set_reg(reg, wd, r);
                }
            }
            0xC0 | 0xC1 => {
                let size = if op == 0xC0 { Size::B } else { wd };
                let (_, reg, rm) = self.modrm(mem, p);
                let dst = self.read_op(mem, rm, size);
                let src = self.reg(reg, size);
                let r = self.add(dst, src, false, size);
                self.set_reg(reg, size, dst);
                self.write_op(mem, rm, size, r);
            }
            0xC8..=0xCF => {
                let r = (op & 7) as usize;
                self.regs[r] = self.regs[r].swap_bytes();
            }
            0x00 | 0x01 => {
                // SGDT/SIDT and friends: real-mode code occasionally stores
                // them to sniff the CPU; give nothing back.
                let (_, _reg, _rm) = self.modrm(mem, p);
            }
            _ => return Err(self.unsupported(0x0F, Some(op), start_ip)),
        }
        let _ = bus;
        Ok(())
    }

    fn bit_op(&mut self, op: u8, p: &Prefix, mem: &mut Memory, _n: Option<u32>) {
        let wd = Size::wd(p.op32);
        let (_, reg, rm) = self.modrm(mem, p);
        let n = self.reg(reg, wd);
        let which = match op {
            0xA3 => 0,
            0xAB => 1,
            0xB3 => 2,
            _ => 3,
        };
        // With a memory operand the bit offset reaches beyond the operand.
        let rm = match rm {
            Operand::Mem(s, o) => {
                let signed = if wd == Size::W { n as u16 as i16 as i32 } else { n as i32 };
                let byte_off = signed.div_euclid(wd.bits() as i32) * wd.bytes() as i32;
                Operand::Mem(s, o.wrapping_add(byte_off as u32) & 0xFFFF)
            }
            r => r,
        };
        self.bit_apply(which, rm, n, wd, mem);
    }

    /// BT (0), BTS (1), BTR (2), BTC (3) on bit `n` of the operand.
    fn bit_apply(&mut self, which: u8, rm: Operand, n: u32, wd: Size, mem: &mut Memory) {
        let n = n % wd.bits();
        let v = self.read_op(mem, rm, wd);
        let bit = (v >> n) & 1 != 0;
        self.set_flag(CF, bit);
        let r = match which {
            0 => return,
            1 => v | (1 << n),
            2 => v & !(1 << n),
            _ => v ^ (1 << n),
        };
        self.write_op(mem, rm, wd, r & wd.mask());
    }

    // ---- multiply and divide --------------------------------------------------

    fn mul(&mut self, v: u32, size: Size) {
        let a = self.reg(0, size) as u64;
        let r = a * (v & size.mask()) as u64;
        let hi = (r >> size.bits()) as u32 & size.mask();
        match size {
            Size::B => self.set_ax(r as u16),
            Size::W => {
                self.set_ax(r as u16);
                self.set_r16(DX, hi as u16);
            }
            Size::D => {
                self.regs[AX] = r as u32;
                self.regs[DX] = hi;
            }
        }
        let over = hi != 0;
        self.set_flag(CF, over);
        self.set_flag(OF, over);
        self.szp(r as u32, size);
    }

    fn sext(v: u32, size: Size) -> i64 {
        match size {
            Size::B => v as u8 as i8 as i64,
            Size::W => v as u16 as i16 as i64,
            Size::D => v as i32 as i64,
        }
    }

    fn imul1(&mut self, v: u32, size: Size) {
        let r = Self::sext(self.reg(0, size), size) * Self::sext(v, size);
        let lo = r as u32 & size.mask();
        let hi = (r >> size.bits()) as u32 & size.mask();
        match size {
            Size::B => self.set_ax(r as u16),
            Size::W => {
                self.set_ax(r as u16);
                self.set_r16(DX, hi as u16);
            }
            Size::D => {
                self.regs[AX] = lo;
                self.regs[DX] = hi;
            }
        }
        let over = r != Self::sext(lo, size);
        self.set_flag(CF, over);
        self.set_flag(OF, over);
        self.szp(lo, size);
    }

    fn imul2(&mut self, a: u32, b: u32, size: Size) -> u32 {
        let r = Self::sext(a, size) * Self::sext(b, size);
        let lo = r as u32 & size.mask();
        let over = r != Self::sext(lo, size);
        self.set_flag(CF, over);
        self.set_flag(OF, over);
        self.szp(lo, size);
        lo
    }

    /// Unsigned divide; false means a divide fault.
    fn div(&mut self, v: u32, size: Size) -> bool {
        let d = (v & size.mask()) as u64;
        if d == 0 {
            return false;
        }
        let (num, qmax): (u64, u64) = match size {
            Size::B => (self.ax() as u64, 0xFF),
            Size::W => ((self.dx() as u64) << 16 | self.ax() as u64, 0xFFFF),
            Size::D => ((self.regs[DX] as u64) << 32 | self.regs[AX] as u64, 0xFFFF_FFFF),
        };
        let (q, r) = (num / d, num % d);
        if q > qmax {
            return false;
        }
        match size {
            Size::B => {
                self.set_al(q as u8);
                self.set_ah(r as u8);
            }
            Size::W => {
                self.set_ax(q as u16);
                self.set_r16(DX, r as u16);
            }
            Size::D => {
                self.regs[AX] = q as u32;
                self.regs[DX] = r as u32;
            }
        }
        true
    }

    fn idiv(&mut self, v: u32, size: Size) -> bool {
        let d = Self::sext(v, size);
        if d == 0 {
            return false;
        }
        let num: i64 = match size {
            Size::B => self.ax() as i16 as i64,
            Size::W => ((self.dx() as u32) << 16 | self.ax() as u32) as i32 as i64,
            Size::D => ((self.regs[DX] as u64) << 32 | self.regs[AX] as u64) as i64,
        };
        let (q, r) = (num / d, num % d);
        let (qmin, qmax): (i64, i64) = match size {
            Size::B => (-128, 127),
            Size::W => (-32768, 32767),
            Size::D => (i32::MIN as i64, i32::MAX as i64),
        };
        if q < qmin || q > qmax {
            return false;
        }
        match size {
            Size::B => {
                self.set_al(q as u8);
                self.set_ah(r as u8);
            }
            Size::W => {
                self.set_ax(q as u16);
                self.set_r16(DX, r as u16);
            }
            Size::D => {
                self.regs[AX] = q as u32;
                self.regs[DX] = r as u32;
            }
        }
        true
    }

    // ---- BCD ----------------------------------------------------------------

    fn daa(&mut self) {
        let old = self.al();
        let cf = self.flag(CF);
        let af = self.flag(AF);
        let mut al = old;
        if old & 0x0F > 9 || af {
            al = al.wrapping_add(6);
            self.set_flag(AF, true);
        } else {
            self.set_flag(AF, false);
        }
        // The 8086 counts 9A-9F as over 99 only with AF clear (seen on
        // the chip); later chips compare with 99 outright.
        let over = if self.model_8086 && af { old > 0x9F } else { old > 0x99 };
        if over || cf {
            al = al.wrapping_add(0x60);
            self.set_flag(CF, true);
        } else {
            self.set_flag(CF, false);
        }
        self.set_al(al);
        self.szp(al as u32, Size::B);
    }

    fn das(&mut self) {
        let old = self.al();
        let cf = self.flag(CF);
        let af = self.flag(AF);
        let mut al = old;
        if old & 0x0F > 9 || af {
            al = al.wrapping_sub(6);
            self.set_flag(AF, true);
        } else {
            self.set_flag(AF, false);
        }
        let over = if self.model_8086 && af { old > 0x9F } else { old > 0x99 };
        if over || cf {
            al = al.wrapping_sub(0x60);
            self.set_flag(CF, true);
        } else {
            self.set_flag(CF, false);
        }
        self.set_al(al);
        self.szp(al as u32, Size::B);
    }

    /// AAA/AAS: AL and AH are adjusted separately (no carry between).
    fn aaa(&mut self) {
        if self.al() & 0x0F > 9 || self.flag(AF) {
            self.set_al(self.al().wrapping_add(6));
            self.set_ah(self.ah().wrapping_add(1));
            self.set_flag(AF, true);
            self.set_flag(CF, true);
        } else {
            self.set_flag(AF, false);
            self.set_flag(CF, false);
        }
        self.set_al(self.al() & 0x0F);
        self.szp(self.al() as u32, Size::B);
    }

    fn aas(&mut self) {
        if self.al() & 0x0F > 9 || self.flag(AF) {
            self.set_al(self.al().wrapping_sub(6));
            self.set_ah(self.ah().wrapping_sub(1));
            self.set_flag(AF, true);
            self.set_flag(CF, true);
        } else {
            self.set_flag(AF, false);
            self.set_flag(CF, false);
        }
        self.set_al(self.al() & 0x0F);
        self.szp(self.al() as u32, Size::B);
    }

    // ---- string instructions ------------------------------------------------

    fn string_op(&mut self, op: u8, p: &Prefix, mem: &mut Memory, bus: &mut dyn Bus) {
        let size = if op & 1 == 0 { Size::B } else { Size::wd(p.op32) };
        let step = if self.flag(DF) { (size.bytes() as i32).wrapping_neg() } else { size.bytes() as i32 };
        let seg = self.sregs[p.seg.unwrap_or(DS)];
        let es = self.sregs[ES];
        let count = |cpu: &Cpu| if p.ad32 { cpu.regs[CX] } else { cpu.r16(CX) as u32 };
        let set_count = |cpu: &mut Cpu, c: u32| {
            if p.ad32 {
                cpu.regs[CX] = c;
            } else {
                cpu.set_r16(CX, c as u16);
            }
        };
        let adv = |cpu: &mut Cpu, r: usize| {
            if p.ad32 {
                cpu.regs[r] = cpu.regs[r].wrapping_add(step as u32);
            } else {
                cpu.set_r16(r, cpu.r16(r).wrapping_add(step as u16));
            }
        };
        let idx = |cpu: &Cpu, r: usize| if p.ad32 { cpu.regs[r] } else { cpu.r16(r) as u32 };
        let mut remaining = if p.rep == Rep::None { 1 } else { count(self) };
        while remaining > 0 {
            if p.rep != Rep::None && count(self) == 0 {
                break;
            }
            let mut stop = false;
            match op {
                // MOVS
                0xA4 | 0xA5 => {
                    let v = self.rd(mem, seg, idx(self, SI), size);
                    self.wr(mem, es, idx(self, DI), size, v);
                    adv(self, SI);
                    adv(self, DI);
                }
                // CMPS
                0xA6 | 0xA7 => {
                    let a = self.rd(mem, seg, idx(self, SI), size);
                    let b = self.rd(mem, es, idx(self, DI), size);
                    self.sub(a, b, false, size);
                    adv(self, SI);
                    adv(self, DI);
                    stop = match p.rep {
                        Rep::Z => !self.flag(ZF),
                        Rep::Nz => self.flag(ZF),
                        Rep::None => false,
                    };
                }
                // STOS
                0xAA | 0xAB => {
                    let v = self.reg(0, size);
                    self.wr(mem, es, idx(self, DI), size, v);
                    adv(self, DI);
                }
                // LODS
                0xAC | 0xAD => {
                    let v = self.rd(mem, seg, idx(self, SI), size);
                    self.set_reg(0, size, v);
                    adv(self, SI);
                }
                // SCAS
                0xAE | 0xAF => {
                    let a = self.reg(0, size);
                    let b = self.rd(mem, es, idx(self, DI), size);
                    self.sub(a, b, false, size);
                    adv(self, DI);
                    stop = match p.rep {
                        Rep::Z => !self.flag(ZF),
                        Rep::Nz => self.flag(ZF),
                        Rep::None => false,
                    };
                }
                // INS
                0x6C | 0x6D => {
                    let v = bus.io_read(self.r16(DX), size);
                    self.wr(mem, es, idx(self, DI), size, v);
                    adv(self, DI);
                }
                // OUTS
                _ => {
                    let v = self.rd(mem, seg, idx(self, SI), size);
                    bus.io_write(self.r16(DX), size, v);
                    adv(self, SI);
                }
            }
            if p.rep != Rep::None {
                set_count(self, count(self).wrapping_sub(1));
            }
            remaining -= 1;
            if stop {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NullBus;

    fn run(code: &[u8], setup: impl FnOnce(&mut Cpu, &mut Memory)) -> (Cpu, Memory) {
        let mut cpu = Cpu::new();
        let mut mem = Memory::new();
        cpu.sregs[CS] = 0x1000;
        cpu.sregs[DS] = 0x1000;
        cpu.sregs[SS] = 0x2000;
        cpu.set_r16(SP, 0xFFFE);
        mem.write(0x10000, code);
        setup(&mut cpu, &mut mem);
        let mut bus = NullBus;
        for _ in 0..1000 {
            if cpu.halted {
                break;
            }
            cpu.step(&mut mem, &mut bus).unwrap();
        }
        (cpu, mem)
    }

    #[test]
    fn arithmetic_and_flags() {
        // mov ax,0x7FFF; add ax,1; hlt
        let (cpu, _) = run(&[0xB8, 0xFF, 0x7F, 0x05, 0x01, 0x00, 0xF4], |_, _| {});
        assert_eq!(cpu.ax(), 0x8000);
        assert!(cpu.flag(OF) && cpu.flag(SF) && !cpu.flag(CF) && !cpu.flag(ZF));
        // mov al,0xFF; add al,1 -> CF ZF AF
        let (cpu, _) = run(&[0xB0, 0xFF, 0x04, 0x01, 0xF4], |_, _| {});
        assert_eq!(cpu.al(), 0);
        assert!(cpu.flag(CF) && cpu.flag(ZF) && cpu.flag(AF) && cpu.flag(PF));
        // mov ax,10; mov bx,3; xor dx,dx; div bx -> ax=3 dx=1
        let (cpu, _) = run(&[0xB8, 10, 0, 0xBB, 3, 0, 0x31, 0xD2, 0xF7, 0xF3, 0xF4], |_, _| {});
        assert_eq!((cpu.ax(), cpu.dx()), (3, 1));
    }

    #[test]
    fn memory_stack_and_calls() {
        // mov word [0x100],0x1234; push word [0x100]; pop bx; call sub; hlt; sub: inc bx; ret
        let code = [
            0xC7, 0x06, 0x00, 0x01, 0x34, 0x12, // mov word [0x100],0x1234
            0xFF, 0x36, 0x00, 0x01, // push word [0x100]
            0x5B, // pop bx
            0xE8, 0x01, 0x00, // call +1
            0xF4, // hlt
            0x43, // inc bx
            0xC3, // ret
        ];
        let (cpu, mem) = run(&code, |_, _| {});
        assert_eq!(cpu.bx(), 0x1235);
        assert_eq!(mem.read16(0x10100), 0x1234);
        assert_eq!(cpu.sp(), 0xFFFE);
    }

    #[test]
    fn strings_loops_and_shifts() {
        // mov si,0x200; mov di,0x300; mov cx,4; cld; rep movsb; mov cx,3; L: shl al,1; loop L; hlt
        let code = [0xBE, 0x00, 0x02, 0xBF, 0x00, 0x03, 0xB9, 4, 0, 0xFC, 0xF3, 0xA4, 0xB9, 3, 0, 0xD0, 0xE0, 0xE2, 0xFC, 0xF4];
        let (cpu, mem) = run(&code, |cpu, mem| {
            cpu.sregs[ES] = 0x1000;
            cpu.set_al(1);
            mem.write(0x10200, b"abcd");
        });
        assert_eq!(mem.read(0x10300, 4), b"abcd");
        assert_eq!((cpu.si(), cpu.di(), cpu.cx()), (0x204, 0x304, 0));
        assert_eq!(cpu.al(), 8);
    }

    #[test]
    fn interrupts_vector_and_return() {
        // IVT 0x21 -> 1000:0010 (handler: inc ax; iret); code: int 21h; hlt
        let code = [0xCD, 0x21, 0xF4];
        let (cpu, _) = run(&code, |cpu, mem| {
            mem.write16(0x21 * 4, 0x0010);
            mem.write16(0x21 * 4 + 2, 0x1000);
            mem.write(0x10010, &[0x40, 0xCF]);
            cpu.set_ax(41);
            cpu.set_flag(IF, true);
        });
        assert_eq!(cpu.ax(), 42);
        assert!(cpu.flag(IF), "IRET restored IF");
        assert_eq!(cpu.sp(), 0xFFFE);
    }

    #[test]
    fn thirty_two_bit_and_0f() {
        // mov eax,0x12345678 (66 B8); movzx bx,al (0F B6 D8); bswap eax (66 0F C8); hlt
        let code = [0x66, 0xB8, 0x78, 0x56, 0x34, 0x12, 0x0F, 0xB6, 0xD8, 0x66, 0x0F, 0xC8, 0xF4];
        let (cpu, _) = run(&code, |_, _| {});
        assert_eq!(cpu.bx(), 0x78);
        assert_eq!(cpu.regs[AX], 0x78563412);
    }
}
