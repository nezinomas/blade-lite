mod common;

use razer_proto::{BLADE_18, Brightness, BrightnessError, ModelProfile, Request, VaporLight};

/// The captured brightness steps: the fixture and the percent Synapse set.
const STEPS: [(&str, u8); 4] = [
    ("13-brightness-0.txt", 0),
    ("14-brightness-1.txt", 1),
    ("15-brightness-50.txt", 50),
    ("16-brightness-100.txt", 100),
];

/// The last two `0303` requests of a fixture: the keyboard, then the vapor light.
fn captured_brightness(fixture: &str) -> Vec<[u8; razer_proto::REPORT_LEN]> {
    let requests: Vec<_> = common::captured_requests()
        .into_iter()
        .filter(|request| request.fixture == fixture && request.bytes[6..8] == [0x03, 0x03])
        .map(|request| request.bytes)
        .collect();
    requests[requests.len() - 2..].to_vec()
}

#[test]
fn each_step_matches_its_golden_bytes() {
    for (fixture, percent) in STEPS {
        let golden = captured_brightness(fixture);
        let brightness = Brightness::new(percent).unwrap();
        let mut commands = brightness.commands(&BLADE_18);
        commands.push(BLADE_18.vapor_light.unwrap().command(brightness, true));

        let built: Vec<_> = commands
            .iter()
            .zip(&golden)
            .map(|(command, golden)| {
                Request::new(golden[1], command.id, &command.args)
                    .unwrap()
                    .to_bytes()
            })
            .collect();

        assert_eq!(commands.len(), 2, "{fixture}");
        assert_eq!(built, golden, "{fixture}");
    }
}

#[test]
fn above_100_percent_cannot_be_built() {
    assert_eq!(Brightness::new(101), Err(BrightnessError::OutOfRange(101)));
}

#[test]
fn one_command_is_built_per_led_of_the_model() {
    let one_led = ModelProfile {
        brightness_leds: &[0x05],
        ..BLADE_18
    };

    let commands = Brightness::new(50).unwrap().commands(&one_led);

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].args, [0x01, 0x05, 128]);
}

#[test]
fn the_vapor_light_gets_the_keyboard_level_while_on() {
    let light = VaporLight { led: 0x26 };

    let command = light.command(Brightness::new(50).unwrap(), true);

    assert_eq!(command.id, 0x0303);
    assert_eq!(command.args, [0x01, 0x26, 128]);
}

#[test]
fn the_vapor_light_gets_level_0_while_off() {
    let light = VaporLight { led: 0x26 };

    let command = light.command(Brightness::new(100).unwrap(), false);

    assert_eq!(command.args, [0x01, 0x26, 0]);
}

#[test]
fn the_query_reads_the_keyboard_led() {
    let query = Brightness::query(&BLADE_18).unwrap();

    assert_eq!(query.id, 0x0383);
    assert_eq!(query.args, [0x01, 0x05, 0x00]);
}

#[test]
fn a_model_without_brightness_has_no_query() {
    let none = ModelProfile {
        brightness_leds: &[],
        ..BLADE_18
    };

    assert_eq!(Brightness::query(&none), None);
    assert_eq!(Brightness::from_reply(&none, &[0x01, 0x05, 0x80]), None);
}

#[test]
fn the_reply_level_reads_back_as_the_percent_set() {
    for percent in 0..=100 {
        let level = Brightness::new(percent).unwrap().level();

        let read = Brightness::from_reply(&BLADE_18, &[0x01, 0x05, level]);

        assert_eq!(read, Some(Brightness::new(percent).unwrap()), "{percent}%");
    }
}

#[test]
fn any_level_reads_as_a_percent_in_range() {
    for level in 0..=u8::MAX {
        let read = Brightness::from_reply(&BLADE_18, &[0x01, 0x05, level]).unwrap();

        assert!(read.percent() <= 100, "level {level}");
    }
}

#[test]
fn a_reply_for_another_led_or_storage_is_none() {
    assert_eq!(Brightness::from_reply(&BLADE_18, &[0x01, 0x26, 0x80]), None);
    assert_eq!(Brightness::from_reply(&BLADE_18, &[0x00, 0x05, 0x80]), None);
    assert_eq!(Brightness::from_reply(&BLADE_18, &[0x01, 0x05]), None);
    assert_eq!(Brightness::from_reply(&BLADE_18, &[]), None);
}

#[test]
fn a_dim_level_is_never_read_as_off() {
    for level in 1..=2 {
        let read = Brightness::from_reply(&BLADE_18, &[0x01, 0x05, level]).unwrap();

        assert_eq!(read.percent(), 1, "level {level}");
    }
}

#[test]
fn the_laptop_steps_read_as_the_nearest_percent() {
    // Fn + F10 / F11 step the level by 15 (read on the laptop).
    let read = |level| {
        Brightness::from_reply(&BLADE_18, &[0x01, 0x05, level])
            .unwrap()
            .percent()
    };

    assert_eq!(read(15), 6);
    assert_eq!(read(30), 12);
    assert_eq!(read(127), 50);
    assert_eq!(read(225), 88);
}
