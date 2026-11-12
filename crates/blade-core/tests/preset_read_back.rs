//! The preset read back after a send (ticket 152).

use blade_core::{Config, Preset, Query, preset_differs};
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
