//! Ticket 298: a preset send that leaves a manual Custom fan goes in two
//! steps, Custom with the fans on auto and the lowest boosts, then the
//! preset, as the quit does. Balanced alone left the fans at 1600 RPM where
//! the two steps reach 0 (ticket 291, `docs/NOTES.md`).

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use razer_proto::{BLADE_18, Command, CpuBoost, Custom, FanMode, GpuBoost, PerfMode};

use super::{Fixture, click, fixture};

fn sent_since(f: &Fixture, before: usize) -> Vec<Command> {
    f.shell.app().machine().controller.inner.sent[before..].to_vec()
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

/// Custom with the fans on auto and the lowest boosts.
fn auto_fans() -> Vec<Command> {
    Custom {
        cpu: CpuBoost::new(0, &BLADE_18).unwrap(),
        gpu: GpuBoost::new(0, &BLADE_18).unwrap(),
        fan: FanMode::Auto,
    }
    .commands(&BLADE_18)
}

/// `auto_fans`, then `preset`.
fn two_steps(preset: PerfMode) -> Vec<Command> {
    let mut sent = auto_fans();
    sent.extend(preset.commands(&BLADE_18).unwrap());
    sent
}

fn manual_custom() -> Config {
    let mut config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    config.custom.fan_rpm = Some(3000);
    config
}

#[test]
fn a_click_from_a_manual_fan_to_balanced_sends_the_fans_on_auto_first() {
    let f = fixture(manual_custom());
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Balanced");

    assert_eq!(sent_since(&f, before), two_steps(PerfMode::Balanced));
}

#[test]
fn a_click_from_custom_on_auto_sends_the_preset_alone() {
    let f = fixture(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Silent");

    assert_eq!(
        sent_since(&f, before),
        PerfMode::Silent.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn a_click_between_presets_sends_the_preset_alone() {
    let f = fixture(Config::default());
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Performance");
    click(f.shell.window(), Button, "Silent");

    let mut sent = PerfMode::Performance.commands(&BLADE_18).unwrap();
    sent.extend(PerfMode::Silent.commands(&BLADE_18).unwrap());
    assert_eq!(sent_since(&f, before), sent);
}

#[test]
fn a_charger_pull_from_a_manual_fan_sends_the_fans_on_auto_first() {
    let f = fixture(manual_custom());
    let before = sent_count(&f);

    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));

    let sent = sent_since(&f, before);
    let steps = two_steps(PerfMode::Balanced);
    assert_eq!(sent[..steps.len()], steps[..], "{sent:?}");
}
