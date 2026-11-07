use std::time::{Duration, Instant};

use blade_core::{POWER_KEY_PAUSE, PowerKeyPresses, Preset, next_preset};
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
fn a_key_held_before_the_first_key_up_does_not_count() {
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    let now = Instant::now();

    // The key may have been held while the reports were opened.
    assert!(!presses.on_report(&report(&KEY_DOWN), now));
    assert!(!presses.on_report(&report(&FN_DOWN), now));
    assert!(presses.on_report(&report(&KEY_DOWN), now));
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
