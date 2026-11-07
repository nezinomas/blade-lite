use std::ffi::CString;

use razer_device::{HidInfo, find_controller, find_key_collections};
use razer_proto::{BLADE_18, UsbId};

/// The keyboard controller, as captured: 1532:02C7, `MI_02`, usage page 1, usage 2.
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

    assert_eq!(
        find_controller(&devices, &BLADE_18.usb),
        Some(&controller())
    );
}

#[test]
fn no_controller_is_none() {
    let devices = [info("interface 3", 0x1532, 0x02C7, 3, 0x59, 0x01)];

    assert_eq!(find_controller(&devices, &BLADE_18.usb), None);
}

#[test]
fn another_model_is_found_by_its_own_usb_id() {
    let other_model = UsbId {
        product_id: 0x0099,
        ..BLADE_18.usb
    };
    let devices = [
        controller(),
        info("other model", 0x1532, 0x0099, 2, 0x01, 0x02),
    ];

    let found = find_controller(&devices, &other_model).unwrap();

    assert_eq!(found.path.to_str().unwrap(), "other model");
}

/// The key report collections, as read on the laptop: interface 1 has two
/// collections with usage page 1 and usage 0.
fn key_collections() -> [HidInfo; 2] {
    [
        info("keys a", 0x1532, 0x02C7, 1, 0x01, 0x00),
        info("keys b", 0x1532, 0x02C7, 1, 0x01, 0x00),
    ]
}

#[test]
fn the_power_key_collections_are_picked_out_of_the_other_devices() {
    let [a, b] = key_collections();
    let devices = [
        info("consumer keys", 0x1532, 0x02C7, 1, 0x0C, 0x01),
        a.clone(),
        info("system keys", 0x1532, 0x02C7, 1, 0x01, 0x80),
        controller(),
        info("other product", 0x1532, 0x0099, 1, 0x01, 0x00),
        info("other vendor", 0x046D, 0x02C7, 1, 0x01, 0x00),
        b.clone(),
    ];
    let key = BLADE_18.power_key.unwrap();

    let found = find_key_collections(&devices, &BLADE_18.usb, &key);

    assert_eq!(found, [&a, &b]);
}

#[test]
fn no_power_key_collection_gives_none() {
    let key = BLADE_18.power_key.unwrap();

    let devices = [controller()];

    let found = find_key_collections(&devices, &BLADE_18.usb, &key);

    assert_eq!(found, Vec::<&HidInfo>::new());
}
