mod common;

use razer_proto::{CHECKSUM_INDEX, checksum};

#[test]
fn checksum_of_every_captured_report_matches_its_checksum_byte() {
    for report in common::captured_reports() {
        assert_eq!(
            checksum(&report.bytes),
            report.bytes[CHECKSUM_INDEX],
            "{}: {:02X?}",
            report.fixture,
            report.bytes
        );
    }
}
