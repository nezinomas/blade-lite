//! Windows' USB selective suspend for the Razer device (ticket 213). With it
//! on, the device sleeps a few seconds after its last use, and each wake
//! drops the keyboard lights (tickets 202 to 205). Read from the registry
//! only, never from the device.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use platform::{RegValue, UsbInstance};
use razer_proto::UsbId;

use crate::report;

/// The value `tools/Set-RazerUsbSuspend.ps1` sets on each interface.
pub const SUSPEND_VALUE: &str = "SelectiveSuspendEnabled";

/// The driver of each of the device's interfaces.
const HID_DRIVER: &str = "HidUsb";

/// Set once the start's read finds suspend on; never set back.
static SUSPEND_ON: AtomicBool = AtomicBool::new(false);

/// Whether the start's read found suspend on (ticket 220): `false` while it
/// is off, odd or not read yet, so the window never says on when it is not
/// known.
pub fn usb_suspend_on() -> bool {
    SUSPEND_ON.load(Ordering::Relaxed)
}

/// Logs once if USB suspend is on for the device, or if its read is odd,
/// and notes on for the window. On a thread of its own, so the start never
/// waits for it; a failed read only logs.
pub fn report_usb_suspend(usb: UsbId) {
    let spawned = thread::Builder::new()
        .name("usb suspend".into())
        .spawn(move || {
            let count = usb.hid_interfaces;
            let reads =
                platform::usb_interfaces(usb.vendor_id, usb.product_id, count, SUSPEND_VALUE);
            let state = usb_suspend(&reads, count);
            if state == UsbSuspend::On {
                SUSPEND_ON.store(true, Ordering::Relaxed);
            }
            if let Some(line) = usb_suspend_report(&state) {
                report(&line);
            }
        });
    if let Err(error) = spawned {
        report(&format!(
            "could not read Windows USB suspend for the Razer device: {error}"
        ));
    }
}

/// What the registry says about USB suspend for the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbSuspend {
    /// Off on every interface.
    Off,
    /// On for at least one interface.
    On,
    /// Not what the script leaves: the reason.
    Odd(String),
}

/// The state from the reads of interfaces `MI_00` up, one entry each, in
/// order. Each interface must have exactly one instance, with the `HidUsb`
/// driver and a value of 0 or 1; `count` interfaces must be read.
pub fn usb_suspend(interfaces: &[io::Result<Vec<UsbInstance>>], count: u8) -> UsbSuspend {
    if count == 0 || interfaces.len() != usize::from(count) {
        return UsbSuspend::Odd(format!("read {} interfaces, not {count}", interfaces.len()));
    }
    let mut on = false;
    for (number, interface) in interfaces.iter().enumerate() {
        match interface_value(interface) {
            Ok(value) => on |= value,
            Err(reason) => return UsbSuspend::Odd(format!("interface MI_{number:02X}: {reason}")),
        }
    }
    if on { UsbSuspend::On } else { UsbSuspend::Off }
}

/// Whether suspend is on for one interface, or why the read is odd.
fn interface_value(interface: &io::Result<Vec<UsbInstance>>) -> Result<bool, String> {
    let instances = interface.as_ref().map_err(ToString::to_string)?;
    let [instance] = instances.as_slice() else {
        return Err(format!("{} instances, not 1", instances.len()));
    };
    let service = instance
        .service
        .as_ref()
        .map_err(|error| format!("driver: {error}"))?;
    if !service.eq_ignore_ascii_case(HID_DRIVER) {
        return Err(format!("driver {service}, not {HID_DRIVER}"));
    }
    let value = instance
        .value
        .as_ref()
        .map_err(|error| format!("{SUSPEND_VALUE}: {error}"))?;
    suspend_on(value).ok_or_else(|| format!("{SUSPEND_VALUE} is {value:?}, not 0 or 1"))
}

/// A DWORD or a one-byte binary value of 0 or 1, as on or off.
fn suspend_on(value: &RegValue) -> Option<bool> {
    let number = match value {
        RegValue::Dword(number) => *number,
        RegValue::Binary(bytes) => match bytes.as_slice() {
            [byte] => u32::from(*byte),
            _ => return None,
        },
        RegValue::Other => return None,
    };
    match number {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// The log line for the state, if it needs one.
pub fn usb_suspend_report(state: &UsbSuspend) -> Option<String> {
    match state {
        UsbSuspend::Off => None,
        UsbSuspend::On => Some(
            "Windows USB suspend is on for the Razer device, so the keyboard lights may go off \
             or blink; run tools\\Set-RazerUsbSuspend.ps1 (README, \"If the keyboard lights go \
             off or blink\")"
                .to_string(),
        ),
        UsbSuspend::Odd(reason) => Some(format!(
            "could not read Windows USB suspend for the Razer device: {reason}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(service: &str, value: RegValue) -> UsbInstance {
        UsbInstance {
            service: Ok(service.to_string()),
            value: Ok(value),
        }
    }

    /// One `HidUsb` instance holding `value`.
    fn one(value: RegValue) -> Vec<UsbInstance> {
        vec![instance("HidUsb", value)]
    }

    fn four(values: [RegValue; 4]) -> Vec<io::Result<Vec<UsbInstance>>> {
        values.into_iter().map(|value| Ok(one(value))).collect()
    }

    fn odd(state: &UsbSuspend) -> bool {
        matches!(state, UsbSuspend::Odd(_))
    }

    #[test]
    fn all_dword_zero_is_off() {
        let reads = four([0, 0, 0, 0].map(RegValue::Dword));
        assert_eq!(usb_suspend(&reads, 4), UsbSuspend::Off);
    }

    #[test]
    fn all_binary_zero_is_off() {
        let reads = four([0, 0, 0, 0].map(|byte| RegValue::Binary(vec![byte])));
        assert_eq!(usb_suspend(&reads, 4), UsbSuspend::Off);
    }

    #[test]
    fn one_interface_on_is_on() {
        let reads = four([
            RegValue::Dword(0),
            RegValue::Binary(vec![1]),
            RegValue::Dword(0),
            RegValue::Dword(0),
        ]);
        assert_eq!(usb_suspend(&reads, 4), UsbSuspend::On);
    }

    #[test]
    fn all_on_is_on() {
        let reads = four([1, 1, 1, 1].map(RegValue::Dword));
        assert_eq!(usb_suspend(&reads, 4), UsbSuspend::On);
    }

    #[test]
    fn an_odd_value_wins_over_on() {
        let reads = four([
            RegValue::Dword(1),
            RegValue::Dword(0),
            RegValue::Dword(0),
            RegValue::Dword(2),
        ]);
        assert!(odd(&usb_suspend(&reads, 4)));
    }

    #[test]
    fn values_other_than_0_or_1_are_odd() {
        for value in [
            RegValue::Dword(2),
            RegValue::Dword(u32::MAX),
            RegValue::Binary(vec![2]),
            RegValue::Binary(vec![]),
            RegValue::Binary(vec![0, 0]),
            RegValue::Binary(vec![0, 0, 0, 0]),
            RegValue::Other,
        ] {
            let mut reads = four([0, 0, 0, 0].map(RegValue::Dword));
            reads[2] = Ok(one(value.clone()));
            assert!(odd(&usb_suspend(&reads, 4)), "{value:?}");
        }
    }

    #[test]
    fn a_count_other_than_the_profiles_is_odd() {
        let three: Vec<_> = (0..3).map(|_| Ok(one(RegValue::Dword(1)))).collect();
        assert!(odd(&usb_suspend(&three, 4)));
        let five: Vec<_> = (0..5).map(|_| Ok(one(RegValue::Dword(1)))).collect();
        assert!(odd(&usb_suspend(&five, 4)));
        assert!(odd(&usb_suspend(&[], 0)));
    }

    #[test]
    fn a_missing_interface_key_is_odd() {
        let mut reads = four([1, 1, 1, 1].map(RegValue::Dword));
        reads[0] = Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(odd(&usb_suspend(&reads, 4)));
    }

    #[test]
    fn other_than_one_instance_is_odd() {
        let mut reads = four([1, 1, 1, 1].map(RegValue::Dword));
        reads[1] = Ok(vec![]);
        assert!(odd(&usb_suspend(&reads, 4)));
        reads[1] = Ok(vec![
            instance("HidUsb", RegValue::Dword(1)),
            instance("HidUsb", RegValue::Dword(1)),
        ]);
        assert!(odd(&usb_suspend(&reads, 4)));
    }

    #[test]
    fn another_driver_is_odd() {
        let mut reads = four([1, 1, 1, 1].map(RegValue::Dword));
        reads[3] = Ok(vec![instance("usbccgp", RegValue::Dword(1))]);
        assert!(odd(&usb_suspend(&reads, 4)));
    }

    #[test]
    fn the_driver_name_is_not_case_sensitive() {
        let mut reads = four([0, 0, 0, 0].map(RegValue::Dword));
        reads[0] = Ok(vec![instance("HIDUSB", RegValue::Dword(1))]);
        assert_eq!(usb_suspend(&reads, 4), UsbSuspend::On);
    }

    #[test]
    fn a_failed_read_is_odd() {
        let mut reads = four([1, 1, 1, 1].map(RegValue::Dword));
        reads[2] = Ok(vec![UsbInstance {
            service: Ok("HidUsb".to_string()),
            value: Err(io::Error::from(io::ErrorKind::NotFound)),
        }]);
        assert!(odd(&usb_suspend(&reads, 4)));
        reads[2] = Ok(vec![UsbInstance {
            service: Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            value: Ok(RegValue::Dword(1)),
        }]);
        assert!(odd(&usb_suspend(&reads, 4)));
    }

    /// On the laptop (`cargo hw`): the real registry reads as on or off.
    /// Reads the registry only, never the device.
    #[test]
    #[ignore = "needs the Blade 18's registry keys"]
    fn the_laptops_registry_reads_as_on_or_off() {
        let usb = razer_proto::BLADE_18.usb;
        let reads = platform::usb_interfaces(
            usb.vendor_id,
            usb.product_id,
            usb.hid_interfaces,
            SUSPEND_VALUE,
        );
        let state = usb_suspend(&reads, usb.hid_interfaces);
        assert!(!odd(&state), "{state:?}");
    }

    #[test]
    fn off_logs_nothing() {
        assert_eq!(usb_suspend_report(&UsbSuspend::Off), None);
    }

    #[test]
    fn on_logs_the_fix() {
        let line = usb_suspend_report(&UsbSuspend::On).unwrap();
        assert!(line.contains("lights may go off or blink"), "{line}");
        assert!(line.contains(r"tools\Set-RazerUsbSuspend.ps1"), "{line}");
    }

    #[test]
    fn odd_logs_the_reason_and_nothing_about_suspend_being_on() {
        let line = usb_suspend_report(&UsbSuspend::Odd("interface MI_02: gone".into())).unwrap();
        assert!(line.contains("interface MI_02: gone"), "{line}");
        assert!(!line.contains("is on"), "{line}");
        assert!(!line.contains("Set-RazerUsbSuspend"), "{line}");
    }
}
