mod common;

use razer_proto::{BLADE_18, Logo, Request};

const SET_LOGO: [u8; 2] = [0x03, 0x00];
const SET_LOGO_EFFECT: [u8; 2] = [0x03, 0x02];

/// The captured steps: the fixture and whether Synapse turned the logo on.
const STEPS: [(&str, bool); 2] = [("19-logo-off.txt", false), ("20-logo-on.txt", true)];

/// The logo requests of a fixture, in the order Synapse sent them.
fn captured_logo(fixture: &str) -> Vec<[u8; razer_proto::REPORT_LEN]> {
    common::captured_requests()
        .into_iter()
        .filter(|request| {
            request.fixture == fixture
                && [SET_LOGO_EFFECT, SET_LOGO].contains(&[request.bytes[6], request.bytes[7]])
        })
        .map(|request| request.bytes)
        .collect()
}

fn logo() -> Logo {
    BLADE_18.logo.expect("the Blade 18 has a lid logo")
}

#[test]
fn on_and_off_match_their_golden_bytes() {
    for (fixture, on) in STEPS {
        let golden = captured_logo(fixture);
        assert_eq!(golden.len(), 2, "{fixture}");
        let commands = logo().commands(on);

        let built: Vec<_> = commands
            .iter()
            .zip(&golden)
            .map(|(command, golden)| {
                Request::new(golden[1], command.id, &command.args)
                    .unwrap()
                    .to_bytes()
            })
            .collect();

        assert_eq!(commands.len(), 2, "{fixture}");
        assert_eq!(built, golden, "{fixture}");
    }
}

#[test]
fn the_query_reads_the_logo_led() {
    let query = logo().query();

    assert_eq!(query.id, 0x0380);
    assert_eq!(query.args, [0x00, 0x04, 0x00]);
}

#[test]
fn a_reply_for_the_logo_led_says_on_or_off() {
    assert_eq!(logo().from_reply(&[0x00, 0x04, 0x01]), Some(true));
    assert_eq!(logo().from_reply(&[0x00, 0x04, 0x00]), Some(false));
}

#[test]
fn a_reply_for_another_led_or_an_odd_value_is_not_read() {
    assert_eq!(logo().from_reply(&[0x00, 0x05, 0x01]), None);
    assert_eq!(logo().from_reply(&[0x00, 0x05, 0x00]), None);
    assert_eq!(logo().from_reply(&[0x00, 0x04, 0x02]), None);
    assert_eq!(logo().from_reply(&[0x00, 0x04]), None);
    // Storage 1 holds another value, so its echo is not the state.
    assert_eq!(logo().from_reply(&[0x01, 0x04, 0x01]), None);
    assert_eq!(logo().from_reply(&[0x01, 0x04, 0x00]), None);
    assert_eq!(logo().from_reply(&[]), None);
}
