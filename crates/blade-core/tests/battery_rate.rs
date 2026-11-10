//! 60 Hz on battery, over fake power events and a fake display list.

mod common;

use blade_core::{BatteryRate, Config, RefreshRate, set_refresh_rate};
use common::{FakeDisplays, mode, panel};
use platform::{ModeLife, PowerEvent};

fn config(sixty_hz_on_battery: bool) -> Config {
    Config {
        sixty_hz_on_battery,
        ..Config::default()
    }
}

/// The panel at 4K and the given rate, as the only display.
fn panel_at(hz: u32) -> FakeDisplays {
    FakeDisplays::new(Some(vec![panel(mode(3840, 2400, hz))]))
}

/// Feeds each event to a fresh [`BatteryRate`], and stops at an error.
fn feed(events: &[PowerEvent], config: Config, api: &mut FakeDisplays) {
    let mut battery_rate = BatteryRate::default();
    for &event in events {
        battery_rate.on_event(event, &config, api).unwrap();
    }
}

#[test]
fn battery_sets_60_hz() {
    let mut api = panel_at(240);

    feed(&[PowerEvent::OnBattery], config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn the_battery_rate_is_not_saved_but_the_put_back_rate_is() {
    let mut api = panel_at(240);

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    let lives: Vec<(u32, ModeLife)> = api
        .set
        .iter()
        .map(|(_, mode, life)| (mode.hz, *life))
        .collect();
    assert_eq!(
        lives,
        [(60, ModeLife::UntilRestart), (240, ModeLife::Saved)]
    );
}

#[test]
fn ac_after_battery_restores_the_rate_that_was_active() {
    let mut api = panel_at(120);

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    assert_eq!(api.panel_mode(), mode(3840, 2400, 120));
}

#[test]
fn a_second_battery_event_keeps_the_first_rate() {
    let mut api = panel_at(240);

    feed(
        &[
            PowerEvent::OnBattery,
            PowerEvent::OnBattery,
            PowerEvent::OnAc,
        ],
        config(true),
        &mut api,
    );

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn with_the_option_off_nothing_changes() {
    let mut api = panel_at(240);

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(false),
        &mut api,
    );

    assert_eq!(api.set, []);
}

#[test]
fn ac_without_battery_first_changes_nothing() {
    // Windows reports the power source once when the app starts.
    let mut api = panel_at(240);

    feed(&[PowerEvent::OnAc], config(true), &mut api);

    assert_eq!(api.set, []);
}

#[test]
fn a_panel_already_at_60_hz_is_left_alone() {
    let mut api = panel_at(60);

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    assert_eq!(api.set, []);
}

#[test]
fn a_resume_with_no_saved_rate_does_not_touch_the_rate() {
    let mut api = panel_at(240);

    feed(&[PowerEvent::Resumed], config(true), &mut api);

    assert_eq!(api.set, []);
}

#[test]
fn put_back_restores_the_rate_from_before_battery_once() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();

    battery_rate.put_back(&mut api).unwrap();
    battery_rate.put_back(&mut api).unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 240]);
}

#[test]
fn put_back_without_battery_first_changes_nothing() {
    let mut api = panel_at(240);

    BatteryRate::default().put_back(&mut api).unwrap();

    assert_eq!(api.set, []);
}

#[test]
fn a_rate_changed_outside_the_app_on_battery_is_kept() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    // Such as in Windows Settings.
    set_refresh_rate(&mut api, RefreshRate(120)).unwrap();

    battery_rate.put_back(&mut api).unwrap();
    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 120]);
}

#[test]
fn a_resume_on_battery_sets_60_hz_again_after_windows_put_the_rate_back() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    // Windows puts the saved rate back in the wake.
    set_refresh_rate(&mut api, RefreshRate(240)).unwrap();

    battery_rate
        .on_event(PowerEvent::Resumed, &config(true), &mut api)
        .unwrap();
    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 240, 60, 240]);
    let (_, _, life) = api.set[2];
    assert_eq!(life, ModeLife::UntilRestart);
}

#[test]
fn a_resume_keeps_a_rate_changed_outside_the_app_on_battery() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    set_refresh_rate(&mut api, RefreshRate(120)).unwrap();

    battery_rate
        .on_event(PowerEvent::Resumed, &config(true), &mut api)
        .unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 120]);
}

#[test]
fn a_resume_with_the_panel_still_at_60_hz_changes_nothing() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();

    battery_rate
        .on_event(PowerEvent::Resumed, &config(true), &mut api)
        .unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60]);
}

#[test]
fn a_resume_after_the_rate_was_put_back_on_ac_changes_nothing() {
    let mut api = panel_at(240);

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc, PowerEvent::Resumed],
        config(true),
        &mut api,
    );

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 240]);
}

#[test]
fn a_resume_with_no_internal_panel_active_is_not_an_error() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    // The lid shut, a monitor only.
    let mut api = FakeDisplays::new(Some(vec![]));

    battery_rate
        .on_event(PowerEvent::Resumed, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.set, []);
}
