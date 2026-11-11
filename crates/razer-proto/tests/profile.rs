mod common;

use razer_proto::{BLADE_18, ModelProfile, PerfMode};

#[test]
fn every_captured_report_went_through_the_profile_interface() {
    for report in common::captured_reports() {
        assert_eq!(
            report.interface, BLADE_18.usb.interface_number,
            "{}",
            report.fixture
        );
    }
}

/// Frame rows are `030B` requests: args `[FF, row, first column, last column, RGB...]`.
const FRAME_ROW: [u8; 2] = [0x03, 0x0B];
const ROW_INDEX: usize = 9;
const LAST_COLUMN_INDEX: usize = 11;
const FIRST_RGB_INDEX: usize = 12;

#[test]
fn every_captured_frame_row_fits_the_key_grid() {
    let grid = BLADE_18.keys;
    let frame_rows: Vec<_> = common::captured_requests()
        .into_iter()
        .filter(|request| request.bytes[6..8] == FRAME_ROW)
        .collect();

    assert!(!frame_rows.is_empty());
    for frame_row in frame_rows {
        let bytes = frame_row.bytes;
        assert!(bytes[ROW_INDEX] < grid.rows, "{}", frame_row.fixture);
        assert_eq!(bytes[LAST_COLUMN_INDEX], grid.columns - 1);
    }
}

#[test]
fn positions_without_a_key_are_always_black() {
    let grid = BLADE_18.keys;
    for frame_row in common::captured_requests()
        .into_iter()
        .filter(|request| request.bytes[6..8] == FRAME_ROW)
    {
        let row = frame_row.bytes[ROW_INDEX];
        for column in (0..grid.columns).filter(|&column| !grid.has_key(row, column)) {
            let rgb = FIRST_RGB_INDEX + 3 * usize::from(column);
            assert_eq!(
                frame_row.bytes[rgb..rgb + 3],
                [0, 0, 0],
                "{} row {row} column {column}",
                frame_row.fixture
            );
        }
    }
}

#[test]
fn the_blade_18_has_7_rows_of_19_columns() {
    assert_eq!((BLADE_18.keys.rows, BLADE_18.keys.columns), (7, 19));
}

#[test]
fn the_power_key_is_read_off_another_interface_than_the_controller() {
    let key = BLADE_18.power_key.unwrap();

    assert_ne!(key.interface_number, BLADE_18.usb.interface_number);
}

#[test]
fn the_power_key_is_held_in_the_report_read_on_the_laptop() {
    let key = BLADE_18.power_key.unwrap();
    let mut report = [0; 16];
    report[..3].copy_from_slice(&[0x04, 0x0A, 0xD3]);

    assert_eq!(key.held(&report), Some(true));
}

#[test]
fn the_vapor_light_is_not_a_keyboard_brightness_led() {
    let light = BLADE_18.vapor_light.unwrap();

    assert_eq!(light.led, 0x26);
    assert!(!BLADE_18.brightness_leds.contains(&light.led));
}

#[test]
fn the_blade_18_mode_bytes_are_the_captured_ones() {
    let bytes = BLADE_18.mode_bytes;

    assert_eq!(
        (
            bytes.silent,
            bytes.balanced,
            bytes.performance,
            BLADE_18.custom.mode
        ),
        (0x05, 0x00, 0x02, 0x04)
    );
}

#[test]
fn the_blade_18_logo_is_in_storage_0() {
    assert_eq!(BLADE_18.logo.unwrap().storage, 0x00);
}

#[test]
fn the_blade_18_profile_is_valid() {
    assert!(BLADE_18.is_valid());
}

#[test]
fn two_presets_with_one_mode_byte_are_not_valid() {
    let mut model = BLADE_18;
    model.mode_bytes.silent = model.mode_bytes.balanced;

    assert!(!model.is_valid());
}

#[test]
fn a_custom_byte_of_a_preset_is_not_valid() {
    let mut model = BLADE_18;
    model.custom.mode = model.mode_bytes.performance;

    assert!(!model.is_valid());
}

#[test]
fn battery_presets_without_balanced_are_not_valid() {
    let model = ModelProfile {
        battery_presets: &[PerfMode::Silent],
        ..BLADE_18
    };

    assert!(!model.is_valid());
}

#[test]
fn a_profile_with_no_firmware_listed_is_not_valid() {
    let model = ModelProfile {
        firmware: &[],
        ..BLADE_18
    };

    assert!(!model.is_valid());
}

#[test]
fn a_battery_preset_the_model_lacks_is_not_valid() {
    let model = ModelProfile {
        presets: &[PerfMode::Balanced, PerfMode::Performance],
        ..BLADE_18
    };

    assert!(!model.is_valid());
}
