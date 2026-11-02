//! The real [`Transport`]: feature reports through hidapi.

use std::io;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use razer_proto::REPORT_LEN;

use crate::{HidInfo, Transport, find_controller};

/// The controller does not number its reports, so each one starts with ID 0.
const REPORT_ID: u8 = 0;

/// The controller, opened through hidapi.
pub struct HidTransport {
    device: HidDevice,
    info: HidInfo,
}

impl HidTransport {
    /// Finds the controller among the HID devices and opens it.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotFound`] if there is no controller, or any hidapi error.
    pub fn open() -> io::Result<Self> {
        let api = HidApi::new().map_err(io::Error::other)?;
        let devices: Vec<HidInfo> = api.device_list().map(hid_info).collect();
        let info = find_controller(&devices)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Blade 18 controller"))?;
        let device = api.open_path(&info.path).map_err(io::Error::other)?;
        Ok(Self { device, info })
    }

    /// The collection that was opened.
    #[must_use]
    pub fn info(&self) -> &HidInfo {
        &self.info
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
