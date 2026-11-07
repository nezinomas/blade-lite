//! The touchpad key: red in the frame while the touchpad is off.

use blade_core::{Controller, Lighting, paint_touchpad_key};
use razer_device::DeviceError;
use razer_proto::{
    BLADE_18, Command, ModelProfile, Rgb, static_colour, static_colour_touchpad_off,
};

const LIGHTING: Lighting = Lighting {
    brightness: 36,
    colour: Rgb::new(0x44, 0xD6, 0x2C),
};

/// Keeps what it runs, or fails every run.
#[derive(Default)]
struct FakeController {
    ran: Vec<Command>,
    fails: bool,
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

#[test]
fn red_paints_the_colour_with_the_key_red_and_no_brightness() {
    let mut controller = FakeController::default();

    paint_touchpad_key(true, LIGHTING, &BLADE_18, &mut controller).unwrap();

    assert_eq!(
        controller.ran,
        static_colour_touchpad_off(LIGHTING.colour, &BLADE_18)
    );
}

#[test]
fn not_red_paints_the_plain_colour() {
    let mut controller = FakeController::default();

    paint_touchpad_key(false, LIGHTING, &BLADE_18, &mut controller).unwrap();

    assert_eq!(controller.ran, static_colour(LIGHTING.colour, &BLADE_18));
}

#[test]
fn a_model_without_the_key_is_left_alone() {
    let no_key = ModelProfile {
        touchpad_key: None,
        ..BLADE_18
    };
    let mut controller = FakeController::default();

    paint_touchpad_key(true, LIGHTING, &no_key, &mut controller).unwrap();

    assert_eq!(controller.ran, []);
}

#[test]
fn a_failed_send_is_an_error() {
    let mut controller = FakeController {
        fails: true,
        ..FakeController::default()
    };

    assert!(paint_touchpad_key(true, LIGHTING, &BLADE_18, &mut controller).is_err());
}
