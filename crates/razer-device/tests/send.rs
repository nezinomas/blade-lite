mod common;

use common::{BUSY, Event, FAILURE, SUCCESS, fakes, reply, request, sent};
use razer_device::{Device, DeviceError, MAX_ATTEMPTS, PAUSE};
use razer_proto::Status;

/// Get firmware version, as captured.
const COMMAND: u16 = 0x0081;

#[test]
fn a_success_reply_is_returned() {
    let (transport, clock, _) = fakes(vec![reply(SUCCESS, 0, COMMAND)]);

    let report = Device::new(transport, clock).send(COMMAND, &[]).unwrap();

    assert_eq!(report.status(), Status::Success);
}

#[test]
fn the_device_pauses_between_send_and_read() {
    let (transport, clock, log) = fakes(vec![reply(SUCCESS, 0, COMMAND)]);

    Device::new(transport, clock)
        .send(COMMAND, &[1, 2])
        .unwrap();

    assert_eq!(
        *log.borrow(),
        [
            Event::Sent(request(0, COMMAND, &[1, 2])),
            Event::Slept(PAUSE),
            Event::Read
        ]
    );
}

#[test]
fn a_busy_reply_is_retried_after_the_pause() {
    let (transport, clock, log) = fakes(vec![reply(BUSY, 0, COMMAND), reply(SUCCESS, 0, COMMAND)]);

    let report = Device::new(transport, clock).send(COMMAND, &[]).unwrap();

    assert_eq!(report.status(), Status::Success);
    assert_eq!(sent(&log), [request(0, COMMAND, &[]); 2]);
    assert_eq!(log.borrow()[3], Event::Slept(PAUSE));
}

#[test]
fn too_many_busy_replies_are_an_error() {
    let busy = vec![reply(BUSY, 0, COMMAND); MAX_ATTEMPTS];
    let (transport, clock, log) = fakes(busy);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(matches!(result, Err(DeviceError::Busy)));
    assert_eq!(sent(&log).len(), MAX_ATTEMPTS);
}

#[test]
fn a_failure_reply_is_an_error() {
    let (transport, clock, _) = fakes(vec![reply(FAILURE, 0, COMMAND)]);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(matches!(
        result,
        Err(DeviceError::Rejected(Status::Failure))
    ));
}

#[test]
fn transaction_ids_count_0_to_31_then_wrap() {
    let replies = (0..33).map(|id| reply(SUCCESS, id % 32, COMMAND)).collect();
    let (transport, clock, log) = fakes(replies);
    let mut device = Device::new(transport, clock);

    for _ in 0..33 {
        device.send(COMMAND, &[]).unwrap();
    }

    let ids: Vec<u8> = sent(&log).iter().map(|bytes| bytes[1]).collect();
    let expected: Vec<u8> = (0..32).chain([0]).collect();
    assert_eq!(ids, expected);
}

#[test]
fn a_reply_with_another_transaction_id_is_an_error() {
    let (transport, clock, log) = fakes(vec![reply(SUCCESS, 7, COMMAND)]);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(
        matches!(
            result,
            Err(DeviceError::WrongReply {
                transaction_id: 7,
                command: COMMAND
            })
        ),
        "{result:?}"
    );
    assert_eq!(sent(&log).len(), 1);
}

#[test]
fn a_reply_to_another_command_is_an_error() {
    let (transport, clock, _) = fakes(vec![reply(SUCCESS, 0, 0x0D82)]);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(
        matches!(
            result,
            Err(DeviceError::WrongReply {
                transaction_id: 0,
                command: 0x0D82
            })
        ),
        "{result:?}"
    );
}

#[test]
fn a_busy_reply_to_another_request_is_not_retried() {
    let (transport, clock, log) = fakes(vec![reply(BUSY, 3, COMMAND), reply(SUCCESS, 0, COMMAND)]);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::WrongReply { .. })),
        "{result:?}"
    );
    assert_eq!(sent(&log).len(), 1);
}

#[test]
fn a_failure_reply_to_another_request_is_a_wrong_reply() {
    let (transport, clock, _) = fakes(vec![reply(FAILURE, 0, 0x0D82)]);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::WrongReply { .. })),
        "{result:?}"
    );
}

#[test]
fn a_busy_retry_must_still_match() {
    let (transport, clock, _) = fakes(vec![reply(BUSY, 0, COMMAND), reply(SUCCESS, 1, COMMAND)]);

    let result = Device::new(transport, clock).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::WrongReply { .. })),
        "{result:?}"
    );
}
