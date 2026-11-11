//! Applying the saved settings.

use std::cell::RefCell;
use std::rc::Rc;

use blade_core::{
    ApplyError, Config, Controller, CustomSettings, Lighting, Preset, apply, apply_lighting,
    apply_performance,
};
use razer_device::DeviceError;
use razer_proto::{
    BLADE_18, Brightness, BrightnessError, Command, CustomError, RedKeys, Rgb, static_colour,
};

/// One thing that happened, in order.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Sent(Command),
}

type Log = Rc<RefCell<Vec<Step>>>;

/// The brightness commands: the keyboard, then the vapor light.
fn brightness_commands(percent: u8, vapor_on: bool) -> Vec<Command> {
    let brightness = Brightness::new(percent).unwrap();
    let mut commands = brightness.commands(&BLADE_18);
    commands.push(BLADE_18.vapor_light.unwrap().command(brightness, vapor_on));
    commands
}

/// Records each command it is asked to run, or fails them all.
struct FakeController {
    log: Log,
    fails: bool,
    /// Takes this many runs, then fails each run after them.
    ok_runs: Option<usize>,
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        if let Some(left) = self.ok_runs.as_mut() {
            if *left == 0 {
                return Err(DeviceError::Busy);
            }
            *left -= 1;
        }
        let sent = commands.iter().cloned().map(Step::Sent);
        self.log.borrow_mut().extend(sent);
        Ok(())
    }
}

fn fakes() -> (FakeController, Log) {
    let log = Log::default();
    let controller = FakeController {
        log: Rc::clone(&log),
        fails: false,
        ok_runs: None,
    };
    (controller, log)
}

fn sent(id: u16, args: &[u8]) -> Step {
    Step::Sent(Command {
        id,
        args: args.to_vec(),
    })
}

/// The preset commands Synapse sends: one per zone, fans on auto.
fn preset_steps(mode: u8) -> Vec<Step> {
    (1..=4)
        .map(|zone| sent(0x0D02, &[1, zone, mode, 0]))
        .collect()
}

fn config(preset: Preset, custom: CustomSettings) -> Config {
    Config {
        preset,
        custom,
        ..Config::default()
    }
}

fn custom(cpu_boost: u8, gpu_boost: u8, fan_rpm: Option<u16>) -> CustomSettings {
    CustomSettings {
        cpu_boost,
        gpu_boost,
        fan_rpm,
    }
}

#[test]
fn each_preset_sends_its_mode_to_every_zone() {
    for (preset, mode) in [
        (Preset::Silent, 0x05),
        (Preset::Balanced, 0x00),
        (Preset::Performance, 0x02),
    ] {
        let (mut controller, log) = fakes();

        apply(
            &config(preset, CustomSettings::default()),
            &BLADE_18,
            &mut controller,
            RedKeys::default(),
        )
        .unwrap();

        assert_eq!(*log.borrow(), preset_steps(mode), "{preset:?}");
    }
}

#[test]
fn custom_with_a_manual_fan_sends_mode_then_boosts_then_fans() {
    let (mut controller, log) = fakes();

    let saved = config(Preset::Custom, custom(2, 3, Some(3000)));
    apply(&saved, &BLADE_18, &mut controller, RedKeys::default()).unwrap();

    let mut expected: Vec<Step> = (1..=4).map(|zone| sent(0x0D02, &[1, zone, 4, 1])).collect();
    expected.extend([
        sent(0x0D07, &[1, 1, 2]),
        sent(0x0D07, &[1, 2, 3]),
        sent(0x0D01, &[1, 1, 30]),
        sent(0x0D01, &[1, 2, 30]),
    ]);
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn custom_with_fans_on_auto_sends_no_fan_target() {
    let (mut controller, log) = fakes();

    apply(
        &config(Preset::Custom, custom(1, 1, None)),
        &BLADE_18,
        &mut controller,
        RedKeys::default(),
    )
    .unwrap();

    let mut expected: Vec<Step> = (1..=4).map(|zone| sent(0x0D02, &[1, zone, 4, 0])).collect();
    expected.extend([sent(0x0D07, &[1, 1, 1]), sent(0x0D07, &[1, 2, 1])]);
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn custom_values_the_model_does_not_take_send_nothing() {
    for (bad, error) in [
        (custom(4, 1, None), CustomError::BoostTooHigh(4)),
        (custom(1, 9, None), CustomError::BoostTooHigh(9)),
        (custom(1, 1, Some(1000)), CustomError::RpmOutOfRange(1000)),
        (custom(1, 1, Some(3050)), CustomError::RpmNotHundreds(3050)),
    ] {
        let (mut controller, log) = fakes();

        let result = apply(
            &config(Preset::Custom, bad),
            &BLADE_18,
            &mut controller,
            RedKeys::default(),
        );

        assert!(
            matches!(result, Err(ApplyError::Custom(found)) if found == error),
            "{bad:?} gave {result:?}"
        );
        assert!(log.borrow().is_empty(), "{bad:?} sent commands");
    }
}

#[test]
fn bad_custom_values_do_not_matter_while_another_preset_is_active() {
    let (mut controller, log) = fakes();

    let saved = config(Preset::Balanced, custom(9, 9, Some(1)));
    apply(&saved, &BLADE_18, &mut controller, RedKeys::default()).unwrap();

    assert_eq!(*log.borrow(), preset_steps(0x00));
}

#[test]
fn saved_lighting_is_sent_after_the_preset() {
    let (mut controller, log) = fakes();
    let colour = Rgb::new(0xE8, 0x11, 0x23);
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 50,
            colour,
            vapor_light: true,
        }),
        ..Config::default()
    };

    apply(&saved, &BLADE_18, &mut controller, RedKeys::default()).unwrap();

    let mut expected = preset_steps(0x00);
    let lighting = brightness_commands(50, true);
    let lighting = lighting.into_iter().chain(static_colour(colour, &BLADE_18));
    expected.extend(lighting.map(Step::Sent));
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn applying_the_performance_leaves_the_lighting_alone() {
    let (mut controller, log) = fakes();
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 50,
            colour: Rgb::new(1, 2, 3),
            vapor_light: true,
        }),
        ..Config::default()
    };

    apply_performance(&saved, &BLADE_18, &mut controller).unwrap();

    assert_eq!(*log.borrow(), preset_steps(0x00));
}

#[test]
fn applying_the_lighting_sends_brightness_then_colour() {
    let (mut controller, log) = fakes();
    let colour = Rgb::new(1, 2, 3);
    let lighting = Lighting {
        brightness: 20,
        colour,
        vapor_light: true,
    };

    apply_lighting(lighting, &BLADE_18, &mut controller, RedKeys::default()).unwrap();

    let brightness = brightness_commands(20, true);
    let expected: Vec<Step> = brightness
        .into_iter()
        .chain(static_colour(colour, &BLADE_18))
        .map(Step::Sent)
        .collect();
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn a_brightness_over_100_sends_nothing() {
    let (mut controller, log) = fakes();
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 101,
            colour: Rgb::new(0, 0, 0),
            vapor_light: true,
        }),
        ..Config::default()
    };

    let result = apply(&saved, &BLADE_18, &mut controller, RedKeys::default());

    assert!(matches!(
        result,
        Err(ApplyError::Brightness(BrightnessError::OutOfRange(101)))
    ));
    assert!(log.borrow().is_empty());
}

#[test]
fn a_device_error_is_returned() {
    let (mut controller, _) = fakes();
    controller.fails = true;

    let result = apply(
        &Config::default(),
        &BLADE_18,
        &mut controller,
        RedKeys::default(),
    );

    assert!(matches!(result, Err(ApplyError::Device(DeviceError::Busy))));
}

#[test]
fn the_preset_is_its_own_run_so_a_lighting_error_is_told_apart() {
    let (mut controller, log) = fakes();
    controller.ok_runs = Some(1);
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 50,
            colour: Rgb::new(0, 0, 0),
            vapor_light: true,
        }),
        ..Config::default()
    };

    let result = apply(&saved, &BLADE_18, &mut controller, RedKeys::default());

    assert!(matches!(
        result,
        Err(ApplyError::AfterPreset(DeviceError::Busy))
    ));
    assert_eq!(*log.borrow(), preset_steps(0x00));
}

#[test]
fn an_error_after_the_preset_is_said_in_plain_text() {
    let error = ApplyError::AfterPreset(DeviceError::Busy);

    assert_eq!(
        error.to_string(),
        "the preset was sent, but the controller did not take the lid logo or lighting, so the lid logo or keys may show part of the change: the controller stayed busy after 5 reads"
    );
}

#[test]
fn a_device_error_is_said_in_plain_text() {
    let error = ApplyError::Device(DeviceError::Busy);

    assert_eq!(
        error.to_string(),
        "the controller did not take a command, so the laptop may run part of the change: the controller stayed busy after 5 reads"
    );
}

#[test]
fn the_touchpad_key_is_red_in_the_one_frame_sent() {
    let (mut controller, log) = fakes();
    let lighting = Lighting {
        brightness: 50,
        colour: Rgb::new(0x44, 0xD6, 0x2C),
        vapor_light: true,
    };

    let touchpad = RedKeys {
        touchpad: true,
        mic: false,
    };
    apply_lighting(lighting, &BLADE_18, &mut controller, touchpad).unwrap();

    let expected: Vec<Step> = brightness_commands(50, true)
        .into_iter()
        .chain(razer_proto::static_colour_touchpad_off(
            lighting.colour,
            &BLADE_18,
        ))
        .map(|command| sent(command.id, &command.args))
        .collect();
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn the_vapor_light_off_gets_level_0_after_the_keyboard() {
    let (mut controller, log) = fakes();
    let colour = Rgb::new(1, 2, 3);
    let lighting = Lighting {
        brightness: 80,
        colour,
        vapor_light: false,
    };

    apply_lighting(lighting, &BLADE_18, &mut controller, RedKeys::default()).unwrap();

    let expected: Vec<Step> = brightness_commands(80, false)
        .into_iter()
        .chain(static_colour(colour, &BLADE_18))
        .map(Step::Sent)
        .collect();
    assert_eq!(*log.borrow(), expected);
    assert_eq!(
        log.borrow()[1],
        Step::Sent(Command {
            id: 0x0303,
            args: vec![1, 0x26, 0]
        })
    );
}

#[test]
fn a_model_without_a_vapor_light_gets_only_the_keyboard() {
    let (mut controller, log) = fakes();
    let model = razer_proto::ModelProfile {
        vapor_light: None,
        ..BLADE_18
    };
    let lighting = Lighting {
        brightness: 80,
        colour: Rgb::new(1, 2, 3),
        vapor_light: true,
    };

    apply_lighting(lighting, &model, &mut controller, RedKeys::default()).unwrap();

    let brightness = Brightness::new(80).unwrap().commands(&model);
    let expected: Vec<Step> = brightness
        .into_iter()
        .chain(static_colour(lighting.colour, &model))
        .map(Step::Sent)
        .collect();
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn a_saved_lid_logo_is_sent_after_the_preset_and_before_the_lighting() {
    let (mut controller, log) = fakes();
    let colour = Rgb::new(1, 2, 3);
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 50,
            colour,
            vapor_light: true,
        }),
        lid_logo: Some(false),
        ..Config::default()
    };

    apply(&saved, &BLADE_18, &mut controller, RedKeys::default()).unwrap();

    let mut expected = preset_steps(0x00);
    let logo = BLADE_18.logo.unwrap().commands(false);
    let lighting = brightness_commands(50, true);
    let lighting = lighting.into_iter().chain(static_colour(colour, &BLADE_18));
    expected.extend(logo.into_iter().chain(lighting).map(Step::Sent));
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn a_model_without_a_lid_logo_gets_no_logo_commands() {
    let (mut controller, log) = fakes();
    let model = razer_proto::ModelProfile {
        logo: None,
        ..BLADE_18
    };
    let saved = Config {
        lid_logo: Some(false),
        ..Config::default()
    };

    apply(&saved, &model, &mut controller, RedKeys::default()).unwrap();

    assert_eq!(*log.borrow(), preset_steps(0x00));
}
