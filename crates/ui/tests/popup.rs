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
    fn the_limits_are_100_for_the_cpu_89_for_the_gpu_and_the_hold_3_minutes() {
        assert_eq!(Sensor::Cpu.limit(), 100);
        assert_eq!(Sensor::Gpu.limit(), 89);
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
    fn a_gap_in_a_hot_spell_does_not_warn_again() {
        // Such as failed reads now and then: only cooling down (or a wake,
        // in the shell) starts a new spell.
        let (mut watch, mut clock) = (HeatWatch::default(), Instant::now());
        run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 2);

        clock += Duration::from_secs(600);
        let warnings = run(&mut watch, &mut clock, Some(101), HEAT_HOLD * 2);

        assert_eq!(warnings, NO_WARNINGS);
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
    fn the_gpu_warns_at_89_and_not_at_88() {
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

        assert_eq!(hot_for_the_hold(89), ["GPU 89°"]);
        assert_eq!(hot_for_the_hold(88), NO_WARNINGS);
    }
}

/// Ticket 272 (user pick A): a CPU not read for the fan guard's 30 s warns
/// once, until a reading comes.
mod cpu_not_read {
    use std::time::{Duration, Instant};

    use ui::{CPU_READ_AGAIN_HOLD, HeatWatch};

    #[test]
    fn a_cpu_not_read_warns_once_a_spell() {
        let (mut watch, now) = (HeatWatch::default(), Instant::now());

        assert!(!watch.note_cpu_unread(false, true, now));
        assert!(watch.note_cpu_unread(true, true, now));
        assert!(!watch.note_cpu_unread(true, true, now), "once a spell");
    }

    #[test]
    fn a_spell_ends_only_after_the_cpu_is_read_again_for_a_while() {
        let (mut watch, now) = (HeatWatch::default(), Instant::now());
        assert!(watch.note_cpu_unread(true, true, now));

        // One good read between failing spells: no new warning.
        assert!(!watch.note_cpu_unread(false, true, now));
        assert!(!watch.note_cpu_unread(true, true, now + Duration::from_secs(32)));

        let read_again = now + Duration::from_secs(40);
        assert!(!watch.note_cpu_unread(false, true, read_again));
        assert!(!watch.note_cpu_unread(false, true, read_again + CPU_READ_AGAIN_HOLD));
        assert!(watch.note_cpu_unread(true, true, read_again + CPU_READ_AGAIN_HOLD * 2));
    }

    #[test]
    fn a_warning_that_cannot_show_waits() {
        let (mut watch, now) = (HeatWatch::default(), Instant::now());

        assert!(!watch.note_cpu_unread(true, false, now));
        assert!(watch.note_cpu_unread(true, true, now), "shown later");
    }

    #[test]
    fn a_wake_keeps_a_warned_spell() {
        let (mut watch, now) = (HeatWatch::default(), Instant::now());
        assert!(watch.note_cpu_unread(true, true, now));

        let mut woke = watch.after_wake();

        assert!(!woke.note_cpu_unread(true, true, now));
    }
}
