//! The main hardware rule for `Device::send`: whatever the controller replies,
//! one call writes its command once, and never panics.
//! Tickets 45, 51, 53 and 55 each fixed one more reply case. This test tries
//! every short mix of replies, so a new case can't break the rule unseen.

mod common;

use common::{BUSY, FAILURE, SUCCESS, fakes, reads, reply, request, sent};
use razer_device::Device;
use razer_proto::REPORT_LEN;

/// Get firmware version, as captured.
const COMMAND: u16 = 0x0081;
/// Not supported, a status the device does not retry on.
const NOT_SUPPORTED: u8 = 0x05;
/// Replies per mix. A shorter mix ends in a read error (no reply left).
const MAX_REPLIES: u32 = 4;

/// Each kind of reply the controller could give to request 0.
fn kinds() -> Vec<[u8; REPORT_LEN]> {
    vec![
        reply(SUCCESS, 0, COMMAND),
        reply(BUSY, 0, COMMAND),
        reply(FAILURE, 0, COMMAND),
        reply(NOT_SUPPORTED, 0, COMMAND),
        // The reply to the request before, read too soon.
        reply(SUCCESS, 31, 0x0D82),
        reply(BUSY, 31, COMMAND),
        // Another transaction ID, and another command.
        reply(SUCCESS, 7, COMMAND),
        reply(SUCCESS, 0, 0x0D82),
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

#[test]
fn every_mix_of_replies_sends_the_command_once() {
    let mixes = mixes();
    assert_eq!(mixes.len(), 1 + 8 + 64 + 512 + 4096);

    for mix in mixes {
        let (transport, clock, log) = fakes(mix.clone());

        let _ = Device::with_first_id(transport, clock, 0).send(COMMAND, &[]);

        assert_eq!(
            sent(&log),
            [request(0, COMMAND, &[])],
            "replies {:?}",
            mix.iter().map(|r| (r[0], r[1])).collect::<Vec<_>>()
        );
        assert!(reads(&log) <= mix.len() + 1, "read past the end: {mix:?}");
    }
}
