//! 60 Hz on battery, over fake power events and a fake display list.

mod common;

use blade_core::{
    BatteryRate, Config, DISPLAY_QUIET_POLLS, DISPLAY_ROUNDS, DISPLAY_SETTLE_POLLS, DisplayChange,
    DisplayError, RESUME_RATE_TRIES, RefreshRate, set_refresh_rate,
};
use common::{FakeDisplays, mode, monitor, panel};
use platform::{DisplayMode, ModeLife, PowerEvent};

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
fn neither_the_battery_rate_nor_the_put_back_rate_is_saved() {
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
        [(60, ModeLife::UntilRestart), (240, ModeLife::UntilRestart)]
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
fn a_resume_on_battery_with_no_saved_rate_sets_60_hz() {
    // The charger was pulled in the sleep: no battery event came first.
    let mut api = panel_at(240);

    BatteryRate::default()
        .on_resume(true, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_resume_on_battery_with_no_saved_rate_and_the_option_off_changes_nothing() {
    let mut api = panel_at(240);

    BatteryRate::default()
        .on_resume(true, &config(false), &mut api)
        .unwrap();

    assert_eq!(api.set, []);
}

#[test]
fn a_60_hz_refused_at_a_wake_with_no_saved_rate_is_tried_again() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    api.failing_sets = 1;

    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();
    battery_rate.retry(&config(true), &mut api).unwrap();
    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 240]);
}

#[test]
fn a_60_hz_refused_at_a_switch_to_battery_is_an_error_and_tried_again() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    api.failing_sets = 1;

    let switched = battery_rate.on_event(PowerEvent::OnBattery, &config(true), &mut api);
    assert!(switched.is_err());
    assert!(battery_rate.retry_pending());
    battery_rate.retry(&config(true), &mut api).unwrap();
    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
    assert!(!battery_rate.retry_pending());
}

#[test]
fn the_first_poll_after_a_refused_switch_sends_nothing() {
    // The poll may come at once; a set seconds later is the one that works.
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    api.failing_sets = 1;
    let _ = battery_rate.on_event(PowerEvent::OnBattery, &config(true), &mut api);
    let before = api.set.len();

    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.set.len(), before);
    assert!(battery_rate.retry_pending());
}

#[test]
fn a_failed_display_read_at_a_switch_is_tried_again() {
    let mut api = FakeDisplays::new(None);
    let mut battery_rate = BatteryRate::default();

    let switched = battery_rate.on_event(PowerEvent::OnBattery, &config(true), &mut api);
    assert!(matches!(switched, Err(DisplayError::Query(_))));
    api.displays = Some(vec![panel(mode(3840, 2400, 240))]);
    battery_rate.retry(&config(true), &mut api).unwrap();
    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_60_hz_the_panel_does_not_offer_at_a_switch_starts_no_tries() {
    // Trying again can't make the panel offer it.
    let mut no_60 = panel(mode(3840, 2400, 240));
    no_60.modes.retain(|mode| mode.hz != 60);
    let mut api = FakeDisplays::new(Some(vec![no_60]));
    let mut battery_rate = BatteryRate::default();

    let switched = battery_rate.on_event(PowerEvent::OnBattery, &config(true), &mut api);

    assert!(matches!(switched, Err(DisplayError::Unsupported(_))));
    assert!(!battery_rate.retry_pending());
}

#[test]
fn a_switch_to_battery_with_no_panel_active_starts_no_tries() {
    // The lid shut, a monitor only.
    let mut api = FakeDisplays::new(Some(vec![]));
    let mut battery_rate = BatteryRate::default();

    let switched = battery_rate.on_event(PowerEvent::OnBattery, &config(true), &mut api);

    assert!(matches!(switched, Err(DisplayError::NoInternalPanel)));
    assert!(!battery_rate.retry_pending());
}

#[test]
fn a_resume_on_battery_with_no_panel_active_and_no_saved_rate_is_not_an_error() {
    // The lid shut, a monitor only.
    let mut api = FakeDisplays::new(Some(vec![]));
    let mut battery_rate = BatteryRate::default();

    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();

    assert!(!battery_rate.retry_pending());
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
        .on_resume(true, &config(true), &mut api)
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
        .on_resume(true, &config(true), &mut api)
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
        .on_resume(true, &config(true), &mut api)
        .unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60]);
}

#[test]
fn a_resume_on_ac_after_the_rate_was_put_back_changes_nothing() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    for event in [PowerEvent::OnBattery, PowerEvent::OnAc] {
        battery_rate
            .on_event(event, &config(true), &mut api)
            .unwrap();
    }

    battery_rate
        .on_resume(false, &config(true), &mut api)
        .unwrap();

    let rates: Vec<u32> = api.set.iter().map(|(_, mode, _)| mode.hz).collect();
    assert_eq!(rates, [60, 240]);
}

#[test]
fn a_resume_on_ac_puts_the_rate_back() {
    // The charger went in during the sleep, before its power event.
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();

    battery_rate
        .on_resume(false, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn a_resume_on_ac_with_no_saved_rate_changes_nothing() {
    let mut api = panel_at(240);

    BatteryRate::default()
        .on_resume(false, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.set, []);
}

#[test]
fn a_put_back_whose_rate_read_fails_keeps_the_rate_for_the_next_try() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    let mut no_list = FakeDisplays::new(None);
    assert!(battery_rate.put_back(&mut no_list).is_err());

    battery_rate.put_back(&mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
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
        .on_resume(true, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.set, []);
}

/// On battery at 60 Hz, then a wake that put 240 Hz back.
fn woken_at_240(api: &mut FakeDisplays) -> BatteryRate {
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), api)
        .unwrap();
    set_refresh_rate(api, RefreshRate(240)).unwrap();
    battery_rate
}

#[test]
fn a_60_hz_refused_after_a_wake_is_tried_again_on_the_next_polls() {
    let mut api = panel_at(240);
    let mut battery_rate = woken_at_240(&mut api);
    api.failing_sets = 2;

    // Not an error yet: the tries show only the last one.
    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();
    assert!(battery_rate.retry_pending());
    battery_rate.retry(&config(true), &mut api).unwrap();
    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
    assert!(!battery_rate.retry_pending());
}

#[test]
fn the_tries_after_a_wake_stop_with_the_last_error() {
    let mut api = panel_at(240);
    let mut battery_rate = woken_at_240(&mut api);
    api.failing_sets = usize::MAX;

    // Not an error yet: the tries show only the last one.
    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();
    let mut results = Vec::new();
    while battery_rate.retry_pending() {
        results.push(battery_rate.retry(&config(true), &mut api).is_ok());
    }

    assert_eq!(results.len(), usize::from(blade_core::RESUME_RATE_TRIES));
    assert_eq!(results.iter().filter(|ok| !**ok).count(), 1);
    assert_eq!(results.last(), Some(&false));
}

#[test]
fn a_try_that_fails_for_good_ends_the_tries_with_its_error() {
    let mut api = panel_at(240);
    let mut battery_rate = woken_at_240(&mut api);
    api.failing_sets = 1;
    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();
    // Such as another program changing the panel's modes: 60 Hz is gone.
    api.displays.as_mut().unwrap()[0]
        .modes
        .retain(|mode| mode.hz != 60);
    let before = api.set.len();

    let tried = battery_rate.retry(&config(true), &mut api);

    assert!(matches!(tried, Err(DisplayError::Unsupported(_))));
    assert_eq!(api.set.len(), before);
    assert!(!battery_rate.retry_pending());
}

#[test]
fn ac_stops_the_tries_after_a_wake() {
    let mut api = panel_at(240);
    let mut battery_rate = woken_at_240(&mut api);
    api.failing_sets = 1;
    let _ = battery_rate.on_resume(true, &config(true), &mut api);

    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    assert!(!battery_rate.retry_pending());
}

#[test]
fn turning_the_option_off_stops_the_tries_after_a_wake() {
    let mut api = panel_at(240);
    let mut battery_rate = woken_at_240(&mut api);
    api.failing_sets = 1;
    let _ = battery_rate.on_resume(true, &config(true), &mut api);
    let before = api.set.len();

    battery_rate.retry(&config(false), &mut api).unwrap();

    assert_eq!(api.set.len(), before);
    assert!(!battery_rate.retry_pending());
}

#[test]
fn a_battery_event_keeps_the_tries_after_a_wake() {
    let mut api = panel_at(240);
    let mut battery_rate = woken_at_240(&mut api);
    api.failing_sets = 1;
    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();

    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

/// The panel at 60 Hz, as a crash on battery leaves it, with `saved` the
/// mode Windows has saved.
fn left_at_60(saved: Option<DisplayMode>) -> FakeDisplays {
    let mut api = panel_at(60);
    api.saved = saved;
    api
}

#[test]
fn a_start_on_battery_at_60_hz_puts_the_saved_rate_back_on_ac() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));

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
    assert_eq!(lives, [(240, ModeLife::UntilRestart)]);
}

#[test]
fn a_saved_60_hz_is_nothing_to_put_back() {
    let mut api = left_at_60(Some(mode(3840, 2400, 60)));

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    assert_eq!(api.set, []);
}

#[test]
fn a_saved_mode_at_another_resolution_is_not_put_back() {
    let mut api = left_at_60(Some(mode(1920, 1200, 240)));

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    assert_eq!(api.set, []);
}

#[test]
fn a_saved_rate_the_panel_does_not_offer_is_not_put_back() {
    let mut api = left_at_60(Some(mode(3840, 2400, 75)));

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    assert_eq!(api.set, []);
}

#[test]
fn a_put_back_is_saved_when_windows_saved_60_hz() {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    // Such as a scale change in Windows Settings on battery.
    api.saved = Some(mode(3840, 2400, 60));

    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.set.last().unwrap().2, ModeLife::Saved);
    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn a_start_on_battery_not_at_60_hz_puts_its_own_rate_back() {
    let mut api = panel_at(120);
    api.saved = Some(mode(3840, 2400, 240));

    feed(
        &[PowerEvent::OnBattery, PowerEvent::OnAc],
        config(true),
        &mut api,
    );

    assert_eq!(api.panel_mode(), mode(3840, 2400, 120));
}

#[test]
fn a_wake_after_a_start_at_60_hz_sets_60_hz_again() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    set_refresh_rate(&mut api, RefreshRate(240)).unwrap();

    battery_rate
        .on_resume(true, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_quit_after_a_start_at_60_hz_puts_the_saved_rate_back() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();

    battery_rate.put_back(&mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

/// The first source a fresh [`BatteryRate`] gets is AC, with the switch
/// as `on`.
fn start_on_ac(on: bool, api: &mut FakeDisplays) {
    feed(&[PowerEvent::OnAc], config(on), api);
}

#[test]
fn a_start_on_ac_at_60_hz_puts_the_saved_rate_back_unsaved() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));

    start_on_ac(true, &mut api);

    let lives: Vec<(u32, ModeLife)> = api
        .set
        .iter()
        .map(|(_, mode, life)| (mode.hz, *life))
        .collect();
    assert_eq!(lives, [(240, ModeLife::UntilRestart)]);
}

#[test]
fn a_start_on_ac_with_the_switch_off_changes_nothing() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));

    start_on_ac(false, &mut api);

    assert_eq!(api.set, []);
}

#[test]
fn a_start_on_ac_not_at_60_hz_changes_nothing() {
    let mut api = panel_at(120);
    api.saved = Some(mode(3840, 2400, 240));

    start_on_ac(true, &mut api);

    assert_eq!(api.set, []);
}

#[test]
fn a_start_on_ac_puts_back_no_saved_rate_that_is_60_hz_not_offered_or_missing() {
    for saved in [
        Some(mode(3840, 2400, 60)),
        Some(mode(3840, 2400, 75)),
        Some(mode(1920, 1200, 240)),
        None,
    ] {
        let mut api = left_at_60(saved);

        start_on_ac(true, &mut api);

        assert_eq!(api.set, [], "saved {saved:?}");
    }
}

#[test]
fn a_start_on_ac_with_no_panel_active_is_not_an_error() {
    let mut api = FakeDisplays::new(Some(vec![]));

    start_on_ac(true, &mut api);

    assert_eq!(api.set, []);
}

#[test]
fn ac_after_a_start_on_ac_put_back_sends_nothing_more() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();
    battery_rate.put_back(&mut api).unwrap();

    assert_eq!(api.set.len(), 1);
}

#[test]
fn a_source_after_the_first_puts_no_saved_rate_back() {
    // Such as another program's 60 Hz, not saved, set after the start.
    for first in [PowerEvent::OnAc, PowerEvent::OnBattery] {
        let mut api = panel_at(240);
        api.saved = Some(mode(3840, 2400, 240));
        let mut battery_rate = BatteryRate::default();
        battery_rate
            .on_event(first, &config(false), &mut api)
            .unwrap();
        api.displays.as_mut().unwrap()[0].current = mode(3840, 2400, 60);

        battery_rate
            .on_event(PowerEvent::OnAc, &config(true), &mut api)
            .unwrap();

        assert_eq!(api.set, [], "first {first:?}");
    }
}

#[test]
fn a_battery_read_at_a_wake_counts_as_the_first_source() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_resume(true, &config(false), &mut api)
        .unwrap();

    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.set, []);
}

#[test]
fn a_first_ac_read_at_a_wake_puts_the_saved_rate_back() {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));

    BatteryRate::default()
        .on_resume(false, &config(true), &mut api)
        .unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn a_first_ac_whose_display_query_fails_is_an_error_and_sets_nothing() {
    let mut api = FakeDisplays::new(None);

    let result = BatteryRate::default().on_event(PowerEvent::OnAc, &config(true), &mut api);

    assert!(matches!(result, Err(DisplayError::Query(_))));
    assert_eq!(api.set, []);
}

/// A first AC [`left_at_60`] with 240 Hz saved, whose put-back Windows
/// refuses `refusals` times. The first refusal is the result.
fn refused_at_the_first_ac(refusals: usize) -> (BatteryRate, FakeDisplays) {
    let mut api = left_at_60(Some(mode(3840, 2400, 240)));
    api.failing_sets = refusals;
    let mut battery_rate = BatteryRate::default();
    let first = battery_rate.on_event(PowerEvent::OnAc, &config(true), &mut api);
    assert!(matches!(first, Err(DisplayError::Set(_))));
    (battery_rate, api)
}

#[test]
fn a_second_ac_or_a_wake_on_ac_keeps_the_put_back_tries() {
    // The app's start puts back, then Windows sends AC at once.
    for wake in [false, true] {
        let (mut battery_rate, mut api) = refused_at_the_first_ac(1);
        let before = api.set.len();

        if wake {
            battery_rate.on_resume(false, &config(true), &mut api)
        } else {
            battery_rate.on_event(PowerEvent::OnAc, &config(true), &mut api)
        }
        .unwrap();
        assert_eq!(api.set.len(), before, "wake {wake}");
        assert!(battery_rate.retry_pending(), "wake {wake}");
        battery_rate.retry(&config(true), &mut api).unwrap();
        battery_rate.retry(&config(true), &mut api).unwrap();

        assert_eq!(api.panel_mode(), mode(3840, 2400, 240), "wake {wake}");
    }
}

#[test]
fn a_refused_put_back_at_the_first_ac_is_tried_again_after_a_poll() {
    let (mut battery_rate, mut api) = refused_at_the_first_ac(1);
    assert!(battery_rate.retry_pending());
    assert!(battery_rate.retries_put_back());

    // The first poll may come at once, so it is skipped.
    battery_rate.retry(&config(true), &mut api).unwrap();
    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
    assert_eq!(api.set.last().unwrap().2, ModeLife::UntilRestart);
    assert!(!battery_rate.retry_pending());
}

#[test]
fn the_put_back_tries_stop_with_the_last_error() {
    let (mut battery_rate, mut api) = refused_at_the_first_ac(usize::MAX);

    let mut results = Vec::new();
    while battery_rate.retry_pending() {
        results.push(battery_rate.retry(&config(true), &mut api).is_ok());
    }

    // The skipped poll, then the tries.
    assert_eq!(
        results.len(),
        usize::from(blade_core::RESUME_RATE_TRIES) + 1
    );
    assert_eq!(results.iter().filter(|ok| !**ok).count(), 1);
    assert_eq!(results.last(), Some(&false));
    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_put_back_try_sends_nothing_once_the_panel_left_60_hz() {
    let (mut battery_rate, mut api) = refused_at_the_first_ac(1);
    // Such as the user picking 120 Hz in Windows Settings.
    api.displays.as_mut().unwrap()[0].current = mode(3840, 2400, 120);
    let before = api.set.len();

    battery_rate.retry(&config(true), &mut api).unwrap();
    battery_rate.retry(&config(true), &mut api).unwrap();

    assert_eq!(api.set.len(), before);
    assert!(!battery_rate.retry_pending());
}

#[test]
fn battery_a_wake_on_battery_or_the_option_off_stops_the_put_back_tries() {
    let (mut battery_rate, mut api) = refused_at_the_first_ac(1);
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    assert!(!battery_rate.retry_pending());

    let (mut battery_rate, mut api) = refused_at_the_first_ac(1);
    battery_rate
        .on_resume(true, &config(false), &mut api)
        .unwrap();
    assert!(!battery_rate.retry_pending());

    let (mut battery_rate, mut api) = refused_at_the_first_ac(1);
    let before = api.set.len();
    battery_rate.retry(&config(false), &mut api).unwrap();
    battery_rate.retry(&config(false), &mut api).unwrap();
    assert_eq!(api.set.len(), before);
    assert!(!battery_rate.retry_pending());
}

#[test]
fn a_first_ac_with_no_panel_starts_no_tries() {
    let mut no_panel = FakeDisplays::new(Some(vec![]));
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnAc, &config(true), &mut no_panel)
        .unwrap();
    assert!(!battery_rate.retry_pending());
}

#[test]
fn a_switch_s_60_hz_tries_are_not_put_back_tries() {
    let mut api = panel_at(240);
    api.failing_sets = 1;
    let mut battery_rate = BatteryRate::default();
    let _ = battery_rate.on_event(PowerEvent::OnBattery, &config(true), &mut api);

    assert!(battery_rate.retry_pending());
    assert!(!battery_rate.retries_put_back());
}

/// On battery at 60 Hz, with the display list seen once.
fn on_battery_at_60() -> (BatteryRate, FakeDisplays) {
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    // The first read's round runs out.
    for _ in 0..DISPLAY_SETTLE_POLLS {
        battery_rate.on_displays(true, &config(true), &mut api);
    }
    (battery_rate, api)
}

/// Windows puts the panel back at its saved rate, as a GPU mode switch or
/// Win + P does (check 79).
fn windows_puts_back_240(api: &mut FakeDisplays) {
    api.displays.as_mut().unwrap()[0].current = mode(3840, 2400, 240);
}

#[test]
fn a_gpu_mode_switch_on_battery_sets_60_hz_again() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    windows_puts_back_240(&mut api);
    // NVIDIA-only mode: the panel's path is DisplayPort (ticket 282).
    api.displays.as_mut().unwrap()[0].connection = 10;

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_display_that_comes_or_goes_on_battery_sets_60_hz_again() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    windows_puts_back_240(&mut api);
    api.displays.as_mut().unwrap().push(monitor());

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_rate_change_alone_is_the_users_and_kept() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    windows_puts_back_240(&mut api);

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn a_display_change_on_ac_or_with_the_option_off_sets_nothing() {
    for (on_battery, option) in [(false, true), (true, false)] {
        let (mut battery_rate, mut api) = on_battery_at_60();
        windows_puts_back_240(&mut api);
        api.displays.as_mut().unwrap()[0].connection = 10;
        let sets = api.set.len();

        battery_rate.on_displays(on_battery, &config(option), &mut api);

        assert_eq!(api.set.len(), sets);
    }
}

#[test]
fn the_first_read_on_battery_begins_a_round() {
    // A monitor that also charged goes with the switch to battery, and
    // Windows puts its rate back after the switch's 60 Hz.
    let mut api = panel_at(240);
    let mut battery_rate = BatteryRate::default();
    battery_rate
        .on_event(PowerEvent::OnBattery, &config(true), &mut api)
        .unwrap();
    assert_eq!(
        battery_rate.on_displays(true, &config(true), &mut api),
        DisplayChange::None
    );
    windows_puts_back_240(&mut api);

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn the_last_settle_poll_still_sets_60_hz() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    api.displays.as_mut().unwrap()[0].connection = 10;
    for _ in 1..DISPLAY_SETTLE_POLLS {
        battery_rate.on_displays(true, &config(true), &mut api);
    }
    windows_puts_back_240(&mut api);

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

/// Changes the list on each of `polls` polls; returns the rounds begun.
fn keep_changing(battery_rate: &mut BatteryRate, api: &mut FakeDisplays, polls: usize) -> u8 {
    let mut rounds = 0;
    for _ in 0..polls {
        let connection = &mut api.displays.as_mut().unwrap()[0].connection;
        *connection = connection.wrapping_add(1);
        if battery_rate.on_displays(true, &config(true), api) == DisplayChange::Began {
            rounds += 1;
        }
    }
    rounds
}

#[test]
fn a_list_that_keeps_changing_begins_at_most_its_rounds() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    let polls = usize::from(DISPLAY_ROUNDS + 2) * usize::from(DISPLAY_SETTLE_POLLS);

    assert_eq!(
        keep_changing(&mut battery_rate, &mut api, polls),
        DISPLAY_ROUNDS
    );
}

#[test]
fn a_quiet_list_lets_rounds_begin_again() {
    // Check 86: five switches a minute or less apart used up the rounds.
    let (mut battery_rate, mut api) = on_battery_at_60();
    let polls = usize::from(DISPLAY_ROUNDS + 2) * usize::from(DISPLAY_SETTLE_POLLS);
    keep_changing(&mut battery_rate, &mut api, polls);
    for _ in 0..DISPLAY_QUIET_POLLS {
        battery_rate.on_displays(true, &config(true), &mut api);
    }

    assert_eq!(keep_changing(&mut battery_rate, &mut api, 1), 1);
}

#[test]
fn a_failed_display_read_keeps_the_list_seen() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    let seen = api.displays.take();
    battery_rate.on_displays(true, &config(true), &mut api);
    api.displays = seen;
    windows_puts_back_240(&mut api);

    battery_rate.on_displays(true, &config(true), &mut api);

    // The list is as seen before the failed read: a rate change alone.
    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn a_refused_set_after_a_display_change_is_tried_again() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    windows_puts_back_240(&mut api);
    api.displays.as_mut().unwrap()[0].connection = 10;
    api.failing_sets = 1;

    battery_rate.on_displays(true, &config(true), &mut api);

    assert!(battery_rate.retry_pending());
    battery_rate.retry(&config(true), &mut api).unwrap();
    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn a_rate_put_back_a_poll_after_the_list_change_still_gets_60_hz() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    api.displays.as_mut().unwrap()[0].connection = 10;
    battery_rate.on_displays(true, &config(true), &mut api);
    // Windows puts its rate back only now.
    windows_puts_back_240(&mut api);

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 60));
}

#[test]
fn the_rate_is_checked_only_for_the_settle_polls_after_a_change() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    api.displays.as_mut().unwrap()[0].connection = 10;
    for _ in 0..DISPLAY_SETTLE_POLLS {
        battery_rate.on_displays(true, &config(true), &mut api);
    }
    windows_puts_back_240(&mut api);

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.panel_mode(), mode(3840, 2400, 240));
}

#[test]
fn a_list_that_keeps_changing_does_not_fill_the_tries_again() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    windows_puts_back_240(&mut api);
    api.failing_sets = usize::MAX;
    let mut errors = 0;
    for poll in 0..usize::from(RESUME_RATE_TRIES) * 3 {
        // A flaky cable: the monitor comes and goes each poll.
        let list = api.displays.as_mut().unwrap();
        if poll % 2 == 0 {
            list.push(monitor());
        } else {
            list.truncate(1);
        }
        battery_rate.on_displays(true, &config(true), &mut api);
        if battery_rate.retry_pending() && battery_rate.retry(&config(true), &mut api).is_err() {
            errors += 1;
        }
    }

    assert!(errors > 0, "the tries never ended");
}

#[test]
fn a_panel_gone_from_the_list_is_left_alone() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    let sets = api.set.len();
    // The lid shut on a monitor: only the monitor is active.
    api.displays = Some(vec![monitor()]);

    battery_rate.on_displays(true, &config(true), &mut api);

    assert_eq!(api.set.len(), sets);
    assert!(!battery_rate.retry_pending());
}

/// The cap used up by a list that keeps changing.
fn capped() -> (BatteryRate, FakeDisplays) {
    let (mut battery_rate, mut api) = on_battery_at_60();
    let polls = usize::from(DISPLAY_ROUNDS + 2) * usize::from(DISPLAY_SETTLE_POLLS);
    keep_changing(&mut battery_rate, &mut api, polls);
    (battery_rate, api)
}

#[test]
fn less_than_a_quiet_minute_keeps_the_cap() {
    let (mut battery_rate, mut api) = capped();
    for _ in 1..DISPLAY_QUIET_POLLS {
        battery_rate.on_displays(true, &config(true), &mut api);
    }

    assert_eq!(keep_changing(&mut battery_rate, &mut api, 1), 0);
}

#[test]
fn a_failed_read_does_not_count_as_quiet() {
    let (mut battery_rate, mut api) = capped();
    let seen = api.displays.clone();
    for poll in 0..DISPLAY_QUIET_POLLS {
        api.displays = if poll % 2 == 0 { None } else { seen.clone() };
        battery_rate.on_displays(true, &config(true), &mut api);
    }
    api.displays = seen;

    assert_eq!(keep_changing(&mut battery_rate, &mut api, 1), 0);
}

#[test]
fn a_change_the_cap_refuses_is_told_once() {
    let (mut battery_rate, mut api) = on_battery_at_60();
    let mut told = 0;
    for _ in 0..usize::from(DISPLAY_ROUNDS + 4) * usize::from(DISPLAY_SETTLE_POLLS) {
        let connection = &mut api.displays.as_mut().unwrap()[0].connection;
        *connection = connection.wrapping_add(1);
        if battery_rate.on_displays(true, &config(true), &mut api) == DisplayChange::Capped {
            told += 1;
        }
    }

    assert_eq!(told, 1);
}
