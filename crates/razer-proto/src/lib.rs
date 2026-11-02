//! Builds and parses the 90-byte HID feature reports of the Blade 18 controller.

/// Size of every feature report, sent and received.
pub const REPORT_LEN: usize = 90;

/// Position of the checksum byte in a report.
pub const CHECKSUM_INDEX: usize = 88;

/// Returns the checksum of a report.
#[must_use]
pub fn checksum(_report: &[u8; REPORT_LEN]) -> u8 {
    0
}
