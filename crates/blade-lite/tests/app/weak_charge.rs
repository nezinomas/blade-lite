//! A weak charge, such as a USB-C monitor that powers the laptop, runs only
//! the battery presets, as Synapse does.

use blade_core::{Config, Preset};
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use tempfile::TempDir;
use ui::PerformanceCommand;

use blade_lite::AppError;

use crate::fakes::{FakeCharger, TestApp};
use crate::{charger_steady, performance_app, sent};

/// Whether every command of the preset was sent.
fn ran(app: &TestApp, mode: PerfMode) -> bool {
    let commands = mode.commands(&BLADE_18).unwrap();
    commands.iter().all(|command| sent(app).contains(command))
}

fn weak_performance_app() -> (TestApp, TempDir) {
    let (mut app, dir, _) = performance_app();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    (app, dir)
}

#[test]
fn a_start_on_a_weak_charge_runs_balanced_for_a_saved_performance() {
    let (mut app, _dir) = weak_performance_app();

    app.start().unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
    assert_eq!(app.config().preset, Preset::Performance);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_failed_charger_read_at_start_runs_balanced() {
    let (mut app, _dir) = weak_performance_app();
    app.machine_mut().controller.inner.charger = FakeCharger::Fails;

    app.start().unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
}

#[test]
fn performance_is_refused_on_a_weak_charge() {
    let (mut app, _dir, _) = crate::app();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
}

#[test]
fn the_performance_view_offers_only_the_battery_presets() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();

    let view = app.performance_view();

    let performance = view
        .presets
        .iter()
        .find(|option| option.preset == Preset::Performance)
        .unwrap();
    assert!(!performance.enabled);
}

#[test]
fn a_charger_change_on_a_poll_sends_the_preset_it_runs() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();

    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    assert!(app.follow_charger().unwrap());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());

    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::Razer;
    // One full power read is not enough: a reply may flicker in a swap.
    assert!(!app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
    assert!(app.follow_charger().unwrap());
    // Then it waits 10 s of steady charger (ticket 263).
    assert_eq!(sent(&app), []);
    charger_steady(&mut app);
    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
}

/// A weak read, then the full charger back for its wait ending at `at`:
/// one flip of a charger read that keeps changing (ticket 294). Returns
/// whether Performance was sent at its end.
fn flip(app: &mut TestApp, at: std::time::Instant) -> bool {
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    app.follow_charger().unwrap();
    app.machine_mut().controller.inner.charger = FakeCharger::Razer;
    app.follow_charger().unwrap();
    app.follow_charger().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.follow_charger_back(at.checked_sub(blade_lite::CHARGER_STEADY).unwrap())
        .unwrap();
    app.follow_charger_back(at).unwrap();
    ran(app, PerfMode::Performance)
}

#[test]
fn a_charger_read_that_keeps_flipping_stops_stepping_up_after_three_times() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    let start = std::time::Instant::now() + blade_lite::CHARGER_STEADY;
    let minute = std::time::Duration::from_secs(60);

    let ups: Vec<bool> = (0..5).map(|n| flip(&mut app, start + n * minute)).collect();

    assert_eq!(ups, [true, true, true, false, false]);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_flipping_charger_steps_up_again_once_it_was_steady_for_ten_minutes() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    let start = std::time::Instant::now() + blade_lite::CHARGER_STEADY;
    let minute = std::time::Duration::from_secs(60);
    for n in 0..4 {
        flip(&mut app, start + n * minute);
    }
    app.machine_mut().controller.inner.sent.clear();

    // The wait goes on; it ends 10 min after the first step up.
    app.follow_charger_back(start + 9 * minute).unwrap();
    assert!(!ran(&app, PerfMode::Performance));
    app.follow_charger_back(start + 10 * minute).unwrap();

    assert!(ran(&app, PerfMode::Performance));
}

#[test]
fn a_poll_with_the_same_charger_sends_nothing() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();

    assert!(!app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_failed_charger_read_on_a_poll_keeps_the_last_charger() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::Fails;

    assert!(!app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn on_battery_the_charger_is_not_read() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.asked.clear();

    assert!(!app.follow_charger().unwrap());
    assert_eq!(app.machine().controller.inner.asked, []);
}

#[test]
fn a_charger_plugged_in_on_a_weak_charge_keeps_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

/// The read at the end of the full charger's wait (ticket 263): a USB-C
/// monitor plugged in the wait, or a failed read, is a weak charge, so
/// Performance does not go, and only two full poll reads end it.
#[test]
fn a_weak_or_failed_read_at_the_end_of_the_wait_keeps_balanced() {
    for charger in [FakeCharger::UsbC, FakeCharger::Fails] {
        let (mut app, _dir, _) = performance_app();
        app.on_power(PowerEvent::OnBattery).unwrap();
        app.on_power(PowerEvent::OnAc).unwrap();
        app.machine_mut().controller.inner.sent.clear();
        app.machine_mut().controller.inner.charger = charger;

        charger_steady(&mut app);
        charger_steady(&mut app);

        assert_eq!(sent(&app), []);
        assert_eq!(app.running_preset(), Preset::Balanced);
        app.machine_mut().controller.inner.charger = FakeCharger::Razer;
        assert!(!app.follow_charger().unwrap());
        assert_eq!(app.running_preset(), Preset::Balanced);
    }
}

#[test]
fn the_power_key_does_not_step_to_performance_on_a_weak_charge() {
    let (mut app, _dir, _) = crate::app();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    app.start().unwrap();

    for _ in 0..4 {
        app.on_power_key().unwrap();
        assert_ne!(app.running_preset(), Preset::Performance);
    }
}

#[test]
fn a_weak_read_between_two_full_ones_keeps_the_weak_charge() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();

    for charger in [FakeCharger::Razer, FakeCharger::UsbC, FakeCharger::Razer] {
        app.machine_mut().controller.inner.charger = charger;
        app.follow_charger().unwrap();
    }

    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_weak_charge_coming_with_balanced_saved_is_still_reported() {
    let (mut app, _dir, _) = crate::app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;

    assert!(app.follow_charger().unwrap());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_failed_charger_read_on_a_power_event_runs_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::Fails;

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_resume_on_a_weak_charge_runs_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;

    app.on_power(PowerEvent::Resumed).unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
}

#[test]
fn a_performance_click_reads_the_charger_first() {
    let (mut app, _dir, _) = crate::app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    // Swapped to USB-C since the last poll.
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
}

#[test]
fn a_refused_click_that_finds_a_weak_charger_sends_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    // Swapped to USB-C since the last poll, while Performance runs.
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Custom));

    assert!(result.is_err());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.running_preset(), Preset::Balanced);
    assert_eq!(app.config().preset, Preset::Performance);
}

#[test]
fn a_refused_click_whose_charger_read_fails_sends_balanced() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::Fails;

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn the_power_key_reads_the_charger_first() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;

    for _ in 0..4 {
        app.on_power_key().unwrap();
        assert_ne!(app.running_preset(), Preset::Performance);
    }
}

#[test]
fn the_polls_not_a_refused_click_send_what_runs_on_the_full_charger() {
    let mut config = Config {
        preset: Preset::Performance,
        ..Config::default()
    };
    // Out of range, as a hand edit could leave it: Custom is refused.
    config.custom.fan_rpm = Some(9000);
    let (mut app, _dir, _) = crate::app_with(config);
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    // Swapped to the full charger: the second poll's full read sends it,
    // once it stayed 10 s (ticket 263).
    app.machine_mut().controller.inner.charger = FakeCharger::Razer;
    app.follow_charger().unwrap();
    app.follow_charger().unwrap();
    charger_steady(&mut app);
    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
    app.machine_mut().controller.inner.sent.clear();

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Custom));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
    assert_eq!(app.running_preset(), Preset::Performance);
}

#[test]
fn a_full_read_at_a_click_does_not_end_a_weak_charge() {
    let (mut app, _dir, _) = crate::app();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    // A reply that flickers to full in a swap.
    app.machine_mut().controller.inner.charger = FakeCharger::Razer;

    for _ in 0..2 {
        let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));
        assert!(result.is_err());
    }
    assert_eq!(sent(&app), []);

    // Two polls in a row take the full charger, after its wait (ticket 263).
    app.follow_charger().unwrap();
    app.follow_charger().unwrap();
    charger_steady(&mut app);
    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));
    assert!(result.is_ok(), "{result:?}");
    assert!(ran(&app, PerfMode::Performance));
}

#[test]
fn a_full_read_at_a_power_key_press_does_not_end_a_weak_charge() {
    let (mut app, _dir, _) = crate::app();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::Razer;
    // One poll read full: a press right after must not be the second read.
    app.follow_charger().unwrap();

    // Balanced runs: on the battery presets the key steps to Silent, then
    // back to Balanced, never to Performance.
    app.on_power_key().unwrap();
    assert!(ran(&app, PerfMode::Silent));
    app.on_power_key().unwrap();

    assert_eq!(app.running_preset(), Preset::Balanced);
    assert!(!ran(&app, PerfMode::Performance));
}

/// An app started on the Razer charger, then swapped to USB-C with no poll
/// since (ticket 257).
fn swapped_to_usb_c(config: Config) -> (TestApp, TempDir) {
    let (mut app, dir, _) = crate::app_with(config);
    app.start().unwrap();
    app.machine_mut().controller.inner.sent.clear();
    app.machine_mut().controller.inner.charger = FakeCharger::UsbC;
    (app, dir)
}

#[test]
fn import_reads_the_charger_first() {
    let (mut app, dir) = swapped_to_usb_c(Config::default());
    let picked = dir.path().join("performance.toml");
    Config {
        preset: Preset::Performance,
        ..Config::default()
    }
    .save(&picked)
    .unwrap();

    app.import_settings(&picked).unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
    assert_eq!(app.config().preset, Preset::Performance);
}

#[test]
fn a_refused_import_whose_charger_read_finds_a_weak_charger_sends_balanced() {
    let (mut app, dir) = swapped_to_usb_c(Config {
        preset: Preset::Performance,
        ..Config::default()
    });
    let picked = dir.path().join("too-fast.toml");
    std::fs::write(
        &picked,
        "preset = \"custom\"\n[custom]\ncpu_boost = 99\ngpu_boost = 1\n",
    )
    .unwrap();

    assert!(app.import_settings(&picked).is_err());

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_custom_slider_whose_charger_read_finds_a_weak_charger_sends_balanced_and_is_refused() {
    let (mut app, _dir) = swapped_to_usb_c(Config {
        preset: Preset::Custom,
        ..Config::default()
    });

    let result = app.on_performance(PerformanceCommand::SetCpuBoost(1));

    assert!(
        matches!(result, Err(AppError::NotOnBattery(Preset::Custom))),
        "{result:?}"
    );
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().custom, Config::default().custom);
}

#[test]
fn import_whose_charger_read_fails_sends_balanced_and_saves_the_file() {
    let (mut app, dir) = swapped_to_usb_c(Config::default());
    app.machine_mut().controller.inner.charger = FakeCharger::Fails;
    let picked = dir.path().join("performance.toml");
    Config {
        preset: Preset::Performance,
        ..Config::default()
    }
    .save(&picked)
    .unwrap();

    app.import_settings(&picked).unwrap();

    assert!(ran(&app, PerfMode::Balanced));
    assert!(!ran(&app, PerfMode::Performance));
    assert_eq!(app.config().preset, Preset::Performance);
}

#[test]
fn no_charger_read_while_a_razer_updater_holds_the_controller() {
    let (mut app, dir) = swapped_to_usb_c(Config::default());
    app.hold_controller(true);
    let picked = dir.path().join("performance.toml");
    Config {
        preset: Preset::Performance,
        ..Config::default()
    }
    .save(&picked)
    .unwrap();

    assert!(app.import_settings(&picked).is_err());
    app.on_power_key().ok();

    let missed = app.take_missed_reads();
    assert!(
        !missed.iter().any(|line| line.starts_with("charger")),
        "{missed:?}"
    );
}

#[test]
fn a_failed_charger_read_on_a_poll_is_kept_for_the_log() {
    let (mut app, _dir) = weak_performance_app();
    app.start().unwrap();
    // This fake has no charge limit or logo reply: those reads failed.
    app.take_missed_reads();
    app.machine_mut().controller.inner.charger = FakeCharger::Fails;

    app.follow_charger().unwrap();
    app.follow_charger().unwrap();

    let missed = app.take_missed_reads();
    assert_eq!(missed.len(), 1, "{missed:?}");
    assert!(missed[0].starts_with("charger: "), "{missed:?}");
    assert_eq!(app.take_missed_reads(), Vec::<String>::new());
}
