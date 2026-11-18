//! The keys go dark while Windows has the screen off, as the idle timeout
//! turns it off with no sleep notice (ticket 241).

use std::time::{Duration, Instant, SystemTime};

use blade_core::Config;
use platform::PowerEvent;
use razer_proto::{BLADE_18, Brightness, Command};

use super::sleep::{pause_ends, power};
use super::{FAKE_REOPENED, FAKE_SCREEN_ON, Fixture, fixture};
use crate::shell::Reopened;

fn level(percent: u8) -> Vec<Command> {
    Brightness::new(percent).unwrap().commands(&BLADE_18)
}

fn lit_fixture() -> Fixture {
    let f = fixture(Config::default());
    f.shell
        .app()
        .machine_mut()
        .controller
        .inner
        .brightness_level = Some(255);
    f
}

fn razer_runs(f: &Fixture) {
    f.shell.programs().set(&["Razer Synapse Service"]);
    f.shell.look_for_razer();
}

fn sent(f: &Fixture) -> Vec<Command> {
    f.shell.app().machine().controller.inner.sent.clone()
}

fn forget_sent(f: &Fixture) {
    f.shell.app().machine_mut().controller.inner.sent.clear();
}

fn screen(f: &Fixture, on: bool) {
    FAKE_SCREEN_ON.set(Some(on));
    power(f, &[PowerEvent::ScreenChanged]);
}

#[test]
fn the_screen_off_makes_the_keys_dark_and_the_screen_on_lights_them() {
    let f = lit_fixture();
    forget_sent(&f);

    screen(&f, false);
    assert_eq!(sent(&f), level(0));
    forget_sent(&f);

    screen(&f, true);
    assert_eq!(sent(&f), level(100));
}

#[test]
fn a_screen_not_known_counts_as_on() {
    let f = lit_fixture();
    screen(&f, false);
    forget_sent(&f);

    FAKE_SCREEN_ON.set(None);
    power(&f, &[PowerEvent::ScreenChanged]);

    assert_eq!(sent(&f), level(100));
}

#[test]
fn a_screen_off_in_the_wake_pause_makes_the_keys_dark_after_its_settings() {
    let f = lit_fixture();
    power(&f, &[PowerEvent::Resumed]);
    forget_sent(&f);

    screen(&f, false);
    assert_eq!(sent(&f), [], "nothing goes in the pause");
    pause_ends();

    assert!(sent(&f).ends_with(&level(0)), "{:?}", sent(&f));
}

#[test]
fn nothing_goes_for_the_screen_between_the_sleep_and_the_wake() {
    let f = lit_fixture();
    power(&f, &[PowerEvent::Suspending]);
    forget_sent(&f);

    screen(&f, false);
    screen(&f, true);

    assert_eq!(sent(&f), []);
}

#[test]
fn the_wake_lights_keys_made_dark_before_the_sleep() {
    // With no saved lighting the wake's settings leave the level alone, so
    // the screen on after the sleep must put it back.
    let f = lit_fixture();
    screen(&f, false);
    power(&f, &[PowerEvent::Suspending]);
    FAKE_SCREEN_ON.set(Some(true));
    power(&f, &[PowerEvent::Resumed]);
    forget_sent(&f);

    pause_ends();

    assert!(sent(&f).ends_with(&level(100)), "{:?}", sent(&f));
}

#[test]
fn the_quit_lights_dark_keys_again() {
    let f = lit_fixture();
    screen(&f, false);
    forget_sent(&f);

    f.shell.quit();

    assert!(sent(&f).ends_with(&level(100)), "{:?}", sent(&f));
}

/// Runs a tick `secs` seconds after `start`.
fn tick(f: &Fixture, start: Instant, secs: u64) {
    f.shell
        .tick_at(start + Duration::from_secs(secs), SystemTime::now());
}

/// Whether the keys were lit again since the last forget.
fn lit_again(f: &Fixture) -> bool {
    sent(f)
        .windows(level(100).len())
        .any(|run| run == level(100))
}

#[test]
fn a_failed_put_back_is_tried_again_once_its_spacing_passed() {
    let f = lit_fixture();
    let start = Instant::now();
    screen(&f, false);
    f.shell.app().machine_mut().controller.inner.fails = true;
    screen(&f, true);
    f.shell.app().machine_mut().controller.inner.fails = false;
    forget_sent(&f);

    // Not at each poll (ticket 253).
    f.shell.follow_own_power();
    assert!(!lit_again(&f));

    tick(&f, start, 11);
    assert!(lit_again(&f));
}

#[test]
fn a_put_back_that_keeps_failing_is_given_up_until_the_screen_goes_off_again() {
    let f = lit_fixture();
    let start = Instant::now();
    put_back_given_up(&f, start);

    // Three tries failed: none more.
    tick(&f, start, 33);
    tick(&f, start, 60);
    assert!(!lit_again(&f));

    screen(&f, false);
    screen(&f, true);
    assert!(lit_again(&f));
}

#[test]
fn a_screen_change_in_an_open_again_pause_is_followed_at_its_end() {
    let f = lit_fixture();
    screen(&f, false);
    FAKE_REOPENED.set(Reopened::Opened);
    f.shell.follow_own_power();
    forget_sent(&f);

    screen(&f, true);
    assert_eq!(sent(&f), [], "nothing goes in the pause");
    pause_ends();

    assert!(sent(&f).ends_with(&level(100)), "{:?}", sent(&f));
}

#[test]
fn the_keys_are_not_made_dark_while_razer_software_runs() {
    let f = lit_fixture();
    razer_runs(&f);
    forget_sent(&f);

    screen(&f, false);

    assert_eq!(sent(&f), []);
}

#[test]
fn keys_made_dark_before_razer_software_came_are_still_lit() {
    let f = lit_fixture();
    screen(&f, false);
    razer_runs(&f);
    forget_sent(&f);

    screen(&f, true);

    assert_eq!(sent(&f), level(100));
}

#[test]
fn a_held_controller_waits_for_its_release_to_light_the_keys() {
    let f = lit_fixture();
    screen(&f, false);
    f.shell.app().hold_controller(true);
    let runs = f.shell.app().machine().controller.inner.runs;

    screen(&f, true);
    assert_eq!(f.shell.app().machine().controller.inner.runs, runs);

    f.shell.app().hold_controller(false);
    forget_sent(&f);
    f.shell.follow_own_power();
    assert_eq!(sent(&f), level(100));
}

#[test]
fn a_screen_change_read_before_a_sleep_in_one_look_sends_nothing() {
    // The sleep notice forgot the screen, which then reads as on: the keys
    // stay dark for the sleep.
    let f = lit_fixture();
    screen(&f, false);
    FAKE_SCREEN_ON.set(None);
    forget_sent(&f);

    power(&f, &[PowerEvent::ScreenChanged, PowerEvent::Suspending]);

    let lit = level(100);
    let sent = sent(&f);
    assert!(!sent.windows(lit.len()).any(|run| run == lit), "{sent:?}");
}

#[test]
fn a_charger_pull_with_the_screen_off_makes_the_new_slot_dark_in_the_same_tick() {
    // No lighting saved: the swap's bookkeeping, then the screen's look
    // (tickets 245, 246).
    let f = lit_fixture();
    let (sender, receiver) = std::sync::mpsc::channel();
    f.shell.listen_for_power(receiver);
    screen(&f, false);
    // The battery slot holds its own level, 40 %.
    f.shell
        .app()
        .machine_mut()
        .controller
        .inner
        .brightness_level = Some(102);
    forget_sent(&f);
    super::FAKE_ON_BATTERY.set(true);
    sender.send(PowerEvent::OnBattery).unwrap();

    f.shell.tick();

    assert_eq!(sent(&f), level(0));
}

#[test]
fn a_failing_put_back_after_a_charger_change_keeps_its_spacing_and_cap() {
    // No lighting saved: the charger change leaves the keys following
    // nothing, which once gave the tries again at each look (ticket 285).
    // The AC slot is left dark with its level kept, then lit on battery.
    let f = lit_fixture();
    let start = Instant::now();
    screen(&f, false);
    super::FAKE_ON_BATTERY.set(true);
    power(&f, &[PowerEvent::OnBattery]);
    screen(&f, true);
    f.shell.app().machine_mut().controller.inner.fails = true;
    let runs = || f.shell.app().machine().controller.inner.runs;

    // The plug, then the next look: the AC slot's kept level goes back,
    // and fails.
    super::FAKE_ON_BATTERY.set(false);
    power(&f, &[PowerEvent::OnAc]);
    let plugged = runs();
    f.shell.follow_own_power();
    assert_eq!(runs(), plugged + 1, "the first try");
    f.shell.follow_own_power();
    f.shell.follow_own_power();
    assert_eq!(runs(), plugged + 1, "no try before the spacing");

    tick(&f, start, 11);
    tick(&f, start, 22);
    let capped = runs();
    tick(&f, start, 33);
    tick(&f, start, 60);
    f.shell.follow_own_power();
    assert_eq!(runs(), capped, "no try after 3");
}

#[test]
fn no_guess_at_the_other_slot_while_razer_software_runs() {
    // No lighting saved: dark before Razer's software came, then a pull.
    // A 0 read at the screen on may be its own lighting.
    let f = lit_fixture();
    screen(&f, false);
    razer_runs(&f);
    super::FAKE_ON_BATTERY.set(true);
    power(&f, &[PowerEvent::OnBattery]);
    f.shell
        .app()
        .machine_mut()
        .controller
        .inner
        .brightness_level = Some(0);
    forget_sent(&f);

    screen(&f, true);

    assert_eq!(sent(&f), []);
}

#[test]
fn no_guess_at_the_other_slot_at_a_quit_while_razer_software_runs() {
    let f = lit_fixture();
    screen(&f, false);
    razer_runs(&f);
    super::FAKE_ON_BATTERY.set(true);
    power(&f, &[PowerEvent::OnBattery]);
    f.shell
        .app()
        .machine_mut()
        .controller
        .inner
        .brightness_level = Some(0);
    forget_sent(&f);

    f.shell.quit();

    let levels = level(100);
    assert!(!sent(&f).iter().any(|command| levels.contains(command)));
}

/// Fails each put-back try, at 0, 11 and 22 s: the tries run out.
fn put_back_given_up(f: &Fixture, start: Instant) {
    screen(f, false);
    f.shell.app().machine_mut().controller.inner.fails = true;
    screen(f, true);
    tick(f, start, 11);
    tick(f, start, 22);
    f.shell.app().machine_mut().controller.inner.fails = false;
    forget_sent(f);
}

#[test]
fn the_quit_lights_keys_whose_put_back_was_given_up() {
    let f = lit_fixture();
    put_back_given_up(&f, Instant::now());

    f.shell.quit();

    assert!(lit_again(&f), "{:?}", sent(&f));
}

#[test]
fn a_put_back_refused_by_a_closed_controller_uses_no_try() {
    let f = lit_fixture();
    let start = Instant::now();
    screen(&f, false);
    forget_sent(&f);
    // Refused before it was sent, as a closed or held controller does.
    f.shell
        .app()
        .machine_mut()
        .controller
        .inner
        .not_connected_runs = 4;
    screen(&f, true);
    tick(&f, start, 11);
    tick(&f, start, 22);
    tick(&f, start, 33);
    tick(&f, start, 44);

    assert!(lit_again(&f), "{:?}", sent(&f));
}
