mod common;

use razer_proto::{BLADE_18, Request, TouchpadKey};

const SET_KEY: [u8; 2] = [0x00, 0x59];

/// The fixtures where the touchpad key was pressed.
const FIXTURES: [&str; 3] = [
    "23-touchpad-key-green.txt",
    "25-touchpad-key-low.txt",
    "26-touchpad-key-zero.txt",
];

/// Every `0059` request Synapse sent in the touchpad key fixtures.
fn captured_key() -> Vec<[u8; razer_proto::REPORT_LEN]> {
    common::captured_requests()
        .into_iter()
        .filter(|request| {
            FIXTURES.contains(&request.fixture.as_str())
                && [request.bytes[6], request.bytes[7]] == SET_KEY
        })
        .map(|request| request.bytes)
        .collect()
}

fn key() -> TouchpadKey {
    BLADE_18
        .touchpad_key
        .expect("the Blade 18 has a touchpad key")
}

#[test]
fn lit_and_unlit_match_their_golden_bytes() {
    let golden = captured_key();
    assert_eq!(golden.len(), 6);
    for report in golden {
        let lit = report[10] == 0x01;
        let command = key().command(lit);

        let built = Request::new(report[1], command.id, &command.args)
            .unwrap()
            .to_bytes();

        assert_eq!(built, report);
    }
}

#[test]
fn the_query_reads_the_key_led() {
    let query = key().query();

    assert_eq!(query.id, 0x00D9);
    assert_eq!(query.args, [0x00, 0x01, 0x00]);
}

#[test]
fn a_reply_for_the_key_led_says_lit_or_not() {
    assert_eq!(key().from_reply(&[0x00, 0x01, 0x01]), Some(true));
    assert_eq!(key().from_reply(&[0x00, 0x01, 0x00]), Some(false));
}

#[test]
fn a_reply_for_another_led_or_an_odd_value_is_not_read() {
    assert_eq!(key().from_reply(&[0x00, 0x02, 0x01]), None);
    assert_eq!(key().from_reply(&[0x00, 0x01, 0x02]), None);
    assert_eq!(key().from_reply(&[0x01, 0x01, 0x01]), None);
    assert_eq!(key().from_reply(&[0x00, 0x01]), None);
    assert_eq!(key().from_reply(&[]), None);
}
