//! The Display group's view model.

use blade_core::{NativeModeState, RefreshRate};
use razer_proto::{BLADE_18, NativeMode};
use ui::{DisplayCommand, DisplayState, DisplayView};

const UHD: NativeMode = BLADE_18.native_modes[0];
const FHD: NativeMode = BLADE_18.native_modes[1];

fn rates(hz: &[u32]) -> Vec<RefreshRate> {
    hz.iter().copied().map(RefreshRate).collect()
}

/// The panel at 3840 × 2400 and 240 Hz, plugged in.
fn state() -> DisplayState<'static> {
    DisplayState {
        model: &BLADE_18,
        offered: rates(&[60, 120, 240]),
        current: RefreshRate(240),
        native: NativeModeState::Active(UHD),
        sixty_hz_on_battery: true,
        on_battery: false,
    }
}

fn labels(view: &DisplayView) -> Vec<&str> {
    view.rates.iter().map(|segment| segment.label.as_str()).collect()
}

#[test]
fn the_segments_match_the_offered_rates() {
    let view = DisplayView::new(&state());

    assert_eq!(labels(&view), ["60 Hz", "120 Hz", "240 Hz"]);
    let selected: Vec<bool> = view.rates.iter().map(|segment| segment.selected).collect();
    assert_eq!(selected, [false, false, true]);
}

#[test]
fn hz_440_appears_only_in_1920_x_1200_mode() {
    let fhd = DisplayState {
        offered: rates(&[60, 120, 240, 440]),
        current: RefreshRate(440),
        native: NativeModeState::Active(FHD),
        ..state()
    };

    assert!(!labels(&DisplayView::new(&state())).contains(&"440 Hz"));
    assert!(labels(&DisplayView::new(&fhd)).contains(&"440 Hz"));
}

#[test]
fn picking_a_rate_sets_it() {
    let view = DisplayView::new(&state());

    assert_eq!(
        view.pick_rate(1),
        Some(DisplayCommand::SetRefreshRate(RefreshRate(120)))
    );
}

#[test]
fn picking_the_current_rate_or_no_rate_does_nothing() {
    let view = DisplayView::new(&state());

    assert_eq!(view.pick_rate(2), None);
    assert_eq!(view.pick_rate(3), None);
}

#[test]
fn on_battery_with_the_option_on_the_rates_are_locked() {
    let view = DisplayView::new(&DisplayState {
        on_battery: true,
        current: RefreshRate(60),
        ..state()
    });

    assert!(!view.rates_enabled);
    assert_eq!(view.rate_note, "Locked to 60 Hz while on battery");
    assert_eq!(view.pick_rate(1), None);
}

#[test]
fn on_battery_with_the_option_off_the_rates_stay_open() {
    let view = DisplayView::new(&DisplayState {
        on_battery: true,
        sixty_hz_on_battery: false,
        ..state()
    });

    assert!(view.rates_enabled);
    assert_eq!(view.rate_note, "");
}

#[test]
fn the_battery_switch_shows_and_flips_the_option() {
    let view = DisplayView::new(&state());

    assert!(view.sixty_hz_on_battery);
    assert_eq!(
        view.toggle_sixty_hz_on_battery(),
        DisplayCommand::SetSixtyHzOnBattery(false)
    );
}

#[test]
fn the_native_modes_come_from_the_model_with_the_chosen_one_selected() {
    let view = DisplayView::new(&state());

    let options: Vec<(&str, bool)> = view
        .native_modes
        .iter()
        .map(|option| (option.label.as_str(), option.selected))
        .collect();
    assert_eq!(
        options,
        [
            ("3840 × 2400, up to 240 Hz", true),
            ("1920 × 1200, up to 440 Hz", false),
        ]
    );
}

#[test]
fn picking_another_native_mode_chooses_it() {
    let view = DisplayView::new(&state());

    assert_eq!(
        view.pick_native_mode(1),
        Some(DisplayCommand::SetNativeMode(FHD))
    );
    assert_eq!(view.pick_native_mode(0), None);
    assert_eq!(view.pick_native_mode(2), None);
}

#[test]
fn an_active_native_mode_shows_no_infobar() {
    let view = DisplayView::new(&state());

    assert!(!view.restart_infobar);
    assert_eq!(view.native_note, "3840 × 2400");
}

#[test]
fn changing_native_mode_shows_the_restart_infobar() {
    let view = DisplayView::new(&DisplayState {
        native: NativeModeState::AppliesAfterRestart(FHD),
        ..state()
    });

    assert!(view.restart_infobar);
    assert_eq!(view.native_note, "1920 × 1200 after restart");
}
