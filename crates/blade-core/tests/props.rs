//! Property tests: random key reports and random config text, never a
//! panic, and a config that loads is always in range (ticket 146).

use std::time::{Duration, Instant};

use blade_core::{Config, PowerKeyPresses, check};
use proptest::prelude::*;
use razer_proto::BLADE_18;

proptest! {
    #[test]
    fn any_key_reports_never_panic(
        reports in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..70), 0..40),
        gaps in prop::collection::vec(0_u64..2_000, 40),
    ) {
        let key = BLADE_18.power_key.expect("the Blade 18 has a power-mode key");
        let mut presses = PowerKeyPresses::new(key);
        let mut now = Instant::now();
        for (report, gap) in reports.iter().zip(&gaps) {
            now += Duration::from_millis(*gap);
            presses.on_report(report, now);
            presses.brightness_due(now);
            presses.on_enter(report.first().is_some_and(|b| b % 2 == 0), now);
            presses.on_mic(report.last().is_some_and(|b| b % 2 == 0), now);
        }
    }

    #[test]
    fn any_config_text_loads_in_range_or_is_refused(text in ".{0,400}") {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, &text).expect("written");

        for config in [Config::load(&path), Config::import(&path)].into_iter().flatten() {
            prop_assert!(check(&config, &BLADE_18).is_ok(), "{config:?}");
        }
    }

    #[test]
    fn config_like_text_loads_in_range_or_is_refused(
        preset in prop::sample::select(vec!["Silent", "Balanced", "Performance", "Custom", "Turbo"]),
        cpu in any::<i64>(),
        gpu in any::<i64>(),
        fan in any::<i64>(),
        brightness in any::<i64>(),
    ) {
        let text = format!(
            "preset = \"{preset}\"\n[custom]\ncpu_boost = {cpu}\ngpu_boost = {gpu}\nfan_rpm = {fan}\n[lighting]\nbrightness = {brightness}\n"
        );
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, &text).expect("written");

        if let Ok(config) = Config::load(&path) {
            prop_assert!(check(&config, &BLADE_18).is_ok(), "{config:?}");
        }
    }
}
