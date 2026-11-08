//! The main hardware rule for `Device::send`: whatever the controller replies,
//! one call writes its command once, never panics, and returns only a reply
//! that is a Success for its own request.
//! Tickets 45, 51, 53 and 55 each fixed one more reply case. This test tries
//! every short mix of replies, so a new case can't break the rule unseen.

mod common;

use common::{BUSY, FAILURE, Log, SUCCESS, fakes, reads, reply, request, sent};
use razer_device::{Device, DeviceError, MAX_BUSY_READS};
use razer_proto::{REPORT_LEN, Report, Status};

/// Get firmware version, as captured.
const COMMAND: u16 = 0x0081;
/// Not supported, a status the device does not retry on.
const NOT_SUPPORTED: u8 = 0x05;
/// Timeout in the other projects, an unknown status here.
const TIMEOUT: u8 = 0x04;
/// Replies per mix. A shorter mix ends in a read error (no reply left).
const MAX_REPLIES: u32 = 4;
/// The most reads one send may take: the first, one more for an older
/// reply, then up to `MAX_BUSY_READS - 1` reads again after Busy.
const MAX_READS: usize = 1 + MAX_BUSY_READS;

/// Each kind of reply the controller could give to request 0.
fn kinds() -> Vec<[u8; REPORT_LEN]> {
    let mut bad_checksum = reply(SUCCESS, 0, COMMAND);
    bad_checksum[88] ^= 0xFF;
    vec![
        reply(SUCCESS, 0, COMMAND),
        reply(BUSY, 0, COMMAND),
        reply(FAILURE, 0, COMMAND),
        reply(NOT_SUPPORTED, 0, COMMAND),
        reply(TIMEOUT, 0, COMMAND),
        reply(0x7F, 0, COMMAND),
        bad_checksum,
        // The reply to the request before, read too soon.
        reply(SUCCESS, 31, 0x0D82),
        reply(BUSY, 31, COMMAND),
        // Another transaction ID, and another command.
        reply(SUCCESS, 7, COMMAND),
        reply(SUCCESS, 0, 0x0D82),
        reply(BUSY, 0, 0x0D82),
    ]
}

/// Every mix of `kinds()` with 0 to `MAX_REPLIES` replies.
fn mixes() -> Vec<Vec<[u8; REPORT_LEN]>> {
    let kinds = kinds();
    let mut all = vec![vec![]];
    let mut last = vec![vec![]];
    for _ in 0..MAX_REPLIES {
        last = last
            .iter()
            .flat_map(|mix: &Vec<_>| {
                kinds.iter().map(move |kind| {
                    let mut longer = mix.clone();
                    longer.push(*kind);
                    longer
                })
            })
            .collect();
        all.extend(last.iter().cloned());
    }
    all
}

/// Long mixes that reach the Busy limit, after an older reply or not.
fn busy_mixes() -> Vec<Vec<[u8; REPORT_LEN]>> {
    let busy = reply(BUSY, 0, COMMAND);
    let older = reply(SUCCESS, 31, 0x0D82);
    let mut all = Vec::new();
    for first in [vec![], vec![older]] {
        for count in [MAX_BUSY_READS, MAX_BUSY_READS + 3] {
            let mut mix = first.clone();
            mix.extend(std::iter::repeat_n(busy, count));
            all.push(mix.clone());
            mix.push(reply(SUCCESS, 0, COMMAND));
            all.push(mix);
        }
    }
    all
}

/// Status, transaction ID and command of each reply, for a failure message.
fn short(mix: &[[u8; REPORT_LEN]]) -> Vec<(u8, u8, u16)> {
    mix.iter()
        .map(|r| (r[0], r[1], u16::from_be_bytes([r[6], r[7]])))
        .collect()
}

#[test]
fn every_mix_of_replies_sends_the_command_once() {
    let mixes = mixes();
    assert_eq!(mixes.len(), 1 + 12 + 144 + 1728 + 20736);

    for mix in mixes.into_iter().chain(busy_mixes()) {
        let (result, log) = send(&mix);

        assert_eq!(sent(&log), [request(0, COMMAND, &[])], "{:?}", short(&mix));
        assert!(
            reads(&log) <= MAX_READS,
            "too many reads: {:?}",
            short(&mix)
        );
        if let Ok(report) = result {
            assert_eq!(report.status(), Status::Success, "{:?}", short(&mix));
            assert_eq!(report.transaction_id(), 0, "{:?}", short(&mix));
            assert_eq!(report.command(), COMMAND, "{:?}", short(&mix));
        }
    }
}

/// So the test above can't pass by refusing every reply.
#[test]
fn a_success_reply_still_succeeds_after_an_older_reply_or_not() {
    let success = reply(SUCCESS, 0, COMMAND);
    let older = reply(SUCCESS, 31, 0x0D82);
    for mix in [vec![success], vec![older, success]] {
        let (result, _) = send(&mix);
        assert!(result.is_ok(), "{:?}: {result:?}", short(&mix));
    }
}

/// At the Busy limit the controller may still be working, so the error says Busy.
#[test]
fn the_busy_limit_is_a_busy_error() {
    for mix in busy_mixes() {
        let (result, _) = send(&mix);
        assert!(
            matches!(result, Err(DeviceError::Busy)),
            "{:?}: {result:?}",
            short(&mix)
        );
    }
}

/// Sends the command once to a fake that replays `mix`.
fn send(mix: &[[u8; REPORT_LEN]]) -> (Result<Report, DeviceError>, Log) {
    let (transport, clock, log) = fakes(mix.to_vec());
    let result = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);
    (result, log)
}
