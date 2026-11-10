//! The real [`Transport`]: feature reports through hidapi. Also the key
//! reports the keyboard sends on its own.

use std::io;
use std::time::Duration;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use razer_proto::{ModelProfile, REPORT_LEN};

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
        let api = HidApi::new().map_err(io::Error::other)?;
        let devices: Vec<HidInfo> = api.device_list().map(hid_info).collect();
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
    devices: Vec<HidDevice>,
    /// The collection that sent the first report. Only that one is read
    /// from then on, so a press can't come in twice or out of order.
    source: Option<usize>,
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
        let api = HidApi::new().map_err(io::Error::other)?;
        let infos: Vec<HidInfo> = api.device_list().map(hid_info).collect();
        let devices: Vec<HidDevice> = find_key_collections(&infos, &model.usb, &key)
            .into_iter()
            .filter_map(|info| api.open_path(&info.path).ok())
            .collect();
        if devices.is_empty() {
            return Err(not_found());
        }
        Ok(Self {
            devices,
            source: None,
        })
    }

    /// Waits up to `wait` for a report: on each collection in turn until
    /// one sends, then on that one alone. `None` if none came.
    ///
    /// # Errors
    ///
    /// Any hidapi error, such as the keyboard going away, or the collection
    /// that sent last no longer being in the list (it can't today: the list
    /// never changes after `open`).
    pub fn read(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
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
            let read = device
                .read_timeout(&mut buffer, millis)
                .map_err(io::Error::other)?;
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
        let mut buffer = [REPORT_ID; REPORT_LEN + 1];
        buffer[1..].copy_from_slice(report);
        self.device
            .send_feature_report(&buffer)
            .map_err(io::Error::other)
    }

    fn read_report(&mut self) -> io::Result<[u8; REPORT_LEN]> {
        let mut buffer = [REPORT_ID; REPORT_LEN + 1];
        let read = self
            .device
            .get_feature_report(&mut buffer)
            .map_err(io::Error::other)?;
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
}
