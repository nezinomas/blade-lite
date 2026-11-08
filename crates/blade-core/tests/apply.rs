//! Applying the saved settings, and applying them again after resume.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use blade_core::{
    ApplyError, Config, Controller, CustomSettings, Lighting, Preset, RESUME_PAUSE, apply,
    apply_lighting, apply_performance, reapply_on_resume,
};
use platform::{PowerEvent, PowerEvents};
use razer_device::{Clock, DeviceError};
use razer_proto::{
    BLADE_18, Brightness, BrightnessError, Command, CustomError, Rgb, static_colour,
};

/// One thing that happened, in order.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Sent(Command),
    Slept(Duration),
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
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        let sent = commands.iter().cloned().map(Step::Sent);
        self.log.borrow_mut().extend(sent);
        Ok(())
    }
}

/// Records each sleep instead of waiting.
struct FakeClock {
    log: Log,
}

impl Clock for FakeClock {
    fn sleep(&self, duration: Duration) {
        self.log.borrow_mut().push(Step::Slept(duration));
    }
}

/// Gives the events it was made with, then says no more will come.
struct FakePowerEvents(std::vec::IntoIter<PowerEvent>);

impl PowerEvents for FakePowerEvents {
    fn next_event(&mut self) -> Option<PowerEvent> {
        self.0.next()
    }
}

fn fakes() -> (FakeController, FakeClock, Log) {
    let log = Log::default();
    let controller = FakeController {
        log: Rc::clone(&log),
        fails: false,
    };
    let clock = FakeClock {
        log: Rc::clone(&log),
    };
    (controller, clock, log)
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
        let (mut controller, _, log) = fakes();

        apply(
            &config(preset, CustomSettings::default()),
            &BLADE_18,
            &mut controller,
            false,
        )
        .unwrap();

        assert_eq!(*log.borrow(), preset_steps(mode), "{preset:?}");
    }
}

#[test]
fn custom_with_a_manual_fan_sends_mode_then_boosts_then_fans() {
    let (mut controller, _, log) = fakes();

    let saved = config(Preset::Custom, custom(2, 3, Some(3000)));
    apply(&saved, &BLADE_18, &mut controller, false).unwrap();

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
    let (mut controller, _, log) = fakes();

    apply(
        &config(Preset::Custom, custom(1, 1, None)),
        &BLADE_18,
        &mut controller,
        false,
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
        let (mut controller, _, log) = fakes();

        let result = apply(
            &config(Preset::Custom, bad),
            &BLADE_18,
            &mut controller,
            false,
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
    let (mut controller, _, log) = fakes();

    let saved = config(Preset::Balanced, custom(9, 9, Some(1)));
    apply(&saved, &BLADE_18, &mut controller, false).unwrap();

    assert_eq!(*log.borrow(), preset_steps(0x00));
}

#[test]
fn saved_lighting_is_sent_after_the_preset() {
    let (mut controller, _, log) = fakes();
    let colour = Rgb::new(0xE8, 0x11, 0x23);
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 50,
            colour,
            vapor_light: true,
        }),
        ..Config::default()
    };

    apply(&saved, &BLADE_18, &mut controller, false).unwrap();

    let mut expected = preset_steps(0x00);
    let lighting = brightness_commands(50, true);
    let lighting = lighting.into_iter().chain(static_colour(colour, &BLADE_18));
    expected.extend(lighting.map(Step::Sent));
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn applying_the_performance_leaves_the_lighting_alone() {
    let (mut controller, _, log) = fakes();
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
    let (mut controller, _, log) = fakes();
    let colour = Rgb::new(1, 2, 3);
    let lighting = Lighting {
        brightness: 20,
        colour,
        vapor_light: true,
    };

    apply_lighting(lighting, &BLADE_18, &mut controller, false).unwrap();

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
    let (mut controller, _, log) = fakes();
    let saved = Config {
        lighting: Some(Lighting {
            brightness: 101,
            colour: Rgb::new(0, 0, 0),
            vapor_light: true,
        }),
        ..Config::default()
    };

    let result = apply(&saved, &BLADE_18, &mut controller, false);

    assert!(matches!(
        result,
        Err(ApplyError::Brightness(BrightnessError::OutOfRange(101)))
    ));
    assert!(log.borrow().is_empty());
}

#[test]
fn a_device_error_is_returned() {
    let (mut controller, _, _) = fakes();
    controller.fails = true;

    let result = apply(&Config::default(), &BLADE_18, &mut controller, false);

    assert!(matches!(result, Err(ApplyError::Device(DeviceError::Busy))));
}

#[test]
fn each_resume_waits_the_pause_then_applies_again() {
    let (mut controller, clock, log) = fakes();
    let mut events = FakePowerEvents(vec![PowerEvent::Resumed, PowerEvent::Resumed].into_iter());

    reapply_on_resume(
        &mut events,
        &clock,
        &Config::default(),
        &BLADE_18,
        &mut controller,
    )
    .unwrap();

    let mut expected = vec![Step::Slept(RESUME_PAUSE)];
    expected.extend(preset_steps(0x00));
    expected.push(Step::Slept(RESUME_PAUSE));
    expected.extend(preset_steps(0x00));
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn power_source_changes_do_not_apply_again() {
    let (mut controller, clock, log) = fakes();
    let mut events = FakePowerEvents(vec![PowerEvent::OnBattery, PowerEvent::OnAc].into_iter());

    reapply_on_resume(
        &mut events,
        &clock,
        &Config::default(),
        &BLADE_18,
        &mut controller,
    )
    .unwrap();

    assert_eq!(*log.borrow(), []);
}

#[test]
fn no_resume_applies_nothing() {
    let (mut controller, clock, log) = fakes();
    let mut events = FakePowerEvents(Vec::new().into_iter());

    reapply_on_resume(
        &mut events,
        &clock,
        &Config::default(),
        &BLADE_18,
        &mut controller,
    )
    .unwrap();

    assert!(log.borrow().is_empty());
}

#[test]
fn a_failed_re_apply_stops_and_is_returned() {
    let (mut controller, clock, _) = fakes();
    controller.fails = true;
    let mut events = FakePowerEvents(vec![PowerEvent::Resumed, PowerEvent::Resumed].into_iter());

    let result = reapply_on_resume(
        &mut events,
        &clock,
        &Config::default(),
        &BLADE_18,
        &mut controller,
    );

    assert!(matches!(result, Err(ApplyError::Device(DeviceError::Busy))));
    assert_eq!(events.next_event(), Some(PowerEvent::Resumed), "it went on");
}

#[test]
fn the_touchpad_key_is_red_in_the_one_frame_sent() {
    let (mut controller, _, log) = fakes();
    let lighting = Lighting {
        brightness: 50,
        colour: Rgb::new(0x44, 0xD6, 0x2C),
        vapor_light: true,
    };

    apply_lighting(lighting, &BLADE_18, &mut controller, true).unwrap();

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
    let (mut controller, _, log) = fakes();
    let colour = Rgb::new(1, 2, 3);
    let lighting = Lighting {
        brightness: 80,
        colour,
        vapor_light: false,
    };

    apply_lighting(lighting, &BLADE_18, &mut controller, false).unwrap();

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
    let (mut controller, _, log) = fakes();
    let model = razer_proto::ModelProfile {
        vapor_light: None,
        ..BLADE_18
    };
    let lighting = Lighting {
        brightness: 80,
        colour: Rgb::new(1, 2, 3),
        vapor_light: true,
    };

    apply_lighting(lighting, &model, &mut controller, false).unwrap();

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
    let (mut controller, _, log) = fakes();
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

    apply(&saved, &BLADE_18, &mut controller, false).unwrap();

    let mut expected = preset_steps(0x00);
    let logo = BLADE_18.logo.unwrap().commands(false);
    let lighting = brightness_commands(50, true);
    let lighting = lighting.into_iter().chain(static_colour(colour, &BLADE_18));
    expected.extend(logo.into_iter().chain(lighting).map(Step::Sent));
    assert_eq!(*log.borrow(), expected);
}

#[test]
fn a_model_without_a_lid_logo_gets_no_logo_commands() {
    let (mut controller, _, log) = fakes();
    let model = razer_proto::ModelProfile {
        logo: None,
        ..BLADE_18
    };
    let saved = Config {
        lid_logo: Some(false),
        ..Config::default()
    };

    apply(&saved, &model, &mut controller, false).unwrap();

    assert_eq!(*log.borrow(), preset_steps(0x00));
}
