mod common;

use razer_proto::{BLADE_18, ModelProfile, PerfMode, PresetError, Request};

/// The captured presets: the fixture and the preset Synapse set.
const STEPS: [(&str, PerfMode); 3] = [
    ("01-preset-silent.txt", PerfMode::Silent),
    ("02-preset-balanced.txt", PerfMode::Balanced),
    ("03-preset-performance.txt", PerfMode::Performance),
];

const SET_PRESET: [u8; 2] = [0x0D, 0x02];

/// The `0D02` requests of a fixture: one per zone.
fn captured_presets(fixture: &str) -> Vec<[u8; razer_proto::REPORT_LEN]> {
    common::captured_requests()
        .into_iter()
        .filter(|request| request.fixture == fixture && request.bytes[6..8] == SET_PRESET)
        .map(|request| request.bytes)
        .collect()
}

#[test]
fn each_preset_matches_its_golden_bytes() {
    for (fixture, preset) in STEPS {
        let golden = captured_presets(fixture);
        let commands = preset.commands(&BLADE_18).unwrap();

        let built: Vec<_> = commands
            .iter()
            .zip(&golden)
            .map(|(command, golden)| {
                Request::new(golden[1], command.id, &command.args)
                    .unwrap()
                    .to_bytes()
            })
            .collect();

        assert_eq!(commands.len(), 4, "{fixture}");
        assert_eq!(built, golden, "{fixture}");
    }
}

#[test]
fn one_query_is_built_per_zone() {
    let queries = PerfMode::queries(&BLADE_18);

    let zones: Vec<_> = queries.iter().map(|query| query.args[1]).collect();
    assert!(queries.iter().all(|query| query.id == 0x0D82));
    assert_eq!(zones, [1, 2, 3, 4]);
}

#[test]
fn a_reply_reads_back_as_its_preset() {
    for (_, preset) in STEPS {
        for command in preset.commands(&BLADE_18).unwrap() {
            let zone = command.args[1];

            assert_eq!(PerfMode::from_reply(zone, &command.args), Some(preset));
        }
    }
}

#[test]
fn an_unknown_mode_reads_back_as_none() {
    // 4 is Custom, which this stage does not read yet.
    assert_eq!(PerfMode::from_reply(0x01, &[0x01, 0x01, 0x04, 0x01]), None);
    assert_eq!(PerfMode::from_reply(0x01, &[0x01]), None);
}

#[test]
fn a_reply_for_another_zone_or_storage_is_none() {
    // Balanced (0) for zone 2, read as zone 1's reply.
    assert_eq!(PerfMode::from_reply(0x01, &[0x01, 0x02, 0x00, 0x00]), None);
    assert_eq!(PerfMode::from_reply(0x01, &[0x00, 0x01, 0x00, 0x00]), None);
}

#[test]
fn a_preset_the_model_lacks_cannot_be_built() {
    let no_silent = ModelProfile {
        presets: &[PerfMode::Balanced, PerfMode::Performance],
        ..BLADE_18
    };

    assert_eq!(
        PerfMode::Silent.commands(&no_silent),
        Err(PresetError::NotOffered(PerfMode::Silent))
    );
}
