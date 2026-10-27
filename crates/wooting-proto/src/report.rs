//! Output reports carrying structured (mostly protobuf) payloads.
//!
//! Layout on ARM boards, as captured from Wootility v5.4:
//!
//! ```text
//! [report_id, 0xD1, 0xDA, type, flags, len_lo, len_hi, payload…]
//! ```
//!
//! `flags` selects the destination: [`FLAGS_RAM`] (0) writes the active
//! profile in RAM, [`flags_persist`]`(slot)` = `(slot << 1) | 1` writes a
//! profile slot in flash, [`FLAGS_GLOBAL`] (1) for device-wide settings.
//! The key-mapping report (0x14) inserts the layer index and two zero bytes
//! between `flags` and the length. The report is padded to the size of the
//! smallest report id that fits; the 60HE+ configuration interface declares
//! output sizes (bytes after the id) 1→32, 2→62, 3→254, 4→510, 5→1022, 6→2046.

use crate::Protocol;

/// Report types (keyboard-side "report ids" in Wooting's naming).
pub mod kind {
    pub const DEVICE_CONFIG: u8 = 0x07;
    pub const WOOT_DEV_RAW: u8 = 0x0B;
    pub const RGB_PROFILE_COLORS_PART1: u8 = 0x0E;
    pub const RGB_PROFILE_COLORS_PART2: u8 = 0x0F;
    pub const KEYBOARD_PROFILE: u8 = 0x11;
    pub const UNKNOWN_12: u8 = 0x12;
    pub const ANALOG_CURVE: u8 = 0x13;
    pub const MAPPING_PROFILE: u8 = 0x14;
    pub const UNKNOWN_15: u8 = 0x15;
    pub const RGB_PROFILE_CORE: u8 = 0x16;
    pub const GLOBAL_SETTINGS: u8 = 0x17;
    pub const ADVANCED_KEYS: u8 = 0x18;
    pub const PROFILE_METADATA: u8 = 0x1A;
    pub const PROFILE_BLOB: u8 = 0x1B;
    pub const UNKNOWN_1D: u8 = 0x1D;
    pub const UNKNOWN_28: u8 = 0x28;
}

/// Write the active profile in RAM (volatile until the profile is saved).
pub const FLAGS_RAM: u8 = 0x00;
/// Device-wide settings (GlobalSettings).
pub const FLAGS_GLOBAL: u8 = 0x01;

/// Flags that persist a profile report into flash slot `slot`.
pub const fn flags_persist(slot: u8) -> u8 {
    (slot << 1) | 1
}

/// `(report_id, payload_len)` pairs of the ARM configuration interface,
/// smallest first.
pub const ARM_OUTPUT_REPORTS: &[(u8, usize)] =
    &[(1, 32), (2, 62), (3, 254), (4, 510), (5, 1022), (6, 2046)];

fn frame(protocol: Protocol, header: &[u8], payload: &[u8]) -> Option<Vec<u8>> {
    let needed = 2 + header.len() + payload.len();
    let (id, size) = ARM_OUTPUT_REPORTS.iter().copied().find(|(_, s)| *s >= needed)?;
    let mut out = Vec::with_capacity(size + 1);
    out.push(id);
    out.push(protocol.protocol_byte());
    out.push(crate::MAGIC);
    out.extend_from_slice(header);
    out.extend_from_slice(payload);
    out.resize(size + 1, 0);
    Some(out)
}

/// Build a structured output report. Returns `None` if the payload is too
/// large for any report id.
pub fn build(protocol: Protocol, report_type: u8, flags: u8, payload: &[u8]) -> Option<Vec<u8>> {
    let len = u16::try_from(payload.len()).ok()?.to_le_bytes();
    frame(protocol, &[report_type, flags, len[0], len[1]], payload)
}

/// Build the key-mapping report for one layer.
pub fn build_mapping(protocol: Protocol, flags: u8, layer: u8, payload: &[u8]) -> Option<Vec<u8>> {
    let len = u16::try_from(payload.len()).ok()?.to_le_bytes();
    frame(
        protocol,
        &[kind::MAPPING_PROFILE, flags, layer, 0, 0, len[0], len[1]],
        payload,
    )
}

/// Build a raw (length-less) report such as DeviceConfig.
pub fn build_raw(protocol: Protocol, report_type: u8, flags: u8, body: &[u8]) -> Option<Vec<u8>> {
    frame(protocol, &[report_type, flags], body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_wootility_capture() {
        // RgbProfileCore write captured from Wootility: d1 da 16 00 0d 00 <13 bytes>
        let payload = [0x0a, 0x0b, 0x08, 0xff, 0x01, 0x10, 0xff, 0xff, 0x03, 0x18, 0xff, 0xff, 0x03];
        let r = build(Protocol::Arm, kind::RGB_PROFILE_CORE, FLAGS_RAM, &payload).unwrap();
        assert_eq!(r.len(), 33);
        assert_eq!(&r[..7], &[1, 0xD1, 0xDA, 0x16, 0x00, 0x0d, 0x00]);
        assert_eq!(&r[7..20], &payload);
        // Mapping layer 1 persisted to slot 3: d1 da 14 07 01 00 00 96 00 …
        let m = build_mapping(Protocol::Arm, flags_persist(3), 1, &[0u8; 150]).unwrap();
        assert_eq!(&m[..10], &[3, 0xD1, 0xDA, 0x14, 0x07, 0x01, 0, 0, 0x96, 0]);
        assert_eq!(m.len(), 255);
        assert!(build(Protocol::Arm, 0, 0, &[0; 3000]).is_none());
        assert_eq!(flags_persist(3), 7);
    }
}
