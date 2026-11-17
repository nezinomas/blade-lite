//! The keys go dark while Windows has the screen off, as the idle timeout
//! turns it off with no sleep notice (ticket 241).

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

#[test]
fn a_failed_put_back_is_tried_again_at_the_next_poll() {
    let f = lit_fixture();
    screen(&f, false);
    f.shell.app().machine_mut().controller.inner.fails = true;
    screen(&f, true);
    f.shell.app().machine_mut().controller.inner.fails = false;
    forget_sent(&f);

    f.shell.follow_own_power();

    assert_eq!(sent(&f), level(100));
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
