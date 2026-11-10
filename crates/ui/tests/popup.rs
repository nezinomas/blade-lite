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

    use ui::{HEAT_HOLD, HeatWatch, Sensor};

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
    fn the_limits_are_100_for_the_cpu_87_for_the_gpu_and_the_hold_3_minutes() {
        assert_eq!(Sensor::Cpu.limit(), 100);
        assert_eq!(Sensor::Gpu.limit(), 87);
        assert_eq!(HEAT_HOLD, Duration::from_secs(180));
    }

    #[test]
    fn a_short_peak_shows_nothing() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let warnings = run(&mut watch, &mut clock, Some(104), JUST_UNDER);

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn a_hot_spell_warns_once_with_the_sensor_and_reading() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let warnings = run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 3);

        assert_eq!(warnings, ["CPU 101°"]);
    }

    #[test]
    fn a_dip_below_the_limit_starts_the_hold_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let mut warnings = run(&mut watch, &mut clock, Some(101), JUST_UNDER);
        warnings.extend(run(&mut watch, &mut clock, Some(99), SECOND));
        warnings.extend(run(&mut watch, &mut clock, Some(101), JUST_UNDER));

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn a_new_spell_warns_again_only_after_it_cooled_down() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 2);
        let mut warnings = run(&mut watch, &mut clock, Some(96), SECOND);
        warnings.extend(run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 2));
        assert_eq!(warnings, NO_WARNINGS, "96 °C does not end the spell");

        run(&mut watch, &mut clock, Some(94), SECOND);
        let warnings = run(&mut watch, &mut clock, Some(102), HEAT_HOLD * 2);
        assert_eq!(warnings, ["CPU 102°"]);
    }

    #[test]
    fn a_reading_that_cant_be_read_now_and_then_does_not_stop_the_warning() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let mut warnings = run(&mut watch, &mut clock, Some(101), HEAT_HOLD / 2);
        warnings.extend(run(&mut watch, &mut clock, None, SECOND));
        warnings.extend(run(&mut watch, &mut clock, Some(101), HEAT_HOLD));

        assert_eq!(warnings, ["CPU 101°"]);
    }

    #[test]
    fn readings_that_cant_be_read_for_long_start_the_hold_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        let mut warnings = run(&mut watch, &mut clock, Some(101), JUST_UNDER);
        warnings.extend(run(&mut watch, &mut clock, None, Duration::from_secs(40)));
        warnings.extend(run(&mut watch, &mut clock, Some(101), JUST_UNDER));

        assert_eq!(warnings, NO_WARNINGS);
    }

    #[test]
    fn after_a_gap_a_new_spell_can_warn_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());
        run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 2);

        clock += Duration::from_secs(600);
        let warnings = run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 2);

        assert_eq!(warnings, ["CPU 101°"]);
    }

    #[test]
    fn a_gap_in_the_readings_starts_the_hold_again() {
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());

        assert_eq!(watch.note(Sensor::Cpu, Some(101), clock), None);
        clock += HEAT_HOLD * 2;

        assert_eq!(watch.note(Sensor::Cpu, Some(101), clock), None);
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

        watch.note(Sensor::Cpu, Some(101), start);
        watch.note(Sensor::Gpu, Some(50), start);
        let mut warnings = Vec::new();
        for step in 1..=200 {
            let now = start + SECOND * step;
            warnings.extend(watch.note(Sensor::Cpu, Some(50), now));
            warnings.extend(watch.note(Sensor::Gpu, Some(101), now));
        }

        assert_eq!(warnings, ["GPU 101°"]);
    }

    #[test]
    fn the_gpu_warns_at_87_and_not_at_86() {
        let start = Instant::now();
        let hot_for_the_hold = |celsius| {
            let mut watch = HeatWatch::default();
            (0..=HEAT_HOLD.as_secs())
                .filter_map(|second| {
                    let now = start + Duration::from_secs(second);
                    watch.note(Sensor::Gpu, Some(celsius), now)
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(hot_for_the_hold(87), ["GPU 87°"]);
        assert_eq!(hot_for_the_hold(86), NO_WARNINGS);
    }
}
