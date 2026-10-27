//! High-level model of a Wooting keyboard: profiles, lighting, settings,
//! image and theme mapping, backups. Built on `wooting-hid`.

pub mod api;
pub mod backup;
pub mod client;
pub mod image_map;
pub mod keyboard;
pub mod theme;

pub use keyboard::{AdvancedKey, DeviceSummary, Keyboard, ProfileInfo};
pub use wooting_proto::{Frame, Layout, Rgb};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Hid(#[from] wooting_hid::Error),
    #[error("keyboard answered {command} with status 0x{status:02x} ({name})")]
    Status {
        command: wooting_proto::Command,
        status: u8,
        name: &'static str,
    },
    #[error("protobuf decode: {0}")]
    Decode(#[from] wooting_proto::prost::DecodeError),
    #[error("unknown key {0:?}")]
    UnknownKey(String),
    #[error("profile slot {0} does not exist")]
    NoSuchProfile(u8),
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
