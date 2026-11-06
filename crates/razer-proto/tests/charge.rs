mod common;

use razer_proto::{BLADE_18, ChargeLimit, ChargeLimitError, Request};

const SET_CHARGE_LIMIT: [u8; 2] = [0x07, 0x12];

/// The captured steps: the fixture and whether Synapse turned the limit on.
const STEPS: [(&str, bool); 2] = [
    ("10-charge-limit-levels.txt", true),
    ("11-charge-limit-off.txt", false),
];

/// The charge-limit requests of a fixture, in the order Synapse sent them.
fn captured_limit(fixture: &str) -> Vec<[u8; razer_proto::REPORT_LEN]> {
    common::captured_requests()
        .into_iter()
        .filter(|request| {
            request.fixture == fixture && [request.bytes[6], request.bytes[7]] == SET_CHARGE_LIMIT
        })
        .map(|request| request.bytes)
        .collect()
}

fn limit() -> ChargeLimit {
    BLADE_18
        .charge_limit
        .expect("the Blade 18 has a charge limit")
}

#[test]
fn on_and_off_match_their_golden_bytes() {
    for (fixture, on) in STEPS {
        let golden = captured_limit(fixture);
        assert_eq!(golden.len(), 1, "{fixture}");
        let command = limit().command(on).unwrap();

        let built = Request::new(golden[0][1], command.id, &command.args)
            .unwrap()
            .to_bytes();

        assert_eq!(built, golden[0], "{fixture}");
    }
}

#[test]
fn the_blade_18_stops_at_80_percent() {
    assert_eq!(limit().percent, 80);
}

#[test]
fn a_percent_outside_the_safe_range_sends_nothing() {
    for percent in [0, 49, 81, 100, 127, 255] {
        assert_eq!(
            ChargeLimit { percent }.command(true),
            Err(ChargeLimitError::UnsafePercent(percent)),
            "{percent}"
        );
        assert_eq!(
            ChargeLimit { percent }.command(false),
            Err(ChargeLimitError::UnsafePercent(percent)),
            "{percent}"
        );
    }
    assert!(ChargeLimit { percent: 50 }.command(true).is_ok());
}

#[test]
fn the_query_reads_the_limit() {
    let query = ChargeLimit::query();

    assert_eq!(query.id, 0x0792);
    assert_eq!(query.args, [0x00]);
}

#[test]
fn a_reply_at_the_model_percent_says_on_or_off() {
    // Read on the laptop 2026-11-06: `0792` replied `[D0]`.
    assert_eq!(limit().from_reply(&[0xD0]), Some(true));
    assert_eq!(limit().from_reply(&[0x50]), Some(false));
}

#[test]
fn a_reply_at_another_percent_or_empty_is_not_read() {
    assert_eq!(limit().from_reply(&[0xBC]), None);
    assert_eq!(limit().from_reply(&[0x3C]), None);
    assert_eq!(limit().from_reply(&[0x80]), None);
    assert_eq!(limit().from_reply(&[0x00]), None);
    assert_eq!(limit().from_reply(&[]), None);
}
