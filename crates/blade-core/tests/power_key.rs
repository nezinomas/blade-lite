use std::time::{Duration, Instant};

use blade_core::{
    BRIGHTNESS_HOLD_LIMIT, BRIGHTNESS_READ_GAP, FN_HOLD_LIMIT, POWER_KEY_PAUSE, PowerKeyPresses,
    Preset, next_preset,
};
use razer_proto::{BLADE_18, ModelProfile, PerfMode};

/// A 16-byte key report as the laptop sends it.
fn report(bytes: &[u8]) -> [u8; 16] {
    let mut report = [0; 16];
    report[..bytes.len()].copy_from_slice(bytes);
    report
}

const FN_DOWN: [u8; 2] = [0x04, 0x0A];
const KEY_DOWN: [u8; 3] = [0x04, 0x0A, 0xD3];
const ALL_UP: [u8; 1] = [0x04];

/// Presses that have seen the key up once, so the next press counts.
fn presses() -> PowerKeyPresses {
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    assert!(!presses.on_report(&report(&ALL_UP), Instant::now()));
    presses
}

#[test]
fn a_first_report_with_the_key_counts_once() {
    // Key reports come only on a change, so a first report listing the
    // key is almost always a new press (ticket 174).
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    let now = Instant::now();

    assert!(presses.on_report(&report(&[0x04, 0xD3]), now));
    assert!(!presses.on_report(&report(&[0x04, 0xD3]), now + POWER_KEY_PAUSE));
}

#[test]
fn a_first_fn_and_numpad_enter_steps_once() {
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    let now = Instant::now();

    assert!(presses.on_report(&report(&KEY_DOWN), now));
    // Windows may get the Enter too: Enter starts unknown, so no second step.
    assert!(!presses.on_enter(true, now + POWER_KEY_PAUSE));
}

#[test]
fn on_ac_the_key_steps_through_the_presets_and_wraps() {
    let next = |preset| next_preset(preset, &BLADE_18, false);

    assert_eq!(next(Preset::Silent), Some(Preset::Balanced));
    assert_eq!(next(Preset::Balanced), Some(Preset::Performance));
    assert_eq!(next(Preset::Performance), Some(Preset::Silent));
}

#[test]
fn custom_steps_to_the_first_preset() {
    // Custom is left out of the cycle: it may hold a manual fan speed.
    assert_eq!(
        next_preset(Preset::Custom, &BLADE_18, false),
        Some(Preset::Silent)
    );
}

#[test]
fn on_battery_the_key_steps_only_through_the_presets_offered_there() {
    let next = |preset| next_preset(preset, &BLADE_18, true);

    assert_eq!(next(Preset::Silent), Some(Preset::Balanced));
    assert_eq!(next(Preset::Balanced), Some(Preset::Silent));
    assert_eq!(next(Preset::Performance), Some(Preset::Silent));
    assert_eq!(next(Preset::Custom), Some(Preset::Silent));
}

#[test]
fn a_model_with_no_presets_has_no_next_one() {
    let none = ModelProfile {
        presets: &[],
        ..BLADE_18
    };

    assert_eq!(next_preset(Preset::Balanced, &none, false), None);
}

#[test]
fn a_model_with_only_performance_has_no_next_one_on_battery() {
    let performance_only = ModelProfile {
        presets: &[PerfMode::Performance],
        ..BLADE_18
    };

    assert_eq!(next_preset(Preset::Balanced, &performance_only, true), None);
}

#[test]
fn a_press_counts_once_while_the_key_is_held() {
    let mut presses = presses();
    let now = Instant::now();

    assert!(!presses.on_report(&report(&FN_DOWN), now));
    assert!(presses.on_report(&report(&KEY_DOWN), now));
    // The keyboard may send the held list again.
    assert!(!presses.on_report(&report(&KEY_DOWN), now + POWER_KEY_PAUSE));
}

#[test]
fn a_second_press_counts_after_a_release() {
    let mut presses = presses();
    let start = Instant::now();
    let later = start + POWER_KEY_PAUSE;

    assert!(presses.on_report(&report(&KEY_DOWN), start));
    assert!(!presses.on_report(&report(&FN_DOWN), later));
    assert!(presses.on_report(&report(&KEY_DOWN), later));
    assert!(!presses.on_report(&report(&ALL_UP), later));
}

#[test]
fn presses_closer_than_the_pause_count_once() {
    let mut presses = presses();
    let start = Instant::now();
    let soon = start + POWER_KEY_PAUSE.saturating_sub(Duration::from_millis(1));

    assert!(presses.on_report(&report(&KEY_DOWN), start));
    assert!(!presses.on_report(&report(&ALL_UP), soon));
    assert!(!presses.on_report(&report(&KEY_DOWN), soon));
}

#[test]
fn other_reports_do_not_change_the_key_state() {
    let mut presses = presses();
    let start = Instant::now();
    let later = start + POWER_KEY_PAUSE;

    assert!(presses.on_report(&report(&KEY_DOWN), start));
    // Not a key report, so the key still counts as held.
    assert!(!presses.on_report(&report(&[0x05]), later));
    assert!(!presses.on_report(&[], later));
    assert!(!presses.on_report(&report(&KEY_DOWN), later));
}

/// Presses with Fn just pressed and Enter seen up, as with Num Lock on:
/// the keyboard sends only Fn, and Windows gets a plain Enter.
fn fn_held_enter_up(now: Instant) -> PowerKeyPresses {
    let mut presses = presses();
    assert!(!presses.on_report(&report(&FN_DOWN), now));
    assert!(!presses.on_enter(false, now));
    presses
}

#[test]
fn enter_is_watched_only_while_fn_is_held() {
    let mut presses = presses();
    let now = Instant::now();
    assert!(!presses.watching_enter(now));

    presses.on_report(&report(&FN_DOWN), now);
    assert!(presses.watching_enter(now));

    presses.on_report(&report(&ALL_UP), now);
    assert!(!presses.watching_enter(now));
}

#[test]
fn enter_pressed_while_fn_is_held_steps() {
    let now = Instant::now();
    let mut presses = fn_held_enter_up(now);

    assert!(presses.on_enter(true, now));
    // Still held: the same press.
    assert!(!presses.on_enter(true, now + POWER_KEY_PAUSE));
}

#[test]
fn enter_without_fn_does_not_step() {
    let mut presses = presses();
    let now = Instant::now();

    assert!(!presses.on_enter(false, now));
    assert!(!presses.on_enter(true, now));
}

#[test]
fn enter_already_down_when_fn_comes_does_not_step() {
    let mut presses = presses();
    let now = Instant::now();
    presses.on_report(&report(&FN_DOWN), now);

    // Enter first, then Fn: not the power-mode key.
    assert!(!presses.on_enter(true, now));
    assert!(!presses.on_enter(true, now));
}

#[test]
fn enter_seen_before_a_release_is_forgotten_by_the_next_fn() {
    let start = Instant::now();
    let later = start + POWER_KEY_PAUSE;
    let mut presses = fn_held_enter_up(start);
    presses.on_report(&report(&ALL_UP), start);

    // Enter is pressed, Fn comes after it.
    presses.on_report(&report(&FN_DOWN), later);
    assert!(!presses.on_enter(true, later));
}

#[test]
fn a_second_enter_while_fn_is_held_steps_again() {
    let start = Instant::now();
    let later = start + POWER_KEY_PAUSE;
    let mut presses = fn_held_enter_up(start);

    assert!(presses.on_enter(true, start));
    assert!(!presses.on_enter(false, later));
    assert!(presses.on_enter(true, later));
}

#[test]
fn enter_and_the_power_key_of_one_press_step_once() {
    // If Windows also gets an Enter while the keyboard sends the key.
    let now = Instant::now();
    let mut presses = fn_held_enter_up(now);

    assert!(presses.on_report(&report(&KEY_DOWN), now));
    assert!(!presses.on_enter(true, now));
}

#[test]
fn enter_then_the_power_key_of_one_press_step_once() {
    let now = Instant::now();
    let mut presses = fn_held_enter_up(now);

    assert!(presses.on_enter(true, now));
    assert!(!presses.on_report(&report(&KEY_DOWN), now));
}

#[test]
fn an_fn_held_past_the_limit_is_not_watched() {
    // A release report lost, such as over a sleep, must not leave every
    // Enter stepping the preset.
    let start = Instant::now();
    let late = start + FN_HOLD_LIMIT;
    let mut presses = fn_held_enter_up(start);

    assert!(!presses.watching_enter(late));
    assert!(!presses.on_enter(true, late));
}

#[test]
fn fn_held_again_in_the_next_report_keeps_its_start() {
    let start = Instant::now();
    let late = start + FN_HOLD_LIMIT;
    let mut presses = fn_held_enter_up(start);

    // The power key's report also lists Fn; Fn is still the same hold.
    presses.on_report(&report(&FN_DOWN), late);
    assert!(!presses.watching_enter(late));
}

#[test]
fn fn_comes_up_when_a_report_drops_it() {
    let mut presses = presses();
    let now = Instant::now();

    presses.on_report(&report(&FN_DOWN), now);
    assert!(!presses.fn_came_up());
    presses.on_report(&report(&ALL_UP), now);
    assert!(presses.fn_came_up());
}

#[test]
fn fn_comes_up_once_per_hold() {
    let mut presses = presses();
    let now = Instant::now();

    presses.on_report(&report(&FN_DOWN), now);
    presses.on_report(&report(&FN_DOWN), now);
    assert!(!presses.fn_came_up());
    presses.on_report(&report(&ALL_UP), now);
    presses.on_report(&report(&ALL_UP), now);
    assert!(!presses.fn_came_up());
}

#[test]
fn fn_comes_up_after_the_power_key_too() {
    let mut presses = presses();
    let now = Instant::now();

    presses.on_report(&report(&KEY_DOWN), now);
    presses.on_report(&report(&ALL_UP), now);

    assert!(presses.fn_came_up());
}

#[test]
fn fn_does_not_come_up_before_it_was_seen_down() {
    // Fn may have been held while the reports were opened: its release
    // is still a release, but a first report without Fn is not.
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());

    presses.on_report(&report(&ALL_UP), Instant::now());

    assert!(!presses.fn_came_up());
}

#[test]
fn another_report_after_fn_came_up_is_not_a_second_release() {
    let mut presses = presses();
    let now = Instant::now();
    presses.on_report(&report(&FN_DOWN), now);
    presses.on_report(&report(&ALL_UP), now);

    presses.on_report(&report(&[0x05]), now);

    assert!(!presses.fn_came_up());
}

/// Presses with Fn just gone down at `start`.
fn fn_down_at(start: Instant) -> PowerKeyPresses {
    let mut presses = presses();
    presses.on_report(&report(&FN_DOWN), start);
    presses
}

#[test]
fn the_brightness_is_read_each_gap_while_fn_is_held() {
    let start = Instant::now();
    let mut presses = fn_down_at(start);

    assert!(!presses.brightness_due(start));
    assert!(!presses.brightness_due(start + BRIGHTNESS_READ_GAP / 2));
    assert!(presses.brightness_due(start + BRIGHTNESS_READ_GAP));
    assert!(!presses.brightness_due(start + BRIGHTNESS_READ_GAP));
    assert!(presses.brightness_due(start + BRIGHTNESS_READ_GAP * 2));
}

#[test]
fn the_brightness_is_not_read_without_fn() {
    let start = Instant::now();
    let mut presses = presses();

    assert!(!presses.brightness_due(start + BRIGHTNESS_READ_GAP * 2));
    let mut presses_up = fn_down_at(start);
    presses_up.on_report(&report(&ALL_UP), start);
    assert!(!presses_up.brightness_due(start + BRIGHTNESS_READ_GAP * 2));
}

#[test]
fn the_brightness_is_still_read_past_the_enter_limit() {
    // Fn may stay held while F11 is pressed many times (laptop, 2026-11-10).
    let start = Instant::now();
    let mut presses = fn_down_at(start);

    assert!(presses.brightness_due(start + FN_HOLD_LIMIT + BRIGHTNESS_READ_GAP));
}

#[test]
fn the_brightness_is_not_read_past_its_hold_limit() {
    // A lost release must not keep reading for ever; Fn up reads once more.
    let start = Instant::now();
    let mut presses = fn_down_at(start);

    assert!(
        presses
            .brightness_due(start + BRIGHTNESS_HOLD_LIMIT.saturating_sub(Duration::from_millis(1)))
    );
    assert!(!presses.brightness_due(start + BRIGHTNESS_HOLD_LIMIT + BRIGHTNESS_READ_GAP));
}

#[test]
fn a_new_fn_hold_waits_a_full_gap() {
    let start = Instant::now();
    let mut presses = fn_down_at(start);
    assert!(presses.brightness_due(start + BRIGHTNESS_READ_GAP));
    let again = start + BRIGHTNESS_READ_GAP * 3;
    presses.on_report(&report(&ALL_UP), again);
    presses.on_report(&report(&FN_DOWN), again);

    assert!(!presses.brightness_due(again));
    assert!(presses.brightness_due(again + BRIGHTNESS_READ_GAP));
}

#[test]
fn after_a_quiet_spell_the_key_state_stays_and_the_fn_state_goes() {
    let now = Instant::now();
    let mut presses = fn_held_enter_up(now).after_quiet_spell();

    // Fn's release may have been lost in the gap.
    assert!(!presses.watching_enter(now));
    assert!(!presses.brightness_due(now + BRIGHTNESS_READ_GAP));
    // The key was last seen up, so the first press counts (ticket 158).
    assert!(presses.on_report(&report(&[0x04, 0xD3]), now));
}

#[test]
fn after_a_quiet_spell_the_pause_still_holds() {
    let now = Instant::now();
    let mut presses = presses();
    assert!(presses.on_report(&report(&KEY_DOWN), now));
    assert!(!presses.on_report(&report(&ALL_UP), now));

    let mut presses = presses.after_quiet_spell();

    assert!(!presses.on_report(&report(&KEY_DOWN), now + POWER_KEY_PAUSE / 2));
}
