//! The coprocessor (D8-DF), a 387 as real-mode programs see it: for
//! doors built for one (Turbo Pascal's `{$N+,E-}`, TW2002). Registers
//! hold `f64`, not the chip's 80-bit extended reals: 80-bit loads and
//! stores convert, and results keep 53 bits of precision rather than 64.
//! Exceptions always get the masked response (the flag is set, the
//! result is a NaN or infinity); none interrupts the program.

use super::{AX, Cpu, Memory, Operand, Prefix, Size};

const IE: u16 = 1;
const ZE: u16 = 1 << 2;
const OE: u16 = 1 << 3;
const SF: u16 = 1 << 6;
const ES: u16 = 1 << 7;
const C0: u16 = 1 << 8;
const C1: u16 = 1 << 9;
const C2: u16 = 1 << 10;
const C3: u16 = 1 << 14;

#[derive(Debug, Clone)]
pub struct Fpu {
    /// The physical registers; ST(i) is `st[(top + i) & 7]`.
    st: [f64; 8],
    /// A bit per physical register that's empty.
    empty: u8,
    top: u8,
    pub cw: u16,
    /// Exception flags and condition codes; TOP is in `top`.
    sw: u16,
}

impl Default for Fpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Fpu {
    pub fn new() -> Self {
        Self { st: [0.0; 8], empty: 0xFF, top: 0, cw: 0x037F, sw: 0 }
    }

    /// The status word, with TOP and the error summary.
    pub fn status(&self) -> u16 {
        let mut sw = (self.sw & !0x3800) | ((self.top as u16) << 11);
        if self.sw & !self.cw & 0x3F != 0 {
            sw |= ES | 0x8000;
        } else {
            sw &= !(ES | 0x8000);
        }
        sw
    }

    fn tags(&self) -> u16 {
        (0..8).fold(0, |t, p| {
            let tag = if self.empty & (1 << p) != 0 {
                3
            } else if self.st[p] == 0.0 {
                1
            } else if self.st[p].is_normal() {
                0
            } else {
                2
            };
            t | (tag << (2 * p))
        })
    }

    fn set_tags(&mut self, t: u16) {
        self.empty = (0..8).fold(0, |e, p| if (t >> (2 * p)) & 3 == 3 { e | (1 << p) } else { e });
    }

    fn phys(&self, i: u8) -> usize {
        ((self.top + i) & 7) as usize
    }

    /// ST(i); an empty register is a stack underflow, read as a NaN.
    fn get(&mut self, i: u8) -> f64 {
        let p = self.phys(i);
        if self.empty & (1 << p) != 0 {
            self.sw = (self.sw | IE | SF) & !C1;
            return f64::NAN;
        }
        self.st[p]
    }

    fn set(&mut self, i: u8, v: f64) {
        let p = self.phys(i);
        self.st[p] = v;
        self.empty &= !(1 << p);
    }

    fn push(&mut self, v: f64) {
        self.top = (self.top + 7) & 7;
        let p = self.top as usize;
        let v = if self.empty & (1 << p) == 0 {
            self.sw |= IE | SF | C1;
            f64::NAN
        } else {
            v
        };
        self.st[p] = v;
        self.empty &= !(1 << p);
    }

    fn pop(&mut self) {
        self.empty |= 1 << self.top;
        self.top = (self.top + 1) & 7;
    }

    /// Round to an integer as the control word says.
    fn round(&self, v: f64) -> f64 {
        match (self.cw >> 10) & 3 {
            0 => v.round_ties_even(),
            1 => v.floor(),
            2 => v.ceil(),
            _ => v.trunc(),
        }
    }

    /// Set C3, C2 and C0 from comparing `a` with `b`.
    fn compare(&mut self, a: f64, b: f64, quiet: bool) {
        self.sw &= !(C0 | C1 | C2 | C3);
        if a.is_nan() || b.is_nan() {
            self.sw |= C0 | C2 | C3;
            if !quiet {
                self.sw |= IE;
            }
        } else if a < b {
            self.sw |= C0;
        } else if a == b {
            self.sw |= C3;
        }
    }

    /// The D8/DC/DA/DE operations 0-7 (FADD FMUL FCOM FCOMP FSUB FSUBR
    /// FDIV FDIVR) on `a` and `b`; the comparisons give `None`.
    fn arith(&mut self, op: u8, a: f64, b: f64) -> Option<f64> {
        let r = match op {
            0 => a + b,
            1 => a * b,
            2 | 3 => {
                self.compare(a, b, false);
                return None;
            }
            4 => a - b,
            5 => b - a,
            6 => self.divide(a, b),
            _ => self.divide(b, a),
        };
        self.check(r, a, b);
        Some(r)
    }

    fn divide(&mut self, a: f64, b: f64) -> f64 {
        if b == 0.0 && a.is_finite() && a != 0.0 {
            self.sw |= ZE;
        }
        a / b
    }

    /// Flag an invalid operation (a NaN out of numbers) or an overflow.
    fn check(&mut self, r: f64, a: f64, b: f64) {
        if r.is_nan() && !a.is_nan() && !b.is_nan() {
            self.sw |= IE;
        } else if r.is_infinite() && a.is_finite() && b.is_finite() && self.sw & ZE == 0 {
            self.sw |= OE;
        }
    }

    /// `v` as an integer of `bits` for FIST, or the integer indefinite.
    fn int_of(&mut self, v: f64, bits: u32) -> i64 {
        let r = self.round(v);
        let lim = (1u64 << (bits - 1)) as f64;
        if r.is_nan() || r >= lim || r < -lim {
            self.sw |= IE;
            return i64::MIN >> (64 - bits);
        }
        r as i64
    }
}

/// `x` times two to the `e`, without overflowing on the way.
fn ldexp(mut x: f64, mut e: i32) -> f64 {
    while e > 1000 {
        x *= 2f64.powi(1000);
        e -= 1000;
    }
    while e < -1000 {
        x *= 2f64.powi(-1000);
        e += 1000;
    }
    x * 2f64.powi(e)
}

/// An 80-bit extended real as `f64` (rounded to nearest).
pub fn from_ext(b: &[u8; 10]) -> f64 {
    let mant = u64::from_le_bytes(b[..8].try_into().unwrap());
    let se = u16::from_le_bytes([b[8], b[9]]);
    let exp = (se & 0x7FFF) as i32;
    let v = if exp == 0x7FFF {
        if mant << 1 == 0 { f64::INFINITY } else { f64::NAN }
    } else if mant == 0 {
        0.0
    } else {
        ldexp(mant as f64, exp.max(1) - 16383 - 63)
    };
    if se & 0x8000 != 0 { -v } else { v }
}

/// `v` as an 80-bit extended real (exactly).
pub fn to_ext(v: f64) -> [u8; 10] {
    let bits = v.to_bits();
    let e = ((bits >> 52) & 0x7FF) as i32;
    let f = bits & ((1 << 52) - 1);
    let (exp, mant): (i32, u64) = if e == 0x7FF {
        (0x7FFF, if f == 0 { 1 << 63 } else { (3 << 62) | (f << 11) })
    } else if e == 0 {
        if f == 0 {
            (0, 0)
        } else {
            let lz = f.leading_zeros();
            (16383 + 63 - 1074 - lz as i32, f << lz)
        }
    } else {
        (e - 1023 + 16383, (1 << 63) | (f << 11))
    };
    let mut out = [0u8; 10];
    out[..8].copy_from_slice(&mant.to_le_bytes());
    let se = exp as u16 | if bits >> 63 != 0 { 0x8000 } else { 0 };
    out[8..].copy_from_slice(&se.to_le_bytes());
    out
}

impl Cpu {
    fn fread(&self, mem: &Memory, seg: u16, off: u32, out: &mut [u8]) {
        for (i, b) in out.iter_mut().enumerate() {
            *b = self.rd(mem, seg, off.wrapping_add(i as u32), Size::B) as u8;
        }
    }

    fn fwrite(&self, mem: &mut Memory, seg: u16, off: u32, data: &[u8]) {
        for (i, b) in data.iter().enumerate() {
            self.wr(mem, seg, off.wrapping_add(i as u32), Size::B, *b as u32);
        }
    }

    /// A coprocessor instruction (`op` D8-DF) with this CPU's FPU.
    pub(super) fn esc(&mut self, op: u8, p: &Prefix, mem: &mut Memory) {
        let Some(mut f) = self.fpu.take() else { return };
        let (_, reg, operand) = self.modrm(mem, p);
        match operand {
            Operand::Reg(rm) => self.esc_reg(&mut f, op, reg, rm),
            Operand::Mem(seg, off) => self.esc_mem(&mut f, op, reg, seg, off, p.op32, mem),
        }
        self.fpu = Some(f);
    }

    #[allow(clippy::too_many_arguments)]
    fn esc_mem(&mut self, f: &mut Fpu, op: u8, reg: u8, seg: u16, off: u32, op32: bool, mem: &mut Memory) {
        let mut b = [0u8; 10];
        let mut load = |cpu: &Cpu, n: usize| -> [u8; 10] {
            cpu.fread(mem, seg, off, &mut b[..n]);
            b
        };
        match (op, reg) {
            // Arithmetic with a memory operand.
            (0xD8 | 0xDA | 0xDC | 0xDE, _) => {
                let v = match op {
                    0xD8 => f32::from_le_bytes(load(self, 4)[..4].try_into().unwrap()) as f64,
                    0xDC => f64::from_le_bytes(load(self, 8)[..8].try_into().unwrap()),
                    0xDA => i32::from_le_bytes(load(self, 4)[..4].try_into().unwrap()) as f64,
                    _ => i16::from_le_bytes(load(self, 2)[..2].try_into().unwrap()) as f64,
                };
                let a = f.get(0);
                if let Some(r) = f.arith(reg, a, v) {
                    f.set(0, r);
                }
                if reg == 3 {
                    f.pop();
                }
            }
            (0xD9, 0) => {
                let v = f32::from_le_bytes(load(self, 4)[..4].try_into().unwrap()) as f64;
                f.push(v);
            }
            (0xD9, 2 | 3) => {
                let v = f.get(0) as f32;
                self.fwrite(mem, seg, off, &v.to_le_bytes());
                if reg == 3 {
                    f.pop();
                }
            }
            (0xD9, 4) => {
                self.fldenv(f, mem, seg, off, op32);
            }
            (0xD9, 5) => f.cw = self.rd(mem, seg, off, Size::W) as u16 | 0x40,
            (0xD9, 6) => {
                self.fstenv(f, mem, seg, off, op32);
            }
            (0xD9, 7) => self.wr(mem, seg, off, Size::W, f.cw as u32),
            (0xDB, 0) => {
                let v = i32::from_le_bytes(load(self, 4)[..4].try_into().unwrap());
                f.push(v as f64);
            }
            (0xDB, 2 | 3) => {
                let v = f.get(0);
                let i = f.int_of(v, 32) as i32;
                self.fwrite(mem, seg, off, &i.to_le_bytes());
                if reg == 3 {
                    f.pop();
                }
            }
            (0xDB, 5) => {
                let v = from_ext(&load(self, 10));
                f.push(v);
            }
            (0xDB, 7) => {
                let v = f.get(0);
                self.fwrite(mem, seg, off, &to_ext(v));
                f.pop();
            }
            (0xDD, 0) => {
                let v = f64::from_le_bytes(load(self, 8)[..8].try_into().unwrap());
                f.push(v);
            }
            (0xDD, 2 | 3) => {
                let v = f.get(0);
                self.fwrite(mem, seg, off, &v.to_le_bytes());
                if reg == 3 {
                    f.pop();
                }
            }
            (0xDD, 4) => {
                let n = self.fldenv(f, mem, seg, off, op32);
                for i in 0..8u8 {
                    let mut r = [0u8; 10];
                    self.fread(mem, seg, off.wrapping_add(n + 10 * i as u32), &mut r);
                    let p = f.phys(i);
                    f.st[p] = from_ext(&r);
                }
            }
            (0xDD, 6) => {
                let n = self.fstenv(f, mem, seg, off, op32);
                for i in 0..8u8 {
                    let v = f.st[f.phys(i)];
                    self.fwrite(mem, seg, off.wrapping_add(n + 10 * i as u32), &to_ext(v));
                }
                *f = Fpu::new();
            }
            (0xDD, 7) => self.wr(mem, seg, off, Size::W, f.status() as u32),
            (0xDF, 0) => {
                let v = i16::from_le_bytes(load(self, 2)[..2].try_into().unwrap());
                f.push(v as f64);
            }
            (0xDF, 2 | 3) => {
                let v = f.get(0);
                let i = f.int_of(v, 16) as i16;
                self.fwrite(mem, seg, off, &i.to_le_bytes());
                if reg == 3 {
                    f.pop();
                }
            }
            (0xDF, 4) => {
                // FBLD: 18 packed BCD digits and a sign byte.
                let b = load(self, 10);
                let v = b[..9].iter().rev().fold(0f64, |v, d| v * 100.0 + ((d >> 4) * 10 + (d & 15)) as f64);
                f.push(if b[9] & 0x80 != 0 { -v } else { v });
            }
            (0xDF, 5) => {
                let v = i64::from_le_bytes(load(self, 8)[..8].try_into().unwrap());
                f.push(v as f64);
            }
            (0xDF, 6) => {
                let v = f.get(0);
                let v = f.round(v);
                let mut out = [0u8; 10];
                if v.is_nan() || v.abs() >= 1e18 {
                    f.sw |= IE;
                    out[7] = 0xC0;
                    out[8] = 0xFF;
                    out[9] = 0xFF;
                } else {
                    let mut n = v.abs() as u64;
                    for d in out.iter_mut().take(9) {
                        *d = ((n % 10) as u8) | (((n / 10 % 10) as u8) << 4);
                        n /= 100;
                    }
                    if v.is_sign_negative() {
                        out[9] = 0x80;
                    }
                }
                self.fwrite(mem, seg, off, &out);
                f.pop();
            }
            (0xDF, 7) => {
                let v = f.get(0);
                let i = f.int_of(v, 64);
                self.fwrite(mem, seg, off, &i.to_le_bytes());
                f.pop();
            }
            _ => {}
        }
    }

    /// FLDENV: the control, status and tag words; the size of the
    /// environment (14 bytes, 28 with a 32-bit operand size).
    fn fldenv(&self, f: &mut Fpu, mem: &Memory, seg: u16, off: u32, op32: bool) -> u32 {
        let step = if op32 { 4 } else { 2 };
        f.cw = self.rd(mem, seg, off, Size::W) as u16 | 0x40;
        let sw = self.rd(mem, seg, off.wrapping_add(step), Size::W) as u16;
        f.top = ((sw >> 11) & 7) as u8;
        f.sw = sw & !0x3800;
        let tw = self.rd(mem, seg, off.wrapping_add(2 * step), Size::W) as u16;
        f.set_tags(tw);
        7 * step
    }

    /// FSTENV, with the instruction and operand pointers as zero.
    fn fstenv(&self, f: &mut Fpu, mem: &mut Memory, seg: u16, off: u32, op32: bool) -> u32 {
        let step = if op32 { 4 } else { 2 };
        let words = [f.cw, f.status(), f.tags(), 0, 0, 0, 0];
        for (i, w) in words.iter().enumerate() {
            let size = if op32 { Size::D } else { Size::W };
            let v = if op32 && i == 0 { *w as u32 | 0xFFFF_0000 } else { *w as u32 };
            self.wr(mem, seg, off.wrapping_add(step * i as u32), size, v);
        }
        f.cw |= 0x3F;
        7 * step
    }

    fn esc_reg(&mut self, f: &mut Fpu, op: u8, reg: u8, i: u8) {
        match (op, reg) {
            (0xD8, _) => {
                let (a, b) = (f.get(0), f.get(i));
                if let Some(r) = f.arith(reg, a, b) {
                    f.set(0, r);
                }
                if reg == 3 {
                    f.pop();
                }
            }
            // DC and DE: ST(i) is the destination, and SUB/SUBR and
            // DIV/DIVR trade places.
            (0xDC | 0xDE, _) => {
                if op == 0xDE && reg == 3 && i != 1 {
                    return;
                }
                let (a, b) = (f.get(i), f.get(0));
                let o = if reg >= 4 { reg ^ 1 } else { reg };
                match f.arith(o, a, b) {
                    Some(r) => f.set(i, r),
                    // FCOM's aliases compare ST(0) with ST(i).
                    None => f.compare(b, a, false),
                }
                if op == 0xDE {
                    f.pop();
                    if reg == 3 {
                        f.pop();
                    }
                } else if reg == 3 {
                    f.pop();
                }
            }
            (0xD9, 0) => {
                let v = f.get(i);
                f.push(v);
            }
            (0xD9, 1) => {
                let (a, b) = (f.get(0), f.get(i));
                f.set(0, b);
                f.set(i, a);
            }
            (0xD9, 2) => {}
            (0xD9, 3) => {
                let v = f.get(0);
                f.set(i, v);
                f.pop();
            }
            (0xD9, _) => self.esc_d9(f, (reg << 3) | i),
            (0xDA, 5) if i == 1 => {
                let (a, b) = (f.get(0), f.get(1));
                f.compare(a, b, true);
                f.pop();
                f.pop();
            }
            (0xDB, 4) => match i {
                2 => f.sw &= !0x80FF,
                3 => *f = Fpu::new(),
                _ => {}
            },
            (0xDD, 0) => f.empty |= 1 << f.phys(i),
            (0xDD, 2 | 3) => {
                let v = f.get(0);
                f.set(i, v);
                if reg == 3 {
                    f.pop();
                }
            }
            (0xDD, 4 | 5) => {
                let (a, b) = (f.get(0), f.get(i));
                f.compare(a, b, true);
                if reg == 5 {
                    f.pop();
                }
            }
            (0xDF, 0) => {
                f.empty |= 1 << f.phys(i);
                f.pop();
            }
            (0xDF, 4) if i == 0 => {
                let sw = f.status();
                self.set_r16(AX, sw);
            }
            _ => {}
        }
    }

    /// D9 E0-FF: the one-operand and constant instructions.
    fn esc_d9(&mut self, f: &mut Fpu, n: u8) {
        use std::f64::consts;
        let range = |v: f64| v.abs() < 2f64.powi(63);
        match n {
            0x20 => {
                let v = f.get(0);
                f.set(0, -v);
            }
            0x21 => {
                let v = f.get(0);
                f.set(0, v.abs());
            }
            0x24 => {
                let v = f.get(0);
                f.compare(v, 0.0, false);
            }
            0x25 => {
                // FXAM
                let p = f.phys(0);
                let v = f.st[p];
                f.sw &= !(C0 | C1 | C2 | C3);
                if v.is_sign_negative() {
                    f.sw |= C1;
                }
                f.sw |= if f.empty & (1 << p) != 0 {
                    C3 | C0
                } else if v.is_nan() {
                    C0
                } else if v.is_infinite() {
                    C2 | C0
                } else if v == 0.0 {
                    C3
                } else if v.is_normal() {
                    C2
                } else {
                    C3 | C2
                };
            }
            0x28..=0x2E => f.push(match n {
                0x28 => 1.0,
                0x29 => consts::LOG2_10,
                0x2A => consts::LOG2_E,
                0x2B => consts::PI,
                0x2C => consts::LOG10_2,
                0x2D => consts::LN_2,
                _ => 0.0,
            }),
            0x30 => {
                let v = f.get(0);
                f.set(0, (v * consts::LN_2).exp_m1());
            }
            0x31 | 0x39 => {
                let (x, y) = (f.get(0), f.get(1));
                let l = if n == 0x31 { x.log2() } else { x.ln_1p() / consts::LN_2 };
                if x == 0.0 && n == 0x31 {
                    f.sw |= ZE;
                }
                f.set(1, y * l);
                f.pop();
            }
            0x32 => {
                let v = f.get(0);
                if range(v) {
                    f.sw &= !C2;
                    f.set(0, v.tan());
                    f.push(1.0);
                } else {
                    f.sw |= C2;
                }
            }
            0x33 => {
                let (x, y) = (f.get(0), f.get(1));
                f.set(1, y.atan2(x));
                f.pop();
            }
            0x34 => {
                // FXTRACT
                let v = f.get(0);
                if v == 0.0 {
                    f.sw |= ZE;
                    f.set(0, f64::NEG_INFINITY);
                    f.push(v);
                } else if !v.is_finite() {
                    f.push(v);
                } else {
                    let e = to_ext(v);
                    let exp = (u16::from_le_bytes([e[8], e[9]]) & 0x7FFF) as i32 - 16383;
                    f.set(0, exp as f64);
                    f.push(ldexp(v, -exp));
                }
            }
            0x35 | 0x38 => {
                // FPREM1 and FPREM: the whole reduction at once.
                let (a, b) = (f.get(0), f.get(1));
                if b == 0.0 || a.is_infinite() || a.is_nan() || b.is_nan() {
                    f.sw |= IE;
                    f.set(0, f64::NAN);
                    return;
                }
                let mut r = a % b;
                let mut q = ((a - r) / b).abs();
                if n == 0x35 {
                    let (hr, hb) = (2.0 * r.abs(), b.abs());
                    if hr > hb || (hr == hb && q % 2.0 == 1.0) {
                        r -= hb.copysign(r);
                        q += 1.0;
                    }
                }
                let q = (q % 8.0) as u8;
                f.sw &= !(C0 | C1 | C2 | C3);
                if q & 1 != 0 {
                    f.sw |= C1;
                }
                if q & 2 != 0 {
                    f.sw |= C3;
                }
                if q & 4 != 0 {
                    f.sw |= C0;
                }
                f.set(0, r);
            }
            0x36 => f.top = (f.top + 7) & 7,
            0x37 => f.top = (f.top + 1) & 7,
            0x3A => {
                let v = f.get(0);
                if v < 0.0 {
                    f.sw |= IE;
                }
                f.set(0, v.sqrt());
            }
            0x3B => {
                let v = f.get(0);
                if range(v) {
                    f.sw &= !C2;
                    f.set(0, v.sin());
                    f.push(v.cos());
                } else {
                    f.sw |= C2;
                }
            }
            0x3C => {
                let v = f.get(0);
                let r = f.round(v);
                f.set(0, r);
            }
            0x3D => {
                let (v, s) = (f.get(0), f.get(1));
                let s = s.trunc().clamp(-1e5, 1e5) as i32;
                f.set(0, ldexp(v, s));
            }
            0x3E | 0x3F => {
                let v = f.get(0);
                if range(v) {
                    f.sw &= !C2;
                    f.set(0, if n == 0x3E { v.sin() } else { v.cos() });
                } else {
                    f.sw |= C2;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_reals() {
        for v in [0.0, 1.0, -2.5, 1e300, -1e-300, 5e-324, 123456.789, f64::INFINITY] {
            assert_eq!(from_ext(&to_ext(v)), v, "{v}");
        }
        // 1.0 is 3FFF 8000000000000000.
        assert_eq!(to_ext(1.0), [0, 0, 0, 0, 0, 0, 0, 0x80, 0xFF, 0x3F]);
        assert!(from_ext(&to_ext(f64::NAN)).is_nan());
    }
}
