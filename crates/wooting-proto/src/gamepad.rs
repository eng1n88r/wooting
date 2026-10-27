//! Gamepad button codes and analog-curve presets (Wootility v5.4, Xbox
//! controller layout). A mapped key is stored as `(code << 8) | matrix_id`
//! in the `GamepadMapping` report.

use crate::pb;

/// `(name, label, code)` for every Xbox-controller button Wootility offers.
pub const BUTTONS: &[(&str, &str, u8)] = &[
    ("a", "A", 0x78),
    ("b", "B", 0x79),
    ("x", "X", 0x7A),
    ("y", "Y", 0x7B),
    ("start", "Start", 0x7C),
    ("select", "Select", 0x7D),
    ("dpad_up", "D-pad ↑", 0x7E),
    ("dpad_down", "D-pad ↓", 0x7F),
    ("dpad_left", "D-pad ←", 0x80),
    ("dpad_right", "D-pad →", 0x81),
    ("lb", "LB", 0x82),
    ("rb", "RB", 0x83),
    ("guide", "Guide", 0x84),
    ("ls_up", "LS ↑", 0x85),
    ("ls_down", "LS ↓", 0x86),
    ("ls_left", "LS ←", 0x87),
    ("ls_right", "LS →", 0x88),
    ("rs_up", "RS ↑", 0x89),
    ("rs_down", "RS ↓", 0x8A),
    ("rs_left", "RS ←", 0x8B),
    ("rs_right", "RS →", 0x8C),
    ("ls_press", "LS press", 0x8D),
    ("rs_press", "RS press", 0x8E),
    ("lt", "LT", 0x8F),
    ("rt", "RT", 0x90),
];

pub fn name(code: u8) -> String {
    BUTTONS
        .iter()
        .find(|(_, _, c)| *c == code)
        .map(|(n, _, _)| n.to_string())
        .unwrap_or_else(|| format!("0x{code:02x}"))
}

pub fn label(code: u8) -> String {
    BUTTONS
        .iter()
        .find(|(_, _, c)| *c == code)
        .map(|(_, l, _)| l.to_string())
        .unwrap_or_else(|| format!("{code:02X}"))
}

/// Accepts a button name, label (case-insensitive) or `0x..` code.
pub fn parse(s: &str) -> Option<u8> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        return u8::from_str_radix(hex, 16).ok();
    }
    BUTTONS
        .iter()
        .find(|(n, l, _)| *n == lower || l.eq_ignore_ascii_case(t) || n.replace('_', "-") == lower)
        .map(|(_, _, c)| *c)
}

/// Curve presets as 4 `(x, y)` points, exactly as Wootility writes them.
pub const CURVE_PRESETS: &[(&str, [(u32, u32); 4])] = &[
    ("linear", [(0, 20), (60, 60), (194, 195), (253, 255)]),
    ("aggro", [(0, 33), (63, 114), (139, 231), (253, 255)]),
    ("slow", [(0, 23), (128, 63), (226, 145), (253, 255)]),
    ("smooth", [(0, 26), (78, 84), (199, 174), (253, 255)]),
    ("steep", [(0, 26), (25, 132), (192, 135), (253, 255)]),
    ("instant", [(0, 255), (2, 255), (192, 255), (255, 255)]),
    ("turbo", [(0, 255), (1, 255), (2, 255), (3, 255)]),
];

pub fn preset(name: &str) -> Option<[(u32, u32); 4]> {
    CURVE_PRESETS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
        .map(|(_, p)| *p)
}

/// Name of the preset matching `points`, if any.
pub fn preset_name(points: &[(u32, u32)]) -> Option<&'static str> {
    CURVE_PRESETS
        .iter()
        .find(|(_, p)| p.as_slice() == points)
        .map(|(n, _)| *n)
}

/// The factory gamepad profile: linear curve, angle off, square off, snappy on, 45°.
pub fn default_profile() -> pb::GamepadProfile {
    let mut g = pb::GamepadProfile::default();
    set_curve(&mut g, &preset("linear").unwrap());
    g.settings = Some(pb::gamepad_profile::Settings {
        angle_enabled: Some(0),
        square: Some(0),
        snappy: Some(1),
        angle: Some(50),
    });
    g
}

pub fn set_curve(g: &mut pb::GamepadProfile, points: &[(u32, u32)]) {
    g.curve = Some(pb::gamepad_profile::Curve {
        points: points
            .iter()
            .map(|(x, y)| pb::gamepad_profile::Point {
                x: Some(*x),
                y: Some(*y),
            })
            .collect(),
    });
}

pub fn curve_points(g: &pb::GamepadProfile) -> Vec<(u32, u32)> {
    g.curve
        .as_ref()
        .map(|c| {
            c.points
                .iter()
                .map(|p| (p.x.unwrap_or(0), p.y.unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn default_profile_matches_wootility() {
        let want = [
            0x0a, 0x1c, 0x0a, 0x04, 0x08, 0x00, 0x10, 0x14, 0x0a, 0x04, 0x08, 0x3c, 0x10, 0x3c,
            0x0a, 0x06, 0x08, 0xc2, 0x01, 0x10, 0xc3, 0x01, 0x0a, 0x06, 0x08, 0xfd, 0x01, 0x10,
            0xff, 0x01, 0x12, 0x08, 0x08, 0x00, 0x10, 0x00, 0x18, 0x01, 0x20, 0x32,
        ];
        assert_eq!(default_profile().encode_to_vec(), want);
        let back = pb::GamepadProfile::decode(&want[..]).unwrap();
        assert_eq!(preset_name(&curve_points(&back)), Some("linear"));
    }

    #[test]
    fn mapping_matches_wootility() {
        // Button A (0x78) on Z (matrix 0x82), override off.
        let m = pb::GamepadMapping {
            layer: Some(pb::gamepad_mapping::Layer {
                keys: Some(pb::gamepad_mapping::Keys {
                    key: vec![(0x78 << 8) | 0x82],
                }),
            }),
            override_keys: Some(0),
        };
        assert_eq!(
            m.encode_to_vec(),
            [0x0a, 0x06, 0x12, 0x04, 0x08, 0x82, 0xf1, 0x01, 0x10, 0x00]
        );
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(parse("A"), Some(0x78));
        assert_eq!(parse("ls-up"), Some(0x85));
        assert_eq!(parse("RS press"), Some(0x8E));
        assert_eq!(name(0x90), "rt");
        assert_eq!(label(0x7E), "D-pad ↑");
    }
}
