mod common;

use razer_proto::{Brightness, BrightnessError, Request};

/// The captured brightness steps: the fixture and the percent Synapse set.
const STEPS: [(&str, u8); 4] = [
    ("13-brightness-0.txt", 0),
    ("14-brightness-1.txt", 1),
    ("15-brightness-50.txt", 50),
    ("16-brightness-100.txt", 100),
];

/// The last two `0303` requests of a fixture: the keyboard, then the underside.
fn captured_brightness(fixture: &str) -> Vec<[u8; razer_proto::REPORT_LEN]> {
    let requests: Vec<_> = common::captured_requests()
        .into_iter()
        .filter(|request| request.fixture == fixture && request.bytes[6..8] == [0x03, 0x03])
        .map(|request| request.bytes)
        .collect();
    requests[requests.len() - 2..].to_vec()
}

#[test]
fn each_step_matches_its_golden_bytes() {
    for (fixture, percent) in STEPS {
        let golden = captured_brightness(fixture);
        let commands = Brightness::new(percent).unwrap().commands();

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
fn above_100_percent_cannot_be_built() {
    assert_eq!(Brightness::new(101), Err(BrightnessError::OutOfRange(101)));
}
