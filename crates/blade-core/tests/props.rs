//! Property tests (ticket 146): key reports keep the press pause, and any
//! config text loads only as written, with `check` refusing exactly the
//! values out of the model's range.

use std::time::{Duration, Instant};

use blade_core::{Config, POWER_KEY_PAUSE, PowerKeyPresses, check};
use proptest::prelude::*;
use razer_proto::BLADE_18;

/// A key report: mostly a well-formed one (16 bytes, report ID 4, key
/// bytes such as the power-mode key's `D3`), sometimes noise.
fn key_report() -> impl Strategy<Value = Vec<u8>> {
    let key_byte = prop_oneof![Just(0x00_u8), Just(0xD3), Just(0x0A), any::<u8>()];
    let formed = prop::collection::vec(key_byte, 15).prop_map(|rest| {
        let mut report = vec![0x04];
        report.extend(rest);
        report
    });
    prop_oneof![
        4 => formed,
        1 => prop::collection::vec(any::<u8>(), 0..70),
    ]
}

proptest! {
    #[test]
    fn steps_never_come_closer_than_the_press_pause(
        presses in prop::collection::vec((key_report(), 0_u64..800, any::<bool>(), any::<bool>()), 0..60),
    ) {
        let key = BLADE_18.power_key.expect("the Blade 18 has a power-mode key");
        let mut keys = PowerKeyPresses::new(key);
        let mut now = Instant::now();
        let (mut last_step, mut last_mic): (Option<Instant>, Option<Instant>) = (None, None);
        for (report, gap, enter, mic) in presses {
            now += Duration::from_millis(gap);
            let stepped = keys.on_report(&report, now) | keys.on_enter(enter, now);
            if stepped {
                if let Some(last) = last_step {
                    prop_assert!(now.duration_since(last) >= POWER_KEY_PAUSE);
                }
                last_step = Some(now);
            }
            if keys.on_mic(mic, now) {
                if let Some(last) = last_mic {
                    prop_assert!(now.duration_since(last) >= POWER_KEY_PAUSE);
                }
                last_mic = Some(now);
            }
            keys.brightness_due(now);
        }
    }

    #[test]
    fn any_config_text_never_panics(text in "(?s).{0,400}") {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, &text).expect("written");

        let _ = Config::load(&path);
        let _ = Config::import(&path);
    }

    #[test]
    fn a_config_loads_as_written_and_check_refuses_exactly_the_out_of_range_values(
        preset in prop::sample::select(vec!["silent", "balanced", "performance", "custom", "turbo"]),
        cpu in -5_i32..300,
        gpu in -5_i32..300,
        fan in prop_oneof![
            prop::sample::select(vec![-1_i32, 0, 1500, 1600, 1650, 4500, 4600, 65_535, 70_000]),
            -1_i32..70_000,
        ],
        brightness in -5_i32..300,
    ) {
        let text = format!(
            "preset = \"{preset}\"\n[custom]\ncpu_boost = {cpu}\ngpu_boost = {gpu}\nfan_rpm = {fan}\n\
             [lighting]\nbrightness = {brightness}\ncolour = \"#FFFFFF\"\n"
        );
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, &text).expect("written");

        let fits = |value: i32, max: i32| (0..=max).contains(&value);
        let loads = preset != "turbo"
            && fits(cpu, 255)
            && fits(gpu, 255)
            && fits(fan, 65_535)
            && fits(brightness, 255);
        let loaded = Config::load(&path);
        prop_assert_eq!(loaded.is_ok(), loads, "{}", text);
        let Ok(config) = loaded else { return Ok(()) };

        // Loaded as written: nothing clamped.
        prop_assert_eq!(i32::from(config.custom.cpu_boost), cpu);
        prop_assert_eq!(config.custom.fan_rpm.map(i32::from), Some(fan));
        let limits = BLADE_18.custom;
        let in_range = fits(cpu, i32::from(limits.cpu_max))
            && fits(gpu, i32::from(limits.gpu_max))
            && (i32::from(limits.fan_rpm.min)..=i32::from(limits.fan_rpm.max)).contains(&fan)
            && fan % 100 == 0
            && fits(brightness, 100);
        prop_assert_eq!(check(&config, &BLADE_18).is_ok(), in_range, "{}", text);
    }
}
