//! Whether the charger gives full power, from the controller's charger read.

use blade_core::{Query, full_power_charger};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Charger, Command, ModelProfile};

/// Gives one reply to the charger read, or fails it with `None`.
struct FakeCharger(Option<Vec<u8>>);

impl Query for FakeCharger {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        assert_eq!(*command, Charger::query());
        self.0.clone().ok_or(DeviceError::Busy)
    }
}

const NO_CHARGER_READ: ModelProfile = ModelProfile {
    charger: None,
    ..BLADE_18
};

fn read(reply: &[u8]) -> Result<bool, DeviceError> {
    full_power_charger(&mut FakeCharger(Some(reply.to_vec())), &BLADE_18)
}

#[test]
fn the_razer_charger_is_full_power() {
    assert!(read(&[0x13, 0x13]).unwrap());
}

#[test]
fn a_usb_c_source_or_none_is_not() {
    assert!(!read(&[0x04, 0x13]).unwrap());
    assert!(!read(&[0x00, 0x13]).unwrap());
}

#[test]
fn a_reply_not_understood_is_not_full_power() {
    assert!(!read(&[0x13]).unwrap());
}

#[test]
fn a_failed_read_is_an_error() {
    let read = full_power_charger(&mut FakeCharger(None), &BLADE_18);

    assert!(matches!(read, Err(DeviceError::Busy)));
}

#[test]
fn a_model_without_the_read_counts_any_charger_as_full_power() {
    // The fake would fail any read: none is sent.
    let read = full_power_charger(&mut FakeCharger(None), &NO_CHARGER_READ);

    assert!(read.unwrap());
}
