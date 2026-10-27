//! Device identification: vendor/product IDs, models, protocol variants.

use serde::{Deserialize, Serialize};

pub const WOOTING_VID_LEGACY: u16 = 0x03EB;
pub const WOOTING_VID: u16 = 0x31E3;

/// HID usage page of the configuration interface, "standard" firmware.
pub const USAGE_PAGE_CFG: u16 = 0x1337;
/// HID usage page of the configuration interface, "ARM"/v3 firmware.
pub const USAGE_PAGE_CFG_V3: u16 = 0xFF55;

/// Which wire variant the keyboard speaks. Decided by the usage page of its
/// configuration interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// Wooting One, Two, original 60HE. Usage page 0x1337.
    Standard,
    /// 60HE+, Two HE ARM, 60HE v2, 80HE, UwU. Usage page 0xFF55.
    Arm,
}

impl Protocol {
    pub fn from_usage_page(page: u16) -> Option<Protocol> {
        match page {
            USAGE_PAGE_CFG => Some(Protocol::Standard),
            USAGE_PAGE_CFG_V3 => Some(Protocol::Arm),
            _ => None,
        }
    }

    /// Report id used for the 8-byte feature command.
    pub fn command_report_id(self) -> u8 {
        match self {
            Protocol::Standard => 0,
            Protocol::Arm => 1,
        }
    }

    /// First magic byte of every packet.
    pub fn protocol_byte(self) -> u8 {
        match self {
            Protocol::Standard => 0xD0,
            Protocol::Arm => 0xD1,
        }
    }

    /// Largest input report the configuration interface can send, including
    /// the report id byte. Short command replies use report 1 (33 bytes) but
    /// profile dumps arrive on the bigger report ids, so always read with
    /// a buffer this large.
    pub fn response_len(self) -> usize {
        match self {
            Protocol::Standard => 256,
            Protocol::Arm => 2047,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    KeyboardTkl,
    KeyboardFull,
    Keyboard60,
    Keyboard80,
    Keypad3Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Model {
    pub name: &'static str,
    pub vid: u16,
    /// Base product id. Wooting also uses `pid + 1` and `pid + 2` for the
    /// same model (e.g. the 60HE+ enumerates as 0x1322).
    pub pid: u16,
    pub kind: DeviceKind,
    /// LED matrix columns in use (rows are always [`crate::rgb::ROWS`]
    /// except for the UwU keypad).
    pub rgb_cols: u8,
    pub rgb_rows: u8,
}

macro_rules! models {
    ($($name:literal, $vid:expr, $pid:expr, $kind:ident, $cols:expr, $rows:expr;)*) => {
        pub const MODELS: &[Model] = &[$(Model {
            name: $name, vid: $vid, pid: $pid, kind: DeviceKind::$kind, rgb_cols: $cols, rgb_rows: $rows,
        },)*];
    };
}

models! {
    "Wooting One",                WOOTING_VID_LEGACY, 0xFF01, KeyboardTkl,  17, 6;
    "Wooting One (V2)",           WOOTING_VID,        0x1100, KeyboardTkl,  17, 6;
    "Wooting Two",                WOOTING_VID_LEGACY, 0xFF02, KeyboardFull, 21, 6;
    "Wooting Two (V2)",           WOOTING_VID,        0x1200, KeyboardFull, 21, 6;
    "Wooting Two Lekker Edition", WOOTING_VID,        0x1210, KeyboardFull, 21, 6;
    "Wooting Two HE",             WOOTING_VID,        0x1220, KeyboardFull, 21, 6;
    "Wooting Two HE (ARM)",       WOOTING_VID,        0x1230, KeyboardFull, 21, 6;
    "Wooting 60HE",               WOOTING_VID,        0x1300, Keyboard60,   14, 6;
    "Wooting 60HE (ARM)",         WOOTING_VID,        0x1310, Keyboard60,   14, 6;
    "Wooting 60HE+",              WOOTING_VID,        0x1320, Keyboard60,   14, 6;
    "Wooting 60HE V2",            WOOTING_VID,        0x1340, Keyboard60,   14, 6;
    "Wooting 80HE",               WOOTING_VID,        0x1400, Keyboard80,   17, 6;
    "Wooting UwU",                WOOTING_VID,        0x1500, Keypad3Key,    7, 5;
    "Wooting UwU RGB",            WOOTING_VID,        0x1510, Keypad3Key,    7, 5;
}

impl Model {
    pub fn lookup(vid: u16, pid: u16) -> Option<&'static Model> {
        MODELS.iter().find(|m| {
            m.vid == vid
                && if vid == WOOTING_VID {
                    (m.pid..=m.pid + 2).contains(&pid)
                } else {
                    m.pid == pid
                }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_he_plus_alternate_pid() {
        let m = Model::lookup(WOOTING_VID, 0x1322).unwrap();
        assert_eq!(m.name, "Wooting 60HE+");
        assert!(Model::lookup(WOOTING_VID, 0x1323).is_none());
    }
}
