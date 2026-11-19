//! The preset read back after a send (ticket 152).

use blade_core::{Config, Preset, Query, manual_fan_reads, preset_differs};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Command};

/// Answers each mode query with `reply(zone)`.
struct Replies(fn(u8) -> Option<Vec<u8>>, Vec<Command>);

impl Query for Replies {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        self.1.push(command.clone());
        (self.0)(command.args[1]).ok_or(DeviceError::Busy)
    }
}

fn silent() -> Config {
    Config {
        preset: Preset::Silent,
        ..Config::default()
    }
}

#[test]
fn a_preset_that_reads_back_as_sent_differs_nowhere() {
    let mut replies = Replies(|zone| Some(vec![1, zone, 0x05, 0]), Vec::new());

    assert_eq!(
        preset_differs(&silent(), &BLADE_18, &mut replies).unwrap(),
        None
    );
    assert_eq!(replies.1.len(), BLADE_18.perf_zones.len());
}

#[test]
fn a_zone_that_reads_back_otherwise_is_named() {
    let mut replies = Replies(
        |zone| Some(vec![1, zone, if zone == 2 { 0 } else { 0x05 }, 0]),
        Vec::new(),
    );

    let differs = preset_differs(&silent(), &BLADE_18, &mut replies)
        .unwrap()
        .unwrap();

    assert_eq!(differs, "zone 2 reads 01 02 00 00, sent 01 02 05 00");
}

#[test]
fn a_short_reply_is_no_reading() {
    let mut replies = Replies(|zone| Some(vec![1, zone]), Vec::new());

    assert!(matches!(
        preset_differs(&silent(), &BLADE_18, &mut replies),
        Err(DeviceError::ShortReply)
    ));
}

#[test]
fn a_value_the_model_refuses_reads_nothing() {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(9000);
    let mut replies = Replies(|_| None, Vec::new());

    assert_eq!(
        preset_differs(&config, &BLADE_18, &mut replies).unwrap(),
        None
    );
    assert_eq!(replies.1, Vec::<Command>::new());
}

/// Answers each query with what the last write of its id and second
/// arg set, as the controller does (ticket 254), but a fan target of
/// `stuck` on fan 1.
struct Controller {
    stuck: u8,
    asked: Vec<Command>,
}

impl Query for Controller {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        self.asked.push(command.clone());
        let which = command.args[1];
        Ok(match command.id {
            0x0D82 => vec![1, which, 0x04, 0x01],
            0x0D87 => vec![1, which, 1],
            0x0D81 if which == 1 => vec![1, which, self.stuck],
            0x0D81 => vec![1, which, 30],
            _ => return Err(DeviceError::Busy),
        })
    }
}

fn custom_3000() -> Config {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.cpu_boost = 1;
    config.custom.gpu_boost = 1;
    config.custom.fan_rpm = Some(3000);
    config
}

#[test]
fn custom_reads_back_its_boosts_and_fan_targets_too() {
    let mut controller = Controller {
        stuck: 30,
        asked: Vec::new(),
    };

    assert_eq!(
        preset_differs(&custom_3000(), &BLADE_18, &mut controller).unwrap(),
        None
    );
    let ids: Vec<u16> = controller.asked.iter().map(|c| c.id).collect();
    let zones = BLADE_18.perf_zones.len();
    assert_eq!(ids.len(), zones + 4);
    assert_eq!(ids[zones..], [0x0D87, 0x0D87, 0x0D81, 0x0D81]);
}

#[test]
fn a_fan_target_the_controller_did_not_take_is_named() {
    let mut controller = Controller {
        stuck: 20,
        asked: Vec::new(),
    };

    let differs = preset_differs(&custom_3000(), &BLADE_18, &mut controller)
        .unwrap()
        .unwrap();

    assert_eq!(differs, "fan 1 target reads 01 01 14, sent 01 01 1E");
}

#[test]
fn a_failed_read_after_a_difference_keeps_the_difference() {
    /// Zone 2 differs, then every boost read fails.
    struct BoostFails;
    impl Query for BoostFails {
        fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
            let zone = command.args[1];
            match command.id {
                0x0D82 => Ok(vec![1, zone, if zone == 2 { 0 } else { 0x04 }, 0x01]),
                _ => Err(DeviceError::Busy),
            }
        }
    }

    let differs = preset_differs(&custom_3000(), &BLADE_18, &mut BoostFails)
        .unwrap()
        .unwrap();

    assert_eq!(differs, "zone 2 reads 01 02 00 01, sent 01 02 04 01");
}

#[test]
fn a_manual_fan_is_read_in_any_zone() {
    let mut one = Replies(
        |zone| {
            Some(vec![
                1,
                zone,
                if zone == 3 { 0x04 } else { 0 },
                u8::from(zone == 3),
            ])
        },
        Vec::new(),
    );
    assert!(manual_fan_reads(&BLADE_18, &mut one).unwrap());

    let mut none = Replies(|zone| Some(vec![1, zone, 0x04, 0]), Vec::new());
    assert!(!manual_fan_reads(&BLADE_18, &mut none).unwrap());
    assert_eq!(none.1.len(), BLADE_18.perf_zones.len());

    let mut fails = Replies(|_| None, Vec::new());
    assert!(manual_fan_reads(&BLADE_18, &mut fails).is_err());
}
