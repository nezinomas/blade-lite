//! The active displays, from `QueryDisplayConfig`, and their modes, from
//! `EnumDisplaySettingsW`. Only `set_mode` changes a display, through
//! `ChangeDisplaySettingsExW`.

use std::io;
use std::ptr;

use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows_sys::Win32::Graphics::Gdi::{
    CDS_TYPE, CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE,
    DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH,
    ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE, ENUM_REGISTRY_SETTINGS,
    EnumDisplaySettingsW,
};

use crate::{Display, DisplayApi, DisplayMode, ModeLife, OutputTechnology, wide, windows_error};

/// How many times the query is tried when the displays change under it.
const QUERY_TRIES: usize = 3;

/// A display's EDID maker letters and product code, such as `(*b"BOE",
/// 0x0D54)`.
pub type PanelId = ([u8; 3], u16);

/// The displays Windows has turned on.
pub struct WindowsDisplays {
    /// The laptop's own panels by EDID id: a display with one of them is
    /// the internal panel however Windows says it is connected.
    pub panels: &'static [PanelId],
}

impl DisplayApi for WindowsDisplays {
    fn saved_mode(&self, name: &str) -> Option<DisplayMode> {
        mode(&wide(name), ENUM_REGISTRY_SETTINGS)
    }

    fn displays(&self) -> io::Result<Vec<Display>> {
        active_paths()?
            .iter()
            .map(|path| display(path, self.panels))
            .collect()
    }

    fn set_mode(&mut self, name: &str, mode: DisplayMode, life: ModeLife) -> io::Result<()> {
        let device = wide(name);
        // Start from the current settings, so colour depth and position stay.
        let mut devmode =
            devmode(&device, ENUM_CURRENT_SETTINGS).ok_or_else(io::Error::last_os_error)?;
        devmode.dmPelsWidth = mode.width;
        devmode.dmPelsHeight = mode.height;
        devmode.dmDisplayFrequency = mode.hz;
        devmode.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        // SAFETY: the name ends in a nul, and the mode is a whole DEVMODEW
        // whose `dmSize` Windows filled in.
        let result = unsafe {
            ChangeDisplaySettingsExW(
                device.as_ptr(),
                &raw const devmode,
                ptr::null_mut(),
                change_flags(life),
                ptr::null(),
            )
        };
        change_result(result)
    }
}

/// A saved mode goes to the user's settings too; any other changes only
/// the screen now.
fn change_flags(life: ModeLife) -> CDS_TYPE {
    match life {
        ModeLife::Saved => CDS_UPDATEREGISTRY,
        ModeLife::UntilRestart => 0,
    }
}

/// `Ok` only when Windows switched the mode at once.
fn change_result(result: DISP_CHANGE) -> io::Result<()> {
    if result == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "ChangeDisplaySettingsExW returned {result}"
        )))
    }
}

/// One path per active display: where it is drawn from and how it is wired.
fn active_paths() -> io::Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
    let mut status = ERROR_SUCCESS;
    for _ in 0..QUERY_TRIES {
        let (mut path_count, mut mode_count) = (0_u32, 0_u32);
        // SAFETY: Windows writes only the two counts.
        status = unsafe {
            GetDisplayConfigBufferSizes(
                QDC_ONLY_ACTIVE_PATHS,
                &raw mut path_count,
                &raw mut mode_count,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(windows_error(status));
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        // SAFETY: the counts give the lengths of the two buffers, and
        // Windows writes no more than that and lowers the counts to match.
        status = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &raw mut path_count,
                paths.as_mut_ptr(),
                &raw mut mode_count,
                modes.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        if status == ERROR_SUCCESS {
            paths.truncate(path_count as usize);
            return Ok(paths);
        }
        // A display came or went between the two calls: ask again.
        if status != ERROR_INSUFFICIENT_BUFFER {
            break;
        }
    }
    Err(windows_error(status))
}

fn display(path: &DISPLAYCONFIG_PATH_INFO, panels: &[PanelId]) -> io::Result<Display> {
    let name = source_name(path)?;
    let device = wide(&name);
    Ok(Display {
        technology: panel_technology(path.targetInfo.outputTechnology, target_edid(path), panels),
        current: mode(&device, ENUM_CURRENT_SETTINGS).ok_or_else(io::Error::last_os_error)?,
        modes: (0..).map_while(|index| mode(&device, index)).collect(),
        name,
    })
}

/// The GDI name of the source a path is drawn from, such as `\\.\DISPLAY1`.
fn source_name(path: &DISPLAYCONFIG_PATH_INFO) -> io::Result<String> {
    let mut request = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
    request.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
    request.header.size =
        u32::try_from(size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>()).unwrap_or(u32::MAX);
    request.header.adapterId = path.sourceInfo.adapterId;
    request.header.id = path.sourceInfo.id;
    // SAFETY: the header's size is that of the whole request, which is the
    // type the header asks for, so Windows writes inside it.
    let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut request.header) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    let name = &request.viewGdiDeviceName;
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..end]))
}

/// One mode of a display, or `None` past the last one.
fn mode(device: &[u16], index: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DisplayMode> {
    devmode(device, index).map(|devmode| DisplayMode {
        width: devmode.dmPelsWidth,
        height: devmode.dmPelsHeight,
        hz: devmode.dmDisplayFrequency,
    })
}

/// One mode of a display as Windows describes it, or `None` past the last one.
fn devmode(device: &[u16], index: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
    let mut devmode = DEVMODEW {
        dmSize: u16::try_from(size_of::<DEVMODEW>()).unwrap_or(u16::MAX),
        ..DEVMODEW::default()
    };
    // SAFETY: the name ends in a nul, and `dmSize` tells Windows how much of
    // the struct it may write.
    let found = unsafe { EnumDisplaySettingsW(device.as_ptr(), index, &raw mut devmode) };
    (found != 0).then_some(devmode)
}

/// The panel as one of the embedded kinds, or by its EDID maker and
/// product code: in NVIDIA-only mode the NVIDIA driver reports the
/// laptop's panel as a `DisplayPort` monitor, in both the path's and the
/// target's base output technology (ticket 282, read 2026-11-18).
fn panel_technology(
    raw: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY,
    edid: Option<PanelId>,
    panels: &[PanelId],
) -> OutputTechnology {
    if edid.is_some_and(|edid| panels.contains(&edid)) {
        OutputTechnology::Internal
    } else {
        technology(raw)
    }
}

/// The EDID maker and product code of a path's target, if Windows says
/// they are valid.
fn target_edid(path: &DISPLAYCONFIG_PATH_INFO) -> Option<PanelId> {
    let mut request = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
    request.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
    request.header.size =
        u32::try_from(size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>()).unwrap_or(u32::MAX);
    request.header.adapterId = path.targetInfo.adapterId;
    request.header.id = path.targetInfo.id;
    // SAFETY: the header's size is that of the whole request, which is the
    // type the header asks for, so Windows writes inside it.
    let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut request.header) };
    // SAFETY: both views of the flags are a plain `u32`.
    let flags = unsafe { request.flags.Anonymous.value };
    if status != 0 || flags & EDID_IDS_VALID == 0 {
        return None;
    }
    Some((
        maker_letters(request.edidManufactureId)?,
        request.edidProductCodeId,
    ))
}

/// `edidIdsValid`, the third bit of `DISPLAYCONFIG_TARGET_DEVICE_NAME_FLAGS`.
const EDID_IDS_VALID: u32 = 0b100;

/// The three letters of an EDID maker id as Windows gives it: the EDID's
/// two bytes read low first, so swapped, then three 5-bit letters, 1 for
/// A. `None` if one is not a letter.
fn maker_letters(raw: u16) -> Option<[u8; 3]> {
    let id = raw.swap_bytes();
    // EDID keeps the top bit 0.
    if id & 0x8000 != 0 {
        return None;
    }
    let letter = |shift: u16| {
        let code = u8::try_from((id >> shift) & 0x1F).ok()?;
        (1..=26).contains(&code).then(|| b'A' + code - 1)
    };
    Some([letter(10)?, letter(5)?, letter(0)?])
}

/// A laptop panel shows up as one of the embedded kinds.
fn technology(raw: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY) -> OutputTechnology {
    match raw {
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
        | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
        | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED
        | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS => OutputTechnology::Internal,
        _ => OutputTechnology::External,
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Devices::Display::{
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI,
    };

    use super::*;

    #[test]
    fn embedded_outputs_are_the_internal_panel() {
        for raw in [
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
        ] {
            assert_eq!(technology(raw), OutputTechnology::Internal, "{raw}");
        }
    }

    #[test]
    fn only_an_immediate_switch_is_success() {
        use windows_sys::Win32::Graphics::Gdi::{DISP_CHANGE_BADMODE, DISP_CHANGE_RESTART};

        assert!(change_result(DISP_CHANGE_SUCCESSFUL).is_ok());
        assert!(change_result(DISP_CHANGE_RESTART).is_err());
        assert!(change_result(DISP_CHANGE_BADMODE).is_err());
    }

    #[test]
    fn only_a_saved_mode_goes_to_the_registry() {
        assert_eq!(change_flags(ModeLife::Saved), CDS_UPDATEREGISTRY);
        assert_eq!(change_flags(ModeLife::UntilRestart), 0);
    }

    /// The laptop's panel as Windows read it in NVIDIA-only mode
    /// (2026-11-18): maker 0xE509, product 0x0D54.
    const BOE_0D54: ([u8; 3], u16) = (*b"BOE", 0x0D54);

    #[test]
    fn the_edid_maker_reads_as_three_letters() {
        assert_eq!(maker_letters(0xE509), Some(*b"BOE"));
    }

    #[test]
    fn an_edid_maker_out_of_the_letters_is_none() {
        assert_eq!(maker_letters(0), None);
        assert_eq!(maker_letters(0xFFFF), None);
        // BOE with the reserved top bit set.
        assert_eq!(maker_letters(0xE589), None);
    }

    /// Prints what Windows says of each active display's target: the
    /// call's status, the flags and the raw EDID ids (laptop check 75).
    #[test]
    #[ignore = "reads the laptop's displays"]
    #[allow(clippy::print_stderr, reason = "a probe for a person to read")]
    fn each_target_s_edid_ids_are_printed() {
        for path in active_paths().unwrap() {
            let mut request = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            request.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            request.header.size =
                u32::try_from(size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>()).unwrap();
            request.header.adapterId = path.targetInfo.adapterId;
            request.header.id = path.targetInfo.id;
            // SAFETY: as in `target_edid`.
            let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut request.header) };
            // SAFETY: as in `target_edid`.
            let flags = unsafe { request.flags.Anonymous.value };
            eprintln!(
                "technology {} status {status} flags {flags:#x} maker {:#06x} product {:#06x} read as {:?}",
                path.targetInfo.outputTechnology,
                request.edidManufactureId,
                request.edidProductCodeId,
                target_edid(&path),
            );
        }
    }

    #[test]
    fn a_known_panel_reported_as_external_is_the_internal_panel() {
        let raw = DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL;

        assert_eq!(
            panel_technology(raw, Some(BOE_0D54), &[BOE_0D54]),
            OutputTechnology::Internal
        );
    }

    #[test]
    fn an_external_display_with_another_or_no_edid_stays_external() {
        let raw = DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI;

        for edid in [Some((*b"BOE", 0x0D55)), Some((*b"DEL", 0x0D54)), None] {
            assert_eq!(
                panel_technology(raw, edid, &[BOE_0D54]),
                OutputTechnology::External,
                "{edid:?}"
            );
        }
    }

    #[test]
    fn an_embedded_output_is_internal_with_no_edid() {
        assert_eq!(
            panel_technology(DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, None, &[]),
            OutputTechnology::Internal
        );
    }

    #[test]
    fn cabled_outputs_are_external() {
        for raw in [
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL,
        ] {
            assert_eq!(technology(raw), OutputTechnology::External, "{raw}");
        }
    }
}
