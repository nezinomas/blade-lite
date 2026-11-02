mod common;

use common::{SUCCESS, fakes, reply, reply_with_args, request, sent};
use razer_device::{Device, DeviceError, FirmwareVersion};

/// Get firmware version, as in r-helper.
const COMMAND: u16 = 0x0081;

#[test]
fn the_firmware_version_is_read_from_the_reply_args() {
    let (transport, clock, log) = fakes(vec![reply_with_args(0, COMMAND, &[1, 7])]);

    let version = Device::new(transport, clock).firmware_version().unwrap();

    assert_eq!(version, FirmwareVersion { major: 1, minor: 7 });
    assert_eq!(sent(&log), [request(0, COMMAND, &[0, 0])]);
}

#[test]
fn a_reply_without_two_args_is_an_error() {
    let (transport, clock, _) = fakes(vec![reply(SUCCESS, 0, COMMAND)]);

    let error = Device::new(transport, clock)
        .firmware_version()
        .unwrap_err();

    assert!(matches!(error, DeviceError::ShortReply), "{error:?}");
}
