//! The writes that repeat with no click (ticket 263): what one of each
//! event sends, counted over the fakes, for the flash wear question (ticket
//! 147 (c)). The counts are pinned, so a change that sends more fails here.

use blade_core::{Config, Lighting, Preset};
use platform::PowerEvent;
use razer_proto::Command;
use razer_proto::Rgb;

use super::sleep::power;
use super::{FAKE_ON_BATTERY, FAKE_SCREEN_ON, Fixture, fixture};
use crate::rules::VARIABLE_STORAGE;

/// The set commands with a storage byte first (as `soak.rs` counts them).
const STORED: [u16; 5] = [0x0D02, 0x0D01, 0x0D07, 0x0303, 0x0300];

fn forget_sent(f: &Fixture) {
    f.shell.app().machine_mut().controller.inner.sent.clear();
}

fn sent(f: &Fixture) -> Vec<Command> {
    f.shell.app().machine().controller.inner.sent.clone()
}

/// All writes and the stored ones.
fn counts(f: &Fixture) -> (usize, usize) {
    let sent = sent(f);
    let stored = sent
        .iter()
        .filter(|c| STORED.contains(&c.id) && c.args.first() == Some(&VARIABLE_STORAGE))
        .count();
    (sent.len(), stored)
}

/// With the lighting saved, as the user has it: the app then paints keys.
fn lit(config: Config) -> Fixture {
    let f = fixture(Config {
        lighting: Some(Lighting {
            brightness: 100,
            colour: Rgb {
                red: 0,
                green: 255,
                blue: 0,
            },
            vapor_light: true,
        }),
        ..config
    });
    f.shell
        .app()
        .machine_mut()
        .controller
        .inner
        .brightness_level = Some(255);
    forget_sent(&f);
    f
}

/// The keys dark at the screen off, lit again at the screen on (ticket 241).
#[test]
fn a_screen_off_and_on_sends_two_stored_levels() {
    let f = lit(Config::default());

    FAKE_SCREEN_ON.set(Some(false));
    power(&f, &[PowerEvent::ScreenChanged]);
    FAKE_SCREEN_ON.set(Some(true));
    power(&f, &[PowerEvent::ScreenChanged]);

    assert_eq!(counts(&f), (2, 2));
}

/// Performance saved: Balanced at the pull, Performance at the plug, 4
/// zones each. The lighting after a charger change is spaced (ticket 246).
#[test]
fn a_bouncing_plug_sends_the_preset_at_each_event() {
    let f = lit(Config {
        preset: Preset::Performance,
        ..Config::default()
    });

    FAKE_ON_BATTERY.set(true);
    power(&f, &[PowerEvent::OnBattery]);
    FAKE_ON_BATTERY.set(false);
    power(&f, &[PowerEvent::OnAc]);

    assert_eq!(counts(&f), (8, 8));
}

/// 16 new levels read while Fn is held: one vapor light write and one
/// config save each (the user asked for the live follow).
#[test]
fn a_held_fn_brightness_sweep_sends_one_vapor_write_per_level() {
    let f = lit(Config::default());

    for level in (0..=255u8).step_by(16) {
        f.shell
            .app()
            .machine_mut()
            .controller
            .inner
            .brightness_level = Some(level);
        f.shell.follow_brightness_keys();
    }

    assert_eq!(counts(&f), (16, 16));
}

/// The effect and the 7 frame rows, with no storage byte.
#[test]
fn a_touchpad_change_sends_a_whole_key_frame() {
    let f = lit(Config::default());

    f.shell.app().machine_mut().registry.touchpad = Some(0);
    f.shell.act(blade_lite::App::on_touchpad_changed);

    assert_eq!(counts(&f), (8, 0));
}
