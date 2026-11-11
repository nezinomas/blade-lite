//! The red keys: the touchpad key while the touchpad is off, the mic key
//! while the mic is muted.

use blade_core::{Controller, Lighting, paint_red_keys};
use razer_device::DeviceError;
use razer_proto::{
    BLADE_18, Command, ModelProfile, RedKeys, Rgb, static_colour, static_colour_red,
    static_colour_touchpad_off,
};

const LIGHTING: Lighting = Lighting {
    brightness: 36,
    colour: Rgb::new(0x44, 0xD6, 0x2C),
    vapor_light: true,
};
const TOUCHPAD: RedKeys = RedKeys {
    touchpad: true,
    mic: false,
};
const MIC: RedKeys = RedKeys {
    touchpad: false,
    mic: true,
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

    paint_red_keys(TOUCHPAD, LIGHTING, &BLADE_18, &mut controller).unwrap();

    assert_eq!(
        controller.ran,
        static_colour_touchpad_off(LIGHTING.colour, &BLADE_18)
    );
}

#[test]
fn not_red_paints_the_plain_colour() {
    let mut controller = FakeController::default();

    paint_red_keys(RedKeys::default(), LIGHTING, &BLADE_18, &mut controller).unwrap();

    assert_eq!(controller.ran, static_colour(LIGHTING.colour, &BLADE_18));
}

#[test]
fn a_model_without_the_keys_is_left_alone() {
    let no_key = ModelProfile {
        touchpad_key: None,
        mic_key: None,
        ..BLADE_18
    };
    let mut controller = FakeController::default();

    paint_red_keys(TOUCHPAD, LIGHTING, &no_key, &mut controller).unwrap();

    assert_eq!(controller.ran, []);
}

#[test]
fn the_mic_key_is_painted_on_a_model_without_the_touchpad_key() {
    let mic_only = ModelProfile {
        touchpad_key: None,
        ..BLADE_18
    };
    let mut controller = FakeController::default();

    paint_red_keys(MIC, LIGHTING, &mic_only, &mut controller).unwrap();

    assert_eq!(
        controller.ran,
        static_colour_red(LIGHTING.colour, &mic_only, MIC)
    );
}

#[test]
fn a_failed_send_is_an_error() {
    let mut controller = FakeController {
        fails: true,
        ..FakeController::default()
    };

    assert!(paint_red_keys(TOUCHPAD, LIGHTING, &BLADE_18, &mut controller).is_err());
}
