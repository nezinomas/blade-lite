//! What differs between laptop models, kept as data.
//! Adding a model means adding a profile, not changing the code.

/// How the controller shows up on USB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbId {
    pub vendor_id: u16,
    pub product_id: u16,
    /// The interface that takes the 90-byte report.
    pub interface_number: i32,
    /// That interface has many collections. Only this one takes the report.
    pub usage_page: u16,
    pub usage: u16,
}

/// One laptop model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelProfile {
    pub name: &'static str,
    pub usb: UsbId,
    /// The LEDs a brightness change sets, in the order Synapse sets them.
    pub brightness_leds: &'static [u8],
}

/// The Blade 18 (RZ09-0529), from the stage 1 capture.
pub const BLADE_18: ModelProfile = ModelProfile {
    name: "Blade 18 (RZ09-0529)",
    usb: UsbId {
        vendor_id: 0x1532,
        product_id: 0x02C7,
        interface_number: 2,
        usage_page: 0x01,
        usage: 0x02,
    },
    // The keyboard, then the underside.
    brightness_leds: &[0x05, 0x26],
};
