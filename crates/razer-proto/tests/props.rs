//! Property tests: any input, never a panic; values out of range refused.

use proptest::prelude::*;
use razer_proto::{
    BLADE_18, Brightness, BrightnessError, BuildError, CHECKSUM_INDEX, Celsius, ChargeLimit,
    ChargeLimitError, CpuBoost, Custom, CustomError, FanMode, FanRpm, GpuBoost, MAX_ARGS,
    NativeMode, ParseError, PerfMode, REPORT_LEN, Report, Request, Rgb, Rpm, checksum,
};

/// Byte positions in a report.
const STATUS: usize = 0;
const DATA_SIZE: usize = 5;
const COMMAND: usize = 6;
const ARGS: usize = 8;
/// The known status bytes: Busy, Success, Failure, `NotSupported`.
const KNOWN_STATUS: [u8; 4] = [0x01, 0x02, 0x03, 0x05];
const SUCCESS: u8 = 0x02;
const SET_FAN: u16 = 0x0D01;
const SET_PRESET: u16 = 0x0D02;
const SET_BOOST: u16 = 0x0D07;

fn report() -> impl Strategy<Value = [u8; REPORT_LEN]> {
    prop::collection::vec(any::<u8>(), REPORT_LEN)
        .prop_map(|bytes| bytes.try_into().expect("REPORT_LEN bytes"))
}

/// A report with the given status byte and a valid checksum.
fn signed(mut bytes: [u8; REPORT_LEN], status: u8) -> [u8; REPORT_LEN] {
    bytes[STATUS] = status;
    bytes[CHECKSUM_INDEX] = checksum(&bytes);
    bytes
}

fn reply_args() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..=MAX_ARGS)
}

proptest! {
    #[test]
    fn the_checksum_ignores_bytes_outside_2_to_87(
        bytes in report(),
        index in prop::sample::select(vec![0, 1, CHECKSUM_INDEX, REPORT_LEN - 1]),
        value in any::<u8>(),
    ) {
        let mut changed = bytes;
        changed[index] = value;
        prop_assert_eq!(checksum(&changed), checksum(&bytes));
    }

    #[test]
    fn the_checksum_changes_with_any_bit_of_bytes_2_to_87(
        bytes in report(),
        index in 2..CHECKSUM_INDEX,
        bit in 0..8_u8,
    ) {
        let mut changed = bytes;
        changed[index] ^= 1 << bit;
        prop_assert_ne!(checksum(&changed), checksum(&bytes));
    }

    #[test]
    fn any_reply_bytes_parse_or_fail_without_a_panic(bytes in report()) {
        let _ = Report::parse(&bytes);
    }

    #[test]
    fn a_signed_reply_parses_exactly_when_its_data_size_fits(
        bytes in report(),
        status in prop::sample::select(KNOWN_STATUS.to_vec()),
    ) {
        let bytes = signed(bytes, status);
        let size = bytes[DATA_SIZE];
        match Report::parse(&bytes) {
            Ok(reply) => {
                prop_assert!(usize::from(size) <= MAX_ARGS);
                prop_assert_eq!(reply.args(), &bytes[ARGS..ARGS + usize::from(size)]);
            }
            Err(error) => {
                prop_assert!(usize::from(size) > MAX_ARGS);
                prop_assert_eq!(error, ParseError::BadDataSize(size));
            }
        }
    }

    #[test]
    fn an_unknown_status_is_refused(bytes in report(), status in any::<u8>()) {
        prop_assume!(!KNOWN_STATUS.contains(&status));
        prop_assert_eq!(
            Report::parse(&signed(bytes, status)),
            Err(ParseError::UnknownStatus(status))
        );
    }

    #[test]
    fn a_reply_with_a_wrong_checksum_is_refused(mut bytes in report(), flip in 1..=u8::MAX) {
        bytes[CHECKSUM_INDEX] = checksum(&bytes) ^ flip;
        let refused = matches!(Report::parse(&bytes), Err(ParseError::BadChecksum { .. }));
        prop_assert!(refused);
    }

    #[test]
    fn a_built_request_has_the_header_sent_and_parses_back(
        transaction_id in any::<u8>(),
        command in any::<u16>(),
        args in reply_args(),
    ) {
        let mut bytes = Request::new(transaction_id, command, &args).unwrap().to_bytes();
        prop_assert_eq!(bytes[STATUS], 0);
        prop_assert_eq!(bytes[1], transaction_id);
        prop_assert_eq!(&bytes[2..DATA_SIZE], &[0, 0, 0]);
        prop_assert_eq!(usize::from(bytes[DATA_SIZE]), args.len());
        prop_assert_eq!(&bytes[COMMAND..ARGS], &command.to_be_bytes());
        prop_assert_eq!(&bytes[ARGS..ARGS + args.len()], args.as_slice());
        prop_assert!(bytes[ARGS + args.len()..CHECKSUM_INDEX].iter().all(|&byte| byte == 0));
        prop_assert_eq!(bytes[REPORT_LEN - 1], 0);

        // The status byte is outside the checksum, so a reply status keeps it valid.
        bytes[STATUS] = SUCCESS;
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
    fn only_hash_and_six_hex_digits_parse_as_a_colour(text in "#?[0-9a-fA-F+\\- gGé]{4,8}") {
        let valid = text.len() == 7
            && text.starts_with('#')
            && text[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
        let parsed = text.parse::<Rgb>();
        prop_assert_eq!(parsed.is_ok(), valid);
        if let Ok(colour) = parsed {
            let channel = |at: usize| u8::from_str_radix(&text[at..at + 2], 16).unwrap();
            prop_assert_eq!(colour, Rgb::new(channel(1), channel(3), channel(5)));
        }
    }

    #[test]
    fn every_colour_reads_back_from_its_hex(
        red in any::<u8>(),
        green in any::<u8>(),
        blue in any::<u8>(),
    ) {
        let text = format!("#{red:02X}{green:02X}{blue:02x}");
        prop_assert_eq!(text.parse(), Ok(Rgb::new(red, green, blue)));
    }

    #[test]
    fn a_fan_speed_is_taken_only_in_range_and_in_hundreds(rpm in any::<u16>()) {
        let range = BLADE_18.custom.fan_rpm;
        let valid = rpm % 100 == 0 && (range.min..=range.max).contains(&rpm);
        prop_assert_eq!(Rpm::new(rpm, &BLADE_18).is_ok(), valid);
    }

    #[test]
    fn a_manual_fan_speed_is_sent_as_rpm_over_100_after_the_mode_and_boosts(
        hundreds in BLADE_18.custom.fan_rpm.min.div_ceil(100)..=BLADE_18.custom.fan_rpm.max / 100,
    ) {
        let rpm = hundreds * 100;
        let custom = Custom {
            cpu: CpuBoost::new(0, &BLADE_18).unwrap(),
            gpu: GpuBoost::new(0, &BLADE_18).unwrap(),
            fan: FanMode::Manual(Rpm::new(rpm, &BLADE_18).unwrap()),
        };
        let commands = custom.commands(&BLADE_18);
        let fans: Vec<_> = commands.iter().filter(|command| command.id == SET_FAN).collect();
        let zones = BLADE_18.custom.fan_zones;
        prop_assert_eq!(fans.len(), zones.len());
        for (command, &zone) in fans.iter().zip(zones) {
            prop_assert_eq!(&command.args, &vec![0x01, zone, u8::try_from(hundreds).unwrap()]);
        }
        let modes: Vec<_> = commands.iter().filter(|command| command.id == SET_PRESET).collect();
        prop_assert_eq!(modes.len(), BLADE_18.perf_zones.len());
        for (command, &zone) in modes.iter().zip(BLADE_18.perf_zones) {
            // Custom mode, with the manual fan flag.
            prop_assert_eq!(&command.args, &vec![0x01, zone, 0x04, 0x01]);
        }
        let last = |id| commands.iter().rposition(|command| command.id == id).unwrap();
        let first_fan = commands.iter().position(|command| command.id == SET_FAN).unwrap();
        prop_assert!(last(SET_PRESET) < first_fan);
        prop_assert!(last(SET_BOOST) < first_fan);
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
        if level > limits.gpu_max {
            prop_assert_eq!(gpu, Err(CustomError::BoostTooHigh(level)));
        }
    }

    #[test]
    fn a_brightness_is_taken_only_up_to_100_percent(percent in any::<u8>()) {
        match Brightness::new(percent) {
            Ok(brightness) => {
                prop_assert!(percent <= 100);
                prop_assert_eq!(brightness.percent(), percent);
            }
            Err(error) => {
                prop_assert!(percent > 100);
                prop_assert_eq!(error, BrightnessError::OutOfRange(percent));
            }
        }
    }

    #[test]
    fn a_brightness_level_rises_with_the_percent_and_reads_back(percent in 0..100_u8) {
        let lower = Brightness::new(percent).unwrap();
        let higher = Brightness::new(percent + 1).unwrap();
        prop_assert!(lower.level() <= higher.level());
        let led = BLADE_18.brightness_leds[0];
        for brightness in [lower, higher] {
            let read = Brightness::from_reply(&BLADE_18, &[0x01, led, brightness.level()]);
            prop_assert_eq!(read, Some(brightness));
        }
    }

    #[test]
    fn a_brightness_reply_for_another_storage_or_led_is_not_read(
        storage in any::<u8>(),
        led in any::<u8>(),
        level in any::<u8>(),
    ) {
        let ours = storage == 0x01 && led == BLADE_18.brightness_leds[0];
        let read = Brightness::from_reply(&BLADE_18, &[storage, led, level]);
        prop_assert_eq!(read.is_some(), ours);
    }

    #[test]
    fn a_charge_limit_reads_on_or_off_only_for_the_model_percent(value in any::<u8>()) {
        let limit = ChargeLimit { percent: 80 };
        let expected = (value & 0x7F == 80).then_some(value & 0x80 != 0);
        prop_assert_eq!(limit.from_reply(&[value]), expected);
        prop_assert_eq!(limit.from_reply(&[]), None);
    }

    #[test]
    fn a_charge_limit_is_sent_only_from_50_to_80_percent(percent in any::<u8>(), on in any::<bool>()) {
        let limit = ChargeLimit { percent };
        match limit.command(on) {
            Ok(command) => {
                prop_assert!((50..=80).contains(&percent));
                let flag = if on { 0x80 } else { 0 };
                prop_assert_eq!(command.args, vec![flag | percent]);
            }
            Err(error) => {
                prop_assert!(!(50..=80).contains(&percent));
                prop_assert_eq!(error, ChargeLimitError::UnsafePercent(percent));
            }
        }
    }

    #[test]
    fn any_reply_args_read_or_fail_without_a_panic(args in reply_args()) {
        let _ = Brightness::from_reply(&BLADE_18, &args);
        let _ = ChargeLimit { percent: 80 }.from_reply(&args);
        let _ = BLADE_18.logo.map(|logo| logo.from_reply(&args));
        let _ = NativeMode::from_reply(&args, &BLADE_18);
        let _ = PerfMode::from_reply(&args);
        let _ = FanRpm::from_reply(&args);
        let _ = Celsius::cpu_from_reply(&args);
    }
}
