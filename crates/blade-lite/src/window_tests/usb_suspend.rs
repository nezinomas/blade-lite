//! Windows' USB suspend on for the Razer device (ticket 220): a line above
//! the groups, text only. It stops nothing and opens no window.

use std::time::{Duration, Instant, SystemTime};

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::Button;
use razer_proto::BLADE_18;
use slint::ComponentHandle;

use super::{FAKE_USB_SUSPEND_ON, Fixture, click, fixture, fixture_with_programs};

const SYNAPSE: &str = "Razer Synapse Service";
const SET_FAN_TARGET: u16 = 0x0D01;

fn shown(f: &Fixture) -> String {
    f.shell.refresh(None);
    f.shell.window().get_error().to_string()
}

fn says_suspend(error: &str) -> bool {
    error.contains("USB suspend")
}

fn manual_fan(rpm: u16) -> Config {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(rpm);
    config
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

/// The fan speeds sent since `before` commands, in RPM.
fn fan_speeds_since(f: &Fixture, before: usize) -> Vec<u16> {
    f.shell.app().machine().controller.inner.sent[before..]
        .iter()
        .filter(|command| command.id == SET_FAN_TARGET)
        .map(|command| u16::from(command.args[2]) * 100)
        .collect()
}

#[test]
fn suspend_on_shows_the_line_with_the_fix() {
    let f = fixture(Config::default());
    FAKE_USB_SUSPEND_ON.set(true);

    let error = shown(&f);

    assert!(says_suspend(&error), "{error}");
    assert!(error.contains("lights may go off or blink"), "{error}");
    assert!(error.contains("Quit Blade Lite"), "{error}");
    assert!(error.contains(r"tools\Set-RazerUsbSuspend.ps1"), "{error}");
    assert!(error.contains("as admin"), "{error}");
}

/// Off, an odd read and a read not ended yet all leave the flag unset.
#[test]
fn suspend_not_known_on_shows_nothing() {
    let f = fixture(Config::default());

    assert_eq!(shown(&f), "");
}

#[test]
fn a_read_that_ends_after_the_window_is_open_shows_at_the_next_fill() {
    let f = fixture(Config::default());
    assert_eq!(shown(&f), "");

    FAKE_USB_SUSPEND_ON.set(true);

    assert!(says_suspend(&shown(&f)));
}

#[test]
fn the_line_does_not_open_the_window() {
    let f = fixture(Config::default());
    f.shell.window().hide().unwrap();
    FAKE_USB_SUSPEND_ON.set(true);

    f.shell.tick();
    let error = shown(&f);

    assert!(says_suspend(&error), "{error}");
    assert!(!f.shell.window().window().is_visible());
}

#[test]
fn the_line_shows_beside_the_razer_warning_which_still_goes() {
    let f = fixture_with_programs(Config::default(), &[SYNAPSE]);
    FAKE_USB_SUSPEND_ON.set(true);

    let error = shown(&f);
    assert!(error.contains(SYNAPSE), "{error}");
    assert!(says_suspend(&error), "{error}");

    f.shell.programs().set(&[]);
    f.shell.look_for_razer();
    let error = shown(&f);

    assert!(!error.contains(SYNAPSE), "{error}");
    assert!(says_suspend(&error), "{error}");
}

#[test]
fn heat_reads_and_the_fan_guard_go_on_while_the_line_shows() {
    let f = fixture(manual_fan(1600));
    FAKE_USB_SUSPEND_ON.set(true);
    assert!(says_suspend(&shown(&f)));
    // A hot CPU, read from the controller in the background.
    f.shell.app().machine_mut().controller.inner.cpu_celsius = Some(96);
    let before = sent_count(&f);

    let t0 = Instant::now();
    for second in 0..4 {
        f.shell
            .tick_at(t0 + Duration::from_secs(second), SystemTime::now());
    }

    let top = BLADE_18.custom.fan_rpm.max;
    assert!(
        fan_speeds_since(&f, before).contains(&top),
        "{:?}",
        fan_speeds_since(&f, before)
    );
}

#[test]
fn the_fan_targets_go_once_more_while_the_line_shows() {
    let mut config = manual_fan(3600);
    config.preset = Preset::Balanced;
    let f = fixture(config);
    FAKE_USB_SUSPEND_ON.set(true);
    assert!(says_suspend(&shown(&f)));
    click(&f.shell.window(), Button, "Custom");
    let before = sent_count(&f);
    let t0 = Instant::now();

    f.shell.tick_at(t0, SystemTime::now());
    f.shell
        .tick_at(t0 + Duration::from_secs(2), SystemTime::now());

    assert_eq!(fan_speeds_since(&f, before), [3600, 3600]);
}
