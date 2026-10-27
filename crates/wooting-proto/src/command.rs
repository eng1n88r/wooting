//! Feature-report command IDs (host -> keyboard).
//!
//! IDs are sequential 0..=59. Entries marked `Removed*` were retired by
//! Wooting but still occupy their slot on the wire.

use serde::{Deserialize, Serialize};

macro_rules! commands {
    ($($name:ident = $id:expr,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(u8)]
        pub enum Command { $($name = $id,)* }

        impl Command {
            pub const ALL: &'static [Command] = &[$(Command::$name,)*];

            pub fn from_u8(id: u8) -> Option<Command> {
                match id { $($id => Some(Command::$name),)* _ => None }
            }

            pub fn name(self) -> &'static str {
                match self { $(Command::$name => stringify!($name),)* }
            }
        }
    };
}

commands! {
    Ping = 0,
    GetVersion = 1,
    ResetToBootloader = 2,
    GetSerial = 3,
    GetRgbProfileCount = 4,
    Removed5 = 5,
    Removed6 = 6,
    ReloadProfile0 = 7,
    SaveRgbProfile = 8,
    GetDigitalProfilesCount = 9,
    GetAnalogProfilesCount = 10,
    GetCurrentKeyboardProfileIndex = 11,
    GetDigitalProfile = 12,
    GetAnalogProfileMainPart = 13,
    GetAnalogProfileCurveChangeMapPart1 = 14,
    GetAnalogProfileCurveChangeMapPart2 = 15,
    GetNumberOfKeys = 16,
    GetMainMappingProfile = 17,
    GetFunctionMappingProfile = 18,
    GetDeviceConfig = 19,
    GetAnalogValues = 20,
    KeysOff = 21,
    KeysOn = 22,
    ActivateProfile = 23,
    GetDksProfile = 24,
    SoftReset = 25,
    Removed26 = 26,
    Removed27 = 27,
    Removed28 = 28,
    RefreshRgbColors = 29,
    WootDevSingleColor = 30,
    WootDevResetColor = 31,
    WootDevResetAll = 32,
    WootDevInit = 33,
    Removed34 = 34,
    GetRgbProfileColorsPart1 = 35,
    GetRgbProfileColorsPart2 = 36,
    Removed37 = 37,
    ReloadProfile = 38,
    GetKeyboardProfile = 39,
    GetGamepadMapping = 40,
    GetGamepadProfile = 41,
    SaveKeyboardProfile = 42,
    ResetSettings = 43,
    SetRawScanning = 44,
    StartXinputDetection = 45,
    StopXinputDetection = 46,
    SaveDksProfile = 47,
    GetMappingProfile = 48,
    GetActuationProfile = 49,
    GetRgbProfileCore = 50,
    GetGlobalSettings = 51,
    GetAkcProfile = 52,
    SaveAkcProfile = 53,
    GetRapidTriggerProfile = 54,
    GetProfileMetadata = 55,
    IsFlashChipConnected = 56,
    GetRgbLayer = 57,
    GetFlashStats = 58,
    GetRgbBins = 59,
    // 63/64/72 seen from Wootility v5.4 (p3 = slot); 73/74 are its polling commands.
    DeleteProfile = 63,
    CreateProfile = 64,
    GetRtKeys = 61,
    GetProfileCount = 62,
    PostSaveProfile = 72,
    Poll73 = 73,
    AnalogReport = 74,
    Cmd81 = 81,
    GetDefaultProfile = 82,
    GetSwitchTable = 83,
}

impl Command {
    /// Commands that change persistent state or reboot the device. Callers
    /// should require explicit confirmation before sending these.
    pub fn is_dangerous(self) -> bool {
        matches!(
            self,
            Command::ResetToBootloader
                | Command::ResetSettings
                | Command::SoftReset
                | Command::DeleteProfile
                | Command::CreateProfile
        )
    }

    /// Case-insensitive lookup by name or decimal id.
    pub fn parse(s: &str) -> Option<Command> {
        if let Ok(id) = s.parse::<u8>() {
            return Command::from_u8(id);
        }
        let s = s.to_ascii_lowercase().replace(['_', '-'], "");
        Command::ALL
            .iter()
            .copied()
            .find(|c| c.name().to_ascii_lowercase() == s)
    }
}

impl From<Command> for u8 {
    fn from(c: Command) -> u8 {
        c as u8
    }
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), *self as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip() {
        for c in Command::ALL {
            assert_eq!(Command::from_u8(*c as u8), Some(*c));
        }
        assert_eq!(
            Command::ALL[..60]
                .iter()
                .map(|c| *c as usize)
                .collect::<Vec<_>>(),
            (0..60).collect::<Vec<_>>()
        );
        assert_eq!(Command::from_u8(63), Some(Command::DeleteProfile));
    }

    #[test]
    fn parse_by_name_or_id() {
        assert_eq!(Command::parse("ping"), Some(Command::Ping));
        assert_eq!(Command::parse("get-serial"), Some(Command::GetSerial));
        assert_eq!(Command::parse("55"), Some(Command::GetProfileMetadata));
        assert_eq!(Command::parse("nope"), None);
    }
}
