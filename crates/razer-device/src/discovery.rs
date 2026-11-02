//! Finds the controller among the HID devices.

use std::ffi::CString;

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

/// Razer.
const VENDOR_ID: u16 = 0x1532;
/// The Blade 18 (RZ09-0529) keyboard controller.
const PRODUCT_ID: u16 = 0x02C7;
/// `MI_02`: the only interface Synapse sends feature reports to.
const INTERFACE_NUMBER: i32 = 2;
/// `MI_02` has many collections. Only this one takes the 90-byte report.
const USAGE_PAGE: u16 = 0x01;
const USAGE: u16 = 0x02;

/// Returns the controller's collection, if it is in `devices`.
#[must_use]
pub fn find_controller(devices: &[HidInfo]) -> Option<&HidInfo> {
    devices.iter().find(|device| is_controller(device))
}

fn is_controller(device: &HidInfo) -> bool {
    device.vendor_id == VENDOR_ID
        && device.product_id == PRODUCT_ID
        && device.interface_number == INTERFACE_NUMBER
        && device.usage_page == USAGE_PAGE
        && device.usage == USAGE
}
