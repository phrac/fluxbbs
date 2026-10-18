//! An 8250 UART for COM1 (3F8h, IRQ4), for doors that drive the port
//! themselves rather than through the FOSSIL. Received bytes come from
//! the host's input queue; sent bytes go to its output. Carrier detect
//! follows the host's carrier flag.

use std::collections::VecDeque;

pub const COM1_BASE: u16 = 0x3F8;
pub const COM1_IRQ: u8 = 4;

#[derive(Default)]
pub struct Uart {
    ier: u8,
    lcr: u8,
    mcr: u8,
    scratch: u8,
    divisor: u16,
    /// Interrupt conditions raised but not yet acknowledged: bit 0 data
    /// available, bit 1 transmitter empty.
    pending: u8,
    thr_empty_pending: bool,
}

impl Uart {
    pub fn new() -> Self {
        Self { divisor: 12, ..Self::default() }
    }

    /// Is an interrupt wanted (IER allows one of the pending causes)?
    pub fn irq(&self) -> bool {
        (self.pending & self.ier & 0x03) != 0
    }

    /// New data has arrived (the caller queued it).
    pub fn data_arrived(&mut self) {
        self.pending |= 0x01;
    }

    pub fn dtr(&self) -> bool {
        self.mcr & 1 != 0
    }

    pub fn read(&mut self, port: u16, input: &mut VecDeque<u8>, carrier: bool) -> u8 {
        let dlab = self.lcr & 0x80 != 0;
        match port - COM1_BASE {
            0 if dlab => self.divisor as u8,
            0 => {
                let b = input.pop_front().unwrap_or(0);
                if input.is_empty() {
                    self.pending &= !0x01;
                }
                b
            }
            1 if dlab => (self.divisor >> 8) as u8,
            1 => self.ier,
            2 => {
                // IIR: highest pending enabled cause.
                if self.pending & self.ier & 0x01 != 0 {
                    0x04
                } else if self.pending & self.ier & 0x02 != 0 {
                    self.pending &= !0x02;
                    0x02
                } else {
                    0x01
                }
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => {
                // LSR: data ready, transmitter always empty.
                let mut v = 0x60;
                if !input.is_empty() {
                    v |= 0x01;
                }
                v
            }
            6 => {
                // MSR: CTS, DSR, and DCD when there's a caller.
                let mut v = 0x30;
                if carrier {
                    v |= 0x80;
                }
                v
            }
            7 => self.scratch,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, port: u16, v: u8, output: &mut Vec<u8>) {
        let dlab = self.lcr & 0x80 != 0;
        match port - COM1_BASE {
            0 if dlab => self.divisor = (self.divisor & 0xFF00) | v as u16,
            0 => {
                output.push(v);
                // The transmitter is empty again straight away.
                if self.ier & 0x02 != 0 {
                    self.pending |= 0x02;
                }
            }
            1 if dlab => self.divisor = (self.divisor & 0x00FF) | (v as u16) << 8,
            1 => {
                let was = self.ier;
                self.ier = v & 0x0F;
                // Enabling THRE raises it at once, as the chip does.
                if v & 0x02 != 0 && was & 0x02 == 0 {
                    self.pending |= 0x02;
                }
            }
            3 => self.lcr = v,
            4 => self.mcr = v,
            7 => self.scratch = v,
            _ => {}
        }
        let _ = self.thr_empty_pending;
    }
}
