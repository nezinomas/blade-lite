mod common;

use razer_proto::{
    BLADE_18, KeyGrid, ModelProfile, ParseRgbError, REPORT_LEN, Request, Rgb, static_colour,
};

/// The captured colour steps: the fixture and the colour Synapse set last.
const STEPS: [(&str, Rgb); 3] = [
    ("17-colour-a-red.txt", Rgb::new(0xFF, 0x00, 0x00)),
    ("18-colour-b-blue.txt", Rgb::new(0x00, 0x00, 0xFF)),
    ("22-colour-green.txt", Rgb::new(0x00, 0xC0, 0x00)),
];

const SET_EFFECT: [u8; 2] = [0x03, 0x0A];
const FRAME_ROW: [u8; 2] = [0x03, 0x0B];

fn requests(fixture: &str, command: [u8; 2]) -> Vec<[u8; REPORT_LEN]> {
    common::captured_requests()
        .into_iter()
        .filter(|request| request.fixture == fixture && request.bytes[6..8] == command)
        .map(|request| request.bytes)
        .collect()
}

/// Builds the bytes of each command with the transaction ID of its golden twin.
fn built(commands: &[razer_proto::Command], golden: &[[u8; REPORT_LEN]]) -> Vec<[u8; REPORT_LEN]> {
    commands
        .iter()
        .zip(golden)
        .map(|(command, golden)| {
            Request::new(golden[1], command.id, &command.args)
                .unwrap()
                .to_bytes()
        })
        .collect()
}

#[test]
fn the_last_frame_of_each_step_matches_its_golden_bytes() {
    for (fixture, colour) in STEPS {
        let rows = requests(fixture, FRAME_ROW);
        let golden = &rows[rows.len() - 7..];
        let commands = static_colour(colour, &BLADE_18);

        assert_eq!(commands.len(), 8, "{fixture}");
        assert_eq!(built(&commands[1..], golden), golden, "{fixture}");
    }
}

#[test]
fn the_custom_frame_effect_comes_first() {
    for (fixture, colour) in STEPS.into_iter().take(2) {
        let golden = requests(fixture, SET_EFFECT);
        let commands = static_colour(colour, &BLADE_18);

        assert_eq!(built(&commands[..1], &golden), golden, "{fixture}");
    }
}

#[test]
fn one_row_is_built_per_row_of_the_model() {
    let small = ModelProfile {
        keys: KeyGrid {
            rows: 2,
            columns: 3,
            no_key: &[(1, 1)],
        },
        ..BLADE_18
    };

    let commands = static_colour(Rgb::new(1, 2, 3), &small);

    let rows: Vec<_> = commands[1..].iter().map(|command| &command.args).collect();
    assert_eq!(
        rows,
        [
            &vec![0xFF, 0, 0, 2, 1, 2, 3, 1, 2, 3, 1, 2, 3],
            &vec![0xFF, 1, 0, 2, 1, 2, 3, 0, 0, 0, 1, 2, 3],
        ]
    );
}

#[test]
fn rgb_is_read_from_hash_rrggbb() {
    assert_eq!("#00C0FF".parse(), Ok(Rgb::new(0x00, 0xC0, 0xFF)));
    assert_eq!("#ff8000".parse(), Ok(Rgb::new(0xFF, 0x80, 0x00)));
}

#[test]
fn other_text_is_not_an_rgb() {
    for text in [
        "", "#", "00C0FF", "#00C0F", "#00C0FFF", "#00C0FG", "#+0C0FF", "#00É0F",
    ] {
        assert_eq!(text.parse::<Rgb>(), Err(ParseRgbError), "{text}");
    }
}
