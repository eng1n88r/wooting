//! Pure wire-protocol layer for Wooting keyboards.
//!
//! No I/O lives here: the crate only knows how to build the bytes a Wooting
//! keyboard expects and how to interpret the bytes it sends back. The
//! facts encoded here come from Wooting's MPL-licensed `wooting-rgb-sdk`,
//! community protocol notes and dumps from a 60HE+; see `docs/protocol.md`.

pub mod command;
pub mod keycode;
pub mod layout;
pub mod model;
pub mod packet;
pub mod report;
pub mod rgb;

/// Protobuf messages carried by reports (generated from `proto/wooting.proto`).
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/wooting.rs"));
}

pub use command::Command;
pub use layout::{Key, Layout};
pub use model::{Model, Protocol, WOOTING_VID, WOOTING_VID_LEGACY};
pub use packet::{CommandPacket, Response, ResponseError, STATUS_OK, STATUS_UNSUPPORTED};
pub use prost;
pub use prost::Message;
pub use rgb::{Frame, Rgb, COLS, ROWS};

/// Second magic byte shared by every Wooting config packet.
pub const MAGIC: u8 = 0xDA;

/// Status byte 0x88 observed on success; 0x66 for unsupported commands.
pub fn status_name(status: u8) -> &'static str {
    match status {
        STATUS_OK => "ok",
        STATUS_UNSUPPORTED => "unsupported",
        _ => "unknown",
    }
}
