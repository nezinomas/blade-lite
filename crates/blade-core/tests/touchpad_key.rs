//! The touchpad key: lit while the touchpad is off, as Synapse lit it.

use std::io;

use blade_core::{Controller, Query, TouchpadKeyError, light_touchpad_key};
use platform::Registry;
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Command, ModelProfile, TouchpadKey};

/// The touchpad state as Windows keeps it, or a failed read.
struct FakeRegistry(Option<u32>);

impl Registry for FakeRegistry {
    fn read_user_dword(&self, _: &str, _: &str) -> io::Result<u32> {
        self.0
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

/// A controller that keeps the key light it is sent and reads it back.
#[derive(Default)]
struct FakeController {
    lit: Option<bool>,
    ran: Vec<Command>,
    /// Reads back this in place of the kept state.
    reads: Option<Vec<u8>>,
    fails: bool,
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        assert_eq!(*command, key().query());
        if let Some(reads) = &self.reads {
            return Ok(reads.clone());
        }
        let lit = self.lit.ok_or(DeviceError::Busy)?;
        Ok(vec![0x00, key().led, u8::from(lit)])
    }
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        for command in commands {
            self.lit = key().from_reply(&command.args);
        }
        self.ran.extend_from_slice(commands);
        Ok(())
    }
}

fn key() -> TouchpadKey {
    BLADE_18.touchpad_key.unwrap()
}

const NO_KEY: ModelProfile = ModelProfile {
    touchpad_key: None,
    ..BLADE_18
};

fn showing(lit: bool) -> FakeController {
    FakeController {
        lit: Some(lit),
        ..FakeController::default()
    }
}

#[test]
fn the_key_lights_when_the_touchpad_is_off() {
    let mut controller = showing(false);

    light_touchpad_key(&FakeRegistry(Some(0)), &mut controller, &BLADE_18).unwrap();

    assert_eq!(controller.ran, [key().command(true)]);
    assert_eq!(controller.lit, Some(true));
}

#[test]
fn the_key_goes_dark_when_the_touchpad_is_on() {
    let mut controller = showing(true);

    light_touchpad_key(&FakeRegistry(Some(1)), &mut controller, &BLADE_18).unwrap();

    assert_eq!(controller.ran, [key().command(false)]);
    assert_eq!(controller.lit, Some(false));
}

#[test]
fn nothing_is_sent_if_the_key_already_shows_the_state() {
    for (touchpad, lit) in [(0, true), (1, false)] {
        let mut controller = showing(lit);

        light_touchpad_key(&FakeRegistry(Some(touchpad)), &mut controller, &BLADE_18).unwrap();

        assert_eq!(controller.ran, [], "touchpad {touchpad}");
    }
}

#[test]
fn an_unreadable_light_is_set_and_read_back() {
    let mut controller = FakeController {
        reads: Some(vec![0x00, key().led, 0x07]),
        ..FakeController::default()
    };

    let result = light_touchpad_key(&FakeRegistry(Some(0)), &mut controller, &BLADE_18);

    assert_eq!(controller.ran, [key().command(true)]);
    assert!(
        matches!(
            result,
            Err(TouchpadKeyError::NotConfirmed {
                wanted: true,
                read: None
            })
        ),
        "{result:?}"
    );
}

#[test]
fn nothing_is_sent_if_the_touchpad_state_cant_be_read() {
    for registry in [FakeRegistry(None), FakeRegistry(Some(2))] {
        let mut controller = showing(false);

        let result = light_touchpad_key(&registry, &mut controller, &BLADE_18);

        assert!(
            matches!(result, Err(TouchpadKeyError::Touchpad(_))),
            "{result:?}"
        );
        assert_eq!(controller.ran, []);
    }
}

#[test]
fn a_model_without_the_key_is_left_alone() {
    let mut controller = showing(false);

    light_touchpad_key(&FakeRegistry(Some(0)), &mut controller, &NO_KEY).unwrap();

    assert_eq!(controller.ran, []);
}

#[test]
fn a_failed_send_is_an_error() {
    let mut controller = FakeController {
        fails: true,
        ..showing(false)
    };

    let result = light_touchpad_key(&FakeRegistry(Some(0)), &mut controller, &BLADE_18);

    assert!(
        matches!(result, Err(TouchpadKeyError::Device(_))),
        "{result:?}"
    );
}

#[test]
fn a_failed_first_read_still_sets_the_light() {
    let mut controller = FakeController::default();

    let result = light_touchpad_key(&FakeRegistry(Some(0)), &mut controller, &BLADE_18);

    // The fake keeps what it was sent, so the read back works.
    result.unwrap();
    assert_eq!(controller.ran, [key().command(true)]);
}
