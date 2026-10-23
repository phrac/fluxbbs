//! Little-endian Turbo Pascal record fields.

use fluxbbs_term::{Color, cp437};

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        s
    }

    pub fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    pub fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }

    pub fn bool(&mut self) -> bool {
        self.u8() != 0
    }

    pub fn char(&mut self) -> char {
        cp437::TO_UNICODE[self.u8() as usize]
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().unwrap())
    }

    pub fn i16(&mut self) -> i16 {
        i16::from_le_bytes(self.take(2).try_into().unwrap())
    }

    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }

    pub fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take(4).try_into().unwrap())
    }

    /// `String[max]`: a length byte then `max` bytes of storage.
    pub fn string(&mut self, max: usize) -> String {
        let raw = self.take(max + 1);
        let len = (raw[0] as usize).min(max);
        cp437::decode(&raw[1..=len])
    }

    /// `tColorRec`.
    pub fn color(&mut self) -> Color {
        let fore = self.u8();
        let back = self.u8();
        let blink = self.bool();
        Color { fore: fore & 15, back: back & 7, blink }
    }

    /// A Pascal `set of` with `bytes` of storage; returns the member
    /// ordinals relative to the first stored byte's bit 0.
    pub fn set_bits(&mut self, bytes: usize) -> Vec<u32> {
        let raw = self.take(bytes);
        (0..bytes as u32 * 8).filter(|&i| raw[(i / 8) as usize] & (1 << (i % 8)) != 0).collect()
    }
}

pub(crate) fn check(what: &'static str, buf: &[u8], expected: usize) -> Result<(), crate::Error> {
    if buf.len() < expected {
        return Err(crate::Error::Size { what, expected, got: buf.len() });
    }
    Ok(())
}
