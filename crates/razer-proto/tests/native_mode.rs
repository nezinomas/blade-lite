//! The panel's native mode. Synapse's code gave the bytes, not a capture, so
//! the golden reports are written by hand.

use razer_proto::{BLADE_18, NativeMode, REPORT_LEN, Request};

/// The transaction ID used for the hand-written reports.
const TRANSACTION_ID: u8 = 0x1F;

/// A report with only these bytes set, the rest zero.
fn golden(bytes: &[(usize, u8)]) -> [u8; REPORT_LEN] {
    let mut report = [0; REPORT_LEN];
    for &(index, byte) in bytes {
        report[index] = byte;
    }
    report
}

fn to_bytes(command: &razer_proto::Command) -> [u8; REPORT_LEN] {
    Request::new(TRANSACTION_ID, command.id, &command.args)
        .unwrap()
        .to_bytes()
}

const UHD: NativeMode = BLADE_18.native_modes[0];
const FHD: NativeMode = BLADE_18.native_modes[1];

#[test]
fn the_blade_18_has_uhd_at_240_and_fhd_at_440() {
    assert_eq!((UHD.width, UHD.height, UHD.hz), (3840, 2400, 240));
    assert_eq!((FHD.width, FHD.height, FHD.hz), (1920, 1200, 440));
}

#[test]
fn each_mode_matches_its_golden_bytes() {
    // 0D0E, data size 1, args [mode]. The checksum is 01 ^ 0D ^ 0E ^ mode.
    let uhd = golden(&[
        (1, 0x1F),
        (5, 0x01),
        (6, 0x0D),
        (7, 0x0E),
        (8, 0x00),
        (88, 0x02),
    ]);
    let fhd = golden(&[
        (1, 0x1F),
        (5, 0x01),
        (6, 0x0D),
        (7, 0x0E),
        (8, 0x01),
        (88, 0x03),
    ]);

    assert_eq!(to_bytes(&UHD.command()), uhd);
    assert_eq!(to_bytes(&FHD.command()), fhd);
}

#[test]
fn the_query_matches_its_golden_bytes() {
    // 0D8E, data size 1, args [0]. The checksum is 01 ^ 0D ^ 8E.
    let query = golden(&[(1, 0x1F), (5, 0x01), (6, 0x0D), (7, 0x8E), (88, 0x82)]);

    assert_eq!(to_bytes(&NativeMode::query()), query);
}

#[test]
fn a_reply_names_the_chosen_mode() {
    assert_eq!(NativeMode::from_reply(&[0x00], &BLADE_18), Some(UHD));
    assert_eq!(NativeMode::from_reply(&[0x01], &BLADE_18), Some(FHD));
}

#[test]
fn an_unknown_mode_byte_is_none() {
    assert_eq!(NativeMode::from_reply(&[0x07], &BLADE_18), None);
    assert_eq!(NativeMode::from_reply(&[], &BLADE_18), None);
}
