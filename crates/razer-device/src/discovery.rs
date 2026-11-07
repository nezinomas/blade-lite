//! Finds the controller among the HID devices.

use std::ffi::CString;

use razer_proto::{PowerKey, UsbId};

/// What the HID layer tells about one device collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidInfo {
    pub path: CString,
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
            device.vendor_id == usb.vendor_id
                && device.product_id == usb.product_id
                && device.interface_number == key.interface_number
                && device.usage_page == key.usage_page
                && device.usage == key.usage
        })
        .collect()
}

fn matches(device: &HidInfo, usb: &UsbId) -> bool {
    device.vendor_id == usb.vendor_id
        && device.product_id == usb.product_id
        && device.interface_number == usb.interface_number
        && device.usage_page == usb.usage_page
        && device.usage == usb.usage
}
