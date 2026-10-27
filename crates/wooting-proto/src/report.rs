//! Output reports carrying structured (mostly protobuf) payloads.
//!
//! Layout on ARM boards: `[report_id, 0xD1, 0xDA, report_type, len_hi, len_lo, payload…]`
//! padded to the size of the chosen report id. The 60HE+ configuration
//! interface declares these output report sizes (bytes after the id):
//! 1→32, 2→62, 3→254, 4→510, 5→1022, 6→2046.

use crate::Protocol;

/// Report types (keyboard-side "report ids" in Wooting's naming).
pub mod kind {
    pub const WOOT_DEV_RAW: u8 = 0x0B;
    pub const RGB_PROFILE_COLORS_PART1: u8 = 0x0E;
    pub const RGB_PROFILE_COLORS_PART2: u8 = 0x0F;
    pub const MAPPING_PROFILE: u8 = 0x14;
    pub const RGB_PROFILE_CORE: u8 = 0x16;
    pub const GLOBAL_SETTINGS: u8 = 0x17;
}

/// `(report_id, payload_len)` pairs of the ARM configuration interface,
/// smallest first.
pub const ARM_OUTPUT_REPORTS: &[(u8, usize)] =
    &[(1, 32), (2, 62), (3, 254), (4, 510), (5, 1022), (6, 2046)];

pub const HEADER_LEN: usize = 6;

/// Build an output report for `report_type` with `payload`, choosing the
/// smallest report id that fits. Returns `None` if the payload is too large.
pub fn build(protocol: Protocol, report_type: u8, payload: &[u8]) -> Option<Vec<u8>> {
    build_with_prefix(protocol, report_type, &[], payload)
}

/// Like [`build`] but inserts `prefix` between the length and the payload.
/// The RGB colour-row reports carry one such byte (observed `0x00`); the
/// length field counts only `payload`.
pub fn build_with_prefix(
    protocol: Protocol,
    report_type: u8,
    prefix: &[u8],
    payload: &[u8],
) -> Option<Vec<u8>> {
    let needed = HEADER_LEN - 1 + prefix.len() + payload.len(); // bytes after the report id
    let (id, size) = ARM_OUTPUT_REPORTS.iter().copied().find(|(_, s)| *s >= needed)?;
    let mut out = vec![0u8; size + 1];
    out[0] = id;
    out[1] = protocol.protocol_byte();
    out[2] = crate::MAGIC;
    out[3] = report_type;
    let len = u16::try_from(payload.len()).ok()?;
    out[4..6].copy_from_slice(&len.to_be_bytes());
    let start = HEADER_LEN + prefix.len();
    out[HEADER_LEN..start].copy_from_slice(prefix);
    out[start..start + payload.len()].copy_from_slice(payload);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_smallest_report() {
        let r = build(Protocol::Arm, kind::RGB_PROFILE_CORE, &[0x0a, 0x02, 0x08, 0x05]).unwrap();
        assert_eq!(r.len(), 33);
        assert_eq!(&r[..6], &[1, 0xD1, 0xDA, 0x16, 0, 4]);
        assert_eq!(&r[6..10], &[0x0a, 0x02, 0x08, 0x05]);
        let big = build(Protocol::Arm, 0, &[0; 300]).unwrap();
        assert_eq!(big[0], 4);
        assert_eq!(big.len(), 511);
        assert!(build(Protocol::Arm, 0, &[0; 3000]).is_none());
        let p = build_with_prefix(Protocol::Arm, kind::RGB_PROFILE_COLORS_PART1, &[0], &[0x0a, 0x01, 0x05]).unwrap();
        assert_eq!(&p[..10], &[1, 0xD1, 0xDA, 0x0E, 0, 3, 0, 0x0a, 0x01, 0x05]);
    }
}
