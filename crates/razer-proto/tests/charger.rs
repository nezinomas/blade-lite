mod common;

use razer_proto::{BLADE_18, Charger, Request};

const GET_CHARGER: [u8; 2] = [0x07, 0x8C];

fn charger() -> Charger {
    BLADE_18.charger.expect("the Blade 18 reports its charger")
}

#[test]
fn the_query_matches_synapses_bytes() {
    let golden = common::captured_requests()
        .into_iter()
        .find(|request| [request.bytes[6], request.bytes[7]] == GET_CHARGER)
        .expect("Synapse read the charger")
        .bytes;
    let query = Charger::query();

    let built = Request::new(golden[1], query.id, &query.args)
        .unwrap()
        .to_bytes();

    assert_eq!(built, golden);
}

#[test]
fn only_the_full_power_charger_counts_as_full_power() {
    // Read on the laptop: Razer charger, USB-C monitor, no charger.
    assert_eq!(charger().full_power(&[0x13, 0x13]), Some(true));
    assert_eq!(charger().full_power(&[0x04, 0x13]), Some(false));
    assert_eq!(charger().full_power(&[0x00, 0x13]), Some(false));
}

#[test]
fn a_full_power_byte_with_an_unseen_second_byte_is_weak() {
    assert_eq!(charger().full_power(&[0x13, 0x00]), Some(false));
}

#[test]
fn a_reply_of_another_length_is_not_understood() {
    assert_eq!(charger().full_power(&[]), None);
    assert_eq!(charger().full_power(&[0x13]), None);
    assert_eq!(charger().full_power(&[0x13, 0x13, 0x00]), None);
}
