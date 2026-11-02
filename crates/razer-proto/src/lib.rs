//! Builds and parses the 90-byte HID feature reports of the Blade 18 controller.

/// Size of every feature report, sent and received.
pub const REPORT_LEN: usize = 90;

/// Position of the checksum byte in a report.
pub const CHECKSUM_INDEX: usize = 88;

/// Returns the checksum of a report: the XOR of bytes 2 to 87.
///
/// Bytes 0 and 1 (status and transaction ID) are not covered.
#[must_use]
pub fn checksum(report: &[u8; REPORT_LEN]) -> u8 {
    report[2..CHECKSUM_INDEX]
        .iter()
        .fold(0, |acc, byte| acc ^ byte)
}
