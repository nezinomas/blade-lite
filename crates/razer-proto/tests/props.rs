//! Property tests: any input, never a panic; values out of range refused.

use proptest::prelude::*;
use razer_proto::{
    BLADE_18, BuildError, CHECKSUM_INDEX, CpuBoost, CustomError, GpuBoost, MAX_ARGS, REPORT_LEN,
    Report, Request, Rgb, Rpm, checksum,
};

/// The status byte of a reply that says Success.
const SUCCESS: u8 = 0x02;

fn report() -> impl Strategy<Value = [u8; REPORT_LEN]> {
    prop::collection::vec(any::<u8>(), REPORT_LEN)
        .prop_map(|bytes| bytes.try_into().expect("REPORT_LEN bytes"))
}

proptest! {
    #[test]
    fn the_checksum_is_the_xor_of_bytes_2_to_87(bytes in report()) {
        let expected = bytes[2..CHECKSUM_INDEX].iter().fold(0, |acc, byte| acc ^ byte);
        prop_assert_eq!(checksum(&bytes), expected);
    }

    #[test]
    fn any_reply_bytes_parse_or_fail_without_a_panic(bytes in report()) {
        if let Ok(reply) = Report::parse(&bytes) {
            prop_assert!(reply.args().len() <= MAX_ARGS);
            prop_assert_eq!(reply.args().len(), usize::from(bytes[5]));
        }
    }

    #[test]
    fn a_reply_with_a_wrong_checksum_is_refused(mut bytes in report(), flip in 1..=u8::MAX) {
        bytes[CHECKSUM_INDEX] = checksum(&bytes) ^ flip;
        prop_assert!(Report::parse(&bytes).is_err());
    }

    #[test]
    fn a_built_request_parses_back_to_the_same_command(
        transaction_id in any::<u8>(),
        command in any::<u16>(),
        args in prop::collection::vec(any::<u8>(), 0..=MAX_ARGS),
    ) {
        let mut bytes = Request::new(transaction_id, command, &args).unwrap().to_bytes();
        // The status byte is outside the checksum, so a reply status keeps it valid.
        bytes[0] = SUCCESS;
        let reply = Report::parse(&bytes).unwrap();
        prop_assert_eq!(reply.transaction_id(), transaction_id);
        prop_assert_eq!(reply.command(), command);
        prop_assert_eq!(reply.args(), args.as_slice());
    }

    #[test]
    fn more_args_than_fit_are_refused(len in MAX_ARGS + 1..1024_usize) {
        prop_assert_eq!(Request::new(0, 0, &vec![0; len]), Err(BuildError::TooManyArgs(len)));
    }

    #[test]
    fn any_text_parses_as_a_colour_or_fails_without_a_panic(text in any::<String>()) {
        let _ = text.parse::<Rgb>();
    }

    #[test]
    fn hash_and_six_chars_parse_or_fail_without_a_panic(text in "#.{6}") {
        let _ = text.parse::<Rgb>();
    }

    #[test]
    fn every_colour_reads_back_from_its_hex(red in any::<u8>(), green in any::<u8>(), blue in any::<u8>()) {
        let colour = Rgb::new(red, green, blue);
        prop_assert_eq!(format!("#{red:02X}{green:02X}{blue:02x}").parse(), Ok(colour));
    }

    #[test]
    fn a_fan_speed_is_taken_only_in_range_and_in_hundreds(rpm in any::<u16>()) {
        let range = BLADE_18.custom.fan_rpm;
        let valid = rpm % 100 == 0 && (range.min..=range.max).contains(&rpm);
        prop_assert_eq!(Rpm::new(rpm, &BLADE_18).is_ok(), valid);
    }

    #[test]
    fn a_boost_is_taken_only_up_to_the_model_max(level in any::<u8>()) {
        let limits = BLADE_18.custom;
        let cpu = CpuBoost::new(level, &BLADE_18);
        let gpu = GpuBoost::new(level, &BLADE_18);
        prop_assert_eq!(cpu.is_ok(), level <= limits.cpu_max);
        prop_assert_eq!(gpu.is_ok(), level <= limits.gpu_max);
        if level > limits.cpu_max {
            prop_assert_eq!(cpu, Err(CustomError::BoostTooHigh(level)));
        }
    }
}
