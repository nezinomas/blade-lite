//! The model's battery presets run on battery. Custom never does.

use blade_core::Preset;
use razer_proto::{BLADE_18, ModelProfile, PerfMode};

const ALL: [Preset; 4] = [
    Preset::Silent,
    Preset::Balanced,
    Preset::Performance,
    Preset::Custom,
];

/// A model that runs only Balanced on battery.
const BALANCED_ONLY: ModelProfile = ModelProfile {
    battery_presets: &[PerfMode::Balanced],
    ..BLADE_18
};

#[test]
fn only_silent_and_balanced_are_offered_on_battery() {
    let offered: Vec<Preset> = ALL
        .into_iter()
        .filter(|preset| preset.offered_on_battery(&BLADE_18))
        .collect();

    assert_eq!(offered, [Preset::Silent, Preset::Balanced]);
}

#[test]
fn the_presets_offered_on_battery_come_from_the_model() {
    let offered: Vec<Preset> = ALL
        .into_iter()
        .filter(|preset| preset.offered_on_battery(&BALANCED_ONLY))
        .collect();

    assert_eq!(offered, [Preset::Balanced]);
    assert_eq!(
        Preset::Silent.running(true, &BALANCED_ONLY),
        Preset::Balanced
    );
}

#[test]
fn on_ac_the_saved_preset_runs() {
    for preset in ALL {
        assert_eq!(preset.running(false, &BLADE_18), preset, "{preset:?}");
    }
}

#[test]
fn on_battery_performance_and_custom_run_as_balanced() {
    let running: Vec<Preset> = ALL
        .into_iter()
        .map(|preset| preset.running(true, &BLADE_18))
        .collect();

    assert_eq!(
        running,
        [
            Preset::Silent,
            Preset::Balanced,
            Preset::Balanced,
            Preset::Balanced
        ]
    );
}
