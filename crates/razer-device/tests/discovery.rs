use std::ffi::CString;

use razer_device::{HidInfo, find_controller};

/// The keyboard controller, as captured: 1532:02C7, MI_02, usage page 1, usage 2.
fn controller() -> HidInfo {
    info("controller", 0x1532, 0x02C7, 2, 0x01, 0x02)
}

fn info(
    path: &str,
    vendor_id: u16,
    product_id: u16,
    interface_number: i32,
    usage_page: u16,
    usage: u16,
) -> HidInfo {
    HidInfo {
        path: CString::new(path).unwrap(),
        vendor_id,
        product_id,
        interface_number,
        usage_page,
        usage,
    }
}

#[test]
fn the_controller_is_picked_out_of_the_other_devices() {
    let devices = [
        info("other vendor", 0x046D, 0x02C7, 2, 0x01, 0x02),
        info("other product", 0x1532, 0x0099, 2, 0x01, 0x02),
        info("interface 3", 0x1532, 0x02C7, 3, 0x59, 0x01),
        info("other collection", 0x1532, 0x02C7, 2, 0x0C, 0x01),
        controller(),
    ];

    assert_eq!(find_controller(&devices), Some(&controller()));
}

#[test]
fn no_controller_is_none() {
    let devices = [info("interface 3", 0x1532, 0x02C7, 3, 0x59, 0x01)];

    assert_eq!(find_controller(&devices), None);
}
