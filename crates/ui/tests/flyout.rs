//! The tray flyout's view model.

use blade_core::{CustomSettings, NativeModeState, Preset, RefreshRate};
use razer_proto::{BLADE_18, Brightness, Rgb};
use ui::{
    DisplayState, DisplayView, FlyoutView, KeyboardState, KeyboardView, PerformanceCommand,
    PerformanceState, PerformanceView,
};

fn performance(preset: Preset) -> PerformanceView {
    PerformanceView::new(&PerformanceState {
        model: &BLADE_18,
        preset,
        on_battery: false,
        custom: CustomSettings::default(),
    })
}

fn display() -> DisplayView {
    DisplayView::new(&DisplayState {
        model: &BLADE_18,
        offered: vec![RefreshRate(60), RefreshRate(120), RefreshRate(240)],
        current: RefreshRate(120),
        native: NativeModeState::Active(BLADE_18.native_modes[0]),
        sixty_hz_on_battery: false,
        on_battery: false,
    })
}

fn keyboard() -> KeyboardView {
    KeyboardView::new(&KeyboardState {
        brightness: Brightness::new(40).unwrap(),
        colour: Rgb::new(0xFF, 0xFF, 0xFF),
        touchpad_on: false,
    })
}

fn flyout(preset: Preset) -> FlyoutView {
    FlyoutView::new(&performance(preset), &display(), &keyboard())
}

#[test]
fn it_lists_the_preset_the_rate_the_lighting_and_the_touchpad() {
    let flyout = flyout(Preset::Silent);

    assert_eq!(flyout.presets, performance(Preset::Silent).presets);
    assert_eq!(flyout.rates, display().rates);
    assert!(flyout.rates_enabled);
    assert_eq!(flyout.brightness, 40);
    assert_eq!(flyout.brightness_text, "40%");
    assert!(!flyout.touchpad_on);
}

#[test]
fn reset_to_balanced_sets_balanced() {
    for preset in [Preset::Silent, Preset::Performance, Preset::Custom] {
        let flyout = flyout(preset);

        assert!(flyout.reset_enabled, "{preset:?}");
        assert_eq!(
            flyout.reset_to_balanced(),
            Some(PerformanceCommand::SetPreset(Preset::Balanced)),
            "{preset:?}"
        );
    }
}

#[test]
fn reset_does_nothing_when_already_balanced() {
    let flyout = flyout(Preset::Balanced);

    assert!(!flyout.reset_enabled);
    assert_eq!(flyout.reset_to_balanced(), None);
}

#[test]
fn rates_locked_on_battery_stay_locked_in_the_flyout() {
    let locked = DisplayView::new(&DisplayState {
        sixty_hz_on_battery: true,
        on_battery: true,
        ..DisplayState {
            model: &BLADE_18,
            offered: vec![RefreshRate(60)],
            current: RefreshRate(60),
            native: NativeModeState::Active(BLADE_18.native_modes[0]),
            sixty_hz_on_battery: false,
            on_battery: false,
        }
    });

    let flyout = FlyoutView::new(&performance(Preset::Balanced), &locked, &keyboard());

    assert!(!flyout.rates_enabled);
}
