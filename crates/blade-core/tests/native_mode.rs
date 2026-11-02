//! The panel's native mode: set through the controller, live after a restart.

mod common;

use blade_core::{Controller, NativeModeState, native_mode_state, set_native_mode};
use common::{FakeDisplays, mode, panel};
use platform::DisplayMode;
use razer_device::DeviceError;
use razer_proto::{BLADE_18, Command, NativeMode};

const UHD: NativeMode = BLADE_18.native_modes[0];
const FHD: NativeMode = BLADE_18.native_modes[1];

/// The panel as Windows lists it after booting in a native mode: its modes
/// go up to that mode's resolution.
fn booted_in(native: NativeMode) -> FakeDisplays {
    let mut display = panel(mode(native.width, native.height, 60));
    display.modes = vec![
        mode(640, 480, 60),
        mode(native.width, native.height, 60),
        mode(1280, 800, 60),
    ];
    FakeDisplays::new(Some(vec![display]))
}

/// Records each command it is asked to run.
#[derive(Default)]
struct FakeController(Vec<Command>);

impl Controller for FakeController {
    fn run(&mut self, commands: &[Command]) -> Result<(), DeviceError> {
        self.0.extend_from_slice(commands);
        Ok(())
    }
}

#[test]
fn the_chosen_mode_the_panel_booted_in_is_active() {
    let state = native_mode_state(FHD, &BLADE_18, &booted_in(FHD)).unwrap();

    assert_eq!(state, NativeModeState::Active(FHD));
}

#[test]
fn another_chosen_mode_applies_after_restart() {
    let state = native_mode_state(FHD, &BLADE_18, &booted_in(UHD)).unwrap();

    assert_eq!(state, NativeModeState::AppliesAfterRestart(FHD));
}

#[test]
fn the_running_mode_is_found_by_the_largest_resolution() {
    let mut api = booted_in(UHD);
    // The current mode is a small one, but the panel still offers 4K.
    api.displays.as_mut().unwrap()[0].current = DisplayMode {
        width: 1920,
        height: 1200,
        hz: 60,
    };

    let state = native_mode_state(UHD, &BLADE_18, &api).unwrap();

    assert_eq!(state, NativeModeState::Active(UHD));
}

#[test]
fn setting_a_mode_sends_its_command() {
    let mut controller = FakeController::default();

    set_native_mode(FHD, &mut controller).unwrap();

    assert_eq!(controller.0, [FHD.command()]);
}
