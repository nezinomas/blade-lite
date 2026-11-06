//! The Performance group's view model.

use blade_core::{CustomSettings, Preset};
use razer_proto::BLADE_18;
use ui::{PerformanceCommand, PerformanceState, PerformanceView, Slider};

fn state(preset: Preset) -> PerformanceState<'static> {
    PerformanceState {
        model: &BLADE_18,
        preset,
        on_battery: false,
        custom: CustomSettings {
            cpu_boost: 2,
            gpu_boost: 1,
            fan_rpm: None,
        },
    }
}

fn custom() -> PerformanceView {
    PerformanceView::new(&state(Preset::Custom))
}

fn slider<'a>(view: &'a PerformanceView, label: &str) -> &'a Slider {
    view.sliders
        .iter()
        .find(|slider| slider.label == label)
        .unwrap_or_else(|| panic!("no {label} slider"))
}

#[test]
fn the_presets_are_the_models_plus_custom_with_the_current_one_selected() {
    let view = PerformanceView::new(&state(Preset::Balanced));

    let presets: Vec<(&str, bool)> = view
        .presets
        .iter()
        .map(|option| (option.label, option.selected))
        .collect();
    assert_eq!(
        presets,
        [
            ("Silent", false),
            ("Balanced", true),
            ("Performance", false),
            ("Custom", false),
        ]
    );
}

#[test]
fn picking_another_preset_sets_it() {
    let view = PerformanceView::new(&state(Preset::Balanced));

    assert_eq!(
        view.pick_preset(3),
        Some(PerformanceCommand::SetPreset(Preset::Custom))
    );
    assert_eq!(view.pick_preset(1), None);
    assert_eq!(view.pick_preset(4), None);
}

fn on_battery(preset: Preset) -> PerformanceView {
    PerformanceView::new(&PerformanceState {
        on_battery: true,
        ..state(preset)
    })
}

#[test]
fn on_ac_every_preset_can_be_picked() {
    let view = PerformanceView::new(&state(Preset::Balanced));

    assert!(view.presets.iter().all(|option| option.enabled));
}

#[test]
fn on_battery_only_silent_and_balanced_can_be_picked() {
    let view = on_battery(Preset::Balanced);

    let enabled: Vec<bool> = view.presets.iter().map(|option| option.enabled).collect();
    assert_eq!(enabled, [true, true, false, false]);
    assert_eq!(
        view.pick_preset(0),
        Some(PerformanceCommand::SetPreset(Preset::Silent))
    );
    assert_eq!(view.pick_preset(2), None);
    assert_eq!(view.pick_preset(3), None);
}

#[test]
fn on_battery_a_saved_performance_shows_as_balanced_without_sliders() {
    for preset in [Preset::Performance, Preset::Custom] {
        let view = on_battery(preset);

        let selected: Vec<&str> = view
            .presets
            .iter()
            .filter(|option| option.selected)
            .map(|option| option.label)
            .collect();
        assert_eq!(selected, ["Balanced"], "{preset:?}");
        assert!(view.sliders.is_empty(), "{preset:?}");
    }
}

#[test]
fn custom_shows_its_three_sliders() {
    let labels: Vec<&str> = custom().sliders.iter().map(|s| s.label).collect();

    assert_eq!(labels, ["CPU boost", "GPU boost", "Fan"]);
}

#[test]
fn the_other_presets_hide_the_sliders() {
    for preset in [Preset::Silent, Preset::Balanced, Preset::Performance] {
        assert!(
            PerformanceView::new(&state(preset)).sliders.is_empty(),
            "{preset:?}"
        );
    }
}

#[test]
fn the_boost_sliders_run_from_0_to_the_models_highest_level() {
    let view = custom();

    let cpu = slider(&view, "CPU boost");
    assert_eq!((cpu.stops, cpu.position, cpu.text.as_str()), (4, 2, "2"));
    let gpu = slider(&view, "GPU boost");
    assert_eq!((gpu.stops, gpu.position, gpu.text.as_str()), (4, 1, "1"));
}

#[test]
fn the_fan_slider_starts_at_auto_then_runs_over_the_models_range() {
    let view = custom();

    // Auto, then 1600 to 4500 rpm in steps of 100.
    let fan = slider(&view, "Fan");
    assert_eq!(
        (fan.stops, fan.position, fan.text.as_str()),
        (31, 0, "Auto")
    );
}

#[test]
fn a_manual_fan_speed_shows_its_rpm() {
    let mut manual = state(Preset::Custom);
    manual.custom.fan_rpm = Some(3100);

    let view = PerformanceView::new(&manual);

    let fan = slider(&view, "Fan");
    assert_eq!((fan.position, fan.text.as_str()), (16, "3100 rpm"));
}

#[test]
fn moving_a_boost_slider_sets_that_boost() {
    let view = custom();

    assert_eq!(
        view.move_slider(0, 3),
        Some(PerformanceCommand::SetCpuBoost(3))
    );
    assert_eq!(
        view.move_slider(1, 0),
        Some(PerformanceCommand::SetGpuBoost(0))
    );
}

#[test]
fn moving_the_fan_slider_sets_a_speed_or_auto() {
    let view = custom();

    assert_eq!(
        view.move_slider(2, 1),
        Some(PerformanceCommand::SetFanRpm(Some(1600)))
    );
    assert_eq!(
        view.move_slider(2, 30),
        Some(PerformanceCommand::SetFanRpm(Some(4500)))
    );

    let mut manual = state(Preset::Custom);
    manual.custom.fan_rpm = Some(3100);
    assert_eq!(
        PerformanceView::new(&manual).move_slider(2, 0),
        Some(PerformanceCommand::SetFanRpm(None))
    );
}

#[test]
fn a_move_that_changes_nothing_or_goes_off_the_end_does_nothing() {
    let view = custom();

    assert_eq!(view.move_slider(0, 2), None);
    assert_eq!(view.move_slider(0, 4), None);
    assert_eq!(view.move_slider(2, 31), None);
    assert_eq!(view.move_slider(3, 0), None);
}

#[test]
fn with_the_sliders_hidden_a_move_does_nothing() {
    let view = PerformanceView::new(&state(Preset::Balanced));

    assert_eq!(view.move_slider(0, 3), None);
}

#[test]
fn a_saved_speed_outside_the_range_sits_at_the_nearest_end() {
    let mut low = state(Preset::Custom);
    low.custom.fan_rpm = Some(900);
    let mut high = state(Preset::Custom);
    high.custom.fan_rpm = Some(9900);

    assert_eq!(slider(&PerformanceView::new(&low), "Fan").position, 1);
    assert_eq!(slider(&PerformanceView::new(&high), "Fan").position, 30);
}
