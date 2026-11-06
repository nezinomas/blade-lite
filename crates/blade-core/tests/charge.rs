//! The battery charge limit: read from the controller, and set through it.

use std::cell::RefCell;

use blade_core::{ChargeError, Controller, Query, charge_limit_on, set_charge_limit};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, ChargeLimit, ChargeLimitError, Command, ModelProfile};

/// A controller that keeps the limit byte. A set changes it unless the fake
/// is `stuck`. `None` fails every read.
#[derive(Default)]
struct FakeController {
    stored: Option<u8>,
    ran: RefCell<Vec<Command>>,
    fails: bool,
    stuck: bool,
    read_fails: bool,
    /// What a set stores in place of its own byte, if any.
    stores: Option<u8>,
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        assert_eq!(*command, ChargeLimit::query());
        if self.read_fails {
            return Err(DeviceError::Busy);
        }
        self.stored.map(|byte| vec![byte]).ok_or(DeviceError::Busy)
    }
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        if !self.stuck {
            self.stored = self
                .stores
                .or_else(|| commands.last().map(|command| command.args[0]));
        }
        self.ran.borrow_mut().extend_from_slice(commands);
        Ok(())
    }
}

fn limit() -> ChargeLimit {
    BLADE_18.charge_limit.unwrap()
}

const NO_LIMIT: ModelProfile = ModelProfile {
    charge_limit: None,
    ..BLADE_18
};

const UNSAFE_LIMIT: ModelProfile = ModelProfile {
    charge_limit: Some(ChargeLimit { percent: 100 }),
    ..BLADE_18
};

fn storing(byte: u8) -> FakeController {
    FakeController {
        stored: Some(byte),
        ..FakeController::default()
    }
}

#[test]
fn the_limit_is_read_from_the_controller() {
    assert_eq!(
        charge_limit_on(&mut storing(0xD0), &BLADE_18).unwrap(),
        Some(true)
    );
    assert_eq!(
        charge_limit_on(&mut storing(0x50), &BLADE_18).unwrap(),
        Some(false)
    );
}

#[test]
fn another_percent_reads_as_unknown() {
    assert_eq!(
        charge_limit_on(&mut storing(0xBC), &BLADE_18).unwrap(),
        None
    );
}

#[test]
fn a_failed_read_is_an_error() {
    let result = charge_limit_on(&mut FakeController::default(), &BLADE_18);

    assert!(matches!(result, Err(DeviceError::Busy)));
}

#[test]
fn a_model_without_a_limit_is_not_asked() {
    // The fake fails every read, so a query would be an error.
    assert_eq!(
        charge_limit_on(&mut FakeController::default(), &NO_LIMIT).unwrap(),
        None
    );
}

#[test]
fn setting_the_limit_sends_its_command_and_reads_it_back() {
    let mut controller = storing(0x50);

    set_charge_limit(true, &mut controller, &BLADE_18).unwrap();

    assert_eq!(*controller.ran.borrow(), [limit().command(true).unwrap()]);
    assert_eq!(controller.stored, Some(0xD0));
}

#[test]
fn a_model_without_a_limit_refuses_and_sends_nothing() {
    let mut controller = storing(0x50);

    let result = set_charge_limit(true, &mut controller, &NO_LIMIT);

    assert!(matches!(result, Err(ChargeError::NoLimit)));
    assert!(controller.ran.borrow().is_empty());
}

#[test]
fn an_unsafe_percent_refuses_and_sends_nothing() {
    let mut controller = storing(0x50);

    let result = set_charge_limit(true, &mut controller, &UNSAFE_LIMIT);

    assert!(matches!(
        result,
        Err(ChargeError::Unsafe(ChargeLimitError::UnsafePercent(100)))
    ));
    assert!(controller.ran.borrow().is_empty());
}

#[test]
fn a_failed_send_is_an_error() {
    let mut controller = FakeController {
        fails: true,
        ..storing(0x50)
    };

    let result = set_charge_limit(true, &mut controller, &BLADE_18);

    assert!(matches!(
        result,
        Err(ChargeError::Device(DeviceError::Busy))
    ));
}

#[test]
fn a_limit_that_does_not_read_back_is_an_error() {
    let mut controller = FakeController {
        stuck: true,
        ..storing(0x50)
    };

    let result = set_charge_limit(true, &mut controller, &BLADE_18);

    assert!(matches!(
        result,
        Err(ChargeError::NotConfirmed {
            wanted: true,
            read: Some(false)
        })
    ));
}

#[test]
fn turning_the_limit_off_sends_80_without_the_on_bit() {
    let mut controller = storing(0xD0);

    set_charge_limit(false, &mut controller, &BLADE_18).unwrap();

    assert_eq!(*controller.ran.borrow(), [limit().command(false).unwrap()]);
    assert_eq!(controller.stored, Some(0x50));
}

#[test]
fn a_read_back_at_another_percent_is_not_confirmed() {
    let mut controller = FakeController {
        stores: Some(0xBC),
        ..storing(0x50)
    };

    let result = set_charge_limit(true, &mut controller, &BLADE_18);

    assert!(matches!(
        result,
        Err(ChargeError::NotConfirmed {
            wanted: true,
            read: None
        })
    ));
}

#[test]
fn a_failed_read_back_says_the_command_was_sent() {
    let mut controller = FakeController {
        read_fails: true,
        ..storing(0x50)
    };

    let result = set_charge_limit(true, &mut controller, &BLADE_18);

    assert!(matches!(
        result,
        Err(ChargeError::ReadBack(DeviceError::Busy))
    ));
    assert_eq!(controller.ran.borrow().len(), 1);
}
