//! What differs between laptop models, kept as data.
//! Adding a model means adding a profile, not changing the code.

use crate::{CustomLimits, NativeMode, PerfMode, RpmRange};

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

/// The keyboard's lighting grid. Each key is at a row and a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyGrid {
    pub rows: u8,
    pub columns: u8,
    /// `(row, column)` positions that have no key. They stay black.
    pub no_key: &'static [(u8, u8)],
}

impl KeyGrid {
    /// True if a key is at this position.
    #[must_use]
    pub fn has_key(&self, row: u8, column: u8) -> bool {
        !self.no_key.contains(&(row, column))
    }
}

/// One laptop model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelProfile {
    pub name: &'static str,
    pub usb: UsbId,
    /// The LEDs a brightness change sets, in the order Synapse sets them.
    pub brightness_leds: &'static [u8],
    pub keys: KeyGrid,
    /// The presets the model offers.
    pub presets: &'static [PerfMode],
    /// The zones a preset change sets, in the order Synapse sets them.
    pub perf_zones: &'static [u8],
    /// What the Custom preset accepts.
    pub custom: CustomLimits,
    /// The panel's native modes. Empty if the panel has only one.
    pub native_modes: &'static [NativeMode],
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
    // Synapse keeps these positions black in every frame.
    keys: KeyGrid {
        rows: 7,
        columns: 19,
        no_key: &[
            (0, 0),
            (1, 0),
            (1, 14),
            (2, 0),
            (2, 14),
            (3, 0),
            (3, 13),
            (3, 14),
            (4, 0),
            (4, 2),
            (4, 13),
            (4, 14),
            (5, 0),
            (5, 4),
            (5, 7),
            (5, 8),
            (6, 0),
        ],
    },
    presets: &[PerfMode::Silent, PerfMode::Balanced, PerfMode::Performance],
    // Synapse sets all four. r-helper sets only 1 and 2.
    perf_zones: &[1, 2, 3, 4],
    custom: CustomLimits {
        // Synapse sent levels 0 to 3 for both.
        cpu_max: 3,
        gpu_max: 3,
        // Only speeds seen to work here: 1600 is the lowest Synapse sent,
        // 4500 the highest r-helper set and read back.
        fan_rpm: RpmRange {
            min: 1600,
            max: 4500,
        },
        // Two fans. Synapse sets and reads only these.
        fan_zones: &[1, 2],
    },
    // From Synapse's code ("Mini LED Panel Resolution"), not a capture.
    native_modes: &[
        NativeMode {
            id: 0,
            width: 3840,
            height: 2400,
            hz: 240,
        },
        NativeMode {
            id: 1,
            width: 1920,
            height: 1200,
            hz: 440,
        },
    ],
};
