//! Refresh rates, native mode, 60 Hz on battery and resume.

use blade_core::{Config, Lighting, Readings, RefreshRate};
use platform::PowerEvent;
use razer_proto::{BLADE_18, PerfMode};
use ui::{DisplayCommand, PowerPill};

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
fn quitting_on_ac_leaves_the_rate() {
    let (mut app, _dir, _) = app_with(Config {
        sixty_hz_on_battery: true,
        ..Config::default()
    });
    app.on_display(DisplayCommand::SetRefreshRate(RefreshRate(120)))
        .unwrap();

    app.on_quit().unwrap();

    assert_eq!(panel_hz(&app), 120);
}
