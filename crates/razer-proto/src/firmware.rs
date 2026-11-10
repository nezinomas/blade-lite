//! The controller's firmware version, read as r-helper reads it.

use crate::Command;

/// Get firmware version. Args are `[0, 0]`, the reply args `[major, minor]`.
const GET_FIRMWARE_VERSION: u16 = 0x0081;

/// The controller's firmware version, such as 1.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareVersion {
    pub major: u8,
    pub minor: u8,
}

impl FirmwareVersion {
    /// The command that reads it.
    #[must_use]
    pub fn query() -> Command {
        Command {
            id: GET_FIRMWARE_VERSION,
            args: vec![0, 0],
        }
    }

    /// The version in a query's reply args, or `None` without two args.
    #[must_use]
    pub fn from_reply(args: &[u8]) -> Option<Self> {
        match *args {
            [major, minor, ..] => Some(Self { major, minor }),
            _ => None,
        }
    }
}
