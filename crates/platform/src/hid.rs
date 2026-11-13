//! HID devices through Windows alone (ticket 194). The list of HID
//! interfaces is read from Windows with no open, so a caller can keep only
//! its own device's paths and open nothing else: an open, even with no
//! access, wakes a USB device in selective suspend (probe 2026-11-13).
//! The calls match what hidapi 2.6.7's `windows-native` code did.

use std::io;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes,
    HidD_GetHidGuid, HidD_GetPreparsedData, HidD_SetFeature, HidD_SetNumInputBuffers,
    HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, DeviceIoControl, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED,
};
use windows_sys::Win32::System::Threading::CreateEventW;
use windows_sys::core::GUID;

use crate::instance::Handle;
use crate::sensors::{check, split_ids};
use crate::wide;

/// `IOCTL_HID_GET_FEATURE`: keyboard device type, function 100, out
/// direct, any access. Not in `windows-sys`; hidapi uses the same value.
const IOCTL_HID_GET_FEATURE: u32 = (0x0B << 16) | (100 << 2) | 2;

/// How many input reports Windows keeps for a collection, as hidapi set.
const INPUT_BUFFERS: u32 = 64;

/// The paths of the HID interfaces that are present. Read from Windows'
/// device list: no device is opened or asked anything.
///
/// # Errors
///
/// The Configuration Manager's error if the list could not be read.
pub fn hid_paths() -> io::Result<Vec<String>> {
    let guid = hid_guid();
    let mut length = 0;
    // SAFETY: the GUID is a whole value; Windows writes only the length.
    check(unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &raw mut length,
            &raw const guid,
            ptr::null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    })?;
    let mut buffer = vec![0_u16; length as usize];
    // SAFETY: the buffer is `length` wide characters long.
    check(unsafe {
        CM_Get_Device_Interface_ListW(
            &raw const guid,
            ptr::null(),
            buffer.as_mut_ptr(),
            length,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    })?;
    Ok(split_ids(&buffer))
}

/// The HID interface class.
fn hid_guid() -> GUID {
    let mut guid = GUID::from_u128(0);
    // SAFETY: Windows writes only the GUID; nothing is sent to a device.
    unsafe { HidD_GetHidGuid(&raw mut guid) };
    guid
}

/// What Windows knows of a HID collection, read with no request to the
/// device: its VID and PID, usage page and usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidDetails {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
}

/// Opens the collection at `path` with no access and reads its details.
/// The open wakes the device if it sleeps, so call this only for a path
/// already known to be the wanted device's.
///
/// # Errors
///
/// The Windows error if the collection could not be opened or read.
pub fn hid_details(path: &str) -> io::Result<HidDetails> {
    let handle = open(path, 0)?;
    let attributes = attributes(&handle)?;
    let caps = caps(&handle)?;
    Ok(HidDetails {
        vendor_id: attributes.VendorID,
        product_id: attributes.ProductID,
        usage_page: caps.UsagePage,
        usage: caps.Usage,
    })
}

/// One opened HID collection: feature reports, and input reads with a
/// wait. Every call waits for its own request to end, so no request is
/// left running between calls.
pub struct HidDevice {
    handle: Handle,
    /// Signalled when a request ends.
    event: Handle,
    /// The longest input report, report ID included.
    input_len: usize,
    /// The longest feature report, report ID included.
    feature_len: usize,
}

impl HidDevice {
    /// Opens the collection at `path` to read and write, or with no
    /// access if Windows refuses that (a mouse collection is held by
    /// Windows, but its feature reports still work), as hidapi did.
    ///
    /// # Errors
    ///
    /// The Windows error if the collection could not be opened or read.
    pub fn open(path: &str) -> io::Result<Self> {
        let handle = open(path, GENERIC_READ | GENERIC_WRITE).or_else(|_| open(path, 0))?;
        // SAFETY: the handle is open; Windows only sets its input buffers.
        if !unsafe { HidD_SetNumInputBuffers(handle.0, INPUT_BUFFERS) } {
            return Err(io::Error::last_os_error());
        }
        let caps = caps(&handle)?;
        // SAFETY: no name or security; a manual-reset event, not set.
        let event = Handle::new(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?;
        Ok(Self {
            handle,
            event,
            input_len: usize::from(caps.InputReportByteLength),
            feature_len: usize::from(caps.FeatureReportByteLength),
        })
    }

    /// Sends a feature report, its ID first. A shorter report is padded
    /// with zeros to the collection's feature length; a longer one is
    /// refused, not cut.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::InvalidInput`] for a report longer than the
    /// collection's, else the Windows error.
    pub fn send_feature(&self, report: &[u8]) -> io::Result<()> {
        if report.len() > self.feature_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "a {}-byte feature report is longer than the collection's {}",
                    report.len(),
                    self.feature_len
                ),
            ));
        }
        let mut buffer = vec![0; self.feature_len];
        buffer[..report.len()].copy_from_slice(report);
        let length = u32::try_from(buffer.len()).map_err(io::Error::other)?;
        // SAFETY: the buffer is `length` bytes long and outlives the call.
        if unsafe { HidD_SetFeature(self.handle.0, buffer.as_ptr().cast(), length) } {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Reads a feature report into `buffer`, whose first byte is the
    /// report ID, and waits for it with no time limit. The count is the
    /// bytes read; for report ID 0 Windows leaves the ID out of its
    /// count, so one is added, as hidapi did.
    ///
    /// # Errors
    ///
    /// The Windows error.
    pub fn get_feature(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = u32::try_from(buffer.len()).map_err(io::Error::other)?;
        let mut overlapped = self.overlapped();
        // SAFETY: the buffer is `length` bytes long; it and the overlapped
        // outlive the request, which `finish` waits for.
        let ok = unsafe {
            DeviceIoControl(
                self.handle.0,
                IOCTL_HID_GET_FEATURE,
                buffer.as_mut_ptr().cast(),
                length,
                buffer.as_mut_ptr().cast(),
                length,
                ptr::null_mut(),
                &raw mut overlapped,
            )
        };
        let read = self.finish(ok != 0, &overlapped, None)?.unwrap_or(0);
        let read = usize::try_from(read).map_err(io::Error::other)?;
        Ok(if buffer.first() == Some(&0) {
            read + 1
        } else {
            read
        })
    }

    /// Waits up to `wait` for one input report and copies it into
    /// `buffer`, without a report ID of 0, as hidapi did. 0 bytes if none
    /// came: the read is then cancelled, and a report Windows already
    /// had stays for the next read.
    ///
    /// # Errors
    ///
    /// The Windows error, such as the device going away.
    pub fn read(&self, buffer: &mut [u8], wait: Duration) -> io::Result<usize> {
        let mut report = vec![0; self.input_len];
        let length = u32::try_from(report.len()).map_err(io::Error::other)?;
        let mut overlapped = self.overlapped();
        // SAFETY: the report is `length` bytes long; it and the overlapped
        // outlive the request, which `finish` waits for or cancels.
        let ok = unsafe {
            ReadFile(
                self.handle.0,
                report.as_mut_ptr(),
                length,
                ptr::null_mut(),
                &raw mut overlapped,
            )
        };
        let Some(read) = self.finish(ok != 0, &overlapped, Some(wait))? else {
            return Ok(0);
        };
        let read = usize::try_from(read).map_err(io::Error::other)?;
        let got = report
            .get(..read)
            .ok_or_else(|| io::Error::other("Windows read more than the report"))?;
        let got = match got.split_first() {
            Some((0, rest)) => rest,
            _ => got,
        };
        let copied = got.len().min(buffer.len());
        buffer[..copied].copy_from_slice(&got[..copied]);
        Ok(copied)
    }

    /// A fresh overlapped that signals this device's event.
    fn overlapped(&self) -> OVERLAPPED {
        OVERLAPPED {
            hEvent: self.event.0,
            ..OVERLAPPED::default()
        }
    }

    /// Waits for a request to end and gives its byte count. With a
    /// `wait`, a request still running then is cancelled and waited for,
    /// and gives `None` unless it ended first.
    fn finish(
        &self,
        started: bool,
        overlapped: &OVERLAPPED,
        wait: Option<Duration>,
    ) -> io::Result<Option<u32>> {
        if !started {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(code(ERROR_IO_PENDING)) {
                return Err(error);
            }
        }
        let mut count = 0;
        let millis = wait.map_or(u32::MAX, |wait| {
            // u32::MAX is no limit: the longest real wait is one less.
            u32::try_from(wait.as_millis()).map_or(u32::MAX - 1, |millis| millis.min(u32::MAX - 1))
        });
        // SAFETY: the overlapped is the running request's; Windows writes
        // only the count.
        if unsafe { GetOverlappedResultEx(self.handle.0, overlapped, &raw mut count, millis, 0) }
            != 0
        {
            return Ok(Some(count));
        }
        let error = io::Error::last_os_error();
        // A wait of 0 ms says "incomplete" where a longer one says "timed out".
        let still_running = [code(WAIT_TIMEOUT), code(ERROR_IO_INCOMPLETE)];
        if !still_running.contains(&error.raw_os_error().unwrap_or(0)) {
            return Err(error);
        }
        self.cancel(overlapped)
    }

    /// Cancels a running request and waits for it to end: its count if
    /// it ended first, else `None`.
    fn cancel(&self, overlapped: &OVERLAPPED) -> io::Result<Option<u32>> {
        let mut count = 0;
        // SAFETY: the overlapped is the running request's. The wait holds
        // until Windows is done with the request's buffer.
        let ended = unsafe {
            CancelIoEx(self.handle.0, overlapped);
            GetOverlappedResult(self.handle.0, overlapped, &raw mut count, 1)
        };
        if ended != 0 {
            return Ok(Some(count));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(code(ERROR_OPERATION_ABORTED)) {
            Ok(None)
        } else {
            Err(error)
        }
    }
}

/// A Windows error code as `io::Error` holds it.
fn code(error: u32) -> i32 {
    i32::try_from(error).unwrap_or(i32::MAX)
}

/// Opens a HID collection for overlapped requests, shared with others.
fn open(path: &str, access: u32) -> io::Result<Handle> {
    let path = wide(path);
    // SAFETY: the path ends in a nul; no security or template.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Handle::new(raw)
}

/// The collection's VID and PID, which Windows keeps: no request goes to
/// the device.
fn attributes(handle: &Handle) -> io::Result<HIDD_ATTRIBUTES> {
    let mut attributes = HIDD_ATTRIBUTES {
        Size: u32::try_from(size_of::<HIDD_ATTRIBUTES>()).map_err(io::Error::other)?,
        ..HIDD_ATTRIBUTES::default()
    };
    // SAFETY: the attributes are a whole value with their size set.
    if unsafe { HidD_GetAttributes(handle.0, &raw mut attributes) } {
        Ok(attributes)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The collection's usages and report lengths, from the data Windows keeps
/// of its report descriptor: no request goes to the device.
fn caps(handle: &Handle) -> io::Result<HIDP_CAPS> {
    let mut data = 0;
    // SAFETY: Windows writes only the data's address.
    if !unsafe { HidD_GetPreparsedData(handle.0, &raw mut data) } {
        return Err(io::Error::last_os_error());
    }
    let mut caps = HIDP_CAPS::default();
    // SAFETY: the data came from Windows above and is freed once, after.
    let status = unsafe {
        let status = HidP_GetCaps(data, &raw mut caps);
        HidD_FreePreparsedData(data);
        status
    };
    if status == HIDP_STATUS_SUCCESS {
        Ok(caps)
    } else {
        Err(io::Error::other(format!("HidP_GetCaps returned {status:#x}")))
    }
}
