//! Fan speeds and the CPU temperature, read from the controller.

mod common;

use razer_proto::{BLADE_18, Celsius, FanRpm, Request};

/// Command bytes of a fan speed read.
const FAN_READ: [u8; 2] = [0x0D, 0x88];

#[test]
fn each_captured_fan_reply_parses_into_its_speed() {
    let replies: Vec<_> = common::captured_replies()
        .into_iter()
        .filter(|reply| reply.bytes[6..8] == FAN_READ)
        .collect();

    for reply in &replies {
        let args = &reply.bytes[8..11];
        let speed = FanRpm::from_reply(args);

        assert_eq!(
            speed,
            Some(FanRpm(u16::from(args[2]) * 100)),
            "{}",
            reply.fixture
        );
    }
    assert!(!replies.is_empty(), "no captured fan reads");
}

#[test]
fn the_idle_capture_reads_both_fans() {
    // 00-idle-dashboard: fan 1 replied 11, fan 2 replied 12.
    assert_eq!(FanRpm::from_reply(&[0x01, 0x01, 0x11]), Some(FanRpm(1700)));
    assert_eq!(FanRpm::from_reply(&[0x01, 0x02, 0x12]), Some(FanRpm(1800)));
}

#[test]
fn the_fan_queries_match_the_captured_requests() {
    let captured: Vec<_> = common::captured_requests()
        .into_iter()
        .filter(|request| request.bytes[6..8] == FAN_READ)
        .take(2)
        .map(|request| request.bytes)
        .collect();

    let built: Vec<_> = FanRpm::queries(&BLADE_18)
        .iter()
        .zip(&captured)
        .map(|(query, golden)| {
            Request::new(golden[1], query.id, &query.args)
                .unwrap()
                .to_bytes()
        })
        .collect();

    assert_eq!(built, captured);
}

#[test]
fn a_short_fan_reply_is_none() {
    assert_eq!(FanRpm::from_reply(&[0x01, 0x01]), None);
}

#[test]
fn the_temperature_reply_gives_the_cpu_in_celsius() {
    // Read on the laptop (2026-11-02). Synapse never sends 0D85.
    assert_eq!(
        Celsius::cpu_from_reply(&[0x02, 52, 0, 0]),
        Some(Celsius(52))
    );
}

#[test]
fn a_zero_temperature_is_not_reported() {
    assert_eq!(Celsius::cpu_from_reply(&[0x02, 0, 0, 0]), None);
    assert_eq!(Celsius::cpu_from_reply(&[]), None);
}

#[test]
fn the_temperature_query_is_0d85() {
    let query = Celsius::query();

    assert_eq!(
        (query.id, query.args),
        (0x0D85, vec![0x00, 0x01, 0x00, 0x00])
    );
}
