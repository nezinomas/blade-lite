//! The real [`Transport`]: feature reports through Windows' HID calls. Also
//! the key reports the keyboard sends on its own. Only the model's own HID
//! paths are ever opened (ticket 194).

use std::io;
use std::time::Duration;

use platform::{HidDetails, HidDevice};
use razer_proto::{ModelProfile, REPORT_LEN, UsbId};

use crate::discovery::model_interface;
use crate::{HidInfo, Transport, find_controller, find_key_collections};

/// The controller does not number its reports, so each one starts with ID 0.
const REPORT_ID: u8 = 0;

/// The controller, opened through Windows' HID calls.
pub struct HidTransport {
    device: HidDevice,
    info: HidInfo,
}

impl HidTransport {
    /// Finds the model's controller among the HID devices and opens it.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if there is no controller, or any HID error.
    pub fn open(model: &ModelProfile) -> io::Result<Self> {
        let devices = model_devices(&WindowsHid, &model.usb)?;
        let info = find_controller(&devices, &model.usb)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no {} controller", model.name),
                )
            })?;
        let device = HidDevice::open(&info.path)?;
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
/// Windows' HID calls. They are only read: nothing is sent to them.
pub struct HidKeyReports {
    sources: KeySources<HidDevice>,
    /// The path of each collection in `sources`, in the same order.
    paths: Vec<String>,
}

impl HidKeyReports {
    /// Opens each collection that sends the model's key reports. One that
    /// can't be opened is left out.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if the model has no power key or none
    /// of its collections opens, or any HID error.
    pub fn open(model: &ModelProfile) -> io::Result<Self> {
        Self::open_preferring(model, None)
    }

    /// As [`open`](Self::open), but only the collection at `sender` (the
    /// one that sent last, see [`sender`](Self::sender)) when it is still
    /// one of the model's key collections and opens; else all of them,
    /// from the same device list.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open).
    pub fn open_preferring(model: &ModelProfile, sender: Option<&str>) -> io::Result<Self> {
        let not_found = || {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no {} key reports", model.name),
            )
        };
        let key = model.power_key.ok_or_else(not_found)?;
        let infos = model_devices(&WindowsHid, &model.usb)?;
        let found = find_key_collections(&infos, &model.usb, &key);
        let picked = pick_key_collections(found.clone(), sender);
        let open_each = |infos: &[&HidInfo]| -> (Vec<String>, Vec<HidDevice>) {
            infos
                .iter()
                .filter_map(|info| {
                    let device = HidDevice::open(&info.path).ok()?;
                    Some((info.path.clone(), device))
                })
                .unzip()
        };
        let (mut paths, mut devices) = open_each(&picked);
        if devices.is_empty() && picked.len() < found.len() {
            (paths, devices) = open_each(&found);
        }
        if devices.is_empty() {
            return Err(not_found());
        }
        Ok(Self {
            sources: KeySources::new(devices),
            paths,
        })
    }

    /// Whether only one collection is read: one was opened, or one has
    /// sent. A long wait then holds no other collection's report back.
    #[must_use]
    pub fn single(&self) -> bool {
        self.sources.single()
    }

    /// The path of the collection that sent, once one has.
    #[must_use]
    pub fn sender(&self) -> Option<&str> {
        let index = self.sources.source?;
        self.paths.get(index).map(String::as_str)
    }

    /// Waits up to `wait` for a report on each collection in turn until one
    /// sends, then on that one alone, so a call before the first report can
    /// take `wait` once per collection. `None` if none came.
    ///
    /// # Errors
    ///
    /// Any HID error, such as the keyboard going away, or the collection
    /// that sent last no longer being in the list (it can't today: the list
    /// never changes after `open`).
    pub fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.sources.read(wait)
    }
}

/// Only the collection at `sender` when it is in `found`, else all of
/// `found`.
fn pick_key_collections<'a>(found: Vec<&'a HidInfo>, sender: Option<&str>) -> Vec<&'a HidInfo> {
    let picked = sender.and_then(|path| found.iter().find(|info| info.path == path));
    match picked {
        Some(&info) => vec![info],
        None => found,
    }
}

/// A collection read with a wait: Windows', or a fake in the tests.
trait ReadTimeout {
    /// Reads one report into `buffer`, waiting up to `millis`. 0 bytes if
    /// none came.
    fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> io::Result<usize>;
}

impl ReadTimeout for HidDevice {
    fn read_timeout(&self, buffer: &mut [u8], millis: i32) -> io::Result<usize> {
        let wait = Duration::from_millis(u64::try_from(millis).unwrap_or(0));
        HidDevice::read(self, buffer, wait)
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

    /// Whether only one collection is read: there is one, or one sent.
    fn single(&self) -> bool {
        self.devices.len() == 1 || self.source.is_some()
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
                    .ok_or_else(|| io::Error::other("the HID read gave more than the buffer"))?;
                return Ok(Some(report.to_vec()));
            }
        }
        Ok(None)
    }
}

/// The HID layer: Windows', or a fake in the tests.
trait HidSystem {
    /// The paths of the HID interfaces present, read with no open.
    fn paths(&self) -> io::Result<Vec<String>>;
    /// The details of the collection at `path`. This opens it, and so
    /// wakes its device if it sleeps.
    fn details(&self, path: &str) -> io::Result<HidDetails>;
}

/// Windows' HID layer.
struct WindowsHid;

impl HidSystem for WindowsHid {
    fn paths(&self) -> io::Result<Vec<String>> {
        platform::hid_paths()
    }

    fn details(&self, path: &str) -> io::Result<HidDetails> {
        platform::hid_details(path)
    }
}

/// The HID collections of the model's device. Only paths that name the
/// model's VID and PID are opened (for their usages), so no other device
/// is opened or asked anything: an open wakes a sleeping USB device
/// (ticket 194). A collection that can't be read, or whose VID and PID
/// are not its path's, is left out.
fn model_devices(system: &impl HidSystem, usb: &UsbId) -> io::Result<Vec<HidInfo>> {
    let infos = system
        .paths()?
        .into_iter()
        .filter_map(|path| {
            let interface_number = model_interface(&path, usb)?;
            let details = system.details(&path).ok()?;
            let same = details.vendor_id == usb.vendor_id && details.product_id == usb.product_id;
            same.then_some(HidInfo {
                path,
                vendor_id: details.vendor_id,
                product_id: details.product_id,
                interface_number,
                usage_page: details.usage_page,
                usage: details.usage,
            })
        })
        .collect();
    Ok(infos)
}

impl Transport for HidTransport {
    fn send_report(&mut self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        self.device.send_feature(&feature_buffer(report))
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        let mut buffer = [REPORT_ID; REPORT_LEN + 1];
        let read = self.device.get_feature(&mut buffer)?;
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
    fn one_collection_is_a_single_source() {
        let (sources, _) = sources(vec![vec![]]);

        assert!(sources.single());
    }

    #[test]
    fn two_collections_are_a_single_source_only_once_one_sent() {
        let (mut sources, _) = sources(vec![vec![], vec![Ok(POWER_KEY.to_vec())]]);
        assert!(!sources.single());

        sources.read(WAIT).unwrap();

        assert!(sources.single());
    }

    fn key_entry(path: &str) -> HidInfo {
        HidInfo {
            path: path.to_owned(),
            vendor_id: 0x1532,
            product_id: 0x02C7,
            interface_number: 1,
            usage_page: 0x01,
            usage: 0x00,
        }
    }

    #[test]
    fn the_last_sender_is_the_only_collection_opened() {
        let found = [key_entry("a"), key_entry("b")];
        let found: Vec<&HidInfo> = found.iter().collect();

        let picked = pick_key_collections(found.clone(), Some("b"));
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].path, "b");
        assert_eq!(pick_key_collections(found.clone(), None).len(), 2);
        // A sender no longer listed (its path changed): all of them.
        assert_eq!(pick_key_collections(found, Some("gone")).len(), 2);
    }

    #[test]
    fn a_wait_too_long_is_the_longest() {
        let (mut sources, log) = sources(vec![vec![]]);

        sources.read(Duration::MAX).unwrap();
        assert_eq!(*log.borrow(), [(0, i32::MAX)]);
    }

    /// A HID path as Windows lists it, for this device part.
    fn path(device: &str) -> String {
        format!(r"\?\HID#{device}#7&8e7fa24&0&0000#{{4d1e55b2-f16f-11cf-88cb-001111000030}}")
    }

    /// A HID layer with these collections, that logs each path it opens.
    struct FakeHid {
        collections: Vec<(String, io::Result<HidDetails>)>,
        opened: RefCell<Vec<String>>,
    }

    impl FakeHid {
        fn new(collections: Vec<(String, io::Result<HidDetails>)>) -> Self {
            Self {
                collections,
                opened: RefCell::default(),
            }
        }
    }

    impl HidSystem for FakeHid {
        fn paths(&self) -> io::Result<Vec<String>> {
            Ok(self
                .collections
                .iter()
                .map(|(path, _)| path.clone())
                .collect())
        }

        fn details(&self, path: &str) -> io::Result<HidDetails> {
            self.opened.borrow_mut().push(path.to_owned());
            let (_, details) = self
                .collections
                .iter()
                .find(|(known, _)| known == path)
                .unwrap();
            match details {
                Ok(details) => Ok(*details),
                Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
            }
        }
    }

    fn details(vendor_id: u16, product_id: u16, usage_page: u16, usage: u16) -> HidDetails {
        HidDetails {
            vendor_id,
            product_id,
            usage_page,
            usage,
        }
    }

    #[test]
    fn only_the_models_paths_are_opened() {
        let usb = &razer_proto::BLADE_18.usb;
        let controller = path("VID_1532&PID_02C7&MI_02");
        let keys = path("VID_1532&PID_02C7&MI_01&COL02");
        let hid = FakeHid::new(vec![
            (
                path("VID_046D&PID_C52B&MI_01&COL01"),
                Ok(details(0x046D, 0xC52B, 1, 2)),
            ),
            (controller.clone(), Ok(details(0x1532, 0x02C7, 1, 2))),
            (path("ELAN0406&COL03"), Ok(details(0x04F3, 0x3282, 0x0D, 5))),
            (keys.clone(), Ok(details(0x1532, 0x02C7, 1, 0))),
        ]);

        let infos = model_devices(&hid, usb).unwrap();

        assert_eq!(*hid.opened.borrow(), [controller.clone(), keys.clone()]);
        assert_eq!(
            infos,
            [
                HidInfo {
                    path: controller,
                    vendor_id: 0x1532,
                    product_id: 0x02C7,
                    interface_number: 2,
                    usage_page: 1,
                    usage: 2,
                },
                HidInfo {
                    path: keys,
                    vendor_id: 0x1532,
                    product_id: 0x02C7,
                    interface_number: 1,
                    usage_page: 1,
                    usage: 0,
                },
            ]
        );
    }

    #[test]
    fn a_collection_unlike_its_path_or_unread_is_left_out() {
        let usb = &razer_proto::BLADE_18.usb;
        let hid = FakeHid::new(vec![
            (
                path("VID_1532&PID_02C7&MI_02"),
                Ok(details(0x1532, 0x0099, 1, 2)),
            ),
            (
                path("VID_1532&PID_02C7&MI_01&COL01"),
                Err(io::Error::other("gone")),
            ),
        ]);

        assert_eq!(model_devices(&hid, usb).unwrap(), []);
        assert_eq!(hid.opened.borrow().len(), 2);
    }

    #[test]
    fn a_short_read_is_an_error() {
        for read in [0, 1, REPORT_LEN] {
            let error = feature_report(&read_buffer(), read).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "{read}");
        }
    }
}
