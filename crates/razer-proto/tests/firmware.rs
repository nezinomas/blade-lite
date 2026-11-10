//! The controller's firmware version, read as r-helper reads it.

use razer_proto::FirmwareVersion;

#[test]
fn the_query_is_0081_with_two_zero_args() {
    let query = FirmwareVersion::query();

    assert_eq!(query.id, 0x0081);
    assert_eq!(query.args, [0x00, 0x00]);
}

#[test]
fn the_version_is_the_first_two_reply_args() {
    assert_eq!(
        FirmwareVersion::from_reply(&[1, 7, 9]),
        Some(FirmwareVersion { major: 1, minor: 7 })
    );
}

#[test]
fn a_reply_without_two_args_is_none() {
    assert_eq!(FirmwareVersion::from_reply(&[1]), None);
    assert_eq!(FirmwareVersion::from_reply(&[]), None);
}
