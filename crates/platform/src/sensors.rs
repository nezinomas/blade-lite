//! The sensors Windows offers: CPU time from `GetSystemTimes`, the battery
//! from `GetSystemPowerStatus`, `CallNtPowerInformation` and its read-only
//! battery queries, and the NVIDIA
//! GPU from its device power state and NVML.

use std::io;
use std::ptr;
use std::time::{Duration, Instant};

use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_GETIDLIST_FILTER_CLASS, CM_GETIDLIST_FILTER_PRESENT,
    CM_Get_DevNode_PropertyW, CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW,
    CM_Get_Device_Interface_List_SizeW, CM_Get_Device_Interface_ListW, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CONFIGRET, CR_SUCCESS,
};
use windows_sys::Win32::Devices::Properties::DEVPKEY_Device_PowerData;
use windows_sys::Win32::Foundation::{FILETIME, GENERIC_READ, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Power::{
    BATTERY_INFORMATION, BATTERY_QUERY_INFORMATION, BATTERY_TAG_INVALID, BATTERY_UNKNOWN_CAPACITY,
    BATTERY_UNKNOWN_RATE, BatteryInformation, CM_POWER_DATA, CallNtPowerInformation,
    DEVICE_POWER_STATE, GUID_DEVICE_BATTERY, GetSystemPowerStatus, IOCTL_BATTERY_QUERY_INFORMATION,
    IOCTL_BATTERY_QUERY_TAG, PowerDeviceD0, PowerDeviceD1, PowerDeviceD2, PowerDeviceD3,
    SYSTEM_BATTERY_STATE, SYSTEM_POWER_STATUS, SystemBatteryState,
};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use crate::instance::Handle;
use crate::{BatteryHealth, CpuTimes, GpuReading, PowerReading, Sensors, wide};

/// The display adapter class, as `CM_Get_Device_ID_ListW` takes it.
const DISPLAY_CLASS: &str = "{4d36e968-e325-11ce-bfc1-08002be10318}";
/// How an NVIDIA PCI device ID starts.
const NVIDIA_ID: &str = r"PCI\VEN_10DE";
/// What `GetSystemPowerStatus` says when it does not know the charge.
const UNKNOWN_PERCENT: u8 = 255;
/// The highest health believed: a full charge well over the design
/// capacity is a bad reading.
const MAX_HEALTH_PERCENT: u32 = 150;
/// How long NVML stays closed after a close. A start and a close each wake
/// a sleeping GPU for about 1 s, and the GPU reads awake in that time, so
/// a start right after a close could feed a loop of wakes (ticket 185).
const NVML_START_PAUSE: Duration = Duration::from_secs(30);

/// The sensors of this PC. NVML is loaded on the first GPU read, and kept:
/// its start and its close each wake a sleeping GPU (probe 2026-11-13).
#[derive(Default)]
pub struct WindowsSensors {
    nvml: Option<Nvml>,
    /// When NVML was last closed after a failed read, or failed to start.
    nvml_closed: Option<Instant>,
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
        let state = battery_state()?;
        Ok(PowerReading {
            battery_percent: battery_percent(status.BatteryLifePercent),
            battery_rate_mw: battery_rate(&state),
            charging: charging_flag(&state),
        })
    }

    fn battery_health(&mut self) -> io::Result<BatteryHealth> {
        let Some(path) = battery_path()? else {
            return Ok(BatteryHealth::default());
        };
        Ok(battery_information(&path)?
            .as_ref()
            .map(battery_health)
            .unwrap_or_default())
    }

    /// A missing adapter or a power state not known is an error: only a
    /// real sleep state counts as asleep, so the fan guard can tell a GPU
    /// that cools from one in trouble (ticket 139).
    fn gpu_awake(&mut self) -> io::Result<bool> {
        let Some(id) = nvidia_display()? else {
            return Err(io::Error::other("no NVIDIA display adapter is present"));
        };
        is_awake(device_power_state(&id)?)
    }

    /// A failed read drops NVML, so a later read starts it again: an old
    /// handle can keep failing after a driver reset (ticket 156). But only
    /// while the GPU still reads awake, since the close wakes a sleeping
    /// one, and the start waits [`NVML_START_PAUSE`] after it (ticket 185).
    fn gpu(&mut self) -> io::Result<GpuReading> {
        let read = self.read_gpu();
        if read.is_err() && close_nvml_after_failure(&self.gpu_awake()) {
            self.nvml = None;
            self.nvml_closed = Some(Instant::now());
        }
        read
    }
}

impl WindowsSensors {
    fn read_gpu(&mut self) -> io::Result<GpuReading> {
        if self.nvml.is_none() {
            if !may_start_nvml(self.nvml_closed, Instant::now()) {
                return Err(io::Error::other(
                    "NVML waits after its close, which may have woken the GPU",
                ));
            }
            // A failed start counts as a close: it may have woken the GPU.
            self.nvml = Some(Nvml::init().map_err(|error| {
                self.nvml_closed = Some(Instant::now());
                io::Error::other(error)
            })?);
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

/// Whether NVML may start: never, or [`NVML_START_PAUSE`] since its last
/// close.
fn may_start_nvml(closed: Option<Instant>, now: Instant) -> bool {
    closed.is_none_or(|closed| now.saturating_duration_since(closed) >= NVML_START_PAUSE)
}

/// Whether to close NVML after a failed read: only on a GPU that reads
/// awake. A GPU asleep, or a power state not known, keeps it, as the close
/// would wake a sleeping GPU.
fn close_nvml_after_failure(awake: &io::Result<bool>) -> bool {
    matches!(awake, Ok(true))
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

/// Whether the battery charges now. On AC with the charge limit reached,
/// Windows says it does not. `None` with no battery, or off AC (a reading
/// on battery says nothing about charging on AC).
fn charging_flag(state: &SYSTEM_BATTERY_STATE) -> Option<bool> {
    (state.BatteryPresent && state.AcOnLine).then_some(state.Charging)
}

/// The health from the battery's static data. A capacity Windows does not
/// know, of 0, or a full charge over [`MAX_HEALTH_PERCENT`] of the design
/// gives no percent; a cycle count of 0 means the battery counts none.
fn battery_health(info: &BATTERY_INFORMATION) -> BatteryHealth {
    BatteryHealth {
        percent: health_percent(info.FullChargedCapacity, info.DesignedCapacity),
        cycles: (info.CycleCount != 0).then_some(info.CycleCount),
    }
}

fn health_percent(full: u32, design: u32) -> Option<u32> {
    let known = |capacity| capacity != 0 && capacity != BATTERY_UNKNOWN_CAPACITY;
    if !known(full) || !known(design) {
        return None;
    }
    let (full, design) = (u64::from(full), u64::from(design));
    let percent = (full * 100 + design / 2) / design;
    u32::try_from(percent)
        .ok()
        .filter(|&percent| percent <= MAX_HEALTH_PERCENT)
}

/// The path of the first battery, or `None` with no battery.
fn battery_path() -> io::Result<Option<String>> {
    let mut length = 0;
    // SAFETY: Windows writes only the length.
    check(unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &raw mut length,
            &GUID_DEVICE_BATTERY,
            ptr::null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    })?;
    let mut buffer = vec![0_u16; length as usize];
    // SAFETY: the buffer is `length` wide characters long.
    check(unsafe {
        CM_Get_Device_Interface_ListW(
            &GUID_DEVICE_BATTERY,
            ptr::null(),
            buffer.as_mut_ptr(),
            length,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    })?;
    Ok(split_ids(&buffer).into_iter().next())
}

/// The battery's static data, through its read-only queries, or `None`
/// with no battery in.
fn battery_information(path: &str) -> io::Result<Option<BATTERY_INFORMATION>> {
    let path = wide(path);
    // SAFETY: the path ends in a nul; the battery is opened to read only.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let battery = Handle::new(raw)?;
    // Wait 0 ms: the tag of the battery in now, or none.
    let tag: u32 = ioctl(&battery, IOCTL_BATTERY_QUERY_TAG, &0_u32)?;
    if tag == BATTERY_TAG_INVALID {
        return Ok(None);
    }
    let query = BATTERY_QUERY_INFORMATION {
        BatteryTag: tag,
        InformationLevel: BatteryInformation,
        AtRate: 0,
    };
    ioctl(&battery, IOCTL_BATTERY_QUERY_INFORMATION, &query).map(Some)
}

/// Sends a query to a device and reads back a whole `Out`.
fn ioctl<In, Out: Default>(device: &Handle, code: u32, input: &In) -> io::Result<Out> {
    let mut output = Out::default();
    let mut returned = 0;
    let size = |bytes: usize| u32::try_from(bytes).unwrap_or(u32::MAX);
    // SAFETY: the input and output are whole values of their sizes.
    let ok = unsafe {
        DeviceIoControl(
            device.0,
            code,
            ptr::from_ref(input).cast(),
            size(size_of::<In>()),
            ptr::from_mut(&mut output).cast(),
            size(size_of::<Out>()),
            &raw mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let expected = size(size_of::<Out>());
    if returned != expected {
        return Err(io::Error::other(format!(
            "device query returned {returned} of {expected} bytes"
        )));
    }
    Ok(output)
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

/// D0 is awake, D1 to D3 are the sleep states; any other state is not
/// known.
fn is_awake(state: DEVICE_POWER_STATE) -> io::Result<bool> {
    if state == PowerDeviceD0 {
        Ok(true)
    } else if [PowerDeviceD1, PowerDeviceD2, PowerDeviceD3].contains(&state) {
        Ok(false)
    } else {
        Err(io::Error::other(format!(
            "GPU power state {state} not known"
        )))
    }
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
    fn the_charging_flag_needs_a_battery_and_ac() {
        let on_ac = SYSTEM_BATTERY_STATE {
            BatteryPresent: true,
            AcOnLine: true,
            ..Default::default()
        };
        let charging = SYSTEM_BATTERY_STATE {
            Charging: true,
            ..on_ac
        };
        let absent = SYSTEM_BATTERY_STATE {
            BatteryPresent: false,
            ..charging
        };
        let on_battery = SYSTEM_BATTERY_STATE {
            AcOnLine: false,
            ..on_ac
        };

        assert_eq!(charging_flag(&charging), Some(true));
        assert_eq!(charging_flag(&on_ac), Some(false));
        assert_eq!(charging_flag(&absent), None);
        assert_eq!(charging_flag(&on_battery), None);
    }

    fn information(design: u32, full: u32, cycles: u32) -> BATTERY_INFORMATION {
        BATTERY_INFORMATION {
            DesignedCapacity: design,
            FullChargedCapacity: full,
            CycleCount: cycles,
            ..Default::default()
        }
    }

    #[test]
    fn health_is_the_full_charge_against_the_design_capacity() {
        assert_eq!(
            // This laptop's battery, 2026-11-10.
            battery_health(&information(97_094, 96_363, 86)),
            BatteryHealth {
                percent: Some(99),
                cycles: Some(86)
            }
        );
        // Rounded to the nearest percent, and a new battery may hold more.
        assert_eq!(battery_health(&information(1000, 994, 1)).percent, Some(99));
        assert_eq!(
            battery_health(&information(1000, 1020, 1)).percent,
            Some(102)
        );
    }

    #[test]
    fn an_unknown_or_zero_capacity_gives_no_health() {
        for (design, full) in [
            (0, 900),
            (BATTERY_UNKNOWN_CAPACITY, 900),
            (1000, 0),
            (1000, BATTERY_UNKNOWN_CAPACITY),
        ] {
            let health = battery_health(&information(design, full, 5));
            assert_eq!(health.percent, None, "{design}, {full}");
        }
    }

    #[test]
    fn health_well_over_the_design_capacity_is_not_believed() {
        assert_eq!(
            battery_health(&information(1000, 1500, 1)).percent,
            Some(150)
        );
        assert_eq!(battery_health(&information(1000, 1506, 1)).percent, None);
    }

    #[test]
    fn no_cycle_counter_gives_no_cycles() {
        assert_eq!(battery_health(&information(1000, 900, 0)).cycles, None);
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
    fn nvml_starts_again_only_after_a_pause_since_its_close() {
        let closed = Instant::now();

        assert!(may_start_nvml(None, closed));
        assert!(!may_start_nvml(Some(closed), closed));
        assert!(!may_start_nvml(
            Some(closed),
            closed + NVML_START_PAUSE.saturating_sub(Duration::from_millis(1))
        ));
        assert!(may_start_nvml(Some(closed), closed + NVML_START_PAUSE));
    }

    #[test]
    fn a_failed_read_closes_nvml_only_while_the_gpu_is_awake() {
        assert!(close_nvml_after_failure(&Ok(true)));
        assert!(!close_nvml_after_failure(&Ok(false)));
        assert!(!close_nvml_after_failure(&Err(io::Error::other("unknown"))));
    }

    #[test]
    fn only_d0_is_awake() {
        assert!(is_awake(PowerDeviceD0).unwrap());
        assert!(!is_awake(PowerDeviceD3).unwrap());
        assert!(is_awake(PowerDeviceUnspecified).is_err());
    }
}
