mod common;

use razer_proto::{CHECKSUM_INDEX, ParseError, Report, Status};

/// Status codes from the controller (same as in r-helper).
const BUSY: u8 = 0x01;
const SUCCESS: u8 = 0x02;
const FAILURE: u8 = 0x03;
const NOT_SUPPORTED: u8 = 0x05;

/// A captured reply with its status byte changed. The checksum does not cover
/// the status byte, so the reply stays valid.
fn reply_with_status(status: u8) -> [u8; 90] {
    let mut bytes = common::captured_replies()[0].bytes;
    bytes[0] = status;
    bytes
}

#[test]
fn every_captured_reply_parses_as_success() {
    for reply in common::captured_replies() {
        let report = Report::parse(&reply.bytes)
            .unwrap_or_else(|error| panic!("{}: {error:?}", reply.fixture));
        assert_eq!(report.status(), Status::Success, "{}", reply.fixture);
    }
}

#[test]
fn status_byte_maps_to_status() {
    for (byte, status) in [
        (SUCCESS, Status::Success),
        (BUSY, Status::Busy),
        (FAILURE, Status::Failure),
        (NOT_SUPPORTED, Status::NotSupported),
    ] {
        let report = Report::parse(&reply_with_status(byte)).unwrap();
        assert_eq!(report.status(), status, "status byte {byte:#04X}");
    }
}

#[test]
fn unknown_status_byte_is_an_error() {
    assert_eq!(
        Report::parse(&reply_with_status(0x7F)),
        Err(ParseError::UnknownStatus(0x7F))
    );
}

#[test]
fn bad_checksum_is_an_error() {
    let mut bytes = reply_with_status(SUCCESS);
    let good = bytes[CHECKSUM_INDEX];
    bytes[CHECKSUM_INDEX] ^= 0xFF;

    assert_eq!(
        Report::parse(&bytes),
        Err(ParseError::BadChecksum {
            expected: good,
            found: good ^ 0xFF,
        })
    );
}
