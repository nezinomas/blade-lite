//! The real [`Transport`]: feature reports through Windows' HID calls. Also
//! the key reports the keyboard sends on its own. Only the model's own HID
//! paths are ever opened (ticket 194).

use std::io;
use std::time::{Duration, Instant};

use platform::{HidDetails, HidDevice};
use razer_proto::{ModelProfile, PowerKey, REPORT_LEN, UsbId};

use crate::discovery::{collection_number, model_interface};
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
    /// one of the model's key collections and opens; else only the one the
    /// profile names, when it is listed and opens (ticket 209); else all
    /// of them, from the same device list.
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
        let picked = pick_key_collections(found.clone(), sender, key.collection);
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
            sources: KeySources::new(devices, key),
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

    /// The path of the one collection read: the one that sent, or the only
    /// one opened. `None` while more than one is read in turn.
    #[must_use]
    pub fn read_path(&self) -> Option<&str> {
        match self.paths.as_slice() {
            [only] => Some(only.as_str()),
            _ => self.sender(),
        }
    }

    /// Waits up to `wait` for a key report on each collection in turn until
    /// one sends, then on that one alone, so a call before the first report
    /// can take `wait` once per collection. `None` if none came. Any other
    /// report is dropped and picks no collection (ticket 203).
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

/// Only the collection at `sender` when it is in `found`; else the ones
/// whose path has the profile's `collection` number, when any has; else
/// all of `found` (ticket 209).
fn pick_key_collections<'a>(
    found: Vec<&'a HidInfo>,
    sender: Option<&str>,
    collection: Option<u8>,
) -> Vec<&'a HidInfo> {
    if let Some(&info) = sender.and_then(|path| found.iter().find(|info| info.path == path)) {
        return vec![info];
    }
    let Some(wanted) = collection else {
        return found;
    };
    let named: Vec<&HidInfo> = found
        .iter()
        .copied()
        .filter(|info| collection_number(&info.path) == Some(wanted))
        .collect();
    if named.is_empty() { found } else { named }
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
    /// The collection that sent the first key report. Only that one is read
    /// from then on, so a press can't come in twice or out of order.
    source: Option<usize>,
    /// What a key report looks like.
    key: PowerKey,
}

impl<D: ReadTimeout> KeySources<D> {
    fn new(devices: Vec<D>, key: PowerKey) -> Self {
        Self {
            devices,
            source: None,
            key,
        }
    }

    /// Whether only one collection is read: there is one, or one sent.
    fn single(&self) -> bool {
        self.devices.len() == 1 || self.source.is_some()
    }

    /// See [`HidKeyReports::read`].
    fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        let first = self.source.unwrap_or(0);
        let devices = match self.source {
            Some(index) => self
                .devices
                .get(index..=index)
                .ok_or_else(|| io::Error::other("the key source is gone"))?,
            None => self.devices.as_slice(),
        };
        for (index, device) in (first..).zip(devices) {
            if let Some(report) = read_key_report(device, self.key, wait)? {
                self.source = Some(index);
                return Ok(Some(report));
            }
        }
        Ok(None)
    }
}

/// Reads `device` until it sends a key report or `wait` is up. Any other
/// report, such as the `05` one another collection sends at each USB wake,
/// is dropped: it picks no source and does not end the wait, so the
/// caller can count the whole wait as quiet (ticket 203).
fn read_key_report(
    device: &impl ReadTimeout,
    key: PowerKey,
    wait: Duration,
) -> io::Result<Option<Vec<u8>>> {
    let deadline = Instant::now().checked_add(wait);
    let mut buffer = [0; KEY_REPORT_ROOM];
    let mut left = wait;
    loop {
        let millis = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        let read = device.read_timeout(&mut buffer, millis)?;
        if read == 0 {
            return Ok(None);
        }
        let report = buffer
            .get(..read)
            .ok_or_else(|| io::Error::other("the HID read gave more than the buffer"))?;
        if key.is_key_report(report) {
            return Ok(Some(report.to_vec()));
        }
        // No deadline (a wait too long to add): the whole wait again.
        left = deadline.map_or(wait, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        if left.is_zero() {
            return Ok(None);
        }
    }
}

/// The HID layer: Windows', or a fake in the tests.
trait HidSystem {
    /// The paths of the HID interfaces present, read with no open.
    fn paths(&self) -> io::Result<Vec<String>>;
    /// The device ID of the interface's parent, read with no open.
    fn parent_id(&self, path: &str) -> io::Result<String>;
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

    fn parent_id(&self, path: &str) -> io::Result<String> {
        platform::hid_parent_id(path)
    }

    fn details(&self, path: &str) -> io::Result<HidDetails> {
        platform::hid_details(path)
    }
}

/// The HID collections of the model's device. Only paths that name the
/// model's VID and PID and sit on a USB device are opened (for their
/// usages), so no other device is opened or asked anything: an open wakes
/// a sleeping USB device (ticket 194). A path on another bus, such as a
/// Razer driver's virtual device named after the real one, is left out,
/// as hidapi left it out (its interface number stayed -1). So is a
/// collection that can't be read, or whose VID and PID are not its path's.
fn model_devices(system: &impl HidSystem, usb: &UsbId) -> io::Result<Vec<HidInfo>> {
    let infos = system
        .paths()?
        .into_iter()
        .filter_map(|path| {
            let interface_number = model_interface(&path, usb)?;
            if !on_usb(&system.parent_id(&path).ok()?) {
                return None;
            }
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

/// Whether a parent device ID is a USB device's, as `USB\VID_1532&...`.
fn on_usb(parent_id: &str) -> bool {
    parent_id
        .get(..4)
        .is_some_and(|bus| bus.eq_ignore_ascii_case("USB\\"))
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
        (KeySources::new(devices, KEY), log)
    }

    const WAIT: Duration = Duration::from_millis(250);
    const KEY: PowerKey = PowerKey {
        interface_number: 1,
        usage_page: 0x01,
        usage: 0x00,
        collection: Some(0x04),
        report_id: 0x04,
        report_len: 16,
        code: 0xD3,
        fn_code: 0x0A,
    };
    const POWER_KEY: [u8; 3] = [0x04, 0x0A, 0xD3];

    /// A 16-byte report as the laptop sends it, from its first bytes.
    fn report(bytes: &[u8]) -> Vec<u8> {
        let mut report = vec![0; 16];
        report[..bytes.len()].copy_from_slice(bytes);
        report
    }

    /// The report collection Col05 sends at each USB wake (ticket 202).
    fn wake_report() -> Vec<u8> {
        report(&[0x05, 0x08, 0x3C])
    }

    #[test]
    fn a_wake_report_picks_nothing_and_the_key_report_after_it_does() {
        let (mut sources, log) = sources(vec![
            vec![Ok(wake_report())],
            vec![Ok(report(&POWER_KEY)), Ok(report(&[0x04]))],
        ]);

        assert_eq!(sources.read(WAIT).unwrap(), Some(report(&POWER_KEY)));
        assert_eq!(sources.source, Some(1));
        assert_eq!(sources.read(WAIT).unwrap(), Some(report(&[0x04])));
        let devices: Vec<usize> = log.borrow().iter().map(|&(device, _)| device).collect();
        // Collection 0 is read again after its wake report, until the
        // wait is up; then only collection 1.
        assert_eq!(devices, [0, 0, 1, 1]);
    }

    #[test]
    fn a_report_of_another_size_picks_nothing() {
        let (mut sources, _) = sources(vec![vec![Ok(POWER_KEY.to_vec())], vec![]]);

        assert_eq!(sources.read(WAIT).unwrap(), None);
        assert_eq!(sources.source, None);
        assert!(!sources.single());
    }

    #[test]
    fn a_dropped_report_does_not_end_the_wait() {
        let (mut sources, log) = sources(vec![vec![Ok(wake_report()), Ok(report(&POWER_KEY))]]);

        assert_eq!(sources.read(WAIT).unwrap(), Some(report(&POWER_KEY)));
        let log = log.borrow();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], (0, 250));
        assert!(log[1].1 <= 250);
    }

    #[test]
    fn dropped_reports_end_when_the_wait_is_up() {
        let flood = (0..1000).map(|_| Ok(wake_report())).collect();
        let (mut sources, log) = sources(vec![flood]);

        assert_eq!(sources.read(Duration::ZERO).unwrap(), None);
        assert_eq!(log.borrow().len(), 1);
    }

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
        let (mut sources, _) = sources(vec![vec![Ok(report(&POWER_KEY))]]);

        assert_eq!(sources.read(WAIT).unwrap(), Some(report(&POWER_KEY)));
    }

    #[test]
    fn the_first_collection_to_send_is_the_only_one_read_then() {
        let (mut sources, log) = sources(vec![
            vec![Ok(Vec::new()), Ok(report(&[0x04, 0x0A]))],
            vec![Ok(report(&POWER_KEY)), Ok(report(&[0x04]))],
        ]);

        assert_eq!(sources.read(WAIT).unwrap(), Some(report(&POWER_KEY)));
        assert_eq!(sources.read(WAIT).unwrap(), Some(report(&[0x04])));
        assert_eq!(sources.read(WAIT).unwrap(), None);
        assert_eq!(*log.borrow(), [(0, 250), (1, 250), (1, 250), (1, 250)]);
    }

    #[test]
    fn the_first_source_is_kept_when_it_is_the_first_collection() {
        let (mut sources, log) = sources(vec![
            vec![Ok(report(&POWER_KEY))],
            vec![Ok(report(&[0x04, 0x0A]))],
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
        let (mut sources, _) = sources(vec![vec![], vec![Ok(report(&POWER_KEY))]]);
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

        let picked = pick_key_collections(found.clone(), Some("b"), None);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].path, "b");
        assert_eq!(pick_key_collections(found.clone(), None, None).len(), 2);
        // A sender no longer listed (its path changed): all of them.
        assert_eq!(pick_key_collections(found, Some("gone"), None).len(), 2);
    }

    /// A key collection's path, as Windows lists it, with this `COLxx`.
    fn col_path(col: &str) -> String {
        format!(r"\\?\HID#VID_1532&PID_02C7&MI_01&{col}#7&1&0&0003#{{4d1e55b2}}")
    }

    #[test]
    fn the_profiles_collection_is_the_only_one_opened_before_any_sender() {
        // Ticket 209: Col05 sends a report at each wake, Col04 the keys.
        let found = [key_entry(&col_path("COL05")), key_entry(&col_path("COL04"))];
        let found: Vec<&HidInfo> = found.iter().collect();

        let picked = pick_key_collections(found.clone(), None, Some(4));

        assert_eq!(picked, [found[1]]);
    }

    #[test]
    fn the_last_sender_wins_over_the_profiles_collection() {
        let found = [key_entry(&col_path("COL05")), key_entry(&col_path("COL04"))];
        let found: Vec<&HidInfo> = found.iter().collect();
        let sender = found[0].path.clone();

        let picked = pick_key_collections(found.clone(), Some(&sender), Some(4));

        assert_eq!(picked, [found[0]]);
    }

    #[test]
    fn with_the_profiles_collection_not_listed_all_are_read() {
        let found = [key_entry(&col_path("COL05")), key_entry(&col_path("COL06"))];
        let found: Vec<&HidInfo> = found.iter().collect();

        assert_eq!(pick_key_collections(found.clone(), None, Some(4)), found);
    }

    #[test]
    fn a_profile_with_no_collection_reads_all() {
        let found = [key_entry(&col_path("COL04")), key_entry("no col")];
        let found: Vec<&HidInfo> = found.iter().collect();

        assert_eq!(pick_key_collections(found.clone(), None, None), found);
    }

    #[test]
    fn a_wait_too_long_is_the_longest() {
        let (mut sources, log) = sources(vec![vec![]]);

        sources.read(Duration::MAX).unwrap();
        assert_eq!(*log.borrow(), [(0, i32::MAX)]);
    }

    /// A HID path as Windows lists it, for this device part.
    fn path(device: &str) -> String {
        format!(r"\\?\HID#{device}#7&8e7fa24&0&0000#{{4d1e55b2-f16f-11cf-88cb-001111000030}}")
    }

    /// A HID layer with these collections, that logs each path it opens.
    struct FakeHid {
        collections: Vec<(String, io::Result<HidDetails>)>,
        /// The paths whose parent is not a USB device; the rest are.
        not_usb: Vec<String>,
        /// The paths whose parent can't be read.
        no_parent: Vec<String>,
        opened: RefCell<Vec<String>>,
    }

    impl FakeHid {
        fn new(collections: Vec<(String, io::Result<HidDetails>)>) -> Self {
            Self {
                collections,
                not_usb: Vec::new(),
                no_parent: Vec::new(),
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

        fn parent_id(&self, path: &str) -> io::Result<String> {
            if self.no_parent.iter().any(|known| known == path) {
                return Err(io::Error::other("no parent"));
            }
            Ok(if self.not_usb.iter().any(|known| known == path) {
                r"RZVIRTUAL\VID_1532&PID_02C7&MI_02".to_owned()
            } else {
                r"USB\VID_1532&PID_02C7&MI_02&2a1f".to_owned()
            })
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
    fn only_a_usb_parent_counts() {
        for (parent, usb) in [
            (r"USB\VID_1532&PID_02C7&MI_02\6&11f2b19&0&0002", true),
            (r"usb\vid_1532&pid_02c7&mi_02\1", true),
            (r"USBPRINT\VID_1532&PID_02C7\1", false),
            (r"RZVIRTUAL\VID_1532&PID_02C7&MI_02\1", false),
            ("USB", false),
            ("", false),
        ] {
            assert_eq!(on_usb(parent), usb, "{parent}");
        }
    }

    #[test]
    fn a_model_path_not_on_usb_or_with_no_parent_is_not_opened() {
        let usb = &razer_proto::BLADE_18.usb;
        let virtual_one = path("VID_1532&PID_02C7&MI_02&COL01");
        let real = path("VID_1532&PID_02C7&MI_02");
        let unknown = path("VID_1532&PID_02C7&MI_02&COL02");
        let mut hid = FakeHid::new(vec![
            (virtual_one.clone(), Ok(details(0x1532, 0x02C7, 1, 2))),
            (unknown.clone(), Ok(details(0x1532, 0x02C7, 1, 2))),
            (real.clone(), Ok(details(0x1532, 0x02C7, 1, 2))),
        ]);
        hid.not_usb = vec![virtual_one];
        hid.no_parent = vec![unknown];

        let infos = model_devices(&hid, usb).unwrap();

        assert_eq!(*hid.opened.borrow(), std::slice::from_ref(&real));
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].path, real);
    }

    #[test]
    fn a_short_read_is_an_error() {
        for read in [0, 1, REPORT_LEN] {
            let error = feature_report(&read_buffer(), read).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "{read}");
        }
    }
}
