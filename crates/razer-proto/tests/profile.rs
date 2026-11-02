mod common;

use razer_proto::BLADE_18;

#[test]
fn every_captured_report_went_through_the_profile_interface() {
    for report in common::captured_reports() {
        assert_eq!(
            report.interface, BLADE_18.usb.interface_number,
            "{}",
            report.fixture
        );
    }
}
