//! Refresh rates, native mode, 60 Hz on battery and resume.

use blade_core::{Config, Lighting, Readings, RefreshRate};
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use ui::{DisplayCommand, PowerPill};

use crate::fakes::TestApp;
use crate::{FHD, WHITE, app, app_with, lighting_commands, panel_hz, sent};

#[test]
fn a_rate_button_switches_the_panel() {
    let (mut app, _dir, _) = app();

    app.on_display(DisplayCommand::SetRefreshRate(RefreshRate(120)))
        .unwrap();

    assert_eq!(panel_hz(&app), 120);
}

#[test]
fn the_display_view_shows_the_chosen_native_mode() {
    let (mut app, _dir, _) = app();

    let view = app.display_view().unwrap();

    let selected = view.native_modes.iter().find(|o| o.selected).unwrap();
    assert_eq!(selected.mode, BLADE_18.native_modes[0]);
    assert!(!view.restart_infobar);
}

#[test]
fn a_native_mode_is_sent_to_the_controller() {
    let (mut app, _dir, _) = app();

    app.on_display(DisplayCommand::SetNativeMode(FHD)).unwrap();

    assert_eq!(sent(&app), [FHD.command()]);
}

#[test]
fn a_native_mode_the_model_lacks_is_refused_and_not_sent() {
    let (mut app, _dir, _) = app();
    let unknown = razer_proto::NativeMode { id: 0x7F, ..FHD };

    let result = app.on_display(DisplayCommand::SetNativeMode(unknown));

    assert!(matches!(
        result,
        Err(blade_lite::AppError::NativeMode(
            blade_core::NativeModeError::NotOffered(mode)
        )) if mode == unknown
    ));
    assert_eq!(sent(&app), []);
}

#[test]
fn turning_on_sixty_hz_on_battery_while_on_battery_drops_the_rate() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();

    assert!(Config::load(&path).unwrap().sixty_hz_on_battery);
    assert_eq!(panel_hz(&app), 60);
}

#[test]
fn turning_it_off_on_battery_puts_the_rate_back() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(false))
        .unwrap();

    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn setting_sixty_hz_on_battery_to_what_it_is_saves_nothing() {
    for on in [false, true] {
        let (mut app, _dir, path) = app_with(Config {
            sixty_hz_on_battery: on,
            ..Config::default()
        });

        app.on_display(DisplayCommand::SetSixtyHzOnBattery(on))
            .unwrap();

        assert!(!path.exists());
    }
}

#[test]
fn turning_it_on_on_battery_when_60_hz_fails_keeps_it_off() {
    let (mut app, _dir, path) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut()
        .displays
        .0
        .modes
        .retain(|mode| mode.hz != 60);

    assert!(
        app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
            .is_err()
    );

    assert!(!app.config().sixty_hz_on_battery);
    assert!(!Config::load(&path).unwrap().sixty_hz_on_battery);
    assert!(app.display_view().unwrap().rates_enabled);
    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn the_switch_flips_the_saved_value() {
    let (mut app, _dir, path) = app();

    app.toggle_sixty_hz_on_battery().unwrap();
    assert!(Config::load(&path).unwrap().sixty_hz_on_battery);

    app.toggle_sixty_hz_on_battery().unwrap();
    assert!(!Config::load(&path).unwrap().sixty_hz_on_battery);
}

#[test]
fn turning_it_on_twice_on_battery_keeps_the_rate_to_put_back() {
    let (mut app, _dir, _) = app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();
    app.on_power(PowerEvent::OnAc).unwrap();

    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn turning_it_on_or_off_on_ac_leaves_the_rate() {
    let (mut app, _dir, _) = app();

    app.on_display(DisplayCommand::SetSixtyHzOnBattery(true))
        .unwrap();
    assert_eq!(panel_hz(&app), 240);
    app.on_display(DisplayCommand::SetSixtyHzOnBattery(false))
        .unwrap();
    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn the_power_source_drives_the_rate_and_the_pill() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });

    app.on_power(PowerEvent::OnBattery).unwrap();
    assert_eq!(panel_hz(&app), 60);
    assert_eq!(app.header(Readings::default()).power, PowerPill::OnBattery);

    app.on_power(PowerEvent::OnAc).unwrap();
    assert_eq!(panel_hz(&app), 240);
    assert_eq!(app.header(Readings::default()).power, PowerPill::PluggedIn);
}

#[test]
fn a_resume_applies_everything_again() {
    let (mut app, _dir, _) = app_with(Config {
        lighting: Some(Lighting {
            brightness: 10,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    });

    app.on_power(PowerEvent::Resumed).unwrap();

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(lighting_commands(10, WHITE));
    assert_eq!(sent(&app), expected);
}

#[test]
fn quitting_on_battery_puts_the_rate_back() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();

    app.on_quit().unwrap();

    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn quitting_after_the_charger_is_back_leaves_the_rate_picked_since() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.on_power(PowerEvent::OnAc).unwrap();
    app.on_display(DisplayCommand::SetRefreshRate(RefreshRate(120)))
        .unwrap();

    app.on_quit().unwrap();

    assert_eq!(panel_hz(&app), 120);
}

#[test]
fn quitting_after_the_switch_is_turned_off_on_battery_leaves_the_rate() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.on_display(DisplayCommand::SetSixtyHzOnBattery(false))
        .unwrap();
    app.on_display(DisplayCommand::SetRefreshRate(RefreshRate(120)))
        .unwrap();

    app.on_quit().unwrap();

    assert_eq!(panel_hz(&app), 120);
}

#[test]
fn a_wake_on_battery_sets_60_hz_again_when_windows_put_the_rate_back() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();
    // Windows puts the saved rate back in the wake.
    app.machine_mut().displays.0.current.hz = 240;

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(panel_hz(&app), 60);
}

#[test]
fn a_wake_on_ac_before_its_power_event_leaves_the_rate() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();
    // The charger went in during the sleep; its event comes later.
    app.machine_mut().displays.0.current.hz = 240;
    app.take_on_battery(Some(false));

    app.on_power(PowerEvent::Resumed).unwrap();

    assert_eq!(panel_hz(&app), 240);
}

/// On battery at 60 Hz, then a wake whose 60 Hz Windows refused once.
fn refused_after_a_wake() -> (TestApp, tempfile::TempDir) {
    let (mut app, dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().displays.0.current.hz = 240;
    app.machine_mut().displays.1 = 1;
    app.on_power(PowerEvent::Resumed).unwrap();
    (app, dir)
}

#[test]
fn a_wake_60_hz_refused_on_battery_is_tried_again() {
    let (mut app, _dir) = refused_after_a_wake();
    assert_eq!(panel_hz(&app), 240);
    assert!(app.battery_rate_pending());

    app.retry_battery_rate().unwrap();

    assert_eq!(panel_hz(&app), 60);
    assert!(!app.battery_rate_pending());
}

#[test]
fn a_wake_60_hz_refused_is_not_tried_again_on_ac() {
    let (mut app, _dir) = refused_after_a_wake();
    app.take_on_battery(Some(false));

    assert!(!app.battery_rate_pending());
    app.retry_battery_rate().unwrap();

    assert_eq!(panel_hz(&app), 240);
}

/// A start after a crash on battery left the panel at 60 Hz, with 240 Hz
/// saved; `on_battery` is the source the start reads.
fn start_left_at_60(
    sixty_hz_on_battery: bool,
    on_battery: Option<bool>,
) -> (TestApp, tempfile::TempDir) {
    let (mut app, dir, _) = app_with(Config {
        sixty_hz_on_battery,
        ..Config::default()
    });
    let displays = &mut app.machine_mut().displays;
    displays.2 = Some(displays.0.current);
    displays.0.current.hz = 60;
    app.take_on_battery(on_battery);
    app.start().unwrap();
    app.start_rate().unwrap();
    (app, dir)
}

#[test]
fn a_start_on_ac_at_60_hz_puts_the_saved_rate_back() {
    let (app, _dir) = start_left_at_60(true, Some(false));

    assert_eq!(panel_hz(&app), 240);
}

#[test]
fn a_start_on_ac_with_the_switch_off_leaves_the_rate() {
    let (app, _dir) = start_left_at_60(false, Some(false));

    assert_eq!(panel_hz(&app), 60);
}

#[test]
fn a_start_on_battery_or_an_unknown_source_leaves_the_rate() {
    for on_battery in [Some(true), None] {
        let (app, _dir) = start_left_at_60(true, on_battery);

        assert_eq!(panel_hz(&app), 60, "source {on_battery:?}");
    }
}
