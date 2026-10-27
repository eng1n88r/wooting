//! Pure wire-protocol layer for Wooting keyboards.
//!
//! No I/O lives here: the crate only knows how to build the bytes a Wooting
//! keyboard expects and how to interpret the bytes it sends back. The
//! facts encoded here come from Wooting's MPL-licensed `wooting-rgb-sdk`
//! and from community protocol notes; see `docs/protocol.md` in the repo.

pub mod command;
pub mod model;
pub mod packet;
pub mod report;
pub mod rgb;

pub use command::Command;
pub use model::{Model, Protocol, WOOTING_VID, WOOTING_VID_LEGACY};
pub use packet::{CommandPacket, Response, ResponseError};
pub use rgb::{Frame, Rgb, COLS, ROWS};

/// Second magic byte shared by every Wooting config packet.
pub const MAGIC: u8 = 0xDA;
