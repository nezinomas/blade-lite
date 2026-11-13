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
/// How long NVML stays closed after a close or a failed start. A start and
/// a close each wake a sleeping GPU for about 1 s, and the GPU reads awake
/// in that time, so a start right after a close could feed a loop of wakes
/// (ticket 185). The GPU is unread for this long after a close, so keep it
/// well under the heat warning's 30 s gap (`HEAT_GAP` in `ui`), after
/// which a reading no longer counts as recent.
const NVML_START_PAUSE: Duration = Duration::from_secs(5);
/// How close a failed read must come after another one to close NVML. One
/// failure may be a GPU on its way to sleep; a stale handle fails at every
/// read, and the slowest reads come each 10 s (the heat warning's).
const NVML_FAIL_WINDOW: Duration = Duration::from_secs(15);

/// The sensors of this PC. NVML is loaded on the first GPU read, and kept:
/// its start and its close each wake a sleeping GPU (probe 2026-11-13).
#[derive(Default)]
pub struct WindowsSensors {
    nvml: NvmlSlot<Nvml>,
}

/// NVML, and what decides when it starts and closes (see
/// [`NvmlSlot::read`]).
struct NvmlSlot<N> {
    nvml: Option<N>,
    /// When NVML was last closed, or failed to start.
    closed: Option<Instant>,
    /// When the last read failed, if no good read came after it and the
    /// GPU did not read asleep after it.
    failed: Option<Instant>,
}

impl<N> Default for NvmlSlot<N> {
    fn default() -> Self {
        Self {
            nvml: None,
            closed: None,
            failed: None,
        }
    }
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
        let awake = nvidia_awake();
        if matches!(awake, Ok(false)) {
            self.nvml.saw_asleep();
        }
        awake
    }

    /// See [`NvmlSlot::read`] for when NVML starts and closes.
    fn gpu(&mut self) -> io::Result<GpuReading> {
        self.nvml.read(
            Instant::now,
            || Nvml::init().map_err(io::Error::other),
            read_nvml,
            nvidia_awake,
        )
    }
}

impl<N> NvmlSlot<N> {
    /// Reads the GPU through NVML, started on the first read. Failed reads
    /// close it, so a later read starts it again: an old handle can keep
    /// failing after a driver reset (ticket 156). But a start and a close
    /// each wake a sleeping GPU (ticket 185), so it closes only on a failed
    /// read within [`NVML_FAIL_WINDOW`] of another, with no good read and no
    /// GPU read asleep between, and while the GPU still reads `awake`; and
    /// no start comes within [`NVML_START_PAUSE`] of a close or a failed
    /// start. A read in that pause fails and leaves the pause as it is.
    fn read(
        &mut self,
        now: impl Fn() -> Instant,
        start: impl FnOnce() -> io::Result<N>,
        read: impl FnOnce(&N) -> io::Result<GpuReading>,
        awake: impl FnOnce() -> io::Result<bool>,
    ) -> io::Result<GpuReading> {
        let nvml = match self.nvml.take() {
            Some(nvml) => nvml,
            None => self.start(&now, start)?,
        };
        let error = match read(&nvml) {
            Ok(reading) => {
                self.failed = None;
                self.nvml = Some(nvml);
                return Ok(reading);
            }
            Err(error) => error,
        };
        let failed_at = now();
        let again = self
            .failed
            .is_some_and(|at| failed_at.saturating_duration_since(at) <= NVML_FAIL_WINDOW);
        let awake = awake();
        self.failed = Some(failed_at);
        if matches!(awake, Ok(false)) {
            self.saw_asleep();
        }
        if again && matches!(awake, Ok(true)) {
            drop(nvml);
            // After the close, which takes about 3 s.
            self.closed = Some(now());
            return Err(io::Error::other(format!("{error} (NVML closed)")));
        }
        self.nvml = Some(nvml);
        Err(error)
    }

    /// The GPU read asleep: a failed read before it shows no stale handle.
    fn saw_asleep(&mut self) {
        self.failed = None;
    }

    /// Starts NVML, unless within [`NVML_START_PAUSE`] of a close or a
    /// failed start.
    fn start(
        &mut self,
        now: &impl Fn() -> Instant,
        start: impl FnOnce() -> io::Result<N>,
    ) -> io::Result<N> {
        if self
            .closed
            .is_some_and(|at| now().saturating_duration_since(at) < NVML_START_PAUSE)
        {
            return Err(io::Error::other(format!(
                "NVML waits {} s after a close or a failed start, as each may wake the GPU",
                NVML_START_PAUSE.as_secs()
            )));
        }
        self.failed = None;
        start().inspect_err(|_| self.closed = Some(now()))
    }
}

fn read_nvml(nvml: &Nvml) -> io::Result<GpuReading> {
    let device = nvml.device_by_index(0).map_err(io::Error::other)?;
    Ok(GpuReading {
        celsius: device
            .temperature(TemperatureSensor::Gpu)
            .map_err(io::Error::other)?,
        load_percent: device.utilization_rates().map_err(io::Error::other)?.gpu,
    })
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

/// Whether the NVIDIA GPU is awake, read without a wake.
fn nvidia_awake() -> io::Result<bool> {
    let Some(id) = nvidia_display()? else {
        return Err(io::Error::other("no NVIDIA display adapter is present"));
    };
    is_awake(device_power_state(&id)?)
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

    /// A fake NVML: the handle is a number, and a read fails on handle 0.
    struct FakeNvml {
        slot: NvmlSlot<u8>,
        now: Instant,
        starts: u8,
    }

    impl FakeNvml {
        fn new() -> Self {
            Self {
                slot: NvmlSlot::default(),
                now: Instant::now(),
                starts: 0,
            }
        }

        /// One read: a start gives `handle` (none fails), and the GPU then
        /// reads `awake`.
        fn read(&mut self, handle: Option<u8>, awake: io::Result<bool>) -> io::Result<GpuReading> {
            let now = self.now;
            let starts = &mut self.starts;
            self.slot.read(
                || now,
                || {
                    *starts += 1;
                    handle.ok_or_else(|| io::Error::other("start failed"))
                },
                |&handle| {
                    if handle == 0 {
                        Err(io::Error::other("read failed"))
                    } else {
                        Ok(GpuReading {
                            celsius: 50,
                            load_percent: 0,
                        })
                    }
                },
                || awake,
            )
        }

        fn wait(&mut self, time: Duration) {
            self.now += time;
        }

        fn open(&self) -> Option<u8> {
            self.slot.nvml
        }
    }

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn nvml_starts_once_and_stays_open() {
        let mut fake = FakeNvml::new();

        assert!(fake.read(Some(1), Ok(true)).is_ok());
        assert!(fake.read(Some(1), Ok(true)).is_ok());

        assert_eq!(
            (fake.starts, fake.open(), fake.slot.closed),
            (1, Some(1), None)
        );
    }

    #[test]
    fn two_failed_reads_on_an_awake_gpu_close_nvml_and_pause_its_start() {
        let mut fake = FakeNvml::new();

        assert!(fake.read(Some(0), Ok(true)).is_err());
        assert_eq!(fake.open(), Some(0));
        fake.wait(SECOND);
        let closed = fake.now;
        let error = fake.read(Some(0), Ok(true)).unwrap_err();
        assert!(error.to_string().ends_with("(NVML closed)"), "{error}");
        assert_eq!((fake.open(), fake.slot.closed), (None, Some(closed)));
        // Reads in the pause neither start NVML nor move the pause on.
        for _ in 0..4 {
            fake.wait(SECOND);
            assert!(fake.read(Some(1), Ok(true)).is_err());
        }
        fake.wait(SECOND.saturating_sub(Duration::from_millis(1)));
        assert!(fake.read(Some(1), Ok(true)).is_err());
        assert_eq!((fake.starts, fake.slot.closed), (1, Some(closed)));

        fake.wait(Duration::from_millis(1));
        assert!(fake.read(Some(1), Ok(true)).is_ok());
        assert_eq!((fake.starts, fake.open()), (2, Some(1)));
    }

    #[test]
    fn a_good_read_between_failures_keeps_nvml() {
        let mut fake = FakeNvml::new();
        assert!(fake.read(Some(1), Ok(true)).is_ok());
        fake.slot.nvml = Some(0);
        assert!(fake.read(Some(1), Ok(true)).is_err());
        fake.slot.nvml = Some(1);
        assert!(fake.read(Some(1), Ok(true)).is_ok());
        fake.slot.nvml = Some(0);

        assert!(fake.read(Some(1), Ok(true)).is_err());

        assert_eq!((fake.open(), fake.slot.closed), (Some(0), None));
    }

    #[test]
    fn failed_reads_far_apart_keep_nvml() {
        let mut fake = FakeNvml::new();

        assert!(fake.read(Some(0), Ok(true)).is_err());
        fake.wait(NVML_FAIL_WINDOW + Duration::from_millis(1));
        assert!(fake.read(Some(0), Ok(true)).is_err());
        assert_eq!(fake.open(), Some(0));

        fake.wait(NVML_FAIL_WINDOW);
        assert!(fake.read(Some(0), Ok(true)).is_err());
        assert_eq!(fake.open(), None);
    }

    #[test]
    fn a_failure_then_a_sleep_then_a_failure_keeps_nvml() {
        let mut fake = FakeNvml::new();

        assert!(fake.read(Some(0), Ok(true)).is_err());
        // Seen asleep by a failed read, or by the power check of a poll
        // that then skips the read.
        assert!(fake.read(Some(0), Ok(false)).is_err());
        assert!(fake.read(Some(0), Ok(true)).is_err());
        fake.slot.saw_asleep();
        assert!(fake.read(Some(0), Ok(true)).is_err());

        assert_eq!((fake.open(), fake.slot.closed), (Some(0), None));
    }

    #[test]
    fn failed_reads_keep_nvml_while_the_gpu_sleeps_or_is_not_known() {
        for known in [true, false] {
            let mut fake = FakeNvml::new();

            for _ in 0..3 {
                let state = if known {
                    Ok(false)
                } else {
                    Err(io::Error::other("not known"))
                };
                assert!(fake.read(Some(0), state).is_err());
            }

            assert_eq!((fake.open(), fake.slot.closed), (Some(0), None));
        }
    }

    #[test]
    fn a_failed_start_pauses_the_next_start() {
        let mut fake = FakeNvml::new();
        let failed = fake.now;

        assert!(fake.read(None, Ok(true)).is_err());
        fake.wait(NVML_START_PAUSE.saturating_sub(SECOND));
        assert!(fake.read(Some(1), Ok(true)).is_err());
        assert_eq!((fake.starts, fake.slot.closed), (1, Some(failed)));

        fake.wait(SECOND);
        assert!(fake.read(Some(1), Ok(true)).is_ok());
        assert_eq!(fake.starts, 2);
    }

    #[test]
    fn only_d0_is_awake() {
        assert!(is_awake(PowerDeviceD0).unwrap());
        assert!(!is_awake(PowerDeviceD3).unwrap());
        assert!(is_awake(PowerDeviceUnspecified).is_err());
    }
}
