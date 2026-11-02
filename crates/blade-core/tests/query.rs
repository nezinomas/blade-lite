//! What the app reads back from the controller: the chosen native mode, the
//! CPU temperature and the fan speeds.

use blade_core::{Query, chosen_native_mode, read_controller};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Celsius, Command, FanRpm, NativeMode};

/// Replies to the commands it knows, and fails the rest.
#[derive(Default)]
struct FakeQuery(Vec<(Command, Vec<u8>)>);

impl FakeQuery {
    fn reply(mut self, command: Command, args: &[u8]) -> Self {
        self.0.push((command, args.to_vec()));
        self
    }
}

impl Query for FakeQuery {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        self.0
            .iter()
            .find(|(known, _)| known == command)
            .map(|(_, args)| args.clone())
            .ok_or(DeviceError::Busy)
    }
}

fn fan_query(index: usize) -> Command {
    FanRpm::queries(&BLADE_18)[index].clone()
}

#[test]
fn the_chosen_native_mode_is_read_back() {
    let mut controller = FakeQuery::default().reply(NativeMode::query(), &[1]);

    let chosen = chosen_native_mode(&mut controller, &BLADE_18).unwrap();

    assert_eq!(chosen, Some(BLADE_18.native_modes[1]));
}

#[test]
fn a_mode_the_model_lacks_is_none() {
    let mut controller = FakeQuery::default().reply(NativeMode::query(), &[7]);

    assert_eq!(
        chosen_native_mode(&mut controller, &BLADE_18).unwrap(),
        None
    );
}

#[test]
fn a_failed_native_mode_read_is_an_error() {
    let result = chosen_native_mode(&mut FakeQuery::default(), &BLADE_18);

    assert!(matches!(result, Err(DeviceError::Busy)));
}

#[test]
fn the_cpu_temperature_and_each_fan_are_read() {
    let mut controller = FakeQuery::default()
        .reply(Celsius::query(), &[2, 52, 0, 0])
        .reply(fan_query(0), &[1, 1, 17])
        .reply(fan_query(1), &[1, 2, 18]);

    let readings = read_controller(&mut controller, &BLADE_18);

    assert_eq!(readings.cpu, Some(Celsius(52)));
    assert_eq!(readings.fans, [Some(FanRpm(1700)), Some(FanRpm(1800))]);
}

#[test]
fn a_failed_read_leaves_only_its_value_empty() {
    let mut controller = FakeQuery::default().reply(fan_query(1), &[1, 2, 18]);

    let readings = read_controller(&mut controller, &BLADE_18);

    assert_eq!(readings.cpu, None);
    assert_eq!(readings.fans, [None, Some(FanRpm(1800))]);
}
