//! The lid logo: read from the controller, and set through it.

use std::cell::RefCell;

use blade_core::{Controller, LogoError, Query, logo_on, set_logo};
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Command, ModelProfile};

/// Replies to the logo query with `reply`, or fails it with `None`.
/// Keeps every command it was asked to run.
#[derive(Default)]
struct FakeController {
    reply: Option<Vec<u8>>,
    ran: RefCell<Vec<Command>>,
    fails: bool,
}

impl Query for FakeController {
    fn query(&mut self, command: &Command) -> Result<Vec<u8>, DeviceError> {
        assert_eq!(*command, logo().query());
        self.reply.clone().ok_or(DeviceError::Busy)
    }
}

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        if self.fails {
            return Err(DeviceError::Busy);
        }
        self.ran.borrow_mut().extend_from_slice(commands);
        Ok(())
    }
}

fn logo() -> razer_proto::Logo {
    BLADE_18.logo.unwrap()
}

const NO_LOGO: ModelProfile = ModelProfile {
    logo: None,
    ..BLADE_18
};

fn replying(args: &[u8]) -> FakeController {
    FakeController {
        reply: Some(args.to_vec()),
        ..FakeController::default()
    }
}

#[test]
fn the_logo_state_is_read_from_the_controller() {
    assert_eq!(
        logo_on(&mut replying(&[0, 4, 1]), &BLADE_18).unwrap(),
        Some(true)
    );
    assert_eq!(
        logo_on(&mut replying(&[0, 4, 0]), &BLADE_18).unwrap(),
        Some(false)
    );
}

#[test]
fn an_odd_reply_reads_as_unknown() {
    assert_eq!(logo_on(&mut replying(&[0, 4, 9]), &BLADE_18).unwrap(), None);
}

#[test]
fn a_failed_read_is_an_error() {
    let result = logo_on(&mut FakeController::default(), &BLADE_18);

    assert!(matches!(result, Err(DeviceError::Busy)));
}

#[test]
fn a_model_without_a_logo_is_not_asked() {
    // The fake would panic on any query but the Blade 18 logo's.
    let mut controller = FakeController::default();

    assert_eq!(logo_on(&mut controller, &NO_LOGO).unwrap(), None);
}

#[test]
fn setting_the_logo_sends_its_commands() {
    let mut controller = FakeController::default();

    set_logo(true, &mut controller, &BLADE_18).unwrap();

    assert_eq!(*controller.ran.borrow(), logo().commands(true));
}

#[test]
fn a_model_without_a_logo_refuses_and_sends_nothing() {
    let mut controller = FakeController::default();

    let result = set_logo(true, &mut controller, &NO_LOGO);

    assert!(matches!(result, Err(LogoError::NoLogo)));
    assert!(controller.ran.borrow().is_empty());
}

#[test]
fn a_failed_send_is_an_error() {
    let mut controller = FakeController {
        fails: true,
        ..FakeController::default()
    };

    let result = set_logo(false, &mut controller, &BLADE_18);

    assert!(matches!(result, Err(LogoError::Device(DeviceError::Busy))));
}
