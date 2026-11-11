//! The header card's view model.

use blade_core::Readings;
use platform::{BatteryHealth, GpuReading};
use razer_proto::{BLADE_18, Celsius, FanRpm};
use ui::{Header, HeaderState, PowerPill, Stat};

fn readings() -> Readings {
    Readings {
        cpu_load_percent: Some(18),
        battery_percent: Some(84),
        power_draw_mw: None,
        charging: None,
        battery_health: health(Some(99), Some(86)),
        gpu: Some(GpuReading {
            celsius: 58,
            load_percent: 9,
        }),
    }
}

fn health(percent: Option<u32>, cycles: Option<u32>) -> BatteryHealth {
    BatteryHealth { percent, cycles }
}

fn state() -> HeaderState<'static> {
    HeaderState {
        model: &BLADE_18,
        on_battery: false,
        weak_charge: false,
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

/// The header plugged in, with the battery's charging flag.
fn plugged_in(charging: Option<bool>) -> Header {
    let mut plugged_in = state();
    plugged_in.readings.charging = charging;
    Header::new(&plugged_in)
}

#[test]
fn plugged_in_says_whether_the_battery_charges() {
    assert_eq!(plugged_in(Some(true)).power, PowerPill::Charging);
    assert_eq!(PowerPill::Charging.label(), "Plugged in \u{b7} Charging");
    assert_eq!(plugged_in(Some(false)).power, PowerPill::NotCharging);
    assert_eq!(
        PowerPill::NotCharging.label(),
        "Plugged in \u{b7} Not charging"
    );
    assert_eq!(plugged_in(None).power, PowerPill::PluggedIn);
}

#[test]
fn on_battery_the_charging_flag_is_not_shown() {
    let mut on_battery = state();
    on_battery.on_battery = true;
    on_battery.readings.charging = Some(true);

    assert_eq!(Header::new(&on_battery).power, PowerPill::OnBattery);
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

#[test]
fn a_weak_charge_shows_its_pill() {
    let mut weak = state();
    weak.weak_charge = true;
    weak.readings.charging = Some(true);

    let header = Header::new(&weak);

    assert_eq!(header.power, PowerPill::WeakCharge);
    assert_eq!(header.power.label(), "Plugged in \u{b7} Weak charger");
}

#[test]
fn on_battery_the_weak_charge_is_not_shown() {
    let mut on_battery = state();
    on_battery.on_battery = true;
    on_battery.weak_charge = true;

    assert_eq!(Header::new(&on_battery).power, PowerPill::OnBattery);
}

#[test]
fn the_battery_health_shows_its_percent_and_cycles() {
    let health_line = |percent, cycles| {
        let mut state = state();
        state.readings.battery_health = health(percent, cycles);
        Header::new(&state).health
    };

    assert_eq!(health_line(Some(99), Some(86)), "99%, 86 cycles");
    assert_eq!(health_line(Some(99), Some(1)), "99%, 1 cycle");
    assert_eq!(health_line(Some(99), None), "99%");
    assert_eq!(health_line(None, Some(86)), "\u{2014}, 86 cycles");
    assert_eq!(health_line(None, None), "", "nothing known shows no text");
}
