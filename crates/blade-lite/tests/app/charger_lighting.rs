//! The controller keeps one keyboard level per power source and swaps them
//! itself at a charger change (ticket 243), so the app sends the saved
//! lighting again then (ticket 244), spaced (ticket 246). With none saved
//! it follows only the app's own dark, per slot (tickets 245, 246).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use blade_core::{Config, Lighting, Preset};
use platform::PowerEvent;
use razer_proto::{BLADE_18, Brightness, Command, PerfMode, static_colour};

use crate::{TestApp, WHITE, app, app_with, lighting_commands, sent};

fn lit_app() -> (TestApp, tempfile::TempDir, std::path::PathBuf) {
    app_with(Config {
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    })
}

fn level(percent: u8) -> Vec<Command> {
    Brightness::new(percent).unwrap().commands(&BLADE_18)
}

fn lit_at(app: &mut TestApp, level: u8) {
    app.machine_mut().controller.inner.brightness_level = Some(level);
}

fn forget_sent(app: &mut TestApp) {
    app.machine_mut().controller.inner.sent.clear();
}

/// A power event, then the shell's look that sends a due lighting, a
/// spacing after the last.
fn power(app: &mut TestApp, event: PowerEvent) {
    app.on_power(event).unwrap();
    app.send_source_lighting(later());
}

/// A look's time, a spacing after every earlier one.
fn later() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    static LOOKS: AtomicU32 = AtomicU32::new(1);
    let looks = LOOKS.fetch_add(1, Ordering::Relaxed);
    *START.get_or_init(Instant::now) + looks * blade_lite::SOURCE_LIGHTING_SPACING
}

#[test]
fn a_charger_pull_sends_the_saved_lighting() {
    let (mut app, _dir, _) = lit_app();

    power(&mut app, PowerEvent::OnBattery);

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn the_charger_back_sends_it_again() {
    let (mut app, _dir, _) = lit_app();
    power(&mut app, PowerEvent::OnBattery);
    forget_sent(&mut app);

    power(&mut app, PowerEvent::OnAc);

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn an_event_for_the_same_source_sends_nothing() {
    let (mut app, _dir, _) = lit_app();

    power(&mut app, PowerEvent::OnAc);

    assert_eq!(sent(&app), []);
}

#[test]
fn a_source_read_after_an_unknown_one_sends_nothing() {
    // Which slot the lighting sent after the unknown read went to is not
    // known; nothing is sent, the safe side.
    let (mut app, _dir, _) = lit_app();
    app.take_on_battery(None);

    power(&mut app, PowerEvent::OnAc);

    assert_eq!(sent(&app), []);
}

#[test]
fn no_saved_lighting_sends_nothing() {
    let (mut app, _dir, _) = app();

    power(&mut app, PowerEvent::OnBattery);

    assert_eq!(sent(&app), []);
}

#[test]
fn dark_keys_stay_dark_with_the_lighting_set_and_the_saved_level_comes_back() {
    let (mut app, _dir, _) = lit_app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    power(&mut app, PowerEvent::OnBattery);

    let mut expected = level(0);
    expected.push(
        BLADE_18
            .vapor_light
            .unwrap()
            .command(Brightness::new(30).unwrap(), true),
    );
    expected.extend(static_colour(WHITE, &BLADE_18));
    assert_eq!(sent(&app), expected);
    forget_sent(&mut app);
    app.follow_screen(false).unwrap();
    assert_eq!(sent(&app), level(30));
}

#[test]
fn dark_keys_with_no_saved_lighting_get_only_the_dark_level() {
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(true).unwrap();

    assert_eq!(sent(&app), level(0));
    forget_sent(&mut app);
    app.follow_screen(false).unwrap();
    assert_eq!(sent(&app), level(100));
}

#[test]
fn dark_keys_with_no_saved_lighting_put_back_the_new_slots_own_level() {
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    // The battery slot holds its own level, 40 %.
    lit_at(&mut app, 102);
    forget_sent(&mut app);

    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(40));
}

#[test]
fn many_changes_before_a_look_send_the_lighting_once() {
    let (mut app, _dir, _) = lit_app();

    app.on_power(PowerEvent::OnBattery).unwrap();
    app.on_power(PowerEvent::OnAc).unwrap();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.send_source_lighting(later());
    app.send_source_lighting(later());

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn a_forgotten_send_sends_nothing() {
    let (mut app, _dir, _) = lit_app();
    app.on_power(PowerEvent::OnBattery).unwrap();

    app.forget_source_lighting();
    app.send_source_lighting(later());

    assert_eq!(sent(&app), []);
}

#[test]
fn the_lighting_goes_after_the_battery_preset() {
    let (mut app, _dir, _) = app_with(Config {
        preset: Preset::Performance,
        lighting: Some(Lighting {
            brightness: 30,
            colour: WHITE,
            vapor_light: true,
        }),
        ..Config::default()
    });

    power(&mut app, PowerEvent::OnBattery);

    let mut expected = PerfMode::Balanced.commands(&BLADE_18).unwrap();
    expected.extend(lighting_commands(30, WHITE));
    assert_eq!(sent(&app), expected);
}

#[test]
fn a_failed_send_is_tried_again_at_the_next_look() {
    let (mut app, _dir, _) = lit_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.fails = true;
    app.send_source_lighting(later());
    app.machine_mut().controller.inner.fails = false;
    forget_sent(&mut app);

    app.send_source_lighting(later());

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn a_send_that_keeps_failing_is_given_up_after_its_tries() {
    let (mut app, _dir, _) = lit_app();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.machine_mut().controller.inner.fails = true;
    for _ in 0..blade_lite::SOURCE_LIGHTING_TRIES {
        assert!(app.source_lighting_due());
        app.send_source_lighting(later());
    }

    assert!(!app.source_lighting_due());
}

#[test]
fn a_pull_and_plug_with_the_screen_off_put_each_slots_own_level_back() {
    // The AC slot holds the app's own 0 at the plug: its level kept from
    // the screen off comes back to it, the battery's at the next pull.
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    lit_at(&mut app, 102);
    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(true).unwrap();
    lit_at(&mut app, 0);
    power(&mut app, PowerEvent::OnAc);
    app.follow_screen(true).unwrap();
    forget_sent(&mut app);

    app.follow_screen(false).unwrap();
    assert_eq!(sent(&app), level(100));
    forget_sent(&mut app);
    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(false).unwrap();
    assert_eq!(sent(&app), level(40));
}

#[test]
fn a_slot_left_dark_gets_its_level_back_at_the_next_swap_into_it() {
    // Ticket 245: screen off on AC, pull, screen on, plug.
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    lit_at(&mut app, 102);
    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(true).unwrap();
    app.follow_screen(false).unwrap();
    // The AC slot still holds the app's 0.
    lit_at(&mut app, 0);
    forget_sent(&mut app);

    power(&mut app, PowerEvent::OnAc);
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(100));
}

#[test]
fn a_slot_level_is_put_back_only_once() {
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(true).unwrap();
    lit_at(&mut app, 102);
    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(true).unwrap();
    app.follow_screen(false).unwrap();
    power(&mut app, PowerEvent::OnAc);
    app.follow_screen(false).unwrap();
    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(false).unwrap();
    forget_sent(&mut app);

    power(&mut app, PowerEvent::OnAc);
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_swap_with_the_users_own_dark_keys_and_the_screen_off_makes_the_new_slot_dark() {
    // Ticket 246 (b): the old slot's level is the user's 0, so the app
    // kept nothing; the new slot's own level lights the keys.
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 0);
    app.follow_screen(true).unwrap();
    lit_at(&mut app, 102);

    power(&mut app, PowerEvent::OnBattery);
    assert!(!app.screen_followed(true));
    app.follow_screen(true).unwrap();
    assert_eq!(sent(&app), level(0));
    forget_sent(&mut app);
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), level(40));
}

#[test]
fn a_swap_with_the_screen_on_and_nothing_kept_sends_nothing() {
    let (mut app, _dir, _) = app();
    lit_at(&mut app, 255);
    app.follow_screen(false).unwrap();

    power(&mut app, PowerEvent::OnBattery);
    app.follow_screen(false).unwrap();

    assert_eq!(sent(&app), []);
}

#[test]
fn a_second_change_inside_the_spacing_waits_for_it() {
    let (mut app, _dir, _) = lit_app();
    let start = Instant::now();
    app.on_power(PowerEvent::OnBattery).unwrap();
    app.send_source_lighting(start);
    forget_sent(&mut app);
    app.on_power(PowerEvent::OnAc).unwrap();

    let almost = blade_lite::SOURCE_LIGHTING_SPACING
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    app.send_source_lighting(start + almost);
    assert_eq!(sent(&app), []);
    assert!(app.source_lighting_due());
    app.send_source_lighting(start + blade_lite::SOURCE_LIGHTING_SPACING);

    assert_eq!(sent(&app), lighting_commands(30, WHITE));
}

#[test]
fn a_bouncing_plug_sends_once_per_spacing() {
    let (mut app, _dir, _) = lit_app();
    let start = Instant::now();
    let mut on_battery = false;
    // One swap and one look each second for a minute.
    for second in 0..60 {
        on_battery = !on_battery;
        let event = if on_battery {
            PowerEvent::OnBattery
        } else {
            PowerEvent::OnAc
        };
        app.on_power(event).unwrap();
        app.send_source_lighting(start + Duration::from_secs(second));
    }

    let sends = 60 / usize::try_from(blade_lite::SOURCE_LIGHTING_SPACING.as_secs()).unwrap();
    let one = lighting_commands(30, WHITE).len();
    assert_eq!(sent(&app).len(), sends * one);
}

#[test]
fn lit_keys_with_the_screen_off_are_looked_at_again() {
    // The screen off read failed, so the keys stayed lit: after the
    // charger change's send, the next look makes them dark.
    let (mut app, _dir, _) = lit_app();
    assert!(app.follow_screen(true).is_err());
    power(&mut app, PowerEvent::OnBattery);
    lit_at(&mut app, 77);
    forget_sent(&mut app);

    app.follow_screen(true).unwrap();

    assert_eq!(sent(&app), level(0));
}
