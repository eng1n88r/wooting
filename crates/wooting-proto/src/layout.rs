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
}

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
