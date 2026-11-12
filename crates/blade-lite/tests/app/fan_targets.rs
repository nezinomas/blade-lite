//! A manual Custom's fan targets due to go once more (ticket 173).

use blade_core::{Config, CustomSettings, Preset};
use ui::PerformanceCommand;

use crate::app_with;

fn manual_fan_saved() -> Config {
    Config {
        custom: CustomSettings {
            fan_rpm: Some(3600),
            ..CustomSettings::default()
        },
        ..Config::default()
    }
}

#[test]
fn a_send_cut_partway_drops_the_fan_targets_due() {
    let (mut app, _dir, _) = app_with(manual_fan_saved());
    app.take_on_battery(Some(false));
    app.on_performance(PerformanceCommand::SetPreset(Preset::Custom))
        .unwrap();
    assert!(app.fan_targets_due().is_some());
    app.machine_mut().controller.inner.fails = true;

    let _ = app.on_performance(PerformanceCommand::SetPreset(Preset::Balanced));

    assert_eq!(app.fan_targets_due(), None);
}

#[test]
fn a_change_refused_before_any_send_keeps_the_fan_targets_due() {
    let (mut app, _dir, _) = app_with(manual_fan_saved());
    app.take_on_battery(Some(false));
    app.on_performance(PerformanceCommand::SetPreset(Preset::Custom))
        .unwrap();
    let due = app.fan_targets_due();
    let runs = app.machine().controller.inner.runs;

    // Out of the model's range: refused before anything is sent.
    assert!(
        app.on_performance(PerformanceCommand::SetFanRpm(Some(9000)))
            .is_err()
    );

    assert_eq!(app.machine().controller.inner.runs, runs);
    assert_eq!(app.fan_targets_due(), due);
}
