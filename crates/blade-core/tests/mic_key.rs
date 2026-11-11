//! Fn + mic: the keyboard sends Fn in its key reports and Win + Alt + K to
//! Windows, so a Win + Alt + K seen while Fn is held is a mic key press.

use std::time::Instant;

use blade_core::{FN_HOLD_LIMIT, POWER_KEY_PAUSE, PowerKeyPresses};
use razer_proto::BLADE_18;

/// A 16-byte key report as the laptop sends it.
fn report(bytes: &[u8]) -> [u8; 16] {
    let mut report = [0; 16];
    report[..bytes.len()].copy_from_slice(bytes);
    report
}

const FN_DOWN: [u8; 2] = [0x04, 0x0A];
const ALL_UP: [u8; 1] = [0x04];

/// Presses with Fn just pressed and Win + Alt + K seen up.
fn fn_held_mic_up(now: Instant) -> PowerKeyPresses {
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    presses.on_report(&report(&ALL_UP), now);
    presses.on_report(&report(&FN_DOWN), now);
    assert!(!presses.on_mic(false, now));
    presses
}

#[test]
fn the_mic_keys_pressed_while_fn_is_held_toggle_once() {
    let now = Instant::now();
    let mut presses = fn_held_mic_up(now);

    assert!(presses.on_mic(true, now));
    // Still held: the same press.
    assert!(!presses.on_mic(true, now + POWER_KEY_PAUSE));
}

#[test]
fn the_mic_keys_without_fn_do_not_toggle() {
    // Win + Alt + K typed on the main keys.
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    let now = Instant::now();
    presses.on_report(&report(&ALL_UP), now);

    assert!(!presses.on_mic(false, now));
    assert!(!presses.on_mic(true, now));
}

#[test]
fn the_mic_keys_already_down_when_fn_comes_do_not_toggle() {
    let mut presses = PowerKeyPresses::new(BLADE_18.power_key.unwrap());
    let now = Instant::now();
    presses.on_report(&report(&FN_DOWN), now);

    assert!(!presses.on_mic(true, now));
}

#[test]
fn a_second_press_while_fn_is_held_toggles_again() {
    let start = Instant::now();
    let later = start + POWER_KEY_PAUSE;
    let mut presses = fn_held_mic_up(start);

    assert!(presses.on_mic(true, start));
    assert!(!presses.on_mic(false, later));
    assert!(presses.on_mic(true, later));
}

#[test]
fn presses_closer_than_the_pause_toggle_once() {
    let start = Instant::now();
    let mut presses = fn_held_mic_up(start);

    assert!(presses.on_mic(true, start));
    assert!(!presses.on_mic(false, start));
    assert!(!presses.on_mic(true, start));
}

#[test]
fn a_power_step_does_not_hold_back_the_mic() {
    let now = Instant::now();
    let mut presses = fn_held_mic_up(now);

    assert!(!presses.on_enter(false, now));
    assert!(presses.on_enter(true, now));
    assert!(presses.on_mic(true, now));
}

#[test]
fn the_mic_keys_seen_before_a_release_are_forgotten_by_the_next_fn() {
    let start = Instant::now();
    let later = start + POWER_KEY_PAUSE;
    let mut presses = fn_held_mic_up(start);
    presses.on_report(&report(&ALL_UP), start);

    presses.on_report(&report(&FN_DOWN), later);
    assert!(!presses.on_mic(true, later));
}

#[test]
fn an_fn_held_past_the_limit_does_not_toggle() {
    let start = Instant::now();
    let late = start + FN_HOLD_LIMIT;
    let mut presses = fn_held_mic_up(start);

    assert!(!presses.on_mic(true, late));
}
