//! 60 Hz on battery, over fake power events and a fake display list.

mod common;

use blade_core::{BatteryRate, Config};
use common::{FakeDisplays, mode, panel};
use platform::PowerEvent;

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
fn resume_does_not_touch_the_rate() {
    let mut api = panel_at(240);

    feed(&[PowerEvent::Resumed], config(true), &mut api);

    assert_eq!(api.set, []);
}
