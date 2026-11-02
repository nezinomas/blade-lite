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
