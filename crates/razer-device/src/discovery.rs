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

/// Whether the device has the model's vendor and product id.
fn same_usb(device: &HidInfo, usb: &UsbId) -> bool {
    device.vendor_id == usb.vendor_id && device.product_id == usb.product_id
}
