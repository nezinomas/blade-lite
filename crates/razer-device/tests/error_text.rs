//! `DeviceError` reads as plain text in the log: commands in hex, details in
//! brackets (the log counts errors that differ only in brackets as one).

use std::io;

use razer_device::DeviceError;
use razer_proto::{BuildError, ParseError, Status};

#[test]
fn wrong_reply_shows_the_command_in_hex() {
    let error = DeviceError::WrongReply {
        transaction_id: 4,
        command: 0x0D81,
    };

    assert_eq!(
        error.to_string(),
        "the reply is for another request (transaction ID 4, command 0D81)"
    );
}

#[test]
fn a_windows_error_is_one_short_line() {
    let error = DeviceError::Io(io::Error::from_raw_os_error(1167));

    let text = error.to_string();

    assert!(text.starts_with("the HID layer failed ("), "{text}");
    assert!(text.contains("os error 1167"), "{text}");
    assert!(!text.contains("Os {"), "{text}");
}

#[test]
fn each_other_error_has_plain_text() {
    let cases = [
        (
            DeviceError::Build(BuildError::TooManyArgs(81)),
            "the command does not fit in a report (81 args)",
        ),
        (
            DeviceError::Parse(ParseError::BadChecksum {
                expected: 0x12,
                found: 0x34,
            }),
            "the reply is not valid (checksum 34, not 12)",
        ),
        (
            DeviceError::Parse(ParseError::UnknownStatus(0x09)),
            "the reply is not valid (unknown status 09)",
        ),
        (
            DeviceError::Parse(ParseError::BadDataSize(0x60)),
            "the reply is not valid (data size 96 is too big)",
        ),
        (
            DeviceError::Busy,
            "the controller stayed busy after 5 reads",
        ),
        (
            DeviceError::Rejected(Status::Failure),
            "the controller replied Failure",
        ),
        (
            DeviceError::Rejected(Status::NotSupported),
            "the controller replied NotSupported",
        ),
        (DeviceError::ShortReply, "the reply has too few args"),
    ];

    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
}
