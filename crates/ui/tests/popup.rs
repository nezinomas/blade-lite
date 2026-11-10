//! When the power mode and touchpad popups show, and what they say.

use blade_core::Preset;
use ui::{ModeWatch, TouchpadWatch};

#[test]
fn the_mode_at_start_shows_no_popup() {
    let mut watch = ModeWatch::new(Preset::Balanced);

    assert_eq!(watch.changed(Preset::Balanced, true), None);
}

#[test]
fn a_new_mode_shows_its_name_once() {
    let mut watch = ModeWatch::new(Preset::Balanced);

    assert_eq!(watch.changed(Preset::Silent, true), Some("Silent"));
    assert_eq!(watch.changed(Preset::Silent, true), None);
}

#[test]
fn each_change_shows_the_mode_it_changed_to() {
    let mut watch = ModeWatch::new(Preset::Silent);

    assert_eq!(
        watch.changed(Preset::Performance, true),
        Some("Performance")
    );
    assert_eq!(watch.changed(Preset::Custom, true), Some("Custom"));
    assert_eq!(watch.changed(Preset::Balanced, true), Some("Balanced"));
}

#[test]
fn a_change_that_failed_shows_no_popup_now_or_later() {
    let mut watch = ModeWatch::new(Preset::Performance);

    assert_eq!(watch.changed(Preset::Balanced, false), None);
    assert_eq!(watch.changed(Preset::Balanced, true), None);
}

#[test]
fn the_touchpad_state_at_start_shows_no_popup() {
    let mut watch = TouchpadWatch::new(Some(true));

    assert_eq!(watch.changed(Some(true)), None);
}

#[test]
fn a_touchpad_change_shows_on_or_off_once() {
    let mut watch = TouchpadWatch::new(Some(true));

    assert_eq!(watch.changed(Some(false)), Some("Off"));
    assert_eq!(watch.changed(Some(false)), None);
    assert_eq!(watch.changed(Some(true)), Some("On"));
}

#[test]
fn a_touchpad_state_that_cant_be_read_shows_no_popup_now_or_after() {
    let mut watch = TouchpadWatch::new(Some(true));

    assert_eq!(watch.changed(None), None);
    assert_eq!(watch.changed(Some(false)), None);
    assert_eq!(watch.changed(Some(true)), Some("On"));
}

mod heat {
    use std::time::{Duration, Instant};

    use ui::{HEAT_HOLD, HEAT_LIMIT, HeatWatch, Sensor};

    const SECOND: Duration = Duration::from_secs(1);
    const NO_WARNINGS: [String; 0] = [];
    /// One reading short of the hold.
    const JUST_UNDER: Duration = HEAT_HOLD.saturating_sub(SECOND);

    /// Feeds `celsius` once a second for `duration`, and returns each warning.
    fn run(
        watch: &mut HeatWatch,
        clock: &mut Instant,
        celsius: Option<u32>,
        duration: Duration,
    ) -> Vec<String> {
        let end = *clock + duration;
        let mut warnings = Vec::new();
        while *clock < end {
            warnings.extend(watch.note(Sensor::Cpu, celsius, *clock));
            *clock += SECOND;
        }
        warnings
    }

    #[test]
    fn the_limit_is_95_and_the_hold_3_minutes() {
        assert_eq!(HEAT_LIMIT, 95);
        assert_eq!(HEAT_HOLD, Duration::from_secs(180));
    }

    #[test]
    fn a_short_peak_shows_nothing() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let warnings = run(&mut watch, &mut clock, Some(99), JUST_UNDER);

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn a_hot_spell_warns_once_with_the_sensor_and_reading() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let warnings = run(&mut watch, &mut clock, Some(96), HEAT_HOLD * 3);

        assert_eq!(warnings, ["CPU 96 °C"]);
    }

    #[test]
    fn a_dip_below_the_limit_starts_the_hold_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let mut warnings = run(&mut watch, &mut clock, Some(96), JUST_UNDER);
        warnings.extend(run(&mut watch, &mut clock, Some(94), SECOND));
        warnings.extend(run(&mut watch, &mut clock, Some(96), JUST_UNDER));

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn a_new_spell_warns_again_only_after_it_cooled_down() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        run(&mut watch, &mut clock, Some(96), HEAT_HOLD * 2);
        let mut warnings = run(&mut watch, &mut clock, Some(91), SECOND);
        warnings.extend(run(&mut watch, &mut clock, Some(96), HEAT_HOLD * 2));
        assert_eq!(warnings, NO_WARNINGS, "91 °C does not end the spell");

        run(&mut watch, &mut clock, Some(89), SECOND);
        let warnings = run(&mut watch, &mut clock, Some(97), HEAT_HOLD * 2);
        assert_eq!(warnings, ["CPU 97 °C"]);
    }

    #[test]
    fn a_reading_that_cant_be_read_starts_the_hold_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let mut warnings = run(&mut watch, &mut clock, Some(96), JUST_UNDER);
        warnings.extend(run(&mut watch, &mut clock, None, SECOND));
        warnings.extend(run(&mut watch, &mut clock, Some(96), JUST_UNDER));

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn a_gap_in_the_readings_starts_the_hold_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        assert_eq!(watch.note(Sensor::Cpu, Some(96), clock), None);
        clock += HEAT_HOLD * 2;

        assert_eq!(watch.note(Sensor::Cpu, Some(96), clock), None);
    }

    #[test]
    fn a_reading_out_of_range_is_not_believed() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let warnings = run(&mut watch, &mut clock, Some(200), HEAT_HOLD * 2);

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn the_cpu_and_gpu_are_watched_apart() {
        let (mut watch, start) = (HeatWatch::default(), Instant::now());

        watch.note(Sensor::Cpu, Some(96), start);
        watch.note(Sensor::Gpu, Some(50), start);
        let mut warnings = Vec::new();
        for step in 1..=200 {
            let now = start + SECOND * step;
            warnings.extend(watch.note(Sensor::Cpu, Some(50), now));
            warnings.extend(watch.note(Sensor::Gpu, Some(96), now));
        }

        assert_eq!(warnings, ["GPU 96 °C"]);
    }
}
