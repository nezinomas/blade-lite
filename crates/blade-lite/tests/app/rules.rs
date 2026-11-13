//! The rules of ticket 146 for what the app sends, shared by the app's
//! property tests (ticket 165) and the window tests' soak (ticket 147):
//! every value in the model's range, a fan target only with a manual
//! Custom, and on battery or a weak charge no Performance, no manual fan
//! and no boost.

use std::collections::HashMap;

use razer_proto::{BLADE_18, Command};

pub const SET_PRESET: u16 = 0x0D02;
pub const SET_FAN: u16 = 0x0D01;
pub const SET_BOOST: u16 = 0x0D07;
const CPU_CLUSTER: u8 = 0x01;
const GPU_CLUSTER: u8 = 0x02;
/// The fan byte of a preset write: the controller picks the speed.
const FAN_AUTO: u8 = 0;

/// The storage byte `OpenRazer` calls the stored one.
pub const VARIABLE_STORAGE: u8 = 0x01;

/// Each preset write has a mode byte of the model, and a manual fan only
/// with Custom's; each boost is in the model's range for its cluster.
pub fn assert_in_range(command: &Command) {
    let (bytes, custom) = (BLADE_18.mode_bytes, BLADE_18.custom);
    match command.id {
        SET_PRESET => {
            let (mode, fan) = (command.args[2], command.args[3]);
            let modes = [bytes.silent, bytes.balanced, bytes.performance, custom.mode];
            assert!(modes.contains(&mode), "{command:?}");
            assert!(fan == FAN_AUTO || mode == custom.mode, "{command:?}");
        }
        SET_BOOST => {
            let max = match command.args[1] {
                CPU_CLUSTER => custom.cpu_max,
                GPU_CLUSTER => custom.gpu_max,
                _ => panic!("no such cluster: {command:?}"),
            };
            assert!(command.args[2] <= max, "{command:?}");
        }
        _ => {}
    }
}

/// On battery or a weak charge, no Performance, no manual fan and no boost
/// goes. Custom with the fans on auto goes only as the first part of the
/// quit preset, so a Silent or Balanced write comes after it, unless a run
/// may have failed partway (`may_cut`): the app then stops, as it should.
pub fn assert_battery_safe(sent: &[Command], may_cut: bool) {
    let bytes = BLADE_18.mode_bytes;
    let quiet = |c: &Command| {
        c.id == SET_PRESET && (c.args[2] == bytes.silent || c.args[2] == bytes.balanced)
    };
    for (at, command) in sent.iter().enumerate() {
        assert_ne!(command.id, SET_FAN, "a fan target on battery: {sent:?}");
        if command.id == SET_BOOST {
            assert_eq!(command.args[2], 0, "a boost on battery: {sent:?}");
        }
        if command.id != SET_PRESET {
            continue;
        }
        let (mode, fan) = (command.args[2], command.args[3]);
        assert!(
            mode != bytes.performance && fan == 0,
            "on battery: {sent:?}"
        );
        if mode == BLADE_18.custom.mode && !may_cut {
            assert!(
                sent[at..].iter().any(quiet),
                "Custom left on battery: {sent:?}"
            );
        }
    }
}

/// The last mode and fan byte written to each zone, so each fan target can
/// be checked against what the controller runs.
#[derive(Debug, Default)]
pub struct Zones(HashMap<u8, (u8, u8)>);

impl Zones {
    /// Each fan target goes only while every zone was last set to Custom
    /// with a manual fan (a mode write after it resets it), to a fan of
    /// the model, in the profile's range.
    pub fn check_fan_targets(&mut self, sent: &[Command]) {
        let custom = BLADE_18.custom;
        for command in sent {
            if command.id == SET_PRESET {
                let (zone, mode, fan) = (command.args[1], command.args[2], command.args[3]);
                self.0.insert(zone, (mode, fan));
            }
            if command.id != SET_FAN {
                continue;
            }
            let manual = BLADE_18.perf_zones.iter().all(|zone| {
                self.0
                    .get(zone)
                    .is_some_and(|&(mode, fan)| mode == custom.mode && fan != FAN_AUTO)
            });
            assert!(manual, "a fan target with no manual Custom: {sent:?}");
            let [storage, fan, speed] = command.args[..] else {
                panic!("{command:?}");
            };
            let rpm = u16::from(speed) * 100;
            assert_eq!(storage, VARIABLE_STORAGE, "{command:?}");
            assert!(custom.fan_zones.contains(&fan), "{command:?}");
            assert!(
                (custom.fan_rpm.min..=custom.fan_rpm.max).contains(&rpm),
                "{command:?}"
            );
        }
    }
}
