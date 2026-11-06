//! The header card's view model.

use blade_core::{Preset, Readings};
use platform::GpuReading;
use razer_proto::{BLADE_18, Celsius, FanRpm};
use ui::{Header, HeaderState, PowerPill, Stat};

fn readings() -> Readings {
    Readings {
        cpu_load_percent: Some(18),
        battery_percent: Some(84),
        power_draw_mw: None,
        gpu: Some(GpuReading {
            celsius: 58,
            load_percent: 9,
        }),
    }
}

fn state() -> HeaderState<'static> {
    HeaderState {
        model: &BLADE_18,
        preset: Preset::Balanced,
        on_battery: false,
        readings: readings(),
        cpu_celsius: Some(Celsius(66)),
        fans: vec![Some(FanRpm(3100)), Some(FanRpm(3300))],
    }
}

/// The value shown for a stat, found by its label.
fn value(header: &Header, label: &str) -> String {
    header
        .stats
        .iter()
        .find(|stat| stat.label == label)
        .map_or_else(|| panic!("no {label} stat"), |stat| stat.value.clone())
}

fn stat(label: &str, value: &str) -> Stat {
    Stat {
        label: label.to_owned(),
        value: value.to_owned(),
    }
}

#[test]
fn the_title_is_the_model_name() {
    assert_eq!(Header::new(&state()).title, "Blade 18 (RZ09-0529)");
}

#[test]
fn the_subtitle_is_the_preset() {
    let custom = HeaderState {
        preset: Preset::Custom,
        ..state()
    };

    assert_eq!(Header::new(&state()).preset, "Balanced");
    assert_eq!(Header::new(&custom).preset, "Custom");
}

#[test]
fn on_battery_a_saved_performance_shows_as_balanced() {
    let header = Header::new(&HeaderState {
        preset: Preset::Performance,
        on_battery: true,
        ..state()
    });

    assert_eq!(header.preset, "Balanced");
}

#[test]
fn readings_become_the_stats_in_order() {
    assert_eq!(
        Header::new(&state()).stats,
        [
            stat("CPU", "66 °C"),
            stat("CPU load", "18%"),
            stat("GPU", "58 °C"),
            stat("GPU load", "9%"),
            stat("Fan 1", "3100 rpm"),
            stat("Fan 2", "3300 rpm"),
            stat("Battery", "84%"),
            stat("Power", "—"),
        ]
    );
}

#[test]
fn a_missing_cpu_temperature_shows_a_dash() {
    let header = Header::new(&HeaderState {
        cpu_celsius: None,
        ..state()
    });

    assert_eq!(value(&header, "CPU"), "—");
}

#[test]
fn a_sleeping_gpu_shows_dashes() {
    let mut asleep = state();
    asleep.readings.gpu = None;

    let header = Header::new(&asleep);

    assert_eq!(value(&header, "GPU"), "—");
    assert_eq!(value(&header, "GPU load"), "—");
}

#[test]
fn every_failed_windows_reading_shows_a_dash() {
    let header = Header::new(&HeaderState {
        readings: Readings::default(),
        ..state()
    });

    for label in ["CPU load", "GPU", "GPU load", "Battery", "Power"] {
        assert_eq!(value(&header, label), "—", "{label}");
    }
}

#[test]
fn each_fan_gets_a_stat() {
    let header = Header::new(&HeaderState {
        fans: vec![Some(FanRpm(1300))],
        ..state()
    });

    assert_eq!(value(&header, "Fan 1"), "1300 rpm");
    assert!(header.stats.iter().all(|stat| stat.label != "Fan 2"));
}

#[test]
fn a_fan_that_could_not_be_read_shows_a_dash() {
    let header = Header::new(&HeaderState {
        fans: vec![None, Some(FanRpm(1800))],
        ..state()
    });

    assert_eq!(value(&header, "Fan 1"), "—");
    assert_eq!(value(&header, "Fan 2"), "1800 rpm");
}

#[test]
fn plugged_in_shows_the_plugged_in_pill() {
    let header = Header::new(&state());

    assert_eq!(header.power, PowerPill::PluggedIn);
    assert_eq!(header.power.label(), "Plugged in");
}

#[test]
fn on_battery_shows_the_battery_pill_and_the_draw_in_watts() {
    let mut on_battery = state();
    on_battery.on_battery = true;
    on_battery.readings.power_draw_mw = Some(31_600);

    let header = Header::new(&on_battery);

    assert_eq!(header.power, PowerPill::OnBattery);
    assert_eq!(header.power.label(), "On battery");
    assert_eq!(value(&header, "Power"), "32 W");
}
