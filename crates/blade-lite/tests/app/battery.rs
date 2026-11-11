//! Presets on battery, and the charge limit.

use blade_core::{Config, CustomSettings, Preset};
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use ui::PerformanceCommand;

use crate::{
    LIMIT_OFF, LIMIT_ON, SET_CHARGE_LIMIT, app, app_with, charge_limit, charge_limit_on, panel_hz,
    performance_app, sent,
};

/// Sets a fan's target speed.
const SET_FAN: u16 = 0x0D01;
/// Sets a CPU or GPU boost.
const SET_BOOST: u16 = 0x0D07;
/// Sets the preset of one zone.
const SET_PRESET: u16 = 0x0D02;
/// The Custom mode byte in a preset set.
const CUSTOM_MODE: u8 = BLADE_18.custom.mode;

#[test]
fn on_battery_performance_runs_as_balanced_and_comes_back_on_ac() {
    let (mut app, _dir, _) = performance_app();

    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
    assert_eq!(app.config().preset, Preset::Performance);

    app.on_power(PowerEvent::OnAc).unwrap();
    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(PerfMode::Performance.commands(&BLADE_18).unwrap());
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_power_event_that_keeps_the_running_preset_sends_nothing() {
    let (mut app, _dir, _) = performance_app();

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn performance_is_refused_on_battery() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();

    let result = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));

    assert!(result.is_err());
    assert_eq!(sent(&app), []);
    assert!(!path.exists());
}

#[test]
fn a_resume_on_battery_applies_balanced_for_a_saved_performance() {
    let (mut app, _dir, _) = performance_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn start_reads_the_charge_limit_for_the_performance_view() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);

    app.start().unwrap();

    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_failed_charge_limit_read_hides_the_switch_and_keeps_the_start() {
    let (mut app, _dir, _) = app();

    app.start().unwrap();

    assert_eq!(charge_limit_on(&app), None);
    assert_ne!(sent(&app), []);
}

#[test]
fn a_resume_reads_the_charge_limit_again() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.charge = Some(LIMIT_OFF);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(charge_limit_on(&app), Some(false));
}

#[test]
fn the_charge_limit_switch_sends_its_command_and_saves_nothing() {
    let (mut app, _dir, path) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_performance(PerformanceCommand::SetChargeLimit(false))
        .unwrap();

    assert_eq!(sent(&app), [charge_limit().command(false).unwrap()]);
    assert_eq!(charge_limit_on(&app), Some(false));
    assert!(!path.exists());
}

#[test]
fn a_failed_charge_limit_switch_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.start().unwrap();
    app.machine_mut().controller.fails = true;

    assert!(
        app.on_performance(PerformanceCommand::SetChargeLimit(false))
            .is_err()
    );
    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_charge_limit_that_does_not_take_shows_the_state_read_back() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_ON);
    app.machine_mut().controller.charge_stuck = true;
    app.start().unwrap();

    assert!(
        app.on_performance(PerformanceCommand::SetChargeLimit(false))
            .is_err()
    );
    assert_eq!(charge_limit_on(&app), Some(true));
}

#[test]
fn a_charge_limit_switch_does_not_touch_the_preset() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.charge = Some(LIMIT_OFF);
    app.start().unwrap();
    let before = *app.config();
    app.machine_mut().controller.sent.clear();

    app.on_performance(PerformanceCommand::SetChargeLimit(true))
        .unwrap();

    assert!(sent(&app).iter().all(|c| c.id == SET_CHARGE_LIMIT));
    assert_eq!(*app.config(), before);
}

#[test]
fn the_running_preset_is_the_one_the_power_source_lets_run() {
    let (mut app, _dir, _) = performance_app();
    assert_eq!(app.running_preset(), Preset::Performance);

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(app.running_preset(), Preset::Balanced);
}

#[test]
fn a_start_on_battery_sends_balanced_for_a_saved_custom_fan() {
    let (mut app, _dir, _) = app_with(Config {
        preset: Preset::Custom,
        custom: CustomSettings {
            fan_rpm: Some(1600),
            ..CustomSettings::default()
        },
        ..Config::default()
    });
    app.take_on_battery(true);

    app.start().unwrap();

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert!(
        sent(&app)
            .windows(balanced.len())
            .any(|run| run == balanced)
    );
    assert!(sent(&app).iter().all(|command| {
        command.id != SET_FAN
            && command.id != SET_BOOST
            && !(command.id == SET_PRESET
                && (command.args.get(2) == Some(&CUSTOM_MODE) || command.args.get(3) != Some(&0)))
    }));
    assert_eq!(app.config().preset, Preset::Custom);
}

#[test]
fn the_power_event_after_a_start_on_battery_sends_nothing() {
    let (mut app, _dir, _) = performance_app();
    app.take_on_battery(true);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn an_ac_event_after_a_start_taken_as_battery_sends_the_saved_preset_once() {
    let (mut app, _dir, _) = performance_app();
    app.take_on_battery(true);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(
        sent(&app),
        PerfMode::Performance.commands(&BLADE_18).unwrap()
    );
    assert_eq!(app.config().preset, Preset::Performance);
}

#[test]
fn a_resume_taken_as_battery_sends_balanced_for_a_saved_performance() {
    let (mut app, _dir, _) = performance_app();
    app.take_on_battery(true);

    app.on_power(PowerEvent::Resumed).unwrap();

    let balanced = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    assert!(
        sent(&app)
            .windows(balanced.len())
            .any(|run| run == balanced)
    );
    let performance = PerfMode::Performance.commands(&BLADE_18).unwrap();
    assert!(
        !sent(&app)
            .windows(performance.len())
            .any(|run| run == performance)
    );
}

#[test]
fn a_power_event_sends_again_after_a_failed_start() {
    let (mut app, _dir, _) = performance_app();
    app.take_on_battery(true);
    app.machine_mut().controller.fails = true;
    assert!(app.start().is_err());
    app.machine_mut().controller.fails = false;
    app.machine_mut().controller.sent.clear();

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn a_power_event_sends_again_after_a_failed_switch() {
    let (mut app, _dir, _) = performance_app();
    app.machine_mut().controller.fails = true;
    assert!(app.on_power(PowerEvent::OnBattery).is_err());
    app.machine_mut().controller.fails = false;

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn a_power_event_sends_again_after_a_failed_preset_click() {
    let (mut app, _dir, _) = app();
    app.machine_mut().controller.fails = true;
    let click = app.on_performance(PerformanceCommand::SetPreset(Preset::Performance));
    assert!(click.is_err());
    app.machine_mut().controller.fails = false;

    app.on_power(PowerEvent::OnBattery).unwrap();

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn the_rate_follows_the_battery_even_when_the_preset_send_fails() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.machine_mut().controller.fails = true;
    assert!(app.start().is_err());

    assert!(app.on_power(PowerEvent::OnBattery).is_err());

    assert_eq!(panel_hz(&app), 60);
}

/// The app with 60 Hz on battery, on battery, the panel at 60 Hz. Returns
/// the rate from before battery.
fn on_battery_at_60_hz() -> (crate::fakes::TestApp, tempfile::TempDir, u32) {
    let (mut app, dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    let before = panel_hz(&app);
    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(panel_hz(&app), 60);
    (app, dir, before)
}

#[test]
fn a_resume_on_ac_puts_the_rate_back() {
    // The charger went in during the sleep, before its power event.
    let (mut app, _dir, before) = on_battery_at_60_hz();
    app.take_on_battery(false);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(panel_hz(&app), before);
}

#[test]
fn a_resume_on_a_new_battery_sets_60_hz() {
    // The charger was pulled in the sleep, before its power event.
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.take_on_battery(true);

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(panel_hz(&app), 60);
}

#[test]
fn the_poll_sends_the_preset_again_after_a_failed_switch() {
    let (mut app, _dir, _) = performance_app();
    app.machine_mut().controller.fails = true;
    assert!(app.on_power(PowerEvent::OnBattery).is_err());
    app.machine_mut().controller.fails = false;

    assert!(app.resend_preset().unwrap());

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn the_poll_sends_nothing_once_the_preset_took() {
    let (mut app, _dir, _) = performance_app();
    app.machine_mut().controller.fails = true;
    assert!(app.on_power(PowerEvent::OnBattery).is_err());
    app.machine_mut().controller.fails = false;
    app.resend_preset().unwrap();
    app.machine_mut().controller.sent.clear();

    assert!(!app.resend_preset().unwrap());

    assert_eq!(sent(&app), []);
}

#[test]
fn the_poll_sends_nothing_after_a_start_that_worked() {
    let (mut app, _dir, _) = performance_app();
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();

    assert!(!app.resend_preset().unwrap());

    assert_eq!(sent(&app), []);
}

#[test]
fn the_poll_tries_again_after_a_failed_resend() {
    let (mut app, _dir, _) = performance_app();
    app.machine_mut().controller.fails = true;
    assert!(app.on_power(PowerEvent::OnBattery).is_err());

    assert!(app.resend_preset().is_err());
    app.machine_mut().controller.fails = false;
    assert!(app.resend_preset().unwrap());

    assert_eq!(sent(&app), PerfMode::Balanced.commands(&BLADE_18).unwrap());
}

#[test]
fn a_custom_send_cut_before_the_fan_is_sent_whole_on_the_next_poll() {
    let config = Config {
        preset: Preset::Custom,
        custom: CustomSettings {
            fan_rpm: Some(3000),
            ..CustomSettings::default()
        },
        ..Config::default()
    };
    let mut whole = crate::fakes::FakeController::default();
    blade_core::apply_performance(&config, &BLADE_18, &mut whole).unwrap();
    let fan_at = whole.sent.iter().position(|c| c.id == SET_FAN).unwrap();
    let (mut app, _dir, _) = app_with(config);
    app.take_on_battery(true);
    app.start().unwrap();
    app.machine_mut().controller.sent.clear();
    // The mode write goes, the fan target does not.
    app.machine_mut().controller.fails_after = Some(fan_at);
    assert!(app.on_power(PowerEvent::OnAc).is_err());
    assert!(sent(&app).iter().all(|c| c.id != SET_FAN));
    app.machine_mut().controller.fails_after = None;
    app.machine_mut().controller.sent.clear();

    assert!(app.resend_preset().unwrap());

    assert_eq!(sent(&app), whole.sent);
}
