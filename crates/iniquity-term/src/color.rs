//! DOS text attributes and the user colour scheme (`tColorRec` / `tColor`).

use serde::{Deserialize, Serialize};

/// DOS colour index (0-7) -> ANSI SGR colour digit.
const ANSI_DIGIT: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// A DOS text attribute: foreground 0-15, background 0-7, blink.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub fore: u8,
    pub back: u8,
    #[serde(default)]
    pub blink: bool,
}

impl Color {
    pub const fn new(fore: u8, back: u8) -> Self {
        Self { fore: fore & 15, back: back & 7, blink: false }
    }

    /// The attribute a terminal is in after `ESC[0m`.
    pub const DEFAULT: Color = Color::new(7, 0);

    /// SGR sequence that moves a terminal from `self` to `to`, or empty if
    /// nothing changes. Same output shape as Iniquity's `oAnsiCode`, except
    /// that turning blink *off* also resets (the original only reset when
    /// turning it on, which left blink stuck).
    pub fn transition(self, to: Color) -> String {
        let mut fore = self.fore != to.fore;
        let mut back = self.back != to.back;
        let mut blink = self.blink != to.blink;
        if !(fore || back || blink) {
            return String::new();
        }
        let mut parts: Vec<String> = Vec::new();
        let mut from_bright = self.fore >= 8;
        if (blink && !to.blink) || (fore && self.fore >= 8 && to.fore < 8) {
            parts.push("0".into());
            from_bright = false;
            blink = to.blink;
            back = to.back != 0;
            fore = to.fore != 7;
        }
        if fore {
            if !from_bright && to.fore >= 8 {
                parts.push("1".into());
            }
            parts.push(format!("3{}", ANSI_DIGIT[(to.fore & 7) as usize]));
        }
        if back {
            parts.push(format!("4{}", ANSI_DIGIT[(to.back & 7) as usize]));
        }
        if blink {
            parts.push("5".into());
        }
        if parts.is_empty() {
            // Only happens when resetting lands exactly on the default.
            parts.push("0".into());
        }
        format!("\x1b[{}m", parts.join(";"))
    }

    /// Apply an SGR parameter list (as received in an ANSI file) to this
    /// attribute, so the cursor tracker knows the current colour.
    pub fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            *self = Color::DEFAULT;
            return;
        }
        for &p in params {
            match p {
                0 => *self = Color::DEFAULT,
                1 => self.fore |= 8,
                22 => self.fore &= 7,
                5 | 6 => self.blink = true,
                25 => self.blink = false,
                30..=37 => self.fore = (self.fore & 8) | dos_index(p - 30),
                // aixterm bright foreground (modern editors save these).
                90..=97 => self.fore = 8 | dos_index(p - 90),
                39 => self.fore = (self.fore & 8) | 7,
                40..=47 => self.back = dos_index(p - 40),
                49 => self.back = 0,
                _ => {}
            }
        }
    }
}

fn dos_index(ansi: u16) -> u8 {
    ANSI_DIGIT.iter().position(|&d| d as u16 == ansi).unwrap_or(7) as u8
}

/// Slots in a colour scheme, matching the `col*` constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Slot {
    Error = 0,
    Text,
    TextLo,
    TextHi,
    Info,
    InfoLo,
    InfoHi,
    Item,
    ItemSel,
    Border,
    Edit,
}

/// A user's colour scheme (`tColor`, 0..maxColor).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette(pub [Color; 11]);

impl Palette {
    pub fn get(&self, slot: Slot) -> Color {
        self.0[slot as usize]
    }

    pub fn index(&self, i: u8) -> Option<Color> {
        self.0.get(i as usize).copied()
    }
}

impl Default for Palette {
    /// Provisional defaults: the original `Cfg^.DefaultCol` values lived in
    /// the shipped iniquity.dat, which isn't part of the source archive.
    fn default() -> Self {
        Palette([
            Color::new(12, 0), // error
            Color::new(7, 0),  // text
            Color::new(8, 0),  // text lo
            Color::new(15, 0), // text hi
            Color::new(11, 0), // info
            Color::new(3, 0),  // info lo
            Color::new(15, 0), // info hi
            Color::new(9, 0),  // item
            Color::new(15, 1), // item selected
            Color::new(8, 0),  // border
            Color::new(15, 1), // edit
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_change_is_empty() {
        assert_eq!(Color::DEFAULT.transition(Color::DEFAULT), "");
    }

    #[test]
    fn dos_order_maps_to_ansi() {
        // DOS 1 is blue, ANSI 34.
        assert_eq!(Color::DEFAULT.transition(Color::new(1, 0)), "\x1b[34m");
        assert_eq!(Color::DEFAULT.transition(Color::new(12, 0)), "\x1b[1;31m");
        assert_eq!(Color::new(12, 0).transition(Color::new(14, 0)), "\x1b[33m");
    }

    #[test]
    fn bright_to_dim_resets() {
        assert_eq!(Color::new(15, 1).transition(Color::new(3, 1)), "\x1b[0;36;44m");
        assert_eq!(Color::new(15, 0).transition(Color::DEFAULT), "\x1b[0m");
    }

    #[test]
    fn blink_off_resets() {
        let on = Color { fore: 7, back: 0, blink: true };
        assert_eq!(on.transition(Color::DEFAULT), "\x1b[0m");
        assert_eq!(Color::DEFAULT.transition(on), "\x1b[5m");
    }

    #[test]
    fn sgr_tracking() {
        let mut c = Color::DEFAULT;
        c.apply_sgr(&[1, 34, 41]);
        assert_eq!(c, Color::new(9, 4));
        c.apply_sgr(&[0]);
        assert_eq!(c, Color::DEFAULT);
    }
}
