//! Ticket 299: a first start reads each zone's mode and fan byte, and sends
//! the two steps that end a manual fan (ticket 298) only when a zone reads
//! Custom with a manual fan, as a kill or Razer software may leave it.

use blade_core::{Config, Preset};
use razer_proto::{BLADE_18, Command, PerfMode};

use crate::{app_with, auto_fans_then, sent};

/// The mode writes.
const SET_MODE: u16 = 0x0D02;

/// The mode writes of the start's send.
fn modes_sent(sent: &[Command]) -> Vec<Command> {
    sent.iter().filter(|c| c.id == SET_MODE).cloned().collect()
}

/// A start with `config` saved, the controller's zones reading
/// `[1, zone, mode, fan]`, or failing to read with `None`.
fn start_with(config: Config, mode_fan: Option<(u8, u8)>) -> Vec<Command> {
    let (mut app, _dir, _) = app_with(config);
    if let Some((mode, fan)) = mode_fan {
        for &zone in BLADE_18.perf_zones {
            app.machine_mut()
                .controller
                .inner
                .modes
                .insert(zone, vec![1, zone, mode, fan]);
        }
    }
    app.start().unwrap();
    sent(&app).to_vec()
}

#[test]
fn a_start_over_a_manual_custom_sends_the_fans_on_auto_first() {
    let sent = start_with(Config::default(), Some((BLADE_18.custom.mode, 0x01)));

    assert_eq!(
        modes_sent(&sent),
        modes_sent(&auto_fans_then(PerfMode::Balanced))
    );
}

#[test]
fn a_start_over_a_preset_sends_the_preset_alone() {
    let sent = start_with(Config::default(), Some((0x00, 0x00)));

    assert_eq!(
        modes_sent(&sent),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn a_start_over_custom_on_auto_sends_the_preset_alone() {
    let sent = start_with(Config::default(), Some((BLADE_18.custom.mode, 0x00)));

    assert_eq!(
        modes_sent(&sent),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn a_start_whose_read_fails_sends_the_preset_alone() {
    let sent = start_with(Config::default(), None);

    assert_eq!(
        modes_sent(&sent),
        PerfMode::Balanced.commands(&BLADE_18).unwrap()
    );
}

#[test]
fn a_start_with_custom_saved_reads_no_mode() {
    let config = Config {
        preset: Preset::Custom,
        ..Config::default()
    };
    let (mut app, _dir, _) = app_with(config);

    app.start().unwrap();

    // Only the read-back after the send asks the modes.
    let asked = &app.machine().controller.inner.asked;
    assert_eq!(
        asked.iter().filter(|c| c.id == 0x0D82).count(),
        BLADE_18.perf_zones.len()
    );
}

#[test]
fn a_second_start_reads_no_mode() {
    let (mut app, _dir, _) = app_with(Config::default());
    app.start().unwrap();
    app.machine_mut().controller.inner.asked.clear();

    app.start().unwrap();

    // Only the read-back after the send asks the modes.
    let asked = &app.machine().controller.inner.asked;
    assert_eq!(
        asked.iter().filter(|c| c.id == 0x0D82).count(),
        BLADE_18.perf_zones.len()
    );
}
