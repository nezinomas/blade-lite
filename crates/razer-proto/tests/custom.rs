mod common;

use razer_proto::{
    BLADE_18, Command, CpuBoost, Custom, CustomError, FanMode, GpuBoost, Request, Rpm,
};

const SET_PRESET: [u8; 2] = [0x0D, 0x02];
const SET_BOOST: [u8; 2] = [0x0D, 0x07];
const SET_FAN: [u8; 2] = [0x0D, 0x01];

/// The Custom requests of a fixture, in the order Synapse sent them.
fn captured_custom(fixture: &str) -> Vec<[u8; razer_proto::REPORT_LEN]> {
    common::captured_requests()
        .into_iter()
        .filter(|request| {
            request.fixture == fixture
                && [SET_PRESET, SET_BOOST, SET_FAN].contains(&[request.bytes[6], request.bytes[7]])
        })
        .map(|request| request.bytes)
        .collect()
}

/// The commands as reports, each with the transaction ID of its golden twin.
fn built_like(
    commands: &[Command],
    golden: &[[u8; razer_proto::REPORT_LEN]],
) -> Vec<[u8; razer_proto::REPORT_LEN]> {
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

fn custom(cpu: u8, gpu: u8, fan: FanMode) -> Custom {
    Custom {
        cpu: CpuBoost::new(cpu, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(gpu, &BLADE_18).unwrap(),
        fan,
    }
}

#[test]
fn custom_with_auto_fan_matches_its_golden_bytes() {
    // Synapse switched to Custom with CPU 1 and GPU 0, fans on auto.
    let golden = &captured_custom("04-custom-cpu-boost.txt")[..6];
    let commands = custom(1, 0, FanMode::Auto).commands(&BLADE_18);

    assert_eq!(commands.len(), 6);
    assert_eq!(built_like(&commands, golden), golden);
}

#[test]
fn custom_with_manual_fan_matches_its_golden_bytes() {
    // Synapse switched to manual fan with CPU 3, GPU 3 and both fans at 1700.
    let golden = &captured_custom("06-fan-manual-min.txt")[..8];
    let fan = FanMode::Manual(Rpm::new(1700, &BLADE_18).unwrap());
    let commands = custom(3, 3, fan).commands(&BLADE_18);

    assert_eq!(commands.len(), 8);
    assert_eq!(built_like(&commands, golden), golden);
}

#[test]
fn every_boost_level_matches_the_capture() {
    // Each 0D07 Synapse sent: args [1, cluster, level].
    let captured: Vec<_> = ["04-custom-cpu-boost.txt", "05-custom-gpu-boost.txt"]
        .into_iter()
        .flat_map(captured_custom)
        .filter(|bytes| bytes[6..8] == SET_BOOST)
        .map(|bytes| (bytes[9], bytes[10]))
        .collect();

    for (cluster, level) in captured {
        let commands = custom(level, level, FanMode::Auto).commands(&BLADE_18);
        let boost = commands
            .iter()
            .find(|command| command.id == 0x0D07 && command.args[1] == cluster)
            .unwrap();
        assert_eq!(boost.args, [1, cluster, level]);
    }
}

#[test]
fn the_fan_targets_alone_are_the_last_commands_of_a_manual_custom() {
    let fan = FanMode::Manual(Rpm::new(3600, &BLADE_18).unwrap());
    let full = custom(3, 3, fan).commands(&BLADE_18);

    let targets = custom(3, 3, fan).fan_target_commands(&BLADE_18);

    assert_eq!(targets.len(), BLADE_18.custom.fan_zones.len());
    assert_eq!(targets, full[full.len() - targets.len()..]);
    assert!(targets.iter().all(|c| c.id == 0x0D01 && c.args[2] == 36));
}

#[test]
fn a_custom_with_auto_fans_has_no_fan_targets_to_send_again() {
    assert_eq!(
        custom(1, 1, FanMode::Auto).fan_target_commands(&BLADE_18),
        []
    );
}

#[test]
fn going_back_to_auto_sends_no_fan_target() {
    let commands = custom(1, 1, FanMode::Auto).commands(&BLADE_18);

    assert!(commands.iter().all(|command| command.id != 0x0D01));
    assert!(
        commands
            .iter()
            .filter(|command| command.id == 0x0D02)
            .all(|command| command.args[2..] == [0x04, 0x00])
    );
}

#[test]
fn a_boost_above_the_model_levels_cannot_be_built() {
    assert_eq!(
        CpuBoost::new(4, &BLADE_18),
        Err(CustomError::BoostTooHigh(4))
    );
    assert_eq!(
        GpuBoost::new(4, &BLADE_18),
        Err(CustomError::BoostTooHigh(4))
    );
}

#[test]
fn a_fan_speed_outside_the_model_range_cannot_be_built() {
    let range = BLADE_18.custom.fan_rpm;

    assert_eq!(
        Rpm::new(range.min - 100, &BLADE_18),
        Err(CustomError::RpmOutOfRange(range.min - 100))
    );
    assert_eq!(
        Rpm::new(range.max + 100, &BLADE_18),
        Err(CustomError::RpmOutOfRange(range.max + 100))
    );
    assert!(Rpm::new(range.min, &BLADE_18).is_ok());
    assert!(Rpm::new(range.max, &BLADE_18).is_ok());
}

#[test]
fn a_fan_speed_that_does_not_fit_in_a_byte_cannot_be_built() {
    // A profile mistake must not let a speed through that the byte can't hold.
    let mut model = BLADE_18;
    model.custom.fan_rpm.max = u16::MAX;

    assert_eq!(
        Rpm::new(25_600, &model),
        Err(CustomError::RpmOutOfRange(25_600))
    );
    assert!(Rpm::new(25_500, &model).is_ok());
}

#[test]
fn a_fan_speed_must_be_whole_hundreds() {
    // The controller takes rpm / 100 in one byte.
    assert_eq!(
        Rpm::new(2050, &BLADE_18),
        Err(CustomError::RpmNotHundreds(2050))
    );
}

#[test]
fn the_custom_mode_byte_comes_from_the_model() {
    let mut model = BLADE_18;
    model.custom.mode = 0x09;
    let sent: Vec<u8> = custom(1, 1, FanMode::Auto)
        .commands(&model)
        .iter()
        .filter(|command| command.id == 0x0D02)
        .map(|command| command.args[2])
        .collect();

    assert_eq!(sent, [0x09; 4]);
}
