//! Only Silent and Balanced run on battery.

use blade_core::Preset;

const ALL: [Preset; 4] = [
    Preset::Silent,
    Preset::Balanced,
    Preset::Performance,
    Preset::Custom,
];

#[test]
fn only_silent_and_balanced_are_offered_on_battery() {
    let offered: Vec<Preset> = ALL
        .into_iter()
        .filter(|preset| preset.offered_on_battery())
        .collect();

    assert_eq!(offered, [Preset::Silent, Preset::Balanced]);
}

#[test]
fn on_ac_the_saved_preset_runs() {
    for preset in ALL {
        assert_eq!(preset.running(false), preset, "{preset:?}");
    }
}

#[test]
fn on_battery_performance_and_custom_run_as_balanced() {
    let running: Vec<Preset> = ALL.into_iter().map(|preset| preset.running(true)).collect();

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
