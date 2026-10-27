//! Wooting key codes as used in mapping profiles.
//!
//! For ordinary keys the code is the USB HID usage minus 3 (A = 0x04 → 1,
//! F1 = 0x3A → 0x37); the keypad block is HID minus 2 (Num 1 = 0x57).
//! Modifiers live at 0x63..=0x6A. Media, RGB and profile keys are
//! Wooting-specific; they were identified from Wootility's Fn-layer view
//! of a 60HE+. Unknown codes are kept numeric.

/// (code, name). Names are lowercase and stable for the API.
pub const KEYCODES: &[(u8, &str)] = &[
    (0x01, "a"),
    (0x02, "b"),
    (0x03, "c"),
    (0x04, "d"),
    (0x05, "e"),
    (0x06, "f"),
    (0x07, "g"),
    (0x08, "h"),
    (0x09, "i"),
    (0x0a, "j"),
    (0x0b, "k"),
    (0x0c, "l"),
    (0x0d, "m"),
    (0x0e, "n"),
    (0x0f, "o"),
    (0x10, "p"),
    (0x11, "q"),
    (0x12, "r"),
    (0x13, "s"),
    (0x14, "t"),
    (0x15, "u"),
    (0x16, "v"),
    (0x17, "w"),
    (0x18, "x"),
    (0x19, "y"),
    (0x1a, "z"),
    (0x1b, "1"),
    (0x1c, "2"),
    (0x1d, "3"),
    (0x1e, "4"),
    (0x1f, "5"),
    (0x20, "6"),
    (0x21, "7"),
    (0x22, "8"),
    (0x23, "9"),
    (0x24, "0"),
    (0x25, "enter"),
    (0x26, "esc"),
    (0x27, "backspace"),
    (0x28, "tab"),
    (0x29, "space"),
    (0x2a, "minus"),
    (0x2b, "equal"),
    (0x2c, "lbracket"),
    (0x2d, "rbracket"),
    (0x2e, "backslash"),
    (0x2f, "nonus_hash"),
    (0x30, "semicolon"),
    (0x31, "quote"),
    (0x32, "grave"),
    (0x33, "comma"),
    (0x34, "period"),
    (0x35, "slash"),
    (0x36, "capslock"),
    (0x37, "f1"),
    (0x38, "f2"),
    (0x39, "f3"),
    (0x3a, "f4"),
    (0x3b, "f5"),
    (0x3c, "f6"),
    (0x3d, "f7"),
    (0x3e, "f8"),
    (0x3f, "f9"),
    (0x40, "f10"),
    (0x41, "f11"),
    (0x42, "f12"),
    (0x43, "printscreen"),
    (0x44, "scrolllock"),
    (0x45, "pause"),
    (0x46, "insert"),
    (0x47, "home"),
    (0x48, "pageup"),
    (0x49, "delete"),
    (0x4a, "end"),
    (0x4b, "pagedown"),
    (0x4c, "right"),
    (0x4d, "left"),
    (0x4e, "down"),
    (0x4f, "up"),
    (0x51, "numlock"),
    (0x52, "kp_slash"),
    (0x53, "kp_asterisk"),
    (0x54, "kp_minus"),
    (0x55, "kp_plus"),
    (0x56, "kp_enter"),
    (0x57, "kp_1"),
    (0x58, "kp_2"),
    (0x59, "kp_3"),
    (0x5a, "kp_4"),
    (0x5b, "kp_5"),
    (0x5c, "kp_6"),
    (0x5d, "kp_7"),
    (0x5e, "kp_8"),
    (0x5f, "kp_9"),
    (0x60, "kp_0"),
    (0x61, "nonus_backslash"),
    (0x63, "lctrl"),
    (0x64, "lshift"),
    (0x65, "lalt"),
    (0x66, "lgui"),
    (0x67, "rctrl"),
    (0x68, "rshift"),
    (0x69, "ralt"),
    (0x6a, "rgui"),
    (0x6b, "f13"),
    (0x6c, "f14"),
    (0x6d, "f15"),
    (0x6e, "f16"),
    (0x6f, "f17"),
    (0x70, "f18"),
    (0x71, "f19"),
    (0x72, "f20"),
    (0x73, "f21"),
    (0x74, "f22"),
    (0x75, "f23"),
    (0x76, "f24"),
    (0x77, "menu"),
    (0xc0, "next_track"),
    (0xc1, "prev_track"),
    (0xc2, "play_pause"),
    (0xc3, "mute"),
    (0xc4, "volume_up"),
    (0xc5, "volume_down"),
    (0xc6, "media_player"),
    (0xc7, "mail"),
    (0xc8, "calculator"),
    (0xc9, "file_explorer"),
    (0xca, "search"),
    (0xcb, "www"),
    (0xcc, "browser_back"),
    (0xcd, "browser_forward"),
    (0xce, "stop"),
    (0xcf, "refresh"),
    (0xd0, "bookmark"),
    (0xd1, "power"),
    (0xd2, "sleep"),
    (0xd4, "screen_switch"),
    (0xd5, "app_grid"),
    (0xd6, "display_brightness_down"),
    (0xd7, "display_brightness_up"),
    (0xd8, "mouse_left"),
    (0xd9, "mouse_right"),
    (0xda, "mouse_middle"),
    (0xdb, "mouse_back"),
    (0xdc, "mouse_forward"),
    (0xe0, "rgb_brightness_up"),
    (0xe1, "rgb_brightness_down"),
    (0xe2, "profile_2"),
    (0xe3, "profile_3"),
    (0xe4, "profile_4"),
    (0xe5, "profile_1"),
    (0xe6, "rgb_mode"),
    (0xe7, "fn1"),
    (0xe8, "key_lock"),
    (0xea, "fn2"),
    (0xeb, "fn3"),
    (0xec, "profile_cycle"),
    (0xed, "ime_on"),
    (0xee, "ime_off"),
    (0xef, "lang3"),
    (0xf0, "lang4"),
    (0xf1, "lang5"),
    (0xf2, "ro"),
    (0xf3, "kana"),
    (0xf4, "yen"),
    (0xf5, "henkan"),
    (0xf6, "muhenkan"),
    (0xff, "none"),
];

pub const NONE: u8 = 0xff;

/// Short keycap-style label for a code (`"["`, `"Shift"`, `"F5"`, `"↑"`);
/// empty for an unmapped key.
pub fn label(code: u8) -> String {
    if code == NONE {
        return String::new();
    }
    let n = name(code);
    let fixed = match n.as_str() {
        "minus" => "-",
        "equal" => "=",
        "lbracket" => "[",
        "rbracket" => "]",
        "backslash" | "nonus_backslash" => "\\",
        "nonus_hash" => "#",
        "semicolon" => ";",
        "quote" => "'",
        "grave" => "`",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "esc" => "Esc",
        "enter" => "Enter",
        "backspace" => "Bksp",
        "tab" => "Tab",
        "space" => "Space",
        "capslock" => "Caps",
        "lshift" | "rshift" => "Shift",
        "lctrl" | "rctrl" => "Ctrl",
        "lalt" | "ralt" => "Alt",
        "lgui" | "rgui" => "Super",
        "menu" => "Menu",
        "fn1" => "Fn 1",
        "fn2" => "Fn 2",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "pageup" => "Pg Up",
        "pagedown" => "Pg Dn",
        "home" => "Home",
        "end" => "End",
        "insert" => "Ins",
        "delete" => "Del",
        "printscreen" => "Prt Sc",
        "scrolllock" => "Scr Lk",
        "pause" => "Pause",
        "numlock" => "Num Lk",
        "kp_enter" => "Num ⏎",
        "kp_slash" => "Num /",
        "kp_asterisk" => "Num *",
        "kp_minus" => "Num -",
        "kp_plus" => "Num +",
        "next_track" => "⏭\u{FE0E}",
        "prev_track" => "⏮\u{FE0E}",
        "play_pause" => "⏯\u{FE0E}",
        "mute" => "Mute",
        "volume_up" => "Vol +",
        "volume_down" => "Vol -",
        "rgb_brightness_up" => "RGB +",
        "rgb_brightness_down" => "RGB -",
        "rgb_mode" => "Mode",
        "profile_1" => "P1",
        "profile_2" => "P2",
        "profile_3" => "P3",
        "profile_4" => "P4",
        "profile_cycle" => "Cycle",
        "key_lock" => "Key Lock",
        "fn3" => "Fn 3",
        "media_player" => "Media",
        "mail" => "Mail",
        "calculator" => "Calc",
        "file_explorer" => "Files",
        "search" => "Search",
        "www" => "Web",
        "browser_back" => "Back",
        "browser_forward" => "Fwd",
        "stop" => "Stop",
        "refresh" => "Refresh",
        "bookmark" => "Bookmark",
        "power" => "Power",
        "sleep" => "Sleep",
        "screen_switch" => "Screen",
        "app_grid" => "Apps",
        "display_brightness_down" => "Bright -",
        "display_brightness_up" => "Bright +",
        "mouse_left" => "Mouse L",
        "mouse_right" => "Mouse R",
        "mouse_middle" => "Mouse M",
        "mouse_back" => "Mouse 4",
        "mouse_forward" => "Mouse 5",
        "ime_on" => "IME On",
        "ime_off" => "IME Off",
        "lang3" => "Lang 3",
        "lang4" => "Lang 4",
        "lang5" => "Lang 5",
        "ro" => "ろ/Ro",
        "kana" => "Kana",
        "yen" => "¥",
        "henkan" => "Conv",
        "muhenkan" => "Non-conv",
        "none" => "",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    if let Some(d) = n.strip_prefix("kp_") {
        return format!("Num {d}");
    }
    // Unidentified media/special codes and raw numbers: short hex.
    if n.starts_with("media_") || n.starts_with("special_") || n.starts_with("0x") {
        return format!("{code:02X}");
    }
    if n.len() == 1 || n.starts_with('f') && n[1..].chars().all(|c| c.is_ascii_digit()) {
        return n.to_ascii_uppercase();
    }
    n
}

pub fn name(code: u8) -> String {
    KEYCODES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("0x{code:02x}"))
}

/// Accepts a name from [`KEYCODES`], a decimal number, or `0x..` hex.
pub fn parse(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u8::from_str_radix(hex, 16).ok();
    }
    if let Ok(n) = s.parse::<u8>() {
        return Some(n);
    }
    let lower = s.to_ascii_lowercase();
    let lower = match lower.as_str() {
        "escape" => "esc",
        "return" => "enter",
        "caps" => "capslock",
        "win" | "super" | "lwin" | "lsuper" => "lgui",
        "rwin" | "rsuper" => "rgui",
        "ctrl" | "control" => "lctrl",
        "shift" => "lshift",
        "alt" => "lalt",
        "del" => "delete",
        "pgup" => "pageup",
        "pgdn" | "pgdown" => "pagedown",
        "fn" => "fn1",
        "lmb" | "mouse1" => "mouse_left",
        "rmb" | "mouse2" => "mouse_right",
        "mmb" | "mouse3" => "mouse_middle",
        "mouse4" => "mouse_back",
        "mouse5" => "mouse_forward",
        "conv" => "henkan",
        "nonconv" | "non-conv" => "muhenkan",
        "cycle" => "profile_cycle",
        "next" => "next_track",
        "prev" | "previous" => "prev_track",
        "play" | "pause" | "playpause" => "play_pause",
        "volup" | "vol+" => "volume_up",
        "voldown" | "vol-" => "volume_down",
        "-" | "none" | "disabled" => "none",
        other => other,
    }
    .to_string();
    KEYCODES.iter().find(|(_, n)| *n == lower).map(|(c, _)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_minus_three_rule() {
        assert_eq!(parse("a"), Some(1));
        assert_eq!(parse("f12"), Some(0x42));
        assert_eq!(parse("Up"), Some(0x4f));
        assert_eq!(parse("0x77"), Some(0x77));
        assert_eq!(name(0x68), "rshift");
        assert_eq!(name(0x99), "0x99");
        assert_eq!(label(0x2c), "[");
        assert_eq!(label(0x3b), "F5");
        assert_eq!(label(0x01), "A");
        assert_eq!(label(0x4f), "↑");
        assert_eq!(label(0x5a), "Num 4");
        assert_eq!(label(0xc2), "⏯\u{FE0E}");
        assert_eq!(label(0xe2), "P2");
        assert_eq!(label(0xe8), "Key Lock");
        assert_eq!(label(0x99), "99");
        assert_eq!(label(NONE), "");
    }
}
