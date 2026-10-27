//! 8-byte feature command and its response.

use crate::{Command, Protocol, MAGIC};
use serde::{Deserialize, Serialize};

/// Length of the command feature report including the report id byte.
pub const COMMAND_LEN: usize = 8;
/// Offset of the command id inside both the command and the response.
pub const CMD_OFFSET: usize = 3;
/// Offset of the first payload byte in a response.
pub const PAYLOAD_OFFSET: usize = 5;

/// Status byte values observed on the 60HE+ (firmware 2.14.1).
pub const STATUS_OK: u8 = 0x88;
/// Returned for commands the firmware does not implement (e.g. GetNumberOfKeys).
pub const STATUS_UNSUPPORTED: u8 = 0x66;

/// The 8-byte feature report:
/// `[report_id, proto, 0xDA, cmd, p3, p2, p1, p0]`.
///
/// Parameters are stored in reverse order, mirroring
/// `wooting_usb_send_feature(cmd, p0, p1, p2, p3)` in Wooting's SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandPacket(pub [u8; COMMAND_LEN]);

impl CommandPacket {
    pub fn new(protocol: Protocol, cmd: Command, params: [u8; 4]) -> Self {
        let [p0, p1, p2, p3] = params;
        CommandPacket([
            protocol.command_report_id(),
            protocol.protocol_byte(),
            MAGIC,
            cmd as u8,
            p3,
            p2,
            p1,
            p0,
        ])
    }

    pub fn bytes(&self) -> &[u8; COMMAND_LEN] {
        &self.0
    }

    pub fn command(&self) -> Option<Command> {
        Command::from_u8(self.0[CMD_OFFSET])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResponseError {
    #[error("response too short ({0} bytes)")]
    TooShort(usize),
    #[error("response is for command {got}, expected {expected}")]
    WrongCommand { expected: u8, got: u8 },
    #[error("bad magic bytes {0:02x} {1:02x}")]
    BadMagic(u8, u8),
}

/// A raw command response as read from the input endpoint:
/// `[report_id, proto, 0xDA, cmd_echo, status, payload...]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub raw: Vec<u8>,
}

impl Response {
    pub fn parse(raw: Vec<u8>, expected: Command) -> Result<Response, ResponseError> {
        if raw.len() <= PAYLOAD_OFFSET {
            return Err(ResponseError::TooShort(raw.len()));
        }
        if raw[2] != MAGIC || !matches!(raw[1], 0xD0 | 0xD1) {
            return Err(ResponseError::BadMagic(raw[1], raw[2]));
        }
        if raw[CMD_OFFSET] != expected as u8 {
            return Err(ResponseError::WrongCommand {
                expected: expected as u8,
                got: raw[CMD_OFFSET],
            });
        }
        Ok(Response { raw })
    }

    /// True when `raw` looks like a response to `cmd` (used to skip
    /// unrelated input reports while waiting).
    pub fn is_for(raw: &[u8], cmd: Command) -> bool {
        raw.len() > CMD_OFFSET && raw[2] == MAGIC && raw[CMD_OFFSET] == cmd as u8
    }

    pub fn command(&self) -> Option<Command> {
        Command::from_u8(self.raw[CMD_OFFSET])
    }

    pub fn status(&self) -> u8 {
        self.raw[4]
    }

    pub fn ok(&self) -> bool {
        self.status() == STATUS_OK
    }

    pub fn payload(&self) -> &[u8] {
        &self.raw[PAYLOAD_OFFSET..]
    }

    /// ARM responses carry `[len, 0, data[0..len]]` in the payload. Returns
    /// `data`, or the whole payload when the length byte is implausible.
    pub fn data(&self) -> &[u8] {
        let p = self.payload();
        let len = p[0] as usize;
        if p.len() >= 2 + len {
            &p[2..2 + len]
        } else {
            p
        }
    }

    /// Payload with trailing zero padding removed.
    pub fn payload_trimmed(&self) -> &[u8] {
        let p = self.payload();
        let end = p.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        &p[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_packet_layout() {
        let p = CommandPacket::new(
            Protocol::Arm,
            Command::WootDevSingleColor,
            [0x25, 10, 20, 30],
        );
        assert_eq!(p.bytes(), &[1, 0xD1, 0xDA, 30, 30, 20, 10, 0x25]);
    }

    #[test]
    fn standard_packet_layout() {
        let p = CommandPacket::new(Protocol::Standard, Command::Ping, [0; 4]);
        assert_eq!(p.bytes(), &[0, 0xD0, 0xDA, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn response_parse() {
        // Real GetVersion reply from a 60HE+: len=3, pad, 2.14.1
        let raw = vec![1, 0xD1, 0xDA, 1, 0x88, 3, 0, 2, 14, 1, 0, 0];
        let r = Response::parse(raw, Command::GetVersion).unwrap();
        assert!(r.ok());
        assert_eq!(r.payload_trimmed(), &[3, 0, 2, 14, 1]);
        assert_eq!(r.data(), &[2, 14, 1]);
        assert!(matches!(
            Response::parse(vec![1, 0xD1, 0xDA, 3, 0, 0], Command::GetVersion),
            Err(ResponseError::WrongCommand {
                expected: 1,
                got: 3
            })
        ));
    }
}
