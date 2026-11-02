//! Builds and parses the 90-byte HID feature reports of the Blade 18 controller.

use std::ops::Range;

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
}

/// A reply from the controller, with a checked checksum and a known status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    status: Status,
}

impl Report {
    /// Checks the checksum, then reads the status.
    ///
    /// # Errors
    ///
    /// [`ParseError::BadChecksum`] if the checksum byte is wrong,
    /// [`ParseError::UnknownStatus`] if the status byte is not known.
    pub fn parse(bytes: &[u8; REPORT_LEN]) -> Result<Self, ParseError> {
        let expected = checksum(bytes);
        let found = bytes[CHECKSUM_INDEX];
        if expected != found {
            return Err(ParseError::BadChecksum { expected, found });
        }
        let status = Status::try_from(bytes[STATUS_INDEX])?;
        Ok(Self { status })
    }

    /// The controller's status for the command.
    #[must_use]
    pub fn status(&self) -> Status {
        self.status
    }
}
