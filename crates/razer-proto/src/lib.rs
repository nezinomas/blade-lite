//! Builds and parses the 90-byte HID feature reports of the Blade 18 controller.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::print_stderr
)]

use std::ops::Range;

mod brightness;
mod charge;
mod colour;
mod command;
mod custom;
mod logo;
mod native_mode;
mod power_key;
mod preset;
mod profile;
mod readings;
mod touchpad_key;

pub use brightness::{Brightness, BrightnessError, VaporLight};
pub use charge::{ChargeLimit, ChargeLimitError};
pub use colour::{ParseRgbError, Rgb, static_colour, static_colour_touchpad_off};
pub use command::Command;
pub use custom::{CpuBoost, Custom, CustomError, CustomLimits, FanMode, GpuBoost, Rpm, RpmRange};
pub use logo::Logo;
pub use native_mode::NativeMode;
pub use power_key::PowerKey;
pub use preset::{PerfMode, PresetError};
pub use profile::{BLADE_18, KeyGrid, ModelProfile, UsbId};
pub use readings::{Celsius, FanRpm};
pub use touchpad_key::{TOUCHPAD_OFF, TouchpadKey};

/// Size of every feature report, sent and received.
pub const REPORT_LEN: usize = 90;

/// Position of the checksum byte in a report.
pub const CHECKSUM_INDEX: usize = 88;

/// Bytes the checksum covers: all after status and transaction ID, up to the checksum.
const CHECKSUMMED: Range<usize> = 2..CHECKSUM_INDEX;

/// Returns the checksum of a report: the XOR of bytes 2 to 87.
#[must_use]
pub fn checksum(report: &[u8; REPORT_LEN]) -> u8 {
    report[CHECKSUMMED].iter().fold(0, |acc, byte| acc ^ byte)
}

/// Most args a report can carry: bytes 8 to 87.
pub const MAX_ARGS: usize = 80;

/// Positions of the request fields in a report.
const TRANSACTION_ID_INDEX: usize = 1;
const DATA_SIZE_INDEX: usize = 5;
const COMMAND_INDEX: usize = 6;
const ARGS_INDEX: usize = 8;

/// Why a request could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// More args than fit in a report.
    TooManyArgs(usize),
}

/// A command for the controller, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    transaction_id: u8,
    command: u16,
    args: [u8; MAX_ARGS],
    data_size: u8,
}

impl Request {
    /// Makes a request. The high byte of `command` is the command class.
    ///
    /// # Errors
    ///
    /// [`BuildError::TooManyArgs`] if there are more than [`MAX_ARGS`] args.
    pub fn new(transaction_id: u8, command: u16, args: &[u8]) -> Result<Self, BuildError> {
        let too_many = || BuildError::TooManyArgs(args.len());
        let data_size = u8::try_from(args.len()).map_err(|_| too_many())?;
        let mut padded = [0; MAX_ARGS];
        padded
            .get_mut(..args.len())
            .ok_or_else(too_many)?
            .copy_from_slice(args);
        Ok(Self {
            transaction_id,
            command,
            args: padded,
            data_size,
        })
    }

    /// The 90 bytes to send, with the checksum filled in.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; REPORT_LEN] {
        let mut bytes = [0; REPORT_LEN];
        bytes[TRANSACTION_ID_INDEX] = self.transaction_id;
        bytes[DATA_SIZE_INDEX] = self.data_size;
        bytes[COMMAND_INDEX..ARGS_INDEX].copy_from_slice(&self.command.to_be_bytes());
        bytes[ARGS_INDEX..CHECKSUM_INDEX].copy_from_slice(&self.args);
        bytes[CHECKSUM_INDEX] = checksum(&bytes);
        bytes
    }
}

/// Position of the status byte in a reply.
const STATUS_INDEX: usize = 0;

/// What the controller says about the command it got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Success,
    Busy,
    Failure,
    NotSupported,
}

impl TryFrom<u8> for Status {
    type Error = ParseError;

    fn try_from(byte: u8) -> Result<Self, Self::Error> {
        match byte {
            0x01 => Ok(Self::Busy),
            0x02 => Ok(Self::Success),
            0x03 => Ok(Self::Failure),
            0x05 => Ok(Self::NotSupported),
            other => Err(ParseError::UnknownStatus(other)),
        }
    }
}

/// Why a reply could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The checksum byte does not match the bytes it covers.
    BadChecksum { expected: u8, found: u8 },
    /// The status byte is not one we know.
    UnknownStatus(u8),
    /// The data size byte says more args than fit in a report.
    BadDataSize(u8),
}

/// A reply from the controller, with a checked checksum and a known status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    status: Status,
    transaction_id: u8,
    command: u16,
    args: [u8; MAX_ARGS],
    data_size: usize,
}

impl Report {
    /// Checks the checksum, then reads the status.
    ///
    /// # Errors
    ///
    /// [`ParseError::BadChecksum`] if the checksum byte is wrong,
    /// [`ParseError::UnknownStatus`] if the status byte is not known,
    /// [`ParseError::BadDataSize`] if the data size is more than [`MAX_ARGS`].
    pub fn parse(bytes: &[u8; REPORT_LEN]) -> Result<Self, ParseError> {
        let expected = checksum(bytes);
        let found = bytes[CHECKSUM_INDEX];
        if expected != found {
            return Err(ParseError::BadChecksum { expected, found });
        }
        let status = Status::try_from(bytes[STATUS_INDEX])?;
        let data_size = bytes[DATA_SIZE_INDEX];
        if usize::from(data_size) > MAX_ARGS {
            return Err(ParseError::BadDataSize(data_size));
        }
        let mut args = [0; MAX_ARGS];
        args.copy_from_slice(&bytes[ARGS_INDEX..CHECKSUM_INDEX]);
        Ok(Self {
            status,
            transaction_id: bytes[TRANSACTION_ID_INDEX],
            command: u16::from_be_bytes([bytes[COMMAND_INDEX], bytes[COMMAND_INDEX + 1]]),
            args,
            data_size: usize::from(data_size),
        })
    }

    /// The controller's status for the command.
    #[must_use]
    pub fn status(&self) -> Status {
        self.status
    }

    /// The transaction ID the controller echoed back.
    #[must_use]
    pub fn transaction_id(&self) -> u8 {
        self.transaction_id
    }

    /// The command the controller echoed back. The high byte is the class.
    #[must_use]
    pub fn command(&self) -> u16 {
        self.command
    }

    /// The args the controller sent back: as many bytes as the data size says.
    /// [`Report::parse`] refuses a data size above [`MAX_ARGS`], so the
    /// empty fallback is never reached; if it were, a short reply fails to
    /// read rather than guessing.
    #[must_use]
    pub fn args(&self) -> &[u8] {
        self.args.get(..self.data_size).unwrap_or_default()
    }
}
