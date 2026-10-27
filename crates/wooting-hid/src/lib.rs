//! Finds Wooting keyboards and talks to their configuration interface.

mod discover;
mod transport;

pub use discover::{enumerate, DeviceInfo};
pub use transport::Transport;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hid: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("no Wooting keyboard with a configuration interface found")]
    NotFound,
    #[error("no response to {0} within {1} ms")]
    Timeout(wooting_proto::Command, u64),
    #[error(transparent)]
    Response(#[from] wooting_proto::ResponseError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
