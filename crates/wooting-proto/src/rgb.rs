//! LED matrix colours and the raw-colour output report.

use serde::{Deserialize, Serialize};

/// LED matrix rows shared by every Wooting keyboard.
pub const ROWS: usize = 6;
/// LED matrix columns of the widest keyboard. Smaller boards use a prefix.
pub const COLS: usize = 21;

/// Report type byte of the raw-colour frame (`WOOTING_RAW_COLORS_REPORT`).
pub const RAW_COLORS_REPORT: u8 = 11;
/// Report id + length of the "v3" raw-colour frame used on ARM boards.
pub const V3_REPORT_ID: u8 = 5;
pub const V3_REPORT_LEN: usize = 2047;
/// Offset of the colour grid inside the v3 frame.
pub const V3_DATA_OFFSET: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    /// RGB565 as sent on the wire: `rrrrrggg gggbbbbb`.
    pub fn to_565(self) -> u16 {
        ((self.r as u16 & 0xf8) << 8)
            | ((self.g as u16 & 0xfc) << 3)
            | ((self.b as u16 & 0xf8) >> 3)
    }

    pub fn from_565(v: u16) -> Rgb {
        Rgb {
            r: ((v >> 8) & 0xf8) as u8,
            g: ((v >> 3) & 0xfc) as u8,
            b: ((v << 3) & 0xf8) as u8,
        }
    }

    /// Parse `#rrggbb`, `rrggbb`, or `#rgb`.
    pub fn parse_hex(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        match s.len() {
            6 => {
                let v = u32::from_str_radix(s, 16).ok()?;
                Some(Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
            }
            3 => {
                let v = u16::from_str_radix(s, 16).ok()?;
                let e = |n: u16| ((n & 0xf) * 17) as u8;
                Some(Rgb::new(e(v >> 8), e(v >> 4), e(v)))
            }
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl std::str::FromStr for Rgb {
    type Err = String;
    fn from_str(s: &str) -> Result<Rgb, String> {
        Rgb::parse_hex(s).ok_or_else(|| format!("invalid colour {s:?}, expected #rrggbb"))
    }
}

/// `KeyboardMatrixID` from the SDK: 5 column bits, then 3 row bits.
pub fn matrix_id(row: u8, col: u8) -> u8 {
    (row << 5) | (col & 0x1f)
}

/// A full-keyboard colour frame, `[row][col]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame(pub [[Rgb; COLS]; ROWS]);

impl Default for Frame {
    fn default() -> Self {
        Frame([[Rgb::BLACK; COLS]; ROWS])
    }
}

impl Frame {
    pub fn filled(c: Rgb) -> Frame {
        Frame([[c; COLS]; ROWS])
    }

    pub fn get(&self, row: usize, col: usize) -> Option<Rgb> {
        self.0.get(row).and_then(|r| r.get(col)).copied()
    }

    pub fn set(&mut self, row: usize, col: usize, c: Rgb) -> bool {
        match self.0.get_mut(row).and_then(|r| r.get_mut(col)) {
            Some(slot) => {
                *slot = c;
                true
            }
            None => false,
        }
    }

    /// The "v3" raw-colour output report used by multi-report (0xFF55)
    /// devices: `[5, 0xD1, 0xDA, 11, u16le[6][21] ...]` padded to 2047 bytes.
    pub fn to_v3_report(&self) -> Vec<u8> {
        let mut out = vec![0u8; V3_REPORT_LEN];
        out[0] = V3_REPORT_ID;
        out[1] = 0xD1;
        out[2] = 0xDA;
        out[3] = RAW_COLORS_REPORT;
        let mut i = V3_DATA_OFFSET;
        for row in &self.0 {
            for c in row {
                out[i..i + 2].copy_from_slice(&c.to_565().to_le_bytes());
                i += 2;
            }
        }
        out
    }

    /// Parameters for `WootDevSingleColor` (command 30) on v2/v3 boards.
    pub fn single_color_params(row: u8, col: u8, c: Rgb) -> [u8; 4] {
        [matrix_id(row, col), c.r, c.g, c.b]
    }

    /// Parameters for `WootDevResetColor` (command 31) on v2/v3 boards.
    pub fn single_reset_params(row: u8, col: u8) -> [u8; 4] {
        [0, 0, 0, matrix_id(row, col)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_roundtrip() {
        let c = Rgb::new(0xff, 0x80, 0x08);
        assert_eq!(c.to_565(), 0xfc01);
        assert_eq!(Rgb::from_565(c.to_565()), Rgb::new(0xf8, 0x80, 0x08));
    }

    #[test]
    fn hex_parse() {
        assert_eq!(Rgb::parse_hex("#7aa2f7"), Some(Rgb::new(0x7a, 0xa2, 0xf7)));
        assert_eq!(Rgb::parse_hex("f0f"), Some(Rgb::new(0xff, 0, 0xff)));
        assert_eq!(Rgb::parse_hex("zz"), None);
    }

    #[test]
    fn v3_report_layout() {
        let mut f = Frame::default();
        f.set(1, 2, Rgb::new(0xff, 0, 0));
        let r = f.to_v3_report();
        assert_eq!(r.len(), V3_REPORT_LEN);
        assert_eq!(&r[..4], &[5, 0xD1, 0xDA, 11]);
        let off = V3_DATA_OFFSET + (COLS + 2) * 2;
        assert_eq!(&r[off..off + 2], &0xf800u16.to_le_bytes());
    }

    #[test]
    fn matrix_id_bits() {
        assert_eq!(matrix_id(5, 13), 0b101_01101);
    }
}
