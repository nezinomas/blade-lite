//! Finds the controller among the HID devices.

use razer_proto::{PowerKey, UsbId};

/// What the HID layer tells about one device collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface_number: i32,
    pub usage_page: u16,
    pub usage: u16,
}

/// Returns the collection that matches the model's USB id, if it is in `devices`.
#[must_use]
pub fn find_controller<'a>(devices: &'a [HidInfo], usb: &UsbId) -> Option<&'a HidInfo> {
    devices.iter().find(|device| matches(device, usb))
}

/// The collections that send the model's key reports, in `devices` order.
#[must_use]
pub fn find_key_collections<'a>(
    devices: &'a [HidInfo],
    usb: &UsbId,
    key: &PowerKey,
) -> Vec<&'a HidInfo> {
    devices
        .iter()
        .filter(|device| {
            same_usb(device, usb)
                && in_collection(device, key.interface_number, key.usage_page, key.usage)
        })
        .collect()
}

fn matches(device: &HidInfo, usb: &UsbId) -> bool {
    same_usb(device, usb) && in_collection(device, usb.interface_number, usb.usage_page, usb.usage)
}

/// Whether the device is this interface, usage page and usage.
fn in_collection(device: &HidInfo, interface_number: i32, usage_page: u16, usage: u16) -> bool {
    device.interface_number == interface_number
        && device.usage_page == usage_page
        && device.usage == usage
}

/// The USB interface number of a HID interface path when the path is the
/// model's device, read from the path alone, with no open: `None` for any
/// other device. Windows names the device in the path's second part, as
/// `VID_1532&PID_02C7&MI_02&COL01` (`MI_` only on a device with more than
/// one interface: none is interface 0, as hidapi took it).
pub(crate) fn model_interface(path: &str, usb: &UsbId) -> Option<i32> {
    let device = path.split('#').nth(1)?.to_ascii_lowercase();
    let mut tokens = device.split('&');
    let ids = [tokens.next()?, tokens.next()?];
    let model = [
        format!("vid_{:04x}", usb.vendor_id),
        format!("pid_{:04x}", usb.product_id),
    ];
    if ids != model.each_ref().map(String::as_str) {
        return None;
    }
    let interfaces: Vec<&str> = tokens
        .filter_map(|token| token.strip_prefix("mi_"))
        .collect();
    match interfaces.as_slice() {
        [] => Some(0),
        [number] if number.len() == 2 && number.bytes().all(|b| b.is_ascii_hexdigit()) => {
            i32::from_str_radix(number, 16).ok()
        }
        _ => None,
    }
}

/// Whether the device has the model's vendor and product id.
fn same_usb(device: &HidInfo, usb: &UsbId) -> bool {
    device.vendor_id == usb.vendor_id && device.product_id == usb.product_id
}

#[cfg(test)]
mod tests {
    use super::*;

    use razer_proto::BLADE_18;

    /// A HID interface path, as Windows lists it, for this instance ID.
    fn path(id: &str) -> String {
        format!(r"\\?\HID#{id}#7&1c181662&0&0000#{{4d1e55b2-f16f-11cf-88cb-001111000030}}")
    }

    #[test]
    fn the_models_paths_give_their_interface() {
        let usb = &BLADE_18.usb;

        assert_eq!(
            model_interface(&path("VID_1532&PID_02C7&MI_02"), usb),
            Some(2)
        );
        assert_eq!(
            model_interface(&path("VID_1532&PID_02C7&MI_01&COL03"), usb),
            Some(1)
        );
        assert_eq!(
            model_interface(&path("vid_1532&pid_02c7&mi_0a&col01"), usb),
            Some(10)
        );
    }

    #[test]
    fn a_path_with_no_interface_is_interface_0() {
        assert_eq!(
            model_interface(&path("VID_1532&PID_02C7"), &BLADE_18.usb),
            Some(0)
        );
    }

    #[test]
    fn other_devices_paths_are_not_the_models() {
        let usb = &BLADE_18.usb;
        for id in [
            "VID_046D&PID_C52B&MI_01&COL01",
            "VID_1532&PID_0099&MI_02",
            "VID_0532&PID_02C7&MI_02",
            "VID_15320&PID_02C7&MI_02",
            "ELAN0406&COL03",
            "CONVERTEDDEVICE&COL01",
            "{00001124-0000-1000-8000-00805f9b34fb}_VID&00021532_PID&02c7&Col01",
            "VID_1532&PID_02C7&MI_xx",
            "VID_1532&PID_02C7&MI_+1",
            "VID_1532&PID_02C7&MI_002",
            "VID_1532&PID_02C7&MI_02&MI_03",
        ] {
            assert_eq!(model_interface(&path(id), usb), None, "{id}");
        }
    }

    #[test]
    fn the_ids_count_only_in_the_device_part() {
        let other =
            r"\\?\HID#VID_046D&PID_C52B#VID_1532&PID_02C7#{4d1e55b2-f16f-11cf-88cb-001111000030}";

        assert_eq!(model_interface(other, &BLADE_18.usb), None);
        assert_eq!(model_interface("", &BLADE_18.usb), None);
    }
}
