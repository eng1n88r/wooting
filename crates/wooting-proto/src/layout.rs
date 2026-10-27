//! Physical key layouts: which matrix cells a key owns and where it sits.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Key {
    /// Stable identifier used by the API and CLI (`"esc"`, `"space"`, …).
    pub id: String,
    pub label: String,
    /// Matrix `[row, col]` cells this key lights; primary cell first.
    pub cells: Vec<[u8; 2]>,
    /// Top-left corner and size in key units.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    #[serde(default = "one")]
    pub h: f32,
}

fn one() -> f32 {
    1.0
}

impl Key {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Centre of the `i`-th cell, spreading cells evenly across the key.
    pub fn cell_center(&self, i: usize) -> (f32, f32) {
        let n = self.cells.len() as f32;
        (self.x + self.w * (i as f32 + 0.5) / n, self.y + self.h / 2.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    /// Matrix columns in use.
    pub cols: u8,
    /// Overall size in key units.
    pub width: f32,
    pub height: f32,
    pub keys: Vec<Key>,
}

pub const LAYOUT_60HE_TOML: &str = include_str!("../layouts/60he.toml");

impl Layout {
    pub fn parse(toml_src: &str) -> Result<Layout, toml::de::Error> {
        toml::from_str(toml_src)
    }

    pub fn sixty_he() -> Layout {
        Layout::parse(LAYOUT_60HE_TOML).expect("bundled 60HE layout is valid")
    }

    /// Best layout for a model name; falls back to the 60HE.
    pub fn for_model(model: &str) -> Layout {
        let _ = model;
        Layout::sixty_he()
    }

    pub fn key(&self, id: &str) -> Option<&Key> {
        let id = id.to_ascii_lowercase();
        self.keys.iter().find(|k| k.id == id)
    }

    pub fn key_at_cell(&self, row: u8, col: u8) -> Option<&Key> {
        self.keys.iter().find(|k| k.cells.contains(&[row, col]))
    }

    pub fn key_ids(&self) -> Vec<&str> {
        self.keys.iter().map(|k| k.id.as_str()).collect()
    }

    /// Factory mapping of a layer (what the keyboard uses when a profile
    /// stores no mapping). Captured from a 60HE+.
    pub fn default_mapping(&self, layer: u8) -> [[u8; crate::COLS]; crate::ROWS] {
        let src: &[[u8; 14]; 6] = match layer {
            0 => &DEFAULT_LAYER0_60HE,
            1 => &DEFAULT_LAYER1_60HE,
            2 => &DEFAULT_LAYER2_60HE,
            _ => &[[0xff; 14]; 6],
        };
        let mut out = [[0xffu8; crate::COLS]; crate::ROWS];
        for (r, row) in src.iter().enumerate() {
            out[r][..14].copy_from_slice(row);
        }
        out
    }
}

pub const DEFAULT_LAYER0_60HE: [[u8; 14]; 6] = [
    [0xff; 14],
    [0x26, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x2a, 0x2b, 0x27],
    [0x28, 0x11, 0x17, 0x05, 0x12, 0x14, 0x19, 0x15, 0x09, 0x0f, 0x10, 0x2c, 0x2d, 0x2e],
    [0x36, 0x01, 0x13, 0x04, 0x06, 0x07, 0x08, 0x0a, 0x0b, 0x0c, 0x30, 0x31, 0x2e, 0x25],
    [0x64, 0x61, 0x1a, 0x18, 0x03, 0x16, 0x02, 0x0e, 0x0d, 0x33, 0x34, 0x35, 0xff, 0x68],
    [0x63, 0x66, 0x65, 0xff, 0x29, 0xff, 0x29, 0xff, 0x29, 0xff, 0x69, 0x77, 0x67, 0xe7],
];

pub const DEFAULT_LAYER1_60HE: [[u8; 14]; 6] = [
    [0xff; 14],
    [0x32, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x49],
    [0x43, 0xff, 0x48, 0xff, 0x5d, 0x5e, 0x5f, 0x60, 0xff, 0xff, 0xe2, 0xe3, 0xe4, 0xff],
    [0xea, 0x47, 0x4b, 0x4a, 0x5a, 0x5b, 0x5c, 0xc1, 0xc2, 0xc0, 0xe1, 0xe0, 0xff, 0xe6],
    [0x64, 0xff, 0xff, 0xff, 0xff, 0x57, 0x58, 0x59, 0xc3, 0xc5, 0xc4, 0x4f, 0xff, 0x68],
    [0x63, 0x66, 0x65, 0xff, 0x29, 0x29, 0x29, 0x29, 0x29, 0xff, 0x4d, 0x4e, 0x4c, 0xe7],
];

pub const DEFAULT_LAYER2_60HE: [[u8; 14]; 6] = [
    [0xff; 14],
    [0xff; 14],
    [0xff, 0xff, 0xff, 0xff, 0x05, 0x10, 0x09, 0x03, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
    [0xea, 0xff, 0xff, 0xff, 0x07, 0x01, 0x0d, 0x05, 0x12, 0xff, 0xff, 0xff, 0xff, 0xff],
    [0xff, 0xff, 0xff, 0xff, 0xff, 0x0d, 0x0f, 0x16, 0x05, 0xff, 0xff, 0xff, 0xff, 0xff],
    [0x4c, 0xe8, 0x4d, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe7],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_he_layout_is_consistent() {
        let l = Layout::sixty_he();
        assert_eq!(l.keys.len(), 61);
        let mut seen = std::collections::HashSet::new();
        for k in &l.keys {
            assert!(seen.insert(k.id.clone()), "duplicate id {}", k.id);
            for c in &k.cells {
                assert!(c[0] < 6 && c[1] < l.cols, "{} cell {:?} out of range", k.id, c);
                assert_eq!(l.key_at_cell(c[0], c[1]).map(|k| &k.id), Some(&k.id));
            }
            assert!(k.x + k.w <= l.width + 1e-3, "{} overflows", k.id);
        }
        assert_eq!(l.key("space").unwrap().cells.len(), 3);
    }
}
