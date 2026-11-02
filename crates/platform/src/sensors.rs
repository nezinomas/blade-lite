//! The sensors Windows offers: CPU time from `GetSystemTimes`, the battery
//! from `GetSystemPowerStatus` and `CallNtPowerInformation`, and the NVIDIA
//! GPU from its device power state and NVML.

use std::io;
use std::ptr;

use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_CLASS, CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW,
    CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CONFIGRET, CR_SUCCESS,
};
use windows_sys::Win32::Devices::Properties::DEVPKEY_Device_PowerData;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Power::{
    BATTERY_UNKNOWN_RATE, CM_POWER_DATA, CallNtPowerInformation, DEVICE_POWER_STATE,
    GetSystemPowerStatus, PowerDeviceD0, SYSTEM_BATTERY_STATE, SYSTEM_POWER_STATUS,
    SystemBatteryState,
};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::{CpuTimes, GpuReading, PowerReading, Sensors, wide};

/// The display adapter class, as `CM_Get_Device_ID_ListW` takes it.
const DISPLAY_CLASS: &str = "{4d36e968-e325-11ce-bfc1-08002be10318}";
/// How an NVIDIA PCI device ID starts.
const NVIDIA_ID: &str = r"PCI\VEN_10DE";
/// What `GetSystemPowerStatus` says when it does not know the charge.
const UNKNOWN_PERCENT: u8 = 255;

/// The sensors of this PC. NVML is loaded on the first GPU read.
#[derive(Default)]
pub struct WindowsSensors {
    nvml: Option<Nvml>,
}

impl Sensors for WindowsSensors {
    fn cpu_times(&mut self) -> io::Result<CpuTimes> {
        let (mut idle, mut kernel, mut user) = Default::default();
        // SAFETY: Windows writes only the three times.
        let ok = unsafe { GetSystemTimes(&raw mut idle, &raw mut kernel, &raw mut user) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cpu_times(idle, kernel, user))
    }

    fn power(&mut self) -> io::Result<PowerReading> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: Windows writes only the status.
        if unsafe { GetSystemPowerStatus(&raw mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(PowerReading {
            battery_percent: battery_percent(status.BatteryLifePercent),
            battery_rate_mw: battery_rate(&battery_state()?),
        })
    }

    fn gpu_awake(&mut self) -> io::Result<bool> {
        let Some(id) = nvidia_display()? else {
            return Ok(false);
        };
        Ok(is_awake(device_power_state(&id)?))
    }

    fn gpu(&mut self) -> io::Result<GpuReading> {
        if self.nvml.is_none() {
            self.nvml = Some(Nvml::init().map_err(io::Error::other)?);
        }
        let nvml = self.nvml.as_ref().ok_or(io::ErrorKind::NotFound)?;
        let device = nvml.device_by_index(0).map_err(io::Error::other)?;
        Ok(GpuReading {
            celsius: device
                .temperature(TemperatureSensor::Gpu)
                .map_err(io::Error::other)?,
            load_percent: device.utilization_rates().map_err(io::Error::other)?.gpu,
        })
    }
}

/// Total time is kernel plus user, since kernel time counts idle time too.
fn cpu_times(idle: FILETIME, kernel: FILETIME, user: FILETIME) -> CpuTimes {
    CpuTimes {
        idle: ticks(idle),
        total: ticks(kernel) + ticks(user),
    }
}

fn ticks(time: FILETIME) -> u64 {
    u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
}

fn battery_percent(raw: u8) -> Option<u8> {
    (raw != UNKNOWN_PERCENT).then_some(raw)
}

/// The battery's state from the power manager.
fn battery_state() -> io::Result<SYSTEM_BATTERY_STATE> {
    let mut state = SYSTEM_BATTERY_STATE::default();
    let size = u32::try_from(size_of::<SYSTEM_BATTERY_STATE>()).unwrap_or(u32::MAX);
    // SAFETY: the output buffer is the state, and `size` is its size.
    let status = unsafe {
        CallNtPowerInformation(
            SystemBatteryState,
            ptr::null(),
            0,
            ptr::from_mut(&mut state).cast(),
            size,
        )
    };
    if status != 0 {
        return Err(io::Error::other(format!(
            "CallNtPowerInformation returned {status:#x}"
        )));
    }
    Ok(state)
}

/// The rate in milliwatts, below zero while draining. Windows stores the
/// signed rate in an unsigned field.
fn battery_rate(state: &SYSTEM_BATTERY_STATE) -> Option<i32> {
    if !state.BatteryPresent || state.Rate == BATTERY_UNKNOWN_RATE {
        return None;
    }
    Some(i32::from_ne_bytes(state.Rate.to_ne_bytes()))
}

/// The device ID of the NVIDIA display adapter, if there is one.
fn nvidia_display() -> io::Result<Option<String>> {
    let ids = display_adapter_ids()?;
    Ok(ids.into_iter().find(|id| id.starts_with(NVIDIA_ID)))
}

/// The IDs of the display adapters that are present.
fn display_adapter_ids() -> io::Result<Vec<String>> {
    let filter = wide(DISPLAY_CLASS);
    let flags = CM_GETIDLIST_FILTER_CLASS | CM_GETIDLIST_FILTER_PRESENT;
    let mut length = 0;
    // SAFETY: the filter ends in a nul; Windows writes only the length.
    check(unsafe { CM_Get_Device_ID_List_SizeW(&raw mut length, filter.as_ptr(), flags) })?;
    let mut buffer = vec![0_u16; length as usize];
    // SAFETY: the buffer is `length` wide characters long.
    check(unsafe { CM_Get_Device_ID_ListW(filter.as_ptr(), buffer.as_mut_ptr(), length, flags) })?;
    Ok(split_ids(&buffer))
}

/// The IDs in a list of nul-ended strings that ends in an empty one.
fn split_ids(buffer: &[u16]) -> Vec<String> {
    buffer
        .split(|&c| c == 0)
        .take_while(|id| !id.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// The power state Windows last saw the device in. Reading it does not wake
/// the device.
fn device_power_state(id: &str) -> io::Result<DEVICE_POWER_STATE> {
    let id = wide(id);
    let mut node = 0;
    // SAFETY: the ID ends in a nul; Windows writes only the node.
    check(unsafe { CM_Locate_DevNodeW(&raw mut node, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) })?;
    let mut data = CM_POWER_DATA::default();
    let mut size = u32::try_from(size_of::<CM_POWER_DATA>()).unwrap_or(u32::MAX);
    let mut kind = 0;
    // SAFETY: the buffer is the power data, and `size` is its size.
    check(unsafe {
        CM_Get_DevNode_PropertyW(
            node,
            &DEVPKEY_Device_PowerData,
            &raw mut kind,
            ptr::from_mut(&mut data).cast(),
            &raw mut size,
            0,
        )
    })?;
    Ok(data.PD_MostRecentPowerState)
}

/// Only D0 is awake. D1 to D3 are the sleep states.
fn is_awake(state: DEVICE_POWER_STATE) -> bool {
    state == PowerDeviceD0
}

fn check(result: CONFIGRET) -> io::Result<()> {
    if result == CR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Configuration Manager returned {result}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Power::{PowerDeviceD3, PowerDeviceUnspecified};

    use super::*;

    const fn filetime(high: u32, low: u32) -> FILETIME {
        FILETIME {
            dwLowDateTime: low,
            dwHighDateTime: high,
        }
    }

    #[test]
    fn total_cpu_time_is_kernel_plus_user() {
        let times = cpu_times(filetime(0, 30), filetime(1, 50), filetime(0, 20));

        assert_eq!(
            times,
            CpuTimes {
                idle: 30,
                total: (1 << 32) + 70
            }
        );
    }

    #[test]
    fn an_unknown_charge_is_none() {
        assert_eq!(battery_percent(80), Some(80));
        assert_eq!(battery_percent(UNKNOWN_PERCENT), None);
    }

    #[test]
    fn a_draining_rate_is_below_zero() {
        let state = SYSTEM_BATTERY_STATE {
            BatteryPresent: true,
            Rate: (-45_000_i32).cast_unsigned(),
            ..Default::default()
        };

        assert_eq!(battery_rate(&state), Some(-45_000));
    }

    #[test]
    fn no_battery_or_an_unknown_rate_is_none() {
        let unknown = SYSTEM_BATTERY_STATE {
            BatteryPresent: true,
            Rate: BATTERY_UNKNOWN_RATE,
            ..Default::default()
        };
        let absent = SYSTEM_BATTERY_STATE {
            Rate: 1000,
            ..Default::default()
        };

        assert_eq!(battery_rate(&unknown), None);
        assert_eq!(battery_rate(&absent), None);
    }

    #[test]
    fn the_id_list_is_split_at_each_nul() {
        let buffer: Vec<u16> = "PCI\\VEN_8086\0PCI\\VEN_10DE&DEV_2900\0\0"
            .encode_utf16()
            .collect();

        assert_eq!(
            split_ids(&buffer),
            [r"PCI\VEN_8086", r"PCI\VEN_10DE&DEV_2900"]
        );
    }

    #[test]
    fn only_d0_is_awake() {
        assert!(is_awake(PowerDeviceD0));
        assert!(!is_awake(PowerDeviceD3));
        assert!(!is_awake(PowerDeviceUnspecified));
    }
}
