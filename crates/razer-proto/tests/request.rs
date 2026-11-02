mod common;

use razer_proto::{BuildError, MAX_ARGS, Request};

/// Byte positions in a captured request.
const TRANSACTION_ID: usize = 1;
const DATA_SIZE: usize = 5;
const COMMAND_CLASS: usize = 6;
const COMMAND_ID: usize = 7;
const ARGS: usize = 8;

/// Rebuilds a captured request from its transaction ID, command and args.
fn rebuild(bytes: &[u8]) -> Request {
    let command = u16::from_be_bytes([bytes[COMMAND_CLASS], bytes[COMMAND_ID]]);
    let args = &bytes[ARGS..ARGS + usize::from(bytes[DATA_SIZE])];
    Request::new(bytes[TRANSACTION_ID], command, args).unwrap()
}

#[test]
fn every_captured_request_is_rebuilt_byte_for_byte() {
    for request in common::captured_requests() {
        assert_eq!(
            rebuild(&request.bytes).to_bytes(),
            request.bytes,
            "{}",
            request.fixture
        );
    }
}

#[test]
fn args_fill_all_80_bytes() {
    let args = [0xAB; MAX_ARGS];

    let bytes = Request::new(0, 0x030B, &args).unwrap().to_bytes();

    assert_eq!(&bytes[ARGS..ARGS + MAX_ARGS], &args[..]);
}

#[test]
fn more_than_80_args_is_an_error() {
    assert_eq!(
        Request::new(0, 0x030B, &[0; MAX_ARGS + 1]),
        Err(BuildError::TooManyArgs(MAX_ARGS + 1))
    );
}
