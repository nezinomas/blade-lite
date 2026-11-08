mod common;

use common::{BUSY, Event, FAILURE, SUCCESS, fakes, reads, reply, request, sent};
use razer_device::{BUSY_PAUSE, Device, DeviceError, MAX_BUSY_READS, PAUSE};
use razer_proto::Status;

/// Get firmware version, as captured.
const COMMAND: u16 = 0x0081;

#[test]
fn a_success_reply_is_returned() {
    let (transport, clock, _) = fakes(vec![reply(SUCCESS, 0, COMMAND)]);

    let report = Device::with_first_id(transport, clock, 0)
        .send(COMMAND, &[])
        .unwrap();

    assert_eq!(report.status(), Status::Success);
}

#[test]
fn the_device_pauses_between_send_and_read() {
    let (transport, clock, log) = fakes(vec![reply(SUCCESS, 0, COMMAND)]);

    Device::with_first_id(transport, clock, 0)
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

/// Busy may mean the controller took the command and is still working on it,
/// so the command is never sent again: the reply is read again after the
/// longer Busy pause, so a slow command has time to finish.
/// `OpenRazer` and `razer-laptop-control` do not resend on Busy either.
#[test]
fn a_busy_reply_is_read_again_after_the_busy_pause_without_a_resend() {
    let (transport, clock, log) = fakes(vec![reply(BUSY, 0, COMMAND), reply(SUCCESS, 0, COMMAND)]);

    let report = Device::with_first_id(transport, clock, 0)
        .send(COMMAND, &[])
        .unwrap();

    assert_eq!(report.status(), Status::Success);
    assert_eq!(
        *log.borrow(),
        [
            Event::Sent(request(0, COMMAND, &[])),
            Event::Slept(PAUSE),
            Event::Read,
            Event::Slept(BUSY_PAUSE),
            Event::Read
        ]
    );
}

#[test]
fn the_last_busy_read_can_still_succeed() {
    let mut replies = vec![reply(BUSY, 0, COMMAND); MAX_BUSY_READS - 1];
    replies.push(reply(SUCCESS, 0, COMMAND));
    let (transport, clock, log) = fakes(replies);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(sent(&log).len(), 1);
    assert_eq!(reads(&log), MAX_BUSY_READS);
}

#[test]
fn the_next_send_takes_the_next_id_after_a_busy_reply() {
    let (transport, clock, log) = fakes(vec![
        reply(BUSY, 0, COMMAND),
        reply(SUCCESS, 0, COMMAND),
        reply(SUCCESS, 1, COMMAND),
    ]);
    let mut device = Device::with_first_id(transport, clock, 0);

    device.send(COMMAND, &[]).unwrap();
    device.send(COMMAND, &[]).unwrap();

    let ids: Vec<u8> = sent(&log).iter().map(|bytes| bytes[1]).collect();
    assert_eq!(ids, [0, 1]);
}

#[test]
fn too_many_busy_replies_are_an_error() {
    let (transport, clock, log) = fakes(vec![reply(BUSY, 0, COMMAND); MAX_BUSY_READS]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(matches!(result, Err(DeviceError::Busy)), "{result:?}");
    assert_eq!(sent(&log).len(), 1);
    assert_eq!(reads(&log), MAX_BUSY_READS);
}

#[test]
fn a_busy_reply_then_a_failure_is_rejected() {
    let (transport, clock, log) = fakes(vec![reply(BUSY, 0, COMMAND), reply(FAILURE, 0, COMMAND)]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::Rejected(Status::Failure))),
        "{result:?}"
    );
    assert_eq!(sent(&log).len(), 1);
}

#[test]
fn a_failure_reply_is_an_error() {
    let (transport, clock, _) = fakes(vec![reply(FAILURE, 0, COMMAND)]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(matches!(
        result,
        Err(DeviceError::Rejected(Status::Failure))
    ));
}

#[test]
fn transaction_ids_count_0_to_31_then_wrap() {
    let replies = (0..33).map(|id| reply(SUCCESS, id % 32, COMMAND)).collect();
    let (transport, clock, log) = fakes(replies);
    let mut device = Device::with_first_id(transport, clock, 0);

    for _ in 0..33 {
        device.send(COMMAND, &[]).unwrap();
    }

    let ids: Vec<u8> = sent(&log).iter().map(|bytes| bytes[1]).collect();
    let expected: Vec<u8> = (0..32).chain([0]).collect();
    assert_eq!(ids, expected);
}

#[test]
fn a_reply_with_another_transaction_id_twice_is_an_error() {
    let (transport, clock, log) = fakes(vec![reply(SUCCESS, 7, COMMAND); 2]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

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
    assert_eq!(reads(&log), 2);
}

#[test]
fn a_reply_to_another_command_is_an_error() {
    let (transport, clock, log) = fakes(vec![reply(SUCCESS, 0, 0x0D82)]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

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
    assert_eq!(sent(&log).len(), 1);
}

#[test]
fn a_busy_reply_to_another_request_is_not_sent_again() {
    let (transport, clock, log) = fakes(vec![reply(BUSY, 3, COMMAND); 2]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::WrongReply { .. })),
        "{result:?}"
    );
    assert_eq!(sent(&log).len(), 1);
    assert_eq!(reads(&log), 2);
}

/// A reply read too soon is the one to the request before. The device pauses
/// and reads once more, and does not send the command again.
#[test]
fn an_older_reply_is_read_again_once_after_the_pause() {
    let (transport, clock, log) =
        fakes(vec![reply(SUCCESS, 31, 0x0D82), reply(SUCCESS, 0, COMMAND)]);

    let report = Device::with_first_id(transport, clock, 0)
        .send(COMMAND, &[])
        .unwrap();

    assert_eq!(report.transaction_id(), 0);
    assert_eq!(
        *log.borrow(),
        [
            Event::Sent(request(0, COMMAND, &[])),
            Event::Slept(PAUSE),
            Event::Read,
            Event::Slept(PAUSE),
            Event::Read
        ]
    );
}

#[test]
fn a_busy_reply_after_an_older_reply_is_read_again() {
    let (transport, clock, log) = fakes(vec![
        reply(SUCCESS, 31, COMMAND),
        reply(BUSY, 0, COMMAND),
        reply(SUCCESS, 0, COMMAND),
    ]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(sent(&log).len(), 1);
    assert_eq!(reads(&log), 3);
}

/// The same transaction ID with another command is not a reply read too soon.
#[test]
fn a_reply_to_another_command_is_not_read_again() {
    let (transport, clock, log) =
        fakes(vec![reply(SUCCESS, 0, 0x0D82), reply(SUCCESS, 0, COMMAND)]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::WrongReply { .. })),
        "{result:?}"
    );
    assert_eq!(reads(&log), 1);
}

#[test]
fn a_failure_reply_to_another_request_is_a_wrong_reply() {
    let (transport, clock, log) = fakes(vec![reply(FAILURE, 0, 0x0D82)]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(
        matches!(result, Err(DeviceError::WrongReply { .. })),
        "{result:?}"
    );
    assert_eq!(sent(&log).len(), 1);
}

/// After a Busy reply, the reply read again must still echo the request.
#[test]
fn a_reply_read_again_after_busy_must_still_match() {
    let (transport, clock, log) = fakes(vec![
        reply(BUSY, 0, COMMAND),
        reply(SUCCESS, 2, COMMAND),
        reply(SUCCESS, 0, COMMAND),
    ]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(
        matches!(
            result,
            Err(DeviceError::WrongReply {
                transaction_id: 2,
                ..
            })
        ),
        "{result:?}"
    );
    assert_eq!(sent(&log).len(), 1);
    assert_eq!(reads(&log), 2);
}

/// This model sets the remaining packets (bytes 2 and 3) in some replies,
/// such as `00 02` for `078C`, so they must not be compared.
#[test]
fn remaining_packets_in_a_reply_are_not_compared() {
    let mut bytes = reply(SUCCESS, 0, COMMAND);
    bytes[2..4].copy_from_slice(&[0x00, 0x02]);
    bytes[razer_proto::CHECKSUM_INDEX] = razer_proto::checksum(&bytes);
    let (transport, clock, _) = fakes(vec![bytes]);

    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn the_first_id_can_be_chosen_and_wraps_after_31() {
    let (transport, clock, log) = fakes(vec![
        reply(SUCCESS, 31, COMMAND),
        reply(SUCCESS, 0, COMMAND),
    ]);
    let mut device = Device::with_first_id(transport, clock, 31);

    device.send(COMMAND, &[]).unwrap();
    device.send(COMMAND, &[]).unwrap();

    let ids: Vec<u8> = sent(&log).iter().map(|bytes| bytes[1]).collect();
    assert_eq!(ids, [31, 0]);
}

/// A first ID above 31 is never sent: it is taken modulo 32.
#[test]
fn a_first_id_above_31_wraps() {
    let (transport, clock, log) = fakes(vec![reply(SUCCESS, 8, COMMAND)]);

    Device::with_first_id(transport, clock, 40)
        .send(COMMAND, &[])
        .unwrap();

    assert_eq!(sent(&log)[0][1], 8);
}

/// A new device does not always start at ID 0, so a reply left in the
/// controller by an earlier run is less likely to match its first request.
#[test]
fn new_devices_start_at_different_ids_below_32() {
    let first_ids: std::collections::BTreeSet<u8> = (0..64)
        .map(|_| {
            let (transport, clock, log) = fakes(vec![]);
            let _ = Device::new(transport, clock).send(COMMAND, &[]);
            sent(&log)[0][1]
        })
        .collect();

    assert!(first_ids.len() > 1, "{first_ids:?}");
    assert!(first_ids.iter().all(|&id| id < 32), "{first_ids:?}");
}
