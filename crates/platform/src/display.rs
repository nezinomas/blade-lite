//! The active displays, from `QueryDisplayConfig`, and their modes, from
//! `EnumDisplaySettingsW`. Only `set_mode` changes a display, through
//! `ChangeDisplaySettingsExW`.

use std::io;
use std::ptr;

use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows_sys::Win32::Graphics::Gdi::{
    CDS_TYPE, CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE,
    DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH,
    ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsW,
};

use crate::{Display, DisplayApi, DisplayMode, ModeLife, OutputTechnology, wide, windows_error};

/// How many times the query is tried when the displays change under it.
const QUERY_TRIES: usize = 3;

/// The displays Windows has turned on.
pub struct WindowsDisplays;

impl DisplayApi for WindowsDisplays {
    fn displays(&self) -> io::Result<Vec<Display>> {
        active_paths()?.iter().map(display).collect()
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

fn display(path: &DISPLAYCONFIG_PATH_INFO) -> io::Result<Display> {
    let name = source_name(path)?;
    let device = wide(&name);
    Ok(Display {
        technology: technology(path.targetInfo.outputTechnology),
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
