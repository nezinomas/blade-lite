//! Reading the keyboard brightness the laptop set itself, and the vapor
//! light that follows it.

use blade_core::{Controller, Lighting, Query, keyboard_brightness, set_vapor_light};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Brightness, Command, ModelProfile, Rgb};

/// Replies to the brightness query with `reply`, or fails it with `None`.
/// Keeps every command it was asked to run, and fails runs while `fails`.
#[derive(Default)]
struct FakeController {
    reply: Option<Vec<u8>>,
    ran: Vec<Command>,
    fails: bool,
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        assert_eq!(Some(command), Brightness::query(&BLADE_18).as_ref());
        self.reply.clone().ok_or(DeviceError::Busy)
    }
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        self.ran.extend_from_slice(commands);
        Ok(())
    }
}

fn replying(args: &[u8]) -> FakeController {
    FakeController {
        reply: Some(args.to_vec()),
        ..FakeController::default()
    }
}

const NO_LIGHTS: ModelProfile = ModelProfile {
    brightness_leds: &[],
    vapor_light: None,
    ..BLADE_18
};

fn lighting(percent: u8, vapor_light: bool) -> Lighting {
    Lighting {
        brightness: percent,
        colour: Rgb::new(0x00, 0xB7, 0xC3),
        vapor_light,
    }
}

#[test]
fn the_level_is_read_as_a_percent() {
    let read = keyboard_brightness(&mut replying(&[1, 5, 255]), &BLADE_18).unwrap();

    assert_eq!(read, Some(Brightness::new(100).unwrap()));
}

#[test]
fn a_reply_for_another_led_reads_as_unknown() {
    let read = keyboard_brightness(&mut replying(&[1, 0x26, 255]), &BLADE_18).unwrap();

    assert_eq!(read, None);
}

#[test]
fn a_failed_read_is_an_error() {
    let result = keyboard_brightness(&mut FakeController::default(), &BLADE_18);

    assert!(matches!(result, Err(DeviceError::Busy)));
}

#[test]
fn a_model_without_brightness_is_not_asked() {
    // The fake would panic on any query but the Blade 18 keyboard's.
    let read = keyboard_brightness(&mut FakeController::default(), &NO_LIGHTS).unwrap();

    assert_eq!(read, None);
}

#[test]
fn the_vapor_light_takes_the_keyboard_level_while_on() {
    let mut controller = FakeController::default();

    set_vapor_light(lighting(50, true), &BLADE_18, &mut controller).unwrap();

    let vapor = BLADE_18.vapor_light.unwrap();
    assert_eq!(
        controller.ran,
        [vapor.command(Brightness::new(50).unwrap(), true)]
    );
}

#[test]
fn the_vapor_light_stays_off_while_off() {
    let mut controller = FakeController::default();

    set_vapor_light(lighting(50, false), &BLADE_18, &mut controller).unwrap();

    let vapor = BLADE_18.vapor_light.unwrap();
    assert_eq!(
        controller.ran,
        [vapor.command(Brightness::new(50).unwrap(), false)]
    );
    assert_eq!(controller.ran[0].args[2], 0);
}

#[test]
fn a_model_without_the_vapor_light_is_sent_nothing() {
    let mut controller = FakeController::default();

    set_vapor_light(lighting(50, true), &NO_LIGHTS, &mut controller).unwrap();

    assert!(controller.ran.is_empty());
}

#[test]
fn a_brightness_over_100_is_refused_unsent() {
    let mut controller = FakeController::default();

    let result = set_vapor_light(lighting(101, true), &BLADE_18, &mut controller);

    assert!(result.is_err());
    assert!(controller.ran.is_empty());
}

#[test]
fn a_failed_vapor_send_is_an_error() {
    let mut controller = FakeController {
        fails: true,
        ..FakeController::default()
    };

    let result = set_vapor_light(lighting(50, true), &BLADE_18, &mut controller);

    assert!(result.is_err());
}
