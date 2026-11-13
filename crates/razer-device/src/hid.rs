//! The real [`Transport`]: feature reports through hidapi. Also the key
//! reports the keyboard sends on its own.

use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use razer_proto::{ModelProfile, REPORT_LEN, UsbId};

use crate::{HidInfo, Transport, find_controller, find_key_collections};

/// The controller does not number its reports, so each one starts with ID 0.
const REPORT_ID: u8 = 0;

/// The controller, opened through hidapi.
pub struct HidTransport {
    device: HidDevice,
    info: HidInfo,
}

impl HidTransport {
    /// Finds the model's controller among the HID devices and opens it.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if there is no controller, or any hidapi error.
    pub fn open(model: &ModelProfile) -> io::Result<Self> {
        let mut api = shared_api();
        let api = api_in(&mut api)?;
        let devices = model_devices(api, &model.usb)?;
        let info = find_controller(&devices, &model.usb)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no {} controller", model.name),
                )
            })?;
        let device = api.open_path(&info.path).map_err(io::Error::other)?;
        Ok(Self { device, info })
    }

    /// The collection that was opened.
    #[must_use]
    pub fn info(&self) -> &HidInfo {
        &self.info
    }
}

/// Room for one key report. The laptop sends 16 bytes.
const KEY_REPORT_ROOM: usize = 64;

/// The collections that send the model's key reports, opened through
/// hidapi. They are only read: nothing is sent to them.
pub struct HidKeyReports {
    sources: KeySources<HidDevice>,
}

impl HidKeyReports {
    /// Opens each collection that sends the model's key reports. One that
    /// can't be opened is left out.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if the model has no power key or none
    /// of its collections opens, or any hidapi error.
    pub fn open(model: &ModelProfile) -> io::Result<Self> {
        let not_found = || {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no {} key reports", model.name),
            )
        };
        let key = model.power_key.ok_or_else(not_found)?;
        let mut api = shared_api();
        let api = api_in(&mut api)?;
        let infos = model_devices(api, &model.usb)?;
        let devices: Vec<HidDevice> = find_key_collections(&infos, &model.usb, &key)
            .into_iter()
            .filter_map(|info| api.open_path(&info.path).ok())
            .collect();
        if devices.is_empty() {
            return Err(not_found());
        }
        Ok(Self {
            sources: KeySources::new(devices),
        })
    }

    /// Waits up to `wait` for a report on each collection in turn until one
    /// sends, then on that one alone, so a call before the first report can
    /// take `wait` once per collection. `None` if none came.
    ///
    /// # Errors
    ///
    /// Any hidapi error, such as the keyboard going away, or the collection
    /// that sent last no longer being in the list (it can't today: the list
    /// never changes after `open`).
    pub fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.sources.read(wait)
    }
}

/// A collection read with a wait: hidapi's, or a fake in the tests.
trait ReadTimeout {
    /// Reads one report into `buffer`, waiting up to `millis`. 0 bytes if
    /// none came.
    fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> io::Result<usize>;
}

impl ReadTimeout for HidDevice {
    fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> io::Result<usize> {
        HidDevice::read_timeout(self, buffer, millis).map_err(io::Error::other)
    }
}

/// The key collections, and the one that sent first.
struct KeySources<D> {
    devices: Vec<D>,
    /// The collection that sent the first report. Only that one is read
    /// from then on, so a press can't come in twice or out of order.
    source: Option<usize>,
}

impl<D: ReadTimeout> KeySources<D> {
    fn new(devices: Vec<D>) -> Self {
        Self {
            devices,
            source: None,
        }
    }

    /// See [`HidKeyReports::read`].
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        let millis = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        let mut buffer = [0; KEY_REPORT_ROOM];
        let first = self.source.unwrap_or(0);
        let devices = match self.source {
            Some(index) => self
                .devices
                .get(index..=index)
                .ok_or_else(|| io::Error::other("the key source is gone"))?,
            None => self.devices.as_slice(),
        };
        for (index, device) in (first..).zip(devices) {
            let read = device.read_timeout(&mut buffer, millis)?;
            if read > 0 {
                self.source = Some(index);
                let report = buffer
                    .get(..read)
                    .ok_or_else(|| io::Error::other("hidapi read more than the buffer"))?;
                return Ok(Some(report.to_vec()));
            }
        }
        Ok(None)
    }
}

/// The one hidapi context of the run. hidapi 2.6 lists every HID device
/// on the PC when a context is made, and asks each one for its strings
/// (requests that go to the device and could wake it), even through
/// `new_without_enumerate`. So the context is made once, and each open
/// after lists only the model's VID and PID (ticket 186).
static API: Mutex<Option<HidApi>> = Mutex::new(None);

/// The shared context's lock. A poisoned lock is taken as it is: the
/// context holds only a device list, which each open lists again.
fn shared_api() -> MutexGuard<'static, Option<HidApi>> {
    API.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The context in `slot`, made on first use.
fn api_in(slot: &mut Option<HidApi>) -> io::Result<&mut HidApi> {
    match slot {
        Some(api) => Ok(api),
        None => Ok(slot.insert(HidApi::new().map_err(io::Error::other)?)),
    }
}

/// The HID device list: hidapi's, or a fake in the tests.
trait DeviceList {
    /// Empties the list.
    fn reset(&mut self);
    /// Adds the devices with this VID and PID.
    fn add(&mut self, vendor_id: u16, product_id: u16) -> io::Result<()>;
    /// The devices in the list.
    fn infos(&self) -> Vec<HidInfo>;
}

impl DeviceList for HidApi {
    fn reset(&mut self) {
        // hidapi's reset only clears its list and can't fail.
        let _ = self.reset_devices();
    }

    fn add(&mut self, vendor_id: u16, product_id: u16) -> io::Result<()> {
        self.add_devices(vendor_id, product_id)
            .map_err(io::Error::other)
    }

    fn infos(&self) -> Vec<HidInfo> {
        self.device_list().map(hid_info).collect()
    }
}

/// The HID collections with the model's VID and PID, listed again. A VID
/// or PID of 0 is refused: hidapi takes 0 as any, which lists every device.
fn model_devices(list: &mut impl DeviceList, usb: &UsbId) -> io::Result<Vec<HidInfo>> {
    if usb.vendor_id == 0 || usb.product_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a VID or PID of 0 would list every HID device",
        ));
    }
    list.reset();
    list.add(usb.vendor_id, usb.product_id)?;
    Ok(list.infos())
}

fn hid_info(device: &DeviceInfo) -> HidInfo {
    HidInfo {
        path: device.path().to_owned(),
        vendor_id: device.vendor_id(),
        product_id: device.product_id(),
        interface_number: device.interface_number(),
        usage_page: device.usage_page(),
        usage: device.usage(),
    }
}

impl Transport for HidTransport {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        self.device
            .send_feature_report(&feature_buffer(report))
            .map_err(io::Error::other)
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        let mut buffer = [REPORT_ID; REPORT_LEN + 1];
        let read = self
            .device
            .get_feature_report(&mut buffer)
            .map_err(io::Error::other)?;
        feature_report(&buffer, read)
    }
}

/// What a feature send writes: the report ID, then the report.
fn feature_buffer(report: &[u8; REPORT_LEN]) -> [u8; REPORT_LEN + 1] {
    let mut buffer = [REPORT_ID; REPORT_LEN + 1];
    buffer[1..].copy_from_slice(report);
    buffer
}

/// The report from a feature read of `read` bytes into `buffer`, without
/// its report ID. A short read is an error.
fn feature_report(buffer: &[u8; REPORT_LEN + 1], read: usize) -> io::Result<[u8; REPORT_LEN]> {
    if read < buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("short feature report: {read} bytes"),
        ));
    }
    let mut report = [0; REPORT_LEN];
    report.copy_from_slice(&buffer[1..]);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    /// A feature read: the report ID, then the report.
    fn read_buffer() -> [u8; REPORT_LEN + 1] {
        let mut buffer = [0xAB; REPORT_LEN + 1];
        buffer[0] = REPORT_ID;
        buffer
    }

    #[test]
    fn a_full_read_gives_the_report_without_its_id() {
        assert_eq!(
            feature_report(&read_buffer(), REPORT_LEN + 1).unwrap(),
            [0xAB; REPORT_LEN]
        );
    }

    #[test]
    fn a_report_is_sent_after_its_id() {
        let buffer = feature_buffer(&[0xAB; REPORT_LEN]);
        assert_eq!(buffer[0], REPORT_ID);
        assert_eq!(buffer[1..], [0xAB; REPORT_LEN]);
    }

    /// A key collection that gives its reports in turn, then 0 bytes, and
    /// logs its number and the wait of each read. A report longer than the
    /// buffer is cut, but its whole length is said, as a broken read would.
    struct FakeCollection {
        number: usize,
        reports: RefCell<VecDeque<io::Result<Vec<u8>>>>,
        log: Rc<RefCell<Vec<(usize, i32)>>>,
    }

    impl ReadTimeout for FakeCollection {
        fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> io::Result<usize> {
            self.log.borrow_mut().push((self.number, millis));
            let Some(report) = self.reports.borrow_mut().pop_front() else {
                return Ok(0);
            };
            let report = report?;
            let fits = report.len().min(buffer.len());
            buffer[..fits].copy_from_slice(&report[..fits]);
            Ok(report.len())
        }
    }

    type ReadLog = Rc<RefCell<Vec<(usize, i32)>>>;

    /// Key sources over collections that give these reports, and their log.
    fn sources(reports: Vec<Vec<io::Result<Vec<u8>>>>) -> (KeySources<FakeCollection>, ReadLog) {
        let log = ReadLog::default();
        let devices = reports
            .into_iter()
            .enumerate()
            .map(|(number, reports)| FakeCollection {
                number,
                reports: RefCell::new(reports.into()),
                log: Rc::clone(&log),
            })
            .collect();
        (KeySources::new(devices), log)
    }

    const WAIT: Duration = Duration::from_millis(250);
    const POWER_KEY: [u8; 3] = [0x04, 0x0A, 0xD3];

    #[test]
    fn no_report_is_none_after_each_collection_waited() {
        let (mut sources, log) = sources(vec![vec![], vec![]]);

        assert_eq!(sources.read(WAIT).unwrap(), None);
        assert_eq!(*log.borrow(), [(0, 250), (1, 250)]);
    }

    #[test]
    fn zero_bytes_is_no_report() {
        let (mut sources, _) = sources(vec![vec![Ok(Vec::new())]]);

        assert_eq!(sources.read(WAIT).unwrap(), None);
    }

    #[test]
    fn the_report_is_the_bytes_read() {
        let (mut sources, _) = sources(vec![vec![Ok(POWER_KEY.to_vec())]]);

        assert_eq!(sources.read(WAIT).unwrap(), Some(POWER_KEY.to_vec()));
    }

    #[test]
    fn the_first_collection_to_send_is_the_only_one_read_then() {
        let (mut sources, log) = sources(vec![
            vec![Ok(Vec::new()), Ok(vec![0x04, 0x0A])],
            vec![Ok(POWER_KEY.to_vec()), Ok(vec![0x04, 0x00])],
        ]);

        assert_eq!(sources.read(WAIT).unwrap(), Some(POWER_KEY.to_vec()));
        assert_eq!(sources.read(WAIT).unwrap(), Some(vec![0x04, 0x00]));
        assert_eq!(sources.read(WAIT).unwrap(), None);
        assert_eq!(*log.borrow(), [(0, 250), (1, 250), (1, 250), (1, 250)]);
    }

    #[test]
    fn the_first_source_is_kept_when_it_is_the_first_collection() {
        let (mut sources, log) = sources(vec![
            vec![Ok(POWER_KEY.to_vec())],
            vec![Ok(vec![0x04, 0x0A])],
        ]);

        sources.read(WAIT).unwrap();
        assert_eq!(sources.read(WAIT).unwrap(), None);
        assert_eq!(*log.borrow(), [(0, 250), (0, 250)]);
    }

    #[test]
    fn a_read_longer_than_the_buffer_is_an_error() {
        let too_long = vec![0x04; KEY_REPORT_ROOM + 1];
        let (mut sources, _) = sources(vec![vec![Ok(too_long)]]);

        assert!(sources.read(WAIT).is_err());
    }

    #[test]
    fn a_source_no_longer_in_the_list_is_an_error() {
        let (mut sources, log) = sources(vec![vec![]]);
        sources.source = Some(5);

        assert!(sources.read(WAIT).is_err());
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_read_error_is_returned() {
        let error = io::Error::other("the keyboard went away");
        let (mut sources, _) = sources(vec![vec![Err(error)]]);

        assert!(sources.read(WAIT).is_err());
    }

    #[test]
    fn a_wait_too_long_for_hidapi_is_its_longest() {
        let (mut sources, log) = sources(vec![vec![]]);

        sources.read(Duration::MAX).unwrap();
        assert_eq!(*log.borrow(), [(0, i32::MAX)]);
    }

    /// A device list that logs each VID and PID it is asked to add, and
    /// keeps a stale entry until it is reset.
    #[derive(Default)]
    struct FakeList {
        asked: Vec<(u16, u16)>,
        entries: Vec<HidInfo>,
    }

    fn entry(vendor_id: u16, product_id: u16) -> HidInfo {
        HidInfo {
            path: c"path".to_owned(),
            vendor_id,
            product_id,
            interface_number: 0,
            usage_page: 0,
            usage: 0,
        }
    }

    impl DeviceList for FakeList {
        fn reset(&mut self) {
            self.entries.clear();
        }

        fn add(&mut self, vendor_id: u16, product_id: u16) -> io::Result<()> {
            self.asked.push((vendor_id, product_id));
            self.entries.push(entry(vendor_id, product_id));
            Ok(())
        }

        fn infos(&self) -> Vec<HidInfo> {
            self.entries.clone()
        }
    }

    #[test]
    fn only_the_models_vid_and_pid_are_listed() {
        let usb = &razer_proto::BLADE_18.usb;
        let mut list = FakeList {
            entries: vec![entry(0x046D, 0xC52B)],
            ..FakeList::default()
        };

        let infos = model_devices(&mut list, usb).unwrap();

        assert_eq!(list.asked, [(usb.vendor_id, usb.product_id)]);
        assert_eq!(infos, [entry(usb.vendor_id, usb.product_id)]);
    }

    #[test]
    fn a_vid_or_pid_of_0_is_refused_and_nothing_is_listed() {
        let usb = razer_proto::BLADE_18.usb;
        for zero in [
            UsbId {
                vendor_id: 0,
                ..usb
            },
            UsbId {
                product_id: 0,
                ..usb
            },
        ] {
            let mut list = FakeList::default();

            let error = model_devices(&mut list, &zero).unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(list.asked, []);
        }
    }

    #[test]
    fn a_short_read_is_an_error() {
        for read in [0, 1, REPORT_LEN] {
            let error = feature_report(&read_buffer(), read).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "{read}");
        }
    }
}
