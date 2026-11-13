//! HID devices through Windows alone (ticket 194). The list of HID
//! interfaces is read from Windows with no open, so a caller can keep only
//! its own device's paths and open nothing else: an open, even with no
//! access, wakes a USB device in selective suspend (probe 2026-11-13).
//! The calls match what hidapi 2.6.7's `windows-native` code did.

use std::io;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_IDW, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_PropertyW, CM_Get_Parent,
    CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CR_BUFFER_SMALL,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes,
    HidD_GetHidGuid, HidD_GetPreparsedData, HidD_SetFeature, HidD_SetNumInputBuffers, HidP_GetCaps,
};
use windows_sys::Win32::Devices::Properties::{DEVPKEY_Device_InstanceId, DEVPROP_TYPE_STRING};
use windows_sys::Win32::Foundation::{
    ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE,
    INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
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

/// How many times the HID list is read when it grows while it is read.
const LIST_TRIES: usize = 3;

/// How many input reports Windows keeps for a collection, as hidapi set.
const INPUT_BUFFERS: u32 = 64;

/// The longest wait for a feature report (ticket 197), then the read is
/// cancelled. The controller answers in milliseconds (not measured after
/// a resume); one that never answers no longer stalls the event-loop
/// thread for good, as hidapi's wait with no limit could, unless its
/// driver ignores the cancel too (ticket 201).
const FEATURE_WAIT: Duration = Duration::from_secs(3);

/// The paths of the HID interfaces that are present. Read from Windows'
/// device list: no device is opened or asked anything.
///
/// # Errors
///
/// The Configuration Manager's error if the list could not be read.
pub fn hid_paths() -> io::Result<Vec<String>> {
    let guid = hid_guid();
    for _ in 0..LIST_TRIES {
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
        let result = unsafe {
            CM_Get_Device_Interface_ListW(
                &raw const guid,
                ptr::null(),
                buffer.as_mut_ptr(),
                length,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        // A device that came between the two calls makes the list longer.
        if result != CR_BUFFER_SMALL {
            check(result)?;
            return Ok(split_ids(&buffer));
        }
    }
    Err(io::Error::other(
        "the HID list kept growing while it was read",
    ))
}

/// The longest device ID Windows makes is 200 characters; room for more.
const ID_ROOM: usize = 512;

/// The device ID of the parent of the HID interface at `path`, such as
/// `USB\VID_1532&PID_02C7&MI_02\...` for a collection of a USB device.
/// Read from Windows' device tree: no device is opened or asked anything.
///
/// # Errors
///
/// The Configuration Manager's error if the tree could not be read.
pub fn hid_parent_id(path: &str) -> io::Result<String> {
    let path = wide(path);
    let mut id = [0_u16; ID_ROOM];
    let mut size = u32::try_from(size_of_val(&id)).map_err(io::Error::other)?;
    let mut kind = 0;
    // SAFETY: the path ends in a nul; the buffer is `size` bytes long.
    check(unsafe {
        CM_Get_Device_Interface_PropertyW(
            path.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &raw mut kind,
            id.as_mut_ptr().cast(),
            &raw mut size,
            0,
        )
    })?;
    if kind != DEVPROP_TYPE_STRING {
        return Err(io::Error::other(format!(
            "the interface's device ID has type {kind}"
        )));
    }
    let mut node = 0;
    // SAFETY: Windows wrote a nul-ended ID into the zeroed buffer, which
    // keeps a nul at its end; Windows writes only the node.
    check(unsafe { CM_Locate_DevNodeW(&raw mut node, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) })?;
    let mut parent = 0;
    // SAFETY: Windows writes only the parent node.
    check(unsafe { CM_Get_Parent(&raw mut parent, node, 0) })?;
    let mut parent_id = [0_u16; ID_ROOM];
    let room = u32::try_from(ID_ROOM - 1).map_err(io::Error::other)?;
    // SAFETY: the buffer is one more than `room` wide characters long, so
    // the ID always ends in a nul.
    check(unsafe { CM_Get_Device_IDW(parent, parent_id.as_mut_ptr(), room, 0) })?;
    Ok(split_ids(&parent_id).into_iter().next().unwrap_or_default())
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
        refuse_empty(report)?;
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
    /// report ID, and waits for it up to [`FEATURE_WAIT`]. The count is the
    /// bytes read; for report ID 0 Windows leaves the ID out of its
    /// count, so one is added, as hidapi did.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::TimedOut`] if no reply came in time: the read is
    /// then cancelled, not tried again. Else the Windows error.
    pub fn get_feature(&self, buffer: &mut [u8]) -> io::Result<usize> {
        refuse_empty(buffer)?;
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
        let read = feature_count(finish(&self.handle, ok != 0, &overlapped, FEATURE_WAIT)?)?;
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
        refuse_empty(buffer)?;
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
        let Some(read) = finish(&self.handle, ok != 0, &overlapped, wait)? else {
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
}

/// Waits up to `wait` for a request on `handle` to end and gives its byte
/// count. A request still running then is cancelled and waited for, and
/// gives `None` unless it ended first.
fn finish(
    handle: &Handle,
    started: bool,
    overlapped: &OVERLAPPED,
    wait: Duration,
) -> io::Result<Option<u32>> {
    if !started {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(code(ERROR_IO_PENDING)) {
            return Err(error);
        }
    }
    let mut count = 0;
    // u32::MAX is no limit: the longest real wait is one less.
    let millis =
        u32::try_from(wait.as_millis()).map_or(u32::MAX - 1, |millis| millis.min(u32::MAX - 1));
    // SAFETY: the overlapped is the running request's; Windows writes
    // only the count.
    if unsafe { GetOverlappedResultEx(handle.0, overlapped, &raw mut count, millis, 0) } != 0 {
        return Ok(Some(count));
    }
    let error = io::Error::last_os_error();
    // A wait of 0 ms says "incomplete" where a longer one says "timed out".
    let still_running = [code(WAIT_TIMEOUT), code(ERROR_IO_INCOMPLETE)];
    if !still_running.contains(&error.raw_os_error().unwrap_or(0)) {
        // Most such errors are the request's own, so it has ended; but
        // a failed wait leaves it running, and its buffer must not be
        // freed under it. Cancel and wait to be sure.
        let _ = cancel(handle, overlapped);
        return Err(error);
    }
    cancel(handle, overlapped)
}

/// Cancels a running request and waits for it to end: its count if it
/// ended first, else `None`. The wait has no limit: the request's buffer
/// must not be freed while Windows still uses it (ticket 201).
fn cancel(handle: &Handle, overlapped: &OVERLAPPED) -> io::Result<Option<u32>> {
    let mut count = 0;
    // SAFETY: the overlapped is the running request's. The wait holds
    // until Windows is done with the request's buffer.
    let ended = unsafe {
        CancelIoEx(handle.0, overlapped);
        GetOverlappedResult(handle.0, overlapped, &raw mut count, 1)
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

/// The byte count of a feature read that ended, or a time-out for one
/// cut off at [`FEATURE_WAIT`]. Never a guessed count: a guess would be
/// parsed as a reply.
fn feature_count(ended: Option<u32>) -> io::Result<u32> {
    ended.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("no feature report within {FEATURE_WAIT:?}"),
        )
    })
}

/// An empty report or buffer is refused, as hidapi did: a send would
/// otherwise go out as a report of zeros.
fn refuse_empty(bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an empty HID report or buffer",
        ));
    }
    Ok(())
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
        Err(io::Error::other(format!(
            "HidP_GetCaps returned {status:#x}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_read_that_ended_gives_its_count() {
        assert_eq!(feature_count(Some(91)).unwrap(), 91);
    }

    #[test]
    fn a_feature_read_cut_off_at_its_limit_is_timed_out() {
        let error = feature_count(None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    /// A request on one end of a new overlapped named pipe, as a HID
    /// request is: the test's stand-in for a device. Gives the server
    /// end, which reads, and the client end, which writes.
    fn pipe(name: &str) -> (Handle, Handle) {
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows_sys::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE};
        let name = format!(r"\\.\pipe\synapse-hid-test-{}-{name}", std::process::id());
        let path = wide(&name);
        // SAFETY: the name ends in a nul; no security.
        let server = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE,
                1,
                64,
                64,
                0,
                ptr::null(),
            )
        };
        assert_ne!(
            server,
            INVALID_HANDLE_VALUE,
            "{}",
            io::Error::last_os_error()
        );
        let server = Handle::new(server).unwrap();
        let client = open(&name, GENERIC_READ | GENERIC_WRITE).unwrap();
        (server, client)
    }

    /// Starts an overlapped read of up to 8 bytes on `handle` and waits
    /// for it up to `wait`.
    fn read_with_limit(handle: &Handle, wait: Duration) -> io::Result<Option<u32>> {
        // SAFETY: no name or security; a manual-reset event, not set.
        let event = Handle::new(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) }).unwrap();
        let mut buffer = [0u8; 8];
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..OVERLAPPED::default()
        };
        // SAFETY: the buffer and overlapped outlive the request, which
        // `finish` waits for or cancels.
        let ok = unsafe {
            ReadFile(
                handle.0,
                buffer.as_mut_ptr(),
                8,
                ptr::null_mut(),
                &raw mut overlapped,
            )
        };
        finish(handle, ok != 0, &overlapped, wait)
    }

    #[test]
    fn a_request_with_no_reply_is_cancelled_at_its_limit() {
        let (server, _client) = pipe("silent");
        let started = std::time::Instant::now();
        assert_eq!(
            read_with_limit(&server, Duration::from_millis(50)).unwrap(),
            None
        );
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(40), "{took:?}");
        assert!(took < Duration::from_secs(2), "{took:?}");
    }

    #[test]
    fn a_request_with_a_reply_gives_its_count() {
        use windows_sys::Win32::Storage::FileSystem::WriteFile;
        let (server, client) = pipe("reply");
        let mut written = 0;
        // SAFETY: the bytes outlive the call; the client end is not overlapped-waited.
        let wrote = unsafe {
            WriteFile(
                client.0,
                b"abc".as_ptr(),
                3,
                &raw mut written,
                ptr::null_mut(),
            )
        };
        assert_ne!(wrote, 0, "{}", io::Error::last_os_error());
        assert_eq!(
            read_with_limit(&server, Duration::from_secs(1)).unwrap(),
            Some(3)
        );
    }

    #[test]
    fn the_feature_read_limit_is_a_few_seconds() {
        assert!((Duration::from_secs(1)..=Duration::from_secs(5)).contains(&FEATURE_WAIT));
    }
}
