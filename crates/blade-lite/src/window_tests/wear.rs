//! The writes that repeat with no click (ticket 263): what one of each
//! event sends, counted over the fakes through the app's own entry points,
//! for the flash wear question (ticket 147 (c)). The counts are pinned, so a
//! change that sends more fails here.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Lighting, Preset};
use platform::PowerEvent;
use razer_proto::{BLADE_18, Brightness, Command, Rgb};

use super::{FAKE_ON_BATTERY, FAKE_SCREEN_ON, Fixture, fixture};
use crate::fakes::FakeCharger;
use crate::rules::VARIABLE_STORAGE;

/// The set commands that carry a storage byte (`soak.rs` counts the same).
/// The lid logo's `0300` and `0302` carry 0 on this model, so they never
/// count as stored.
const STORED: [u16; 4] = [0x0D02, 0x0D01, 0x0D07, 0x0303];

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

/// Listens for power events as the app does, from a sender of its own.
fn listening(f: &Fixture) -> Sender<PowerEvent> {
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    sender
}

/// One power event, taken as a click or poll takes it (`follow_own_power`:
/// the event, then the charger change's lighting, then the screen).
fn event(f: &Fixture, sender: &Sender<PowerEvent>, event: PowerEvent) {
    sender.send(event).unwrap();
    f.shell.follow_own_power();
}

fn level(percent: u8) -> Vec<Command> {
    Brightness::new(percent).unwrap().commands(&BLADE_18)
}

/// The keys dark at the screen off, lit again at the screen on (ticket 241).
#[test]
fn a_screen_off_and_on_sends_two_stored_levels() {
    let f = lit(Config::default());
    let power = listening(&f);

    FAKE_SCREEN_ON.set(Some(false));
    event(&f, &power, PowerEvent::ScreenChanged);
    FAKE_SCREEN_ON.set(Some(true));
    event(&f, &power, PowerEvent::ScreenChanged);

    assert_eq!(sent(&f), [level(0), level(100)].concat());
    assert_eq!(counts(&f), (2, 2));
}

/// Performance saved: Balanced at the pull (4 zones), the lighting once
/// (keyboard and vapor levels stored, the effect and 7 rows not), then
/// Performance at the plug (4 zones). The lighting waits 10 s for its next
/// send (ticket 246); the mode writes have no spacing, so each bounce
/// inside it adds 8 stored writes.
#[test]
fn a_bouncing_plug_sends_the_preset_at_each_event_and_the_lighting_spaced() {
    let f = lit(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let power = listening(&f);
    let bounce = || {
        FAKE_ON_BATTERY.set(true);
        event(&f, &power, PowerEvent::OnBattery);
        FAKE_ON_BATTERY.set(false);
        event(&f, &power, PowerEvent::OnAc);
    };

    bounce();
    assert_eq!(counts(&f), (18, 10));
    bounce();
    assert_eq!(counts(&f), (26, 18));
}

/// A weak charger read and the full one back on the polls, Performance
/// saved: the battery preset, then Performance, 4 zones each, with no
/// power event. With no view open the charger is read with the heat, each
/// 10 s.
#[test]
fn a_charger_read_weak_then_full_sends_the_preset_each_time() {
    let f = lit(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let start = Instant::now();
    // A tick each second for `secs`, from `from`: the reads come when due.
    let ticks = |from: u64, secs: u64| {
        for second in from..from + secs {
            f.shell
                .tick_at(start + Duration::from_secs(second), SystemTime::now());
        }
    };
    ticks(0, 1);
    forget_sent(&f);

    f.shell.app().machine_mut().controller.inner.charger = FakeCharger::UsbC;
    ticks(1, 20);
    let weak = sent(&f).iter().filter(|c| c.id == 0x0D02).count();
    f.shell.app().machine_mut().controller.inner.charger = FakeCharger::Razer;
    ticks(21, 20);

    let modes = sent(&f).iter().filter(|c| c.id == 0x0D02).count();
    assert_eq!((weak, modes), (4, 8));
}

/// Fn + F11 held from the top down, at the laptop's steps (15 up to 225,
/// then 255, `NOTES.md`): 16 new levels, each one stored vapor light
/// write (and a config save, `on_brightness_keys`). Going back and forth
/// has no limit.
#[test]
fn a_held_fn_brightness_sweep_sends_one_vapor_write_per_level() {
    let f = lit(Config::default());

    for level in (0..=225u8).rev().step_by(15) {
        f.shell
            .app()
            .machine_mut()
            .controller
            .inner
            .brightness_level = Some(level);
        f.shell.follow_brightness_keys();
    }

    assert!(sent(&f).iter().all(|c| c.id == 0x0303 && c.args[1] == 0x26));
    assert_eq!(counts(&f), (16, 16));
}

/// The effect and the 7 frame rows, with no storage byte.
#[test]
fn a_touchpad_change_sends_a_whole_key_frame() {
    let f = lit(Config::default());

    f.shell.app().machine_mut().registry.touchpad = Some(0);
    f.shell.follow_touchpad();

    assert_eq!(counts(&f), (8, 0));
}
