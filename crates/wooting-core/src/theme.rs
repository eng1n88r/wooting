//! Map an Omarchy theme to a keyboard colour frame.
//!
//! Omarchy keeps the active theme at `~/.local/state/omarchy/current/theme/`
//! with a `colors.toml` (accent, background, red, green, …) and optionally a
//! `keyboard.rgb` holding one hex colour.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use wooting_proto::{Frame, Layout, Rgb};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColors {
    pub base: Rgb,
    pub modifiers: Rgb,
    pub escape: Rgb,
    pub enter: Rgb,
    pub space: Rgb,
    pub fn_key: Rgb,
}

/// Keys that count as modifiers for theming.
const MODIFIERS: &[&str] = &[
    "lshift",
    "rshift",
    "lctrl",
    "rctrl",
    "lalt",
    "ralt",
    "lgui",
    "capslock",
    "tab",
    "backspace",
    "menu",
];

pub fn default_theme_dir() -> Option<std::path::PathBuf> {
    dirs::state_dir().map(|d| d.join("omarchy/current/theme"))
}

/// Read `colors.toml` (and `keyboard.rgb` if present) from a theme dir.
pub fn load_theme(dir: &Path) -> Result<ThemeColors> {
    let colors_path = dir.join("colors.toml");
    let text = std::fs::read_to_string(&colors_path)
        .map_err(|e| Error::Other(format!("{}: {e}", colors_path.display())))?;
    let table: HashMap<String, toml::Value> =
        toml::from_str(&text).map_err(|e| Error::Other(format!("colors.toml: {e}")))?;
    let get = |k: &str| -> Option<Rgb> {
        table
            .get(k)
            .and_then(|v| v.as_str())
            .and_then(Rgb::parse_hex)
    };
    let keyboard_rgb = std::fs::read_to_string(dir.join("keyboard.rgb"))
        .ok()
        .and_then(|s| Rgb::parse_hex(s.trim()));
    let accent = get("accent").ok_or_else(|| Error::Other("colors.toml has no accent".into()))?;
    Ok(ThemeColors {
        base: keyboard_rgb.unwrap_or(accent),
        modifiers: get("muted")
            .or_else(|| get("lighter_background"))
            .unwrap_or(dim(accent, 0.35)),
        escape: get("red").unwrap_or(accent),
        enter: get("green").unwrap_or(accent),
        space: accent,
        fn_key: get("yellow").or_else(|| get("orange")).unwrap_or(accent),
    })
}

fn dim(c: Rgb, f: f32) -> Rgb {
    let s = |v: u8| (v as f32 * f).round() as u8;
    Rgb::new(s(c.r), s(c.g), s(c.b))
}

pub fn frame_from_theme(theme: &ThemeColors, layout: &Layout) -> Frame {
    let mut frame = Frame::default();
    for key in &layout.keys {
        let colour = match key.id.as_str() {
            "esc" => theme.escape,
            "enter" => theme.enter,
            "space" => theme.space,
            "fn" => theme.fn_key,
            id if MODIFIERS.contains(&id) => theme.modifiers,
            _ => theme.base,
        };
        for c in &key.cells {
            frame.set(c[0] as usize, c[1] as usize, colour);
        }
    }
    frame
}
